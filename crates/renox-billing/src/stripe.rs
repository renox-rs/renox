//! Stripe: Checkout Sessions in `subscription` mode, the Subscriptions API
//! for plan changes (prorated), cancellations and resumes, and the
//! `customer.subscription.*` and `invoice.payment_*` webhooks, signed in
//! the `Stripe-Signature` header.

use std::time::Duration;

use renox::Config;
use renox::axum::http::HeaderMap;
use renox::http::Request;
use renox::prelude::*;
use renox::serde_json::Value;

use crate::gateway::{Secret, from_unix};
use crate::{
    BoxFuture, Checkout, CheckoutRequest, Gateway, Notice, Owner, Payment, Plan, Remote,
    Subscription, SubscriptionStatus,
};

const API: &str = "https://api.stripe.com/v1";
/// How long a call to Stripe may take.
const TIMEOUT: Duration = Duration::from_secs(15);
/// How old a signed webhook may be (Stripe's own libraries use five minutes).
const TOLERANCE: Duration = Duration::from_secs(300);
/// Checkout refuses a trial that ends sooner than this.
const SHORTEST_TRIAL: i64 = 48 * 60 * 60;

/// Stripe, with its secret key (`sk_…`) and the webhook endpoint's signing
/// secret (`whsec_…`).
///
/// ```
/// use renox_billing::{Billing, Stripe};
///
/// # let _ =
/// Billing::new().stripe(); // STRIPE_SECRET and STRIPE_WEBHOOK_SECRET from .env
/// # let _ =
/// Billing::new().gateway(Stripe::new("sk_test_…", "whsec_…")); // or in code
/// ```
///
/// Each plan needs a recurring Price made in Stripe's dashboard:
/// [`Plan::price_id`]`("stripe", "price_…")`, or `STRIPE_PRICE_<KEY>`
/// (`STRIPE_PRICE_PRO`) in `.env`. The webhook endpoint is
/// `/billing/webhooks/stripe`, with the events
/// `customer.subscription.created`, `.updated`, `.deleted`,
/// `invoice.payment_succeeded` and `invoice.payment_failed`.
#[derive(Debug, Clone)]
pub struct Stripe {
    secret: Secret,
    webhook_secret: Secret,
}

impl Stripe {
    /// Stripe with these keys.
    pub fn new(secret_key: impl Into<String>, webhook_secret: impl Into<String>) -> Self {
        Self {
            secret: Secret::Given(secret_key.into()),
            webhook_secret: Secret::Given(webhook_secret.into()),
        }
    }

    /// `STRIPE_SECRET` and `STRIPE_WEBHOOK_SECRET` from the configuration,
    /// read when a call needs them.
    pub fn from_config() -> Self {
        Self {
            secret: Secret::Config("STRIPE_SECRET"),
            webhook_secret: Secret::Config("STRIPE_WEBHOOK_SECRET"),
        }
    }

    /// A request to Stripe's API with the secret key.
    fn call(&self, state: &AppState, method: &str, path: &str) -> Result<Request> {
        let key = self.secret.require(&state.config, "Stripe's secret key")?;
        let url = format!("{API}{path}");
        let request = match method {
            "GET" => state.http.get(url),
            "DELETE" => state.http.delete(url),
            _ => state.http.post(url),
        };
        Ok(request.bearer(&key).timeout(TIMEOUT))
    }
}

/// Sends `request`; Stripe's error message on a 4xx/5xx.
async fn send(request: Request) -> Result<Value> {
    let response = request.send().await?;
    let body: Value = response.json().unwrap_or_default();
    if !response.ok() {
        let message = body["error"]["message"]
            .as_str()
            .unwrap_or("no message")
            .to_owned();
        return Err(
            renox::anyhow::anyhow!("Stripe answered {}: {message}", response.status()).into(),
        );
    }
    Ok(body)
}

fn text(value: &Value) -> Option<String> {
    value.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

/// An id that may be expanded into its object.
fn id_of(value: &Value) -> Option<String> {
    text(value).or_else(|| text(&value["id"]))
}

/// A subscription object as a [`Remote`].
pub(crate) fn remote(sub: &Value) -> Remote {
    let mut remote = Remote::new(text(&sub["id"]).unwrap_or_default());
    if let Some(customer) = id_of(&sub["customer"]) {
        remote = remote.customer(customer);
    }
    let status = match sub["status"].as_str().unwrap_or_default() {
        "trialing" => SubscriptionStatus::Trialing,
        "active" => SubscriptionStatus::Active,
        "past_due" | "unpaid" => SubscriptionStatus::PastDue,
        "paused" => SubscriptionStatus::Paused,
        "canceled" | "incomplete_expired" => SubscriptionStatus::Canceled,
        _ => SubscriptionStatus::Incomplete,
    };
    // A subscription whose collection is paused stays `active` at Stripe.
    let paused = sub["pause_collection"].is_object();
    remote = remote.status(match status {
        SubscriptionStatus::Active if paused => SubscriptionStatus::Paused,
        other => other,
    });
    let metadata = &sub["metadata"];
    if let Some(owner) = text(&metadata["renox_billable"]) {
        remote = remote.owner(owner);
    }
    if let Some(name) = text(&metadata["renox_name"]) {
        remote = remote.name(name);
    }
    if let Some(plan) = text(&metadata["renox_plan"]) {
        remote = remote.plan(plan);
    }
    let item = &sub["items"]["data"][0];
    if let Some(price) = id_of(&item["price"]) {
        remote = remote.price_id(price);
    }
    remote = remote.trial_ends_at(sub["trial_end"].as_i64().and_then(from_unix));
    // Newer API versions keep the period on the subscription's items.
    let period_end = sub["current_period_end"]
        .as_i64()
        .or_else(|| item["current_period_end"].as_i64())
        .and_then(from_unix);
    remote = remote.current_period_end(period_end);
    let ends_at = sub["ended_at"]
        .as_i64()
        .or_else(|| sub["cancel_at"].as_i64())
        .and_then(from_unix)
        .or_else(|| {
            sub["cancel_at_period_end"]
                .as_bool()
                .unwrap_or(false)
                .then_some(period_end)
                .flatten()
        });
    remote.ends_at(ends_at)
}

impl Gateway for Stripe {
    fn name(&self) -> &str {
        "stripe"
    }

    fn label(&self) -> &str {
        "Stripe"
    }

    fn configured(&self, config: &Config) -> bool {
        self.secret.resolve(config).is_some()
    }

    fn prorates(&self) -> bool {
        true
    }

    fn resumes(&self) -> bool {
        true
    }

    fn pauses(&self) -> bool {
        true
    }

    fn create_customer<'a>(
        &'a self,
        state: &'a AppState,
        owner: &'a Owner,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let mut form = vec![("metadata[renox_billable]", owner.key())];
            if let Some(email) = &owner.email {
                form.push(("email", email.clone()));
            }
            if let Some(name) = &owner.name {
                form.push(("name", name.clone()));
            }
            let customer = send(self.call(state, "POST", "/customers")?.form(&form)).await?;
            text(&customer["id"]).ok_or_else(|| {
                renox::anyhow::anyhow!("Stripe made a customer without an id").into()
            })
        })
    }

    fn checkout<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<Checkout>> {
        Box::pin(async move {
            let price = request.price_id.clone().ok_or_else(|| {
                renox::anyhow::anyhow!(
                    "the plan `{}` has no Stripe price: add Plan::price_id(\"stripe\", \"price_…\") \
                     or STRIPE_PRICE_{} in .env",
                    request.plan.key,
                    request.plan.key.to_ascii_uppercase().replace('-', "_")
                )
            })?;
            let mut form = vec![
                ("mode", "subscription".to_owned()),
                ("customer", request.customer_id.clone()),
                ("client_reference_id", request.owner.key()),
                ("success_url", request.success_url.clone()),
                ("cancel_url", request.cancel_url.clone()),
                ("line_items[0][price]", price),
                ("line_items[0][quantity]", "1".to_owned()),
            ];
            for (key, value) in request.metadata() {
                form.push((subscription_metadata(key), value));
            }
            let now = renox::db::now().timestamp();
            if let Some(trial_end) = request.trial_ends_at.map(|at| at.timestamp())
                && trial_end - now >= SHORTEST_TRIAL
            {
                form.push(("subscription_data[trial_end]", trial_end.to_string()));
            }
            let session = send(self.call(state, "POST", "/checkout/sessions")?.form(&form)).await?;
            let url = text(&session["url"]).ok_or_else(|| {
                renox::anyhow::anyhow!("Stripe made a checkout session without a URL")
            })?;
            Ok(Checkout::redirect(url))
        })
    }

    fn swap<'a>(
        &'a self,
        state: &'a AppState,
        subscription: &'a Subscription,
        plan: &'a Plan,
        prorate: bool,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let id = gateway_id(subscription)?;
            let price = plan.price_id_for("stripe", &state.config).ok_or_else(|| {
                renox::anyhow::anyhow!("the plan `{}` has no Stripe price", plan.key)
            })?;
            let current = send(self.call(state, "GET", &format!("/subscriptions/{id}"))?).await?;
            let item = id_of(&current["items"]["data"][0]).ok_or_else(|| {
                renox::anyhow::anyhow!("the Stripe subscription {id} has no item")
            })?;
            let form = vec![
                ("items[0][id]", item),
                ("items[0][price]", price),
                (
                    "proration_behavior",
                    if prorate { "create_prorations" } else { "none" }.to_owned(),
                ),
                ("metadata[renox_plan]", plan.key.clone()),
            ];
            let updated = send(
                self.call(state, "POST", &format!("/subscriptions/{id}"))?
                    .form(&form),
            )
            .await?;
            Ok(remote(&updated))
        })
    }

    fn cancel<'a>(
        &'a self,
        state: &'a AppState,
        subscription: &'a Subscription,
        at_period_end: bool,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let id = gateway_id(subscription)?;
            let path = format!("/subscriptions/{id}");
            let updated = if at_period_end {
                let form = [("cancel_at_period_end", "true")];
                send(self.call(state, "POST", &path)?.form(&form)).await?
            } else {
                send(self.call(state, "DELETE", &path)?).await?
            };
            Ok(remote(&updated))
        })
    }

    fn resume<'a>(
        &'a self,
        state: &'a AppState,
        subscription: &'a Subscription,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let id = gateway_id(subscription)?;
            let form = [("cancel_at_period_end", "false")];
            let updated = send(
                self.call(state, "POST", &format!("/subscriptions/{id}"))?
                    .form(&form),
            )
            .await?;
            Ok(remote(&updated))
        })
    }

    fn pause<'a>(
        &'a self,
        state: &'a AppState,
        subscription: &'a Subscription,
        resumes_at: Option<DateTime>,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let id = gateway_id(subscription)?;
            let mut form = vec![("pause_collection[behavior]", "void".to_owned())];
            if let Some(at) = resumes_at {
                form.push(("pause_collection[resumes_at]", at.timestamp().to_string()));
            }
            let updated = send(
                self.call(state, "POST", &format!("/subscriptions/{id}"))?
                    .form(&form),
            )
            .await?;
            Ok(remote(&updated))
        })
    }

    fn unpause<'a>(
        &'a self,
        state: &'a AppState,
        subscription: &'a Subscription,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let id = gateway_id(subscription)?;
            // An empty value clears the field at Stripe.
            let form = [("pause_collection", "")];
            let updated = send(
                self.call(state, "POST", &format!("/subscriptions/{id}"))?
                    .form(&form),
            )
            .await?;
            Ok(remote(&updated))
        })
    }

    fn verify_webhook(&self, config: &Config, headers: &HeaderMap, body: &[u8]) -> Result {
        let secret = self
            .webhook_secret
            .require(config, "Stripe's webhook signing secret")?;
        let header = headers
            .get("stripe-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        renox::webhook::ensure(renox::webhook::verify_timestamped(
            secret, body, header, TOLERANCE,
        ))
    }

    fn parse_webhook(&self, body: &[u8]) -> Result<Vec<Notice>> {
        let event: Value = renox::serde_json::from_slice(body)?;
        let kind = event["type"].as_str().unwrap_or_default();
        let object = &event["data"]["object"];
        let created = event["created"].as_i64();
        Ok(match kind {
            k if k.starts_with("customer.subscription.") => {
                let mut remote = remote(object);
                if let Some(at) = created {
                    remote = remote.at(at);
                }
                vec![Notice::Subscription(remote)]
            }
            "invoice.payment_succeeded" | "invoice.payment_failed" => {
                let succeeded = kind == "invoice.payment_succeeded";
                let amount = if succeeded {
                    object["amount_paid"].as_i64()
                } else {
                    object["amount_due"].as_i64()
                }
                .unwrap_or(0);
                // A trial's first invoice is for nothing.
                if succeeded && amount == 0 {
                    return Ok(Vec::new());
                }
                let currency = object["currency"].as_str().unwrap_or_default();
                let mut payment = Payment::new(
                    text(&object["id"]).unwrap_or_default(),
                    amount,
                    currency,
                    succeeded,
                );
                // Newer API versions name the subscription under `parent`.
                let subscription = id_of(&object["subscription"])
                    .or_else(|| id_of(&object["parent"]["subscription_details"]["subscription"]));
                if let Some(subscription) = subscription {
                    payment = payment.subscription(subscription);
                }
                if let Some(customer) = id_of(&object["customer"]) {
                    payment = payment.customer(customer);
                }
                vec![Notice::Payment(payment)]
            }
            _ => Vec::new(),
        })
    }
}

/// `subscription_data[metadata][key]`.
fn subscription_metadata(key: &str) -> &'static str {
    match key {
        "renox_billable" => "subscription_data[metadata][renox_billable]",
        "renox_name" => "subscription_data[metadata][renox_name]",
        _ => "subscription_data[metadata][renox_plan]",
    }
}

/// The subscription's id at the gateway.
pub(crate) fn gateway_id(subscription: &Subscription) -> Result<&str> {
    subscription.gateway_id.as_deref().ok_or_else(|| {
        renox::anyhow::anyhow!("the subscription {} has no gateway id", subscription.id).into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use renox::serde_json::json;

    #[test]
    fn reads_a_subscription_with_the_period_on_its_items() {
        let remote = remote(&json!({
            "id": "sub_1",
            "customer": "cus_1",
            "status": "active",
            "cancel_at_period_end": true,
            "metadata": { "renox_billable": "user:3", "renox_plan": "pro" },
            "items": { "data": [{ "id": "si_1", "price": { "id": "price_pro" }, "current_period_end": 1_800_000_000 }] },
        }));
        assert_eq!(remote.id, "sub_1");
        assert_eq!(remote.status, Some(SubscriptionStatus::Active));
        assert_eq!(remote.owner.as_deref(), Some("user:3"));
        assert_eq!(remote.price_id.as_deref(), Some("price_pro"));
        let end = from_unix(1_800_000_000);
        assert_eq!(remote.current_period_end, Some(end));
        // Canceled at the period's end: access until then.
        assert_eq!(remote.ends_at, Some(end));
    }

    #[test]
    fn statuses_stripe_may_add_later_are_not_paid_yet() {
        for (status, expected) in [
            ("trialing", SubscriptionStatus::Trialing),
            ("unpaid", SubscriptionStatus::PastDue),
            ("paused", SubscriptionStatus::Paused),
            ("incomplete_expired", SubscriptionStatus::Canceled),
            ("something_new", SubscriptionStatus::Incomplete),
        ] {
            let remote = remote(&json!({ "id": "sub_1", "status": status }));
            assert_eq!(remote.status, Some(expected), "{status}");
        }
        // An expanded customer object, and an end from `cancel_at`.
        let remote = remote(&json!({
            "id": "sub_1", "status": "active",
            "customer": { "id": "cus_9" }, "cancel_at": 1_800_000_000,
        }));
        assert_eq!(remote.customer_id.as_deref(), Some("cus_9"));
        assert_eq!(remote.ends_at, Some(from_unix(1_800_000_000)));
    }

    #[test]
    fn stripe_prorates_resumes_and_hides_its_keys() {
        let stripe = Stripe::new("sk_live_hidden", "whsec_hidden");
        assert!(stripe.prorates() && stripe.resumes());
        let shown = format!("{stripe:?}");
        assert!(
            !shown.contains("sk_live_hidden") && !shown.contains("whsec_hidden"),
            "{shown}"
        );
        assert!(format!("{:?}", Stripe::from_config()).contains("from STRIPE_SECRET"));
    }
}
