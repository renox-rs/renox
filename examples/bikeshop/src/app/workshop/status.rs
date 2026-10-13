//! A work order's life: which status may follow which, what changing it
//! does (the customer is told; collecting a fleet repair puts the bike back
//! in the fleet), and its totals.
//!
//! ```text
//! booked → checked in → in progress → waiting for parts ─┐
//!                            ↑  ↓    → waiting for approval ┤ → in progress
//!                            └─ ready → collected            │
//! booked → cancelled                                         ┘
//! ```

use renox::prelude::*;

use super::model::{CustomerBike, WorkOrder, WorkOrderPart, WorkOrderTask, WorkSource, WorkStatus};
use crate::app::accounts::model::Customer;
use crate::app::accounts::preferences::Kind;
use crate::app::rentals::model::{BikeCondition, BikeStatus, RentalBike};
use crate::app::rentals::notify::{self, Notice, Tone};

/// The board's columns, in order (collected and cancelled orders leave it).
pub const BOARD: [WorkStatus; 6] = [
    WorkStatus::Booked,
    WorkStatus::CheckedIn,
    WorkStatus::InProgress,
    WorkStatus::WaitingParts,
    WorkStatus::WaitingApproval,
    WorkStatus::Ready,
];

/// A status's key, as stored and in URLs (`waiting_parts`).
pub fn key(status: WorkStatus) -> &'static str {
    match status {
        WorkStatus::Booked => "booked",
        WorkStatus::CheckedIn => "checked_in",
        WorkStatus::InProgress => "in_progress",
        WorkStatus::WaitingParts => "waiting_parts",
        WorkStatus::WaitingApproval => "waiting_approval",
        WorkStatus::Ready => "ready",
        WorkStatus::Completed => "completed",
        WorkStatus::Cancelled => "cancelled",
    }
}

/// The status with this key.
pub fn from_key(text: &str) -> Option<WorkStatus> {
    [
        WorkStatus::Booked,
        WorkStatus::CheckedIn,
        WorkStatus::InProgress,
        WorkStatus::WaitingParts,
        WorkStatus::WaitingApproval,
        WorkStatus::Ready,
        WorkStatus::Completed,
        WorkStatus::Cancelled,
    ]
    .into_iter()
    .find(|s| key(*s) == text)
}

/// Whether a work order may go from `from` to `to` (moving a card on the
/// board, or a button on its page).
pub fn allowed(from: WorkStatus, to: WorkStatus) -> bool {
    use WorkStatus::*;
    matches!(
        (from, to),
        (Booked, CheckedIn | Cancelled)
            | (CheckedIn, Booked | InProgress | Cancelled)
            | (
                InProgress,
                CheckedIn | WaitingParts | WaitingApproval | Ready
            )
            | (WaitingParts | WaitingApproval, InProgress)
            | (Ready, InProgress | Completed)
    )
}

/// Labour and parts added up again from the order's tasks and parts.
pub async fn recompute(db: &Db, order: &mut WorkOrder) -> Result {
    let labour: i64 = WorkOrderTask::query()
        .where_(WorkOrderTask::WORK_ORDER_ID.eq(order.id))
        .sum(db, "price")
        .await?;
    let parts: i64 = WorkOrderPart::query()
        .where_(WorkOrderPart::WORK_ORDER_ID.eq(order.id))
        .sum(db, "total")
        .await?;
    order.labour = labour;
    order.parts = parts;
    order.total = labour + parts;
    order.save_only(db, &["labour", "parts", "total"]).await?;
    Ok(())
}

/// The customer of a work order on a customer's bike (none for the fleet).
pub async fn customer_of(db: &Db, order: &WorkOrder) -> Result<Option<Customer>> {
    let Some(bike_id) = order.customer_bike_id else {
        return Ok(None);
    };
    let Some(bike) = CustomerBike::find(db, bike_id).await? else {
        return Ok(None);
    };
    Customer::find(db, bike.customer_id).await
}

/// A work order was collected: closed for good. The intercompany books
/// (#245) listen for fleet repairs billed to another store.
#[derive(Debug, Clone)]
pub struct WorkOrderClosed {
    pub work_order_id: i64,
}

impl Event for WorkOrderClosed {}

/// Moves `order` to `to` (already checked with [`allowed`]): the dates it
/// implies, the customer told (mail + in-app), and for a collected fleet
/// repair the bike back in the fleet, serviced.
pub async fn set_status(state: &AppState, order: &mut WorkOrder, to: WorkStatus) -> Result {
    let now = renox::db::now();
    order.status = to;
    match to {
        WorkStatus::InProgress if order.started_at.is_none() => order.started_at = Some(now),
        WorkStatus::Completed => order.completed_at = Some(now),
        WorkStatus::Cancelled => order.cancelled_at = Some(now),
        _ => {}
    }
    order
        .save_only(
            &state.db,
            &["status", "started_at", "completed_at", "cancelled_at"],
        )
        .await?;
    if to == WorkStatus::Completed {
        if order.source == WorkSource::Fleet
            && let Some(bike_id) = order.rental_bike_id
            && let Some(mut bike) = RentalBike::find(&state.db, bike_id).await?
        {
            bike.status = BikeStatus::Available;
            bike.condition = BikeCondition::Good;
            bike.serviced_at_hours = bike.ridden_hours;
            bike.save(&state.db).await?;
        }
        state
            .emit(WorkOrderClosed {
                work_order_id: order.id,
            })
            .await?;
    }
    tell_customer(state, order).await
}

/// Tells the customer where their bike stands (nothing for the fleet).
pub async fn tell_customer(state: &AppState, order: &WorkOrder) -> Result {
    let Some(customer) = customer_of(&state.db, order).await? else {
        return Ok(());
    };
    let lang = state.current_lang();
    let label = lang.t(&format!("workshop.status.{}", key(order.status)), &[]);
    let url = crate::app::rentals::link(state, "workshop.service.show", Some(order.id))?;
    let notice = Notice::new(
        "workshop-status",
        "workshop.mail.status.title",
        match order.status {
            WorkStatus::Ready => "workshop.mail.status.ready",
            WorkStatus::WaitingParts => "workshop.mail.status.parts",
            _ => "workshop.mail.status.body",
        },
    )
    .param("number", order.id)
    .param("status", label)
    .row(
        "workshop.fields.total",
        crate::app::rentals::reserve::money(state, order.total),
    )
    .tone(if order.status == WorkStatus::Ready {
        Tone::Success
    } else {
        Tone::Info
    })
    .view("mail/workshop/notice")
    .url(url);
    notify::customer(state, &customer, Kind::Workshop, &notice).await
}
