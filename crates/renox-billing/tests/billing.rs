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
async fn a_resolved_key_is_asked_on_every_call_and_never_shown() {
    let key = std::sync::Arc::new(std::sync::Mutex::new("sk_first".to_string()));
    let shared = key.clone();
    let stripe = Stripe::resolved(move |_state| {
        let key = shared.lock().unwrap().clone();
        async move { Ok(Some(key)) }
    });
    assert_eq!(
        format!("{stripe:?}").matches("sk_").count(),
        0,
        "{stripe:?}"
    );
    let billing = Billing::new()
        .plan(
            Plan::new("basic", "Basic")
                .price(900, "USD", Interval::Month)
                .price_id("stripe", "price_basic"),
        )
        .gateway(stripe);
    let app = app_with(billing).await;
    user(&app, "Ana").await;
    let http = stripe_checkout(&app);
    app.post("/billing/checkout/basic", &[])
        .await
        .assert_status(303);
    // The key is changed in the store: the next call uses it, no restart.
    *key.lock().unwrap() = "sk_second".to_string();
    app.post("/billing/checkout/basic", &[])
        .await
        .assert_status(303);
    let auth: Vec<_> = http
        .sent()
        .iter()
        .map(|r| r.header("authorization").map(str::to_owned))
        .collect();
    assert_eq!(
        auth,
        [
            Some("Bearer sk_first".to_string()),
            Some("Bearer sk_first".to_string()),
            Some("Bearer sk_second".to_string()),
        ]
    );
}

#[renox::test]
async fn a_resolver_without_a_key_fails_without_showing_anything() {
    let billing = Billing::new()
        .plan(
            Plan::new("basic", "Basic")
                .price(900, "USD", Interval::Month)
                .price_id("stripe", "price_basic"),
        )
        .gateway(Stripe::resolved(|_state| async { Ok(None) }));
    let app = app_with(billing).await;
    user(&app, "Ana").await;
    let http = stripe_checkout(&app);
    let response = app.post("/billing/checkout/basic", &[]).await;
    assert!(http.sent().is_empty(), "no call without a key");
    assert!(!response.text().contains("sk_"));
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

#[renox::test]
async fn payments_and_refunds_are_stored_with_money_and_the_reference() {
    use renox::money::Money;
    use renox_billing::{BillingPayment, BillingRefund, PaymentKind, PaymentStatus, RefundStatus};

    let app = app().await;
    let mut new = BillingPayment::new("stripe", PaymentKind::Link, Money::usd(4_500));
    new.reference_type = Some("job".into());
    new.reference_id = Some("17".into());
    new.gateway_id = Some("cs_1".into());
    let payment = BillingPayment::create(app.db(), new).await.unwrap();
    assert_eq!(payment.amount, Money::usd(4_500));
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert!(payment.owner().is_none());
    assert_eq!(payment.refundable(), Money::usd(0));

    let found = BillingPayment::find_at(app.db(), "stripe", "cs_1")
        .await
        .unwrap()
        .expect("found by gateway id");
    assert_eq!(found.id, payment.id);
    let by_reference = BillingPayment::for_reference(app.db(), "job", "17")
        .await
        .unwrap();
    assert_eq!(by_reference.len(), 1);

    let mut paid = found;
    paid.status = PaymentStatus::PartiallyRefunded;
    paid.refunded_amount = 1_000;
    paid.save(app.db()).await.unwrap();
    assert_eq!(paid.refunded(), Money::usd(1_000));
    assert_eq!(paid.refundable(), Money::usd(3_500));

    BillingRefund::create(
        app.db(),
        BillingRefund {
            payment_id: paid.id,
            amount: 1_000,
            status: RefundStatus::Succeeded,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(paid.refunds(app.db()).await.unwrap().len(), 1);
}

#[test]
fn unsupported_names_the_gateway_and_the_operation() {
    use renox_billing::Unsupported;

    let err: Error = Unsupported::new("xendit", "accept_dispute").into();
    let found = Unsupported::of(&err).expect("downcast");
    assert_eq!(found.gateway, "xendit");
    assert_eq!(found.operation, "accept_dispute");
    assert!(Unsupported::of(&Error::NotFound).is_none());
}

/// A webhook signed with `secret`, posted to `path`.
async fn signed_stripe(app: &TestApp, path: &str, id: &str, secret: &str) -> TestResponse {
    let payload = json!({
        "id": id, "object": "event", "type": "ping", "created": 1,
        "data": { "object": {} },
    })
    .to_string();
    let t = now(app).await;
    let signature = renox::webhook::hmac_sha256_hex(secret, format!("{t}.{payload}"));
    app.request()
        .header("stripe-signature", &format!("t={t},v1={signature}"))
        .post_body(path, "application/json", payload)
        .await
}

#[renox::test]
async fn webhooks_of_a_named_account_use_that_accounts_secret() {
    let app = App::new()
        .module(Auth::new().account())
        .module(Billing::new().gateway(Stripe::from_config()));
    let app = TestApp::with_config(app, |config| {
        for (name, value) in [
            ("STRIPE_SECRET", "sk_test_1"),
            ("STRIPE_WEBHOOK_SECRET", "whsec_default"),
            ("STRIPE_WEBHOOK_SECRET_ACME_LLC", "whsec_acme"),
            ("STRIPE_WEBHOOK_SECRET_B", "whsec_b"),
        ] {
            config.vars.insert(name.into(), value.into());
        }
    })
    .await;
    let acme = "/billing/webhooks/stripe/acme-llc";

    // The account's secret verifies; the default's and another account's don't.
    signed_stripe(&app, acme, "evt_1", "whsec_acme")
        .await
        .assert_ok();
    signed_stripe(&app, acme, "evt_2", "whsec_default")
        .await
        .assert_unauthorized();
    signed_stripe(&app, acme, "evt_3", "whsec_b")
        .await
        .assert_unauthorized();
    // The default URL keeps the default secret only.
    signed_stripe(&app, "/billing/webhooks/stripe", "evt_1", "whsec_default")
        .await
        .assert_ok();
    signed_stripe(&app, "/billing/webhooks/stripe", "evt_4", "whsec_acme")
        .await
        .assert_unauthorized();
    // Dedupe is per account: the same event id at the default account is
    // another event; a repeat at the same account changes nothing.
    app.assert_database_count("webhook_calls", 2).await;
    signed_stripe(&app, acme, "evt_1", "whsec_acme")
        .await
        .assert_ok();
    app.assert_database_count("webhook_calls", 2).await;

    // An unknown or malformed account, or a gateway without accounts, is a 404.
    for path in [
        "/billing/webhooks/stripe/nobody",
        "/billing/webhooks/stripe/bad%20name",
        "/billing/webhooks/paypal/acme-llc",
    ] {
        signed_stripe(&app, path, "evt_9", "whsec_acme")
            .await
            .assert_not_found();
    }
}

/// A gateway that implements none of the one-off payment methods.
struct Bare;

impl renox_billing::Gateway for Bare {
    fn name(&self) -> &str {
        "bare"
    }
    fn label(&self) -> &str {
        "Bare"
    }
    fn configured(&self, _config: &Config) -> bool {
        true
    }
    fn create_customer<'a>(
        &'a self,
        _state: &'a AppState,
        _owner: &'a renox_billing::Owner,
    ) -> renox_billing::BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("bare-1".into()) })
    }
    fn checkout<'a>(
        &'a self,
        _state: &'a AppState,
        _request: &'a renox_billing::CheckoutRequest,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::Checkout>> {
        Box::pin(async { Ok(renox_billing::Checkout::redirect("/bare")) })
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
async fn operations_a_gateway_does_not_have_answer_unsupported() {
    use renox::money::{Currency, Money};
    use renox_billing::payment::{Charge, PaymentLink, Refund, Unsupported};
    use renox_billing::{BillingPayment, Gateway, PaymentKind};

    let app = app().await;
    let state = app.state();
    let money = Money::new(1_000, Currency::parse("USD").unwrap());
    let charge = Charge::new(money).account("acme-llc");
    let link = PaymentLink::new(money);
    let payment = BillingPayment::new("bare", PaymentKind::Charge, money);
    let refund = Refund::new(money);
    // A gateway that implements none of the new methods.
    let gateway = Bare;

    let refusals = [
        (
            "charge",
            gateway.charge(state, charge.request()).await.err(),
        ),
        (
            "payment_link",
            gateway.payment_link(state, link.request()).await.err(),
        ),
        (
            "refund",
            gateway.refund(state, &payment, &refund).await.err(),
        ),
        (
            "payment_status",
            gateway.payment_status(state, &payment).await.err(),
        ),
        (
            "accept_dispute",
            gateway.accept_dispute(state, &payment).await.err(),
        ),
    ];
    for (operation, err) in refusals {
        let err = err.unwrap_or_else(|| panic!("{operation} should be refused"));
        let found = Unsupported::of(&err).expect("a typed refusal");
        assert_eq!(found.gateway, "bare");
        assert_eq!(found.operation, operation);
    }
}

/// A gateway of the app's own that can charge, link, refund and report.
struct PayHub {
    calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl renox_billing::Gateway for PayHub {
    fn name(&self) -> &str {
        "payhub"
    }
    fn label(&self) -> &str {
        "PayHub"
    }
    fn configured(&self, _config: &Config) -> bool {
        true
    }
    fn create_customer<'a>(
        &'a self,
        _state: &'a AppState,
        _owner: &'a renox_billing::Owner,
    ) -> renox_billing::BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("hub-1".into()) })
    }
    fn checkout<'a>(
        &'a self,
        _state: &'a AppState,
        _request: &'a renox_billing::CheckoutRequest,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::Checkout>> {
        Box::pin(async { Ok(renox_billing::Checkout::redirect("/hub")) })
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
    fn parse_webhook(&self, body: &[u8]) -> Result<Vec<renox_billing::Notice>> {
        use renox_billing::{
            DisputeState, DisputeUpdate, Notice, PaymentStatus, PaymentUpdate, RefundStatus,
            RefundUpdate,
        };
        let json: renox::serde_json::Value = renox::serde_json::from_slice(body)?;
        let text = |key: &str| json[key].as_str().unwrap_or_default().to_owned();
        let at = json["at"].as_i64();
        let status = match text("status").as_str() {
            "succeeded" => PaymentStatus::Succeeded,
            "failed" => PaymentStatus::Failed,
            "expired" => PaymentStatus::Expired,
            _ => PaymentStatus::Pending,
        };
        Ok(match text("kind").as_str() {
            "payment" => {
                let mut update = PaymentUpdate::new(text("pay"), status);
                update.at = at;
                vec![Notice::OneOff(update)]
            }
            "refund" => {
                let refund_status = match text("status").as_str() {
                    "succeeded" => RefundStatus::Succeeded,
                    "failed" => RefundStatus::Failed,
                    _ => RefundStatus::Pending,
                };
                let mut update =
                    RefundUpdate::new(text("refund"), refund_status).payment(text("pay"));
                update.reason = json["reason"].as_str().map(str::to_owned);
                vec![Notice::Refund(update)]
            }
            "dispute" => {
                let state = match text("status").as_str() {
                    "won" => DisputeState::Won,
                    "lost" => DisputeState::Lost,
                    _ => DisputeState::Opened,
                };
                vec![Notice::Dispute(
                    DisputeUpdate::new(text("pay"), state).evidence_due_by(renox::db::now()),
                )]
            }
            _ => Vec::new(),
        })
    }
    fn charge<'a>(
        &'a self,
        _state: &'a AppState,
        request: &'a renox_billing::ChargeRequest,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::PaymentPage>> {
        self.calls.lock().unwrap().push(format!(
            "charge {} {} {:?} {:?}",
            request.payment_id,
            request.amount.minor,
            request.account,
            request.metadata()
        ));
        let id = request.payment_id;
        Box::pin(async move {
            Ok(renox_billing::PaymentPage::new(
                format!("pay_{id}"),
                format!("https://pay.example/{id}"),
            ))
        })
    }
    fn payment_link<'a>(
        &'a self,
        state: &'a AppState,
        request: &'a renox_billing::ChargeRequest,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::PaymentPage>> {
        self.charge(state, request)
    }
    fn refund<'a>(
        &'a self,
        _state: &'a AppState,
        payment: &'a renox_billing::BillingPayment,
        refund: &'a renox_billing::Refund,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::RefundResult>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("refund {} {}", payment.id, refund.amount.minor));
        let now = refund.reason.as_deref() == Some("now");
        Box::pin(async move {
            Ok(if now {
                renox_billing::RefundResult::succeeded("re_1")
            } else {
                renox_billing::RefundResult::pending("re_2")
            })
        })
    }
    fn payment_status<'a>(
        &'a self,
        _state: &'a AppState,
        payment: &'a renox_billing::BillingPayment,
    ) -> renox_billing::BoxFuture<'a, Result<renox_billing::PaymentState>> {
        use renox_billing::PaymentStatus;
        let status = match payment.reference_type.as_deref() {
            Some("unpaid") => PaymentStatus::Pending,
            Some("declined") => PaymentStatus::Failed,
            Some("gone") => PaymentStatus::Expired,
            _ if payment.gateway_id.is_some() => PaymentStatus::Succeeded,
            _ => PaymentStatus::Pending,
        };
        Box::pin(async move { Ok(renox_billing::PaymentState::new(status)) })
    }
}

fn assert_send<T: Send>(_: &T) {}

/// Charges and links are written with their reference, owner and account,
/// refunds are checked before the gateway and add up, and a gateway that
/// cannot leaves no row.
#[renox::test]
async fn one_off_payments_are_stored_refunded_and_checked() {
    use renox::money::Money;
    use renox_billing::{
        BillingPayment, BillingRefund, Charge, PaymentLink, PaymentRefunded, PaymentStatus, Refund,
        RefundStatus, RefundSucceeded, Unsupported,
    };
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = app_with(
        Billing::new()
            .gateway(PayHub {
                calls: calls.clone(),
            })
            .gateway(Manual),
    )
    .await;
    app.fake_events();
    let user = user(&app, "Mia").await;
    let state = app.state();

    let customer = Billing::of(state, &user);
    let charge = customer.charge(
        Charge::new(Money::usd(4_500))
            .reference("job", 42)
            .account("acme"),
    );
    assert_send(&charge);
    let payment = charge.await.unwrap();
    assert_eq!(payment.gateway, "payhub");
    assert_eq!(payment.account.as_deref(), Some("acme"));
    assert_eq!(
        payment.gateway_id.as_deref(),
        Some(&*format!("pay_{}", payment.id))
    );
    assert_eq!(
        payment.url.as_deref(),
        Some(&*format!("https://pay.example/{}", payment.id))
    );
    assert_eq!(payment.status, PaymentStatus::Pending);
    assert_eq!(payment.billable_id, Some(user.id));
    assert_eq!(payment.reference_id.as_deref(), Some("42"));
    let seen = calls.lock().unwrap()[0].clone();
    assert!(
        seen.contains("4500") && seen.contains("Some(\"acme\")"),
        "{seen}"
    );
    assert!(
        seen.contains(&format!("renox_payment\", \"{}", payment.id)),
        "{seen}"
    );
    let found = BillingPayment::for_reference(&state.db, "job", "42")
        .await
        .unwrap();
    assert_eq!(found.len(), 1);

    // A link has no owner.
    let link = Billing::payment_link(
        state,
        PaymentLink::new(Money::usd(900)).reference("invoice", "7"),
    )
    .await
    .unwrap();
    assert!(link.billable_id.is_none() && link.url.is_some());

    // Unsupported leaves nothing behind.
    let before = BillingPayment::query().count(&state.db).await.unwrap();
    let err = Billing::of(state, &user)
        .charge(Charge::new(Money::usd(100)).via("manual"))
        .await
        .unwrap_err();
    assert_eq!(Unsupported::of(&err).unwrap().operation, "charge");
    let err = Billing::payment_link(state, PaymentLink::new(Money::usd(100)).via("manual"))
        .await
        .unwrap_err();
    assert_eq!(Unsupported::of(&err).unwrap().operation, "payment_link");
    assert_eq!(
        BillingPayment::query().count(&state.db).await.unwrap(),
        before
    );
    assert!(
        Billing::of(state, &user)
            .charge(Charge::new(Money::usd(0)))
            .await
            .is_err()
    );

    // Refunds need a paid payment.
    let refund = |minor, reason: &str| Refund::new(Money::usd(minor)).reason(reason);
    assert!(matches!(
        Billing::refund(state, payment.id, refund(100, "x")).await,
        Err(Error::BadRequest(_))
    ));
    let status = Billing::payment_status(state, payment.id).await.unwrap();
    assert_eq!(status.status, PaymentStatus::Succeeded);
    let mut paid = payment.clone();
    paid.status = PaymentStatus::Succeeded;
    paid.save(&state.db).await.unwrap();

    // Pending refunds count against what is left; over-refunding never
    // reaches the gateway.
    let calls_before = calls.lock().unwrap().len();
    let pending = Billing::refund(state, payment.id, refund(3_000, "late"))
        .await
        .unwrap();
    assert_eq!(pending.status, RefundStatus::Pending);
    assert_eq!(pending.gateway_id.as_deref(), Some("re_2"));
    assert!(matches!(
        Billing::refund(state, payment.id, refund(1_501, "now")).await,
        Err(Error::BadRequest(_))
    ));
    let euro = Money::new(1, renox::money::Currency::parse("EUR").unwrap());
    assert!(matches!(
        Billing::refund(state, payment.id, Refund::new(euro)).await,
        Err(Error::BadRequest(_))
    ));
    assert_eq!(calls.lock().unwrap().len(), calls_before + 1);

    // A refund done at once moves the payment and is announced.
    let done = Billing::refund(state, payment.id, refund(1_000, "now"))
        .await
        .unwrap();
    assert_eq!(done.status, RefundStatus::Succeeded);
    let stored = BillingPayment::find(&state.db, payment.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (stored.refunded_amount, stored.status),
        (1_000, PaymentStatus::PartiallyRefunded)
    );
    app.assert_emitted::<RefundSucceeded>(|e| e.amount == 1_000);
    app.assert_emitted::<PaymentRefunded>(|e| e.payment.id == payment.id);
    assert_eq!(stored.refunds(&state.db).await.unwrap().len(), 2);

    // A gateway without refunds leaves no refund row.
    let rows = BillingRefund::query().count(&state.db).await.unwrap();
    let mut other = BillingPayment::new(
        "manual",
        renox_billing::PaymentKind::Charge,
        Money::usd(500),
    );
    other.status = PaymentStatus::Succeeded;
    let other = BillingPayment::create(&state.db, other).await.unwrap();
    let err = Billing::refund(state, other.id, refund(100, "x"))
        .await
        .unwrap_err();
    assert_eq!(Unsupported::of(&err).unwrap().operation, "refund");
    assert_eq!(BillingRefund::query().count(&state.db).await.unwrap(), rows);
}

/// Payment, refund and dispute notices are applied to the rows once, a late
/// success is applied and a late failure is not, and each change carries the
/// reference in its event.
#[renox::test]
async fn payment_webhooks_are_applied_once_with_the_late_success_rule() {
    use renox::money::Money;
    use renox_billing::{
        BillingPayment, Charge, DisputeLost, DisputeOpened, DisputeWon, PaymentFailed,
        PaymentRefunded, PaymentStatus, PaymentSucceeded, Refund, RefundFailed, RefundSucceeded,
    };
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = app_with(Billing::new().gateway(PayHub { calls })).await;
    app.fake_events();
    let user = user(&app, "Mia").await;
    let state = app.state();
    let make = |reference: i32| {
        let user = &user;
        async move {
            Billing::of(state, user)
                .charge(Charge::new(Money::usd(4_500)).reference("job", reference))
                .await
                .unwrap()
        }
    };
    let hook = |body: renox::serde_json::Value| {
        let app = &app;
        async move {
            app.post_body(
                "/billing/webhooks/payhub",
                "application/json",
                body.to_string(),
            )
            .await;
            app.run_jobs().await;
        }
    };
    let reload =
        |id: i64| async move { BillingPayment::find(&state.db, id).await.unwrap().unwrap() };

    // Expired, then paid late: the success is applied and evented.
    let a = make(1).await;
    let pay = a.gateway_id.clone().unwrap();
    hook(json!({"id": "e1", "kind": "payment", "pay": pay, "status": "expired", "at": 100})).await;
    assert_eq!(reload(a.id).await.status, PaymentStatus::Expired);
    hook(json!({"id": "e2", "kind": "payment", "pay": pay, "status": "succeeded", "at": 50})).await;
    assert_eq!(reload(a.id).await.status, PaymentStatus::Succeeded);
    app.assert_emitted::<PaymentSucceeded>(|e| {
        e.reference_id.as_deref() == Some("1") && e.reference_type.as_deref() == Some("job")
    });
    // A failure after the success is ignored, and a redelivery fires nothing.
    hook(json!({"id": "e3", "kind": "payment", "pay": pay, "status": "failed", "at": 200})).await;
    hook(json!({"id": "e2", "kind": "payment", "pay": pay, "status": "succeeded", "at": 50})).await;
    assert_eq!(reload(a.id).await.status, PaymentStatus::Succeeded);

    // Refund: pending, failed with a reason, then done after all (a refund's
    // success is never refused), once; a later failure is ignored.
    let refund = Billing::refund(state, a.id, Refund::new(Money::usd(1_000)))
        .await
        .unwrap();
    let rid = refund.gateway_id.clone().unwrap();
    hook(json!({"id": "e4", "kind": "refund", "pay": pay, "refund": rid, "status": "failed", "reason": "closed"})).await;
    app.assert_emitted::<RefundFailed>(|e| e.reason.as_deref() == Some("closed"));
    assert_eq!(reload(a.id).await.refunded_amount, 0);
    hook(json!({"id": "e5", "kind": "refund", "pay": pay, "refund": rid, "status": "succeeded"}))
        .await;
    let stored = reload(a.id).await;
    assert_eq!(
        (stored.refunded_amount, stored.status),
        (1_000, PaymentStatus::PartiallyRefunded)
    );
    hook(json!({"id": "e6", "kind": "refund", "pay": pay, "refund": rid, "status": "succeeded"}))
        .await;
    hook(json!({"id": "e7", "kind": "refund", "pay": pay, "refund": rid, "status": "failed"}))
        .await;
    assert_eq!(reload(a.id).await.refunded_amount, 1_000);
    app.assert_emitted::<RefundSucceeded>(|e| e.amount == 1_000);
    app.assert_emitted::<PaymentRefunded>(|e| e.payment.id == a.id);

    // Disputes: opened, won (back to partially refunded), opened, lost.
    hook(json!({"id": "e8", "kind": "dispute", "pay": pay, "status": "opened"})).await;
    let stored = reload(a.id).await;
    assert_eq!(stored.status, PaymentStatus::Disputed);
    assert!(stored.evidence_due_by.is_some());
    app.assert_emitted::<DisputeOpened>(|e| e.payment.id == a.id && e.evidence_due_by.is_some());
    hook(json!({"id": "e9", "kind": "dispute", "pay": pay, "status": "won"})).await;
    assert_eq!(reload(a.id).await.status, PaymentStatus::PartiallyRefunded);
    app.assert_emitted::<DisputeWon>(|e| e.payment.id == a.id);
    hook(json!({"id": "e10", "kind": "dispute", "pay": pay, "status": "opened"})).await;
    hook(json!({"id": "e11", "kind": "dispute", "pay": pay, "status": "lost"})).await;
    let stored = reload(a.id).await;
    assert_eq!(
        (stored.status, stored.refundable().minor),
        (PaymentStatus::Refunded, 0)
    );
    app.assert_emitted::<DisputeLost>(|e| e.payment.id == a.id);

    // A failed payment that is later asked about is paid: the status check
    // applies it like a webhook.
    let b = make(2).await;
    let pay_b = b.gateway_id.clone().unwrap();
    hook(json!({"id": "e12", "kind": "payment", "pay": pay_b, "status": "failed", "at": 10})).await;
    assert_eq!(reload(b.id).await.status, PaymentStatus::Failed);
    app.assert_emitted::<PaymentFailed>(|e| e.reference_id.as_deref() == Some("2"));
    Billing::payment_status(state, b.id).await.unwrap();
    assert_eq!(reload(b.id).await.status, PaymentStatus::Succeeded);
}

/// A Stripe fixture (written from Stripe's public API docs).
fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/stripe/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    renox::serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn stripe_charge() -> renox_billing::Charge {
    renox_billing::Charge::new(renox::money::Money::usd(4_500))
        .reference("job", 42)
        .description("Brake service")
        .success_url("https://shop.test/paid")
        .cancel_url("https://shop.test/cancelled")
}

/// A Stripe charge and a Payment Link send the amount, the currency and the
/// reference, and a paid, refunded and disputed charge is followed through
/// status checks and webhooks.
#[renox::test]
async fn stripe_charges_links_refunds_and_disputes_against_fixtures() {
    use renox::money::Money;
    use renox_billing::{
        BillingPayment, BillingRefund, PaymentLink, PaymentStatus, Refund, RefundFailed,
        RefundStatus, RefundSucceeded,
    };
    let app = App::new()
        .module(Auth::new().account())
        .module(Billing::new().gateway(Stripe::new("sk_test_fake", "whsec_test")));
    let app = TestApp::with_config(app, |_| {}).await;
    app.fake_events();
    let ana = user(&app, "Ana").await;
    let state = app.state();
    let http = app.fake_http();
    http.on(
        &format!("POST {STRIPE}/checkout/sessions"),
        FakeResponse::json(200, fixture("checkout_session")),
    );
    http.on(
        &format!("POST {STRIPE}/products"),
        FakeResponse::json(200, fixture("product")),
    );
    http.on(
        &format!("POST {STRIPE}/payment_links"),
        FakeResponse::json(200, fixture("payment_link")),
    );

    // A charge: Checkout in payment mode with the amount and the reference.
    let payment = Billing::of(state, &ana)
        .charge(stripe_charge())
        .await
        .unwrap();
    assert_eq!(payment.gateway_id.as_deref(), Some("cs_test_a1B2c3"));
    assert_eq!(
        payment.url.as_deref(),
        Some("https://checkout.stripe.com/c/pay/cs_test_a1B2c3")
    );
    let sent = http.sent();
    assert_eq!(sent[0].header("authorization"), Some("Bearer sk_test_fake"));
    assert_eq!(sent[0].url, format!("{STRIPE}/checkout/sessions"));
    let body = form(&sent[0].body);
    for (name, value) in [
        ("mode", "payment".to_owned()),
        ("line_items[0][price_data][currency]", "usd".into()),
        ("line_items[0][price_data][unit_amount]", "4500".into()),
        (
            "line_items[0][price_data][product_data][name]",
            "Brake service".into(),
        ),
        ("client_reference_id", "42".into()),
        ("metadata[renox_payment]", payment.id.to_string()),
        ("metadata[renox_reference_type]", "job".into()),
        ("metadata[renox_reference_id]", "42".into()),
        (
            "payment_intent_data[metadata][renox_reference_id]",
            "42".into(),
        ),
        ("customer_email", "ana@example.com".into()),
        ("success_url", "https://shop.test/paid".into()),
    ] {
        assert_eq!(
            param(&body, name).as_deref(),
            Some(value.as_str()),
            "{name}"
        );
    }

    // A link: a product, then the link, one payment per link.
    let link = Billing::payment_link(
        state,
        PaymentLink::new(Money::usd(900))
            .reference("invoice", "7")
            .description("Invoice 7"),
    )
    .await
    .unwrap();
    assert_eq!(link.gateway_id.as_deref(), Some("plink_1Abc"));
    assert_eq!(
        link.url.as_deref(),
        Some("https://buy.stripe.com/test_9AQ1Abc")
    );
    let sent = http.sent();
    assert_eq!(
        param(&form(&sent[1].body), "name").as_deref(),
        Some("Invoice 7")
    );
    assert_eq!(sent[2].url, format!("{STRIPE}/payment_links"));
    let body = form(&sent[2].body);
    for (name, value) in [
        ("line_items[0][price_data][product]", "prod_Q1"),
        ("line_items[0][price_data][unit_amount]", "900"),
        ("line_items[0][price_data][currency]", "usd"),
        ("restrictions[completed_sessions][limit]", "1"),
        ("metadata[renox_reference_id]", "7"),
    ] {
        assert_eq!(param(&body, name).as_deref(), Some(value), "{name}");
    }

    // The session is paid (by webhook), then refunded in part.
    stripe_webhook(
        &app,
        "evt_1",
        "checkout.session.completed",
        now(&app).await,
        fixture("checkout_session_paid"),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let reload =
        |id: i64| async move { BillingPayment::find(&state.db, id).await.unwrap().unwrap() };
    assert_eq!(reload(payment.id).await.status, PaymentStatus::Succeeded);
    http.on(
        &format!("GET {STRIPE}/checkout/sessions/cs_test_a1B2c3"),
        FakeResponse::json(200, fixture("checkout_session_paid")),
    );
    http.on(
        &format!("POST {STRIPE}/refunds"),
        FakeResponse::json(200, fixture("refund_pending")),
    );
    let refund = Billing::refund(
        state,
        payment.id,
        Refund::new(Money::usd(1_000)).reason("duplicate"),
    )
    .await
    .unwrap();
    assert_eq!(
        (refund.status, refund.gateway_id.as_deref()),
        (RefundStatus::Pending, Some("re_3Abc"))
    );
    let sent = http.sent();
    let made = sent.last().unwrap();
    assert_eq!(made.url, format!("{STRIPE}/refunds"));
    let body = form(&made.body);
    assert_eq!(param(&body, "payment_intent").as_deref(), Some("pi_3Abc"));
    assert_eq!(param(&body, "amount").as_deref(), Some("1000"));
    assert_eq!(
        param(&body, "metadata[renox_reason]").as_deref(),
        Some("duplicate")
    );

    // The refund fails, then Stripe's `charge.refunded` says it went through.
    stripe_webhook(
        &app,
        "evt_2",
        "charge.refund.updated",
        now(&app).await,
        fixture("event_refund_updated"),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<RefundFailed>(|e| e.reason.as_deref() == Some("expired_or_canceled_card"));
    stripe_webhook(
        &app,
        "evt_3",
        "charge.refunded",
        now(&app).await,
        fixture("event_charge_refunded"),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<RefundSucceeded>(|e| e.amount == 1_000);
    let stored = reload(payment.id).await;
    assert_eq!(
        (stored.refunded_amount, stored.status),
        (1_000, PaymentStatus::PartiallyRefunded)
    );
    assert_eq!(BillingRefund::query().count(&state.db).await.unwrap(), 1);

    // A status check on the link reads its newest session; the charge's
    // refunds are reported; an expired session is reported expired.
    http.on(
        &format!("GET {STRIPE}/checkout/sessions?payment_link=plink_1Abc&limit=1"),
        FakeResponse::json(200, fixture("session_list_paid")),
    );
    http.on(
        &format!("GET {STRIPE}/refunds?payment_intent=pi_3Link"),
        FakeResponse::json(200, json!({ "object": "list", "data": [] })),
    );
    let reported = Billing::payment_status(state, link.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Succeeded);
    assert_eq!(reload(link.id).await.status, PaymentStatus::Succeeded);
    http.on(
        &format!("GET {STRIPE}/refunds?payment_intent=pi_3Abc"),
        FakeResponse::json(200, fixture("refund_list")),
    );
    // (Answers for one URL are given in turn: the paid session of the
    // refund, then expired.)
    http.on(
        &format!("GET {STRIPE}/checkout/sessions/cs_test_a1B2c3"),
        FakeResponse::json(200, fixture("checkout_session_expired")),
    );
    let reported = Billing::payment_status(state, payment.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Succeeded);
    assert_eq!(reported.refunds.len(), 1);
    let reported = Billing::payment_status(state, payment.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Expired);
    // An expiry after a success changes nothing.
    assert_eq!(
        reload(payment.id).await.status,
        PaymentStatus::PartiallyRefunded
    );

    // Accepting a dispute on the link's payment closes it at Stripe.
    http.on(
        &format!("GET {STRIPE}/disputes?payment_intent=pi_3Link&limit=1"),
        FakeResponse::json(200, fixture("dispute_list")),
    );
    http.on(
        &format!("POST {STRIPE}/disputes/dp_1Abc/close"),
        FakeResponse::json(200, fixture("dispute_closed")),
    );
    let gateway = Stripe::new("sk_test_fake", "whsec_test");
    renox_billing::Gateway::accept_dispute(&gateway, state, &reload(link.id).await)
        .await
        .unwrap();
    http.assert_sent(|r| r.method == "POST" && r.url == format!("{STRIPE}/disputes/dp_1Abc/close"));
}

/// Stripe's one-off webhooks are read into notices; sessions of a Payment
/// Link, delayed methods and subscriptions are told apart.
#[test]
fn stripe_parses_one_off_webhooks() {
    use renox_billing::payment::{DisputeState, PaymentStatus, RefundStatus};
    use renox_billing::{Gateway, Notice};
    let stripe = Stripe::new("sk_test_fake", "whsec_test");
    let parse = |kind: &str, object: Value| {
        let body =
            json!({ "id": "evt", "type": kind, "created": 77, "data": { "object": object } });
        stripe.parse_webhook(body.to_string().as_bytes()).unwrap()
    };
    let one_off = |notices: Vec<Notice>| match notices.as_slice() {
        [Notice::OneOff(update)] => (update.gateway_id.clone(), update.status, update.at),
        other => panic!("{other:?}"),
    };
    // The session's own id, or the Payment Link's.
    let paid = parse(
        "checkout.session.completed",
        fixture("checkout_session_paid"),
    );
    assert_eq!(
        one_off(paid),
        ("cs_test_a1B2c3".into(), PaymentStatus::Succeeded, Some(77))
    );
    let paid = parse(
        "checkout.session.completed",
        fixture("event_link_session_completed"),
    );
    assert_eq!(one_off(paid).0, "plink_1Abc");
    let expired = parse(
        "checkout.session.expired",
        fixture("checkout_session_expired"),
    );
    assert_eq!(one_off(expired).1, PaymentStatus::Expired);
    // A link's lapsed session, a delayed method still unpaid and a
    // subscription's checkout say nothing final about a one-off payment.
    let mut lapsed = fixture("checkout_session_expired");
    lapsed["payment_link"] = json!("plink_1Abc");
    assert!(parse("checkout.session.expired", lapsed).is_empty());
    let mut delayed = fixture("checkout_session_paid");
    delayed["payment_status"] = json!("unpaid");
    assert!(parse("checkout.session.completed", delayed.clone()).is_empty());
    assert_eq!(
        one_off(parse(
            "checkout.session.async_payment_failed",
            delayed.clone()
        ))
        .1,
        PaymentStatus::Failed
    );
    delayed["mode"] = json!("subscription");
    assert!(parse("checkout.session.completed", delayed).is_empty());

    match parse("charge.refunded", fixture("event_charge_refunded")).as_slice() {
        [Notice::Refund(update)] => {
            assert_eq!(update.gateway_id, "re_3Abc");
            assert_eq!(
                (update.status, update.amount),
                (RefundStatus::Succeeded, Some(1_000))
            );
            assert_eq!(update.payment_gateway_id.as_deref(), Some("pi_3Abc"));
        }
        other => panic!("{other:?}"),
    }
    match parse("charge.refund.updated", fixture("event_refund_updated")).as_slice() {
        [Notice::Refund(update)] => {
            assert_eq!(update.status, RefundStatus::Failed);
            assert_eq!(update.reason.as_deref(), Some("expired_or_canceled_card"));
        }
        other => panic!("{other:?}"),
    }
    match parse("charge.dispute.created", fixture("event_dispute_created")).as_slice() {
        [Notice::Dispute(update)] => {
            assert_eq!(update.state, DisputeState::Opened);
            assert_eq!(update.payment_gateway_id, "pi_3Abc");
            assert_eq!(
                update.evidence_due_by.map(|d| d.timestamp()),
                Some(1_893_456_000)
            );
        }
        other => panic!("{other:?}"),
    }
    match parse("charge.dispute.closed", fixture("event_dispute_won")).as_slice() {
        [Notice::Dispute(update)] => assert_eq!(update.state, DisputeState::Won),
        other => panic!("{other:?}"),
    }
    let mut closed = fixture("event_dispute_won");
    closed["status"] = json!("warning_closed");
    assert!(parse("charge.dispute.closed", closed).is_empty());
}

/// Stripe's disputes and dashboard-made refunds name only the PaymentIntent:
/// the paid webhook stores it on the row, and they reach that row by it.
#[renox::test]
async fn stripe_disputes_and_dashboard_refunds_find_the_row_by_its_payment_intent() {
    use renox_billing::{
        BillingPayment, DisputeLost, DisputeOpened, DisputeWon, PaymentStatus, RefundSucceeded,
    };
    let app = App::new()
        .module(Auth::new().account())
        .module(Billing::new().gateway(Stripe::new("sk_test_fake", "whsec_test")));
    let app = TestApp::with_config(app, |_| {}).await;
    app.fake_events();
    let ana = user(&app, "Ana").await;
    let state = app.state();
    app.fake_http().on(
        &format!("POST {STRIPE}/checkout/sessions"),
        FakeResponse::json(200, fixture("checkout_session")),
    );
    let payment = Billing::of(state, &ana)
        .charge(stripe_charge())
        .await
        .unwrap();
    let reload = || async {
        BillingPayment::find(&state.db, payment.id)
            .await
            .unwrap()
            .unwrap()
    };
    assert_eq!(reload().await.gateway_intent, None);

    // Before it is paid the intent is unknown, so a dispute finds nothing.
    let mut dispute = fixture("event_dispute_created");
    stripe_webhook(
        &app,
        "evt_d0",
        "charge.dispute.created",
        100,
        dispute.clone(),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    assert_eq!(reload().await.status, PaymentStatus::Pending);

    // The paid webhook stores the PaymentIntent.
    stripe_webhook(
        &app,
        "evt_p",
        "checkout.session.completed",
        200,
        fixture("checkout_session_paid"),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    let paid = reload().await;
    assert_eq!(paid.status, PaymentStatus::Succeeded);
    assert_eq!(paid.gateway_intent.as_deref(), Some("pi_3Abc"));

    // A refund made in Stripe's dashboard is new to the app and names only
    // the intent.
    let mut dashboard = fixture("event_refund_updated");
    dashboard["id"] = json!("re_dashboard");
    dashboard["status"] = json!("succeeded");
    dashboard["amount"] = json!(500);
    stripe_webhook(&app, "evt_r", "charge.refund.updated", 300, dashboard)
        .await
        .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<RefundSucceeded>(|e| e.payment.id == payment.id && e.amount == 500);
    let refunded = reload().await;
    assert_eq!(
        (refunded.refunded_amount, refunded.status),
        (500, PaymentStatus::PartiallyRefunded)
    );

    // A dispute opens, is won, opens again and is lost.
    stripe_webhook(
        &app,
        "evt_d1",
        "charge.dispute.created",
        400,
        dispute.clone(),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<DisputeOpened>(|e| {
        e.payment.id == payment.id
            && e.evidence_due_by.map(|d| d.timestamp()) == Some(1_893_456_000)
    });
    assert_eq!(reload().await.status, PaymentStatus::Disputed);
    stripe_webhook(
        &app,
        "evt_d2",
        "charge.dispute.closed",
        500,
        fixture("event_dispute_won"),
    )
    .await
    .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<DisputeWon>(|e| e.payment.id == payment.id);
    assert_eq!(reload().await.status, PaymentStatus::PartiallyRefunded);
    stripe_webhook(
        &app,
        "evt_d3",
        "charge.dispute.created",
        600,
        dispute.clone(),
    )
    .await
    .assert_ok();
    dispute["status"] = json!("lost");
    stripe_webhook(&app, "evt_d4", "charge.dispute.closed", 700, dispute)
        .await
        .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<DisputeLost>(|e| e.payment.id == payment.id);
    let lost = reload().await;
    assert_eq!(
        (lost.status, lost.refunded_amount),
        (PaymentStatus::Refunded, 4_500)
    );
}

/// A charge on a named account uses that account's key, and a refund of it
/// uses the account stored on the payment; the default account's key is not
/// used for either, and an account without a key is refused.
#[renox::test]
async fn stripe_calls_use_the_account_of_the_payment() {
    use renox::money::Money;
    use renox_billing::{PaymentStatus, Refund};
    let app = App::new()
        .module(Auth::new().account())
        .module(Billing::new().gateway(Stripe::from_config()));
    let app = TestApp::with_config(app, |config| {
        for (name, value) in [
            ("STRIPE_SECRET", "sk_test_fake_default"),
            ("STRIPE_SECRET_ACME_LLC", "sk_test_fake_acme"),
            ("STRIPE_WEBHOOK_SECRET", "whsec_test"),
        ] {
            config.vars.insert(name.into(), value.into());
        }
    })
    .await;
    let ana = user(&app, "Ana").await;
    let state = app.state();
    let http = app.fake_http();
    http.on(
        &format!("POST {STRIPE}/checkout/sessions"),
        FakeResponse::json(200, fixture("checkout_session")),
    );
    http.on(
        &format!("GET {STRIPE}/checkout/sessions/cs_test_a1B2c3"),
        FakeResponse::json(200, fixture("checkout_session_paid")),
    );
    http.on(
        &format!("POST {STRIPE}/refunds"),
        FakeResponse::json(200, fixture("refund_pending")),
    );
    let mut payment = Billing::of(state, &ana)
        .charge(stripe_charge().account("acme-llc"))
        .await
        .unwrap();
    assert_eq!(payment.account.as_deref(), Some("acme-llc"));
    payment.status = PaymentStatus::Succeeded;
    payment.save(&state.db).await.unwrap();
    Billing::refund(state, payment.id, Refund::new(Money::usd(100)))
        .await
        .unwrap();
    let sent = http.sent();
    assert_eq!(sent.len(), 3, "the session, its read, the refund");
    for request in sent {
        assert_eq!(
            request.header("authorization"),
            Some("Bearer sk_test_fake_acme"),
            "{}",
            request.url
        );
    }
    // An account without a key is refused before any request is sent.
    let err = Billing::of(state, &ana)
        .charge(stripe_charge().account("nobody"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Stripe's secret key"), "{err}");
    assert_eq!(http.sent().len(), 3);
}

/// A Xendit fixture (written from Xendit's public API docs).
fn xendit_fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/xendit/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    renox::serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// An invoice callback: the invoice itself, with the callback token.
async fn xendit_invoice_callback(app: &TestApp, invoice: Value) -> TestResponse {
    app.request()
        .header("x-callback-token", "callback-token")
        .post_body(
            "/billing/webhooks/xendit",
            "application/json",
            invoice.to_string(),
        )
        .await
}

/// A Xendit refund event from a fixture.
async fn xendit_refund_event(app: &TestApp, fixture: &str) -> TestResponse {
    let event = xendit_fixture(fixture);
    xendit_webhook(
        app,
        event["event"].as_str().unwrap(),
        event["created"].as_str().unwrap(),
        event["data"].clone(),
    )
    .await
}

/// A Xendit charge and link are invoices that send the amount in whole
/// units, the currency and the reference; a paid, refunded and expired
/// payment is followed through status checks and callbacks.
#[renox::test]
async fn xendit_charges_links_refunds_and_reads_callbacks_against_fixtures() {
    use renox::money::Money;
    use renox_billing::{
        BillingPayment, BillingRefund, PaymentLink, PaymentLinkExpired, PaymentStatus, Refund,
        RefundFailed, RefundStatus, RefundSucceeded,
    };
    const BASIC: &str = "Basic eG5kX2RldmVsb3BtZW50X2Zha2U6";
    let app = App::new()
        .module(Auth::new().account())
        .module(Billing::new().gateway(renox_billing::Xendit::new(
            "xnd_development_fake",
            "callback-token",
        )));
    let app = TestApp::with_config(app, |_| {}).await;
    app.fake_events();
    let ana = user(&app, "Ana").await;
    let state = app.state();
    let http = app.fake_http();
    http.on(
        &format!("POST {XENDIT}/v2/invoices"),
        FakeResponse::json(200, xendit_fixture("invoice_created")),
    );
    // (Answers for one URL are given in turn: the charge's, then the link's.)
    http.on(
        &format!("POST {XENDIT}/v2/invoices"),
        FakeResponse::json(200, xendit_fixture("invoice_link_created")),
    );

    // A charge: an invoice of 45 dollars (4500 cents) with the reference.
    let payment = Billing::of(state, &ana)
        .charge(stripe_charge())
        .await
        .unwrap();
    assert_eq!(
        payment.gateway_id.as_deref(),
        Some("579c8d61f23fa4ca35e52da4")
    );
    assert_eq!(
        payment.url.as_deref(),
        Some("https://checkout.xendit.co/web/579c8d61f23fa4ca35e52da4")
    );
    let sent = http.sent();
    assert_eq!(sent[0].url, format!("{XENDIT}/v2/invoices"));
    assert_eq!(sent[0].header("authorization"), Some(BASIC));
    let body = sent[0].json();
    assert_eq!(body["external_id"], format!("job-42-{}", payment.id));
    assert_eq!(body["amount"], 45);
    assert_eq!(body["currency"], "USD");
    assert_eq!(body["description"], "Brake service");
    assert_eq!(body["payer_email"], "ana@example.com");
    assert_eq!(body["success_redirect_url"], "https://shop.test/paid");
    assert_eq!(body["failure_redirect_url"], "https://shop.test/cancelled");
    assert_eq!(body["metadata"]["renox_payment"], payment.id.to_string());
    assert_eq!(body["metadata"]["renox_reference_id"], "42");

    // A link in rupiah (no minor unit): the amount is sent as it is.
    let link = Billing::payment_link(
        state,
        PaymentLink::new(Money::new(99_000, renox::money::Currency::IDR))
            .reference("invoice", "7")
            .description("Invoice 7"),
    )
    .await
    .unwrap();
    assert_eq!(link.gateway_id.as_deref(), Some("579c8d61f23fa4ca35e52db5"));
    let body = http.sent()[1].json();
    assert_eq!(body["external_id"], format!("invoice-7-{}", link.id));
    assert_eq!(body["amount"], 99_000);
    assert_eq!(body["currency"], "IDR");
    assert!(body.get("payer_email").is_none());

    // The invoice is paid (callback), then refunded in part.
    xendit_invoice_callback(&app, xendit_fixture("invoice_paid"))
        .await
        .assert_ok();
    app.run_jobs().await;
    let reload =
        |id: i64| async move { BillingPayment::find(&state.db, id).await.unwrap().unwrap() };
    assert_eq!(reload(payment.id).await.status, PaymentStatus::Succeeded);
    app.assert_emitted::<PaymentSucceeded>(|e| {
        e.gateway == "xendit" && e.reference_id.as_deref() == Some("42")
    });
    // A repeated delivery changes nothing.
    xendit_invoice_callback(&app, xendit_fixture("invoice_paid"))
        .await
        .assert_ok();
    app.run_jobs().await;
    assert_eq!(reload(payment.id).await.status, PaymentStatus::Succeeded);

    http.on(
        &format!("POST {XENDIT}/refunds"),
        FakeResponse::json(200, xendit_fixture("refund_pending")),
    );
    let refund = Billing::refund(
        state,
        payment.id,
        Refund::new(Money::usd(1_000)).reason("duplicate"),
    )
    .await
    .unwrap();
    assert_eq!(
        (refund.status, refund.gateway_id.as_deref()),
        (RefundStatus::Pending, Some("rfd_8d3f1a"))
    );
    let sent = http.sent();
    let made = sent.last().unwrap();
    assert_eq!(made.url, format!("{XENDIT}/refunds"));
    assert_eq!(made.header("authorization"), Some(BASIC));
    let body = made.json();
    assert_eq!(body["invoice_id"], "579c8d61f23fa4ca35e52da4");
    assert_eq!(body["amount"], 10);
    assert_eq!(body["currency"], "USD");
    assert_eq!(body["reason"], "DUPLICATE");
    assert_eq!(body["metadata"]["renox_payment"], payment.id.to_string());

    // The refund fails, then Xendit says it went through.
    xendit_refund_event(&app, "event_refund_failed")
        .await
        .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<RefundFailed>(|e| e.reason.as_deref() == Some("INSUFFICIENT_BALANCE"));
    xendit_refund_event(&app, "event_refund_succeeded")
        .await
        .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<RefundSucceeded>(|e| e.amount == 1_000);
    let stored = reload(payment.id).await;
    assert_eq!(
        (stored.refunded_amount, stored.status),
        (1_000, PaymentStatus::PartiallyRefunded)
    );
    assert_eq!(BillingRefund::query().count(&state.db).await.unwrap(), 1);

    // A wrong callback token is refused.
    app.request()
        .header("x-callback-token", "wrong")
        .post_body(
            "/billing/webhooks/xendit",
            "application/json",
            xendit_fixture("invoice_paid").to_string(),
        )
        .await
        .assert_status(401);

    // Status: the invoice and its refunds are read; an expired invoice is
    // reported expired.
    http.on(
        &format!("GET {XENDIT}/v2/invoices/579c8d61f23fa4ca35e52da4"),
        FakeResponse::json(200, xendit_fixture("invoice_paid")),
    );
    http.on(
        &format!("GET {XENDIT}/refunds?invoice_id=579c8d61f23fa4ca35e52da4"),
        FakeResponse::json(200, xendit_fixture("refund_list")),
    );
    let reported = Billing::payment_status(state, payment.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Succeeded);
    assert_eq!(reported.refunds.len(), 1);
    assert_eq!(reported.refunds[0].amount, Some(1_000));
    let sent = http.sent();
    let status = sent
        .iter()
        .find(|r| r.method == "GET" && r.url.contains("/v2/invoices/"))
        .unwrap();
    assert_eq!(status.header("authorization"), Some(BASIC));

    http.on(
        &format!("GET {XENDIT}/v2/invoices/579c8d61f23fa4ca35e52db5"),
        FakeResponse::json(200, xendit_fixture("invoice_expired")),
    );
    http.on(
        &format!("GET {XENDIT}/refunds?invoice_id=579c8d61f23fa4ca35e52db5"),
        FakeResponse::json(200, json!({ "data": [] })),
    );
    let reported = Billing::payment_status(state, link.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Expired);
    assert_eq!(reload(link.id).await.status, PaymentStatus::Expired);
    app.assert_emitted::<PaymentLinkExpired>(|e| e.payment.id == link.id);
}

/// The scheduled sweep expires pending links that are past due and unpaid,
/// asks the gateway first (a paid link becomes succeeded, never expired),
/// leaves links that are not due, and announces `PaymentLinkExpired` once.
#[renox::test]
async fn expire_links_sweep_expires_only_unpaid_past_due_links() {
    use renox::money::Money;
    use renox_billing::{BillingPayment, PaymentLink, PaymentLinkExpired, PaymentStatus};
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = app_with(Billing::new().gateway(PayHub { calls })).await;
    app.fake_events();
    let state = app.state();
    let hour = renox::db::now() + renox::chrono::Duration::hours(1);
    let week = renox::db::now() + renox::chrono::Duration::days(7);
    let link = |reference: &'static str, at| {
        Billing::payment_link(
            state,
            PaymentLink::new(Money::usd(900))
                .reference(reference, 1)
                .expires_at(at),
        )
    };
    let unpaid = link("unpaid", hour).await.unwrap();
    let paid = link("invoice", hour).await.unwrap();
    let later = link("unpaid", week).await.unwrap();
    let status = |id| async move {
        BillingPayment::find(&state.db, id)
            .await
            .unwrap()
            .unwrap()
            .status
    };

    // Nothing is due yet.
    app.run_scheduled("billing:expire-links").await.unwrap();
    assert_eq!(status(unpaid.id).await, PaymentStatus::Pending);

    app.travel(Duration::from_secs(2 * 3600));
    app.run_scheduled("billing:expire-links").await.unwrap();
    assert_eq!(status(unpaid.id).await, PaymentStatus::Expired);
    assert_eq!(status(paid.id).await, PaymentStatus::Succeeded);
    assert_eq!(status(later.id).await, PaymentStatus::Pending);
    assert_eq!(app.emitted::<PaymentLinkExpired>().len(), 1);
    app.assert_emitted::<PaymentLinkExpired>(|e| e.payment.id == unpaid.id);

    // A second sweep changes nothing and announces nothing.
    assert_eq!(
        app.at_travelled_time(Billing::expire_links(state))
            .await
            .unwrap(),
        0
    );
    assert_eq!(app.emitted::<PaymentLinkExpired>().len(), 1);
}

/// Xendit has no disputes API here: accepting one is the typed refusal.
#[renox::test]
async fn xendit_answers_unsupported_for_disputes() {
    use renox::money::Money;
    use renox_billing::{BillingPayment, Gateway, PaymentKind, Unsupported};
    let app = app().await;
    let gateway = renox_billing::Xendit::new("xnd_development_fake", "callback-token");
    let payment = BillingPayment::new("xendit", PaymentKind::Charge, Money::usd(100));
    let err = gateway
        .accept_dispute(app.state(), &payment)
        .await
        .unwrap_err();
    let found = Unsupported::of(&err).expect("a typed refusal");
    assert_eq!(
        (found.gateway.as_str(), found.operation.as_str()),
        ("xendit", "accept_dispute")
    );
}

/// Xendit's callbacks are read into notices: an invoice callback is the
/// invoice (paid or expired), refund events carry whole-unit amounts.
#[test]
fn xendit_parses_one_off_callbacks() {
    use renox_billing::payment::{PaymentStatus, RefundStatus};
    use renox_billing::{Gateway, Notice};
    let xendit = renox_billing::Xendit::new("xnd_development_fake", "callback-token");
    let parse = |body: Value| xendit.parse_webhook(body.to_string().as_bytes()).unwrap();
    match parse(xendit_fixture("invoice_paid")).as_slice() {
        [Notice::OneOff(update)] => {
            assert_eq!(update.gateway_id, "579c8d61f23fa4ca35e52da4");
            assert_eq!(update.status, PaymentStatus::Succeeded);
            assert_eq!(update.at, Some(1_792_236_600));
        }
        other => panic!("{other:?}"),
    }
    match parse(xendit_fixture("invoice_expired")).as_slice() {
        [Notice::OneOff(update)] => assert_eq!(update.status, PaymentStatus::Expired),
        other => panic!("{other:?}"),
    }
    // A pending invoice says nothing new.
    assert!(parse(xendit_fixture("invoice_created")).is_empty());
    match parse(xendit_fixture("event_refund_failed")).as_slice() {
        [Notice::Refund(update)] => {
            assert_eq!(update.gateway_id, "rfd_8d3f1a");
            assert_eq!(
                (update.status, update.amount),
                (RefundStatus::Failed, Some(1_000))
            );
            assert_eq!(
                update.payment_gateway_id.as_deref(),
                Some("579c8d61f23fa4ca35e52da4")
            );
            assert_eq!(update.reason.as_deref(), Some("INSUFFICIENT_BALANCE"));
        }
        other => panic!("{other:?}"),
    }
}

/// A charge on a named account uses that account's key, and a refund of it
/// uses the account stored on the payment.
#[renox::test]
async fn xendit_calls_use_the_account_of_the_payment() {
    use renox::money::Money;
    use renox_billing::{PaymentStatus, Refund};
    let app = App::new()
        .module(Auth::new().account())
        .module(
            Billing::new().gateway(renox_billing::Xendit::resolved_accounts(
                |_state, account: &str| {
                    let key = format!("xnd_development_{account}");
                    async move { Ok(Some(key)) }
                },
            )),
        );
    let app = TestApp::with_config(app, |_| {}).await;
    let ana = user(&app, "Ana").await;
    let state = app.state();
    let http = app.fake_http();
    http.on(
        &format!("POST {XENDIT}/v2/invoices"),
        FakeResponse::json(200, xendit_fixture("invoice_created")),
    );
    http.on(
        &format!("POST {XENDIT}/refunds"),
        FakeResponse::json(200, xendit_fixture("refund_pending")),
    );
    let mut payment = Billing::of(state, &ana)
        .charge(stripe_charge().account("acme"))
        .await
        .unwrap();
    assert_eq!(payment.account.as_deref(), Some("acme"));
    payment.status = PaymentStatus::Succeeded;
    payment.save(&state.db).await.unwrap();
    Billing::refund(state, payment.id, Refund::new(Money::usd(100)))
        .await
        .unwrap();
    let sent = http.sent();
    assert_eq!(sent.len(), 2, "the invoice, the refund");
    for request in sent {
        assert_eq!(
            request.header("authorization"),
            Some("Basic eG5kX2RldmVsb3BtZW50X2FjbWU6"),
            "{}",
            request.url
        );
    }
}

const NGENIUS: &str = "https://api-gateway.ngenius-payments.com";
const NGENIUS_ORDER: &str = "6b1a8c3e-4f0d-4c2a-9a55-1d2e3f4a5b6c";
const NGENIUS_LINK_ORDER: &str = "0c7d2e91-8a3b-4d6f-b1e4-5a9c8d7e6f01";

/// An N-Genius fixture (written from Network International's public API
/// docs).
fn ngenius_fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/ngenius/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    renox::serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// An N-Genius webhook event from a fixture, with the shared secret.
async fn ngenius_event(app: &TestApp, fixture: &str, secret: &str) -> TestResponse {
    app.request()
        .header("x-webhook-secret", secret)
        .post_body(
            "/billing/webhooks/ngenius",
            "application/json",
            ngenius_fixture(fixture).to_string(),
        )
        .await
}

/// A charge and a link are orders that send the amount in minor units, the
/// currency, the reference and the outlet with a bearer token traded for
/// the API key; a paid, refunded and declined payment is followed through
/// status checks and webhooks.
#[renox::test]
async fn ngenius_charges_links_refunds_and_reads_webhooks_against_fixtures() {
    use renox::money::{Currency, Money};
    use renox_billing::{
        BillingPayment, BillingRefund, PaymentLink, PaymentStatus, Refund, RefundFailed,
        RefundStatus, RefundSucceeded,
    };
    let app = App::new()
        .module(Auth::new().account())
        .module(Billing::new().gateway(renox_billing::Ngenius::new(
            "ngenius-api-key-fake",
            "outlet-ref-fake",
            "hook-secret",
        )));
    let app = TestApp::with_config(app, |_| {}).await;
    app.fake_events();
    let ana = user(&app, "Ana").await;
    let state = app.state();
    let http = app.fake_http();
    let orders = format!("{NGENIUS}/transactions/outlets/outlet-ref-fake/orders");
    http.on(
        &format!("POST {NGENIUS}/identity/auth/access-token"),
        FakeResponse::json(200, ngenius_fixture("access_token")),
    );
    http.on(
        &format!("POST {orders}"),
        FakeResponse::json(200, ngenius_fixture("order_created")),
    );
    http.on(
        &format!("POST {orders}"),
        FakeResponse::json(200, ngenius_fixture("order_link_created")),
    );

    // A charge: the token first, then an order of 4500 cents of USD.
    let payment = Billing::of(state, &ana)
        .charge(stripe_charge())
        .await
        .unwrap();
    assert_eq!(payment.gateway_id.as_deref(), Some(NGENIUS_ORDER));
    assert_eq!(
        payment.url.as_deref(),
        Some("https://paypage.ngenius-payments.com/?code=fake-code")
    );
    let sent = http.sent();
    assert_eq!(sent[0].url, format!("{NGENIUS}/identity/auth/access-token"));
    assert_eq!(
        sent[0].header("authorization"),
        Some("Basic ngenius-api-key-fake")
    );
    assert_eq!(sent[1].url, orders);
    assert_eq!(
        sent[1].header("authorization"),
        Some("Bearer fake-access-token-1")
    );
    let body = sent[1].json();
    assert_eq!(body["action"], "SALE");
    assert_eq!(body["amount"]["value"], 4_500);
    assert_eq!(body["amount"]["currencyCode"], "USD");
    assert_eq!(
        body["merchantOrderReference"],
        format!("job-42-{}", payment.id)
    );
    assert_eq!(body["emailAddress"], "ana@example.com");
    assert_eq!(
        body["merchantAttributes"]["redirectUrl"],
        "https://shop.test/paid"
    );
    assert_eq!(
        body["merchantAttributes"]["cancelUrl"],
        "https://shop.test/cancelled"
    );

    // A link in dirhams reuses the kept token.
    let link = Billing::payment_link(
        state,
        PaymentLink::new(Money::new(12_000, Currency::parse("AED").unwrap()))
            .reference("invoice", "7"),
    )
    .await
    .unwrap();
    assert_eq!(link.gateway_id.as_deref(), Some(NGENIUS_LINK_ORDER));
    let sent = http.sent();
    assert_eq!(sent.len(), 3, "no second token request");
    let body = sent[2].json();
    assert_eq!(
        body["merchantOrderReference"],
        format!("invoice-7-{}", link.id)
    );
    assert_eq!(body["amount"]["value"], 12_000);
    assert_eq!(body["amount"]["currencyCode"], "AED");
    assert!(body.get("emailAddress").is_none());

    // The order is purchased (webhook); a repeat and a wrong secret change
    // nothing.
    ngenius_event(&app, "event_purchased", "hook-secret")
        .await
        .assert_ok();
    app.run_jobs().await;
    let reload =
        |id: i64| async move { BillingPayment::find(&state.db, id).await.unwrap().unwrap() };
    assert_eq!(reload(payment.id).await.status, PaymentStatus::Succeeded);
    app.assert_emitted::<PaymentSucceeded>(|e| {
        e.gateway == "ngenius" && e.reference_id.as_deref() == Some("42")
    });
    ngenius_event(&app, "event_purchased", "hook-secret")
        .await
        .assert_ok();
    ngenius_event(&app, "event_declined", "wrong")
        .await
        .assert_status(401);
    app.run_jobs().await;
    assert_eq!(reload(link.id).await.status, PaymentStatus::Pending);

    // A refund goes to the refund link of the order's capture.
    let order_url = format!("{orders}/{NGENIUS_ORDER}");
    let refund_url = format!("{order_url}/payments/fake-payment-1/captures/fake-capture-1/refund");
    http.on(
        &format!("GET {order_url}"),
        FakeResponse::json(200, ngenius_fixture("order_purchased")),
    );
    // (Answers are given in turn: the refund reads the first, the status
    // check below the second.)
    http.on(
        &format!("GET {order_url}"),
        FakeResponse::json(200, ngenius_fixture("order_refunded")),
    );
    http.on(
        &format!("POST {refund_url}"),
        FakeResponse::json(201, ngenius_fixture("refund_pending")),
    );
    let refund = Billing::refund(state, payment.id, Refund::new(Money::usd(1_000)))
        .await
        .unwrap();
    assert_eq!(
        (refund.status, refund.gateway_id.as_deref()),
        (RefundStatus::Pending, Some("fake-refund-1"))
    );
    let sent = http.sent();
    let made = sent.last().unwrap();
    assert_eq!(made.method, "POST");
    assert_eq!(made.url, refund_url);
    assert_eq!(
        made.header("authorization"),
        Some("Bearer fake-access-token-1")
    );
    let body = made.json();
    assert_eq!(body["amount"]["value"], 1_000);
    assert_eq!(body["amount"]["currencyCode"], "USD");

    // The refund fails, then N-Genius says it went through.
    ngenius_event(&app, "event_refund_failed", "hook-secret")
        .await
        .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<RefundFailed>(|e| e.amount == 1_000);
    ngenius_event(&app, "event_refunded", "hook-secret")
        .await
        .assert_ok();
    app.run_jobs().await;
    app.assert_emitted::<RefundSucceeded>(|e| e.amount == 1_000);
    let stored = reload(payment.id).await;
    assert_eq!(
        (stored.refunded_amount, stored.status),
        (1_000, PaymentStatus::PartiallyRefunded)
    );
    assert_eq!(BillingRefund::query().count(&state.db).await.unwrap(), 1);

    // Status: the order and its refunds are read.
    let reported = Billing::payment_status(state, payment.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Succeeded);
    assert_eq!(reported.refunds.len(), 1);
    assert_eq!(reported.refunds[0].amount, Some(1_000));
    http.on(
        &format!("GET {orders}/{NGENIUS_LINK_ORDER}"),
        FakeResponse::json(200, ngenius_fixture("order_pending")),
    );
    let reported = Billing::payment_status(state, link.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Pending);

    // A declined payment fails the link.
    ngenius_event(&app, "event_declined", "hook-secret")
        .await
        .assert_ok();
    app.run_jobs().await;
    assert_eq!(reload(link.id).await.status, PaymentStatus::Failed);
    app.assert_emitted::<PaymentFailed>(|e| e.gateway == "ngenius");
}

/// The access token is kept per account until it expires, a changed key
/// gets a new one, accounts use their own key, outlet and webhook secret,
/// and a missing key is refused before any request.
#[renox::test]
async fn ngenius_tokens_are_kept_per_account_and_follow_the_key() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    let version = Arc::new(AtomicU32::new(1));
    let seen = version.clone();
    let app = App::new()
        .module(Auth::new().account())
        .module(
            Billing::new().gateway(renox_billing::Ngenius::resolved_accounts(
                move |_state, account: &str| {
                    let key = (account != "nobody")
                        .then(|| format!("key-{account}-{}", seen.load(Ordering::SeqCst)));
                    async move { Ok(key) }
                },
            )),
        );
    let app = TestApp::with_config(app, |config| {
        for (name, value) in [
            ("NGENIUS_OUTLET_REF", "o-default"),
            ("NGENIUS_OUTLET_REF_ACME", "o-acme"),
            ("NGENIUS_WEBHOOK_SECRET", "default-secret"),
            ("NGENIUS_WEBHOOK_SECRET_ACME", "acme-secret"),
        ] {
            config.vars.insert(name.into(), value.into());
        }
    })
    .await;
    let ana = user(&app, "Ana").await;
    let state = app.state();
    let http = app.fake_http();
    http.on(
        &format!("POST {NGENIUS}/identity/auth/access-token"),
        FakeResponse::json(200, ngenius_fixture("access_token")),
    );
    // Every order has its own reference (answers are given in turn).
    for (outlet, count) in [("o-default", 4), ("o-acme", 1)] {
        for n in 0..count {
            let mut order = ngenius_fixture("order_created");
            order["reference"] = json!(format!("{outlet}-{n}"));
            http.on(
                &format!("POST {NGENIUS}/transactions/outlets/{outlet}/orders"),
                FakeResponse::json(200, order),
            );
        }
    }
    let tokens = || {
        http.sent()
            .into_iter()
            .filter(|r| r.url.ends_with("/identity/auth/access-token"))
            .map(|r| r.header("authorization").unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let ana = &ana;
    let charge = |account: Option<&'static str>| async move {
        let charge = match account {
            Some(name) => stripe_charge().account(name),
            None => stripe_charge(),
        };
        Billing::of(state, ana).charge(charge).await
    };

    charge(None).await.unwrap();
    charge(None).await.unwrap();
    assert_eq!(tokens(), ["Basic key-default-1"], "kept for the next call");
    let acme = charge(Some("acme")).await.unwrap();
    assert_eq!(acme.account.as_deref(), Some("acme"));
    assert_eq!(tokens(), ["Basic key-default-1", "Basic key-acme-1"]);
    assert!(
        http.sent()
            .last()
            .unwrap()
            .url
            .contains("/outlets/o-acme/orders")
    );

    // The stored key changed: the next call trades the new one.
    version.store(2, Ordering::SeqCst);
    charge(None).await.unwrap();
    assert_eq!(tokens().last().unwrap(), "Basic key-default-2");

    // The token ends: the next call asks for a new one.
    let before = tokens().len();
    app.travel(Duration::from_secs(400));
    app.at_travelled_time(charge(None)).await.unwrap();
    assert_eq!(tokens().len(), before + 1);

    // No key: refused before any request, without printing a key.
    let sent = http.sent().len();
    let err = charge(Some("nobody")).await.unwrap_err();
    assert!(err.to_string().contains("N-Genius's API key"), "{err}");
    assert!(!format!("{err:?}").contains("key-"), "{err:?}");
    assert_eq!(http.sent().len(), sent);

    // Each account has its own webhook secret.
    let post = |path: &'static str, secret: &'static str| {
        app.request()
            .header("x-webhook-secret", secret)
            .post_body(path, "application/json", "{}")
    };
    post("/billing/webhooks/ngenius/acme", "default-secret")
        .await
        .assert_status(401);
    post("/billing/webhooks/ngenius/acme", "acme-secret")
        .await
        .assert_ok();
    post("/billing/webhooks/ngenius", "acme-secret")
        .await
        .assert_status(401);
    post("/billing/webhooks/ngenius/nobody", "acme-secret")
        .await
        .assert_not_found();
}

/// N-Genius has no subscriptions and no disputes API: those answer the
/// typed refusal.
#[renox::test]
async fn ngenius_answers_unsupported_for_subscriptions_and_disputes() {
    use renox_billing::{BillingPayment, Gateway, Owner, PaymentKind, Unsupported};
    let app = app().await;
    let state = app.state();
    let gateway = renox_billing::Ngenius::new("ngenius-api-key-fake", "outlet", "hook-secret");
    assert_eq!(gateway.name(), "ngenius");
    let payment = BillingPayment::new(
        "ngenius",
        PaymentKind::Charge,
        renox::money::Money::usd(100),
    );
    let refusals = [
        (
            "create_customer",
            gateway
                .create_customer(state, &Owner::new("user", 1))
                .await
                .err(),
        ),
        (
            "accept_dispute",
            gateway.accept_dispute(state, &payment).await.err(),
        ),
    ];
    for (operation, err) in refusals {
        let err = err.unwrap_or_else(|| panic!("{operation} should be refused"));
        let found = Unsupported::of(&err).expect("a typed refusal");
        assert_eq!(
            (found.gateway.as_str(), found.operation.as_str()),
            ("ngenius", operation)
        );
    }
    app.fake_http().assert_sent_count(0);
}

/// N-Genius' webhook events are read into notices.
#[test]
fn ngenius_parses_webhook_events() {
    use renox_billing::payment::{PaymentStatus, RefundStatus};
    use renox_billing::{Gateway, Notice};
    let gateway = renox_billing::Ngenius::new("ngenius-api-key-fake", "outlet", "hook-secret");
    let parse = |name: &str| {
        gateway
            .parse_webhook(ngenius_fixture(name).to_string().as_bytes())
            .unwrap()
    };
    match parse("event_purchased").as_slice() {
        [Notice::OneOff(update)] => {
            assert_eq!(update.gateway_id, NGENIUS_ORDER);
            assert_eq!(update.status, PaymentStatus::Succeeded);
        }
        other => panic!("{other:?}"),
    }
    match parse("event_declined").as_slice() {
        [Notice::OneOff(update)] => assert_eq!(update.status, PaymentStatus::Failed),
        other => panic!("{other:?}"),
    }
    match parse("event_refunded").as_slice() {
        [Notice::Refund(update)] => {
            assert_eq!(update.gateway_id, "fake-refund-1");
            assert_eq!(update.payment_gateway_id.as_deref(), Some(NGENIUS_ORDER));
            assert_eq!(
                (update.status, update.amount),
                (RefundStatus::Succeeded, Some(1_000))
            );
        }
        other => panic!("{other:?}"),
    }
    match parse("event_refund_failed").as_slice() {
        [Notice::Refund(update)] => assert_eq!(update.status, RefundStatus::Failed),
        other => panic!("{other:?}"),
    }
    // An event that says nothing new is ignored; the id is the event's.
    assert!(
        gateway
            .parse_webhook(br#"{"eventName":"AUTHORISED","order":{"reference":"o"}}"#)
            .unwrap()
            .is_empty()
    );
    let id = gateway
        .webhook_event_id(
            &renox::axum::http::HeaderMap::new(),
            ngenius_fixture("event_purchased").to_string().as_bytes(),
        )
        .unwrap();
    assert_eq!(id, "evt-0001");
}

/// A status check turns a pending payment into failed or expired like a
/// webhook would (the event carries the reference), an expiry from a webhook
/// announces the link, and a late success after either is still applied.
#[renox::test]
async fn status_checks_and_expiry_webhooks_apply_like_webhooks() {
    use renox::money::Money;
    use renox_billing::{
        BillingPayment, PaymentFailed, PaymentLink, PaymentLinkExpired, PaymentStatus,
        PaymentSucceeded,
    };
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = app_with(Billing::new().gateway(PayHub { calls })).await;
    app.fake_events();
    let state = app.state();
    let link = |reference: &'static str| {
        Billing::payment_link(
            state,
            PaymentLink::new(Money::usd(900)).reference(reference, 5),
        )
    };
    let reload =
        |id: i64| async move { BillingPayment::find(&state.db, id).await.unwrap().unwrap() };

    let declined = link("declined").await.unwrap();
    let reported = Billing::payment_status(state, declined.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Failed);
    assert_eq!(reload(declined.id).await.status, PaymentStatus::Failed);
    app.assert_emitted::<PaymentFailed>(|e| e.reference_type.as_deref() == Some("declined"));

    let gone = link("gone").await.unwrap();
    let reported = Billing::payment_status(state, gone.id).await.unwrap();
    assert_eq!(reported.status, PaymentStatus::Expired);
    assert_eq!(reload(gone.id).await.status, PaymentStatus::Expired);
    app.assert_emitted::<PaymentLinkExpired>(|e| e.payment.id == gone.id);

    // The gateway's own expiry webhook announces the link too, once.
    let hooked = link("unpaid").await.unwrap();
    let pay = hooked.gateway_id.clone().unwrap();
    for id in ["x1", "x1"] {
        app.post_body(
            "/billing/webhooks/payhub",
            "application/json",
            json!({"id": id, "kind": "payment", "pay": pay, "status": "expired"}).to_string(),
        )
        .await;
        app.run_jobs().await;
    }
    assert_eq!(reload(hooked.id).await.status, PaymentStatus::Expired);
    assert_eq!(app.emitted::<PaymentLinkExpired>().len(), 2);

    // Paid after all: the success wins over both and is evented.
    for payment in [&declined, &gone] {
        // Their reference says declined or gone; a webhook says paid.
        let pay = payment.gateway_id.clone().unwrap();
        app.post_body(
            "/billing/webhooks/payhub",
            "application/json",
            json!({"id": format!("late-{pay}"), "kind": "payment", "pay": pay, "status": "succeeded"})
                .to_string(),
        )
        .await;
        app.run_jobs().await;
        assert_eq!(reload(payment.id).await.status, PaymentStatus::Succeeded);
    }
    assert_eq!(app.emitted::<PaymentSucceeded>().len(), 2);
}

/// Two accounts of one gateway keep their own keys; a key rotated in the
/// store reaches the next call, a refund included; an account without a key
/// fails without a request and without showing any key.
#[renox::test]
async fn accounts_keep_their_keys_and_a_rotated_key_reaches_refunds() {
    use renox::money::Money;
    use renox_billing::{BillingPayment, PaymentStatus, Refund};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    let keys: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::from([
        ("a".to_string(), "sk_test_fake_a1".to_string()),
        ("b".to_string(), "sk_test_fake_b1".to_string()),
    ])));
    let shared = keys.clone();
    let stripe = Stripe::resolved_accounts(move |_state, account: &str| {
        let key = shared.lock().unwrap().get(account).cloned();
        async move { Ok(key) }
    });
    assert!(!format!("{stripe:?}").contains("sk_"));
    let app = App::new()
        .module(Auth::new().account())
        .module(Billing::new().gateway(stripe));
    let app = TestApp::with_config(app, |config| {
        config
            .vars
            .insert("STRIPE_WEBHOOK_SECRET".into(), "whsec_test".into());
    })
    .await;
    let ana = user(&app, "Ana").await;
    let state = app.state();
    let http = app.fake_http();
    http.on(
        &format!("POST {STRIPE}/checkout/sessions"),
        FakeResponse::json(200, fixture("checkout_session")),
    );
    // (Answers for one URL are given in turn: a's session, then b's.)
    let mut second = fixture("checkout_session");
    second["id"] = json!("cs_test_b2");
    http.on(
        &format!("POST {STRIPE}/checkout/sessions"),
        FakeResponse::json(200, second),
    );
    let mut second_refund = fixture("refund_pending");
    second_refund["id"] = json!("re_b2");
    for refund in [fixture("refund_pending"), second_refund] {
        http.on(
            &format!("POST {STRIPE}/refunds"),
            FakeResponse::json(200, refund),
        );
    }
    let mut paid_b = fixture("checkout_session_paid");
    paid_b["id"] = json!("cs_test_b2");
    for (id, body) in [
        ("cs_test_a1B2c3", fixture("checkout_session_paid")),
        ("cs_test_b2", paid_b),
    ] {
        http.on(
            &format!("GET {STRIPE}/checkout/sessions/{id}"),
            FakeResponse::json(200, body),
        );
    }
    let mut paid = Vec::new();
    for account in ["a", "b"] {
        let mut payment = Billing::of(state, &ana)
            .charge(stripe_charge().account(account))
            .await
            .unwrap();
        assert_eq!(payment.account.as_deref(), Some(account));
        payment.status = PaymentStatus::Succeeded;
        payment.save(&state.db).await.unwrap();
        paid.push(payment);
    }
    // a's key is rotated; the refund of a's payment uses the new one, b's
    // refund keeps b's.
    keys.lock()
        .unwrap()
        .insert("a".into(), "sk_test_fake_a2".into());
    for payment in &paid {
        Billing::refund(state, payment.id, Refund::new(Money::usd(100)))
            .await
            .unwrap();
    }
    let auth: Vec<_> = http
        .sent()
        .iter()
        .map(|r| {
            let key = r.header("authorization").unwrap_or_default().to_owned();
            (
                format!("{} {}", r.method, r.url.rsplit('/').next().unwrap()),
                key,
            )
        })
        .collect();
    let bearer = |key: &str| format!("Bearer {key}");
    assert_eq!(
        auth,
        [
            ("POST sessions".to_string(), bearer("sk_test_fake_a1")),
            ("POST sessions".to_string(), bearer("sk_test_fake_b1")),
            ("GET cs_test_a1B2c3".to_string(), bearer("sk_test_fake_a2")),
            ("POST refunds".to_string(), bearer("sk_test_fake_a2")),
            ("GET cs_test_b2".to_string(), bearer("sk_test_fake_b1")),
            ("POST refunds".to_string(), bearer("sk_test_fake_b1")),
        ]
    );

    // An account without a key: no request, no key in the message or the row.
    let rows = BillingPayment::query().count(&state.db).await.unwrap();
    let err = Billing::of(state, &ana)
        .charge(stripe_charge().account("c"))
        .await
        .unwrap_err();
    assert!(!err.to_string().contains("sk_"), "{err}");
    assert_eq!(http.sent().len(), 6);
    assert_eq!(
        BillingPayment::query().count(&state.db).await.unwrap(),
        rows
    );
}
