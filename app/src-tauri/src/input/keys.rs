use dm_protocol::{Emote, InputFrame};

/// Teclas que o modo bichinho e o viewer do convidado usam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    W,
    A,
    S,
    D,
    Shift,
    Esc,
    /// `1`–`6`, na ordem de `Emote::ALL`. Não é tecla de movimento.
    Emote(Emote),
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
            0x31..=0x36 => Emote::ALL
                .get(usize::from(vk - 0x31))
                .copied()
                .map(Key::Emote),
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
            "Digit1" => Some(Key::Emote(Emote::Heart)),
            "Digit2" => Some(Key::Emote(Emote::Laugh)),
            "Digit3" => Some(Key::Emote(Emote::Wow)),
            "Digit4" => Some(Key::Emote(Emote::Angry)),
            "Digit5" => Some(Key::Emote(Emote::Music)),
            "Digit6" => Some(Key::Emote(Emote::Question)),
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
            Key::Esc | Key::Emote(_) => {}
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

/// Teclas de emote seguradas. O emote sai só no key-down que vem depois de um key-up da
/// mesma tecla: o autorepeat de quem segura não reenvia.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmoteKeys(u8);

impl EmoteKeys {
    /// Registra a tecla e diz se é um key-down novo (a tecla estava solta).
    pub fn press(&mut self, emote: Emote, down: bool) -> bool {
        let bit = 1u8 << emote_index(emote);
        let fresh = down && self.0 & bit == 0;
        if down {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
        fresh
    }
}

/// Posição em `Emote::ALL` (0..6).
fn emote_index(emote: Emote) -> u8 {
    match emote {
        Emote::Heart => 0,
        Emote::Laugh => 1,
        Emote::Wow => 2,
        Emote::Angry => 3,
        Emote::Music => 4,
        Emote::Question => 5,
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

fn swallow_bit(key: Key) -> u16 {
    match key {
        Key::W => 1,
        Key::A => 1 << 1,
        Key::S => 1 << 2,
        Key::D => 1 << 3,
        Key::Esc => 1 << 4,
        Key::Emote(emote) => 1 << (5 + emote_index(emote)),
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
/// - Os dígitos `1`–`6` (emotes) seguem as mesmas regras de WASD.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn filter_key(
    swallowed: u16,
    pet_mode: bool,
    key: Option<Key>,
    down: bool,
    modifier_down: bool,
) -> (u16, HookDecision) {
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
    use dm_protocol::Emote;

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
    fn digits_are_emotes_in_order() {
        let codes = ["Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6"];
        for (i, emote) in Emote::ALL.into_iter().enumerate() {
            assert_eq!(Key::from_code(codes[i]), Some(Key::Emote(emote)));
            assert_eq!(Key::from_vk(0x31 + i as u16), Some(Key::Emote(emote)));
            assert_eq!(usize::from(emote_index(emote)), i);
        }
        assert_eq!(Key::from_code("Digit1"), Some(Key::Emote(Emote::Heart)));
        assert_eq!(Key::from_code("Digit6"), Some(Key::Emote(Emote::Question)));
        assert_eq!(Key::from_code("Digit7"), None);
        assert_eq!(Key::from_code("Digit0"), None);
        assert_eq!(Key::from_code("Numpad1"), None);
        assert_eq!(Key::from_vk(0x30), None);
        assert_eq!(Key::from_vk(0x37), None);
    }

    #[test]
    fn emote_does_not_touch_movement() {
        let mut s = state(&[Key::D, Key::Shift]);
        let before = s;
        for emote in Emote::ALL {
            s.set(Key::Emote(emote), true);
            s.set(Key::Emote(emote), false);
        }
        assert_eq!(s, before);
        assert_eq!(state(&[Key::Emote(Emote::Heart)]), KeyState::default());
    }

    #[test]
    fn emote_keys_fire_on_edge_only() {
        let mut held = EmoteKeys::default();
        assert!(held.press(Emote::Wow, true));
        // Autorepeat: a tecla segue segurada, nada novo.
        assert!(!held.press(Emote::Wow, true));
        // Outra tecla é independente.
        assert!(held.press(Emote::Heart, true));
        assert!(!held.press(Emote::Wow, false));
        assert!(held.press(Emote::Wow, true));
        assert!(!held.press(Emote::Heart, true));
        // Key-up sem key-down antes não faz nada de errado.
        assert!(!held.press(Emote::Question, false));
        for emote in Emote::ALL {
            let mut held = EmoteKeys::default();
            assert!(held.press(emote, true), "{emote:?}");
            assert!(!held.press(emote, true), "{emote:?}");
        }
    }

    #[test]
    fn swallow_bits_are_distinct() {
        let keys = [Key::W, Key::A, Key::S, Key::D, Key::Esc]
            .into_iter()
            .chain(Emote::ALL.into_iter().map(Key::Emote));
        let mut all = 0u16;
        for key in keys {
            let bit = swallow_bit(key);
            assert_eq!(bit.count_ones(), 1, "{key:?}");
            assert_eq!(all & bit, 0, "{key:?}");
            all |= bit;
        }
        assert_eq!(swallow_bit(Key::Shift), 0);
    }

    #[test]
    fn hook_swallows_emote_keys_in_pet_mode() {
        for emote in Emote::ALL {
            let key = Key::Emote(emote);
            // Modo ligado, sem modificador: engolida e repassada, como WASD.
            let (s, d) = on(0b1_0101, key, true, false);
            assert!(d.swallow && d.forward, "{emote:?}");
            assert_eq!(s, 0b1_0101 | swallow_bit(key));
            let (s, d) = on(s, key, false, false);
            assert!(d.swallow && d.forward, "{emote:?}");
            assert_eq!(s, 0b1_0101);
        }
        let key = Key::Emote(Emote::Wow);
        // Com Ctrl/Alt/Win o dígito vai para o sistema; o key-up também.
        let (s, d) = on(0, key, true, true);
        assert!(!d.swallow && !d.forward);
        let (s, d) = on(s, key, false, false);
        assert!(!d.swallow && d.forward);
        assert_eq!(s, 0);
        // Modo desligado: o dígito é do sistema.
        let (s, d) = filter_key(0, false, Some(key), true, false);
        assert!(!d.swallow && !d.forward);
        let (s, d) = filter_key(s, false, Some(key), false, false);
        assert!(!d.swallow && d.forward);
        assert_eq!(s, 0);
        // Modo desligado no meio: o key-up do dígito engolido ainda é engolido.
        let (s, _) = on(0, key, true, false);
        let (s, d) = filter_key(s, false, Some(key), false, false);
        assert!(d.swallow);
        assert_eq!(s, 0);
    }

    fn on(state: u16, key: Key, down: bool, modifier: bool) -> (u16, HookDecision) {
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
