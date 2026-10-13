//! The seeded people on the login page (`resources/views/renox/auth/login_options.html`),
//! so trying the app locally is one tap: shared with every view as
//! `demo_logins` (`src/lib.rs`).
//!
//! It is a list only on `/login`, with `APP_ENV=local`, and once `db:seed`
//! made the people (`super::seed::PEOPLE`); anywhere else it is `none`, so
//! a server in production never shows it, whatever its database holds.

use renox::Environment;
use renox::prelude::*;
use serde::Serialize;

use super::seed::{PASSWORD, PEOPLE};

/// One person to log in as.
#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub name: &'static str,
    pub email: &'static str,
}

/// What the login page shows: the people and their password.
#[derive(Debug, Clone, Serialize)]
pub struct Demo {
    pub password: &'static str,
    pub accounts: Vec<Account>,
}

/// The `demo_logins` view value.
pub async fn for_view(ctx: renox::view::ViewContext) -> Result<Option<Demo>> {
    if ctx.path != "/login" || ctx.state.config.env != Environment::Local {
        return Ok(None);
    }
    let mut accounts = Vec::new();
    for &(name, email, _) in PEOPLE {
        if User::find_by_email(&ctx.state.db, email).await?.is_some() {
            accounts.push(Account { name, email });
        }
    }
    Ok((!accounts.is_empty()).then_some(Demo {
        password: PASSWORD,
        accounts,
    }))
}
