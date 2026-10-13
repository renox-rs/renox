use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use renox::Kernel;
use renox::prelude::*;
use tower::ServiceExt;

struct Api;

impl Module for Api {
    fn name(&self) -> &'static str {
        "api"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { "home" })
            .name("home")
            .merge(
                Routes::new()
                    .get("/search", || async { "results" })
                    .throttle(2, Duration::from_secs(60)),
            )
    }
}

fn config(dir: &std::path::Path) -> Config {
    {
        let mut c = Config::default();
        c.env = Environment::Testing;
        c.key = Some(renox::generate_key());
        c.views_path = dir.join("views");
        c.storage_path = dir.join("storage");
        c
    }
}

async fn kernel(config: Config) -> Kernel {
    let kernel = App::with_config(config).module(Api).boot().await.unwrap();
    kernel.migrate().await.unwrap();
    kernel
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

async fn send(kernel: &Kernel, req: Request<Body>) -> Reply {
    let res = kernel.router().oneshot(req).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        headers,
        body: String::from_utf8(body.to_vec()).unwrap(),
    }
}

fn from(ip: &str, uri: &str) -> Request<Body> {
    let mut req = Request::get(uri).body(Body::empty()).unwrap();
    let addr: SocketAddr = format!("{ip}:5000").parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

async fn cache_basics(kernel: &Kernel) {
    let cache = &kernel.state().cache;
    assert_eq!(cache.get::<String>("menu").await.unwrap(), None);
    cache
        .put("menu", &vec!["coffee", "tea"], None)
        .await
        .unwrap();
    assert_eq!(
        cache.get::<Vec<String>>("menu").await.unwrap().unwrap(),
        ["coffee", "tea"]
    );
    assert!(cache.has("menu").await.unwrap());
    assert!(
        cache.get::<i64>("menu").await.is_err(),
        "a wrong type is an error, not a silent miss"
    );

    let calls = Arc::new(AtomicUsize::new(0));
    for _ in 0..3 {
        let calls = calls.clone();
        let total: i64 = cache
            .remember("total", Duration::from_secs(60), || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(42)
            })
            .await
            .unwrap();
        assert_eq!(total, 42);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1, "computed once");

    let failed: Result<i64> = cache
        .remember("broken", Duration::from_secs(60), || async {
            Err(Error::NotFound)
        })
        .await;
    assert!(failed.is_err());
    assert!(!cache.has("broken").await.unwrap(), "errors are not cached");

    cache.forget("menu").await.unwrap();
    assert!(!cache.has("menu").await.unwrap());
    cache.flush().await.unwrap();
    assert!(!cache.has("total").await.unwrap());
}

#[tokio::test]
async fn memory_cache() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel(config(dir.path())).await;
    cache_basics(&kernel).await;

    let cache = &kernel.state().cache;
    cache
        .put("short", &1, Some(Duration::from_secs(1)))
        .await
        .unwrap();
    assert!(cache.has("short").await.unwrap());
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert!(!cache.has("short").await.unwrap(), "expired");
}

#[tokio::test]
async fn database_cache() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel({
        let mut c = config(dir.path());
        c.cache_store = renox::CacheStore::Database;
        c
    })
    .await;
    cache_basics(&kernel).await;

    let cache = &kernel.state().cache;
    cache
        .put("short", &1, Some(Duration::from_secs(60)))
        .await
        .unwrap();
    renox::db::sql("UPDATE cache SET expires_at = 1")
        .execute(kernel.db())
        .await
        .unwrap();
    assert!(
        !cache.has("short").await.unwrap(),
        "expired rows are ignored"
    );

    // An unknown store (`CACHE_STORE=redis`) is refused when the config is
    // read: see the config tests.
}

#[tokio::test]
async fn throttled_routes_answer_429() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel(config(dir.path())).await;

    let first = send(&kernel, from("10.0.0.1", "/search")).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.headers["x-ratelimit-limit"], "2");
    assert_eq!(first.headers["x-ratelimit-remaining"], "1");
    assert_eq!(
        send(&kernel, from("10.0.0.1", "/search")).await.status,
        StatusCode::OK
    );

    let limited = send(&kernel, from("10.0.0.1", "/search")).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        limited.headers["retry-after"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            <= 60
    );
    assert!(limited.body.contains("429"), "{}", limited.body);

    assert_eq!(
        send(&kernel, from("10.0.0.2", "/search")).await.status,
        StatusCode::OK,
        "per IP"
    );
    assert_eq!(
        send(&kernel, from("10.0.0.1", "/")).await.status,
        StatusCode::OK,
        "only throttled routes"
    );
}

#[tokio::test]
async fn maintenance_mode_with_a_bypass_secret() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("views/errors")).unwrap();
    std::fs::write(
        dir.path().join("views/errors/503.html"),
        "Down for maintenance, back soon",
    )
    .unwrap();
    let config = config(dir.path());
    let kernel = kernel(config.clone()).await;

    renox::maintenance::down(
        &config.storage_path,
        renox::maintenance::DownOptions::new()
            .secret("opensesame")
            .retry(120),
    )
    .unwrap();
    let down = send(&kernel, from("10.0.0.1", "/")).await;
    assert_eq!(down.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(down.headers["retry-after"], "120");
    assert_eq!(down.body, "Down for maintenance, back soon");

    let health = send(&kernel, from("10.0.0.1", "/health")).await;
    assert_eq!(health.status, StatusCode::OK, "health checks keep working");
    assert!(health.body.contains(r#""maintenance":true"#));

    let bypass = send(&kernel, from("10.0.0.1", "/opensesame")).await;
    assert_eq!(bypass.status, StatusCode::SEE_OTHER);
    let cookie = bypass.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut req = from("10.0.0.1", "/");
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    assert_eq!(
        send(&kernel, req).await.body,
        "home",
        "the cookie lets the owner in"
    );

    let mut wrong = from("10.0.0.1", "/");
    wrong
        .headers_mut()
        .insert("cookie", "renox_maintenance=guess".parse().unwrap());
    assert_eq!(
        send(&kernel, wrong).await.status,
        StatusCode::SERVICE_UNAVAILABLE
    );

    assert!(renox::maintenance::up(&config.storage_path).unwrap());
    assert!(!renox::maintenance::up(&config.storage_path).unwrap());
    assert_eq!(send(&kernel, from("10.0.0.1", "/")).await.body, "home");
}

#[tokio::test]
async fn health_reports_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel(config(dir.path())).await;
    let ok = send(&kernel, from("10.0.0.1", "/health")).await;
    assert_eq!(ok.status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&ok.body).unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "ok");
    assert_eq!(body["queue"]["pending"], 0);
    assert_eq!(body["queue"]["failed"], 0);

    kernel.db().close().await;
    let down = send(&kernel, from("10.0.0.1", "/health")).await;
    assert_eq!(down.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(down.body.contains(r#""status":"error""#), "{}", down.body);
}

struct Limited;

impl Module for Limited {
    fn name(&self) -> &'static str {
        "limited"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/search", || async { "results" })
            .throttle(2, std::time::Duration::from_secs(60))
    }
}

/// Two servers sharing one database: with `CACHE_STORE=database` they share
/// rate limits and the login lock; with `memory` each counts its own.
#[renox::test(flavor = "multi_thread", worker_threads = 4)]
async fn several_servers_share_limits_with_the_database_store() {
    use renox::testing::TestApp;
    for (store, shared) in [("database", true), ("memory", false)] {
        let dir = tempfile::tempdir().unwrap();
        let (url, schema) = shared_database(dir.path()).await;
        let server = |url: String| {
            let store = match store {
                "database" => renox::CacheStore::Database,
                _ => renox::CacheStore::Memory,
            };
            TestApp::with_config(App::new().module(Auth::new()).module(Limited), move |c| {
                c.database_url = url;
                c.cache_store = store;
            })
        };
        let a = server(url.clone()).await;
        let b = server(url).await;
        a.get("/search").await.assert_ok();
        a.get("/search").await.assert_ok();
        a.get("/search").await.assert_status(429);
        let other = b.get("/search").await;
        assert_eq!(
            other.status.as_u16() == 429,
            shared,
            "{store}: {}",
            other.status
        );

        User::register(a.db(), "Alex", "alex@example.com", "letmein123")
            .await
            .unwrap();
        for _ in 0..5 {
            a.htmx()
                .post(
                    "/login",
                    &[("email", "alex@example.com"), ("password", "wrong")],
                )
                .await
                .assert_invalid("email");
        }
        let res = b
            .htmx()
            .post(
                "/login",
                &[("email", "alex@example.com"), ("password", "letmein123")],
            )
            .await;
        if shared {
            res.assert_invalid("email");
            assert!(
                res.text().contains("Too many login attempts"),
                "{}",
                res.text()
            );
        } else {
            assert_ne!(res.status.as_u16(), 422, "memory: b has its own counts");
        }
        if let Some(drop) = schema {
            renox::db::sql(drop).execute(a.db()).await.unwrap();
        }
    }
}

/// A database two servers share: a file on SQLite; on PostgreSQL (with
/// `TEST_DATABASE_URL`) a schema of its own, named in the URL so the test
/// swap that gives every boot a fresh schema doesn't apply. Also the
/// statement that drops that schema afterwards.
async fn shared_database(dir: &std::path::Path) -> (String, Option<String>) {
    let file = format!("sqlite://{}/app.db", dir.display());
    let Some(base) = std::env::var("TEST_DATABASE_URL")
        .ok()
        .filter(|url| url.starts_with("postgres"))
    else {
        return (file, None);
    };
    let schema = format!(
        "renox_shared_{}",
        dir.file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase()
            .replace(|c: char| !c.is_ascii_alphanumeric(), "_")
    );
    let mut config = Config::default();
    config.database_url = base.clone();
    let kernel = App::with_config(config).boot().await.unwrap();
    renox::db::sql(format!("CREATE SCHEMA {schema}"))
        .execute(kernel.db())
        .await
        .unwrap();
    kernel.db().close().await;
    let glue = if base.contains('?') { '&' } else { '?' };
    (
        format!("{base}{glue}options=-c%20search_path%3D{schema}"),
        Some(format!("DROP SCHEMA {schema} CASCADE")),
    )
}
