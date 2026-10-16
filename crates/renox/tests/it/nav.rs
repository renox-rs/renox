//! Navigation links contributed to menus (`NavLink`, `App::nav`, `App::nav_hide`).

use renox::nav::NavLink;
use renox::prelude::*;
use renox::testing::TestApp;

struct Billing;

impl Module for Billing {
    fn name(&self) -> &'static str {
        "billing"
    }

    fn register(&self, app: &mut Registry) {
        app.nav(
            "staff",
            NavLink::route("invoices.index", "Invoices")
                .icon("receipt")
                .group("Billing")
                .order(10)
                .permission("invoices.view")
                .gate("admin")
                .when(|user| user.user().id > 0)
                .badge(|_state: AppState, _user: AuthUser| async move { Ok(Some("3".to_owned())) }),
        );
    }
}

#[renox::test]
async fn modules_and_the_app_contribute_links_to_one_menu() {
    let app = App::with_config(Config::default())
        .module(Billing)
        .nav("staff", NavLink::url("/docs", "Docs").key("docs"))
        .nav("portal", NavLink::route("invoices.index", "Invoices"))
        .nav_hide("staff", "docs");
    assert!(app.boot().await.is_ok());
}

#[renox::test]
async fn a_duplicate_key_in_one_menu_is_a_boot_error() {
    let err = App::with_config(Config::default())
        .module(Billing)
        .nav("staff", NavLink::route("invoices.index", "Again"))
        .boot()
        .await
        .err()
        .unwrap();
    assert!(
        format!("{err:?}").contains("two links with the key `invoices.index`"),
        "{err:?}"
    );
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "nav-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/invoices", || async { view("invoices.html", context! {}) })
            .name("invoices.index")
            .get("/plain", || async { view("plain.html", context! {}) })
            .name("plain")
    }
}

static BADGE_RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[renox::test]
async fn nav_lists_the_menu_in_templates_and_builds_it_only_when_named() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write("layout.html", "{% block body %}{% endblock %}");
    write(
        "invoices.html",
        "{% extends 'layout.html' %}{% block body %}{% for group in nav('staff') %}[{{ group.label }}:{% for link in group.items %}{{ link.label }}={{ link.url }}{% if link.active %}*{% endif %}{% if link.badge %}#{{ link.badge }}{% endif %},{% endfor %}]{% endfor %}{% endblock %}",
    );
    write("plain.html", "plain page");
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(Pages)
            .nav(
                "staff",
                NavLink::route("invoices.index", "Invoices")
                    .group("Billing")
                    .badge(|_state: AppState, _user: AuthUser| async move {
                        BADGE_RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(Some("3".to_owned()))
                    }),
            )
            .nav("staff", NavLink::url("/docs", "Docs").key("docs")),
        move |c| c.views_path = path,
    )
    .await;
    // Anonymous visitors see unrestricted links; badges need a user.
    app.get("/invoices")
        .await
        .assert_ok()
        .assert_see("[None:Docs=/docs,][Billing:Invoices=/invoices*,]");
    assert_eq!(BADGE_RUNS.load(std::sync::atomic::Ordering::SeqCst), 0);
    let ana = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    app.acting_as(&ana);
    // A page that doesn't name the menu runs no badge.
    app.get("/plain").await.assert_ok().assert_see("plain page");
    assert_eq!(BADGE_RUNS.load(std::sync::atomic::Ordering::SeqCst), 0);
    app.get("/invoices")
        .await
        .assert_ok()
        .assert_see("Invoices=/invoices*#3,");
    assert_eq!(BADGE_RUNS.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[renox::test]
async fn rx_sidebar_and_rx_nav_links_draw_a_menu() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write(
        "invoices.html",
        "<rx-sidebar brand=\"Shop\" menu=\"staff\"><rx-sidebar-link href=\"/by-hand\" label=\"By hand\"/></rx-sidebar>\
         <rx-nav-links menu=\"staff\"></rx-nav-links>",
    );
    write("plain.html", "plain page");
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Pages)
            .nav(
                "staff",
                NavLink::route("invoices.index", "Invoices")
                    .group("Billing")
                    .icon("receipt"),
            )
            .nav("staff", NavLink::url("/docs", "Docs").key("docs")),
        move |c| c.views_path = path,
    )
    .await;
    app.get("/invoices")
        .await
        .assert_ok()
        .assert_see(r#"<p class="rx-sidebar__section">Billing</p>"#)
        .assert_see(r#"<a class="rx-sidebar__link" href="/invoices" aria-current="page">"#)
        .assert_see(r#"<a class="rx-sidebar__link" href="/docs"><span>Docs</span></a>"#)
        .assert_see(r#"<a class="rx-sidebar__link" href="/by-hand">"#)
        .assert_see(
            r#"<a class="rx-navbar__link" href="/invoices" aria-current="page"><span>Invoices</span></a>"#,
        );
}

#[renox::test]
async fn the_same_key_in_two_menus_is_fine() {
    let app = App::with_config(Config::default())
        .nav("staff", NavLink::route("home", "Home"))
        .nav("portal", NavLink::route("home", "Home"));
    assert!(app.boot().await.is_ok());
}
