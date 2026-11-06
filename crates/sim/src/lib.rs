//! Simulação pura e determinística dos bichinhos. Sem I/O, relógio ou RNG global.

pub mod consts;
mod pet;
mod rng;
mod wander;
mod world;

pub use rng::Rng;
pub use world::{SimError, World};
