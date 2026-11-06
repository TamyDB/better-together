use dm_protocol::Vec2;

use crate::rng::Rng;

const IDLE_MIN_TICKS: u32 = 120;
const IDLE_MAX_TICKS: u32 = 300;
const TARGET_MIN: f32 = 0.05;
const TARGET_MAX: f32 = 0.95;
const ARRIVE_DIST: f32 = 0.01;

/// Comportamento autônomo: espera um pouco, escolhe um ponto e anda até ele.
#[derive(Debug, Clone, Default)]
pub(crate) enum Wander {
    #[default]
    Start,
    Waiting { ticks_left: u32 },
    Walking { target: Vec2 },
}

impl Wander {
    /// Direção a seguir neste tick (sem correr).
    pub fn next_input(&mut self, pos: Vec2, rng: &mut Rng) -> (i8, i8) {
        match *self {
            Wander::Start => {
                *self = Wander::Waiting { ticks_left: rng.range_u32(IDLE_MIN_TICKS, IDLE_MAX_TICKS + 1) };
                (0, 0)
            }
            Wander::Waiting { ticks_left: 0 } => {
                let target = Vec2 {
                    x: rng.range_f32(TARGET_MIN, TARGET_MAX),
                    y: rng.range_f32(TARGET_MIN, TARGET_MAX),
                };
                *self = Wander::Walking { target };
                self.next_input(pos, rng)
            }
            Wander::Waiting { ticks_left } => {
                *self = Wander::Waiting { ticks_left: ticks_left - 1 };
                (0, 0)
            }
            Wander::Walking { target } => {
                let axis = |d: f32| if d.abs() <= ARRIVE_DIST { 0 } else { d.signum() as i8 };
                let dir = (axis(target.x - pos.x), axis(target.y - pos.y));
                if dir == (0, 0) {
                    *self = Wander::Start;
                }
                dir
            }
        }
    }
}
