//! Staff helping another store (#245).
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/help` | `multistore.help` | `staff.help` (in the active store) |
//! | `GET /staff/help/new`, `POST /staff/help` | `multistore.help.create`, `.store` | `staff.help` in the store asking |
//! | `POST /staff/help/{request}/approve`, `…/refuse` | `multistore.help.approve`, `.refuse` | `staff.help` in the **lending** store |
//! | `POST /staff/help/{request}/withdraw` | `multistore.help.withdraw` | `staff.help` in the **asking** store |
//! | `POST /staff/help/{request}/end` | `multistore.help.end` | `staff.help` in **either** store |
//! | `POST /staff/help/{request}/hours` | `multistore.help.log` | `staff.help` in the helped store |
//! | `GET /staff/help/hours` | `multistore.help.hours` | `staff.help` |
//!
//! A store short of people asks another for someone, for some days, with
//! the role they'll have (a role's name from the access catalogue). The
//! lending store's manager (or the owner) approves, and the helper gets
//! that role **in the helped store, between the two dates**:
//! `assign_role_in(db, role, &Scope::of(store)).from(start).until(end)`
//! (Renox #244). Nothing has to run at the end date: the role simply stops
//! counting, so the store switcher no longer offers the store and its pages
//! answer 403. Ending early moves the end to now. Hours helped are recorded
//! for reports and never charged between stores (the owner's decision 2).

use std::collections::HashMap;

use renox::auth::permissions::Scopes;
use renox::chrono::{Duration, NaiveDate};
use renox::prelude::*;
use renox::validation::FormContext;
use serde::{Deserialize, Serialize};

use super::audit;
use crate::app::access::policy::store_scope;
use crate::app::access::{active_store, can_in, catalogue};
use crate::app::rentals::booking::{from_local, to_local};
use crate::app::rentals::notify::{self, Notice, Tone};
use crate::app::staff::model::{HelpStatus, Staff, StaffHelpHour, StaffHelpRequest, Store};

/// The roles a helper may be given: every role given in a store (not the
/// owner's global one), from the catalogue.
pub fn roles() -> Vec<(&'static str, &'static str)> {
    catalogue::roles()
        .into_iter()
        .filter(|r| !r.global)
        .map(|r| (r.name, r.label))
        .collect()
}

/// The request `id`, or a 404 unless `user` holds `staff.help` in one of
/// its two stores.
pub async fn find(db: &Db, user: &User, id: i64) -> Result<StaffHelpRequest> {
    let request = StaffHelpRequest::find_or_404(db, id).await?;
    if !can_in(user, catalogue::STAFF_HELP, request.from_store_id)
        && !can_in(user, catalogue::STAFF_HELP, request.to_store_id)
    {
        return Err(Error::NotFound);
    }
    Ok(request)
}

/// A request as the page lists it.
#[derive(Serialize, Debug, Clone)]
pub struct Row {
    #[serde(flatten)]
    pub request: StaffHelpRequest,
    pub helper: String,
    pub from: String,
    pub to: String,
    /// The role's label.
    pub role_label: String,
    /// Approved and counting now.
    pub active: bool,
    /// Approved, not over yet (so it can still be ended early).
    pub running: bool,
    pub minutes: i64,
    pub can_decide: bool,
    pub can_withdraw: bool,
    pub can_end: bool,
    pub can_log: bool,
}

/// The rows of `requests`, in five queries whatever their number: the
/// helpers' staff rows, their users, the stores, the hours.
pub async fn rows(db: &Db, user: &User, requests: Vec<StaffHelpRequest>) -> Result<Vec<Row>> {
    let staff =
        renox::db::relations::belongs_to::<Staff, _, _>(db, &requests, |r| r.staff_id).await?;
    let users = renox::db::relations::belongs_to::<User, _, _>(
        db,
        &staff.values().cloned().collect::<Vec<_>>(),
        |s| s.user_id,
    )
    .await?;
    let stores: HashMap<i64, String> = Store::all_by_name(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    let ids: Vec<i64> = requests.iter().map(|r| r.id).collect();
    let minutes: Vec<(i64, i64)> = StaffHelpHour::query()
        .where_(StaffHelpHour::HELP_REQUEST_ID.is_in(ids))
        .group_by("help_request_id")
        .select_as(db, "help_request_id, CAST(SUM(minutes) AS BIGINT)")
        .await?;
    let minutes: HashMap<i64, i64> = minutes.into_iter().collect();
    let labels: HashMap<&str, &str> = roles().into_iter().collect();
    let now = renox::db::now();
    Ok(requests
        .into_iter()
        .map(|r| {
            let approved = r.status == HelpStatus::Approved;
            let lending = can_in(user, catalogue::STAFF_HELP, r.from_store_id);
            let asking = can_in(user, catalogue::STAFF_HELP, r.to_store_id);
            Row {
                helper: staff
                    .get(&r.staff_id)
                    .and_then(|s| users.get(&s.user_id))
                    .map(|u| u.name.clone())
                    .unwrap_or_default(),
                from: stores.get(&r.from_store_id).cloned().unwrap_or_default(),
                to: stores.get(&r.to_store_id).cloned().unwrap_or_default(),
                role_label: labels
                    .get(r.role.as_str())
                    .copied()
                    .unwrap_or("")
                    .to_owned(),
                active: approved && r.starts_at <= now && now < r.ends_at,
                running: approved && now < r.ends_at,
                minutes: minutes.get(&r.id).copied().unwrap_or(0),
                can_decide: r.status == HelpStatus::Requested && lending,
                can_withdraw: r.status == HelpStatus::Requested && asking,
                can_end: approved && now < r.ends_at && (lending || asking),
                can_log: approved && r.starts_at <= now && asking,
                request: r,
            }
        })
        .collect())
}

// [explain:multistore.help.handler]
/// `GET /staff/help` (`multistore.help`): help asked by the active store,
/// help asked of it, and who is helping now, both ways.
pub async fn index(State(db): State<Db>, user: AuthUser) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let requests = StaffHelpRequest::query()
        .where_any(|q| {
            q.where_(StaffHelpRequest::FROM_STORE_ID.eq(store))
                .where_(StaffHelpRequest::TO_STORE_ID.eq(store))
        })
        .order_by_desc(StaffHelpRequest::STARTS_AT)
        .order_by_desc(StaffHelpRequest::ID)
        .limit(100)
        .get(&db)
        .await?;
    let rows = rows(&db, &user, requests).await?;
    let (asked_by_us, asked_of_us): (Vec<Row>, Vec<Row>) = rows
        .into_iter()
        .partition(|r| r.request.to_store_id == store);
    Ok(view(
        "multistore/help/index.html",
        context! { asked_by_us, asked_of_us, store },
    ))
}
// [/explain:multistore.help.handler]

/// `?store=` on the new-request page: the store to ask.
#[derive(Deserialize, Default)]
pub struct NewQuery {
    #[serde(default)]
    pub store: Option<i64>,
}

// [explain:multistore.help.create.handler]
/// `GET /staff/help/new` (`multistore.help.create`): ask another store for
/// someone: who, with which role here, from when to when, and why.
pub async fn create(
    State(db): State<Db>,
    Query(query): Query<NewQuery>,
    lang: Lang,
) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let others: Vec<Store> = Store::all_by_name(&db)
        .await?
        .into_iter()
        .filter(|s| s.id != store)
        .collect();
    let lending = query
        .store
        .filter(|id| others.iter().any(|s| s.id == *id))
        .or_else(|| others.first().map(|s| s.id))
        .unwrap_or_default();
    let staff = Staff::query()
        .where_(Staff::HOME_STORE_ID.eq(lending))
        .where_(Staff::ACTIVE.eq(true))
        .get(&db)
        .await?;
    // [/explain:multistore.help.create.handler]
    let users = renox::db::relations::belongs_to::<User, _, _>(&db, &staff, |s| s.user_id).await?;
    let mut people: Vec<(i64, String)> = staff
        .iter()
        .filter_map(|s| users.get(&s.user_id).map(|u| (s.id, u.name.clone())))
        .collect();
    people.sort_by(|a, b| a.1.cmp(&b.1));
    // The roles with their label in the page's language (`tx` for handlers).
    let role_options: Vec<(&str, String)> = roles()
        .into_iter()
        .map(|(name, label)| (name, crate::app::staff::team::translated(&lang, label)))
        .collect();
    let store_options: Vec<(i64, String)> = others.iter().map(|s| (s.id, s.name.clone())).collect();
    // [explain:multistore.help.create.handler]
    let today = crate::seed::today();
    Ok(view(
        "multistore/help/new.html",
        context! { store_options, lending, people, role_options, today },
    ))
}
// [/explain:multistore.help.create.handler]

/// The request form.
#[derive(Deserialize, Debug)]
pub struct HelpForm {
    /// The store asked to lend someone.
    pub store: Option<i64>,
    /// Their `staff` row.
    pub staff: Option<i64>,
    pub role: String,
    /// First day, in the shop's time zone.
    pub starts_on: Option<NaiveDate>,
    /// Last day (the role ends at the end of it).
    pub ends_on: Option<NaiveDate>,
    pub reason: String,
}

// [explain:multistore.help.create.form]
impl Validate for HelpForm {
    fn rules(&self, v: &mut Validator) {
        let names: Vec<&str> = roles().into_iter().map(|(name, _)| name).collect();
        v.field("store", &self.store).required();
        v.field("staff", &self.staff).required();
        v.field("role", &self.role).required().one_of(&names);
        v.field("starts_on", &self.starts_on).required();
        v.field("ends_on", &self.ends_on).required();
        v.field("reason", &self.reason).required().max(300);
    }

    /// The person works at the lending store; the days are in order and
    /// not in the past.
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let lang = form.state.current_lang();
        if let (Some(start), Some(end)) = (self.starts_on, self.ends_on) {
            if end < start {
                errors.add("ends_on", lang.t("multistore.help.errors.order", &[]));
            }
            if start < crate::seed::today() {
                errors.add("starts_on", lang.t("multistore.help.errors.past", &[]));
            }
        }
        // [/explain:multistore.help.create.form]
        if let (Some(staff), Some(store)) = (self.staff, self.store) {
            let found = Staff::find(&form.state.db, staff).await?;
            if found.is_none_or(|s| s.home_store_id != store || !s.active) {
                errors.add("staff", lang.t("multistore.help.errors.staff", &[]));
            }
        }
        // [explain:multistore.help.create.form]
        if active_store::current() == self.store {
            errors.add("store", lang.t("multistore.help.errors.store", &[]));
        }
        Ok(())
    }
}
// [/explain:multistore.help.create.form]

/// Midnight starting `day` in `APP_TIMEZONE`, as a moment.
pub fn day_start(config: &Config, day: NaiveDate) -> DateTime {
    from_local(config, day.and_hms_opt(0, 0, 0).expect("midnight"))
}

/// `POST /staff/help` (`multistore.help.store`): the active store asks.
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<HelpForm>,
) -> Result<(Toast, Redirect)> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let config = &state.config;
    let request = StaffHelpRequest::create(
        &state.db,
        StaffHelpRequest {
            from_store_id: form.store.unwrap_or_default(),
            to_store_id: store,
            staff_id: form.staff.unwrap_or_default(),
            role: form.role.clone(),
            starts_at: day_start(config, form.starts_on.unwrap_or_default()),
            ends_at: day_start(config, form.ends_on.unwrap_or_default() + Duration::days(1)),
            reason: form.reason.trim().to_owned(),
            status: HelpStatus::Requested,
            requested_by: Some(user.id),
            ..Default::default()
        },
    )
    .await?;
    audit::record(
        &state,
        &user,
        "staff_help.requested",
        store,
        (StaffHelpRequest::TABLE, request.id),
        json!({ "lending_store_id": request.from_store_id, "role": request.role }),
    )
    .await?;
    tell(
        &state,
        request.from_store_id,
        &request,
        "multistore.mail.help.requested",
    )
    .await?;
    Ok((
        Toast::success(state.current_lang().t("multistore.help.sent", &[])),
        Redirect::route("multistore.help", &[])?,
    ))
}

/// Tells the managers of `store` (in the app) about `request`.
async fn tell(
    state: &AppState,
    store: i64,
    request: &StaffHelpRequest,
    body: &'static str,
) -> Result {
    let notice = Notice::new("staff-help", "multistore.mail.help.title", body)
        .param("number", request.id)
        .url(crate::app::rentals::link(
            state,
            "multistore.help",
            None::<i64>,
        )?)
        .in_app_only();
    notify::staff(state, catalogue::STAFF_HELP, &[store], &notice).await
}

fn moved_on(state: &AppState) -> Error {
    abort(
        StatusCode::CONFLICT,
        state
            .current_lang()
            .t("multistore.help.errors.moved_on", &[]),
    )
}

/// Moves `request` from `from` to `to`, only if it is still `from`.
async fn advance(
    db: &Db,
    request: &mut StaffHelpRequest,
    from: HelpStatus,
    to: HelpStatus,
    user: &User,
) -> Result<bool> {
    let now = renox::db::now();
    let moved = StaffHelpRequest::query()
        .where_(StaffHelpRequest::ID.eq(request.id))
        .where_(StaffHelpRequest::STATUS.eq(from))
        .update(
            db,
            &[
                ("status", &to as &(dyn renox::db::ToDbValue + Sync)),
                ("approved_by", &user.id),
                ("decided_at", &now),
            ],
        )
        .await?;
    if moved == 0 {
        return Ok(false);
    }
    request.status = to;
    request.approved_by = Some(user.id);
    request.decided_at = Some(now);
    Ok(true)
}

/// `POST /staff/help/{request}/approve` (`multistore.help.approve`): the
/// lending store says yes; the helper gets the role in the helped store
/// between the two dates. Refused when they already hold that role there
/// for good (the dates would replace it).
// [explain:multistore.help.approve]
pub async fn approve(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut request = find(db, &user, id).await?;
    if !can_in(&user, catalogue::STAFF_HELP, request.from_store_id) {
        return Err(Error::Forbidden);
    }
    // [/explain:multistore.help.approve]
    let helper_staff = Staff::find_or_404(db, request.staff_id).await?;
    let helper = User::find_or_404(db, helper_staff.user_id).await?;
    let scope = store_scope(request.to_store_id);
    let lang = state.current_lang();
    let for_good = helper.assignments(db).await?.into_iter().any(|a| {
        a.scope == scope && a.role == request.role && a.starts_at.is_none() && a.ends_at.is_none()
    });
    if for_good {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("multistore.help.errors.already", &[]),
        ));
    }
    if !advance(
        db,
        &mut request,
        HelpStatus::Requested,
        HelpStatus::Approved,
        &user,
    )
    .await?
    {
        return Err(moved_on(&state));
    }
    // [explain:multistore.help.approve]
    helper
        .assign_role_in(db, &request.role, &scope)
        .from(request.starts_at)
        .until(request.ends_at)
        .await?;
    // [/explain:multistore.help.approve]
    audit::record(
        &state,
        &user,
        "staff_help.approved",
        request.from_store_id,
        (StaffHelpRequest::TABLE, request.id),
        json!({ "helper": helper.id, "helped_store_id": request.to_store_id, "role": request.role }),
    )
    .await?;
    tell(
        &state,
        request.to_store_id,
        &request,
        "multistore.mail.help.approved",
    )
    .await?;
    let starts = to_local(&state.config, request.starts_at).date();
    let ends = to_local(&state.config, request.ends_at - Duration::seconds(1)).date();
    state
        .notify(
            &helper,
            &Notice::new(
                "staff-help-helper",
                "multistore.mail.help.helper_title",
                "multistore.mail.help.helper_body",
            )
            .param(
                "store",
                Store::find_or_404(db, request.to_store_id).await?.name,
            )
            .param("from", starts)
            .param("until", ends)
            .tone(Tone::Success)
            .view("mail/multistore/notice")
            .url(crate::app::rentals::link(
                &state,
                "staff.dashboard",
                None::<i64>,
            )?),
        )
        .await?;
    Ok((
        Toast::success(lang.t("multistore.help.approved", &[])),
        Redirect::route("multistore.help", &[])?,
    ))
}

/// `POST /staff/help/{request}/refuse` (`multistore.help.refuse`).
pub async fn refuse(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut request = find(&state.db, &user, id).await?;
    if !can_in(&user, catalogue::STAFF_HELP, request.from_store_id) {
        return Err(Error::Forbidden);
    }
    if !advance(
        &state.db,
        &mut request,
        HelpStatus::Requested,
        HelpStatus::Refused,
        &user,
    )
    .await?
    {
        return Err(moved_on(&state));
    }
    audit::record(
        &state,
        &user,
        "staff_help.refused",
        request.from_store_id,
        (StaffHelpRequest::TABLE, request.id),
        json!({}),
    )
    .await?;
    tell(
        &state,
        request.to_store_id,
        &request,
        "multistore.mail.help.refused",
    )
    .await?;
    Ok((
        Toast::success(state.current_lang().t("multistore.help.refused", &[])),
        Redirect::route("multistore.help", &[])?,
    ))
}

/// `POST /staff/help/{request}/withdraw` (`multistore.help.withdraw`): the
/// asking store takes its request back before it is decided.
pub async fn withdraw(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut request = find(&state.db, &user, id).await?;
    if !can_in(&user, catalogue::STAFF_HELP, request.to_store_id) {
        return Err(Error::Forbidden);
    }
    if !advance(
        &state.db,
        &mut request,
        HelpStatus::Requested,
        HelpStatus::Withdrawn,
        &user,
    )
    .await?
    {
        return Err(moved_on(&state));
    }
    audit::record(
        &state,
        &user,
        "staff_help.withdrawn",
        request.to_store_id,
        (StaffHelpRequest::TABLE, request.id),
        json!({}),
    )
    .await?;
    Ok((
        Toast::success(state.current_lang().t("multistore.help.withdrawn", &[])),
        Redirect::route("multistore.help", &[])?,
    ))
}

/// `POST /staff/help/{request}/end` (`multistore.help.end`): either store
/// (or the owner) ends the help now. The role's end moves to now (or the
/// role goes, if it hadn't started).
pub async fn end(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut request = find(db, &user, id).await?;
    let now = renox::db::now();
    if request.status != HelpStatus::Approved || request.ends_at <= now {
        return Err(moved_on(&state));
    }
    let helper =
        User::find_or_404(db, Staff::find_or_404(db, request.staff_id).await?.user_id).await?;
    let scope = store_scope(request.to_store_id);
    if request.starts_at < now {
        helper
            .assign_role_in(db, &request.role, &scope)
            .from(request.starts_at)
            .until(now)
            .await?;
    } else {
        helper.remove_role_in(db, &request.role, &scope).await?;
    }
    let acting = if can_in(&user, catalogue::STAFF_HELP, request.to_store_id) {
        request.to_store_id
    } else {
        request.from_store_id
    };
    StaffHelpRequest::query()
        .where_(StaffHelpRequest::ID.eq(request.id))
        .update(
            db,
            &[
                (
                    "status",
                    &HelpStatus::EndedEarly as &(dyn renox::db::ToDbValue + Sync),
                ),
                ("ends_at", &now),
            ],
        )
        .await?;
    request.status = HelpStatus::EndedEarly;
    audit::record(
        &state,
        &user,
        "staff_help.ended_early",
        acting,
        (StaffHelpRequest::TABLE, request.id),
        json!({ "helper": helper.id }),
    )
    .await?;
    let other = if acting == request.to_store_id {
        request.from_store_id
    } else {
        request.to_store_id
    };
    tell(&state, other, &request, "multistore.mail.help.ended").await?;
    Ok((
        Toast::success(state.current_lang().t("multistore.help.ended", &[])),
        Redirect::route("multistore.help", &[])?,
    ))
}

/// Hours worked on one day.
#[derive(Deserialize, Validate, Debug)]
pub struct HoursForm {
    #[validate(required)]
    pub worked_on: Option<NaiveDate>,
    #[validate(required, min = 0.5, max = 16.0)]
    pub hours: Option<f64>,
    #[validate(max = 200)]
    pub note: Option<String>,
}

// [explain:multistore.help.hours.log]
/// `POST /staff/help/{request}/hours` (`multistore.help.log`): the helped
/// store records a day's hours (for reports; never charged).
pub async fn log_hours(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<HoursForm>,
) -> Result<(Toast, Redirect)> {
    let request = find(&state.db, &user, id).await?;
    if !can_in(&user, catalogue::STAFF_HELP, request.to_store_id) {
        return Err(Error::Forbidden);
    }
    let lang = state.current_lang();
    let day = form.worked_on.unwrap_or_default();
    let first = to_local(&state.config, request.starts_at).date();
    let last = to_local(&state.config, request.ends_at - Duration::seconds(1)).date();
    if request.status == HelpStatus::Requested || day < first || day > last {
        return Err(abort(
            StatusCode::UNPROCESSABLE_ENTITY,
            lang.t("multistore.help.errors.day", &[]),
        ));
    }
    // [/explain:multistore.help.hours.log]
    let minutes = (form.hours.unwrap_or(0.0) * 60.0).round() as i64;
    let hour = StaffHelpHour::create(
        &state.db,
        StaffHelpHour {
            help_request_id: request.id,
            staff_id: request.staff_id,
            store_id: request.to_store_id,
            worked_on: day,
            minutes,
            note: form.note.clone().filter(|n| !n.trim().is_empty()),
            ..Default::default()
        },
    )
    .await?;
    audit::record(
        &state,
        &user,
        "staff_help.hours_logged",
        request.to_store_id,
        (StaffHelpHour::TABLE, hour.id),
        json!({ "minutes": minutes, "day": day }),
    )
    .await?;
    Ok((
        Toast::success(lang.t("multistore.help.logged", &[])),
        Redirect::route("multistore.help", &[])?,
    ))
}

/// One line of the hours report.
#[derive(FromRow, Serialize, Debug, Clone)]
pub struct HoursLine {
    pub staff_id: i64,
    pub store_id: i64,
    pub days: i64,
    pub minutes: i64,
}

// [explain:multistore.help.hours.query]
/// `GET /staff/help/hours` (`multistore.help.hours`): hours helped per
/// person and store, for the stores the person may see: help given **to**
/// them (the helped store is theirs) and **by** them (the helper's home
/// store is theirs, wherever they helped), in one grouped query.
pub async fn hours(State(db): State<Db>, user: AuthUser) -> Result<View> {
    let scopes = renox::auth::permissions::scopes_with::<Store>(catalogue::STAFF_HELP);
    let query = match &scopes {
        Scopes::All => StaffHelpHour::query(),
        Scopes::Only(ids) if ids.is_empty() => {
            StaffHelpHour::query().where_raw("1 = 0", std::iter::empty::<i64>())
        }
        Scopes::Only(ids) => {
            let marks = vec!["?"; ids.len()].join(", ");
            StaffHelpHour::query().where_raw(
                &format!(
                    "(store_id IN ({marks}) OR staff_id IN \
                     (SELECT id FROM staff WHERE home_store_id IN ({marks})))"
                ),
                ids.iter().chain(ids.iter()).copied(),
            )
        }
    };
    let lines: Vec<HoursLine> = query
        .group_by("staff_id")
        .group_by("store_id")
        .select_as(
            &db,
            "staff_id, store_id, COUNT(DISTINCT worked_on) AS days, CAST(SUM(minutes) AS BIGINT) AS minutes",
        )
        .await?;
    // [/explain:multistore.help.hours.query]
    let staff = Staff::find_many(&db, lines.iter().map(|l| l.staff_id).collect::<Vec<_>>()).await?;
    let users = renox::db::relations::belongs_to::<User, _, _>(&db, &staff, |s| s.user_id).await?;
    let stores: HashMap<i64, String> = Store::all_by_name(&db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    let name_of = |staff_id: i64| {
        staff
            .iter()
            .find(|s| s.id == staff_id)
            .and_then(|s| {
                users.get(&s.user_id).map(|u| {
                    (
                        u.name.clone(),
                        stores.get(&s.home_store_id).cloned().unwrap_or_default(),
                    )
                })
            })
            .unwrap_or_default()
    };
    let mut rows: Vec<_> = lines
        .iter()
        .map(|l| {
            let (name, home) = name_of(l.staff_id);
            json!({
                "name": name,
                "home": home,
                "store": stores.get(&l.store_id).cloned().unwrap_or_default(),
                "days": l.days,
                "hours": l.minutes as f64 / 60.0,
            })
        })
        .collect();
    rows.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let total: i64 = lines.iter().map(|l| l.minutes).sum();
    let _ = user;
    Ok(view(
        "multistore/help/hours.html",
        context! { rows, total_hours => total as f64 / 60.0 },
    ))
}
