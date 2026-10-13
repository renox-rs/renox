//! Security of the web layer: sessions, CSRF, login, redirects, validation,
//! XSS, uploads, headers and the client IP. Started as the probes of the
//! pre-1.0 review (`docs/audit/2026-09-pre-1.0.md`); every test asserts the
//! secure behaviour.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use renox::TrustedProxies;
use renox::prelude::*;
use renox::testing::{TestApp, TestResponse};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRsmall-but-valid-enough-for-sniffing";
const XSS: &str = "\"'><script>alert(1)</script>";

// ---------------------------------------------------------------- fixture

#[derive(Serialize, Clone)]
struct Post {
    id: i64,
    user_id: i64,
}

impl Policy for Post {
    fn allows(&self, user: &User, _: &str) -> bool {
        self.user_id == user.id
    }
}

#[derive(Deserialize, Serialize)]
struct ProfileForm {
    name: String,
    age: Option<i64>,
}

impl Validate for ProfileForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required().max(50);
        v.field("age", &self.age).min(0).max(150);
    }
}

#[derive(Deserialize)]
struct ScoreForm {
    score: f64,
}

impl Validate for ScoreForm {
    fn rules(&self, v: &mut Validator) {
        v.field("score", &self.score).required();
    }
}

#[derive(Deserialize)]
struct AnyFileForm {
    file: Option<Upload>,
}

impl Validate for AnyFileForm {
    fn rules(&self, v: &mut Validator) {
        v.field("file", &self.file).max(8192); // KB, below UPLOAD_MAX_SIZE
    }
}

#[derive(Deserialize)]
struct ImageForm {
    file: Option<Upload>,
}

impl Validate for ImageForm {
    fn rules(&self, v: &mut Validator) {
        v.field("file", &self.file).image().max(1024);
    }
}

#[derive(Deserialize)]
struct PngOrSvgForm {
    file: Upload,
}

impl Validate for PngOrSvgForm {
    fn rules(&self, v: &mut Validator) {
        v.field("file", &self.file)
            .required()
            .mimes(&["png", "svg"]);
    }
}

struct Probe;

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn routes(&self) -> Routes {
        let public = Routes::new()
            .get("/", home)
            .name("home")
            .get("/token", |session: Session| async move { session.token() })
            .get("/ip", |ClientIp(ip): ClientIp| async move {
                ip.map_or("unknown".to_owned(), |ip| ip.to_string())
            })
            .get("/whoami", |user: Option<AuthUser>| async move {
                user.map_or("guest".to_owned(), |u| format!("user:{}", u.email))
            })
            .get("/back", |back: Back| async move { back })
            .post("/profile", |Valid(f): Valid<ProfileForm>| async move {
                format!("ok:{}", f.name.len())
            })
            .post("/score", |Valid(f): Valid<ScoreForm>| async move {
                format!("score:{}", f.score)
            })
            .post("/api/echo", |Valid(f): Valid<ProfileForm>| async move {
                Json(json!({ "name": f.name }))
            })
            .post("/flash", flash)
            .post("/only-post", || async { "posted" })
            .put("/items/{id}", |Path(id): Path<i64>| async move {
                format!("put {id}")
            })
            .delete("/items/{id}", |Path(id): Path<i64>| async move {
                format!("deleted {id}")
            })
            .post("/upload/any", upload_any)
            .post("/upload/image", upload_image)
            .post("/upload/png-or-svg", upload_png_or_svg)
            .get("/private-link/{name}", private_link)
            .get("/own-permissions", || async {
                ([("permissions-policy", "camera=(self)")], "mine")
            })
            .get("/boom", || async {
                Err::<String, Error>(
                    anyhow::anyhow!("db password=hunter2 <script>alert(9)</script>").into(),
                )
            })
            .get(
                "/lang/{locale}",
                |session: Session, back: Back, Path(l): Path<String>| async move {
                    renox::i18n::remember_locale(&session, &l).unwrap();
                    back
                },
            );
        let cors = Routes::new()
            .post("/api/cors", || async { "cors" })
            .cors(&["https://app.example.com"]);
        let authed = Routes::new()
            .get("/dashboard", || async { "dashboard" })
            .get("/api/me", |u: AuthUser| async move {
                Json(json!({ "email": u.email }))
            })
            .get("/admin", |u: AuthUser| async move {
                u.gate("admin").map(|()| "admin")
            })
            .get("/unknown-gate", |u: AuthUser| async move {
                u.gate("no-such-gate").map(|()| "in")
            })
            .get("/posts/{id}/edit", |u: AuthUser| async move {
                u.authorize(
                    "update",
                    &Post {
                        id: 1,
                        user_id: 999_999,
                    },
                )
                .map(|()| "edit")
            })
            .require_auth();
        let verified = Routes::new()
            .get("/verified", || async { "verified-only" })
            .require_verified();
        public.merge(cors).merge(authed).merge(verified)
    }
}

async fn home(Query(q): Query<HashMap<String, String>>, user: Option<AuthUser>) -> View {
    let q = q.get("q").cloned().unwrap_or_default();
    let post = Can::new(Post { id: 1, user_id: 1 }, user.as_deref(), &["update"]);
    let items: Paginated<i64> = Paginated::new(vec![1], 1, 1, 3);
    view("page.html", context! { q, post, items })
}

async fn flash(session: Session, Form(f): Form<HashMap<String, String>>) -> Redirect {
    session
        .flash("status", f.get("msg").cloned().unwrap_or_default())
        .unwrap();
    Redirect::to("/")
}

async fn upload_any(State(state): State<AppState>, Valid(f): Valid<AnyFileForm>) -> Result<String> {
    Ok(match f.file {
        Some(file) => state
            .storage
            .url(&file.store_public(&state.storage, "any").await?),
        None => "none".into(),
    })
}

async fn upload_image(State(state): State<AppState>, Valid(f): Valid<ImageForm>) -> Result<String> {
    Ok(match f.file {
        Some(file) => state
            .storage
            .url(&file.store_public(&state.storage, "img").await?),
        None => "none".into(),
    })
}

async fn upload_png_or_svg(
    State(state): State<AppState>,
    Valid(f): Valid<PngOrSvgForm>,
) -> Result<String> {
    Ok(state
        .storage
        .url(&f.file.store_public(&state.storage, "pics").await?))
}

async fn private_link(State(state): State<AppState>, Path(name): Path<String>) -> Result<String> {
    let key = format!("private/{name}");
    state
        .storage
        .put(
            &key,
            format!("<html><script>alert('{name}')</script></html>").into(),
        )
        .await?;
    state
        .storage
        .temporary_url(&state, &key, Duration::from_secs(60))
        .await
}

const PAGE: &str = r#"<head>{{ seo(title=q, description=q, image=q) }}{{ renox_head() }}</head>
<p id="old">{{ old('name') }}</p><input id="oldattr" value="{{ old('name') }}">
<p id="err">{{ error('name') }}</p>
<p id="flash">{{ flash.status }}</p>
<a id="attr" title="{{ q }}">{{ q }}</a>
<p id="t">{{ t('greet', name=q) }}</p>
<a id="pg" href="{{ page_url(2) }}">next</a>
{% from "renox/pagination.html" import pagination %}{{ pagination(items) }}
{% if can('update', post) %}EDIT{% endif %}{% if can('admin') %}ADMIN{% endif %}{% if can('no-such-gate') %}NOGATE{% endif %}
"#;

struct Fixture {
    app: TestApp,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for Fixture {
    type Target = TestApp;
    fn deref(&self) -> &TestApp {
        &self.app
    }
}

async fn fixture_with(auth: Auth, configure: impl FnOnce(&mut Config) + Send) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let views = dir.path().join("views");
    let lang = dir.path().join("lang");
    let public = dir.path().join("public");
    for d in [&views, &lang, &public] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(views.join("page.html"), PAGE).unwrap();
    std::fs::write(lang.join("en.json"), r#"{"greet": "Hello :name"}"#).unwrap();
    std::fs::write(public.join("app.css"), "body{}").unwrap();
    let app = TestApp::with_config(
        App::new()
            .module(auth)
            .module(Probe)
            .gate("admin", |u| u.email.ends_with("@admin.test")),
        |c| {
            c.views_path = views;
            c.lang_path = lang;
            c.public_path = public;
            configure(c);
        },
    )
    .await;
    Fixture { app, _dir: dir }
}

async fn fixture() -> Fixture {
    fixture_with(Auth::new(), |_| {}).await
}

async fn alex(app: &TestApp) -> User {
    User::register(app.db(), "Alex", "alex@example.com", "letmein123")
        .await
        .unwrap()
}

fn session_pair(res: &TestResponse) -> Option<String> {
    res.headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("renox_session="))
        .map(|v| v.split(';').next().unwrap().to_owned())
}

fn set_cookie_line(res: &TestResponse, name: &str) -> Option<String> {
    res.headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(str::to_owned)
}

/// Sends a request straight to the router (no TestApp cookie jar / CSRF).
async fn raw(app: &TestApp, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app.kernel().router().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

/// A request that carries only `cookie` (e.g. an old or foreign one).
async fn get_with_cookie(app: &TestApp, uri: &str, cookie: &str) -> String {
    let req = Request::get(uri)
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap();
    raw(app, req).await.2
}

fn assert_security_headers(res_headers: &HeaderMap, what: &str) {
    for h in [
        "x-content-type-options",
        "x-frame-options",
        "referrer-policy",
        "content-security-policy",
    ] {
        assert!(
            res_headers.get(h).is_some(),
            "{what}: missing {h} in {res_headers:?}"
        );
    }
}

fn multipart(fields: &[(&str, &str)], files: &[(&str, &str, &[u8])], boundary: &str) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, file_name, bytes) in files {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

// ---------------------------------------------------------- 1. sessions

#[renox::test]
async fn session_garbage_tampered_truncated_cookies_are_ignored() {
    let app = fixture().await;
    let user = alex(&app).await;
    app.acting_as(&user);
    let good = session_pair(&app.get("/whoami").await).unwrap();
    assert_eq!(
        get_with_cookie(&app, "/whoami", &good).await,
        "user:alex@example.com"
    );

    let value = good.trim_start_matches("renox_session=");
    let mut flipped: Vec<char> = value.chars().collect();
    let mid = flipped.len() / 2;
    flipped[mid] = if flipped[mid] == 'A' { 'B' } else { 'A' };
    let flipped: String = flipped.into_iter().collect();
    for cookie in [
        format!("renox_session={flipped}"),
        format!("renox_session={}", &value[..value.len() / 2]),
        "renox_session=garbage%%%".to_owned(),
        "renox_session=".to_owned(),
        format!("renox_session={}", "A".repeat(200_000)),
        format!("renox_session={value}; renox_session=garbage"),
    ] {
        let req = Request::get("/whoami")
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        let (status, _, body) = raw(&app, req).await;
        assert_eq!(status, 200, "cookie {:.40}", cookie);
        if !cookie.contains(value) {
            assert_eq!(body, "guest", "cookie {:.40}", cookie);
        }
    }
}

#[renox::test]
async fn session_cookie_from_another_app_key_is_ignored() {
    let a = fixture().await;
    let b = fixture().await; // new random APP_KEY
    let user = alex(&a).await;
    alex(&b).await;
    a.acting_as(&user);
    let cookie = session_pair(&a.get("/whoami").await).unwrap();
    assert_eq!(get_with_cookie(&b, "/whoami", &cookie).await, "guest");
}

#[renox::test]
async fn session_login_rotates_csrf_token() {
    let app = fixture().await;
    alex(&app).await;
    let before = app.get("/token").await.text();
    app.post(
        "/login",
        &[("email", "alex@example.com"), ("password", "letmein123")],
    )
    .await
    .assert_redirect("/");
    let after = app.get("/token").await.text();
    assert_ne!(before, after, "login must rotate the CSRF token");
    // The pre-login token no longer works.
    app.request()
        .without_csrf()
        .header("x-csrf-token", &before)
        .post("/profile", &[("name", "x")])
        .await
        .assert_status(419);
}

#[renox::test]
async fn session_logout_clears_session_and_token() {
    let app = fixture().await;
    alex(&app).await;
    app.post(
        "/login",
        &[("email", "alex@example.com"), ("password", "letmein123")],
    )
    .await;
    let token = app.get("/token").await.text();
    app.post("/logout", &[]).await.assert_redirect("/");
    assert_eq!(app.get("/whoami").await.text(), "guest");
    assert_ne!(app.get("/token").await.text(), token);
}

/// A cookie copied before logout no longer authenticates (sessions are
/// stateless cookies; logout revokes them through `users.sessions_revoked_at`).
#[renox::test]
async fn session_old_cookie_is_dead_after_logout() {
    let app = fixture().await;
    alex(&app).await;
    let login = app
        .post(
            "/login",
            &[("email", "alex@example.com"), ("password", "letmein123")],
        )
        .await;
    let stolen = session_pair(&login).unwrap();
    app.post("/logout", &[]).await.assert_redirect("/");
    assert_eq!(
        get_with_cookie(&app, "/whoami", &stolen).await,
        "guest",
        "a cookie captured before logout still authenticates after logout"
    );
}

#[renox::test]
async fn session_remember_me_cookie_flags() {
    let app = fixture().await;
    alex(&app).await;
    let res = app
        .post(
            "/login",
            &[
                ("email", "alex@example.com"),
                ("password", "letmein123"),
                ("remember", "1"),
            ],
        )
        .await;
    let line = set_cookie_line(&res, "renox_session").unwrap();
    assert!(line.contains("HttpOnly"), "{line}");
    assert!(line.contains("SameSite=Lax"), "{line}");
    assert!(line.contains(&format!("Max-Age={}", 43200 * 60)), "{line}");
}

#[renox::test]
async fn session_cookie_is_secure_on_https() {
    let app = fixture_with(Auth::new(), |c| c.url = "https://shop.example.com".into()).await;
    let line = set_cookie_line(&app.get("/").await, "renox_session").unwrap();
    assert!(line.contains("Secure"), "{line}");
}

#[renox::test]
async fn session_password_change_kills_other_sessions() {
    let app = fixture().await;
    let mut user = alex(&app).await;
    let login = app
        .post(
            "/login",
            &[("email", "alex@example.com"), ("password", "letmein123")],
        )
        .await;
    let other_device = session_pair(&login).unwrap();
    user.set_password(app.db(), "new-letmein-123")
        .await
        .unwrap();
    assert_eq!(
        get_with_cookie(&app, "/whoami", &other_device).await,
        "guest"
    );
}

/// Session data (old input) grows the cookie; a big failed form must not
/// produce a Set-Cookie browsers/proxies drop (>4 KB).
#[renox::test]
async fn session_cookie_stays_small_after_big_failed_form() {
    let app = fixture().await;
    let long = "x".repeat(20_000); // fails max(50)
    let res = app
        .request()
        .header("referer", "/")
        .post("/profile", &[("name", &long)])
        .await;
    res.assert_status(303);
    let line = set_cookie_line(&res, "renox_session").unwrap();
    assert!(
        line.len() <= 4096,
        "Set-Cookie is {} bytes after a failed form with a 20 KB field",
        line.len()
    );
}

// -------------------------------------------------------------- 2. CSRF

#[renox::test]
async fn csrf_missing_or_wrong_token_on_every_method() {
    let app = fixture().await;
    app.get("/").await;
    for wrong in [None, Some("wrong"), Some("")] {
        let req = || match wrong {
            Some(t) => app.request().without_csrf().header("x-csrf-token", t),
            None => app.request().without_csrf(),
        };
        req()
            .post("/profile", &[("name", "x")])
            .await
            .assert_status(419);
        req().put("/items/1", &[]).await.assert_status(419);
        req().delete("/items/1").await.assert_status(419);
        req()
            .post("/items/1", &[("_method", "DELETE")])
            .await
            .assert_status(419);
        req()
            .post("/only-post", &[("_token", "wrong")])
            .await
            .assert_status(419);
    }
    // PATCH to a route without PATCH: CSRF runs first either way.
    app.request()
        .without_csrf()
        .patch("/items/1", &[])
        .await
        .assert_status(419);
}

#[renox::test]
async fn csrf_token_field_and_header() {
    let app = fixture().await;
    let token = app.csrf_token();
    app.request()
        .without_csrf()
        .post("/profile", &[("name", "x"), ("_token", &token)])
        .await
        .assert_ok();
    app.request()
        .without_csrf()
        .header("x-csrf-token", &token)
        .post("/profile", &[("name", "x")])
        .await
        .assert_ok();
    // A wrong header wins over a right field.
    app.request()
        .without_csrf()
        .header("x-csrf-token", "nope")
        .post("/profile", &[("name", "x"), ("_token", &token)])
        .await
        .assert_status(419);
    // Token in the query string is not accepted.
    app.request()
        .without_csrf()
        .post(&format!("/profile?_token={token}"), &[("name", "x")])
        .await
        .assert_status(419);
}

#[renox::test]
async fn csrf_multipart_token_after_large_file() {
    let app = fixture().await;
    let token = app.csrf_token();
    let big = vec![b'a'; 2 * 1024 * 1024];
    let body = multipart(
        &[("_token", &token)],
        &[("file", "big.txt", &big)],
        "b0undary",
    );
    app.request()
        .without_csrf()
        .post_body(
            "/upload/any",
            "multipart/form-data; boundary=b0undary",
            body,
        )
        .await
        .assert_ok();
    let body = multipart(
        &[("_token", "wrong")],
        &[("file", "big.txt", &big)],
        "b0undary",
    );
    app.request()
        .without_csrf()
        .post_body(
            "/upload/any",
            "multipart/form-data; boundary=b0undary",
            body,
        )
        .await
        .assert_status(419);
}

#[renox::test]
async fn csrf_json_requests() {
    let app = fixture().await;
    app.request()
        .without_csrf()
        .post_json("/api/echo", &json!({ "name": "x" }))
        .await
        .assert_status(419);
    app.request()
        .without_csrf()
        .post_json(
            "/api/echo",
            &json!({ "name": "x", "_token": app.csrf_token() }),
        )
        .await
        .assert_status(419);
    app.post_json("/api/echo", &json!({ "name": "x" }))
        .await
        .assert_ok();
    // text/plain (a CORS "simple" request) without a token.
    app.request()
        .without_csrf()
        .post_body("/profile", "text/plain", "name=x")
        .await
        .assert_status(419);
}

#[renox::test]
async fn csrf_valid_bearer_skips_csrf() {
    let app = fixture().await;
    let user = alex(&app).await;
    let token = user.create_token(app.db(), "cli", None).await.unwrap();
    app.request()
        .without_csrf()
        .header("authorization", &format!("Bearer {}", token.plain))
        .post("/profile", &[("name", "x")])
        .await
        .assert_ok();
}

/// A *wrong* Bearer token must not switch CSRF off.
#[renox::test]
async fn csrf_wrong_bearer_does_not_skip_csrf() {
    let app = fixture().await;
    alex(&app).await;
    for bearer in [
        "Bearer garbage",
        "Bearer 1|wrong",
        "Bearer 999|x",
        "Bearer ",
    ] {
        let res = app
            .request()
            .without_csrf()
            .header("authorization", bearer)
            .post("/profile", &[("name", "x")])
            .await;
        // Refused either as bad credentials (401) or for the missing CSRF
        // token (419); what matters is that the handler never runs.
        assert!(
            matches!(res.status.as_u16(), 401 | 419),
            "`{bearer}` without a CSRF token got {} {:?}",
            res.status,
            res.text()
        );
    }
}

/// Nor can a junk Bearer token forge a logout or login.
#[renox::test]
async fn csrf_wrong_bearer_cannot_forge_session_writes() {
    let app = fixture().await;
    alex(&app).await;
    let res = app
        .request()
        .without_csrf()
        .header("authorization", "Bearer x")
        .post("/flash", &[("msg", "forged")])
        .await;
    assert!(
        matches!(res.status.as_u16(), 401 | 419),
        "forged session write accepted: {}",
        res.status
    );
}

// -------------------------------------------------------------- 3. auth

#[renox::test]
async fn auth_wrong_password_and_unknown_email_look_the_same() {
    let app = fixture().await;
    alex(&app).await;
    let a = app
        .htmx()
        .post(
            "/login",
            &[("email", "alex@example.com"), ("password", "wrong-pass")],
        )
        .await;
    let b = app
        .htmx()
        .post(
            "/login",
            &[("email", "nobody@example.com"), ("password", "wrong-pass")],
        )
        .await;
    assert_eq!(a.status, b.status);
    assert_eq!(a.text(), b.text());
}

#[renox::test]
async fn auth_throttle_after_five_attempts_including_case_variants() {
    let app = fixture().await;
    alex(&app).await;
    for i in 0..5 {
        let email = if i % 2 == 0 {
            "ALEX@example.com"
        } else {
            "alex@EXAMPLE.com"
        };
        let res = app
            .htmx()
            .post("/login", &[("email", email), ("password", "wrong-pass")])
            .await;
        assert_eq!(
            res.status.as_u16(),
            422,
            "attempt {i}: {} {:?} {}",
            res.status,
            res.headers,
            res.text()
        );
    }
    let res = app
        .htmx()
        .post(
            "/login",
            &[("email", "alex@example.com"), ("password", "letmein123")],
        )
        .await;
    res.assert_status(422);
    assert!(res.text().contains("Too many"), "{}", res.text());
}

async fn login_from(app: &TestApp, ip: &str, email: &str, password: &str) -> StatusCode {
    let probe = app.get("/token").await;
    let token = probe.text();
    let cookie = session_pair(&probe).unwrap();
    let body = format!("email={}&password={}", enc(email), enc(password));
    let mut req = Request::post("/login")
        .header("cookie", cookie)
        .header("x-csrf-token", token)
        .header("hx-request", "true")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();
    let addr: SocketAddr = format!("{ip}:5555").parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    raw(app, req).await.0
}

/// Rotating IPs doesn't give unlimited guesses against one account.
#[renox::test]
async fn auth_throttle_holds_across_ips() {
    let app = fixture().await;
    alex(&app).await;
    for i in 0..5 {
        assert_eq!(
            login_from(&app, "10.0.0.1", "alex@example.com", "x-wrong").await,
            422,
            "{i}"
        );
    }
    // Same IP is locked even with the right password.
    let locked = login_from(&app, "10.0.0.1", "alex@example.com", "letmein123").await;
    assert_eq!(locked, 422);
    // 30 more guesses, each from a fresh IP.
    for i in 0..30 {
        login_from(&app, &format!("10.1.0.{i}"), "alex@example.com", "x-wrong").await;
    }
    let from_new_ip = login_from(&app, "10.2.0.1", "alex@example.com", "letmein123").await;
    assert_ne!(
        from_new_ip.as_u16(),
        200,
        "35 failed guesses from rotating IPs, the account is still not locked"
    );
}

#[renox::test]
async fn auth_intended_url_cannot_leave_the_site() {
    // Only same-site paths are honoured.
    let app = fixture().await;
    alex(&app).await;
    app.get("/dashboard?x=1").await.assert_redirect("/login");
    app.post(
        "/login",
        &[("email", "alex@example.com"), ("password", "letmein123")],
    )
    .await
    .assert_redirect("/dashboard?x=1");
    // Backslash / protocol-relative forms can't even be requested as a path.
}

/// `Back` (and the validation redirect) follow only a same-site Referer.
#[renox::test]
async fn auth_back_does_not_redirect_off_site() {
    let app = fixture().await;
    for evil in [
        "https://evil.com/x",
        "//evil.com",
        "/\\evil.com",
        "javascript:alert(1)",
    ] {
        let res = app.request().header("referer", evil).get("/back").await;
        let loc = res.header("location").unwrap_or_default().to_owned();
        assert!(
            loc.starts_with('/') && !loc.starts_with("//") && !loc.starts_with("/\\"),
            "Back redirected to `{loc}` for Referer `{evil}`"
        );
    }
}

#[renox::test]
async fn auth_validation_redirect_does_not_go_off_site() {
    let app = fixture().await;
    let res = app
        .request()
        .header("referer", "https://evil.com/phish")
        .post("/profile", &[("name", "")])
        .await;
    res.assert_status(303);
    let loc = res.header("location").unwrap_or_default();
    assert!(
        !loc.contains("evil.com"),
        "validation failure redirected to {loc}"
    );
}

#[renox::test]
async fn auth_set_locale_link_is_not_an_open_redirect() {
    let app = fixture().await;
    let res = app
        .request()
        .header("referer", "https://evil.com/")
        .get("/lang/es")
        .await;
    let loc = res.header("location").unwrap_or_default();
    assert!(
        !loc.contains("evil.com"),
        "GET /lang/es redirected to {loc}"
    );
}

#[renox::test]
async fn auth_deleted_user_session_is_dead() {
    let app = fixture().await;
    let user = alex(&app).await;
    app.acting_as(&user);
    assert_eq!(app.get("/whoami").await.text(), "user:alex@example.com");
    renox::db::sql("DELETE FROM users WHERE id = ?")
        .bind(user.id)
        .execute(app.db())
        .await
        .unwrap();
    assert_eq!(app.get("/whoami").await.text(), "guest");
    app.get("/dashboard").await.assert_redirect("/login");
}

#[renox::test]
async fn auth_unverified_user_on_verified_route() {
    let app = fixture_with(Auth::new().verify_email(), |_| {}).await;
    let user = alex(&app).await;
    app.acting_as(&user);
    app.get("/verified").await.assert_redirect("/verify-email");
    app.request()
        .json()
        .get("/verified")
        .await
        .assert_status(403);
    app.htmx()
        .get("/verified")
        .await
        .assert_hx_redirect("/verify-email");
}

#[renox::test]
async fn auth_logged_in_user_on_guest_pages() {
    let app = fixture().await;
    let user = alex(&app).await;
    app.acting_as(&user);
    for p in [
        "/login",
        "/register",
        "/forgot-password",
        "/reset-password/abc",
    ] {
        app.get(p).await.assert_redirect("/");
    }
}

fn reset_token(app: &TestApp, to: &str) -> String {
    let mail = app
        .sent_mail()
        .into_iter()
        .rev()
        .find(|m| m.is_for(to))
        .unwrap();
    let url = mail
        .text
        .split_whitespace()
        .find(|w| w.starts_with("http"))
        .unwrap()
        .to_owned();
    url.split("/reset-password/")
        .nth(1)
        .unwrap()
        .split('?')
        .next()
        .unwrap()
        .to_owned()
}

async fn reset(app: &TestApp, token: &str, email: &str) -> TestResponse {
    app.htmx()
        .post(
            "/reset-password",
            &[
                ("token", token),
                ("email", email),
                ("password", "brand-new-pass"),
                ("password_confirmation", "brand-new-pass"),
            ],
        )
        .await
}

#[renox::test]
async fn auth_password_reset_token_misuse() {
    let app = fixture().await;
    alex(&app).await;
    User::register(app.db(), "Ben", "ben@example.com", "ben-secret-1")
        .await
        .unwrap();
    app.post("/forgot-password", &[("email", "alex@example.com")])
        .await;
    app.post("/forgot-password", &[("email", "ben@example.com")])
        .await;
    let alex_token = reset_token(&app, "alex@example.com");

    // Alex's token on Ben's account.
    reset(&app, &alex_token, "ben@example.com")
        .await
        .assert_status(422);
    let ben = User::find_by_email(app.db(), "ben@example.com")
        .await
        .unwrap()
        .unwrap();
    assert!(ben.check_password("ben-secret-1").await);

    // Works once (case-variant email), then not again.
    reset(&app, &alex_token, "ALEX@example.com")
        .await
        .assert_hx_redirect("/login");
    reset(&app, &alex_token, "alex@example.com")
        .await
        .assert_status(422);

    // Expired.
    app.post("/forgot-password", &[("email", "ben@example.com")])
        .await;
    let ben_token = reset_token(&app, "ben@example.com");
    renox::db::sql("UPDATE password_reset_tokens SET created_at = ?")
        .bind(renox::db::now() - renox::chrono::TimeDelta::hours(2))
        .execute(app.db())
        .await
        .unwrap();
    reset(&app, &ben_token, "ben@example.com")
        .await
        .assert_status(422);
}

#[renox::test]
async fn auth_password_reset_revokes_api_tokens() {
    let app = fixture().await;
    let user = alex(&app).await;
    let api = user.create_token(app.db(), "cli", None).await.unwrap();
    app.post("/forgot-password", &[("email", "alex@example.com")])
        .await;
    let token = reset_token(&app, "alex@example.com");
    reset(&app, &token, "alex@example.com")
        .await
        .assert_hx_redirect("/login");
    app.logout();
    let res = app
        .request()
        .header("authorization", &format!("Bearer {}", api.plain))
        .get("/api/me")
        .await;
    assert_eq!(
        res.status.as_u16(),
        401,
        "an API token created before a password reset still works"
    );
}

#[renox::test]
async fn auth_reset_form_does_not_enumerate_emails() {
    let app = fixture().await;
    alex(&app).await;
    let a = app
        .htmx()
        .post("/forgot-password", &[("email", "alex@example.com")])
        .await;
    let a_page = app.get("/forgot-password").await.text();
    let b = app
        .htmx()
        .post("/forgot-password", &[("email", "ghost@example.com")])
        .await;
    let b_page = app.get("/forgot-password").await.text();
    assert_eq!(a.status, b.status);
    assert_eq!(a.header("hx-redirect"), b.header("hx-redirect"));
    let strip = |s: &str| s.split("csrf").next().unwrap().to_owned();
    assert_eq!(strip(&a_page), strip(&b_page));
}

#[renox::test]
async fn auth_email_case_and_whitespace() {
    let app = fixture().await;
    app.post(
        "/register",
        &[
            ("name", "Alex"),
            ("email", "  Alex@Example.COM "),
            ("password", "letmein123"),
            ("password_confirmation", "letmein123"),
        ],
    )
    .await
    .assert_redirect("/");
    app.assert_database_has("users", &[("email", &"alex@example.com")])
        .await;
    app.post("/logout", &[]).await;
    app.htmx()
        .post(
            "/register",
            &[
                ("name", "Dup"),
                ("email", "ALEX@example.com"),
                ("password", "letmein123"),
                ("password_confirmation", "letmein123"),
            ],
        )
        .await
        .assert_invalid("email");
    app.post(
        "/login",
        &[("email", " ALEX@EXAMPLE.COM"), ("password", "letmein123")],
    )
    .await
    .assert_redirect("/");
}

// ----------------------------------------------------- 4. authorization

#[renox::test]
async fn authz_policies_gates_and_templates() {
    let app = fixture().await;
    // Guest: Can in templates is false, gates are false.
    let page = app.get("/").await.text();
    assert!(!page.contains("EDIT") && !page.contains("ADMIN") && !page.contains("NOGATE"));
    let user = alex(&app).await; // id 1 owns the post
    app.acting_as(&user);
    let page = app.get("/").await.text();
    assert!(page.contains("EDIT") && !page.contains("ADMIN") && !page.contains("NOGATE"));
    app.get("/posts/1/edit").await.assert_forbidden();
    app.get("/admin").await.assert_forbidden();
    app.get("/unknown-gate").await.assert_forbidden();
    app.request().json().get("/admin").await.assert_forbidden();
}

// -------------------------------------------------------- 5. validation

#[renox::test]
async fn validation_huge_unicode_nul() {
    let app = fixture().await;
    app.htmx()
        .post("/profile", &[("name", &"x".repeat(100_000))])
        .await
        .assert_invalid("name");
    app.htmx()
        .post("/profile", &[("name", &"😀".repeat(50))])
        .await
        .assert_ok();
    app.htmx()
        .post("/profile", &[("name", &"😀".repeat(51))])
        .await
        .assert_invalid("name");
    // Combining marks: 50 "characters" as chars, but each is 2 code points.
    let res = app.htmx().post("/profile", &[("name", "a\u{0}b")]).await;
    assert_ne!(res.status.as_u16(), 500, "NUL byte: {}", res.text());
    // Over the 2 MB form limit.
    let res = app
        .htmx()
        .post("/profile", &[("name", &"x".repeat(3 * 1024 * 1024))])
        .await;
    res.assert_status(413);
}

#[renox::test]
async fn validation_array_where_scalar_expected() {
    let app = fixture().await;
    for body in [
        "name%5B%5D=a",
        "name%5Bx%5D=b",
        "name=a&name=b",
        "name=a&age=1&age=2",
    ] {
        let res = app
            .request()
            .htmx()
            .header("x-csrf-token", &app.csrf_token())
            .without_csrf()
            .post_body("/profile", "application/x-www-form-urlencoded", body)
            .await;
        assert_eq!(
            res.status.as_u16(),
            422,
            "{body}: {} {}",
            res.status,
            res.text()
        );
    }
}

/// Nested names (`a[b][c]`) are read as a tree; one far deeper than any form
/// is ignored instead of recursing, and the rest of the form still counts.
#[renox::test]
async fn nested_names_are_bounded() {
    let app = fixture().await;
    let body = format!("name=x&a{}=1&age=3", "%5Bb%5D".repeat(5_000));
    let res = app
        .request()
        .htmx()
        .header("x-csrf-token", &app.csrf_token())
        .without_csrf()
        .post_body("/profile", "application/x-www-form-urlencoded", body)
        .await;
    assert_eq!(res.status.as_u16(), 200, "{}", res.text());
}

#[renox::test]
async fn validation_integer_overflow_and_float_specials() {
    let app = fixture().await;
    app.htmx()
        .post(
            "/profile",
            &[("name", "x"), ("age", "99999999999999999999")],
        )
        .await
        .assert_invalid("age");
    app.htmx()
        .post(
            "/profile",
            &[("name", "x"), ("age", "-9223372036854775809")],
        )
        .await
        .assert_invalid("age");
    let mut accepted = Vec::new();
    for special in ["NaN", "inf", "-infinity", "1e999"] {
        let res = app.htmx().post("/score", &[("score", special)]).await;
        if res.status.as_u16() != 422 {
            accepted.push(format!("{special} -> {}", res.text()));
        }
    }
    assert!(accepted.is_empty(), "accepted: {accepted:?}");
}

#[renox::test]
async fn validation_bodies_and_content_types() {
    let app = fixture().await;
    let token = app.csrf_token();
    let send = |ct: &'static str, body: Vec<u8>| {
        let token = token.clone();
        let app = &app;
        async move {
            app.request()
                .without_csrf()
                .header("x-csrf-token", &token)
                .post_body("/api/echo", ct, body)
                .await
        }
    };
    // Empty form body.
    send("application/x-www-form-urlencoded", vec![])
        .await
        .assert_status(303);
    // Invalid JSON / wrong JSON shapes: 400 as JSON.
    for body in [&b"{"[..], b"[1,2]", b"\"x\"", b"null"] {
        let res = send("application/json", body.to_vec()).await;
        res.assert_status(400);
        assert!(
            res.header("content-type")
                .unwrap_or_default()
                .contains("json"),
            "{:?}",
            res.headers
        );
    }
    // Deep nesting doesn't crash.
    let deep = format!(
        "{{\"name\":{}1{}}}",
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    let res = send("application/json", deep.into_bytes()).await;
    assert_ne!(res.status.as_u16(), 500);
    // Bigger than UPLOAD_MAX_SIZE (10 MB): 413.
    let big = format!("{{\"name\":\"{}\"}}", "x".repeat(11 * 1024 * 1024));
    send("application/json", big.into_bytes())
        .await
        .assert_status(413);
    // A text/plain body is parsed as a form (not rejected).
    let res = send("text/plain", b"name=x".to_vec()).await;
    assert!(
        matches!(res.status.as_u16(), 415 | 422),
        "text/plain body accepted as a form: {} {}",
        res.status,
        res.text()
    );
}

#[renox::test]
async fn validation_unique_race() {
    let app = fixture().await;
    let form = |n: &'static str| {
        [
            ("name", n),
            ("email", "race@example.com"),
            ("password", "letmein123"),
            ("password_confirmation", "letmein123"),
        ]
    };
    let (f1, f2) = (form("A"), form("B"));
    let (a, b) = tokio::join!(
        app.htmx().post("/register", &f1),
        app.htmx().post("/register", &f2)
    );
    app.assert_database_count("users", 1).await;
    for r in [&a, &b] {
        assert_ne!(
            r.status.as_u16(),
            500,
            "unique race gave a 500: {}",
            r.text()
        );
    }
}

// ----------------------------------------------------------------- 6. XSS

fn assert_escaped(page: &str) {
    assert!(
        !page.contains("<script>alert(1)"),
        "raw payload in page:\n{page}"
    );
    assert!(
        !page.contains("\"'><"),
        "attribute breakout in page:\n{page}"
    );
}

#[renox::test]
async fn xss_query_in_text_attr_seo_t_and_pagination() {
    let app = fixture().await;
    let q = enc(XSS);
    let evil_key = "%22%3E%3Cscript%3Ealert(1)%3C/script%3E";
    let page = app.get(&format!("/?q={q}&{evil_key}=1")).await.text();
    assert_escaped(&page);
    assert!(page.contains("&lt;script&gt;alert(1)"), "{page}");
}

#[renox::test]
async fn xss_flash_old_error() {
    let app = fixture().await;
    app.post("/flash", &[("msg", XSS)])
        .await
        .assert_redirect("/");
    assert_escaped(&app.get("/").await.text());
    let long_xss = format!("{XSS}{}", "x".repeat(60));
    app.request()
        .header("referer", "/")
        .post("/profile", &[("name", &long_xss)])
        .await
        .assert_status(303);
    let page = app.get("/").await.text();
    assert_escaped(&page);
    assert!(page.contains("may not be longer"), "{page}");
}

#[renox::test]
async fn xss_error_pages_and_debug_detail() {
    let prod = fixture_with(Auth::new(), |c| c.debug = false).await;
    let html = prod.get("/boom").await;
    html.assert_status(500);
    assert!(
        !html.text().contains("hunter2"),
        "debug detail leaked with APP_DEBUG off"
    );
    let json = prod.request().json().get("/boom").await;
    assert!(!json.text().contains("hunter2"), "{}", json.text());
    let nf = prod.request().json().get("/nope").await;
    nf.assert_status(404);
    assert_eq!(nf.json::<serde_json::Value>()["message"], "Not Found");
    prod.get("/_renox/mail").await.assert_status(404);

    let dev = fixture().await;
    let page = dev.get("/boom").await.text();
    assert!(page.contains("hunter2"), "debug shows the detail");
    assert!(
        !page.contains("<script>alert(9)"),
        "debug detail is not escaped:\n{page}"
    );
}

// ------------------------------------------------------------ 7. uploads

#[renox::test]
async fn upload_over_limit_is_413_with_headers() {
    let app = fixture_with(Auth::new(), |c| c.upload_max_size = 64 * 1024).await;
    let big = vec![0u8; 200 * 1024];
    let res = app
        .post_multipart("/upload/any", &[], &[("file", "big.bin", &big)])
        .await;
    res.assert_status(413);
    assert_security_headers(&res.headers, "413 upload");
}

#[renox::test]
async fn upload_zero_byte_file_fails_image_rule() {
    let app = fixture().await;
    let res = app
        .htmx()
        .post_multipart("/upload/image", &[], &[("file", "evil.html", b"")])
        .await;
    let text = res.text();
    assert!(
        res.status.as_u16() == 422 || text == "none",
        "a zero-byte `evil.html` passed image(): {} {text}",
        res.status
    );
}

#[renox::test]
async fn upload_fake_and_double_extensions() {
    let app = fixture().await;
    app.post_multipart(
        "/upload/image",
        &[],
        &[("file", "x.png", b"<html>not a png")],
    )
    .await
    .assert_status(303);
    let res = app
        .post_multipart("/upload/image", &[], &[("file", "x.php.png", PNG)])
        .await;
    res.assert_ok();
    assert!(res.text().ends_with(".png"), "{}", res.text());
    let res = app
        .post_multipart("/upload/image", &[], &[("file", "shell.php", PNG)])
        .await;
    assert!(res.text().ends_with(".png"), "{}", res.text());
    let long = format!("{}.png", "é".repeat(400));
    for name in [
        "../../../../etc/passwd.png",
        "..\\..\\x.png",
        &long,
        "a\u{202e}gnp.exe",
    ] {
        let res = app
            .post_multipart("/upload/image", &[], &[("file", name, PNG)])
            .await;
        res.assert_ok();
        let url = res.text();
        assert!(
            url.starts_with("/storage/img/") && !url.contains(".."),
            "{url}"
        );
    }
}

/// An upload without a type rule can't become a page on the app's origin:
/// active extensions are stored as `.txt`, and `/storage` sandboxes files.
#[renox::test]
async fn upload_html_is_not_served_as_html() {
    let app = fixture().await;
    let res = app
        .post_multipart(
            "/upload/any",
            &[],
            &[(
                "file",
                "cv.html",
                b"<script>alert(document.cookie)</script>",
            )],
        )
        .await;
    res.assert_ok();
    let url = res.text();
    let served = app.get(&url).await;
    served.assert_ok();
    let ct = served.header("content-type").unwrap_or_default().to_owned();
    let csp = served
        .header("content-security-policy")
        .unwrap_or_default()
        .to_owned();
    let disposition = served
        .header("content-disposition")
        .unwrap_or_default()
        .to_owned();
    assert!(
        !ct.starts_with("text/html")
            || csp.contains("sandbox")
            || disposition.starts_with("attachment"),
        "uploaded HTML served from {url} as `{ct}` with CSP `{csp}`"
    );
}

#[renox::test]
async fn upload_svg_is_served_sandboxed() {
    let app = fixture().await;
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"><script>alert(2)</script></svg>"#;
    let res = app
        .post_multipart("/upload/png-or-svg", &[], &[("file", "logo.svg", svg)])
        .await;
    res.assert_ok();
    let url = res.text();
    let served = app.get(&url).await;
    let ct = served.header("content-type").unwrap_or_default().to_owned();
    let csp = served
        .header("content-security-policy")
        .unwrap_or_default()
        .to_owned();
    assert!(
        !ct.contains("svg") || csp.contains("sandbox") || !csp.contains("'unsafe-inline'"),
        "SVG served from {url} as `{ct}` with CSP `{csp}` (inline script allowed)"
    );
}

#[renox::test]
async fn upload_svg_is_not_an_image() {
    let app = fixture().await;
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(2)</script></svg>"#;
    app.htmx()
        .post_multipart("/upload/image", &[], &[("file", "logo.svg", svg)])
        .await
        .assert_invalid("file");
}

#[renox::test]
async fn upload_private_files_signatures() {
    let app = fixture().await;
    let link = app.get("/private-link/a.html").await.text();
    let path = link.trim_start_matches("http://127.0.0.1:3000");
    let ok = app.get(path).await;
    ok.assert_ok();
    let ct = ok.header("content-type").unwrap_or_default();
    assert!(!ct.contains("html"), "private file served as {ct}");
    assert_security_headers(&ok.headers, "private file");

    // Tampered signature / expiry / other key / traversal.
    let last = if path.ends_with('0') { '1' } else { '0' };
    let tampered = format!("{}{last}", &path[..path.len() - 1]);
    app.get(&tampered).await.assert_forbidden();
    let other = app.get("/private-link/b.html").await.text();
    let other_path = other.trim_start_matches("http://127.0.0.1:3000");
    let query = path.split('?').nth(1).unwrap();
    let swapped = format!("{}?{query}", other_path.split('?').next().unwrap());
    app.get(&swapped).await.assert_forbidden();
    let expires_bumped = path.replacen("expires=", "expires=9", 1);
    app.get(&expires_bumped).await.assert_forbidden();
    app.get(&format!("/_renox/files/..%2f..%2fetc%2fpasswd?{query}"))
        .await
        .assert_forbidden();
    app.get("/_renox/files/private/a.html")
        .await
        .assert_forbidden();
    app.get(&format!("{path}&signature=x"))
        .await
        .assert_forbidden();
}

#[renox::test]
async fn upload_private_link_expires() {
    let app = fixture().await;
    let state = app.state().clone();
    state
        .storage
        .put("private/x.txt", "x".into())
        .await
        .unwrap();
    let link = state
        .storage
        .temporary_url(&state, "private/x.txt", Duration::from_secs(1))
        .await
        .unwrap();
    let path = link.trim_start_matches("http://127.0.0.1:3000").to_owned();
    app.get(&path).await.assert_ok();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    app.get(&path).await.assert_forbidden();
}

// -------------------------------------------- 8. method spoofing / routing

#[renox::test]
async fn method_spoofing_edge_cases() {
    let app = fixture().await;
    for m in [
        "GET",
        "HEAD",
        "OPTIONS",
        "TRACE",
        "garbage",
        "DELETE\r\nX: y",
    ] {
        app.post("/only-post", &[("_method", m)])
            .await
            .assert_ok()
            .assert_see("posted");
    }
    // A spoofed PUT on a POST-only route.
    app.post("/only-post", &[("_method", "PUT")])
        .await
        .assert_status(405);
    // _method on a GET is ignored.
    app.get("/items/1?_method=DELETE").await.assert_status(405);
    // Header override also needs CSRF.
    app.request()
        .without_csrf()
        .header("x-http-method-override", "DELETE")
        .post("/items/1", &[])
        .await
        .assert_status(419);
}

#[renox::test]
async fn routing_404s_trailing_slash_long_urls() {
    let app = fixture().await;
    let html = app.get("/nope").await;
    html.assert_not_found();
    assert!(
        html.header("content-type")
            .unwrap()
            .starts_with("text/html")
    );
    let json = app.request().json().get("/nope").await;
    json.assert_not_found();
    assert!(json.header("content-type").unwrap().contains("json"));
    app.get("/dashboard/").await.assert_not_found();
    let long = format!("/{}", "a".repeat(60_000));
    let res = app.get(&long).await;
    assert!(matches!(res.status.as_u16(), 404 | 414), "{}", res.status);
    // Unauthenticated API-ish requests get 401 JSON.
    app.request()
        .json()
        .get("/dashboard")
        .await
        .assert_unauthorized();
}

// ------------------------------------------- 9. security headers / CORS

#[renox::test]
async fn headers_on_every_response_type() {
    let app = fixture().await;
    let page = app.get("/").await;
    let htmx_src = page
        .text()
        .split("src=\"")
        .find(|s| s.starts_with("/_renox/htmx"))
        .map(|s| s.split('"').next().unwrap().to_owned())
        .unwrap();
    let upload = app
        .post_multipart("/upload/image", &[], &[("file", "a.png", PNG)])
        .await
        .text();
    let cases: Vec<(&str, TestResponse)> = vec![
        ("page", page.clone()),
        (
            "redirect",
            app.request().header("referer", "/x").get("/back").await,
        ),
        ("404", app.get("/nope").await),
        ("500", app.get("/boom").await),
        (
            "419",
            app.request().without_csrf().post("/profile", &[]).await,
        ),
        ("422 json", app.htmx().post("/profile", &[]).await),
        ("public file", app.get("/app.css").await),
        ("storage", app.get(&upload).await),
        ("asset", app.get(&htmx_src).await),
        ("health", app.get("/health").await),
        ("robots", app.get("/robots.txt").await),
        ("hx-redirect", app.htmx().get("/dashboard").await),
    ];
    for (what, res) in &cases {
        assert_security_headers(&res.headers, what);
    }
    // Over the urlencoded form limit, the method-spoofing layer answers first.
    let res = app
        .post("/profile", &[("name", &"x".repeat(3 * 1024 * 1024))])
        .await;
    res.assert_status(413);
    assert_security_headers(&res.headers, "413 from method spoofing");
}

#[renox::test]
async fn permissions_policy_is_on_every_response() {
    let app = fixture().await;
    let default = app
        .get("/")
        .await
        .header("permissions-policy")
        .map(str::to_owned);
    let default = default.expect("a page has a Permissions-Policy");
    assert!(
        default.contains("camera=()") && default.contains("geolocation=()"),
        "{default}"
    );
    let cases: Vec<(&str, TestResponse)> = vec![
        ("json", app.request().json().get("/whoami").await),
        (
            "redirect",
            app.request().header("referer", "/x").get("/back").await,
        ),
        ("404", app.get("/nope").await),
        ("500", app.get("/boom").await),
        ("422", app.htmx().post("/profile", &[]).await),
        ("public file", app.get("/app.css").await),
    ];
    for (what, res) in &cases {
        assert_eq!(
            res.header("permissions-policy"),
            Some(default.as_str()),
            "{what}"
        );
    }
    // A handler's own header wins.
    app.get("/own-permissions")
        .await
        .assert_header("permissions-policy", "camera=(self)");
}

#[renox::test]
async fn permissions_policy_follows_the_setting_and_ignores_csp() {
    use renox::PermissionsMode;
    let off = fixture_with(Auth::new(), |c| c.permissions_policy = PermissionsMode::Off).await;
    assert!(off.get("/").await.header("permissions-policy").is_none());
    assert!(
        off.get("/nope")
            .await
            .header("permissions-policy")
            .is_none()
    );

    let custom = fixture_with(Auth::new(), |c| {
        c.permissions_policy = PermissionsMode::Custom("geolocation=(self)".into());
    })
    .await;
    custom
        .get("/")
        .await
        .assert_header("permissions-policy", "geolocation=(self)");

    let no_csp = fixture_with(Auth::new(), |c| c.csp = renox::CspMode::Off).await;
    let res = no_csp.get("/").await;
    assert!(res.header("content-security-policy").is_none());
    assert!(res.header("permissions-policy").is_some());
}

#[renox::test]
async fn cors_preflight_from_disallowed_origin() {
    let app = fixture().await;
    let req = |origin: &str| {
        Request::builder()
            .method("OPTIONS")
            .uri("/api/cors")
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "content-type,x-csrf-token",
            )
            .body(Body::empty())
            .unwrap()
    };
    let (_, headers, _) = raw(&app, req("https://evil.com")).await;
    assert!(
        headers.get("access-control-allow-origin").is_none(),
        "{headers:?}"
    );
    assert!(headers.get("access-control-allow-credentials").is_none());
    let (_, headers, _) = raw(&app, req("https://app.example.com")).await;
    assert_eq!(
        headers.get("access-control-allow-origin").unwrap(),
        "https://app.example.com"
    );
    // A preflight to a route without CORS gets no allow headers either.
    let mut r = req("https://evil.com");
    *r.uri_mut() = "/profile".parse().unwrap();
    let (_, headers, _) = raw(&app, r).await;
    assert!(headers.get("access-control-allow-origin").is_none());
}

// ------------------------------------------------------------- 10. HTMX

#[renox::test]
async fn htmx_behaviour() {
    let app = fixture().await;
    app.htmx()
        .post("/profile", &[("name", "")])
        .await
        .assert_invalid("name");
    let res = app.htmx().get("/dashboard").await;
    res.assert_ok().assert_hx_redirect("/login");
    app.get("/back").await.assert_redirect("/");
    // HX-Redirect with a user-controlled Back isn't produced (Back is a 303).
    let res = app
        .htmx()
        .header("referer", "https://evil.com")
        .get("/back")
        .await;
    assert!(res.header("hx-redirect").is_none());
}

// ------------------------------------------------------ 11. maintenance

#[renox::test]
async fn maintenance_mode() {
    let app = fixture_with(Auth::new(), |c| c.url = "https://shop.example.com".into()).await;
    let storage = app.state().config.storage_path.clone();
    renox::maintenance::down(
        &storage,
        renox::maintenance::DownOptions::new()
            .secret("s3cr3t")
            .retry(60),
    )
    .unwrap();

    let res = app.get("/").await;
    res.assert_status(503);
    assert_eq!(res.header("retry-after"), Some("60"));
    assert!(
        res.header("permissions-policy").is_some(),
        "the 503 page too"
    );
    app.request().json().get("/").await.assert_status(503);
    app.get("/health").await.assert_ok();
    app.request()
        .header("cookie", "renox_maintenance=wrong")
        .get("/")
        .await
        .assert_status(503);
    app.request()
        .header("cookie", "renox_maintenance=")
        .get("/")
        .await
        .assert_status(503);
    app.get("/S3CR3T").await.assert_status(503);
    app.get("/s3cr3t/").await.assert_status(503);

    let res = app.get("/s3cr3t").await;
    res.assert_redirect("/");
    let line = set_cookie_line(&res, "renox_maintenance").unwrap();
    assert!(line.contains("HttpOnly"), "{line}");
    // The cookie the server set gets in; the raw secret as a cookie doesn't.
    let pair = line.split(';').next().unwrap().trim().to_owned();
    app.request()
        .header("cookie", &pair)
        .get("/")
        .await
        .assert_ok();
    app.request()
        .header("cookie", "renox_maintenance=s3cr3t")
        .get("/")
        .await
        .assert_status(503);
    assert!(
        line.contains("Secure"),
        "bypass cookie without Secure on an https app: {line}"
    );
    renox::maintenance::up(&storage).unwrap();
}

#[renox::test]
async fn maintenance_bypass_cookie_is_not_the_raw_secret() {
    let app = fixture().await;
    let storage = app.state().config.storage_path.clone();
    renox::maintenance::down(
        &storage,
        renox::maintenance::DownOptions::new().secret("s3cr3t"),
    )
    .unwrap();
    let res = app.get("/s3cr3t").await;
    let line = set_cookie_line(&res, "renox_maintenance").unwrap();
    assert!(
        !line.contains("=s3cr3t"),
        "the bypass cookie is the secret itself: {line}"
    );
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Leading/trailing spaces in the email field: consistent across forms?
#[renox::test]
async fn auth_login_with_padded_email() {
    let app = fixture().await;
    alex(&app).await;
    let res = app
        .htmx()
        .post(
            "/login",
            &[("email", " alex@example.com "), ("password", "letmein123")],
        )
        .await;
    assert_eq!(res.status.as_u16(), 200, "{} {}", res.status, res.text());
}

/// Token field *before* a file bigger than 2 MB (browser order when
/// csrf_field() comes first) vs after it.
#[renox::test]
async fn csrf_multipart_field_order_with_2mb_file() {
    let app = fixture().await;
    let token = app.csrf_token();
    let big = vec![b'a'; 3 * 1024 * 1024];
    let mut body =
        format!("--b0undary\r\nContent-Disposition: form-data; name=\"_token\"\r\n\r\n{token}\r\n")
            .into_bytes();
    body.extend(multipart(&[], &[("file", "big.txt", &big)], "b0undary"));
    let first = app
        .request()
        .without_csrf()
        .post_body(
            "/upload/any",
            "multipart/form-data; boundary=b0undary",
            body,
        )
        .await;
    let small = vec![b'a'; 1024 * 1024];
    let body = multipart(
        &[("_token", &token)],
        &[("file", "small.txt", &small)],
        "b0undary",
    );
    let after_small = app
        .request()
        .without_csrf()
        .post_body(
            "/upload/any",
            "multipart/form-data; boundary=b0undary",
            body,
        )
        .await;
    assert_eq!(
        (first.status.as_u16(), after_small.status.as_u16()),
        (200, 200),
        "token first + 3MB file: {} / token after 1MB file: {}",
        first.status,
        after_small.status
    );
}

/// `_method` after a >2 MB file part is ignored too.
#[renox::test]
async fn method_field_after_large_file() {
    let app = fixture().await;
    let big = vec![b'a'; 3 * 1024 * 1024];
    let body = multipart(
        &[("_method", "PUT")],
        &[("file", "big.txt", &big)],
        "b0undary",
    );
    let res = app
        .request()
        .post_body("/items/5", "multipart/form-data; boundary=b0undary", body)
        .await;
    assert_eq!(res.status.as_u16(), 200, "{} {}", res.status, res.text());
}

/// One path segment longer than NAME_MAX reaches the public/ fallback.
#[renox::test]
async fn routing_long_path_segment_is_404_not_500() {
    for debug in [true, false] {
        let app = fixture_with(Auth::new(), |c| c.debug = debug).await;
        for len in [255usize, 300, 5000] {
            let res = app.get(&format!("/{}", "a".repeat(len))).await;
            assert_eq!(
                res.status.as_u16(),
                404,
                "debug={debug} len={len}: {} {:.300}",
                res.status,
                res.text()
            );
        }
    }
}

struct Slugs;

impl Module for Slugs {
    fn name(&self) -> &'static str {
        "slugs"
    }
    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { "home" })
            .name("home")
            .get("/{slug}", |Path(s): Path<String>| async move { s })
            .require_auth()
    }
}

/// `intended()` rejects `//x` but not `/\x`, which browsers treat as `//x`.
/// A raw backslash path reaches a `/{slug}` route guarded by require_auth.
#[renox::test]
async fn auth_intended_backslash_path() {
    let app = TestApp::new(App::new().module(Auth::new()).module(Slugs)).await;
    alex(&app).await;
    app.get("/\\evil.com").await.assert_redirect("/login");
    let res = app
        .post(
            "/login",
            &[("email", "alex@example.com"), ("password", "letmein123")],
        )
        .await;
    let loc = res.header("location").unwrap_or_default();
    assert!(!loc.starts_with("/\\"), "after login redirected to `{loc}`");
}

// quiet the unused warning on helpers used only in some configs
#[allow(dead_code)]
fn _unused(_: StatusCode) {}

// ----------------------------------------------------- 12. client IP

async fn get_from(app: &TestApp, peer: &str, forwarded_for: Option<&str>) -> String {
    let mut req = Request::get("/ip");
    if let Some(value) = forwarded_for {
        req = req.header("x-forwarded-for", value);
    }
    let mut req = req.body(Body::empty()).unwrap();
    let addr: SocketAddr = format!("{peer}:5555").parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(addr));
    raw(app, req).await.2
}

#[renox::test]
async fn client_ip_trusts_forwarded_for_only_from_trusted_proxies() {
    let app = fixture().await;
    // No TRUSTED_PROXIES: the header is anyone's to fake.
    assert_eq!(
        get_from(&app, "10.0.0.1", Some("1.2.3.4")).await,
        "10.0.0.1"
    );

    let app = fixture_with(Auth::new(), |c| {
        c.trusted_proxies = TrustedProxies::parse("10.0.0.0/8").unwrap();
    })
    .await;
    assert_eq!(get_from(&app, "10.0.0.1", Some("1.2.3.4")).await, "1.2.3.4");
    // The visitor's own made-up entry comes first; the proxy's is last.
    assert_eq!(
        get_from(&app, "10.0.0.1", Some("6.6.6.6, 1.2.3.4")).await,
        "1.2.3.4"
    );
    assert_eq!(
        get_from(&app, "203.0.113.9", Some("1.2.3.4")).await,
        "203.0.113.9"
    );
    assert_eq!(get_from(&app, "10.0.0.1", None).await, "10.0.0.1");
}

/// Behind a proxy, the login lock is per visitor, not per proxy.
#[renox::test]
async fn login_lock_behind_a_proxy_uses_the_forwarded_ip() {
    let app = fixture_with(Auth::new(), |c| {
        c.trusted_proxies = TrustedProxies::parse("127.0.0.1").unwrap();
    })
    .await;
    alex(&app).await;
    let login = |forwarded: &'static str, password: &'static str| {
        let app = &app;
        async move {
            let probe = app.get("/token").await;
            let body = format!("email=alex%40example.com&password={password}");
            let mut req = Request::post("/login")
                .header("cookie", session_pair(&probe).unwrap())
                .header("x-csrf-token", probe.text())
                .header("x-forwarded-for", forwarded)
                .header("hx-request", "true")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap();
            req.extensions_mut()
                .insert(ConnectInfo("127.0.0.1:5555".parse::<SocketAddr>().unwrap()));
            raw(app, req).await.0
        }
    };
    for _ in 0..5 {
        assert_eq!(login("1.1.1.1", "wrong").await, 422);
    }
    assert_eq!(login("1.1.1.1", "letmein123").await, 422, "locked");
    assert_eq!(login("2.2.2.2", "letmein123").await, 200, "another visitor");
}
