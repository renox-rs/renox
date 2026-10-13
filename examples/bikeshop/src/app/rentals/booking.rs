//! Availability and booking: which bikes can be rented at a store for a
//! period, and making the rental without ever giving one bike to two
//! customers.
//!
//! **Availability is by location** (#245): the bikes a store rents out are
//! the bikes standing there now (`location_store_id`), whoever owns them.
//! A bike placed at South by North is South's to rent out; the rental
//! remembers North as the owner for the books.
//!
//! **The overlap rule:** a bike can't have two rentals whose periods
//! overlap. Periods touching end to start are fine (one returns at 12:00,
//! the next leaves at 12:00). An overdue rental blocks the bike for any
//! period, since nobody knows when it comes back.
//!
//! The rule is checked twice, on purpose:
//!
//! 1. in the form's validation `after` hook ([`ReserveForm`](super::reserve::ReserveForm)),
//!    so the customer sees "this bike was just taken" next to the field,
//!    with their input kept;
//! 2. again in the transaction that writes the rental ([`book`]), because
//!    between the check and the write another customer may have booked the
//!    same bike. The transaction takes the bike's row first
//!    (`begin_immediate` + `lock_for_update`: SQLite's write lock,
//!    PostgreSQL's row lock), so two bookings of one bike run one after the
//!    other and the second sees the first.

use renox::chrono::Duration;
use renox::db::{Query, Transaction, Ulid};
use renox::prelude::*;

use super::model::{BikeStatus, DepositStatus, Rental, RentalBike, RentalStatus};
use super::pricing::{Quote, quote};

/// The rental statuses that hold a bike for their period.
pub const HOLDING: [RentalStatus; 3] = [
    RentalStatus::Reserved,
    RentalStatus::Active,
    RentalStatus::Overdue,
];

/// The rentals of `bike_id` that clash with `start..end` (touching periods
/// don't clash; an overdue rental clashes with everything).
pub fn clashing(bike_id: i64, start: DateTime, end: DateTime) -> Query<Rental> {
    Rental::query()
        .where_(Rental::RENTAL_BIKE_ID.eq(bike_id))
        .where_any(|any| {
            any.where_(Rental::STATUS.eq(RentalStatus::Overdue))
                .where_all(|all| {
                    all.where_(Rental::STATUS.is_in([RentalStatus::Reserved, RentalStatus::Active]))
                        .where_(Rental::STARTS_AT.lt(end))
                        .where_(Rental::DUE_AT.gt(start))
                })
        })
}

/// Whether a bike in this status may be booked at all.
pub fn rentable(status: BikeStatus) -> bool {
    matches!(
        status,
        BikeStatus::Available | BikeStatus::Reserved | BikeStatus::Rented | BikeStatus::Overdue
    )
}

/// The bikes standing at `store_id` that are free for `start..end`, with
/// their quotes, cheapest first. Two queries: the bikes at the store, then
/// the rentals that clash with the period for any of them.
pub async fn free_bikes(
    db: &Db,
    store_id: i64,
    start: DateTime,
    end: DateTime,
    variant_ids: Option<Vec<i64>>,
) -> Result<Vec<(RentalBike, Quote)>> {
    let mut bikes = RentalBike::query()
        .where_(RentalBike::LOCATION_STORE_ID.eq(store_id))
        .where_not_in(
            "status",
            [
                BikeStatus::Maintenance,
                BikeStatus::InTransit,
                BikeStatus::Retired,
            ],
        )
        .order_by(RentalBike::DAILY_RATE)
        .order_by(RentalBike::ID);
    if let Some(ids) = variant_ids {
        bikes = bikes.where_(RentalBike::VARIANT_ID.is_in(ids));
    }
    let bikes = bikes.get(db).await?;
    let ids: Vec<i64> = bikes.iter().map(|b| b.id).collect();
    let busy: Vec<i64> = Rental::query()
        .where_(Rental::RENTAL_BIKE_ID.is_in(ids))
        .where_any(|any| {
            any.where_(Rental::STATUS.eq(RentalStatus::Overdue))
                .where_all(|all| {
                    all.where_(Rental::STATUS.is_in([RentalStatus::Reserved, RentalStatus::Active]))
                        .where_(Rental::STARTS_AT.lt(end))
                        .where_(Rental::DUE_AT.gt(start))
                })
        })
        .pluck(db, Rental::RENTAL_BIKE_ID)
        .await?;
    Ok(bikes
        .into_iter()
        .filter(|b| !busy.contains(&b.id))
        .map(|b| {
            let q = quote(&b, start, end);
            (b, q)
        })
        .collect())
}

/// What a new rental needs.
#[derive(Debug, Clone)]
pub struct NewRental {
    pub bike_id: i64,
    pub customer_id: i64,
    /// The store serving the customer: where the bike stands now.
    pub operating_store_id: i64,
    pub start: DateTime,
    pub end: DateTime,
    /// Who made it at the counter (a `staff` row), for walk-ins.
    pub served_by: Option<i64>,
}

/// Why a booking was refused in the transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Another rental took the bike for (part of) the period.
    Taken,
    /// The bike isn't at the store any more, or can't be rented now.
    Gone,
}

impl Refusal {
    /// The translation key of the message.
    pub fn key(self) -> &'static str {
        match self {
            Refusal::Taken => "rentals.errors.taken",
            Refusal::Gone => "rentals.errors.gone",
        }
    }
}

/// Books `new` in one transaction: takes the bike's row, checks it is
/// still at the store and free for the period, then writes the rental
/// (reserved, with its quote and a fresh `Ulid` code). The owner store is
/// copied from the bike, so the books never change if the bike moves
/// later. `Ok(Err(refusal))` when the bike was taken meanwhile.
// [explain:rentals.create.book]
pub async fn book(db: &Db, new: NewRental) -> Result<std::result::Result<Rental, Refusal>> {
    let mut tx = db.begin_immediate().await?;
    let result = book_in(&mut tx, &new).await?;
    if result.is_ok() {
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(result)
}
// [/explain:rentals.create.book]

// [explain:rentals.create.book]
async fn book_in(
    tx: &mut Transaction,
    new: &NewRental,
) -> Result<std::result::Result<Rental, Refusal>> {
    let Some(bike) = RentalBike::query()
        .where_(RentalBike::ID.eq(new.bike_id))
        .lock_for_update()
        .first(&mut *tx)
        .await?
    else {
        return Ok(Err(Refusal::Gone));
    };
    if bike.location_store_id != new.operating_store_id || !rentable(bike.status) {
        return Ok(Err(Refusal::Gone));
    }
    if clashing(bike.id, new.start, new.end)
        .exists(&mut *tx)
        .await?
    {
        return Ok(Err(Refusal::Taken));
    }
    // [/explain:rentals.create.book]
    let q = quote(&bike, new.start, new.end);
    let rental = Rental::create(
        &mut *tx,
        Rental {
            reservation_code: Ulid::new(),
            customer_id: new.customer_id,
            rental_bike_id: bike.id,
            owner_store_id: bike.owner_store_id,
            operating_store_id: new.operating_store_id,
            rate: q.rate,
            starts_at: new.start,
            due_at: new.end,
            status: RentalStatus::Reserved,
            price: q.price,
            deposit: q.deposit,
            deposit_status: DepositStatus::Unpaid,
            served_by: new.served_by,
            ..Default::default()
        },
    )
    .await?;
    // [explain:rentals.create.book]
    Ok(Ok(rental))
}
// [/explain:rentals.create.book]

/// The local wall-clock time `local` in `APP_TIMEZONE`, as a moment (UTC).
/// A time skipped by daylight saving time moves an hour on.
pub fn from_local(config: &Config, local: renox::chrono::NaiveDateTime) -> DateTime {
    let zone = &config.timezone;
    let unix = zone
        .resolve(local)
        .or_else(|| zone.resolve(local + Duration::hours(1)))
        .unwrap_or_else(|| local.and_utc().timestamp());
    DateTime::from_timestamp(unix, 0).unwrap_or_default()
}

/// The moment `at` as wall-clock time in `APP_TIMEZONE`.
pub fn to_local(config: &Config, at: DateTime) -> renox::chrono::NaiveDateTime {
    config.timezone.local(at.timestamp())
}
