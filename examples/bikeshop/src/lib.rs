//! Example: a bike shop with three stores, Renox's flagship example (#231).
//! Customers buy bikes and gear, rent a bike by the hour or the day, and
//! have their bikes serviced, once or on a plan; the three stores share
//! staff, bikes and goods, and settle with each other monthly.
//!
//! Every page explains itself: an "About this page" panel says what the
//! page is for, who uses it, which Renox features it uses and why, and
//! links to the guide and the source files (`src/explain.rs`). `/about/pages`
//! lists every page by feature and by role.
//!
//! How it was made: `rnx new bikeshop` wrote the app (lib + bin, the
//! `home` module, the layout, `.env.example`), then it moved into the
//! workspace like the other examples (`renox.workspace = true` in
//! `Cargo.toml`). The areas in `src/app/` name the `rnx make:*` commands
//! that made their parts.
//!
//! Run it from this directory:
//!
//! ```text
//! cargo run -- migrate
//! cargo run          # http://127.0.0.1:3000
//! ```

pub mod app;
pub mod code;
pub mod explain;
pub mod highlight;
pub mod money;
pub mod report;
pub mod seed;
pub mod text;

use renox::prelude::*;

/// The application: its modules, migrations and seeders. `main.rs` runs it,
/// and tests boot it with `renox::testing::TestApp`.
pub fn app() -> App {
    App::new()
        .embed(renox::embedded!())
        .migrations(renox::migrations!())
        // A visitor's browser language wins when the shop has it (en, es),
        // until they pick one in the language menu.
        .detect_locale()
        // The "About this page" panel: `about_page(request.route, request.path)` in the layouts,
        // hidden with BIKESHOP_EXPLAIN=false.
        .templates(explain::register)
        // `tx(text)` and `tk(key, fallback)`: texts that come from Rust in English.
        .templates(text::register)
        .share(
            "explain_panels",
            |ctx: renox::view::ViewContext| async move { Ok(explain::enabled(&ctx.state.config)) },
        )
        // The demo accounts and their password on the login page of a seeded
        // demo, hidden with BIKESHOP_DEMO_LOGINS=false (src/app/accounts/demo_logins.rs).
        .share("demo_logins", app::accounts::demo_logins::for_view)
        // Amounts are stored in cents but typed in dollars (src/money.rs):
        // a number input's `step` (`0.01`), the scale to divide a stored
        // amount by to fill one (100), and the symbol before it (`$`).
        .share("money_step", |ctx: renox::view::ViewContext| async move {
            Ok(money::step(&ctx.state.config.currency))
        })
        .share("money_scale", |ctx: renox::view::ViewContext| async move {
            Ok(money::scale(&ctx.state.config.currency))
        })
        .share("money_symbol", |ctx: renox::view::ViewContext| async move {
            Ok(money::symbol(&ctx.state.config.currency))
        })
        // Login, register, password reset, email verification, /account with
        // the areas' sections, and the notification bell's list and stream;
        // signing up makes a customer (src/app/accounts).
        .module(
            renox::auth::Auth::new()
                .account()
                .verify_email()
                .notifications()
                .on_registered(app::accounts::registration::on_registered),
        )
        // Roles given per store, with dates (#244); the catalogue is src/app/access.
        .module(renox::auth::permissions::Permissions)
        // `db:seed`: a small shop in seconds (`demo:seed --size large` for Pagila's volume).
        .seeder(seed::run)
        .typed_command::<seed::DemoSeed>()
        // Service plans billed as subscriptions (#237): renox-billing's
        // plans, gateways (Stripe, Xendit, the demo) and webhooks.
        .module(app::plans::billing())
        // The JSON API's limit per token (#241): `.throttle_by("bikeshop-api")`.
        .rate_limiter(app::api::LIMITER, app::api::limit)
        // --- Areas (alphabetical; add new ones in order) ---
        .module(app::about::About)
        .module(app::access::Access)
        .module(app::accounts::Accounts)
        .module(app::api::Api)
        .module(app::catalog::Catalog)
        .module(app::home::Home)
        .module(app::multistore::Multistore)
        .module(app::plans::Plans)
        .module(app::rentals::Rentals)
        .module(app::reports::Reports)
        .module(app::sales::Sales)
        .module(app::staff::Staff)
        .module(app::stock::Stock)
        .module(app::workshop::Workshop)
        // --- end of areas ---
        // --- Plugins and app-wide layers (append) ---
        // "Continue with Google / GitHub" when their keys are set (#238).
        .module(renox_oauth::OAuth::new().google().github())
        // Two-factor login: optional for customers, required for staff (#238, #239).
        .module(renox_2fa::TwoFactor::new())
        // The audit log: logins, deleted accounts, claimed records, the staff
        // side's sensitive changes (src/app/staff/audit.rs) and the books'
        // (src/app/multistore/audit.rs).
        .module(renox::audit::Audit)
        // Whether "About this page" is docked or folded, from its cookie (src/explain.rs).
        .layer(renox::axum::middleware::from_fn(explain::dock_layer))
        // The language follows the account to every device (src/app/accounts/locale.rs).
        .layer(renox::axum::middleware::from_fn(
            app::accounts::locale::middleware,
        ))
        // The admin panel for the catalogue, workshop, suppliers and stores
        // (renox-admin, authorized by permission: src/app/staff/admin.rs), and
        // the editors its forms use (renox-editors).
        .module(app::staff::admin::panel())
        .module(renox_editors::Editors::new())
        // The blocks the kit doesn't have (renox-blocks, #347): the product
        // gallery and variant chips, the cart's stepper, the counter's keypad,
        // the workshop board, the rental timeline, the plans' table.
        .module(renox_blocks::Blocks::new())
        // The active store for the panel's pages too (src/app/staff/admin.rs).
        .layer(renox::axum::middleware::from_fn(app::staff::admin::layer))
        // Staff without two-factor login are sent to set it up (src/app/staff/two_factor.rs).
        .layer(renox::axum::middleware::from_fn(
            app::staff::two_factor::middleware,
        ))
        // Every 500, job failed for good and failed scheduled task: logged and
        // kept in storage/logs/errors.log (src/report.rs).
        .report(report::log)
}
