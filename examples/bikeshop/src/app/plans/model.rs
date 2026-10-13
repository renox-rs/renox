//! Service plans: a set of tasks repeated every week, fortnight, month or
//! quarter, and the customers' bikes subscribed to them. Each visit becomes
//! a work order (`WorkSource::Plan`) made ahead automatically (#237).
//!
//! Migrations: `migrations/20260101000900_create_plans_tables.*` (the
//! plans, their tasks, the subscriptions) and
//! `migrations/20260103000900_add_billing_to_plans.*` (the parts discount,
//! billing, visits and invoices).

use renox::chrono::{Datelike, Duration, Months, NaiveDate};
use renox::db::relations::Pivot;
use renox::prelude::*;
use serde::Serialize;

use crate::app::access::{StoreAttr, StoreRecord, catalogue};

/// How often a plan's visit comes round.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Frequency {
    Weekly,
    Fortnightly,
    #[default]
    Monthly,
    Quarterly,
}

impl Frequency {
    /// Days between two visits (a month counts as 30 here; the scheduler
    /// uses calendar months, [`Frequency::after`]).
    pub fn days(self) -> i64 {
        match self {
            Frequency::Weekly => 7,
            Frequency::Fortnightly => 14,
            Frequency::Monthly => 30,
            Frequency::Quarterly => 91,
        }
    }

    /// The day `n` periods after `start`: `n` weeks or fortnights, or `n`
    /// calendar months or quarters (31 January + 1 month is 28 February).
    pub fn after(self, start: NaiveDate, n: i64) -> NaiveDate {
        let n = n.max(0);
        match self {
            Frequency::Weekly => start + Duration::days(7 * n),
            Frequency::Fortnightly => start + Duration::days(14 * n),
            Frequency::Monthly => start
                .checked_add_months(Months::new(n as u32))
                .unwrap_or(start),
            Frequency::Quarterly => start
                .checked_add_months(Months::new(3 * n as u32))
                .unwrap_or(start),
        }
    }

    /// Visits in a year: 52, 26, 12 or 4.
    pub fn visits_a_year(self) -> i64 {
        match self {
            Frequency::Weekly => 52,
            Frequency::Fortnightly => 26,
            Frequency::Monthly => 12,
            Frequency::Quarterly => 4,
        }
    }
}

/// A service plan customers subscribe a bike to.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "service_plans")]
pub struct ServicePlan {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub frequency: Frequency,
    /// Per visit, in the smallest unit of `APP_CURRENCY`. Every plan is
    /// billed monthly ([`ServicePlan::monthly_price`]).
    pub price: i64,
    pub description: String,
    pub active: bool,
    /// The discount on spare parts while subscribed, in basis points
    /// (1000 = 10 %): at checkout, at the counter (#234) and on the plan's
    /// work orders.
    pub parts_discount_bp: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl ServicePlan {
    /// What the plan costs a month: the visits of a year at its price,
    /// spread over twelve months and rounded to a whole dollar (a weekly
    /// plan at $8.00 a visit is $35.00 a month).
    pub fn monthly_price(&self) -> i64 {
        monthly_price(self.price, self.frequency)
    }
}

/// [`ServicePlan::monthly_price`] from a price per visit and a frequency.
pub fn monthly_price(per_visit: i64, frequency: Frequency) -> i64 {
    // Cents: rounded to whole dollars, so a month reads `$35.00`.
    let monthly = per_visit * frequency.visits_a_year() / 12;
    (monthly + 50) / 100 * 100
}

/// The tasks of each plan: plan → service tasks.
pub const PLAN_TASKS: Pivot =
    Pivot::new("plan_tasks", "service_plan_id", "service_task_id").with_timestamps();

/// Where a subscription stands, in the shop. The gateway's own state is
/// renox-billing's `subscriptions` row; [`super::sync`] mirrors it here.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubscriptionStatus {
    /// Running: visits are made ahead (unless paused or on hold).
    #[default]
    Active,
    /// Visits stopped by the customer for a while.
    Paused,
    /// Over: no more visits.
    Cancelled,
    /// Chosen online, waiting for the gateway to confirm the payment.
    Pending,
}

/// A customer's bike on a plan, serviced at one store.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "plan_subscriptions")]
pub struct PlanSubscription {
    pub id: i64,
    pub customer_bike_id: i64,
    pub service_plan_id: i64,
    /// The store whose workshop does the visits (the "home store").
    pub store_id: i64,
    /// 1 = Monday … 7 = Sunday.
    pub preferred_weekday: i64,
    pub status: SubscriptionStatus,
    /// The first visit's day; the others follow from it ([`visit_day`]).
    pub starts_on: NaiveDate,
    /// The next visit's day, for lists (worked out by the scheduler).
    pub next_visit_on: Option<NaiveDate>,
    pub cancelled_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
    /// Who pays online (renox-billing's owner `user:{id}`); `None` for a
    /// plan sold at the counter.
    pub user_id: Option<i64>,
    /// `stripe`, `xendit` or `demo`; empty for a plan paid at the counter.
    pub gateway: String,
    /// How many visits were planned so far (made, skipped or passed while
    /// paused): the next one is visit number `visit_seq`.
    pub visit_seq: i64,
    /// A plan change waiting for the next billing period.
    pub next_plan_id: Option<i64>,
    /// The day it takes effect.
    pub swap_on: Option<NaiveDate>,
    /// Paused by the customer: no visits are made.
    pub paused_at: Option<DateTime>,
    /// A payment failed: no visits until it is paid.
    pub held_at: Option<DateTime>,
    /// Cancelled at the period's end: the last day with visits.
    pub ends_on: Option<NaiveDate>,
}

impl PlanSubscription {
    /// renox-billing's subscription name for this bike: an owner (the
    /// user) has one subscription per bike, `bike-{id}`.
    pub fn billing_name(&self) -> String {
        billing_name(self.customer_bike_id)
    }

    /// Whether visits are being made now: running, not paused, not on hold.
    pub fn making_visits(&self) -> bool {
        self.status == SubscriptionStatus::Active
            && self.paused_at.is_none()
            && self.held_at.is_none()
    }

    /// Whether it still counts as the bike's plan (one per bike).
    pub fn live(&self) -> bool {
        matches!(
            self.status,
            SubscriptionStatus::Active | SubscriptionStatus::Paused
        )
    }

    /// Billed online through renox-billing (not at the counter).
    pub fn online(&self) -> bool {
        !self.gateway.is_empty()
    }

    /// The day of visit number `seq`: [`visit_day`].
    pub fn visit_day(&self, frequency: Frequency, seq: i64) -> NaiveDate {
        visit_day(self.starts_on, self.preferred_weekday, frequency, seq)
    }
}

/// `bike-{id}`: see [`PlanSubscription::billing_name`].
pub fn billing_name(customer_bike_id: i64) -> String {
    format!("bike-{customer_bike_id}")
}

/// The bike of a billing subscription's name (`bike-12` → 12).
pub fn bike_of_billing_name(name: &str) -> Option<i64> {
    name.strip_prefix("bike-")?.parse().ok()
}

/// The day of visit number `seq` of a plan started on `starts_on`: `seq`
/// periods later ([`Frequency::after`]), then forward to the preferred
/// weekday (1 = Monday … 7 = Sunday). Counting from the start each time
/// keeps a monthly plan on the same week of the month instead of
/// drifting.
pub fn visit_day(starts_on: NaiveDate, weekday: i64, frequency: Frequency, seq: i64) -> NaiveDate {
    next_weekday(frequency.after(starts_on, seq), weekday)
}

/// `day` itself when it is `weekday` (1 = Monday … 7 = Sunday), else the
/// next such day.
pub fn next_weekday(day: NaiveDate, weekday: i64) -> NaiveDate {
    let have = day.weekday().number_from_monday() as i64;
    let want = weekday.clamp(1, 7);
    day + Duration::days((want - have).rem_euclid(7))
}

impl StoreRecord for PlanSubscription {
    const VIEW: &'static str = catalogue::WORKORDERS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["store_id"];

    fn store_id(&self, _attr: StoreAttr) -> Option<i64> {
        Some(self.store_id)
    }
}

/// What became of a planned visit.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisitStatus {
    /// Booked as a work order.
    #[default]
    Scheduled,
    /// The work order was collected.
    Done,
    /// The customer skipped it.
    Skipped,
    /// The day passed and the bike never came: recorded, not rolled over.
    Missed,
    /// Taken off the workshop's day after a failed payment; booked again
    /// when the payment comes in (if its day hasn't passed).
    Held,
    /// The plan ended or was paused before it.
    Cancelled,
}

/// One visit of a plan: the day the plan says (`due_on`) and the work
/// order that books it (on that day, or the next one with room).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "plan_visits")]
pub struct PlanVisit {
    pub id: i64,
    pub plan_subscription_id: i64,
    /// Which visit of the plan (0, 1, 2…): unique per subscription, so the
    /// scheduler never makes one twice.
    pub seq: i64,
    pub due_on: NaiveDate,
    pub work_order_id: Option<i64>,
    pub status: VisitStatus,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// A payment the gateway reported for a subscription (renox-billing's
/// `PaymentSucceeded` / `PaymentFailed`): the customer's invoices.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "plan_invoices")]
pub struct PlanInvoice {
    pub id: i64,
    pub plan_subscription_id: i64,
    pub gateway: String,
    /// The gateway's id (Stripe's invoice, Xendit's cycle): one row each.
    pub payment_id: String,
    /// In the currency's smallest unit.
    pub amount: i64,
    pub currency: String,
    /// Paid, or failed (a failed one that is paid later turns paid).
    pub paid: bool,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}
