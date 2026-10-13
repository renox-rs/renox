//! The posting rules of the books between stores: **the one place** that
//! writes `intercompany_entries` (#245).
//!
//! Whenever the store that owns a bike or goods isn't the one that did the
//! work, the work is booked to the owner and the other store earns a fee
//! (or is paid for its work). Each function here takes one business event
//! and writes its entries, with the **fee rate in force copied onto the
//! entry** (`fee_rate_bp`), so changing a store's rate later never rewrites
//! history:
//!
//! | Event | Called from | Entries |
//! |---|---|---|
//! | consigned goods sold (or returned) at another store | `sales::ledger` (in the sale's transaction) | [`consigned_sale`]: `sale_revenue` seller → owner, `selling_fee` owner → seller |
//! | a rental closed ([`crate::app::rentals::RentalClosed`]) | the multistore module's listener | [`rental`]: `rental_revenue` and the late and damage fees → owner, `operating_fee` owner → operating store |
//! | a fleet repair collected ([`crate::app::workshop::status::WorkOrderClosed`]) | the multistore module's listener | [`repair`]: `repair` owner → the workshop's store |
//! | consigned goods missing at a stock take | `stock::take` (in the take's transaction) | [`consignment_loss`]: `consignment_loss` holder → owner, at cost |
//!
//! The **deposit** is never booked: it stays with the store that served the
//! customer. Fees taken out of the deposit were collected by that store;
//! what the customer paid on top at a return elsewhere was collected by
//! the store that took the bike back, so that store owes it.
//!
//! Staff helping another store are never charged (the owner's decision 2).

use renox::db::{Transaction, sql};
use renox::prelude::*;

use super::model::{EntryKind, IntercompanyEntry};
use crate::app::rentals::model::{DepositStatus, Rental};
use crate::app::staff::model::fee;
use crate::app::workshop::model::{WorkOrder, WorkSource};

/// One line to book: `debtor` owes `creditor` `amount` for `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posting {
    pub debtor: i64,
    pub creditor: i64,
    pub amount: i64,
    pub kind: EntryKind,
    /// The fee rate the amount was worked out with (basis points).
    pub rate_bp: Option<i64>,
}

impl Posting {
    /// `debtor` owes `creditor` `amount`.
    pub fn owe(debtor: i64, creditor: i64, amount: i64, kind: EntryKind) -> Self {
        Posting {
            debtor,
            creditor,
            amount,
            kind,
            rate_bp: None,
        }
    }

    /// A fee: `rate_bp` of `base`, rounded half up.
    pub fn fee(debtor: i64, creditor: i64, base: i64, rate_bp: i64, kind: EntryKind) -> Self {
        Posting {
            debtor,
            creditor,
            amount: fee(base, rate_bp),
            kind,
            rate_bp: Some(rate_bp),
        }
    }
}

/// Writes `postings` for `source` (`(table, id)`) in `tx`, skipping
/// nothing-to-book lines (zero amounts, a store owing itself). Returns what
/// was written.
pub async fn post(
    tx: &mut Transaction,
    source: (&str, i64),
    postings: &[Posting],
) -> Result<Vec<IntercompanyEntry>> {
    let now = renox::db::now();
    let mut written = Vec::new();
    for p in postings {
        if p.amount <= 0 || p.debtor == p.creditor {
            continue;
        }
        let entry = IntercompanyEntry {
            debtor_store_id: p.debtor,
            creditor_store_id: p.creditor,
            amount: p.amount,
            kind: p.kind,
            fee_rate_bp: p.rate_bp,
            source_type: source.0.to_owned(),
            source_id: source.1,
            booked_at: now,
            ..Default::default()
        };
        written.push(IntercompanyEntry::create(&mut *tx, entry).await?);
    }
    Ok(written)
}

/// The fee rate store `store_id` earns now (basis points), read in `tx` so
/// the rate copied onto an entry is the one in force when it is booked.
pub async fn rate_of(tx: &mut Transaction, store_id: i64) -> Result<i64> {
    Ok(sql("SELECT fee_rate_bp FROM stores WHERE id = ?")
        .bind(store_id)
        .scalar(&mut *tx)
        .await?)
}

/// Whether `source` was booked already (a listener that runs twice books once).
async fn booked(tx: &mut Transaction, source: (&str, i64)) -> Result<bool> {
    let count: i64 =
        sql("SELECT COUNT(*) FROM intercompany_entries WHERE source_type = ? AND source_id = ?")
            .bind(source.0)
            .bind(source.1)
            .scalar(&mut *tx)
            .await?;
    Ok(count > 0)
}

/// Consigned goods of `owner` sold at `seller` for `amount` (or taken back,
/// `reverse`), for order `order_id`, in the sale's transaction: the seller
/// owes the owner the line (`sale_revenue`), the owner owes the seller its
/// fee at the seller's rate (`selling_fee`). A return books both the other
/// way, at the rate in force at the return.
pub async fn consigned_sale(
    tx: &mut Transaction,
    order_id: i64,
    seller: i64,
    owner: i64,
    amount: i64,
    reverse: bool,
) -> Result<Vec<IntercompanyEntry>> {
    let rate = rate_of(tx, seller).await?;
    let (from, to) = if reverse {
        (owner, seller)
    } else {
        (seller, owner)
    };
    post(
        tx,
        ("orders", order_id),
        &[
            Posting::owe(from, to, amount, EntryKind::SaleRevenue),
            Posting::fee(to, from, amount, rate, EntryKind::SellingFee),
        ],
    )
    .await
}

/// `(taken from what's left, the rest)` of `amount`.
fn split(amount: i64, left: i64) -> (i64, i64) {
    let taken = amount.min(left.max(0));
    (taken, amount - taken)
}

/// The postings of a closed rental (see the module docs), without writing
/// them: also what the receipt and the tests read.
pub fn rental_postings(rental: &Rental, operating_rate_bp: i64) -> Vec<Posting> {
    let owner = rental.owner_store_id;
    let operating = rental.operating_store_id;
    let took_back = rental.return_store_id.unwrap_or(operating);
    let held = if matches!(
        rental.deposit_status,
        DepositStatus::Held | DepositStatus::Settled
    ) {
        rental.deposit
    } else {
        0
    };
    let (late_from_deposit, late_rest) = split(rental.late_fee, held);
    let (damage_from_deposit, damage_rest) = split(rental.damage_fee, held - late_from_deposit);
    vec![
        Posting::owe(operating, owner, rental.price, EntryKind::RentalRevenue),
        Posting::fee(
            owner,
            operating,
            rental.price,
            operating_rate_bp,
            EntryKind::OperatingFee,
        ),
        Posting::owe(operating, owner, late_from_deposit, EntryKind::LateFee),
        Posting::owe(took_back, owner, late_rest, EntryKind::LateFee),
        Posting::owe(operating, owner, damage_from_deposit, EntryKind::DamageFee),
        Posting::owe(took_back, owner, damage_rest, EntryKind::DamageFee),
    ]
}

/// Books a closed rental (on [`crate::app::rentals::RentalClosed`]): nothing
/// when the owner store served it itself, else the postings of
/// [`rental_postings`]. Safe to run twice.
pub async fn rental(db: &Db, rental_id: i64) -> Result<Vec<IntercompanyEntry>> {
    let Some(rental) = Rental::find(db, rental_id).await? else {
        return Ok(Vec::new());
    };
    let source = (Rental::TABLE, rental.id);
    let mut tx = db.begin().await?;
    if booked(&mut tx, source).await? {
        return Ok(Vec::new());
    }
    let rate = rate_of(&mut tx, rental.operating_store_id).await?;
    let written = post(&mut tx, source, &rental_postings(&rental, rate)).await?;
    tx.commit().await?;
    Ok(written)
}

/// Books a collected work order (on
/// [`crate::app::workshop::status::WorkOrderClosed`]): a repair of a rental
/// bike billed to its owner store, done by another store's workshop, is
/// owed by the owner to that store. Safe to run twice.
pub async fn repair(db: &Db, work_order_id: i64) -> Result<Vec<IntercompanyEntry>> {
    let Some(order) = WorkOrder::find(db, work_order_id).await? else {
        return Ok(Vec::new());
    };
    let Some(owner) = order.billed_store_id else {
        return Ok(Vec::new());
    };
    if order.source != WorkSource::Fleet || owner == order.store_id {
        return Ok(Vec::new());
    }
    let source = (WorkOrder::TABLE, order.id);
    let mut tx = db.begin().await?;
    if booked(&mut tx, source).await? {
        return Ok(Vec::new());
    }
    let written = post(
        &mut tx,
        source,
        &[Posting::owe(
            owner,
            order.store_id,
            order.total,
            EntryKind::Repair,
        )],
    )
    .await?;
    tx.commit().await?;
    Ok(written)
}

/// `owner`'s consigned goods worth `cost` went missing (or broke) at
/// `holder`, found by the stock-take adjustment `movement_id`: the holder
/// owes the owner their cost.
pub async fn consignment_loss(
    tx: &mut Transaction,
    movement_id: i64,
    holder: i64,
    owner: i64,
    cost: i64,
) -> Result<Vec<IntercompanyEntry>> {
    post(
        tx,
        ("stock_movements", movement_id),
        &[Posting::owe(
            holder,
            owner,
            cost,
            EntryKind::ConsignmentLoss,
        )],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rental(price: i64, deposit: i64, late: i64, damage: i64) -> Rental {
        Rental {
            owner_store_id: 1,
            operating_store_id: 2,
            return_store_id: Some(3),
            price,
            deposit,
            late_fee: late,
            damage_fee: damage,
            deposit_status: DepositStatus::Settled,
            ..Default::default()
        }
    }

    fn owed(postings: &[Posting], debtor: i64, kind: EntryKind) -> i64 {
        postings
            .iter()
            .filter(|p| p.debtor == debtor && p.kind == kind)
            .map(|p| p.amount)
            .sum()
    }

    #[test]
    fn fees_beyond_the_deposit_are_owed_by_the_store_that_took_them() {
        let p = rental_postings(&rental(90_000, 100_000, 60_000, 200_000), 2_000);
        assert_eq!(owed(&p, 2, EntryKind::RentalRevenue), 90_000);
        assert_eq!(owed(&p, 1, EntryKind::OperatingFee), 18_000);
        // The deposit covered the late fee and 40,000 of the damage.
        assert_eq!(owed(&p, 2, EntryKind::LateFee), 60_000);
        assert_eq!(owed(&p, 2, EntryKind::DamageFee), 40_000);
        assert_eq!(owed(&p, 3, EntryKind::DamageFee), 160_000);
    }
}
