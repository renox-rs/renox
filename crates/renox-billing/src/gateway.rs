//! What a payment gateway is: the calls the module makes (a customer, a
//! checkout, a plan change, a cancellation) and how its webhooks read.
//! Stripe and Xendit come with the crate; another gateway is one impl of
//! [`Gateway`]. Every answer is normalized: a subscription's state is a
//! [`Remote`], a webhook is a list of [`Notice`]s.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use renox::Config;
use renox::axum::http::HeaderMap;
use renox::prelude::*;

use crate::payment::{
    ChargeRequest, DisputeUpdate, PaymentPage, PaymentState, PaymentUpdate, Refund, RefundResult,
    RefundUpdate,
};
use crate::{BillingPayment, Owner, Plan, Subscription, SubscriptionStatus, Unsupported};

/// A boxed future that is `Send`, what [`Gateway`]'s methods return (so the
/// trait works behind `dyn`, and handlers stay `Send`).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A payment provider that sells subscriptions. Each call gets the app's
/// state (`state.http` for requests, `state.config` for keys) and answers
/// in the module's terms; the module stores what it answers and turns
/// webhooks into rows and events.
///
/// The webhook URL is `/billing/webhooks/{name}`: Renox checks it with
/// [`Gateway::verify_webhook`], stores it once per
/// [`Gateway::webhook_event_id`] (a provider's retry changes nothing), and a
/// queue worker applies what [`Gateway::parse_webhook`] reads from it.
/// `/billing/webhooks/{name}/{account}` is the same for a named account
/// ([`Gateway::has_account`], [`Gateway::verify_webhook_account`]).
pub trait Gateway: Send + Sync + 'static {
    /// The gateway's name in URLs and in the `subscriptions` and
    /// `billing_customers` tables: short, lower case, never changed.
    fn name(&self) -> &str;

    /// Its name for people: `Stripe`.
    fn label(&self) -> &str;

    /// Whether its keys are set (in code or the configuration). A gateway
    /// without them sells nothing and its webhook URL answers 404.
    fn configured(&self, config: &Config) -> bool;

    /// Whether [`Gateway::swap`] charges or credits the difference for the
    /// rest of the period (Stripe does).
    fn prorates(&self) -> bool {
        false
    }

    /// Whether a subscription canceled at the end of its period can be
    /// resumed before then ([`Gateway::resume`]).
    fn resumes(&self) -> bool {
        false
    }

    /// Whether the gateway can [pause](Gateway::pause) billing for a while.
    fn pauses(&self) -> bool {
        false
    }

    /// Makes a customer for `owner` and answers its id.
    fn create_customer<'a>(
        &'a self,
        state: &'a AppState,
        owner: &'a Owner,
    ) -> BoxFuture<'a, Result<String>>;

    /// Starts a subscription: usually a page at the provider where the
    /// customer pays, which sends them back to `request.success_url`.
    fn checkout<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a CheckoutRequest,
    ) -> BoxFuture<'a, Result<Checkout>>;

    /// Moves `subscription` to `plan` (an upgrade or a downgrade),
    /// prorated when `prorate` and the gateway [prorates](Gateway::prorates).
    fn swap<'a>(
        &'a self,
        state: &'a AppState,
        subscription: &'a Subscription,
        plan: &'a Plan,
        prorate: bool,
    ) -> BoxFuture<'a, Result<Remote>>;

    /// Cancels `subscription`: at the end of the period it's paid for
    /// (`at_period_end`, a grace period until then), or now.
    fn cancel<'a>(
        &'a self,
        state: &'a AppState,
        subscription: &'a Subscription,
        at_period_end: bool,
    ) -> BoxFuture<'a, Result<Remote>>;

    /// Takes back a cancellation during the grace period. The default
    /// refuses: override it with [`Gateway::resumes`].
    fn resume<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
    ) -> BoxFuture<'a, Result<Remote>> {
        let label = self.label().to_owned();
        Box::pin(async move {
            Err(Error::BadRequest(format!(
                "{label} can't resume a canceled subscription: subscribe again"
            )))
        })
    }

    /// Stops charging `subscription` until [`Gateway::unpause`] (or, when
    /// `resumes_at` is given, until then). Answer the subscription with
    /// [`SubscriptionStatus::Paused`]. The default refuses; override it
    /// together with [`Gateway::pauses`].
    fn pause<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
        _resumes_at: Option<DateTime>,
    ) -> BoxFuture<'a, Result<Remote>> {
        let label = self.label().to_owned();
        Box::pin(async move {
            Err(Error::BadRequest(format!(
                "{label} can't pause a subscription: cancel it instead"
            )))
        })
    }

    /// Takes billing up again after [`Gateway::pause`]. Answer the
    /// subscription with its status from before the pause (usually
    /// [`SubscriptionStatus::Active`]). The default refuses.
    fn unpause<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
    ) -> BoxFuture<'a, Result<Remote>> {
        let label = self.label().to_owned();
        Box::pin(async move {
            Err(Error::BadRequest(format!(
                "{label} can't pause a subscription, so there is nothing to take up again"
            )))
        })
    }

    /// Accepts a webhook only if it comes from the provider (a signature
    /// over `body`, a token in a header); an error answers 401.
    ///
    /// A gateway whose key lives in the app's settings rather than in
    /// [`Config`] overrides [`Gateway::verify_webhook_with`] instead.
    fn verify_webhook(&self, config: &Config, headers: &HeaderMap, body: &[u8]) -> Result;

    /// [`Gateway::verify_webhook`] with the whole app state: its database,
    /// its [provided](Registry::provide) settings, its HTTP client. This is
    /// what the webhook route calls; the default calls `verify_webhook`
    /// with `state.config`, so existing gateways keep working.
    fn verify_webhook_with(&self, state: &AppState, headers: &HeaderMap, body: &[u8]) -> Result {
        self.verify_webhook(&state.config, headers, body)
    }

    /// Whether the gateway has an account called `account`, so that
    /// `/billing/webhooks/{name}/{account}` is served (an unknown account
    /// answers 404). The default is `false`: a gateway that knows nothing of
    /// accounts only serves its default account's URL.
    fn has_account(&self, _state: &AppState, _account: &str) -> bool {
        false
    }

    /// [`Gateway::verify_webhook_with`] for the named account (`None` is
    /// the default account). This is what the webhook route calls; the
    /// default calls `verify_webhook_with`, so existing gateways keep
    /// working.
    fn verify_webhook_account(
        &self,
        state: &AppState,
        _account: Option<&str>,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result {
        self.verify_webhook_with(state, headers, body)
    }

    /// [`Gateway::parse_webhook_with`] for the named account (`None` is the
    /// default account). This is what the queue worker calls; the default
    /// calls `parse_webhook_with`.
    fn parse_webhook_account(
        &self,
        state: &AppState,
        _account: Option<&str>,
        body: &[u8],
    ) -> Result<Vec<Notice>> {
        self.parse_webhook_with(state, body)
    }

    /// The webhook's event id, so a provider's retry is processed once. The
    /// default: the `webhook-id` header, else the JSON body's top-level
    /// `id` (Stripe's `evt_…`), else a hash of the body.
    fn webhook_event_id(&self, headers: &HeaderMap, body: &[u8]) -> Result<String> {
        if let Some(id) = headers
            .get("webhook-id")
            .and_then(|v| v.to_str().ok())
            .filter(|id| !id.trim().is_empty())
        {
            return Ok(id.trim().to_owned());
        }
        let json: renox::serde_json::Value = renox::serde_json::from_slice(body)
            .map_err(|err| Error::BadRequest(format!("the webhook isn't JSON: {err}")))?;
        Ok(match json.get("id").and_then(|id| id.as_str()) {
            Some(id) if !id.is_empty() => id.to_owned(),
            _ => format!("sha256:{}", renox::webhook::sha256_hex(body)),
        })
    }

    /// What a verified webhook says, in the module's terms (often nothing:
    /// providers send many events a subscription doesn't need).
    fn parse_webhook(&self, body: &[u8]) -> Result<Vec<Notice>>;

    /// [`Gateway::parse_webhook`] with the app state, for a gateway that
    /// needs its settings to read an event. This is what the queue worker
    /// calls; the default calls `parse_webhook`.
    fn parse_webhook_with(&self, _state: &AppState, body: &[u8]) -> Result<Vec<Notice>> {
        self.parse_webhook(body)
    }

    /// Starts a one-off charge: a page at the provider where the customer
    /// pays `request.amount`. `request.account` names the account to use
    /// (`None` is the default). The default answers [`Unsupported`].
    fn charge<'a>(
        &'a self,
        _state: &'a AppState,
        _request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        unsupported(self.name(), "charge")
    }

    /// Makes a payment link for `request`. The default answers
    /// [`Unsupported`].
    fn payment_link<'a>(
        &'a self,
        _state: &'a AppState,
        _request: &'a ChargeRequest,
    ) -> BoxFuture<'a, Result<PaymentPage>> {
        unsupported(self.name(), "payment_link")
    }

    /// Refunds part or all of `payment`, with the keys of the account the
    /// payment was made through (`payment.account`). Answer a pending
    /// refund unless the gateway refunded at once. The default answers
    /// [`Unsupported`].
    fn refund<'a>(
        &'a self,
        _state: &'a AppState,
        _payment: &'a BillingPayment,
        _refund: &'a Refund,
    ) -> BoxFuture<'a, Result<RefundResult>> {
        unsupported(self.name(), "refund")
    }

    /// Asks the gateway where `payment` stands now, with the payment's
    /// account. The default answers [`Unsupported`].
    fn payment_status<'a>(
        &'a self,
        _state: &'a AppState,
        _payment: &'a BillingPayment,
    ) -> BoxFuture<'a, Result<PaymentState>> {
        unsupported(self.name(), "payment_status")
    }

    /// Accepts the dispute on `payment` (gives up contesting it). The
    /// default answers [`Unsupported`].
    fn accept_dispute<'a>(
        &'a self,
        _state: &'a AppState,
        _payment: &'a BillingPayment,
    ) -> BoxFuture<'a, Result> {
        unsupported(self.name(), "accept_dispute")
    }
}

/// A future that answers [`Unsupported`] for `operation` of `gateway`.
pub(crate) fn unsupported<'a, T: Send + 'a>(
    gateway: &str,
    operation: &str,
) -> BoxFuture<'a, Result<T>> {
    let refusal = Unsupported::new(gateway, operation);
    Box::pin(async move { Err(refusal.into()) })
}

/// What [`Gateway::checkout`] gets: who subscribes, to what, and where the
/// customer goes afterwards.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CheckoutRequest {
    /// Who subscribes.
    pub owner: Owner,
    /// Their customer id at the gateway.
    pub customer_id: String,
    /// Which of the owner's subscriptions (`default`).
    pub name: String,
    /// The plan.
    pub plan: Plan,
    /// The plan's price id at this gateway, when it has one.
    pub price_id: Option<String>,
    /// The end of a free trial, if one applies (no charge before it).
    pub trial_ends_at: Option<DateTime>,
    /// Where the provider sends the customer after paying (absolute).
    pub success_url: String,
    /// Where it sends them when they give up (absolute).
    pub cancel_url: String,
}

impl CheckoutRequest {
    /// What the gateway stores with the subscription so its webhooks find
    /// the owner, the name and the plan again: `renox_billable` (the owner's
    /// key, `user:5`), `renox_name` and `renox_plan`.
    pub fn metadata(&self) -> Vec<(&'static str, String)> {
        vec![
            ("renox_billable", self.owner.key()),
            ("renox_name", self.name.clone()),
            ("renox_plan", self.plan.key.clone()),
        ]
    }
}

/// What [`Gateway::checkout`] answers: the page where the customer pays.
/// The subscription itself arrives by webhook, carrying the request's
/// [metadata](CheckoutRequest::metadata).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Checkout {
    /// The page to send the customer to.
    pub url: String,
}

impl Checkout {
    /// Sends the customer to `url`.
    pub fn redirect(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

/// A subscription as a gateway describes it, from a call or a webhook.
/// Only what is set changes the stored row: a webhook that only knows the
/// new period end sets only that.
///
/// ```
/// use renox_billing::{Remote, SubscriptionStatus};
///
/// let remote = Remote::new("sub_123")
///     .status(SubscriptionStatus::Active)
///     .plan("pro")
///     .ends_at(None) // not canceled
///     .at(1_790_000_000); // the event's time
/// assert_eq!(remote.id(), "sub_123");
/// ```
#[derive(Debug, Clone, Default)]
pub struct Remote {
    pub(crate) id: String,
    pub(crate) customer_id: Option<String>,
    pub(crate) status: Option<SubscriptionStatus>,
    pub(crate) plan: Option<String>,
    pub(crate) price_id: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) trial_ends_at: Option<Option<DateTime>>,
    pub(crate) current_period_end: Option<Option<DateTime>>,
    pub(crate) ends_at: Option<Option<DateTime>>,
    pub(crate) at: Option<i64>,
}

impl Remote {
    /// The subscription with this id at the gateway.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }

    /// Its id at the gateway.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The customer's id at the gateway (finds the owner when the
    /// subscription carries no `renox_billable`).
    pub fn customer(mut self, id: impl Into<String>) -> Self {
        self.customer_id = Some(id.into());
        self
    }

    /// Its status. A canceled one without [`Remote::ends_at`] ends now,
    /// unless the row already has an end (a grace period).
    pub fn status(mut self, status: SubscriptionStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// The plan's key (from the metadata the module sent).
    pub fn plan(mut self, key: impl Into<String>) -> Self {
        self.plan = Some(key.into());
        self
    }

    /// The gateway's price id; it names the plan when a plan has that
    /// price id ([`Plan::price_id_for`]), before [`Remote::plan`].
    pub fn price_id(mut self, id: impl Into<String>) -> Self {
        self.price_id = Some(id.into());
        self
    }

    /// The owner's key (`user:5`, from `renox_billable`).
    pub fn owner(mut self, key: impl Into<String>) -> Self {
        self.owner = Some(key.into());
        self
    }

    /// Which of the owner's subscriptions (`renox_name`).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// When the free trial ends (`None`: no trial).
    pub fn trial_ends_at(mut self, at: Option<DateTime>) -> Self {
        self.trial_ends_at = Some(at);
        self
    }

    /// When the period paid for ends (the next charge).
    pub fn current_period_end(mut self, at: Option<DateTime>) -> Self {
        self.current_period_end = Some(at);
        self
    }

    /// When access ends after a cancellation (`None`: not canceled).
    pub fn ends_at(mut self, at: Option<DateTime>) -> Self {
        self.ends_at = Some(at);
        self
    }

    /// When the gateway said this (unix seconds). The module ignores what
    /// is older than the newest thing it applied to the row, so events
    /// that arrive out of order don't undo newer ones.
    pub fn at(mut self, unix_seconds: i64) -> Self {
        self.at = Some(unix_seconds);
        self
    }
}

/// A payment a gateway reports by webhook.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Payment {
    /// The payment's id at the gateway (Stripe's invoice, Xendit's cycle).
    pub id: String,
    /// The subscription it's for, at the gateway.
    pub subscription_id: Option<String>,
    /// The customer, at the gateway.
    pub customer_id: Option<String>,
    /// How much, in the currency's smallest unit.
    pub amount: i64,
    /// The ISO 4217 code, upper case.
    pub currency: String,
    /// Paid, or failed.
    pub succeeded: bool,
}

impl Payment {
    /// A payment of `amount` in `currency` that `succeeded` or failed.
    pub fn new(id: impl Into<String>, amount: i64, currency: &str, succeeded: bool) -> Self {
        Self {
            id: id.into(),
            subscription_id: None,
            customer_id: None,
            amount,
            currency: currency.to_ascii_uppercase(),
            succeeded,
        }
    }

    /// For this subscription at the gateway.
    pub fn subscription(mut self, id: impl Into<String>) -> Self {
        self.subscription_id = Some(id.into());
        self
    }

    /// By this customer at the gateway.
    pub fn customer(mut self, id: impl Into<String>) -> Self {
        self.customer_id = Some(id.into());
        self
    }
}

/// One thing a webhook says.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Notice {
    /// A subscription changed (or was made).
    Subscription(Remote),
    /// A payment succeeded or failed.
    Payment(Payment),
    /// A one-off payment (a charge or a link) changed.
    OneOff(PaymentUpdate),
    /// A refund changed.
    Refund(RefundUpdate),
    /// A dispute was opened or decided.
    Dispute(DisputeUpdate),
}

/// The account name a resolver gets for the gateway's default account.
pub(crate) const DEFAULT_ACCOUNT: &str = "default";

/// An account name as it appears in a configuration name (`acme-llc` is
/// `ACME_LLC`); `None` for a name that isn't 1 to 64 letters, digits, `-` or
/// `_`, which no account can have.
pub(crate) fn account_suffix(account: &str) -> Option<String> {
    let valid = !account.is_empty()
        && account.len() <= 64
        && account
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| account.to_ascii_uppercase().replace('-', "_"))
}

/// A key looked up by the app on every use: gets the state and the account
/// name, answers the key (`None` when the installation has none).
pub(crate) type Resolver =
    Arc<dyn Fn(&AppState, &str) -> BoxFuture<'static, Result<Option<String>>> + Send + Sync>;

/// A key given in code, read from the configuration when a call needs it
/// (`config.var`: `.env` or the environment), or looked up by a resolver
/// on every call (never cached).
#[derive(Clone)]
pub(crate) enum Secret {
    Given(String),
    Config(&'static str),
    Resolved(Resolver),
}

impl Secret {
    /// A secret from `resolver`, which ignores the account.
    pub(crate) fn resolved<F, Fut>(resolver: F) -> Self
    where
        F: Fn(&AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        Self::Resolved(Arc::new(move |state, _account| Box::pin(resolver(state))))
    }

    /// A secret from `resolver`, which gets the account's name.
    pub(crate) fn resolved_accounts<F, Fut>(resolver: F) -> Self
    where
        F: Fn(&AppState, &str) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Option<String>>> + Send + 'static,
    {
        Self::Resolved(Arc::new(move |state, account| {
            Box::pin(resolver(state, account))
        }))
    }

    /// The key a synchronous check can see: a resolved key is not known
    /// without awaiting it, so this is `None` for one.
    pub(crate) fn resolve(&self, config: &Config) -> Option<String> {
        match self {
            Self::Given(value) => Some(value.clone()).filter(|v| !v.is_empty()),
            Self::Config(name) => config.var(name),
            Self::Resolved(_) => None,
        }
    }

    /// The webhook secret of `account` (`None`: the default account's, as
    /// [`Secret::resolve`]). An account's secret is read from the same
    /// configuration name with the account appended, upper case with `-` as
    /// `_` (`STRIPE_WEBHOOK_SECRET_ACME_LLC` for `acme-llc`). A secret given
    /// in code belongs to the default account only.
    pub(crate) fn resolve_account(&self, config: &Config, account: Option<&str>) -> Option<String> {
        let Some(account) = account else {
            return self.resolve(config);
        };
        match self {
            Self::Config(name) => config
                .var(&format!("{name}_{}", account_suffix(account)?))
                .filter(|v| !v.is_empty()),
            _ => None,
        }
    }

    /// Whether the key may be there: a resolved one is asked at use.
    pub(crate) fn is_set(&self, config: &Config) -> bool {
        matches!(self, Self::Resolved(_)) || self.resolve(config).is_some()
    }

    /// The key, or an error naming where to set it.
    pub(crate) fn require(&self, config: &Config, what: &str) -> Result<String> {
        self.resolve(config).ok_or_else(|| self.missing(what))
    }

    /// The key, asking the resolver when there is one.
    pub(crate) async fn require_async(&self, state: &AppState, what: &str) -> Result<String> {
        match self {
            Self::Resolved(resolver) => resolver(state, DEFAULT_ACCOUNT)
                .await?
                .filter(|key| !key.is_empty())
                .ok_or_else(|| self.missing(what)),
            _ => self.require(&state.config, what),
        }
    }

    /// The key of `account` (`None`: the default account's, as
    /// [`Secret::require_async`]). A resolver gets the account's name; a
    /// configured key is read with the account appended (`STRIPE_SECRET_ACME_LLC`);
    /// a key given in code belongs to the default account only.
    pub(crate) async fn require_account(
        &self,
        state: &AppState,
        account: Option<&str>,
        what: &str,
    ) -> Result<String> {
        let Some(name) = account else {
            return self.require_async(state, what).await;
        };
        match self {
            Self::Resolved(resolver) => resolver(state, name)
                .await?
                .filter(|key| !key.is_empty())
                .ok_or_else(|| self.missing(what)),
            _ => self
                .resolve_account(&state.config, account)
                .ok_or_else(|| self.missing(what)),
        }
    }

    fn missing(&self, what: &str) -> Error {
        let hint = match self {
            Self::Config(name) => format!("set {name} in .env"),
            Self::Given(_) => "it was given empty".into(),
            Self::Resolved(_) => "the key resolver answered none".into(),
        };
        renox::anyhow::anyhow!("{what} is missing: {hint}").into()
    }
}

impl fmt::Debug for Secret {
    /// Never prints the key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Given(_) | Self::Resolved(_) => f.write_str("(hidden)"),
            Self::Config(name) => write!(f, "from {name}"),
        }
    }
}

/// Unix seconds as a time.
pub(crate) fn from_unix(seconds: i64) -> Option<DateTime> {
    renox::chrono::DateTime::from_timestamp(seconds, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A missing key says where to set it, or that it was given empty.
    #[test]
    fn missing_keys_say_where_to_set_them() {
        let config = Config::default();
        let err = Secret::Config("STRIPE_SECRET")
            .require(&config, "Stripe's secret key")
            .unwrap_err();
        assert!(
            format!("{err:?}")
                .contains("Stripe's secret key is missing: set STRIPE_SECRET in .env"),
            "{err:?}"
        );
        let err = Secret::Given(String::new())
            .require(&config, "Xendit's secret key")
            .unwrap_err();
        assert!(format!("{err:?}").contains("it was given empty"), "{err:?}");
        let mut config = Config::default();
        config.vars.insert("STRIPE_SECRET".into(), "sk_1".into());
        assert_eq!(
            Secret::Config("STRIPE_SECRET")
                .require(&config, "x")
                .unwrap(),
            "sk_1"
        );
        assert_eq!(format!("{:?}", Secret::Given("sk".into())), "(hidden)");
        let resolved = Secret::resolved(|_| async { Ok(Some("sk_2".to_string())) });
        assert_eq!(format!("{resolved:?}"), "(hidden)");
        assert!(resolved.is_set(&Config::default()));
        assert!(!Secret::Config("X").is_set(&Config::default()));
    }

    #[test]
    fn payments_keep_their_id_and_an_upper_case_currency() {
        let payment = Payment::new("in_1", 900, "usd", false)
            .subscription("sub_1")
            .customer("cus_1");
        assert_eq!(payment.id, "in_1");
        assert_eq!(payment.currency, "USD");
        assert_eq!(payment.subscription_id.as_deref(), Some("sub_1"));
        assert_eq!(payment.customer_id.as_deref(), Some("cus_1"));
        assert!(!payment.succeeded);
    }
}
