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

/// O que o hook faz com um evento de tecla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct HookDecision {
    /// Não repassar ao sistema.
    pub swallow: bool,
    /// Entregar ao `PetMode`.
    pub forward: bool,
}

fn swallow_bit(key: Key) -> u8 {
    match key {
        Key::W => 1,
        Key::A => 1 << 1,
        Key::S => 1 << 2,
        Key::D => 1 << 3,
        Key::Esc => 1 << 4,
        Key::Shift => 0,
    }
}

/// Decide um evento do hook. `swallowed` guarda quais teclas tiveram o key-down engolido.
///
/// - Key-down só é engolido com o modo ligado e sem Ctrl/Alt/Win (Alt+Tab etc. funcionam).
/// - Key-up só é engolido se o key-down correspondente foi; senão o sistema ficaria com a
///   tecla "presa".
/// - Shift nunca é engolido (para não quebrar Ctrl+Shift+P e Shift+Alt+Tab); o pet só o lê.
/// - Key-ups sempre vão ao `PetMode`, para o movimento não ficar preso.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn filter_key(
    swallowed: u8,
    pet_mode: bool,
    key: Option<Key>,
    down: bool,
    modifier_down: bool,
) -> (u8, HookDecision) {
    let Some(key) = key else {
        return (
            swallowed,
            HookDecision {
                swallow: false,
                forward: false,
            },
        );
    };
    let bit = swallow_bit(key);
    if !down {
        let swallow = swallowed & bit != 0;
        return (
            swallowed & !bit,
            HookDecision {
                swallow,
                forward: true,
            },
        );
    }
    if key == Key::Shift {
        return (
            swallowed,
            HookDecision {
                swallow: false,
                forward: pet_mode,
            },
        );
    }
    if pet_mode && !modifier_down {
        (
            swallowed | bit,
            HookDecision {
                swallow: true,
                forward: true,
            },
        )
    } else {
        (
            swallowed,
            HookDecision {
                swallow: false,
                forward: false,
            },
        )
    }
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

    fn on(state: u8, key: Key, down: bool, modifier: bool) -> (u8, HookDecision) {
        filter_key(state, true, Some(key), down, modifier)
    }

    #[test]
    fn swallow_rules() {
        assert!(!filter_key(0, false, Some(Key::W), true, false).1.swallow);
        assert!(on(0, Key::W, true, false).1.swallow);
        assert!(!on(0, Key::W, true, true).1.swallow);
        assert!(!filter_key(0, true, None, true, false).1.swallow);
        assert!(on(0, Key::Esc, true, false).1.swallow);
    }

    #[test]
    fn up_passes_when_its_down_passed() {
        // Ctrl+W com o modo ligado: o W-down passa; o W-up (Ctrl já solto) também precisa passar.
        let (s, d) = on(0, Key::W, true, true);
        assert!(!d.swallow);
        assert!(!on(s, Key::W, false, false).1.swallow);
    }

    #[test]
    fn up_swallowed_when_its_down_was() {
        let (s, d) = on(0, Key::D, true, false);
        assert!(d.swallow && d.forward);
        let (s, d) = on(s, Key::D, false, true);
        assert!(d.swallow && d.forward);
        assert_eq!(s, 0);
    }

    #[test]
    fn shift_never_swallowed_but_forwarded() {
        let (s, d) = on(0, Key::Shift, true, false);
        assert!(!d.swallow && d.forward);
        let (_, d) = on(s, Key::Shift, false, false);
        assert!(!d.swallow && d.forward);
    }

    #[test]
    fn ups_always_forwarded() {
        // Solta com modificador ainda atualiza o estado do pet (não "prende" o movimento).
        assert!(on(0, Key::A, false, true).1.forward);
    }
}
