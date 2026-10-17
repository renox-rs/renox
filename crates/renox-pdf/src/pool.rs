//! The render pool: PDF work runs on blocking threads, a few at a time.
//!
//! Compiling a template takes CPU for tens or hundreds of milliseconds, so it
//! never runs on the async runtime: [`Pool::run`] hands it to
//! `tokio::task::spawn_blocking` after taking a permit from a semaphore that
//! caps how many renders run at once (`PDF_MAX_CONCURRENT`). The wait for a
//! permit and the render together must finish within `PDF_TIMEOUT`.
//!
//! A blocking thread cannot be interrupted, so a render that timed out keeps
//! running until it ends. It keeps its permit too, which makes the cap honest:
//! a template that never finishes cannot pile up more threads than the cap.

use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use renox::Config;
use renox::tokio::sync::Semaphore;

use crate::error::PdfError;

/// Seconds a render may take when `PDF_TIMEOUT` is not set.
const DEFAULT_TIMEOUT_SECS: u64 = 30;
/// Renders that may run at once when `PDF_MAX_CONCURRENT` is not set.
const DEFAULT_MAX_CONCURRENT: u64 = 2;

/// The limits of a render, read from the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    /// `PDF_TIMEOUT`; `None` when it is 0 (no limit).
    pub(crate) timeout: Option<Duration>,
    /// `PDF_MAX_CONCURRENT`, at least 1.
    pub(crate) max_concurrent: usize,
}

impl Limits {
    /// Reads `PDF_TIMEOUT` (seconds, default 30, 0 = none) and
    /// `PDF_MAX_CONCURRENT` (default 2, at least 1) the way `Config` reads
    /// `var`s: `config.vars` first, then the environment.
    pub(crate) fn from_config(config: &Config) -> Result<Self, PdfError> {
        let number = |name: &str, default: u64| -> Result<u64, PdfError> {
            match config.var(name) {
                None => Ok(default),
                Some(value) => value.trim().parse().map_err(|_| {
                    PdfError::Setup(format!("{name} must be a whole number, got `{value}`"))
                }),
            }
        };
        let timeout = number("PDF_TIMEOUT", DEFAULT_TIMEOUT_SECS)?;
        let max = number("PDF_MAX_CONCURRENT", DEFAULT_MAX_CONCURRENT)?;
        if max == 0 {
            return Err(PdfError::Setup(
                "PDF_MAX_CONCURRENT must be at least 1".to_owned(),
            ));
        }
        Ok(Limits {
            timeout: (timeout > 0).then(|| Duration::from_secs(timeout)),
            max_concurrent: usize::try_from(max)
                .unwrap_or(Semaphore::MAX_PERMITS)
                .min(Semaphore::MAX_PERMITS),
        })
    }
}

/// The semaphore that caps concurrent renders. It is made by the first render
/// (the configuration is not known when the module registers) and keeps the
/// size it was made with.
#[derive(Default)]
pub(crate) struct Pool {
    semaphore: OnceLock<Arc<Semaphore>>,
}

impl Pool {
    /// Runs `job` on a blocking thread once a permit is free, failing with
    /// [`PdfError::Timeout`] when waiting and running together outlast
    /// `limits.timeout`. A plain `fn` returning `impl Future + Send`, so a
    /// handler that awaits it stays `Send`.
    pub(crate) fn run<T, F>(
        &self,
        limits: Limits,
        job: F,
    ) -> impl Future<Output = Result<T, PdfError>> + Send + use<T, F>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, PdfError> + Send + 'static,
    {
        let semaphore = self
            .semaphore
            .get_or_init(|| Arc::new(Semaphore::new(limits.max_concurrent)))
            .clone();
        async move {
            let work = async move {
                let permit = semaphore
                    .acquire_owned()
                    .await
                    .map_err(|_| PdfError::Failed("the render pool is closed".to_owned()))?;
                // The permit moves into the thread: it is released when the
                // render really ends, even if nobody waits for it any more.
                let handle = renox::tokio::task::spawn_blocking(move || {
                    let result = job();
                    drop(permit);
                    result
                });
                handle
                    .await
                    .map_err(|e| PdfError::Failed(format!("the render thread failed: {e}")))?
            };
            match limits.timeout {
                None => work.await,
                Some(limit) => renox::tokio::time::timeout(limit, work)
                    .await
                    .map_err(|_| PdfError::Timeout(limit))?,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Instant;

    use super::*;

    fn limits(timeout_ms: Option<u64>, max_concurrent: usize) -> Limits {
        Limits {
            timeout: timeout_ms.map(Duration::from_millis),
            max_concurrent,
        }
    }

    #[test]
    fn limits_are_read_from_the_config() {
        let config = |pairs: &[(&str, &str)]| {
            let mut config = Config::default();
            for (k, v) in pairs {
                config.vars.insert((*k).to_owned(), (*v).to_owned());
            }
            config
        };
        // Variables of the real environment would change what this reads.
        if std::env::var("PDF_TIMEOUT").is_err() && std::env::var("PDF_MAX_CONCURRENT").is_err() {
            let defaults = Limits::from_config(&config(&[])).unwrap();
            assert_eq!(defaults, limits(Some(30_000), 2));
        }
        let set = Limits::from_config(&config(&[
            ("PDF_TIMEOUT", "5"),
            ("PDF_MAX_CONCURRENT", "4"),
        ]))
        .unwrap();
        assert_eq!(set, limits(Some(5_000), 4));
        let none = Limits::from_config(&config(&[("PDF_TIMEOUT", "0")])).unwrap();
        assert_eq!(none.timeout, None);
        for bad in [("PDF_TIMEOUT", "soon"), ("PDF_MAX_CONCURRENT", "0")] {
            let error = Limits::from_config(&config(&[bad])).unwrap_err();
            assert!(matches!(error, PdfError::Setup(m) if m.contains(bad.0)));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_runtime_stays_responsive_during_a_render() {
        let pool = Pool::default();
        // On this one-thread runtime a render that ran on it would stall the
        // sleep below for the whole 300 ms.
        let render = renox::tokio::spawn(pool.run(limits(None, 1), || {
            std::thread::sleep(Duration::from_millis(300));
            Ok(())
        }));
        let started = Instant::now();
        for _ in 0..5 {
            renox::tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(started.elapsed() < Duration::from_millis(250));
        render.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn no_more_than_the_cap_run_at_once() {
        let pool = Pool::default();
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..6 {
            let (running, peak) = (running.clone(), peak.clone());
            let job = pool.run(limits(None, 2), move || {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(50));
                running.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            });
            tasks.push(renox::tokio::spawn(job));
        }
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_render_that_never_ends_times_out_and_keeps_its_permit() {
        let pool = Pool::default();
        let release = Arc::new(AtomicBool::new(false));
        let stuck = release.clone();
        let started = Instant::now();
        let out: Result<(), _> = pool
            .run(limits(Some(100), 1), move || {
                while !stuck.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(())
            })
            .await;
        assert_eq!(out, Err(PdfError::Timeout(Duration::from_millis(100))));
        assert!(started.elapsed() < Duration::from_secs(2));
        // The thread still runs, so the one permit is still taken: the next
        // render waits for it and times out too.
        let next: Result<(), _> = pool.run(limits(Some(100), 1), || Ok(())).await;
        assert!(matches!(next, Err(PdfError::Timeout(_))));
        release.store(true, Ordering::SeqCst);
        renox::tokio::time::sleep(Duration::from_millis(100)).await;
        let after: Result<u8, _> = pool.run(limits(Some(1000), 1), || Ok(1)).await;
        assert_eq!(after, Ok(1));
    }

    #[tokio::test]
    async fn a_panicking_render_is_an_error_and_frees_its_permit() {
        let pool = Pool::default();
        let out: Result<(), _> = pool.run(limits(None, 1), || panic!("boom")).await;
        assert!(matches!(out, Err(PdfError::Failed(_))));
        let again: Result<u8, _> = pool.run(limits(None, 1), || Ok(1)).await;
        assert_eq!(again, Ok(1));
    }
}
