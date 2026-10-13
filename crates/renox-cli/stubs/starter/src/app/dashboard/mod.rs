//! The page people land on after logging in: a welcome card, a few figures
//! over a period (`?period=`, `renox::chart::Period`), new sign-ups as a
//! chart, a summary with totals, and what the signed-in person did lately
//! (the activity log). The template is `resources/views/dashboard/show.html`,
//! made of the app's page patterns (`resources/views/components/`).

use renox::audit;
use renox::chart::{Period, Trend};
use renox::prelude::*;

pub struct Dashboard;

impl Module for Dashboard {
    fn name(&self) -> &'static str {
        "dashboard"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/dashboard", show)
            .name("dashboard")
            .require_verified()
    }
}

async fn show(State(state): State<AppState>, user: AuthUser, period: Period) -> Result<View> {
    let signups = Trend::of(User::query(), "created_at")
        .over(period)
        .count(&state)
        .await?;
    let signups_before = Trend::of(User::query(), "created_at")
        .over(period.previous())
        .count(&state)
        .await?;
    let users = User::query().count(&state.db).await?;
    let verified = User::query()
        .where_not_null("email_verified_at")
        .count(&state.db)
        .await?;
    let unread = user.unread_notification_count(&state.db).await?;
    let activity = audit::for_user(&state.db, user.id, 8).await?;
    Ok(view(
        "dashboard/show.html",
        context! {
            period,
            users,
            verified,
            unread,
            activity,
            signups,
            signups_total => signups.total(),
            signups_change => signups.change_from(&signups_before),
            signups_before => signups_before.total(),
        },
    ))
}
