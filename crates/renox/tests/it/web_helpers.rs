//! #252: the pieces between a request and its handler that no test had
//! reached: cookie attributes, extractors, routing helpers, host and CSRF
//! checks, rate limits by user and by name, and session details.

use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderValue, Request};

use renox::prelude::*;
use renox::rate_limit::Limit;
use renox::security::CspNonce;
use renox::select::OptionQuery;
use renox::testing::TestApp;
use renox::{CacheStore, Cookies, CspMode, SetCookie};
use tower::ServiceExt;

struct Web;

impl Module for Web {
    fn name(&self) -> &'static str {
        "web"
    }

    fn routes(&self) -> Routes {
        let one_route = renox::axum::Router::new().route(
            "/from-router",
            renox::axum::routing::get(|| async { "from a router" }),
        );
        Routes::new()
            .get("/cookie", |State(state): State<AppState>| async move {
                (
                    SetCookie::new(&state, "theme", "dark")
                        .path("/admin")
                        .readable_by_scripts()
                        .strict(),
                    "set",
                )
            })
            .get("/read-cookies", |cookies: Cookies| async move {
                format!(
                    "{}|{}",
                    cookies.get("a").unwrap_or_default(),
                    cookies.get("b").unwrap_or_default()
                )
            })
            .get("/shout/{word}", |mut word: Path<String>| async move {
                word.push('!');
                word.0
            })
            .get("/two/{a}/{b}", |word: Path<String>| async move { word.0 })
            .get("/length/{word}", |word: Path<String>| async move {
                word.len().to_string()
            })
            .get("/no-params", |word: Path<String>| async move { word.0 })
            .get("/member/{id}", |mut user: Found<User>| async move {
                user.name.push_str(" (seen)");
                user.name.clone()
            })
            .get("/page", |Page(page): Page| async move { page.to_string() })
            .route(
                "/any-method",
                renox::axum::routing::get(|| async { "got" }).put(|| async { "put" }),
            )
            .merge(Routes::from(one_route))
            .merge(Routes::new().get("/open", || async { "open" }).cors(&["*"]))
            .redirect("/broken-redirect", "/a\nb")
            .get("/nonce", |nonce: CspNonce| async move {
                format!("[{}]", nonce.0)
            })
            .get("/options", |query: OptionQuery| async move {
                format!(
                    "{}|{}|{}",
                    query.q,
                    query.values.join(","),
                    query.is_lookup()
                )
            })
            .post("/form", || async { "posted" })
            .merge(
                Routes::new()
                    .get("/mine", || async { "mine" })
                    .throttle(1, Duration::from_secs(60))
                    .require_auth(),
            )
            .merge(
                Routes::new()
                    .get("/hourly", || async { "hourly" })
                    .throttle_by("hourly"),
            )
            .get("/push", |session: Session| async move {
                session.put("seen", "home")?;
                let count = session.push("seen", "cart")?;
                let errors = session.errors_in("no-such-bag");
                Ok::<_, Error>(format!("{count} {}", errors.len()))
            })
    }
}

fn web() -> App {
    App::new()
        .module(Auth::new())
        .module(Web)
        .rate_limiter("hourly", |_| Limit::per_hour(1))
}

#[renox::test]
async fn cookie_attributes_follow_the_builder_and_the_apps_url() {
    let app = TestApp::with_config(web(), |c| c.url = "https://shop.test".into()).await;
    let res = app.get("/cookie").await;
    let cookie = res.header("set-cookie").unwrap().to_owned();
    assert!(cookie.starts_with("theme=dark"), "{cookie}");
    assert!(cookie.contains("Path=/admin"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(cookie.contains("Secure"), "{cookie}");
    assert!(!cookie.contains("HttpOnly"), "{cookie}");
}

#[renox::test]
async fn cookies_come_from_every_cookie_header_and_skip_unreadable_ones() {
    let app = TestApp::new(web()).await;
    let mut req = Request::get("/read-cookies").body(Body::empty()).unwrap();
    let headers = req.headers_mut();
    headers.append("cookie", HeaderValue::from_static("a=1"));
    headers.append("cookie", HeaderValue::from_bytes(b"x=\xff").unwrap());
    headers.append("cookie", HeaderValue::from_static("b=2"));
    let res = app.kernel().router().oneshot(req).await.unwrap();
    let body = renox::axum::body::to_bytes(res.into_body(), 1024)
        .await
        .unwrap();
    assert_eq!(&body[..], b"1|2");
}

#[renox::test]
async fn path_and_found_can_be_changed_in_the_handler() {
    let app = TestApp::new(web()).await;
    app.get("/shout/hey").await.assert_see("hey!");
    let ann = User::register(app.db(), "Ann", "ann@example.com", "password123")
        .await
        .unwrap();
    app.get(&format!("/member/{}", ann.id))
        .await
        .assert_see("Ann (seen)");
}

#[renox::test]
async fn a_path_extractor_that_doesnt_fit_the_route_is_a_500() {
    let app = TestApp::new(web()).await;
    app.get("/two/a/b").await.assert_status(500);
}

#[renox::test]
async fn page_zero_is_the_first_page() {
    let app = TestApp::new(web()).await;
    app.get("/page?page=0").await.assert_see("1");
    app.get("/page?page=3").await.assert_see("3");
}

#[renox::test]
async fn routes_from_method_routers_and_plain_routers() {
    let app = TestApp::new(web()).await;
    app.get("/any-method").await.assert_see("got");
    app.put("/any-method", &[]).await.assert_see("put");
    app.get("/from-router").await.assert_see("from a router");
}

#[renox::test]
async fn cors_for_any_origin() {
    let app = TestApp::new(web()).await;
    let res = app
        .request()
        .header("origin", "https://elsewhere.test")
        .get("/open")
        .await;
    res.assert_ok()
        .assert_header("access-control-allow-origin", "*");
}

#[renox::test]
async fn a_redirect_to_an_invalid_url_is_a_500_not_a_panic() {
    let app = TestApp::new(web()).await;
    app.get("/broken-redirect").await.assert_status(500);
}

#[test]
fn redirect_route_needs_the_app() {
    let err = Redirect::route("home", &[]).unwrap_err();
    assert!(
        format!("{err:?}").contains("needs the app: call it in a handler, job or command"),
        "{err:?}"
    );
}

#[renox::test]
async fn csp_off_sends_no_policy_but_pages_still_get_a_nonce() {
    let app = TestApp::with_config(web(), |c| c.csp = CspMode::Off).await;
    let res = app.get("/nonce").await;
    assert!(res.header("content-security-policy").is_none());
    assert!(res.text().len() > 2, "{}", res.text());
    let strict = TestApp::with_config(web(), |c| c.csp = CspMode::Strict).await;
    let res = strict.get("/nonce").await;
    let nonce = res.text().trim_matches(['[', ']']).to_owned();
    let policy = res.header("content-security-policy").unwrap();
    assert!(policy.contains(&format!("'nonce-{nonce}'")), "{policy}");
}

#[renox::test]
async fn option_queries_read_the_search_and_the_lookups() {
    let app = TestApp::new(web()).await;
    app.get("/options?q=+cof+&values=3&values=5")
        .await
        .assert_see("cof|3,5|true");
    app.get("/options").await.assert_see("||false");
}

#[renox::test]
async fn trusted_hosts_refuse_a_request_with_no_host() {
    let app = TestApp::with_config(web(), |c| c.trusted_hosts = vec!["shop.test".into()]).await;
    let req = Request::get("/page").body(Body::empty()).unwrap();
    let res = app.kernel().router().oneshot(req).await.unwrap();
    assert_eq!(res.status(), 400);
    let mut req = Request::get("/page").body(Body::empty()).unwrap();
    req.headers_mut()
        .insert("host", HeaderValue::from_static("shop.test"));
    let res = app.kernel().router().oneshot(req).await.unwrap();
    assert_eq!(res.status(), 200);
}

#[renox::test]
async fn a_form_over_the_form_limit_is_refused() {
    // Over 2 MB of form fields: refused before any handler (or the CSRF
    // check) reads it.
    let app = TestApp::with_config(web(), |c| c.upload_max_size = 8 * 1024 * 1024).await;
    let big = "x".repeat(3 * 1024 * 1024);
    app.post("/form", &[("note", big.as_str())])
        .await
        .assert_status(413);
}

#[renox::test]
async fn throttles_count_each_logged_in_user_apart() {
    let app = TestApp::new(web()).await;
    let ann = User::register(app.db(), "Ann", "ann@example.com", "password123")
        .await
        .unwrap();
    let bob = User::register(app.db(), "Bob", "bob@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&ann);
    app.get("/mine").await.assert_ok();
    app.get("/mine").await.assert_status(429);
    app.acting_as(&bob);
    app.get("/mine").await.assert_ok();
}

#[renox::test]
async fn hourly_named_limits_reset_after_their_window() {
    let app = TestApp::new(web()).await;
    app.get("/hourly").await.assert_ok();
    app.get("/hourly").await.assert_status(429);
    app.travel(Duration::from_secs(3601));
    app.get("/hourly").await.assert_ok();
}

#[renox::test]
async fn named_limits_are_shared_through_the_database_store() {
    let app = TestApp::with_config(web(), |c| c.cache_store = CacheStore::Database).await;
    app.get("/hourly").await.assert_ok();
    app.get("/hourly").await.assert_status(429);
    let rows: i64 = renox::db::sql("SELECT COUNT(*) FROM cache WHERE key LIKE ?")
        .bind("%named:hourly%")
        .scalar(app.db())
        .await
        .unwrap();
    assert!(rows >= 1, "the count lives in the cache table");
}

#[renox::test]
async fn a_failing_shared_counter_lets_the_request_through() {
    let (logs, _logged) = crate::logs::capture();
    let app = TestApp::with_config(web(), |c| c.cache_store = CacheStore::Database).await;
    renox::db::sql("DROP TABLE cache")
        .execute(app.db())
        .await
        .unwrap();
    app.get("/hourly").await.assert_ok();
    app.get("/hourly").await.assert_ok();
    assert!(
        logs.has(&["could not count a rate-limited request"]),
        "{}",
        logs.text()
    );
}

#[renox::test]
async fn throttle_by_needs_a_limiter_of_that_name() {
    let err = App::new()
        .module(Web)
        .boot()
        .await
        .err()
        .expect("boot refuses an unknown limiter");
    assert!(
        format!("{err:?}").contains(
            "uses throttle_by(\"hourly\"), but there's no App::rate_limiter(\"hourly\", …)"
        ),
        "{err:?}"
    );
}

#[renox::test]
async fn push_turns_a_single_value_into_a_list() {
    let app = TestApp::new(web()).await;
    app.get("/push").await.assert_see("2 0");
    assert_eq!(
        app.session_get::<Vec<String>>("seen").unwrap(),
        ["home", "cart"]
    );
}

/// A database session that can't be saved: the page still answers (the
/// error is logged), and the next request is a fresh session.
#[renox::test]
async fn a_session_that_cant_be_saved_doesnt_break_the_page() {
    let (logs, _logged) = crate::logs::capture();
    let app = TestApp::with_config(web(), |c| {
        c.session_driver = renox::SessionDriver::Database;
        // Outside `testing`, sessions go to the table, not the test mirror.
        c.env = Environment::Local;
    })
    .await;
    app.get("/push").await.assert_see("2 0");
    renox::db::sql("DROP TABLE sessions")
        .execute(app.db())
        .await
        .unwrap();
    app.get("/push").await.assert_ok();
    assert!(logs.has(&["could not save the session"]), "{}", logs.text());
}

#[renox::test]
async fn path_reads_through_to_its_value_and_a_route_without_parameters_is_a_500() {
    let app = TestApp::new(web()).await;
    app.get("/length/coffee").await.assert_see("6");
    // While debugging, the page names what the route lacks.
    app.get("/no-params")
        .await
        .assert_status(500)
        .assert_see("parameters");
}

/// A form sent to a URL no route matches still needs its CSRF token (419),
/// and its token is looked for in the body, which has a size limit.
#[renox::test]
async fn csrf_applies_to_unmatched_urls_and_limits_the_form_it_reads() {
    let app = TestApp::new(web()).await;
    app.request()
        .without_csrf()
        .post("/nowhere", &[("a", "b")])
        .await
        .assert_status(419);
    // The method override in a header leaves the body to the CSRF check,
    // which stops reading at 2 MB.
    let big = "x".repeat(3 * 1024 * 1024);
    app.request()
        .without_csrf()
        .header("x-http-method-override", "PUT")
        .post("/any-method", &[("note", big.as_str())])
        .await
        .assert_status(400)
        .assert_see("The form is too large.");
}
