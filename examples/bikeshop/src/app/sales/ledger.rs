//! What a sale does to the stock ledger and the books between stores.
//!
//! An order moves stock in three steps, each a `stock_movements` row
//! pointing at the order (`reference_type = 'orders'`):
//!
//! 1. **Placed** ([`reserve`]): `reserved` — the units are put aside
//!    (`stock_levels.reserved` goes up), so nobody else can buy them while
//!    the customer pays.
//! 2. **Paid** ([`sell`]): `released` + `sale` — the reservation becomes a
//!    sale (`reserved` and `on_hand` go down). Consigned goods (another
//!    store's, #245) also book the debt between the two stores: the selling
//!    store owes the owner the line, the owner owes the seller its fee.
//! 3. **Expired or cancelled** ([`release`]): `released` — the units are
//!    free again. A **return** ([`take_back`]) is a `return` movement
//!    (`on_hand` goes up) and reverses the books.
//!
//! **Why it can't oversell.** [`reserve`] runs in the caller's transaction
//! and puts units aside with a conditional update:
//! `UPDATE stock_levels SET reserved = reserved + ? WHERE id = ? AND
//! on_hand - reserved >= ?`. When two customers want the last helmet at
//! once, both may have *seen* one available, but the database runs the two
//! updates one after the other (PostgreSQL locks the row; SQLite lets one
//! writer in at a time), and the second finds the condition false: zero
//! rows changed, a [`Shortfall`], and its transaction is rolled back. The
//! transaction's first statement is that update (the levels are read
//! before it begins), so on SQLite it takes the write lock straight away
//! rather than failing to upgrade a read.
//!
//! The steps after the first read what the ledger says the order still
//! holds ([`held`]), so they are safe to repeat and also work for orders
//! the seeder made.

use std::collections::HashMap;

use renox::db::{Transaction, sql};
use renox::prelude::*;

use super::model::{Order, OrderItem};
use crate::app::multistore::books;
use crate::app::stock::model::{MovementReason, StockLevel, StockMovement};

/// Not enough of a variant at the store: what the customer asked for and
/// what was left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortfall {
    pub variant_id: i64,
    pub wanted: i64,
    pub left: i64,
}

/// The stock levels a store can sell from, per variant: the store's
/// locations, consigned goods first (sold before the store's own, so the
/// owner store gets its goods sold), then by id. Read before the
/// transaction (see the module docs).
pub async fn levels_at(
    db: &Db,
    store_id: i64,
    variant_ids: &[i64],
) -> Result<HashMap<i64, Vec<StockLevel>>> {
    let mut by_variant: HashMap<i64, Vec<StockLevel>> = HashMap::new();
    if variant_ids.is_empty() {
        return Ok(by_variant);
    }
    for level in StockLevel::query()
        .where_(StockLevel::LOCATION_STORE_ID.eq(store_id))
        .where_(StockLevel::VARIANT_ID.is_in(variant_ids.to_vec()))
        .order_by(StockLevel::ID)
        .get(db)
        .await?
    {
        by_variant.entry(level.variant_id).or_default().push(level);
    }
    for levels in by_variant.values_mut() {
        levels.sort_by_key(|l| (!l.consigned(), l.id));
    }
    Ok(by_variant)
}

/// One part of a line taken from one stock level (an owner store).
#[derive(Debug, Clone, Copy)]
pub struct Taken {
    pub variant_id: i64,
    pub owner_store_id: i64,
    pub quantity: i64,
}

/// Puts `quantity` of each `(variant, quantity)` aside at `store_id` for
/// `order_id`, in `tx`, from `levels` (from [`levels_at`]). Returns what
/// was taken from which owner, or the first variant there isn't enough of
/// (the caller rolls back).
pub async fn reserve(
    tx: &mut Transaction,
    order_id: i64,
    store_id: i64,
    lines: &[(i64, i64)],
    levels: &HashMap<i64, Vec<StockLevel>>,
    staff_id: Option<i64>,
) -> Result<std::result::Result<Vec<Taken>, Shortfall>> {
    let now = renox::db::now();
    let mut taken = Vec::new();
    for &(variant_id, wanted) in lines {
        let mut left = wanted;
        for level in levels.get(&variant_id).map(Vec::as_slice).unwrap_or(&[]) {
            if left == 0 {
                break;
            }
            let take = left.min(level.available());
            if take <= 0 {
                continue;
            }
            // The guard: only if the units are still there now.
            let changed = sql(
                "UPDATE stock_levels SET reserved = reserved + ?, updated_at = ? \
                 WHERE id = ? AND on_hand - reserved >= ?",
            )
            .bind(take)
            .bind(now)
            .bind(level.id)
            .bind(take)
            .execute(&mut *tx)
            .await?;
            if changed == 0 {
                continue; // someone was quicker; try the next owner's goods
            }
            StockMovement {
                variant_id,
                owner_store_id: level.owner_store_id,
                location_store_id: store_id,
                quantity: take,
                reason: MovementReason::Reserved,
                reference_type: Some(Order::TABLE.into()),
                reference_id: Some(order_id),
                staff_id,
                ..Default::default()
            }
            .insert(&mut *tx)
            .await?;
            taken.push(Taken {
                variant_id,
                owner_store_id: level.owner_store_id,
                quantity: take,
            });
            left -= take;
        }
        if left > 0 {
            return Ok(Err(Shortfall {
                variant_id,
                wanted,
                left: wanted - left,
            }));
        }
    }
    Ok(Ok(taken))
}

/// What the ledger says the order still holds, and has sold, per
/// (variant, owner, location): `(reserved − released, −sales − … + returns)`.
pub async fn held(
    tx: &mut Transaction,
    order_id: i64,
) -> Result<HashMap<(i64, i64, i64), (i64, i64)>> {
    let rows: Vec<(i64, i64, i64, String, i64)> = sql(
        "SELECT variant_id, owner_store_id, location_store_id, reason, \
         CAST(SUM(quantity) AS BIGINT) FROM stock_movements \
         WHERE reference_type = ? AND reference_id = ? \
         GROUP BY variant_id, owner_store_id, location_store_id, reason",
    )
    .bind(Order::TABLE)
    .bind(order_id)
    .fetch_as(&mut *tx)
    .await?;
    let mut held: HashMap<(i64, i64, i64), (i64, i64)> = HashMap::new();
    for (variant, owner, location, reason, quantity) in rows {
        let entry = held.entry((variant, owner, location)).or_default();
        match reason.parse::<MovementReason>().ok() {
            Some(MovementReason::Reserved) => entry.0 += quantity,
            Some(MovementReason::Released) => entry.0 -= quantity,
            Some(MovementReason::Sale | MovementReason::Return) => entry.1 -= quantity,
            _ => {}
        }
    }
    Ok(held)
}

fn movement(
    order: &Order,
    item: &OrderItem,
    quantity: i64,
    reason: MovementReason,
    staff_id: Option<i64>,
) -> StockMovement {
    StockMovement {
        variant_id: item.variant_id,
        owner_store_id: item.owner_store_id,
        location_store_id: order.operating_store_id,
        quantity,
        reason,
        reference_type: Some(Order::TABLE.into()),
        reference_id: Some(order.id),
        staff_id,
        ..Default::default()
    }
}

/// Turns what the order holds into a sale (step 2), in `tx`: for each
/// line, the units still reserved are released and every unit not sold yet
/// is sold. Consigned lines book the debt between the stores. Safe to run
/// twice: the second time there is nothing left to do.
pub async fn sell(
    tx: &mut Transaction,
    order: &Order,
    items: &[OrderItem],
    staff_id: Option<i64>,
) -> Result {
    let held = held(tx, order.id).await?;
    let mut sold_now: HashMap<(i64, i64, i64), i64> = HashMap::new();
    for item in items {
        let key = (
            item.variant_id,
            item.owner_store_id,
            order.operating_store_id,
        );
        let (reserved, sold) = held.get(&key).copied().unwrap_or((0, 0));
        let already = sold + sold_now.get(&key).copied().unwrap_or(0);
        let to_sell = (item.quantity - already).max(0);
        if to_sell == 0 {
            continue;
        }
        let release = reserved.min(to_sell).max(0);
        if release > 0 {
            StockMovement::record(
                tx,
                movement(order, item, release, MovementReason::Released, staff_id),
            )
            .await?;
        }
        StockMovement::record(
            tx,
            movement(order, item, -to_sell, MovementReason::Sale, staff_id),
        )
        .await?;
        *sold_now.entry(key).or_default() += to_sell;
        if item.owner_store_id != order.operating_store_id {
            book_consigned(tx, order, item, item.unit_price * to_sell, false).await?;
        }
    }
    Ok(())
}

/// Frees what the order still holds (step 3: expired or cancelled).
pub async fn release(tx: &mut Transaction, order: &Order, staff_id: Option<i64>) -> Result {
    for ((variant, owner, location), (reserved, _)) in held(tx, order.id).await? {
        if reserved > 0 {
            StockMovement::record(
                tx,
                StockMovement {
                    variant_id: variant,
                    owner_store_id: owner,
                    location_store_id: location,
                    quantity: reserved,
                    reason: MovementReason::Released,
                    reference_type: Some(Order::TABLE.into()),
                    reference_id: Some(order.id),
                    staff_id,
                    ..Default::default()
                },
            )
            .await?;
        }
    }
    Ok(())
}

/// A return: `quantity` of `item` back on the shelf at the selling store,
/// still its owner's; for consigned goods the books are reversed.
pub async fn take_back(
    tx: &mut Transaction,
    order: &Order,
    item: &OrderItem,
    quantity: i64,
    staff_id: Option<i64>,
    reason: &str,
) -> Result {
    let mut back = movement(order, item, quantity, MovementReason::Return, staff_id);
    back.note = Some(reason.to_owned());
    StockMovement::record(tx, back).await?;
    if item.owner_store_id != order.operating_store_id {
        book_consigned(tx, order, item, item.unit_price * quantity, true).await?;
    }
    Ok(())
}

/// The books for consigned goods sold (or returned, `reverse`) at the
/// order's store: written by the books between stores
/// (`multistore::books::consigned_sale`, the one place that writes
/// intercompany entries), in this sale's transaction.
async fn book_consigned(
    tx: &mut Transaction,
    order: &Order,
    item: &OrderItem,
    amount: i64,
    reverse: bool,
) -> Result {
    books::consigned_sale(
        tx,
        order.id,
        order.operating_store_id,
        item.owner_store_id,
        amount,
        reverse,
    )
    .await?;
    Ok(())
}
