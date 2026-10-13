//! The audit log: every sensitive change, with who, in which store, and
//! with which role (#239).
//!
//! Renox's `Audit` module (in `src/lib.rs`) owns the `audit_logs` table and
//! already records logins, failed logins, lockouts, password changes,
//! deleted accounts, two-factor changes and social logins. The shop adds
//! two columns (`migrations/20260102000200_add_store_and_role_to_audit_logs.*`)
//! and records its own sensitive actions through [`record`], which fills
//! them from the request:
//!
//! - `store_id`: the active store (`access::active_store::current`);
//! - `role`: which of the person's roles, in force now, granted the
//!   permission the action needed there (a global role first).
//!
//! **Every area calls [`record`]** for its sensitive actions, e.g. a refund:
//!
//! ```no_run
//! use bikeshop::app::access::catalogue::ORDERS_REFUND;
//! use bikeshop::app::staff::audit;
//! use renox::prelude::*;
//!
//! # async fn demo(db: &Db, user: &User) -> Result {
//! audit::record(db, user, ORDERS_REFUND, "order.refunded")
//!     .subject("orders", 42)
//!     .data(json!({ "amount": 7_500, "reason": "Wrong size" }))
//!     .save()
//!     .await?;
//! # Ok(()) }
//! ```
//!
//! Actions recorded so far: `role.permission_granted` / `_revoked`,
//! `staff.role_assigned` / `_removed`, `staff.invited`, `staff.joined`,
//! `staff.deactivated` / `_reactivated`, `store.updated`,
//! `store.fee_rate_changed`, `catalog.prices_changed`,
//! `catalog.category_moved`, `customer.claimed`, `customer.erased`. The
//! other areas add theirs (stock adjustments, refunds, ID approvals) with
//! the same call.

use renox::prelude::*;
use serde::Serialize;

use crate::app::access::active_store;
use crate::app::access::policy::store_scope;

/// One row of `audit_logs`, with the shop's two columns, for the audit
/// page's grid.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "audit_logs")]
pub struct AuditEntry {
    pub id: i64,
    pub user_id: Option<i64>,
    /// `store.fee_rate_changed`, `auth.login`…
    pub action: String,
    pub subject_type: Option<String>,
    pub subject_id: Option<i64>,
    /// JSON: the old and new values, a reason…
    pub data: String,
    pub ip: Option<String>,
    /// The store the person was working in (the shop's entries only).
    pub store_id: Option<i64>,
    /// The role that granted the permission (the shop's entries only).
    pub role: Option<String>,
    pub created_at: Option<DateTime>,
}

/// An entry being written: [`record`], then `subject`, `data`, `save`.
#[must_use = "call .save().await to write it"]
pub struct Record<'a> {
    db: &'a Db,
    user_id: i64,
    permission: &'a str,
    action: &'a str,
    subject: Option<(&'a str, i64)>,
    data: renox::serde_json::Value,
}

/// Starts an audit entry: `user` did `action`, which needed `permission`.
pub fn record<'a>(db: &'a Db, user: &User, permission: &'a str, action: &'a str) -> Record<'a> {
    Record {
        db,
        user_id: user.id,
        permission,
        action,
        subject: None,
        data: json!({}),
    }
}

impl<'a> Record<'a> {
    /// The record it happened to: `("stores", 3)`.
    pub fn subject(mut self, kind: &'a str, id: i64) -> Self {
        self.subject = Some((kind, id));
        self
    }

    /// Details: old and new values, a reason.
    pub fn data(mut self, data: renox::serde_json::Value) -> Self {
        self.data = data;
        self
    }

    // [explain:staff.audit.index.record]
    /// Writes it, with the active store and the role used.
    pub async fn save(self) -> Result {
        let store = active_store::current();
        let role = role_used(self.db, self.user_id, self.permission, store).await?;
        let (subject_type, subject_id) = match self.subject {
            Some((kind, id)) => (Some(kind.to_owned()), Some(id)),
            None => (None, None),
        };
        renox::db::sql(
            "INSERT INTO audit_logs (user_id, action, subject_type, subject_id, data, ip, \
             store_id, role, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.user_id)
        .bind(self.action)
        .bind(subject_type)
        .bind(subject_id)
        .bind(self.data.to_string())
        .bind(Option::<String>::None)
        .bind(store)
        .bind(role)
        .bind(renox::db::now())
        .execute(self.db)
        .await?;
        Ok(())
    }
    // [/explain:staff.audit.index.record]
}

/// The role that grants `permission` to `user_id` now: a global one first,
/// else one given in `store`. One query.
pub async fn role_used(
    db: &Db,
    user_id: i64,
    permission: &str,
    store: Option<i64>,
) -> Result<Option<String>> {
    let now = renox::db::now();
    let scope = store.map(store_scope);
    renox::db::sql(
        "SELECT r.name FROM role_user ru \
         JOIN roles r ON r.id = ru.role_id \
         JOIN permission_role pr ON pr.role_id = r.id \
         JOIN permissions p ON p.id = pr.permission_id \
         WHERE ru.user_id = ? AND p.name = ? \
         AND (ru.scope_type = '' OR (ru.scope_type = ? AND ru.scope_id = ?)) \
         AND (ru.starts_at IS NULL OR ru.starts_at <= ?) \
         AND (ru.ends_at IS NULL OR ru.ends_at > ?) \
         ORDER BY ru.scope_type, r.name LIMIT 1",
    )
    .bind(user_id)
    .bind(permission)
    .bind(
        scope
            .as_ref()
            .map(|s| s.kind().to_owned())
            .unwrap_or_default(),
    )
    .bind(
        scope
            .as_ref()
            .map(|s| s.id().to_owned())
            .unwrap_or_default(),
    )
    .bind(now)
    .bind(now)
    .scalar_optional(db)
    .await
    .map_err(Into::into)
}

// [explain:staff.audit.index.grid]
/// The audit page's grid: newest first, filtered by any column's heading
/// (who, what, which store, which role, when), searched by action.
pub fn grid() -> renox::grid::Grid {
    use renox::grid::{Column, Grid};
    Grid::new("audit")
        .column(Column::datetime("created_at", "When").mobile())
        .column(Column::related("person", "Who", "users", "user_id", "name").mobile())
        .column(Column::text("action", "What").searchable().mobile())
        .column(Column::related(
            "store", "Store", "stores", "store_id", "name",
        ))
        .column(Column::text("role", "Role"))
        .column(Column::text("subject_type", "Record"))
        .column(Column::number("subject_id", "Id"))
        .column(Column::custom("data", "Details"))
        .column(Column::text("ip", "IP"))
        .sort_by("-created_at")
        .per_page(50)
}
// [/explain:staff.audit.index.grid]

/// The route (`audit.view`).
pub fn routes() -> Routes {
    active_store::staff_routes(
        Routes::new()
            .get("/staff/audit", index)
            .name("staff.audit.index")
            .require_permission(crate::app::access::catalogue::AUDIT_VIEW),
    )
}

// [explain:staff.audit.index.grid]
/// `GET /staff/audit`: the audit log, every store's (the trail is the
/// company's: only `audit.view`, the owner's, opens it).
pub async fn index(request: renox::grid::GridRequest) -> Result<View> {
    let page = grid().page(AuditEntry::query(), &request).await?;
    Ok(view("staff/audit/index.html", context! { entries => page }))
}
// [/explain:staff.audit.index.grid]
