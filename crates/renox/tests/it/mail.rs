use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use renox::Kernel;
use renox::auth::{Channel, Notification, Recipient};
use renox::mail::{Mail, MailConfig};
use renox::prelude::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;

fn config(dir: &std::path::Path) -> Config {
    {
        let mut c = Config::default();
        c.env = Environment::Testing;
        c.key = Some(renox::generate_key());
        c.views_path = dir.to_path_buf();
        c.name = "Coffee Shop".into();
        c
    }
}

async fn kernel_with(config: Config) -> Kernel {
    let kernel = App::with_config(config)
        .module(Auth::new())
        .boot()
        .await
        .unwrap();
    kernel.migrate().await.unwrap();
    kernel
}

fn views() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("mail")).unwrap();
    std::fs::write(
        dir.path().join("mail/receipt.html"),
        r#"{% extends "renox/mail/layout.html" %}{% from "renox/mail/button.html" import button %}
{% block content %}<h1>Thank you, {{ name }}</h1><table><tr><td>Total</td><td>${{ total }}</td></tr></table>{{ button("https://shop.test/o/1", "View order") }}{% endblock %}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mail/plain.html"),
        "<p>Only HTML for {{ name }}</p>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mail/plain.txt"),
        "Text version for {{ name }} from {{ app.name }}",
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn mail_views_render_html_and_text() {
    let dir = views();
    let kernel = kernel_with(config(dir.path())).await;
    let state = kernel.state();

    let mail = state
        .mail_view(
            "ben@example.com",
            "Receipt",
            "mail/receipt",
            context! { name => "Ben", total => "25.00" },
        )
        .unwrap();
    let html = mail.html.as_deref().unwrap();
    assert!(
        html.contains("<h1>Thank you, Ben</h1>") && html.contains("Coffee Shop"),
        "layout and app name"
    );
    assert!(html.contains(r#"href="https://shop.test/o/1""#));
    assert!(mail.text.contains("Thank you, Ben"), "{}", mail.text);
    assert!(mail.text.contains("Total $25.00"), "{}", mail.text);
    assert!(
        mail.text.contains("View order (https://shop.test/o/1)"),
        "{}",
        mail.text
    );
    assert!(!mail.text.contains('<'), "{}", mail.text);

    let plain = state
        .mail_view("a@b.test", "x", "mail/plain", context! { name => "Anna" })
        .unwrap();
    assert_eq!(
        plain.text, "Text version for Anna from Coffee Shop",
        "a .txt template wins"
    );
}

/// #509: a mail written with `<rx-mail>` and its parts is the same HTML, byte for byte, as the
/// `{% extends %}` version, and the framework's own auth mails (written with the tags) are
/// the same as they were with `{% extends %}`.
#[tokio::test]
async fn tagged_mails_render_byte_for_byte_like_the_template_statements() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("mail")).unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write(
        "mail/old.html",
        r#"{% extends "renox/mail/layout.html" %}
{% from "renox/mail/components.html" import button, panel, table, divider %}
{% block title %}Order {{ number }}{% endblock %}
{% block content %}
<p>Hi {{ name }}</p>
{{ button(link, "View") }}
{% call panel() %}Ships soon{% endcall %}
{{ table(rows, head=["Item", "Price"], total=["Total", "$3"]) }}
{{ divider() }}
{% endblock %}
{% block footer %}Bye{% endblock %}"#,
    );
    write(
        "mail/new.html",
        r#"<rx-mail subject="Order {{ number }}">
<p>Hi {{ name }}</p>
<rx-mail-button :url="link" label="View"/>
<rx-mail-panel>Ships soon</rx-mail-panel>
<rx-mail-table :rows="rows" :head="['Item', 'Price']" :total="['Total', '$3']"/>
<rx-mail-divider/>
<rx-slot name="footer">Bye</rx-slot></rx-mail>"#,
    );
    // The auth mails as they were before the tags.
    write(
        "mail/old-verify.html",
        r#"{% extends "renox/mail/layout.html" %}
{% from "renox/mail/button.html" import button %}
{% block content %}
<p>{{ text.mail_verify_intro }}</p>
{{ button(link, text.verify_title) }}
<p>{{ text.mail_verify_outro }}</p>
{% endblock %}
"#,
    );
    write(
        "mail/old-reset.html",
        r#"{% extends "renox/mail/layout.html" %}
{% from "renox/mail/button.html" import button %}
{% block content %}
<p>{{ text.mail_reset_intro }}</p>
{{ button(link, text.mail_reset_subject) }}
<p>{{ text.mail_reset_outro }}</p>
{% endblock %}
"#,
    );
    let kernel = kernel_with(config(dir.path())).await;
    let state = kernel.state();
    let ctx = context! {
        number => 7, name => "Ben", link => "https://shop.test/o/7",
        rows => vec![vec!["Coffee", "$1"], vec!["Tea", "$2"]],
        text => context! {
            mail_verify_intro => "Verify it", verify_title => "Verify",
            mail_verify_outro => "Thanks", mail_reset_intro => "Reset it",
            mail_reset_subject => "Reset", mail_reset_outro => "Bye",
        },
    };
    let html = |view: &str| {
        // Windows checks templates out with CRLF; the generated parts use LF.
        state
            .mail_view("a@b.test", "s", view, ctx.clone())
            .unwrap()
            .html
            .unwrap()
            .replace("\r\n", "\n")
    };
    let new = html("mail/new");
    assert_eq!(new, html("mail/old"));
    assert!(new.contains("<title>Order 7</title>"), "{new}");
    assert!(new.contains("Bye"), "{new}");
    assert_eq!(
        html("renox/mail/auth/verify-email"),
        html("mail/old-verify")
    );
    assert_eq!(
        html("renox/mail/auth/reset-password"),
        html("mail/old-reset")
    );
    assert!(html("renox/mail/auth/verify-email").contains(r#"href="https://shop.test/o/7""#));
}

#[tokio::test]
async fn queued_mail_is_sent_by_a_worker() {
    let dir = views();
    let kernel = kernel_with(config(dir.path())).await;
    kernel
        .state()
        .queue_mail(Mail::new("a@b.test", "Later", "body"))
        .await
        .unwrap();
    assert!(kernel.mailer().sent().is_empty());
    kernel.run_jobs().await.unwrap();
    assert_eq!(kernel.mailer().sent()[0].subject, "Later");
}

async fn get(kernel: &Kernel, uri: &str) -> (StatusCode, String) {
    let res = kernel
        .router()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn sent_mail_can_be_previewed_in_debug_only() {
    let dir = views();
    let kernel = kernel_with({
        let mut c = config(dir.path());
        c.debug = true;
        c
    })
    .await;
    let mail = Mail::new("ben@example.com", "Hello <Ben>", "text").html("<p>\"html\" & more</p>");
    kernel.mailer().send(mail).await.unwrap();

    let (status, list) = get(&kernel, "/_renox/mail").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        list.contains("Hello &lt;Ben&gt;") && list.contains(r#"href="/_renox/mail/1""#),
        "{list}"
    );
    let (_, page) = get(&kernel, "/_renox/mail/1").await;
    assert!(
        page.contains(
            r#"<iframe sandbox srcdoc="&lt;p&gt;&quot;html&quot; &amp; more&lt;/p&gt;">"#
        ),
        "{page}"
    );
    assert!(page.contains("<pre>text</pre>"));

    let full = Mail::new("ben@example.com", "Invoice", "see file")
        .cc("tim@example.com")
        .reply_to("hello@example.com")
        .attach("<inv>.pdf", "application/pdf", vec![1, 2, 3]);
    kernel.mailer().send(full).await.unwrap();
    let (_, page) = get(&kernel, "/_renox/mail/2").await;
    assert!(page.contains("<p>Cc: tim@example.com</p>"), "{page}");
    assert!(
        page.contains("<p>Reply-To: hello@example.com</p>"),
        "{page}"
    );
    assert!(
        page.contains("Attachments: &lt;inv&gt;.pdf (application/pdf, 3 bytes)"),
        "{page}"
    );
    assert_eq!(
        get(&kernel, "/_renox/mail/9").await.0,
        StatusCode::NOT_FOUND
    );

    let production = kernel_with({
        let mut c = config(dir.path());
        c.debug = false;
        c
    })
    .await;
    assert_eq!(
        get(&production, "/_renox/mail").await.0,
        StatusCode::NOT_FOUND
    );
}

/// Accepts one message, speaking just enough SMTP, and returns what was sent.
async fn fake_smtp() -> (u16, Arc<Mutex<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(String::new()));
    let log = received.clone();
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read, mut write) = socket.into_split();
        let mut lines = BufReader::new(read).lines();
        write.write_all(b"220 fake ESMTP\r\n").await.unwrap();
        let mut in_data = false;
        while let Ok(Some(line)) = lines.next_line().await {
            log.lock().unwrap().push_str(&format!("{line}\n"));
            let reply: &[u8] = if in_data {
                if line == "." {
                    in_data = false;
                    b"250 queued\r\n"
                } else {
                    continue;
                }
            } else if line.starts_with("EHLO") {
                b"250-fake\r\n250 8BITMIME\r\n"
            } else if line.starts_with("DATA") {
                in_data = true;
                b"354 go ahead\r\n"
            } else if line.starts_with("QUIT") {
                write.write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                b"250 ok\r\n"
            };
            write.write_all(reply).await.unwrap();
        }
    });
    (port, received)
}

#[tokio::test]
async fn smtp_sends_multipart_mail() {
    let dir = views();
    let (port, received) = fake_smtp().await;
    let smtp = {
        let mut mail = MailConfig::default();
        mail.mailer = renox::mail::MailDriver::Smtp;
        mail.host = "127.0.0.1".into();
        mail.port = Some(port);
        mail.encryption = renox::mail::MailEncryption::None;
        mail.from_address = "shop@example.com".into();
        mail
    };
    let kernel = kernel_with({
        let mut c = config(dir.path());
        c.mail = smtp;
        c
    })
    .await;
    let mail = kernel
        .state()
        .mail_view(
            "ben@example.com",
            "Order receipt",
            "mail/receipt",
            context! { name => "Ben", total => "1" },
        )
        .unwrap();
    kernel.mailer().send(mail).await.unwrap();

    let data = received.lock().unwrap().clone();
    assert!(data.contains("MAIL FROM:<shop@example.com>"), "{data}");
    assert!(data.contains("RCPT TO:<ben@example.com>"));
    assert!(
        data.contains("From: \"Coffee Shop\" <shop@example.com>")
            || data.contains("From: Coffee Shop <shop@example.com>"),
        "{data}"
    );
    assert!(data.contains("Subject: Order receipt"));
    assert!(data.contains("multipart/alternative"));
    assert!(data.contains("text/plain") && data.contains("text/html"));
}

/// A calendar invitation goes over SMTP as a `text/calendar` part with its
/// `method` (#48), so mail clients offer to add it.
#[tokio::test]
async fn smtp_sends_a_calendar_invitation() {
    use renox::calendar::{Event, Ics};
    use renox::chrono::{TimeZone, Utc};

    let dir = views();
    let (port, received) = fake_smtp().await;
    let smtp = {
        let mut mail = MailConfig::default();
        mail.mailer = renox::mail::MailDriver::Smtp;
        mail.host = "127.0.0.1".into();
        mail.port = Some(port);
        mail.encryption = renox::mail::MailEncryption::None;
        mail.from_address = "shop@example.com".into();
        mail
    };
    let kernel = kernel_with({
        let mut c = config(dir.path());
        c.mail = smtp;
        c
    })
    .await;
    let starts = Utc.with_ymd_and_hms(2026, 11, 3, 9, 0, 0).unwrap();
    let ics = Ics::new("-//Acme//Bookings//EN").event(
        Event::new(
            "booking-42@acme.test",
            starts,
            starts + renox::chrono::Duration::hours(1),
        )
        .summary("Service")
        .attendee("ben@example.com"),
    );
    kernel
        .mailer()
        .send(Mail::new("ben@example.com", "Your booking", "See you.").attach_ics(&ics))
        .await
        .unwrap();

    let data = received.lock().unwrap().clone();
    assert!(data.contains("Content-Type: text/calendar"), "{data}");
    assert!(data.contains("method=REQUEST"), "{data}");
    assert!(data.contains("invite.ics"), "{data}");
}

#[tokio::test]
async fn bad_mail_settings_fail_at_boot() {
    let dir = views();
    for (mail, expected) in [
        // An unknown MAIL_MAILER or MAIL_ENCRYPTION is refused when the
        // config is read (the config tests); a bad sender only when SMTP starts.
        (
            {
                let mut mail = MailConfig::default();
                mail.mailer = renox::mail::MailDriver::Smtp;
                mail.from_address = "not an address".into();
                mail
            },
            "MAIL_FROM_ADDRESS",
        ),
    ] {
        let err = App::with_config({
            let mut c = config(dir.path());
            c.mail = mail;
            c
        })
        .boot()
        .await
        .err()
        .unwrap();
        assert!(format!("{err:?}").contains(expected), "{err:?}");
    }
}

struct OrderShipped {
    order_id: i64,
}

impl Notification for OrderShipped {
    fn kind(&self) -> &'static str {
        "order-shipped"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Mail, Channel::Database]
    }

    fn to_mail(&self, to: &Recipient, _: &AppState) -> Result<Mail> {
        Ok(Mail::new(
            to.email().unwrap_or_default(),
            format!("Order #{} shipped", self.order_id),
            "On its way.",
        ))
    }

    fn to_database(&self, _: &Recipient, _state: &renox::AppState) -> Result<serde_json::Value> {
        Ok(serde_json::json!({ "order_id": self.order_id }))
    }
}

struct DatabaseOnly;

impl Notification for DatabaseOnly {
    fn kind(&self) -> &'static str {
        "promo"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Mail]
    }
}

#[tokio::test]
async fn notifications_go_to_mail_and_the_database() {
    let dir = views();
    let kernel = kernel_with(config(dir.path())).await;
    let (state, db) = (kernel.state(), kernel.db());
    let ben = User::register(db, "Ben", "ben@example.com", "secret123")
        .await
        .unwrap();
    let anna = User::register(db, "Anna", "anna@example.com", "secret123")
        .await
        .unwrap();

    state
        .notify(&ben, &OrderShipped { order_id: 7 })
        .await
        .unwrap();
    state
        .notify(&ben, &OrderShipped { order_id: 8 })
        .await
        .unwrap();
    assert_eq!(kernel.mailer().sent()[1].subject, "Order #8 shipped");

    let all = ben.notifications(db, 10).await.unwrap();
    assert_eq!((all.len(), all[0].kind.as_str()), (2, "order-shipped"));
    assert_eq!(all[0].data["order_id"], 8, "newest first");
    assert_eq!(ben.unread_notification_count(db).await.unwrap(), 2);

    assert!(
        !anna.mark_notification_read(db, all[0].id).await.unwrap(),
        "not Anna's"
    );
    assert!(ben.mark_notification_read(db, all[0].id).await.unwrap());
    assert_eq!(
        ben.unread_notifications(db).await.unwrap()[0].data["order_id"],
        7
    );
    assert_eq!(ben.mark_all_notifications_read(db).await.unwrap(), 1);
    assert_eq!(ben.unread_notification_count(db).await.unwrap(), 0);

    let err = state.notify(&anna, &DatabaseOnly).await.unwrap_err();
    assert!(format!("{err:?}").contains("notification `promo` has no mail version"));
}

#[tokio::test]
async fn auth_mails_are_html_with_a_text_version() {
    let dir = views();
    let kernel = kernel_with(config(dir.path())).await;
    User::register(kernel.db(), "Ben", "ben@example.com", "secret123")
        .await
        .unwrap();
    let router = kernel.router();
    let res = router
        .clone()
        .oneshot(
            Request::get("/forgot-password")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = res.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let page =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    let token = page
        .split(r#"name="_token" value=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    router
        .oneshot(
            Request::post("/forgot-password")
                .header("cookie", cookie)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!("_token={token}&email=ben@example.com")))
                .unwrap(),
        )
        .await
        .unwrap();

    let mail = kernel.mailer().sent().pop().unwrap();
    assert!(
        mail.html
            .as_deref()
            .unwrap()
            .contains("Reset your password</a>")
    );
    assert!(
        mail.text.starts_with("You asked to reset your password."),
        "{}",
        mail.text
    );
    assert!(
        mail.text
            .lines()
            .any(|l| l.starts_with("http://127.0.0.1:3000/reset-password/"))
    );
}

#[tokio::test]
async fn smtp_sends_cc_bcc_reply_to_from_and_attachments() {
    let dir = views();
    let (port, received) = fake_smtp().await;
    let kernel = kernel_with({
        let mut c = config(dir.path());
        c.mail.mailer = renox::mail::MailDriver::Smtp;
        c.mail.host = "127.0.0.1".into();
        c.mail.port = Some(port);
        c.mail.encryption = renox::mail::MailEncryption::None;
        c.mail.from_address = "shop@example.com".into();
        c
    })
    .await;
    let pdf = b"%PDF-1.7 invoice".to_vec();
    let mail = Mail::new("ben@example.com", "Invoice INV-001", "Attached.")
        .also_to("sara@example.com")
        .cc("sales@example.com")
        .bcc("archive@example.com")
        .reply_to("Coffee Shop <hello@example.com>")
        .from("Shop Billing <billing@example.com>")
        .attach("INV-001.pdf", "application/pdf", pdf.clone());
    kernel.mailer().send(mail.clone()).await.unwrap();

    let data = received.lock().unwrap().clone();
    for rcpt in [
        "ben@example.com",
        "sara@example.com",
        "sales@example.com",
        "archive@example.com",
    ] {
        assert!(
            data.contains(&format!("RCPT TO:<{rcpt}>")),
            "{rcpt}: {data}"
        );
    }
    assert!(data.contains("MAIL FROM:<billing@example.com>"), "{data}");
    assert!(data.contains("Cc: sales@example.com"), "{data}");
    assert!(!data.contains("Bcc:"), "bcc isn't a header: {data}");
    assert!(data.contains("Reply-To:") && data.contains("hello@example.com"));
    assert!(data.contains("multipart/mixed") && data.contains("application/pdf"));
    assert!(data.contains("INV-001.pdf"));
    let _ = pdf;

    // An invalid address anywhere fails the send, permanently (no retries).
    let err = kernel
        .mailer()
        .send(Mail::new("ben@example.com", "x", "y").cc("not an email"))
        .await
        .unwrap_err();
    assert!(err.is_permanent(), "{err:?}");
}

#[tokio::test]
async fn queued_mail_keeps_its_attachments() {
    let dir = views();
    let kernel = kernel_with({
        let mut c = config(dir.path());
        c.mail.mailer = renox::mail::MailDriver::Memory;
        c
    })
    .await;
    let bytes: Vec<u8> = (0..=255).collect();
    let mail = Mail::new("ben@example.com", "Data", "see attachment")
        .cc("tim@example.com")
        .attach("data.bin", "application/octet-stream", bytes.clone());
    kernel.state().queue_mail(mail).await.unwrap();
    kernel.run_jobs().await.unwrap();
    let sent = kernel.mailer().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].attachments[0].data, bytes);
    assert!(sent[0].is_for("tim@example.com"));
}

struct Shipped {
    order_id: i64,
}

impl Notification for Shipped {
    fn kind(&self) -> &'static str {
        "shipped"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![
            Channel::Mail,
            Channel::Database,
            Channel::Custom("whatsapp"),
        ]
    }

    fn to_mail(&self, to: &Recipient, _: &AppState) -> Result<Mail> {
        Ok(Mail::new(
            to.email().unwrap_or_default(),
            format!("Order #{} shipped", self.order_id),
            "On its way.",
        ))
    }

    fn to_database(&self, _: &Recipient, _state: &renox::AppState) -> Result<serde_json::Value> {
        Ok(serde_json::json!({ "order_id": self.order_id }))
    }

    fn to_channel(
        &self,
        channel: &str,
        _: &Recipient,
        _state: &renox::AppState,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::json!({ "channel": channel, "text": format!("#{} shipped", self.order_id) }))
    }
}

type Outbox = Arc<Mutex<Vec<(Option<String>, serde_json::Value)>>>;

async fn channel_kernel(dir: &std::path::Path) -> (Kernel, Outbox) {
    let outbox: Outbox = Arc::default();
    let sent = outbox.clone();
    let kernel = App::with_config(config(dir))
        .module(Auth::new())
        .migrations(&[renox::db::Migration::new(
            "20300101000000_add_phone_to_users",
            "ALTER TABLE users ADD COLUMN phone TEXT",
            None,
        )])
        .channel("whatsapp", move |to: Recipient, message, _| {
            let sent = sent.clone();
            async move {
                let phone = to.address("whatsapp").or_else(|| to.user()?.get("phone"));
                sent.lock().unwrap().push((phone, message));
                Ok(())
            }
        })
        .boot()
        .await
        .unwrap();
    kernel.migrate().await.unwrap();
    (kernel, outbox)
}

#[tokio::test]
async fn notifications_reach_custom_channels_and_people_without_accounts() {
    let dir = views();
    let (kernel, outbox) = channel_kernel(dir.path()).await;
    let (state, db) = (kernel.state(), kernel.db());
    let mut ben = User::register(db, "Ben", "ben@example.com", "secret123")
        .await
        .unwrap();
    ben.set(db, "phone", "+15550111").await.unwrap();

    state.notify(&ben, &Shipped { order_id: 7 }).await.unwrap();
    assert_eq!(ben.unread_notification_count(db).await.unwrap(), 1);
    assert!(
        kernel
            .mailer()
            .sent()
            .iter()
            .any(|m| m.is_for("ben@example.com"))
    );
    let (phone, message) = outbox.lock().unwrap()[0].clone();
    assert_eq!(phone.as_deref(), Some("+15550111"));
    assert_eq!(message["text"], "#7 shipped");

    // Someone without an account: mail and WhatsApp, no database row.
    let guest = Recipient::to("mail", "guest@example.com").and("whatsapp", "+15550122");
    state
        .notify(&guest, &Shipped { order_id: 8 })
        .await
        .unwrap();
    assert!(
        kernel
            .mailer()
            .sent()
            .iter()
            .any(|m| m.is_for("guest@example.com"))
    );
    assert_eq!(outbox.lock().unwrap()[1].0.as_deref(), Some("+15550122"));
    let rows: i64 = renox::db::sql("SELECT COUNT(*) FROM notifications")
        .scalar(db)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn queued_notifications_send_each_channel_as_its_own_job() {
    let dir = views();
    let (kernel, outbox) = channel_kernel(dir.path()).await;
    let (state, db) = (kernel.state(), kernel.db());
    let ben = User::register(db, "Ben", "ben@example.com", "secret123")
        .await
        .unwrap();
    let mails_before = kernel.mailer().sent().len();

    state
        .notify_later(&ben, &Shipped { order_id: 9 })
        .await
        .unwrap();
    // The database row is written at once; mail and WhatsApp wait for a worker.
    assert_eq!(ben.unread_notification_count(db).await.unwrap(), 1);
    assert_eq!(kernel.mailer().sent().len(), mails_before);
    assert!(outbox.lock().unwrap().is_empty());
    assert_eq!(state.queue.pending().await.unwrap(), 2);

    kernel.run_jobs().await.unwrap();
    assert!(
        kernel
            .mailer()
            .sent()
            .iter()
            .any(|m| m.subject == "Order #9 shipped")
    );
    assert_eq!(outbox.lock().unwrap().len(), 1);
    assert_eq!(state.queue.pending().await.unwrap(), 0);
}

struct Unknown;

impl Notification for Unknown {
    fn kind(&self) -> &'static str {
        "unknown"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Custom("pigeon")]
    }

    fn to_channel(
        &self,
        _: &str,
        _: &Recipient,
        _state: &renox::AppState,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::Value::Null)
    }
}

#[tokio::test]
async fn an_unregistered_channel_is_an_error() {
    let dir = views();
    let (kernel, _) = channel_kernel(dir.path()).await;
    let ben = User::register(kernel.db(), "Ben", "ben@example.com", "secret123")
        .await
        .unwrap();
    for result in [
        kernel.state().notify(&ben, &Unknown).await,
        kernel.state().notify_later(&ben, &Unknown).await,
    ] {
        let err = format!("{:?}", result.unwrap_err());
        assert!(err.contains("no `pigeon` notification channel"), "{err}");
    }
}

/// #521: a text mail written with `<rx-mail-text>` renders byte for byte what the template
/// statements did, with `&` and `<` left alone like in any `.txt` view.
#[tokio::test]
async fn tagged_text_mails_render_byte_for_byte_like_the_template_statements() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("mail")).unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    for name in ["old", "new"] {
        write(&format!("mail/{name}.html"), "<p>{{ title }}</p>");
    }
    write(
        "mail/old.txt",
        "{{ title }}\n\n{{ body }}\n{% for row in rows %}\n{{ row.label }}: {{ row.value }}{% endfor %}\n{% if url %}\n{{ action }}: {{ url }}{% endif %}\n",
    );
    write(
        "mail/new.txt",
        "<rx-mail-text>\n{{ title }}\n\n{{ body }}\n<template rx-for=\"row in rows\">\n{{ row.label }}: {{ row.value }}</template>\n<template rx-if=\"url\">\n{{ action }}: {{ url }}</template>\n</rx-mail-text>\n",
    );
    let kernel = kernel_with(config(dir.path())).await;
    let state = kernel.state();
    let text = |view: &str, url: Option<&str>| {
        state
            .mail_view(
                "a@b.test",
                "s",
                view,
                context! {
                    title => "Q&A <now>", body => "It's \"here\"",
                    rows => vec![
                        context! { label => "Store", value => "A & B" },
                        context! { label => "Items", value => "3" },
                    ],
                    url => url, action => "Open",
                },
            )
            .unwrap()
            .text
    };
    for url in [Some("https://shop.test/n/1?a=1&b=2"), None] {
        assert_eq!(text("mail/new", url), text("mail/old", url));
    }
    assert_eq!(
        text("mail/new", Some("https://x.test")),
        "Q&A <now>\n\nIt's \"here\"\n\nStore: A & B\nItems: 3\n\nOpen: https://x.test"
    );
}
