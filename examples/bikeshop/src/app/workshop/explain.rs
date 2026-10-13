//! "About this page" entries for the workshop area's pages (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

const MOD: &str = "examples/bikeshop/src/app/workshop/mod.rs";
const MODEL: &str = "examples/bikeshop/src/app/workshop/model.rs";
const BIKES: &str = "examples/bikeshop/src/app/workshop/bikes.rs";
const BOOKING: &str = "examples/bikeshop/src/app/workshop/booking.rs";
const CAPACITY: &str = "examples/bikeshop/src/app/workshop/capacity.rs";
const STATUS: &str = "examples/bikeshop/src/app/workshop/status.rs";
const BOARD: &str = "examples/bikeshop/src/app/workshop/board.rs";
const ORDER: &str = "examples/bikeshop/src/app/workshop/order.rs";
const APPROVAL: &str = "examples/bikeshop/src/app/workshop/approval.rs";
const TASKS: &str = "examples/bikeshop/src/app/workshop/tasks.rs";
const NOTIFY: &str = "examples/bikeshop/src/app/rentals/notify.rs";
const HISTORY: &str = "crates/renox-blocks/views/blocks.html";
const TESTS: &str = "examples/bikeshop/tests/workshop.rs";
const BROWSER: &str = "tests/browser/bikeshop-workshop.test.mjs";

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "workshop.bikes",
            path: "/bikes",
            title: "My bikes",
            purpose: "The customer's own bikes, registered by hand (a catalogue model or \
                      free text, a frame number, a size, a photo) or by themselves when \
                      bought in the shop, each with its last service and a button to book \
                      the next one.",
            who: "Customers.",
            audience: &[Audience::Customer],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "relations::has_many",
                    why: "The work orders of all the bikes come in one query (`has_many` keyed \
                          by `customer_bike_id`), newest first, so the \"last service\" line \
                          and the count cost nothing per bike: a customer with ten bikes \
                          doesn't mean ten more queries.",
                },
                Feature {
                    api: "Upload",
                    why: "The photo is an optional `Upload` field, checked as an image by its \
                          content (not its name) and stored on the **private** disk \
                          (`store`, not `store_public`): a frame photo with its number is \
                          the owner's business. The bike's page opens it through a \
                          short-lived signed link (`temporary_url`) after checking the owner.",
                },
                Feature {
                    api: "#[derive(Validate)]",
                    why: "The form's rules are attributes on the struct (`required`, `max`, \
                          `image`), next to the fields they check, so the handler only runs \
                          with clean input and the rules can't drift away from the form.",
                },
                Feature {
                    api: "UI kit: select (searchable) + form_grid",
                    why: "The catalogue's bike models in a searchable select (typing beats \
                          scrolling a few hundred names), with \"another model\" for free \
                          text; the fields sit two by two on wider screens and in one \
                          column on a phone.",
                },
            ],
            under_hood: "Loading: the customer, their bikes, the work orders of all of them \
                         (one query), then the catalogue's bike categories and models for the \
                         select. Adding a bike: `Valid<BikeForm>`, the photo stored privately, \
                         the `customer_bikes` row written, and a redirect with a toast to the \
                         bike's history page.",
            docs: &[
                "docs/relations.md#the-loaders",
                "docs/validation.md#derivevalidate",
                "docs/validation.md#uploads",
            ],
            sources: &[
                BIKES,
                MODEL,
                "examples/bikeshop/resources/views/workshop/bikes.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: each bike's work orders in one `has_many` query",
                    region: "workshop.bikes.handler",
                },
                Code {
                    title: "Form: `#[derive(Validate)]` with an optional photo checked by content",
                    region: "workshop.bikes.form",
                },
                Code {
                    title: "Template: a multipart form on the kit's `<rx-form-grid>`",
                    region: "workshop.bikes.template",
                },
            ],
        },
        Explanation {
            route: "workshop.bikes.show",
            path: "/bikes/{bike}",
            title: "Bike service history",
            purpose: "One of the customer's bikes and everything done to it: each work order \
                      on a timeline with its tasks (ticked when done), the parts fitted and \
                      the mechanic's notes, plus what is open now and what it has cost.",
            who: "The bike's owner (anyone else gets a 404).",
            audience: &[Audience::Customer],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "UI kit: infolist",
                    why: "The bike's details as labels and values: the purchase date and the \
                          money spent formatted by the kit (`format=\"date\"`, `\"money\"`), \
                          the frame number copyable for an insurance or police form.",
                },
                Feature {
                    api: "blocks: history",
                    why: "The kit has no timeline; the example's `history` block draws the work \
                          orders as an ordered list with a dated marker each (colour **and** \
                          icon by status, never colour alone), its body in Markdown (the \
                          tasks and parts as a list, the notes as quotes).",
                },
                Feature {
                    api: "relations::has_many",
                    why: "The tasks, parts and notes of every work order come in one query each, \
                          and their names and stores in a few more (`belongs_to`), so the page \
                          costs the same number of queries for one service or fifty.",
                },
                Feature {
                    api: "Query<T>",
                    why: "`own_bike` looks the bike up by its id **and** the signed-in \
                          customer (two typed `eq` conditions), so another customer's id simply isn't \
                          found (404): there is no separate check to forget, and ids can't be \
                          probed.",
                },
            ],
            under_hood: "The customer and the bike, its work orders, then one query each for \
                         their tasks, the tasks' names, parts, notes and stores, two for the \
                         parts' variants and products, and one for the bike's catalogue model. \
                         The photo link (`workshop.bikes.photo`) checks the owner again and \
                         redirects to a 10-minute signed URL.",
            docs: &[
                "docs/ui.md#infolists-read-only-details",
                "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
            ],
            sources: &[
                BIKES,
                "examples/bikeshop/resources/views/workshop/bike.html",
                HISTORY,
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: the history in a fixed number of queries, however long",
                    region: "workshop.bikes.show.handler",
                },
                Code {
                    title: "Template: open orders, then the timeline as the `history` block",
                    region: "workshop.bikes.show.template",
                },
            ],
        },
        Explanation {
            route: "workshop.book",
            path: "/service/book",
            title: "Book a service",
            purpose: "Book a bike into a store's workshop: a package (tune-up, overhaul) or \
                      single tasks, a day that still has room, and a note. The estimate \
                      (price and mechanic time) and the days that are full update as the \
                      form changes.",
            who: "Customers with a registered bike.",
            audience: &[Audience::Customer],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "UI kit: date_picker (disabled_dates, closed_weekdays)",
                    why: "The kit's `date_picker` with the full days and the store's closed \
                          weekdays greyed out and skipped by the arrow keys, so people pick a day \
                          with room instead of learning it from an error; the server checks \
                          again, since anyone can send any date.",
                },
                Feature {
                    api: "UI kit: checkbox_list + radio",
                    why: "The tasks as a `checkbox_list` (a `Vec<i64>`, each with its time and \
                          price as a hint), the packages as a `radio` with a line on what each \
                          includes: both readable at a glance, and plain form fields that work \
                          without JavaScript.",
                },
                Feature {
                    api: "Htmx",
                    why: "The form `hx-get`s this same page on every change and swaps only the \
                          day picker and the estimate (`hx-select=\"#booking-live\"`), since \
                          the full days depend on the store and on the minutes chosen. One \
                          handler serves both the page and the update, with no JSON API.",
                },
                Feature {
                    api: "Valid<T> + after hook",
                    why: "The `after` hook checks the bike is the customer's, that something \
                          was chosen and that the day has room (`capacity::check_day`): the \
                          first capacity check, which puts the error next to the field.",
                },
                Feature {
                    api: "Db::begin_immediate + lock_for_update",
                    why: "The second capacity check, in the transaction that writes the work \
                          order (`capacity::book`): the store's row is taken first, so two \
                          bookings of the last slot run one after the other and the second is \
                          refused instead of overbooking the day.",
                },
                Feature {
                    api: "notify",
                    why: "The confirmation goes out as a mail and an in-app notification (one \
                          `Notice`), written in the customer's language (their `locale`), \
                          whoever's request sent it.",
                },
            ],
            under_hood: "Loading: the customer's bikes, the stores, the tasks, and the period's \
                         work orders of the chosen store (one query, summed per day in Rust) \
                         for the full days. Booking: `Valid<BookForm>` with its `after` hook, \
                         then `capacity::book` (store row locked, the day's minutes summed \
                         again, the work order and its tasks written), the confirmation, and \
                         a redirect to the work order's page. The scheduled \
                         `workshop:reminders` reminds the customer the evening before.",
            docs: &[
                "docs/validation.md#hooks-prepare-authorize-after",
                "docs/ui.md#form-fields",
                "docs/ui.md#fragments-and-out-of-band-swaps",
                "docs/relations.md#more-of-the-query-builder",
                "docs/mail.md#notifications",
            ],
            sources: &[
                BOOKING,
                CAPACITY,
                NOTIFY,
                "examples/bikeshop/resources/views/workshop/book.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Template: htmx asks the page again and swaps only the day and estimate",
                    region: "workshop.book.template",
                },
                Code {
                    title: "Handler: the full days for this store and these minutes",
                    region: "workshop.book.handler",
                },
                Code {
                    title: "Transaction: the day checked again with the store's row locked",
                    region: "workshop.book.capacity",
                },
            ],
        },
        Explanation {
            route: "workshop.service.show",
            path: "/service/{order}",
            title: "My service",
            purpose: "A work order for its customer: a progress line (scheduled, checked in, \
                      in progress, ready, collected), the tasks ticked as the mechanic does \
                      them, the parts, the notes, extra work and its answer, the total; \
                      reschedule or cancel until 24 hours before, pay online when it's ready.",
            who: "The customer whose bike it is.",
            audience: &[Audience::Customer],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "notify",
                    why: "Every status change sends a mail and an in-app notification (a \
                          `DatabaseMessage`), which the bell in the layout shows live through \
                          its notification stream: the customer needn't keep reloading this \
                          page to know the bike is ready.",
                },
                Feature {
                    api: "renox::webhook",
                    why: "\"Pay\" starts an online payment through the shared payments \
                          contract; the gateway's signed webhook marks the payment paid and \
                          the app emits `PaymentSucceeded`, which the workshop listens to and \
                          marks the work order paid. The page never trusts the browser's \
                          return for that.",
                },
                Feature {
                    api: "UI kit: action_sheet + confirm + date_picker",
                    why: "Rescheduling is a small form in a sheet, sent with htmx: a full day \
                          answers 422 and the error stays in the sheet. Cancelling asks first \
                          with `confirm`. Both show only while the booking can still change.",
                },
                Feature {
                    api: "workshop::capacity (move_booking)",
                    why: "A new day goes through the same locked transaction as a booking \
                          (the store's row first, the day's room checked again), so a \
                          reschedule can't overbook a day either.",
                },
                Feature {
                    api: "Schedule::daily_at",
                    why: "`workshop:reminders` (18:00, `APP_TIMEZONE`) reminds tomorrow's \
                          bookings once (`reminded_at`); a rescheduled booking is reminded \
                          again for its new day.",
                },
            ],
            under_hood: "The work order is found through the customer's bikes (a 404 for \
                         anyone else and for fleet repairs); then its tasks and their names, \
                         parts and their names, notes, extra work, the bike and the store. \
                         Cancelling sets the status (the customer is told); rescheduling runs \
                         `capacity::move_booking`; \"Pay\" redirects to the gateway's page.",
            docs: &[
                "docs/mail.md#database-notifications",
                "docs/ui.md#actions",
                "docs/scheduling.md#scheduled-tasks",
                "docs/scheduling.md#events",
            ],
            sources: &[
                BOOKING,
                STATUS,
                CAPACITY,
                TASKS,
                "examples/bikeshop/resources/views/workshop/service.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Policy: only the bike's owner sees its work orders",
                    region: "workshop.service.show.owner",
                },
                Code {
                    title: "Handler: the progress steps from the order's status",
                    region: "workshop.service.show.handler",
                },
                Code {
                    title: "Template: pay, move the day in an `<rx-action-sheet>`, or cancel",
                    region: "workshop.service.show.template",
                },
            ],
        },
        Explanation {
            route: "workshop.extra.show",
            path: "/service/approve/{extra}",
            title: "Approve extra work",
            purpose: "The mechanic found more to do: the customer reads what and for how much, \
                      and approves or refuses it from the mail's link, without logging in.",
            who: "The customer, from the link in their mail.",
            audience: &[Audience::Customer, Audience::Visitor],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "Signed URLs",
                    why: "`state.signed_url` makes the link (HMAC-SHA256 with `APP_KEY`, valid \
                          72 hours); `ValidSignature` refuses one changed by hand or expired with \
                          a 403. The buttons post to the same signed address. The bike waits \
                          on the stand, so answering from the phone in one tap, without a \
                          password, gets the work going sooner.",
                },
                Feature {
                    api: "Single use",
                    why: "The answer is recorded once (`status`, `decided_at`); afterwards the \
                          page shows the decision and a second answer, or one after the \
                          deadline, is refused (409), so a forwarded mail can't change it.",
                },
                Feature {
                    api: "StockMovement::record",
                    why: "Approved parts are taken from the store's stock, each in a \
                          transaction with a ledger movement (reason `service`), or recorded \
                          as waited for when the shelf is short.",
                },
            ],
            under_hood: "Showing: the extra work, its work order, store and bike. Approving \
                         adds the tasks to the work order, takes the parts and adds up the \
                         totals; either answer puts a work order that was waiting for approval \
                         back in progress (the customer is told). The store's staff who may \
                         work on it get an in-app notification either way. The page is \
                         `noindex`.",
            docs: &["docs/routing.md#signed-urls", "docs/routing.md#csrf"],
            sources: &[
                APPROVAL,
                ORDER,
                "examples/bikeshop/resources/views/workshop/approve.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: `ValidSignature` instead of a login",
                    region: "workshop.extra.show.handler",
                },
                Code {
                    title: "Decision: single use, refused once answered or expired",
                    region: "workshop.extra.show.decide",
                },
                Code {
                    title: "Template: two buttons posting back to the same signed address",
                    region: "workshop.extra.show.template",
                },
            ],
        },
        Explanation {
            route: "workshop.board",
            path: "/staff/workshop",
            title: "Workshop board",
            purpose: "The active store's work orders in columns by status (scheduled, \
                      checked in, in progress, waiting for parts, waiting for approval, \
                      ready): bookings, walk-ins, plan visits and fleet repairs together, \
                      labelled by source; filtered by mechanic and day; moved by dragging.",
            who: "Mechanics and store managers (`workorders.view`; moving needs \
                  `workorders.update`).",
            audience: &[Audience::Mechanic, Audience::Manager],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "blocks: kanban",
                    why: "The kit has no board; the example's `kanban` block moves cards by \
                          mouse, touch or keyboard and sends each move as an htmx POST. The \
                          server checks the step is allowed and the person may work on that \
                          work order in its store, or the card goes back: the browser never \
                          decides what a move means.",
                },
                Feature {
                    api: "renox::context",
                    why: "The board is the active store's (`active_store`, read from the \
                          request's context); its mechanics (for the filter) are the people \
                          holding `workorders.update` there now.",
                },
                Feature {
                    api: "relations::belongs_to",
                    why: "The cards' bikes, customers, fleet bikes, models and mechanics come \
                          in a fixed number of queries, however many cards the board has.",
                },
                Feature {
                    api: "htmx polling",
                    why: "The board reloads itself every minute (`hx-trigger=\"every 60s\"`, \
                          `hx-select` of the board), so other mechanics' moves and new \
                          bookings show up without a reload or a socket.",
                },
            ],
            under_hood: "One query for the open work orders (filters applied), then the \
                         cards' relations, then the mechanics. A move: `Valid<Move>`, \
                         `access::find` (404 for another store's), `access::require` \
                         (`workorders.update` in its operating store), the step checked \
                         against `status::allowed`, the status saved and the customer told.",
            docs: &[
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
                "docs/ui.md#fragments-and-out-of-band-swaps",
            ],
            sources: &[
                BOARD,
                STATUS,
                "examples/bikeshop/resources/views/workshop/board.html",
                "crates/renox-blocks/views/blocks.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Template: filters, then the `kanban` block that reloads every minute",
                    region: "workshop.board.template",
                },
                Code {
                    title: "Handler: the active store's open orders, filtered by mechanic and day",
                    region: "workshop.board.handler",
                },
                Code {
                    title: "Move: each drag is a POST checked against the allowed steps",
                    region: "workshop.board.move",
                },
            ],
        },
        Explanation {
            route: "workshop.walkin",
            path: "/staff/workshop/new",
            title: "Walk-in work order",
            purpose: "A work order for someone at the counter: a known customer (searched) or \
                      a new one without an account, the bike, the tasks and the day (today by \
                      default; a work order for today is checked in at once).",
            who: "Whoever works on work orders in the active store (`workorders.update`): \
                  mechanics, store managers and the owner. The counter's cashiers see the \
                  board but don't open work orders (the form and its POST answer 403).",
            audience: &[Audience::Mechanic, Audience::Manager, Audience::Owner],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "renox::select",
                    why: "The customer is a searchable select answered by the server \
                          (`options_url`, `OptionQuery`), by name, email or phone: the shop \
                          has too many customers to send them all with the page.",
                },
                Feature {
                    api: "UI kit: date_picker (disabled_dates, closed_weekdays)",
                    why: "The store's closed weekdays and the days too full for a half-hour \
                          job can't be picked; the exact check, for the tasks chosen, runs on \
                          the server.",
                },
                Feature {
                    api: "Valid<T> + after hook",
                    why: "`required_if` asks for a new customer's name only when none was \
                          picked; the hook checks the day's capacity for the tasks chosen.",
                },
            ],
            under_hood: "Loading: the tasks, the active store and its full days. Saving: the \
                         customer is found or created (no user account), the bike found by \
                         name or registered to them, and the same `capacity::book` \
                         transaction as online bookings writes the work order; then its page.",
            docs: &[
                "docs/ui.md#options-from-the-server",
                "docs/validation.md#every-rule",
                "docs/validation.md#hooks-prepare-authorize-after",
            ],
            sources: &[
                BOARD,
                CAPACITY,
                "examples/bikeshop/resources/views/workshop/walk_in.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Template: a known customer from a searchable `<rx-select>`, or a new one",
                    region: "workshop.walkin.template",
                },
                Code {
                    title: "Form: the new name is `required_if` no customer was picked",
                    region: "workshop.walkin.form",
                },
                Code {
                    title: "Handler: finds the customer or creates one at the counter",
                    region: "workshop.walkin.store",
                },
            ],
        },
        Explanation {
            route: "workshop.order",
            path: "/staff/workshop/{order}",
            title: "Work order",
            purpose: "The bench: the checklist from the tasks, the parts used (found by what \
                      fits the bike and taken from the store's stock, or waited for), notes \
                      and before/after photos, extra work proposed to the customer, who works \
                      on it, the next status, and the payment at the counter when it's ready.",
            who: "Mechanics; cashiers for the payment.",
            audience: &[Audience::Mechanic, Audience::Cashier, Audience::Manager],
            flow: Flow::Service,
            features: &[
                Feature {
                    api: "renox::select",
                    why: "The part select asks the server as the mechanic types: the parts \
                          that **fit** this bike (`part_fits`, a `Pivot`, through \
                          `FITTING_PARTS.ids`), each with the store's stock, so nobody fits \
                          the wrong chain or promises a part that isn't on the shelf.",
                },
                Feature {
                    api: "StockMovement::record",
                    why: "A part used is a stock ledger movement (reason `service`, a `Morph` \
                          reference to the work order) and its line in one transaction; out \
                          of stock, the line waits and the order goes to \"waiting for parts\" \
                          (purchasing, #240, sees what is waited for).",
                },
                Feature {
                    api: "Signed URLs",
                    why: "Proposing extra work mails the customer a signed link to approve or \
                          refuse it without logging in; the order waits for the answer.",
                },
                Feature {
                    api: "has_permission_in",
                    why: "Every action checks `workorders.update` (or `orders.sell` for the \
                          payment) in the work order's own store (`access::require`); another \
                          store's staff get a 404, and someone who may only look sees no \
                          buttons.",
                },
                Feature {
                    api: "Events and listeners",
                    why: "Collecting emits `WorkOrderClosed`: the intercompany books (#245) \
                          bill a fleet repair to its owner store and plans (#237) mark the \
                          visit done, without the workshop knowing about either. The counter \
                          payment emits `PaymentSucceeded`, which marks the order paid.",
                },
                Feature {
                    api: "Upload",
                    why: "Before/after photos are private uploads, opened through a short-lived \
                          signed link after the store is checked, since they show customers' \
                          bikes.",
                },
            ],
            under_hood: "The order, its tasks and their names, parts and their names, notes, \
                         extra work, the bike, the customer, the stores and the mechanic. Each \
                         action is one short handler that redirects back with a toast. \
                         Collecting needs the order paid (409 otherwise), except a fleet \
                         repair, which also puts the bike back in the fleet, serviced.",
            docs: &[
                "docs/ui.md#options-from-the-server",
                "docs/relations.md#changing-a-many-to-many",
                "docs/relations.md#polymorphic-relations",
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/scheduling.md#events",
            ],
            sources: &[
                ORDER,
                STATUS,
                APPROVAL,
                NOTIFY,
                MOD,
                "examples/bikeshop/src/app/stock/model.rs",
                "examples/bikeshop/resources/views/workshop/order.html",
                HISTORY,
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Policy: visible in the store, and `workorders.update` to work on it",
                    region: "workshop.order.policy",
                },
                Code {
                    title: "Stock: a part taken in one transaction, or left waiting",
                    region: "workshop.order.part",
                },
                Code {
                    title: "Template: the parts, with a searchable `<rx-select>` fed by the server",
                    region: "workshop.order.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![
        NotAPage {
            route: "workshop.bikes.photo",
            reason: "a redirect to the bike photo's signed temporary URL",
        },
        NotAPage {
            route: "workshop.customers",
            reason: "JSON options for the walk-in form's customer select (renox::select)",
        },
        NotAPage {
            route: "workshop.parts",
            reason: "JSON options for the part select: parts that fit the bike (renox::select)",
        },
        NotAPage {
            route: "workshop.order.photo",
            reason: "a redirect to a note photo's signed temporary URL",
        },
    ]
}
