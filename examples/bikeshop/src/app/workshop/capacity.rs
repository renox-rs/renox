//! The workshop's day: what a booking costs and takes, and whether a
//! store's workshop has room for it.
//!
//! Each store has `workshop_minutes_per_day` of mechanic time. A booking
//! takes the minutes of its tasks; a day is **full** for a booking when the
//! minutes already booked there plus the booking's own would pass the
//! store's capacity. Days the store is closed (missing from its
//! `opening_hours`) can't be booked either.
//!
//! The rule is checked twice, like the rentals' overlap rule: in the booking
//! form's `after` hook (the error shows next to the day; the date picker
//! already greys the full days out) and again in the transaction that
//! writes the work order ([`book`]), which takes the store's row first so
//! two bookings of the last slot run one after the other.

use renox::chrono::{Datelike, Duration, NaiveDate};
use renox::db::Transaction;
use renox::prelude::*;
use serde::Serialize;

use super::model::{CustomerBike, ServiceTask, WorkOrder, WorkOrderTask, WorkSource, WorkStatus};
use crate::app::rentals::booking::{from_local, to_local};
use crate::app::staff::model::Store;

/// The packages offered when booking: a name and the tasks it bundles
/// (by slug).
pub const PACKAGES: [(&str, &[&str]); 2] = [
    (
        "tune-up",
        &[
            "safety-check",
            "brake-adjustment",
            "gear-indexing",
            "chain-clean",
        ],
    ),
    (
        "overhaul",
        &[
            "full-service",
            "wheel-truing",
            "brake-bleed",
            "chain-replacement",
        ],
    ),
];

/// How many days ahead a service can be booked.
pub const BOOK_AHEAD_DAYS: i64 = 60;

/// The tasks of a booking: the package's and the ones ticked, once each.
pub fn chosen_tasks(
    all: &[ServiceTask],
    package: Option<&str>,
    ticked: &[i64],
) -> Vec<ServiceTask> {
    let slugs: &[&str] = package
        .and_then(|p| PACKAGES.iter().find(|(key, _)| *key == p))
        .map(|(_, slugs)| *slugs)
        .unwrap_or(&[]);
    all.iter()
        .filter(|t| slugs.contains(&t.slug.as_str()) || ticked.contains(&t.id))
        .cloned()
        .collect()
}

/// A booking's price and time, shown before confirming.
#[derive(Serialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Estimate {
    pub minutes: i64,
    pub price: i64,
}

/// The estimate of `tasks`.
pub fn estimate(tasks: &[ServiceTask]) -> Estimate {
    Estimate {
        minutes: tasks.iter().map(|t| t.minutes).sum(),
        price: tasks.iter().map(|t| t.price).sum(),
    }
}

/// The moments a local day of `APP_TIMEZONE` starts and ends.
pub fn day_bounds(config: &Config, day: NaiveDate) -> (DateTime, DateTime) {
    let start = from_local(config, day.and_hms_opt(0, 0, 0).unwrap_or_default());
    let end = from_local(
        config,
        (day + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default(),
    );
    (start, end)
}

/// When a booked bike is expected: the day at 09:00, local time.
pub fn drop_off(config: &Config, day: NaiveDate) -> DateTime {
    from_local(config, day.and_hms_opt(9, 0, 0).unwrap_or_default())
}

/// The weekdays a store is closed (0 is Sunday … 6 Saturday): those missing
/// from its opening hours. A store with no hours set is open every day.
pub fn closed_weekdays(store: &Store) -> Vec<u32> {
    let open: Vec<&str> = store.opening_hours.iter().map(|h| h.day.as_str()).collect();
    if open.is_empty() {
        return Vec::new();
    }
    ["sun", "mon", "tue", "wed", "thu", "fri", "sat"]
        .iter()
        .enumerate()
        .filter(|(_, day)| !open.contains(day))
        .map(|(n, _)| n as u32)
        .collect()
}

/// Minutes already booked at `store_id` on the local `day` (cancelled work
/// orders don't count).
pub async fn booked_minutes<'c, E: renox::db::Executor<'c>>(
    db: E,
    config: &Config,
    store_id: i64,
    day: NaiveDate,
) -> Result<i64> {
    let (start, end) = day_bounds(config, day);
    let minutes: i64 = WorkOrder::query()
        .where_(WorkOrder::STORE_ID.eq(store_id))
        .where_(WorkOrder::STATUS.ne(WorkStatus::Cancelled))
        .where_(WorkOrder::SCHEDULED_FOR.gte(start))
        .where_(WorkOrder::SCHEDULED_FOR.lt(end))
        .sum(db, "minutes")
        .await?;
    Ok(minutes)
}

/// The days from today on (for [`BOOK_AHEAD_DAYS`]) that are full at
/// `store` for a booking of `needed` minutes, as `YYYY-MM-DD`, for the
/// date picker. Two queries: the work orders of the period, then nothing
/// per day (summed in Rust).
pub async fn full_days(
    db: &Db,
    config: &Config,
    store: &Store,
    needed: i64,
) -> Result<Vec<String>> {
    let today = to_local(config, renox::db::now()).date();
    let last = today + Duration::days(BOOK_AHEAD_DAYS);
    let (from, _) = day_bounds(config, today);
    let (_, to) = day_bounds(config, last);
    let orders: Vec<(DateTime, i64)> = WorkOrder::query()
        .where_(WorkOrder::STORE_ID.eq(store.id))
        .where_(WorkOrder::STATUS.ne(WorkStatus::Cancelled))
        .where_(WorkOrder::SCHEDULED_FOR.gte(from))
        .where_(WorkOrder::SCHEDULED_FOR.lt(to))
        .select_as(db, "scheduled_for, minutes")
        .await?;
    let mut per_day: std::collections::HashMap<NaiveDate, i64> = Default::default();
    for (at, minutes) in orders {
        *per_day.entry(to_local(config, at).date()).or_default() += minutes;
    }
    let mut full: Vec<String> = per_day
        .into_iter()
        .filter(|(_, booked)| booked + needed.max(1) > store.workshop_minutes_per_day)
        .map(|(day, _)| day.to_string())
        .collect();
    full.sort();
    Ok(full)
}

/// Why a day can't take a booking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayProblem {
    Past,
    TooFar,
    Closed,
    Full,
}

impl DayProblem {
    /// The translation key of the message.
    pub fn key(self) -> &'static str {
        match self {
            DayProblem::Past => "workshop.errors.past",
            DayProblem::TooFar => "workshop.errors.too_far",
            DayProblem::Closed => "workshop.errors.closed",
            DayProblem::Full => "workshop.errors.full",
        }
    }
}

/// Whether `day` can take `minutes` more at `store` (one query).
pub async fn check_day<'c, E: renox::db::Executor<'c>>(
    db: E,
    config: &Config,
    store: &Store,
    day: NaiveDate,
    minutes: i64,
) -> Result<Option<DayProblem>> {
    let today = to_local(config, renox::db::now()).date();
    if day < today {
        return Ok(Some(DayProblem::Past));
    }
    if day > today + Duration::days(BOOK_AHEAD_DAYS) {
        return Ok(Some(DayProblem::TooFar));
    }
    if closed_weekdays(store).contains(&day.weekday().num_days_from_sunday()) {
        return Ok(Some(DayProblem::Closed));
    }
    let booked = booked_minutes(db, config, store.id, day).await?;
    Ok((booked + minutes > store.workshop_minutes_per_day).then_some(DayProblem::Full))
}

/// A new work order to book.
#[derive(Debug, Clone)]
pub struct NewBooking {
    pub bike_id: i64,
    pub store_id: i64,
    pub day: NaiveDate,
    pub tasks: Vec<ServiceTask>,
    pub package: Option<String>,
    pub note: Option<String>,
    pub source: WorkSource,
    /// Checked in at once (a walk-in today).
    pub checked_in: bool,
}

/// Books `new` in one transaction: takes the store's row, checks the day
/// again, then writes the work order and its tasks. `Ok(Err(problem))`
/// when the day filled up meanwhile.
pub async fn book(
    db: &Db,
    config: &Config,
    new: NewBooking,
) -> Result<std::result::Result<WorkOrder, DayProblem>> {
    let mut tx = db.begin_immediate().await?;
    let result = book_in(&mut tx, config, &new).await?;
    if result.is_ok() {
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(result)
}

// [explain:workshop.book.capacity]
async fn book_in(
    tx: &mut Transaction,
    config: &Config,
    new: &NewBooking,
) -> Result<std::result::Result<WorkOrder, DayProblem>> {
    let store = Store::query()
        .where_(Store::ID.eq(new.store_id))
        .lock_for_update()
        .first(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    let needed = estimate(&new.tasks);
    if let Some(problem) = check_day(&mut *tx, config, &store, new.day, needed.minutes).await? {
        return Ok(Err(problem));
    }
    // [/explain:workshop.book.capacity]
    let bike = CustomerBike::find_or_404(&mut *tx, new.bike_id).await?;
    let order = WorkOrder::create(
        &mut *tx,
        WorkOrder {
            customer_bike_id: Some(bike.id),
            store_id: store.id,
            source: new.source,
            scheduled_for: drop_off(config, new.day),
            status: if new.checked_in {
                WorkStatus::CheckedIn
            } else {
                WorkStatus::Booked
            },
            labour: needed.price,
            total: needed.price,
            minutes: needed.minutes,
            package: new.package.clone(),
            customer_note: new.note.clone(),
            ..Default::default()
        },
    )
    .await?;
    for task in &new.tasks {
        WorkOrderTask::create(
            &mut *tx,
            WorkOrderTask {
                work_order_id: order.id,
                service_task_id: task.id,
                minutes: task.minutes,
                price: task.price,
                ..Default::default()
            },
        )
        .await?;
    }
    Ok(Ok(order))
}

/// Moves a booked work order to `day`, in one transaction that takes the
/// store's row first (like [`book`]); its own minutes don't count against
/// its current day. `Some(problem)` when the day can't take it (nothing
/// changes). Rescheduling a booking (customers) and moving a plan's visit
/// (#237) both go through it.
pub async fn move_booking(
    db: &Db,
    config: &Config,
    order: &mut WorkOrder,
    day: NaiveDate,
) -> Result<Option<DayProblem>> {
    let mut tx = db.begin_immediate().await?;
    let store = Store::query()
        .where_(Store::ID.eq(order.store_id))
        .lock_for_update()
        .first(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;
    let mut problem = check_day(&mut tx, config, &store, day, order.minutes).await?;
    let current_day = to_local(config, order.scheduled_for).date();
    if problem == Some(DayProblem::Full) && day == current_day {
        problem = None;
    }
    if problem.is_some() {
        tx.rollback().await?;
        return Ok(problem);
    }
    order.scheduled_for = drop_off(config, day);
    order.reminded_at = None;
    order
        .save_only(&mut tx, &["scheduled_for", "reminded_at"])
        .await?;
    tx.commit().await?;
    Ok(None)
}
