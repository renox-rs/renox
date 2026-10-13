//! Access: who may do what, and in which store (RBAC + ABAC, #239, #245).
//!
//! - **RBAC, for *what*:** [`catalogue`] names every permission
//!   (`rentals.checkout`, `stock.adjust`…) and the roles that bundle them
//!   (owner, manager, cashier, mechanic, staff). Code checks permissions,
//!   never role names (`tests/access.rs` enforces it).
//! - **ABAC, for *where*:** roles are given in a store, with optional dates
//!   (Renox's `Permissions` module, #244: `assign_role_in(…).from(…).until(…)`).
//!   [`active_store`] picks the store a staff request works in
//!   (`permissions::set_scope`); [`policy`] checks one record against the
//!   store attribute that matters for the action (owner, location or
//!   operating store) and filters lists to "mine or at my store".
//!
//! The area has no page of its own: the roles page and staff management
//! are the staff area's (#239). Its one route is the store switcher's
//! `POST /staff/store/{store}`. It shares the switcher's data with every view.
//! Renox's `Permissions` module (the tables `roles`, `permissions`,
//! `permission_role`, `role_user` with scopes and dates, and the
//! `permissions:prune` command) is turned on in `src/lib.rs`.
//!
//! Made with `rnx make:module access`, then the files by hand.

pub mod active_store;
pub mod catalogue;
pub mod explain;
pub mod policy;

pub use active_store::staff_routes;
pub use policy::{StoreAttr, StoreRecord, can, can_in, can_see, find, require, visible};

use renox::prelude::*;

/// The access area, registered in `src/lib.rs`.
pub struct Access;

impl Module for Access {
    fn name(&self) -> &'static str {
        "access"
    }

    fn routes(&self) -> Routes {
        staff_routes(
            Routes::new()
                .post("/staff/store/{store}", active_store::switch)
                .name("access.store.switch"),
        )
    }

    fn register(&self, app: &mut Registry) {
        // The store switcher in the staff layout (`components/store_switcher.html`).
        app.share(
            "store_switcher",
            |ctx: renox::view::ViewContext| async move {
                active_store::switcher(&ctx.state.db).await
            },
        );
    }
}
