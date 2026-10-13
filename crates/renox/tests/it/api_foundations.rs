//! M14a: errors with any status, route groups and app commands.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use renox::command::Args;
use renox::prelude::*;
use renox::testing::TestApp;

struct Shop;

impl Module for Shop {
    fn name(&self) -> &'static str {
        "shop"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { "home" })
            .name("home")
            .get("/download", download)
            .get("/gone", || async {
                Err::<(), _>(abort(StatusCode::GONE, "This offer ended."))
            })
            .group(
                "/admin",
                "admin.",
                Routes::new()
                    .get("/", || async { "dashboard" })
                    .name("dashboard")
                    .get("/products/{id}", |Path(id): Path<i64>| async move {
                        format!("product {id}")
                    })
                    .name("products.show")
                    .require_auth(),
            )
            .group(
                "/api/v1",
                "api.",
                Routes::new()
                    .post("/ping", || async { "pong" })
                    .name("ping")
                    .without_csrf(),
            )
    }
}

#[derive(serde::Deserialize)]
struct Paid {
    paid: Option<bool>,
}

async fn download(Query(q): Query<Paid>) -> Result<&'static str> {
    abort_unless(
        q.paid == Some(true),
        StatusCode::PAYMENT_REQUIRED,
        "Pay for the order first.",
    )?;
    Ok("the file")
}

async fn app() -> TestApp {
    TestApp::new(App::new().module(Auth::new()).module(Shop)).await
}

#[renox::test]
async fn any_status_with_a_message_for_pages_and_json() {
    let app = app().await;
    let res = app.get("/download").await;
    res.assert_status(402)
        .assert_see("Pay for the order first.");
    app.get("/download?paid=true")
        .await
        .assert_ok()
        .assert_see("the file");

    let res = app
        .request()
        .header("accept", "application/json")
        .get("/gone")
        .await;
    res.assert_status(410);
    assert_eq!(
        res.json::<renox::serde_json::Value>()["message"],
        "This offer ended."
    );

    let err = abort(StatusCode::CONFLICT, "taken");
    assert_eq!(err.status(), StatusCode::CONFLICT);
    assert!(abort_if(false, StatusCode::CONFLICT, "x").is_ok());
}

#[renox::test]
async fn route_groups_prefix_paths_and_names() {
    let app = app().await;
    let state = app.state();
    assert_eq!(state.url("admin.dashboard", &[]).unwrap(), "/admin");
    assert_eq!(
        state.url("admin.products.show", &[&7]).unwrap(),
        "/admin/products/7"
    );
    assert_eq!(state.url("api.ping", &[]).unwrap(), "/api/v1/ping");

    // The group's guard covers only the group.
    app.get("/").await.assert_ok();
    app.get("/admin").await.assert_redirect("/login");
    app.get("/admin/products/7").await.assert_redirect("/login");
    let user = User::register(app.db(), "Alex", "alex@example.com", "letmein123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.get("/admin").await.assert_ok().assert_see("dashboard");
    app.get("/admin/products/7").await.assert_see("product 7");

    // `without_csrf` inside a group applies to the prefixed path.
    app.request()
        .without_csrf()
        .post("/api/v1/ping", &[])
        .await
        .assert_see("pong");

    let listed: Vec<_> = app
        .kernel()
        .routes()
        .iter()
        .filter(|r| r.module == "shop")
        .map(|r| {
            (
                r.method.as_str(),
                r.path.as_str(),
                r.name.clone(),
                r.middleware.clone(),
            )
        })
        .collect();
    assert!(listed.contains(&(
        "GET",
        "/admin",
        Some("admin.dashboard".into()),
        vec!["auth".into()]
    )));
    assert!(listed.contains(&(
        "POST",
        "/api/v1/ping",
        Some("api.ping".into()),
        vec!["no-csrf".into()]
    )));
}

#[test]
#[should_panic(expected = "must start with `/`")]
fn group_prefixes_must_be_paths() {
    let _ = Routes::new().group("admin", "admin.", Routes::new());
}

#[renox::test]
async fn app_commands_run_with_their_arguments() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let app = TestApp::new(App::new().module(Auth::new()).command(
        "admin:create",
        "Create an admin user",
        move |args: Args, state: AppState| {
            let seen = seen.clone();
            async move {
                let email = args
                    .value("--email")
                    .unwrap_or("admin@example.com")
                    .to_owned();
                User::register(&state.db, "Admin", &email, "letmein123").await?;
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
    ))
    .await;
    app.kernel()
        .call("admin:create", ["--email", "boss@example.com"])
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    app.assert_database_has("users", &[("email", &"boss@example.com")])
        .await;
    assert!(
        app.kernel()
            .call("nope", Vec::<String>::new())
            .await
            .is_err()
    );
}

#[renox::test]
async fn command_names_are_checked_at_boot() {
    async fn noop(_: Args, _: AppState) -> Result {
        Ok(())
    }
    for (name, why) in [("migrate", "built in"), ("two words", "not a valid")] {
        let err = App::new()
            .command(name, "", noop)
            .boot()
            .await
            .err()
            .map(|e| format!("{e:?}"))
            .unwrap_or_default();
        assert!(err.contains(why), "{name}: {err}");
    }
    let twice = App::new()
        .command("report", "", noop)
        .command("report", "", noop)
        .boot()
        .await;
    assert!(format!("{:?}", twice.err().unwrap()).contains("registered twice"));
}
