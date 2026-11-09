//! Sessão multiplayer: autoridade do Host e tipos de rede.

pub mod guest;
pub mod host;
pub mod interp;
pub mod net;

use dm_protocol::PlayerId;

/// O pet do Host é sempre o jogador 0.
pub const HOST_PLAYER: PlayerId = PlayerId(0);
