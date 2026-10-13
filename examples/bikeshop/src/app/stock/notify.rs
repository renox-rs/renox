//! Telling a store's staff about stock: a mail and the bell.
//!
//! The messages are the rentals area's [`Notice`] (one `Notification` for
//! every message of the shop: title, sentence, rows of details, a link,
//! written in the recipient's language when sent) with the stock's own mail
//! view, `mail/stock/notice.html`. Who gets it is a **permission** in a
//! store (`purchasing.manage` at North), never a role's name:
//! [`crate::app::rentals::notify::staff_with_permission`] asks which roles
//! grant it.

use renox::prelude::*;

use crate::app::rentals::notify::{Notice, staff_with_permission};

/// Sends `notice` by mail and in the app to everyone holding `permission`
/// in store `store_id` now (the owner, through their global role, too).
pub async fn store_staff(
    state: &AppState,
    permission: &str,
    store_id: i64,
    notice: &Notice,
) -> Result<usize> {
    let people = staff_with_permission(&state.db, permission, store_id).await?;
    for user in &people {
        state.notify(user, notice).await?;
    }
    Ok(people.len())
}
