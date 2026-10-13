//! The checkout: `GET /checkout` (`checkout.show`), the kit's `wizard`
//! (contact → pickup or delivery → review → pay), and `POST /checkout`
//! (`checkout.place`), which places the order and sends the customer to the
//! gateway's page.
//!
//! Placing an order, in one transaction:
//!
//! 1. the order row (`pending`, online, the customer's language);
//! 2. every line's units reserved at the store (`ledger::reserve`: a
//!    conditional update per stock level, so two customers can't both get
//!    the last helmet: the second gets zero rows changed, a `Shortfall`,
//!    and a rollback);
//! 3. the order's lines, one per owner of the goods (consigned goods are
//!    another store's, #245).
//!
//! Then `payments::start` records the pending payment and gives the
//! gateway's page; the cart is emptied. Unpaid, the order expires after 30
//! minutes (`orders::expire`, a scheduled task) and the units are free again.
//!
//! The form is `#[derive(Validate)]` with hooks (`prepare` tidies the
//! input, `after` checks the phone number), validated live while it is
//! filled in (`data-live-validate`: each field is checked by the same
//! rules, with `X-Renox-Validate`), and sent back with the old input after
//! a failed plain submit.

use std::collections::HashMap;

use renox::db::relations::belongs_to;
use renox::prelude::*;
use renox::validation::{Errors, FormContext, ValidateHooks};
use serde::{Deserialize, Serialize};

use super::cart::{Cart, CartView};
use super::gateway;
use super::ledger::{self, Shortfall};
use super::model::{Channel, Fulfilment, Order, OrderItem, OrderStatus};
use super::orders;
use super::payments::{self, Charge, Payable};
use crate::app::accounts::model::{Address, City, Country, Customer};
use crate::app::catalog::model::{Product, ProductVariant};
use crate::app::staff::model::{Store, fee};

/// Delivery in a city with one of our stores.
pub const DELIVERY_LOCAL: i64 = 1_000;
/// Delivery elsewhere in the same country.
pub const DELIVERY_COUNTRY: i64 = 2_500;
/// Delivery abroad.
pub const DELIVERY_ABROAD: i64 = 6_000;

// [explain:checkout.show.form]
/// The checkout form: four steps of one form.
#[derive(Deserialize, Serialize, Validate, Debug, Clone, Default)]
#[validate(hooks)]
pub struct CheckoutForm {
    #[validate(required, max = 100)]
    pub name: String,
    #[validate(required, email, max = 150)]
    pub email: String,
    #[validate(required, min = 6, max = 30)]
    pub phone: String,
    #[validate(required, one_of(&["pickup", "delivery"]))]
    pub fulfilment: String,
    /// The store it is picked up from, or sent from (the cart's store).
    #[serde(default)]
    #[validate(required, exists("stores", "id"))]
    pub store_id: Option<i64>,
    #[serde(default)]
    #[validate(required_if(self.fulfilment == "delivery"), exists("cities", "id"))]
    pub city_id: Option<i64>,
    #[serde(default)]
    #[validate(required_if(self.fulfilment == "delivery"), max = 200)]
    pub line1: Option<String>,
    #[serde(default)]
    #[validate(max = 20)]
    pub postal_code: Option<String>,
}
// [/explain:checkout.show.form]

impl ValidateHooks for CheckoutForm {
    /// Tidies what was typed before the rules run.
    fn prepare(&mut self) {
        self.name = self.name.split_whitespace().collect::<Vec<_>>().join(" ");
        self.email = self.email.trim().to_lowercase();
        self.phone = self.phone.trim().to_owned();
        for field in [&mut self.line1, &mut self.postal_code] {
            *field = field
                .take()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty());
        }
    }

    /// A phone number is digits, with spaces, dashes and a leading `+`.
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let digits = self.phone.chars().filter(char::is_ascii_digit).count();
        let tidy = self
            .phone
            .chars()
            .enumerate()
            .all(|(i, c)| c.is_ascii_digit() || c == ' ' || c == '-' || (i == 0 && c == '+'));
        if !tidy || digits < 6 {
            errors.add(
                "phone",
                form.state
                    .current_lang()
                    .t("sales.checkout.phone_digits", &[]),
            );
        }
        Ok(())
    }
}

/// The money of an order before it is placed.
#[derive(Serialize, Debug, Clone, Default)]
pub struct Totals {
    pub subtotal: i64,
    pub discount: i64,
    pub delivery_fee: i64,
    pub total: i64,
    /// Whether the plan discount applied.
    pub plan_discount: bool,
    /// The plan's parts discount, in basis points (1000 = 10 %).
    pub discount_bp: i64,
    /// The same as people write it: `10`, `7.5`.
    pub discount_percent: String,
}

/// What delivery to a city costs: in a city with a store, elsewhere in a
/// country with a store, or abroad. Three queries.
pub async fn delivery_fee(db: &Db, city_id: i64) -> Result<i64> {
    let Some(city) = City::find(db, city_id).await? else {
        return Ok(DELIVERY_ABROAD);
    };
    let addresses =
        Address::find_many(db, Store::query().pluck::<i64, _>(db, "address_id").await?).await?;
    let store_cities = belongs_to::<City, _, _>(db, &addresses, |a| a.city_id).await?;
    Ok(if store_cities.contains_key(&city.id) {
        DELIVERY_LOCAL
    } else if store_cities
        .values()
        .any(|c| c.country_id == city.country_id)
    {
        DELIVERY_COUNTRY
    } else {
        DELIVERY_ABROAD
    })
}

/// The parts discount of the customer's service plan, in basis points
/// (1000 = 10 %; 0 without a plan): the subscriber's plan decides (#237).
pub async fn on_a_plan(db: &Db, customer_id: Option<i64>) -> Result<i64> {
    crate::app::plans::parts_discount_bp(db, customer_id).await
}

/// The totals for a cart: its lines, the plan discount on parts, and the
/// delivery fee.
pub fn totals(cart: &CartView, discount_bp: i64, delivery_fee: i64) -> Totals {
    let subtotal = cart.subtotal;
    let parts: i64 = cart.lines.iter().filter(|l| l.part).map(|l| l.total).sum();
    let discount = fee(parts, discount_bp.max(0));
    Totals {
        subtotal,
        discount,
        delivery_fee,
        total: subtotal - discount + delivery_fee,
        plan_discount: discount > 0,
        discount_bp,
        discount_percent: crate::app::rentals::counter::percent(discount_bp),
    }
}

/// The customer of a logged-in user, if they have one yet.
async fn customer_of(db: &Db, user: Option<&User>) -> Result<Option<Customer>> {
    match user {
        Some(user) => Customer::of_user(db, user.id).await,
        None => Ok(None),
    }
}

/// The city options, "City, Country", by country then city (two queries).
async fn city_options(db: &Db) -> Result<Vec<(String, String)>> {
    let cities = City::query().order_by(City::NAME).get(db).await?;
    let countries: HashMap<i64, Country> =
        belongs_to::<Country, _, _>(db, &cities, |c| c.country_id).await?;
    let mut options: Vec<(String, String, String)> = cities
        .into_iter()
        .map(|c| {
            let country = countries
                .get(&c.country_id)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            (
                country.clone(),
                c.id.to_string(),
                format!("{}, {country}", c.name),
            )
        })
        .collect();
    options.sort();
    Ok(options
        .into_iter()
        .map(|(_, id, label)| (id, label))
        .collect())
}

/// The review step's preview values, read from the query (htmx sends the
/// form's fields as they change).
#[derive(Deserialize, Default, Debug)]
pub struct Preview {
    #[serde(default)]
    pub fulfilment: Option<String>,
    #[serde(default)]
    pub city_id: Option<String>,
}

/// `GET /checkout` (`checkout.show`): the wizard. With htmx (a change of
/// pickup / delivery or city) only the `summary` block, with the delivery
/// fee for that city.
pub async fn show(
    State(db): State<Db>,
    session: Session,
    user: Option<AuthUser>,
    lang: Lang,
    Query(preview): Query<Preview>,
) -> Result<Response> {
    let user = user.as_deref();
    let mut cart = Cart::load(&db, &session, user).await?;
    let data = CartView::load(&db, &mut cart).await?;
    if data.changed {
        cart.save(&db, &session, user).await?;
    }
    if data.lines.is_empty() {
        return Ok((
            Toast::info(lang.t("sales.checkout.empty", &[])),
            Redirect::to("/cart"),
        )
            .into_response());
    }
    let customer = customer_of(&db, user).await?;
    let address = match customer.as_ref().and_then(|c| c.address_id) {
        Some(id) => Address::find(&db, id).await?,
        None => None,
    };
    let delivery = preview.fulfilment.as_deref() == Some("delivery");
    let city = preview
        .city_id
        .as_deref()
        .and_then(|c| c.parse::<i64>().ok())
        .or(address.as_ref().map(|a| a.city_id));
    let fee = match (delivery, city) {
        (true, Some(city)) => delivery_fee(&db, city).await?,
        _ => 0,
    };
    let plan = on_a_plan(&db, customer.as_ref().map(|c| c.id)).await?;
    let totals = totals(&data, plan, fee);
    let prefill = CheckoutForm {
        name: customer
            .as_ref()
            .map(|c| c.name.clone())
            .or(user.map(|u| u.name.clone()))
            .unwrap_or_default(),
        email: customer
            .as_ref()
            .and_then(|c| c.email.clone())
            .or(user.map(|u| u.email.clone()))
            .unwrap_or_default(),
        phone: customer
            .as_ref()
            .and_then(|c| c.phone.clone())
            .unwrap_or_default(),
        fulfilment: if delivery { "delivery" } else { "pickup" }.into(),
        store_id: data.store.as_ref().map(|s| s.id),
        city_id: address.as_ref().map(|a| a.city_id),
        line1: address.as_ref().map(|a| a.line1.clone()),
        postal_code: address.as_ref().and_then(|a| a.postal_code.clone()),
    };
    let stores: Vec<(String, String)> = data
        .stores
        .iter()
        .map(|s| (s.id.to_string(), s.name.clone()))
        .collect();
    let cities = city_options(&db).await?;
    let fees = [DELIVERY_LOCAL, DELIVERY_COUNTRY, DELIVERY_ABROAD];
    Ok(view(
        "sales/checkout/show.html",
        context! { cart => data, totals, prefill, stores, cities, delivery, fees, plan,
        discount_percent => crate::app::rentals::counter::percent(plan) },
    )
    .fragment("summary")
    .into_response())
}

/// `POST /checkout` (`checkout.place`): places the order (see the module
/// docs) and sends the customer to the gateway's page.
// [explain:checkout.show.place]
pub async fn place(
    State(state): State<AppState>,
    session: Session,
    user: Option<AuthUser>,
    lang: Lang,
    Valid(form): Valid<CheckoutForm>,
) -> Result<Response> {
    // [/explain:checkout.show.place]
    let db = &state.db;
    let user = user.as_deref();
    let mut cart = Cart::load(db, &session, user).await?;
    let store_id = form.store_id.unwrap_or_default();
    if cart.store_id != Some(store_id) {
        cart.store_id = Some(store_id); // the store chosen in the wizard
    }
    let data = CartView::load(db, &mut cart).await?;
    if data.changed || data.lines.is_empty() {
        cart.save(db, &session, user).await?;
        return Ok((
            Toast::warning(lang.t("sales.checkout.cart_changed", &[])),
            Redirect::to("/cart"),
        )
            .into_response());
    }

    // The customer: the account's, or one made (or found) for a guest.
    let mut customer = match customer_of(db, user).await? {
        Some(c) => c,
        None => match user {
            None => Customer::query()
                .where_(Customer::EMAIL.eq(form.email.as_str()))
                .where_(Customer::USER_ID.is_null())
                .first(db)
                .await?
                .unwrap_or_default(),
            Some(_) => Customer::default(),
        },
    };
    customer.user_id = user.map(|u| u.id).or(customer.user_id);
    customer.name = form.name.clone();
    customer.email = Some(form.email.clone());
    customer.phone = Some(form.phone.clone());
    customer.active = true;
    let delivery = form.fulfilment == "delivery";
    let address_id = if delivery {
        let address = Address::create(
            db,
            Address {
                city_id: form.city_id.unwrap_or_default(),
                line1: form.line1.clone().unwrap_or_default(),
                postal_code: form.postal_code.clone(),
                ..Default::default()
            },
        )
        .await?;
        customer.address_id.get_or_insert(address.id);
        Some(address.id)
    } else {
        None
    };
    customer.save(db).await?;

    let plan = on_a_plan(db, Some(customer.id)).await?;
    let fee = match (delivery, form.city_id) {
        (true, Some(city)) => delivery_fee(db, city).await?,
        _ => 0,
    };
    let totals = totals(&data, plan, fee);
    let lines: Vec<(i64, i64)> = data
        .lines
        .iter()
        .map(|l| (l.variant.id, l.quantity))
        .collect();
    let prices: HashMap<i64, i64> = data
        .lines
        .iter()
        .map(|l| (l.variant.id, l.variant.price))
        .collect();
    // Read before the transaction, so its first statement is a write.
    let levels =
        ledger::levels_at(db, store_id, &lines.iter().map(|l| l.0).collect::<Vec<_>>()).await?;

    // [explain:checkout.show.place]
    let now = renox::db::now();
    let number = number(&state, store_id).await?;
    let mut tx = db.begin().await?;
    // [/explain:checkout.show.place]
    let mut order = Order {
        number,
        customer_id: Some(customer.id),
        operating_store_id: store_id,
        channel: Channel::Online,
        fulfilment: if delivery {
            Fulfilment::Delivery
        } else {
            Fulfilment::Pickup
        },
        status: OrderStatus::Pending,
        subtotal: totals.subtotal,
        discount: totals.discount,
        delivery_fee: totals.delivery_fee,
        total: totals.total,
        delivery_address_id: address_id,
        placed_at: Some(now),
        locale: Some(lang.locale.clone()),
        ..Default::default()
    };
    // [explain:checkout.show.place]
    order.insert(&mut tx).await?;
    let taken = match ledger::reserve(&mut tx, order.id, store_id, &lines, &levels, None).await? {
        Ok(taken) => taken,
        Err(short) => {
            tx.rollback().await?;
            return Ok(sold_out(db, &lang, short).await?.into_response());
        }
    };
    // [/explain:checkout.show.place]
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
    // [explain:checkout.show.place]
    OrderItem::insert_many(&mut tx, items).await?;
    tx.commit().await?;

    Cart::clear(db, &session, user).await?;
    // [/explain:checkout.show.place]
    orders::remember(&session, order.id)?;
    let checkout = payments::start(
        &state,
        Charge {
            payable: Payable::Order(order.id),
            customer_id: Some(customer.id),
            store_id,
            amount: order.total,
        },
    )
    .await?;
    gateway::remember(&session, checkout.payment_id)?;
    Ok(Redirect::to(&checkout.redirect_url).into_response())
}

/// The answer when someone else was quicker: back to the cart (which now
/// shows what's left), saying which product ran out.
async fn sold_out(db: &Db, lang: &Lang, short: Shortfall) -> Result<(Toast, Redirect)> {
    let name = match ProductVariant::find(db, short.variant_id).await? {
        Some(v) => Product::find(db, v.product_id)
            .await?
            .map(|p| p.name)
            .unwrap_or_default(),
        None => String::new(),
    };
    Ok((
        Toast::error(lang.t(
            "sales.checkout.sold_out",
            &[("name", &name), ("count", &short.left.to_string())],
        ))
        .persistent(),
        Redirect::to("/cart"),
    ))
}

/// A new order number: the store's initial and eight characters of a ULID
/// (`N-7XK2M9QD`), unique without asking the database for the last one.
pub async fn number(state: &AppState, store_id: i64) -> Result<String> {
    let initial = Store::find(&state.db, store_id)
        .await?
        .and_then(|s| s.name.chars().next())
        .unwrap_or('S')
        .to_ascii_uppercase();
    let ulid = renox::db::Ulid::new().to_string();
    Ok(format!("{initial}-{}", &ulid[ulid.len() - 8..]))
}
