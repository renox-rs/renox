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
//!     Claim::InProgress => { /* the first request is still running */ }
//! }
//! # Ok(()) }
//! ```

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
