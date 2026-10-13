//! Dashboards, reports and exports (#242).
//!
//! | Who | Pages | File |
//! |---|---|---|
//! | The owner, managers (`reports.view`) | `/staff/reports`: the dashboard (income by stream, the fleet, the workshop, plans, the books between stores, help, trends, the best sellers, the stores side by side) | [`dashboard`], [`numbers`] |
//! | The same | `/staff/reports/{orders,rentals,work-orders,payments,customers,intercompany}`: grids to filter, group, sum and export | [`grids`] |
//! | The same | `/staff/reports/monthly`: the monthly report, run for any month, with progress | [`monthly`] |
//! | Everyone with `reports.view` in the active store | `/staff` (the staff home page) shows the store's last 7 days | [`dashboard::overview`] |
//!
//! Where the numbers come from: the `report_*` views ([`model`]), plain
//! queries over what the sales, rentals, workshop, plans and multi-store
//! areas wrote. Who sees which stores: [`scope`] (`scopes_with` over
//! `reports.view`, #244), and whose books or whose work ([`scope::By`],
//! #245).
//!
//! The area listens to the events that change income (a rental closed, a
//! work order collected, a payment received) and marks the cached
//! dashboards stale ([`numbers::changed`]).
//!
//! Made with `rnx make:module reports`, then the files by hand.

pub mod dashboard;
pub mod explain;
pub mod grids;
pub mod model;
pub mod monthly;
pub mod numbers;
pub mod scope;

use renox::prelude::*;

use crate::app::access::{self, catalogue};
use crate::app::rentals::RentalClosed;
use crate::app::sales::payments::PaymentSucceeded;
use crate::app::workshop::status::WorkOrderClosed;

/// The reports area, registered in `src/lib.rs`.
pub struct Reports;

impl Module for Reports {
    fn name(&self) -> &'static str {
        "reports"
    }

    fn routes(&self) -> Routes {
        access::staff_routes(
            Routes::new()
                .get("/staff/reports", dashboard::show)
                .name("reports.dashboard")
                .get("/staff/reports/orders", grids::orders)
                .name("reports.orders")
                .get("/staff/reports/rentals", grids::rentals)
                .name("reports.rentals")
                .get("/staff/reports/work-orders", grids::work_orders)
                .name("reports.work_orders")
                .get("/staff/reports/payments", grids::payments)
                .name("reports.payments")
                .get("/staff/reports/customers", grids::customers)
                .name("reports.customers")
                .get("/staff/reports/intercompany", grids::entries)
                .name("reports.entries")
                .get("/staff/reports/monthly", monthly::index)
                .name("reports.monthly")
                .post("/staff/reports/monthly", monthly::run)
                .name("reports.monthly.run")
                .get("/staff/reports/monthly/runs", monthly::runs_fragment)
                .name("reports.monthly.runs")
                .get("/staff/reports/monthly/{month}/{store}", monthly::file)
                .name("reports.monthly.file")
                .require_permission(catalogue::REPORTS_VIEW),
        )
    }

    fn register(&self, app: &mut Registry) {
        // Income changed: every cached dashboard is stale.
        app.listen(
            |_: RentalClosed, state: AppState| async move { numbers::changed(&state).await },
        );
        app.listen(
            |_: WorkOrderClosed, state: AppState| async move { numbers::changed(&state).await },
        );
        app.listen(
            |_: PaymentSucceeded, state: AppState| async move { numbers::changed(&state).await },
        );
        app.job::<monthly::BuildStoreReport>();
        app.job::<monthly::SendMonthlyReport>();
        monthly::schedule(app.schedule());
    }
}
