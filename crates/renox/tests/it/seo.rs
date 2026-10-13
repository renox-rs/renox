//! seo(), robots.txt, sitemaps, Search Console / GA4 / GTM head tags, and
//! analytics events reaching the browser.

use renox::analytics::{self, ServerEvent};
use renox::prelude::*;
use renox::seo::Sitemap;
use renox::testing::TestApp;
use serde_json::json;

struct Shop;

impl Module for Shop {
    fn name(&self) -> &'static str {
        "shop"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { view("page.html", ()) })
            .name("home")
            .get("/products/{id}", |Path(id): Path<i64>| async move {
                view("product.html", context! { id })
            })
            .name("products.show")
            .get("/sitemap.xml", sitemap)
            .name("sitemap")
            .get("/welcome", |session: Session| async move {
                analytics::event(&session, "tutorial_begin", json!({}))?;
                Ok::<_, Error>(view("page.html", ()))
            })
            .post("/signup", |session: Session| async move {
                analytics::event(&session, "sign_up", json!({ "method": "email" }))?;
                Ok::<_, Error>(Redirect::to("/"))
            })
            .post("/cart", |session: Session| async move {
                analytics::event(&session, "add_to_cart", json!({ "value": 18000 }))?;
                Ok::<_, Error>((
                    HxTrigger("cart-updated".into()),
                    view("page.html", ()).fragment("body"),
                ))
            })
            .get(
                "/ga",
                |analytics::GaClientId(id): analytics::GaClientId| async move {
                    id.unwrap_or_else(|| "none".into())
                },
            )
            .post("/track", |State(state): State<AppState>| async move {
                state
                    .dispatch(ServerEvent::new(None, "purchase").param("value", 18000))
                    .await?;
                Ok::<_, Error>("queued")
            })
    }
}

async fn sitemap(State(state): State<AppState>) -> Result<Sitemap> {
    let at = "2026-09-01T10:00:00Z".parse::<DateTime>().unwrap();
    Sitemap::new(&state)
        .route("home", &[], None)?
        .route("products.show", &[&7], Some(at))
        .map(|map| map.add("/search?q=coffee&page=2", None))
}

const PAGE: &str = r#"<head>{{ renox_head() }}</head>{% block body %}<p>body</p>{% endblock %}"#;
const PRODUCT: &str = r#"<head>{{ seo(title='Coffee "Latte" · Shop', description='Tasty & cheap', image='/img/coffee.jpg', type='product') }}</head>"#;

async fn app(configure: impl FnOnce(&mut Config) + Send) -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("page.html"), PAGE).unwrap();
    std::fs::write(dir.path().join("product.html"), PRODUCT).unwrap();
    let views = dir.path().to_path_buf();
    let public = dir.path().join("public");
    let app = TestApp::with_config(App::new().module(Shop), |c| {
        c.name = "Shop".into();
        c.url = "https://shop.example".into();
        c.views_path = views;
        c.public_path = public;
        configure(c);
    })
    .await;
    (app, dir)
}

fn production(c: &mut Config) {
    c.env = Environment::Production;
    c.analytics.google_site_verification = Some("abc123".into());
    c.analytics.ga4_measurement_id = Some("G-TEST123".into());
    c.analytics.gtm_container_id = Some("GTM-TEST".into());
}

#[renox::test]
async fn seo_writes_title_description_canonical_and_social_cards() {
    let (app, _dir) = app(|_| {}).await;
    let page = app.get("/products/7?utm_source=x").await.text();
    for expected in [
        "<title>Coffee &quot;Latte&quot; · Shop</title>",
        r#"<meta name="description" content="Tasty &amp; cheap">"#,
        r#"<link rel="canonical" href="https://shop.example/products/7">"#,
        r#"<meta property="og:title" content="Coffee &quot;Latte&quot; · Shop">"#,
        r#"<meta property="og:type" content="product">"#,
        r#"<meta property="og:url" content="https://shop.example/products/7">"#,
        r#"<meta property="og:image" content="https://shop.example/img/coffee.jpg">"#,
        r#"<meta property="og:site_name" content="Shop">"#,
        r#"<meta property="og:locale" content="en">"#,
        r#"<meta name="twitter:card" content="summary_large_image">"#,
    ] {
        assert!(page.contains(expected), "{expected}\n{page}");
    }
}

#[renox::test]
async fn staging_is_kept_out_of_search_engines_and_analytics() {
    let (app, _dir) = app(|c| {
        c.analytics.ga4_measurement_id = Some("G-TEST123".into());
    })
    .await;
    let page = app.get("/").await.text();
    assert!(page.contains(r#"<meta name="robots" content="noindex, nofollow">"#));
    assert!(
        !page.contains("googletagmanager"),
        "no analytics outside production"
    );
    let robots = app.get("/robots.txt").await;
    assert_eq!(robots.text(), "User-agent: *\nDisallow: /\n");
}

#[renox::test]
async fn production_pages_get_verification_ga4_and_gtm_with_the_csp_nonce() {
    let (app, _dir) = app(production).await;
    let res = app.get("/").await;
    let csp = res.header("content-security-policy").unwrap().to_owned();
    let page = res.text();
    assert!(!page.contains("noindex"));
    assert!(page.contains(r#"<meta name="google-site-verification" content="abc123">"#));
    assert!(page.contains(r#"src="https://www.googletagmanager.com/gtag/js?id=G-TEST123""#));
    assert!(page.contains("gtag('config','G-TEST123')"));
    assert!(page.contains("'GTM-TEST'"));
    // Inline tags carry the nonce, and the CSP allows Google's hosts.
    let nonce = page
        .split("nonce=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(!nonce.is_empty());
    assert!(csp.contains("https://www.googletagmanager.com"), "{csp}");
    assert!(
        csp.contains("connect-src 'self' https://*.google-analytics.com"),
        "{csp}"
    );
}

#[renox::test]
async fn robots_txt_and_the_sitemap_in_production() {
    let (app, _dir) = app(production).await;
    assert_eq!(
        app.get("/robots.txt").await.text(),
        "User-agent: *\nAllow: /\n\nSitemap: https://shop.example/sitemap.xml\n"
    );
    let res = app.get("/sitemap.xml").await;
    res.assert_header("content-type", "application/xml; charset=utf-8");
    let xml = res.text();
    assert!(xml.contains("<loc>https://shop.example/</loc>"), "{xml}");
    assert!(
        xml.contains(
            "<loc>https://shop.example/products/7</loc><lastmod>2026-09-01T10:00:00Z</lastmod>"
        ),
        "{xml}"
    );
    assert!(
        xml.contains("<loc>https://shop.example/search?q=coffee&amp;page=2</loc>"),
        "{xml}"
    );
}

#[renox::test]
async fn an_apps_own_robots_txt_wins() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("public")).unwrap();
    std::fs::write(
        dir.path().join("public/robots.txt"),
        "User-agent: *\nDisallow: /admin\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("page.html"), PAGE).unwrap();
    let (views, public) = (dir.path().to_path_buf(), dir.path().join("public"));
    let app = TestApp::with_config(App::new().module(Shop), |c| {
        c.views_path = views;
        c.public_path = public;
    })
    .await;
    assert_eq!(
        app.get("/robots.txt").await.text(),
        "User-agent: *\nDisallow: /admin\n"
    );
}

#[renox::test]
async fn favicon_ico_is_quiet_unless_the_app_has_one() {
    let (app, _dir) = app(|_| {}).await;
    let res = app.get("/favicon.ico").await;
    res.assert_status(204)
        .assert_header("cache-control", "public, max-age=86400");
    assert!(res.body.is_empty());
    assert!(res.header("set-cookie").is_none(), "no session for it");

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("public")).unwrap();
    std::fs::write(dir.path().join("public/favicon.ico"), b"\0\0\x01\0icon").unwrap();
    std::fs::write(dir.path().join("page.html"), PAGE).unwrap();
    let (views, public) = (dir.path().to_path_buf(), dir.path().join("public"));
    let app = TestApp::with_config(App::new().module(Shop), |c| {
        c.views_path = views;
        c.public_path = public;
    })
    .await;
    let res = app.get("/favicon.ico").await;
    res.assert_ok();
    assert_eq!(&res.body[..], b"\0\0\x01\0icon");
}

#[renox::test]
async fn events_reach_the_browser_with_the_page_the_swap_or_the_next_page() {
    let (app, _dir) = app(|_| {}).await;

    // A page: in its head.
    let page = app.get("/welcome").await.text();
    assert!(
        page.contains(
            r#"<meta name="renox-analytics" content="[{&quot;name&quot;:&quot;tutorial_begin&quot;"#
        ),
        "{page}"
    );
    assert!(
        !app.get("/").await.text().contains("renox-analytics"),
        "delivered once"
    );

    // An htmx swap: in its HX-Trigger, next to the handler's own trigger.
    let res = app.htmx().post("/cart", &[]).await;
    let trigger: serde_json::Value =
        serde_json::from_str(res.header("hx-trigger").unwrap()).unwrap();
    assert_eq!(trigger["cart-updated"], serde_json::Value::Null);
    assert_eq!(
        trigger["renox:analytics"]["events"][0]["name"],
        "add_to_cart"
    );
    assert_eq!(
        trigger["renox:analytics"]["events"][0]["params"]["value"],
        18000
    );

    // A redirect: with the next page.
    app.post("/signup", &[]).await.assert_redirect("/");
    let next = app.get("/").await.text();
    assert!(next.contains("sign_up") && next.contains("email"), "{next}");
}

#[renox::test]
async fn server_events_are_queued_and_skipped_without_ga4() {
    let (app, _dir) = app(|_| {}).await;
    app.post("/track", &[]).await.assert_ok();
    assert_eq!(app.queued_jobs().await, ["renox:analytics"]);
    assert_eq!(app.run_jobs().await, 1);
}

/// #252: the `_ga` cookie read on the server.
#[renox::test]
async fn the_ga_client_id_comes_from_the_cookie() {
    let (app, _dir) = app(|_| {}).await;
    app.request()
        .header("cookie", "theme=dark; _ga=GA1.1.1234567890.1700000000")
        .get("/ga")
        .await
        .assert_see("1234567890.1700000000");
    app.request()
        .header("cookie", "_ga=junk")
        .get("/ga")
        .await
        .assert_see("none");
    app.get("/ga").await.assert_see("none");
}

/// #252: with GA4 set, events are only logged outside production, and sent
/// to the Measurement Protocol in production; an error answer fails the job.
#[renox::test]
async fn server_events_go_to_ga4_in_production_only() {
    use renox::http::FakeResponse;
    let ga4 = |c: &mut Config| {
        c.analytics.ga4_measurement_id = Some("G-TEST".into());
        c.analytics.ga4_api_secret = Some("s3cret".into());
    };
    let (local, _dir) = app(ga4).await;
    let http = local.fake_http();
    local.post("/track", &[]).await.assert_ok();
    assert_eq!(local.run_jobs().await, 1);
    assert!(http.sent().is_empty(), "nothing leaves a local app");

    let (live, _dir) = app(move |c| {
        ga4(c);
        c.env = Environment::Production;
        c.debug = false;
    })
    .await;
    let http = live.fake_http();
    http.on(
        "POST https://www.google-analytics.com/mp/collect*",
        FakeResponse::status(204),
    );
    live.post("/track", &[]).await.assert_ok();
    live.run_jobs().await;
    let sent = http.sent();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(
        sent[0].url.contains("measurement_id=G-TEST"),
        "{}",
        sent[0].url
    );
    let body: serde_json::Value = serde_json::from_str(&sent[0].body).unwrap();
    assert_eq!(body["events"][0]["name"], "purchase");

    let (failing, _dir) = app(move |c| {
        ga4(c);
        c.env = Environment::Production;
        c.debug = false;
    })
    .await;
    failing.fake_http().on(
        "POST https://www.google-analytics.com/mp/collect*",
        FakeResponse::status(500),
    );
    failing.post("/track", &[]).await.assert_ok();
    failing.run_jobs().await;
    let failed: i64 = renox::db::sql("SELECT COUNT(*) FROM failed_jobs")
        .scalar(failing.db())
        .await
        .unwrap();
    let waiting: i64 = renox::db::sql("SELECT COUNT(*) FROM jobs")
        .scalar(failing.db())
        .await
        .unwrap();
    assert_eq!(failed + waiting, 1, "the job failed and is kept to retry");
}
