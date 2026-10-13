//! #168: sections other modules add to the `/account` page.

use renox::prelude::*;
use renox::testing::TestApp;

/// Adds two sections, out of order, and reads the user in one of them.
struct Extras;

impl Module for Extras {
    fn name(&self) -> &'static str {
        "extras"
    }

    fn register(&self, app: &mut Registry) {
        app.templates(|env| {
            env.add_template(
                "extras/security.html",
                r#"<section id="security">Security for {{ user.name }}: {{ section.data.level }}</section>"#,
            )
            .unwrap();
            env.add_template(
                "extras/logins.html",
                r#"<section id="logins">{{ section.data.count }} linked logins</section>"#,
            )
            .unwrap();
        });
        app.account_section("extras/logins.html", 20, |_user, _state| async {
            Ok(json!({ "count": 2 }))
        });
        app.account_section("extras/security.html", 10, |user, _state| async move {
            Ok(json!({ "level": if user.name == "Ana" { "high" } else { "normal" } }))
        });
    }
}

async fn app(with_extras: bool) -> TestApp {
    let mut app = App::new().module(Auth::new().account());
    if with_extras {
        app = app.module(Extras);
    }
    let app = TestApp::new(app).await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    app.acting_as(&ana);
    app
}

#[renox::test]
async fn modules_add_sections_to_the_account_page() {
    let app = app(true).await;
    let page = app.get("/account").await.assert_ok().text();
    let security = page.find(r#"<section id="security">Security for Ana: high</section>"#);
    let logins = page.find(r#"<section id="logins">2 linked logins</section>"#);
    let password = page.find(r#"name="current_password""#);
    let delete = page.find(r#"id="delete-account""#);
    let (Some(security), Some(logins), Some(password), Some(delete)) =
        (security, logins, password, delete)
    else {
        panic!("a section or a built-in card is missing:\n{page}");
    };
    // In `order`, after the built-in cards, before deleting the account.
    assert!(
        password < security && security < logins && logins < delete,
        "{page}"
    );
}

#[renox::test]
async fn without_sections_the_page_is_as_before() {
    let app = app(false).await;
    app.get("/account")
        .await
        .assert_ok()
        .assert_see(r#"id="delete-account""#)
        .assert_dont_see(r#"id="security""#);
}
