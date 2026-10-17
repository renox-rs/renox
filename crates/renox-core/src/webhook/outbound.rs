//! Outbound webhooks: the endpoints other systems register to be told about
//! events in this app. [`Endpoint`] is the subscription; its `whsec_` secret
//! is shown once, by [`Endpoint::create`], and stored sealed with `APP_KEY`.

use chrono::{DateTime, Utc};

use super::signing;
use crate::db::{Db, DbError, Row};
use crate::queue::unix_now;
use crate::{AppState, Error, Result};

/// A URL that receives this app's webhook events.
///
/// The sealed secret is kept private: [`Endpoint::list`] and
/// [`Endpoint::find`] never expose it, and [`Endpoint::secret`] opens it only
/// for signing a delivery.
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(state: AppState) -> Result {
/// use renox::webhook::outbound::Endpoint;
///
/// let (endpoint, secret) =
///     Endpoint::create(&state, "https://example.com/hooks", "Billing", &["invoice.paid"]).await?;
/// assert!(secret.starts_with("whsec_")); // shown only now
/// assert!(endpoint.subscribes_to("invoice.paid"));
/// assert!(!endpoint.subscribes_to("user.created"));
/// Endpoint::disable(&state.db, endpoint.id, "paused").await?;
/// Endpoint::delete(&state.db, endpoint.id).await?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// Primary key.
    pub id: i64,
    /// Where events are POSTed.
    pub url: String,
    /// A note for people (which system this is).
    pub description: String,
    /// Event names this endpoint receives; empty means every event.
    pub events: Vec<String>,
    /// Whether deliveries are sent to it.
    pub enabled: bool,
    /// Deliveries in a row that ended in failure; reset by a success.
    pub consecutive_failures: i64,
    /// When it was disabled, if it is.
    pub disabled_at: Option<DateTime<Utc>>,
    /// Why it was disabled, if it is.
    pub disabled_reason: Option<String>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    secret: String,
}

const COLUMNS: &str = "id, url, description, secret, events, enabled, consecutive_failures, \
                       disabled_at, disabled_reason, created_at, updated_at";

impl Endpoint {
    /// Registers an endpoint for `events` (none: every event) and returns it
    /// with its secret, `whsec_<base64>`. The secret is not recoverable by
    /// listing or finding the endpoint, so show it to the user now.
    ///
    /// # Errors
    /// When `url` is not an `http(s)` URL or the insert fails.
    pub async fn create(
        state: &AppState,
        url: &str,
        description: &str,
        events: &[&str],
    ) -> Result<(Self, String)> {
        let url = url.trim();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(Error::from(anyhow::anyhow!(
                "the webhook endpoint URL must start with https:// or http://"
            )));
        }
        let secret = signing::generate_secret();
        let sealed = state.encrypt(&secret);
        let now = unix_now();
        let id: i64 = crate::db::sql(
            "INSERT INTO webhook_endpoints (url, description, secret, events, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(url)
        .bind(description)
        .bind(sealed)
        .bind(serde_json::to_string(events)?)
        .bind(now)
        .bind(now)
        .scalar(&state.db)
        .await?;
        let endpoint = Self::find(&state.db, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("the webhook endpoint vanished after insert"))?;
        Ok((endpoint, secret))
    }

    /// Every endpoint, oldest first.
    pub async fn list(db: &Db) -> Result<Vec<Self>> {
        let rows = crate::db::sql(format!(
            "SELECT {COLUMNS} FROM webhook_endpoints ORDER BY id"
        ))
        .fetch_all(db)
        .await?;
        rows.iter()
            .map(|row| from_row(row).map_err(Into::into))
            .collect()
    }

    /// The endpoint with this id, if any.
    pub async fn find(db: &Db, id: i64) -> Result<Option<Self>> {
        let row = crate::db::sql(format!(
            "SELECT {COLUMNS} FROM webhook_endpoints WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(db)
        .await?;
        row.map(|row| from_row(&row).map_err(Into::into))
            .transpose()
    }

    /// The enabled endpoints that receive `event`, oldest first.
    pub async fn subscribed_to(db: &Db, event: &str) -> Result<Vec<Self>> {
        Ok(Self::list(db)
            .await?
            .into_iter()
            .filter(|e| e.enabled && e.subscribes_to(event))
            .collect())
    }

    /// Whether this endpoint receives `event` (an empty filter receives all).
    pub fn subscribes_to(&self, event: &str) -> bool {
        self.events.is_empty() || self.events.iter().any(|e| e == event)
    }

    /// The plain `whsec_` secret, for signing a delivery.
    ///
    /// # Errors
    /// When the sealed value was changed or `APP_KEY` differs.
    pub fn secret(&self, state: &AppState) -> Result<String> {
        state.decrypt(&self.secret)
    }

    /// Enables the endpoint again and clears its failure count; `false` if
    /// there is no such endpoint.
    pub async fn enable(db: &Db, id: i64) -> Result<bool> {
        let changed = crate::db::sql(
            "UPDATE webhook_endpoints SET enabled = 1, consecutive_failures = 0, \
             disabled_at = NULL, disabled_reason = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(unix_now())
        .bind(id)
        .execute(db)
        .await?;
        Ok(changed > 0)
    }

    /// Disables the endpoint, recording `reason`; `false` if there is no such
    /// endpoint.
    pub async fn disable(db: &Db, id: i64, reason: &str) -> Result<bool> {
        let now = unix_now();
        let changed = crate::db::sql(
            "UPDATE webhook_endpoints SET enabled = 0, disabled_at = ?, disabled_reason = ?, \
             updated_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(id)
        .execute(db)
        .await?;
        Ok(changed > 0)
    }

    /// Replaces the event filter (empty: every event); `false` if there is no
    /// such endpoint.
    pub async fn set_events(db: &Db, id: i64, events: &[&str]) -> Result<bool> {
        let changed =
            crate::db::sql("UPDATE webhook_endpoints SET events = ?, updated_at = ? WHERE id = ?")
                .bind(serde_json::to_string(events)?)
                .bind(unix_now())
                .bind(id)
                .execute(db)
                .await?;
        Ok(changed > 0)
    }

    /// Deletes the endpoint; `false` if there is no such endpoint.
    pub async fn delete(db: &Db, id: i64) -> Result<bool> {
        let changed = crate::db::sql("DELETE FROM webhook_endpoints WHERE id = ?")
            .bind(id)
            .execute(db)
            .await?;
        Ok(changed > 0)
    }
}

fn from_row(row: &Row) -> std::result::Result<Endpoint, DbError> {
    let events: String = row.try_get("events")?;
    Ok(Endpoint {
        id: row.try_get("id")?,
        url: row.try_get("url")?,
        description: row.try_get("description")?,
        secret: row.try_get("secret")?,
        events: serde_json::from_str(&events)
            .map_err(|err| DbError::from(sqlx::Error::Decode(err.into())))?,
        enabled: row.try_get::<i64>("enabled")? != 0,
        consecutive_failures: row.try_get("consecutive_failures")?,
        disabled_at: row
            .try_get::<Option<i64>>("disabled_at")?
            .map(crate::db::from_unix),
        disabled_reason: row.try_get("disabled_reason")?,
        created_at: crate::db::from_unix(row.try_get("created_at")?),
        updated_at: crate::db::from_unix(row.try_get("updated_at")?),
    })
}
