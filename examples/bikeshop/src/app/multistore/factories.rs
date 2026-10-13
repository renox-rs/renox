//! Factories for the books between stores.

use renox::chrono::Duration;
use renox::db::FactoryBuilder;
use renox::prelude::*;

use super::model::{EntryKind, IntercompanyEntry, Settlement, SettlementStatus};
use crate::seed::today;

impl Factory for IntercompanyEntry {
    fn definition() -> Self {
        IntercompanyEntry {
            amount: 10_000,
            kind: EntryKind::RentalRevenue,
            booked_at: renox::db::now(),
            ..Default::default()
        }
    }
}

/// `IntercompanyEntry::factory()`.
pub fn intercompany_entries() -> FactoryBuilder<IntercompanyEntry> {
    IntercompanyEntry::factory()
}

/// States of an entry.
pub trait EntryStates {
    /// `debtor` owes `creditor` `amount` for `kind`.
    fn owing(self, debtor: i64, creditor: i64, amount: i64, kind: EntryKind) -> Self;
}

impl EntryStates for FactoryBuilder<IntercompanyEntry> {
    fn owing(self, debtor: i64, creditor: i64, amount: i64, kind: EntryKind) -> Self {
        self.state(move |e| {
            e.debtor_store_id = debtor;
            e.creditor_store_id = creditor;
            e.amount = amount;
            e.kind = kind;
        })
    }
}

impl Factory for Settlement {
    fn definition() -> Self {
        let end = today() - Duration::days(1);
        Settlement {
            period_start: end - Duration::days(29),
            period_end: end,
            status: SettlementStatus::Open,
            ..Default::default()
        }
    }
}

/// States of a settlement.
pub trait SettlementStates {
    /// Marked settled by `user_id`.
    fn settled_by(self, user_id: i64) -> Self;
}

impl SettlementStates for FactoryBuilder<Settlement> {
    fn settled_by(self, user_id: i64) -> Self {
        self.state(move |s| {
            s.status = SettlementStatus::Settled;
            s.settled_at = Some(renox::db::now());
            s.settled_by = Some(user_id);
        })
    }
}
