//! `App::channel_rate_limit` and `App::mailer_rate_limit`: queued deliveries
//! over the limit wait for the next window without using up an attempt.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use renox::CacheStore;
use renox::auth::{Channel, Notification, Recipient};
use renox::mail::{Mail, MailConfig, MailDriver};
use renox::prelude::*;
use renox::testing::TestApp;

struct Ping(u32);

impl Notification for Ping {
    fn kind(&self) -> &'static str {
        "ping"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Custom("sms")]
    }

    fn to_channel(&self, _: &str, _: &Recipient, _: &AppState) -> Result<serde_json::Value> {
        Ok(serde_json::json!({ "n": self.0 }))
    }
}

/// A limit of 2 per hour (the window is moved with `travel`, so the test
/// doesn't depend on where in a second it runs).
const PER: Duration = Duration::from_secs(3600);

async fn sms_app(store: CacheStore) -> (TestApp, Arc<Mutex<Vec<u64>>>) {
    let sent: Arc<Mutex<Vec<u64>>> = Arc::default();
    let log = sent.clone();
    let app = TestApp::with_config(
        App::new()
            .channel("sms", move |_to, message, _| {
                let log = log.clone();
                async move {
                    log.lock().unwrap().push(message["n"].as_u64().unwrap());
                    Ok(())
                }
            })
            .channel_rate_limit("sms", 2, PER),
        move |c| c.cache_store = store,
    )
    .await;
    (app, sent)
}

async fn count(app: &TestApp, sql: &str) -> i64 {
    renox::db::sql(sql.to_owned())
        .scalar(app.db())
        .await
        .unwrap()
}

async fn channel_sends_run_two_two_one(store: CacheStore) {
    let (app, sent) = sms_app(store).await;
    let to = Recipient::to("sms", "+15550100");
    for n in 1..=5 {
        app.state().notify_later(&to, &Ping(n)).await.unwrap();
    }
    app.run_jobs().await;
    assert_eq!(*sent.lock().unwrap(), [1, 2]);
    // Held back, not failed: no attempt used.
    assert_eq!(count(&app, "SELECT COUNT(*) FROM jobs").await, 3);
    assert_eq!(count(&app, "SELECT MAX(attempts) FROM jobs").await, 0);
    assert_eq!(count(&app, "SELECT COUNT(*) FROM failed_jobs").await, 0);

    app.travel(PER);
    app.run_jobs().await;
    assert_eq!(sent.lock().unwrap().len(), 4);
    assert_eq!(count(&app, "SELECT MAX(attempts) FROM jobs").await, 0);

    app.travel(PER);
    app.run_jobs().await;
    assert_eq!(*sent.lock().unwrap(), [1, 2, 3, 4, 5]);
    assert_eq!(count(&app, "SELECT COUNT(*) FROM jobs").await, 0);
    assert_eq!(count(&app, "SELECT COUNT(*) FROM failed_jobs").await, 0);
}

#[renox::test]
async fn queued_channel_sends_run_two_two_one() {
    channel_sends_run_two_two_one(CacheStore::Memory).await;
}

#[renox::test]
async fn channel_limits_work_with_the_database_cache() {
    channel_sends_run_two_two_one(CacheStore::Database).await;
}

fn bulk(_: &Config) -> Result<MailConfig> {
    let mut config = MailConfig::default();
    config.mailer = MailDriver::Memory;
    Ok(config)
}

#[renox::test]
async fn queued_mail_on_a_named_mailer_runs_two_two_one() {
    let app = TestApp::new(
        App::new()
            .mailer("bulk", bulk)
            .mailer_rate_limit("bulk", 2, PER),
    )
    .await;
    let state = app.state();
    for n in 1..=5 {
        state
            .queue_mail_via("bulk", Mail::new("a@example.com", format!("m{n}"), "t"))
            .await
            .unwrap();
    }
    // Mail through another mailer isn't held back by `bulk`'s limit.
    state
        .queue_mail(Mail::new("b@example.com", "default", "t"))
        .await
        .unwrap();
    let sent = || state.mailer_named("bulk").unwrap().sent().len();
    app.run_jobs().await;
    assert_eq!(sent(), 2);
    assert_eq!(app.sent_mail().len(), 1);
    assert_eq!(count(&app, "SELECT MAX(attempts) FROM jobs").await, 0);
    app.travel(PER);
    app.run_jobs().await;
    assert_eq!(sent(), 4);
    app.travel(PER);
    app.run_jobs().await;
    assert_eq!(sent(), 5);
    assert_eq!(count(&app, "SELECT COUNT(*) FROM failed_jobs").await, 0);
}

#[renox::test]
async fn queued_mail_on_the_default_mailer_can_be_limited_too() {
    let app = TestApp::new(App::new().mailer_rate_limit("default", 2, PER)).await;
    for n in 1..=3 {
        app.state()
            .queue_mail(Mail::new("a@example.com", format!("m{n}"), "t"))
            .await
            .unwrap();
    }
    app.run_jobs().await;
    assert_eq!(app.sent_mail().len(), 2);
    app.travel(PER);
    app.run_jobs().await;
    assert_eq!(app.sent_mail().len(), 3);
}

#[renox::test]
async fn limits_for_unknown_channels_and_mailers_stop_the_boot() {
    let err = App::new()
        .channel_rate_limit("sms", 1, PER)
        .boot()
        .await
        .err()
        .expect("boot fails");
    assert!(format!("{err:?}").contains("names `sms`"), "{err:?}");
    let err = App::new()
        .mailer_rate_limit("bulk", 1, PER)
        .boot()
        .await
        .err()
        .expect("boot fails");
    assert!(format!("{err:?}").contains("names `bulk`"), "{err:?}");
}
