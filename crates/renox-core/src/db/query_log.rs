//! Counting the SQL a piece of code runs, e.g. to catch an N+1 in a test.

use std::future::Future;
use std::sync::{Arc, Mutex};

tokio::task_local! {
    static LOG: Arc<Mutex<Vec<String>>>;
}

/// Notes a statement if someone is capturing.
pub(crate) fn record(sql: &str) {
    let _ = LOG.try_with(|log| {
        log.lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(sql.to_owned());
    });
}

/// Runs `fut` and returns its output with the SQL statements it ran, in
/// order: queries in this task, including requests sent through `TestApp`
/// (not jobs run by workers or anything `tokio::spawn`ed).
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(app: renox::testing::TestApp) {
/// let (page, queries) = renox::db::capture_queries(app.get("/posts")).await;
/// page.assert_ok();
/// assert!(queries.len() <= 3, "N+1? {queries:#?}");
/// # }
/// ```
pub async fn capture_queries<F: Future>(fut: F) -> (F::Output, Vec<String>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let output = LOG.scope(log.clone(), fut).await;
    let queries = std::mem::take(&mut *log.lock().unwrap_or_else(|e| e.into_inner()));
    (output, queries)
}
