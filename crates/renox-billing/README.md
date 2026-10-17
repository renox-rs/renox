# renox-billing

Subscriptions for [Renox](https://github.com/renox-rs/renox) apps (Laravel's Cashier): plans
declared in code, free trials, upgrades and downgrades, cancellations with a grace period, with
Stripe and Xendit, plus one-off payments and payment links (Stripe, Xendit, N-Genius).

```rust
use renox::prelude::*;
use renox_billing::{Billing, Interval, Plan};

App::new()
    .module(Auth::new().account()) // the account page shows the subscription
    .module(
        Billing::new()
            .plan(Plan::new("basic", "Basic").price(900, "USD", Interval::Month))
            .plan(Plan::new("pro", "Pro").price(1_900, "USD", Interval::Month).trial_days(14))
            .stripe() // STRIPE_SECRET, STRIPE_WEBHOOK_SECRET, STRIPE_PRICE_<PLAN>
            .xendit(), // XENDIT_SECRET_KEY, XENDIT_CALLBACK_TOKEN (plans `.via("xendit")`)
    )
```

What it adds:

- a plans page (`/billing`) that sends the user to the gateway's payment page (Stripe
  Checkout, Xendit's recurring plan page), and a "Subscription" card on `/account` with
  "Change plan", "Cancel" and "Resume";
- `Billing::of(&state, &user)`: `subscribed()`, `subscribed_to(plan)`, `on_trial()`,
  `on_grace_period()`, `checkout(plan)`, `start_trial(plan)`, `swap(plan)`, `cancel()`,
  `cancel_now()`, `resume()`; any `Billable` owner (a team) works too;
- `require_subscription()` and `require_plan(&[…])` guards for routes;
- the gateways' webhooks through `renox::webhook` (`/billing/webhooks/{gateway}`): verified,
  processed once per event, out-of-order safe, into the `subscriptions` and
  `billing_customers` tables;
- the events `SubscriptionCreated`, `SubscriptionUpdated`, `SubscriptionCanceled`,
  `PaymentSucceeded` and `PaymentFailed`, recorded in the activity log when the app has the
  `Audit` module;
- one-off payments: `Billing::of(&state, &user).charge(Charge::new(money).reference("job", 42))`
  and `Billing::payment_link(&state, PaymentLink::new(money))` store a `billing_payments` row
  with your reference; `Billing::refund` (pending until the gateway confirms, partial refunds
  add up), `Billing::payment_status`, disputes, and a scheduled `billing:expire-links` sweep for
  links that ran out; a success that arrives after a failure is still applied;
- keys looked up per installation (`Stripe::resolved(..)`) and several accounts per gateway
  (`.account("acme-llc")`, webhook `/billing/webhooks/{gateway}/{account}`);
- events `PaymentRefunded`, `RefundSucceeded`, `RefundFailed`, `DisputeOpened`, `DisputeWon`,
  `DisputeLost` and `PaymentLinkExpired`;
- Stripe, Xendit and N-Genius (`.ngenius()`); another provider is one impl of the `Gateway` trait.

The guide is [docs/billing.md](https://github.com/renox-rs/renox/blob/main/docs/billing.md);
examples/bikeshop uses it for its service plans (Stripe, Xendit and a demo gateway). Versioned with `renox`: use the same version for both.
