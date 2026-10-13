//! Counters in the `cache` table, shared by every server using the same
//! database: rate limits and the login lock with `CACHE_STORE=database`.

use std::time::Duration;

use crate::Result;
use crate::db::{Db, sql};
use crate::queue::unix_now;

/// Rows are `renox:count:…`, so `Cache::flush` keeps them.
fn row(key: &str) -> String {
    format!("renox:count:{key}")
}

/// Adds one to `key`'s counter for the current `window` (a new window
/// starts at 1); returns the count and when the window ends (unix seconds).
pub(crate) async fn increment(db: &Db, key: &str, window: Duration) -> Result<(u32, i64)> {
    let now = unix_now();
    let ends = now + window.as_secs().max(1) as i64;
    let row = sql(
        "INSERT INTO cache (key, value, expires_at) VALUES (?, '1', ?) \
         ON CONFLICT (key) DO UPDATE SET \
           value = CASE WHEN cache.expires_at IS NULL OR cache.expires_at <= ? THEN '1' \
                   ELSE CAST(CAST(cache.value AS BIGINT) + 1 AS TEXT) END, \
           expires_at = CASE WHEN cache.expires_at IS NULL OR cache.expires_at <= ? \
                   THEN excluded.expires_at ELSE cache.expires_at END \
         RETURNING value, expires_at",
    )
    .bind(row(key))
    .bind(ends)
    .bind(now)
    .bind(now)
    .fetch_one(db)
    .await?;
    let count: String = row.try_get("value")?;
    let ends: Option<i64> = row.try_get("expires_at")?;
    Ok((
        count.parse().unwrap_or(u32::MAX),
        ends.unwrap_or(ends_default(now)),
    ))
}

fn ends_default(now: i64) -> i64 {
    now + 1
}

/// `key`'s count and window end, if its window hasn't ended.
pub(crate) async fn read(db: &Db, key: &str) -> Result<Option<(u32, i64)>> {
    let row = sql("SELECT value, expires_at FROM cache WHERE key = ? AND expires_at > ?")
        .bind(row(key))
        .bind(unix_now())
        .fetch_optional(db)
        .await?;
    Ok(match row {
        Some(row) => {
            let count: String = row.try_get("value")?;
            Some((count.parse().unwrap_or(0), row.try_get("expires_at")?))
        }
        None => None,
    })
}

pub(crate) async fn clear(db: &Db, key: &str) -> Result {
    sql("DELETE FROM cache WHERE key = ?")
        .bind(row(key))
        .execute(db)
        .await?;
    Ok(())
}

/// Seconds from now until `ends`, at least 1.
pub(crate) fn seconds_until(ends: i64) -> u64 {
    (ends - unix_now()).max(1) as u64
}
