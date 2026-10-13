//! Everyone who signed up, and their roles, for admins (`users.manage`).
//! Role changes are written to the activity log.

use std::collections::BTreeMap;

use renox::grid::{Column, Grid, GridRequest};
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::roles::{ADMIN, ROLES, USERS};

pub struct Users;

impl Module for Users {
    fn name(&self) -> &'static str {
        "users"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/users", index)
            .name("users.index")
            .put("/users/{id}/roles", update_roles)
            .name("users.roles")
            .require_permission(USERS)
            .require_verified()
    }
}

pub fn grid() -> Grid {
    Grid::new("users")
        .title("Users")
        .column(
            Column::text("name", "Name")
                .mobile()
                .searchable()
                .description("email"),
        )
        .column(
            Column::text("email", "Email")
                .hidden()
                .searchable()
                .copyable(),
        )
        .column(Column::custom("roles", "Roles").mobile())
        .column(Column::datetime("email_verified_at", "Verified"))
        .column(Column::date("created_at", "Joined"))
        .column(Column::custom("actions", "Change"))
        .sort_by("-created_at")
        .cards_on_mobile()
        .empty_state(
            "Nobody has signed up yet",
            Some("People show up here once they register."),
        )
}

/// Every user's roles, by user id.
async fn roles_by_user(db: &Db) -> Result<BTreeMap<i64, Vec<String>>> {
    let pairs: Vec<(i64, String)> = renox::db::sql(
        "SELECT role_user.user_id, roles.name FROM role_user \
         JOIN roles ON roles.id = role_user.role_id ORDER BY roles.name",
    )
    .fetch_as(db)
    .await?;
    let mut map: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for (user, role) in pairs {
        map.entry(user).or_default().push(role);
    }
    Ok(map)
}

async fn index(State(db): State<Db>, request: GridRequest) -> Result<View> {
    let roles = roles_by_user(&db).await?;
    let page = grid().page(User::query(), &request).await?;
    // The roles sheets sit after the grid (a grid is a form, and forms
    // don't nest), one per person on the page.
    let members: Vec<_> = page
        .items()
        .iter()
        .map(|user| {
            json!({
                "id": user.id,
                "name": user.name,
                "roles": roles.get(&user.id).cloned().unwrap_or_default(),
            })
        })
        .collect();
    let users =
        page.extend(|user| json!({ "roles": roles.get(&user.id).cloned().unwrap_or_default() }));
    let role_names: Vec<&str> = ROLES.iter().map(|(name, _)| *name).collect();
    // The figures over the grid.
    let people = User::query().count(&db).await?;
    let verified = User::query()
        .where_not_null("email_verified_at")
        .count(&db)
        .await?;
    let admins = roles
        .values()
        .filter(|names| names.iter().any(|name| name == ADMIN))
        .count();
    let totals = json!({ "people": people, "admins": admins, "verified": verified });
    Ok(view(
        "users/index.html",
        context! { users, members, role_names, totals },
    ))
}

/// Ticked boxes: `roles=admin&roles=member` (none ticked: none sent).
#[derive(Deserialize, Serialize, Validate)]
pub struct RolesForm {
    #[serde(default)]
    #[validate(each(one_of(&["admin", "member"])))]
    pub roles: Vec<String>,
}

async fn update_roles(
    State(db): State<Db>,
    admin: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<RolesForm>,
) -> Result<(Toast, HxRefresh)> {
    let user = User::find_or_404(&db, id).await?;
    let chosen: Vec<&str> = form.roles.iter().map(String::as_str).collect();
    // Nobody locks themselves out of this page.
    if user.id == admin.id && !chosen.contains(&ADMIN) {
        let mut errors = Errors::new();
        errors.add("roles", "You can't take away your own admin role.");
        return Err(ValidationError::new(errors).into());
    }
    let before = user.roles(&db).await?;
    user.sync_roles(&db, &chosen).await?;
    renox::audit::record(
        &db,
        renox::audit::Entry::new("user.roles_changed")
            .user(admin.id)
            .subject("users", user.id)
            .data(json!({ "from": before, "to": chosen })),
    )
    .await?;
    Ok((
        Toast::success(format!("{}'s roles saved.", user.name)),
        HxRefresh,
    ))
}
