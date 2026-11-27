//! Seafaring rules on the archipelago: ships, the pirate, gold fields and
//! island bonuses.

use isle_core::board::GOLD_NUMBERS;
use isle_core::resources::cost;
use isle_core::topology::{
    ARCHIPELAGO, ARCHIPELAGO_MAP, FOG_ISLANDS, FOG_ISLANDS_MAP, NONE, Topology, edges_in,
    vertices_in,
};
use isle_core::{
    Action, Board, BoardLayout, ConfigError, Event, GameConfig, GameState, MAX_SHIPS, Phase,
    Resource, ResourceSet, Rng, RuleError, Scenario, Terrain,
};

const T: &Topology = &ARCHIPELAGO;

// ---------- helpers ----------

fn game(n: u8) -> GameState {
    GameState::new(GameConfig::scenario(Scenario::Archipelago, n), 11).unwrap()
}

/// Past setup, player 0's main phase with the dice rolled and no pieces.
fn main_phase(n: u8) -> GameState {
    let mut g = game(n);
    g.setup_step = 2 * n;
    g.phase = Phase::Main;
    g.dice = Some((2, 3));
    g
}

fn give(g: &mut GameState, p: u8, set: ResourceSet) {
    g.bank -= set;
    g.players[p as usize].hand += set;
}

fn put_settlement(g: &mut GameState, p: u8, v: u8) {
    g.settlements[p as usize] |= 1 << v;
    g.players[p as usize].settlements_left -= 1;
    g.players[p as usize].home_islands |= 1 << g.board.vertex_island[v as usize];
}

fn put_ship(g: &mut GameState, p: u8, e: u8) {
    g.ships[p as usize] |= 1 << e;
    g.players[p as usize].ships_left -= 1;
    g.players[p as usize].longest_road = g.compute_longest_road(p);
}

fn put_road(g: &mut GameState, p: u8, e: u8) {
    g.roads[p as usize] |= 1 << e;
    g.players[p as usize].roads_left -= 1;
    g.players[p as usize].longest_road = g.compute_longest_road(p);
}

fn other_end(e: u8, v: u8) -> u8 {
    let [a, b] = T.edge_vertices[e as usize];
    if a == v { b } else { a }
}

fn edges_at(v: u8) -> impl Iterator<Item = u8> {
    edges_in(T.vertex_edge_mask[v as usize])
}

fn island_of(g: &GameState, v: u8) -> u8 {
    g.board.vertex_island[v as usize]
}

/// Shortest chain of free sea edges (away from the pirate) from `from` to
/// a free spot on `island` where a settlement could stand; returns the
/// edges and the landing vertex.
fn sea_path(g: &GameState, from: u8, island: u8) -> (Vec<u8>, u8) {
    let mut prev = [NONE; 128];
    let mut seen = 1u128 << from;
    let mut queue = vec![from];
    let spots = g.settlement_spots(0, false);
    let mut i = 0;
    while i < queue.len() {
        let v = queue[i];
        i += 1;
        if island_of(g, v) == island && spots & (1 << v) != 0 {
            let mut path = Vec::new();
            let mut w = v;
            while w != from {
                let e = prev[w as usize];
                path.push(e);
                w = other_end(e, w);
            }
            path.reverse();
            return (path, v);
        }
        for e in edges_at(v) {
            let bit = 1u128 << e;
            if g.board.sea_edges & bit == 0 || g.pirate_edges() & bit != 0 {
                continue;
            }
            let u = other_end(e, v);
            if seen & (1 << u) == 0 {
                seen |= 1 << u;
                prev[u as usize] = e;
                queue.push(u);
            }
        }
    }
    panic!("no sea path from {from} to island {island}");
}

/// A home-island vertex on the coast from which the given island can be
/// reached, with the path there.
fn launch_point(g: &GameState, island: u8) -> (u8, Vec<u8>, u8) {
    vertices_in(g.setup_spots())
        .filter(|&v| edges_at(v).any(|e| g.board.sea_edges & (1 << e) != 0))
        .map(|v| {
            let (path, land) = sea_path(g, v, island);
            (v, path, land)
        })
        .min_by_key(|(_, path, _)| path.len())
        .unwrap()
}

fn roll(g: &mut GameState, sum: u8) {
    for seed in 0.. {
        let mut h = *g;
        h.rng = Rng::new(seed);
        h.apply(h.current, Action::RollDice).unwrap();
        let (a, b) = h.dice.unwrap();
        if a + b == sum {
            *g = h;
            return;
        }
    }
}

// ---------- board and setup ----------

#[test]
fn boards_follow_the_map() {
    let mut rng = Rng::new(5);
    for _ in 0..300 {
        let b = Board::generate(&mut rng, BoardLayout::Archipelago, true);
        let mut gold = 0;
        for t in 0..T.num_tiles {
            let ch = ARCHIPELAGO_MAP[t];
            let terrain = b.terrain[t];
            assert_eq!(terrain == Terrain::Sea, ch == b'.' || ch == b'~');
            assert_eq!(b.tile_island[t] == 0, terrain == Terrain::Sea);
            if ch == b'M' {
                assert_eq!(b.tile_island[t], 1);
                assert_ne!(terrain, Terrain::Gold);
            }
            if terrain == Terrain::Gold {
                gold += 1;
                assert!(
                    GOLD_NUMBERS.contains(&b.numbers[t]),
                    "gold on {}",
                    b.numbers[t]
                );
            }
            assert_eq!(b.numbers[t] != 0, terrain.produces());
            for &n in &T.tile_neighbors[t] {
                let red = |x: u8| x == 6 || x == 8;
                assert!(n == NONE || !(red(b.numbers[t]) && red(b.numbers[n as usize])));
            }
        }
        assert_eq!(gold, 2);
        assert_eq!(b.island_count(), 3);
        // One gold field on each small island.
        for island in 2..=3 {
            let golds = (0..T.num_tiles)
                .filter(|&t| b.tile_island[t] == island && b.terrain[t] == Terrain::Gold)
                .count();
            assert_eq!(golds, 1);
        }
        assert_eq!(b.terrain[b.desert as usize], Terrain::Desert);
        assert_eq!(b.tile_island[b.desert as usize], 1);
        assert_eq!(ARCHIPELAGO_MAP[b.pirate_start as usize], b'~');
        assert_eq!(b.land_vertices.count_ones(), 72);
        assert_eq!(b.setup_vertices & !b.land_vertices, 0);
        assert!(vertices_in(b.setup_vertices).all(|v| b.vertex_island[v as usize] == 1));
        assert_eq!(b.vertex_harbor.iter().filter(|&&h| h != 0).count(), 18);
        // Coasts take both roads and ships; inland only roads, open sea
        // only ships.
        assert_ne!(b.land_edges & b.sea_edges, 0);
        assert_eq!(b.land_edges | b.sea_edges, T.all_edges);
    }
}

#[test]
fn the_archipelago_seats_two_to_four() {
    let config = GameConfig::scenario(Scenario::Archipelago, 4);
    assert_eq!(config.victory_points, 12);
    assert_eq!(config.layout(), BoardLayout::Archipelago);
    for n in 2..=4 {
        let g = game(n);
        assert_eq!(g.pirate, g.board.pirate_start);
        assert!(g.players.iter().all(|p| p.ships_left == MAX_SHIPS));
        g.check_invariants().unwrap();
    }
    assert_eq!(
        GameState::new(GameConfig::scenario(Scenario::Archipelago, 5), 1).unwrap_err(),
        ConfigError::PlayerCount
    );
    // Records from before seafaring are classic.
    let classic: GameConfig = GameConfig::new(4);
    assert_eq!(classic.scenario, Scenario::Classic);
    assert_eq!(classic.layout(), BoardLayout::Base);
}

#[test]
fn setup_is_on_the_home_island_with_a_road_or_a_ship() {
    let mut g = game(3);
    let mut legal = Vec::new();
    g.legal_actions(0, &mut legal);
    assert!(legal.iter().all(|a| matches!(a, Action::BuildSettlement(v)
        if g.board.setup_vertices & (1 << v) != 0)));
    let off_home = vertices_in(g.board.land_vertices & !g.board.setup_vertices)
        .next()
        .unwrap();
    assert_eq!(
        g.apply(0, Action::BuildSettlement(off_home)),
        Err(RuleError::WrongGround)
    );
    // A coastal spot offers both.
    let v = vertices_in(g.setup_spots())
        .find(|&v| {
            let m = T.vertex_edge_mask[v as usize];
            m & g.board.sea_edges & !g.pirate_edges() != 0 && m & g.board.land_edges != 0
        })
        .unwrap();
    g.apply(0, Action::BuildSettlement(v)).unwrap();
    g.legal_actions(0, &mut legal);
    assert!(legal.iter().any(|a| matches!(a, Action::BuildShip(_))));
    assert!(legal.iter().any(|a| matches!(a, Action::BuildRoad(_))));
    let ship = legal
        .iter()
        .find_map(|a| match a {
            Action::BuildShip(e) => Some(*e),
            _ => None,
        })
        .unwrap();
    g.apply(0, Action::BuildShip(ship)).unwrap();
    assert_eq!(g.ships[0], 1 << ship);
    assert_eq!(g.players[0].ships_left, MAX_SHIPS - 1);
    assert_eq!(g.players[0].home_islands, 1 << 1);
    assert_eq!(g.current, 1);
    g.check_invariants().unwrap();
}

// ---------- ships ----------

#[test]
fn ships_sail_from_buildings_and_other_ships() {
    let mut g = main_phase(3);
    let (v, path, _) = launch_point(&g, 2);
    put_settlement(&mut g, 0, v);
    give(&mut g, 0, cost::SHIP + cost::SHIP + cost::ROAD);
    let (e1, e2) = (path[0], path[1]);
    assert_ne!(g.ship_spots(0) & (1 << e1), 0);
    // Not yet connected.
    assert_eq!(
        g.apply(0, Action::BuildShip(e2)),
        Err(RuleError::NotConnected)
    );
    g.apply(0, Action::BuildShip(e1)).unwrap();
    assert_eq!(g.players[0].hand, cost::SHIP + cost::ROAD);
    g.apply(0, Action::BuildShip(e2)).unwrap();
    assert_eq!(g.players[0].longest_road, 2);
    // A road cannot start from a ship, and roads stay on land.
    let tip = other_end(
        e2,
        T.edge_vertices[e1 as usize]
            .iter()
            .copied()
            .find(|&x| T.edge_vertices[e2 as usize].contains(&x))
            .unwrap(),
    );
    let from_ship = edges_at(tip).find(|&e| g.board.land_edges & (1 << e) != 0);
    if let Some(e) = from_ship.filter(|e| g.occupied_edges() & (1 << e) == 0) {
        assert_eq!(
            g.apply(0, Action::BuildRoad(e)),
            Err(RuleError::NotConnected)
        );
    }
    let open_sea = edges_in(g.board.sea_edges & !g.board.land_edges)
        .next()
        .unwrap();
    assert_eq!(
        g.apply(0, Action::BuildRoad(open_sea)),
        Err(RuleError::WrongGround)
    );
    let inland = edges_in(g.board.land_edges & !g.board.sea_edges)
        .next()
        .unwrap();
    assert_eq!(
        g.apply(0, Action::BuildShip(inland)),
        Err(RuleError::WrongGround)
    );
    g.check_invariants().unwrap();
}

#[test]
fn ships_stop_at_opponents_and_the_pirate() {
    let mut g = main_phase(3);
    let (v, path, _) = launch_point(&g, 2);
    put_settlement(&mut g, 0, v);
    put_ship(&mut g, 0, path[0]);
    give(&mut g, 0, cost::SHIP);
    // An opponent settles at the ship's open end: no passing through.
    let joint = T.edge_vertices[path[0] as usize]
        .iter()
        .copied()
        .find(|&x| x != v)
        .unwrap();
    if g.board.land_vertices & (1 << joint) != 0 {
        let mut h = g;
        put_settlement(&mut h, 1, joint);
        assert_eq!(
            h.apply(0, Action::BuildShip(path[1])),
            Err(RuleError::NotConnected)
        );
    }
    // The pirate on a tile next to the next edge blocks it.
    let tile = (0..T.num_tiles as u8)
        .find(|&t| g.board.is_sea(t) && T.tile_edge_mask(t) & (1 << path[1]) != 0)
        .unwrap();
    g.pirate = tile;
    assert_eq!(
        g.apply(0, Action::BuildShip(path[1])),
        Err(RuleError::PirateBlocks)
    );
    assert_eq!(g.ship_spots(0) & T.tile_edge_mask(tile), 0);
}

#[test]
fn open_ships_move_once_per_turn() {
    let mut g = main_phase(3);
    let (v, path, _) = launch_point(&g, 2);
    put_settlement(&mut g, 0, v);
    put_ship(&mut g, 0, path[0]);
    put_ship(&mut g, 0, path[1]);
    // Only the end of the line can move.
    assert_eq!(g.movable_ships(0), 1 << path[1]);
    let to = edges_in(g.ship_moves(0, path[1]))
        .find(|&e| e != path[1])
        .unwrap();
    assert_eq!(
        g.apply(0, Action::MoveShip { from: path[0], to }),
        Err(RuleError::ShipCannotMove)
    );
    let mut events = Vec::new();
    g.apply_with(0, Action::MoveShip { from: path[1], to }, &mut events)
        .unwrap();
    assert!(events.contains(&Event::ShipMoved {
        player: 0,
        from: path[1],
        to
    }));
    assert_eq!(g.ships[0], (1 << path[0]) | (1 << to));
    let again = edges_in(g.ship_moves(0, to)).next().unwrap();
    assert_eq!(
        g.apply(
            0,
            Action::MoveShip {
                from: to,
                to: again
            }
        ),
        Err(RuleError::ShipAlreadyMoved)
    );
    // Ships built this turn wait for the next one.
    g.apply(0, Action::EndTurn).unwrap();
    assert!(!g.ship_moved);
    let mut h = main_phase(3);
    put_settlement(&mut h, 0, v);
    give(&mut h, 0, cost::SHIP);
    h.apply(0, Action::BuildShip(path[0])).unwrap();
    assert_eq!(h.movable_ships(0), 0);
    h.check_invariants().unwrap();
}

#[test]
fn ships_between_two_settlements_are_fixed() {
    let mut g = main_phase(3);
    let (v, path, land) = launch_point(&g, 2);
    put_settlement(&mut g, 0, v);
    for &e in &path {
        put_ship(&mut g, 0, e);
    }
    assert_ne!(g.movable_ships(0), 0);
    put_settlement(&mut g, 0, land);
    assert_eq!(g.movable_ships(0), 0, "a closed route cannot move");
}

// ---------- pirate, gold, islands ----------

#[test]
fn the_pirate_robs_ship_owners() {
    let mut g = main_phase(3);
    g.phase = Phase::MoveRobber;
    let (v, path, _) = launch_point(&g, 2);
    put_settlement(&mut g, 1, v);
    put_ship(&mut g, 1, path[0]);
    give(&mut g, 1, ResourceSet::new(0, 0, 0, 3, 0));
    let tile = (0..T.num_tiles as u8)
        .find(|&t| g.board.is_sea(t) && t != g.pirate && T.tile_edge_mask(t) & (1 << path[0]) != 0)
        .unwrap();
    assert_eq!(g.robber_victims(tile, 0), 1 << 1);
    let robber = g.robber;
    let mut events = Vec::new();
    g.apply_with(
        0,
        Action::MoveRobber {
            tile,
            victim: Some(1),
        },
        &mut events,
    )
    .unwrap();
    assert_eq!((g.pirate, g.robber), (tile, robber));
    assert!(events.contains(&Event::PirateMoved { player: 0, tile }));
    assert_eq!(g.players[0].hand.total(), 1);
    // It must move on next time.
    g.phase = Phase::MoveRobber;
    assert_eq!(
        g.apply(0, Action::MoveRobber { tile, victim: None }),
        Err(RuleError::RobberMustMove)
    );
}

#[test]
fn gold_fields_let_players_pick() {
    let mut g = main_phase(3);
    g.phase = Phase::PreRoll;
    g.dice = None;
    let t = (0..T.num_tiles)
        .find(|&t| g.board.terrain[t] == Terrain::Gold)
        .unwrap();
    let (a, b) = (T.tile_vertices[t][0], T.tile_vertices[t][3]);
    put_settlement(&mut g, 1, a);
    g.cities[2] |= 1 << b;
    g.players[2].cities_left -= 1;
    let mut events = Vec::new();
    let number = g.board.numbers[t];
    roll(&mut g, number);
    assert_eq!(g.phase, Phase::Gold { vertex: None });
    assert_eq!(&g.pending_gold[..3], &[0, 1, 2]);
    assert_eq!(g.to_move(), Some(1));
    let mut legal = Vec::new();
    g.legal_actions(2, &mut legal);
    assert_eq!(legal.len(), 15, "two picks from five resources");
    assert_eq!(
        g.apply(2, Action::ChooseGold(ResourceSet::new(1, 0, 0, 0, 0))),
        Err(RuleError::WrongGoldAmount)
    );
    assert_eq!(
        g.apply(0, Action::ChooseGold(ResourceSet::EMPTY)),
        Err(RuleError::NoGoldOwed)
    );
    assert_eq!(g.apply(0, Action::EndTurn), Err(RuleError::WrongPhase));
    let hand2 = g.players[2].hand;
    g.apply_with(
        2,
        Action::ChooseGold(ResourceSet::new(0, 0, 0, 0, 2)),
        &mut events,
    )
    .unwrap();
    assert_eq!(g.players[2].hand, hand2 + ResourceSet::new(0, 0, 0, 0, 2));
    assert_eq!(g.phase, Phase::Gold { vertex: None });
    g.apply(1, Action::ChooseGold(ResourceSet::new(1, 0, 0, 0, 0)))
        .unwrap();
    assert_eq!(g.phase, Phase::Main);
    g.check_invariants().unwrap();

    // A short bank gives what it has.
    let mut h = main_phase(3);
    h.phase = Phase::Gold { vertex: None };
    h.pending_gold[1] = 2;
    let rest = h.bank - ResourceSet::new(0, 0, 0, 1, 0);
    give(&mut h, 2, rest);
    h.legal_actions(1, &mut legal);
    assert_eq!(legal, [Action::ChooseGold(ResourceSet::new(0, 0, 0, 1, 0))]);
}

#[test]
fn a_gold_start_is_picked_before_the_road() {
    let mut g = game(2);
    let t = (0..T.num_tiles)
        .find(|&t| g.board.terrain[t] == Terrain::Gold)
        .unwrap();
    // Setup only allows the home island; pretend gold is there.
    g.board.setup_vertices |= T.tile_vertex_mask[t];
    let v = T.tile_vertices[t][0];
    g.setup_step = 2; // second round, seat 1
    g.current = 1;
    g.apply(1, Action::BuildSettlement(v)).unwrap();
    assert_eq!(g.phase, Phase::Gold { vertex: Some(v) });
    assert!(g.is_setup());
    g.apply(1, Action::ChooseGold(ResourceSet::new(0, 1, 0, 0, 0)))
        .unwrap();
    assert_eq!(g.phase, Phase::SetupRoad { vertex: v });
}

#[test]
fn the_first_settlement_on_a_new_island_scores() {
    let mut g = main_phase(3);
    let island = 2;
    let (v, path, land) = launch_point(&g, island);
    put_settlement(&mut g, 0, v);
    for &e in &path {
        put_ship(&mut g, 0, e);
    }
    give(&mut g, 0, cost::SETTLEMENT);
    let before = g.victory_points(0);
    let mut events = Vec::new();
    g.apply_with(0, Action::BuildSettlement(land), &mut events)
        .unwrap();
    assert_eq!(g.victory_points(0), before + 3);
    assert!(events.contains(&Event::IslandSettled {
        player: 0,
        island,
        points: 2
    }));
    assert_eq!(g.players[0].islands, 1 << island);
    g.check_invariants().unwrap();
    // Another settlement on the same island: no second bonus.
    let mut h = g;
    let second = vertices_in(h.settlement_spots(0, false))
        .find(|&w| island_of(&h, w) == island)
        .unwrap();
    h.ships[0] |= T.vertex_edge_mask[second as usize] & h.board.sea_edges & !h.occupied_edges();
    h.players[0].ships_left = MAX_SHIPS - h.ships[0].count_ones() as u8;
    h.players[0].longest_road = h.compute_longest_road(0);
    give(&mut h, 0, cost::SETTLEMENT);
    if h.check(0, Action::BuildSettlement(second)).is_ok() {
        let vp = h.victory_points(0);
        h.apply(0, Action::BuildSettlement(second)).unwrap();
        assert_eq!(h.victory_points(0), vp + 1);
    }
}

#[test]
fn trade_routes_join_roads_and_ships_at_settlements() {
    let mut g = main_phase(3);
    let (v, path, _) = launch_point(&g, 2);
    // Two ships from v, and two roads inland from v.
    put_ship(&mut g, 0, path[0]);
    put_ship(&mut g, 0, path[1]);
    let free_land = |g: &GameState, e: u8| {
        g.board.land_edges & (1 << e) != 0 && g.occupied_edges() & (1 << e) == 0
    };
    let r1 = edges_at(v).find(|&e| free_land(&g, e)).unwrap();
    let w = other_end(r1, v);
    let r2 = edges_at(w).find(|&e| e != r1 && free_land(&g, e)).unwrap();
    put_road(&mut g, 0, r1);
    put_road(&mut g, 0, r2);
    // Without a settlement at v the route breaks there.
    assert_eq!(g.compute_longest_road(0), 2);
    put_settlement(&mut g, 0, v);
    assert_eq!(g.compute_longest_road(0), 4);
}

#[test]
fn road_building_also_places_ships() {
    let mut g = main_phase(3);
    let (v, path, _) = launch_point(&g, 2);
    put_settlement(&mut g, 0, v);
    g.players[0].dev_cards[isle_core::DevCard::RoadBuilding.index()] = 1;
    g.dev_deck_len -= 1;
    g.apply(0, Action::PlayRoadBuilding).unwrap();
    assert_eq!(g.phase, Phase::RoadBuilding { remaining: 2 });
    g.apply(0, Action::BuildShip(path[0])).unwrap();
    g.apply(0, Action::BuildShip(path[1])).unwrap();
    assert_eq!(g.phase, Phase::Main);
    assert_eq!(g.players[0].hand, ResourceSet::EMPTY);
    assert_eq!(g.ships[0].count_ones(), 2);
}

#[test]
fn seafaring_actions_encode_compactly() {
    for a in [
        Action::BuildShip(116),
        Action::MoveShip { from: 3, to: 99 },
        Action::ChooseGold(ResourceSet::new(1, 0, 2, 0, 0)),
    ] {
        let mut bytes = Vec::new();
        a.encode(&mut bytes);
        assert_eq!(Action::decode(&bytes), Some((a, bytes.len())));
    }
}

#[test]
fn settling_where_road_meets_ship_lengthens_the_route() {
    let mut g = main_phase(3);
    let (v, path, _) = launch_point(&g, 2);
    // A road reaching v from inland, ships leaving v: two separate routes
    // until a settlement joins them.
    let r = edges_at(v)
        .find(|&e| g.board.land_edges & (1 << e) != 0 && e != path[0])
        .unwrap();
    let w = other_end(r, v);
    let r0 = edges_at(w)
        .find(|&e| e != r && g.board.land_edges & (1 << e) != 0)
        .unwrap();
    put_road(&mut g, 0, r0);
    put_road(&mut g, 0, r);
    put_ship(&mut g, 0, path[0]);
    put_ship(&mut g, 0, path[1]);
    assert_eq!(g.players[0].longest_road, 2);
    give(&mut g, 0, cost::SETTLEMENT);
    g.apply(0, Action::BuildSettlement(v)).unwrap();
    assert_eq!(g.players[0].longest_road, 4);
    g.check_invariants().unwrap();
}

// ---------- fog islands ----------

#[test]
fn fog_boards_follow_the_map() {
    const F: &Topology = &FOG_ISLANDS;
    let mut rng = Rng::new(17);
    for _ in 0..200 {
        let g = GameState::new(
            GameConfig::scenario(Scenario::FogIslands, 4),
            rng.next_u64(),
        )
        .unwrap();
        let b = &g.board;
        let mut fog = 0;
        for t in 0..F.num_tiles {
            match FOG_ISLANDS_MAP[t] {
                b'.' | b'~' => {
                    assert_eq!(b.terrain[t], Terrain::Sea);
                    assert_eq!(b.tile_island[t], 0);
                    assert_eq!(b.numbers[t], 0);
                }
                b'?' => {
                    fog += 1;
                    assert_eq!(b.terrain[t], Terrain::Fog);
                    assert_eq!(b.tile_island[t], 0);
                    assert_eq!(b.numbers[t], 0);
                }
                b'M' => {
                    assert!(b.terrain[t].is_land());
                    assert_eq!(b.tile_island[t], 1);
                    assert_eq!(b.numbers[t] != 0, b.terrain[t].produces());
                }
                _ => unreachable!(),
            }
            for &n in &F.tile_neighbors[t] {
                let red = |x: u8| x == 6 || x == 8;
                assert!(n == NONE || !(red(b.numbers[t]) && red(b.numbers[n as usize])));
            }
        }
        assert_eq!(fog, 13);
        assert_eq!(g.fog_terrain_len, 13);
        assert_eq!(g.fog_numbers_len, 9);
        assert!(vertices_in(b.setup_vertices).all(|v| b.vertex_island[v as usize] == 1));
        g.check_invariants().unwrap();
    }
}

#[test]
fn ships_reveal_fog_tiles_and_grant_discovery_rewards() {
    const F: &Topology = &FOG_ISLANDS;
    let mut g = GameState::new(GameConfig::scenario(Scenario::FogIslands, 3), 29).unwrap();
    g.setup_step = 6;
    g.phase = Phase::Main;
    g.dice = Some((3, 3));

    // Find a fog tile for the robber check.
    let any_fog = (0..F.num_tiles as u8)
        .find(|&t| g.board.terrain[t as usize] == Terrain::Fog)
        .unwrap();
    // Robber and pirate cannot move onto unrevealed fog.
    let mut r = g;
    r.phase = Phase::MoveRobber;
    assert_eq!(
        r.apply(
            0,
            Action::MoveRobber {
                tile: any_fog,
                victim: None
            }
        ),
        Err(RuleError::InvalidTile)
    );

    // Pick a vertex of `sea_neighbor` that does not yet touch any fog tile, and an edge from it
    // whose other end touches `fog_tile`.
    let touches_fog = |s: &GameState, v: u8| {
        F.vertex_tiles[v as usize]
            .iter()
            .any(|&t| t != NONE && s.board.terrain[t as usize] == Terrain::Fog)
    };
    let (start_v, explore_edge, fog_tile) = vertices_in(g.board.land_vertices)
        .filter(|&v| !touches_fog(&g, v))
        .find_map(|v| {
            edges_in(F.vertex_edge_mask[v as usize] & g.board.sea_edges & !g.pirate_edges())
                .find_map(|e| {
                    let [a, b] = F.edge_vertices[e as usize];
                    let far = if a == v { b } else { a };
                    // Pick an edge whose tip touches exactly one fog tile.
                    let mut ft: Vec<u8> = F.vertex_tiles[far as usize]
                        .iter()
                        .copied()
                        .filter(|&t| t != NONE && g.board.terrain[t as usize] == Terrain::Fog)
                        .collect();
                    (ft.len() == 1).then(|| (v, e, ft.pop().unwrap()))
                })
        })
        .unwrap();

    // Force the top of the fog stack to Mountains (8) so we test regular land reward + settling.
    let top = g.fog_terrain_len as usize - 1;
    let m_idx = g.fog_terrain[..=top]
        .iter()
        .position(|&t| t == Terrain::Mountains)
        .unwrap();
    g.fog_terrain.swap(top, m_idx);
    let ntop = g.fog_numbers_len as usize - 1;
    let eight_idx = g.fog_numbers[..=ntop].iter().position(|&n| n == 8).unwrap();
    g.fog_numbers.swap(ntop, eight_idx);
    g.check_invariants().unwrap();

    g.settlements[0] |= 1 << start_v;
    g.players[0].settlements_left -= 1;
    g.players[0].home_islands = 1 << 1;
    give(&mut g, 0, cost::SHIP);
    let mut events = Vec::new();
    g.apply_with(0, Action::BuildShip(explore_edge), &mut events)
        .unwrap();
    assert_eq!(g.board.terrain[fog_tile as usize], Terrain::Mountains);
    assert_eq!(g.board.numbers[fog_tile as usize], 8);
    assert_eq!(g.board.tile_island[fog_tile as usize], 2);
    assert_eq!(g.players[0].hand[Resource::Ore], 1);
    assert!(events.contains(&Event::TileRevealed {
        player: 0,
        tile: fog_tile,
        terrain: Terrain::Mountains,
        number: 8,
        reward: Some(Resource::Ore),
    }));
    g.check_invariants().unwrap();

    // Sail one coastal edge along the newly revealed island (respecting the distance rule) and settle it.
    let ship_edge = edges_in(g.ship_spots(0) & F.tile_edge_mask(fog_tile))
        .find(|&e| {
            let [u, w] = F.edge_vertices[e as usize];
            g.settlement_spots(0, false) & ((1 << u) | (1 << w)) != 0
        })
        .unwrap();
    give(&mut g, 0, cost::SHIP + cost::SETTLEMENT);
    g.apply(0, Action::BuildShip(ship_edge)).unwrap();
    let settle_v = vertices_in(g.settlement_spots(0, true) & F.tile_vertex_mask[fog_tile as usize])
        .next()
        .unwrap();
    let vp_before = g.victory_points(0);
    events.clear();
    g.apply_with(0, Action::BuildSettlement(settle_v), &mut events)
        .unwrap();
    assert_eq!(g.victory_points(0), vp_before + 3);
    assert!(events.contains(&Event::IslandSettled {
        player: 0,
        island: 2,
        points: 2,
    }));
    g.check_invariants().unwrap();
}

#[test]
fn discovering_gold_during_road_building_resumes_the_second_ship() {
    const F: &Topology = &FOG_ISLANDS;
    let mut g = GameState::new(GameConfig::scenario(Scenario::FogIslands, 3), 41).unwrap();
    g.setup_step = 6;
    g.phase = Phase::Main;
    g.dice = Some((2, 4));

    let touches_fog = |s: &GameState, v: u8| {
        F.vertex_tiles[v as usize]
            .iter()
            .any(|&t| t != NONE && s.board.terrain[t as usize] == Terrain::Fog)
    };
    let (start_v, explore_edge) = vertices_in(g.board.land_vertices)
        .filter(|&v| !touches_fog(&g, v))
        .find_map(|v| {
            edges_in(F.vertex_edge_mask[v as usize] & g.board.sea_edges & !g.pirate_edges())
                .find(|&e| {
                    let [a, b] = F.edge_vertices[e as usize];
                    let far = if a == v { b } else { a };
                    touches_fog(&g, far)
                })
                .map(|e| (v, e))
        })
        .unwrap();

    // Put Gold on top of the fog stack (and Sea below it in case two fog tiles touch `far`).
    let top = g.fog_terrain_len as usize - 1;
    let g_idx = g.fog_terrain[..=top]
        .iter()
        .position(|&t| t == Terrain::Gold)
        .unwrap();
    g.fog_terrain.swap(top, g_idx);
    let s_idx = g.fog_terrain[..top]
        .iter()
        .position(|&t| t == Terrain::Sea)
        .unwrap();
    g.fog_terrain.swap(top - 1, s_idx);

    g.settlements[0] |= 1 << start_v;
    g.players[0].settlements_left -= 1;
    g.players[0].home_islands = 1 << 1;
    let dlen = g.dev_deck_len as usize;
    let rb_pos = g.dev_deck[..dlen]
        .iter()
        .position(|&c| c == isle_core::DevCard::RoadBuilding)
        .unwrap();
    g.dev_deck.swap(rb_pos, dlen - 1);
    g.dev_deck_len -= 1;
    g.players[0].dev_cards[isle_core::DevCard::RoadBuilding.index()] = 1;
    g.check_invariants().unwrap();

    g.apply(0, Action::PlayRoadBuilding).unwrap();
    assert_eq!(g.phase, Phase::RoadBuilding { remaining: 2 });
    g.apply(0, Action::BuildShip(explore_edge)).unwrap();
    assert_eq!(g.phase, Phase::Gold { vertex: None });
    assert_eq!(g.pending_gold[0], 1);
    g.apply(0, Action::ChooseGold(ResourceSet::single(Resource::Brick, 1)))
        .unwrap();
    assert_eq!(g.players[0].hand[Resource::Brick], 1);
    assert_eq!(g.phase, Phase::RoadBuilding { remaining: 1 });
    g.check_invariants().unwrap();
}
