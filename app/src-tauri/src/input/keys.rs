use dm_protocol::InputFrame;

/// Teclas que o modo bichinho usa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    W,
    A,
    S,
    D,
    Shift,
    Esc,
}

impl Key {
    /// Virtual-key code do Windows.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn from_vk(vk: u16) -> Option<Key> {
        match vk {
            0x57 => Some(Key::W),
            0x41 => Some(Key::A),
            0x53 => Some(Key::S),
            0x44 => Some(Key::D),
            0x10 | 0xA0 | 0xA1 => Some(Key::Shift),
            0x1B => Some(Key::Esc),
            _ => None,
        }
    }

    /// `KeyboardEvent.code` da webview (modo dev).
    pub fn from_code(code: &str) -> Option<Key> {
        match code {
            "KeyW" => Some(Key::W),
            "KeyA" => Some(Key::A),
            "KeyS" => Some(Key::S),
            "KeyD" => Some(Key::D),
            "ShiftLeft" | "ShiftRight" => Some(Key::Shift),
            "Escape" => Some(Key::Esc),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyState {
    pub w: bool,
    pub a: bool,
    pub s: bool,
    pub d: bool,
    pub shift: bool,
}

impl KeyState {
    pub fn set(&mut self, key: Key, down: bool) {
        match key {
            Key::W => self.w = down,
            Key::A => self.a = down,
            Key::S => self.s = down,
            Key::D => self.d = down,
            Key::Shift => self.shift = down,
            Key::Esc => {}
        }
    }

    /// Teclas opostas se anulam; y cresce para baixo.
    pub fn to_input(self, seq: u32) -> InputFrame {
        let axis = |neg: bool, pos: bool| pos as i8 - neg as i8;
        InputFrame {
            seq,
            move_x: axis(self.a, self.d),
            move_y: axis(self.w, self.s),
            run: self.shift,
        }
    }
}

/// O hook só engole teclas do modo bichinho, com ele ligado e sem Ctrl/Alt
/// (assim Alt+Tab e a própria hotkey continuam chegando ao sistema).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn should_swallow(pet_mode: bool, key: Option<Key>, ctrl_or_alt_down: bool) -> bool {
    pet_mode && key.is_some() && !ctrl_or_alt_down
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(keys: &[Key]) -> KeyState {
        let mut s = KeyState::default();
        for k in keys {
            s.set(*k, true);
        }
        s
    }

    #[test]
    fn wasd_mapping() {
        let f = state(&[Key::W, Key::D]).to_input(9);
        assert_eq!((f.seq, f.move_x, f.move_y, f.run), (9, 1, -1, false));
        assert_eq!(state(&[Key::A, Key::D]).to_input(1).move_x, 0);
        assert_eq!(state(&[Key::W, Key::S]).to_input(1).move_y, 0);
        assert!(state(&[Key::Shift]).to_input(1).run);

        let mut s = state(&[Key::D]);
        s.set(Key::D, false);
        assert_eq!(s.to_input(1).move_x, 0);
    }

    #[test]
    fn vk_and_code_mapping() {
        assert_eq!(Key::from_vk(0x57), Some(Key::W));
        assert_eq!(Key::from_vk(0x44), Some(Key::D));
        assert_eq!(Key::from_vk(0xA1), Some(Key::Shift));
        assert_eq!(Key::from_vk(0x1B), Some(Key::Esc));
        assert_eq!(Key::from_vk(0x5A), None);
        assert_eq!(Key::from_code("KeyA"), Some(Key::A));
        assert_eq!(Key::from_code("ShiftRight"), Some(Key::Shift));
        assert_eq!(Key::from_code("Escape"), Some(Key::Esc));
        assert_eq!(Key::from_code("KeyZ"), None);
    }

    #[test]
    fn swallow_rules() {
        assert!(!should_swallow(false, Some(Key::W), false));
        assert!(should_swallow(true, Some(Key::W), false));
        assert!(!should_swallow(true, Some(Key::W), true));
        assert!(!should_swallow(true, None, false));
    }
}
