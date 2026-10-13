//! What the reports read: database **views** over the other areas' tables
//! (`migrations/20260102001000_create_report_views.*`), each with a
//! read-only model.
//!
//! Nothing here is copied or kept in step by code: a view is a saved
//! query, so a rental returned a second ago is in `report_revenue` now.
//! The views exist so that `renox::grid` and `renox::chart` can work on
//! plain columns: a grid filters, groups and sums only a model's own
//! columns, and `Trend` buckets one model's date column. A store's name
//! is a column of `report_orders`, so the orders grid can group by it.
//!
//! | View | Model | One row per |
//! |---|---|---|
//! | `report_revenue` | [`RevenueLine`] | line of income (order line, returned rental, completed work order, plan payment), with both store attributes |
//! | `report_customers` | [`CustomerValue`] | customer, with their lifetime value per stream |
//! | `report_orders` | [`OrderRow`] | order, with its store and customer by name |
//! | `report_rentals` | [`RentalRow`] | rental, with both stores, the bike, its model and category |
//! | `report_work_orders` | [`WorkOrderRow`] | work order, with the workshop, customer, bike and mechanic |
//! | `report_payments` | [`PaymentRow`] | payment, with the store and customer |
//! | `report_entries` | [`EntryRow`] | entry of the books between stores, with both stores and its settlement |
//!
//! Every model is a [`StoreRecord`] whose permission is `reports.view`, so
//! lists go through the same `scopes_with` + `Scopes::apply` filter as the
//! rest of the app ("rows of the stores where I may see reports").

use renox::prelude::*;
use serde::Serialize;

use crate::app::access::{StoreAttr, StoreRecord, catalogue};

/// The four ways the shop earns money.
pub const STREAMS: [&str; 4] = ["sales", "rentals", "workshop", "plans"];

/// One line of income (`report_revenue`).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "report_revenue")]
pub struct RevenueLine {
    /// Unique across the view's four parts (the source's id × 4 + part).
    pub id: i64,
    /// `sales`, `rentals`, `workshop` or `plans` ([`STREAMS`]).
    pub stream: String,
    /// `orders`, `rentals`, `work_orders` or `plan_subscriptions`.
    pub source_type: String,
    pub source_id: i64,
    /// Whose books it is in: the goods' or the bike's owner store.
    pub owner_store_id: i64,
    /// The store that did the work (served the customer).
    pub operating_store_id: i64,
    pub customer_id: Option<i64>,
    /// For sales and rentals: the product sold, or the rented bike's model.
    pub product_id: Option<i64>,
    pub category_id: Option<i64>,
    pub quantity: i64,
    /// In the smallest unit of `APP_CURRENCY`.
    pub amount: i64,
    /// When it was earned: paid, returned, completed.
    pub booked_at: DateTime,
}

impl StoreRecord for RevenueLine {
    const VIEW: &'static str = catalogue::REPORTS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["owner_store_id", "operating_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.owner_store_id),
            StoreAttr::Operating | StoreAttr::Location => Some(self.operating_store_id),
        }
    }
}

/// A customer and what they brought in (`report_customers`): company-wide,
/// since customers belong to the company, not to a store.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "report_customers")]
pub struct CustomerValue {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub city: Option<String>,
    pub created_at: Option<DateTime>,
    pub sales_value: i64,
    pub rentals_value: i64,
    pub workshop_value: i64,
    pub plans_value: i64,
    /// Orders + rentals + services + plans.
    pub lifetime_value: i64,
    /// Orders, rentals and work orders (each counted once).
    pub visits: i64,
    pub first_at: Option<DateTime>,
    pub last_at: Option<DateTime>,
}

/// An order (`report_orders`).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "report_orders")]
pub struct OrderRow {
    pub id: i64,
    pub number: String,
    pub operating_store_id: i64,
    /// The selling store's name.
    pub store: String,
    pub customer_id: Option<i64>,
    /// Empty for an anonymous counter sale.
    pub customer: String,
    pub channel: String,
    pub fulfilment: String,
    pub status: String,
    /// Units on the order.
    pub units: i64,
    pub subtotal: i64,
    pub discount: i64,
    pub delivery_fee: i64,
    pub total: i64,
    pub placed_at: Option<DateTime>,
    pub paid_at: Option<DateTime>,
    pub completed_at: Option<DateTime>,
}

impl StoreRecord for OrderRow {
    const VIEW: &'static str = catalogue::REPORTS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["operating_store_id"];

    fn store_id(&self, _attr: StoreAttr) -> Option<i64> {
        Some(self.operating_store_id)
    }
}

/// A rental (`report_rentals`).
// [explain:reports.rentals.model]
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "report_rentals")]
pub struct RentalRow {
    pub id: i64,
    /// The reservation code.
    pub code: String,
    pub operating_store_id: i64,
    pub owner_store_id: i64,
    // [/explain:reports.rentals.model]
    /// The store that served it.
    pub store: String,
    /// The bike's owner store.
    pub owner_store: String,
    pub customer: String,
    /// The bike's frame number.
    pub bike: String,
    pub model: String,
    pub category: String,
    pub rate: String,
    pub status: String,
    pub starts_at: DateTime,
    pub due_at: DateTime,
    pub picked_up_at: Option<DateTime>,
    pub returned_at: Option<DateTime>,
    pub price: i64,
    pub late_fee: i64,
    pub damage_fee: i64,
    /// Price + late fee + damage fee.
    pub total: i64,
    pub deposit: i64,
    // [explain:reports.rentals.model]
}

impl StoreRecord for RentalRow {
    const VIEW: &'static str = catalogue::REPORTS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["operating_store_id", "owner_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.owner_store_id),
            StoreAttr::Operating | StoreAttr::Location => Some(self.operating_store_id),
        }
    }
}
// [/explain:reports.rentals.model]

/// A work order (`report_work_orders`).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "report_work_orders")]
pub struct WorkOrderRow {
    pub id: i64,
    pub store_id: i64,
    /// The workshop's store.
    pub store: String,
    /// For a fleet repair of another store's bike: the store billed.
    pub billed_store_id: Option<i64>,
    pub source: String,
    pub status: String,
    /// Empty for a fleet repair.
    pub customer: String,
    /// The customer's bike, or the rental bike's frame number.
    pub bike: String,
    pub mechanic_name: String,
    pub scheduled_for: DateTime,
    pub started_at: Option<DateTime>,
    pub completed_at: Option<DateTime>,
    pub labour: i64,
    pub parts: i64,
    pub total: i64,
}

impl StoreRecord for WorkOrderRow {
    const VIEW: &'static str = catalogue::REPORTS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["store_id"];

    fn store_id(&self, _attr: StoreAttr) -> Option<i64> {
        Some(self.store_id)
    }
}

/// A payment (`report_payments`).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "report_payments")]
pub struct PaymentRow {
    pub id: i64,
    pub store_id: i64,
    /// The store that received it.
    pub store: String,
    pub customer_id: Option<i64>,
    pub customer: String,
    /// What it paid for: `orders`, `rentals`, `work_orders`…
    pub kind: String,
    pub payable_id: i64,
    pub amount: i64,
    pub method: String,
    pub status: String,
    pub paid_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    /// The gateway's reference, if any.
    pub reference: String,
}

impl StoreRecord for PaymentRow {
    const VIEW: &'static str = catalogue::REPORTS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["store_id"];

    fn store_id(&self, _attr: StoreAttr) -> Option<i64> {
        Some(self.store_id)
    }
}

/// An entry of the books between stores (`report_entries`).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "report_entries")]
pub struct EntryRow {
    pub id: i64,
    pub booked_at: DateTime,
    pub kind: String,
    pub debtor_store_id: i64,
    pub creditor_store_id: i64,
    /// The store that owes.
    pub debtor: String,
    /// The store that is owed.
    pub creditor: String,
    pub amount: i64,
    pub fee_rate_bp: Option<i64>,
    pub source_type: String,
    pub source_id: i64,
    pub settlement_id: Option<i64>,
    /// `unsettled`, `open` or `settled`.
    pub settlement: String,
}

impl StoreRecord for EntryRow {
    const VIEW: &'static str = catalogue::REPORTS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["debtor_store_id", "creditor_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.creditor_store_id),
            StoreAttr::Operating | StoreAttr::Location => Some(self.debtor_store_id),
        }
    }
}
