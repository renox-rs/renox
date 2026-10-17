# Billing

Billing lets an app charge for plans: a monthly "Pro", a yearly "Business", with a free trial,
upgrades and downgrades, and cancellations that keep access until the end of the period paid
for. Renox's `renox-billing` crate does it with Stripe (cards, worldwide) and Xendit (cards and
e-wallets in Indonesia and the Philippines): plans declared in code, a plans page, a card on the
account page, the providers' webhooks, and a guard for pages only subscribers see.
[examples/bikeshop](../examples/bikeshop) bills service plans with it: a named subscription per bike.

In this guide:

- [Add it to your app](#add-it-to-your-app)
- [Plans](#plans)
- [Setting up Stripe](#setting-up-stripe)
- [Setting up Xendit](#setting-up-xendit)
- [What your users see](#what-your-users-see)
- [Checking a subscription in code](#checking-a-subscription-in-code)
- [Pages for subscribers only](#pages-for-subscribers-only)
- [Trials](#trials)
- [Changing plan, canceling, resuming](#changing-plan-canceling-resuming)
- [Webhooks](#webhooks)
- [Events and the activity log](#events-and-the-activity-log)
- [Billing teams instead of users](#billing-teams-instead-of-users)
- [Another payment provider](#another-payment-provider)
- [Changing the pages](#changing-the-pages)
- [Testing](#testing)

### Words you'll meet

| Word | What it means |
|---|---|
| **plan** | What you sell: a key (`pro`), a name, a price and how often it's charged. Declared in code, not in a table. |
| **gateway** | The payment provider that charges: Stripe or Xendit (or one of your own). |
| **subscription** | One owner's subscription to a plan at a gateway: a row in `subscriptions`. |
| **owner** (billable) | Who pays: a user by default, or what your app bills (a team). |
| **trial** | Free days before the first charge. |
| **grace period** | After a cancellation, the time left of the period already paid for: the owner keeps access until it ends. |
| **webhook** | A call from the gateway to your app when something happens (a payment, a renewal, a cancellation). The subscription's state arrives this way. |
| **proration** | Charging (or crediting) the difference when a plan changes in the middle of a period. |

> [!NOTE]
> **Coming from Laravel:** this is Cashier: `Billing::of(&state, &user)` is the billable model
> (`subscribed()`, `on_trial()`, `swap()`, `cancel()`, `resume()`), with the plans page, the
> account card and the webhook handling done for you.

## Add it to your app

Add the crate next to `renox`, at the same version:

```toml
[dependencies]
renox = "1.1"
renox-billing = "1.1"
```

Then add the module, next to the `Auth` module:

```rust
use renox::prelude::*;
use renox_billing::{Billing, Interval, Plan};

/// The app: login pages, the account page, and subscriptions.
pub fn app() -> App {
    App::new()
        // `.account()` is the /account page, where the subscription's card goes.
        .module(Auth::new().account())
        // Brings its tables (a migration), its routes, the plans page and its card on /account.
        .module(
            Billing::new()
                .plan(Plan::new("basic", "Basic").price(900, "USD", Interval::Month))
                .plan(
                    Plan::new("pro", "Pro")
                        .price(1_900, "USD", Interval::Month)
                        .trial_days(14)
                        .feature("Unlimited projects"),
                )
                .stripe(), // STRIPE_SECRET and STRIPE_WEBHOOK_SECRET from .env
        )
}
```

Run `migrate`: the module brings two tables, `subscriptions` and `billing_customers` (one
customer id per owner and gateway). Webhooks are processed by queue workers, which `serve`
runs (`QUEUE_WORKERS`, 2 by default).

## Plans

A plan is a key that never changes once people subscribed (it is stored with each
subscription), a name, and a price:

```rust
use renox_billing::{Interval, Plan};

# let _ =
Plan::new("pro", "Pro")
    .price(1_900, "USD", Interval::Month) // amounts in the smallest unit: $19.00
    .trial_days(14)                        // free days, for a first subscription
    .description("For growing teams")
    .feature("Unlimited projects")         // listed on the plans page
    .feature("Priority support")
    .via("stripe")                         // the gateway; the first one set up when left out
    .price_id("stripe", "price_1Pq…")      // Stripe's Price (see below)
# ;
```

Amounts are `i64` in the currency's smallest unit as Renox counts it
(`renox::currency_decimals`): cents for `USD` (`1_900` is $19.00), rupiah for `IDR` (`99_000`
is Rp 99.000). Intervals are `Day`, `Week`, `Month` and `Year`. Plans show in the order they're
added.

## Setting up Stripe

1. In Stripe's dashboard, make a product per plan with a **recurring** price, and copy each
   price's id (`price_…`).
2. Give the ids to the plans: `.price_id("stripe", "price_…")` in code, or
   `STRIPE_PRICE_<KEY>` in `.env` (`STRIPE_PRICE_PRO`; a `-` in the key is `_`). The variable
   lets test mode and live mode use different prices.
3. Add a webhook endpoint at `APP_URL/billing/webhooks/stripe` with the events
   `customer.subscription.created`, `customer.subscription.updated`,
   `customer.subscription.deleted`, `invoice.payment_succeeded` and `invoice.payment_failed`.
4. Put the keys in `.env`:

```bash
STRIPE_SECRET=sk_test_…
STRIPE_WEBHOOK_SECRET=whsec_…
STRIPE_PRICE_BASIC=price_…
STRIPE_PRICE_PRO=price_…
```

`Billing::new().stripe()` reads them when a call needs them; a gateway without its secret key
is off. Keys in code are `.gateway(Stripe::new("sk_…", "whsec_…"))`.

Subscribing goes through Stripe Checkout (a hosted payment page, `mode=subscription`); plan
changes, cancellations and resumes use the Subscriptions API. Stripe prorates plan changes.
Its Checkout refuses a trial shorter than 48 hours, so a trial with less left than that isn't
passed on.

## Setting up Xendit

Xendit is the Indonesian gateway (Midtrans is the other common one). Renox uses Xendit
because it has recurring plans (`/recurring/plans`): Xendit stores the customer's card or
e-wallet and charges each cycle itself, where Midtrans' subscriptions need a saved card token
the app collects first.

1. In Xendit's dashboard, copy the secret API key (with write access to customers and
   recurring plans) and the webhook verification token.
2. Set the webhook URL for the Recurring events (`recurring.plan.activated`,
   `recurring.plan.inactivated`, `recurring.cycle.created`, `recurring.cycle.succeeded`,
   `recurring.cycle.retrying`, `recurring.cycle.failed`) to
   `APP_URL/billing/webhooks/xendit`.
3. Put the keys in `.env`, and add the module's gateway:

```bash
XENDIT_SECRET_KEY=xnd_development_…
XENDIT_CALLBACK_TOKEN=…
```

```rust
use renox_billing::{Billing, Interval, Plan};

# let _ =
Billing::new()
    .plan(Plan::new("pro", "Pro").price(1_900, "USD", Interval::Month))
    .plan(Plan::new("pro-idr", "Pro (IDR)").price(99_000, "IDR", Interval::Month).via("xendit"))
    .stripe()
    .xendit()
# ;
```

Xendit charges the plan's own amount and currency. Subscribing makes a recurring plan and
sends the customer to Xendit's page to link a payment method; with a trial, the first charge is
scheduled for the trial's end. What Xendit doesn't do:

- **prorate**: a plan change applies its new amount from the next cycle;
- **change the interval**: a monthly plan can't become a yearly one (cancel, then subscribe);
- **resume**: canceling stops the charges at once (the customer keeps access until the end of
  the period paid for); to go on, they subscribe again after it ends.

## What your users see

- **`/billing`** (`billing.plans`): the plans, each with its price, trial and features, and
  "Subscribe" (or "Switch to …" when subscribed; "Start free trial" with
  [generic trials](#trials)).
- **The gateway's payment page.** Afterwards they come back to `/billing/return`, then go to
  the account page (or `Billing::redirect_to`) with "Thank you! Your subscription starts as
  soon as the payment is confirmed." The subscription arrives by webhook, usually within
  seconds.
- **A "Subscription" card on `/account`**: the plan and price, "Free trial until …", "Renews
  on …" or "Canceled: you have access until …", a warning after a failed payment, and "Change
  plan", "Cancel subscription" (with a confirmation) and "Resume".

## Checking a subscription in code

`Billing::of(&state, &user)` is the user's subscriptions, Cashier's billable methods:

```rust
use renox::prelude::*;
use renox_billing::Billing;

async fn dashboard(State(state): State<AppState>, user: AuthUser) -> Result<View> {
    let billing = Billing::of(&state, &*user);
    let subscribed = billing.subscribed().await?;       // trial, paid, or in its grace period
    let pro = billing.subscribed_to("pro").await?;      // …to this plan
    let on_trial = billing.on_trial().await?;
    let grace = billing.on_grace_period().await?;       // canceled, not ended yet
    let subscription = billing.subscription().await?;   // the newest row, whatever its state
    let renews = subscription.and_then(|s| s.current_period_end);
    Ok(view("dashboard.html", context! { subscribed, pro, on_trial, grace, renews }))
}
```

A `Subscription` answers the same questions: `valid()`, `on_trial()`, `canceled()`,
`on_grace_period()`, `ended()`, `past_due()`, `has_plan(key)`. Its `status` is what the
gateway last said: `Trialing`, `Active`, `PastDue` (a payment failed and is being retried),
`Canceled` or `Incomplete` (not paid yet). Valid means on a trial, active and not canceled, or
canceled with time left; past due and incomplete aren't valid.

Every method works on the `default` subscription. An app that sells more than one thing names
the others: `Billing::of(&state, &user).named("storage").subscribed()`.

## Pages for subscribers only

`SubscriptionRoutes` adds two guards to `Routes`. Like `require_auth`, a guard covers the
routes added before it; put `.require_auth()` after it:

```rust
use renox::prelude::*;
use renox_billing::SubscriptionRoutes;

# async fn reports() -> &'static str { "" }
# async fn exports() -> &'static str { "" }
fn routes() -> Routes {
    Routes::new()
        .get("/reports", reports)
        .require_subscription() // any plan
        .merge(
            Routes::new()
                .get("/exports", exports)
                .require_plan(&["pro", "business"]), // these plans only
        )
        .require_auth()
}
# fn main() { let _ = routes; }
```

Others go to the plans page with "Choose a plan to continue."; a JSON request gets
`402 Payment Required`. A paused subscription is not valid, so the guards turn it away too.

An app that sells a subscription per item (`Billing::of(..).named("garage")`) names it in the
guard: `require_subscription_named` and `require_plan_named` check that subscription instead
of `default`:

```rust
use renox::prelude::*;
use renox_billing::SubscriptionRoutes;

# async fn garage() -> &'static str { "" }
# async fn tuning() -> &'static str { "" }
fn routes() -> Routes {
    Routes::new()
        .get("/garage", garage)
        .require_subscription_named("garage")
        .merge(
            Routes::new()
                .get("/garage/tuning", tuning)
                .require_plan_named("garage", &["pro"]),
        )
        .require_auth()
}
# fn main() { let _ = routes; }
```

The name is fixed per guard. When it depends on the URL (one subscription per bike id), check
in the handler with `Billing::of(..).named(..).subscribed()`.

## Trials

A plan's `trial_days` apply to an owner's first subscription of that name. At Stripe it's
the subscription's trial (no charge until it ends); at Xendit the first cycle is scheduled at
its end.

With `.generic_trials()`, the plans page also offers "Start free trial" for plans that have a
trial: it starts without a payment method (Cashier's "generic trial"), as a subscription with
no gateway. When the user subscribes before it ends, the days left carry over (they aren't
charged for them); when it ends without that, the subscription isn't valid any more.

```rust
use renox::prelude::*;
use renox_billing::Billing;

async fn start(state: &AppState, user: &User) -> Result {
    Billing::of(state, user).start_trial("pro").await?;
    Ok(())
}
# fn main() { let _ = start; }
```

## Changing plan, canceling, resuming

The account card and the plans page do these for the logged-in user; in code:

```rust
use renox::prelude::*;
use renox_billing::Billing;

async fn manage(state: &AppState, user: &User) -> Result {
    let billing = Billing::of(state, user);
    let url = billing.checkout("pro").await?; // the gateway's payment page: redirect there
    billing.swap("basic").await?;              // upgrade or downgrade, prorated at Stripe
    billing.cancel().await?;                   // at the period's end: the grace period
    billing.resume().await?;                   // before it ends (Stripe)
    billing.cancel_now().await?;               // access ends at once
    billing.pause(None).await?;                // stop billing for a while (Stripe)
    billing.unpause().await?;                  // take billing up again
    # let _ = url;
    Ok(())
}
# fn main() { let _ = manage; }
```

`Billing::new().without_proration()` makes plan changes apply without charging or crediting
the rest of the period. A plan sold through another gateway can't be swapped to (cancel, then
subscribe). A refusal (already subscribed, an unknown plan, nothing to resume) is an
`Error::BadRequest` whose message the pages show as a toast.

**Pausing** stops the charges without canceling: `pause(Some(date))` also tells the gateway when
to go on by itself (Stripe's `pause_collection`, charges are voided meanwhile). A paused
subscription has the status `Paused`, is not valid (the guards turn the owner away; `cancel`
still works), and `SubscriptionPaused` / `SubscriptionUnpaused` are emitted when it starts and
ends, also when the change arrives by webhook. The account card shows "Pause billing" when
`can_pause()` (an active, paid subscription at a gateway that pauses) and "Take it up again"
while paused. Xendit can't pause: `pause` answers an `Error::BadRequest` and changes nothing.
A gateway of your own implements `pauses`, `pause` and `unpause`.

## Webhooks

The gateways call `POST /billing/webhooks/{gateway}` (or `/billing/webhooks/{gateway}/{account}`
for a named account of that gateway: it is verified with that account's secret, read from the
default secret's name with the account appended, `STRIPE_WEBHOOK_SECRET_ACME_LLC` for `acme-llc`
or `XENDIT_CALLBACK_TOKEN_ACME_LLC`; an account without one answers 404, and an event id is
stored per account). It goes through `renox::webhook`:

1. the gateway checks the call: Stripe's `Stripe-Signature` (an HMAC-SHA256 of the timestamp
   and the body with `STRIPE_WEBHOOK_SECRET`, at most five minutes old), Xendit's
   `x-callback-token` (compared in constant time with `XENDIT_CALLBACK_TOKEN`); otherwise 401
   and nothing is stored;
2. the call is stored in `webhook_calls` once per event id, so a provider's retry is answered
   200 and not processed again;
3. a queue worker applies it: the subscription's row is made or updated, and the events below
   are emitted. `webhook:failed` lists calls whose processing failed; `webhook:retry <id>` runs
   one again.

Applying is safe to repeat and to receive out of order: a change that changes nothing emits
nothing, and each row keeps the time of the newest gateway event applied (`synced_at`), so an
older event that arrives late is ignored. A subscription made outside the app (no
`renox_billable` metadata and an unknown customer) is ignored too.

## Events and the activity log

| Event | When |
|---|---|
| `SubscriptionCreated` | a subscription was made (a webhook, or a trial without a payment method) |
| `SubscriptionUpdated` | its plan, status, trial, period or end changed (`previous_plan`, `previous_status`) |
| `SubscriptionCanceled` | it was canceled, now or at the period's end (`subscription.ends_at`) |
| `SubscriptionPaused` | billing was paused (`Customer::pause`, or the gateway said so) |
| `SubscriptionUnpaused` | a paused subscription is billed again |
| `PaymentSucceeded` | the gateway took a payment (`amount`, `currency`, `owner`, `subscription`) |
| `PaymentFailed` | a payment failed; the subscription is past due while the gateway retries |

```rust
use renox::prelude::*;
use renox_billing::{PaymentFailed, SubscriptionCreated};

# let _ =
App::new()
    .listen(|e: SubscriptionCreated, _state| async move {
        tracing::info!(plan = e.subscription.plan, "a new subscription");
        Ok(())
    })
    .listen(|e: PaymentFailed, _state| async move {
        // e.g. mail the owner a link to their payment method
        tracing::warn!(amount = e.amount, "a payment failed");
        Ok(())
    })
# ;
```

With the `Audit` module, a user's `billing.subscribed`, `billing.canceled` and
`billing.payment_failed` are recorded in `audit_logs`.

When a user deletes their account, their running subscriptions are canceled at the gateway and
their billing rows deleted.

## Billing teams instead of users

The owner is any `Billable`: a kind and an id, with an address and a name for the gateway's
customer record. Users are billable as they are; for a team:

```rust
use renox::prelude::*;
use renox_billing::{Billable, Billing, Owner};

struct Team {
    id: i64,
    name: String,
    billing_email: String,
}

impl Billable for Team {
    fn owner(&self) -> Owner {
        Owner::new("team", self.id)
            .name(self.name.clone())
            .email(self.billing_email.clone())
    }
}

async fn team_is_pro(state: &AppState, team: &Team) -> Result<bool> {
    Billing::of(state, team).subscribed_to("pro").await
}
# fn main() { let _ = team_is_pro; }
```

The built-in pages and guards are about the logged-in user; a team's pages call the same
methods (`checkout`, `swap`, `cancel`, …) with the team.

## Another payment provider

A gateway is one impl of the `Gateway` trait: making a customer, a checkout, a plan change, a
cancellation (and a resume, if it can), and reading its webhooks into `Notice`s. Each method
returns a `BoxFuture` (a boxed, `Send` future) so the trait works behind `dyn`; a
subscription's state is a `Remote`, where only what is set changes the row:

```rust
use renox::Config;
use renox::axum::http::HeaderMap;
use renox::prelude::*;
use renox_billing::{
    BoxFuture, Checkout, CheckoutRequest, Gateway, Notice, Owner, Plan, Remote, Subscription,
    SubscriptionStatus,
};

/// A provider with a hosted checkout and JSON webhooks.
struct Acme;

impl Gateway for Acme {
    fn name(&self) -> &str {
        "acme"
    }
    fn label(&self) -> &str {
        "Acme Pay"
    }
    fn configured(&self, config: &Config) -> bool {
        config.var("ACME_KEY").is_some()
    }
    fn create_customer<'a>(&'a self, state: &'a AppState, owner: &'a Owner) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let made: renox::serde_json::Value = state
                .http
                .post("https://api.acme.test/customers")
                .json(&json!({ "email": owner.email, "reference": owner.key() }))
                .send()
                .await?
                .error_for_status()?
                .json()?;
            Ok(made["id"].as_str().unwrap_or_default().to_owned())
        })
    }
    fn checkout<'a>(&'a self, state: &'a AppState, request: &'a CheckoutRequest) -> BoxFuture<'a, Result<Checkout>> {
        Box::pin(async move {
            // Send the metadata along: the webhooks bring it back.
            let metadata: std::collections::HashMap<_, _> = request.metadata().into_iter().collect();
            let session: renox::serde_json::Value = state
                .http
                .post("https://api.acme.test/subscriptions")
                .json(&json!({
                    "customer": request.customer_id,
                    "amount": request.plan.amount,
                    "currency": request.plan.currency,
                    "return_url": request.success_url,
                    "metadata": metadata,
                }))
                .send()
                .await?
                .error_for_status()?
                .json()?;
            Ok(Checkout::redirect(session["pay_url"].as_str().unwrap_or_default()))
        })
    }
    fn swap<'a>(&'a self, _state: &'a AppState, subscription: &'a Subscription, plan: &'a Plan, _prorate: bool) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            // … call the provider …
            Ok(Remote::new(subscription.gateway_id.clone().unwrap_or_default()).plan(plan.key.clone()))
        })
    }
    fn cancel<'a>(&'a self, _state: &'a AppState, subscription: &'a Subscription, at_period_end: bool) -> BoxFuture<'a, Result<Remote>> {
        Box::pin(async move {
            let remote = Remote::new(subscription.gateway_id.clone().unwrap_or_default());
            Ok(if at_period_end {
                remote.ends_at(subscription.current_period_end)
            } else {
                remote.status(SubscriptionStatus::Canceled)
            })
        })
    }
    fn verify_webhook(&self, config: &Config, headers: &HeaderMap, body: &[u8]) -> Result {
        let key = config.var("ACME_WEBHOOK_KEY").unwrap_or_default();
        let signature = headers.get("acme-signature").and_then(|v| v.to_str().ok()).unwrap_or_default();
        renox::webhook::ensure(renox::webhook::verify_hmac_sha256(key, body, signature))
    }
    fn parse_webhook(&self, body: &[u8]) -> Result<Vec<Notice>> {
        let event: renox::serde_json::Value = renox::serde_json::from_slice(body)?;
        let sub = &event["subscription"];
        let status = match sub["status"].as_str() {
            Some("active") => SubscriptionStatus::Active,
            Some("canceled") => SubscriptionStatus::Canceled,
            _ => SubscriptionStatus::Incomplete,
        };
        let mut remote = Remote::new(sub["id"].as_str().unwrap_or_default()).status(status);
        if let Some(owner) = sub["metadata"]["renox_billable"].as_str() {
            remote = remote.owner(owner);
        }
        if let Some(plan) = sub["metadata"]["renox_plan"].as_str() {
            remote = remote.plan(plan);
        }
        if let Some(at) = event["created"].as_i64() {
            remote = remote.at(at); // orders late events
        }
        Ok(vec![Notice::Subscription(remote)])
    }
}

# let _ =
renox_billing::Billing::new().gateway(Acme)
# ;
```

`verify_webhook` gets only the configuration. A gateway whose signing key lives in the app's own
settings (given with `App::provide`, say) overrides `verify_webhook_with(state, headers, body)`
instead: it is what the webhook route calls, and by default it calls `verify_webhook` with
`state.config`. In the same way `parse_webhook_with(state, body)` defaults to `parse_webhook`.
Pausing is `pauses`, `pause` and `unpause` (the defaults refuse).

Its webhooks come to `/billing/webhooks/acme`. Event ids for "process once" are the
`webhook-id` header, else the JSON body's `id`, else a hash of the body; override
`Gateway::webhook_event_id` for another rule. Use the app's HTTP client (`state.http`) for
every call: tests fake it.

## Changing the pages

The templates are compiled into the crate. To change one, add a file with the same name to
your app's `resources/views/`: yours is used instead.

| File | What it is |
|---|---|
| `billing/plans.html` | the plans page (gets `plans`: `key`, `label`, `description`, `price`, `trial_days`, `features`, `current`; `current`: the subscription as below, or none; `subscribed`, `trial_available`, `generic_trials`, `account`) |
| `billing/section.html` | the card on `/account` (gets `section.data`: `current` with `plan`, `plan_label`, `price`, `status`, `status_label`, `valid`, `on_trial`, `generic_trial`, `trial_ends_at`, `canceled`, `on_grace_period`, `ends_at`, `current_period_end`, `past_due`, `can_resume`, `paused`, `can_pause`, `gateway`; and `plans_url`) |

The routes are `billing.plans` (`GET /billing`), `billing.checkout` and `billing.trial`
(`POST /billing/checkout/{plan}`, `/billing/trial/{plan}`), `billing.return`
(`GET /billing/return`), `billing.swap` (`POST /billing/swap/{plan}`), `billing.cancel` and
`billing.resume`, `billing.pause` and `billing.unpause` (`POST /billing/cancel`, `/billing/resume`,
`/billing/pause`, `/billing/unpause`), all for logged-in users, and
`webhooks.billing` (`POST /billing/webhooks/{gateway}` and `/billing/webhooks/{gateway}/{account}`).

## Testing

Tests never reach the gateways: `TestApp::fake_http` answers the API calls, and a test signs
its webhooks with the test secret. Run the queued work with `run_jobs`, and move the clock with
`travel` to end a trial or a period:

```rust
use renox::http::FakeResponse;
use renox::prelude::*;
use renox::testing::TestApp;
use renox_billing::{Billing, Interval, Plan, Stripe};

async fn demo() {
    let app = TestApp::new(
        App::new().module(Auth::new()).module(
            Billing::new()
                .plan(Plan::new("pro", "Pro").price(1_900, "USD", Interval::Month).price_id("stripe", "price_pro"))
                // Keys in code: tests don't read `.env`.
                .gateway(Stripe::new("sk_test", "whsec_test")),
        ),
    )
    .await;
    let user = User::register(app.db(), "Ana", "ana@example.com", "a long password").await.unwrap();
    app.acting_as(&user);

    // Subscribing: a customer, then a Checkout Session.
    let http = app.fake_http();
    http.on("POST https://api.stripe.com/v1/customers", FakeResponse::json(200, json!({ "id": "cus_1" })));
    http.on(
        "POST https://api.stripe.com/v1/checkout/sessions",
        FakeResponse::json(200, json!({ "id": "cs_1", "url": "https://checkout.stripe.com/c/pay/cs_1" })),
    );
    app.post("/billing/checkout/pro", &[]).await.assert_redirect("https://checkout.stripe.com/c/pay/cs_1");

    // Stripe's webhook, signed with the test secret.
    let now = renox::db::now().timestamp();
    let payload = json!({
        "id": "evt_1",
        "type": "customer.subscription.created",
        "created": now,
        "data": { "object": {
            "id": "sub_1",
            "customer": "cus_1",
            "status": "active",
            "metadata": { "renox_billable": format!("user:{}", user.id), "renox_plan": "pro" },
            "items": { "data": [{ "id": "si_1", "price": { "id": "price_pro" } }] },
        } },
    })
    .to_string();
    let signature = renox::webhook::hmac_sha256_hex("whsec_test", format!("{now}.{payload}"));
    app.request()
        .header("stripe-signature", &format!("t={now},v1={signature}"))
        .post_body("/billing/webhooks/stripe", "application/json", payload)
        .await
        .assert_ok();
    app.run_jobs().await;
    assert!(Billing::of(app.state(), &user).subscribed().await.unwrap());
}
# fn main() {
#     let _ = demo;
# }
```

The crate's own tests (`crates/renox-billing/tests/billing.rs`) cover every case above, on
SQLite and PostgreSQL.
