//! Roles and permissions, opt in with `App::module(Permissions)`.
//!
//! A role (`editor`) grants permissions (`posts.publish`); users have roles.
//! Define roles once (a seeder or a command), assign them to users, then
//! check in handlers, routes and templates:
//!
//! ```
//! use renox::prelude::*;
//! use renox::auth::{Permissions, permissions};
//!
//! fn app() -> App {
//!     App::new()
//!         .module(Auth::new())
//!         .module(Permissions)
//!         .gate_before(|user, _ability| {
//!             // Super-admins may do everything (a column the app added).
//!             (user.get::<bool>("is_super_admin") == Some(true)).then_some(true)
//!         })
//! }
//!
//! async fn setup(db: &Db, user: &User) -> Result {
//!     permissions::define_role(db, "editor", &["posts.create", "posts.publish"]).await?;
//!     user.assign_role(db, "editor").await?;
//!     Ok(())
//! }
//!
//! async fn publish(user: AuthUser) -> Result<&'static str> {
//!     if !user.allows("posts.publish") { // gate_before, a gate, then permissions
//!         return Err(Error::Forbidden);
//!     }
//!     let _editor = user.has_role("editor");
//!     Ok("published")
//! }
//!
//! fn routes() -> Routes {
//!     Routes::new()
//!         .post("/posts/{id}/publish", publish)
//!         .require_permission("posts.publish") // 403 otherwise, shown in route:list
//! }
//! # let _ = (app, routes);
//! ```
//!
//! In templates, `can('posts.publish')` and `auth.roles` work the same way.
//! A user's roles and permissions are loaded once per request.
//!
//! # Roles in one record
//!
//! A role can also be given in one record only (a store, a branch, a team)
//! and for a period: [`User::assign_role_in`] with a [`Scope`]. A role
//! means the same everywhere (its permissions are global); only who has it
//! where changes. Each request picks its scope with [`set_scope`] (an app
//! middleware, like the current team of a multi-tenant app); then every
//! check above counts the global roles plus the roles in that scope that
//! are within their dates:
//!
//! ```
//! use renox::prelude::*;
//! use renox::auth::permissions::{self, Scope};
//! use renox::axum::{extract::Request, middleware::{Next, from_fn}};
//!
//! #[derive(Model, serde::Serialize, Default)]
//! #[model(table = "stores")]
//! struct Store { id: i64, name: String }
//!
//! async fn setup(db: &Db, user: &User, store: &Store) -> Result {
//!     permissions::define_role(db, "manager", &["orders.refund"]).await?;
//!     let until = renox::db::now() + renox::chrono::Duration::days(30);
//!     user.assign_role_in(db, "manager", &Scope::of(store)).until(until).await?;
//!     Ok(())
//! }
//!
//! /// Works in the store the session says (checked against the user's stores).
//! async fn pick_store(session: Session, req: Request, next: Next) -> Response {
//!     if let Some(store) = session.get::<i64>("store_id") {
//!         permissions::set_scope(Scope::of_id::<Store>(store));
//!     }
//!     next.run(req).await
//! }
//!
//! fn app() -> App {
//!     App::new()
//!         .module(Auth::new())
//!         .module(permissions::Permissions)
//!         .layer(from_fn(pick_store))
//! }
//! # let _ = app;
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::{Future, IntoFuture};
use std::pin::Pin;

use super::User;
use crate::db::{DateTime, Db, Migration, Model, Query, ToDbValue, now, sql};
use crate::{Module, Registry, Result, Routes};

const MIGRATIONS: &[Migration] = &[
    crate::db::framework_migration!(
        "permissions",
        "00010101000500_create_roles_and_permissions_tables"
    ),
    Migration::new(
        "00010101000510_add_scope_to_role_user",
        include_str!("../../migrations/permissions/00010101000510_add_scope_to_role_user.up.sql"),
        Some(include_str!(
            "../../migrations/permissions/00010101000510_add_scope_to_role_user.down.sql"
        )),
    )
    .postgres(
        include_str!(
            "../../migrations/permissions/00010101000510_add_scope_to_role_user.postgres.up.sql"
        ),
        Some(include_str!(
            "../../migrations/permissions/00010101000510_add_scope_to_role_user.postgres.down.sql"
        )),
    ),
];

/// Adds roles and permissions to the app: their tables, loading the
/// current user's grants for `AuthUser::has_role` / `has_permission`,
/// `allows`, `Routes::require_role` / `require_permission` and `can()` in
/// templates, and the `permissions:prune` command. Needs the `Auth`
/// module's `users` table.
pub struct Permissions;

impl Module for Permissions {
    fn name(&self) -> &'static str {
        "permissions"
    }

    fn migrations(&self) -> &'static [Migration] {
        MIGRATIONS
    }

    fn routes(&self) -> Routes {
        Routes::new()
    }

    fn register(&self, app: &mut Registry) {
        app.permissions = true;
        // Ended assignments are already ignored; this keeps the table small.
        app.command(
            "permissions:prune",
            "Delete role assignments that ended more than --days ago (default 30)",
            |args, state| async move {
                let days: u64 = args
                    .value("--days")
                    .unwrap_or("30")
                    .parse()
                    .map_err(|_| anyhow::anyhow!("--days takes a number of days"))?;
                let age = std::time::Duration::from_secs(days * 24 * 60 * 60);
                let pruned = prune_ended_assignments(&state.db, age).await?;
                println!("Deleted {pruned} ended role assignments.");
                Ok(())
            },
        );
    }
}

/// Where a role is given: everywhere ([`Scope::global`]) or in one record,
/// named by its model's table and its key (`Scope::of(&store)`,
/// `Scope::of_id::<Store>(7)`). It is stored as two text columns of
/// `role_user`, `scope_type` (the table) and `scope_id` (the key).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize)]
pub struct Scope {
    kind: String,
    id: String,
}

impl Scope {
    /// Everywhere: a role given this way counts in every scope, as
    /// `assign_role` does.
    pub fn global() -> Self {
        Self::default()
    }

    /// The record `model` (its table and its key).
    pub fn of<M: Model>(model: &M) -> Self {
        Self::of_id::<M>(model.id())
    }

    /// The record of model `M` with the key `id`.
    pub fn of_id<M: Model>(id: M::Key) -> Self {
        Self::new(M::TABLE, id)
    }

    /// A scope by name, for records that aren't models: `kind` is usually
    /// a table name, `id` the record's key.
    pub fn new(kind: &str, id: impl fmt::Display) -> Self {
        Self {
            kind: kind.to_owned(),
            id: id.to_string(),
        }
    }

    /// Whether this is [`Scope::global`].
    pub fn is_global(&self) -> bool {
        self.kind.is_empty() && self.id.is_empty()
    }

    /// The record's kind: its model's table (`""` when global).
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The record's key as text (`""` when global).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The record's key, when this scope is a record of model `M`.
    pub fn key<M: Model>(&self) -> Option<M::Key> {
        (self.kind == M::TABLE)
            .then(|| self.id.parse().ok())
            .flatten()
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_global() {
            f.write_str("global")
        } else {
            write!(f, "{}:{}", self.kind, self.id)
        }
    }
}

/// The records in which a permission is granted, from
/// [`User::scopes_with`] / [`scopes_with`]: every one (a global role grants
/// it) or only these keys. Narrow a list with [`Scopes::apply`], e.g. in a
/// `#[model(default_scope)]`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Scopes<K> {
    /// A global role grants the permission: every record.
    All,
    /// Only the records with these keys (none when empty).
    Only(Vec<K>),
}

impl<K> Scopes<K> {
    /// Whether the record with key `key` is among them.
    pub fn contains(&self, key: &K) -> bool
    where
        K: PartialEq,
    {
        match self {
            Scopes::All => true,
            Scopes::Only(keys) => keys.contains(key),
        }
    }

    /// Whether no record at all is granted.
    pub fn is_empty(&self) -> bool {
        matches!(self, Scopes::Only(keys) if keys.is_empty())
    }

    /// Keeps the rows of `query` whose value in any of `columns` is one of
    /// the keys (`owner_store_id IN (…) OR location_store_id IN (…)`);
    /// [`Scopes::All`] keeps every row, and no key (or no column) none.
    ///
    /// ```
    /// use renox::prelude::*;
    /// use renox::auth::permissions;
    ///
    /// #[derive(Model, serde::Serialize, Default)]
    /// #[model(table = "stores")]
    /// struct Store { id: i64 }
    ///
    /// /// A transfer is seen from the store it leaves and the one it goes to.
    /// #[derive(Model, serde::Serialize, Default)]
    /// #[model(table = "transfers", default_scope = "my_stores")]
    /// struct Transfer { id: i64, from_store_id: i64, to_store_id: i64 }
    ///
    /// fn my_stores(query: renox::db::Query<Transfer>) -> renox::db::Query<Transfer> {
    ///     permissions::scopes_with::<Store>("transfers.view")
    ///         .apply(query, &["from_store_id", "to_store_id"])
    /// }
    /// ```
    pub fn apply<M: Model>(&self, query: Query<M>, columns: &[&str]) -> Query<M>
    where
        K: ToDbValue + Clone,
    {
        match self {
            Scopes::All => query,
            Scopes::Only(keys) if keys.is_empty() || columns.is_empty() => query.none(),
            Scopes::Only(keys) => query.where_any(|mut group| {
                for column in columns {
                    group = group.where_in(column, keys.iter().cloned());
                }
                group
            }),
        }
    }
}

/// One role given to a user, from [`User::assignments`]: where and when.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[non_exhaustive]
pub struct Assignment {
    /// The role's name.
    pub role: String,
    /// Where it counts ([`Scope::global`] for everywhere).
    pub scope: Scope,
    /// When it starts counting; `None` from the start.
    pub starts_at: Option<DateTime>,
    /// When it stops counting; `None` for good.
    pub ends_at: Option<DateTime>,
}

impl Assignment {
    /// Whether it counts now (`starts_at <= now < ends_at`).
    pub fn is_active(&self) -> bool {
        self.is_active_at(now())
    }

    /// Whether it counts at `at`.
    pub fn is_active_at(&self, at: DateTime) -> bool {
        self.starts_at.is_none_or(|start| start <= at) && self.ends_at.is_none_or(|end| at < end)
    }
}

/// The scope a request works in (`set_scope`), in `renox::context`.
#[derive(Clone)]
struct ActiveScope(Scope);

/// Makes `scope` the one this request (job, command) works in: from now on,
/// `has_role`, `has_permission`, `allows`, the route guards and `can()` in
/// templates count the user's global roles plus the roles given in
/// `scope`. Call it from a middleware after checking the user may work
/// there, like the current team in docs/authorization.md. Route guards
/// see it when the middleware runs before them: an `App::layer`, or a
/// route layer added after the guard.
pub fn set_scope(scope: Scope) {
    crate::context::set(ActiveScope(scope));
}

/// The scope set with [`set_scope`] for this request, if any.
pub fn active_scope() -> Option<Scope> {
    crate::context::get::<ActiveScope>().map(|active| active.0)
}

/// Back to global roles only for the rest of this request.
pub fn clear_scope() {
    crate::context::remove::<ActiveScope>();
}

/// The records of model `M` in which the logged-in user of this request
/// holds `permission` (see [`User::scopes_with`]); none without a
/// logged-in user. Made for a `#[model(default_scope)]`, which gets no
/// user: see [`Scopes::apply`].
pub fn scopes_with<M: Model>(permission: &str) -> Scopes<M::Key> {
    match super::current_user_id().and_then(super::current_grants) {
        Some(grants) => grants.scopes_with::<M>(permission),
        None => Scopes::Only(Vec::new()),
    }
}

/// Creates the role `name` if it's new, and makes it grant exactly
/// `permissions` (creating the ones that don't exist yet).
pub async fn define_role(db: &Db, name: &str, permissions: &[&str]) -> Result {
    let mut tx = db.begin().await?;
    let role = ensure(&mut tx, "roles", name).await?;
    sql("DELETE FROM permission_role WHERE role_id = ?")
        .bind(role)
        .execute(&mut tx)
        .await?;
    for permission in permissions {
        let permission = ensure(&mut tx, "permissions", permission).await?;
        link(&mut tx, permission, role).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Adds `permissions` to the existing role `name`.
pub async fn grant(db: &Db, role: &str, permissions: &[&str]) -> Result {
    let mut tx = db.begin().await?;
    let role = existing_role(&mut tx, role).await?;
    for permission in permissions {
        let permission = ensure(&mut tx, "permissions", permission).await?;
        link(&mut tx, permission, role).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Removes `permissions` from the role `name`.
pub async fn revoke(db: &Db, role: &str, permissions: &[&str]) -> Result {
    let mut tx = db.begin().await?;
    let role = existing_role(&mut tx, role).await?;
    for permission in permissions {
        sql(
            "DELETE FROM permission_role WHERE role_id = ? AND permission_id = \
             (SELECT id FROM permissions WHERE name = ?)",
        )
        .bind(role)
        .bind(*permission)
        .execute(&mut tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Deletes the role `name`; users lose it. Returns whether it existed.
pub async fn delete_role(db: &Db, name: &str) -> Result<bool> {
    Ok(sql("DELETE FROM roles WHERE name = ?")
        .bind(name)
        .execute(db)
        .await?
        > 0)
}

/// Every role, by name, with the permissions it grants.
pub async fn roles(db: &Db) -> Result<Vec<(String, Vec<String>)>> {
    let rows: Vec<(String, Option<String>)> = sql("SELECT r.name, p.name FROM roles r \
         LEFT JOIN permission_role pr ON pr.role_id = r.id \
         LEFT JOIN permissions p ON p.id = pr.permission_id \
         ORDER BY r.name, p.name")
    .fetch_as(db)
    .await?;
    let mut roles: Vec<(String, Vec<String>)> = Vec::new();
    for (role, permission) in rows {
        if roles.last().is_none_or(|(name, _)| *name != role) {
            roles.push((role, Vec::new()));
        }
        if let (Some(permission), Some((_, list))) = (permission, roles.last_mut()) {
            list.push(permission);
        }
    }
    Ok(roles)
}

/// Deletes role assignments that ended more than `age` ago (they stopped
/// counting when they ended), and returns how many. `rnx permissions:prune`
/// runs it.
pub async fn prune_ended_assignments(db: &Db, age: std::time::Duration) -> Result<u64> {
    let age = chrono::Duration::from_std(age).unwrap_or(chrono::Duration::MAX);
    let cut_off = now()
        .checked_sub_signed(age)
        .unwrap_or(chrono::DateTime::<chrono::Utc>::MIN_UTC);
    Ok(
        sql("DELETE FROM role_user WHERE ends_at IS NOT NULL AND ends_at < ?")
            .bind(cut_off)
            .execute(db)
            .await?,
    )
}

/// Gives a user a role in a scope, from [`User::assign_role_in`]; set its
/// dates with [`from`](AssignRole::from) / [`until`](AssignRole::until),
/// then `.await` it. Assigning the same role in the same scope again
/// replaces the dates.
#[must_use = "an assignment does nothing until it is awaited"]
pub struct AssignRole<'a> {
    db: &'a Db,
    user_id: i64,
    role: &'a str,
    scope: Scope,
    starts_at: Option<DateTime>,
    ends_at: Option<DateTime>,
}

impl AssignRole<'_> {
    /// The role counts from `at` on (before, it is ignored).
    pub fn from(mut self, at: DateTime) -> Self {
        self.starts_at = Some(at);
        self
    }

    /// The role stops counting at `at`.
    pub fn until(mut self, at: DateTime) -> Self {
        self.ends_at = Some(at);
        self
    }
}

impl<'a> IntoFuture for AssignRole<'a> {
    type Output = Result;
    type IntoFuture = Pin<Box<dyn Future<Output = Result> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if let (Some(start), Some(end)) = (self.starts_at, self.ends_at)
                && end <= start
            {
                return Err(
                    anyhow::anyhow!("the role `{}` would end before it starts", self.role).into(),
                );
            }
            assign(
                self.db,
                self.user_id,
                self.role,
                &self.scope,
                self.starts_at,
                self.ends_at,
            )
            .await
        })
    }
}

/// Gives `role` in `scope` with these dates, replacing the dates of the
/// same assignment.
async fn assign(
    db: &Db,
    user_id: i64,
    role: &str,
    scope: &Scope,
    starts_at: Option<DateTime>,
    ends_at: Option<DateTime>,
) -> Result {
    let mut tx = db.begin().await?;
    let role = existing_role(&mut tx, role).await?;
    sql(
        "INSERT INTO role_user (role_id, user_id, scope_type, scope_id, starts_at, ends_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT (role_id, user_id, scope_type, scope_id) \
         DO UPDATE SET starts_at = excluded.starts_at, ends_at = excluded.ends_at",
    )
    .bind(role)
    .bind(user_id)
    .bind(scope.kind())
    .bind(scope.id())
    .bind(starts_at)
    .bind(ends_at)
    .execute(&mut tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

impl User {
    /// Gives the user the existing role `role` (see
    /// `permissions::define_role`) everywhere and for good.
    pub async fn assign_role(&self, db: &Db, role: &str) -> Result {
        assign(db, self.id, role, &Scope::global(), None, None).await
    }

    /// Gives the user the existing role `role` in `scope` (a store, a
    /// team), optionally from / until a date; `.await` it:
    /// `user.assign_role_in(&db, "manager", &Scope::of(&store)).until(end).await?`.
    /// It counts when that scope is the request's ([`set_scope`]) and for
    /// [`User::has_permission_in`] on that scope. With
    /// [`Scope::global`], it is a global role with dates.
    pub fn assign_role_in<'a>(&self, db: &'a Db, role: &'a str, scope: &Scope) -> AssignRole<'a> {
        AssignRole {
            db,
            user_id: self.id,
            role,
            scope: scope.clone(),
            starts_at: None,
            ends_at: None,
        }
    }

    /// Takes the global role `role` away from the user (roles given in a
    /// scope stay; see [`User::remove_role_in`]).
    pub async fn remove_role(&self, db: &Db, role: &str) -> Result {
        self.remove_role_in(db, role, &Scope::global()).await
    }

    /// Takes the role `role` in `scope` away from the user.
    pub async fn remove_role_in(&self, db: &Db, role: &str, scope: &Scope) -> Result {
        sql(
            "DELETE FROM role_user WHERE user_id = ? AND scope_type = ? AND scope_id = ? \
             AND role_id = (SELECT id FROM roles WHERE name = ?)",
        )
        .bind(self.id)
        .bind(scope.kind())
        .bind(scope.id())
        .bind(role)
        .execute(db)
        .await?;
        Ok(())
    }

    /// Makes the user's global roles exactly `roles` (each must exist);
    /// roles given in a scope stay.
    pub async fn sync_roles(&self, db: &Db, roles: &[&str]) -> Result {
        self.sync_roles_in(db, roles, &Scope::global()).await
    }

    /// Makes the user's roles in `scope` exactly `roles` (each must exist),
    /// with no dates; other scopes stay.
    pub async fn sync_roles_in(&self, db: &Db, roles: &[&str], scope: &Scope) -> Result {
        let mut tx = db.begin().await?;
        let mut ids = Vec::new();
        for role in roles {
            ids.push(existing_role(&mut tx, role).await?);
        }
        sql("DELETE FROM role_user WHERE user_id = ? AND scope_type = ? AND scope_id = ?")
            .bind(self.id)
            .bind(scope.kind())
            .bind(scope.id())
            .execute(&mut tx)
            .await?;
        for role in ids {
            sql(
                "INSERT INTO role_user (role_id, user_id, scope_type, scope_id) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(role)
            .bind(self.id)
            .bind(scope.kind())
            .bind(scope.id())
            .execute(&mut tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The names of the user's roles in effect now, sorted: the global ones
    /// plus those in the request's scope ([`set_scope`]), within their
    /// dates.
    pub async fn roles(&self, db: &Db) -> Result<Vec<String>> {
        Ok(grants(db, self.id).await?.roles())
    }

    /// The permissions the user's roles in effect now grant, sorted (the
    /// same roles as [`User::roles`]).
    pub async fn permissions(&self, db: &Db) -> Result<Vec<String>> {
        let grants = grants(db, self.id).await?;
        let scope = active_scope();
        let mut list: Vec<String> = grants
            .permissions_in(scope.as_ref())
            .into_iter()
            .map(str::to_owned)
            .collect();
        list.sort();
        Ok(list)
    }

    /// Every role the user was given, with where and when, ordered by role
    /// and scope; ended ones too until `permissions:prune` deletes them.
    /// For account and admin pages.
    pub async fn assignments(&self, db: &Db) -> Result<Vec<Assignment>> {
        Ok(load_assignments(db, self.id, false)
            .await?
            .into_iter()
            .map(|granted| Assignment {
                role: granted.role,
                scope: granted.scope,
                starts_at: granted.starts_at,
                ends_at: granted.ends_at,
            })
            .collect())
    }

    /// Whether this user has `role` globally or in `scope`, within its
    /// dates, from the roles loaded for the current request (like
    /// [`User::has_role`]); `scope` is the record's, not the request's.
    pub fn has_role_in(&self, role: &str, scope: &Scope) -> bool {
        super::current_grants(self.id).is_some_and(|g| g.has_role_in(role, Some(scope)))
    }

    /// Whether a global role of this user, or one given in `scope`, grants
    /// `permission` now: for policies, which check the record's scope
    /// (`Scope::of_id::<Store>(order.store_id)`) rather than the request's.
    /// Answered from the roles loaded for the current request (like
    /// [`User::has_permission`]): `false` for another user and outside a
    /// request.
    pub fn has_permission_in(&self, permission: &str, scope: &Scope) -> bool {
        super::current_grants(self.id).is_some_and(|g| g.has_permission_in(permission, Some(scope)))
    }

    /// The records of model `M` in which this user holds `permission` now:
    /// [`Scopes::All`] when a global role grants it, else the keys of the
    /// records whose roles do. For filtering lists (`Scopes::apply`).
    /// Answered from the roles loaded for the current request; nothing for
    /// another user and outside a request.
    pub fn scopes_with<M: Model>(&self, permission: &str) -> Scopes<M::Key> {
        match super::current_grants(self.id) {
            Some(grants) => grants.scopes_with::<M>(permission),
            None => Scopes::Only(Vec::new()),
        }
    }
}

/// The users who have the global role `role` now, ordered by id (e.g. to
/// notify every admin).
pub async fn users_with_role(db: &Db, role: &str) -> Result<Vec<User>> {
    users_with_role_in(db, role, &Scope::global()).await
}

/// The users who have `role` in `scope` now (given there, or globally),
/// ordered by id, e.g. to notify the managers of one store.
pub async fn users_with_role_in(db: &Db, role: &str, scope: &Scope) -> Result<Vec<User>> {
    users_granted(
        db,
        "JOIN roles r ON r.id = ru.role_id WHERE r.name = ?",
        role,
        scope,
    )
    .await
}

/// The users a global role lets do `permission` now, ordered by id (e.g. to
/// notify everyone who may approve refunds).
pub async fn users_with_permission(db: &Db, permission: &str) -> Result<Vec<User>> {
    users_with_permission_in(db, permission, &Scope::global()).await
}

/// The users who may do `permission` in `scope` now: through a role given
/// there or a global one, within its dates, ordered by id. The question a
/// job asks to notify "everyone who may take overdue bikes back in this
/// store", without naming roles.
///
/// ```
/// # use renox::prelude::*;
/// # use renox::auth::permissions::{self, Scope};
/// # #[derive(Model, serde::Serialize)] struct Store { id: i64 }
/// # async fn demo(db: &renox::db::Db, store: &Store) -> Result {
/// let staff = permissions::users_with_permission_in(db, "rentals.return", &Scope::of(store)).await?;
/// # Ok(()) }
/// ```
pub async fn users_with_permission_in(
    db: &Db,
    permission: &str,
    scope: &Scope,
) -> Result<Vec<User>> {
    users_granted(
        db,
        "JOIN permission_role pr ON pr.role_id = ru.role_id \
         JOIN permissions p ON p.id = pr.permission_id WHERE p.name = ?",
        permission,
        scope,
    )
    .await
}

/// The users whose assignment, joined by `join` (which ends in a `WHERE`
/// on `name`), is in effect in `scope` (or global) now.
async fn users_granted(db: &Db, join: &str, name: &str, scope: &Scope) -> Result<Vec<User>> {
    let at = now();
    let ids: Vec<i64> = sql(format!(
        "SELECT DISTINCT ru.user_id FROM role_user ru {join} \
         AND ((ru.scope_type = '' AND ru.scope_id = '') OR (ru.scope_type = ? AND ru.scope_id = ?)) \
         AND (ru.starts_at IS NULL OR ru.starts_at <= ?) \
         AND (ru.ends_at IS NULL OR ru.ends_at > ?) \
         ORDER BY ru.user_id"
    ))
    .bind(name)
    .bind(scope.kind())
    .bind(scope.id())
    .bind(at)
    .bind(at)
    .scalars(db)
    .await?;
    let mut users = User::find_many(db, ids).await?;
    users.sort_by_key(|u| u.id);
    Ok(users)
}

/// One role given to the current user, as the auth middleware loads it.
#[derive(Debug, Clone)]
pub(crate) struct Granted {
    role: String,
    scope: Scope,
    starts_at: Option<DateTime>,
    ends_at: Option<DateTime>,
}

impl Granted {
    fn active_at(&self, at: DateTime) -> bool {
        self.starts_at.is_none_or(|start| start <= at) && self.ends_at.is_none_or(|end| at < end)
    }
}

/// A user's role assignments (with their scopes and dates) and what each
/// role grants, loaded once per request; every check filters them by scope
/// and the current time, since the app's scope middleware runs after the
/// auth middleware.
#[derive(Default, Debug)]
pub(crate) struct Grants {
    assignments: Vec<Granted>,
    permissions: HashMap<String, HashSet<String>>,
}

impl Grants {
    /// The assignments in effect now in `scope` (plus the global ones).
    fn in_effect<'a>(&'a self, scope: Option<&'a Scope>) -> impl Iterator<Item = &'a Granted> {
        let at = now();
        self.assignments.iter().filter(move |granted| {
            (granted.scope.is_global() || scope.is_some_and(|scope| granted.scope == *scope))
                && granted.active_at(at)
        })
    }

    fn grants(&self, granted: &Granted, permission: &str) -> bool {
        self.permissions
            .get(&granted.role)
            .is_some_and(|list| list.contains(permission))
    }

    /// The role names in effect in the request's scope, sorted, once each.
    pub(crate) fn roles(&self) -> Vec<String> {
        let scope = active_scope();
        let mut roles: Vec<String> = self
            .in_effect(scope.as_ref())
            .map(|granted| granted.role.clone())
            .collect();
        roles.sort();
        roles.dedup();
        roles
    }

    /// Whether `role` is in effect in the request's scope.
    pub(crate) fn has_role(&self, role: &str) -> bool {
        self.has_role_in(role, active_scope().as_ref())
    }

    /// Whether a role in effect in the request's scope grants `permission`.
    pub(crate) fn has_permission(&self, permission: &str) -> bool {
        self.has_permission_in(permission, active_scope().as_ref())
    }

    pub(crate) fn has_role_in(&self, role: &str, scope: Option<&Scope>) -> bool {
        self.in_effect(scope).any(|granted| granted.role == role)
    }

    pub(crate) fn has_permission_in(&self, permission: &str, scope: Option<&Scope>) -> bool {
        self.in_effect(scope)
            .any(|granted| self.grants(granted, permission))
    }

    fn permissions_in(&self, scope: Option<&Scope>) -> HashSet<&str> {
        self.in_effect(scope)
            .filter_map(|granted| self.permissions.get(&granted.role))
            .flatten()
            .map(String::as_str)
            .collect()
    }

    pub(crate) fn scopes_with<M: Model>(&self, permission: &str) -> Scopes<M::Key> {
        let at = now();
        let mut keys: Vec<M::Key> = Vec::new();
        for granted in &self.assignments {
            if !granted.active_at(at) || !self.grants(granted, permission) {
                continue;
            }
            if granted.scope.is_global() {
                return Scopes::All;
            }
            if let Some(key) = granted.scope.key::<M>()
                && !keys.contains(&key)
            {
                keys.push(key);
            }
        }
        keys.sort();
        Scopes::Only(keys)
    }
}

/// A user's role assignments; with `current`, only those not ended yet.
async fn load_assignments(db: &Db, user_id: i64, current: bool) -> Result<Vec<Granted>> {
    let mut query = String::from(
        "SELECT r.name, ru.scope_type, ru.scope_id, ru.starts_at, ru.ends_at \
         FROM role_user ru JOIN roles r ON r.id = ru.role_id WHERE ru.user_id = ?",
    );
    if current {
        query.push_str(" AND (ru.ends_at IS NULL OR ru.ends_at > ?)");
    }
    query.push_str(" ORDER BY r.name, ru.scope_type, ru.scope_id");
    let mut statement = sql(query).bind(user_id);
    if current {
        statement = statement.bind(now());
    }
    type Row = (String, String, String, Option<DateTime>, Option<DateTime>);
    let rows: Vec<Row> = statement.fetch_as(db).await?;
    Ok(rows
        .into_iter()
        .map(|(role, kind, id, starts_at, ends_at)| Granted {
            role,
            scope: Scope { kind, id },
            starts_at,
            ends_at,
        })
        .collect())
}

/// A user's role assignments and their roles' permissions, for the auth
/// middleware.
pub(crate) async fn grants(db: &Db, user_id: i64) -> Result<Grants> {
    let assignments = load_assignments(db, user_id, true).await?;
    let mut permissions: HashMap<String, HashSet<String>> = HashMap::new();
    if !assignments.is_empty() {
        let rows: Vec<(String, String)> = sql("SELECT DISTINCT r.name, p.name FROM roles r \
             JOIN permission_role pr ON pr.role_id = r.id \
             JOIN permissions p ON p.id = pr.permission_id \
             WHERE r.id IN (SELECT role_id FROM role_user WHERE user_id = ?)")
        .bind(user_id)
        .fetch_as(db)
        .await?;
        for (role, permission) in rows {
            permissions.entry(role).or_default().insert(permission);
        }
    }
    Ok(Grants {
        assignments,
        permissions,
    })
}

/// The id of the row named `name` in `table`, created if missing.
async fn ensure(tx: &mut crate::db::Transaction, table: &str, name: &str) -> Result<i64> {
    let at = now();
    sql(format!(
        "INSERT INTO {table} (name, created_at, updated_at) SELECT ?, ?, ? \
         WHERE NOT EXISTS (SELECT 1 FROM {table} WHERE name = ?)"
    ))
    .bind(name)
    .bind(at)
    .bind(at)
    .bind(name)
    .execute(&mut *tx)
    .await?;
    Ok(sql(format!("SELECT id FROM {table} WHERE name = ?"))
        .bind(name)
        .scalar(&mut *tx)
        .await?)
}

async fn existing_role(tx: &mut crate::db::Transaction, name: &str) -> Result<i64> {
    let id: Option<i64> = sql("SELECT id FROM roles WHERE name = ?")
        .bind(name)
        .scalars(&mut *tx)
        .await?
        .into_iter()
        .next();
    id.ok_or_else(|| {
        anyhow::anyhow!("there is no role `{name}`; create it with permissions::define_role").into()
    })
}

async fn link(tx: &mut crate::db::Transaction, permission: i64, role: i64) -> Result {
    sql(
        "INSERT INTO permission_role (permission_id, role_id) SELECT ?, ? \
         WHERE NOT EXISTS (SELECT 1 FROM permission_role WHERE permission_id = ? AND role_id = ?)",
    )
    .bind(permission)
    .bind(role)
    .bind(permission)
    .bind(role)
    .execute(&mut *tx)
    .await?;
    Ok(())
}
