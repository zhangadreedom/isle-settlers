//! WebAssembly bindings.
//!
//! [`LocalMatch`] hosts a whole game in the browser: the engine and the bots
//! run inside WASM, and JavaScript only ever receives the human seat's
//! [`PlayerView`](isle_proto::PlayerView) plus events redacted for that seat,
//! exactly what a server will send in online play.

mod host;

pub use host::{BotStep, HostError, MatchHost};
use isle_ai::{Bot, WeightedRandomBot};
use isle_core::topology::*;
use isle_core::{Action, GameConfig, GameState, MAX_PLAYERS, ResourceSet};
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Values cross the boundary as JSON text: `serde_json` plus the browser's
/// native `JSON.parse` is compact and fast, and the browser never has to
/// encode actions (it picks them by index from `legalActions`).
fn to_js<T: Serialize>(value: &T) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(|e| JsError::new(&e.to_string()))
}

fn err(e: HostError) -> JsError {
    JsError::new(&e.code())
}

/// Legal actions together with the state version they belong to.
#[derive(Serialize)]
struct LegalActions<'a> {
    version: u32,
    actions: &'a [Action],
}

#[wasm_bindgen]
pub struct LocalMatch {
    host: MatchHost,
}

#[wasm_bindgen]
impl LocalMatch {
    /// New match. `human_seat` is the human's seat, or -1 to watch bots only.
    /// `bot` is a bot name (`greedy`, `weighted`, `random`).
    #[wasm_bindgen(constructor)]
    pub fn new(
        num_players: u8,
        seed: u32,
        human_seat: i32,
        bot: &str,
    ) -> Result<LocalMatch, JsError> {
        let human = u8::try_from(human_seat).ok();
        Ok(LocalMatch {
            host: MatchHost::new(num_players, seed as u64, human, bot).map_err(err)?,
        })
    }

    /// Static board layout (`BoardInfo` JSON).
    pub fn board(&self) -> Result<String, JsError> {
        to_js(&isle_proto::board_info(&self.host.state))
    }

    /// Current state as seen by the human seat (`PlayerView` JSON).
    pub fn view(&self) -> Result<String, JsError> {
        to_js(&isle_proto::player_view(&self.host.state, self.host.human))
    }

    /// Legal actions for the human seat: `{ version, actions: Action[] }`.
    /// Open-ended trade proposals are not listed; use `proposeTrade`.
    #[wasm_bindgen(js_name = legalActions)]
    pub fn legal_actions(&mut self) -> Result<String, JsError> {
        let version = self.host.version;
        to_js(&LegalActions {
            version,
            actions: self.host.legal_actions(),
        })
    }

    /// Plays `actions[index]` from the `legalActions()` result tagged with
    /// `version`. Returns the resulting events (`Event[]` JSON); throws with
    /// `Stale`, `NoSuchAction` or a `RuleError` name.
    pub fn apply(&mut self, version: u32, index: u32) -> Result<String, JsError> {
        to_js(
            &self
                .host
                .apply_index(version, index as usize)
                .map_err(err)?,
        )
    }

    /// Offers `give` for `get` (5 counts each, in resource order) to the
    /// other players. Returns events; throws with a `RuleError` name.
    #[wasm_bindgen(js_name = proposeTrade)]
    pub fn propose_trade(&mut self, give: &[u8], get: &[u8]) -> Result<String, JsError> {
        let set = |v: &[u8]| -> Result<ResourceSet, JsError> {
            Ok(ResourceSet(
                v.try_into().map_err(|_| JsError::new("InvalidTrade"))?,
            ))
        };
        let (give, get) = (set(give)?, set(get)?);
        to_js(&self.host.propose_trade(give, get).map_err(err)?)
    }

    /// Seat of the bot that should act now, or -1 when waiting for the
    /// human (or the game is over).
    #[wasm_bindgen(js_name = nextBot)]
    pub fn next_bot(&self) -> i32 {
        self.host.next_bot().map_or(-1, i32::from)
    }

    /// Lets the next bot act once. Returns `{ player, action, events }`
    /// JSON (events redacted for the human), or `null`.
    #[wasm_bindgen(js_name = stepBot)]
    pub fn step_bot(&mut self) -> Result<String, JsError> {
        match self.host.step_bot() {
            Some(step) => to_js(&step),
            None => Ok("null".into()),
        }
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

/// Corners of each tile (6 per tile, clockwise from the top), flattened.
#[wasm_bindgen(js_name = tileVertices)]
pub fn tile_vertices() -> Vec<u8> {
    TILE_VERTICES.iter().flatten().copied().collect()
}

/// Coastal edge of each harbor slot.
#[wasm_bindgen(js_name = harborEdges)]
pub fn harbor_edges() -> Vec<u8> {
    HARBOR_EDGES.to_vec()
}
