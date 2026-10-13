//! Factories for stores, staff and help between stores.

use renox::chrono::{Duration, NaiveDate};
use renox::db::{FactoryBuilder, Json};
use renox::fake::Fake;
use renox::fake::faker::phone_number::en::PhoneNumber;
use renox::prelude::*;

use super::model::{HelpStatus, OpeningHours, Staff, StaffHelpHour, StaffHelpRequest, Store};
use crate::app::access::catalogue;
use crate::seed::{today, unique};

/// Monday to Saturday 08:00–19:00, Sunday 09:00–15:00.
pub fn usual_hours() -> Vec<OpeningHours> {
    ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
        .iter()
        .map(|day| OpeningHours {
            day: (*day).to_owned(),
            opens: if *day == "sun" { "09:00" } else { "08:00" }.to_owned(),
            closes: if *day == "sun" { "15:00" } else { "19:00" }.to_owned(),
        })
        .collect()
}

impl Factory for Store {
    fn definition() -> Self {
        let n = unique();
        Store {
            name: format!("Store {n}"),
            slug: format!("store-{n}"),
            phone: PhoneNumber().fake(),
            email: format!("store{n}@bikeshop.test"),
            opening_hours: Json(usual_hours()),
            workshop_minutes_per_day: 960,
            fee_rate_bp: 2_000,
            ..Default::default()
        }
    }
}

/// `Store::factory()` at `address_id`.
pub fn stores_at(address_id: i64) -> FactoryBuilder<Store> {
    Store::factory().state(move |s| s.address_id = address_id)
}

impl Factory for Staff {
    fn definition() -> Self {
        Staff {
            phone: Some(PhoneNumber().fake()),
            hired_on: Some(today() - Duration::days((30..2_000).fake::<i64>())),
            active: true,
            ..Default::default()
        }
    }
}

/// States of a staff member.
pub trait StaffStates {
    /// The user `user_id`, working at `store_id`.
    fn of(self, user_id: i64, store_id: i64) -> Self;
    /// No longer working here.
    fn inactive(self) -> Self;
}

impl StaffStates for FactoryBuilder<Staff> {
    fn of(self, user_id: i64, store_id: i64) -> Self {
        self.state(move |s| {
            s.user_id = user_id;
            s.home_store_id = store_id;
        })
    }

    fn inactive(self) -> Self {
        self.state(|s| s.active = false)
    }
}

impl Factory for StaffHelpRequest {
    fn definition() -> Self {
        let start = renox::db::now() + Duration::days(1);
        StaffHelpRequest {
            role: catalogue::STAFF.to_owned(),
            starts_at: start,
            ends_at: start + Duration::days(5),
            reason: "Short of people while a colleague is on leave.".into(),
            status: HelpStatus::Requested,
            ..Default::default()
        }
    }
}

/// States of a help request.
pub trait HelpStates {
    /// `staff_id` from `from_store` helps `to_store`.
    fn lending(self, staff_id: i64, from_store: i64, to_store: i64) -> Self;
    /// Approved by `user_id`.
    fn approved_by(self, user_id: i64) -> Self;
    /// From this Monday 00:00 to next Monday 00:00 (UTC).
    fn this_week(self) -> Self;
}

impl HelpStates for FactoryBuilder<StaffHelpRequest> {
    fn lending(self, staff_id: i64, from_store: i64, to_store: i64) -> Self {
        self.state(move |r| {
            r.staff_id = staff_id;
            r.from_store_id = from_store;
            r.to_store_id = to_store;
        })
    }

    fn approved_by(self, user_id: i64) -> Self {
        self.state(move |r| {
            r.status = HelpStatus::Approved;
            r.approved_by = Some(user_id);
            r.decided_at = Some(renox::db::now() - Duration::days(2));
        })
    }

    fn this_week(self) -> Self {
        self.state(|r| {
            let (start, end) = this_week();
            r.starts_at = start;
            r.ends_at = end;
        })
    }
}

/// This week, Monday 00:00 to the next Monday 00:00, in UTC.
pub fn this_week() -> (DateTime, DateTime) {
    use renox::chrono::Datelike;
    let today = today();
    let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let start = midnight(monday);
    (start, start + Duration::days(7))
}

/// 00:00 UTC on `day`.
pub fn midnight(day: NaiveDate) -> DateTime {
    day.and_hms_opt(0, 0, 0).expect("midnight").and_utc()
}

impl Factory for StaffHelpHour {
    fn definition() -> Self {
        StaffHelpHour {
            worked_on: today(),
            minutes: (4..9).fake::<i64>() * 60,
            ..Default::default()
        }
    }
}
