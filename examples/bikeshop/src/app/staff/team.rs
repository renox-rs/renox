//! The team of a store: who works there, invitations, roles given in the
//! store (with dates), and deactivation (#239).
//!
//! Everything here needs `staff.manage` **in the active store**: a manager
//! of North manages North's team; the owner (a global role) picks the store
//! in the switcher first.
//!
//! - **Invite** (`staff.invitations.*`): a name-free mail with a signed link
//!   (`renox::signed`, seven days) to `/staff/join/{store}/{role}/{email}`.
//!   The signature covers the store, the role and the address, so nobody
//!   can change the role in the link. Opening it ([`join`]) asks for a
//!   name and a password (or a login, for someone who already has an
//!   account), then gives the role in that store, makes the `staff` row
//!   with that home store, and sends them to log in, where two-factor login
//!   is required before the back office opens.
//! - **Roles** (`staff.team.roles.*`): give a role in this store, optionally
//!   from / until a date (`assign_role_in(…).from(…).until(…)`, #244), or
//!   take one away. Someone may only give a role whose every permission
//!   they hold themselves in this store, so a manager can't make an owner.
//! - **Deactivate** (`staff.team.deactivate`): the person leaves. Every role
//!   they had is removed and **all their sessions end at once**
//!   (`User::revoke_sessions`), so a stolen or shared login stops working
//!   everywhere. Reactivating keeps them logged out until given a role again.
//!
//! Every change is in the audit log (`staff::audit`).

use renox::auth::permissions;
use renox::prelude::*;
use renox::signed::ValidSignature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use super::audit;
use super::model::{Staff, Store};
use crate::app::access::catalogue::{self, STAFF_MANAGE};
use crate::app::access::policy::store_scope;
use crate::app::access::{active_store, can_in};
use crate::app::accounts::claim::normalize_email;

/// How long an invitation link works.
pub const INVITATION_VALID_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The staff routes (`staff.manage` in the active store) and the public
/// join link.
pub fn routes() -> Routes {
    let managed = active_store::staff_routes(
        Routes::new()
            .get("/staff/team", index)
            .name("staff.team.index")
            .get("/staff/team/invite", invite)
            .name("staff.invitations.create")
            .post("/staff/team/invite", send_invitation)
            .name("staff.invitations.store")
            .get("/staff/team/{staff}", show)
            .name("staff.team.show")
            .post("/staff/team/{staff}/roles", assign)
            .name("staff.team.roles.store")
            .delete("/staff/team/{staff}/roles/{role}", remove)
            .name("staff.team.roles.destroy")
            .post("/staff/team/{staff}/deactivate", deactivate)
            .name("staff.team.deactivate")
            .post("/staff/team/{staff}/reactivate", reactivate)
            .name("staff.team.reactivate")
            .require_permission(STAFF_MANAGE),
    );
    let public = Routes::new()
        .get("/staff/join/{store}/{role}/{email}", join)
        .name("staff.invitations.accept")
        .post("/staff/join/{store}/{role}/{email}", accept)
        .name("staff.invitations.join");
    managed.merge(public)
}

/// The store this request works in (the staff routes always have one).
fn active() -> Result<i64> {
    active_store::current().ok_or(Error::Forbidden)
}

/// A member of the team, for the list and the page.
#[derive(Serialize, Debug, Clone)]
pub struct Member {
    pub staff: Staff,
    pub name: String,
    pub email: String,
    pub home_store: String,
    /// Roles in force in the active store (and global ones), by name.
    pub roles: Vec<String>,
}

// [explain:staff.team.index.members]
/// Everyone who belongs to `store`: their home store, or a role given there
/// (in force or later). Three queries however many there are.
async fn members(db: &Db, store: i64) -> Result<Vec<Member>> {
    let scope = store_scope(store);
    let with_role: Vec<i64> = renox::db::sql(
        "SELECT DISTINCT user_id FROM role_user WHERE scope_type = ? AND scope_id = ? \
         AND (ends_at IS NULL OR ends_at > ?)",
    )
    .bind(scope.kind().to_owned())
    .bind(scope.id().to_owned())
    .bind(renox::db::now())
    .scalars(db)
    .await?;
    let mut staff: Vec<Staff> = Staff::query().order_by(Staff::ID).get(db).await?;
    staff.retain(|s| s.home_store_id == store || with_role.contains(&s.user_id));
    let user_ids: Vec<i64> = staff.iter().map(|s| s.user_id).collect();
    // [/explain:staff.team.index.members]
    let users: HashMap<i64, User> = if user_ids.is_empty() {
        HashMap::new()
    } else {
        User::query()
            .where_in("id", user_ids.clone())
            .get(db)
            .await?
            .into_iter()
            .map(|u| (u.id, u))
            .collect()
    };
    let roles = roles_in(db, &user_ids, store).await?;
    let stores: HashMap<i64, String> = Store::all_by_name(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    // [explain:staff.team.index.members]
    let mut found: Vec<Member> = staff
        .into_iter()
        .filter_map(|s| {
            let user = users.get(&s.user_id)?;
            Some(Member {
                name: user.name.clone(),
                email: user.email.clone(),
                home_store: stores.get(&s.home_store_id).cloned().unwrap_or_default(),
                roles: roles.get(&s.user_id).cloned().unwrap_or_default(),
                staff: s,
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
    // [/explain:staff.team.index.members]
}

/// The roles in force now for each of `user_ids`, in `store` or global.
async fn roles_in(db: &Db, user_ids: &[i64], store: i64) -> Result<HashMap<i64, Vec<String>>> {
    let mut found: HashMap<i64, Vec<String>> = HashMap::new();
    if user_ids.is_empty() {
        return Ok(found);
    }
    let scope = store_scope(store);
    let now = renox::db::now();
    let marks = vec!["?"; user_ids.len()].join(", ");
    let mut query = renox::db::sql(format!(
        "SELECT ru.user_id, r.name FROM role_user ru JOIN roles r ON r.id = ru.role_id \
         WHERE ru.user_id IN ({marks}) \
         AND (ru.scope_type = '' OR (ru.scope_type = ? AND ru.scope_id = ?)) \
         AND (ru.starts_at IS NULL OR ru.starts_at <= ?) \
         AND (ru.ends_at IS NULL OR ru.ends_at > ?) ORDER BY r.name"
    ));
    for id in user_ids {
        query = query.bind(*id);
    }
    let rows: Vec<(i64, String)> = query
        .bind(scope.kind().to_owned())
        .bind(scope.id().to_owned())
        .bind(now)
        .bind(now)
        .fetch_as(db)
        .await?;
    for (user, role) in rows {
        found.entry(user).or_default().push(role);
    }
    Ok(found)
}

/// One member of the active store's team, or a 404.
async fn member(db: &Db, staff_id: i64) -> Result<Member> {
    let store = active()?;
    members(db, store)
        .await?
        .into_iter()
        .find(|m| m.staff.id == staff_id)
        .ok_or(Error::NotFound)
}

/// A catalogue text (a role's label) in the page's language: the template
/// function `tx` for handlers, which build option lists with it. The
/// translation lives under `text.<english>`; the English shows when the
/// language has none.
pub fn translated(lang: &Lang, text: &str) -> String {
    let key = format!("text.{text}");
    let said = lang.t(&key, &[]);
    if said == key { text.to_owned() } else { said }
}

/// `[value, label]` pairs for a select, the labels in the page's language.
pub fn role_options(lang: &Lang, roles: &[(String, String)]) -> Vec<(String, String)> {
    roles
        .iter()
        .map(|(name, label)| (name.clone(), translated(lang, label)))
        .collect()
}

/// The roles `user` may give in `store`: not a global one, and only roles
/// whose every permission `user` holds there themselves. From the database
/// (the owner may have changed what a role grants).
pub async fn assignable(db: &Db, user: &User, store: i64) -> Result<Vec<(String, String)>> {
    let globals: Vec<&str> = catalogue::roles()
        .into_iter()
        .filter(|r| r.global)
        .map(|r| r.name)
        .collect();
    let labels: HashMap<&str, &str> = catalogue::roles()
        .into_iter()
        .map(|r| (r.name, r.label))
        .collect();
    Ok(permissions::roles(db)
        .await?
        .into_iter()
        .filter(|(name, granted)| {
            !globals.contains(&name.as_str()) && granted.iter().all(|p| can_in(user, p, store))
        })
        .map(|(name, _)| {
            let label = labels
                .get(name.as_str())
                .map_or(name.clone(), |l| (*l).to_owned());
            (name, label)
        })
        .collect())
}

// [explain:staff.team.index.handler]
/// `GET /staff/team`: the active store's team.
pub async fn index(State(db): State<Db>) -> Result<View> {
    let store = Store::find_or_404(&db, active()?).await?;
    let members = members(&db, store.id).await?;
    Ok(view("staff/team/index.html", context! { store, members }))
}
// [/explain:staff.team.index.handler]

/// `GET /staff/team/{staff}`: one person's roles everywhere, and the forms.
pub async fn show(
    State(db): State<Db>,
    user: AuthUser,
    lang: Lang,
    Path(id): Path<i64>,
) -> Result<View> {
    let store = Store::find_or_404(&db, active()?).await?;
    let member = member(&db, id).await?;
    let person = User::find_or_404(&db, member.staff.user_id).await?;
    let stores: HashMap<String, String> = Store::all_by_name(&db)
        .await?
        .into_iter()
        .map(|s| (s.id.to_string(), s.name))
        .collect();
    let scope = store_scope(store.id);
    let assignments: Vec<_> = person
        .assignments(&db)
        .await?
        .into_iter()
        .map(|a| {
            let here = a.scope.kind() == scope.kind() && a.scope.id() == scope.id();
            json!({
                "role": a.role,
                "store": if a.scope.is_global() { None } else { stores.get(a.scope.id()).cloned() },
                "global": a.scope.is_global(),
                "here": here,
                "starts_at": a.starts_at,
                "ends_at": a.ends_at,
                "active": a.is_active(),
            })
        })
        .collect();
    let roles = assignable(&db, &user, store.id).await?;
    let role_options = role_options(&lang, &roles);
    Ok(view(
        "staff/team/show.html",
        context! {
            store,
            member,
            assignments,
            roles,
            role_options,
            is_me => person.id == user.id,
        },
    ))
}

/// The "give a role" form.
#[derive(Deserialize, Validate)]
pub struct AssignForm {
    #[validate(required)]
    pub role: String,
    pub starts_on: Option<renox::chrono::NaiveDate>,
    #[validate(after_or_equal("starts_on"))]
    pub ends_on: Option<renox::chrono::NaiveDate>,
}

// [explain:staff.team.show.assign]
/// `POST /staff/team/{staff}/roles`: gives a role in the active store.
pub async fn assign(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<AssignForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let store = active()?;
    let member = member(db, id).await?;
    if !assignable(db, &user, store)
        .await?
        .iter()
        .any(|(name, _)| *name == form.role)
    {
        return Err(Error::Forbidden);
    }
    let person = User::find_or_404(db, member.staff.user_id).await?;
    let mut assign = person.assign_role_in(db, &form.role, &store_scope(store));
    if let Some(day) = form.starts_on {
        assign = assign.from(midnight(day));
    }
    if let Some(day) = form.ends_on {
        assign = assign.until(midnight(day + renox::chrono::Duration::days(1)));
    }
    assign.await?;
    // [/explain:staff.team.show.assign]
    audit::record(db, &user, STAFF_MANAGE, "staff.role_assigned")
        .subject("staff", member.staff.id)
        .data(json!({ "role": form.role, "from": form.starts_on, "until": form.ends_on }))
        .save()
        .await?;
    done(&state, "staff.team.role_given", id)
}

/// `DELETE /staff/team/{staff}/roles/{role}`: takes a role away in the active store.
pub async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, role)): Path<(i64, String)>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let store = active()?;
    let member = member(db, id).await?;
    if !assignable(db, &user, store)
        .await?
        .iter()
        .any(|(name, _)| *name == role)
    {
        return Err(Error::Forbidden);
    }
    let person = User::find_or_404(db, member.staff.user_id).await?;
    person
        .remove_role_in(db, &role, &store_scope(store))
        .await?;
    audit::record(db, &user, STAFF_MANAGE, "staff.role_removed")
        .subject("staff", member.staff.id)
        .data(json!({ "role": role }))
        .save()
        .await?;
    done(&state, "staff.team.role_removed", id)
}

/// Whether `user_id` has a global role (the owner): nobody deactivates them here.
async fn has_global_role(db: &Db, user_id: i64) -> Result<bool> {
    let count: i64 =
        renox::db::sql("SELECT COUNT(*) FROM role_user WHERE user_id = ? AND scope_type = ''")
            .bind(user_id)
            .scalar(db)
            .await?;
    Ok(count > 0)
}

// [explain:staff.team.show.deactivate]
/// `POST /staff/team/{staff}/deactivate`: removes every role and ends every
/// session of the person (see the module docs).
pub async fn deactivate(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut member = member(db, id).await?;
    if member.staff.user_id == user.id || has_global_role(db, member.staff.user_id).await? {
        return Err(Error::Forbidden);
    }
    // [/explain:staff.team.show.deactivate]
    // The home store's manager decides about the person, not a store they help.
    if !can_in(&user, STAFF_MANAGE, member.staff.home_store_id) {
        return Err(Error::Forbidden);
    }
    // [explain:staff.team.show.deactivate]
    let person = User::find_or_404(db, member.staff.user_id).await?;
    let removed: Vec<_> = person
        .assignments(db)
        .await?
        .into_iter()
        .map(|a| json!({ "role": a.role, "scope": a.scope.id(), "until": a.ends_at }))
        .collect();
    renox::db::sql("DELETE FROM role_user WHERE user_id = ?")
        .bind(person.id)
        .execute(db)
        .await?;
    member.staff.active = false;
    member.staff.save(db).await?;
    person.revoke_sessions(db).await?;
    // [/explain:staff.team.show.deactivate]
    audit::record(db, &user, STAFF_MANAGE, "staff.deactivated")
        .subject("staff", member.staff.id)
        .data(json!({ "roles_removed": removed }))
        .save()
        .await?;
    done(&state, "staff.team.deactivated", id)
}

/// `POST /staff/team/{staff}/reactivate`: back on the team (give a role to
/// let them work again).
pub async fn reactivate(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut member = member(db, id).await?;
    if !can_in(&user, STAFF_MANAGE, member.staff.home_store_id) {
        return Err(Error::Forbidden);
    }
    member.staff.active = true;
    member.staff.save(db).await?;
    audit::record(db, &user, STAFF_MANAGE, "staff.reactivated")
        .subject("staff", member.staff.id)
        .save()
        .await?;
    done(&state, "staff.team.reactivated", id)
}

/// Back to the person's page with a toast.
fn done(state: &AppState, key: &str, staff_id: i64) -> Result<(Toast, Redirect)> {
    Ok((
        Toast::success(state.current_lang().t(key, &[])),
        Redirect::to(&state.url("staff.team.show", &[&staff_id])?),
    ))
}

/// 00:00 UTC on `day`.
fn midnight(day: renox::chrono::NaiveDate) -> DateTime {
    super::factories::midnight(day)
}

/// `GET /staff/team/invite`: the invitation form.
pub async fn invite(State(db): State<Db>, user: AuthUser, lang: Lang) -> Result<View> {
    let store = Store::find_or_404(&db, active()?).await?;
    let roles = role_options(&lang, &assignable(&db, &user, store.id).await?);
    Ok(view(
        "staff/team/invite.html",
        context! { store, role_options => roles },
    ))
}

/// The invitation form.
#[derive(Deserialize, Validate)]
pub struct InvitationForm {
    #[validate(required, email, max = 255)]
    pub email: String,
    #[validate(required)]
    pub role: String,
}

// [explain:staff.invitations.create.send]
/// `POST /staff/team/invite`: mails the signed link.
pub async fn send_invitation(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<InvitationForm>,
) -> Result<(Toast, Redirect)> {
    // [/explain:staff.invitations.create.send]
    let store = Store::find_or_404(&state.db, active()?).await?;
    if !assignable(&state.db, &user, store.id)
        .await?
        .iter()
        .any(|(name, _)| *name == form.role)
    {
        return Err(Error::Forbidden);
    }
    // [explain:staff.invitations.create.send]
    let email = normalize_email(&form.email);
    let link = state.signed_url(
        "staff.invitations.accept",
        &[&store.id, &form.role, &email],
        INVITATION_VALID_FOR,
    )?;
    let lang = state.current_lang();
    let role_label = role_label(&form.role);
    let mail = state.mail_view(
        &email,
        lang.t("staff.invitations.mail.subject", &[("store", &store.name)]),
        "mail/staff/invitation",
        context! {
            store => &store,
            role => role_label,
            inviter => &user.name,
            link,
            days => INVITATION_VALID_FOR.as_secs() / 86_400,
        },
    )?;
    state.queue_mail(mail).await?;
    // [/explain:staff.invitations.create.send]
    audit::record(&state.db, &user, STAFF_MANAGE, "staff.invited")
        .subject("stores", store.id)
        .data(json!({ "email": email, "role": form.role }))
        .save()
        .await?;
    Ok((
        Toast::success(lang.t("staff.invitations.sent", &[("email", &email)])),
        Redirect::to(&state.url("staff.team.index", &[])?),
    ))
}

/// A role's label from the catalogue (its name when the owner added one).
fn role_label(name: &str) -> String {
    catalogue::roles()
        .into_iter()
        .find(|r| r.name == name)
        .map_or_else(|| name.to_owned(), |r| r.label.to_owned())
}

/// Whether the invitation was used: the person has the role in the store.
async fn already_joined(db: &Db, email: &str, store: i64, role: &str) -> Result<bool> {
    let Some(user) = User::query().where_eq("email", email).first(db).await? else {
        return Ok(false);
    };
    let scope = store_scope(store);
    Ok(user
        .assignments(db)
        .await?
        .iter()
        .any(|a| a.role == role && a.scope.kind() == scope.kind() && a.scope.id() == scope.id()))
}

// [explain:staff.invitations.accept.join]
/// `GET /staff/join/{store}/{role}/{email}` (signed): the page the
/// invitation opens.
pub async fn join(
    _: ValidSignature,
    State(db): State<Db>,
    user: Option<AuthUser>,
    renox::axum::extract::OriginalUri(uri): renox::axum::extract::OriginalUri,
    Path((store, role, email)): Path<(i64, String, String)>,
) -> Result<View> {
    let store = Store::find_or_404(&db, store).await?;
    let email = normalize_email(&email);
    let has_account = User::query()
        .where_eq("email", email.clone())
        .first(&db)
        .await?
        .is_some();
    let used = already_joined(&db, &email, store.id, &role).await?;
    let signed_in_as_invited = user.as_ref().is_some_and(|u| u.email == email);
    Ok(view(
        "staff/team/join.html",
        context! {
            store,
            role => role_label(&role),
            email,
            has_account,
            used,
            signed_in_as_invited,
            action => uri.to_string(),
        },
    ))
}
// [/explain:staff.invitations.accept.join]

/// What a new member of staff types.
#[derive(Deserialize)]
pub struct JoinForm {
    pub name: Option<String>,
    pub password: Option<String>,
    pub password_confirmation: Option<String>,
}

impl Validate for JoinForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).max(100);
        v.field("password", &self.password)
            .min(8)
            .confirmed(&self.password_confirmation);
    }
}

// [explain:staff.invitations.accept.accept]
/// `POST /staff/join/{store}/{role}/{email}` (signed): makes the account if
/// needed, the `staff` row and the role, then sends them to log in.
pub async fn accept(
    _: ValidSignature,
    State(state): State<AppState>,
    user: Option<AuthUser>,
    session: Session,
    Path((store, role, email)): Path<(i64, String, String)>,
    Valid(form): Valid<JoinForm>,
) -> Result<(Toast, Redirect)> {
    // [/explain:staff.invitations.accept.accept]
    let db = &state.db;
    let store = Store::find_or_404(db, store).await?;
    let email = normalize_email(&email);
    if !catalogue::roles()
        .iter()
        .any(|r| r.name == role && !r.global)
    {
        return Err(Error::NotFound);
    }
    if already_joined(db, &email, store.id, &role).await? {
        return Err(Error::NotFound);
    }
    let person = match User::query()
        .where_eq("email", email.clone())
        .first(db)
        .await?
    {
        // An existing account joins only when logged in as itself.
        Some(existing) => {
            if !user.as_ref().is_some_and(|u| u.id == existing.id) {
                return Err(Error::Forbidden);
            }
            existing
        }
        None => {
            let (Some(name), Some(password)) = (form.name, form.password) else {
                return Err(Error::BadRequest("name and password are required".into()));
            };
            let mut created = User::register(db, name.trim(), &email, &password).await?;
            // The link reached this address, so it is verified.
            created.email_verified_at = Some(renox::db::now());
            created.save(db).await?;
            created
        }
    };
    match Staff::of_user(db, person.id).await? {
        Some(mut staff) => {
            staff.active = true;
            staff.save(db).await?;
        }
        None => {
            Staff::create(
                db,
                Staff {
                    user_id: person.id,
                    home_store_id: store.id,
                    hired_on: Some(renox::db::now().date_naive()),
                    active: true,
                    ..Default::default()
                },
            )
            .await?;
        }
    }
    // [explain:staff.invitations.accept.accept]
    person
        .assign_role_in(db, &role, &store_scope(store.id))
        .await?;
    renox::audit::record(
        db,
        renox::audit::Entry::new("staff.joined")
            .user(person.id)
            .subject("stores", store.id)
            .data(json!({ "role": role })),
    )
    .await?;
    // Log in afresh: the login asks a member of staff to set up two-factor login.
    if user.is_some() {
        session.flush();
    }
    // [/explain:staff.invitations.accept.accept]
    Ok((
        Toast::success(
            state
                .current_lang()
                .t("staff.invitations.joined", &[("store", &store.name)]),
        ),
        Redirect::to(&state.url("login", &[])?),
    ))
}
