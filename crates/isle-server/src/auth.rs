//! Account HTTP API (bodies in `isle_proto::account`).
//!
//! Sessions are bearer tokens: the client keeps the token and sends it in
//! `Authorization: Bearer …`, and with `Create` / `Join` on the WebSocket.
//! No cookies, so there is nothing for another site to ride on (CSRF).
//!
//! Password hashing (Argon2id, deliberately slow and memory hungry) runs on
//! the blocking pool with at most half the cores at a time, and register /
//! login attempts are rate limited per client address (IPv6 per /64).
//!
//! Also serves the public ranking pages: `GET /api/leaderboard` and
//! `GET /api/players/{username}`.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::extract::{ConnectInfo, Path, RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use isle_proto::account::{AccountInfo, ApiError, Credentials, SessionGrant, SessionInfo};
use isle_proto::ranking::{RatingChange, SeatRating};
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::time::Instant;

use crate::account_store::{
    Account, AccountStore, AccountStoreConfig, AccountStoreError, NewSession, Session,
};
use crate::ranking::MatchEntry;
use crate::store::SeatAccount;
use crate::{AppState, ServerConfig};

/// Largest accepted request body (credentials are a few hundred bytes).
const MAX_BODY: usize = 4 * 1024;
/// How long a request waits for a hashing slot before giving up with `Busy`.
const HASH_WAIT: Duration = Duration::from_secs(10);
/// Rate-limit buckets kept before idle ones are dropped.
const MAX_BUCKETS: usize = 4096;

/// The account service behind the routes and the WebSocket handshake.
pub struct Accounts {
    store: Arc<AccountStore>,
    hashing: Arc<Semaphore>,
    limiter: RateLimiter,
    ip_header: Option<HeaderName>,
    placement_games: u32,
}

/// Leaderboard page size limits.
const DEFAULT_PAGE: u32 = 50;
const MAX_PAGE: u32 = 100;
/// Ranked games listed on a profile.
const PROFILE_RECENT: u32 = 20;

impl Accounts {
    /// Opens the store in the data directory. `Ok(None)` when accounts are
    /// off: no data directory, or disabled in the config.
    pub fn open(config: &ServerConfig) -> Result<Option<Accounts>, String> {
        let Some(dir) = config.data_dir.as_ref().filter(|_| config.accounts) else {
            return Ok(None);
        };
        let ip_header = config
            .client_ip_header
            .as_deref()
            .map(HeaderName::try_from)
            .transpose()
            .map_err(|e| format!("bad client IP header name: {e}"))?;
        let store = AccountStore::open_with(
            dir,
            AccountStoreConfig {
                session_ttl: config.session_ttl,
            },
        )
        .map_err(|e| format!("cannot open the account store in {}: {e}", dir.display()))?;
        let slots = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1));
        Ok(Some(Accounts {
            store: Arc::new(store),
            hashing: Arc::new(Semaphore::new(slots)),
            limiter: RateLimiter::new(config.auth_burst, config.auth_refill),
            ip_header,
            placement_games: config.placement_games,
        }))
    }

    /// The session behind `token`; `Ok(None)` if unknown, expired or revoked.
    pub async fn session(&self, token: String) -> Result<Option<Session>, AccountStoreError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || store.validate_session(&token))
            .await
            .unwrap_or_else(|e| Err(AccountStoreError::Corrupt(format!("task failed: {e}"))))
    }

    /// The account behind `token` with its current rating, for seating a
    /// player; `Ok(None)` if the session is unknown, expired or revoked.
    pub async fn seat_account(
        &self,
        token: String,
    ) -> Result<Option<SeatAccount>, AccountStoreError> {
        let store = self.store.clone();
        let placement = self.placement_games;
        tokio::task::spawn_blocking(move || {
            let Some(session) = store.validate_session(&token)? else {
                return Ok(None);
            };
            let rating = store.seat_rating(session.account.id, placement)?;
            Ok(Some(SeatAccount {
                id: session.account.id,
                username: session.account.username,
                rating: Some(rating),
            }))
        })
        .await
        .unwrap_or_else(|e| Err(AccountStoreError::Corrupt(format!("task failed: {e}"))))
    }

    /// Rates a finished ranked game once (see
    /// [`AccountStore::record_match`]); returns each player's change and new
    /// seat rating.
    pub async fn record_match(
        &self,
        id: String,
        seats: u8,
        entries: Vec<MatchEntry>,
    ) -> Result<Vec<(RatingChange, SeatRating)>, AccountStoreError> {
        let store = self.store.clone();
        let placement = self.placement_games;
        tokio::task::spawn_blocking(move || {
            let changes = store.record_match(&id, seats, &entries)?;
            changes
                .into_iter()
                .map(|c| {
                    let account = entries
                        .iter()
                        .find(|e| e.seat == c.seat)
                        .map(|e| e.account_id)
                        .ok_or_else(|| AccountStoreError::Corrupt("match seat mismatch".into()))?;
                    let rating = store.seat_rating(account, placement)?;
                    Ok((c, rating))
                })
                .collect()
        })
        .await
        .unwrap_or_else(|e| Err(AccountStoreError::Corrupt(format!("task failed: {e}"))))
    }

    /// Runs a quick store operation on the blocking pool (SQLite with
    /// `synchronous = FULL` waits for the disk on every write).
    async fn blocking<T, F>(&self, f: F) -> Result<T, Fail>
    where
        F: FnOnce(&AccountStore) -> Result<T, AccountStoreError> + Send + 'static,
        T: Send + 'static,
    {
        let store = self.store.clone();
        match tokio::task::spawn_blocking(move || f(&store)).await {
            Ok(result) => result.map_err(Fail::from_store),
            Err(e) => {
                tracing::error!("account task failed: {e}");
                Err(Fail::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal"))
            }
        }
    }

    /// Like [`Accounts::blocking`] for operations that hash a password: waits
    /// for a hashing slot, held until the work is done even if the client
    /// goes away meanwhile.
    async fn hashing<T, F>(&self, f: F) -> Result<T, Fail>
    where
        F: FnOnce(&AccountStore) -> Result<T, AccountStoreError> + Send + 'static,
        T: Send + 'static,
    {
        let permit = tokio::time::timeout(HASH_WAIT, self.hashing.clone().acquire_owned())
            .await
            .map_err(|_| Fail::new(StatusCode::SERVICE_UNAVAILABLE, "Busy"))?
            .map_err(|_| Fail::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal"))?;
        self.blocking(move |store| {
            let result = f(store);
            drop(permit);
            result
        })
        .await
    }

    /// Spends one register / login attempt of the client's address.
    fn limit(&self, req: &Request) -> Result<(), Fail> {
        let ip = self.client_ip(req);
        self.limiter
            .take(ip, Instant::now())
            .map_err(|retry_after| Fail {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "TooManyAttempts",
                retry_after: Some(retry_after),
            })
    }

    /// The peer address, or with a configured header (set by a trusted
    /// reverse proxy, e.g. `X-Forwarded-For`) the last address in it.
    fn client_ip(&self, req: &Request) -> IpAddr {
        if let Some(name) = &self.ip_header
            && let Some(ip) = req
                .headers()
                .get_all(name)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .flat_map(|v| v.split(','))
                .filter_map(|s| {
                    let s = s.trim();
                    s.parse::<IpAddr>()
                        .ok()
                        .or_else(|| s.parse::<SocketAddr>().ok().map(|a| a.ip()))
                })
                .next_back()
        {
            return ip;
        }
        req.extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::from([0, 0, 0, 0]), |c| c.0.ip())
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/accounts", post(register))
        .route("/api/sessions", post(login).delete(logout_all))
        .route("/api/session", get(current).delete(logout))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/players/{username}", get(profile))
}

/// `GET /api/leaderboard?limit=&offset=`: placed players, best first.
async fn leaderboard(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Response, Fail> {
    let accounts = enabled(&state)?;
    let (mut limit, mut offset) = (DEFAULT_PAGE, 0u32);
    for (key, value) in query
        .as_deref()
        .unwrap_or("")
        .split('&')
        .filter_map(|kv| kv.split_once('='))
    {
        let bad = Fail::new(StatusCode::BAD_REQUEST, "BadRequest");
        match key {
            "limit" => limit = value.parse().map_err(|_| bad)?,
            "offset" => offset = value.parse().map_err(|_| bad)?,
            _ => {}
        }
    }
    let limit = limit.clamp(1, MAX_PAGE);
    let placement = accounts.placement_games;
    let board = accounts
        .blocking(move |store| store.leaderboard(placement, limit, offset))
        .await?;
    Ok(json(StatusCode::OK, &board))
}

/// `GET /api/players/{username}`: rating, rank and recent ranked games.
async fn profile(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Response, Fail> {
    let accounts = enabled(&state)?;
    let placement = accounts.placement_games;
    accounts
        .blocking(move |store| store.profile(&username, placement, PROFILE_RECENT))
        .await?
        .map(|p| json(StatusCode::OK, &p))
        .ok_or(Fail::new(StatusCode::NOT_FOUND, "PlayerNotFound"))
}

/// `POST /api/accounts`: create an account and log it in.
async fn register(State(state): State<AppState>, req: Request) -> Result<Response, Fail> {
    let accounts = enabled(&state)?;
    accounts.limit(&req)?;
    let c = credentials(req).await?;
    let new = accounts
        .hashing(move |store| store.register_session(&c.username, &c.password))
        .await?;
    tracing::info!(account = new.session.account.id, "account registered");
    Ok(json(StatusCode::CREATED, &grant(new)))
}

/// `POST /api/sessions`: log in.
async fn login(State(state): State<AppState>, req: Request) -> Result<Response, Fail> {
    let accounts = enabled(&state)?;
    accounts.limit(&req)?;
    let c = credentials(req).await?;
    let new = accounts
        .hashing(move |store| store.login(&c.username, &c.password))
        .await?;
    Ok(json(StatusCode::OK, &grant(new)))
}

/// `GET /api/session`: who the token belongs to.
async fn current(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, Fail> {
    let session = valid_session(&state, &headers).await?;
    Ok(json(
        StatusCode::OK,
        &SessionInfo {
            account: info(&session.account),
            expires_at: session.expires_at,
        },
    ))
}

/// `DELETE /api/session`: log this token out. Succeeds for unknown tokens.
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, Fail> {
    let accounts = enabled(&state)?;
    if let Some(token) = bearer(&headers) {
        accounts.blocking(move |store| store.logout(&token)).await?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `DELETE /api/sessions`: log out every session of the token's account.
async fn logout_all(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, Fail> {
    let session = valid_session(&state, &headers).await?;
    let accounts = enabled(&state)?;
    let id = session.account.id;
    let n = accounts.blocking(move |store| store.logout_all(id)).await?;
    tracing::info!(account = id, sessions = n, "logged out everywhere");
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn enabled(state: &AppState) -> Result<&Arc<Accounts>, Fail> {
    state.accounts.as_ref().ok_or(Fail::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "AccountsDisabled",
    ))
}

async fn valid_session(state: &AppState, headers: &HeaderMap) -> Result<Session, Fail> {
    let accounts = enabled(state)?;
    let not_logged_in = Fail::new(StatusCode::UNAUTHORIZED, "NotLoggedIn");
    let token = bearer(headers).ok_or(not_logged_in)?;
    accounts
        .blocking(move |store| store.validate_session(&token))
        .await?
        .ok_or(not_logged_in)
}

/// The token of an `Authorization: Bearer <token>` header.
fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then(|| token.to_owned())
}

async fn credentials(req: Request) -> Result<Credentials, Fail> {
    let bad = Fail::new(StatusCode::BAD_REQUEST, "BadRequest");
    let body = axum::body::to_bytes(req.into_body(), MAX_BODY)
        .await
        .map_err(|_| bad)?;
    serde_json::from_slice(&body).map_err(|_| bad)
}

fn info(account: &Account) -> AccountInfo {
    AccountInfo {
        username: account.username.clone(),
        created_at: account.created_at,
    }
}

fn grant(new: NewSession) -> SessionGrant {
    SessionGrant {
        account: info(&new.session.account),
        token: new.token.expose().to_owned(),
        expires_at: new.session.expires_at,
    }
}

/// JSON response that no cache may keep (it may hold a token).
fn json<T: Serialize>(status: StatusCode, body: &T) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        axum::Json(body),
    )
        .into_response()
}

/// A failed request: status plus an [`ApiError`] code.
#[derive(Clone, Copy, Debug)]
struct Fail {
    status: StatusCode,
    code: &'static str,
    retry_after: Option<u64>,
}

impl Fail {
    const fn new(status: StatusCode, code: &'static str) -> Fail {
        Fail {
            status,
            code,
            retry_after: None,
        }
    }

    fn from_store(e: AccountStoreError) -> Fail {
        use AccountStoreError as E;
        match e {
            E::InvalidUsername(_) => Fail::new(StatusCode::BAD_REQUEST, "InvalidUsername"),
            E::PasswordTooShort => Fail::new(StatusCode::BAD_REQUEST, "PasswordTooShort"),
            E::PasswordTooLong => Fail::new(StatusCode::BAD_REQUEST, "PasswordTooLong"),
            E::UsernameTaken => Fail::new(StatusCode::CONFLICT, "UsernameTaken"),
            E::InvalidCredentials => Fail::new(StatusCode::UNAUTHORIZED, "InvalidCredentials"),
            other => {
                tracing::error!("account store: {other}");
                Fail::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal")
            }
        }
    }
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        let mut res = json(
            self.status,
            &ApiError {
                error: self.code.into(),
            },
        );
        if let Some(secs) = self.retry_after {
            res.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(secs));
        }
        res
    }
}

/// Token buckets per client address: `burst` attempts, refilled one per
/// `refill`. A burst of 0 turns limiting off.
struct RateLimiter {
    burst: f64,
    refill: Duration,
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
}

#[derive(Clone, Copy, Debug)]
struct Bucket {
    tokens: f64,
    at: Instant,
}

impl RateLimiter {
    fn new(burst: u32, refill: Duration) -> RateLimiter {
        RateLimiter {
            burst: f64::from(burst),
            refill,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn level(&self, b: Bucket, now: Instant) -> f64 {
        if self.refill.is_zero() {
            return self.burst;
        }
        let regained = now.duration_since(b.at).as_secs_f64() / self.refill.as_secs_f64();
        (b.tokens + regained).min(self.burst)
    }

    /// Takes one attempt for `ip`; when none is left, returns the seconds
    /// until the next one.
    fn take(&self, ip: IpAddr, now: Instant) -> Result<(), u64> {
        if self.burst == 0.0 {
            return Ok(());
        }
        let key = bucket_key(ip);
        let mut map = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        if map.len() >= MAX_BUCKETS && !map.contains_key(&key) {
            // Full buckets carry no information.
            map.retain(|_, b| self.level(*b, now) < self.burst);
            if map.len() >= 4 * MAX_BUCKETS {
                map.clear();
            }
        }
        let b = map.entry(key).or_insert(Bucket {
            tokens: self.burst,
            at: now,
        });
        let tokens = self.level(*b, now);
        if tokens >= 1.0 {
            *b = Bucket {
                tokens: tokens - 1.0,
                at: now,
            };
            Ok(())
        } else {
            *b = Bucket { tokens, at: now };
            let wait = (1.0 - tokens) * self.refill.as_secs_f64();
            Err(wait.ceil().max(1.0) as u64)
        }
    }
}

/// IPv4 addresses count individually; IPv6 per /64, which one subscriber
/// usually holds whole.
fn bucket_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_refill_over_time() {
        let limiter = RateLimiter::new(3, Duration::from_secs(10));
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        let t0 = Instant::now();
        for _ in 0..3 {
            assert_eq!(limiter.take(ip, t0), Ok(()));
        }
        assert_eq!(limiter.take(ip, t0), Err(10));
        assert_eq!(limiter.take(ip, t0 + Duration::from_secs(4)), Err(6));
        assert_eq!(limiter.take(ip, t0 + Duration::from_secs(11)), Ok(()));
        // Other addresses are unaffected.
        assert_eq!(limiter.take("203.0.113.8".parse().unwrap(), t0), Ok(()));
    }

    #[test]
    fn ipv6_is_limited_per_slash_64() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let t0 = Instant::now();
        assert_eq!(limiter.take("2001:db8:1:2::1".parse().unwrap(), t0), Ok(()));
        assert!(
            limiter
                .take("2001:db8:1:2:ffff::9".parse().unwrap(), t0)
                .is_err()
        );
        assert_eq!(limiter.take("2001:db8:1:3::1".parse().unwrap(), t0), Ok(()));
        // IPv4-mapped addresses share the IPv4 bucket.
        assert_eq!(limiter.take("198.51.100.1".parse().unwrap(), t0), Ok(()));
        assert!(
            limiter
                .take("::ffff:198.51.100.1".parse().unwrap(), t0)
                .is_err()
        );
    }

    #[test]
    fn zero_burst_disables_limiting() {
        let limiter = RateLimiter::new(0, Duration::from_secs(60));
        let t0 = Instant::now();
        for _ in 0..100 {
            assert_eq!(limiter.take(IpAddr::from([127, 0, 0, 1]), t0), Ok(()));
        }
    }

    #[test]
    fn bearer_tokens_are_parsed() {
        let mut h = HeaderMap::new();
        assert_eq!(bearer(&h), None);
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(bearer(&h).as_deref(), Some("abc"));
        h.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer  xyz "),
        );
        assert_eq!(bearer(&h).as_deref(), Some("xyz"));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc"));
        assert_eq!(bearer(&h), None);
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert_eq!(bearer(&h), None);
    }
}
