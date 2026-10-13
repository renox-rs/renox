//! Factories for the workshop: `work_orders().at(store).waiting_parts()`,
//! `work_orders().fleet_repair(&bike, store)`…

use renox::chrono::Duration;
use renox::db::FactoryBuilder;
use renox::fake::Fake;
use renox::prelude::*;

use super::model::{CustomerBike, ServiceTask, WorkOrder, WorkOrderTask, WorkSource, WorkStatus};
use crate::app::rentals::model::RentalBike;
use crate::seed::unique;

impl Factory for CustomerBike {
    fn definition() -> Self {
        CustomerBike {
            name: "City bike, grey".into(),
            frame_number: Some(format!("CB{:08}", unique())),
            ..Default::default()
        }
    }
}

/// `CustomerBike::factory()` of `customer_id`.
pub fn customer_bikes_of(customer_id: i64) -> FactoryBuilder<CustomerBike> {
    CustomerBike::factory().state(move |b| b.customer_id = customer_id)
}

impl Factory for ServiceTask {
    fn definition() -> Self {
        let n = unique();
        ServiceTask {
            name: format!("Task {n}"),
            slug: format!("task-{n}"),
            minutes: (2..10).fake::<i64>() * 10,
            price: (20..80).fake::<i64>() * 100,
            ..Default::default()
        }
    }
}

impl Factory for WorkOrder {
    fn definition() -> Self {
        WorkOrder {
            source: WorkSource::WalkIn,
            status: WorkStatus::Booked,
            scheduled_for: renox::db::now() + Duration::days(1),
            ..Default::default()
        }
    }
}

/// `WorkOrder::factory()`.
pub fn work_orders() -> FactoryBuilder<WorkOrder> {
    WorkOrder::factory()
}

/// States of a work order.
pub trait WorkOrderStates {
    /// Done by `store_id`'s workshop.
    fn at(self, store_id: i64) -> Self;
    /// On `customer_bike_id`.
    fn on_bike(self, customer_bike_id: i64) -> Self;
    /// A repair of `bike` by `store_id`'s workshop, billed to its owner when
    /// that's another store.
    fn fleet_repair(self, bike: &RentalBike, store_id: i64) -> Self;
    /// Being worked on since this morning.
    fn in_progress(self) -> Self;
    /// Waiting for a part.
    fn waiting_parts(self) -> Self;
    /// Finished and picked up yesterday, labour `labour`.
    fn completed(self, labour: i64) -> Self;
}

impl WorkOrderStates for FactoryBuilder<WorkOrder> {
    fn at(self, store_id: i64) -> Self {
        self.state(move |w| w.store_id = store_id)
    }

    fn on_bike(self, customer_bike_id: i64) -> Self {
        self.state(move |w| w.customer_bike_id = Some(customer_bike_id))
    }

    fn fleet_repair(self, bike: &RentalBike, store_id: i64) -> Self {
        let (bike_id, owner) = (bike.id, bike.owner_store_id);
        self.state(move |w| {
            w.rental_bike_id = Some(bike_id);
            w.customer_bike_id = None;
            w.store_id = store_id;
            w.source = WorkSource::Fleet;
            w.billed_store_id = (owner != store_id).then_some(owner);
        })
    }

    fn in_progress(self) -> Self {
        self.state(|w| {
            let now = renox::db::now();
            w.status = WorkStatus::InProgress;
            w.scheduled_for = now - Duration::hours(3);
            w.started_at = Some(now - Duration::hours(2));
        })
    }

    fn waiting_parts(self) -> Self {
        self.state(|w| {
            let now = renox::db::now();
            w.status = WorkStatus::WaitingParts;
            w.scheduled_for = now - Duration::days(1);
            w.started_at = Some(now - Duration::days(1));
        })
    }

    fn completed(self, labour: i64) -> Self {
        self.state(move |w| {
            let now = renox::db::now();
            w.status = WorkStatus::Completed;
            w.scheduled_for = now - Duration::days(2);
            w.started_at = Some(now - Duration::days(2));
            w.completed_at = Some(now - Duration::days(1));
            w.labour = labour;
            w.total = labour + w.parts;
        })
    }
}

impl Factory for WorkOrderTask {
    fn definition() -> Self {
        WorkOrderTask {
            minutes: 30,
            price: 3_000,
            ..Default::default()
        }
    }
}
