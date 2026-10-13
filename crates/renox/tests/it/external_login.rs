//! #147: logging in another way than with the password (a social login):
//! `auth::sign_in`, `register_verified`, `confirm_identity`, and the login
//! pages' `renox/auth/login_options.html`. The handlers are routed, so
//! their futures are checked for `Send` too.

use renox::auth::events::{LoggedIn, Registered};
use renox::prelude::*;
use renox::testing::TestApp;

/// "Proves" the visitor is `email` (in a real app: a provider said so).
struct Proof;

#[derive(serde::Deserialize)]
struct ProofQuery {
    email: String,
}

impl Module for Proof {
    fn name(&self) -> &'static str {
        "proof"
    }

    fn register(&self, app: &mut Registry) {
        app.second_factor("proof.challenge", |user, _state| async move {
            Ok(user.name == "Guarded")
        });
        app.templates(|env| {
            let _ = env.add_template(
                "renox/auth/login_options.html",
                r#"<p id="options">Other ways on the {{ page }} page</p>"#,
            );
        });
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/proof", prove)
            .get("/proof/confirm", confirm)
            .get("/challenge", || async { "challenge" })
            .name("proof.challenge")
            .get("/", || async { "home" })
            .name("home")
            .merge(
                Routes::new()
                    .get("/secret", || async { "secret" })
                    .require_password_confirmed()
                    .require_auth(),
            )
    }
}

async fn prove(
    State(state): State<AppState>,
    session: Session,
    ClientIp(ip): ClientIp,
    Query(query): Query<ProofQuery>,
) -> Result<Response> {
    let user = match User::find_by_email(&state.db, &query.email).await? {
        Some(user) => user,
        None if renox::auth::registration_open(&state) => {
            renox::auth::register_verified(&state, "", &query.email, &[("via", "proof")]).await?
        }
        None => return Err(Error::Forbidden),
    };
    let to = renox::auth::sign_in(&state, &session, &user, false, ip).await?;
    Ok(Redirect::to(&to).into_response())
}

async fn confirm(session: Session, _user: AuthUser) -> Result<Response> {
    let to = renox::auth::confirm_identity(&session)?;
    Ok(Redirect::to(&to).into_response())
}

async fn app(auth: Auth) -> TestApp {
    TestApp::new(App::new().module(auth).module(Proof)).await
}

#[renox::test]
async fn a_new_address_gets_an_account_without_a_password() {
    let app = app(Auth::new().on_registered(|user, form, _state| async move {
        assert_eq!(form.get("via"), "proof");
        assert_eq!(form.get("email"), user.email);
        Ok(())
    }))
    .await;
    app.fake_events();
    app.get("/proof?email=Nia@Example.com")
        .await
        .assert_redirect("/");
    let nia = User::find_by_email(app.db(), "nia@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(nia.name, "nia");
    assert!(!nia.has_password());
    assert!(nia.email_verified_at.is_some());
    // No typed password matches the empty one.
    assert!(!nia.check_password("").await);
    app.assert_authenticated(Some(&nia));
    assert_eq!(app.emitted::<Registered>().len(), 1);
    assert_eq!(app.emitted::<LoggedIn>().len(), 1);
    // Logging in counts as confirming who they are.
    app.get("/secret").await.assert_ok();
}

#[renox::test]
async fn closed_registration_and_failing_hooks_make_no_account() {
    let app = app(Auth::new().without_registration()).await;
    app.get("/proof?email=nia@example.com")
        .await
        .assert_forbidden();
    app.assert_database_count("users", 0).await;

    let app = app_failing().await;
    app.get("/proof?email=nia@example.com")
        .await
        .assert_status(500);
    app.assert_database_count("users", 0).await;
}

async fn app_failing() -> TestApp {
    app(
        Auth::new().on_registered(|_user, _form, _state| async move {
            Err(Error::Internal(anyhow::anyhow!("the CRM is down")))
        }),
    )
    .await
}

#[renox::test]
async fn sign_in_goes_where_the_login_page_would() {
    let app = app(Auth::new().redirect_to("/dashboard")).await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    app.get("/proof?email=ana@example.com")
        .await
        .assert_redirect("/dashboard");
    app.assert_authenticated(Some(&ana));
}

#[renox::test]
async fn sign_in_waits_for_the_second_step() {
    let app = app(Auth::new()).await;
    User::register(app.db(), "Guarded", "g@example.com", "a long password 12")
        .await
        .unwrap();
    app.get("/proof?email=g@example.com")
        .await
        .assert_redirect("/challenge");
    app.assert_guest();
    // The login waits in the session, as after the right password.
    app.assert_session_has("_auth_pending");
}

#[renox::test]
async fn confirming_another_way_returns_to_the_page_that_asked() {
    let app = app(Auth::new()).await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    app.acting_as(&ana);
    app.get("/secret")
        .await
        .assert_redirect("/confirm-password");
    app.get("/confirm-password")
        .await
        .assert_see("Other ways on the confirm page");
    app.get("/proof/confirm").await.assert_redirect("/secret");
    app.get("/secret").await.assert_ok();
}

#[renox::test]
async fn the_login_pages_include_a_modules_other_ways_to_log_in() {
    let app = app(Auth::new()).await;
    app.get("/login")
        .await
        .assert_see("Other ways on the login page");
    app.get("/register")
        .await
        .assert_see("Other ways on the register page");
    // Without such a module, nothing (and no error).
    let plain = TestApp::new(App::new().module(Auth::new())).await;
    plain
        .get("/login")
        .await
        .assert_ok()
        .assert_dont_see("Other ways");
}

/// A user made by `register_verified`, logged in through `/proof`.
async fn passwordless(app: &TestApp) -> User {
    app.get("/proof?email=nia@example.com")
        .await
        .assert_redirect("/");
    User::find_by_email(app.db(), "nia@example.com")
        .await
        .unwrap()
        .unwrap()
}

#[renox::test]
async fn a_user_without_a_password_sets_one_on_the_account_page() {
    let app = app(Auth::new().account()).await;
    let nia = passwordless(&app).await;
    app.get("/account")
        .await
        .assert_see("Set a password")
        .assert_dont_see("name=\"current_password\"");
    app.put(
        "/account/password",
        &[
            ("password", "a brand new password"),
            ("password_confirmation", "a brand new password"),
        ],
    )
    .await
    .assert_redirect("/account");
    let nia = User::find_or_404(app.db(), nia.id).await.unwrap();
    assert!(nia.has_password());
    assert!(nia.check_password("a brand new password").await);
    // From now on, the current password is asked for.
    app.get("/account")
        .await
        .assert_see("name=\"current_password\"");
    app.htmx()
        .put(
            "/account/password",
            &[
                ("password", "another new password"),
                ("password_confirmation", "another new password"),
            ],
        )
        .await
        .assert_invalid("current_password");
}

#[renox::test]
async fn a_user_without_a_password_confirms_it_is_them_another_way() {
    let app = app(Auth::new().account()).await;
    let nia = passwordless(&app).await;
    // Hours after logging in, the account page's actions need a confirmation.
    app.travel(std::time::Duration::from_secs(4 * 60 * 60));
    app.acting_as(&nia);
    app.delete("/account")
        .await
        .assert_redirect("/confirm-password");
    app.get("/proof/confirm").await.assert_redirect("/account");
    app.post("/account/logout-others", &[])
        .await
        .assert_redirect("/account");
    app.delete("/account").await.assert_redirect("/");
    app.assert_database_count("users", 0).await;
}

#[renox::test]
async fn users_with_a_password_still_type_it() {
    let app = app(Auth::new().account()).await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    app.acting_as(&ana);
    app.confirm_password();
    app.htmx()
        .delete("/account")
        .await
        .assert_invalid("password");
    app.htmx()
        .post("/account/logout-others", &[("password", "wrong")])
        .await
        .assert_invalid("password");
    app.assert_database_count("users", 1).await;
}
