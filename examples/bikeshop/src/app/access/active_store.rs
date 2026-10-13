//! The active store: which store's work someone is doing in this request.
//!
//! A cashier of North who helps South this week has two stores; what they
//! may do depends on which one they're working in. The staff side keeps
//! the choice in the session ([`SESSION_KEY`]) and, on every staff request,
//! [`middleware`]:
//!
//! 1. reads the stores where the user holds [`STAFF_ACCESS`] **now**
//!    (`AuthUser::scopes_with::<Store>`: roles given there, within their
//!    dates; every store for the owner's global role), from the roles the
//!    auth middleware already loaded (no query);
//! 2. keeps the session's choice if it is still one of them, else falls
//!    back to the person's home store, else the first of them (and
//!    remembers it);
//! 3. calls `permissions::set_scope(Scope::of(store))` (#244), so for the
//!    rest of the request `require_permission`, `allows` and `can()` in
//!    templates count the global roles plus the roles **in that store**,
//!    and stores the id in `renox::context` for [`current`].
//!
//! [`staff_routes`] wraps a module's staff routes with it, after
//! `require_permission(STAFF_ACCESS)` (so it runs first) and before
//! `require_auth` (so guests go to the login page).

use renox::auth::permissions::{self, Scopes};
use renox::axum::extract::Request;
use renox::axum::middleware::{Next, from_fn};
use renox::prelude::*;

use super::catalogue::STAFF_ACCESS;
use super::policy::store_scope;
use crate::app::staff::model::{Staff, Store};

/// The session key holding the chosen store's id.
pub const SESSION_KEY: &str = "active_store_id";

/// The active store of this request, in `renox::context`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveStore(pub i64);

/// The id of the store this request works in, once [`middleware`] ran;
/// `None` on public pages and for people with no staff role.
pub fn current() -> Option<i64> {
    renox::context::get::<ActiveStore>().map(|s| s.0)
}

/// The stores where `user` may work now (holds [`STAFF_ACCESS`]), by id;
/// `All` for a global role.
pub fn workable(user: &User) -> Scopes<i64> {
    user.scopes_with::<Store>(STAFF_ACCESS)
}

/// Guards a module's staff routes: logged in, working in a store they hold
/// [`STAFF_ACCESS`] in (picked by [`middleware`]). Add the routes'
/// own `require_permission(…)` before calling it, so they see the store.
///
/// ```
/// use bikeshop::app::access::{active_store::staff_routes, catalogue};
/// use renox::prelude::*;
///
/// let routes = staff_routes(
///     Routes::new()
///         .get("/staff/stock", || async { "stock" })
///         .name("stock.index")
///         .require_permission(catalogue::STOCK_VIEW),
/// );
/// # let _ = routes;
/// ```
// [explain:staff.dashboard.store]
pub fn staff_routes(routes: Routes) -> Routes {
    routes
        .require_permission(STAFF_ACCESS)
        .route_layer(from_fn(middleware))
        .require_auth()
}

/// Picks the active store (see the module docs). Never refuses: the guards
/// after it do.
pub async fn middleware(
    user: Option<AuthUser>,
    session: Session,
    req: Request,
    next: Next,
) -> Response {
    if let Some(user) = user {
        let chosen = session.get::<i64>(SESSION_KEY);
        let store = match chosen.filter(|id| workable(&user).contains(id)) {
            Some(id) => Some(id),
            None => {
                let picked = default_store(&user).await;
                if let Some(id) = picked {
                    let _ = session.put(SESSION_KEY, id);
                }
                picked
            }
        };
        if let Some(id) = store {
            permissions::set_scope(store_scope(id));
            renox::context::set(ActiveStore(id));
        }
    }
    next.run(req).await
}
// [/explain:staff.dashboard.store]

/// The store to start in: the home store when the user may work there,
/// else the first store they may work in (the first store at all for a
/// global role).
async fn default_store(user: &User) -> Option<i64> {
    let scopes = workable(user);
    if scopes.is_empty() {
        return None;
    }
    let state = renox::context::app()?;
    let home = Staff::of_user(&state.db, user.id)
        .await
        .ok()
        .flatten()
        .map(|s| s.home_store_id);
    if let Some(home) = home.filter(|id| scopes.contains(id)) {
        return Some(home);
    }
    match scopes {
        Scopes::Only(ids) => ids.first().copied(),
        Scopes::All => Store::query()
            .order_by(Store::ID)
            .first(&state.db)
            .await
            .ok()
            .flatten()
            .map(|s| s.id),
    }
}

/// What the store switcher (`components/store_switcher.html`) shows: the
/// active store and the stores the user may switch to. Shared with every
/// view as `store_switcher` (see [`super::Access`]); `None` for people
/// with no staff role, so customers' pages run no query for it.
#[derive(serde::Serialize, Debug, Clone)]
pub struct Switcher {
    /// The active store.
    pub current: Option<SwitcherStore>,
    /// Every store the user may work in now, by name.
    pub stores: Vec<SwitcherStore>,
}

/// A store in the switcher.
#[derive(serde::Serialize, Debug, Clone)]
pub struct SwitcherStore {
    pub id: i64,
    pub name: String,
    /// Names the store's photo (`public/images/site/store-{slug}.webp`) on
    /// the staff dashboard's welcome card.
    pub slug: String,
}

/// The switcher for the request being rendered.
pub async fn switcher(db: &Db) -> Result<Option<Switcher>> {
    let scopes = permissions::scopes_with::<Store>(STAFF_ACCESS);
    if scopes.is_empty() {
        return Ok(None);
    }
    let stores: Vec<SwitcherStore> = scopes
        .apply(Store::query(), &["id"])
        .order_by(Store::NAME)
        .get(db)
        .await?
        .into_iter()
        .map(|s| SwitcherStore {
            id: s.id,
            name: s.name,
            slug: s.slug,
        })
        .collect();
    let active = current();
    Ok(Some(Switcher {
        current: stores.iter().find(|s| Some(s.id) == active).cloned(),
        stores,
    }))
}

/// `POST /staff/store/{store}` (`access.store.switch`): works in that
/// store from now on, if the user may work there today (else a 403). Goes
/// back to the page it came from.
pub async fn switch(
    session: Session,
    user: AuthUser,
    back: Back,
    Path(store): Path<i64>,
) -> Result<Back> {
    if !workable(&user).contains(&store) {
        return Err(Error::Forbidden);
    }
    session.put(SESSION_KEY, store)?;
    Ok(back)
}
