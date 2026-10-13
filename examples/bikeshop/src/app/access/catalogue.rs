//! The permission catalogue and the roles that bundle them.
//!
//! **Code checks permissions, never role names.** Every check in the app
//! names one of the constants below (`access::catalogue::RENTALS_CHECKOUT`),
//! through `Routes::require_permission`, `can('…')` in templates, or the
//! policy helpers in [`super::policy`]. A role is only a named set of
//! permissions: the owner can change what a cashier may do without a
//! deploy, and no code changes. This file is the only place role names
//! appear; `tests/access.rs` fails when one shows up anywhere else in a
//! check (`has_role`, `require_role`, `auth.roles`, a `"manager"` literal).
//!
//! **Where:** roles are given *in a store* (`assign_role_in(…,
//! &Scope::of(&store))`, #244), optionally between two dates; only the
//! owner's role is global. So "may this person refund?" is always "…in
//! this store?": the active store for the request (see
//! [`super::active_store`]) or the store attribute of the record at hand
//! (see [`super::policy`]).

/// A permission: its name (what code checks) and what it lets you do.
#[derive(Debug, Clone, Copy)]
pub struct Permission {
    /// `rentals.checkout`.
    pub name: &'static str,
    /// What it allows, for the roles page and `/about/data`.
    pub description: &'static str,
}

/// A role: a named set of permissions.
#[derive(Debug, Clone, Copy)]
pub struct Role {
    /// The role's name in `roles` (`manager`).
    pub name: &'static str,
    /// Its English label.
    pub label: &'static str,
    /// What the role is for.
    pub description: &'static str,
    /// Whether it is given globally (every store) rather than in a store.
    pub global: bool,
    /// The permissions it grants.
    pub permissions: &'static [&'static str],
}

// --- Permissions (what code checks) ---

/// Work in a store at all: open the staff side, appear in its switcher.
/// Every staff role grants it.
pub const STAFF_ACCESS: &str = "staff.access";
/// Edit the company-wide catalogue: products, variants, categories, brands, what fits what.
pub const CATALOG_MANAGE: &str = "catalog.manage";
/// Change selling prices and rental rates (checked in the owner store).
pub const PRICES_CHANGE: &str = "prices.change";
/// See stock levels and the stock ledger.
pub const STOCK_VIEW: &str = "stock.view";
/// Correct stock by hand (a stock take, damage), with a reason.
pub const STOCK_ADJUST: &str = "stock.adjust";
/// Receive deliveries from suppliers and consignment shipments.
pub const STOCK_RECEIVE: &str = "stock.receive";
/// Send goods to another store on consignment, and recall them.
pub const CONSIGNMENT_MANAGE: &str = "consignment.manage";
/// Order from suppliers.
pub const PURCHASING_MANAGE: &str = "purchasing.manage";
/// See orders.
pub const ORDERS_VIEW: &str = "orders.view";
/// Sell at the counter and hand over online orders.
pub const ORDERS_SELL: &str = "orders.sell";
/// Refund an order.
pub const ORDERS_REFUND: &str = "orders.refund";
/// See rentals.
pub const RENTALS_VIEW: &str = "rentals.view";
/// Hand a rental bike over to a customer.
pub const RENTALS_CHECKOUT: &str = "rentals.checkout";
/// Take a rental bike back, charge late and damage fees.
pub const RENTALS_RETURN: &str = "rentals.return";
/// Check and approve a customer's ID document.
pub const RENTALS_VERIFY_ID: &str = "rentals.verify_id";
/// See the rental fleet.
pub const FLEET_VIEW: &str = "fleet.view";
/// Place a store's bikes at another store, and call them back.
pub const FLEET_PLACE: &str = "fleet.place";
/// Buy, retire or sell rental bikes (checked in the owner store).
pub const FLEET_MANAGE: &str = "fleet.manage";
/// See work orders.
pub const WORKORDERS_VIEW: &str = "workorders.view";
/// Work on a work order: tasks done, parts used, status.
pub const WORKORDERS_UPDATE: &str = "workorders.update";
/// Give a work order to a mechanic, plan the workshop's day.
pub const WORKORDERS_ASSIGN: &str = "workorders.assign";
/// Manage service plans and their subscriptions.
pub const PLANS_MANAGE: &str = "plans.manage";
/// See customers.
pub const CUSTOMERS_VIEW: &str = "customers.view";
/// Add and edit customers.
pub const CUSTOMERS_MANAGE: &str = "customers.manage";
/// Invite, edit and deactivate staff; give roles in the store.
pub const STAFF_MANAGE: &str = "staff.manage";
/// Ask another store for help, and approve lending staff to one.
pub const STAFF_HELP: &str = "staff.help";
/// Change what each role may do.
pub const ROLES_MANAGE: &str = "roles.manage";
/// Edit the stores themselves: address, hours, workshop time.
pub const STORES_MANAGE: &str = "stores.manage";
/// See reports and dashboards.
pub const REPORTS_VIEW: &str = "reports.view";
/// See the books between stores.
pub const INTERCOMPANY_VIEW: &str = "intercompany.view";
/// Mark a monthly settlement between stores as settled.
pub const INTERCOMPANY_SETTLE: &str = "intercompany.settle";
/// Change the fee rate stores earn for work done for each other.
pub const SETTINGS_FEES: &str = "settings.fees";
/// Read the audit log.
pub const AUDIT_VIEW: &str = "audit.view";

/// Every permission, with what it allows.
pub const PERMISSIONS: &[Permission] = &[
    p(
        STAFF_ACCESS,
        "Work in a store at all: open the staff side, pick the store.",
    ),
    p(
        CATALOG_MANAGE,
        "Edit the catalogue: products, variants, categories, brands, fits.",
    ),
    p(
        PRICES_CHANGE,
        "Change selling prices and rental rates (in the owner store).",
    ),
    p(STOCK_VIEW, "See stock levels and the stock ledger."),
    p(STOCK_ADJUST, "Correct stock by hand, with a reason."),
    p(
        STOCK_RECEIVE,
        "Receive deliveries and consignment shipments.",
    ),
    p(
        CONSIGNMENT_MANAGE,
        "Send goods to another store on consignment, recall them.",
    ),
    p(PURCHASING_MANAGE, "Order from suppliers."),
    p(ORDERS_VIEW, "See orders."),
    p(ORDERS_SELL, "Sell at the counter, hand over online orders."),
    p(ORDERS_REFUND, "Refund an order."),
    p(RENTALS_VIEW, "See rentals."),
    p(RENTALS_CHECKOUT, "Hand a rental bike over."),
    p(
        RENTALS_RETURN,
        "Take a rental bike back, charge late and damage fees.",
    ),
    p(
        RENTALS_VERIFY_ID,
        "Check and approve a customer's ID document.",
    ),
    p(FLEET_VIEW, "See the rental fleet."),
    p(FLEET_PLACE, "Place bikes at another store, call them back."),
    p(
        FLEET_MANAGE,
        "Buy, retire or sell rental bikes (in the owner store).",
    ),
    p(WORKORDERS_VIEW, "See work orders."),
    p(WORKORDERS_UPDATE, "Work on a work order."),
    p(WORKORDERS_ASSIGN, "Give work orders to mechanics."),
    p(PLANS_MANAGE, "Manage service plans and subscriptions."),
    p(CUSTOMERS_VIEW, "See customers."),
    p(CUSTOMERS_MANAGE, "Add and edit customers."),
    p(
        STAFF_MANAGE,
        "Invite and deactivate staff, give roles in the store.",
    ),
    p(STAFF_HELP, "Ask another store for help, lend staff to one."),
    p(ROLES_MANAGE, "Change what each role may do."),
    p(
        STORES_MANAGE,
        "Edit the stores: address, hours, workshop time.",
    ),
    p(REPORTS_VIEW, "See reports and dashboards."),
    p(INTERCOMPANY_VIEW, "See the books between stores."),
    p(INTERCOMPANY_SETTLE, "Mark a monthly settlement as settled."),
    p(
        SETTINGS_FEES,
        "Change the fee rate stores earn from each other.",
    ),
    p(AUDIT_VIEW, "Read the audit log."),
];

const fn p(name: &'static str, description: &'static str) -> Permission {
    Permission { name, description }
}

// --- Roles (named sets of permissions; only this file names them) ---

/// The owner: every permission, in every store (a global role).
pub const OWNER: &str = "owner";
/// Runs one store.
pub const MANAGER: &str = "manager";
/// Sells and rents at a store's counter.
pub const CASHIER: &str = "cashier";
/// Works in a store's workshop.
pub const MECHANIC: &str = "mechanic";
/// Helps on the shop floor (also the role a helper from another store
/// usually gets).
pub const STAFF: &str = "staff";

const MANAGER_PERMISSIONS: &[&str] = &[
    STAFF_ACCESS,
    PRICES_CHANGE,
    STOCK_VIEW,
    STOCK_ADJUST,
    STOCK_RECEIVE,
    CONSIGNMENT_MANAGE,
    PURCHASING_MANAGE,
    ORDERS_VIEW,
    ORDERS_SELL,
    ORDERS_REFUND,
    RENTALS_VIEW,
    RENTALS_CHECKOUT,
    RENTALS_RETURN,
    RENTALS_VERIFY_ID,
    FLEET_VIEW,
    FLEET_PLACE,
    FLEET_MANAGE,
    WORKORDERS_VIEW,
    WORKORDERS_UPDATE,
    WORKORDERS_ASSIGN,
    PLANS_MANAGE,
    CUSTOMERS_VIEW,
    CUSTOMERS_MANAGE,
    STAFF_MANAGE,
    STAFF_HELP,
    REPORTS_VIEW,
    INTERCOMPANY_VIEW,
];

const CASHIER_PERMISSIONS: &[&str] = &[
    STAFF_ACCESS,
    STOCK_VIEW,
    ORDERS_VIEW,
    ORDERS_SELL,
    RENTALS_VIEW,
    RENTALS_CHECKOUT,
    RENTALS_RETURN,
    RENTALS_VERIFY_ID,
    FLEET_VIEW,
    WORKORDERS_VIEW,
    CUSTOMERS_VIEW,
    CUSTOMERS_MANAGE,
];

const MECHANIC_PERMISSIONS: &[&str] = &[
    STAFF_ACCESS,
    STOCK_VIEW,
    STOCK_RECEIVE,
    FLEET_VIEW,
    RENTALS_VIEW,
    WORKORDERS_VIEW,
    WORKORDERS_UPDATE,
    CUSTOMERS_VIEW,
];

const STAFF_PERMISSIONS: &[&str] = &[
    STAFF_ACCESS,
    STOCK_VIEW,
    ORDERS_VIEW,
    ORDERS_SELL,
    RENTALS_VIEW,
    RENTALS_CHECKOUT,
    RENTALS_RETURN,
    FLEET_VIEW,
    WORKORDERS_VIEW,
    CUSTOMERS_VIEW,
];

/// Every permission's name (the owner's role grants them all).
pub fn all_permission_names() -> Vec<&'static str> {
    PERMISSIONS.iter().map(|p| p.name).collect()
}

/// The roles, as the seeders define them. The owner may change their
/// permissions later on the roles page (#239): code never relies on what a
/// role grants, only on the permission it checks.
pub fn roles() -> Vec<Role> {
    vec![
        Role {
            name: OWNER,
            label: "Owner",
            description: "Runs all three stores: every permission, everywhere.",
            global: true,
            permissions: OWNER_PERMISSIONS,
        },
        Role {
            name: MANAGER,
            label: "Store manager",
            description: "Runs one store: its staff, stock, counter, fleet and workshop.",
            global: false,
            permissions: MANAGER_PERMISSIONS,
        },
        Role {
            name: CASHIER,
            label: "Cashier",
            description: "Sells and rents at the counter, checks customers' IDs.",
            global: false,
            permissions: CASHIER_PERMISSIONS,
        },
        Role {
            name: MECHANIC,
            label: "Mechanic",
            description: "Works on work orders in the store's workshop.",
            global: false,
            permissions: MECHANIC_PERMISSIONS,
        },
        Role {
            name: STAFF,
            label: "Staff",
            description: "Helps on the floor: sells, hands bikes over and takes them back.",
            global: false,
            permissions: STAFF_PERMISSIONS,
        },
    ]
}

/// Every permission, for the owner's role.
const OWNER_PERMISSIONS: &[&str] = &[
    STAFF_ACCESS,
    CATALOG_MANAGE,
    PRICES_CHANGE,
    STOCK_VIEW,
    STOCK_ADJUST,
    STOCK_RECEIVE,
    CONSIGNMENT_MANAGE,
    PURCHASING_MANAGE,
    ORDERS_VIEW,
    ORDERS_SELL,
    ORDERS_REFUND,
    RENTALS_VIEW,
    RENTALS_CHECKOUT,
    RENTALS_RETURN,
    RENTALS_VERIFY_ID,
    FLEET_VIEW,
    FLEET_PLACE,
    FLEET_MANAGE,
    WORKORDERS_VIEW,
    WORKORDERS_UPDATE,
    WORKORDERS_ASSIGN,
    PLANS_MANAGE,
    CUSTOMERS_VIEW,
    CUSTOMERS_MANAGE,
    STAFF_MANAGE,
    STAFF_HELP,
    ROLES_MANAGE,
    STORES_MANAGE,
    REPORTS_VIEW,
    INTERCOMPANY_VIEW,
    INTERCOMPANY_SETTLE,
    SETTINGS_FEES,
    AUDIT_VIEW,
];

/// Creates every role with its permissions (`permissions::define_role`,
/// which also creates the permissions). Safe to run again: it sets each
/// role's permissions to exactly the list above.
pub async fn define_roles(db: &renox::db::Db) -> renox::Result {
    for role in roles() {
        renox::auth::permissions::define_role(db, role.name, role.permissions).await?;
    }
    Ok(())
}
