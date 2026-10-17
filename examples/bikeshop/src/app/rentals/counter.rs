//! The counter: handing bikes out and taking them back, in the store the
//! person works in today (the active store, `renox::context`).
//!
//! - **Pick-up** happens at the rental's **operating store** (where the bike
//!   stands): the customer must be verified, the condition checklist is
//!   ticked, and what isn't paid yet (the price, the deposit when it wasn't
//!   paid online) is taken at the counter through the shared payments
//!   contract (`payments::record_counter`).
//! - **Return** happens at **any** store: the bike's **location** becomes
//!   the store that took it back, its **owner** never changes. When it isn't
//!   back where it was placed, the receipt suggests sending it back (#245).
//!   Late fee per started hour after a 15-minute grace, damage with photos,
//!   a note and a fee; the deposit (held by the operating store) settles
//!   against the fees; the hours ridden are added to the bike. A damaged
//!   bike goes to `maintenance` and the workshop gets a work order
//!   ([`super::FleetRepairNeeded`]); [`super::RentalClosed`] lets the
//!   intercompany books (#245) book the revenue to the owner store.
//!
//! Who may see a rental here: the usual "mine or at my store" rule
//! (`access::find`), plus, for a bike that is **out**, anyone who may take
//! bikes back in the active store, since a bike can come back anywhere.

use renox::chrono::{Duration, NaiveDateTime};
use renox::db::Json as DbJson;
use renox::prelude::*;
use renox::select::{OptionQuery, SelectOption};
use renox::state_machine::{StateMachine, TransitionError};
use renox::validation::FormContext;
use serde::{Deserialize, Serialize};

use super::booking::{self, NewRental, clashing, rentable, to_local};
use super::model::{
    BikeCondition, BikePlacement, BikeStatus, DepositStatus, IdentityStatus, PhotoKind,
    PlacementStatus, Rental, RentalBike, RentalPhoto, RentalRow, RentalStatus,
};
use super::notify::{self, Notice, Tone};
use super::pricing::{self, Settlement, late_fee, settle};
use super::reserve::money;
use super::{FleetRepairNeeded, RentalClosed, active_store, identity, staff_id};
use crate::app::access::{self, StoreAttr, catalogue};
use crate::app::accounts::model::Customer;
use crate::app::accounts::preferences::Kind;
use crate::app::sales::model::{PAYABLE_RENTAL, Payment, PaymentMethod, PaymentStatus};
use crate::app::sales::payments::{self, Charge, Payable};
use crate::app::staff::model::Store;

/// The condition checklist, the same at pick-up and return (translated as
/// `rentals.checklist.<item>`).
pub const CHECKLIST: [&str; 7] = [
    "frame", "brakes", "tyres", "gears", "lights", "lock", "bell",
];

/// How a customer pays at the counter (the form's `method`).
pub const COUNTER_METHODS: [&str; 2] = ["cash", "card"];

/// The payment method for a counter form's `method` (card unless cash).
pub fn counter_method(method: &str) -> PaymentMethod {
    if method == "cash" {
        PaymentMethod::Cash
    } else {
        PaymentMethod::Card
    }
}

/// The rental `id` as the counter may see it: visible in one of its stores,
/// or out and returnable in the active store. A 404 otherwise.
pub async fn counter_rental(db: &Db, user: &User, id: i64) -> Result<Rental> {
    let rental = Rental::find_or_404(db, id).await?;
    if access::can_see(user, &rental) || returnable_here(user, &rental) {
        Ok(rental)
    } else {
        Err(Error::NotFound)
    }
}

/// Out with the customer, and the user may take bikes back in the store
/// they work in now.
fn returnable_here(user: &User, rental: &Rental) -> bool {
    rental.is_out()
        && access::active_store::current()
            .is_some_and(|store| access::can_in(user, catalogue::RENTALS_RETURN, store))
}

/// `?q=` on the counter: a reservation code or part of a customer's name.
#[derive(Deserialize, Default)]
pub struct CounterQuery {
    #[serde(default)]
    pub q: String,
}

/// `GET /staff/rentals` (`rentals.counter`): the active store's day at the
/// counter (pick-ups due today, bikes out, overdue ones) and a search by
/// reservation code or customer. All lists in one `RentalRow::load` (six
/// queries) whatever their length.
// [explain:rentals.counter.handler]
pub async fn index(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<CounterQuery>,
) -> Result<View> {
    let store = active_store()?;
    let db = &state.db;
    let local_today = to_local(&state.config, renox::db::now()).date();
    let tomorrow = booking::from_local(
        &state.config,
        (local_today + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default(),
    );
    let mut rentals = Rental::query()
        .where_(Rental::OPERATING_STORE_ID.eq(store))
        .where_any(|any| {
            any.where_(Rental::STATUS.is_in([RentalStatus::Active, RentalStatus::Overdue]))
                .where_all(|all| {
                    all.where_(Rental::STATUS.eq(RentalStatus::Reserved))
                        .where_(Rental::STARTS_AT.lt(tomorrow))
                })
        })
        .order_by(Rental::STARTS_AT)
        .limit(200)
        .get(db)
        .await?;
    // [/explain:rentals.counter.handler]
    let q = query.q.trim().to_owned();
    let mut found_ids: Vec<i64> = Vec::new();
    if !q.is_empty() {
        let customers: Vec<i64> = Customer::query()
            .where_any(|any| {
                any.where_(Customer::NAME.like(&format!("%{q}%")))
                    .where_(Customer::EMAIL.like(&format!("%{q}%")))
            })
            .limit(20)
            .pluck(db, Customer::ID)
            .await?;
        let found = Rental::query()
            .where_any(|any| {
                any.where_eq("reservation_code", q.to_uppercase())
                    .where_(Rental::CUSTOMER_ID.is_in(customers))
            })
            .where_(Rental::STATUS.is_in(booking::HOLDING))
            .order_by(Rental::STARTS_AT)
            .limit(20)
            .get(db)
            .await?;
        for rental in found {
            if access::can_see(&user, &rental) || returnable_here(&user, &rental) {
                found_ids.push(rental.id);
                if !rentals.iter().any(|r| r.id == rental.id) {
                    rentals.push(rental);
                }
            }
        }
    }
    // [explain:rentals.counter.handler]
    let rows = RentalRow::load(db, rentals).await?;
    // [/explain:rentals.counter.handler]
    let mut pickups = Vec::new();
    let mut out = Vec::new();
    let mut overdue = Vec::new();
    let mut found = Vec::new();
    for row in rows {
        if found_ids.contains(&row.rental.id) {
            found.push(row.clone());
        }
        if row.rental.operating_store_id != store {
            continue;
        }
        match row.rental.status {
            RentalStatus::Reserved => pickups.push(row),
            RentalStatus::Overdue => overdue.push(row),
            _ if row.overdue => overdue.push(row),
            _ => out.push(row),
        }
    }
    Ok(view(
        "rentals/counter.html",
        context! { q, searched => !query.q.trim().is_empty(), found, pickups, out, overdue },
    ))
}

/// The store a bike goes back to after a return elsewhere: where it was
/// placed (the latest placement that moved it), else its owner store.
pub async fn home_store(db: &Db, bike: &RentalBike) -> Result<i64> {
    Ok(BikePlacement::query()
        .where_(BikePlacement::RENTAL_BIKE_ID.eq(bike.id))
        .where_(BikePlacement::STATUS.eq(PlacementStatus::Moved))
        .order_by_desc(BikePlacement::MOVED_AT)
        .first(db)
        .await?
        .map(|p| p.to_store_id)
        .unwrap_or(bike.owner_store_id))
}

/// What the desk page shows about money at a return, worked out now.
#[derive(Serialize, Debug, Clone)]
struct ReturnPreview {
    late_fee: i64,
    late_minutes: i64,
    held: i64,
    settlement: Settlement,
}

fn held(rental: &Rental) -> i64 {
    if rental.deposit_status == DepositStatus::Held {
        rental.deposit
    } else {
        0
    }
}

/// `GET /staff/rentals/{rental}` (`rentals.desk`): one rental at the
/// counter: the customer (verified or not, the ID masked), the bike, the
/// period, and the pick-up or the return form, whichever applies here.
// [explain:rentals.desk.handler]
pub async fn desk(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let store = active_store()?;
    let rental = counter_rental(db, &user, id).await?;
    let customer = Customer::find(db, rental.customer_id).await?;
    let document = identity::latest(db, rental.customer_id)
        .await?
        .filter(|d| d.status == IdentityStatus::Pending);
    let can_pickup = rental.is_reserved()
        && rental.operating_store_id == store
        && access::can(
            &user,
            catalogue::RENTALS_CHECKOUT,
            StoreAttr::Operating,
            &rental,
        );
    let can_return = rental.is_out() && access::can_in(&user, catalogue::RENTALS_RETURN, store);
    // [/explain:rentals.desk.handler]
    let now = renox::db::now();
    let late = late_fee(
        RentalBike::find(db, rental.rental_bike_id)
            .await?
            .map(|b| b.hourly_rate)
            .unwrap_or_default(),
        rental.due_at,
        now,
    );
    let preview = ReturnPreview {
        late_fee: late,
        late_minutes: (now - rental.due_at).num_minutes().max(0),
        held: held(&rental),
        settlement: settle(held(&rental), late),
    };
    let due_at_pickup = rental.price
        + if rental.deposit_status == DepositStatus::Unpaid {
            rental.deposit
        } else {
            0
        };
    let photos = RentalPhoto::query()
        .where_(RentalPhoto::RENTAL_ID.eq(rental.id))
        .order_by(RentalPhoto::ID)
        .get(db)
        .await?;
    let stores = Store::all_by_name(db).await?;
    let wrong_store = rental.is_reserved() && rental.operating_store_id != store;
    let row = RentalRow::load(db, vec![rental])
        .await?
        .pop()
        .ok_or(Error::NotFound)?;
    let lang = state.current_lang();
    let checklist_options: Vec<[String; 2]> = CHECKLIST
        .iter()
        .map(|item| {
            [
                item.to_string(),
                lang.t(&format!("rentals.checklist.{item}"), &[]),
            ]
        })
        .collect();
    Ok(view(
        "rentals/desk.html",
        context! {
            verified => customer.as_ref().is_some_and(|c| c.id_verified()),
            masked => customer.as_ref().and_then(|c| c.masked_id_number()),
            customer,
            document,
            can_pickup,
            can_return,
            wrong_store,
            preview,
            due_at_pickup,
            photos,
            stores,
            checklist => CHECKLIST,
            checklist_options,
            row,
        },
    ))
}

/// The pick-up form.
#[derive(Deserialize, Debug)]
pub struct PickupForm {
    /// The checklist items found in order.
    #[serde(default)]
    pub checklist: Vec<String>,
    /// How the customer pays what is due now: `cash` or `card`.
    pub method: String,
}

impl Validate for PickupForm {
    fn rules(&self, v: &mut Validator) {
        v.each("checklist", &self.checklist, |item| item.one_of(&CHECKLIST));
        v.field("method", &self.method)
            .required()
            .one_of(&COUNTER_METHODS);
    }
}

/// `POST /staff/rentals/{rental}/pickup` (`rentals.pickup`): the bike
/// leaves. Checked in the rental's operating store (`rentals.checkout`
/// there), which must be the store the person works in now; the customer
/// must be verified. The price (and the deposit when it wasn't paid
/// online) is recorded as counter payments after the rental is saved.
pub async fn pickup(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<PickupForm>,
) -> Result<Response> {
    let db = &state.db;
    let store = active_store()?;
    let lang = state.current_lang();
    let mut rental = counter_rental(db, &user, id).await?;
    access::require(
        &user,
        catalogue::RENTALS_CHECKOUT,
        StoreAttr::Operating,
        &rental,
    )?;
    if !rental.is_reserved() || rental.operating_store_id != store {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("rentals.desk.not_here", &[]),
        ));
    }
    let staff = staff_id(db, &user).await?.ok_or_else(|| {
        abort(
            StatusCode::FORBIDDEN,
            lang.t("rentals.errors.no_staff", &[]),
        )
    })?;
    hand_over(&state, &mut rental, &form, Some(staff)).await?;
    Ok((
        Toast::success(lang.t("rentals.desk.picked_up", &[])),
        Redirect::route("rentals.desk", &[&rental.id])?,
    )
        .into_response())
}

/// The bike leaves with the customer: the rule both the counter
/// ([`pickup`]) and a store's kiosk (the JSON API, #241) follow, after
/// each checked that `rental` is reserved at its own store. The customer
/// must be verified (a 422 on `method` otherwise); what isn't paid yet (the
/// price, the deposit when it wasn't paid online) is recorded as counter
/// payments after the rental is saved. `staff`: who handed it over (`None`
/// for a kiosk).
// [explain:rentals.desk.handover]
pub async fn hand_over(
    state: &AppState,
    rental: &mut Rental,
    form: &PickupForm,
    staff: Option<i64>,
) -> Result {
    let db = &state.db;
    let lang = state.current_lang();
    let customer = Customer::find_or_404(db, rental.customer_id).await?;
    if !customer.id_verified() {
        let mut errors = Errors::new();
        errors.add("method", lang.t("rentals.errors.unverified", &[]));
        return Err(errors.into());
    }
    let now = renox::db::now();
    let deposit_due = rental.deposit_status == DepositStatus::Unpaid;
    let mut tx = db.begin().await?;
    rental.picked_up_at = Some(now);
    rental.served_by = staff.or(rental.served_by);
    rental.pickup_checklist = Some(DbJson(form.checklist.clone()));
    if deposit_due {
        rental.deposit_status = DepositStatus::Held;
    }
    rental.save(&mut tx).await?;
    // The status moves through the machine (`machine.rs`), in this transaction.
    let moved = rental
        .transition("pickup")
        .by_system()
        .meta(json!({ "staff": staff }))
        .run_in(&mut tx)
        .await
        .map_err(TransitionError::into_error)?;
    rental.status = RentalStatus::Active;
    RentalBike::query()
        .where_(RentalBike::ID.eq(rental.rental_bike_id))
        .update(&mut tx, &[("status", &BikeStatus::Rented)])
        .await?;
    tx.commit().await?;
    moved.emit(state).await?;
    // [/explain:rentals.desk.handover]
    let charge = |amount: i64| Charge {
        payable: Payable::Rental(rental.id),
        customer_id: Some(customer.id),
        store_id: rental.operating_store_id,
        amount,
    };
    if rental.price > 0 {
        payments::record_counter(
            state,
            charge(rental.price),
            counter_method(&form.method),
            staff,
        )
        .await?;
    }
    if deposit_due && rental.deposit > 0 {
        payments::record_counter(
            state,
            charge(rental.deposit),
            counter_method(&form.method),
            staff,
        )
        .await?;
    }
    Ok(())
}

/// The return form (multipart: damage photos).
#[derive(Deserialize)]
pub struct ReturnForm {
    #[serde(default)]
    pub checklist: Vec<String>,
    /// Damage found.
    #[serde(default)]
    pub damaged: bool,
    /// In whole units as typed (`45.00`); stored in the smallest unit.
    pub damage_fee: Option<f64>,
    pub damage_note: Option<String>,
    #[serde(default)]
    pub photos: Vec<Upload>,
    /// How the customer pays fees beyond the deposit: `cash` or `card`.
    pub method: String,
}

impl Validate for ReturnForm {
    fn rules(&self, v: &mut Validator) {
        v.each("checklist", &self.checklist, |item| item.one_of(&CHECKLIST));
        v.field("damage_note", &self.damage_note)
            .required_if(self.damaged)
            .max(500);
        v.field("damage_fee", &self.damage_fee).min(0);
        v.field("photos", &self.photos).max(6);
        v.each("photos", &self.photos, |photo| photo.image().max(5 * 1024));
        v.field("method", &self.method)
            .required()
            .one_of(&COUNTER_METHODS);
    }

    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        if self.damaged && self.damage_fee.unwrap_or(0.0) <= 0.0 && self.photos.is_empty() {
            errors.add(
                "damage_fee",
                form.state
                    .current_lang()
                    .t("rentals.errors.damage_fee", &[]),
            );
        }
        Ok(())
    }
}

/// `POST /staff/rentals/{rental}/return` (`rentals.return`): the bike is
/// back, at whichever store took it (see the module docs).
pub async fn give_back(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<ReturnForm>,
) -> Result<Response> {
    let db = &state.db;
    let store = active_store()?;
    let lang = state.current_lang();
    let mut rental = counter_rental(db, &user, id).await?;
    if !rental.is_out() {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("rentals.desk.not_out", &[]),
        ));
    }
    if !access::can_in(&user, catalogue::RENTALS_RETURN, store) {
        return Err(Error::Forbidden);
    }
    let staff = staff_id(db, &user).await?.ok_or_else(|| {
        abort(
            StatusCode::FORBIDDEN,
            lang.t("rentals.errors.no_staff", &[]),
        )
    })?;
    take_back(&state, &mut rental, &form, store, Some(staff)).await?;
    Ok((
        Toast::success(lang.t("rentals.desk.returned", &[])),
        Redirect::route("rentals.receipt", &[&rental.id])?,
    )
        .into_response())
}

/// The bike is back at `store`: the rule both the counter ([`give_back`])
/// and a store's kiosk (the JSON API, #241) follow, after each checked
/// that `rental` is out and may come back here. Late fee, damage (photos,
/// a note, a fee), the deposit settled against the fees, the hours ridden
/// added to the bike, its location moved to `store`; a damaged bike goes to
/// the workshop ([`FleetRepairNeeded`]), [`RentalClosed`] tells the books,
/// the customer gets the receipt. `staff`: who took it back (`None` for a
/// kiosk). Answers the settlement.
pub async fn take_back(
    state: &AppState,
    rental: &mut Rental,
    form: &ReturnForm,
    store: i64,
    staff: Option<i64>,
) -> Result<Settlement> {
    let db = &state.db;
    let lang = state.current_lang();
    let mut bike = RentalBike::find_or_404(db, rental.rental_bike_id).await?;
    // The photos go to private storage before the transaction.
    let mut stored = Vec::new();
    for photo in &form.photos {
        stored.push(photo.store(&state.storage, "rentals").await?);
    }
    let now = renox::db::now();
    let damage_fee = crate::money::from_form(form.damage_fee).max(0);
    let damaged = form.damaged || damage_fee > 0;
    rental.late_fee = late_fee(bike.hourly_rate, rental.due_at, now);
    rental.damage_fee = if damaged { damage_fee } else { 0 };
    let settlement = settle(held(rental), rental.fees());
    let minutes = pricing::ridden_minutes(rental.picked_up_at.unwrap_or(rental.starts_at), now);

    // [explain:rentals.desk.return]
    let mut tx = db.begin().await?;
    rental.returned_at = Some(now);
    rental.returned_by = staff;
    rental.return_store_id = (store != rental.operating_store_id).then_some(store);
    rental.return_checklist = Some(DbJson(form.checklist.clone()));
    rental.damage_note = form.damage_note.clone().filter(|n| !n.trim().is_empty());
    rental.ridden_minutes = minutes;
    if rental.deposit_status == DepositStatus::Held {
        rental.deposit_status = DepositStatus::Settled;
        rental.deposit_refunded = settlement.refund;
    }
    rental.save(&mut tx).await?;
    let moved = rental
        .transition("return")
        .by_system()
        .meta(json!({ "staff": staff, "store": store }))
        .run_in(&mut tx)
        .await
        .map_err(TransitionError::into_error)?;
    rental.status = RentalStatus::Returned;
    // [/explain:rentals.desk.return]
    bike.location_store_id = store;
    bike.ridden_hours += pricing::ridden_hours(minutes);
    if damaged {
        bike.status = BikeStatus::Maintenance;
        bike.condition = BikeCondition::NeedsRepair;
    } else {
        bike.status = BikeStatus::Available;
    }
    bike.save(&mut tx).await?;
    for path in stored {
        RentalPhoto::create(
            &mut tx,
            RentalPhoto {
                rental_id: rental.id,
                kind: if damaged {
                    PhotoKind::Damage
                } else {
                    PhotoKind::Return
                },
                path,
                ..Default::default()
            },
        )
        .await?;
    }
    // [explain:rentals.desk.return]
    tx.commit().await?;
    // [/explain:rentals.desk.return]
    moved.emit(state).await?;

    if settlement.due > 0 {
        payments::record_counter(
            state,
            Charge {
                payable: Payable::Rental(rental.id),
                customer_id: Some(rental.customer_id),
                store_id: store,
                amount: settlement.due,
            },
            counter_method(&form.method),
            staff,
        )
        .await?;
    }
    // [explain:rentals.desk.return]
    if damaged {
        state
            .emit(FleetRepairNeeded {
                bike_id: bike.id,
                rental_id: Some(rental.id),
                note: rental
                    .damage_note
                    .clone()
                    .unwrap_or_else(|| lang.t("rentals.desk.damage", &[])),
            })
            .await?;
    }
    state
        .emit(RentalClosed {
            rental_id: rental.id,
        })
        .await?;
    // [/explain:rentals.desk.return]
    if let Some(customer) = Customer::find(db, rental.customer_id).await? {
        notify::customer(
            state,
            &customer,
            Kind::Rental,
            &Notice::new(
                "rental-returned",
                "rentals.mail.returned.title",
                "rentals.mail.returned.body",
            )
            .param("code", &rental.reservation_code)
            .row("rentals.fields.price", money(state, rental.price))
            .row("rentals.fields.late_fee", money(state, rental.late_fee))
            .row("rentals.fields.damage_fee", money(state, rental.damage_fee))
            .row(
                "rentals.fields.refund",
                money(state, rental.deposit_refunded),
            )
            .row("rentals.fields.paid_now", money(state, settlement.due))
            .tone(Tone::Success)
            .url(super::link(
                state,
                "rentals.show",
                Some(&rental.reservation_code),
            )?),
        )
        .await?;
    }
    Ok(settlement)
}

/// How a rental is booked between stores, for the receipt (#245 writes the
/// entries; this shows the numbers).
#[derive(Serialize, Debug, Clone)]
struct Books {
    owner: String,
    operating: String,
    split: bool,
    /// The rate as people write it: `20`, `17.5`.
    fee_rate: String,
    fee: i64,
    owner_share: i64,
}

/// Basis points as a percentage without trailing zeros: 2000 → `20`, 1750 → `17.5`.
pub fn percent(bp: i64) -> String {
    let text = format!("{:.2}", bp as f64 / 100.0);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `GET /staff/rentals/{rental}/receipt` (`rentals.receipt`): what the
/// customer paid and got back, the payments recorded, how it is booked
/// between the owner and the operating store, and where the bike should go
/// next.
// [explain:rentals.receipt.handler]
pub async fn receipt(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let rental = counter_rental(db, &user, id).await?;
    let payments = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq(PAYABLE_RENTAL))
        .where_(Payment::PAYABLE_ID.eq(rental.id))
        .where_(Payment::STATUS.eq(PaymentStatus::Paid))
        .order_by(Payment::ID)
        .get(db)
        .await?;
    // [/explain:rentals.receipt.handler]
    let bike = RentalBike::find_or_404(db, rental.rental_bike_id).await?;
    let home = home_store(db, &bike).await?;
    let stores =
        Store::find_many(db, [rental.owner_store_id, rental.operating_store_id, home]).await?;
    let name = |id: i64| {
        stores
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    };
    // [explain:rentals.receipt.handler]
    let operating = stores.iter().find(|s| s.id == rental.operating_store_id);
    let fee = if rental.owner_store_id != rental.operating_store_id {
        operating.map(|s| s.fee_on(rental.price)).unwrap_or(0)
    } else {
        0
    };
    let books = Books {
        owner: name(rental.owner_store_id),
        operating: name(rental.operating_store_id),
        split: rental.owner_store_id != rental.operating_store_id,
        fee_rate: operating
            .map(|s| percent(s.fee_rate_bp))
            .unwrap_or_default(),
        fee,
        owner_share: rental.total() - fee,
    };
    // [/explain:rentals.receipt.handler]
    let send_back = (rental.status == RentalStatus::Returned && bike.location_store_id != home)
        .then(|| name(home));
    let settlement = settle(
        if matches!(rental.deposit_status, DepositStatus::Settled) {
            rental.deposit
        } else {
            0
        },
        rental.fees(),
    );
    let row = RentalRow::load(db, vec![rental])
        .await?
        .pop()
        .ok_or(Error::NotFound)?;
    Ok(view(
        "rentals/receipt.html",
        context! { row, payments, books, send_back, settlement, bike },
    ))
}

/// `GET /staff/rentals/{rental}/photos/{photo}` (`rentals.photo`): a
/// photo taken at the counter, through a signed temporary URL, for whoever
/// may see the rental.
pub async fn photo(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, photo)): Path<(i64, i64)>,
) -> Result<Redirect> {
    let rental = counter_rental(&state.db, &user, id).await?;
    let photo = RentalPhoto::query()
        .where_(RentalPhoto::ID.eq(photo))
        .where_(RentalPhoto::RENTAL_ID.eq(rental.id))
        .first_or_404(&state.db)
        .await?;
    let url = state
        .storage
        .temporary_url(&state, &photo.path, identity::PHOTO_LINK)
        .await?;
    Ok(Redirect::to(&url))
}

/// `GET /staff/rentals/customers` (`rentals.customers`): verified
/// customers for the walk-in form's searchable select (`renox::select`).
// [explain:rentals.walkin.options]
pub async fn customer_options(
    State(db): State<Db>,
    query: OptionQuery,
) -> Result<Json<Vec<SelectOption>>> {
    let customers = if query.is_lookup() {
        Customer::query()
            .where_(Customer::ID.is_in(query.values_as::<i64>()))
            .get(&db)
            .await?
    } else {
        let q = query.q.trim().to_owned();
        Customer::query()
            .where_(Customer::ID_VERIFIED_AT.is_not_null())
            .where_any(|any| {
                any.where_(Customer::NAME.like(&format!("%{q}%")))
                    .where_(Customer::EMAIL.like(&format!("%{q}%")))
            })
            .order_by(Customer::NAME)
            .limit(20)
            .get(&db)
            .await?
    };
    // [/explain:rentals.walkin.options]
    Ok(Json(
        customers
            .iter()
            .map(|c| {
                SelectOption::new(
                    c.id,
                    match &c.email {
                        Some(email) => format!("{} · {email}", c.name),
                        None => c.name.clone(),
                    },
                )
            })
            .collect(),
    ))
}

/// `GET /staff/rentals/walk-in` (`rentals.walkin`): a rental for someone
/// at the counter without a reservation: a verified customer, a period
/// starting now, and a bike standing here and free for it.
pub async fn walk_in(State(state): State<AppState>) -> Result<View> {
    let store = active_store()?;
    let start = renox::db::now();
    let end = start + Duration::hours(2);
    let offers = booking::free_bikes(&state.db, store, start, end, None).await?;
    let names = super::reserve::variant_names(
        &state.db,
        offers.iter().map(|(b, _)| b.variant_id).collect(),
    )
    .await?;
    let bikes: Vec<(i64, String)> = offers
        .iter()
        .map(|(bike, _)| {
            let (model, size) = names.get(&bike.variant_id).cloned().unwrap_or_default();
            (
                bike.id,
                format!(
                    "{model} {} · {} · {}/h",
                    size.unwrap_or_default(),
                    bike.frame_number,
                    money(&state, bike.hourly_rate)
                ),
            )
        })
        .collect();
    let format = "%Y-%m-%dT%H:%M";
    Ok(view(
        "rentals/walk_in.html",
        context! {
            bikes,
            starts_at => to_local(&state.config, start).format(format).to_string(),
            ends_at => to_local(&state.config, end).format(format).to_string(),
            today => to_local(&state.config, start).date().to_string(),
        },
    ))
}

/// The walk-in form.
#[derive(Deserialize, Debug)]
pub struct WalkInForm {
    #[serde(default)]
    pub customer: i64,
    #[serde(default)]
    pub bike: i64,
    pub starts_at: NaiveDateTime,
    pub ends_at: NaiveDateTime,
}

// [explain:rentals.walkin.form]
impl Validate for WalkInForm {
    fn rules(&self, _v: &mut Validator) {}

    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let lang = form.state.current_lang();
        let db = &form.state.db;
        match Customer::find(db, self.customer).await? {
            Some(c) if c.id_verified() => {}
            Some(_) => errors.add("customer", lang.t("rentals.errors.unverified", &[])),
            None => errors.add("customer", lang.t("rentals.errors.pick_customer", &[])),
        }
        let config = &form.state.config;
        let (start, end) = (
            booking::from_local(config, self.starts_at),
            booking::from_local(config, self.ends_at),
        );
        if let Some(key) = pricing::period_problem(start, end, renox::db::now()) {
            errors.add("ends_at", lang.t(key, &[]));
            return Ok(());
        }
        let store = access::active_store::current().unwrap_or_default();
        match RentalBike::find(db, self.bike).await? {
            Some(bike) if bike.location_store_id == store && rentable(bike.status) => {
                if clashing(bike.id, start, end).exists(db).await? {
                    errors.add("bike", lang.t("rentals.errors.taken", &[]));
                }
            }
            _ => errors.add("bike", lang.t("rentals.errors.pick_bike", &[])),
        }
        Ok(())
    }
}
// [/explain:rentals.walkin.form]

/// `POST /staff/rentals/walk-in` (`rentals.walkin.store`): books the bike
/// in the same transaction as a reservation (served by this person, in the
/// active store), then opens it at the desk for the pick-up.
pub async fn walk_in_store(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<WalkInForm>,
) -> Result<Redirect> {
    let store = active_store()?;
    let staff = staff_id(&state.db, &user).await?;
    let booked = booking::book(
        &state.db,
        NewRental {
            bike_id: form.bike,
            customer_id: form.customer,
            operating_store_id: store,
            start: booking::from_local(&state.config, form.starts_at),
            end: booking::from_local(&state.config, form.ends_at),
            served_by: staff,
        },
    )
    .await?;
    match booked {
        Ok(rental) => Redirect::route("rentals.desk", &[&rental.id]),
        Err(refusal) => {
            let mut errors = Errors::new();
            errors.add("bike", state.current_lang().t(refusal.key(), &[]));
            Err(errors.into())
        }
    }
}
