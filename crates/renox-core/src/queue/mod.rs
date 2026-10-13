//! Background jobs stored in SQLite, with retries and a failed-jobs table.
//!
//! ```
//! # use renox::prelude::*;
//! # #[derive(Model, serde::Serialize, Default)] struct Order { id: i64 }
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct SendReceipt { order_id: i64 }
//!
//! impl Job for SendReceipt {
//!     const NAME: &'static str = "send-receipt";
//!     const MAX_ATTEMPTS: u32 = 5;
//!
//!     async fn handle(self, ctx: JobContext) -> Result {
//!         let order = Order::find_or_404(&ctx.state.db, self.order_id).await?;
//!         // … send it
//! #       let _ = order;
//!         Ok(())
//!     }
//! }
//!
//! # let _ =
//! App::new().job::<SendReceipt>();            // register the handler
//! # async fn demo(state: AppState, order_id: i64) -> Result {
//! state.dispatch(SendReceipt { order_id }).await?;   // in a handler
//! # Ok(()) }
//! ```
//!
//! `serve` runs workers in the same process (`QUEUE_WORKERS`, default 2; 0 to
//! turn off); `my-app queue:work` runs them on their own, and
//! `queue:work --queue high,default` drains `high` before `default`.
//!
//! More than one job at a time:
//!
//! ```
//! # use renox::prelude::*;
//! # #[derive(serde::Serialize, serde::Deserialize)] struct Import { file: String }
//! # impl Job for Import { const NAME: &'static str = "import"; async fn handle(self, _: JobContext) -> Result { Ok(()) } }
//! # #[derive(serde::Serialize, serde::Deserialize)] struct Notify { user_id: i64 }
//! # impl Job for Notify { const NAME: &'static str = "notify"; async fn handle(self, _: JobContext) -> Result { Ok(()) } }
//! # async fn demo(state: AppState) -> Result {
//! // One after another; a failure stops the rest.
//! state.queue.chain()
//!     .then(Import { file: "a.csv".into() })
//!     .then(Notify { user_id: 7 })
//!     .dispatch()
//!     .await?;
//!
//! // Side by side, with progress and follow-up jobs.
//! let batch = state.queue.batch("import-october")
//!     .push(Import { file: "a.csv".into() })
//!     .push(Import { file: "b.csv".into() })
//!     .then(Notify { user_id: 7 })   // all succeeded
//!     .catch(Notify { user_id: 1 })  // the first failure (which cancels the rest)
//!     .dispatch()
//!     .await?;
//! let status = state.queue.batch_status(batch).await?; // total, pending, failed, progress()
//! # let _ = status; Ok(()) }
//! ```

mod dashboard;
mod worker;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

pub use dashboard::{Dashboard, GATE as DASHBOARD_GATE, QueueCounts, QueueStats};
pub use worker::Worker;

use crate::db::{Db, Migration, Transaction};
use crate::{AppState, Error, Result};

pub(crate) const MIGRATIONS: &[Migration] = &[
    crate::db::framework_migration!("queue", "00010101000100_create_jobs_table"),
    crate::db::framework_migration!("queue", "00010101000110_add_chains_and_batches_to_jobs"),
    crate::db::framework_migration!("queue", "00010101000120_add_callback_of_to_jobs"),
];

/// Payloads of `ENCRYPTED` jobs start with this; JSON never does.
const SEALED: &str = "enc:";
/// Cache rows claiming a unique job, holding its id.
const UNIQUE_PREFIX: &str = "renox:unique:";

/// A unit of background work. It is stored as JSON, so keep it to ids and
/// small values rather than whole models.
pub trait Job: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// A stable name; stored with each job, so don't rename it while jobs are queued.
    const NAME: &'static str;
    /// Its queue; `queue:work --queue high,default` drains queues in order.
    const QUEUE: &'static str = "default";
    /// Runs before the job is moved to `failed_jobs`.
    const MAX_ATTEMPTS: u32 = 3;
    /// How long one attempt may run.
    const TIMEOUT: Duration = Duration::from_secs(60);
    /// Store the payload encrypted with `APP_KEY` (it holds personal data or
    /// secrets). The worker decrypts it; `failed_jobs` keeps it encrypted.
    const ENCRYPTED: bool = false;
    /// While a job with the same [`Job::unique_id`] is queued or running,
    /// dispatching another returns the queued one's id instead, for up to
    /// this long (the claim is dropped when the job finishes).
    const UNIQUE_FOR: Option<Duration> = None;

    /// Wait before retrying after the given failed attempt (1-based).
    fn backoff(attempt: u32) -> Duration {
        Duration::from_secs(10 * u64::from(attempt))
    }

    /// What makes two jobs "the same" for [`Job::UNIQUE_FOR`], e.g. the
    /// order id. Empty (the default) makes every job of the type the same.
    fn unique_id(&self) -> String {
        String::new()
    }

    /// Conditions checked before each attempt; a job that can't run yet is
    /// put back without using up an attempt.
    fn middleware(&self) -> Vec<Middleware> {
        Vec::new()
    }

    /// Does the work; an error fails the attempt (retried unless `Error::permanent`).
    fn handle(self, ctx: JobContext) -> impl Future<Output = Result> + Send;

    /// Runs once the job has failed for good (attempts used up, or a
    /// permanent error), with the last error, e.g. to tell the user. `ctx`
    /// is the last attempt's: the state, the job's id, the attempt, its batch.
    fn failed(self, ctx: JobContext, error: String) -> impl Future<Output = ()> + Send {
        let _ = (ctx, error);
        async {}
    }
}

/// A condition checked before a job's attempt; see [`Job::middleware`].
///
/// ```
/// # use renox::prelude::*;
/// use renox::queue::Middleware;
/// # use std::time::Duration;
/// # #[derive(serde::Serialize, serde::Deserialize)] struct SyncStock { shop_id: i64 }
/// impl Job for SyncStock {
///     const NAME: &'static str = "sync-stock";
///
///     fn middleware(&self) -> Vec<Middleware> {
///         vec![
///             // One sync per shop at a time; others wait 10 s and try again.
///             Middleware::without_overlapping(format!("shop:{}", self.shop_id))
///                 .release_after(Duration::from_secs(10)),
///             // The supplier's API allows 60 calls a minute.
///             Middleware::rate_limited("supplier-api", 60, Duration::from_secs(60)),
///         ]
///     }
///
///     async fn handle(self, _ctx: JobContext) -> Result { Ok(()) }
/// }
/// ```
///
/// Both use the cache (`CACHE_STORE`): with `memory`, they hold within one
/// process; use `database` when several processes run workers.
#[derive(Debug, Clone)]
pub struct Middleware(MiddlewareKind);

#[derive(Debug, Clone)]
enum MiddlewareKind {
    WithoutOverlapping {
        key: String,
        release_after: Duration,
    },
    RateLimited {
        key: String,
        max: u32,
        per: Duration,
    },
}

impl Middleware {
    /// One job with this `key` at a time; others are put back for 5 seconds
    /// (see [`Middleware::release_after`]).
    pub fn without_overlapping(key: impl Into<String>) -> Self {
        Self(MiddlewareKind::WithoutOverlapping {
            key: key.into(),
            release_after: Duration::from_secs(5),
        })
    }

    /// At most `max` attempts with this `key` per `per` (a fixed window);
    /// the rest are put back until the window ends.
    pub fn rate_limited(key: impl Into<String>, max: u32, per: Duration) -> Self {
        Self(MiddlewareKind::RateLimited {
            key: key.into(),
            max,
            per: per.max(Duration::from_secs(1)),
        })
    }

    /// How long a job waits before trying again when it would overlap.
    pub fn release_after(mut self, wait: Duration) -> Self {
        if let MiddlewareKind::WithoutOverlapping { release_after, .. } = &mut self.0 {
            *release_after = wait;
        }
        self
    }
}

/// What a job gets when it runs.
#[non_exhaustive]
pub struct JobContext {
    /// The app's state (database, mailer, queue…).
    pub state: AppState,
    /// 1 on the first try.
    pub attempt: u32,
    /// The job's id (0 for [`AppState::dispatch_sync`]).
    pub id: i64,
    /// The batch it belongs to, or, for a batch's `then`/`catch`/`finally`
    /// job, the batch it follows (read it with `queue.batch_status`).
    pub batch_id: Option<i64>,
}

type RunFn =
    Arc<dyn Fn(String, JobContext) -> Pin<Box<dyn Future<Output = Result> + Send>> + Send + Sync>;
type FailedFn = Arc<
    dyn Fn(String, JobContext, String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

/// A registered job type: how to run it and when to retry it.
#[derive(Clone)]
pub(crate) struct JobHandler {
    pub(crate) run: RunFn,
    pub(crate) failed: FailedFn,
    pub(crate) backoff: fn(u32) -> Duration,
    pub(crate) timeout: Duration,
    /// The unique claim's cache key, from the (decrypted) payload.
    pub(crate) unique_key: fn(&str) -> Option<String>,
    pub(crate) middleware: fn(&str) -> Vec<Middleware>,
}

/// The handler for `J`; `Registry::job` registers it.
pub(crate) fn handler<J: Job>() -> JobHandler {
    JobHandler {
        run: Arc::new(|payload, ctx| {
            Box::pin(async move {
                // A payload that doesn't decode now never will.
                let job: J = serde_json::from_str(&payload).map_err(crate::Error::permanent)?;
                job.handle(ctx).await
            })
        }),
        failed: Arc::new(|payload, ctx, error| {
            Box::pin(async move {
                if let Ok(job) = serde_json::from_str::<J>(&payload) {
                    job.failed(ctx, error).await;
                }
            })
        }),
        backoff: J::backoff,
        timeout: J::TIMEOUT,
        unique_key: |payload| {
            J::UNIQUE_FOR?;
            let job: J = serde_json::from_str(payload).ok()?;
            Some(unique_key::<J>(&job))
        },
        middleware: |payload| {
            serde_json::from_str::<J>(payload)
                .map(|job| job.middleware())
                .unwrap_or_default()
        },
    }
}

fn unique_key<J: Job>(job: &J) -> String {
    format!("{UNIQUE_PREFIX}{}:{}", J::NAME, job.unique_id())
}

pub(crate) type Handlers = Arc<HashMap<&'static str, JobHandler>>;

/// A job ready to be stored: for chains, batches and their follow-ups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Encoded {
    queue: String,
    job: String,
    payload: String,
    max_attempts: u32,
}

/// Dispatches jobs and wakes the workers of this process.
#[derive(Clone)]
pub struct Queue {
    db: Db,
    key: cookie::Key,
    wake: Arc<Notify>,
}

pub(crate) fn unix_now() -> i64 {
    crate::clock::unix_secs()
}

impl Queue {
    pub(crate) fn new(db: Db, key: cookie::Key) -> Self {
        Self {
            db,
            key,
            wake: Arc::new(Notify::new()),
        }
    }

    pub(crate) fn wake(&self) -> &Notify {
        &self.wake
    }

    pub(crate) fn wake_workers(&self) {
        self.wake.notify_waiters();
    }

    fn encode<J: Job>(&self, job: &J, queue: &str) -> Result<Encoded> {
        let json = serde_json::to_string(job)?;
        let payload = if J::ENCRYPTED {
            format!("{SEALED}{}", crate::crypto::seal(&self.key, &json))
        } else {
            json
        };
        Ok(Encoded {
            queue: queue.to_owned(),
            job: J::NAME.to_owned(),
            payload,
            max_attempts: J::MAX_ATTEMPTS.max(1),
        })
    }

    /// The JSON of a stored payload, decrypting an encrypted one.
    pub(crate) fn open(&self, payload: &str) -> Result<String> {
        match payload.strip_prefix(SEALED) {
            Some(sealed) => Ok(crate::crypto::open(&self.key, sealed).map_err(Error::permanent)?),
            None => Ok(payload.to_owned()),
        }
    }

    /// Queues a job to run as soon as a worker is free; returns its id.
    pub async fn dispatch<J: Job>(&self, job: J) -> Result<i64> {
        self.dispatch_after(job, Duration::ZERO).await
    }

    /// Queues a job to run after `delay`; returns its id.
    pub async fn dispatch_after<J: Job>(&self, job: J, delay: Duration) -> Result<i64> {
        self.push(job, J::QUEUE, delay).await
    }

    /// Queues a job on `queue` instead of its [`Job::QUEUE`].
    pub async fn dispatch_on<J: Job>(&self, queue: &str, job: J) -> Result<i64> {
        self.push(job, queue, Duration::ZERO).await
    }

    async fn push<J: Job>(&self, job: J, queue: &str, delay: Duration) -> Result<i64> {
        let encoded = self.encode(&job, queue)?;
        let id = match J::UNIQUE_FOR {
            None => insert(&self.db, &encoded, delay, None, None, None).await?,
            Some(ttl) => {
                let mut tx = self.db.begin().await?;
                let id =
                    insert_unique(&mut tx, &unique_key::<J>(&job), ttl, &encoded, delay).await?;
                tx.commit().await?;
                id
            }
        };
        self.wake.notify_waiters();
        Ok(id)
    }

    /// Queues a job inside `tx`, so it only exists if the transaction
    /// commits; workers pick it up within a second of the commit. Use it
    /// instead of `dispatch` while a transaction is open: on SQLite, which
    /// writes one transaction at a time, `dispatch` would wait for `tx`.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(serde::Serialize, serde::Deserialize)] struct SendReceipt { order_id: i64 }
    /// # impl Job for SendReceipt { const NAME: &'static str = "r"; async fn handle(self, _: JobContext) -> Result { Ok(()) } }
    /// # async fn demo(state: AppState) -> Result {
    /// let mut tx = state.db.begin().await?;
    /// let order_id: i64 = renox::db::sql("INSERT INTO orders (total) VALUES (?) RETURNING id")
    ///     .bind(75_000)
    ///     .scalar(&mut tx)
    ///     .await?;
    /// state.queue.dispatch_in(&mut tx, SendReceipt { order_id }).await?;
    /// tx.commit().await?; // no order, no receipt
    /// # Ok(()) }
    /// ```
    pub async fn dispatch_in<J: Job>(&self, tx: &mut Transaction, job: J) -> Result<i64> {
        let encoded = self.encode(&job, J::QUEUE)?;
        match J::UNIQUE_FOR {
            None => insert(tx, &encoded, Duration::ZERO, None, None, None).await,
            Some(ttl) => {
                insert_unique(tx, &unique_key::<J>(&job), ttl, &encoded, Duration::ZERO).await
            }
        }
    }

    /// Jobs that run one after another: the next is queued when the one
    /// before succeeds, and a job that fails for good stops the chain
    /// (`queue:retry` resumes it).
    pub fn chain(&self) -> Chain {
        Chain {
            queue: self.clone(),
            jobs: Vec::new(),
            error: None,
        }
    }

    /// Jobs that run side by side, tracked together; see [`Batch`].
    pub fn batch(&self, name: &str) -> Batch {
        Batch {
            queue: self.clone(),
            name: name.to_owned(),
            jobs: Vec::new(),
            then: None,
            catch: None,
            finally: None,
            allow_failures: false,
            error: None,
        }
    }

    /// How a batch is doing; `None` if there's no such batch.
    pub async fn batch_status(&self, id: i64) -> Result<Option<BatchStatus>> {
        let row = crate::db::sql(
            "SELECT id, name, total, pending, failed, cancelled_at, finished_at, created_at \
             FROM job_batches WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(BatchStatus {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            total: row.try_get("total")?,
            pending: row.try_get("pending")?,
            failed: row.try_get("failed")?,
            cancelled: row.try_get::<Option<i64>>("cancelled_at")?.is_some(),
            finished: row.try_get::<Option<i64>>("finished_at")?.is_some(),
            created_at: crate::db::from_unix(row.try_get("created_at")?),
        }))
    }

    /// Cancels a batch: its jobs that haven't run are skipped.
    pub async fn cancel_batch(&self, id: i64) -> Result<bool> {
        Ok(crate::db::sql(
            "UPDATE job_batches SET cancelled_at = ? WHERE id = ? AND cancelled_at IS NULL",
        )
        .bind(unix_now())
        .bind(id)
        .execute(&self.db)
        .await?
            == 1)
    }

    /// Jobs waiting or running.
    pub async fn pending(&self) -> Result<i64> {
        Ok(crate::db::sql("SELECT COUNT(*) FROM jobs")
            .scalar(&self.db)
            .await?)
    }

    /// Jobs that failed for good, oldest first.
    pub async fn failed(&self) -> Result<Vec<FailedJob>> {
        let rows = crate::db::sql(
            "SELECT id, queue, job, payload, error, failed_at FROM failed_jobs ORDER BY id",
        )
        .fetch_all(&self.db)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(FailedJob {
                    id: r.try_get("id")?,
                    queue: r.try_get("queue")?,
                    job: r.try_get("job")?,
                    payload: r.try_get("payload")?,
                    error: r.try_get("error")?,
                    failed_at: crate::db::from_unix(r.try_get("failed_at")?),
                })
            })
            .collect()
    }

    /// Puts the failed job `id` back on its queue with fresh attempts (and
    /// its chain and batch); returns whether there was such a job.
    pub async fn retry(&self, id: i64) -> Result<bool> {
        Ok(self.move_failed(Some(id)).await? == 1)
    }

    /// Puts every failed job back on its queue; returns how many.
    pub async fn retry_all(&self) -> Result<u64> {
        self.move_failed(None).await
    }

    async fn move_failed(&self, id: Option<i64>) -> Result<u64> {
        let mut tx = self.db.begin().await?;
        let filter = if id.is_some() { " WHERE id = ?" } else { "" };
        let mut batches = crate::db::sql(format!(
            "UPDATE job_batches SET failed = failed - 1, pending = pending + 1, finished_at = NULL \
             WHERE id IN (SELECT batch_id FROM failed_jobs{filter})"
        ));
        if let Some(id) = id {
            batches = batches.bind(id);
        }
        batches.execute(&mut tx).await?;
        let insert = format!(
            "INSERT INTO jobs (queue, job, payload, max_attempts, available_at, created_at, chain, \
             batch_id, callback_of) SELECT queue, job, payload, max_attempts, ?, ?, chain, batch_id, \
             callback_of FROM failed_jobs{filter}"
        );
        let mut query = crate::db::sql(insert).bind(unix_now()).bind(unix_now());
        if let Some(id) = id {
            query = query.bind(id);
        }
        let moved = query.execute(&mut tx).await?;
        let mut delete = crate::db::sql(format!("DELETE FROM failed_jobs{filter}"));
        if let Some(id) = id {
            delete = delete.bind(id);
        }
        delete.execute(&mut tx).await?;
        tx.commit().await?;
        self.wake.notify_waiters();
        Ok(moved)
    }

    /// Deletes one failed job; returns whether it existed.
    pub async fn forget_failed(&self, id: i64) -> Result<bool> {
        Ok(crate::db::sql("DELETE FROM failed_jobs WHERE id = ?")
            .bind(id)
            .execute(&self.db)
            .await?
            == 1)
    }

    /// Deletes failed jobs older than `age`; returns how many.
    pub async fn prune_failed(&self, age: Duration) -> Result<u64> {
        Ok(
            crate::db::sql("DELETE FROM failed_jobs WHERE failed_at < ?")
                .bind(unix_now() - age.as_secs() as i64)
                .execute(&self.db)
                .await?,
        )
    }

    /// Deletes batches that finished (or were cancelled) more than `age` ago.
    pub async fn prune_batches(&self, age: Duration) -> Result<u64> {
        let before = unix_now() - age.as_secs() as i64;
        Ok(crate::db::sql(
            "DELETE FROM job_batches WHERE (finished_at IS NOT NULL AND finished_at < ?) \
             OR (cancelled_at IS NOT NULL AND cancelled_at < ? AND pending = 0)",
        )
        .bind(before)
        .bind(before)
        .execute(&self.db)
        .await?)
    }

    /// Deletes failed jobs.
    pub async fn flush_failed(&self) -> Result<u64> {
        Ok(crate::db::sql("DELETE FROM failed_jobs")
            .execute(&self.db)
            .await?)
    }
}

/// Stores one job; returns its id.
async fn insert<'c>(
    db: impl crate::db::Executor<'c>,
    job: &Encoded,
    delay: Duration,
    chain: Option<String>,
    batch_id: Option<i64>,
    callback_of: Option<i64>,
) -> Result<i64> {
    let now = unix_now();
    let id: i64 = crate::db::sql(
        "INSERT INTO jobs (queue, job, payload, max_attempts, available_at, created_at, chain, \
         batch_id, callback_of) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&job.queue)
    .bind(&job.job)
    .bind(&job.payload)
    .bind(i64::from(job.max_attempts))
    .bind(now + delay.as_secs() as i64)
    .bind(now)
    .bind(chain)
    .bind(batch_id)
    .bind(callback_of)
    .scalar(db)
    .await?;
    Ok(id)
}

/// Stores a unique job unless its claim is held; returns its id or the
/// holder's. The claim is a `cache` row (whatever `CACHE_STORE` is), so it
/// holds across processes.
async fn insert_unique(
    tx: &mut Transaction,
    key: &str,
    ttl: Duration,
    job: &Encoded,
    delay: Duration,
) -> Result<i64> {
    let now = unix_now();
    let claimed = crate::db::sql(
        "INSERT INTO cache (key, value, expires_at) VALUES (?, '0', ?) \
         ON CONFLICT (key) DO UPDATE SET value = '0', expires_at = excluded.expires_at \
         WHERE cache.expires_at IS NOT NULL AND cache.expires_at <= ?",
    )
    .bind(key)
    .bind(now + ttl.as_secs().max(1) as i64)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    if claimed == 0 {
        let holder: String = crate::db::sql("SELECT value FROM cache WHERE key = ?")
            .bind(key)
            .scalar(&mut *tx)
            .await?;
        return Ok(holder.parse().unwrap_or_default());
    }
    let id = insert(&mut *tx, job, delay, None, None, None).await?;
    crate::db::sql("UPDATE cache SET value = ? WHERE key = ?")
        .bind(id.to_string())
        .bind(key)
        .execute(&mut *tx)
        .await?;
    Ok(id)
}

/// Jobs that run one after another; from [`Queue::chain`].
#[must_use = "a chain does nothing until dispatched"]
pub struct Chain {
    queue: Queue,
    jobs: Vec<Encoded>,
    error: Option<Error>,
}

impl Chain {
    /// Adds a job to the end of the chain.
    pub fn then<J: Job>(mut self, job: J) -> Self {
        match self.queue.encode(&job, J::QUEUE) {
            Ok(encoded) => self.jobs.push(encoded),
            Err(err) => {
                self.error.get_or_insert(err);
            }
        }
        self
    }

    /// Queues the first job; returns its id (0 for an empty chain).
    pub async fn dispatch(self) -> Result<i64> {
        if let Some(err) = self.error {
            return Err(err);
        }
        let mut jobs = self.jobs.into_iter();
        let Some(first) = jobs.next() else {
            return Ok(0);
        };
        let rest: Vec<Encoded> = jobs.collect();
        let chain = (!rest.is_empty())
            .then(|| serde_json::to_string(&rest))
            .transpose()?;
        let id = insert(&self.queue.db, &first, Duration::ZERO, chain, None, None).await?;
        self.queue.wake_workers();
        Ok(id)
    }
}

/// Jobs that run side by side and are tracked together; from
/// [`Queue::batch`]. The first job that fails for good cancels the batch
/// (jobs that haven't run are skipped) unless [`Batch::allow_failures`].
#[must_use = "a batch does nothing until dispatched"]
pub struct Batch {
    queue: Queue,
    name: String,
    jobs: Vec<Encoded>,
    then: Option<Encoded>,
    catch: Option<Encoded>,
    finally: Option<Encoded>,
    allow_failures: bool,
    error: Option<Error>,
}

impl Batch {
    fn encode<J: Job>(&mut self, job: J) -> Option<Encoded> {
        match self.queue.encode(&job, J::QUEUE) {
            Ok(encoded) => Some(encoded),
            Err(err) => {
                self.error.get_or_insert(err);
                None
            }
        }
    }

    /// Adds a job to the batch.
    pub fn push<J: Job>(mut self, job: J) -> Self {
        if let Some(job) = self.encode(job) {
            self.jobs.push(job);
        }
        self
    }

    /// Queued when every job has succeeded.
    pub fn then<J: Job>(mut self, job: J) -> Self {
        self.then = self.encode(job);
        self
    }

    /// Queued when the first job fails for good.
    pub fn catch<J: Job>(mut self, job: J) -> Self {
        self.catch = self.encode(job);
        self
    }

    /// Queued when every job has run or been skipped, however it went.
    pub fn finally<J: Job>(mut self, job: J) -> Self {
        self.finally = self.encode(job);
        self
    }

    /// A job failing for good doesn't cancel the others.
    pub fn allow_failures(mut self) -> Self {
        self.allow_failures = true;
        self
    }

    /// Stores the batch and its jobs; returns the batch id.
    pub async fn dispatch(self) -> Result<i64> {
        if let Some(err) = self.error {
            return Err(err);
        }
        let json = |job: &Option<Encoded>| job.as_ref().map(serde_json::to_string).transpose();
        let (then, catch, finally) = (json(&self.then)?, json(&self.catch)?, json(&self.finally)?);
        let total = self.jobs.len() as i64;
        let mut tx = self.queue.db.begin().await?;
        let id: i64 = crate::db::sql(
            "INSERT INTO job_batches (name, total, pending, allow_failures, then_job, catch_job, \
             finally_job, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(&self.name)
        .bind(total)
        .bind(total)
        .bind(self.allow_failures)
        .bind(then)
        .bind(catch)
        .bind(finally)
        .bind(unix_now())
        .scalar(&mut tx)
        .await?;
        for job in &self.jobs {
            insert(&mut tx, job, Duration::ZERO, None, Some(id), None).await?;
        }
        if self.jobs.is_empty() {
            finish_batch(&mut tx, id, 0).await?;
        }
        tx.commit().await?;
        self.queue.wake_workers();
        Ok(id)
    }
}

/// Counts one of a batch's jobs as done (`failed`: for good) and queues the
/// follow-up jobs that are due.
pub(crate) async fn batch_job_done(tx: &mut Transaction, batch_id: i64, failed: bool) -> Result {
    let row: Option<(i64, i64, Option<String>)> = crate::db::sql(
        "UPDATE job_batches SET pending = pending - 1, failed = failed + ?, \
         cancelled_at = CASE WHEN ? AND NOT allow_failures THEN COALESCE(cancelled_at, ?) \
         ELSE cancelled_at END \
         WHERE id = ? RETURNING pending, failed, catch_job",
    )
    .bind(i64::from(failed))
    .bind(failed)
    .bind(unix_now())
    .bind(batch_id)
    .fetch_as(&mut *tx)
    .await?
    .into_iter()
    .next();
    let Some((pending, failures, catch)) = row else {
        return Ok(()); // pruned meanwhile
    };
    if failed && failures == 1 {
        queue_follow_up(tx, catch, batch_id).await?;
    }
    if pending <= 0 {
        finish_batch(tx, batch_id, failures).await?;
    }
    Ok(())
}

async fn finish_batch(tx: &mut Transaction, batch_id: i64, failures: i64) -> Result {
    let (then, finally, cancelled): (Option<String>, Option<String>, Option<i64>) = crate::db::sql(
        "UPDATE job_batches SET finished_at = ? WHERE id = ? \
         RETURNING then_job, finally_job, cancelled_at",
    )
    .bind(unix_now())
    .bind(batch_id)
    .fetch_as(&mut *tx)
    .await?
    .into_iter()
    .next()
    .unwrap_or_default();
    if failures == 0 && cancelled.is_none() {
        queue_follow_up(tx, then, batch_id).await?;
    }
    queue_follow_up(tx, finally, batch_id).await
}

/// Queues a batch's `then`/`catch`/`finally` job, which sees the batch in
/// `JobContext::batch_id` but isn't counted in it.
async fn queue_follow_up(tx: &mut Transaction, job: Option<String>, batch_id: i64) -> Result {
    if let Some(job) = job {
        let job: Encoded = serde_json::from_str(&job)?;
        insert(&mut *tx, &job, Duration::ZERO, None, None, Some(batch_id)).await?;
    }
    Ok(())
}

/// Queues the next job of a chain, handing it the rest.
pub(crate) async fn continue_chain(tx: &mut Transaction, chain: &str) -> Result {
    let rest: Vec<Encoded> = serde_json::from_str(chain)?;
    let mut rest = rest.into_iter();
    if let Some(next) = rest.next() {
        let rest: Vec<Encoded> = rest.collect();
        let chain = (!rest.is_empty())
            .then(|| serde_json::to_string(&rest))
            .transpose()?;
        insert(&mut *tx, &next, Duration::ZERO, chain, None, None).await?;
    }
    Ok(())
}

/// Drops a unique job's claim.
pub(crate) async fn release_unique(tx: &mut Transaction, key: &str) -> Result {
    crate::db::sql("DELETE FROM cache WHERE key = ?")
        .bind(key)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// How a batch is doing; from [`Queue::batch_status`].
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct BatchStatus {
    /// The `job_batches` row id.
    pub id: i64,
    /// The name given to [`Queue::batch`].
    pub name: String,
    /// Jobs in the batch.
    pub total: i64,
    /// Jobs not run yet (or being retried).
    pub pending: i64,
    /// Jobs that failed for good.
    pub failed: i64,
    /// Cancelled by a failure (without `allow_failures`) or `cancel_batch`.
    pub cancelled: bool,
    /// Every job has run or been skipped.
    pub finished: bool,
    /// When it was made.
    pub created_at: crate::db::DateTime,
}

impl BatchStatus {
    /// Percent of the jobs that have run, 0–100.
    pub fn progress(&self) -> u8 {
        if self.total == 0 {
            return 100;
        }
        ((self.total - self.pending) * 100 / self.total).clamp(0, 100) as u8
    }
}

/// A job that used up its attempts.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct FailedJob {
    /// The `failed_jobs` row id, for `queue:retry` and `queue:forget`.
    pub id: i64,
    /// The queue it ran on.
    pub queue: String,
    /// The job type's [`Job::NAME`].
    pub job: String,
    /// The serialized job (JSON, or `enc:…` when encrypted).
    pub payload: String,
    /// The last attempt's error.
    pub error: String,
    /// When it failed for good.
    pub failed_at: crate::db::DateTime,
}

impl AppState {
    /// Shorthand for `state.queue.dispatch(job)`.
    pub async fn dispatch<J: Job>(&self, job: J) -> Result<i64> {
        self.queue.dispatch(job).await
    }

    /// Runs `job` now, in this task, instead of queueing it (no retries,
    /// middleware or `failed` hook): the error, if any, is returned.
    pub async fn dispatch_sync<J: Job>(&self, job: J) -> Result {
        let ctx = JobContext {
            state: self.clone(),
            attempt: 1,
            id: 0,
            batch_id: None,
        };
        crate::context::scope_app(self.clone(), job.handle(ctx)).await
    }
}
