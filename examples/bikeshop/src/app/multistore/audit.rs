//! The audit trail of the work between stores (#245): **who** did it,
//! **in which store**, and **with which role there**.
//!
//! Someone may hold roles in two stores (a manager of North helping South
//! as staff this week); "who approved this shipment?" is only half the
//! answer without the store they were working in and the role that let
//! them. [`record`] writes Renox's `audit_logs` row (the `Audit` module,
//! turned on in `src/lib.rs`) with that in its data:
//!
//! ```json
//! { "store_id": 2, "roles": ["staff"], "…": "the action's own details" }
//! ```
//!
//! The roles are the person's assignments counting in that store **now**
//! (given there within their dates, or global): read from the
//! `Permissions` module's `assignments`, never compared with a role name.

use renox::audit::{self, Entry};
use renox::prelude::*;

use crate::app::access::policy::store_scope;

/// The names of the roles `user` holds in `store_id` now (global ones
/// too), for the audit trail.
pub async fn roles_in(db: &Db, user: &User, store_id: i64) -> Result<Vec<String>> {
    let scope = store_scope(store_id);
    let mut names: Vec<String> = user
        .assignments(db)
        .await?
        .into_iter()
        .filter(|a| a.is_active() && (a.scope.is_global() || a.scope == scope))
        .map(|a| a.role)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Records `action` by `user`, working in `store_id`, on `subject`
/// (`(table, id)`), with `data` (an object) plus the store and the roles.
pub async fn record(
    state: &AppState,
    user: &User,
    action: &str,
    store_id: i64,
    subject: (&str, i64),
    data: renox::serde_json::Value,
) -> Result {
    let roles = roles_in(&state.db, user, store_id).await?;
    let mut data = match data {
        renox::serde_json::Value::Object(map) => map,
        other => {
            let mut map = renox::serde_json::Map::new();
            map.insert("details".into(), other);
            map
        }
    };
    data.insert("store_id".into(), json!(store_id));
    data.insert("roles".into(), json!(roles));
    audit::record(
        &state.db,
        Entry::new(action)
            .user(user.id)
            .subject(subject.0, subject.1)
            .data(renox::serde_json::Value::Object(data)),
    )
    .await
}
