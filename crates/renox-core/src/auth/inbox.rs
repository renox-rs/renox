//! The in-app notification list behind the UI kit's `notification_bell`
//! (turned on with `Auth::new().notifications()`): a page that is also the
//! bell's panel, the actions on it, and a stream of Server-Sent Events that
//! tells open pages about new notifications as they arrive.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use super::AuthUser;
use super::notifications::{Broadcast, DatabaseNotification, Signal};
use crate::htmx::{Back, Htmx};
use crate::toast::{ToastAction, ToastKind, safe_url};
use crate::view::{View, view};
use crate::{AppState, Path, Result, Routes};

/// Notifications per page (and in the bell's panel).
const PAGE: u32 = 20;
/// How often a stream looks at the table even when nothing in this process
/// woke it: changes made by another server or by `queue:work`.
const POLL: Duration = Duration::from_secs(15);
/// A stream ends after this long and the browser opens a new one, so a
/// logged-out or revoked session doesn't keep one open.
const LIFETIME: Duration = Duration::from_secs(5 * 60);

pub(super) fn routes() -> Routes {
    Routes::new()
        .get("/notifications", index)
        .name("notifications.index")
        .delete("/notifications", clear)
        .name("notifications.clear")
        .get("/notifications/stream", events)
        .name("notifications.stream")
        .post("/notifications/read-all", read_all)
        .name("notifications.read_all")
        .post("/notifications/{id}/read", read)
        .name("notifications.read")
        .post("/notifications/{id}/unread", unread)
        .name("notifications.unread")
        .post("/notifications/{id}/open", open)
        .name("notifications.open")
        .delete("/notifications/{id}", destroy)
        .name("notifications.destroy")
        .require_auth()
}

/// A notification as the list shows it: the stored `DatabaseMessage`, or
/// for other data its `title` or `message` key, else its kind.
#[derive(Serialize)]
struct Item {
    id: i64,
    kind: String,
    status: ToastKind,
    title: String,
    body: Option<String>,
    url: Option<String>,
    actions: Vec<ToastAction>,
    read: bool,
    created_at: crate::db::DateTime,
    data: Value,
}

impl From<DatabaseNotification> for Item {
    fn from(n: DatabaseNotification) -> Self {
        let message = n.message();
        let text = |key: &str| n.data.get(key).and_then(Value::as_str).map(str::to_owned);
        let title = message
            .as_ref()
            .map(|m| m.title.clone())
            .or_else(|| text("title"))
            .or_else(|| text("message"))
            .unwrap_or_else(|| n.kind.replace(['-', '_', '.'], " "));
        let url = message
            .as_ref()
            .and_then(|m| m.url.clone())
            .or_else(|| text("url"))
            .filter(|url| safe_url(url));
        Self {
            id: n.id,
            status: message.as_ref().map_or(ToastKind::Info, |m| m.status),
            body: message
                .as_ref()
                .and_then(|m| m.body.clone())
                .or_else(|| text("body")),
            actions: message
                .map(|m| m.actions)
                .unwrap_or_default()
                .into_iter()
                // Links and requests (stored data has no page to send events to).
                .filter(|a| a.url.is_some() && a.is_safe())
                .collect(),
            title,
            url,
            read: n.read_at.is_some(),
            created_at: n.created_at,
            kind: n.kind,
            data: n.data,
        }
    }
}

#[derive(Deserialize)]
struct Page {
    before: Option<i64>,
}

/// `GET /notifications`: the page, or with `HX-Request` only its list
/// (the bell's panel). `?before=<id>` shows older ones.
async fn index(
    State(state): State<AppState>,
    user: AuthUser,
    Query(page): Query<Page>,
) -> Result<View> {
    list(&state, &user, page.before).await
}

async fn list(state: &AppState, user: &AuthUser, before: Option<i64>) -> Result<View> {
    let rows = match before {
        Some(before) => user.notifications_before(&state.db, before, PAGE).await?,
        None => user.notifications(&state.db, PAGE).await?,
    };
    let older = (rows.len() == PAGE as usize)
        .then(|| rows.last().map(|n| n.id))
        .flatten();
    let notifications: Vec<Item> = rows.into_iter().map(Item::from).collect();
    let unread = user.unread_notification_count(&state.db).await?;
    // The app's own layout when it has the usual one, else Renox's.
    let layout = if state.views.exists("layouts/app.html") {
        "layouts/app.html"
    } else {
        "renox/auth/layout.html"
    };
    Ok(view(
        "renox/notifications.html",
        crate::context! { notifications, unread, older, before, layout },
    )
    .fragment("panel"))
}

/// After an action: the list again for the bell's panel (htmx), else back.
async fn answer(state: &AppState, user: &AuthUser, htmx: &Htmx, back: Back) -> Result<Response> {
    state.notification_hub.touch(user.id);
    if htmx.request {
        Ok(list(state, user, None).await?.into_response())
    } else {
        Ok(back.into_response())
    }
}

async fn read(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    back: Back,
    Path(id): Path<i64>,
) -> Result<Response> {
    user.mark_notification_read(&state.db, id).await?;
    answer(&state, &user, &htmx, back).await
}

async fn unread(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    back: Back,
    Path(id): Path<i64>,
) -> Result<Response> {
    user.mark_notification_unread(&state.db, id).await?;
    answer(&state, &user, &htmx, back).await
}

async fn destroy(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    back: Back,
    Path(id): Path<i64>,
) -> Result<Response> {
    user.delete_notification(&state.db, id).await?;
    answer(&state, &user, &htmx, back).await
}

async fn read_all(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    back: Back,
) -> Result<Response> {
    user.mark_all_notifications_read(&state.db).await?;
    answer(&state, &user, &htmx, back).await
}

async fn clear(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    back: Back,
) -> Result<Response> {
    user.delete_notifications(&state.db).await?;
    answer(&state, &user, &htmx, back).await
}

/// Marks the notification read and goes where it points (or to the list).
async fn open(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Redirect> {
    let Some(notification) = user.notification(&state.db, id).await? else {
        return Err(crate::Error::NotFound);
    };
    user.mark_notification_read(&state.db, id).await?;
    state.notification_hub.touch(user.id);
    let item = Item::from(notification);
    Ok(Redirect::to(&item.url.unwrap_or_else(|| {
        state
            .routes
            .url("notifications.index", &[])
            .unwrap_or_else(|_| "/notifications".into())
    })))
}

struct Watch {
    state: AppState,
    user_id: i64,
    rx: tokio::sync::broadcast::Receiver<Signal>,
    last_id: i64,
    unread: i64,
    pending: VecDeque<Event>,
    until: tokio::time::Instant,
}

/// `GET /notifications/stream`: Server-Sent Events. `count` (the unread
/// count) first and whenever it changes; `notification` (the new one as
/// JSON, shaped like the list's items) when one arrives; `broadcast`
/// (`{"event": name, "data": …}`) for the app's own events
/// (`AppState::broadcast`), which renox-ui.js dispatches on `document`.
async fn events(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Sse<impl Stream<Item = std::result::Result<Event, Infallible>>>> {
    let rx = state.notification_hub.subscribe();
    let last_id: i64 =
        crate::db::sql("SELECT COALESCE(MAX(id), 0) FROM notifications WHERE user_id = ?")
            .bind(user.id)
            .scalar(&state.db)
            .await?;
    let unread = user.unread_notification_count(&state.db).await?;
    let first = Event::default()
        .event("count")
        .data(unread.to_string())
        .retry(Duration::from_secs(3));
    let watch = Watch {
        state,
        user_id: user.id,
        rx,
        last_id,
        unread,
        pending: VecDeque::from([first]),
        until: tokio::time::Instant::now() + LIFETIME,
    };
    let events = stream::unfold(watch, |mut watch| async move {
        loop {
            if let Some(event) = watch.pending.pop_front() {
                return Some((Ok(event), watch));
            }
            let woken = tokio::select! {
                signal = watch.rx.recv() => match signal {
                    Ok(Signal::User(id)) => id == watch.user_id,
                    Ok(Signal::Event(to, event)) => {
                        if to.is_none_or(|id| id == watch.user_id) {
                            watch.pending.push_back(broadcast_event(&event));
                        }
                        false
                    }
                    Ok(Signal::Stop) | Err(RecvError::Closed) => return None,
                    // Missed some: look anyway.
                    Err(RecvError::Lagged(_)) => true,
                },
                _ = tokio::time::sleep(POLL) => true,
                _ = tokio::time::sleep_until(watch.until) => return None,
            };
            if woken && watch.look().await.is_err() {
                return None;
            }
        }
    });
    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

/// An app event as the stream sends it: its name and data in one JSON
/// object, so the page needs one listener for all of them.
fn broadcast_event(event: &Broadcast) -> Event {
    // `data` is JSON already: put it in as it is.
    let name = serde_json::to_string(&event.event).unwrap_or_default();
    Event::default()
        .event("broadcast")
        .data(format!(r#"{{"event":{name},"data":{}}}"#, event.data))
}

impl Watch {
    /// Queues events for what changed since the last look.
    async fn look(&mut self) -> Result {
        let db = &self.state.db;
        let rows = crate::db::sql(
            "SELECT id, kind, data, read_at, created_at FROM notifications \
             WHERE user_id = ? AND id > ? ORDER BY id LIMIT 20",
        )
        .bind(self.user_id)
        .bind(self.last_id)
        .fetch_all(db)
        .await?;
        for row in &rows {
            let notification = super::notifications::from_row(row)?;
            self.last_id = self.last_id.max(notification.id);
            let item = Item::from(notification);
            self.pending.push_back(
                Event::default()
                    .event("notification")
                    .data(serde_json::to_string(&item).unwrap_or_default()),
            );
        }
        let unread: i64 = crate::db::sql(
            "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND read_at IS NULL",
        )
        .bind(self.user_id)
        .scalar(db)
        .await?;
        if unread != self.unread || !rows.is_empty() {
            self.unread = unread;
            self.pending
                .push_back(Event::default().event("count").data(unread.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::auth::{Auth, User};
    use crate::testing::TestApp;

    /// A stream open for `user`, its first frame (the unread count) read.
    async fn open(app: &TestApp, user: &User) -> Body {
        let token = user.create_token(app.db(), "page", None).await.unwrap();
        let req = axum::http::Request::builder()
            .uri("/notifications/stream")
            .header("authorization", format!("Bearer {}", token.plain))
            .body(Body::empty())
            .unwrap();
        let res = app.kernel().router().oneshot(req).await.unwrap();
        assert_eq!(res.status(), 200);
        let mut body = res.into_body();
        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(String::from_utf8_lossy(&first).contains("event: count"));
        body
    }

    async fn rest(body: Body) -> String {
        let bytes = tokio::time::timeout(std::time::Duration::from_secs(5), body.collect())
            .await
            .expect("the stream ends")
            .unwrap()
            .to_bytes();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    async fn app() -> (TestApp, User) {
        let app = TestApp::new(crate::App::new().module(Auth::new().notifications())).await;
        let user = User::register(app.db(), "Ann", "ann@example.com", "password123")
            .await
            .unwrap();
        (app, user)
    }

    /// At shutdown the hub stops every stream, also one that fell behind
    /// (it looks at the table, then ends).
    #[tokio::test]
    async fn streams_end_when_the_hub_stops() {
        let (app, user) = app().await;
        let body = open(&app, &user).await;
        let hub = app.state().notification_hub.clone();
        for _ in 0..300 {
            hub.touch(user.id + 1);
        }
        hub.stop();
        rest(body).await;
    }

    /// A look at the table that fails ends the stream; the page reconnects.
    #[tokio::test]
    async fn a_stream_ends_when_its_look_fails() {
        let (app, user) = app().await;
        let body = open(&app, &user).await;
        crate::db::sql("ALTER TABLE notifications RENAME TO notifications_gone")
            .execute(app.db())
            .await
            .unwrap();
        app.state().notification_hub.touch(user.id);
        rest(body).await;
    }
}
