//! Stock with two store attributes (#245), suppliers and purchasing.
//!
//! Goods have an **owner store** (whose books they are in) and a
//! **location store** (where they are). Normally both are the same; goods
//! sent to another store on consignment change location only, and stay the
//! owner's until sold there.
//!
//! - [`StockMovement`] is the ledger: every change, `+` or `−`, with its
//!   reason ([`MovementReason`]) and what caused it (a [`Morph`] reference
//!   to the order, work order, shipment or purchase order).
//! - [`StockLevel`] holds the sums per variant × owner × location, kept in
//!   step with the ledger by [`StockMovement::record`] in one transaction.
//!
//! Migration: `migrations/20260101000500_create_stock_tables.*`.

use renox::db::relations::Morph;
use renox::db::{Transaction, sql};
use renox::prelude::*;
use serde::Serialize;

use crate::app::access::{StoreAttr, StoreRecord, catalogue};

/// Why stock moved.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MovementReason {
    /// Received from a supplier (`+`).
    #[default]
    Purchase,
    /// Sold (`−`).
    Sale,
    /// A customer brought it back (`+`).
    Return,
    /// Used in a work order (`−`).
    Service,
    /// Sent to another store on consignment (`−` at the sender's location).
    ConsignOut,
    /// Arrived on consignment (`+` at the receiving location).
    ConsignIn,
    /// Consigned goods called back by their owner.
    Recall,
    /// Corrected by hand (a stock take, damage).
    Adjustment,
    /// Put aside for an order (moves `on_hand` to `reserved`).
    Reserved,
    /// Put back from `reserved`.
    Released,
    /// A new bike taken from sale stock into the rental fleet (`−`).
    ToFleet,
    /// A rental bike retired into sale stock, to be sold as used (`+`).
    FromFleet,
}

/// What caused a movement: `reference_type` is the table (`orders`,
/// `work_orders`, `consignment_shipments`, `purchase_orders`) and
/// `reference_id` the row.
pub const REFERENCE: Morph = Morph::new("reference_type", "reference_id");

/// How many of a variant one store owns at one location.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "stock_levels")]
pub struct StockLevel {
    pub id: i64,
    pub variant_id: i64,
    pub owner_store_id: i64,
    pub location_store_id: i64,
    pub on_hand: i64,
    /// Put aside for orders not picked up yet.
    pub reserved: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl StockLevel {
    /// What can still be sold: on hand minus reserved.
    pub fn available(&self) -> i64 {
        self.on_hand - self.reserved
    }

    /// Whether these are another store's goods held here on consignment.
    pub fn consigned(&self) -> bool {
        self.owner_store_id != self.location_store_id
    }
}

impl StoreRecord for StockLevel {
    const VIEW: &'static str = catalogue::STOCK_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["owner_store_id", "location_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.owner_store_id),
            StoreAttr::Location | StoreAttr::Operating => Some(self.location_store_id),
        }
    }
}

/// One line of the stock ledger.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "stock_movements")]
pub struct StockMovement {
    pub id: i64,
    pub variant_id: i64,
    pub owner_store_id: i64,
    pub location_store_id: i64,
    /// `+` in, `−` out.
    pub quantity: i64,
    pub reason: MovementReason,
    pub reference_type: Option<String>,
    pub reference_id: Option<i64>,
    /// Who did it (a `staff` row).
    pub staff_id: Option<i64>,
    pub note: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl StoreRecord for StockMovement {
    const VIEW: &'static str = catalogue::STOCK_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["owner_store_id", "location_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.owner_store_id),
            StoreAttr::Location | StoreAttr::Operating => Some(self.location_store_id),
        }
    }
}

impl StockMovement {
    /// Writes `movement` to the ledger and applies it to its stock level
    /// (creating the level when it's the first), in the caller's
    /// transaction, so both happen or neither. `Reserved` / `Released` move
    /// units between `on_hand` and `reserved` instead.
    pub async fn record(
        tx: &mut Transaction,
        mut movement: StockMovement,
    ) -> Result<StockMovement> {
        let (on_hand, reserved) = match movement.reason {
            MovementReason::Reserved => (0, movement.quantity),
            MovementReason::Released => (0, -movement.quantity),
            _ => (movement.quantity, 0),
        };
        let at = renox::db::now();
        sql(
            "INSERT INTO stock_levels (variant_id, owner_store_id, location_store_id, on_hand, \
             reserved, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (variant_id, owner_store_id, location_store_id) DO UPDATE SET \
             on_hand = stock_levels.on_hand + excluded.on_hand, \
             reserved = stock_levels.reserved + excluded.reserved, updated_at = excluded.updated_at",
        )
        .bind(movement.variant_id)
        .bind(movement.owner_store_id)
        .bind(movement.location_store_id)
        .bind(on_hand)
        .bind(reserved)
        .bind(at)
        .bind(at)
        .execute(&mut *tx)
        .await?;
        movement.insert(&mut *tx).await?;
        Ok(movement)
    }
}

/// Someone the shop buys from.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "suppliers")]
pub struct Supplier {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address_id: Option<i64>,
    /// Usual days between ordering and delivery.
    pub lead_days: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// Where a purchase order stands: draft → ordered (mailed to the
/// supplier) → partial → received, or cancelled.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PurchaseStatus {
    #[default]
    Draft,
    Ordered,
    /// Some lines arrived.
    Partial,
    Received,
    Cancelled,
}

/// An order to a supplier, for one store (which will own the goods).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "purchase_orders")]
pub struct PurchaseOrder {
    pub id: i64,
    pub supplier_id: i64,
    /// The store that orders, receives and owns the goods.
    pub store_id: i64,
    pub status: PurchaseStatus,
    pub ordered_at: Option<DateTime>,
    pub expected_on: Option<renox::chrono::NaiveDate>,
    pub received_at: Option<DateTime>,
    pub total: i64,
    pub created_by: Option<i64>,
    /// A note for the supplier, printed on the order.
    pub note: Option<String>,
    /// Drafted by the daily reorder check rather than by a person.
    pub suggested: bool,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl StoreRecord for PurchaseOrder {
    const VIEW: &'static str = catalogue::STOCK_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["store_id"];

    fn store_id(&self, _attr: StoreAttr) -> Option<i64> {
        Some(self.store_id)
    }
}

/// A line of a purchase order.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "purchase_order_lines")]
pub struct PurchaseOrderLine {
    pub id: i64,
    pub purchase_order_id: i64,
    pub variant_id: i64,
    pub quantity: i64,
    pub received_quantity: i64,
    pub unit_cost: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// Where a consignment shipment stands (see `src/app/stock/consignment.rs`
/// for who moves it on).
///
/// ```text
/// requested ─→ approved ─→ sent ─→ partly received ─→ received ─→ recall requested ─→ recall sent ─→ recalled
///     └─→ refused            └──────────────────────────↗
/// ```
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShipmentStatus {
    /// Being put together by the owner store (older rows; new shipments
    /// start as requested or approved).
    #[default]
    Draft,
    /// Asked for by the location store, waiting for the owner store.
    Requested,
    /// Approved by the owner store (or made by it), not shipped yet.
    Approved,
    /// The owner store said no.
    Refused,
    /// On its way to the location store.
    Sent,
    /// Some of it arrived; the rest is still on its way.
    PartlyReceived,
    /// Arrived: the goods are at the location store, still the owner's.
    Received,
    /// The owner asked for what's left back.
    RecallRequested,
    /// The location store sent what was left back; on its way home.
    RecallSent,
    /// What was left is back at the owner store.
    Recalled,
}

impl ShipmentStatus {
    /// Goods are between two stores (either way).
    pub fn in_transit(self) -> bool {
        matches!(
            self,
            ShipmentStatus::Sent | ShipmentStatus::PartlyReceived | ShipmentStatus::RecallSent
        )
    }
}

/// Goods sent by their owner store to another store, to be sold there on
/// consignment (#245): their location changes, their owner doesn't.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "consignment_shipments")]
pub struct ConsignmentShipment {
    pub id: i64,
    pub owner_store_id: i64,
    pub location_store_id: i64,
    pub status: ShipmentStatus,
    pub sent_at: Option<DateTime>,
    pub received_at: Option<DateTime>,
    pub recalled_at: Option<DateTime>,
    pub created_by: Option<i64>,
    pub note: Option<String>,
    /// The user who asked for it (the location store's, or the owner's own).
    pub requested_by: Option<i64>,
    /// The user of the owner store who approved it.
    pub approved_by: Option<i64>,
    pub approved_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl StoreRecord for ConsignmentShipment {
    const VIEW: &'static str = catalogue::STOCK_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["owner_store_id", "location_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.owner_store_id),
            StoreAttr::Location | StoreAttr::Operating => Some(self.location_store_id),
        }
    }
}

/// A line of a consignment shipment: sent, sold there, sent back.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "consignment_shipment_lines")]
pub struct ConsignmentShipmentLine {
    pub id: i64,
    pub shipment_id: i64,
    pub variant_id: i64,
    /// Sent (or asked for, before it ships).
    pub quantity: i64,
    pub sold_quantity: i64,
    /// Sent back to the owner store by a recall.
    pub returned_quantity: i64,
    /// Arrived at the location store so far (a partial receipt is less).
    pub received_quantity: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// What one supplier sells and at what cost: their price list, kept by
/// the price list import (`src/app/stock/import.rs`). The purchase order
/// form and the reorder check pick a supplier's items from it.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "supplier_items")]
pub struct SupplierItem {
    pub id: i64,
    pub supplier_id: i64,
    pub variant_id: i64,
    /// The supplier's price to us, in the smallest unit of `APP_CURRENCY`.
    pub cost: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// One stock level as the stock grid shows it: a row of the database view
/// `stock_overview` (`migrations/20260101002400_add_stock_details.*`),
/// which joins the level with its variant, product and category. A view
/// rather than a join in Rust, so the grid can filter, sort, **group** and
/// **sum** these columns like any of a model's own (`renox::grid` groups
/// and sums only a model's columns). Read only: levels change through
/// [`StockMovement::record`].
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "stock_overview")]
pub struct StockRow {
    /// The stock level's id.
    pub id: i64,
    pub variant_id: i64,
    pub owner_store_id: i64,
    pub location_store_id: i64,
    pub on_hand: i64,
    pub reserved: i64,
    pub available: i64,
    pub sku: String,
    pub size: Option<String>,
    pub colour: Option<String>,
    /// The variant's average cost.
    pub cost: i64,
    pub reorder_level: i64,
    /// `on_hand × cost`.
    pub value_at_cost: i64,
    pub product_id: i64,
    pub product: String,
    pub category: String,
    /// Bikes, gear or parts.
    pub category_kind: crate::app::catalog::model::CategoryKind,
    pub updated_at: Option<DateTime>,
}

impl StoreRecord for StockRow {
    const VIEW: &'static str = catalogue::STOCK_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["owner_store_id", "location_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.owner_store_id),
            StoreAttr::Location | StoreAttr::Operating => Some(self.location_store_id),
        }
    }
}
