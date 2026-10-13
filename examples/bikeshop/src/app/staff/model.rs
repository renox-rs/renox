//! Stores and the people who work in them (Pagila's `store` and `staff`).
//!
//! **No role column on `staff`.** What someone may do comes from the
//! `Permissions` module's roles, given *in a store* (`assign_role_in` with
//! `Scope::of(&store)`, #244), with optional dates; the owner has a global
//! role. A row here only says who works for the company, their home store
//! and whether they are active. See `src/app/access` for the permission
//! catalogue and the policy helpers.
//!
//! Migration: `migrations/20260101000200_create_stores_and_staff_tables.*`.

use renox::db::Json;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// One day's opening hours, e.g. `{ "day": "mon", "opens": "09:00", "closes": "19:00" }`.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
pub struct OpeningHours {
    /// `mon`…`sun`.
    pub day: String,
    /// `HH:MM`, local time (`APP_TIMEZONE`).
    pub opens: String,
    /// `HH:MM`.
    pub closes: String,
}

/// A store of the shop. Three of them work together (#245).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "stores")]
pub struct Store {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub address_id: i64,
    pub phone: String,
    pub email: String,
    /// The week's opening hours, stored as JSON (a list keeps the order).
    pub opening_hours: Json<Vec<OpeningHours>>,
    /// How many minutes of mechanic time the workshop has per day (for
    /// booking services).
    pub workshop_minutes_per_day: i64,
    /// The fee this store earns for work done for another store (a rental
    /// of another store's bike, a sale of consigned goods), in basis
    /// points: 2000 = 20 %. Changed only by the owner (`settings.fees`).
    pub fee_rate_bp: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl Store {
    /// The fee rate as a percentage, for display: `20.0`.
    pub fn fee_rate_percent(&self) -> f64 {
        self.fee_rate_bp as f64 / 100.0
    }

    /// `amount × fee rate`, rounded half up to the smallest unit.
    pub fn fee_on(&self, amount: i64) -> i64 {
        fee(amount, self.fee_rate_bp)
    }

    /// Every store, by name.
    pub async fn all_by_name(db: &Db) -> Result<Vec<Store>> {
        Store::query().order_by(Store::NAME).get(db).await
    }
}

/// `amount × rate_bp / 10 000`, rounded half up (integers only: money is
/// never a float).
pub fn fee(amount: i64, rate_bp: i64) -> i64 {
    (amount * rate_bp + 5_000).div_euclid(10_000)
}

/// Someone who works for the shop: a user with a home store. Their roles
/// live in `role_user` (per store, with dates).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "staff")]
pub struct Staff {
    pub id: i64,
    pub user_id: i64,
    /// The store they normally work in (the store switcher starts there).
    pub home_store_id: i64,
    pub phone: Option<String>,
    pub hired_on: Option<renox::chrono::NaiveDate>,
    /// Deactivated staff can't work anywhere (and are logged out).
    pub active: bool,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl Staff {
    /// The staff record of a user, if they work here.
    pub async fn of_user(db: &Db, user_id: i64) -> Result<Option<Staff>> {
        Staff::query()
            .where_(Staff::USER_ID.eq(user_id))
            .first(db)
            .await
    }
}

/// Where a help request stands.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpStatus {
    /// Asked by the store that needs help, waiting for the lending store.
    #[default]
    Requested,
    /// Approved: the helper has a dated role in the store they help.
    Approved,
    /// The lending store (or the owner) said no.
    Refused,
    /// The asking store took the request back.
    Withdrawn,
    /// Ended before its end date, by either manager or the owner.
    EndedEarly,
}

/// A store short of people asks another store to lend someone for some
/// days (#245). Approved, it becomes a role in the helped store with start
/// and end dates (`assign_role_in(…).from(…).until(…)`), which ends by
/// itself.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "staff_help_requests")]
pub struct StaffHelpRequest {
    pub id: i64,
    /// The helper's store, which lends them.
    pub from_store_id: i64,
    /// The store that asked for help, where the helper will work.
    pub to_store_id: i64,
    /// The person lent.
    pub staff_id: i64,
    /// The role they'll have in the helped store (a role's name from the
    /// access catalogue, e.g. `access::catalogue::STAFF`).
    pub role: String,
    pub starts_at: DateTime,
    pub ends_at: DateTime,
    pub reason: String,
    pub status: HelpStatus,
    /// The user who asked (the helped store's manager).
    pub requested_by: Option<i64>,
    /// The user who approved or refused it.
    pub approved_by: Option<i64>,
    pub decided_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// Hours someone worked while helping another store: recorded for
/// reports, never charged between stores (the owner's decision, #245).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "staff_help_hours")]
pub struct StaffHelpHour {
    pub id: i64,
    pub help_request_id: i64,
    pub staff_id: i64,
    /// The store helped.
    pub store_id: i64,
    pub worked_on: renox::chrono::NaiveDate,
    pub minutes: i64,
    pub note: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}
