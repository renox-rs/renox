//! The activity log for admins (`activity.view`): the `Audit` module's
//! `audit_logs` table in a grid. Logins and account changes are recorded by
//! Renox; role changes by this app (`renox::audit::record`). `Entry` is a
//! read-only model over that table, so the grid can page, filter and export.

use renox::grid::{Column, Grid, GridRequest};
use renox::prelude::*;
use serde::Serialize;

use super::roles::ACTIVITY;

pub struct Activity;

impl Module for Activity {
    fn name(&self) -> &'static str {
        "activity"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/activity", index)
            .name("activity.index")
            .require_permission(ACTIVITY)
            .require_verified()
    }
}

#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "audit_logs")]
pub struct Entry {
    pub id: i64,
    pub user_id: Option<i64>,
    pub action: String,
    pub subject_type: Option<String>,
    pub subject_id: Option<i64>,
    /// JSON, as text.
    pub data: String,
    pub ip: Option<String>,
    pub created_at: Option<DateTime>,
}

pub fn grid() -> Grid {
    Grid::new("activity")
        .title("Activity")
        .per_page(25)
        .column(Column::datetime("created_at", "When").mobile())
        .column(
            Column::related("user", "Who", "users", "user_id", "name")
                .mobile()
                .searchable(),
        )
        .column(
            Column::text("action", "What")
                .mobile()
                .searchable()
                .badges(&[
                    ("auth.login", "success"),
                    ("auth.login_failed", "danger"),
                    ("auth.locked_out", "danger"),
                    ("user.roles_changed", "warning"),
                ]),
        )
        .column(Column::text("data", "Details").limit(60).tooltip("data"))
        .column(Column::text("ip", "IP").hidden())
        .sort_by("-created_at")
        .exports()
        .empty_state("Nothing recorded in this view", None)
}

async fn index(State(db): State<Db>, request: GridRequest) -> Result<Response> {
    if let Some(file) = grid().export(Entry::query(), &request).await? {
        return Ok(file);
    }
    let activity = grid().page(Entry::query(), &request).await?;
    // Nothing recorded at all: the page shows an empty state instead of the grid.
    let recorded = Entry::query().count(&db).await? > 0;
    Ok(view("activity/index.html", context! { activity, recorded }).into_response())
}
