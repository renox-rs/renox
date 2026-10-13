//! Service plans (#237): the plans page, subscribing through Stripe (on
//! `FakeHttp`, with signed test webhooks), Xendit and the demo gateway, one
//! plan per bike, visits made ahead over months (`TestApp::travel`) on the
//! weekday and within the workshop's capacity, paused and cancelled plans
//! making nothing, missed visits not rolling over, skipping and moving,
//! payments that fail and come in, swap / cancel / resume, the parts
//! discount for subscribers only, and a deleted account cancelling its
//! subscription.

use bikeshop::app::accounts::model::Customer;
use bikeshop::app::plans::demo::{DemoEvent, SIGNATURE_HEADER};
use bikeshop::app::plans::factories::{SubscriptionStates, plan_subscriptions};
use bikeshop::app::plans::model::{
    Frequency, PLAN_TASKS, PlanInvoice, PlanSubscription, PlanVisit, ServicePlan,
    SubscriptionStatus, VisitStatus,
};
use bikeshop::app::plans::{self, tasks, visits};
use bikeshop::app::staff::model::Store;
use bikeshop::app::workshop::capacity::{self, NewBooking};
use bikeshop::app::workshop::model::{
    CustomerBike, ServiceTask, WorkOrder, WorkSource, WorkStatus,
};
use bikeshop::seed::{fixtures, unique};
use renox::chrono::{Datelike, Duration as Span, NaiveDate};
use renox::db::capture_queries;
use renox::http::FakeResponse;
use renox::prelude::*;
use renox::serde_json::Value;
use renox::testing::{TestApp, TestResponse};
use std::time::Duration;

const STRIPE: &str = "https://api.stripe.com/v1";
const XENDIT: &str = "https://api.xendit.co";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const PASSWORD: &str = "a long password";

struct World {
    app: TestApp,
    north: Store,
    check: ServiceTask,
    monthly: ServicePlan,
    weekly: ServicePlan,
    rider: User,
    customer: Customer,
    bike: CustomerBike,
}

/// Stripe and Xendit with test keys (tests don't read `.env`), or neither
/// (the demo gateway).
async fn world_with(gateways: bool) -> World {
    let app = TestApp::with_config(bikeshop::app(), |c| {
        if gateways {
            for (k, v) in [
                ("STRIPE_SECRET", "sk_test"),
                ("STRIPE_WEBHOOK_SECRET", "whsec_test"),
                ("STRIPE_PRICE_MONTHLY_TUNE_UP", "price_monthly"),
                ("STRIPE_PRICE_COMMUTER_CHECK", "price_weekly"),
                ("XENDIT_SECRET_KEY", "xnd_development_1"),
                ("XENDIT_CALLBACK_TOKEN", "callback-token"),
            ] {
                c.vars.insert(k.into(), v.into());
            }
        }
    })
    .await;
    app.fake_notifications();
    let db = app.db();
    let mut north = fixtures::store(db, "North").await.unwrap();
    north.workshop_minutes_per_day = 60;
    north.save(db).await.unwrap();
    let check = ServiceTask::create(
        db,
        ServiceTask {
            name: "Check".into(),
            slug: format!("check-{}", unique()),
            minutes: 30,
            price: 3_000,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let plan = |name: &str, slug: &str, frequency: Frequency, price: i64, bp: i64| ServicePlan {
        name: name.into(),
        slug: slug.into(),
        frequency,
        price,
        description: "Care.".into(),
        active: true,
        parts_discount_bp: bp,
        ..Default::default()
    };
    let monthly = ServicePlan::create(
        db,
        plan(
            "Monthly tune-up",
            "monthly-tune-up",
            Frequency::Monthly,
            6_000,
            1_000,
        ),
    )
    .await
    .unwrap();
    let weekly = ServicePlan::create(
        db,
        plan(
            "Commuter check",
            "commuter-check",
            Frequency::Weekly,
            800,
            500,
        ),
    )
    .await
    .unwrap();
    for p in [&monthly, &weekly] {
        PLAN_TASKS.attach(db, p.id, vec![check.id]).await.unwrap();
    }
    let rider = User::register(db, "Rider", "rider@example.com", PASSWORD)
        .await
        .unwrap();
    let customer = Customer::create(
        db,
        Customer {
            user_id: Some(rider.id),
            name: "Rider".into(),
            email: Some("rider@example.com".into()),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let bike = new_bike(db, customer.id, "Blue roadie").await;
    World {
        app,
        north,
        check,
        monthly,
        weekly,
        rider,
        customer,
        bike,
    }
}

async fn world() -> World {
    world_with(true).await
}

async fn new_bike(db: &Db, customer_id: i64, name: &str) -> CustomerBike {
    CustomerBike::create(
        db,
        CustomerBike {
            customer_id,
            name: name.into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

/// The shop's date, after `travel`.
async fn today(app: &TestApp) -> NaiveDate {
    let state = app.state().clone();
    app.at_travelled_time(async move { visits::today(&state.config) })
        .await
}

/// Now in unix seconds, after `travel`.
async fn now(app: &TestApp) -> i64 {
    app.at_travelled_time(async { renox::db::now().timestamp() })
        .await
}

fn weekday(day: NaiveDate) -> i64 {
    day.weekday().number_from_monday() as i64
}

/// A Stripe subscription object for the world's bike.
fn stripe_sub(w: &World, status: &str, price: &str, plan: &str, period_end: i64) -> Value {
    json!({
        "id": "sub_1",
        "object": "subscription",
        "customer": "cus_1",
        "status": status,
        "cancel_at_period_end": false,
        "cancel_at": null,
        "ended_at": null,
        "trial_end": null,
        "metadata": {
            "renox_billable": format!("user:{}", w.rider.id),
            "renox_name": format!("bike-{}", w.bike.id),
            "renox_plan": plan,
        },
        "items": { "data": [{ "id": "si_1", "price": { "id": price }, "current_period_end": period_end }] },
    })
}

/// A signed Stripe webhook.
async fn stripe_webhook(app: &TestApp, id: &str, kind: &str, object: Value) -> TestResponse {
    let t = now(app).await;
    let payload = json!({
        "id": id,
        "object": "event",
        "type": kind,
        "created": t,
        "data": { "object": object },
    })
    .to_string();
    let signature = renox::webhook::hmac_sha256_hex("whsec_test", format!("{t}.{payload}"));
    app.request()
        .header("stripe-signature", &format!("t={t},v1={signature}"))
        .post_body("/billing/webhooks/stripe", "application/json", payload)
        .await
}

/// A Stripe invoice webhook (paid or failed) for `sub_1`.
async fn stripe_invoice(app: &TestApp, id: &str, paid: bool, amount: i64) -> TestResponse {
    let kind = if paid {
        "invoice.payment_succeeded"
    } else {
        "invoice.payment_failed"
    };
    let invoice = json!({
        "id": id,
        "object": "invoice",
        "customer": "cus_1",
        "subscription": "sub_1",
        "amount_paid": if paid { amount } else { 0 },
        "amount_due": amount,
        "currency": "usd",
    });
    stripe_webhook(app, &format!("evt_{id}_{paid}"), kind, invoice).await
}

/// The rider subscribes the bike to the monthly plan through Stripe, on
/// `weekday`, and the gateway confirms it: the plan, now active.
async fn subscribed(w: &World, weekday: i64) -> PlanSubscription {
    let http = w.app.fake_http();
    http.on(
        &format!("POST {STRIPE}/customers"),
        FakeResponse::json(200, json!({ "id": "cus_1" })),
    );
    http.on(
        &format!("POST {STRIPE}/checkout/sessions"),
        FakeResponse::json(
            200,
            json!({ "id": "cs_1", "url": "https://checkout.stripe.com/c/pay/cs_1" }),
        ),
    );
    w.app.acting_as(&w.rider);
    let bike = w.bike.id.to_string();
    let store = w.north.id.to_string();
    let day = weekday.to_string();
    w.app
        .post(
            "/plans/subscribe",
            &[
                ("bike", &bike),
                ("plan", "monthly-tune-up"),
                ("store", &store),
                ("weekday", &day),
                ("pay_with", "card"),
            ],
        )
        .await
        .assert_redirect("https://checkout.stripe.com/c/pay/cs_1");
    let period_end = now(&w.app).await + 30 * DAY.as_secs() as i64;
    stripe_webhook(
        &w.app,
        "evt_created",
        "customer.subscription.created",
        stripe_sub(w, "active", "price_monthly", "monthly-tune-up", period_end),
    )
    .await
    .assert_ok();
    w.app.run_jobs().await;
    latest(w).await
}

async fn latest(w: &World) -> PlanSubscription {
    PlanSubscription::query()
        .where_(PlanSubscription::CUSTOMER_BIKE_ID.eq(w.bike.id))
        .order_by_desc(PlanSubscription::ID)
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap()
}

async fn visits_of(w: &World, sub: &PlanSubscription) -> Vec<PlanVisit> {
    PlanVisit::query()
        .where_(PlanVisit::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .order_by(PlanVisit::SEQ)
        .get(w.app.db())
        .await
        .unwrap()
}

async fn order_of(w: &World, visit: &PlanVisit) -> WorkOrder {
    WorkOrder::find(w.app.db(), visit.work_order_id.unwrap())
        .await
        .unwrap()
        .unwrap()
}

/// A plan paid at the counter (no gateway), running from `starts_on`.
async fn counter_plan(w: &World, plan: &ServicePlan, starts_on: NaiveDate) -> PlanSubscription {
    let mut sub = plan_subscriptions()
        .of(w.bike.id, plan.id, w.north.id)
        .create_one(w.app.db())
        .await
        .unwrap();
    sub.starts_on = starts_on;
    sub.preferred_weekday = weekday(starts_on);
    sub.next_visit_on = Some(starts_on);
    sub.save(w.app.db()).await.unwrap();
    sub
}

async fn run_task(w: &World) -> tasks::Run {
    let state = w.app.state().clone();
    w.app
        .at_travelled_time(async move { tasks::run(&state).await })
        .await
        .unwrap()
}

fn local_day(w: &World, at: DateTime) -> NaiveDate {
    bikeshop::app::rentals::booking::to_local(&w.app.state().config, at).date()
}

#[renox::test]
async fn the_plans_page_compares_the_plans_by_the_month() {
    let w = world().await;
    let money = |n: i64| bikeshop::app::rentals::reserve::money(w.app.state(), n);
    let page = w.app.get("/plans").await;
    page.assert_ok()
        .assert_see("Monthly tune-up")
        .assert_see("Commuter check")
        // $60.00 a visit, monthly; $8.00 a visit weekly is $35.00 a month.
        .assert_see("$60.00")
        .assert_see("$35.00")
        .assert_see(&money(6_000))
        .assert_see(&money(3_500))
        .assert_see("10 % off spare parts")
        .assert_see("/plans/subscribe?plan=monthly-tune-up");
    assert_eq!(w.weekly.monthly_price(), 3_500);
    // The comparison costs the same whatever the number of plans.
    let (_, few) = capture_queries(w.app.get("/plans")).await;
    for n in 0..3 {
        ServicePlan::factory()
            .state(move |p| p.name = format!("Extra {n}"))
            .create_one(w.app.db())
            .await
            .unwrap();
    }
    let (_, many) = capture_queries(w.app.get("/plans")).await;
    assert_eq!(few.len(), many.len(), "no query per plan");
}

#[renox::test]
async fn subscribing_checks_out_at_stripe_and_the_webhook_starts_the_plan() {
    let w = world().await;
    let first = today(&w.app).await + Span::days(3);
    let sub = subscribed(&w, weekday(first)).await;
    // The checkout carried the bike's subscription name back to us.
    let sent = w.app.fake_http().sent();
    let session = sent
        .iter()
        .find(|r| r.url.ends_with("/checkout/sessions"))
        .unwrap();
    assert!(
        session.body.contains(&format!("bike-{}", w.bike.id)),
        "{}",
        session.body
    );
    assert!(session.body.contains("price_monthly"));

    assert_eq!(sub.status, SubscriptionStatus::Active);
    assert_eq!(sub.user_id, Some(w.rider.id));
    assert_eq!(sub.gateway, "stripe");
    assert_eq!(sub.starts_on, first);
    // The first visit, made at once: on the weekday, at the home store, a
    // work order of source `plan` with the plan's tasks free of charge.
    let made = visits_of(&w, &sub).await;
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].due_on, first);
    let order = order_of(&w, &made[0]).await;
    assert_eq!(order.source, WorkSource::Plan);
    assert_eq!(order.plan_subscription_id, Some(sub.id));
    assert_eq!(order.store_id, w.north.id);
    assert_eq!(local_day(&w, order.scheduled_for), first);
    assert_eq!(order.total, 0);
    assert_eq!(order.minutes, w.check.minutes);
    w.app.assert_notified(&w.rider, "plans-visit-booked");

    // The first payment: an invoice, and the "plan started" mail.
    stripe_invoice(&w.app, "in_1", true, 6_000)
        .await
        .assert_ok();
    w.app.run_jobs().await;
    let invoices = PlanInvoice::query()
        .where_(PlanInvoice::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .get(w.app.db())
        .await
        .unwrap();
    assert_eq!(invoices.len(), 1);
    assert!(invoices[0].paid);
    assert_eq!(invoices[0].amount, 6_000);
    w.app.assert_notified(&w.rider, "plans-started");
    // The next month's payment is a renewal.
    stripe_invoice(&w.app, "in_2", true, 6_000)
        .await
        .assert_ok();
    w.app.run_jobs().await;
    w.app.assert_notified(&w.rider, "plans-renewed");

    // The customer's pages.
    w.app
        .get("/plans/mine")
        .await
        .assert_ok()
        .assert_see("Blue roadie")
        .assert_see("Running");
    w.app
        .get(&format!("/plans/mine/{}", sub.id))
        .await
        .assert_ok()
        .assert_see("Upcoming visits")
        .assert_see("Card (Stripe)");
}

#[renox::test]
async fn a_bike_has_one_plan_at_a_time() {
    let w = world().await;
    let day = weekday(today(&w.app).await + Span::days(2));
    subscribed(&w, day).await;
    let bike = w.bike.id.to_string();
    let store = w.north.id.to_string();
    let day = day.to_string();
    let answer = w
        .app
        .request()
        .json()
        .post(
            "/plans/subscribe",
            &[
                ("bike", &bike),
                ("plan", "commuter-check"),
                ("store", &store),
                ("weekday", &day),
                ("pay_with", "card"),
            ],
        )
        .await;
    answer.assert_status(422);
    assert!(
        answer.text().contains("one plan at a time"),
        "{}",
        answer.text()
    );
    // Someone else's bike isn't theirs to subscribe.
    let other = Customer::create(
        w.app.db(),
        Customer {
            name: "Other".into(),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let theirs = new_bike(w.app.db(), other.id, "Not mine").await;
    let theirs = theirs.id.to_string();
    w.app
        .request()
        .json()
        .post(
            "/plans/subscribe",
            &[
                ("bike", &theirs),
                ("plan", "commuter-check"),
                ("store", &store),
                ("weekday", &day),
                ("pay_with", "card"),
            ],
        )
        .await
        .assert_status(422);
}

#[renox::test]
async fn visits_come_a_week_ahead_on_the_weekday_for_months_and_missed_ones_dont_roll_over() {
    let w = world().await;
    let start = today(&w.app).await + Span::days(2);
    let sub = counter_plan(&w, &w.weekly, start).await;
    // Ten weeks of mornings, nobody bringing the bike in.
    for _ in 0..70 {
        run_task(&w).await;
        w.app.travel(DAY);
    }
    run_task(&w).await;
    let end = today(&w.app).await;
    let made = visits_of(&w, &sub).await;
    let expected = (0..)
        .map(|n| start + Span::days(7 * n))
        .take_while(|d| *d <= end + Span::days(visits::AHEAD_DAYS))
        .count();
    assert_eq!(made.len(), expected, "one visit a week, a week ahead");
    for (n, visit) in made.iter().enumerate() {
        assert_eq!(visit.seq, n as i64, "made once each");
        assert_eq!(visit.due_on, start + Span::days(7 * n as i64));
        assert_eq!(weekday(visit.due_on), weekday(start), "on the weekday");
        let order = order_of(&w, visit).await;
        if visit.due_on < end {
            assert_eq!(visit.status, VisitStatus::Missed, "{visit:?}");
            assert_eq!(order.status, WorkStatus::Cancelled, "the slot freed");
        } else {
            assert_eq!(visit.status, VisitStatus::Scheduled);
        }
    }
    // Missed visits are recorded, not booked again.
    let orders = WorkOrder::query()
        .where_(WorkOrder::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .count(w.app.db())
        .await
        .unwrap();
    assert_eq!(orders as usize, made.len());
    w.app.assert_notified(&w.rider, "plans-visit-missed");
}

#[renox::test]
async fn a_monthly_plan_stays_on_its_weekday_and_week_of_the_month() {
    let w = world().await;
    let start = today(&w.app).await + Span::days(1);
    let sub = counter_plan(&w, &w.monthly, start).await;
    // Four months of mornings.
    for _ in 0..120 {
        run_task(&w).await;
        w.app.travel(DAY);
    }
    let made = visits_of(&w, &sub).await;
    assert!(made.len() >= 4, "{made:?}");
    assert_eq!(made[0].due_on, start);
    for (n, visit) in made.iter().enumerate() {
        assert_eq!(visit.seq, n as i64, "none skipped, none twice");
    }
    for pair in made.windows(2) {
        let gap = (pair[1].due_on - pair[0].due_on).num_days();
        assert!((25..=37).contains(&gap), "about a month apart: {gap}");
        assert_eq!(weekday(pair[1].due_on), weekday(start));
    }
}

#[renox::test]
async fn a_full_day_moves_the_visit_to_the_next_day_with_room() {
    let w = world().await;
    let due = today(&w.app).await + Span::days(3);
    // Another bike fills the workshop's 60 minutes that day.
    let other = new_bike(w.app.db(), w.customer.id, "Old commuter").await;
    for _ in 0..2 {
        capacity::book(
            w.app.db(),
            &w.app.state().config,
            NewBooking {
                bike_id: other.id,
                store_id: w.north.id,
                day: due,
                tasks: vec![w.check.clone()],
                package: None,
                note: None,
                source: WorkSource::Booking,
                checked_in: false,
            },
        )
        .await
        .unwrap()
        .unwrap();
    }
    let sub = counter_plan(&w, &w.weekly, due).await;
    run_task(&w).await;
    let made = visits_of(&w, &sub).await;
    assert_eq!(made[0].due_on, due, "due on the weekday");
    let order = order_of(&w, &made[0]).await;
    assert_eq!(
        local_day(&w, order.scheduled_for),
        due + Span::days(1),
        "booked the next day with room"
    );
}

#[renox::test]
async fn paused_and_cancelled_plans_make_no_visits() {
    let w = world().await;
    let start = today(&w.app).await + Span::days(2);
    let paused = plan_subscriptions()
        .of(w.bike.id, w.weekly.id, w.north.id)
        .paused()
        .create_one(w.app.db())
        .await
        .unwrap();
    let second = new_bike(w.app.db(), w.customer.id, "Gravel").await;
    let cancelled = plan_subscriptions()
        .of(second.id, w.weekly.id, w.north.id)
        .cancelled()
        .create_one(w.app.db())
        .await
        .unwrap();
    for _ in 0..14 {
        run_task(&w).await;
        w.app.travel(DAY);
    }
    assert!(visits_of(&w, &paused).await.is_empty());
    assert!(visits_of(&w, &cancelled).await.is_empty());
    // Resumed: visits again, the weeks passed while paused not made up.
    w.app.acting_as(&w.rider);
    w.app
        .post(&format!("/plans/mine/{}/unpause", paused.id), &[])
        .await
        .assert_status(303);
    let made = visits_of(&w, &paused).await;
    let today = today(&w.app).await;
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(made[0].due_on > today);
    assert!(made[0].due_on <= today + Span::days(visits::AHEAD_DAYS));
    let _ = start;
    // Pausing again takes the visit off the workshop's day.
    w.app
        .post(&format!("/plans/mine/{}/pause", paused.id), &[])
        .await
        .assert_status(303);
    let made = visits_of(&w, &paused).await;
    assert_eq!(made[0].status, VisitStatus::Cancelled);
    assert_eq!(order_of(&w, &made[0]).await.status, WorkStatus::Cancelled);
}

#[renox::test]
async fn customers_skip_and_move_visits_on_days_with_room() {
    let w = world().await;
    let start = today(&w.app).await + Span::days(2);
    let sub = counter_plan(&w, &w.weekly, start).await;
    run_task(&w).await;
    let visit = visits_of(&w, &sub).await.remove(0);
    w.app.acting_as(&w.rider);

    // Move: a full day is refused, a free one is taken.
    let full = start + Span::days(1);
    let other = new_bike(w.app.db(), w.customer.id, "Spare").await;
    for _ in 0..2 {
        capacity::book(
            w.app.db(),
            &w.app.state().config,
            NewBooking {
                bike_id: other.id,
                store_id: w.north.id,
                day: full,
                tasks: vec![w.check.clone()],
                package: None,
                note: None,
                source: WorkSource::Booking,
                checked_in: false,
            },
        )
        .await
        .unwrap()
        .unwrap();
    }
    let url = format!("/plans/visits/{}/move", visit.id);
    w.app
        .request()
        .json()
        .post(&url, &[("day", &full.to_string())])
        .await
        .assert_status(422);
    let free = start + Span::days(2);
    w.app
        .post(&url, &[("day", &free.to_string())])
        .await
        .assert_status(303);
    assert_eq!(
        local_day(&w, order_of(&w, &visit).await.scheduled_for),
        free
    );

    // Skip: the work order is cancelled, the next visit comes as planned.
    w.app
        .post(&format!("/plans/visits/{}/skip", visit.id), &[])
        .await
        .assert_status(303);
    let visit = PlanVisit::find(w.app.db(), visit.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(visit.status, VisitStatus::Skipped);
    assert_eq!(order_of(&w, &visit).await.status, WorkStatus::Cancelled);
    for _ in 0..7 {
        w.app.travel(DAY);
    }
    run_task(&w).await;
    let made = visits_of(&w, &sub).await;
    assert_eq!(made.len(), 2);
    assert_eq!(made[1].due_on, start + Span::days(7));
    assert_eq!(made[1].status, VisitStatus::Scheduled);

    // A skipped visit can't be skipped again; someone else's is a 404.
    w.app
        .post(&format!("/plans/visits/{}/skip", visit.id), &[])
        .await
        .assert_status(303);
    assert_eq!(
        PlanVisit::find(w.app.db(), visit.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        VisitStatus::Skipped
    );
    let stranger = User::register(w.app.db(), "Stranger", "s@example.com", PASSWORD)
        .await
        .unwrap();
    w.app.acting_as(&stranger);
    w.app
        .post(&format!("/plans/visits/{}/skip", made[1].id), &[])
        .await
        .assert_status(404);
    w.app
        .get(&format!("/plans/mine/{}", sub.id))
        .await
        .assert_status(404);
}

#[renox::test]
async fn a_failed_payment_holds_the_visits_until_it_is_paid() {
    let w = world().await;
    let first = today(&w.app).await + Span::days(3);
    let sub = subscribed(&w, weekday(first)).await;
    let visit = visits_of(&w, &sub).await.remove(0);
    let period_end = now(&w.app).await + 30 * DAY.as_secs() as i64;

    // The renewal fails: Stripe marks the subscription past due.
    stripe_invoice(&w.app, "in_9", false, 6_000)
        .await
        .assert_ok();
    stripe_webhook(
        &w.app,
        "evt_past_due",
        "customer.subscription.updated",
        stripe_sub(
            &w,
            "past_due",
            "price_monthly",
            "monthly-tune-up",
            period_end,
        ),
    )
    .await
    .assert_ok();
    w.app.run_jobs().await;
    let held = latest(&w).await;
    assert!(held.held_at.is_some());
    w.app.assert_notified(&w.rider, "plans-payment-failed");
    let visit_now = PlanVisit::find(w.app.db(), visit.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(visit_now.status, VisitStatus::Held);
    assert_eq!(order_of(&w, &visit_now).await.status, WorkStatus::Cancelled);
    // No discount while on hold, and the visits can't be changed.
    assert_eq!(
        plans::parts_discount_bp(w.app.db(), Some(w.customer.id))
            .await
            .unwrap(),
        0
    );
    w.app.acting_as(&w.rider);
    w.app
        .post(&format!("/plans/visits/{}/skip", visit.id), &[])
        .await
        .assert_redirect(&format!("/plans/mine/{}", sub.id));
    // Nothing is made while on hold.
    run_task(&w).await;
    assert_eq!(visits_of(&w, &sub).await.len(), 1);
    w.app
        .get(&format!("/plans/mine/{}", sub.id))
        .await
        .assert_see("On hold");

    // Paid on the retry: booked again.
    stripe_invoice(&w.app, "in_9", true, 6_000)
        .await
        .assert_ok();
    stripe_webhook(
        &w.app,
        "evt_active_again",
        "customer.subscription.updated",
        stripe_sub(&w, "active", "price_monthly", "monthly-tune-up", period_end),
    )
    .await
    .assert_ok();
    w.app.run_jobs().await;
    let released = latest(&w).await;
    assert!(released.held_at.is_none());
    let visit_now = PlanVisit::find(w.app.db(), visit.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(visit_now.status, VisitStatus::Scheduled);
    let order = order_of(&w, &visit_now).await;
    assert_eq!(order.status, WorkStatus::Booked);
    assert_eq!(local_day(&w, order.scheduled_for), first);
    // One invoice row for the payment, now paid.
    let invoice = PlanInvoice::query()
        .where_(PlanInvoice::PAYMENT_ID.eq("in_9"))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();
    assert!(invoice.paid);
}

#[renox::test]
async fn swap_cancel_and_resume_go_through_stripe() {
    let w = world().await;
    let sub = subscribed(&w, weekday(today(&w.app).await + Span::days(2))).await;
    let period_end = now(&w.app).await + 30 * DAY.as_secs() as i64;
    let http = w.app.fake_http();
    http.on(
        &format!("GET {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(
            200,
            stripe_sub(&w, "active", "price_monthly", "monthly-tune-up", period_end),
        ),
    );
    let mut swapped = stripe_sub(&w, "active", "price_weekly", "commuter-check", period_end);
    let mut cancelled = swapped.clone();
    cancelled["cancel_at_period_end"] = json!(true);
    // Answered in turn: the swap, the cancellation, the resume.
    http.on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, swapped.clone()),
    );
    http.on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, cancelled),
    );
    swapped["cancel_at_period_end"] = json!(false);
    http.on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, swapped),
    );
    w.app.acting_as(&w.rider);
    let page = format!("/plans/mine/{}", sub.id);

    // Another plan, from the next period (no proration).
    w.app
        .post(&format!("{page}/swap"), &[("plan", "commuter-check")])
        .await
        .assert_redirect(&page);
    let swap_form = http
        .sent()
        .into_iter()
        .rfind(|r| r.method == "POST" && r.url.ends_with("/subscriptions/sub_1"))
        .unwrap();
    assert!(
        swap_form.body.contains("proration_behavior=none"),
        "{}",
        swap_form.body
    );
    let after_swap = latest(&w).await;
    assert_eq!(after_swap.service_plan_id, w.monthly.id, "not yet");
    assert_eq!(after_swap.next_plan_id, Some(w.weekly.id));
    let period_end_day = local_day(&w, DateTime::from_timestamp(period_end, 0).unwrap());
    assert_eq!(after_swap.swap_on, Some(period_end_day));

    // Cancel at the period's end: visits until then.
    w.app
        .post(&format!("{page}/cancel"), &[])
        .await
        .assert_redirect(&page);
    let ending = latest(&w).await;
    assert_eq!(ending.ends_on, Some(period_end_day));
    assert_eq!(ending.status, SubscriptionStatus::Active);
    w.app.get(&page).await.assert_see("Ending");

    // Resume before then.
    w.app
        .post(&format!("{page}/resume"), &[])
        .await
        .assert_redirect(&page);
    assert_eq!(latest(&w).await.ends_on, None);

    // At the period's end, the new plan's visits start.
    for _ in 0..31 {
        w.app.travel(DAY);
    }
    run_task(&w).await;
    assert_eq!(latest(&w).await.service_plan_id, w.weekly.id);

    // Stripe ends it: the plan is over, its visits cancelled.
    let mut ended = stripe_sub(&w, "canceled", "price_weekly", "commuter-check", period_end);
    ended["ended_at"] = json!(now(&w.app).await);
    stripe_webhook(
        &w.app,
        "evt_deleted",
        "customer.subscription.deleted",
        ended,
    )
    .await
    .assert_ok();
    w.app.run_jobs().await;
    let over = latest(&w).await;
    assert_eq!(over.status, SubscriptionStatus::Cancelled);
    for visit in visits_of(&w, &over).await {
        assert_ne!(visit.status, VisitStatus::Scheduled, "{visit:?}");
    }
}

#[renox::test]
async fn the_parts_discount_is_for_subscribers_only() {
    let w = world().await;
    let db = w.app.db();
    assert_eq!(
        plans::parts_discount_bp(db, Some(w.customer.id))
            .await
            .unwrap(),
        0
    );
    assert_eq!(plans::parts_discount_bp(db, None).await.unwrap(), 0);
    let sub = counter_plan(&w, &w.monthly, today(&w.app).await + Span::days(2)).await;
    assert_eq!(
        plans::parts_discount_bp(db, Some(w.customer.id))
            .await
            .unwrap(),
        1_000,
        "the plan's own discount"
    );
    // Another customer without a plan.
    let other = Customer::create(
        db,
        Customer {
            name: "Walk-in".into(),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        plans::parts_discount_bp(db, Some(other.id)).await.unwrap(),
        0
    );
    // On a plan's work order a part costs less; on a booking it doesn't.
    run_task(&w).await;
    let visit = visits_of(&w, &sub).await.remove(0);
    let plan_order = order_of(&w, &visit).await;
    assert_eq!(
        plans::part_price(db, &plan_order, 10_000).await.unwrap(),
        9_000
    );
    let booking = WorkOrder {
        source: WorkSource::Booking,
        ..plan_order.clone()
    };
    let booking = WorkOrder {
        plan_subscription_id: None,
        ..booking
    };
    assert_eq!(
        plans::part_price(db, &booking, 10_000).await.unwrap(),
        10_000
    );
    // A cancelled plan gives nothing.
    let mut sub = sub;
    visits::end(db, &mut sub).await.unwrap();
    assert_eq!(
        plans::parts_discount_bp(db, Some(w.customer.id))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        plans::part_price(db, &plan_order, 10_000).await.unwrap(),
        10_000
    );
}

#[renox::test]
async fn deleting_the_account_cancels_the_subscription() {
    let w = world().await;
    let sub = subscribed(&w, weekday(today(&w.app).await + Span::days(2))).await;
    w.app.fake_http().on(
        &format!("DELETE {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(
            200,
            stripe_sub(
                &w,
                "canceled",
                "price_monthly",
                "monthly-tune-up",
                now(&w.app).await,
            ),
        ),
    );
    w.app.acting_as(&w.rider);
    w.app.confirm_password();
    w.app
        .post("/account", &[("_method", "DELETE"), ("password", PASSWORD)])
        .await
        .assert_status(303);
    // renox-billing's listener cancelled it at Stripe and deleted its rows.
    w.app
        .fake_http()
        .assert_sent(|r| r.method == "DELETE" && r.url.ends_with("/subscriptions/sub_1"));
    w.app.assert_database_count("subscriptions", 0).await;
    // The shop ended the plan and freed the workshop's slot.
    let over = PlanSubscription::find(w.app.db(), sub.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(over.status, SubscriptionStatus::Cancelled);
    for visit in visits_of(&w, &over).await {
        assert_eq!(visit.status, VisitStatus::Cancelled);
        assert_eq!(order_of(&w, &visit).await.status, WorkStatus::Cancelled);
    }
}

#[renox::test]
async fn xendit_charges_the_monthly_price_in_rupiah() {
    let w = world().await;
    let http = w.app.fake_http();
    http.on(
        &format!("POST {XENDIT}/customers"),
        FakeResponse::json(200, json!({ "id": "cust-1" })),
    );
    http.on(
        &format!("POST {XENDIT}/recurring/plans"),
        FakeResponse::json(
            201,
            json!({ "id": "repl_1", "actions": [{ "url": "https://linking-dev.xendit.co/link/1" }] }),
        ),
    );
    w.app.acting_as(&w.rider);
    let bike = w.bike.id.to_string();
    let store = w.north.id.to_string();
    w.app
        .post(
            "/plans/subscribe",
            &[
                ("bike", &bike),
                ("plan", "commuter-check"),
                ("store", &store),
                ("weekday", "3"),
                ("pay_with", "xendit"),
            ],
        )
        .await
        .assert_redirect("https://linking-dev.xendit.co/link/1");
    let plan = http
        .sent()
        .into_iter()
        .find(|r| r.url.ends_with("/recurring/plans"))
        .unwrap()
        .json();
    // The seeded content's price for the commuter check: $8.00 a visit,
    // weekly, so $35.00 a month; Xendit charges rupiah at the shop's rate.
    assert_eq!(plan["currency"], "IDR");
    assert_eq!(plan["amount"], 560_000);
    assert_eq!(plan["metadata"]["renox_plan"], "commuter-check-xendit");
    assert_eq!(
        plan["metadata"]["renox_name"],
        format!("bike-{}", w.bike.id)
    );
}

#[renox::test]
async fn without_keys_the_demo_gateway_subscribes_end_to_end() {
    let w = world_with(false).await;
    w.app.acting_as(&w.rider);
    let bike = w.bike.id.to_string();
    let store = w.north.id.to_string();
    let first = today(&w.app).await + Span::days(2);
    let day = weekday(first).to_string();
    let form = w.app.get("/plans/subscribe").await;
    form.assert_ok().assert_see("Demo card (no real money)");
    let answer = w
        .app
        .post(
            "/plans/subscribe",
            &[
                ("bike", &bike),
                ("plan", "monthly-tune-up"),
                ("store", &store),
                ("weekday", &day),
                ("pay_with", "card"),
            ],
        )
        .await;
    answer.assert_status(303);
    let page = answer.header("location").unwrap().to_owned();
    let path = page
        .split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|i| rest[i..].to_owned()))
        .unwrap();
    assert!(path.starts_with("/plans/demo-pay/"), "{path}");
    w.app
        .get(&path)
        .await
        .assert_ok()
        .assert_see("No real money");
    // A tampered link is refused.
    w.app
        .get(&path.replace("signature=", "signature=0"))
        .await
        .assert_status(403);
    // Paying queues the gateway's webhook to this app; the fake catches it.
    let http = w.app.fake_http();
    http.on("POST */billing/webhooks/demo", FakeResponse::status(200));
    w.app
        .post(&path, &[("outcome", "pay")])
        .await
        .assert_redirect("/billing/return");
    w.app.run_jobs().await;
    let sent = http
        .sent()
        .into_iter()
        .find(|r| r.url.ends_with("/billing/webhooks/demo"))
        .expect("the demo webhook was sent");
    // …and it arrives here, signed.
    w.app
        .request()
        .header(SIGNATURE_HEADER, sent.header(SIGNATURE_HEADER).unwrap())
        .post_body(
            "/billing/webhooks/demo",
            "application/json",
            sent.body.clone(),
        )
        .await
        .assert_ok();
    w.app.run_jobs().await;
    let sub = latest(&w).await;
    assert_eq!(sub.status, SubscriptionStatus::Active);
    assert_eq!(sub.gateway, "demo");
    assert_eq!(visits_of(&w, &sub).await.len(), 1);
    w.app.assert_notified(&w.rider, "plans-started");
    // A forged one isn't.
    let forged = DemoEvent::new(w.rider.id, &sub.billing_name(), "monthly-tune-up", 1, true);
    w.app
        .request()
        .header(SIGNATURE_HEADER, "00")
        .post_body(
            "/billing/webhooks/demo",
            "application/json",
            renox::serde_json::to_string(&forged).unwrap(),
        )
        .await
        .assert_status(401);
    // The demo's renewal, failed: the plan goes on hold.
    w.app
        .post(
            &format!("/plans/mine/{}/demo-renew", sub.id),
            &[("outcome", "failed")],
        )
        .await
        .assert_redirect(&format!("/plans/mine/{}", sub.id));
    w.app.run_jobs().await;
    let failed = http
        .sent()
        .into_iter()
        .rfind(|r| r.url.ends_with("/billing/webhooks/demo"))
        .unwrap();
    w.app
        .request()
        .header(SIGNATURE_HEADER, failed.header(SIGNATURE_HEADER).unwrap())
        .post_body(
            "/billing/webhooks/demo",
            "application/json",
            failed.body.clone(),
        )
        .await
        .assert_ok();
    w.app.run_jobs().await;
    assert!(latest(&w).await.held_at.is_some());
    w.app.assert_notified(&w.rider, "plans-payment-failed");
}

#[renox::test]
async fn every_plans_page_answers_in_a_fixed_number_of_queries() {
    let w = world().await;
    let sub = counter_plan(&w, &w.monthly, today(&w.app).await + Span::days(2)).await;
    run_task(&w).await;
    w.app
        .get("/plans/mails")
        .await
        .assert_ok()
        .assert_see("Payment failed");
    w.app.acting_as(&w.rider);
    for page in [
        "/plans".to_owned(),
        "/plans/subscribe".to_owned(),
        "/plans/mine".to_owned(),
        format!("/plans/mine/{}", sub.id),
        "/billing".to_owned(),
        "/account".to_owned(),
    ] {
        w.app.get(&page).await.assert_ok();
    }
    w.app
        .get("/plans/mine")
        .await
        .assert_see("Blue roadie")
        .assert_see("Paid at the store");
    // My plans: the same queries for one plan or four.
    let (_, few) = capture_queries(w.app.get("/plans/mine")).await;
    for n in 0..3 {
        let bike = new_bike(w.app.db(), w.customer.id, &format!("Bike {n}")).await;
        plan_subscriptions()
            .of(bike.id, w.weekly.id, w.north.id)
            .create_one(w.app.db())
            .await
            .unwrap();
    }
    let (_, many) = capture_queries(w.app.get("/plans/mine")).await;
    assert_eq!(few.len(), many.len(), "no query per plan");
}
