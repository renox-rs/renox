//! The workshop's scheduled task, registered from the module's `register`:
//! `workshop:reminders`, daily at 18:00 (`APP_TIMEZONE`): every booking
//! for tomorrow gets its reminder, once (a mail and an in-app
//! notification). A rescheduled booking is reminded again for its new day.

use renox::chrono::Duration;
use renox::prelude::*;
use renox::schedule::Schedule;

use super::capacity::day_bounds;
use super::model::{WorkOrder, WorkSource, WorkStatus};
use super::status::customer_of;
use crate::app::accounts::preferences::Kind;
use crate::app::rentals::booking::to_local;
use crate::app::rentals::notify::{self, Notice};

/// Registers the task.
pub fn schedule(s: &mut Schedule) {
    s.daily_at(
        "18:00",
        "workshop:reminders",
        |state: AppState| async move { reminders(&state).await.map(|_| ()) },
    );
}

/// Reminds the customers of tomorrow's bookings; how many were reminded.
pub async fn reminders(state: &AppState) -> Result<u64> {
    let tomorrow = to_local(&state.config, renox::db::now()).date() + Duration::days(1);
    let (from, to) = day_bounds(&state.config, tomorrow);
    let orders = WorkOrder::query()
        .where_(WorkOrder::STATUS.eq(WorkStatus::Booked))
        .where_(WorkOrder::SOURCE.ne(WorkSource::Fleet))
        .where_(WorkOrder::REMINDED_AT.is_null())
        .where_(WorkOrder::SCHEDULED_FOR.gte(from))
        .where_(WorkOrder::SCHEDULED_FOR.lt(to))
        .get(&state.db)
        .await?;
    for mut order in orders.iter().cloned() {
        order.reminded_at = Some(renox::db::now());
        order.save_only(&state.db, &["reminded_at"]).await?;
        if let Some(customer) = customer_of(&state.db, &order).await? {
            let url = crate::app::rentals::link(state, "workshop.service.show", Some(order.id))?;
            notify::customer(
                state,
                &customer,
                Kind::Workshop,
                &Notice::new(
                    "workshop-reminder",
                    "workshop.mail.reminder.title",
                    "workshop.mail.reminder.body",
                )
                .param("number", order.id)
                .param(
                    "time",
                    to_local(&state.config, order.scheduled_for).format("%H:%M"),
                )
                .view("mail/workshop/notice")
                .url(url),
            )
            .await?;
        }
    }
    Ok(orders.len() as u64)
}
