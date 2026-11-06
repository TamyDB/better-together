use dm_protocol::{AnimState, Facing, InputFrame, PetState};

use crate::consts::{DT, EDGE_MARGIN, RUN_SPEED, WALK_SPEED};

pub(crate) struct Pet {
    pub state: PetState,
    pub input: Option<InputFrame>,
    pub input_tick: u32,
    pub autonomous: bool,
}

impl Pet {
    /// Move o pet um tick na direção `(move_x, move_y)`; diagonais não são mais rápidas.
    pub fn advance(&mut self, move_x: i8, move_y: i8, run: bool, aspect: f32) {
        let (mut dir_x, mut dir_y) = (move_x as f32, move_y as f32);
        let len = (dir_x * dir_x + dir_y * dir_y).sqrt();
        if len == 0.0 {
            self.state.anim = AnimState::Idle;
            return;
        }
        dir_x /= len;
        dir_y /= len;

        let speed = if run { RUN_SPEED } else { WALK_SPEED };
        let dx = dir_x * speed * DT / aspect;
        let dy = dir_y * speed * DT;
        let pos = &mut self.state.pos;
        pos.x = (pos.x + dx).clamp(EDGE_MARGIN, 1.0 - EDGE_MARGIN);
        pos.y = (pos.y + dy).clamp(EDGE_MARGIN, 1.0 - EDGE_MARGIN);

        if dx > 0.0 {
            self.state.facing = Facing::Right;
        } else if dx < 0.0 {
            self.state.facing = Facing::Left;
        }
        self.state.anim = if run { AnimState::Run } else { AnimState::Walk };
    }
}
