//! M21c: `Routes::resource`, and the TestApp tools: time travel, event and
//! notification fakes, view/JSON/session/auth assertions, a real server.

use std::time::Duration;

use renox::auth::{Channel, Notification, Recipient};
use renox::mail::Mail;
use renox::prelude::*;
use renox::testing::TestApp;

#[derive(Clone, Debug)]
struct Ordered {
    id: i64,
}

impl Event for Ordered {}

struct Shipped;

impl Notification for Shipped {
    fn kind(&self) -> &'static str {
        "shipped"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Mail]
    }

    fn to_mail(&self, to: &Recipient, _: &AppState) -> Result<Mail> {
        Ok(Mail::new(
            to.email().unwrap_or_default(),
            "Shipped",
            "On its way.",
        ))
    }
}

struct Things;

impl Module for Things {
    fn name(&self) -> &'static str {
        "things"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .resource(
                "/things",
                "things",
                Resource::new()
                    .index(|| async {
                        renox::axum::Json(
                            json!({ "data": [{ "name": "a" }, { "name": "b" }], "total": 2 }),
                        )
                    })
                    .create(|| async { "create" })
                    .store(|State(state): State<AppState>, user: AuthUser| async move {
                        state.emit(Ordered { id: 7 }).await?;
                        state.notify(&user, &Shipped).await?;
                        Ok::<_, Error>(Redirect::to("/things"))
                    })
                    .show(|Path(id): Path<i64>| async move { format!("show {id}") })
                    .edit(|Path(id): Path<i64>| async move { format!("edit {id}") })
                    .update(|Path(id): Path<i64>| async move { format!("update {id}") })
                    .destroy(|Path(id): Path<i64>| async move { format!("destroy {id}") }),
            )
            .resource(
                "/notes",
                "notes",
                Resource::new().index(|| async { "notes" }),
            )
            .get("/page", || async { view("page.html", context! {}) })
            .get("/remember", |session: Session| async move {
                session.put("seen", true)?;
                Ok::<_, Error>("ok")
            })
            .get("/now", || async { renox::db::now().to_rfc3339() })
            .merge(
                Routes::new()
                    .get("/limited", || async { "ok" })
                    .throttle(2, Duration::from_secs(60)),
            )
    }

    fn register(&self, app: &mut Registry) {
        app.listen(|_: Ordered, _| async { panic!("listeners don't run while events are faked") });
        app.job::<NoteTheTime>();
    }
}

/// When a job last ran, by its own clock.
static JOB_RAN_AT: std::sync::Mutex<Option<DateTime>> = std::sync::Mutex::new(None);

#[derive(serde::Serialize, serde::Deserialize)]
struct NoteTheTime;

impl Job for NoteTheTime {
    const NAME: &'static str = "note-the-time";

    async fn handle(self, _ctx: JobContext) -> Result {
        *JOB_RAN_AT.lock().unwrap() = Some(renox::db::now());
        Ok(())
    }
}

async fn app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("page.html"), "<h1>Page</h1>").unwrap();
    let views = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Auth::new()).module(Things), move |c| {
        c.views_path = views
    })
    .await;
    (app, dir)
}

#[renox::test]
async fn resources_register_what_they_are_given() {
    let (app, _dir) = app().await;
    app.get("/things/new").await.assert_see("create");
    app.get("/things/3").await.assert_see("show 3");
    app.get("/things/3/edit").await.assert_see("edit 3");
    app.put("/things/3", &[]).await.assert_see("update 3");
    app.patch("/things/3", &[]).await.assert_see("update 3");
    app.delete("/things/3").await.assert_see("destroy 3");
    app.get("/things/x").await.assert_not_found();
    // Names follow Laravel's.
    let state = app.state();
    assert_eq!(state.url("things.edit", &[&4]).unwrap(), "/things/4/edit");
    assert_eq!(state.url("things.create", &[]).unwrap(), "/things/new");
    assert_eq!(state.url("notes.index", &[]).unwrap(), "/notes");
    // Only what was given.
    assert!(state.url("notes.show", &[&1]).is_err());
    app.get("/notes/1").await.assert_not_found();
    let routes: Vec<(String, String)> = app
        .kernel()
        .routes()
        .iter()
        .filter(|r| r.path.starts_with("/things"))
        .map(|r| (r.method.clone(), r.path.clone()))
        .collect();
    assert!(
        routes.contains(&("PUT|PATCH".into(), "/things/{id}".into())),
        "{routes:?}"
    );
}

#[renox::test]
async fn json_view_session_and_auth_assertions() {
    let (app, _dir) = app().await;
    let res = app.get("/things").await;
    res.assert_json_path("data.1.name", "b")
        .assert_json_path("total", 2)
        .assert_json(json!({ "total": 2, "data": [{ "name": "a" }, {}] }));
    assert_eq!(res.json_path("data.5.name"), renox::serde_json::Value::Null);
    assert!(res.view.is_none());
    app.get("/page").await.assert_view("page.html");

    app.assert_guest().assert_session_missing("seen");
    app.get("/remember").await.assert_ok();
    app.assert_session_has("seen");
    assert_eq!(app.session_get::<bool>("seen"), Some(true));
    let user = User::register(app.db(), "U", "u@t.id", "password123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.assert_authenticated(Some(&user))
        .assert_authenticated(None);
}

#[renox::test]
async fn events_and_notifications_can_be_faked() {
    let (app, _dir) = app().await;
    let user = User::register(app.db(), "U", "u@t.id", "password123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.fake_events().fake_notifications();
    app.post("/things", &[]).await.assert_redirect("/things");
    app.assert_emitted::<Ordered>(|e| e.id == 7);
    assert_eq!(app.emitted::<Ordered>().len(), 1);
    app.assert_notified(&user, "shipped")
        .assert_notified_to("u@t.id", "shipped");
    assert!(app.sent_mail().is_empty(), "faked: nothing sent");
    assert_eq!(app.notifications()[0].kind, "shipped");
}

#[renox::test]
async fn time_travel_moves_the_clock_for_requests_and_jobs() {
    let (app, _dir) = app().await;
    let now = |text: String| renox::chrono::DateTime::parse_from_rfc3339(&text).unwrap();
    let before = now(app.get("/now").await.text());
    app.travel(Duration::from_secs(3 * 24 * 60 * 60));
    let later = now(app.get("/now").await.text());
    let moved = (later - before).num_hours();
    assert!((71..=73).contains(&moved), "{moved}");
    let inside = app.at_travelled_time(async { renox::db::now() }).await;
    assert!((inside - before.to_utc()).num_hours() >= 71);
    // A job runs in a task of its own, and still at the travelled time: a
    // task-local clock offset isn't inherited by `tokio::spawn`.
    app.state().dispatch(NoteTheTime).await.unwrap();
    assert_eq!(app.run_jobs().await, 1);
    let ran_at = JOB_RAN_AT.lock().unwrap().expect("the job ran");
    assert!((ran_at - before.to_utc()).num_hours() >= 71, "{ran_at}");
    app.travel_back();
    let back = now(app.get("/now").await.text());
    assert!((back - before).num_hours() < 1);

    // A signed link expires when the clock passes it.
    let link = app
        .state()
        .sign_path("/things/1", Duration::from_secs(60))
        .unwrap();
    assert!(link.contains("expires="));
}

async fn login(app: &TestApp, password: &str) -> renox::testing::TestResponse {
    app.post(
        "/login",
        &[("email", "alex@example.com"), ("password", password)],
    )
    .await
}

#[renox::test]
async fn time_travel_reaches_rate_limits_and_the_login_lock() {
    let (app, _dir) = app().await;
    for _ in 0..2 {
        app.get("/limited").await.assert_ok();
    }
    app.get("/limited").await.assert_status(429);
    app.travel(Duration::from_secs(61));
    app.get("/limited").await.assert_ok();

    User::register(app.db(), "Alex", "alex@example.com", "password123")
        .await
        .unwrap();
    for _ in 0..5 {
        login(&app, "wrong").await;
    }
    login(&app, "password123").await;
    app.assert_guest();
    app.travel(Duration::from_secs(61));
    login(&app, "password123").await.assert_redirect("/");
    app.assert_authenticated(None);

    // Past the session's lifetime: the next request starts a new session,
    // and TestApp's CSRF token comes from that one (not a 419).
    app.travel(Duration::from_secs(3 * 24 * 60 * 60));
    app.assert_guest();
    login(&app, "password123").await.assert_redirect("/");
    app.assert_authenticated(None);
}

#[renox::test]
async fn the_app_can_be_served_for_a_browser() {
    let (app, _dir) = app().await;
    let url = app.serve().await;
    assert!(url.starts_with("http://127.0.0.1:"));
    let page = app
        .state()
        .http
        .get(format!("{url}/page"))
        .send()
        .await
        .unwrap();
    assert!(page.ok() && page.text().contains("<h1>Page</h1>"));
}

/// What the "stamp" task last saw as the time (unix seconds).
static STAMPED: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// #311: a scheduled task run from a test sees the travelled clock, as
/// requests and jobs do.
#[renox::test]
async fn scheduled_tasks_run_at_the_travelled_time() {
    let app = TestApp::new(App::new().schedule(|s| {
        s.daily_at("03:00", "stamp", |_| async {
            STAMPED.store(
                renox::db::now().timestamp(),
                std::sync::atomic::Ordering::SeqCst,
            );
            Ok(())
        });
    }))
    .await;
    let real = renox::db::now().timestamp();
    app.travel(std::time::Duration::from_secs(40 * 24 * 3600));
    app.run_scheduled("stamp").await.unwrap();
    let seen = STAMPED.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        seen >= real + 39 * 24 * 3600,
        "the task saw {seen}, the real time is {real}"
    );
    assert!(app.run_scheduled("nope").await.is_err(), "an unknown task");
}

#[renox::test]
async fn assert_views_compile_passes_and_names_broken_templates() {
    let dir = tempfile::tempdir().unwrap();
    let views = dir.path().join("views");
    std::fs::create_dir_all(&views).unwrap();
    std::fs::write(views.join("ok.html"), "<p>{{ 1 + 1 }}</p>").unwrap();
    let path = views.clone();
    let app = TestApp::with_config(App::new(), move |c| c.views_path = path).await;
    app.assert_views_compile();

    std::fs::write(views.join("broken.html"), "{% if %}").unwrap();
    let path = views.clone();
    let app = TestApp::with_config(App::new(), move |c| c.views_path = path).await;
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.assert_views_compile();
    }))
    .expect_err("a broken template fails the check");
    let message = failed.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(message.contains("broken.html"), "{message}");
}
