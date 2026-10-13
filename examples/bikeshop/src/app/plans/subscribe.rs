//! The plans page (anyone) and subscribing a bike (customers): choose a
//! bike, a plan, the home store, the weekday and how to pay; the gateway's
//! page takes the payment, its webhook starts the plan ([`super::sync`]).

use std::collections::HashMap;

use renox::chrono::{Datelike, Duration, NaiveDate};
use renox::prelude::*;
use renox::validation::FormContext;
use renox_billing::Billing;
use serde::{Deserialize, Serialize};

use super::billing::{PayWith, billing_key, ways_to_pay};
use super::model::{PlanSubscription, ServicePlan, SubscriptionStatus, billing_name, next_weekday};
use super::visits;
use crate::app::rentals::customer_of;
use crate::app::staff::model::Store;
use crate::app::workshop::capacity::closed_weekdays;
use crate::app::workshop::model::{CustomerBike, ServiceTask};

/// The plan highlighted on the plans page.
pub const HIGHLIGHT: &str = "monthly-tune-up";

/// A plan's card on the plans page (the `compare_plans` block's shape).
#[derive(Serialize, Debug, Clone)]
pub struct Card {
    pub key: String,
    pub name: String,
    /// A month.
    pub price: i64,
    pub interval: &'static str,
    pub description: String,
    pub perks: Vec<String>,
    pub url: String,
    pub cta: String,
}

/// A row of the comparison table.
#[derive(Serialize, Debug, Clone)]
pub struct Row {
    pub label: String,
    pub values: HashMap<String, renox::serde_json::Value>,
}

/// The sold plans and each one's tasks (the `plan_tasks` pivot,
/// `PLAN_TASKS`), in three queries.
// [explain:plans.index.query]
pub async fn plans_with_tasks(db: &Db) -> Result<Vec<(ServicePlan, Vec<ServiceTask>)>> {
    let plans = ServicePlan::query()
        .where_(ServicePlan::ACTIVE.eq(true))
        .order_by(ServicePlan::PRICE)
        .get(db)
        .await?;
    let pairs: Vec<(i64, i64)> =
        renox::db::sql("SELECT service_plan_id, service_task_id FROM plan_tasks")
            .fetch_as::<(i64, i64)>(db)
            .await?;
    let tasks = ServiceTask::query()
        .order_by(ServiceTask::NAME)
        .get(db)
        .await?;
    let mut out: Vec<(ServicePlan, Vec<ServiceTask>)> = plans
        .into_iter()
        .map(|plan| {
            let mine: Vec<ServiceTask> = tasks
                .iter()
                .filter(|t| pairs.contains(&(plan.id, t.id)))
                .cloned()
                .collect();
            (plan, mine)
        })
        .collect();
    out.sort_by_key(|(p, _)| p.monthly_price());
    Ok(out)
}
// [/explain:plans.index.query]

/// `GET /plans` (`plans.index`): the plans side by side (the
/// `compare_plans` block), each a month's price with what it includes, and
/// a table comparing them task by task.
// [explain:plans.index.cards]
pub async fn index(State(state): State<AppState>, lang: Lang) -> Result<View> {
    let plans = plans_with_tasks(&state.db).await?;
    let mut cards = Vec::new();
    for (plan, tasks) in &plans {
        let mut perks = vec![
            lang.t(&format!("plans.every.{}", plan.frequency.as_str()), &[]),
            lang.t(
                "plans.perks.discount",
                &[("percent", &percent(plan.parts_discount_bp))],
            ),
        ];
        perks.extend(tasks.iter().take(3).map(|t| t.name.clone()));
        cards.push(Card {
            key: plan.slug.clone(),
            name: plan.name.clone(),
            price: plan.monthly_price(),
            interval: "month",
            description: plan.description.clone(),
            perks,
            url: format!("{}?plan={}", state.url("plans.subscribe", &[])?, plan.slug),
            cta: lang.t("plans.index.choose", &[]),
        });
    }
    // [/explain:plans.index.cards]
    let mut rows = vec![
        Row {
            label: lang.t("plans.compare.how_often", &[]),
            values: plans
                .iter()
                .map(|(p, _)| {
                    (
                        p.slug.clone(),
                        json!(lang.t(&format!("plans.frequency.{}", p.frequency.as_str()), &[])),
                    )
                })
                .collect(),
        },
        Row {
            label: lang.t("plans.compare.per_visit", &[]),
            values: plans
                .iter()
                .map(|(p, _)| {
                    (
                        p.slug.clone(),
                        json!(crate::app::rentals::reserve::money(&state, p.price)),
                    )
                })
                .collect(),
        },
        Row {
            label: lang.t("plans.compare.discount", &[]),
            values: plans
                .iter()
                .map(|(p, _)| {
                    (
                        p.slug.clone(),
                        json!(format!("{} %", percent(p.parts_discount_bp))),
                    )
                })
                .collect(),
        },
    ];
    let mut all_tasks: Vec<ServiceTask> = plans.iter().flat_map(|(_, t)| t.clone()).collect();
    all_tasks.sort_by(|a, b| a.name.cmp(&b.name));
    all_tasks.dedup_by_key(|t| t.id);
    for task in all_tasks {
        rows.push(Row {
            label: task.name.clone(),
            values: plans
                .iter()
                .map(|(p, tasks)| (p.slug.clone(), json!(tasks.iter().any(|t| t.id == task.id))))
                .collect(),
        });
    }
    rows.push(Row {
        label: lang.t("plans.compare.flexible", &[]),
        values: plans
            .iter()
            .map(|(p, _)| (p.slug.clone(), json!(true)))
            .collect(),
    });
    Ok(view(
        "plans/index.html",
        context! { cards, rows, highlight => HIGHLIGHT },
    ))
}

/// Basis points as a percentage for people: 1000 → `10`, 750 → `7.5`.
pub fn percent(bp: i64) -> String {
    crate::app::rentals::counter::percent(bp)
}

/// The subscribe form, also read from the query string to preselect.
#[derive(Deserialize, Default, Debug)]
pub struct SubscribeForm {
    pub bike: Option<i64>,
    pub plan: Option<String>,
    pub store: Option<i64>,
    /// 1 = Monday … 7 = Sunday.
    pub weekday: Option<i64>,
    pub pay_with: Option<String>,
}

/// The GET form has no rules: every value is optional while choosing.
#[derive(Deserialize, Default, Debug)]
pub struct SubscribeQuery {
    pub bike: Option<i64>,
    pub plan: Option<String>,
    pub store: Option<i64>,
    pub weekday: Option<i64>,
}

impl Validate for SubscribeQuery {
    fn rules(&self, _v: &mut Validator) {}
}

/// The weekdays, Monday first: (1…7, translation key).
pub const WEEKDAYS: [(i64, &str); 7] = [
    (1, "mon"),
    (2, "tue"),
    (3, "wed"),
    (4, "thu"),
    (5, "fri"),
    (6, "sat"),
    (7, "sun"),
];

/// The first visit of a plan subscribed today: the preferred weekday,
/// from tomorrow on.
pub fn first_visit(today: NaiveDate, weekday: i64) -> NaiveDate {
    next_weekday(today + Duration::days(1), weekday)
}

/// The bikes of a customer that have a running plan already.
pub async fn bikes_on_a_plan(db: &Db, bike_ids: Vec<i64>) -> Result<Vec<i64>> {
    PlanSubscription::query()
        .where_(PlanSubscription::CUSTOMER_BIKE_ID.is_in(bike_ids))
        .where_(
            PlanSubscription::STATUS
                .is_in([SubscriptionStatus::Active, SubscriptionStatus::Paused]),
        )
        .pluck(db, PlanSubscription::CUSTOMER_BIKE_ID)
        .await
}

/// `GET /plans/subscribe` (`plans.subscribe`): the form, with the chosen
/// plan's monthly price and the first visit's day in a summary.
pub async fn form(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    Valid(query): Valid<SubscribeQuery>,
) -> Result<View> {
    let db = &state.db;
    let customer = customer_of(db, &user).await?;
    let bikes = CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .order_by(CustomerBike::NAME)
        .get(db)
        .await?;
    let taken = bikes_on_a_plan(db, bikes.iter().map(|b| b.id).collect()).await?;
    let bike_options: Vec<(i64, String)> = bikes
        .iter()
        .filter(|b| !taken.contains(&b.id))
        .map(|b| (b.id, b.name.clone()))
        .collect();
    let plans = ServicePlan::query()
        .where_(ServicePlan::ACTIVE.eq(true))
        .order_by(ServicePlan::PRICE)
        .get(db)
        .await?;
    let chosen = query
        .plan
        .as_deref()
        .and_then(|slug| plans.iter().find(|p| p.slug == slug))
        .or_else(|| plans.iter().find(|p| p.slug == HIGHLIGHT))
        .or(plans.first())
        .cloned();
    let plan_options: Vec<(String, String, String)> = plans
        .iter()
        .map(|p| {
            (
                p.slug.clone(),
                format!(
                    "{} · {} / {}",
                    p.name,
                    crate::app::rentals::reserve::money(&state, p.monthly_price()),
                    lang.t("blocks.plans.month", &[])
                ),
                lang.t(&format!("plans.every.{}", p.frequency.as_str()), &[]),
            )
        })
        .collect();
    let stores = Store::all_by_name(db).await?;
    let store = query
        .store
        .and_then(|id| stores.iter().find(|s| s.id == id))
        .or(stores.first())
        .cloned();
    let closed = store.as_ref().map(closed_weekdays).unwrap_or_default();
    let weekdays: Vec<(i64, String)> = WEEKDAYS
        .iter()
        .filter(|(n, _)| !closed.contains(&((*n % 7) as u32)))
        .map(|(n, key)| (*n, lang.t(&format!("plans.weekday.{key}"), &[])))
        .collect();
    let weekday = query
        .weekday
        .filter(|w| weekdays.iter().any(|(n, _)| n == w))
        .or_else(|| weekdays.first().map(|(n, _)| *n));
    let today = visits::today(&state.config);
    let first = weekday.map(|w| first_visit(today, w).to_string());
    let rate = crate::money::format(
        super::billing::RUPIAH_PER_DOLLAR,
        super::billing::XENDIT_CURRENCY,
        &lang.locale,
    );
    let ways: Vec<(String, String)> = ways_to_pay(&state.config)
        .into_iter()
        .map(|(with, gateway)| {
            (
                with.key().to_owned(),
                // Xendit charges rupiah: its label names the shop's rate.
                lang.t(&format!("plans.pay_with.{gateway}"), &[("rate", &rate)]),
            )
        })
        .collect();
    let pay_with = ways.first().map(|(k, _)| k.clone());
    Ok(view(
        "plans/subscribe.html",
        context! {
            bikes => bike_options,
            has_bikes => !bikes.is_empty(),
            bike => query.bike,
            plans => plan_options,
            monthly => chosen.as_ref().map(|p| p.monthly_price()),
            plan => chosen,
            stores => stores.iter().map(|s| (s.id, s.name.clone())).collect::<Vec<_>>(),
            store_id => store.as_ref().map(|s| s.id),
            weekdays,
            weekday,
            first,
            ways,
            pay_with,
        },
    ))
}

impl Validate for SubscribeForm {
    fn rules(&self, v: &mut Validator) {
        v.field("bike", &self.bike).required();
        v.field("plan", &self.plan)
            .required()
            .exists("service_plans", "slug");
        v.field("store", &self.store)
            .required()
            .exists("stores", "id");
        v.field("weekday", &self.weekday).required().min(1).max(7);
        v.field("pay_with", &self.pay_with)
            .required()
            .one_of(&["card", "xendit"]);
    }

    /// The bike is the customer's and has no plan yet ("a bike can have one
    /// active plan"), the plan is sold, the store opens that weekday, and
    /// that way of paying is set up.
    // [explain:plans.subscribe.form]
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let db = &form.state.db;
        let lang = form.state.current_lang();
        let Some(user) = form.user else {
            return Ok(());
        };
        let customer = customer_of(db, user).await?;
        let bike = self.bike.unwrap_or_default();
        let owns = CustomerBike::query()
            .where_(CustomerBike::ID.eq(bike))
            .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
            .exists(db)
            .await?;
        if !owns {
            errors.add("bike", lang.t("plans.errors.bike", &[]));
        } else if !bikes_on_a_plan(db, vec![bike]).await?.is_empty() {
            errors.add("bike", lang.t("plans.errors.one_plan", &[]));
        }
        // [/explain:plans.subscribe.form]
        if let Some(slug) = &self.plan
            && !ServicePlan::query()
                .where_(ServicePlan::SLUG.eq(slug.as_str()))
                .where_(ServicePlan::ACTIVE.eq(true))
                .exists(db)
                .await?
        {
            errors.add("plan", lang.t("plans.errors.not_sold", &[]));
        }
        if let (Some(store), Some(weekday)) = (
            Store::find(db, self.store.unwrap_or_default()).await?,
            self.weekday,
        ) && closed_weekdays(&store).contains(&((weekday.rem_euclid(7)) as u32))
        {
            errors.add("weekday", lang.t("plans.errors.closed", &[]));
        }
        // [explain:plans.subscribe.form]
        let with = self.pay_with.as_deref().and_then(PayWith::from_key);
        if with.is_some_and(|w| !ways_to_pay(&form.state.config).iter().any(|(x, _)| *x == w)) {
            errors.add("pay_with", lang.t("plans.errors.pay_with", &[]));
        }
        Ok(())
        // [/explain:plans.subscribe.form]
    }
}

/// `POST /plans/subscribe` (`plans.subscribe.store`): the plan, pending,
/// then renox-billing's checkout for the bike's subscription
/// (`Billing::of(&state, &user).named("bike-…").checkout(…)`): the
/// customer goes to the gateway's page; the webhook starts the plan.
// [explain:plans.subscribe.checkout]
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Valid(form): Valid<SubscribeForm>,
) -> Result<Response> {
    // [/explain:plans.subscribe.checkout]
    let db = &state.db;
    let bike = form.bike.unwrap_or_default();
    let slug = form.plan.clone().unwrap_or_default();
    let plan = ServicePlan::query()
        .where_(ServicePlan::SLUG.eq(slug.as_str()))
        .first(db)
        .await?
        .ok_or(Error::NotFound)?;
    let with = form
        .pay_with
        .as_deref()
        .and_then(PayWith::from_key)
        .unwrap_or(PayWith::Card);
    let weekday = form.weekday.unwrap_or(1);
    let today = visits::today(&state.config);
    // [explain:plans.subscribe.checkout]
    // An abandoned checkout of this bike is reused, not left behind.
    let pending = PlanSubscription::query()
        .where_(PlanSubscription::CUSTOMER_BIKE_ID.eq(bike))
        .where_(PlanSubscription::STATUS.eq(SubscriptionStatus::Pending))
        .first(db)
        .await?;
    let mut sub = pending.unwrap_or_default();
    sub.customer_bike_id = bike;
    sub.service_plan_id = plan.id;
    sub.store_id = form.store.unwrap_or_default();
    sub.preferred_weekday = weekday;
    sub.status = SubscriptionStatus::Pending;
    sub.starts_on = first_visit(today, weekday);
    sub.next_visit_on = Some(sub.starts_on);
    sub.visit_seq = 0;
    sub.user_id = Some(user.id);
    sub.save(db).await?;
    let lang = state.current_lang();
    let checkout = Billing::of(&state, &*user)
        .named(billing_name(bike))
        .checkout(&billing_key(&plan.slug, with))
        .await;
    match checkout {
        Ok(url) => Ok(if htmx.request {
            HxRedirect(url).into_response()
        } else {
            Redirect::to(&url).into_response()
        }),
        // [/explain:plans.subscribe.checkout]
        Err(Error::BadRequest(message)) => {
            let mut errors = Errors::new();
            errors.add("plan", message);
            Err(errors.into())
        }
        Err(err) => {
            tracing::warn!(error = ?err, "plans: the payment gateway didn't answer");
            let mut errors = Errors::new();
            errors.add("pay_with", lang.t("plans.errors.gateway", &[]));
            Err(errors.into())
        }
    }
}

/// The weekday number (1 = Monday) of a date, for the pages.
pub fn weekday_of(day: NaiveDate) -> i64 {
    day.weekday().number_from_monday() as i64
}
