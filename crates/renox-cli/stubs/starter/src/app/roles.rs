//! The roles and what each may do. Routes ask for a permission
//! (`require_permission`), templates with `can('users.manage')`; add a
//! permission here, give it to roles, and `define` makes the database agree
//! (it runs on every sign-up, seed and `users:admin`).

use renox::anyhow::anyhow;
use renox::auth::permissions;
use renox::command::Args;
use renox::prelude::*;

/// Manages users and their roles.
pub const USERS: &str = "users.manage";
/// Reads the activity log.
pub const ACTIVITY: &str = "activity.view";

/// Everything.
pub const ADMIN: &str = "admin";
/// Everyone who signs up.
pub const MEMBER: &str = "member";

/// Each role and its permissions.
pub const ROLES: &[(&str, &[&str])] = &[(ADMIN, &[USERS, ACTIVITY]), (MEMBER, &[])];

/// Creates the roles, or brings their permissions up to date.
pub async fn define(db: &Db) -> Result {
    for (role, grants) in ROLES {
        permissions::define_role(db, role, grants).await?;
    }
    Ok(())
}

/// `users:admin <email>`: the first admin of a new server, after they
/// signed up.
pub async fn make_admin(args: Args, state: AppState) -> Result {
    let Some(email) = args.positional().first().copied() else {
        return Err(anyhow!("usage: users:admin <email>").into());
    };
    let user = User::find_by_email(&state.db, email)
        .await?
        .ok_or_else(|| anyhow!("no user with the email {email}"))?;
    define(&state.db).await?;
    user.assign_role(&state.db, ADMIN).await?;
    println!("{} is an admin.", user.email);
    Ok(())
}
