//! The pages: the plans, starting a checkout or a trial, coming back from
//! the gateway, changing plan, canceling, resuming and pausing. All for the
//! logged-in user's `default` subscription.

use renox::axum::middleware::from_fn;
use renox::prelude::*;
use serde::Serialize;

use crate::model::Subscription;
use crate::webhook::{BillingWebhook, mark};
use crate::{Billing, Customer, Plan, Setup};

/// Every route, named `billing.*`.
pub(crate) fn routes() -> Routes {
    let pages = Routes::new()
        .get("/billing", plans)
        .name("billing.plans")
        .post("/billing/checkout/{plan}", checkout)
        .name("billing.checkout")
        .post("/billing/trial/{plan}", trial)
        .name("billing.trial")
        .get("/billing/return", back)
        .name("billing.return")
        .post("/billing/swap/{plan}", swap)
        .name("billing.swap")
        .post("/billing/cancel", cancel)
        .name("billing.cancel")
        .post("/billing/resume", resume)
        .name("billing.resume")
        .post("/billing/pause", pause)
        .name("billing.pause")
        .post("/billing/unpause", unpause)
        .name("billing.unpause")
        .require_auth();
    let webhooks = Routes::new()
        .webhook::<BillingWebhook>("/billing/webhooks/{gateway}")
        .route_layer(from_fn(mark));
    pages.merge(webhooks)
}

/// A plan as the plans page shows it.
#[derive(Serialize)]
struct PlanCard {
    key: String,
    label: String,
    description: Option<String>,
    price: String,
    trial_days: u32,
    features: Vec<String>,
    current: bool,
}

/// The subscription as the pages show it.
#[derive(Serialize)]
pub(crate) struct Current {
    plan: String,
    plan_label: String,
    price: Option<String>,
    status: String,
    status_label: &'static str,
    valid: bool,
    on_trial: bool,
    generic_trial: bool,
    trial_ends_at: Option<DateTime>,
    canceled: bool,
    on_grace_period: bool,
    ends_at: Option<DateTime>,
    current_period_end: Option<DateTime>,
    past_due: bool,
    can_resume: bool,
    paused: bool,
    can_pause: bool,
    gateway: String,
}

impl Current {
    pub(crate) async fn of(
        state: &AppState,
        setup: &Setup,
        billing: &Customer<'_>,
        subscription: Subscription,
    ) -> Result<Self> {
        let plan = setup.plan(&subscription.plan).ok();
        let status_label = if subscription.paused() {
            "Paused"
        } else if subscription.on_grace_period() {
            "Canceled"
        } else if subscription.on_trial() {
            "Trial"
        } else if subscription.ended() {
            "Ended"
        } else {
            match subscription.status.as_str() {
                "active" => "Active",
                "past_due" => "Past due",
                "paused" => "Paused",
                "canceled" => "Canceled",
                "trialing" => "Trial over",
                _ => "Not paid yet",
            }
        };
        let gateway = setup
            .gateway(state, &subscription.gateway)
            .map(|g| g.label().to_owned())
            .unwrap_or_default();
        Ok(Self {
            plan_label: plan.map_or_else(|| subscription.plan.clone(), |p| p.label.clone()),
            price: plan.map(|p| p.price_label(&state.config.locale)),
            plan: subscription.plan.clone(),
            status: subscription.status.to_string(),
            status_label,
            valid: subscription.valid(),
            on_trial: subscription.on_trial(),
            generic_trial: subscription.is_generic_trial(),
            trial_ends_at: subscription.trial_ends_at,
            canceled: subscription.canceled(),
            on_grace_period: subscription.on_grace_period(),
            ends_at: subscription.ends_at,
            current_period_end: subscription.current_period_end,
            past_due: subscription.past_due(),
            can_resume: billing.can_resume().await?,
            paused: subscription.paused(),
            can_pause: billing.can_pause().await?,
            gateway,
        })
    }
}

/// The plans, with the user's own marked.
async fn plans(State(state): State<AppState>, user: AuthUser) -> Result<View> {
    let setup = crate::setup(&state)?;
    let billing = Billing::of(&state, &user);
    let subscription = billing.subscription().await?;
    let trial_available = subscription.is_none();
    let current = match subscription {
        Some(s) => Some(Current::of(&state, &setup, &billing, s).await?),
        None => None,
    };
    let subscribed = current.as_ref().is_some_and(|c| c.valid);
    let cards: Vec<PlanCard> = setup
        .plans
        .iter()
        .map(|plan: &Plan| PlanCard {
            key: plan.key.clone(),
            label: plan.label.clone(),
            description: plan.description.clone(),
            price: plan.price_label(&state.config.locale),
            trial_days: plan.trial_days,
            features: plan.features.clone(),
            // A canceled plan can be subscribed to again.
            current: current
                .as_ref()
                .is_some_and(|c| c.valid && !c.canceled && c.plan == plan.key),
        })
        .collect();
    Ok(view(
        "billing/plans.html",
        context! {
            plans => cards,
            current,
            subscribed,
            trial_available,
            generic_trials => setup.generic_trials,
            account => account(&state),
        },
    ))
}

/// The account page when the app has it, else `/`.
fn account(state: &AppState) -> String {
    state
        .url("account.show", &[])
        .unwrap_or_else(|_| "/".into())
}

/// Where to go after a change: the module's `redirect_to`, else the
/// account page.
fn after(state: &AppState) -> String {
    crate::setup(state)
        .ok()
        .and_then(|setup| setup.redirect_to.clone())
        .unwrap_or_else(|| account(state))
}

/// Sends the browser to `to`, the htmx way when htmx asked.
fn go(htmx: &Htmx, to: &str) -> Response {
    if htmx.request {
        HxRedirect(to.to_owned()).into_response()
    } else {
        Redirect::to(to).into_response()
    }
}

/// What happened, as a toast on the next page: `Ok` → `done`; a refusal
/// (`Error::BadRequest`) says why; anything else (the gateway didn't
/// answer) is logged and the user is asked to try again.
fn outcome<T>(htmx: &Htmx, result: Result<T>, done: &str, to: &str, back: &str) -> Response {
    match result {
        Ok(_) => (Toast::success(done), go(htmx, to)).into_response(),
        Err(Error::BadRequest(message)) => (Toast::error(message), go(htmx, back)).into_response(),
        Err(err) => {
            tracing::warn!(error = ?err, "billing: the payment provider call failed");
            (
                Toast::error("The payment provider didn't answer. Please try again."),
                go(htmx, back),
            )
                .into_response()
        }
    }
}

/// To the gateway's payment page.
async fn checkout(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(plan): Path<String>,
) -> Result<Response> {
    let plans = state.url("billing.plans", &[])?;
    match Billing::of(&state, &user).checkout(&plan).await {
        Ok(url) => Ok(go(&htmx, &url)),
        Err(err) => Ok(outcome::<()>(&htmx, Err(err), "", &plans, &plans)),
    }
}

/// A free trial without a payment method.
async fn trial(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(plan): Path<String>,
) -> Result<Response> {
    let setup = crate::setup(&state)?;
    if !setup.generic_trials {
        return Err(Error::NotFound);
    }
    let plans = state.url("billing.plans", &[])?;
    let result = Billing::of(&state, &user).start_trial(&plan).await;
    Ok(outcome(
        &htmx,
        result,
        "Your free trial has started.",
        &after(&state),
        &plans,
    ))
}

/// Back from the gateway's page. The subscription arrives by webhook,
/// usually within seconds.
async fn back(State(state): State<AppState>, _user: AuthUser) -> Response {
    (
        Toast::success("Thank you! Your subscription starts as soon as the payment is confirmed."),
        Redirect::to(&after(&state)),
    )
        .into_response()
}

/// Another plan.
async fn swap(
    State(state): State<AppState>,
    user: AuthUser,
    htmx: Htmx,
    Path(plan): Path<String>,
) -> Result<Response> {
    let plans = state.url("billing.plans", &[])?;
    let result = Billing::of(&state, &user).swap(&plan).await;
    Ok(outcome(
        &htmx,
        result,
        "Your plan has changed.",
        &after(&state),
        &plans,
    ))
}

/// Cancels at the end of the period.
async fn cancel(State(state): State<AppState>, user: AuthUser, htmx: Htmx) -> Response {
    let result = Billing::of(&state, &user).cancel().await;
    let done = match &result {
        Ok(s) if s.on_grace_period() => {
            "Your subscription is canceled. You keep access until the end of the period."
        }
        _ => "Your subscription is canceled.",
    };
    let to = after(&state);
    outcome(&htmx, result, done, &to, &to)
}

/// Takes the cancellation back.
async fn resume(State(state): State<AppState>, user: AuthUser, htmx: Htmx) -> Response {
    let result = Billing::of(&state, &user).resume().await;
    let to = after(&state);
    outcome(&htmx, result, "Your subscription goes on.", &to, &to)
}

/// Stops billing for a while.
async fn pause(State(state): State<AppState>, user: AuthUser, htmx: Htmx) -> Response {
    let result = Billing::of(&state, &user).pause(None).await;
    let to = after(&state);
    outcome(
        &htmx,
        result,
        "Your subscription is paused. Nothing is charged until you take it up again.",
        &to,
        &to,
    )
}

/// Takes billing up again.
async fn unpause(State(state): State<AppState>, user: AuthUser, htmx: Htmx) -> Response {
    let result = Billing::of(&state, &user).unpause().await;
    let to = after(&state);
    outcome(&htmx, result, "Your subscription goes on.", &to, &to)
}
