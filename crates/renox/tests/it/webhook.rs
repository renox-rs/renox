//! Webhooks: verified, stored once per event, processed in the queue, and
//! still received in maintenance mode.

use std::sync::atomic::{AtomicBool, Ordering};

use renox::prelude::*;
use renox::testing::TestApp;
use renox::webhook;
use serde::Deserialize;

const SECRET: &str = "whsec_test";

/// A provider that signs the body with HMAC-SHA256 in `X-Signature`.
struct Pay;

#[derive(Deserialize)]
struct Event {
    id: String,
    order: String,
}

/// Lets a test make processing fail, then succeed.
static BROKEN: AtomicBool = AtomicBool::new(false);

impl Webhook for Pay {
    const PROVIDER: &'static str = "pay";

    fn verify(request: &WebhookRequest, _: &AppState) -> Result {
        let signature = request.header("x-signature").unwrap_or_default();
        webhook::ensure(webhook::verify_hmac_sha256(
            SECRET,
            &request.body,
            signature,
        ))
    }

    fn event_id(request: &WebhookRequest) -> Result<String> {
        Ok(request.json::<Event>()?.id)
    }

    async fn handle(call: WebhookCall, ctx: JobContext) -> Result {
        let event: Event = call.json()?;
        if event.order == "broken" && BROKEN.load(Ordering::SeqCst) {
            return Err(Error::BadRequest("the shop is closed".into()));
        }
        ctx.state
            .cache
            .put(&format!("paid:{}", event.order), &true, None)
            .await
    }
}

struct Shop;

impl Module for Shop {
    fn name(&self) -> &'static str {
        "shop"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { "home" })
            .name("home")
            .webhook::<Pay>("/webhooks/pay")
    }

    fn register(&self, app: &mut Registry) {
        app.webhook::<Pay>();
    }
}

async fn app() -> TestApp {
    TestApp::new(App::new().module(Shop)).await
}

fn body(id: &str, order: &str) -> String {
    format!(r#"{{"id":"{id}","order":"{order}"}}"#)
}

async fn send(app: &TestApp, body: &str, signature: &str) -> renox::testing::TestResponse {
    app.request()
        .without_csrf()
        .header("x-signature", signature)
        .post_body("/webhooks/pay", "application/json", body)
        .await
}

async fn paid(app: &TestApp, order: &str) -> bool {
    app.state()
        .cache
        .get::<bool>(&format!("paid:{order}"))
        .await
        .unwrap()
        .unwrap_or(false)
}

#[renox::test]
async fn verified_calls_are_stored_once_and_processed_in_the_queue() {
    let app = app().await;
    let call = body("evt_1", "A-1");
    let signature = webhook::hmac_sha256_hex(SECRET, &call);

    send(&app, &call, &signature)
        .await
        .assert_ok()
        .assert_see("ok");
    app.assert_database_has(
        "webhook_calls",
        &[
            ("provider", &"pay"),
            ("event_id", &"evt_1"),
            ("status", &"received"),
        ],
    )
    .await;
    assert!(!paid(&app, "A-1").await, "processed later, by a worker");
    assert_eq!(app.queued_jobs().await, ["renox:webhook"]);

    // The provider retries: answered 200, not stored or queued again.
    send(&app, &call, &signature)
        .await
        .assert_ok()
        .assert_see("already received");
    app.assert_database_count("webhook_calls", 1).await;
    assert_eq!(app.queued_jobs().await.len(), 1);

    app.run_jobs().await;
    assert!(paid(&app, "A-1").await);
    app.assert_database_has(
        "webhook_calls",
        &[("event_id", &"evt_1"), ("status", &"processed")],
    )
    .await;
}

#[renox::test]
async fn forged_or_incomplete_calls_are_refused() {
    let app = app().await;
    let call = body("evt_2", "A-2");
    send(&app, &call, &webhook::hmac_sha256_hex("guess", &call))
        .await
        .assert_status(401);
    send(&app, &call, "").await.assert_status(401);
    let no_id = r#"{"order":"A-2"}"#;
    send(&app, no_id, &webhook::hmac_sha256_hex(SECRET, no_id))
        .await
        .assert_status(400);
    app.assert_database_count("webhook_calls", 0).await;
}

#[renox::test]
async fn failed_calls_can_be_retried() {
    let app = app().await;
    let call = body("evt_3", "broken");
    BROKEN.store(true, Ordering::SeqCst);
    send(&app, &call, &webhook::hmac_sha256_hex(SECRET, &call))
        .await
        .assert_ok();
    app.run_jobs().await;
    let failed = webhook::WebhookCall::failed(app.db()).await.unwrap();
    assert_eq!(failed.len(), 1);
    assert!(
        failed[0]
            .error
            .as_deref()
            .unwrap()
            .contains("the shop is closed")
    );

    BROKEN.store(false, Ordering::SeqCst);
    assert!(webhook::retry(app.state(), failed[0].id).await.unwrap());
    // The first job is still waiting for its retry backoff; the new one runs now.
    app.run_jobs().await;
    assert!(paid(&app, "broken").await);
    assert!(
        webhook::WebhookCall::failed(app.db())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!webhook::retry(app.state(), 999).await.unwrap());
}

#[renox::test]
async fn webhooks_work_in_maintenance_mode_and_show_in_route_list() {
    let app = app().await;
    renox::maintenance::down(
        &app.state().config.storage_path,
        renox::maintenance::DownOptions::new(),
    )
    .unwrap();
    app.get("/").await.assert_status(503);
    let call = body("evt_4", "A-4");
    send(&app, &call, &webhook::hmac_sha256_hex(SECRET, &call))
        .await
        .assert_ok();

    let route = app
        .kernel()
        .routes()
        .iter()
        .find(|r| r.path == "/webhooks/pay")
        .unwrap();
    assert_eq!(route.name.as_deref(), Some("webhooks.pay"));
    assert_eq!(route.middleware, ["no-csrf", "webhook:pay"]);
}

#[renox::test]
async fn a_webhook_route_needs_its_registration() {
    struct Forgetful;

    impl Module for Forgetful {
        fn name(&self) -> &'static str {
            "forgetful"
        }

        fn routes(&self) -> Routes {
            Routes::new().webhook::<Pay>("/webhooks/pay")
        }
    }

    let err = App::with_config(Config::default())
        .module(Forgetful)
        .boot()
        .await
        .err()
        .unwrap();
    assert!(
        format!("{err:?}").contains("add `app.webhook::<…>()`"),
        "{err:?}"
    );
}

/// #256: a provider without an event id in the body, and the processing job
/// meeting a call whose provider is gone.
#[renox::test]
async fn calls_without_an_id_and_calls_for_a_provider_thats_gone() {
    let app = app().await;
    let body = r#"{"id": "  ", "order": "A-1"}"#;
    send(&app, body, &webhook::hmac_sha256_hex(SECRET, body))
        .await
        .assert_status(400)
        .assert_see("the webhook has no event id");

    // A stored call whose provider this app no longer registers.
    let body = r#"{"id": "evt_gone", "order": "A-2"}"#;
    send(&app, body, &webhook::hmac_sha256_hex(SECRET, body))
        .await
        .assert_ok();
    renox::db::sql("UPDATE webhook_calls SET provider = ?")
        .bind("gone")
        .execute(app.db())
        .await
        .unwrap();
    app.run_jobs().await;
    let id: i64 = renox::db::sql("SELECT id FROM webhook_calls")
        .scalar(app.db())
        .await
        .unwrap();
    let call = WebhookCall::find(app.db(), id).await.unwrap().unwrap();
    assert_eq!(call.status, webhook::WebhookStatus::Failed);
    assert_eq!(
        call.error.as_deref(),
        Some("no webhook `gone` is registered")
    );
    assert_eq!(call.text().unwrap(), body);
    assert!(call.form::<Vec<(String, String)>>().is_ok());

    // A missing secret names the variable.
    let err = webhook::secret(app.state(), "PAY_SECRET").unwrap_err();
    assert!(
        format!("{err:?}").contains("set PAY_SECRET in .env"),
        "{err:?}"
    );
}

/// A provider that posts a form, read with `WebhookRequest::form`.
struct FormPay;

impl Webhook for FormPay {
    const PROVIDER: &'static str = "form-pay";

    fn verify(_: &WebhookRequest, _: &AppState) -> Result {
        Ok(())
    }

    fn event_id(request: &WebhookRequest) -> Result<String> {
        Ok(request.form::<Event>()?.id)
    }

    async fn handle(_call: WebhookCall, _ctx: JobContext) -> Result {
        Ok(())
    }
}

struct FormShop;

impl Module for FormShop {
    fn name(&self) -> &'static str {
        "form-shop"
    }

    fn routes(&self) -> Routes {
        Routes::new().webhook::<FormPay>("/webhooks/form-pay")
    }

    fn register(&self, app: &mut Registry) {
        app.webhook::<FormPay>();
    }
}

/// #256: form bodies, a store that can't be written (the provider sends
/// again), and the job meeting a call that is gone or already processed.
#[renox::test]
async fn form_calls_failed_stores_and_calls_done_meanwhile() {
    let app = TestApp::new(App::new().module(Shop).module(FormShop)).await;
    let post_form = |body: &'static str| {
        let app = &app;
        async move {
            app.request()
                .without_csrf()
                .post_body(
                    "/webhooks/form-pay",
                    "application/x-www-form-urlencoded",
                    body,
                )
                .await
        }
    };
    post_form("id=evt_f1&order=F-1")
        .await
        .assert_ok()
        .assert_see("ok");
    // Not a form with an `id`: refused.
    post_form("order=F-2").await.assert_status(400);

    // Gone before its job ran, and processed before its job ran: both jobs
    // end quietly.
    let gone = body("evt_gone", "G-1");
    send(&app, &gone, &webhook::hmac_sha256_hex(SECRET, &gone))
        .await
        .assert_ok();
    let done = body("evt_done", "D-1");
    send(&app, &done, &webhook::hmac_sha256_hex(SECRET, &done))
        .await
        .assert_ok();
    renox::db::sql("DELETE FROM webhook_calls WHERE event_id LIKE ?")
        .bind("%evt_gone")
        .execute(app.db())
        .await
        .unwrap();
    renox::db::sql("UPDATE webhook_calls SET status = 'processed' WHERE event_id LIKE ?")
        .bind("%evt_done")
        .execute(app.db())
        .await
        .unwrap();
    app.run_jobs().await;
    assert!(!paid(&app, "G-1").await && !paid(&app, "D-1").await);
    app.assert_database_count("failed_jobs", 0).await;

    // A call that failed before (its first try) is handled again by its job
    // and ends processed.
    let again = body("evt_again", "A-1");
    send(&app, &again, &webhook::hmac_sha256_hex(SECRET, &again))
        .await
        .assert_ok();
    renox::db::sql(
        "UPDATE webhook_calls SET status = 'failed', error = 'earlier' WHERE event_id LIKE ?",
    )
    .bind("%evt_again")
    .execute(app.db())
    .await
    .unwrap();
    app.run_jobs().await;
    assert!(paid(&app, "A-1").await);
    let status: String = renox::db::sql("SELECT status FROM webhook_calls WHERE event_id LIKE ?")
        .bind("%evt_again")
        .scalar(app.db())
        .await
        .unwrap();
    assert_eq!(status, "processed");

    // The table can't be written: a 500, so the provider sends it again.
    renox::db::sql("ALTER TABLE webhook_calls RENAME TO webhook_calls_away")
        .execute(app.db())
        .await
        .unwrap();
    let later = body("evt_later", "L-1");
    send(&app, &later, &webhook::hmac_sha256_hex(SECRET, &later))
        .await
        .assert_status(500);
}

#[renox::test]
async fn a_webhook_registered_twice_stops_the_boot() {
    struct Twice;

    impl Module for Twice {
        fn name(&self) -> &'static str {
            "twice"
        }

        fn register(&self, app: &mut Registry) {
            app.webhook::<Pay>();
            app.webhook::<Pay>();
        }
    }

    let err = App::with_config(Config::default())
        .module(Twice)
        .boot()
        .await
        .err()
        .unwrap();
    assert!(
        format!("{err:?}").contains("`pay` is registered twice"),
        "{err:?}"
    );
}

/// The routes only: the app itself registers the webhook.
struct PayRoutes;

impl Module for PayRoutes {
    fn name(&self) -> &'static str {
        "pay-routes"
    }

    fn routes(&self) -> Routes {
        Routes::new().webhook::<Pay>("/webhooks/pay")
    }
}

/// `App::webhook` registers a webhook without a module's `register`.
#[renox::test]
async fn the_app_can_register_a_webhook_itself() {
    let app = TestApp::new(App::new().webhook::<Pay>().module(PayRoutes)).await;
    let call = body("evt_app", "P-1");
    send(&app, &call, &webhook::hmac_sha256_hex(SECRET, &call))
        .await
        .assert_ok();
    app.run_jobs().await;
    assert!(paid(&app, "P-1").await);
}
