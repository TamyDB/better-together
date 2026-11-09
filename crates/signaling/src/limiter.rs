//! Token bucket por IP: capacidade 10, recarga de 1 token a cada 6 s.

use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};

const CAPACITY: f64 = 10.0;
const REFILL: Duration = Duration::from_secs(6);

#[derive(Default)]
pub struct RateLimiter {
    buckets: HashMap<IpAddr, (f64, Instant)>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow(&mut self, ip: IpAddr, now: Instant) -> bool {
        let (tokens, last) = self.buckets.entry(ip).or_insert((CAPACITY, now));
        let elapsed = now.saturating_duration_since(*last);
        *tokens = (*tokens + elapsed.as_secs_f64() / REFILL.as_secs_f64()).min(CAPACITY);
        *last = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_bucket() {
        let mut l = RateLimiter::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        let other: IpAddr = "5.6.7.8".parse().unwrap();
        let t0 = Instant::now();
        for _ in 0..10 {
            assert!(l.allow(ip, t0));
        }
        assert!(!l.allow(ip, t0));
        assert!(l.allow(other, t0));
        let t1 = t0 + Duration::from_secs(6);
        assert!(l.allow(ip, t1));
        assert!(!l.allow(ip, t1));
    }
}
