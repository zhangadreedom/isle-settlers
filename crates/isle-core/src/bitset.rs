//! Fixed-width 192-bit set for vertex and edge masks (`0..192`).
//!
//! Fits 5–6 player seafaring maps (up to 192 vertices and 192 edges) while
//! remaining a 24-byte heap-free `Copy` value.

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

/// A 192-bit mask stored as three little-endian 64-bit words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct BitSet(pub [u64; 3]);

impl BitSet {
    pub const EMPTY: BitSet = BitSet([0, 0, 0]);

    #[inline]
    pub const fn from_words(w0: u64, w1: u64, w2: u64) -> Self {
        BitSet([w0, w1, w2])
    }

    /// Mask with bit `i` set (`i < 192`).
    #[inline]
    pub const fn bit(i: u8) -> Self {
        let mut w = [0u64; 3];
        w[(i >> 6) as usize] = 1u64 << (i & 63);
        BitSet(w)
    }

    #[inline]
    pub const fn contains(self, i: u8) -> bool {
        (self.0[(i >> 6) as usize] & (1u64 << (i & 63))) != 0
    }

    #[inline]
    pub fn insert(&mut self, i: u8) {
        self.0[(i >> 6) as usize] |= 1u64 << (i & 63);
    }

    #[inline]
    pub fn remove(&mut self, i: u8) {
        self.0[(i >> 6) as usize] &= !(1u64 << (i & 63));
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        (self.0[0] | self.0[1] | self.0[2]) == 0
    }

    #[inline]
    pub const fn intersects(self, other: Self) -> bool {
        ((self.0[0] & other.0[0]) | (self.0[1] & other.0[1]) | (self.0[2] & other.0[2])) != 0
    }

    #[inline]
    pub const fn count_ones(self) -> u32 {
        self.0[0].count_ones() + self.0[1].count_ones() + self.0[2].count_ones()
    }
}

impl BitOr for BitSet {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        BitSet([
            self.0[0] | rhs.0[0],
            self.0[1] | rhs.0[1],
            self.0[2] | rhs.0[2],
        ])
    }
}

impl BitOrAssign for BitSet {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0[0] |= rhs.0[0];
        self.0[1] |= rhs.0[1];
        self.0[2] |= rhs.0[2];
    }
}

impl BitAnd for BitSet {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self {
        BitSet([
            self.0[0] & rhs.0[0],
            self.0[1] & rhs.0[1],
            self.0[2] & rhs.0[2],
        ])
    }
}

impl BitAndAssign for BitSet {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.0[0] &= rhs.0[0];
        self.0[1] &= rhs.0[1];
        self.0[2] &= rhs.0[2];
    }
}

impl BitXor for BitSet {
    type Output = Self;
    #[inline]
    fn bitxor(self, rhs: Self) -> Self {
        BitSet([
            self.0[0] ^ rhs.0[0],
            self.0[1] ^ rhs.0[1],
            self.0[2] ^ rhs.0[2],
        ])
    }
}

impl BitXorAssign for BitSet {
    #[inline]
    fn bitxor_assign(&mut self, rhs: Self) {
        self.0[0] ^= rhs.0[0];
        self.0[1] ^= rhs.0[1];
        self.0[2] ^= rhs.0[2];
    }
}

impl Not for BitSet {
    type Output = Self;
    #[inline]
    fn not(self) -> Self {
        BitSet([!self.0[0], !self.0[1], !self.0[2]])
    }
}

/// Iterates the indices (`0..192`) of the set bits of `mask` in ascending order.
#[inline]
pub fn bits(mut mask: BitSet) -> impl Iterator<Item = u8> {
    let mut word = 0usize;
    std::iter::from_fn(move || {
        while word < 3 {
            let w = mask.0[word];
            if w != 0 {
                let i = w.trailing_zeros() as u8 + (word as u8) * 64;
                mask.0[word] = w & (w - 1);
                return Some(i);
            }
            word += 1;
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_across_words() {
        let a = BitSet::bit(0) | BitSet::bit(64) | BitSet::bit(127) | BitSet::bit(180);
        assert_eq!(a.count_ones(), 4);
        assert!(a.contains(0) && a.contains(64) && a.contains(127) && a.contains(180));
        assert!(!a.contains(63) && !a.contains(181));
        assert_eq!(bits(a).collect::<Vec<_>>(), vec![0, 64, 127, 180]);
        let b = a & !BitSet::bit(64);
        assert_eq!(bits(b).collect::<Vec<_>>(), vec![0, 127, 180]);
    }
}
