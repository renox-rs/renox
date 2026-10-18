//! Outbound webhooks: the endpoints other systems register to be told about
//! events in this app. [`Endpoint`] is the subscription; its `whsec_` secret
//! is shown once, by [`Endpoint::create`], and stored sealed with `APP_KEY`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::signing;
use crate::db::{Db, DbError, Row, Transaction};
use crate::events::Event;
use crate::queue::{Job, JobContext, unix_now};
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

/// Emitted when an endpoint was disabled: its URL answered `410 Gone`, or
/// `WEBHOOK_DISABLE_AFTER` deliveries in a row failed. Listen to it to tell
/// whoever owns the endpoint.
///
/// ```
/// # use renox::prelude::*;
/// use renox::webhook::outbound::EndpointDisabled;
///
/// # let _ =
/// App::new().listen(|event: EndpointDisabled, _state| async move {
///     tracing::warn!(endpoint = event.endpoint_id, "webhook endpoint disabled");
///     Ok(())
/// })
/// # ;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointDisabled {
    /// The [`Endpoint::id`] that was disabled.
    pub endpoint_id: i64,
}

impl Event for EndpointDisabled {}

type PayloadFn = Arc<dyn Fn(&dyn Any) -> Option<serde_json::Value> + Send + Sync>;

/// The webhook name and payload mapping of one event type, from
/// `App::webhook_event`.
pub(crate) struct EventDef {
    pub(crate) name: String,
    map: PayloadFn,
}

impl EventDef {
    pub(crate) fn new<E: Event>(
        name: &str,
        map: impl Fn(&E) -> serde_json::Value + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.to_owned(),
            map: Arc::new(move |event| event.downcast_ref::<E>().map(&map)),
        }
    }
}

pub(crate) type EventDefs = Arc<HashMap<TypeId, EventDef>>;

/// Queues one delivery per enabled endpoint subscribed to `event`, inside
/// `tx`: the deliveries are sent only if the transaction commits, and
/// never if it rolls back (a transactional outbox). `E` must be registered
/// with `App::webhook_event`. Returns how many deliveries were queued.
///
/// Outside a transaction a plain `state.emit(event)` delivers too.
///
/// ```
/// # use renox::prelude::*;
/// # #[derive(Clone)] struct InvoiceIssued { id: i64 }
/// # impl Event for InvoiceIssued {}
/// # async fn demo(state: AppState) -> Result {
/// use renox::webhook::outbound::emit_in;
///
/// let mut tx = state.db.begin().await?;
/// // … write the invoice with `tx` …
/// emit_in(&state, &mut tx, &InvoiceIssued { id: 7 }).await?;
/// tx.commit().await?; // sent after this, never if it rolls back
/// # Ok(()) }
/// ```
///
/// # Errors
/// When `E` is not registered or the insert fails.
pub async fn emit_in<E: Event>(state: &AppState, tx: &mut Transaction, event: &E) -> Result<usize> {
    let def = state
        .webhook_events
        .get(&TypeId::of::<E>())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "`{}` is not a webhook event: register it with App::webhook_event",
                std::any::type_name::<E>()
            )
        })?;
    let data = (def.map)(event).unwrap_or(serde_json::Value::Null);
    enqueue(state, tx, &def.name, data).await
}

/// What a plain `state.emit` does: `emit_in` in its own transaction.
pub(crate) async fn emit_alone(state: &AppState, name: &str, data: serde_json::Value) -> Result {
    let mut tx = state.db.begin().await?;
    let queued = enqueue(state, &mut tx, name, data).await?;
    tx.commit().await?;
    if queued > 0 {
        state.queue.wake_workers();
    }
    Ok(())
}

async fn enqueue(
    state: &AppState,
    tx: &mut Transaction,
    event: &str,
    data: serde_json::Value,
) -> Result<usize> {
    // Read through `tx`: with one pooled connection, `state.db` would wait for it.
    let rows = crate::db::sql(format!(
        "SELECT {COLUMNS} FROM webhook_endpoints WHERE enabled = 1 ORDER BY id"
    ))
    .fetch_all(&mut *tx)
    .await?;
    let mut endpoints = Vec::new();
    for row in &rows {
        let endpoint = from_row(row)?;
        if endpoint.subscribes_to(event) {
            endpoints.push(endpoint);
        }
    }
    if endpoints.is_empty() {
        return Ok(0);
    }
    let message_id = format!("msg_{}", crate::db::Ulid::new().as_str());
    let created_at = unix_now();
    let payload = serde_json::to_string(&serde_json::json!({
        "id": message_id,
        "type": event,
        "created_at": crate::db::from_unix(created_at)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "data": data,
    }))?;
    for endpoint in &endpoints {
        let delivery_id: i64 = crate::db::sql(
            "INSERT INTO webhook_deliveries (endpoint_id, message_id, event_type, payload, status, created_at) \
             VALUES (?, ?, ?, ?, 'pending', ?) RETURNING id",
        )
        .bind(endpoint.id)
        .bind(&message_id)
        .bind(event)
        .bind(&payload)
        .bind(created_at)
        .scalar(&mut *tx)
        .await?;
        let job = DeliverWebhook {
            endpoint_id: endpoint.id,
            delivery_id,
        };
        state.queue.dispatch_in(tx, job).await?;
    }
    Ok(endpoints.len())
}

/// The queue job that sends one stored delivery and records the outcome.
/// Retried on 5xx, 429, timeouts and connection errors, on the Standard
/// Webhooks schedule; other answers end the delivery.
#[derive(Serialize, Deserialize)]
pub(crate) struct DeliverWebhook {
    endpoint_id: i64,
    delivery_id: i64,
}

/// How long one request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// What the receiver's answer means for the delivery.
enum Outcome {
    Delivered(u16),
    /// 410: the receiver asked not to be called again.
    Gone(u16),
    /// Another answer that retrying can't change.
    Rejected(u16),
    /// Worth another try: 5xx, 429, a timeout or a connection error.
    Retry(Option<u16>, String),
}

impl Job for DeliverWebhook {
    const NAME: &'static str = "renox:deliver-webhook";
    /// The first try plus seven retries.
    const MAX_ATTEMPTS: u32 = 8;
    const ENCRYPTED: bool = true;

    /// The Standard Webhooks schedule after the first try: 5 s, 5 min, 30
    /// min, 2 h, 5 h, then 10 h.
    fn backoff(attempt: u32) -> Duration {
        const SECONDS: [u64; 7] = [5, 300, 1_800, 7_200, 18_000, 36_000, 36_000];
        let index = (attempt.max(1) - 1) as usize;
        Duration::from_secs(SECONDS[index.min(SECONDS.len() - 1)])
    }

    async fn handle(self, ctx: JobContext) -> Result {
        let state = &ctx.state;
        let db = &state.db;
        let row = crate::db::sql(
            "SELECT message_id, payload, status FROM webhook_deliveries WHERE id = ?",
        )
        .bind(self.delivery_id)
        .fetch_optional(db)
        .await?;
        let Some(row) = row else { return Ok(()) };
        if row.try_get::<String>("status")? == "succeeded" {
            return Ok(());
        }
        let message_id: String = row.try_get("message_id")?;
        let payload: String = row.try_get("payload")?;
        let id = self.delivery_id;
        let attempt = ctx.attempt;
        let endpoint = match Endpoint::find(db, self.endpoint_id).await? {
            Some(endpoint) if endpoint.enabled => endpoint,
            _ => {
                let why = "the endpoint is disabled or gone";
                return record(db, id, "failed", attempt, None, Some(why), 0, None).await;
            }
        };
        if endpoint.url.starts_with("http://")
            && matches!(state.config.env, crate::config::Environment::Production)
        {
            let why = "the endpoint URL must be https in production";
            record(db, id, "failed", attempt, None, Some(why), 0, None).await?;
            return fail(state, &endpoint, why).await;
        }

        let timestamp = unix_now();
        let secret = endpoint.secret(state)?;
        let signature = signing::sign(&secret, &message_id, timestamp, payload.as_bytes())?;
        let started = crate::clock::unix_millis();
        let sent = state
            .http
            .post(endpoint.url.as_str())
            .header("webhook-id", message_id)
            .header("webhook-timestamp", timestamp.to_string())
            .header("webhook-signature", signature)
            .body("application/json", payload.into_bytes())
            .timeout(REQUEST_TIMEOUT)
            .no_redirects()
            .send()
            .await;
        let duration = crate::clock::unix_millis() - started;
        let outcome = match sent {
            Ok(response) => {
                let code = response.status().as_u16();
                match code {
                    200..=299 => Outcome::Delivered(code),
                    410 => Outcome::Gone(code),
                    429 | 500..=599 => {
                        Outcome::Retry(Some(code), format!("the endpoint answered {code}"))
                    }
                    _ => Outcome::Rejected(code),
                }
            }
            Err(err) => Outcome::Retry(None, err.to_string()),
        };

        match outcome {
            Outcome::Delivered(code) => {
                record(
                    db,
                    id,
                    "succeeded",
                    attempt,
                    Some(code),
                    None,
                    duration,
                    None,
                )
                .await?;
                crate::db::sql(
                    "UPDATE webhook_endpoints SET consecutive_failures = 0 \
                     WHERE id = ? AND consecutive_failures <> 0",
                )
                .bind(endpoint.id)
                .execute(db)
                .await?;
                Ok(())
            }
            Outcome::Gone(code) => {
                let why = "the endpoint answered 410 Gone";
                record(
                    db,
                    id,
                    "failed",
                    attempt,
                    Some(code),
                    Some(why),
                    duration,
                    None,
                )
                .await?;
                Endpoint::disable(db, endpoint.id, why).await?;
                let event = EndpointDisabled {
                    endpoint_id: endpoint.id,
                };
                state.emit(event).await
            }
            Outcome::Rejected(code) => {
                let why = format!("the endpoint answered {code}");
                record(
                    db,
                    id,
                    "failed",
                    attempt,
                    Some(code),
                    Some(&why),
                    duration,
                    None,
                )
                .await?;
                fail(state, &endpoint, &why).await
            }
            Outcome::Retry(code, why) if attempt >= Self::MAX_ATTEMPTS => {
                record(db, id, "failed", attempt, code, Some(&why), duration, None).await?;
                fail(state, &endpoint, &why).await
            }
            Outcome::Retry(code, why) => {
                let wait = i64::try_from(Self::backoff(attempt).as_secs()).unwrap_or(i64::MAX);
                let next = Some(unix_now().saturating_add(wait));
                record(
                    db,
                    id,
                    "retrying",
                    attempt,
                    code,
                    Some(&why),
                    duration,
                    next,
                )
                .await?;
                Err(anyhow::anyhow!(why).into())
            }
        }
    }
}

/// Writes the outcome of an attempt on the delivery row.
#[allow(clippy::too_many_arguments)]
async fn record(
    db: &Db,
    id: i64,
    status: &str,
    attempt: u32,
    response_status: Option<u16>,
    error: Option<&str>,
    duration_ms: i64,
    next_attempt_at: Option<i64>,
) -> Result {
    crate::db::sql(
        "UPDATE webhook_deliveries SET status = ?, attempt = ?, response_status = ?, error = ?, \
         duration_ms = ?, attempted_at = ?, next_attempt_at = ? WHERE id = ?",
    )
    .bind(status)
    .bind(i64::from(attempt))
    .bind(response_status.map(i64::from))
    .bind(error)
    .bind(duration_ms)
    .bind(unix_now())
    .bind(next_attempt_at)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// Counts a delivery that ended in failure; disables the endpoint (and emits
/// [`EndpointDisabled`]) once `WEBHOOK_DISABLE_AFTER` in a row failed.
async fn fail(state: &AppState, endpoint: &Endpoint, reason: &str) -> Result {
    crate::db::sql(
        "UPDATE webhook_endpoints SET consecutive_failures = consecutive_failures + 1, \
         updated_at = ? WHERE id = ?",
    )
    .bind(unix_now())
    .bind(endpoint.id)
    .execute(&state.db)
    .await?;
    let Some(current) = Endpoint::find(&state.db, endpoint.id).await? else {
        return Ok(());
    };
    let limit = i64::from(state.config.webhook_disable_after);
    if current.enabled && current.consecutive_failures >= limit {
        let why = format!("{limit} deliveries in a row failed (last: {reason})");
        Endpoint::disable(&state.db, endpoint.id, &why).await?;
        let event = EndpointDisabled {
            endpoint_id: endpoint.id,
        };
        state.emit(event).await?;
    }
    Ok(())
}

/// One stored delivery of an event to an endpoint: the log behind
/// `webhook:deliveries`. The body is kept so [`Delivery::redeliver`] can send
/// it again.
#[derive(Debug, Clone)]
pub struct Delivery {
    /// The delivery id.
    pub id: i64,
    /// The endpoint it goes to.
    pub endpoint_id: i64,
    /// The `webhook-id` header, the same for every attempt and redelivery.
    pub message_id: String,
    /// The event name, such as `invoice.paid`.
    pub event_type: String,
    /// `pending`, `retrying`, `succeeded` or `failed`.
    pub status: String,
    /// The number of the last attempt (0 before the first).
    pub attempt: i64,
    /// The HTTP status the endpoint answered on the last attempt.
    pub response_status: Option<i64>,
    /// Why the last attempt failed.
    pub error: Option<String>,
    /// How long the last attempt took, in milliseconds.
    pub duration_ms: Option<i64>,
    /// When the delivery was created.
    pub created_at: DateTime<Utc>,
}

impl Delivery {
    /// The newest deliveries first, at most `limit`; only those of
    /// `endpoint_id` and/or only the ones that `failed` when asked.
    pub async fn list(
        db: &Db,
        endpoint_id: Option<i64>,
        failed_only: bool,
        limit: u32,
    ) -> Result<Vec<Self>> {
        let rows = crate::db::sql(
            "SELECT id, endpoint_id, message_id, event_type, status, attempt, response_status, \
             error, duration_ms, created_at FROM webhook_deliveries \
             WHERE (? = 0 OR endpoint_id = ?) AND (? = 0 OR status = 'failed') \
             ORDER BY id DESC LIMIT ?",
        )
        .bind(i64::from(endpoint_id.is_some()))
        .bind(endpoint_id.unwrap_or(0))
        .bind(i64::from(failed_only))
        .bind(i64::from(limit))
        .fetch_all(db)
        .await?;
        rows.iter()
            .map(|row| -> std::result::Result<Self, DbError> {
                Ok(Self {
                    id: row.try_get("id")?,
                    endpoint_id: row.try_get("endpoint_id")?,
                    message_id: row.try_get("message_id")?,
                    event_type: row.try_get("event_type")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    response_status: row.try_get("response_status")?,
                    error: row.try_get("error")?,
                    duration_ms: row.try_get("duration_ms")?,
                    created_at: crate::db::from_unix(row.try_get("created_at")?),
                })
            })
            .collect::<std::result::Result<_, _>>()
            .map_err(Into::into)
    }

    /// Sends the stored body of delivery `id` again, with a fresh timestamp
    /// and the same `webhook-id`. `false` when there is no such delivery.
    /// Fails when its endpoint is disabled: enable it first.
    pub async fn redeliver(state: &AppState, id: i64) -> Result<bool> {
        let row = crate::db::sql("SELECT endpoint_id FROM webhook_deliveries WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
        let Some(row) = row else { return Ok(false) };
        let endpoint_id: i64 = row.try_get("endpoint_id")?;
        match Endpoint::find(&state.db, endpoint_id).await? {
            Some(endpoint) if endpoint.enabled => {}
            _ => {
                return Err(anyhow::anyhow!(
                    "the endpoint #{endpoint_id} of delivery #{id} is disabled or gone"
                )
                .into());
            }
        }
        crate::db::sql(
            "UPDATE webhook_deliveries SET status = 'pending', error = NULL, \
             next_attempt_at = NULL WHERE id = ?",
        )
        .bind(id)
        .execute(&state.db)
        .await?;
        state
            .queue
            .dispatch(DeliverWebhook {
                endpoint_id,
                delivery_id: id,
            })
            .await?;
        Ok(true)
    }

    /// Deletes the deliveries created more than `age` ago; returns how many.
    pub async fn prune(db: &Db, age: Duration) -> Result<u64> {
        Ok(
            crate::db::sql("DELETE FROM webhook_deliveries WHERE created_at < ?")
                .bind(unix_now() - age.as_secs() as i64)
                .execute(db)
                .await?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_follows_the_standard_webhooks_schedule() {
        let waits: Vec<u64> = (1..=9)
            .map(|attempt| DeliverWebhook::backoff(attempt).as_secs())
            .collect();
        assert_eq!(
            waits,
            [5, 300, 1_800, 7_200, 18_000, 36_000, 36_000, 36_000, 36_000]
        );
    }
}
