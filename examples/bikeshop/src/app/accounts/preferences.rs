//! How each kind of notification reaches a customer: by mail, in the app,
//! both, or not at all; and the language they read the shop in.
//!
//! Both live on the customer's login, in two columns this example adds to
//! Renox's `users` table (`migrations/20260102000100_add_account_preferences_to_users.*`):
//!
//! - `notification_preferences`: JSON, `{"order": "both", "marketing": "none"}`.
//!   A kind that isn't there uses its default ([`Kind::default_choice`]).
//! - `locale`: `en` or `es`. Renox's notifications read a `locale` column by
//!   themselves (`Recipient::locale`), so mails go out in the customer's
//!   language with no code here.
//!
//! **Every notification to a customer asks [`channels_for`]** in its
//! `Notification::channels`, so the customer's choice is respected
//! everywhere: the rentals', workshop's and plans' notices
//! (`rentals::notify::customer`, given the [`Kind`]) and the orders' mail and
//! notification (`sales::notify::tell`). Notices to staff are work, not
//! preferences, and keep their own channels. No customer notification is
//! marketing yet; [`Kind::Marketing`] is there for the first one. It reads the preferences from the `User` the notification is
//! for (Renox loads `users` with `SELECT *`, so the column is in
//! `user.extra`): no query.
//!
//! ```
//! use bikeshop::app::accounts::preferences::{Kind, channels_for};
//! use renox::auth::{Channel, Notification, Recipient};
//!
//! struct RentalDueBack;
//!
//! impl Notification for RentalDueBack {
//!     fn kind(&self) -> &'static str {
//!         "rental-due-back"
//!     }
//!
//!     // Mail, the bell, both or neither: as the customer chose for rentals.
//!     fn channels(&self, to: &Recipient) -> Vec<Channel> {
//!         channels_for(to, Kind::Rental)
//!     }
//! }
//! # let _ = RentalDueBack;
//! ```

use renox::auth::{Channel, Recipient, User};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The column holding the preferences.
pub const COLUMN: &str = "notification_preferences";

/// The kinds of notification a customer chooses for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Orders: confirmed, ready to collect, shipped, refunded.
    Order,
    /// Rentals: reserved, due back soon, late, returned.
    Rental,
    /// The workshop: booked, ready to collect, waiting for parts.
    Workshop,
    /// Service plans: the next visit, a failed payment.
    Plan,
    /// News and offers.
    Marketing,
}

impl Kind {
    /// Every kind, in the order the preferences form lists them.
    pub const ALL: [Kind; 5] = [
        Kind::Order,
        Kind::Rental,
        Kind::Workshop,
        Kind::Plan,
        Kind::Marketing,
    ];

    /// The key in the JSON and in the form (`order`).
    pub fn key(self) -> &'static str {
        match self {
            Kind::Order => "order",
            Kind::Rental => "rental",
            Kind::Workshop => "workshop",
            Kind::Plan => "plan",
            Kind::Marketing => "marketing",
        }
    }

    /// The kind with this key.
    pub fn from_key(key: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == key)
    }

    /// Until the customer chooses: everything about their own orders,
    /// rentals and bikes by mail and in the app; no marketing (they opt in).
    pub fn default_choice(self) -> Choice {
        match self {
            Kind::Marketing => Choice::None,
            _ => Choice::Both,
        }
    }
}

/// How one kind reaches the customer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// By mail only.
    Mail,
    /// In the app only (the bell and the notifications page).
    InApp,
    /// Both.
    Both,
    /// Not at all.
    None,
}

impl Choice {
    /// Every choice, in the order the form shows them.
    pub const ALL: [Choice; 4] = [Choice::Both, Choice::Mail, Choice::InApp, Choice::None];

    /// The key in the JSON and in the form (`in_app`).
    pub fn key(self) -> &'static str {
        match self {
            Choice::Mail => "mail",
            Choice::InApp => "in_app",
            Choice::Both => "both",
            Choice::None => "none",
        }
    }

    /// The choice with this key.
    pub fn from_key(key: &str) -> Option<Choice> {
        Choice::ALL.into_iter().find(|c| c.key() == key)
    }

    /// Renox's channels for this choice.
    pub fn channels(self) -> Vec<Channel> {
        match self {
            Choice::Mail => vec![Channel::Mail],
            Choice::InApp => vec![Channel::Database],
            Choice::Both => vec![Channel::Mail, Channel::Database],
            Choice::None => vec![],
        }
    }
}

/// A customer's choices, one per kind (missing kinds use their default).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences(pub BTreeMap<Kind, Choice>);

impl Preferences {
    /// The preferences stored on `user` (the defaults when there are none,
    /// or when the column holds something unreadable).
    pub fn of(user: &User) -> Preferences {
        user.get::<String>(COLUMN)
            .and_then(|json| renox::serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// The choice for `kind`.
    pub fn choice(&self, kind: Kind) -> Choice {
        self.0
            .get(&kind)
            .copied()
            .unwrap_or_else(|| kind.default_choice())
    }

    /// The preferences as stored in the column.
    pub fn to_json(&self) -> String {
        renox::serde_json::to_string(&self.0).unwrap_or_else(|_| "{}".into())
    }

    /// Every kind with its choice, for the form.
    pub fn rows(&self) -> Vec<(Kind, Choice)> {
        Kind::ALL.into_iter().map(|k| (k, self.choice(k))).collect()
    }
}

// [explain:notifications.channels]
/// The channels a notification of `kind` goes out on for `to`: the
/// customer's choice for that kind. Someone without an account (a walk-in
/// mailed at the address they gave) gets mail for their own business and
/// never marketing, since they couldn't have opted in.
pub fn channels_for(to: &Recipient, kind: Kind) -> Vec<Channel> {
    match to.user() {
        Some(user) => Preferences::of(user).choice(kind).channels(),
        None if kind == Kind::Marketing => vec![],
        None => vec![Channel::Mail],
    }
}
// [/explain:notifications.channels]

/// Saves `preferences` on `user`.
pub async fn save(db: &renox::db::Db, user: &mut User, preferences: &Preferences) -> renox::Result {
    user.set(db, COLUMN, preferences.to_json()).await
}
