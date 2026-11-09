use std::thread;
use std::time::{Duration, Instant};

use dm_sim::consts::TICK_HZ;

const MAX_CATCH_UP: u32 = 5;

/// O que um loop faz a cada passagem. Separado do relógio para os testes não dormirem.
pub trait TickLoop {
    /// Processa os comandos pendentes; `false` encerra o loop (canal de comandos fechado).
    fn drain(&mut self, now: Instant) -> bool;
    /// Executa os ticks devidos; `ticks` pode ser 0.
    fn run_ticks(&mut self, ticks: u32, now: Instant);
}

/// Roda o loop a `TICK_HZ` até `drain` indicar que o canal de comandos fechou.
pub fn run_tick_loop(body: &mut impl TickLoop) {
    let mut ticker = Ticker::new(
        Duration::from_nanos(1_000_000_000 / TICK_HZ as u64),
        MAX_CATCH_UP,
    );
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        if !body.drain(now) {
            return;
        }
        let ticks = ticker.advance(now - last);
        last = now;
        body.run_ticks(ticks, now);
        thread::sleep(ticker.time_to_next());
    }
}

/// Timestep fixo: converte tempo real decorrido em quantidade de ticks a simular.
pub struct Ticker {
    step: Duration,
    max_catch_up: u32,
    acc: Duration,
}

impl Ticker {
    pub fn new(step: Duration, max_catch_up: u32) -> Ticker {
        Ticker {
            step,
            max_catch_up,
            acc: Duration::ZERO,
        }
    }

    /// Ticks a executar agora. Acima de `max_catch_up` o atraso é descartado,
    /// evitando espiral e "teletransporte" após uma travada.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        self.acc += elapsed;
        let mut ticks = 0;
        while self.acc >= self.step {
            if ticks == self.max_catch_up {
                self.acc = Duration::ZERO;
                break;
            }
            self.acc -= self.step;
            ticks += 1;
        }
        ticks
    }

    /// Quanto falta até o próximo tick.
    pub fn time_to_next(&self) -> Duration {
        self.step.saturating_sub(self.acc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: Duration = Duration::from_nanos(1_000_000_000 / 60);

    struct StopAfter {
        passes: u32,
        ticks_calls: u32,
    }

    impl TickLoop for StopAfter {
        fn drain(&mut self, _: Instant) -> bool {
            if self.passes == 0 {
                return false;
            }
            self.passes -= 1;
            true
        }
        fn run_ticks(&mut self, _: u32, _: Instant) {
            self.ticks_calls += 1;
        }
    }

    #[test]
    fn loop_stops_when_drain_reports_closed() {
        let mut body = StopAfter {
            passes: 2,
            ticks_calls: 0,
        };
        run_tick_loop(&mut body);
        assert_eq!(body.ticks_calls, 2);
    }

    #[test]
    fn exact_steps() {
        let mut t = Ticker::new(STEP, 5);
        assert_eq!(t.advance(Duration::from_millis(50)), 3);
        assert_eq!(t.advance(Duration::ZERO), 0);
    }

    #[test]
    fn accumulates_remainder() {
        let mut t = Ticker::new(STEP, 5);
        assert_eq!(t.advance(Duration::from_millis(10)), 0);
        assert_eq!(t.advance(Duration::from_millis(10)), 1);
    }

    #[test]
    fn caps_catch_up() {
        let mut t = Ticker::new(STEP, 5);
        assert_eq!(t.advance(Duration::from_secs(10)), 5);
        assert_eq!(t.advance(Duration::ZERO), 0);
    }
}
