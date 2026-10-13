//! Signed snapshots of a live component's state.
//!
//! Format: `base64url({"c": name, "i": id, "s": state}) + "." + hex HMAC-SHA256`
//! over `"renox.live:" + body`. Signed, not encrypted: keep secrets out of fields.

use anyhow::anyhow;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use crate::crypto::constant_time_eq;
use crate::signed::hmac_hex;
use crate::{Error, Result};

fn stale() -> Error {
    Error::BadRequest("This page is out of date. Reload it and try again.".into())
}

fn mac(key: &[u8], body: &str) -> String {
    hmac_hex(key, &format!("renox.live:{body}"))
}

/// Signs a component's state; fails when the snapshot is over `max` bytes.
pub(crate) fn seal(key: &[u8], max: usize, name: &str, id: &str, state: &Value) -> Result<String> {
    let json = serde_json::to_vec(&json!({"c": name, "i": id, "s": state}))
        .map_err(|e| Error::Internal(e.into()))?;
    let body = URL_SAFE_NO_PAD.encode(json);
    let sealed = format!("{body}.{}", mac(key, &body));
    if sealed.len() > max {
        return Err(Error::Internal(anyhow!(
            "live component `{name}`'s state is {} KB, over LIVE_SNAPSHOT_MAX_SIZE ({} KB)",
            sealed.len().div_ceil(1024),
            max / 1024
        )));
    }
    Ok(sealed)
}

/// Checks a snapshot made by [`seal`] for component `name`; returns its id and state.
pub(crate) fn open(key: &[u8], max: usize, name: &str, sealed: &str) -> Result<(String, Value)> {
    if sealed.len() > max {
        return Err(stale());
    }
    let (body, signature) = sealed.split_once('.').ok_or_else(stale)?;
    if !constant_time_eq(&mac(key, body), signature) {
        return Err(stale());
    }
    let bytes = URL_SAFE_NO_PAD.decode(body).map_err(|_| stale())?;
    let mut doc: Value = serde_json::from_slice(&bytes).map_err(|_| stale())?;
    if doc["c"].as_str() != Some(name) {
        return Err(stale());
    }
    let id = doc["i"].as_str().ok_or_else(stale)?.to_string();
    Ok((id, doc["s"].take()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: usize = 64 * 1024;

    #[test]
    fn round_trip() {
        let key = [7u8; 64];
        let s = json!({"count": 3, "tags": ["a"]});
        let sealed = seal(&key, MAX, "counter", "x1", &s).unwrap();
        assert_eq!(
            open(&key, MAX, "counter", &sealed).unwrap(),
            ("x1".into(), s)
        );
    }

    #[test]
    fn changed_body_fails() {
        let key = [7u8; 64];
        let sealed = seal(&key, MAX, "counter", "x1", &json!({"n": 1})).unwrap();
        let other = seal(&key, MAX, "counter", "x1", &json!({"n": 2})).unwrap();
        let forged = format!(
            "{}.{}",
            other.split_once('.').unwrap().0,
            sealed.split_once('.').unwrap().1
        );
        assert!(matches!(
            open(&key, MAX, "counter", &forged),
            Err(Error::BadRequest(_))
        ));
        assert!(open(&key, MAX, "counter", "garbage").is_err());
    }

    #[test]
    fn other_name_fails() {
        let key = [7u8; 64];
        let sealed = seal(&key, MAX, "counter", "x1", &json!({})).unwrap();
        assert!(matches!(
            open(&key, MAX, "cart", &sealed),
            Err(Error::BadRequest(_))
        ));
    }

    #[test]
    fn wrong_key_fails() {
        let sealed = seal(&[7u8; 64], MAX, "counter", "x1", &json!({})).unwrap();
        assert!(open(&[8u8; 64], MAX, "counter", &sealed).is_err());
    }

    #[test]
    fn over_the_limit_fails_both_ways() {
        let key = [7u8; 64];
        let big = json!({"text": "x".repeat(3000)});
        let err = seal(&key, 2048, "counter", "x1", &big).unwrap_err();
        assert!(matches!(err, Error::Internal(_)));
        assert!(err.to_string().contains("LIVE_SNAPSHOT_MAX_SIZE"));
        let sealed = seal(&key, MAX, "counter", "x1", &big).unwrap();
        assert!(matches!(
            open(&key, 2048, "counter", &sealed),
            Err(Error::BadRequest(_))
        ));
    }
}
