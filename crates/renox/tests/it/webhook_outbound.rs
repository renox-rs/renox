//! Outbound webhooks: the endpoint model.

use renox::prelude::*;
use renox::testing::TestApp;
use renox::webhook::outbound::Endpoint;

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
