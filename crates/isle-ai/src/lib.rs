//! # isle-ai
//!
//! Computer opponents. This first version ships the two baseline bots used
//! for testing and benchmarking; search-based bots (expectimax, ISMCTS) will
//! build on the same [`Bot`] trait.

use isle_core::{Action, GameState, Rng};

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
            Action::BuildRoad(_) => 10,
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
}
