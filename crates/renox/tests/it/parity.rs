//! Gaps the Laravel parity review listed, closed in M33: route model
//! binding (`Found`), `Routes::view`/`redirect`, `Routes::etag`, the
//! `XSRF-TOKEN` cookie, `TRUSTED_HOSTS`, named disks and image dimensions.

use renox::prelude::*;
use renox::storage::StorageConfig;
use renox::testing::TestApp;
use serde::Serialize;

#[derive(Model, Serialize, Default, Clone)]
#[model(table = "post")]
struct Post {
    id: i64,
    slug: String,
    title: String,
}

#[derive(Model, Serialize, Default, Clone)]
#[model(table = "team")]
struct Team {
    id: i64,
    name: String,
}

async fn post(Found(post): Found<Post>) -> String {
    post.title
}

async fn team_post(Found(team): Found<Team>, Found(post): Found<Post>) -> String {
    format!("{} / {}", team.name, post.title)
}

async fn tables(app: &TestApp) {
    for sql in [
        "CREATE TABLE post (id BIGINT PRIMARY KEY, slug TEXT NOT NULL, title TEXT NOT NULL)",
        "CREATE TABLE team (id BIGINT PRIMARY KEY, name TEXT NOT NULL)",
        "INSERT INTO post (id, slug, title) VALUES (1, 'hello', 'Hello'), (2, 'second', 'Second')",
        "INSERT INTO team (id, name) VALUES (7, 'Roasters')",
    ] {
        renox::db::sql(sql).execute(app.db()).await.unwrap();
    }
}

struct Binding;

impl Module for Binding {
    fn name(&self) -> &'static str {
        "binding"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/posts/{post}", post)
            .get("/by-id/{id}", post)
            .get("/blog/{slug}", post)
            .get("/teams/{team}/posts/{post}", team_post)
            .get("/odd/{a}/{b}", post)
    }
}

#[renox::test]
async fn found_loads_the_model_the_route_names() {
    let app = TestApp::new(App::new().module(Binding)).await;
    tables(&app).await;

    // By key: the parameter named after the table, or the only one.
    app.get("/posts/1").await.assert_ok().assert_see("Hello");
    app.get("/by-id/2").await.assert_ok().assert_see("Second");
    // By column: the only parameter is named after a column.
    app.get("/blog/second")
        .await
        .assert_ok()
        .assert_see("Second");
    // Several parameters: each model takes its own.
    app.get("/teams/7/posts/1")
        .await
        .assert_ok()
        .assert_see("Roasters / Hello");

    // Missing rows and values that aren't keys are 404s, like missing routes.
    app.get("/posts/99").await.assert_not_found();
    app.get("/posts/abc").await.assert_not_found();
    app.get("/posts/0").await.assert_not_found();
    app.get("/blog/nope").await.assert_not_found();
    app.get("/teams/8/posts/1").await.assert_not_found();
    // A route whose parameters don't say which is the post: the app's mistake.
    app.get("/odd/1/2").await.assert_status(500);
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .view("/about", "about.html")
            .name("about")
            .redirect("/old-about", "/about")
            .permanent_redirect("/info", "/about")
            .get("/catalogue", || async { "coffee, tea" })
            .view("/menu", "menu.html")
            .etag()
            .get("/fresh", || async { "no etag here" })
    }
}

async fn pages_app() -> (TestApp, tempfile::TempDir) {
    let views = tempfile::tempdir().unwrap();
    std::fs::write(
        views.path().join("about.html"),
        "<h1>About {{ app.name }}</h1><a href=\"{{ route('about') }}\">here</a><p>Prices in {{ app.currency }}</p>",
    )
    .unwrap();
    std::fs::write(
        views.path().join("menu.html"),
        "<h1>Menu</h1><script nonce=\"{{ csp_nonce() }}\">let x = 1;</script>",
    )
    .unwrap();
    let path = views.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pages), |c| {
        c.views_path = path;
        c.name = "Roastery".into();
        c.currency = "EUR".into();
    })
    .await;
    (app, views)
}

#[renox::test]
async fn view_and_redirect_routes_need_no_handler() {
    let (app, _views) = pages_app().await;
    app.get("/about")
        .await
        .assert_ok()
        .assert_view("about.html")
        .assert_see("About Roastery")
        .assert_see("href=\"/about\"")
        // Script that formats money reads the currency from the page (#305).
        .assert_see("Prices in EUR");
    let moved = app.get("/old-about").await;
    moved.assert_status(302).assert_header("location", "/about");
    let gone = app.get("/info").await;
    gone.assert_status(301).assert_header("location", "/about");
    // Any method is redirected, as with Laravel's `Route::redirect`.
    app.post("/old-about", &[]).await.assert_status(302);
}

#[renox::test]
async fn etag_routes_answer_304_when_the_browser_has_the_page() {
    let (app, _views) = pages_app().await;
    let first = app.get("/catalogue").await;
    first.assert_ok().assert_see("coffee, tea");
    let tag = first.header("etag").expect("an ETag").to_owned();
    assert!(tag.starts_with('"') && tag.ends_with('"'), "{tag}");

    let again = app
        .request()
        .header("if-none-match", &tag)
        .get("/catalogue")
        .await;
    again.assert_status(304).assert_header("etag", &tag);
    assert!(again.body.is_empty());

    // Weak and listed tags match too; another version gets the page.
    let weak = app
        .request()
        .header("if-none-match", &format!("\"other\", W/{tag}"))
        .get("/catalogue")
        .await;
    weak.assert_status(304);
    app.request()
        .header("if-none-match", "\"old\"")
        .get("/catalogue")
        .await
        .assert_ok()
        .assert_see("coffee, tea");

    // Only routes added before `.etag()` get one.
    assert_eq!(app.get("/fresh").await.header("etag"), None);
}

/// #306: a page with the request's CSP nonce in it (every layout with
/// scripts) gets the same ETag each time, so it can be answered 304; the 304
/// carries no CSP header, so the browser keeps the one it stored with the
/// page, whose nonce matches the page's.
#[renox::test]
async fn pages_with_a_nonce_still_answer_304() {
    let (app, _views) = pages_app().await;
    let first = app.get("/menu").await;
    first.assert_ok().assert_see("<script nonce=");
    assert!(first.header("content-security-policy").is_some());
    let tag = first.header("etag").expect("an ETag").to_owned();
    let second = app.get("/menu").await;
    assert_ne!(first.text(), second.text(), "a new nonce each time");
    assert_eq!(second.header("etag"), Some(tag.as_str()), "the same ETag");

    let cached = app
        .request()
        .header("if-none-match", &tag)
        .get("/menu")
        .await;
    cached.assert_status(304);
    assert_eq!(cached.header("content-security-policy"), None);
}

struct Api;

impl Module for Api {
    fn name(&self) -> &'static str {
        "api"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { "home" })
            .post("/notes", || async { "saved" })
    }
}

fn xsrf_cookie(res: &renox::testing::TestResponse) -> Option<String> {
    res.headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| v.strip_prefix("XSRF-TOKEN="))
        .map(|v| v.split(';').next().unwrap_or_default().to_owned())
}

#[renox::test]
async fn the_xsrf_cookie_carries_the_csrf_token_for_scripts() {
    let app = TestApp::new(App::new().module(Api).xsrf_cookie()).await;
    let home = app.get("/").await;
    let token = xsrf_cookie(&home).expect("an XSRF-TOKEN cookie");
    assert_eq!(token, app.csrf_token());
    let set = home
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("XSRF-TOKEN="))
        .unwrap()
        .to_owned();
    assert!(!set.contains("HttpOnly"), "scripts must read it: {set}");
    assert!(set.contains("SameSite=Lax"), "{set}");

    // Sent back in `X-XSRF-TOKEN`, it passes the CSRF check.
    app.request()
        .without_csrf()
        .header("x-xsrf-token", &token)
        .post("/notes", &[])
        .await
        .assert_ok()
        .assert_see("saved");
    app.request()
        .without_csrf()
        .header("x-xsrf-token", "forged")
        .post("/notes", &[])
        .await
        .assert_status(419);

    // Off unless asked for.
    let plain = TestApp::new(App::new().module(Api)).await;
    assert_eq!(xsrf_cookie(&plain.get("/").await), None);
}

#[renox::test]
async fn trusted_hosts_refuse_other_hosts() {
    let app = TestApp::with_config(App::new().module(Api), |c| {
        c.trusted_hosts = vec!["shop.example.com".into(), "*.example.org".into()];
        c.url = "https://www.example.net".into();
    })
    .await;
    let get = |host: &'static str| {
        let app = &app;
        async move { app.request().header("host", host).get("/").await }
    };
    get("shop.example.com").await.assert_ok();
    get("shop.example.com:8080").await.assert_ok();
    get("eu.example.org").await.assert_ok();
    get("www.example.net").await.assert_ok(); // APP_URL's host
    get("example.org").await.assert_status(400); // `*.` means subdomains
    get("evil.com").await.assert_status(400);
    get("shop.example.com.evil.com").await.assert_status(400);
    // Load balancers check health by IP.
    app.request()
        .header("host", "10.0.0.5")
        .get("/health")
        .await
        .assert_ok();
}

#[renox::test]
async fn named_disks_keep_their_own_files() {
    let app = TestApp::new(
        App::new()
            .disk("exports", |_| Ok(StorageConfig::default()))
            .disk("backups", |config| {
                StorageConfig::from_env(config, "BACKUPS")
            }),
    )
    .await;
    let state = app.state();
    let exports = state.disk_named("exports").unwrap();
    assert_eq!(exports.name(), Some("exports"));
    exports
        .put("reports/october.csv", "a,b\n1,2\n".into())
        .await
        .unwrap();
    exports.put("public/logo.txt", "logo".into()).await.unwrap();
    // Each disk is its own folder: the default disk doesn't see it.
    assert!(!state.storage.exists("reports/october.csv").await.unwrap());
    assert!(
        state
            .config
            .storage_path
            .join("exports/reports/october.csv")
            .exists()
    );
    assert!(state.disk_named("backups").is_ok());
    assert!(state.disk_named("nope").is_err());

    // Public files are served at the disk's URL; the rest need a signed link.
    let url = exports.url("public/logo.txt");
    assert_eq!(url, "/_renox/disks/exports/public/logo.txt");
    app.get(&url).await.assert_ok().assert_see("logo");
    app.get("/_renox/disks/exports/reports/october.csv")
        .await
        .assert_forbidden();
    let link = exports
        .temporary_url(
            state,
            "reports/october.csv",
            std::time::Duration::from_secs(60),
        )
        .await
        .unwrap();
    let path = link.trim_start_matches(&state.config.url);
    assert!(path.starts_with("/_renox/disks/exports/"), "{path}");
    let csv = app.get(path).await;
    csv.assert_ok().assert_see("a,b");
    // Served like the default disk's private files: never as a page.
    assert!(
        csv.header("content-security-policy")
            .unwrap()
            .contains("sandbox")
    );
    app.get("/_renox/disks/nope/public/logo.txt")
        .await
        .assert_not_found();
}

#[renox::test]
async fn disk_names_are_checked_at_boot() {
    let config = || {
        let mut config = Config::default();
        config.database_url = "sqlite::memory:".into();
        config
    };
    let bad = App::with_config(config()).disk("bad name", |_| Ok(StorageConfig::default()));
    let twice = App::with_config(config())
        .disk("twice", |_| Ok(StorageConfig::default()))
        .disk("twice", |_| Ok(StorageConfig::default()));
    for (app, expected) in [(bad, "disk name"), (twice, "two disks")] {
        let err = match app.boot().await {
            Ok(_) => panic!("{expected}: booted"),
            Err(err) => format!("{err:?}"),
        };
        assert!(err.contains(expected), "{err}");
    }
}

#[test]
fn from_env_reads_a_prefix_and_falls_back_to_s3() {
    let mut config = Config::default();
    config.vars.insert("ARCHIVE_DISK".into(), "s3".into());
    config
        .vars
        .insert("ARCHIVE_BUCKET".into(), "old-orders".into());
    config.storage.region = Some("ap-southeast-1".into());
    let archive = StorageConfig::from_env(&config, "archive").unwrap();
    assert_eq!(archive.disk, renox::storage::DiskDriver::S3);
    assert_eq!(archive.bucket.as_deref(), Some("old-orders"));
    assert_eq!(archive.region.as_deref(), Some("ap-southeast-1"));
    assert_eq!(archive.url, None);
    let local = StorageConfig::from_env(&config, "SCRATCH").unwrap();
    assert_eq!(local.disk, renox::storage::DiskDriver::Local);
    assert_eq!(local.root, None);
}
