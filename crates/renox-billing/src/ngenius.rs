//! N-Genius (Network International): hosted-payment-page orders. Every call
//! first trades the API key for a short-lived access token (kept in memory
//! per account until it expires), then uses it as a bearer token. One-off
//! payments are orders (`/transactions/outlets/{outlet}/orders`, a charge
//! and a link alike), read back with the order's status and refunded
//! through the refund link of the captured payment. Webhooks are checked
//! against a shared secret sent in the `x-webhook-secret` header (set it as
//! the webhook's custom header in the N-Genius portal). N-Genius has no
//! subscriptions here and no disputes API, so those operations answer
//! [`Unsupported`](crate::Unsupported).

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use renox::Config;
use renox::axum::http::HeaderMap;
use renox::http::Request;
use renox::prelude::*;
use renox::serde_json::{Value, json};

use crate::gateway::{Secret, unsupported};
use crate::payment::{
    BillingPayment, ChargeRequest, PaymentPage, PaymentState, PaymentStatus, PaymentUpdate, Refund,
    RefundResult, RefundStatus, RefundUpdate,
};
use crate::{
    BoxFuture, Checkout, CheckoutRequest, Gateway, Notice, Owner, Plan, Remote, Subscription,
};

const API: &str = "https://api-gateway.ngenius-payments.com";
const SANDBOX_API: &str = "https://api-gateway.sandbox.ngenius-payments.com";
/// How long a call to N-Genius may take.
const TIMEOUT: Duration = Duration::from_secs(15);
/// A token is renewed this long before it expires, in seconds.
const TOKEN_MARGIN: i64 = 10;
const IDENTITY_TYPE: &str = "application/vnd.ni-identity.v1+json";
const PAYMENT_TYPE: &str = "application/vnd.ni-payment.v2+json";

/// N-Genius, with its API key, the outlet reference orders are made in and
/// the shared secret the webhook sends.
///
/// ```
/// use renox_billing::{Billing, Ngenius};
///
/// # let _ =
/// Billing::new().ngenius(); // NGENIUS_API_KEY, NGENIUS_OUTLET_REF and NGENIUS_WEBHOOK_SECRET from .env
/// # let _ =
/// Billing::new().gateway(Ngenius::new("api-key", "outlet-ref", "webhook-secret")); // or in code
/// ```
///
/// Amounts are sent in minor units with the order's currency. A payment
/// link is an order too; N-Genius pages don't expire on their own. Only
/// one-off payments are supported: subscriptions and disputes answer
/// [`Unsupported`](crate::Unsupported). The webhook URL is
/// `/billing/webhooks/ngenius`.
#[derive(Debug, Clone)]
pub struct Ngenius {
    api_key: Secret,
    outlet: Secret,
    webhook_secret: Secret,
    base: &'static str,
    tokens: Tokens,
}

/// Access tokens by account, with the key they were made from (so a
/// changed key is not served a token of the old one) and when they end.
/// Never printed.
#[derive(Clone, Default)]
struct Tokens(Arc<Mutex<HashMap<String, Kept>>>);

/// A token kept: the fingerprint of its key, the token, its end.
type Kept = (String, String, DateTime);

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("(hidden)")
    }
}

impl Tokens {
    fn get(&self, account: &str, fingerprint: &str) -> Option<String> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(account)
            .filter(|(print, _, until)| print == fingerprint && *until > renox::db::now())
            .map(|(_, token, _)| token.clone())
    }

    fn put(&self, account: &str, fingerprint: String, token: String, until: DateTime) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(account.to_owned(), (fingerprint, token, until));
    }
}

impl Ngenius {
    /// N-Genius with these keys.
    pub fn new(
        api_key: impl Into<String>,
        outlet_ref: impl Into<String>,
        webhook_secret: impl Into<String>,
    ) -> Self {
        Self::with(
            Secret::Given(api_key.into()),
            Secret::Given(outlet_ref.into()),
            Secret::Given(webhook_secret.into()),
        )
    }

    /// `NGENIUS_API_KEY`, `NGENIUS_OUTLET_REF` and `NGENIUS_WEBHOOK_SECRET`
    /// from the configuration, read when a call needs them.
    pub fn from_config() -> Self {
        Self::with(
            Secret::Config("NGENIUS_API_KEY"),
            Secret::Config("NGENIUS_OUTLET_REF"),
            Secret::Config("NGENIUS_WEBHOOK_SECRET"),
        )
    }

    /// N-Genius whose API key is looked up by `resolver` on every call, so
    /// a key changed in the app's store is used by the next call without a
    /// restart (the access token is kept only for the key it was made
    /// from). The closure gets the app state and answers `Ok(None)` when
    /// the installation has no key yet; the future it returns is `Send`.
    /// The outlet and the webhook secret stay `NGENIUS_OUTLET_REF` and
    /// `NGENIUS_WEBHOOK_SECRET`.
    ///
    /// ```
    /// use renox_billing::Ngenius;
    ///
    /// # let _ =
    /// Ngenius::resolved(|_state| async move { Ok(Some("api-key".to_string())) });
    /// ```
    pub fn resolved<F, Fut>(resolver: F) -> Self
    where
        F: Fn(&AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        Self::with(
            Secret::resolved(resolver),
            Secret::Config("NGENIUS_OUTLET_REF"),
            Secret::Config("NGENIUS_WEBHOOK_SECRET"),
        )
    }

    /// [`Ngenius::resolved`] for several accounts: the closure also gets the
    /// account's name (`"default"` for the gateway's default account).
    ///
    /// ```
    /// use renox_billing::Ngenius;
    ///
    /// # let _ =
    /// Ngenius::resolved_accounts(|_state, account: &str| {
    ///     let key = format!("api-key-{account}");
    ///     async move { Ok(Some(key)) }
    /// });
    /// ```
    pub fn resolved_accounts<F, Fut>(resolver: F) -> Self
    where
        F: Fn(&AppState, &str) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        Self::with(
            Secret::resolved_accounts(resolver),
            Secret::Config("NGENIUS_OUTLET_REF"),
            Secret::Config("NGENIUS_WEBHOOK_SECRET"),
        )
    }

    /// Uses N-Genius' sandbox instead of the live API.
    pub fn sandbox(mut self) -> Self {
        self.base = SANDBOX_API;
        self
    }

    fn with(api_key: Secret, outlet: Secret, webhook_secret: Secret) -> Self {
        Self {
            api_key,
            outlet,
            webhook_secret,
            base: API,
            tokens: Tokens::default(),
        }
    }

    /// The access token of `account` (`None`: default): the kept one while
    /// it is valid, else a new one traded for the API key.
    async fn token(&self, state: &AppState, account: Option<&str>) -> Result<String> {
        let key = self
            .api_key
            .require_account(state, account, "N-Genius's API key")
            .await?;
        let name = account.unwrap_or("default");
        let fingerprint = renox::webhook::sha256_hex(key.as_bytes());
        if let Some(token) = self.tokens.get(name, &fingerprint) {
            return Ok(token);
        }
        let request = state
            .http
            .post(format!("{}/identity/auth/access-token", self.base))
            .header("authorization", format!("Basic {key}"))
            .header("accept", IDENTITY_TYPE)
            .timeout(TIMEOUT);
        let answer = send(request.body(IDENTITY_TYPE, b"{}".to_vec())).await?;
        let token = text(&answer["access_token"]).ok_or_else(|| {
            renox::anyhow::anyhow!("N-Genius answered an access token request without a token")
        })?;
        let lasts = answer["expires_in"].as_i64().unwrap_or(0) - TOKEN_MARGIN;
        let until = renox::db::now() + renox::chrono::Duration::seconds(lasts.max(0));
        self.tokens.put(name, fingerprint, token.clone(), until);
        Ok(token)
    }

    /// A request to the transactions API, `path` under the account's outlet
    /// (or an absolute URL N-Genius gave), with the bearer token.
    async fn call(
        &self,
        state: &AppState,
        account: Option<&str>,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        let token = self.token(state, account).await?;
        let url = if path.starts_with("https://") {
            path.to_owned()
        } else {
            let outlet = self
                .outlet
                .require_account(state, account, "N-Genius's outlet reference")
                .await?;
            format!("{}/transactions/outlets/{outlet}{path}", self.base)
        };
        let request = match method {
            "GET" => state.http.get(url),
            _ => state.http.post(url),
        };
        let mut request = request
            .bearer(&token)
            .header("accept", PAYMENT_TYPE)
            .timeout(TIMEOUT);
        if let Some(body) = body {
            request = request.body(PAYMENT_TYPE, renox::serde_json::to_vec(body)?);
        }
        send(request).await
    }

    /// Makes an order (N-Genius' hosted payment page) for `request`: both a
    /// charge and a payment link are one.
    async fn order(&self, state: &AppState, request: &ChargeRequest) -> Result<PaymentPage> {
        let reference = match (&request.reference_type, &request.reference_id) {
            (Some(kind), Some(id)) => format!("{kind}-{id}-{}", request.payment_id),
            (None, Some(id)) => format!("{id}-{}", request.payment_id),
            _ => format!("renox-payment-{}", request.payment_id),
        };
        let mut body = json!({
            "action": "SALE",
            "amount": {
                "currencyCode": request.amount.currency.code(),
                "value": request.amount.minor,
            },
            "merchantOrderReference": reference,
        });
        let mut attributes = json!({ "skipConfirmationPage": true });
        if let Some(url) = &request.success_url {
            attributes["redirectUrl"] = json!(url);
        }
        if let Some(url) = &request.cancel_url {
            attributes["cancelUrl"] = json!(url);
        }
        body["merchantAttributes"] = attributes;
        if let Some(email) = request.owner.as_ref().and_then(|o| o.email.clone()) {
            body["emailAddress"] = json!(email);
        }
        let created = self
            .call(
                state,
                request.account.as_deref(),
                "POST",
                "/orders",
                Some(&body),
            )
            .await?;
        let id = text(&created["reference"])
            .ok_or_else(|| renox::anyhow::anyhow!("N-Genius made an order without a reference"))?;
        let url = text(&created["_links"]["payment"]["href"]).ok_or_else(|| {
            renox::anyhow::anyhow!("N-Genius made an order without a payment page")
        })?;
        Ok(PaymentPage::new(id, url))
    }

    /// Checks the shared secret against the account's (the default
    /// account's when `account` is `None`); an account never falls back to
    /// the default account's secret.
    fn verify_account(
        &self,
        config: &Config,
        account: Option<&str>,
        headers: &HeaderMap,
    ) -> Result {
        let secret = match account {
            None => self
                .webhook_secret
                .require(config, "N-Genius's webhook secret")?,
            Some(_) => self
                .webhook_secret
                .resolve_account(config, account)
                .ok_or(Error::Unauthorized)?,
        };
        let sent = headers
            .get("x-webhook-secret")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        renox::webhook::ensure(renox::webhook::same(sent, &secret))
    }
}

/// Sends `request`; N-Genius' error message on a 4xx/5xx (never the
/// request, which carries the credentials).
async fn send(request: Request) -> Result<Value> {
    let response = request.send().await?;
    let body: Value = response.json().unwrap_or_default();
    if !response.ok() {
        let message = body["errors"][0]["message"]
            .as_str()
            .or_else(|| body["message"].as_str())
            .unwrap_or("no message")
            .to_owned();
        return Err(
            renox::anyhow::anyhow!("N-Genius answered {}: {message}", response.status()).into(),
        );
    }
    Ok(body)
}

fn text(value: &Value) -> Option<String> {
    value.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The id of a refund object: its `reference`, else the last part of its
/// `_id` (`urn:payment:…:refund:<id>`).
fn refund_id(refund: &Value) -> Option<String> {
    text(&refund["reference"])
        .or_else(|| text(&refund["_id"]).and_then(|id| id.rsplit(':').next().map(str::to_owned)))
}

/// A refund object as an update; `None` for one without an id.
fn refund_update(refund: &Value, order: &str) -> Option<RefundUpdate> {
    let status = match refund["state"].as_str() {
        Some("SUCCESS") => RefundStatus::Succeeded,
        Some("FAILED") => RefundStatus::Failed,
        _ => RefundStatus::Pending,
    };
    let mut update = RefundUpdate::new(refund_id(refund)?, status).payment(order);
    if let Some(minor) = refund["amount"]["value"].as_i64() {
        update = update.amount(minor);
    }
    Some(update)
}

/// The payment of an order (its first).
fn payment_of(order: &Value) -> &Value {
    &order["_embedded"]["payment"][0]
}

/// What an N-Genius payment state means for the payment.
fn status_of(state: Option<&str>) -> PaymentStatus {
    match state {
        Some("PURCHASED" | "CAPTURED") => PaymentStatus::Succeeded,
        Some("FAILED" | "DECLINED" | "AUTH_FAILED") => PaymentStatus::Failed,
        _ => PaymentStatus::Pending,
    }
}

/// The order a payment was made through.
fn order_of(payment: &BillingPayment) -> Result<&str> {
    payment.gateway_id.as_deref().ok_or_else(|| {
        renox::anyhow::anyhow!("the payment {} has no N-Genius order", payment.id).into()
    })
}

impl Gateway for Ngenius {
    fn name(&self) -> &str {
        "ngenius"
    }

    fn label(&self) -> &str {
        "N-Genius"
    }

    fn configured(&self, config: &Config) -> bool {
        self.api_key.is_set(config)
    }

    fn create_customer<'a>(
        &'a self,
        _state: &'a AppState,
        _owner: &'a Owner,
    ) -> BoxFuture<'a, Result<String>> {
        unsupported(self.name(), "create_customer")
    }

    fn checkout<'a>(
        &'a self,
        _state: &'a AppState,
        _request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<Checkout>> {
        unsupported(self.name(), "checkout")
    }

    fn swap<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
        _plan: &'a Plan,
        _prorate: bool,
    ) -> BoxFuture<'a, Result<Remote>> {
        unsupported(self.name(), "swap")
    }

    fn cancel<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
        _at_period_end: bool,
    ) -> BoxFuture<'a, Result<Remote>> {
        unsupported(self.name(), "cancel")
    }

    fn charge<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        Box::pin(self.order(state, request))
    }

    fn payment_link<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        Box::pin(self.order(state, request))
    }

    fn refund<'a>(
        &'a self,
        state: &'a AppState,
        payment: &'a BillingPayment,
        refund: &'a Refund,
    ) -> BoxFuture<'a, Result<RefundResult>> {
        Box::pin(async move {
            let order = order_of(payment)?;
            let account = payment.account.as_deref();
            let found = self
                .call(state, account, "GET", &format!("/orders/{order}"), None)
                .await?;
            // The refund link hangs off the capture of the payment.
            let capture = &payment_of(&found)["_embedded"]["cnp:capture"][0];
            let link = text(&capture["_links"]["cnp:refund"]["href"])
                .or_else(|| text(&payment_of(&found)["_links"]["cnp:refund"]["href"]))
                .ok_or_else(|| {
                    renox::anyhow::anyhow!("N-Genius has no captured payment to refund")
                })?;
            let body = json!({
                "amount": {
                    "currencyCode": refund.amount.currency.code(),
                    "value": refund.amount.minor,
                },
            });
            let made = self
                .call(state, account, "POST", &link, Some(&body))
                .await?;
            let id = refund_id(&made)
                .ok_or_else(|| renox::anyhow::anyhow!("N-Genius made a refund without an id"))?;
            match made["state"].as_str() {
                Some("FAILED") => Err(renox::anyhow::anyhow!("N-Genius could not refund").into()),
                Some("SUCCESS") => Ok(RefundResult::succeeded(id)),
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
            let order = order_of(payment)?;
            let found = self
                .call(
                    state,
                    payment.account.as_deref(),
                    "GET",
                    &format!("/orders/{order}"),
                    None,
                )
                .await?;
            let paid = payment_of(&found);
            let mut reported = PaymentState::new(status_of(paid["state"].as_str()));
            let refunds = paid["_embedded"]["cnp:refund"].as_array();
            for refund in refunds.into_iter().flatten() {
                if let Some(update) = refund_update(refund, order) {
                    reported = reported.refund(update);
                }
            }
            Ok(reported)
        })
    }

    fn verify_webhook(&self, config: &Config, headers: &HeaderMap, _body: &[u8]) -> Result {
        self.verify_account(config, None, headers)
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
        _body: &[u8],
    ) -> Result {
        self.verify_account(&state.config, account, headers)
    }

    fn webhook_event_id(&self, _headers: &HeaderMap, body: &[u8]) -> Result<String> {
        let event: Value = renox::serde_json::from_slice(body)
            .map_err(|err| Error::BadRequest(format!("the webhook isn't JSON: {err}")))?;
        Ok(match text(&event["eventId"]) {
            Some(id) => id,
            None => format!("sha256:{}", renox::webhook::sha256_hex(body)),
        })
    }

    fn parse_webhook(&self, body: &[u8]) -> Result<Vec<Notice>> {
        let event: Value = renox::serde_json::from_slice(body)?;
        let order = &event["order"];
        let Some(reference) = text(&order["reference"]) else {
            return Ok(Vec::new());
        };
        Ok(match event["eventName"].as_str().unwrap_or_default() {
            "PURCHASED" | "CAPTURED" => vec![Notice::OneOff(PaymentUpdate::new(
                reference,
                PaymentStatus::Succeeded,
            ))],
            "FAILED" | "DECLINED" | "AUTH_FAILED" => vec![Notice::OneOff(PaymentUpdate::new(
                reference,
                PaymentStatus::Failed,
            ))],
            name @ ("REFUNDED" | "PARTIALLY_REFUNDED" | "REFUND_FAILED") => {
                let refunds = payment_of(order)["_embedded"]["cnp:refund"].as_array();
                let last = refunds.and_then(|list| list.last());
                match last.and_then(|refund| refund_update(refund, &reference)) {
                    Some(mut update) => {
                        if name == "REFUND_FAILED" {
                            update.status = RefundStatus::Failed;
                        }
                        vec![Notice::Refund(update)]
                    }
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_given_in_code_never_print() {
        let gateway = Ngenius::new("api_key_value", "outlet_value", "secret_value");
        let shown = format!("{gateway:?}");
        for hidden in ["api_key_value", "secret_value"] {
            assert!(!shown.contains(hidden), "{shown}");
        }
        assert!(gateway.configured(&renox::Config::default()));
        assert!(!Ngenius::new("", "", "").configured(&renox::Config::default()));
        assert!(!Ngenius::from_config().configured(&renox::Config::default()));
    }

    #[test]
    fn payment_states_map_to_statuses() {
        assert_eq!(status_of(Some("PURCHASED")), PaymentStatus::Succeeded);
        assert_eq!(status_of(Some("DECLINED")), PaymentStatus::Failed);
        assert_eq!(status_of(Some("AWAITING_3DS")), PaymentStatus::Pending);
        assert_eq!(status_of(None), PaymentStatus::Pending);
    }
}
