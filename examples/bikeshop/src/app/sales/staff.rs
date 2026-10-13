//! The staff's side of orders: the store's orders, one order with its
//! ledger, and the actions that move it on.
//!
//! | Route | Name | Needs (in the order's store) |
//! |---|---|---|
//! | `GET /staff/orders` | `sales.orders.index` | `orders.view` |
//! | `GET /staff/orders/{order}` | `sales.orders.show` | `orders.view` |
//! | `POST /staff/orders/{order}/ready` | `sales.orders.ready` | `orders.sell` |
//! | `POST /staff/orders/{order}/complete` | `sales.orders.complete` | `orders.sell` |
//! | `POST /staff/orders/{order}/cancel` | `sales.orders.cancel` | `orders.sell` |
//! | `POST /staff/orders/{order}/return` | `sales.orders.return` | `orders.refund` |
//!
//! Lists show the orders of the active store (`access::visible` plus the
//! store chosen in the switcher); one order is found with `access::find`
//! (another store's order is a 404) and each action is checked with
//! `access::require` against the order's **operating** store, the one that
//! served the customer (#245's rule for orders, returns and refunds).

use std::collections::HashMap;

use renox::chrono::Duration;
use renox::db::relations::belongs_to;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::ledger;
use super::model::{
    Fulfilment, Order, OrderItem, OrderStatus, Payment, PaymentMethod, PaymentStatus,
};
use super::notify::{self, Moment, OrderView};
use super::orders::{self, RETURN_DAYS};
use crate::app::access::{self, StoreAttr, active_store, catalogue};
use crate::app::accounts::model::Customer;
use crate::app::staff::model::Staff;
use crate::app::stock::model::StockMovement;
use crate::app::workshop::model::CustomerBike;

/// Orders per page.
pub const PER_PAGE: u32 = 25;

/// The status tabs of the list.
pub const TABS: [&str; 6] = [
    "open",
    "pending",
    "ready",
    "completed",
    "cancelled",
    "refunded",
];

/// `?status=` and `?page=`.
#[derive(Deserialize, Default, Debug)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
}

/// A row of the list.
#[derive(Serialize, Debug, Clone)]
pub struct Row {
    #[serde(flatten)]
    pub order: Order,
    pub customer: Option<String>,
}

/// `GET /staff/orders` (`sales.orders.index`): the active store's orders,
/// newest first, by status. Three queries for a page of 25 (count, page,
/// customers).
// [explain:sales.orders.index.handler]
pub async fn index(
    State(state): State<AppState>,
    Page(page): Page,
    Query(q): Query<ListQuery>,
) -> Result<View> {
    let db = state.db.clone();
    let store = active_store::current();
    let tab = q
        .status
        .filter(|s| TABS.contains(&s.as_str()))
        .unwrap_or_else(|| "open".to_owned());
    let query = access::visible::<Order>(catalogue::ORDERS_VIEW).when(store.is_some(), |q| {
        q.where_(Order::OPERATING_STORE_ID.eq(store.unwrap_or_default()))
    });
    let query = match tab.as_str() {
        "open" => query.where_(Order::STATUS.is_in([OrderStatus::Paid, OrderStatus::Ready])),
        "pending" => query.where_(Order::STATUS.eq(OrderStatus::Pending)),
        "ready" => query.where_(Order::STATUS.eq(OrderStatus::Ready)),
        "completed" => query.where_(Order::STATUS.eq(OrderStatus::Completed)),
        "cancelled" => query.where_(Order::STATUS.eq(OrderStatus::Cancelled)),
        _ => query.where_(Order::STATUS.eq(OrderStatus::Refunded)),
    };
    let orders = query
        .order_by_desc(Order::PLACED_AT)
        .order_by_desc(Order::ID)
        .paginate(&db, page, PER_PAGE)
        .await?;
    let customers = belongs_to::<Customer, _, _>(&db, &orders.items, |o| o.customer_id).await?;
    // [/explain:sales.orders.index.handler]
    let orders = orders.map(|order| Row {
        customer: order
            .customer_id
            .and_then(|id| customers.get(&id))
            .map(|c| c.name.clone()),
        order,
    });
    // The tabs: each status's address and its label.
    let lang = state.current_lang();
    let base = state.url("sales.orders.index", &[])?;
    let tab_items: Vec<[String; 2]> = TABS
        .iter()
        .map(|key| {
            [
                format!("{base}?status={key}"),
                lang.t(&format!("sales.staff.tab.{key}"), &[]),
            ]
        })
        .collect();
    let current_tab = format!("{base}?status={tab}");
    Ok(view(
        "sales/staff/index.html",
        context! { orders, tab, tabs => TABS, tab_items, current_tab },
    ))
}

/// One order as the staff page shows it.
#[derive(Serialize, Debug, Clone)]
pub struct StaffOrder {
    #[serde(flatten)]
    pub view: OrderView,
    pub payments: Vec<Payment>,
    pub movements: Vec<StockMovement>,
    /// The bikes it registered to the customer (for their frame numbers).
    pub bikes: Vec<CustomerBike>,
    pub can_sell: bool,
    pub can_refund: bool,
    pub returnable: bool,
    /// The last day a return is taken.
    pub return_until: Option<DateTime>,
}

// [explain:sales.orders.show.handler]
/// `GET /staff/orders/{order}` (`sales.orders.show`).
pub async fn show(State(db): State<Db>, user: AuthUser, Path(id): Path<i64>) -> Result<View> {
    let order = access::find::<Order>(&db, &user, id).await?;
    let can_sell = access::can(&user, catalogue::ORDERS_SELL, StoreAttr::Operating, &order);
    let can_refund = access::can(
        &user,
        catalogue::ORDERS_REFUND,
        StoreAttr::Operating,
        &order,
    );
    let returnable = orders::returnable(&order);
    // [/explain:sales.orders.show.handler]
    let return_until = order
        .completed_at
        .or(order.paid_at)
        .map(|t| t + Duration::days(RETURN_DAYS));
    let payments = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq(Order::TABLE))
        .where_(Payment::PAYABLE_ID.eq(order.id))
        .order_by(Payment::ID)
        .get(&db)
        .await?;
    let movements = StockMovement::query()
        .where_(StockMovement::REFERENCE_TYPE.eq(Order::TABLE))
        .where_(StockMovement::REFERENCE_ID.eq(order.id))
        .order_by(StockMovement::ID)
        .get(&db)
        .await?;
    let bikes = CustomerBike::query()
        .where_(CustomerBike::ORDER_ID.eq(order.id))
        .order_by(CustomerBike::ID)
        .get(&db)
        .await?;
    let data = StaffOrder {
        view: OrderView::load(&db, order).await?,
        payments,
        movements,
        bikes,
        can_sell,
        can_refund,
        returnable,
        return_until,
    };
    Ok(view("sales/staff/show.html", context! { data }))
}

/// The answer to an action: htmx (an action sheet) reloads the page, which
/// shows the toast; a plain form goes back to the order.
fn done(htmx: &Htmx, toast: Toast, id: i64) -> Response {
    if htmx.request {
        (toast, renox::HxRefresh).into_response()
    } else {
        (toast, Redirect::to(&format!("/staff/orders/{id}"))).into_response()
    }
}

/// The staff row of the logged-in user (who did it, in the ledger).
async fn staff_id(db: &Db, user: &User) -> Result<Option<i64>> {
    Ok(Staff::of_user(db, user.id).await?.map(|s| s.id))
}

// [explain:sales.orders.show.handler]
/// `POST /staff/orders/{order}/ready` (`sales.orders.ready`): a paid
/// order is ready to collect, or (a delivery) sent out; the customer hears.
pub async fn ready(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    htmx: Htmx,
    Path(id): Path<i64>,
) -> Result<Response> {
    let order = access::find::<Order>(&state.db, &user, id).await?;
    access::require(&user, catalogue::ORDERS_SELL, StoreAttr::Operating, &order)?;
    let moved = Order::query()
        .where_(Order::ID.eq(order.id))
        .where_(Order::STATUS.eq(OrderStatus::Paid))
        .update(&state.db, &[("status", &OrderStatus::Ready)])
        .await?;
    abort_if(
        moved == 0,
        StatusCode::CONFLICT,
        lang.t("sales.staff.not_paid", &[]),
    )?;
    // [/explain:sales.orders.show.handler]
    let moment = if order.fulfilment == Fulfilment::Delivery {
        Moment::Shipped
    } else {
        Moment::Ready
    };
    notify::tell(&state, &order, moment, None).await?;
    let text = lang.t(
        &format!("sales.staff.{}_done", moment.key()),
        &[("number", &order.number)],
    );
    // [explain:sales.orders.show.handler]
    Ok(done(&htmx, Toast::success(text), id))
}
// [/explain:sales.orders.show.handler]

/// A bike's frame number, written at the handover.
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Frame {
    pub bike_id: i64,
    #[serde(default)]
    pub number: Option<String>,
}

/// The handover form: the frame numbers of the bikes in the order.
#[derive(Deserialize, Serialize, Validate, Debug, Default)]
pub struct CompleteForm {
    #[serde(default)]
    #[validate(max = 20)]
    pub frames: Vec<Frame>,
}

/// `POST /staff/orders/{order}/complete` (`sales.orders.complete`): the
/// goods are in the customer's hands (picked up or delivered); the bikes
/// get their frame numbers. The 14-day return window starts now.
pub async fn complete(
    State(db): State<Db>,
    user: AuthUser,
    lang: Lang,
    htmx: Htmx,
    Path(id): Path<i64>,
    Valid(form): Valid<CompleteForm>,
) -> Result<Response> {
    let order = access::find::<Order>(&db, &user, id).await?;
    access::require(&user, catalogue::ORDERS_SELL, StoreAttr::Operating, &order)?;
    let now = renox::db::now();
    let moved = Order::query()
        .where_(Order::ID.eq(order.id))
        .where_(Order::STATUS.is_in([OrderStatus::Paid, OrderStatus::Ready]))
        .update(
            &db,
            &[("status", &OrderStatus::Completed), ("completed_at", &now)],
        )
        .await?;
    abort_if(
        moved == 0,
        StatusCode::CONFLICT,
        lang.t("sales.staff.not_paid", &[]),
    )?;
    for frame in form.frames {
        let number = frame
            .number
            .map(|n| n.trim().to_uppercase())
            .filter(|n| !n.is_empty());
        if let Some(number) = number {
            CustomerBike::query()
                .where_(CustomerBike::ID.eq(frame.bike_id))
                .where_(CustomerBike::ORDER_ID.eq(order.id))
                .update(&db, &[("frame_number", &number)])
                .await?;
        }
    }
    Ok(done(
        &htmx,
        Toast::success(lang.t("sales.staff.completed", &[("number", &order.number)])),
        id,
    ))
}

/// `POST /staff/orders/{order}/cancel` (`sales.orders.cancel`): an unpaid
/// order called off; its reservation is released.
pub async fn cancel(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    htmx: Htmx,
    Path(id): Path<i64>,
) -> Result<Response> {
    let order = access::find::<Order>(&state.db, &user, id).await?;
    access::require(&user, catalogue::ORDERS_SELL, StoreAttr::Operating, &order)?;
    let staff = staff_id(&state.db, &user).await?;
    let cancelled = orders::cancel(&state, &order, staff).await?;
    abort_if(
        !cancelled,
        StatusCode::CONFLICT,
        lang.t("sales.staff.not_pending", &[]),
    )?;
    Ok(done(
        &htmx,
        Toast::info(lang.t("sales.staff.cancelled", &[("number", &order.number)])),
        id,
    ))
}

/// One line coming back.
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ReturnLine {
    pub item_id: i64,
    #[serde(default)]
    pub quantity: i64,
}

/// The return form: how many of each line come back, and why.
#[derive(Deserialize, Serialize, Validate, Debug, Default)]
pub struct ReturnForm {
    #[serde(default)]
    #[validate(required, max = 50)]
    pub lines: Vec<ReturnLine>,
    #[validate(required, max = 300)]
    pub reason: String,
}

/// `POST /staff/orders/{order}/return` (`sales.orders.return`): goods
/// brought back within 14 days. In one transaction: a `return` movement per
/// line (the stock is back at the store, still its owner's), the books
/// reversed for consigned goods, a refund in `payments`; the order is
/// `refunded` when everything came back. The customer gets a mail.
pub async fn take_back(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    htmx: Htmx,
    Path(id): Path<i64>,
    Valid(form): Valid<ReturnForm>,
) -> Result<Response> {
    let db = &state.db;
    let order = access::find::<Order>(db, &user, id).await?;
    access::require(
        &user,
        catalogue::ORDERS_REFUND,
        StoreAttr::Operating,
        &order,
    )?;
    abort_unless(
        orders::returnable(&order),
        StatusCode::CONFLICT,
        lang.t(
            "sales.staff.not_returnable",
            &[("days", &RETURN_DAYS.to_string())],
        ),
    )?;
    let items = OrderItem::query()
        .where_(OrderItem::ORDER_ID.eq(order.id))
        .get(db)
        .await?;
    let wanted: HashMap<i64, i64> = form
        .lines
        .iter()
        .filter(|l| l.quantity > 0)
        .map(|l| (l.item_id, l.quantity))
        .collect();
    abort_if(
        wanted.is_empty(),
        StatusCode::UNPROCESSABLE_ENTITY,
        lang.t("sales.staff.nothing_returned", &[]),
    )?;
    let original = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq(Order::TABLE))
        .where_(Payment::PAYABLE_ID.eq(order.id))
        .where_(Payment::STATUS.eq(PaymentStatus::Paid))
        .order_by(Payment::ID)
        .first(db)
        .await?;
    let staff = staff_id(db, &user).await?;

    let mut tx = db.begin().await?;
    let held = ledger::held(&mut tx, order.id).await?;
    let mut refund = 0;
    let mut all_back = true;
    for item in &items {
        let key = (
            item.variant_id,
            item.owner_store_id,
            order.operating_store_id,
        );
        let sold = held.get(&key).map_or(0, |h| h.1);
        let back = wanted.get(&item.id).copied().unwrap_or(0).min(sold).max(0);
        if back > 0 {
            ledger::take_back(&mut tx, &order, item, back, staff, &form.reason).await?;
            refund += item.unit_price * back;
        }
        if sold - back > 0 {
            all_back = false;
        }
    }
    abort_if(
        refund == 0,
        StatusCode::UNPROCESSABLE_ENTITY,
        lang.t("sales.staff.nothing_returned", &[]),
    )?;
    // The discount is given back in proportion; the delivery fee isn't.
    if order.subtotal > 0 && order.discount > 0 {
        refund -= order.discount * refund / order.subtotal;
    }
    Payment {
        customer_id: order.customer_id,
        payable_type: Order::TABLE.into(),
        payable_id: order.id,
        store_id: order.operating_store_id,
        amount: refund,
        method: original.as_ref().map_or(PaymentMethod::Cash, |p| p.method),
        gateway_reference: original.as_ref().and_then(|p| p.gateway_reference.clone()),
        status: PaymentStatus::Refunded,
        paid_at: Some(renox::db::now()),
        received_by: staff,
        ..Default::default()
    }
    .insert(&mut tx)
    .await?;
    let now = renox::db::now();
    let status = if all_back {
        OrderStatus::Refunded
    } else {
        order.status
    };
    Order::query()
        .where_(Order::ID.eq(order.id))
        .update(&mut tx, &[("returned_at", &now), ("status", &status)])
        .await?;
    tx.commit().await?;
    let order = Order::find_or_404(db, order.id).await?;
    notify::tell(&state, &order, Moment::Refunded, Some(refund)).await?;
    Ok(done(
        &htmx,
        Toast::success(lang.t("sales.staff.refunded", &[("number", &order.number)])),
        id,
    ))
}
