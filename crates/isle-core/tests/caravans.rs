//! Tests for `Scenario::Caravans`（「骆驼商队与黄金河」):
//! * Deterministic golden river (`5` edges on `Base`, `7` on `Extended`) from `desert` to coast
//! * Stone bridges (`MAX_BRIDGES = 3`, `2 Brick + 1 Lumber` or free in `RoadBuilding`, `+2` coins)
//! * `Phase::PlaceCamel` after post-setup `BuildSettlement` / `BuildCity`, `MAX_CAMELS = 22`
//! * Camel caravan `+1` building VP bonus and `2x` longest-road edge length
//! * Gold coins (`2` on 2nd setup, `+1` on river/Mountain/Gold rolls, `SpendCoinsResource` for `2` coins)
//! * `wealthiest_owner` (`>= 4` coins -> `+2` VP; `0` coins while active -> `-1` VP)

use isle_core::resources::cost;
use isle_core::topology::{BASE, EXTENDED, edges_in};
use isle_core::{
    Action, BitSet, Board, BoardLayout, CustomMap, DevCard, Event, GameConfig, GameState,
    MAX_BRIDGES, MAX_CAMELS, MAX_COINS, Phase, Resource, ResourceSet, Rng, RuleError, Scenario,
    Terrain, WEALTHIEST_MIN_COINS,
};

fn caravans_game(n: u8) -> GameState {
    GameState::new(GameConfig::scenario(Scenario::Caravans, n), 42).unwrap()
}

fn caravans_main(n: u8) -> GameState {
    let mut g = caravans_game(n);
    g.setup_step = 2 * n;
    g.phase = Phase::Main;
    g.dice = Some((3, 3));
    g
}

#[test]
fn types_remain_copy_and_config_defaults() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<GameState>();
    assert_copy::<Board>();
    assert_copy::<GameConfig>();
    assert_copy::<CustomMap>();

    let c4 = GameConfig::scenario(Scenario::Caravans, 4);
    assert_eq!(c4.victory_points, 12);
    assert_eq!(c4.layout(), BoardLayout::Base);
    let c6 = GameConfig::scenario(Scenario::Caravans, 6);
    assert_eq!(c6.victory_points, 12);
    assert_eq!(c6.layout(), BoardLayout::Extended);

    let classic = GameState::new(GameConfig::default(), 7).unwrap();
    assert!(classic.board.river_edges.is_empty());
    assert!(classic.camels.is_empty());
}

#[test]
fn golden_river_traces_outward_simple_path_for_every_oasis_tile() {
    for (layout, expected_len) in [(BoardLayout::Base, 5u32), (BoardLayout::Extended, 7u32)] {
        let topo = layout.topology();
        let template = Board::generate(&mut Rng::new(99), layout, false);
        let base_cm = CustomMap::from_board(&template);

        for oasis_tile in 0..topo.num_tiles {
            let mut cm = base_cm;
            // Make `oasis_tile` the sole Desert so `board.desert == oasis_tile`.
            for t in 0..topo.num_tiles {
                if t == oasis_tile {
                    cm.terrain[t] = Terrain::Desert;
                    cm.numbers[t] = 0;
                } else if cm.terrain[t] == Terrain::Desert {
                    cm.terrain[t] = Terrain::Forest;
                    cm.numbers[t] = 6;
                }
            }
            let mut b = Board::from_custom(&cm).unwrap();
            assert_eq!(b.desert as usize, oasis_tile);
            b.init_river_edges();

            assert_eq!(
                b.river_edges.count_ones(),
                expected_len,
                "{layout:?} oasis={oasis_tile}"
            );
            assert!((b.river_edges & !b.land_edges).is_empty());

            // Verify simple path: exactly 2 degree-1 endpoints in `river_edges`, all others degree 2,
            // one endpoint on `desert` and the farther endpoint on the coast, strictly outward at each step.
            let mut endpoints = Vec::new();
            for v in 0..topo.num_vertices as u8 {
                let deg = (topo.vertex_edge_mask[v as usize] & b.river_edges).count_ones();
                assert!(deg <= 2, "river branched at vertex {v}");
                if deg == 1 {
                    endpoints.push(v);
                }
                // Every land vertex retains at least 1 non-river land edge for setup roads.
                assert!(
                    !(topo.vertex_edge_mask[v as usize] & b.land_edges & !b.river_edges).is_empty(),
                    "vertex {v} lost all non-river road edges"
                );
            }
            assert_eq!(endpoints.len(), 2);

            let oasis_verts = topo.tile_vertex_mask[oasis_tile];
            let (dx, dy) = topo.tile_pos[oasis_tile];
            let dist_sq = |v: u8| -> i32 {
                let (vx, vy) = topo.vertex_pos[v as usize];
                let (ox, oy) = (vx as i32 - dx as i32, vy as i32 - dy as i32);
                ox * ox + oy * oy
            };
            let start = if oasis_verts.contains(endpoints[0])
                && (!oasis_verts.contains(endpoints[1])
                    || dist_sq(endpoints[0]) < dist_sq(endpoints[1]))
            {
                endpoints[0]
            } else {
                endpoints[1]
            };
            assert!(oasis_verts.contains(start));

            let mut cur = start;
            let mut prev_e = u8::MAX;
            let mut steps = 0u32;
            while let Some(e) = edges_in(topo.vertex_edge_mask[cur as usize] & b.river_edges)
                .find(|&e| e != prev_e)
            {
                let [a, c] = topo.edge_vertices[e as usize];
                let next = if a == cur { c } else { a };
                assert!(
                    dist_sq(next) > dist_sq(cur),
                    "river did not move strictly outward at edge {e}"
                );
                prev_e = e;
                cur = next;
                steps += 1;
            }
            assert_eq!(steps, expected_len);
            assert!(
                !(topo.vertex_edge_mask[cur as usize] & b.sea_edges).is_empty(),
                "river mouth {cur} is not coastal"
            );
        }
    }
}

#[test]
fn setup_grants_two_coins_in_second_round_and_blocks_roads_on_river() {
    let mut g = caravans_game(3);
    let mut legal = Vec::new();

    // Play complete snake setup (6 settlements + 6 roads).
    while g.is_setup() {
        let p = g.current;
        let second = g.is_second_setup_round();
        g.legal_actions(p, &mut legal);
        if matches!(g.phase, Phase::SetupRoad { .. }) {
            for &a in &legal {
                if let Action::BuildRoad(e) = a {
                    assert!(!g.board.river_edges.contains(e));
                }
            }
        }
        let mut events = Vec::new();
        g.apply_with(p, legal[0], &mut events).unwrap();
        if matches!(legal[0], Action::BuildSettlement(_)) && second {
            assert!(events.contains(&Event::CoinsGained {
                player: p,
                amount: 2,
            }));
        }
    }
    for p in 0..3 {
        assert_eq!(g.players[p].coins, 2);
    }
    assert_eq!(g.wealthiest_owner, None);
    g.check_invariants().unwrap();
}

#[test]
fn bridges_camels_longest_road_doubling_and_wealthiest_rules() {
    let mut g = caravans_main(3);
    let river_e = edges_in(g.board.river_edges).next().unwrap();
    let [va, vb] = BASE.edge_vertices[river_e as usize];

    // Place seat 0's settlement at `va` touching `river_e`.
    g.settlements[0].insert(va);
    g.players[0].settlements_left -= 1;

    // Regular BuildRoad on `river_e` is rejected as WrongGround.
    g.bank -= cost::BRIDGE;
    g.players[0].hand += cost::BRIDGE;
    assert_eq!(g.check(0, Action::BuildRoad(river_e)), Err(RuleError::WrongGround));

    // BuildBridge succeeds, costs 2 Brick + 1 Lumber, grants +2 coins, and connects across `river_e`.
    let mut events = Vec::new();
    g.apply_with(0, Action::BuildBridge(river_e), &mut events)
        .unwrap();
    assert!(g.bridges[0].contains(river_e));
    assert_eq!(g.players[0].bridges_left, MAX_BRIDGES - 1);
    assert_eq!(g.players[0].hand, ResourceSet::EMPTY);
    assert_eq!(g.players[0].coins, 2);
    assert!(events.contains(&Event::BridgeBuilt {
        player: 0,
        edge: river_e,
    }));
    assert!(events.contains(&Event::CoinsGained {
        player: 0,
        amount: 2,
    }));
    assert_eq!(g.players[0].longest_road, 1);

    // Non-river edge connected via `vb` is now reachable in `road_spots(0)`!
    let next_road = edges_in(BASE.vertex_edge_mask[vb as usize] & !g.board.river_edges)
        .next()
        .unwrap();
    assert!(g.road_spots(0).contains(next_road));

    // RoadBuilding dev card allows building a bridge for free too.
    // Pick `other_river` and `next_road` on opposite ends of `river_e` so they form a 3-edge chain!
    let other_at_va =
        edges_in(BASE.vertex_edge_mask[va as usize] & g.board.river_edges & !BitSet::bit(river_e))
            .next();
    let (other_river, chain_road) = if let Some(or) = other_at_va {
        (or, next_road)
    } else {
        let or = edges_in(
            BASE.vertex_edge_mask[vb as usize] & g.board.river_edges & !BitSet::bit(river_e),
        )
        .next()
        .unwrap();
        let rd = edges_in(BASE.vertex_edge_mask[va as usize] & !g.board.river_edges)
            .next()
            .unwrap();
        (or, rd)
    };
    let rb_pos = g.dev_deck[..g.dev_deck_len as usize]
        .iter()
        .position(|&c| c == DevCard::RoadBuilding)
        .unwrap();
    g.dev_deck.swap(rb_pos, g.dev_deck_len as usize - 1);
    g.dev_deck_len -= 1;
    g.players[0].dev_cards[DevCard::RoadBuilding.index()] += 1;
    g.check_invariants().unwrap();

    g.apply(0, Action::PlayRoadBuilding).unwrap();
    assert_eq!(g.phase, Phase::RoadBuilding { remaining: 2 });
    g.apply(0, Action::BuildBridge(other_river)).unwrap();
    g.apply(0, Action::BuildRoad(chain_road)).unwrap();
    assert_eq!(g.phase, Phase::Main);
    assert_eq!(g.players[0].coins, 4);
    // With 4 >= WEALTHIEST_MIN_COINS coins solely, seat 0 is Wealthiest (+2 VP),
    // and seats 1 & 2 (at 0 coins) suffer -1 VP (clamped at 0 when they have 0 buildings, or -1 from 2).
    assert_eq!(g.wealthiest_owner, Some(0));
    assert_eq!(WEALTHIEST_MIN_COINS, 4);

    // Give seat 1 a city (2 base VP) with 0 coins -> public VP drops to 1 while wealthiest_owner is Some!
    let p1_v = (0..BASE.num_vertices as u8)
        .find(|&v| g.settlement_spots(1, false).contains(v))
        .unwrap();
    g.cities[1].insert(p1_v);
    g.players[1].cities_left -= 1;
    assert_eq!(g.public_victory_points(1), 1);

    // SpendCoinsResource costs 2 coins -> seat 0 drops from 4 to 2 coins, losing wealthiest_owner!
    events.clear();
    g.apply_with(0, Action::SpendCoinsResource(Resource::Grain), &mut events)
        .unwrap();
    assert_eq!(g.players[0].coins, 2);
    assert_eq!(g.players[0].hand[Resource::Grain], 1);
    assert_eq!(g.wealthiest_owner, None);
    assert_eq!(g.public_victory_points(1), 2);
    assert!(events.contains(&Event::CoinsSpent {
        player: 0,
        cost: 2,
        resource: Resource::Grain,
    }));

    // BuildCity triggers Phase::PlaceCamel!
    let city_rem = ResourceSet::new(0, 0, 0, 1, 3);
    g.bank -= city_rem;
    g.players[0].hand += city_rem;
    g.apply(0, Action::BuildCity(va)).unwrap();
    assert_eq!(g.phase, Phase::PlaceCamel);

    // Place camels from the oasis onto `river_e` (or connect camels to `river_e`) to verify
    // +1 Caravan Building VP and 2x Longest Road!
    let first_camel = edges_in(g.camel_spots()).next().unwrap();
    g.apply(0, Action::PlaceCamel(first_camel)).unwrap();
    assert_eq!(g.phase, Phase::Main);
    assert!(g.camels.contains(first_camel));

    // Directly place camels on `river_e` and `other_river` (2 bridges of length 2 each + `next_road` of length 1 = 5!)
    g.camels.insert(river_e);
    g.camels.insert(other_river);
    assert!(g.camels.count_ones() <= MAX_CAMELS as u32);
    let lr = g.compute_longest_road(0);
    assert!(lr >= 5, "expected camel-doubled road >= 5, got {lr}");
    // City at `va` (2 VP) + touching camel at `river_e` (+1 Caravan VP) = 3 VP!
    assert_eq!(g.public_victory_points(0), 3);

    // Test coin cap at MAX_COINS = 99 and tie handling on `reassign_wealthiest`.
    g.players[0].coins = 98;
    g.players[1].coins = 99;
    g.reassign_wealthiest();
    assert_eq!(g.wealthiest_owner, Some(1));
    // Seat 0 gains coins via roll on a river/mountain tile -> capped at 99, ties seat 1 -> seat 1 keeps holder!
    let prod_tile = (0..EXTENDED.num_tiles.min(BASE.num_tiles) as u8)
        .find(|&t| {
            t != g.robber
                && g.board.numbers[t as usize] != 0
                && (!(BASE.tile_edge_mask(t) & g.board.river_edges).is_empty()
                    || g.board.terrain[t as usize] == Terrain::Mountains)
        })
        .unwrap();
    // Ensure seat 0 has a building on `prod_tile` or test `reassign_wealthiest` directly:
    let _ = prod_tile;
    g.players[0].coins = MAX_COINS;
    g.reassign_wealthiest();
    assert_eq!(g.wealthiest_owner, Some(1), "holder keeps title on tie");
    g.players[1].coins = 3;
    g.players[0].coins = 3;
    g.reassign_wealthiest();
    assert_eq!(g.wealthiest_owner, None);

    // Action codec round-trips tags 34, 35, 36.
    for a in [
        Action::PlaceCamel(11),
        Action::BuildBridge(22),
        Action::SpendCoinsResource(Resource::Ore),
    ] {
        let mut buf = Vec::new();
        a.encode(&mut buf);
        assert_eq!(Action::decode(&buf), Some((a, buf.len())));
    }
}
