use serde::{Deserialize, Serialize};

/// Identificador de jogador dentro de uma sessão (0 = Host).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub u8);

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Animal {
    Duck,
    Capybara,
    Horse,
    Chicken,
}

impl Animal {
    pub const ALL: [Animal; 4] = [
        Animal::Duck,
        Animal::Capybara,
        Animal::Horse,
        Animal::Chicken,
    ];
}

/// Posição normalizada em [0,1] relativa ao monitor primário do Host.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Facing {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AnimState {
    Idle,
    Walk,
    Run,
}

/// Input de um jogador em um instante; `move_*` ∈ {-1, 0, 1} após `sanitized`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFrame {
    pub seq: u32,
    pub move_x: i8,
    pub move_y: i8,
    pub run: bool,
}

impl InputFrame {
    /// Garante direções válidas vindas de fontes não confiáveis.
    pub fn sanitized(self) -> InputFrame {
        InputFrame {
            move_x: self.move_x.clamp(-1, 1),
            move_y: self.move_y.clamp(-1, 1),
            ..self
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PetState {
    pub id: PlayerId,
    pub animal: Animal,
    pub pos: Vec2,
    pub facing: Facing,
    pub anim: AnimState,
    pub last_input_seq: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub tick: u32,
    pub pets: Vec<PetState>,
}
