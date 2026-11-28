//! Anti-cheat Global Daily Challenge speedrun leaderboard types
//! (`GET /api/daily?date=YYYY-MM-DD`, `POST /api/daily`).

use serde::{Deserialize, Serialize};

use crate::ReplayRecord;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DailyScoreEntry {
    pub rank: u32,
    pub player_name: String,
    pub registered: bool,
    pub won: bool,
    pub turns: u32,
    pub points: u8,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub submitted_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DailyLeaderboard {
    pub date_key: String,
    pub entries: Vec<DailyScoreEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct DailySubmitRequest {
    pub date_key: String,
    pub seat: u8,
    pub player_name: String,
    pub replay: ReplayRecord,
}
