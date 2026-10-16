//! #42: passwordless login (magic links and one-time codes), and the
//! `OneTimeCode` primitive they are built on.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use renox::audit::{self, Audit};
use renox::auth::events::{LockedOut, LoggedIn, LoginFailed};
use renox::auth::{CodeCheck, LoginCodes, OneTimeCode, Recipient};
use renox::db::Migration;
use renox::prelude::*;
use renox::testing::TestApp;

const PASSWORD: &str = "a long password 12";

type Outbox = Arc<Mutex<Vec<(Option<String>, serde_json::Value)>>>;

/// Asks users named "Guarded" for a second step after any login.
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
            .get("/challenge", || async { "challenge" })
            .name("guard.challenge")
            .guest_only()
    }
}

fn base(auth: Auth) -> (App, Outbox) {
    let outbox: Outbox = Arc::default();
    let sent = outbox.clone();
    let app = App::new()
        .migrations(&[Migration::new(
            "20300101000000_add_phone_to_users",
            "ALTER TABLE users ADD COLUMN phone TEXT",
            None,
        )])
        .channel("sms", move |to: Recipient, message, _| {
            let sent = sent.clone();
            async move {
                sent.lock().unwrap().push((to.address("sms"), message));
                Ok(())
            }
        })
        .channel("whatsapp", |_to: Recipient, _message, _| async { Ok(()) })
        .module(auth)
        .module(Guard);
    (app, outbox)
}

fn codes() -> LoginCodes {
    LoginCodes::new()
        .channel("sms")
        .max_attempts(3)
        .identify(|state, input: String| async move {
            Ok(renox::db::sql("SELECT * FROM users WHERE phone = ?")
                .bind(input.trim())
                .fetch_optional_as::<User>(&state.db)
                .await?)
        })
}

async fn app_with(auth: Auth) -> (TestApp, Outbox, User) {
    let (app, outbox) = base(auth);
    let app = TestApp::new(app).await;
    let mut ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
        .await
        .unwrap();
    ana.set(app.db(), "phone", "+15550111").await.unwrap();
    (app, outbox, ana)
}

/// The path of a link from the mail (`/login/link/<token>`).
fn path_of(link: &str) -> &str {
    &link[link.find("/login/link/").expect("a login link")..]
}

async fn ask_for_link(app: &TestApp, email: &str) {
    app.post("/login/link", &[("email", email)])
        .await
        .assert_redirect("/login/link");
}

async fn ask_for_code(app: &TestApp, identifier: &str) -> renox::testing::TestResponse {
    app.post("/login/code", &[("identifier", identifier)]).await
}

fn forget_session(app: &TestApp) -> Option<String> {
    let cookie = app.session_cookie();
    app.use_session_cookie(None);
    cookie
}

// --- Magic links -------------------------------------------------------------

#[renox::test]
async fn a_magic_link_logs_in_once() {
    let (app, _, ana) = app_with(Auth::new().magic_link()).await;
    ask_for_link(&app, "Ana@Example.com").await;
    let link = app.last_login_link().await.expect("a link was mailed");
    assert!(link.starts_with("http"));
    let mail = &app.sent_mail()[0];
    assert_eq!(mail.to[0], "ana@example.com");
    assert!(mail.text.contains(&link), "the text part has the link");
    let path = path_of(&link).to_owned();

    // Opening the link (a mail scanner does) neither logs in nor burns it.
    for _ in 0..2 {
        app.get(&path)
            .await
            .assert_ok()
            .assert_see("ana@example.com")
            .assert_see("Log in");
        app.assert_guest();
    }

    app.post(&path, &[]).await.assert_redirect("/");
    app.assert_authenticated(Some(&ana));

    // The token is used up.
    app.logout();
    app.get(&path)
        .await
        .assert_ok()
        .assert_see("invalid or has expired");
    app.post(&path, &[]).await.assert_redirect("/login/link");
    app.assert_guest();
}

#[renox::test]
async fn a_magic_link_expires() {
    let (app, _, _) = app_with(Auth::new().magic_link()).await;
    ask_for_link(&app, "ana@example.com").await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    app.travel(Duration::from_secs(16 * 60));
    app.post(&path, &[]).await.assert_redirect("/login/link");
    app.assert_guest();
    app.travel_back();

    let (app, _, _) = app_with(Auth::new().magic_link_ttl(Duration::from_secs(3600))).await;
    ask_for_link(&app, "ana@example.com").await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    app.travel(Duration::from_secs(45 * 60));
    app.post(&path, &[]).await.assert_redirect("/");
}

#[renox::test]
async fn a_new_link_replaces_the_old_one() {
    let (app, _, _) = app_with(Auth::new().magic_link()).await;
    ask_for_link(&app, "ana@example.com").await;
    let first = path_of(&app.last_login_link().await.unwrap()).to_owned();
    app.travel(Duration::from_secs(1));
    ask_for_link(&app, "ana@example.com").await;
    let second = path_of(&app.last_login_link().await.unwrap()).to_owned();
    assert_ne!(first, second);
    app.post(&first, &[]).await.assert_redirect("/login/link");
    app.assert_guest();
    app.post(&second, &[]).await.assert_redirect("/");
}

#[renox::test]
async fn a_link_works_in_any_browser_unless_same_browser_is_on() {
    let (app, _, ana) = app_with(Auth::new().magic_link()).await;
    ask_for_link(&app, "ana@example.com").await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    forget_session(&app); // the mail is opened on another device
    app.post(&path, &[]).await.assert_redirect("/");
    app.assert_authenticated(Some(&ana));

    let (app, _, ana) = app_with(Auth::new().magic_link().same_browser()).await;
    ask_for_link(&app, "ana@example.com").await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    let asking = forget_session(&app);
    // Another browser: refused, and the link is not burned by the try.
    app.get(&path).await.assert_see("invalid or has expired");
    app.post(&path, &[]).await.assert_redirect("/login/link");
    app.assert_guest();
    app.use_session_cookie(asking);
    app.get(&path).await.assert_see("ana@example.com");
    app.post(&path, &[]).await.assert_redirect("/");
    app.assert_authenticated(Some(&ana));
}

#[renox::test]
async fn unknown_addresses_get_the_same_answer_and_no_mail() {
    let (app, _, _) = app_with(Auth::new().magic_link()).await;
    ask_for_link(&app, "ana@example.com").await;
    let known = app.get("/login/link").await;
    forget_session(&app);
    ask_for_link(&app, "nobody@example.com").await;
    let unknown = app.get("/login/link").await;
    assert_eq!(known.status, unknown.status);
    assert!(known.text().contains("a link is on its way"));
    assert!(unknown.text().contains("a link is on its way"));
    app.run_jobs().await;
    assert_eq!(app.sent_mail().len(), 1, "only the account got a mail");
    // Both did the same work in the database.
    app.assert_database_count("one_time_codes", 2).await;
}

#[renox::test]
async fn sending_is_limited_per_address() {
    let (app, _, _) = app_with(Auth::new().magic_link()).await;
    for _ in 0..8 {
        ask_for_link(&app, "ana@example.com").await;
    }
    app.run_jobs().await;
    assert_eq!(app.sent_mail().len(), 5);
}

#[renox::test]
async fn links_go_through_the_second_login_step() {
    let (app, _, _) = app_with(Auth::new().magic_link()).await;
    let guarded = User::register(app.db(), "Guarded", "guarded@example.com", PASSWORD)
        .await
        .unwrap();
    ask_for_link(&app, &guarded.email).await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    app.post(&path, &[]).await.assert_redirect("/challenge");
    app.assert_guest();
    app.assert_session_has("_auth_pending");
}

#[renox::test]
async fn a_link_for_a_locked_out_address_is_refused_and_not_burned() {
    let (app, _, _) = app_with(Auth::new().magic_link()).await;
    ask_for_link(&app, "ana@example.com").await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    // Five wrong passwords a minute lock the address out.
    for _ in 0..5 {
        app.htmx()
            .post(
                "/login",
                &[("email", "ana@example.com"), ("password", "wrong")],
            )
            .await
            .assert_invalid("email");
    }
    app.fake_events();
    app.htmx().post(&path, &[]).await.assert_invalid("email");
    app.assert_guest();
    app.assert_emitted::<LockedOut>(|e| e.email == "ana@example.com");
}

#[renox::test]
async fn magic_links_register_unknown_addresses_only_when_asked_to() {
    let (app, _, _) = app_with(Auth::new().magic_link()).await;
    ask_for_link(&app, "new@example.com").await;
    assert!(app.last_login_link().await.is_none());
    app.assert_database_missing("users", &[("email", &"new@example.com")])
        .await;

    let (app, _, _) = app_with(
        Auth::new()
            .magic_link()
            .register_on_first_use()
            .on_registered(|user, form, _| async move {
                assert_eq!(form.get("via"), "magic_link");
                assert_eq!(user.email, "new@example.com");
                Ok(())
            }),
    )
    .await;
    ask_for_link(&app, "New@Example.com").await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    // Nothing is created by asking, nor by opening the link.
    app.get(&path).await.assert_see("new@example.com");
    app.assert_database_missing("users", &[("email", &"new@example.com")])
        .await;
    app.post(&path, &[]).await.assert_redirect("/");
    let new = User::find_by_email(app.db(), "new@example.com")
        .await
        .unwrap()
        .unwrap();
    assert!(!new.has_password());
    assert!(new.email_verified_at.is_some());
    app.assert_authenticated(Some(&new));
}

#[renox::test]
async fn a_link_verifies_the_address_and_says_how_the_user_logged_in() {
    let (app, _, _) = app_with(Auth::new().magic_link().verify_email()).await;
    let ben = User::register(app.db(), "Ben", "ben@example.com", PASSWORD)
        .await
        .unwrap();
    assert!(ben.email_verified_at.is_none());
    app.fake_events();
    ask_for_link(&app, "ben@example.com").await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    app.post(&path, &[]).await.assert_redirect("/");
    let ben = User::find_or_404(app.db(), ben.id).await.unwrap();
    assert!(ben.email_verified_at.is_some());
    let logins = app.emitted::<LoggedIn>();
    assert_eq!(logins.len(), 1);
    assert_eq!(logins[0].via, "magic_link");
}

// --- Login codes -------------------------------------------------------------

#[renox::test]
async fn a_code_sent_by_sms_logs_in() {
    let (app, outbox, ana) = app_with(Auth::new().login_codes(codes())).await;
    app.fake_events();
    ask_for_code(&app, "+15550111")
        .await
        .assert_redirect("/login/code/verify");
    app.get("/login/code/verify")
        .await
        .assert_ok()
        .assert_see("SMS");
    let code = app
        .last_login_code_on("sms")
        .await
        .expect("a code was sent");
    assert_eq!(code.len(), 6);
    {
        let sent = outbox.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0.as_deref(), Some("+15550111"));
        assert_eq!(sent[0].1["code"], code.as_str());
        assert!(sent[0].1["text"].as_str().unwrap().contains(&code));
    }
    app.post(
        "/login/code/verify",
        &[("code", &format!("{} {}", &code[..3], &code[3..]))],
    )
    .await
    .assert_redirect("/");
    app.assert_authenticated(Some(&ana));
    let logins = app.emitted::<LoggedIn>();
    assert_eq!(logins[0].via, "code");

    // Used up.
    app.logout();
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/login/code");
    app.assert_guest();
}

#[renox::test]
async fn wrong_codes_count_and_the_last_one_burns_the_code() {
    let (app, _, ana) = app_with(Auth::new().login_codes(codes())).await;
    app.fake_events();
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..2 {
        app.htmx()
            .post("/login/code/verify", &[("code", wrong)])
            .await
            .assert_invalid("code");
    }
    // The third wrong try (max_attempts 3) burns the code.
    app.post("/login/code/verify", &[("code", wrong)])
        .await
        .assert_redirect("/login/code");
    assert_eq!(app.emitted::<LoginFailed>().len(), 3);
    // Even the right code is refused now: there is nothing to check it
    // against, and a new one must be requested.
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/login/code");
    app.assert_guest();
    ask_for_code(&app, "+15550111").await;
    let newest = app.last_login_code().await.unwrap();
    app.post("/login/code/verify", &[("code", &newest)])
        .await
        .assert_redirect("/");
    app.assert_authenticated(Some(&ana));
}

#[renox::test]
async fn a_correct_code_after_the_limit_is_refused_even_with_a_pending_request() {
    // Two browsers share one pending request by sharing a session cookie
    // copy: the burned code stays refused wherever the request is still known.
    let (app, _, _) = app_with(Auth::new().login_codes(codes())).await;
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    let cookie = app.session_cookie();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..3 {
        app.post("/login/code/verify", &[("code", wrong)]).await;
    }
    // The first browser's session forgot the request; a copy of the earlier
    // session (same cookie) still has it.
    app.use_session_cookie(cookie);
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/login/code");
    app.assert_guest();
}

#[renox::test]
async fn a_code_expires() {
    let (app, _, _) = app_with(Auth::new().login_codes(codes())).await;
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    app.travel(Duration::from_secs(5 * 60 + 5));
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/login/code");
    app.assert_guest();
}

#[renox::test]
async fn a_code_works_only_in_the_browser_that_asked_for_it() {
    let (app, _, ana) = app_with(Auth::new().login_codes(codes())).await;
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    let asking = forget_session(&app);
    // Someone who got the SMS but not the session: they can't even start.
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/login/code");
    // Nor by asking for a code of their own to get a session.
    ask_for_code(&app, "+15550999").await;
    app.htmx()
        .post("/login/code/verify", &[("code", &code)])
        .await
        .assert_invalid("code");
    app.assert_guest();
    // The real browser is unharmed by those tries.
    app.use_session_cookie(asking);
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/");
    app.assert_authenticated(Some(&ana));
}

#[renox::test]
async fn unknown_identifiers_behave_like_known_ones() {
    let (app, outbox, _) = app_with(Auth::new().login_codes(codes())).await;
    let known = ask_for_code(&app, "+15550111").await;
    forget_session(&app);
    let unknown = ask_for_code(&app, "+15559999").await;
    assert_eq!(known.status, unknown.status);
    assert_eq!(known.header("location"), unknown.header("location"));
    app.run_jobs().await;
    assert_eq!(outbox.lock().unwrap().len(), 1, "no SMS for a stranger");
    app.assert_database_count("one_time_codes", 2).await;
    // Wrong codes, the burn and the messages are the same for a stranger.
    let wrong = "000000";
    for _ in 0..2 {
        app.htmx()
            .post("/login/code/verify", &[("code", wrong)])
            .await
            .assert_invalid("code");
    }
    app.post("/login/code/verify", &[("code", wrong)])
        .await
        .assert_redirect("/login/code");
    app.get("/login/code")
        .await
        .assert_see("Too many wrong codes");
}

#[renox::test]
async fn wrong_codes_lock_the_identifier_out() {
    let (app, _, _) = app_with(Auth::new().login_codes(codes().max_attempts(20))).await;
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    app.fake_events();
    for _ in 0..5 {
        app.htmx()
            .post("/login/code/verify", &[("code", wrong)])
            .await
            .assert_invalid("code");
    }
    // Five wrong tries a minute is the login throttle's limit.
    app.htmx()
        .post("/login/code/verify", &[("code", &code)])
        .await
        .assert_invalid("code");
    app.assert_guest();
    app.assert_emitted::<LockedOut>(|e| e.email == "+15550111");
    // Asking for yet another code is refused too while locked out.
    app.htmx()
        .post("/login/code", &[("identifier", "+15550111")])
        .await
        .assert_invalid("identifier");
}

#[renox::test]
async fn sending_codes_is_limited() {
    let (app, outbox, _) = app_with(Auth::new().login_codes(codes())).await;
    for _ in 0..8 {
        ask_for_code(&app, "+15550111").await;
    }
    app.run_jobs().await;
    assert_eq!(outbox.lock().unwrap().len(), 5);
}

#[renox::test]
async fn codes_go_through_the_second_login_step() {
    let (app, _, mut ana) = app_with(Auth::new().login_codes(codes())).await;
    ana.name = "Guarded".into();
    ana.save(app.db()).await.unwrap();
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/challenge");
    app.assert_guest();
}

#[renox::test]
async fn mail_is_the_default_channel_and_finds_accounts_by_email() {
    let (app, _, ana) = app_with(Auth::new().login_codes(LoginCodes::new())).await;
    ask_for_code(&app, "ana@example.com").await;
    let code = app.last_login_code().await.unwrap();
    let mail = &app.sent_mail()[0];
    assert_eq!(mail.to[0], "ana@example.com");
    assert!(mail.text.contains(&code));
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/");
    app.assert_authenticated(Some(&ana));
}

#[renox::test]
async fn codes_register_unknown_addresses_when_asked_to() {
    let (app, _, _) = app_with(Auth::new().login_codes(LoginCodes::new())).await;
    ask_for_code(&app, "new@example.com").await;
    assert!(app.last_login_code().await.is_none());

    let (app, _, _) = app_with(
        Auth::new()
            .register_on_first_use()
            .login_codes(LoginCodes::new()),
    )
    .await;
    ask_for_code(&app, "new@example.com").await;
    let code = app.last_login_code().await.unwrap();
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/");
    let new = User::find_by_email(app.db(), "new@example.com")
        .await
        .unwrap()
        .unwrap();
    assert!(!new.has_password());
    app.assert_authenticated(Some(&new));
}

#[renox::test]
async fn several_channels_let_the_user_choose_and_send_another_way() {
    let (app, outbox, _) =
        app_with(Auth::new().login_codes(codes().channels(["sms", "whatsapp", "mail"]))).await;
    app.get("/login/code")
        .await
        .assert_see("SMS")
        .assert_see("WhatsApp")
        .assert_see("Email");
    app.post(
        "/login/code",
        &[("identifier", "+15550111"), ("channel", "whatsapp")],
    )
    .await
    .assert_redirect("/login/code/verify");
    app.get("/login/code/verify")
        .await
        .assert_see("WhatsApp")
        .assert_see("Send it another way")
        .assert_see(r#"value="sms""#);
    assert!(app.last_login_code_on("whatsapp").await.is_some());
    // Send it another way.
    app.post("/login/code/resend", &[("channel", "sms")])
        .await
        .assert_redirect("/login/code/verify");
    let code = app.last_login_code_on("sms").await.unwrap();
    assert_eq!(outbox.lock().unwrap().len(), 1);
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/");
}

#[renox::test]
async fn the_app_can_deliver_codes_itself() {
    let delivered: Arc<Mutex<Vec<renox::auth::LoginCodeDelivery>>> = Arc::default();
    let seen = delivered.clone();
    let (app, outbox, ana) = app_with(Auth::new().login_codes(codes().deliver(
        move |_state, delivery| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(delivery);
                Ok(())
            }
        },
    )))
    .await;
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    assert!(
        outbox.lock().unwrap().is_empty(),
        "the hook replaces the channel"
    );
    let delivered = delivered.lock().unwrap();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].code, code);
    assert_eq!(delivered[0].channel, "sms");
    assert_eq!(delivered[0].address, "+15550111");
    assert_eq!(delivered[0].locale, "en");
    assert_eq!(delivered[0].user.as_ref().map(|u| u.id), Some(ana.id));
    assert_eq!(delivered[0].expires_in, Duration::from_secs(300));
}

#[renox::test]
async fn the_delivery_job_is_stored_encrypted() {
    let (app, _, _) = app_with(Auth::new().login_codes(codes())).await;
    ask_for_code(&app, "+15550111").await;
    let payload: String = renox::db::sql("SELECT payload FROM jobs")
        .scalar(app.db())
        .await
        .unwrap();
    assert!(payload.starts_with("enc:"), "{payload}");
    let code = app.last_login_code().await.unwrap();
    assert!(!payload.contains(&code));
}

#[renox::test]
async fn only_hashes_are_stored() {
    let (app, _, _) = app_with(Auth::new().magic_link().login_codes(codes())).await;
    ask_for_link(&app, "ana@example.com").await;
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    let link = app.last_login_link().await.unwrap();
    let secret = link.rsplit('.').next().unwrap().to_owned();
    let stored: Vec<String> = renox::db::sql("SELECT code FROM one_time_codes")
        .scalars(app.db())
        .await
        .unwrap();
    assert_eq!(stored.len(), 2);
    for hash in stored {
        assert_ne!(hash, code);
        assert!(!hash.contains(&secret));
        assert_eq!(hash.len(), 64);
    }
}

// --- Pages and routes -------------------------------------------------------

#[renox::test]
async fn the_login_page_lists_the_options_that_are_on() {
    let (app, _, _) = app_with(Auth::new()).await;
    app.get("/login")
        .await
        .assert_dont_see("data-passwordless")
        .assert_ok();
    app.get("/login/link").await.assert_not_found();
    app.get("/login/code").await.assert_not_found();

    let (app, _, _) = app_with(Auth::new().magic_link().login_codes(codes())).await;
    app.get("/login")
        .await
        .assert_see(r#"href="/login/link""#)
        .assert_see(r#"href="/login/code""#)
        .assert_see("Email me a login link")
        .assert_see("Log in with a code");
    app.get("/login/link")
        .await
        .assert_ok()
        .assert_see("Email me a link");
    app.get("/login/code")
        .await
        .assert_ok()
        .assert_see("Send me a code");
    app.assert_views_compile();
}

#[renox::test]
async fn the_routes_are_named() {
    let (app, _, _) = app_with(Auth::new().magic_link().login_codes(codes())).await;
    let state = app.state();
    for name in [
        "login.link.request",
        "login.link.send",
        "login.code.request",
        "login.code.send",
        "login.code.verify",
        "login.code.resend",
    ] {
        assert!(state.url(name, &[]).is_ok(), "{name}");
    }
    assert!(state.url("login.link.show", &[&"t"]).is_ok());
    assert!(state.url("login.link.consume", &[&"t"]).is_ok());
}

#[renox::test]
async fn logged_in_users_do_not_see_the_pages() {
    let (app, _, ana) = app_with(Auth::new().magic_link().login_codes(codes())).await;
    app.acting_as(&ana);
    app.get("/login/link").await.assert_redirect("/");
    app.get("/login/code").await.assert_redirect("/");
}

// --- The audit log -----------------------------------------------------------

#[renox::test]
async fn the_audit_log_records_passwordless_logins() {
    let (app, _, ana) = {
        let (app, outbox) = base(Auth::new().magic_link().login_codes(codes()));
        let app = TestApp::new(app.module(Audit)).await;
        let mut ana = User::register(app.db(), "Ana", "ana@example.com", PASSWORD)
            .await
            .unwrap();
        ana.set(app.db(), "phone", "+15550111").await.unwrap();
        (app, outbox, ana)
    };
    ask_for_link(&app, "ana@example.com").await;
    let path = path_of(&app.last_login_link().await.unwrap()).to_owned();
    app.post(&path, &[]).await.assert_redirect("/");
    app.logout();
    ask_for_code(&app, "+15550111").await;
    let code = app.last_login_code().await.unwrap();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    app.htmx()
        .post("/login/code/verify", &[("code", wrong)])
        .await
        .assert_invalid("code");
    app.post("/login/code/verify", &[("code", &code)])
        .await
        .assert_redirect("/");

    let entries = audit::for_user(app.db(), ana.id, 10).await.unwrap();
    let vias: Vec<_> = entries
        .iter()
        .filter(|e| e.action == "auth.login")
        .map(|e| e.data["via"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(vias, ["code", "magic_link"]);
    app.assert_database_has("audit_logs", &[("action", &"auth.login_failed")])
        .await;
}

#[renox::test]
async fn password_logins_say_so() {
    let (app, _, ana) = app_with(Auth::new().magic_link()).await;
    app.fake_events();
    app.post(
        "/login",
        &[("email", "ana@example.com"), ("password", PASSWORD)],
    )
    .await
    .assert_redirect("/");
    app.assert_authenticated(Some(&ana));
    assert_eq!(app.emitted::<LoggedIn>()[0].via, "password");
}

// --- OneTimeCode ---------------------------------------------------------------

#[renox::test]
async fn one_time_codes_are_single_use_and_limited() {
    let (app, _, _) = app_with(Auth::new()).await;
    let state = app.state();
    let ttl = Duration::from_secs(60);
    let code = OneTimeCode::issue(state, "payout", "42", ttl, 3)
        .await
        .unwrap();
    assert_eq!(code.len(), 6);
    let wrong = if code == "000000" { "111111" } else { "000000" };

    // Another subject or purpose has nothing to check against.
    assert_eq!(
        OneTimeCode::verify(state, "payout", "43", &code)
            .await
            .unwrap(),
        CodeCheck::Missing
    );
    assert_eq!(
        OneTimeCode::verify(state, "refund", "42", &code)
            .await
            .unwrap(),
        CodeCheck::Missing
    );
    assert_eq!(
        OneTimeCode::verify(state, "payout", "42", wrong)
            .await
            .unwrap(),
        CodeCheck::Wrong { tries_left: 2 }
    );
    assert_eq!(
        OneTimeCode::verify(state, "payout", "42", &code)
            .await
            .unwrap(),
        CodeCheck::Valid
    );
    // Used up.
    assert_eq!(
        OneTimeCode::verify(state, "payout", "42", &code)
            .await
            .unwrap(),
        CodeCheck::Missing
    );

    // Three wrong tries burn it; the right code is refused afterwards.
    let code = OneTimeCode::issue(state, "payout", "42", ttl, 3)
        .await
        .unwrap();
    for left in [2, 1, 0] {
        assert_eq!(
            OneTimeCode::verify(state, "payout", "42", wrong)
                .await
                .unwrap(),
            CodeCheck::Wrong { tries_left: left }
        );
    }
    assert_eq!(
        OneTimeCode::verify(state, "payout", "42", &code)
            .await
            .unwrap(),
        CodeCheck::Missing
    );
}

#[renox::test]
async fn one_time_codes_expire_are_replaced_and_discarded() {
    let (app, _, _) = app_with(Auth::new()).await;
    let state = app.state();
    let ttl = Duration::from_secs(60);
    let first = OneTimeCode::issue_digits(state, "p", "s", 8, ttl, 5)
        .await
        .unwrap();
    assert_eq!(first.len(), 8);
    let second = OneTimeCode::issue(state, "p", "s", ttl, 5).await.unwrap();
    if first != second {
        assert_eq!(
            OneTimeCode::verify(state, "p", "s", &first).await.unwrap(),
            CodeCheck::Wrong { tries_left: 4 }
        );
    }
    app.travel(Duration::from_secs(61));
    let late = app
        .at_travelled_time(OneTimeCode::verify(state, "p", "s", &second))
        .await
        .unwrap();
    assert_eq!(late, CodeCheck::Missing);
    app.travel_back();
    let third = OneTimeCode::issue(state, "p", "s", ttl, 5).await.unwrap();
    assert!(OneTimeCode::discard(state, "p", "s").await.unwrap());
    assert!(!OneTimeCode::discard(state, "p", "s").await.unwrap());
    assert_eq!(
        OneTimeCode::verify(state, "p", "s", &third).await.unwrap(),
        CodeCheck::Missing
    );
}

#[renox::test]
async fn parallel_guesses_cannot_exceed_the_limit() {
    let (app, _, _) = app_with(Auth::new()).await;
    let state = app.state().clone();
    let code = OneTimeCode::issue(&state, "p", "race", Duration::from_secs(60), 3)
        .await
        .unwrap();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let state = state.clone();
        tasks.push(tokio::spawn(async move {
            OneTimeCode::verify(&state, "p", "race", wrong)
                .await
                .unwrap()
        }));
    }
    let mut wrongs = 0;
    for task in tasks {
        if matches!(task.await.unwrap(), CodeCheck::Wrong { .. }) {
            wrongs += 1;
        }
    }
    assert_eq!(wrongs, 3, "only max_tries guesses are ever checked");

    // Two requests with the right code: one wins.
    let code = OneTimeCode::issue(&state, "p", "race", Duration::from_secs(60), 5)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        OneTimeCode::verify(&state, "p", "race", &code),
        OneTimeCode::verify(&state, "p", "race", &code)
    );
    let results = [a.unwrap(), b.unwrap()];
    assert_eq!(
        results.iter().filter(|r| **r == CodeCheck::Valid).count(),
        1,
        "{results:?}"
    );
}
