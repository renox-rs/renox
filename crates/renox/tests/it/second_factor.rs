//! #167: a second login step that a module provides (two-factor authentication).

use std::time::Duration;

use renox::auth::events::{LoggedIn, LoginFailed};
use renox::auth::{complete_login, pending_login};
use renox::prelude::*;
use renox::testing::TestApp;
use serde::Deserialize;

const PASSWORD: &str = "a long password 12";
const CODE: &str = "123456";

/// Asks users named "Guarded" for a code after their password.
struct Code;

impl Module for Code {
    fn name(&self) -> &'static str {
        "code"
    }

    fn register(&self, app: &mut Registry) {
        app.second_factor("code.challenge", |user, _state| async move {
            Ok(user.name == "Guarded")
        });
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/code", show)
            .post("/code", check)
            .name("code.challenge")
            .guest_only()
            .get("/", || async { "home" })
            .name("home")
    }
}

async fn show(session: Session) -> Response {
    match pending_login(&session) {
        Some(pending) => format!("code for user {}", pending.user_id).into_response(),
        None => Redirect::to("/login").into_response(),
    }
}

#[derive(Deserialize, Validate)]
struct CodeForm {
    #[validate(required)]
    code: String,
}

async fn check(
    State(state): State<AppState>,
    session: Session,
    htmx: Htmx,
    ClientIp(ip): ClientIp,
    Valid(form): Valid<CodeForm>,
) -> Result<Response> {
    let Some(pending) = pending_login(&session) else {
        return Ok(Redirect::to("/login").into_response());
    };
    if pending.locked_out(&state, ip).await.is_some() {
        return Err(Error::TooManyRequests);
    }
    if form.code != CODE {
        pending.failed(&state, ip).await;
        let mut errors = Errors::new();
        errors.add("code", "That code is wrong.");
        return Err(ValidationError::new(errors).into());
    }
    Ok(
        match complete_login(&state, &session, &pending, ip).await? {
            Some(to) if htmx.request => HxRedirect(to).into_response(),
            Some(to) => Redirect::to(&to).into_response(),
            None => Redirect::to("/login").into_response(),
        },
    )
}

async fn app() -> (TestApp, User, User) {
    let app = TestApp::new(App::new().module(Auth::new().account()).module(Code)).await;
    let guarded = User::register(app.db(), "Guarded", "guarded@example.com", PASSWORD)
        .await
        .unwrap();
    let plain = User::register(app.db(), "Plain", "plain@example.com", PASSWORD)
        .await
        .unwrap();
    (app, guarded, plain)
}

async fn log_in(app: &TestApp, email: &str) -> renox::testing::TestResponse {
    app.post("/login", &[("email", email), ("password", PASSWORD)])
        .await
}

#[renox::test]
async fn users_without_the_step_log_in_as_before() {
    let (app, _, plain) = app().await;
    app.fake_events();
    log_in(&app, &plain.email).await.assert_redirect("/");
    app.get("/account").await.assert_ok();
    assert_eq!(app.emitted::<LoggedIn>().len(), 1);
}

#[renox::test]
async fn the_right_password_waits_for_the_code() {
    let (app, guarded, _) = app().await;
    app.fake_events();
    log_in(&app, &guarded.email).await.assert_redirect("/code");
    // Not logged in yet: the account page sends them to log in.
    app.get("/account").await.assert_redirect("/login");
    assert!(app.emitted::<LoggedIn>().is_empty());

    log_in(&app, &guarded.email).await.assert_redirect("/code");
    app.get("/code")
        .await
        .assert_ok()
        .assert_see(&format!("code for user {}", guarded.id));

    // A wrong code: refused, counted, and still not logged in.
    app.htmx()
        .post("/code", &[("code", "000000")])
        .await
        .assert_invalid("code");
    assert_eq!(app.emitted::<LoginFailed>().len(), 1);
    app.get("/account").await.assert_redirect("/login");

    // The right one finishes the login, and goes where the user was sent
    // from (the account page they opened above).
    app.post("/code", &[("code", CODE)])
        .await
        .assert_redirect("/account");
    app.get("/account").await.assert_ok();
    assert_eq!(app.emitted::<LoggedIn>().len(), 1);
    // And the wait is over: the challenge isn't there any more.
    app.post("/logout", &[]).await;
    app.get("/code").await.assert_redirect("/login");
}

#[renox::test]
async fn the_code_page_sends_the_user_where_they_were_going() {
    let (app, guarded, _) = app().await;
    app.get("/account").await.assert_redirect("/login");
    log_in(&app, &guarded.email).await.assert_redirect("/code");
    app.post("/code", &[("code", CODE)])
        .await
        .assert_redirect("/account");
}

#[renox::test]
async fn htmx_logins_go_to_the_challenge_with_hx_redirect() {
    let (app, guarded, _) = app().await;
    let res = app
        .htmx()
        .post(
            "/login",
            &[("email", &guarded.email), ("password", PASSWORD)],
        )
        .await;
    assert_eq!(res.header("hx-redirect"), Some("/code"));
}

#[renox::test]
async fn a_waiting_login_expires() {
    let (app, guarded, _) = app().await;
    log_in(&app, &guarded.email).await.assert_redirect("/code");
    app.travel(Duration::from_secs(11 * 60));
    app.get("/code").await.assert_redirect("/login");
    app.post("/code", &[("code", CODE)])
        .await
        .assert_redirect("/login");
    app.get("/account").await.assert_redirect("/login");
}

#[renox::test]
async fn a_new_password_ends_the_wait() {
    let (app, guarded, _) = app().await;
    log_in(&app, &guarded.email).await.assert_redirect("/code");
    let mut user = User::find_or_404(app.db(), guarded.id).await.unwrap();
    user.password = renox::auth::hash_password("another password 34")
        .await
        .unwrap();
    user.save(app.db()).await.unwrap();
    app.post("/code", &[("code", CODE)])
        .await
        .assert_redirect("/login");
    app.get("/account").await.assert_redirect("/login");
}

#[renox::test]
async fn wrong_codes_count_towards_the_login_throttle() {
    let (app, guarded, _) = app().await;
    log_in(&app, &guarded.email).await.assert_redirect("/code");
    for _ in 0..5 {
        app.htmx()
            .post("/code", &[("code", "000000")])
            .await
            .assert_invalid("code");
    }
    app.post("/code", &[("code", CODE)])
        .await
        .assert_status(429);
    // Logging in again with the right password doesn't reset the count.
    app.htmx()
        .post(
            "/login",
            &[("email", &guarded.email), ("password", PASSWORD)],
        )
        .await
        .assert_invalid("email");
}

#[renox::test]
async fn remember_me_is_kept_through_the_challenge() {
    let (app, guarded, _) = app().await;
    app.post(
        "/login",
        &[
            ("email", guarded.email.as_str()),
            ("password", PASSWORD),
            ("remember", "1"),
        ],
    )
    .await
    .assert_redirect("/code");
    let res = app.post("/code", &[("code", CODE)]).await;
    res.assert_redirect("/");
    let cookie = res
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("renox_session="))
        .unwrap()
        .to_owned();
    // REMEMBER_LIFETIME (30 days by default), not SESSION_LIFETIME's 2 hours.
    let max_age: u64 = cookie
        .split(';')
        .find_map(|part| part.trim().strip_prefix("Max-Age="))
        .unwrap()
        .parse()
        .unwrap();
    assert!(max_age > 24 * 60 * 60, "{cookie}");
}

#[renox::test]
async fn misconfigured_steps_fail_at_boot() {
    struct Again;
    impl Module for Again {
        fn name(&self) -> &'static str {
            "again"
        }
        fn register(&self, app: &mut Registry) {
            app.second_factor("code.challenge", |_, _| async { Ok(true) });
        }
    }
    let twice = App::with_config(Config::default())
        .module(Auth::new())
        .module(Code)
        .module(Again)
        .boot()
        .await;
    assert!(format!("{:?}", twice.err().unwrap()).contains("two modules set a second login step"));

    struct Nowhere;
    impl Module for Nowhere {
        fn name(&self) -> &'static str {
            "nowhere"
        }
        fn register(&self, app: &mut Registry) {
            app.second_factor("missing.challenge", |_, _| async { Ok(true) });
        }
    }
    let missing = App::with_config(Config::default())
        .module(Auth::new())
        .module(Nowhere)
        .boot()
        .await;
    assert!(format!("{:?}", missing.err().unwrap()).contains("`missing.challenge` doesn't exist"));
}

/// A step the module asks of every matching user also follows registration:
/// signing up doesn't skip it (#219).
#[renox::test]
async fn registration_goes_through_the_step_too() {
    let (app, _, _) = app().await;
    app.fake_events();
    let form = |name: &'static str, email: &'static str| {
        [
            ("name", name),
            ("email", email),
            ("password", PASSWORD),
            ("password_confirmation", PASSWORD),
        ]
    };
    app.post("/register", &form("Guarded", "new-guarded@example.com"))
        .await
        .assert_redirect("/code");
    app.get("/account").await.assert_redirect("/login");
    assert!(app.emitted::<LoggedIn>().is_empty());
    app.post("/code", &[("code", CODE)])
        .await
        .assert_redirect("/");
    app.get("/account").await.assert_ok();

    app.post("/logout", &[]).await;
    app.post("/register", &form("Plain", "new-plain@example.com"))
        .await
        .assert_redirect("/");
    app.get("/account").await.assert_ok();
}
