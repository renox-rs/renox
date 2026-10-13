//! Two-factor login: optional for customers, **required for staff** (#239).
//!
//! `renox-2fa` (registered in `src/lib.rs`) asks for a code after the
//! password of anyone who turned it on. To make staff turn it on, the shop
//! enforces it **at login**:
//!
//! 1. [`on_logged_in`] (a `LoggedIn` listener) looks at who just logged in:
//!    someone who holds `staff.access` in any store (a permission, never a
//!    role's name) and has no confirmed two-factor login is noted in the
//!    cache (`bikeshop:2fa-required:{user}`). Turning it off later notes
//!    them again ([`on_disabled`]).
//! 2. [`middleware`] (an `App::layer` in `src/lib.rs`) runs on `/staff…`
//!    and `/admin…`: a noted person is sent to their account page, with a
//!    toast saying why, until they turn it on. Turning it on
//!    ([`on_enabled`]) lifts the note.
//!
//! Customers are never noted, so for them it stays a choice on the account
//! page.
//!
//! `BIKESHOP_STAFF_2FA=optional` turns the requirement off (a demo where
//! people try the staff side without an authenticator app, or a browser
//! test); anything else, or nothing, keeps it.

use renox::auth::events::LoggedIn;
use renox::axum::Extension;
use renox::axum::extract::Request;
use renox::axum::middleware::Next;
use renox::prelude::*;
use renox_2fa::{TwoFactorCredential, TwoFactorDisabled, TwoFactorEnabled};
use std::time::Duration;

use crate::app::access::catalogue::STAFF_ACCESS;

/// How long the note lasts (longer than any session).
const NOTE_FOR: Duration = Duration::from_secs(60 * 24 * 60 * 60);

/// The cache key noting that `user_id` must set up two-factor login.
pub fn note_key(user_id: i64) -> String {
    format!("bikeshop:2fa-required:{user_id}")
}

/// Whether `user_id` holds `staff.access` in any store now (a role given
/// there within its dates, or a global role). One query.
pub async fn is_staff(db: &Db, user_id: i64) -> Result<bool> {
    let now = renox::db::now();
    let count: i64 = renox::db::sql(
        "SELECT COUNT(*) FROM role_user ru \
         JOIN permission_role pr ON pr.role_id = ru.role_id \
         JOIN permissions p ON p.id = pr.permission_id \
         WHERE ru.user_id = ? AND p.name = ? \
         AND (ru.starts_at IS NULL OR ru.starts_at <= ?) \
         AND (ru.ends_at IS NULL OR ru.ends_at > ?)",
    )
    .bind(user_id)
    .bind(STAFF_ACCESS)
    .bind(now)
    .bind(now)
    .scalar(db)
    .await?;
    Ok(count > 0)
}

/// Whether staff must use two-factor login (`BIKESHOP_STAFF_2FA`, on by default).
pub fn required(config: &renox::Config) -> bool {
    !config
        .var("BIKESHOP_STAFF_2FA")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("optional"))
}

// [explain:two-factor.note]
/// Notes `user_id` when they are staff without two-factor login.
async fn note_if_needed(state: &AppState, user_id: i64) -> Result {
    if required(&state.config)
        && is_staff(&state.db, user_id).await?
        && !TwoFactorCredential::enabled(&state.db, user_id).await?
    {
        state
            .cache
            .put(&note_key(user_id), &true, Some(NOTE_FOR))
            .await?;
    }
    Ok(())
}

/// `LoggedIn`: notes a member of staff who logged in without two-factor login.
pub async fn on_logged_in(event: LoggedIn, state: AppState) -> Result {
    note_if_needed(&state, event.user_id).await
}

/// `TwoFactorDisabled`: a member of staff turning it off must turn it on again.
pub async fn on_disabled(event: TwoFactorDisabled, state: AppState) -> Result {
    note_if_needed(&state, event.user_id).await
}

/// `TwoFactorEnabled`: lifts the note.
pub async fn on_enabled(event: TwoFactorEnabled, state: AppState) -> Result {
    state.cache.forget(&note_key(event.user_id)).await
}
// [/explain:two-factor.note]

// [explain:two-factor.guard]
/// Sends a noted member of staff from the back office (`/staff…`,
/// `/admin…`) to their account page until two-factor login is on.
pub async fn middleware(
    user: Option<AuthUser>,
    Extension(state): Extension<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let back_office = ["/staff", "/admin"]
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")));
    if let (true, Some(user)) = (back_office, user) {
        let key = note_key(user.id);
        if state.cache.get::<bool>(&key).await.ok().flatten().is_some() {
            if TwoFactorCredential::enabled(&state.db, user.id)
                .await
                .unwrap_or(false)
            {
                let _ = state.cache.forget(&key).await;
            } else {
                let to = state
                    .url("account.show", &[])
                    .unwrap_or_else(|_| "/account".into());
                let message = state.current_lang().t("staff.two_factor.required", &[]);
                return (Toast::warning(message), Redirect::to(&to)).into_response();
            }
        }
    }
    next.run(req).await
}
// [/explain:two-factor.guard]
