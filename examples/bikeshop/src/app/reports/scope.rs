//! Whose numbers a report shows: which stores ([`Reach`]) and counted by
//! which store attribute ([`By`]).
//!
//! **Which stores.** The stores where the person holds `reports.view`
//! *now* (`AuthUser::scopes_with::<Store>`, #244): every store for the
//! owner's global role, the stores of their dated roles for a manager.
//! `?store=` narrows the page to one of them; anything else in it is
//! ignored (so another store's id shows the person's own stores, never
//! the other store's numbers).
//!
//! **Counted how.** A North bike rented out at South is North's asset and
//! South's work (#245). [`By::Books`] counts income in the books of the
//! store that owns the bike or the goods (`owner_store_id`); [`By::Work`]
//! counts it at the store that did the work (`operating_store_id`). Over
//! every store, both add up to the same company total.

use renox::auth::permissions::Scopes;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::access::catalogue;
use crate::app::staff::model::Store;

/// Counted in whose books, or where the work was done (`?by=books|work`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum By {
    /// The owner store of the bike or the goods: whose books.
    Books,
    /// The store that served the customer: who did the work.
    #[default]
    Work,
}

impl By {
    /// Reads `?by=`: `books` or `work` (the default for anything else).
    pub fn parse(text: Option<&str>) -> Self {
        match text {
            Some("books") => By::Books,
            _ => By::Work,
        }
    }

    /// The key in URLs and translations.
    pub fn key(self) -> &'static str {
        match self {
            By::Books => "books",
            By::Work => "work",
        }
    }

    /// The column of `report_revenue` (and of rentals) this counts by.
    pub fn store_column(self) -> &'static str {
        match self {
            By::Books => "owner_store_id",
            By::Work => "operating_store_id",
        }
    }

    /// The column of `rental_bikes` a fleet is counted by: the bikes a
    /// store owns, or the bikes standing at it.
    pub fn fleet_column(self) -> &'static str {
        match self {
            By::Books => "owner_store_id",
            By::Work => "location_store_id",
        }
    }
}

/// A store a person may report on.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct StoreRef {
    pub id: i64,
    pub name: String,
}

/// The stores a report covers.
#[derive(Serialize, Debug, Clone)]
pub struct Reach {
    /// Every store the person may report on, by name.
    pub stores: Vec<StoreRef>,
    /// The stores this page shows: all of [`Reach::stores`], or the one
    /// picked with `?store=`.
    pub chosen: Vec<i64>,
    /// The store picked with `?store=`, if any.
    pub store: Option<i64>,
    /// Whether the person sees every store (a global role).
    pub everywhere: bool,
}

impl Reach {
    /// The stores `user` may report on, narrowed to `picked` when it is one
    /// of them. One query (the stores' names).
    pub async fn of(db: &Db, user: &User, picked: Option<i64>) -> Result<Reach> {
        let scopes = user.scopes_with::<Store>(catalogue::REPORTS_VIEW);
        let everywhere = matches!(scopes, Scopes::All);
        let stores: Vec<StoreRef> = scopes
            .apply(Store::query(), &["id"])
            .order_by(Store::NAME)
            .get(db)
            .await?
            .into_iter()
            .map(|s| StoreRef {
                id: s.id,
                name: s.name,
            })
            .collect();
        Ok(Reach::from_stores(stores, picked, everywhere))
    }

    /// The reach over `stores`, narrowed to `picked` when it is one of them.
    pub fn from_stores(stores: Vec<StoreRef>, picked: Option<i64>, everywhere: bool) -> Reach {
        let store = picked.filter(|id| stores.iter().any(|s| s.id == *id));
        let chosen = match store {
            Some(id) => vec![id],
            None => stores.iter().map(|s| s.id).collect(),
        };
        Reach {
            stores,
            chosen,
            store,
            everywhere,
        }
    }

    /// Whether the person may report on no store at all.
    pub fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }

    /// Whether the page compares stores (more than one in view).
    pub fn compares(&self) -> bool {
        self.chosen.len() > 1
    }

    /// The name of store `id`.
    pub fn name_of(&self, id: i64) -> String {
        self.stores
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    }

    /// A short key for cache keys and file names: the chosen ids.
    pub fn key(&self) -> String {
        self.chosen
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join("-")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stores() -> Vec<StoreRef> {
        vec![
            StoreRef {
                id: 1,
                name: "North".into(),
            },
            StoreRef {
                id: 2,
                name: "South".into(),
            },
        ]
    }

    #[test]
    fn a_picked_store_must_be_one_of_the_persons() {
        assert_eq!(Reach::from_stores(stores(), Some(2), false).chosen, vec![2]);
        let other = Reach::from_stores(stores(), Some(3), false);
        assert_eq!(other.chosen, vec![1, 2]);
        assert_eq!(other.store, None);
        assert!(other.compares());
    }

    #[test]
    fn by_reads_books_and_work() {
        assert_eq!(By::parse(Some("books")), By::Books);
        assert_eq!(By::parse(Some("nonsense")), By::Work);
        assert_eq!(By::Books.store_column(), "owner_store_id");
    }
}
