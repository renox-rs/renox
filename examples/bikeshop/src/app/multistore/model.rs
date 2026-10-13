//! The books between stores (#245).
//!
//! Whenever the store that owns a bike or goods isn't the one that did the
//! work, an [`IntercompanyEntry`] records who owes whom, how much and why,
//! with a [`Morph`] link to the rental, order or work order ([`SOURCE`]).
//! The owner's rules (decisions 1–3 of #245):
//!
//! | Booking | Entry |
//! |---|---|
//! | rental of A's bike served by B | `rental_revenue`: B owes A the price; `operating_fee`: A owes B the fee (B's rate) |
//! | late or damage fee on A's bike, served by B | `late_fee` / `damage_fee`: B owes A the fee |
//! | sale at B of A's consigned goods | `sale_revenue`: B owes A the line; `selling_fee`: A owes B the fee |
//! | B's workshop repairs A's rental bike | `repair`: A owes B the work order's total |
//! | A's consigned goods missing at B's stock take | `consignment_loss`: B owes A their cost |
//!
//! The deposit stays with the store that served the customer; staff
//! helping another store are never charged. Entries are summed per store
//! pair into a monthly [`Settlement`], which both stores mark settled
//! (the owner, through a global role, may do it for either).
//!
//! [`super::books`] writes the entries (the one place that does);
//! `settlements.rs` nets them into the monthly statements.
//!
//! Migration: `migrations/20260101001000_create_intercompany_tables.*`.

use renox::chrono::NaiveDate;
use renox::db::relations::Morph;
use renox::prelude::*;
use serde::Serialize;

use crate::app::access::{StoreAttr, StoreRecord, catalogue};

/// Why one store owes another.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EntryKind {
    /// The rental price of the creditor's bike, collected by the debtor.
    #[default]
    RentalRevenue,
    /// The fee the operating store earns for renting out another's bike.
    OperatingFee,
    /// The price of the creditor's consigned goods, sold by the debtor.
    SaleRevenue,
    /// The fee the selling store earns for selling another's goods.
    SellingFee,
    /// A late fee on the creditor's bike.
    LateFee,
    /// A damage fee on the creditor's bike.
    DamageFee,
    /// A repair of the debtor's rental bike by the creditor's workshop.
    Repair,
    /// The creditor's consigned goods lost or damaged while the debtor
    /// held them (a stock take found fewer), at cost.
    ConsignmentLoss,
}

/// What an entry was booked for: `source_type` is `rentals`, `orders` or
/// `work_orders`, `source_id` the row.
pub const SOURCE: Morph = Morph::new("source_type", "source_id");

/// One line of the books between two stores: `debtor` owes `creditor`.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "intercompany_entries")]
pub struct IntercompanyEntry {
    pub id: i64,
    pub debtor_store_id: i64,
    pub creditor_store_id: i64,
    /// In the smallest unit of `APP_CURRENCY`, always positive.
    pub amount: i64,
    pub kind: EntryKind,
    /// For fees: the rate in force when it was booked (basis points),
    /// copied so a later change of rate doesn't rewrite history.
    pub fee_rate_bp: Option<i64>,
    pub source_type: String,
    pub source_id: i64,
    /// Set once the month it belongs to is settled.
    pub settlement_id: Option<i64>,
    pub booked_at: DateTime,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl StoreRecord for IntercompanyEntry {
    const VIEW: &'static str = catalogue::INTERCOMPANY_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["debtor_store_id", "creditor_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.creditor_store_id),
            StoreAttr::Location | StoreAttr::Operating => Some(self.debtor_store_id),
        }
    }
}

/// Where a settlement stands.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettlementStatus {
    /// Summed, not paid yet.
    #[default]
    Open,
    /// Paid between the stores: confirmed by both of them.
    Settled,
}

/// One month's net balance between two stores.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "settlements")]
pub struct Settlement {
    pub id: i64,
    pub debtor_store_id: i64,
    pub creditor_store_id: i64,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub amount: i64,
    pub status: SettlementStatus,
    pub settled_at: Option<DateTime>,
    /// The user whose confirmation completed it.
    pub settled_by: Option<i64>,
    /// When the paying store confirmed it paid (`intercompany.settle` there).
    pub debtor_confirmed_at: Option<DateTime>,
    pub debtor_confirmed_by: Option<i64>,
    /// When the store being paid confirmed it was paid.
    pub creditor_confirmed_at: Option<DateTime>,
    pub creditor_confirmed_by: Option<i64>,
    /// When the statement went out to both stores.
    pub mailed_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl Settlement {
    /// Whether the paying store has confirmed (a settled row counts as confirmed by both).
    pub fn debtor_confirmed(&self) -> bool {
        self.status == SettlementStatus::Settled || self.debtor_confirmed_at.is_some()
    }

    /// Whether the store being paid has confirmed.
    pub fn creditor_confirmed(&self) -> bool {
        self.status == SettlementStatus::Settled || self.creditor_confirmed_at.is_some()
    }
}

impl StoreRecord for Settlement {
    const VIEW: &'static str = catalogue::INTERCOMPANY_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["debtor_store_id", "creditor_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.creditor_store_id),
            StoreAttr::Location | StoreAttr::Operating => Some(self.debtor_store_id),
        }
    }
}
