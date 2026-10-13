//! Error reports: every error a person should look at (a 500, a job that
//! failed for good, a scheduled task that failed), handed to the app's
//! reporters, e.g. to send them to Sentry or a chat channel.
//!
//! ```
//! # use renox::prelude::*;
//! use renox::report::ErrorReport;
//!
//! # let _ =
//! App::new().report(|report: ErrorReport, state: AppState| async move {
//!     // e.g. POST it to your error tracker with state.http.
//!     let _ = state
//!         .http
//!         .post("https://errors.example.com/api/events")
//!         .json(&report)
//!         .send()
//!         .await;
//! })
//! # ;
//! ```
//!
//! Reporters run in the background, after the response is sent; one that
//! fails or panics doesn't affect the others. Errors are logged too, with or
//! without reporters.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;

use crate::AppState;

/// Where an error happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReportKind {
    /// A request answered 500.
    Request,
    /// A job failed for good (attempts used up, or a permanent error).
    Job,
    /// A scheduled task failed or panicked.
    ScheduledTask,
}

/// The request an error came from.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct RequestReport {
    /// The HTTP method, e.g. `POST`.
    pub method: String,
    /// The URL path, without the query string.
    pub path: String,
    /// The request id, also in the logs and the response's `X-Request-Id`.
    pub id: String,
    /// The client's IP address (`ClientIp`), if known.
    pub ip: Option<String>,
    /// The logged-in user, if any.
    pub user_id: Option<i64>,
}

/// One error, for [`App::report`](crate::App::report).
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct ErrorReport {
    /// Where it happened: a request, a job or a scheduled task.
    pub kind: ReportKind,
    /// The error's own message.
    pub message: String,
    /// The error with its causes (`{:?}`), for the developer.
    pub details: String,
    /// The scheduled task's name, or the job's name and id (`send-invoice #42`).
    pub source: Option<String>,
    /// The request, for [`ReportKind::Request`].
    pub request: Option<RequestReport>,
    /// `APP_ENV`: `local`, `testing` or `production`.
    pub environment: String,
    /// Unix seconds.
    pub at: i64,
}

pub(crate) type ReportFn =
    Arc<dyn Fn(ErrorReport, AppState) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

pub(crate) fn report_fn<F, Fut>(reporter: F) -> ReportFn
where
    F: Fn(ErrorReport, AppState) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |report, state| Box::pin(reporter(report, state)))
}

impl ErrorReport {
    pub(crate) fn new(
        state: &AppState,
        kind: ReportKind,
        message: String,
        details: String,
        source: Option<String>,
    ) -> Self {
        let request =
            crate::context::get::<crate::context::RequestInfo>().map(|info| RequestReport {
                method: info.method,
                path: info.path,
                id: info.id,
                ip: info.ip,
                user_id: crate::auth::current_user_id(),
            });
        Self {
            kind,
            message,
            details,
            source,
            request,
            environment: format!("{:?}", state.config.env).to_lowercase(),
            at: crate::clock::unix_secs(),
        }
    }
}

/// Hands `report` to every reporter, in the background.
pub(crate) fn send(state: &AppState, report: ErrorReport) {
    if state.reporters.is_empty() {
        return;
    }
    for reporter in state.reporters.iter() {
        let (reporter, report, state) = (reporter.clone(), report.clone(), state.clone());
        let run = async move {
            let task = tokio::spawn(crate::context::scope_app(
                state.clone(),
                reporter(report, state),
            ));
            if task.await.is_err() {
                tracing::error!("an error reporter panicked");
            }
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(run);
        }
    }
}

/// A report for a 500 in the current request, when there's an app to send it to.
pub(crate) fn request_error(err: &anyhow::Error) {
    if let Some(state) = crate::context::app() {
        let report = ErrorReport::new(
            &state,
            ReportKind::Request,
            err.to_string(),
            format!("{err:?}"),
            None,
        );
        send(&state, report);
    }
}
