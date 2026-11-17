//! Resources, terrain, harbors and fixed-size resource multisets.

use std::fmt;
use std::ops::{Add, AddAssign, Index, IndexMut, Sub, SubAssign};

/// The five tradeable resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Resource {
    Brick = 0,
    Lumber = 1,
    Wool = 2,
    Grain = 3,
    Ore = 4,
}

impl Resource {
    pub const ALL: [Resource; 5] = [
        Resource::Brick,
        Resource::Lumber,
        Resource::Wool,
        Resource::Grain,
        Resource::Ore,
    ];

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn from_index(i: usize) -> Option<Resource> {
        Self::ALL.get(i).copied()
    }
}

/// Terrain of a land tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Terrain {
    Hills = 0,
    Forest = 1,
    Pasture = 2,
    Fields = 3,
    Mountains = 4,
    Desert = 5,
}

impl Terrain {
    /// Resource produced by this terrain, if any.
    #[inline]
    pub const fn resource(self) -> Option<Resource> {
        match self {
            Terrain::Hills => Some(Resource::Brick),
            Terrain::Forest => Some(Resource::Lumber),
            Terrain::Pasture => Some(Resource::Wool),
            Terrain::Fields => Some(Resource::Grain),
            Terrain::Mountains => Some(Resource::Ore),
            Terrain::Desert => None,
        }
    }
}

/// Kind of harbor: a generic 3:1 harbor or a 2:1 harbor for one resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HarborKind {
    Generic,
    Special(Resource),
}

impl HarborKind {
    /// Bit used in a player's harbor-access mask.
    #[inline]
    pub const fn bit(self) -> u8 {
        match self {
            HarborKind::Special(r) => 1 << (r as u8),
            HarborKind::Generic => 1 << 5,
        }
    }

    /// Compact code: 0..=4 for special harbors, 5 for generic.
    #[inline]
    pub const fn code(self) -> u8 {
        match self {
            HarborKind::Special(r) => r as u8,
            HarborKind::Generic => 5,
        }
    }
}

/// A multiset of resources (counts per resource type).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ResourceSet(pub [u8; 5]);

impl ResourceSet {
    pub const EMPTY: ResourceSet = ResourceSet([0; 5]);

    #[inline]
    pub const fn new(brick: u8, lumber: u8, wool: u8, grain: u8, ore: u8) -> Self {
        ResourceSet([brick, lumber, wool, grain, ore])
    }

    #[inline]
    pub fn single(r: Resource, n: u8) -> Self {
        let mut s = Self::EMPTY;
        s.0[r.index()] = n;
        s
    }

    /// Total number of cards.
    #[inline]
    pub fn total(&self) -> u32 {
        self.0.iter().map(|&c| c as u32).sum()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0 == [0; 5]
    }

    /// `true` if `self` has at least the counts in `other`.
    #[inline]
    pub fn contains(&self, other: &ResourceSet) -> bool {
        self.0.iter().zip(other.0.iter()).all(|(a, b)| a >= b)
    }

    /// `true` if some resource type appears in both sets.
    #[inline]
    pub fn overlaps(&self, other: &ResourceSet) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .any(|(&a, &b)| a > 0 && b > 0)
    }

    /// Returns the resource at position `index` when the cards are laid out
    /// in resource order. Used for uniformly random card selection.
    #[inline]
    pub fn nth_card(&self, mut index: u32) -> Option<Resource> {
        for r in Resource::ALL {
            let c = self.0[r.index()] as u32;
            if index < c {
                return Some(r);
            }
            index -= c;
        }
        None
    }
}

impl Index<Resource> for ResourceSet {
    type Output = u8;
    #[inline]
    fn index(&self, r: Resource) -> &u8 {
        &self.0[r.index()]
    }
}

impl IndexMut<Resource> for ResourceSet {
    #[inline]
    fn index_mut(&mut self, r: Resource) -> &mut u8 {
        &mut self.0[r.index()]
    }
}

impl Add for ResourceSet {
    type Output = ResourceSet;
    #[inline]
    fn add(mut self, rhs: ResourceSet) -> ResourceSet {
        self += rhs;
        self
    }
}

impl AddAssign for ResourceSet {
    #[inline]
    fn add_assign(&mut self, rhs: ResourceSet) {
        for i in 0..5 {
            self.0[i] += rhs.0[i];
        }
    }
}

impl Sub for ResourceSet {
    type Output = ResourceSet;
    #[inline]
    fn sub(mut self, rhs: ResourceSet) -> ResourceSet {
        self -= rhs;
        self
    }
}

impl SubAssign for ResourceSet {
    /// Panics on underflow in debug builds; callers must check `contains`.
    #[inline]
    fn sub_assign(&mut self, rhs: ResourceSet) {
        for i in 0..5 {
            self.0[i] -= rhs.0[i];
        }
    }
}

impl fmt::Debug for ResourceSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [b, l, w, g, o] = self.0;
        write!(f, "[brick {b}, lumber {l}, wool {w}, grain {g}, ore {o}]")
    }
}

/// Building costs.
pub mod cost {
    use super::ResourceSet;
    pub const ROAD: ResourceSet = ResourceSet::new(1, 1, 0, 0, 0);
    pub const SETTLEMENT: ResourceSet = ResourceSet::new(1, 1, 1, 1, 0);
    pub const CITY: ResourceSet = ResourceSet::new(0, 0, 0, 2, 3);
    pub const DEV_CARD: ResourceSet = ResourceSet::new(0, 0, 1, 1, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_ops() {
        let a = ResourceSet::new(1, 2, 0, 0, 3);
        let b = ResourceSet::new(1, 0, 0, 0, 1);
        assert!(a.contains(&b));
        assert!(!b.contains(&a));
        assert_eq!((a - b).total(), 4);
        assert_eq!((a + b).0, [2, 2, 0, 0, 4]);
        assert!(a.overlaps(&b));
        assert!(!ResourceSet::single(Resource::Wool, 1).overlaps(&b));
        assert_eq!(a.nth_card(0), Some(Resource::Brick));
        assert_eq!(a.nth_card(1), Some(Resource::Lumber));
        assert_eq!(a.nth_card(3), Some(Resource::Ore));
        assert_eq!(a.nth_card(6), None);
    }
}
