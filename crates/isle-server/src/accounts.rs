//! Player accounts: username normalization and password hashing.
//!
//! Usernames are 1..=32 ASCII characters (`a-z`, `0-9`, `_`, `-`) and are
//! case-insensitive: they are stored and compared in lowercase form.
//! Passwords are hashed with Argon2id (v0x13, default parameters) and a
//! random salt from the OS RNG; hashes are stored as PHC strings.

use std::fmt;

use argon2::password_hash::{self, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

/// Maximum username length in (ASCII) characters.
pub const MAX_USERNAME_LEN: usize = 32;
/// Longer passwords are refused so hashing cost stays bounded.
pub const MAX_PASSWORD_LEN: usize = 1024;
/// Salt length in bytes (128 bits).
const SALT_LEN: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsernameError {
    Empty,
    TooLong,
    InvalidChar(char),
}

impl fmt::Display for UsernameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UsernameError::Empty => write!(f, "username is empty"),
            UsernameError::TooLong => {
                write!(f, "username is longer than {MAX_USERNAME_LEN} characters")
            }
            UsernameError::InvalidChar(c) => write!(f, "username contains invalid character {c:?}"),
        }
    }
}

impl std::error::Error for UsernameError {}

fn is_username_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Validates a username and returns its canonical (lowercase) form.
/// Surrounding whitespace is ignored.
pub fn normalize_username(raw: &str) -> Result<String, UsernameError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(UsernameError::Empty);
    }
    if let Some(c) = name.chars().find(|c| !is_username_char(*c)) {
        return Err(UsernameError::InvalidChar(c));
    }
    // All characters are ASCII here, so bytes == characters.
    if name.len() > MAX_USERNAME_LEN {
        return Err(UsernameError::TooLong);
    }
    Ok(name.to_ascii_lowercase())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialError {
    EmptyPassword,
    PasswordTooLong,
    /// The OS random number generator failed.
    Rng,
    /// The stored hash is not an Argon2id hash.
    UnsupportedAlgorithm,
    /// Malformed hash string or hashing failure.
    Hash(password_hash::Error),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::EmptyPassword => write!(f, "password is empty"),
            CredentialError::PasswordTooLong => {
                write!(f, "password is longer than {MAX_PASSWORD_LEN} bytes")
            }
            CredentialError::Rng => write!(f, "OS randomness unavailable"),
            CredentialError::UnsupportedAlgorithm => write!(f, "stored hash is not argon2id"),
            CredentialError::Hash(e) => write!(f, "password hash error: {e}"),
        }
    }
}

impl std::error::Error for CredentialError {}

/// The production hasher: Argon2id, version 0x13, default cost parameters.
fn argon2() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

/// Hashes a password with a fresh OS-generated salt; returns a PHC string
/// (`$argon2id$v=19$m=...,t=...,p=...$salt$hash`).
pub fn hash_password(password: &str) -> Result<String, CredentialError> {
    hash_with(&argon2(), password)
}

/// Hashes with an explicit hasher (crate-internal; tests use cheap params).
pub(crate) fn hash_with(argon: &Argon2<'_>, password: &str) -> Result<String, CredentialError> {
    if password.is_empty() {
        return Err(CredentialError::EmptyPassword);
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(CredentialError::PasswordTooLong);
    }
    let mut salt_bytes = [0u8; SALT_LEN];
    getrandom::fill(&mut salt_bytes).map_err(|_| CredentialError::Rng)?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(CredentialError::Hash)?;
    let hash = argon
        .hash_password(password.as_bytes(), &salt)
        .map_err(CredentialError::Hash)?;
    Ok(hash.to_string())
}

/// Checks a password against a stored PHC hash. A wrong password is
/// `Ok(false)`; a malformed or non-Argon2id hash is an error.
pub fn verify_password(password: &str, stored: &str) -> Result<bool, CredentialError> {
    let parsed = PasswordHash::new(stored).map_err(CredentialError::Hash)?;
    if parsed.algorithm.as_str() != "argon2id" {
        return Err(CredentialError::UnsupportedAlgorithm);
    }
    // `$argon2id$xyz` parses with `xyz` as the salt and no output; the
    // verifier would report that as a wrong password rather than damage.
    if parsed.salt.is_none() || parsed.hash.is_none() {
        return Err(CredentialError::Hash(password_hash::Error::PhcStringField));
    }
    if password.is_empty() || password.len() > MAX_PASSWORD_LEN {
        // Such passwords can never have been hashed.
        return Ok(false);
    }
    // Cost parameters and salt are taken from the stored hash.
    match argon2().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(password_hash::Error::Password) => Ok(false),
        Err(e) => Err(CredentialError::Hash(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal-cost hasher so tests stay fast in debug builds.
    fn cheap(algorithm: Algorithm) -> Argon2<'static> {
        let params = Params::new(8, 1, 1, None).expect("valid params");
        Argon2::new(algorithm, Version::V0x13, params)
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize_username("Alice_01").as_deref(), Ok("alice_01"));
        assert_eq!(
            normalize_username("bob-THE-builder").as_deref(),
            Ok("bob-the-builder")
        );
    }

    #[test]
    fn normalize_is_case_insensitive() {
        assert_eq!(normalize_username("BoB"), normalize_username("bob"));
        assert_eq!(normalize_username("BOB"), normalize_username("bOb"));
    }

    #[test]
    fn normalize_trims_whitespace() {
        assert_eq!(normalize_username("  Carol \t").as_deref(), Ok("carol"));
    }

    #[test]
    fn normalize_rejects_empty() {
        assert_eq!(normalize_username(""), Err(UsernameError::Empty));
        assert_eq!(normalize_username("   "), Err(UsernameError::Empty));
    }

    #[test]
    fn normalize_length_limit() {
        let max = "A".repeat(MAX_USERNAME_LEN);
        assert_eq!(normalize_username(&max), Ok("a".repeat(MAX_USERNAME_LEN)));
        let over = "a".repeat(MAX_USERNAME_LEN + 1);
        assert_eq!(normalize_username(&over), Err(UsernameError::TooLong));
    }

    #[test]
    fn normalize_rejects_invalid_chars() {
        assert_eq!(
            normalize_username("al ice"),
            Err(UsernameError::InvalidChar(' '))
        );
        assert_eq!(
            normalize_username("bob!"),
            Err(UsernameError::InvalidChar('!'))
        );
        assert_eq!(
            normalize_username("zoë"),
            Err(UsernameError::InvalidChar('ë'))
        );
        // Non-ASCII letters that lowercase/uppercase to ASCII are still rejected.
        assert_eq!(
            normalize_username("\u{212A}im"),
            Err(UsernameError::InvalidChar('\u{212A}'))
        );
    }

    #[test]
    fn hash_and_verify_with_default_params() {
        let hash = hash_password("correct horse battery staple").expect("hash");
        assert!(hash.starts_with("$argon2id$v=19$"));
        assert_eq!(
            verify_password("correct horse battery staple", &hash),
            Ok(true)
        );
        assert_eq!(
            verify_password("Correct horse battery staple", &hash),
            Ok(false)
        );
    }

    #[test]
    fn verify_uses_stored_params() {
        let hash = hash_with(&cheap(Algorithm::Argon2id), "hunter2").expect("hash");
        assert!(hash.starts_with("$argon2id$v=19$m=8,t=1,p=1$"));
        assert_eq!(verify_password("hunter2", &hash), Ok(true));
        assert_eq!(verify_password("hunter3", &hash), Ok(false));
        assert_eq!(verify_password("", &hash), Ok(false));
    }

    #[test]
    fn salts_are_random() {
        let argon = cheap(Algorithm::Argon2id);
        let a = hash_with(&argon, "same password").expect("hash");
        let b = hash_with(&argon, "same password").expect("hash");
        assert_ne!(a, b);
        assert_eq!(verify_password("same password", &a), Ok(true));
        assert_eq!(verify_password("same password", &b), Ok(true));
    }

    #[test]
    fn rejects_bad_passwords_on_hash() {
        assert_eq!(hash_password(""), Err(CredentialError::EmptyPassword));
        let long = "x".repeat(MAX_PASSWORD_LEN + 1);
        assert_eq!(hash_password(&long), Err(CredentialError::PasswordTooLong));
    }

    #[test]
    fn malformed_hash_is_error() {
        assert!(matches!(
            verify_password("pw", "not a hash"),
            Err(CredentialError::Hash(_))
        ));
    }

    #[test]
    fn hash_without_salt_or_output_is_error() {
        let hash = hash_with(&cheap(Algorithm::Argon2id), "pw").expect("hash");
        let without_output = &hash[..hash.rfind('$').unwrap()];
        let without_salt = &without_output[..without_output.rfind('$').unwrap()];
        for stored in [without_output, without_salt, "$argon2id$garbage"] {
            assert!(
                matches!(verify_password("pw", stored), Err(CredentialError::Hash(_))),
                "{stored}"
            );
        }
    }

    #[test]
    fn non_argon2id_hash_is_rejected() {
        let hash = hash_with(&cheap(Algorithm::Argon2i), "pw").expect("hash");
        assert!(hash.starts_with("$argon2i$"));
        assert_eq!(
            verify_password("pw", &hash),
            Err(CredentialError::UnsupportedAlgorithm)
        );
    }
}
