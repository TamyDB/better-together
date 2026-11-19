//! Lado do convidado: lobby, predição do próprio pet, reconciliação com o Host e frame de
//! render. Puro: recebe eventos de rede e teclas, devolve o que enviar e o que desenhar.

use std::collections::VecDeque;

use dm_protocol::{
    decode, encode, sanitize_chat, sanitize_name, Animal, Channel, Emote, GuestMsg, HostMsg,
    InputFrame, PetState, PlayerId, RejectReason, RosterEntry, Snapshot, PROTOCOL_VERSION,
};
use dm_sim::World;

use super::interp::{Interpolator, INTERP_DELAY_TICKS};
use super::net::{NetIn, Out, PeerId};
use crate::input::keys::{EmoteKeys, Key, KeyState};

/// O convidado só conversa com o Host, sempre neste peer.
pub const HOST_PEER: PeerId = 0;
/// Inputs ainda não confirmados pelo Host que o convidado guarda (2 s a 60 Hz).
pub const MAX_PENDING: usize = 120;
/// Ticks sem snapshot novo até o convidado dar o Host por perdido (10 s a 60 Hz).
pub const HOST_SILENCE_TICKS: u32 = 600;
/// Mensagens de `events` à espera de `take_outgoing`. O loop drena a cada comando; o teto
/// só protege contra quem não drenar. O Host limita os emotes de qualquer forma.
const MAX_OUTGOING: usize = 32;

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
    Chat {
        from: PlayerId,
        text: String,
    },
    Emote {
        from: PlayerId,
        emote: Emote,
    },
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
    emote_keys: EmoteKeys,
    playing: Option<Playing>,
    /// Depois de `HostLost` o cliente fica encerrado; reconectar usa um novo cliente com o
    /// mesmo `rejoin_token`.
    lost: bool,
    /// O watchdog encerrou o cliente e `take_lost_event` ainda não entregou o `HostLost`.
    silence_lost: bool,
    /// Emotes das teclas `1`–`6`, à espera de `take_outgoing`.
    outgoing: Vec<Out>,
}

impl GuestClient {
    pub fn new(rejoin_token: u64) -> GuestClient {
        GuestClient {
            rejoin_token,
            keys: KeyState::default(),
            emote_keys: EmoteKeys::default(),
            playing: None,
            lost: false,
            silence_lost: false,
            outgoing: Vec::new(),
        }
    }

    #[cfg(test)]
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
        to_host(&hello)
    }

    pub fn handle(&mut self, ev: NetIn) -> Vec<GuestEvent> {
        if self.lost {
            return Vec::new();
        }
        match ev {
            NetIn::PeerClosed(HOST_PEER) => {
                self.lost = true;
                self.playing = None;
                self.outgoing.clear();
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

    /// Teclas de movimento entram no input do próximo `tick`. Key-down de emote enfileira um
    /// `GuestMsg::Emote` para `take_outgoing`, só na borda (o autorepeat não reenvia; ver
    /// `EmoteKeys`); key-up de emote só solta a tecla. Antes do `Welcome` ou depois de
    /// `HostLost`, emotes são descartados.
    pub fn set_key(&mut self, key: Key, down: bool) {
        match key {
            Key::Emote(emote) => {
                let fresh = self.emote_keys.press(emote, down);
                if fresh && self.playing.is_some() && self.outgoing.len() < MAX_OUTGOING {
                    self.outgoing.push(to_host(&GuestMsg::Emote(emote)));
                }
            }
            _ => self.keys.set(key, down),
        }
    }

    /// Mensagens de `events` geradas por `set_key` (emotes), em ordem. O loop chama depois
    /// de cada comando, para o emote não esperar o próximo tick.
    pub fn take_outgoing(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.outgoing)
    }

    /// Mensagem de chat para o Host, já limpa. Só depois do `Welcome`; texto vazio depois de
    /// limpo → `None`. Sem limite local: o Host decide, e a mensagem só aparece no eco dele.
    pub fn chat(&mut self, text: &str) -> Option<Out> {
        self.playing.as_ref()?;
        let text = sanitize_chat(text)?;
        Some(to_host(&GuestMsg::Chat { text }))
    }

    /// `HostLost` decidido pelo watchdog de silêncio em `tick`; sai uma única vez.
    pub fn take_lost_event(&mut self) -> Option<GuestEvent> {
        std::mem::take(&mut self.silence_lost).then_some(GuestEvent::HostLost)
    }

    /// Um tick local: manda o input atual ao Host, avança a predição e devolve o frame de
    /// render (outros pets interpolados + o próprio previsto, ordenados por id). O `tick` do
    /// frame é o tick estimado do Host. Antes do `Welcome` ou depois de `HostLost`: nada.
    /// Sem snapshot novo por `HOST_SILENCE_TICKS`, encerra como no `PeerClosed` e deixa o
    /// `HostLost` em `take_lost_event`.
    pub fn tick(&mut self) -> (Option<Out>, Option<Snapshot>) {
        let Some(p) = self.playing.as_mut() else {
            return (None, None);
        };
        p.ticks_since = p.ticks_since.saturating_add(1);
        if p.ticks_since >= HOST_SILENCE_TICKS {
            self.lost = true;
            self.silence_lost = true;
            self.playing = None;
            self.outgoing.clear();
            return (None, None);
        }
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
            (HostMsg::Chat { .. } | HostMsg::Emote { .. }, Channel::Events)
                if self.playing.is_none() =>
            {
                log::debug!("Host: chat/emote antes do Welcome; ignorado");
                Vec::new()
            }
            // O Host também não é confiável: o texto é limpo de novo antes de chegar à UI.
            (HostMsg::Chat { from, text }, Channel::Events) => match sanitize_chat(&text) {
                Some(text) => vec![GuestEvent::Chat { from, text }],
                None => {
                    log::debug!("Host: chat vazio depois de limpo; ignorado");
                    Vec::new()
                }
            },
            (HostMsg::Emote { from, emote }, Channel::Events) => {
                vec![GuestEvent::Emote { from, emote }]
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
        retain_on_screen(&mut snapshot);
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
        retain_on_screen(&mut snapshot);
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

/// Mensagem ao Host pelo canal `events` (confiável e ordenado).
fn to_host(msg: &GuestMsg) -> Out {
    Out::Send {
        peer: HOST_PEER,
        channel: Channel::Events,
        bytes: encode(msg),
    }
}

/// Descarta pets fora de `[0, 1]`, o que inclui NaN e infinitos. Valores finitos gigantes
/// também não passam: o `lerp` os transformaria em NaN e envenenaria a predição e o render.
fn retain_on_screen(snapshot: &mut Snapshot) {
    let valid = |v: f32| (0.0..=1.0).contains(&v);
    snapshot.pets.retain(|p| valid(p.pos.x) && valid(p.pos.y));
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_protocol::{
        decode, encode, AnimState, Channel, Emote, Facing, GuestMsg, HostMsg, InputFrame, PetState,
        Vec2, MAX_CHAT_CHARS, PROTOCOL_VERSION,
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
    fn out_of_range_pets_discarded() {
        let mut g = welcomed(0.5, 0.5, 0);
        g.handle(snapshot(
            20,
            vec![
                // `±f32::MAX` são finitos, mas o `lerp` os transforma em NaN.
                pet(PlayerId(0), Animal::Duck, f32::MAX, 0.5, 0),
                pet(ME, Animal::Capybara, -f32::MAX, 0.3, 0),
                pet(PlayerId(2), Animal::Horse, 0.5, 1.0001, 0),
                pet(PlayerId(3), Animal::Chicken, 0.0, 1.0, 0),
            ],
        ));
        assert_eq!(g.predicted().unwrap().pos, Vec2 { x: 0.5, y: 0.5 });
        let mut frame = None;
        for _ in 0..10 {
            frame = g.tick().1;
        }
        let ids: Vec<PlayerId> = frame.unwrap().pets.iter().map(|p| p.id).collect();
        assert_eq!(ids, vec![ME, PlayerId(3)]);
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
    fn host_silence_loses_host_once() {
        let mut g = welcomed(0.5, 0.5, 0);
        for _ in 1..HOST_SILENCE_TICKS {
            assert!(g.tick().0.is_some());
            assert_eq!(g.take_lost_event(), None);
        }
        assert_eq!(g.tick(), (None, None));
        assert_eq!(g.take_lost_event(), Some(GuestEvent::HostLost));
        assert_eq!(g.take_lost_event(), None);
        assert_eq!(g.tick(), (None, None));
        assert_eq!(g.take_lost_event(), None);
        // Já encerrado: o canal fechando depois não repete o evento.
        assert!(g.handle(NetIn::PeerClosed(HOST_PEER)).is_empty());
    }

    #[test]
    fn steady_snapshots_keep_host_alive() {
        let mut g = welcomed(0.5, 0.5, 0);
        let mut tick = 10;
        for i in 1..=1200 {
            assert!(g.tick().0.is_some());
            if i % 3 == 0 {
                tick += 3;
                g.handle(snapshot(tick, vec![pet(ME, Animal::Capybara, 0.5, 0.5, 0)]));
            }
            assert_eq!(g.take_lost_event(), None);
        }
    }

    #[test]
    fn stale_or_rejected_snapshot_does_not_reset_silence() {
        let mut g = welcomed(0.5, 0.5, 0);
        for _ in 1..HOST_SILENCE_TICKS {
            g.tick();
            // Antigo (o `Welcome` foi no tick 10), lixo e canal errado não contam como sinal de vida.
            g.handle(snapshot(5, vec![pet(ME, Animal::Capybara, 0.5, 0.5, 0)]));
            g.handle(NetIn::Recv {
                peer: HOST_PEER,
                channel: Channel::State,
                bytes: vec![0xFF; 8],
            });
            g.handle(from_host(
                Channel::Events,
                &HostMsg::Snapshot(Snapshot {
                    tick: 999,
                    pets: vec![pet(ME, Animal::Capybara, 0.5, 0.5, 0)],
                }),
            ));
            assert_eq!(g.take_lost_event(), None);
        }
        assert_eq!(g.tick(), (None, None));
        assert_eq!(g.take_lost_event(), Some(GuestEvent::HostLost));
    }

    /// A `GuestMsg` de um `Out`, que precisa ir ao Host pelo canal `Events`.
    fn sent_event(out: &Out) -> GuestMsg {
        match out {
            Out::Send {
                peer: HOST_PEER,
                channel: Channel::Events,
                bytes,
            } => decode::<GuestMsg>(bytes).unwrap(),
            other => panic!("esperava Send em Events, veio {other:?}"),
        }
    }

    #[test]
    fn chat_before_welcome_is_none() {
        let mut g = GuestClient::new(42);
        assert_eq!(g.chat("oi"), None);
        g.handle(from_host(
            Channel::Events,
            &HostMsg::Lobby { taken: vec![] },
        ));
        assert_eq!(g.chat("oi"), None);
    }

    #[test]
    fn chat_after_welcome_sends_on_events() {
        let mut g = welcomed(0.5, 0.5, 0);
        let out = g.chat("  oi\u{202E}   gente ").expect("chat");
        assert_eq!(
            sent_event(&out),
            GuestMsg::Chat {
                text: "oi gente".into()
            }
        );
        // Sem limite local: o Host decide.
        for _ in 0..20 {
            assert!(g.chat("de novo").is_some());
        }
        // Encerrado: nada sai.
        g.handle(NetIn::PeerClosed(HOST_PEER));
        assert_eq!(g.chat("oi"), None);
    }

    #[test]
    fn chat_blank_is_none() {
        let mut g = welcomed(0.5, 0.5, 0);
        assert_eq!(g.chat(""), None);
        assert_eq!(g.chat("   "), None);
        assert_eq!(g.chat("\u{200B}\u{202E}\n"), None);
    }

    #[test]
    fn emote_key_sends_once() {
        let mut g = welcomed(0.5, 0.5, 0);
        let emote = Key::from_code("Digit2").unwrap();
        g.set_key(emote, true);
        let outs = g.take_outgoing();
        assert_eq!(outs.len(), 1);
        assert_eq!(sent_event(&outs[0]), GuestMsg::Emote(Emote::Laugh));
        assert!(g.take_outgoing().is_empty());
        // Key-up não manda nada.
        g.set_key(emote, false);
        assert!(g.take_outgoing().is_empty());
        // Só na borda: o autorepeat (key-downs sem key-up) não reenvia.
        for _ in 0..3 {
            g.set_key(emote, true);
        }
        let outs = g.take_outgoing();
        assert_eq!(outs.len(), 1);
        assert_eq!(sent_event(&outs[0]), GuestMsg::Emote(Emote::Laugh));
        // Segurando o `2`, o `3` é outra tecla e sai.
        g.set_key(Key::Emote(Emote::Wow), true);
        g.set_key(emote, true);
        let outs = g.take_outgoing();
        assert_eq!(outs.len(), 1);
        assert_eq!(sent_event(&outs[0]), GuestMsg::Emote(Emote::Wow));
        // Solta e aperta de novo: outro emote; quem limita é o Host.
        g.set_key(emote, false);
        g.set_key(emote, true);
        assert_eq!(g.take_outgoing().len(), 1);
        // O emote não mexe no pet: o input do tick continua parado.
        let (out, _) = g.tick();
        let f = sent_input(&out.unwrap());
        assert_eq!((f.move_x, f.move_y, f.run), (0, 0, false));
        assert!(g.take_outgoing().is_empty());
    }

    #[test]
    fn undrained_emotes_are_capped() {
        let mut g = welcomed(0.5, 0.5, 0);
        for _ in 0..1000 {
            g.set_key(Key::Emote(Emote::Wow), true);
            g.set_key(Key::Emote(Emote::Wow), false);
        }
        assert_eq!(g.take_outgoing().len(), MAX_OUTGOING);
        g.set_key(Key::Emote(Emote::Wow), true);
        assert_eq!(g.take_outgoing().len(), 1);
    }

    #[test]
    fn emote_key_before_welcome_or_after_loss_sends_nothing() {
        let mut g = GuestClient::new(42);
        g.set_key(Key::Emote(Emote::Heart), true);
        assert!(g.take_outgoing().is_empty());

        let mut g = welcomed(0.5, 0.5, 0);
        g.set_key(Key::Emote(Emote::Heart), true);
        // O Host caiu antes do loop drenar: o emote pendente não sai mais.
        g.handle(NetIn::PeerClosed(HOST_PEER));
        assert!(g.take_outgoing().is_empty());
        g.set_key(Key::Emote(Emote::Heart), false);
        g.set_key(Key::Emote(Emote::Heart), true);
        assert!(g.take_outgoing().is_empty());
    }

    #[test]
    fn host_chat_becomes_event() {
        let mut g = welcomed(0.5, 0.5, 0);
        let chat = |text: &str| HostMsg::Chat {
            from: PlayerId(2),
            text: text.into(),
        };
        assert_eq!(
            g.handle(from_host(Channel::Events, &chat("oi\u{202E}lá"))),
            vec![GuestEvent::Chat {
                from: PlayerId(2),
                text: "oilá".into()
            }]
        );
        // Host não confiável: texto gigante é cortado; só invisíveis não gera evento.
        match g
            .handle(from_host(Channel::Events, &chat(&"x".repeat(10_000))))
            .as_slice()
        {
            [GuestEvent::Chat { text, .. }] => assert_eq!(text.chars().count(), MAX_CHAT_CHARS),
            other => panic!("esperava um Chat, veio {other:?}"),
        }
        assert!(g
            .handle(from_host(Channel::Events, &chat("\u{200B}\u{202E}  ")))
            .is_empty());
        // Canal errado: ignorado.
        assert!(g.handle(from_host(Channel::State, &chat("oi"))).is_empty());
    }

    #[test]
    fn host_emote_becomes_event() {
        let mut g = welcomed(0.5, 0.5, 0);
        let msg = HostMsg::Emote {
            from: PlayerId(0),
            emote: Emote::Music,
        };
        assert_eq!(
            g.handle(from_host(Channel::Events, &msg)),
            vec![GuestEvent::Emote {
                from: PlayerId(0),
                emote: Emote::Music
            }]
        );
        assert!(g.handle(from_host(Channel::State, &msg)).is_empty());
    }

    #[test]
    fn host_chat_before_welcome_ignored() {
        let mut g = GuestClient::new(42);
        let chat = HostMsg::Chat {
            from: PlayerId(0),
            text: "oi".into(),
        };
        let emote = HostMsg::Emote {
            from: PlayerId(0),
            emote: Emote::Heart,
        };
        assert!(g.handle(from_host(Channel::Events, &chat)).is_empty());
        assert!(g.handle(from_host(Channel::Events, &emote)).is_empty());
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
