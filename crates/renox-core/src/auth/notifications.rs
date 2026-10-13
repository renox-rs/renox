//! Notifications: one message to a user (or to an address), delivered by
//! email, stored for an in-app list, sent through the app's own channels
//! (WhatsApp, SMS, Slack…), or all of these.
//!
//! ```
//! # use renox::prelude::*;
//! use renox::auth::{Channel, DatabaseMessage, Notification, Recipient};
//! use renox::mail::Mail;
//!
//! struct OrderShipped { order_id: i64 }
//!
//! impl Notification for OrderShipped {
//!     fn kind(&self) -> &'static str { "order-shipped" }
//!     // Per recipient, like Laravel's `via`: WhatsApp only for those with a number.
//!     fn channels(&self, to: &Recipient) -> Vec<Channel> {
//!         let mut channels = vec![Channel::Mail, Channel::Database];
//!         if to.address("whatsapp").is_some() {
//!             channels.push(Channel::Custom("whatsapp"));
//!         }
//!         channels
//!     }
//!
//!     fn to_mail(&self, to: &Recipient, state: &AppState) -> Result<Mail> {
//!         let email = to.email().unwrap_or_default();
//!         state.mail_view(&email, "Your order has shipped", "mail/shipped", context! { id => self.order_id })
//!     }
//!
//!     // What the in-app list (the UI kit's `notification_bell`) shows.
//!     fn to_database(&self, _: &Recipient, _: &AppState) -> Result<renox::serde_json::Value> {
//!         Ok(DatabaseMessage::success(format!("Order #{} shipped", self.order_id))
//!             .body("It arrives in 2–3 days.")
//!             .url(format!("/orders/{}", self.order_id))
//!             .with("order_id", self.order_id) // any other keys the app reads back
//!             .into())
//!     }
//!
//!     fn to_channel(&self, _channel: &str, _: &Recipient, _: &AppState) -> Result<renox::serde_json::Value> {
//!         Ok(json!({ "text": format!("Order #{} has shipped", self.order_id) }))
//!     }
//! }
//!
//! # async fn demo(state: AppState, user: User, db: Db, order_id: i64) -> Result {
//! state.notify(&user, &OrderShipped { order_id }).await?;          // now
//! state.notify_later(&user, &OrderShipped { order_id }).await?;    // through the queue
//! // Someone without an account:
//! let guest = Recipient::to("mail", "guest@example.com").and("whatsapp", "+6281234567890");
//! state.notify(&guest, &OrderShipped { order_id }).await?;
//! let unread = user.unread_notifications(&db).await?;
//! # let _ = unread; Ok(()) }
//!
//! // The app's own channel: `message` is what `to_channel` returned.
//! # let _ =
//! App::new().channel("whatsapp", |to: Recipient, message, state| async move {
//!     let phone = to.address("whatsapp").or_else(|| to.user()?.get("phone"));
//!     let _ = (state, phone, message); // call your provider's API here
//!     Ok(())
//! })
//! # ;
//! ```

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::User;
use crate::db::{DateTime, Db, now};
use crate::i18n::with_locale;
use crate::mail::Mail;
use crate::queue::{Job, JobContext};
use crate::toast::{ToastAction, ToastKind};
use crate::{AppState, Result};

/// Where a notification goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Channel {
    /// Sent with `state.mailer` (`to_mail`).
    Mail,
    /// Stored in the `notifications` table for an in-app list
    /// (`to_database`); only for users.
    Database,
    /// One of the app's own channels, registered with `App::channel`
    /// (`to_channel`).
    Custom(&'static str),
}

/// Who a notification goes to: a user, addresses for someone without an
/// account, or a user with an extra address.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Recipient {
    user: Option<User>,
    /// An address per channel, e.g. `mail` → `a@b.c`, `whatsapp` → `+62…`.
    routes: BTreeMap<String, String>,
    /// The language to write in; see [`Recipient::locale`].
    #[serde(default)]
    language: Option<String>,
}

impl Recipient {
    /// The user, at their email address (and any addresses added with `and`).
    pub fn for_user(user: &User) -> Self {
        Self {
            user: Some(user.clone()),
            routes: BTreeMap::new(),
            language: None,
        }
    }

    /// The user, if the recipient has an account.
    pub fn user(&self) -> Option<&User> {
        self.user.as_ref()
    }

    /// Someone without an account: `Recipient::to("mail", "a@b.c")`.
    pub fn to(channel: &str, address: impl Into<String>) -> Self {
        Self::default().and(channel, address)
    }

    /// Adds (or replaces) the address for `channel`.
    pub fn and(mut self, channel: &str, address: impl Into<String>) -> Self {
        self.routes.insert(channel.to_owned(), address.into());
        self
    }

    /// The address for `channel`; for `mail`, the user's email when no
    /// other address was given.
    pub fn address(&self, channel: &str) -> Option<String> {
        self.routes.get(channel).cloned().or_else(|| match channel {
            "mail" => self.user.as_ref().map(|u| u.email.clone()),
            _ => None,
        })
    }

    /// Whether `address` is one of the recipient's addresses, on any channel.
    pub(crate) fn has_address(&self, address: &str) -> bool {
        self.routes.values().any(|a| a == address) || self.email().as_deref() == Some(address)
    }

    /// The address for the `mail` channel (see [`Recipient::address`]).
    pub fn email(&self) -> Option<String> {
        self.address("mail")
    }

    /// Writes to this recipient in `locale` (e.g. `"es"`).
    pub fn in_locale(mut self, locale: impl Into<String>) -> Self {
        self.language = Some(locale.into());
        self
    }

    /// The recipient's language: the one given with [`Recipient::in_locale`],
    /// else the user's `locale` column if the `users` table has one.
    /// Notifications build their messages in it (`t()` in mail views,
    /// `state.current_lang()` in code).
    pub fn locale(&self) -> Option<String> {
        self.language.clone().or_else(|| {
            self.user
                .as_ref()?
                .get::<String>("locale")
                .filter(|l| !l.is_empty())
        })
    }
}

impl From<&User> for Recipient {
    fn from(user: &User) -> Self {
        Self::for_user(user)
    }
}

impl From<&crate::AuthUser> for Recipient {
    fn from(user: &crate::AuthUser) -> Self {
        Self::for_user(user)
    }
}

impl From<&Recipient> for Recipient {
    fn from(recipient: &Recipient) -> Self {
        recipient.clone()
    }
}

/// A message to a [`Recipient`], with a version for each channel it goes out on.
pub trait Notification: Send + Sync {
    /// Stored with database notifications, e.g. to pick an icon.
    fn kind(&self) -> &'static str;

    /// The channels to deliver on for this recipient (Laravel's `via`),
    /// e.g. WhatsApp only for users who turned it on; only `Channel::Mail`
    /// unless overridden.
    fn channels(&self, to: &Recipient) -> Vec<Channel> {
        let _ = to;
        vec![Channel::Mail]
    }

    /// The mail for `Channel::Mail`; an error unless overridden.
    fn to_mail(&self, _to: &Recipient, _state: &AppState) -> Result<Mail> {
        Err(anyhow!("notification `{}` has no mail version", self.kind()).into())
    }

    /// The JSON stored for `Channel::Database` (`DatabaseNotification::data`);
    /// `null` by default. Return a [`DatabaseMessage`] for the UI kit's
    /// `notification_bell`. It runs in the recipient's language, like
    /// `to_mail`.
    fn to_database(&self, to: &Recipient, state: &AppState) -> Result<Value> {
        let _ = (to, state);
        Ok(Value::Null)
    }

    /// The message for one of the app's own channels; the channel's handler
    /// gets it.
    fn to_channel(&self, channel: &str, to: &Recipient, state: &AppState) -> Result<Value> {
        let _ = (to, state);
        Err(anyhow!(
            "notification `{}` has no version for the `{channel}` channel",
            self.kind()
        )
        .into())
    }
}

/// What a notification stores for the in-app list, in the shape the UI
/// kit's `notification_bell` shows (and pushes as a toast when it arrives):
/// a status (its icon), a title, a line of text, a link and buttons. Return
/// it from [`Notification::to_database`] with `.into()`; `with` adds the
/// app's own keys next to them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DatabaseMessage {
    /// Its icon and color: success, info, warning or error.
    pub status: ToastKind,
    /// The first line.
    pub title: String,
    /// A second, lighter line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Where opening it goes (it is marked read on the way).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Links or buttons under the text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ToastAction>,
    /// The app's own keys (`with`), stored next to the others.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl DatabaseMessage {
    /// A message with `status` and `title`.
    pub fn new(status: ToastKind, title: impl Into<String>) -> Self {
        Self {
            status,
            title: title.into(),
            body: None,
            url: None,
            actions: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }

    /// A success message (a green check).
    pub fn success(title: impl Into<String>) -> Self {
        Self::new(ToastKind::Success, title)
    }

    /// An info message.
    pub fn info(title: impl Into<String>) -> Self {
        Self::new(ToastKind::Info, title)
    }

    /// A warning.
    pub fn warning(title: impl Into<String>) -> Self {
        Self::new(ToastKind::Warning, title)
    }

    /// An error.
    pub fn error(title: impl Into<String>) -> Self {
        Self::new(ToastKind::Error, title)
    }

    /// Adds a second line.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Where opening the notification goes.
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Adds a link or button.
    pub fn action(mut self, action: ToastAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Adds a link: `.link("Invoice", "/invoices/7")`.
    pub fn link(self, label: impl Into<String>, url: impl Into<String>) -> Self {
        self.action(ToastAction::link(label, url))
    }

    /// Stores `key` next to the message, for the app's own pages
    /// (`notification.data.order_id`). The message's own keys win.
    pub fn with(mut self, key: &str, value: impl Serialize) -> Self {
        self.extra.insert(
            key.to_owned(),
            serde_json::to_value(value).unwrap_or(Value::Null),
        );
        self
    }
}

impl From<DatabaseMessage> for Value {
    fn from(message: DatabaseMessage) -> Self {
        serde_json::to_value(message).unwrap_or(Value::Null)
    }
}

/// A stored notification.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct DatabaseNotification {
    /// The `notifications` row id.
    pub id: i64,
    /// The notification's [`Notification::kind`].
    pub kind: String,
    /// What [`Notification::to_database`] returned.
    pub data: Value,
    /// When it was marked read; `None` while unread.
    pub read_at: Option<DateTime>,
    /// When it was stored.
    pub created_at: DateTime,
}

impl DatabaseNotification {
    /// The stored [`DatabaseMessage`], when `to_database` returned one (its
    /// data has a `title`).
    pub fn message(&self) -> Option<DatabaseMessage> {
        serde_json::from_value(self.data.clone()).ok()
    }
}

/// Wakes the open notification streams (`/notifications/stream`) of one
/// user when something changed for them in this process; streams also look
/// at the table every few seconds, for changes made by other servers or by
/// `queue:work`. It also carries the app's own events
/// ([`AppState::broadcast`]) to the streams open in this process.
pub(crate) struct Hub {
    tx: tokio::sync::broadcast::Sender<Signal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Signal {
    /// Something changed for this user.
    User(i64),
    /// An app event for one user's streams, or (`None`) for every stream.
    Event(Option<i64>, Arc<Broadcast>),
    /// The server is shutting down.
    Stop,
}

/// An app event on its way to the open pages: its DOM event name and its
/// data, already JSON.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Broadcast {
    pub(crate) event: String,
    pub(crate) data: String,
}

impl Hub {
    pub(crate) fn new() -> Self {
        Self {
            tx: tokio::sync::broadcast::channel(256).0,
        }
    }

    /// Tells `user_id`'s streams to look now.
    pub(crate) fn touch(&self, user_id: i64) {
        let _ = self.tx.send(Signal::User(user_id));
    }

    /// Sends an app event to `user_id`'s streams, or to every stream.
    pub(crate) fn event(&self, user_id: Option<i64>, event: Broadcast) {
        let _ = self.tx.send(Signal::Event(user_id, Arc::new(event)));
    }

    /// Ends every stream, so a graceful shutdown doesn't wait for them.
    pub(crate) fn stop(&self) {
        let _ = self.tx.send(Signal::Stop);
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Signal> {
        self.tx.subscribe()
    }
}

pub(crate) type ChannelFn = Arc<
    dyn Fn(
            AppState,
            Recipient,
            Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result> + Send>>
        + Send
        + Sync,
>;

pub(crate) fn channel_fn<F, Fut>(send: F) -> ChannelFn
where
    F: Fn(Recipient, Value, AppState) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result> + Send + 'static,
{
    Arc::new(move |state, to, message| Box::pin(send(to, message, state)))
}

/// Delivers a message to one of the app's channels from the queue.
#[derive(Serialize, Deserialize)]
pub(crate) struct SendToChannel {
    channel: String,
    to: Recipient,
    message: Value,
}

impl Job for SendToChannel {
    const NAME: &'static str = "renox.send-to-channel";
    const MAX_ATTEMPTS: u32 = 5;

    async fn handle(self, ctx: JobContext) -> Result {
        let send = ctx.state.channel(&self.channel)?;
        send(ctx.state.clone(), self.to, self.message).await
    }
}

/// The channels a notification is delivered to, in delivery order: the
/// database row first, so a failure there doesn't leave a sent message
/// behind that a retry would send again; mail last.
fn ordered(notification: &impl Notification, to: &Recipient) -> Vec<Channel> {
    let mut channels = notification.channels(to);
    channels.sort_by_key(|channel| match channel {
        Channel::Database => 0,
        Channel::Custom(_) => 1,
        _ => 2,
    });
    channels
}

/// Whether `name` can be a DOM event name sent to pages: letters, digits
/// and `-`, `_`, `:`, `.` (what `hx-trigger` and Alpine's `x-on` can name).
fn event_name(name: &str) -> Result<&str> {
    let valid = !name.is_empty()
        && name.len() <= 100
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'));
    if valid {
        Ok(name)
    } else {
        Err(anyhow!(
            "`{name}` can't be a broadcast event's name: use letters, digits, `-`, `_`, `:` and `.`"
        )
        .into())
    }
}

impl AppState {
    /// Sends the DOM event `event`, with `data` as its `detail`, to every
    /// page open in this process with a notification stream (the UI kit's
    /// `notification_bell` or `event_stream()`, which need
    /// `Auth::new().notifications()`), whoever is logged in there.
    ///
    /// Pages listen as to any DOM event on `document`: htmx with
    /// `hx-trigger="order-updated from:document"`, Alpine with
    /// `x-on:order-updated.document="…"`, a script with
    /// `document.addEventListener("order-updated", e => e.detail)`. The
    /// event `renox:toast` with `{"toasts": [toast]}` shows toasts.
    ///
    /// It is fire-and-forget: nothing is stored, and pages that aren't
    /// connected at that moment (closed, reconnecting, open on another
    /// server, or a `queue:work` process sent it) never get it. Use a
    /// database notification ([`AppState::notify`]) for what must arrive.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # fn demo(state: AppState, user: User) -> Result {
    /// state.broadcast("order-updated", json!({ "id": 7, "status": "paid" }))?;
    /// // Only Ana's pages (any tab, any device on this server):
    /// state.broadcast_to(user.id, "renox:toast", json!({ "toasts": [Toast::info("Export ready")] }))?;
    /// # Ok(()) }
    /// ```
    pub fn broadcast(&self, event: &str, data: impl Serialize) -> Result {
        self.send_broadcast(None, event, data)
    }

    /// [`AppState::broadcast`] to the pages of one user (by id) only.
    pub fn broadcast_to(&self, user_id: i64, event: &str, data: impl Serialize) -> Result {
        self.send_broadcast(Some(user_id), event, data)
    }

    fn send_broadcast(&self, user_id: Option<i64>, event: &str, data: impl Serialize) -> Result {
        let event = event_name(event)?.to_owned();
        let data = serde_json::to_value(&data).map_err(anyhow::Error::from)?;
        let sent = crate::SentBroadcast {
            user_id,
            event,
            data,
        };
        if self.fakes.record_broadcast(sent.clone()) {
            return Ok(());
        }
        self.notification_hub.event(
            user_id,
            Broadcast {
                event: sent.event,
                data: sent.data.to_string(),
            },
        );
        Ok(())
    }

    fn channel(&self, name: &str) -> Result<ChannelFn> {
        self.channels.get(name).cloned().ok_or_else(|| {
            anyhow!("no `{name}` notification channel: register it with `App::channel`").into()
        })
    }

    async fn store_notification(&self, to: &Recipient, notification: &impl Notification) -> Result {
        // Only users have an in-app list.
        let Some(user) = &to.user else { return Ok(()) };
        crate::db::sql(
            "INSERT INTO notifications (user_id, kind, data, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(user.id)
        .bind(notification.kind())
        .bind(
            with_locale(to.locale().as_deref(), || {
                notification.to_database(to, self)
            })?
            .to_string(),
        )
        .bind(now())
        .execute(&self.db)
        .await?;
        self.notification_hub.touch(user.id);
        Ok(())
    }

    /// Delivers `notification` now to a user (`&user`) or to addresses (a
    /// [`Recipient`]), on each of its channels. The database row is written
    /// first and mail sent last, so a failure doesn't leave a message behind
    /// that a retry would send again.
    pub async fn notify(
        &self,
        to: impl Into<Recipient>,
        notification: &impl Notification,
    ) -> Result {
        let to = &to.into();
        if self.fakes.record_notification(notification.kind(), to) {
            return Ok(());
        }
        let locale = to.locale();
        let locale = locale.as_deref();
        for channel in ordered(notification, to) {
            match channel {
                Channel::Database => self.store_notification(to, notification).await?,
                Channel::Custom(name) => {
                    let send = self.channel(name)?;
                    let message = with_locale(locale, || notification.to_channel(name, to, self))?;
                    send(self.clone(), to.clone(), message).await?;
                }
                Channel::Mail => {
                    let mail = with_locale(locale, || notification.to_mail(to, self))?;
                    self.mailer.send(mail).await?;
                }
            }
        }
        Ok(())
    }

    /// Like `notify`, but mail and the app's channels are sent by queue
    /// workers, each channel as its own job with its own retries. The
    /// messages are built now; the database row is written now.
    pub async fn notify_later(
        &self,
        to: impl Into<Recipient>,
        notification: &impl Notification,
    ) -> Result {
        let to = to.into();
        if self.fakes.record_notification(notification.kind(), &to) {
            return Ok(());
        }
        let locale = to.locale();
        let locale = locale.as_deref();
        for channel in ordered(notification, &to) {
            match channel {
                Channel::Database => self.store_notification(&to, notification).await?,
                Channel::Custom(name) => {
                    self.channel(name)?; // fail now for an unknown channel
                    let message = with_locale(locale, || notification.to_channel(name, &to, self))?;
                    self.dispatch(SendToChannel {
                        channel: name.to_owned(),
                        to: to.clone(),
                        message,
                    })
                    .await?;
                }
                Channel::Mail => {
                    let mail = with_locale(locale, || notification.to_mail(&to, self))?;
                    self.queue_mail(mail).await?;
                }
            }
        }
        Ok(())
    }
}

pub(super) fn from_row(row: &crate::db::Row) -> Result<DatabaseNotification> {
    let data: String = row.try_get("data")?;
    Ok(DatabaseNotification {
        id: row.try_get("id")?,
        kind: row.try_get("kind")?,
        data: serde_json::from_str(&data).unwrap_or(Value::Null),
        read_at: row.try_get("read_at")?,
        created_at: row.try_get("created_at")?,
    })
}

impl User {
    /// The user's notifications, newest first.
    pub async fn notifications(&self, db: &Db, limit: u32) -> Result<Vec<DatabaseNotification>> {
        let rows = crate::db::sql(
            "SELECT id, kind, data, read_at, created_at FROM notifications \
             WHERE user_id = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(self.id)
        .bind(i64::from(limit))
        .fetch_all(db)
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// The user's notifications older than the one with id `before`,
    /// newest first: the next page after a list ending at `before`.
    pub async fn notifications_before(
        &self,
        db: &Db,
        before: i64,
        limit: u32,
    ) -> Result<Vec<DatabaseNotification>> {
        let rows = crate::db::sql(
            "SELECT id, kind, data, read_at, created_at FROM notifications \
             WHERE user_id = ? AND id < ? ORDER BY id DESC LIMIT ?",
        )
        .bind(self.id)
        .bind(before)
        .bind(i64::from(limit))
        .fetch_all(db)
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// One of the user's notifications, or `None` if it isn't theirs.
    pub async fn notification(&self, db: &Db, id: i64) -> Result<Option<DatabaseNotification>> {
        let row = crate::db::sql(
            "SELECT id, kind, data, read_at, created_at FROM notifications \
             WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(self.id)
        .fetch_optional(db)
        .await?;
        row.as_ref().map(from_row).transpose()
    }

    /// The user's unread notifications, newest first.
    pub async fn unread_notifications(&self, db: &Db) -> Result<Vec<DatabaseNotification>> {
        let rows = crate::db::sql(
            "SELECT id, kind, data, read_at, created_at FROM notifications \
             WHERE user_id = ? AND read_at IS NULL ORDER BY id DESC",
        )
        .bind(self.id)
        .fetch_all(db)
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// How many of the user's notifications are unread.
    pub async fn unread_notification_count(&self, db: &Db) -> Result<i64> {
        Ok(crate::db::sql(
            "SELECT COUNT(*) FROM notifications WHERE user_id = ? AND read_at IS NULL",
        )
        .bind(self.id)
        .scalar(db)
        .await?)
    }

    /// Marks one of the user's notifications read; returns whether it was theirs.
    pub async fn mark_notification_read(&self, db: &Db, id: i64) -> Result<bool> {
        let done = crate::db::sql(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?) WHERE id = ? AND user_id = ?",
        )
        .bind(now())
        .bind(id)
        .bind(self.id)
        .execute(db)
        .await?;
        Ok(done > 0)
    }

    /// Marks one of the user's notifications unread again; returns whether
    /// it was theirs.
    pub async fn mark_notification_unread(&self, db: &Db, id: i64) -> Result<bool> {
        let done =
            crate::db::sql("UPDATE notifications SET read_at = NULL WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(self.id)
                .execute(db)
                .await?;
        Ok(done > 0)
    }

    /// Deletes one of the user's notifications; returns whether it was theirs.
    pub async fn delete_notification(&self, db: &Db, id: i64) -> Result<bool> {
        let done = crate::db::sql("DELETE FROM notifications WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(self.id)
            .execute(db)
            .await?;
        Ok(done > 0)
    }

    /// Deletes all the user's notifications; returns how many there were.
    pub async fn delete_notifications(&self, db: &Db) -> Result<u64> {
        Ok(
            crate::db::sql("DELETE FROM notifications WHERE user_id = ?")
                .bind(self.id)
                .execute(db)
                .await?,
        )
    }

    /// Marks all the user's unread notifications read; returns how many there were.
    pub async fn mark_all_notifications_read(&self, db: &Db) -> Result<u64> {
        let done = crate::db::sql(
            "UPDATE notifications SET read_at = ? WHERE user_id = ? AND read_at IS NULL",
        )
        .bind(now())
        .bind(self.id)
        .execute(db)
        .await?;
        Ok(done)
    }
}

/// Deletes notifications read more than `age` ago; returns how many. Unread
/// ones stay, however old. `rnx notifications:prune` (from the `Auth` module)
/// runs it with `--days` (30 by default); schedule it, e.g. daily, since
/// nothing else removes read notifications while their user exists.
pub async fn prune_read_notifications(db: &Db, age: std::time::Duration) -> Result<u64> {
    let before = now() - chrono::Duration::from_std(age).unwrap_or_default();
    Ok(
        crate::db::sql("DELETE FROM notifications WHERE read_at < ?")
            .bind(before)
            .execute(db)
            .await?,
    )
}
