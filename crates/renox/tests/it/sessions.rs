//! M21g: sessions kept in the database (`SESSION_DRIVER=database`).

use renox::prelude::*;
use renox::testing::TestApp;

struct Probe;

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get(
                "/put/{v}",
                |session: Session, Path(v): Path<String>| async move {
                    session.put("v", v)?;
                    Ok::<_, Error>("ok")
                },
            )
            .get("/big", |session: Session| async move {
                session.put("big", "x".repeat(20_000))?;
                Ok::<_, Error>("ok")
            })
            .get("/get", |session: Session| async move {
                session.get::<String>("v").unwrap_or_else(|| "none".into())
            })
            .get(
                "/login/{id}",
                |State(db): State<Db>, session: Session, Path(id): Path<i64>| async move {
                    let user = User::find_or_404(&db, id).await?;
                    renox::auth::login(&session, &user, None)?;
                    Ok::<_, Error>("in")
                },
            )
            .get("/logout", |session: Session| async move {
                session.flush();
                "out"
            })
            .get("/whoami", |user: Option<AuthUser>| async move {
                user.map_or_else(|| "guest".to_owned(), |u| u.name.clone())
            })
    }
}

fn app() -> App {
    App::new().module(Auth::new()).module(Probe)
}

async fn rows(app: &TestApp) -> i64 {
    renox::db::sql("SELECT COUNT(*) FROM sessions")
        .scalar(app.db())
        .await
        .unwrap()
}

#[renox::test]
async fn the_database_holds_the_session_and_the_cookie_only_its_id() {
    // Outside the testing environment, so the real table is used.
    let app = TestApp::with_config(app(), |c| {
        c.session_driver = renox::SessionDriver::Database;
        c.env = renox::Environment::Local;
    })
    .await;
    let user = User::register(app.db(), "Ana", "ana@example.com", "password123")
        .await
        .unwrap();

    app.get("/put/coffee").await.assert_see("ok");
    let cookie = app.session_cookie().unwrap();
    assert!(cookie.len() < 200, "only an id: {cookie}");
    assert_eq!(rows(&app).await, 1);
    app.get("/get").await.assert_see("coffee");
    // No 4 KB limit.
    app.get("/big").await.assert_ok();
    assert!(app.session_cookie().unwrap().len() < 200);
    app.get("/get").await.assert_see("coffee");
    // The table has the id's hash, not the id.
    let stored: String = renox::db::sql("SELECT id FROM sessions")
        .scalar(app.db())
        .await
        .unwrap();
    assert!(!cookie.contains(&stored));

    // Logging in gives the session a new id and deletes the old row, so a
    // copy of the old cookie is worthless.
    let before = app.session_cookie();
    app.get(&format!("/login/{}", user.id))
        .await
        .assert_see("in");
    let after = app.session_cookie();
    assert_ne!(before, after);
    app.get("/whoami").await.assert_see("Ana");
    app.get("/get").await.assert_see("coffee"); // the data came along
    let owner: Option<i64> = renox::db::sql("SELECT user_id FROM sessions")
        .scalar(app.db())
        .await
        .unwrap();
    assert_eq!(owner, Some(user.id));
    assert_eq!(rows(&app).await, 1);
    app.use_session_cookie(before);
    app.get("/whoami").await.assert_see("guest");
    app.get("/get").await.assert_see("none");

    // Logging out does the same.
    app.use_session_cookie(after.clone());
    app.get("/logout").await.assert_see("out");
    app.use_session_cookie(after);
    app.get("/whoami").await.assert_see("guest");
}

#[renox::test]
async fn cookie_sessions_carry_over_and_expired_rows_are_pruned() {
    let app = TestApp::with_config(app(), |c| {
        c.session_driver = renox::SessionDriver::Database;
        c.env = renox::Environment::Local;
    })
    .await;
    let user = User::register(app.db(), "Ben", "ben@example.com", "password123")
        .await
        .unwrap();
    // A whole-session cookie (as the cookie driver wrote it) still works:
    // switching SESSION_DRIVER logs nobody out.
    app.acting_as(&user);
    app.get("/whoami").await.assert_see("Ben");
    assert!(app.session_cookie().unwrap().len() < 200, "now an id");
    app.get("/whoami").await.assert_see("Ben");

    renox::db::sql(
        "INSERT INTO sessions (id, user_id, payload, expires_at, last_activity) VALUES ('old', NULL, '{}', 1, 1)",
    )
    .execute(app.db())
    .await
    .unwrap();
    // A request's background prune (1 in 50) may have beaten us to it.
    let pruned = Session::prune_expired(app.db()).await.unwrap();
    assert!(pruned <= 1, "{pruned}");
    assert_eq!(rows(&app).await, 1, "only the live session is left");
    app.get("/whoami").await.assert_see("Ben");
}

#[renox::test]
async fn test_helpers_work_with_database_sessions() {
    let app =
        TestApp::with_config(app(), |c| c.session_driver = renox::SessionDriver::Database).await;
    let user = User::register(app.db(), "Cindy", "cindy@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.get("/put/tea").await.assert_ok();
    assert_eq!(app.session_get::<String>("v").as_deref(), Some("tea"));
    app.assert_authenticated(Some(&user))
        .assert_session_has("v");
    app.get("/whoami").await.assert_see("Cindy");
    // Forms still get their CSRF token from the session.
    app.post("/logout", &[]).await.assert_redirect("/");
    app.assert_guest();
}

/// #252: the cookie driver still sends a session over 4 KB (and logs that
/// browsers may drop it).
#[renox::test]
async fn a_cookie_session_over_4_kb_is_still_sent() {
    let (logs, _logged) = crate::logs::capture();
    let app = TestApp::new(app()).await;
    app.get("/big").await.assert_ok();
    assert!(app.session_cookie().unwrap().len() > 4000);
    assert!(
        logs.has(&["WARN", "larger than browsers reliably store"]),
        "{}",
        logs.text()
    );
}

/// With the test mirror (`APP_ENV=testing`), a new id at login drops the
/// old session from the mirror.
#[renox::test]
async fn logging_in_drops_the_old_session_from_the_test_mirror() {
    let app =
        TestApp::with_config(app(), |c| c.session_driver = renox::SessionDriver::Database).await;
    let user = User::register(app.db(), "Dee", "dee@example.com", "password123")
        .await
        .unwrap();
    app.get("/put/coffee").await.assert_ok();
    let before = app.session_cookie();
    app.get(&format!("/login/{}", user.id))
        .await
        .assert_see("in");
    app.get("/get").await.assert_see("coffee");
    app.use_session_cookie(before);
    app.get("/get").await.assert_see("none");
}

/// A session row that can't be deleted at login (the table is gone): the
/// failure is logged and the page still answers.
#[renox::test]
async fn a_session_that_cant_be_deleted_doesnt_break_the_login() {
    let app = TestApp::with_config(app(), |c| {
        c.session_driver = renox::SessionDriver::Database;
        c.env = renox::Environment::Local;
    })
    .await;
    let user = User::register(app.db(), "Eli", "eli@example.com", "password123")
        .await
        .unwrap();
    app.get("/put/coffee").await.assert_ok();
    renox::db::sql("ALTER TABLE sessions RENAME TO sessions_gone")
        .execute(app.db())
        .await
        .unwrap();
    app.get(&format!("/login/{}", user.id))
        .await
        .assert_ok()
        .assert_see("in");
}

/// Requests prune expired sessions now and then (1 in 50), in the
/// background.
#[renox::test]
async fn requests_prune_expired_sessions_now_and_then() {
    let app = TestApp::with_config(app(), |c| {
        c.session_driver = renox::SessionDriver::Database;
        c.env = renox::Environment::Local;
    })
    .await;
    renox::db::sql(
        "INSERT INTO sessions (id, user_id, payload, expires_at, last_activity) VALUES ('old', NULL, '{}', 1, 1)",
    )
    .execute(app.db())
    .await
    .unwrap();
    let expired = || async {
        renox::db::sql("SELECT COUNT(*) FROM sessions WHERE id = 'old'")
            .scalar::<i64>(app.db())
            .await
            .unwrap()
    };
    // The chance that 1,000 requests never prune is about 2 in a billion.
    for _ in 0..1000 {
        app.get("/get").await.assert_ok();
        if expired().await == 0 {
            break;
        }
    }
    for _ in 0..100 {
        if expired().await == 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(expired().await, 0, "the expired session is pruned");
}

/// "Remember me" sessions keep their lifetime when a test helper rewrites
/// the cookie.
#[renox::test]
async fn test_helpers_keep_a_remembered_session() {
    let app = TestApp::new(app()).await;
    let user = User::register(app.db(), "Fay", "fay@example.com", "password123")
        .await
        .unwrap();
    app.post(
        "/login",
        &[
            ("email", "fay@example.com"),
            ("password", "password123"),
            ("remember", "on"),
        ],
    )
    .await
    .assert_redirect("/");
    app.confirm_password();
    app.assert_authenticated(Some(&user));
    app.get("/whoami").await.assert_see("Fay");
    // Three hours on, past SESSION_LIFETIME (two hours): still logged in,
    // the cookie keeps REMEMBER_LIFETIME's minutes.
    app.travel(std::time::Duration::from_secs(3 * 3600));
    app.get("/whoami").await.assert_see("Fay");
}

/// Without "remember me" the same three hours end the session.
#[renox::test]
async fn a_session_not_remembered_ends_after_its_lifetime() {
    let app = TestApp::new(app()).await;
    User::register(app.db(), "Gus", "gus@example.com", "password123")
        .await
        .unwrap();
    app.post(
        "/login",
        &[("email", "gus@example.com"), ("password", "password123")],
    )
    .await
    .assert_redirect("/");
    app.get("/whoami").await.assert_see("Gus");
    app.travel(std::time::Duration::from_secs(3 * 3600));
    app.get("/whoami").await.assert_dont_see("Gus");
}

/// A session cookie that doesn't decrypt (tampered, or from another
/// APP_KEY) is ignored: the visitor is a guest with a fresh session.
#[renox::test]
async fn a_session_cookie_that_doesnt_decrypt_is_ignored() {
    let app = TestApp::new(app()).await;
    let res = app
        .request()
        .header("cookie", "renox_session=bm90IGVuY3J5cHRlZA")
        .get("/whoami")
        .await;
    res.assert_ok().assert_dont_see("Fay");
    assert!(
        res.header("set-cookie")
            .is_some_and(|c| c.starts_with("renox_session=")),
        "a fresh session is sent"
    );
}
