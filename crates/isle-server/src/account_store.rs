//! Persistent player accounts and login sessions.
//!
//! Storage is a single SQLite database, `<dir>/accounts.sqlite3`, opened in
//! WAL mode with `synchronous=FULL`: every change is one atomic, durable
//! transaction (also on Windows). The store never keeps secrets in plain
//! text:
//!
//! * passwords are stored only as Argon2id PHC strings (see [`accounts`]);
//! * session tokens are 32 random bytes from the OS RNG, handed to the
//!   client hex encoded; only their SHA-256 digest is stored.
//!
//! Usernames are canonicalized with [`accounts::normalize_username`]
//! (lowercase), so uniqueness is case-insensitive.
//!
//! A database that is not a valid isle-settlers account store (not SQLite,
//! failed integrity check, foreign or newer schema) is refused with an error
//! and left untouched; it is never re-initialized or overwritten.
//!
//! All methods block (Argon2 and disk I/O); call them from
//! `tokio::task::spawn_blocking` in async code.

use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ErrorCode, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::accounts::{self, CredentialError, MAX_PASSWORD_LEN, UsernameError};

/// File name of the database inside the configured directory.
pub const DB_FILE_NAME: &str = "accounts.sqlite3";
/// Minimum password length in bytes, enforced at registration.
pub const MIN_PASSWORD_LEN: usize = 12;
/// Session token entropy in bytes.
pub const TOKEN_BYTES: usize = 32;
/// Default session lifetime.
pub const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Longest session lifetime accepted in the configuration.
pub const MAX_SESSION_TTL: Duration = Duration::from_secs(366 * 24 * 60 * 60);

/// `PRAGMA application_id` marking our databases ("ISLE").
const APPLICATION_ID: i32 = 0x4953_4C45;
/// `PRAGMA user_version` of the current schema: 1 = accounts and sessions,
/// 2 = adds ratings and ranked match history. Older versions are migrated
/// in place when opened.
const SCHEMA_VERSION: i32 = 2;
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

const SCHEMA: &str = "
CREATE TABLE accounts (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE
                  -- The column's NOCASE collation would make this check
                  -- always true, so it compares bytes explicitly.
                  CHECK (length(username) BETWEEN 1 AND 32
                         AND username = lower(username) COLLATE BINARY),
    password_hash TEXT    NOT NULL CHECK (password_hash LIKE '$argon2id$%'),
    created_at    INTEGER NOT NULL
) STRICT;
CREATE TABLE sessions (
    token_hash BLOB    NOT NULL PRIMARY KEY CHECK (length(token_hash) = 32),
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL CHECK (expires_at > created_at)
) STRICT, WITHOUT ROWID;
CREATE INDEX sessions_account ON sessions(account_id);
CREATE INDEX sessions_expiry ON sessions(expires_at);
";

/// Version 2: ratings (see `rating`) and ranked games.
const SCHEMA_V2: &str = "
CREATE TABLE ratings (
    account_id INTEGER NOT NULL PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    mu         REAL    NOT NULL,
    sigma      REAL    NOT NULL CHECK (sigma > 0),
    games      INTEGER NOT NULL CHECK (games >= 0),
    wins       INTEGER NOT NULL CHECK (wins BETWEEN 0 AND games),
    updated_at INTEGER NOT NULL
) STRICT;
CREATE TABLE matches (
    id          TEXT    NOT NULL PRIMARY KEY,
    finished_at INTEGER NOT NULL,
    seats       INTEGER NOT NULL CHECK (seats BETWEEN 2 AND 6)
) STRICT;
CREATE TABLE match_players (
    match_id   TEXT    NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    seat       INTEGER NOT NULL CHECK (seat BETWEEN 0 AND 5),
    place      INTEGER NOT NULL CHECK (place BETWEEN 1 AND 6),
    points     INTEGER NOT NULL CHECK (points >= 0),
    before     INTEGER NOT NULL,
    after      INTEGER NOT NULL,
    PRIMARY KEY (match_id, account_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX match_players_account ON match_players(account_id);
CREATE INDEX matches_finished ON matches(finished_at);
";

#[derive(Debug)]
pub enum AccountStoreError {
    InvalidConfig(String),
    /// A game result that cannot be recorded (see `ranking`).
    InvalidMatch(String),
    InvalidUsername(UsernameError),
    PasswordTooShort,
    PasswordTooLong,
    UsernameTaken,
    /// Unknown username or wrong password (deliberately indistinguishable).
    InvalidCredentials,
    /// The OS random number generator failed.
    Rng,
    /// The database file or its contents are damaged.
    Corrupt(String),
    /// A valid SQLite file that is not (this version of) our store.
    Incompatible(String),
    Credential(CredentialError),
    Io(io::Error),
    Db(rusqlite::Error),
}

impl fmt::Display for AccountStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountStoreError::InvalidConfig(m) => write!(f, "invalid account store config: {m}"),
            AccountStoreError::InvalidMatch(m) => write!(f, "invalid match result: {m}"),
            AccountStoreError::InvalidUsername(e) => write!(f, "{e}"),
            AccountStoreError::PasswordTooShort => {
                write!(f, "password is shorter than {MIN_PASSWORD_LEN} bytes")
            }
            AccountStoreError::PasswordTooLong => {
                write!(f, "password is longer than {MAX_PASSWORD_LEN} bytes")
            }
            AccountStoreError::UsernameTaken => write!(f, "username is already taken"),
            AccountStoreError::InvalidCredentials => write!(f, "invalid username or password"),
            AccountStoreError::Rng => write!(f, "OS randomness unavailable"),
            AccountStoreError::Corrupt(m) => write!(f, "account storage is corrupted: {m}"),
            AccountStoreError::Incompatible(m) => write!(f, "incompatible account storage: {m}"),
            AccountStoreError::Credential(e) => write!(f, "{e}"),
            AccountStoreError::Io(e) => write!(f, "account storage I/O error: {e}"),
            AccountStoreError::Db(e) => write!(f, "account database error: {e}"),
        }
    }
}

impl std::error::Error for AccountStoreError {}

impl From<io::Error> for AccountStoreError {
    fn from(e: io::Error) -> Self {
        AccountStoreError::Io(e)
    }
}

impl From<rusqlite::Error> for AccountStoreError {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) => {
                AccountStoreError::Corrupt(e.to_string())
            }
            _ => AccountStoreError::Db(e),
        }
    }
}

impl From<CredentialError> for AccountStoreError {
    fn from(e: CredentialError) -> Self {
        match e {
            CredentialError::Rng => AccountStoreError::Rng,
            CredentialError::PasswordTooLong => AccountStoreError::PasswordTooLong,
            other => AccountStoreError::Credential(other),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AccountStoreConfig {
    /// How long a session stays valid after login.
    pub session_ttl: Duration,
}

impl Default for AccountStoreConfig {
    fn default() -> Self {
        AccountStoreConfig {
            session_ttl: DEFAULT_SESSION_TTL,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    /// Canonical (lowercase) username.
    pub username: String,
    /// Unix seconds.
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub account: Account,
    /// Unix seconds; the session is invalid from this instant on.
    pub expires_at: i64,
}

/// A bearer secret for the client. `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// The token text (64 lowercase hex characters) to send to the client.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

/// Result of a successful login.
#[derive(Clone, Debug)]
pub struct NewSession {
    pub token: SessionToken,
    pub session: Session,
}

type Hasher = fn(&str) -> Result<String, CredentialError>;

pub struct AccountStore {
    path: PathBuf,
    conn: Mutex<Connection>,
    ttl_secs: i64,
    hasher: Hasher,
    /// Hash verified for unknown usernames so login timing does not reveal
    /// whether an account exists.
    dummy_hash: OnceLock<Option<String>>,
}

impl fmt::Debug for AccountStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountStore")
            .field("path", &self.path)
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

impl AccountStore {
    /// Opens (or creates) the store in `dir` with default settings.
    pub fn open(dir: &Path) -> Result<AccountStore, AccountStoreError> {
        AccountStore::open_with(dir, AccountStoreConfig::default())
    }

    pub fn open_with(
        dir: &Path,
        config: AccountStoreConfig,
    ) -> Result<AccountStore, AccountStoreError> {
        AccountStore::open_inner(dir, config, accounts::hash_password)
    }

    fn open_inner(
        dir: &Path,
        config: AccountStoreConfig,
        hasher: Hasher,
    ) -> Result<AccountStore, AccountStoreError> {
        if config.session_ttl < Duration::from_secs(1) || config.session_ttl > MAX_SESSION_TTL {
            return Err(AccountStoreError::InvalidConfig(format!(
                "session_ttl must be between 1 s and {} s",
                MAX_SESSION_TTL.as_secs()
            )));
        }
        std::fs::create_dir_all(dir)?;
        let path = dir.join(DB_FILE_NAME);
        check_file(&path)?;
        let mut conn = Connection::open(&path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        prepare_database(&mut conn)?;
        Ok(AccountStore {
            path,
            conn: Mutex::new(conn),
            ttl_secs: config.session_ttl.as_secs() as i64,
            hasher,
            dummy_hash: OnceLock::new(),
        })
    }

    /// A store that hashes with minimal Argon2 cost, for fast tests.
    #[cfg(test)]
    pub(crate) fn open_for_tests(dir: &Path) -> AccountStore {
        AccountStore::open_inner(dir, AccountStoreConfig::default(), tests::cheap_hash)
            .expect("open store")
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        // SQLite rolls back unfinished transactions, so the connection is
        // still consistent after a panic elsewhere.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Creates an account. The username is canonicalized; the password must
    /// be `MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN` bytes.
    pub fn register(&self, username: &str, password: &str) -> Result<Account, AccountStoreError> {
        self.register_at(username, password, now_secs())
    }

    fn register_at(
        &self,
        username: &str,
        password: &str,
        now: i64,
    ) -> Result<Account, AccountStoreError> {
        let username =
            accounts::normalize_username(username).map_err(AccountStoreError::InvalidUsername)?;
        if password.len() < MIN_PASSWORD_LEN {
            return Err(AccountStoreError::PasswordTooShort);
        }
        if password.len() > MAX_PASSWORD_LEN {
            return Err(AccountStoreError::PasswordTooLong);
        }
        // Hash before taking the lock: Argon2 is deliberately slow.
        let hash = (self.hasher)(password)?;
        let conn = self.lock();
        match conn.execute(
            "INSERT INTO accounts (username, password_hash, created_at) VALUES (?1, ?2, ?3)",
            params![username, hash, now],
        ) {
            Ok(_) => Ok(Account {
                id: conn.last_insert_rowid(),
                username,
                created_at: now,
            }),
            Err(e) if is_unique_violation(&e) => Err(AccountStoreError::UsernameTaken),
            Err(e) => Err(e.into()),
        }
    }

    /// Creates an account and starts its first session, hashing the password
    /// once (registering and then logging in would hash it twice).
    pub fn register_session(
        &self,
        username: &str,
        password: &str,
    ) -> Result<NewSession, AccountStoreError> {
        let now = now_secs();
        let account = self.register_at(username, password, now)?;
        self.create_session_at(account, now)
    }

    /// The account with this (case-insensitive) username, if any.
    pub fn find_account(&self, username: &str) -> Result<Option<Account>, AccountStoreError> {
        let Ok(username) = accounts::normalize_username(username) else {
            return Ok(None);
        };
        Ok(self.lookup(&username)?.map(|(account, _)| account))
    }

    fn lookup(&self, username: &str) -> Result<Option<(Account, String)>, AccountStoreError> {
        let row = self
            .lock()
            .query_row(
                "SELECT id, username, password_hash, created_at FROM accounts WHERE username = ?1",
                params![username],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, username, hash, created_at)) = row else {
            return Ok(None);
        };
        check_canonical(&username)?;
        Ok(Some((
            Account {
                id,
                username,
                created_at,
            },
            hash,
        )))
    }

    /// Verifies the credentials and starts a new session.
    pub fn login(&self, username: &str, password: &str) -> Result<NewSession, AccountStoreError> {
        self.login_at(username, password, now_secs())
    }

    fn login_at(
        &self,
        username: &str,
        password: &str,
        now: i64,
    ) -> Result<NewSession, AccountStoreError> {
        let found = match accounts::normalize_username(username) {
            Ok(name) => self.lookup(&name)?,
            Err(_) => None,
        };
        let Some((account, hash)) = found else {
            self.burn_dummy_verify(password);
            return Err(AccountStoreError::InvalidCredentials);
        };
        match accounts::verify_password(password, &hash) {
            Ok(true) => {}
            Ok(false) => return Err(AccountStoreError::InvalidCredentials),
            Err(e) => {
                return Err(AccountStoreError::Corrupt(format!(
                    "password hash of account {}: {e}",
                    account.id
                )));
            }
        }
        self.create_session_at(account, now)
    }

    fn burn_dummy_verify(&self, password: &str) {
        let dummy = self
            .dummy_hash
            .get_or_init(|| (self.hasher)("isle-settlers timing equalizer").ok());
        if let Some(hash) = dummy {
            let _ = accounts::verify_password(password, hash);
        }
    }

    fn create_session_at(
        &self,
        account: Account,
        now: i64,
    ) -> Result<NewSession, AccountStoreError> {
        let mut raw = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut raw).map_err(|_| AccountStoreError::Rng)?;
        let token_hash = hash_token(&raw);
        let expires_at = now
            .checked_add(self.ttl_secs)
            .ok_or_else(|| AccountStoreError::InvalidConfig("clock out of range".into()))?;
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?;
        tx.execute(
            "INSERT INTO sessions (token_hash, account_id, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![&token_hash[..], account.id, now, expires_at],
        )?;
        tx.commit()?;
        Ok(NewSession {
            token: SessionToken(to_hex(&raw)),
            session: Session {
                account,
                expires_at,
            },
        })
    }

    /// The session for `token`, or `None` if the token is malformed, unknown,
    /// logged out or expired (expired sessions are deleted).
    pub fn validate_session(&self, token: &str) -> Result<Option<Session>, AccountStoreError> {
        self.validate_session_at(token, now_secs())
    }

    fn validate_session_at(
        &self,
        token: &str,
        now: i64,
    ) -> Result<Option<Session>, AccountStoreError> {
        let Some(raw) = parse_token(token) else {
            return Ok(None);
        };
        let token_hash = hash_token(&raw);
        let conn = self.lock();
        let row = conn
            .query_row(
                "SELECT s.expires_at, a.id, a.username, a.created_at
                 FROM sessions s JOIN accounts a ON a.id = s.account_id
                 WHERE s.token_hash = ?1",
                params![&token_hash[..]],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((expires_at, id, username, created_at)) = row else {
            return Ok(None);
        };
        if expires_at <= now {
            conn.execute(
                "DELETE FROM sessions WHERE token_hash = ?1",
                params![&token_hash[..]],
            )?;
            return Ok(None);
        }
        check_canonical(&username)?;
        Ok(Some(Session {
            account: Account {
                id,
                username,
                created_at,
            },
            expires_at,
        }))
    }

    /// Ends one session. Returns whether a session was removed.
    pub fn logout(&self, token: &str) -> Result<bool, AccountStoreError> {
        let Some(raw) = parse_token(token) else {
            return Ok(false);
        };
        let token_hash = hash_token(&raw);
        let removed = self.lock().execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            params![&token_hash[..]],
        )?;
        Ok(removed > 0)
    }

    /// Ends every session of an account. Returns how many were removed.
    pub fn logout_all(&self, account_id: i64) -> Result<usize, AccountStoreError> {
        Ok(self.lock().execute(
            "DELETE FROM sessions WHERE account_id = ?1",
            params![account_id],
        )?)
    }

    /// Deletes expired sessions. Returns how many were removed.
    pub fn purge_expired(&self) -> Result<usize, AccountStoreError> {
        self.purge_expired_at(now_secs())
    }

    fn purge_expired_at(&self, now: i64) -> Result<usize, AccountStoreError> {
        Ok(self
            .lock()
            .execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?)
    }
}

pub(crate) fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Refuses anything at `path` that is neither absent, empty, nor a file with
/// an SQLite header. Nothing is written.
fn check_file(path: &Path) -> Result<(), AccountStoreError> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() {
        return Err(AccountStoreError::Corrupt(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    if meta.len() == 0 {
        return Ok(());
    }
    let mut header = [0u8; 16];
    let ok = meta.len() >= 16
        && std::fs::File::open(path)?.read_exact(&mut header).is_ok()
        && &header == SQLITE_HEADER;
    if !ok {
        return Err(AccountStoreError::Corrupt(format!(
            "{} is not an SQLite database",
            path.display()
        )));
    }
    Ok(())
}

/// Reads identity markers: `None` = empty new database, `Some(v)` = our
/// schema at version `v` (current or older), error otherwise.
fn identify(conn: &Connection) -> Result<Option<i32>, AccountStoreError> {
    let app_id: i32 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let objects: i64 = conn.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))?;
    if app_id == 0 && version == 0 && objects == 0 {
        return Ok(None);
    }
    if app_id != APPLICATION_ID {
        return Err(AccountStoreError::Incompatible(format!(
            "not an isle-settlers account database (application_id {app_id:#x})"
        )));
    }
    if !(1..=SCHEMA_VERSION).contains(&version) {
        return Err(AccountStoreError::Incompatible(format!(
            "unsupported account schema version {version} (expected {SCHEMA_VERSION})"
        )));
    }
    Ok(Some(version))
}

/// Validates an existing database (read-only checks first) or initializes
/// an empty one.
fn prepare_database(conn: &mut Connection) -> Result<(), AccountStoreError> {
    integrity_check(conn)?;
    let found = identify(conn)?;
    // Only now that the file is known to be ours (or empty) do we write.
    // WAL may be unavailable on some filesystems; the rollback journal
    // SQLite then keeps is atomic too.
    let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    conn.execute_batch("PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;")?;
    if found != Some(SCHEMA_VERSION) {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Another process may have initialized or migrated it meanwhile.
        let start = match identify(&tx)? {
            None => {
                tx.execute_batch(SCHEMA)?;
                tx.execute_batch(&format!("PRAGMA application_id = {APPLICATION_ID};"))?;
                1
            }
            Some(v) => v,
        };
        if start < 2 {
            tx.execute_batch(SCHEMA_V2)?;
        }
        tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        tx.commit()?;
        if identify(conn)? != Some(SCHEMA_VERSION) {
            return Err(AccountStoreError::Corrupt(
                "schema initialization did not persist".into(),
            ));
        }
    }
    verify_schema(conn)
}

fn integrity_check(conn: &Connection) -> Result<(), AccountStoreError> {
    let mut stmt = conn.prepare("PRAGMA integrity_check")?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() == 1 && rows[0] == "ok" {
        Ok(())
    } else {
        Err(AccountStoreError::Corrupt(format!(
            "integrity check failed: {}",
            rows.join("; ")
        )))
    }
}

fn verify_schema(conn: &Connection) -> Result<(), AccountStoreError> {
    for sql in [
        "SELECT id, username, password_hash, created_at FROM accounts LIMIT 0",
        "SELECT token_hash, account_id, created_at, expires_at FROM sessions LIMIT 0",
        "SELECT account_id, mu, sigma, games, wins, updated_at FROM ratings LIMIT 0",
        "SELECT id, finished_at, seats FROM matches LIMIT 0",
        "SELECT match_id, account_id, seat, place, points, before, after FROM match_players LIMIT 0",
    ] {
        conn.prepare(sql)
            .map_err(|e| AccountStoreError::Corrupt(format!("schema mismatch: {e}")))?;
    }
    Ok(())
}

fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

pub(crate) fn check_canonical(username: &str) -> Result<(), AccountStoreError> {
    match accounts::normalize_username(username) {
        Ok(n) if n == username => Ok(()),
        _ => Err(AccountStoreError::Corrupt(format!(
            "stored username {username:?} is not canonical"
        ))),
    }
}

fn hash_token(raw: &[u8; TOKEN_BYTES]) -> [u8; 32] {
    let digest = Sha256::digest(raw);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Strict parse: exactly `2 * TOKEN_BYTES` lowercase hex characters.
fn parse_token(token: &str) -> Option<[u8; TOKEN_BYTES]> {
    fn nibble(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        }
    }
    let bytes = token.as_bytes();
    if bytes.len() != TOKEN_BYTES * 2 {
        return None;
    }
    let mut out = [0u8; TOKEN_BYTES];
    for (i, pair) in bytes.chunks_exact(2).enumerate() {
        out[i] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const PW: &str = "correct horse battery";

    /// Minimal-cost Argon2id so tests stay fast.
    pub(crate) fn cheap_hash(password: &str) -> Result<String, CredentialError> {
        let params = argon2::Params::new(8, 1, 1, None).expect("params");
        let argon =
            argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        accounts::hash_with(&argon, password)
    }

    /// Unique directory under the OS temp dir, removed on drop.
    pub(crate) struct TempDir(pub(crate) PathBuf);

    impl TempDir {
        pub(crate) fn new() -> TempDir {
            let mut bytes = [0u8; 8];
            getrandom::fill(&mut bytes).expect("rng");
            let dir = std::env::temp_dir().join(format!("isle-account-store-{}", to_hex(&bytes)));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn open_ttl(dir: &Path, ttl: Duration) -> AccountStore {
        AccountStore::open_inner(dir, AccountStoreConfig { session_ttl: ttl }, cheap_hash)
            .expect("open store")
    }

    fn open(dir: &Path) -> AccountStore {
        open_ttl(dir, DEFAULT_SESSION_TTL)
    }

    fn try_open(dir: &Path) -> Result<AccountStore, AccountStoreError> {
        AccountStore::open_inner(dir, AccountStoreConfig::default(), cheap_hash)
    }

    fn session_count(store: &AccountStore) -> i64 {
        store
            .lock()
            .query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
            .expect("count")
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn store_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AccountStore>();
    }

    #[test]
    fn creates_directory_and_database() {
        let tmp = TempDir::new();
        let dir = tmp.0.join("nested").join("accounts");
        let store = open(&dir);
        assert_eq!(store.path(), dir.join(DB_FILE_NAME));
        assert!(store.path().is_file());
    }

    #[test]
    fn register_and_login() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        let account = store.register("Alice_01", PW).expect("register");
        assert_eq!(account.username, "alice_01");
        let login = store.login("alice_01", PW).expect("login");
        assert_eq!(login.session.account, account);
        let session = store
            .validate_session(login.token.expose())
            .expect("validate")
            .expect("valid session");
        assert_eq!(session, login.session);
    }

    #[test]
    fn register_session_starts_a_session() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        let new = store.register_session("Dora", PW).expect("register");
        assert_eq!(new.session.account.username, "dora");
        let session = store
            .validate_session(new.token.expose())
            .expect("validate")
            .expect("valid");
        assert_eq!(session, new.session);
        assert!(matches!(
            store.register_session("DORA", PW),
            Err(AccountStoreError::UsernameTaken)
        ));
        assert_eq!(session_count(&store), 1);
    }

    #[test]
    fn usernames_are_case_insensitive_and_unique() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        let account = store.register("Alice", PW).expect("register");
        assert!(matches!(
            store.register("alice", "another password"),
            Err(AccountStoreError::UsernameTaken)
        ));
        assert!(matches!(
            store.register("  ALICE ", "another password"),
            Err(AccountStoreError::UsernameTaken)
        ));
        assert_eq!(
            store.login("aLiCe", PW).expect("login").session.account,
            account
        );
        assert_eq!(store.find_account("ALICE").expect("find"), Some(account));
        assert_eq!(store.find_account("bob").expect("find"), None);
        assert_eq!(store.find_account("not valid!").expect("find"), None);
    }

    #[test]
    fn registration_validates_username() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        assert!(matches!(
            store.register("", PW),
            Err(AccountStoreError::InvalidUsername(UsernameError::Empty))
        ));
        assert!(matches!(
            store.register("bad name", PW),
            Err(AccountStoreError::InvalidUsername(
                UsernameError::InvalidChar(' ')
            ))
        ));
        assert!(matches!(
            store.register(&"a".repeat(33), PW),
            Err(AccountStoreError::InvalidUsername(UsernameError::TooLong))
        ));
    }

    #[test]
    fn registration_validates_password_length_in_bytes() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        assert!(matches!(
            store.register("a", ""),
            Err(AccountStoreError::PasswordTooShort)
        ));
        assert!(matches!(
            store.register("a", "abcdefghijk"),
            Err(AccountStoreError::PasswordTooShort)
        ));
        store
            .register("b", "abcdefghijkl")
            .expect("12 bytes is enough");
        // 5 two-byte characters = 10 bytes; 6 = 12 bytes.
        assert!(matches!(
            store.register("c", "ééééé"),
            Err(AccountStoreError::PasswordTooShort)
        ));
        store.register("d", "éééééé").expect("12 bytes is enough");
        assert!(matches!(
            store.register("e", &"x".repeat(MAX_PASSWORD_LEN + 1)),
            Err(AccountStoreError::PasswordTooLong)
        ));
        store
            .register("f", &"x".repeat(MAX_PASSWORD_LEN))
            .expect("maximum length is allowed");
    }

    #[test]
    fn bad_credentials_are_indistinguishable() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        store.register("alice", PW).expect("register");
        for (user, pw) in [
            ("alice", "wrong password!"),
            ("alice", ""),
            ("bob", PW),
            ("not valid!", PW),
            ("alice", &"x".repeat(MAX_PASSWORD_LEN + 1)),
        ] {
            assert!(
                matches!(
                    store.login(user, pw),
                    Err(AccountStoreError::InvalidCredentials)
                ),
                "{user:?}"
            );
        }
        assert_eq!(session_count(&store), 0);
    }

    #[test]
    fn tokens_are_random_hex() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        store.register("alice", PW).expect("register");
        let a = store.login("alice", PW).expect("login");
        let b = store.login("alice", PW).expect("login");
        for t in [&a.token, &b.token] {
            assert_eq!(t.expose().len(), TOKEN_BYTES * 2);
            assert!(
                t.expose()
                    .bytes()
                    .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
            );
            assert_eq!(format!("{t:?}"), "SessionToken(<redacted>)");
        }
        assert_ne!(a.token, b.token);
        assert_eq!(session_count(&store), 2);
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        store.register("alice", PW).expect("register");
        let login = store.login("alice", PW).expect("login");
        let good = login.token.expose();
        let upper = good.to_ascii_uppercase();
        let long = format!("{good}0");
        let non_hex = format!("{}g", &good[..63]);
        for bad in [
            "",
            "abc",
            &good[..63],
            long.as_str(),
            non_hex.as_str(),
            upper.as_str(),
        ] {
            if bad == good {
                continue; // a token without letters stays unchanged by uppercasing
            }
            assert_eq!(
                store.validate_session(bad).expect("validate"),
                None,
                "{bad:?}"
            );
            assert!(!store.logout(bad).expect("logout"), "{bad:?}");
        }
        assert!(store.validate_session(good).expect("validate").is_some());
    }

    #[test]
    fn logout_ends_only_that_session() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        store.register("alice", PW).expect("register");
        let a = store.login("alice", PW).expect("login");
        let b = store.login("alice", PW).expect("login");
        assert!(store.logout(a.token.expose()).expect("logout"));
        assert_eq!(
            store.validate_session(a.token.expose()).expect("validate"),
            None
        );
        assert!(
            store
                .validate_session(b.token.expose())
                .expect("validate")
                .is_some()
        );
        assert!(
            !store
                .logout(a.token.expose())
                .expect("second logout is a no-op")
        );
    }

    #[test]
    fn logout_all_ends_every_session_of_the_account() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        let alice = store.register("alice", PW).expect("register");
        store.register("bob", PW).expect("register");
        let a1 = store.login("alice", PW).expect("login");
        let a2 = store.login("alice", PW).expect("login");
        let b = store.login("bob", PW).expect("login");
        assert_eq!(store.logout_all(alice.id).expect("logout all"), 2);
        assert_eq!(
            store.validate_session(a1.token.expose()).expect("validate"),
            None
        );
        assert_eq!(
            store.validate_session(a2.token.expose()).expect("validate"),
            None
        );
        assert!(
            store
                .validate_session(b.token.expose())
                .expect("validate")
                .is_some()
        );
    }

    #[test]
    fn sessions_expire() {
        let tmp = TempDir::new();
        let store = open_ttl(&tmp.0, Duration::from_secs(60));
        store.register_at("alice", PW, 1_000).expect("register");
        let login = store.login_at("alice", PW, 1_000).expect("login");
        assert_eq!(login.session.expires_at, 1_060);
        let token = login.token.expose();
        assert!(
            store
                .validate_session_at(token, 1_059)
                .expect("validate")
                .is_some()
        );
        assert_eq!(
            store.validate_session_at(token, 1_060).expect("validate"),
            None
        );
        // The expired session was deleted, not just hidden.
        assert_eq!(session_count(&store), 0);
        assert_eq!(
            store.validate_session_at(token, 1_000).expect("validate"),
            None
        );
    }

    #[test]
    fn expired_sessions_are_purged() {
        let tmp = TempDir::new();
        let store = open_ttl(&tmp.0, Duration::from_secs(60));
        store.register_at("alice", PW, 1_000).expect("register");
        store.login_at("alice", PW, 1_000).expect("login");
        store.login_at("alice", PW, 1_030).expect("login");
        assert_eq!(session_count(&store), 2);
        // Logging in purges sessions expired by then.
        let late = store.login_at("alice", PW, 1_070).expect("login");
        assert_eq!(session_count(&store), 2);
        assert_eq!(store.purge_expired_at(1_090).expect("purge"), 1);
        assert_eq!(session_count(&store), 1);
        assert!(
            store
                .validate_session_at(late.token.expose(), 1_090)
                .expect("validate")
                .is_some()
        );
    }

    #[test]
    fn rejects_invalid_session_ttl() {
        let tmp = TempDir::new();
        for ttl in [Duration::ZERO, MAX_SESSION_TTL + Duration::from_secs(1)] {
            assert!(matches!(
                AccountStore::open_inner(
                    &tmp.0,
                    AccountStoreConfig { session_ttl: ttl },
                    cheap_hash
                ),
                Err(AccountStoreError::InvalidConfig(_))
            ));
        }
    }

    #[test]
    fn data_survives_reopen() {
        let tmp = TempDir::new();
        let (account, token) = {
            let store = open(&tmp.0);
            let account = store.register("Alice", PW).expect("register");
            let login = store.login("alice", PW).expect("login");
            (account, login.token)
        };
        let store = open(&tmp.0);
        let session = store
            .validate_session(token.expose())
            .expect("validate")
            .expect("session survives restart");
        assert_eq!(session.account, account);
        assert!(matches!(
            store.register("ALICE", PW),
            Err(AccountStoreError::UsernameTaken)
        ));
        assert!(store.login("alice", PW).is_ok());
        assert!(store.logout(token.expose()).expect("logout"));
        drop(store);
        let store = open(&tmp.0);
        assert_eq!(
            store.validate_session(token.expose()).expect("validate"),
            None
        );
    }

    #[test]
    fn no_plaintext_secrets_on_disk() {
        let tmp = TempDir::new();
        let password = "plaintext-canary-password-1234";
        let token = {
            let store = open(&tmp.0);
            store.register("alice", password).expect("register");
            let token = store.login("alice", password).expect("login").token;
            let (hash, token_hash): (String, Vec<u8>) = store
                .lock()
                .query_row(
                    "SELECT a.password_hash, s.token_hash
                     FROM accounts a JOIN sessions s ON s.account_id = a.id",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .expect("row");
            assert!(hash.starts_with("$argon2id$v=19$"));
            assert!(!hash.contains(password));
            let raw = parse_token(token.expose()).expect("well-formed token");
            assert_eq!(token_hash, Sha256::digest(raw).to_vec());
            assert_ne!(token_hash, raw.to_vec());
            token
        };
        let raw = parse_token(token.expose()).expect("well-formed token");
        let mut bytes = Vec::new();
        for entry in std::fs::read_dir(&tmp.0).expect("read dir") {
            bytes.extend(std::fs::read(entry.expect("entry").path()).expect("read file"));
        }
        assert!(contains(&bytes, b"alice"), "sanity: the data is on disk");
        assert!(!contains(&bytes, password.as_bytes()));
        assert!(!contains(&bytes, token.expose().as_bytes()));
        assert!(!contains(&bytes, &raw));
    }

    #[test]
    fn refuses_non_sqlite_file_without_touching_it() {
        let tmp = TempDir::new();
        let path = tmp.0.join(DB_FILE_NAME);
        let garbage = b"this is definitely not an sqlite database; keep me".to_vec();
        std::fs::write(&path, &garbage).expect("write");
        assert!(matches!(
            try_open(&tmp.0),
            Err(AccountStoreError::Corrupt(_))
        ));
        assert_eq!(std::fs::read(&path).expect("read"), garbage);

        std::fs::write(&path, b"SQLite").expect("write");
        assert!(matches!(
            try_open(&tmp.0),
            Err(AccountStoreError::Corrupt(_))
        ));
        assert_eq!(std::fs::read(&path).expect("read"), b"SQLite");
    }

    #[test]
    fn refuses_damaged_sqlite_file_without_touching_it() {
        let tmp = TempDir::new();
        let path = tmp.0.join(DB_FILE_NAME);
        let mut damaged = SQLITE_HEADER.to_vec();
        damaged.extend(std::iter::repeat_n(0xFF, 4096 - 16));
        std::fs::write(&path, &damaged).expect("write");
        assert!(matches!(
            try_open(&tmp.0),
            Err(AccountStoreError::Corrupt(_))
        ));
        assert_eq!(std::fs::read(&path).expect("read"), damaged);
    }

    #[test]
    fn refuses_directory_in_place_of_database() {
        let tmp = TempDir::new();
        std::fs::create_dir(tmp.0.join(DB_FILE_NAME)).expect("mkdir");
        assert!(matches!(
            try_open(&tmp.0),
            Err(AccountStoreError::Corrupt(_))
        ));
    }

    #[test]
    fn refuses_foreign_sqlite_database() {
        let tmp = TempDir::new();
        let path = tmp.0.join(DB_FILE_NAME);
        {
            let conn = Connection::open(&path).expect("open");
            conn.execute_batch("CREATE TABLE other (x INTEGER); INSERT INTO other VALUES (42);")
                .expect("create");
        }
        assert!(matches!(
            try_open(&tmp.0),
            Err(AccountStoreError::Incompatible(_))
        ));
        let conn = Connection::open(&path).expect("open");
        let x: i64 = conn
            .query_row("SELECT x FROM other", [], |r| r.get(0))
            .expect("foreign data kept");
        assert_eq!(x, 42);
    }

    #[test]
    fn refuses_unknown_schema_version() {
        let tmp = TempDir::new();
        open(&tmp.0).register("alice", PW).expect("register");
        {
            let conn = Connection::open(tmp.0.join(DB_FILE_NAME)).expect("open");
            conn.execute_batch("PRAGMA user_version = 99;")
                .expect("bump version");
        }
        assert!(matches!(
            try_open(&tmp.0),
            Err(AccountStoreError::Incompatible(_))
        ));
        let conn = Connection::open(tmp.0.join(DB_FILE_NAME)).expect("open");
        let n: i64 = conn
            .query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))
            .expect("count");
        assert_eq!(n, 1, "data kept");
    }

    #[test]
    fn refuses_missing_tables() {
        let tmp = TempDir::new();
        open(&tmp.0);
        {
            let conn = Connection::open(tmp.0.join(DB_FILE_NAME)).expect("open");
            conn.execute_batch("DROP TABLE sessions;").expect("drop");
        }
        assert!(matches!(
            try_open(&tmp.0),
            Err(AccountStoreError::Corrupt(_))
        ));
    }

    #[test]
    fn corrupted_password_hash_is_an_error_not_a_login_failure() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        store.register("alice", PW).expect("register");
        store
            .lock()
            .execute(
                "UPDATE accounts SET password_hash = '$argon2id$garbage'",
                [],
            )
            .expect("update");
        assert!(matches!(
            store.login("alice", PW),
            Err(AccountStoreError::Corrupt(_))
        ));
        assert_eq!(session_count(&store), 0);
    }

    #[test]
    fn schema_rejects_non_canonical_usernames() {
        let tmp = TempDir::new();
        let store = open(&tmp.0);
        let hash = cheap_hash(PW).expect("hash");
        let result = store.lock().execute(
            "INSERT INTO accounts (username, password_hash, created_at) VALUES ('Alice', ?1, 0)",
            params![hash],
        );
        assert!(result.is_err());
    }
}
