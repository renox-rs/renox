//! The payment gateway: its hosted page, its signed webhook, and the
//! page the customer comes back to.
//!
//! **Midtrans** (Snap): with `MIDTRANS_SERVER_KEY`
//! set, [`hosted_page`] asks Midtrans for a payment page (through
//! `state.http`) and the customer pays there; Midtrans then calls
//! `POST /webhooks/midtrans` with an HTTP notification whose `signature_key`
//! is the SHA-512 of order id, status code, amount and the server key.
//! Midtrans only charges rupiah: with a real key the shop runs with
//! `APP_CURRENCY=IDR` (amounts are then whole rupiah, as Midtrans counts
//! them). The demo gateway takes the shop's dollars (cents) as they are.
//!
//! **The demo gateway**, without a key: a page of the app itself
//! (`/pay/demo/{payment}`, a signed URL) stands in for Midtrans' page, so
//! the example can be followed end to end on a laptop. Paying there queues
//! a job ([`DemoNotify`]) that sends the very notification Midtrans would,
//! signed the same way (with a key derived from `APP_KEY`), over HTTP to
//! this app's own webhook. Nothing is special-cased: the webhook can't tell
//! the two apart.
//!
//! The webhook goes through `renox::webhook`: [`Midtrans::verify`] checks
//! the signature (401 otherwise), `event_id` is `transaction:status`, so a
//! notification sent twice is stored and handled once, and [`Midtrans::handle`]
//! runs in a queue worker, retried on errors (`webhook:failed`,
//! `webhook:retry`). It marks the payment paid (`payments::mark_paid`),
//! whose `PaymentSucceeded` the areas listen to: an order becomes paid
//! (src/app/sales/orders.rs), a rental's deposit is held, a work order is
//! settled.
//!
//! After paying, the customer lands on `/pay/{payment}` (`pay.show`): it
//! waits for the webhook (htmx asks again every two seconds) and shows the
//! result.

use std::time::Duration;

use renox::analytics;
use renox::prelude::*;
use renox::signed::ValidSignature;
use renox::webhook;
use serde::{Deserialize, Serialize};

use super::model::{Order, OrderStatus, Payment, PaymentStatus};
use super::orders;
use super::payments::{self, Payable};
use crate::app::accounts::model::Customer;

/// How long the demo gateway's page link works.
pub const DEMO_LINK_TTL: Duration = Duration::from_secs(60 * 60);
/// The session key of the payments this browser started.
pub const SESSION_PAYMENTS: &str = "my_payments";
/// Session keys of purchases already reported to analytics.
pub const SESSION_REPORTED: &str = "purchases_reported";

/// The order id the gateway knows a payment by: `BS-{payment id}`.
pub fn reference(payment_id: i64) -> String {
    format!("BS-{payment_id}")
}

/// The payment id in a gateway order id (`BS-42` → 42).
pub fn payment_id(reference: &str) -> Option<i64> {
    reference.strip_prefix("BS-")?.parse().ok()
}

/// Whether the real Midtrans is configured (else the demo gateway is used).
pub fn midtrans_configured(state: &AppState) -> bool {
    state.config.var("MIDTRANS_SERVER_KEY").is_some()
}

/// The server key notifications are signed with: Midtrans' own, or for
/// the demo gateway one derived from `APP_KEY` (never written anywhere).
pub fn server_key(state: &AppState) -> String {
    state.config.var("MIDTRANS_SERVER_KEY").unwrap_or_else(|| {
        webhook::sha256_hex(format!(
            "bikeshop-demo-gateway:{}",
            state.config.key.clone().unwrap_or_default()
        ))
    })
}

/// A notification's signature: SHA-512 of order id, status code, gross
/// amount and the server key (Midtrans' rule).
pub fn signature(order_id: &str, status_code: &str, gross_amount: &str, key: &str) -> String {
    webhook::sha512_hex(format!("{order_id}{status_code}{gross_amount}{key}"))
}

/// An amount as Midtrans writes it: `450000.00` (whole rupiah). The demo
/// gateway writes the shop's smallest unit (cents) the same way, signed and
/// checked alike.
pub fn gross(amount: i64) -> String {
    format!("{amount}.00")
}

#[derive(Deserialize)]
struct SnapPage {
    redirect_url: String,
}

/// A token that lets whoever comes back from the gateway see `/pay/{id}`
/// (an HMAC of the id with `APP_KEY`). Unlike a signed URL it survives the
/// query parameters Midtrans adds to its "finish" redirect.
pub fn pay_token(state: &AppState, payment_id: i64) -> String {
    let key = state.config.key.clone().unwrap_or_default();
    webhook::hmac_sha256_hex(key, format!("bikeshop-pay:{payment_id}"))[..32].to_owned()
}

/// Where the gateway sends the customer back: `/pay/{id}?token=…`.
pub fn back_url(state: &AppState, payment_id: i64) -> Result<String> {
    Ok(format!(
        "{}?token={}",
        state.absolute_url("pay.show", &[&payment_id])?,
        pay_token(state, payment_id)
    ))
}

/// The page the customer pays on, for the pending `payment`: Midtrans'
/// (Snap) when configured, else the demo gateway's (a signed link).
pub async fn hosted_page(state: &AppState, payment: &Payment) -> Result<String> {
    let back = back_url(state, payment.id)?;
    if !midtrans_configured(state) {
        return state.signed_url("pay.demo", &[&payment.id], DEMO_LINK_TTL);
    }
    let customer = match payment.customer_id {
        Some(id) => Customer::find(&state.db, id).await?,
        None => None,
    };
    let key = state.config.var("MIDTRANS_SERVER_KEY").unwrap_or_default();
    let base = state
        .config
        .var("MIDTRANS_URL")
        .unwrap_or_else(|| "https://app.sandbox.midtrans.com".into());
    let response = state
        .http
        .post(format!("{base}/snap/v1/transactions"))
        .basic_auth(&key, "")
        .json(&json!({
            "transaction_details": { "order_id": reference(payment.id), "gross_amount": payment.amount },
            "customer_details": {
                "first_name": customer.as_ref().map(|c| c.name.clone()),
                "email": customer.as_ref().and_then(|c| c.email.clone()),
            },
            "callbacks": { "finish": back },
        }))
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if !response.ok() {
        return Err(abort(
            StatusCode::BAD_GATEWAY,
            format!(
                "Midtrans answered {}; try again in a minute.",
                response.status()
            ),
        ));
    }
    Ok(response.json::<SnapPage>()?.redirect_url)
}

/// Midtrans' HTTP notification (the fields the shop reads).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Notification {
    pub order_id: String,
    pub status_code: String,
    pub gross_amount: String,
    pub signature_key: String,
    pub transaction_id: String,
    pub transaction_status: String,
    #[serde(default)]
    pub fraud_status: Option<String>,
}

impl Notification {
    /// A notification for `payment` with `status` (`settlement`, `cancel`…),
    /// signed with `key`, as Midtrans sends them.
    pub fn signed(payment: &Payment, status: &str, key: &str) -> Notification {
        let order_id = reference(payment.id);
        let status_code = if status == "settlement" { "200" } else { "202" };
        let gross_amount = gross(payment.amount);
        Notification {
            signature_key: signature(&order_id, status_code, &gross_amount, key),
            order_id,
            status_code: status_code.into(),
            gross_amount,
            transaction_id: renox::db::Ulid::new().to_string().to_lowercase(),
            transaction_status: status.into(),
            fraud_status: Some("accept".into()),
        }
    }
}

/// The Midtrans webhook (`POST /webhooks/midtrans`), through `renox::webhook`.
pub struct Midtrans;

// [explain:pay.show.webhook]
impl Webhook for Midtrans {
    const PROVIDER: &'static str = "midtrans";

    fn verify(request: &WebhookRequest, state: &AppState) -> Result {
        let n: Notification = request.json()?;
        let expected = signature(
            &n.order_id,
            &n.status_code,
            &n.gross_amount,
            &server_key(state),
        );
        webhook::ensure(webhook::same(&n.signature_key, &expected))
    }

    fn event_id(request: &WebhookRequest) -> Result<String> {
        let n: Notification = request.json()?;
        Ok(format!("{}:{}", n.transaction_id, n.transaction_status))
    }

    // [/explain:pay.show.webhook]
    async fn handle(call: WebhookCall, ctx: JobContext) -> Result {
        let n: Notification = call.json()?;
        let Some(id) = payment_id(&n.order_id) else {
            return Err(Error::permanent(renox::anyhow::anyhow!(
                "unknown order id {}",
                n.order_id
            )));
        };
        let Some(payment) = Payment::find(&ctx.state.db, id).await? else {
            return Err(Error::permanent(renox::anyhow::anyhow!("no payment {id}")));
        };
        if n.gross_amount != gross(payment.amount) {
            return Err(Error::permanent(renox::anyhow::anyhow!(
                "payment {id}: {} paid, {} due",
                n.gross_amount,
                payment.amount
            )));
        }
        // [explain:pay.show.webhook]
        let paid = n.transaction_status == "settlement"
            || (n.transaction_status == "capture" && n.fraud_status.as_deref() == Some("accept"));
        if paid {
            payments::mark_paid(&ctx.state, id, &n.transaction_id).await?;
        } else if matches!(
            n.transaction_status.as_str(),
            "cancel" | "deny" | "expire" | "failure"
        ) {
            payments::mark_failed(&ctx.state, id).await?;
        }
        Ok(())
    }
    // [/explain:pay.show.webhook]
}

/// The demo gateway's notification, sent to this app's webhook from the
/// queue (as Midtrans would, a moment after the customer pays).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DemoNotify {
    pub notification: Notification,
}

// [explain:pay.demo.job]
impl Job for DemoNotify {
    const NAME: &'static str = "bikeshop.demo-gateway-notify";
    const MAX_ATTEMPTS: u32 = 5;

    async fn handle(self, ctx: JobContext) -> Result {
        let url = format!(
            "{}/webhooks/midtrans",
            ctx.state.config.url.trim_end_matches('/')
        );
        let response = ctx
            .state
            .http
            .post(url)
            .json(&self.notification)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        if !response.ok() {
            return Err(
                renox::anyhow::anyhow!("the webhook answered {}", response.status()).into(),
            );
        }
        Ok(())
    }
}
// [/explain:pay.demo.job]

/// What a payment pays for, as the pages show it.
#[derive(Serialize, Debug, Clone)]
pub struct Paying {
    pub payment: Payment,
    /// `orders`, `rentals`, `work_orders`.
    pub kind: String,
    /// The order, when it pays for one.
    pub order: Option<Order>,
}

async fn paying(db: &Db, payment: Payment) -> Result<Paying> {
    let order = match Payable::of(&payment) {
        Some(Payable::Order(id)) => Order::find(db, id).await?,
        _ => None,
    };
    Ok(Paying {
        kind: payment.payable_type.clone(),
        payment,
        order,
    })
}

/// Remembers that this browser started `payment_id` (for `/pay/{payment}`).
pub fn remember(session: &Session, payment_id: i64) -> Result {
    let mut ids: Vec<i64> = session.get(SESSION_PAYMENTS).unwrap_or_default();
    if !ids.contains(&payment_id) {
        ids.push(payment_id);
        if ids.len() > 20 {
            ids.remove(0);
        }
        session.put(SESSION_PAYMENTS, &ids)?;
    }
    Ok(())
}

/// The payment, if this visitor started it, came back from the gateway
/// with its token, or owns it as a customer.
async fn own_payment(
    state: &AppState,
    session: &Session,
    user: Option<&User>,
    id: i64,
    token: Option<&str>,
) -> Result<Payment> {
    let db = &state.db;
    let payment = Payment::find_or_404(db, id).await?;
    if token.is_some_and(|t| webhook::same(t, &pay_token(state, id))) {
        remember(session, id)?;
        if let Some(Payable::Order(order)) = Payable::of(&payment) {
            orders::remember(session, order)?;
        }
        return Ok(payment);
    }
    let mine: Vec<i64> = session.get(SESSION_PAYMENTS).unwrap_or_default();
    if mine.contains(&id) {
        return Ok(payment);
    }
    if let (Some(user), Some(customer)) = (user, payment.customer_id)
        && Customer::find(db, customer)
            .await?
            .is_some_and(|c| c.user_id == Some(user.id))
    {
        return Ok(payment);
    }
    Err(Error::NotFound)
}

/// `?token=` on the way back from the gateway.
#[derive(Deserialize, Default)]
pub struct ReturnToken {
    token: Option<String>,
}

/// `GET /pay/{payment}` (`pay.show`): where the customer comes back to
/// after paying. While the webhook hasn't arrived it says so and htmx asks
/// again every two seconds (the `status` block); then it shows the result.
/// A paid order is reported to analytics once (`purchase`).
// [explain:pay.show.handler]
pub async fn show(
    State(state): State<AppState>,
    session: Session,
    user: Option<AuthUser>,
    Path(id): Path<i64>,
    Query(back): Query<ReturnToken>,
) -> Result<View> {
    let payment = own_payment(&state, &session, user.as_deref(), id, back.token.as_deref()).await?;
    let data = paying(&state.db, payment).await?;
    // [/explain:pay.show.handler]
    if data.payment.status == PaymentStatus::Paid
        && let Some(order) = &data.order
    {
        let mut reported: Vec<i64> = session.get(SESSION_REPORTED).unwrap_or_default();
        if !reported.contains(&order.id) {
            analytics::event(
                &session,
                "purchase",
                json!({
                    "transaction_id": order.number,
                    "value": order.total,
                    "currency": state.config.currency,
                }),
            )?;
            reported.push(order.id);
            session.put(SESSION_REPORTED, &reported)?;
        }
    }
    // [explain:pay.show.handler]
    let guest = user.is_none();
    let expired = data
        .order
        .as_ref()
        .is_some_and(|o| o.status == OrderStatus::Cancelled);
    Ok(view(
        "sales/pay/show.html",
        context! { paying => data, guest, expired, minutes => orders::PAY_WITHIN_MINUTES },
    )
    .fragment("status"))
}
// [/explain:pay.show.handler]

/// `GET /pay/demo/{payment}` (`pay.demo`): the demo gateway's hosted page
/// (only through the signed link `payments::start` gave).
// [explain:pay.demo.handler]
pub async fn demo(
    _: ValidSignature,
    State(db): State<Db>,
    uri: renox::axum::http::Uri,
    Path(id): Path<i64>,
) -> Result<View> {
    let payment = Payment::find_or_404(&db, id).await?;
    let data = paying(&db, payment).await?;
    // The form posts to the same signed address: only the link's holder can pay.
    let action = uri.to_string();
    Ok(view(
        "sales/pay/demo.html",
        context! { paying => data, action },
    ))
}
// [/explain:pay.demo.handler]

/// What the demo page sends: pay or cancel.
#[derive(Deserialize, Validate, Debug)]
pub struct DemoForm {
    #[validate(required, one_of(&["settlement", "cancel"]))]
    pub outcome: String,
}

/// `POST /pay/demo/{payment}` (`pay.demo.complete`): the demo gateway
/// "takes" the payment, queues its notification to the webhook, and sends
/// the customer back to the shop, as Midtrans would.
// [explain:pay.demo.handler]
pub async fn demo_complete(
    _: ValidSignature,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Valid(form): Valid<DemoForm>,
) -> Result<Redirect> {
    abort_if(
        midtrans_configured(&state),
        StatusCode::NOT_FOUND,
        "The demo gateway is off when Midtrans is configured.",
    )?;
    let payment = Payment::find_or_404(&state.db, id).await?;
    if payment.status == PaymentStatus::Pending {
        let notification = Notification::signed(&payment, &form.outcome, &server_key(&state));
        state.dispatch(DemoNotify { notification }).await?;
    }
    Ok(Redirect::to(&back_url(&state, id)?))
}
// [/explain:pay.demo.handler]
