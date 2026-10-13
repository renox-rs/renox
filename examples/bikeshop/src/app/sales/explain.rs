//! "About this page" entries for the sales area's pages (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "cart.show",
            path: "/cart",
            title: "Cart",
            purpose: "What the shopper is about to buy, checked against the stock of one store: \
                  the store they pick up from (or that sends the delivery). Quantities can be \
                  changed or lines removed without a reload; then on to the checkout.",
            who: "Shoppers, logged in or not. A guest's cart follows them into their account \
              when they log in.",
            audience: &[Audience::Visitor, Audience::Customer, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Sessions",
                    why: "A guest's cart is a small value in the session \
                      (`session.put(\"cart\", …)`): no table rows for people who never buy. \
                      A logged-in customer's cart is a row in `carts`, so it is there on \
                      their phone and their laptop; the first request after logging in \
                      merges the session's cart into it (`Cart::load`).",
                },
                Feature {
                    api: "View::also",
                    why: "A change answers the cart's `lines` block plus the navbar's `mini` \
                      block (`.fragment(\"lines\").also(\"mini\")`); the second one has \
                      `hx-swap-oob`, so htmx puts it into the navbar's cart link while the \
                      first replaces the cart: one answer, two places updated, and no second \
                      request just to refresh the count.",
                },
                Feature {
                    api: "Toast",
                    why: "Every change says what happened in a toast (\"The store has fewer: \
                      the quantity was lowered.\"), so a quantity that silently went down \
                      never surprises the shopper. Renox sends it in the `HX-Trigger` header \
                      for htmx, or keeps it in the session across a plain form's redirect.",
                },
                Feature {
                    api: "renox::db::relations",
                    why: "The stores, the lines' variants, their products, categories and \
                      photos, and the store's stock are read in six queries however long the \
                      cart is (`find_many`, `has_many`), instead of a few queries per line.",
                },
                Feature {
                    api: "renox-blocks",
                    why: "The quantity of each line is the `quantity` block (− / number / +), \
                      big enough to tap on a phone; each step sends `change`, which htmx turns \
                      into a `PATCH` after 300 ms (`hx-trigger=\"change delay:300ms\"`), so \
                      three quick taps are one request.",
                },
                Feature {
                    api: "Method spoofing",
                    why: "Without JavaScript the quantity and remove forms are `POST`s carrying \
                      `_method=PATCH` / `DELETE` (`method_field`), routed to the same handlers \
                      as the htmx requests: one set of routes for both.",
                },
            ],
            under_hood: "Each request loads the cart (session or `carts`), then the variants and \
                     the chosen store's `stock_levels` (available = on hand − reserved, over \
                     every owner of the goods there). A line asking for more than the store \
                     has is lowered to what's left, and the page says so; a discontinued \
                     product's line goes. Changing the store checks every line against the \
                     new store at once. Nothing is reserved yet: that happens when the order \
                     is placed, in a transaction (the checkout).",
            docs: &[
                "docs/routing.md#sessions",
                "docs/ui.md#fragments-and-out-of-band-swaps",
                "docs/ui.md#toasts",
                "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
                "docs/routing.md#method-spoofing",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/cart.rs",
                "examples/bikeshop/resources/views/sales/cart/show.html",
                "examples/bikeshop/resources/views/sales/cart/_mini.html",
                "examples/bikeshop/resources/views/components/sales_cart_mini.html",
                "examples/bikeshop/resources/views/components/nav_cart.html",
                "crates/renox-blocks/views/blocks.html",
                "examples/bikeshop/migrations/20260102000700_create_carts_table.up.sql",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Model: a guest's cart in the session, a customer's in `carts`",
                    region: "cart.show.load",
                },
                Code {
                    title: "Handler: a change answers the `lines` block plus the count out of band",
                    region: "cart.show.htmx",
                },
                Code {
                    title: "Template: `hx-patch` on change, the `mini` block for `.also()`",
                    region: "cart.show.template",
                },
            ],
        },
        Explanation {
            route: "checkout.show",
            path: "/checkout",
            title: "Checkout",
            purpose: "Where a cart becomes an order: who the customer is, pickup at a store or \
                      delivery (with a fee by city), a last look, then the payment page. \
                      Placing the order puts the stock aside, so it is still there when the \
                      payment arrives.",
            who: "Shoppers, with an account or as guests (they are offered one afterwards).",
            audience: &[Audience::Visitor, Audience::Customer, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "UI kit: wizard",
                    why: "Contact → pickup or delivery → review is the kit's `wizard`: one \
                          form in steps, so a phone shows a few fields at a time but the \
                          server gets (and checks) everything in one submit. \"Next\" checks \
                          the step first, the first step with an error opens after a failed \
                          submit, and without JavaScript all steps show at once. \
                          `toggle_buttons` and `show_when` hide the address for a pickup \
                          (hidden fields are disabled, so not sent), and `required_if` asks \
                          for it only for a delivery.",
                },
                Feature {
                    api: "#[derive(Validate)]",
                    why: "`CheckoutForm`'s rules are attributes next to its fields \
                          (`required`, `email`, `one_of`, `exists(\"stores\", \"id\")`, \
                          `required_if(self.fulfilment == \"delivery\")`); \
                          `#[validate(hooks)]` adds `prepare` (tidy the name, trim and \
                          lowercase the email) and `after` (the phone is digits, spaces, \
                          dashes and a leading `+`). One place for the rules, used by the \
                          submit and the live checks alike.",
                },
                Feature {
                    api: "Live validation",
                    why: "`data-live-validate` on the form: leaving a field asks the server \
                          with `X-Renox-Validate`, and `Valid<T>` answers that field's errors \
                          from the same rules without running the handler. Nothing is \
                          written twice, and no JavaScript copy of the rules can drift.",
                },
                Feature {
                    api: "Valid<T>",
                    why: "A failed plain submit goes back with the errors and the old input \
                          (the kit's fields read `old()`), so nothing typed is lost; htmx \
                          gets a 422 with the errors placed next to the fields.",
                },
                Feature {
                    api: "Transactions",
                    why: "The order, the reservation of every line and the order's lines are \
                          one transaction. Each reservation is a conditional update \
                          (`WHERE on_hand - reserved >= ?`): when two customers buy the last \
                          helmet at once, the database runs the updates one after the other \
                          and the second changes no row, so it rolls back and that customer is \
                          told, while the first pays. Checking first and writing later would \
                          sell the helmet twice. `tests/sales.rs` races two checkouts.",
                },
                Feature {
                    api: "htmx fragments",
                    why: "Choosing delivery or a city asks for the `summary` block again \
                          (`hx-get` on the same page, `hx-include` of the two fields), with \
                          the city's delivery fee: the same handler and template as the \
                          page, so the total can't be worked out two ways.",
                },
            ],
            under_hood: "The page reads the cart (lowering lines a store can't fill; an empty \
                         cart goes back to `/cart`), the customer, their address and the parts \
                         discount of their service plan, if they subscribe to one. Placing the \
                         order: `Valid<CheckoutForm>`, the customer row (found by email or made \
                         for a guest), a delivery address, the stock levels read, then one \
                         transaction (order `pending`, `stock_movements` reason `reserved` per \
                         line, `stock_levels.reserved` up, `order_items` with the owner store \
                         of each unit), then `payments::start` (a pending `payments` row and \
                         the gateway's page), the cart emptied, the order and payment ids \
                         kept in the session, and a redirect to the gateway. An order left \
                         unpaid is cancelled after 30 minutes by the scheduled task \
                         `sales:expire-orders`.",
            docs: &[
                "docs/ui.md#form-fields",
                "docs/ui.md#a-field-that-depends-on-another",
                "docs/validation.md#derivevalidate",
                "docs/validation.md#hooks-with-the-derive",
                "docs/validation.md#live-validation",
                "docs/relations.md#more-of-the-query-builder",
                "docs/ui.md#fragments-and-out-of-band-swaps",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/checkout.rs",
                "examples/bikeshop/src/app/sales/ledger.rs",
                "examples/bikeshop/src/app/sales/payments.rs",
                "examples/bikeshop/resources/views/sales/checkout/show.html",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Form: `#[derive(Validate)]` rules, `required_if` for delivery",
                    region: "checkout.show.form",
                },
                Code {
                    title: "Handler: the order and its reservation in one transaction",
                    region: "checkout.show.place",
                },
                Code {
                    title: "Template: the kit's `<rx-wizard>`, checked live, the summary asked again",
                    region: "checkout.show.template",
                },
            ],
        },
        Explanation {
            route: "pay.show",
            path: "/pay/{payment}",
            title: "Payment status",
            purpose: "Where the customer lands after paying: it waits for the payment \
                      provider's word (the webhook), then says paid (with the order and, for \
                      a guest, an offer to create an account) or not.",
            who: "Customers who just paid online, for an order, a rental or a workshop job.",
            audience: &[Audience::Visitor, Audience::Customer, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "renox::webhook",
                    why: "The payment is marked paid by Midtrans' webhook, never by this \
                          page, since anyone can open a URL that says \"paid\": \
                          `POST /webhooks/midtrans` checks the SHA-512 signature (a 401 \
                          otherwise), stores each event once (keyed by the transaction id and \
                          status, so a notification sent twice is handled once), and runs the \
                          handler in a queue worker with retries. An amount that doesn't \
                          match the payment is a permanent failure; `webhook:failed` lists \
                          what failed and `webhook:retry` runs it again.",
                },
                Feature {
                    api: "Events and listeners",
                    why: "The handler only marks the payment paid and emits \
                          `PaymentSucceeded`; the sales area's listener makes the order paid \
                          in a transaction (the reservation becomes a sale), registers the \
                          bikes and queues the confirmation mail. Rentals and the workshop \
                          listen to the same event, so the payment code knows none of them.",
                },
                Feature {
                    api: "htmx polling",
                    why: "While the payment is pending the status block asks again every two \
                          seconds (`hx-trigger=\"every 2s\"`, the `status` fragment) and stops \
                          once it is paid, failed or expired: simpler than a live stream for a \
                          wait of a few seconds.",
                },
                Feature {
                    api: "Schedule::every_minute",
                    why: "`sales:expire-orders` runs every minute: an order still unpaid after \
                          30 minutes is cancelled, its pending payment failed, its reservation \
                          released, and the customer mailed. Without it, an abandoned payment \
                          would hold the stock forever.",
                },
                Feature {
                    api: "analytics::event",
                    why: "When the page first sees the order paid it records a GA4 \
                          `purchase` event (number, value, currency) in the session, once per \
                          order; Renox delivers it with this answer (the page, or the poll's \
                          `HX-Trigger`). Google's tags load only in production with \
                          `GA4_MEASUREMENT_ID` set.",
                },
            ],
            under_hood: "Who may see it: the browser that started the payment (its id is in \
                         the session), someone coming back from the gateway with the page's \
                         token (an HMAC of the id with `APP_KEY`, which survives the query \
                         parameters Midtrans adds), or the paying customer's account; anyone \
                         else gets a 404. Reads the `payments` row and the order it pays; \
                         while it is pending, htmx asks for the `status` block every two \
                         seconds.",
            docs: &[
                "docs/operations.md#failed-webhook-calls",
                "docs/queue.md#failures-and-retries",
                "docs/scheduling.md#scheduled-tasks",
                "docs/scheduling.md#events",
                "docs/operations.md#all-settings",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/gateway.rs",
                "examples/bikeshop/src/app/sales/payments.rs",
                "examples/bikeshop/src/app/sales/orders.rs",
                "examples/bikeshop/resources/views/sales/pay/show.html",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Handler: the payment this visitor started, reported to analytics once",
                    region: "pay.show.handler",
                },
                Code {
                    title: "Template: htmx asks for the `status` block every two seconds",
                    region: "pay.show.template",
                },
                Code {
                    title: "Webhook: `renox::webhook` checks the signature, then marks it paid",
                    region: "pay.show.webhook",
                },
            ],
        },
        Explanation {
            route: "pay.demo",
            path: "/pay/demo/{payment}",
            title: "Demo payment page",
            purpose: "Stands in for Midtrans' hosted payment page when `MIDTRANS_SERVER_KEY` \
                      isn't set, so the whole buying flow can be followed on a laptop. \"Pay\" \
                      (or \"Cancel\") sends the app exactly what Midtrans would: a signed \
                      notification to the webhook. It takes the shop's dollars; the real \
                      Midtrans only charges rupiah, so a shop using it runs with \
                      `APP_CURRENCY=IDR`.",
            who: "Developers and anyone trying the demo; never real customers (with Midtrans \
                  configured no link to it is given, and paying on it answers 404).",
            audience: &[Audience::Visitor, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Signed URLs",
                    why: "The page and its form only work through the signed link \
                          `payments::start` gave (`state.signed_url`, an hour): someone \
                          guessing another payment's id gets a 403, with no table of tokens \
                          to keep.",
                },
                Feature {
                    api: "Queue",
                    why: "Paying queues `DemoNotify`, which POSTs Midtrans' notification \
                          (signed with a key derived from `APP_KEY`) to this app's \
                          `/webhooks/midtrans`, a moment later, as the real provider does. \
                          The webhook can't tell the difference, so the demo runs the real \
                          code path; a failed send is retried (five attempts).",
                },
                Feature {
                    api: "renox::http",
                    why: "`DemoNotify` sends through `state.http`, the same client that asks \
                          Midtrans' Snap API for the real page (`hosted_page`); tests answer \
                          Midtrans with `FakeHttp`, so no test needs the network.",
                },
            ],
            under_hood: "Reads the payment and its order. The form posts to the same signed \
                         address; the handler queues the notification (only while the payment \
                         is pending) and sends the customer to `/pay/{payment}?token=…`, \
                         which waits for the webhook.",
            docs: &[
                "docs/routing.md#signed-urls",
                "docs/queue.md#a-job",
                "docs/testing.md#jobs-events-notifications-mail-http",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/gateway.rs",
                "examples/bikeshop/src/app/sales/payments.rs",
                "examples/bikeshop/resources/views/sales/pay/demo.html",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Handler: only the signed link opens it; paying queues the notification",
                    region: "pay.demo.handler",
                },
                Code {
                    title: "Job: the notification Midtrans would send, posted to our own webhook",
                    region: "pay.demo.job",
                },
                Code {
                    title: "Template: two forms posting to the same signed address",
                    region: "pay.demo.template",
                },
            ],
        },
        Explanation {
            route: "orders.show",
            path: "/orders/{order}",
            title: "Order",
            purpose: "The customer's view of one order: where it stands (placed, paid, ready \
                      or sent, handed over, maybe returned), its lines and totals, payments, \
                      the store and the return window.",
            who: "The customer who placed it (an account, the browser that ordered, or the \
                  signed link in their mails); staff of its store.",
            audience: &[
                Audience::Visitor,
                Audience::Customer,
                Audience::Staff,
                Audience::Developer,
            ],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Signed URLs",
                    why: "Every mail links to `/orders/{id}/view?expires=…&signature=…` \
                          (30 days): a guest without an account opens their order, and the \
                          browser remembers it, without a password or a stored token.",
                },
                Feature {
                    api: "access policy helpers",
                    why: "Staff see it when they may see orders in its store \
                          (`access::can_see`, the operating store); anyone else who isn't \
                          its customer gets a 404, not a 403, so order ids can't be probed.",
                },
                Feature {
                    api: "UI kit: infolist + entry",
                    why: "The order's details are an `infolist`; the lines a kit `table`; \
                          the progress the `history` block (a timeline with icons, not \
                          colour alone), so the page needs no CSS of its own for them.",
                },
                Feature {
                    api: "money filter",
                    why: "Every amount is an integer in the smallest unit (no float rounding), \
                          written with `{{ amount | money }}` in `APP_CURRENCY` and the \
                          visitor's language.",
                },
            ],
            under_hood: "Reads the order, its lines, their variants and products \
                         (discontinued ones too, `with_trashed`), the store, the customer, \
                         the delivery address, then its payments: a handful of queries \
                         however many lines.",
            docs: &[
                "docs/routing.md#signed-urls",
                "docs/ui.md#infolists-read-only-details",
                "docs/types.md#money",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/orders.rs",
                "examples/bikeshop/src/app/sales/notify.rs",
                "examples/bikeshop/resources/views/sales/orders/show.html",
                "crates/renox-blocks/views/blocks.html",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Handler: another customer's order is a 404, not a 403",
                    region: "orders.show.handler",
                },
                Code {
                    title: "Template: the page header's badge and the `history` block",
                    region: "orders.show.template",
                },
                Code {
                    title: "Listener: `PaymentSucceeded` marks the order paid, exactly once",
                    region: "orders.show.paid",
                },
            ],
        },
        Explanation {
            route: "orders.invoice",
            path: "/orders/{order}/invoice",
            title: "Invoice and receipt",
            purpose: "The order as a printable invoice, or the counter's receipt right after a \
                      sale (with the change to give).",
            who: "Customers who want a copy; cashiers printing a receipt.",
            audience: &[Audience::Customer, Audience::Cashier, Audience::Developer],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "Print styles",
                    why: "`@media print` in `public/sales/sales.css` hides the navigation, \
                          the buttons and the toasts, so the browser's print gives a clean \
                          page without a PDF library; \"Print\" (or the R key, `data-rx-key`) \
                          calls `window.print()`. On a counter receipt, N starts a new sale.",
                },
                Feature {
                    api: "Session flash",
                    why: "The change to give after a cash sale is flashed by the counter \
                          (`session.flash(\"change\", …)`) and shown once, here: a reload \
                          doesn't show it again, and nothing is stored for it.",
                },
            ],
            under_hood: "The same order data and access rules as the order page.",
            docs: &["docs/routing.md#sessions", "docs/ui.md#actions"],
            sources: &[
                "examples/bikeshop/src/app/sales/orders.rs",
                "examples/bikeshop/resources/views/sales/orders/invoice.html",
                "examples/bikeshop/public/sales/sales.css",
                "examples/bikeshop/public/sales/sales.js",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Handler: the same visibility check, and the signed link from the mails",
                    region: "orders.invoice.handler",
                },
                Code {
                    title: "Template: a print button and the kit's `<rx-table>`",
                    region: "orders.invoice.template",
                },
                Code {
                    title: "CSS: print styles hide everything but the invoice",
                    region: "orders.invoice.print",
                },
            ],
        },
        Explanation {
            route: "sales.orders.index",
            path: "/staff/orders",
            title: "Orders (staff)",
            purpose: "The orders of the store the person works in today, by status: open \
                      (paid or ready, still to hand over), unpaid, ready, completed, \
                      cancelled, refunded.",
            who: "Cashiers, store staff and managers of a store; the owner, in the store \
                  they switched to.",
            audience: &[
                Audience::Staff,
                Audience::Cashier,
                Audience::Manager,
                Audience::Owner,
            ],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Routes::require_permission",
                    why: "`access::staff_routes` around `require_permission(\"orders.view\")`: \
                          a login, `staff.access` in some store, then the permission in the \
                          active store (roles given per store, #244). The guard sits on the \
                          routes, so no handler can forget it.",
                },
                Feature {
                    api: "renox::context",
                    why: "The active store (picked by the store switcher, checked against \
                          today's roles) is in `renox::context`; `active_store::current()` \
                          scopes the list to it, and `access::visible::<Order>` \
                          (`scopes_with`) keeps only orders the user may see at all, without \
                          passing the store through every function.",
                },
                Feature {
                    api: "UI kit: link_tabs + table",
                    why: "The status filter is the kit's `link_tabs` (links, so each tab has \
                          an address to bookmark or share); the orders a `table` with \
                          `badge`s; pagination below.",
                },
            ],
            under_hood: "Three queries for a page of 25: the count, the page, the customers \
                         (`belongs_to`).",
            docs: &[
                "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
                "docs/authorization.md#lists-scopes_with",
                "docs/ui.md#navigation-and-page-structure",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/staff.rs",
                "examples/bikeshop/resources/views/sales/staff/index.html",
                "examples/bikeshop/src/app/access/policy.rs",
                "examples/bikeshop/src/app/access/active_store.rs",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Handler: `access::visible` scopes the query, `belongs_to` loads customers",
                    region: "sales.orders.index.handler",
                },
                Code {
                    title: "Template: `<rx-link-tabs>` by status and the kit's `<rx-table>`",
                    region: "sales.orders.index.template",
                },
            ],
        },
        Explanation {
            route: "sales.orders.show",
            path: "/staff/orders/{order}",
            title: "Order (staff)",
            purpose: "One order with everything staff need: lines (with the owner store of \
                      consigned goods), payments, the stock movements it caused, the bikes it \
                      registered, and the next step: ready or sent out, hand over (with frame \
                      numbers), cancel an unpaid one, or take a return and refund it.",
            who: "Cashiers and floor staff, who hold `orders.sell` in the order's store: \
                  they mark it ready or sent out, hand it over and cancel an unpaid one. \
                  Store managers also hold `orders.refund` for returns and refunds; the \
                  owner does all of it in any store.",
            audience: &[
                Audience::Cashier,
                Audience::Staff,
                Audience::Manager,
                Audience::Owner,
            ],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "access policy helpers",
                    why: "`access::find::<Order>` answers 404 for another store's order; each \
                          action is `access::require(user, permission, StoreAttr::Operating, \
                          &order)`: `orders.sell` to move it on or cancel it, `orders.refund` \
                          for a return, both in the store that served the customer, which a \
                          plain role check couldn't tell.",
                },
                Feature {
                    api: "UI kit: action_sheet",
                    why: "\"Hand over\" and \"Return and refund\" are `action_sheet`s: a form \
                          in a sheet, sent with htmx, so the order stays in view; a 422 stays \
                          in the sheet, success reloads the page with a toast (`HxRefresh`). \
                          Cancelling asks first in a `confirm`.",
                },
                Feature {
                    api: "Stock ledger",
                    why: "A return writes a `return` movement per line (the goods are back at \
                          the store, still their owner's), reverses the books between stores \
                          for consigned goods, and records a refund in `payments`, in one \
                          transaction, so stock and money can't disagree.",
                },
                Feature {
                    api: "notify",
                    why: "Ready, sent out and refunded each queue a mail in the language the \
                          customer ordered in and, for a customer with an account, add a \
                          `DatabaseMessage` to their notifications.",
                },
            ],
            under_hood: "Reads the order (`OrderView`), its payments, its `stock_movements` \
                         and the `customer_bikes` it registered. Status changes are conditional \
                         updates (`WHERE status = 'paid'`), so two clicks change it once; the \
                         handover writes the frame numbers on the order's bikes and starts the \
                         14-day return window.",
            docs: &[
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/ui.md#actions",
                "docs/mail.md#database-notifications",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/staff.rs",
                "examples/bikeshop/src/app/sales/ledger.rs",
                "examples/bikeshop/src/app/sales/notify.rs",
                "examples/bikeshop/resources/views/sales/staff/show.html",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Handler: `access::find`, then each action checked in the order's store",
                    region: "sales.orders.show.handler",
                },
                Code {
                    title: "Template: `<rx-action-sheet>` and `<rx-confirm>` only where the user may act",
                    region: "sales.orders.show.template",
                },
            ],
        },
        Explanation {
            route: "sales.counter",
            path: "/staff/counter",
            title: "Counter",
            purpose: "Selling over the counter: find a product by SKU, barcode text or name, \
                      ring it up, attach a customer or sell to a walk-in, take cash (with the \
                      change worked out) or card, print the receipt. Same stock and payment \
                      rules as the web shop.",
            who: "Cashiers at a store's counter.",
            audience: &[Audience::Cashier, Audience::Manager],
            flow: Flow::Buy,
            features: &[
                Feature {
                    api: "renox::select",
                    why: "The product and customer pickers are the kit's searchable `select` \
                          with `options_url`: they ask `/staff/counter/variants?q=` (each \
                          option with its price and this store's stock) and \
                          `/staff/counter/customers?q=` for `SelectOption`s as the cashier \
                          types, instead of sending every product and customer with the page.",
                },
                Feature {
                    api: "renox::db::search",
                    why: "The product search puts an exact SKU first (what a barcode scanner \
                          types), then `Product::search`'s full-text matches on the name, so \
                          one box serves the scanner and the cashier's typing.",
                },
                Feature {
                    api: "Keyboard shortcuts (data-rx-key)",
                    why: "A: add, P: take the payment, X: start again, and on the receipt R: \
                          print, N: new sale (`button(key=…)`, `data-rx-key`), so a cashier \
                          rarely needs the mouse.",
                },
                Feature {
                    api: "renox::context",
                    why: "The sale belongs to the active store (`active_store::current()`); \
                          the stock checked, the order's operating store and the payment's \
                          store are all that one, with no store id in the forms to tamper \
                          with.",
                },
                Feature {
                    api: "renox-blocks",
                    why: "The `keypad` block types the amount received on a touch screen \
                          (and works from a keyboard too); `quantity` changes a line.",
                },
                Feature {
                    api: "Transactions",
                    why: "Paying reserves the units in a transaction exactly as the checkout \
                          does (`ledger::reserve`), so a counter sale and a web order can't \
                          both take the last helmet; `payments::record_counter` then records \
                          the cash or card payment, whose `PaymentSucceeded` turns the \
                          reservation into a sale.",
                },
            ],
            under_hood: "The sale being rung up is in the session (`counter:{store}`), like a \
                         cart; the forms are boosted by htmx, so nothing reloads, and work as \
                         plain forms without JavaScript. Paying: cash less than the total is \
                         refused, then the order (`counter`, served by this staff member), \
                         the reservation and the lines in one transaction, then \
                         `record_counter` (the listener makes it paid, registers the bikes of \
                         a known customer and queues their confirmation mail), the order \
                         completed at once, the change flashed, and the receipt.",
            docs: &[
                "docs/ui.md#options-from-the-server",
                "docs/search.md#3-searching",
                "docs/ui.md#what-every-button-can-carry",
                "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/counter.rs",
                "examples/bikeshop/src/app/sales/payments.rs",
                "examples/bikeshop/resources/views/sales/counter/show.html",
                "crates/renox-blocks/views/blocks.html",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Handler: the sale rung up lives in the session, per store",
                    region: "sales.counter.handler",
                },
                Code {
                    title: "Pay: change worked out, paid at once, receipt next",
                    region: "sales.counter.pay",
                },
                Code {
                    title: "Template: a searchable `<rx-select>` from the server and the `keypad`",
                    region: "sales.counter.template",
                },
            ],
        },
        Explanation {
            route: "sales.mails",
            path: "/sales/mails",
            title: "Order mails",
            purpose: "Every mail a customer gets about an order (confirmation, ready for \
                      pickup, sent out, refund, cancelled), rendered for a made-up order.",
            who: "Developers and the shop owner checking what customers receive.",
            audience: &[Audience::Owner, Audience::Developer],
            flow: Flow::Learn,
            features: &[
                Feature {
                    api: "Mail",
                    why: "Each mail is `state.mail_view_in(locale, to, subject, \
                          \"mail/sales/…\", ctx)` on Renox's mail layout with its components \
                          (`table`, `panel`, `button`), styled inline for mail clients; the \
                          plain-text part is made from the HTML, so there is one template per \
                          mail. This page renders them through the same call as the real \
                          mails, so what you see is what is sent.",
                },
                Feature {
                    api: "queue_mail",
                    why: "Real mails are queued (`state.queue_mail`), so a slow mail server \
                          never slows a page or the webhook; the queue tries five times.",
                },
                Feature {
                    api: "Localized mail",
                    why: "Orders keep the language the customer ordered in (`orders.locale`), \
                          and mails sent later from the queue or by staff are written in it; \
                          here they show in your language.",
                },
            ],
            under_hood: "No database query: a made-up order is rendered through each mail \
                         view and shown in a sandboxed frame.",
            docs: &[
                "docs/mail.md#mail-views",
                "docs/mail.md#localized-mail",
                "docs/mail.md#sending-a-mail",
            ],
            sources: &[
                "examples/bikeshop/src/app/sales/mails.rs",
                "examples/bikeshop/src/app/sales/notify.rs",
                "examples/bikeshop/resources/views/sales/mails.html",
                "examples/bikeshop/resources/views/mail/sales/confirmation.html",
                "examples/bikeshop/tests/sales.rs",
            ],
            code: &[
                Code {
                    title: "Handler: every mail rendered with `mail_view_in` for a made-up order",
                    region: "sales.mails.handler",
                },
                Code {
                    title: "Mail: the confirmation on the kit's mail components",
                    region: "sales.mails.template",
                },
                Code {
                    title: "Notify: the real send, queued, with a signed link to the order",
                    region: "sales.mails.tell",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![
        NotAPage {
            route: "cart.mini",
            reason: "an htmx fragment: the navbar's cart link with its count (explained on \
                     the cart page)",
        },
        NotAPage {
            route: "orders.signed",
            reason: "the signed link in the mails: it lets the browser see the order and \
                     redirects to it (explained on the order page)",
        },
        NotAPage {
            route: "sales.counter.variants",
            reason: "JSON: the counter's product search options (explained on the counter page)",
        },
        NotAPage {
            route: "sales.counter.customers",
            reason: "JSON: the counter's customer search options (explained on the counter page)",
        },
    ]
}
