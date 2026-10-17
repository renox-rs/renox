//! The rentals' scheduled tasks, registered from the module's `register`
//! (`schedule:list` shows them; `schedule:run rentals:watch` runs one now).
//!
//! - `rentals:watch`, every 15 minutes on the clock (:00, :15, :30, :45):
//!   1. unpaid reservations past their payment window are called off;
//!   2. reservations not picked up half an hour after their start are
//!      no-shows: the bike is released and the deposit rule applied;
//!   3. rentals ending within the hour get their reminder (once);
//!   4. rentals past their due time become **overdue**: the customer, the
//!      operating store's staff and the owner store's are told once, and the
//!      late fee grows on every run;
//!   5. the fleet board's "reserved" follows the pick-ups of the next two
//!      hours.
//! - `rentals:service`, daily at 06:00: bikes whose ridden hours reached
//!   their service interval go to the workshop (`FleetRepairNeeded`).
//!
//! Each step is a plain `async fn(&AppState)`, so tests call them directly
//! after `TestApp::travel`, and `app.run_scheduled("rentals:watch")`
//! runs them all.

use renox::chrono::Duration;
use renox::prelude::*;
use renox::schedule::Schedule;
use renox::state_machine::{StateMachine, TransitionError};

use super::FleetRepairNeeded;
use super::booking::to_local;
use super::model::{BikeStatus, DepositStatus, Rental, RentalBike, RentalStatus};
use super::notify::{self, Notice, Tone};
use super::pricing::{self, late_fee, no_show_fee};
use super::reserve::money;
use crate::app::access::catalogue;
use crate::app::accounts::model::Customer;
use crate::app::accounts::preferences::Kind;

/// Registers the tasks.
pub fn schedule(s: &mut Schedule) {
    s.every_minutes(15, "rentals:watch", |state: AppState| async move {
        watch(&state).await
    });
    s.daily_at("06:00", "rentals:service", |state: AppState| async move {
        service_due(&state).await.map(|_| ())
    });
}

/// Every step of `rentals:watch`, in order.
pub async fn watch(state: &AppState) -> Result {
    lapse_unpaid(state).await?;
    no_shows(state).await?;
    reminders(state).await?;
    overdue(state).await?;
    mark_reserved(state).await
}

/// Reservations made online whose deposit wasn't paid within
/// [`pricing::PAYMENT_WINDOW_MINUTES`] are called off.
pub async fn lapse_unpaid(state: &AppState) -> Result<u64> {
    let before = renox::db::now() - Duration::minutes(pricing::PAYMENT_WINDOW_MINUTES);
    let lapsed = Rental::query()
        .where_(Rental::STATUS.eq(RentalStatus::Reserved))
        .where_(Rental::DEPOSIT_STATUS.eq(DepositStatus::Unpaid))
        .where_(Rental::SERVED_BY.is_null())
        .where_(Rental::CREATED_AT.lt(before))
        .get(&state.db)
        .await?;
    for mut rental in lapsed.iter().cloned() {
        rental.cancelled_at = Some(renox::db::now());
        let mut tx = state.db.begin().await?;
        rental.save(&mut tx).await?;
        let moved = rental
            .transition("lapse")
            .by_system()
            .reason("the deposit was not paid in time")
            .run_in(&mut tx)
            .await
            .map_err(TransitionError::into_error)?;
        tx.commit().await?;
        rental.status = RentalStatus::Cancelled;
        moved.emit(state).await?;
        if let Some(customer) = Customer::find(&state.db, rental.customer_id).await? {
            notify::customer(
                state,
                &customer,
                Kind::Rental,
                &Notice::new(
                    "rental-unpaid",
                    "rentals.mail.unpaid.title",
                    "rentals.mail.unpaid.body",
                )
                .param("code", &rental.reservation_code)
                .tone(Tone::Warning)
                .url(super::link(state, "rentals.create", None::<i64>)?),
            )
            .await?;
        }
    }
    Ok(lapsed.len() as u64)
}

/// Reservations not picked up [`pricing::NO_SHOW_AFTER_MINUTES`] after
/// their start: a no-show. The deposit keeps the rental's price (never
/// more than the deposit) and the rest is given back; the bike is free.
pub async fn no_shows(state: &AppState) -> Result<u64> {
    let before = renox::db::now() - Duration::minutes(pricing::NO_SHOW_AFTER_MINUTES);
    let missed = Rental::query()
        .where_(Rental::STATUS.eq(RentalStatus::Reserved))
        .where_(Rental::STARTS_AT.lt(before))
        .get(&state.db)
        .await?;
    for mut rental in missed.iter().cloned() {
        let mut kept = 0;
        if rental.deposit_status == DepositStatus::Held {
            kept = no_show_fee(rental.price, rental.deposit);
            rental.deposit_refunded = rental.deposit - kept;
            rental.deposit_status = DepositStatus::Forfeited;
        }
        let mut tx = state.db.begin().await?;
        rental.save(&mut tx).await?;
        let moved = rental
            .transition("no_show")
            .by_system()
            .run_in(&mut tx)
            .await
            .map_err(TransitionError::into_error)?;
        tx.commit().await?;
        rental.status = RentalStatus::NoShow;
        moved.emit(state).await?;
        RentalBike::query()
            .where_(RentalBike::ID.eq(rental.rental_bike_id))
            .where_(RentalBike::STATUS.eq(BikeStatus::Reserved))
            .update(&state.db, &[("status", &BikeStatus::Available)])
            .await?;
        if let Some(customer) = Customer::find(&state.db, rental.customer_id).await? {
            notify::customer(
                state,
                &customer,
                Kind::Rental,
                &Notice::new(
                    "rental-no-show",
                    "rentals.mail.no_show.title",
                    "rentals.mail.no_show.body",
                )
                .param("code", &rental.reservation_code)
                .row("rentals.fields.kept", money(state, kept))
                .row(
                    "rentals.fields.refund",
                    money(state, rental.deposit_refunded),
                )
                .tone(Tone::Warning)
                .url(super::link(
                    state,
                    "rentals.show",
                    Some(&rental.reservation_code),
                )?),
            )
            .await?;
        }
    }
    Ok(missed.len() as u64)
}

/// Rentals ending within [`pricing::REMIND_BEFORE_MINUTES`]: a mail and an
/// in-app notification, once.
pub async fn reminders(state: &AppState) -> Result<u64> {
    let now = renox::db::now();
    let due = Rental::query()
        .where_(Rental::STATUS.eq(RentalStatus::Active))
        .where_(Rental::REMINDED_AT.is_null())
        .where_(Rental::DUE_AT.gt(now))
        .where_(Rental::DUE_AT.lte(now + Duration::minutes(pricing::REMIND_BEFORE_MINUTES)))
        .get(&state.db)
        .await?;
    for mut rental in due.iter().cloned() {
        rental.reminded_at = Some(now);
        rental.save_only(&state.db, &["reminded_at"]).await?;
        if let Some(customer) = Customer::find(&state.db, rental.customer_id).await? {
            notify::customer(
                state,
                &customer,
                Kind::Rental,
                &Notice::new(
                    "rental-reminder",
                    "rentals.mail.reminder.title",
                    "rentals.mail.reminder.body",
                )
                .param("code", &rental.reservation_code)
                .param(
                    "time",
                    to_local(&state.config, rental.due_at).format("%H:%M"),
                )
                .url(super::link(
                    state,
                    "rentals.show",
                    Some(&rental.reservation_code),
                )?),
            )
            .await?;
        }
    }
    Ok(due.len() as u64)
}

/// Rentals past their due time. The first time: status `overdue`, the bike
/// `overdue` on the board, and the customer, the operating store's staff
/// and (when it's another) the owner store's staff are told. Every time:
/// the late fee is brought up to now.
pub async fn overdue(state: &AppState) -> Result<u64> {
    let now = renox::db::now();
    let late = Rental::query()
        .where_(Rental::STATUS.is_in([RentalStatus::Active, RentalStatus::Overdue]))
        .where_(Rental::DUE_AT.lt(now))
        .get(&state.db)
        .await?;
    let bike_ids: Vec<i64> = late.iter().map(|r| r.rental_bike_id).collect();
    let bikes = RentalBike::find_many(&state.db, bike_ids).await?;
    let mut newly = 0;
    for mut rental in late {
        let hourly = bikes
            .iter()
            .find(|b| b.id == rental.rental_bike_id)
            .map(|b| b.hourly_rate)
            .unwrap_or_default();
        let first_time = rental.status == RentalStatus::Active;
        rental.late_fee = late_fee(hourly, rental.due_at, now);
        if !first_time {
            rental.save(&state.db).await?;
            continue;
        }
        let mut tx = state.db.begin().await?;
        rental.save(&mut tx).await?;
        let moved = rental
            .transition("overdue")
            .by_system()
            .run_in(&mut tx)
            .await
            .map_err(TransitionError::into_error)?;
        tx.commit().await?;
        rental.status = RentalStatus::Overdue;
        moved.emit(state).await?;
        newly += 1;
        RentalBike::query()
            .where_(RentalBike::ID.eq(rental.rental_bike_id))
            .update(&state.db, &[("status", &BikeStatus::Overdue)])
            .await?;
        let url = super::link(state, "rentals.desk", Some(&rental.id))?;
        let customer = Customer::find(&state.db, rental.customer_id).await?;
        let name = customer
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        if let Some(customer) = &customer {
            notify::customer(
                state,
                customer,
                Kind::Rental,
                &Notice::new(
                    "rental-overdue",
                    "rentals.mail.overdue.title",
                    "rentals.mail.overdue.body",
                )
                .param("code", &rental.reservation_code)
                .row("rentals.fields.late_fee", money(state, rental.late_fee))
                .tone(Tone::Warning)
                .url(super::link(
                    state,
                    "rentals.show",
                    Some(&rental.reservation_code),
                )?),
            )
            .await?;
        }
        let mut stores = vec![rental.operating_store_id];
        if rental.owner_store_id != rental.operating_store_id {
            stores.push(rental.owner_store_id);
        }
        notify::staff(
            state,
            catalogue::RENTALS_VIEW,
            &stores,
            &Notice::new(
                "rental-overdue-staff",
                "rentals.mail.overdue_staff.title",
                "rentals.mail.overdue_staff.body",
            )
            .param("code", &rental.reservation_code)
            .param("name", name)
            .tone(Tone::Warning)
            .url(url),
        )
        .await?;
    }
    Ok(newly)
}

/// The fleet board's "reserved": a bike standing available with a pick-up
/// in the next [`pricing::RESERVED_AHEAD_MINUTES`] is shown reserved; a
/// reserved one without is available again.
pub async fn mark_reserved(state: &AppState) -> Result {
    let now = renox::db::now();
    let soon: Vec<i64> = Rental::query()
        .where_(Rental::STATUS.eq(RentalStatus::Reserved))
        .where_(Rental::STARTS_AT.lt(now + Duration::minutes(pricing::RESERVED_AHEAD_MINUTES)))
        .pluck(&state.db, Rental::RENTAL_BIKE_ID)
        .await?;
    RentalBike::query()
        .where_(RentalBike::STATUS.eq(BikeStatus::Available))
        .where_(RentalBike::ID.is_in(soon.clone()))
        .update(&state.db, &[("status", &BikeStatus::Reserved)])
        .await?;
    RentalBike::query()
        .where_(RentalBike::STATUS.eq(BikeStatus::Reserved))
        .where_not_in("id", soon)
        .update(&state.db, &[("status", &BikeStatus::Available)])
        .await?;
    Ok(())
}

/// Bikes standing available whose ridden hours reached their service
/// interval go to the workshop: `maintenance`, and a work order through
/// [`FleetRepairNeeded`] (the workshop area opens it).
pub async fn service_due(state: &AppState) -> Result<u64> {
    let bikes = RentalBike::query()
        .where_(RentalBike::STATUS.eq(BikeStatus::Available))
        .where_raw(
            "ridden_hours - serviced_at_hours >= ?",
            [pricing::SERVICE_EVERY_HOURS],
        )
        .get(&state.db)
        .await?;
    let lang = state.current_lang();
    for bike in &bikes {
        RentalBike::query()
            .where_(RentalBike::ID.eq(bike.id))
            .update(&state.db, &[("status", &BikeStatus::Maintenance)])
            .await?;
        let note = lang.t(
            "rentals.fleet.service_note",
            &[("hours", &bike.ridden_hours)],
        );
        state
            .emit(FleetRepairNeeded {
                bike_id: bike.id,
                rental_id: None,
                note,
            })
            .await?;
    }
    Ok(bikes.len() as u64)
}
