//! Running the shop (#243): error pages in the shop's own layout, errors
//! reported to `storage/logs/errors.log`, maintenance mode, `/health`.

use bikeshop::report;
use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

/// A route that fails, to see a 500 the way a visitor would.
struct Boom;

impl Module for Boom {
    fn name(&self) -> &'static str {
        "boom"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/boom", boom).name("boom")
    }
}

async fn boom() -> Result<&'static str> {
    Err(renox::anyhow::anyhow!("the chain came off").into())
}

/// The app as in production (no debug details), plus the failing route.
async fn production() -> TestApp {
    TestApp::with_config(bikeshop::app().module(Boom), |c| c.debug = false).await
}

#[renox::test]
async fn a_missing_page_is_a_404_in_the_shops_layout() {
    let app = production().await;
    app.get("/no-such-page")
        .await
        .assert_not_found()
        // The public layout: its grid, the navbar's links, the footer.
        .assert_see("bs-public")
        .assert_see("href=\"/shop\"")
        .assert_see("Go to the home page");
}

#[renox::test]
async fn a_failure_is_a_500_in_the_shops_layout_and_is_reported() {
    let app = production().await;
    let res = app.get("/boom").await;
    res.assert_status(500)
        .assert_see("bs-public")
        .assert_see("Go to the home page")
        .assert_dont_see("the chain came off");
    let id = res.header("x-request-id").unwrap().to_owned();

    // The reporter runs after the response: wait for its line.
    let path = report::log_path(app.state());
    let mut written = String::new();
    for _ in 0..100 {
        written = std::fs::read_to_string(&path).unwrap_or_default();
        if !written.is_empty() {
            break;
        }
        renox::tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let line: renox::serde_json::Value =
        renox::serde_json::from_str(written.lines().next().expect("a report")).unwrap();
    assert_eq!(line["kind"], "request");
    assert_eq!(line["message"], "the chain came off");
    assert_eq!(line["request"]["path"], "/boom");
    assert_eq!(line["request"]["id"], id.as_str());
}

#[renox::test]
async fn maintenance_mode_shows_the_shops_page_and_health_still_answers() {
    let app = production().await;
    let storage = app.state().config.storage_path.clone();
    renox::maintenance::down(&storage, renox::maintenance::DownOptions::new().retry(60)).unwrap();
    let res = app.get("/").await;
    res.assert_status(503)
        .assert_see("bs-public")
        .assert_see("quick tune-up");
    assert_eq!(res.header("retry-after"), Some("60"));
    // In the visitor's language.
    app.request()
        .header("accept-language", "es")
        .get("/")
        .await
        .assert_status(503)
        .assert_see("puesta a punto");
    app.get("/health").await.assert_ok();

    assert!(renox::maintenance::up(&storage).unwrap());
    app.get("/").await.assert_ok();
}
