//! Telling people: the customer (a mail and an in-app notification) and a
//! store's staff (in-app only), through Renox's notifications.
//!
//! [`Notice`] is one `Notification` for every message of the rentals and
//! workshop areas: a title, a sentence, a few rows of details and a link,
//! all as translation keys with their values, so each message is written
//! in the **recipient's** language when it is sent (`state.current_lang()`
//! inside `to_mail` / `to_database`), not the language of whoever caused
//! it. Mail goes through `mail/rentals/notice.html` (+ `.txt`), built on the
//! kit's mail layout; the in-app row is a `DatabaseMessage`, what the kit's
//! notification bell and its live stream show.
//!
//! A notice to a **customer** ([`customer`]) goes out on the channels they
//! chose for its kind on their account (`accounts::preferences`: rentals,
//! the workshop, plans), so "mail only" or "none" is respected. A notice to
//! **staff** ([`staff`], or `state.notify` on a member of staff) is work, not
//! a preference: it keeps its own channels (the bell, and mail when
//! [`Notice::mail`] is set).

use renox::auth::permissions;
use renox::auth::{Channel, DatabaseMessage, Notification, Recipient};
use renox::mail::Mail;
use renox::prelude::*;
use serde::Serialize;

use crate::app::access::policy::store_scope;
use crate::app::accounts::model::Customer;
use crate::app::accounts::preferences::{Kind, channels_for};

/// How a notice looks in the bell: its colour and icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Warning,
}

/// One message to a customer or to staff.
#[derive(Debug, Clone)]
pub struct Notice {
    /// What kind of message it is (`rental-reserved`), for tests and the
    /// notifications table.
    pub kind: &'static str,
    /// The mail's subject and the notification's title: a translation key.
    pub title: &'static str,
    /// The first sentence: a translation key.
    pub body: &'static str,
    /// Values for both keys (`:code` → the reservation code).
    pub params: Vec<(&'static str, String)>,
    /// Details as (label key, value), shown as a table in the mail.
    pub rows: Vec<(&'static str, String)>,
    /// Where the button and the notification lead (an absolute URL).
    pub url: Option<String>,
    /// The button's label: a translation key.
    pub action: &'static str,
    pub tone: Tone,
    /// Also by mail (customers), or only in the app (staff).
    pub mail: bool,
    /// The mail view: `mail/rentals/notice` or the workshop's.
    pub view: &'static str,
    /// For a customer: which of their notification preferences decides the
    /// channels (set by [`customer`]). `None` for staff.
    pub topic: Option<Kind>,
}

impl Notice {
    /// A notice in the rentals' mail view, by mail and in the app.
    pub fn new(kind: &'static str, title: &'static str, body: &'static str) -> Self {
        Notice {
            kind,
            title,
            body,
            params: Vec::new(),
            rows: Vec::new(),
            url: None,
            action: "rentals.mail.open",
            tone: Tone::Info,
            mail: true,
            view: "mail/rentals/notice",
            topic: None,
        }
    }

    /// A value for `:name` in the title and the sentence.
    pub fn param(mut self, name: &'static str, value: impl ToString) -> Self {
        self.params.push((name, value.to_string()));
        self
    }

    /// A row of details: its label's key and its value.
    pub fn row(mut self, label: &'static str, value: impl ToString) -> Self {
        self.rows.push((label, value.to_string()));
        self
    }

    /// Where it leads.
    pub fn url(mut self, url: String) -> Self {
        self.url = Some(url);
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// In the app only (for staff).
    pub fn in_app_only(mut self) -> Self {
        self.mail = false;
        self
    }

    /// Another mail view (the workshop's).
    pub fn view(mut self, view: &'static str) -> Self {
        self.view = view;
        self
    }

    fn text(&self, state: &AppState, key: &str) -> String {
        let params: Vec<(&str, &dyn std::fmt::Display)> = self
            .params
            .iter()
            .map(|(k, v)| (*k, v as &dyn std::fmt::Display))
            .collect();
        state.current_lang().t(key, &params)
    }
}

/// A row of the mail's details table.
#[derive(Serialize)]
struct Row {
    label: String,
    value: String,
}

impl Notification for Notice {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn channels(&self, to: &Recipient) -> Vec<Channel> {
        if let Some(kind) = self.topic {
            // A customer: what they chose for this kind, where it can reach them.
            return channels_for(to, kind)
                .into_iter()
                .filter(|c| match c {
                    Channel::Mail => self.mail && to.email().is_some(),
                    Channel::Database => to.user().is_some(),
                    _ => true,
                })
                .collect();
        }
        let mut channels = Vec::new();
        if to.user().is_some() {
            channels.push(Channel::Database);
        }
        if self.mail && to.email().is_some() {
            channels.push(Channel::Mail);
        }
        channels
    }

    fn to_mail(&self, to: &Recipient, state: &AppState) -> Result<Mail> {
        let lang = state.current_lang();
        let rows: Vec<Row> = self
            .rows
            .iter()
            .map(|(label, value)| Row {
                label: lang.t(label, &[]),
                value: value.clone(),
            })
            .collect();
        state.mail_view(
            to.email().unwrap_or_default(),
            self.text(state, self.title),
            self.view,
            context! {
                title => self.text(state, self.title),
                body => self.text(state, self.body),
                table => rows.iter().map(|r| [r.label.clone(), r.value.clone()]).collect::<Vec<_>>(),
                rows,
                url => self.url,
                action => lang.t(self.action, &[]),
            },
        )
    }

    fn to_database(&self, _to: &Recipient, state: &AppState) -> Result<renox::serde_json::Value> {
        let title = self.text(state, self.title);
        let message = match self.tone {
            Tone::Info => DatabaseMessage::info(title),
            Tone::Success => DatabaseMessage::success(title),
            Tone::Warning => DatabaseMessage::warning(title),
        }
        .body(self.text(state, self.body));
        let message = match &self.url {
            Some(url) => message.url(url.clone()),
            None => message,
        };
        Ok(message.into())
    }
}

/// Sends `notice` about `kind` to a customer: to their account when they
/// have one (by mail, in the app, both or neither, as they chose for
/// `kind`), else by mail to their address, else nowhere (a walk-in who left
/// no address).
pub async fn customer(
    state: &AppState,
    customer: &Customer,
    kind: Kind,
    notice: &Notice,
) -> Result {
    let notice = &Notice {
        topic: Some(kind),
        ..notice.clone()
    };
    if let Some(user_id) = customer.user_id
        && let Some(user) = User::find(&state.db, user_id).await?
    {
        return state.notify(&user, notice).await;
    }
    if let Some(email) = customer.email.as_deref().filter(|e| !e.is_empty()) {
        return state.notify(Recipient::to("mail", email), notice).await;
    }
    Ok(())
}

/// The users who hold `permission` in store `store_id` now: a role given
/// in that store and within its dates, or a global role (the owner). The
/// app checks permissions, never role names, so it asks
/// `permissions::users_with_permission_in`, which reads which roles grant
/// the permission.
pub async fn staff_with_permission(db: &Db, permission: &str, store_id: i64) -> Result<Vec<User>> {
    permissions::users_with_permission_in(db, permission, &store_scope(store_id)).await
}

/// Sends `notice` (in the app only) to everyone holding `permission` in
/// each of `stores` (once per person, even when they work in two of them).
pub async fn staff(state: &AppState, permission: &str, stores: &[i64], notice: &Notice) -> Result {
    let mut sent: Vec<i64> = Vec::new();
    let notice = notice.clone().in_app_only();
    for store in stores {
        for user in staff_with_permission(&state.db, permission, *store).await? {
            if sent.contains(&user.id) {
                continue;
            }
            sent.push(user.id);
            state.notify(&user, &notice).await?;
        }
    }
    Ok(())
}
