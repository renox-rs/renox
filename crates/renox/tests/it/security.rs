//! Security headers, the Content-Security-Policy, CSRF-free routes (webhooks)
//! and CORS.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use renox::prelude::*;
use renox::testing::TestApp;
use renox::{CspMode, Kernel};
use tower::ServiceExt;

struct Site;

impl Module for Site {
    fn name(&self) -> &'static str {
        "site"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { view("page.html", ()) })
            .name("home")
            .post("/contact", || async { "sent" })
            .get("/framed", || async {
                (
                    [("content-security-policy", "frame-ancestors *")],
                    "embeddable",
                )
            })
            .merge(
                Routes::new()
                    .post("/webhooks/pay", || async { "paid" })
                    .without_csrf(),
            )
            .merge(
                Routes::new()
                    .resource(
                        "/feeds",
                        "feeds",
                        Resource::new()
                            .store(|| async { "stored" })
                            .update(|| async { "updated" }),
                    )
                    .without_csrf(),
            )
            .merge(
                Routes::new()
                    .get("/api/stock", || async { "12" })
                    .post("/api/stock", || async { "saved" })
                    .cors(&["https://app.example.com"]),
            )
    }
}

async fn app(
    configure: impl FnOnce(&mut Config) + Send,
    build: impl FnOnce(App) -> App,
) -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        r#"<head>{{ renox_head() }}</head><script nonce="{{ csp_nonce() }}">1</script>"#,
    )
    .unwrap();
    let views = dir.path().to_path_buf();
    let app = TestApp::with_config(build(App::new().module(Site)), |c| {
        c.views_path = views;
        configure(c);
    })
    .await;
    (app, dir)
}

fn nonce_in(page: &str) -> String {
    page.split(r#"<script nonce=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}

#[renox::test]
async fn every_response_gets_security_headers() {
    let (app, _dir) = app(|_| {}, |a| a).await;
    for uri in ["/", "/health", "/missing"] {
        let res = app.get(uri).await;
        res.assert_header("x-content-type-options", "nosniff")
            .assert_header("referrer-policy", "strict-origin-when-cross-origin")
            .assert_header("x-frame-options", "SAMEORIGIN");
        assert!(res.header("content-security-policy").is_some(), "{uri}");
        assert!(
            res.header("strict-transport-security").is_none(),
            "no HSTS without https"
        );
    }
    // A header the handler set is kept.
    app.get("/framed")
        .await
        .assert_header("content-security-policy", "frame-ancestors *");
}

#[renox::test]
async fn the_relaxed_policy_lets_alpine_and_inline_scripts_run() {
    let (app, _dir) = app(|_| {}, |a| a).await;
    let res = app.get("/").await;
    let csp = res.header("content-security-policy").unwrap().to_owned();
    assert!(
        csp.contains("script-src 'self' 'unsafe-inline' 'unsafe-eval'"),
        "{csp}"
    );
    assert!(csp.contains("frame-ancestors 'self'"), "{csp}");
    assert!(csp.contains("object-src 'none'"), "{csp}");
    assert!(
        !csp.contains("nonce-"),
        "a nonce would switch off 'unsafe-inline': {csp}"
    );
    let page = res.text();
    assert!(page.contains("/_renox/alpine-3."), "{page}");
    assert!(!page.contains("htmx-config"), "{page}");
}

#[renox::test]
async fn the_strict_policy_needs_the_nonce() {
    let (app, _dir) = app(|c| c.csp = CspMode::Strict, |a| a).await;
    let first = app.get("/").await;
    let csp = first.header("content-security-policy").unwrap().to_owned();
    let nonce = nonce_in(&first.text());
    assert!(!nonce.is_empty());
    assert!(
        csp.contains(&format!("script-src 'self' 'nonce-{nonce}'")),
        "{csp}"
    );
    assert!(
        !csp.contains("unsafe-eval") && !csp.contains("'unsafe-inline' 'unsafe-eval'"),
        "{csp}"
    );
    // Alpine's CSP build and htmx without eval.
    let page = first.text();
    let alpine = page
        .split("src=\"")
        .find(|s| s.starts_with("/_renox/alpine-csp-"))
        .unwrap();
    let alpine = alpine.split('"').next().unwrap();
    app.get(alpine).await.assert_ok().assert_see("Alpine");
    assert!(page.contains(r#"content='{"allowEval":false}'"#), "{page}");
    // A new nonce for every response.
    assert_ne!(nonce_in(&app.get("/").await.text()), nonce);
}

#[renox::test]
async fn the_policy_can_be_extended_or_turned_off() {
    let (extended, _dir) = app(
        |_| {},
        |a| {
            a.csp(|csp| {
                csp.allow("script-src", "https://www.googletagmanager.com")
                    .allow("frame-src", "https://www.youtube.com");
            })
        },
    )
    .await;
    let csp = extended
        .get("/")
        .await
        .header("content-security-policy")
        .unwrap()
        .to_owned();
    assert!(
        csp.contains("'unsafe-eval' https://www.googletagmanager.com"),
        "{csp}"
    );
    assert!(
        csp.contains("frame-src 'self' https://www.youtube.com"),
        "{csp}"
    );

    let (off, _dir) = app(|c| c.csp = CspMode::Off, |a| a).await;
    let res = off.get("/").await;
    assert!(res.header("content-security-policy").is_none());
    res.assert_header("x-content-type-options", "nosniff");
}

#[renox::test]
async fn an_app_allows_features_in_the_permissions_policy() {
    let (app, _dir) = app(
        |_| {},
        |a| {
            a.permissions_policy(|p| {
                p.allow("geolocation", "self")
                    .allow("payment", "https://pay.example.com");
            })
        },
    )
    .await;
    let res = app.get("/").await;
    let policy = res.header("permissions-policy").unwrap();
    assert!(policy.contains("geolocation=(self)"), "{policy}");
    assert!(
        policy.contains(r#"payment=("https://pay.example.com")"#),
        "{policy}"
    );
    assert!(policy.contains("camera=()"), "{policy}");
}

#[renox::test]
async fn a_bad_permissions_policy_origin_fails_at_boot() {
    let app = App::new().permissions_policy(|p| {
        p.allow("camera", "two words");
    });
    let err = app.boot().await.err().expect("boot fails");
    assert!(format!("{err:#}").contains("permissions_policy"), "{err:#}");
}

#[renox::test]
async fn production_over_https_gets_hsts() {
    let (app, _dir) = app(
        |c| {
            c.env = Environment::Production;
            c.url = "https://shop.example".into();
        },
        |a| a,
    )
    .await;
    app.get("/")
        .await
        .assert_header("strict-transport-security", "max-age=31536000");
}

#[renox::test]
async fn webhooks_skip_csrf_but_nothing_else_does() {
    let (app, _dir) = app(|_| {}, |a| a).await;
    app.request()
        .without_csrf()
        .post("/webhooks/pay", &[("status", "settlement")])
        .await
        .assert_ok()
        .assert_see("paid");
    app.request()
        .without_csrf()
        .post("/contact", &[])
        .await
        .assert_status(419);
    let webhook = app
        .kernel()
        .routes()
        .iter()
        .find(|r| r.path == "/webhooks/pay")
        .unwrap();
    assert_eq!(webhook.middleware, ["no-csrf"]);
}

/// A resource's `update` route answers PUT and PATCH (listed as `PUT|PATCH`):
/// `.without_csrf()` exempts both methods (#219).
#[renox::test]
async fn a_resource_update_without_csrf_skips_it_for_put_and_patch() {
    let (app, _dir) = app(|_| {}, |a| a).await;
    app.request()
        .without_csrf()
        .post("/feeds", &[])
        .await
        .assert_ok()
        .assert_see("stored");
    app.request()
        .without_csrf()
        .put("/feeds/1", &[])
        .await
        .assert_ok()
        .assert_see("updated");
    app.request()
        .without_csrf()
        .patch("/feeds/1", &[])
        .await
        .assert_ok()
        .assert_see("updated");
}

async fn raw(kernel: &Kernel, req: Request<Body>) -> axum::http::Response<Body> {
    kernel.router().oneshot(req).await.unwrap()
}

#[renox::test]
async fn cors_answers_preflights_for_allowed_origins() {
    let (app, _dir) = app(|_| {}, |a| a).await;
    let preflight = |origin: &str| {
        Request::builder()
            .method("OPTIONS")
            .uri("/api/stock")
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type")
            .body(Body::empty())
            .unwrap()
    };
    let res = raw(app.kernel(), preflight("https://app.example.com")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()["access-control-allow-origin"],
        "https://app.example.com"
    );
    assert!(
        res.headers()["access-control-allow-methods"]
            .to_str()
            .unwrap()
            .contains("POST")
    );

    let res = raw(app.kernel(), preflight("https://evil.example")).await;
    assert!(res.headers().get("access-control-allow-origin").is_none());

    let get = Request::get("/api/stock")
        .header("origin", "https://app.example.com")
        .body(Body::empty())
        .unwrap();
    let res = raw(app.kernel(), get).await;
    assert_eq!(
        res.headers()["access-control-allow-origin"],
        "https://app.example.com"
    );
    // Other routes don't get CORS headers.
    let home = Request::get("/")
        .header("origin", "https://app.example.com")
        .body(Body::empty())
        .unwrap();
    assert!(
        raw(app.kernel(), home)
            .await
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
    let api = app
        .kernel()
        .routes()
        .iter()
        .find(|r| r.path == "/api/stock")
        .unwrap();
    assert_eq!(api.middleware, ["cors"]);
}
