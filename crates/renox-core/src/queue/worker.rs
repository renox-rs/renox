use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinSet;

use super::{Handlers, JobContext, Middleware, MiddlewareKind, unix_now};
use crate::AppState;
use crate::db::Dialect;

/// A reserved job older than this is assumed abandoned (e.g. the process
/// crashed) and becomes available again. Jobs with a longer `TIMEOUT` get
/// their reservation extended (`extend_reservation`).
const RESERVATION: i64 = 15 * 60;
const POLL: Duration = Duration::from_secs(1);
/// The error a job gets when its last attempt never finished.
const CRASHED: &str = "the worker stopped during the last attempt (crash, kill or out of memory)";

struct Reserved {
    id: i64,
    queue: String,
    job: String,
    payload: String,
    attempts: u32,
    max_attempts: u32,
    chain: Option<String>,
    batch_id: Option<i64>,
    /// The batch this `then`/`catch`/`finally` job follows.
    callback_of: Option<i64>,
}

/// Runs queued jobs.
#[derive(Clone)]
pub struct Worker {
    state: AppState,
    handlers: Handlers,
    queues: Vec<String>,
    /// Unix seconds of the last `sweep_exhausted`.
    last_sweep: Arc<AtomicI64>,
}

/// Why an attempt failed, and whether another attempt could succeed.
struct Failure {
    error: String,
    permanent: bool,
}

impl Failure {
    fn retry(error: String) -> Self {
        Self {
            error,
            permanent: false,
        }
    }

    fn permanent(error: String) -> Self {
        Self {
            error,
            permanent: true,
        }
    }
}

/// What became of a reserved job.
enum Outcome {
    Done,
    /// Its batch was cancelled: not run, counted as done.
    Skipped,
    /// Middleware held it back: queued again after the wait, the attempt
    /// not counted.
    Released(Duration),
    Failed(Failure),
}

fn panic_message(err: tokio::task::JoinError) -> String {
    match err.try_into_panic() {
        Ok(panic) => format!("the job panicked: {}", crate::error::panic_message(&*panic)),
        Err(err) => format!("the job was cancelled: {err}"),
    }
}

async fn fail(tx: &mut crate::db::Transaction, job: &Reserved, error: &str) -> crate::Result {
    crate::db::sql(
        "INSERT INTO failed_jobs (queue, job, payload, max_attempts, error, failed_at, chain, \
         batch_id, callback_of) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&job.queue)
    .bind(&job.job)
    .bind(&job.payload)
    .bind(i64::from(job.max_attempts))
    .bind(error)
    .bind(unix_now())
    .bind(job.chain.clone())
    .bind(job.batch_id)
    .bind(job.callback_of)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// Runs a bookkeeping write, retrying for about 10 seconds while the
/// database is briefly unavailable (restarting, or SQLite busy).
async fn retry_write<F, Fut>(mut write: F) -> crate::Result
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = crate::Result>,
{
    let mut delays = [100u64, 400, 1000, 2500, 6000].into_iter();
    loop {
        match write().await {
            Ok(()) => return Ok(()),
            Err(err) => match delays.next() {
                Some(ms) => {
                    tracing::warn!(error = ?err, "queue bookkeeping failed, retrying");
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                }
                None => return Err(err),
            },
        }
    }
}

impl Worker {
    pub(crate) fn new(state: AppState, handlers: Handlers, queues: Vec<String>) -> Self {
        Self {
            state,
            handlers,
            queues,
            last_sweep: Arc::default(),
        }
    }

    async fn reserve(&self) -> crate::Result<Option<Reserved>> {
        let now = unix_now();
        let (queues, order) = if self.queues.is_empty() {
            (String::new(), String::new())
        } else {
            let marks = vec!["?"; self.queues.len()].join(", ");
            // Listed first, drained first.
            let whens: String = (0..self.queues.len())
                .map(|i| format!(" WHEN ? THEN {i}"))
                .collect();
            (
                format!(" AND queue IN ({marks})"),
                format!("CASE queue{whens} END, "),
            )
        };
        // SQLite runs one write at a time, so the UPDATE is enough. On
        // PostgreSQL, workers on several servers must not pick the same row.
        let lock = match self.state.db.dialect() {
            Dialect::Sqlite => "",
            Dialect::Postgres => " FOR UPDATE SKIP LOCKED",
        };
        let sql = format!(
            "UPDATE jobs SET reserved_at = ?, attempts = attempts + 1 WHERE id = (\
                SELECT id FROM jobs WHERE available_at <= ? \
                AND (reserved_at IS NULL OR reserved_at <= ?) \
                AND attempts < max_attempts{queues} \
                ORDER BY {order}available_at, id LIMIT 1{lock}) \
             RETURNING id, queue, job, payload, attempts, max_attempts, chain, batch_id, callback_of"
        );
        let mut query = crate::db::sql(sql)
            .bind(now)
            .bind(now)
            .bind(now - RESERVATION);
        for queue in self.queues.iter().chain(&self.queues) {
            query = query.bind(queue);
        }
        let Some(row) = query.fetch_optional(&self.state.db).await? else {
            return Ok(None);
        };
        Ok(Some(Reserved {
            id: row.try_get("id")?,
            queue: row.try_get("queue")?,
            job: row.try_get("job")?,
            payload: row.try_get("payload")?,
            attempts: row.try_get::<i64>("attempts")? as u32,
            max_attempts: row.try_get::<i64>("max_attempts")? as u32,
            chain: row.try_get("chain")?,
            batch_id: row.try_get("batch_id")?,
            callback_of: row.try_get("callback_of")?,
        }))
    }

    async fn batch_cancelled(&self, batch_id: Option<i64>) -> crate::Result<bool> {
        let Some(id) = batch_id else {
            return Ok(false);
        };
        let cancelled: Option<Option<i64>> =
            crate::db::sql("SELECT cancelled_at FROM job_batches WHERE id = ?")
                .bind(id)
                .scalar_optional(&self.state.db)
                .await?;
        Ok(cancelled.flatten().is_some())
    }

    /// Checks the job's middleware; `Some(wait)` puts it back for `wait`.
    /// A lock it takes is returned to be held while the job runs.
    async fn check_middleware(
        &self,
        middleware: Vec<Middleware>,
        timeout: Duration,
    ) -> crate::Result<(Option<Duration>, Vec<crate::cache::LockGuard>)> {
        let mut guards = Vec::new();
        for Middleware(kind) in middleware {
            match kind {
                MiddlewareKind::WithoutOverlapping { key, release_after } => {
                    let lock = self
                        .state
                        .cache
                        .lock(&format!("overlap:{key}"), timeout + Duration::from_secs(60));
                    match lock.try_acquire().await? {
                        Some(guard) => guards.push(guard),
                        None => return Ok((Some(release_after), guards)),
                    }
                }
                MiddlewareKind::RateLimited { key, max, per } => {
                    let per = per.as_secs() as i64;
                    let now = unix_now();
                    let window = now - now.rem_euclid(per);
                    let counter = format!("renox:rate:{key}:{window}");
                    let cache = &self.state.cache;
                    let ttl = Duration::from_secs(per as u64 + 1);
                    cache.add(&counter, &0, Some(ttl)).await?;
                    if cache.increment(&counter, 1).await? > i64::from(max) {
                        let wait = (window + per - now).max(1) as u64;
                        return Ok((Some(Duration::from_secs(wait)), guards));
                    }
                }
            }
        }
        Ok((None, guards))
    }

    /// Runs one available job; returns whether there was one.
    pub async fn run_next(&self) -> crate::Result<bool> {
        self.sweep_exhausted().await?;
        let Some(job) = self.reserve().await? else {
            return Ok(false);
        };
        let handler = self.handlers.get(job.job.as_str()).cloned();
        if let Some(handler) = &handler {
            self.extend_reservation(&job, handler.timeout).await?;
        }
        let plain = self.state.queue.open(&job.payload);
        let unique = match (&handler, &plain) {
            (Some(handler), Ok(plain)) => (handler.unique_key)(plain),
            _ => None,
        };
        let mut guards = Vec::new();
        let outcome = match (&handler, plain.as_ref()) {
            (None, _) => Outcome::Failed(Failure::permanent(format!(
                "no handler registered for job `{}`",
                job.job
            ))),
            (Some(_), Err(err)) => Outcome::Failed(Failure::permanent(format!("{err:?}"))),
            (Some(_), Ok(_)) if self.batch_cancelled(job.batch_id).await? => Outcome::Skipped,
            (Some(handler), Ok(plain)) => {
                let (release, held) = self
                    .check_middleware((handler.middleware)(plain), handler.timeout)
                    .await?;
                guards = held;
                match release {
                    Some(wait) => Outcome::Released(wait),
                    None => self.attempt(handler, &job, plain.clone()).await,
                }
            }
        };

        // The job ran; don't let a brief database hiccup strand it until its
        // reservation expires.
        let backoff = handler
            .as_ref()
            .map(|h| (h.backoff)(job.attempts))
            .unwrap_or_default();
        retry_write(|| self.record(&job, &outcome, backoff, unique.as_deref())).await?;
        for guard in guards {
            let _ = guard.release().await;
        }
        match outcome {
            Outcome::Done => tracing::info!(job = %job.job, id = job.id, "job done"),
            Outcome::Skipped => {
                tracing::info!(job = %job.job, id = job.id, "job skipped: its batch was cancelled");
            }
            Outcome::Released(wait) => {
                tracing::debug!(job = %job.job, id = job.id, ?wait, "job held back by its middleware");
            }
            Outcome::Failed(failure) if !failure.permanent && job.attempts < job.max_attempts => {
                tracing::warn!(job = %job.job, id = job.id, attempt = job.attempts, error = %failure.error, "job failed, will retry");
            }
            Outcome::Failed(failure) => {
                tracing::error!(job = %job.job, id = job.id, error = %failure.error, "job failed for good");
                self.failed_for_good(&job, plain.ok(), failure.error).await;
            }
        }
        Ok(true)
    }

    /// Reports a job that failed for good (`App::report`) and runs its
    /// `failed` hook (when its handler is known and its payload opens).
    async fn failed_for_good(&self, job: &Reserved, plain: Option<String>, error: String) {
        let report = crate::report::ErrorReport::new(
            &self.state,
            crate::report::ReportKind::Job,
            error.lines().next().unwrap_or_default().to_owned(),
            error.clone(),
            Some(format!("{} #{}", job.job, job.id)),
        );
        crate::report::send(&self.state, report);
        if let (Some(handler), Some(plain)) = (self.handlers.get(job.job.as_str()), plain) {
            let ctx = JobContext {
                state: self.state.clone(),
                attempt: job.attempts,
                id: job.id,
                batch_id: job.batch_id.or(job.callback_of),
            };
            let hook = (handler.failed)(plain, ctx, error);
            let hook = crate::context::scope_app(self.state.clone(), hook);
            if tokio::spawn(crate::clock::carry(hook)).await.is_err() {
                tracing::error!(job = %job.job, id = job.id, "the job's failed hook panicked");
            }
        }
    }

    /// One attempt, in its own task so a panic is a failed attempt, not a
    /// dead worker.
    async fn attempt(
        &self,
        handler: &super::JobHandler,
        job: &Reserved,
        payload: String,
    ) -> Outcome {
        let ctx = JobContext {
            state: self.state.clone(),
            attempt: job.attempts,
            id: job.id,
            batch_id: job.batch_id.or(job.callback_of),
        };
        let run = (handler.run)(payload, ctx);
        let run = crate::context::scope_app(self.state.clone(), run);
        let mut task = tokio::spawn(crate::clock::carry(run));
        match tokio::time::timeout(handler.timeout, &mut task).await {
            Ok(Ok(Ok(()))) => Outcome::Done,
            Ok(Ok(Err(err))) => Outcome::Failed(Failure {
                permanent: err.is_permanent(),
                error: format!("{err:?}"),
            }),
            Ok(Err(join)) => Outcome::Failed(Failure::retry(panic_message(join))),
            Err(_) => {
                task.abort();
                Outcome::Failed(Failure::retry(format!(
                    "timed out after {:?}",
                    handler.timeout
                )))
            }
        }
    }

    /// Writes a job's outcome: deleted (with its chain, batch and unique
    /// claim moved on), back on the queue, or failed.
    async fn record(
        &self,
        job: &Reserved,
        outcome: &Outcome,
        backoff: Duration,
        unique: Option<&str>,
    ) -> crate::Result {
        let db = &self.state.db;
        let failure = match outcome {
            Outcome::Released(wait) => {
                crate::db::sql(
                    "UPDATE jobs SET reserved_at = NULL, available_at = ?, \
                     attempts = attempts - 1 WHERE id = ?",
                )
                .bind(unix_now() + wait.as_secs() as i64)
                .bind(job.id)
                .execute(db)
                .await?;
                return Ok(());
            }
            Outcome::Failed(failure) if !failure.permanent && job.attempts < job.max_attempts => {
                crate::db::sql("UPDATE jobs SET reserved_at = NULL, available_at = ? WHERE id = ?")
                    .bind(unix_now() + backoff.as_secs() as i64)
                    .bind(job.id)
                    .execute(db)
                    .await?;
                return Ok(());
            }
            Outcome::Failed(failure) => Some(failure),
            Outcome::Done | Outcome::Skipped => None,
        };
        let mut tx = db.begin().await?;
        let deleted = crate::db::sql("DELETE FROM jobs WHERE id = ?")
            .bind(job.id)
            .execute(&mut tx)
            .await?;
        if deleted > 0 {
            match failure {
                Some(failure) => fail(&mut tx, job, &failure.error).await?,
                None => {
                    if let (Outcome::Done, Some(chain)) = (outcome, &job.chain) {
                        super::continue_chain(&mut tx, chain).await?;
                    }
                }
            }
            if let Some(batch) = job.batch_id {
                super::batch_job_done(&mut tx, batch, failure.is_some()).await?;
            }
            if let Some(key) = unique {
                super::release_unique(&mut tx, key).await?;
            }
            if !matches!(outcome, Outcome::Skipped) {
                super::dashboard::count_finished(&mut tx, failure.is_some()).await?;
            }
        }
        tx.commit().await?;
        if deleted > 0 {
            self.state.queue.wake_workers();
        }
        Ok(())
    }

    /// A job whose attempts may outlast the reservation keeps its row for
    /// the attempt's timeout plus a minute, so no other worker starts it again.
    async fn extend_reservation(&self, job: &Reserved, timeout: Duration) -> crate::Result {
        let needed = timeout.as_secs() as i64 + 60;
        if needed > RESERVATION {
            crate::db::sql("UPDATE jobs SET reserved_at = ? WHERE id = ?")
                .bind(unix_now() + (needed - RESERVATION))
                .bind(job.id)
                .execute(&self.state.db)
                .await?;
        }
        Ok(())
    }

    /// Moves jobs whose last attempt never finished (the process crashed,
    /// was killed or ran out of memory) to `failed_jobs`, at most once a minute.
    async fn sweep_exhausted(&self) -> crate::Result {
        let now = unix_now();
        let last = self.last_sweep.load(Ordering::Relaxed);
        if now - last < 60
            || self
                .last_sweep
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            return Ok(());
        }
        let mut tx = self.state.db.begin().await?;
        let rows = crate::db::sql(
            "DELETE FROM jobs WHERE attempts >= max_attempts AND reserved_at IS NOT NULL \
             AND reserved_at <= ? \
             RETURNING id, queue, job, payload, attempts, max_attempts, chain, batch_id, callback_of",
        )
        .bind(now - RESERVATION)
        .fetch_all(&mut tx)
        .await?;
        let mut swept = Vec::new();
        for row in &rows {
            let job = Reserved {
                id: row.try_get("id")?,
                queue: row.try_get("queue")?,
                job: row.try_get("job")?,
                payload: row.try_get("payload")?,
                attempts: row.try_get::<i64>("attempts")? as u32,
                max_attempts: row.try_get::<i64>("max_attempts")? as u32,
                chain: row.try_get("chain")?,
                batch_id: row.try_get("batch_id")?,
                callback_of: row.try_get("callback_of")?,
            };
            fail(&mut tx, &job, CRASHED).await?;
            if let Some(batch) = job.batch_id {
                super::batch_job_done(&mut tx, batch, true).await?;
            }
            let unique = self.handlers.get(job.job.as_str()).and_then(|handler| {
                let plain = self.state.queue.open(&job.payload).ok()?;
                (handler.unique_key)(&plain)
            });
            if let Some(key) = unique {
                super::release_unique(&mut tx, &key).await?;
            }
            tracing::error!(job = %job.job, "job's last attempt never finished; moved to failed_jobs");
            swept.push(job);
        }
        tx.commit().await?;
        // Like a last attempt that returned an error: reported, and the
        // job's `failed` hook runs.
        for job in swept {
            let plain = self.state.queue.open(&job.payload).ok();
            self.failed_for_good(&job, plain, CRASHED.to_owned()).await;
        }
        Ok(())
    }

    /// Runs every job that is available now; returns how many ran.
    pub async fn drain(&self) -> crate::Result<usize> {
        let mut ran = 0;
        while self.run_next().await? {
            ran += 1;
        }
        Ok(ran)
    }

    /// Runs `concurrency` loops until `shutdown` flips to true, then lets
    /// running jobs finish.
    pub async fn run(self, concurrency: usize, mut shutdown: watch::Receiver<bool>) {
        let mut loops = JoinSet::new();
        for _ in 0..concurrency.max(1) {
            let worker = self.clone();
            let mut stop = shutdown.clone();
            loops.spawn(async move {
                let wake = worker.state.queue.wake();
                loop {
                    if *stop.borrow() {
                        break;
                    }
                    // Listen before looking for work: a job dispatched while
                    // the query runs still wakes this loop instead of waiting
                    // for the next poll.
                    let notified = wake.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    match worker.run_next().await {
                        Ok(true) => continue,
                        Ok(false) => {}
                        Err(err) => tracing::error!(error = ?err, "queue worker error"),
                    }
                    tokio::select! {
                        _ = notified => {}
                        _ = tokio::time::sleep(POLL) => {}
                        _ = stop.changed() => {}
                    }
                }
            });
        }
        let _ = shutdown.changed().await;
        while loops.join_next().await.is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn an_aborted_attempt_says_it_was_cancelled() {
        let task = tokio::spawn(std::future::pending::<()>());
        task.abort();
        let err = task.await.unwrap_err();
        assert!(panic_message(err).starts_with("the job was cancelled"));
    }

    /// Bookkeeping retries while the database is briefly away: a write
    /// that works on its second try succeeds; one that never works gives
    /// up after the last delay with its error.
    #[tokio::test(start_paused = true)]
    async fn bookkeeping_writes_are_retried_then_given_up() {
        let tries = Arc::new(AtomicUsize::new(0));
        let counted = tries.clone();
        retry_write(|| {
            let n = counted.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Err(crate::Error::Internal(anyhow::anyhow!(
                        "database is locked"
                    )))
                } else {
                    Ok(())
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(tries.load(Ordering::SeqCst), 2);

        let tries = Arc::new(AtomicUsize::new(0));
        let counted = tries.clone();
        let started = tokio::time::Instant::now();
        let err = retry_write(|| {
            counted.fetch_add(1, Ordering::SeqCst);
            async { Err(crate::Error::Internal(anyhow::anyhow!("database is gone"))) }
        })
        .await
        .unwrap_err();
        assert!(format!("{err:?}").contains("database is gone"));
        assert_eq!(
            tries.load(Ordering::SeqCst),
            6,
            "the first try and five retries"
        );
        assert_eq!(started.elapsed(), Duration::from_millis(10_000));
    }
}
