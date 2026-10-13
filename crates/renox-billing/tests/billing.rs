//! Subscriptions against fake gateways (`TestApp::fake_http`: no network)
//! and signed webhooks: Stripe's checkout, plan changes, cancellations and
//! resumes; Xendit's recurring plans and cycles; trials without a payment
//! method; idempotent and out-of-order webhooks; the route guards; the
//! pages; and accounts deleted with their subscriptions.

use std::time::Duration;

use renox::http::{FakeHttp, FakeResponse};
use renox::prelude::*;
use renox::serde_json::Value;
use renox::testing::{TestApp, TestResponse};
use renox_billing::{
    Billing, Interval, PaymentFailed, PaymentSucceeded, Plan, Stripe, Subscription,
    SubscriptionCanceled, SubscriptionCreated, SubscriptionPaused, SubscriptionRoutes,
    SubscriptionStatus, SubscriptionUnpaused, SubscriptionUpdated,
};

const PASSWORD: &str = "a long password 12";
const STRIPE: &str = "https://api.stripe.com/v1";
const XENDIT: &str = "https://api.xendit.co";
const DAY: u64 = 24 * 60 * 60;

/// Pages only subscribers see.
struct Area;

impl Module for Area {
    fn name(&self) -> &'static str {
        "area"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/reports", || async { "reports" })
            .require_subscription()
            .merge(
                Routes::new()
                    .get("/exports", || async { "exports" })
                    .require_plan(&["pro"]),
            )
            .merge(
                Routes::new()
                    .get("/garage", || async { "garage" })
                    .require_subscription_named("garage"),
            )
            .merge(
                Routes::new()
                    .get("/garage/pro", || async { "garage pro" })
                    .require_plan_named("garage", &["pro"]),
            )
            .require_auth()
    }
}

fn billing() -> Billing {
    Billing::new()
        .plan(
            Plan::new("basic", "Basic")
                .price(900, "USD", Interval::Month)
                .price_id("stripe", "price_basic"),
        )
        // Its Stripe price comes from STRIPE_PRICE_PRO.
        .plan(
            Plan::new("pro", "Pro")
                .price(1_900, "USD", Interval::Month)
                .trial_days(14)
                .feature("Unlimited projects"),
        )
        .plan(
            Plan::new("local", "Local")
                .price(99_000, "IDR", Interval::Month)
                .trial_days(7)
                .via("xendit"),
        )
        .plan(
            Plan::new("local-year", "Local yearly")
                .price(990_000, "IDR", Interval::Year)
                .via("xendit"),
        )
        .gateway(Stripe::new("sk_test_123", "whsec_test"))
        .xendit()
}

async fn app_with(billing: Billing) -> TestApp {
    let app = App::new()
        .module(Auth::new().account())
        .module(billing)
        .module(Area);
    TestApp::with_config(app, |config| {
        config
            .vars
            .insert("STRIPE_PRICE_PRO".into(), "price_pro".into());
        config
            .vars
            .insert("XENDIT_SECRET_KEY".into(), "xnd_development_1".into());
        config
            .vars
            .insert("XENDIT_CALLBACK_TOKEN".into(), "callback-token".into());
    })
    .await
}

async fn app() -> TestApp {
    app_with(billing()).await
}

async fn user(app: &TestApp, name: &str) -> User {
    let email = format!("{}@example.com", name.to_lowercase());
    let user = User::register(app.db(), name, &email, PASSWORD)
        .await
        .unwrap();
    app.acting_as(&user);
    user
}

/// Now, as the app sees it (after `travel`).
async fn now(app: &TestApp) -> i64 {
    app.at_travelled_time(async { renox::db::now().timestamp() })
        .await
}

/// Stripe answers a new customer and a checkout session.
fn stripe_checkout(app: &TestApp) -> FakeHttp {
    let http = app.fake_http();
    http.on(
        &format!("POST {STRIPE}/customers"),
        FakeResponse::json(200, json!({ "id": "cus_1", "object": "customer" })),
    );
    http.on(
        &format!("POST {STRIPE}/checkout/sessions"),
        FakeResponse::json(
            200,
            json!({ "id": "cs_1", "url": "https://checkout.stripe.com/c/pay/cs_1" }),
        ),
    );
    http
}

/// The form of a request the fake received, as pairs.
fn form(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn decode(text: &str) -> String {
    let text = text.replace('+', " ");
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn param(pairs: &[(String, String)], name: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

/// A Stripe subscription object.
fn stripe_sub(user: &User, status: &str, price: &str, plan: &str, period_end: i64) -> Value {
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
            "renox_billable": format!("user:{}", user.id),
            "renox_name": "default",
            "renox_plan": plan,
        },
        "items": { "data": [{
            "id": "si_1",
            "price": { "id": price },
            "current_period_end": period_end,
        }] },
    })
}

/// A signed Stripe webhook.
async fn stripe_webhook(
    app: &TestApp,
    id: &str,
    kind: &str,
    created: i64,
    object: Value,
) -> TestResponse {
    let payload = json!({
        "id": id,
        "object": "event",
        "type": kind,
        "created": created,
        "data": { "object": object },
    })
    .to_string();
    let t = now(app).await;
    let signature = renox::webhook::hmac_sha256_hex("whsec_test", format!("{t}.{payload}"));
    app.request()
        .header("stripe-signature", &format!("t={t},v1={signature}"))
        .post_body("/billing/webhooks/stripe", "application/json", payload)
        .await
}

/// A Xendit webhook with the callback token.
async fn xendit_webhook(app: &TestApp, event: &str, created: &str, data: Value) -> TestResponse {
    let payload = json!({
        "created": created,
        "business_id": "biz_1",
        "event": event,
        "data": data,
        "api_version": "v1",
    })
    .to_string();
    app.request()
        .header("x-callback-token", "callback-token")
        .post_body("/billing/webhooks/xendit", "application/json", payload)
        .await
}

/// Ana, subscribed to Pro through Stripe (active until `period_end`).
async fn subscribed(app: &TestApp) -> (User, i64) {
    let ana = user(app, "Ana").await;
    stripe_checkout(app);
    app.post("/billing/checkout/pro", &[])
        .await
        .assert_status(303);
    let period_end = now(app).await + 30 * DAY as i64;
    stripe_webhook(
        app,
        "evt_created",
        "customer.subscription.created",
        now(app).await,
        stripe_sub(&ana, "active", "price_pro", "pro", period_end),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    (ana, period_end)
}

async fn latest(app: &TestApp, user: &User) -> Subscription {
    Billing::of(app.state(), user)
        .subscription()
        .await
        .unwrap()
        .expect("a subscription")
}

#[renox::test]
async fn checkout_sends_the_user_to_stripe_with_the_plan_and_its_trial() {
    let app = app().await;
    let ana = user(&app, "Ana").await;
    let http = stripe_checkout(&app);
    let response = app.post("/billing/checkout/pro", &[]).await;
    response.assert_redirect("https://checkout.stripe.com/c/pay/cs_1");

    let sent = http.sent();
    assert_eq!(sent.len(), 2, "a customer, then the session");
    assert_eq!(sent[0].header("authorization"), Some("Bearer sk_test_123"));
    let customer = form(&sent[0].body);
    assert_eq!(param(&customer, "email").unwrap(), "ana@example.com");
    assert_eq!(
        param(&customer, "metadata[renox_billable]").unwrap(),
        format!("user:{}", ana.id)
    );
    let session = form(&sent[1].body);
    assert_eq!(param(&session, "mode").unwrap(), "subscription");
    assert_eq!(param(&session, "customer").unwrap(), "cus_1");
    assert_eq!(
        param(&session, "line_items[0][price]").unwrap(),
        "price_pro"
    );
    assert_eq!(
        param(&session, "subscription_data[metadata][renox_plan]").unwrap(),
        "pro"
    );
    assert!(
        param(&session, "success_url")
            .unwrap()
            .ends_with("/billing/return")
    );
    // A first subscription gets the plan's 14 days.
    let trial_end: i64 = param(&session, "subscription_data[trial_end]")
        .unwrap()
        .parse()
        .unwrap();
    let expected = now(&app).await + 14 * DAY as i64;
    assert!(
        (trial_end - expected).abs() < 60,
        "{trial_end} vs {expected}"
    );
    app.assert_database_has(
        "billing_customers",
        &[("gateway", &"stripe"), ("gateway_id", &"cus_1")],
    )
    .await;

    // The customer is made once; nothing is subscribed until the webhook.
    app.post("/billing/checkout/basic", &[])
        .await
        .assert_status(303);
    let sessions: Vec<_> = http
        .sent()
        .into_iter()
        .filter(|r| r.url.ends_with("/customers"))
        .collect();
    assert_eq!(sessions.len(), 1);
    assert!(!Billing::of(app.state(), &ana).subscribed().await.unwrap());
    // An unknown plan is refused with a message.
    let refused = app.post("/billing/checkout/gold", &[]).await;
    refused.assert_redirect("/billing");
}

#[renox::test]
async fn webhooks_make_and_update_the_subscription_once_each() {
    let app = app().await;
    app.fake_events();
    let (ana, period_end) = subscribed(&app).await;
    let subscription = latest(&app, &ana).await;
    assert_eq!(subscription.plan, "pro");
    assert_eq!(subscription.gateway, "stripe");
    assert_eq!(subscription.gateway_id.as_deref(), Some("sub_1"));
    assert_eq!(subscription.status, SubscriptionStatus::Active);
    assert_eq!(
        subscription.current_period_end.map(|t| t.timestamp()),
        Some(period_end)
    );
    let billing = Billing::of(app.state(), &ana);
    assert!(billing.subscribed().await.unwrap());
    assert!(billing.subscribed_to("pro").await.unwrap());
    assert!(!billing.subscribed_to("basic").await.unwrap());
    assert_eq!(app.emitted::<SubscriptionCreated>().len(), 1);

    // The provider sends the same event again: answered, not processed.
    let again = stripe_webhook(
        &app,
        "evt_created",
        "customer.subscription.created",
        now(&app).await,
        stripe_sub(&ana, "active", "price_pro", "pro", period_end),
    )
    .await;
    again.assert_ok();
    assert_eq!(again.text(), "already received");
    assert_eq!(app.run_jobs().await, 0);
    app.assert_database_count("webhook_calls", 1).await;
    app.assert_database_count("subscriptions", 1).await;

    // Another event saying the same changes nothing and emits nothing.
    stripe_webhook(
        &app,
        "evt_same",
        "customer.subscription.updated",
        now(&app).await,
        stripe_sub(&ana, "active", "price_pro", "pro", period_end),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    assert!(app.emitted::<SubscriptionUpdated>().is_empty());

    // A plan changed in Stripe's dashboard: the price names the plan.
    stripe_webhook(
        &app,
        "evt_swapped",
        "customer.subscription.updated",
        now(&app).await + 5,
        stripe_sub(&ana, "active", "price_basic", "pro", period_end),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    assert_eq!(latest(&app, &ana).await.plan, "basic");
    app.assert_emitted::<SubscriptionUpdated>(|e| {
        e.previous_plan == "pro" && e.subscription.plan == "basic"
    });
}

#[renox::test]
async fn an_older_event_arriving_late_changes_nothing() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    let t = now(&app).await;
    stripe_webhook(
        &app,
        "evt_past_due",
        "customer.subscription.updated",
        t + 100,
        stripe_sub(&ana, "past_due", "price_pro", "pro", period_end),
    )
    .await
    .assert_ok();
    // Sent before the one above, delivered after it.
    stripe_webhook(
        &app,
        "evt_older",
        "customer.subscription.updated",
        t + 50,
        stripe_sub(&ana, "active", "price_pro", "pro", period_end),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let subscription = latest(&app, &ana).await;
    assert_eq!(subscription.status, SubscriptionStatus::PastDue);
    assert!(!subscription.valid(), "past due isn't valid");
}

#[renox::test]
async fn webhooks_with_a_wrong_or_old_signature_are_refused() {
    let app = app().await;
    let payload = json!({ "id": "evt_1", "type": "customer.subscription.created" }).to_string();
    let t = now(&app).await;
    let forged = renox::webhook::hmac_sha256_hex("not-the-secret", format!("{t}.{payload}"));
    app.request()
        .header("stripe-signature", &format!("t={t},v1={forged}"))
        .post_body(
            "/billing/webhooks/stripe",
            "application/json",
            payload.clone(),
        )
        .await
        .assert_unauthorized();
    let old = t - 600;
    let stale = renox::webhook::hmac_sha256_hex("whsec_test", format!("{old}.{payload}"));
    app.request()
        .header("stripe-signature", &format!("t={old},v1={stale}"))
        .post_body(
            "/billing/webhooks/stripe",
            "application/json",
            payload.clone(),
        )
        .await
        .assert_unauthorized();
    // A sent gateway header doesn't pick another gateway.
    app.request()
        .header("x-renox-billing-gateway", "xendit")
        .header("x-callback-token", "callback-token")
        .post_body(
            "/billing/webhooks/stripe",
            "application/json",
            payload.clone(),
        )
        .await
        .assert_unauthorized();
    app.request()
        .header("x-callback-token", "wrong")
        .post_body(
            "/billing/webhooks/xendit",
            "application/json",
            payload.clone(),
        )
        .await
        .assert_unauthorized();
    app.post_body("/billing/webhooks/paypal", "application/json", payload)
        .await
        .assert_not_found();
    app.assert_database_count("webhook_calls", 0).await;
}

#[renox::test]
async fn swapping_plans_is_prorated_at_stripe() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    app.fake_events();
    let http = app.fake_http();
    http.on(
        &format!("GET {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(
            200,
            stripe_sub(&ana, "active", "price_pro", "pro", period_end),
        ),
    );
    http.on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(
            200,
            stripe_sub(&ana, "active", "price_basic", "basic", period_end),
        ),
    );
    app.post("/billing/swap/basic", &[])
        .await
        .assert_redirect("/account");
    let update = http
        .sent()
        .into_iter()
        .find(|r| r.method == "POST" && r.url.ends_with("/subscriptions/sub_1"))
        .unwrap();
    let pairs = form(&update.body);
    assert_eq!(param(&pairs, "items[0][id]").unwrap(), "si_1");
    assert_eq!(param(&pairs, "items[0][price]").unwrap(), "price_basic");
    assert_eq!(
        param(&pairs, "proration_behavior").unwrap(),
        "create_prorations"
    );
    assert_eq!(latest(&app, &ana).await.plan, "basic");
    app.assert_emitted::<SubscriptionUpdated>(|e| e.previous_plan == "pro");
    // A plan sold through another gateway can't be swapped to.
    app.post("/billing/swap/local", &[])
        .await
        .assert_redirect("/billing");
    assert_eq!(latest(&app, &ana).await.plan, "basic");
}

#[renox::test]
async fn without_proration_swaps_say_so() {
    let app = app_with(billing().without_proration()).await;
    let (ana, period_end) = subscribed(&app).await;
    let http = app.fake_http();
    http.on(
        &format!("GET {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(
            200,
            stripe_sub(&ana, "active", "price_pro", "pro", period_end),
        ),
    );
    http.on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(
            200,
            stripe_sub(&ana, "active", "price_basic", "basic", period_end),
        ),
    );
    Billing::of(app.state(), &ana).swap("basic").await.unwrap();
    http.assert_sent(|r| r.method == "POST" && r.body.contains("proration_behavior=none"));
}

#[renox::test]
async fn canceling_keeps_access_until_the_period_ends_and_can_be_resumed() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    app.fake_events();
    let http = app.fake_http();
    let mut canceling = stripe_sub(&ana, "active", "price_pro", "pro", period_end);
    canceling["cancel_at_period_end"] = json!(true);
    // Stripe answers the cancellation, then the resume.
    http.on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, canceling),
    )
    .on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(
            200,
            stripe_sub(&ana, "active", "price_pro", "pro", period_end),
        ),
    );
    app.post("/billing/cancel", &[])
        .await
        .assert_redirect("/account");
    http.assert_sent(|r| r.body.contains("cancel_at_period_end=true"));
    let subscription = latest(&app, &ana).await;
    assert!(subscription.canceled());
    assert!(subscription.on_grace_period());
    assert!(subscription.valid());
    assert_eq!(
        subscription.ends_at.map(|t| t.timestamp()),
        Some(period_end)
    );
    app.assert_emitted::<SubscriptionCanceled>(|e| e.subscription.id == subscription.id);
    app.get("/reports").await.assert_ok();

    // The account page offers to resume it.
    app.get("/account")
        .await
        .assert_ok()
        .assert_see("Canceled: you have access until")
        .assert_see("Resume");
    // The plans page offers the canceled plan again.
    app.get("/billing")
        .await
        .assert_see("/billing/checkout/pro")
        .assert_dont_see("Your plan");
    app.post("/billing/resume", &[])
        .await
        .assert_redirect("/account");
    http.assert_sent(|r| r.body.contains("cancel_at_period_end=false"));
    let subscription = latest(&app, &ana).await;
    assert!(!subscription.canceled());
    assert!(subscription.valid());
}

#[renox::test]
async fn a_subscription_ends_when_its_period_runs_out() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    let mut canceling = stripe_sub(&ana, "active", "price_pro", "pro", period_end);
    canceling["cancel_at_period_end"] = json!(true);
    app.fake_http().on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, canceling),
    );
    Billing::of(app.state(), &ana).cancel().await.unwrap();

    app.travel(Duration::from_secs(31 * DAY));
    app.acting_as(&ana); // the session ran out too
    let mut ended = stripe_sub(&ana, "canceled", "price_pro", "pro", period_end);
    ended["ended_at"] = json!(period_end);
    stripe_webhook(
        &app,
        "evt_deleted",
        "customer.subscription.deleted",
        now(&app).await,
        ended,
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let subscription = app.at_travelled_time(latest(&app, &ana)).await;
    assert_eq!(subscription.status, SubscriptionStatus::Canceled);
    assert!(app.at_travelled_time(async { subscription.ended() }).await);
    // The guard sends them to the plans.
    app.get("/reports").await.assert_redirect("/billing");
    app.get("/account")
        .await
        .assert_see("Your Pro subscription has ended.");
}

#[renox::test]
async fn cancel_now_ends_access_at_once() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    let mut canceled = stripe_sub(&ana, "canceled", "price_pro", "pro", period_end);
    canceled["ended_at"] = json!(now(&app).await);
    let http = app.fake_http();
    http.on(
        &format!("DELETE {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, canceled),
    );
    let subscription = Billing::of(app.state(), &ana).cancel_now().await.unwrap();
    assert_eq!(subscription.status, SubscriptionStatus::Canceled);
    assert!(!subscription.valid());
    assert!(!Billing::of(app.state(), &ana).subscribed().await.unwrap());
}

#[renox::test]
async fn the_guards_let_subscribers_through() {
    let app = app().await;
    let ben = user(&app, "Ben").await;
    app.get("/reports").await.assert_redirect("/billing");
    app.request()
        .header("accept", "application/json")
        .get("/reports")
        .await
        .assert_status(402);
    app.logout();
    app.get("/reports").await.assert_redirect("/login");

    app.acting_as(&ben);
    let (_ana, _) = subscribed(&app).await; // acting as Ana now
    app.get("/reports").await.assert_ok().assert_see("reports");
    app.get("/exports").await.assert_ok();
    app.acting_as(&ben);
    app.get("/exports").await.assert_redirect("/billing");
}

#[renox::test]
async fn payments_are_announced_and_a_failure_makes_it_past_due() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    app.fake_events();
    let invoice = |id: &str, amount: i64| {
        json!({
            "id": id,
            "object": "invoice",
            "customer": "cus_1",
            "amount_paid": amount,
            "amount_due": amount,
            "currency": "usd",
            "parent": { "subscription_details": { "subscription": "sub_1" } },
        })
    };
    stripe_webhook(
        &app,
        "evt_paid",
        "invoice.payment_succeeded",
        now(&app).await,
        invoice("in_1", 1_900),
    )
    .await
    .assert_ok();
    // The trial's $0 invoice says nothing.
    stripe_webhook(
        &app,
        "evt_zero",
        "invoice.payment_succeeded",
        now(&app).await,
        invoice("in_0", 0),
    )
    .await
    .assert_ok();
    stripe_webhook(
        &app,
        "evt_failed",
        "invoice.payment_failed",
        now(&app).await,
        invoice("in_2", 1_900),
    )
    .await
    .assert_ok();
    stripe_webhook(
        &app,
        "evt_past_due",
        "customer.subscription.updated",
        now(&app).await + 1,
        stripe_sub(&ana, "past_due", "price_pro", "pro", period_end),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let paid = app.emitted::<PaymentSucceeded>();
    assert_eq!(paid.len(), 1);
    assert_eq!(paid[0].amount, 1_900);
    assert_eq!(paid[0].currency, "USD");
    assert_eq!(paid[0].owner.as_ref().unwrap().id, ana.id);
    assert_eq!(
        paid[0].subscription.as_ref().unwrap().gateway_id.as_deref(),
        Some("sub_1")
    );
    app.assert_emitted::<PaymentFailed>(|e| e.payment_id == "in_2");
    assert!(!Billing::of(app.state(), &ana).subscribed().await.unwrap());
    app.get("/account")
        .await
        .assert_see("Your last payment failed");
}

#[renox::test]
async fn a_trial_without_a_payment_method_carries_over_to_the_checkout() {
    let app = app_with(billing().generic_trials()).await;
    let ana = user(&app, "Ana").await;
    app.fake_events();
    app.get("/billing")
        .await
        .assert_ok()
        .assert_see("Start free trial");
    app.post("/billing/trial/pro", &[])
        .await
        .assert_redirect("/account");
    let billing = Billing::of(app.state(), &ana);
    assert!(billing.on_trial().await.unwrap());
    assert!(billing.subscribed_to("pro").await.unwrap());
    app.assert_emitted::<SubscriptionCreated>(|e| e.subscription.is_generic_trial());
    // One trial per owner.
    app.post("/billing/trial/pro", &[])
        .await
        .assert_redirect("/billing");
    app.assert_database_count("subscriptions", 1).await;
    app.get("/account")
        .await
        .assert_see("Free trial until")
        .assert_see("Subscribe before then");

    // Ten days later they subscribe: the four days left aren't charged.
    app.travel(Duration::from_secs(10 * DAY));
    app.acting_as(&ana);
    let http = stripe_checkout(&app);
    app.post("/billing/checkout/pro", &[])
        .await
        .assert_status(303);
    let session = http
        .sent()
        .into_iter()
        .find(|r| r.url.ends_with("/checkout/sessions"))
        .unwrap();
    let trial_end: i64 = param(&form(&session.body), "subscription_data[trial_end]")
        .unwrap()
        .parse()
        .unwrap();
    let trial = app.at_travelled_time(latest(&app, &ana)).await;
    assert_eq!(Some(trial_end), trial.trial_ends_at.map(|t| t.timestamp()));

    // Stripe's subscription takes over the trial's row.
    let mut trialing = stripe_sub(&ana, "trialing", "price_pro", "pro", trial_end);
    trialing["trial_end"] = json!(trial_end);
    stripe_webhook(
        &app,
        "evt_trial",
        "customer.subscription.created",
        now(&app).await,
        trialing,
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    app.assert_database_count("subscriptions", 1).await;
    // The row now has the gateway's id: announced as an update.
    app.assert_emitted::<SubscriptionUpdated>(|e| {
        e.subscription.gateway_id.is_some() && e.previous_status == SubscriptionStatus::Trialing
    });
    let subscription = app.at_travelled_time(latest(&app, &ana)).await;
    assert_eq!(subscription.gateway, "stripe");
    assert_eq!(subscription.status, SubscriptionStatus::Trialing);
    assert!(
        app.at_travelled_time(async { subscription.on_trial() })
            .await
    );
}

#[renox::test]
async fn a_trial_without_a_payment_method_runs_out() {
    let app = app_with(billing().generic_trials()).await;
    let ana = user(&app, "Ana").await;
    Billing::of(app.state(), &ana)
        .start_trial("pro")
        .await
        .unwrap();
    app.get("/reports").await.assert_ok();
    app.travel(Duration::from_secs(15 * DAY));
    app.acting_as(&ana);
    app.get("/reports").await.assert_redirect("/billing");
    assert!(
        !app.at_travelled_time(Billing::of(app.state(), &ana).subscribed())
            .await
            .unwrap()
    );
    // Not offered on a plan without a trial, nor when the module doesn't.
    let other = app_with(billing()).await;
    user(&other, "Ben").await;
    other
        .post("/billing/trial/pro", &[])
        .await
        .assert_not_found();
}

#[renox::test]
async fn xendit_plans_link_a_payment_method_then_charge_by_cycle() {
    let app = app().await;
    let ana = user(&app, "Ana").await;
    app.fake_events();
    let http = app.fake_http();
    http.on(
        &format!("POST {XENDIT}/customers"),
        FakeResponse::json(200, json!({ "id": "cust-1" })),
    );
    http.on(
        &format!("POST {XENDIT}/recurring/plans"),
        FakeResponse::json(
            201,
            json!({
                "id": "repl_1",
                "status": "REQUIRES_ACTION",
                "actions": [{ "action": "AUTH", "url": "https://linking-dev.xendit.co/link/1", "url_type": "WEB", "method": "GET" }],
            }),
        ),
    );
    app.post("/billing/checkout/local", &[])
        .await
        .assert_redirect("https://linking-dev.xendit.co/link/1");
    let sent = http.sent();
    assert!(
        sent[0]
            .header("authorization")
            .unwrap()
            .starts_with("Basic ")
    );
    let plan = sent[1].json();
    assert_eq!(plan["customer_id"], "cust-1");
    assert_eq!(plan["amount"], 99_000);
    assert_eq!(plan["currency"], "IDR");
    assert_eq!(plan["schedule"]["interval"], "MONTH");
    assert_eq!(plan["metadata"]["renox_plan"], "local");
    // A seven-day trial: the first cycle is then, nothing charged now.
    assert!(plan["schedule"]["anchor_date"].is_string());
    assert!(plan.get("immediate_action_type").is_none());
    let trial_end: i64 = plan["metadata"]["renox_trial_ends_at"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    let activated = json!({
        "id": "repl_1",
        "customer_id": "cust-1",
        "status": "ACTIVE",
        "currency": "IDR",
        "amount": 99_000,
        "metadata": plan["metadata"],
    });
    xendit_webhook(
        &app,
        "recurring.plan.activated",
        "2026-10-05T10:00:00Z",
        activated.clone(),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let subscription = latest(&app, &ana).await;
    assert_eq!(subscription.gateway, "xendit");
    assert_eq!(subscription.status, SubscriptionStatus::Trialing);
    assert_eq!(
        subscription.trial_ends_at.map(|t| t.timestamp()),
        Some(trial_end)
    );
    assert!(subscription.on_trial());

    // The first cycle is paid; the next one is scheduled.
    let cycle = |id: &str, status: &str| {
        json!({
            "id": id,
            "plan_id": "repl_1",
            "customer_id": "cust-1",
            "status": status,
            "amount": 99_000,
            "currency": "IDR",
            "scheduled_timestamp": "2026-12-12T10:00:00Z",
        })
    };
    xendit_webhook(
        &app,
        "recurring.cycle.succeeded",
        "2026-10-12T10:00:00Z",
        cycle("rpcyc_1", "SUCCEEDED"),
    )
    .await
    .assert_ok();
    xendit_webhook(
        &app,
        "recurring.cycle.created",
        "2026-10-12T10:00:01Z",
        cycle("rpcyc_2", "SCHEDULED"),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let subscription = latest(&app, &ana).await;
    assert_eq!(subscription.status, SubscriptionStatus::Active);
    assert_eq!(subscription.trial_ends_at, None);
    assert_eq!(
        subscription.current_period_end.map(|t| t.to_rfc3339()),
        Some("2026-12-12T10:00:00+00:00".into())
    );
    app.assert_emitted::<PaymentSucceeded>(|e| e.payment_id == "rpcyc_1" && e.amount == 99_000);

    // Swapping to a yearly plan isn't something Xendit does.
    app.post("/billing/swap/local-year", &[])
        .await
        .assert_redirect("/billing");
    assert_eq!(latest(&app, &ana).await.plan, "local");

    // Canceling stops the charges now and keeps access to the period's end.
    http.on(
        &format!("POST {XENDIT}/recurring/plans/repl_1/deactivate"),
        FakeResponse::json(200, json!({ "id": "repl_1", "status": "INACTIVE" })),
    );
    app.post("/billing/cancel", &[])
        .await
        .assert_redirect("/account");
    let subscription = latest(&app, &ana).await;
    assert!(subscription.on_grace_period());
    assert_eq!(subscription.ends_at, subscription.current_period_end);
    // No resuming at Xendit.
    app.get("/account").await.assert_dont_see(">Resume<");
    let mut inactive = activated;
    inactive["status"] = json!("INACTIVE");
    xendit_webhook(
        &app,
        "recurring.plan.inactivated",
        "2026-10-20T10:00:00Z",
        inactive,
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let subscription = latest(&app, &ana).await;
    assert_eq!(subscription.status, SubscriptionStatus::Canceled);
    assert!(subscription.on_grace_period(), "the paid period is kept");
}

#[renox::test]
async fn a_failed_xendit_cycle_is_past_due() {
    let app = app().await;
    let ana = user(&app, "Ana").await;
    app.fake_events();
    let metadata = json!({ "renox_billable": format!("user:{}", ana.id), "renox_name": "default", "renox_plan": "local" });
    xendit_webhook(
        &app,
        "recurring.plan.activated",
        "2026-10-05T10:00:00Z",
        json!({ "id": "repl_9", "customer_id": "cust-9", "status": "ACTIVE", "metadata": metadata }),
    )
    .await
    .assert_ok();
    xendit_webhook(
        &app,
        "recurring.cycle.retrying",
        "2026-11-05T10:00:00Z",
        json!({ "id": "rpcyc_9", "plan_id": "repl_9", "status": "RETRYING", "amount": 99_000, "currency": "IDR" }),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    assert_eq!(latest(&app, &ana).await.status, SubscriptionStatus::PastDue);
    app.assert_emitted::<PaymentFailed>(|e| e.gateway == "xendit" && e.amount == 99_000);
}

#[renox::test]
async fn the_pages_show_the_plans_and_the_subscription() {
    let app = app().await;
    user(&app, "Ana").await;
    app.get("/billing")
        .await
        .assert_ok()
        .assert_see("Choose a plan")
        .assert_see("$19.00 / month")
        .assert_see("Rp 99,000 / month")
        .assert_see("14-day free trial")
        .assert_see("Unlimited projects")
        .assert_see("Subscribe");
    app.get("/account")
        .await
        .assert_ok()
        .assert_see("You&#39;re not subscribed.")
        .assert_see("See plans");
    let app = app_with(billing()).await;
    subscribed(&app).await;
    app.get("/account")
        .await
        .assert_see("Pro · $19.00 / month")
        .assert_see("Renews on")
        .assert_see("Cancel subscription")
        .assert_see("Change plan");
    app.get("/billing")
        .await
        .assert_see("Your plan")
        .assert_see("Switch to Basic");
    // Already subscribed: a second checkout is refused.
    app.post("/billing/checkout/basic", &[])
        .await
        .assert_redirect("/billing");
    app.get("/billing/return").await.assert_redirect("/account");
}

#[renox::test]
async fn deleting_an_account_cancels_its_subscription() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    let mut canceled = stripe_sub(&ana, "canceled", "price_pro", "pro", period_end);
    canceled["ended_at"] = json!(now(&app).await);
    let http = app.fake_http();
    http.on(
        &format!("DELETE {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, canceled),
    );
    app.confirm_password();
    app.post("/account", &[("_method", "DELETE"), ("password", PASSWORD)])
        .await
        .assert_status(303);
    http.assert_sent(|r| r.method == "DELETE" && r.url.ends_with("/subscriptions/sub_1"));
    app.assert_database_count("subscriptions", 0).await;
    app.assert_database_count("billing_customers", 0).await;
}

// ---------- #259: the paths no test had reached ----------

/// Ana, on Xendit's monthly `local` plan (activated by its webhook).
async fn on_xendit(app: &TestApp) -> User {
    let ana = user(app, "Ana").await;
    let metadata = json!({ "renox_billable": format!("user:{}", ana.id), "renox_name": "default", "renox_plan": "local" });
    xendit_webhook(
        app,
        "recurring.plan.activated",
        "2026-10-05T10:00:00Z",
        json!({ "id": "repl_9", "customer_id": "cust-9", "status": "ACTIVE", "currency": "IDR", "amount": 99_000, "metadata": metadata }),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    ana
}

fn with_local_plus() -> Billing {
    billing().plan(
        Plan::new("local-plus", "Local plus")
            .price(149_000, "IDR", Interval::Month)
            .via("xendit"),
    )
}

#[renox::test]
async fn xendit_swaps_between_plans_of_the_same_interval() {
    let app = app_with(with_local_plus()).await;
    let ana = on_xendit(&app).await;
    let http = app.fake_http();
    http.on(
        &format!("PATCH {XENDIT}/recurring/plans/repl_9"),
        FakeResponse::json(200, json!({ "id": "repl_9", "status": "ACTIVE" })),
    );
    app.post("/billing/swap/local-plus", &[])
        .await
        .assert_redirect("/account");
    let body = http.sent()[0].json();
    assert_eq!(body["amount"], 149_000);
    assert_eq!(body["currency"], "IDR");
    assert_eq!(body["metadata"]["renox_plan"], "local-plus");
    assert_eq!(latest(&app, &ana).await.plan, "local-plus");
}

#[renox::test]
async fn xendit_cancels_at_once() {
    let app = app().await;
    let ana = on_xendit(&app).await;
    app.fake_http().on(
        &format!("POST {XENDIT}/recurring/plans/repl_9/deactivate"),
        FakeResponse::json(200, json!({ "id": "repl_9", "status": "INACTIVE" })),
    );
    let canceled = renox_billing::Billing::of(app.state(), &ana)
        .cancel_now()
        .await
        .unwrap();
    assert_eq!(canceled.status, SubscriptionStatus::Canceled);
    assert!(!canceled.on_grace_period());
    // Nothing left to resume.
    assert!(
        !renox_billing::Billing::of(app.state(), &ana)
            .can_resume()
            .await
            .unwrap()
    );
}

#[renox::test]
async fn a_failed_xendit_cycle_reports_the_payment() {
    let app = app().await;
    let ana = on_xendit(&app).await;
    app.fake_events();
    xendit_webhook(
        &app,
        "recurring.cycle.failed",
        "2026-11-05T10:00:00Z",
        json!({ "id": "rpcyc_8", "plan_id": "repl_9", "status": "FAILED", "amount": 99_000, "currency": "IDR" }),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    assert_eq!(latest(&app, &ana).await.status, SubscriptionStatus::PastDue);
    app.assert_emitted::<PaymentFailed>(|e| e.gateway == "xendit");
}

#[renox::test]
async fn a_gateway_that_refuses_asks_the_user_to_try_again() {
    let app = app().await;
    user(&app, "Ana").await;
    let http = app.fake_http();
    http.on(
        &format!("POST {STRIPE}/customers"),
        FakeResponse::json(
            402,
            json!({ "error": { "message": "Your card was declined." } }),
        ),
    );
    // The page asks to try again (the toast waits for the next page), and
    // shows nothing of the gateway's keys.
    app.htmx()
        .post("/billing/checkout/basic", &[])
        .await
        .assert_hx_redirect("/billing");
    app.get("/billing")
        .await
        .assert_see("The payment provider didn&#x27;t answer. Please try again.")
        .assert_dont_see("sk_test");
}

/// A gateway of the app's own that keeps the trait's defaults.
struct Manual;

impl renox_billing::Gateway for Manual {
    fn name(&self) -> &str {
        "manual"
    }
    fn label(&self) -> &str {
        "Bank transfer"
    }
    fn configured(&self, _config: &Config) -> bool {
        true
    }
    fn create_customer<'a>(
        &'a self,
        _state: &'a AppState,
        _owner: &'a renox_billing::Owner,
    ) -> renox_billing::BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("manual-1".into()) })
    }
    fn checkout<'a>(
        &'a self,
        _state: &'a AppState,
        _request: &'a renox_billing::CheckoutRequest,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::Checkout>> {
        Box::pin(async { Ok(renox_billing::Checkout::redirect("/pay-by-transfer")) })
    }
    fn swap<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
        _plan: &'a Plan,
        _prorate: bool,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::Remote>> {
        Box::pin(async { Err(Error::BadRequest("no".into())) })
    }
    fn cancel<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
        _at_period_end: bool,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::Remote>> {
        Box::pin(async { Err(Error::BadRequest("no".into())) })
    }
    fn verify_webhook(
        &self,
        _config: &Config,
        _headers: &renox::axum::http::HeaderMap,
        _body: &[u8],
    ) -> Result {
        Ok(())
    }
    fn parse_webhook(&self, _body: &[u8]) -> Result<Vec<renox_billing::Notice>> {
        Ok(Vec::new())
    }
}

#[renox::test]
async fn a_gateway_of_the_apps_own_gets_the_defaults() {
    use renox_billing::Gateway;
    let app = app().await;
    assert!(!Manual.prorates() && !Manual.resumes());
    let err = Manual
        .resume(app.state(), &Subscription::default())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::BadRequest(ref m) if m == "Bank transfer can't resume a canceled subscription: subscribe again"),
        "{err:?}"
    );
    // The event id: the `webhook-id` header, else the body's `id`, else a hash.
    let mut headers = renox::axum::http::HeaderMap::new();
    headers.insert("webhook-id", " evt_9 ".parse().unwrap());
    assert_eq!(Manual.webhook_event_id(&headers, b"{}").unwrap(), "evt_9");
    let none = renox::axum::http::HeaderMap::new();
    assert_eq!(
        Manual
            .webhook_event_id(&none, br#"{"id": "evt_1"}"#)
            .unwrap(),
        "evt_1"
    );
    assert!(
        Manual
            .webhook_event_id(&none, br#"{"id": ""}"#)
            .unwrap()
            .starts_with("sha256:")
    );
    assert!(matches!(
        Manual.webhook_event_id(&none, b"not json"),
        Err(Error::BadRequest(_))
    ));
}

/// A failed payment is written to the audit log when the app keeps one.
#[renox::test]
async fn failed_payments_go_to_the_audit_log() {
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new().account())
            .module(renox::audit::Audit)
            .module(billing())
            .module(Area),
        |config| {
            config
                .vars
                .insert("XENDIT_SECRET_KEY".into(), "xnd_development_1".into());
            config
                .vars
                .insert("XENDIT_CALLBACK_TOKEN".into(), "callback-token".into());
        },
    )
    .await;
    on_xendit(&app).await;
    xendit_webhook(
        &app,
        "recurring.cycle.retrying",
        "2026-11-05T10:00:00Z",
        json!({ "id": "rpcyc_7", "plan_id": "repl_9", "status": "RETRYING", "amount": 99_000, "currency": "IDR" }),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let entries: i64 = renox::db::sql("SELECT COUNT(*) FROM audit_logs WHERE action = ?")
        .bind("billing.payment_failed")
        .scalar(app.db())
        .await
        .unwrap();
    assert_eq!(entries, 1);
}

// ---------- #259: the rest of the gateways, Customer, module, pages, guards ----------

fn err_text(err: Error) -> String {
    format!("{err:?}")
}

/// A row made directly: what the module would have stored.
async fn row(app: &TestApp, user: &User, gateway: &str, gateway_id: Option<&str>) -> Subscription {
    Subscription::create(
        app.db(),
        Subscription {
            billable_type: "user".into(),
            billable_id: user.id,
            name: "default".into(),
            plan: "pro".into(),
            gateway: gateway.into(),
            gateway_id: gateway_id.map(str::to_owned),
            status: SubscriptionStatus::Active,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[renox::test]
async fn xendit_cancels_at_the_period_end_or_at_once_when_it_passed() {
    let app = app().await;
    let ana = on_xendit(&app).await;
    app.fake_http().on(
        &format!("POST {XENDIT}/recurring/plans/repl_9/deactivate"),
        FakeResponse::json(200, json!({ "id": "repl_9", "status": "INACTIVE" })),
    );
    // No period paid for: the end has passed, canceled now.
    let mut sub = latest(&app, &ana).await;
    let gone = Billing::of(app.state(), &ana).cancel().await.unwrap();
    assert_eq!(gone.status, SubscriptionStatus::Canceled);
    // A period paid for: access until its end.
    let end = renox::db::now() + renox::chrono::Duration::days(10);
    sub.ends_at = None;
    sub.status = SubscriptionStatus::Active;
    sub.current_period_end = Some(end);
    sub.save(app.db()).await.unwrap();
    let kept = Billing::of(app.state(), &ana).cancel().await.unwrap();
    assert!(kept.on_grace_period(), "{kept:?}");
    assert_eq!(kept.ends_at, Some(end));
    // Canceled already: nothing more to do, nothing sent.
    let sent = app.fake_http().sent().len();
    let again = Billing::of(app.state(), &ana).cancel().await.unwrap();
    assert_eq!(again.ends_at, Some(end));
    assert_eq!(app.fake_http().sent().len(), sent);
    // Xendit can't resume.
    assert!(!Billing::of(app.state(), &ana).can_resume().await.unwrap());
}

#[renox::test]
async fn xendit_checkout_charges_at_once_without_a_trial_and_reports_errors() {
    let app = app_with(billing().gateway(renox_billing::Xendit::new("xnd_in_code", "tok"))).await;
    let ana = user(&app, "Ana").await;
    let http = app.fake_http();
    http.on(
        &format!("POST {XENDIT}/customers"),
        FakeResponse::json(200, json!({ "id": "cust-1" })),
    );
    http.on(
        &format!("POST {XENDIT}/recurring/plans"),
        FakeResponse::json(
            200,
            json!({ "id": "repl_1", "actions": [{ "url": "https://linking.xendit.co/1" }] }),
        ),
    );
    let url = Billing::of(app.state(), &ana)
        .checkout("local-year")
        .await
        .unwrap();
    assert_eq!(url, "https://linking.xendit.co/1");
    let sent = http.sent();
    let plan = sent
        .iter()
        .find(|r| r.url.ends_with("/recurring/plans"))
        .unwrap();
    assert_eq!(plan.json()["immediate_action_type"], "FULL_AMOUNT");
    assert_eq!(plan.json()["schedule"]["interval"], "YEAR");
    // The key given in code, not the one in .env.
    // Basic auth with the key and no password: base64("xnd_in_code:").
    assert_eq!(plan.header("authorization"), Some("Basic eG5kX2luX2NvZGU6"));

    // Every answer queued first (the fake gives them in turn): a customer
    // without an id, Xendit's refusal, then one that works but whose plan
    // has no linking page, so the customer goes back to the app.
    let app = app_with(billing()).await;
    let bo = user(&app, "Bo").await;
    let http = app.fake_http();
    http.on(
        &format!("POST {XENDIT}/customers"),
        FakeResponse::json(200, json!({})),
    );
    http.on(
        &format!("POST {XENDIT}/customers"),
        FakeResponse::json(
            400,
            json!({ "error_code": "API_VALIDATION_ERROR", "message": "email is invalid" }),
        ),
    );
    http.on(
        &format!("POST {XENDIT}/customers"),
        FakeResponse::json(200, json!({ "id": "cust-2" })),
    );
    http.on(
        &format!("POST {XENDIT}/recurring/plans"),
        FakeResponse::json(200, json!({ "id": "repl_2" })),
    );
    let err = Billing::of(app.state(), &bo)
        .checkout("local-year")
        .await
        .unwrap_err();
    assert!(err_text(err).contains("Xendit made a customer without an id"));
    let shown = err_text(
        Billing::of(app.state(), &bo)
            .checkout("local-year")
            .await
            .unwrap_err(),
    );
    assert!(
        shown.contains("Xendit answered 400 Bad Request: API_VALIDATION_ERROR email is invalid"),
        "{shown}"
    );
    let url = Billing::of(app.state(), &bo)
        .checkout("local-year")
        .await
        .unwrap();
    assert!(url.ends_with("/billing/return"), "{url}");
}

#[renox::test]
async fn stripe_from_config_and_its_refusals() {
    let app = app_with(
        Billing::new()
            .plan(
                Plan::new("basic", "Basic")
                    .price(900, "USD", Interval::Month)
                    .price_id("stripe", "price_basic"),
            )
            .plan(Plan::new("bare", "Bare").price(500, "USD", Interval::Month))
            .stripe(),
    )
    .await;
    // STRIPE_SECRET isn't set: Stripe is off, so nothing sells.
    let ana = user(&app, "Ana").await;
    let err = Billing::of(app.state(), &ana)
        .checkout("basic")
        .await
        .unwrap_err();
    assert!(err_text(err).contains("no payment gateway is set up for the plan `basic`"));

    let app = TestApp::with_config(
        App::new().module(Auth::new().account()).module(
            Billing::new()
                .plan(
                    Plan::new("basic", "Basic")
                        .price(900, "USD", Interval::Month)
                        .price_id("stripe", "price_basic"),
                )
                .plan(Plan::new("bare", "Bare").price(500, "USD", Interval::Month))
                .stripe(),
        ),
        |c| {
            c.vars.insert("STRIPE_SECRET".into(), "sk_from_env".into());
            c.vars
                .insert("STRIPE_WEBHOOK_SECRET".into(), "whsec_test".into());
        },
    )
    .await;
    let ana = user(&app, "Ana").await;
    // Every answer queued first: Ana's customer, then Bo's without an id,
    // then Stripe's refusal for Bo; one session without a URL.
    let http = app.fake_http();
    http.on(
        &format!("POST {STRIPE}/customers"),
        FakeResponse::json(200, json!({ "id": "cus_1" })),
    );
    http.on(
        &format!("POST {STRIPE}/customers"),
        FakeResponse::json(200, json!({})),
    );
    http.on(
        &format!("POST {STRIPE}/customers"),
        FakeResponse::json(
            402,
            json!({ "error": { "message": "Your card was declined." } }),
        ),
    );
    http.on(
        &format!("POST {STRIPE}/checkout/sessions"),
        FakeResponse::json(200, json!({ "id": "cs_1" })),
    );
    // A plan without a Stripe price.
    let err = Billing::of(app.state(), &ana)
        .checkout("bare")
        .await
        .unwrap_err();
    assert!(
        err_text(err).contains("the plan `bare` has no Stripe price"),
        "STRIPE_PRICE_BARE"
    );
    // A session without a URL.
    let err = Billing::of(app.state(), &ana)
        .checkout("basic")
        .await
        .unwrap_err();
    assert!(err_text(err).contains("Stripe made a checkout session without a URL"));
    assert_eq!(
        http.sent()[0].header("authorization"),
        Some("Bearer sk_from_env")
    );

    let bo = user(&app, "Bo").await;
    let shown = err_text(
        Billing::of(app.state(), &bo)
            .checkout("basic")
            .await
            .unwrap_err(),
    );
    assert!(
        shown.contains("Stripe made a customer without an id"),
        "{shown}"
    );
    let shown = err_text(
        Billing::of(app.state(), &bo)
            .checkout("basic")
            .await
            .unwrap_err(),
    );
    assert!(
        shown.contains("Stripe answered 402 Payment Required: Your card was declined."),
        "{shown}"
    );
}

#[renox::test]
async fn stripe_swaps_and_cancels_need_a_price_an_item_and_an_id() {
    let app =
        app_with(billing().plan(Plan::new("bare", "Bare").price(500, "USD", Interval::Month)))
            .await;
    let (ana, _) = subscribed(&app).await;
    let err = Billing::of(app.state(), &ana)
        .swap("bare")
        .await
        .unwrap_err();
    assert!(err_text(err).contains("the plan `bare` has no Stripe price"));
    app.fake_http().on(
        &format!("GET {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, json!({ "id": "sub_1", "items": { "data": [] } })),
    );
    let err = Billing::of(app.state(), &ana)
        .swap("basic")
        .await
        .unwrap_err();
    assert!(err_text(err).contains("the Stripe subscription sub_1 has no item"));
    // The same plan: nothing to do, nothing sent.
    let sent = app.fake_http().sent().len();
    assert_eq!(
        Billing::of(app.state(), &ana)
            .swap("pro")
            .await
            .unwrap()
            .plan,
        "pro"
    );
    assert_eq!(app.fake_http().sent().len(), sent);

    // A row the gateway never named.
    let bo = user(&app, "Bo").await;
    row(&app, &bo, "stripe", None).await;
    let err = Billing::of(app.state(), &bo)
        .cancel_now()
        .await
        .unwrap_err();
    assert!(err_text(err).contains("has no gateway id"));
}

#[renox::test]
async fn subscriptions_on_a_gateway_that_is_gone() {
    let app = app().await;
    let ana = user(&app, "Ana").await;
    let mut sub = row(&app, &ana, "paypal", Some("P-1")).await;
    let billing = || Billing::of(app.state(), &ana);
    assert!(
        err_text(billing().cancel_now().await.unwrap_err())
            .contains("the payment gateway `paypal` isn't set up")
    );
    assert!(
        err_text(billing().customer_id("paypal").await.unwrap_err())
            .contains("the payment gateway `paypal` isn't set up")
    );
    sub.ends_at = Some(renox::db::now() + renox::chrono::Duration::days(3));
    sub.save(app.db()).await.unwrap();
    assert!(!billing().can_resume().await.unwrap());
    assert!(err_text(billing().resume().await.unwrap_err()).contains("isn't set up"));
}

#[renox::test]
async fn customers_by_name_and_owner_and_their_states() {
    let app = app().await;
    let (ana, _) = subscribed(&app).await;
    let billing = Billing::of(app.state(), &ana);
    assert!(billing.owner().is_user(ana.id));
    assert!(!billing.owner().is_user(ana.id + 1));
    assert_eq!(billing.subscriptions().await.unwrap().len(), 1);
    assert!(!billing.on_grace_period().await.unwrap());
    let team = Billing::of(app.state(), &ana).named("team");
    assert!(team.subscription().await.unwrap().is_none());
    assert!(!team.subscribed().await.unwrap());
    // An Owner is billable as it is.
    use renox_billing::Billable;
    let owner = renox_billing::Owner::new("team", 3);
    assert_eq!(owner.owner().key(), "team:3");
    assert!(!owner.is_user(3));
    // Found by the gateway's id.
    let found = Subscription::find_at(app.db(), "stripe", "sub_1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.billable_id, ana.id);
    assert!(
        Subscription::find_at(app.db(), "xendit", "sub_1")
            .await
            .unwrap()
            .is_none()
    );

    // Not resumable before it's canceled; resumed once it is.
    assert!(
        err_text(Billing::of(app.state(), &ana).resume().await.unwrap_err())
            .contains("Only a canceled subscription")
    );
    assert!(
        err_text(
            Billing::of(app.state(), &ana)
                .start_trial("pro")
                .await
                .unwrap_err()
        )
        .contains("You've had a subscription already")
    );
    assert!(
        err_text(
            Billing::of(app.state(), &ana)
                .start_trial("basic")
                .await
                .unwrap_err()
        )
        .contains("Basic has no free trial")
    );
}

#[renox::test]
async fn a_new_checkout_during_a_grace_period_starts_at_the_old_end() {
    let app = app().await;
    let (ana, _) = subscribed(&app).await;
    let mut sub = latest(&app, &ana).await;
    let end = renox::db::now() + renox::chrono::Duration::days(20);
    sub.ends_at = Some(end);
    sub.save(app.db()).await.unwrap();
    assert!(
        Billing::of(app.state(), &ana)
            .on_grace_period()
            .await
            .unwrap()
    );
    // Canceled: changing plans asks to resume first.
    assert!(
        err_text(
            Billing::of(app.state(), &ana)
                .swap("basic")
                .await
                .unwrap_err()
        )
        .contains("Resume the subscription")
    );
    let http = stripe_checkout(&app);
    Billing::of(app.state(), &ana)
        .checkout("basic")
        .await
        .unwrap();
    let session = http
        .sent()
        .into_iter()
        .rev()
        .find(|r| r.url.ends_with("/checkout/sessions"))
        .unwrap();
    assert_eq!(
        param(&form(&session.body), "subscription_data[trial_end]"),
        Some(end.timestamp().to_string())
    );
}

#[renox::test]
async fn generic_trials_cancel_resume_and_go_where_the_module_says() {
    let app = app_with(billing().generic_trials().redirect_to("/dashboard")).await;
    let ana = user(&app, "Ana").await;
    app.post("/billing/trial/pro", &[])
        .await
        .assert_redirect("/dashboard");
    let billing = || Billing::of(app.state(), &ana);
    assert!(billing().on_trial().await.unwrap());
    // A plan change during the trial: just the row.
    assert_eq!(billing().swap("basic").await.unwrap().plan, "basic");
    let canceled = billing().cancel().await.unwrap();
    assert!(canceled.on_grace_period());
    assert!(billing().can_resume().await.unwrap());
    let back = billing().resume().await.unwrap();
    assert_eq!(back.status, SubscriptionStatus::Trialing);
    assert!(back.ends_at.is_none());
    // Over htmx, pages answer with HX-Redirect.
    app.htmx()
        .post("/billing/cancel", &[])
        .await
        .assert_hx_redirect("/dashboard");
    app.get("/account").await.assert_see("Canceled");
    let gone = billing().cancel_now().await.unwrap();
    assert!(!gone.valid());
}

#[renox::test]
async fn the_module_needs_to_be_added_and_its_gateways_found() {
    let app = TestApp::new(App::new().module(Auth::new())).await;
    let ana = user(&app, "Ana").await;
    let err = Billing::of(app.state(), &ana)
        .checkout("pro")
        .await
        .unwrap_err();
    assert!(err_text(err).contains("add the module with App::module(Billing::new()"));
    let app = app_with(
        billing().plan(
            Plan::new("pal", "Pal")
                .price(1, "USD", Interval::Month)
                .via("paypal"),
        ),
    )
    .await;
    let ana = user(&app, "Ana").await;
    let err = Billing::of(app.state(), &ana)
        .checkout("pal")
        .await
        .unwrap_err();
    assert!(err_text(err).contains("no payment gateway is set up for the plan `pal`"));
    let shown = format!(
        "{:?}",
        Billing::default()
            .gateway(renox_billing::Stripe::new("sk_live_x", "whsec_x"))
            .plan(Plan::new("a", "A"))
    );
    assert!(
        shown.contains("\"stripe\"") && shown.contains("\"a\"") && !shown.contains("sk_live_x"),
        "{shown}"
    );
    assert!(format!("{:?}", Billing::new().stripe()).contains("stripe"));
}

#[renox::test]
async fn webhooks_name_their_events_and_ignore_what_they_dont_use() {
    let app = app().await;
    let ana = user(&app, "Ana").await;
    // Xendit's `webhook-id` header is the event id.
    app.request()
        .header("x-callback-token", "callback-token")
        .header("webhook-id", "whk_42")
        .post_body(
            "/billing/webhooks/xendit",
            "application/json",
            r#"{"event":"payment.succeeded","data":{}}"#,
        )
        .await
        .assert_ok();
    // A long Stripe id is hashed to fit a header.
    let long = format!("evt_{}", "x".repeat(200));
    stripe_webhook(&app, &long, "charge.refunded", now(&app).await, json!({}))
        .await
        .assert_ok();
    app.run_jobs().await;
    let ids: Vec<String> = renox::db::sql("SELECT event_id FROM webhook_calls ORDER BY id")
        .scalars(app.db())
        .await
        .unwrap();
    assert_eq!(ids[0], "xendit:whk_42");
    assert!(ids[1].starts_with("stripe:sha256:"), "{ids:?}");
    // Neither changed anything.
    assert!(
        Billing::of(app.state(), &ana)
            .subscription()
            .await
            .unwrap()
            .is_none()
    );
    // An unknown gateway is a 404; a body over the limit a 413.
    app.request()
        .post_body("/billing/webhooks/paypal", "application/json", "{}")
        .await
        .assert_status(404);
    let big = format!(
        r#"{{"id":"evt_big","pad":"{}"}}"#,
        "x".repeat(3 * 1024 * 1024)
    );
    app.request()
        .post_body("/billing/webhooks/stripe", "application/json", big)
        .await
        .assert_status(413);
}

#[renox::test]
async fn a_stored_call_whose_gateway_went_away_fails() {
    let app = app().await;
    renox::db::sql(
        "INSERT INTO webhook_calls (provider, event_id, payload, status, received_at) \
         VALUES ('billing', 'paypal:evt_1', ?, 'failed', 0)",
    )
    .bind(b"{}".to_vec())
    .execute(app.db())
    .await
    .unwrap();
    let id: i64 = renox::db::sql("SELECT id FROM webhook_calls")
        .scalar(app.db())
        .await
        .unwrap();
    assert!(renox::webhook::retry(app.state(), id).await.unwrap());
    app.run_jobs().await;
    let error: String = renox::db::sql("SELECT error FROM webhook_calls WHERE id = ?")
        .bind(id)
        .scalar(app.db())
        .await
        .unwrap();
    assert!(
        error.contains("the payment gateway `paypal` isn't set up any more"),
        "{error}"
    );
}

#[renox::test]
async fn webhooks_for_no_known_owner_or_plan_and_payments_by_customer() {
    let app = app().await;
    let ana = user(&app, "Ana").await;
    app.fake_events();
    let period_end = now(&app).await + 30 * DAY as i64;
    // No owner in its metadata and an unknown customer: ignored.
    let mut orphan = stripe_sub(&ana, "active", "price_pro", "pro", period_end);
    orphan["metadata"] = json!({});
    orphan["customer"] = json!("cus_unknown");
    stripe_webhook(
        &app,
        "evt_o",
        "customer.subscription.created",
        now(&app).await,
        orphan,
    )
    .await
    .assert_ok();
    // An owner but a plan the app doesn't sell: ignored.
    let mut stranger = stripe_sub(&ana, "active", "price_other", "gold", period_end);
    stranger["id"] = json!("sub_2");
    stripe_webhook(
        &app,
        "evt_s",
        "customer.subscription.created",
        now(&app).await,
        stranger,
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    assert!(
        Billing::of(app.state(), &ana)
            .subscriptions()
            .await
            .unwrap()
            .is_empty()
    );

    // A failed invoice with only its customer: the owner is found by it.
    stripe_checkout(&app);
    Billing::of(app.state(), &ana)
        .checkout("basic")
        .await
        .unwrap();
    stripe_webhook(
        &app,
        "evt_inv",
        "invoice.payment_failed",
        now(&app).await,
        json!({ "id": "in_1", "amount_due": 900, "currency": "usd", "customer": "cus_1" }),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let id = ana.id;
    app.assert_emitted::<PaymentFailed>(move |e| {
        e.owner.as_ref().is_some_and(|o| o.is_user(id))
            && e.subscription.is_none()
            && e.amount == 900
    });

    // Canceled by Stripe without an end: it ends now.
    let (bo, _) = {
        let app = &app;
        let bo = user(app, "Bo").await;
        let mut sub = stripe_sub(&bo, "active", "price_pro", "pro", period_end);
        sub["id"] = json!("sub_bo");
        stripe_webhook(
            app,
            "evt_b1",
            "customer.subscription.created",
            now(app).await,
            sub.clone(),
        )
        .await
        .assert_ok();
        app.run_jobs().await;
        sub["status"] = json!("canceled");
        stripe_webhook(
            app,
            "evt_b2",
            "customer.subscription.deleted",
            now(app).await + 1,
            sub,
        )
        .await
        .assert_ok();
        app.run_jobs().await;
        (bo, ())
    };
    let ended = latest(&app, &bo).await;
    assert_eq!(ended.status, SubscriptionStatus::Canceled);
    assert!(ended.ends_at.is_some() && !ended.valid());
    app.assert_emitted::<SubscriptionCanceled>(|e| {
        e.subscription.gateway_id.as_deref() == Some("sub_bo")
    });
}

/// The guards: a guest, an htmx visitor without a plan, and a lookup that
/// fails.
struct Open;

impl Module for Open {
    fn name(&self) -> &'static str {
        "open"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/open-reports", || async { "reports" })
            .require_subscription()
    }
}

#[renox::test]
async fn guards_for_guests_htmx_and_a_broken_table() {
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new().account())
            .module(billing())
            .module(Open),
        |_| {},
    )
    .await;
    app.get("/open-reports").await.assert_status(401);
    user(&app, "Ana").await;
    app.htmx()
        .get("/open-reports")
        .await
        .assert_hx_redirect("/billing");
    renox::db::sql("ALTER TABLE subscriptions RENAME TO subscriptions_away")
        .execute(app.db())
        .await
        .unwrap();
    app.get("/open-reports").await.assert_status(500);
}

#[renox::test]
async fn billing_changes_go_to_the_audit_log_with_amounts() {
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new().account())
            .module(renox::audit::Audit)
            .module(billing())
            .module(Area),
        |config| {
            config
                .vars
                .insert("XENDIT_SECRET_KEY".into(), "xnd_development_1".into());
            config
                .vars
                .insert("XENDIT_CALLBACK_TOKEN".into(), "callback-token".into());
        },
    )
    .await;
    on_xendit(&app).await;
    // A failed cycle for a plan nobody has: no owner, nothing written.
    xendit_webhook(
        &app,
        "recurring.cycle.failed",
        "2026-11-05T10:00:00Z",
        json!({ "id": "rpcyc_x", "plan_id": "repl_unknown", "status": "FAILED", "amount": 5, "currency": "IDR" }),
    )
    .await
    .assert_ok();
    xendit_webhook(
        &app,
        "recurring.cycle.failed",
        "2026-11-06T10:00:00Z",
        json!({ "id": "rpcyc_7", "plan_id": "repl_9", "status": "FAILED", "amount": 99_000, "currency": "IDR" }),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let rows = renox::db::sql("SELECT action, data FROM audit_logs ORDER BY id")
        .fetch_all(app.db())
        .await
        .unwrap();
    let actions: Vec<String> = rows.iter().map(|r| r.try_get("action").unwrap()).collect();
    assert!(
        actions.contains(&"billing.subscribed".to_owned()),
        "{actions:?}"
    );
    let failed: Vec<String> = rows
        .iter()
        .filter(|r| r.try_get::<String>("action").unwrap() == "billing.payment_failed")
        .map(|r| r.try_get::<String>("data").unwrap_or_default())
        .collect();
    assert_eq!(failed.len(), 1, "{actions:?}");
    assert!(failed[0].contains("99000"), "{failed:?}");
}

/// What a test logs, through a subscriber of this thread only.
fn capture_logs() -> (
    std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    tracing::subscriber::DefaultGuard,
) {
    #[derive(Clone)]
    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
        type Writer = Sink;
        fn make_writer(&'a self) -> Sink {
            self.clone()
        }
    }
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(Sink(lines.clone()))
        .with_ansi(false)
        .finish();
    (lines, tracing::subscriber::set_default(subscriber))
}

#[renox::test]
async fn a_deleted_account_loses_its_rows_even_when_the_gateway_refuses() {
    let (logs, _logged) = capture_logs();
    let app = app().await;
    let (ana, _) = subscribed(&app).await;
    // An old subscription without a gateway id (a trial that never paid):
    // nothing to cancel at the gateway, its row goes too.
    renox::db::sql(
        "INSERT INTO subscriptions (billable_type, billable_id, name, plan, gateway, status) \
         VALUES ('user', ?, 'old', 'pro', '', 'trialing')",
    )
    .bind(ana.id)
    .execute(app.db())
    .await
    .unwrap();
    app.fake_http().on(
        &format!("DELETE {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(500, json!({ "error": { "message": "try later" } })),
    );
    app.confirm_password();
    app.post("/account", &[("_method", "DELETE"), ("password", PASSWORD)])
        .await
        .assert_status(303);
    let _ = ana;
    app.assert_database_count("subscriptions", 0).await;
    app.assert_database_count("billing_customers", 0).await;
    let logged = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    assert!(
        logged.contains("billing: couldn't cancel a deleted account's subscription"),
        "{logged}"
    );
    assert_eq!(
        logged.matches("couldn't cancel").count(),
        1,
        "only the one with a gateway id was cancelled there"
    );
}

#[renox::test]
async fn plans_show_their_description() {
    let app = app_with(
        billing().plan(
            Plan::new("day", "Day pass")
                .price(500, "USD", Interval::Day)
                .description("For one busy day."),
        ),
    )
    .await;
    user(&app, "Ana").await;
    app.get("/billing")
        .await
        .assert_see("For one busy day.")
        .assert_see("$5.00 / day");
}

// ---------- #315: named guards, pause, the webhook context ----------

#[renox::test]
async fn the_named_guards_check_that_subscription_and_not_the_default() {
    let app = app().await;
    let (ana, _) = subscribed(&app).await;
    // Ana's `default` subscription doesn't open the garage.
    app.get("/garage").await.assert_redirect("/billing");
    app.request()
        .header("accept", "application/json")
        .get("/garage")
        .await
        .assert_status(402);
    // A `garage` subscription to Basic opens the first page, not the Pro one.
    let mut garage = row(&app, &ana, "stripe", Some("sub_garage")).await;
    garage.name = "garage".into();
    garage.plan = "basic".into();
    garage.save(app.db()).await.unwrap();
    app.get("/garage").await.assert_ok().assert_see("garage");
    app.get("/garage/pro").await.assert_redirect("/billing");
    garage.plan = "pro".into();
    garage.save(app.db()).await.unwrap();
    app.get("/garage/pro").await.assert_ok();
    // The default guards still look at `default` only.
    app.get("/reports").await.assert_ok();
}

#[renox::test]
async fn pausing_stops_billing_and_taking_it_up_again_restores_it() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    app.fake_events();
    let http = app.fake_http();
    let mut paused = stripe_sub(&ana, "active", "price_pro", "pro", period_end);
    paused["pause_collection"] = json!({ "behavior": "void", "resumes_at": null });
    // Queue every answer first: the pause, then the unpause.
    http.on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, paused.clone()),
    )
    .on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(
            200,
            stripe_sub(&ana, "active", "price_pro", "pro", period_end),
        ),
    );
    app.get("/account")
        .await
        .assert_ok()
        .assert_see("Pause billing");
    app.post("/billing/pause", &[])
        .await
        .assert_redirect("/account");
    http.assert_sent(|r| r.body.contains("pause_collection%5Bbehavior%5D=void"));
    let subscription = latest(&app, &ana).await;
    assert!(subscription.paused());
    assert!(!subscription.valid());
    app.assert_emitted::<SubscriptionPaused>(|e| e.subscription.id == subscription.id);
    // The guards turn a paused subscriber away; the account page offers to go on.
    app.get("/reports").await.assert_redirect("/billing");
    app.get("/account")
        .await
        .assert_ok()
        .assert_see("Billing is paused")
        .assert_see("Take it up again");

    app.post("/billing/unpause", &[])
        .await
        .assert_redirect("/account");
    http.assert_sent(|r| r.body.contains("pause_collection="));
    let subscription = latest(&app, &ana).await;
    assert!(!subscription.paused() && subscription.valid());
    app.assert_emitted::<SubscriptionUnpaused>(|e| e.subscription.id == subscription.id);
    app.get("/reports").await.assert_ok();
}

#[renox::test]
async fn a_pause_until_a_date_is_sent_and_stripes_webhooks_keep_it_paused() {
    let app = app().await;
    let (ana, period_end) = subscribed(&app).await;
    let http = app.fake_http();
    let until = now(&app).await + 7 * DAY as i64;
    let mut paused = stripe_sub(&ana, "active", "price_pro", "pro", period_end);
    paused["pause_collection"] = json!({ "behavior": "void", "resumes_at": until });
    http.on(
        &format!("POST {STRIPE}/subscriptions/sub_1"),
        FakeResponse::json(200, paused.clone()),
    );
    let resumes_at = renox::chrono::DateTime::from_timestamp(until, 0);
    Billing::of(app.state(), &ana)
        .pause(resumes_at)
        .await
        .unwrap();
    http.assert_sent(|r| r.body.contains(&format!("resumes_at%5D={until}")));
    // Pausing twice is a no-op; a webhook about the paused subscription
    // keeps it paused (Stripe says `active` with `pause_collection`).
    assert!(
        Billing::of(app.state(), &ana)
            .pause(None)
            .await
            .unwrap()
            .paused()
    );
    stripe_webhook(
        &app,
        "evt_renewed",
        "customer.subscription.updated",
        now(&app).await + 5,
        paused,
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    assert!(latest(&app, &ana).await.paused());
    // A webhook without `pause_collection` is the gateway resuming it.
    stripe_webhook(
        &app,
        "evt_auto_resumed",
        "customer.subscription.updated",
        now(&app).await + 10,
        stripe_sub(&ana, "active", "price_pro", "pro", period_end),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    assert!(latest(&app, &ana).await.valid());
}

#[renox::test]
async fn a_gateway_that_cannot_pause_says_so_and_changes_nothing() {
    let app = app().await;
    let ana = on_xendit(&app).await;
    assert!(!Billing::of(app.state(), &ana).can_pause().await.unwrap());
    let err = Billing::of(app.state(), &ana)
        .pause(None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::BadRequest(ref m) if m.contains("can't pause a subscription")),
        "{err:?}"
    );
    assert!(latest(&app, &ana).await.valid());
    // Nothing to take up again, and the page toasts the refusal.
    let err = Billing::of(app.state(), &ana).unpause().await.unwrap_err();
    assert!(matches!(err, Error::BadRequest(_)), "{err:?}");
    app.post("/billing/pause", &[])
        .await
        .assert_redirect("/account");
    assert!(latest(&app, &ana).await.valid());
}

/// A gateway whose signing key is an app-provided value, not configuration.
struct Keyed;

/// The app's own settings, given with `App::provide`.
struct KeyedSettings {
    token: &'static str,
}

impl renox_billing::Gateway for Keyed {
    fn name(&self) -> &str {
        "keyed"
    }
    fn label(&self) -> &str {
        "Keyed"
    }
    fn configured(&self, _config: &Config) -> bool {
        true
    }
    fn create_customer<'a>(
        &'a self,
        _state: &'a AppState,
        _owner: &'a renox_billing::Owner,
    ) -> renox_billing::BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("keyed-1".into()) })
    }
    fn checkout<'a>(
        &'a self,
        _state: &'a AppState,
        _request: &'a renox_billing::CheckoutRequest,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::Checkout>> {
        Box::pin(async { Ok(renox_billing::Checkout::redirect("/pay")) })
    }
    fn swap<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
        _plan: &'a Plan,
        _prorate: bool,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::Remote>> {
        Box::pin(async { Err(Error::BadRequest("no".into())) })
    }
    fn cancel<'a>(
        &'a self,
        _state: &'a AppState,
        _subscription: &'a Subscription,
        _at_period_end: bool,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::Remote>> {
        Box::pin(async { Err(Error::BadRequest("no".into())) })
    }
    fn verify_webhook(
        &self,
        _config: &Config,
        _headers: &renox::axum::http::HeaderMap,
        _body: &[u8],
    ) -> Result {
        Err(Error::Unauthorized)
    }
    fn verify_webhook_with(
        &self,
        state: &AppState,
        headers: &renox::axum::http::HeaderMap,
        _body: &[u8],
    ) -> Result {
        let settings = state
            .provided::<KeyedSettings>()
            .ok_or(Error::Unauthorized)?;
        let sent = headers
            .get("x-keyed-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        renox::webhook::ensure(renox::webhook::same(sent, settings.token))
    }
    fn parse_webhook(&self, _body: &[u8]) -> Result<Vec<renox_billing::Notice>> {
        Ok(Vec::new())
    }
    fn parse_webhook_with(
        &self,
        state: &AppState,
        _body: &[u8],
    ) -> Result<Vec<renox_billing::Notice>> {
        let settings = state
            .provided::<KeyedSettings>()
            .ok_or(Error::Unauthorized)?;
        Ok(vec![renox_billing::Notice::Payment(
            renox_billing::Payment::new(settings.token, 500, "usd", true),
        )])
    }
}

#[renox::test]
async fn a_custom_gateway_verifies_webhooks_with_the_app_state() {
    let app = TestApp::new(
        App::new()
            .provide(KeyedSettings { token: "s3cret" })
            .module(Auth::new().account())
            .module(
                Billing::new()
                    .gateway(Keyed)
                    .plan(Plan::new("basic", "Basic").price(900, "USD", Interval::Month)),
            )
            .module(Area),
    )
    .await;
    app.fake_events();
    let wrong = app
        .request()
        .header("x-keyed-token", "nope")
        .post_body(
            "/billing/webhooks/keyed",
            "application/json",
            r#"{"id":"e1"}"#,
        )
        .await;
    wrong.assert_status(401);
    app.request()
        .header("x-keyed-token", "s3cret")
        .post_body(
            "/billing/webhooks/keyed",
            "application/json",
            r#"{"id":"e2"}"#,
        )
        .await
        .assert_ok();
    app.run_jobs().await;
    // The worker read the event with the state too.
    app.assert_emitted::<PaymentSucceeded>(|e| e.payment_id == "s3cret");
    // The old method alone (the default of the new one) still works.
    use renox_billing::Gateway;
    assert!(
        Manual
            .verify_webhook_with(app.state(), &Default::default(), b"")
            .is_ok()
    );
}
