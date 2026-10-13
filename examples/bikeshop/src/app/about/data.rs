//! `/about/data` (`about.data`): the bike shop's data model explained.
//!
//! What a reader learns here, from the code itself rather than a copy of
//! it: every table grouped by the area that owns its model, the relations
//! and how they are loaded, Pagila's tables and what they became, the three
//! store attributes, how money, secrets, statuses and deletions are stored,
//! the factories and the seeders, and the roles with their permissions
//! (read from `access::catalogue`). The row counts are live, so after
//! `demo:seed --size large` the page shows Pagila's volume.

use renox::db::sql;
use renox::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

use crate::app::access::catalogue;
use crate::app::catalog::model::CategoryKind;
use crate::app::multistore::model::EntryKind;
use crate::app::rentals::model::{BikeStatus, RentalStatus};
use crate::app::sales::model::{OrderStatus, PaymentMethod};
use crate::app::stock::model::MovementReason;
use crate::app::workshop::model::WorkStatus;

/// A table and what it holds.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Table {
    /// The table's name.
    pub name: &'static str,
    /// The model that reads and writes it.
    pub model: &'static str,
    /// What one row is.
    pub holds: &'static str,
}

/// The tables of one area, with the file that has their models.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Area {
    /// The area's name.
    pub title: &'static str,
    /// Its models' file, from the repository's root.
    pub file: &'static str,
    /// Its tables.
    pub tables: &'static [Table],
}

const fn t(name: &'static str, model: &'static str, holds: &'static str) -> Table {
    Table { name, model, holds }
}

/// Every table of the shop, by area.
pub const AREAS: &[Area] = &[
    Area {
        title: "Places and customers",
        file: "examples/bikeshop/src/app/accounts/model.rs",
        tables: &[
            t("countries", "Country", "A country, as in Pagila."),
            t("cities", "City", "A city in a country."),
            t(
                "addresses",
                "Address",
                "A street address: of a store, a customer, a supplier, a delivery.",
            ),
            t(
                "customers",
                "Customer",
                "A customer of the company (not of a store); the ID number sealed; soft deleted.",
            ),
        ],
    },
    Area {
        title: "Stores and staff",
        file: "examples/bikeshop/src/app/staff/model.rs",
        tables: &[
            t(
                "stores",
                "Store",
                "One of the three stores: hours as JSON, workshop minutes, its fee rate.",
            ),
            t(
                "staff",
                "Staff",
                "Someone who works for the shop: a user and a home store. No role column.",
            ),
            t(
                "staff_help_requests",
                "StaffHelpRequest",
                "A store asking another to lend someone, with dates and the role wanted.",
            ),
            t(
                "staff_help_hours",
                "StaffHelpHour",
                "Hours worked helping another store (for reports, never charged).",
            ),
        ],
    },
    Area {
        title: "Catalogue",
        file: "examples/bikeshop/src/app/catalog/model.rs",
        tables: &[
            t(
                "categories",
                "Category",
                "A category in a tree, of a kind: bike, gear or part.",
            ),
            t("brands", "Brand", "A brand."),
            t(
                "products",
                "Product",
                "A bike model, a piece of gear or a part; searchable; discontinued = soft deleted.",
            ),
            t(
                "product_variants",
                "ProductVariant",
                "What is sold and stocked: one SKU per size and colour, with its price and cost.",
            ),
            t("product_photos", "ProductPhoto", "A photo of a product."),
            t(
                "part_fits",
                "PART_FITS (Pivot)",
                "Which parts fit which bike models, with a note.",
            ),
        ],
    },
    Area {
        title: "Stock and purchasing",
        file: "examples/bikeshop/src/app/stock/model.rs",
        tables: &[
            t(
                "stock_levels",
                "StockLevel",
                "How many of a variant one store owns at one location.",
            ),
            t(
                "stock_movements",
                "StockMovement",
                "The ledger: every change, its reason and what caused it.",
            ),
            t("suppliers", "Supplier", "Someone the shop buys from."),
            t(
                "purchase_orders",
                "PurchaseOrder",
                "An order to a supplier for one store.",
            ),
            t("purchase_order_lines", "PurchaseOrderLine", "A line of it."),
            t(
                "consignment_shipments",
                "ConsignmentShipment",
                "Goods sent by their owner store to be sold at another.",
            ),
            t(
                "consignment_shipment_lines",
                "ConsignmentShipmentLine",
                "A line: sent, sold there, sent back.",
            ),
        ],
    },
    Area {
        title: "Fleet and rentals",
        file: "examples/bikeshop/src/app/rentals/model.rs",
        tables: &[
            t(
                "rental_bikes",
                "RentalBike",
                "A bike of the rental fleet: its owner store and where it is.",
            ),
            t(
                "bike_placements",
                "BikePlacement",
                "A bike placed at another store, approved by its owner, maybe called back.",
            ),
            t(
                "rentals",
                "Rental",
                "A bike rented by the hour or the day: owner, operating and return stores.",
            ),
        ],
    },
    Area {
        title: "Sales and payments",
        file: "examples/bikeshop/src/app/sales/model.rs",
        tables: &[
            t(
                "orders",
                "Order",
                "An order online or at a counter, sold by its operating store.",
            ),
            t(
                "order_items",
                "OrderItem",
                "A line, with the owner store of the goods (another store's when consigned).",
            ),
            t(
                "payments",
                "Payment",
                "A payment for an order, a rental or a work order.",
            ),
        ],
    },
    Area {
        title: "Workshop and plans",
        file: "examples/bikeshop/src/app/workshop/model.rs",
        tables: &[
            t("customer_bikes", "CustomerBike", "A customer's own bike."),
            t(
                "service_tasks",
                "ServiceTask",
                "Something a mechanic does, with its time and price.",
            ),
            t(
                "work_orders",
                "WorkOrder",
                "A job for a store's workshop, on a customer's bike or a fleet bike.",
            ),
            t(
                "work_order_tasks",
                "WorkOrderTask",
                "A task of a work order: done or not.",
            ),
            t(
                "service_plans",
                "ServicePlan",
                "A set of tasks repeated weekly, monthly…",
            ),
            t(
                "plan_tasks",
                "PLAN_TASKS (Pivot)",
                "The tasks of each plan.",
            ),
            t(
                "plan_subscriptions",
                "PlanSubscription",
                "A customer's bike on a plan, at a store, with its next visit.",
            ),
        ],
    },
    Area {
        title: "Between stores",
        file: "examples/bikeshop/src/app/multistore/model.rs",
        tables: &[
            t(
                "intercompany_entries",
                "IntercompanyEntry",
                "One store owing another: why, how much, for which rental, sale or repair.",
            ),
            t(
                "settlements",
                "Settlement",
                "A month's net balance between two stores.",
            ),
        ],
    },
    Area {
        title: "Pages about Renox",
        file: "examples/bikeshop/src/app/about/fields.rs",
        tables: &[t(
            "field_samples",
            "FieldSample",
            "A visitor's sample bike on /about/fields: one column per kind of form input.",
        )],
    },
    Area {
        title: "Renox's tables",
        file: "examples/bikeshop/src/lib.rs",
        tables: &[
            t(
                "users",
                "User (Auth module)",
                "Everyone who logs in: staff and customers with an account.",
            ),
            t(
                "roles",
                "Permissions module",
                "The roles: owner, manager, cashier, mechanic, staff.",
            ),
            t(
                "permissions",
                "Permissions module",
                "The permissions the code checks.",
            ),
            t(
                "permission_role",
                "Permissions module",
                "Which role grants which permission.",
            ),
            t(
                "role_user",
                "Permissions module",
                "Who has which role, in which store (scope), between which dates.",
            ),
        ],
    },
];

/// A relation and how the code loads it.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Relation {
    pub from: &'static str,
    pub kind: &'static str,
    pub to: &'static str,
    pub how: &'static str,
}

const fn r(
    from: &'static str,
    kind: &'static str,
    to: &'static str,
    how: &'static str,
) -> Relation {
    Relation {
        from,
        kind,
        to,
        how,
    }
}

/// The main relations.
pub const RELATIONS: &[Relation] = &[
    r(
        "products",
        "belongs to",
        "brands, categories",
        "`belongs_to` in `ProductCard::load` (one query each for a whole page)",
    ),
    r(
        "products",
        "has many",
        "product_variants",
        "`has_many` in `ProductCard::load`",
    ),
    r(
        "products (parts)",
        "many to many",
        "products (bikes)",
        "`Pivot`: `PART_FITS` / `FITTING_PARTS`, with a `note` read by `load_with_pivot`",
    ),
    r(
        "rentals",
        "belongs to",
        "rental_bikes, customers, stores",
        "`belongs_to` and `find_many` in `RentalRow::load` (5 queries)",
    ),
    r(
        "customers",
        "has many through",
        "work_orders (via customer_bikes)",
        "`has_many_through` in `workshop::model::work_orders_of` (2 queries)",
    ),
    r(
        "service_plans",
        "many to many",
        "service_tasks",
        "`Pivot`: `PLAN_TASKS`",
    ),
    r(
        "stock_movements",
        "morph to",
        "orders, work_orders, consignment_shipments, purchase_orders",
        "`Morph`: `REFERENCE` (`reference_type`, `reference_id`)",
    ),
    r(
        "payments",
        "morph to",
        "orders, rentals, work_orders",
        "`Morph`: `PAYABLE` (`payable_type`, `payable_id`)",
    ),
    r(
        "intercompany_entries",
        "morph to",
        "rentals, orders, work_orders",
        "`Morph`: `SOURCE` (`source_type`, `source_id`)",
    ),
    r(
        "addresses",
        "belongs to",
        "cities → countries",
        "`FullAddress::load` (3 queries)",
    ),
];

/// Pagila's table and what it became.
pub const PAGILA: &[(&str, &str)] = &[
    ("store", "stores: three, working together"),
    (
        "staff",
        "staff, with roles per store (role_user) instead of a store column",
    ),
    (
        "customer, address, city, country",
        "customers (of the company), addresses, cities, countries",
    ),
    (
        "film",
        "products: bike models, gear and parts, with product_variants",
    ),
    (
        "category, film_category",
        "categories (a tree, of a kind); a product has one",
    ),
    (
        "actor, film_actor",
        "part_fits: which parts fit which bike models",
    ),
    (
        "film_text (full-text)",
        "products' search index (FTS5 / tsvector)",
    ),
    (
        "inventory",
        "rental_bikes and stock_levels, each with an owner and a location store",
    ),
    (
        "rental",
        "rentals: by the hour or the day, late fees, the operating store",
    ),
    ("payment", "payments for sales, rentals and services"),
    (
        "(new)",
        "orders, order_items, customer_bikes, service_tasks, work_orders, service_plans, plan_subscriptions, suppliers, purchase_orders, consignment_shipments, bike_placements, staff_help_requests, intercompany_entries, settlements",
    ),
];

/// A record kind and its store attributes.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Attributes {
    pub record: &'static str,
    pub owner: &'static str,
    pub location: &'static str,
    pub operating: &'static str,
}

/// Which column holds which store attribute, per record.
pub const STORE_ATTRIBUTES: &[Attributes] = &[
    Attributes {
        record: "rental_bikes",
        owner: "owner_store_id",
        location: "location_store_id",
        operating: "—",
    },
    Attributes {
        record: "rentals",
        owner: "owner_store_id (copied from the bike)",
        location: "return_store_id",
        operating: "operating_store_id",
    },
    Attributes {
        record: "stock_levels, stock_movements",
        owner: "owner_store_id",
        location: "location_store_id",
        operating: "—",
    },
    Attributes {
        record: "consignment_shipments",
        owner: "owner_store_id",
        location: "location_store_id",
        operating: "—",
    },
    Attributes {
        record: "orders",
        owner: "order_items.owner_store_id",
        location: "—",
        operating: "operating_store_id",
    },
    Attributes {
        record: "work_orders",
        owner: "billed_store_id (fleet repairs)",
        location: "—",
        operating: "store_id",
    },
    Attributes {
        record: "intercompany_entries",
        owner: "creditor_store_id",
        location: "—",
        operating: "debtor_store_id",
    },
];

/// An enum and the words it is stored as.
#[derive(Debug, Clone, Serialize)]
pub struct EnumDoc {
    pub name: &'static str,
    pub column: &'static str,
    pub values: Vec<&'static str>,
}

fn enums() -> Vec<EnumDoc> {
    vec![
        EnumDoc {
            name: "CategoryKind",
            column: "categories.kind",
            values: CategoryKind::ALL.iter().map(|v| v.as_str()).collect(),
        },
        EnumDoc {
            name: "BikeStatus",
            column: "rental_bikes.status",
            values: BikeStatus::ALL.iter().map(|v| v.as_str()).collect(),
        },
        EnumDoc {
            name: "RentalStatus",
            column: "rentals.status",
            values: RentalStatus::ALL.iter().map(|v| v.as_str()).collect(),
        },
        EnumDoc {
            name: "OrderStatus",
            column: "orders.status",
            values: OrderStatus::ALL.iter().map(|v| v.as_str()).collect(),
        },
        EnumDoc {
            name: "PaymentMethod",
            column: "payments.method",
            values: PaymentMethod::ALL.iter().map(|v| v.as_str()).collect(),
        },
        EnumDoc {
            name: "WorkStatus",
            column: "work_orders.status",
            values: WorkStatus::ALL.iter().map(|v| v.as_str()).collect(),
        },
        EnumDoc {
            name: "MovementReason",
            column: "stock_movements.reason",
            values: MovementReason::ALL.iter().map(|v| v.as_str()).collect(),
        },
        EnumDoc {
            name: "EntryKind",
            column: "intercompany_entries.kind",
            values: EntryKind::ALL.iter().map(|v| v.as_str()).collect(),
        },
    ]
}

/// A role as the page shows it.
#[derive(Debug, Clone, Serialize)]
struct RoleDoc {
    name: String,
    label: String,
    description: String,
    global: bool,
    permissions: Vec<String>,
}

// [explain:about.data.counts]
/// Rows per table, in one query.
async fn counts(db: &Db) -> Result<HashMap<String, i64>> {
    let names: Vec<&str> = AREAS
        .iter()
        .flat_map(|a| a.tables.iter().map(|t| t.name))
        .collect();
    let union = names
        .iter()
        .map(|name| format!("SELECT '{name}', COUNT(*) FROM {name}"))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let rows: Vec<(String, i64)> = sql(union).fetch_as(db).await?;
    Ok(rows.into_iter().collect())
}
// [/explain:about.data.counts]

// [explain:about.data.handler]
/// `GET /about/data`.
pub async fn show(State(db): State<Db>) -> Result<View> {
    let counts = counts(&db).await?;
    // The roles as they are in the database now (the owner may have changed
    // what they grant), with their labels from the catalogue.
    let defined = catalogue::roles();
    let roles: Vec<RoleDoc> = renox::auth::permissions::roles(&db)
        .await?
        .into_iter()
        .map(|(name, permissions)| {
            let known = defined.iter().find(|r| r.name == name);
            RoleDoc {
                label: known.map_or("", |r| r.label).to_owned(),
                description: known.map_or("", |r| r.description).to_owned(),
                global: known.is_some_and(|r| r.global),
                name,
                permissions,
            }
        })
        .collect();
    let total: i64 = counts.values().sum();
    // [/explain:about.data.handler]
    Ok(view(
        "about/data.html",
        context! {
            areas => AREAS,
            counts,
            total,
            relations => RELATIONS,
            pagila => PAGILA,
            attributes => STORE_ATTRIBUTES,
            enums => enums(),
            roles,
            permissions => catalogue::PERMISSIONS.iter().map(|p| context! { name => p.name, description => p.description }).collect::<Vec<_>>(),
            repository => crate::explain::REPOSITORY,
        },
    ))
}
