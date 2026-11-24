//! Ranked play: tiers, rating changes after a game, the leaderboard
//! (`GET /api/leaderboard?limit=&offset=`) and player profiles
//! (`GET /api/players/{username}`).
//!
//! Ratings start at 1000. The first [`PLACEMENT_GAMES`] ranked games are
//! placement games: the rating moves but no tier or leaderboard rank is
//! shown yet.

use serde::{Deserialize, Serialize};

/// Ranked games before a player gets a tier and a leaderboard rank (the
/// server can be configured otherwise).
pub const PLACEMENT_GAMES: u32 = 5;

/// Rating bands, lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Tier {
    /// Below 1100.
    Bronze,
    /// 1100–1249.
    Silver,
    /// 1250–1399.
    Gold,
    /// 1400–1549.
    Platinum,
    /// 1550–1699.
    Diamond,
    /// 1700 and up.
    Master,
}

impl Tier {
    pub fn of(rating: i32) -> Tier {
        match rating {
            ..1100 => Tier::Bronze,
            1100..1250 => Tier::Silver,
            1250..1400 => Tier::Gold,
            1400..1550 => Tier::Platinum,
            1550..1700 => Tier::Diamond,
            _ => Tier::Master,
        }
    }
}

/// A player's rating as shown next to their name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SeatRating {
    pub rating: i32,
    /// `None` during placement games.
    pub tier: Option<Tier>,
}

/// How a ranked game changed one player's rating.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RatingChange {
    pub seat: u8,
    pub username: String,
    pub before: i32,
    pub after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LeaderboardEntry {
    /// 1-based; players with equal ratings share a rank.
    pub rank: u32,
    pub username: String,
    pub rating: i32,
    pub tier: Tier,
    pub games: u32,
    pub wins: u32,
}

/// One page of the leaderboard: placed players, best first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Leaderboard {
    pub entries: Vec<LeaderboardEntry>,
    /// Placed players in total.
    pub total: u32,
}

/// A ranked game from one player's side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MatchSummary {
    /// Replay id (`#/replay/{id}`, `/api/replays/{id}`).
    pub id: String,
    /// Unix seconds.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub finished_at: i64,
    /// Seats in the game, bots and guests included.
    pub seats: u8,
    /// 1-based finishing place among all seats.
    pub place: u8,
    pub points: u8,
    pub before: i32,
    pub after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PlayerProfile {
    pub username: String,
    pub rating: i32,
    /// `None` during placement games.
    pub tier: Option<Tier>,
    /// Leaderboard rank, once placed.
    pub rank: Option<u32>,
    pub games: u32,
    pub wins: u32,
    /// Placement games still to play (0 once placed).
    pub placement_left: u32,
    /// Most recent ranked games, newest first.
    pub recent: Vec<MatchSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_bands() {
        let bands = [
            (i32::MIN, Tier::Bronze),
            (1099, Tier::Bronze),
            (1100, Tier::Silver),
            (1249, Tier::Silver),
            (1250, Tier::Gold),
            (1400, Tier::Platinum),
            (1550, Tier::Diamond),
            (1699, Tier::Diamond),
            (1700, Tier::Master),
            (i32::MAX, Tier::Master),
        ];
        for (rating, tier) in bands {
            assert_eq!(Tier::of(rating), tier, "{rating}");
        }
    }
}
