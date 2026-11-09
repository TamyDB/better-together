//! Lado do convidado: lobby, predição do próprio pet, reconciliação com o Host e frame de
//! render. Puro: recebe eventos de rede e teclas, devolve o que enviar e o que desenhar.

use std::collections::VecDeque;

use dm_protocol::{
    decode, encode, sanitize_name, Animal, Channel, GuestMsg, HostMsg, InputFrame, PetState,
    PlayerId, RejectReason, RosterEntry, Snapshot, PROTOCOL_VERSION,
};
use dm_sim::World;

use super::interp::{Interpolator, INTERP_DELAY_TICKS};
use super::net::{NetIn, Out, PeerId};
use crate::input::keys::{Key, KeyState};

/// O convidado só conversa com o Host, sempre neste peer.
pub const HOST_PEER: PeerId = 0;
/// Inputs ainda não confirmados pelo Host que o convidado guarda (2 s a 60 Hz).
pub const MAX_PENDING: usize = 120;

const GUEST_FALLBACK_NAME: &str = "Convidado";
const ROSTER_FALLBACK_NAME: &str = "Jogador";

#[derive(Debug, Clone, PartialEq)]
pub enum GuestEvent {
    Lobby(Vec<Animal>),
    Welcome {
        me: PlayerId,
        animal: Animal,
        aspect: f32,
    },
    Rejected(RejectReason),
    Roster(Vec<RosterEntry>),
    HostLost,
}

/// Estado da partida, criado no `Welcome`.
struct Playing {
    me: PlayerId,
    /// Sim de predição: só o próprio pet.
    world: World,
    /// Inputs enviados que o Host ainda não confirmou, em ordem de `seq`.
    pending: VecDeque<InputFrame>,
    next_seq: u32,
    interp: Interpolator,
    /// Tick do snapshot mais novo do Host (o do `Welcome` no começo).
    latest_tick: u32,
    /// Ticks locais desde que `latest_tick` chegou.
    ticks_since: u32,
}

impl Playing {
    fn own(&self) -> Option<PetState> {
        self.world
            .snapshot()
            .pets
            .into_iter()
            .find(|p| p.id == self.me)
    }

    /// Volta ao estado do Host e reaplica os inputs que ele ainda não processou.
    fn reconcile(&mut self, host: PetState) {
        let acked = host.last_input_seq;
        self.pending.retain(|f| f.seq > acked);
        // Se o Host já viu um seq além do nosso, os próximos seriam todos descartados.
        self.next_seq = self.next_seq.max(acked.saturating_add(1));
        // `put_pet` não zera o input guardado; zera aqui para que nada além dos pendentes
        // seja aplicado. O pet de predição nunca dorme, e `put_pet` repõe a `anim` do Host.
        self.world.set_asleep(self.me, false);
        self.world.put_pet(host);
        for frame in &self.pending {
            self.world.apply_input(self.me, *frame);
            self.world.step();
        }
    }
}

pub struct GuestClient {
    rejoin_token: u64,
    keys: KeyState,
    playing: Option<Playing>,
    /// Depois de `HostLost` o cliente fica encerrado; reconectar usa um novo cliente com o
    /// mesmo `rejoin_token`.
    lost: bool,
}

impl GuestClient {
    pub fn new(rejoin_token: u64) -> GuestClient {
        GuestClient {
            rejoin_token,
            keys: KeyState::default(),
            playing: None,
            lost: false,
        }
    }

    pub fn rejoin_token(&self) -> u64 {
        self.rejoin_token
    }

    /// `Hello` com o nome e o animal escolhidos no lobby.
    pub fn choose(&mut self, name: &str, animal: Animal) -> Out {
        let hello = GuestMsg::Hello {
            protocol_version: PROTOCOL_VERSION,
            name: sanitize_name(name, GUEST_FALLBACK_NAME),
            desired_animal: animal,
            rejoin_token: self.rejoin_token,
        };
        Out::Send {
            peer: HOST_PEER,
            channel: Channel::Events,
            bytes: encode(&hello),
        }
    }

    pub fn handle(&mut self, ev: NetIn) -> Vec<GuestEvent> {
        if self.lost {
            return Vec::new();
        }
        match ev {
            NetIn::PeerClosed(HOST_PEER) => {
                self.lost = true;
                self.playing = None;
                vec![GuestEvent::HostLost]
            }
            NetIn::Recv {
                peer: HOST_PEER,
                channel,
                bytes,
            } => self.on_recv(channel, &bytes),
            NetIn::PeerOpened(_) | NetIn::PeerClosed(_) | NetIn::Recv { .. } => Vec::new(),
        }
    }

    pub fn set_key(&mut self, key: Key, down: bool) {
        self.keys.set(key, down);
    }

    /// Um tick local: manda o input atual ao Host, avança a predição e devolve o frame de
    /// render (outros pets interpolados + o próprio previsto, ordenados por id). O `tick` do
    /// frame é o tick estimado do Host. Antes do `Welcome` ou depois de `HostLost`: nada.
    pub fn tick(&mut self) -> (Option<Out>, Option<Snapshot>) {
        let Some(p) = self.playing.as_mut() else {
            return (None, None);
        };
        let frame = self.keys.to_input(p.next_seq);
        p.next_seq = p.next_seq.saturating_add(1);
        if p.pending.len() == MAX_PENDING {
            p.pending.pop_front();
        }
        p.pending.push_back(frame);
        p.world.apply_input(p.me, frame);
        p.world.step();
        let out = Out::Send {
            peer: HOST_PEER,
            channel: Channel::Input,
            bytes: encode(&GuestMsg::Input(frame)),
        };

        p.ticks_since = p.ticks_since.saturating_add(1);
        let host_tick = p.latest_tick.saturating_add(p.ticks_since);
        let render_tick = host_tick as f32 - INTERP_DELAY_TICKS as f32;
        let me = p.me;
        let mut pets: Vec<PetState> = p
            .interp
            .sample(render_tick)
            .into_iter()
            .filter(|s| s.id != me)
            .collect();
        pets.extend(p.own());
        pets.sort_by_key(|s| s.id);
        (
            Some(out),
            Some(Snapshot {
                tick: host_tick,
                pets,
            }),
        )
    }

    fn on_recv(&mut self, channel: Channel, bytes: &[u8]) -> Vec<GuestEvent> {
        let msg = match decode::<HostMsg>(bytes) {
            Ok(msg) => msg,
            Err(e) => {
                log::debug!("Host: mensagem descartada ({e})");
                return Vec::new();
            }
        };
        match (msg, channel) {
            (HostMsg::Lobby { taken }, Channel::Events) => vec![GuestEvent::Lobby(taken)],
            (HostMsg::Reject(reason), Channel::Events) => vec![GuestEvent::Rejected(reason)],
            (HostMsg::Roster(roster), Channel::Events) => {
                let roster = roster
                    .into_iter()
                    .map(|e| RosterEntry {
                        name: sanitize_name(&e.name, ROSTER_FALLBACK_NAME),
                        ..e
                    })
                    .collect();
                vec![GuestEvent::Roster(roster)]
            }
            (
                HostMsg::Welcome {
                    player_id,
                    animal,
                    snapshot,
                    aspect,
                },
                Channel::Events,
            ) => self.on_welcome(player_id, animal, snapshot, aspect),
            (HostMsg::Snapshot(snapshot), Channel::State) => {
                self.on_snapshot(snapshot);
                Vec::new()
            }
            (_, channel) => {
                log::debug!("Host: mensagem no canal errado ({channel:?}); ignorada");
                Vec::new()
            }
        }
    }

    fn on_welcome(
        &mut self,
        me: PlayerId,
        animal: Animal,
        mut snapshot: Snapshot,
        aspect: f32,
    ) -> Vec<GuestEvent> {
        if !(aspect.is_finite() && aspect > 0.0) {
            log::debug!("Welcome com aspect inválido ({aspect}); ignorado");
            return Vec::new();
        }
        retain_finite(&mut snapshot);
        let Some(own) = snapshot.pets.iter().find(|p| p.id == me).cloned() else {
            log::debug!("Welcome sem o próprio pet; ignorado");
            return Vec::new();
        };
        // O Host descarta seq ≤ último processado, e numa reconexão o pet mantém o antigo.
        let mut next_seq = own.last_input_seq.saturating_add(1);
        if let Some(old) = &self.playing {
            next_seq = next_seq.max(old.next_seq);
        }
        let mut world = World::new(aspect, 0);
        world.put_pet(own);
        let latest_tick = snapshot.tick;
        let mut interp = Interpolator::new();
        interp.push(snapshot);
        self.playing = Some(Playing {
            me,
            world,
            pending: VecDeque::new(),
            next_seq,
            interp,
            latest_tick,
            ticks_since: 0,
        });
        vec![GuestEvent::Welcome { me, animal, aspect }]
    }

    fn on_snapshot(&mut self, mut snapshot: Snapshot) {
        let Some(p) = self.playing.as_mut() else {
            log::debug!("Snapshot antes do Welcome; ignorado");
            return;
        };
        if snapshot.tick <= p.latest_tick {
            // Fora de ordem: reconciliar com ele desfaria inputs já confirmados.
            return;
        }
        retain_finite(&mut snapshot);
        p.latest_tick = snapshot.tick;
        p.ticks_since = 0;
        if let Some(own) = snapshot.pets.iter().find(|s| s.id == p.me).cloned() {
            p.reconcile(own);
        }
        p.interp.push(snapshot);
    }

    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.playing.as_ref().map_or(0, |p| p.pending.len())
    }

    #[cfg(test)]
    fn predicted(&self) -> Option<PetState> {
        self.playing.as_ref().and_then(Playing::own)
    }
}

/// Descarta pets com posição não finita (NaN/inf), que envenenariam a predição e o render.
fn retain_finite(snapshot: &mut Snapshot) {
    snapshot
        .pets
        .retain(|p| p.pos.x.is_finite() && p.pos.y.is_finite());
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_protocol::{
        decode, encode, AnimState, Channel, Facing, GuestMsg, HostMsg, InputFrame, PetState, Vec2,
        PROTOCOL_VERSION,
    };
    use dm_sim::consts::{DT, WALK_SPEED};
    use dm_sim::World;

    const ASPECT: f32 = 16.0 / 9.0;
    const ME: PlayerId = PlayerId(1);

    fn pet(id: PlayerId, animal: Animal, x: f32, y: f32, seq: u32) -> PetState {
        PetState {
            id,
            animal,
            pos: Vec2 { x, y },
            facing: Facing::Right,
            anim: AnimState::Idle,
            last_input_seq: seq,
        }
    }

    fn from_host(channel: Channel, msg: &HostMsg) -> NetIn {
        NetIn::Recv {
            peer: HOST_PEER,
            channel,
            bytes: encode(msg),
        }
    }

    fn welcome(tick: u32, pets: Vec<PetState>) -> NetIn {
        from_host(
            Channel::Events,
            &HostMsg::Welcome {
                player_id: ME,
                animal: Animal::Capybara,
                snapshot: Snapshot { tick, pets },
                aspect: ASPECT,
            },
        )
    }

    fn snapshot(tick: u32, pets: Vec<PetState>) -> NetIn {
        from_host(Channel::State, &HostMsg::Snapshot(Snapshot { tick, pets }))
    }

    /// Convidado com Welcome: Host (pato) em (0.5, 0.8) e o próprio pet em `(x, y)`.
    fn welcomed(x: f32, y: f32, seq: u32) -> GuestClient {
        let mut g = GuestClient::new(42);
        let evs = g.handle(welcome(
            10,
            vec![
                pet(PlayerId(0), Animal::Duck, 0.5, 0.8, 0),
                pet(ME, Animal::Capybara, x, y, seq),
            ],
        ));
        assert_eq!(evs.len(), 1);
        g
    }

    /// O `InputFrame` de um `Out`, que precisa ir ao Host pelo canal `Input`.
    fn sent_input(out: &Out) -> InputFrame {
        match out {
            Out::Send {
                peer: HOST_PEER,
                channel: Channel::Input,
                bytes,
            } => match decode::<GuestMsg>(bytes).unwrap() {
                GuestMsg::Input(f) => f,
                other => panic!("esperava Input, veio {other:?}"),
            },
            other => panic!("esperava Send em Input, veio {other:?}"),
        }
    }

    fn own(frame: &Snapshot) -> &PetState {
        frame.pets.iter().find(|p| p.id == ME).expect("próprio pet")
    }

    #[test]
    fn lobby_and_welcome_events() {
        let mut g = GuestClient::new(42);
        assert_eq!(g.rejoin_token(), 42);
        match g.choose("Ana", Animal::Capybara) {
            Out::Send {
                peer: HOST_PEER,
                channel: Channel::Events,
                bytes,
            } => assert_eq!(
                decode::<GuestMsg>(&bytes).unwrap(),
                GuestMsg::Hello {
                    protocol_version: PROTOCOL_VERSION,
                    name: "Ana".into(),
                    desired_animal: Animal::Capybara,
                    rejoin_token: 42,
                }
            ),
            other => panic!("esperava Hello em Events, veio {other:?}"),
        }

        let lobby = HostMsg::Lobby {
            taken: vec![Animal::Duck],
        };
        assert_eq!(
            g.handle(from_host(Channel::Events, &lobby)),
            vec![GuestEvent::Lobby(vec![Animal::Duck])]
        );
        assert_eq!(
            g.handle(from_host(
                Channel::Events,
                &HostMsg::Reject(RejectReason::AnimalTaken)
            )),
            vec![GuestEvent::Rejected(RejectReason::AnimalTaken)]
        );
        assert_eq!(
            g.handle(welcome(10, vec![pet(ME, Animal::Capybara, 0.4, 0.8, 0)])),
            vec![GuestEvent::Welcome {
                me: ME,
                animal: Animal::Capybara,
                aspect: ASPECT,
            }]
        );
        let roster = vec![RosterEntry {
            id: ME,
            name: "Ana".into(),
            animal: Animal::Capybara,
            asleep: false,
        }];
        assert_eq!(
            g.handle(from_host(Channel::Events, &HostMsg::Roster(roster.clone()))),
            vec![GuestEvent::Roster(roster)]
        );
    }

    #[test]
    fn garbage_and_wrong_peer_or_channel_ignored() {
        let mut g = GuestClient::new(42);
        let lobby = encode(&HostMsg::Lobby { taken: vec![] });
        let cases = [
            NetIn::Recv {
                peer: HOST_PEER,
                channel: Channel::Events,
                bytes: vec![0xFF; 10],
            },
            NetIn::Recv {
                peer: 9,
                channel: Channel::Events,
                bytes: lobby.clone(),
            },
            NetIn::Recv {
                peer: HOST_PEER,
                channel: Channel::State,
                bytes: lobby,
            },
            NetIn::PeerOpened(HOST_PEER),
            NetIn::PeerClosed(9),
        ];
        for ev in cases {
            assert!(g.handle(ev).is_empty());
        }
        // Welcome sem o próprio pet não começa a partida.
        assert!(g
            .handle(welcome(
                0,
                vec![pet(PlayerId(0), Animal::Duck, 0.5, 0.5, 0)]
            ))
            .is_empty());
        assert_eq!(g.tick(), (None, None));
    }

    #[test]
    fn no_input_before_welcome() {
        let mut g = GuestClient::new(42);
        g.set_key(Key::D, true);
        assert_eq!(g.tick(), (None, None));
    }

    #[test]
    fn prediction_is_immediate() {
        let mut g = welcomed(0.5, 0.5, 0);
        g.set_key(Key::D, true);
        let (out, frame) = g.tick();
        let f = sent_input(&out.expect("input"));
        assert_eq!((f.seq, f.move_x, f.move_y, f.run), (1, 1, 0, false));
        let frame = frame.expect("frame");
        assert!(own(&frame).pos.x > 0.5, "{frame:?}");
        // Outros pets também vão no frame, ordenados por id.
        let ids: Vec<PlayerId> = frame.pets.iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![PlayerId(0), ME]);
    }

    #[test]
    fn seq_continues_after_welcome() {
        let mut g = welcomed(0.5, 0.5, 50);
        let (out, _) = g.tick();
        assert_eq!(sent_input(&out.unwrap()).seq, 51);
        let (out, _) = g.tick();
        assert_eq!(sent_input(&out.unwrap()).seq, 52);
    }

    #[test]
    fn reconcile_matches_host() {
        let mut host = World::new(ASPECT, 7);
        host.spawn(PlayerId(0), Animal::Duck, Vec2 { x: 0.5, y: 0.8 })
            .unwrap();
        host.set_autonomous(PlayerId(0), true);
        host.spawn(ME, Animal::Capybara, Vec2 { x: 0.4, y: 0.8 })
            .unwrap();
        let mut g = GuestClient::new(42);
        g.handle(from_host(
            Channel::Events,
            &HostMsg::Welcome {
                player_id: ME,
                animal: Animal::Capybara,
                snapshot: host.snapshot(),
                aspect: host.aspect(),
            },
        ));

        // Snapshots chegam 2 ticks atrasados: a reconciliação reaplica pendentes.
        let mut in_flight: Vec<(u32, Snapshot)> = Vec::new();
        g.set_key(Key::D, true);
        for n in 1..=30u32 {
            if n == 15 {
                g.set_key(Key::D, false);
                g.set_key(Key::S, true);
                g.set_key(Key::Shift, true);
            }
            let (out, _) = g.tick();
            host.apply_input(ME, sent_input(&out.unwrap()));
            host.step();
            if n % 3 == 0 {
                in_flight.push((n + 2, host.snapshot()));
            }
            for (_, s) in in_flight.extract_if(.., |(at, _)| *at == n) {
                g.handle(from_host(Channel::State, &HostMsg::Snapshot(s)));
            }
        }
        let h = host
            .snapshot()
            .pets
            .into_iter()
            .find(|p| p.id == ME)
            .unwrap();
        let p = g.predicted().unwrap();
        assert!(h.pos.x > 0.41 && h.pos.y > 0.85, "o pet andou: {h:?}");
        assert!((p.pos.x - h.pos.x).abs() < 1e-5, "{p:?} vs {h:?}");
        assert!((p.pos.y - h.pos.y).abs() < 1e-5, "{p:?} vs {h:?}");
    }

    #[test]
    fn reconcile_corrects_divergence() {
        let mut g = welcomed(0.5, 0.5, 0);
        g.set_key(Key::D, true);
        for _ in 0..5 {
            g.tick();
        }
        assert_eq!(g.pending_len(), 5);
        g.handle(snapshot(11, vec![pet(ME, Animal::Capybara, 0.1, 0.1, 2)]));
        assert_eq!(g.pending_len(), 3);

        // Mesmo caminho numa sim de referência: (0.1, 0.1) + os inputs 3, 4 e 5.
        let mut reference = World::new(ASPECT, 0);
        reference.put_pet(pet(ME, Animal::Capybara, 0.1, 0.1, 2));
        for seq in 3..=5 {
            reference.apply_input(
                ME,
                InputFrame {
                    seq,
                    move_x: 1,
                    move_y: 0,
                    run: false,
                },
            );
            reference.step();
        }
        let expected = reference.snapshot().pets[0].pos;
        let p = g.predicted().unwrap();
        assert_eq!(p.pos, expected);
        assert!((p.pos.x - (0.1 + 3.0 * WALK_SPEED * DT / ASPECT)).abs() < 1e-6);
        assert_eq!(p.pos.y, 0.1);
        assert_eq!(p.last_input_seq, 5);
    }

    #[test]
    fn stored_input_does_not_leak_after_reconcile() {
        let mut g = welcomed(0.5, 0.5, 0);
        g.set_key(Key::D, true);
        g.tick();
        // Host (bugado) confirma o maior seq possível: o próximo input é recusado pela sim, e
        // o input guardado antes do reconcile não pode mover o pet.
        g.handle(snapshot(
            11,
            vec![pet(ME, Animal::Capybara, 0.3, 0.3, u32::MAX)],
        ));
        assert_eq!(g.pending_len(), 0);
        g.tick();
        assert_eq!(g.predicted().unwrap().pos, Vec2 { x: 0.3, y: 0.3 });
    }

    #[test]
    fn stale_snapshot_ignored() {
        let mut g = welcomed(0.5, 0.5, 0);
        g.handle(snapshot(20, vec![pet(ME, Animal::Capybara, 0.3, 0.3, 0)]));
        g.handle(snapshot(15, vec![pet(ME, Animal::Capybara, 0.9, 0.9, 0)]));
        assert_eq!(g.predicted().unwrap().pos, Vec2 { x: 0.3, y: 0.3 });
    }

    #[test]
    fn non_finite_pets_discarded() {
        let mut g = welcomed(0.5, 0.5, 0);
        g.handle(snapshot(
            20,
            vec![
                pet(PlayerId(0), Animal::Duck, f32::INFINITY, 0.5, 0),
                pet(ME, Animal::Capybara, f32::NAN, 0.3, 0),
                pet(PlayerId(2), Animal::Horse, 0.2, 0.2, 0),
            ],
        ));
        assert_eq!(g.predicted().unwrap().pos, Vec2 { x: 0.5, y: 0.5 });
        // Bem depois do snapshot 20, o render fixa nele: o pato (inf) some; o cavalo aparece.
        let mut frame = None;
        for _ in 0..10 {
            frame = g.tick().1;
        }
        let frame = frame.unwrap();
        assert!(frame
            .pets
            .iter()
            .all(|p| p.pos.x.is_finite() && p.pos.y.is_finite()));
        let ids: Vec<PlayerId> = frame.pets.iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![ME, PlayerId(2)]);
    }

    #[test]
    fn host_lost_stops_ticking() {
        let mut g = welcomed(0.5, 0.5, 0);
        assert!(g.tick().0.is_some());
        assert_eq!(
            g.handle(NetIn::PeerClosed(HOST_PEER)),
            vec![GuestEvent::HostLost]
        );
        assert_eq!(g.tick(), (None, None));
        // Encerrado: nada mais tem efeito.
        assert!(g.handle(NetIn::PeerClosed(HOST_PEER)).is_empty());
        assert!(g
            .handle(welcome(30, vec![pet(ME, Animal::Capybara, 0.5, 0.5, 0)]))
            .is_empty());
        assert_eq!(g.tick(), (None, None));
        assert_eq!(g.rejoin_token(), 42);
    }

    #[test]
    fn pending_capped() {
        let mut g = welcomed(0.5, 0.5, 0);
        g.set_key(Key::A, true);
        for _ in 0..500 {
            g.tick();
        }
        assert!(g.pending_len() <= MAX_PENDING, "{}", g.pending_len());
        assert_eq!(g.pending_len(), MAX_PENDING);
    }
}
