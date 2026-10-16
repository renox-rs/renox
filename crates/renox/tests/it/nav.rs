//! Navigation links contributed to menus (`NavLink`, `App::nav`, `App::nav_hide`).

use renox::nav::NavLink;
use renox::prelude::*;

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

#[renox::test]
async fn the_same_key_in_two_menus_is_fine() {
    let app = App::with_config(Config::default())
        .nav("staff", NavLink::route("home", "Home"))
        .nav("portal", NavLink::route("home", "Home"));
    assert!(app.boot().await.is_ok());
}
