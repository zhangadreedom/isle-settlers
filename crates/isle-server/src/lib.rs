//! # isle-server
//!
//! Online play over WebSockets. One tokio task (actor) per room owns the
//! authoritative [`isle_core::GameState`]; every connection receives views
//! and events projected for its seat (see `isle-proto`). Rooms are joined by
//! invite link (room id); a per-seat token lets a player reconnect. Bots fill
//! empty seats and play for disconnected players ("autopilot"). With a data
//! directory, rooms survive restarts: each is stored as its seed plus action
//! log and rebuilt by replay. Finished games are kept as replays, served at
//! `/api/replays/{id}`.

mod registry;
mod room;
mod store;
mod ws;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::routing::get;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

pub use registry::Registry;
pub use store::ReplayArchive;

/// Server tuning. `Default` is production-like; tests shorten the timings.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Multiplier for bot "thinking" delays (0 = act immediately).
    pub bot_delay_scale: f64,
    /// A bot plays for a human after they have been disconnected this long.
    pub autopilot_after: Duration,
    /// Rooms without connected players are closed after these idle times.
    pub idle_ttl_lobby: Duration,
    pub idle_ttl_game: Duration,
    pub idle_ttl_finished: Duration,
    pub max_rooms: usize,
    /// Bot used for the hard level (`isle_ai::bot_by_name` syntax), e.g. a
    /// smaller search budget on slow machines or in tests.
    pub hard_bot: String,
    /// Where rooms are persisted; `None` keeps them in memory only.
    pub data_dir: Option<PathBuf>,
    /// Built web client (`web/dist`) served at `/`.
    pub static_dir: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bot_delay_scale: 1.0,
            autopilot_after: Duration::from_secs(30),
            idle_ttl_lobby: Duration::from_secs(15 * 60),
            idle_ttl_game: Duration::from_secs(60 * 60),
            idle_ttl_finished: Duration::from_secs(10 * 60),
            max_rooms: 10_000,
            hard_bot: "mcts".into(),
            data_dir: None,
            static_dir: None,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<Registry>,
    started: Instant,
}

impl AppState {
    /// Creates the state and restores persisted rooms.
    pub fn new(config: ServerConfig) -> AppState {
        AppState {
            registry: Registry::new(config),
            started: Instant::now(),
        }
    }
}

/// HTTP routes: `/api/ws` (game protocol), `/api/health`, and the web client.
pub fn app(state: AppState) -> Router {
    let mut router = Router::new()
        .route("/api/ws", get(ws::upgrade))
        .route("/api/health", get(health))
        .route("/api/replays/{id}", get(replay))
        .with_state(state.clone());
    if let Some(dir) = &state.registry.config().static_dir {
        // Hashed assets never change; everything else is revalidated.
        let assets = Router::new()
            .fallback_service(ServeDir::new(dir.join("assets")))
            .layer(SetResponseHeaderLayer::overriding(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ));
        let site = ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")));
        router = router.nest("/assets", assets).fallback_service(site);
    }
    router.layer(CompressionLayer::new())
}

/// A finished game as a `ReplayRecord`. Seat tokens are never part of it.
async fn replay(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match state.registry.replays().get(&id.to_ascii_uppercase()).await {
        Some(json) => (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                ),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=86400, immutable"),
                ),
            ],
            json.to_string(),
        )
            .into_response(),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

async fn health(State(state): State<AppState>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "ok": true,
        "rooms": state.registry.room_count(),
        "uptimeSecs": state.started.elapsed().as_secs(),
    }))
}

/// Random identifiers from the OS RNG.
pub(crate) mod ids {
    /// Unambiguous characters (no 0/O, 1/I/L) for ids people may type.
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";

    pub fn room_id() -> String {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("OS randomness");
        bytes
            .iter()
            .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
            .collect()
    }

    /// 128-bit secret, hex encoded.
    pub fn token() -> String {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).expect("OS randomness");
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn seed() -> u64 {
        getrandom::u64().expect("OS randomness")
    }
}
