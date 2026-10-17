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
