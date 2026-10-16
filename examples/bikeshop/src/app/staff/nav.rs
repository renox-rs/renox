//! The staff sidebar's links (`<rx-sidebar menu="staff"/>` in
//! `resources/views/layouts/staff.html`).
//!
//! Each link names its route, a translation key for the label, the group
//! heading and the permission that shows it. The sidebar draws only the
//! links the person may open (a permission held in at least one store is
//! enough; the page then filters by the store being worked in), groups them
//! by the lowest `order` of their links and marks the current page. A new
//! area adds its line to [`LINKS`], behind the permission it needs.

use renox::nav::NavLink;
use renox::prelude::*;

/// The name of the menu the staff layout draws.
pub const MENU: &str = "staff";

const FRONT: &str = "staff.nav.sections.front";
const WORK: &str = "staff.nav.sections.work";
const STORES: &str = "staff.nav.sections.stores";
const MANAGE: &str = "staff.nav.sections.manage";

/// One row per link: group, order, route, label key, icon, permission and
/// the route pattern that marks it active.
const LINKS: &[(&str, i32, &str, &str, &str, &str, &str)] = &[
    // Front desk
    (
        FRONT,
        10,
        "sales.orders.index",
        "sales.staff.title",
        "receipt",
        "orders.view",
        "sales.orders.*",
    ),
    (
        FRONT,
        11,
        "sales.counter",
        "sales.counter.title",
        "scan-barcode",
        "orders.sell",
        "sales.counter*",
    ),
    (
        FRONT,
        12,
        "rentals.counter",
        "rentals.nav.counter",
        "key-round",
        "rentals.view",
        "rentals.counter*",
    ),
    (
        FRONT,
        13,
        "rentals.fleet",
        "rentals.nav.fleet",
        "bike",
        "fleet.view",
        "rentals.fleet*",
    ),
    (
        FRONT,
        14,
        "rentals.identities",
        "rentals.nav.identities",
        "id-card",
        "rentals.verify_id",
        "rentals.identities",
    ),
    // Workshop and stock
    (
        WORK,
        20,
        "workshop.board",
        "workshop.nav.board",
        "wrench",
        "workorders.view",
        "workshop.*",
    ),
    (
        WORK,
        21,
        "stock.index",
        "stock.index.title",
        "boxes",
        "stock.view",
        "stock.index*",
    ),
    (
        WORK,
        22,
        "stock.consignments",
        "stock.consignments.title",
        "truck",
        "stock.view",
        "stock.consignments*",
    ),
    (
        WORK,
        23,
        "stock.purchasing",
        "stock.purchasing.title",
        "clipboard-list",
        "purchasing.manage",
        "stock.purchasing*",
    ),
    // Between stores
    (
        STORES,
        30,
        "multistore.help",
        "multistore.help.title",
        "handshake",
        "staff.help",
        "multistore.help*",
    ),
    (
        STORES,
        31,
        "multistore.placements",
        "multistore.placements.title",
        "map-pin",
        "fleet.view",
        "multistore.placements*",
    ),
    (
        STORES,
        32,
        "multistore.books",
        "multistore.books.title",
        "book-open",
        "intercompany.view",
        "multistore.books*",
    ),
    // Manage
    (
        MANAGE,
        40,
        "reports.dashboard",
        "reports.nav",
        "chart-column",
        "reports.view",
        "reports.*",
    ),
    (
        MANAGE,
        41,
        "staff.team.index",
        "staff.nav.team",
        "users",
        "staff.manage",
        "staff.team.*",
    ),
    (
        MANAGE,
        42,
        "staff.stores.index",
        "staff.nav.stores",
        "store",
        "stores.manage",
        "staff.stores.*",
    ),
    (
        MANAGE,
        43,
        "staff.roles.index",
        "staff.nav.roles",
        "shield-check",
        "roles.manage",
        "staff.roles.*",
    ),
    (
        MANAGE,
        44,
        "staff.audit.index",
        "staff.nav.audit",
        "history",
        "audit.view",
        "staff.audit.*",
    ),
    (
        MANAGE,
        46,
        "api.kiosks",
        "api.kiosks.nav",
        "tablet-smartphone",
        "fleet.manage",
        "api.kiosks",
    ),
];

/// Adds the staff links to the `staff` menu.
pub fn register(app: &mut Registry) {
    // The dashboard comes first, outside any group.
    app.nav(
        MENU,
        NavLink::route("staff.dashboard", "nav.dashboard")
            .icon("layout-dashboard")
            .active("staff.dashboard"),
    );
    for &(group, order, route, label, icon, permission, active) in LINKS {
        app.nav(
            MENU,
            NavLink::route(route, label)
                .icon(icon)
                .group(group)
                .order(order)
                .active(active)
                .permission(permission),
        );
    }
    // The admin pages open for any of four permissions.
    app.nav(
        MENU,
        NavLink::route("admin.dashboard", "staff.nav.admin")
            .icon("settings")
            .group(MANAGE)
            .order(45)
            .active("admin.*")
            .when(|user| {
                [
                    "catalog.manage",
                    "plans.manage",
                    "purchasing.manage",
                    "stores.manage",
                ]
                .iter()
                .any(|permission| user.has_permission_anywhere(permission))
            }),
    );
}
