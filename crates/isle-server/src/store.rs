//! Room persistence. A room is stored as its seed, seats and action log;
//! the deterministic engine rebuilds the exact game state by replay.

use std::io;
use std::path::PathBuf;

use isle_core::Action;
use isle_proto::net::{BotLevel, HouseRules};
use isle_proto::ranking::SeatRating;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoomRecord {
    pub format: u32,
    pub id: String,
    pub seed: u64,
    pub bot_level: BotLevel,
    pub host: u8,
    pub seats: Vec<SeatRecord>,
    pub started: bool,
    /// Every action applied so far, with the acting seat, in order.
    pub actions: Vec<(u8, Action)>,
    /// Replay id once the game is over.
    #[serde(default)]
    pub replay: Option<String>,
    /// Ranked room (absent in records from before ranking: casual).
    #[serde(default)]
    pub ranked: bool,
    /// Absent in records from before seafaring: classic.
    #[serde(default)]
    pub scenario: isle_core::Scenario,
    #[serde(default)]
    pub rules: HouseRules,
}

impl RoomRecord {
    pub const FORMAT: u32 = 1;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SeatRecord {
    Open,
    Human {
        name: String,
        token: String,
        /// The signed-in account holding the seat (absent for guests and in
        /// records written before accounts existed).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<SeatAccount>,
    },
    Bot {
        level: BotLevel,
    },
}

/// An account bound to a seat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatAccount {
    pub id: i64,
    pub username: String,
    /// Rating when they sat down, refreshed after a ranked game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<SeatRating>,
}

/// One JSON file per room in `dir`, replaced atomically on every save.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

impl Store {
    pub fn open(dir: PathBuf) -> Store {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!("cannot create data dir {}: {e}", dir.display());
        }
        Store { dir }
    }

    /// Reads every stored room; unreadable files are skipped with a warning.
    pub fn load_all(&self) -> Vec<RoomRecord> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            match std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    serde_json::from_slice::<RoomRecord>(&bytes).map_err(|e| e.to_string())
                }) {
                Ok(r) if r.format == RoomRecord::FORMAT && valid_id(&r.id) => out.push(r),
                Ok(r) => tracing::warn!(
                    "skipping {}: unsupported format {}",
                    path.display(),
                    r.format
                ),
                Err(e) => tracing::warn!("skipping {}: {e}", path.display()),
            }
        }
        out
    }

    pub async fn save(&self, record: RoomRecord) -> io::Result<()> {
        if !valid_id(&record.id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad room id"));
        }
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || {
            let bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
            let tmp = dir.join(format!("{}.json.tmp", record.id));
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, dir.join(format!("{}.json", record.id)))
        })
        .await
        .map_err(io::Error::other)?
    }

    pub async fn delete(&self, id: &str) {
        if !valid_id(id) {
            return;
        }
        let path = self.dir.join(format!("{id}.json"));
        if let Err(e) = tokio::fs::remove_file(&path).await
            && e.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!("cannot delete {}: {e}", path.display());
        }
    }
}

/// Finished games, kept so they can be watched again. Recent replays stay in
/// memory; with a data directory every replay is also written to
/// `<data dir>/replays/<id>.json`.
pub struct ReplayArchive {
    dir: Option<PathBuf>,
    recent: std::sync::Mutex<RecentReplays>,
}

#[derive(Default)]
struct RecentReplays {
    order: std::collections::VecDeque<String>,
    json: std::collections::HashMap<String, std::sync::Arc<str>>,
}

/// Replays kept in memory.
const RECENT_REPLAYS: usize = 256;

impl ReplayArchive {
    pub fn new(data_dir: Option<&std::path::Path>) -> ReplayArchive {
        let dir = data_dir.map(|d| d.join("replays"));
        if let Some(dir) = &dir
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            tracing::error!("cannot create replay dir {}: {e}", dir.display());
        }
        ReplayArchive {
            dir,
            recent: std::sync::Mutex::new(RecentReplays::default()),
        }
    }

    pub fn valid_id(id: &str) -> bool {
        valid_id(id)
    }

    /// Stores a replay under `id` (memory now, disk in the background).
    pub fn put(&self, id: &str, record: &isle_proto::ReplayRecord) {
        if !valid_id(id) {
            return;
        }
        let json: std::sync::Arc<str> = serde_json::to_string(record).expect("serializable").into();
        {
            let mut recent = self.recent.lock().expect("replay lock");
            if recent.json.insert(id.to_owned(), json.clone()).is_none() {
                recent.order.push_back(id.to_owned());
            }
            while recent.order.len() > RECENT_REPLAYS {
                if let Some(old) = recent.order.pop_front() {
                    recent.json.remove(&old);
                }
            }
        }
        if let Some(dir) = self.dir.clone() {
            let id = id.to_owned();
            let write = move || {
                let tmp = dir.join(format!("{id}.json.tmp"));
                let result = std::fs::write(&tmp, json.as_bytes())
                    .and_then(|_| std::fs::rename(&tmp, dir.join(format!("{id}.json"))));
                if let Err(e) = result {
                    tracing::error!(replay = %id, "could not save replay: {e}");
                }
            };
            match tokio::runtime::Handle::try_current() {
                Ok(rt) => drop(rt.spawn_blocking(write)),
                Err(_) => write(),
            }
        }
    }

    /// The replay's JSON, if known.
    pub async fn get(&self, id: &str) -> Option<std::sync::Arc<str>> {
        if !valid_id(id) {
            return None;
        }
        if let Some(json) = self.recent.lock().expect("replay lock").json.get(id) {
            return Some(json.clone());
        }
        let path = self.dir.as_ref()?.join(format!("{id}.json"));
        let text = tokio::fs::read_to_string(path).await.ok()?;
        Some(text.into())
    }
}
