//! Autoridade da sessão no Host: lobby, handshake, inputs, snapshots, sono e reconexão.
//! Pura: recebe eventos de rede e o instante atual, devolve o que enviar.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use dm_protocol::{
    check_version, decode, encode, sanitize_chat, sanitize_name, Animal, Channel, Emote, GuestMsg,
    HostMsg, InputFrame, PlayerId, RejectReason, RosterEntry, Snapshot, Vec2,
};
use dm_sim::{SimError, World};

use super::net::{NetIn, Out, PeerId};
use super::rate::{Cooldown, TokenBucket};
use super::HOST_PLAYER;

/// Um `Snapshot` em `State` a cada 3 ticks (60 Hz → 20 Hz).
pub const SNAPSHOT_EVERY_TICKS: u64 = 3;
/// Tempo que um pet dorme esperando o convidado reconectar antes de ser removido.
pub const SLEEP_TIMEOUT: Duration = Duration::from_secs(30);
/// Inputs aceitos por peer em cada janela de 1 s.
pub const MAX_INPUTS_PER_SEC: u32 = 120;
/// Chats seguidos que um jogador pode mandar antes de esperar a recarga.
pub const CHAT_BURST: u32 = 3;
/// Recarga de 1 chat no limite de cada jogador.
pub const CHAT_REFILL: Duration = Duration::from_secs(2);
/// Intervalo mínimo entre dois emotes do mesmo jogador.
pub const EMOTE_COOLDOWN: Duration = Duration::from_secs(1);

const INPUT_WINDOW: Duration = Duration::from_secs(1);
const GUEST_FALLBACK_NAME: &str = "Convidado";
/// `rejoin_token` 0 significa "sem reconexão".
const NO_REJOIN_TOKEN: u64 = 0;

struct Player {
    name: String,
    animal: Animal,
    /// `None` para o Host e para quem entrou com token 0 ou repetido: nunca reconecta.
    rejoin_token: Option<u64>,
    /// Peer que controla o pet; `None` para o Host e para quem está dormindo.
    peer: Option<PeerId>,
    /// Quando o peer caiu; `Some` enquanto o pet dorme.
    asleep_since: Option<Instant>,
    /// Limites de chat e emote: ficam com o jogador (sobrevivem ao rejoin) e somem com ele.
    chat_limit: TokenBucket,
    emote_limit: Cooldown,
}

impl Player {
    fn new(
        name: String,
        animal: Animal,
        rejoin_token: Option<u64>,
        peer: Option<PeerId>,
    ) -> Player {
        Player {
            name,
            animal,
            rejoin_token,
            peer,
            asleep_since: None,
            chat_limit: TokenBucket::new(CHAT_BURST, CHAT_REFILL),
            emote_limit: Cooldown::new(EMOTE_COOLDOWN),
        }
    }

    /// Dorme há mais de `SLEEP_TIMEOUT`: não religa mais e sai no próximo `tick`.
    fn sleep_expired(&self, now: Instant) -> bool {
        self.asleep_since
            .is_some_and(|t| now.saturating_duration_since(t) > SLEEP_TIMEOUT)
    }
}

/// Conexão de um convidado: no lobby (`player == None`) ou jogando.
#[derive(Default)]
struct Peer {
    player: Option<PlayerId>,
    window_start: Option<Instant>,
    window_inputs: u32,
}

impl Peer {
    /// Admitido: recebeu `Welcome` e joga. Só ele recebe snapshots, vídeo, roster e chat.
    fn admitted(&self) -> bool {
        self.player.is_some()
    }

    /// Janela fixa de 1 s ancorada no primeiro input depois que a anterior venceu.
    fn allow_input(&mut self, now: Instant) -> bool {
        match self.window_start {
            Some(start) if now.saturating_duration_since(start) < INPUT_WINDOW => {}
            _ => {
                self.window_start = Some(now);
                self.window_inputs = 0;
            }
        }
        self.window_inputs = self.window_inputs.saturating_add(1);
        self.window_inputs <= MAX_INPUTS_PER_SEC
    }
}

pub struct HostSession {
    world: World,
    players: BTreeMap<PlayerId, Player>,
    peers: BTreeMap<PeerId, Peer>,
    ticks: u64,
}

fn send(peer: PeerId, channel: Channel, msg: &HostMsg) -> Out {
    Out::Send {
        peer,
        channel,
        bytes: encode(msg),
    }
}

impl HostSession {
    pub fn new(aspect: f32, seed: u64, host_name: &str, host_animal: Animal) -> HostSession {
        let mut world = World::new(aspect, seed);
        world
            .spawn(HOST_PLAYER, host_animal, Vec2 { x: 0.5, y: 0.8 })
            .expect("mundo vazio aceita o pet do Host");
        world.set_autonomous(HOST_PLAYER, true);
        let mut players = BTreeMap::new();
        players.insert(
            HOST_PLAYER,
            Player::new(sanitize_name(host_name, "Host"), host_animal, None, None),
        );
        HostSession {
            world,
            players,
            peers: BTreeMap::new(),
            ticks: 0,
        }
    }

    pub fn handle(&mut self, ev: NetIn, now: Instant) -> Vec<Out> {
        match ev {
            NetIn::PeerOpened(peer) => {
                if self.peers.contains_key(&peer) {
                    log::debug!("peer {peer} aberto de novo; ignorado");
                    return Vec::new();
                }
                self.peers.insert(peer, Peer::default());
                vec![send(peer, Channel::Events, &self.lobby_msg())]
            }
            NetIn::PeerClosed(peer) => self.on_closed(peer, now),
            NetIn::Recv {
                peer,
                channel,
                bytes,
            } => self.on_recv(peer, channel, &bytes, now),
        }
    }

    pub fn host_input(&mut self, frame: InputFrame) {
        self.world.apply_input(HOST_PLAYER, frame);
    }

    /// Chat do próprio Host: mesmas regras e limites de um convidado.
    #[cfg_attr(not(test), expect(dead_code, reason = "os comandos da Task 4 chamam"))]
    pub fn host_chat(&mut self, text: &str, now: Instant) -> Vec<Out> {
        self.chat_from(HOST_PLAYER, text, now)
    }

    /// Emote do próprio Host: mesmas regras e limites de um convidado.
    #[cfg_attr(not(test), expect(dead_code, reason = "os comandos da Task 4 chamam"))]
    pub fn host_emote(&mut self, emote: Emote, now: Instant) -> Vec<Out> {
        self.emote_from(HOST_PLAYER, emote, now)
    }

    pub fn set_host_autonomous(&mut self, on: bool) {
        self.world.set_autonomous(HOST_PLAYER, on);
    }

    /// Um passo da simulação. Antes, remove quem dorme há mais de `SLEEP_TIMEOUT`; depois,
    /// a cada `SNAPSHOT_EVERY_TICKS`, manda o `Snapshot` aos peers que já jogam.
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        let expired: Vec<PlayerId> = self
            .players
            .iter()
            .filter(|(_, p)| p.sleep_expired(now))
            .map(|(id, _)| *id)
            .collect();
        if !expired.is_empty() {
            for id in expired {
                self.players.remove(&id);
                self.world.despawn(id);
            }
            out.extend(self.players_changed());
        }

        self.world.step();
        self.ticks += 1;
        if self.ticks.is_multiple_of(SNAPSHOT_EVERY_TICKS) {
            let bytes = encode(&HostMsg::Snapshot(self.world.snapshot()));
            out.extend(self.to_admitted(Channel::State, &bytes));
        }
        out
    }

    pub fn snapshot(&self) -> Snapshot {
        self.world.snapshot()
    }

    pub fn roster(&self) -> Vec<RosterEntry> {
        self.players
            .iter()
            .map(|(id, p)| RosterEntry {
                id: *id,
                name: p.name.clone(),
                animal: p.animal,
                asleep: p.asleep_since.is_some(),
            })
            .collect()
    }

    fn on_recv(&mut self, peer: PeerId, channel: Channel, bytes: &[u8], now: Instant) -> Vec<Out> {
        if !self.peers.contains_key(&peer) {
            log::debug!("mensagem de peer desconhecido {peer}; ignorada");
            return Vec::new();
        }
        let msg = match decode::<GuestMsg>(bytes) {
            Ok(msg) => msg,
            Err(e) => {
                log::debug!("peer {peer}: mensagem descartada ({e})");
                return Vec::new();
            }
        };
        match (msg, channel) {
            (
                GuestMsg::Hello {
                    protocol_version,
                    name,
                    desired_animal,
                    rejoin_token,
                },
                Channel::Events,
            ) => self.on_hello(
                peer,
                protocol_version,
                &name,
                desired_animal,
                rejoin_token,
                now,
            ),
            (GuestMsg::Input(frame), Channel::Input) => {
                self.on_input(peer, frame, now);
                Vec::new()
            }
            (GuestMsg::Chat { text }, Channel::Events) => match self.admitted_player(peer) {
                Some(id) => self.chat_from(id, &text, now),
                None => Vec::new(),
            },
            (GuestMsg::Emote(emote), Channel::Events) => match self.admitted_player(peer) {
                Some(id) => self.emote_from(id, emote, now),
                None => Vec::new(),
            },
            (_, channel) => {
                log::debug!("peer {peer}: mensagem no canal errado ({channel:?}); ignorada");
                Vec::new()
            }
        }
    }

    fn on_hello(
        &mut self,
        peer: PeerId,
        version: u16,
        name: &str,
        desired: Animal,
        token: u64,
        now: Instant,
    ) -> Vec<Out> {
        if self.peers.get(&peer).is_some_and(|p| p.player.is_some()) {
            log::debug!("peer {peer}: Hello repetido; ignorado");
            return Vec::new();
        }
        if let Err(reason) = check_version(version) {
            return vec![send(peer, Channel::Events, &HostMsg::Reject(reason))];
        }
        if let Some(id) = self.rejoinable(token, now) {
            return self.rejoin(peer, id);
        }
        let Some(id) = (1..=3)
            .map(PlayerId)
            .find(|id| !self.players.contains_key(id))
        else {
            return vec![reject(peer, RejectReason::RoomFull)];
        };
        if self.players.values().any(|p| p.animal == desired) {
            return vec![
                reject(peer, RejectReason::AnimalTaken),
                send(peer, Channel::Events, &self.lobby_msg()),
            ];
        }
        let pos = Vec2 {
            x: 0.2 + 0.2 * f32::from(id.0),
            y: 0.8,
        };
        if let Err(e) = self.world.spawn(id, desired, pos) {
            // Não deveria acontecer: jogadores e mundo andam juntos.
            log::warn!("spawn do jogador {} falhou: {e}", id.0);
            let reason = match e {
                SimError::AnimalTaken => RejectReason::AnimalTaken,
                SimError::Full | SimError::PlayerExists => RejectReason::RoomFull,
            };
            return vec![reject(peer, reason)];
        }
        self.players.insert(
            id,
            Player::new(
                sanitize_name(name, GUEST_FALLBACK_NAME),
                desired,
                self.fresh_token(token),
                Some(peer),
            ),
        );
        self.bind(peer, id)
    }

    /// Religa o jogador a um novo peer, com o animal e o nome originais. Se dormia, acorda o
    /// pet; se ainda estava ligado a outro peer, esse peer é dado como morto, esquecido e
    /// perde o vídeo.
    fn rejoin(&mut self, peer: PeerId, id: PlayerId) -> Vec<Out> {
        let Some(player) = self.players.get_mut(&id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(old) = player.peer.replace(peer) {
            log::debug!(
                "peer {old} substituído pelo peer {peer} no jogador {}",
                id.0
            );
            self.peers.remove(&old);
            out.push(Out::Video {
                peer: old,
                on: false,
            });
        }
        if player.asleep_since.take().is_some() {
            self.world.set_asleep(id, false);
        }
        out.extend(self.bind(peer, id));
        out
    }

    /// Associa o peer ao jogador, manda o `Welcome`, liga o vídeo e anuncia a mudança. Só
    /// peers admitidos chegam aqui: lobby e `Reject` nunca recebem a tela do Host.
    fn bind(&mut self, peer: PeerId, id: PlayerId) -> Vec<Out> {
        if let Some(state) = self.peers.get_mut(&peer) {
            state.player = Some(id);
        }
        let Some(animal) = self.players.get(&id).map(|p| p.animal) else {
            return Vec::new();
        };
        let welcome = HostMsg::Welcome {
            player_id: id,
            animal,
            snapshot: self.world.snapshot(),
            aspect: self.world.aspect(),
        };
        let mut out = vec![
            send(peer, Channel::Events, &welcome),
            Out::Video { peer, on: true },
        ];
        out.extend(self.players_changed());
        out
    }

    fn on_input(&mut self, peer: PeerId, frame: InputFrame, now: Instant) {
        let Some(state) = self.peers.get_mut(&peer) else {
            return;
        };
        let Some(id) = state.player else {
            log::debug!("peer {peer}: Input antes do Welcome; ignorado");
            return;
        };
        if !state.allow_input(now) {
            if state.window_inputs == MAX_INPUTS_PER_SEC + 1 {
                log::debug!("peer {peer}: mais de {MAX_INPUTS_PER_SEC} inputs/s; descartando");
            }
            return;
        }
        // `seq` repetido ou antigo é descartado pela sim.
        self.world.apply_input(id, frame);
    }

    /// Jogador do peer, se ele já foi admitido. O autor de chat e emote vem sempre daqui.
    fn admitted_player(&self, peer: PeerId) -> Option<PlayerId> {
        let id = self.peers.get(&peer).and_then(|p| p.player);
        if id.is_none() {
            log::debug!("peer {peer}: chat ou emote antes do Welcome; ignorado");
        }
        id
    }

    /// Chat de um jogador: sanitiza, aplica o limite dele e, se passar, manda a todos os
    /// admitidos (inclusive o autor) e à UI local. Texto vazio não gasta o limite.
    fn chat_from(&mut self, from: PlayerId, text: &str, now: Instant) -> Vec<Out> {
        let Some(text) = sanitize_chat(text) else {
            log::debug!("jogador {}: chat vazio; ignorado", from.0);
            return Vec::new();
        };
        let Some(player) = self.players.get_mut(&from) else {
            return Vec::new();
        };
        if !player.chat_limit.allow(now) {
            log::debug!("jogador {}: chat acima do limite; descartado", from.0);
            return Vec::new();
        }
        let bytes = encode(&HostMsg::Chat {
            from,
            text: text.clone(),
        });
        let mut out = self.to_admitted(Channel::Events, &bytes);
        out.push(Out::Chat { from, text });
        out
    }

    /// Emote de um jogador: aplica o intervalo mínimo dele e repassa como o chat.
    fn emote_from(&mut self, from: PlayerId, emote: Emote, now: Instant) -> Vec<Out> {
        let Some(player) = self.players.get_mut(&from) else {
            return Vec::new();
        };
        if !player.emote_limit.allow(now) {
            log::debug!("jogador {}: emote antes do intervalo; descartado", from.0);
            return Vec::new();
        }
        let bytes = encode(&HostMsg::Emote { from, emote });
        let mut out = self.to_admitted(Channel::Events, &bytes);
        out.push(Out::Emote { from, emote });
        out
    }

    /// Os mesmos bytes para cada peer admitido.
    fn to_admitted(&self, channel: Channel, bytes: &[u8]) -> Vec<Out> {
        self.peers
            .iter()
            .filter(|(_, state)| state.admitted())
            .map(|(peer, _)| Out::Send {
                peer: *peer,
                channel,
                bytes: bytes.to_vec(),
            })
            .collect()
    }

    fn on_closed(&mut self, peer: PeerId, now: Instant) -> Vec<Out> {
        let Some(state) = self.peers.remove(&peer) else {
            return Vec::new();
        };
        let Some(id) = state.player else {
            return Vec::new();
        };
        let Some(player) = self.players.get_mut(&id) else {
            return Vec::new();
        };
        player.peer = None;
        if player.asleep_since.is_none() {
            player.asleep_since = Some(now);
            self.world.set_asleep(id, true);
        }
        self.players_changed()
    }

    /// Jogador que o token religa: dormindo há no máximo `SLEEP_TIMEOUT`, ou acordado (o peer
    /// antigo ainda não caiu, p.ex. ICE demorando a falhar). Token 0 nunca religa.
    fn rejoinable(&self, token: u64, now: Instant) -> Option<PlayerId> {
        if token == NO_REJOIN_TOKEN {
            return None;
        }
        self.players
            .iter()
            .find(|(_, p)| p.rejoin_token == Some(token) && !p.sleep_expired(now))
            .map(|(id, _)| *id)
    }

    /// Token de reconexão para um jogador novo: 0 ou um token já em uso valem como nenhum.
    fn fresh_token(&self, token: u64) -> Option<u64> {
        let in_use = self.players.values().any(|p| p.rejoin_token == Some(token));
        (token != NO_REJOIN_TOKEN && !in_use).then_some(token)
    }

    fn lobby_msg(&self) -> HostMsg {
        let taken = Animal::ALL
            .into_iter()
            .filter(|a| self.players.values().any(|p| p.animal == *a))
            .collect();
        HostMsg::Lobby { taken }
    }

    /// `Roster` a quem joga, `Lobby` a quem ainda escolhe e o roster para a UI local.
    fn players_changed(&self) -> Vec<Out> {
        let roster = self.roster();
        let roster_bytes = encode(&HostMsg::Roster(roster.clone()));
        let lobby_bytes = encode(&self.lobby_msg());
        let mut out: Vec<Out> = self
            .peers
            .iter()
            .map(|(peer, state)| Out::Send {
                peer: *peer,
                channel: Channel::Events,
                bytes: if state.admitted() {
                    roster_bytes.clone()
                } else {
                    lobby_bytes.clone()
                },
            })
            .collect();
        out.push(Out::Roster(roster));
        out
    }
}

fn reject(peer: PeerId, reason: RejectReason) -> Out {
    send(peer, Channel::Events, &HostMsg::Reject(reason))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::net::PeerId;
    use crate::session::HOST_PLAYER;
    use dm_protocol::{
        decode, encode, AnimState, Channel, GuestMsg, HostMsg, PetState, PlayerId, RejectReason,
        MAX_CHAT_CHARS, PROTOCOL_VERSION,
    };

    const ASPECT: f32 = 16.0 / 9.0;

    fn session() -> HostSession {
        HostSession::new(ASPECT, 1, "Tamy", Animal::Duck)
    }

    fn hello_v(version: u16, name: &str, animal: Animal, token: u64) -> Vec<u8> {
        encode(&GuestMsg::Hello {
            protocol_version: version,
            name: name.into(),
            desired_animal: animal,
            rejoin_token: token,
        })
    }

    fn hello(name: &str, animal: Animal, token: u64) -> Vec<u8> {
        hello_v(PROTOCOL_VERSION, name, animal, token)
    }

    fn input(seq: u32, move_x: i8) -> Vec<u8> {
        encode(&GuestMsg::Input(InputFrame {
            seq,
            move_x,
            move_y: 0,
            run: false,
        }))
    }

    fn recv(peer: PeerId, channel: Channel, bytes: Vec<u8>) -> NetIn {
        NetIn::Recv {
            peer,
            channel,
            bytes,
        }
    }

    /// Só os `Send`, decodificados.
    fn decode_out(outs: &[Out]) -> Vec<(PeerId, Channel, HostMsg)> {
        outs.iter()
            .filter_map(|o| match o {
                Out::Send {
                    peer,
                    channel,
                    bytes,
                } => Some((*peer, *channel, decode::<HostMsg>(bytes).unwrap())),
                Out::Roster(_) | Out::Video { .. } | Out::Chat { .. } | Out::Emote { .. } => None,
            })
            .collect()
    }

    fn local_roster(outs: &[Out]) -> Option<Vec<RosterEntry>> {
        outs.iter().rev().find_map(|o| match o {
            Out::Roster(r) => Some(r.clone()),
            Out::Send { .. } | Out::Video { .. } | Out::Chat { .. } | Out::Emote { .. } => None,
        })
    }

    /// Só os `Video`, na ordem: `(peer, on)`.
    fn videos(outs: &[Out]) -> Vec<(PeerId, bool)> {
        outs.iter()
            .filter_map(|o| match o {
                Out::Video { peer, on } => Some((*peer, *on)),
                Out::Send { .. } | Out::Roster(_) | Out::Chat { .. } | Out::Emote { .. } => None,
            })
            .collect()
    }

    fn to_peer(outs: &[Out], peer: PeerId) -> Vec<(Channel, HostMsg)> {
        decode_out(outs)
            .into_iter()
            .filter(|(p, _, _)| *p == peer)
            .map(|(_, c, m)| (c, m))
            .collect()
    }

    fn pet(s: &HostSession, id: u8) -> Option<PetState> {
        s.snapshot().pets.into_iter().find(|p| p.id == PlayerId(id))
    }

    /// Abre o peer e manda Hello; devolve só as saídas do Hello.
    fn join(
        s: &mut HostSession,
        peer: PeerId,
        animal: Animal,
        token: u64,
        now: Instant,
    ) -> Vec<Out> {
        s.handle(NetIn::PeerOpened(peer), now);
        s.handle(
            recv(peer, Channel::Events, hello("Ana", animal, token)),
            now,
        )
    }

    #[test]
    fn lobby_on_open() {
        let mut s = session();
        let outs = s.handle(NetIn::PeerOpened(7), Instant::now());
        assert_eq!(
            decode_out(&outs),
            vec![(
                7,
                Channel::Events,
                HostMsg::Lobby {
                    taken: vec![Animal::Duck]
                }
            )]
        );
    }

    #[test]
    fn hello_welcome_and_roster() {
        let mut s = session();
        let outs = join(&mut s, 7, Animal::Capybara, 1, Instant::now());
        let msgs = to_peer(&outs, 7);
        match &msgs[0] {
            (
                Channel::Events,
                HostMsg::Welcome {
                    player_id,
                    animal,
                    snapshot,
                    aspect,
                },
            ) => {
                assert_eq!(*player_id, PlayerId(1));
                assert_eq!(*animal, Animal::Capybara);
                assert_eq!(*aspect, ASPECT);
                assert_eq!(snapshot.pets.len(), 2);
            }
            other => panic!("esperava Welcome, veio {other:?}"),
        }
        let roster = msgs
            .iter()
            .find_map(|(c, m)| match m {
                HostMsg::Roster(r) if *c == Channel::Events => Some(r.clone()),
                _ => None,
            })
            .expect("Roster para o convidado");
        assert_eq!(roster.len(), 2);
        assert_eq!(roster[0].id, HOST_PLAYER);
        assert_eq!(roster[0].name, "Tamy");
        assert_eq!(roster[1].animal, Animal::Capybara);
        assert_eq!(local_roster(&outs), Some(roster));
        let p = pet(&s, 1).expect("pet do convidado");
        assert!((p.pos.x - 0.4).abs() < 1e-6 && (p.pos.y - 0.8).abs() < 1e-6);
    }

    #[test]
    fn same_animal_race() {
        let mut s = session();
        let now = Instant::now();
        s.handle(NetIn::PeerOpened(7), now);
        s.handle(NetIn::PeerOpened(8), now);
        let a = s.handle(recv(7, Channel::Events, hello("A", Animal::Horse, 1)), now);
        let b = s.handle(recv(8, Channel::Events, hello("B", Animal::Horse, 2)), now);
        assert!(matches!(
            to_peer(&a, 7)[0],
            (Channel::Events, HostMsg::Welcome { .. })
        ));
        assert_eq!(
            to_peer(&b, 8),
            vec![
                (Channel::Events, HostMsg::Reject(RejectReason::AnimalTaken)),
                (
                    Channel::Events,
                    HostMsg::Lobby {
                        taken: vec![Animal::Duck, Animal::Horse]
                    }
                ),
            ]
        );
        assert_eq!(s.roster().len(), 2);
    }

    #[test]
    fn version_mismatch_rejected() {
        let mut s = session();
        let now = Instant::now();
        s.handle(NetIn::PeerOpened(7), now);
        let outs = s.handle(
            recv(7, Channel::Events, hello_v(99, "A", Animal::Horse, 1)),
            now,
        );
        assert_eq!(
            decode_out(&outs),
            vec![(
                7,
                Channel::Events,
                HostMsg::Reject(RejectReason::VersionMismatch { host: 1 })
            )]
        );
        assert_eq!(s.roster().len(), 1);
    }

    #[test]
    fn room_full() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 1, Animal::Capybara, 1, now);
        join(&mut s, 2, Animal::Horse, 2, now);
        join(&mut s, 3, Animal::Chicken, 3, now);
        assert_eq!(s.roster().len(), 4);
        let outs = join(&mut s, 4, Animal::Horse, 4, now);
        assert_eq!(
            decode_out(&outs),
            vec![(4, Channel::Events, HostMsg::Reject(RejectReason::RoomFull))]
        );
        assert_eq!(s.roster().len(), 4);

        // Sala cheia com um jogador dormindo: o token dele ainda religa.
        s.handle(NetIn::PeerClosed(2), now);
        let outs = s.handle(recv(4, Channel::Events, hello("D", Animal::Duck, 2)), now);
        assert_eq!(welcome_of(&outs, 4), Some((PlayerId(2), Animal::Horse)));
        assert_eq!(s.roster().len(), 4);
    }

    #[test]
    fn garbage_and_wrong_channel_ignored() {
        let mut s = session();
        let now = Instant::now();
        s.handle(NetIn::PeerOpened(7), now);
        let cases = [
            recv(7, Channel::Events, vec![0xFF; 10]),
            recv(7, Channel::Input, hello("A", Animal::Horse, 1)),
            recv(7, Channel::State, hello("A", Animal::Horse, 1)),
            recv(7, Channel::Input, input(1, 1)),
            recv(7, Channel::Input, Vec::new()),
            recv(99, Channel::Events, hello("A", Animal::Horse, 1)),
            NetIn::PeerClosed(99),
        ];
        for ev in cases {
            assert!(s.handle(ev, now).is_empty());
        }
        assert_eq!(s.roster().len(), 1);

        // Input no canal Events, mesmo com Welcome, também é ignorado.
        join(&mut s, 8, Animal::Horse, 2, now);
        assert!(s
            .handle(recv(8, Channel::Events, input(1, 1)), now)
            .is_empty());
        assert_eq!(pet(&s, 1).unwrap().last_input_seq, 0);
    }

    #[test]
    fn long_name_sanitized() {
        let mut s = session();
        let now = Instant::now();
        s.handle(NetIn::PeerOpened(7), now);
        let name = format!("{}\n", "x".repeat(10_000));
        let outs = s.handle(
            recv(7, Channel::Events, hello(&name, Animal::Horse, 1)),
            now,
        );
        let roster = local_roster(&outs).expect("roster");
        let entry = roster.iter().find(|e| e.id == PlayerId(1)).unwrap();
        assert!(entry.name.chars().count() <= 16, "{}", entry.name);
        assert!(!entry.name.chars().any(char::is_control));
    }

    #[test]
    fn input_moves_guest_pet() {
        let mut s = session();
        let t0 = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, t0);
        let x0 = pet(&s, 1).unwrap().pos.x;
        for n in 1..=60u32 {
            let now = t0 + Duration::from_millis(16 * n as u64);
            s.handle(recv(7, Channel::Input, input(n, 1)), now);
            s.tick(now);
        }
        let p = pet(&s, 1).unwrap();
        assert!(p.pos.x > x0 + 0.05, "x0={x0} x={}", p.pos.x);
        assert_eq!(p.last_input_seq, 60);
    }

    #[test]
    fn input_flood_capped() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, now);
        for seq in 1..=1000 {
            s.handle(recv(7, Channel::Input, input(seq, 1)), now);
        }
        let seq = pet(&s, 1).unwrap().last_input_seq;
        assert!(seq <= MAX_INPUTS_PER_SEC, "seq = {seq}");

        // Nova janela, novos inputs aceitos.
        let later = now + Duration::from_secs(1);
        s.handle(recv(7, Channel::Input, input(1001, 1)), later);
        assert_eq!(pet(&s, 1).unwrap().last_input_seq, 1001);
    }

    #[test]
    fn snapshots_every_three_ticks() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, now);
        join(&mut s, 8, Animal::Horse, 2, now);
        s.handle(NetIn::PeerOpened(9), now);
        let mut outs = Vec::new();
        for _ in 0..9 {
            outs.extend(s.tick(now));
        }
        let sent = decode_out(&outs);
        for peer in [7, 8] {
            let snaps = sent
                .iter()
                .filter(|(p, c, m)| {
                    *p == peer && *c == Channel::State && matches!(m, HostMsg::Snapshot(_))
                })
                .count();
            assert_eq!(snaps, 3, "peer {peer}");
        }
        assert_eq!(sent.len(), 6);
        assert!(sent.iter().all(|(p, _, _)| *p != 9));
    }

    #[test]
    fn sleep_and_rejoin() {
        let mut s = session();
        let t0 = Instant::now();
        s.handle(NetIn::PeerOpened(5), t0);
        join(&mut s, 7, Animal::Capybara, 42, t0);
        let outs = s.handle(NetIn::PeerClosed(7), t0);
        let roster = local_roster(&outs).expect("roster local");
        let entry = roster.iter().find(|e| e.id == PlayerId(1)).unwrap();
        assert!(entry.asleep);
        assert_eq!(pet(&s, 1).unwrap().anim, AnimState::Sleep);
        // O animal continua ocupado para quem está no lobby.
        assert!(to_peer(&outs, 5).contains(&(
            Channel::Events,
            HostMsg::Lobby {
                taken: vec![Animal::Duck, Animal::Capybara]
            }
        )));

        // Dormindo, o pet não recebe snapshots nem mexe.
        for _ in 0..3 {
            assert!(decode_out(&s.tick(t0)).is_empty());
        }

        let t1 = t0 + Duration::from_secs(10);
        s.handle(NetIn::PeerOpened(8), t1);
        let outs = s.handle(
            recv(8, Channel::Events, hello("Ana", Animal::Chicken, 42)),
            t1,
        );
        match &to_peer(&outs, 8)[0] {
            (
                Channel::Events,
                HostMsg::Welcome {
                    player_id, animal, ..
                },
            ) => {
                assert_eq!(*player_id, PlayerId(1));
                assert_eq!(*animal, Animal::Capybara);
            }
            other => panic!("esperava Welcome, veio {other:?}"),
        }
        assert!(!local_roster(&outs).unwrap()[1].asleep);
        assert_eq!(pet(&s, 1).unwrap().anim, AnimState::Idle);
        assert!(pet(&s, 4).is_none() && s.roster().len() == 2);

        // O novo peer controla o pet; o tempo de sono não vence mais.
        s.handle(recv(8, Channel::Input, input(1, 1)), t1);
        s.tick(t1 + Duration::from_secs(40));
        assert_eq!(pet(&s, 1).unwrap().last_input_seq, 1);
    }

    /// Devolve `(player_id, animal)` do `Welcome` mandado ao peer, se houver.
    fn welcome_of(outs: &[Out], peer: PeerId) -> Option<(PlayerId, Animal)> {
        to_peer(outs, peer).into_iter().find_map(|(c, m)| match m {
            HostMsg::Welcome {
                player_id, animal, ..
            } if c == Channel::Events => Some((player_id, animal)),
            _ => None,
        })
    }

    #[test]
    fn rejoin_while_old_peer_alive() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 42, now);
        s.handle(recv(7, Channel::Input, input(5, 1)), now);

        // O peer antigo ainda não caiu (ICE demora); o novo peer religa o mesmo jogador.
        let outs = join(&mut s, 9, Animal::Horse, 42, now);
        assert_eq!(welcome_of(&outs, 9), Some((PlayerId(1), Animal::Capybara)));
        assert_eq!(s.roster().len(), 2);
        assert!(!local_roster(&outs).unwrap()[1].asleep);
        assert!(to_peer(&outs, 7).is_empty(), "peer antigo foi esquecido");

        // O PeerClosed atrasado do peer antigo não põe o pet para dormir.
        assert!(s.handle(NetIn::PeerClosed(7), now).is_empty());
        assert!(!s.roster()[1].asleep);
        assert_eq!(pet(&s, 1).unwrap().anim, AnimState::Idle);

        // Input do peer antigo é ignorado; o novo controla o pet.
        s.handle(recv(7, Channel::Input, input(6, 1)), now);
        assert_eq!(pet(&s, 1).unwrap().last_input_seq, 5);
        s.handle(recv(9, Channel::Input, input(7, 1)), now);
        assert_eq!(pet(&s, 1).unwrap().last_input_seq, 7);

        // Snapshots só vão para o peer novo.
        let mut ticks = Vec::new();
        for _ in 0..3 {
            ticks.extend(s.tick(now));
        }
        assert!(decode_out(&ticks).iter().all(|(p, _, _)| *p == 9));
        assert_eq!(decode_out(&ticks).len(), 1);

        // Um segundo Hello do peer que já tem Welcome continua sem efeito.
        let again = s.handle(
            recv(9, Channel::Events, hello("B", Animal::Chicken, 42)),
            now,
        );
        assert!(again.is_empty());
        assert_eq!(s.roster().len(), 2);
    }

    #[test]
    fn duplicate_or_zero_token_not_rejoinable() {
        let mut s = session();
        let t0 = Instant::now();

        // Token 0 = sem reconexão.
        join(&mut s, 5, Animal::Horse, 0, t0);
        s.handle(NetIn::PeerClosed(5), t0);
        let outs = join(&mut s, 6, Animal::Horse, 0, t0);
        assert_eq!(
            to_peer(&outs, 6)[0],
            (Channel::Events, HostMsg::Reject(RejectReason::AnimalTaken))
        );

        // Jogador 2 (Capybara, token 42) dorme e vence sem tick no meio: o token não religa
        // mais, e o novo jogador com o mesmo token entra sem token de reconexão.
        join(&mut s, 7, Animal::Capybara, 42, t0);
        s.handle(NetIn::PeerClosed(7), t0);
        let t1 = t0 + Duration::from_secs(31);
        let outs = join(&mut s, 8, Animal::Chicken, 42, t1);
        assert_eq!(welcome_of(&outs, 8), Some((PlayerId(3), Animal::Chicken)));
        // O tick remove os jogadores 1 e 2, que dormem desde t0.
        s.tick(t1);
        assert!(pet(&s, 1).is_none() && pet(&s, 2).is_none());

        // O jogador 3 cai; o token 42 duplicado não o religa: entra um jogador novo.
        s.handle(NetIn::PeerClosed(8), t1);
        let outs = join(&mut s, 9, Animal::Capybara, 42, t1);
        assert_eq!(welcome_of(&outs, 9), Some((PlayerId(1), Animal::Capybara)));
        assert!(s.roster().iter().any(|e| e.id == PlayerId(3) && e.asleep));
    }

    #[test]
    fn expired_sleeper_not_rejoined_before_tick() {
        let mut s = session();
        let t0 = Instant::now();
        join(&mut s, 7, Animal::Capybara, 42, t0);
        s.handle(NetIn::PeerClosed(7), t0);
        let outs = join(&mut s, 8, Animal::Chicken, 42, t0 + Duration::from_secs(31));
        assert_ne!(welcome_of(&outs, 8), Some((PlayerId(1), Animal::Capybara)));
        assert_eq!(pet(&s, 1).unwrap().anim, AnimState::Sleep);
    }

    #[test]
    fn sleep_timeout_frees_animal() {
        let mut s = session();
        let t0 = Instant::now();
        join(&mut s, 7, Animal::Capybara, 42, t0);
        s.handle(NetIn::PeerClosed(7), t0);
        s.handle(NetIn::PeerOpened(9), t0);

        assert!(local_roster(&s.tick(t0 + Duration::from_secs(30))).is_none());
        let outs = s.tick(t0 + Duration::from_secs(31));
        assert!(pet(&s, 1).is_none());
        assert_eq!(local_roster(&outs).unwrap().len(), 1);
        assert_eq!(s.roster().len(), 1);
        assert!(to_peer(&outs, 9).contains(&(
            Channel::Events,
            HostMsg::Lobby {
                taken: vec![Animal::Duck]
            }
        )));

        // O token antigo não religa mais: vira um Hello comum.
        let outs = s.handle(
            recv(9, Channel::Events, hello("Ana", Animal::Chicken, 42)),
            t0 + Duration::from_secs(32),
        );
        match &to_peer(&outs, 9)[0] {
            (
                Channel::Events,
                HostMsg::Welcome {
                    player_id, animal, ..
                },
            ) => {
                assert_eq!(*player_id, PlayerId(1));
                assert_eq!(*animal, Animal::Chicken);
            }
            other => panic!("esperava Welcome, veio {other:?}"),
        }
    }

    #[test]
    fn host_input_and_autonomy() {
        let mut s = session();
        s.set_host_autonomous(false);
        let x0 = pet(&s, 0).unwrap().pos.x;
        for seq in 1..=30 {
            s.host_input(InputFrame {
                seq,
                move_x: -1,
                move_y: 0,
                run: false,
            });
            s.tick(Instant::now());
        }
        assert!(pet(&s, 0).unwrap().pos.x < x0);
        assert_eq!(s.roster()[0].animal, Animal::Duck);
    }

    #[test]
    fn welcome_turns_video_on() {
        let mut s = session();
        let now = Instant::now();
        assert!(videos(&s.handle(NetIn::PeerOpened(7), now)).is_empty());
        let outs = s.handle(
            recv(7, Channel::Events, hello("Ana", Animal::Capybara, 1)),
            now,
        );
        assert_eq!(welcome_of(&outs, 7), Some((PlayerId(1), Animal::Capybara)));
        assert_eq!(videos(&outs), vec![(7, true)]);

        // Hello repetido, input e tick não mexem no vídeo.
        let again = s.handle(
            recv(7, Channel::Events, hello("Ana", Animal::Horse, 1)),
            now,
        );
        assert!(videos(&again).is_empty());
        let mut ticks = Vec::new();
        for _ in 0..3 {
            ticks.extend(s.tick(now));
        }
        assert!(videos(&ticks).is_empty());
    }

    #[test]
    fn rejected_hello_has_no_video() {
        let mut s = session();
        let now = Instant::now();
        s.handle(NetIn::PeerOpened(7), now);
        let wrong_version = s.handle(
            recv(7, Channel::Events, hello_v(99, "A", Animal::Horse, 1)),
            now,
        );
        assert!(videos(&wrong_version).is_empty());
        // O Pato é do Host.
        let taken = s.handle(recv(7, Channel::Events, hello("A", Animal::Duck, 1)), now);
        assert_eq!(
            to_peer(&taken, 7)[0],
            (Channel::Events, HostMsg::Reject(RejectReason::AnimalTaken))
        );
        assert!(videos(&taken).is_empty());
        // Lixo e canal errado também não.
        assert!(videos(&s.handle(recv(7, Channel::Events, vec![0xFF; 8]), now)).is_empty());

        join(&mut s, 1, Animal::Capybara, 2, now);
        join(&mut s, 2, Animal::Horse, 3, now);
        join(&mut s, 3, Animal::Chicken, 4, now);
        let full = join(&mut s, 4, Animal::Horse, 5, now);
        assert!(videos(&full).is_empty());
    }

    #[test]
    fn rejoin_moves_video_to_new_peer() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 42, now);

        // Peer antigo ainda vivo: o vídeo sai dele e vai para o novo.
        let outs = join(&mut s, 9, Animal::Horse, 42, now);
        assert_eq!(welcome_of(&outs, 9), Some((PlayerId(1), Animal::Capybara)));
        assert_eq!(videos(&outs), vec![(7, false), (9, true)]);

        // Peer caído antes do rejoin: não há o que cortar.
        s.handle(NetIn::PeerClosed(9), now);
        let outs = join(&mut s, 11, Animal::Horse, 42, now + Duration::from_secs(5));
        assert_eq!(videos(&outs), vec![(11, true)]);
    }

    fn chat(text: &str) -> Vec<u8> {
        encode(&GuestMsg::Chat { text: text.into() })
    }

    fn emote(e: Emote) -> Vec<u8> {
        encode(&GuestMsg::Emote(e))
    }

    /// Só os `Chat`/`Emote` locais, na ordem.
    fn locals(outs: &[Out]) -> Vec<Out> {
        outs.iter()
            .filter(|o| matches!(o, Out::Chat { .. } | Out::Emote { .. }))
            .cloned()
            .collect()
    }

    /// Esperado de um chat aceito: o `HostMsg::Chat` para cada peer (na ordem) e o local.
    fn chat_outs(peers: &[PeerId], from: PlayerId, text: &str) -> Vec<Out> {
        let msg = HostMsg::Chat {
            from,
            text: text.into(),
        };
        let mut out: Vec<Out> = peers
            .iter()
            .map(|p| send(*p, Channel::Events, &msg))
            .collect();
        out.push(Out::Chat {
            from,
            text: text.into(),
        });
        out
    }

    fn emote_outs(peers: &[PeerId], from: PlayerId, e: Emote) -> Vec<Out> {
        let msg = HostMsg::Emote { from, emote: e };
        let mut out: Vec<Out> = peers
            .iter()
            .map(|p| send(*p, Channel::Events, &msg))
            .collect();
        out.push(Out::Emote { from, emote: e });
        out
    }

    #[test]
    fn guest_chat_broadcast() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, now);
        join(&mut s, 8, Animal::Horse, 2, now);
        // Peer no lobby não recebe nada.
        s.handle(NetIn::PeerOpened(9), now);
        let outs = s.handle(recv(7, Channel::Events, chat("oi gente")), now);
        assert_eq!(outs, chat_outs(&[7, 8], PlayerId(1), "oi gente"));

        let outs = s.handle(recv(8, Channel::Events, emote(Emote::Heart)), now);
        assert_eq!(outs, emote_outs(&[7, 8], PlayerId(2), Emote::Heart));
    }

    #[test]
    fn chat_from_lobby_ignored() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, now);
        s.handle(NetIn::PeerOpened(9), now);
        assert!(s
            .handle(recv(9, Channel::Events, chat("oi")), now)
            .is_empty());
        assert!(s
            .handle(recv(9, Channel::Events, emote(Emote::Wow)), now)
            .is_empty());
        // Peer rejeitado (versão errada) continua no lobby: também não fala.
        s.handle(
            recv(9, Channel::Events, hello_v(99, "A", Animal::Horse, 1)),
            now,
        );
        assert!(s
            .handle(recv(9, Channel::Events, chat("oi")), now)
            .is_empty());
        // Peer desconhecido também não.
        assert!(s
            .handle(recv(99, Channel::Events, chat("oi")), now)
            .is_empty());
    }

    #[test]
    fn chat_wrong_channel_ignored() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, now);
        for channel in [Channel::Input, Channel::State] {
            assert!(s.handle(recv(7, channel, chat("oi")), now).is_empty());
            assert!(s
                .handle(recv(7, channel, emote(Emote::Wow)), now)
                .is_empty());
        }
        // O canal errado não gastou o limite: os 3 chats e o emote ainda passam.
        for _ in 0..3 {
            assert!(!locals(&s.handle(recv(7, Channel::Events, chat("oi")), now)).is_empty());
        }
        assert!(!locals(&s.handle(recv(7, Channel::Events, emote(Emote::Wow)), now)).is_empty());
    }

    #[test]
    fn chat_sanitized_and_empty_dropped() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, now);
        let outs = s.handle(
            recv(
                7,
                Channel::Events,
                chat("  a\u{0}b\n\u{202E}c   d\u{200B} "),
            ),
            now,
        );
        assert_eq!(outs, chat_outs(&[7], PlayerId(1), "abc d"));

        for junk in ["     ", "\u{200B}\u{202E}\u{FEFF}", "\n\t\r", ""] {
            assert!(
                s.handle(recv(7, Channel::Events, chat(junk)), now)
                    .is_empty(),
                "{junk:?}"
            );
        }
        // Texto gigante sai cortado.
        let outs = s.handle(recv(7, Channel::Events, chat(&"x".repeat(10_000))), now);
        match locals(&outs).as_slice() {
            [Out::Chat { text, .. }] => assert_eq!(text.chars().count(), MAX_CHAT_CHARS),
            other => panic!("esperava um Chat local, veio {other:?}"),
        }
        // Lixo no canal events não derruba nem sai.
        assert!(s
            .handle(recv(7, Channel::Events, vec![5, 0xFF, 0xFF, 0xFF]), now)
            .is_empty());
    }

    #[test]
    fn chat_rate_limited() {
        let mut s = session();
        let t0 = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, t0);
        join(&mut s, 8, Animal::Horse, 2, t0);
        let mut outs = Vec::new();
        for n in 0..5 {
            outs.extend(s.handle(recv(7, Channel::Events, chat(&format!("m{n}"))), t0));
        }
        let texts: Vec<_> = locals(&outs)
            .into_iter()
            .map(|o| match o {
                Out::Chat { text, .. } => text,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(texts, ["m0", "m1", "m2"]);
        // O que foi barrado não chega a ninguém.
        assert_eq!(decode_out(&outs).len(), 6);

        // Outro jogador tem o próprio limite.
        assert!(!locals(&s.handle(recv(8, Channel::Events, chat("eu")), t0)).is_empty());

        let t1 = t0 + Duration::from_secs(2);
        let mut outs = Vec::new();
        for n in 5..8 {
            outs.extend(s.handle(recv(7, Channel::Events, chat(&format!("m{n}"))), t1));
        }
        assert_eq!(
            locals(&outs),
            vec![Out::Chat {
                from: PlayerId(1),
                text: "m5".into()
            }]
        );

        // Flood de 100 por segundo durante 10 s: no máximo 3 + 10/2.
        let mut accepted = 0;
        for ms in (0..10_000u64).step_by(10) {
            let now = t1 + Duration::from_secs(10) + Duration::from_millis(ms);
            accepted += locals(&s.handle(recv(7, Channel::Events, chat("spam")), now)).len();
        }
        assert!(accepted <= 3 + 5, "aceitos = {accepted}");
    }

    #[test]
    fn emote_cooldown() {
        let mut s = session();
        let t0 = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, t0);
        let mut outs = s.handle(recv(7, Channel::Events, emote(Emote::Heart)), t0);
        outs.extend(s.handle(recv(7, Channel::Events, emote(Emote::Laugh)), t0));
        assert_eq!(outs, emote_outs(&[7], PlayerId(1), Emote::Heart));
        let half = t0 + Duration::from_millis(500);
        assert!(s
            .handle(recv(7, Channel::Events, emote(Emote::Laugh)), half)
            .is_empty());
        let t1 = t0 + Duration::from_secs(1);
        let outs = s.handle(recv(7, Channel::Events, emote(Emote::Laugh)), t1);
        assert_eq!(outs, emote_outs(&[7], PlayerId(1), Emote::Laugh));
        // Emote e chat têm limites separados.
        assert!(!locals(&s.handle(recv(7, Channel::Events, chat("oi")), t1)).is_empty());
    }

    #[test]
    fn author_is_peer_player() {
        let mut s = session();
        let now = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, now);
        join(&mut s, 8, Animal::Horse, 2, now);
        // Mesmo um texto que imita um autor sai com o `from` do peer que mandou.
        let outs = s.handle(recv(8, Channel::Events, chat("from: 0 Tamy: oi")), now);
        let sent = decode_out(&outs);
        assert_eq!(sent.len(), 2);
        for (_, _, msg) in sent {
            assert!(
                matches!(
                    msg,
                    HostMsg::Chat {
                        from: PlayerId(2),
                        ..
                    }
                ),
                "{msg:?}"
            );
        }
        assert!(matches!(
            locals(&outs).as_slice(),
            [Out::Chat {
                from: PlayerId(2),
                ..
            }]
        ));
        // Um `HostMsg::Chat` forjado vindo do convidado não decodifica como `GuestMsg::Chat`
        // com outro autor: ou vira lixo, ou é outra variante; nunca sai em nome do Host.
        let forged = encode(&HostMsg::Chat {
            from: HOST_PLAYER,
            text: "sou o Host".into(),
        });
        let outs = s.handle(recv(8, Channel::Events, forged), now);
        assert!(outs.iter().all(|o| !matches!(
            o,
            Out::Chat { from, .. } | Out::Emote { from, .. } if *from != PlayerId(2)
        )));
        assert!(decode_out(&outs).iter().all(|(_, _, m)| !matches!(
            m,
            HostMsg::Chat { from, .. } | HostMsg::Emote { from, .. } if *from != PlayerId(2)
        )));
    }

    #[test]
    fn host_chat_and_emote() {
        let mut s = session();
        let t0 = Instant::now();
        join(&mut s, 7, Animal::Capybara, 1, t0);
        join(&mut s, 8, Animal::Horse, 2, t0);
        s.handle(NetIn::PeerOpened(9), t0);

        let outs = s.host_chat("  olá\u{202E} ", t0);
        assert_eq!(outs, chat_outs(&[7, 8], HOST_PLAYER, "olá"));
        assert!(s.host_chat("   ", t0).is_empty());
        assert!(!s.host_chat("2", t0).is_empty());
        assert!(!s.host_chat("3", t0).is_empty());
        assert!(s.host_chat("4", t0).is_empty(), "o Host respeita o limite");
        assert!(!s.host_chat("5", t0 + Duration::from_secs(2)).is_empty());

        let outs = s.host_emote(Emote::Music, t0);
        assert_eq!(outs, emote_outs(&[7, 8], HOST_PLAYER, Emote::Music));
        assert!(s.host_emote(Emote::Music, t0).is_empty());
        assert!(!s
            .host_emote(Emote::Question, t0 + Duration::from_secs(1))
            .is_empty());

        // Sem convidados, só a UI local.
        let mut alone = session();
        assert_eq!(alone.host_chat("oi", t0), chat_outs(&[], HOST_PLAYER, "oi"));
    }

    #[test]
    fn rejoin_keeps_author() {
        let mut s = session();
        let t0 = Instant::now();
        join(&mut s, 7, Animal::Capybara, 42, t0);
        join(&mut s, 8, Animal::Horse, 43, t0);
        for _ in 0..3 {
            s.handle(recv(7, Channel::Events, chat("oi")), t0);
        }
        s.handle(recv(7, Channel::Events, emote(Emote::Heart)), t0);

        // Dormindo, o pet recebe os balões dos outros e o peer caído sai do broadcast.
        s.handle(NetIn::PeerClosed(7), t0);
        let outs = s.handle(recv(8, Channel::Events, chat("cadê?")), t0);
        assert_eq!(outs, chat_outs(&[8], PlayerId(2), "cadê?"));
        // O peer morto não fala mais.
        assert!(s
            .handle(recv(7, Channel::Events, chat("fantasma")), t0)
            .is_empty());

        // Volta pelo rejoin em outro peer: mesmo PlayerId e o mesmo limite de antes.
        join(&mut s, 11, Animal::Chicken, 42, t0);
        assert!(s
            .handle(recv(11, Channel::Events, chat("voltei")), t0)
            .is_empty());
        assert!(s
            .handle(recv(11, Channel::Events, emote(Emote::Wow)), t0)
            .is_empty());
        let t1 = t0 + Duration::from_secs(2);
        let outs = s.handle(recv(11, Channel::Events, chat("voltei")), t1);
        assert_eq!(outs, chat_outs(&[8, 11], PlayerId(1), "voltei"));
        let outs = s.handle(recv(11, Channel::Events, emote(Emote::Wow)), t1);
        assert_eq!(outs, emote_outs(&[8, 11], PlayerId(1), Emote::Wow));

        // Rejoin com o peer antigo ainda vivo: o antigo é esquecido e não fala mais.
        let t2 = t1 + Duration::from_secs(4);
        join(&mut s, 12, Animal::Chicken, 42, t2);
        assert!(s
            .handle(recv(11, Channel::Events, chat("velho")), t2)
            .is_empty());
        let outs = s.handle(recv(12, Channel::Events, chat("novo")), t2);
        assert_eq!(outs, chat_outs(&[8, 12], PlayerId(1), "novo"));
    }
}
