use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use dm_protocol::{Animal, InputFrame, PlayerId, Snapshot, Vec2};
use dm_sim::{consts::TICK_HZ, World};
use tauri::{AppHandle, Emitter};

use super::ticker::Ticker;

pub const HOST_PLAYER: PlayerId = PlayerId(0);
pub const SNAPSHOT_EVENT: &str = "sim://snapshot";
const MAX_CATCH_UP: u32 = 5;

/// Destino de comandos para a simulação; injetável para testes.
pub trait SimSink: Send + Sync {
    fn send_input(&self, frame: InputFrame);
    fn set_autonomous(&self, on: bool);
}

enum Command {
    Input(InputFrame),
    Autonomous(bool),
}

/// Ponta de envio para a thread da simulação.
pub struct SimHandle {
    tx: Sender<Command>,
}

impl SimSink for SimHandle {
    fn send_input(&self, frame: InputFrame) {
        let _ = self.tx.send(Command::Input(frame));
    }

    fn set_autonomous(&self, on: bool) {
        let _ = self.tx.send(Command::Autonomous(on));
    }
}

/// Inicia a thread da simulação com o pet do Host e emite um snapshot após cada lote de ticks.
pub fn spawn_sim_loop(app: AppHandle, aspect: f32, seed: u64) -> SimHandle {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("sim".into())
        .spawn(move || {
            run_loop(rx, aspect, seed, |snap| {
                if let Err(e) = app.emit(SNAPSHOT_EVENT, snap) {
                    log::warn!("falha ao emitir snapshot: {e}");
                }
            })
        })
        .expect("falha ao criar a thread da simulação");
    SimHandle { tx }
}

fn run_loop(rx: Receiver<Command>, aspect: f32, seed: u64, emit: impl Fn(&Snapshot)) {
    let mut world = World::new(aspect, seed);
    world
        .spawn(HOST_PLAYER, Animal::Duck, Vec2 { x: 0.5, y: 0.8 })
        .expect("mundo vazio aceita o pet do Host");
    world.set_autonomous(HOST_PLAYER, true);

    let mut ticker = Ticker::new(
        Duration::from_nanos(1_000_000_000 / TICK_HZ as u64),
        MAX_CATCH_UP,
    );
    let mut last = Instant::now();
    loop {
        loop {
            match rx.try_recv() {
                Ok(Command::Input(frame)) => world.apply_input(HOST_PLAYER, frame),
                Ok(Command::Autonomous(on)) => world.set_autonomous(HOST_PLAYER, on),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        let now = Instant::now();
        let ticks = ticker.advance(now - last);
        last = now;
        for _ in 0..ticks {
            world.step();
        }
        if ticks > 0 {
            emit(&world.snapshot());
        }
        thread::sleep(ticker.time_to_next());
    }
}
