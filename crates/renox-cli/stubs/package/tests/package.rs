use std::path::Path;

use __lib__::__Struct__;
use renox::prelude::*;
use renox::testing::TestApp;

/// The pages extend the app's layout; a package test has no app, so it
/// brings the smallest one.
const LAYOUT: &str = r#"<!doctype html>
<html><body><rx-outlet name="content"/></body></html>
"#;

/// A views directory holding the layout and `files` (the app's own views).
fn views(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary views directory");
    for (name, text) in [("layouts/app.html", LAYOUT)].iter().chain(files) {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("a file path")).expect("a directory");
        std::fs::write(path, text).expect("a view file");
    }
    dir
}

/// The module mounted in an empty app that uses `views`.
async fn boot(views: &Path) -> TestApp {
    TestApp::with_config(App::new().module(__Struct__), |config| {
        config.views_path = views.to_path_buf();
    })
    .await
}

#[renox::test]
async fn the_page_answers() {
    let dir = views(&[]);
    let app = boot(dir.path()).await;
    app.get("/__crate__")
        .await
        .assert_ok()
        .assert_view("__crate__/index.html")
        .assert_see("Nothing here yet.");
}

#[renox::test]
async fn the_models_match_the_schema() {
    let dir = views(&[]);
    boot(dir.path()).await.assert_models_match_schema().await;
}

#[renox::test]
async fn every_view_compiles() {
    let dir = views(&[]);
    boot(dir.path()).await.assert_views_compile();
}

#[renox::test]
async fn an_app_file_of_the_same_name_wins() {
    let dir = views(&[(
        "__crate__/index.html",
        "<rx-page layout=\"layouts/app.html\" title=\"Mine\">The app's own page.</rx-page>",
    )]);
    let app = boot(dir.path()).await;
    app.get("/__crate__")
        .await
        .assert_ok()
        .assert_see("The app's own page.")
        .assert_dont_see("Nothing here yet.");
}
