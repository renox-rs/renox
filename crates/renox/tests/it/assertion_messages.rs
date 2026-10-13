//! #257: what `renox::testing` says when an assertion fails. App authors read
//! these messages when their own tests break, so each one is checked here,
//! along with the assertions that had no test at all.

use renox::auth::{Channel, Notification, Recipient};
use renox::mail::Mail;
use renox::prelude::*;
use renox::testing::TestApp;

#[derive(Clone, Debug)]
struct Paid;

impl Event for Paid {}

struct Welcome;

impl Notification for Welcome {
    fn kind(&self) -> &'static str {
        "welcome"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Mail]
    }

    fn to_mail(&self, to: &Recipient, _: &AppState) -> Result<Mail> {
        Ok(Mail::new(to.email().unwrap_or_default(), "Welcome", "Hi."))
    }
}

#[derive(serde::Deserialize, Validate)]
struct Signup {
    #[validate(required)]
    name: String,
    #[serde(default)]
    email: String,
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/hello", || async { "hello" })
            .get("/long", || async { "x".repeat(700) })
            .get("/go", || async { Redirect::to("/there") })
            .get("/hx", || async { HxRedirect("/x".into()) })
            .get("/header", || async { ([("x-test", "a")], "ok") })
            .post("/signup", |Valid(form): Valid<Signup>| async move {
                let _ = form.email;
                form.name
            })
            .post(
                "/upload",
                |mut parts: renox::axum::extract::Multipart| async move {
                    let mut seen = Vec::new();
                    while let Ok(Some(field)) = parts.next_field().await {
                        let name = field.name().unwrap_or_default().to_owned();
                        let file = field.file_name().map(str::to_owned);
                        let bytes = field.bytes().await.unwrap_or_default();
                        seen.push(format!(
                            "{name}:{}:{}",
                            file.unwrap_or_default(),
                            bytes.len()
                        ));
                    }
                    seen.join(",")
                },
            )
    }
}

async fn app() -> TestApp {
    TestApp::new(App::new().module(Auth::new()).module(Pages)).await
}

#[renox::test]
#[should_panic(expected = "expected status 404, got 200 OK:\nhello")]
async fn a_wrong_status_shows_the_body() {
    app().await.get("/hello").await.assert_status(404);
}

#[renox::test]
#[should_panic(expected = "expected a redirect to /there, got 200 OK:\nhello")]
async fn a_redirect_that_isnt_one() {
    app().await.get("/hello").await.assert_redirect("/there");
}

#[renox::test]
#[should_panic(expected = "expected a redirect to /else, got one to /there")]
async fn a_redirect_to_somewhere_else() {
    app().await.get("/go").await.assert_redirect("/else");
}

#[renox::test]
#[should_panic(expected = "expected HX-Redirect to /y, got `/x` (200 OK)")]
async fn an_hx_redirect_to_somewhere_else() {
    app().await.get("/hx").await.assert_hx_redirect("/y");
}

#[renox::test]
#[should_panic(expected = "expected not to see \"hell\" in:\nhello")]
async fn text_that_shouldnt_be_there() {
    app().await.get("/hello").await.assert_dont_see("hell");
}

#[renox::test]
#[should_panic(expected = "…")]
async fn long_bodies_are_cut_in_messages() {
    app().await.get("/long").await.assert_see("y");
}

#[renox::test]
#[should_panic(expected = "expected header x-test: b, got Some(\"a\")")]
async fn a_header_with_another_value() {
    app()
        .await
        .get("/header")
        .await
        .assert_header("x-test", "b");
}

#[renox::test]
#[should_panic(expected = "the body is not the expected JSON")]
async fn json_from_a_body_that_isnt() {
    let _: serde_json::Value = app().await.get("/hello").await.json();
}

#[renox::test]
#[should_panic(expected = "expected a validation error for `email`, got {\"name\":")]
async fn a_validation_error_for_another_field() {
    app()
        .await
        .request()
        .json()
        .post("/signup", &[("name", "")])
        .await
        .assert_invalid("name")
        .assert_invalid("email");
}

#[renox::test]
#[should_panic(expected = "expected status 422, got 200 OK")]
async fn assert_invalid_on_a_valid_form() {
    app()
        .await
        .request()
        .json()
        .post("/signup", &[("name", "Ann")])
        .await
        .assert_invalid("name");
}

#[renox::test]
#[should_panic(expected = "expected a logged-in session, it's a guest")]
async fn a_guest_isnt_authenticated() {
    app().await.assert_authenticated(None);
}

#[renox::test]
#[should_panic(
    expected = "expected `users` to have a row with email = Text(\"nobody@example.com\")"
)]
async fn a_missing_row_is_described() {
    let app = app().await;
    app.assert_database_has("users", &[("email", &"nobody@example.com")])
        .await;
}

#[renox::test]
#[should_panic(
    expected = "expected `users` to have no row with email = Text(\"ann@example.com\"), found 1"
)]
async fn a_row_that_should_be_gone() {
    let app = app().await;
    User::register(app.db(), "Ann", "ann@example.com", "password123")
        .await
        .unwrap();
    app.assert_database_missing("users", &[("email", &"ann@example.com")])
        .await;
}

#[renox::test]
#[should_panic(
    expected = "no mail to ann@example.com about \"Invoice\"; sent: [\"Welcome (bob@example.com)\"]"
)]
async fn a_missing_mail_lists_what_was_sent() {
    let app = app().await;
    app.mailer()
        .send(Mail::new("bob@example.com", "Welcome", "Hi."))
        .await
        .unwrap();
    assert_eq!(app.mailer().sent().len(), 1);
    app.assert_mail_sent("ann@example.com", "Invoice");
}

#[renox::test]
#[should_panic(expected = "was emitted (0 of that type)")]
async fn an_event_that_wasnt_emitted() {
    let app = app().await;
    app.fake_events();
    app.assert_emitted::<Paid>(|_| true);
}

#[renox::test]
async fn no_events_emitted_passes() {
    let app = app().await;
    app.fake_events();
    app.assert_not_emitted::<Paid>();
}

#[renox::test]
#[should_panic(expected = "assertion_messages::Paid emitted")]
async fn an_event_that_shouldnt_have_been_emitted() {
    let app = app().await;
    app.fake_events();
    app.state().emit(Paid).await.unwrap();
    app.assert_not_emitted::<Paid>();
}

#[renox::test]
#[should_panic(expected = "got no `invoice` notification; sent: [\"welcome\"]")]
async fn a_missing_notification_lists_what_was_sent() {
    let app = app().await;
    app.fake_notifications();
    let ann = User::register(app.db(), "Ann", "ann@example.com", "password123")
        .await
        .unwrap();
    app.state().notify(&ann, &Welcome).await.unwrap();
    app.assert_notified(&ann, "invoice");
}

#[renox::test]
async fn nothing_notified_passes_when_nothing_was_sent() {
    let app = app().await;
    app.fake_notifications();
    app.assert_nothing_notified();
}

#[renox::test]
#[should_panic(expected = "1 notification(s) were sent")]
async fn nothing_notified_fails_after_a_notification() {
    let app = app().await;
    app.fake_notifications();
    let ann = User::register(app.db(), "Ann", "ann@example.com", "password123")
        .await
        .unwrap();
    app.state().notify(&ann, &Welcome).await.unwrap();
    app.assert_nothing_notified();
}

#[renox::test]
#[should_panic(expected = "no matching `tick` was broadcast; sent: [(\"tock\", \"1\")]")]
async fn a_missing_broadcast_lists_what_was_sent() {
    let app = app().await;
    app.fake_broadcasts();
    app.state().broadcast("tock", 1).unwrap();
    app.assert_broadcast("tick", |_| true);
}

#[renox::test]
async fn multipart_posts_carry_fields_and_files_together() {
    let app = app().await;
    app.post_multipart(
        "/upload",
        &[("title", "Holiday"), ("note", "two files")],
        &[("photos", "a.png", b"12345"), ("photos", "b.png", b"123")],
    )
    .await
    .assert_ok()
    .assert_see("title::7,note::9,photos:a.png:5,photos:b.png:3");
}
