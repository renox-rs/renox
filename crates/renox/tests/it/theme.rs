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
        "[{{ renox_theme_url | default('none') }}]",
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

#[tokio::test]
async fn renox_theme_renders_the_link_after_the_kit() {
    let teal = theme("#0F766E");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        "<head><rx-ui/><rx-theme/></head>|{{ renox_theme() }}",
    )
    .unwrap();
    let views = dir.path().to_path_buf();
    let themed = TestApp::with_config(App::new().module(Pages).theme(teal.clone()), {
        let views = views.clone();
        move |c| c.views_path = views
    })
    .await;
    let link = format!("<link rel=\"stylesheet\" href=\"{}\">", url(&teal));
    let body = themed.get("/").await.text();
    assert_eq!(body.matches(&link).count(), 2, "{body}");
    assert!(
        body.find("<script src=").unwrap() < body.find(&link).unwrap(),
        "the theme link comes after the kit: {body}"
    );

    // Without a theme: no link, no error.
    let plain = TestApp::with_config(App::new().module(Pages), move |c| c.views_path = views).await;
    let body = plain.get("/").await.text();
    assert!(!body.contains("/_renox/theme/"), "{body}");
    assert!(body.ends_with('|'), "{body}");
}

#[tokio::test]
async fn the_route_serves_immutable_css_and_answers_404_for_an_unknown_hash() {
    let teal = theme("#0F766E");
    let (app, _dir) = app(App::new().theme(teal.clone())).await;
    let res = app.get(&url(&teal)).await;
    res.assert_ok();
    assert_eq!(
        res.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert!(res.header("content-type").unwrap().starts_with("text/css"));
    let css = res.text();
    for block in [
        ":root{",
        "@media (prefers-color-scheme:dark){:root:not([data-theme=\"light\"]){",
        ":root[data-theme=\"dark\"]{",
    ] {
        assert!(css.contains(block), "{block} in {css}");
    }
    // A hash nobody was handed, a path that is not a hash, and a missing suffix.
    for path in [
        "/_renox/theme/0000000000000000.css",
        "/_renox/theme/nope.css",
        &format!("/_renox/theme/{}", teal.hash()),
    ] {
        app.get(path).await.assert_not_found();
    }
}

#[tokio::test]
async fn two_hosts_render_different_links_and_the_urls_serve_different_css() {
    let (north, south) = (theme("#1E3A5F"), theme("#9A3412"));
    let (n, s) = (north.clone(), south.clone());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        "<head><rx-ui/><rx-theme/></head>",
    )
    .unwrap();
    let views = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Pages)
            .theme_for(move |req: ThemeRequest| {
                let (n, s) = (n.clone(), s.clone());
                async move {
                    Ok(match req.host.as_str() {
                        "north.test" => Some(n),
                        "south.test" => Some(s),
                        _ => None,
                    })
                }
            }),
        move |c| c.views_path = views,
    )
    .await;
    let page = |host: &'static str| {
        let app = &app;
        async move { app.request().header("host", host).get("/").await.text() }
    };
    let (a, b, none) = (
        page("north.test").await,
        page("south.test").await,
        page("x.test").await,
    );
    assert!(a.contains(&url(&north)) && !a.contains(&url(&south)), "{a}");
    assert!(b.contains(&url(&south)) && !b.contains(&url(&north)), "{b}");
    assert!(!none.contains("/_renox/theme/"), "{none}");
    let (css_a, css_b) = (app.get(&url(&north)).await, app.get(&url(&south)).await);
    assert_ne!(css_a.text(), css_b.text());
    assert!(css_a.text().contains(&north.tokens()["--rx-accent"]));
    assert!(css_b.text().contains(&south.tokens()["--rx-accent"]));
}

#[tokio::test]
async fn the_theme_works_under_strict_csp_without_a_nonce() {
    let teal = theme("#0F766E");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        "<head><rx-ui/><rx-theme/></head>",
    )
    .unwrap();
    let views = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pages).theme(teal.clone()), move |c| {
        c.views_path = views;
        c.csp = renox::CspMode::Strict;
    })
    .await;
    let res = app.get("/").await;
    res.assert_ok();
    let csp = res.header("content-security-policy").expect("a CSP header");
    let style_src = csp
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("style-src"))
        .unwrap_or_else(|| panic!("a style-src in {csp}"));
    assert!(style_src.contains("'self'"), "{csp}");
    let html = res.text();
    let link = format!("<link rel=\"stylesheet\" href=\"{}\">", url(&teal));
    assert!(
        html.contains(&link),
        "a plain link, no nonce needed: {html}"
    );
    assert!(
        !html.contains("<style"),
        "the theme adds no inline style: {html}"
    );
    // The stylesheet itself is a same-origin file the policy allows.
    app.get(&url(&teal)).await.assert_ok();
}

#[tokio::test]
async fn the_brand_is_as_specific_as_the_kits_theme_blocks_and_loads_after_them() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("page.html"), "<head><rx-ui/></head>").unwrap();
    let views = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pages), move |c| c.views_path = views).await;
    let html = app.get("/").await.text();
    let href = html
        .split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .find(|h| h.ends_with(".css"))
        .unwrap_or_else(|| panic!("the kit stylesheet in {html}"))
        .to_owned();
    let kit = app.get(&href).await.text();
    // Every `data-rx-theme` selector in the kit sits inside `:where`, which
    // adds no specificity, so a plain `:root{...}` brand block beats it.
    let selectors: Vec<&str> = kit
        .lines()
        .filter(|l| l.contains("[data-rx-theme=") && l.trim_end().ends_with('{'))
        .collect();
    assert!(
        selectors.len() >= 6,
        "classic and warm blocks: {selectors:?}"
    );
    for s in &selectors {
        assert!(s.contains(":where([data-rx-theme="), "{s}");
    }
    let css = theme("#0F766E").css().to_string();
    assert!(!css.contains(":where") && !css.contains("data-rx-theme"));
    assert!(css.contains(":root[data-theme=\"dark\"]{"));
}

#[tokio::test]
async fn a_themed_mail_uses_the_brand_and_keeps_it_when_queued() {
    let navy = theme("#1E3A5F");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("mail")).unwrap();
    std::fs::write(
        dir.path().join("mail/receipt.html"),
        r#"{% extends "renox/mail/layout.html" %}{% from "renox/mail/button.html" import button %}
{% block content %}<h1>Thanks</h1>{{ button("https://shop.test/o/1", "View order") }}{% endblock %}"#,
    )
    .unwrap();
    let views = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new(), move |c| c.views_path = views).await;
    let tokens = navy.tokens();
    let mail = app
        .state()
        .mail_view("ben@example.com", "Receipt", "mail/receipt", context! {})
        .unwrap();
    assert!(
        mail.html.as_deref().unwrap().contains("#1c1917"),
        "the default ink"
    );
    let mail = mail.theme(&navy);
    let html = mail.html.clone().unwrap();
    assert!(
        html.contains(&format!(
            "background:{};color:{};",
            tokens["--rx-accent"], tokens["--rx-on-accent"]
        )),
        "{html}"
    );
    assert!(!html.contains("background:#1c1917"), "{html}");

    app.state().queue_mail(mail).await.unwrap();
    app.kernel().run_jobs().await.unwrap();
    let sent = app.sent_mail();
    let sent_html = sent[0].html.as_deref().unwrap();
    assert!(sent_html.contains(&tokens["--rx-accent"]), "{sent_html}");
}
