//! #253: auth paths no test had reached: the `AuthUser` methods, the auth
//! middleware when its lookups fail, guards for guests, the session of an
//! older login, registration fields, and the helpers for other login ways.

use renox::auth::permissions::{self, Permissions, Scope};
use renox::auth::{DatabaseMessage, complete_login, pending_login};
use renox::prelude::*;
use renox::testing::TestApp;

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/home", || async { "home" })
            .get("/me", |user: AuthUser| async move {
                format!(
                    "{}|{:?}|{}|{}",
                    user.has_permission("orders.view"),
                    user.token_id(),
                    user.has_role_in("manager", &Scope::new("stores", 7)),
                    user.has_role_in("manager", &Scope::new("stores", 8)),
                )
            })
            .get("/who", |user: Option<AuthUser>| async move {
                user.map_or_else(|| "guest".to_owned(), |u| u.name.clone())
            })
            .get("/beta", |user: AuthUser| async move {
                Ok::<_, Error>(format!(
                    "{}|{}",
                    user.allows_async("beta").await?,
                    user.allows_async("no-such-gate").await?
                ))
            })
            .get("/forget-session-id", |session: Session| async move {
                // As a login from before sessions had ids.
                session.remove("_auth_session_id");
                "ok"
            })
            .post(
                "/logout-now",
                |State(state): State<AppState>, session: Session| async move {
                    renox::auth::logout(&state.db, &session).await?;
                    Ok::<_, Error>("out")
                },
            )
            .get(
                "/pending",
                |State(state): State<AppState>, session: Session| async move {
                    let Some(pending) = pending_login(&session) else {
                        return Ok::<_, Error>("none".to_owned());
                    };
                    Ok(format!(
                        "{:?}",
                        complete_login(&state, &session, &pending, None).await?
                    ))
                },
            )
            .merge(
                Routes::new()
                    .get("/verified-only", || async { "verified" })
                    .require_verified(),
            )
    }
}

async fn app() -> TestApp {
    TestApp::new(
        App::new()
            .module(Auth::new())
            .module(Permissions)
            .module(Pages)
            .gate_async("beta", |_user, _state| async { Ok(false) }),
    )
    .await
}

async fn ann(app: &TestApp) -> User {
    User::register(app.db(), "Ann", "ann@example.com", "password123")
        .await
        .unwrap()
}

#[renox::test]
async fn auth_user_answers_for_permissions_tokens_and_scoped_roles() {
    let app = app().await;
    let db = app.db();
    permissions::define_role(db, "clerk", &["orders.view"])
        .await
        .unwrap();
    permissions::define_role(db, "manager", &[]).await.unwrap();
    let ann = ann(&app).await;
    ann.assign_role(db, "clerk").await.unwrap();
    ann.assign_role_in(db, "manager", &Scope::new("stores", 7))
        .await
        .unwrap();
    app.acting_as(&ann);
    app.get("/me").await.assert_see("true|None|true|false");
    // With a token, the request knows which one.
    let token = ann.create_token(db, "cli", None).await.unwrap();
    let res = app
        .request()
        .header("authorization", &format!("Bearer {}", token.plain))
        .get("/me")
        .await;
    let text = res.text();
    assert!(text.starts_with("true|Some("), "{text}");
}

#[renox::test]
async fn async_gates_and_unknown_gates_refuse() {
    let app = app().await;
    app.acting_as(&ann(&app).await);
    app.get("/beta").await.assert_see("false|false");
}

#[renox::test]
async fn a_guest_asking_for_json_gets_a_401() {
    let app = app().await;
    let res = app.request().json().get("/me").await;
    res.assert_unauthorized();
    assert!(res.json_path("message").is_string());
}

#[renox::test]
async fn require_verified_sends_guests_to_log_in() {
    let app = app().await;
    app.get("/verified-only").await.assert_redirect("/login");
    app.request()
        .json()
        .get("/verified-only")
        .await
        .assert_unauthorized();
}

#[renox::test]
async fn logging_out_an_older_session_ends_all_of_the_users_sessions() {
    let app = app().await;
    let ann = ann(&app).await;
    app.post(
        "/login",
        &[("email", "ann@example.com"), ("password", "password123")],
    )
    .await
    .assert_redirect("/");
    let other_device = app.session_cookie();
    app.get("/forget-session-id").await.assert_ok();
    app.post("/logout-now", &[]).await.assert_see("out");
    // The other device's session (same user) is over too.
    app.use_session_cookie(other_device);
    app.get("/who").await.assert_see("guest");
    let revoked: i64 = renox::db::sql(
        "SELECT COUNT(*) FROM users WHERE id = ? AND sessions_revoked_at IS NOT NULL",
    )
    .bind(ann.id)
    .scalar(app.db())
    .await
    .unwrap();
    assert_eq!(revoked, 1);
}

#[renox::test]
async fn a_broken_token_table_makes_a_guest_not_an_error() {
    let app = app().await;
    let ann = ann(&app).await;
    let token = ann.create_token(app.db(), "cli", None).await.unwrap();
    renox::db::sql("DROP TABLE personal_access_tokens")
        .execute(app.db())
        .await
        .unwrap();
    app.request()
        .header("authorization", &format!("Bearer {}", token.plain))
        .get("/who")
        .await
        .assert_ok()
        .assert_see("guest");
}

#[renox::test]
async fn a_broken_roles_table_leaves_the_user_without_roles() {
    let app = app().await;
    let db = app.db();
    permissions::define_role(db, "clerk", &["orders.view"])
        .await
        .unwrap();
    let ann = ann(&app).await;
    ann.assign_role(db, "clerk").await.unwrap();
    renox::db::sql("DROP TABLE role_user")
        .execute(db)
        .await
        .unwrap();
    app.acting_as(&ann);
    app.get("/me")
        .await
        .assert_ok()
        .assert_see("false|None|false|false");
}

#[renox::test]
async fn a_broken_users_table_makes_a_guest() {
    let app = app().await;
    app.acting_as(&ann(&app).await);
    app.get("/who").await.assert_see("Ann");
    renox::db::sql("ALTER TABLE users RENAME TO people")
        .execute(app.db())
        .await
        .unwrap();
    app.get("/who").await.assert_ok().assert_see("guest");
}

/// A second login step that every user must pass, for the pending-login test.
struct Step;

impl Module for Step {
    fn name(&self) -> &'static str {
        "step"
    }

    fn register(&self, app: &mut Registry) {
        app.second_factor("step.challenge", |_user, _state| async { Ok(true) });
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/step", || async { "step" })
            .name("step.challenge")
            .get("/", || async { "home" })
            .name("home")
    }
}

#[renox::test]
async fn completing_a_login_for_a_user_who_is_gone_ends_the_wait() {
    let app = TestApp::new(App::new().module(Auth::new()).module(Step).module(Pages)).await;
    app.get("/pending").await.assert_see("none");
    let ann = ann(&app).await;
    app.post(
        "/login",
        &[("email", "ann@example.com"), ("password", "password123")],
    )
    .await
    .assert_redirect("/step");
    let mut ann = ann;
    ann.delete(app.db()).await.unwrap();
    // The wait is over and nobody is logged in.
    app.get("/pending").await.assert_see("None");
    app.get("/pending").await.assert_see("none");
    app.get("/who").await.assert_see("guest");
}

#[renox::test]
async fn database_messages_have_a_kind_for_each_level() {
    for (message, kind) in [
        (DatabaseMessage::info("Synced"), "info"),
        (DatabaseMessage::warning("Low disk"), "warning"),
        (DatabaseMessage::error("Failed"), "error"),
    ] {
        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value["status"], kind, "{value}");
    }
}

struct Extra;

impl Module for Extra {
    fn name(&self) -> &'static str {
        "extra"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { "home" })
            .name("home")
            .post(
                "/join-by-invite",
                |State(state): State<AppState>| async move {
                    renox::auth::register_verified(&state, "Bob", "bob@example.com", &[]).await?;
                    Ok::<_, Error>("joined")
                },
            )
            .merge(
                Routes::new()
                    .post("/danger", || async { "done" })
                    .require_password_confirmed()
                    .require_auth(),
            )
    }
}

/// Registration fields sent several times (a group of checkboxes) reach
/// `registration_rules` and `on_registered` whole.
#[renox::test]
async fn registration_fields_sent_twice_are_lists() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let kept = seen.clone();
    let app = TestApp::new(
        App::new()
            .module(
                Auth::new()
                    .registration_rules(|reg, v| {
                        v.field("topics", &reg.all("topics")).required();
                    })
                    .on_registered(move |user, reg, _state| {
                        let kept = kept.clone();
                        async move {
                            kept.lock().unwrap().push(format!(
                                "{}:{}:{}",
                                user.name,
                                reg.get("topics"),
                                reg.all("topics").join("+")
                            ));
                            Ok(())
                        }
                    }),
            )
            .module(Extra),
    )
    .await;
    let form = [
        ("name", "Cara"),
        ("email", "cara@example.com"),
        ("password", "a long password 12"),
        ("password_confirmation", "a long password 12"),
    ];
    let mut with_topics = form.to_vec();
    with_topics.extend([("topics", "news"), ("topics", "deals")]);
    app.post("/register", &with_topics)
        .await
        .assert_redirect("/");
    assert_eq!(*seen.lock().unwrap(), ["Cara:news:news+deals"]);
    app.logout();
    let mut without = form.to_vec();
    without[1] = ("email", "dan@example.com");
    app.htmx()
        .post("/register", &without)
        .await
        .assert_invalid("topics");
}

#[renox::test]
async fn register_verified_is_refused_when_registration_is_closed() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new().without_registration())
            .module(Extra),
    )
    .await;
    app.post("/join-by-invite", &[]).await.assert_forbidden();
    app.assert_database_missing("users", &[("email", &"bob@example.com")])
        .await;
}

#[renox::test]
async fn an_htmx_request_is_sent_to_confirm_the_password_with_hx_redirect() {
    let app = TestApp::new(App::new().module(Auth::new()).module(Extra)).await;
    app.acting_as(&ann(&app).await);
    app.htmx()
        .post("/danger", &[])
        .await
        .assert_hx_redirect("/confirm-password");
    // Once confirmed, it goes through.
    app.confirm_password();
    app.htmx().post("/danger", &[]).await.assert_see("done");
}

/// A notification that only names itself: mail by default, and no mail
/// version, so sending it says what's missing.
struct Bare;

impl renox::auth::Notification for Bare {
    fn kind(&self) -> &'static str {
        "bare"
    }
}

/// Database only, with the default `null` data; and a channel with no
/// version for it.
struct Quiet;

impl renox::auth::Notification for Quiet {
    fn kind(&self) -> &'static str {
        "quiet"
    }

    fn channels(&self, _to: &renox::auth::Recipient) -> Vec<renox::auth::Channel> {
        vec![
            renox::auth::Channel::Database,
            renox::auth::Channel::Custom("sms"),
        ]
    }
}

#[renox::test]
async fn notifications_use_the_trait_defaults() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .channel("sms", |_to, _message, _state| async { Ok(()) }),
    )
    .await;
    let ann = ann(&app).await;
    let err = app.state().notify(&ann, &Bare).await.unwrap_err();
    assert!(
        format!("{err:?}").contains("notification `bare` has no mail version"),
        "{err:?}"
    );
    let err = app.state().notify(&ann, &Quiet).await.unwrap_err();
    assert!(
        format!("{err:?}").contains("has no version for the `sms` channel"),
        "{err:?}"
    );
    // The database row was written first, with `null` data.
    let data: Option<String> = renox::db::sql("SELECT data FROM notifications WHERE kind = ?")
        .bind("quiet")
        .scalar_optional(app.db())
        .await
        .unwrap();
    assert_eq!(data.as_deref(), Some("null"));
}

#[renox::test]
async fn notify_later_is_recorded_by_the_fake() {
    let app = TestApp::new(App::new().module(Auth::new())).await;
    app.fake_notifications();
    let ann = ann(&app).await;
    app.state().notify_later(&ann, &Bare).await.unwrap();
    app.assert_notified(&ann, "bare");
    assert!(app.queued_jobs().await.is_empty());
}

// The rest of #253: the auth paths left after the first pass, many of them
// what happens when a table, a template or a listener fails.

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "stores")]
struct Store {
    id: i64,
}

/// A record whose policy lets its owner edit it.
struct Doc {
    owner_id: i64,
}

impl Policy for Doc {
    fn allows(&self, user: &User, ability: &str) -> bool {
        ability == "edit" && user.id == self.owner_id
    }
}

struct More;

impl Module for More {
    fn name(&self) -> &'static str {
        "more"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { "home" })
            .name("home")
            .get("/policy", |user: AuthUser| async move {
                let mine = Doc { owner_id: user.id };
                let theirs = Doc { owner_id: 0 };
                let can = Can::new(mine, Some(&user), &["edit", "delete"]);
                let me = user.user();
                format!(
                    "{:?}|{}|{}|{}|{}",
                    can.abilities,
                    me.can("edit", &theirs),
                    me.authorize("edit", &Doc { owner_id: me.id }).is_ok(),
                    me.authorize("edit", &theirs).is_err(),
                    User::has_permission(me, "orders.view"),
                )
            })
            .get(
                "/scopes",
                |State(db): State<Db>, user: AuthUser| async move {
                    let store = Scope::of_id::<Store>(7);
                    permissions::set_scope(store.clone());
                    let scoped = user.has_role("manager");
                    permissions::clear_scope();
                    let unscoped = user.has_role("manager");
                    let me = user.user();
                    let stores = match me.scopes_with::<Store>("orders.refund") {
                        permissions::Scopes::All => "all".to_owned(),
                        permissions::Scopes::Only(ids) => format!("{ids:?}"),
                    };
                    let active = me
                        .assignments(&db)
                        .await?
                        .iter()
                        .map(|a| format!("{}:{}", a.role, a.is_active()))
                        .collect::<Vec<_>>()
                        .join(",");
                    Ok::<_, Error>(format!(
                        "{scoped}|{unscoped}|{}|{stores}|{active}",
                        me.has_role_in("manager", &store)
                    ))
                },
            )
            .merge(
                Routes::new()
                    .get("/flaky", || async { "never" })
                    .require_gate("flaky"),
            )
            .merge(
                Routes::new()
                    .post("/save", || async { "saved" })
                    .require_auth(),
            )
    }
}

fn more() -> App {
    App::new()
        .module(Auth::default())
        .module(Permissions)
        .module(More)
        .gate_async("flaky", |_user, _state| async {
            Err::<bool, _>(Error::Internal(renox::anyhow::anyhow!("gate service down")))
        })
}

#[renox::test]
async fn policies_see_the_auth_user_and_users_answer_for_themselves() {
    let app = TestApp::new(more()).await;
    let db = app.db();
    permissions::define_role(db, "clerk", &["orders.view"])
        .await
        .unwrap();
    let ann = ann(&app).await;
    ann.assign_role(db, "clerk").await.unwrap();
    app.acting_as(&ann);
    app.get("/policy")
        .await
        .assert_ok()
        .assert_see(r#"{"delete": false, "edit": true}|false|true|true|true"#);
}

#[renox::test]
async fn scopes_can_be_set_and_cleared_and_users_answer_for_a_scope() {
    let app = TestApp::new(more()).await;
    let db = app.db();
    permissions::define_role(db, "manager", &["orders.refund"])
        .await
        .unwrap();
    let ann = ann(&app).await;
    ann.assign_role_in(db, "manager", &Scope::of_id::<Store>(7))
        .await
        .unwrap();
    app.acting_as(&ann);
    app.get("/scopes")
        .await
        .assert_ok()
        .assert_see("true|false|true|[7]|manager:true");
}

#[renox::test]
async fn an_async_gate_that_fails_is_a_500_not_a_pass() {
    let app = TestApp::new(more()).await;
    app.acting_as(&ann(&app).await);
    app.get("/flaky").await.assert_status(500);
}

/// A guest who posts to a guarded route goes to log in, without the post
/// being remembered as where to go back to; a guest's logout just goes home.
#[renox::test]
async fn guests_posting_are_sent_to_log_in_and_their_logout_goes_home() {
    let app = TestApp::new(more()).await;
    app.post("/save", &[]).await.assert_redirect("/login");
    app.assert_session_missing("_intended");
    app.post("/logout", &[]).await.assert_redirect("/");
}

/// `auth::sign_in` in an app without the `Auth` module: the page that asked
/// for a login, else home, else `/`.
#[renox::test]
async fn sign_in_without_the_auth_module_goes_home() {
    struct SignIn;
    impl Module for SignIn {
        fn name(&self) -> &'static str {
            "sign-in"
        }
        fn routes(&self) -> Routes {
            Routes::new().post(
                "/sso",
                |State(state): State<AppState>, session: Session| async move {
                    let mut user = User::default();
                    user.id = 42;
                    user.name = "Sso".into();
                    renox::auth::sign_in(&state, &session, &user, false, None).await
                },
            )
        }
    }
    let app = TestApp::new(App::new().module(SignIn)).await;
    app.post("/sso", &[]).await.assert_ok().assert_see("/");

    // With a route named `home`, there; with a page that asked for a login
    // (what `require_auth` keeps in the session), that page.
    struct Home;
    impl Module for Home {
        fn name(&self) -> &'static str {
            "home"
        }
        fn routes(&self) -> Routes {
            Routes::new()
                .get("/dashboard", || async { "dashboard" })
                .name("home")
                .get("/asked", |session: Session| async move {
                    session.put("_intended", "/reports")?;
                    Ok::<_, Error>("asked")
                })
        }
    }
    let app = TestApp::new(App::new().module(SignIn).module(Home)).await;
    app.post("/sso", &[]).await.assert_see("/dashboard");
    app.get("/asked").await.assert_ok();
    app.post("/sso", &[]).await.assert_see("/reports");
    // The page was used: the next login goes home again.
    app.post("/sso", &[]).await.assert_see("/dashboard");
}

/// A failure while saving the new user (not a duplicate email) is a 500,
/// and nobody is logged in.
#[renox::test]
async fn a_sign_up_the_database_refuses_is_a_500() {
    let app = TestApp::new(App::new().module(Auth::new()).module(More)).await;
    let refuse = match app.db().dialect() {
        renox::db::Dialect::Sqlite => {
            "CREATE TRIGGER no_sign_ups BEFORE INSERT ON users BEGIN SELECT RAISE(ABORT, 'closed'); END"
        }
        _ => "ALTER TABLE users ADD CONSTRAINT no_sign_ups CHECK (email <> 'eve@example.com')",
    };
    renox::db::sql(refuse).execute(app.db()).await.unwrap();
    app.post(
        "/register",
        &[
            ("name", "Eve"),
            ("email", "eve@example.com"),
            ("password", "a long password 12"),
            ("password_confirmation", "a long password 12"),
        ],
    )
    .await
    .assert_status(500);
    app.assert_guest();
}

/// One value for a field read with `Registration::all` is a list of one.
#[renox::test]
async fn a_registration_field_sent_once_is_a_list_of_one() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let kept = seen.clone();
    let app = TestApp::new(App::new().module(Auth::new().on_registered(
        move |_user, reg, _state| {
            let kept = kept.clone();
            async move {
                kept.lock().unwrap().push(reg.all("topics"));
                Ok(())
            }
        },
    )))
    .await;
    app.post(
        "/register",
        &[
            ("name", "Cara"),
            ("email", "cara@example.com"),
            ("password", "a long password 12"),
            ("password_confirmation", "a long password 12"),
            ("topics", "news"),
        ],
    )
    .await
    .assert_redirect("/");
    assert_eq!(*seen.lock().unwrap(), [vec!["news".to_owned()]]);
}

/// A user without a password (made by a social login) confirms who they
/// are before changing their password or ending other sessions; a user
/// with one must type it.
#[renox::test]
async fn account_actions_ask_for_confirmation_or_the_password() {
    let app = TestApp::new(App::new().module(Auth::new().account()).module(More)).await;
    let ann = ann(&app).await;
    let mut social = User::register(app.db(), "Sol", "sol@example.com", "password123")
        .await
        .unwrap();
    renox::db::sql("UPDATE users SET password = '' WHERE id = ?")
        .bind(social.id)
        .execute(app.db())
        .await
        .unwrap();
    social.password = String::new();
    app.acting_as(&social);
    app.put(
        "/account/password",
        &[
            ("password", "a long password 12"),
            ("password_confirmation", "a long password 12"),
        ],
    )
    .await
    .assert_redirect("/confirm-password");
    app.post("/account/logout-others", &[])
        .await
        .assert_redirect("/confirm-password");

    app.acting_as(&ann);
    app.htmx()
        .post("/account/logout-others", &[("password", "")])
        .await
        .assert_invalid("password");
}

/// Without `verify_email`, a new address is saved as it is: no new
/// verification, and the old verification date stays.
#[renox::test]
async fn a_new_email_needs_no_verification_unless_asked() {
    let app = TestApp::new(App::new().module(Auth::new().account()).module(More)).await;
    let mut ann = ann(&app).await;
    ann.email_verified_at = Some(renox::db::now());
    ann.save(app.db()).await.unwrap();
    app.acting_as(&ann);
    app.put(
        "/account/profile",
        &[("name", "Ann"), ("email", "ann.b@example.com")],
    )
    .await
    .assert_redirect("/account");
    let me = User::find_by_email(app.db(), "ann.b@example.com")
        .await
        .unwrap()
        .unwrap();
    assert!(me.email_verified_at.is_some());
    assert!(app.sent_mail().is_empty());
}

/// An app's copy of the auth mails that doesn't render: the request fails
/// (500) instead of claiming a mail went out.
fn broken_mail_views() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("renox/mail/auth");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("reset-password.html"),
        "{{ link | no_such_filter }}",
    )
    .unwrap();
    std::fs::write(dir.join("verify-email.txt"), "{% if %}").unwrap();
    root
}

#[renox::test]
async fn auth_mails_that_dont_render_fail_the_request() {
    let views = broken_mail_views();
    let path = views.path().to_owned();
    let app = TestApp::with_config(
        App::new().module(Auth::new().verify_email()).module(More),
        move |c| c.views_path = path,
    )
    .await;
    let ann = ann(&app).await;
    app.post("/forgot-password", &[("email", "ann@example.com")])
        .await
        .assert_status(500);
    app.acting_as(&ann);
    app.post("/email/verification-notification", &[])
        .await
        .assert_status(500);
    assert!(app.sent_mail().is_empty());
}

#[renox::test]
async fn a_reset_form_that_isnt_valid_goes_back_with_errors() {
    let app = TestApp::new(App::new().module(Auth::new()).module(More)).await;
    app.htmx()
        .post(
            "/reset-password",
            &[("token", ""), ("email", "not an email")],
        )
        .await
        .assert_invalid("email");
}

/// The verification link works once and then just goes home; asking for
/// another link when verified goes home too.
#[renox::test]
async fn verifying_twice_and_resending_when_verified_go_home() {
    let app = TestApp::new(App::new().module(Auth::new().verify_email()).module(More)).await;
    app.post(
        "/register",
        &[
            ("name", "Ann"),
            ("email", "ann@example.com"),
            ("password", "a long password 12"),
            ("password_confirmation", "a long password 12"),
        ],
    )
    .await
    .assert_redirect("/");
    let mail = app.sent_mail().pop().expect("a verification mail");
    let html = mail.html.unwrap_or_default();
    let start = html.find("/verify-email/").expect("a link");
    let end = start + html[start..].find('"').unwrap();
    let link = html[start..end].replace("&amp;", "&");
    app.get(&link).await.assert_redirect("/");
    let verified_at = User::find_by_email(app.db(), "ann@example.com")
        .await
        .unwrap()
        .unwrap()
        .email_verified_at;
    assert!(verified_at.is_some());
    app.get(&link).await.assert_redirect("/");
    app.post("/email/verification-notification", &[])
        .await
        .assert_redirect("/");
    assert_eq!(app.sent_mail().len(), 1, "no second mail");
}

/// `send_verification` in an app without the `Auth` module's routes says
/// the route is missing, instead of mailing a broken link.
#[renox::test]
async fn sending_a_verification_without_its_route_is_an_error() {
    let app = TestApp::new(App::new()).await;
    let mut user = User::default();
    user.id = 1;
    user.email = "ann@example.com".into();
    let err = renox::auth::send_verification(app.state(), &user)
        .await
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("verification.verify"),
        "{err:?}"
    );
}

#[renox::test]
async fn a_failing_login_listener_doesnt_stop_the_login() {
    let (logs, _logged) = crate::logs::capture();
    let app = TestApp::new(App::new().module(Auth::new()).module(More).listen(
        |_: renox::auth::events::LoggedIn, _state| async {
            Err::<(), _>(Error::Internal(renox::anyhow::anyhow!("audit is down")))
        },
    ))
    .await;
    let ann = ann(&app).await;
    app.post(
        "/login",
        &[("email", "ann@example.com"), ("password", "password123")],
    )
    .await
    .assert_redirect("/");
    app.assert_authenticated(Some(&ann));
    assert!(logs.has(&["auth listener failed"]), "{}", logs.text());
}

#[renox::test]
async fn users_end_their_sessions_and_wrong_passwords_find_nobody() {
    let app = TestApp::new(App::new().module(Auth::new()).module(Pages)).await;
    let ann = ann(&app).await;
    assert!(
        User::attempt(app.db(), "ann@example.com", "wrong")
            .await
            .unwrap()
            .is_none()
    );
    app.acting_as(&ann);
    app.get("/who").await.assert_see("Ann");
    ann.revoke_sessions(app.db()).await.unwrap();
    app.get("/who").await.assert_see("guest");
}

/// A bearer token that isn't `id|secret` with a numeric id: a guest.
#[renox::test]
async fn a_malformed_token_is_a_guest() {
    let app = app().await;
    ann(&app).await;
    app.request()
        .header("authorization", "Bearer abc|def")
        .get("/who")
        .await
        .assert_ok()
        .assert_see("guest");
}

/// With `CACHE_STORE=database`, a successful login clears the login lock's
/// counters for the address and the account; a cache table that can't be
/// read or written leaves logins working.
#[renox::test]
async fn the_shared_login_lock_clears_on_success_and_survives_a_broken_cache() {
    let app = TestApp::with_config(App::new().module(Auth::new()).module(More), |c| {
        c.cache_store = renox::CacheStore::Database;
    })
    .await;
    let ann = ann(&app).await;
    let counters = |pattern: &'static str| {
        let app = &app;
        async move {
            renox::db::sql("SELECT COUNT(*) FROM cache WHERE key LIKE ?")
                .bind(pattern)
                .scalar::<i64>(app.db())
                .await
                .unwrap()
        }
    };
    let wrong = [("email", "ann@example.com"), ("password", "nope")];
    let right = [("email", "ann@example.com"), ("password", "password123")];
    app.post("/login", &wrong).await.assert_redirect("/");
    assert_eq!(counters("%login:account:%").await, 1);
    app.post("/login", &right).await.assert_redirect("/");
    assert_eq!(counters("%login:account:%").await, 0);
    assert_eq!(counters("%login:pair:%").await, 0);
    assert_eq!(
        counters("%login:ip:%").await,
        1,
        "the address keeps its count"
    );
    app.assert_authenticated(Some(&ann));

    app.logout();
    renox::db::sql("ALTER TABLE cache RENAME TO cache_gone")
        .execute(app.db())
        .await
        .unwrap();
    app.post("/login", &wrong).await.assert_redirect("/");
    app.assert_guest();
    app.post("/login", &right).await.assert_redirect("/");
    app.assert_authenticated(Some(&ann));
}

#[renox::test]
async fn audit_entries_keep_the_address() {
    let app = TestApp::new(App::new().module(Auth::new()).module(renox::audit::Audit)).await;
    let ip: std::net::IpAddr = "203.0.113.7".parse().unwrap();
    renox::audit::record(app.db(), renox::audit::Entry::new("export").ip(Some(ip)))
        .await
        .unwrap();
    let latest = renox::audit::latest(app.db(), 1).await.unwrap();
    assert_eq!(latest[0].ip.as_deref(), Some("203.0.113.7"));
}
