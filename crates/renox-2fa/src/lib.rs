//! Two-factor authentication for Renox apps: after the password, a code
//! from an authenticator app (TOTP, RFC 6238), or one of eight one-time
//! recovery codes.
//!
//! ```
//! use renox::prelude::*;
//! use renox_2fa::TwoFactor;
//!
//! # let _ =
//! App::new()
//!     .module(Auth::new().account()) // the account page, where users turn it on
//!     .module(TwoFactor::new())
//! # ;
//! ```
//!
//! Users turn it on from their account page: after their password, a QR
//! code to scan and a code to confirm; then their recovery codes, shown
//! once. From then on, logging in asks for a code after the password. The
//! guide is docs/two-factor.md in the Renox repository.

use renox::db::Migration;
use renox::prelude::*;
use serde::Serialize;

pub mod events;
mod handlers;
mod model;
pub mod qr;
pub mod recovery;
pub mod totp;

pub use events::{RecoveryCodeUsed, TwoFactorDisabled, TwoFactorEnabled};
pub use model::TwoFactorCredential;

const MIGRATIONS: &[Migration] = &[Migration::new(
    "00010101000700_create_two_factor_table",
    "",
    Some(include_str!(
        "../migrations/00010101000700_create_two_factor_table.down.sql"
    )),
)
.sqlite(
    include_str!("../migrations/00010101000700_create_two_factor_table.up.sql"),
    None,
)
.postgres(
    include_str!("../migrations/00010101000700_create_two_factor_table.postgres.up.sql"),
    None,
)];

/// The pages, compiled in. An app replaces one with a file of the same name
/// under its views directory (`resources/views/two-factor/setup.html`).
const VIEWS: &[(&str, &str)] = &[
    (
        "two-factor/challenge.html",
        include_str!("../views/challenge.html"),
    ),
    ("two-factor/setup.html", include_str!("../views/setup.html")),
    (
        "two-factor/recovery-codes.html",
        include_str!("../views/recovery-codes.html"),
    ),
    (
        "two-factor/section.html",
        include_str!("../views/section.html"),
    ),
];

/// The two-factor authentication module: add it next to `Auth` (with
/// `.account()`, where users turn it on).
#[derive(Debug, Clone, Default)]
pub struct TwoFactor {}

impl TwoFactor {
    /// The module with its defaults.
    pub fn new() -> Self {
        Self::default()
    }
}

/// What the account page's section shows.
#[derive(Serialize)]
struct Section {
    enabled: bool,
    since: Option<DateTime>,
    recovery_codes_left: usize,
}

impl Module for TwoFactor {
    fn name(&self) -> &'static str {
        "two-factor"
    }

    fn migrations(&self) -> &'static [Migration] {
        MIGRATIONS
    }

    fn routes(&self) -> Routes {
        handlers::routes()
    }

    fn register(&self, app: &mut Registry) {
        // Users with it on are asked for a code after their password.
        app.second_factor("two-factor.challenge", |user, state| async move {
            TwoFactorCredential::enabled(&state.db, user.id).await
        });
        app.account_section("two-factor/section.html", 100, |user, state| async move {
            let credential = TwoFactorCredential::of(&state.db, user.id).await?;
            let section = match credential.filter(TwoFactorCredential::is_confirmed) {
                Some(credential) => Section {
                    enabled: true,
                    since: credential.confirmed_at,
                    recovery_codes_left: credential.recovery_codes_left(),
                },
                None => Section {
                    enabled: false,
                    since: None,
                    recovery_codes_left: 0,
                },
            };
            Ok(renox::serde_json::to_value(section)?)
        });
        app.templates(|env| {
            for (name, source) in VIEWS {
                // The app's own file of that name wins.
                if env.get_template(name).is_err() {
                    renox::view::add_template(env, *name, *source)
                        .expect("a built-in template compiles");
                }
            }
        });
        // Recorded in the activity log when the app has the `Audit` module.
        app.listen(|e: TwoFactorEnabled, state| async move {
            audit(
                &state,
                renox::audit::Entry::new("two_factor.enabled").user(e.user_id),
            )
            .await
        })
        .listen(|e: TwoFactorDisabled, state| async move {
            audit(
                &state,
                renox::audit::Entry::new("two_factor.disabled").user(e.user_id),
            )
            .await
        })
        .listen(|e: RecoveryCodeUsed, state| async move {
            let entry = renox::audit::Entry::new("two_factor.recovery_code_used")
                .user(e.user_id)
                .data(renox::serde_json::json!({ "remaining": e.remaining }));
            audit(&state, entry).await
        });
    }
}

/// Records `entry` when the app has the `Audit` module (its table exists).
async fn audit(state: &AppState, entry: renox::audit::Entry) -> Result {
    let has_log = renox::db::sql("SELECT COUNT(*) FROM audit_logs WHERE 1 = 0")
        .scalar::<i64>(&state.db)
        .await
        .is_ok();
    if has_log {
        renox::audit::record(&state.db, entry).await?;
    }
    Ok(())
}

/// Compiles the Rust in docs/two-factor.md (the guide) as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/two-factor.md")]
pub struct Guide;
