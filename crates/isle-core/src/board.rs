//! Board layout: terrain, number tokens and harbors on top of a static topology.

use crate::bitset::BitSet;
use crate::resources::{HarborKind, Resource, Terrain};
use crate::rng::Rng;
use crate::topology::*;

/// Terrain tiles of the base island.
pub const TERRAIN_POOL: [Terrain; 19] = {
    use Terrain::*;
    [
        Forest, Forest, Forest, Forest, Pasture, Pasture, Pasture, Pasture, Fields, Fields, Fields,
        Fields, Hills, Hills, Hills, Mountains, Mountains, Mountains, Desert,
    ]
};

/// Terrain tiles of the extended island.
pub const EXTENDED_TERRAIN_POOL: [Terrain; 30] = {
    use Terrain::*;
    [
        Forest, Forest, Forest, Forest, Forest, Forest, Pasture, Pasture, Pasture, Pasture,
        Pasture, Pasture, Fields, Fields, Fields, Fields, Fields, Fields, Hills, Hills, Hills,
        Hills, Hills, Mountains, Mountains, Mountains, Mountains, Mountains, Desert, Desert,
    ]
};

/// Number tokens of the base island (one per non-desert tile).
pub const NUMBER_POOL: [u8; 18] = [2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12];

/// Number tokens of the extended island.
pub const EXTENDED_NUMBER_POOL: [u8; 28] = [
    2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6, 8, 8, 8, 9, 9, 9, 10, 10, 10, 11, 11, 11, 12, 12,
];

/// Harbors of the base island: four generic and one special per resource.
pub const HARBOR_POOL: [HarborKind; 9] = [
    HarborKind::Generic,
    HarborKind::Generic,
    HarborKind::Generic,
    HarborKind::Generic,
    HarborKind::Special(Resource::Brick),
    HarborKind::Special(Resource::Lumber),
    HarborKind::Special(Resource::Wool),
    HarborKind::Special(Resource::Grain),
    HarborKind::Special(Resource::Ore),
];

/// Harbors of the extended island: five generic, one special per resource
/// and a second wool harbor.
pub const EXTENDED_HARBOR_POOL: [HarborKind; 11] = [
    HarborKind::Generic,
    HarborKind::Generic,
    HarborKind::Generic,
    HarborKind::Generic,
    HarborKind::Generic,
    HarborKind::Special(Resource::Brick),
    HarborKind::Special(Resource::Lumber),
    HarborKind::Special(Resource::Wool),
    HarborKind::Special(Resource::Wool),
    HarborKind::Special(Resource::Grain),
    HarborKind::Special(Resource::Ore),
];

/// Archipelago: land of the home island (`M` on the map).
pub const ARCHIPELAGO_HOME_POOL: [Terrain; 15] = {
    use Terrain::*;
    [
        Forest, Forest, Forest, Forest, Pasture, Pasture, Pasture, Fields, Fields, Fields, Hills,
        Hills, Mountains, Mountains, Desert,
    ]
};

/// Archipelago: land of the small islands besides one gold field each.
pub const ARCHIPELAGO_OUTER_POOL: [Terrain; 4] = {
    use Terrain::*;
    [Pasture, Fields, Hills, Mountains]
};

/// Archipelago: number tokens, one per land tile but the desert.
pub const ARCHIPELAGO_NUMBER_POOL: [u8; 20] = [
    2, 3, 3, 4, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 10, 11, 11, 12,
];

/// Fog islands: known land of the crescent home island (`M`).
pub const FOG_HOME_POOL: [Terrain; 14] = {
    use Terrain::*;
    [
        Forest, Forest, Forest, Pasture, Pasture, Pasture, Fields, Fields, Fields, Hills, Hills,
        Mountains, Mountains, Desert,
    ]
};

/// Fog islands: hidden stack of thirteen unexplored tiles (`?`).
pub const FOG_TERRAIN_POOL: [Terrain; 13] = {
    use Terrain::*;
    [
        Sea, Sea, Sea, Sea, Gold, Gold, Hills, Hills, Mountains, Mountains, Forest, Pasture, Fields,
    ]
};

/// Fog islands: number tokens placed on the home island's thirteen producing tiles.
pub const FOG_HOME_NUMBER_POOL: [u8; 13] = [2, 3, 4, 4, 5, 5, 6, 8, 9, 9, 10, 10, 11];

/// Fog islands: hidden stack of number tokens drawn when land is discovered.
pub const FOG_NUMBER_POOL: [u8; 9] = [3, 4, 5, 6, 8, 9, 10, 11, 12];

/// Extended archipelago (5–6 players): 21-tile home island.
pub const EXTENDED_ARCHIPELAGO_HOME_POOL: [Terrain; 21] = {
    use Terrain::*;
    [
        Forest, Forest, Forest, Forest, Forest, Pasture, Pasture, Pasture, Pasture, Pasture,
        Fields, Fields, Fields, Fields, Hills, Hills, Hills, Mountains, Mountains, Mountains,
        Desert,
    ]
};

/// Extended archipelago: non-gold tiles of the three outer islands.
pub const EXTENDED_ARCHIPELAGO_OUTER_POOL: [Terrain; 4] = {
    use Terrain::*;
    [Pasture, Fields, Hills, Mountains]
};

/// Extended archipelago: number tokens for the 27 producing tiles.
pub const EXTENDED_ARCHIPELAGO_NUMBER_POOL: [u8; 27] = [
    2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6, 8, 8, 8, 9, 9, 9, 10, 10, 10, 11, 11, 11, 12,
];

/// Extended fog islands (5–6 players): 20-tile crescent home island.
pub const EXTENDED_FOG_HOME_POOL: [Terrain; 20] = {
    use Terrain::*;
    [
        Forest, Forest, Forest, Forest, Forest, Pasture, Pasture, Pasture, Pasture, Fields, Fields,
        Fields, Fields, Hills, Hills, Hills, Mountains, Mountains, Mountains, Desert,
    ]
};

/// Extended fog islands: hidden stack of eighteen unexplored tiles (`?`).
pub const EXTENDED_FOG_TERRAIN_POOL: [Terrain; 18] = {
    use Terrain::*;
    [
        Sea, Sea, Sea, Sea, Sea, Gold, Gold, Gold, Hills, Hills, Mountains, Mountains, Forest,
        Forest, Pasture, Pasture, Fields, Fields,
    ]
};

/// Extended fog islands: number tokens placed on the home island's 19 producing tiles.
pub const EXTENDED_FOG_HOME_NUMBER_POOL: [u8; 19] = [
    2, 3, 3, 4, 4, 5, 5, 5, 6, 6, 8, 8, 9, 9, 9, 10, 10, 11, 12,
];

/// Extended fog islands: hidden stack of 13 number tokens for discovered land.
pub const EXTENDED_FOG_NUMBER_POOL: [u8; 13] = [2, 3, 4, 4, 5, 6, 8, 9, 10, 10, 11, 11, 12];

/// Number tokens a gold field may carry: neither the likeliest (6, 8) nor
/// the rarest (2, 12).
pub const GOLD_NUMBERS: [u8; 6] = [3, 4, 5, 9, 10, 11];

/// Number of dice combinations producing each roll (out of 36).
pub const PIPS: [u8; 13] = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];

/// Most tiles sharing one number token (custom maps allow up to twelve).
pub const MAX_TILES_PER_NUMBER: usize = 12;

/// Maximum number of coastal fishing grounds on a board (6 on Base, 8 on Extended).
pub const MAX_FISHING_GROUNDS: usize = 8;

/// Number tokens assigned to the 6 coastal fishing grounds on the base island.
pub const BASE_FISHING_NUMBERS: [u8; 6] = [4, 5, 6, 8, 9, 10];

/// Number tokens assigned to the 8 coastal fishing grounds on the extended island.
pub const EXTENDED_FISHING_NUMBERS: [u8; 8] = [4, 5, 5, 6, 8, 9, 9, 10];

/// A coastal shoal producing fish when `number` is rolled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FishingGround {
    pub edge: u8,
    pub number: u8,
}

impl BoardLayout {
    /// Terrain tiles of a classic board, one per tile (empty for seafaring
    /// maps, whose terrain is placed island by island).
    pub fn terrain_pool(self) -> &'static [Terrain] {
        match self {
            BoardLayout::Base => &TERRAIN_POOL,
            BoardLayout::Extended => &EXTENDED_TERRAIN_POOL,
            BoardLayout::Archipelago
            | BoardLayout::FogIslands
            | BoardLayout::ArchipelagoExtended
            | BoardLayout::FogIslandsExtended => &[],
        }
    }

    /// Hidden fog terrain pool for fog layouts.
    pub fn fog_terrain_pool(self) -> &'static [Terrain] {
        match self {
            BoardLayout::FogIslands => &FOG_TERRAIN_POOL,
            BoardLayout::FogIslandsExtended => &EXTENDED_FOG_TERRAIN_POOL,
            _ => &[],
        }
    }

    /// Hidden fog number pool for fog layouts.
    pub fn fog_number_pool(self) -> &'static [u8] {
        match self {
            BoardLayout::FogIslands => &FOG_NUMBER_POOL,
            BoardLayout::FogIslandsExtended => &EXTENDED_FOG_NUMBER_POOL,
            _ => &[],
        }
    }

    /// Initial terrain counts of a board of this layout, indexed by `Terrain`.
    pub fn terrain_counts(self) -> [u8; Terrain::COUNT] {
        let mut n = [0u8; Terrain::COUNT];
        let mut add = |tiles: &[Terrain]| tiles.iter().for_each(|&t| n[t as usize] += 1);
        match self {
            BoardLayout::Base => add(&TERRAIN_POOL),
            BoardLayout::Extended => add(&EXTENDED_TERRAIN_POOL),
            BoardLayout::Archipelago | BoardLayout::ArchipelagoExtended => {
                if self == BoardLayout::Archipelago {
                    add(&ARCHIPELAGO_HOME_POOL);
                    add(&ARCHIPELAGO_OUTER_POOL);
                } else {
                    add(&EXTENDED_ARCHIPELAGO_HOME_POOL);
                    add(&EXTENDED_ARCHIPELAGO_OUTER_POOL);
                }
                let map = self.map().unwrap_or(&[]);
                let mut islands: Vec<u8> = map
                    .iter()
                    .copied()
                    .filter(|&c| c != b'M' && !is_sea_char(c))
                    .collect();
                islands.sort_unstable();
                islands.dedup();
                n[Terrain::Gold as usize] += islands.len() as u8;
                n[Terrain::Sea as usize] += map.iter().filter(|&&c| is_sea_char(c)).count() as u8;
            }
            BoardLayout::FogIslands | BoardLayout::FogIslandsExtended => {
                if self == BoardLayout::FogIslands {
                    add(&FOG_HOME_POOL);
                } else {
                    add(&EXTENDED_FOG_HOME_POOL);
                }
                let map = self.map().unwrap_or(&[]);
                n[Terrain::Sea as usize] += map.iter().filter(|&&c| is_sea_char(c)).count() as u8;
                n[Terrain::Fog as usize] += map.iter().filter(|&&c| c == b'?').count() as u8;
            }
        }
        n
    }

    /// Total terrain counts once every fog tile is accounted for.
    pub fn full_terrain_counts(self) -> [u8; Terrain::COUNT] {
        let mut n = self.terrain_counts();
        if self.has_fog() {
            n[Terrain::Fog as usize] = 0;
            for &t in self.fog_terrain_pool() {
                n[t as usize] += 1;
            }
        }
        n
    }

    /// Number tokens placed on the initial board, one per producing tile.
    pub fn number_pool(self) -> &'static [u8] {
        match self {
            BoardLayout::Base => &NUMBER_POOL,
            BoardLayout::Extended => &EXTENDED_NUMBER_POOL,
            BoardLayout::Archipelago => &ARCHIPELAGO_NUMBER_POOL,
            BoardLayout::FogIslands => &FOG_HOME_NUMBER_POOL,
            BoardLayout::ArchipelagoExtended => &EXTENDED_ARCHIPELAGO_NUMBER_POOL,
            BoardLayout::FogIslandsExtended => &EXTENDED_FOG_HOME_NUMBER_POOL,
        }
    }

    /// Harbors, one per harbor slot of the topology.
    pub fn harbor_pool(self) -> &'static [HarborKind] {
        if self.is_extended() {
            &EXTENDED_HARBOR_POOL
        } else {
            &HARBOR_POOL
        }
    }
}

#[inline]
fn is_sea_char(c: u8) -> bool {
    c == b'.' || c == b'~'
}

/// Terrain, tokens and harbors of one game. Arrays are sized for the largest
/// board; entries past the layout's counts are padding (desert, `0`, generic).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Board {
    pub layout: BoardLayout,
    pub terrain: [Terrain; MAX_TILES],
    /// Number token per tile, `0` for deserts.
    pub numbers: [u8; MAX_TILES],
    /// Harbor kind per slot of the topology's `harbor_edges`.
    pub harbors: [HarborKind; MAX_HARBORS],
    /// Harbor-access bit granted by building on each vertex (`0` if none).
    pub vertex_harbor: [u8; MAX_VERTICES],
    /// Tiles carrying each number token, padded with `NONE`.
    pub tiles_by_number: [[u8; MAX_TILES_PER_NUMBER]; 13],
    /// Tile where the robber starts (the first desert).
    pub desert: u8,
    /// Island of each tile: `0` for the sea, otherwise numbered from 1 with
    /// the home island (or the only island) first.
    pub tile_island: [u8; MAX_TILES],
    /// Island of the land tiles around each vertex, `0` if all sea.
    pub vertex_island: [u8; MAX_VERTICES],
    /// Vertices touching land: where settlements may stand.
    pub land_vertices: BitSet,
    /// Edges touching land: where roads may go.
    pub land_edges: BitSet,
    /// Edges touching the sea or the edge of the map: where ships may go.
    pub sea_edges: BitSet,
    /// Vertices open for initial settlements.
    pub setup_vertices: BitSet,
    /// Sea tile where the pirate starts, `NONE` without sea.
    pub pirate_start: u8,
    /// Fishermen scenario: coastal fishing grounds (`fishing_grounds[..fishing_ground_count]`).
    pub fishing_grounds: [FishingGround; MAX_FISHING_GROUNDS],
    pub fishing_ground_count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardError {
    WrongTerrainCount,
    WrongHarborCount,
    /// Sea and land are not where the map has them.
    WrongShape,
    NumberOnDesert,
    MissingNumber,
    InvalidNumber,
    BadCustomCode,
    MissingDesert,
    MissingSea,
    UnexpectedSea,
    UnexpectedFog,
    TooMuchFog,
    FogTouchesLand,
    TooLittleLand,
}

/// Compact DIY board specification (`IM1:<layout>:<tiles>:<harbors>`).
///
/// `Copy` and heap-free so it can live directly inside [`crate::GameConfig`]
/// and network rule structs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(type = "string"))]
pub struct CustomMap {
    pub layout: BoardLayout,
    pub terrain: [Terrain; MAX_TILES],
    pub numbers: [u8; MAX_TILES],
    pub harbors: [HarborKind; MAX_HARBORS],
}

impl CustomMap {
    /// Extracts a normalized [`CustomMap`] from an existing [`Board`].
    pub fn from_board(board: &Board) -> CustomMap {
        let topo = board.topo();
        let nt = topo.num_tiles;
        let nh = topo.harbor_edges.len();
        let mut terrain = [Terrain::Desert; MAX_TILES];
        let mut numbers = [0u8; MAX_TILES];
        let mut harbors = [HarborKind::Generic; MAX_HARBORS];
        terrain[..nt].copy_from_slice(&board.terrain[..nt]);
        numbers[..nt].copy_from_slice(&board.numbers[..nt]);
        harbors[..nh].copy_from_slice(&board.harbors[..nh]);
        CustomMap {
            layout: board.layout,
            terrain,
            numbers,
            harbors,
        }
    }

    /// Encodes as `IM1:<layoutChar>:<tiles>:<harbors>`.
    pub fn encode(&self) -> String {
        let topo = self.layout.topology();
        let nt = topo.num_tiles;
        let nh = topo.harbor_edges.len();
        let mut out = String::with_capacity(6 + 2 * nt + nh);
        out.push_str("IM1:");
        out.push(match self.layout {
            BoardLayout::Base => 'B',
            BoardLayout::Extended => 'E',
            BoardLayout::Archipelago => 'A',
            BoardLayout::FogIslands => 'F',
            BoardLayout::ArchipelagoExtended => 'X',
            BoardLayout::FogIslandsExtended => 'Z',
        });
        out.push(':');
        for t in 0..nt {
            out.push(match self.terrain[t] {
                Terrain::Forest => 'L',
                Terrain::Hills => 'B',
                Terrain::Pasture => 'W',
                Terrain::Fields => 'G',
                Terrain::Mountains => 'O',
                Terrain::Desert => 'D',
                Terrain::Sea => 'S',
                Terrain::Gold => 'U',
                Terrain::Fog => 'Q',
            });
            out.push(match self.numbers[t] {
                0 => '0',
                n @ (2..=6 | 8..=9) => (b'0' + n) as char,
                10 => 'A',
                11 => 'B',
                12 => 'C',
                _ => '0',
            });
        }
        out.push(':');
        for h in 0..nh {
            out.push(match self.harbors[h] {
                HarborKind::Generic => '3',
                HarborKind::Special(Resource::Lumber) => 'L',
                HarborKind::Special(Resource::Brick) => 'B',
                HarborKind::Special(Resource::Wool) => 'W',
                HarborKind::Special(Resource::Grain) => 'G',
                HarborKind::Special(Resource::Ore) => 'O',
            });
        }
        out
    }

    /// Decodes an `IM1:<layoutChar>:<tiles>:<harbors>` string (case-insensitive,
    /// ignoring leading/trailing whitespace).
    pub fn decode(raw: &str) -> Result<CustomMap, BoardError> {
        let s = raw.trim().to_ascii_uppercase();
        let mut parts = s.split(':');
        let Some("IM1") = parts.next() else {
            return Err(BoardError::BadCustomCode);
        };
        let (Some(layout_s), Some(tiles_s), Some(harbors_s), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(BoardError::BadCustomCode);
        };
        let layout = match layout_s {
            "B" => BoardLayout::Base,
            "E" => BoardLayout::Extended,
            "A" => BoardLayout::Archipelago,
            "F" => BoardLayout::FogIslands,
            "X" => BoardLayout::ArchipelagoExtended,
            "Z" => BoardLayout::FogIslandsExtended,
            _ => return Err(BoardError::BadCustomCode),
        };
        let topo = layout.topology();
        let nt = topo.num_tiles;
        let nh = topo.harbor_edges.len();
        let tb = tiles_s.as_bytes();
        let hb = harbors_s.as_bytes();
        if tb.len() != 2 * nt || hb.len() != nh {
            return Err(BoardError::BadCustomCode);
        }
        let mut terrain = [Terrain::Desert; MAX_TILES];
        let mut numbers = [0u8; MAX_TILES];
        for t in 0..nt {
            terrain[t] = match tb[2 * t] {
                b'L' => Terrain::Forest,
                b'B' => Terrain::Hills,
                b'W' => Terrain::Pasture,
                b'G' => Terrain::Fields,
                b'O' => Terrain::Mountains,
                b'D' => Terrain::Desert,
                b'S' => Terrain::Sea,
                b'U' => Terrain::Gold,
                b'Q' => Terrain::Fog,
                _ => return Err(BoardError::BadCustomCode),
            };
            numbers[t] = match tb[2 * t + 1] {
                b'0' => 0,
                b @ (b'2'..=b'6' | b'8'..=b'9') => b - b'0',
                b'A' => 10,
                b'B' => 11,
                b'C' => 12,
                _ => return Err(BoardError::BadCustomCode),
            };
        }
        let mut harbors = [HarborKind::Generic; MAX_HARBORS];
        for h in 0..nh {
            harbors[h] = match hb[h] {
                b'3' => HarborKind::Generic,
                b'L' => HarborKind::Special(Resource::Lumber),
                b'B' => HarborKind::Special(Resource::Brick),
                b'W' => HarborKind::Special(Resource::Wool),
                b'G' => HarborKind::Special(Resource::Grain),
                b'O' => HarborKind::Special(Resource::Ore),
                _ => return Err(BoardError::BadCustomCode),
            };
        }
        Ok(CustomMap {
            layout,
            terrain,
            numbers,
            harbors,
        })
    }
}

impl std::fmt::Display for CustomMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.encode())
    }
}

impl std::str::FromStr for CustomMap {
    type Err = BoardError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::decode(s)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for CustomMap {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.encode())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for CustomMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        CustomMap::decode(&s).map_err(|e| serde::de::Error::custom(format!("{e:?}")))
    }
}

impl Board {
    /// Random board. With `balanced`, the high-probability tokens 6 and 8
    /// are never placed on adjacent tiles.
    pub fn generate(rng: &mut Rng, layout: BoardLayout, balanced: bool) -> Board {
        if layout.map().is_some() {
            return Self::generate_map(rng, layout, balanced);
        }
        let topo = layout.topology();
        let nt = topo.num_tiles;
        let mut terrain = [Terrain::Desert; MAX_TILES];
        terrain[..nt].copy_from_slice(layout.terrain_pool());
        rng.shuffle(&mut terrain[..nt]);
        let nh = topo.harbor_edges.len();
        let mut harbors = [HarborKind::Generic; MAX_HARBORS];
        harbors[..nh].copy_from_slice(layout.harbor_pool());
        rng.shuffle(&mut harbors[..nh]);

        let tokens = layout.number_pool();
        let mut numbers = [0u8; MAX_TILES];
        for _attempt in 0..10_000 {
            let mut pool = [0u8; MAX_TILES];
            let pool = &mut pool[..tokens.len()];
            pool.copy_from_slice(tokens);
            rng.shuffle(pool);
            let mut it = pool.iter();
            for t in 0..nt {
                numbers[t] = if terrain[t] == Terrain::Desert {
                    0
                } else {
                    *it.next().unwrap()
                };
            }
            if !balanced || !has_adjacent_red_numbers(topo, &numbers) {
                break;
            }
        }
        Board::from_layout(layout, &terrain[..nt], &numbers[..nt], &harbors[..nh])
            .expect("generated board is valid")
    }

    /// A seafaring board: sea where the map has it, the home island's
    /// terrain shuffled, and on the archipelago one gold field per small
    /// island (or on fog maps, `Terrain::Fog` over `?` tiles).
    fn generate_map(rng: &mut Rng, layout: BoardLayout, balanced: bool) -> Board {
        if layout.has_fog() {
            return Self::generate_fog(rng, layout, balanced);
        }
        let topo = layout.topology();
        let map = layout.map().expect("seafaring layout");
        let nt = topo.num_tiles;
        let mut home_buf = [Terrain::Desert; 24];
        let home_src: &[Terrain] = if layout == BoardLayout::Archipelago {
            &ARCHIPELAGO_HOME_POOL
        } else {
            &EXTENDED_ARCHIPELAGO_HOME_POOL
        };
        let home_slice = &mut home_buf[..home_src.len()];
        home_slice.copy_from_slice(home_src);
        let mut outer = if layout == BoardLayout::Archipelago {
            ARCHIPELAGO_OUTER_POOL
        } else {
            EXTENDED_ARCHIPELAGO_OUTER_POOL
        };
        rng.shuffle(home_slice);
        rng.shuffle(&mut outer);
        let (mut home, mut outer) = (home_slice.iter().copied(), outer.into_iter());
        // One gold field per small island, on a random tile of it.
        let mut gold = [false; MAX_TILES];
        let mut letters: Vec<u8> = map[..nt]
            .iter()
            .copied()
            .filter(|&c| c != b'M' && !is_sea_char(c))
            .collect();
        letters.sort_unstable();
        letters.dedup();
        for letter in letters {
            let tiles: Vec<usize> = (0..nt).filter(|&t| map[t] == letter).collect();
            gold[tiles[rng.below(tiles.len() as u32) as usize]] = true;
        }
        let mut terrain = [Terrain::Desert; MAX_TILES];
        for t in 0..nt {
            terrain[t] = match map[t] {
                c if is_sea_char(c) => Terrain::Sea,
                b'M' => home.next().expect("home pool fits the map"),
                _ if gold[t] => Terrain::Gold,
                _ => outer.next().expect("outer pool fits the map"),
            };
        }
        let nh = topo.harbor_edges.len();
        let mut harbors = [HarborKind::Generic; MAX_HARBORS];
        harbors[..nh].copy_from_slice(layout.harbor_pool());
        rng.shuffle(&mut harbors[..nh]);

        let tokens = layout.number_pool();
        let mut numbers = [0u8; MAX_TILES];
        for _attempt in 0..10_000 {
            let mut pool = [0u8; MAX_TILES];
            let pool = &mut pool[..tokens.len()];
            pool.copy_from_slice(tokens);
            rng.shuffle(pool);
            let mut it = pool.iter();
            for t in 0..nt {
                numbers[t] = if terrain[t].produces() {
                    *it.next().unwrap()
                } else {
                    0
                };
            }
            let gold_ok =
                (0..nt).all(|t| terrain[t] != Terrain::Gold || GOLD_NUMBERS.contains(&numbers[t]));
            if gold_ok && (!balanced || !has_adjacent_red_numbers(topo, &numbers)) {
                break;
            }
        }
        Board::from_layout(layout, &terrain[..nt], &numbers[..nt], &harbors[..nh])
            .expect("generated board is valid")
    }

    fn generate_fog(rng: &mut Rng, layout: BoardLayout, balanced: bool) -> Board {
        let topo = layout.topology();
        let map = layout.map().expect("fog map");
        let nt = topo.num_tiles;
        let mut home_buf = [Terrain::Desert; 24];
        let home_src: &[Terrain] = if layout == BoardLayout::FogIslands {
            &FOG_HOME_POOL
        } else {
            &EXTENDED_FOG_HOME_POOL
        };
        let home_slice = &mut home_buf[..home_src.len()];
        home_slice.copy_from_slice(home_src);
        rng.shuffle(home_slice);
        let mut home = home_slice.iter().copied();
        let mut terrain = [Terrain::Desert; MAX_TILES];
        for t in 0..nt {
            terrain[t] = match map[t] {
                c if is_sea_char(c) => Terrain::Sea,
                b'?' => Terrain::Fog,
                b'M' => home.next().expect("home pool fits the map"),
                _ => unreachable!("unexpected fog map character"),
            };
        }
        let nh = topo.harbor_edges.len();
        let mut harbors = [HarborKind::Generic; MAX_HARBORS];
        harbors[..nh].copy_from_slice(layout.harbor_pool());
        rng.shuffle(&mut harbors[..nh]);

        let tokens = layout.number_pool();
        let mut numbers = [0u8; MAX_TILES];
        for _attempt in 0..10_000 {
            let mut pool = [0u8; MAX_TILES];
            let pool = &mut pool[..tokens.len()];
            pool.copy_from_slice(tokens);
            rng.shuffle(pool);
            let mut it = pool.iter();
            for t in 0..nt {
                numbers[t] = if terrain[t].produces() {
                    *it.next().unwrap()
                } else {
                    0
                };
            }
            if !balanced || !has_adjacent_red_numbers(topo, &numbers) {
                break;
            }
        }
        Board::from_layout(layout, &terrain[..nt], &numbers[..nt], &harbors[..nh])
            .expect("generated fog board is valid")
    }

    /// Builds a board from an explicit layout (custom maps, tests). Slices
    /// hold one entry per tile and per harbor slot of `layout`.
    pub fn from_layout(
        layout: BoardLayout,
        terrain: &[Terrain],
        numbers: &[u8],
        harbors: &[HarborKind],
    ) -> Result<Board, BoardError> {
        let topo = layout.topology();
        let nt = topo.num_tiles;
        if terrain.len() != nt || numbers.len() != nt {
            return Err(BoardError::WrongTerrainCount);
        }
        if harbors.len() != topo.harbor_edges.len() {
            return Err(BoardError::WrongHarborCount);
        }
        let mut counts = [0u8; Terrain::COUNT];
        for &t in terrain {
            counts[t as usize] += 1;
        }
        if counts != layout.terrain_counts() {
            return Err(BoardError::WrongTerrainCount);
        }
        let map = layout.map();
        if let Some(map) = map
            && (0..nt).any(|t| match map[t] {
                c if is_sea_char(c) => terrain[t] != Terrain::Sea,
                b'?' => terrain[t] != Terrain::Fog,
                _ => !terrain[t].is_land(),
            })
        {
            return Err(BoardError::WrongShape);
        }
        let mut desert = NONE;
        let mut tiles_by_number = [[NONE; MAX_TILES_PER_NUMBER]; 13];
        for t in 0..nt {
            let n = numbers[t];
            if !terrain[t].produces() {
                if n != 0 {
                    return Err(BoardError::NumberOnDesert);
                }
                if desert == NONE && terrain[t] == Terrain::Desert {
                    desert = t as u8;
                }
                continue;
            }
            if n == 0 {
                return Err(BoardError::MissingNumber);
            }
            if !(2..=12).contains(&n) || n == 7 {
                return Err(BoardError::InvalidNumber);
            }
            let slot = tiles_by_number[n as usize]
                .iter_mut()
                .find(|s| **s == NONE)
                .ok_or(BoardError::InvalidNumber)?;
            *slot = t as u8;
        }
        let mut vertex_harbor = [0u8; MAX_VERTICES];
        for (slot, &e) in topo.harbor_edges.iter().enumerate() {
            for v in topo.edge_vertices[e as usize] {
                vertex_harbor[v as usize] = harbors[slot].bit();
            }
        }

        // Islands: the map's letters, the home island first; a classic
        // board is one island.
        let mut tile_island = [0u8; MAX_TILES];
        let mut letters: Vec<u8> = vec![b'M'];
        for t in 0..nt {
            if !terrain[t].is_land() {
                continue;
            }
            let letter = map.map_or(b'M', |m| m[t]);
            let i = letters
                .iter()
                .position(|&l| l == letter)
                .unwrap_or_else(|| {
                    letters.push(letter);
                    letters.len() - 1
                });
            tile_island[t] = i as u8 + 1;
        }
        let mut vertex_island = [0u8; MAX_VERTICES];
        let mut land_vertices = BitSet::EMPTY;
        let mut setup_vertices = BitSet::EMPTY;
        for v in 0..topo.num_vertices {
            for &t in &topo.vertex_tiles[v] {
                if t != NONE && tile_island[t as usize] != 0 {
                    vertex_island[v] = tile_island[t as usize];
                    land_vertices.insert(v as u8);
                    // Seafaring maps start everyone on the home island.
                    if map.is_none() || tile_island[t as usize] == 1 {
                        setup_vertices.insert(v as u8);
                    }
                }
            }
        }
        let mut edge_land = [false; MAX_EDGES];
        let mut edge_tiles = [0u8; MAX_EDGES];
        let mut edge_sea = [false; MAX_EDGES];
        let mut edge_fog = [false; MAX_EDGES];
        for t in 0..nt {
            for &e in &topo.tile_edges[t] {
                edge_tiles[e as usize] += 1;
                if terrain[t] == Terrain::Sea {
                    edge_sea[e as usize] = true;
                } else if terrain[t].is_land() {
                    edge_land[e as usize] = true;
                } else {
                    edge_fog[e as usize] = true;
                }
            }
        }
        let (mut land_edges, mut sea_edges) = (BitSet::EMPTY, BitSet::EMPTY);
        for e in 0..topo.num_edges {
            if edge_land[e] {
                land_edges.insert(e as u8);
            }
            // The map's border is open sea once the tile inside is known.
            if edge_sea[e] || (edge_tiles[e] == 1 && !edge_fog[e]) {
                sea_edges.insert(e as u8);
            }
        }
        let pirate_start = map
            .and_then(|m| m[..nt].iter().position(|&c| c == b'~'))
            .map_or(NONE, |t| t as u8);

        let mut board = Board {
            layout,
            terrain: [Terrain::Desert; MAX_TILES],
            numbers: [0; MAX_TILES],
            harbors: [HarborKind::Generic; MAX_HARBORS],
            vertex_harbor,
            tiles_by_number,
            desert,
            tile_island,
            vertex_island,
            land_vertices,
            land_edges,
            sea_edges,
            setup_vertices,
            pirate_start,
            fishing_grounds: [FishingGround {
                edge: NONE,
                number: 0,
            }; MAX_FISHING_GROUNDS],
            fishing_ground_count: 0,
        };
        board.terrain[..nt].copy_from_slice(terrain);
        board.numbers[..nt].copy_from_slice(numbers);
        board.harbors[..harbors.len()].copy_from_slice(harbors);
        Ok(board)
    }

    /// Builds and validates a DIY board from a [`CustomMap`], computing
    /// connected islands, setup spots, pirate start and buildable masks.
    pub fn from_custom(custom: &CustomMap) -> Result<Board, BoardError> {
        let layout = custom.layout;
        let topo = layout.topology();
        let nt = topo.num_tiles;
        let nh = topo.harbor_edges.len();
        let terrain = &custom.terrain[..nt];
        let numbers = &custom.numbers[..nt];
        let harbors = &custom.harbors[..nh];

        let mut land_count = 0usize;
        let mut sea_count = 0usize;
        let mut fog_count = 0usize;
        let mut desert = NONE;
        for (t, &ter) in terrain.iter().enumerate() {
            match ter {
                Terrain::Sea => {
                    if !layout.has_ships() {
                        return Err(BoardError::UnexpectedSea);
                    }
                    sea_count += 1;
                }
                Terrain::Fog => {
                    if !layout.has_fog() {
                        return Err(BoardError::UnexpectedFog);
                    }
                    fog_count += 1;
                }
                other => {
                    debug_assert!(other.is_land());
                    land_count += 1;
                    if other == Terrain::Desert && desert == NONE {
                        desert = t as u8;
                    }
                }
            }
        }
        if fog_count > crate::state::MAX_FOG_TILES {
            return Err(BoardError::TooMuchFog);
        }
        if layout.has_ships() && sea_count == 0 {
            return Err(BoardError::MissingSea);
        }
        if desert == NONE {
            return Err(BoardError::MissingDesert);
        }
        if land_count + sea_count < 2 || (!layout.has_ships() && land_count < 2) {
            return Err(BoardError::TooLittleLand);
        }

        let mut tiles_by_number = [[NONE; MAX_TILES_PER_NUMBER]; 13];
        for t in 0..nt {
            let n = numbers[t];
            if !terrain[t].produces() {
                if n != 0 {
                    return Err(BoardError::NumberOnDesert);
                }
                continue;
            }
            if n == 0 {
                return Err(BoardError::MissingNumber);
            }
            if !(2..=12).contains(&n) || n == 7 {
                return Err(BoardError::InvalidNumber);
            }
            let slot = tiles_by_number[n as usize]
                .iter_mut()
                .find(|s| **s == NONE)
                .ok_or(BoardError::InvalidNumber)?;
            *slot = t as u8;
        }

        if layout.has_fog() {
            let mut land_verts = BitSet::EMPTY;
            for t in 0..nt {
                if terrain[t].is_land() {
                    land_verts |= topo.tile_vertex_mask[t];
                }
            }
            for t in 0..nt {
                if terrain[t] == Terrain::Fog
                    && !(topo.tile_vertex_mask[t] & land_verts).is_empty()
                {
                    return Err(BoardError::FogTouchesLand);
                }
            }
        }

        let mut vertex_harbor = [0u8; MAX_VERTICES];
        for (slot, &e) in topo.harbor_edges.iter().enumerate() {
            for v in topo.edge_vertices[e as usize] {
                vertex_harbor[v as usize] = harbors[slot].bit();
            }
        }

        // Group land tiles into connected components (vertex-sharing), largest first.
        let mut visited = [false; MAX_TILES];
        let mut comps = [(0usize, 0usize, 0u64); MAX_TILES];
        let mut num_comps = 0usize;
        for start in 0..nt {
            if !terrain[start].is_land() || visited[start] {
                continue;
            }
            visited[start] = true;
            let mut queue = [0usize; MAX_TILES];
            let (mut head, mut tail) = (0usize, 1usize);
            queue[0] = start;
            let mut mask = 1u64 << start;
            while head < tail {
                let a = queue[head];
                head += 1;
                let va = topo.tile_vertex_mask[a];
                for b in 0..nt {
                    if !visited[b]
                        && terrain[b].is_land()
                        && !(va & topo.tile_vertex_mask[b]).is_empty()
                    {
                        visited[b] = true;
                        mask |= 1u64 << b;
                        queue[tail] = b;
                        tail += 1;
                    }
                }
            }
            comps[num_comps] = (tail, start, mask);
            num_comps += 1;
        }
        comps[..num_comps].sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let mut tile_island = [0u8; MAX_TILES];
        for (idx, &(_, _, mut mask)) in comps[..num_comps].iter().enumerate() {
            let island = (idx + 1).min(7) as u8;
            while mask != 0 {
                let t = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                tile_island[t] = island;
            }
        }

        let mut vertex_island = [0u8; MAX_VERTICES];
        let mut land_vertices = BitSet::EMPTY;
        let mut island1_verts = BitSet::EMPTY;
        for v in 0..topo.num_vertices {
            for &t in &topo.vertex_tiles[v] {
                if t != NONE && tile_island[t as usize] != 0 {
                    vertex_island[v] = tile_island[t as usize];
                    land_vertices.insert(v as u8);
                    if tile_island[t as usize] == 1 {
                        island1_verts.insert(v as u8);
                    }
                }
            }
        }

        let min_setup =
            (4 * (2 * layout.max_players() as u32 - 1) + 1).min(topo.num_vertices as u32);
        let setup_vertices = if layout.has_ships() && island1_verts.count_ones() >= min_setup {
            island1_verts
        } else {
            land_vertices
        };
        if setup_vertices.count_ones() < min_setup {
            return Err(BoardError::TooLittleLand);
        }

        let mut edge_land = [false; MAX_EDGES];
        let mut edge_tiles = [0u8; MAX_EDGES];
        let mut edge_sea = [false; MAX_EDGES];
        let mut edge_fog = [false; MAX_EDGES];
        for t in 0..nt {
            for &e in &topo.tile_edges[t] {
                edge_tiles[e as usize] += 1;
                if terrain[t] == Terrain::Sea {
                    edge_sea[e as usize] = true;
                } else if terrain[t].is_land() {
                    edge_land[e as usize] = true;
                } else {
                    edge_fog[e as usize] = true;
                }
            }
        }
        let (mut land_edges, mut sea_edges) = (BitSet::EMPTY, BitSet::EMPTY);
        for e in 0..topo.num_edges {
            if edge_land[e] {
                land_edges.insert(e as u8);
            }
            if edge_sea[e] || (edge_tiles[e] == 1 && !edge_fog[e]) {
                sea_edges.insert(e as u8);
            }
        }

        let pirate_start = if layout.has_ships() {
            layout
                .map()
                .and_then(|m| {
                    m[..nt]
                        .iter()
                        .position(|&c| c == b'~')
                        .filter(|&p| terrain[p] == Terrain::Sea)
                })
                .or_else(|| terrain.iter().position(|&t| t == Terrain::Sea))
                .expect("checked sea_count >= 1") as u8
        } else {
            NONE
        };

        let mut board = Board {
            layout,
            terrain: [Terrain::Desert; MAX_TILES],
            numbers: [0; MAX_TILES],
            harbors: [HarborKind::Generic; MAX_HARBORS],
            vertex_harbor,
            tiles_by_number,
            desert,
            tile_island,
            vertex_island,
            land_vertices,
            land_edges,
            sea_edges,
            setup_vertices,
            pirate_start,
            fishing_grounds: [FishingGround {
                edge: NONE,
                number: 0,
            }; MAX_FISHING_GROUNDS],
            fishing_ground_count: 0,
        };
        board.terrain[..nt].copy_from_slice(terrain);
        board.numbers[..nt].copy_from_slice(numbers);
        board.harbors[..nh].copy_from_slice(harbors);
        Ok(board)
    }

    #[inline]
    pub fn topo(&self) -> &'static Topology {
        self.layout.topology()
    }

    #[inline]
    pub fn is_sea(&self, t: u8) -> bool {
        self.terrain[t as usize] == Terrain::Sea
    }

    /// Number of islands (1 on classic boards).
    pub fn island_count(&self) -> u8 {
        self.tile_island.iter().copied().max().unwrap_or(0)
    }

    /// Terrain of each tile of the layout.
    #[inline]
    pub fn tiles(&self) -> &[Terrain] {
        &self.terrain[..self.topo().num_tiles]
    }

    /// Harbor kind of each harbor slot of the layout.
    #[inline]
    pub fn harbor_kinds(&self) -> &[HarborKind] {
        &self.harbors[..self.topo().harbor_edges.len()]
    }

    /// Active coastal fishing grounds (`0` on non-Fishermen boards).
    #[inline]
    pub fn active_fishing_grounds(&self) -> &[FishingGround] {
        &self.fishing_grounds[..self.fishing_ground_count as usize]
    }

    /// Deterministically populates evenly spaced non-harbor coastal fishing
    /// grounds around the perimeter (`6` on `Base`, `8` on `Extended`).
    pub fn init_fishing_grounds(&mut self) {
        let topo = self.topo();
        let nums: &[u8] = if self.layout.is_extended() {
            &EXTENDED_FISHING_NUMBERS
        } else {
            &BASE_FISHING_NUMBERS
        };
        let mut avail = [NONE; MAX_EDGES];
        let mut avail_len = 0usize;
        for &e in topo.coast_edges {
            if self.land_edges.contains(e) && !topo.harbor_edges.contains(&e) {
                avail[avail_len] = e;
                avail_len += 1;
            }
        }
        let count = nums.len().min(avail_len).min(MAX_FISHING_GROUNDS);
        self.fishing_grounds = [FishingGround {
            edge: NONE,
            number: 0,
        }; MAX_FISHING_GROUNDS];
        for (i, &num) in nums.iter().take(count).enumerate() {
            self.fishing_grounds[i] = FishingGround {
                edge: avail[i * avail_len / count],
                number: num,
            };
        }
        self.fishing_ground_count = count as u8;
    }

    /// Production weight (pips) of a vertex, summed over adjacent tiles.
    pub fn vertex_pips(&self, v: u8) -> u8 {
        self.topo().vertex_tiles[v as usize]
            .iter()
            .filter(|&&t| t != NONE)
            .map(|&t| PIPS[self.numbers[t as usize] as usize])
            .sum()
    }

    /// Turns an unexplored [`Terrain::Fog`] tile face up, updating the
    /// number lookup, island ids and buildable masks in place.
    pub fn reveal_tile(&mut self, tile: u8, terrain: Terrain, number: u8) {
        let t = tile as usize;
        let topo = self.topo();
        debug_assert_eq!(self.terrain[t], Terrain::Fog);
        debug_assert_ne!(terrain, Terrain::Fog);
        debug_assert_eq!(terrain.produces(), number != 0);
        self.terrain[t] = terrain;
        self.numbers[t] = number;
        if number != 0
            && let Some(slot) = self.tiles_by_number[number as usize]
                .iter_mut()
                .find(|s| **s == NONE)
        {
            *slot = tile;
        }
        if terrain == Terrain::Sea {
            self.sea_edges |= topo.tile_edge_mask(tile);
        } else if terrain.is_land() {
            // Discovered land belongs to island 2 (distinct from home island 1).
            self.tile_island[t] = 2;
            for &v in &topo.tile_vertices[t] {
                self.vertex_island[v as usize] = 2;
                self.land_vertices.insert(v);
            }
            for &e in &topo.tile_edges[t] {
                self.land_edges.insert(e);
                // Outer border edges of newly revealed land face the open sea.
                if topo.coast_edges.contains(&e) {
                    self.sea_edges.insert(e);
                }
            }
        }
    }
}

fn has_adjacent_red_numbers(topo: &Topology, numbers: &[u8; MAX_TILES]) -> bool {
    let red = |n: u8| n == 6 || n == 8;
    (0..topo.num_tiles).any(|t| {
        red(numbers[t])
            && topo.tile_neighbors[t]
                .iter()
                .any(|&n| n != NONE && red(numbers[n as usize]))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_boards_are_valid_and_balanced() {
        let mut rng = Rng::new(123);
        for (layout, harbors, tokens) in
            [(BoardLayout::Base, 9, 18), (BoardLayout::Extended, 11, 28)]
        {
            let topo = layout.topology();
            for _ in 0..300 {
                let b = Board::generate(&mut rng, layout, true);
                assert!(!has_adjacent_red_numbers(topo, &b.numbers));
                assert_eq!(b.terrain[b.desert as usize], Terrain::Desert);
                assert_eq!(
                    b.vertex_harbor.iter().filter(|&&h| h != 0).count(),
                    2 * harbors
                );
                let placed = b
                    .tiles_by_number
                    .iter()
                    .flatten()
                    .filter(|&&t| t != NONE)
                    .count();
                assert_eq!(placed, tokens);
                assert!(
                    b.terrain[topo.num_tiles..]
                        .iter()
                        .all(|&t| t == Terrain::Desert)
                );
                assert!(b.numbers[topo.num_tiles..].iter().all(|&n| n == 0));
            }
        }
    }

    #[test]
    fn rejects_bad_layouts() {
        let b = Board::generate(&mut Rng::new(1), BoardLayout::Base, false);
        let mut numbers = b.numbers;
        numbers[b.desert as usize] = 5;
        assert_eq!(
            Board::from_layout(b.layout, b.tiles(), &numbers[..19], b.harbor_kinds()),
            Err(BoardError::NumberOnDesert)
        );
        let mut terrain = b.terrain;
        terrain[b.desert as usize] = Terrain::Forest;
        assert_eq!(
            Board::from_layout(b.layout, &terrain[..19], &b.numbers[..19], b.harbor_kinds()),
            Err(BoardError::WrongTerrainCount)
        );
        assert_eq!(
            Board::from_layout(
                BoardLayout::Extended,
                b.tiles(),
                &b.numbers[..19],
                b.harbor_kinds()
            ),
            Err(BoardError::WrongTerrainCount)
        );
        assert_eq!(
            Board::from_layout(b.layout, b.tiles(), &b.numbers[..19], &b.harbors[..8]),
            Err(BoardError::WrongHarborCount)
        );
        assert_eq!(
            Board::from_layout(b.layout, b.tiles(), &b.numbers[..19], b.harbor_kinds()),
            Ok(b)
        );
    }

    #[test]
    fn custom_map_round_trips_and_matches_generated_boards() {
        let mut rng = Rng::new(2026);
        for layout in [
            BoardLayout::Base,
            BoardLayout::Extended,
            BoardLayout::Archipelago,
            BoardLayout::FogIslands,
            BoardLayout::ArchipelagoExtended,
            BoardLayout::FogIslandsExtended,
        ] {
            for _ in 0..8 {
                let b = Board::generate(&mut rng, layout, true);
                let cm = CustomMap::from_board(&b);
                let code = cm.encode();
                let decoded = CustomMap::decode(&format!("  {} \n", code.to_ascii_lowercase()))
                    .expect("valid code");
                assert_eq!(decoded, cm);
                assert_eq!(decoded.encode(), code);
                let rebuilt = Board::from_custom(&decoded).expect("valid custom board");
                assert_eq!(rebuilt, b, "layout {layout:?}");
            }
        }
    }

    #[test]
    fn custom_map_validation_errors_and_island_components() {
        // Codec errors.
        assert_eq!(CustomMap::decode(""), Err(BoardError::BadCustomCode));
        assert_eq!(
            CustomMap::decode("IM2:B:L2:3"),
            Err(BoardError::BadCustomCode)
        );
        assert_eq!(
            CustomMap::decode("IM1:Q:L2:3"),
            Err(BoardError::BadCustomCode)
        );
        assert_eq!(
            CustomMap::decode("IM1:B:D0:333333333"),
            Err(BoardError::BadCustomCode)
        );

        let base = CustomMap::from_board(&Board::generate(
            &mut Rng::new(7),
            BoardLayout::Base,
            false,
        ));
        let base_code = base.encode();
        assert_eq!(
            CustomMap::decode(&format!("{base_code}:extra")),
            Err(BoardError::BadCustomCode)
        );

        // MissingDesert.
        let mut no_desert = base;
        for t in 0..19 {
            if no_desert.terrain[t] == Terrain::Desert {
                no_desert.terrain[t] = Terrain::Forest;
                no_desert.numbers[t] = 6;
            }
        }
        assert_eq!(
            Board::from_custom(&no_desert),
            Err(BoardError::MissingDesert)
        );

        // UnexpectedSea / UnexpectedFog on Base.
        let mut sea_on_base = base;
        sea_on_base.terrain[0] = Terrain::Sea;
        sea_on_base.numbers[0] = 0;
        assert_eq!(
            Board::from_custom(&sea_on_base),
            Err(BoardError::UnexpectedSea)
        );
        let mut fog_on_base = base;
        fog_on_base.terrain[0] = Terrain::Fog;
        fog_on_base.numbers[0] = 0;
        assert_eq!(
            Board::from_custom(&fog_on_base),
            Err(BoardError::UnexpectedFog)
        );

        // 12 tiles sharing one number is allowed; 13 is rejected.
        let mut twelve_sixes = base;
        for t in 0..19 {
            if t == 0 {
                twelve_sixes.terrain[t] = Terrain::Desert;
                twelve_sixes.numbers[t] = 0;
            } else {
                twelve_sixes.terrain[t] = Terrain::Gold;
                twelve_sixes.numbers[t] = if t <= 12 { 6 } else { 8 };
            }
        }
        assert!(Board::from_custom(&twelve_sixes).is_ok());
        let mut thirteen_sixes = twelve_sixes;
        thirteen_sixes.numbers[13] = 6;
        assert_eq!(
            Board::from_custom(&thirteen_sixes),
            Err(BoardError::InvalidNumber)
        );

        // MissingSea on Archipelago.
        let arch = CustomMap::from_board(&Board::generate(
            &mut Rng::new(9),
            BoardLayout::Archipelago,
            false,
        ));
        let mut no_sea = arch;
        for t in 0..33 {
            if no_sea.terrain[t] == Terrain::Sea {
                no_sea.terrain[t] = Terrain::Desert;
                no_sea.numbers[t] = 0;
            }
        }
        assert_eq!(Board::from_custom(&no_sea), Err(BoardError::MissingSea));

        // FogTouchesLand and TooMuchFog on FogIslands / FogIslandsExtended.
        let fog = CustomMap::from_board(&Board::generate(
            &mut Rng::new(11),
            BoardLayout::FogIslands,
            false,
        ));
        let mut touches = fog;
        // Tile 3 on FOG_ISLANDS is the sea channel separating home land from fog.
        touches.terrain[3] = Terrain::Fog;
        touches.numbers[3] = 0;
        assert_eq!(
            Board::from_custom(&touches),
            Err(BoardError::FogTouchesLand)
        );

        let fog_ext = CustomMap::from_board(&Board::generate(
            &mut Rng::new(12),
            BoardLayout::FogIslandsExtended,
            false,
        ));
        let mut too_much_fog = fog_ext;
        let mut made_fog = 0;
        for t in 0..46 {
            if t == 0 {
                too_much_fog.terrain[t] = Terrain::Desert;
                too_much_fog.numbers[t] = 0;
            } else if t == 1 {
                too_much_fog.terrain[t] = Terrain::Sea;
                too_much_fog.numbers[t] = 0;
            } else if made_fog < 25 {
                too_much_fog.terrain[t] = Terrain::Fog;
                too_much_fog.numbers[t] = 0;
                made_fog += 1;
            }
        }
        assert_eq!(
            Board::from_custom(&too_much_fog),
            Err(BoardError::TooMuchFog)
        );

        // TooLittleLand: Archipelago with only 2 land tiles (< 29 setup vertices).
        let mut tiny = arch;
        for t in 0..33 {
            tiny.terrain[t] = if t == 0 {
                Terrain::Desert
            } else if t == 1 {
                Terrain::Forest
            } else {
                Terrain::Sea
            };
            tiny.numbers[t] = if t == 1 { 8 } else { 0 };
        }
        assert_eq!(Board::from_custom(&tiny), Err(BoardError::TooLittleLand));

        // Small scattered islands on Archipelago: island 1 < 29 vertices, but
        // total land_vertices >= 29 -> setup_vertices falls back to land_vertices.
        let mut scattered = arch;
        // Split home island by turning middle row into Sea while keeping east islands.
        for t in [13, 14, 15] {
            scattered.terrain[t] = Terrain::Sea;
            scattered.numbers[t] = 0;
        }
        // Ensure at least one desert remains on top half.
        scattered.terrain[0] = Terrain::Desert;
        scattered.numbers[0] = 0;
        let sb = Board::from_custom(&scattered).expect("scattered archipelago valid");
        assert!(sb.island_count() >= 4);
        assert_eq!(sb.setup_vertices, sb.land_vertices);
    }
}
