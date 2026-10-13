//! M21d: request ids, error reports, the app's error pages, `route()` with
//! a query string, named rate limiters and the debug inspector.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use renox::prelude::*;
use renox::rate_limit::Limit;
use renox::report::{ErrorReport, ReportKind};
use renox::testing::TestApp;
use renox::{Environment, RequestId};

struct Ops;

impl Module for Ops {
    fn name(&self) -> &'static str {
        "ops"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/fine", |id: RequestId| async move { id.to_string() })
            .get("/boom", || async {
                Err::<&str, _>(Error::Internal(anyhow::anyhow!("the till is empty")))
            })
            .get("/secret", || async { Err::<&str, _>(Error::Forbidden) })
            .get("/links", || async { view("links.html", context! {}) })
            .get("/things/{id}", |Path(id): Path<i64>| async move {
                format!("thing {id}")
            })
            .name("things.show")
            .get("/posts", |State(state): State<AppState>| async move {
                for _ in 0..3 {
                    renox::db::sql("SELECT 1 AS one")
                        .fetch_all(&state.db)
                        .await?;
                }
                Ok::<_, Error>(view("links.html", context! {}))
            })
            .merge(
                Routes::new()
                    .get("/api/ping", || async { "pong" })
                    .throttle_by("api"),
            )
    }
}

fn views() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    std::fs::create_dir_all(path.join("errors")).unwrap();
    std::fs::write(
        path.join("links.html"),
        "<a href=\"{{ route('things.show', 3, tab='a&b', q=none) }}\">x</a>",
    )
    .unwrap();
    std::fs::write(
        path.join("errors/404.html"),
        "<h1>Lost at {{ request.path }}</h1>",
    )
    .unwrap();
    std::fs::write(
        path.join("errors/default.html"),
        "<h1>App error {{ status }}: {{ reason }}</h1>{{ csrf_field() }}",
    )
    .unwrap();
    dir
}

fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(Ops)
        .rate_limiter("api", |req| {
            match req
                .headers
                .get("x-api-key")
                .and_then(|key| key.to_str().ok())
            {
                Some("partner") => Limit::none(),
                Some(key) => Limit::per_minute(3).by(format!("key:{key}")),
                None => Limit::per_minute(1),
            }
        })
}

async fn test_app(views: &tempfile::TempDir) -> TestApp {
    let path = views.path().to_path_buf();
    TestApp::with_config(app(), move |c| c.views_path = path).await
}

#[renox::test]
async fn every_response_carries_a_request_id() {
    let views = views();
    let app = test_app(&views).await;
    let res = app.get("/fine").await;
    res.assert_ok();
    let id = res
        .headers
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(id.len() >= 8, "{id}");
    assert_eq!(res.text(), id, "handlers see the same id");

    // One from a proxy is kept when it looks like an id.
    let res = app
        .request()
        .header("x-request-id", "edge-4f2a9c1d")
        .get("/fine")
        .await;
    assert_eq!(res.text(), "edge-4f2a9c1d");
    // Anything else is replaced, so logs can't be forged.
    let res = app
        .request()
        .header("x-request-id", "bad id\" injected")
        .get("/fine")
        .await;
    assert_ne!(res.text(), "bad id\" injected");
    assert_eq!(
        res.headers.get("x-request-id").unwrap().to_str().unwrap(),
        res.text()
    );
}

#[renox::test]
async fn failures_reach_the_reporters() {
    let views = views();
    let seen: Arc<Mutex<Vec<ErrorReport>>> = Arc::default();
    let reports = seen.clone();
    let path = views.path().to_path_buf();
    let app = TestApp::with_config(
        app().report(move |report, _state| {
            let reports = reports.clone();
            async move { reports.lock().unwrap().push(report) }
        }),
        move |c| c.views_path = path,
    )
    .await;
    let res = app.get("/boom").await;
    res.assert_status(500);
    assert!(
        !res.text().contains("the till is empty"),
        "not shown outside debug"
    );
    let id = res
        .headers
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    // Only 500s are reported, not a 403 or a 404.
    app.get("/secret").await.assert_forbidden();
    app.get("/nowhere").await.assert_not_found();

    let report = wait_for(&seen, 1).await.remove(0);
    assert_eq!(report.kind, ReportKind::Request);
    assert_eq!(report.message, "the till is empty");
    let request = report.request.expect("the request");
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("GET", "/boom")
    );
    assert_eq!(request.id, id);
    assert_eq!(report.environment, "testing");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(seen.lock().unwrap().len(), 1);
}

async fn wait_for(seen: &Arc<Mutex<Vec<ErrorReport>>>, n: usize) -> Vec<ErrorReport> {
    for _ in 0..100 {
        if seen.lock().unwrap().len() >= n {
            return seen.lock().unwrap().clone();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no report arrived");
}

#[renox::test]
async fn error_pages_are_the_apps_with_its_globals() {
    let views = views();
    let app = test_app(&views).await;
    // errors/404.html, with the request globals a layout uses.
    app.get("/nowhere")
        .await
        .assert_not_found()
        .assert_see("Lost at /nowhere");
    // errors/default.html for the rest.
    app.get("/secret")
        .await
        .assert_forbidden()
        .assert_see("App error 403: Forbidden")
        .assert_see("name=\"_token\"");
    app.get("/boom")
        .await
        .assert_status(500)
        .assert_see("App error 500");
    // JSON clients still get JSON.
    let res = app.request().json().get("/secret").await;
    res.assert_forbidden();
    assert!(res.text().starts_with('{'), "{}", res.text());
}

#[renox::test]
async fn the_built_in_error_page_uses_the_ui_kit() {
    let app = TestApp::with_config(app(), |c| c.views_path = "/nonexistent/views".into()).await;
    app.get("/nowhere")
        .await
        .assert_not_found()
        .assert_see("404")
        .assert_see("rx-page")
        .assert_see("href=\"/\"");
}

#[renox::test]
async fn route_turns_named_arguments_into_a_query_string() {
    let views = views();
    let app = test_app(&views).await;
    app.get("/links")
        .await
        .assert_ok()
        .assert_see("href=\"/things/3?tab=a%26b\"");
}

#[renox::test]
async fn named_limiters_pick_a_limit_per_request() {
    let views = views();
    let app = test_app(&views).await;
    // Guests: one a minute.
    app.get("/api/ping").await.assert_ok();
    app.get("/api/ping").await.assert_status(429);
    // A key: three a minute, counted per key.
    let with_key = |key: &'static str| app.request().header("x-api-key", key);
    for _ in 0..3 {
        let res = with_key("k1").get("/api/ping").await;
        res.assert_ok();
        assert_eq!(res.headers.get("x-ratelimit-limit").unwrap(), "3");
    }
    let res = with_key("k1").get("/api/ping").await;
    res.assert_status(429);
    assert!(res.headers.get("retry-after").is_some());
    // JSON clients get the header too.
    let res = with_key("k1").json().get("/api/ping").await;
    res.assert_status(429);
    assert!(
        res.headers.get("retry-after").is_some(),
        "{:?}",
        res.headers
    );
    with_key("k2").get("/api/ping").await.assert_ok();
    // No limit at all.
    for _ in 0..10 {
        with_key("partner").get("/api/ping").await.assert_ok();
    }
}

#[renox::test]
async fn a_throttle_without_its_limiter_fails_at_boot() {
    let err = App::new()
        .module(Ops)
        .config(Config::default())
        .boot()
        .await
        .err()
        .expect("boot fails");
    assert!(format!("{err:?}").contains("api"), "{err:?}");
}

async fn local_app(views: &tempfile::TempDir) -> TestApp {
    let path = views.path().to_path_buf();
    TestApp::with_config(app(), move |c| {
        c.views_path = path;
        c.env = Environment::Local;
        c.debug = true;
    })
    .await
}

#[renox::test]
async fn the_inspector_lists_recent_requests_and_their_sql() {
    let views = views();
    let app = local_app(&views).await;
    app.get("/posts").await.assert_ok();
    app.get("/nowhere").await.assert_not_found();

    let list = app.get("/_renox/debug").await;
    list.assert_ok();
    list.assert_see("/posts")
        .assert_see("/nowhere")
        .assert_see("links.html");
    // Its own pages aren't listed.
    assert!(!list.text().contains("GET</strong> /_renox/debug"));
    // Newest first: /nowhere is #2, /posts #1.
    let detail = app.get("/_renox/debug/1").await;
    detail.assert_ok();
    detail
        .assert_see("SELECT 1 AS one")
        .assert_see("Possible N+1 query")
        .assert_see("Ran 3 times");
    app.get("/_renox/debug/99").await.assert_not_found();
}

#[renox::test]
async fn the_inspector_is_only_there_while_developing() {
    let views = views();
    let app = test_app(&views).await;
    app.get("/posts").await.assert_ok();
    app.get("/_renox/debug").await.assert_not_found();
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Doomed;

impl Job for Doomed {
    const NAME: &'static str = "doomed";
    const MAX_ATTEMPTS: u32 = 1;

    async fn handle(self, _: JobContext) -> Result {
        Err(abort(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the printer is on fire",
        ))
    }
}

#[renox::test]
async fn failed_jobs_and_scheduled_tasks_are_reported() {
    let seen: Arc<Mutex<Vec<ErrorReport>>> = Arc::default();
    let reports = seen.clone();
    let app = TestApp::new(
        App::new()
            .job::<Doomed>()
            .report(move |report, _state| {
                let reports = reports.clone();
                async move { reports.lock().unwrap().push(report) }
            })
            .schedule(|s| {
                s.hourly("nightly", |_| async {
                    Err(abort(StatusCode::INTERNAL_SERVER_ERROR, "disk full"))
                });
            }),
    )
    .await;
    app.state().dispatch(Doomed).await.unwrap();
    app.run_all_jobs().await;
    assert!(app.kernel().run_scheduled("nightly").await.is_err());

    let mut reports = wait_for(&seen, 2).await;
    reports.sort_by_key(|r| r.source.clone());
    let (job, task) = (&reports[0], &reports[1]);
    assert_eq!(job.kind, ReportKind::Job);
    assert_eq!(
        job.source.as_deref(),
        Some("doomed #1"),
        "the name and the job's id"
    );
    assert!(
        job.details.contains("the printer is on fire"),
        "{}",
        job.details
    );
    assert!(job.request.is_none());
    assert_eq!(
        (task.kind, task.source.as_deref()),
        (ReportKind::ScheduledTask, Some("nightly"))
    );
    assert!(task.details.contains("disk full"), "{}", task.details);
}

#[renox::test]
async fn error_pages_get_shared_values_too() {
    // A layout that shows a shared value (a cart count) must work on error
    // pages; with APP_DEBUG an undefined value is an error, so without the
    // shares the app's page would fail and Renox's would show instead.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("errors")).unwrap();
    std::fs::write(
        dir.path().join("errors/404.html"),
        "<p>Cart ({{ cart_count }})</p>Lost",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new().share("cart_count", |_| async { Ok(3) }),
        move |c| {
            c.views_path = path;
            c.debug = true;
        },
    )
    .await;
    let res = app.get("/nowhere").await;
    res.assert_not_found().assert_see("<p>Cart (3)</p>Lost");
}

/// #256: the inspector keeps the last 50 requests; its pages are 404s
/// where it doesn't run, and an app's copy of its page that fails is a 500.
#[renox::test]
async fn the_inspector_keeps_fifty_requests_and_answers_404_where_it_is_off() {
    let views = views();
    let app = local_app(&views).await;
    for _ in 0..51 {
        app.get("/fine").await.assert_ok();
    }
    app.get("/_renox/debug/1").await.assert_not_found();
    app.get("/_renox/debug/2").await.assert_ok();
    app.get("/_renox/debug/51").await.assert_ok();

    let views = self::views();
    let app = test_app(&views).await;
    app.get("/_renox/debug/1").await.assert_not_found();
}

#[renox::test]
async fn an_inspector_page_that_fails_is_a_500() {
    let views = views();
    std::fs::create_dir_all(views.path().join("renox")).unwrap();
    std::fs::write(
        views.path().join("renox/debug.html"),
        "{{ no_such_function() }}",
    )
    .unwrap();
    let app = local_app(&views).await;
    app.get("/fine").await.assert_ok();
    app.get("/_renox/debug").await.assert_status(500);
}

/// `/health` waits two seconds for the database, then says it timed out.
#[renox::test]
async fn health_reports_a_database_that_doesnt_answer_in_time() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", dir.path().join("app.db").display());
    let app = TestApp::with_config(App::new(), move |c| {
        c.database_url = url;
        c.database_pool_size = 1;
    })
    .await;
    // The only connection is busy.
    let tx = app.db().begin().await.unwrap();
    let res = app.get("/health").await;
    res.assert_status(503).assert_see("timed out");
    drop(tx);
    app.get("/health").await.assert_ok();
}

/// A reporter that panics is logged; the others still get the report.
#[renox::test]
async fn a_reporter_that_panics_doesnt_stop_the_others() {
    let (logs, _logged) = crate::logs::capture();
    let views = views();
    let seen: Arc<Mutex<Vec<ErrorReport>>> = Arc::default();
    let reports = seen.clone();
    let path = views.path().to_path_buf();
    let app = TestApp::with_config(
        app()
            .report(|_report, _state| async { panic!("the reporter broke") })
            .report(move |report, _state| {
                let reports = reports.clone();
                async move { reports.lock().unwrap().push(report) }
            }),
        move |c| c.views_path = path,
    )
    .await;
    app.get("/boom").await.assert_status(500);
    assert_eq!(wait_for(&seen, 1).await.len(), 1);
    assert!(logs.has(&["an error reporter panicked"]), "{}", logs.text());
}
