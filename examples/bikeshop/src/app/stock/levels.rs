//! The stock grid and one level's ledger.
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/stock` | `stock.index` | `stock.view` (in the active store) |
//! | `GET /staff/stock/{level}` | `stock.ledger` | `stock.view` in the level's owner **or** location store |
//! | `POST /staff/stock/{level}/write-off` | `stock.write_off` | `stock.adjust` in the **owner** store |
//!
//! Goods have two store attributes (#245): the **owner** (whose books) and
//! the **location** (where they are). The grid's tabs show the active
//! store's own goods, goods it holds for other stores (consigned here), its
//! own goods at other stores, and what is under its reorder level.

use std::collections::HashMap;

use renox::grid::{Column, Grid, GridRequest, Summary};
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::ledger::{self, store_names, variant_names};
use super::model::{
    ConsignmentShipment, MovementReason, PurchaseOrder, REFERENCE, StockLevel, StockMovement,
    StockRow,
};
use crate::app::access::{self, StoreAttr, StoreRecord, active_store, catalogue};
use crate::app::rentals::model::RentalBike;
use crate::app::sales::model::Order;
use crate::app::staff::model::Staff;
use crate::app::workshop::model::WorkOrder;

/// The grid's tabs (`?view=`).
pub const VIEWS: [&str; 4] = ["own", "held", "away", "low"];

/// The stock grid: one row per stock level (the `stock_overview` view),
/// with the owner and location stores, sums under the quantity and value
/// columns, grouping by category, exports, and cards on phones.
// [explain:stock.index.grid]
pub fn stock_grid(lang: &Lang) -> Grid {
    let t = |key: &str| lang.t(&format!("stock.fields.{key}"), &[]);
    Grid::new("stock")
        .title(&lang.t("stock.index.title", &[]))
        .column(
            Column::text("product", &t("product"))
                .frozen()
                .mobile()
                .searchable()
                .link("/staff/stock/{id}"),
        )
        // [/explain:stock.index.grid]
        .column(Column::text("sku", &t("sku")).searchable().copyable())
        .column(Column::text("size", &t("size")))
        .column(Column::text("category", &t("category")))
        .column(Column::related(
            "owner_store",
            &t("owner_store"),
            "stores",
            "owner_store_id",
            "name",
        ))
        .column(Column::related(
            "location_store",
            &t("location_store"),
            "stores",
            "location_store_id",
            "name",
        ))
        // [explain:stock.index.grid]
        .column(Column::number("on_hand", &t("on_hand")).summary(Summary::Sum))
        .column(Column::number("reserved", &t("reserved")).summary(Summary::Sum))
        .column(
            Column::number("available", &t("available"))
                .summary(Summary::Sum)
                .mobile(),
        )
        .column(Column::number("reorder_level", &t("reorder_level")))
        .column(Column::money("cost", &t("cost")).hidden())
        .column(Column::money("value_at_cost", &t("value")).summary(Summary::Sum))
        .groups(&["category"])
        .sort_by("product,sku")
        .per_page(50)
        .row_url("/staff/stock/{id}")
        .exports()
        .cards_on_mobile()
        .empty_state(&lang.t("stock.index.empty", &[]), None)
}
// [/explain:stock.index.grid]

/// `?view=` on the grid.
#[derive(Deserialize, Default)]
pub struct StockQuery {
    #[serde(default)]
    pub view: String,
}

/// The rows of one tab: always within what the person may see ("mine or
/// at my store", `scopes_with`), then narrowed to the active store.
// [explain:stock.index.handler]
pub fn rows_of(tab: &str, store: i64) -> renox::db::Query<StockRow> {
    let rows = access::visible::<StockRow>(catalogue::STOCK_VIEW);
    match tab {
        "held" => rows
            .where_(StockRow::LOCATION_STORE_ID.eq(store))
            .where_(StockRow::OWNER_STORE_ID.ne(store)),
        "away" => rows
            .where_(StockRow::OWNER_STORE_ID.eq(store))
            .where_(StockRow::LOCATION_STORE_ID.ne(store)),
        "low" => rows
            .where_(StockRow::LOCATION_STORE_ID.eq(store))
            .where_(StockRow::REORDER_LEVEL.gt(0))
            .where_raw("available < reorder_level", Vec::<i64>::new()),
        _ => rows
            .where_(StockRow::OWNER_STORE_ID.eq(store))
            .where_(StockRow::LOCATION_STORE_ID.eq(store)),
    }
}
// [/explain:stock.index.handler]

/// `GET /staff/stock` (`stock.index`): the grid, or its export
/// (`?export=csv|xlsx|print` from the grid's menu).
// [explain:stock.index.handler]
pub async fn index(
    State(db): State<Db>,
    lang: Lang,
    request: GridRequest,
    Query(query): Query<StockQuery>,
) -> Result<Response> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let tab = if VIEWS.contains(&query.view.as_str()) {
        query.view.clone()
    } else {
        "own".to_owned()
    };
    let grid = stock_grid(&lang);
    if let Some(file) = grid.export(rows_of(&tab, store), &request).await? {
        return Ok(file);
    }
    let page = grid.page(rows_of(&tab, store), &request).await?;
    // [/explain:stock.index.handler]
    // The counts on the tabs: four small queries.
    let mut counts = HashMap::new();
    for key in VIEWS {
        counts.insert(key, rows_of(key, store).count(&db).await?);
    }
    Ok(view(
        "stock/index.html",
        context! { stock => page, tab, views => VIEWS, counts },
    )
    .into_response())
}

/// A movement as the ledger lists it.
#[derive(Serialize, Debug, Clone)]
pub struct LedgerLine {
    #[serde(flatten)]
    pub movement: StockMovement,
    /// The person who made it.
    pub by: Option<String>,
    /// The document that caused it: its label and link.
    pub source: Option<(String, String)>,
    /// The level after this movement (running, newest first).
    pub after: i64,
}

/// What a store may do with one level, as the page lists it (ABAC).
#[derive(Serialize, Debug, Clone)]
pub struct Ability {
    pub action: &'static str,
    pub permission: &'static str,
    pub attribute: &'static str,
    pub store: String,
    pub allowed: bool,
}

/// `GET /staff/stock/{level}` (`stock.ledger`): one variant's stock owned
/// by one store at one store, and every movement that made it, newest
/// first, each linked to its source document. Visible to the staff of the
/// owner **and** the location store; anyone else gets a 404.
// [explain:stock.ledger.handler]
pub async fn show(
    State(db): State<Db>,
    user: AuthUser,
    Page(page): Page,
    Path(id): Path<i64>,
) -> Result<View> {
    let level = access::find::<StockLevel>(&db, &user, id).await?;
    let names = variant_names(&db, vec![level.variant_id]).await?;
    let variant = names.get(&level.variant_id).cloned().unwrap_or_default();
    let stores = store_names(&db).await?;
    let store = |id: i64| stores.get(&id).cloned().unwrap_or_default();
    let movements = StockMovement::query()
        .where_(StockMovement::VARIANT_ID.eq(level.variant_id))
        .where_(StockMovement::OWNER_STORE_ID.eq(level.owner_store_id))
        .where_(StockMovement::LOCATION_STORE_ID.eq(level.location_store_id))
        .order_by_desc(StockMovement::ID)
        .paginate(&db, page, 25)
        .await?;
    // [/explain:stock.ledger.handler]
    // The level after each movement: from today's level, walking back.
    let newer: i64 = StockMovement::query()
        .where_(StockMovement::VARIANT_ID.eq(level.variant_id))
        .where_(StockMovement::OWNER_STORE_ID.eq(level.owner_store_id))
        .where_(StockMovement::LOCATION_STORE_ID.eq(level.location_store_id))
        .where_not_in(
            "reason",
            [MovementReason::Reserved, MovementReason::Released],
        )
        .when(!movements.items.is_empty(), |q| {
            q.where_(
                StockMovement::ID.gt(movements.items.first().map(|m| m.id).unwrap_or_default()),
            )
        })
        .sum(&db, "quantity")
        .await?;
    let staff = renox::db::relations::belongs_to::<Staff, _, _>(&db, &movements.items, |m| {
        m.staff_id.unwrap_or_default()
    })
    .await?;
    let users = renox::db::relations::belongs_to::<User, _, _>(
        &db,
        &staff.values().cloned().collect::<Vec<_>>(),
        |s| s.user_id,
    )
    .await?;
    let sources = sources(&db, &movements.items).await?;
    let mut running = level.on_hand - newer;
    let movements = movements.map(|m| {
        let after = running;
        if !matches!(
            m.reason,
            MovementReason::Reserved | MovementReason::Released
        ) {
            running -= m.quantity;
        }
        LedgerLine {
            by: m
                .staff_id
                .and_then(|id| staff.get(&id))
                .and_then(|s| users.get(&s.user_id))
                .map(|u| u.name.clone()),
            source: m
                .reference_type
                .as_deref()
                .zip(m.reference_id)
                .and_then(|(kind, id)| sources.get(&(kind.to_owned(), id)).cloned()),
            after,
            movement: m,
        }
    });
    let actions: [(&str, &str, StoreAttr); 4] = [
        (
            "stock.abilities.sell",
            catalogue::ORDERS_SELL,
            StoreAttr::Location,
        ),
        (
            "stock.abilities.count",
            catalogue::STOCK_ADJUST,
            StoreAttr::Location,
        ),
        (
            "stock.abilities.recall",
            catalogue::CONSIGNMENT_MANAGE,
            StoreAttr::Owner,
        ),
        (
            "stock.abilities.write_off",
            catalogue::STOCK_ADJUST,
            StoreAttr::Owner,
        ),
    ];
    // [explain:stock.ledger.handler]
    let abilities: Vec<Ability> = actions
        .iter()
        .filter(|(action, ..)| level.consigned() || *action != "stock.abilities.recall")
        .map(|(action, permission, attr)| Ability {
            action,
            permission,
            attribute: if *attr == StoreAttr::Owner {
                "owner_store"
            } else {
                "location_store"
            },
            store: store(level.store_id(*attr).unwrap_or_default()),
            allowed: access::can(&user, permission, *attr, &level),
        })
        .collect();
    let can_write_off = access::can(&user, catalogue::STOCK_ADJUST, StoreAttr::Owner, &level);
    // [/explain:stock.ledger.handler]
    Ok(view(
        "stock/ledger.html",
        context! {
            owner => store(level.owner_store_id),
            location => store(level.location_store_id),
            value => level.on_hand * variant.cost,
            available => level.available(),
            consigned => level.consigned(),
            label => variant.label(),
            variant,
            abilities,
            can_write_off,
            movements,
            level,
        },
    ))
}

// [explain:stock.ledger.sources]
/// The documents behind a page of movements, by `(table, id)`: one query
/// per kind of document (`Morph::parents`), with a label and a link.
async fn sources(
    db: &Db,
    movements: &[StockMovement],
) -> Result<HashMap<(String, i64), (String, String)>> {
    let key = |m: &StockMovement| {
        (
            m.reference_type.clone().unwrap_or_default(),
            m.reference_id.unwrap_or_default(),
        )
    };
    let mut found = HashMap::new();
    for (id, order) in REFERENCE.parents::<Order, _>(db, movements, key).await? {
        found.insert(
            (Order::TABLE.to_owned(), id),
            (format!("#{}", order.number), format!("/staff/orders/{id}")),
        );
    }
    // [/explain:stock.ledger.sources]
    for (id, _) in REFERENCE
        .parents::<WorkOrder, _>(db, movements, key)
        .await?
    {
        found.insert(
            (WorkOrder::TABLE.to_owned(), id),
            (format!("WO-{id}"), format!("/staff/workshop/{id}")),
        );
    }
    for (id, _) in REFERENCE
        .parents::<ConsignmentShipment, _>(db, movements, key)
        .await?
    {
        found.insert(
            (ConsignmentShipment::TABLE.to_owned(), id),
            (format!("CS-{id}"), format!("/staff/consignments/{id}")),
        );
    }
    for (id, _) in REFERENCE
        .parents::<PurchaseOrder, _>(db, movements, key)
        .await?
    {
        found.insert(
            (PurchaseOrder::TABLE.to_owned(), id),
            (format!("PO-{id}"), format!("/staff/purchase-orders/{id}")),
        );
    }
    for (id, bike) in REFERENCE
        .parents::<RentalBike, _>(db, movements, key)
        .await?
    {
        found.insert(
            (RentalBike::TABLE.to_owned(), id),
            (bike.frame_number.clone(), format!("/staff/fleet/{id}")),
        );
    }
    // [explain:stock.ledger.sources]
    Ok(found)
}
// [/explain:stock.ledger.sources]

/// The write-off form.
#[derive(Deserialize, Validate, Debug)]
pub struct WriteOffForm {
    #[validate(required, min = 1, max = 10000)]
    pub quantity: Option<i64>,
    #[validate(required, max = 200)]
    pub reason: String,
}

/// `POST /staff/stock/{level}/write-off` (`stock.write_off`): the owner
/// store writes goods off (broken, lost, given away), wherever they are.
/// Only the **owner** may: for consigned goods at another store, that
/// store may count them (a stock take) but not write them off. Audited.
// [explain:stock.ledger.write_off]
pub async fn write_off(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<WriteOffForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let level = access::find::<StockLevel>(db, &user, id).await?;
    access::require(&user, catalogue::STOCK_ADJUST, StoreAttr::Owner, &level)?;
    let lang = state.current_lang();
    let quantity = form.quantity.unwrap_or_default();
    let staff = Staff::of_user(db, user.id).await?.map(|s| s.id);
    let mut tx = db.begin().await?;
    let taken = ledger::take(
        // [/explain:stock.ledger.write_off]
        &mut tx,
        StockMovement {
            variant_id: level.variant_id,
            owner_store_id: level.owner_store_id,
            location_store_id: level.location_store_id,
            quantity: -quantity,
            reason: MovementReason::Adjustment,
            staff_id: staff,
            note: Some(format!("Written off: {}", form.reason.trim())),
            ..Default::default()
        },
    )
    .await?;
    // [explain:stock.ledger.write_off]
    let Some(movement) = taken else {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("stock.ledger.not_enough", &[]),
        ));
    };
    tx.commit().await?;
    // [/explain:stock.ledger.write_off]
    crate::app::multistore::audit::record(
        &state,
        &user,
        "stock.written_off",
        level.owner_store_id,
        ("stock_movements", movement.id),
        json!({ "level": level.id, "quantity": quantity, "reason": form.reason }),
    )
    .await?;
    Ok((
        Toast::success(lang.t("stock.ledger.written_off", &[])),
        Redirect::route("stock.ledger", &[&level.id])?,
    ))
}
