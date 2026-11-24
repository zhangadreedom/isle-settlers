//! Bodies of the account HTTP API.
//!
//! | Request | Body | Success |
//! |---|---|---|
//! | `POST /api/accounts` (register) | [`Credentials`] | 201 [`SessionGrant`] |
//! | `POST /api/sessions` (log in) | [`Credentials`] | 200 [`SessionGrant`] |
//! | `GET /api/session` | | 200 [`SessionInfo`] |
//! | `DELETE /api/session` (log out) | | 204 |
//! | `DELETE /api/sessions` (log out everywhere) | | 204 |
//!
//! The session token goes in `Authorization: Bearer <token>`. Every failure
//! answers with an [`ApiError`].

use serde::{Deserialize, Serialize};

/// Username and password. `Debug` hides the password.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// The public part of an account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    /// Canonical (lowercase) username, also the player's name in rooms.
    pub username: String,
    /// Unix seconds.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub created_at: i64,
}

/// A new session, after registering or logging in.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SessionGrant {
    pub account: AccountInfo,
    /// Bearer token for later requests and for joining rooms.
    pub token: String,
    /// Unix seconds; the token stops working at this instant.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub expires_at: i64,
}

impl std::fmt::Debug for SessionGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionGrant")
            .field("account", &self.account)
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// The account behind a valid token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub account: AccountInfo,
    /// Unix seconds.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub expires_at: i64,
}

/// Error body. `error` is one of: `AccountsDisabled` (503, the server runs
/// without a data directory), `BadRequest` (400), `InvalidUsername` (400),
/// `PasswordTooShort` (400), `PasswordTooLong` (400), `UsernameTaken` (409),
/// `InvalidCredentials` (401, unknown user or wrong password),
/// `NotLoggedIn` (401, missing, expired or revoked token),
/// `TooManyAttempts` (429, with `Retry-After`), `PlayerNotFound` (404),
/// `Busy` (503) or `Internal` (500).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ApiError {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_stay_out_of_debug_output() {
        let c = Credentials {
            username: "ann".into(),
            password: "hunter2hunter2".into(),
        };
        assert!(!format!("{c:?}").contains("hunter2"));
        let g = SessionGrant {
            account: AccountInfo {
                username: "ann".into(),
                created_at: 1,
            },
            token: "abcdef0123".into(),
            expires_at: 2,
        };
        assert!(!format!("{g:?}").contains("abcdef0123"));
        let json = serde_json::to_string(&g).unwrap();
        assert!(json.contains(r#""expiresAt":2"#) && json.contains(r#""createdAt":1"#));
    }
}
