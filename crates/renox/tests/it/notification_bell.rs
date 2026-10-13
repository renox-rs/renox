//! Toasts with a body, actions and a duration; the in-app notification
//! list (`Auth::new().notifications()`): `DatabaseMessage`, the page and the
//! bell's panel, the actions on it, and the Server-Sent Events stream.

use std::time::Duration;

use renox::ToastAction;
use renox::auth::{Channel, DatabaseMessage, Notification, Recipient};
use renox::prelude::*;
use renox::testing::TestApp;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct OrderShipped {
    order: i64,
}

impl Notification for OrderShipped {
    fn kind(&self) -> &'static str {
        "order-shipped"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Database]
    }

    fn to_database(
        &self,
        _: &Recipient,
        _state: &renox::AppState,
    ) -> Result<renox::serde_json::Value> {
        Ok(
            DatabaseMessage::success(format!("Order #{} shipped", self.order))
                .body("It arrives in 2–3 days.")
                .url(format!("/orders/{}", self.order))
                .link("Track", "https://track.example.com/7")
                .action(ToastAction::link("Bad", "javascript:alert(1)"))
                .action(ToastAction::delete(
                    "Cancel",
                    format!("/orders/{}", self.order),
                ))
                .action(ToastAction::post("Steal", "https://evil.example/x"))
                .with("order_id", self.order)
                .into(),
        )
    }
}

/// Data that isn't a `DatabaseMessage`: the list falls back to its keys.
struct Plain;

impl Notification for Plain {
    fn kind(&self) -> &'static str {
        "weekly-report"
    }

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Database]
    }

    fn to_database(
        &self,
        _: &Recipient,
        _state: &renox::AppState,
    ) -> Result<renox::serde_json::Value> {
        Ok(json!({ "message": "Your weekly report is ready" }))
    }
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "bell-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/bell", || async { view("bell.html", context! {}) })
            .post("/save", || async {
                (
                    Toast::success("Order <7> placed")
                        .body("Thanks!")
                        .link("View", "/orders/7")
                        .seconds(8)
                        .id("order-7"),
                    Redirect::to("/bell"),
                )
            })
            .post("/save-htmx", || async {
                (
                    Toast::info("Saved").action(ToastAction::event("Undo", "undo")),
                    "ok",
                )
            })
            .post("/archive", || async {
                Toast::info("Archived").action(ToastAction::delete("Undo", "/archive"))
            })
            // What the toast's "Undo" sends: an htmx request, answered with a toast.
            .delete("/archive", || async { Toast::success("Restored") })
    }
}

const BELL: &str = r#"{% from "renox/ui.html" import notification_bell %}<nav>{{ notification_bell(unread_notifications) }}</nav>{{ toasts(position="bottom-end") }}"#;

async fn app(notifications: bool, with_layout: bool) -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bell.html"), BELL).unwrap();
    if with_layout {
        std::fs::create_dir_all(dir.path().join("layouts")).unwrap();
        std::fs::write(
            dir.path().join("layouts/app.html"),
            "<html><body class=\"app-layout\">{% block content %}{% endblock %}</body></html>",
        )
        .unwrap();
    }
    let auth = if notifications {
        Auth::new().notifications()
    } else {
        Auth::new()
    };
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(auth).module(Pages), move |c| {
        c.views_path = path
    })
    .await;
    (app, dir)
}

async fn user(app: &TestApp, email: &str) -> User {
    User::register(app.db(), "Ana", email, "password123")
        .await
        .unwrap()
}

#[renox::test]
async fn toasts_carry_a_body_actions_a_duration_and_a_position() {
    let (app, _dir) = app(false, false).await;
    app.post("/save", &[]).await.assert_redirect("/bell");
    app.get("/bell")
        .await
        .assert_see(r#"class="rx-toasts rx-toasts--bottom-end""#)
        .assert_see(r#"<p class="rx-toast__message">Order &lt;7&gt; placed</p><p class="rx-toast__body">Thanks!</p>"#)
        .assert_see(r#"<a class="rx-toast__action" href="/orders/7" data-renox-dismiss>View</a>"#)
        .assert_see(r#"data-duration="8000" data-toast-id="order-7""#)
        // A guest gets no bell, and without `.notifications()` there is no share.
        .assert_dont_see("rx-bell");
    let res = app.htmx().post("/save-htmx", &[]).await;
    let trigger = res.header("hx-trigger").unwrap().to_owned();
    assert!(
        trigger.contains(r#""actions":[{"event":"undo","label":"Undo"}]"#),
        "{trigger}"
    );
    assert!(
        !trigger.contains("body") && !trigger.contains("duration"),
        "{trigger}"
    );

    // A request action: its method and path go to the page's script.
    let res = app.htmx().post("/archive", &[]).await;
    res.assert_status(204);
    let trigger = res.header("hx-trigger").unwrap().to_owned();
    assert!(
        trigger.contains(r#""actions":[{"label":"Undo","method":"DELETE","url":"/archive"}]"#),
        "{trigger}"
    );
    // The request it sends (htmx, with the CSRF token TestApp adds): its answer's toast.
    let res = app.htmx().delete("/archive").await;
    res.assert_status(204);
    assert!(res.header("hx-trigger").unwrap().contains("Restored"));
    // On the next page (a plain form post) it is a button carrying the same.
    app.post("/archive", &[]).await;
    app.get("/bell").await.assert_see(
        r#"<button type="button" class="rx-toast__action" data-rx-request="/archive" data-rx-method="DELETE" data-renox-dismiss>Undo</button>"#,
    );
}

#[renox::test]
async fn the_list_shows_marks_deletes_and_opens_notifications() {
    let (app, _dir) = app(true, true).await;
    let ana = user(&app, "ana@example.com").await;
    let ben = user(&app, "ben@example.com").await;
    app.state()
        .notify(&ana, &OrderShipped { order: 7 })
        .await
        .unwrap();
    app.state().notify(&ana, &Plain).await.unwrap();
    app.state()
        .notify(&ben, &OrderShipped { order: 9 })
        .await
        .unwrap();
    let ids: Vec<i64> = ana
        .notifications(app.db(), 10)
        .await
        .unwrap()
        .iter()
        .map(|n| n.id)
        .collect();
    let (plain, shipped) = (ids[0], ids[1]);
    let stored = &ana.notifications(app.db(), 10).await.unwrap()[1];
    let message = stored.message().unwrap();
    assert_eq!(message.title, "Order #7 shipped");
    assert_eq!(stored.data["order_id"], 7);

    // Guests are sent to log in.
    app.get("/notifications").await.assert_redirect("/login");

    app.acting_as(&ana);
    app.get("/bell")
        .await
        .assert_see(r#"data-stream="/notifications/stream""#)
        .assert_see(r#"aria-label="Notifications, 2 unread""#)
        .assert_see(
            r#"<span class="rx-bell__badge" data-rx-bell-count aria-hidden="true">2</span>"#,
        );
    let page = app.get("/notifications").await;
    page.assert_ok()
        .assert_view("renox/notifications.html")
        .assert_see(r#"class="app-layout""#)
        .assert_see(r#"data-unread="2""#)
        .assert_see(">Order #7 shipped</button>")
        .assert_see("It arrives in 2–3 days.")
        .assert_see(r#"href="https://track.example.com/7""#)
        // A request action is a form (sent by renox-ui.js with htmx, or plainly
        // without script); one to another site isn't shown.
        .assert_see(r#"<form method="post" action="/orders/7"><input type="hidden" name="_token""#)
        .assert_see(r#"<input type="hidden" name="_method" value="DELETE"><button class="rx-link rx-link--button" type="submit" data-rx-request="/orders/7" data-rx-method="DELETE">Cancel</button></form>"#)
        .assert_dont_see("evil.example")
        .assert_see("<p class=\"rx-notification__title\">Your weekly report is ready</p>")
        .assert_see("rx-notification--success rx-notification--unread")
        .assert_see("just now</time>")
        .assert_dont_see("javascript")
        .assert_dont_see("Order #9");
    // The bell's panel: the list alone.
    let panel = app.htmx().get("/notifications").await;
    panel
        .assert_ok()
        .assert_dont_see("app-layout")
        .assert_see("data-rx-notifications");

    // Mark read and unread, from the panel (htmx: the list again) or a page.
    app.htmx()
        .post(&format!("/notifications/{plain}/read"), &[])
        .await
        .assert_see(r#"data-unread="1""#);
    app.post(&format!("/notifications/{plain}/unread"), &[])
        .await
        .assert_status(303);
    assert_eq!(ana.unread_notification_count(app.db()).await.unwrap(), 2);
    // Someone else's notification is out of reach.
    let theirs = ben.notifications(app.db(), 1).await.unwrap()[0].id;
    app.post(&format!("/notifications/{theirs}/read"), &[])
        .await;
    app.post(&format!("/notifications/{theirs}/open"), &[])
        .await
        .assert_not_found();
    app.delete(&format!("/notifications/{theirs}")).await;
    assert_eq!(ben.unread_notification_count(app.db()).await.unwrap(), 1);

    // Opening one marks it read and goes where it points.
    app.post(&format!("/notifications/{shipped}/open"), &[])
        .await
        .assert_redirect("/orders/7");
    app.post(&format!("/notifications/{plain}/open"), &[])
        .await
        .assert_redirect("/notifications");
    assert_eq!(ana.unread_notification_count(app.db()).await.unwrap(), 0);
    app.post(&format!("/notifications/{plain}/unread"), &[])
        .await;
    app.htmx()
        .post("/notifications/read-all", &[])
        .await
        .assert_see(r#"data-unread="0""#);

    app.htmx()
        .delete(&format!("/notifications/{plain}"))
        .await
        .assert_ok();
    assert_eq!(ana.notifications(app.db(), 10).await.unwrap().len(), 1);
    app.htmx()
        .delete("/notifications")
        .await
        .assert_see("No notifications");
    assert!(ana.notifications(app.db(), 10).await.unwrap().is_empty());
    assert_eq!(ben.notifications(app.db(), 10).await.unwrap().len(), 1);
}

#[renox::test]
async fn the_list_pages_and_falls_back_to_renox_layout() {
    let (app, _dir) = app(true, false).await;
    let ana = user(&app, "ana@example.com").await;
    for _ in 0..25 {
        app.state().notify(&ana, &Plain).await.unwrap();
    }
    app.acting_as(&ana);
    let first = app.get("/notifications").await;
    first.assert_see("rx-auth").assert_see(">Older</a>");
    assert_eq!(
        first.text().matches("<li class=\"rx-notification ").count(),
        20
    );
    let oldest_shown = ana.notifications(app.db(), 20).await.unwrap()[19].id;
    let second = app
        .get(&format!("/notifications?before={oldest_shown}"))
        .await;
    assert_eq!(
        second
            .text()
            .matches("<li class=\"rx-notification ")
            .count(),
        5
    );
    second
        .assert_see(">Newest</a>")
        .assert_dont_see(">Older</a>");
}

#[renox::test]
async fn without_the_option_there_are_no_routes() {
    let (app, _dir) = app(false, true).await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.get("/notifications").await.assert_not_found();
}

#[renox::test]
async fn the_list_speaks_the_apps_language() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bell.html"), BELL).unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new().module(Auth::new().notifications()).module(Pages),
        move |c| {
            c.views_path = path;
            c.locale = "es".into();
            c.lang_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lang");
        },
    )
    .await;
    let ana = user(&app, "ana@example.com").await;
    app.state().notify(&ana, &Plain).await.unwrap();
    app.acting_as(&ana);
    app.get("/bell")
        .await
        .assert_see(r#"aria-label="Notificaciones, 1 sin leer""#);
    app.get("/notifications")
        .await
        .assert_see("Marcar todo como leído")
        .assert_see("justo ahora</time>")
        .assert_see("Borrar todo");
}

/// Reads the stream until `needle` shows up (or 5 s pass).
async fn read_until(stream: &mut tokio::net::TcpStream, seen: &mut String, needle: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut buf = [0u8; 4096];
    while !seen.contains(needle) {
        let n = tokio::time::timeout_at(deadline, stream.read(&mut buf))
            .await
            .unwrap_or_else(|_| panic!("no `{needle}` in:\n{seen}"))
            .unwrap();
        assert!(n > 0, "the stream ended before `{needle}`:\n{seen}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
}

#[renox::test]
async fn new_notifications_arrive_over_server_sent_events() {
    let (app, _dir) = app(true, true).await;
    let ana = user(&app, "ana@example.com").await;
    let ben = user(&app, "ben@example.com").await;
    app.acting_as(&ana);
    let url = app.serve().await;
    let cookie = app.session_cookie().unwrap();
    let mut stream = tokio::net::TcpStream::connect(url.trim_start_matches("http://"))
        .await
        .unwrap();
    stream
        .write_all(
            format!(
                "GET /notifications/stream HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\nCookie: {cookie}\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut seen = String::new();
    read_until(&mut stream, &mut seen, "event: count\ndata: 0\n").await;
    assert!(seen.contains("content-type: text/event-stream"), "{seen}");

    // Someone else's notification says nothing here; Ana's arrives at once.
    app.state()
        .notify(&ben, &OrderShipped { order: 9 })
        .await
        .unwrap();
    app.state()
        .notify(&ana, &OrderShipped { order: 7 })
        .await
        .unwrap();
    read_until(&mut stream, &mut seen, "event: count\ndata: 1\n").await;
    assert!(seen.contains("event: notification\n"), "{seen}");
    assert!(seen.contains(r#""title":"Order #7 shipped""#), "{seen}");
    assert!(seen.contains(r#""url":"/orders/7""#), "{seen}");
    assert!(!seen.contains("Order #9"), "{seen}");

    // Reading it elsewhere (another tab) updates the count.
    let id = ana.notifications(app.db(), 1).await.unwrap()[0].id;
    app.post(&format!("/notifications/{id}/read"), &[]).await;
    read_until(&mut stream, &mut seen, "event: count\ndata: 0\n\n").await;

    // The app's own events: Ben's don't come here, everyone's and Ana's do.
    let state = app.state();
    state
        .broadcast_to(ben.id, "order-updated", json!({ "id": 9 }))
        .unwrap();
    state
        .broadcast("order-updated", json!({ "id": 7, "status": "paid" }))
        .unwrap();
    state
        .broadcast_to(
            ana.id,
            "renox:toast",
            json!({ "toasts": [Toast::info("Export ready")] }),
        )
        .unwrap();
    read_until(&mut stream, &mut seen, "Export ready").await;
    assert!(
        seen.contains(
            "event: broadcast\ndata: {\"event\":\"order-updated\",\"data\":{\"id\":7,\"status\":\"paid\"}}\n"
        ),
        "{seen}"
    );
    assert!(
        seen.contains(r#"data: {"event":"renox:toast","data":{"toasts":[{"kind":"info","message":"Export ready"}]}}"#),
        "{seen}"
    );
    assert!(!seen.contains(r#""id":9"#), "{seen}");
    // Names a page can listen to; anything else is an error.
    for bad in ["", "two words", "a\nb", "<x>"] {
        assert!(state.broadcast(bad, json!(null)).is_err(), "{bad:?}");
    }
}

#[renox::test]
async fn tests_can_record_broadcasts() {
    let (app, _dir) = app(true, true).await;
    app.fake_broadcasts();
    app.state()
        .broadcast_to(7, "order-updated", json!({ "id": 3 }))
        .unwrap();
    app.assert_broadcast("order-updated", |b| {
        b.user_id == Some(7) && b.data["id"] == 3
    });
    assert_eq!(app.broadcasts().len(), 1);
}

#[renox::test]
async fn pages_without_the_bell_can_open_the_stream() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("bell.html"),
        r#"{% from "renox/ui.html" import event_stream %}<main>{{ event_stream() }}</main>{{ toasts() }}"#,
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new().module(Auth::new().notifications()).module(Pages),
        move |c| c.views_path = path,
    )
    .await;
    // Guests have no stream.
    app.get("/bell")
        .await
        .assert_dont_see("data-rx-event-stream")
        .assert_see(r#"data-failed-label="That didn&#x27;t work. Try again.""#);
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.get("/bell")
        .await
        .assert_see(r#"<span hidden data-rx-event-stream="/notifications/stream"></span>"#);
}
