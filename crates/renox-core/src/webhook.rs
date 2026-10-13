//! Webhooks: calls from payment gateways and other services, received
//! safely. For each call Renox:
//!
//! 1. checks it came from the provider ([`Webhook::verify`], usually a
//!    signature over the raw body), answering 401 otherwise;
//! 2. stores it in `webhook_calls`, once per provider event id, so the
//!    provider's retries of the same event are answered 200 and not
//!    processed twice;
//! 3. answers 200 at once and runs [`Webhook::handle`] in a queue worker,
//!    retrying on errors; `webhook:failed` lists what failed and
//!    `webhook:retry <id>` runs a call again.
//!
//! ```
//! # use renox::prelude::*;
//! # #[derive(serde::Deserialize)] struct Invoice { id: String, status: String, external_id: String }
//! # async fn mark_paid(_: &Db, _: &str) -> Result { Ok(()) }
//! use renox::webhook;
//!
//! struct Xendit;
//!
//! impl Webhook for Xendit {
//!     const PROVIDER: &'static str = "xendit";
//!
//!     fn verify(req: &WebhookRequest, state: &AppState) -> Result {
//!         let token = webhook::secret(state, "XENDIT_CALLBACK_TOKEN")?;
//!         webhook::ensure(req.header("x-callback-token").is_some_and(|t| webhook::same(t, &token)))
//!     }
//!
//!     fn event_id(req: &WebhookRequest) -> Result<String> {
//!         let invoice: Invoice = req.json()?;
//!         Ok(format!("{}:{}", invoice.id, invoice.status))
//!     }
//!
//!     async fn handle(call: WebhookCall, ctx: JobContext) -> Result {
//!         let invoice: Invoice = call.json()?;
//!         mark_paid(&ctx.state.db, &invoice.external_id).await
//!     }
//! }
//!
//! // Module::routes:   Routes::new().webhook::<Xendit>("/webhooks/xendit")
//! // Module::register: app.webhook::<Xendit>();
//! ```
//!
//! Webhook routes skip CSRF (callers have no session) and keep working in
//! maintenance mode (calls are stored and processed as usual).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use hmac::{Hmac, KeyInit, Mac};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

use crate::db::{Db, Migration};
use crate::queue::{Job, JobContext, unix_now};
use crate::{AppState, Error, Result};

pub(crate) const MIGRATIONS: [Migration; 2] = [
    crate::db::framework_migration!("webhook", "00010101000300_create_webhook_calls_table"),
    Migration::new(
        "00010101000301_store_webhook_payloads_as_bytes",
        include_str!("../migrations/webhook/00010101000301_store_webhook_payloads_as_bytes.up.sql"),
        Some(include_str!(
            "../migrations/webhook/00010101000301_store_webhook_payloads_as_bytes.down.sql"
        )),
    )
    .postgres(
        include_str!(
            "../migrations/webhook/00010101000301_store_webhook_payloads_as_bytes.postgres.up.sql"
        ),
        Some(include_str!(
            "../migrations/webhook/00010101000301_store_webhook_payloads_as_bytes.postgres.down.sql"
        )),
    ),
];

/// Tells Renox how to receive one provider's webhooks.
pub trait Webhook: Send + Sync + 'static {
    /// A short, stable name such as `"midtrans"`; stored with each call.
    const PROVIDER: &'static str;

    /// Accepts the call only if it really comes from the provider, usually
    /// by checking a signature over [`WebhookRequest::body`]. An error
    /// answers 401 and nothing is stored.
    fn verify(request: &WebhookRequest, state: &AppState) -> Result;

    /// The provider's id for this event. A call whose id was seen before is
    /// answered 200 without being processed again. Include the status when
    /// a provider sends several calls for one object (e.g. `"{id}:{status}"`).
    fn event_id(request: &WebhookRequest) -> Result<String>;

    /// Processes a stored call in a queue worker. An error marks the call
    /// failed and retries it (five attempts in all).
    fn handle(call: WebhookCall, ctx: JobContext) -> impl Future<Output = Result> + Send;
}

/// The incoming call: headers and the raw body, exactly as signed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WebhookRequest {
    /// The request's headers.
    pub headers: HeaderMap,
    /// The raw body, unparsed.
    pub body: Bytes,
}

impl WebhookRequest {
    /// A header's value; `None` if it is missing or not visible ASCII.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The body as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|err| Error::BadRequest(err.to_string()))
    }

    /// The body as a urlencoded form.
    pub fn form<T: DeserializeOwned>(&self) -> Result<T> {
        serde_urlencoded::from_bytes(&self.body).map_err(|err| Error::BadRequest(err.to_string()))
    }
}

setting_enum! {
    /// Where a stored webhook call is, in `webhook_calls.status`.
    pub enum WebhookStatus ("webhook_calls.status") {
        /// Stored, waiting for its job (or retried).
        Received = "received",
        /// Handled without an error.
        Processed = "processed",
        /// Its handler failed; `webhook:retry <id>` runs it again.
        Failed = "failed",
    }
}

/// A stored call, as `handle` gets it.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct WebhookCall {
    /// The `webhook_calls` row id, for `webhook:retry`.
    pub id: i64,
    /// The [`Webhook::PROVIDER`] that received it.
    pub provider: String,
    /// The provider's id, or `sha256:` and its hash when longer than 200 bytes.
    pub event_id: String,
    /// The body exactly as received.
    pub payload: Vec<u8>,
    /// Received, processed or failed.
    pub status: WebhookStatus,
    /// The last processing error; `None` unless `failed`.
    pub error: Option<String>,
    /// When it arrived.
    pub received_at: crate::db::DateTime,
    /// When processing succeeded; `None` until then.
    pub processed_at: Option<crate::db::DateTime>,
}

impl WebhookCall {
    /// The payload as JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_slice(&self.payload)?)
    }

    /// The payload as a urlencoded form.
    pub fn form<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_urlencoded::from_bytes(&self.payload)?)
    }

    /// The payload as text; an error if it isn't UTF-8.
    pub fn text(&self) -> Result<&str> {
        Ok(std::str::from_utf8(&self.payload)?)
    }

    /// The call with this id, if any.
    pub async fn find(db: &Db, id: i64) -> Result<Option<Self>> {
        let row = crate::db::sql(format!("SELECT {COLUMNS} FROM webhook_calls WHERE id = ?"))
            .bind(id)
            .fetch_optional(db)
            .await?;
        row.map(|row| from_row(&row).map_err(Into::into))
            .transpose()
    }

    /// Calls whose processing failed, oldest first.
    pub async fn failed(db: &Db) -> Result<Vec<Self>> {
        let rows = crate::db::sql(format!(
            "SELECT {COLUMNS} FROM webhook_calls WHERE status = 'failed' ORDER BY id"
        ))
        .fetch_all(db)
        .await?;
        Ok(rows
            .iter()
            .map(from_row)
            .collect::<std::result::Result<_, _>>()?)
    }
}

const COLUMNS: &str = "id, provider, event_id, payload, status, error, received_at, processed_at";

fn from_row(row: &crate::db::Row) -> std::result::Result<WebhookCall, crate::db::DbError> {
    Ok(WebhookCall {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        event_id: row.try_get("event_id")?,
        payload: row.try_get("payload")?,
        status: WebhookStatus::parse(&row.try_get::<String>("status")?)
            .map_err(|err| crate::db::DbError::from(sqlx::Error::Decode(err.into())))?,
        error: row.try_get("error")?,
        received_at: crate::db::from_unix(row.try_get("received_at")?),
        processed_at: row
            .try_get::<Option<i64>>("processed_at")?
            .map(crate::db::from_unix),
    })
}

pub(crate) type HandleFn = Arc<
    dyn Fn(WebhookCall, JobContext) -> Pin<Box<dyn Future<Output = Result> + Send>> + Send + Sync,
>;

/// Registered providers and their `handle`.
pub(crate) type Handlers = Arc<HashMap<&'static str, HandleFn>>;

pub(crate) fn handler<W: Webhook>() -> HandleFn {
    Arc::new(|call, ctx| Box::pin(W::handle(call, ctx)))
}

/// The route handler behind `Routes::webhook::<W>(path)`.
pub(crate) async fn receive<W: Webhook>(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = WebhookRequest { headers, body };
    if let Err(err) = W::verify(&request, &state) {
        tracing::warn!(provider = W::PROVIDER, error = ?err, "webhook refused");
        return (StatusCode::UNAUTHORIZED, "invalid webhook").into_response();
    }
    let event_id = match W::event_id(&request) {
        Ok(id) if !id.trim().is_empty() => stored_event_id(id),
        Ok(_) => return Error::BadRequest("the webhook has no event id".into()).into_response(),
        Err(err) => return err.into_response(),
    };
    match store(&state, W::PROVIDER, &event_id, &request.body).await {
        Ok(Some(id)) => {
            tracing::info!(provider = W::PROVIDER, event_id, id, "webhook received");
            (StatusCode::OK, "ok").into_response()
        }
        Ok(None) => {
            tracing::info!(provider = W::PROVIDER, event_id, "webhook already received");
            (StatusCode::OK, "already received").into_response()
        }
        // The provider will send it again.
        Err(err) => err.into_response(),
    }
}

/// Event ids longer than this are stored as their hash, so the unique index
/// stays small (PostgreSQL refuses index rows over about 2.7 KB).
const EVENT_ID_MAX: usize = 200;

fn stored_event_id(id: String) -> String {
    if id.len() <= EVENT_ID_MAX {
        return id;
    }
    format!("sha256:{}", sha256_hex(&id))
}

/// Stores the call and queues its processing in one transaction; `None`
/// when this event was stored before.
async fn store(
    state: &AppState,
    provider: &str,
    event_id: &str,
    body: &[u8],
) -> Result<Option<i64>> {
    let mut tx = state.db.begin().await?;
    let id: Option<i64> = crate::db::sql(
        "INSERT INTO webhook_calls (provider, event_id, payload, status, received_at) \
         VALUES (?, ?, ?, 'received', ?) ON CONFLICT (provider, event_id) DO NOTHING RETURNING id",
    )
    .bind(provider)
    .bind(event_id)
    .bind(body.to_vec())
    .bind(unix_now())
    .scalar_optional(&mut tx)
    .await?;
    if let Some(id) = id {
        state
            .queue
            .dispatch_in(&mut tx, ProcessWebhook { call_id: id })
            .await?;
    }
    tx.commit().await?;
    if id.is_some() {
        state.queue.wake_workers();
    }
    Ok(id)
}

/// Runs a stored call again (e.g. after fixing a bug); `false` if there's no such call.
pub async fn retry(state: &AppState, id: i64) -> Result<bool> {
    let mut tx = state.db.begin().await?;
    let changed = crate::db::sql(
        "UPDATE webhook_calls SET status = 'received', error = NULL, processed_at = NULL WHERE id = ?",
    )
    .bind(id)
    .execute(&mut tx)
    .await?;
    if changed == 0 {
        return Ok(false);
    }
    state
        .queue
        .dispatch_in(&mut tx, ProcessWebhook { call_id: id })
        .await?;
    tx.commit().await?;
    state.queue.wake_workers();
    Ok(true)
}

/// The queue job that runs `Webhook::handle` for a stored call.
#[derive(Serialize, Deserialize)]
pub(crate) struct ProcessWebhook {
    call_id: i64,
}

impl Job for ProcessWebhook {
    const NAME: &'static str = "renox:webhook";
    const MAX_ATTEMPTS: u32 = 5;

    fn backoff(attempt: u32) -> Duration {
        Duration::from_secs(30 * u64::from(attempt))
    }

    async fn handle(self, ctx: JobContext) -> Result {
        let db = ctx.state.db.clone();
        let Some(call) = WebhookCall::find(&db, self.call_id).await? else {
            return Ok(());
        };
        if call.status == WebhookStatus::Processed {
            return Ok(());
        }
        let Some(handle) = ctx.state.webhooks.get(call.provider.as_str()).cloned() else {
            let error = format!("no webhook `{}` is registered", call.provider);
            mark(&db, call.id, WebhookStatus::Failed, Some(&error)).await?;
            return Err(anyhow!(error).into());
        };
        let id = call.id;
        // Its own task, so a panicking handler marks the call failed too.
        let outcome = match tokio::spawn(crate::clock::carry(handle(call, ctx))).await {
            Ok(outcome) => outcome,
            Err(join) => Err(anyhow!(
                "the webhook handler panicked: {}",
                join.try_into_panic()
                    .map(|panic| crate::error::panic_message(&*panic))
                    .unwrap_or_else(|join| join.to_string())
            )
            .into()),
        };
        match outcome {
            Ok(()) => mark(&db, id, WebhookStatus::Processed, None).await,
            Err(err) => {
                mark(&db, id, WebhookStatus::Failed, Some(&format!("{err:?}"))).await?;
                Err(err)
            }
        }
    }
}

async fn mark(db: &Db, id: i64, status: WebhookStatus, error: Option<&str>) -> Result {
    let processed_at = (status == WebhookStatus::Processed).then(unix_now);
    crate::db::sql("UPDATE webhook_calls SET status = ?, error = ?, processed_at = ? WHERE id = ?")
        .bind(status.as_str())
        .bind(error)
        .bind(processed_at)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// Signature helpers.

/// A secret from `.env` (or `Config::vars`), e.g.
/// `secret(state, "MIDTRANS_SERVER_KEY")`; missing is an error.
pub fn secret(state: &AppState, name: &str) -> Result<String> {
    state
        .config
        .var(name)
        .ok_or_else(|| anyhow!("set {name} in .env to receive these webhooks").into())
}

/// `Ok` if `valid`, otherwise the error `verify` returns for a forged call.
pub fn ensure(valid: bool) -> Result {
    if valid {
        Ok(())
    } else {
        Err(Error::Unauthorized)
    }
}

/// Compares two strings in time independent of where they differ.
pub fn same(a: &str, b: &str) -> bool {
    crate::crypto::constant_time_eq(a, b)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Lowercase hex SHA-256 of `data`.
pub fn sha256_hex(data: impl AsRef<[u8]>) -> String {
    hex(&Sha256::digest(data.as_ref()))
}

/// Lowercase hex SHA-512 of `data` (Midtrans' `signature_key`).
pub fn sha512_hex(data: impl AsRef<[u8]>) -> String {
    hex(&Sha512::digest(data.as_ref()))
}

/// Lowercase hex HMAC-SHA256 of `data` with `key`.
pub fn hmac_sha256_hex(key: impl AsRef<[u8]>, data: impl AsRef<[u8]>) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.as_ref())
        .expect("HMAC accepts keys of any length");
    mac.update(data.as_ref());
    hex(&mac.finalize().into_bytes())
}

/// Lowercase hex HMAC-SHA512 of `data` with `key`.
pub fn hmac_sha512_hex(key: impl AsRef<[u8]>, data: impl AsRef<[u8]>) -> String {
    let mut mac = <Hmac<Sha512> as KeyInit>::new_from_slice(key.as_ref())
        .expect("HMAC accepts keys of any length");
    mac.update(data.as_ref());
    hex(&mac.finalize().into_bytes())
}

/// Checks a hex HMAC-SHA256 `signature` of `body`, with or without a
/// `sha256=` prefix (GitHub, Shopify-style hex, …), ignoring hex case.
pub fn verify_hmac_sha256(key: impl AsRef<[u8]>, body: &[u8], signature: &str) -> bool {
    let signature = signature.trim();
    let signature = signature.strip_prefix("sha256=").unwrap_or(signature);
    same(&signature.to_ascii_lowercase(), &hmac_sha256_hex(key, body))
}

/// Checks a Stripe-style signature header, `t=<unix time>,v1=<hex>[,v1=…]`,
/// where each `v1` is HMAC-SHA256 of `"{t}.{body}"`, and refuses one older or
/// newer than `tolerance` (Stripe uses five minutes) so it can't be replayed.
pub fn verify_timestamped(
    key: impl AsRef<[u8]>,
    body: &[u8],
    header: &str,
    tolerance: Duration,
) -> bool {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", value)) => timestamp = value.parse::<i64>().ok(),
            Some(("v1", value)) => signatures.push(value.to_ascii_lowercase()),
            _ => {}
        }
    }
    let Some(timestamp) = timestamp else {
        return false;
    };
    if (unix_now() - timestamp).unsigned_abs() > tolerance.as_secs() {
        return false;
    }
    let mut signed = format!("{timestamp}.").into_bytes();
    signed.extend_from_slice(body);
    let expected = hmac_sha256_hex(key, &signed);
    signatures.iter().any(|s| same(s, &expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_match_known_values() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(sha512_hex("abc").starts_with("ddaf35a193617aba"));
        // RFC 4231 test case 2.
        assert_eq!(
            hmac_sha256_hex("Jefe", "what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert!(
            hmac_sha512_hex("Jefe", "what do ya want for nothing?").starts_with("164b7a7bfcf819e2")
        );
    }

    #[test]
    fn checks_hmac_and_timestamped_signatures() {
        let body = br#"{"id":"evt_1"}"#;
        let signature = hmac_sha256_hex("secret", body);
        assert!(verify_hmac_sha256("secret", body, &signature));
        assert!(verify_hmac_sha256(
            "secret",
            body,
            &format!("sha256={}", signature.to_uppercase())
        ));
        assert!(!verify_hmac_sha256("other", body, &signature));

        let now = unix_now();
        let signed = |t: i64, key: &str| {
            let mut data = format!("{t}.").into_bytes();
            data.extend_from_slice(body);
            format!("t={t},v1=bad,v1={}", hmac_sha256_hex(key, data))
        };
        let five_minutes = Duration::from_secs(300);
        assert!(verify_timestamped(
            "secret",
            body,
            &signed(now, "secret"),
            five_minutes
        ));
        assert!(!verify_timestamped(
            "secret",
            body,
            &signed(now, "other"),
            five_minutes
        ));
        assert!(!verify_timestamped(
            "secret",
            body,
            &signed(now - 600, "secret"),
            five_minutes
        ));
        assert!(!verify_timestamped(
            "secret",
            b"{}",
            &signed(now, "secret"),
            five_minutes
        ));
        assert!(!verify_timestamped("secret", body, "v1=abc", five_minutes));
    }
}
