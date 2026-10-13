//! Payments for anything the shop charges for: an order, a rental's
//! deposit or fees, a work order. One contract shared by the sales,
//! rentals and workshop areas.
//!
//! - Online: [`start`] records a pending `payments` row and gives the URL
//!   of the gateway's hosted page; the gateway's webhook marks it paid and
//!   the app emits [`PaymentSucceeded`] (or [`PaymentFailed`]).
//! - At the counter: [`record_counter`] records a cash or card payment
//!   that is paid at once, and emits [`PaymentSucceeded`] too.
//!
//! The areas that sell something listen to [`PaymentSucceeded`] for their
//! own [`Payable`] kind (an order becomes paid, a rental's deposit is
//! held, a work order is settled), so the payment code never needs to know
//! about them.
//!
//! The gateway itself (Midtrans' hosted page and signed webhook through
//! `renox::webhook`, or the demo gateway without a key) is in
//! `src/app/sales/gateway.rs`; the page the customer comes back to is
//! `/pay/{payment}`. Keep these signatures: other areas call them.

use renox::prelude::*;

use super::model::{
    PAYABLE_ORDER, PAYABLE_RENTAL, PAYABLE_WORK_ORDER, Payment, PaymentMethod, PaymentStatus,
};

/// What a payment pays for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payable {
    /// An online or counter order (`orders`).
    Order(i64),
    /// A rental: its deposit or its fees (`rentals`).
    Rental(i64),
    /// A workshop job (`work_orders`).
    WorkOrder(i64),
}

impl Payable {
    /// The `payable_type` stored in `payments`.
    pub fn kind(self) -> &'static str {
        match self {
            Payable::Order(_) => PAYABLE_ORDER,
            Payable::Rental(_) => PAYABLE_RENTAL,
            Payable::WorkOrder(_) => PAYABLE_WORK_ORDER,
        }
    }

    /// The row it points at.
    pub fn id(self) -> i64 {
        match self {
            Payable::Order(id) | Payable::Rental(id) | Payable::WorkOrder(id) => id,
        }
    }

    /// Reads it back from a `payments` row.
    pub fn of(payment: &Payment) -> Option<Self> {
        match payment.payable_type.as_str() {
            PAYABLE_ORDER => Some(Payable::Order(payment.payable_id)),
            PAYABLE_RENTAL => Some(Payable::Rental(payment.payable_id)),
            PAYABLE_WORK_ORDER => Some(Payable::WorkOrder(payment.payable_id)),
            _ => None,
        }
    }
}

/// An online payment waiting for the customer on the gateway's page.
#[derive(Debug, Clone)]
pub struct Checkout {
    /// The pending `payments` row.
    pub payment_id: i64,
    /// Where to send the customer to pay.
    pub redirect_url: String,
}

/// What to charge.
#[derive(Debug, Clone)]
pub struct Charge {
    /// What it pays for.
    pub payable: Payable,
    /// Who pays (none for a walk-in at the counter).
    pub customer_id: Option<i64>,
    /// The operating store, which receives the money.
    pub store_id: i64,
    /// In the smallest unit of `APP_CURRENCY`.
    pub amount: i64,
}

/// A payment went through: online (the webhook) or at the counter.
#[derive(Debug, Clone)]
pub struct PaymentSucceeded {
    /// The `payments` row, now `paid`.
    pub payment_id: i64,
    /// What it paid for.
    pub payable: Payable,
    /// How much.
    pub amount: i64,
}

impl Event for PaymentSucceeded {}

/// An online payment failed or expired.
#[derive(Debug, Clone)]
pub struct PaymentFailed {
    /// The `payments` row, now `failed`.
    pub payment_id: i64,
    /// What it was for.
    pub payable: Payable,
}

impl Event for PaymentFailed {}

/// Starts an online payment: a pending row and the gateway's page.
pub async fn start(state: &AppState, charge: Charge) -> Result<Checkout> {
    let payment = Payment::create(
        &state.db,
        Payment {
            customer_id: charge.customer_id,
            payable_type: charge.payable.kind().into(),
            payable_id: charge.payable.id(),
            store_id: charge.store_id,
            amount: charge.amount,
            method: PaymentMethod::Gateway,
            status: PaymentStatus::Pending,
            ..Default::default()
        },
    )
    .await?;
    // The gateway's hosted page: Midtrans' with MIDTRANS_SERVER_KEY set,
    // else the demo gateway's (src/app/sales/gateway.rs). Its webhook
    // calls `mark_paid` / `mark_failed`.
    let redirect_url = super::gateway::hosted_page(state, &payment).await?;
    Ok(Checkout {
        payment_id: payment.id,
        redirect_url,
    })
}

/// Records a payment taken at the counter (cash or card), paid at once,
/// and emits [`PaymentSucceeded`].
pub async fn record_counter(
    state: &AppState,
    charge: Charge,
    method: PaymentMethod,
    received_by: impl Into<Option<i64>>,
) -> Result<Payment> {
    let payment = Payment::create(
        &state.db,
        Payment {
            customer_id: charge.customer_id,
            payable_type: charge.payable.kind().into(),
            payable_id: charge.payable.id(),
            store_id: charge.store_id,
            amount: charge.amount,
            method,
            status: PaymentStatus::Paid,
            paid_at: Some(renox::db::now()),
            // The staff row that took it; none for the owner, who has a
            // global role but may not be on any store's staff.
            received_by: received_by.into(),
            ..Default::default()
        },
    )
    .await?;
    state
        .emit(PaymentSucceeded {
            payment_id: payment.id,
            payable: charge.payable,
            amount: payment.amount,
        })
        .await?;
    Ok(payment)
}

/// Marks a pending online payment paid and emits [`PaymentSucceeded`];
/// called by the gateway's webhook. Paying twice does nothing.
pub async fn mark_paid(state: &AppState, payment_id: i64, reference: &str) -> Result<()> {
    let Some(mut payment) = Payment::find(&state.db, payment_id).await? else {
        return Ok(());
    };
    if payment.status == PaymentStatus::Paid {
        return Ok(());
    }
    payment.status = PaymentStatus::Paid;
    payment.paid_at = Some(renox::db::now());
    payment.gateway_reference = Some(reference.to_owned());
    payment.save(&state.db).await?;
    if let Some(payable) = Payable::of(&payment) {
        state
            .emit(PaymentSucceeded {
                payment_id: payment.id,
                payable,
                amount: payment.amount,
            })
            .await?;
    }
    Ok(())
}

/// Marks a pending online payment failed and emits [`PaymentFailed`].
pub async fn mark_failed(state: &AppState, payment_id: i64) -> Result<()> {
    let Some(mut payment) = Payment::find(&state.db, payment_id).await? else {
        return Ok(());
    };
    if payment.status != PaymentStatus::Pending {
        return Ok(());
    }
    payment.status = PaymentStatus::Failed;
    payment.save(&state.db).await?;
    if let Some(payable) = Payable::of(&payment) {
        state
            .emit(PaymentFailed {
                payment_id: payment.id,
                payable,
            })
            .await?;
    }
    Ok(())
}
