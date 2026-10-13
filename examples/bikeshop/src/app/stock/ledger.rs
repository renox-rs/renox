//! The stock ledger's rules, shared by every page that moves stock.
//!
//! **Levels only change with a movement, in the same transaction.** Two
//! ways in, both writing the `stock_movements` row and changing the
//! `stock_levels` row together:
//!
//! - [`StockMovement::record`] (in the model): adds a movement, whatever
//!   its sign (a receipt, a return, an adjustment found more);
//! - [`take`]: takes goods **out** only if they are there, with a guarded
//!   update (`… WHERE on_hand - reserved >= ?`). Two people shipping or
//!   selling the last unit at once both pass a check made before, but the
//!   database runs the updates one after the other and the second changes
//!   no row: it gets `false`, and nothing was written.
//!
//! [`mismatches`] proves it: every level equals the sum of its movements
//! (`reserved` / `released` move units between `on_hand` and `reserved`).
//! `tests/stock.rs` checks it after concurrent sales.

use std::collections::HashMap;

use renox::db::relations::belongs_to;
use renox::db::{Transaction, sql};
use renox::prelude::*;
use serde::Serialize;

use super::model::{MovementReason, StockMovement};
use crate::app::catalog::model::{Product, ProductVariant};
use crate::app::staff::model::Store;

/// Takes `-movement.quantity` units out of the level the movement names
/// (`movement.quantity` is negative), only if that many are available, and
/// writes the movement: `Ok(Some(movement))`, or `Ok(None)` when they
/// aren't there (nothing was written).
pub async fn take(tx: &mut Transaction, movement: StockMovement) -> Result<Option<StockMovement>> {
    let units = -movement.quantity;
    debug_assert!(units > 0, "take moves stock out");
    let changed = sql(
        "UPDATE stock_levels SET on_hand = on_hand - ?, updated_at = ? \
         WHERE variant_id = ? AND owner_store_id = ? AND location_store_id = ? \
         AND on_hand - reserved >= ?",
    )
    .bind(units)
    .bind(renox::db::now())
    .bind(movement.variant_id)
    .bind(movement.owner_store_id)
    .bind(movement.location_store_id)
    .bind(units)
    .execute(&mut *tx)
    .await?;
    if changed == 0 {
        return Ok(None);
    }
    let mut movement = movement;
    movement.insert(&mut *tx).await?;
    Ok(Some(movement))
}

/// A stock level that doesn't equal the sum of its movements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mismatch {
    pub variant_id: i64,
    pub owner_store_id: i64,
    pub location_store_id: i64,
    /// `(on_hand, reserved)` in `stock_levels`.
    pub level: (i64, i64),
    /// `(on_hand, reserved)` summed from `stock_movements`.
    pub ledger: (i64, i64),
}

/// A level and what its movements add up to.
#[derive(FromRow)]
struct Sums {
    variant_id: i64,
    owner_store_id: i64,
    location_store_id: i64,
    on_hand: i64,
    reserved: i64,
    ledger_on_hand: i64,
    ledger_reserved: i64,
}

/// Every level whose `on_hand` or `reserved` differs from what its
/// movements add up to (none, if every change went through the ledger).
pub async fn mismatches(db: &Db) -> Result<Vec<Mismatch>> {
    let reserved = MovementReason::Reserved.as_str();
    let released = MovementReason::Released.as_str();
    let rows: Vec<Sums> = sql(format!(
        "SELECT l.variant_id, l.owner_store_id, l.location_store_id, l.on_hand, l.reserved, \
         CAST(COALESCE(SUM(CASE WHEN m.reason IN ('{reserved}', '{released}') THEN 0 ELSE m.quantity END), 0) AS BIGINT) AS ledger_on_hand, \
         CAST(COALESCE(SUM(CASE WHEN m.reason = '{reserved}' THEN m.quantity \
                                WHEN m.reason = '{released}' THEN -m.quantity ELSE 0 END), 0) AS BIGINT) AS ledger_reserved \
         FROM stock_levels l LEFT JOIN stock_movements m ON m.variant_id = l.variant_id \
         AND m.owner_store_id = l.owner_store_id AND m.location_store_id = l.location_store_id \
         GROUP BY l.id, l.variant_id, l.owner_store_id, l.location_store_id, l.on_hand, l.reserved"
    ))
    .fetch_as(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|r| (r.on_hand, r.reserved) != (r.ledger_on_hand, r.ledger_reserved))
        .map(|r| Mismatch {
            variant_id: r.variant_id,
            owner_store_id: r.owner_store_id,
            location_store_id: r.location_store_id,
            level: (r.on_hand, r.reserved),
            ledger: (r.ledger_on_hand, r.ledger_reserved),
        })
        .collect())
}

/// A variant as stock pages name it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VariantName {
    pub product: String,
    pub sku: String,
    pub size: Option<String>,
    pub colour: Option<String>,
    pub cost: i64,
    pub price: i64,
    pub reorder_level: i64,
}

impl VariantName {
    /// `Trail 5 (M)`.
    pub fn label(&self) -> String {
        match &self.size {
            Some(size) => format!("{} ({size})", self.product),
            None => self.product.clone(),
        }
    }
}

/// The names of these variants, in two queries (variants, products).
pub async fn variant_names(db: &Db, ids: Vec<i64>) -> Result<HashMap<i64, VariantName>> {
    let mut ids = ids;
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let variants = ProductVariant::find_many(db, ids).await?;
    let products = belongs_to::<Product, _, _>(db, &variants, |v| v.product_id).await?;
    Ok(variants
        .into_iter()
        .map(|v| {
            let product = products
                .get(&v.product_id)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            (
                v.id,
                VariantName {
                    product,
                    sku: v.sku,
                    size: v.size,
                    colour: v.colour,
                    cost: v.cost,
                    price: v.price,
                    reorder_level: v.reorder_level,
                },
            )
        })
        .collect())
}

/// Every store's name by id (three rows).
pub async fn store_names(db: &Db) -> Result<HashMap<i64, String>> {
    Ok(Store::all_by_name(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect())
}
