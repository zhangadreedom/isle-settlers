//! Board layout: terrain, number tokens and harbors on top of the static topology.

use crate::resources::{HarborKind, Resource, Terrain};
use crate::rng::Rng;
use crate::topology::*;

/// Terrain tiles of the base game.
pub const TERRAIN_POOL: [Terrain; NUM_TILES] = {
    use Terrain::*;
    [
        Forest, Forest, Forest, Forest, Pasture, Pasture, Pasture, Pasture, Fields, Fields, Fields,
        Fields, Hills, Hills, Hills, Mountains, Mountains, Mountains, Desert,
    ]
};

/// Number tokens of the base game (one per non-desert tile).
pub const NUMBER_POOL: [u8; NUM_TILES - 1] =
    [2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12];

/// Harbors of the base game: four generic and one special per resource.
pub const HARBOR_POOL: [HarborKind; NUM_HARBORS] = [
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

/// Number of dice combinations producing each roll (out of 36).
pub const PIPS: [u8; 13] = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Board {
    pub terrain: [Terrain; NUM_TILES],
    /// Number token per tile, `0` for the desert.
    pub numbers: [u8; NUM_TILES],
    /// Harbor kind per slot of `HARBOR_EDGES`.
    pub harbors: [HarborKind; NUM_HARBORS],
    /// Harbor-access bit granted by building on each vertex (`0` if none).
    pub vertex_harbor: [u8; NUM_VERTICES],
    /// Tiles carrying each number token, padded with `NONE`.
    pub tiles_by_number: [[u8; 2]; 13],
    /// Tile where the robber starts.
    pub desert: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardError {
    WrongTerrainCount,
    NumberOnDesert,
    MissingNumber,
    InvalidNumber,
}

impl Board {
    /// Random board. With `balanced`, the high-probability tokens 6 and 8
    /// are never placed on adjacent tiles.
    pub fn generate(rng: &mut Rng, balanced: bool) -> Board {
        let mut terrain = TERRAIN_POOL;
        rng.shuffle(&mut terrain);
        let mut harbors = HARBOR_POOL;
        rng.shuffle(&mut harbors);

        let mut numbers = [0u8; NUM_TILES];
        for _attempt in 0..10_000 {
            let mut pool = NUMBER_POOL;
            rng.shuffle(&mut pool);
            let mut it = pool.iter();
            for t in 0..NUM_TILES {
                numbers[t] = if terrain[t] == Terrain::Desert {
                    0
                } else {
                    *it.next().unwrap()
                };
            }
            if !balanced || !has_adjacent_red_numbers(&numbers) {
                break;
            }
        }
        Board::from_layout(terrain, numbers, harbors).expect("generated board is valid")
    }

    /// Builds a board from an explicit layout (custom maps, tests).
    pub fn from_layout(
        terrain: [Terrain; NUM_TILES],
        numbers: [u8; NUM_TILES],
        harbors: [HarborKind; NUM_HARBORS],
    ) -> Result<Board, BoardError> {
        let mut sorted_terrain = terrain;
        sorted_terrain.sort_by_key(|t| *t as u8);
        let mut expected = TERRAIN_POOL;
        expected.sort_by_key(|t| *t as u8);
        if sorted_terrain != expected {
            return Err(BoardError::WrongTerrainCount);
        }
        let mut desert = NONE;
        let mut tiles_by_number = [[NONE; 2]; 13];
        for t in 0..NUM_TILES {
            let n = numbers[t];
            if terrain[t] == Terrain::Desert {
                if n != 0 {
                    return Err(BoardError::NumberOnDesert);
                }
                desert = t as u8;
                continue;
            }
            if n == 0 {
                return Err(BoardError::MissingNumber);
            }
            if !(2..=12).contains(&n) || n == 7 {
                return Err(BoardError::InvalidNumber);
            }
            let slot = &mut tiles_by_number[n as usize];
            if slot[0] == NONE {
                slot[0] = t as u8;
            } else if slot[1] == NONE {
                slot[1] = t as u8;
            } else {
                return Err(BoardError::InvalidNumber);
            }
        }
        let mut vertex_harbor = [0u8; NUM_VERTICES];
        for (slot, &e) in HARBOR_EDGES.iter().enumerate() {
            for v in EDGE_VERTICES[e as usize] {
                vertex_harbor[v as usize] = harbors[slot].bit();
            }
        }
        Ok(Board {
            terrain,
            numbers,
            harbors,
            vertex_harbor,
            tiles_by_number,
            desert,
        })
    }

    /// Production weight (pips) of a vertex, summed over adjacent tiles.
    pub fn vertex_pips(&self, v: u8) -> u8 {
        VERTEX_TILES[v as usize]
            .iter()
            .filter(|&&t| t != NONE)
            .map(|&t| PIPS[self.numbers[t as usize] as usize])
            .sum()
    }
}

fn has_adjacent_red_numbers(numbers: &[u8; NUM_TILES]) -> bool {
    let red = |n: u8| n == 6 || n == 8;
    (0..NUM_TILES).any(|t| {
        red(numbers[t])
            && TILE_NEIGHBORS[t]
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
        for _ in 0..500 {
            let b = Board::generate(&mut rng, true);
            assert!(!has_adjacent_red_numbers(&b.numbers));
            assert_eq!(b.terrain[b.desert as usize], Terrain::Desert);
            assert_eq!(b.vertex_harbor.iter().filter(|&&h| h != 0).count(), 18);
            let tokens: u32 = b
                .tiles_by_number
                .iter()
                .flatten()
                .filter(|&&t| t != NONE)
                .count() as u32;
            assert_eq!(tokens, 18);
        }
    }

    #[test]
    fn rejects_bad_layouts() {
        let b = Board::generate(&mut Rng::new(1), false);
        let mut numbers = b.numbers;
        numbers[b.desert as usize] = 5;
        assert_eq!(
            Board::from_layout(b.terrain, numbers, b.harbors),
            Err(BoardError::NumberOnDesert)
        );
        let mut terrain = b.terrain;
        terrain[b.desert as usize] = Terrain::Forest;
        assert_eq!(
            Board::from_layout(terrain, b.numbers, b.harbors),
            Err(BoardError::WrongTerrainCount)
        );
    }
}
