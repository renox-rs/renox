//! Signing and verifying webhook requests in the
//! [Standard Webhooks](https://www.standardwebhooks.com) format, so receivers
//! can use any of the existing libraries for it.
//!
//! A request carries three headers: `webhook-id` (unique per message, the
//! same on every retry), `webhook-timestamp` (Unix seconds) and
//! `webhook-signature`, a space-separated list of `v1,<base64>` values where
//! each is `HMAC-SHA256(key, "{id}.{timestamp}.{body}")`. The secret is
//! written `whsec_<base64>`; the HMAC key is the decoded part.
//!
//! ```
//! use std::time::Duration;
//! use renox::webhook::signing;
//!
//! // The test vector from the Standard Webhooks specification.
//! let secret = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
//! let id = "msg_p5jXN8AQM9LWM0D4loKWxJek";
//! let body = br#"{"test": 2432232314}"#;
//!
//! let signature = signing::sign(secret, id, 1614265330, body)?;
//! assert_eq!(signature, "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE=");
//!
//! // Long after the timestamp: refused by the tolerance window.
//! assert!(!signing::verify(secret, id, "1614265330", &signature, body, Duration::from_secs(300)));
//! # Ok::<(), renox::Error>(())
//! ```

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::{Error, Result};

/// The prefix of a signing secret.
pub const SECRET_PREFIX: &str = "whsec_";

/// Generates a new secret, `whsec_` followed by 24 random bytes in base64
/// (the spec allows 24 to 64 bytes).
pub fn generate_secret() -> String {
    let mut bytes = [0u8; 24];
    rand::fill(&mut bytes);
    format!("{SECRET_PREFIX}{}", STANDARD.encode(bytes))
}

/// Decodes a `whsec_<base64>` secret (the prefix is optional) into the HMAC
/// key.
///
/// # Errors
/// When the rest isn't valid base64.
pub fn decode_secret(secret: &str) -> Result<Vec<u8>> {
    let encoded = secret.strip_prefix(SECRET_PREFIX).unwrap_or(secret);
    STANDARD.decode(encoded.trim()).map_err(|_| {
        Error::from(anyhow::anyhow!(
            "the webhook secret is not valid base64 (expected `whsec_<base64>`)"
        ))
    })
}

fn mac(key: &[u8], id: &str, timestamp: &str, body: &[u8]) -> Hmac<Sha256> {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    mac
}

/// The `webhook-signature` value (`v1,<base64>`) for a message.
///
/// # Errors
/// When `secret` isn't valid base64.
pub fn sign(secret: &str, id: &str, timestamp: i64, body: &[u8]) -> Result<String> {
    let key = decode_secret(secret)?;
    let digest = mac(&key, id, &timestamp.to_string(), body)
        .finalize()
        .into_bytes();
    Ok(format!("v1,{}", STANDARD.encode(digest)))
}

/// Checks a request's headers against `secret`: the timestamp must be within
/// `tolerance` of now (so a captured request can't be replayed later), and
/// one of the `v1` signatures in `signature_header` must match. Comparison
/// takes the same time wherever it differs. Signatures of other versions are
/// ignored; an invalid secret or header gives `false`.
pub fn verify(
    secret: &str,
    id: &str,
    timestamp_header: &str,
    signature_header: &str,
    body: &[u8],
    tolerance: Duration,
) -> bool {
    let timestamp_header = timestamp_header.trim();
    let Ok(timestamp) = timestamp_header.parse::<i64>() else {
        return false;
    };
    if (crate::clock::unix_secs() - timestamp).unsigned_abs() > tolerance.as_secs() {
        return false;
    }
    let Ok(key) = decode_secret(secret) else {
        return false;
    };
    let expected = mac(&key, id, timestamp_header, body)
        .finalize()
        .into_bytes();
    signature_header
        .split(' ')
        .filter_map(|part| part.strip_prefix("v1,"))
        .filter_map(|b64| STANDARD.decode(b64).ok())
        .fold(false, |ok, sig| {
            let equal = sig.len() == expected.len()
                && sig
                    .iter()
                    .zip(expected.iter())
                    .fold(0u8, |acc, (x, y)| acc | (x ^ y))
                    == 0;
            ok | equal
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
    const ID: &str = "msg_p5jXN8AQM9LWM0D4loKWxJek";
    const BODY: &[u8] = br#"{"test": 2432232314}"#;
    const VECTOR: &str = "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE=";
    const WINDOW: Duration = Duration::from_secs(300);

    #[test]
    fn matches_the_specification_test_vector() {
        assert_eq!(sign(SECRET, ID, 1614265330, BODY).unwrap(), VECTOR);
    }

    #[test]
    fn verifies_within_the_window_and_rejects_tampering() {
        let now = crate::clock::unix_secs();
        let signature = sign(SECRET, ID, now, BODY).unwrap();
        let ts = now.to_string();
        assert!(verify(SECRET, ID, &ts, &signature, BODY, WINDOW));
        assert!(!verify(SECRET, ID, &ts, &signature, b"{}", WINDOW));
        assert!(!verify(SECRET, "msg_other", &ts, &signature, BODY, WINDOW));
        assert!(!verify(
            &generate_secret(),
            ID,
            &ts,
            &signature,
            BODY,
            WINDOW
        ));
        assert!(!verify(
            SECRET,
            ID,
            "not a number",
            &signature,
            BODY,
            WINDOW
        ));
    }

    #[test]
    fn rejects_old_and_future_timestamps() {
        let now = crate::clock::unix_secs();
        for ts in [now - 301, now + 301] {
            let signature = sign(SECRET, ID, ts, BODY).unwrap();
            assert!(!verify(
                SECRET,
                ID,
                &ts.to_string(),
                &signature,
                BODY,
                WINDOW
            ));
        }
    }

    #[test]
    fn accepts_any_of_several_signatures_and_skips_other_versions() {
        let now = crate::clock::unix_secs();
        let good = sign(SECRET, ID, now, BODY).unwrap();
        let other = sign(&generate_secret(), ID, now, BODY).unwrap();
        let ts = now.to_string();
        let header = format!("{other} {good}");
        assert!(verify(SECRET, ID, &ts, &header, BODY, WINDOW));
        let header = format!("v2,abc {good}");
        assert!(verify(SECRET, ID, &ts, &header, BODY, WINDOW));
        assert!(!verify(SECRET, ID, &ts, "v1,!!! v2,abc", BODY, WINDOW));
        assert!(!verify(SECRET, ID, &ts, "", BODY, WINDOW));
    }

    #[test]
    fn generated_secrets_decode_and_differ() {
        let a = generate_secret();
        assert!(a.starts_with(SECRET_PREFIX));
        assert_eq!(decode_secret(&a).unwrap().len(), 24);
        assert_ne!(a, generate_secret());
        assert!(decode_secret("whsec_***").is_err());
    }
}
