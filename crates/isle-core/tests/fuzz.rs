//! Randomised self-play: plays many games with random (but progress-biased)
//! moves and checks after every step that
//! * conservation laws and structural invariants hold,
//! * `legal_actions` and `check` agree exactly on random candidate actions,
//! * rejected actions leave the state untouched.

use isle_core::{
    Action, Event, GameConfig, GameState, Phase, Resource, ResourceSet, Rng, TradeResponse,
};

fn random_set(rng: &mut Rng, max_total: u32) -> ResourceSet {
    let mut s = ResourceSet::EMPTY;
    let n = rng.below(max_total + 1);
    for _ in 0..n {
        s.0[rng.below(5) as usize] += 1;
    }
    s
}

fn random_resource(rng: &mut Rng) -> Resource {
    Resource::from_index(rng.below(5) as usize).unwrap()
}

/// Arbitrary (often illegal) action, excluding open-ended trade proposals.
fn random_action(rng: &mut Rng) -> Action {
    match rng.below(17) {
        0 => Action::RollDice,
        1 => Action::BuildRoad(rng.below(115) as u8),
        2 => Action::BuildSettlement(rng.below(85) as u8),
        3 => Action::BuildCity(rng.below(85) as u8),
        4 => Action::BuyDevCard,
        5 => Action::PlayKnight,
        6 => Action::PlayRoadBuilding,
        7 => Action::PlayYearOfPlenty(random_set(rng, 3)),
        8 => Action::PlayMonopoly(random_resource(rng)),
        9 => Action::MoveRobber {
            tile: rng.below(32) as u8,
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
        Action::BuildCity(_) | Action::BuildSettlement(_) => 1000,
        Action::BuyDevCard => 100,
        Action::BuildRoad(_) => 20,
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
    let config = GameConfig {
        num_players: players,
        ..GameConfig::default()
    };
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
