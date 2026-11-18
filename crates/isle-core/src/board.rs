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

/// Number of dice combinations producing each roll (out of 36).
pub const PIPS: [u8; 13] = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];

/// Most tiles sharing one number token (on the extended island).
pub const MAX_TILES_PER_NUMBER: usize = 3;

impl BoardLayout {
    /// Terrain tiles, one per tile of the topology.
    pub fn terrain_pool(self) -> &'static [Terrain] {
        match self {
            BoardLayout::Base => &TERRAIN_POOL,
            BoardLayout::Extended => &EXTENDED_TERRAIN_POOL,
        }
    }

    /// Number tokens, one per non-desert tile.
    pub fn number_pool(self) -> &'static [u8] {
        match self {
            BoardLayout::Base => &NUMBER_POOL,
            BoardLayout::Extended => &EXTENDED_NUMBER_POOL,
        }
    }

    /// Harbors, one per harbor slot of the topology.
    pub fn harbor_pool(self) -> &'static [HarborKind] {
        match self {
            BoardLayout::Base => &HARBOR_POOL,
            BoardLayout::Extended => &EXTENDED_HARBOR_POOL,
        }
    }
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardError {
    WrongTerrainCount,
    WrongHarborCount,
    NumberOnDesert,
    MissingNumber,
    InvalidNumber,
}

impl Board {
    /// Random board. With `balanced`, the high-probability tokens 6 and 8
    /// are never placed on adjacent tiles.
    pub fn generate(rng: &mut Rng, layout: BoardLayout, balanced: bool) -> Board {
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
        let count = |tiles: &[Terrain]| {
            let mut n = [0u8; 6];
            for &t in tiles {
                n[t as usize] += 1;
            }
            n
        };
        if count(terrain) != count(layout.terrain_pool()) {
            return Err(BoardError::WrongTerrainCount);
        }
        let mut desert = NONE;
        let mut tiles_by_number = [[NONE; MAX_TILES_PER_NUMBER]; 13];
        for t in 0..nt {
            let n = numbers[t];
            if terrain[t] == Terrain::Desert {
                if n != 0 {
                    return Err(BoardError::NumberOnDesert);
                }
                if desert == NONE {
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
        let mut board = Board {
            layout,
            terrain: [Terrain::Desert; MAX_TILES],
            numbers: [0; MAX_TILES],
            harbors: [HarborKind::Generic; MAX_HARBORS],
            vertex_harbor,
            tiles_by_number,
            desert,
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
