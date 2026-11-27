//! Tests for the Citadels (Cities & Knights) expansion: second-round city
//! setup, commodities, urban improvement tracks (Lv3 perks & Metropolises),
//! board knights (recruit, activate, promote, move/displace, chase robber),
//! and barbarian raids.

use isle_core::resources::cost;
use isle_core::topology::{BASE, NONE, vertices_in};
use isle_core::{
    Action, BARBARIAN_STEPS, CityTrack, Commodity, CommoditySet, Event, GameConfig, GameState,
    Phase, Resource, ResourceSet, Rng, RuleError, Scenario, Terrain, TradeGood,
};

fn game(n: u8) -> GameState {
    GameState::new(GameConfig::scenario(Scenario::Citadels, n), 19).unwrap()
}

fn main_phase(n: u8) -> GameState {
    let mut g = game(n);
    g.setup_step = 2 * n;
    g.phase = Phase::Main;
    g.dice = Some((2, 4));
    g
}

fn give(g: &mut GameState, p: u8, set: ResourceSet) {
    g.bank -= set;
    g.players[p as usize].hand += set;
}

fn give_comm(g: &mut GameState, p: u8, set: CommoditySet) {
    g.commodity_bank -= set;
    g.players[p as usize].commodities += set;
}

#[test]
fn second_setup_round_places_cities_and_commodities_produce() {
    let mut g = game(3);
    assert_eq!(g.config.victory_points, 13);
    let mut legal = Vec::new();
    while g.is_setup() {
        let p = g.current;
        g.legal_actions(p, &mut legal);
        g.apply(p, legal[0]).unwrap();
    }
    for p in 0..3 {
        assert_eq!(g.settlements[p].count_ones(), 1);
        assert_eq!(g.cities[p].count_ones(), 1);
        assert_eq!(g.public_victory_points(p as u8), 3);
    }
    g.check_invariants().unwrap();

    // Place a city on a Forest tile and verify 1 Lumber + 1 Paper on its roll.
    let mut h = main_phase(2);
    let t = (0..BASE.num_tiles)
        .find(|&t| h.board.terrain[t] == Terrain::Forest && t as u8 != h.robber)
        .unwrap();
    let v = BASE.tile_vertices[t][0];
    h.cities[0].insert(v);
    h.players[0].cities_left -= 1;
    let num = h.board.numbers[t];
    for seed in 0.. {
        let mut r = h;
        r.phase = Phase::PreRoll;
        r.dice = None;
        r.rng = Rng::new(seed);
        r.apply(0, Action::RollDice).unwrap();
        let (a, b) = r.dice.unwrap();
        if a + b == num {
            assert!(r.players[0].hand[Resource::Lumber] >= 1);
            assert!(r.players[0].commodities[Commodity::Paper] >= 1);
            r.check_invariants().unwrap();
            break;
        }
    }
}

#[test]
fn city_tracks_unlock_perks_and_metropolises() {
    let mut g = main_phase(2);
    let v0 = 0u8;
    let v1 = 10u8;
    g.cities[0].insert(v0);
    g.players[0].cities_left -= 1;
    g.cities[1].insert(v1);
    g.players[1].cities_left -= 1;

    // Upgrade Trade from 0 -> 4 (costs 1 + 2 + 3 + 4 = 10 Cloth).
    give_comm(&mut g, 0, CommoditySet::new(0, 10, 0));
    for _ in 1..=3 {
        g.apply(
            0,
            Action::UpgradeCity {
                track: CityTrack::Trade,
                metropolis: None,
            },
        )
        .unwrap();
    }
    // Trade Lv3 unlocks 2:1 commodity trading!
    assert_eq!(g.commodity_trade_ratio(0), 2);
    give_comm(&mut g, 0, CommoditySet::new(2, 0, 0));
    g.apply(
        0,
        Action::MaritimeTradeGood {
            give: TradeGood::Commodity(Commodity::Paper),
            get: TradeGood::Resource(Resource::Ore),
        },
    )
    .unwrap();
    assert_eq!(g.players[0].hand[Resource::Ore], 1);

    // Lv4 claims the Trade Metropolis on `v0` (+2 public VP).
    assert_eq!(
        g.apply(
            0,
            Action::UpgradeCity {
                track: CityTrack::Trade,
                metropolis: None,
            }
        ),
        Err(RuleError::InvalidMetropolis)
    );
    g.apply(
        0,
        Action::UpgradeCity {
            track: CityTrack::Trade,
            metropolis: Some(v0),
        },
    )
    .unwrap();
    assert_eq!(g.metropolis_owners[CityTrack::Trade.index()], Some(0));
    assert_eq!(g.public_victory_points(0), 4);

    // Player 1 reaches Lv5 first and steals the Trade Metropolis onto `v1`!
    g.players[1].city_progress[CityTrack::Trade.index()] = 4;
    give_comm(&mut g, 1, CommoditySet::new(0, 5, 0));
    g.current = 1;
    g.apply(
        1,
        Action::UpgradeCity {
            track: CityTrack::Trade,
            metropolis: Some(v1),
        },
    )
    .unwrap();
    assert_eq!(g.metropolis_owners[CityTrack::Trade.index()], Some(1));
    assert_eq!(g.metropolises[CityTrack::Trade.index()], v1);
    assert_eq!(g.public_victory_points(0), 2);
    assert_eq!(g.public_victory_points(1), 4);
    g.check_invariants().unwrap();
}

#[test]
fn knights_recruit_activate_promote_displace_and_chase_the_robber() {
    let mut g = main_phase(2);
    let v0 = 0u8;
    let e0 = BASE.vertex_edges[0][0];
    let v1 = BASE.vertex_vertices[0][0];
    let e1 = BASE.vertex_edges[v1 as usize]
        .iter()
        .copied()
        .find(|&e| e != NONE && e != e0)
        .unwrap();
    let [a, b] = BASE.edge_vertices[e1 as usize];
    let v2 = if a == v1 { b } else { a };

    g.settlements[0].insert(v0);
    g.players[0].settlements_left -= 1;
    g.roads[0].insert(e0);
    g.roads[0].insert(e1);
    g.players[0].roads_left -= 2;
    g.players[0].longest_road = g.compute_longest_road(0);

    // Opponent knight at `v2` (Lv1).
    g.knight_owner[v2 as usize] = 1;
    g.knight_level[v2 as usize] = 1;
    g.players[1].knights_left[0] -= 1;

    // Recruit, activate and promote our knight at `v1` to Lv2.
    give(&mut g, 0, cost::KNIGHT + cost::ACTIVATE_KNIGHT + cost::KNIGHT);
    g.apply(0, Action::RecruitKnight(v1)).unwrap();
    g.apply(0, Action::ActivateKnight(v1)).unwrap();
    g.apply(0, Action::PromoteKnight(v1)).unwrap();
    assert_eq!(g.knight_level[v1 as usize], 2);
    // Freshly activated this turn: cannot move yet.
    assert_eq!(
        g.apply(0, Action::MoveKnight { from: v1, to: v2 }),
        Err(RuleError::KnightCannotAct)
    );
    // Next turn cycle: clear `knight_fresh` and displace opponent's Lv1 knight!
    g.knight_fresh = isle_core::BitSet::EMPTY;
    g.knight_promoted = isle_core::BitSet::EMPTY;
    let mut events = Vec::new();
    g.apply_with(0, Action::MoveKnight { from: v1, to: v2 }, &mut events)
        .unwrap();
    assert_eq!(g.knight_owner[v2 as usize], 0);
    assert_eq!(g.knight_level[v2 as usize], 2);
    // Opponent's knight had no connected road, so it returned to their supply.
    assert_eq!(g.players[1].knights_left[0], 2);
    g.check_invariants().unwrap();

    // Reactivate, wait a turn, and chase the robber off an adjacent tile.
    let robber_tile = BASE.vertex_tiles[v2 as usize]
        .iter()
        .copied()
        .find(|&t| t != NONE)
        .unwrap();
    g.robber = robber_tile;
    give(&mut g, 0, cost::ACTIVATE_KNIGHT);
    g.apply(0, Action::ActivateKnight(v2)).unwrap();
    g.knight_fresh = isle_core::BitSet::EMPTY;
    let dest = (0..BASE.num_tiles as u8).find(|&t| t != robber_tile).unwrap();
    g.apply(
        0,
        Action::ChaseRobber {
            vertex: v2,
            tile: dest,
            victim: None,
        },
    )
    .unwrap();
    assert_eq!(g.robber, dest);
    assert!(!g.knight_active.contains(v2));
    g.check_invariants().unwrap();
}

#[test]
fn barbarian_raids_award_defenders_or_pillage_cities() {
    // 1. Pillage when defense < cities (Metropolis cities are immune).
    let mut g = main_phase(2);
    g.phase = Phase::PreRoll;
    g.dice = None;
    g.barbarian_pos = BARBARIAN_STEPS - 1;
    let c0 = vertices_in(g.setup_spots()).next().unwrap();
    g.cities[0].insert(c0);
    g.players[0].cities_left -= 1;
    g.metropolises[0] = c0;
    g.metropolis_owners[0] = Some(0); // Player 0's city is a Metropolis (immune!)

    let c1 = vertices_in(g.setup_spots()).next().unwrap();
    g.cities[1].insert(c1);
    g.players[1].cities_left -= 1; // Player 1's city is vulnerable!
    for seed in 0.. {
        let mut h = g;
        h.rng = Rng::new(seed);
        let mut ev = Vec::new();
        h.apply_with(0, Action::RollDice, &mut ev).unwrap();
        if ev.iter().any(|e| matches!(e, Event::BarbarianAdvanced { .. })) {
            assert!(ev.contains(&Event::CityPillaged {
                player: 1,
                vertex: c1
            }));
            assert!(h.settlements[1].contains(c1));
            assert!(h.cities[0].contains(c0));
            h.check_invariants().unwrap();
            break;
        }
    }

    // 2. Sole defender earns +1 Defender VP when defense >= cities!
    let mut g2 = main_phase(2);
    g2.phase = Phase::PreRoll;
    g2.dice = None;
    g2.barbarian_pos = BARBARIAN_STEPS - 1;
    let c = vertices_in(g2.setup_spots()).next().unwrap();
    g2.cities[1].insert(c);
    g2.players[1].cities_left -= 1;
    let kv = vertices_in(g2.setup_spots()).next().unwrap();
    g2.knight_owner[kv as usize] = 0;
    g2.knight_level[kv as usize] = 2;
    g2.players[0].knights_left[1] -= 1;
    g2.knight_active.insert(kv);
    for seed in 0.. {
        let mut h = g2;
        h.rng = Rng::new(seed);
        let mut ev = Vec::new();
        h.apply_with(0, Action::RollDice, &mut ev).unwrap();
        if ev.iter().any(|e| matches!(e, Event::BarbarianAdvanced { .. })) {
            assert_eq!(h.players[0].defender_points, 1);
            assert_eq!(h.public_victory_points(0), 1);
            assert!(!h.knight_active.contains(kv));
            h.check_invariants().unwrap();
            break;
        }
    }
}
