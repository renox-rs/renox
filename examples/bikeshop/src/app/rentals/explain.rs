//! "About this page" entries for the rentals area's pages (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

const MOD: &str = "examples/bikeshop/src/app/rentals/mod.rs";
const MODEL: &str = "examples/bikeshop/src/app/rentals/model.rs";
const PRICING: &str = "examples/bikeshop/src/app/rentals/pricing.rs";
const BOOKING: &str = "examples/bikeshop/src/app/rentals/booking.rs";
const RESERVE: &str = "examples/bikeshop/src/app/rentals/reserve.rs";
const IDENTITY: &str = "examples/bikeshop/src/app/rentals/identity.rs";
const COUNTER: &str = "examples/bikeshop/src/app/rentals/counter.rs";
const FLEET: &str = "examples/bikeshop/src/app/rentals/fleet.rs";
const TASKS: &str = "examples/bikeshop/src/app/rentals/tasks.rs";
const NOTIFY: &str = "examples/bikeshop/src/app/rentals/notify.rs";
const POLICY: &str = "examples/bikeshop/src/app/access/policy.rs";
const TESTS: &str = "examples/bikeshop/tests/rentals.rs";
const BROWSER: &str = "tests/browser/bikeshop-rentals.test.mjs";

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "rentals.create",
            path: "/rent",
            title: "Rent a bike",
            purpose: "Where a rental starts: pick a store, a start and an end (by the hour or \
                      the day), a bike type and a size, and see the bikes standing at that \
                      store and free for the whole period, each with its price and deposit, \
                      then reserve one. Under the list, the store's day as a timeline of \
                      bikes against hours; a free hour starts a search from there.",
            who: "Customers planning a ride (anyone can look; reserving needs an account).",
            audience: &[Audience::Visitor, Audience::Customer],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "blocks: datetime_range",
                    why: "Bikes are rented by the hour, and the kit's `date_picker` picks a \
                          day but not an hour; the example's `datetime_range` block puts two \
                          of them beside time selects (only the shop's hours, 08:00 to 20:00) \
                          and sends two plain fields, `starts_at` / `ends_at` \
                          (`YYYY-MM-DDTHH:MM`). The reservation form reads them as \
                          `NaiveDateTime` (wall-clock time in `APP_TIMEZONE`) and Rust turns \
                          them into moments, so no date parsing happens in the browser.",
                },
                Feature {
                    api: "blocks: availability",
                    why: "The kit has no timeline, so the example's `availability` block draws \
                          the store's day as a table of bikes × hours (booked, free; at most 12 \
                          bikes). Each free hour is a plain link to this page with a two-hour \
                          period from there, so it works without JavaScript, and being a real \
                          table it reads well to a screen reader (\"Trail 5, 11:00, Book\").",
                },
                Feature {
                    api: "Htmx",
                    why: "The search form re-asks the same page on every change (`hx-get`, \
                          `hx-select` of `#rent-results`, `hx-push-url`), so only the bike list \
                          moves and the address can be shared. The handler needs nothing \
                          special: it always renders the whole page and htmx keeps the part it \
                          wants.",
                },
                Feature {
                    api: "Valid<T> + after hook",
                    why: "The reservation form's rules run first; its `after` hook then checks \
                          the period (in the future, at most 14 days) and that the bike still \
                          stands at the store and is free (the **first** of two overlap \
                          checks). It needs the database, which plain rules can't reach, and it \
                          puts a clash next to the bike with the form kept.",
                },
                Feature {
                    api: "Db::begin_immediate + lock_for_update",
                    why: "The **second** overlap check, in the transaction that writes the \
                          rental: the bike's row is taken first (SQLite's write lock, \
                          PostgreSQL's row lock), so two customers booking the last bike at \
                          once run one after the other and the second is told it was just \
                          taken. Both checks are needed: the hook gives a friendly error, the \
                          transaction makes it true.",
                },
                Feature {
                    api: "Pricing in Rust",
                    why: "`pricing::quote` is a plain function (whole days at the daily rate, \
                          the hours after them counted when started and capped at one daily \
                          rate, the deposit on top). Kept out of handlers and SQL, it is \
                          unit-tested without a database and gives the same number on this \
                          page, in the booking and at the counter.",
                },
                Feature {
                    api: "StoreAttr::Location",
                    why: "Availability is by **location**: the bikes listed are those standing \
                          at the store now (`location_store_id`), whoever owns them, since only \
                          a bike that is there can be handed over (a bike North placed at South \
                          is South's to rent out, #245). The owner store is copied onto the \
                          rental for the books.",
                },
            ],
            under_hood: "Loading: the stores, the bike categories, the variants standing at \
                         the store with their models (for sizes and names), the free bikes (two \
                         queries: the bikes at the store, then the clashing rentals of all of \
                         them at once) and the day's timeline (the bikes, their rentals that \
                         day, their names). Reserving (`POST /rent`): `Valid<ReserveForm>` with \
                         its `after` hook; a customer who never sent an ID is sent to the ID \
                         page first; then `booking::book` in one transaction \
                         (`begin_immediate`, the bike's row with `lock_for_update`, the clash \
                         query again, the insert with a fresh `Ulid` code and the bike's owner \
                         store), then a redirect to the reservation, where the deposit is \
                         paid.",
            docs: &[
                "docs/validation.md#hooks-prepare-authorize-after",
                "docs/relations.md#more-of-the-query-builder",
                "docs/postgresql.md#things-that-behave-differently-on-purpose",
                "docs/types.md#dates-and-times",
                "docs/types.md#time-zones",
            ],
            sources: &[
                RESERVE,
                BOOKING,
                PRICING,
                MODEL,
                "examples/bikeshop/resources/views/rentals/search.html",
                "crates/renox-blocks/views/blocks.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Template: the filters, a GET form htmx re-sends as they change",
                    region: "rentals.create.filters",
                },
                Code {
                    title: "Form: `Validate::after` checks the period and the bike",
                    region: "rentals.create.form",
                },
                Code {
                    title: "Transaction: the second check, under `begin_immediate` and `lock_for_update`",
                    region: "rentals.create.book",
                },
            ],
        },
        Explanation {
            route: "rentals.show",
            path: "/rentals/{code}",
            title: "A reservation",
            purpose: "The reservation after booking: its code (to show at the counter), the \
                      bike, the store, the period, the price and the deposit, the deadline \
                      to pay the deposit online and the time the bike is held until; \
                      \"Pay the deposit\" and \"Cancel\" while they apply.",
            who: "The customer who made it (anyone else gets a 404).",
            audience: &[Audience::Customer],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "Ulid",
                    why: "The reservation code is a `Ulid` (`Ulid::new()` in the booking): \
                          made without asking the database, sortable by time, impossible to \
                          guess, and it doesn't reveal how many rentals there are (an id \
                          would). It is also the address of this page and what the cashier \
                          searches for at the counter.",
                },
                Feature {
                    api: "renox::webhook",
                    why: "\"Pay the deposit\" starts an online payment through the shared \
                          payments contract (`payments::start`) and sends the customer to the \
                          gateway's page (Midtrans' with a key, else the demo gateway). This \
                          page never marks anything paid: only the gateway's webhook does \
                          (Midtrans' is signed and checked), so a customer can't fake a \
                          payment by opening a URL.",
                },
                Feature {
                    api: "Schedule::every_minutes",
                    why: "The page states the two deadlines the `rentals:watch` task enforces \
                          every 15 minutes: an unpaid reservation is called off 30 minutes \
                          after it was made, and one not picked up 30 minutes after its start \
                          is a no-show (the bike is released, the deposit keeps the price at \
                          most, the customer is told). A task, not a check on each page view, \
                          so the bike is freed even if nobody opens the page.",
                },
                Feature {
                    api: "Events and listeners",
                    why: "`PaymentSucceeded` / `PaymentFailed` are the sales area's events; \
                          rentals listens (`Registry::listen` in its module) and acts only on \
                          `Payable::Rental`: a paid deposit becomes held and the confirmation \
                          goes out, a failed one calls the reservation off. The payment code \
                          never knows about rentals.",
                },
                Feature {
                    api: "UI kit: infolist",
                    why: "The reservation's details as the kit's `infolist`: labels and values, \
                          money and dates formatted by `entry(…, format=…)`, the code \
                          `copyable` so it can be pasted or shown at the counter.",
                },
            ],
            under_hood: "The rental is looked up by code **and** customer (someone else's \
                         code is a 404), loaded with its bike, model, customer and stores by \
                         `RentalRow::load` (five queries). \"Pay the deposit\" (`POST \
                         /rentals/{code}/pay`) writes a pending payment and redirects to the \
                         gateway; its webhook emits `PaymentSucceeded`, the deposit is marked \
                         held and the customer gets a mail and an in-app notification with the \
                         code. Cancelling (`POST /rentals/{code}/cancel`) is allowed until an \
                         hour before the start; a held deposit is given back whole, and a mail \
                         and an in-app notification say so.",
            docs: &[
                "docs/types.md#keys",
                "docs/operations.md#failed-webhook-calls",
                "docs/scheduling.md#scheduled-tasks",
                "docs/scheduling.md#events",
                "docs/ui.md#infolists-read-only-details",
            ],
            sources: &[
                RESERVE,
                MODEL,
                MOD,
                TASKS,
                NOTIFY,
                "examples/bikeshop/src/app/sales/payments.rs",
                "examples/bikeshop/resources/views/rentals/show.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: only the customer's own reservation, else a 404",
                    region: "rentals.show.handler",
                },
                Code {
                    title: "Template: pay the deposit, or cancel with the kit's `<rx-confirm>`",
                    region: "rentals.show.actions",
                },
                Code {
                    title: "Payment: the deposit goes to the gateway through `payments::start`",
                    region: "rentals.show.pay",
                },
            ],
        },
        Explanation {
            route: "rentals.mine",
            path: "/rentals",
            title: "My rentals",
            purpose: "The customer's rentals: the current ones (reserved, out, overdue) with \
                      their code and due time, and the past ones with the price, the late \
                      and damage fees and what came back of the deposit. Also whether the ID \
                      check is done.",
            who: "Customers.",
            audience: &[Audience::Customer],
            flow: Flow::Account,
            features: &[
                Feature {
                    api: "relations::belongs_to",
                    why: "`RentalRow::load` brings each rental's bike, model, customer and \
                          stores with `belongs_to`, one query per relation however many \
                          rentals there are, instead of a query per row (N+1); \
                          `tests/rentals.rs` checks the count stays the same with more \
                          rentals.",
                },
                Feature {
                    api: "UI kit: list + badge",
                    why: "Each rental is a row of the kit's `list`, its status a `badge` whose \
                          colour and word agree (never colour alone), so the page is readable \
                          for colour-blind people and screen readers.",
                },
                Feature {
                    api: "money filter",
                    why: "Money is stored as integers in the smallest unit (no rounding errors \
                          from floats) and shown with `money` in the visitor's locale \
                          (`$150.00` / `$150,00`).",
                },
            ],
            under_hood: "One query for the customer record (made from the account on the \
                         first visit), one for their latest 50 rentals, then their relations \
                         all at once (`RentalRow::load`, five queries), then the latest ID \
                         document for the ID check's status. Nothing is written.",
            docs: &[
                "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
                "docs/types.md#money",
            ],
            sources: &[
                RESERVE,
                MODEL,
                "examples/bikeshop/resources/views/rentals/mine.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: current and past rentals from one `RentalRow::load`",
                    region: "rentals.mine.handler",
                },
                Code {
                    title: "Query: a page of rentals with their relations in five queries",
                    region: "rentals.rows",
                },
                Code {
                    title: "Template: the kit's `<rx-list>` with a shared `rental_item` macro",
                    region: "rentals.mine.template",
                },
            ],
        },
        Explanation {
            route: "rentals.identity",
            path: "/rentals/identity",
            title: "ID check",
            purpose: "Before the first rental, the customer sends a photo of their ID \
                      document and its number, and picks the store that checks it. Once \
                      staff approve it, they rent in every store without showing it again.",
            who: "Customers renting for the first time.",
            audience: &[Audience::Customer],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "Upload",
                    why: "The photo is a form field (`Option<Upload>`) checked with `image()` \
                          (by its **content**, sniffed, not its name) and `max` (5 MB), then \
                          stored with `Upload::store` under a random name in the storage \
                          disk's `identity/`, **outside** `public/`: no address reaches it, \
                          which an ID document needs.",
                },
                Feature {
                    api: "Encrypted<T>",
                    why: "The ID number is the customer's `id_number`, an \
                          `Encrypted<String>` sealed with `APP_KEY` when saved (`save_only` \
                          writes just that column and `id_verified_at`), so a copy of the \
                          database leaks nothing; staff see it masked (`•••••678`).",
                },
                Feature {
                    api: "Valid<T> + prepare hook",
                    why: "`prepare` tidies the number (trimmed, upper case) before the rules \
                          check its length and characters, so \" ab-123 \" and \"AB-123\" are \
                          the same number and the customer isn't refused for a space.",
                },
                Feature {
                    api: "notify",
                    why: "The chosen store's staff who may verify IDs get an in-app \
                          notification (only in the app: it is work, not news); the customer \
                          gets a mail and a notification when it is approved or refused, each \
                          written in the recipient's language.",
                },
            ],
            under_hood: "Loading: the customer, their latest ID document and the stores. On \
                         submit: the photo goes to private storage, the number is sealed onto \
                         the customer (who is unverified until staff approve), an earlier \
                         pending document is marked refused, an `identity_documents` row is \
                         written, and `notify::staff` finds the users holding \
                         `rentals.verify_id` in that store (a query over the permission \
                         tables within their dates, never a role name). A customer with a \
                         document waiting may already reserve; the pick-up needs it \
                         approved.",
            docs: &[
                "docs/validation.md#uploads",
                "docs/types.md#the-table",
                "docs/validation.md#hooks-prepare-authorize-after",
                "docs/mail.md#database-notifications",
            ],
            sources: &[
                IDENTITY,
                NOTIFY,
                MODEL,
                "examples/bikeshop/resources/views/rentals/identity.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Form: `prepare` tidies the number, `image()` sniffs the photo",
                    region: "rentals.identity.form",
                },
                Code {
                    title: "Handler: the photo to private storage, the number `Encrypted`",
                    region: "rentals.identity.store",
                },
                Code {
                    title: "Template: a multipart form with the kit's `<rx-file>` field",
                    region: "rentals.identity.template",
                },
            ],
        },
        Explanation {
            route: "rentals.identities",
            path: "/staff/identities",
            title: "ID checks waiting",
            purpose: "The ID documents customers sent to this store, oldest first: open the \
                      photo, compare it with the masked number, approve or refuse with a \
                      note.",
            who: "Cashiers and managers (`rentals.verify_id` in the store).",
            audience: &[Audience::Cashier, Audience::Manager],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "Signed URLs",
                    why: "\"Open photo\" goes through a route that checks the permission in the \
                          document's store (or a store serving one of the customer's open \
                          rentals) and then redirects to `storage.temporary_url`, a link signed \
                          with `APP_KEY` that works for five minutes. The photo never gets a \
                          lasting address: other customers and staff without the permission \
                          never get a link, and a link passed on soon stops working.",
                },
                Feature {
                    api: "scopes_with",
                    why: "The list is `access::visible::<IdentityDocument>(rentals.verify_id)`: \
                          the documents of the stores where the person holds the permission \
                          now (all of them for the owner's global role), one `WHERE` added to \
                          the query rather than a filter in Rust after loading everything.",
                },
                Feature {
                    api: "has_permission_in",
                    why: "Opening, approving and refusing check the permission in the \
                          document's store or a store serving one of the customer's open \
                          rentals, not the active store: ABAC on the record's own \
                          attributes. Anyone else gets a 404, so document ids can't be \
                          probed.",
                },
                Feature {
                    api: "UI kit: action_sheet",
                    why: "\"Refuse\" opens a sheet with a required note the customer will \
                          read, sent with htmx; a missing note stays in the sheet with its \
                          error, so the list behind it is never reloaded for a mistake.",
                },
                Feature {
                    api: "Toast",
                    why: "Approve and refuse answer with a toast and go back to the list \
                          (`Back`), so the next document is right there.",
                },
            ],
            under_hood: "Three queries: the waiting documents (oldest first, at most 100), \
                         their customers, their stores. Approving sets the document's status \
                         and reviewer and the customer's `id_verified_at` (valid in every \
                         store), then sends the customer a mail and an in-app notification; \
                         refusing stores the note and tells the customer, who may send \
                         another document.",
            docs: &[
                "docs/routing.md#signed-urls",
                "docs/authorization.md#lists-scopes_with",
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/ui.md#actions",
                "docs/ui.md#toasts",
            ],
            sources: &[
                IDENTITY,
                POLICY,
                NOTIFY,
                "examples/bikeshop/resources/views/rentals/identities.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: `access::visible` keeps it to the stores one checks for",
                    region: "rentals.identities.handler",
                },
                Code {
                    title: "Photo: a signed link that works for five minutes",
                    region: "rentals.identities.photo",
                },
                Code {
                    title: "Template: approve in a form, refuse with a note in an `<rx-action-sheet>`",
                    region: "rentals.identities.template",
                },
            ],
        },
        Explanation {
            route: "rentals.counter",
            path: "/staff/rentals",
            title: "Rental counter",
            purpose: "A cashier's day: the pick-ups due today at this store, the bikes out \
                      and the overdue ones, and a search by reservation code or customer. \
                      A rental out from another store shows up too when it is searched for: \
                      bikes can come back anywhere.",
            who: "Cashiers and staff at the counter (`rentals.view` in the active store).",
            audience: &[Audience::Cashier, Audience::Staff, Audience::Manager],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "renox::context",
                    why: "The active store is the access area's middleware's choice, kept in \
                          `renox::context` for the request, so every handler and helper reads \
                          it (`access::active_store::current()`) without passing it around; \
                          every list here is that store's.",
                },
                Feature {
                    api: "StoreAttr::Operating",
                    why: "Pick-ups belong to the rental's **operating** store (the store that \
                          serves the customer, where the bike stands). Returns are allowed in \
                          any store where the person holds `rentals.return`, so the search \
                          also finds bikes out from another store: a customer can bring a \
                          bike back anywhere.",
                },
                Feature {
                    api: "relations::belongs_to",
                    why: "All three lists and the search results are loaded together by \
                          `RentalRow::load`: one query per relation, not per row, so a busy \
                          day doesn't make the page slower (`tests/rentals.rs` counts them).",
                },
                Feature {
                    api: "UI kit: columns + list",
                    why: "Three columns on a desk, one under the other on a phone (the kit's \
                          `columns`, a CSS grid), each a `list` of rentals with its count: \
                          the layout needs no CSS of the app's own.",
                },
            ],
            under_hood: "One query for the store's rentals (out, overdue, or reserved to \
                         start before tomorrow); with a search, two more (the customers whose \
                         name or email matches, then their rentals or the one with that code, \
                         kept only when the person may see it); then `RentalRow::load` once \
                         for all of them. Nothing is written.",
            docs: &[
                "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
                "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
                "docs/ui.md#navigation-and-page-structure",
            ],
            sources: &[
                COUNTER,
                "examples/bikeshop/src/app/access/active_store.rs",
                "examples/bikeshop/resources/views/rentals/counter.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: the active store's day in one query, rows in one load",
                    region: "rentals.counter.handler",
                },
                Code {
                    title: "Template: three columns of the kit's `<rx-list>`",
                    region: "rentals.counter.template",
                },
            ],
        },
        Explanation {
            route: "rentals.walkin",
            path: "/staff/rentals/walk-in",
            title: "Walk-in rental",
            purpose: "A rental for someone standing at the counter without a reservation: \
                      a verified customer, a period starting now, and a bike standing here \
                      and free for it. It is booked like a reservation, then opened at the \
                      desk for the pick-up.",
            who: "Cashiers and staff (`rentals.checkout` in the active store).",
            audience: &[Audience::Cashier, Audience::Staff],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "renox::select",
                    why: "The customer is a searchable select whose options come from the \
                          server as the cashier types (`options_url`, `OptionQuery`, \
                          `SelectOption` from `GET /staff/rentals/customers`): only verified \
                          customers, at most 20 at a time, never the whole customer table in \
                          the page.",
                },
                Feature {
                    api: "blocks: datetime_range",
                    why: "The same period picker as `/rent`, filled in from now to two hours \
                          later in half-hour steps, so the cashier only changes the end.",
                },
                Feature {
                    api: "Valid<T> + after hook",
                    why: "The hook checks the customer is verified, the period, and that the \
                          bike stands here and is free, with the errors next to the fields; \
                          the booking transaction checks the overlap again, since another \
                          booking may land between the two.",
                },
            ],
            under_hood: "The bikes standing at the store and free for the next two hours \
                         (two queries) and their names (two more) fill the bike select. On \
                         submit the same `booking::book` transaction as online reservations \
                         runs, with the active store as operating store and the cashier as \
                         `served_by` (so the 30-minute online payment deadline doesn't apply), \
                         then the desk opens for the pick-up, where the price and deposit are \
                         paid.",
            docs: &[
                "docs/ui.md#options-from-the-server",
                "docs/validation.md#hooks-prepare-authorize-after",
            ],
            sources: &[
                COUNTER,
                BOOKING,
                "examples/bikeshop/resources/views/rentals/walk_in.html",
                "crates/renox-blocks/views/blocks.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Template: a searchable `<rx-select>` that asks the server for options",
                    region: "rentals.walkin.template",
                },
                Code {
                    title: "Options: `OptionQuery` answers searches and lookups",
                    region: "rentals.walkin.options",
                },
                Code {
                    title: "Form: `after` checks the customer, the period and the bike",
                    region: "rentals.walkin.form",
                },
            ],
        },
        Explanation {
            route: "rentals.desk",
            path: "/staff/rentals/{rental}",
            title: "Pick-up and return",
            purpose: "One rental at the counter. Before the ride: the customer (verified, the \
                      ID masked), the condition checklist, and what is paid now (the price, \
                      and the deposit when it wasn't paid online). After it: the checklist \
                      again, the late fee so far, damage (photos, a note, a fee), and how the \
                      deposit settles against the fees.",
            who: "Cashiers and staff: the pick-up in the operating store, the return in any \
                  store.",
            audience: &[Audience::Cashier, Audience::Staff],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "renox::context",
                    why: "The active store decides what the page offers: the pick-up only in \
                          the rental's operating store, the return wherever the person holds \
                          `rentals.return`; the bike's **location** becomes the store that took \
                          it back, its **owner** never changes. The same rental shows a \
                          different form in each store without a parameter in the URL.",
                },
                Feature {
                    api: "UI kit: checkbox_list + toggle_buttons",
                    why: "The condition checklist is a `checkbox_list` (a `Vec<String>`, each \
                          item checked with `one_of`), cash or card a `toggle_buttons`; the \
                          damage fields appear only when the \"damaged\" switch is on \
                          (`show_when`). Kit fields keep the old input and show the errors \
                          after a failed submit with no code of the page's own.",
                },
                Feature {
                    api: "Upload",
                    why: "Damage photos are a `Vec<Upload>` (each `image()` by content, at most \
                          six of 5 MB), stored privately and shown to staff through signed \
                          links that expire, since they show a customer's rental.",
                },
                Feature {
                    api: "Events and listeners",
                    why: "A damaged return emits `FleetRepairNeeded`; the workshop area listens \
                          and opens a work order at this store, billed to the bike's owner store \
                          when it is another. Every return emits `RentalClosed`, where the \
                          intercompany books (#245) book the revenue to the owner store and the \
                          operating store's fee. Rentals stays unaware of the workshop and the \
                          books: each area adds its own listener.",
                },
                Feature {
                    api: "Transactions",
                    why: "At the return the rental, the bike (status, location, ridden hours) \
                          and the photos' rows are written in one transaction, so a failure \
                          never leaves a closed rental with a bike still marked out (the \
                          pick-up does the same for the rental and the bike). The counter \
                          payments follow through the shared payments contract.",
                },
                Feature {
                    api: "Pricing in Rust",
                    why: "`pricing::late_fee` (per started hour after 15 minutes of grace) and \
                          `pricing::settle` (fees out of the deposit first, the rest given back, \
                          any excess paid now) are unit-tested functions; the page shows their \
                          answer for now, and the return uses the same functions, so the \
                          preview and the receipt agree.",
                },
            ],
            under_hood: "Loading: the rental (a 404 unless the person may see it, or it is \
                         out and may come back here), the customer and their pending ID \
                         document, the bike's rate for the late fee so far, the photos, the \
                         stores and `RentalRow::load`. Pick-up (`POST …/pickup`): \
                         `access::require(rentals.checkout, Operating)`, the customer must be \
                         verified, one transaction (rental active, checklist, bike `rented`), \
                         then `payments::record_counter` for the price and, when due, the \
                         deposit. Return (`POST …/return`): the photos to private storage, one \
                         transaction (late and damage fees, the deposit settled, the return \
                         store, the ridden minutes; the bike's location, hours and status, \
                         `maintenance` when damaged), the excess paid at the counter, the \
                         events, a mail and an in-app notification with the amounts to the \
                         customer, then the receipt.",
            docs: &[
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/scheduling.md#events",
                "docs/validation.md#uploads",
                "docs/ui.md#form-fields",
                "docs/ui.md#a-field-that-depends-on-another",
            ],
            sources: &[
                COUNTER,
                PRICING,
                MOD,
                "examples/bikeshop/src/app/sales/payments.rs",
                "examples/bikeshop/src/app/workshop/fleet.rs",
                "examples/bikeshop/resources/views/rentals/desk.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: what this person may do here, store by store",
                    region: "rentals.desk.handler",
                },
                Code {
                    title: "Pick-up: verified customers only, then one transaction",
                    region: "rentals.desk.handover",
                },
                Code {
                    title: "Return: one transaction, then `FleetRepairNeeded` and `RentalClosed`",
                    region: "rentals.desk.return",
                },
            ],
        },
        Explanation {
            route: "rentals.receipt",
            path: "/staff/rentals/{rental}/receipt",
            title: "Rental receipt",
            purpose: "What the customer paid and got back, the payments recorded, how the \
                      rental is booked between the bike's owner store and the operating \
                      store (revenue and fees to the owner, the operating store's fee at its \
                      rate), and where the bike should go next when it came back elsewhere.",
            who: "Cashiers, after a return; managers checking a rental.",
            audience: &[Audience::Cashier, Audience::Manager],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "Query<T>",
                    why: "The payments are rows of the shared `payments` table pointing at the \
                          rental by `payable_type` + `payable_id` (the same table sales and \
                          the workshop use), read with a plain query: `Payment::query().where_(Payment::PAYABLE_ID.eq(…))` \
                          on both columns and the paid status. One kind of parent is needed \
                          here, so no `Morph` loader is.",
                },
                Feature {
                    api: "StoreAttr::Owner",
                    why: "The books follow the **owner** store copied onto the rental at \
                          booking (so moving the bike later never changes them): the owner \
                          earns the revenue and fees, the operating store its fee \
                          (`fee_rate_bp`, 20 % by default) when the two differ. #245 writes \
                          those entries on `RentalClosed`; the receipt shows the same \
                          numbers.",
                },
                Feature {
                    api: "money filter",
                    why: "Integers in the smallest unit, formatted in the visitor's locale by \
                          `money` and `entry(…, format=\"money\")`, so no amount is formatted \
                          by hand.",
                },
            ],
            under_hood: "The rental, its paid payments, the bike, the store it was placed at \
                         (the latest placement that moved it, else the owner), the three \
                         stores, then `RentalRow::load`: about ten queries, nothing written. \
                         When the bike came back elsewhere, the page says where to send it.",
            docs: &[
                "docs/relations.md#more-of-the-query-builder",
                "docs/types.md#money",
            ],
            sources: &[
                COUNTER,
                "examples/bikeshop/src/app/multistore/books.rs",
                "examples/bikeshop/src/app/multistore/model.rs",
                "examples/bikeshop/resources/views/rentals/receipt.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: the payments, and the fee the operating store earns",
                    region: "rentals.receipt.handler",
                },
                Code {
                    title: "Template: how the rental is booked between two stores",
                    region: "rentals.receipt.template",
                },
            ],
        },
        Explanation {
            route: "rentals.fleet",
            path: "/staff/fleet",
            title: "Fleet board",
            purpose: "A store's rental bikes at a glance: available, reserved, out, overdue, \
                      in maintenance; who owns each and where it stands; the hours to its \
                      next service. Tabs show the bikes the store owns and has here, the ones \
                      other stores placed here, and its own bikes placed elsewhere.",
            who: "Store managers and staff (`fleet.view`).",
            audience: &[Audience::Manager, Audience::Staff, Audience::Cashier],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "renox::grid",
                    why: "Columns, filters, sorting, search (by frame number) and paging from \
                          one description in Rust (`fleet_grid`), kept in the address so a view \
                          can be shared; the status `select` column filters by status. A \
                          hand-made table would need all of that written per page.",
                },
                Feature {
                    api: "Grid::poll",
                    why: "`poll(30)` reloads the board every 30 seconds while the tab is \
                          visible and nobody is editing, selecting or filtering, so a bike \
                          returned at the counter or marked overdue by the scheduled task \
                          appears by itself, without a socket or a server push.",
                },
                Feature {
                    api: "Grid::cards_on_mobile",
                    why: "On a phone each bike is a card with the columns marked `mobile()`: \
                          frame number, model, status and location, instead of a table that \
                          scrolls sideways.",
                },
                Feature {
                    api: "Column::badges",
                    why: "Statuses are badges whose tone and word agree (overdue danger, \
                          maintenance warning…), so the board can be scanned at a glance and \
                          still read without colour.",
                },
                Feature {
                    api: "Column::related",
                    why: "Owner and location are the stores' names, read by subqueries in the \
                          page's one query, and they sort and filter like the model's own \
                          columns: no join written by hand, no query per row.",
                },
                Feature {
                    api: "scopes_with",
                    why: "The grid starts from `access::visible::<RentalBike>(fleet.view)` \
                          (owner **or** location among the person's stores), then the tab \
                          narrows it to the active store by owner and location. The \
                          permission limit is in the query, so paging and counts are right.",
                },
                Feature {
                    api: "UI kit: link_tabs",
                    why: "The tabs (all, mine here, placed here by others, mine elsewhere) are \
                          links with their counts (`?view=`), so each is an address of its \
                          own that can be bookmarked, unlike panels switched in the page.",
                },
            ],
            under_hood: "One query for the page (with the related store names as \
                         subqueries), one for the total, two for the models' names, three \
                         counts for the tabs. Nothing is written; the board changes through \
                         the counter, placements and the `rentals:watch` task.",
            docs: &[
                "docs/grid.md#polling",
                "docs/grid.md#cards-on-phones",
                "docs/grid.md#columns-from-other-tables",
                "docs/grid.md#options-every-column-takes",
                "docs/authorization.md#lists-scopes_with",
                "docs/ui.md#navigation-and-page-structure",
            ],
            sources: &[
                FLEET,
                POLICY,
                "examples/bikeshop/resources/views/rentals/fleet.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Grid: `renox::grid` with related columns, cards on phones and `poll`",
                    region: "rentals.fleet.grid",
                },
                Code {
                    title: "Handler: within what the person may see, narrowed by the tab",
                    region: "rentals.fleet.handler",
                },
                Code {
                    title: "Template: the grid with two cells drawn by the page",
                    region: "rentals.fleet.template",
                },
            ],
        },
        Explanation {
            route: "rentals.fleet.show",
            path: "/staff/fleet/{bike}",
            title: "A rental bike",
            purpose: "One bike: its owner store and where it stands, what each of those \
                      stores may do with it, the hours to its next service, and its history: \
                      rentals, placements at other stores, work orders.",
            who: "Staff of the bike's owner store and of the store where it stands.",
            audience: &[Audience::Manager, Audience::Staff],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "has_permission_in",
                    why: "ABAC: renting it out, taking it back and repairing it are checked in \
                          the **location** store; its rates, placing it elsewhere and retiring \
                          it in the **owner** store. The page lists each action with the store \
                          it is checked in and whether the person may do it, worked out by the \
                          same `access::can` the handlers use, so the page can't disagree with \
                          what the buttons elsewhere allow.",
                },
                Feature {
                    api: "relations::belongs_to",
                    why: "Its latest 20 rentals come with their customers and stores in a \
                          fixed number of queries (`RentalRow::load`), not one per rental.",
                },
                Feature {
                    api: "UI kit: infolist + tabs",
                    why: "The bike's details as an `infolist`; its history (rentals, \
                          placements, work orders) in the kit's `tabs`, which switch panels \
                          in the page with the keyboard too, so three lists don't make one \
                          long page.",
                },
            ],
            under_hood: "`access::find::<RentalBike>` answers 404 to staff of a third store. \
                         Then the store it should go back to (its latest placement), the \
                         stores, the latest 20 rentals with their relations, the latest 20 \
                         placements and work orders, and the model's name. Nothing is \
                         written.",
            docs: &[
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/relations.md#a-page-of-rows-with-their-relations-no-n1",
                "docs/ui.md#infolists-read-only-details",
                "docs/ui.md#buttons-surfaces-and-other-parts",
            ],
            sources: &[
                FLEET,
                POLICY,
                "examples/bikeshop/resources/views/rentals/fleet_show.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: each action checked in the store it depends on",
                    region: "rentals.fleet.show.handler",
                },
                Code {
                    title: "Template: the answers as a kit `<rx-list>` with badges",
                    region: "rentals.fleet.show.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![
        NotAPage {
            route: "rentals.customers",
            reason: "JSON options for the walk-in form's customer select (renox::select)",
        },
        NotAPage {
            route: "rentals.receipt.pdf",
            reason: "the receipt as a PDF file (renox-pdf), the same bytes as the return mail's attachment",
        },
        NotAPage {
            route: "rentals.photo",
            reason: "a redirect to a counter photo's signed temporary URL",
        },
        NotAPage {
            route: "rentals.identities.photo",
            reason: "a redirect to an ID photo's signed temporary URL",
        },
    ]
}
