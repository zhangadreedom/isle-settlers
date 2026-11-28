//! Static evaluation: how strong each seat's position is, in victory-point
//! units, and the conversion into per-seat rewards for search.

use isle_core::board::PIPS;
use isle_core::topology::{NONE, vertices_in};
use isle_core::{BARBARIAN_STEPS, CityTrack, DevCard, GameState, MAX_PLAYERS, Terrain};

use crate::BotPersona;

/// Tunable weights of the evaluation.
#[derive(Clone, Copy, Debug)]
pub struct Weights {
    /// Per expected card per dice roll (weighted by scarcity).
    pub income: f32,
    /// Per resource type produced.
    pub diversity: f32,
    /// Per settlement spot reachable now (up to 3).
    pub spots: f32,
    /// Per card in hand (up to the discard limit).
    pub hand: f32,
    /// Per card above the discard limit (exposed to a 7).
    pub overflow: f32,
    /// Per unplayed development card (victory points are counted as points).
    pub dev_card: f32,
    /// Per knight played, while largest army is not settled.
    pub knight: f32,
    /// Per segment of longest road.
    pub road: f32,
    /// Per harbor matching own production.
    pub harbor: f32,
    /// How close the hand is to the next city / settlement / card.
    pub build: f32,
    /// Seafaring: how close the fleet is to an island bonus (1 = a spot
    /// in reach now).
    pub sea: f32,
    /// Sharpness of the win-probability estimate (per point of lead).
    pub temperature: f32,
    /// Optional AI personality bias (`Balanced` keeps canonical weights).
    pub persona: BotPersona,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            income: 2.4,
            diversity: 0.25,
            spots: 0.35,
            hand: 0.07,
            overflow: 0.1,
            dev_card: 0.3,
            knight: 0.25,
            road: 0.08,
            harbor: 0.2,
            build: 0.0,
            sea: 0.8,
            temperature: 1.3,
            persona: BotPersona::Balanced,
        }
    }
}

impl Weights {
    /// Evaluation weights biased for `persona`.
    pub fn for_persona(persona: BotPersona) -> Self {
        let mut w = Weights {
            persona,
            ..Weights::default()
        };
        match persona {
            BotPersona::Balanced => {}
            BotPersona::RoadBuilder => {
                w.road *= 1.35;
                w.sea *= 1.35;
                w.spots *= 1.35;
            }
            BotPersona::CityBaron => {
                w.income *= 1.15;
            }
            BotPersona::Warlord => {
                w.dev_card *= 1.35;
                w.knight *= 1.35;
            }
            BotPersona::Merchant => {
                w.harbor *= 1.35;
            }
        }
        w
    }
}

/// Expected cards of each resource per dice roll for seat `q` (robbed tiles
/// count a little: the robber moves).
pub fn income(s: &GameState, q: u8) -> [f32; 5] {
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
                let pips = PIPS[s.board.numbers[t as usize] as usize] as f32;
                let robbed = if t == s.robber { 0.35 } else { 1.0 };
                match s.board.terrain[t as usize] {
                    // Gold is any resource.
                    Terrain::Gold => {
                        for x in &mut out {
                            *x += pips / 36.0 * mult * robbed * 0.2;
                        }
                    }
                    terrain => {
                        if let Some(r) = terrain.resource() {
                            out[r.index()] += pips / 36.0 * mult * robbed;
                        }
                    }
                }
            }
        }
    }
    out
}

/// Expected commodities per dice roll for seat `q` in Citadels mode.
pub fn commodity_income(s: &GameState, q: u8) -> [f32; 3] {
    let mut out = [0.0; 3];
    for v in vertices_in(s.cities[q as usize]) {
        for &t in &s.topo().vertex_tiles[v as usize] {
            if t == NONE {
                break;
            }
            if let Some(c) = s.board.terrain[t as usize].commodity() {
                let pips = PIPS[s.board.numbers[t as usize] as usize] as f32;
                let robbed = if t == s.robber { 0.35 } else { 1.0 };
                out[c.index()] += pips / 36.0 * robbed;
            }
        }
    }
    out
}

/// Expected fish tokens caught per dice roll for seat `q` in Fishermen mode.
pub fn fish_income(s: &GameState, q: u8) -> f32 {
    let topo = s.topo();
    let mut out = 0.0f32;
    for fg in s.board.active_fishing_grounds() {
        let pips = PIPS[fg.number as usize] as f32;
        for v in topo.edge_vertices[fg.edge as usize] {
            if s.settlements[q as usize].contains(v) {
                out += pips / 36.0;
            } else if s.cities[q as usize].contains(v) {
                out += 2.0 * pips / 36.0;
            }
        }
    }
    if s.board.desert != NONE {
        let lake_pips = if s.robber == s.board.desert { 1.1 } else { 2.0 };
        let tv = topo.tile_vertex_mask[s.board.desert as usize];
        let s_cnt = (s.settlements[q as usize] & tv).count_ones() as f32;
        let c_cnt = (s.cities[q as usize] & tv).count_ones() as f32;
        out += (s_cnt + 2.0 * c_cnt) * (lake_pips / 36.0);
    }
    out
}

/// Expected gold coins gained per dice roll for seat `q` in Caravans mode.
pub fn coin_income(s: &GameState, q: u8) -> f32 {
    let topo = s.topo();
    let buildings = s.buildings_of(q);
    let mut out = 0.0f32;
    for v in vertices_in(buildings) {
        for &t in &topo.vertex_tiles[v as usize] {
            if t == NONE {
                break;
            }
            let terrain = s.board.terrain[t as usize];
            let gives_coin = matches!(terrain, Terrain::Gold | Terrain::Mountains)
                || !(topo.tile_edge_mask(t) & s.board.river_edges).is_empty();
            if gives_coin && terrain.produces() {
                let pips = PIPS[s.board.numbers[t as usize] as usize] as f32;
                let robbed = if t == s.robber { 0.35 } else { 1.0 };
                out += pips / 36.0 * robbed;
            }
        }
    }
    out
}

/// Evaluates every seat's position strength with `persona` biases applied.
pub fn evaluate(s: &GameState, persona: BotPersona, out: &mut [f32; MAX_PLAYERS]) {
    scores(s, &Weights::for_persona(persona), out);
}

/// Position strength of every seat, in victory-point units.
pub fn scores(s: &GameState, w: &Weights, out: &mut [f32; MAX_PLAYERS]) {
    let n = s.num_players();
    let scarce = crate::greedy::scarcity(s);
    let citadels = s.config.scenario.is_citadels();
    let (cities_total, defense_total, min_vuln_str, max_str, max_count, urgency) = if citadels {
        let mut c_tot = 0u8;
        let mut d_tot = 0u8;
        let mut min_v: Option<u8> = None;
        let mut max_s = 0u8;
        let mut max_c = 0usize;
        for q in 0..n as u8 {
            let vuln = s.non_metropolis_cities(q).count_ones() as u8;
            let st = s.active_knight_strength(q);
            c_tot += vuln;
            d_tot += st;
            if vuln > 0 {
                min_v = Some(min_v.map_or(st, |m| m.min(st)));
            }
            if st > max_s {
                max_s = st;
                max_c = 1;
            } else if st == max_s {
                max_c += 1;
            }
        }
        let urg = 0.4 + 0.6 * (s.barbarian_pos as f32 / BARBARIAN_STEPS as f32);
        (c_tot, d_tot, min_v, max_s, max_c, urg)
    } else {
        (0, 0, None, 0, 0, 0.0)
    };

    for q in 0..n as u8 {
        let ps = &s.players[q as usize];
        let mut score = s.victory_points(q) as f32;

        let inc = income(s, q);
        let weighted: f32 = (0..5).map(|r| inc[r] * scarce[r]).sum();
        score += w.income * weighted;
        score += w.diversity * inc.iter().filter(|&&x| x > 0.0).count() as f32;
        for r in 0..5 {
            if ps.harbors & (1 << r) != 0 && inc[r] > 0.08 {
                score += w.harbor;
            }
        }
        if ps.harbors & (1 << 5) != 0 {
            score += w.harbor * 0.5;
        }

        if ps.settlements_left > 0 {
            let spots = s.settlement_spots(q, true).count_ones().min(3);
            score += w.spots * spots as f32;
        }

        if w.build != 0.0 {
            use isle_core::resources::cost;
            let near = |price| 1.0 / (1.0 + crate::greedy::missing(ps.hand, price) as f32);
            let mut potential = 0.0;
            if ps.cities_left > 0 && !s.settlements[q as usize].is_empty() {
                potential += near(cost::CITY);
            }
            if ps.settlements_left > 0 && !s.settlement_spots(q, true).is_empty() {
                potential += near(cost::SETTLEMENT);
            }
            if s.dev_deck_len > 0 {
                potential += 0.4 * near(cost::DEV_CARD);
            }
            score += w.build * potential;
        }

        let cards = if citadels {
            ps.total_cards() as f32
        } else {
            ps.hand.total() as f32
        };
        score += w.hand * cards.min(7.0) - w.overflow * (cards - 7.0).max(0.0);

        let vp_cards = ps.dev_cards[DevCard::VictoryPoint.index()] as f32;
        score += w.dev_card * (ps.dev_card_count() as f32 - vp_cards);
        if s.largest_army_owner != Some(q) {
            score += w.knight * ps.knights_played().min(3) as f32;
        }
        if s.longest_road_owner != Some(q) {
            score += w.road * ps.longest_road.min(6) as f32;
        }
        if w.sea != 0.0 && s.board.layout.island_bonus() > 0 {
            score += w.sea * crate::sea::island_progress(s, q);
        }

        if s.config.scenario.is_fishermen() {
            let f_mult = if w.persona == BotPersona::Merchant {
                1.35
            } else {
                1.0
            };
            score += (w.income * 0.42 * fish_income(s, q) + 0.09 * ps.fish as f32) * f_mult;
            if s.old_boot_owner == Some(q) {
                score -= 0.85;
            }
        }

        if s.config.scenario.is_caravans() {
            let c_mult = if w.persona == BotPersona::Merchant {
                1.35
            } else {
                1.0
            };
            score += 0.55 * c_mult * coin_income(s, q);
            score += 0.18 * c_mult * ps.coins.min(8) as f32;
            if ps.coins >= 1 {
                score += 0.25 * c_mult;
            }
        }

        if s.config.scenario.is_explorers() {
            let c_mult = if w.persona == BotPersona::Merchant {
                1.35
            } else {
                1.0
            };
            score += 0.22 * c_mult * ps.coins.min(10) as f32;
            if ps.explorer_missions[2] >= 1 {
                score += 0.35 * c_mult;
            }
            if ps.explorer_missions[2] >= 2 {
                score += 0.55 * c_mult;
            }
            for m in 0..3 {
                if ps.explorer_missions[m] < 3 && crate::sea::explorer_track_reached(s, q, m) {
                    score += 0.45
                        + if s.explorer_champions[m].is_none() && ps.explorer_missions[m] == 2 {
                            0.35
                        } else {
                            0.0
                        };
                }
            }
        }

        match w.persona {
            BotPersona::Balanced => {}
            BotPersona::RoadBuilder => {
                if s.longest_road_owner == Some(q) {
                    score += 0.7;
                }
                if s.config.scenario.is_caravans() {
                    let camel_roads =
                        ((s.roads[q as usize] | s.bridges[q as usize]) & s.camels).count_ones()
                            as f32;
                    score +=
                        0.28 * camel_roads + 0.22 * s.bridges[q as usize].count_ones() as f32;
                }
            }
            BotPersona::CityBaron => {
                score += 0.35 * w.income * (inc[3] * scarce[3] + inc[4] * scarce[4]);
                score += 0.7 * s.cities[q as usize].count_ones() as f32;
                if citadels {
                    let metros = s
                        .metropolis_owners
                        .iter()
                        .filter(|&&o| o == Some(q))
                        .count() as f32;
                    let prog: u8 = ps.city_progress.iter().sum();
                    score += 0.7 * metros + 0.15 * prog as f32;
                }
            }
            BotPersona::Warlord => {
                if s.largest_army_owner == Some(q) {
                    score += 0.7;
                }
                if citadels {
                    score += 0.35 * ps.defender_points as f32
                        + 0.08 * s.active_knight_strength(q) as f32;
                }
            }
            BotPersona::Merchant => {
                if s.wealthiest_owner == Some(q) {
                    score += 0.7;
                }
                if citadels {
                    let csum: f32 = commodity_income(s, q).iter().sum();
                    score += 0.7 * csum + 0.25 * ps.city_progress[CityTrack::Trade.index()] as f32;
                }
            }
        }

        if citadels {
            let cinc = commodity_income(s, q);
            let csum: f32 = cinc.iter().sum();
            let ckinds = cinc.iter().filter(|&&x| x > 0.0).count() as f32;
            score += 2.0 * csum + 0.2 * ckinds;

            let has_city = !s.cities[q as usize].is_empty();
            for track in CityTrack::ALL {
                let lvl = ps.city_progress[track.index()];
                score += match lvl {
                    0 => 0.0,
                    1 => 0.15,
                    2 => 0.35,
                    3 => {
                        if track == CityTrack::Science {
                            1.1
                        } else {
                            0.85
                        }
                    }
                    4 => 1.05,
                    _ => 1.25,
                };
                if has_city && lvl < 5 {
                    let next = lvl + 1;
                    let have = ps.commodities[track.commodity()].min(next) as f32;
                    let frac = have / next as f32;
                    let weight = if s.upgrade_claims_metropolis(q, track) {
                        0.65
                    } else if next == 3 {
                        0.4
                    } else {
                        0.2
                    };
                    score += weight * frac;
                }
            }

            let active_str = s.active_knight_strength(q);
            let inactive_lvl: u8 = vertices_in(s.knights_of(q) & !s.knight_active)
                .map(|v| s.knight_level[v as usize])
                .sum();
            score += 0.18 * active_str.min(6) as f32 + 0.08 * inactive_lvl.min(4) as f32;

            if cities_total > 0 {
                if defense_total >= cities_total && max_str > 0 && active_str == max_str {
                    score += if max_count == 1 {
                        0.6 * urgency
                    } else {
                        0.2 * urgency
                    };
                }
                if !s.non_metropolis_cities(q).is_empty() && min_vuln_str == Some(active_str) {
                    if defense_total < cities_total {
                        score -= 0.9 * urgency;
                    } else if active_str == 0 {
                        score -= 0.25 * urgency;
                    }
                }
            }
        }

        out[q as usize] = score;
    }
}

/// Estimated chance of winning for every seat: 1/0 once the game is over,
/// otherwise a softmax over position scores.
pub fn rewards(s: &GameState, w: &Weights, out: &mut [f32; MAX_PLAYERS]) {
    let n = s.num_players();
    if let Some(winner) = s.winner {
        for q in 0..n {
            out[q] = if q == winner as usize { 1.0 } else { 0.0 };
        }
        return;
    }
    let mut sc = [0.0f32; MAX_PLAYERS];
    scores(s, w, &mut sc);
    let max = sc[..n].iter().copied().fold(f32::MIN, f32::max);
    let mut total = 0.0;
    for q in 0..n {
        out[q] = ((sc[q] - max) * w.temperature).exp();
        total += out[q];
    }
    for q in 0..n {
        out[q] /= total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use isle_core::{Action, GameConfig};

    #[test]
    fn more_points_and_production_score_higher() {
        let mut g = GameState::new(GameConfig::default(), 3).unwrap();
        // Finish setup with the first legal spots.
        let mut legal = Vec::new();
        while g.is_setup() {
            let p = g.current;
            g.legal_actions(p, &mut legal);
            g.apply(p, legal[0]).unwrap();
        }
        let w = Weights::default();
        let mut base = [0.0; MAX_PLAYERS];
        scores(&g, &w, &mut base);
        let mut h = g;
        h.players[1].dev_cards[DevCard::VictoryPoint.index()] += 2;
        let mut more = [0.0; MAX_PLAYERS];
        scores(&h, &w, &mut more);
        assert!(more[1] > base[1] + 1.5);

        let mut r = [0.0; MAX_PLAYERS];
        rewards(&h, &w, &mut r);
        assert!((r.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        assert!(r[1] > r[0] && r[1] > r[2] && r[1] > r[3]);
        let _ = Action::EndTurn;
    }

    #[test]
    fn finished_games_reward_the_winner_only() {
        let mut g = GameState::new(GameConfig::default(), 4).unwrap();
        g.winner = Some(2);
        let mut r = [0.0; MAX_PLAYERS];
        rewards(&g, &Weights::default(), &mut r);
        assert_eq!(r[..4], [0.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn citadels_scores_tracks_defense_and_total_card_overflow() {
        use isle_core::{Commodity, Scenario};

        let mut g = GameState::new(GameConfig::scenario(Scenario::Citadels, 4), 11).unwrap();
        let mut legal = Vec::new();
        while g.is_setup() {
            let p = g.current;
            g.legal_actions(p, &mut legal);
            g.apply(p, legal[0]).unwrap();
        }
        let w = Weights::default();
        let mut base = [0.0; MAX_PLAYERS];
        scores(&g, &w, &mut base);

        // Unlocking Science Lv3 + defending with an active knight boosts seat 0.
        let mut defended = g;
        defended.players[0].city_progress[CityTrack::Science.index()] = 3;
        let kv = vertices_in(defended.knight_recruit_spots(0)).next().unwrap();
        defended.knight_owner[kv as usize] = 0;
        defended.knight_level[kv as usize] = 2;
        defended.players[0].knights_left[1] -= 1;
        defended.knight_active.insert(kv);
        defended.barbarian_pos = 6;
        let mut after = [0.0; MAX_PLAYERS];
        scores(&defended, &w, &mut after);
        assert!(after[0] > base[0] + 1.0);

        // Holding 6 resources + 5 commodities = 11 total cards incurs the >7 overflow penalty.
        let mut safe = g;
        safe.players[0].city_progress = [5, 5, 5];
        safe.players[0].commodities[Commodity::Paper] = 1;
        let mut bloated = safe;
        bloated.players[0].commodities[Commodity::Paper] = 9;
        bloated.commodity_bank[Commodity::Paper] -= 8;
        let (mut s_safe, mut s_bloated) = ([0.0; MAX_PLAYERS], [0.0; MAX_PLAYERS]);
        scores(&safe, &w, &mut s_safe);
        scores(&bloated, &w, &mut s_bloated);
        let hand_cap_gain = w.hand * (7.0 - safe.players[0].total_cards().min(7) as f32);
        assert!(s_bloated[0] < s_safe[0] + hand_cap_gain);
    }
}
