//! Brand themes (#36): the theme an app or a per-request chooser picks, as
//! the stylesheet URL the views get.

use renox::prelude::*;
use renox::testing::TestApp;
use renox::theme::{Brand, Theme, ThemeRequest};

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/", || async { view("page.html", context! {}) })
    }
}

fn theme(color: &str) -> Theme {
    Theme::from_brand(&Brand::new(color)).unwrap()
}

async fn app(app: App) -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        "[{{ renox_theme | default('none') }}]",
    )
    .unwrap();
    let views = dir.path().to_path_buf();
    let app = TestApp::with_config(app.module(Pages), move |c| c.views_path = views).await;
    (app, dir)
}

fn url(theme: &Theme) -> String {
    format!("/_renox/theme/{}.css", theme.hash())
}

#[tokio::test]
async fn no_theme_shares_nothing() {
    let (app, _dir) = app(App::new()).await;
    assert_eq!(app.get("/").await.text(), "[none]");
}

#[tokio::test]
async fn the_app_theme_reaches_every_page_and_is_served() {
    let teal = theme("#0F766E");
    let (app, _dir) = app(App::new().theme(teal.clone())).await;
    assert_eq!(app.get("/").await.text(), format!("[{}]", url(&teal)));
    let css = app.get(&url(&teal)).await;
    assert_eq!(css.status.as_u16(), 200);
    assert_eq!(css.text(), teal.css());
}

#[tokio::test]
async fn the_chooser_picks_a_theme_by_host_and_falls_back() {
    let (north, south, fallback) = (theme("#0F766E"), theme("#B8532E"), theme("#1E3A5F"));
    let (n, s) = (north.clone(), south.clone());
    let (app, _dir) =
        app(App::new()
            .theme(fallback.clone())
            .theme_for(move |req: ThemeRequest| {
                let (n, s) = (n.clone(), s.clone());
                async move {
                    match req.host.as_str() {
                        "north.test" => Ok(Some(n)),
                        "south.test" => Ok(Some(s)),
                        "broken.test" => Err(renox::Error::BadRequest("no brand".into())),
                        _ => Ok(None),
                    }
                }
            }))
        .await;

    let page = |host: &'static str| {
        let app = &app;
        async move { app.request().header("host", host).get("/").await.text() }
    };
    assert_eq!(page("north.test").await, format!("[{}]", url(&north)));
    assert_eq!(page("south.test:3000").await, format!("[{}]", url(&south)));
    // No answer, or a failing chooser: the app's theme.
    assert_eq!(page("other.test").await, format!("[{}]", url(&fallback)));
    assert_eq!(page("broken.test").await, format!("[{}]", url(&fallback)));

    // Every theme handed out is served, and they differ.
    let north_css = app.get(&url(&north)).await.text();
    let south_css = app.get(&url(&south)).await.text();
    assert_ne!(north_css, south_css);
}

#[tokio::test]
async fn a_chooser_without_an_app_theme_leaves_other_hosts_unthemed() {
    let north = theme("#0F766E");
    let n = north.clone();
    let (app, _dir) = app(App::new().theme_for(move |req: ThemeRequest| {
        let n = n.clone();
        async move { Ok((req.host == "north.test").then_some(n)) }
    }))
    .await;
    let page = |host: &'static str| {
        let app = &app;
        async move { app.request().header("host", host).get("/").await.text() }
    };
    assert_eq!(page("north.test").await, format!("[{}]", url(&north)));
    assert_eq!(page("x.test").await, "[none]");
}
