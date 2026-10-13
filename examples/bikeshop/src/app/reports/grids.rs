//! The report grids: orders, rentals, work orders, payments, customers
//! (with their lifetime value) and the books between stores, each on
//! `renox::grid` over a view of `super::model`.
//!
//! | Route | Name | Grid |
//! |---|---|---|
//! | `GET /staff/reports/orders` | `reports.orders` | [`orders_grid`] over `report_orders` |
//! | `GET /staff/reports/rentals` | `reports.rentals` | [`rentals_grid`] over `report_rentals` |
//! | `GET /staff/reports/work-orders` | `reports.work_orders` | [`work_orders_grid`] over `report_work_orders` |
//! | `GET /staff/reports/payments` | `reports.payments` | [`payments_grid`] over `report_payments` |
//! | `GET /staff/reports/customers` | `reports.customers` | [`customers_grid`] over `report_customers` |
//! | `GET /staff/reports/intercompany` | `reports.entries` | [`entries_grid`] over `report_entries` |
//!
//! Every grid has the same toolbox: a search box (`searchable`), a filter
//! in each heading (dates in `APP_TIMEZONE`), the advanced filter, groups
//! (by store, category, status…), sums under the money columns (for every
//! row the filters match, not only the page), a frozen first column,
//! choices remembered per person (`remember`, column preferences), cards on
//! phones, and exports (CSV, Excel, print). Each page answers its export
//! links itself (`grid.export` first).
//!
//! **Who sees what.** Rows come through `access::visible::<M>(reports.view)`:
//! the rows of the stores where the person holds `reports.view` (in any
//! of the row's stores: a rental counts at its owner and at its operating
//! store). Customers belong to the company: a manager sees the customers
//! who did business with their stores, with the customer's whole lifetime
//! value.

use renox::grid::{Column, Grid, GridRequest, Summary};
use renox::prelude::*;
use serde::Serialize;

use super::model::{CustomerValue, EntryRow, OrderRow, PaymentRow, RentalRow, WorkOrderRow};
use crate::app::access::{self, catalogue};
use crate::app::staff::model::Store;

/// The report pages, in the tabs' order: (key, route name).
pub const REPORTS: [(&str, &str); 7] = [
    ("dashboard", "reports.dashboard"),
    ("orders", "reports.orders"),
    ("rentals", "reports.rentals"),
    ("work_orders", "reports.work_orders"),
    ("payments", "reports.payments"),
    ("customers", "reports.customers"),
    ("entries", "reports.entries"),
];

/// A tab above the report pages.
#[derive(Serialize, Debug, Clone)]
pub struct Tab {
    pub key: &'static str,
    pub route: &'static str,
}

/// The tabs, for the templates.
pub fn tabs() -> Vec<Tab> {
    REPORTS
        .iter()
        .map(|(key, route)| Tab { key, route })
        .collect()
}

fn field(lang: &Lang, key: &str) -> String {
    lang.t(&format!("reports.fields.{key}"), &[])
}

/// `(value, label)` pairs from translation keys `{prefix}.{value}`.
fn options(lang: &Lang, prefix: &str, values: &[&str]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|v| ((*v).to_owned(), lang.t(&format!("{prefix}.{v}"), &[])))
        .collect()
}

/// The stores by name, as a select column's options (a store's name is
/// what the views hold).
pub fn store_options(stores: &[Store]) -> Vec<(String, String)> {
    stores
        .iter()
        .map(|s| (s.name.clone(), s.name.clone()))
        .collect()
}

/// What every report grid shares.
// [explain:reports.orders.grid]
fn report_grid(id: &str, lang: &Lang, empty: &str) -> Grid {
    Grid::new(id)
        .per_page(25)
        .advanced_filter()
        .remember()
        .cards_on_mobile()
        .exports()
        .empty_state(&lang.t(empty, &[]), None)
}
// [/explain:reports.orders.grid]

/// The orders grid.
// [explain:reports.orders.grid]
pub fn orders_grid(lang: &Lang, stores: &[Store]) -> Grid {
    let f = |key: &str| field(lang, key);
    report_grid("report-orders", lang, "reports.empty.orders")
        .column(
            Column::text("number", &f("number"))
                .frozen()
                .mobile()
                .searchable()
                .copyable(),
        )
        .column(Column::datetime("placed_at", &f("placed_at")).mobile())
        // [/explain:reports.orders.grid]
        .column(Column::select("store", &f("store"), store_options(stores)))
        .column(
            Column::text("customer", &f("customer"))
                .searchable()
                .limit(32),
        )
        .column(Column::select(
            "channel",
            &f("channel"),
            options(lang, "sales.channel", &["online", "counter"]),
        ))
        .column(
            Column::select(
                "status",
                &f("status"),
                options(
                    lang,
                    "sales.status",
                    &[
                        "pending",
                        "paid",
                        "ready",
                        "completed",
                        "cancelled",
                        "refunded",
                    ],
                ),
            )
            .badges(&[
                ("paid", "info"),
                ("ready", "info"),
                ("completed", "success"),
                ("pending", "warning"),
                ("cancelled", "neutral"),
                ("refunded", "danger"),
            ])
            .mobile(),
        )
        .column(Column::number("units", &f("units")).summary(Summary::Sum))
        .column(
            Column::money("discount", &f("discount"))
                .summary(Summary::Sum)
                .hidden(),
        )
        // [explain:reports.orders.grid]
        .column(
            Column::money("total", &f("total"))
                .summary(Summary::Sum)
                .summary(Summary::Average)
                .mobile(),
        )
        .column(Column::datetime("paid_at", &f("paid_at")).hidden())
        .groups(&["store", "status", "channel"])
        .sort_by("-placed_at")
        .row_url("/staff/orders/{id}")
}
// [/explain:reports.orders.grid]

/// The rentals grid.
// [explain:reports.rentals.grid]
pub fn rentals_grid(lang: &Lang, stores: &[Store]) -> Grid {
    let f = |key: &str| field(lang, key);
    report_grid("report-rentals", lang, "reports.empty.rentals")
        .column(
            Column::text("code", &f("code"))
                .frozen()
                .mobile()
                .searchable()
                .limit(12),
        )
        .column(Column::datetime("starts_at", &f("starts_at")).mobile())
        .column(Column::select("store", &f("store"), store_options(stores)))
        // [/explain:reports.rentals.grid]
        .column(Column::select(
            "owner_store",
            &f("owner_store"),
            store_options(stores),
        ))
        .column(Column::text("customer", &f("customer")).searchable())
        .column(Column::text("bike", &f("bike")).searchable().hidden())
        .column(Column::text("model", &f("model")).searchable())
        .column(Column::text("category", &f("category")))
        .column(Column::select(
            "rate",
            &f("rate"),
            options(lang, "reports.rate", &["hourly", "daily"]),
        ))
        .column(
            Column::select(
                "status",
                &f("status"),
                options(
                    lang,
                    "rentals.status.rental",
                    &[
                        "reserved",
                        "active",
                        "overdue",
                        "returned",
                        "cancelled",
                        "no_show",
                    ],
                ),
            )
            .badges(&[
                ("active", "info"),
                ("overdue", "danger"),
                ("returned", "success"),
                ("reserved", "neutral"),
            ])
            .mobile(),
        )
        // [explain:reports.rentals.grid]
        .column(Column::money("price", &f("price")).summary(Summary::Sum))
        .column(Column::money("late_fee", &f("late_fee")).summary(Summary::Sum))
        .column(
            Column::money("damage_fee", &f("damage_fee"))
                .summary(Summary::Sum)
                .hidden(),
        )
        .column(
            Column::money("total", &f("total"))
                .summary(Summary::Sum)
                .summary(Summary::Average)
                .mobile(),
        )
        .column(Column::datetime("returned_at", &f("returned_at")).hidden())
        .groups(&["store", "owner_store", "category", "status"])
        .sort_by("-starts_at")
        .row_url("/staff/rentals/{id}")
}
// [/explain:reports.rentals.grid]

/// The work orders grid.
// [explain:reports.work_orders.grid]
pub fn work_orders_grid(lang: &Lang, stores: &[Store]) -> Grid {
    let f = |key: &str| field(lang, key);
    report_grid("report-work-orders", lang, "reports.empty.work_orders")
        .column(Column::number("id", &f("work_order")).frozen().mobile())
        .column(Column::datetime("scheduled_for", &f("scheduled_for")).mobile())
        .column(Column::select("store", &f("store"), store_options(stores)))
        // [/explain:reports.work_orders.grid]
        .column(Column::select(
            "source",
            &f("source"),
            options(
                lang,
                "workshop.source",
                &["walk_in", "booking", "plan", "fleet"],
            ),
        ))
        .column(
            Column::select(
                "status",
                &f("status"),
                options(
                    lang,
                    "workshop.status",
                    &[
                        "booked",
                        "checked_in",
                        "in_progress",
                        "waiting_parts",
                        "waiting_approval",
                        "ready",
                        "completed",
                        "cancelled",
                    ],
                ),
            )
            .badges(&[
                ("completed", "success"),
                ("ready", "info"),
                ("in_progress", "info"),
                ("waiting_parts", "warning"),
                ("waiting_approval", "warning"),
                ("cancelled", "neutral"),
            ])
            .mobile(),
        )
        // [explain:reports.work_orders.grid]
        .column(Column::text("customer", &f("customer")).searchable())
        .column(Column::text("bike", &f("bike")).searchable().limit(32))
        .column(Column::text("mechanic_name", &f("mechanic_name")).searchable())
        .column(Column::money("labour", &f("labour")).summary(Summary::Sum))
        .column(Column::money("parts", &f("parts")).summary(Summary::Sum))
        .column(
            Column::money("total", &f("total"))
                .summary(Summary::Sum)
                .summary(Summary::Average)
                .mobile(),
        )
        .column(Column::datetime("completed_at", &f("completed_at")).hidden())
        .groups(&["store", "status", "source"])
        .sort_by("-scheduled_for")
        .row_url("/staff/workshop/{id}")
}
// [/explain:reports.work_orders.grid]

/// The payments grid.
// [explain:reports.payments.grid]
pub fn payments_grid(lang: &Lang, stores: &[Store]) -> Grid {
    let f = |key: &str| field(lang, key);
    report_grid("report-payments", lang, "reports.empty.payments")
        .column(Column::number("id", &f("payment")).frozen())
        .column(Column::datetime("paid_at", &f("paid_at")).mobile())
        .column(Column::select("store", &f("store"), store_options(stores)))
        .column(Column::text("customer", &f("customer")).searchable())
        // [/explain:reports.payments.grid]
        .column(
            Column::select(
                "kind",
                &f("kind"),
                options(
                    lang,
                    "reports.payable",
                    &["orders", "rentals", "work_orders", "plan_subscriptions"],
                ),
            )
            .mobile(),
        )
        .column(Column::number("payable_id", &f("payable_id")).hidden())
        .column(Column::select(
            "method",
            &f("method"),
            options(lang, "sales.method", &["cash", "card", "gateway"]),
        ))
        .column(
            Column::select(
                "status",
                &f("status"),
                options(
                    lang,
                    "reports.payment_status",
                    &["pending", "paid", "failed", "refunded"],
                ),
            )
            .badges(&[
                ("paid", "success"),
                ("pending", "warning"),
                ("failed", "danger"),
                ("refunded", "neutral"),
            ]),
        )
        // [explain:reports.payments.grid]
        .column(
            Column::money("amount", &f("amount"))
                .summary(Summary::Sum)
                .mobile(),
        )
        .column(
            Column::text("reference", &f("reference"))
                .searchable()
                .hidden(),
        )
        .groups(&["store", "kind", "method", "status"])
        .sort_by("-paid_at")
}
// [/explain:reports.payments.grid]

/// The customers grid, with their lifetime value per stream.
// [explain:reports.customers.grid]
pub fn customers_grid(lang: &Lang) -> Grid {
    let f = |key: &str| field(lang, key);
    report_grid("report-customers", lang, "reports.empty.customers")
        .column(
            Column::text("name", &f("customer"))
                .frozen()
                .mobile()
                .searchable()
                .description("email"),
        )
        .column(Column::text("email", &f("email")).searchable().hidden())
        .column(Column::text("city", &f("city")))
        .column(Column::number("visits", &f("visits")).summary(Summary::Sum))
        .column(
            Column::money("sales_value", &f("sales_value"))
                .summary(Summary::Sum)
                .under([f("value")]),
        )
        // [/explain:reports.customers.grid]
        .column(
            Column::money("rentals_value", &f("rentals_value"))
                .summary(Summary::Sum)
                .under([f("value")]),
        )
        .column(
            Column::money("workshop_value", &f("workshop_value"))
                .summary(Summary::Sum)
                .under([f("value")]),
        )
        .column(
            Column::money("plans_value", &f("plans_value"))
                .summary(Summary::Sum)
                .under([f("value")]),
        )
        // [explain:reports.customers.grid]
        .column(
            Column::money("lifetime_value", &f("lifetime_value"))
                .summary(Summary::Sum)
                .summary(Summary::Average)
                .under([f("value")])
                .mobile(),
        )
        .column(Column::datetime("first_at", &f("first_at")).hidden())
        .column(Column::datetime("last_at", &f("last_at")).mobile())
        .groups(&["city"])
        .sort_by("-lifetime_value")
}
// [/explain:reports.customers.grid]

/// The books between stores.
// [explain:reports.entries.grid]
pub fn entries_grid(lang: &Lang, stores: &[Store]) -> Grid {
    let f = |key: &str| field(lang, key);
    // [/explain:reports.entries.grid]
    let kinds = [
        "rental_revenue",
        "operating_fee",
        "sale_revenue",
        "selling_fee",
        "late_fee",
        "damage_fee",
        "repair",
        "consignment_loss",
    ];
    // [explain:reports.entries.grid]
    report_grid("report-entries", lang, "reports.empty.entries")
        .column(
            Column::datetime("booked_at", &f("booked_at"))
                .frozen()
                .mobile(),
        )
        .column(
            Column::select(
                "kind",
                &f("kind"),
                options(lang, "multistore.books.kind", &kinds),
            )
            .mobile(),
        )
        .column(Column::select(
            "debtor",
            &f("debtor"),
            store_options(stores),
        ))
        .column(Column::select(
            "creditor",
            &f("creditor"),
            store_options(stores),
        ))
        .column(
            Column::money("amount", &f("amount"))
                .summary(Summary::Sum)
                .mobile(),
        )
        // [/explain:reports.entries.grid]
        .column(Column::number("fee_rate_bp", &f("fee_rate_bp")).hidden())
        .column(Column::custom("source", &f("source")))
        .column(
            Column::select(
                "settlement",
                &f("settlement"),
                options(
                    lang,
                    "reports.settlement",
                    &["unsettled", "open", "settled"],
                ),
            )
            .badges(&[
                ("settled", "success"),
                ("open", "warning"),
                ("unsettled", "neutral"),
            ]),
        )
        .groups(&["kind", "debtor", "creditor", "settlement"])
        .sort_by("-booked_at")
}

/// What the grid template needs besides the grid.
fn page(tab: &str, rows: impl Serialize) -> View {
    view("reports/grid.html", context! { rows, tab, tabs => tabs() })
}

// [explain:reports.orders.handler]
/// `GET /staff/reports/orders` (`reports.orders`).
pub async fn orders(State(db): State<Db>, lang: Lang, request: GridRequest) -> Result<Response> {
    let stores = Store::all_by_name(&db).await?;
    let grid = orders_grid(&lang, &stores);
    let rows = || access::visible::<OrderRow>(catalogue::REPORTS_VIEW);
    if let Some(file) = grid.export(rows(), &request).await? {
        return Ok(file);
    }
    let rows = grid.page(rows(), &request).await?;
    Ok(page("orders", rows).into_response())
}
// [/explain:reports.orders.handler]

// [explain:reports.rentals.handler]
/// `GET /staff/reports/rentals` (`reports.rentals`).
pub async fn rentals(State(db): State<Db>, lang: Lang, request: GridRequest) -> Result<Response> {
    let stores = Store::all_by_name(&db).await?;
    let grid = rentals_grid(&lang, &stores);
    let rows = || access::visible::<RentalRow>(catalogue::REPORTS_VIEW);
    if let Some(file) = grid.export(rows(), &request).await? {
        return Ok(file);
    }
    let rows = grid.page(rows(), &request).await?;
    Ok(page("rentals", rows).into_response())
}
// [/explain:reports.rentals.handler]

// [explain:reports.work_orders.handler]
/// `GET /staff/reports/work-orders` (`reports.work_orders`).
pub async fn work_orders(
    State(db): State<Db>,
    lang: Lang,
    request: GridRequest,
) -> Result<Response> {
    let stores = Store::all_by_name(&db).await?;
    let grid = work_orders_grid(&lang, &stores);
    let rows = || access::visible::<WorkOrderRow>(catalogue::REPORTS_VIEW);
    if let Some(file) = grid.export(rows(), &request).await? {
        return Ok(file);
    }
    let rows = grid.page(rows(), &request).await?;
    Ok(page("work_orders", rows).into_response())
}
// [/explain:reports.work_orders.handler]

// [explain:reports.payments.handler]
/// `GET /staff/reports/payments` (`reports.payments`).
pub async fn payments(State(db): State<Db>, lang: Lang, request: GridRequest) -> Result<Response> {
    let stores = Store::all_by_name(&db).await?;
    let grid = payments_grid(&lang, &stores);
    let rows = || access::visible::<PaymentRow>(catalogue::REPORTS_VIEW);
    if let Some(file) = grid.export(rows(), &request).await? {
        return Ok(file);
    }
    let rows = grid.page(rows(), &request).await?;
    Ok(page("payments", rows).into_response())
}
// [/explain:reports.payments.handler]

// [explain:reports.customers.query]
/// The customers a person may see in the reports: everyone for a global
/// role, else those with income at one of their stores (the store that
/// did the work, or owns what was sold or rented).
pub fn visible_customers() -> renox::db::Query<CustomerValue> {
    use renox::auth::permissions::{self, Scopes};
    match permissions::scopes_with::<Store>(catalogue::REPORTS_VIEW) {
        Scopes::All => CustomerValue::query(),
        Scopes::Only(ids) if ids.is_empty() => CustomerValue::query().none(),
        Scopes::Only(ids) => {
            let marks = vec!["?"; ids.len()].join(", ");
            let mut binds = ids.clone();
            binds.extend(ids.iter().copied());
            CustomerValue::query().where_raw(
                &format!(
                    "id IN (SELECT customer_id FROM report_revenue WHERE operating_store_id IN ({marks}) \
                     OR owner_store_id IN ({marks}))"
                ),
                binds,
            )
        }
    }
}

/// `GET /staff/reports/customers` (`reports.customers`).
pub async fn customers(lang: Lang, request: GridRequest) -> Result<Response> {
    let grid = customers_grid(&lang);
    if let Some(file) = grid.export(visible_customers(), &request).await? {
        return Ok(file);
    }
    let rows = grid.page(visible_customers(), &request).await?;
    Ok(page("customers", rows).into_response())
}
// [/explain:reports.customers.query]

// [explain:reports.entries.handler]
/// `GET /staff/reports/intercompany` (`reports.entries`).
pub async fn entries(State(db): State<Db>, lang: Lang, request: GridRequest) -> Result<Response> {
    let stores = Store::all_by_name(&db).await?;
    let grid = entries_grid(&lang, &stores);
    let rows = || access::visible::<EntryRow>(catalogue::REPORTS_VIEW);
    if let Some(file) = grid.export(rows(), &request).await? {
        return Ok(file);
    }
    let rows = grid.page(rows(), &request).await?.extend(|e| {
        json!({
            "source_url": crate::app::multistore::intercompany::source_url(&e.source_type, e.source_id),
            "source_label": format!("{} #{}", e.source_type, e.source_id),
        })
    });
    Ok(page("entries", rows).into_response())
}
// [/explain:reports.entries.handler]
