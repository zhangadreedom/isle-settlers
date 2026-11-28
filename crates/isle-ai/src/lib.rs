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

    /// Enables or disables proactive domestic `ProposeTrade` offers.
    fn set_propose_trades(&mut self, _enabled: bool) {}
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
            Action::PassOldBoot(_) => 5_000,
            Action::BuildSettlement(_) => 1_000,
            Action::BuyDevCard | Action::FishBuyDevCard => 100,
            Action::FishBuildRoad(_) => 15,
            Action::BuildRoad(_) | Action::BuildShip(_) | Action::FishTakeResource(_) => 10,
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

/// Creates a bot from its name: `random`, `weighted`, `greedy`, `greedy:trade`
/// or `mcts`. The search bot takes options: `mcts:N` sets the iterations per
/// decision, and `mcts:N/key=value/...` overrides settings (see [`MctsConfig::set`]).
pub fn bot_by_name(name: &str, seed: u64) -> Option<Box<dyn Bot + Send>> {
    Some(match name {
        "random" => Box::new(RandomBot::new(seed)),
        "weighted" => Box::new(WeightedRandomBot::new(seed)),
        "greedy" => Box::new(GreedyBot::new(seed)),
        "greedy:trade" => {
            let mut bot = GreedyBot::new(seed);
            bot.set_propose_trades(true);
            Box::new(bot)
        }
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
    while let Some(next) = state.to_move() {
        if state.turn >= max_turns {
            break;
        }
        let p = if state.phase == isle_core::Phase::Main
            && let Some(offer) = state.trade
            && let Some(q) = (0..state.num_players())
                .find(|&q| offer.responses[q] == isle_core::TradeResponse::Pending)
        {
            q as u8
        } else {
            next
        };
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

    #[test]
    fn greedy_proposes_accepts_confirms_and_builds() {
        use isle_core::topology::{BASE, NONE};
        use isle_core::{Phase, Resource, ResourceSet};

        let mut g = GameState::new(GameConfig::new(3), 17).unwrap();
        g.setup_step = 6;
        g.phase = Phase::Main;
        g.current = 0;
        g.turn = 3;
        g.dice = Some((2, 4));

        // Player 0 has a settlement at v0 and two roads reaching open spot v2.
        let v0 = 0u8;
        let e0 = BASE.vertex_edges[0][0];
        let v1 = BASE.vertex_vertices[0][0];
        let k1 = BASE.vertex_edges[v1 as usize]
            .iter()
            .position(|&e| e != NONE && e != e0)
            .unwrap();
        let e1 = BASE.vertex_edges[v1 as usize][k1];
        let v2 = BASE.vertex_vertices[v1 as usize][k1];

        g.settlements[0].insert(v0);
        g.players[0].settlements_left -= 1;
        g.roads[0].insert(e0);
        g.roads[0].insert(e1);
        g.players[0].roads_left -= 2;
        g.players[0].longest_road = g.compute_longest_road(0);
        assert!(g.settlement_spots(0, true).contains(v2));

        // Player 1 and Player 2 each have a settlement (targeting a City: 2 Grain + 3 Ore).
        let p1_v = (0..BASE.num_vertices as u8)
            .find(|&v| g.settlement_spots(1, false).contains(v) && v != v2)
            .unwrap();
        g.settlements[1].insert(p1_v);
        g.players[1].settlements_left -= 1;

        let p2_v = (0..BASE.num_vertices as u8)
            .find(|&v| g.settlement_spots(2, false).contains(v) && v != v2)
            .unwrap();
        g.settlements[2].insert(p2_v);
        g.players[2].settlements_left -= 1;
        // Give Player 2 an extra city so Player 1 has strictly lower public VP than Player 2.
        let p2_c = (0..BASE.num_vertices as u8)
            .find(|&v| g.settlement_spots(2, false).contains(v) && v != v2)
            .unwrap();
        g.cities[2].insert(p2_c);
        g.players[2].cities_left -= 1;

        // Player 0 is 1 Grain short of a Settlement, with 1 surplus Ore.
        let h0 = ResourceSet::new(1, 1, 1, 0, 1);
        // Players 1 and 2 are both 1 Ore short of a City, with 1 surplus Grain!
        let h_partner = ResourceSet::new(0, 0, 0, 3, 2);
        g.bank -= h0 + h_partner + h_partner;
        g.players[0].hand = h0;
        g.players[1].hand = h_partner;
        g.players[2].hand = h_partner;
        g.check_invariants().unwrap();

        let mut b0 = GreedyBot::new(1);
        b0.set_propose_trades(true);
        let mut b1 = GreedyBot::new(2);
        let mut b2 = GreedyBot::new(3);
        let mut legal = Vec::new();

        // 1. Player 0 proposes 1 Ore for 1 Grain.
        g.legal_actions(0, &mut legal);
        let offer = b0.choose(&g, 0, &legal);
        assert_eq!(
            offer,
            Action::ProposeTrade {
                give: ResourceSet::single(Resource::Ore, 1),
                get: ResourceSet::single(Resource::Grain, 1),
            }
        );
        g.apply(0, offer).unwrap();

        // 2. Both Player 1 and Player 2 find the trade mutually beneficial and accept.
        g.legal_actions(1, &mut legal);
        let r1 = b1.choose(&g, 1, &legal);
        assert_eq!(r1, Action::RespondTrade { accept: true });
        g.apply(1, r1).unwrap();

        g.legal_actions(2, &mut legal);
        let r2 = b2.choose(&g, 2, &legal);
        assert_eq!(r2, Action::RespondTrade { accept: true });
        g.apply(2, r2).unwrap();

        // 3. Player 0 confirms with Player 1 (lowest public VP: 1 vs 3).
        g.legal_actions(0, &mut legal);
        let confirm = b0.choose(&g, 0, &legal);
        assert_eq!(confirm, Action::ConfirmTrade { partner: 1 });
        g.apply(0, confirm).unwrap();

        // 4. Player 0 immediately builds the settlement!
        g.legal_actions(0, &mut legal);
        let build = b0.choose(&g, 0, &legal);
        assert_eq!(build, Action::BuildSettlement(v2));
        g.apply(0, build).unwrap();
        assert!(g.settlements[0].contains(v2));
        assert_eq!(g.players[1].hand, isle_core::resources::cost::CITY);
        g.check_invariants().unwrap();

        // Rejected offers are cancelled and not re-offered on the same turn.
        let mut h = g;
        h.turn = 4;
        // Player 0 now targets a City (2 Grain + 3 Ore): 1 Ore short with 1 surplus Brick.
        let deficit = ResourceSet::new(1, 0, 0, 2, 2);
        h.bank -= deficit;
        h.players[0].hand += deficit;
        h.legal_actions(0, &mut legal);
        let offer2 = b0.choose(&h, 0, &legal);
        assert!(matches!(offer2, Action::ProposeTrade { .. }));
        h.apply(0, offer2).unwrap();
        h.apply(1, Action::RespondTrade { accept: false }).unwrap();
        h.apply(2, Action::RespondTrade { accept: false }).unwrap();
        h.legal_actions(0, &mut legal);
        assert_eq!(b0.choose(&h, 0, &legal), Action::CancelTrade);
        h.apply(0, Action::CancelTrade).unwrap();
        h.legal_actions(0, &mut legal);
        assert!(!matches!(b0.choose(&h, 0, &legal), Action::ProposeTrade { .. }));
    }

    #[test]
    fn greedy_citadels_crowns_best_metropolis_and_defends_cities() {
        use isle_core::resources::cost;
        use isle_core::topology::vertices_in;
        use isle_core::{CityTrack, Commodity, CommoditySet, Phase, Scenario};

        let mut g = GameState::new(GameConfig::scenario(Scenario::Citadels, 3), 23).unwrap();
        g.setup_step = 6;
        g.phase = Phase::Main;
        g.dice = Some((2, 4));

        // Give Player 0 two cities with distinct pip totals; Metropolis Lv4 should crown the richer one.
        let spots: Vec<u8> = vertices_in(g.setup_spots()).collect();
        let low_v = *spots.iter().min_by_key(|&&v| g.board.vertex_pips(v)).unwrap();
        let high_v = *spots
            .iter()
            .filter(|&&v| {
                v != low_v && !g.topo().vertex_adj_mask[low_v as usize].contains(v)
            })
            .max_by_key(|&&v| g.board.vertex_pips(v))
            .unwrap();
        assert!(g.board.vertex_pips(high_v) > g.board.vertex_pips(low_v));
        for v in [low_v, high_v] {
            g.cities[0].insert(v);
            g.players[0].cities_left -= 1;
        }
        let road_e = g.topo().vertex_edges[low_v as usize][0];
        g.roads[0].insert(road_e);
        g.players[0].roads_left -= 1;
        g.players[0].longest_road = g.compute_longest_road(0);

        g.players[0].city_progress[CityTrack::Trade.index()] = 3;
        g.commodity_bank[Commodity::Cloth] -= 4;
        g.players[0].commodities = CommoditySet::new(0, 4, 0);
        let mut bot = GreedyBot::new(9);
        let mut legal = Vec::new();
        g.legal_actions(0, &mut legal);
        assert_eq!(
            bot.choose(&g, 0, &legal),
            Action::UpgradeCity {
                track: CityTrack::Trade,
                metropolis: Some(high_v),
            }
        );
        g.apply(
            0,
            Action::UpgradeCity {
                track: CityTrack::Trade,
                metropolis: Some(high_v),
            },
        )
        .unwrap();

        // Low city is still vulnerable (`cities_total = 1 > defense_total = 0`): recruit & activate!
        let k_funds = cost::KNIGHT + cost::ACTIVATE_KNIGHT;
        g.bank -= k_funds;
        g.players[0].hand += k_funds;
        g.legal_actions(0, &mut legal);
        let recruit = bot.choose(&g, 0, &legal);
        assert!(matches!(recruit, Action::RecruitKnight(_)));
        g.apply(0, recruit).unwrap();
        g.legal_actions(0, &mut legal);
        let activate = bot.choose(&g, 0, &legal);
        assert!(matches!(activate, Action::ActivateKnight(_)));
        g.apply(0, activate).unwrap();
        assert_eq!(g.active_knight_strength(0), 1);
        g.check_invariants().unwrap();
    }

    #[test]
    fn greedy_and_mcts_play_fishermen_and_pass_old_boot() {
        use isle_core::resources::cost;
        use isle_core::topology::vertices_in;
        use isle_core::{Phase, Resource, ResourceSet, Scenario};

        // Tactical check: GreedyBot passes Old Boot immediately and uses 4 fish for a missing City card.
        let mut g = GameState::new(
            GameConfig {
                event_deck: true,
                ..GameConfig::scenario(Scenario::Fishermen, 3)
            },
            31,
        )
        .unwrap();
        g.setup_step = 6;
        g.phase = Phase::Main;
        g.dice = Some((3, 5));
        let spots: Vec<u8> = vertices_in(g.setup_spots()).collect();
        g.settlements[0].insert(spots[0]);
        g.players[0].settlements_left -= 1;
        let p1_v = *spots
            .iter()
            .find(|&&v| v != spots[0] && !g.topo().vertex_adj_mask[spots[0] as usize].contains(v))
            .unwrap();
        g.cities[1].insert(p1_v);
        g.players[1].cities_left -= 1;

        g.old_boot_owner = Some(0);
        g.players[0].fish = 4;
        let almost_city = ResourceSet::new(0, 0, 0, 2, 2); // 1 Ore short of City
        g.bank -= almost_city;
        g.players[0].hand = almost_city;

        let mut bot = GreedyBot::new(5);
        let mut legal = Vec::new();
        g.legal_actions(0, &mut legal);
        assert_eq!(bot.choose(&g, 0, &legal), Action::PassOldBoot(1));
        g.apply(0, Action::PassOldBoot(1)).unwrap();

        g.legal_actions(0, &mut legal);
        assert_eq!(
            bot.choose(&g, 0, &legal),
            Action::FishTakeResource(Resource::Ore)
        );
        g.apply(0, Action::FishTakeResource(Resource::Ore)).unwrap();
        assert_eq!(g.players[0].hand, cost::CITY);

        g.legal_actions(0, &mut legal);
        assert_eq!(bot.choose(&g, 0, &legal), Action::BuildCity(spots[0]));
        g.apply(0, Action::BuildCity(spots[0])).unwrap();
        g.check_invariants().unwrap();

        // Self-play check: 20 Fishermen + event_deck games finish cleanly with invariants checked.
        let mut finished = 0;
        for seed in 0..20 {
            let players = if seed % 4 == 0 { 5 } else { 4 };
            let config = GameConfig {
                event_deck: seed % 2 == 0,
                paired_turns: players > 4 && seed % 8 == 0,
                ..GameConfig::scenario(Scenario::Fishermen, players)
            };
            let mut h = GameState::new(config, 500 + seed).unwrap();
            let mut b: Vec<GreedyBot> = (0..players as u64)
                .map(|k| GreedyBot::new(seed * 7 + k))
                .collect();
            let mut bots: Vec<&mut dyn Bot> = b.iter_mut().map(|b| b as &mut dyn Bot).collect();
            if play_game(&mut h, &mut bots, 1000, true).winner.is_some() {
                finished += 1;
            }
        }
        assert!(finished >= 18, "only {finished}/20 fishermen greedy games finished");
    }
}
