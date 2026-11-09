//! Token bucket por IP: capacidade 10, recarga de 1 token a cada 6 s.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant},
};

const CAPACITY: f64 = 10.0;
const REFILL: Duration = Duration::from_secs(6);

/// Chave canônica: IPv4-mapped vira v4; IPv6 vale pelo prefixo /64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    V4(Ipv4Addr),
    V6([u8; 8]),
}

impl Key {
    fn from_ip(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => Key::V4(v4),
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => Key::V4(v4),
                None => {
                    let mut p = [0u8; 8];
                    p.copy_from_slice(&v6.octets()[..8]);
                    Key::V6(p)
                }
            },
        }
    }
}

fn refilled(tokens: f64, last: Instant, now: Instant) -> f64 {
    let elapsed = now.saturating_duration_since(last);
    (tokens + elapsed.as_secs_f64() / REFILL.as_secs_f64()).min(CAPACITY)
}

#[derive(Default)]
pub struct RateLimiter {
    buckets: HashMap<Key, (f64, Instant)>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow(&mut self, ip: IpAddr, now: Instant) -> bool {
        let (tokens, last) = self
            .buckets
            .entry(Key::from_ip(ip))
            .or_insert((CAPACITY, now));
        *tokens = refilled(*tokens, *last, now);
        // O tempo pode não ser monotônico entre chamadas; nunca volta atrás.
        *last = (*last).max(now);
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Remove baldes que já estariam cheios em `now` (iguais a um IP novo).
    pub fn prune(&mut self, now: Instant) {
        self.buckets
            .retain(|_, (tokens, last)| refilled(*tokens, *last, now) < CAPACITY);
    }

    pub fn len(&self) -> usize {
        self.buckets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
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

    #[test]
    fn prune_removes_full_buckets() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        for i in 0..5u8 {
            l.allow(IpAddr::from([10, 0, 0, i]), t0);
        }
        let kept: IpAddr = "9.9.9.9".parse().unwrap();
        for _ in 0..10 {
            l.allow(kept, t0);
        }
        assert_eq!(l.len(), 6);
        // 5 IPs gastaram 1 token (cheios em 6 s); o outro precisa de 60 s.
        l.prune(t0 + Duration::from_secs(30));
        assert_eq!(l.len(), 1);
        l.prune(t0 + Duration::from_secs(1000));
        assert!(l.is_empty());
    }

    #[test]
    fn ipv6_prefix_and_mapped_share_bucket() {
        let mut l = RateLimiter::new();
        let t0 = Instant::now();
        for i in 0..10u16 {
            let ip: IpAddr = format!("2001:db8:1:2::{i:x}").parse().unwrap();
            assert!(l.allow(ip, t0));
        }
        assert!(!l.allow("2001:db8:1:2:ffff::1".parse().unwrap(), t0));
        assert!(l.allow("2001:db8:1:3::1".parse().unwrap(), t0));
        let v4: IpAddr = "1.2.3.4".parse().unwrap();
        for _ in 0..10 {
            assert!(l.allow(v4, t0));
        }
        assert!(!l.allow("::ffff:1.2.3.4".parse().unwrap(), t0));
    }

    #[test]
    fn partial_refill_and_cap() {
        let mut l = RateLimiter::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        let t0 = Instant::now();
        for _ in 0..10 {
            l.allow(ip, t0);
        }
        assert!(!l.allow(ip, t0 + Duration::from_secs(5)));
        let t1 = t0 + Duration::from_secs(10_000);
        for _ in 0..10 {
            assert!(l.allow(ip, t1));
        }
        assert!(!l.allow(ip, t1));
    }
}
