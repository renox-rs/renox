//! Stripe: Checkout Sessions in `subscription` mode, the Subscriptions API
//! for plan changes (prorated), cancellations and resumes, and the
//! `customer.subscription.*` and `invoice.payment_*` webhooks, signed in
//! the `Stripe-Signature` header.

use std::future::Future;
use std::time::Duration;

use renox::Config;
use renox::axum::http::HeaderMap;
use renox::http::Request;
use renox::prelude::*;
use renox::serde_json::Value;

use crate::gateway::{Secret, from_unix};
use crate::payment::{
    BillingPayment, ChargeRequest, DisputeState, DisputeUpdate, PaymentPage, PaymentState,
    PaymentStatus, PaymentUpdate, Refund, RefundResult, RefundStatus, RefundUpdate,
};
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

    /// Stripe whose secret key is looked up by `resolver` on every call, so
    /// a key changed in the app's store is used by the next call without a
    /// restart (nothing is cached). The closure gets the app state and
    /// answers `Ok(None)` when the installation has no key yet; the future
    /// it returns is `Send`. The webhook signing secret stays
    /// `STRIPE_WEBHOOK_SECRET`.
    ///
    /// ```
    /// use renox_billing::Stripe;
    ///
    /// # let _ =
    /// Stripe::resolved(|_state| async move { Ok(Some("sk_test_…".to_string())) });
    /// ```
    pub fn resolved<F, Fut>(resolver: F) -> Self
    where
        F: Fn(&AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        Self {
            secret: Secret::resolved(resolver),
            webhook_secret: Secret::Config("STRIPE_WEBHOOK_SECRET"),
        }
    }

    /// [`Stripe::resolved`] for several accounts: the closure also gets the
    /// account's name (`"default"` for the gateway's default account).
    ///
    /// ```
    /// use renox_billing::Stripe;
    ///
    /// # let _ =
    /// Stripe::resolved_accounts(|_state, account: &str| {
    ///     let key = format!("sk_test_{account}");
    ///     async move { Ok(Some(key)) }
    /// });
    /// ```
    pub fn resolved_accounts<F, Fut>(resolver: F) -> Self
    where
        F: Fn(&AppState, &str) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        Self {
            secret: Secret::resolved_accounts(resolver),
            webhook_secret: Secret::Config("STRIPE_WEBHOOK_SECRET"),
        }
    }

    /// A request to Stripe's API with the secret key.
    async fn call(&self, state: &AppState, method: &str, path: &str) -> Result<Request> {
        self.call_as(state, None, method, path).await
    }

    /// [`Stripe::call`] with the keys of a named account (`None`: default).
    async fn call_as(
        &self,
        state: &AppState,
        account: Option<&str>,
        method: &str,
        path: &str,
    ) -> Result<Request> {
        let key = self
            .secret
            .require_account(state, account, "Stripe's secret key")
            .await?;
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

/// The form fields every one-off payment carries: the row's id and the
/// app's reference, as `{prefix}[renox_…]`.
fn metadata_fields(request: &ChargeRequest, prefix: &str) -> Vec<(String, String)> {
    request
        .metadata()
        .into_iter()
        .map(|(key, value)| (format!("{prefix}[{key}]"), value))
        .collect()
}

/// The amount as a price with its currency (lower case, in cents).
fn price_fields(request: &ChargeRequest) -> Vec<(String, String)> {
    vec![
        (
            "line_items[0][price_data][currency]".into(),
            request.amount.currency.code().to_ascii_lowercase(),
        ),
        (
            "line_items[0][price_data][unit_amount]".into(),
            request.amount.minor.to_string(),
        ),
        ("line_items[0][quantity]".into(), "1".into()),
    ]
}

fn pairs(fields: &[(String, String)]) -> Vec<(&str, String)> {
    fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect()
}

impl Stripe {
    /// A call with the keys of the account a stored payment was made through.
    async fn call_for(
        &self,
        state: &AppState,
        payment: &BillingPayment,
        method: &str,
        path: &str,
    ) -> Result<Request> {
        self.call_as(state, payment.account.as_deref(), method, path)
            .await
    }

    /// The Checkout Session behind a payment: a session is read by id, a
    /// Payment Link by its newest session (`None` while it has none).
    async fn session_of(
        &self,
        state: &AppState,
        payment: &BillingPayment,
    ) -> Result<Option<Value>> {
        let id = payment
            .gateway_id
            .as_deref()
            .ok_or_else(|| renox::anyhow::anyhow!("the payment {} has no Stripe id", payment.id))?;
        if id.starts_with("plink_") {
            let path = format!("/checkout/sessions?payment_link={id}&limit=1");
            let list = send(self.call_for(state, payment, "GET", &path).await?).await?;
            return Ok(list["data"].get(0).cloned());
        }
        let path = format!("/checkout/sessions/{id}");
        Ok(Some(
            send(self.call_for(state, payment, "GET", &path).await?).await?,
        ))
    }

    /// The PaymentIntent a paid payment was made with: the one stored on the
    /// row, else the session's, which is then stored for later notices.
    async fn intent_of(&self, state: &AppState, payment: &BillingPayment) -> Result<String> {
        if let Some(intent) = &payment.gateway_intent {
            return Ok(intent.clone());
        }
        let intent = self
            .session_of(state, payment)
            .await?
            .and_then(|session| id_of(&session["payment_intent"]))
            .ok_or_else(|| {
                renox::anyhow::anyhow!("the payment {} has not been paid at Stripe", payment.id)
            })?;
        crate::sync::remember_intent(state, payment.clone(), Some(&intent)).await?;
        Ok(intent)
    }

    /// Checks the signature with the account's webhook secret (the default
    /// account's when `account` is `None`).
    fn verify_account(
        &self,
        config: &Config,
        account: Option<&str>,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result {
        // An account never falls back to the default account's secret.
        let secret = match account {
            None => self
                .webhook_secret
                .require(config, "Stripe's webhook signing secret")?,
            Some(_) => self
                .webhook_secret
                .resolve_account(config, account)
                .ok_or(Error::Unauthorized)?,
        };
        let header = headers
            .get("stripe-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        renox::webhook::ensure(renox::webhook::verify_timestamped(
            secret, body, header, TOLERANCE,
        ))
    }
}

impl Gateway for Stripe {
    fn name(&self) -> &str {
        "stripe"
    }

    fn label(&self) -> &str {
        "Stripe"
    }

    fn configured(&self, config: &Config) -> bool {
        self.secret.is_set(config)
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
            let customer = send(self.call(state, "POST", "/customers").await?.form(&form)).await?;
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
            let session = send(
                self.call(state, "POST", "/checkout/sessions")
                    .await?
                    .form(&form),
            )
            .await?;
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
            let current = send(
                self.call(state, "GET", &format!("/subscriptions/{id}"))
                    .await?,
            )
            .await?;
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
                self.call(state, "POST", &format!("/subscriptions/{id}"))
                    .await?
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
                send(self.call(state, "POST", &path).await?.form(&form)).await?
            } else {
                send(self.call(state, "DELETE", &path).await?).await?
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
                self.call(state, "POST", &format!("/subscriptions/{id}"))
                    .await?
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
                self.call(state, "POST", &format!("/subscriptions/{id}"))
                    .await?
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
                self.call(state, "POST", &format!("/subscriptions/{id}"))
                    .await?
                    .form(&form),
            )
            .await?;
            Ok(remote(&updated))
        })
    }

    fn charge<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        Box::pin(async move {
            let (Some(success), Some(cancel)) = (&request.success_url, &request.cancel_url) else {
                return Err(Error::BadRequest(
                    "A Stripe charge needs success_url and cancel_url.".into(),
                ));
            };
            let mut form = vec![
                ("mode".to_owned(), "payment".to_owned()),
                ("success_url".to_owned(), success.clone()),
                ("cancel_url".to_owned(), cancel.clone()),
                (
                    "client_reference_id".to_owned(),
                    request
                        .reference_id
                        .clone()
                        .unwrap_or_else(|| request.payment_id.to_string()),
                ),
                (
                    "line_items[0][price_data][product_data][name]".to_owned(),
                    request
                        .description
                        .as_deref()
                        .unwrap_or("Payment")
                        .to_owned(),
                ),
            ];
            form.extend(price_fields(request));
            form.extend(metadata_fields(request, "metadata"));
            form.extend(metadata_fields(request, "payment_intent_data[metadata]"));
            if let Some(email) = request.owner.as_ref().and_then(|o| o.email.clone()) {
                form.push(("customer_email".into(), email));
            }
            // Checkout sessions last 30 minutes to 24 hours.
            let now = renox::db::now().timestamp();
            if let Some(at) = request.expires_at.map(|at| at.timestamp())
                && (now + 1_800..=now + 86_400).contains(&at)
            {
                form.push(("expires_at".into(), at.to_string()));
            }
            let account = request.account.as_deref();
            let session = send(
                self.call_as(state, account, "POST", "/checkout/sessions")
                    .await?
                    .form(&pairs(&form)),
            )
            .await?;
            page_of(&session)
        })
    }

    fn payment_link<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        Box::pin(async move {
            let account = request.account.as_deref();
            // A Payment Link's price points at a Product.
            let name = request.description.as_deref().unwrap_or("Payment");
            let product = send(
                self.call_as(state, account, "POST", "/products")
                    .await?
                    .form(&[("name", name.to_owned())]),
            )
            .await?;
            let product = text(&product["id"])
                .ok_or_else(|| renox::anyhow::anyhow!("Stripe made a product without an id"))?;
            let mut form = vec![
                ("line_items[0][price_data][product]".to_owned(), product),
                // One payment per link.
                (
                    "restrictions[completed_sessions][limit]".to_owned(),
                    "1".to_owned(),
                ),
            ];
            form.extend(price_fields(request));
            form.extend(metadata_fields(request, "metadata"));
            form.extend(metadata_fields(request, "payment_intent_data[metadata]"));
            if let Some(url) = &request.success_url {
                form.push(("after_completion[type]".into(), "redirect".into()));
                form.push(("after_completion[redirect][url]".into(), url.clone()));
            }
            let link = send(
                self.call_as(state, account, "POST", "/payment_links")
                    .await?
                    .form(&pairs(&form)),
            )
            .await?;
            let mut page = page_of(&link)?;
            // Stripe's links do not lapse; the sweep expires them.
            page.expires_at = request.expires_at;
            Ok(page)
        })
    }

    fn refund<'a>(
        &'a self,
        state: &'a AppState,
        payment: &'a BillingPayment,
        refund: &'a Refund,
    ) -> BoxFuture<'a, Result<RefundResult>> {
        Box::pin(async move {
            let intent = self.intent_of(state, payment).await?;
            let mut form = vec![
                ("payment_intent", intent),
                ("amount", refund.amount.minor.to_string()),
                ("metadata[renox_payment]", payment.id.to_string()),
            ];
            if let Some(reason) = &refund.reason {
                form.push(("metadata[renox_reason]", reason.clone()));
            }
            let made = send(
                self.call_for(state, payment, "POST", "/refunds")
                    .await?
                    .form(&form),
            )
            .await?;
            let id = text(&made["id"])
                .ok_or_else(|| renox::anyhow::anyhow!("Stripe made a refund without an id"))?;
            match made["status"].as_str() {
                Some("failed" | "canceled") => Err(renox::anyhow::anyhow!(
                    "Stripe could not refund: {}",
                    made["failure_reason"].as_str().unwrap_or("no reason")
                )
                .into()),
                Some("succeeded") => Ok(RefundResult::succeeded(id)),
                _ => Ok(RefundResult::pending(id)),
            }
        })
    }

    fn payment_status<'a>(
        &'a self,
        state: &'a AppState,
        payment: &'a BillingPayment,
    ) -> BoxFuture<'a, Result<PaymentState>> {
        Box::pin(async move {
            let session = self.session_of(state, payment).await?;
            let status = session.as_ref().map_or(PaymentStatus::Pending, |s| {
                match (s["status"].as_str(), s["payment_status"].as_str()) {
                    (_, Some("paid" | "no_payment_required")) => PaymentStatus::Succeeded,
                    (Some("expired"), _) => PaymentStatus::Expired,
                    _ => PaymentStatus::Pending,
                }
            });
            let mut reported = PaymentState::new(status);
            if let Some(intent) = session.as_ref().and_then(|s| id_of(&s["payment_intent"])) {
                reported = reported.intent(intent.clone());
                let path = format!("/refunds?payment_intent={intent}");
                let list = send(self.call_for(state, payment, "GET", &path).await?).await?;
                for refund in list["data"].as_array().into_iter().flatten() {
                    if let Some(update) = refund_update(refund) {
                        reported = reported.refund(update);
                    }
                }
            }
            Ok(reported)
        })
    }

    fn accept_dispute<'a>(
        &'a self,
        state: &'a AppState,
        payment: &'a BillingPayment,
    ) -> BoxFuture<'a, Result> {
        Box::pin(async move {
            let intent = self.intent_of(state, payment).await?;
            let path = format!("/disputes?payment_intent={intent}&limit=1");
            let list = send(self.call_for(state, payment, "GET", &path).await?).await?;
            let dispute = text(&list["data"][0]["id"]).ok_or_else(|| {
                renox::anyhow::anyhow!("the payment {} has no dispute at Stripe", payment.id)
            })?;
            let close = format!("/disputes/{dispute}/close");
            send(self.call_for(state, payment, "POST", &close).await?).await?;
            Ok(())
        })
    }

    fn verify_webhook(&self, config: &Config, headers: &HeaderMap, body: &[u8]) -> Result {
        self.verify_account(config, None, headers, body)
    }

    fn has_account(&self, state: &AppState, account: &str) -> bool {
        self.webhook_secret
            .resolve_account(&state.config, Some(account))
            .is_some()
    }

    fn verify_webhook_account(
        &self,
        state: &AppState,
        account: Option<&str>,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result {
        self.verify_account(&state.config, account, headers, body)
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
            "checkout.session.completed" | "checkout.session.async_payment_succeeded"
                if object["mode"] == "payment" =>
            {
                // A delayed method completes unpaid and succeeds later.
                if object["payment_status"] == "unpaid" {
                    return Ok(Vec::new());
                }
                vec![session_update(object, PaymentStatus::Succeeded, created)]
            }
            "checkout.session.async_payment_failed" if object["mode"] == "payment" => {
                vec![session_update(object, PaymentStatus::Failed, created)]
            }
            // A Payment Link stays open when one of its sessions lapses.
            "checkout.session.expired"
                if object["mode"] == "payment" && object["payment_link"].is_null() =>
            {
                vec![session_update(object, PaymentStatus::Expired, created)]
            }
            "charge.refunded" => {
                let intent = id_of(&object["payment_intent"]);
                let refunds = object["refunds"]["data"].as_array();
                refunds
                    .into_iter()
                    .flatten()
                    .filter_map(refund_update)
                    .map(|mut update| {
                        if let (None, Some(intent)) = (&update.payment_gateway_id, &intent) {
                            update = update.payment(intent.clone());
                        }
                        Notice::Refund(update)
                    })
                    .collect()
            }
            "charge.refund.updated" => refund_update(object)
                .map(|update| vec![Notice::Refund(update)])
                .unwrap_or_default(),
            "charge.dispute.created" | "charge.dispute.closed" => {
                let state = match (kind, object["status"].as_str()) {
                    ("charge.dispute.created", _) => DisputeState::Opened,
                    (_, Some("won")) => DisputeState::Won,
                    (_, Some("lost")) => DisputeState::Lost,
                    _ => return Ok(Vec::new()),
                };
                let Some(payment) = id_of(&object["payment_intent"]) else {
                    return Ok(Vec::new());
                };
                let mut update = DisputeUpdate::new(payment, state);
                if let Some(due) = object["evidence_details"]["due_by"]
                    .as_i64()
                    .and_then(from_unix)
                {
                    update = update.evidence_due_by(due);
                }
                if let Some(at) = created {
                    update = update.at(at);
                }
                vec![Notice::Dispute(update)]
            }
            _ => Vec::new(),
        })
    }
}

/// What a created Checkout Session or Payment Link says: its id and URL.
fn page_of(object: &Value) -> Result<PaymentPage> {
    let id = text(&object["id"]).ok_or_else(|| renox::anyhow::anyhow!("Stripe made no id"))?;
    let url = text(&object["url"]).ok_or_else(|| renox::anyhow::anyhow!("Stripe made no URL"))?;
    let mut page = PaymentPage::new(id, url);
    if let Some(at) = object["expires_at"].as_i64().and_then(from_unix) {
        page = page.expires_at(at);
    }
    Ok(page)
}

/// A payment notice for a Checkout Session; the sessions of a Payment Link
/// are the link's payment.
fn session_update(session: &Value, status: PaymentStatus, at: Option<i64>) -> Notice {
    let id = id_of(&session["payment_link"])
        .or_else(|| text(&session["id"]))
        .unwrap_or_default();
    let mut update = PaymentUpdate::new(id, status);
    if let Some(at) = at {
        update = update.at(at);
    }
    if let Some(intent) = id_of(&session["payment_intent"]) {
        update = update.intent(intent);
    }
    Notice::OneOff(update)
}

/// A Refund object as an update; `None` for one without an id.
fn refund_update(refund: &Value) -> Option<RefundUpdate> {
    let status = match refund["status"].as_str() {
        Some("succeeded") => RefundStatus::Succeeded,
        Some("failed" | "canceled") => RefundStatus::Failed,
        _ => RefundStatus::Pending,
    };
    let mut update = RefundUpdate::new(text(&refund["id"])?, status);
    if let Some(amount) = refund["amount"].as_i64() {
        update = update.amount(amount);
    }
    if let Some(reason) = text(&refund["failure_reason"]) {
        update = update.reason(reason);
    }
    if let Some(payment) = id_of(&refund["payment_intent"]) {
        update = update.payment(payment);
    }
    Some(update)
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
