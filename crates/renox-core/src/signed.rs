//! Signed URLs: links that can't be altered and may expire, e.g. for email
//! verification. `state.signed_url(...)` builds one; the `ValidSignature`
//! extractor rejects tampered or expired ones with 403, `Option<ValidSignature>`
//! lets a handler decide, and [`verify`] checks any URL.

use std::fmt::Display;
use std::time::Duration;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::crypto::constant_time_eq;
use crate::{AppState, Error, Result};

fn now() -> u64 {
    crate::clock::unix_secs().max(0) as u64
}

pub(crate) fn signature(state: &AppState, payload: &str) -> String {
    hmac_hex(state.key.signing(), payload)
}

/// HMAC-SHA256 of `payload` under `key`, as lowercase hex.
pub(crate) fn hmac_hex(key: &[u8], payload: &str) -> String {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(payload.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl AppState {
    /// An absolute URL to a named route, valid for `ttl` and signed with `APP_KEY`.
    pub fn signed_url(&self, name: &str, params: &[&dyn Display], ttl: Duration) -> Result<String> {
        let path = self.url(name, params)?;
        self.sign_path(&path, ttl)
    }

    /// Like `signed_url`, for an already-encoded path such as `/_renox/files/a.pdf`.
    pub fn sign_path(&self, path: &str, ttl: Duration) -> Result<String> {
        let unsigned = format!("{path}?expires={}", now().saturating_add(ttl.as_secs()));
        let signature = signature(self, &unsigned);
        Ok(format!(
            "{}{unsigned}&signature={signature}",
            self.config.url.trim_end_matches('/')
        ))
    }

    /// An absolute URL to a named route.
    pub fn absolute_url(&self, name: &str, params: &[&dyn Display]) -> Result<String> {
        Ok(format!(
            "{}{}",
            self.config.url.trim_end_matches('/'),
            self.url(name, params)?
        ))
    }
}

/// Proof that the request's URL was signed by `signed_url` and hasn't expired.
/// A route that takes it answers 403 to any other URL.
///
/// As `Option<ValidSignature>` it never refuses, so one route can accept a
/// signed link (a guest coming back from a mail or a payment page) or a
/// logged-in user:
///
/// ```
/// # use renox::prelude::*;
/// use renox::signed::ValidSignature;
///
/// async fn receipt(
///     signature: Option<ValidSignature>,
///     user: Option<AuthUser>,
///     Path(order): Path<i64>,
/// ) -> Result<String> {
///     if signature.is_none() && user.is_none() {
///         return Err(Error::Forbidden);
///     }
///     Ok(format!("Order {order}"))
/// }
/// # let _ = Routes::new().get("/orders/{order}/receipt", receipt);
/// ```
///
/// (A logged-in user still needs a check that the order is theirs.)
pub struct ValidSignature;

impl<S: Send + Sync> FromRequestParts<S> for ValidSignature {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self> {
        if signed_request(parts)? {
            Ok(Self)
        } else {
            Err(Error::Forbidden)
        }
    }
}

impl<S: Send + Sync> axum::extract::OptionalFromRequestParts<S> for ValidSignature {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Option<Self>> {
        Ok(signed_request(parts)?.then_some(Self))
    }
}

/// Whether the request's whole URL is signed. Inside a `Routes::group`, axum
/// strips the group's prefix from `parts.uri`, so the original URI is used.
fn signed_request(parts: &Parts) -> Result<bool> {
    let state = parts
        .extensions
        .get::<AppState>()
        .ok_or_else(|| anyhow::anyhow!("the auth middleware is not installed"))?;
    let uri = parts
        .extensions
        .get::<axum::extract::OriginalUri>()
        .map_or(&parts.uri, |original| &original.0);
    Ok(verify(state, uri))
}

/// Whether `uri` (a path with its query, as the request has it) carries a
/// valid, unexpired signature from [`AppState::signed_url`] or
/// [`AppState::sign_path`], for a check inside a handler or middleware. The
/// [`ValidSignature`] extractor does the same for the request's own URL.
///
/// ```
/// # use renox::prelude::*;
/// # use std::time::Duration;
/// # fn demo(state: &AppState) -> Result {
/// let url = state.sign_path("/invoices/7", Duration::from_secs(600))?;
/// let path = url.trim_start_matches(&state.config.url);
/// assert!(renox::signed::verify(state, &path.parse()?));
/// assert!(!renox::signed::verify(state, &"/invoices/8?expires=1&signature=00".parse()?));
/// # Ok(()) }
/// ```
pub fn verify(state: &AppState, uri: &axum::http::Uri) -> bool {
    let query = uri.query().unwrap_or_default();
    let Some((unsigned_query, given)) = query.rsplit_once("&signature=") else {
        return false;
    };
    let Some(expires) = unsigned_query
        .strip_prefix("expires=")
        .and_then(|v| v.parse::<u64>().ok())
    else {
        return false;
    };
    let unsigned = format!("{}?{unsigned_query}", uri.path());
    expires >= now() && constant_time_eq(&signature(state, &unsigned), given)
}
