//! Anti-cheat Global Daily Challenge leaderboard (`GET /api/daily`, `POST /api/daily`).
//!
//! Every score submission includes a full deterministic [`isle_proto::ReplayRecord`].
//! The server replays the game from `(config, seed, actions)` before accepting
//! any entry, computing `won`, `turns` and `points` directly from the verified
//! final [`isle_core::GameState`].

use std::cmp::{Ordering, Reverse};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::{RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use isle_core::Phase;
use isle_proto::account::ApiError;
use isle_proto::daily::{DailyLeaderboard, DailyScoreEntry, DailySubmitRequest};
use isle_proto::net::clean_name;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::bearer;

/// Maximum request body size for `POST /api/daily` (full `ReplayRecord`).
const MAX_DAILY_BODY: usize = 256 * 1024;
/// Maximum entries returned in `DailyLeaderboard`.
const TOP_LIMIT: usize = 50;
/// Maximum deduplicated entries retained per date key.
const MAX_STORED_PER_DATE: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredDailyEntry {
    pub player_name: String,
    pub registered: bool,
    pub won: bool,
    pub turns: u32,
    pub points: u8,
    pub submitted_at: u64,
}

pub struct DailyArchive {
    file: Option<PathBuf>,
    by_date: Mutex<HashMap<String, Vec<StoredDailyEntry>>>,
}

/// Validates strict `YYYY-MM-DD` format (length 10).
pub fn valid_date_key(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    if !b[0..4].iter().all(u8::is_ascii_digit)
        || !b[5..7].iter().all(u8::is_ascii_digit)
        || !b[8..10].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let month = (b[5] - b'0') * 10 + (b[6] - b'0');
    let day = (b[8] - b'0') * 10 + (b[9] - b'0');
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn cmp_entries(a: &StoredDailyEntry, b: &StoredDailyEntry) -> Ordering {
    b.won
        .cmp(&a.won)
        .then_with(|| a.turns.cmp(&b.turns))
        .then_with(|| b.points.cmp(&a.points))
        .then_with(|| a.submitted_at.cmp(&b.submitted_at))
}

fn is_better_run(candidate: &StoredDailyEntry, existing: &StoredDailyEntry) -> bool {
    (candidate.won, Reverse(candidate.turns), candidate.points)
        > (existing.won, Reverse(existing.turns), existing.points)
}

fn to_leaderboard(date_key: &str, entries: &[StoredDailyEntry]) -> DailyLeaderboard {
    DailyLeaderboard {
        date_key: date_key.to_owned(),
        entries: entries
            .iter()
            .take(TOP_LIMIT)
            .enumerate()
            .map(|(i, e)| DailyScoreEntry {
                rank: i as u32 + 1,
                player_name: e.player_name.clone(),
                registered: e.registered,
                won: e.won,
                turns: e.turns,
                points: e.points,
                submitted_at: e.submitted_at,
            })
            .collect(),
    }
}

impl DailyArchive {
    pub fn new(data_dir: Option<&Path>) -> DailyArchive {
        let file = data_dir.map(|d| d.join("daily.json"));
        let mut initial: HashMap<String, Vec<StoredDailyEntry>> = HashMap::new();
        if let Some(path) = &file
            && path.exists()
        {
            match std::fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
            {
                Ok(loaded) => {
                    initial = loaded;
                    for list in initial.values_mut() {
                        list.sort_by(cmp_entries);
                    }
                }
                Err(e) => tracing::warn!("could not load {}: {e}", path.display()),
            }
        }
        DailyArchive {
            file,
            by_date: Mutex::new(initial),
        }
    }

    pub fn leaderboard(&self, date_key: &str) -> DailyLeaderboard {
        let map = self.by_date.lock().unwrap_or_else(PoisonError::into_inner);
        match map.get(date_key) {
            Some(list) => to_leaderboard(date_key, list),
            None => DailyLeaderboard {
                date_key: date_key.to_owned(),
                entries: Vec::new(),
            },
        }
    }

    pub async fn submit(&self, date_key: String, entry: StoredDailyEntry) -> DailyLeaderboard {
        let canon = entry.player_name.to_lowercase();
        let (board, snapshot) = {
            let mut map = self.by_date.lock().unwrap_or_else(PoisonError::into_inner);
            let list = map.entry(date_key.clone()).or_default();
            let mut changed = false;
            if let Some(existing) = list
                .iter_mut()
                .find(|e| e.registered == entry.registered && e.player_name.to_lowercase() == canon)
            {
                if is_better_run(&entry, existing) {
                    *existing = entry;
                    changed = true;
                }
            } else {
                list.push(entry);
                changed = true;
            }
            if changed {
                list.sort_by(cmp_entries);
                list.truncate(MAX_STORED_PER_DATE);
            }
            let board = to_leaderboard(&date_key, list);
            let snapshot = if changed && self.file.is_some() {
                serde_json::to_vec(&*map).ok()
            } else {
                None
            };
            (board, snapshot)
        };
        if let (Some(file), Some(bytes)) = (self.file.clone(), snapshot) {
            let write = move || {
                if let Some(parent) = file.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let tmp = file.with_extension("json.tmp");
                if let Err(e) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &file))
                {
                    tracing::error!("could not save {}: {e}", file.display());
                }
            };
            let _ = tokio::task::spawn_blocking(write).await;
        }
        board
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/daily", get(get_daily).post(post_daily))
}

fn json_err(status: StatusCode, code: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        axum::Json(ApiError { error: code.into() }),
    )
        .into_response()
}

fn json_ok<T: Serialize>(body: &T) -> Response {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        axum::Json(body),
    )
        .into_response()
}

async fn get_daily(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Response {
    let date = query
        .as_deref()
        .unwrap_or("")
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find_map(|(k, v)| (k == "date").then_some(v));
    let Some(date) = date.filter(|d| valid_date_key(d)) else {
        return json_err(StatusCode::BAD_REQUEST, "BadDate");
    };
    json_ok(&state.registry.daily().leaderboard(date))
}

async fn post_daily(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: Request,
) -> Response {
    let Ok(body) = axum::body::to_bytes(req.into_body(), MAX_DAILY_BODY).await else {
        return json_err(StatusCode::BAD_REQUEST, "BadRequest");
    };
    let Ok(submit) = serde_json::from_slice::<DailySubmitRequest>(&body) else {
        return json_err(StatusCode::BAD_REQUEST, "BadRequest");
    };
    if !valid_date_key(&submit.date_key) {
        return json_err(StatusCode::BAD_REQUEST, "BadDate");
    }
    if submit.seat as usize >= submit.replay.config.num_players as usize {
        return json_err(StatusCode::BAD_REQUEST, "InvalidSeat");
    }
    let replay = submit.replay;
    let verified = tokio::task::spawn_blocking(move || {
        let final_state = replay.play(|_, _, _, _| {})?;
        if final_state.phase != Phase::GameOver
            || final_state.winner.is_none()
            || final_state.check_invariants().is_err()
        {
            return Err(isle_proto::ReplayError::Config);
        }
        Ok(final_state)
    })
    .await;
    let Ok(Ok(final_state)) = verified else {
        return json_err(StatusCode::BAD_REQUEST, "InvalidReplay");
    };

    let won = final_state.winner == Some(submit.seat);
    let turns = final_state.turn;
    let points = final_state.victory_points(submit.seat);

    let (player_name, registered) = match (&state.accounts, bearer(&headers)) {
        (Some(accounts), Some(token)) => match accounts.session(token).await {
            Ok(Some(session)) => (session.account.username, true),
            _ => (
                clean_name(&submit.player_name).unwrap_or_else(|| "Settler".into()),
                false,
            ),
        },
        _ => (
            clean_name(&submit.player_name).unwrap_or_else(|| "Settler".into()),
            false,
        ),
    };

    let submitted_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let board = state
        .registry
        .daily()
        .submit(
            submit.date_key,
            StoredDailyEntry {
                player_name,
                registered,
                won,
                turns,
                points,
                submitted_at,
            },
        )
        .await;
    json_ok(&board)
}
