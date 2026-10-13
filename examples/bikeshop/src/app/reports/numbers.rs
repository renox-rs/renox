//! The dashboard's numbers for one period, set of stores and way of
//! counting, computed in a fixed number of queries and cached.
//!
//! Everything is read from what the other areas wrote: income from the
//! `report_revenue` view ([`RevenueLine`]), the fleet and overdue rentals
//! from `rentals` and `rental_bikes`, the workshop from `work_orders`,
//! plans from `plan_subscriptions`, the books from `intercompany_entries`
//! (netted with the books area's own `intercompany::balances`), help from
//! `staff_help_hours`. Nothing here writes.
//!
//! **Caching.** [`Numbers::for_page`] keeps the result in the cache
//! (`Cache::remember`) under a key made of the period, the stores, the
//! way of counting, today's date and a *generation* number. Whenever the
//! data behind the numbers changes, a listener (see `super::Reports`)
//! bumps the generation ([`changed`]): every cached dashboard is then
//! stale and the next visit computes afresh. A late return or a refund can
//! change a past month too, so every period is cleared, not only the
//! current one. A ten-minute lifetime is the safety net for changes made
//! without an event (seeders, imports).
//!
//! Every query below runs once per computation, whatever the data: the
//! dashboard's query count is fixed (`tests/reports.rs` checks it).

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use renox::chart::{Period, Trend};
use renox::chrono::{Datelike, Timelike};
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::model::{RevenueLine, STREAMS};
use super::scope::{By, Reach};
use crate::app::accounts::model::Customer;
use crate::app::catalog::model::{Category, Product};
use crate::app::multistore::intercompany;
use crate::app::multistore::model::{EntryKind, IntercompanyEntry};
use crate::app::plans::model::{PlanSubscription, ServicePlan, SubscriptionStatus};
use crate::app::rentals::model::{BikeStatus, Rental, RentalBike, RentalStatus};
use crate::app::staff::model::{Staff, StaffHelpHour, Store};
use crate::app::workshop::model::{WorkOrder, WorkStatus};

/// The cache key holding the generation number.
pub const GENERATION_KEY: &str = "reports:generation";

/// How long a computed dashboard is kept at most.
pub const LIFETIME: Duration = Duration::from_secs(10 * 60);

/// Marks every cached report stale: the next visit computes afresh. Called
/// by the listeners in `super::Reports` when a rental closes, a work order
/// is collected or a payment succeeds.
// [explain:reports.dashboard.cache]
pub async fn changed(state: &AppState) -> Result {
    state.cache.increment(GENERATION_KEY, 1).await?;
    Ok(())
}
// [/explain:reports.dashboard.cache]

/// One income stream in the period.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct StreamFigure {
    /// `sales`, `rentals`, `workshop` or `plans`.
    pub key: String,
    pub amount: i64,
    /// The same, in the period before (as long).
    pub previous: i64,
    /// Percent change from the period before; `None` when that was 0.
    pub change: Option<f64>,
    /// Orders, rentals or work orders (each counted once).
    pub count: i64,
    /// One value per day, week or month (the period's buckets).
    pub trend: Vec<f64>,
}

/// One row of a "top" list.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct TopLine {
    pub id: i64,
    pub name: String,
    pub quantity: i64,
    pub amount: i64,
}

/// A store in the comparison.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct StoreFigure {
    pub id: i64,
    pub name: String,
    /// Per stream, in [`STREAMS`]' order.
    pub streams: Vec<i64>,
    pub total: i64,
    /// Share of the stores' total, in percent.
    pub share: f64,
}

/// An open balance between two stores.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BalanceLine {
    pub debtor: String,
    pub creditor: String,
    pub amount: i64,
}

/// A cell of the weekday × hour heatmap.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct HourPoint {
    /// Hour of the day, 0–23 (in `APP_TIMEZONE`): the heatmap's column.
    pub x: u32,
    /// Weekday, 0 = Monday … 6 = Sunday: the heatmap's row.
    pub y: u32,
    /// Rentals picked up then (the cell's value).
    pub value: i64,
    /// `Mon 10:00`.
    pub label: String,
}

/// Everything the dashboard shows.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Numbers {
    /// The buckets' labels (days, weeks or months).
    pub labels: Vec<String>,
    pub streams: Vec<StreamFigure>,
    pub revenue: i64,
    pub revenue_previous: i64,
    pub revenue_change: Option<f64>,
    /// The four streams added up, per bucket (the revenue stat's sparkline).
    pub revenue_trend: Vec<f64>,
    pub orders: i64,
    pub orders_change: Option<f64>,
    /// Sales ÷ orders.
    pub average_order: i64,
    /// Rentals returned in the period.
    pub rentals: i64,
    /// Bike-hours rented ÷ bike-hours the fleet had, in percent.
    pub fleet_use: f64,
    /// Bikes in the fleet (not retired).
    pub fleet: i64,
    /// Rentals overdue now.
    pub overdue: i64,
    /// Work orders completed in the period (customers' and the fleet's).
    pub work_done: i64,
    /// Hours from the booked time to completion, on average.
    pub turnaround_hours: Option<f64>,
    /// Plan subscriptions active now.
    pub active_plans: i64,
    /// What the active plans bring in a month (a weekly plan's price × 30 ÷ 7…).
    pub recurring_monthly: i64,
    /// Subscriptions cancelled in the period ÷ those active during it, in percent.
    pub churn: Option<f64>,
    /// Fees earned from other stores in the period (operating and selling fees).
    pub fees_earned: i64,
    /// Fees paid to other stores in the period.
    pub fees_paid: i64,
    /// Owed between stores and not settled yet, pair by pair.
    pub balances: Vec<BalanceLine>,
    /// The stores' position on those balances: owed to them minus owed by them.
    pub position: i64,
    pub help_given_hours: f64,
    pub help_received_hours: f64,
    pub rental_hours: Vec<HourPoint>,
    pub top_products: Vec<TopLine>,
    pub top_categories: Vec<TopLine>,
    pub top_customers: Vec<TopLine>,
    /// One per store when the page shows several.
    pub comparison: Vec<StoreFigure>,
}

/// Percent change from `before` to `now`; `None` when `before` is 0.
pub fn change(now: i64, before: i64) -> Option<f64> {
    (before != 0).then(|| (now - before) as f64 * 100.0 / before as f64)
}

impl Numbers {
    /// The numbers for `reach`'s chosen stores, counted `by`, over `period`:
    /// from the cache when nothing changed since they were computed.
    // [explain:reports.dashboard.cache]
    pub async fn for_page(
        state: &AppState,
        reach: &Reach,
        by: By,
        period: Period,
    ) -> Result<Numbers> {
        let generation: i64 = state.cache.get(GENERATION_KEY).await?.unwrap_or(0);
        let today = renox::db::now().date_naive();
        let key = format!(
            "reports:dashboard:{generation}:{}:{}:{}:{today}",
            by.key(),
            reach.key(),
            period.key()
        );
        let (state2, reach2) = (state.clone(), reach.clone());
        state
            .cache
            .remember(&key, LIFETIME, move || {
                Numbers::compute(state2, reach2, by, period)
            })
            .await
    }
    // [/explain:reports.dashboard.cache]

    /// Computes the numbers (no cache): a fixed number of queries.
    pub async fn compute(state: AppState, reach: Reach, by: By, period: Period) -> Result<Numbers> {
        let db = &state.db;
        let zone = state.config.timezone;
        let stores = reach.chosen.clone();
        let column = by.store_column();
        let (start, end) = period.range(zone);
        let (before_start, before_end) = period.previous().range(zone);
        let now = renox::db::now();
        let lines = || RevenueLine::query().where_in(column, stores.clone());
        let mut n = Numbers {
            labels: period.labels(zone),
            ..Default::default()
        };

        // Income per stream, now and before: two grouped queries.
        let totals = |from: DateTime, until: DateTime| {
            lines()
                .where_(RevenueLine::BOOKED_AT.gte(from))
                .where_(RevenueLine::BOOKED_AT.lt(until))
                .group_by("stream")
        };
        let count = "stream, CAST(SUM(amount) AS BIGINT), \
                     COUNT(DISTINCT source_type || ':' || CAST(source_id AS TEXT))";
        let current: Vec<(String, i64, i64)> = totals(start, end).select_as(db, count).await?;
        let previous: Vec<(String, i64, i64)> = totals(before_start, before_end)
            .select_as(db, count)
            .await?;
        let find = |rows: &[(String, i64, i64)], key: &str| {
            rows.iter()
                .find(|r| r.0 == key)
                .map(|r| (r.1, r.2))
                .unwrap_or((0, 0))
        };
        // Per stream and bucket: `Trend` (one query per stream).
        n.revenue_trend = vec![0.0; n.labels.len()];
        for key in STREAMS {
            let series = Trend::of(lines().where_(RevenueLine::STREAM.eq(key)), "booked_at")
                .over(period)
                .sum(&state, "amount")
                .await?;
            for (total, value) in n.revenue_trend.iter_mut().zip(&series.values) {
                *total += value;
            }
            let (amount, count) = find(&current, key);
            let (before, _) = find(&previous, key);
            n.streams.push(StreamFigure {
                key: key.to_owned(),
                amount,
                previous: before,
                change: change(amount, before),
                count,
                trend: series.values,
            });
        }
        n.revenue = n.streams.iter().map(|s| s.amount).sum();
        n.revenue_previous = n.streams.iter().map(|s| s.previous).sum();
        n.revenue_change = change(n.revenue, n.revenue_previous);
        let (sales, orders) = find(&current, "sales");
        n.orders = orders;
        n.orders_change = change(orders, find(&previous, "sales").1);
        n.average_order = if orders > 0 { sales / orders } else { 0 };
        n.rentals = find(&current, "rentals").1;

        // The fleet: bike-hours rented over bike-hours available, and
        // rentals overdue now. Counted where the bikes are (work) or by
        // whose bikes they are (books).
        let rental_column = column;
        let fleet_until = end.min(now);
        let spans: Vec<(Option<DateTime>, Option<DateTime>)> = Rental::query()
            .where_in(rental_column, stores.clone())
            .where_(Rental::STATUS.is_in([
                RentalStatus::Active,
                RentalStatus::Overdue,
                RentalStatus::Returned,
            ]))
            .where_(Rental::PICKED_UP_AT.is_not_null())
            .where_(Rental::PICKED_UP_AT.lt(end))
            .where_any(|q| {
                q.where_(Rental::RETURNED_AT.is_null())
                    .where_(Rental::RETURNED_AT.gt(start))
            })
            .select_as(db, "picked_up_at, returned_at")
            .await?;
        let mut rented = 0.0;
        let mut starts: BTreeMap<(u32, u32), i64> = BTreeMap::new();
        for (picked, returned) in &spans {
            let Some(picked) = picked else { continue };
            let from = (*picked).max(start);
            let until = returned.unwrap_or(now).min(fleet_until);
            if until > from {
                rented += (until - from).num_minutes() as f64 / 60.0;
            }
            if *picked >= start {
                let local = zone.local(picked.timestamp());
                *starts
                    .entry((local.weekday().number_from_monday(), local.hour()))
                    .or_default() += 1;
            }
        }
        n.fleet = RentalBike::query()
            .where_in(by.fleet_column(), stores.clone())
            .where_(RentalBike::STATUS.ne(BikeStatus::Retired))
            .count(db)
            .await? as i64;
        let hours = (fleet_until - start).num_minutes().max(0) as f64 / 60.0;
        n.fleet_use = if n.fleet > 0 && hours > 0.0 {
            (rented / (n.fleet as f64 * hours) * 100.0).min(100.0)
        } else {
            0.0
        };
        const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        n.rental_hours = starts
            .into_iter()
            .map(|((day, hour), count)| HourPoint {
                x: hour,
                y: day - 1,
                value: count,
                label: format!("{} {hour:02}:00", DAYS[(day - 1) as usize]),
            })
            .collect();
        n.overdue = Rental::query()
            .where_in(rental_column, stores.clone())
            .where_any(|q| {
                q.where_(Rental::STATUS.eq(RentalStatus::Overdue))
                    .where_all(|q| {
                        q.where_(Rental::STATUS.eq(RentalStatus::Active))
                            .where_(Rental::DUE_AT.lt(now))
                    })
            })
            .count(db)
            .await? as i64;

        // The workshop: work completed in the period and how long it took.
        let done: Vec<(DateTime, Option<DateTime>)> = WorkOrder::query()
            .where_(WorkOrder::STORE_ID.is_in(stores.clone()))
            .where_(WorkOrder::STATUS.eq(WorkStatus::Completed))
            .where_(WorkOrder::COMPLETED_AT.gte(start))
            .where_(WorkOrder::COMPLETED_AT.lt(end))
            .select_as(db, "scheduled_for, completed_at")
            .await?;
        n.work_done = done.len() as i64;
        let took: Vec<f64> = done
            .iter()
            .filter_map(|(from, until)| {
                until.map(|u| (u - *from).num_minutes().max(0) as f64 / 60.0)
            })
            .collect();
        n.turnaround_hours =
            (!took.is_empty()).then(|| took.iter().sum::<f64>() / took.len() as f64);

        // Plans: active now, what they bring a month, churn in the period.
        let active: Vec<(i64, i64)> = PlanSubscription::query()
            .where_(PlanSubscription::STORE_ID.is_in(stores.clone()))
            .where_(PlanSubscription::STATUS.eq(SubscriptionStatus::Active))
            .group_by("service_plan_id")
            .select_as(db, "service_plan_id, COUNT(*)")
            .await?;
        let plans: HashMap<i64, ServicePlan> = ServicePlan::query()
            .get(db)
            .await?
            .into_iter()
            .map(|p| (p.id, p))
            .collect();
        n.active_plans = active.iter().map(|(_, c)| c).sum();
        n.recurring_monthly = active
            .iter()
            .filter_map(|(plan, count)| {
                plans
                    .get(plan)
                    .map(|p| p.price * 30 * count / p.frequency.days())
            })
            .sum();
        let cancelled = PlanSubscription::query()
            .where_(PlanSubscription::STORE_ID.is_in(stores.clone()))
            .where_(PlanSubscription::CANCELLED_AT.gte(start))
            .where_(PlanSubscription::CANCELLED_AT.lt(end))
            .count(db)
            .await? as i64;
        let during = n.active_plans + cancelled;
        n.churn = (during > 0).then(|| cancelled as f64 * 100.0 / during as f64);

        // The books between stores: fees in the period, open balances.
        let names: HashMap<i64, String> = Store::all_by_name(db)
            .await?
            .into_iter()
            .map(|s| (s.id, s.name))
            .collect();
        let fees: Vec<(i64, i64, i64)> = IntercompanyEntry::query()
            .where_(IntercompanyEntry::KIND.is_in([EntryKind::OperatingFee, EntryKind::SellingFee]))
            .where_(IntercompanyEntry::BOOKED_AT.gte(start))
            .where_(IntercompanyEntry::BOOKED_AT.lt(end))
            .where_any(|q| {
                q.where_(IntercompanyEntry::DEBTOR_STORE_ID.is_in(stores.clone()))
                    .where_(IntercompanyEntry::CREDITOR_STORE_ID.is_in(stores.clone()))
            })
            .group_by("debtor_store_id")
            .group_by("creditor_store_id")
            .select_as(
                db,
                "debtor_store_id, creditor_store_id, CAST(SUM(amount) AS BIGINT)",
            )
            .await?;
        for (debtor, creditor, amount) in &fees {
            if stores.contains(creditor) {
                n.fees_earned += amount;
            }
            if stores.contains(debtor) {
                n.fees_paid += amount;
            }
        }
        let open: Vec<(i64, i64, i64)> = IntercompanyEntry::query()
            .where_any(|q| {
                q.where_(IntercompanyEntry::SETTLEMENT_ID.is_null())
                    .where_raw(
                        "settlement_id IN (SELECT id FROM settlements WHERE status = ?)",
                        ["open"],
                    )
            })
            .where_any(|q| {
                q.where_(IntercompanyEntry::DEBTOR_STORE_ID.is_in(stores.clone()))
                    .where_(IntercompanyEntry::CREDITOR_STORE_ID.is_in(stores.clone()))
            })
            .group_by("debtor_store_id")
            .group_by("creditor_store_id")
            .select_as(
                db,
                "debtor_store_id, creditor_store_id, CAST(SUM(amount) AS BIGINT)",
            )
            .await?;
        n.balances = intercompany::balances(&open, &names)
            .into_iter()
            .map(|b| BalanceLine {
                debtor: b.debtor,
                creditor: b.creditor,
                amount: b.amount,
            })
            .collect();
        let positions = intercompany::positions(&open);
        n.position = stores
            .iter()
            .map(|s| positions.get(s).copied().unwrap_or(0))
            .sum();

        // Staff help: hours worked at the stores in view by people from
        // elsewhere (received), and by their people elsewhere (given).
        let first_day = zone.local(start.timestamp()).date();
        let last_day = zone.local(end.timestamp()).date();
        let in_period = |q: renox::db::Query<StaffHelpHour>| {
            q.where_(StaffHelpHour::WORKED_ON.gte(first_day))
                .where_(StaffHelpHour::WORKED_ON.lt(last_day))
        };
        let received: i64 = in_period(StaffHelpHour::query())
            .where_(StaffHelpHour::STORE_ID.is_in(stores.clone()))
            .sum(db, "minutes")
            .await?;
        let people: Vec<i64> = Staff::query()
            .where_(Staff::HOME_STORE_ID.is_in(stores.clone()))
            .pluck(db, Staff::ID)
            .await?;
        let given: i64 = in_period(StaffHelpHour::query())
            .where_(StaffHelpHour::STAFF_ID.is_in(people))
            .sum(db, "minutes")
            .await?;
        n.help_received_hours = received as f64 / 60.0;
        n.help_given_hours = given as f64 / 60.0;

        // The best sellers, categories and customers.
        let in_period = || {
            lines()
                .where_(RevenueLine::BOOKED_AT.gte(start))
                .where_(RevenueLine::BOOKED_AT.lt(end))
        };
        let ranked = "CAST(SUM(quantity) AS BIGINT), CAST(SUM(amount) AS BIGINT)";
        let products: Vec<(i64, i64, i64)> = in_period()
            .where_(RevenueLine::STREAM.eq("sales"))
            .group_by("product_id")
            .order_by_raw("3 DESC")
            .limit(8)
            .select_as(db, &format!("product_id, {ranked}"))
            .await?;
        let categories: Vec<(i64, i64, i64)> = in_period()
            .where_(RevenueLine::CATEGORY_ID.is_not_null())
            .group_by("category_id")
            .order_by_raw("3 DESC")
            .limit(8)
            .select_as(db, &format!("category_id, {ranked}"))
            .await?;
        let customers: Vec<(i64, i64, i64)> = in_period()
            .where_(RevenueLine::CUSTOMER_ID.is_not_null())
            .group_by("customer_id")
            .order_by_raw("3 DESC")
            .limit(8)
            .select_as(
                db,
                "customer_id, COUNT(DISTINCT source_type || ':' || CAST(source_id AS TEXT)), \
                 CAST(SUM(amount) AS BIGINT)",
            )
            .await?;
        let product_names: HashMap<i64, String> =
            Product::find_many(db, products.iter().map(|p| p.0).collect::<Vec<_>>())
                .await?
                .into_iter()
                .map(|p| (p.id, p.name))
                .collect();
        let category_names: HashMap<i64, String> =
            Category::find_many(db, categories.iter().map(|c| c.0).collect::<Vec<_>>())
                .await?
                .into_iter()
                .map(|c| (c.id, c.name))
                .collect();
        let customer_names: HashMap<i64, String> =
            Customer::find_many(db, customers.iter().map(|c| c.0).collect::<Vec<_>>())
                .await?
                .into_iter()
                .map(|c| (c.id, c.name))
                .collect();
        let top = |rows: &[(i64, i64, i64)], names: &HashMap<i64, String>| -> Vec<TopLine> {
            rows.iter()
                .map(|(id, quantity, amount)| TopLine {
                    id: *id,
                    name: names.get(id).cloned().unwrap_or_else(|| format!("#{id}")),
                    quantity: *quantity,
                    amount: *amount,
                })
                .collect()
        };
        n.top_products = top(&products, &product_names);
        n.top_categories = top(&categories, &category_names);
        n.top_customers = top(&customers, &customer_names);

        // The stores side by side, when the page shows several.
        if reach.compares() {
            let per_store: Vec<(i64, String, i64)> = in_period()
                .group_by(column)
                .group_by("stream")
                .select_as(
                    db,
                    &format!("{column}, stream, CAST(SUM(amount) AS BIGINT)"),
                )
                .await?;
            let all: i64 = per_store.iter().map(|r| r.2).sum();
            n.comparison = stores
                .iter()
                .map(|id| {
                    let streams: Vec<i64> = STREAMS
                        .iter()
                        .map(|key| {
                            per_store
                                .iter()
                                .find(|r| r.0 == *id && r.1 == *key)
                                .map_or(0, |r| r.2)
                        })
                        .collect();
                    let total: i64 = streams.iter().sum();
                    StoreFigure {
                        id: *id,
                        name: reach.name_of(*id),
                        streams,
                        total,
                        share: if all > 0 {
                            total as f64 * 100.0 / all as f64
                        } else {
                            0.0
                        },
                    }
                })
                .collect();
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::change;

    #[test]
    fn a_change_needs_a_period_before() {
        assert_eq!(change(150, 100), Some(50.0));
        assert_eq!(change(50, 100), Some(-50.0));
        assert_eq!(change(10, 0), None);
    }
}
