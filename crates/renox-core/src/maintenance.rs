//! Maintenance mode: `my-app down` makes the site answer 503 (with
//! `errors/503.html` or the built-in error page) until `my-app up`.
//!
//! `my-app down --secret s3cr3t` lets you in anyway: visiting `/s3cr3t` sets a
//! cookie that bypasses the 503. `/health` and Renox's assets keep working.
//! The state lives in `storage/framework/down`, so every process sees it.

use std::path::{Path, PathBuf};

use axum::extract::{Request, State};
use axum::http::HeaderValue;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use serde::{Deserialize, Serialize};

use crate::crypto::constant_time_eq;
use crate::queue::unix_now;
use crate::{AppState, Error, Result};

const BYPASS_COOKIE: &str = "renox_maintenance";

/// The maintenance state `down` writes to `storage/framework/down`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Down {
    /// When the app went down (stored as unix seconds).
    #[serde(with = "chrono::serde::ts_seconds")]
    pub since: crate::db::DateTime,
    /// Seconds to suggest in `Retry-After`.
    pub retry: Option<u64>,
    /// The path that sets the bypass cookie (`/{secret}`), if any.
    pub secret: Option<String>,
}

pub(crate) fn file(storage: &Path) -> PathBuf {
    storage.join("framework").join("down")
}

/// How [`down`] takes the app down, as `my-app down --secret … --retry …`.
///
/// ```no_run
/// use renox::maintenance::{self, DownOptions};
/// # fn demo(storage: &std::path::Path) -> renox::Result {
/// maintenance::down(storage, DownOptions::new().secret("let-me-in").retry(120))?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct DownOptions {
    secret: Option<String>,
    retry: Option<u64>,
}

impl DownOptions {
    /// No bypass secret, no `Retry-After`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `/{secret}` sets a cookie that lets its browser through.
    pub fn secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Seconds to suggest in `Retry-After`.
    pub fn retry(mut self, seconds: u64) -> Self {
        self.retry = Some(seconds);
        self
    }
}

/// Takes the app down (every process that shares `storage`).
pub fn down(storage: &Path, options: DownOptions) -> Result {
    let path = file(storage);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let state = Down {
        since: crate::db::from_unix(unix_now()),
        retry: options.retry,
        secret: options.secret,
    };
    std::fs::write(path, serde_json::to_string(&state)?)?;
    Ok(())
}

/// Brings the app back; returns whether it was down.
pub fn up(storage: &Path) -> Result<bool> {
    match std::fs::remove_file(file(storage)) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

/// The maintenance state, or `None` while the app is up.
pub fn status(storage: &Path) -> Option<Down> {
    let text = std::fs::read_to_string(file(storage)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The bypass cookie's value: an HMAC of the secret, so the cookie can't be
/// read back into the secret (which also opens `/{secret}`).
fn bypass_token(state: &AppState, secret: &str) -> String {
    crate::signed::signature(state, &format!("maintenance-bypass:{secret}"))
}

fn has_bypass(req: &Request, token: &str) -> bool {
    req.headers()
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .any(|(name, value)| name == BYPASS_COOKIE && constant_time_eq(value, token))
}

pub(crate) async fn middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(down) = status(&state.config.storage_path) else {
        return next.run(req).await;
    };
    // Providers would retry, but may give up; store the calls instead.
    if state
        .security
        .is_webhook(req.extensions().get::<axum::extract::MatchedPath>())
    {
        return next.run(req).await;
    }
    if let Some(secret) = &down.secret {
        if has_bypass(&req, &bypass_token(&state, secret)) {
            return next.run(req).await;
        }
        if req.uri().path().trim_start_matches('/') == secret {
            let mut res = Redirect::to("/").into_response();
            let secure = if state.config.url.starts_with("https://") {
                "; Secure"
            } else {
                ""
            };
            let token = bypass_token(&state, secret);
            let cookie = format!("{BYPASS_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax{secure}");
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                res.headers_mut().append(SET_COOKIE, value);
            }
            return res;
        }
    }
    let mut res = Error::ServiceUnavailable.into_response();
    if let Some(retry) = down.retry {
        res.headers_mut().insert("retry-after", retry.into());
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `up` that can't remove the marker (here a folder in its place) says
    /// so, instead of reporting the app as up.
    #[test]
    fn up_reports_a_marker_it_cant_remove() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!up(dir.path()).unwrap(), "nothing to bring up");
        std::fs::create_dir_all(file(dir.path())).unwrap();
        assert!(up(dir.path()).is_err());
    }
}
