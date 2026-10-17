//! Xendit: Recurring Plans (`/recurring/plans`), whose hosted page links
//! the customer's card or e-wallet, and the `recurring.plan.*` and
//! `recurring.cycle.*` webhooks, with the `x-callback-token` header. One-off
//! payments are invoices (`/v2/invoices`, a charge and a link alike) and
//! refunds (`/refunds`); their invoice and `refund.*` callbacks are applied
//! like any one-off notice. Xendit has no disputes API here, so
//! `accept_dispute` stays [`Unsupported`](crate::Unsupported).

use std::future::Future;
use std::time::Duration;

use renox::Config;
use renox::axum::http::HeaderMap;
use renox::http::Request;
use renox::prelude::*;
use renox::serde_json::{Value, json};

use crate::gateway::{Secret, from_unix};
use crate::payment::{
    BillingPayment, ChargeRequest, PaymentKind, PaymentPage, PaymentState, PaymentStatus,
    PaymentUpdate, Refund, RefundResult, RefundStatus, RefundUpdate,
};
use crate::stripe::gateway_id;
use crate::{
    BoxFuture, Checkout, CheckoutRequest, Gateway, Notice, Owner, Payment, Plan, Remote,
    Subscription, SubscriptionStatus,
};

const API: &str = "https://api.xendit.co";
/// How long a call to Xendit may take.
const TIMEOUT: Duration = Duration::from_secs(15);

/// Xendit, with its secret API key and the webhook verification token
/// (both from Xendit's dashboard).
///
/// ```
/// use renox_billing::{Billing, Xendit};
///
/// # let _ =
/// Billing::new().xendit(); // XENDIT_SECRET_KEY and XENDIT_CALLBACK_TOKEN from .env
/// # let _ =
/// Billing::new().gateway(Xendit::new("xnd_development_…", "callback-token")); // or in code
/// ```
///
/// Xendit charges the plan's own amount and currency (`IDR` in rupiah). It
/// doesn't prorate a plan change (the new amount applies from the next
/// cycle) or change a plan's interval, and a canceled plan can't be resumed:
/// canceling stops further charges at once and the customer keeps access
/// until the period's end, after which they subscribe again. The webhook
/// URL is `/billing/webhooks/xendit` for the Recurring events.
#[derive(Debug, Clone)]
pub struct Xendit {
    secret: Secret,
    callback_token: Secret,
}

impl Xendit {
    /// Xendit with these keys.
    pub fn new(secret_key: impl Into<String>, callback_token: impl Into<String>) -> Self {
        Self {
            secret: Secret::Given(secret_key.into()),
            callback_token: Secret::Given(callback_token.into()),
        }
    }

    /// `XENDIT_SECRET_KEY` and `XENDIT_CALLBACK_TOKEN` from the
    /// configuration, read when a call needs them.
    pub fn from_config() -> Self {
        Self {
            secret: Secret::Config("XENDIT_SECRET_KEY"),
            callback_token: Secret::Config("XENDIT_CALLBACK_TOKEN"),
        }
    }

    /// Xendit whose secret key is looked up by `resolver` on every call, so
    /// a key changed in the app's store is used by the next call without a
    /// restart (nothing is cached). The closure gets the app state and
    /// answers `Ok(None)` when the installation has no key yet; the future
    /// it returns is `Send`. The callback token stays
    /// `XENDIT_CALLBACK_TOKEN`.
    ///
    /// ```
    /// use renox_billing::Xendit;
    ///
    /// # let _ =
    /// Xendit::resolved(|_state| async move { Ok(Some("xnd_development_…".to_string())) });
    /// ```
    pub fn resolved<F, Fut>(resolver: F) -> Self
    where
        F: Fn(&AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        Self {
            secret: Secret::resolved(resolver),
            callback_token: Secret::Config("XENDIT_CALLBACK_TOKEN"),
        }
    }

    /// [`Xendit::resolved`] for several accounts: the closure also gets the
    /// account's name (`"default"` for the gateway's default account).
    ///
    /// ```
    /// use renox_billing::Xendit;
    ///
    /// # let _ =
    /// Xendit::resolved_accounts(|_state, account: &str| {
    ///     let key = format!("xnd_development_{account}");
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
            callback_token: Secret::Config("XENDIT_CALLBACK_TOKEN"),
        }
    }

    /// A request to Xendit's API with the secret key (HTTP basic auth).
    async fn call(&self, state: &AppState, method: &str, path: &str) -> Result<Request> {
        self.call_as(state, None, method, path).await
    }

    /// [`Xendit::call`] with the keys of a named account (`None`: default).
    async fn call_as(
        &self,
        state: &AppState,
        account: Option<&str>,
        method: &str,
        path: &str,
    ) -> Result<Request> {
        let key = self
            .secret
            .require_account(state, account, "Xendit's secret key")
            .await?;
        let url = format!("{API}{path}");
        let request = match method {
            "GET" => state.http.get(url),
            "PATCH" => state.http.patch(url),
            _ => state.http.post(url),
        };
        Ok(request.basic_auth(&key, "").timeout(TIMEOUT))
    }

    /// Makes an invoice (Xendit's hosted payment page) for `request`: both
    /// a charge and a payment link are one.
    async fn invoice(&self, state: &AppState, request: &ChargeRequest) -> Result<PaymentPage> {
        let reference = match (&request.reference_type, &request.reference_id) {
            (Some(kind), Some(id)) => format!("{kind}-{id}-{}", request.payment_id),
            (None, Some(id)) => format!("{id}-{}", request.payment_id),
            _ => format!("renox-payment-{}", request.payment_id),
        };
        let mut body = json!({
            "external_id": reference,
            "amount": major(&request.amount),
            "currency": request.amount.currency.code(),
            "description": request.description.as_deref().unwrap_or("Payment"),
        });
        let mut metadata = json!({});
        for (key, value) in request.metadata() {
            metadata[key] = json!(value);
        }
        body["metadata"] = metadata;
        if let Some(url) = &request.success_url {
            body["success_redirect_url"] = json!(url);
        }
        if let Some(url) = &request.cancel_url {
            body["failure_redirect_url"] = json!(url);
        }
        if request.kind == PaymentKind::Charge
            && let Some(email) = request.owner.as_ref().and_then(|o| o.email.clone())
        {
            body["payer_email"] = json!(email);
        }
        // How long the page stays open, in seconds.
        if let Some(at) = request.expires_at {
            body["invoice_duration"] = json!((at - renox::db::now()).num_seconds().max(1));
        }
        let account = request.account.as_deref();
        let created = send(
            self.call_as(state, account, "POST", "/v2/invoices")
                .await?
                .json(&body),
        )
        .await?;
        let id = text(&created["id"])
            .ok_or_else(|| renox::anyhow::anyhow!("Xendit made an invoice without an id"))?;
        let url = text(&created["invoice_url"])
            .ok_or_else(|| renox::anyhow::anyhow!("Xendit made an invoice without a URL"))?;
        let mut page = PaymentPage::new(id, url);
        if let Some(at) = time(&created["expiry_date"]) {
            page = page.expires_at(at);
        }
        Ok(page)
    }
}

/// An amount in whole currency units, as Xendit takes it (`45` for 4500
/// cents of USD, `99000` for rupiah, which have no minor unit).
fn major(money: &renox::money::Money) -> Value {
    let scale = 10_i64.pow(money.currency.decimals());
    if money.minor % scale == 0 {
        json!(money.minor / scale)
    } else {
        json!(money.minor as f64 / scale as f64)
    }
}

/// An amount Xendit reports in whole units, in minor units.
fn minor(amount: &Value, currency: &Value) -> Option<i64> {
    let decimals = currency
        .as_str()
        .and_then(|code| renox::money::Currency::parse(code).ok())
        .map_or(0, |currency| currency.decimals());
    let scale = 10_f64.powi(decimals as i32);
    amount.as_f64().map(|value| (value * scale).round() as i64)
}

/// A Refund object as an update; `None` for one without an id.
fn refund_update(refund: &Value) -> Option<RefundUpdate> {
    let status = match refund["status"].as_str() {
        Some("SUCCEEDED") => RefundStatus::Succeeded,
        Some("FAILED" | "CANCELLED") => RefundStatus::Failed,
        _ => RefundStatus::Pending,
    };
    let mut update = RefundUpdate::new(text(&refund["id"])?, status);
    if let Some(amount) = minor(&refund["amount"], &refund["currency"]) {
        update = update.amount(amount);
    }
    if let Some(reason) = text(&refund["failure_code"]) {
        update = update.reason(reason);
    }
    if let Some(invoice) = text(&refund["invoice_id"]) {
        update = update.payment(invoice);
    }
    Some(update)
}

/// Xendit's refund reasons: its own list, else `OTHERS`.
fn refund_reason(reason: &str) -> &'static str {
    match reason.trim().to_ascii_uppercase().as_str() {
        "FRAUDULENT" => "FRAUDULENT",
        "DUPLICATE" => "DUPLICATE",
        "REQUESTED_BY_CUSTOMER" => "REQUESTED_BY_CUSTOMER",
        _ => "OTHERS",
    }
}

/// Sends `request`; Xendit's error message on a 4xx/5xx.
async fn send(request: Request) -> Result<Value> {
    let response = request.send().await?;
    let body: Value = response.json().unwrap_or_default();
    if !response.ok() {
        let message = body["message"].as_str().unwrap_or("no message").to_owned();
        let code = body["error_code"].as_str().unwrap_or_default().to_owned();
        return Err(renox::anyhow::anyhow!(
            "Xendit answered {}: {code} {message}",
            response.status()
        )
        .into());
    }
    Ok(body)
}

fn text(value: &Value) -> Option<String> {
    value.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

/// An RFC 3339 time.
fn time(value: &Value) -> Option<DateTime> {
    value
        .as_str()
        .and_then(|s| renox::chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.to_utc())
}

fn interval(plan: &Plan) -> &'static str {
    match plan.interval {
        crate::Interval::Day => "DAY",
        crate::Interval::Week => "WEEK",
        crate::Interval::Year => "YEAR",
        _ => "MONTH",
    }
}

/// A recurring plan object as a [`Remote`].
fn plan_remote(plan: &Value) -> Remote {
    let mut remote = Remote::new(text(&plan["id"]).unwrap_or_default());
    if let Some(customer) = text(&plan["customer_id"]) {
        remote = remote.customer(customer);
    }
    let metadata = &plan["metadata"];
    if let Some(owner) = text(&metadata["renox_billable"]) {
        remote = remote.owner(owner);
    }
    if let Some(name) = text(&metadata["renox_name"]) {
        remote = remote.name(name);
    }
    if let Some(key) = text(&metadata["renox_plan"]) {
        remote = remote.plan(key);
    }
    // The first charge waits for the trial's end (the schedule's anchor).
    let trial_end = text(&metadata["renox_trial_ends_at"])
        .and_then(|t| t.parse::<i64>().ok())
        .and_then(from_unix);
    let status = match plan["status"].as_str().unwrap_or_default() {
        "ACTIVE" if trial_end.is_some_and(|t| t > renox::db::now()) => SubscriptionStatus::Trialing,
        "ACTIVE" => SubscriptionStatus::Active,
        "INACTIVE" => SubscriptionStatus::Canceled,
        _ => SubscriptionStatus::Incomplete,
    };
    remote = remote.status(status);
    if status == SubscriptionStatus::Trialing {
        remote = remote
            .trial_ends_at(trial_end)
            .current_period_end(trial_end);
    }
    remote
}

impl Xendit {
    /// Checks the callback token against the account's (the default
    /// account's when `account` is `None`); an account never falls back to
    /// the default account's token.
    fn verify_account(
        &self,
        config: &Config,
        account: Option<&str>,
        headers: &HeaderMap,
        _body: &[u8],
    ) -> Result {
        let token = match account {
            None => self
                .callback_token
                .require(config, "Xendit's callback token")?,
            Some(_) => self
                .callback_token
                .resolve_account(config, account)
                .ok_or(Error::Unauthorized)?,
        };
        let sent = headers
            .get("x-callback-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        renox::webhook::ensure(renox::webhook::same(sent, &token))
    }
}

impl Gateway for Xendit {
    fn name(&self) -> &str {
        "xendit"
    }

    fn label(&self) -> &str {
        "Xendit"
    }

    fn configured(&self, config: &Config) -> bool {
        self.secret.is_set(config)
    }

    fn create_customer<'a>(
        &'a self,
        state: &'a AppState,
        owner: &'a Owner,
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            // Unique per business: a database reset must not collide with
            // the customers made before it.
            let suffix: String = renox::random_token().chars().take(10).collect();
            let mut body = json!({
                "reference_id": format!("renox-{}-{}-{suffix}", owner.kind, owner.id),
                "type": "INDIVIDUAL",
                "individual_detail": {
                    "given_names": owner.name.clone().unwrap_or_else(|| "Customer".into()),
                },
            });
            if let Some(email) = &owner.email {
                body["email"] = json!(email);
            }
            let customer = send(self.call(state, "POST", "/customers").await?.json(&body)).await?;
            text(&customer["id"]).ok_or_else(|| {
                renox::anyhow::anyhow!("Xendit made a customer without an id").into()
            })
        })
    }

    fn checkout<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<Checkout>> {
        Box::pin(async move {
            let plan = &request.plan;
            let reference = format!(
                "renox-{}-{}-{}",
                request.owner.key().replace(':', "-"),
                request.name,
                renox::random_token().chars().take(10).collect::<String>()
            );
            let mut metadata = json!({});
            for (key, value) in request.metadata() {
                metadata[key] = json!(value);
            }
            let mut schedule = json!({
                "reference_id": format!("{reference}-schedule"),
                "interval": interval(plan),
                "interval_count": 1,
                "retry_interval": "DAY",
                "retry_interval_count": 1,
                "total_retry": 3,
            });
            let mut body = json!({
                "reference_id": reference,
                "customer_id": request.customer_id,
                "recurring_action": "PAYMENT",
                "currency": plan.currency,
                "amount": plan.amount,
                "failed_cycle_action": "STOP",
                "description": plan.label,
                "success_return_url": request.success_url,
                "failure_return_url": request.cancel_url,
            });
            match request.trial_ends_at {
                // No charge before the trial ends: the first cycle is then.
                Some(trial_end) if trial_end > renox::db::now() => {
                    schedule["anchor_date"] = json!(trial_end.to_rfc3339());
                    metadata["renox_trial_ends_at"] = json!(trial_end.timestamp().to_string());
                }
                _ => body["immediate_action_type"] = json!("FULL_AMOUNT"),
            }
            body["schedule"] = schedule;
            body["metadata"] = metadata;
            let created = send(
                self.call(state, "POST", "/recurring/plans")
                    .await?
                    .json(&body),
            )
            .await?;
            // The page where the customer links a card or an e-wallet.
            let url = created["actions"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|action| text(&action["url"]))
                .unwrap_or_else(|| request.success_url.clone());
            Ok(Checkout::redirect(url))
        })
    }

    fn swap<'a>(
        &'a self,
        state: &'a AppState,
        subscription: &'a Subscription,
        plan: &'a Plan,
        _prorate: bool,
    ) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let id = gateway_id(subscription)?;
            let setup = crate::setup(state)?;
            let current = setup.plan(&subscription.plan)?;
            if current.interval != plan.interval {
                return Err(Error::BadRequest(format!(
                    "Xendit can't move a {} plan to a {} one: cancel, then subscribe to {}.",
                    current.interval.as_str(),
                    plan.interval.as_str(),
                    plan.label
                )));
            }
            let body = json!({
                "amount": plan.amount,
                "currency": plan.currency,
                "metadata": {
                    "renox_billable": subscription.owner().key(),
                    "renox_name": subscription.name,
                    "renox_plan": plan.key,
                },
            });
            send(
                self.call(state, "PATCH", &format!("/recurring/plans/{id}"))
                    .await?
                    .json(&body),
            )
            .await?;
            Ok(Remote::new(id).plan(plan.key.clone()))
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
            send(
                self.call(state, "POST", &format!("/recurring/plans/{id}/deactivate"))
                    .await?,
            )
            .await?;
            let now = renox::db::now();
            // Xendit stops charging now; access lasts until what was paid
            // for (or the trial) runs out.
            let end = if at_period_end {
                subscription
                    .current_period_end
                    .or(subscription.trial_ends_at)
                    .filter(|end| *end > now)
                    .unwrap_or(now)
            } else {
                now
            };
            let remote = Remote::new(id).ends_at(Some(end));
            Ok(if end <= now {
                remote.status(SubscriptionStatus::Canceled)
            } else {
                remote
            })
        })
    }

    fn charge<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        Box::pin(self.invoice(state, request))
    }

    fn payment_link<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        Box::pin(self.invoice(state, request))
    }

    fn refund<'a>(
        &'a self,
        state: &'a AppState,
        payment: &'a BillingPayment,
        refund: &'a Refund,
    ) -> BoxFuture<'a, Result<RefundResult>> {
        Box::pin(async move {
            let invoice = invoice_of(payment)?;
            let mut body = json!({
                "invoice_id": invoice,
                "reference_id": format!(
                    "renox-refund-{}-{}",
                    payment.id,
                    renox::random_token().chars().take(10).collect::<String>()
                ),
                "amount": major(&refund.amount),
                "currency": refund.amount.currency.code(),
                "reason": refund_reason(refund.reason.as_deref().unwrap_or_default()),
                "metadata": { "renox_payment": payment.id.to_string() },
            });
            if let Some(reason) = &refund.reason {
                body["metadata"]["renox_reason"] = json!(reason);
            }
            let made = send(
                self.call_as(state, payment.account.as_deref(), "POST", "/refunds")
                    .await?
                    .json(&body),
            )
            .await?;
            let id = text(&made["id"])
                .ok_or_else(|| renox::anyhow::anyhow!("Xendit made a refund without an id"))?;
            match made["status"].as_str() {
                Some("FAILED" | "CANCELLED") => Err(renox::anyhow::anyhow!(
                    "Xendit could not refund: {}",
                    made["failure_code"].as_str().unwrap_or("no reason")
                )
                .into()),
                Some("SUCCEEDED") => Ok(RefundResult::succeeded(id)),
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
            let invoice = invoice_of(payment)?;
            let account = payment.account.as_deref();
            let found = send(
                self.call_as(state, account, "GET", &format!("/v2/invoices/{invoice}"))
                    .await?,
            )
            .await?;
            let status = match found["status"].as_str() {
                Some("PAID" | "SETTLED") => PaymentStatus::Succeeded,
                Some("EXPIRED") => PaymentStatus::Expired,
                _ => PaymentStatus::Pending,
            };
            let mut reported = PaymentState::new(status);
            let list = send(
                self.call_as(
                    state,
                    account,
                    "GET",
                    &format!("/refunds?invoice_id={invoice}"),
                )
                .await?,
            )
            .await?;
            // Either `{ "data": [...] }` or a bare array.
            let refunds = list["data"].as_array().or_else(|| list.as_array());
            for refund in refunds.into_iter().flatten() {
                if let Some(update) = refund_update(refund) {
                    reported = reported.refund(update);
                }
            }
            Ok(reported)
        })
    }

    fn verify_webhook(&self, config: &Config, headers: &HeaderMap, body: &[u8]) -> Result {
        self.verify_account(config, None, headers, body)
    }

    fn has_account(&self, state: &AppState, account: &str) -> bool {
        self.callback_token
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

    fn webhook_event_id(&self, headers: &HeaderMap, body: &[u8]) -> Result<String> {
        if let Some(id) = headers
            .get("webhook-id")
            .and_then(|v| v.to_str().ok())
            .filter(|id| !id.trim().is_empty())
        {
            return Ok(id.trim().to_owned());
        }
        let event: Value = renox::serde_json::from_slice(body)
            .map_err(|err| Error::BadRequest(format!("the webhook isn't JSON: {err}")))?;
        // An invoice callback is the invoice itself, without an envelope.
        let data = if event["event"].is_null() {
            &event
        } else {
            &event["data"]
        };
        Ok(format!(
            "{}:{}:{}:{}",
            event["event"].as_str().unwrap_or("invoice"),
            data["id"].as_str().unwrap_or_default(),
            data["status"].as_str().unwrap_or_default(),
            data["updated"]
                .as_str()
                .or_else(|| event["created"].as_str())
                .unwrap_or_default(),
        ))
    }

    fn parse_webhook(&self, body: &[u8]) -> Result<Vec<Notice>> {
        let event: Value = renox::serde_json::from_slice(body)?;
        // An invoice callback has no `event`: it is the invoice, paid or
        // expired. The invoice's id is the payment's gateway id.
        if event["event"].is_null() && event["external_id"].is_string() {
            let status = match event["status"].as_str() {
                Some("PAID" | "SETTLED") => PaymentStatus::Succeeded,
                Some("EXPIRED") => PaymentStatus::Expired,
                _ => return Ok(Vec::new()),
            };
            let mut update = PaymentUpdate::new(text(&event["id"]).unwrap_or_default(), status);
            let at = match status {
                PaymentStatus::Succeeded => time(&event["paid_at"]),
                _ => None,
            };
            if let Some(at) = at.or_else(|| time(&event["updated"])) {
                update = update.at(at.timestamp());
            }
            return Ok(vec![Notice::OneOff(update)]);
        }
        let kind = event["event"].as_str().unwrap_or_default();
        let data = &event["data"];
        let at = time(&event["created"]).map(|t| t.timestamp());
        let stamped = |remote: Remote| match at {
            Some(at) => remote.at(at),
            None => remote,
        };
        let plan_id = text(&data["plan_id"]).unwrap_or_default();
        let payment = |succeeded: bool| {
            let mut payment = Payment::new(
                text(&data["id"]).unwrap_or_default(),
                data["amount"].as_i64().unwrap_or(0),
                data["currency"].as_str().unwrap_or_default(),
                succeeded,
            )
            .subscription(plan_id.clone());
            if let Some(customer) = text(&data["customer_id"]) {
                payment = payment.customer(customer);
            }
            Notice::Payment(payment)
        };
        Ok(match kind {
            "recurring.plan.activated" | "recurring.plan.inactivated" => {
                vec![Notice::Subscription(stamped(plan_remote(data)))]
            }
            // The next charge: the end of the period paid for.
            "recurring.cycle.created" => vec![Notice::Subscription(stamped(
                Remote::new(plan_id.clone()).current_period_end(time(&data["scheduled_timestamp"])),
            ))],
            "recurring.cycle.succeeded" => vec![
                Notice::Subscription(stamped(
                    Remote::new(plan_id.clone())
                        .status(SubscriptionStatus::Active)
                        .trial_ends_at(None),
                )),
                payment(true),
            ],
            "recurring.cycle.retrying" | "recurring.cycle.failed" => vec![
                Notice::Subscription(stamped(
                    Remote::new(plan_id.clone()).status(SubscriptionStatus::PastDue),
                )),
                payment(false),
            ],
            "refund.succeeded" | "refund.failed" | "refund.pending" => refund_update(data)
                .map_or_else(Vec::new, |mut update| {
                    if let Some(at) = at {
                        update = update.at(at);
                    }
                    vec![Notice::Refund(update)]
                }),
            _ => Vec::new(),
        })
    }
}

/// The invoice a payment was made through.
fn invoice_of(payment: &BillingPayment) -> Result<&str> {
    payment.gateway_id.as_deref().ok_or_else(|| {
        renox::anyhow::anyhow!("the payment {} has no Xendit invoice", payment.id).into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interval;

    #[test]
    fn intervals_and_plan_statuses() {
        let plan = |interval| Plan::new("p", "P").price(1, "IDR", interval);
        assert_eq!(interval(&plan(Interval::Day)), "DAY");
        assert_eq!(interval(&plan(Interval::Week)), "WEEK");
        assert_eq!(interval(&plan(Interval::Month)), "MONTH");
        assert_eq!(interval(&plan(Interval::Year)), "YEAR");
        // A status Xendit may add later: not paid yet.
        let remote = plan_remote(&json!({ "id": "repl_1", "status": "REQUIRES_ACTION" }));
        assert_eq!(remote.status, Some(SubscriptionStatus::Incomplete));
        assert_eq!(remote.owner, None);
        assert_eq!(
            plan_remote(&json!({ "id": "repl_1", "status": "INACTIVE" })).status,
            Some(SubscriptionStatus::Canceled)
        );
    }

    #[test]
    fn keys_given_in_code_never_print() {
        let xendit = Xendit::new("xnd_secret_value", "callback-value");
        let shown = format!("{xendit:?}");
        assert!(
            !shown.contains("xnd_secret_value") && !shown.contains("callback-value"),
            "{shown}"
        );
        assert!(xendit.configured(&renox::Config::default()));
        assert!(!Xendit::new("", "").configured(&renox::Config::default()));
        assert!(!Xendit::from_config().configured(&renox::Config::default()));
    }
}
