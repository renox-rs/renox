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
    Billing, BoxFuture, Checkout, CheckoutRequest, Gateway, Notice, Owner, Payment, Plan, Remote,
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

    fn parse_webhook(&self, body: &[u8]) -> Result<Vec<Notice>> {
        let event: DemoEvent = renox::serde_json::from_slice(body)?;
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
        let body = renox::serde_json::to_string(&self.event)?;
        let signature = renox::webhook::hmac_sha256_hex(signing_key(&ctx.state.config), &body);
        let url = format!(
            "{}/billing/webhooks/demo",
            ctx.state.config.url.trim_end_matches('/')
        );
        let response = ctx
            .state
            .http
            .post(url)
            .header(SIGNATURE_HEADER, signature)
            .body("application/json", body)
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
// [/explain:plans.demo.job]

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

/// The JSON of a [`DemoEvent`] and its signature, for tests that post the
/// webhook themselves.
pub fn signed(config: &Config, event: &DemoEvent) -> (String, String) {
    let body = renox::serde_json::to_string(event).unwrap_or_default();
    let signature = renox::webhook::hmac_sha256_hex(signing_key(config), &body);
    (body, signature)
}
