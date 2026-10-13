//! Repairs of the rental fleet: when the rentals area reports a bike that
//! needs the workshop (damaged at a return, or due for its service by
//! ridden hours, `rentals::FleetRepairNeeded`), a work order is opened at
//! the workshop of the store where the bike stands, labelled "fleet" on
//! the board. When the bike belongs to another store, the work order is
//! **billed to its owner store** (`billed_store_id`): the intercompany
//! books (#245) charge the owner for the repair.

use renox::prelude::*;

use super::model::{WorkOrder, WorkSource, WorkStatus};
use crate::app::rentals::FleetRepairNeeded;
use crate::app::rentals::model::RentalBike;

/// The statuses of a work order that isn't finished yet.
pub const OPEN: [WorkStatus; 6] = [
    WorkStatus::Booked,
    WorkStatus::CheckedIn,
    WorkStatus::InProgress,
    WorkStatus::WaitingParts,
    WorkStatus::WaitingApproval,
    WorkStatus::Ready,
];

/// Opens the fleet work order for `event`, unless the bike already has an
/// open one (then the note is added to it).
pub async fn open_repair(state: &AppState, event: &FleetRepairNeeded) -> Result<WorkOrder> {
    let bike = RentalBike::find_or_404(&state.db, event.bike_id).await?;
    if let Some(mut open) = WorkOrder::query()
        .where_(WorkOrder::RENTAL_BIKE_ID.eq(bike.id))
        .where_(WorkOrder::STATUS.is_in(OPEN))
        .first(&state.db)
        .await?
    {
        let note = match &open.customer_note {
            Some(before) => format!("{before}\n{}", event.note),
            None => event.note.clone(),
        };
        open.customer_note = Some(note);
        open.save_only(&state.db, &["customer_note"]).await?;
        return Ok(open);
    }
    WorkOrder::create(
        &state.db,
        WorkOrder {
            rental_bike_id: Some(bike.id),
            store_id: bike.location_store_id,
            source: WorkSource::Fleet,
            billed_store_id: (bike.owner_store_id != bike.location_store_id)
                .then_some(bike.owner_store_id),
            scheduled_for: renox::db::now(),
            status: WorkStatus::CheckedIn,
            customer_note: Some(event.note.clone()),
            ..Default::default()
        },
    )
    .await
}
