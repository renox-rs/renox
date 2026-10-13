//! The rental rules in plain Rust: prices, late fees, the deposit's
//! settlement and the time limits. No database here, so every rule is a
//! unit test away (`cargo test -p bikeshop --lib pricing`) and the pages,
//! the counter and the scheduled tasks all use the same numbers.
//!
//! Money is an integer in the smallest unit of `APP_CURRENCY` (cents
//! here), never a float.

use renox::chrono::Duration;
use renox::prelude::*;
use serde::Serialize;

use super::model::{RentalBike, RentalRate};

/// Minutes a returned bike may be late before the late fee starts.
pub const GRACE_MINUTES: i64 = 15;
/// A reservation not picked up this many minutes after its start is a
/// no-show (the bike is released).
pub const NO_SHOW_AFTER_MINUTES: i64 = 30;
/// A reservation whose deposit isn't paid this many minutes after it was
/// made is called off (the bike is released).
pub const PAYMENT_WINDOW_MINUTES: i64 = 30;
/// Customers may cancel until this many minutes before the start.
pub const CANCEL_UNTIL_MINUTES: i64 = 60;
/// The reminder goes out this many minutes before the end.
pub const REMIND_BEFORE_MINUTES: i64 = 60;
/// A bike is reserved on the fleet board this many minutes before a
/// pick-up.
pub const RESERVED_AHEAD_MINUTES: i64 = 120;
/// Ridden hours between two services of a rental bike.
pub const SERVICE_EVERY_HOURS: i64 = 200;
/// The longest rental that can be booked.
pub const MAX_DAYS: i64 = 14;

/// What a rental costs, shown before the customer confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Quote {
    /// Hourly when it is shorter than a day, daily otherwise.
    pub rate: RentalRate,
    /// Whole days charged at the daily rate.
    pub days: i64,
    /// Started hours after the whole days, at the hourly rate (capped at
    /// one daily rate).
    pub hours: i64,
    /// The rental's price.
    pub price: i64,
    /// The deposit, held until the bike is back.
    pub deposit: i64,
}

impl Quote {
    /// Price plus deposit: what is paid before riding off.
    pub fn due_now(&self) -> i64 {
        self.price + self.deposit
    }
}

/// The price of renting `bike` from `start` to `end`:
///
/// - every whole day at the daily rate;
/// - the hours after them at the hourly rate, each started hour counted,
///   but never more than one daily rate (four hours of a bike whose hours
///   add up past its day rate cost the day rate);
/// - the bike's deposit on top.
///
/// ```
/// use bikeshop::app::rentals::pricing::quote;
/// use bikeshop::app::rentals::model::RentalBike;
/// use renox::chrono::{Duration, Utc};
///
/// let bike = RentalBike { hourly_rate: 1_500, daily_rate: 6_000, deposit: 30_000, ..Default::default() };
/// let start = Utc::now();
/// // 2 h 10 min: three started hours.
/// assert_eq!(quote(&bike, start, start + Duration::minutes(130)).price, 4_500);
/// // 7 hours would be $105.00: capped at the daily rate.
/// assert_eq!(quote(&bike, start, start + Duration::hours(7)).price, 6_000);
/// // A day and two hours.
/// assert_eq!(quote(&bike, start, start + Duration::hours(26)).price, 9_000);
/// ```
pub fn quote(bike: &RentalBike, start: DateTime, end: DateTime) -> Quote {
    let minutes = (end - start).num_minutes().max(0);
    let days = minutes / (24 * 60);
    let rest = minutes % (24 * 60);
    let hours = (rest + 59) / 60;
    let price = days * bike.daily_rate + (hours * bike.hourly_rate).min(bike.daily_rate);
    Quote {
        rate: if days == 0 {
            RentalRate::Hourly
        } else {
            RentalRate::Daily
        },
        days,
        hours,
        price,
        deposit: bike.deposit,
    }
}

/// The late fee for a bike due at `due` and back at `back`: nothing within
/// [`GRACE_MINUTES`], then the hourly rate for every started hour past the
/// due time (16 minutes late is one hour, 61 minutes two).
pub fn late_fee(hourly_rate: i64, due: DateTime, back: DateTime) -> i64 {
    let late = (back - due).num_minutes();
    if late <= GRACE_MINUTES {
        return 0;
    }
    (late + 59) / 60 * hourly_rate
}

/// How a deposit settles against the fees at the return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Settlement {
    /// Late and damage fees.
    pub fees: i64,
    /// The part of the deposit kept for them.
    pub kept: i64,
    /// The part of the deposit given back.
    pub refund: i64,
    /// What the customer still pays when the fees are more than the deposit.
    pub due: i64,
}

/// Settles `held` (the deposit the operating store holds, 0 when none was
/// paid) against `fees`: the fees come out of the deposit first, the rest
/// is given back, and fees beyond it are paid at the counter.
pub fn settle(held: i64, fees: i64) -> Settlement {
    let kept = held.min(fees);
    Settlement {
        fees,
        kept,
        refund: held - kept,
        due: fees - kept,
    }
}

/// What a no-show keeps of the deposit: the rental's price (the bike stood
/// unused for the customer), never more than the deposit. The rest is given
/// back.
pub fn no_show_fee(price: i64, deposit_held: i64) -> i64 {
    price.min(deposit_held)
}

/// Minutes ridden between pick-up and return.
pub fn ridden_minutes(picked_up: DateTime, back: DateTime) -> i64 {
    (back - picked_up).num_minutes().max(0)
}

/// Whole hours to add to a bike's `ridden_hours` (started hours count).
pub fn ridden_hours(minutes: i64) -> i64 {
    (minutes + 59) / 60
}

/// Whether a bike is due for its periodic service.
pub fn service_due(bike: &RentalBike) -> bool {
    bike.ridden_hours - bike.serviced_at_hours >= SERVICE_EVERY_HOURS
}

/// The period a booking may ask for: ends after it starts, starts in the
/// future (a few minutes of slack for a walk-in typed at the counter), and
/// lasts at most [`MAX_DAYS`].
pub fn period_problem(start: DateTime, end: DateTime, now: DateTime) -> Option<&'static str> {
    if end <= start {
        Some("rentals.errors.order")
    } else if start < now - Duration::minutes(10) {
        Some("rentals.errors.past")
    } else if end - start > Duration::days(MAX_DAYS) {
        Some("rentals.errors.too_long")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use renox::chrono::{TimeZone, Utc};

    fn bike() -> RentalBike {
        RentalBike {
            hourly_rate: 1_500,
            daily_rate: 6_000,
            deposit: 30_000,
            ..Default::default()
        }
    }

    fn at(h: i64, m: i64) -> DateTime {
        Utc.with_ymd_and_hms(2026, 10, 6, 0, 0, 0).unwrap()
            + Duration::hours(h)
            + Duration::minutes(m)
    }

    #[test]
    fn hours_are_counted_when_started_and_capped_at_a_day() {
        let start = at(9, 0);
        assert_eq!(quote(&bike(), start, at(10, 0)).price, 1_500);
        assert_eq!(quote(&bike(), start, at(10, 1)).price, 3_000);
        assert_eq!(quote(&bike(), start, at(14, 0)).price, 6_000);
        let q = quote(&bike(), start, at(9 + 72, 0));
        assert_eq!((q.days, q.hours, q.price), (3, 0, 18_000));
        assert_eq!(q.rate, RentalRate::Daily);
        assert_eq!(quote(&bike(), start, at(11, 0)).rate, RentalRate::Hourly);
        assert_eq!(quote(&bike(), start, at(11, 0)).due_now(), 3_000 + 30_000);
    }

    #[test]
    fn the_late_fee_starts_after_the_grace_period() {
        let due = at(18, 0);
        assert_eq!(late_fee(1_500, due, at(17, 0)), 0);
        assert_eq!(late_fee(1_500, due, at(18, 15)), 0);
        assert_eq!(late_fee(1_500, due, at(18, 16)), 1_500);
        assert_eq!(late_fee(1_500, due, at(19, 0)), 1_500);
        assert_eq!(late_fee(1_500, due, at(19, 1)), 3_000);
    }

    #[test]
    fn the_deposit_settles_against_the_fees() {
        assert_eq!(
            settle(30_000, 0),
            Settlement {
                fees: 0,
                kept: 0,
                refund: 30_000,
                due: 0
            }
        );
        assert_eq!(
            settle(30_000, 8_000),
            Settlement {
                fees: 8_000,
                kept: 8_000,
                refund: 22_000,
                due: 0
            }
        );
        assert_eq!(
            settle(30_000, 36_000),
            Settlement {
                fees: 36_000,
                kept: 30_000,
                refund: 0,
                due: 6_000
            }
        );
        assert_eq!(settle(0, 4_500).due, 4_500);
        assert_eq!(no_show_fee(6_000, 30_000), 6_000);
        assert_eq!(no_show_fee(36_000, 30_000), 30_000);
    }

    #[test]
    fn periods_must_be_in_order_in_the_future_and_not_too_long() {
        let now = at(8, 0);
        assert_eq!(
            period_problem(at(10, 0), at(9, 0), now),
            Some("rentals.errors.order")
        );
        assert_eq!(
            period_problem(at(2, 0), at(9, 0), now),
            Some("rentals.errors.past")
        );
        assert_eq!(
            period_problem(at(10, 0), at(10 + 24 * 15, 0), now),
            Some("rentals.errors.too_long")
        );
        assert_eq!(period_problem(at(10, 0), at(12, 0), now), None);
    }

    #[test]
    fn hours_ridden_and_service_due() {
        assert_eq!(ridden_hours(ridden_minutes(at(9, 0), at(11, 1))), 3);
        let mut b = bike();
        b.ridden_hours = 450;
        b.serviced_at_hours = 260;
        assert!(!service_due(&b));
        b.ridden_hours = 460;
        assert!(service_due(&b));
    }
}
