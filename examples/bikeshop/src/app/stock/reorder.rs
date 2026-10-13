//! The daily reorder check (`stock:reorder`, every day at 06:30, in
//! `APP_TIMEZONE`; `schedule:run stock:reorder` runs it now).
//!
//! For each store, the variants whose stock **at** the store (its own and
//! goods held for others, minus what is reserved) is under their reorder
//! level. For each store with any:
//!
//! 1. a **suggested purchase order** per supplier (the cheapest supplier
//!    whose price list has the variant), topping each variant up to twice
//!    its level; yesterday's suggestions that nobody touched are replaced,
//!    so the drafts always say what is short today;
//! 2. **"another store has spare"**: other stores whose own stock of the
//!    variant stays above its level after lending what is short, a hint to
//!    ask them for a consignment instead of buying;
//! 3. a **mail and an in-app notification** to the people who may order
//!    for the store (`purchasing.manage` there, so the manager and the
//!    owner), listing all of it.
//!
//! Each step is a plain function, so tests run [`run`] after
//! `TestApp::travel` and read what it did.

use std::collections::{BTreeMap, HashMap};

use renox::prelude::*;
use renox::schedule::Schedule;
use serde::Serialize;

use super::ledger::{store_names, variant_names};
use super::model::{PurchaseOrder, PurchaseOrderLine, PurchaseStatus, StockRow, SupplierItem};
use super::{notify, purchasing};
use crate::app::access::catalogue;
use crate::app::rentals::notify::{Notice, Tone};
use crate::app::staff::model::Store;

// [explain:stock.purchasing.reorder]
/// Registers the task.
pub fn schedule(s: &mut Schedule) {
    s.daily_at("06:30", "stock:reorder", |state: AppState| async move {
        run(&state).await.map(|_| ())
    });
}
// [/explain:stock.purchasing.reorder]

/// A variant short at a store.
#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Low {
    pub variant_id: i64,
    pub name: String,
    pub sku: String,
    /// Available at the store now.
    pub available: i64,
    pub reorder_level: i64,
    /// What the suggestion orders: up to twice the level.
    pub order: i64,
    /// Other stores that could lend it: (store, units they can spare).
    pub spare: Vec<(i64, i64)>,
    /// The cheapest supplier selling it: (supplier, unit cost).
    pub supplier: Option<(i64, i64)>,
}

/// What the check found and did at one store.
#[derive(Serialize, Debug, Clone, Default)]
pub struct StoreReport {
    pub store_id: i64,
    pub low: Vec<Low>,
    /// The suggested purchase orders it drafted.
    pub orders: Vec<i64>,
    /// How many people were told.
    pub told: usize,
}

/// The variants short at `store`, with spares elsewhere and suppliers.
pub async fn low_at(db: &Db, store: i64) -> Result<Vec<Low>> {
    let rows = StockRow::query()
        .where_(StockRow::LOCATION_STORE_ID.eq(store))
        .where_(StockRow::REORDER_LEVEL.gt(0))
        .get(db)
        .await?;
    let mut here: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    for row in rows {
        let entry = here.entry(row.variant_id).or_insert((0, row.reorder_level));
        entry.0 += row.available;
    }
    here.retain(|_, (available, level)| *available < *level);
    if here.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = here.keys().copied().collect();
    // Other stores' own goods on their own shelves.
    let elsewhere = StockRow::query()
        .where_(StockRow::VARIANT_ID.is_in(ids.clone()))
        .where_(StockRow::OWNER_STORE_ID.ne(store))
        .where_raw("owner_store_id = location_store_id", Vec::<i64>::new())
        .get(db)
        .await?;
    let items = SupplierItem::query()
        .where_(SupplierItem::VARIANT_ID.is_in(ids.clone()))
        .order_by(SupplierItem::COST)
        .order_by(SupplierItem::SUPPLIER_ID)
        .get(db)
        .await?;
    let names = variant_names(db, ids).await?;
    Ok(here
        .into_iter()
        .map(|(variant_id, (available, level))| {
            let order = level * 2 - available;
            let short = level - available;
            let spare = elsewhere
                .iter()
                .filter(|r| r.variant_id == variant_id && r.available - level >= short)
                .map(|r| (r.owner_store_id, r.available - level))
                .collect();
            let supplier = items
                .iter()
                .find(|i| i.variant_id == variant_id)
                .map(|i| (i.supplier_id, i.cost));
            let name = names.get(&variant_id).cloned().unwrap_or_default();
            Low {
                variant_id,
                name: name.label(),
                sku: name.sku,
                available,
                reorder_level: level,
                order,
                spare,
                supplier,
            }
        })
        .collect())
}

// [explain:stock.purchasing.reorder]
/// The whole check, for every store (see the module docs).
pub async fn run(state: &AppState) -> Result<Vec<StoreReport>> {
    let mut reports = Vec::new();
    for store in Store::all_by_name(&state.db).await? {
        let low = low_at(&state.db, store.id).await?;
        let mut report = StoreReport {
            store_id: store.id,
            ..Default::default()
        };
        if low.is_empty() {
            reports.push(report);
            continue;
        }
        report.orders = suggest(&state.db, store.id, &low).await?;
        report.told = tell(state, &store, &low).await?;
        report.low = low;
        reports.push(report);
    }
    Ok(reports)
}
// [/explain:stock.purchasing.reorder]

/// Replaces the store's untouched suggested drafts with today's: one per
/// supplier, for the variants it sells.
async fn suggest(db: &Db, store: i64, low: &[Low]) -> Result<Vec<i64>> {
    let old: Vec<i64> = PurchaseOrder::query()
        .where_(PurchaseOrder::STORE_ID.eq(store))
        .where_(PurchaseOrder::STATUS.eq(PurchaseStatus::Draft))
        .where_(PurchaseOrder::SUGGESTED.eq(true))
        .get(db)
        .await?
        .into_iter()
        .map(|o| o.id)
        .collect();
    if !old.is_empty() {
        let mut tx = db.begin().await?;
        PurchaseOrderLine::query()
            .where_(PurchaseOrderLine::PURCHASE_ORDER_ID.is_in(old.clone()))
            .delete(&mut tx)
            .await?;
        PurchaseOrder::query()
            .where_(PurchaseOrder::ID.is_in(old))
            .delete(&mut tx)
            .await?;
        tx.commit().await?;
    }
    let mut by_supplier: BTreeMap<i64, Vec<(i64, i64, i64)>> = BTreeMap::new();
    for item in low {
        if let Some((supplier, cost)) = item.supplier {
            by_supplier
                .entry(supplier)
                .or_default()
                .push((item.variant_id, item.order, cost));
        }
    }
    let mut ids = Vec::new();
    for (supplier, lines) in by_supplier {
        let order = purchasing::draft(
            db,
            store,
            supplier,
            None,
            Some("Suggested by the daily reorder check.".into()),
            &lines,
            true,
        )
        .await?;
        ids.push(order.id);
    }
    Ok(ids)
}

/// The mail and the bell for the store's buyers.
async fn tell(state: &AppState, store: &Store, low: &[Low]) -> Result<usize> {
    let stores: HashMap<i64, String> = store_names(&state.db).await?;
    let lang = state.current_lang();
    let mut notice = Notice::new(
        "stock-reorder",
        "stock.mail.reorder.title",
        "stock.mail.reorder.body",
    )
    .param("store", &store.name)
    .param("count", low.len())
    .tone(Tone::Warning)
    .view("mail/stock/notice")
    .url(crate::app::rentals::link(
        state,
        "stock.purchasing",
        None::<i64>,
    )?);
    for item in low {
        notice = notice.row(
            "stock.fields.short",
            lang.t(
                "stock.reorder.line",
                &[
                    ("name", &item.name as &dyn std::fmt::Display),
                    ("sku", &item.sku),
                    ("available", &item.available),
                    ("level", &item.reorder_level),
                    ("order", &item.order),
                ],
            ),
        );
        for (other, units) in &item.spare {
            notice = notice.row(
                "stock.fields.spare",
                lang.t(
                    "stock.reorder.spare",
                    &[
                        (
                            "store",
                            &stores.get(other).cloned().unwrap_or_default()
                                as &dyn std::fmt::Display,
                        ),
                        ("units", units),
                        ("name", &item.name),
                    ],
                ),
            );
        }
    }
    notify::store_staff(state, catalogue::PURCHASING_MANAGE, store.id, &notice).await
}
