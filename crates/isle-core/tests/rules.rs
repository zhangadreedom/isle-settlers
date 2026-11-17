//! Scenario tests for individual rules.

use isle_core::topology::*;
use isle_core::{
    Action, DevCard, Event, GameConfig, GameState, HarborKind, Phase, Resource, ResourceSet, Rng,
    RuleError,
};

// ---------- helpers ----------

fn game(n: u8) -> GameState {
    GameState::new(
        GameConfig {
            num_players: n,
            ..GameConfig::default()
        },
        7,
    )
    .unwrap()
}

/// A game past setup, in the main phase of player 0 with dice already rolled
/// and an empty board, so scenarios can place pieces directly.
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
    g.players[p as usize].harbors |= g.board.vertex_harbor[v as usize];
}

fn put_city(g: &mut GameState, p: u8, v: u8) {
    g.cities[p as usize] |= 1 << v;
    g.players[p as usize].cities_left -= 1;
}

fn put_road(g: &mut GameState, p: u8, e: u8) {
    g.roads[p as usize] |= 1 << e;
    g.players[p as usize].roads_left -= 1;
    g.players[p as usize].longest_road = g.compute_longest_road(p);
}

fn edge_between(a: u8, b: u8) -> u8 {
    let k = VERTEX_VERTICES[a as usize]
        .iter()
        .position(|&n| n == b)
        .expect("adjacent");
    VERTEX_EDGES[a as usize][k]
}

fn put_path(g: &mut GameState, p: u8, path: &[u8]) {
    for w in path.windows(2) {
        put_road(g, p, edge_between(w[0], w[1]));
    }
}

/// Finds a simple path of `len` edges avoiding `forbidden` vertices, such
/// that `pred` accepts it.
fn find_path(len: usize, forbidden: u64, pred: &dyn Fn(&[u8]) -> bool) -> Vec<u8> {
    fn dfs(path: &mut Vec<u8>, len: usize, forbidden: u64, pred: &dyn Fn(&[u8]) -> bool) -> bool {
        if path.len() == len + 1 {
            return pred(path);
        }
        let v = *path.last().unwrap();
        for &n in &VERTEX_VERTICES[v as usize] {
            if n == NONE || forbidden & (1 << n) != 0 || path.contains(&n) {
                continue;
            }
            path.push(n);
            if dfs(path, len, forbidden, pred) {
                return true;
            }
            path.pop();
        }
        false
    }
    for start in 0..NUM_VERTICES as u8 {
        if forbidden & (1 << start) != 0 {
            continue;
        }
        let mut path = vec![start];
        if dfs(&mut path, len, forbidden, pred) {
            return path;
        }
    }
    panic!("no path of length {len}");
}

/// Path vertices plus their neighbours.
fn halo(path: &[u8]) -> u64 {
    path.iter()
        .fold(0, |m, &v| m | 1 << v | VERTEX_ADJ_MASK[v as usize])
}

/// Third neighbour of `path[i]` (not on the path), if the vertex has degree 3.
fn side_neighbor(path: &[u8], i: usize) -> Option<u8> {
    VERTEX_VERTICES[path[i] as usize]
        .iter()
        .copied()
        .find(|&n| n != NONE && !path.contains(&n))
}

/// Rolls the dice so that they sum to `sum` by searching RNG seeds.
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

fn play_setup_first_legal(g: &mut GameState) -> Vec<u8> {
    let mut order = Vec::new();
    let mut legal = Vec::new();
    while g.is_setup() {
        let p = g.current;
        if g.phase == Phase::SetupSettlement {
            order.push(p);
        }
        g.legal_actions(p, &mut legal);
        g.apply(p, legal[0]).unwrap();
    }
    order
}

// ---------- setup ----------

#[test]
fn setup_follows_snake_order_and_pays_second_settlement() {
    let mut g = game(4);
    let mut events = Vec::new();
    let mut legal = Vec::new();
    let mut order = Vec::new();
    while g.is_setup() {
        let p = g.current;
        if g.phase == Phase::SetupSettlement {
            order.push(p);
        }
        g.legal_actions(p, &mut legal);
        g.apply_with(p, legal[0], &mut events).unwrap();
    }
    assert_eq!(order, vec![0, 1, 2, 3, 3, 2, 1, 0]);
    assert_eq!(g.phase, Phase::PreRoll);
    assert_eq!(g.current, 0);
    for p in 0..4u8 {
        assert_eq!(g.settlements[p as usize].count_ones(), 2);
        assert_eq!(g.roads[p as usize].count_ones(), 2);
        // Starting resources equal the producing tiles around the second settlement.
        let starting: Vec<ResourceSet> = events
            .iter()
            .filter_map(|e| match e {
                Event::StartingResources { player, resources } if *player == p => Some(*resources),
                _ => None,
            })
            .collect();
        assert_eq!(starting.len(), 1);
        assert_eq!(g.players[p as usize].hand, starting[0]);
    }
    g.check_invariants().unwrap();
}

#[test]
fn setup_road_must_touch_the_new_settlement() {
    let mut g = game(3);
    g.apply(0, Action::BuildSettlement(0)).unwrap();
    let far = (0..NUM_EDGES as u8)
        .find(|&e| VERTEX_EDGE_MASK[0] & (1 << e) == 0)
        .unwrap();
    assert_eq!(
        g.apply(0, Action::BuildRoad(far)),
        Err(RuleError::NotConnected)
    );
    let near = VERTEX_EDGES[0][0];
    g.apply(0, Action::BuildRoad(near)).unwrap();
    assert_eq!(g.current, 1);
}

#[test]
fn distance_rule() {
    let mut g = game(3);
    g.apply(0, Action::BuildSettlement(10)).unwrap();
    g.apply(0, Action::BuildRoad(VERTEX_EDGES[10][0])).unwrap();
    let neighbor = VERTEX_VERTICES[10][1];
    assert_eq!(
        g.apply(1, Action::BuildSettlement(neighbor)),
        Err(RuleError::TooCloseToBuilding)
    );
    assert_eq!(
        g.apply(1, Action::BuildSettlement(10)),
        Err(RuleError::Occupied)
    );
    assert_eq!(
        g.apply(0, Action::BuildSettlement(30)),
        Err(RuleError::NotYourTurn)
    );
}

// ---------- production ----------

fn producing_tile(g: &GameState) -> usize {
    (0..NUM_TILES)
        .find(|&t| g.board.numbers[t] != 0 && t as u8 != g.robber)
        .unwrap()
}

/// Expected yield for a building at `v` on a roll of `sum`.
fn expected_yield(g: &GameState, v: u8, sum: u8, mult: u8) -> ResourceSet {
    let mut s = ResourceSet::EMPTY;
    for &t in &VERTEX_TILES[v as usize] {
        if t != NONE && g.board.numbers[t as usize] == sum && t != g.robber {
            s[g.board.terrain[t as usize].resource().unwrap()] += mult;
        }
    }
    s
}

#[test]
fn settlements_produce_one_and_cities_two() {
    let mut g = main_phase(3);
    g.phase = Phase::PreRoll;
    g.dice = None;
    let t = producing_tile(&g);
    let (a, b) = (TILE_VERTICES[t][0], TILE_VERTICES[t][2]);
    put_settlement(&mut g, 0, a);
    put_city(&mut g, 1, b);
    let sum = g.board.numbers[t];
    let (ea, eb) = (expected_yield(&g, a, sum, 1), expected_yield(&g, b, sum, 2));
    roll(&mut g, sum);
    assert_eq!(g.players[0].hand, ea);
    assert_eq!(g.players[1].hand, eb);
    assert_eq!(g.phase, Phase::Main);
}

#[test]
fn robber_blocks_production() {
    let mut g = main_phase(3);
    g.phase = Phase::PreRoll;
    g.dice = None;
    let t = producing_tile(&g);
    let v = TILE_VERTICES[t][0];
    put_settlement(&mut g, 0, v);
    g.robber = t as u8;
    let sum = g.board.numbers[t];
    let expected = expected_yield(&g, v, sum, 1);
    roll(&mut g, sum);
    assert_eq!(g.players[0].hand, expected); // other tiles only
    let r = g.board.terrain[t].resource().unwrap();
    let other_same: u8 = VERTEX_TILES[v as usize]
        .iter()
        .filter(|&&x| x != NONE && x as usize != t)
        .filter(|&&x| {
            g.board.numbers[x as usize] == sum && g.board.terrain[x as usize].resource() == Some(r)
        })
        .count() as u8;
    assert_eq!(g.players[0].hand[r], other_same);
}

#[test]
fn bank_shortage_rule() {
    // Two claimants and not enough cards: nobody receives that resource.
    let mut g = main_phase(3);
    g.phase = Phase::PreRoll;
    g.dice = None;
    let t = producing_tile(&g);
    let r = g.board.terrain[t].resource().unwrap();
    let sum = g.board.numbers[t];
    put_settlement(&mut g, 0, TILE_VERTICES[t][0]);
    put_city(&mut g, 1, TILE_VERTICES[t][2]);
    give(&mut g, 2, ResourceSet::single(r, 18)); // bank keeps 1
    let mut h = g;
    roll(&mut h, sum);
    assert_eq!(h.players[0].hand[r], 0);
    assert_eq!(h.players[1].hand[r], 0);
    assert_eq!(h.bank[r], 1);

    // A single claimant takes whatever is left.
    let mut g2 = g;
    g2.settlements[0] = 0;
    g2.players[0].settlements_left += 1;
    let only_tile = VERTEX_TILES[TILE_VERTICES[t][2] as usize]
        .iter()
        .filter(|&&x| x != NONE && g2.board.numbers[x as usize] == sum)
        .count();
    roll(&mut g2, sum);
    if only_tile == 1 {
        assert_eq!(g2.players[1].hand[r], 1);
        assert_eq!(g2.bank[r], 0);
    }
    g2.check_invariants().unwrap();
}

// ---------- the robber ----------

#[test]
fn seven_forces_discards_then_robber() {
    let mut g = main_phase(3);
    g.phase = Phase::PreRoll;
    g.dice = None;
    give(&mut g, 0, ResourceSet::new(3, 2, 2, 1, 1)); // 9 cards -> discard 4
    give(&mut g, 1, ResourceSet::new(2, 2, 1, 1, 1)); // 7 cards -> safe
    give(&mut g, 2, ResourceSet::new(3, 3, 2, 2, 0)); // 10 cards -> discard 5
    let t = producing_tile(&g);
    put_settlement(&mut g, 1, TILE_VERTICES[t][0]);
    roll(&mut g, 7);
    assert_eq!(g.phase, Phase::Discard);
    assert_eq!(g.pending_discard, [4, 0, 5, 0]);
    assert_eq!(g.to_move(), Some(0));

    assert_eq!(
        g.apply(1, Action::Discard(ResourceSet::single(Resource::Brick, 1))),
        Err(RuleError::NothingToDiscard)
    );
    assert_eq!(
        g.apply(0, Action::Discard(ResourceSet::new(3, 0, 0, 0, 0))),
        Err(RuleError::WrongDiscardAmount)
    );
    assert_eq!(g.apply(0, Action::EndTurn), Err(RuleError::WrongPhase));
    // Any owing player may go first.
    g.apply(2, Action::Discard(ResourceSet::new(3, 2, 0, 0, 0)))
        .unwrap();
    assert_eq!(g.phase, Phase::Discard);
    g.apply(0, Action::Discard(ResourceSet::new(3, 1, 0, 0, 0)))
        .unwrap();
    assert_eq!(g.phase, Phase::MoveRobber);

    let robber = g.robber;
    assert_eq!(
        g.apply(
            0,
            Action::MoveRobber {
                tile: robber,
                victim: None
            }
        ),
        Err(RuleError::RobberMustMove)
    );
    assert_eq!(
        g.apply(
            0,
            Action::MoveRobber {
                tile: t as u8,
                victim: None
            }
        ),
        Err(RuleError::InvalidVictim)
    );
    assert_eq!(
        g.apply(
            0,
            Action::MoveRobber {
                tile: t as u8,
                victim: Some(2)
            }
        ),
        Err(RuleError::InvalidVictim)
    );
    let before = (g.players[0].hand.total(), g.players[1].hand.total());
    g.apply(
        0,
        Action::MoveRobber {
            tile: t as u8,
            victim: Some(1),
        },
    )
    .unwrap();
    assert_eq!(
        (g.players[0].hand.total(), g.players[1].hand.total()),
        (before.0 + 1, before.1 - 1)
    );
    assert_eq!(g.robber, t as u8);
    assert_eq!(g.phase, Phase::Main);
    g.check_invariants().unwrap();
}

#[test]
fn robber_may_go_to_the_desert_and_skip_stealing_from_empty_hands() {
    let mut g = main_phase(3);
    g.phase = Phase::MoveRobber;
    let t = producing_tile(&g);
    g.robber = t as u8;
    let desert = g.board.desert;
    put_settlement(&mut g, 1, TILE_VERTICES[desert as usize][0]);
    // Player 1 has no cards, so there is nobody to rob.
    assert_eq!(
        g.apply(
            0,
            Action::MoveRobber {
                tile: desert,
                victim: Some(1)
            }
        ),
        Err(RuleError::InvalidVictim)
    );
    g.apply(
        0,
        Action::MoveRobber {
            tile: desert,
            victim: None,
        },
    )
    .unwrap();
}

// ---------- development cards ----------

fn put_on_top_of_deck(g: &mut GameState, card: DevCard) {
    let top = g.dev_deck_len as usize - 1;
    let i = g.dev_deck[..=top].iter().position(|&c| c == card).unwrap();
    g.dev_deck.swap(i, top);
}

#[test]
fn dev_cards_wait_a_turn_and_one_per_turn() {
    let mut g = main_phase(2);
    put_on_top_of_deck(&mut g, DevCard::Knight);
    give(&mut g, 0, ResourceSet::new(0, 0, 2, 2, 2));
    g.apply(0, Action::BuyDevCard).unwrap();
    assert_eq!(g.players[0].new_dev_cards[DevCard::Knight.index()], 1);
    assert_eq!(g.apply(0, Action::PlayKnight), Err(RuleError::NoSuchCard));
    put_on_top_of_deck(&mut g, DevCard::Knight);
    g.apply(0, Action::BuyDevCard).unwrap();
    g.apply(0, Action::EndTurn).unwrap();
    g.apply(1, Action::RollDice).ok();
    // Get player 1 to their main phase whatever the roll, then end the turn.
    while g.current == 1 {
        let mut legal = Vec::new();
        let p = g.to_move().unwrap();
        g.legal_actions(p, &mut legal);
        let a = *legal
            .iter()
            .find(|a| matches!(a, Action::EndTurn))
            .unwrap_or(&legal[0]);
        g.apply(p, a).unwrap();
    }
    // Knight before rolling, then back to the pre-roll phase.
    assert_eq!(g.phase, Phase::PreRoll);
    g.apply(0, Action::PlayKnight).unwrap();
    assert_eq!(g.phase, Phase::MoveRobber);
    let tile = (g.robber + 1) % NUM_TILES as u8;
    let victim = (g.robber_victims(tile, 0) != 0).then_some(1);
    g.apply(0, Action::MoveRobber { tile, victim }).unwrap();
    assert_eq!(g.phase, Phase::PreRoll);
    assert_eq!(
        g.apply(0, Action::PlayKnight),
        Err(RuleError::DevCardAlreadyPlayed)
    );
    assert_eq!(g.players[0].knights_played(), 1);
}

#[test]
fn victory_point_cards_count_immediately() {
    let mut g = main_phase(2);
    put_on_top_of_deck(&mut g, DevCard::VictoryPoint);
    give(&mut g, 0, ResourceSet::new(0, 0, 1, 1, 1));
    let before = g.victory_points(0);
    g.apply(0, Action::BuyDevCard).unwrap();
    assert_eq!(g.victory_points(0), before + 1);
    assert_eq!(g.public_victory_points(0), before);
}

#[test]
fn largest_army_needs_three_and_must_be_exceeded() {
    let mut g = main_phase(2);
    g.players[0].played_dev_cards[DevCard::Knight.index()] = 2;
    g.players[0].dev_cards[DevCard::Knight.index()] = 1;
    let mut events = Vec::new();
    g.apply_with(0, Action::PlayKnight, &mut events).unwrap();
    assert!(events.contains(&Event::LargestArmyChanged { owner: Some(0) }));
    assert_eq!(g.largest_army_owner, Some(0));

    let mut h = g;
    h.largest_army_owner = Some(0);
    h.current = 1;
    h.phase = Phase::Main;
    h.dev_card_played = false;
    h.players[1].played_dev_cards[DevCard::Knight.index()] = 2;
    h.players[1].dev_cards[DevCard::Knight.index()] = 2;
    h.apply(1, Action::PlayKnight).unwrap(); // 3 vs 3: holder keeps
    assert_eq!(h.largest_army_owner, Some(0));
    let tile = (h.robber + 1) % NUM_TILES as u8;
    h.apply(1, Action::MoveRobber { tile, victim: None })
        .unwrap();
    h.dev_card_played = false;
    h.apply(1, Action::PlayKnight).unwrap(); // 4 vs 3: taken
    assert_eq!(h.largest_army_owner, Some(1));
}

#[test]
fn year_of_plenty_and_monopoly() {
    let mut g = main_phase(3);
    g.players[0].dev_cards[DevCard::YearOfPlenty.index()] = 1;
    assert_eq!(
        g.apply(
            0,
            Action::PlayYearOfPlenty(ResourceSet::single(Resource::Ore, 1))
        ),
        Err(RuleError::InvalidTrade)
    );
    g.apply(0, Action::PlayYearOfPlenty(ResourceSet::new(0, 0, 0, 1, 1)))
        .unwrap();
    assert_eq!(g.players[0].hand, ResourceSet::new(0, 0, 0, 1, 1));

    let mut g = main_phase(3);
    g.players[0].dev_cards[DevCard::Monopoly.index()] = 1;
    give(&mut g, 1, ResourceSet::new(0, 0, 3, 1, 0));
    give(&mut g, 2, ResourceSet::new(0, 0, 2, 0, 0));
    g.apply(0, Action::PlayMonopoly(Resource::Wool)).unwrap();
    assert_eq!(g.players[0].hand[Resource::Wool], 5);
    assert_eq!(g.players[1].hand, ResourceSet::new(0, 0, 0, 1, 0));
    assert_eq!(g.players[2].hand, ResourceSet::EMPTY);
}

#[test]
fn road_building_places_two_free_roads() {
    let mut g = main_phase(2);
    put_settlement(&mut g, 0, 20);
    g.players[0].dev_cards[DevCard::RoadBuilding.index()] = 1;
    g.apply(0, Action::PlayRoadBuilding).unwrap();
    assert_eq!(g.phase, Phase::RoadBuilding { remaining: 2 });
    let e1 = VERTEX_EDGES[20][0];
    g.apply(0, Action::BuildRoad(e1)).unwrap();
    assert_eq!(g.phase, Phase::RoadBuilding { remaining: 1 });
    assert_eq!(g.apply(0, Action::EndTurn), Err(RuleError::WrongPhase));
    let e2 = edges_in(g.road_spots(0)).next().unwrap();
    g.apply(0, Action::BuildRoad(e2)).unwrap();
    assert_eq!(g.phase, Phase::Main);
    assert_eq!(g.players[0].hand, ResourceSet::EMPTY);
    assert_eq!(g.players[0].roads_left, 13);
}

// ---------- roads ----------

#[test]
fn roads_cannot_continue_through_an_opponent_settlement() {
    let mut g = main_phase(2);
    let path = find_path(1, 0, &|p| side_neighbor(p, 1).is_some());
    put_settlement(&mut g, 0, path[0]);
    put_path(&mut g, 0, &path);
    put_settlement(&mut g, 1, path[1]);
    give(&mut g, 0, ResourceSet::new(1, 1, 0, 0, 0));
    let beyond = edge_between(path[1], side_neighbor(&path, 1).unwrap());
    assert_eq!(
        g.apply(0, Action::BuildRoad(beyond)),
        Err(RuleError::NotConnected)
    );
}

#[test]
fn longest_road_needs_five_and_must_be_exceeded() {
    let mut g = main_phase(3);
    let a = find_path(5, 0, &|_| true);
    put_settlement(&mut g, 0, a[0]);
    put_path(&mut g, 0, &a[..5]); // 4 roads
    give(&mut g, 0, ResourceSet::new(1, 1, 0, 0, 0));
    let mut events = Vec::new();
    g.apply_with(0, Action::BuildRoad(edge_between(a[4], a[5])), &mut events)
        .unwrap();
    assert!(events.contains(&Event::LongestRoadChanged { owner: Some(0) }));
    assert_eq!(g.players[0].longest_road, 5);

    let b = find_path(6, halo(&a), &|_| true);
    g.current = 1;
    put_settlement(&mut g, 1, b[0]);
    put_path(&mut g, 1, &b[..6]); // 5 roads: tie, holder keeps
    give(&mut g, 1, ResourceSet::new(1, 1, 0, 0, 0));
    assert_eq!(g.players[1].longest_road, 5);
    assert_eq!(g.longest_road_owner, Some(0));
    g.apply(1, Action::BuildRoad(edge_between(b[5], b[6])))
        .unwrap(); // 6 > 5
    assert_eq!(g.longest_road_owner, Some(1));
    g.check_invariants().unwrap_or_else(|e| panic!("{e}"));
}

/// Builds: player 0 owns a long road `a` (holder), player 2 a road `c` of
/// `c_len`, and player 1 is about to settle on `a[cut]`.
fn break_scenario(a_len: usize, cut: usize, c_len: usize, d_len: usize) -> GameState {
    let mut g = main_phase(4);
    let a = find_path(a_len, 0, &|p| side_neighbor(p, cut).is_some());
    put_path(&mut g, 0, &a);
    let side = side_neighbor(&a, cut).unwrap();
    let mut used = halo(&a);
    if c_len > 0 {
        let c = find_path(c_len, used, &|_| true);
        put_path(&mut g, 2, &c);
        used |= halo(&c);
    }
    if d_len > 0 {
        let d = find_path(d_len, used, &|_| true);
        put_path(&mut g, 3, &d);
    }
    g.longest_road_owner = Some(0);
    // Player 1 reaches the cut point with a road from the side.
    put_road(&mut g, 1, edge_between(side, a[cut]));
    g.current = 1;
    give(&mut g, 1, ResourceSet::new(1, 1, 1, 1, 0));
    g.apply(1, Action::BuildSettlement(a[cut])).unwrap();
    g
}

#[test]
fn broken_longest_road_goes_to_the_sole_longest() {
    let g = break_scenario(6, 3, 5, 0);
    assert_eq!(g.players[0].longest_road, 3);
    assert_eq!(g.longest_road_owner, Some(2));
}

#[test]
fn broken_longest_road_is_set_aside_on_a_tie() {
    let g = break_scenario(6, 3, 5, 5);
    assert_eq!(g.longest_road_owner, None);
    let g = break_scenario(6, 3, 0, 0);
    assert_eq!(g.longest_road_owner, None);
}

#[test]
fn holder_keeps_longest_road_while_still_tied() {
    let g = break_scenario(7, 5, 5, 0);
    assert_eq!(g.players[0].longest_road, 5);
    assert_eq!(g.longest_road_owner, Some(0));
}

// ---------- trading ----------

#[test]
fn maritime_trade_ratios() {
    let mut g = main_phase(2);
    give(&mut g, 0, ResourceSet::new(4, 0, 0, 0, 0));
    let trade = Action::MaritimeTrade {
        give: Resource::Brick,
        get: Resource::Ore,
    };
    let mut h = g;
    h.apply(0, trade).unwrap();
    assert_eq!(h.players[0].hand, ResourceSet::new(0, 0, 0, 0, 1));

    g.players[0].harbors |= HarborKind::Generic.bit();
    let mut h = g;
    h.apply(0, trade).unwrap();
    assert_eq!(h.players[0].hand, ResourceSet::new(1, 0, 0, 0, 1));

    g.players[0].harbors |= HarborKind::Special(Resource::Brick).bit();
    let mut h = g;
    h.apply(0, trade).unwrap();
    assert_eq!(h.players[0].hand, ResourceSet::new(2, 0, 0, 0, 1));
    assert_eq!(
        h.apply(
            0,
            Action::MaritimeTrade {
                give: Resource::Ore,
                get: Resource::Ore
            }
        ),
        Err(RuleError::InvalidTrade)
    );
}

#[test]
fn harbor_access_comes_from_settling_on_it() {
    let mut g = game(2);
    let v = EDGE_VERTICES[HARBOR_EDGES[0] as usize][0];
    g.apply(0, Action::BuildSettlement(v)).unwrap();
    assert_eq!(g.players[0].harbors, g.board.harbors[0].bit());
}

#[test]
fn domestic_trade_flow() {
    let mut g = main_phase(3);
    give(&mut g, 0, ResourceSet::new(2, 0, 0, 0, 0));
    give(&mut g, 1, ResourceSet::new(0, 0, 0, 0, 1));
    let give_ = ResourceSet::single(Resource::Brick, 1);
    let get = ResourceSet::single(Resource::Ore, 1);

    assert_eq!(
        g.apply(
            1,
            Action::ProposeTrade {
                give: get,
                get: give_
            }
        ),
        Err(RuleError::NotYourTurn)
    );
    assert_eq!(
        g.apply(
            0,
            Action::ProposeTrade {
                give: give_,
                get: give_
            }
        ),
        Err(RuleError::InvalidTrade)
    );
    assert_eq!(
        g.apply(
            0,
            Action::ProposeTrade {
                give: give_,
                get: ResourceSet::EMPTY
            }
        ),
        Err(RuleError::InvalidTrade)
    );
    g.apply(0, Action::ProposeTrade { give: give_, get })
        .unwrap();
    assert_eq!(
        g.apply(2, Action::RespondTrade { accept: true }),
        Err(RuleError::InsufficientResources)
    );
    g.apply(2, Action::RespondTrade { accept: false }).unwrap();
    g.apply(1, Action::RespondTrade { accept: true }).unwrap();
    assert_eq!(
        g.apply(0, Action::ConfirmTrade { partner: 2 }),
        Err(RuleError::PartnerHasNotAccepted)
    );
    g.apply(0, Action::ConfirmTrade { partner: 1 }).unwrap();
    assert_eq!(g.players[0].hand, ResourceSet::new(1, 0, 0, 0, 1));
    assert_eq!(g.players[1].hand, ResourceSet::new(1, 0, 0, 0, 0));
    assert_eq!(g.trade, None);

    // Doing something else withdraws the offer.
    g.apply(0, Action::ProposeTrade { give: give_, get })
        .unwrap();
    give(&mut g, 0, ResourceSet::new(3, 0, 0, 0, 0));
    g.apply(
        0,
        Action::MaritimeTrade {
            give: Resource::Brick,
            get: Resource::Wool,
        },
    )
    .unwrap();
    assert_eq!(g.trade, None);
    assert_eq!(
        g.apply(1, Action::RespondTrade { accept: true }),
        Err(RuleError::NoOpenTrade)
    );
}

#[test]
fn no_trading_or_building_before_rolling() {
    let mut g = main_phase(2);
    g.phase = Phase::PreRoll;
    g.dice = None;
    give(&mut g, 0, ResourceSet::new(4, 1, 1, 1, 0));
    assert_eq!(
        g.apply(
            0,
            Action::MaritimeTrade {
                give: Resource::Brick,
                get: Resource::Ore
            }
        ),
        Err(RuleError::WrongPhase)
    );
    assert_eq!(g.apply(0, Action::EndTurn), Err(RuleError::WrongPhase));
}

// ---------- winning ----------

#[test]
fn win_immediately_on_own_turn() {
    let mut g = main_phase(2);
    let path = find_path(1, 0, &|_| true);
    put_settlement(&mut g, 0, path[0]);
    put_road(&mut g, 0, edge_between(path[0], path[1]));
    let far = find_path(2, halo(&path), &|_| true);
    put_settlement(&mut g, 0, far[0]);
    put_road(&mut g, 0, edge_between(far[0], far[1]));
    put_road(&mut g, 0, edge_between(far[1], far[2]));
    g.players[0].dev_cards[DevCard::VictoryPoint.index()] = 7; // 2 + 7 = 9
    give(&mut g, 0, ResourceSet::new(1, 1, 1, 1, 0));
    assert_eq!(g.victory_points(0), 9);
    let mut events = Vec::new();
    g.apply_with(0, Action::BuildSettlement(far[2]), &mut events)
        .unwrap();
    assert_eq!(g.phase, Phase::GameOver);
    assert_eq!(g.winner, Some(0));
    assert!(events.contains(&Event::GameWon {
        player: 0,
        points: 10
    }));
    assert_eq!(g.apply(1, Action::RollDice), Err(RuleError::GameOver));
}

#[test]
fn points_reached_off_turn_win_when_the_turn_begins() {
    let mut g = main_phase(2);
    g.players[1].dev_cards[DevCard::VictoryPoint.index()] = 5;
    put_city(&mut g, 1, 0);
    put_city(&mut g, 1, 2);
    put_settlement(&mut g, 1, 4);
    assert_eq!(g.victory_points(1), 10);
    give(&mut g, 0, ResourceSet::new(4, 0, 0, 0, 0));
    g.apply(
        0,
        Action::MaritimeTrade {
            give: Resource::Brick,
            get: Resource::Ore,
        },
    )
    .unwrap();
    assert_eq!(g.winner, None, "cannot win on someone else's turn");
    g.apply(0, Action::EndTurn).unwrap();
    assert_eq!(g.winner, Some(1));
}

// ---------- misc ----------

#[test]
fn games_are_reproducible_from_seed() {
    let mut a = game(4);
    let mut b = game(4);
    assert_eq!(
        play_setup_first_legal(&mut a),
        play_setup_first_legal(&mut b)
    );
    a.apply(0, Action::RollDice).unwrap();
    b.apply(0, Action::RollDice).unwrap();
    assert_eq!(a, b);
}
