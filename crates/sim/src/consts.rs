//! Constantes da simulação. Velocidades em alturas de tela por segundo.

pub const TICK_HZ: u32 = 60;
pub const DT: f32 = 1.0 / TICK_HZ as f32;
pub const MAX_PLAYERS: usize = 4;
pub const WALK_SPEED: f32 = 0.15;
pub const RUN_SPEED: f32 = 0.35;
/// Ticks sem um novo `InputFrame` até o input ser considerado parado.
pub const INPUT_TIMEOUT_TICKS: u32 = 30;
pub const EDGE_MARGIN: f32 = 0.0;
