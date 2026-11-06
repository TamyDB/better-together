/// xorshift64*: pequeno, rápido e idêntico em todas as plataformas.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Valor uniforme em `[lo, hi)`.
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        let unit = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        lo + unit * (hi - lo)
    }

    /// Valor uniforme em `[lo, hi)`.
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next_u64() % u64::from(hi - lo)) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_deterministic() {
        let (mut a, mut b) = (Rng::new(42), Rng::new(42));
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut z = Rng::new(0);
        assert!((0..10).any(|_| z.next_u64() != 0));
    }

    #[test]
    fn ranges_stay_in_bounds() {
        let mut r = Rng::new(9);
        for _ in 0..1000 {
            let f = r.range_f32(0.05, 0.95);
            assert!((0.05..0.95).contains(&f));
            let u = r.range_u32(120, 301);
            assert!((120..301).contains(&u));
        }
    }
}
