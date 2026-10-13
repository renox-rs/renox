//! M20c: the HTTP client (real and faked), schedule pings, storage listing,
//! localized mail and notifications, mail components, the queue dashboard.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use renox::auth::{Channel, Notification, Recipient};
use renox::http::FakeResponse;
use renox::mail::Mail;
use renox::prelude::*;
use renox::testing::TestApp;

/// A local server: `/flaky` fails twice with 503, `/slow` takes 2 s.
async fn server() -> (String, Arc<AtomicUsize>) {
    use axum::routing::{get, post};
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let router = axum::Router::new()
        .route(
            "/flaky",
            get(move || {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n < 2 {
                        (
                            axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            "busy".to_owned(),
                        )
                    } else {
                        (axum::http::StatusCode::OK, r#"{"rate":16000}"#.to_owned())
                    }
                }
            }),
        )
        .route(
            "/echo",
            post(|headers: axum::http::HeaderMap, body: String| async move {
                let auth = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_owned();
                format!("{auth}|{body}")
            }),
        )
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                "late"
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    (url, hits)
}

#[renox::test]
async fn the_http_client_talks_to_a_real_server() {
    let app = TestApp::new(App::new()).await;
    let http = &app.state().http;
    let (url, hits) = server().await;

    #[derive(serde::Deserialize)]
    struct Rate {
        rate: i64,
    }
    let rate: Rate = http
        .get(format!("{url}/flaky"))
        .retry(3, Duration::from_millis(10))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!((rate.rate, hits.load(Ordering::SeqCst)), (16000, 3));

    let echo = http
        .post(format!("{url}/echo"))
        .bearer("s3cret")
        .json(&json!({ "a": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(echo.text(), r#"Bearer s3cret|{"a":1}"#);

    let started = std::time::Instant::now();
    let slow = http
        .get(format!("{url}/slow"))
        .timeout(Duration::from_millis(200))
        .send()
        .await;
    assert!(slow.is_err() && started.elapsed() < Duration::from_secs(1));
    assert!(http.get("http://127.0.0.1:1/").send().await.is_err());
}

#[renox::test]
async fn faked_requests_and_schedule_pings() {
    let app = TestApp::new(App::new().schedule(|s| {
        s.hourly("report", |state| async move {
            let res = state.http.get("https://api.test/report").send().await?;
            res.error_for_status()?;
            Ok(())
        })
        .ping_before("https://ping.test/start")
        .ping_on_success("https://ping.test/ok")
        .ping_on_failure("https://ping.test/fail")
        .then_ping("https://ping.test/done");
    }))
    .await;
    let http = app.fake_http();
    http.on("https://ping.test/*", FakeResponse::status(200));
    http.on(
        "https://api.test/report",
        FakeResponse::json(200, json!({})),
    );
    http.on("https://api.test/report", FakeResponse::status(500));

    app.kernel().run_scheduled("report").await.unwrap();
    assert!(app.kernel().run_scheduled("report").await.is_err());
    let urls: Vec<String> = http.sent().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls,
        [
            "https://ping.test/start",
            "https://api.test/report",
            "https://ping.test/done",
            "https://ping.test/ok",
            "https://ping.test/start",
            "https://api.test/report",
            "https://ping.test/done",
            "https://ping.test/fail",
        ]
    );
    // A request without a fake fails instead of reaching the network.
    assert!(
        app.state()
            .http
            .get("https://example.com")
            .send()
            .await
            .is_err()
    );
}

#[renox::test]
async fn storage_lists_copies_and_moves() {
    let app = TestApp::new(App::new()).await;
    let storage = &app.state().storage;
    for (key, body) in [
        ("invoices/2026/a.pdf", "aa"),
        ("invoices/2026/q4/b.pdf", "bbb"),
        ("invoices/2025/c.pdf", "c"),
        ("public/logo.png", "png"),
    ] {
        storage.put(key, body.into()).await.unwrap();
    }
    let keys = |files: Vec<renox::storage::FileInfo>| -> Vec<(String, u64)> {
        files.into_iter().map(|f| (f.key, f.size)).collect()
    };
    assert_eq!(
        keys(storage.list("invoices/2026").await.unwrap()),
        [
            ("invoices/2026/a.pdf".to_owned(), 2),
            ("invoices/2026/q4/b.pdf".to_owned(), 3)
        ]
    );
    assert_eq!(storage.list("").await.unwrap().len(), 4);
    assert!(storage.list("nothing/here").await.unwrap().is_empty());
    assert!(storage.list("../etc").await.is_err());
    assert!(
        storage
            .list("invoices/2025/c.pdf")
            .await
            .unwrap()
            .is_empty(),
        "a file isn't a folder"
    );

    storage
        .copy("public/logo.png", "public/old/logo.png")
        .await
        .unwrap();
    storage
        .rename("invoices/2025/c.pdf", "archive/c.pdf")
        .await
        .unwrap();
    assert!(!storage.exists("invoices/2025/c.pdf").await.unwrap());
    assert_eq!(
        storage.get("archive/c.pdf").await.unwrap().as_deref(),
        Some(&b"c"[..])
    );
    assert_eq!(storage.size("public/old/logo.png").await.unwrap(), Some(3));
    assert_eq!(storage.size("public/none.png").await.unwrap(), None);
    assert!(matches!(
        storage.copy("missing", "x").await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        storage.rename("missing", "x").await,
        Err(Error::NotFound)
    ));
    assert!(storage.copy("public/logo.png", "../out").await.is_err());

    assert_eq!(storage.delete_all("invoices").await.unwrap(), 2);
    assert!(storage.delete_all("").await.is_err());
    assert!(storage.delete_all("/").await.is_err());
    assert_eq!(storage.list("").await.unwrap().len(), 3);
}

struct Shipped {
    order: i64,
}

impl Notification for Shipped {
    fn kind(&self) -> &'static str {
        "shipped"
    }

    fn channels(&self, to: &Recipient) -> Vec<Channel> {
        match to.address("whatsapp") {
            Some(_) => vec![Channel::Mail, Channel::Custom("whatsapp")],
            None => vec![Channel::Mail],
        }
    }

    fn to_mail(&self, to: &Recipient, state: &AppState) -> Result<Mail> {
        let subject = state
            .current_lang()
            .t("shipped.subject", &[("order", &self.order)]);
        state.mail_view(
            to.email().unwrap_or_default(),
            subject,
            "mail/shipped",
            context! { order => self.order },
        )
    }

    fn to_channel(
        &self,
        _: &str,
        to: &Recipient,
        _state: &renox::AppState,
    ) -> Result<renox::serde_json::Value> {
        Ok(json!({ "text": to.locale().unwrap_or_else(|| "en".into()) }))
    }
}

fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("views/mail")).unwrap();
    std::fs::create_dir_all(dir.path().join("lang")).unwrap();
    std::fs::write(
        dir.path().join("views/mail/shipped.html"),
        r#"{% extends "renox/mail/layout.html" %}
{% from "renox/mail/components.html" import button, panel, table, divider %}
{% block content %}<p>{{ t('shipped.body', order=order) }} ({{ app.locale }})</p>
{% call panel() %}{{ t('shipped.panel') }}{% endcall %}
{{ table([["Coffee", "4.50"]], head=["Item", "USD"], total=["Total", "4.50"]) }}{{ divider() }}
{{ button("https://shop.test/o/" ~ order, t('shipped.track')) }}{% endblock %}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lang/en.json"),
        r#"{"shipped": {"subject": "Order :order shipped", "body": "Order :order is on its way", "panel": "Arrives tomorrow", "track": "Track it"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lang/es.json"),
        r#"{"shipped": {"subject": "Pedido :order enviado", "body": "El pedido :order va en camino", "panel": "Llega mañana", "track": "Seguir"}}"#,
    )
    .unwrap();
    dir
}

#[renox::test]
async fn notifications_speak_the_recipients_language() {
    let dir = site();
    let root = dir.path().to_path_buf();
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = sent.clone();
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .channel("whatsapp", move |_, message, _| {
                let log = log.clone();
                async move {
                    log.lock()
                        .unwrap()
                        .push(message["text"].as_str().unwrap_or_default().to_owned());
                    Ok(())
                }
            }),
        |c| {
            c.views_path = root.join("views");
            c.lang_path = root.join("lang");
        },
    )
    .await;
    let state = app.state();
    renox::db::sql("ALTER TABLE users ADD COLUMN locale TEXT")
        .execute(app.db())
        .await
        .unwrap();
    let ana = User::register(app.db(), "Ana", "ana@example.test", "password-123")
        .await
        .unwrap();
    renox::db::sql("UPDATE users SET locale = 'es' WHERE id = ?")
        .bind(ana.id)
        .execute(app.db())
        .await
        .unwrap();
    let ana = User::find_or_404(app.db(), ana.id).await.unwrap();

    state.notify(&ana, &Shipped { order: 7 }).await.unwrap();
    let guest = Recipient::to("mail", "guest@example.test").and("whatsapp", "+15550111");
    state.notify(&guest, &Shipped { order: 8 }).await.unwrap();
    state
        .notify(&guest.clone().in_locale("es"), &Shipped { order: 9 })
        .await
        .unwrap();

    let mail = app.sent_mail();
    assert_eq!(mail[0].subject, "Pedido 7 enviado");
    let html = mail[0].html.clone().unwrap();
    assert!(html.contains("El pedido 7 va en camino (es)"), "{html}");
    assert!(html.contains("Llega mañana") && html.contains(">Seguir<") && html.contains("4.50"));
    assert!(
        mail[0].text.contains("El pedido 7 va en camino"),
        "{}",
        mail[0].text
    );
    assert_eq!(mail[1].subject, "Order 8 shipped");
    assert!(mail[1].html.as_deref().unwrap().contains("(en)"));
    assert_eq!(mail[2].subject, "Pedido 9 enviado");
    // Per-recipient channels: only the guests had WhatsApp, each in their language.
    assert_eq!(*sent.lock().unwrap(), ["en", "es"]);

    // Outside a notification: the app's language, or the one asked for.
    assert_eq!(state.current_lang().t("shipped.track", &[]), "Track it");
    assert_eq!(state.lang("es").t("shipped.track", &[]), "Seguir");
    let direct = state
        .mail_view_in(
            "es",
            "x@example.test",
            "s",
            "mail/shipped",
            context! { order => 1 },
        )
        .unwrap();
    assert!(direct.html.unwrap().contains("(es)"));
}

async fn admin(app: &TestApp, email: &str) -> User {
    User::register(app.db(), "Admin", email, "password-123")
        .await
        .unwrap()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Boom;

impl Job for Boom {
    const NAME: &'static str = "boom";
    const MAX_ATTEMPTS: u32 = 1;

    async fn handle(self, _: JobContext) -> Result {
        Err(abort(StatusCode::BAD_GATEWAY, "upstream exploded"))
    }
}

#[renox::test]
async fn the_queue_dashboard_is_gated_and_acts() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .module(renox::queue::Dashboard)
            .job::<Boom>()
            .gate(renox::queue::DASHBOARD_GATE, |user| {
                user.email.starts_with("ops@")
            }),
    )
    .await;
    let queue = &app.state().queue;
    queue.dispatch(Boom).await.unwrap();
    queue.dispatch(Boom).await.unwrap();
    app.run_jobs().await;
    queue
        .dispatch_after(Boom, Duration::from_secs(600))
        .await
        .unwrap();
    queue.batch("nightly").push(Boom).dispatch().await.unwrap();

    app.get("/_renox/queue").await.assert_status(303);
    let someone = admin(&app, "someone@example.test").await;
    app.acting_as(&someone);
    app.get("/_renox/queue").await.assert_status(403);

    let ops = admin(&app, "ops@example.test").await;
    app.acting_as(&ops);
    let page = app.get("/_renox/queue").await;
    page.assert_ok()
        .assert_see("Failed jobs")
        .assert_see("upstream exploded")
        .assert_see("nightly")
        .assert_see("hx-trigger=\"every 5s\"");
    let stats = queue.stats().await.unwrap();
    assert_eq!(stats.failed_total, 2);
    assert_eq!(stats.failed_last_hour, 2);
    assert_eq!(stats.queues[0].ready, 1);
    assert_eq!(stats.queues[0].delayed, 1);
    assert_eq!(stats.oldest_wait.map(|w| w < 5), Some(true));

    let failed = queue.failed().await.unwrap();
    app.post(
        &format!("/_renox/queue/failed/{}/forget", failed[0].id),
        &[],
    )
    .await
    .assert_status(303);
    app.post(&format!("/_renox/queue/failed/{}/retry", failed[1].id), &[])
        .await
        .assert_status(303);
    assert!(queue.failed().await.unwrap().is_empty());
    assert_eq!(queue.pending().await.unwrap(), 3, "delayed, batch, retried");
    app.run_jobs().await;
    app.post("/_renox/queue/failed/retry-all", &[])
        .await
        .assert_status(303);
    assert!(queue.failed().await.unwrap().is_empty());
}
