//! Static evaluation: how strong each seat's position is, in victory-point
//! units, and the conversion into per-seat rewards for search.

use isle_core::board::PIPS;
use isle_core::topology::*;
use isle_core::{DevCard, GameState, MAX_PLAYERS};

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
    /// Sharpness of the win-probability estimate (per point of lead).
    pub temperature: f32,
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
            temperature: 1.3,
        }
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
            for &t in &VERTEX_TILES[v as usize] {
                if t == NONE {
                    break;
                }
                if let Some(r) = s.board.terrain[t as usize].resource() {
                    let pips = PIPS[s.board.numbers[t as usize] as usize] as f32;
                    let robbed = if t == s.robber { 0.35 } else { 1.0 };
                    out[r.index()] += pips / 36.0 * mult * robbed;
                }
            }
        }
    }
    out
}

/// Position strength of every seat, in victory-point units.
pub fn scores(s: &GameState, w: &Weights, out: &mut [f32; MAX_PLAYERS]) {
    let n = s.num_players();
    let scarce = crate::greedy::scarcity(s);
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
            if ps.cities_left > 0 && s.settlements[q as usize] != 0 {
                potential += near(cost::CITY);
            }
            if ps.settlements_left > 0 && s.settlement_spots(q, true) != 0 {
                potential += near(cost::SETTLEMENT);
            }
            if s.dev_deck_len > 0 {
                potential += 0.4 * near(cost::DEV_CARD);
            }
            score += w.build * potential;
        }

        let cards = ps.hand.total() as f32;
        score += w.hand * cards.min(7.0) - w.overflow * (cards - 7.0).max(0.0);

        let vp_cards = ps.dev_cards[DevCard::VictoryPoint.index()] as f32;
        score += w.dev_card * (ps.dev_card_count() as f32 - vp_cards);
        if s.largest_army_owner != Some(q) {
            score += w.knight * ps.knights_played().min(3) as f32;
        }
        if s.longest_road_owner != Some(q) {
            score += w.road * ps.longest_road.min(6) as f32;
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
        assert_eq!(r, [0.0, 0.0, 1.0, 0.0]);
    }
}
