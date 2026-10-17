//! `App::pwa`: the manifest, worker, offline page and icon routes, merged
//! before sessions and listed by `route:list`.

use renox::prelude::*;
use renox::pwa::Pwa;
use renox::testing::TestApp;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nxx";

fn icons(root: &std::path::Path) -> std::path::PathBuf {
    let dir = root.join("icons");
    std::fs::create_dir_all(&dir).unwrap();
    for file in ["icon-192.png", "icon-512.png", "icon-maskable-512.png"] {
        std::fs::write(dir.join(file), PNG).unwrap();
    }
    dir
}

async fn app(dir: &tempfile::TempDir) -> TestApp {
    let public = dir.path().join("public");
    let pwa = Pwa::new("Bikeshop").icons(icons(dir.path()));
    TestApp::with_config(App::new().pwa(pwa), |c| c.public_path = public).await
}

#[renox::test]
async fn pwa_routes_are_served_without_a_session_cookie_and_listed() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(&dir).await;
    let manifest = app.get("/manifest.webmanifest").await;
    manifest.assert_ok();
    assert_eq!(
        manifest.header("content-type"),
        Some("application/manifest+json")
    );
    assert!(manifest.header("set-cookie").is_none());
    let json: serde_json::Value = manifest.json();
    assert_eq!(json["name"], "Bikeshop");
    assert_eq!(json["dir"], "ltr");

    let worker = app.get("/sw.js").await;
    worker.assert_ok();
    assert_eq!(worker.header("service-worker-allowed"), Some("/"));
    assert_eq!(worker.header("cache-control"), Some("no-cache"));
    assert!(worker.header("set-cookie").is_none());
    app.get("/_renox/offline").await.assert_ok();
    app.get("/_renox/pwa/icons/icon-192.png").await.assert_ok();

    let listing = app.kernel().routes();
    for path in ["/manifest.webmanifest", "/sw.js", "/_renox/offline"] {
        assert!(listing.iter().any(|r| r.path == path), "{path} is listed");
    }
}

#[renox::test]
async fn public_files_replace_the_generated_manifest_and_worker() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("public")).unwrap();
    std::fs::write(dir.path().join("public/sw.js"), "// mine").unwrap();
    std::fs::write(dir.path().join("public/manifest.webmanifest"), "{}").unwrap();
    let app = app(&dir).await;
    assert_eq!(app.get("/sw.js").await.text(), "// mine");
    assert_eq!(app.get("/manifest.webmanifest").await.text(), "{}");
    app.get("/_renox/offline").await.assert_ok();
}

#[renox::test]
async fn a_missing_icon_is_a_boot_error_naming_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("none");
    std::fs::create_dir_all(&empty).unwrap();
    let err = App::new()
        .pwa(Pwa::new("Bikeshop").icons(empty))
        .into_router()
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("icon-192.png"), "{err:#}");
}

fn branded(dir: &tempfile::TempDir) -> App {
    let brand_icons = dir.path().join("north-icons");
    std::fs::create_dir_all(&brand_icons).unwrap();
    for file in ["icon-192.png", "icon-512.png", "icon-maskable-512.png"] {
        std::fs::write(brand_icons.join(file), b"\x89PNG\r\n\x1a\nnorth").unwrap();
    }
    App::new()
        .pwa(Pwa::new("Bikeshop").icons(icons(dir.path())))
        .pwa_for(move |req: renox::pwa::PwaRequest| {
            let brand_icons = brand_icons.clone();
            async move {
                match req.host() {
                    "north.test" => Ok(Some(
                        renox::pwa::PwaBrand::new("North Bikes")
                            .theme_color("#ff0000")
                            .icons(brand_icons),
                    )),
                    "broken.test" => Ok(Some(
                        renox::pwa::PwaBrand::new("Broken").icons("/nonexistent/icons"),
                    )),
                    "error.test" => Err(anyhow::anyhow!("boom").into()),
                    _ => Ok(None),
                }
            }
        })
}

#[renox::test]
async fn pwa_for_serves_a_manifest_icons_and_worker_per_host() {
    let dir = tempfile::tempdir().unwrap();
    let public = dir.path().join("public");
    let app = TestApp::with_config(branded(&dir), |c| c.public_path = public).await;
    let get = |host: &'static str, uri: &'static str| app.request().header("host", host).get(uri);

    let plain = get("other.test", "/manifest.webmanifest").await;
    assert_eq!(plain.json::<serde_json::Value>()["name"], "Bikeshop");
    assert_eq!(plain.header("vary"), Some("Host"));
    let north = get("north.test", "/manifest.webmanifest").await;
    assert_eq!(north.header("vary"), Some("Host"));
    let json: serde_json::Value = north.json();
    assert_eq!(json["name"], "North Bikes");
    assert_eq!(json["theme_color"], "#ff0000");
    let src = json["icons"][0]["src"].as_str().unwrap().to_owned();
    assert!(
        src.starts_with("/_renox/pwa/icons/") && src.matches('/').count() == 5,
        "{src}"
    );
    let icon = app.get(&src).await;
    icon.assert_ok();
    assert_eq!(icon.header("content-type"), Some("image/png"));

    let version = |text: String| {
        text.lines()
            .find(|l| l.contains("const VERSION"))
            .unwrap()
            .to_owned()
    };
    let a = version(get("other.test", "/sw.js").await.text());
    let b = version(get("north.test", "/sw.js").await.text());
    assert_ne!(a, b);
    assert_eq!(b, version(get("north.test", "/sw.js").await.text()));
    let worker = get("north.test", "/sw.js").await;
    assert_eq!(worker.header("vary"), Some("Host"));
    get("north.test", "/_renox/offline").await.assert_ok();
}

#[renox::test]
async fn pwa_for_falls_back_on_errors_and_invalid_icons() {
    let dir = tempfile::tempdir().unwrap();
    let public = dir.path().join("public");
    let app = TestApp::with_config(branded(&dir), |c| c.public_path = public).await;
    let get = |host: &'static str| {
        app.request()
            .header("host", host)
            .get("/manifest.webmanifest")
    };
    let failed = get("error.test").await;
    assert_eq!(failed.json::<serde_json::Value>()["name"], "Bikeshop");
    let broken: serde_json::Value = get("broken.test").await.json();
    assert_eq!(broken["name"], "Broken");
    assert_eq!(broken["icons"][0]["src"], "/_renox/pwa/icons/icon-192.png");
}

#[renox::test]
async fn pwa_for_without_pwa_is_a_boot_error() {
    let err = App::new()
        .pwa_for(|_req: renox::pwa::PwaRequest| async move { Ok(None) })
        .into_router()
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("App::pwa"), "{err:#}");
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/", || async { view("page.html", context! {}) })
    }
}

async fn page(pwa: Option<Pwa>, strict: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        "<head><rx-pwa/></head>|{{ renox_pwa() }}",
    )
    .unwrap();
    let (views, public) = (dir.path().to_path_buf(), dir.path().join("public"));
    let app = match pwa {
        Some(pwa) => App::new().module(Pages).pwa(pwa),
        None => App::new().module(Pages),
    };
    let app = TestApp::with_config(app, move |c| {
        c.views_path = views;
        c.public_path = public;
        if strict {
            c.csp = renox::CspMode::Strict;
        }
    })
    .await;
    let res = app.get("/").await;
    res.assert_ok();
    res.text()
}

#[renox::test]
async fn rx_pwa_renders_the_head_tags_and_a_script_that_carries_the_nonce() {
    let dir = tempfile::tempdir().unwrap();
    let pwa = Pwa::new("Bikeshop")
        .theme_color("#123456")
        .icons(icons(dir.path()));
    let html = page(Some(pwa), true).await;
    assert_eq!(html.matches("rel=\"manifest\"").count(), 2, "{html}");
    assert_eq!(
        html.matches("name=\"theme-color\" content=\"#123456\"")
            .count(),
        2
    );
    assert_eq!(html.matches("rel=\"apple-touch-icon\"").count(), 2);
    let nonce = html
        .split("<script nonce=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_default();
    assert!(!nonce.is_empty(), "the script carries the nonce: {html}");
    assert_eq!(
        html.matches(&format!("<script nonce=\"{nonce}\">")).count(),
        2
    );
    assert!(html.contains("serviceWorker.register('/sw.js'"), "{html}");
}

#[renox::test]
async fn rx_pwa_renders_nothing_without_a_pwa() {
    let html = page(None, true).await;
    assert_eq!(html, "<head></head>|", "{html}");
}

fn version_of(worker: &str) -> String {
    worker
        .lines()
        .find(|l| l.contains("const VERSION"))
        .expect("the worker declares its version")
        .to_owned()
}

/// A booted app whose offline view is `offline.html` with `html` as content.
async fn app_offline(html: &str) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("views")).unwrap();
    std::fs::write(dir.path().join("views/offline.html"), html).unwrap();
    let (views, public) = (dir.path().join("views"), dir.path().join("public"));
    let pwa = Pwa::new("Bikeshop")
        .icons(icons(dir.path()))
        .offline_page("offline.html");
    TestApp::with_config(App::new().pwa(pwa), move |c| {
        c.views_path = views;
        c.public_path = public;
    })
    .await
}

#[renox::test]
async fn the_manifest_is_valid_json_with_every_member_and_the_three_icons() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(&dir).await;
    let res = app.get("/manifest.webmanifest").await;
    assert_eq!(
        res.header("content-type"),
        Some("application/manifest+json")
    );
    let json: serde_json::Value = serde_json::from_str(&res.text()).expect("valid JSON");
    for key in [
        "name",
        "short_name",
        "start_url",
        "display",
        "theme_color",
        "background_color",
        "lang",
        "dir",
    ] {
        assert!(
            json[key].as_str().is_some_and(|v| !v.is_empty()),
            "{key}: {json}"
        );
    }
    assert_eq!(json["display"], "standalone");
    let icons = json["icons"].as_array().unwrap();
    let find = |sizes: &str, purpose: &str| {
        icons
            .iter()
            .any(|i| i["sizes"] == sizes && i["purpose"] == purpose)
    };
    assert!(find("192x192", "any"), "{json}");
    assert!(find("512x512", "any"), "{json}");
    assert!(find("512x512", "maskable"), "{json}");
    for icon in icons {
        app.get(icon["src"].as_str().unwrap()).await.assert_ok();
    }
}

#[renox::test]
async fn the_manifest_name_follows_the_request_locale_when_lang_files_define_it() {
    let dir = tempfile::tempdir().unwrap();
    let lang = dir.path().join("lang");
    std::fs::create_dir_all(&lang).unwrap();
    std::fs::write(
        lang.join("es.json"),
        r#"{"pwa": {"name": "Tienda de bicis", "short_name": "Bicis"}}"#,
    )
    .unwrap();
    std::fs::write(lang.join("ar.json"), r#"{"pwa": {"name": "Bicis arabes"}}"#).unwrap();
    let public = dir.path().join("public");
    let pwa = Pwa::new("Bikeshop").icons(icons(dir.path()));
    let app = TestApp::with_config(App::new().detect_locale().pwa(pwa), |c| {
        c.lang_path = lang;
        c.public_path = public;
    })
    .await;
    let get = |language: &'static str| {
        app.request()
            .header("accept-language", language)
            .get("/manifest.webmanifest")
    };
    let es: serde_json::Value = get("es").await.json();
    assert_eq!(es["name"], "Tienda de bicis");
    assert_eq!(es["short_name"], "Bicis");
    assert_eq!(es["lang"], "es");
    assert_eq!(es["dir"], "ltr");
    let ar: serde_json::Value = get("ar").await.json();
    assert_eq!(ar["name"], "Bicis arabes");
    assert_eq!(ar["dir"], "rtl");
    let en: serde_json::Value = get("en").await.json();
    assert_eq!(en["name"], "Bikeshop");
    assert_eq!(en["lang"], "en");
}

#[renox::test]
async fn the_worker_version_is_stable_and_follows_the_offline_page() {
    let first = app_offline("<p>Offline one</p>").await;
    let again = app_offline("<p>Offline one</p>").await;
    let changed = app_offline("<p>Offline two</p>").await;
    let a = version_of(&first.get("/sw.js").await.text());
    let b = version_of(&again.get("/sw.js").await.text());
    let c = version_of(&changed.get("/sw.js").await.text());
    assert_eq!(a, b, "the same inputs give the same version");
    assert_ne!(a, c, "another offline page gives another version");
    first
        .get("/_renox/offline")
        .await
        .assert_ok()
        .assert_see("Offline one");
}

#[renox::test]
async fn an_offline_view_that_fails_to_render_is_a_boot_error_naming_the_view() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("views")).unwrap();
    std::fs::write(dir.path().join("views/broken.html"), "{{ nope(").unwrap();
    let mut config = renox::Config::default();
    config.views_path = dir.path().join("views");
    config.public_path = dir.path().join("public");
    for view in ["broken.html", "absent.html"] {
        let pwa = Pwa::new("Bikeshop")
            .icons(icons(dir.path()))
            .offline_page(view);
        let err = App::with_config(config.clone())
            .pwa(pwa)
            .into_router()
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains(view), "{view}: {err:#}");
    }
}

#[renox::test]
async fn pwa_for_gives_each_host_its_own_manifest_colour_and_icons() {
    let dir = tempfile::tempdir().unwrap();
    let public = dir.path().join("public");
    let app = TestApp::with_config(branded(&dir), |c| c.public_path = public).await;
    let manifest = |host: &'static str| {
        let req = app.request().header("host", host);
        async move {
            let res = req.get("/manifest.webmanifest").await;
            assert_eq!(res.header("vary"), Some("Host"));
            res.json::<serde_json::Value>()
        }
    };
    let (plain, north) = (manifest("other.test").await, manifest("north.test").await);
    assert_ne!(plain["name"], north["name"]);
    assert_ne!(plain["theme_color"], north["theme_color"]);
    assert_ne!(plain["icons"][0]["src"], north["icons"][0]["src"]);
    let icon_of = |m: &serde_json::Value| m["icons"][0]["src"].as_str().unwrap().to_owned();
    let plain_icon = app.get(&icon_of(&plain)).await.body;
    let north_icon = app.get(&icon_of(&north)).await.body;
    assert_ne!(plain_icon, north_icon);
}
