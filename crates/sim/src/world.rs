use std::collections::BTreeMap;

use dm_protocol::{AnimState, Animal, Facing, InputFrame, PetState, PlayerId, Snapshot, Vec2};

use crate::consts::{INPUT_TIMEOUT_TICKS, MAX_PLAYERS};
use crate::pet::Pet;
use crate::rng::Rng;
use crate::wander::Wander;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SimError {
    #[error("sessão cheia")]
    Full,
    #[error("jogador já existe")]
    PlayerExists,
    #[error("animal já escolhido")]
    AnimalTaken,
}

/// Estado completo da sessão. O Host é a autoridade; Convidados usam para predição.
pub struct World {
    aspect: f32,
    tick: u32,
    pets: BTreeMap<PlayerId, Pet>,
    rng: Rng,
}

impl World {
    /// `aspect` = largura / altura do monitor do Host (> 0).
    pub fn new(aspect: f32, seed: u64) -> World {
        World {
            aspect,
            tick: 0,
            pets: BTreeMap::new(),
            rng: Rng::new(seed),
        }
    }

    pub fn spawn(&mut self, id: PlayerId, animal: Animal, pos: Vec2) -> Result<(), SimError> {
        if self.pets.contains_key(&id) {
            return Err(SimError::PlayerExists);
        }
        if self.pets.len() >= MAX_PLAYERS {
            return Err(SimError::Full);
        }
        if self.pets.values().any(|p| p.state.animal == animal) {
            return Err(SimError::AnimalTaken);
        }
        let state = PetState {
            id,
            animal,
            pos,
            facing: Facing::Right,
            anim: AnimState::Idle,
            last_input_seq: 0,
        };
        self.pets.insert(
            id,
            Pet {
                state,
                input: None,
                input_tick: 0,
                autonomous: false,
                wander: Wander::default(),
            },
        );
        Ok(())
    }

    pub fn despawn(&mut self, id: PlayerId) {
        self.pets.remove(&id);
    }

    /// Registra o input mais recente; `seq` repetido ou antigo é ignorado.
    pub fn apply_input(&mut self, id: PlayerId, input: InputFrame) {
        let Some(pet) = self.pets.get_mut(&id) else {
            return;
        };
        if input.seq <= pet.state.last_input_seq {
            return;
        }
        pet.state.last_input_seq = input.seq;
        pet.input = Some(input.sanitized());
        pet.input_tick = self.tick;
    }

    pub fn set_autonomous(&mut self, id: PlayerId, on: bool) {
        if let Some(pet) = self.pets.get_mut(&id) {
            pet.autonomous = on;
        }
    }

    pub fn step(&mut self) {
        for pet in self.pets.values_mut() {
            let fresh = pet
                .input
                .filter(|_| self.tick - pet.input_tick <= INPUT_TIMEOUT_TICKS);
            match fresh {
                Some(i) => pet.advance(i.move_x, i.move_y, i.run, self.aspect),
                None if pet.autonomous => {
                    let (x, y) = pet.wander.next_input(pet.state.pos, &mut self.rng);
                    pet.advance(x, y, false, self.aspect);
                }
                None => pet.advance(0, 0, false, self.aspect),
            }
        }
        self.tick += 1;
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            pets: self.pets.values().map(|p| p.state.clone()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_protocol::{AnimState, Animal, Facing, InputFrame, PlayerId, Vec2};

    const EPS: f32 = 1e-4;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    fn input(seq: u32, move_x: i8, move_y: i8) -> InputFrame {
        InputFrame {
            seq,
            move_x,
            move_y,
            run: false,
        }
    }

    /// Simula um remetente a 60 Hz: reaplica o mesmo input a cada tick.
    fn hold(world: &mut World, id: PlayerId, move_x: i8, move_y: i8, ticks: u32) {
        for seq in 1..=ticks {
            world.apply_input(id, input(seq, move_x, move_y));
            world.step();
        }
    }

    fn pos(world: &World, id: PlayerId) -> Vec2 {
        world
            .snapshot()
            .pets
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .pos
    }

    #[test]
    fn spawn_limits() {
        let mut w = World::new(1.0, 1);
        for (i, a) in Animal::ALL.iter().enumerate() {
            w.spawn(PlayerId(i as u8), *a, v(0.5, 0.5)).unwrap();
        }
        assert_eq!(
            w.spawn(PlayerId(9), Animal::Duck, v(0.5, 0.5)),
            Err(SimError::Full)
        );

        let mut w = World::new(1.0, 1);
        w.spawn(PlayerId(0), Animal::Duck, v(0.5, 0.5)).unwrap();
        assert_eq!(
            w.spawn(PlayerId(0), Animal::Horse, v(0.5, 0.5)),
            Err(SimError::PlayerExists)
        );
        assert_eq!(
            w.spawn(PlayerId(1), Animal::Duck, v(0.5, 0.5)),
            Err(SimError::AnimalTaken)
        );
    }

    #[test]
    fn moves_right_at_walk_speed() {
        let mut w = World::new(1.0, 1);
        w.spawn(PlayerId(0), Animal::Duck, v(0.5, 0.5)).unwrap();
        hold(&mut w, PlayerId(0), 1, 0, 60);
        let p = &w.snapshot().pets[0];
        assert!((p.pos.x - 0.65).abs() < EPS, "x = {}", p.pos.x);
        assert!((p.pos.y - 0.5).abs() < EPS);
        assert_eq!(p.facing, Facing::Right);
        assert_eq!(p.anim, AnimState::Walk);
    }

    #[test]
    fn isotropic_on_ultrawide() {
        let aspect = 21.0 / 9.0;
        let mut w = World::new(aspect, 1);
        w.spawn(PlayerId(0), Animal::Duck, v(0.2, 0.2)).unwrap();
        w.spawn(PlayerId(1), Animal::Horse, v(0.2, 0.2)).unwrap();
        for seq in 1..=60 {
            w.apply_input(PlayerId(0), input(seq, 1, 0));
            w.apply_input(PlayerId(1), input(seq, 0, 1));
            w.step();
        }
        let dx = pos(&w, PlayerId(0)).x - 0.2;
        let dy = pos(&w, PlayerId(1)).y - 0.2;
        assert!(
            (dx * aspect - dy).abs() < EPS,
            "dx*aspect={} dy={}",
            dx * aspect,
            dy
        );
    }

    #[test]
    fn diagonal_not_faster() {
        let aspect = 16.0 / 9.0;
        let mut w = World::new(aspect, 1);
        w.spawn(PlayerId(0), Animal::Duck, v(0.2, 0.2)).unwrap();
        hold(&mut w, PlayerId(0), 1, 1, 60);
        let p = pos(&w, PlayerId(0));
        let dist = (((p.x - 0.2) * aspect).powi(2) + (p.y - 0.2).powi(2)).sqrt();
        assert!((dist - 0.15).abs() < EPS, "dist = {dist}");
    }

    #[test]
    fn clamps_at_edges() {
        let mut w = World::new(1.0, 1);
        w.spawn(PlayerId(0), Animal::Duck, v(0.99, 0.01)).unwrap();
        hold(&mut w, PlayerId(0), 1, -1, 120);
        assert_eq!(pos(&w, PlayerId(0)), v(1.0, 0.0));
    }

    #[test]
    fn stale_input_stops() {
        let mut w = World::new(1.0, 1);
        w.spawn(PlayerId(0), Animal::Duck, v(0.2, 0.5)).unwrap();
        w.apply_input(PlayerId(0), input(1, 1, 0));
        for _ in 0..40 {
            w.step();
        }
        let before = pos(&w, PlayerId(0));
        assert!(before.x > 0.2, "deveria ter andado antes de expirar");
        for _ in 0..10 {
            w.step();
        }
        assert_eq!(pos(&w, PlayerId(0)), before);
        assert_eq!(w.snapshot().pets[0].anim, AnimState::Idle);
    }

    #[test]
    fn old_seq_ignored() {
        let mut w = World::new(1.0, 1);
        w.spawn(PlayerId(0), Animal::Duck, v(0.5, 0.5)).unwrap();
        w.apply_input(PlayerId(0), input(5, 1, 0));
        w.apply_input(PlayerId(0), input(3, -1, 0));
        w.step();
        let p = &w.snapshot().pets[0];
        assert_eq!(p.last_input_seq, 5);
        assert_eq!(p.facing, Facing::Right);
    }

    #[test]
    fn snapshot_sorted() {
        let mut w = World::new(1.0, 1);
        w.spawn(PlayerId(3), Animal::Duck, v(0.5, 0.5)).unwrap();
        w.spawn(PlayerId(1), Animal::Horse, v(0.5, 0.5)).unwrap();
        w.spawn(PlayerId(2), Animal::Chicken, v(0.5, 0.5)).unwrap();
        let ids: Vec<u8> = w.snapshot().pets.iter().map(|p| p.id.0).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(w.snapshot().tick, 0);
        w.step();
        w.step();
        assert_eq!(w.snapshot().tick, 2);
    }
}

#[cfg(test)]
mod autonomous_tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

    #[test]
    fn wander_eventually_moves_and_stays_in_bounds() {
        let mut w = World::new(16.0 / 9.0, 3);
        w.spawn(PlayerId(0), Animal::Duck, v(0.5, 0.5)).unwrap();
        w.set_autonomous(PlayerId(0), true);
        let mut moved = false;
        for _ in 0..3600 {
            w.step();
            let p = w.snapshot().pets[0].pos;
            moved |= p != v(0.5, 0.5);
            assert!(
                (0.04..=0.96).contains(&p.x) && (0.04..=0.96).contains(&p.y),
                "{p:?}"
            );
        }
        assert!(moved);
    }

    #[test]
    fn world_deterministic() {
        let run = || {
            let mut w = World::new(16.0 / 9.0, 7);
            w.spawn(PlayerId(0), Animal::Duck, v(0.5, 0.8)).unwrap();
            w.spawn(PlayerId(1), Animal::Chicken, v(0.3, 0.3)).unwrap();
            w.set_autonomous(PlayerId(0), true);
            for t in 0..1000u32 {
                if t % 7 == 0 {
                    let m = ((t / 7) % 3) as i8 - 1;
                    w.apply_input(
                        PlayerId(1),
                        InputFrame {
                            seq: t + 1,
                            move_x: m,
                            move_y: -m,
                            run: t % 2 == 0,
                        },
                    );
                }
                w.step();
            }
            w.snapshot()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn input_overrides_wander() {
        let mut w = World::new(1.0, 5);
        w.spawn(PlayerId(0), Animal::Duck, v(0.5, 0.5)).unwrap();
        w.set_autonomous(PlayerId(0), true);
        w.apply_input(
            PlayerId(0),
            InputFrame {
                seq: 1,
                move_x: -1,
                move_y: 0,
                run: false,
            },
        );
        w.step();
        let p = &w.snapshot().pets[0];
        assert_eq!(p.facing, Facing::Left);
        assert_eq!(p.anim, AnimState::Walk);
    }
}
