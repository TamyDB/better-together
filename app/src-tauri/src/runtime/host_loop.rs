//! Thread da autoridade: roda a `HostSession` a 60 Hz e publica o que ela produz.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use dm_protocol::{InputFrame, RosterEntry};
use tauri::AppHandle;

use super::backlog::{RecvBacklog, MAX_COMMANDS_PER_PASS};
use super::events::{emit_to_ui, out_event, snapshot_event, UiEvent};
use super::ticker::{run_tick_loop, TickLoop};
use crate::session::host::HostSession;
use crate::session::net::{NetIn, Out};

/// Destino de comandos para a simulação; injetável para testes.
pub trait SimSink: Send + Sync {
    fn send_input(&self, frame: InputFrame);
    fn set_autonomous(&self, on: bool);
}

pub enum HostCommand {
    HostInput(InputFrame),
    HostAutonomous(bool),
    Net(NetIn),
}

/// Ponta de envio para a thread do Host. Soltar a última referência fecha o canal e
/// encerra a thread.
pub struct HostHandle {
    tx: Sender<HostCommand>,
    backlog: RecvBacklog,
    /// Último roster publicado: a overlay pode nascer depois do evento.
    roster: Arc<Mutex<Vec<RosterEntry>>>,
}

impl HostHandle {
    #[cfg(test)]
    pub(crate) fn from_parts(tx: Sender<HostCommand>, backlog: RecvBacklog) -> HostHandle {
        HostHandle::with_roster(tx, backlog, Vec::new())
    }

    pub(crate) fn with_roster(
        tx: Sender<HostCommand>,
        backlog: RecvBacklog,
        roster: Vec<RosterEntry>,
    ) -> HostHandle {
        HostHandle {
            tx,
            backlog,
            roster: Arc::new(Mutex::new(roster)),
        }
    }

    /// Cópia do último roster; o lock não é segurado além da cópia.
    pub fn roster(&self) -> Vec<RosterEntry> {
        self.roster.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// Repassa um evento da rede. Mensagens acima do limite da fila são descartadas.
    pub fn net(&self, ev: NetIn) {
        if matches!(ev, NetIn::Recv { .. }) && !self.backlog.try_acquire() {
            log::debug!("fila de rede do Host cheia; mensagem descartada");
            return;
        }
        let _ = self.tx.send(HostCommand::Net(ev));
    }
}

impl SimSink for HostHandle {
    fn send_input(&self, frame: InputFrame) {
        let _ = self.tx.send(HostCommand::HostInput(frame));
    }

    fn set_autonomous(&self, on: bool) {
        let _ = self.tx.send(HostCommand::HostAutonomous(on));
    }
}

struct HostLoop<E> {
    rx: Receiver<HostCommand>,
    backlog: RecvBacklog,
    session: HostSession,
    roster: Arc<Mutex<Vec<RosterEntry>>>,
    emit: E,
}

impl<E: FnMut(UiEvent)> HostLoop<E> {
    fn apply(&mut self, cmd: HostCommand, now: Instant) {
        match cmd {
            HostCommand::HostInput(frame) => self.session.host_input(frame),
            HostCommand::HostAutonomous(on) => self.session.set_host_autonomous(on),
            HostCommand::Net(ev) => {
                if matches!(ev, NetIn::Recv { .. }) {
                    self.backlog.release();
                }
                let outs = self.session.handle(ev, now);
                self.emit_outs(outs);
            }
        }
    }

    fn emit_outs(&mut self, outs: Vec<Out>) {
        for out in outs {
            if let Out::Roster(roster) = &out {
                if let Ok(mut last) = self.roster.lock() {
                    *last = roster.clone();
                }
            }
            (self.emit)(out_event(out));
        }
    }
}

impl<E: FnMut(UiEvent)> TickLoop for HostLoop<E> {
    fn drain(&mut self, now: Instant) -> bool {
        for _ in 0..MAX_COMMANDS_PER_PASS {
            match self.rx.try_recv() {
                Ok(cmd) => self.apply(cmd, now),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
        true
    }

    fn run_ticks(&mut self, ticks: u32, now: Instant) {
        if ticks == 0 {
            return;
        }
        let mut outs = Vec::new();
        for _ in 0..ticks {
            outs.extend(self.session.tick(now));
        }
        (self.emit)(snapshot_event(&self.session.snapshot()));
        self.emit_outs(outs);
    }
}

/// Inicia a thread do Host: emite `sim://snapshot` e os `Out` a cada lote de ticks.
pub fn spawn_host_loop(app: AppHandle, session: HostSession) -> HostHandle {
    let (tx, rx) = mpsc::channel();
    let backlog = RecvBacklog::default();
    let handle = HostHandle::with_roster(tx, backlog.clone(), session.roster());
    let mut body = HostLoop {
        rx,
        backlog,
        roster: handle.roster.clone(),
        session,
        emit: move |ev| emit_to_ui(&app, ev),
    };
    thread::Builder::new()
        .name("host-sim".into())
        .spawn(move || run_tick_loop(&mut body))
        .expect("falha ao criar a thread do Host");
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::backlog::MAX_QUEUED_RECV;
    use dm_protocol::{encode, Animal, Channel, GuestMsg, PROTOCOL_VERSION};
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<UiEvent>>>;

    fn host_loop() -> (HostHandle, HostLoop<impl FnMut(UiEvent)>, Events) {
        let (tx, rx) = mpsc::channel();
        let backlog = RecvBacklog::default();
        let events: Events = Rc::default();
        let sink = events.clone();
        let session = HostSession::new(2.0, 1, "Tamy", Animal::Duck);
        let handle = HostHandle::with_roster(tx, backlog.clone(), session.roster());
        let body = HostLoop {
            rx,
            backlog,
            roster: handle.roster.clone(),
            session,
            emit: move |ev| sink.borrow_mut().push(ev),
        };
        (handle, body, events)
    }

    fn names(events: &Events) -> Vec<&'static str> {
        events.borrow().iter().map(|(n, _)| *n).collect()
    }

    fn hello_recv(peer: u64) -> NetIn {
        NetIn::Recv {
            peer,
            channel: Channel::Events,
            bytes: encode(&GuestMsg::Hello {
                protocol_version: PROTOCOL_VERSION,
                name: "Ana".into(),
                desired_animal: Animal::Capybara,
                rejoin_token: 9,
            }),
        }
    }

    #[test]
    fn peer_opened_sends_lobby_to_webview() {
        let (handle, mut body, events) = host_loop();
        handle.net(NetIn::PeerOpened(7));
        assert!(body.drain(Instant::now()));
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "net://send");
        assert_eq!(events[0].1["peer"], json!(7));
        assert_eq!(events[0].1["channel"], json!("events"));
        assert!(events[0].1["bytes"].is_array());
    }

    #[test]
    fn hello_emits_welcome_and_roster() {
        let (handle, mut body, events) = host_loop();
        handle.net(NetIn::PeerOpened(7));
        handle.net(hello_recv(7));
        assert!(body.drain(Instant::now()));
        let names = names(&events);
        assert!(names.contains(&"session://roster"), "{names:?}");
        assert_eq!(names.iter().filter(|n| **n == "net://send").count(), 3);
        let events = events.borrow();
        let video: Vec<&Value> = events
            .iter()
            .filter(|(n, _)| *n == "net://video")
            .map(|(_, p)| p)
            .collect();
        assert_eq!(video, vec![&json!({ "peer": 7, "on": true })]);
    }

    #[test]
    fn ticks_emit_snapshot_then_outs() {
        let (handle, mut body, events) = host_loop();
        handle.net(NetIn::PeerOpened(7));
        handle.net(hello_recv(7));
        let now = Instant::now();
        assert!(body.drain(now));
        events.borrow_mut().clear();

        // Sem ticks devidos, nada é emitido.
        body.run_ticks(0, now);
        assert!(events.borrow().is_empty());

        body.run_ticks(3, now);
        let events = events.borrow();
        assert_eq!(events[0].0, "sim://snapshot");
        assert_eq!(events[0].1["tick"], json!(3));
        assert_eq!(events[1].0, "net://send");
        assert_eq!(events[1].1["channel"], json!("state"));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn host_commands_reach_session() {
        let (handle, mut body, events) = host_loop();
        handle.send_input(InputFrame {
            seq: 5,
            move_x: 1,
            move_y: 0,
            run: false,
        });
        handle.set_autonomous(false);
        let now = Instant::now();
        assert!(body.drain(now));
        body.run_ticks(1, now);
        let events = events.borrow();
        assert_eq!(events[0].1["pets"][0]["last_input_seq"], json!(5));
    }

    #[test]
    fn garbage_from_peer_is_ignored() {
        let (handle, mut body, events) = host_loop();
        handle.net(NetIn::PeerOpened(7));
        handle.net(NetIn::Recv {
            peer: 7,
            channel: Channel::Events,
            bytes: vec![0xFF; 64],
        });
        handle.net(NetIn::Recv {
            peer: 99,
            channel: Channel::Input,
            bytes: Vec::new(),
        });
        assert!(body.drain(Instant::now()));
        assert_eq!(names(&events), vec!["net://send"]);
    }

    #[test]
    fn recv_flood_is_capped_but_peer_events_pass() {
        let (handle, mut body, events) = host_loop();
        let junk = || NetIn::Recv {
            peer: 1,
            channel: Channel::Input,
            bytes: vec![1],
        };
        for _ in 0..MAX_QUEUED_RECV + 10 {
            handle.net(junk());
        }
        handle.net(NetIn::PeerOpened(7));
        assert!(body.drain(Instant::now()));
        assert_eq!(names(&events), vec!["net://send"]);
        // A fila esvaziou: a vaga volta a existir.
        handle.net(hello_recv(7));
        assert!(body.drain(Instant::now()));
        assert!(names(&events).contains(&"session://roster"));
    }

    #[test]
    fn drain_is_bounded_per_pass() {
        let (tx, rx) = mpsc::channel();
        let events: Events = Rc::default();
        let sink = events.clone();
        let mut body = HostLoop {
            rx,
            backlog: RecvBacklog::default(),
            session: HostSession::new(2.0, 1, "Tamy", Animal::Duck),
            roster: Arc::default(),
            emit: move |ev: UiEvent| sink.borrow_mut().push(ev),
        };
        for peer in 0..MAX_COMMANDS_PER_PASS as u64 + 5 {
            tx.send(HostCommand::Net(NetIn::PeerOpened(peer))).unwrap();
        }
        assert!(body.drain(Instant::now()));
        assert_eq!(events.borrow().len(), MAX_COMMANDS_PER_PASS);
        assert!(body.drain(Instant::now()));
        assert_eq!(events.borrow().len(), MAX_COMMANDS_PER_PASS + 5);
    }

    #[test]
    fn drain_reports_closed_channel() {
        let (handle, mut body, _events) = host_loop();
        drop(handle);
        assert!(!body.drain(Instant::now()));
    }

    #[test]
    fn host_handle_keeps_roster() {
        let (handle, mut body, _events) = host_loop();
        let initial = handle.roster();
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].name, "Tamy");

        handle.net(NetIn::PeerOpened(7));
        handle.net(hello_recv(7));
        assert!(body.drain(Instant::now()));
        let roster = handle.roster();
        assert_eq!(roster.len(), 2);
        assert_eq!(roster[1].name, "Ana");
    }

    #[test]
    fn roster_payload_lists_host_first() {
        let (handle, mut body, events) = host_loop();
        handle.net(NetIn::PeerOpened(7));
        handle.net(hello_recv(7));
        assert!(body.drain(Instant::now()));
        let events = events.borrow();
        let (_, roster): &(&str, Value) = events
            .iter()
            .find(|(n, _)| *n == "session://roster")
            .unwrap();
        assert_eq!(roster[0]["id"], json!(0));
        assert_eq!(roster[1]["name"], json!("Ana"));
    }
}
