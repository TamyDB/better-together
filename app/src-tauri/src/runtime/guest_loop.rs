//! Thread do convidado: roda o `GuestClient` a 60 Hz e publica o que ele produz.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Instant;

use dm_protocol::Animal;
use tauri::AppHandle;

use super::backlog::{RecvBacklog, MAX_COMMANDS_PER_PASS};
use super::events::{emit_to_ui, guest_event, out_event, snapshot_event, UiEvent};
use super::ticker::{run_tick_loop, TickLoop};
use crate::input::keys::Key;
use crate::session::guest::GuestClient;
use crate::session::net::NetIn;

pub enum GuestCommand {
    Key(Key, bool),
    Choose { name: String, animal: Animal },
    Net(NetIn),
}

/// Ponta de envio para a thread do convidado. Soltá-la fecha o canal e encerra a thread.
pub struct GuestHandle {
    tx: Sender<GuestCommand>,
    backlog: RecvBacklog,
}

impl GuestHandle {
    pub(crate) fn from_parts(tx: Sender<GuestCommand>, backlog: RecvBacklog) -> GuestHandle {
        GuestHandle { tx, backlog }
    }

    /// Repassa um comando ao loop. Mensagens de rede acima do limite da fila são descartadas.
    pub fn send(&self, cmd: GuestCommand) {
        if matches!(cmd, GuestCommand::Net(NetIn::Recv { .. })) && !self.backlog.try_acquire() {
            log::debug!("fila de rede do convidado cheia; mensagem descartada");
            return;
        }
        let _ = self.tx.send(cmd);
    }
}

struct GuestLoop<E> {
    rx: Receiver<GuestCommand>,
    backlog: RecvBacklog,
    client: GuestClient,
    emit: E,
}

impl<E: FnMut(UiEvent)> GuestLoop<E> {
    fn apply(&mut self, cmd: GuestCommand) {
        match cmd {
            GuestCommand::Key(key, down) => self.client.set_key(key, down),
            GuestCommand::Choose { name, animal } => {
                let out = self.client.choose(&name, animal);
                (self.emit)(out_event(out));
            }
            GuestCommand::Net(ev) => {
                if matches!(ev, NetIn::Recv { .. }) {
                    self.backlog.release();
                }
                for event in self.client.handle(ev) {
                    (self.emit)(guest_event(event));
                }
            }
        }
    }
}

impl<E: FnMut(UiEvent)> TickLoop for GuestLoop<E> {
    fn drain(&mut self, _now: Instant) -> bool {
        for _ in 0..MAX_COMMANDS_PER_PASS {
            match self.rx.try_recv() {
                Ok(cmd) => self.apply(cmd),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
        true
    }

    fn run_ticks(&mut self, ticks: u32, _now: Instant) {
        let mut outs = Vec::new();
        let mut frame = None;
        for _ in 0..ticks {
            let (out, snapshot) = self.client.tick();
            outs.extend(out);
            if snapshot.is_some() {
                frame = snapshot;
            }
        }
        if let Some(frame) = &frame {
            (self.emit)(snapshot_event(frame));
        }
        for out in outs {
            (self.emit)(out_event(out));
        }
    }
}

/// Inicia a thread do convidado: emite o frame de render, os inputs e os eventos da sessão.
pub fn spawn_guest_loop(app: AppHandle, client: GuestClient) -> GuestHandle {
    let (tx, rx) = mpsc::channel();
    let backlog = RecvBacklog::default();
    let mut body = GuestLoop {
        rx,
        backlog: backlog.clone(),
        client,
        emit: move |ev| emit_to_ui(&app, ev),
    };
    thread::Builder::new()
        .name("guest-sim".into())
        .spawn(move || run_tick_loop(&mut body))
        .expect("falha ao criar a thread do convidado");
    GuestHandle::from_parts(tx, backlog)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::backlog::MAX_QUEUED_RECV;
    use crate::session::guest::HOST_PEER;
    use dm_protocol::{
        encode, AnimState, Channel, Facing, HostMsg, PetState, PlayerId, RejectReason, Snapshot,
        Vec2,
    };
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<UiEvent>>>;

    fn guest_loop() -> (GuestHandle, GuestLoop<impl FnMut(UiEvent)>, Events) {
        let (tx, rx) = mpsc::channel();
        let backlog = RecvBacklog::default();
        let events: Events = Rc::default();
        let sink = events.clone();
        let body = GuestLoop {
            rx,
            backlog: backlog.clone(),
            client: GuestClient::new(42),
            emit: move |ev| sink.borrow_mut().push(ev),
        };
        (GuestHandle::from_parts(tx, backlog), body, events)
    }

    fn from_host(msg: &HostMsg) -> GuestCommand {
        GuestCommand::Net(NetIn::Recv {
            peer: HOST_PEER,
            channel: Channel::Events,
            bytes: encode(msg),
        })
    }

    fn welcome() -> HostMsg {
        HostMsg::Welcome {
            player_id: PlayerId(1),
            animal: Animal::Capybara,
            snapshot: Snapshot {
                tick: 10,
                pets: vec![PetState {
                    id: PlayerId(1),
                    animal: Animal::Capybara,
                    pos: Vec2 { x: 0.5, y: 0.5 },
                    facing: Facing::Right,
                    anim: AnimState::Idle,
                    last_input_seq: 0,
                }],
            },
            aspect: 2.0,
        }
    }

    #[test]
    fn choose_sends_hello_on_events() {
        let (handle, mut body, events) = guest_loop();
        handle.send(GuestCommand::Choose {
            name: "Ana".into(),
            animal: Animal::Horse,
        });
        assert!(body.drain(Instant::now()));
        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "net://send");
        assert_eq!(events[0].1["peer"], json!(0));
        assert_eq!(events[0].1["channel"], json!("events"));
    }

    #[test]
    fn host_messages_become_session_events() {
        let (handle, mut body, events) = guest_loop();
        handle.send(from_host(&HostMsg::Lobby {
            taken: vec![Animal::Duck],
        }));
        handle.send(from_host(&HostMsg::Reject(RejectReason::RoomFull)));
        handle.send(from_host(&welcome()));
        handle.send(GuestCommand::Net(NetIn::PeerClosed(HOST_PEER)));
        assert!(body.drain(Instant::now()));
        let events = events.borrow();
        let names: Vec<&str> = events.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "session://lobby",
                "session://rejected",
                "session://welcome",
                "session://host-lost",
            ]
        );
        assert_eq!(events[0].1, json!({ "taken": ["duck"] }));
        assert_eq!(events[1].1, json!({ "reason": "room_full" }));
    }

    #[test]
    fn nothing_rendered_before_welcome() {
        let (_handle, mut body, events) = guest_loop();
        body.run_ticks(3, Instant::now());
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn ticks_emit_one_frame_and_one_input_per_tick() {
        let (handle, mut body, events) = guest_loop();
        handle.send(from_host(&welcome()));
        handle.send(GuestCommand::Key(Key::D, true));
        let now = Instant::now();
        assert!(body.drain(now));
        events.borrow_mut().clear();

        body.run_ticks(2, now);
        let events = events.borrow();
        let names: Vec<&str> = events.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["sim://snapshot", "net://send", "net://send"]);
        assert_eq!(events[1].1["channel"], json!("input"));
    }

    #[test]
    fn garbage_from_host_is_ignored() {
        let (handle, mut body, events) = guest_loop();
        handle.send(GuestCommand::Net(NetIn::Recv {
            peer: HOST_PEER,
            channel: Channel::Events,
            bytes: vec![0xFF; 64],
        }));
        assert!(body.drain(Instant::now()));
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn recv_flood_is_capped_but_close_passes() {
        let (handle, mut body, events) = guest_loop();
        for _ in 0..MAX_QUEUED_RECV + 10 {
            handle.send(GuestCommand::Net(NetIn::Recv {
                peer: HOST_PEER,
                channel: Channel::Events,
                bytes: vec![0xFF],
            }));
        }
        handle.send(GuestCommand::Net(NetIn::PeerClosed(HOST_PEER)));
        assert!(body.drain(Instant::now()));
        assert_eq!(events.borrow()[0].0, "session://host-lost");
    }

    #[test]
    fn drain_reports_closed_channel() {
        let (handle, mut body, _events) = guest_loop();
        drop(handle);
        assert!(!body.drain(Instant::now()));
    }
}
