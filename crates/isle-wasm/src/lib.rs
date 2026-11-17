//! WebAssembly bindings. Data crosses the boundary as typed arrays (no
//! serialization): the browser renders straight from engine tables.

use isle_ai::{Bot, WeightedRandomBot};
use isle_core::topology::*;
use isle_core::{Action, GameConfig, GameState, MAX_PLAYERS};
use wasm_bindgen::prelude::*;

/// Value used in per-spot arrays for "nobody".
pub const EMPTY: u8 = 255;
/// Flag OR-ed into a building's owner when it is a city.
pub const CITY_FLAG: u8 = 0x80;

#[wasm_bindgen]
pub struct Game {
    state: GameState,
    bots: Vec<WeightedRandomBot>,
    legal: Vec<Action>,
}

#[wasm_bindgen]
impl Game {
    /// New game with a random board. `seed` seeds both the board and the bots.
    #[wasm_bindgen(constructor)]
    pub fn new(num_players: u8, seed: u32) -> Result<Game, JsError> {
        let config = GameConfig {
            num_players,
            ..GameConfig::default()
        };
        let state = GameState::new(config, seed as u64)
            .map_err(|e| JsError::new(&format!("invalid config: {e:?}")))?;
        let bots = (0..num_players as u64)
            .map(|p| WeightedRandomBot::new(((seed as u64) << 8) | p))
            .collect();
        Ok(Game {
            state,
            bots,
            legal: Vec::with_capacity(128),
        })
    }

    #[wasm_bindgen(getter, js_name = numPlayers)]
    pub fn num_players(&self) -> u8 {
        self.state.config.num_players
    }

    /// Terrain code per tile (see `Terrain`).
    pub fn terrain(&self) -> Vec<u8> {
        self.state.board.terrain.iter().map(|&t| t as u8).collect()
    }

    /// Number token per tile, 0 for the desert.
    pub fn numbers(&self) -> Vec<u8> {
        self.state.board.numbers.to_vec()
    }

    /// Harbor code per harbor slot: 0-4 special (resource), 5 generic 3:1.
    pub fn harbors(&self) -> Vec<u8> {
        self.state.board.harbors.iter().map(|h| h.code()).collect()
    }

    #[wasm_bindgen(getter)]
    pub fn robber(&self) -> u8 {
        self.state.robber
    }

    #[wasm_bindgen(getter)]
    pub fn turn(&self) -> u32 {
        self.state.turn
    }

    /// Winning seat, or -1.
    #[wasm_bindgen(getter)]
    pub fn winner(&self) -> i32 {
        self.state.winner.map_or(-1, i32::from)
    }

    /// Per vertex: `EMPTY`, the owner seat, or owner | `CITY_FLAG`.
    pub fn buildings(&self) -> Vec<u8> {
        let mut out = vec![EMPTY; NUM_VERTICES];
        for p in 0..self.state.num_players() {
            for v in vertices_in(self.state.settlements[p]) {
                out[v as usize] = p as u8;
            }
            for v in vertices_in(self.state.cities[p]) {
                out[v as usize] = p as u8 | CITY_FLAG;
            }
        }
        out
    }

    /// Per edge: `EMPTY` or the owner seat.
    pub fn roads(&self) -> Vec<u8> {
        let mut out = vec![EMPTY; NUM_EDGES];
        for p in 0..self.state.num_players() {
            for e in edges_in(self.state.roads[p]) {
                out[e as usize] = p as u8;
            }
        }
        out
    }

    /// Total victory points per seat (including hidden cards).
    #[wasm_bindgen(js_name = victoryPoints)]
    pub fn victory_points(&self) -> Vec<u8> {
        (0..self.state.num_players() as u8)
            .map(|p| self.state.victory_points(p))
            .collect()
    }

    /// Cards in hand per seat.
    #[wasm_bindgen(js_name = handSizes)]
    pub fn hand_sizes(&self) -> Vec<u8> {
        (0..self.state.num_players())
            .map(|p| self.state.players[p].hand.total() as u8)
            .collect()
    }

    /// Lets the built-in bots play up to `max_actions` actions. Returns
    /// `true` while the game is still running.
    pub fn step(&mut self, max_actions: u32) -> bool {
        for _ in 0..max_actions {
            let Some(p) = self.state.to_move() else {
                return false;
            };
            self.state.legal_actions(p, &mut self.legal);
            let a = self.bots[p as usize].choose(&self.state, p, &self.legal);
            self.state.apply(p, a).expect("bots play legal actions");
        }
        self.state.to_move().is_some()
    }
}

/// Plays `games` complete bot-vs-bot games and returns the number of actions
/// applied. Used to measure engine speed inside the browser.
#[wasm_bindgen(js_name = runBenchmark)]
pub fn run_benchmark(games: u32, seed: u32) -> f64 {
    let mut actions = 0u64;
    for i in 0..games {
        let s = ((seed as u64) << 32) | i as u64;
        let mut g = GameState::new(GameConfig::default(), s).expect("valid");
        let mut b: Vec<WeightedRandomBot> = (0..MAX_PLAYERS as u64)
            .map(|k| WeightedRandomBot::new(s ^ (k + 1)))
            .collect();
        let mut bots: Vec<&mut dyn Bot> = b.iter_mut().map(|b| b as &mut dyn Bot).collect();
        actions += isle_ai::play_game(&mut g, &mut bots, 1000, false).actions;
    }
    actions as f64
}

/// Lattice position `(x, y)` of each tile centre, interleaved.
#[wasm_bindgen(js_name = tilePositions)]
pub fn tile_positions() -> Vec<i16> {
    TILE_POS.iter().flat_map(|&(x, y)| [x, y]).collect()
}

/// Lattice position `(x, y)` of each vertex, interleaved.
#[wasm_bindgen(js_name = vertexPositions)]
pub fn vertex_positions() -> Vec<i16> {
    VERTEX_POS.iter().flat_map(|&(x, y)| [x, y]).collect()
}

/// Endpoints of each edge, interleaved.
#[wasm_bindgen(js_name = edgeVertices)]
pub fn edge_vertices() -> Vec<u8> {
    EDGE_VERTICES.iter().flatten().copied().collect()
}

/// Coastal edge of each harbor slot.
#[wasm_bindgen(js_name = harborEdges)]
pub fn harbor_edges() -> Vec<u8> {
    HARBOR_EDGES.to_vec()
}
