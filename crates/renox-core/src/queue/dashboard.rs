//! The queue dashboard.

use axum::extract::{Path, State};
use axum::response::Redirect;
use serde::Serialize;

use super::{BatchStatus, FailedJob, unix_now};
use crate::db::Transaction;
use crate::{AppState, Module, Result, Routes, View, context, view};

/// The gate that decides who sees the dashboard.
pub const GATE: &str = "view-queue-dashboard";
const DONE: &str = "renox:queue:done:";
const FAILED: &str = "renox:queue:failed:";

/// A page showing the queue (`/_renox/queue`): jobs waiting per queue,
/// how long the oldest has waited, throughput, failed jobs (retry or
/// forget them) and recent batches.
///
/// ```
/// # use renox::prelude::*;
/// # let _ =
/// App::new()
///     .module(renox::queue::Dashboard)
///     // Who may see it; nobody else, not even in development.
///     .gate("view-queue-dashboard", |user| user.email.ends_with("@example.com"))
/// # ;
/// ```
pub struct Dashboard;

impl Module for Dashboard {
    fn name(&self) -> &'static str {
        "queue-dashboard"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/_renox/queue", show)
            .name("queue.dashboard")
            .post("/_renox/queue/failed/{id}/retry", retry)
            .name("queue.retry")
            .post("/_renox/queue/failed/{id}/forget", forget)
            .name("queue.forget")
            .post("/_renox/queue/failed/retry-all", retry_all)
            .name("queue.retry_all")
            .require_gate(GATE)
    }
}

/// Jobs of one queue.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct QueueCounts {
    /// The queue's name.
    pub queue: String,
    /// Available now, waiting for a worker.
    pub ready: i64,
    /// Waiting for their delay or backoff.
    pub delayed: i64,
    /// Being run.
    pub running: i64,
}

/// The queue at a glance; from [`Queue::stats`](super::Queue::stats).
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct QueueStats {
    /// One entry per queue that has jobs, by name.
    pub queues: Vec<QueueCounts>,
    /// Seconds the oldest ready job has waited.
    pub oldest_wait: Option<i64>,
    /// Jobs finished in the last hour.
    pub done_last_hour: i64,
    /// Jobs that failed for good in the last hour.
    pub failed_last_hour: i64,
    /// Rows in `failed_jobs`, of any age.
    pub failed_total: i64,
}

impl super::Queue {
    /// Counts for monitoring (the dashboard shows them).
    pub async fn stats(&self) -> Result<QueueStats> {
        let now = unix_now();
        let rows = crate::db::sql(
            "SELECT queue, \
             SUM(CASE WHEN reserved_at IS NULL AND available_at <= ? THEN 1 ELSE 0 END) AS ready, \
             SUM(CASE WHEN reserved_at IS NULL AND available_at > ? THEN 1 ELSE 0 END) AS delayed, \
             SUM(CASE WHEN reserved_at IS NOT NULL THEN 1 ELSE 0 END) AS running, \
             MIN(CASE WHEN reserved_at IS NULL AND available_at <= ? THEN available_at END) AS oldest \
             FROM jobs GROUP BY queue ORDER BY queue",
        )
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_all(&self.db)
        .await?;
        let mut queues = Vec::new();
        let mut oldest: Option<i64> = None;
        for row in &rows {
            queues.push(QueueCounts {
                queue: row.try_get("queue")?,
                ready: row.try_get("ready")?,
                delayed: row.try_get("delayed")?,
                running: row.try_get("running")?,
            });
            if let Some(at) = row.try_get::<Option<i64>>("oldest")? {
                oldest = Some(oldest.map_or(at, |o| o.min(at)));
            }
        }
        let counters: Vec<(String, String)> =
            crate::db::sql("SELECT key, value FROM cache WHERE key LIKE ? OR key LIKE ?")
                .bind(format!("{DONE}%"))
                .bind(format!("{FAILED}%"))
                .fetch_as(&self.db)
                .await?;
        let since = now / 60 - 60;
        let (mut done, mut failed) = (0, 0);
        for (key, value) in counters {
            let (total, minute) = match key.strip_prefix(DONE) {
                Some(minute) => (&mut done, minute),
                None => (&mut failed, key.strip_prefix(FAILED).unwrap_or_default()),
            };
            if minute.parse::<i64>().is_ok_and(|m| m > since) {
                *total += value.parse::<i64>().unwrap_or(0);
            }
        }
        let failed_total: i64 = crate::db::sql("SELECT COUNT(*) FROM failed_jobs")
            .scalar(&self.db)
            .await?;
        Ok(QueueStats {
            queues,
            oldest_wait: oldest.map(|at| now - at),
            done_last_hour: done,
            failed_last_hour: failed,
            failed_total,
        })
    }

    /// The most recent batches, newest first.
    pub async fn recent_batches(&self, limit: u32) -> Result<Vec<BatchStatus>> {
        let ids: Vec<i64> = crate::db::sql("SELECT id FROM job_batches ORDER BY id DESC LIMIT ?")
            .bind(i64::from(limit))
            .scalars(&self.db)
            .await?;
        let mut batches = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(batch) = self.batch_status(id).await? {
                batches.push(batch);
            }
        }
        Ok(batches)
    }
}

/// Counts a job that finished (`failed`: for good) in this minute's
/// throughput counter, kept two hours.
pub(crate) async fn count_finished(tx: &mut Transaction, failed: bool) -> Result {
    let now = unix_now();
    let prefix = if failed { FAILED } else { DONE };
    crate::db::sql(
        "INSERT INTO cache (key, value, expires_at) VALUES (?, '1', ?) \
         ON CONFLICT (key) DO UPDATE SET value = CAST(CAST(cache.value AS BIGINT) + 1 AS TEXT)",
    )
    .bind(format!("{prefix}{}", now / 60))
    .bind(now + 2 * 60 * 60)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

#[derive(Serialize)]
struct FailedRow {
    id: i64,
    job: String,
    queue: String,
    /// The error's first line.
    error: String,
    ago: String,
}

fn ago(seconds: i64) -> String {
    match seconds.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

async fn show(State(state): State<AppState>) -> Result<View> {
    let queue = &state.queue;
    let stats = queue.stats().await?;
    let now = unix_now();
    let mut failed: Vec<FailedJob> = queue.failed().await?;
    failed.reverse();
    failed.truncate(25);
    let failed: Vec<FailedRow> = failed
        .into_iter()
        .map(|f| FailedRow {
            id: f.id,
            error: f
                .error
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect(),
            job: f.job,
            queue: f.queue,
            ago: ago(now - f.failed_at.timestamp()),
        })
        .collect();
    let batches = queue.recent_batches(10).await?;
    let batches: Vec<_> = batches
        .into_iter()
        .map(|b| {
            let progress = b.progress();
            context! { batch => b, progress => progress }
        })
        .collect();
    Ok(view(
        "renox/queue/dashboard.html",
        context! {
            stats => stats,
            oldest_wait => stats.oldest_wait.map(ago),
            failed => failed,
            batches => batches,
        },
    ))
}

async fn retry(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Redirect> {
    state.queue.retry(id).await?;
    Ok(Redirect::to("/_renox/queue"))
}

async fn forget(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Redirect> {
    state.queue.forget_failed(id).await?;
    Ok(Redirect::to("/_renox/queue"))
}

async fn retry_all(State(state): State<AppState>) -> Result<Redirect> {
    state.queue.retry_all().await?;
    Ok(Redirect::to("/_renox/queue"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn ages_read_in_the_largest_whole_unit() {
        let ago = super::ago;
        assert_eq!(
            [
                ago(-5),
                ago(59),
                ago(60),
                ago(3599),
                ago(3600),
                ago(86_399),
                ago(86_400 * 3)
            ],
            ["0s", "59s", "1m", "59m", "1h", "23h", "3d"]
        );
    }
}
