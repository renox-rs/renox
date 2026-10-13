//! Demo data for a local database: `cargo run -- db:seed` (or
//! `migrate:fresh --seed`). Seeding twice changes nothing. In local, the
//! login page lists these people to log in with a tap (`super::demo`).

use renox::prelude::*;

use super::roles::{self, ADMIN, MEMBER};

/// Every seeded person's password.
pub const PASSWORD: &str = "password123";

/// The people `db:seed` makes: name, email and role.
pub const PEOPLE: &[(&str, &str, &str)] = &[
    ("Admin", "admin@example.com", ADMIN),
    ("Member", "member@example.com", MEMBER),
];

pub async fn run(state: AppState) -> Result {
    let db = &state.db;
    roles::define(db).await?;
    for &(name, email, role) in PEOPLE {
        if User::find_by_email(db, email).await?.is_some() {
            continue;
        }
        let mut user = User::register(db, name, email, PASSWORD).await?;
        // Seeded people need no verification mail.
        user.email_verified_at = Some(renox::db::now());
        user.save(db).await?;
        user.assign_role(db, role).await?;
    }
    Ok(())
}
