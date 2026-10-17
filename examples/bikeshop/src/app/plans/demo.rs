//! The demo billing gateway: subscribing to a plan on a laptop, without
//! Stripe's or Xendit's keys.
//!
//! It is an ordinary renox-billing [`Gateway`] (docs/billing.md, "Another
//! payment provider"), set up only when Stripe isn't and the app doesn't
//! run in production. Its "hosted page" is a page of this app
//! (`/plans/demo-pay/…`, a signed link): paying there queues
//! [`DemoBillingNotify`], which sends the webhook a gateway would send
//! (the subscription, then the payment), signed with a key derived from
//! `APP_KEY`, over HTTP to this app's own `/billing/webhooks/demo`. From
//! there it is renox-billing's usual path: verified, stored once, applied
//! by a queue worker, `SubscriptionCreated` and `PaymentSucceeded` emitted,
//! and the shop's listeners ([`super::sync`]) take over. Nothing is
//! special-cased.
//!
//! **One-off payments.** The same gateway makes payment links
//! (`payment_link`, docs/billing.md, "One-off payments"): a work order's
//! "pay by link" (`/staff/workshop/{order}/pay-link`) gets a signed page of
//! this app (`/plans/demo-pay/link/{payment}`), and paying there queues a
//! one-off webhook ([`DemoPaymentNotify`]) the same way. `PaymentSucceeded`
//! then carries the work order's reference and the workshop marks it paid.
//!
//! A subscription made this way can also be "renewed" from its page
//! (paid, or failed, to see the payment-failed mail and the hold), which a
//! real gateway does on its own each month.

use std::time::Duration;

use renox::Config;
use renox::Environment;
use renox::axum::http::HeaderMap;
use renox::prelude::*;
use renox::signed::ValidSignature;
use renox_billing::{
    Billing, BillingPayment, BoxFuture, ChargeRequest, Checkout, CheckoutRequest, Gateway, Notice,
    Owner, Payment, PaymentPage, PaymentState, PaymentStatus, PaymentUpdate, Plan, Remote,
    Subscription, SubscriptionStatus,
};
use serde::{Deserialize, Serialize};

use super::billing::{CURRENCY, slug_of};
use super::model::{ServicePlan, bike_of_billing_name};
use crate::app::workshop::model::CustomerBike;

/// How long the demo page's link works.
pub const LINK_TTL: Duration = Duration::from_secs(60 * 60);
/// The header carrying the webhook's signature.
pub const SIGNATURE_HEADER: &str = "x-demo-signature";

/// The demo gateway (`demo`).
#[derive(Debug, Clone, Copy)]
pub struct DemoGateway;

/// The key the demo webhooks are signed with: derived from `APP_KEY`,
/// never written anywhere.
pub fn signing_key(config: &Config) -> String {
    renox::webhook::sha256_hex(format!(
        "bikeshop-demo-billing:{}",
        config.key.clone().unwrap_or_default()
    ))
}

/// The demo's subscription id for an owner's named subscription.
fn subscription_id(owner: &str, name: &str) -> String {
    format!("demo_sub_{}_{name}", owner.replace(':', "_"))
}

fn period_end(from: DateTime) -> DateTime {
    renox_billing::Interval::Month.after(from)
}

impl Gateway for DemoGateway {
    fn name(&self) -> &str {
        "demo"
    }

    fn label(&self) -> &str {
        "Demo card"
    }

    /// On when Stripe isn't set up, and never in production.
    fn configured(&self, config: &Config) -> bool {
        config.env != Environment::Production && config.var("STRIPE_SECRET").is_none()
    }

    fn resumes(&self) -> bool {
        true
    }

    fn create_customer<'a>(
        &'a self,
        _state: &'a AppState,
        owner: &'a Owner,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move { Ok(format!("demo_cus_{}", owner.key().replace(':', "_"))) })
    }

    // [explain:plans.demo.checkout]
    /// The demo's page, signed: who, which subscription, which plan.
    fn checkout<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<Checkout>> {
        Box::pin(async move {
            let url = state.signed_url(
                "plans.demo",
                &[&request.owner.id, &request.name, &request.plan.key],
                LINK_TTL,
            )?;
            Ok(Checkout::redirect(url))
        })
    }
    // [/explain:plans.demo.checkout]

    fn swap<'a>(
        &'a self,
        _state: &'a AppState,
        subscription: &'a Subscription,
        plan: &'a Plan,
        _prorate: bool,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let id = subscription.gateway_id.clone().unwrap_or_default();
            Ok(Remote::new(id).plan(plan.key.clone()))
        })
    }

    fn cancel<'a>(
        &'a self,
        _state: &'a AppState,
        subscription: &'a Subscription,
        at_period_end: bool,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let remote = Remote::new(subscription.gateway_id.clone().unwrap_or_default());
            Ok(if at_period_end {
                remote.ends_at(Some(
                    subscription
                        .current_period_end
                        .unwrap_or_else(renox::db::now),
                ))
            } else {
                remote.status(SubscriptionStatus::Canceled)
            })
        })
    }

    fn resume<'a>(
        &'a self,
        _state: &'a AppState,
        subscription: &'a Subscription,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            Ok(Remote::new(subscription.gateway_id.clone().unwrap_or_default()).ends_at(None))
        })
    }

    fn verify_webhook(&self, config: &Config, headers: &HeaderMap, body: &[u8]) -> Result {
        let signature = headers
            .get(SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        renox::webhook::ensure(renox::webhook::verify_hmac_sha256(
            signing_key(config),
            body,
            signature,
        ))
    }

    /// A one-off charge is paid on the same kind of page as a link.
    fn charge<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        Box::pin(async move { link_page(state, request) })
    }

    /// The payment link: a signed page of this app, valid for [`LINK_TTL`].
    fn payment_link<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        Box::pin(async move { link_page(state, request) })
    }

    /// The demo keeps no ledger of its own: a payment is what the webhook
    /// last said, so a status check answers the stored state.
    fn payment_status<'a>(
        &'a self,
        _state: &'a AppState,
        payment: &'a BillingPayment,
    ) -> BoxFuture<'a, Result<PaymentState>> {
        Box::pin(async move { Ok(PaymentState::new(payment.status)) })
    }

    fn parse_webhook(&self, body: &[u8]) -> Result<Vec<Notice>> {
        let event = match renox::serde_json::from_slice(body)? {
            DemoBody::OneOff(update) => {
                let status = if update.paid {
                    PaymentStatus::Succeeded
                } else {
                    PaymentStatus::Failed
                };
                return Ok(vec![Notice::OneOff(
                    PaymentUpdate::new(update.gateway_payment, status).at(update.created),
                )]);
            }
            DemoBody::Subscription(event) => event,
        };
        let sub = &event.subscription;
        let mut notices = vec![Notice::Subscription(
            Remote::new(sub.id.clone())
                .customer(sub.customer.clone())
                .owner(sub.owner.clone())
                .name(sub.name.clone())
                .plan(sub.plan.clone())
                .status(if sub.past_due {
                    SubscriptionStatus::PastDue
                } else {
                    SubscriptionStatus::Active
                })
                .current_period_end(DateTime::from_timestamp(sub.period_end, 0))
                .at(event.created),
        )];
        if let Some(payment) = &event.payment {
            notices.push(Notice::Payment(
                Payment::new(
                    payment.id.clone(),
                    payment.amount,
                    &payment.currency,
                    payment.paid,
                )
                .subscription(sub.id.clone())
                .customer(sub.customer.clone()),
            ));
        }
        Ok(notices)
    }
}

/// The page a charge or a link is paid on: its id at the "gateway" is
/// `demo_pay_{payment}`.
fn link_page(state: &AppState, request: &ChargeRequest) -> Result<PaymentPage> {
    let url = state.signed_url("plans.demo.link", &[&request.payment_id], LINK_TTL)?;
    let expires = renox::db::now() + renox::chrono::Duration::seconds(LINK_TTL.as_secs() as i64);
    Ok(PaymentPage::new(payment_id_at_gateway(request.payment_id), url).expires_at(expires))
}

/// The demo's id for the payment row `payment`.
pub fn payment_id_at_gateway(payment: i64) -> String {
    format!("demo_pay_{payment}")
}

/// What the demo's webhook can carry: a one-off payment's outcome or a
/// subscription (the one-off is tried first, by its own field).
#[derive(Deserialize)]
#[serde(untagged)]
enum DemoBody {
    OneOff(DemoOneOff),
    Subscription(DemoEvent),
}

/// A demo webhook for a one-off payment (a link or a charge).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DemoOneOff {
    pub id: String,
    /// Unix seconds.
    pub created: i64,
    /// The payment's id at the demo gateway (`demo_pay_7`).
    pub gateway_payment: String,
    pub paid: bool,
}

impl DemoOneOff {
    /// The webhook for payment row `payment`: paid, or failed.
    pub fn new(payment: i64, paid: bool) -> Self {
        let n = renox::random_token();
        DemoOneOff {
            id: format!("evt_demo_{}", &n[..16]),
            created: renox::db::now().timestamp(),
            gateway_payment: payment_id_at_gateway(payment),
            paid,
        }
    }
}

/// A demo webhook: the subscription as it is now, and the payment taken.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DemoEvent {
    pub id: String,
    /// Unix seconds (orders late events, as a real gateway's do).
    pub created: i64,
    pub subscription: DemoSubscription,
    pub payment: Option<DemoPayment>,
}

/// The subscription in a [`DemoEvent`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DemoSubscription {
    pub id: String,
    pub customer: String,
    /// renox-billing's owner key, `user:5`.
    pub owner: String,
    /// `bike-12`.
    pub name: String,
    pub plan: String,
    pub past_due: bool,
    /// Unix seconds.
    pub period_end: i64,
}

/// The payment in a [`DemoEvent`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DemoPayment {
    pub id: String,
    pub amount: i64,
    pub currency: String,
    pub paid: bool,
}

impl DemoEvent {
    /// The event for `owner`'s subscription `name` to `plan`: paid for a
    /// month from now, or failed (past due).
    pub fn new(owner_id: i64, name: &str, plan: &str, amount: i64, paid: bool) -> Self {
        let owner = format!("user:{owner_id}");
        let now = renox::db::now();
        let n = renox::random_token();
        DemoEvent {
            id: format!("evt_demo_{}", &n[..16]),
            created: now.timestamp(),
            subscription: DemoSubscription {
                id: subscription_id(&owner, name),
                customer: format!("demo_cus_{}", owner.replace(':', "_")),
                owner,
                name: name.to_owned(),
                plan: plan.to_owned(),
                past_due: !paid,
                period_end: period_end(now).timestamp(),
            },
            payment: Some(DemoPayment {
                id: format!("inv_demo_{}", &n[16..32]),
                amount,
                currency: CURRENCY.into(),
                paid,
            }),
        }
    }
}

/// Sends a [`DemoEvent`] to this app's billing webhook from the queue, a
/// moment after the customer pays (as a gateway would).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DemoBillingNotify {
    pub event: DemoEvent,
}

// [explain:plans.demo.job]
impl Job for DemoBillingNotify {
    const NAME: &'static str = "bikeshop.demo-billing-notify";
    const MAX_ATTEMPTS: u32 = 5;

    async fn handle(self, ctx: JobContext) -> Result {
        deliver(&ctx.state, &self.event).await
    }
}
// [/explain:plans.demo.job]

/// Sends a [`DemoOneOff`] (a payment link was paid, or declined) the same way.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DemoPaymentNotify {
    pub event: DemoOneOff,
}

impl Job for DemoPaymentNotify {
    const NAME: &'static str = "bikeshop.demo-payment-notify";
    const MAX_ATTEMPTS: u32 = 5;

    async fn handle(self, ctx: JobContext) -> Result {
        deliver(&ctx.state, &self.event).await
    }
}

/// Posts `event`, signed, to this app's own demo webhook.
async fn deliver<E: Serialize>(state: &AppState, event: &E) -> Result {
    let body = renox::serde_json::to_string(event)?;
    let signature = renox::webhook::hmac_sha256_hex(signing_key(&state.config), &body);
    let url = format!(
        "{}/billing/webhooks/demo",
        state.config.url.trim_end_matches('/')
    );
    let response = state
        .http
        .post(url)
        .header(SIGNATURE_HEADER, signature)
        .body("application/json", body)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;
    if !response.ok() {
        return Err(renox::anyhow::anyhow!("the webhook answered {}", response.status()).into());
    }
    Ok(())
}

/// What the demo page shows.
#[derive(Serialize)]
struct Paying {
    plan: String,
    amount: i64,
    bike: String,
}

/// `GET /plans/demo-pay/{user}/{name}/{plan}` (`plans.demo`): the demo
/// gateway's hosted page, through the signed link its `checkout` made.
pub async fn page(
    _: ValidSignature,
    State(state): State<AppState>,
    uri: renox::axum::http::Uri,
    Path((_user, name, plan)): Path<(i64, String, String)>,
) -> Result<View> {
    let paying = paying(&state, &name, &plan).await?;
    Ok(view(
        "plans/demo.html",
        context! { paying, action => uri.to_string() },
    ))
}

async fn paying(state: &AppState, name: &str, key: &str) -> Result<Paying> {
    let service = ServicePlan::query()
        .where_(ServicePlan::SLUG.eq(slug_of(key)))
        .first(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    let bike = match bike_of_billing_name(name) {
        Some(id) => CustomerBike::find(&state.db, id).await?,
        None => None,
    };
    Ok(Paying {
        plan: service.name.clone(),
        amount: service.monthly_price(),
        bike: bike.map(|b| b.name).unwrap_or_default(),
    })
}

/// Pay, or decline (the payment fails).
#[derive(Deserialize, Validate, Debug)]
pub struct DemoForm {
    #[validate(required, one_of(&["pay", "decline"]))]
    pub outcome: String,
}

/// `POST /plans/demo-pay/{user}/{name}/{plan}` (`plans.demo.complete`):
/// "takes" the payment, queues the gateway's webhook, and sends the
/// customer back as a gateway would (renox-billing's `billing.return`).
// [explain:plans.demo.complete]
pub async fn complete(
    _: ValidSignature,
    State(state): State<AppState>,
    Path((user, name, plan)): Path<(i64, String, String)>,
    Valid(form): Valid<DemoForm>,
) -> Result<Redirect> {
    abort_if(
        !DemoGateway.configured(&state.config),
        StatusCode::NOT_FOUND,
        "The demo gateway is off.",
    )?;
    let paying = paying(&state, &name, &plan).await?;
    let event = DemoEvent::new(user, &name, &plan, paying.amount, form.outcome == "pay");
    state.dispatch(DemoBillingNotify { event }).await?;
    Redirect::route("billing.return", &[])
}
// [/explain:plans.demo.complete]

/// What the payment link's page shows.
#[derive(Serialize)]
struct LinkPaying {
    order: i64,
    amount: i64,
    done: bool,
}

/// The demo gateway's payment of a link's page, 404 for any other.
async fn link_payment(state: &AppState, id: i64) -> Result<BillingPayment> {
    let payment = BillingPayment::find(&state.db, id)
        .await?
        .ok_or(Error::NotFound)?;
    if payment.gateway != "demo" {
        return Err(Error::NotFound);
    }
    Ok(payment)
}

/// `GET /plans/demo-pay/link/{payment}` (`plans.demo.link`): the demo
/// gateway's page for a payment link, through the signed URL its
/// `payment_link` made.
pub async fn link_show(
    _: ValidSignature,
    State(state): State<AppState>,
    uri: renox::axum::http::Uri,
    Path(id): Path<i64>,
) -> Result<View> {
    let payment = link_payment(&state, id).await?;
    let order = payment
        .reference_id
        .as_deref()
        .and_then(|id| id.parse().ok())
        .unwrap_or_default();
    let paying = LinkPaying {
        order,
        amount: payment.amount.minor,
        done: payment.status != PaymentStatus::Pending,
    };
    Ok(view(
        "plans/demo_link.html",
        context! { paying, action => uri.to_string() },
    ))
}

/// `POST /plans/demo-pay/link/{payment}` (`plans.demo.link.complete`):
/// "takes" the payment (or declines it) and queues the gateway's webhook.
pub async fn link_complete(
    _: ValidSignature,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Valid(form): Valid<DemoForm>,
) -> Result<Redirect> {
    abort_if(
        !DemoGateway.configured(&state.config),
        StatusCode::NOT_FOUND,
        "The demo gateway is off.",
    )?;
    let payment = link_payment(&state, id).await?;
    if payment.status == PaymentStatus::Pending {
        let event = DemoOneOff::new(payment.id, form.outcome == "pay");
        state.dispatch(DemoPaymentNotify { event }).await?;
    }
    Redirect::route("billing.return", &[])
}

/// Renew now, paid or failed.
#[derive(Deserialize, Validate, Debug)]
pub struct RenewForm {
    #[validate(required, one_of(&["paid", "failed"]))]
    pub outcome: String,
}

/// `POST /plans/mine/{subscription}/demo-renew` (`plans.demo.renew`): for a
/// plan paid through the demo gateway, the next month's charge, now, paid
/// or failed (a real gateway charges on its own).
pub async fn renew(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<RenewForm>,
) -> Result<(Toast, Redirect)> {
    let sub = super::mine::own_subscription(&state.db, &user, id).await?;
    abort_if(
        sub.gateway != "demo" || !DemoGateway.configured(&state.config),
        StatusCode::NOT_FOUND,
        "Only plans paid through the demo gateway renew from here.",
    )?;
    let billing = Billing::of(&state, &*user)
        .named(sub.billing_name())
        .subscription()
        .await?
        .ok_or(Error::NotFound)?;
    let plan = ServicePlan::find_or_404(&state.db, sub.service_plan_id).await?;
    let event = DemoEvent::new(
        user.id,
        &sub.billing_name(),
        &billing.plan,
        plan.monthly_price(),
        form.outcome == "paid",
    );
    state.dispatch(DemoBillingNotify { event }).await?;
    Ok((
        Toast::info(state.current_lang().t("plans.demo.renewing", &[])),
        Redirect::route("plans.show", &[&sub.id])?,
    ))
}

/// The JSON of a [`DemoOneOff`] and its signature, for tests that post the
/// webhook themselves.
pub fn signed_one_off(config: &Config, event: &DemoOneOff) -> (String, String) {
    let body = renox::serde_json::to_string(event).unwrap_or_default();
    let signature = renox::webhook::hmac_sha256_hex(signing_key(config), &body);
    (body, signature)
}

/// The JSON of a [`DemoEvent`] and its signature, for tests that post the
/// webhook themselves.
pub fn signed(config: &Config, event: &DemoEvent) -> (String, String) {
    let body = renox::serde_json::to_string(event).unwrap_or_default();
    let signature = renox::webhook::hmac_sha256_hex(signing_key(config), &body);
    (body, signature)
}
