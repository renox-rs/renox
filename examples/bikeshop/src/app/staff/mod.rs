//! The staff side (#239): the back office's home, the stores, the team and
//! its invitations, the role × permission matrix, the audit log, two-factor
//! login for staff, and the admin panel for the catalogue.
//!
//! Every page goes through `access::staff_routes` (a login, then
//! `staff.access` in the active store, `src/app/access`) and asks for its
//! own permission on top: `stores.manage`, `staff.manage`, `roles.manage`,
//! `audit.view`, `catalog.manage`. The admin panel (`renox-admin`) is
//! [`admin::panel`], registered in `src/lib.rs`.
//!
//! - [`stores`]: the stores' details, opening hours (a repeater) and fee rate;
//! - [`team`]: who works in the store, invitations, roles per store with
//!   dates, deactivation;
//! - [`roles`]: the role × permission matrix, edited live;
//! - [`audit`]: the audit log page and `audit::record`, which every area
//!   uses for its sensitive actions;
//! - [`two_factor`]: two-factor login required for staff;
//! - [`admin`] and [`catalog_tools`]: the panel and its two extra pages.

pub mod admin;
pub mod audit;
pub mod catalog_tools;
pub mod explain;
pub mod factories;
pub mod model;
pub mod roles;
pub mod stores;
pub mod team;
pub mod two_factor;

use renox::prelude::*;

/// The staff area.
pub struct Staff;

impl Module for Staff {
    fn name(&self) -> &'static str {
        "staff"
    }

    // [explain:staff.dashboard.handler]
    fn routes(&self) -> Routes {
        // Logged in, and working in a store where a role grants
        // `staff.access` today (the active store, src/app/access).
        crate::app::access::staff_routes(
            Routes::new()
                .get("/staff", dashboard)
                .name("staff.dashboard"),
        )
        .merge(stores::routes())
        .merge(team::routes())
        .merge(roles::routes())
        .merge(audit::routes())
        .merge(catalog_tools::routes())
    }
    // [/explain:staff.dashboard.handler]

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<crate::app::staff::model::Store>()
            .model::<crate::app::staff::model::Staff>()
            .model::<crate::app::staff::model::StaffHelpRequest>()
            .model::<crate::app::staff::model::StaffHelpHour>();
        // Two-factor login is required for staff (two_factor.rs).
        app.listen(two_factor::on_logged_in)
            .listen(two_factor::on_enabled)
            .listen(two_factor::on_disabled);
    }
}

// [explain:staff.dashboard.handler]
/// `GET /staff` (`staff.dashboard`): for people who see reports in the
/// active store, its last 7 days (`reports::dashboard::overview`, #242).
async fn dashboard(State(state): State<AppState>, user: AuthUser) -> Result<View> {
    let overview = crate::app::reports::dashboard::overview(&state, &user).await?;
    Ok(view("staff/dashboard.html", context! { overview }))
}
// [/explain:staff.dashboard.handler]
