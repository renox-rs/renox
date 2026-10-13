//! Factories for stock, suppliers and purchasing:
//! `stock_levels().of(variant).consigned(owner, location).low()`…

use renox::chrono::Duration;
use renox::db::FactoryBuilder;
use renox::fake::Fake;
use renox::fake::faker::company::en::CompanyName;
use renox::prelude::*;

use super::model::{
    ConsignmentShipment, ConsignmentShipmentLine, MovementReason, PurchaseOrder, PurchaseOrderLine,
    PurchaseStatus, ShipmentStatus, StockLevel, StockMovement, Supplier,
};
use crate::seed::unique;

impl Factory for StockLevel {
    fn definition() -> Self {
        StockLevel {
            on_hand: (3..25).fake(),
            ..Default::default()
        }
    }
}

/// `StockLevel::factory()`.
pub fn stock_levels() -> FactoryBuilder<StockLevel> {
    StockLevel::factory()
}

/// States of a stock level.
pub trait StockStates {
    /// Of `variant_id`, owned and held by `store_id`.
    fn of(self, variant_id: i64, store_id: i64) -> Self;
    /// Owned by `owner`, held by `location` (consigned).
    fn consigned(self, owner: i64, location: i64) -> Self;
    /// Below a reorder level of 5.
    fn low(self) -> Self;
}

impl StockStates for FactoryBuilder<StockLevel> {
    fn of(self, variant_id: i64, store_id: i64) -> Self {
        self.state(move |l| {
            l.variant_id = variant_id;
            l.owner_store_id = store_id;
            l.location_store_id = store_id;
        })
    }

    fn consigned(self, owner: i64, location: i64) -> Self {
        self.state(move |l| {
            l.owner_store_id = owner;
            l.location_store_id = location;
        })
    }

    fn low(self) -> Self {
        self.state(|l| l.on_hand = (0..2).fake())
    }
}

impl Factory for StockMovement {
    fn definition() -> Self {
        StockMovement {
            quantity: (1..10).fake(),
            reason: MovementReason::Purchase,
            ..Default::default()
        }
    }
}

impl Factory for Supplier {
    fn definition() -> Self {
        Supplier {
            name: CompanyName().fake(),
            email: Some(format!("orders{}@supplier.example", unique())),
            lead_days: (3..21).fake(),
            ..Default::default()
        }
    }
}

impl Factory for PurchaseOrder {
    fn definition() -> Self {
        PurchaseOrder {
            status: PurchaseStatus::Draft,
            ..Default::default()
        }
    }
}

/// States of a purchase order.
pub trait PurchaseStates {
    /// Sent to the supplier, expected in a week.
    fn ordered(self) -> Self;
    /// Delivered.
    fn received(self) -> Self;
}

impl PurchaseStates for FactoryBuilder<PurchaseOrder> {
    fn ordered(self) -> Self {
        self.state(|o| {
            o.status = PurchaseStatus::Ordered;
            o.ordered_at = Some(renox::db::now() - Duration::days(2));
            o.expected_on = Some(crate::seed::today() + Duration::days(5));
        })
    }

    fn received(self) -> Self {
        self.state(|o| {
            o.status = PurchaseStatus::Received;
            o.ordered_at = Some(renox::db::now() - Duration::days(12));
            o.received_at = Some(renox::db::now() - Duration::days(3));
        })
    }
}

impl Factory for PurchaseOrderLine {
    fn definition() -> Self {
        PurchaseOrderLine {
            quantity: (2..12).fake(),
            unit_cost: (10..500).fake::<i64>() * 100,
            ..Default::default()
        }
    }
}

impl Factory for ConsignmentShipment {
    fn definition() -> Self {
        ConsignmentShipment::default()
    }
}

/// States of a consignment shipment.
pub trait ShipmentStates {
    /// From `owner` to `location`.
    fn between(self, owner: i64, location: i64) -> Self;
    /// Arrived at the location store.
    fn received(self) -> Self;
    /// Called back by the owner, back home.
    fn recalled(self) -> Self;
}

impl ShipmentStates for FactoryBuilder<ConsignmentShipment> {
    fn between(self, owner: i64, location: i64) -> Self {
        self.state(move |s| {
            s.owner_store_id = owner;
            s.location_store_id = location;
        })
    }

    fn received(self) -> Self {
        self.state(|s| {
            s.status = ShipmentStatus::Received;
            s.sent_at = Some(renox::db::now() - Duration::days(10));
            s.received_at = Some(renox::db::now() - Duration::days(9));
        })
    }

    fn recalled(self) -> Self {
        self.state(|s| {
            s.status = ShipmentStatus::Recalled;
            s.sent_at = Some(renox::db::now() - Duration::days(60));
            s.received_at = Some(renox::db::now() - Duration::days(59));
            s.recalled_at = Some(renox::db::now() - Duration::days(5));
        })
    }
}

impl Factory for ConsignmentShipmentLine {
    fn definition() -> Self {
        ConsignmentShipmentLine {
            quantity: (2..8).fake(),
            ..Default::default()
        }
    }
}
