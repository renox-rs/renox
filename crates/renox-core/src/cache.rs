//! A key-value cache for expensive results.
//!
//! ```
//! # use renox::prelude::*;
//! # use std::time::Duration;
//! # #[derive(Model, serde::Serialize, serde::Deserialize, Default)] struct Product { id: i64, name: String }
//! # async fn demo(state: AppState) -> Result {
//! let menu: Vec<Product> = state.cache.remember("menu", Duration::from_secs(600), || async {
//!     Product::query().order_by("name").get(&state.db).await
//! }).await?;
//! state.cache.forget("menu").await?;   // after the menu changes
//! # let _ = menu; Ok(()) }
//! ```
//!
//! Counters, one-time values and locks:
//!
//! ```
//! # use renox::prelude::*;
//! # use std::time::Duration;
//! # async fn demo(state: AppState, order_id: i64) -> Result {
//! let views = state.cache.increment("views:home", 1).await?; // atomic; 1 the first time
//! if state.cache.add("welcome-sent:7", &true, Some(Duration::from_secs(86_400))).await? {
//!     // only the first caller gets here
//! }
//! let code: Option<String> = state.cache.pull("otp:7").await?; // read once, then gone
//!
//! // One process at a time handles this order; others wait up to 5 s.
//! let lock = state.cache.lock(&format!("order:{order_id}"), Duration::from_secs(30));
//! let guard = lock.block(Duration::from_secs(5)).await?;
//! // ... work on the order ...
//! guard.release().await?; // or let it drop
//! # let _ = (views, code); Ok(()) }
//! ```
//!
//! `CACHE_STORE=memory` (default) keeps values in this process; `database`
//! keeps them in the `cache` table, so they survive restarts and are shared
//! with `queue:work` processes and other servers (locks included). Values are
//! stored as JSON. Keys starting with `renox:` belong to the framework (e.g.
//! scheduler claims, locks). Expired rows of the database store are deleted
//! now and then as values are written, and by `my-app cache:prune`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::Result;
use crate::db::{Db, Migration};
use crate::queue::unix_now;

pub(crate) const MIGRATION: Migration =
    crate::db::framework_migration!("cache", "00010101000200_create_cache_table");

/// Entries the memory store keeps before dropping expired ones on write.
const SWEEP_AT: usize = 10_000;

/// Value and expiry (unix seconds) per key.
type Entries = HashMap<String, (Value, Option<i64>)>;

#[derive(Clone)]
enum Store {
    Memory(Arc<Mutex<Entries>>),
    Database(Db),
}

/// The app's cache (`state.cache`), in memory or the `cache` table (`CACHE_STORE`).
#[derive(Clone)]
pub struct Cache {
    store: Store,
    /// One lock per key being computed by `remember` in this process.
    computing: Arc<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>>,
}

/// Unix seconds when a value stored now for `ttl` expires, rounded up.
fn expiry(ttl: Option<Duration>) -> Option<i64> {
    ttl.map(|ttl| unix_now() + ttl.as_secs() as i64 + i64::from(ttl.subsec_nanos() > 0))
}

/// Keys of framework rows, which `flush` keeps.
const FRAMEWORK_PREFIX: &str = "renox:";
/// Keys of locks.
const LOCK_PREFIX: &str = "renox:lock:";
/// Seconds between the database store's automatic prunes, per process.
const PRUNE_EVERY: i64 = 60 * 60;

fn live(expires: Option<i64>, now: i64) -> bool {
    expires.is_none_or(|at| at > now)
}

impl Cache {
    pub(crate) fn new(store: crate::CacheStore, db: Db) -> anyhow::Result<Self> {
        let store = match store {
            crate::CacheStore::Memory => Store::Memory(Arc::default()),
            crate::CacheStore::Database => Store::Database(db),
        };
        Ok(Self {
            store,
            computing: Arc::default(),
        })
    }

    async fn raw(&self, key: &str) -> Result<Option<Value>> {
        let now = unix_now();
        match &self.store {
            Store::Memory(map) => {
                let map = map.lock().unwrap_or_else(|e| e.into_inner());
                Ok(map
                    .get(key)
                    .filter(|(_, expires)| expires.is_none_or(|at| at > now))
                    .map(|(value, _)| value.clone()))
            }
            Store::Database(db) => {
                let text: Option<String> = crate::db::sql(
                    "SELECT value FROM cache WHERE key = ? AND (expires_at IS NULL OR expires_at > ?)",
                )
                .bind(key)
                .bind(now)
                .scalar_optional(db)
                .await?;
                Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
            }
        }
    }

    /// The cached value, if present and not expired. A value of another
    /// type than `T` is an error.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.raw(key).await? {
            None => Ok(None),
            Some(value) => serde_json::from_value(value).map(Some).map_err(|err| {
                anyhow::Error::new(err)
                    .context(format!(
                        "the cached `{key}` is not a {}",
                        std::any::type_name::<T>()
                    ))
                    .into()
            }),
        }
    }

    /// Whether `key` holds a value that hasn't expired.
    pub async fn has(&self, key: &str) -> Result<bool> {
        Ok(self.raw(key).await?.is_some())
    }

    /// Stores `value` for `ttl`, or until forgotten when `ttl` is `None`. A
    /// zero `ttl` stores nothing (and forgets the key).
    pub async fn put(&self, key: &str, value: &impl Serialize, ttl: Option<Duration>) -> Result {
        if ttl == Some(Duration::ZERO) {
            return self.forget(key).await;
        }
        let value = serde_json::to_value(value)?;
        let expires = expiry(ttl);
        match &self.store {
            Store::Memory(map) => {
                let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
                if map.len() >= SWEEP_AT {
                    let now = unix_now();
                    map.retain(|_, (_, at)| at.is_none_or(|at| at > now));
                }
                map.insert(key.to_owned(), (value, expires));
            }
            Store::Database(db) => {
                self.prune_now_and_then(db).await;
                crate::db::sql(
                    "INSERT INTO cache (key, value, expires_at) VALUES (?, ?, ?) \
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value, expires_at = excluded.expires_at",
                )
                .bind(key)
                .bind(value.to_string())
                .bind(expires)
                .execute(db)
                .await?;
            }
        }
        Ok(())
    }

    /// Stores `value` only if `key` has no live value; returns whether it
    /// did. Atomic, so of several callers exactly one gets `true`.
    pub async fn add(
        &self,
        key: &str,
        value: &impl Serialize,
        ttl: Option<Duration>,
    ) -> Result<bool> {
        let value = serde_json::to_value(value)?;
        let expires = expiry(ttl);
        let now = unix_now();
        match &self.store {
            Store::Memory(map) => {
                let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
                if map.get(key).is_some_and(|(_, at)| live(*at, now)) {
                    return Ok(false);
                }
                map.insert(key.to_owned(), (value, expires));
                Ok(true)
            }
            Store::Database(db) => {
                self.prune_now_and_then(db).await;
                let added = crate::db::sql(
                    "INSERT INTO cache (key, value, expires_at) VALUES (?, ?, ?) \
                     ON CONFLICT (key) DO UPDATE SET value = excluded.value, \
                     expires_at = excluded.expires_at \
                     WHERE cache.expires_at IS NOT NULL AND cache.expires_at <= ?",
                )
                .bind(key)
                .bind(value.to_string())
                .bind(expires)
                .bind(now)
                .execute(db)
                .await?;
                Ok(added == 1)
            }
        }
    }

    /// The cached value, removed from the cache in the same step (a one-time
    /// code, a flash of data between requests).
    pub async fn pull<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let now = unix_now();
        let value = match &self.store {
            Store::Memory(map) => map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(key)
                .filter(|(_, at)| live(*at, now))
                .map(|(value, _)| value),
            Store::Database(db) => {
                let row: Option<(String, Option<i64>)> =
                    crate::db::sql("DELETE FROM cache WHERE key = ? RETURNING value, expires_at")
                        .bind(key)
                        .fetch_as(db)
                        .await?
                        .into_iter()
                        .next();
                row.filter(|(_, at)| live(*at, now))
                    .and_then(|(text, _)| serde_json::from_str(&text).ok())
            }
        };
        match value {
            None => Ok(None),
            Some(value) => Ok(Some(serde_json::from_value(value).map_err(|err| {
                anyhow::Error::new(err).context(format!("the cached `{key}` is not that type"))
            })?)),
        }
    }

    /// Adds `by` (negative to subtract) to the number at `key` and returns
    /// the new value; a missing or expired key counts as 0 and never
    /// expires. Atomic, so concurrent calls don't lose counts. A live value
    /// that isn't a whole number is an error.
    pub async fn increment(&self, key: &str, by: i64) -> Result<i64> {
        let now = unix_now();
        match &self.store {
            Store::Memory(map) => {
                let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
                let (current, expires) = match map.get(key) {
                    Some((value, at)) if live(*at, now) => {
                        let Some(n) = value.as_i64() else {
                            return Err(anyhow::anyhow!(
                                "the cached `{key}` is not a whole number"
                            )
                            .into());
                        };
                        (n, *at)
                    }
                    _ => (0, None),
                };
                let next = current
                    .checked_add(by)
                    .ok_or_else(|| anyhow::anyhow!("the cached `{key}` would overflow"))?;
                map.insert(key.to_owned(), (Value::from(next), expires));
                Ok(next)
            }
            Store::Database(db) => {
                if let Some(value) = self.raw(key).await?
                    && !value.is_i64()
                {
                    return Err(anyhow::anyhow!("the cached `{key}` is not a whole number").into());
                }
                let next: String = crate::db::sql(
                    "INSERT INTO cache (key, value, expires_at) VALUES (?, ?, NULL) \
                     ON CONFLICT (key) DO UPDATE SET \
                     value = CASE WHEN cache.expires_at IS NOT NULL AND cache.expires_at <= ? \
                         THEN excluded.value \
                         ELSE CAST(CAST(cache.value AS BIGINT) + ? AS TEXT) END, \
                     expires_at = CASE WHEN cache.expires_at IS NOT NULL AND cache.expires_at <= ? \
                         THEN NULL ELSE cache.expires_at END \
                     RETURNING value",
                )
                .bind(key)
                .bind(by.to_string())
                .bind(now)
                .bind(by)
                .bind(now)
                .scalar(db)
                .await?;
                Ok(next
                    .parse()
                    .map_err(|_| anyhow::anyhow!("the cached `{key}` is not a whole number"))?)
            }
        }
    }

    /// `increment(key, -by)`.
    pub async fn decrement(&self, key: &str, by: i64) -> Result<i64> {
        self.increment(key, -by).await
    }

    /// A lock named `name`, held for `ttl` (up to a second more, never
    /// less) once acquired unless released earlier, so a crashed holder
    /// doesn't block others forever. With the database store
    /// it works across processes and servers; with the memory store, within
    /// this process.
    pub fn lock(&self, name: &str, ttl: Duration) -> Lock {
        Lock {
            cache: self.clone(),
            key: format!("{LOCK_PREFIX}{name}"),
            ttl: ttl.max(Duration::from_secs(1)),
        }
    }

    /// Deletes expired entries; returns how many (the database store's
    /// count; the memory store returns 0 after sweeping).
    pub async fn prune(&self) -> Result<u64> {
        let now = unix_now();
        match &self.store {
            Store::Memory(map) => {
                map.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .retain(|_, (_, at)| live(*at, now));
                Ok(0)
            }
            Store::Database(db) => Ok(crate::db::sql(
                "DELETE FROM cache WHERE expires_at IS NOT NULL AND expires_at <= ?",
            )
            .bind(now)
            .execute(db)
            .await?),
        }
    }

    /// Prunes expired rows at most once an hour per process.
    async fn prune_now_and_then(&self, db: &Db) {
        static LAST: AtomicI64 = AtomicI64::new(0);
        let now = unix_now();
        let last = LAST.load(Ordering::Relaxed);
        if now - last < PRUNE_EVERY
            || LAST
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        delete_expired(db, now).await;
    }

    /// Deletes `key` if it holds `value`; returns whether it did.
    async fn forget_if(&self, key: &str, value: &Value) -> Result<bool> {
        match &self.store {
            Store::Memory(map) => {
                let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
                if map.get(key).is_some_and(|(held, _)| held == value) {
                    map.remove(key);
                    return Ok(true);
                }
                Ok(false)
            }
            Store::Database(db) => Ok(crate::db::sql(
                "DELETE FROM cache WHERE key = ? AND value = ?",
            )
            .bind(key)
            .bind(value.to_string())
            .execute(db)
            .await?
                == 1),
        }
    }

    /// Returns the cached value, or runs `compute`, caches its result for
    /// `ttl` and returns it. Errors are not cached. Concurrent calls for the
    /// same key in this process compute once; the others wait for it. A
    /// cached value of another type (e.g. after a deploy) is computed again.
    pub async fn remember<T, F, Fut>(&self, key: &str, ttl: Duration, compute: F) -> Result<T>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        if let Some(value) = self.cached(key).await? {
            return Ok(value);
        }
        let lock = self.computing_lock(key);
        let _computing = lock.lock().await;
        if let Some(value) = self.cached(key).await? {
            return Ok(value);
        }
        let value = compute().await?;
        self.put(key, &value, Some(ttl)).await?;
        Ok(value)
    }

    /// Like `get`, but a value of another type is a miss.
    async fn cached<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        Ok(self
            .raw(key)
            .await?
            .and_then(|v| serde_json::from_value(v).ok()))
    }

    fn computing_lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.computing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
            return lock;
        }
        locks.retain(|_, lock| lock.strong_count() > 0);
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(key.to_owned(), Arc::downgrade(&lock));
        lock
    }

    /// Removes `key` (nothing happens if it's missing).
    pub async fn forget(&self, key: &str) -> Result {
        match &self.store {
            Store::Memory(map) => {
                map.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
            }
            Store::Database(db) => {
                crate::db::sql("DELETE FROM cache WHERE key = ?")
                    .bind(key)
                    .execute(db)
                    .await?;
            }
        }
        Ok(())
    }

    /// Removes everything the app cached (not the framework's `renox:` rows).
    pub async fn flush(&self) -> Result {
        match &self.store {
            Store::Memory(map) => map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|key, _| key.starts_with(FRAMEWORK_PREFIX)),
            Store::Database(db) => {
                crate::db::sql("DELETE FROM cache WHERE key NOT LIKE ?")
                    .bind(format!("{FRAMEWORK_PREFIX}%"))
                    .execute(db)
                    .await?;
            }
        }
        Ok(())
    }
}

/// A named lock from [`Cache::lock`].
#[derive(Clone)]
pub struct Lock {
    cache: Cache,
    key: String,
    ttl: Duration,
}

/// A held [`Lock`]. Released by [`LockGuard::release`], when dropped (in the
/// background), or when its `ttl` runs out.
#[must_use = "the lock is released when the guard is dropped"]
pub struct LockGuard {
    cache: Cache,
    key: String,
    owner: Value,
    released: bool,
}

impl Lock {
    /// Takes the lock if it's free.
    pub async fn try_acquire(&self) -> Result<Option<LockGuard>> {
        let owner = Value::String(crate::crypto::random_token());
        // Expiry is kept in whole seconds, so a second more: never less than `ttl`.
        let ttl = self.ttl + Duration::from_secs(1);
        if self.cache.add(&self.key, &owner, Some(ttl)).await? {
            return Ok(Some(LockGuard {
                cache: self.cache.clone(),
                key: self.key.clone(),
                owner,
                released: false,
            }));
        }
        Ok(None)
    }

    /// Waits up to `wait` for the lock, then gives up with a 423 Locked
    /// error.
    pub async fn block(&self, wait: Duration) -> Result<LockGuard> {
        let deadline = tokio::time::Instant::now() + wait;
        let mut pause = Duration::from_millis(25);
        loop {
            if let Some(guard) = self.try_acquire().await? {
                return Ok(guard);
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                let name = self.key.trim_start_matches(LOCK_PREFIX);
                return Err(crate::abort(
                    axum::http::StatusCode::LOCKED,
                    format!("the lock `{name}` is held elsewhere"),
                ));
            }
            tokio::time::sleep(pause.min(deadline - now)).await;
            pause = (pause * 2).min(Duration::from_millis(250));
        }
    }

    /// Whether someone holds the lock now.
    pub async fn is_held(&self) -> Result<bool> {
        self.cache.has(&self.key).await
    }

    /// Releases the lock whoever holds it (e.g. from an admin command).
    pub async fn force_release(&self) -> Result {
        self.cache.forget(&self.key).await
    }
}

impl LockGuard {
    /// Releases the lock now, if this guard still holds it; returns whether
    /// it did (false when the `ttl` ran out and someone else took it).
    pub async fn release(mut self) -> Result<bool> {
        self.released = true;
        self.cache.forget_if(&self.key, &self.owner).await
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let (cache, key, owner) = (
            self.cache.clone(),
            std::mem::take(&mut self.key),
            self.owner.take(),
        );
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(err) = cache.forget_if(&key, &owner).await {
                    tracing::warn!(lock = %key, error = ?err, "could not release a lock");
                }
            });
        }
    }
}

/// Deletes the rows expired at `now`; a failure is logged, not returned
/// (pruning is housekeeping).
async fn delete_expired(db: &Db, now: i64) {
    let pruned =
        crate::db::sql("DELETE FROM cache WHERE expires_at IS NOT NULL AND expires_at <= ?")
            .bind(now)
            .execute(db)
            .await;
    if let Err(err) = pruned {
        tracing::warn!(error = %err, "could not prune expired cache rows");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> Db {
        crate::db::connect(&crate::Config::default()).await.unwrap()
    }

    /// Past `SWEEP_AT` keys, the memory store drops what expired.
    #[tokio::test]
    async fn the_memory_store_sweeps_expired_values_once_it_is_large() {
        let cache = Cache::new(crate::CacheStore::Memory, db().await).unwrap();
        for i in 0..SWEEP_AT {
            cache
                .put(&format!("k{i}"), &i, Some(Duration::from_secs(1)))
                .await
                .unwrap();
        }
        crate::clock::with_offset(10, cache.put("late", &1, None))
            .await
            .unwrap();
        let Store::Memory(map) = &cache.store else {
            unreachable!()
        };
        assert_eq!(map.lock().unwrap().len(), 1);
    }

    /// The database store when its table is missing or goes away: writing
    /// is an error (after a failed prune, logged), and a lock whose release
    /// fails stays held until its time runs out.
    #[tokio::test]
    async fn the_database_store_without_its_table() {
        let (logs, _logged) = crate::test_logs::capture();
        let db = db().await;
        let cache = Cache::new(crate::CacheStore::Database, db.clone()).unwrap();
        assert!(cache.put("k", &1, None).await.is_err());
        delete_expired(&db, 0).await;
        assert!(
            logs.has(&["could not prune expired cache rows"]),
            "{}",
            logs.text()
        );

        crate::db::sql("CREATE TABLE cache (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL, expires_at BIGINT)")
            .execute(&db)
            .await
            .unwrap();
        let lock = cache.lock("report", Duration::from_secs(60));
        let guard = lock.try_acquire().await.unwrap().expect("free");
        crate::db::sql("ALTER TABLE cache RENAME TO cache_away")
            .execute(&db)
            .await
            .unwrap();
        drop(guard);
        tokio::time::sleep(Duration::from_millis(100)).await;
        crate::db::sql("ALTER TABLE cache_away RENAME TO cache")
            .execute(&db)
            .await
            .unwrap();
        assert!(lock.is_held().await.unwrap());
        assert!(logs.has(&["could not release a lock"]), "{}", logs.text());
    }
}
