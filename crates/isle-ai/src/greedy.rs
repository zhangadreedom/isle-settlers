//! A plan-based heuristic opponent.
//!
//! Fair play: the bot only reads public information (board, buildings,
//! card *counts*, played cards, bank) and its own hand. It never looks at
//! other players' hand composition, the development deck or the RNG.

use isle_core::board::PIPS;
use isle_core::resources::cost;
use isle_core::topology::{NONE, vertices_in};
use isle_core::{
    Action, BARBARIAN_STEPS, BitSet, CityTrack, Commodity, CommoditySet, GameState, Phase,
    Resource, ResourceSet, Rng, Terrain, TradeGood,
};

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
    /// Citadels: recruit / promote or activate a knight for barbarian defense.
    Knight,
    ActivateKnight,
}

impl Target {
    fn cost(self) -> ResourceSet {
        match self {
            Target::City => cost::CITY,
            Target::Settlement => cost::SETTLEMENT,
            Target::Road => cost::ROAD,
            Target::Ship => cost::SHIP,
            Target::DevCard => cost::DEV_CARD,
            Target::Knight => cost::KNIGHT,
            Target::ActivateKnight => cost::ACTIVATE_KNIGHT,
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
    propose_trades: bool,
    last_offer: Option<(u32, u8)>,
}

impl GreedyBot {
    pub fn new(seed: u64) -> Self {
        GreedyBot {
            rng: Rng::new(seed),
            propose_trades: false,
            last_offer: None,
        }
    }
}

impl Bot for GreedyBot {
    fn name(&self) -> &'static str {
        "greedy"
    }

    fn set_propose_trades(&mut self, enabled: bool) {
        self.propose_trades = enabled;
    }

    fn choose(&mut self, s: &GameState, p: u8, legal: &[Action]) -> Action {
        if legal.len() == 1
            && !(self.propose_trades
                && s.phase == Phase::Main
                && p == s.current
                && s.trade.is_none())
        {
            return legal[0];
        }
        let citadels = s.config.scenario.is_citadels();
        let choice = match s.phase {
            Phase::SetupSettlement => best_by(legal, |a| match a {
                Action::BuildSettlement(v) if citadels => Some(setup_spot_value(s, p, *v)),
                Action::BuildSettlement(v) => Some(spot_value(s, p, *v)),
                _ => None,
            }),
            Phase::SetupRoad { .. } | Phase::RoadBuilding { .. } => best_route(s, p, legal),
            Phase::PreRoll => pre_roll(s, p, legal),
            Phase::Discard if citadels => best_by(legal, |a| match a {
                Action::DiscardCitadel {
                    resources,
                    commodities,
                } => Some(citadel_keep_value(
                    s,
                    p,
                    s.players[p as usize].hand - *resources,
                    s.players[p as usize].commodities - *commodities,
                )),
                _ => None,
            }),
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
            Phase::Main | Phase::SpecialBuild { .. } | Phase::PairedTurn { .. } => {
                self.main_turn(s, p, legal)
            }
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
    let mut val = value + island_bonus_value(s, p, v);
    if s.config.scenario.is_fishermen() {
        val += 0.42 * fish_pips_at_vertex(s, v);
    }
    val
}

/// Fish production weight (pips) at vertex `v` in Fishermen mode.
pub(crate) fn fish_pips_at_vertex(s: &GameState, v: u8) -> f32 {
    let topo = s.topo();
    let mut pips = 0.0f32;
    for fg in s.board.active_fishing_grounds() {
        if topo.edge_vertices[fg.edge as usize].contains(&v) {
            pips += PIPS[fg.number as usize] as f32;
        }
    }
    if s.board.desert != NONE && topo.tile_vertex_mask[s.board.desert as usize].contains(v) {
        pips += if s.robber == s.board.desert && !s.is_setup() {
            1.1
        } else {
            2.0
        };
    }
    pips
}

/// Value of upgrading `v` into a city in Fishermen mode (tile pips + fish pips).
pub(crate) fn fishermen_city_value(s: &GameState, v: u8) -> f32 {
    vertex_production(s, v) + 0.42 * fish_pips_at_vertex(s, v)
}

/// Setup spot value: in Citadels, the 2nd setup round places the player's
/// starting City, where `Forest`, `Pasture`, and `Mountains` produce both
/// a Resource and a Commodity.
pub(crate) fn setup_spot_value(s: &GameState, p: u8, v: u8) -> f32 {
    let mut val = spot_value(s, p, v);
    if s.config.scenario.is_citadels() && s.is_second_setup_round() {
        let mut seen_comm = 0u8;
        for &t in &s.topo().vertex_tiles[v as usize] {
            if t == NONE {
                break;
            }
            if let Some(comm) = s.board.terrain[t as usize].commodity() {
                let pips = tile_pips(s, t);
                let bit = 1u8 << comm.index();
                let distinct_bonus = if seen_comm & bit == 0 { 0.9 } else { 0.0 };
                seen_comm |= bit;
                val += pips * 0.9 + distinct_bonus;
            }
        }
    }
    val
}

/// Value of upgrading `p`'s settlement at `v` into a city in Citadels mode.
pub(crate) fn citadel_city_value(s: &GameState, p: u8, v: u8) -> f32 {
    let mut existing = [0.0f32; 3];
    for cv in vertices_in(s.cities[p as usize]) {
        for &t in &s.topo().vertex_tiles[cv as usize] {
            if t != NONE
                && let Some(c) = s.board.terrain[t as usize].commodity()
            {
                existing[c.index()] += tile_pips(s, t);
            }
        }
    }
    let mut val = vertex_production(s, v);
    for &t in &s.topo().vertex_tiles[v as usize] {
        if t == NONE {
            break;
        }
        if let Some(c) = s.board.terrain[t as usize].commodity() {
            let mult = if existing[c.index()] == 0.0 { 1.1 } else { 0.65 };
            val += tile_pips(s, t) * mult;
        }
    }
    val
}

/// Which `CityTrack` consumes commodity `c`.
#[inline]
pub(crate) const fn track_for_commodity(c: Commodity) -> CityTrack {
    match c {
        Commodity::Paper => CityTrack::Science,
        Commodity::Cloth => CityTrack::Trade,
        Commodity::Coin => CityTrack::Politics,
    }
}

/// Score of an `UpgradeCity` action (preferring Metropolises on the
/// highest-production non-metropolis city, then Lv3 perks).
pub(crate) fn upgrade_city_value(
    s: &GameState,
    p: u8,
    track: CityTrack,
    metropolis: Option<u8>,
) -> f32 {
    let cur = s.players[p as usize].city_progress[track.index()];
    let next = cur + 1;
    if let Some(mv) = metropolis {
        let steal_bonus = if s.metropolis_owners[track.index()].is_some() {
            10.0
        } else {
            0.0
        };
        let comm_pips: f32 = s.topo().vertex_tiles[mv as usize]
            .iter()
            .filter(|&&t| t != NONE && s.board.terrain[t as usize].commodity().is_some())
            .map(|&t| tile_pips(s, t))
            .sum();
        100.0 + steal_bonus + vertex_production(s, mv) + 0.05 * comm_pips
    } else if next == 3 {
        match track {
            CityTrack::Science => 56.0,
            CityTrack::Trade => 53.0,
            CityTrack::Politics => 50.0,
        }
    } else {
        20.0 + next as f32 * 5.0 + [0.2, 0.1, 0.3][track.index()]
    }
}

/// Score of remaining resources + commodities after `DiscardCitadel`.
pub(crate) fn citadel_keep_value(
    s: &GameState,
    p: u8,
    hand: ResourceSet,
    comm: CommoditySet,
) -> f32 {
    let mut val = hand_value(s, p, hand);
    let ps = &s.players[p as usize];
    let has_city = !s.cities[p as usize].is_empty();
    let comm_ratio = s.commodity_trade_ratio(p);
    for track in CityTrack::ALL {
        let lvl = ps.city_progress[track.index()];
        let c = track.commodity();
        let kept = comm[c];
        if has_city && lvl < 5 {
            let need_c = lvl + 1;
            let toward = kept.min(need_c) as f32 / need_c as f32;
            let tier = if s.upgrade_claims_metropolis(p, track) {
                9.0
            } else if need_c >= 4 {
                5.0
            } else if need_c == 3 {
                7.0
            } else {
                4.0
            };
            val += toward * tier;
            if kept >= need_c {
                val += tier * 0.6;
            }
            let extra = kept.saturating_sub(need_c);
            val += (extra / comm_ratio) as f32 * 1.2 + extra as f32 * 0.25;
        } else {
            val += (kept / comm_ratio) as f32 * 1.4 + kept as f32 * 0.2;
        }
    }
    val
}

/// Score of recruiting a level-1 knight at `v`: avoid blocking our own
/// legal settlement spots while preferring opponent roads and the robber.
pub(crate) fn knight_spot_value(s: &GameState, p: u8, v: u8) -> f32 {
    let mut val = 1.0;
    if s.settlement_spots(p, true).contains(v) {
        val -= 20.0;
    }
    if s.topo().tile_vertex_mask[s.robber as usize].contains(v) {
        val += 3.0;
    }
    let at_v = s.topo().vertex_edge_mask[v as usize];
    for q in 0..s.num_players() as u8 {
        if q != p {
            let opp = s.roads[q as usize] | s.ships[q as usize];
            let hits = (opp & at_v).count_ones();
            if hits >= 2 {
                val += 5.0;
            } else if hits == 1 {
                val += 1.5;
            }
        }
    }
    val
}

/// Settlement spots reachable from vertex `from` within two road segments
/// (not passing through opponent buildings), scored and decayed by distance.
fn frontier_value(s: &GameState, p: u8, from: u8, legal_spots: BitSet) -> f32 {
    let blocked = (s.occupied_vertices() & !s.buildings_of(p)) | s.opponent_knights(p);
    let mut best = 0.0f32;
    let mut frontier = vec![(from, 0u8)];
    let mut seen = BitSet::bit(from);
    while let Some((v, d)) = frontier.pop() {
        if legal_spots.contains(v) {
            best = best.max(spot_value(s, p, v) * [1.0, 0.75, 0.5][d as usize]);
        }
        if d == 2 || blocked.contains(v) {
            continue;
        }
        // Roads stay on land.
        let topo = s.topo();
        for k in 0..3 {
            let n = topo.vertex_vertices[v as usize][k];
            if n != NONE
                && !seen.contains(n)
                && s.board.land_edges.contains(topo.vertex_edges[v as usize][k])
            {
                seen.insert(n);
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
    let far = if reach.contains(a) { b } else { a };
    let mut value = frontier_value(s, p, far, spots);
    let mut after = *s;
    after.roads[p as usize].insert(e);
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
    if s.config.scenario.is_citadels() && !s.non_metropolis_cities(p).is_empty() {
        let n = s.num_players();
        let cities_total: u8 = (0..n)
            .map(|q| s.non_metropolis_cities(q as u8).count_ones() as u8)
            .sum();
        let defense_total: u8 = (0..n).map(|q| s.active_knight_strength(q as u8)).sum();
        let my_str = s.active_knight_strength(p);
        let min_vuln_str = (0..n)
            .filter(|&q| !s.non_metropolis_cities(q as u8).is_empty())
            .map(|q| s.active_knight_strength(q as u8))
            .min()
            .unwrap_or(0);
        let urgent = (defense_total < cities_total && my_str <= min_vuln_str)
            || (my_str == 0 && s.barbarian_pos >= 3);
        if urgent {
            if !(s.knights_of(p) & !s.knight_active).is_empty() && !hand.contains(&cost::CITY) {
                return Target::ActivateKnight;
            }
            let can_knight = (ps.knights_left[0] > 0 && !s.knight_recruit_spots(p).is_empty())
                || vertices_in(s.knights_of(p)).any(|v| {
                    let lvl = s.knight_level[v as usize];
                    let max_lvl = if ps.city_progress[CityTrack::Politics.index()] >= 3 {
                        3
                    } else {
                        2
                    };
                    !s.knight_promoted.contains(v)
                        && lvl > 0
                        && lvl < max_lvl
                        && ps.knights_left[lvl as usize] > 0
                });
            if can_knight && !hand.contains(&cost::CITY) && !hand.contains(&cost::SETTLEMENT) {
                return Target::Knight;
            }
        }
    }
    let can_city = ps.cities_left > 0 && !s.settlements[p as usize].is_empty();
    let can_settle = ps.settlements_left > 0 && !s.settlement_spots(p, true).is_empty();
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

pub(crate) fn robber_hurts_me(s: &GameState, p: u8) -> bool {
    !(s.buildings_of(p) & s.topo().tile_vertex_mask[s.robber as usize]).is_empty()
        && tile_pips(s, s.robber) >= 3.0
}

fn knight_wins_army(s: &GameState, p: u8) -> bool {
    let k = s.players[p as usize].knights_played() + 1;
    s.largest_army_owner != Some(p)
        && k >= 3
        && (0..s.num_players()).all(|q| q == p as usize || s.players[q].knights_played() < k)
}

/// Checks whether a proactive 1-for-1 `ProposeTrade` offer is available for
/// `p` to complete `need`.
pub(crate) fn propose_trade_offer(s: &GameState, p: u8, need: ResourceSet) -> Option<Action> {
    let hand = s.players[p as usize].hand;
    if s.phase != Phase::Main || p != s.current || s.trade.is_some() || missing(hand, need) != 1 {
        return None;
    }
    let has_partner = (0..s.num_players() as u8).any(|q| {
        q != p
            && s.players[q as usize].hand.total() > 0
            && s.public_victory_points(q) + 2 < s.target_victory_points(q)
    });
    if !has_partner {
        return None;
    }
    let want = Resource::ALL.into_iter().find(|&r| need[r] > hand[r])?;
    let give = Resource::ALL
        .into_iter()
        .filter(|&r| r != want && hand[r] > need[r])
        .max_by_key(|&r| hand[r] - need[r])?;
    let offer = Action::ProposeTrade {
        give: ResourceSet::single(give, 1),
        get: ResourceSet::single(want, 1),
    };
    s.check(p, offer).is_ok().then_some(offer)
}

/// Whether deactivating knight `v` (via `ChaseRobber` or `MoveKnight`) keeps
/// `p`'s cities safe from an approaching barbarian raid.
fn safe_to_use_knight(s: &GameState, p: u8, v: u8) -> bool {
    if s.non_metropolis_cities(p).is_empty()
        || s.barbarian_pos < BARBARIAN_STEPS - 2
        || s.players[p as usize].hand.contains(&cost::ACTIVATE_KNIGHT)
    {
        return true;
    }
    let n = s.num_players();
    let cities_total: u8 = (0..n)
        .map(|q| s.non_metropolis_cities(q as u8).count_ones() as u8)
        .sum();
    let defense_total: u8 = (0..n).map(|q| s.active_knight_strength(q as u8)).sum();
    let lvl = s.knight_level[v as usize];
    defense_total.saturating_sub(lvl) >= cities_total
}

/// Best barbarian-defense knight action (`ActivateKnight`, `PromoteKnight`, `RecruitKnight`).
fn best_defense_action(s: &GameState, p: u8, legal: &[Action]) -> Option<Action> {
    if let Some(a) = best_by(legal, |a| match a {
        Action::ActivateKnight(v) => Some(s.knight_level[*v as usize] as f32),
        _ => None,
    }) {
        return Some(a);
    }
    if let Some(a) = best_by(legal, |a| match a {
        Action::PromoteKnight(v) if s.knight_active.contains(*v) => {
            Some(10.0 + s.knight_level[*v as usize] as f32)
        }
        _ => None,
    }) {
        return Some(a);
    }
    if let Some(a) = best_by(legal, |a| match a {
        Action::RecruitKnight(v) => Some(knight_spot_value(s, p, *v)),
        _ => None,
    }) {
        return Some(a);
    }
    best_by(legal, |a| match a {
        Action::PromoteKnight(v) => Some(s.knight_level[*v as usize] as f32),
        _ => None,
    })
}

/// Citadels maritime trade helper: convert surplus commodities (or resources)
/// into 1 missing upgrade commodity or needed build resources.
fn citadel_maritime_trade(
    s: &GameState,
    p: u8,
    need: ResourceSet,
    legal: &[Action],
) -> Option<Action> {
    let ps = &s.players[p as usize];
    let hand = ps.hand;
    let comm = ps.commodities;
    let comm_ratio = s.commodity_trade_ratio(p);
    let has_city = !s.cities[p as usize].is_empty();

    let comm_surplus = |c: Commodity, allow_dip: bool| -> i32 {
        let track = track_for_commodity(c);
        let lvl = ps.city_progress[track.index()];
        let reserve = if !has_city
            || lvl >= 5
            || (allow_dip
                && lvl >= 3
                && !s.upgrade_claims_metropolis(p, track)
                && comm[c] < lvl + 1)
        {
            0
        } else {
            (lvl + 1) as i32
        };
        comm[c] as i32 - reserve - comm_ratio as i32
    };

    // 1. Buy the 1 missing commodity for an immediate `UpgradeCity` (especially Lv3 / Metropolis).
    if has_city
        && let Some(a) = best_by(legal, |a| match a {
            Action::MaritimeTradeGood {
                give,
                get: TradeGood::Commodity(get_c),
            } => {
                let get_track = track_for_commodity(*get_c);
                let lvl = ps.city_progress[get_track.index()];
                if lvl >= 5 || comm[*get_c] + 1 != lvl + 1 {
                    return None;
                }
                let ok = match give {
                    TradeGood::Commodity(give_c) => {
                        *give_c != *get_c
                            && (comm_surplus(*give_c, true) >= 0
                                || (s.upgrade_claims_metropolis(p, get_track)
                                    && comm[*give_c] >= comm_ratio
                                    && !s.upgrade_claims_metropolis(
                                        p,
                                        track_for_commodity(*give_c),
                                    )))
                    }
                    TradeGood::Resource(give_r) => {
                        let ratio = s.trade_ratio(p, *give_r) as i32;
                        hand[*give_r] as i32 - need[*give_r] as i32 >= ratio
                            || (s.upgrade_claims_metropolis(p, get_track)
                                && hand[*give_r] as i32 >= ratio
                                && missing(hand, cost::CITY) > 1)
                    }
                };
                ok.then(|| upgrade_city_value(s, p, get_track, None))
            }
            _ => None,
        })
    {
        return Some(a);
    }

    // 2. Convert surplus commodities into needed build resources.
    if missing(hand, need) > 0 {
        let one_short = missing(hand, need) == 1;
        if let Some(a) = best_by(legal, |a| match a {
            Action::MaritimeTradeGood {
                give: TradeGood::Commodity(c),
                get: TradeGood::Resource(r),
            } if need[*r] > hand[*r] => {
                let surp = comm_surplus(*c, one_short || comm_ratio == 2);
                (surp >= 0).then_some(surp as f32 + 5.0 - comm_ratio as f32)
            }
            _ => None,
        }) {
            return Some(a);
        }
    }

    None
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
    if s.public_victory_points(s.current) + 2 >= s.target_victory_points(s.current) {
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

impl GreedyBot {
    fn main_turn(&mut self, s: &GameState, p: u8, legal: &[Action]) -> Option<Action> {
        // 0. Resolve any open domestic trade offer first.
        if s.trade.is_some() && p == s.current {
            if let Some(a) = best_by(legal, |a| match a {
                Action::ConfirmTrade { partner } => {
                    Some(-(s.public_victory_points(*partner) as f32))
                }
                _ => None,
            }) {
                return Some(a);
            }
            if legal.contains(&Action::CancelTrade) {
                return Some(Action::CancelTrade);
            }
        }

        let ps = &s.players[p as usize];
        let hand = ps.hand;
        let citadels = s.config.scenario.is_citadels();
        let fishermen = s.config.scenario.is_fishermen();

        // Fishermen: pass the Old Boot before building (while own public VP is lower).
        if fishermen
            && let Some(a) = best_by(legal, |a| match a {
                Action::PassOldBoot(q) => Some(
                    s.public_victory_points(*q) as f32 * 10.0
                        + s.players[*q as usize].hand.total() as f32,
                ),
                _ => None,
            })
        {
            return Some(a);
        }

        // Citadels: chase the robber off our own tile with a ready knight so
        // any stolen card is available for this turn's builds.
        if citadels
            && robber_hurts_me(s, p)
            && let Some(a) = best_by(legal, |a| match a {
                Action::ChaseRobber {
                    vertex,
                    tile,
                    victim,
                } if safe_to_use_knight(s, p, *vertex) => {
                    Some(robber_value(s, p, *tile, *victim) - 0.1 * s.knight_level[*vertex as usize] as f32)
                }
                _ => None,
            })
        {
            return Some(a);
        }

        // 1. Cities, then settlements: always worth a point.
        if let Some(a) = best_by(legal, |a| match a {
            Action::BuildCity(v) if citadels => Some(citadel_city_value(s, p, *v)),
            Action::BuildCity(v) if fishermen => Some(fishermen_city_value(s, *v)),
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

        if fishermen {
            // Complete an immediate City or Settlement with 4 fish if 1 card short.
            if ps.fish >= 4 {
                let can_city = ps.cities_left > 0 && !s.settlements[p as usize].is_empty();
                let can_settle =
                    ps.settlements_left > 0 && !s.settlement_spots(p, true).is_empty();
                for (ok, goal) in [(can_city, cost::CITY), (can_settle, cost::SETTLEMENT)] {
                    if ok
                        && missing(hand, goal) == 1
                        && let Some(a) = legal.iter().copied().find(|a| match a {
                            Action::FishTakeResource(r) => goal[*r] > hand[*r],
                            _ => false,
                        })
                    {
                        return Some(a);
                    }
                }
            }
            if robber_hurts_me(s, p) && legal.contains(&Action::FishDriveOffRobber) {
                return Some(Action::FishDriveOffRobber);
            }
        }

        // Citadels: immediately take urban improvements, and prioritize
        // barbarian defense when vulnerable or positioned for sole defender.
        if citadels {
            if let Some(a) = best_by(legal, |a| match a {
                Action::UpgradeCity { track, metropolis } => {
                    Some(upgrade_city_value(s, p, *track, *metropolis))
                }
                _ => None,
            }) {
                return Some(a);
            }

            let n = s.num_players();
            let my_vuln = s.non_metropolis_cities(p).count_ones() as u8;
            let cities_total: u8 = (0..n)
                .map(|q| s.non_metropolis_cities(q as u8).count_ones() as u8)
                .sum();
            let defense_total: u8 = (0..n).map(|q| s.active_knight_strength(q as u8)).sum();
            let my_str = s.active_knight_strength(p);
            let min_vuln_str = (0..n)
                .filter(|&q| !s.non_metropolis_cities(q as u8).is_empty())
                .map(|q| s.active_knight_strength(q as u8))
                .min()
                .unwrap_or(0);
            let max_other_str = (0..n)
                .filter(|&q| q != p as usize)
                .map(|q| s.active_knight_strength(q as u8))
                .max()
                .unwrap_or(0);

            let vuln_danger =
                my_vuln > 0 && (defense_total < cities_total || my_str <= min_vuln_str);
            let sole_lead_shot = cities_total > 0
                && s.barbarian_pos >= 4
                && my_str <= max_other_str
                && my_str + 2 > max_other_str
                && defense_total + 2 >= cities_total;
            if (vuln_danger || sole_lead_shot)
                && let Some(a) = best_defense_action(s, p, legal)
            {
                return Some(a);
            }
        }

        let target = choose_target(s, p, hand);
        let need = target.cost();

        // 2. Development cards.
        if !s.dev_card_played {
            if legal.contains(&Action::PlayKnight)
                && (robber_hurts_me(s, p) || knight_wins_army(s, p))
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
                Target::Knight | Target::ActivateKnight => {
                    if let Some(a) = best_defense_action(s, p, legal) {
                        return Some(a);
                    }
                }
                _ => {}
            }
        }

        // 4. Trade surplus with the bank (or Citadels TradeGood) for a missing card.
        if fishermen {
            if matches!(target, Target::Road)
                && let Some(a) = best_by(legal, |a| match a {
                    Action::FishBuildRoad(e) => Some(road_value(s, p, *e)),
                    _ => None,
                })
            {
                return Some(a);
            }
            if legal.contains(&Action::FishBuyDevCard) {
                return Some(Action::FishBuyDevCard);
            }
            if missing(hand, need) > 0
                && (missing(hand, need) <= 2 || ps.fish >= 6)
                && let Some(a) = best_by(legal, |a| match a {
                    Action::FishTakeResource(r) if need[*r] > hand[*r] => {
                        Some((need[*r] - hand[*r]) as f32)
                    }
                    _ => None,
                })
            {
                return Some(a);
            }
        }
        if citadels && let Some(a) = citadel_maritime_trade(s, p, need, legal) {
            return Some(a);
        }
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

        // 4b. Proactive 1-for-1 player trade offer (at most once per turn).
        if self.propose_trades
            && s.phase == Phase::Main
            && p == s.current
            && s.trade.is_none()
            && self.last_offer != Some((s.turn, p))
            && let Some(offer) = propose_trade_offer(s, p, need)
        {
            self.last_offer = Some((s.turn, p));
            return Some(offer);
        }

        // 5. Spend spare cards that the target does not need (and Citadels knight actions).
        let total_cards = if citadels {
            ps.total_cards()
        } else {
            hand.total()
        };
        let spare_after = |price: ResourceSet| {
            hand.contains(&price) && missing(hand - price, need) == missing(hand, need)
        };

        if citadels {
            // Activate idle knights when Grain is spare or hand overflows.
            if (spare_after(cost::ACTIVATE_KNIGHT) || total_cards > 7)
                && let Some(a) = best_by(legal, |a| match a {
                    Action::ActivateKnight(v) => Some(s.knight_level[*v as usize] as f32),
                    _ => None,
                })
            {
                return Some(a);
            }
            // Use ready knights to rob a leader or displace a weaker opponent knight.
            if let Some(a) = best_by(legal, |a| match a {
                Action::ChaseRobber {
                    vertex,
                    tile,
                    victim: Some(q),
                } if safe_to_use_knight(s, p, *vertex)
                    && (s.public_victory_points(*q) + 4 >= s.config.victory_points
                        || s.public_victory_points(*q) > s.public_victory_points(p)) =>
                {
                    let v = robber_value(s, p, *tile, Some(*q));
                    (v > 3.0).then_some(v)
                }
                _ => None,
            }) {
                return Some(a);
            }
            if let Some(a) = best_by(legal, |a| match a {
                Action::MoveKnight { from, to } if safe_to_use_knight(s, p, *from) => {
                    if s.knight_owner[*to as usize] != NONE && s.knight_owner[*to as usize] != p {
                        let opp = s.knight_owner[*to as usize];
                        let destroyed = s.knight_retreat_spot(opp, *to, *from).is_none();
                        Some(if destroyed { 10.0 } else { 4.0 } + s.knight_level[*to as usize] as f32)
                    } else {
                        // Vacate `from` if our own knight is standing on a valid settlement spot!
                        let occ = s.occupied_vertices();
                        let other_k = s.all_knights() & !BitSet::bit(*from);
                        let would_be_spot = s.board.land_vertices.contains(*from)
                            && !(occ | s.topo().neighbors_of(occ) | other_k).contains(*from)
                            && !(s.topo().vertex_edge_mask[*from as usize]
                                & (s.roads[p as usize] | s.ships[p as usize]))
                                .is_empty();
                        (would_be_spot && knight_spot_value(s, p, *to) > 0.0)
                            .then(|| knight_spot_value(s, p, *to))
                    }
                }
                _ => None,
            }) {
                return Some(a);
            }
            if (spare_after(cost::KNIGHT) || (total_cards > 7 && hand.contains(&cost::KNIGHT)))
                && let Some(a) = best_defense_action(s, p, legal)
            {
                return Some(a);
            }
            if total_cards > 7
                && let Some(a) = best_by(legal, |a| match a {
                    Action::MaritimeTradeGood {
                        give: TradeGood::Commodity(c),
                        get: TradeGood::Resource(r),
                    } => {
                        let track = track_for_commodity(*c);
                        let lvl = ps.city_progress[track.index()];
                        let keep = if lvl < 5 && !s.cities[p as usize].is_empty() {
                            lvl + 1
                        } else {
                            0
                        };
                        (ps.commodities[*c] >= keep + s.commodity_trade_ratio(p)
                            || lvl >= 5)
                            .then_some(if need[*r] > hand[*r] { 2.0 } else { 1.0 })
                    }
                    _ => None,
                })
            {
                return Some(a);
            }
        }

        if legal.contains(&Action::BuyDevCard) && (spare_after(cost::DEV_CARD) || total_cards > 7) {
            return Some(Action::BuyDevCard);
        }
        if (spare_after(cost::ROAD) || (total_cards > 7 && hand.contains(&cost::ROAD)))
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
            if (spare_after(cost::SHIP) || (total_cards > 7 && hand.contains(&cost::SHIP)))
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

        if fishermen {
            if let Some(a) = best_by(legal, |a| match a {
                Action::FishBuildRoad(e) => {
                    let v = road_value(s, p, *e);
                    (v > 0.5).then_some(v)
                }
                _ => None,
            }) {
                return Some(a);
            }
            if let Some(a) = best_by(legal, |a| match a {
                Action::FishSteal(q)
                    if ps.fish >= 5
                        || s.public_victory_points(*q) + 3 >= s.target_victory_points(*q)
                        || s.players[*q as usize].hand.total() >= 4 =>
                {
                    Some(
                        s.public_victory_points(*q) as f32 * 2.0
                            + s.players[*q as usize].hand.total() as f32,
                    )
                }
                _ => None,
            }) {
                return Some(a);
            }
        }

        Some(Action::EndTurn)
    }
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
