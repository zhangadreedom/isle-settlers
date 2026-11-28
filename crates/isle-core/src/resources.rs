//! Resources, terrain, harbors and fixed-size resource multisets.

use std::fmt;
use std::ops::{Add, AddAssign, Index, IndexMut, Sub, SubAssign};

/// The five tradeable resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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

/// Terrain of a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[repr(u8)]
pub enum Terrain {
    Hills = 0,
    Forest = 1,
    Pasture = 2,
    Fields = 3,
    Mountains = 4,
    Desert = 5,
    /// Seafaring maps: open water between the islands.
    Sea = 6,
    /// Seafaring maps: produces a resource of the owner's choice.
    Gold = 7,
    /// Fog maps: unexplored tile, revealed when a route reaches it.
    Fog = 8,
}

impl Terrain {
    pub const COUNT: usize = 9;

    /// Land carries buildings and roads; the sea carries ships.
    #[inline]
    pub const fn is_land(self) -> bool {
        !matches!(self, Terrain::Sea | Terrain::Fog)
    }

    /// Carries a number token.
    #[inline]
    pub const fn produces(self) -> bool {
        !matches!(self, Terrain::Desert | Terrain::Sea | Terrain::Fog)
    }

    /// Resource produced by this terrain, if any.
    #[inline]
    pub const fn resource(self) -> Option<Resource> {
        match self {
            Terrain::Hills => Some(Resource::Brick),
            Terrain::Forest => Some(Resource::Lumber),
            Terrain::Pasture => Some(Resource::Wool),
            Terrain::Fields => Some(Resource::Grain),
            Terrain::Mountains => Some(Resource::Ore),
            Terrain::Desert | Terrain::Sea | Terrain::Gold | Terrain::Fog => None,
        }
    }

    /// Commodity produced by a city on this terrain in Citadels mode.
    #[inline]
    pub const fn commodity(self) -> Option<Commodity> {
        match self {
            Terrain::Forest => Some(Commodity::Paper),
            Terrain::Pasture => Some(Commodity::Cloth),
            Terrain::Mountains => Some(Commodity::Coin),
            _ => None,
        }
    }
}

/// Refined urban goods produced by cities in Citadels (Cities & Knights) mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[repr(u8)]
pub enum Commodity {
    /// Produced by cities on Forest; advances Science.
    Paper = 0,
    /// Produced by cities on Pasture; advances Trade.
    Cloth = 1,
    /// Produced by cities on Mountains; advances Politics.
    Coin = 2,
}

impl Commodity {
    pub const ALL: [Commodity; 3] = [Commodity::Paper, Commodity::Cloth, Commodity::Coin];

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn from_index(i: usize) -> Option<Commodity> {
        Self::ALL.get(i).copied()
    }
}

/// Three-track urban improvements in Citadels mode (levels 0..=5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[repr(u8)]
pub enum CityTrack {
    /// Paid in Cloth; Lv3 unlocks 2:1 commodity maritime trade.
    Trade = 0,
    /// Paid in Coin; Lv3 unlocks promoting knights to Lv3 (Mighty).
    Politics = 1,
    /// Paid in Paper; Lv3 unlocks Aqueduct (1 free resource on dry non-7 rolls).
    Science = 2,
}

impl CityTrack {
    pub const ALL: [CityTrack; 3] = [CityTrack::Trade, CityTrack::Politics, CityTrack::Science];

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn from_index(i: usize) -> Option<CityTrack> {
        Self::ALL.get(i).copied()
    }

    #[inline]
    pub const fn commodity(self) -> Commodity {
        match self {
            CityTrack::Trade => Commodity::Cloth,
            CityTrack::Politics => Commodity::Coin,
            CityTrack::Science => Commodity::Paper,
        }
    }
}

/// A multiset of commodities (counts of Paper, Cloth, Coin).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CommoditySet(pub [u8; 3]);

impl CommoditySet {
    pub const EMPTY: CommoditySet = CommoditySet([0; 3]);

    #[inline]
    pub const fn new(paper: u8, cloth: u8, coin: u8) -> Self {
        CommoditySet([paper, cloth, coin])
    }

    #[inline]
    pub fn single(c: Commodity, n: u8) -> Self {
        let mut s = Self::EMPTY;
        s.0[c.index()] = n;
        s
    }

    #[inline]
    pub fn total(&self) -> u32 {
        self.0.iter().map(|&c| c as u32).sum()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0 == [0; 3]
    }

    #[inline]
    pub fn contains(&self, other: &CommoditySet) -> bool {
        self.0.iter().zip(other.0.iter()).all(|(a, b)| a >= b)
    }

    #[inline]
    pub fn nth_card(&self, mut index: u32) -> Option<Commodity> {
        for c in Commodity::ALL {
            let n = self.0[c.index()] as u32;
            if index < n {
                return Some(c);
            }
            index -= n;
        }
        None
    }
}

impl Index<Commodity> for CommoditySet {
    type Output = u8;
    #[inline]
    fn index(&self, c: Commodity) -> &u8 {
        &self.0[c.index()]
    }
}

impl IndexMut<Commodity> for CommoditySet {
    #[inline]
    fn index_mut(&mut self, c: Commodity) -> &mut u8 {
        &mut self.0[c.index()]
    }
}

impl Add for CommoditySet {
    type Output = CommoditySet;
    #[inline]
    fn add(mut self, rhs: CommoditySet) -> CommoditySet {
        self += rhs;
        self
    }
}

impl AddAssign for CommoditySet {
    #[inline]
    fn add_assign(&mut self, rhs: CommoditySet) {
        for i in 0..3 {
            self.0[i] += rhs.0[i];
        }
    }
}

impl Sub for CommoditySet {
    type Output = CommoditySet;
    #[inline]
    fn sub(mut self, rhs: CommoditySet) -> CommoditySet {
        self -= rhs;
        self
    }
}

impl SubAssign for CommoditySet {
    #[inline]
    fn sub_assign(&mut self, rhs: CommoditySet) {
        for i in 0..3 {
            self.0[i] -= rhs.0[i];
        }
    }
}

/// Kind of harbor: a generic 3:1 harbor or a 2:1 harbor for one resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "serde", serde(tag = "type", content = "value"))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
    /// Seafaring.
    pub const SHIP: ResourceSet = ResourceSet::new(0, 1, 1, 0, 0);
    /// Citadels: recruiting or promoting a knight.
    pub const KNIGHT: ResourceSet = ResourceSet::new(0, 0, 1, 0, 1);
    /// Citadels: activating a knight with grain rations.
    pub const ACTIVATE_KNIGHT: ResourceSet = ResourceSet::new(0, 0, 0, 1, 0);
    /// Caravans: building a stone bridge across the golden river.
    pub const BRIDGE: ResourceSet = ResourceSet::new(2, 1, 0, 0, 0);
    /// Explorers: raiding a pirate lair (1 Wool + 1 Ore).
    pub const PIRATE_LAIR: ResourceSet = ResourceSet::new(0, 0, 1, 0, 1);
    /// Explorers: hauling fish from a deep shoal (1 Lumber + 1 Grain).
    pub const FISH_SHOAL: ResourceSet = ResourceSet::new(0, 1, 0, 1, 0);
    /// Explorers: trading at a spice island (1 Wool + 1 Grain).
    pub const SPICE_TRADE: ResourceSet = ResourceSet::new(0, 0, 1, 1, 0);
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
