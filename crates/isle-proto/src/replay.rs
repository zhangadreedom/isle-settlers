//! Replays. The engine is deterministic, so a whole game is its
//! configuration, seed and action log; replaying the log rebuilds every
//! intermediate state exactly, hidden information included.

use isle_core::{Action, Event, GameConfig, GameState};
use serde::{Deserialize, Serialize};

use crate::net::BotLevel;

/// Who sat in a seat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReplayPlayer {
    /// Display name; empty for computer players.
    pub name: String,
    /// Strength of a computer player, `None` for humans.
    pub bot: Option<BotLevel>,
}

/// A recorded game, as stored by the server and in downloaded files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReplayRecord {
    pub format: u32,
    pub config: GameConfig,
    /// Game seed in decimal: a `u64` does not fit a JavaScript number.
    pub seed: String,
    pub players: Vec<ReplayPlayer>,
    /// Every action applied, in order, each as the acting seat followed by
    /// [`Action::encode`] bytes.
    pub actions: Vec<u8>,
    pub winner: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WinProbPoint {
    pub turn: u32,
    /// Win probability (0.0..=100.0) per seat.
    pub probs: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TurningPoint {
    /// 1-based action index after which the swing occurred (seek to `actionIndex` or `actionIndex - 1` in Replay).
    pub action_index: u32,
    pub turn: u32,
    pub player: u8,
    pub action: Action,
    pub before_pct: f32,
    pub after_pct: f32,
    pub delta_pct: f32,
    /// Tag such as `"LongestRoad"`, `"LargestArmy"`, `"Metropolis"`, `"CityBuilt"`, `"SettlementBuilt"`, `"Monopoly"`, `"BarbarianRaid"`, `"ExplorerMission"`, `"CamelCaravan"`, `"DiceSwing"`, `"TacticalPlay"`.
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReplayAnalysis {
    pub win_series: Vec<WinProbPoint>,
    pub turning_points: Vec<TurningPoint>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayError {
    Format,
    Seed,
    Config,
    /// The action at this index was rejected by the rules.
    Action(usize),
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayError::Format => f.write_str("UnsupportedReplay"),
            ReplayError::Seed => f.write_str("BadReplaySeed"),
            ReplayError::Config => f.write_str("BadReplayConfig"),
            ReplayError::Action(i) => write!(f, "BadReplayAction{i}"),
        }
    }
}

impl ReplayRecord {
    pub const FORMAT: u32 = 1;

    pub fn new(
        config: GameConfig,
        seed: u64,
        players: Vec<ReplayPlayer>,
        actions: &[(u8, Action)],
        winner: Option<u8>,
    ) -> ReplayRecord {
        ReplayRecord {
            format: Self::FORMAT,
            config,
            seed: seed.to_string(),
            players,
            actions: encode_actions(actions),
            winner,
        }
    }

    /// The decoded action log.
    pub fn actions(&self) -> Result<Vec<(u8, Action)>, ReplayError> {
        decode_actions(&self.actions)
    }

    /// The state before the first action.
    pub fn initial_state(&self) -> Result<GameState, ReplayError> {
        if self.format != Self::FORMAT {
            return Err(ReplayError::Format);
        }
        let seed: u64 = self.seed.parse().map_err(|_| ReplayError::Seed)?;
        if self.players.len() != self.config.num_players as usize {
            return Err(ReplayError::Config);
        }
        GameState::new(self.config, seed).map_err(|_| ReplayError::Config)
    }

    /// Replays every action, calling `step` after each with the new state,
    /// the actor, the action and its events. Returns the final state.
    pub fn play(
        &self,
        mut step: impl FnMut(&GameState, u8, Action, &[Event]),
    ) -> Result<GameState, ReplayError> {
        let mut state = self.initial_state()?;
        let mut events = Vec::new();
        for (i, (seat, action)) in self.actions()?.into_iter().enumerate() {
            events.clear();
            state
                .apply_with(seat, action, &mut events)
                .map_err(|_| ReplayError::Action(i))?;
            step(&state, seat, action, &events);
        }
        Ok(state)
    }
}

/// Seat byte plus [`Action::encode`] bytes per action.
pub fn encode_actions(actions: &[(u8, Action)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(actions.len() * 3);
    for &(seat, action) in actions {
        out.push(seat);
        action.encode(&mut out);
    }
    out
}

/// Inverse of [`encode_actions`]; fails on truncated or unknown codes with
/// the index of the bad action.
pub fn decode_actions(mut bytes: &[u8]) -> Result<Vec<(u8, Action)>, ReplayError> {
    let mut out = Vec::with_capacity(bytes.len() / 3);
    while let Some((&seat, rest)) = bytes.split_first() {
        let (action, used) = Action::decode(rest).ok_or(ReplayError::Action(out.len()))?;
        out.push((seat, action));
        bytes = &rest[used..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use isle_ai::{Bot, GreedyBot};

    fn record_game(players: u8, seed: u64) -> (ReplayRecord, GameState) {
        let config = GameConfig::new(players);
        let mut g = GameState::new(config, seed).unwrap();
        let mut bots: Vec<GreedyBot> = (0..players as u64)
            .map(|k| GreedyBot::new(seed + k))
            .collect();
        let mut actions = Vec::new();
        let mut legal = Vec::new();
        while let Some(p) = g.to_move() {
            g.legal_actions(p, &mut legal);
            let a = bots[p as usize].choose(&g, p, &legal);
            g.apply(p, a).unwrap();
            actions.push((p, a));
        }
        let players = (0..players)
            .map(|_| ReplayPlayer {
                name: String::new(),
                bot: Some(BotLevel::Normal),
            })
            .collect();
        (
            ReplayRecord::new(config, seed, players, &actions, g.winner),
            g,
        )
    }

    #[test]
    fn replays_reproduce_games_through_json() {
        for (players, seed) in [(4, u64::MAX - 3), (6, 17)] {
            let (record, end) = record_game(players, seed);
            let json = serde_json::to_string(&record).unwrap();
            let back: ReplayRecord = serde_json::from_str(&json).unwrap();
            assert_eq!(back, record);
            let mut steps = 0;
            let replayed = back.play(|_, _, _, _| steps += 1).unwrap();
            assert_eq!(replayed, end);
            assert_eq!(steps, record.actions().unwrap().len());
            // Three or four bytes per action on average.
            assert!(record.actions.len() < steps * 4);
            assert_eq!(replayed.winner, record.winner);
        }
    }

    #[test]
    fn corrupt_replays_are_rejected() {
        let (record, _) = record_game(3, 5);
        let mut bad = record.clone();
        bad.seed = "x".into();
        assert_eq!(bad.initial_state().unwrap_err(), ReplayError::Seed);
        let mut bad = record.clone();
        bad.format = 99;
        assert_eq!(bad.initial_state().unwrap_err(), ReplayError::Format);
        let mut bad = record.clone();
        bad.players.pop();
        assert_eq!(bad.initial_state().unwrap_err(), ReplayError::Config);
        // Seat 0's road before its first settlement is illegal.
        let mut log = record.actions().unwrap();
        log.swap(0, 1);
        let mut bad = record.clone();
        bad.actions = encode_actions(&log);
        assert_eq!(
            bad.play(|_, _, _, _| {}).unwrap_err(),
            ReplayError::Action(0)
        );
        // Truncated log.
        let mut bad = record;
        bad.actions.push(0);
        assert!(matches!(bad.actions(), Err(ReplayError::Action(_))));
    }
}
