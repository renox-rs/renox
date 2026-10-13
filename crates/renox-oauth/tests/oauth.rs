//! Social login against fake providers (`TestApp::fake_http`: no network):
//! the redirect with `state` and PKCE, the callback's checks, logging in,
//! linking by verified email, new accounts without a password, the second
//! login step, linking and unlinking from the account page, and confirming
//! who you are with a linked provider.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use renox::audit::Audit;
use renox::auth::events::{LoggedIn, Registered};
use renox::http::{FakeHttp, FakeResponse};
use renox::prelude::*;
use renox::testing::{TestApp, TestResponse};
use renox_oauth::{
    AccountLinked, AccountUnlinked, GitHub, Google, LoggedInWith, OAuth, OAuthAccount,
};
use sha2::{Digest, Sha256};

const PASSWORD: &str = "a long password 12";
const GOOGLE_TOKEN: &str = "POST https://oauth2.googleapis.com/token";
const GOOGLE_USER: &str = "https://openidconnect.googleapis.com/v1/userinfo";

fn oauth() -> OAuth {
    OAuth::new()
        .provider(Google::new("google-id", "google-secret"))
        .provider(GitHub::new("github-id", "github-secret"))
}

async fn app_with(auth: Auth) -> TestApp {
    TestApp::new(App::new().module(auth).module(Audit).module(oauth())).await
}

async fn app() -> TestApp {
    app_with(Auth::new().account()).await
}

/// Google answers the code with a token, and the token with this person.
fn google_says(app: &TestApp, sub: &str, email: &str, verified: bool) -> FakeHttp {
    let http = app.fake_http();
    http.on(
        GOOGLE_TOKEN,
        FakeResponse::json(
            200,
            json!({ "access_token": "google-token", "token_type": "Bearer", "expires_in": 3599 }),
        ),
    );
    http.on(
        GOOGLE_USER,
        FakeResponse::json(
            200,
            json!({
                "sub": sub,
                "email": email,
                "email_verified": verified,
                "name": "Nia Example",
                "picture": "https://example.com/nia.png",
            }),
        ),
    );
    http
}

/// The query string of `url` as pairs.
fn query(url: &str) -> Vec<(String, String)> {
    let (_, query) = url.split_once('?').unwrap_or((url, ""));
    form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn param(pairs: &[(String, String)], name: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

/// Starts a sign-in; returns the provider's authorization URL.
async fn start(app: &TestApp, provider: &str, extra: &str) -> String {
    let response = app.get(&format!("/auth/{provider}/redirect{extra}")).await;
    response.assert_status(303);
    response.header("location").unwrap().to_owned()
}

/// The provider sends the browser back with a code and `state`.
async fn back(app: &TestApp, provider: &str, state: &str) -> TestResponse {
    let state: String = form_urlencoded::byte_serialize(state.as_bytes()).collect();
    app.get(&format!(
        "/auth/{provider}/callback?code=the-code&state={state}"
    ))
    .await
}

/// A whole sign-in: to the provider and back.
async fn sign_in_with(app: &TestApp, provider: &str) -> TestResponse {
    let url = start(app, provider, "").await;
    let state = param(&query(&url), "state").unwrap();
    back(app, provider, &state).await
}

#[renox::test]
async fn the_redirect_carries_state_and_a_pkce_challenge() {
    let app = app().await;
    let url = start(&app, "google", "").await;
    assert!(
        url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"),
        "{url}"
    );
    let pairs = query(&url);
    assert_eq!(param(&pairs, "response_type").unwrap(), "code");
    assert_eq!(param(&pairs, "client_id").unwrap(), "google-id");
    assert_eq!(param(&pairs, "scope").unwrap(), "openid email profile");
    assert_eq!(param(&pairs, "prompt").unwrap(), "select_account");
    assert_eq!(param(&pairs, "code_challenge_method").unwrap(), "S256");
    let redirect_uri = param(&pairs, "redirect_uri").unwrap();
    assert!(
        redirect_uri.ends_with("/auth/google/callback"),
        "{redirect_uri}"
    );
    assert!(redirect_uri.starts_with("http"), "absolute: {redirect_uri}");
    let state = param(&pairs, "state").unwrap();
    assert!(state.len() >= 43, "{state}");
    let challenge = param(&pairs, "code_challenge").unwrap();
    // The client secret never goes to the browser.
    assert!(!url.contains("google-secret"));

    let http = google_says(&app, "g-1", "nia@example.com", true);
    back(&app, "google", &state).await.assert_redirect("/");
    // The code was exchanged with the verifier whose SHA-256 is the challenge.
    let sent = http.sent();
    let exchange = &sent[0];
    assert_eq!(exchange.method, "POST");
    let form = query(&format!("?{}", exchange.body));
    assert_eq!(param(&form, "grant_type").unwrap(), "authorization_code");
    assert_eq!(param(&form, "code").unwrap(), "the-code");
    assert_eq!(param(&form, "client_secret").unwrap(), "google-secret");
    assert_eq!(param(&form, "redirect_uri").unwrap(), redirect_uri);
    let verifier = param(&form, "code_verifier").unwrap();
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        challenge
    );
    assert_eq!(sent[1].header("authorization"), Some("Bearer google-token"));
}

#[renox::test]
async fn a_first_sign_in_makes_an_account_without_a_password() {
    let app = app().await;
    app.fake_events();
    let http = google_says(&app, "g-1", "Nia@Example.com", true);
    // The second time, Google gives a new address for the same person.
    http.on(
        GOOGLE_USER,
        FakeResponse::json(
            200,
            json!({ "sub": "g-1", "email": "nia@new.example", "email_verified": true }),
        ),
    );
    sign_in_with(&app, "google").await.assert_redirect("/");

    let nia = User::find_by_email(app.db(), "nia@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(nia.name, "Nia Example");
    assert!(!nia.has_password());
    assert!(nia.email_verified_at.is_some());
    app.assert_authenticated(Some(&nia));
    let link = OAuthAccount::find_linked(app.db(), "google", "g-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.user_id, nia.id);
    assert_eq!(link.email.as_deref(), Some("Nia@Example.com"));
    assert_eq!(app.emitted::<Registered>().len(), 1);
    assert_eq!(app.emitted::<AccountLinked>().len(), 1);
    assert_eq!(app.emitted::<LoggedIn>().len(), 1);
    let logged_in = app.emitted::<LoggedInWith>();
    assert_eq!(logged_in.len(), 1);
    assert!(!logged_in[0].second_step);

    // The account page lists it, and says there's no password.
    app.get("/account")
        .await
        .assert_see("Linked accounts")
        .assert_see("Set a password")
        .assert_see("Your only login");

    // Next time, the same Google account logs in the same user, even with a
    // new address at Google.
    app.logout();
    sign_in_with(&app, "google").await.assert_redirect("/");
    app.assert_authenticated(Some(&nia));
    app.assert_database_count("users", 1).await;
    let link = OAuthAccount::find_linked(app.db(), "google", "g-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.email.as_deref(), Some("nia@new.example"));
}

#[renox::test]
async fn sign_ins_are_recorded_in_the_activity_log() {
    let app = app().await;
    google_says(&app, "g-1", "nia@example.com", true);
    sign_in_with(&app, "google").await.assert_redirect("/");
    app.assert_database_has("audit_logs", &[("action", &"oauth.linked")])
        .await;
    app.assert_database_has("audit_logs", &[("action", &"oauth.login")])
        .await;
}

#[renox::test]
async fn a_verified_address_links_to_its_existing_account() {
    let app = app().await;
    let mut ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    ana.email_verified_at = Some(renox::db::now());
    ana.save(app.db()).await.unwrap();
    google_says(&app, "g-ana", "ANA@example.com", true);
    sign_in_with(&app, "google").await.assert_redirect("/");
    app.assert_authenticated(Some(&ana));
    assert!(
        OAuthAccount::of_user_at(app.db(), ana.id, "google")
            .await
            .unwrap()
            .is_some()
    );
    app.assert_database_count("users", 1).await;
}

#[renox::test]
async fn an_unverified_address_never_links_or_makes_an_account() {
    let app = app().await;
    let mut ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    ana.email_verified_at = Some(renox::db::now());
    ana.save(app.db()).await.unwrap();
    // Someone typed Ana's address into their provider account, unverified.
    google_says(&app, "g-mallory", "ana@example.com", false);
    sign_in_with(&app, "google").await.assert_redirect("/login");
    app.assert_guest();
    app.get("/login").await.assert_see("verified email address");
    app.assert_database_count("oauth_accounts", 0).await;

    // Nor a new account from an unverified one.
    let other = app_with(Auth::new()).await;
    google_says(&other, "g-1", "new@example.com", false);
    sign_in_with(&other, "google")
        .await
        .assert_redirect("/login");
    other.assert_database_count("users", 0).await;
}

#[renox::test]
async fn an_account_whose_own_address_is_unverified_is_not_linked() {
    // Someone may have registered with this person's address and a password
    // of their own: linking would let them share the account.
    let app = app().await;
    User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    google_says(&app, "g-ana", "ana@example.com", true);
    sign_in_with(&app, "google").await.assert_redirect("/login");
    app.assert_guest();
    app.get("/login")
        .await
        .assert_see("Log in with your password, then link Google");
    app.assert_database_count("oauth_accounts", 0).await;
}

#[renox::test]
async fn closed_registration_makes_no_account() {
    let app = app_with(Auth::new().without_registration()).await;
    google_says(&app, "g-1", "nia@example.com", true);
    sign_in_with(&app, "google").await.assert_redirect("/login");
    app.assert_database_count("users", 0).await;
    app.get("/login").await.assert_see("No account uses");
}

#[renox::test]
async fn a_wrong_or_missing_state_is_refused_before_any_call() {
    let app = app().await;
    let http = google_says(&app, "g-1", "nia@example.com", true);
    // Never started in this session.
    back(&app, "google", "made-up")
        .await
        .assert_redirect("/login");
    let url = start(&app, "google", "").await;
    let state = param(&query(&url), "state").unwrap();
    // A wrong one uses up the started sign-in: the right one fails after it.
    back(&app, "google", "made-up")
        .await
        .assert_redirect("/login");
    back(&app, "google", &state).await.assert_redirect("/login");
    http.assert_sent_count(0);
    app.assert_guest();
    app.get("/login").await.assert_see("expired");
}

#[renox::test]
async fn state_is_single_use_and_bound_to_the_session() {
    let app = app().await;
    let http = google_says(&app, "g-1", "nia@example.com", true);
    let url = start(&app, "google", "").await;
    let state = param(&query(&url), "state").unwrap();
    // Another browser (a fresh session) brings the state back: refused.
    let mine = app.session_cookie();
    app.logout();
    back(&app, "google", &state).await.assert_redirect("/login");
    app.assert_guest();
    // The browser that started it succeeds, once.
    app.use_session_cookie(mine);
    back(&app, "google", &state).await.assert_redirect("/");
    let calls = http.sent().len();
    app.post("/logout", &[]).await;
    back(&app, "google", &state).await.assert_redirect("/login");
    app.assert_guest();
    assert_eq!(http.sent().len(), calls, "no second exchange");
}

#[renox::test]
async fn a_sign_in_expires_after_ten_minutes() {
    let app = app().await;
    google_says(&app, "g-1", "nia@example.com", true);
    let url = start(&app, "google", "").await;
    let state = param(&query(&url), "state").unwrap();
    app.travel(Duration::from_secs(11 * 60));
    back(&app, "google", &state).await.assert_redirect("/login");
    app.assert_guest();
}

#[renox::test]
async fn cancelling_or_a_failed_exchange_logs_nobody_in() {
    let app = app().await;
    let url = start(&app, "google", "").await;
    let state = param(&query(&url), "state").unwrap();
    app.get(&format!(
        "/auth/google/callback?error=access_denied&state={state}"
    ))
    .await
    .assert_redirect("/login");
    app.get("/login").await.assert_see("was cancelled");

    let http = app.fake_http();
    http.on(
        GOOGLE_TOKEN,
        FakeResponse::json(400, json!({ "error": "invalid_grant" })),
    );
    sign_in_with(&app, "google").await.assert_redirect("/login");
    app.assert_guest();
    app.assert_database_count("users", 0).await;
}

#[renox::test]
async fn github_gives_the_verified_primary_address() {
    let app = app().await;
    let http = app.fake_http();
    http.on(
        "POST https://github.com/login/oauth/access_token",
        FakeResponse::json(
            200,
            json!({ "access_token": "gh-token", "scope": "read:user,user:email" }),
        ),
    );
    http.on(
        "https://api.github.com/user",
        FakeResponse::json(
            200,
            json!({ "id": 42, "login": "nia", "name": null, "email": "public@example.com", "avatar_url": "https://example.com/a.png" }),
        ),
    );
    http.on(
        "https://api.github.com/user/emails",
        FakeResponse::json(
            200,
            json!([
                { "email": "old@example.com", "primary": false, "verified": true },
                { "email": "nia@example.com", "primary": true, "verified": true },
            ]),
        ),
    );
    sign_in_with(&app, "github").await.assert_redirect("/");
    let nia = User::find_by_email(app.db(), "nia@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(nia.name, "nia", "the login when there is no name");
    assert!(
        OAuthAccount::find_linked(app.db(), "github", "42")
            .await
            .unwrap()
            .is_some()
    );
    http.assert_sent(|r| {
        r.url.ends_with("/access_token") && r.header("accept") == Some("application/json")
    });
}

#[renox::test]
async fn github_errors_answered_with_200_are_errors() {
    let app = app().await;
    let http = app.fake_http();
    http.on(
        "POST https://github.com/login/oauth/access_token",
        FakeResponse::json(200, json!({ "error": "bad_verification_code" })),
    );
    sign_in_with(&app, "github").await.assert_redirect("/login");
    app.assert_guest();
}

/// Asks users named "Guarded" for a second step.
struct Guard;

impl Module for Guard {
    fn name(&self) -> &'static str {
        "guard"
    }

    fn register(&self, app: &mut Registry) {
        app.second_factor("guard.challenge", |user, _state| async move {
            Ok(user.name == "Guarded")
        });
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/guard", || async { "the challenge" })
            .name("guard.challenge")
    }
}

#[renox::test]
async fn the_second_login_step_still_applies() {
    let app = TestApp::new(App::new().module(Auth::new()).module(Guard).module(oauth())).await;
    app.fake_events();
    let mut guarded = User::register(app.db(), "Guarded", "g@example.com", PASSWORD)
        .await
        .unwrap();
    guarded.email_verified_at = Some(renox::db::now());
    guarded.save(app.db()).await.unwrap();
    google_says(&app, "g-1", "g@example.com", true);
    sign_in_with(&app, "google").await.assert_redirect("/guard");
    app.assert_guest();
    // The login waits in the session for the challenge.
    app.assert_session_has("_auth_pending");
    let events = app.emitted::<LoggedInWith>();
    assert!(events[0].second_step);
    assert!(app.emitted::<LoggedIn>().is_empty());
}

#[renox::test]
async fn a_logged_in_user_links_a_provider_from_the_account_page() {
    let app = app().await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    app.acting_as(&ana);
    app.fake_events();
    app.get("/account")
        .await
        .assert_see("Linked accounts")
        .assert_see("href=\"/auth/google/redirect\"");
    // Linking needs no matching address: the user proved both sides.
    let http = google_says(&app, "g-ana", "ana.personal@example.com", false);
    // The second time, another Google account.
    http.on(
        GOOGLE_USER,
        FakeResponse::json(200, json!({ "sub": "g-other", "email": "x@example.com" })),
    );
    sign_in_with(&app, "google")
        .await
        .assert_redirect("/account");
    assert_eq!(
        OAuthAccount::find_linked(app.db(), "google", "g-ana")
            .await
            .unwrap()
            .unwrap()
            .user_id,
        ana.id
    );
    assert_eq!(app.emitted::<AccountLinked>().len(), 1);
    app.get("/account")
        .await
        .assert_see("ana.personal@example.com")
        .assert_see("Unlink");

    // A second Google account for the same user: refused.
    sign_in_with(&app, "google")
        .await
        .assert_redirect("/account");
    app.assert_database_count("oauth_accounts", 1).await;
}

#[renox::test]
async fn a_provider_account_links_to_one_user_only() {
    let app = app().await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    let ben = User::register(app.db(), "Ben", "ben@example.com", PASSWORD)
        .await
        .unwrap();
    google_says(&app, "g-shared", "ana@example.com", true);
    app.acting_as(&ana);
    sign_in_with(&app, "google")
        .await
        .assert_redirect("/account");
    app.acting_as(&ben);
    sign_in_with(&app, "google")
        .await
        .assert_redirect("/account");
    app.get("/account")
        .await
        .assert_see("linked to another user");
    let link = OAuthAccount::find_linked(app.db(), "google", "g-shared")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(link.user_id, ana.id);
}

#[renox::test]
async fn unlinking_never_leaves_a_user_without_a_way_to_log_in() {
    let app = app().await;
    app.fake_events();
    google_says(&app, "g-1", "nia@example.com", true);
    sign_in_with(&app, "google").await.assert_redirect("/");
    let nia = User::find_by_email(app.db(), "nia@example.com")
        .await
        .unwrap()
        .unwrap();
    // Google is Nia's only way in (no password): refused.
    app.delete("/auth/google").await.assert_redirect("/account");
    app.assert_database_count("oauth_accounts", 1).await;
    assert!(app.emitted::<AccountUnlinked>().is_empty());

    // With GitHub linked too, one of them can go.
    let http = app.fake_http();
    http.on(
        "POST https://github.com/login/oauth/access_token",
        FakeResponse::json(200, json!({ "access_token": "gh-token" })),
    );
    http.on(
        "https://api.github.com/user",
        FakeResponse::json(200, json!({ "id": 7, "login": "nia" })),
    );
    http.on(
        "https://api.github.com/user/emails",
        FakeResponse::json(200, json!([])),
    );
    sign_in_with(&app, "github")
        .await
        .assert_redirect("/account");
    app.delete("/auth/google").await.assert_redirect("/account");
    assert!(
        OAuthAccount::of_user_at(app.db(), nia.id, "google")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(app.emitted::<AccountUnlinked>().len(), 1);
    // GitHub is the last one again.
    app.delete("/auth/github").await.assert_redirect("/account");
    app.assert_database_count("oauth_accounts", 1).await;

    // With a password, the last one can go too.
    let mut nia = User::find_or_404(app.db(), nia.id).await.unwrap();
    nia.set_password(app.db(), PASSWORD).await.unwrap();
    app.acting_as(&nia);
    app.delete("/auth/github").await.assert_redirect("/account");
    app.assert_database_count("oauth_accounts", 0).await;
}

#[renox::test]
async fn a_user_without_a_password_confirms_it_is_them_with_a_linked_provider() {
    let app = app().await;
    google_says(&app, "g-1", "nia@example.com", true);
    sign_in_with(&app, "google").await.assert_redirect("/");
    // Hours later, deleting the account asks who they are.
    app.travel(Duration::from_secs(4 * 60 * 60));
    let nia = User::find_by_email(app.db(), "nia@example.com")
        .await
        .unwrap()
        .unwrap();
    app.acting_as(&nia);
    app.delete("/account")
        .await
        .assert_redirect("/confirm-password");
    // The confirm page offers the linked provider only.
    app.get("/confirm-password")
        .await
        .assert_see("/auth/google/redirect?intent=confirm")
        .assert_dont_see("/auth/github/redirect");
    let url = start(&app, "google", "?intent=confirm").await;
    let state = param(&query(&url), "state").unwrap();
    back(&app, "google", &state)
        .await
        .assert_redirect("/account");
    app.delete("/account").await.assert_redirect("/");
    app.assert_database_count("users", 0).await;
    // Its provider accounts went with it.
    app.assert_database_count("oauth_accounts", 0).await;
}

#[renox::test]
async fn confirming_with_a_provider_account_that_is_not_yours_fails() {
    let app = app().await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    app.acting_as(&ana);
    google_says(&app, "g-someone", "someone@example.com", true);
    let url = start(&app, "google", "?intent=confirm").await;
    let state = param(&query(&url), "state").unwrap();
    back(&app, "google", &state)
        .await
        .assert_redirect("/confirm-password");
    app.assert_database_count("oauth_accounts", 0).await;
}

#[renox::test]
async fn the_login_and_register_pages_show_the_configured_providers() {
    let app = app().await;
    app.get("/login")
        .await
        .assert_see("Or continue with")
        .assert_see("href=\"/auth/google/redirect\"")
        .assert_see("href=\"/auth/github/redirect\"");
    app.get("/register")
        .await
        .assert_see("href=\"/auth/google/redirect\"");
    // A provider without credentials is off.
    app.get("/auth/gitlab/redirect").await.assert_not_found();
    let unset = TestApp::new(App::new().module(Auth::new()).module(OAuth::new().google())).await;
    unset
        .get("/login")
        .await
        .assert_ok()
        .assert_dont_see("Or continue with");
    unset.get("/auth/google/redirect").await.assert_not_found();
    let set = TestApp::with_config(
        App::new().module(Auth::new()).module(OAuth::new().google()),
        |config| {
            config
                .vars
                .insert("GOOGLE_CLIENT_ID".into(), "from-env".into());
            config
                .vars
                .insert("GOOGLE_CLIENT_SECRET".into(), "secret".into());
        },
    )
    .await;
    let url = start(&set, "google", "").await;
    assert_eq!(param(&query(&url), "client_id").unwrap(), "from-env");
}

#[renox::test]
async fn the_table_keeps_one_link_per_provider_account_and_per_user() {
    let app = app().await;
    let mut ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    let ben = User::register(app.db(), "Ben", "ben@example.com", PASSWORD)
        .await
        .unwrap();
    let link = |user_id, id: &str| OAuthAccount {
        user_id,
        provider: "google".into(),
        provider_user_id: id.into(),
        ..Default::default()
    };
    OAuthAccount::create(app.db(), link(ana.id, "g-1"))
        .await
        .unwrap();
    // The same Google account for another user, or a second one for Ana.
    assert!(
        OAuthAccount::create(app.db(), link(ben.id, "g-1"))
            .await
            .is_err()
    );
    assert!(
        OAuthAccount::create(app.db(), link(ana.id, "g-2"))
            .await
            .is_err()
    );
    OAuthAccount::create(app.db(), link(ben.id, "g-2"))
        .await
        .unwrap();
    // Deleting a user deletes their links (the foreign key cascades).
    ana.delete(app.db()).await.unwrap();
    app.assert_database_count("oauth_accounts", 1).await;
}

// ---------- #261: the callback's other refusals, and secrets kept out of Debug ----------

#[renox::test]
async fn a_callback_without_a_code_or_with_a_profile_without_an_id_logs_nobody_in() {
    let app = app().await;
    let url = start(&app, "google", "").await;
    let state = param(&query(&url), "state").unwrap();
    app.get(&format!("/auth/google/callback?code=&state={state}"))
        .await
        .assert_redirect("/login");
    app.get("/login")
        .await
        .assert_see("Google didn&#x27;t send a sign-in code. Try again.");

    // The provider answers, but its profile has no id.
    google_says(&app, "", "nia@example.com", true);
    sign_in_with(&app, "google").await.assert_redirect("/login");
    app.assert_guest();
    app.assert_database_count("users", 0).await;
}

#[renox::test]
async fn a_token_answer_that_isnt_json_asks_to_try_again() {
    let app = app().await;
    app.fake_http().on(
        GOOGLE_TOKEN,
        FakeResponse::text(502, "<html>Bad gateway</html>"),
    );
    sign_in_with(&app, "google").await.assert_redirect("/login");
    app.get("/login").await.assert_see("Try again.");
    app.assert_guest();
}

#[renox::test]
async fn providers_from_config_and_their_debug_keep_secrets_out() {
    // From config: credentials from GITHUB_CLIENT_ID/_SECRET; without them
    // the provider isn't offered.
    let app = TestApp::with_config(
        App::new().module(Auth::new()).module(OAuth::new().github()),
        |c| {
            c.vars.insert("GITHUB_CLIENT_ID".into(), "gh-id".into());
            c.vars
                .insert("GITHUB_CLIENT_SECRET".into(), "gh-very-secret".into());
        },
    )
    .await;
    let url = start(&app, "github", "").await;
    assert!(
        url.starts_with("https://github.com/login/oauth/authorize"),
        "{url}"
    );
    assert_eq!(param(&query(&url), "client_id").as_deref(), Some("gh-id"));
    let unset = TestApp::new(App::new().module(Auth::new()).module(OAuth::new().github())).await;
    unset.get("/auth/github/redirect").await.assert_not_found();

    let shown = format!("{:?}", oauth());
    assert!(
        !shown.contains("google-secret") && !shown.contains("github-secret"),
        "{shown}"
    );
}

// ---------- #261: the rest of the linking rules, unlinking, GitHub, the section ----------

#[renox::test]
async fn a_verified_address_whose_user_has_another_account_there_is_refused() {
    let app = app().await;
    // Nia signs up with Google account g-1.
    let http = google_says(&app, "g-1", "nia@example.com", true);
    // Another Google account with the same verified address.
    http.on(
        GOOGLE_USER,
        FakeResponse::json(
            200,
            json!({ "sub": "g-2", "email": "nia@example.com", "email_verified": true }),
        ),
    );
    sign_in_with(&app, "google").await.assert_redirect("/");
    app.logout();
    sign_in_with(&app, "google").await.assert_redirect("/login");
    app.assert_guest();
    app.get("/login")
        .await
        .assert_see("This account is linked to another Google account.");
    app.assert_database_count("oauth_accounts", 1).await;
}

#[renox::test]
async fn unlinking_over_htmx_and_what_was_not_linked() {
    let app = app().await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    app.acting_as(&ana);
    google_says(&app, "g-ana", "ana@example.com", true);
    sign_in_with(&app, "google")
        .await
        .assert_redirect("/account");
    // Nothing linked at GitHub: back to the account page, nothing changes.
    app.delete("/auth/github").await.assert_redirect("/account");
    app.htmx()
        .delete("/auth/google")
        .await
        .assert_hx_redirect("/account");
    app.assert_database_count("oauth_accounts", 0).await;
    // Recorded in the activity log.
    let unlinked: i64 = renox::db::sql("SELECT COUNT(*) FROM audit_logs WHERE action = ?")
        .bind("oauth.unlinked")
        .scalar(app.db())
        .await
        .unwrap();
    assert_eq!(unlinked, 1);

    // Without the Audit module (no audit_logs table) the same works.
    let app = TestApp::new(App::new().module(Auth::new().account()).module(oauth())).await;
    let bo = User::register(app.db(), "Bo", "bo@example.com", PASSWORD)
        .await
        .unwrap();
    app.acting_as(&bo);
    google_says(&app, "g-bo", "bo@example.com", true);
    sign_in_with(&app, "google")
        .await
        .assert_redirect("/account");
    app.delete("/auth/google").await.assert_redirect("/account");
    app.assert_database_count("oauth_accounts", 0).await;
}

#[renox::test]
async fn a_public_github_address_that_isnt_verified_doesnt_sign_in() {
    let app = app().await;
    let http = app.fake_http();
    http.on(
        "POST https://github.com/login/oauth/access_token",
        FakeResponse::json(200, json!({ "access_token": "gh-token" })),
    );
    http.on(
        "https://api.github.com/user",
        FakeResponse::json(
            200,
            json!({ "id": 9, "login": "pat", "email": "pat@example.com" }),
        ),
    );
    // The address list can't be read (a private scope): the public address
    // is all there is, unverified.
    http.on(
        "https://api.github.com/user/emails",
        FakeResponse::json(404, json!({ "message": "Not Found" })),
    );
    sign_in_with(&app, "github").await.assert_redirect("/login");
    app.assert_guest();
    app.get("/login")
        .await
        .assert_see("GitHub didn&#x27;t share a verified email address");
    app.assert_database_count("users", 0).await;
}

#[renox::test]
async fn the_account_page_lists_providers_the_app_no_longer_offers() {
    let app = app().await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    OAuthAccount::create(
        app.db(),
        OAuthAccount {
            user_id: ana.id,
            provider: "gitlab".into(),
            provider_user_id: "gl-1".into(),
            email: Some("ana@gitlab.example".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    app.acting_as(&ana);
    app.get("/account")
        .await
        .assert_see("gitlab")
        .assert_see("ana@gitlab.example");
    // It can still be unlinked.
    app.delete("/auth/gitlab").await.assert_redirect("/account");
    app.assert_database_count("oauth_accounts", 0).await;
}
