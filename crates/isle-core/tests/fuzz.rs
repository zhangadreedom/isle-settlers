//! Randomised self-play: plays many games with random (but progress-biased)
//! moves and checks after every step that
//! * conservation laws and structural invariants hold,
//! * `legal_actions` and `check` agree exactly on random candidate actions,
//! * rejected actions leave the state untouched.

use isle_core::{
    Action, CityTrack, Commodity, CommoditySet, Event, GameConfig, GameState, Phase, Resource,
    ResourceSet, Rng, Scenario, TradeGood, TradeResponse,
};

fn random_set(rng: &mut Rng, max_total: u32) -> ResourceSet {
    let mut s = ResourceSet::EMPTY;
    let n = rng.below(max_total + 1);
    for _ in 0..n {
        s.0[rng.below(5) as usize] += 1;
    }
    s
}

fn random_cset(rng: &mut Rng, max_total: u32) -> CommoditySet {
    let mut s = CommoditySet::EMPTY;
    let n = rng.below(max_total + 1);
    for _ in 0..n {
        s.0[rng.below(3) as usize] += 1;
    }
    s
}

fn random_resource(rng: &mut Rng) -> Resource {
    Resource::from_index(rng.below(5) as usize).unwrap()
}

fn random_good(rng: &mut Rng) -> TradeGood {
    TradeGood::from_code(rng.below(8) as u8).unwrap()
}

/// Arbitrary (often illegal) action, excluding open-ended trade proposals.
fn random_action(rng: &mut Rng) -> Action {
    match rng.below(28) {
        17 => Action::BuildShip(rng.below(170) as u8),
        18 => Action::MoveShip {
            from: rng.below(170) as u8,
            to: rng.below(170) as u8,
        },
        19 => Action::ChooseGold(random_set(rng, 3)),
        20 => Action::DiscardCitadel {
            resources: random_set(rng, 6),
            commodities: random_cset(rng, 4),
        },
        21 => Action::MaritimeTradeGood {
            give: random_good(rng),
            get: random_good(rng),
        },
        22 => Action::UpgradeCity {
            track: CityTrack::from_index(rng.below(3) as usize).unwrap(),
            metropolis: if rng.below(2) == 0 {
                None
            } else {
                Some(rng.below(90) as u8)
            },
        },
        23 => Action::RecruitKnight(rng.below(90) as u8),
        24 => Action::ActivateKnight(rng.below(90) as u8),
        25 => Action::PromoteKnight(rng.below(90) as u8),
        26 => Action::MoveKnight {
            from: rng.below(90) as u8,
            to: rng.below(90) as u8,
        },
        27 => Action::ChaseRobber {
            vertex: rng.below(90) as u8,
            tile: rng.below(48) as u8,
            victim: if rng.below(3) == 0 {
                None
            } else {
                Some(rng.below(7) as u8)
            },
        },
        0 => Action::RollDice,
        1 => Action::BuildRoad(rng.below(170) as u8),
        2 => Action::BuildSettlement(rng.below(125) as u8),
        3 => Action::BuildCity(rng.below(125) as u8),
        4 => Action::BuyDevCard,
        5 => Action::PlayKnight,
        6 => Action::PlayRoadBuilding,
        7 => Action::PlayYearOfPlenty(random_set(rng, 3)),
        8 => Action::PlayMonopoly(random_resource(rng)),
        9 => Action::MoveRobber {
            tile: rng.below(48) as u8,
            victim: if rng.below(3) == 0 {
                None
            } else {
                Some(rng.below(7) as u8)
            },
        },
        10 => Action::Discard(random_set(rng, 8)),
        11 => Action::MaritimeTrade {
            give: random_resource(rng),
            get: random_resource(rng),
        },
        12 => Action::RespondTrade {
            accept: rng.below(2) == 0,
        },
        13 => Action::ConfirmTrade {
            partner: rng.below(7) as u8,
        },
        14 => Action::CancelTrade,
        _ => Action::EndTurn,
    }
}

fn weight(a: &Action) -> u32 {
    match a {
        Action::BuildCity(_) | Action::BuildSettlement(_) | Action::UpgradeCity { .. } => 1000,
        Action::BuyDevCard | Action::RecruitKnight(_) | Action::PromoteKnight(_) => 100,
        Action::ActivateKnight(_) => 150,
        Action::BuildRoad(_) | Action::BuildShip(_) => 20,
        Action::MoveShip { .. } | Action::MoveKnight { .. } | Action::ChaseRobber { .. } => 3,
        Action::EndTurn => 5,
        _ => 1,
    }
}

fn pick(rng: &mut Rng, actions: &[Action]) -> Action {
    let total: u32 = actions.iter().map(weight).sum();
    let mut x = rng.below(total);
    for a in actions {
        let w = weight(a);
        if x < w {
            return *a;
        }
        x -= w;
    }
    unreachable!()
}

/// The compact replay encoding decodes back to the same action.
fn assert_round_trip(a: Action) {
    let mut bytes = vec![0xEE];
    a.encode(&mut bytes);
    assert_eq!(
        Action::decode(&bytes[1..]),
        Some((a, bytes.len() - 1)),
        "{a:?}"
    );
    // A truncated encoding is rejected.
    assert_eq!(Action::decode(&bytes[1..bytes.len() - 1]), None, "{a:?}");
}

fn play_one(seed: u64, players: u8, cross_check: bool) -> (GameState, usize) {
    play_scenario(Scenario::Classic, seed, players, cross_check)
}

fn play_scenario(
    scenario: Scenario,
    seed: u64,
    players: u8,
    cross_check: bool,
) -> (GameState, usize) {
    let config = GameConfig::scenario(scenario, players);
    let mut g = GameState::new(config, seed).unwrap();
    let mut rng = Rng::new(seed ^ 0xDEAD_BEEF);
    let mut legal = Vec::new();
    let mut events: Vec<Event> = Vec::new();
    let mut steps = 0;

    while g.phase != Phase::GameOver && g.turn < 600 {
        g.check_invariants()
            .unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}\n{g:#?}"));

        if cross_check {
            for p in 0..players {
                g.legal_actions(p, &mut legal);
                for &a in &legal {
                    assert_eq!(
                        g.check(p, a),
                        Ok(()),
                        "seed {seed}: generated {a:?} for {p} is illegal"
                    );
                }
                for _ in 0..24 {
                    let a = random_action(&mut rng);
                    let ok = g.check(p, a).is_ok();
                    assert_eq!(
                        ok,
                        legal.contains(&a),
                        "seed {seed}: check/legal disagree on {a:?} for {p}"
                    );
                    if !ok {
                        let before = g;
                        assert!(g.apply(p, a).is_err());
                        assert_eq!(before, g, "rejected action mutated state");
                    }
                }
            }
        }

        // Occasionally exercise domestic trading.
        if g.phase == Phase::Main && g.trade.is_none() && rng.below(8) == 0 {
            let give = random_set(&mut rng, 2);
            let get = random_set(&mut rng, 2);
            let a = Action::ProposeTrade { give, get };
            assert_round_trip(a);
            let expected = !give.is_empty()
                && !get.is_empty()
                && !give.overlaps(&get)
                && g.players[g.current as usize].hand.contains(&give);
            assert_eq!(g.apply(g.current, a).is_ok(), expected);
            if expected {
                for q in 0..players {
                    if q != g.current {
                        g.legal_actions(q, &mut legal);
                        let choice = legal[rng.below(legal.len() as u32) as usize];
                        g.apply(q, choice).unwrap();
                    }
                }
                let offer = g.trade.unwrap();
                assert!(offer.responses.iter().all(|r| *r != TradeResponse::Pending));
            }
        }

        let p = g.to_move().unwrap();
        g.legal_actions(p, &mut legal);
        assert!(
            !legal.is_empty(),
            "seed {seed}: no legal action for {p} in {:?}",
            g.phase
        );
        let a = pick(&mut rng, &legal);
        assert_round_trip(a);
        events.clear();
        g.apply_with(p, a, &mut events).unwrap();
        assert!(!events.is_empty(), "every action emits at least one event");
        steps += 1;
    }
    g.check_invariants().unwrap();
    (g, steps)
}

#[test]
fn random_games_with_cross_checking() {
    for seed in 0..50 {
        play_one(seed, 2 + (seed % 5) as u8, true);
    }
}

#[test]
fn random_games_finish_and_are_reproducible() {
    let mut finished = 0;
    for seed in 100..400 {
        let (g, steps) = play_one(seed, 4, false);
        if let Some(w) = g.winner {
            finished += 1;
            assert!(g.victory_points(w) >= g.config.victory_points);
        }
        let (again, steps_again) = play_one(seed, 4, false);
        assert_eq!(g, again);
        assert_eq!(steps, steps_again);
    }
    assert!(finished > 250, "only {finished}/300 random games finished");
}

#[test]
fn five_and_six_player_games_finish() {
    for players in [5, 6] {
        let mut finished = 0;
        for seed in 500..650 {
            let (g, _) = play_one(seed, players, false);
            if let Some(w) = g.winner {
                finished += 1;
                assert!(g.victory_points(w) >= g.config.victory_points);
            }
        }
        assert!(
            finished > 120,
            "only {finished}/150 random {players}-player games finished"
        );
    }
}

#[test]
fn archipelago_games_with_cross_checking() {
    for seed in 0..60 {
        play_scenario(Scenario::Archipelago, seed, 2 + (seed % 3) as u8, true);
    }
}

#[test]
fn archipelago_games_finish_and_use_the_sea() {
    let (mut finished, mut ships, mut islands, mut gold) = (0, 0, 0, 0);
    for seed in 700..900 {
        let (g, _) = play_scenario(Scenario::Archipelago, seed, 3 + (seed % 2) as u8, false);
        if let Some(w) = g.winner {
            finished += 1;
            assert!(g.victory_points(w) >= g.config.victory_points);
        }
        let n = g.num_players();
        ships += (0..n).filter(|&p| !g.ships[p].is_empty()).count();
        islands += (0..n).filter(|&p| g.players[p].islands != 0).count();
        gold += (0..n)
            .filter(|&p| {
                let tiles = g.board.tiles();
                (0..tiles.len()).any(|t| {
                    tiles[t] == isle_core::Terrain::Gold
                        && !(g.buildings_of(p as u8) & g.topo().tile_vertex_mask[t]).is_empty()
                })
            })
            .count();
        let (again, _) = play_scenario(Scenario::Archipelago, seed, 3 + (seed % 2) as u8, false);
        assert_eq!(g, again);
    }
    assert!(
        finished > 150,
        "only {finished}/200 archipelago games finished"
    );
    assert!(
        ships > 300 && islands > 50 && gold > 20,
        "ships {ships} islands {islands} gold {gold}"
    );
}

#[test]
fn fog_games_with_cross_checking() {
    for seed in 0..60 {
        play_scenario(Scenario::FogIslands, seed, 2 + (seed % 3) as u8, true);
    }
}

#[test]
fn fog_games_finish_and_uncover_the_mist() {
    let (mut finished, mut ships, mut revealed, mut islands) = (0, 0, 0, 0);
    for seed in 900..1100 {
        let (g, _) = play_scenario(Scenario::FogIslands, seed, 3 + (seed % 2) as u8, false);
        if let Some(w) = g.winner {
            finished += 1;
            assert!(g.victory_points(w) >= g.config.victory_points);
        }
        let n = g.num_players();
        ships += (0..n).filter(|&p| !g.ships[p].is_empty()).count();
        islands += (0..n).filter(|&p| g.players[p].islands != 0).count();
        revealed += 13 - g.fog_terrain_len as usize;
        let (again, _) = play_scenario(Scenario::FogIslands, seed, 3 + (seed % 2) as u8, false);
        assert_eq!(g, again);
    }
    assert!(finished > 140, "only {finished}/200 fog games finished");
    assert!(
        ships > 300 && revealed > 500 && islands > 30,
        "ships {ships} revealed {revealed} islands {islands}"
    );
}

fn play_config(config: GameConfig, seed: u64, cross_check: bool) -> GameState {
    let players = config.num_players;
    let mut g = GameState::new(config, seed).unwrap();
    let mut rng = Rng::new(seed ^ 0xCAFE_BABE);
    let mut legal = Vec::new();
    let mut events = Vec::new();
    while g.phase != Phase::GameOver && g.turn < 600 {
        g.check_invariants()
            .unwrap_or_else(|e| panic!("seed {seed}: {e}\n{g:#?}"));
        if cross_check {
            for p in 0..players {
                g.legal_actions(p, &mut legal);
                for &a in &legal {
                    assert_eq!(g.check(p, a), Ok(()), "seed {seed}: {a:?}");
                }
                for _ in 0..24 {
                    let a = random_action(&mut rng);
                    assert_eq!(
                        g.check(p, a).is_ok(),
                        legal.contains(&a),
                        "seed {seed}: check/legal disagree on {a:?} for {p} in {:?}",
                        g.phase
                    );
                }
            }
        }
        let p = g.to_move().unwrap();
        g.legal_actions(p, &mut legal);
        let a = pick(&mut rng, &legal);
        assert_round_trip(a);
        events.clear();
        g.apply_with(p, a, &mut events).unwrap();
        assert!(!events.is_empty());
    }
    g.check_invariants().unwrap();
    g
}

#[test]
fn citadels_and_extended_seafaring_and_variants_cross_check() {
    for seed in 0..25 {
        let _ = Commodity::Paper;
        play_config(
            GameConfig {
                friendly_robber: seed % 2 == 0,
                paired_turns: seed % 3 == 0,
                ..GameConfig::scenario(Scenario::Citadels, 2 + (seed % 5) as u8)
            },
            seed + 2000,
            true,
        );
    }
    for seed in 0..15 {
        play_config(
            GameConfig {
                friendly_robber: true,
                paired_turns: seed % 2 == 0,
                ..GameConfig::scenario(Scenario::Archipelago, 5 + (seed % 2) as u8)
            },
            seed + 3000,
            true,
        );
        play_config(
            GameConfig {
                friendly_robber: seed % 2 == 0,
                paired_turns: true,
                ..GameConfig::scenario(Scenario::FogIslands, 5 + (seed % 2) as u8)
            },
            seed + 4000,
            true,
        );
    }
}
