//! Servidor de sinalização: partes puras (mensagens, códigos, registro de salas, rate limit)
//! e o servidor WebSocket (`server`).

pub mod code;
pub mod limiter;
pub mod proto;
pub mod registry;
pub mod server;

pub use proto::ConnId;
