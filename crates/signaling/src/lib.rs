//! Servidor de sinalização: partes puras (mensagens, códigos, registro de salas, rate limit).

pub mod code;
pub mod limiter;
pub mod proto;
pub mod registry;

pub use proto::ConnId;
