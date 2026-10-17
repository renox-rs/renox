//! Idempotency storage: the first response to a request that carries an
//! `Idempotency-Key`, kept so a replay of the same request gets the same
//! answer without running the handler again. It backs `.offline()` routes
//! (#52); this module is only the storage, the middleware comes on top.
//!
//! A key belongs to a *scope* (the user or session that sent it, `""` when
//! anonymous) and a *route*, so one caller cannot replay another's key. The
//! row is inserted *before* the handler runs ([`claim`]); the unique
//! (scope, route, key) index is what makes two concurrent requests with one
//! key run the handler once. When the handler has answered, [`complete`]
//! stores the status, headers and body; [`release`] drops the claim again
//! (for an answer that must be retried, such as a 5xx).
//!
//! [`Routes::offline`](crate::Routes::offline) is the middleware on top: it
//! requires the header, claims, runs the handler once and replays its answer.
//!
//! ```
//! use renox::idempotency::{self, Claim};
//!
//! # async fn demo(db: renox::db::Db) -> renox::Result<()> {
//! let hash = idempotency::body_hash(b"qty=3");
//! match idempotency::claim(&db, "user:7", "/stops/{id}/complete", "k-1", &hash).await? {
//!     Claim::Claimed => {
//!         // Run the handler, then keep its answer.
//!         let headers = [("content-type".to_owned(), "text/plain".to_owned())];
//!         idempotency::complete(&db, "user:7", "/stops/{id}/complete", "k-1", 200, &headers, b"ok")
//!             .await?;
//!     }
//!     Claim::Replay(stored) => assert_eq!(stored.status, 200),
//!     Claim::Mismatch => { /* same key, other body: answer 422 */ }
//!     // `Claim::InProgress`: the first request is still running.
//!     _ => {}
//! }
//! # Ok(()) }
//! ```

use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::extract::{MatchedPath, Request};
use axum::http::header::{CONTENT_TYPE, DATE, SET_COOKIE};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{DateTime, Db, Migration, now, sql};
use crate::{Error, Result};

pub(crate) const MIGRATION: Migration = crate::db::framework_migration!(
    "idempotency",
    "00010101000440_create_idempotency_keys_table"
);

/// How long keys are kept by default before [`prune`] may remove them.
pub const DEFAULT_RETENTION: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

/// The response stored for a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Stored {
    /// The HTTP status of the stored response.
    pub status: u16,
    /// The stored response headers, in order.
    pub headers: Vec<(String, String)>,
    /// The stored response body.
    pub body: Vec<u8>,
    /// When the key was first seen.
    pub created_at: DateTime,
}

/// What [`claim`] found for a key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Claim {
    /// The key is new and now belongs to this request: run the handler, then
    /// call [`complete`] (or [`release`]).
    Claimed,
    /// The key was seen with the same body and has a stored response.
    Replay(Stored),
    /// The key was seen with a different body.
    Mismatch,
    /// The key was seen with the same body, but its handler has not answered
    /// yet (a concurrent request, or one that was interrupted).
    InProgress,
}

/// The SHA-256 of a request body, as lowercase hex.
pub fn body_hash(body: &[u8]) -> String {
    use std::fmt::Write;
    Sha256::digest(body)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Claims `key` for the request whose body hashes to `body_hash`, or says what
/// is already stored for it. Exactly one of several concurrent callers gets
/// [`Claim::Claimed`].
pub async fn claim(db: &Db, scope: &str, route: &str, key: &str, body_hash: &str) -> Result<Claim> {
    let inserted = sql(
        "INSERT INTO idempotency_keys (scope, route, key, body_hash, created_at) \
         VALUES (?, ?, ?, ?, ?) ON CONFLICT (scope, route, key) DO NOTHING",
    )
    .bind(scope)
    .bind(route)
    .bind(key)
    .bind(body_hash)
    .bind(now())
    .execute(db)
    .await?;
    if inserted == 1 {
        return Ok(Claim::Claimed);
    }
    let row = sql("SELECT body_hash, status, headers, response, created_at \
         FROM idempotency_keys WHERE scope = ? AND route = ? AND key = ?")
    .bind(scope)
    .bind(route)
    .bind(key)
    .fetch_optional(db)
    .await?;
    let Some(row) = row else {
        // Pruned between the insert and the select: it is free again.
        return Box::pin(claim(db, scope, route, key, body_hash)).await;
    };
    let stored_hash: String = row.try_get("body_hash")?;
    if stored_hash != body_hash {
        return Ok(Claim::Mismatch);
    }
    let status: Option<i64> = row.try_get("status")?;
    let Some(status) = status else {
        return Ok(Claim::InProgress);
    };
    let headers: Option<String> = row.try_get("headers")?;
    let headers = match headers {
        Some(text) => serde_json::from_str(&text).map_err(anyhow::Error::from)?,
        None => Vec::new(),
    };
    Ok(Claim::Replay(Stored {
        status: u16::try_from(status).map_err(anyhow::Error::from)?,
        headers,
        body: row
            .try_get::<Option<Vec<u8>>>("response")?
            .unwrap_or_default(),
        created_at: row.try_get("created_at")?,
    }))
}

/// Stores the response for a key claimed with [`claim`]. Errors with
/// [`Error::NotFound`] when the key is no longer there (it was pruned or
/// released).
pub async fn complete(
    db: &Db,
    scope: &str,
    route: &str,
    key: &str,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<()> {
    let headers = serde_json::to_string(headers).map_err(anyhow::Error::from)?;
    let changed = sql(
        "UPDATE idempotency_keys SET status = ?, headers = ?, response = ? \
         WHERE scope = ? AND route = ? AND key = ?",
    )
    .bind(i64::from(status))
    .bind(headers)
    .bind(body.to_vec())
    .bind(scope)
    .bind(route)
    .bind(key)
    .execute(db)
    .await?;
    if changed == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

/// Drops a key that has no stored response yet, so the request can be sent
/// again (a handler that failed, or answered with a 5xx). A key that was
/// completed is kept.
pub async fn release(db: &Db, scope: &str, route: &str, key: &str) -> Result<()> {
    sql("DELETE FROM idempotency_keys \
         WHERE scope = ? AND route = ? AND key = ? AND status IS NULL")
    .bind(scope)
    .bind(route)
    .bind(key)
    .execute(db)
    .await?;
    Ok(())
}

/// Removes the keys first seen before `before`; returns how many.
pub async fn prune(db: &Db, before: DateTime) -> Result<u64> {
    Ok(sql("DELETE FROM idempotency_keys WHERE created_at < ?")
        .bind(before)
        .execute(db)
        .await?)
}

/// Removes the keys older than the app's retention (`IDEMPOTENCY_RETENTION_DAYS`,
/// 7 by default, or `days` when given); returns how many. What
/// `my-app idempotency:prune` and the scheduled task run.
pub(crate) async fn prune_retained(state: &crate::AppState, days: Option<u32>) -> Result<u64> {
    let days = days.unwrap_or(state.config.idempotency_retention_days);
    prune(&state.db, now() - chrono::TimeDelta::days(i64::from(days))).await
}

/// The header that carries the key of a request to an `.offline()` route.
pub const KEY_HEADER: &str = "idempotency-key";

/// Set to `true` on a response replayed from the store.
pub const REPLAY_HEADER: &str = "idempotent-replayed";

/// Longest key accepted.
const KEY_LIMIT: usize = 255;

/// Largest response kept for replay.
const RESPONSE_LIMIT: usize = 16 * 1024 * 1024;

/// How long a request waits for the first one with its key to answer.
const WAIT: Duration = Duration::from_secs(5);

/// How far a device's clock may differ from the server's before a request to
/// an `.offline()` route counts as `skew_exceeded`: by default 24 hours in
/// the past and 5 minutes in the future.
///
/// ```
/// use std::time::Duration;
/// use renox::idempotency::Skew;
///
/// let skew = Skew::new().past(Duration::from_secs(72 * 3600)).future(Duration::from_secs(60));
/// assert_eq!(skew.max_past(), Duration::from_secs(72 * 3600));
/// assert_eq!(Skew::default().max_future(), Duration::from_secs(300));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skew {
    past: Duration,
    future: Duration,
}

impl Default for Skew {
    fn default() -> Self {
        Self {
            past: Duration::from_secs(24 * 60 * 60),
            future: Duration::from_secs(5 * 60),
        }
    }
}

impl Skew {
    /// The default limits: 24 hours in the past, 5 minutes in the future.
    pub fn new() -> Self {
        Self::default()
    }

    /// How old a device time may be.
    #[must_use]
    pub fn past(mut self, limit: Duration) -> Self {
        self.past = limit;
        self
    }

    /// How far ahead of the server a device time may be.
    #[must_use]
    pub fn future(mut self, limit: Duration) -> Self {
        self.future = limit;
        self
    }

    /// How old a device time may be.
    pub fn max_past(&self) -> Duration {
        self.past
    }

    /// How far ahead of the server a device time may be.
    pub fn max_future(&self) -> Duration {
        self.future
    }
}

/// The header that carries the time the device recorded the request.
pub const RECORDED_AT_HEADER: &str = "x-recorded-at";

/// The header that carries the id of the device (outbox) that sent it.
pub const CLIENT_ID_HEADER: &str = "x-client-id";

/// Longest client id kept.
const CLIENT_ID_LIMIT: usize = 128;

/// What a handler on an `.offline()` route knows about when and where the
/// request was made: the outbox sends the device time in `X-Recorded-At`
/// (RFC 3339) and its id in `X-Client-Id`.
///
/// Outside the route's [`Skew`] limits the handler still gets the device time
/// in `recorded_at`, with `skew_exceeded` set; whether to refuse, accept for
/// review or ask for approval is the app's decision. A missing or garbled
/// `X-Recorded-At` falls back to the server time with `skew_exceeded` false.
///
/// ```
/// use renox::prelude::*;
///
/// async fn complete_stop(offline: Offline) -> Result<String> {
///     if offline.skew_exceeded {
///         return Err(Error::BadRequest("The device clock is off.".into()));
///     }
///     Ok(format!("done at {}", offline.recorded_at))
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offline {
    /// When the user acted, by the device's clock (the server time when the
    /// header is missing or unreadable).
    pub recorded_at: DateTime,
    /// The id of the device's outbox, from `X-Client-Id`, when sent.
    pub client_id: Option<String>,
    /// When the server received the request.
    pub received_at: DateTime,
    /// Whether `recorded_at` is further in the past or future than the
    /// route's [`Skew`] allows.
    pub skew_exceeded: bool,
}

impl Offline {
    /// Reads the headers against the clock and the skew limits.
    pub(crate) fn read(headers: &axum::http::HeaderMap, skew: Skew, received_at: DateTime) -> Self {
        let text = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let device = text(RECORDED_AT_HEADER)
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
            .map(|t| t.with_timezone(&chrono::Utc));
        let recorded_at = device.unwrap_or(received_at);
        let skew_exceeded = device.is_some_and(|t| {
            let age = received_at - t;
            let past =
                chrono::TimeDelta::from_std(skew.max_past()).unwrap_or(chrono::TimeDelta::MAX);
            let future =
                chrono::TimeDelta::from_std(skew.max_future()).unwrap_or(chrono::TimeDelta::MAX);
            age > past || -age > future
        });
        let client_id = text(CLIENT_ID_HEADER)
            .filter(|v| v.len() <= CLIENT_ID_LIMIT)
            .map(str::to_owned);
        Self {
            recorded_at,
            client_id,
            received_at,
            skew_exceeded,
        }
    }
}

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for Offline {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let skew = parts.extensions.get::<Skew>().copied().unwrap_or_default();
        Ok(Self::read(&parts.headers, skew, now()))
    }
}

/// Who a key belongs to: the user, the device, or else the session.
fn scope_of(req: &Request) -> String {
    let current = req.extensions().get::<crate::auth::CurrentUser>();
    if let Some(user) = current.and_then(|c| c.user.as_ref()) {
        return format!("user:{}", user.id);
    }
    if let Some(device) = current.and_then(|c| c.device.as_ref()) {
        return format!("device:{}", device.key());
    }
    req.extensions()
        .get::<crate::session::Session>()
        .map_or_else(String::new, |s| format!("session:{}", s.token()))
}

/// The bytes a body is compared by: a replay carries a fresh CSRF token (and,
/// for a multipart form, a new boundary), which must not make it "another
/// body".
fn comparable(headers: &axum::http::HeaderMap, body: &[u8]) -> Vec<u8> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if content_type.starts_with("application/x-www-form-urlencoded") {
        let mut out = form_urlencoded::Serializer::new(String::new());
        for (name, value) in
            form_urlencoded::parse(body).filter(|(name, _)| name != crate::csrf::CSRF_FIELD)
        {
            out.append_pair(&name, &value);
        }
        return out.finish().into_bytes();
    }
    let boundary = content_type
        .split(';')
        .filter_map(|part| part.trim().strip_prefix("boundary="))
        .next()
        .map(|b| b.trim_matches('"'))
        .filter(|b| !b.is_empty());
    match (content_type.starts_with("multipart/form-data"), boundary) {
        (true, Some(boundary)) => String::from_utf8_lossy(body)
            .replace(boundary, "-")
            .into_bytes(),
        _ => body.to_vec(),
    }
}

fn replay_of(stored: Stored) -> Response {
    let mut res = Response::new(Body::from(stored.body));
    *res.status_mut() = StatusCode::from_u16(stored.status).unwrap_or(StatusCode::OK);
    for (name, value) in stored.headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            res.headers_mut().append(name, value);
        }
    }
    res.headers_mut().insert(
        HeaderName::from_static(REPLAY_HEADER),
        HeaderValue::from_static("true"),
    );
    res
}

/// Frees a claim again when the handler's future is dropped before it
/// answered (the client went away), or when its answer is not kept.
struct Guard {
    db: Db,
    scope: String,
    route: String,
    key: String,
    armed: bool,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (db, scope, route, key) = (
            self.db.clone(),
            std::mem::take(&mut self.scope),
            std::mem::take(&mut self.route),
            std::mem::take(&mut self.key),
        );
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = release(&db, &scope, &route, &key).await;
            });
        }
    }
}

/// The `.offline()` middleware (`Routes::offline_with`).
pub(crate) async fn middleware(skew: Skew, mut req: Request, next: Next) -> Response {
    if matches!(
        *req.method(),
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    ) {
        return next.run(req).await;
    }
    let Some(key) = req
        .headers()
        .get(KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty() && v.len() <= KEY_LIMIT)
    else {
        return Error::BadRequest(
            "This request needs an Idempotency-Key header (at most 255 characters).".into(),
        )
        .into_response();
    };
    let Some(state) = req.extensions().get::<crate::AppState>().cloned() else {
        return Error::from(anyhow::anyhow!(
            "`.offline()` needs the app state; it runs inside an App"
        ))
        .into_response();
    };
    let db = state.db.clone();
    let scope = scope_of(&req);
    let route = format!(
        "{} {}",
        req.method(),
        req.extensions()
            .get::<MatchedPath>()
            .map_or_else(|| req.uri().path(), MatchedPath::as_str)
    );

    let limit = state.config.upload_max_size.max(crate::csrf::FORM_LIMIT);
    let target = req
        .uri()
        .path_and_query()
        .map_or_else(String::new, ToString::to_string);
    let (parts, body) = req.into_parts();
    let Ok(bytes) = to_bytes(body, limit).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    // The URL is part of the request: one key must not stand for two targets.
    let mut compared = target.into_bytes();
    compared.push(b'\n');
    compared.extend(comparable(&parts.headers, &bytes));
    let hash = body_hash(&compared);
    req = Request::from_parts(parts, Body::from(bytes));

    let waited = std::time::Instant::now();
    loop {
        match claim(&db, &scope, &route, &key, &hash).await {
            Err(err) => return err.into_response(),
            Ok(Claim::Replay(stored)) => return replay_of(stored),
            Ok(Claim::Mismatch) => {
                return Error::Status(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "This Idempotency-Key was already used for a different request.".into(),
                )
                .into_response();
            }
            Ok(Claim::InProgress) => {
                if waited.elapsed() >= WAIT {
                    let mut res = Error::Status(
                        StatusCode::CONFLICT,
                        "The first request with this Idempotency-Key is still running.".into(),
                    )
                    .into_response();
                    res.headers_mut()
                        .insert("retry-after", HeaderValue::from_static("1"));
                    return res;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok(Claim::Claimed) => break,
        }
    }

    let mut guard = Guard {
        db: db.clone(),
        scope: scope.clone(),
        route: route.clone(),
        key: key.clone(),
        armed: true,
    };
    req.extensions_mut().insert(skew);
    let res = next.run(req).await;
    if res.status().is_server_error() {
        // Not kept: the guard frees the key so the client can send it again.
        return res;
    }
    let (parts, body) = res.into_parts();
    let Ok(bytes) = to_bytes(body, RESPONSE_LIMIT).await else {
        return Error::from(anyhow::anyhow!("the response is too large to keep")).into_response();
    };
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .filter(|(name, _)| **name != SET_COOKIE && **name != DATE)
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    match complete(
        &db,
        &scope,
        &route,
        &key,
        parts.status.as_u16(),
        &headers,
        &bytes,
    )
    .await
    {
        Ok(()) => guard.armed = false,
        Err(err) => tracing::warn!(error = ?err, "could not store an idempotent response"),
    }
    Response::from_parts(parts, Body::from(bytes))
}
