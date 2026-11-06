//! Tipos e mensagens trocados entre Host e Convidados.

mod codec;
mod messages;
mod types;

pub use codec::{check_version, decode, encode, ProtocolError};
pub use messages::{GuestMsg, HostMsg, RejectReason};
pub use types::{AnimState, Animal, Facing, InputFrame, PetState, PlayerId, Snapshot, Vec2};

/// Versão do protocolo de rede. Incrementar a cada mudança incompatível.
pub const PROTOCOL_VERSION: u16 = 1;

/// Tamanho máximo aceito para uma mensagem decodificada.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
