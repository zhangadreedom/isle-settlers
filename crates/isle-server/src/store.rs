//! Room persistence. A room is stored as its seed, seats and action log;
//! the deterministic engine rebuilds the exact game state by replay.

use std::io;
use std::path::PathBuf;

use isle_core::Action;
use isle_proto::net::BotLevel;
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
}

impl RoomRecord {
    pub const FORMAT: u32 = 1;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SeatRecord {
    Open,
    Human { name: String, token: String },
    Bot { level: BotLevel },
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
