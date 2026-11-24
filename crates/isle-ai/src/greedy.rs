//! A plan-based heuristic opponent.
//!
//! Fair play: the bot only reads public information (board, buildings,
//! card *counts*, played cards, bank) and its own hand. It never looks at
//! other players' hand composition, the development deck or the RNG.

use isle_core::board::PIPS;
use isle_core::resources::cost;
use isle_core::topology::{NONE, vertices_in};
use isle_core::{Action, GameState, Phase, Resource, ResourceSet, Rng, Terrain};

use crate::Bot;
use crate::sea::{best_sea_target, island_bonus_value, move_value, ship_value};

/// What the bot is saving up for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    City,
    Settlement,
    Road,
    /// Seafaring: sail towards a spot out of reach by road.
    Ship,
    DevCard,
}

impl Target {
    fn cost(self) -> ResourceSet {
        match self {
            Target::City => cost::CITY,
            Target::Settlement => cost::SETTLEMENT,
            Target::Road => cost::ROAD,
            Target::Ship => cost::SHIP,
            Target::DevCard => cost::DEV_CARD,
        }
    }
}

/// A gold field's pips count as this much of a regular resource (any).
const GOLD_WEIGHT: f32 = 1.15;

/// Sea target value above which ships take priority over cities.
const SEA_PRIORITY: f32 = 4.0;

/// Relative value of each resource when settling.
const BASE_WEIGHT: [f32; 5] = [1.0, 1.0, 0.85, 1.0, 1.0];

pub struct GreedyBot {
    rng: Rng,
}

impl GreedyBot {
    pub fn new(seed: u64) -> Self {
        GreedyBot {
            rng: Rng::new(seed),
        }
    }
}

impl Bot for GreedyBot {
    fn name(&self) -> &'static str {
        "greedy"
    }

    fn choose(&mut self, s: &GameState, p: u8, legal: &[Action]) -> Action {
        if legal.len() == 1 {
            return legal[0];
        }
        let choice = match s.phase {
            Phase::SetupSettlement => best_by(legal, |a| match a {
                Action::BuildSettlement(v) => Some(spot_value(s, p, *v)),
                _ => None,
            }),
            Phase::SetupRoad { .. } | Phase::RoadBuilding { .. } => best_route(s, p, legal),
            Phase::PreRoll => pre_roll(s, p, legal),
            Phase::Discard => best_by(legal, |a| match a {
                Action::Discard(d) => Some(hand_value(s, p, s.players[p as usize].hand - *d)),
                _ => None,
            }),
            Phase::MoveRobber => best_by(legal, |a| match a {
                Action::MoveRobber { tile, victim } => Some(robber_value(s, p, *tile, *victim)),
                _ => None,
            }),
            Phase::Gold { .. } => best_by(legal, |a| match a {
                Action::ChooseGold(set) => {
                    Some(hand_value(s, p, s.players[p as usize].hand + *set))
                }
                _ => None,
            }),
            Phase::Main if p != s.current => Some(respond_trade(s, p, legal)),
            // Out-of-turn building only offers builds, purchases and passing.
            Phase::Main | Phase::SpecialBuild { .. } => main_turn(s, p, legal),
            Phase::GameOver => None,
        };
        choice.unwrap_or_else(|| {
            // Fallback: anything that ends the turn, otherwise a random move.
            legal
                .iter()
                .copied()
                .find(|a| matches!(a, Action::EndTurn | Action::RollDice))
                .unwrap_or_else(|| legal[self.rng.below(legal.len() as u32) as usize])
        })
    }
}

/// Highest-scoring action among those `score` rates (first wins ties).
fn best_by(legal: &[Action], mut score: impl FnMut(&Action) -> Option<f32>) -> Option<Action> {
    let mut best: Option<(f32, Action)> = None;
    for a in legal {
        if let Some(v) = score(a)
            && best.is_none_or(|(b, _)| v > b)
        {
            best = Some((v, *a));
        }
    }
    best.map(|(_, a)| a)
}

// ---------------------------------------------------------------- evaluation

pub(crate) fn tile_pips(s: &GameState, t: u8) -> f32 {
    PIPS[s.board.numbers[t as usize] as usize] as f32
}

/// Expected production per resource (pips, cities count double).
pub(crate) fn production(s: &GameState, q: u8) -> [f32; 5] {
    let mut out = [0.0; 5];
    for (mask, mult) in [
        (s.settlements[q as usize], 1.0),
        (s.cities[q as usize], 2.0),
    ] {
        for v in vertices_in(mask) {
            for &t in &s.topo().vertex_tiles[v as usize] {
                if t == NONE {
                    break;
                }
                let blocked = if t == s.robber { 0.3 } else { 1.0 };
                match s.board.terrain[t as usize] {
                    // Gold is any resource: spread it.
                    Terrain::Gold => {
                        for x in &mut out {
                            *x += tile_pips(s, t) * mult * blocked * 0.2;
                        }
                    }
                    terrain => {
                        if let Some(r) = terrain.resource() {
                            out[r.index()] += tile_pips(s, t) * mult * blocked;
                        }
                    }
                }
            }
        }
    }
    out
}

/// Board-wide scarcity: resources with fewer pips are worth more.
pub(crate) fn scarcity(s: &GameState) -> [f32; 5] {
    let mut pips = [0.0f32; 5];
    for t in 0..s.topo().num_tiles as u8 {
        if let Some(r) = s.board.terrain[t as usize].resource() {
            pips[r.index()] += tile_pips(s, t);
        }
    }
    let avg = pips.iter().sum::<f32>() / 5.0;
    pips.map(|x| (avg / x.max(1.0)).clamp(0.7, 1.5))
}

/// How good vertex `v` is as a settlement for `p`.
pub(crate) fn spot_value(s: &GameState, p: u8, v: u8) -> f32 {
    let own = production(s, p);
    let scarce = scarcity(s);
    let mut value = 0.0;
    let mut kinds = 0u8;
    let mut gained = [0.0f32; 5];
    for &t in &s.topo().vertex_tiles[v as usize] {
        if t == NONE {
            break;
        }
        if let Some(r) = s.board.terrain[t as usize].resource() {
            let i = r.index();
            let pips = tile_pips(s, t);
            let new_kind = if own[i] == 0.0 && gained[i] == 0.0 {
                1.3
            } else {
                1.0
            };
            value += pips * BASE_WEIGHT[i] * scarce[i] * new_kind;
            if gained[i] == 0.0 {
                kinds += 1;
            }
            gained[i] += pips;
        } else if s.board.terrain[t as usize] == Terrain::Gold {
            value += tile_pips(s, t) * GOLD_WEIGHT;
            kinds += 1;
        }
    }
    value += kinds as f32 * 0.6;
    let harbor = s.board.vertex_harbor[v as usize];
    if harbor != 0 {
        if harbor == 1 << 5 {
            value += 1.2;
        } else {
            let r = harbor.trailing_zeros() as usize;
            value += 0.35 * (own[r] + gained[r]);
        }
    }
    value + island_bonus_value(s, p, v)
}

/// Settlement spots reachable from vertex `from` within two road segments
/// (not passing through opponent buildings), scored and decayed by distance.
fn frontier_value(s: &GameState, p: u8, from: u8, legal_spots: u128) -> f32 {
    let blocked = s.occupied_vertices() & !s.buildings_of(p);
    let mut best = 0.0f32;
    let mut frontier = vec![(from, 0u8)];
    let mut seen = 1u128 << from;
    while let Some((v, d)) = frontier.pop() {
        if legal_spots & (1 << v) != 0 {
            best = best.max(spot_value(s, p, v) * [1.0, 0.75, 0.5][d as usize]);
        }
        if d == 2 || blocked & (1 << v) != 0 {
            continue;
        }
        // Roads stay on land.
        let topo = s.topo();
        for k in 0..3 {
            let n = topo.vertex_vertices[v as usize][k];
            if n != NONE
                && seen & (1 << n) == 0
                && s.board.land_edges & (1 << topo.vertex_edges[v as usize][k]) != 0
            {
                seen |= 1 << n;
                frontier.push((n, d + 1));
            }
        }
    }
    best
}

/// Value of building road `e`: the best settlement spot it leads towards,
/// plus a little for longest-road progress.
fn road_value(s: &GameState, p: u8, e: u8) -> f32 {
    let spots = s.settlement_spots(p, false);
    let [a, b] = s.topo().edge_vertices[e as usize];
    let reach = s.buildings_of(p) | s.topo().endpoints_of(s.roads[p as usize]);
    // Score from the endpoint the road extends towards.
    let far = if reach & (1 << a) != 0 { b } else { a };
    let mut value = frontier_value(s, p, far, spots);
    let mut after = *s;
    after.roads[p as usize] |= 1 << e;
    let gain = after.compute_longest_road(p) as f32 - s.players[p as usize].longest_road as f32;
    value += gain * 0.4;
    value
}

fn best_road(s: &GameState, p: u8, legal: &[Action]) -> Option<Action> {
    best_by(legal, |a| match a {
        Action::BuildRoad(e) => Some(road_value(s, p, *e)),
        _ => None,
    })
}

/// Best road or ship (setup, Road Building).
fn best_route(s: &GameState, p: u8, legal: &[Action]) -> Option<Action> {
    best_by(legal, |a| match a {
        Action::BuildRoad(e) => Some(road_value(s, p, *e)),
        Action::BuildShip(e) => Some(ship_value(s, p, *e)),
        _ => None,
    })
}

fn best_ship(s: &GameState, p: u8, legal: &[Action], min: f32) -> Option<Action> {
    best_by(legal, |a| match a {
        Action::BuildShip(e) => {
            let v = ship_value(s, p, *e);
            (v > min).then_some(v)
        }
        _ => None,
    })
}

/// Best land frontier value of `p`'s road network (0 if none).
fn land_room(s: &GameState, p: u8) -> f32 {
    let spots = s.settlement_spots(p, false);
    let reach = s.buildings_of(p) | s.topo().endpoints_of(s.roads[p as usize]);
    vertices_in(reach)
        .map(|v| frontier_value(s, p, v, spots))
        .fold(0.0, f32::max)
}

pub(crate) fn missing(hand: ResourceSet, need: ResourceSet) -> u32 {
    (0..5)
        .map(|i| need.0[i].saturating_sub(hand.0[i]) as u32)
        .sum()
}

/// Is any settlement spot within two road segments of our network?
fn has_expansion_room(s: &GameState, p: u8) -> bool {
    let spots = s.settlement_spots(p, false);
    let reach = s.buildings_of(p) | s.topo().endpoints_of(s.roads[p as usize]);
    vertices_in(reach).any(|v| frontier_value(s, p, v, spots) > 0.0)
}

fn choose_target(s: &GameState, p: u8, hand: ResourceSet) -> Target {
    let ps = &s.players[p as usize];
    let can_city = ps.cities_left > 0 && s.settlements[p as usize] != 0;
    let can_settle = ps.settlements_left > 0 && s.settlement_spots(p, true) != 0;
    match (can_city, can_settle) {
        (true, true) => {
            if missing(hand, cost::SETTLEMENT) < missing(hand, cost::CITY) {
                Target::Settlement
            } else {
                Target::City
            }
        }
        // Seafaring: an island settlement (with its bonus) beats a city
        // when ships can get there.
        (true, false)
            if s.board.layout.has_ships()
                && ps.settlements_left > 0
                && missing(hand, cost::SHIP) <= missing(hand, cost::CITY)
                && best_sea_target(s, p) > SEA_PRIORITY =>
        {
            Target::Ship
        }
        (true, false) => Target::City,
        (false, true) => Target::Settlement,
        (false, false) if s.board.layout.has_ships() && ps.settlements_left > 0 => {
            // Seafaring: by road where there is room, by sea where it
            // leads somewhere better (new islands score a bonus).
            let land = if ps.roads_left > 0 {
                land_room(s, p)
            } else {
                0.0
            };
            let sea = best_sea_target(s, p);
            if sea > 0.0 && sea > land * 0.8 {
                Target::Ship
            } else if land > 0.0 {
                Target::Road
            } else {
                Target::DevCard
            }
        }
        (false, false)
            if ps.roads_left > 0 && ps.settlements_left > 0 && has_expansion_room(s, p) =>
        {
            Target::Road
        }
        _ => Target::DevCard,
    }
}

/// Usefulness of a hand: progress towards the target first, then breadth.
fn hand_value(s: &GameState, p: u8, hand: ResourceSet) -> f32 {
    let target = choose_target(s, p, hand).cost();
    let covered: u32 = (0..5).map(|i| hand.0[i].min(target.0[i]) as u32).sum();
    let breadth: u32 = hand.0.iter().map(|&c| c.min(2) as u32).sum();
    covered as f32 * 3.0 + breadth as f32 * 0.5 + hand.total() as f32 * 0.1
}

pub(crate) fn robber_value(s: &GameState, p: u8, tile: u8, victim: Option<u8>) -> f32 {
    if s.board.is_sea(tile) {
        return pirate_value(s, p, tile, victim);
    }
    let pips = tile_pips(s, tile);
    let tv = s.topo().tile_vertex_mask[tile as usize];
    let mut value = 0.0;
    for q in 0..s.num_players() as u8 {
        let weight = (s.settlements[q as usize] & tv).count_ones() as f32
            + 2.0 * (s.cities[q as usize] & tv).count_ones() as f32;
        if q == p {
            value -= weight * pips * 4.0;
        } else {
            value += weight * pips * (1.0 + s.public_victory_points(q) as f32 / 4.0);
        }
    }
    if let Some(q) = victim {
        value += 1.0
            + 0.3 * s.players[q as usize].hand.total() as f32
            + 0.5 * s.public_victory_points(q) as f32;
    }
    value
}

/// The pirate blocks ships around its tile and robs their owners.
fn pirate_value(s: &GameState, p: u8, tile: u8, victim: Option<u8>) -> f32 {
    let te = s.topo().tile_edge_mask(tile);
    let mut value = 0.0;
    for q in 0..s.num_players() as u8 {
        let ships = (s.ships[q as usize] & te).count_ones() as f32;
        if q == p {
            value -= ships * 2.0;
        } else {
            value += ships * (0.5 + s.public_victory_points(q) as f32 / 8.0);
        }
    }
    if let Some(q) = victim {
        value += 1.0
            + 0.3 * s.players[q as usize].hand.total() as f32
            + 0.5 * s.public_victory_points(q) as f32;
    }
    value
}

fn robber_hurts_me(s: &GameState, p: u8) -> bool {
    s.buildings_of(p) & s.topo().tile_vertex_mask[s.robber as usize] != 0
        && tile_pips(s, s.robber) >= 3.0
}

fn knight_wins_army(s: &GameState, p: u8) -> bool {
    let k = s.players[p as usize].knights_played() + 1;
    s.largest_army_owner != Some(p)
        && k >= 3
        && (0..s.num_players()).all(|q| q == p as usize || s.players[q].knights_played() < k)
}

// ------------------------------------------------------------------- phases

fn pre_roll(s: &GameState, p: u8, legal: &[Action]) -> Option<Action> {
    let has_knight = legal.contains(&Action::PlayKnight);
    if has_knight && (robber_hurts_me(s, p) || knight_wins_army(s, p)) {
        return Some(Action::PlayKnight);
    }
    Some(Action::RollDice)
}

fn respond_trade(s: &GameState, p: u8, legal: &[Action]) -> Action {
    let reject = Action::RespondTrade { accept: false };
    let Some(offer) = s.trade else { return reject };
    if !legal.contains(&Action::RespondTrade { accept: true }) {
        return reject;
    }
    // Never help a player who is close to winning.
    if s.public_victory_points(s.current) + 2 >= s.config.victory_points {
        return reject;
    }
    let hand = s.players[p as usize].hand;
    let target = choose_target(s, p, hand).cost();
    let after = hand - offer.get + offer.give;
    let (before_missing, after_missing) = (missing(hand, target), missing(after, target));
    let accept = after_missing < before_missing
        || (after_missing == before_missing && offer.give.total() > offer.get.total());
    Action::RespondTrade { accept }
}

fn main_turn(s: &GameState, p: u8, legal: &[Action]) -> Option<Action> {
    let ps = &s.players[p as usize];
    let hand = ps.hand;

    // 1. Cities, then settlements: always worth a point.
    if let Some(a) = best_by(legal, |a| match a {
        Action::BuildCity(v) => Some(vertex_production(s, *v)),
        _ => None,
    }) {
        return Some(a);
    }
    if let Some(a) = best_by(legal, |a| match a {
        Action::BuildSettlement(v) => Some(spot_value(s, p, *v)),
        _ => None,
    }) {
        return Some(a);
    }

    let target = choose_target(s, p, hand);
    let need = target.cost();

    // 2. Development cards.
    if !s.dev_card_played {
        if legal.contains(&Action::PlayKnight) && (robber_hurts_me(s, p) || knight_wins_army(s, p))
        {
            return Some(Action::PlayKnight);
        }
        if legal.contains(&Action::PlayRoadBuilding)
            && matches!(target, Target::Road | Target::Ship)
        {
            return Some(Action::PlayRoadBuilding);
        }
        if let Some(a) = best_by(legal, |a| match a {
            Action::PlayYearOfPlenty(set) => {
                let gain = missing(hand, need) as f32 - missing(hand + *set, need) as f32;
                (gain > 0.0).then_some(gain)
            }
            _ => None,
        }) {
            return Some(a);
        }
        if let Some(a) = best_by(legal, |a| match a {
            Action::PlayMonopoly(r) => {
                let est = monopoly_estimate(s, p, *r);
                let wanted = need[*r] > hand[*r];
                (est >= 4.0 || (wanted && est >= 2.5)).then_some(est)
            }
            _ => None,
        }) {
            return Some(a);
        }
    }

    // 3. Spend on the target when affordable.
    if hand.contains(&need) {
        match target {
            Target::Road => {
                if let Some(a) = best_road(s, p, legal) {
                    return Some(a);
                }
            }
            Target::Ship => {
                if let Some(a) = best_ship(s, p, legal, 0.0) {
                    return Some(a);
                }
            }
            Target::DevCard if legal.contains(&Action::BuyDevCard) => {
                return Some(Action::BuyDevCard);
            }
            _ => {}
        }
    }

    // 4. Trade surplus with the bank for a missing card.
    if missing(hand, need) > 0 {
        let surplus = |r: Resource| hand[r] as i32 - need[r] as i32 - s.trade_ratio(p, r) as i32;
        if let Some(a) = best_by(legal, |a| match a {
            Action::MaritimeTrade { give, get }
                if need[*get] > hand[*get] && surplus(*give) >= 0 =>
            {
                Some(surplus(*give) as f32 - s.trade_ratio(p, *give) as f32 * 0.1)
            }
            _ => None,
        }) {
            return Some(a);
        }
    }

    // 5. Spend spare cards that the target does not need.
    let spare_after = |price: ResourceSet| {
        hand.contains(&price) && missing(hand - price, need) == missing(hand, need)
    };
    if legal.contains(&Action::BuyDevCard) && (spare_after(cost::DEV_CARD) || hand.total() > 7) {
        return Some(Action::BuyDevCard);
    }
    if (spare_after(cost::ROAD) || (hand.total() > 7 && hand.contains(&cost::ROAD)))
        && let Some(a) = best_by(legal, |a| match a {
            Action::BuildRoad(e) => {
                let v = road_value(s, p, *e);
                (v > 0.5).then_some(v)
            }
            _ => None,
        })
    {
        return Some(a);
    }
    if s.board.layout.has_ships() {
        if (spare_after(cost::SHIP) || (hand.total() > 7 && hand.contains(&cost::SHIP)))
            && let Some(a) = best_ship(s, p, legal, 2.0)
        {
            return Some(a);
        }
        // Re-aim an idle ship at something better.
        if let Some(a) = best_by(legal, |a| match a {
            Action::MoveShip { from, to } => {
                let gain = move_value(s, p, *from, *to);
                (gain > 1.0).then_some(gain)
            }
            _ => None,
        }) {
            return Some(a);
        }
    }

    Some(Action::EndTurn)
}

pub(crate) fn vertex_production(s: &GameState, v: u8) -> f32 {
    s.topo().vertex_tiles[v as usize]
        .iter()
        .filter(|&&t| t != NONE)
        .map(|&t| tile_pips(s, t))
        .sum()
}

/// Expected cards from a monopoly on `r`, estimated from opponents' public
/// production and hand sizes (not their actual hands).
fn monopoly_estimate(s: &GameState, p: u8, r: Resource) -> f32 {
    (0..s.num_players() as u8)
        .filter(|&q| q != p)
        .map(|q| {
            let prod = production(s, q);
            let total: f32 = prod.iter().sum();
            if total == 0.0 {
                0.0
            } else {
                s.players[q as usize].hand.total() as f32 * prod[r.index()] / total
            }
        })
        .sum()
}
