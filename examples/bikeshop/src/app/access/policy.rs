//! ABAC: each action is checked against the store attribute that matters.
//!
//! Records carry up to three store attributes ([`StoreAttr`]):
//!
//! | Attribute | Means | Checked for |
//! |---|---|---|
//! | **owner** | whose books the bike or goods are in | prices, retiring or selling a bike, recalling consigned goods, the asset's history |
//! | **location** | where the bike or goods are now | renting out, the counter, stock takes, repairs at the bench |
//! | **operating** | the store that served the customer | the rental or order itself: returns, refunds, fees earned |
//!
//! A record implements [`StoreRecord`] to say which stores it has. Then:
//!
//! - [`can_in`]: does the user hold `permission` in this store (a role there
//!   within its dates, or a global role)?
//! - [`require`]: for one record and one action. Someone who may not *see*
//!   the record in any of its stores gets a **404** (another store's record
//!   doesn't exist for them, so ids can't be probed); someone who sees it
//!   but may not do this action in the store that matters gets a **403**.
//! - [`visible`]: the list query "records I may see in any of their
//!   stores" (`owner_store_id IN (…) OR location_store_id IN (…)`), from
//!   `permissions::scopes_with` + `Scopes::apply` (#244). A global role
//!   sees everything.
//!
//! None of these check a role's name; the owner passes because their
//! global role grants every permission.
//!
//! ```no_run
//! use bikeshop::app::access::{self, StoreAttr, catalogue};
//! use bikeshop::app::rentals::model::RentalBike;
//! use renox::prelude::*;
//!
//! // Changing a bike's daily rate is the owner store's business…
//! async fn change_rate(State(db): State<Db>, user: AuthUser, Path(id): Path<i64>) -> Result<&'static str> {
//!     let bike = access::find::<RentalBike>(&db, &user, id).await?; // 404 unless seen
//!     access::require(&user, catalogue::PRICES_CHANGE, StoreAttr::Owner, &bike)?; // 403
//!     Ok("changed")
//! }
//!
//! // …while the bikes a counter lists are those at the store or owned by it.
//! async fn fleet(State(db): State<Db>) -> Result<String> {
//!     let bikes = access::visible::<RentalBike>(catalogue::FLEET_VIEW).get(&db).await?;
//!     Ok(format!("{} bikes", bikes.len()))
//! }
//! ```

use renox::auth::permissions::{self, Scope};
use renox::db::{Db, Model, Query};
use renox::prelude::*;
use std::future::Future;

use crate::app::staff::model::Store;

/// A store attribute of a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreAttr {
    /// Whose books it is in.
    Owner,
    /// Where it is now.
    Location,
    /// The store that served the customer.
    Operating,
}

/// A record that belongs to stores: rentals, bikes, stock, orders, work
/// orders, placements, shipments…
pub trait StoreRecord: Model {
    /// The permission that lets someone see this kind of record at all
    /// (`rentals.view`), in one of its stores.
    const VIEW: &'static str;
    /// The columns holding its stores, for filtering lists ([`visible`]).
    const STORE_COLUMNS: &'static [&'static str];

    /// The store for `attr`, or `None` when the record has no such
    /// attribute (an order has no location store).
    fn store_id(&self, attr: StoreAttr) -> Option<i64>;

    /// Every store the record belongs to, once each.
    fn store_ids(&self) -> Vec<i64> {
        let mut ids = Vec::new();
        for attr in [StoreAttr::Owner, StoreAttr::Location, StoreAttr::Operating] {
            if let Some(id) = self.store_id(attr)
                && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
        ids
    }
}

/// The scope of a store, for `has_permission_in` and `assign_role_in`.
pub fn store_scope(store_id: i64) -> Scope {
    Scope::of_id::<Store>(store_id)
}

/// Whether `user` holds `permission` in store `store_id` now: a role given
/// there and within its dates, or a global role. Answered from the roles
/// loaded for this request (no query).
pub fn can_in(user: &User, permission: &str, store_id: i64) -> bool {
    user.has_permission_in(permission, &store_scope(store_id))
}

/// Whether `user` may see `record`: [`StoreRecord::VIEW`] in any of its stores.
pub fn can_see<R: StoreRecord>(user: &User, record: &R) -> bool {
    record
        .store_ids()
        .into_iter()
        .any(|store| can_in(user, R::VIEW, store))
}

/// Whether `user` may do `permission` to `record`, checked in the store
/// that `attr` names (`false` when the record has no such store).
pub fn can<R: StoreRecord>(user: &User, permission: &str, attr: StoreAttr, record: &R) -> bool {
    record
        .store_id(attr)
        .is_some_and(|store| can_in(user, permission, store))
}

/// `Ok` when `user` may do `permission` to `record` in the store `attr`
/// names; a 404 when they may not even see the record, a 403 when they see
/// it but may not do this.
pub fn require<R: StoreRecord>(
    user: &User,
    permission: &str,
    attr: StoreAttr,
    record: &R,
) -> Result<()> {
    if !can_see(user, record) {
        return Err(Error::NotFound);
    }
    if !can(user, permission, attr, record) {
        return Err(Error::Forbidden);
    }
    Ok(())
}

/// The records of `M` the logged-in user may see with `permission` in any
/// of their stores: every one for a global role, none without a user (it
/// fails closed). Add filters, order and paging as usual.
pub fn visible<M: StoreRecord>(permission: &str) -> Query<M> {
    permissions::scopes_with::<Store>(permission).apply(M::query(), M::STORE_COLUMNS)
}

/// The record `id` of `M`, or a 404 when it doesn't exist or `user` may
/// not see it ([`StoreRecord::VIEW`] in none of its stores).
///
/// A plain `fn` returning a `Send` future (not an `async fn`), so handlers
/// that await it stay `Send` (Renox's docs/contributing/conventions.md §4.2: rustc can't prove it
/// for a generic async fn over an executor).
pub fn find<'a, M: StoreRecord<Key = i64> + Send + 'a>(
    db: &'a Db,
    user: &'a User,
    id: i64,
) -> impl Future<Output = Result<M>> + Send + 'a {
    let found = M::find_or_404(db, id);
    async move {
        let record = found.await?;
        if !can_see(user, &record) {
            return Err(Error::NotFound);
        }
        Ok(record)
    }
}
