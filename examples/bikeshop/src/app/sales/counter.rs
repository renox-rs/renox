//! The counter (point of sale): a cashier sells over the counter in the
//! store they work in (the active store).
//!
//! | Route | Name |
//! |---|---|
//! | `GET /staff/counter` | `sales.counter` (the screen) |
//! | `GET /staff/counter/variants?q=` | `sales.counter.variants` (the product search's options, JSON) |
//! | `GET /staff/counter/customers?q=` | `sales.counter.customers` (the customer search's options, JSON) |
//! | `POST /staff/counter/lines` | `sales.counter.add` |
//! | `PATCH /staff/counter/lines/{variant}` | `sales.counter.update` |
//! | `DELETE /staff/counter/lines/{variant}` | `sales.counter.remove` |
//! | `POST /staff/counter/customer` | `sales.counter.customer` |
//! | `POST /staff/counter/clear` | `sales.counter.clear` |
//! | `POST /staff/counter/pay` | `sales.counter.pay` |
//!
//! The sale being rung up lives in the session, one per store
//! (`counter:{store}`), like a cart. Paying runs the same rules as online:
//! the units are reserved in a transaction (`ledger::reserve`, so the last
//! helmet can't be sold twice, even with the web shop), the payment is
//! recorded through the payments contract (`payments::record_counter`, cash
//! or card), its `PaymentSucceeded` turns the reservation into a sale, and
//! the goods are handed over at once (`completed`). A bike sold to a known
//! customer is registered to them. Every route needs `orders.sell` in the
//! active store.

use renox::prelude::*;
use renox::select::{OptionQuery, SelectOption};
use serde::{Deserialize, Serialize};

use super::cart::{Cart, CartView, MAX_QUANTITY, available_at};
use super::checkout::{self, number, totals};
use super::ledger;
use super::model::{Channel, Fulfilment, Order, OrderItem, OrderStatus, PaymentMethod};
use super::payments::{self, Charge, Payable};
use crate::app::access::active_store;
use crate::app::accounts::model::Customer;
use crate::app::catalog::model::{Product, ProductVariant};
use crate::app::staff::model::{Staff, Store};

/// A sale being rung up.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CounterSale {
    pub cart: Cart,
    pub customer_id: Option<i64>,
}

fn key(store: i64) -> String {
    format!("counter:{store}")
}

/// The active store, or a 403 (the staff routes always set one).
fn store() -> Result<i64> {
    active_store::current().ok_or(Error::Forbidden)
}

// [explain:sales.counter.handler]
fn load(session: &Session, store: i64) -> CounterSale {
    let mut sale: CounterSale = session.get(&key(store)).unwrap_or_default();
    sale.cart.store_id = Some(store);
    sale
}

fn save(session: &Session, store: i64, sale: &CounterSale) -> Result {
    session.put(&key(store), sale)
}
// [/explain:sales.counter.handler]

// [explain:sales.counter.handler]
/// `GET /staff/counter` (`sales.counter`): the screen. With htmx (a change)
/// only its `sale` block.
pub async fn show(State(db): State<Db>, session: Session) -> Result<View> {
    let store_id = store()?;
    let mut sale = load(&session, store_id);
    let data = CartView::load(&db, &mut sale.cart).await?;
    if data.changed {
        save(&session, store_id, &sale)?;
    }
    let customer = match sale.customer_id {
        Some(id) => Customer::find(&db, id).await?,
        None => None,
    };
    let plan = checkout::on_a_plan(&db, sale.customer_id).await?;
    let totals = totals(&data, plan, 0);
    let store = Store::find_or_404(&db, store_id).await?;
    Ok(view(
        "sales/counter/show.html",
        context! {
            sale => data, customer, totals, store,
            // Typed amounts are whole units; the total is in the smallest.
            scale => crate::money::scale(&crate::money::currency()),
        },
    )
    .fragment("sale"))
}
// [/explain:sales.counter.handler]

/// `GET /staff/counter/variants?q=` (`sales.counter.variants`): what the
/// product search offers: an exact SKU (or barcode text) first, then the
/// full-text matches; each with its price and what this store has.
pub async fn variants(
    State(state): State<AppState>,
    lang: Lang,
    query: OptionQuery,
) -> Result<Json<Vec<SelectOption>>> {
    let db = state.db.clone();
    let store_id = store()?;
    let variants: Vec<ProductVariant> = if query.is_lookup() {
        ProductVariant::find_many(&db, query.values_as::<i64>()).await?
    } else if query.q.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
        Vec::new()
    } else {
        let mut found = ProductVariant::query()
            .where_(ProductVariant::SKU.eq(query.q.to_uppercase()))
            .get(&db)
            .await?;
        let products = Product::search(&query.q).limit(10).get(&db).await?;
        let more = ProductVariant::query()
            .where_(
                ProductVariant::PRODUCT_ID.is_in(products.iter().map(|p| p.id).collect::<Vec<_>>()),
            )
            .order_by(ProductVariant::PRODUCT_ID)
            .order_by(ProductVariant::ID)
            .limit(30)
            .get(&db)
            .await?;
        for variant in more {
            if !found.iter().any(|f| f.id == variant.id) {
                found.push(variant);
            }
        }
        found
    };
    let products = Product::find_many(
        &db,
        variants.iter().map(|v| v.product_id).collect::<Vec<_>>(),
    )
    .await?;
    let stock = available_at(
        &db,
        store_id,
        &variants.iter().map(|v| v.id).collect::<Vec<_>>(),
    )
    .await?;
    let money = |amount: i64| crate::money::format(amount, &state.config.currency, &lang.locale);
    Ok(Json(
        variants
            .iter()
            .filter_map(|v| {
                let product = products.iter().find(|p| p.id == v.product_id)?;
                let what = [v.size.clone(), v.colour.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                let left = stock.get(&v.id).copied().unwrap_or(0);
                Some(SelectOption::new(
                    v.id,
                    format!(
                        "{} · {} {} · {} · {}",
                        v.sku,
                        product.name,
                        what,
                        money(v.price),
                        lang.t("sales.counter.here", &[("count", &left.to_string())])
                    ),
                ))
            })
            .collect(),
    ))
}

/// `GET /staff/counter/customers?q=` (`sales.counter.customers`): customers
/// by name, email or phone.
pub async fn customers(
    State(db): State<Db>,
    query: OptionQuery,
) -> Result<Json<Vec<SelectOption>>> {
    let found = if query.is_lookup() {
        Customer::find_many(&db, query.values_as::<i64>()).await?
    } else if query.q.chars().count() < 2 {
        Vec::new()
    } else {
        let like = format!("%{}%", query.q);
        Customer::query()
            .where_any(|q| {
                q.where_(Customer::NAME.like(&like))
                    .where_(Customer::EMAIL.like(&like))
                    .where_(Customer::PHONE.like(&like))
            })
            .order_by(Customer::NAME)
            .limit(20)
            .get(&db)
            .await?
    };
    Ok(Json(
        found
            .iter()
            .map(|c| {
                let contact = c.email.clone().or(c.phone.clone()).unwrap_or_default();
                SelectOption::new(c.id, format!("{} · {contact}", c.name))
            })
            .collect(),
    ))
}

/// A line rung up.
#[derive(Deserialize, Validate, Debug)]
pub struct LineForm {
    #[validate(required, exists("product_variants", "id"))]
    pub variant_id: Option<i64>,
    #[validate(between(1, 20))]
    pub quantity: i64,
}

/// `POST /staff/counter/lines` (`sales.counter.add`): adds a product, up to
/// what the store has.
pub async fn add(
    State(db): State<Db>,
    session: Session,
    lang: Lang,
    Valid(form): Valid<LineForm>,
) -> Result<(Toast, Redirect)> {
    let store_id = store()?;
    let variant = form.variant_id.unwrap_or_default();
    let mut sale = load(&session, store_id);
    let left = available_at(&db, store_id, &[variant])
        .await?
        .get(&variant)
        .copied()
        .unwrap_or(0);
    let in_sale = sale
        .cart
        .lines
        .iter()
        .find(|l| l.variant_id == variant)
        .map_or(0, |l| l.quantity);
    let added = form.quantity.min(left - in_sale).clamp(0, MAX_QUANTITY);
    let toast = if added == 0 {
        Toast::warning(lang.t("sales.counter.none_left", &[]))
    } else {
        sale.cart.add(variant, added);
        save(&session, store_id, &sale)?;
        Toast::success(lang.t("sales.counter.added", &[("count", &added.to_string())]))
    };
    Ok((toast, Redirect::to("/staff/counter")))
}

/// A new quantity.
#[derive(Deserialize, Validate, Debug)]
pub struct QuantityForm {
    #[validate(between(0, 20))]
    pub quantity: i64,
}

/// `PATCH /staff/counter/lines/{variant}` (`sales.counter.update`).
pub async fn update(
    session: Session,
    Path(variant): Path<i64>,
    Valid(form): Valid<QuantityForm>,
) -> Result<Redirect> {
    let store_id = store()?;
    let mut sale = load(&session, store_id);
    sale.cart.set(variant, form.quantity);
    save(&session, store_id, &sale)?;
    Ok(Redirect::to("/staff/counter"))
}

/// `DELETE /staff/counter/lines/{variant}` (`sales.counter.remove`).
pub async fn remove(session: Session, Path(variant): Path<i64>) -> Result<Redirect> {
    let store_id = store()?;
    let mut sale = load(&session, store_id);
    sale.cart.set(variant, 0);
    save(&session, store_id, &sale)?;
    Ok(Redirect::to("/staff/counter"))
}

/// The customer of the sale (empty: a walk-in).
#[derive(Deserialize, Validate, Debug)]
pub struct CustomerForm {
    #[serde(default)]
    #[validate(exists("customers", "id"))]
    pub customer_id: Option<i64>,
}

/// `POST /staff/counter/customer` (`sales.counter.customer`).
pub async fn customer(session: Session, Valid(form): Valid<CustomerForm>) -> Result<Redirect> {
    let store_id = store()?;
    let mut sale = load(&session, store_id);
    sale.customer_id = form.customer_id;
    save(&session, store_id, &sale)?;
    Ok(Redirect::to("/staff/counter"))
}

/// `POST /staff/counter/clear` (`sales.counter.clear`): starts again.
pub async fn clear(session: Session) -> Result<Redirect> {
    let store_id = store()?;
    session.remove(&key(store_id));
    Ok(Redirect::to("/staff/counter"))
}

/// How the customer pays.
#[derive(Deserialize, Validate, Debug)]
pub struct PayForm {
    #[validate(required, one_of(&["cash", "card"]))]
    pub method: String,
    /// What the customer handed over, for cash (the change is worked
    /// out), in whole units as typed: `50`, `50.00` or `50,00`
    /// ([`crate::money::parse`]).
    #[serde(default)]
    #[validate(max = 20)]
    pub tendered: Option<String>,
}

/// `POST /staff/counter/pay` (`sales.counter.pay`): rings the sale up (see
/// the module docs) and shows the receipt.
pub async fn pay(
    State(state): State<AppState>,
    session: Session,
    user: AuthUser,
    lang: Lang,
    Valid(form): Valid<PayForm>,
) -> Result<Response> {
    let db = &state.db;
    let store_id = store()?;
    let mut sale = load(&session, store_id);
    let data = CartView::load(db, &mut sale.cart).await?;
    if data.lines.is_empty() || data.changed {
        save(&session, store_id, &sale)?;
        return Ok((
            Toast::warning(lang.t("sales.counter.changed", &[])),
            Redirect::to("/staff/counter"),
        )
            .into_response());
    }
    let plan = checkout::on_a_plan(db, sale.customer_id).await?;
    let totals = totals(&data, plan, 0);
    let method = if form.method == "card" {
        PaymentMethod::Card
    } else {
        PaymentMethod::Cash
    };
    let tendered = match form.tendered.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => {
            let Some(amount) = crate::money::parse(text, &state.config.currency) else {
                let mut errors = renox::validation::Errors::default();
                errors.add("tendered", lang.t("sales.counter.not_an_amount", &[]));
                return Err(errors.into());
            };
            Some(amount)
        }
        _ => None,
    };
    // [explain:sales.counter.pay]
    if method == PaymentMethod::Cash && tendered.is_some_and(|t| t < totals.total) {
        let mut errors = renox::validation::Errors::default();
        errors.add("tendered", lang.t("sales.counter.not_enough", &[]));
        return Err(errors.into());
    }
    // [/explain:sales.counter.pay]
    // Counter sales are taken by a member of staff (the ledger and the
    // payment name them).
    let staff_id = Staff::of_user(db, user.id)
        .await?
        .ok_or(Error::Forbidden)?
        .id;
    let staff = Some(staff_id);
    let lines: Vec<(i64, i64)> = data
        .lines
        .iter()
        .map(|l| (l.variant.id, l.quantity))
        .collect();
    let prices: std::collections::HashMap<i64, i64> = data
        .lines
        .iter()
        .map(|l| (l.variant.id, l.variant.price))
        .collect();
    let levels =
        ledger::levels_at(db, store_id, &lines.iter().map(|l| l.0).collect::<Vec<_>>()).await?;
    let number = number(&state, store_id).await?;
    let now = renox::db::now();

    let mut tx = db.begin().await?;
    let mut order = Order {
        number,
        customer_id: sale.customer_id,
        operating_store_id: store_id,
        channel: Channel::Counter,
        fulfilment: Fulfilment::Pickup,
        status: OrderStatus::Pending,
        subtotal: totals.subtotal,
        discount: totals.discount,
        total: totals.total,
        placed_at: Some(now),
        served_by: staff,
        locale: Some(lang.locale.clone()),
        ..Default::default()
    };
    order.insert(&mut tx).await?;
    let taken = match ledger::reserve(&mut tx, order.id, store_id, &lines, &levels, staff).await? {
        Ok(taken) => taken,
        Err(_) => {
            tx.rollback().await?;
            return Ok((
                Toast::error(lang.t("sales.counter.changed", &[])),
                Redirect::to("/staff/counter"),
            )
                .into_response());
        }
    };
    let items: Vec<OrderItem> = taken
        .iter()
        .map(|t| {
            let price = prices.get(&t.variant_id).copied().unwrap_or(0);
            OrderItem {
                order_id: order.id,
                variant_id: t.variant_id,
                owner_store_id: t.owner_store_id,
                quantity: t.quantity,
                unit_price: price,
                total: price * t.quantity,
                ..Default::default()
            }
        })
        .collect();
    OrderItem::insert_many(&mut tx, items).await?;
    tx.commit().await?;

    // [explain:sales.counter.pay]
    // Paid at once; its PaymentSucceeded turns the reservation into a sale.
    payments::record_counter(
        &state,
        Charge {
            payable: Payable::Order(order.id),
            customer_id: order.customer_id,
            store_id,
            amount: order.total,
        },
        method,
        staff_id,
    )
    .await?;
    // Handed over across the counter.
    Order::query()
        .where_(Order::ID.eq(order.id))
        .update(
            db,
            &[("status", &OrderStatus::Completed), ("completed_at", &now)],
        )
        .await?;
    session.remove(&key(store_id));
    if let Some(tendered) = tendered.filter(|_| method == PaymentMethod::Cash) {
        session.flash("change", tendered - order.total)?;
    }
    Ok((
        Toast::success(lang.t("sales.counter.sold", &[("number", &order.number)])),
        Redirect::to(&format!("/orders/{}/invoice?receipt=1", order.id)),
    )
        .into_response())
    // [/explain:sales.counter.pay]
}
