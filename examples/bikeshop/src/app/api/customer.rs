//! The customer API (`/api/v1/…`) for the shop's mobile app: the
//! catalogue, and the customer's own rentals, orders, bikes, work orders
//! and plans, with a personal token made on `/account/api-tokens`.
//!
//! Abilities: `read` (the catalogue, bikes, work orders, plans, who I am),
//! `rent` (my rentals: list, reserve, cancel), `order` (my orders). A token
//! without the endpoint's ability gets 403, no token 401.
//!
//! The rules are the website's: reserving is `Valid<ReserveForm>` (the
//! period's rules and the overlap check in its `after` hook), the ID check
//! (`identity::submitted`) and `booking::book` (the overlap checked again
//! in its transaction); cancelling is `reserve::cancel_rental`. Lists are
//! paginated (`?page=`) with `links` to the other pages.

use renox::db::Paginated;
use renox::prelude::*;
use renox::serde_json::Value;
use serde::{Deserialize, Serialize};

use super::kiosk::rental_json;
use crate::app::catalog::model::{Product, ProductCard, ProductVariant};
use crate::app::plans::model::{PlanSubscription, ServicePlan};
use crate::app::rentals::booking::{self, NewRental};
use crate::app::rentals::customer_of;
use crate::app::rentals::identity;
use crate::app::rentals::model::Rental;
use crate::app::rentals::reserve::{self, ReserveForm};
use crate::app::sales::model::Order;
use crate::app::staff::model::Store;
use crate::app::stock::model::StockLevel;
use crate::app::workshop::model::{CustomerBike, WorkOrder};

/// Rows per page.
pub const PER_PAGE: u32 = 20;

/// A page as JSON: `data`, `meta` (page, per page, total, last page) and
/// `links` (first, last, prev, next: absolute URLs, `null` at the ends).
/// `url(n)` makes the address of page `n`.
pub fn page_json<T: Serialize>(page: &Paginated<T>, url: impl Fn(u32) -> String) -> Value {
    json!({
        "data": page.items,
        "meta": {
            "page": page.page,
            "per_page": page.per_page,
            "total": page.total,
            "last_page": page.last_page,
        },
        "links": {
            "first": url(1),
            "last": url(page.last_page),
            "prev": page.has_prev.then(|| url(page.page - 1)),
            "next": page.has_next.then(|| url(page.page + 1)),
        },
    })
}

/// The absolute address of the route `name` with `query` and `page`.
fn page_url(state: &AppState, name: &str, query: &[(&str, &str)], page: u32) -> String {
    let base = state.absolute_url(name, &[]).unwrap_or_default();
    let mut pairs: Vec<String> = query
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect();
    pairs.push(format!("page={page}"));
    format!("{base}?{}", pairs.join("&"))
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `GET /api/v1/me` (`api.me`, `read`): who the token belongs to and what
/// it may do.
pub async fn me(State(db): State<Db>, user: AuthUser) -> Result<Json<Value>> {
    let customer = customer_of(&db, &user).await?;
    let abilities: Vec<&str> = super::CUSTOMER_ABILITIES
        .iter()
        .map(|(a, _)| *a)
        .filter(|a| user.token_can(a))
        .collect();
    Ok(Json(json!({
        "data": {
            "name": customer.name,
            "email": user.email,
            "verified_for_rentals": customer.id_verified(),
            "abilities": abilities,
        }
    })))
}

/// The catalogue's query: `?q=` searches (the same full-text index as the
/// website), `?page=`.
#[derive(Deserialize, Default, Debug)]
pub struct CatalogueQuery {
    #[serde(default)]
    pub q: String,
    pub page: Option<u32>,
}

/// A product in a list.
#[derive(Serialize, Debug, Clone)]
pub struct ProductJson {
    pub slug: String,
    pub name: String,
    pub brand: Option<String>,
    pub category: Option<String>,
    pub price_from: Option<i64>,
    pub url: String,
}

/// `GET /api/v1/products` (`api.products`, `read`): the catalogue, or what
/// `?q=` finds, a page at a time (four queries: the page, its count,
/// brands and categories and variants for the cards).
pub async fn products(
    State(state): State<AppState>,
    Query(query): Query<CatalogueQuery>,
) -> Result<Json<Value>> {
    let db = &state.db;
    let q = query.q.trim().to_owned();
    let found = if q.is_empty() {
        Product::query().order_by(Product::NAME)
    } else {
        Product::search(&q)
    };
    let page = found
        .paginate(db, query.page.unwrap_or(1), PER_PAGE)
        .await?;
    let cards = ProductCard::load(db, page.items.clone()).await?;
    let items: Vec<ProductJson> = cards
        .into_iter()
        .map(|c| ProductJson {
            url: state
                .absolute_url("api.products.show", &[&c.product.slug])
                .unwrap_or_default(),
            slug: c.product.slug,
            name: c.product.name,
            brand: c.brand.map(|b| b.name),
            category: c.category.map(|c| c.name),
            price_from: c.price_from,
        })
        .collect();
    let page = Paginated::new(items, page.page, page.per_page, page.total);
    Ok(Json(page_json(&page, |n| {
        page_url(&state, "api.products", &[("q", &q)], n)
    })))
}

/// `GET /api/v1/products/{slug}` (`api.products.show`, `read`): a product,
/// its variants, and how many of each are available at each store.
pub async fn product(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>> {
    let db = &state.db;
    let product = Product::query()
        .where_(Product::SLUG.eq(slug.as_str()))
        .first(db)
        .await?
        .ok_or(Error::NotFound)?;
    let card = ProductCard::load(db, vec![product])
        .await?
        .pop()
        .ok_or(Error::NotFound)?;
    let ids: Vec<i64> = card.variants.iter().map(|v| v.id).collect();
    let levels = StockLevel::query()
        .where_(StockLevel::VARIANT_ID.is_in(ids))
        .get(db)
        .await?;
    let stores = Store::all_by_name(db).await?;
    let variants: Vec<Value> = card
        .variants
        .iter()
        .map(|v: &ProductVariant| {
            let stock: Vec<Value> = stores
                .iter()
                .map(|s| {
                    let available: i64 = levels
                        .iter()
                        .filter(|l| l.variant_id == v.id && l.location_store_id == s.id)
                        .map(|l| l.available())
                        .sum();
                    json!({ "store_id": s.id, "store": s.name, "available": available.max(0) })
                })
                .collect();
            json!({
                "sku": v.sku,
                "size": v.size,
                "colour": v.colour,
                "price": v.price,
                "stock": stock,
            })
        })
        .collect();
    Ok(Json(json!({
        "data": {
            "slug": card.product.slug,
            "name": card.product.name,
            "description": card.product.description,
            "brand": card.brand.map(|b| b.name),
            "category": card.category.map(|c| c.name),
            "price_from": card.price_from,
            "variants": variants,
        }
    })))
}

/// `GET /api/v1/me/bikes` (`api.me.bikes`, `read`): the customer's bikes.
pub async fn bikes(State(db): State<Db>, user: AuthUser) -> Result<Json<Value>> {
    let customer = customer_of(&db, &user).await?;
    let bikes = CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .order_by(CustomerBike::NAME)
        .get(&db)
        .await?;
    let data: Vec<Value> = bikes
        .iter()
        .map(|b| json!({ "id": b.id, "name": b.name, "brand": b.brand, "size": b.size, "frame_number": b.frame_number }))
        .collect();
    Ok(Json(json!({ "data": data })))
}

/// `?page=` for the customer's lists.
#[derive(Deserialize, Default, Debug)]
pub struct PageQuery {
    pub page: Option<u32>,
}

/// `GET /api/v1/me/work-orders` (`api.me.work_orders`, `read`): the work
/// orders on the customer's bikes, newest first, with their status.
pub async fn work_orders(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<PageQuery>,
) -> Result<Json<Value>> {
    let db = &state.db;
    let customer = customer_of(db, &user).await?;
    let bikes: Vec<i64> = CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .pluck(db, CustomerBike::ID)
        .await?;
    let page = WorkOrder::query()
        .where_(WorkOrder::CUSTOMER_BIKE_ID.is_in(bikes))
        .order_by_desc(WorkOrder::SCHEDULED_FOR)
        .paginate(db, query.page.unwrap_or(1), PER_PAGE)
        .await?;
    let page = page.map(|o| {
        json!({
            "number": o.id,
            "bike_id": o.customer_bike_id,
            "store_id": o.store_id,
            "source": o.source.as_str(),
            "status": o.status.as_str(),
            "scheduled_for": o.scheduled_for,
            "total": o.total,
            "paid": o.paid_at.is_some(),
        })
    });
    Ok(Json(page_json(&page, |n| {
        page_url(&state, "api.me.work_orders", &[], n)
    })))
}

/// `GET /api/v1/me/plan` (`api.me.plan`, `read`): the plans on the
/// customer's bikes, with the next visit.
pub async fn plans(State(db): State<Db>, user: AuthUser) -> Result<Json<Value>> {
    let customer = customer_of(&db, &user).await?;
    let bikes: Vec<i64> = CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .pluck(&db, CustomerBike::ID)
        .await?;
    let subs = PlanSubscription::query()
        .where_(PlanSubscription::CUSTOMER_BIKE_ID.is_in(bikes))
        .order_by_desc(PlanSubscription::ID)
        .get(&db)
        .await?;
    let plans = ServicePlan::find_many(
        &db,
        subs.iter().map(|s| s.service_plan_id).collect::<Vec<_>>(),
    )
    .await?;
    let data: Vec<Value> = subs
        .iter()
        .map(|s| {
            let plan = plans.iter().find(|p| p.id == s.service_plan_id);
            json!({
                "bike_id": s.customer_bike_id,
                "plan": plan.map(|p| p.name.clone()),
                "status": crate::app::plans::mine::shown_status(s),
                "store_id": s.store_id,
                "next_visit_on": s.next_visit_on,
                "monthly_price": plan.map(|p| p.monthly_price()),
            })
        })
        .collect();
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/v1/me/rentals` (`api.me.rentals`, `rent`): the customer's
/// rentals, newest first.
pub async fn rentals(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<PageQuery>,
) -> Result<Json<Value>> {
    let db = &state.db;
    let customer = customer_of(db, &user).await?;
    let page = Rental::query()
        .where_(Rental::CUSTOMER_ID.eq(customer.id))
        .order_by_desc(Rental::STARTS_AT)
        .paginate(db, query.page.unwrap_or(1), PER_PAGE)
        .await?;
    let page = page.map(|r| rental_json(&state, &r));
    Ok(Json(page_json(&page, |n| {
        page_url(&state, "api.me.rentals", &[], n)
    })))
}

/// `POST /api/v1/me/rentals` (`api.me.rentals.store`, `rent`): reserves a
/// bike, with the website's rules: the same `ReserveForm` (`store`, `bike`,
/// `starts_at` and `ends_at` as local `YYYY-MM-DDTHH:MM:SS`), the ID check,
/// and `booking::book`. A refusal is a 422 with the field's message.
pub async fn reserve(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<ReserveForm>,
) -> Result<(StatusCode, Json<Value>)> {
    let lang = state.current_lang();
    let customer = customer_of(&state.db, &user).await?;
    if !identity::submitted(&state.db, &customer).await? {
        let mut errors = Errors::new();
        errors.add("identity", lang.t("rentals.identity.needed", &[]));
        return Err(errors.into());
    }
    let (start, end) = form.period(&state.config);
    let booked = booking::book(
        &state.db,
        NewRental {
            bike_id: form.bike,
            customer_id: customer.id,
            operating_store_id: form.store,
            start,
            end,
            served_by: None,
        },
    )
    .await?;
    match booked {
        Ok(rental) => Ok((
            StatusCode::CREATED,
            Json(json!({ "data": rental_json(&state, &rental) })),
        )),
        Err(refusal) => {
            let mut errors = Errors::new();
            errors.add("bike", lang.t(refusal.key(), &[]));
            Err(errors.into())
        }
    }
}

/// `DELETE /api/v1/me/rentals/{code}` (`api.me.rentals.destroy`, `rent`):
/// cancels a reservation (`reserve::cancel_rental`: until an hour before;
/// 409 after).
pub async fn cancel(
    State(state): State<AppState>,
    user: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<Value>> {
    let (customer, mut rental) = reserve::own_rental(&state.db, &user, &code).await?;
    if !reserve::cancel_rental(&state, &customer, &mut rental).await? {
        return Err(abort(
            StatusCode::CONFLICT,
            state.current_lang().t("rentals.show.too_late", &[]),
        ));
    }
    Ok(Json(json!({ "data": rental_json(&state, &rental) })))
}

/// `GET /api/v1/me/orders` (`api.me.orders`, `order`): the customer's
/// orders, newest first.
pub async fn orders(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<PageQuery>,
) -> Result<Json<Value>> {
    let db = &state.db;
    let customer = customer_of(db, &user).await?;
    let page = Order::query()
        .where_(Order::CUSTOMER_ID.eq(customer.id))
        .order_by_desc(Order::ID)
        .paginate(db, query.page.unwrap_or(1), PER_PAGE)
        .await?;
    let page = page.map(|o| {
        json!({
            "number": o.number,
            "status": o.status.as_str(),
            "fulfilment": o.fulfilment.as_str(),
            "store_id": o.operating_store_id,
            "total": o.total,
            "placed_at": o.placed_at,
            "paid_at": o.paid_at,
        })
    });
    Ok(Json(page_json(&page, |n| {
        page_url(&state, "api.me.orders", &[], n)
    })))
}
