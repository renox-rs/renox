//! The bike shop's areas, one folder each: `src/app/<area>/mod.rs` (the
//! module and its routes), `explain.rs` (its pages' "About this page"
//! entries), with views in `resources/views/<area>/` and tests in
//! `tests/<area>.rs`.
//!
//! A new area is added in three places, each in alphabetical order: the
//! `mod` lines below, the two lists in [`explanations`] and [`not_pages`],
//! and the module list in `src/lib.rs`.

use crate::explain::{Explanation, NotAPage};

// --- Areas (alphabetical; add new ones in order) ---
pub mod about;
pub mod access;
pub mod accounts;
pub mod api;
pub mod catalog;
pub mod home;
pub mod multistore;
pub mod plans;
pub mod rentals;
pub mod reports;
pub mod sales;
pub mod staff;
pub mod stock;
pub mod workshop;
// --- end of areas ---

/// Every area's "About this page" entries.
pub fn explanations() -> Vec<Explanation> {
    [
        // --- Areas (alphabetical) ---
        about::explain::entries(),
        access::explain::entries(),
        accounts::explain::entries(),
        api::explain::entries(),
        catalog::explain::entries(),
        home::explain::entries(),
        multistore::explain::entries(),
        plans::explain::entries(),
        rentals::explain::entries(),
        reports::explain::entries(),
        sales::explain::entries(),
        staff::explain::entries(),
        stock::explain::entries(),
        workshop::explain::entries(),
        // --- end of areas ---
    ]
    .concat()
}

/// Every area's GET routes that aren't pages.
pub fn not_pages() -> Vec<NotAPage> {
    [
        // --- Areas (alphabetical) ---
        about::explain::not_pages(),
        access::explain::not_pages(),
        accounts::explain::not_pages(),
        api::explain::not_pages(),
        catalog::explain::not_pages(),
        home::explain::not_pages(),
        multistore::explain::not_pages(),
        plans::explain::not_pages(),
        rentals::explain::not_pages(),
        reports::explain::not_pages(),
        sales::explain::not_pages(),
        staff::explain::not_pages(),
        stock::explain::not_pages(),
        workshop::explain::not_pages(),
        // --- end of areas ---
    ]
    .concat()
}
