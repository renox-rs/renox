//! One-off payments: the `billing_payments` and `billing_refunds` tables,
//! their status types and the [`Unsupported`] error a gateway answers with
//! for an operation it cannot do.

use std::fmt;

use renox::money::Money;
use renox::prelude::*;
use serde::Serialize;

use crate::{Billing, Owner};

/// What a payment is (`billing_payments.kind`).
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PaymentKind {
    /// A charge the app made for a customer.
    #[default]
    Charge,
    /// A payment link anyone with the URL can pay.
    Link,
}

/// Where a payment is, as the gateway last said (`billing_payments.status`).
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PaymentStatus {
    /// Waiting for the customer to pay.
    #[default]
    Pending,
    /// Paid.
    Succeeded,
    /// The payment was refused or abandoned.
    Failed,
    /// The link or session ran out before it was paid.
    Expired,
    /// Refunded in full.
    Refunded,
    /// Refunded in part.
    PartiallyRefunded,
    /// The customer disputed it (a chargeback is open).
    Disputed,
}

/// Where a refund is (`billing_refunds.status`).
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RefundStatus {
    /// Asked for; the gateway has not finished it.
    #[default]
    Pending,
    /// The money went back.
    Succeeded,
    /// The gateway could not refund it.
    Failed,
}

/// A one-off payment: a charge or a payment link, with the app's reference
/// to what it is for. Not to be confused with [`crate::Payment`], the
/// gateway's notice that an invoice was paid.
#[derive(Model, Serialize, Debug, Clone)]
#[model(table = "billing_payments")]
pub struct BillingPayment {
    /// The row's id.
    pub id: i64,
    /// What kind of owner paid; `None` for a link without an owner.
    pub billable_type: Option<String>,
    /// The owner's id.
    pub billable_id: Option<i64>,
    /// The kind of the app's record this pays for (`job`, `order`).
    pub reference_type: Option<String>,
    /// That record's id.
    pub reference_id: Option<String>,
    /// What is charged.
    #[model(money(currency = "currency"))]
    pub amount: Money,
    /// The gateway's name.
    pub gateway: String,
    /// The gateway's account; `None` is its default account.
    pub account: Option<String>,
    /// The payment's id at the gateway; `None` until it made one.
    pub gateway_id: Option<String>,
    /// Charge or link.
    pub kind: PaymentKind,
    /// As the gateway last said.
    pub status: PaymentStatus,
    /// The sum of the refunds that succeeded, in the payment's currency.
    pub refunded_amount: i64,
    /// The payment page, for a link or a hosted charge.
    pub url: Option<String>,
    /// When the link or session stops working.
    pub expires_at: Option<DateTime>,
    /// When the evidence for an open dispute is due.
    pub evidence_due_by: Option<DateTime>,
    /// The newest gateway event applied (unix seconds).
    #[serde(skip)]
    pub synced_at: Option<i64>,
    /// When it was made.
    pub created_at: Option<DateTime>,
    /// When it last changed.
    pub updated_at: Option<DateTime>,
}

impl BillingPayment {
    /// A new pending payment of `amount` through `gateway`, not saved yet.
    pub fn new(gateway: impl Into<String>, kind: PaymentKind, amount: Money) -> Self {
        Self {
            id: 0,
            billable_type: None,
            billable_id: None,
            reference_type: None,
            reference_id: None,
            amount,
            gateway: gateway.into(),
            account: None,
            gateway_id: None,
            kind,
            status: PaymentStatus::Pending,
            refunded_amount: 0,
            url: None,
            expires_at: None,
            evidence_due_by: None,
            synced_at: None,
            created_at: None,
            updated_at: None,
        }
    }

    /// The part refunded so far, in the payment's currency.
    pub fn refunded(&self) -> Money {
        Money::new(self.refunded_amount, self.amount.currency)
    }

    /// What can still be refunded; zero before it is paid.
    pub fn refundable(&self) -> Money {
        let paid = matches!(
            self.status,
            PaymentStatus::Succeeded | PaymentStatus::PartiallyRefunded | PaymentStatus::Disputed
        );
        let left = if paid {
            (self.amount.minor - self.refunded_amount).max(0)
        } else {
            0
        };
        Money::new(left, self.amount.currency)
    }

    /// Who it belongs to, if it has an owner.
    pub fn owner(&self) -> Option<Owner> {
        Some(Owner::new(self.billable_type.clone()?, self.billable_id?))
    }

    /// The payment with this id at `gateway`.
    pub async fn find_at(db: &Db, gateway: &str, gateway_id: &str) -> Result<Option<Self>> {
        Self::where_eq("gateway", gateway)
            .where_eq("gateway_id", gateway_id)
            .first(db)
            .await
    }

    /// The payments for the app's record `reference_type` / `reference_id`,
    /// newest first.
    pub async fn for_reference(
        db: &Db,
        reference_type: &str,
        reference_id: &str,
    ) -> Result<Vec<Self>> {
        Self::where_eq("reference_type", reference_type)
            .where_eq("reference_id", reference_id)
            .order_by_desc("id")
            .get(db)
            .await
    }

    /// The refunds of this payment, oldest first.
    pub async fn refunds(&self, db: &Db) -> Result<Vec<BillingRefund>> {
        BillingRefund::where_eq("payment_id", self.id)
            .order_by("id")
            .get(db)
            .await
    }
}

/// A refund of a [`BillingPayment`], whole or partial.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "billing_refunds")]
pub struct BillingRefund {
    /// The row's id.
    pub id: i64,
    /// The payment refunded.
    pub payment_id: i64,
    /// The refund's id at the gateway; `None` until it made one.
    pub gateway_id: Option<String>,
    /// The amount refunded, in the payment's currency.
    pub amount: i64,
    /// As the gateway last said.
    pub status: RefundStatus,
    /// Why, as the app or the gateway said.
    pub reason: Option<String>,
    /// When it was asked for.
    pub created_at: Option<DateTime>,
    /// When it last changed.
    pub updated_at: Option<DateTime>,
}

/// A gateway cannot do an operation (Xendit has no disputes to accept, a
/// custom gateway may not refund). It travels inside an [`Error`]; find it
/// again with [`Unsupported::of`].
///
/// ```
/// use renox::prelude::*;
/// use renox_billing::payment::Unsupported;
///
/// let err: Error = Unsupported::new("xendit", "accept_dispute").into();
/// let found = Unsupported::of(&err).expect("an unsupported error");
/// assert_eq!(found.gateway, "xendit");
/// assert_eq!(found.operation, "accept_dispute");
/// assert_eq!(err.to_string(), "The gateway xendit does not support accept_dispute.");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    /// The gateway's name.
    pub gateway: String,
    /// What was asked: `charge`, `payment_link`, `refund`, …
    pub operation: String,
}

impl Unsupported {
    /// The refusal of `operation` by `gateway`.
    pub fn new(gateway: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            gateway: gateway.into(),
            operation: operation.into(),
        }
    }

    /// The refusal inside `err`, if that is what it carries.
    pub fn of(err: &Error) -> Option<&Unsupported> {
        match err {
            Error::Internal(inner) => inner.downcast_ref::<Unsupported>(),
            _ => None,
        }
    }
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The gateway {} does not support {}.",
            self.gateway, self.operation
        )
    }
}

impl std::error::Error for Unsupported {}

/// Starts a charge or a link: the row is written first (the gateway stores
/// its id), then the gateway is called. If the gateway cannot (or fails),
/// the row is removed again, so nothing is left behind.
pub(crate) async fn start(
    state: &AppState,
    gateway: Option<&str>,
    mut request: ChargeRequest,
) -> Result<BillingPayment> {
    if request.amount.minor <= 0 {
        return Err(Error::BadRequest(
            "A payment must be for more than zero.".into(),
        ));
    }
    let gateway = crate::setup(state)?.payment_gateway(state, gateway)?;
    let mut row = BillingPayment::new(gateway.name(), request.kind, request.amount);
    row.account = request.account.clone();
    row.expires_at = request.expires_at;
    row.reference_type = request.reference_type.clone();
    row.reference_id = request.reference_id.clone();
    if let Some(owner) = &request.owner {
        row.billable_type = Some(owner.kind.clone());
        row.billable_id = Some(owner.id);
    }
    let mut row = BillingPayment::create(&state.db, row).await?;
    request.payment_id = row.id;
    let page = match request.kind {
        PaymentKind::Charge => gateway.charge(state, &request).await,
        PaymentKind::Link => gateway.payment_link(state, &request).await,
    };
    let page = match page {
        Ok(page) => page,
        Err(err) => {
            BillingPayment::query()
                .where_eq("id", row.id)
                .delete(&state.db)
                .await?;
            return Err(err);
        }
    };
    row.gateway_id = Some(page.gateway_id);
    row.url = page.url;
    if page.expires_at.is_some() {
        row.expires_at = page.expires_at;
    }
    row.save(&state.db).await?;
    Ok(row)
}

impl Billing {
    /// Makes a payment link anyone can pay, with or without an owner. The
    /// payment is stored (pending) with its reference; `payment.url` is the
    /// link. A gateway that cannot make links answers [`Unsupported`] and
    /// leaves no row.
    pub async fn payment_link(state: &AppState, link: PaymentLink) -> Result<BillingPayment> {
        start(state, link.gateway(), link.request().clone()).await
    }

    /// Refunds `refund.amount` of the payment `payment_id` (partial refunds
    /// add up). More than what is left to refund, counting refunds still
    /// pending, is refused before the gateway is called. The refund is
    /// pending unless the gateway refunded at once; then the payment moves
    /// to refunded or partially refunded and `RefundSucceeded` and
    /// `PaymentRefunded` are announced.
    pub async fn refund(
        state: &AppState,
        payment_id: i64,
        refund: Refund,
    ) -> Result<BillingRefund> {
        let mut payment = BillingPayment::find(&state.db, payment_id)
            .await?
            .ok_or(Error::NotFound)?;
        if refund.amount.currency != payment.amount.currency || refund.amount.minor <= 0 {
            return Err(Error::BadRequest(
                "A refund is a positive amount in the payment's currency.".into(),
            ));
        }
        let pending: i64 = BillingRefund::where_eq("payment_id", payment.id)
            .where_eq("status", RefundStatus::Pending)
            .sum(&state.db, "amount")
            .await?;
        if refund.amount.minor > payment.refundable().minor - pending {
            return Err(Error::BadRequest(
                "That is more than is left to refund.".into(),
            ));
        }
        let gateway = crate::setup(state)?.payment_gateway(state, Some(&payment.gateway))?;
        let mut row = BillingRefund::create(
            &state.db,
            BillingRefund {
                payment_id: payment.id,
                amount: refund.amount.minor,
                reason: refund.reason.clone(),
                ..Default::default()
            },
        )
        .await?;
        let result = match gateway.refund(state, &payment, &refund).await {
            Ok(result) => result,
            Err(err) => {
                BillingRefund::query()
                    .where_eq("id", row.id)
                    .delete(&state.db)
                    .await?;
                return Err(err);
            }
        };
        row.gateway_id = result.gateway_id;
        row.status = result.status;
        row.save(&state.db).await?;
        if row.status == RefundStatus::Succeeded {
            crate::sync::refund_done(state, &mut payment, row.amount).await?;
        }
        Ok(row)
    }

    /// Asks the gateway where the payment `payment_id` stands now (with the
    /// account it was made through), applies the answer to the row and its
    /// refunds the way a webhook would (a success always wins over failed
    /// or expired; a failure never undoes a success) and returns it.
    pub async fn payment_status(state: &AppState, payment_id: i64) -> Result<PaymentState> {
        let payment = BillingPayment::find(&state.db, payment_id)
            .await?
            .ok_or(Error::NotFound)?;
        let gateway = crate::setup(state)?.payment_gateway(state, Some(&payment.gateway))?;
        let reported = gateway.payment_status(state, &payment).await?;
        crate::sync::apply_state(state, payment, &reported).await?;
        Ok(reported)
    }
}

/// The builder methods `Charge` and `PaymentLink` share.
macro_rules! request_builder {
    ($name:ident) => {
        impl $name {
            /// Who pays; a payment link may have no owner.
            pub fn owner(mut self, owner: Owner) -> Self {
                self.request.owner = Some(owner);
                self
            }

            /// The app's record this pays for (`"job"`, `"42"`), kept on the
            /// payment and carried by its events.
            pub fn reference(mut self, kind: impl Into<String>, id: impl ToString) -> Self {
                self.request.reference_type = Some(kind.into());
                self.request.reference_id = Some(id.to_string());
                self
            }

            /// What the customer sees on the payment page.
            pub fn description(mut self, text: impl Into<String>) -> Self {
                self.request.description = Some(text.into());
                self
            }

            /// Where the gateway sends the customer after paying (absolute).
            pub fn success_url(mut self, url: impl Into<String>) -> Self {
                self.request.success_url = Some(url.into());
                self
            }

            /// Where it sends them when they give up (absolute).
            pub fn cancel_url(mut self, url: impl Into<String>) -> Self {
                self.request.cancel_url = Some(url.into());
                self
            }

            /// When the page stops working.
            pub fn expires_at(mut self, at: DateTime) -> Self {
                self.request.expires_at = Some(at);
                self
            }

            /// Uses this gateway (by name) instead of the default one.
            pub fn via(mut self, gateway: impl Into<String>) -> Self {
                self.gateway = Some(gateway.into());
                self
            }

            /// Uses this named account of the gateway; without it, the
            /// gateway's default account (and URLs) apply. The account is
            /// stored on the payment, so refunds, status checks and webhooks
            /// use the same account's keys.
            pub fn account(mut self, account: impl Into<String>) -> Self {
                self.request.account = Some(account.into());
                self
            }

            /// The gateway asked for, if [`via`](Self::via) named one.
            pub fn gateway(&self) -> Option<&str> {
                self.gateway.as_deref()
            }

            /// The request a [`Gateway`](crate::Gateway) gets.
            pub fn request(&self) -> &ChargeRequest {
                &self.request
            }
        }
    };
}

/// A one-off charge of an amount, built before it is handed to the
/// gateway. The customer pays on the gateway's page.
///
/// ```
/// use renox::money::{Currency, Money};
/// use renox_billing::payment::Charge;
///
/// let usd = Currency::parse("USD").unwrap();
/// let charge = Charge::new(Money::new(4_500, usd))
///     .reference("job", 42)
///     .description("Brake service")
///     .via("stripe")
///     .account("acme-llc");
/// assert_eq!(charge.gateway(), Some("stripe"));
/// assert_eq!(charge.request().account.as_deref(), Some("acme-llc"));
/// assert_eq!(charge.request().reference_id.as_deref(), Some("42"));
/// ```
#[derive(Debug, Clone)]
pub struct Charge {
    gateway: Option<String>,
    request: ChargeRequest,
}

impl Charge {
    /// A charge of `amount`.
    pub fn new(amount: Money) -> Self {
        Self {
            gateway: None,
            request: ChargeRequest::new(PaymentKind::Charge, amount),
        }
    }
}

request_builder!(Charge);

/// A payment link: a URL anyone can pay, with or without an owner.
///
/// ```
/// use renox::money::{Currency, Money};
/// use renox_billing::payment::PaymentLink;
///
/// let usd = Currency::parse("USD").unwrap();
/// let link = PaymentLink::new(Money::new(900, usd)).reference("invoice", "INV-7");
/// assert_eq!(link.request().reference_type.as_deref(), Some("invoice"));
/// assert!(link.gateway().is_none()); // the default gateway
/// ```
#[derive(Debug, Clone)]
pub struct PaymentLink {
    gateway: Option<String>,
    request: ChargeRequest,
}

impl PaymentLink {
    /// A link that collects `amount`.
    pub fn new(amount: Money) -> Self {
        Self {
            gateway: None,
            request: ChargeRequest::new(PaymentKind::Link, amount),
        }
    }
}

request_builder!(PaymentLink);

/// What [`Gateway::charge`](crate::Gateway::charge) and
/// [`Gateway::payment_link`](crate::Gateway::payment_link) get.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChargeRequest {
    /// Charge or link.
    pub kind: PaymentKind,
    /// The row's id in `billing_payments` (0 before it is saved).
    pub payment_id: i64,
    /// How much.
    pub amount: Money,
    /// Who pays, if known.
    pub owner: Option<Owner>,
    /// The kind of the app's record this pays for.
    pub reference_type: Option<String>,
    /// That record's id.
    pub reference_id: Option<String>,
    /// What the customer sees.
    pub description: Option<String>,
    /// Where the customer goes after paying.
    pub success_url: Option<String>,
    /// Where the customer goes when they give up.
    pub cancel_url: Option<String>,
    /// When the page stops working.
    pub expires_at: Option<DateTime>,
    /// The gateway's account; `None` is its default account.
    pub account: Option<String>,
}

impl ChargeRequest {
    /// A request of `kind` for `amount`, with nothing else set.
    pub fn new(kind: PaymentKind, amount: Money) -> Self {
        Self {
            kind,
            payment_id: 0,
            amount,
            owner: None,
            reference_type: None,
            reference_id: None,
            description: None,
            success_url: None,
            cancel_url: None,
            expires_at: None,
            account: None,
        }
    }

    /// What the gateway stores with the payment so its webhooks find the
    /// row again: `renox_payment` (the row's id), plus `renox_billable`,
    /// `renox_reference_type` and `renox_reference_id` when set.
    pub fn metadata(&self) -> Vec<(&'static str, String)> {
        let mut meta = vec![("renox_payment", self.payment_id.to_string())];
        if let Some(owner) = &self.owner {
            meta.push(("renox_billable", owner.key()));
        }
        if let Some(kind) = &self.reference_type {
            meta.push(("renox_reference_type", kind.clone()));
        }
        if let Some(id) = &self.reference_id {
            meta.push(("renox_reference_id", id.clone()));
        }
        meta
    }
}

/// What a gateway answers to a charge or a link: where the customer pays.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PaymentPage {
    /// The payment's id at the gateway.
    pub gateway_id: String,
    /// The page to send the customer to.
    pub url: Option<String>,
    /// When it stops working, if the gateway says.
    pub expires_at: Option<DateTime>,
}

impl PaymentPage {
    /// The payment `gateway_id` is paid at `url`.
    pub fn new(gateway_id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            gateway_id: gateway_id.into(),
            url: Some(url.into()),
            expires_at: None,
        }
    }

    /// It stops working at `at`.
    pub fn expires_at(mut self, at: DateTime) -> Self {
        self.expires_at = Some(at);
        self
    }
}

/// A refund to ask for: the whole payment or part of it.
///
/// ```
/// use renox::money::{Currency, Money};
/// use renox_billing::payment::Refund;
///
/// let usd = Currency::parse("USD").unwrap();
/// let refund = Refund::new(Money::new(1_000, usd)).reason("duplicate");
/// assert_eq!(refund.amount.minor, 1_000);
/// assert_eq!(refund.reason.as_deref(), Some("duplicate"));
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Refund {
    /// How much to give back.
    pub amount: Money,
    /// Why.
    pub reason: Option<String>,
}

impl Refund {
    /// A refund of `amount`.
    pub fn new(amount: Money) -> Self {
        Self {
            amount,
            reason: None,
        }
    }

    /// Says why.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

/// What a gateway answers to [`Gateway::refund`](crate::Gateway::refund).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RefundResult {
    /// The refund's id at the gateway.
    pub gateway_id: Option<String>,
    /// [`RefundStatus::Pending`] unless the gateway refunded at once.
    pub status: RefundStatus,
}

impl RefundResult {
    /// The gateway took the refund and will finish it later.
    pub fn pending(gateway_id: impl Into<String>) -> Self {
        Self {
            gateway_id: Some(gateway_id.into()),
            status: RefundStatus::Pending,
        }
    }

    /// The gateway refunded at once.
    pub fn succeeded(gateway_id: impl Into<String>) -> Self {
        Self {
            gateway_id: Some(gateway_id.into()),
            status: RefundStatus::Succeeded,
        }
    }
}

/// A payment as the gateway reports it when asked
/// ([`Gateway::payment_status`](crate::Gateway::payment_status)).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PaymentState {
    /// Pending, succeeded, failed or expired (or a later state).
    pub status: PaymentStatus,
    /// When the gateway said so (unix seconds).
    pub at: Option<i64>,
    /// The state of its refunds the gateway knows.
    pub refunds: Vec<RefundUpdate>,
}

impl PaymentState {
    /// A payment in `status`.
    pub fn new(status: PaymentStatus) -> Self {
        Self {
            status,
            at: None,
            refunds: Vec::new(),
        }
    }

    /// The gateway said this at `unix_seconds`.
    pub fn at(mut self, unix_seconds: i64) -> Self {
        self.at = Some(unix_seconds);
        self
    }

    /// With the state of one of its refunds.
    pub fn refund(mut self, update: RefundUpdate) -> Self {
        self.refunds.push(update);
        self
    }
}

/// A webhook says a one-off payment changed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PaymentUpdate {
    /// The payment's id at the gateway.
    pub gateway_id: String,
    /// Its new status.
    pub status: PaymentStatus,
    /// When the gateway said so (unix seconds).
    pub at: Option<i64>,
}

impl PaymentUpdate {
    /// Payment `gateway_id` is now `status`.
    pub fn new(gateway_id: impl Into<String>, status: PaymentStatus) -> Self {
        Self {
            gateway_id: gateway_id.into(),
            status,
            at: None,
        }
    }

    /// The gateway said this at `unix_seconds`.
    pub fn at(mut self, unix_seconds: i64) -> Self {
        self.at = Some(unix_seconds);
        self
    }
}

/// A refund changed, from a webhook or a status check.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RefundUpdate {
    /// The refund's id at the gateway.
    pub gateway_id: String,
    /// The payment's id at the gateway, when the event names it.
    pub payment_gateway_id: Option<String>,
    /// Where the refund is.
    pub status: RefundStatus,
    /// The amount, in the payment's currency, when the event says.
    pub amount: Option<i64>,
    /// Why it failed, or the reason given.
    pub reason: Option<String>,
    /// When the gateway said so (unix seconds).
    pub at: Option<i64>,
}

impl RefundUpdate {
    /// Refund `gateway_id` is now `status`.
    pub fn new(gateway_id: impl Into<String>, status: RefundStatus) -> Self {
        Self {
            gateway_id: gateway_id.into(),
            payment_gateway_id: None,
            status,
            amount: None,
            reason: None,
            at: None,
        }
    }

    /// Of the payment `id` at the gateway.
    pub fn payment(mut self, id: impl Into<String>) -> Self {
        self.payment_gateway_id = Some(id.into());
        self
    }

    /// For `minor` units of the payment's currency.
    pub fn amount(mut self, minor: i64) -> Self {
        self.amount = Some(minor);
        self
    }

    /// With this reason.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The gateway said this at `unix_seconds`.
    pub fn at(mut self, unix_seconds: i64) -> Self {
        self.at = Some(unix_seconds);
        self
    }
}

/// How a dispute stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DisputeState {
    /// The customer disputed the payment; evidence is due.
    Opened,
    /// Decided for the merchant.
    Won,
    /// Decided against the merchant.
    Lost,
}

/// A dispute opened or closed, from a webhook.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DisputeUpdate {
    /// The disputed payment's id at the gateway.
    pub payment_gateway_id: String,
    /// How it stands.
    pub state: DisputeState,
    /// When the evidence is due, for an opened dispute.
    pub evidence_due_by: Option<DateTime>,
    /// When the gateway said so (unix seconds).
    pub at: Option<i64>,
}

impl DisputeUpdate {
    /// The dispute on payment `payment_gateway_id` is now `state`.
    pub fn new(payment_gateway_id: impl Into<String>, state: DisputeState) -> Self {
        Self {
            payment_gateway_id: payment_gateway_id.into(),
            state,
            evidence_due_by: None,
            at: None,
        }
    }

    /// The evidence is due by `at`.
    pub fn evidence_due_by(mut self, at: DateTime) -> Self {
        self.evidence_due_by = Some(at);
        self
    }

    /// The gateway said this at `unix_seconds`.
    pub fn at(mut self, unix_seconds: i64) -> Self {
        self.at = Some(unix_seconds);
        self
    }
}
