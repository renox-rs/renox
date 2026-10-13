//! Per-route rate limits: `Routes::throttle(60, Duration::from_secs(60))`.
//!
//! Requests are counted per logged-in user, or per IP address for guests
//! (see [`ClientIp`](crate::ClientIp) behind a proxy), in
//! fixed windows kept in memory. Over the limit, the response is 429 with
//! `Retry-After`; every allowed response carries `X-RateLimit-Limit` and
//! `X-RateLimit-Remaining`.

use crate::clock::Stamp;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::Error;
use crate::auth::CurrentUser;

/// Keys kept before stale windows are dropped on the next hit.
const SWEEP_AT: usize = 10_000;

pub(crate) struct Limiter {
    /// Tells this limit's counters apart in the shared table: made from the
    /// routes it covers and its numbers, so every server agrees on it.
    id: String,
    max: u32,
    window: Duration,
    hits: Mutex<HashMap<String, (u32, Stamp)>>,
}

pub(crate) enum Verdict {
    Allowed { remaining: u32 },
    Limited { retry_after: u64 },
}

impl Limiter {
    pub fn new(id: String, max: u32, window: Duration) -> Self {
        Self {
            id,
            max: max.max(1),
            window,
            hits: Mutex::new(HashMap::new()),
        }
    }

    pub fn hit(&self, key: &str) -> Verdict {
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        if hits.len() >= SWEEP_AT {
            hits.retain(|_, (_, start)| start.elapsed() < self.window);
        }
        let entry = hits.entry(key.to_owned()).or_insert((0, Stamp::now()));
        if entry.1.elapsed() >= self.window {
            *entry = (0, Stamp::now());
        }
        if entry.0 >= self.max {
            let left = self.window.saturating_sub(entry.1.elapsed());
            return Verdict::Limited {
                retry_after: left.as_secs().max(1),
            };
        }
        entry.0 += 1;
        Verdict::Allowed {
            remaining: self.max - entry.0,
        }
    }

    pub fn max(&self) -> u32 {
        self.max
    }
}

fn key(req: &Request) -> String {
    let user = req
        .extensions()
        .get::<CurrentUser>()
        .and_then(|c| c.user.as_ref().map(|u| u.id));
    let device = req
        .extensions()
        .get::<CurrentUser>()
        .and_then(|c| c.device.as_ref().map(|d| d.key().to_owned()));
    match (user, device) {
        (Some(id), _) => format!("user:{id}"),
        (None, Some(device)) => format!("device:{device}"),
        (None, None) => match crate::ClientIp::of(req) {
            Some(ip) => format!("ip:{ip}"),
            None => "ip:unknown".to_owned(),
        },
    }
}

/// Counts the request in the database (shared by every server), letting it
/// through if the database can't be reached.
async fn shared_hit(limiter: &Limiter, db: &crate::db::Db, key: &str) -> Verdict {
    let key = format!("throttle:{}:{key}", limiter.id);
    match crate::counters::increment(db, &key, limiter.window).await {
        Ok((count, ends)) if count > limiter.max => Verdict::Limited {
            retry_after: crate::counters::seconds_until(ends),
        },
        Ok((count, _)) => Verdict::Allowed {
            remaining: limiter.max - count,
        },
        Err(err) => {
            tracing::warn!(error = ?err, "could not count a rate-limited request");
            Verdict::Allowed {
                remaining: limiter.max,
            }
        }
    }
}

pub(crate) async fn check(limiter: &Limiter, req: Request, next: Next) -> Response {
    let shared = req
        .extensions()
        .get::<crate::AppState>()
        .filter(|state| state.config.cache_store == crate::CacheStore::Database)
        .map(|state| state.db.clone());
    let verdict = match &shared {
        Some(db) => shared_hit(limiter, db, &key(&req)).await,
        None => limiter.hit(&key(&req)),
    };
    respond(verdict, limiter.max(), req, next).await
}

/// What a named limiter's rule sees of a request (`App::rate_limiter`).
#[non_exhaustive]
pub struct LimitRequest<'a> {
    /// The logged-in user, if any.
    pub user: Option<&'a crate::auth::User>,
    /// The device a device token authenticated (`auth::DeviceToken`), if any.
    pub device: Option<&'a crate::auth::Device>,
    /// The client's IP (`ClientIp`), if known.
    pub ip: Option<std::net::IpAddr>,
    /// The request method.
    pub method: &'a axum::http::Method,
    /// The request path.
    pub path: &'a str,
    /// The request headers.
    pub headers: &'a axum::http::HeaderMap,
}

/// A limit a rule picks for one request: how many per how long, counted
/// under which key (the user, or the IP for guests, unless `by` says).
///
/// ```
/// # use renox::prelude::*;
/// use renox::rate_limit::Limit;
///
/// # let _ =
/// App::new().rate_limiter("api", |req| match req.user {
///     Some(user) if user.has_role("partner") => Limit::none(),
///     Some(_) => Limit::per_minute(600),
///     None => Limit::per_minute(60), // per IP
/// })
/// # ;
/// // then: Routes::new().get("/api/orders", orders).throttle_by("api")
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limit {
    max: u32,
    per: Duration,
    key: Option<String>,
    unlimited: bool,
}

impl Limit {
    /// At most `max` requests a minute.
    pub fn per_minute(max: u32) -> Self {
        Self::per(max, Duration::from_secs(60))
    }

    /// At most `max` requests an hour.
    pub fn per_hour(max: u32) -> Self {
        Self::per(max, Duration::from_secs(3600))
    }

    /// At most `max` requests (at least 1) per `per`.
    pub fn per(max: u32, per: Duration) -> Self {
        Self {
            max: max.max(1),
            per: per.max(Duration::from_secs(1)),
            key: None,
            unlimited: false,
        }
    }

    /// No limit for this request.
    pub fn none() -> Self {
        Self {
            max: u32::MAX,
            per: Duration::from_secs(1),
            key: None,
            unlimited: true,
        }
    }

    /// Counts under `key` instead of the user or IP, e.g. an API key or
    /// a team: `Limit::per_minute(100).by(format!("team:{team}"))`.
    pub fn by(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }
}

pub(crate) type LimitRule = std::sync::Arc<dyn Fn(&LimitRequest) -> Limit + Send + Sync>;

/// A named limiter: its rule and its in-memory counters.
pub(crate) struct NamedLimiter {
    pub rule: LimitRule,
    hits: Mutex<HashMap<String, (u32, Stamp)>>,
}

impl NamedLimiter {
    pub fn new(rule: LimitRule) -> Self {
        Self {
            rule,
            hits: Mutex::new(HashMap::new()),
        }
    }

    fn hit(&self, key: &str, limit: &Limit) -> Verdict {
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        if hits.len() >= SWEEP_AT {
            hits.retain(|_, (_, start)| start.elapsed() < Duration::from_secs(24 * 60 * 60));
        }
        let entry = hits.entry(key.to_owned()).or_insert((0, Stamp::now()));
        if entry.1.elapsed() >= limit.per {
            *entry = (0, Stamp::now());
        }
        if entry.0 >= limit.max {
            let left = limit.per.saturating_sub(entry.1.elapsed());
            return Verdict::Limited {
                retry_after: left.as_secs().max(1),
            };
        }
        entry.0 += 1;
        Verdict::Allowed {
            remaining: limit.max - entry.0,
        }
    }
}

/// `Routes::throttle_by(name)`: the named limiter's rule picks the limit.
pub(crate) async fn check_named(name: &str, req: Request, next: Next) -> Response {
    let Some(state) = req.extensions().get::<crate::AppState>().cloned() else {
        return next.run(req).await;
    };
    let Some(limiter) = state.limiters.get(name) else {
        return Error::Internal(anyhow::anyhow!(
            "no rate limiter `{name}`: define it with App::rate_limiter"
        ))
        .into_response();
    };
    let user = req
        .extensions()
        .get::<CurrentUser>()
        .and_then(|c| c.user.clone());
    let device = req
        .extensions()
        .get::<CurrentUser>()
        .and_then(|c| c.device.clone());
    let limit = (limiter.rule)(&LimitRequest {
        user: user.as_deref(),
        device: device.as_deref(),
        ip: crate::ClientIp::of(&req),
        method: req.method(),
        path: req.uri().path(),
        headers: req.headers(),
    });
    if limit.unlimited {
        return next.run(req).await;
    }
    let key = format!(
        "{name}:{}:{}",
        limit.key.clone().unwrap_or_else(|| key(&req)),
        limit.per.as_secs()
    );
    let verdict = if state.config.cache_store == crate::CacheStore::Database {
        let shared = Limiter::new(format!("named:{name}"), limit.max, limit.per);
        shared_hit(&shared, &state.db, &key).await
    } else {
        limiter.hit(&key, &limit)
    };
    respond(verdict, limit.max, req, next).await
}

async fn respond(verdict: Verdict, max: u32, req: Request, next: Next) -> Response {
    match verdict {
        Verdict::Limited { retry_after } => {
            let mut res = Error::TooManyRequests.into_response();
            if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                res.headers_mut().insert("retry-after", value);
            }
            res
        }
        Verdict::Allowed { remaining } => {
            let mut res = next.run(req).await;
            let headers = res.headers_mut();
            headers.insert("x-ratelimit-limit", max.into());
            headers.insert("x-ratelimit-remaining", remaining.into());
            res
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_per_key_and_window() {
        let limiter = Limiter::new("t".into(), 2, Duration::from_millis(50));
        assert!(matches!(
            limiter.hit("a"),
            Verdict::Allowed { remaining: 1 }
        ));
        assert!(matches!(
            limiter.hit("a"),
            Verdict::Allowed { remaining: 0 }
        ));
        assert!(matches!(
            limiter.hit("a"),
            Verdict::Limited { retry_after: 1 }
        ));
        assert!(matches!(limiter.hit("b"), Verdict::Allowed { .. }));
        std::thread::sleep(Duration::from_millis(60));
        assert!(matches!(
            limiter.hit("a"),
            Verdict::Allowed { remaining: 1 }
        ));
    }

    /// Past `SWEEP_AT` keys, counters whose window ended are dropped, so
    /// many one-off visitors don't grow the map for good.
    #[test]
    fn old_counters_are_swept_once_there_are_many() {
        let limiter = Limiter::new("t".into(), 5, Duration::from_secs(60));
        let named = NamedLimiter::new(std::sync::Arc::new(|_: &LimitRequest| Limit::per_hour(5)));
        let limit = Limit::per_hour(5);
        for i in 0..SWEEP_AT {
            limiter.hit(&format!("ip:{i}"));
            named.hit(&format!("ip:{i}"), &limit);
        }
        // A day later (and a minute for the plain limiter), the next hit
        // finds only itself.
        crate::clock::with_offset_sync(25 * 60 * 60, || {
            limiter.hit("late");
            named.hit("late", &limit);
        });
        assert_eq!(limiter.hits.lock().unwrap().len(), 1);
        assert_eq!(named.hits.lock().unwrap().len(), 1);
        // The clock can go back too: a counter is never older than now.
        crate::clock::with_offset_sync(-10, || {
            assert!(matches!(
                limiter.hit("late"),
                Verdict::Allowed { remaining: 3 }
            ));
        });
    }

    #[tokio::test]
    async fn a_named_limit_outside_the_app_lets_requests_through_and_an_unknown_one_is_a_500() {
        use tower::ServiceExt;
        let router = |name: &'static str| {
            axum::Router::new()
                .route("/", axum::routing::get(|| async { "ok" }))
                .layer(axum::middleware::from_fn(move |req, next| {
                    check_named(name, req, next)
                }))
        };
        let request = || {
            Request::builder()
                .uri("/")
                .body(axum::body::Body::empty())
                .unwrap()
        };
        // No app state (a router used on its own): not limited.
        let res = router("api").oneshot(request()).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        // `App::boot` refuses a `throttle_by` name with no limiter; a
        // router merged in another way still gets a clear 500.
        let app = crate::testing::TestApp::new(crate::App::new()).await;
        let mut req = request();
        req.extensions_mut().insert(app.state().clone());
        let res = router("nowhere").oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }
}
