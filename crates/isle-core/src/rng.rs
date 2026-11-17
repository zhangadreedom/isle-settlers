//! Small, deterministic PRNG (xoshiro256++ seeded through SplitMix64).
//!
//! The engine owns its RNG so that a game is fully reproducible from its seed
//! plus the action log, identically on native and WebAssembly targets.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let mut next = || {
            sm = sm.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = sm;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        Rng {
            s: [next(), next(), next(), next()],
        }
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform integer in `0..n` (Lemire's nearly-divisionless method).
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0);
        let n64 = n as u64;
        let mut m = (self.next_u64() >> 32) * n64;
        if (m as u32) < n {
            let threshold = n.wrapping_neg() % n;
            while (m as u32) < threshold {
                m = (self.next_u64() >> 32) * n64;
            }
        }
        (m >> 32) as u32
    }

    /// Roll of one six-sided die.
    #[inline]
    pub fn die(&mut self) -> u8 {
        self.below(6) as u8 + 1
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }

    #[test]
    fn dice_are_roughly_uniform() {
        let mut rng = Rng::new(7);
        let mut counts = [0u32; 7];
        for _ in 0..60_000 {
            counts[rng.die() as usize] += 1;
        }
        assert_eq!(counts[0], 0);
        for &c in &counts[1..] {
            assert!((9_000..11_000).contains(&c), "{counts:?}");
        }
    }
}
