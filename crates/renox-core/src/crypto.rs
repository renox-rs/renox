use anyhow::{Context, ensure};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use cookie::Key;

/// Generates a new `APP_KEY` value, e.g. `base64:3q2+7w==...`.
pub fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    format!("base64:{}", STANDARD.encode(bytes))
}

/// Parses an `APP_KEY` value (`base64:...` or at least 32 raw bytes).
pub(crate) fn parse_key(value: &str) -> anyhow::Result<Key> {
    let bytes = match value.strip_prefix("base64:") {
        Some(encoded) => STANDARD
            .decode(encoded)
            .context("APP_KEY is not valid base64")?,
        None => value.as_bytes().to_vec(),
    };
    ensure!(
        bytes.len() >= 32,
        "APP_KEY must be at least 32 bytes; generate one with `rnx key:generate`"
    );
    Ok(Key::derive_from(&bytes))
}

/// Associated data for [`seal`], so its values can't pass as an encrypted
/// cookie or the other way round.
const SEALED: &str = "renox.encrypted";

/// Encrypts `plain` with AES-256-GCM under `key` (via the cookie crate's
/// private jar); base64 text.
pub(crate) fn seal(key: &Key, plain: &str) -> String {
    let mut jar = cookie::CookieJar::new();
    jar.private_mut(key)
        .add(cookie::Cookie::new(SEALED, plain.to_owned()));
    jar.get(SEALED)
        .map(|sealed| sealed.value().to_owned())
        .unwrap_or_default()
}

/// Reads a value from [`seal`]; fails if it was changed or sealed with
/// another key.
pub(crate) fn open(key: &Key, sealed: &str) -> anyhow::Result<String> {
    cookie::CookieJar::new()
        .private(key)
        .decrypt(cookie::Cookie::new(SEALED, sealed.to_owned()))
        .map(|plain| plain.value().to_owned())
        .context("the value can't be decrypted with this APP_KEY")
}

/// A random, URL-safe token with 256 bits of entropy (43 characters), e.g.
/// for an invitation link or an API key.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Compares two strings in time independent of where they differ.
pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_parse() {
        let key = generate_key();
        assert!(key.starts_with("base64:"));
        assert!(parse_key(&key).is_ok());
    }

    #[test]
    fn short_keys_are_rejected() {
        assert!(parse_key("too-short").is_err());
    }

    #[test]
    fn compares_tokens() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
