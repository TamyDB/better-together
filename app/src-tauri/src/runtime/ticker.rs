use std::time::Duration;

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
    use std::time::Duration;

    const STEP: Duration = Duration::from_nanos(1_000_000_000 / 60);

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
