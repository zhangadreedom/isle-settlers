//! # isle-ai
//!
//! Computer opponents behind one [`Bot`] trait:
//!
//! * [`RandomBot`] / [`WeightedRandomBot`]: baselines for tests and benchmarks.
//! * [`GreedyBot`]: plan-based heuristic player ("normal" difficulty) that
//!   only uses public information and its own hand.
//! * [`MctsBot`]: information-set Monte Carlo tree search ("hard"). It only
//!   ever searches [`determinize`]d states, so it cannot see hidden cards.

pub mod determinize;
pub mod eval;
mod greedy;
mod mcts;
mod sea;

pub use greedy::GreedyBot;
use isle_core::{Action, GameState, Rng};
pub use mcts::{MctsBot, MctsConfig, SearchStats};

/// A computer player.
pub trait Bot {
    fn name(&self) -> &'static str;

    /// Picks one of `legal` (the output of `state.legal_actions(player)`,
    /// never empty) for `player`.
    ///
    /// Bots receive the full state for now; once search bots land they will
    /// receive a per-player view so they cannot see hidden cards or the RNG.
    fn choose(&mut self, state: &GameState, player: u8, legal: &[Action]) -> Action;
}

/// Uniformly random legal moves.
pub struct RandomBot {
    rng: Rng,
}

impl RandomBot {
    pub fn new(seed: u64) -> Self {
        RandomBot {
            rng: Rng::new(seed),
        }
    }
}

impl Bot for RandomBot {
    fn name(&self) -> &'static str {
        "random"
    }

    fn choose(&mut self, _state: &GameState, _player: u8, legal: &[Action]) -> Action {
        legal[self.rng.below(legal.len() as u32) as usize]
    }
}

/// Random moves biased towards building, so games actually finish.
pub struct WeightedRandomBot {
    rng: Rng,
}

impl WeightedRandomBot {
    pub fn new(seed: u64) -> Self {
        WeightedRandomBot {
            rng: Rng::new(seed),
        }
    }

    fn weight(action: &Action) -> u32 {
        match action {
            Action::BuildCity(_) => 10_000,
            Action::BuildSettlement(_) => 1_000,
            Action::BuyDevCard => 100,
            Action::BuildRoad(_) | Action::BuildShip(_) => 10,
            _ => 1,
        }
    }
}

impl Bot for WeightedRandomBot {
    fn name(&self) -> &'static str {
        "weighted-random"
    }

    fn choose(&mut self, _state: &GameState, _player: u8, legal: &[Action]) -> Action {
        let total: u32 = legal.iter().map(Self::weight).sum();
        let mut x = self.rng.below(total);
        for a in legal {
            let w = Self::weight(a);
            if x < w {
                return *a;
            }
            x -= w;
        }
        *legal.last().unwrap()
    }
}

/// Names accepted by [`bot_by_name`].
pub const BOT_NAMES: [&str; 4] = ["random", "weighted", "greedy", "mcts"];

/// Creates a bot from its name: `random`, `weighted`, `greedy` or `mcts`.
/// The search bot takes options: `mcts:N` sets the iterations per decision,
/// and `mcts:N/key=value/...` overrides settings (see [`MctsConfig::set`]).
pub fn bot_by_name(name: &str, seed: u64) -> Option<Box<dyn Bot + Send>> {
    Some(match name {
        "random" => Box::new(RandomBot::new(seed)),
        "weighted" => Box::new(WeightedRandomBot::new(seed)),
        "greedy" => Box::new(GreedyBot::new(seed)),
        "mcts" => Box::new(MctsBot::new(seed)),
        _ => {
            let mut parts = name.strip_prefix("mcts:")?.split('/');
            let mut config = MctsConfig {
                iterations: parts.next()?.parse().ok().filter(|&n| n > 0)?,
                ..MctsConfig::default()
            };
            for kv in parts {
                let (key, value) = kv.split_once('=')?;
                config.set(key, value.parse().ok()?)?;
            }
            Box::new(MctsBot::with_config(seed, config))
        }
    })
}

/// Result of [`play_game`].
#[derive(Clone, Copy, Debug)]
pub struct GameOutcome {
    pub winner: Option<u8>,
    pub turns: u32,
    pub actions: u64,
}

/// Plays `state` to the end (or `max_turns`) with one bot per seat.
/// With `check_invariants`, the state is verified after every action.
pub fn play_game(
    state: &mut GameState,
    bots: &mut [&mut dyn Bot],
    max_turns: u32,
    check_invariants: bool,
) -> GameOutcome {
    assert_eq!(bots.len(), state.num_players(), "one bot per seat");
    let mut legal = Vec::with_capacity(128);
    let mut actions = 0u64;
    while let Some(p) = state.to_move() {
        if state.turn >= max_turns {
            break;
        }
        state.legal_actions(p, &mut legal);
        let action = bots[p as usize].choose(state, p, &legal);
        state
            .apply(p, action)
            .expect("bots only play legal actions");
        actions += 1;
        if check_invariants && let Err(e) = state.check_invariants() {
            panic!("invariant violated after {action:?}: {e}");
        }
    }
    GameOutcome {
        winner: state.winner,
        turns: state.turn,
        actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isle_core::GameConfig;

    #[test]
    fn weighted_bots_finish_games() {
        let mut finished = 0;
        for seed in 0..50 {
            let mut g = GameState::new(GameConfig::default(), seed).unwrap();
            let mut b: Vec<WeightedRandomBot> = (0..4)
                .map(|i| WeightedRandomBot::new(seed * 10 + i))
                .collect();
            let mut bots: Vec<&mut dyn Bot> = b.iter_mut().map(|b| b as &mut dyn Bot).collect();
            let out = play_game(&mut g, &mut bots, 1000, true);
            if out.winner.is_some() {
                finished += 1;
            }
        }
        assert!(finished >= 45, "{finished}/50 finished");
    }

    #[test]
    fn greedy_bots_sail_to_the_islands() {
        let (mut finished, mut islands) = (0, 0);
        for seed in 0..40 {
            let config = GameConfig::scenario(isle_core::Scenario::Archipelago, 4);
            let mut g = GameState::new(config, seed).unwrap();
            let mut b: Vec<GreedyBot> = (0..4).map(|i| GreedyBot::new(seed * 10 + i)).collect();
            let mut bots: Vec<&mut dyn Bot> = b.iter_mut().map(|b| b as &mut dyn Bot).collect();
            let out = play_game(&mut g, &mut bots, 1000, true);
            finished += out.winner.is_some() as u32;
            islands += (0..4)
                .map(|p| g.players[p].islands.count_ones())
                .sum::<u32>();
        }
        assert_eq!(finished, 40);
        assert!(islands >= 40, "only {islands} island bonuses in 40 games");
    }

    /// Plays `games` games with a fixed lineup; returns wins per seat.
    fn tournament(games: u64, make: impl Fn(usize, u64) -> Box<dyn Bot>) -> [u32; 4] {
        let mut wins = [0; 4];
        for seed in 0..games {
            let mut g = GameState::new(GameConfig::default(), 1000 + seed).unwrap();
            let mut b: Vec<Box<dyn Bot>> = (0..4).map(|i| make(i, seed * 4 + i as u64)).collect();
            let mut bots: Vec<&mut dyn Bot> =
                b.iter_mut().map(|b| &mut **b as &mut dyn Bot).collect();
            if let Some(w) = play_game(&mut g, &mut bots, 1000, true).winner {
                wins[w as usize] += 1;
            }
        }
        wins
    }

    #[test]
    fn greedy_beats_weighted_random() {
        let wins = tournament(100, |seat, seed| -> Box<dyn Bot> {
            if seat == 0 {
                Box::new(GreedyBot::new(seed))
            } else {
                Box::new(WeightedRandomBot::new(seed))
            }
        });
        assert!(
            wins[0] >= 70,
            "greedy won only {} of 100 against three weighted-random bots",
            wins[0]
        );
    }

    #[test]
    fn greedy_self_play_finishes() {
        let wins = tournament(60, |_, seed| -> Box<dyn Bot> {
            Box::new(GreedyBot::new(seed))
        });
        let finished: u32 = wins.iter().sum();
        assert!(finished >= 57, "only {finished}/60 greedy games finished");
    }
}
