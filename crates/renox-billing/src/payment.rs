//! One-off payments: the `billing_payments` and `billing_refunds` tables,
//! their status types and the [`Unsupported`] error a gateway answers with
//! for an operation it cannot do.

use std::fmt;

use renox::money::Money;
use renox::prelude::*;
use serde::Serialize;

use crate::Owner;

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
