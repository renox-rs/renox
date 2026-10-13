//! Where the shop's errors go (`App::report` in `src/lib.rs`).
//!
//! Renox hands every error a person should look at to the app's reporters:
//! a request that answered 500, a job that failed for good, a scheduled
//! task that failed. Renox logs them anyway; this reporter also keeps each
//! one as a line of JSON in `storage/logs/errors.log` (the whole report:
//! where, the message, the error chain, the request's method, path, id and
//! user), so in development the errors of the last session are in one
//! file you can open, and the request id leads to the log lines and to
//! `/_renox/debug`.
//!
//! In production a shop would send them to an error tracker instead
//! (Sentry, a chat channel), for example by posting them to a webhook with
//! `state.http`.

use renox::prelude::*;
use renox::report::ErrorReport;
use std::io::Write;

/// The file the reports go to, under `STORAGE_PATH`.
pub fn log_path(state: &AppState) -> std::path::PathBuf {
    state.config.storage_path.join("logs").join("errors.log")
}

/// Logs `report` and appends it to [`log_path`].
pub async fn log(report: ErrorReport, state: AppState) {
    let request = report
        .request
        .as_ref()
        .map(|r| format!(" {} {} (request {})", r.method, r.path, r.id))
        .unwrap_or_default();
    let source = report
        .source
        .as_deref()
        .map(|s| format!(" {s}"))
        .unwrap_or_default();
    tracing::error!(
        "error report ({:?}{source}{request}): {}",
        report.kind,
        report.message
    );
    let path = log_path(&state);
    let line = match renox::serde_json::to_string(&report) {
        Ok(line) => line,
        Err(err) => return tracing::warn!("could not write the error report: {err}"),
    };
    let written = append_line(&path, &line);
    if let Err(err) = written {
        tracing::warn!("could not write {}: {err}", path.display());
    }
}

/// Appends one line to `path`, making its folder first. Small and rare
/// enough to write without a blocking task.
fn append_line(path: &std::path::Path, line: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")
}
