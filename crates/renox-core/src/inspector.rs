//! `/_renox/debug`: the last requests with their status, time, view and
//! SQL, while developing locally (`APP_DEBUG` on and `APP_ENV=local`).

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::Router;
use axum::extract::{Path, Request, State};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;

use crate::{AppState, Error};

/// Requests kept.
const KEEP: usize = 50;
/// SQL kept per request.
const KEEP_QUERIES: usize = 200;

#[derive(Default)]
pub(crate) struct Inspector {
    requests: Mutex<VecDeque<Record>>,
    next: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
struct Record {
    n: u64,
    at: String,
    method: String,
    path: String,
    status: u16,
    millis: f64,
    id: String,
    view: Option<String>,
    queries: Vec<String>,
    /// Statements run three or more times: a likely N+1.
    repeated: Vec<(String, usize)>,
}

/// Records each request (not Renox's own `/_renox/*` pages).
pub(crate) async fn middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let Some(inspector) = state.inspector.clone() else {
        return next.run(req).await;
    };
    if req.uri().path().starts_with("/_renox/") {
        return next.run(req).await;
    }
    let method = req.method().to_string();
    let path = req
        .uri()
        .path_and_query()
        .map_or_else(|| req.uri().path().to_owned(), |p| p.as_str().to_owned());
    let id = req
        .extensions()
        .get::<crate::RequestId>()
        .map(|id| id.0.clone())
        .unwrap_or_default();
    let started = Instant::now();
    let (res, mut queries) = crate::db::capture_queries(next.run(req)).await;
    let millis = started.elapsed().as_secs_f64() * 1000.0;
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for sql in &queries {
        *counts.entry(sql.as_str()).or_default() += 1;
    }
    let mut repeated: Vec<(String, usize)> = counts
        .into_iter()
        .filter(|(_, n)| *n >= 3)
        .map(|(sql, n)| (sql.to_owned(), n))
        .collect();
    repeated.sort_by_key(|r| std::cmp::Reverse(r.1));
    queries.truncate(KEEP_QUERIES);
    let record = Record {
        n: inspector.next.fetch_add(1, Ordering::Relaxed) + 1,
        at: chrono::Local::now().format("%H:%M:%S").to_string(),
        method,
        path,
        status: res.status().as_u16(),
        millis: (millis * 10.0).round() / 10.0,
        id,
        view: res
            .extensions()
            .get::<crate::view::RenderedView>()
            .map(|v| v.0.clone()),
        queries,
        repeated,
    };
    let mut requests = inspector.requests.lock().unwrap_or_else(|e| e.into_inner());
    if requests.len() >= KEEP {
        requests.pop_back();
    }
    requests.push_front(record);
    res
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/_renox/debug", get(list))
        .route("/_renox/debug/{n}", get(show))
}

fn page(state: &AppState, ctx: minijinja::Value) -> Response {
    match state.views.render("renox/debug.html", ctx) {
        Ok(html) => Html(html).into_response(),
        Err(err) => Error::Internal(err).into_response(),
    }
}

async fn list(State(state): State<AppState>) -> Response {
    let Some(inspector) = state.inspector.clone() else {
        return Error::NotFound.into_response();
    };
    let requests: Vec<Record> = inspector
        .requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect();
    page(
        &state,
        minijinja::context! { requests, app_name => state.config.name.clone() },
    )
}

async fn show(State(state): State<AppState>, Path(n): Path<u64>) -> Response {
    let Some(inspector) = state.inspector.clone() else {
        return Error::NotFound.into_response();
    };
    let record = inspector
        .requests
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|r| r.n == n)
        .cloned();
    match record {
        Some(record) => page(
            &state,
            minijinja::context! { record, app_name => state.config.name.clone() },
        ),
        None => Error::NotFound.into_response(),
    }
}
