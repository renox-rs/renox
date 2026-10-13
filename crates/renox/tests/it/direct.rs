//! Direct tests for APIs the other tests only reach on the way to something
//! else: session helpers, htmx response headers, custom rules, raw SQL
//! variants and signed URLs.

use std::time::Duration;

use renox::prelude::*;
use renox::signed::ValidSignature;
use renox::testing::TestApp;

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/session/put", |s: Session| async move {
                s.put("cart", 3).unwrap();
                s.put("coupon", "SAVE10").unwrap();
                s.flash("status", "Saved.").unwrap();
                "put"
            })
            .get("/session/read", |s: Session| async move {
                format!(
                    "cart={:?} has_coupon={} status={:?}",
                    s.get::<i64>("cart"),
                    s.has("coupon"),
                    s.flashed().get("status").cloned()
                )
            })
            .get("/session/pull", |s: Session| async move {
                let coupon: Option<String> = s.pull("coupon");
                let removed = s.remove("cart");
                format!(
                    "pulled={coupon:?} removed={removed:?} again={:?}",
                    s.get::<String>("coupon")
                )
            })
            .get("/session/reflash", |s: Session| async move {
                s.reflash();
                "kept"
            })
            .get("/session/longer", |s: Session| async move {
                s.set_lifetime(std::time::Duration::from_secs(60 * 60 * 24 * 7));
                "a week"
            })
            .get("/session/token", |s: Session| async move {
                let before = s.token();
                s.regenerate_token();
                format!("{}", before != s.token())
            })
            .get("/hx/redirect", || async { HxRedirect("/done".into()) })
            .get("/hx/refresh", || async { HxRefresh })
            .get(
                "/download/{file}",
                |_: ValidSignature, Path(file): Path<String>| async move { format!("file {file}") },
            )
            .name("download")
    }
}

async fn app() -> TestApp {
    TestApp::new(App::new().module(Pages)).await
}

fn session_cookie(res: &renox::testing::TestResponse) -> String {
    res.headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .find(|c| c.starts_with("renox_session="))
        .unwrap()
}

#[renox::test]
async fn session_helpers() {
    let app = app().await;
    app.get("/session/put").await.assert_ok();
    app.get("/session/read")
        .await
        .assert_see("cart=Some(3) has_coupon=true status=Some(String(\"Saved.\"))");
    // The flash is gone on the request after next, unless reflashed.
    app.get("/session/read").await.assert_see("status=None");

    app.get("/session/put").await;
    app.get("/session/reflash").await; // reads the flash and keeps it
    app.get("/session/read").await.assert_see("status=Some");

    app.get("/session/pull")
        .await
        .assert_see("pulled=Some(\"SAVE10\") removed=Some(Number(3)) again=None");
    app.get("/session/read")
        .await
        .assert_see("cart=None has_coupon=false");

    let week = session_cookie(&app.get("/session/longer").await);
    assert!(
        week.contains(&format!("Max-Age={}", 60 * 24 * 7 * 60)),
        "{week}"
    );
    let default = session_cookie(&app.get("/session/read").await);
    assert!(
        default.contains(&format!("Max-Age={}", 60 * 24 * 7 * 60)),
        "the lifetime stays: {default}"
    );

    app.get("/session/token").await.assert_see("true");
}

#[renox::test]
async fn htmx_response_headers() {
    let app = app().await;
    app.get("/hx/redirect")
        .await
        .assert_header("hx-redirect", "/done");
    app.get("/hx/refresh")
        .await
        .assert_header("hx-refresh", "true");
}

#[renox::test]
async fn custom_rules_and_raw_sql_variants() {
    let app = app().await;
    let db = app.db();
    let mut v = Validator::new();
    v.field("stock", &5)
        .rule(5 <= 10, "never")
        .rule(5 > 10, "Not enough stock.")
        .rule(false, "only the first failure counts");
    v.field("name", &"")
        .rule(false, "a rule still runs on a missing value");
    let errors = v.finish(db).await.unwrap();
    let stock: Vec<_> = errors
        .iter()
        .filter(|(f, _)| *f == "stock")
        .flat_map(|(_, m)| m.to_vec())
        .collect();
    assert_eq!(stock, ["Not enough stock."]);
    assert!(errors.has("name"));

    renox::db::sql(
        "CREATE TABLE notes (id BIGINT PRIMARY KEY, body TEXT NOT NULL, stars BIGINT NOT NULL)",
    )
    .execute(db)
    .await
    .unwrap();
    // Plain SQL on both databases: no auto-increment, BIGINT for i64 binds.
    let values = vec![
        renox::db::DbValue::Integer(1),
        renox::db::DbValue::Text("coffee".into()),
        renox::db::DbValue::Integer(5),
    ];
    renox::db::sql("INSERT INTO notes (id, body, stars) VALUES (?, ?, ?)")
        .bind_all(values)
        .execute(db)
        .await
        .unwrap();
    let found = renox::db::sql("SELECT body FROM notes WHERE stars = ?")
        .bind(5)
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.try_get::<String>("body").unwrap());
    assert_eq!(found.as_deref(), Some("coffee"));
    let none = renox::db::sql("SELECT body FROM notes WHERE stars = ?")
        .bind(1)
        .fetch_optional(db)
        .await
        .unwrap();
    assert!(none.is_none());
    let missing = renox::db::sql("SELECT body FROM notes WHERE stars = 1")
        .fetch_one(db)
        .await
        .unwrap_err();
    assert!(missing.is_row_not_found());
}

#[renox::test]
async fn signed_urls_refuse_tampering_and_expire() {
    let app = app().await;
    let state = app.state();
    let url = state
        .signed_url("download", &[&"report.pdf"], Duration::from_secs(60))
        .unwrap();
    let path = url.trim_start_matches(&state.config.url);
    app.get(path)
        .await
        .assert_ok()
        .assert_see("file report.pdf");

    for forged in [
        path.replace("report.pdf", "salary.pdf"),  // another file
        path.replace("expires=", "expires=9"),     // a later expiry
        path.replace("signature=", "signature=0"), // a changed signature
        path.split("&signature=").next().unwrap().to_owned(), // no signature
        "/download/report.pdf".to_owned(),         // not signed at all
    ] {
        app.get(&forged).await.assert_forbidden();
    }

    let gone = state
        .sign_path("/download/report.pdf", Duration::ZERO)
        .unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    app.get(&gone).await.assert_forbidden();

    let other = TestApp::new(App::new().module(Pages)).await;
    other.get(path).await.assert_forbidden(); // signed with another APP_KEY
}
