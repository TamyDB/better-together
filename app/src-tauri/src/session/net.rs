//! Fronteira entre a sessão (pura) e o transporte WebRTC.

use dm_protocol::{Channel, RosterEntry};

/// Identificador local de uma conexão com um convidado.
pub type PeerId = u64;

/// Eventos vindos do transporte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetIn {
    PeerOpened(PeerId),
    PeerClosed(PeerId),
    Recv {
        peer: PeerId,
        channel: Channel,
        bytes: Vec<u8>,
    },
}

/// Efeitos pedidos pela sessão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Send {
        peer: PeerId,
        channel: Channel,
        bytes: Vec<u8>,
    },
    /// Roster para a UI local do Host.
    Roster(Vec<RosterEntry>),
    /// Liga (`on`) ou corta o vídeo do Host para o peer: só quem foi admitido vê a tela.
    Video { peer: PeerId, on: bool },
}
