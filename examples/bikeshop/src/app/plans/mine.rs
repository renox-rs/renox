//! The customer's plans: every bike's plan with its next visit, visits
//! done and invoices (`/plans/mine`), and one plan's page
//! (`/plans/mine/{subscription}`): the upcoming visits on a calendar, skip
//! or move one, change plan, pause, cancel or resume.
//!
//! Money changes go through renox-billing (`Billing::of(&state, &user)
//! .named("bike-…")`: `swap`, `cancel`, `resume`), which calls the gateway
//! and emits the events [`super::sync`] mirrors. Pausing is the shop's own
//! (the gateway keeps charging: the plan keeps its slot and price), and so
//! are skipping and moving visits.
//!
//! **Paid up first.** renox-billing's `require_subscription` guard checks a
//! user's `default` subscription; a customer here has one subscription per
//! bike, so the visit routes ask the bike's own subscription
//! ([`paid_up`]) and send the customer back to the plan's page with a
//! message while a payment is due, as the guard would.

use std::collections::HashMap;

use renox::chrono::NaiveDate;
use renox::prelude::*;
use renox_billing::Billing;
use serde::{Deserialize, Serialize};

use super::billing::{PayWith, billing_key};
use super::model::{
    PlanInvoice, PlanSubscription, PlanVisit, ServicePlan, SubscriptionStatus, VisitStatus,
};
use super::visits::{self, MoveForm};
use crate::app::rentals::booking::to_local;
use crate::app::rentals::customer_of;
use crate::app::staff::model::Store;
use crate::app::workshop::capacity;
use crate::app::workshop::model::{CustomerBike, WorkOrder};

/// The customer's own subscription `id`, or a 404.
pub async fn own_subscription(db: &Db, user: &User, id: i64) -> Result<PlanSubscription> {
    let customer = customer_of(db, user).await?;
    let sub = PlanSubscription::find_or_404(db, id).await?;
    let owns = CustomerBike::query()
        .where_(CustomerBike::ID.eq(sub.customer_bike_id))
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .exists(db)
        .await?;
    if owns { Ok(sub) } else { Err(Error::NotFound) }
}

/// Whether the plan is paid up: its renox-billing subscription is valid
/// (paid, or cancelled with time left) for a plan paid online, and it
/// isn't on hold. A plan paid at the counter is paid up unless on hold.
pub async fn paid_up(state: &AppState, user: &User, sub: &PlanSubscription) -> Result<bool> {
    if sub.held_at.is_some() {
        return Ok(false);
    }
    if !sub.online() {
        return Ok(true);
    }
    Billing::of(state, user)
        .named(sub.billing_name())
        .subscribed()
        .await
}

/// A plan in the list.
#[derive(Serialize, Debug, Clone)]
pub struct Line {
    pub subscription: PlanSubscription,
    pub status: &'static str,
    pub bike: String,
    pub plan: Option<ServicePlan>,
    pub store: String,
    pub done: i64,
    pub last_invoice: Option<PlanInvoice>,
}

/// The state shown for a plan: on hold, ending, or its status.
pub fn shown_status(sub: &PlanSubscription) -> &'static str {
    if sub.status == SubscriptionStatus::Active && sub.held_at.is_some() {
        "held"
    } else if sub.status == SubscriptionStatus::Active && sub.ends_on.is_some() {
        "ending"
    } else {
        sub.status.as_str()
    }
}

/// `GET /plans/mine` (`plans.mine`): the customer's plans, in six queries
/// whatever their number.
// [explain:plans.mine.handler]
pub async fn index(State(state): State<AppState>, user: AuthUser) -> Result<View> {
    let db = &state.db;
    let customer = customer_of(db, &user).await?;
    let bikes = CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .get(db)
        .await?;
    let subs = PlanSubscription::query()
        .where_(
            PlanSubscription::CUSTOMER_BIKE_ID
                .is_in(bikes.iter().map(|b| b.id).collect::<Vec<_>>()),
        )
        .order_by_desc(PlanSubscription::ID)
        .get(db)
        .await?;
    let lines = lines(db, subs, &bikes).await?;
    let (current, past): (Vec<Line>, Vec<Line>) = lines
        .into_iter()
        .partition(|l| l.subscription.status != SubscriptionStatus::Cancelled);
    Ok(view(
        "plans/mine.html",
        context! { current, past, has_bikes => !bikes.is_empty() },
    ))
}
// [/explain:plans.mine.handler]

// [explain:plans.mine.query]
async fn lines(db: &Db, subs: Vec<PlanSubscription>, bikes: &[CustomerBike]) -> Result<Vec<Line>> {
    let plans = ServicePlan::find_many(
        db,
        subs.iter().map(|s| s.service_plan_id).collect::<Vec<_>>(),
    )
    .await?;
    let stores = Store::all_by_name(db).await?;
    let ids: Vec<i64> = subs.iter().map(|s| s.id).collect();
    let done: Vec<(i64, i64)> = PlanVisit::query()
        .where_(PlanVisit::PLAN_SUBSCRIPTION_ID.is_in(ids.clone()))
        .where_(PlanVisit::STATUS.eq(VisitStatus::Done))
        .group_by("plan_subscription_id")
        .select_as(db, "plan_subscription_id, COUNT(*)")
        .await?;
    let done: HashMap<i64, i64> = done.into_iter().collect();
    let invoices = PlanInvoice::query()
        .where_(PlanInvoice::PLAN_SUBSCRIPTION_ID.is_in(ids))
        .order_by_desc(PlanInvoice::ID)
        .get(db)
        .await?;
    // [/explain:plans.mine.query]
    Ok(subs
        .into_iter()
        .map(|s| Line {
            status: shown_status(&s),
            bike: bikes
                .iter()
                .find(|b| b.id == s.customer_bike_id)
                .map(|b| b.name.clone())
                .unwrap_or_default(),
            plan: plans.iter().find(|p| p.id == s.service_plan_id).cloned(),
            store: stores
                .iter()
                .find(|st| st.id == s.store_id)
                .map(|st| st.name.clone())
                .unwrap_or_default(),
            done: done.get(&s.id).copied().unwrap_or(0),
            last_invoice: invoices
                .iter()
                .find(|i| i.plan_subscription_id == s.id)
                .cloned(),
            subscription: s,
        })
        .collect())
}

/// A visit on the plan's page.
#[derive(Serialize, Debug, Clone)]
pub struct VisitRow {
    pub visit: PlanVisit,
    pub status: &'static str,
    /// The work order's day (local), when it has one.
    pub day: Option<String>,
    pub time: Option<String>,
    pub work_order_id: Option<i64>,
    /// Skip and move are offered.
    pub changeable: bool,
}

/// What the renox-billing subscription says, for the page.
#[derive(Serialize, Debug, Clone, Default)]
pub struct BillingState {
    pub gateway: String,
    pub renews_on: Option<DateTime>,
    pub ends_at: Option<DateTime>,
    pub past_due: bool,
    pub can_resume: bool,
}

/// `GET /plans/mine/{subscription}` (`plans.show`): one plan: its state
/// and billing, the upcoming visits on a month calendar and as a list
/// (skip, move), the past visits, the invoices, and the changes.
pub async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let sub = own_subscription(db, &user, id).await?;
    let bike = CustomerBike::find_or_404(db, sub.customer_bike_id).await?;
    let plan = ServicePlan::find_or_404(db, sub.service_plan_id).await?;
    let next_plan = match sub.next_plan_id {
        Some(id) => ServicePlan::find(db, id).await?,
        None => None,
    };
    let store = Store::find(db, sub.store_id).await?;
    let visits = PlanVisit::query()
        .where_(PlanVisit::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .order_by_desc(PlanVisit::SEQ)
        .limit(40)
        .get(db)
        .await?;
    let orders = WorkOrder::find_many(
        db,
        visits
            .iter()
            .filter_map(|v| v.work_order_id)
            .collect::<Vec<_>>(),
    )
    .await?;
    let rows: Vec<VisitRow> = visits
        .into_iter()
        .map(|visit| {
            let order = orders.iter().find(|o| Some(o.id) == visit.work_order_id);
            let local = order.map(|o| to_local(&state.config, o.scheduled_for));
            VisitRow {
                status: visit.status.as_str(),
                day: local.map(|l| l.date().to_string()),
                time: local.map(|l| l.format("%H:%M").to_string()),
                work_order_id: order.map(|o| o.id),
                changeable: visits::changeable(&sub, &visit, order).is_ok(),
                visit,
            }
        })
        .collect();
    let (upcoming, past): (Vec<VisitRow>, Vec<VisitRow>) = rows
        .into_iter()
        .partition(|r| matches!(r.visit.status, VisitStatus::Scheduled | VisitStatus::Held));
    let mut upcoming = upcoming;
    upcoming.reverse();
    let events: Vec<renox::serde_json::Value> = upcoming
        .iter()
        .filter_map(|r| {
            let day = r.day.clone()?;
            Some(json!({
                "date": day,
                "time": r.time,
                "title": plan.name,
                "kind": if r.visit.status == VisitStatus::Held { "warning" } else { "info" },
            }))
        })
        .collect();
    let invoices = PlanInvoice::query()
        .where_(PlanInvoice::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .order_by_desc(PlanInvoice::ID)
        .get(db)
        .await?;
    // [explain:plans.show.billing]
    let billing = if sub.online() {
        let customer = Billing::of(&state, &*user).named(sub.billing_name());
        let can_resume = customer.can_resume().await?;
        customer.subscription().await?.map(|s| BillingState {
            gateway: s.gateway.clone(),
            renews_on: s.current_period_end,
            ends_at: s.ends_at,
            past_due: s.past_due(),
            can_resume,
        })
    } else {
        None
    };
    // [/explain:plans.show.billing]
    let others: Vec<(String, String)> = ServicePlan::query()
        .where_(ServicePlan::ACTIVE.eq(true))
        .order_by(ServicePlan::PRICE)
        .get(db)
        .await?
        .into_iter()
        .filter(|p| p.id != plan.id)
        .map(|p| (p.slug.clone(), p.name.clone()))
        .collect();
    // The days a moved visit can't land on: full at the home store for
    // the plan's minutes, or closed.
    let minutes: i64 = visits::plan_tasks(db, plan.id)
        .await?
        .iter()
        .map(|t| t.minutes)
        .sum();
    let (full, closed) = match &store {
        Some(store) => (
            capacity::full_days(db, &state.config, store, minutes).await?,
            capacity::closed_weekdays(store),
        ),
        None => (Vec::new(), Vec::new()),
    };
    let today = visits::today(&state.config);
    let month = upcoming
        .first()
        .and_then(|r| r.day.clone())
        .unwrap_or_else(|| today.to_string())[..7]
        .to_owned();
    // A Xendit plan is charged in rupiah (billing.rs): its own price.
    let (monthly, monthly_currency) = if sub.gateway == "xendit" {
        (
            super::billing::xendit_price(plan.monthly_price()),
            super::billing::XENDIT_CURRENCY.to_owned(),
        )
    } else {
        (plan.monthly_price(), state.config.currency.clone())
    };
    let rate = crate::money::format(
        super::billing::RUPIAH_PER_DOLLAR,
        super::billing::XENDIT_CURRENCY,
        &state.current_lang().locale,
    );
    Ok(view(
        "plans/show.html",
        context! {
            status => shown_status(&sub),
            demo => sub.gateway == "demo",
            monthly,
            monthly_currency,
            rate,
            discount => super::subscribe::percent(plan.parts_discount_bp),
            sub,
            bike,
            plan,
            next_plan,
            store,
            upcoming,
            past,
            events,
            month,
            full,
            closed,
            min_day => (today + renox::chrono::Duration::days(1)).to_string(),
            today => today.to_string(),
            max_day => (today + renox::chrono::Duration::days(capacity::BOOK_AHEAD_DAYS)).to_string(),
            invoices,
            billing,
            others,
        },
    ))
}

/// A toast and the plan's page (a full page load after an htmx sheet).
fn back(state: &AppState, htmx: &Htmx, id: i64, toast: Toast) -> Result<Response> {
    let to = state.url("plans.show", &[&id])?;
    Ok((toast, htmx.redirect(&to)).into_response())
}

/// What renox-billing answered, as a toast: done, its refusal, or "try
/// again" when the gateway didn't answer.
fn outcome<T>(
    state: &AppState,
    htmx: &Htmx,
    id: i64,
    result: Result<T>,
    done: &str,
) -> Result<Response> {
    let lang = state.current_lang();
    match result {
        Ok(_) => back(state, htmx, id, Toast::success(lang.t(done, &[]))),
        Err(Error::BadRequest(message)) => back(state, htmx, id, Toast::error(message)),
        Err(err) => {
            tracing::warn!(error = ?err, "plans: the payment gateway didn't answer");
            back(
                state,
                htmx,
                id,
                Toast::error(lang.t("plans.errors.gateway", &[])),
            )
        }
    }
}

/// Another plan.
#[derive(Deserialize, Validate, Debug)]
pub struct SwapForm {
    #[validate(required, exists("service_plans", "slug"))]
    pub plan: Option<String>,
}

/// `POST /plans/mine/{subscription}/swap` (`plans.swap`): another plan from
/// the next period: renox-billing's `swap` without proration (the gateway
/// charges the new price from the next invoice); the visits follow on the
/// period's first day.
// [explain:plans.show.swap]
pub async fn swap(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<i64>,
    Valid(form): Valid<SwapForm>,
) -> Result<Response> {
    let db = &state.db;
    let mut sub = own_subscription(db, &user, id).await?;
    // [/explain:plans.show.swap]
    let slug = form.plan.unwrap_or_default();
    let plan = ServicePlan::query()
        .where_(ServicePlan::SLUG.eq(slug.as_str()))
        .where_(ServicePlan::ACTIVE.eq(true))
        .first(db)
        .await?
        .ok_or(Error::NotFound)?;
    if !sub.live() {
        return back(
            &state,
            &htmx,
            id,
            Toast::error(state.current_lang().t("plans.errors.not_running", &[])),
        );
    }
    // [explain:plans.show.swap]
    if sub.online() {
        let key = billing_key(&plan.slug, PayWith::of_gateway(&sub.gateway));
        let result = Billing::of(&state, &*user)
            .named(sub.billing_name())
            .swap(&key)
            .await;
        return outcome(&state, &htmx, id, result, "plans.show.swapped");
    }
    // [/explain:plans.show.swap]
    // Paid at the counter: from the next visit on.
    sub.next_plan_id = Some(plan.id);
    sub.swap_on = Some(visits::today(&state.config));
    sub.save_only(db, &["next_plan_id", "swap_on"]).await?;
    back(
        &state,
        &htmx,
        id,
        Toast::success(state.current_lang().t("plans.show.swapped", &[])),
    )
}

/// `POST /plans/mine/{subscription}/pause` (`plans.pause`): no visits until
/// resumed; the upcoming ones leave the workshop's days.
pub async fn pause(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<i64>,
) -> Result<Response> {
    let db = &state.db;
    let mut sub = own_subscription(db, &user, id).await?;
    if sub.status == SubscriptionStatus::Active {
        sub.status = SubscriptionStatus::Paused;
        sub.paused_at = Some(renox::db::now());
        sub.save_only(db, &["status", "paused_at"]).await?;
        visits::cancel_upcoming(db, &sub, None).await?;
    }
    back(
        &state,
        &htmx,
        id,
        Toast::success(state.current_lang().t("plans.show.paused", &[])),
    )
}

/// `POST /plans/mine/{subscription}/unpause` (`plans.unpause`): visits
/// again, from the next due day (the ones passed while paused aren't made
/// up).
pub async fn unpause(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<i64>,
) -> Result<Response> {
    let db = &state.db;
    let mut sub = own_subscription(db, &user, id).await?;
    if sub.status == SubscriptionStatus::Paused {
        sub.status = SubscriptionStatus::Active;
        sub.paused_at = None;
        sub.save_only(db, &["status", "paused_at"]).await?;
        visits::plan_ahead(&state, &mut sub).await?;
    }
    back(
        &state,
        &htmx,
        id,
        Toast::success(state.current_lang().t("plans.show.unpaused", &[])),
    )
}

/// `POST /plans/mine/{subscription}/cancel` (`plans.cancel`): at the end of
/// the period paid for (renox-billing's `cancel`): visits until then. A
/// plan paid at the counter ends now.
pub async fn cancel(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<i64>,
) -> Result<Response> {
    let db = &state.db;
    let mut sub = own_subscription(db, &user, id).await?;
    if sub.online() && sub.status != SubscriptionStatus::Pending {
        let result = Billing::of(&state, &*user)
            .named(sub.billing_name())
            .cancel()
            .await;
        return outcome(&state, &htmx, id, result, "plans.show.cancelled");
    }
    visits::end(db, &mut sub).await?;
    back(
        &state,
        &htmx,
        id,
        Toast::success(state.current_lang().t("plans.show.ended", &[])),
    )
}

/// `POST /plans/mine/{subscription}/resume` (`plans.resume`): takes a
/// cancellation back before the period ends (renox-billing's `resume`; not
/// at Xendit, which stops charging at once).
pub async fn resume(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<i64>,
) -> Result<Response> {
    let sub = own_subscription(&state.db, &user, id).await?;
    let result = Billing::of(&state, &*user)
        .named(sub.billing_name())
        .resume()
        .await;
    outcome(&state, &htmx, id, result, "plans.show.resumed")
}

/// The customer's own visit and its plan, or a 404.
async fn own_visit(db: &Db, user: &User, id: i64) -> Result<(PlanSubscription, PlanVisit)> {
    let visit = PlanVisit::find_or_404(db, id).await?;
    let sub = own_subscription(db, user, visit.plan_subscription_id).await?;
    Ok((sub, visit))
}

/// The visit may change: paid up, still scheduled, its time not come.
/// Otherwise the plan's page with why (renox-billing's guard sends people
/// to the plans page the same way).
async fn check_visit(
    state: &AppState,
    htmx: &Htmx,
    user: &User,
    sub: &PlanSubscription,
    visit: &PlanVisit,
) -> Result<std::result::Result<(), Response>> {
    let lang = state.current_lang();
    if !paid_up(state, user, sub).await? {
        let message = lang.t(visits::Refusal::OnHold.key(), &[]);
        return Ok(Err(back(state, htmx, sub.id, Toast::info(message))?));
    }
    let order = match visit.work_order_id {
        Some(id) => WorkOrder::find(&state.db, id).await?,
        None => None,
    };
    if let Err(refusal) = visits::changeable(sub, visit, order.as_ref()) {
        return Ok(Err(back(
            state,
            htmx,
            sub.id,
            Toast::error(lang.t(refusal.key(), &[])),
        )?));
    }
    Ok(Ok(()))
}

/// `POST /plans/visits/{visit}/skip` (`plans.visits.skip`): this visit
/// won't happen; the next one comes as planned.
pub async fn skip(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<i64>,
) -> Result<Response> {
    let (sub, mut visit) = own_visit(&state.db, &user, id).await?;
    if let Err(refused) = check_visit(&state, &htmx, &user, &sub, &visit).await? {
        return Ok(refused);
    }
    visits::skip(&state.db, &mut visit).await?;
    back(
        &state,
        &htmx,
        sub.id,
        Toast::success(state.current_lang().t("plans.show.skipped", &[])),
    )
}

/// `POST /plans/visits/{visit}/move` (`plans.visits.move`): another day
/// with room at the home store (the workshop's capacity rule).
pub async fn move_visit(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(id): Path<i64>,
    Valid(form): Valid<MoveForm>,
) -> Result<Response> {
    let (sub, visit) = own_visit(&state.db, &user, id).await?;
    if let Err(refused) = check_visit(&state, &htmx, &user, &sub, &visit).await? {
        return Ok(refused);
    }
    let day: NaiveDate = form.day.unwrap_or_default();
    let lang = state.current_lang();
    if let Some(problem) = visits::move_to(&state, &visit, day).await? {
        let mut errors = Errors::new();
        errors.add("day", lang.t(problem.key(), &[]));
        return Err(errors.into());
    }
    back(
        &state,
        &htmx,
        sub.id,
        Toast::success(lang.t("plans.show.moved", &[("day", &day)])),
    )
}
