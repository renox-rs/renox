//! The role × permission matrix: what each role may do, changed live by
//! the owner (#239).
//!
//! **RBAC in one page.** A role is only a named set of permissions; code
//! never asks for a role, only for a permission (`access::catalogue`). So
//! "cashiers may now refund" is one switch here, no deploy: the next
//! request of every cashier counts it (Renox loads roles and permissions
//! with the user, per request).
//!
//! Each switch posts on its own with htmx ([`toggle`]): `permissions::grant`
//! or `permissions::revoke`, an audit entry, and a toast. Two guards keep
//! the owner from locking themselves out: a global role always keeps
//! `roles.manage` and `staff.access`.

use renox::auth::permissions;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::audit;
use crate::app::access::active_store;
use crate::app::access::catalogue::{self, PERMISSIONS, ROLES_MANAGE, STAFF_ACCESS};

/// The routes (`roles.manage`).
pub fn routes() -> Routes {
    active_store::staff_routes(
        Routes::new()
            .get("/staff/roles", index)
            .name("staff.roles.index")
            .post("/staff/roles/{role}/{permission}", toggle)
            .name("staff.roles.toggle")
            .require_permission(ROLES_MANAGE),
    )
}

/// A row of the matrix: one permission, and whether each role grants it.
#[derive(Serialize, Debug)]
pub struct Row {
    pub name: &'static str,
    pub description: &'static str,
    /// The area, for the group headings (`rentals` of `rentals.checkout`).
    pub group: &'static str,
    /// Whether this row starts a new group (its heading is drawn above it).
    pub new_group: bool,
    /// One per role, in the columns' order.
    pub cells: Vec<Cell>,
}

/// One switch.
#[derive(Serialize, Debug)]
pub struct Cell {
    pub role: String,
    pub granted: bool,
    /// Switched off and locked (a global role's `roles.manage`).
    pub locked: bool,
}

/// A column of the matrix.
#[derive(Serialize, Debug)]
pub struct Column {
    pub name: String,
    pub label: String,
    pub global: bool,
    pub count: usize,
}

/// Whether `role` is a global role (in the catalogue).
fn is_global(role: &str) -> bool {
    catalogue::roles()
        .into_iter()
        .any(|r| r.global && r.name == role)
}

/// What can't be taken from a global role.
fn locked(role: &str, permission: &str) -> bool {
    is_global(role) && [ROLES_MANAGE, STAFF_ACCESS].contains(&permission)
}

/// Marks the rows that start a group, whose heading the page draws above.
fn mark_groups(rows: &mut [Row]) {
    let mut previous = "";
    for row in rows {
        row.new_group = row.group != previous;
        previous = row.group;
    }
}

// [explain:staff.roles.index.handler]
/// `GET /staff/roles`: the matrix. Two queries (Renox's `permissions::roles`).
pub async fn index(State(db): State<Db>) -> Result<View> {
    let roles = permissions::roles(&db).await?;
    let labels: Vec<_> = catalogue::roles();
    let columns: Vec<Column> = roles
        .iter()
        .map(|(name, granted)| {
            let known = labels.iter().find(|r| r.name == name);
            Column {
                name: name.clone(),
                label: known.map_or_else(|| name.clone(), |r| r.label.to_owned()),
                global: is_global(name),
                count: granted.len(),
            }
        })
        .collect();
    let mut rows: Vec<Row> = PERMISSIONS
        .iter()
        .map(|p| Row {
            name: p.name,
            description: p.description,
            group: p.name.split('.').next().unwrap_or(p.name),
            new_group: false,
            cells: roles
                .iter()
                .map(|(role, granted)| Cell {
                    role: role.clone(),
                    granted: granted.iter().any(|g| g == p.name),
                    locked: locked(role, p.name),
                })
                .collect(),
        })
        .collect();
    mark_groups(&mut rows);
    Ok(view("staff/roles/index.html", context! { columns, rows }))
}
// [/explain:staff.roles.index.handler]

/// The switch's value: present when it was switched on.
#[derive(Deserialize, Debug, Default)]
pub struct ToggleForm {
    pub granted: Option<String>,
}

// [explain:staff.roles.index.toggle]
/// `POST /staff/roles/{role}/{permission}`: grants or revokes one
/// permission; answers with a toast (htmx) or goes back (a plain form).
pub async fn toggle(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path((role, permission)): Path<(String, String)>,
    Form(form): Form<ToggleForm>,
) -> Result<Response> {
    // [/explain:staff.roles.index.toggle]
    let db = &state.db;
    if !PERMISSIONS.iter().any(|p| p.name == permission) {
        return Err(Error::NotFound);
    }
    if !permissions::roles(db)
        .await?
        .iter()
        .any(|(name, _)| *name == role)
    {
        return Err(Error::NotFound);
    }
    // [explain:staff.roles.index.toggle]
    let grant = form.granted.is_some();
    if !grant && locked(&role, &permission) {
        return Err(Error::Forbidden);
    }
    let lang = state.current_lang();
    let (action, key) = if grant {
        permissions::grant(db, &role, &[&permission]).await?;
        ("role.permission_granted", "staff.roles.granted")
    } else {
        permissions::revoke(db, &role, &[&permission]).await?;
        ("role.permission_revoked", "staff.roles.revoked")
    };
    audit::record(db, &user, ROLES_MANAGE, action)
        .data(json!({ "role": role, "permission": permission }))
        .save()
        .await?;
    let toast = Toast::success(lang.t(key, &[("role", &role), ("permission", &permission)]));
    if htmx.request {
        return Ok(toast.into_response());
    }
    Ok((toast, Redirect::to(&state.url("staff.roles.index", &[])?)).into_response())
    // [/explain:staff.roles.index.toggle]
}
