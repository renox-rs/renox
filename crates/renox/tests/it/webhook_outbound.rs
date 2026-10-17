//! Outbound webhooks: the endpoint model.

use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

use renox::http::FakeResponse;
use renox::webhook::outbound::{Delivery, Endpoint, EndpointDisabled, emit_in};
use renox::webhook::signing;

#[renox::test]
async fn endpoint_secret_is_shown_once_and_stored_sealed() {
    let app = TestApp::new(App::new()).await;
    let state = app.state();
    let (endpoint, secret) = Endpoint::create(state, "https://example.com/h", "Billing", &[])
        .await
        .unwrap();
    assert!(secret.starts_with("whsec_"));

    let stored: String = renox::db::sql("SELECT secret FROM webhook_endpoints WHERE id = ?")
        .bind(endpoint.id)
        .scalar(app.db())
        .await
        .unwrap();
    assert_ne!(stored, secret);
    assert!(!stored.contains(&secret));
    assert_eq!(endpoint.secret(state).unwrap(), secret);

    let found = Endpoint::find(app.db(), endpoint.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.url, "https://example.com/h");
    assert!(!format!("{found:?}").contains(&secret));
    assert_eq!(Endpoint::list(app.db()).await.unwrap().len(), 1);
    assert!(Endpoint::find(app.db(), 999).await.unwrap().is_none());
}

#[renox::test]
async fn endpoint_event_filter_enable_disable_and_delete() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let (all, _) = Endpoint::create(app.state(), "https://a.test", "", &[])
        .await
        .unwrap();
    let (some, _) = Endpoint::create(app.state(), "http://b.test", "", &["invoice.paid"])
        .await
        .unwrap();

    let ids = |list: Vec<Endpoint>| list.into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(
        ids(Endpoint::subscribed_to(db, "invoice.paid").await.unwrap()),
        [all.id, some.id]
    );
    assert_eq!(
        ids(Endpoint::subscribed_to(db, "user.created").await.unwrap()),
        [all.id]
    );

    assert!(Endpoint::disable(db, all.id, "manual").await.unwrap());
    let off = Endpoint::find(db, all.id).await.unwrap().unwrap();
    assert!(!off.enabled);
    assert_eq!(off.disabled_reason.as_deref(), Some("manual"));
    assert!(off.disabled_at.is_some());
    assert_eq!(
        ids(Endpoint::subscribed_to(db, "user.created").await.unwrap()),
        Vec::<i64>::new()
    );

    assert!(Endpoint::enable(db, all.id).await.unwrap());
    let on = Endpoint::find(db, all.id).await.unwrap().unwrap();
    assert!(on.enabled && on.disabled_at.is_none() && on.disabled_reason.is_none());

    assert!(
        Endpoint::set_events(db, some.id, &["user.created"])
            .await
            .unwrap()
    );
    let changed = Endpoint::find(db, some.id).await.unwrap().unwrap();
    assert_eq!(changed.events, ["user.created"]);

    assert!(Endpoint::delete(db, some.id).await.unwrap());
    assert!(!Endpoint::delete(db, some.id).await.unwrap());
    assert!(!Endpoint::enable(db, some.id).await.unwrap());
}

#[renox::test]
async fn endpoint_url_must_be_http() {
    let app = TestApp::new(App::new()).await;
    assert!(
        Endpoint::create(app.state(), "ftp://x.test", "", &[])
            .await
            .is_err()
    );
}

#[renox::test]
async fn disable_after_defaults_to_twenty() {
    let app = TestApp::new(App::new()).await;
    assert_eq!(app.state().config.webhook_disable_after, 20);
}

#[derive(Clone)]
struct InvoiceIssued {
    id: i64,
}
impl Event for InvoiceIssued {}

fn app() -> App {
    App::new().webhook_event::<InvoiceIssued>("invoice.issued", |e| json!({ "id": e.id }))
}

async fn statuses(app: &TestApp) -> Vec<String> {
    renox::db::sql("SELECT status FROM webhook_deliveries ORDER BY id")
        .fetch_all(app.db())
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("status").unwrap())
        .collect()
}

async fn create(app: &TestApp, url: &str, events: &[&str]) -> (Endpoint, String) {
    Endpoint::create(app.state(), url, "", events)
        .await
        .unwrap()
}

#[renox::test]
async fn emit_in_delivers_once_per_subscribed_endpoint_after_commit() {
    let app = TestApp::new(app()).await;
    let state = app.state();
    let http = app.fake_http();
    http.on("POST https://*", FakeResponse::status(200));
    let (_, secret) = create(&app, "https://a.test/h", &["invoice.issued"]).await;
    create(&app, "https://b.test/h", &["other"]).await;
    let (disabled, _) = create(&app, "https://c.test/h", &[]).await;
    Endpoint::disable(app.db(), disabled.id, "x").await.unwrap();

    let mut tx = app.db().begin().await.unwrap();
    let queued = emit_in(state, &mut tx, &InvoiceIssued { id: 7 })
        .await
        .unwrap();
    assert_eq!(queued, 1);
    tx.rollback().await.unwrap();
    app.run_jobs().await;
    http.assert_sent_count(0);

    let mut tx = app.db().begin().await.unwrap();
    emit_in(state, &mut tx, &InvoiceIssued { id: 7 })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    app.run_jobs().await;
    http.assert_sent_count(1);

    let sent = http.sent().remove(0);
    assert_eq!(sent.url, "https://a.test/h");
    let body: serde_json::Value = serde_json::from_str(&sent.body).unwrap();
    assert_eq!(body["type"], "invoice.issued");
    assert_eq!(body["data"], json!({ "id": 7 }));
    assert!(body["created_at"].as_str().unwrap().ends_with('Z'));
    let id = sent.header("webhook-id").unwrap();
    assert_eq!(body["id"], id);
    let timestamp = sent.header("webhook-timestamp").unwrap();
    let signature = sent.header("webhook-signature").unwrap();
    let tolerance = Duration::from_secs(300);
    assert!(signing::verify(
        &secret,
        id,
        timestamp,
        signature,
        sent.body.as_bytes(),
        tolerance
    ));
    assert_eq!(statuses(&app).await, ["succeeded"]);
}

#[renox::test]
async fn a_plain_emit_of_a_registered_event_delivers() {
    let app = TestApp::new(app()).await;
    let http = app.fake_http();
    http.on("POST https://a.test/*", FakeResponse::status(204));
    create(&app, "https://a.test/h", &[]).await;
    app.state().emit(InvoiceIssued { id: 1 }).await.unwrap();
    app.run_jobs().await;
    http.assert_sent_count(1);
}

#[renox::test]
async fn server_errors_are_retried_and_a_success_resets_the_count() {
    let app = TestApp::new(app()).await;
    let http = app.fake_http();
    http.on("POST https://a.test/*", FakeResponse::status(503));
    http.on("POST https://a.test/*", FakeResponse::status(200));
    let (endpoint, _) = create(&app, "https://a.test/h", &[]).await;
    app.state().emit(InvoiceIssued { id: 1 }).await.unwrap();
    app.run_jobs().await;
    assert_eq!(statuses(&app).await, ["retrying"]);
    let row = renox::db::sql("SELECT attempted_at, next_attempt_at FROM webhook_deliveries")
        .fetch_one(app.db())
        .await
        .unwrap();
    let (at, next): (i64, i64) = (
        row.try_get("attempted_at").unwrap(),
        row.try_get("next_attempt_at").unwrap(),
    );
    assert_eq!(next - at, 5);

    app.travel(Duration::from_secs(6));
    app.run_jobs().await;
    assert_eq!(statuses(&app).await, ["succeeded"]);
    let row = renox::db::sql("SELECT attempt, response_status FROM webhook_deliveries")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(row.try_get::<i64>("attempt").unwrap(), 2);
    assert_eq!(row.try_get::<i64>("response_status").unwrap(), 200);
    http.assert_sent_count(2);
    let found = Endpoint::find(app.db(), endpoint.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.consecutive_failures, 0);
}

#[renox::test]
async fn a_410_disables_the_endpoint_at_once() {
    let app = TestApp::new(app()).await;
    app.fake_http()
        .on("POST https://a.test/*", FakeResponse::status(410));
    let (endpoint, _) = create(&app, "https://a.test/h", &[]).await;
    app.state().emit(InvoiceIssued { id: 1 }).await.unwrap();
    app.fake_events();
    app.run_jobs().await;
    let found = Endpoint::find(app.db(), endpoint.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!found.enabled);
    assert_eq!(statuses(&app).await, ["failed"]);
    app.assert_emitted(|e: &EndpointDisabled| e.endpoint_id == endpoint.id);
}

#[renox::test]
async fn consecutive_failed_deliveries_disable_the_endpoint() {
    let app = TestApp::with_config(app(), |c| c.webhook_disable_after = 2).await;
    app.fake_http()
        .on("POST https://a.test/*", FakeResponse::status(400));
    let (endpoint, _) = create(&app, "https://a.test/h", &[]).await;
    for id in 1..=2 {
        let mut tx = app.db().begin().await.unwrap();
        emit_in(app.state(), &mut tx, &InvoiceIssued { id })
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    app.fake_events();
    app.run_jobs().await;
    assert_eq!(statuses(&app).await, ["failed", "failed"]);
    let found = Endpoint::find(app.db(), endpoint.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!found.enabled);
    assert_eq!(found.consecutive_failures, 2);
    app.assert_emitted(|e: &EndpointDisabled| e.endpoint_id == endpoint.id);
}

#[renox::test]
async fn deliveries_list_redeliver_and_prune() {
    let app = TestApp::new(app()).await;
    let http = app.fake_http();
    http.on("POST https://a.test/*", FakeResponse::status(400));
    http.on("POST https://a.test/*", FakeResponse::status(200));
    let (endpoint, _) = create(&app, "https://a.test/h", &[]).await;
    let (other, _) = create(&app, "https://b.test/h", &["nothing"]).await;
    app.state().emit(InvoiceIssued { id: 1 }).await.unwrap();
    app.run_jobs().await;
    assert_eq!(statuses(&app).await, ["failed"]);

    let db = app.db();
    assert_eq!(Delivery::list(db, None, true, 10).await.unwrap().len(), 1);
    let of_endpoint = Delivery::list(db, Some(endpoint.id), false, 10)
        .await
        .unwrap();
    assert_eq!(of_endpoint.len(), 1);
    assert_eq!(of_endpoint[0].response_status, Some(400));
    assert!(
        Delivery::list(db, Some(other.id), false, 10)
            .await
            .unwrap()
            .is_empty()
    );

    let first = http.sent().remove(0);
    let id = of_endpoint[0].id;
    assert!(Delivery::redeliver(app.state(), id).await.unwrap());
    app.run_jobs().await;
    assert_eq!(statuses(&app).await, ["succeeded"]);
    let again = http.sent().remove(1);
    assert_eq!(again.body, first.body);
    assert_eq!(again.header("webhook-id"), first.header("webhook-id"));
    assert!(!Delivery::redeliver(app.state(), 999).await.unwrap());

    assert_eq!(
        Delivery::prune(db, Duration::from_secs(86_400))
            .await
            .unwrap(),
        0
    );
    app.travel(Duration::from_secs(2 * 86_400));
    let pruned = app
        .at_travelled_time(Delivery::prune(db, Duration::from_secs(86_400)))
        .await
        .unwrap();
    assert_eq!(pruned, 1);
    assert!(
        Delivery::list(db, None, false, 10)
            .await
            .unwrap()
            .is_empty()
    );
}
