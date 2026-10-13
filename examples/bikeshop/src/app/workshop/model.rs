//! The workshop: customers' bikes, service tasks and work orders.
//!
//! A [`WorkOrder`] is done by one store's workshop (`store_id`) on a
//! customer's bike or on a bike of the rental fleet. A fleet repair of a
//! bike owned by another store is billed to that owner store
//! (`billed_store_id`), which the intercompany books record (#245).
//!
//! Migration: `migrations/20260101000800_create_workshop_tables.*`.

use renox::chrono::NaiveDate;
use renox::prelude::*;
use serde::Serialize;

use crate::app::access::{StoreAttr, StoreRecord, catalogue};

/// A customer's own bike, registered to them (bought here or brought in).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "customer_bikes")]
pub struct CustomerBike {
    pub id: i64,
    pub customer_id: i64,
    /// The bike model, when it's one of the catalogue's.
    pub product_id: Option<i64>,
    /// What the customer calls it: "Trek Domane AL 2, blue".
    pub name: String,
    pub frame_number: Option<String>,
    /// The order it was bought with, when bought here.
    pub order_id: Option<i64>,
    pub bought_on: Option<NaiveDate>,
    /// The brand when it isn't a catalogue model ("Gazelle").
    pub brand: Option<String>,
    /// The frame size ("M", "54 cm").
    pub size: Option<String>,
    /// A photo, as a private storage key (shown through a signed link).
    #[serde(skip_serializing)]
    pub photo_path: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// Something a mechanic does, with its usual time and price.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "service_tasks")]
pub struct ServiceTask {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub minutes: i64,
    /// Labour price in the smallest unit of `APP_CURRENCY`.
    pub price: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// Why a work order exists.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkSource {
    /// A customer walked in with their bike.
    #[default]
    WalkIn,
    /// Booked online.
    Booking,
    /// A visit of a service plan.
    Plan,
    /// A repair of a rental bike.
    Fleet,
}

/// Where a work order stands.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkStatus {
    /// Booked for `scheduled_for`.
    #[default]
    Booked,
    /// The bike is at the workshop.
    CheckedIn,
    /// A mechanic is on it.
    InProgress,
    /// Waiting for a part to arrive.
    WaitingParts,
    /// Extra work proposed, waiting for the customer's answer.
    WaitingApproval,
    /// Done, waiting for the customer.
    Ready,
    /// Picked up (and paid).
    Completed,
    Cancelled,
}

/// A job for a store's workshop.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "work_orders")]
pub struct WorkOrder {
    pub id: i64,
    /// A customer's bike…
    pub customer_bike_id: Option<i64>,
    /// …or a bike of the rental fleet.
    pub rental_bike_id: Option<i64>,
    /// The store whose workshop does it.
    pub store_id: i64,
    /// The mechanic (a `staff` row).
    pub mechanic_id: Option<i64>,
    pub source: WorkSource,
    pub plan_subscription_id: Option<i64>,
    /// For a fleet repair of another store's bike: the owner store, which
    /// pays for it.
    pub billed_store_id: Option<i64>,
    pub scheduled_for: DateTime,
    pub status: WorkStatus,
    /// Money in the smallest unit of `APP_CURRENCY`.
    pub labour: i64,
    pub parts: i64,
    pub total: i64,
    pub customer_note: Option<String>,
    pub started_at: Option<DateTime>,
    pub completed_at: Option<DateTime>,
    /// Mechanic time booked, for the store's daily capacity.
    pub minutes: i64,
    /// The package chosen when booking (`tune-up`, `overhaul`), if any.
    pub package: Option<String>,
    /// When the "tomorrow" reminder went out.
    pub reminded_at: Option<DateTime>,
    pub cancelled_at: Option<DateTime>,
    /// When the customer paid (online or at the counter).
    pub paid_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl WorkOrder {
    /// Still to be worked on or collected.
    pub fn is_open(&self) -> bool {
        !matches!(self.status, WorkStatus::Completed | WorkStatus::Cancelled)
    }

    /// The customer may still reschedule or cancel it: booked, and more
    /// than 24 hours before the day.
    pub fn changeable(&self) -> bool {
        self.status == WorkStatus::Booked
            && renox::db::now() < self.scheduled_for - renox::chrono::Duration::hours(24)
    }
}

impl StoreRecord for WorkOrder {
    const VIEW: &'static str = catalogue::WORKORDERS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["store_id", "billed_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Operating | StoreAttr::Location => Some(self.store_id),
            StoreAttr::Owner => Some(self.billed_store_id.unwrap_or(self.store_id)),
        }
    }
}

/// A task of a work order: done or not, with a note.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "work_order_tasks")]
pub struct WorkOrderTask {
    pub id: i64,
    pub work_order_id: i64,
    pub service_task_id: i64,
    pub minutes: i64,
    pub price: i64,
    pub done: bool,
    pub note: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// The work orders of a page of customers, through their bikes (customer →
/// customer bikes → work orders), newest first, in two queries: Renox's
/// `has_many_through`. Keyed by customer id.
pub fn work_orders_of<'a>(
    db: &'a Db,
    customers: &[crate::app::accounts::model::Customer],
) -> impl std::future::Future<Output = Result<std::collections::HashMap<i64, Vec<WorkOrder>>>> + Send + 'a
{
    renox::db::relations::has_many_through(
        db,
        customers,
        CustomerBike::query(),
        "customer_id",
        |bike: &CustomerBike| bike.customer_id,
        WorkOrder::query().order_by_desc(WorkOrder::SCHEDULED_FOR),
        "customer_bike_id",
        |order: &WorkOrder| order.customer_bike_id,
    )
}

/// Whether a part was taken from stock or is waited for.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PartStatus {
    /// Taken from the store's stock (a ledger movement, reason `service`).
    #[default]
    Used,
    /// Out of stock: the work order waits for it (purchasing, #240).
    Waiting,
}

/// A part used on a work order.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "work_order_parts")]
pub struct WorkOrderPart {
    pub id: i64,
    pub work_order_id: i64,
    /// The part (a catalogue variant).
    pub variant_id: i64,
    pub quantity: i64,
    /// Money in the smallest unit of `APP_CURRENCY`.
    pub unit_price: i64,
    pub total: i64,
    pub status: PartStatus,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// What a note on a work order is.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteKind {
    /// Text only.
    #[default]
    Note,
    /// A photo before the work.
    Before,
    /// A photo after the work.
    After,
}

/// A mechanic's note or photo on a work order.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "work_order_notes")]
pub struct WorkOrderNote {
    pub id: i64,
    pub work_order_id: i64,
    /// Who wrote it (a `staff` row).
    pub staff_id: Option<i64>,
    pub kind: NoteKind,
    pub body: Option<String>,
    /// A private storage key, shown through a signed link.
    #[serde(skip_serializing)]
    pub photo_path: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// One task or part proposed as extra work.
#[derive(Serialize, serde::Deserialize, Default, Debug, Clone, PartialEq)]
pub struct ExtraItem {
    /// `task` or `part`.
    pub kind: String,
    /// The service task's or the variant's id.
    pub id: i64,
    pub name: String,
    pub quantity: i64,
    /// The line's price.
    pub price: i64,
}

/// Where a proposal of extra work stands.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtraStatus {
    /// Sent to the customer, not answered yet.
    #[default]
    Pending,
    Approved,
    Refused,
}

/// Extra work the mechanic proposes; the customer answers through a signed
/// link in a mail (no login), once, before it expires.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "extra_work_requests")]
pub struct ExtraWork {
    pub id: i64,
    pub work_order_id: i64,
    /// Why it's needed, in the mechanic's words.
    pub description: String,
    pub items: renox::db::Json<Vec<ExtraItem>>,
    pub total: i64,
    pub status: ExtraStatus,
    pub expires_at: DateTime,
    pub decided_at: Option<DateTime>,
    /// Who proposed it (a `staff` row).
    pub created_by: Option<i64>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}
