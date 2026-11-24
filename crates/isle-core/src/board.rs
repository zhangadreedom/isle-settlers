//! Board layout: terrain, number tokens and harbors on top of a static topology.

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

/// Number tokens a gold field may carry: neither the likeliest (6, 8) nor
/// the rarest (2, 12).
pub const GOLD_NUMBERS: [u8; 6] = [3, 4, 5, 9, 10, 11];

/// Number of dice combinations producing each roll (out of 36).
pub const PIPS: [u8; 13] = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];

/// Most tiles sharing one number token (on the extended island).
pub const MAX_TILES_PER_NUMBER: usize = 3;

impl BoardLayout {
    /// Terrain tiles of a classic board, one per tile (empty for seafaring
    /// maps, whose terrain is placed island by island).
    pub fn terrain_pool(self) -> &'static [Terrain] {
        match self {
            BoardLayout::Base => &TERRAIN_POOL,
            BoardLayout::Extended => &EXTENDED_TERRAIN_POOL,
            BoardLayout::Archipelago => &[],
        }
    }

    /// Terrain counts of a board of this layout, indexed by `Terrain`.
    pub fn terrain_counts(self) -> [u8; Terrain::COUNT] {
        let mut n = [0u8; Terrain::COUNT];
        let mut add = |tiles: &[Terrain]| tiles.iter().for_each(|&t| n[t as usize] += 1);
        match self {
            BoardLayout::Base => add(&TERRAIN_POOL),
            BoardLayout::Extended => add(&EXTENDED_TERRAIN_POOL),
            BoardLayout::Archipelago => {
                add(&ARCHIPELAGO_HOME_POOL);
                add(&ARCHIPELAGO_OUTER_POOL);
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
        }
        n
    }

    /// Number tokens, one per producing tile.
    pub fn number_pool(self) -> &'static [u8] {
        match self {
            BoardLayout::Base => &NUMBER_POOL,
            BoardLayout::Extended => &EXTENDED_NUMBER_POOL,
            BoardLayout::Archipelago => &ARCHIPELAGO_NUMBER_POOL,
        }
    }

    /// Harbors, one per harbor slot of the topology.
    pub fn harbor_pool(self) -> &'static [HarborKind] {
        match self {
            BoardLayout::Extended => &EXTENDED_HARBOR_POOL,
            _ => &HARBOR_POOL,
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
    pub land_vertices: u128,
    /// Edges touching land: where roads may go.
    pub land_edges: u128,
    /// Edges touching the sea or the edge of the map: where ships may go.
    pub sea_edges: u128,
    /// Vertices open for initial settlements.
    pub setup_vertices: u128,
    /// Sea tile where the pirate starts, `NONE` without sea.
    pub pirate_start: u8,
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
    /// terrain shuffled, one gold field on each small island and the rest of
    /// their terrain shuffled; gold fields carry mid numbers only.
    fn generate_map(rng: &mut Rng, layout: BoardLayout, balanced: bool) -> Board {
        let topo = layout.topology();
        let map = layout.map().expect("seafaring layout");
        let nt = topo.num_tiles;
        let mut home = ARCHIPELAGO_HOME_POOL;
        let mut outer = ARCHIPELAGO_OUTER_POOL;
        rng.shuffle(&mut home);
        rng.shuffle(&mut outer);
        let (mut home, mut outer) = (home.into_iter(), outer.into_iter());
        // One gold field per small island, on a random tile of it.
        let mut gold = [false; MAX_TILES];
        let mut letters: Vec<u8> = map[..nt]
            .iter()
            .copied()
            .filter(|&c| c != b'M' && !is_sea_char(c))
            .collect();
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
            && (0..nt).any(|t| is_sea_char(map[t]) != (terrain[t] == Terrain::Sea))
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
            if terrain[t] == Terrain::Sea {
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
        let mut land_vertices = 0u128;
        let mut setup_vertices = 0u128;
        for v in 0..topo.num_vertices {
            for &t in &topo.vertex_tiles[v] {
                if t != NONE && tile_island[t as usize] != 0 {
                    vertex_island[v] = tile_island[t as usize];
                    land_vertices |= 1u128 << v;
                    // Seafaring maps start everyone on the home island.
                    if map.is_none() || tile_island[t as usize] == 1 {
                        setup_vertices |= 1u128 << v;
                    }
                }
            }
        }
        let mut edge_land = [false; MAX_EDGES];
        let mut edge_tiles = [0u8; MAX_EDGES];
        let mut edge_sea = [false; MAX_EDGES];
        for t in 0..nt {
            for &e in &topo.tile_edges[t] {
                edge_tiles[e as usize] += 1;
                if terrain[t] == Terrain::Sea {
                    edge_sea[e as usize] = true;
                } else {
                    edge_land[e as usize] = true;
                }
            }
        }
        let (mut land_edges, mut sea_edges) = (0u128, 0u128);
        for e in 0..topo.num_edges {
            if edge_land[e] {
                land_edges |= 1u128 << e;
            }
            // The map's border is open sea.
            if edge_sea[e] || edge_tiles[e] == 1 {
                sea_edges |= 1u128 << e;
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
        };
        board.terrain[..nt].copy_from_slice(terrain);
        board.numbers[..nt].copy_from_slice(numbers);
        board.harbors[..harbors.len()].copy_from_slice(harbors);
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

    /// Production weight (pips) of a vertex, summed over adjacent tiles.
    pub fn vertex_pips(&self, v: u8) -> u8 {
        self.topo().vertex_tiles[v as usize]
            .iter()
            .filter(|&&t| t != NONE)
            .map(|&t| PIPS[self.numbers[t as usize] as usize])
            .sum()
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
}
