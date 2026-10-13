//! "About this page" entries for the plans area's pages (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

const MODEL: &str = "examples/bikeshop/src/app/plans/model.rs";
const BILLING: &str = "examples/bikeshop/src/app/plans/billing.rs";
const SUBSCRIBE: &str = "examples/bikeshop/src/app/plans/subscribe.rs";
const MINE: &str = "examples/bikeshop/src/app/plans/mine.rs";
const VISITS: &str = "examples/bikeshop/src/app/plans/visits.rs";
const SYNC: &str = "examples/bikeshop/src/app/plans/sync.rs";
const TASKS: &str = "examples/bikeshop/src/app/plans/tasks.rs";
const DEMO: &str = "examples/bikeshop/src/app/plans/demo.rs";
const MAILS: &str = "examples/bikeshop/src/app/plans/mails.rs";
const CAPACITY: &str = "examples/bikeshop/src/app/workshop/capacity.rs";
const TESTS: &str = "examples/bikeshop/tests/plans.rs";
const BROWSER: &str = "tests/browser/bikeshop-plans.test.mjs";

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "plans.index",
            path: "/plans",
            title: "Service plans",
            purpose: "The service plans side by side: a weekly commuter check, e-bike care \
                      every two weeks, a monthly tune-up and a quarterly full service, each \
                      with its monthly price, how often the bike comes in, the parts discount \
                      it gives, and a table comparing what every plan includes, task by task. \
                      This is where a customer who rides a lot starts a plan.",
            who: "Anyone: visitors compare, customers choose a plan for one of their bikes.",
            audience: &[Audience::Visitor, Audience::Customer],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "renox_billing::Plan + Interval",
                    why: "Each service plan is a renox-billing `Plan` declared in code \
                          (`Plan::new(\"monthly-tune-up\", …).price(amount, \"USD\", \
                          Interval::Month)`), twice: paid by card (Stripe, or the demo \
                          gateway without Stripe's keys) and through Xendit, which only \
                          charges rupiah, so its copy carries its own `IDR` price (the \
                          dollar price at the shop's fixed `RUPIAH_PER_DOLLAR`). Plans live in \
                          code because a gateway's price can't change under a subscriber. \
                          The `service_plans` rows this page reads (what the admin panel \
                          edits) are seeded from the same list \
                          (`seed::content::SERVICE_PLANS`), so keys and prices agree.",
                },
                Feature {
                    api: "money filter",
                    why: "Prices are integers in cents (`price` per visit, \
                          `ServicePlan::monthly_price()` a month). The cards use the `money` \
                          filter and the table's per-visit row `renox::format_money`, the \
                          function behind it (through `money::format`, which turns cents \
                          into dollars first), so both are written the visitor's way with \
                          `APP_CURRENCY` and nothing is formatted by hand.",
                },
                Feature {
                    api: "renox-blocks",
                    why: "The kit has no pricing cards or comparison table, so the \
                          `compare_plans` block of renox-blocks draws both: cards on a CSS grid (one column \
                          on a phone), the monthly tune-up highlighted, and a table whose \
                          ticks and dashes have screen-reader text (\"Included\", \"Not \
                          included\"), so the comparison doesn't rest on an icon alone.",
                },
                Feature {
                    api: "Pivot",
                    why: "A plan's tasks are a many-to-many: the `Pivot` `PLAN_TASKS` (table \
                          `plan_tasks`), filled by the seeder with `attach`. To compare every \
                          plan at once the page doesn't load each plan's tasks: it reads all \
                          the pairs in one plain query and every task in another, so the \
                          table costs three queries whatever the number of plans (a test \
                          adds plans and counts them).",
                },
            ],
            under_hood: "Three queries: the plans sold, the `plan_tasks` pairs, the service \
                         tasks. Nothing is written. Each card links to the subscribe form \
                         with the plan chosen (`/plans/subscribe?plan=…`).",
            docs: &[
                "docs/billing.md#plans",
                "docs/relations.md#pivot-columns",
                "docs/ui.md#formatting-values",
            ],
            sources: &[
                SUBSCRIBE,
                BILLING,
                MODEL,
                "examples/bikeshop/src/seed/content.rs",
                "examples/bikeshop/resources/views/plans/index.html",
                "crates/renox-blocks/views/blocks.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Query: the plans and their tasks, the pivot read with `fetch_as`",
                    region: "plans.index.query",
                },
                Code {
                    title: "Handler: one card per plan for the `compare_plans` block",
                    region: "plans.index.cards",
                },
                Code {
                    title: "Template: the shop's `compare_plans` block, then how it works",
                    region: "plans.index.template",
                },
            ],
        },
        Explanation {
            route: "plans.subscribe",
            path: "/plans/subscribe",
            title: "Subscribe a bike to a plan",
            purpose: "The customer chooses one of their bikes, a plan, the home store whose \
                      workshop does the visits, the weekday they prefer and how to pay (a \
                      card, or Xendit, in rupiah, by card or e-wallet). The summary shows the \
                      monthly price and the first visit's day. Sending it takes them to the \
                      gateway's payment page; the plan starts when the gateway confirms.",
            who: "Logged-in customers with a bike registered (`/bikes`).",
            audience: &[Audience::Customer],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "Billing::of(…).named(…).checkout(…)",
                    why: "A customer may have a plan on each of their bikes, so each bike \
                          is its own renox-billing subscription of the user, named \
                          `bike-{id}`: `Billing::of(&state, &user).named(\"bike-12\")\
                          .checkout(\"monthly-tune-up\")` makes the gateway's customer, \
                          starts its checkout and answers the page to send the customer to. \
                          The name rides in the gateway's metadata and comes back with \
                          every webhook, so the shop knows which bike was paid for.",
                },
                Feature {
                    api: "renox-billing gateways: Stripe, Xendit, a Gateway of the shop's own",
                    why: "A card plan goes to Stripe when `STRIPE_SECRET` is set; the Xendit \
                          variant (`.via(\"xendit\")`) when `XENDIT_SECRET_KEY` is. Without \
                          Stripe's key, and never in production, a demo `Gateway` written \
                          in the example (`plans/demo.rs`) stands in: its \"hosted page\" is \
                          a signed page of the app, and its webhook takes the very path a \
                          real one does, so the whole flow runs on a laptop.",
                },
                Feature {
                    api: "Registry::provide",
                    why: "renox-billing gives its settings (plans, gateways) to the whole \
                          app with `Registry::provide`, so `Billing::of(&state, …)` finds \
                          them from the state alone: in this page's handler, and in code \
                          without a request (the webhook's queue job, the event listeners). \
                          The form offers only the ways of paying whose gateway says it is \
                          `configured`.",
                },
                Feature {
                    api: "Valid<T> + after hook",
                    why: "The rules check the plan and the store exist, the weekday is 1–7 \
                          and the way of paying is `card` or `xendit`; the `after` hook \
                          checks what needs the database: the bike is the customer's, it \
                          has no running plan (one plan per bike), the store opens on that \
                          weekday, and that way of paying is set up. Errors show next to \
                          the fields with the input kept, before any gateway is called.",
                },
                Feature {
                    api: "Htmx",
                    why: "The weekdays depend on the store's opening days and the summary \
                          on the plan, so the form `hx-get`s this page on every change and \
                          swaps only that part (`hx-select`); without JavaScript the form \
                          still sends, and the server checks everything again.",
                },
                Feature {
                    api: "UI kit: select, radio, toggle_buttons, infolist",
                    why: "Kit fields only: the bike and the store as selects, the plan and \
                          the way of paying as radios, the weekday as toggle buttons (only \
                          the days the store opens), the summary as an infolist. The \
                          summary sits beside the form on a wide screen and under it on a \
                          phone (the page's own CSS grid).",
                },
                Feature {
                    api: "HxRedirect",
                    why: "The form is sent with htmx, and paying happens on the gateway's own \
                          page: a plain `303` would only be followed inside htmx's request and \
                          swapped into this page, so the handler answers an htmx request with \
                          `HxRedirect(url)` and the browser leaves for Stripe or Xendit; \
                          without JavaScript it is a normal `Redirect::to`.",
                },
            ],
            under_hood: "On load: the customer's bikes and which already have a plan, the \
                         plans, the stores. On send: the plan is written as **pending** (an \
                         abandoned checkout of the same bike is reused), then renox-billing \
                         makes the customer at the gateway (`billing_customers`) and the \
                         checkout; the browser goes to the gateway (a gateway that doesn't \
                         answer shows an error next to the way of paying). Its webhook \
                         (`POST /billing/webhooks/{gateway}`) is verified, stored once in \
                         `webhook_calls` and applied by a queue worker into `subscriptions`; \
                         `SubscriptionCreated` and `PaymentSucceeded` follow, and the shop's \
                         listeners (`plans/sync.rs`) start the plan, book its first visits \
                         and mail the customer.",
            docs: &[
                "docs/billing.md#checking-a-subscription-in-code",
                "docs/billing.md#webhooks",
                "docs/billing.md#another-payment-provider",
                "docs/validation.md#hooks-prepare-authorize-after",
            ],
            sources: &[
                SUBSCRIBE,
                BILLING,
                DEMO,
                SYNC,
                "examples/bikeshop/resources/views/plans/subscribe.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Template: htmx swaps the weekdays and the summary as the form changes",
                    region: "plans.subscribe.template",
                },
                Code {
                    title: "Form: one plan per bike, a weekday the store opens, a gateway set up",
                    region: "plans.subscribe.form",
                },
                Code {
                    title: "Checkout: renox-billing's `Billing::of(…).named(…).checkout(…)`",
                    region: "plans.subscribe.checkout",
                },
            ],
        },
        Explanation {
            route: "plans.mine",
            path: "/plans/mine",
            title: "My plans",
            purpose: "Every bike's plan: its state (running, paused, on hold after a failed \
                      payment, ending at the period's end), the next visit, the visits done, \
                      the last invoice, and the plans that ended.",
            who: "Logged-in customers.",
            audience: &[Audience::Customer],
            flow: Flow::Account,
            features: &[
                Feature {
                    api: "Query::group_by + select_as",
                    why: "The visits done per plan are one grouped query \
                          (`COUNT(*) … GROUP BY plan_subscription_id`), not a count per plan, \
                          so the page costs the same for one bike or ten.",
                },
                Feature {
                    api: "Billing::redirect_to",
                    why: "renox-billing sends customers back here after paying \
                          (`Billing::new().redirect_to(\"/plans/mine\")`, through its \
                          `billing.return` route), with its \"payment is being confirmed\" \
                          message as a toast: the plan shows as pending until the webhook \
                          arrives.",
                },
                Feature {
                    api: "UI kit: card + infolist",
                    why: "Each plan is a kit card with an infolist (next visit, visits done, \
                          last invoice) and a status `badge` whose word and colour agree; \
                          no plan yet shows the kit's `empty` state with a link to the \
                          plans.",
                },
            ],
            under_hood: "Seven queries whatever the number of plans: the customer, their \
                         bikes, the plans of those bikes, the service plans, the stores, the \
                         visits done (grouped) and the invoices. Nothing is written.",
            docs: &[
                "docs/billing.md#what-your-users-see",
                "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
                "docs/ui.md#infolists-read-only-details",
            ],
            sources: &[
                MINE,
                MODEL,
                "examples/bikeshop/resources/views/plans/mine.html",
                "examples/bikeshop/resources/views/components/plans_status.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: the customer's bikes and their subscriptions",
                    region: "plans.mine.handler",
                },
                Code {
                    title: "Query: visits done per plan with `group_by` and `select_as`",
                    region: "plans.mine.query",
                },
                Code {
                    title: "Template: a CSS grid of cards with the kit's `infolist`",
                    region: "plans.mine.template",
                },
            ],
        },
        Explanation {
            route: "plans.show",
            path: "/plans/mine/{subscription}",
            title: "A plan and its visits",
            purpose: "One bike's plan: the upcoming visits on a month calendar and as a \
                      list, each with \"Skip\" and \"Move\" (to another day with room), the \
                      past visits (done, skipped, missed), the invoices, and the changes: \
                      another plan from the next period, pause, cancel at the period's end, \
                      resume.",
            who: "The bike's owner (anyone else gets a 404).",
            audience: &[Audience::Customer],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "Customer::swap / cancel / resume",
                    why: "Money changes go through renox-billing: `swap` (without \
                          proration, so the new price starts with the next invoice), `cancel` \
                          (at the period's end: visits go on until then) and `resume` \
                          (offered only when `can_resume` says the gateway can: Stripe and \
                          the demo can, Xendit stops charging at once). The gateway answers, \
                          renox-billing stores it and emits `SubscriptionUpdated`, which the \
                          shop mirrors into the plan. Pausing is the shop's own: the gateway \
                          keeps charging, so the plan keeps its price.",
                },
                Feature {
                    api: "Customer::subscribed (the require_subscription check)",
                    why: "Skipping or moving a visit needs the plan paid up. renox-billing's \
                          `require_subscription` guard checks a user's `default` \
                          subscription; here each bike has its own, so the visit routes ask \
                          `Billing::of(…).named(\"bike-…\").subscribed()` and send the \
                          customer back with a message while a payment is due, as the guard \
                          would.",
                },
                Feature {
                    api: "Schedule::daily_at",
                    why: "The visits on this page were made by `plans:visits`, a task that \
                          runs every morning at 06:00 and books the next week's visits as \
                          work orders; the same run records missed visits and ends plans \
                          whose last day passed. Booking only a week ahead means a plan \
                          change, a pause or a hold takes effect at the next run, with few \
                          work orders to undo.",
                },
                Feature {
                    api: "workshop::capacity (move_booking)",
                    why: "Moving a visit is the workshop's own rescheduling: one \
                          transaction that takes the store's row, checks the day has room \
                          for the visit's minutes, and moves it, so a plan can't overbook \
                          the workshop.",
                },
                Feature {
                    api: "Bike shop blocks",
                    why: "The kit has no calendar of events, so the `month_calendar` \
                          block of renox-blocks shows the upcoming visits on a month grid that turns into a \
                          list on a phone; \"Move\" uses the kit's `date_picker` with \
                          `disabled_dates` and `closed_weekdays`: the full and closed days are \
                          greyed out (the server checks again).",
                },
                Feature {
                    api: "UI kit: action_sheet",
                    why: "\"Move\" and \"Change plan\" open a sheet with their form (a 422 \
                          stays in the sheet, a success reloads the page with a toast); \
                          \"Skip\" and \"Cancel\" ask first with the kit's `confirm`.",
                },
            ],
            under_hood: "Loads the plan, its bike, the service plan, the store, the last 40 \
                         visits and their work orders, the invoices, the renox-billing \
                         subscription (`subscriptions`), the plan's tasks and the store's \
                         full days (for the date picker). Skip cancels the visit's work \
                         order (its minutes free up); move runs `capacity::move_booking`; \
                         pause cancels the upcoming work orders and makes none until resumed. \
                         A plan paid at the counter changes and ends in the shop alone. A \
                         plan paid through the demo gateway can be renewed from here, paid \
                         or failed (`plans.demo.renew` queues the webhook), to see the \
                         renewal and payment-failed mails and the hold.",
            docs: &[
                "docs/billing.md#changing-plan-canceling-resuming",
                "docs/billing.md#pages-for-subscribers-only",
                "docs/scheduling.md#scheduled-tasks",
                "docs/ui.md#actions",
            ],
            sources: &[
                MINE,
                VISITS,
                TASKS,
                CAPACITY,
                DEMO,
                "examples/bikeshop/resources/views/plans/show.html",
                "crates/renox-blocks/views/blocks.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Template: the `month_calendar` block, Move in a sheet, Skip with `confirm`",
                    region: "plans.show.template",
                },
                Code {
                    title: "Billing: the gateway's side, read through renox-billing",
                    region: "plans.show.billing",
                },
                Code {
                    title: "Swap: renox-billing's `swap`, or the next visit for counter plans",
                    region: "plans.show.swap",
                },
            ],
        },
        Explanation {
            route: "plans.mails",
            path: "/plans/mails",
            title: "Plan mails",
            purpose: "Every mail a plan sends, previewed for a made-up subscriber: the plan \
                      started (its first payment), renewed (each payment after), a payment \
                      failed (the plan is on hold until paid), a visit booked, a visit \
                      missed, each with what sends it.",
            who: "Anyone reading the example; the mails themselves go to customers.",
            audience: &[Audience::Developer, Audience::Customer],
            flow: Flow::Learn,
            features: &[
                Feature {
                    api: "Events and listeners",
                    why: "The billing mails are sent by the shop's listeners on \
                          renox-billing's `PaymentSucceeded` and `PaymentFailed` events \
                          (`app.listen` in the plans module's `register`): a payment the \
                          gateway reported by webhook becomes a `plan_invoices` row and one \
                          of these mails. renox-billing knows nothing of bikes or visits; \
                          the listeners keep that knowledge in the shop.",
                },
                Feature {
                    api: "Notification (mail + database)",
                    why: "Each message is a `Notification` sent to both channels: a mail in \
                          the recipient's language and a row the kit's notification bell \
                          shows. The visit mails come the same way from the `plans:visits` \
                          task.",
                },
                Feature {
                    api: "AppState::mail_view_in",
                    why: "The previews render the real mail view in the visitor's language, \
                          without sending anything, each in a sandboxed frame as a mail \
                          client would show it; the page can't drift from what customers \
                          get.",
                },
            ],
            under_hood: "Renders five mails from `mail/plans/notice.html` with made-up values; \
                         no query, nothing sent.",
            docs: &[
                "docs/billing.md#events-and-the-activity-log",
                "docs/mail.md#notifications",
                "docs/mail.md#localized-mail",
            ],
            sources: &[
                MAILS,
                SYNC,
                VISITS,
                "examples/bikeshop/src/app/rentals/notify.rs",
                "examples/bikeshop/resources/views/mail/plans/notice.html",
                "examples/bikeshop/resources/views/plans/mails.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: each mail rendered with `mail_view_in`, as it is sent",
                    region: "plans.mails.handler",
                },
                Code {
                    title: "Template: every mail in a sandboxed frame",
                    region: "plans.mails.template",
                },
            ],
        },
        Explanation {
            route: "plans.demo",
            path: "/plans/demo-pay/{user}/{name}/{plan}",
            title: "Demo payment page",
            purpose: "Stands in for Stripe's or Xendit's payment page when the shop runs \
                      without their keys, so a plan can be subscribed to end to end on a \
                      laptop: pay, or decline to see a failed payment.",
            who: "A customer subscribing in the demo (never in production).",
            audience: &[Audience::Customer, Audience::Developer],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "renox_billing::Gateway",
                    why: "The demo is one impl of renox-billing's `Gateway` trait (a \
                          customer, a checkout, a swap, a cancel, a resume, its webhook's \
                          check and reading): the module treats it like Stripe, so nothing \
                          in the shop is special-cased for the demo.",
                },
                Feature {
                    api: "Signed URLs",
                    why: "The page's link is a signed URL made by the gateway's `checkout` \
                          (`state.signed_url(\"plans.demo\", …)`); `ValidSignature` checks \
                          it on the page and on its form: only its holder can pay, and it \
                          expires in an hour. No session or login is needed, as with a real \
                          gateway's page.",
                },
                Feature {
                    api: "Queue",
                    why: "Paying queues `DemoBillingNotify` instead of applying the payment \
                          at once: the webhook arrives a moment later, as a real gateway's \
                          does, and a failed delivery is retried (five attempts).",
                },
                Feature {
                    api: "renox::http",
                    why: "The job posts the webhook to this app's own \
                          `/billing/webhooks/demo` through `state.http`, signed with an \
                          HMAC of a key derived from `APP_KEY`, so it goes through \
                          renox-billing's real verification.",
                },
            ],
            under_hood: "GET reads the service plan and the bike. POST (a 404 when the demo \
                         gateway is off) queues the webhook and redirects to renox-billing's \
                         `billing.return`, which sends the customer to their plans; the \
                         webhook is verified, stored once and applied by a queue worker a \
                         moment later.",
            docs: &[
                "docs/billing.md#another-payment-provider",
                "docs/routing.md#signed-urls",
                "docs/queue.md#a-job",
            ],
            sources: &[
                DEMO,
                BILLING,
                "examples/bikeshop/resources/views/plans/demo.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Gateway: the demo's checkout is a signed link to this page",
                    region: "plans.demo.checkout",
                },
                Code {
                    title: "Handler: `ValidSignature`, then the webhook queued as a job",
                    region: "plans.demo.complete",
                },
                Code {
                    title: "Job: the webhook posted, signed, to the app's own endpoint",
                    region: "plans.demo.job",
                },
            ],
        },
        Explanation {
            route: "billing.plans",
            path: "/billing",
            title: "Billing plans (from renox-billing)",
            purpose: "renox-billing's own plans page, which subscribes a user's `default` \
                      subscription. The shop sells plans per bike, so its template is \
                      replaced by the app (`resources/views/billing/plans.html`) with the \
                      plans and a link to subscribe a bike. renox-billing's own links (and \
                      its `require_subscription` guard, wherever a page uses it) lead here.",
            who: "Customers who land here from renox-billing's links.",
            audience: &[Audience::Customer],
            flow: Flow::Service,
            features: &[Feature {
                api: "renox-billing views (overridable)",
                why: "A plugin's template is used unless the app has a file of the same \
                      name: `billing/plans.html` and `billing/section.html` (the card on \
                      `/account`) are the shop's own, pointing at the per-bike plans. The \
                      route and its handler stay renox-billing's, so nothing is forked.",
            }],
            under_hood: "renox-billing's handler reads the plans it sells and the user's \
                         `default` subscription (none here), and renders the shop's template: \
                         the card plans (the `-xendit` copies left out), each linking to \
                         `/plans/subscribe?plan=…`.",
            docs: &["docs/billing.md#changing-the-pages"],
            sources: &[
                BILLING,
                "crates/renox-billing/src/handlers.rs",
                "examples/bikeshop/resources/views/billing/plans.html",
                "examples/bikeshop/resources/views/billing/section.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Module: each plan given to renox-billing in USD and in IDR",
                    region: "billing.plans.module",
                },
                Code {
                    title: "Template: the plugin's page replaced by a file of the same name",
                    region: "billing.plans.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![NotAPage {
        route: "billing.return",
        reason: "renox-billing's return from the gateway: a redirect to /plans/mine with a toast",
    }]
}
