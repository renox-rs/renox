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
