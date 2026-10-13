//! M18b: account pages, password policy and confirmation, per-device
//! logout, auth events and the audit log, bcrypt users from Laravel.

use renox::audit::{self, Audit, Entry};
use renox::auth::events::LoggedIn;
use renox::prelude::*;
use renox::testing::TestApp;
use renox::validation::Password;

/// Laravel's classic factory hash of "password".
const LARAVEL_HASH: &str = "$2y$10$92IXUNpkjO0rOQ5byMi.Ye4oKoEa3Ro9llC/.og/at2.uheWG/igi";

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        let me = Routes::new()
            .get("/me", |user: AuthUser| async move { user.email.clone() })
            .require_auth();
        let billing = Routes::new()
            .get("/billing", || async { "billing settings" })
            .require_password_confirmed();
        me.merge(billing)
    }
}

fn app() -> App {
    App::new()
        .module(
            Auth::new()
                .account()
                .verify_email()
                .password_rules(Password::min(10).numbers()),
        )
        .module(Audit)
        .module(Pages)
        .listen(|e: LoggedIn, state| async move {
            renox::db::sql("UPDATE users SET name = name WHERE id = ?")
                .bind(e.user_id)
                .execute(&state.db)
                .await?;
            Ok(())
        })
}

async fn boot() -> TestApp {
    TestApp::new(app()).await
}

async fn user(app: &TestApp, email: &str) -> User {
    User::register(app.db(), "Ana", email, "password123")
        .await
        .unwrap()
}

async fn log_in(app: &TestApp, email: &str, password: &str) -> u16 {
    app.post("/login", &[("email", email), ("password", password)])
        .await
        .status
        .as_u16()
}

#[renox::test]
async fn laravel_users_log_in_and_are_rehashed() {
    let app = boot().await;
    for email in ["old@example.com", "api@example.com"] {
        renox::db::sql(
            "INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind("Old")
        .bind(email)
        .bind(LARAVEL_HASH)
        .bind(renox::db::now())
        .bind(renox::db::now())
        .execute(app.db())
        .await
        .unwrap();
    }
    assert_eq!(log_in(&app, "old@example.com", "wrong").await, 303);
    app.get("/me").await.assert_redirect("/login");
    assert_eq!(log_in(&app, "old@example.com", "password").await, 303);
    app.get("/me").await.assert_see("old@example.com");
    let hash = |email: &'static str| {
        let db = app.db().clone();
        async move {
            renox::db::sql("SELECT password FROM users WHERE email = ?")
                .bind(email)
                .scalar::<String>(&db)
                .await
                .unwrap()
        }
    };
    assert!(hash("old@example.com").await.starts_with("$argon2id$"));
    // The session made during the rehash stays valid.
    app.get("/me").await.assert_ok();
    // `User::attempt` (API logins) rehashes too.
    assert!(
        User::attempt(app.db(), "api@example.com", "password")
            .await
            .unwrap()
            .is_some()
    );
    assert!(!renox::auth::needs_rehash(&hash("api@example.com").await));
}

#[renox::test]
async fn logout_ends_this_device_only() {
    let app = boot().await;
    user(&app, "ana@example.com").await;
    log_in(&app, "ana@example.com", "password123").await;
    let laptop = app.session_cookie();
    app.use_session_cookie(None);
    log_in(&app, "ana@example.com", "password123").await;
    let phone = app.session_cookie();

    app.post("/logout", &[]).await.assert_status(303);
    // A copy of the phone's cookie is dead; the laptop is still logged in.
    app.use_session_cookie(phone);
    app.get("/me").await.assert_redirect("/login");
    app.use_session_cookie(laptop.clone());
    app.get("/me").await.assert_see("ana@example.com");

    // "Log out other devices" keeps this one.
    app.use_session_cookie(None);
    log_in(&app, "ana@example.com", "password123").await;
    let tablet = app.session_cookie();
    app.use_session_cookie(laptop.clone());
    app.htmx()
        .post("/account/logout-others", &[("password", "wrong")])
        .await
        .assert_invalid("password");
    app.post("/account/logout-others", &[("password", "password123")])
        .await
        .assert_redirect("/account");
    app.get("/me").await.assert_ok();
    app.use_session_cookie(tablet);
    app.get("/me").await.assert_redirect("/login");
}

#[renox::test]
async fn the_account_page_changes_profile_password_and_deletes() {
    let app = boot().await;
    let mut ana = user(&app, "ana@example.com").await;
    user(&app, "bo@example.com").await;
    ana.email_verified_at = Some(renox::db::now());
    ana.save(app.db()).await.unwrap();
    app.get("/account").await.assert_redirect("/login");
    log_in(&app, "ana@example.com", "password123").await;
    app.get("/account")
        .await
        .assert_ok()
        .assert_see(r#"value="Ana""#)
        .assert_see("Change password");

    // Profile: a taken email is refused; a new one must be verified again.
    app.htmx()
        .put(
            "/account/profile",
            &[("name", "Ana"), ("email", "bo@example.com")],
        )
        .await
        .assert_invalid("email");
    app.put(
        "/account/profile",
        &[("name", "Ana B"), ("email", "ANA.B@example.com ")],
    )
    .await
    .assert_redirect("/account");
    let me = User::find_by_email(app.db(), "ana.b@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(me.name, "Ana B");
    assert!(
        me.email_verified_at.is_none(),
        "a new email is verified again"
    );
    app.assert_mail_sent("ana.b@example.com", "Verify your email address");
    app.get("/account")
        .await
        .assert_see("Profile")
        .assert_see("Your profile is saved.");

    // Password: the current one must be right and the new one follow the policy.
    let other = {
        let mine = app.session_cookie();
        app.use_session_cookie(None);
        log_in(&app, "ana.b@example.com", "password123").await;
        let other = app.session_cookie();
        app.use_session_cookie(mine);
        other
    };
    let change = |current: &'static str, new: &'static str| {
        let app = &app;
        async move {
            app.htmx()
                .put(
                    "/account/password",
                    &[
                        ("current_password", current),
                        ("password", new),
                        ("password_confirmation", new),
                    ],
                )
                .await
        }
    };
    change("wrong", "longenough1")
        .await
        .assert_invalid("current_password");
    change("password123", "short1")
        .await
        .assert_invalid("password");
    change("password123", "no-digits-here")
        .await
        .assert_invalid("password");
    change("password123", "longenough1")
        .await
        .assert_hx_redirect("/account");
    app.get("/me").await.assert_ok(); // this device stays
    app.use_session_cookie(other);
    app.get("/me").await.assert_redirect("/login"); // the other one doesn't
    app.use_session_cookie(None);
    assert_eq!(log_in(&app, "ana.b@example.com", "password123").await, 303);
    app.get("/me").await.assert_redirect("/login");
    log_in(&app, "ana.b@example.com", "longenough1").await;
    app.get("/me").await.assert_ok();

    // Deleting asks for the password, and takes the user's grid choices.
    let id = User::find_by_email(app.db(), "ana.b@example.com")
        .await
        .unwrap()
        .unwrap()
        .id;
    renox::db::sql(
        "INSERT INTO grid_preferences (user_id, grid, data, updated_at) VALUES (?, 'orders', '{}', ?)",
    )
    .bind(id)
    .bind(renox::db::now())
    .execute(app.db())
    .await
    .unwrap();
    app.htmx()
        .post("/account", &[("_method", "DELETE"), ("password", "nope")])
        .await
        .assert_invalid("password");
    app.htmx()
        .post(
            "/account",
            &[("_method", "DELETE"), ("password", "longenough1")],
        )
        .await
        .assert_hx_redirect("/");
    assert!(
        User::find_by_email(app.db(), "ana.b@example.com")
            .await
            .unwrap()
            .is_none()
    );
    let left: i64 = renox::db::sql("SELECT COUNT(*) FROM grid_preferences WHERE user_id = ?")
        .bind(id)
        .scalar(app.db())
        .await
        .unwrap();
    assert_eq!(left, 0);
    app.get("/me").await.assert_redirect("/login");
}

#[renox::test]
async fn registration_follows_the_password_policy() {
    let app = boot().await;
    app.htmx()
        .post(
            "/register",
            &[
                ("name", "C"),
                ("email", "c@example.com"),
                ("password", "passwordxx"),
                ("password_confirmation", "passwordxx"),
            ],
        )
        .await
        .assert_invalid("password")
        .assert_see("at least one number");
    app.htmx()
        .post(
            "/register",
            &[
                ("name", "C"),
                ("email", "c@example.com"),
                ("password", "password12"),
                ("password_confirmation", "password12"),
            ],
        )
        .await
        .assert_hx_redirect("/");
}

#[renox::test]
async fn secure_pages_ask_for_the_password_again() {
    let app = boot().await;
    let ana = user(&app, "ana@example.com").await;
    app.get("/billing").await.assert_redirect("/login");
    app.acting_as(&ana); // a session without a recent password
    app.get("/billing")
        .await
        .assert_redirect("/confirm-password");
    app.get("/confirm-password")
        .await
        .assert_see("Confirm your password");
    app.htmx()
        .post("/confirm-password", &[("password", "wrong")])
        .await
        .assert_invalid("password");
    app.post("/confirm-password", &[("password", "password123")])
        .await
        .assert_redirect("/billing");
    app.get("/billing").await.assert_see("billing settings");
    // A login through the form counts as a confirmation.
    app.use_session_cookie(None);
    log_in(&app, "ana@example.com", "password123").await;
    app.get("/billing").await.assert_ok();
}

#[renox::test]
async fn auth_events_land_in_the_audit_log() {
    let app = boot().await;
    let ana = user(&app, "ana@example.com").await;
    log_in(&app, "ana@example.com", "wrong").await;
    log_in(&app, "ana@example.com", "password123").await;
    app.post("/logout", &[]).await;
    for _ in 0..6 {
        log_in(&app, "ana@example.com", "wrong").await; // the 6th is locked out
    }
    let actions: Vec<String> = audit::latest(app.db(), 20)
        .await
        .unwrap()
        .into_iter()
        .rev()
        .map(|e| e.action)
        .collect();
    assert_eq!(
        &actions[..3],
        ["auth.login_failed", "auth.login", "auth.logout"]
    );
    assert!(
        actions.contains(&"auth.locked_out".to_owned()),
        "{actions:?}"
    );
    let failed = audit::latest(app.db(), 1).await.unwrap().remove(0);
    assert_eq!(failed.data["email"], "ana@example.com");

    // The app's own entries.
    audit::record(
        app.db(),
        Entry::new("order.refunded")
            .user(ana.id)
            .subject("orders", 42)
            .data(json!({ "amount": 75_000 })),
    )
    .await
    .unwrap();
    let order = audit::for_subject(app.db(), "orders", 42, 10)
        .await
        .unwrap();
    assert_eq!(
        (order.len(), order[0].data["amount"].as_i64()),
        (1, Some(75_000))
    );
    let mine: Vec<String> = audit::for_user(app.db(), ana.id, 10)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(mine, ["order.refunded", "auth.logout", "auth.login"]);

    renox::db::sql("UPDATE audit_logs SET created_at = ? WHERE action = 'order.refunded'")
        .bind(renox::db::now() - renox::chrono::TimeDelta::days(400))
        .execute(app.db())
        .await
        .unwrap();
    app.kernel()
        .call("audit:prune", ["--days", "365"])
        .await
        .unwrap();
    assert!(
        audit::for_subject(app.db(), "orders", 42, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[renox::test]
async fn password_policy_messages_name_each_rule_and_are_translated() {
    use renox::validation::Validator;
    struct Form(&'static str);
    impl Validate for Form {
        fn rules(&self, v: &mut Validator) {
            let policy = Password::min(8).mixed_case().numbers().symbols();
            v.field("password", &self.0).password(&policy);
        }
    }
    let app = boot().await;
    let message = |password: &'static str| {
        let db = app.db().clone();
        async move {
            Validator::rules_of(&Form(password))
                .finish(&db)
                .await
                .unwrap()
                .first("password")
                .map(str::to_owned)
        }
    };
    assert_eq!(message("Abcdefg1!").await, None);
    assert!(
        message("abcdefg1!")
            .await
            .unwrap()
            .contains("uppercase and one lowercase")
    );
    assert!(
        message("Abcdefgh!")
            .await
            .unwrap()
            .contains("at least one number")
    );
    assert!(
        message("Abcdefgh1")
            .await
            .unwrap()
            .contains("at least one symbol")
    );
    assert!(message("Ab1!").await.unwrap().contains("8"));

    // The app's lang file translates them (`tests/lang/es.json`), with its
    // name for the field.
    let spanish = TestApp::with_config(self::app(), |c| {
        c.locale = "es".into();
        c.lang_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lang");
    })
    .await;
    spanish
        .htmx()
        .post(
            "/register",
            &[
                ("name", "C"),
                ("email", "c@example.com"),
                ("password", "passwordxx"),
                ("password_confirmation", "passwordxx"),
            ],
        )
        .await
        .assert_invalid("password")
        .assert_see("El campo contraseña debe contener al menos un número.");
}

#[renox::test]
async fn read_notifications_are_pruned_after_a_while() {
    let app = boot().await;
    let ana = user(&app, "ana@example.com").await;
    let ago = |days: i64| renox::db::now() - renox::chrono::TimeDelta::days(days);
    for (kind, read_at) in [
        ("old", Some(ago(40))),
        ("recent", Some(ago(2))),
        ("unread", None),
    ] {
        renox::db::sql(
            "INSERT INTO notifications (user_id, kind, data, read_at, created_at) VALUES (?, ?, '{}', ?, ?)",
        )
        .bind(ana.id)
        .bind(kind)
        .bind(read_at)
        .bind(ago(60))
        .execute(app.db())
        .await
        .unwrap();
    }
    let kinds = || async {
        renox::db::sql("SELECT kind FROM notifications ORDER BY id")
            .scalars::<String>(app.db())
            .await
            .unwrap()
    };
    app.kernel()
        .call("notifications:prune", Vec::<String>::new())
        .await
        .unwrap();
    assert_eq!(kinds().await, ["recent", "unread"], "read over 30 days ago");
    let pruned = renox::auth::prune_read_notifications(
        app.db(),
        std::time::Duration::from_secs(24 * 60 * 60),
    )
    .await
    .unwrap();
    assert_eq!(pruned, 1);
    assert_eq!(kinds().await, ["unread"], "unread ones stay");
}
