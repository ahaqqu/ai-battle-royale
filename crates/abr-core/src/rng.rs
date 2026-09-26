//! Seeded, platform-independent PRNG (SplitMix64 + xorshift64*).
//! The only randomness allowed in the sim; its state lives in `WorldState`
//! so replays reproduce it exactly (PLAN §5.1).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    pub(crate) s: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { s: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        // SplitMix64 step for state advance...
        self.s = self.s.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, n).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next_u64() % n
    }

    /// Uniform Fix in [0, 1).
    pub fn unit(&mut self) -> crate::fixed::Fix {
        (self.next_u64() >> 32) as i64 * 65536 / (1i64 << 32)
    }

    /// Uniform Fix in [-1, 1).
    pub fn unit_signed(&mut self) -> crate::fixed::Fix {
        self.unit() * 2 - crate::fixed::ONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_stream() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn below_in_range() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            assert!(r.below(100) < 100);
        }
    }

    #[test]
    fn unit_in_range() {
        use crate::fixed::{to_f64, ONE};
        let mut r = Rng::new(9);
        for _ in 0..10_000 {
            let u = r.unit();
            assert!((0..ONE).contains(&u), "unit out of range: {u}");
            let _ = to_f64(u);
        }
    }
}
