//! A plan's visits: made ahead as work orders, within the workshop's
//! capacity, and what the customer and the calendar do to them.
//!
//! **Making them ([`plan_ahead`]).** Every morning the scheduled task
//! `plans:visits` ([`super::tasks`]) looks [`AHEAD_DAYS`] days ahead for each
//! running subscription. Visit number `n` is due `n` periods after the
//! plan's first day, on the preferred weekday ([`visit_day`]): every week,
//! fortnight, month or quarter. Each due visit becomes a work order at the
//! home store, source `plan`, with the plan's tasks at no charge (the plan
//! pays for them), booked through the workshop's own
//! [`capacity::book`] (the same transaction and capacity check as an online
//! booking). A full or closed day moves it to the next day with room.
//! A visit is made once: `plan_visits` is unique per subscription and
//! number, and the subscription remembers the next number (`visit_seq`).
//!
//! **What makes none.** A paused plan, a plan on hold after a failed
//! payment, a plan waiting for its first payment, and a cancelled one. Due
//! days that pass while paused are counted and skipped, not made up later.
//!
//! **Afterwards.** The customer may skip a visit, or move it to another day
//! with room ([`capacity::move_booking`], the same rule as rescheduling a
//! booking). A visit whose day passed with the bike never brought in is
//! recorded as **missed** ([`mark_missed`]) and not rolled over. A failed
//! payment takes the upcoming visits off the workshop's days ([`hold`]);
//! the payment coming in books them again ([`release`]). A collected work
//! order makes the visit **done** ([`done`]).

use renox::chrono::{Duration, NaiveDate};
use renox::prelude::*;
use serde::Deserialize;

use super::model::{
    Frequency, PLAN_TASKS, PlanSubscription, PlanVisit, ServicePlan, SubscriptionStatus,
    VisitStatus, visit_day,
};
use crate::app::accounts::preferences::Kind;
use crate::app::rentals::booking::to_local;
use crate::app::rentals::notify::{self, Notice, Tone};
use crate::app::workshop::capacity::{self, DayProblem, NewBooking, day_bounds};
use crate::app::workshop::model::{ServiceTask, WorkOrder, WorkSource, WorkStatus};
use crate::app::workshop::status::customer_of;

/// How far ahead visits are made.
pub const AHEAD_DAYS: i64 = 7;
/// How many days after the due day a visit may land when the workshop is
/// full (the next day with room).
pub const LATEST_SLIP_DAYS: i64 = 14;

/// The shop's date today (`APP_TIMEZONE`).
pub fn today(config: &Config) -> NaiveDate {
    to_local(config, renox::db::now()).date()
}

/// The plan's tasks, at no charge: a plan's visit costs nothing on the day
/// (parts are extra, with the plan's discount).
pub async fn plan_tasks(db: &Db, plan_id: i64) -> Result<Vec<ServiceTask>> {
    let ids: Vec<i64> = PLAN_TASKS.ids(db, plan_id).await?;
    let tasks = ServiceTask::find_many(db, ids).await?;
    Ok(tasks
        .into_iter()
        .map(|mut t| {
            t.price = 0;
            t
        })
        .collect())
}

/// Applies a plan change whose day has come: the next visits follow the
/// new plan.
pub async fn apply_swap(db: &Db, sub: &mut PlanSubscription, today: NaiveDate) -> Result<bool> {
    let (Some(next), Some(on)) = (sub.next_plan_id, sub.swap_on) else {
        return Ok(false);
    };
    if on > today {
        return Ok(false);
    }
    sub.service_plan_id = next;
    sub.next_plan_id = None;
    sub.swap_on = None;
    sub.save_only(db, &["service_plan_id", "next_plan_id", "swap_on"])
        .await?;
    Ok(true)
}

/// Makes the visits of `sub` due in the next [`AHEAD_DAYS`] days (see the
/// module docs); the visits made. Visits due today or before are counted
/// and skipped (a plan paused or on hold doesn't catch up).
pub async fn plan_ahead(state: &AppState, sub: &mut PlanSubscription) -> Result<Vec<PlanVisit>> {
    let db = &state.db;
    let config = &state.config;
    let today = today(config);
    apply_swap(db, sub, today).await?;
    let mut made = Vec::new();
    if !sub.making_visits() {
        return Ok(made);
    }
    let plan = ServicePlan::find_or_404(db, sub.service_plan_id).await?;
    let tasks = plan_tasks(db, plan.id).await?;
    let horizon = today + Duration::days(AHEAD_DAYS);
    loop {
        let due = sub.visit_day(plan.frequency, sub.visit_seq);
        if due > horizon || sub.ends_on.is_some_and(|end| due > end) {
            break;
        }
        // Today's visit was made by an earlier run; one due today now
        // would already be late (visits are expected at 09:00).
        if due > today {
            match book_visit(state, sub, &tasks, due).await? {
                Some(visit) => made.push(visit),
                // No room within LATEST_SLIP_DAYS: tried again tomorrow.
                None => break,
            }
        }
        sub.visit_seq += 1;
    }
    sub.next_visit_on = next_visit_on(db, sub, plan.frequency).await?;
    sub.save_only(db, &["visit_seq", "next_visit_on"]).await?;
    Ok(made)
}

/// The next visit's day: the earliest scheduled visit's work order, else
/// the next one the plan will make.
async fn next_visit_on(
    db: &Db,
    sub: &PlanSubscription,
    frequency: Frequency,
) -> Result<Option<NaiveDate>> {
    let ids: Vec<i64> = PlanVisit::query()
        .where_(PlanVisit::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .where_(PlanVisit::STATUS.eq(VisitStatus::Scheduled))
        .where_(PlanVisit::WORK_ORDER_ID.is_not_null())
        .pluck(db, PlanVisit::WORK_ORDER_ID)
        .await?;
    let first: Option<DateTime> = WorkOrder::query()
        .where_(WorkOrder::ID.is_in(ids))
        .order_by(WorkOrder::SCHEDULED_FOR)
        .first(db)
        .await?
        .map(|o| o.scheduled_for);
    if let Some(at) = first {
        return Ok(Some(at.date_naive()));
    }
    if !sub.live() {
        return Ok(None);
    }
    Ok(Some(sub.visit_day(frequency, sub.visit_seq)))
}

/// Books visit number `sub.visit_seq`, due on `due`, at the first day from
/// `due` with room (the visit row and its work order). `None` when no day
/// within [`LATEST_SLIP_DAYS`] has room.
async fn book_visit(
    state: &AppState,
    sub: &PlanSubscription,
    tasks: &[ServiceTask],
    due: NaiveDate,
) -> Result<Option<PlanVisit>> {
    let Some(order) = book_order(state, sub, tasks, due).await? else {
        tracing::warn!(subscription = sub.id, %due, "no workshop day with room for a plan visit");
        return Ok(None);
    };
    let visit = PlanVisit::create(
        &state.db,
        PlanVisit {
            plan_subscription_id: sub.id,
            seq: sub.visit_seq,
            due_on: due,
            work_order_id: Some(order.id),
            status: VisitStatus::Scheduled,
            ..Default::default()
        },
    )
    .await?;
    tell(
        state,
        &order,
        "plans-visit-booked",
        "plans.mail.visit.title",
        "plans.mail.visit.body",
        Tone::Info,
    )
    .await?;
    Ok(Some(visit))
}

/// A work order for a plan visit on the first day from `from` with room.
async fn book_order(
    state: &AppState,
    sub: &PlanSubscription,
    tasks: &[ServiceTask],
    from: NaiveDate,
) -> Result<Option<WorkOrder>> {
    for slip in 0..=LATEST_SLIP_DAYS {
        let day = from + Duration::days(slip);
        let booked = capacity::book(
            &state.db,
            &state.config,
            NewBooking {
                bike_id: sub.customer_bike_id,
                store_id: sub.store_id,
                day,
                tasks: tasks.to_vec(),
                package: None,
                note: None,
                source: WorkSource::Plan,
                checked_in: false,
            },
        )
        .await?;
        match booked {
            Ok(mut order) => {
                order.plan_subscription_id = Some(sub.id);
                order
                    .save_only(&state.db, &["plan_subscription_id"])
                    .await?;
                return Ok(Some(order));
            }
            Err(DayProblem::Full | DayProblem::Closed) => continue,
            Err(DayProblem::Past | DayProblem::TooFar) => return Ok(None),
        }
    }
    Ok(None)
}

/// Tells the bike's owner about a visit (mail + the bell).
async fn tell(
    state: &AppState,
    order: &WorkOrder,
    kind: &'static str,
    title: &'static str,
    body: &'static str,
    tone: Tone,
) -> Result {
    let Some(customer) = customer_of(&state.db, order).await? else {
        return Ok(());
    };
    let day = to_local(&state.config, order.scheduled_for)
        .format("%Y-%m-%d")
        .to_string();
    let url = crate::app::rentals::link(state, "plans.mine", None::<i64>)?;
    notify::customer(
        state,
        &customer,
        Kind::Plan,
        &Notice::new(kind, title, body)
            .param("day", &day)
            .param("number", order.id)
            .row("workshop.fields.day", &day)
            .tone(tone)
            .view("mail/plans/notice")
            .url(url),
    )
    .await
}

/// Takes a scheduled visit's work order off the workshop's day (cancelled,
/// so its minutes free up) and gives the visit `status`.
async fn release_slot(db: &Db, visit: &mut PlanVisit, status: VisitStatus) -> Result {
    if let Some(id) = visit.work_order_id
        && let Some(mut order) = WorkOrder::find(db, id).await?
        && order.status == WorkStatus::Booked
    {
        order.status = WorkStatus::Cancelled;
        order.cancelled_at = Some(renox::db::now());
        order.save_only(db, &["status", "cancelled_at"]).await?;
    }
    visit.status = status;
    visit.save_only(db, &["status"]).await?;
    Ok(())
}

/// Why a visit can't be changed by the customer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It isn't scheduled any more (done, skipped, missed…).
    NotScheduled,
    /// The bike is already at the workshop, or the day has begun.
    TooLate,
    /// The plan is on hold until a payment comes in.
    OnHold,
}

impl Refusal {
    /// The translation key of the message.
    pub fn key(self) -> &'static str {
        match self {
            Refusal::NotScheduled => "plans.errors.not_scheduled",
            Refusal::TooLate => "plans.errors.too_late",
            Refusal::OnHold => "plans.errors.on_hold",
        }
    }
}

/// Whether the customer may still skip or move `visit` (its work order
/// still booked, its time not come, the plan paid up).
pub fn changeable(
    sub: &PlanSubscription,
    visit: &PlanVisit,
    order: Option<&WorkOrder>,
) -> std::result::Result<(), Refusal> {
    if sub.held_at.is_some() {
        return Err(Refusal::OnHold);
    }
    if visit.status != VisitStatus::Scheduled {
        return Err(Refusal::NotScheduled);
    }
    match order {
        Some(o) if o.status == WorkStatus::Booked && o.scheduled_for > renox::db::now() => Ok(()),
        _ => Err(Refusal::TooLate),
    }
}

/// Skips `visit`: its work order is cancelled (the slot frees up); the
/// next visit comes as planned.
pub async fn skip(db: &Db, visit: &mut PlanVisit) -> Result {
    release_slot(db, visit, VisitStatus::Skipped).await
}

/// The new day of a visit.
#[derive(Deserialize, Validate, Debug)]
pub struct MoveForm {
    #[validate(required)]
    pub day: Option<NaiveDate>,
}

/// Moves `visit` to `day`, when that day has room (the workshop's own
/// rule, [`capacity::move_booking`]); `Some(problem)` otherwise.
pub async fn move_to(
    state: &AppState,
    visit: &PlanVisit,
    day: NaiveDate,
) -> Result<Option<DayProblem>> {
    let id = visit.work_order_id.ok_or(Error::NotFound)?;
    let mut order = WorkOrder::find_or_404(&state.db, id).await?;
    capacity::move_booking(&state.db, &state.config, &mut order, day).await
}

/// Records the visits whose day has passed without the bike: still
/// booked after their day ended. Their work orders are cancelled, the
/// visits **missed** (not rolled over), the customer told. How many.
pub async fn mark_missed(state: &AppState) -> Result<u64> {
    let db = &state.db;
    let (start_of_today, _) = day_bounds(&state.config, today(&state.config));
    let orders = WorkOrder::query()
        .where_(WorkOrder::SOURCE.eq(WorkSource::Plan))
        .where_(WorkOrder::STATUS.eq(WorkStatus::Booked))
        .where_(WorkOrder::SCHEDULED_FOR.lt(start_of_today))
        .get(db)
        .await?;
    let ids: Vec<i64> = orders.iter().map(|o| o.id).collect();
    let mut visits = PlanVisit::query()
        .where_(PlanVisit::WORK_ORDER_ID.is_in(ids))
        .where_(PlanVisit::STATUS.eq(VisitStatus::Scheduled))
        .get(db)
        .await?;
    for visit in &mut visits {
        release_slot(db, visit, VisitStatus::Missed).await?;
        if let Some(order) = orders.iter().find(|o| Some(o.id) == visit.work_order_id) {
            tell(
                state,
                order,
                "plans-visit-missed",
                "plans.mail.missed.title",
                "plans.mail.missed.body",
                Tone::Warning,
            )
            .await?;
        }
    }
    Ok(visits.len() as u64)
}

/// A payment failed: the plan goes on hold, and its upcoming visits leave
/// the workshop's days (held).
pub async fn hold(db: &Db, sub: &mut PlanSubscription) -> Result {
    if sub.held_at.is_none() {
        sub.held_at = Some(renox::db::now());
        sub.save_only(db, &["held_at"]).await?;
    }
    let mut visits = PlanVisit::query()
        .where_(PlanVisit::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .where_(PlanVisit::STATUS.eq(VisitStatus::Scheduled))
        .get(db)
        .await?;
    for visit in &mut visits {
        release_slot(db, visit, VisitStatus::Held).await?;
    }
    Ok(())
}

/// The payment came in: the hold ends, the held visits whose day hasn't
/// passed are booked again (the first day with room from their due day,
/// or from today), and the next ones are made.
pub async fn release(state: &AppState, sub: &mut PlanSubscription) -> Result {
    let db = &state.db;
    if sub.held_at.is_none() {
        return Ok(());
    }
    sub.held_at = None;
    sub.save_only(db, &["held_at"]).await?;
    let today = today(&state.config);
    let tasks = plan_tasks(db, sub.service_plan_id).await?;
    let held = PlanVisit::query()
        .where_(PlanVisit::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .where_(PlanVisit::STATUS.eq(VisitStatus::Held))
        .get(db)
        .await?;
    for mut visit in held {
        if visit.due_on <= today {
            visit.status = VisitStatus::Missed;
            visit.save_only(db, &["status"]).await?;
            continue;
        }
        if let Some(order) = book_order(state, sub, &tasks, visit.due_on).await? {
            visit.work_order_id = Some(order.id);
            visit.status = VisitStatus::Scheduled;
            visit.save_only(db, &["work_order_id", "status"]).await?;
        }
    }
    plan_ahead(state, sub).await?;
    Ok(())
}

/// Paused, cancelled or ended: the upcoming visits are taken off the
/// workshop's days (cancelled).
pub async fn cancel_upcoming(db: &Db, sub: &PlanSubscription, from: Option<NaiveDate>) -> Result {
    let mut visits = PlanVisit::query()
        .where_(PlanVisit::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .where_(PlanVisit::STATUS.is_in([VisitStatus::Scheduled, VisitStatus::Held]))
        .get(db)
        .await?;
    for visit in &mut visits {
        if from.is_some_and(|day| visit.due_on <= day) {
            continue;
        }
        release_slot(db, visit, VisitStatus::Cancelled).await?;
    }
    Ok(())
}

/// A work order was collected: its visit is done.
pub async fn done(db: &Db, work_order_id: i64) -> Result {
    if let Some(mut visit) = PlanVisit::query()
        .where_(PlanVisit::WORK_ORDER_ID.eq(work_order_id))
        .first(db)
        .await?
        && visit.status == VisitStatus::Scheduled
    {
        visit.status = VisitStatus::Done;
        visit.save_only(db, &["status"]).await?;
    }
    Ok(())
}

/// Ends the plans cancelled at their period's end once that day has
/// passed (status cancelled, the rest of the visits cancelled). How many.
pub async fn end_finished(db: &Db, today: NaiveDate) -> Result<u64> {
    let subs = PlanSubscription::query()
        .where_(
            PlanSubscription::STATUS
                .is_in([SubscriptionStatus::Active, SubscriptionStatus::Paused]),
        )
        .where_(PlanSubscription::ENDS_ON.lt(today))
        .get(db)
        .await?;
    for mut sub in subs.iter().cloned() {
        end(db, &mut sub).await?;
    }
    Ok(subs.len() as u64)
}

/// The plan is over: cancelled now, the upcoming visits cancelled.
pub async fn end(db: &Db, sub: &mut PlanSubscription) -> Result {
    sub.status = SubscriptionStatus::Cancelled;
    sub.cancelled_at.get_or_insert_with(renox::db::now);
    sub.next_visit_on = None;
    sub.save_only(db, &["status", "cancelled_at", "next_visit_on"])
        .await?;
    cancel_upcoming(db, sub, None).await
}

/// `visit_day` re-exported for the pages.
pub fn day_of(sub: &PlanSubscription, frequency: Frequency, seq: i64) -> NaiveDate {
    visit_day(sub.starts_on, sub.preferred_weekday, frequency, seq)
}
