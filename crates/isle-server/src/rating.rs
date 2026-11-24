//! Skill ratings for free-for-all games of 2–6 players.
//!
//! Weng & Lin, "A Bayesian Approximation Method for Online Ranking" (JMLR
//! 2011), Plackett–Luce model, with one player per team: the same update
//! OpenSkill uses. Each player has a skill estimate `mu` and an uncertainty
//! `sigma`; a game updates everyone from the order they finished in, ties
//! included. Before each game `sigma` grows by `TAU`, so ratings keep moving
//! as players improve.
//!
//! The rating people see is the conservative estimate `mu - 3 sigma`, mapped
//! to `1000 + 25 (mu - 3 sigma)`: everyone starts at 1000 and climbs as the
//! system becomes sure of them, which also keeps lucky newcomers off the top
//! of the leaderboard.

use isle_proto::ranking::Tier;

pub const MU: f64 = 25.0;
pub const SIGMA: f64 = MU / 3.0;
/// Performance spread within one game (luck): half the starting sigma.
pub const BETA: f64 = SIGMA / 2.0;
/// Lower bound of the variance shrink factor, so `sigma` never collapses.
pub const KAPPA: f64 = 1e-4;
/// Added uncertainty per game.
pub const TAU: f64 = MU / 300.0;

/// One player's skill estimate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skill {
    pub mu: f64,
    pub sigma: f64,
}

impl Default for Skill {
    fn default() -> Skill {
        Skill {
            mu: MU,
            sigma: SIGMA,
        }
    }
}

impl Skill {
    /// The rating shown to players.
    pub fn display(self) -> i32 {
        (1000.0 + 25.0 * (self.mu - 3.0 * self.sigma)).round() as i32
    }

    pub fn tier(self) -> Tier {
        Tier::of(self.display())
    }
}

/// New skills after a game. `places[i]` is player `i`'s finishing place
/// (lower is better, equal values are ties; only the order matters).
pub fn rate(players: &[Skill], places: &[u32]) -> Vec<Skill> {
    assert_eq!(players.len(), places.len());
    let n = players.len();
    if n < 2 {
        return players.to_vec();
    }
    let players: Vec<Skill> = players
        .iter()
        .map(|p| Skill {
            mu: p.mu,
            sigma: (p.sigma * p.sigma + TAU * TAU).sqrt(),
        })
        .collect();
    let c = players
        .iter()
        .map(|p| p.sigma * p.sigma + BETA * BETA)
        .sum::<f64>()
        .sqrt();
    let strength: Vec<f64> = players.iter().map(|p| (p.mu / c).exp()).collect();
    // For each q: total strength of everyone placed at or below q, and how
    // many share q's place.
    let sum_q: Vec<f64> = (0..n)
        .map(|q| {
            (0..n)
                .filter(|&i| places[i] >= places[q])
                .map(|i| strength[i])
                .sum()
        })
        .collect();
    let ties: Vec<f64> = (0..n)
        .map(|q| (0..n).filter(|&i| places[i] == places[q]).count() as f64)
        .collect();
    players
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (mut omega, mut delta) = (0.0, 0.0);
            for q in (0..n).filter(|&q| places[q] <= places[i]) {
                let quotient = strength[i] / sum_q[q];
                omega += if q == i { 1.0 - quotient } else { -quotient } / ties[q];
                delta += quotient * (1.0 - quotient) / ties[q];
            }
            let variance = p.sigma * p.sigma;
            let gamma = p.sigma / c;
            Skill {
                mu: p.mu + omega * variance / c,
                sigma: p.sigma * (1.0 - gamma * delta * variance / (c * c)).max(KAPPA).sqrt(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn matches_the_reference_two_player_update() {
        // Values from the published Plackett–Luce update (as in OpenSkill).
        let r = rate(&[Skill::default(); 2], &[0, 1]);
        assert!(close(r[0].mu, 27.635389493), "{r:?}");
        assert!(close(r[1].mu, 22.364610507), "{r:?}");
        assert!(close(r[0].sigma, 8.065901414) && close(r[1].sigma, r[0].sigma));
    }

    #[test]
    fn better_places_gain_more() {
        let r = rate(&[Skill::default(); 4], &[0, 1, 2, 3]);
        let d: Vec<i32> = r.iter().map(|s| s.display()).collect();
        assert!(d.windows(2).all(|w| w[0] > w[1]), "{d:?}");
        assert!(d[0] > 1000 && d[3] < 1000, "{d:?}");
        // First game, four newcomers: about +75 for the winner, -82 last.
        assert_eq!(d, [1075, 1050, 1011, 918]);
        assert!(r.iter().all(|s| s.sigma < SIGMA));
    }

    #[test]
    fn ties_are_rated_alike() {
        let r = rate(&[Skill::default(); 3], &[0, 1, 1]);
        assert!(close(r[1].mu, r[2].mu) && close(r[1].sigma, r[2].sigma));
        // Only the order matters, not the numbers.
        let gapped = rate(&[Skill::default(); 3], &[2, 7, 7]);
        assert!(r.iter().zip(&gapped).all(|(a, b)| close(a.mu, b.mu)));
    }

    #[test]
    fn upsets_move_ratings_more_than_expected_results() {
        let strong = Skill {
            mu: 32.0,
            sigma: 3.0,
        };
        let weak = Skill {
            mu: 20.0,
            sigma: 3.0,
        };
        let expected = rate(&[strong, weak], &[0, 1]);
        let upset = rate(&[strong, weak], &[1, 0]);
        assert!(upset[1].mu - weak.mu > expected[0].mu - strong.mu);
        assert!(upset[0].mu < strong.mu && upset[1].mu > weak.mu);
    }

    #[test]
    fn tiers_follow_the_rating() {
        assert_eq!(Skill::default().display(), 1000);
        assert_eq!(Skill::default().tier(), Tier::Bronze);
        assert_eq!(Tier::of(1099), Tier::Bronze);
        assert_eq!(Tier::of(1100), Tier::Silver);
        assert_eq!(Tier::of(1250), Tier::Gold);
        assert_eq!(Tier::of(1400), Tier::Platinum);
        assert_eq!(Tier::of(1550), Tier::Diamond);
        assert_eq!(Tier::of(1700), Tier::Master);
    }

    #[test]
    fn ratings_converge_to_skill_order() {
        // Deterministic pseudo-random games between players of known skill.
        let skills = [-1.5, -0.5, 0.0, 0.5, 1.5];
        let mut ratings = [Skill::default(); 5];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut noise = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        for _ in 0..400 {
            let perf: Vec<f64> = skills.iter().map(|s| s + 3.0 * noise()).collect();
            let places: Vec<u32> = perf
                .iter()
                .map(|p| perf.iter().filter(|q| *q > p).count() as u32)
                .collect();
            let new = rate(&ratings, &places);
            ratings.copy_from_slice(&new);
        }
        let d: Vec<i32> = ratings.iter().map(|s| s.display()).collect();
        assert!(d.windows(2).all(|w| w[0] < w[1]), "{d:?}");
    }
}
