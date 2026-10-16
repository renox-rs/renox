//! An order's life after the checkout, and the customer's order pages.
//!
//! ```text
//! pending ──paid──▶ paid ──▶ ready (pickup: ready to collect / delivery: sent out) ──▶ completed
//!    │                                                                       │
//!    └─ 30 min unpaid, payment failed ──▶ cancelled                          └─ return ≤ 14 days ──▶ refunded
//! ```
//!
//! - [`paid`] runs when the payment goes through (the `PaymentSucceeded`
//!   listener): the reservation becomes a sale, bikes are registered to the
//!   customer, the confirmation mail is queued.
//! - [`cancel`] runs when the payment fails or the 30 minutes pass
//!   ([`expire`], a scheduled task every minute): the reservation is
//!   released and the customer told.
//! - Staff move it on and take returns (`src/app/sales/staff.rs`).
//!
//! Every change of status is a conditional update (`WHERE status = 'pending'`),
//! so a webhook that comes twice, or a payment that arrives as the order
//! expires, changes it once.
//!
//! The customer's pages: `GET /orders/{order}` (`orders.show`) and its
//! printable invoice `GET /orders/{order}/invoice` (`orders.invoice`).
//! Who may see one: the customer whose account placed it, the browser that
//! placed it (the order's id is in its session), anyone with the signed link
//! from a mail (`orders.signed`), and staff who may see orders in its store.

use renox::chrono::Duration;
use renox::prelude::*;
use renox::signed::ValidSignature;
use serde::Serialize;

use super::ledger;
use super::model::{Channel, Fulfilment, Order, OrderItem, OrderStatus, Payment, PaymentStatus};
use super::notify::{self, Moment, OrderView};
use super::payments::{self, Payable, PaymentFailed, PaymentSucceeded};
use crate::app::access;
use crate::app::accounts::model::Customer;
use crate::app::catalog::model::{Category, CategoryKind, Product, ProductVariant};
use crate::app::workshop::model::CustomerBike;

/// Unpaid online orders are cancelled after this long.
pub const PAY_WITHIN_MINUTES: i64 = 30;
/// Days after the handover a customer may bring goods back.
pub const RETURN_DAYS: i64 = 14;
/// The session key of the orders this browser placed (guests too).
pub const SESSION_ORDERS: &str = "my_orders";

// [explain:orders.show.paid]
/// The `PaymentSucceeded` listener for orders: the order becomes paid.
pub async fn on_payment(event: PaymentSucceeded, state: AppState) -> Result {
    if let Payable::Order(id) = event.payable {
        paid(&state, id, None).await?;
    }
    Ok(())
}
// [/explain:orders.show.paid]

/// The `PaymentFailed` listener for orders: the order is cancelled.
pub async fn on_payment_failed(event: PaymentFailed, state: AppState) -> Result {
    if let Payable::Order(id) = event.payable
        && let Some(order) = Order::find(&state.db, id).await?
    {
        cancel(&state, &order, None).await?;
    }
    Ok(())
}

// [explain:orders.show.paid]
/// Marks the order paid (once): its reservation becomes a sale, each bike
/// bought becomes one of the customer's bikes, and the confirmation mail
/// is queued. `staff_id`: who sold it, at the counter.
pub async fn paid(state: &AppState, order_id: i64, staff_id: Option<i64>) -> Result<bool> {
    let db = &state.db;
    let now = renox::db::now();
    let mut tx = db.begin().await?;
    let moved = Order::query()
        .where_(Order::ID.eq(order_id))
        .where_(Order::STATUS.eq(OrderStatus::Pending))
        .update(
            &mut tx,
            &[("status", &OrderStatus::Paid), ("paid_at", &now)],
        )
        .await?;
    if moved == 0 {
        tx.rollback().await?;
        return Ok(false); // paid already, or cancelled
    }
    let order = Order::find_or_404(&mut tx, order_id).await?;
    let items = OrderItem::query()
        .where_(OrderItem::ORDER_ID.eq(order.id))
        .get(&mut tx)
        .await?;
    ledger::sell(&mut tx, &order, &items, staff_id.or(order.served_by)).await?;
    tx.commit().await?;
    register_bikes(db, &order, &items).await?;
    notify::tell(state, &order, Moment::Paid, None).await?;
    Ok(true)
}
// [/explain:orders.show.paid]

/// Cancels a pending order (once): the reservation is released, a pending
/// payment fails, and the customer hears why.
pub async fn cancel(state: &AppState, order: &Order, staff_id: Option<i64>) -> Result<bool> {
    let mut tx = state.db.begin().await?;
    let moved = Order::query()
        .where_(Order::ID.eq(order.id))
        .where_(Order::STATUS.eq(OrderStatus::Pending))
        .update(&mut tx, &[("status", &OrderStatus::Cancelled)])
        .await?;
    if moved == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    ledger::release(&mut tx, order, staff_id).await?;
    Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq(Order::TABLE))
        .where_(Payment::PAYABLE_ID.eq(order.id))
        .where_(Payment::STATUS.eq(PaymentStatus::Pending))
        .update(&mut tx, &[("status", &PaymentStatus::Failed)])
        .await?;
    tx.commit().await?;
    if order.channel == Channel::Online {
        notify::tell(state, order, Moment::Expired, None).await?;
    }
    Ok(true)
}

/// The scheduled task `sales:expire-orders` (every minute): online orders
/// still unpaid after [`PAY_WITHIN_MINUTES`] are cancelled. Their pending
/// payments are marked failed through the payments contract, whose
/// `PaymentFailed` cancels the order; orders without one are cancelled
/// directly. Returns how many were cancelled.
pub async fn expire(state: AppState) -> Result<usize> {
    let cutoff = renox::db::now() - Duration::minutes(PAY_WITHIN_MINUTES);
    let overdue = Order::query()
        .where_(Order::STATUS.eq(OrderStatus::Pending))
        .where_(Order::CHANNEL.eq(Channel::Online))
        .where_(Order::PLACED_AT.lt(cutoff))
        .order_by(Order::ID)
        .limit(500)
        .get(&state.db)
        .await?;
    let mut cancelled = 0;
    for order in overdue {
        let pending: Vec<i64> = Payment::query()
            .where_(Payment::PAYABLE_TYPE.eq(Order::TABLE))
            .where_(Payment::PAYABLE_ID.eq(order.id))
            .where_(Payment::STATUS.eq(PaymentStatus::Pending))
            .pluck(&state.db, Payment::ID)
            .await?;
        for payment in pending {
            payments::mark_failed(&state, payment).await?;
        }
        if cancel(&state, &order, None).await? {
            cancelled += 1;
        } else if Order::find(&state.db, order.id)
            .await?
            .is_some_and(|o| o.status == OrderStatus::Cancelled)
        {
            cancelled += 1; // cancelled by the PaymentFailed listener
        }
    }
    Ok(cancelled)
}

/// Each bike in the order becomes one of the customer's bikes (the
/// workshop's `customer_bikes`), linked to the order; the frame number is
/// added at the handover. Walk-ins without a customer register nothing.
pub async fn register_bikes(db: &Db, order: &Order, items: &[OrderItem]) -> Result<usize> {
    let Some(customer_id) = order.customer_id else {
        return Ok(0);
    };
    if CustomerBike::query()
        .where_(CustomerBike::ORDER_ID.eq(order.id))
        .exists(db)
        .await?
    {
        return Ok(0); // once
    }
    let variants =
        ProductVariant::find_many(db, items.iter().map(|i| i.variant_id).collect::<Vec<_>>())
            .await?;
    let products = Product::query()
        .with_trashed()
        .where_(Product::ID.is_in(variants.iter().map(|v| v.product_id).collect::<Vec<_>>()))
        .get(db)
        .await?;
    let bikes: Vec<i64> = Category::query()
        .where_(Category::KIND.eq(CategoryKind::Bike))
        .pluck(db, Category::ID)
        .await?;
    let today = order.paid_at.unwrap_or_else(renox::db::now).date_naive();
    let mut made = 0;
    for item in items {
        let Some(variant) = variants.iter().find(|v| v.id == item.variant_id) else {
            continue;
        };
        let Some(product) = products.iter().find(|p| p.id == variant.product_id) else {
            continue;
        };
        if !bikes.contains(&product.category_id) {
            continue;
        }
        let name = match &variant.colour {
            Some(colour) => format!("{}, {}", product.name, colour.to_lowercase()),
            None => product.name.clone(),
        };
        for _ in 0..item.quantity {
            CustomerBike::create(
                db,
                CustomerBike {
                    customer_id,
                    product_id: Some(product.id),
                    name: name.clone(),
                    order_id: Some(order.id),
                    bought_on: Some(today),
                    ..Default::default()
                },
            )
            .await?;
            made += 1;
        }
    }
    Ok(made)
}

/// Whether the order can still be returned: completed (or paid, for a
/// delivery not marked yet) less than [`RETURN_DAYS`] ago.
pub fn returnable(order: &Order) -> bool {
    let since = order.completed_at.or(order.paid_at);
    matches!(
        order.status,
        OrderStatus::Completed | OrderStatus::Ready | OrderStatus::Paid
    ) && since.is_some_and(|t| renox::db::now() - t <= Duration::days(RETURN_DAYS))
}

/// Remembers in the session that this browser placed (or was given the
/// link to) the order.
pub fn remember(session: &Session, order_id: i64) -> Result {
    let mut ids: Vec<i64> = session.get(SESSION_ORDERS).unwrap_or_default();
    if !ids.contains(&order_id) {
        ids.push(order_id);
        if ids.len() > 20 {
            ids.remove(0);
        }
        session.put(SESSION_ORDERS, &ids)?;
    }
    Ok(())
}

/// Whether this visitor may see the order (see the module docs). The
/// customer lookup runs only for a logged-in user who didn't place it here.
pub async fn may_see(
    db: &Db,
    session: &Session,
    user: Option<&User>,
    order: &Order,
) -> Result<bool> {
    let ids: Vec<i64> = session.get(SESSION_ORDERS).unwrap_or_default();
    if ids.contains(&order.id) {
        return Ok(true);
    }
    let Some(user) = user else {
        return Ok(false);
    };
    if access::can_see(user, order) {
        return Ok(true);
    }
    Ok(match order.customer_id {
        Some(id) => Customer::find(db, id)
            .await?
            .is_some_and(|c| c.user_id == Some(user.id)),
        None => false,
    })
}

// [explain:orders.show.handler]
/// The order, or a 404 when this visitor may not see it (another
/// customer's order doesn't exist for them).
pub async fn visible(db: &Db, session: &Session, user: Option<&User>, id: i64) -> Result<Order> {
    let order = Order::find_or_404(db, id).await?;
    if !may_see(db, session, user, &order).await? {
        return Err(Error::NotFound);
    }
    Ok(order)
}
// [/explain:orders.show.handler]

/// Payments of an order, for its page.
#[derive(Serialize, Debug, Clone)]
pub struct OrderPage {
    #[serde(flatten)]
    pub view: OrderView,
    pub payments: Vec<Payment>,
    pub returnable: bool,
    pub can_pay: bool,
}

async fn page_data(db: &Db, order: Order) -> Result<OrderPage> {
    let payments = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq(Order::TABLE))
        .where_(Payment::PAYABLE_ID.eq(order.id))
        .order_by(Payment::ID)
        .get(db)
        .await?;
    let returnable = returnable(&order);
    let can_pay = order.status == OrderStatus::Pending && order.channel == Channel::Online;
    Ok(OrderPage {
        view: OrderView::load(db, order).await?,
        payments,
        returnable,
        can_pay,
    })
}

// [explain:orders.show.handler]
/// `GET /orders/{order}` (`orders.show`): the order's status, lines,
/// totals, payments and what happens next.
pub async fn show(
    State(state): State<AppState>,
    session: Session,
    user: Option<AuthUser>,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let order = visible(db, &session, user.as_deref(), id).await?;
    let page = page_data(db, order).await?;
    let steps = steps(&page.view.order);
    let progress = progress(&state, &steps);
    Ok(view(
        "sales/orders/show.html",
        context! { page, steps, progress },
    ))
}
// [/explain:orders.show.handler]

/// The timeline (the `history` block takes these): the steps reached, with
/// their words and a tone.
fn progress(state: &AppState, steps: &[Step]) -> Vec<renox::serde_json::Value> {
    let lang = state.current_lang();
    steps
        .iter()
        .filter(|step| step.done)
        .map(|step| {
            let kind = match step.key {
                "cancelled" => "error",
                "refunded" => "info",
                _ => "success",
            };
            json!({
                "time": step.at,
                "title": lang.t(&format!("sales.step.{}", step.key), &[]),
                "kind": kind,
            })
        })
        .collect()
}

// [explain:orders.invoice.handler]
/// `GET /orders/{order}/invoice` (`orders.invoice`): the printable invoice
/// (or the counter's receipt).
pub async fn invoice(
    State(state): State<AppState>,
    session: Session,
    user: Option<AuthUser>,
    Path(id): Path<i64>,
) -> Result<View> {
    let order = visible(&state.db, &session, user.as_deref(), id).await?;
    let page = page_data(&state.db, order).await?;
    let receipt = receipt(&state, &page.view);
    Ok(view(
        "sales/orders/invoice.html",
        context! { page, receipt },
    ))
}

/// `GET /orders/{order}/view?signature=…` (`orders.signed`): the link in
/// the mails. A valid signature lets this browser see the order, then
/// goes to its page.
pub async fn signed(_: ValidSignature, session: Session, Path(id): Path<i64>) -> Result<Redirect> {
    remember(&session, id)?;
    Ok(Redirect::to(&format!("/orders/{id}")))
}
// [/explain:orders.invoice.handler]

/// What the counter's till receipt (the `receipt` block, 80 mm) prints: the
/// lines with their quantity and price, the totals, the store's lines and the
/// shop's thanks, in the language of the person looking at it.
fn receipt(state: &AppState, order: &OrderView) -> renox::serde_json::Value {
    let lang = state.current_lang();
    let lines: Vec<_> = order
        .lines
        .iter()
        .map(|line| {
            json!({
                "name": line.name,
                "note": line.variant,
                "quantity": line.item.quantity,
                "price": line.item.unit_price,
                "total": line.item.total,
            })
        })
        .collect();
    let mut totals = vec![json!({
        "label": lang.t("sales.orders.subtotal", &[]),
        "amount": order.order.subtotal,
    })];
    if order.order.discount > 0 {
        totals.push(json!({
            "label": lang.t("sales.orders.discount", &[]),
            "amount": -order.order.discount,
        }));
    }
    if order.delivery {
        totals.push(json!({
            "label": lang.t("sales.checkout.delivery_fee", &[]),
            "amount": order.order.delivery_fee,
        }));
    }
    totals.push(json!({
        "label": lang.t("sales.orders.total", &[]),
        "amount": order.order.total,
        "strong": true,
    }));
    let header: Vec<String> = order
        .store
        .iter()
        .flat_map(|store| [store.name.clone(), store.phone.clone()])
        .filter(|line| !line.is_empty())
        .collect();
    json!({
        "lines": lines,
        "totals": totals,
        "header": header,
        "footer": [lang.t("sales.invoice.footer", &[("days", &14)])],
    })
}

/// One step of the order's progress, for the page's history.
#[derive(Serialize, Debug, Clone)]
pub struct Step {
    pub key: &'static str,
    pub at: Option<DateTime>,
    pub done: bool,
}

/// The steps an order goes through, done or not.
pub fn steps(order: &Order) -> Vec<Step> {
    let reached = |s: OrderStatus| -> bool {
        let rank = |s: OrderStatus| match s {
            OrderStatus::Pending => 0,
            OrderStatus::Paid => 1,
            OrderStatus::Ready => 2,
            OrderStatus::Completed => 3,
            OrderStatus::Refunded => 4,
            OrderStatus::Cancelled => -1,
        };
        rank(order.status) >= rank(s)
    };
    let mut steps = vec![
        Step {
            key: "placed",
            at: order.placed_at,
            done: true,
        },
        Step {
            key: "paid",
            at: order.paid_at,
            done: reached(OrderStatus::Paid),
        },
        Step {
            key: if order.fulfilment == Fulfilment::Delivery {
                "shipped"
            } else {
                "ready"
            },
            at: None,
            done: reached(OrderStatus::Ready),
        },
        Step {
            key: "completed",
            at: order.completed_at,
            done: reached(OrderStatus::Completed),
        },
    ];
    match order.status {
        OrderStatus::Cancelled => steps.push(Step {
            key: "cancelled",
            at: order.updated_at,
            done: true,
        }),
        OrderStatus::Refunded => steps.push(Step {
            key: "refunded",
            at: order.returned_at,
            done: true,
        }),
        _ => {}
    }
    // A step done without its own time (ready) shows the order's last change.
    for step in &mut steps {
        if step.done && step.at.is_none() {
            step.at = order.updated_at.or(order.placed_at);
        }
    }
    steps
}
