mod support;

use axum::extract::State;
use axum::http::StatusCode;
use renox_core::{
    AppState, Back, Htmx, HxTrigger, Module, Result, Routes, Session, View, context, view,
};
use serde::{Deserialize, Serialize};
use support::TestApp;

struct Demo;

#[derive(Serialize, Deserialize)]
struct Note {
    title: String,
}

impl Module for Demo {
    fn name(&self) -> &'static str {
        "demo"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", |State(s): State<AppState>| async move {
                s.config.name.clone()
            })
            .name("home")
            .get("/notes/{id}", |State(s): State<AppState>| async move {
                s.url("notes.show", &[&7]).unwrap()
            })
            .name("notes.show")
            .get("/fail", fail)
            .get("/token", |s: Session| async move { s.token() })
            .get("/counter", counter)
            .post("/flash", flash)
            .get("/flashed", |s: Session| async move {
                s.get::<String>("status").unwrap_or_default()
            })
            .get("/page", page)
            .post("/notes", store)
            .get("/notes/create", create)
            .get("/hx", |htmx: Htmx| async move {
                format!("{} {}", htmx.request, htmx.wants_fragment())
            })
            .post("/hx", || async { (HxTrigger("saved".into()), "ok") })
    }
}

async fn fail() -> Result<String> {
    let n: u8 = "not a number".parse()?;
    Ok(n.to_string())
}

async fn counter(session: Session) -> Result<String> {
    let n = session.get::<u32>("n").unwrap_or(0) + 1;
    session.put("n", n)?;
    Ok(n.to_string())
}

async fn flash(session: Session, back: Back) -> Result<Back> {
    session.flash("status", "Saved")?;
    Ok(back)
}

async fn page() -> View {
    view(
        "page.html",
        context! { items => ["a", "b"], title => "List" },
    )
    .fragment("list")
}

async fn create() -> View {
    view("create.html", ())
}

async fn store(session: Session, back: Back, axum::Form(note): axum::Form<Note>) -> Result<Back> {
    session.flash_input(&note)?;
    session.flash_errors(&context! { title => ["The title is taken."] })?;
    Ok(back)
}

const PAGE: &str = r#"<html><head>{{ renox_head() }}</head><body>
<h1>{{ title }} - {{ app.name }}</h1>
<a href="{{ route('notes.show', 3) }}">note</a>
{% block list %}<ul>{% for i in items %}<li>{{ i }}</li>{% endfor %}</ul>{% endblock %}
<p>{{ request.path }} {{ flash.status }}</p>
</body></html>"#;

const CREATE: &str = r#"<form method="post">{{ csrf_field() }}
<input name="title" value="{{ old('title') }}">
{% for e in errors.title %}<span class="error">{{ e }}</span>{% endfor %}
</form>"#;

async fn app() -> TestApp {
    TestApp::new(
        true,
        &[
            ("page.html", PAGE),
            ("create.html", CREATE),
            ("errors/403.html", "custom forbidden"),
        ],
        |app| app.module(Demo),
    )
    .await
}

async fn token(app: &mut TestApp) -> String {
    app.get("/token").await.body
}

#[tokio::test]
async fn module_routes_receive_app_state() {
    let res = app().await.get("/").await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body, "Test App");
}

#[tokio::test]
async fn named_routes_build_urls() {
    assert_eq!(app().await.get("/notes/1").await.body, "/notes/7");
}

#[tokio::test]
async fn unknown_route_renders_the_builtin_error_template() {
    let res = app().await.get("/nope").await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert!(res.body.contains("404 · Not Found"));
    assert!(
        res.body.contains("<style>"),
        "rendered from renox/error.html"
    );
}

#[tokio::test]
async fn app_templates_override_error_pages() {
    struct Forbidden;
    impl Module for Forbidden {
        fn name(&self) -> &'static str {
            "forbidden"
        }
        fn routes(&self) -> Routes {
            Routes::new().get("/", || async { renox_core::Error::Forbidden })
        }
    }
    let mut app = TestApp::new(true, &[("errors/403.html", "custom forbidden")], |a| {
        a.module(Forbidden)
    })
    .await;
    let res = app.get("/").await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body, "custom forbidden");
}

#[tokio::test]
async fn internal_error_shows_detail_in_debug() {
    let res = app().await.get("/fail").await;
    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(res.body.contains("invalid digit"));
}

#[tokio::test]
async fn session_persists_between_requests() {
    let mut app = app().await;
    assert_eq!(app.get("/counter").await.body, "1");
    assert_eq!(app.get("/counter").await.body, "2");
    app.clear_cookies();
    assert_eq!(app.get("/counter").await.body, "1");
}

#[tokio::test]
async fn tampered_session_cookie_is_ignored() {
    let mut app = app().await;
    app.get("/counter").await;
    app.clear_cookies();
    let res = app
        .send(
            axum::http::Request::get("/counter")
                .header("cookie", "renox_session=forged")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.body, "1");
}

#[tokio::test]
async fn flash_lasts_for_one_request() {
    let mut app = app().await;
    let token = token(&mut app).await;
    let res = app.post_form("/flash", &format!("_token={token}")).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(app.get("/flashed").await.body, "Saved");
    assert_eq!(app.get("/flashed").await.body, "");
}

#[tokio::test]
async fn csrf_rejects_requests_without_a_valid_token() {
    let mut app = app().await;
    token(&mut app).await;
    assert_eq!(app.post_form("/flash", "").await.status.as_u16(), 419);
    assert_eq!(
        app.post_form("/flash", "_token=wrong")
            .await
            .status
            .as_u16(),
        419
    );
    let res = app.htmx_post("/hx", "wrong").await;
    assert_eq!(res.status.as_u16(), 419);
    assert!(res.body.contains("419 · Page Expired"));
}

#[tokio::test]
async fn csrf_accepts_the_header_used_by_htmx() {
    let mut app = app().await;
    let token = token(&mut app).await;
    let res = app.htmx_post("/hx", &token).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.headers["hx-trigger"], "saved");
}

#[tokio::test]
async fn views_render_with_globals() {
    let mut app = app().await;
    let res = app.get("/page").await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.headers["content-type"], "text/html; charset=utf-8");
    assert!(res.body.contains("<h1>List - Test App</h1>"));
    assert!(res.body.contains(r#"href="/notes/3""#));
    assert!(res.body.contains("<li>a</li><li>b</li>"));
    assert!(res.body.contains("<p>/page </p>"), "`/` is not escaped");
    assert!(res.body.contains(r#"<meta name="csrf-token" content=""#));
    assert!(res.body.contains("/_renox/htmx-2.0.11.min.js"));
}

#[tokio::test]
async fn htmx_requests_get_only_the_fragment() {
    let res = app().await.htmx_get("/page").await;
    assert_eq!(res.body, "<ul><li>a</li><li>b</li></ul>");
    assert_eq!(app().await.htmx_get("/hx").await.body, "true true");
}

#[tokio::test]
async fn old_input_and_errors_survive_the_redirect() {
    let mut app = app().await;
    let token = token(&mut app).await;
    app.get("/notes/create").await;
    app.post_form("/notes", &format!("_token={token}&title=Hello+%3Cb%3E"))
        .await;
    let res = app.get("/notes/create").await;
    assert!(
        res.body.contains(r#"value="Hello &lt;b&gt;""#),
        "{}",
        res.body
    );
    assert!(
        res.body
            .contains(r#"<span class="error">The title is taken.</span>"#)
    );
    assert!(
        res.body
            .contains(&format!(r#"name="_token" value="{token}""#))
    );
}

#[tokio::test]
async fn embedded_assets_are_served_with_long_caching() {
    let mut app = app().await;
    let page = app.get("/page").await.body;
    let src = page
        .split("src=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap()
        .to_owned();
    let res = app.get(&src).await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(
        res.headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    assert!(res.body.starts_with("var htmx"));
}

#[tokio::test]
async fn public_files_are_served() {
    let res = app().await.get("/robots.txt").await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body, "User-agent: *");
}

#[tokio::test]
async fn internal_error_hides_detail_without_debug() {
    // Runs next to the debug tests above: each app decides for itself.
    let mut app = TestApp::new(false, &[], |app| app.module(Demo)).await;
    let res = app.get("/fail").await;
    assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(res.body.contains("500 · Internal Server Error"));
    assert!(!res.body.contains("invalid digit"));
}
