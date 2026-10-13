//! Device tokens (a kiosk without a user row), `RequestLocale::switch` and
//! `normalize_email`.

use renox::auth::{Device, DeviceToken};
use renox::axum::extract::Request;
use renox::axum::middleware::{Next, from_fn};
use renox::i18n::RequestLocale;
use renox::prelude::*;
use renox::testing::TestApp;

struct Kiosks;

impl Module for Kiosks {
    fn name(&self) -> &'static str {
        "kiosks"
    }

    fn routes(&self) -> Routes {
        let mut routes = Routes::new()
            .get("/kiosk/hello", |device: Device| async move {
                format!("{} {}", device.key(), device.id_of("kiosk").unwrap_or("-"))
            })
            .require_device();
        routes = routes
            .post("/kiosk/sales", |_: Device| async { "sold" })
            .require_device_ability("sales:create");
        routes
            .get("/me", |user: AuthUser| async move { user.email.clone() })
            .get(
                "/who",
                |user: Option<AuthUser>, device: Option<Device>| async move {
                    format!("{} {}", user.is_some(), device.is_some())
                },
            )
            .get("/lang", |lang: Lang| async move { lang.locale })
    }
}

async fn to_spanish(mut req: Request, next: Next) -> Response {
    RequestLocale::switch(&mut req, "es");
    next.run(req).await
}

async fn boot() -> TestApp {
    TestApp::new(App::new().module(Auth::new()).module(Kiosks)).await
}

fn bearer(plain: &str) -> String {
    format!("Bearer {plain}")
}

#[renox::test]
async fn a_device_token_acts_without_a_user() {
    let app = boot().await;
    let db = app.db();
    let made = DeviceToken::create(db, "kiosk:7", "front", Some(&["sales:create"]), None)
        .await
        .unwrap();
    assert!(made.plain.starts_with(&format!("d{}|", made.token.id)));
    assert_eq!(made.token.device, "kiosk:7");

    let get = |uri: &'static str, plain: String| {
        let app = &app;
        async move {
            app.request()
                .json()
                .header("authorization", &bearer(&plain))
                .get(uri)
                .await
        }
    };
    let hello = get("/kiosk/hello", made.plain.clone()).await;
    hello.assert_ok();
    assert_eq!(hello.text(), "kiosk:7 7");
    // No user: a user-only extractor answers 401, an optional one sees both.
    assert_eq!(get("/me", made.plain.clone()).await.status.as_u16(), 401);
    assert_eq!(get("/who", made.plain.clone()).await.text(), "false true");
    // CSRF does not apply to a token.
    app.request()
        .json()
        .header("authorization", &bearer(&made.plain))
        .post("/kiosk/sales", &[])
        .await
        .assert_ok();
    assert!(
        DeviceToken::for_device(db, "kiosk:7").await.unwrap()[0]
            .last_used_at
            .is_some()
    );

    // Wrong secret, unknown id, a user-style token and no token: not a device.
    let wrong = format!("d{}|nope", made.token.id);
    assert_eq!(get("/kiosk/hello", wrong).await.status.as_u16(), 401);
    assert_eq!(
        get("/kiosk/hello", "d999|x".into()).await.status.as_u16(),
        401
    );
    assert_eq!(
        app.request()
            .json()
            .get("/kiosk/hello")
            .await
            .status
            .as_u16(),
        401
    );
    // A user's token is not a device.
    let ana = User::register(db, "Ana", "ana@example.com", "password123")
        .await
        .unwrap();
    let user_token = ana.create_token(db, "cli", None).await.unwrap();
    assert_eq!(
        get("/kiosk/hello", user_token.plain.clone())
            .await
            .status
            .as_u16(),
        401
    );
    assert_eq!(get("/me", user_token.plain).await.text(), "ana@example.com");
}

#[renox::test]
async fn device_abilities_expiry_and_revocation() {
    let app = boot().await;
    let db = app.db();
    let read = DeviceToken::create(db, "kiosk:1", "read", Some(&["stock:read"]), None)
        .await
        .unwrap();
    let all = DeviceToken::create(db, "kiosk:1", "all", None, None)
        .await
        .unwrap();
    let other = DeviceToken::create(db, "kiosk:2", "other", None, None)
        .await
        .unwrap();
    let post = |plain: String| {
        let app = &app;
        async move {
            app.request()
                .json()
                .header("authorization", &bearer(&plain))
                .post("/kiosk/sales", &[])
                .await
                .status
                .as_u16()
        }
    };
    assert_eq!(post(read.plain.clone()).await, 403);
    assert_eq!(post(all.plain.clone()).await, 200);

    // Expired tokens stop working and are pruned (by `tokens:prune` too).
    let soon = DeviceToken::create(
        db,
        "kiosk:3",
        "soon",
        None,
        Some(renox::db::now() - renox::chrono::TimeDelta::days(2)),
    )
    .await
    .unwrap();
    assert_eq!(post(soon.plain).await, 401);
    app.kernel()
        .call("tokens:prune", Vec::<String>::new())
        .await
        .unwrap();
    assert!(
        DeviceToken::for_device(db, "kiosk:3")
            .await
            .unwrap()
            .is_empty()
    );

    // Revoking is per device.
    assert!(
        !DeviceToken::revoke(db, "kiosk:2", read.token.id)
            .await
            .unwrap()
    );
    assert!(
        DeviceToken::revoke(db, "kiosk:1", read.token.id)
            .await
            .unwrap()
    );
    assert_eq!(post(read.plain).await, 401);
    assert_eq!(DeviceToken::revoke_all(db, "kiosk:1").await.unwrap(), 1);
    assert_eq!(post(all.plain).await, 401);
    assert_eq!(post(other.plain).await, 200);
    assert_eq!(
        DeviceToken::prune_expired(db, std::time::Duration::from_secs(60))
            .await
            .unwrap(),
        0
    );
}

#[renox::test]
async fn a_layer_switches_the_language_of_the_request() {
    let app = TestApp::with_config(App::new().module(Kiosks).layer(from_fn(to_spanish)), |c| {
        c.locale = "en".into()
    })
    .await;
    assert_eq!(app.request().get("/lang").await.text(), "es");
    assert_eq!(RequestLocale::new("fr").as_str(), "fr");
}

#[test]
fn email_normalisation_is_public() {
    assert_eq!(
        renox::auth::normalize_email("  Ana@Example.COM "),
        "ana@example.com"
    );
}
