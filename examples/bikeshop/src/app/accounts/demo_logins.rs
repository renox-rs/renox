//! The demo accounts on the login page: who to log in as, and the password,
//! so a visitor of the demo can try every side of the shop.
//!
//! [`for_view`] is shared with every view as `demo_logins` (`src/lib.rs`)
//! and the login page shows it (`resources/views/renox/auth/login_options.html`).
//! It is a list only on `/login`, when the seeders' users exist (a
//! database seeded with `db:seed` or `demo:seed`, where every user has the
//! password [`DEMO_PASSWORD`]) and `BIKESHOP_DEMO_LOGINS` isn't `false`;
//! otherwise it is empty, so a shop with real users never shows it.

use renox::db::sql;
use renox::prelude::*;
use serde::Serialize;

use crate::seed::DEMO_PASSWORD;

/// One account to try: its email and who they are.
#[derive(Debug, Clone, Serialize)]
pub struct DemoLogin {
    /// The email to log in with.
    pub email: &'static str,
    /// Who this is: a key of `accounts.demo.who.*` (not a role's name).
    pub who: &'static str,
}

/// The accounts listed, one per side of the shop (the seeders make them).
pub const LOGINS: &[DemoLogin] = &[
    DemoLogin {
        email: "customer@bikeshop.test",
        who: "shopper",
    },
    DemoLogin {
        email: "owner@bikeshop.test",
        who: "shop_owner",
    },
    DemoLogin {
        email: "manager.north@bikeshop.test",
        who: "store_manager",
    },
    DemoLogin {
        email: "cashier.north@bikeshop.test",
        who: "till",
    },
    DemoLogin {
        email: "mechanic.north@bikeshop.test",
        who: "workshop",
    },
    DemoLogin {
        email: "floater@bikeshop.test",
        who: "two_stores",
    },
];

/// Whether the list may be shown: `BIKESHOP_DEMO_LOGINS` unset, or anything
/// but `false`, `0`, `off` or `no`.
pub fn enabled(config: &renox::Config) -> bool {
    !matches!(
        config
            .var("BIKESHOP_DEMO_LOGINS")
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Some("false" | "0" | "off" | "no")
    )
}

/// What the login page shows.
#[derive(Debug, Clone, Serialize)]
pub struct Demo {
    /// Every demo user's password.
    pub password: &'static str,
    /// Whether staff are asked to turn on two-factor login first.
    pub staff_2fa: bool,
    /// The accounts.
    pub accounts: &'static [DemoLogin],
}

// [explain:login.demo]
/// The `demo_logins` view value: the [`Demo`] on the login page of a
/// seeded demo, else `none`.
pub async fn for_view(ctx: renox::view::ViewContext) -> Result<Option<Demo>> {
    if ctx.path != "/login" || !enabled(&ctx.state.config) {
        return Ok(None);
    }
    let seeded = sql("SELECT COUNT(*) FROM users WHERE email = ?")
        .bind(LOGINS[1].email)
        .scalar::<i64>(&ctx.state.db)
        .await?
        > 0;
    Ok(seeded.then(|| Demo {
        password: DEMO_PASSWORD,
        staff_2fa: crate::app::staff::two_factor::required(&ctx.state.config),
        accounts: LOGINS,
    }))
}
// [/explain:login.demo]
