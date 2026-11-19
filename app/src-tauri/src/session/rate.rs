//! Limites de taxa puros, com tempo injetado: nunca leem o relógio nem entram em panic com
//! um `now` anterior ao último visto.

use std::time::{Duration, Instant};

/// Balde de fichas: começa cheio com `capacity` e ganha 1 ficha a cada `refill`, sem passar
/// de `capacity`. A recarga conta a partir do primeiro gasto com o balde cheio.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    capacity: u32,
    refill: Duration,
    tokens: u32,
    /// Início da recarga da próxima ficha; `None` enquanto o balde nunca foi usado.
    since: Option<Instant>,
}

impl TokenBucket {
    pub fn new(capacity: u32, refill: Duration) -> TokenBucket {
        TokenBucket {
            capacity,
            refill,
            tokens: capacity,
            since: None,
        }
    }

    /// Gasta uma ficha se houver.
    pub fn allow(&mut self, now: Instant) -> bool {
        self.recharge(now);
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }

    fn recharge(&mut self, now: Instant) {
        let since = match self.since {
            Some(since) if self.tokens < self.capacity && !self.refill.is_zero() => since,
            _ => {
                // Cheio (ou recarga instantânea): a contagem recomeça agora.
                if self.refill.is_zero() {
                    self.tokens = self.capacity;
                }
                self.since = Some(now);
                return;
            }
        };
        // `now` anterior a `since` (relógio voltando) vale como zero.
        let elapsed = now.saturating_duration_since(since);
        let earned = elapsed.as_nanos() / self.refill.as_nanos();
        if earned == 0 {
            return;
        }
        let missing = self.capacity - self.tokens;
        let earned = u32::try_from(earned).unwrap_or(u32::MAX);
        if earned >= missing {
            self.tokens = self.capacity;
            self.since = Some(now);
        } else {
            self.tokens += earned;
            // Guarda a fração de ficha já recarregada; `earned < capacity`, sem estouro.
            self.since = self
                .refill
                .checked_mul(earned)
                .and_then(|d| since.checked_add(d))
                .or(Some(now));
        }
    }
}

/// Intervalo mínimo `min` entre dois usos aceitos. Um uso barrado não reinicia a contagem.
#[derive(Debug, Clone)]
pub struct Cooldown {
    min: Duration,
    last: Option<Instant>,
}

impl Cooldown {
    pub fn new(min: Duration) -> Cooldown {
        Cooldown { min, last: None }
    }

    /// Aceita se o último uso aceito foi há pelo menos `min`.
    pub fn allow(&mut self, now: Instant) -> bool {
        // `now` anterior a `last` (relógio voltando) vale como zero: barrado, sem panic.
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < self.min)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Duration = Duration::from_secs(1);

    fn allowed(bucket: &mut TokenBucket, now: Instant, tries: u32) -> u32 {
        (0..tries).filter(|_| bucket.allow(now)).count() as u32
    }

    #[test]
    fn bucket_burst_then_refill() {
        let mut b = TokenBucket::new(3, 2 * SEC);
        let t0 = Instant::now();
        assert_eq!(allowed(&mut b, t0, 4), 3, "3 seguidos, o 4º barrado");
        assert!(!b.allow(t0 + SEC), "1 s não recarrega");
        assert_eq!(allowed(&mut b, t0 + 2 * SEC, 5), 1, "2 s recarregam 1");
        assert_eq!(allowed(&mut b, t0 + 4 * SEC, 5), 1);
        assert_eq!(
            allowed(&mut b, t0 + 7 * SEC, 5),
            1,
            "3 s depois: 1 ficha e meia"
        );
        assert_eq!(
            allowed(&mut b, t0 + 8 * SEC, 5),
            1,
            "a meia ficha não se perde"
        );
    }

    #[test]
    fn bucket_caps_at_capacity() {
        let mut b = TokenBucket::new(3, 2 * SEC);
        let t0 = Instant::now();
        assert_eq!(allowed(&mut b, t0, 3), 3);
        assert_eq!(allowed(&mut b, t0 + 1000 * SEC, 10), 3);
        // Cheio desde t0+1000: a recarga recomeça no gasto, não acumula o tempo parado.
        assert_eq!(allowed(&mut b, t0 + 1001 * SEC, 10), 0);
        assert_eq!(allowed(&mut b, t0 + 1002 * SEC, 10), 1);
    }

    #[test]
    fn bucket_refill_starts_at_first_spend() {
        let mut b = TokenBucket::new(3, 2 * SEC);
        let t0 = Instant::now();
        assert!(b.allow(t0 + 10 * SEC));
        assert_eq!(allowed(&mut b, t0 + 11 * SEC, 5), 2);
        assert_eq!(allowed(&mut b, t0 + 12 * SEC, 5), 1);
    }

    #[test]
    fn bucket_time_going_back_does_not_panic() {
        let mut b = TokenBucket::new(3, 2 * SEC);
        let t1 = Instant::now() + 100 * SEC;
        assert_eq!(allowed(&mut b, t1, 3), 3);
        assert_eq!(allowed(&mut b, t1 - 50 * SEC, 3), 0);
        assert_eq!(allowed(&mut b, t1 + 2 * SEC, 3), 1);
    }

    #[test]
    fn bucket_zero_refill_does_not_panic() {
        let mut b = TokenBucket::new(1, Duration::ZERO);
        let t0 = Instant::now();
        assert!(b.allow(t0));
        assert!(b.allow(t0));
    }

    #[test]
    fn cooldown_min_interval() {
        let mut c = Cooldown::new(SEC);
        let t0 = Instant::now();
        assert!(c.allow(t0));
        assert!(!c.allow(t0));
        assert!(!c.allow(t0 + Duration::from_millis(999)));
        assert!(c.allow(t0 + SEC));
        // O barrado não reinicia a contagem.
        assert!(!c.allow(t0 + Duration::from_millis(1500)));
        assert!(c.allow(t0 + 2 * SEC));
    }

    #[test]
    fn cooldown_time_going_back_does_not_panic() {
        let mut c = Cooldown::new(SEC);
        let t1 = Instant::now() + 100 * SEC;
        assert!(c.allow(t1));
        assert!(!c.allow(t1 - 50 * SEC));
        assert!(c.allow(t1 + SEC));
    }
}
