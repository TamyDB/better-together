use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::input::keys::{Key, KeyState};
use crate::runtime::sim_loop::SimSink;

/// Intervalo do reenvio do input enquanto o modo está ligado (abaixo do timeout da sim).
pub const KEEP_ALIVE_MS: u64 = 100;
/// O reenvio só continua enquanto houver eventos reais de tecla recentes. Segurar uma
/// tecla gera autorepeat (atraso máx. de 1 s no Windows), então um key-up perdido para
/// o pet em vez de deixá-lo andando para sempre.
const KEY_FRESH: Duration = Duration::from_millis(1500);

/// "Modo bichinho": quando ligado, o teclado controla o pet do Host.
pub struct PetMode {
    sink: Arc<dyn SimSink>,
    on: AtomicBool,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    keys: KeyState,
    seq: u32,
    last_key_at: Option<Instant>,
}

impl PetMode {
    pub fn new(sink: Arc<dyn SimSink>) -> PetMode {
        PetMode {
            sink,
            on: AtomicBool::new(false),
            inner: Mutex::default(),
        }
    }

    /// Leitura sem lock, segura para o callback do hook.
    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Acquire)
    }

    pub fn toggle(&self) {
        if self.is_on() {
            self.exit();
        } else {
            let mut inner = self.inner.lock().unwrap();
            inner.keys = KeyState::default();
            self.on.store(true, Ordering::Release);
            self.sink.set_autonomous(false);
            log::info!("modo bichinho ligado");
        }
    }

    /// Sai do modo soltando todas as teclas e devolvendo o pet ao vagar.
    pub fn exit(&self) {
        if !self.on.swap(false, Ordering::AcqRel) {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.keys = KeyState::default();
        Self::send(&self.sink, &mut inner);
        self.sink.set_autonomous(true);
        log::info!("modo bichinho desligado");
    }

    pub fn on_key(&self, key: Key, down: bool) {
        self.on_key_at(key, down, Instant::now());
    }

    pub fn on_key_at(&self, key: Key, down: bool, now: Instant) {
        if key == Key::Esc {
            if down {
                self.exit();
            }
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        // Checado sob o lock para não sujar o estado depois de um `exit` concorrente.
        if !self.is_on() {
            return;
        }
        inner.keys.set(key, down);
        inner.last_key_at = Some(now);
        Self::send(&self.sink, &mut inner);
    }

    /// Reenvia o input atual para não expirar na sim enquanto uma tecla está segurada.
    pub fn keep_alive(&self) {
        self.keep_alive_at(Instant::now());
    }

    pub fn keep_alive_at(&self, now: Instant) {
        let mut inner = self.inner.lock().unwrap();
        let fresh = inner
            .last_key_at
            .is_some_and(|t| now.duration_since(t) <= KEY_FRESH);
        if self.is_on() && fresh {
            Self::send(&self.sink, &mut inner);
        }
    }

    fn send(sink: &Arc<dyn SimSink>, inner: &mut Inner) {
        inner.seq += 1;
        sink.send_input(inner.keys.to_input(inner.seq));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_protocol::InputFrame;
    use std::time::{Duration, Instant};

    #[derive(Debug, PartialEq)]
    enum Call {
        Input(i8, i8, bool),
        Autonomous(bool),
    }

    #[derive(Default)]
    struct FakeSink(Mutex<Vec<Call>>);

    impl SimSink for FakeSink {
        fn send_input(&self, f: InputFrame) {
            self.0
                .lock()
                .unwrap()
                .push(Call::Input(f.move_x, f.move_y, f.run));
        }
        fn set_autonomous(&self, on: bool) {
            self.0.lock().unwrap().push(Call::Autonomous(on));
        }
    }

    fn setup() -> (Arc<FakeSink>, PetMode) {
        let sink = Arc::new(FakeSink::default());
        (sink.clone(), PetMode::new(sink))
    }

    #[test]
    fn exit_releases() {
        let (sink, mode) = setup();
        mode.toggle();
        mode.on_key(Key::D, true);
        mode.on_key(Key::Esc, true);
        let calls = sink.0.lock().unwrap();
        assert_eq!(calls.first(), Some(&Call::Autonomous(false)));
        assert!(calls.contains(&Call::Input(1, 0, false)));
        let n = calls.len();
        assert_eq!(calls[n - 2], Call::Input(0, 0, false));
        assert_eq!(calls[n - 1], Call::Autonomous(true));
        assert!(!mode.is_on());
    }

    #[test]
    fn keys_ignored_when_off() {
        let (sink, mode) = setup();
        mode.on_key(Key::W, true);
        mode.keep_alive();
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[test]
    fn keep_alive_resends_held_input_with_new_seq() {
        let sink = Arc::new(SeqSink::default());
        let mode = PetMode::new(sink.clone());
        mode.toggle();
        mode.on_key(Key::A, true);
        mode.keep_alive();
        let seqs = sink.0.lock().unwrap();
        assert_eq!(seqs.len(), 2);
        assert!(seqs[1].0 > seqs[0].0);
        assert_eq!(seqs[1].1, -1);
    }

    #[test]
    fn keep_alive_stops_when_key_events_go_stale() {
        let sink = Arc::new(SeqSink::default());
        let mode = PetMode::new(sink.clone());
        let t0 = Instant::now();
        mode.toggle();
        mode.on_key_at(Key::D, true, t0);
        mode.keep_alive_at(t0 + Duration::from_millis(1000));
        assert_eq!(sink.0.lock().unwrap().len(), 2);
        // Key-up perdido: sem eventos reais há muito tempo, o reenvio para e a sim expira o input.
        mode.keep_alive_at(t0 + Duration::from_millis(2000));
        assert_eq!(sink.0.lock().unwrap().len(), 2);
    }

    #[derive(Default)]
    struct SeqSink(Mutex<Vec<(u32, i8)>>);

    impl SimSink for SeqSink {
        fn send_input(&self, f: InputFrame) {
            self.0.lock().unwrap().push((f.seq, f.move_x));
        }
        fn set_autonomous(&self, _: bool) {}
    }
}
