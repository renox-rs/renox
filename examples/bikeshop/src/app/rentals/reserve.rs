//! The customer's side: find a bike (`/rent`), reserve it, pay the deposit
//! online, see or cancel the reservation, and the list of one's rentals.

use renox::chrono::{Duration, NaiveDateTime, Timelike};
use renox::prelude::*;
use renox::validation::FormContext;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::booking::{self, NewRental, clashing, free_bikes, from_local, rentable, to_local};
use super::model::{DepositStatus, Rental, RentalBike, RentalRow, RentalStatus};
use super::notify::{self, Notice, Tone};
use super::pricing::{self, Quote, period_problem};
use super::{customer_of, identity};
use crate::app::accounts::model::Customer;
use crate::app::accounts::preferences::Kind;
use crate::app::catalog::model::{Category, CategoryKind, Product, ProductVariant};
use crate::app::sales::model::{Payment, PaymentMethod};
use crate::app::sales::payments::{self, Charge, Payable};
use crate::app::staff::model::Store;

/// The form fields' date-time format (`datetime_range` sends it).
const FIELD_FORMAT: &str = "%Y-%m-%dT%H:%M";

/// `/rent`'s query string: the store, the period, a bike type and a size.
#[derive(Deserialize, Default, Debug)]
pub struct SearchQuery {
    pub store: Option<i64>,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    /// A category id, or empty for any type.
    pub category: Option<String>,
    pub size: Option<String>,
}

/// A free bike as the rent page lists it.
#[derive(Serialize, Debug, Clone)]
pub struct BikeOffer {
    pub id: i64,
    pub model: String,
    pub size: Option<String>,
    pub frame_number: String,
    pub owner_store: String,
    pub placed_here: bool,
    pub quote: Quote,
}

fn parse_local(text: Option<&str>) -> Option<NaiveDateTime> {
    let text = text?.trim();
    NaiveDateTime::parse_from_str(text, FIELD_FORMAT)
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"))
        .ok()
}

/// The hours the rent form offers (`opens`/`closes` of its `datetime_range`).
const OPENS: u32 = 8;
const CLOSES: u32 = 20;

/// The period the rent page proposes at `local` (now, in `APP_TIMEZONE`):
/// from the next full hour for three hours, kept inside the hours the form
/// can show. Outside them it proposes the next opening, because a time the
/// form's lists lack is cleared by the form, which then re-asks the server.
fn default_period(local: NaiveDateTime) -> (NaiveDateTime, NaiveDateTime) {
    let hour = local
        .date()
        .and_hms_opt(local.hour(), 0, 0)
        .unwrap_or(local);
    let mut start = hour + Duration::hours(1);
    if start.hour() < OPENS {
        start = start.date().and_hms_opt(OPENS, 0, 0).unwrap_or(start);
    } else if start.hour() >= CLOSES - 1 {
        start = (start.date() + Duration::days(1))
            .and_hms_opt(OPENS, 0, 0)
            .unwrap_or(start);
    }
    let closing = start.date().and_hms_opt(CLOSES, 0, 0).unwrap_or(start);
    (start, (start + Duration::hours(3)).min(closing))
}

/// The model's name and size of each variant: two queries.
pub async fn variant_names(
    db: &Db,
    variant_ids: Vec<i64>,
) -> Result<HashMap<i64, (String, Option<String>)>> {
    let variants = ProductVariant::find_many(db, variant_ids).await?;
    let products =
        renox::db::relations::belongs_to::<Product, _, _>(db, &variants, |v| v.product_id).await?;
    Ok(variants
        .into_iter()
        .map(|v| {
            let name = products
                .get(&v.product_id)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            (v.id, (name, v.size))
        })
        .collect())
}

/// `GET /rent` (`rentals.create`): pick a store, a period, a bike type and
/// a size; the bikes standing at that store and free for the period are
/// listed with their price and deposit, and the store's day is drawn as a
/// timeline. The form re-asks with htmx as it changes, so only the results
/// are swapped.
pub async fn search(
    State(state): State<AppState>,
    user: Option<AuthUser>,
    Query(query): Query<SearchQuery>,
) -> Result<View> {
    let db = &state.db;
    let stores = Store::all_by_name(db).await?;
    let store_id = query
        .store
        .filter(|id| stores.iter().any(|s| s.id == *id))
        .or_else(|| stores.first().map(|s| s.id))
        .unwrap_or_default();
    let (default_start, default_end) = default_period(to_local(&state.config, renox::db::now()));
    let start_local = parse_local(query.starts_at.as_deref()).unwrap_or(default_start);
    let end_local =
        parse_local(query.ends_at.as_deref()).unwrap_or(if start_local == default_start {
            default_end
        } else {
            start_local + Duration::hours(3)
        });
    let (start, end) = (
        from_local(&state.config, start_local),
        from_local(&state.config, end_local),
    );
    let problem = period_problem(start, end, renox::db::now());

    let categories = Category::query()
        .where_(Category::KIND.eq(CategoryKind::Bike))
        .order_by(Category::NAME)
        .get(db)
        .await?;
    // The variants rented out at this store, for the size filter and names.
    let at_store: Vec<i64> = RentalBike::query()
        .where_(RentalBike::LOCATION_STORE_ID.eq(store_id))
        .pluck(db, RentalBike::VARIANT_ID)
        .await?;
    let names = variant_names(db, at_store.clone()).await?;
    let mut sizes: Vec<String> = names.values().filter_map(|(_, s)| s.clone()).collect();
    sizes.sort();
    sizes.dedup();

    // The type and size narrow the variants.
    let mut wanted: Option<Vec<i64>> = None;
    let category = query
        .category
        .as_deref()
        .and_then(|c| c.parse::<i64>().ok())
        .filter(|c| *c > 0);
    if let Some(category) = category {
        let products: Vec<i64> = Product::query()
            .where_(Product::CATEGORY_ID.eq(category))
            .pluck(db, Product::ID)
            .await?;
        let ids: Vec<i64> = ProductVariant::query()
            .where_(ProductVariant::PRODUCT_ID.is_in(products))
            .pluck(db, ProductVariant::ID)
            .await?;
        wanted = Some(ids);
    }
    if let Some(size) = query.size.as_deref().filter(|s| !s.is_empty()) {
        let ids: Vec<i64> = names
            .iter()
            .filter(|(_, (_, s))| s.as_deref() == Some(size))
            .map(|(id, _)| *id)
            .collect();
        wanted = Some(match wanted {
            Some(before) => before.into_iter().filter(|id| ids.contains(id)).collect(),
            None => ids,
        });
    }

    let mut offers = Vec::new();
    if problem.is_none() && store_id > 0 {
        let stores_by_id: HashMap<i64, &Store> = stores.iter().map(|s| (s.id, s)).collect();
        for (bike, quote) in free_bikes(db, store_id, start, end, wanted).await? {
            let (model, size) = names.get(&bike.variant_id).cloned().unwrap_or_default();
            offers.push(BikeOffer {
                id: bike.id,
                model,
                size,
                placed_here: bike.placed_elsewhere(),
                owner_store: stores_by_id
                    .get(&bike.owner_store_id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default(),
                frame_number: bike.frame_number,
                quote,
            });
        }
    }
    let timeline = day_timeline(&state, store_id, start_local, &query).await?;
    let lang = state.current_lang();
    let problem = problem.map(|key| lang.t(key, &[]));
    // The selects' options: [value, label] pairs, and for the bikes [value, label, hint].
    let store_options: Vec<_> = stores.iter().map(|s| json!([s.id, s.name])).collect();
    let mut type_options = vec![json!(["", lang.t("rentals.search.any_type", &[])])];
    type_options.extend(categories.iter().map(|c| json!([c.id, c.name])));
    let mut size_options = vec![json!(["", lang.t("rentals.search.any_size", &[])])];
    size_options.extend(sizes.iter().map(|s| json!([s, s])));
    let deposit_label = lang.t("rentals.fields.deposit", &[]);
    let bike_options: Vec<_> = offers
        .iter()
        .map(|o| {
            let mut hint = format!(
                "{} · {} {} · {}",
                money(&state, o.quote.price),
                deposit_label,
                money(&state, o.quote.deposit),
                o.frame_number
            );
            if o.placed_here {
                hint.push_str(&format!(
                    " · {}",
                    lang.t("rentals.search.placed_by", &[("store", &o.owner_store)])
                ));
            }
            let label = match o.size.as_deref().filter(|s| !s.is_empty()) {
                Some(size) => format!("{} ({size})", o.model),
                None => o.model.clone(),
            };
            json!([o.id, label, hint])
        })
        .collect();
    Ok(view(
        "rentals/search.html",
        context! {
            stores,
            store_options,
            type_options,
            size_options,
            bike_options,
            store_id,
            categories,
            sizes,
            category => category.unwrap_or_default(),
            size => query.size.unwrap_or_default(),
            starts_at => start_local.format(FIELD_FORMAT).to_string(),
            ends_at => end_local.format(FIELD_FORMAT).to_string(),
            min_day => to_local(&state.config, renox::db::now()).date().to_string(),
            max_day => (to_local(&state.config, renox::db::now()).date() + Duration::days(60)).to_string(),
            offers,
            problem,
            timeline,
            signed_in => user.is_some(),
        },
    ))
}

/// The store's day as the `availability` block draws it: its bikes (at
/// most 12) against the hours from 08:00 to 20:00, booked or free. A free
/// hour links to the same page with a two-hour period from there. Two
/// queries: the bikes, then the day's rentals of all of them.
async fn day_timeline(
    state: &AppState,
    store_id: i64,
    day_of: NaiveDateTime,
    query: &SearchQuery,
) -> Result<renox::serde_json::Value> {
    const OPENS: u32 = 8;
    const CLOSES: u32 = 20;
    let db = &state.db;
    let day = day_of.date();
    let first = from_local(
        &state.config,
        day.and_hms_opt(OPENS, 0, 0).unwrap_or(day_of),
    );
    let last = from_local(
        &state.config,
        day.and_hms_opt(CLOSES, 0, 0).unwrap_or(day_of),
    );
    let bikes = RentalBike::query()
        .where_(RentalBike::LOCATION_STORE_ID.eq(store_id))
        .order_by(RentalBike::ID)
        .limit(12)
        .get(db)
        .await?;
    let ids: Vec<i64> = bikes.iter().map(|b| b.id).collect();
    let rentals = Rental::query()
        .where_(Rental::RENTAL_BIKE_ID.is_in(ids))
        .where_(Rental::STATUS.is_in(booking::HOLDING))
        .where_(Rental::STARTS_AT.lt(last))
        .where_(Rental::DUE_AT.gt(first))
        .get(db)
        .await?;
    let names = variant_names(db, bikes.iter().map(|b| b.variant_id).collect()).await?;
    let lang = state.current_lang();
    let columns: Vec<String> = (OPENS..CLOSES).map(|h| format!("{h:02}:00")).collect();
    let mut rows = Vec::new();
    for bike in &bikes {
        let mut states: Vec<&str> = Vec::new();
        for hour in OPENS..CLOSES {
            let from = from_local(&state.config, day.and_hms_opt(hour, 0, 0).unwrap_or(day_of));
            let to = from + Duration::hours(1);
            let booked = !rentable(bike.status)
                || rentals.iter().any(|r| {
                    r.rental_bike_id == bike.id
                        && (r.status == RentalStatus::Overdue
                            || (r.starts_at < to && r.due_at > from))
                });
            states.push(if booked { "booked" } else { "free" });
        }
        // Neighbouring hours in the same state become one slot.
        let mut slots = Vec::new();
        let mut hour = 0;
        while hour < states.len() {
            let state_here = states[hour];
            let mut span = 1;
            while hour + span < states.len()
                && states[hour + span] == state_here
                && state_here == "booked"
            {
                span += 1;
            }
            if state_here == "free" {
                let at = day.and_hms_opt(OPENS + hour as u32, 0, 0).unwrap_or(day_of);
                let mut url = format!(
                    "/rent?store={store_id}&starts_at={}&ends_at={}",
                    at.format(FIELD_FORMAT),
                    (at + Duration::hours(2)).format(FIELD_FORMAT)
                );
                if let Some(c) = query.category.as_deref().filter(|c| !c.is_empty()) {
                    url.push_str(&format!("&category={c}"));
                }
                slots.push(json!({ "state": "free", "url": url }));
            } else {
                slots.push(json!({ "state": "booked", "span": span, "title": lang.t("rentals.search.taken", &[]) }));
            }
            hour += span;
        }
        let (model, size) = names.get(&bike.variant_id).cloned().unwrap_or_default();
        rows.push(json!({
            "label": model,
            "note": format!("{} · {}", size.unwrap_or_default(), bike.frame_number),
            "slots": slots,
        }));
    }
    Ok(json!({ "columns": columns, "rows": rows }))
}

/// The reservation form, sent from `/rent`.
#[derive(Deserialize, Debug)]
pub struct ReserveForm {
    pub store: i64,
    pub starts_at: NaiveDateTime,
    pub ends_at: NaiveDateTime,
    #[serde(default)]
    pub bike: i64,
}

impl ReserveForm {
    /// The period as moments (the fields are `APP_TIMEZONE` wall-clock times).
    pub fn period(&self, config: &Config) -> (DateTime, DateTime) {
        (
            from_local(config, self.starts_at),
            from_local(config, self.ends_at),
        )
    }
}

// [explain:rentals.create.form]
impl Validate for ReserveForm {
    fn rules(&self, v: &mut Validator) {
        v.field("store", &self.store).exists("stores", "id");
    }

    /// The period's rules and the overlap check, with the form's errors:
    /// the first of the two checks of the overlap rule (see
    /// [`super::booking`]); [`booking::book`] checks again in its
    /// transaction.
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let lang = form.state.current_lang();
        let (start, end) = self.period(&form.state.config);
        if let Some(key) = period_problem(start, end, renox::db::now()) {
            errors.add("ends_at", lang.t(key, &[]));
            return Ok(());
        }
        if self.bike <= 0 {
            errors.add("bike", lang.t("rentals.errors.pick_bike", &[]));
            return Ok(());
        }
        let db = &form.state.db;
        match RentalBike::find(db, self.bike).await? {
            Some(bike) if bike.location_store_id == self.store && rentable(bike.status) => {
                if clashing(bike.id, start, end).exists(db).await? {
                    errors.add("bike", lang.t("rentals.errors.taken", &[]));
                }
            }
            _ => errors.add("bike", lang.t("rentals.errors.gone", &[])),
        }
        Ok(())
    }
}
// [/explain:rentals.create.form]

/// `POST /rent` (`rentals.reserve`): books the bike (the transaction
/// checks the overlap again; the loser of a race gets the "just taken"
/// error), then shows the reservation, where the deposit is paid. A
/// customer who never sent their ID is asked for it first.
pub async fn reserve(
    State(state): State<AppState>,
    user: AuthUser,
    session: Session,
    Valid(form): Valid<ReserveForm>,
) -> Result<Response> {
    let customer = customer_of(&state.db, &user).await?;
    if !identity::submitted(&state.db, &customer).await? {
        session.flash(
            "status",
            state.current_lang().t("rentals.identity.needed", &[]),
        )?;
        return Ok(Redirect::route("rentals.identity", &[])?.into_response());
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
    let rental = match booked {
        Ok(rental) => rental,
        Err(refusal) => {
            let mut errors = Errors::new();
            errors.add("bike", state.current_lang().t(refusal.key(), &[]));
            return Err(errors.into());
        }
    };
    Ok(Redirect::route("rentals.show", &[&rental.reservation_code])?.into_response())
}

/// The customer's own rental by its code, or a 404 (someone else's code
/// is as good as a wrong one).
pub async fn own_rental(db: &Db, user: &User, code: &str) -> Result<(Customer, Rental)> {
    let customer = customer_of(db, user).await?;
    let rental = Rental::query()
        .where_eq("reservation_code", code)
        .where_(Rental::CUSTOMER_ID.eq(customer.id))
        .first(db)
        .await?
        .ok_or(Error::NotFound)?;
    Ok((customer, rental))
}

/// `GET /rentals/{code}` (`rentals.show`): the reservation, its code, the
/// bike, the store, the price and the deposit, with "Pay the deposit"
/// until it is paid and "Cancel" until an hour before the start.
// [explain:rentals.show.handler]
pub async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path(code): Path<String>,
) -> Result<View> {
    let (_, rental) = own_rental(&state.db, &user, &code).await?;
    let row = RentalRow::load(&state.db, vec![rental])
        .await?
        .pop()
        .ok_or(Error::NotFound)?;
    let pay_by = row
        .rental
        .created_at
        .map(|at| at + Duration::minutes(pricing::PAYMENT_WINDOW_MINUTES));
    let held_until = row.rental.starts_at + Duration::minutes(pricing::NO_SHOW_AFTER_MINUTES);
    Ok(view(
        "rentals/show.html",
        context! {
            cancellable => row.rental.cancellable(),
            needs_payment => row.rental.is_reserved() && row.rental.deposit_status == DepositStatus::Unpaid,
            pay_by,
            held_until,
            row,
        },
    ))
}
// [/explain:rentals.show.handler]

/// `POST /rentals/{code}/pay` (`rentals.pay`): starts the online deposit
/// payment (the shared payments contract) and sends the customer to the
/// gateway's page. Its webhook emits `PaymentSucceeded`, which
/// [`deposit_paid`] turns into a held deposit.
// [explain:rentals.show.pay]
pub async fn pay(
    State(state): State<AppState>,
    user: AuthUser,
    Path(code): Path<String>,
) -> Result<Redirect> {
    let (customer, rental) = own_rental(&state.db, &user, &code).await?;
    if !rental.is_reserved() || rental.deposit_status != DepositStatus::Unpaid {
        return Redirect::route("rentals.show", &[&rental.reservation_code]);
    }
    let checkout = payments::start(
        &state,
        Charge {
            payable: Payable::Rental(rental.id),
            customer_id: Some(customer.id),
            store_id: rental.operating_store_id,
            amount: rental.deposit,
        },
    )
    .await?;
    Ok(Redirect::to(&checkout.redirect_url))
}
// [/explain:rentals.show.pay]

/// `POST /rentals/{code}/cancel` (`rentals.cancel`): until an hour before
/// the start; a paid deposit is given back whole.
pub async fn cancel(
    State(state): State<AppState>,
    user: AuthUser,
    session: Session,
    Path(code): Path<String>,
) -> Result<Redirect> {
    let (customer, mut rental) = own_rental(&state.db, &user, &code).await?;
    let lang = state.current_lang();
    if !cancel_rental(&state, &customer, &mut rental).await? {
        session.flash("status", lang.t("rentals.show.too_late", &[]))?;
        return Redirect::route("rentals.show", &[&rental.reservation_code]);
    }
    session.flash("status", lang.t("rentals.show.cancelled", &[]))?;
    Redirect::route("rentals.show", &[&rental.reservation_code])
}

/// Cancels the customer's reservation when it still may be (until an hour
/// before the start; a paid deposit is given back whole) and tells them:
/// the rule the page above and the JSON API (#241) share. `false` when
/// it's too late.
pub async fn cancel_rental(
    state: &AppState,
    customer: &Customer,
    rental: &mut Rental,
) -> Result<bool> {
    if !rental.cancellable() {
        return Ok(false);
    }
    rental.status = RentalStatus::Cancelled;
    rental.cancelled_at = Some(renox::db::now());
    if rental.deposit_status == DepositStatus::Held {
        rental.deposit_status = DepositStatus::Refunded;
        rental.deposit_refunded = rental.deposit;
    }
    rental.save(&state.db).await?;
    notify::customer(
        state,
        customer,
        Kind::Rental,
        &Notice::new(
            "rental-cancelled",
            "rentals.mail.cancelled.title",
            "rentals.mail.cancelled.body",
        )
        .param("code", &rental.reservation_code)
        .row(
            "rentals.fields.refund",
            money(state, rental.deposit_refunded),
        )
        .url(super::link(
            state,
            "rentals.show",
            Some(&rental.reservation_code),
        )?),
    )
    .await?;
    Ok(true)
}

/// `GET /rentals` (`rentals.mine`): the customer's rentals, current first,
/// then past ones with their fees. Six queries whatever the number of
/// rentals (`RentalRow::load`).
// [explain:rentals.mine.handler]
pub async fn mine(State(state): State<AppState>, user: AuthUser) -> Result<View> {
    let customer = customer_of(&state.db, &user).await?;
    let rentals = Rental::query()
        .where_(Rental::CUSTOMER_ID.eq(customer.id))
        .order_by_desc(Rental::STARTS_AT)
        .limit(50)
        .get(&state.db)
        .await?;
    let rows = RentalRow::load(&state.db, rentals).await?;
    let (current, past): (Vec<_>, Vec<_>) = rows.into_iter().partition(|r| {
        matches!(
            r.rental.status,
            RentalStatus::Reserved | RentalStatus::Active | RentalStatus::Overdue
        )
    });
    let identity = identity::latest(&state.db, customer.id).await?;
    Ok(view(
        "rentals/mine.html",
        context! { current, past, verified => customer.id_verified(), identity },
    ))
}
// [/explain:rentals.mine.handler]

/// The amount (smallest unit) in the `money` filter's format, for mails
/// built in Rust.
pub fn money(state: &AppState, amount: i64) -> String {
    crate::money::format(amount, &state.config.currency, &state.current_lang().locale)
}

/// `PaymentSucceeded` for a rental: an **online** payment is the deposit
/// of a reservation, now held by the operating store; the customer gets
/// the confirmation with the reservation code. (Counter payments are
/// recorded by the counter itself.)
pub async fn deposit_paid(state: &AppState, payment_id: i64, rental_id: i64) -> Result {
    let Some(payment) = Payment::find(&state.db, payment_id).await? else {
        return Ok(());
    };
    if payment.method != PaymentMethod::Gateway {
        return Ok(());
    }
    let Some(mut rental) = Rental::find(&state.db, rental_id).await? else {
        return Ok(());
    };
    if rental.deposit_status != DepositStatus::Unpaid {
        return Ok(());
    }
    let late = !rental.is_reserved();
    rental.deposit_status = if late {
        // Paid after the reservation lapsed: given back.
        rental.deposit_refunded = payment.amount;
        DepositStatus::Refunded
    } else {
        DepositStatus::Held
    };
    rental.save(&state.db).await?;
    if late {
        return Ok(());
    }
    let row = RentalRow::load(&state.db, vec![rental.clone()])
        .await?
        .pop();
    let customer = Customer::find(&state.db, rental.customer_id).await?;
    if let (Some(row), Some(customer)) = (row, customer) {
        let lang = state.current_lang();
        let local = to_local(&state.config, rental.starts_at);
        notify::customer(
            state,
            &customer,
            Kind::Rental,
            &Notice::new(
                "rental-reserved",
                "rentals.mail.reserved.title",
                "rentals.mail.reserved.body",
            )
            .param("code", &rental.reservation_code)
            .param(
                "store",
                row.operating_store.map(|s| s.name).unwrap_or_default(),
            )
            .row("rentals.fields.code", &rental.reservation_code)
            .row("rentals.fields.bike", row.model.unwrap_or_default())
            .row("rentals.fields.starts", local.format("%Y-%m-%d %H:%M"))
            .row(
                "rentals.fields.ends",
                to_local(&state.config, rental.due_at).format("%Y-%m-%d %H:%M"),
            )
            .row("rentals.fields.price", money(state, rental.price))
            .row("rentals.fields.deposit", money(state, rental.deposit))
            .tone(Tone::Success)
            .url(super::link(
                state,
                "rentals.show",
                Some(&rental.reservation_code),
            )?),
        )
        .await?;
        let _ = lang;
    }
    Ok(())
}

/// `PaymentFailed` for a rental: an unpaid reservation is called off, so
/// the bike is free again.
pub async fn deposit_failed(state: &AppState, rental_id: i64) -> Result {
    let Some(mut rental) = Rental::find(&state.db, rental_id).await? else {
        return Ok(());
    };
    if !rental.is_reserved() || rental.deposit_status != DepositStatus::Unpaid {
        return Ok(());
    }
    rental.status = RentalStatus::Cancelled;
    rental.cancelled_at = Some(renox::db::now());
    rental.save(&state.db).await?;
    if let Some(customer) = Customer::find(&state.db, rental.customer_id).await? {
        notify::customer(
            state,
            &customer,
            Kind::Rental,
            &Notice::new(
                "rental-payment-failed",
                "rentals.mail.unpaid.title",
                "rentals.mail.unpaid.body",
            )
            .param("code", &rental.reservation_code)
            .tone(Tone::Warning)
            .url(super::link(state, "rentals.create", None::<i64>)?),
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        renox::chrono::NaiveDate::from_ymd_opt(2026, 10, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn the_default_period_stays_inside_the_forms_hours() {
        // Mid-morning: the next hour for three hours.
        assert_eq!(default_period(at(9, 10, 30)), (at(9, 11, 0), at(9, 14, 0)));
        // Late afternoon: the end stops at closing time (the form has no 22:00).
        assert_eq!(default_period(at(9, 17, 5)), (at(9, 18, 0), at(9, 20, 0)));
        // The last bookable hour keeps its two hours.
        assert_eq!(default_period(at(9, 17, 59)), (at(9, 18, 0), at(9, 20, 0)));
        // Evening and night: the next morning's opening.
        assert_eq!(default_period(at(9, 18, 1)), (at(10, 8, 0), at(10, 11, 0)));
        assert_eq!(default_period(at(9, 23, 30)), (at(10, 8, 0), at(10, 11, 0)));
        // Before opening: today's opening.
        assert_eq!(default_period(at(9, 3, 0)), (at(9, 8, 0), at(9, 11, 0)));
    }
}
