//! Simulação pura e determinística dos bichinhos. Sem I/O, relógio ou RNG global.

pub mod consts;
mod pet;
mod world;

pub use world::{SimError, World};
