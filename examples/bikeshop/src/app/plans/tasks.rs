//! The plans' scheduled task, registered from the module's `register`:
//! `plans:visits`, daily at 06:00 (`APP_TIMEZONE`), before the shops open.
//!
//! 1. Plans cancelled at their period's end whose last day has passed end
//!    ([`visits::end_finished`]).
//! 2. Visits whose day passed without the bike are recorded as missed
//!    ([`visits::mark_missed`]): not rolled over.
//! 3. Every running plan gets its visits for the next week
//!    ([`visits::plan_ahead`]): a plan change whose day has come applies
//!    first; paused plans, plans on hold and pending ones make none.
//!
//! The reminder the day before is the workshop's own `workshop:reminders`
//! (18:00): a plan's visit is a booked work order like any other.
//!
//! The task is idempotent: run twice, it makes nothing twice (each visit
//! has its number, unique per plan). Several servers sharing a database
//! run it once (Renox claims each run, docs/scheduling.md).

use renox::prelude::*;
use renox::schedule::Schedule;

use super::model::{PlanSubscription, SubscriptionStatus};
use super::visits;

/// Registers the task.
pub fn schedule(s: &mut Schedule) {
    s.daily_at("06:00", "plans:visits", |state: AppState| async move {
        run(&state).await.map(|_| ())
    });
}

/// What one run did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Run {
    pub ended: u64,
    pub missed: u64,
    pub made: u64,
}

/// One run of `plans:visits` (see the module docs).
pub async fn run(state: &AppState) -> Result<Run> {
    let db = &state.db;
    let today = visits::today(&state.config);
    let ended = visits::end_finished(db, today).await?;
    let missed = visits::mark_missed(state).await?;
    let subs = PlanSubscription::query()
        .where_(PlanSubscription::STATUS.eq(SubscriptionStatus::Active))
        .where_(PlanSubscription::PAUSED_AT.is_null())
        .where_(PlanSubscription::HELD_AT.is_null())
        .order_by(PlanSubscription::ID)
        .get(db)
        .await?;
    let mut made = 0;
    for mut sub in subs {
        made += visits::plan_ahead(state, &mut sub).await?.len() as u64;
    }
    Ok(Run {
        ended,
        missed,
        made,
    })
}
