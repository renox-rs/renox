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

mod inbound;
pub mod signing;

pub use inbound::*;
