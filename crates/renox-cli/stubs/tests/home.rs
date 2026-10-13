use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

#[renox::test]
async fn the_home_page_works() {
    let app = TestApp::new({{crate_name}}::app()).await;
    app.get("/")
        .await
        .assert_ok()
        .assert_view("home/index.html")
        .assert_see("<h1");
}

#[renox::test]
async fn guests_can_register() {
    let app = TestApp::new({{crate_name}}::app()).await;
    app.post(
        "/register",
        &[
            ("name", "Anna"),
            ("email", "anna@example.com"),
            ("password", "secret123"),
            ("password_confirmation", "secret123"),
        ],
    )
    .await
    .assert_redirect("/");
    app.assert_database_has("users", &[("email", &"anna@example.com")])
        .await;
    // Registered and logged in: the account page is theirs.
    app.get("/account")
        .await
        .assert_ok()
        .assert_see("anna@example.com");
}

#[renox::test]
async fn sessions_end_after_their_lifetime() {
    let app = TestApp::new({{crate_name}}::app()).await;
    let user = User::register(app.db(), "Anna", "anna@example.com", "secret123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.get("/account").await.assert_ok();
    // Time moves for the app (renox::db::now(), sessions, the queue), not for the test.
    app.travel(Duration::from_secs(3 * 60 * 60)); // past SESSION_LIFETIME's 120 minutes
    app.get("/account").await.assert_redirect("/login");
}

#[renox::test]
async fn the_models_match_the_schema() {
    TestApp::new({{crate_name}}::app())
        .await
        .assert_models_match_schema()
        .await;
}

#[renox::test]
async fn every_view_compiles() {
    let app = TestApp::new({{crate_name}}::app()).await;
    app.assert_views_compile();
}
