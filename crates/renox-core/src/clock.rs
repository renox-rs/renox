//! The current time, with an offset tests can move (`TestApp::travel`).

use std::future::Future;
use std::time::{Duration, SystemTime};

tokio::task_local! {
    /// Seconds added to the real clock in this task (tests only).
    static OFFSET: i64;
}

/// Now, plus the task's offset.
pub(crate) fn system_now() -> SystemTime {
    let offset = OFFSET.try_with(|o| *o).unwrap_or(0);
    let now = SystemTime::now();
    if offset >= 0 {
        now + Duration::from_secs(offset as u64)
    } else {
        now - Duration::from_secs(offset.unsigned_abs())
    }
}

/// Seconds since the Unix epoch.
pub(crate) fn unix_secs() -> i64 {
    system_now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Milliseconds since the Unix epoch.
pub(crate) fn unix_millis() -> i64 {
    system_now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// A moment on this clock, for in-memory windows (rate limits, the login
/// lock) that `TestApp::travel` must reach: `Instant` ignores it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stamp(i64);

impl Stamp {
    pub(crate) fn now() -> Self {
        Self(unix_millis())
    }

    /// Time since this stamp; zero if the clock went back.
    pub(crate) fn elapsed(&self) -> Duration {
        Duration::from_millis(u64::try_from(unix_millis() - self.0).unwrap_or(0))
    }
}

/// Runs `f` with the clock `seconds` ahead, for synchronous code.
pub(crate) fn with_offset_sync<T>(seconds: i64, f: impl FnOnce() -> T) -> T {
    OFFSET.sync_scope(seconds, f)
}

/// Runs `fut` with the clock `seconds` ahead (behind when negative).
pub(crate) async fn with_offset<F: Future>(seconds: i64, fut: F) -> F::Output {
    OFFSET.scope(seconds, fut).await
}

/// `fut` on this task's clock, for code moved to a task of its own:
/// `tokio::spawn` starts without task-locals, so a job run by
/// `TestApp::run_jobs` after `travel` ran on the real clock. Wrap what is
/// spawned: `tokio::spawn(clock::carry(run))`.
pub(crate) fn carry<F: Future>(fut: F) -> impl Future<Output = F::Output> {
    let offset = OFFSET.try_with(|o| *o).unwrap_or(0);
    OFFSET.scope(offset, fut)
}
