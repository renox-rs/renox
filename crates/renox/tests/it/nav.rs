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

#[renox::test]
async fn a_layout_reached_after_an_apostrophe_in_a_comment_still_names_its_menu() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write(
        "layout.html",
        "<rx-sidebar menu=\"staff\"></rx-sidebar>{% block body %}{% endblock %}",
    );
    write(
        "invoices.html",
        "{# the page's note #}{% extends \"layout.html\" %}{% block body %}x{% endblock %}",
    );
    write("plain.html", "plain page");
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Pages)
            .nav("staff", NavLink::url("/docs", "Docs").key("docs")),
        move |c| c.views_path = path,
    )
    .await;
    app.get("/invoices")
        .await
        .assert_ok()
        .assert_see(r#"href="/docs""#);
}

// ---- Acceptance checks (AC1-AC9 of the navigation menus) ----

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "stores")]
struct Store {
    id: i64,
    name: String,
}

/// Routes `invoices.index` and `invoices.show`, and a page that lists the
/// `staff` menu as `[group:label*#badge,]`.
struct Ledger;

impl Module for Ledger {
    fn name(&self) -> &'static str {
        "nav-ledger"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/invoices", || async { view("menu.html", context! {}) })
            .name("invoices.index")
            .get("/invoices/{id}", |Path(_id): Path<i64>| async {
                view("menu.html", context! {})
            })
            .name("invoices.show")
            .get("/plain", || async { view("plain.html", context! {}) })
            .name("plain")
    }
}

const LIST: &str = "{% for g in nav('staff') %}[{{ g.label }}:{% for l in g.items %}{{ l.label }}{% if l.active %}*{% endif %}{% if l.badge %}#{{ l.badge }}{% endif %},{% endfor %}]{% endfor %}";

fn views(menu_html: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("menu.html"), menu_html).unwrap();
    std::fs::write(dir.path().join("plain.html"), "plain page").unwrap();
    dir
}

async fn app_in(dir: &tempfile::TempDir, app: App) -> TestApp {
    let path = dir.path().to_path_buf();
    TestApp::with_config(app, move |c| c.views_path = path).await
}

async fn member(app: &TestApp, email: &str) -> User {
    User::register(app.db(), "U", email, "a long password 12")
        .await
        .unwrap()
}

struct Early;

impl Module for Early {
    fn name(&self) -> &'static str {
        "nav-early"
    }

    fn register(&self, app: &mut Registry) {
        app.nav("staff", NavLink::url("/z2", "z2").group("Zeta").order(30));
        app.nav("staff", NavLink::url("/z1", "z1").group("Zeta").order(5));
    }
}

struct Late;

impl Module for Late {
    fn name(&self) -> &'static str {
        "nav-late"
    }

    fn register(&self, app: &mut Registry) {
        app.nav("staff", NavLink::url("/a2", "a2").group("Alpha").order(40));
        app.nav("staff", NavLink::url("/a1", "a1").group("Alpha").order(10));
    }
}

// AC1
#[renox::test]
async fn links_from_two_modules_and_the_app_render_in_one_grouped_menu() {
    let dir = views(LIST);
    let app = app_in(
        &dir,
        App::new()
            .module(Ledger)
            .module(Early)
            .module(Late)
            .nav("staff", NavLink::url("/top", "Top").order(99)),
    )
    .await;
    // Ungrouped links first; groups by their lowest order; links by order.
    app.get("/invoices")
        .await
        .assert_ok()
        .assert_see("[None:Top,][Zeta:z1,z2,][Alpha:a1,a2,]");
}

async fn with_permission_app(permissions: bool) -> (TestApp, tempfile::TempDir) {
    let dir = views(LIST);
    let mut app = App::new()
        .module(Auth::new())
        .module(Ledger)
        .nav("staff", NavLink::url("/open", "Open").order(1).group("G"))
        .nav(
            "staff",
            NavLink::url("/bills", "Bills")
                .order(2)
                .group("G")
                .permission("invoices.view"),
        );
    if permissions {
        app = app.module(renox::auth::permissions::Permissions);
    } else {
        // Without the module a permission name falls back to the gates.
        app = app.gate("invoices.view", |user| user.email.starts_with("gated"));
    }
    (app_in(&dir, app).await, dir)
}

// AC2
#[renox::test]
async fn a_permission_link_follows_global_and_scoped_grants() {
    let (app, _dir) = with_permission_app(true).await;
    let db = app.db();
    renox::auth::permissions::define_role(db, "biller", &["invoices.view"])
        .await
        .unwrap();
    // Anonymous visitors never see a restricted link.
    app.get("/invoices")
        .await
        .assert_see("[G:Open,]")
        .assert_dont_see("Bills");

    let none = member(&app, "none@example.com").await;
    app.acting_as(&none);
    app.get("/invoices")
        .await
        .assert_see("[G:Open,]")
        .assert_dont_see("Bills");

    let global = member(&app, "global@example.com").await;
    global.assign_role(db, "biller").await.unwrap();
    app.acting_as(&global);
    app.get("/invoices").await.assert_see("[G:Open,Bills,]");

    // Only through a role scoped to one store: held "anywhere".
    let scoped = member(&app, "scoped@example.com").await;
    scoped
        .assign_role_in(
            db,
            "biller",
            &renox::auth::permissions::Scope::of_id::<Store>(2),
        )
        .await
        .unwrap();
    app.acting_as(&scoped);
    app.get("/invoices").await.assert_see("[G:Open,Bills,]");
}

#[renox::test]
async fn without_the_permissions_module_a_permission_name_falls_back_to_the_gates() {
    let (app, _dir) = with_permission_app(false).await;
    let plain = member(&app, "plain@example.com").await;
    app.acting_as(&plain);
    app.get("/invoices")
        .await
        .assert_see("[G:Open,]")
        .assert_dont_see("Bills");
    let gated = member(&app, "gated@example.com").await;
    app.acting_as(&gated);
    app.get("/invoices").await.assert_see("[G:Open,Bills,]");
}

// AC3
#[renox::test]
async fn gate_and_when_hide_links_and_anonymous_users_see_only_open_ones() {
    let dir = views(LIST);
    let app = app_in(
        &dir,
        App::new()
            .module(Auth::new())
            .module(Ledger)
            .gate("admin", |user| user.email.ends_with("@shop.test"))
            .nav("staff", NavLink::url("/open", "Open").order(1))
            .nav(
                "staff",
                NavLink::url("/admin", "Admin").order(2).gate("admin"),
            )
            .nav(
                "staff",
                NavLink::url("/vip", "Vip")
                    .order(3)
                    .when(|user| user.user().email.starts_with("vip")),
            )
            .nav(
                "staff",
                NavLink::url("/ghost", "Ghost").order(4).gate("nope"),
            ),
    )
    .await;
    app.get("/invoices").await.assert_see("[None:Open,]");
    let user = member(&app, "vip@elsewhere.test").await;
    app.acting_as(&user);
    // `when` passes, the gate does not, an unknown gate denies.
    app.get("/invoices").await.assert_see("[None:Open,Vip,]");
    let admin = member(&app, "boss@shop.test").await;
    app.acting_as(&admin);
    app.get("/invoices").await.assert_see("[None:Open,Admin,]");
}

// AC4
#[renox::test]
async fn the_active_link_follows_the_route_and_its_children() {
    let dir = views(&format!("<rx-sidebar menu=\"staff\"></rx-sidebar>{LIST}"));
    let app = app_in(
        &dir,
        App::new()
            .module(Ledger)
            .nav("staff", NavLink::route("invoices.index", "Invoices"))
            .nav("staff", NavLink::route("plain", "Plain")),
    )
    .await;
    app.get("/invoices")
        .await
        .assert_see("[None:Invoices*,Plain,]");
    // A child route keeps its parent's link active, with `aria-current`.
    app.get("/invoices/7")
        .await
        .assert_ok()
        .assert_see("[None:Invoices*,Plain,]")
        .assert_see(r#"<a class="rx-sidebar__link" href="/invoices" aria-current="page">"#)
        .assert_see(r#"<a class="rx-sidebar__link" href="/plain">"#);
}

static OK_BADGES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static HIDDEN_BADGES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

// AC5
#[renox::test]
async fn badges_run_for_visible_links_and_a_failing_badge_is_dropped() {
    use std::sync::atomic::Ordering::SeqCst;
    let dir = views(LIST);
    let app = app_in(
        &dir,
        App::new()
            .module(Auth::new())
            .module(Ledger)
            .nav(
                "staff",
                NavLink::url("/ok", "Ok")
                    .order(1)
                    .badge(|_s: AppState, _u: AuthUser| async move {
                        OK_BADGES.fetch_add(1, SeqCst);
                        Ok(Some("2".to_owned()))
                    }),
            )
            .nav(
                "staff",
                NavLink::url("/broken", "Broken")
                    .order(2)
                    .badge(|_s: AppState, _u: AuthUser| async move { Err(Error::NotFound) }),
            )
            .nav(
                "staff",
                NavLink::url("/hidden", "Hidden")
                    .order(3)
                    .gate("nope")
                    .badge(|_s: AppState, _u: AuthUser| async move {
                        HIDDEN_BADGES.fetch_add(1, SeqCst);
                        Ok(Some("9".to_owned()))
                    }),
            ),
    )
    .await;
    let user = member(&app, "ana@example.com").await;
    app.acting_as(&user);
    app.get("/plain").await.assert_ok();
    assert_eq!(OK_BADGES.load(SeqCst), 0, "no menu on the page, no badge");
    app.get("/invoices")
        .await
        .assert_ok()
        .assert_see("[None:Ok#2,Broken,]");
    assert_eq!(OK_BADGES.load(SeqCst), 1);
    assert_eq!(HIDDEN_BADGES.load(SeqCst), 0, "a hidden link runs no badge");
}

// AC6
#[renox::test]
async fn nav_hide_removes_a_link_and_a_duplicate_key_names_the_menu() {
    let dir = views(LIST);
    let app = app_in(
        &dir,
        App::new()
            .module(Ledger)
            .nav("staff", NavLink::url("/docs", "Docs").key("docs").order(1))
            .nav("staff", NavLink::url("/help", "Help").order(2))
            .nav_hide("staff", "docs"),
    )
    .await;
    app.get("/invoices")
        .await
        .assert_see("[None:Help,]")
        .assert_dont_see("Docs");

    let err = App::with_config(Config::default())
        .nav("staff", NavLink::url("/a", "A").key("same"))
        .nav("staff", NavLink::url("/b", "B").key("same"))
        .boot()
        .await
        .err()
        .unwrap();
    let message = format!("{err:?}");
    assert!(
        message.contains("two links with the key `same`"),
        "{message}"
    );
    assert!(message.contains("staff"), "{message}");
}

// AC7
#[renox::test]
async fn group_and_link_labels_go_through_t() {
    let dir = views(LIST);
    let lang = tempfile::tempdir().unwrap();
    std::fs::write(
        lang.path().join("en.json"),
        r#"{ "nav": { "billing": "Billing desk", "invoices": "All invoices" } }"#,
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let lang_path = lang.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Ledger)
            .nav(
                "staff",
                NavLink::url("/i", "nav.invoices").group("nav.billing"),
            )
            .nav(
                "staff",
                NavLink::url("/x", "Untranslated").group("Plain group"),
            ),
        move |c| {
            c.views_path = path;
            c.lang_path = lang_path;
        },
    )
    .await;
    app.get("/invoices")
        .await
        .assert_see("[Billing desk:All invoices,]")
        .assert_see("[Plain group:Untranslated,]");
}

// AC8
#[renox::test]
async fn nav_in_custom_markup_and_view_check_knows_menu_and_nav() {
    let dir = views(
        "<ul>{% for g in nav('staff') %}{% for l in g.items %}<li class=\"mine\">{{ l.label }}</li>{% endfor %}{% endfor %}</ul>\
         <rx-sidebar menu=\"staff\"></rx-sidebar><rx-nav-links menu=\"staff\"></rx-nav-links>",
    );
    let app = app_in(
        &dir,
        App::new()
            .module(Ledger)
            .nav("staff", NavLink::url("/docs", "Docs")),
    )
    .await;
    app.get("/invoices")
        .await
        .assert_ok()
        .assert_see(r#"<li class="mine">Docs</li>"#);
    // `nav` is a request global, `menu` a known prop: nothing is reported.
    app.assert_views_compile();
}

struct Own;

impl Module for Own {
    fn name(&self) -> &'static str {
        "own-nav"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/own", || async {
            view("own.html", context! { nav => vec!["one", "two"] })
        })
    }
}

#[renox::test]
async fn a_page_that_passes_its_own_nav_value_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("own.html"),
        "{% for item in nav %}<{{ item }}>{% endfor %}",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Auth::new()).module(Own), move |c| {
        c.views_path = path
    })
    .await;
    app.get("/own").await.assert_ok().assert_see("<one><two>");
}
