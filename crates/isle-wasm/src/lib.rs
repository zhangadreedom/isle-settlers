//! WebAssembly bindings.
//!
//! [`LocalMatch`] hosts a whole game in the browser: the engine and the bots
//! run inside WASM, and JavaScript only ever receives the human seat's
//! [`PlayerView`](isle_proto::PlayerView) plus events redacted for that seat,
//! exactly what a server will send in online play.

mod host;

pub use host::{BotStep, HostError, MatchHost};
use isle_ai::{Bot, WeightedRandomBot};
use isle_core::{Action, BoardLayout, Event, GameConfig, GameState, MAX_PLAYERS, ResourceSet};
use isle_proto::ReplayError;
use isle_proto::replay::decode_actions;
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

    /// The game so far as a `ReplayRecord` (JSON).
    pub fn replay(&self, human_name: &str) -> Result<String, JsError> {
        to_js(&self.host.replay(human_name))
    }
}

/// Steps through a recorded game. Every state is rebuilt by the engine, so
/// any seat's private view can be shown; checkpoints make seeking cheap.
///
/// JavaScript parses the `ReplayRecord` file itself and passes its fields,
/// which keeps a JSON parser out of the WebAssembly binary.
#[wasm_bindgen]
pub struct Replay {
    actions: Vec<(u8, Action)>,
    /// State before action `i * CHECKPOINT_EVERY`.
    checkpoints: Vec<GameState>,
    state: GameState,
    position: usize,
    events: Vec<Event>,
}

const CHECKPOINT_EVERY: usize = 32;

/// One replayed action.
#[derive(Serialize)]
struct ReplayStep<'a> {
    player: u8,
    action: Action,
    events: &'a [Event],
}

#[wasm_bindgen]
impl Replay {
    /// Loads a game from its config, seed (decimal) and encoded action log
    /// (`ReplayRecord.actions`), checking it by playing it through. Throws
    /// with `BadReplaySeed`, `BadReplayConfig` or `BadReplayAction<i>`.
    #[wasm_bindgen(constructor)]
    pub fn new(
        num_players: u8,
        victory_points: u8,
        balanced_board: bool,
        seed: &str,
        actions: &[u8],
    ) -> Result<Replay, JsError> {
        let fail = |e: ReplayError| JsError::new(&e.to_string());
        let seed: u64 = seed.parse().map_err(|_| fail(ReplayError::Seed))?;
        let config = GameConfig {
            num_players,
            victory_points,
            balanced_board,
            ..GameConfig::default()
        };
        let start = GameState::new(config, seed).map_err(|_| fail(ReplayError::Config))?;
        let actions = decode_actions(actions).map_err(fail)?;
        let mut checkpoints = vec![start];
        let mut state = start;
        for (i, &(seat, action)) in actions.iter().enumerate() {
            state
                .apply(seat, action)
                .map_err(|_| fail(ReplayError::Action(i)))?;
            if (i + 1) % CHECKPOINT_EVERY == 0 {
                checkpoints.push(state);
            }
        }
        Ok(Replay {
            actions,
            checkpoints,
            state: start,
            position: 0,
            events: Vec::new(),
        })
    }

    /// Number of actions in the game.
    pub fn length(&self) -> u32 {
        self.actions.len() as u32
    }

    /// Actions applied so far.
    pub fn position(&self) -> u32 {
        self.position as u32
    }

    /// Static board layout (`BoardInfo` JSON).
    pub fn board(&self) -> Result<String, JsError> {
        to_js(&isle_proto::board_info(&self.state))
    }

    /// The current state as seen by `viewer` (-1 for a spectator).
    pub fn view(&self, viewer: i32) -> Result<String, JsError> {
        to_js(&isle_proto::player_view(
            &self.state,
            u8::try_from(viewer).ok(),
        ))
    }

    /// Jumps to the state after `position` actions.
    pub fn seek(&mut self, position: u32) {
        let target = (position as usize).min(self.actions.len());
        let k = (target / CHECKPOINT_EVERY).min(self.checkpoints.len() - 1);
        self.state = self.checkpoints[k];
        self.position = k * CHECKPOINT_EVERY;
        while self.position < target {
            let (seat, action) = self.actions[self.position];
            self.state.apply(seat, action).expect("checked on load");
            self.position += 1;
        }
        self.events.clear();
    }

    /// Applies the next action. Returns `{ player, action, events }` JSON
    /// with events redacted for `viewer` (-1 for a spectator), or `null` at
    /// the end.
    pub fn step(&mut self, viewer: i32) -> Result<String, JsError> {
        let Some(&(seat, action)) = self.actions.get(self.position) else {
            return Ok("null".into());
        };
        self.events.clear();
        self.state
            .apply_with(seat, action, &mut self.events)
            .expect("checked on load");
        self.position += 1;
        let viewer = u8::try_from(viewer).ok();
        let events: Vec<Event> = self.events.iter().map(|e| e.redacted_for(viewer)).collect();
        to_js(&ReplayStep {
            player: seat,
            action,
            events: &events,
        })
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

/// Flattened topology tables of one island, for the renderer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TopologyInfo {
    /// Lattice position `(x, y)` of each tile centre, interleaved.
    tile_pos: Vec<i16>,
    /// Lattice position `(x, y)` of each vertex, interleaved.
    vertex_pos: Vec<i16>,
    /// Endpoints of each edge, interleaved.
    edge_vertices: Vec<u8>,
    /// Corners of each tile (6 per tile, clockwise from the top), flattened.
    tile_vertices: Vec<u8>,
    /// Coastal edge of each harbor slot.
    harbor_edges: Vec<u8>,
}

/// Topology tables of `layout` (`"Base"` or `"Extended"`) as JSON.
#[wasm_bindgen]
pub fn topology(layout: &str) -> Result<String, JsError> {
    let layout = match layout {
        "Base" => BoardLayout::Base,
        "Extended" => BoardLayout::Extended,
        _ => return Err(JsError::new("UnknownLayout")),
    };
    let t = layout.topology();
    to_js(&TopologyInfo {
        tile_pos: t.tile_pos.iter().flat_map(|&(x, y)| [x, y]).collect(),
        vertex_pos: t.vertex_pos.iter().flat_map(|&(x, y)| [x, y]).collect(),
        edge_vertices: t.edge_vertices.iter().flatten().copied().collect(),
        tile_vertices: t.tile_vertices.iter().flatten().copied().collect(),
        harbor_edges: t.harbor_edges.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_seeking_matches_stepping() {
        let mut host = MatchHost::new(5, 21, None, "greedy").unwrap();
        while host.step_bot().is_some() {}
        let record = host.replay("");
        let c = record.config;
        let mut replay = Replay::new(
            c.num_players,
            c.victory_points,
            c.balanced_board,
            &record.seed,
            &record.actions,
        )
        .unwrap();
        let len = replay.length() as usize;
        assert!(len > 2 * CHECKPOINT_EVERY);
        let mut states = vec![replay.state];
        while replay.step(-1).unwrap() != "null" {
            states.push(replay.state);
        }
        assert_eq!(states.len(), len + 1);
        assert_eq!(replay.state, host.state);
        for pos in [len, 0, 31, 32, 33, len / 2, 1, len - 1, 64] {
            replay.seek(pos as u32);
            assert_eq!(replay.position() as usize, pos);
            assert_eq!(replay.state, states[pos], "seek to {pos}");
        }
        replay.seek(u32::MAX);
        assert_eq!(replay.position() as usize, len);
    }
}
