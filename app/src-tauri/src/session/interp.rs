//! Interpolação dos pets dos outros jogadores entre snapshots do Host.

use std::collections::{BTreeMap, VecDeque};

use dm_protocol::{PetState, PlayerId, Snapshot, Vec2};

/// Atraso de render em ticks: 2 snapshots (a 20 Hz) de folga contra jitter.
pub const INTERP_DELAY_TICKS: u32 = 6;
/// Snapshots guardados.
pub const BUFFER: usize = 32;

/// Últimos snapshots do Host, em ordem crescente de tick.
#[derive(Default)]
pub struct Interpolator {
    buf: VecDeque<Snapshot>,
}

impl Interpolator {
    pub fn new() -> Interpolator {
        Interpolator::default()
    }

    /// Guarda o snapshot; tick menor ou igual ao último (fora de ordem ou repetido) é ignorado.
    pub fn push(&mut self, s: Snapshot) {
        if self.buf.back().is_some_and(|last| s.tick <= last.tick) {
            return;
        }
        if self.buf.len() == BUFFER {
            self.buf.pop_front();
        }
        self.buf.push_back(s);
    }

    /// Pets em `render_tick`, ordenados por id: `pos` interpolada entre os 2 snapshots que o
    /// envolvem; `facing`, `anim` e `last_input_seq` do mais novo. Fora da faixa (ou NaN),
    /// fixa no snapshot da ponta, sem extrapolar. Pet presente em só um dos dois: usa esse.
    pub fn sample(&self, render_tick: f32) -> Vec<PetState> {
        let Some(last) = self.buf.back() else {
            return Vec::new();
        };
        let render_tick = f64::from(render_tick);
        let Some(i) = self
            .buf
            .iter()
            .position(|s| f64::from(s.tick) > render_tick)
        else {
            return by_id(&last.pets).into_values().cloned().collect();
        };
        let Some(older) = i.checked_sub(1).and_then(|j| self.buf.get(j)) else {
            return by_id(&self.buf[i].pets).into_values().cloned().collect();
        };
        let newer = &self.buf[i];
        // `push` garante newer.tick > older.tick.
        let span = f64::from(newer.tick) - f64::from(older.tick);
        let t = ((render_tick - f64::from(older.tick)) / span).clamp(0.0, 1.0) as f32;

        let mut pets = by_id(&older.pets);
        let mut out = BTreeMap::new();
        for (id, b) in by_id(&newer.pets) {
            let mut pet = b.clone();
            if let Some(a) = pets.remove(&id) {
                pet.pos = lerp(a.pos, b.pos, t);
            }
            out.insert(id, pet);
        }
        out.extend(pets.into_iter().map(|(id, a)| (id, a.clone())));
        out.into_values().collect()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.buf.len()
    }
}

/// Pets indexados por id; id repetido no snapshot fica com o último.
fn by_id(pets: &[PetState]) -> BTreeMap<PlayerId, &PetState> {
    pets.iter().map(|p| (p.id, p)).collect()
}

fn lerp(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    Vec2 {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_protocol::{AnimState, Animal, Facing, PlayerId, Vec2};

    fn pet(id: u8, x: f32, facing: Facing, seq: u32) -> PetState {
        PetState {
            id: PlayerId(id),
            animal: Animal::ALL[usize::from(id) % 4],
            pos: Vec2 { x, y: 0.5 },
            facing,
            anim: AnimState::Walk,
            last_input_seq: seq,
        }
    }

    fn snap(tick: u32, pets: Vec<PetState>) -> Snapshot {
        Snapshot { tick, pets }
    }

    /// Tick 0 com o pet 0 em x=0.1 e tick 3 com ele em x=0.4.
    fn two() -> Interpolator {
        let mut i = Interpolator::new();
        i.push(snap(0, vec![pet(0, 0.1, Facing::Left, 1)]));
        i.push(snap(3, vec![pet(0, 0.4, Facing::Right, 2)]));
        i
    }

    fn x_of(pets: &[PetState], id: u8) -> f32 {
        pets.iter().find(|p| p.id == PlayerId(id)).unwrap().pos.x
    }

    #[test]
    fn interp_midpoint() {
        let pets = two().sample(1.5);
        assert_eq!(pets.len(), 1);
        assert!((x_of(&pets, 0) - 0.25).abs() < 1e-6, "{pets:?}");
        // Discretos vêm do mais novo.
        assert_eq!(pets[0].facing, Facing::Right);
        assert_eq!(pets[0].last_input_seq, 2);
    }

    #[test]
    fn interp_clamps() {
        let i = two();
        assert!((x_of(&i.sample(10.0), 0) - 0.4).abs() < 1e-6);
        assert!((x_of(&i.sample(-5.0), 0) - 0.1).abs() < 1e-6);
        assert!((x_of(&i.sample(f32::NAN), 0) - 0.4).abs() < 1e-6);
        assert!(Interpolator::new().sample(1.0).is_empty());
    }

    #[test]
    fn old_ticks_ignored_and_buffer_bounded() {
        let mut i = two();
        i.push(snap(3, vec![pet(0, 0.9, Facing::Left, 9)]));
        i.push(snap(1, vec![pet(0, 0.9, Facing::Left, 9)]));
        assert!((x_of(&i.sample(1.5), 0) - 0.25).abs() < 1e-6);

        for t in 4..200 {
            i.push(snap(t, vec![pet(0, 0.5, Facing::Left, t)]));
        }
        assert_eq!(i.len(), BUFFER);
        // O mais antigo guardado é 199 - 31 = 168: abaixo disso fixa nele.
        assert_eq!(i.sample(0.0)[0].last_input_seq, 168);
    }

    #[test]
    fn missing_pet_uses_the_one_present() {
        let mut i = Interpolator::new();
        i.push(snap(
            0,
            vec![pet(0, 0.1, Facing::Left, 1), pet(1, 0.7, Facing::Left, 1)],
        ));
        i.push(snap(
            3,
            vec![pet(0, 0.4, Facing::Left, 2), pet(2, 0.3, Facing::Left, 1)],
        ));
        let pets = i.sample(1.5);
        let ids: Vec<u8> = pets.iter().map(|p| p.id.0).collect();
        assert_eq!(ids, vec![0, 1, 2]);
        assert!((x_of(&pets, 1) - 0.7).abs() < 1e-6);
        assert!((x_of(&pets, 2) - 0.3).abs() < 1e-6);
    }
}
