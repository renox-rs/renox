//! "About this page" entries for the multistore area's pages (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

const MOD: &str = "examples/bikeshop/src/app/multistore/mod.rs";
const MODEL: &str = "examples/bikeshop/src/app/multistore/model.rs";
const HELP: &str = "examples/bikeshop/src/app/multistore/help.rs";
const PLACEMENTS: &str = "examples/bikeshop/src/app/multistore/placements.rs";
const BOOKS: &str = "examples/bikeshop/src/app/multistore/books.rs";
const INTERCOMPANY: &str = "examples/bikeshop/src/app/multistore/intercompany.rs";
const SETTLEMENTS: &str = "examples/bikeshop/src/app/multistore/settlements.rs";
const AUDIT: &str = "examples/bikeshop/src/app/multistore/audit.rs";
const ACTIVE_STORE: &str = "examples/bikeshop/src/app/access/active_store.rs";
const POLICY: &str = "examples/bikeshop/src/app/access/policy.rs";
const TESTS: &str = "examples/bikeshop/tests/multistore.rs";
const BROWSER: &str = "tests/browser/bikeshop-multistore.test.mjs";

const AUDITED: Feature = Feature {
    api: "Audit module",
    why: "Renox's `Audit` module keeps `audit_logs`; `multistore::audit::record` adds the store \
          the person was working in and the roles they held **there** (from `assignments`, never \
          a role-name check), so \"who did this, as what, where\" has an answer even for someone \
          with roles in two stores.",
};

const SCOPES: Feature = Feature {
    api: "scopes_with",
    why: "Lists start from `access::visible::<M>(permission)`: Renox's `scopes_with` turns the \
          person's roles into the stores they may see that in, and `Scopes::apply` filters on \
          the record's store columns (a row counts when **either** store is one of them). One \
          filter in SQL instead of a check per row, so paging and totals stay right; the \
          owner's global role sees everything, without any check on a role's name.",
};

const POSTING: Feature = Feature {
    api: "Events and listeners",
    why: "The books follow the business without the other areas knowing them: the module \
          listens to `rentals::RentalClosed` and `workshop::status::WorkOrderClosed` \
          (`Registry::listen`) and books the entries in `multistore::books`, the one place that \
          writes them (sales call it for consigned goods, the stock take for losses).",
};

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "multistore.help",
            path: "/staff/help",
            title: "Help between stores",
            purpose: "A store short of people borrows someone from another store for some days. \
                      The page has two lists, help asked of the active store (to approve or \
                      refuse) and help it asked for, each with its status and hours so far; \
                      either store can end a help early and the helped store logs the hours. An \
                      approved request gives the helper a role **in the helped store, between \
                      two dates**: their access there starts and ends by itself. The store \
                      switcher in the top bar is how the helper works there: it offers every \
                      store where they hold a role today.",
            who: "Store managers (`staff.help`) and the owner.",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "assign_role_in(…).from(…).until(…)",
                    why: "Approving calls `helper.assign_role_in(db, role, &scope)\
                          .from(start).until(end)`, the scope being the helped store's \
                          (`Scope::of_id::<Store>(id)`, Renox #244). Nothing has to run at the \
                          end: the role stops counting, so `scopes_with` no longer lists the \
                          store in the helper's switcher and its pages answer 403, with no cron \
                          job to forget. Ending early moves the end to now (or removes the role \
                          with `remove_role_in`, if it hadn't started).",
                },
                Feature {
                    api: "renox::context",
                    why: "The store switcher (`access::active_store`) keeps the chosen store in \
                          the session and calls `permissions::set_scope` on every staff request, \
                          so the helper's rights come from the roles **in that store only**: a \
                          manager of North working in South as staff has staff rights there.",
                },
                Feature {
                    api: "has_permission_in",
                    why: "Approving and refusing need `staff.help` in the **lending** store, \
                          withdrawing in the **asking** store, ending in either: \
                          `access::can_in(user, STAFF_HELP, store)` per action.",
                },
                Feature {
                    api: "notify",
                    why: "The other store's people with `staff.help` hear of a request, an \
                          approval, a refusal or an early end in the app (the bell, no mail: \
                          they are at work in the app anyway); the helper gets a mail and a \
                          notification with the store and the dates, since they may not be \
                          looking.",
                },
                Feature {
                    api: "UI kit: action_sheet + date_picker",
                    why: "Logging a day's hours is a small form in a sheet on the row (day, \
                          hours, note), so the manager stays on the list; a day outside the \
                          help's dates is refused.",
                },
                AUDITED,
                Feature {
                    api: "Redirect::route",
                    why: "Every form on the page (ask, approve, refuse, end early, log hours) \
                          answers with `Redirect::route(\"multistore.help\", &[])`: the \
                          redirect names the route rather than a path written by hand, so \
                          moving the page to another address cannot leave a form sending people \
                          to a 404.",
                },
            ],
            under_hood: "Loading: up to 100 requests of the store, then the helpers' staff rows \
                         and users, the stores and the hours, five queries whatever the number. \
                         Approving, refusing and withdrawing move the status with `UPDATE … \
                         WHERE status = ?` (pressing twice acts once, the second gets a 409); \
                         approving and ending give or shorten the dated role; each action writes \
                         an audit row, and all but withdrawing and logging hours notify.",
            docs: &[
                "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
                "docs/authorization.md#managing-assignments",
                "docs/mail.md#database-notifications",
                "docs/ui.md#actions",
                "docs/authorization.md#sensitive-actions-and-the-audit-trail",
            ],
            sources: &[
                HELP,
                ACTIVE_STORE,
                AUDIT,
                "examples/bikeshop/resources/views/multistore/help/index.html",
                "examples/bikeshop/resources/views/components/store_switcher.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: help asked by and of the active store, in one query",
                    region: "multistore.help.handler",
                },
                Code {
                    title: "Approve: a role in the helped store, between two dates",
                    region: "multistore.help.approve",
                },
                Code {
                    title: "Template: the kit's `<rx-row-actions>` with an `<rx-action-sheet>` for hours",
                    region: "multistore.help.template",
                },
            ],
        },
        Explanation {
            route: "multistore.help.create",
            path: "/staff/help/new",
            title: "Ask another store for help",
            purpose: "Ask another store to lend someone: the person (from that store's staff), \
                      the role they'll have here (a role from the catalogue, never typed), the \
                      first and last day, and why.",
            who: "Store managers (`staff.help` in the store asking).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Valid<T> + after hook",
                    why: "`HelpForm`'s rules check the fields (the role `one_of` the \
                          catalogue's store roles, so no role can be typed in); its `after` \
                          hook checks what needs two fields or the database: the days are in \
                          order and not past, the person works at the lending store, and that \
                          store isn't the one asking. Errors come back next to the fields like \
                          any other rule's.",
                },
                Feature {
                    api: "UI kit: date_picker",
                    why: "The kit's date picker (Cally in a popover, `min` today) for the two \
                          days, the same on every browser. The handler turns them into moments \
                          at midnight in `APP_TIMEZONE` (the last day ends at the next \
                          midnight), so the role starts and ends on the shop's own calendar.",
                },
            ],
            under_hood: "Loading: the stores, the lending store's active staff and their users \
                         (the lending store is picked by a small GET form above). Sending: \
                         `Valid<HelpForm>` (its `after` hook reads the staff row), one insert, \
                         an audit row, an in-app notice to the lending store's managers.",
            docs: &[
                "docs/validation.md#hooks-prepare-authorize-after",
                "docs/ui.md#form-fields",
                "docs/types.md#dates-and-times",
            ],
            sources: &[
                HELP,
                "examples/bikeshop/resources/views/multistore/help/new.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: the lending store's people, by `belongs_to`",
                    region: "multistore.help.create.handler",
                },
                Code {
                    title: "Form: `Validate` rules, then an `after` hook that reads the database",
                    region: "multistore.help.create.form",
                },
                Code {
                    title: "Template: `<rx-form-grid>` with a searchable select and date pickers",
                    region: "multistore.help.create.template",
                },
            ],
        },
        Explanation {
            route: "multistore.help.hours",
            path: "/staff/help/hours",
            title: "Hours helped",
            purpose: "Hours helped per person and store, for reports. Help between stores is \
                      never charged (the owner's decision 2): these hours are counted, not \
                      booked.",
            who: "Store managers and the owner (`staff.help`).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "scopes_with",
                    why: "`permissions::scopes_with::<Store>(\"staff.help\")` gives the stores \
                          the person holds `staff.help` in, and the query keeps the hours \
                          worked in those stores **or** by people whose home store is one of \
                          them: a manager sees the help their store received and the help \
                          their own people gave elsewhere; the owner's global role \
                          (`Scopes::All`) sees every store, with no role name checked.",
                },
                Feature {
                    api: "Query<T>",
                    why: "One grouped query (`group_by` + `select_as`: `COUNT(DISTINCT \
                          worked_on)`, `SUM(minutes)`) per person and store: the database adds \
                          up, rather than every hour row being loaded into Rust.",
                },
                Feature {
                    api: "UI kit: stats + table",
                    why: "The total hours and the number of lines as the kit's stats, the lines \
                          as its table: a report page made of kit parts, nothing hand-built.",
                },
            ],
            under_hood: "Four queries: the grouped hours, the staff rows, their users, the stores.",
            docs: &[
                "docs/authorization.md#lists-scopes_with",
                "docs/relations.md#more-of-the-query-builder",
            ],
            sources: &[
                HELP,
                "examples/bikeshop/resources/views/multistore/help/hours.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Query: hours per person and store, grouped in one statement",
                    region: "multistore.help.hours.query",
                },
                Code {
                    title: "Logging: the helped store records a day, within the request's dates",
                    region: "multistore.help.hours.log",
                },
                Code {
                    title: "Template: the kit's `<rx-stats>` over an `<rx-table>`",
                    region: "multistore.help.hours.template",
                },
            ],
        },
        Explanation {
            route: "multistore.placements",
            path: "/staff/placements",
            title: "Bike placements",
            purpose: "Rental bikes placed at another store: requested by the store that wants \
                      them, approved and moved by the owner store, called back by it. The bike \
                      stays the owner's (its books, rates, retiring it) while its location \
                      changes (renting it out is the location's business). Bikes brought back at \
                      a store other than their home get a \"send back to …\" task.",
            who: "Staff of both stores (`fleet.view`); managers act (`fleet.place`).",
            audience: &[Audience::Staff, Audience::Manager, Audience::Owner],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "has_permission_in",
                    why: "Owner, location and operating store are different attributes: \
                          approving, moving and recalling check `fleet.place` in the **owner** \
                          store (`access::require(…, StoreAttr::Owner, …)`), sending a bike \
                          back checks it in its **location** (or owner) store, renting it out \
                          (the rentals area) in its location.",
                },
                Feature {
                    api: "Db::begin_immediate + lock_for_update",
                    why: "A recall and a booking of the same bike can't both win: the recall \
                          locks the bike's row and looks for open rentals in one transaction, \
                          as the booking does, so one waits for the other and the second is \
                          refused. A bike out with a customer is never called away from under \
                          them.",
                },
                SCOPES,
                AUDITED,
            ],
            under_hood: "Loading: the store's placements, their bikes, the bikes owned here or \
                         standing here and their homes (the latest `moved` placement of all of \
                         them in one query), the variants' names and the stores. Deciding: a \
                         status guard (`UPDATE … WHERE status = 'requested'`). Moving, recalling \
                         and sending back: a transaction with the bike's row locked, then its \
                         location. Each writes an audit row; all but sending back notify the \
                         other store in the app.",
            docs: &[
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/relations.md#more-of-the-query-builder",
                "docs/authorization.md#sensitive-actions-and-the-audit-trail",
            ],
            sources: &[
                PLACEMENTS,
                POLICY,
                "examples/bikeshop/src/app/rentals/model.rs",
                "examples/bikeshop/resources/views/multistore/placements/index.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: placements to and from the store, and bikes away from home",
                    region: "multistore.placements.handler",
                },
                Code {
                    title: "Move: the bike's row locked in one transaction",
                    region: "multistore.placements.move",
                },
                Code {
                    title: "Template: each placement with the actions its state allows",
                    region: "multistore.placements.template",
                },
            ],
        },
        Explanation {
            route: "multistore.placements.create",
            path: "/staff/placements/new",
            title: "New bike placement",
            purpose: "Place one of our bikes standing at home at another store (approved at \
                      once: it's ours), or ask another store for one of its bikes (it decides).",
            who: "Managers (`fleet.place` in the active store).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::Rent,
            features: &[
                Feature {
                    api: "#[derive(Validate)]",
                    why: "`PlacementForm`'s rules are attributes on its fields (the direction \
                          `one_of` place or ask), short enough not to need an `impl Validate`; \
                          the handler then checks what needs the database: the bike stands free \
                          at its owner store, which isn't the other store.",
                },
                Feature {
                    api: "UI kit: toggle_buttons + select",
                    why: "Direction and other store are a small GET form, so the bike list is \
                          the right owner's without any JavaScript; the bike is the kit's \
                          searchable select, quick to use with up to 200 bikes.",
                },
            ],
            under_hood: "Loading: the stores, up to 200 free bikes of the owner and their names. \
                         Saving: `Valid<PlacementForm>`, the bike checked, one insert, an audit \
                         row, a notice to the other store.",
            docs: &[
                "docs/validation.md#derivevalidate",
                "docs/ui.md#form-fields",
            ],
            sources: &[
                PLACEMENTS,
                ACTIVE_STORE,
                "examples/bikeshop/resources/views/multistore/placements/new.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: our bikes to place, or theirs to ask for",
                    region: "multistore.placements.create.handler",
                },
                Code {
                    title: "Form: `#[derive(Validate)]` with `one_of`",
                    region: "multistore.placements.create.form",
                },
                Code {
                    title: "Store: our own bike is approved at once, theirs is asked for",
                    region: "multistore.placements.create.store",
                },
            ],
        },
        Explanation {
            route: "multistore.books",
            path: "/staff/books",
            title: "Books between stores",
            purpose: "Who owes whom for work done for each other. A rental of North's bike \
                      served by South: South owes North the price, North owes South its fee; \
                      late and damage fees go to the owner; consigned goods sold at another \
                      store: the seller owes the owner, minus its fee; a fleet repair by \
                      another store's workshop: the owner pays it. The open balance per pair of \
                      stores, the active store's position, and every entry in a grid.",
            who: "Store managers (`intercompany.view`) and the owner.",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                POSTING,
                Feature {
                    api: "renox::grid",
                    why: "The entries in one `Grid`: filter by kind (a select column), sort, \
                          group by kind, `Column::summary(Summary::Sum)` under the amounts, the \
                          two stores as `Column::related` subqueries, CSV/Excel exports for the \
                          accountant, cards on phones. A ledger is exactly what the grid is for, \
                          so the page writes no table, filter or export code of its own.",
                },
                SCOPES,
                Feature {
                    api: "Rate copied on the entry",
                    why: "Each fee entry keeps the fee rate in force when it was booked \
                          (`fee_rate_bp`), read in the booking's transaction: changing a \
                          store's rate never rewrites history.",
                },
            ],
            under_hood: "One grouped query for the open balances (`debtor`, `creditor`, \
                         `SUM(amount)` where not settled), netted per pair in Rust \
                         (`intercompany::balances`; the positions of all stores add up to \
                         zero), then the grid's count, page, summaries and store names.",
            docs: &[
                "docs/scheduling.md#events",
                "docs/grid.md#summaries-and-groups",
                "docs/grid.md#exports",
                "docs/authorization.md#lists-scopes_with",
            ],
            sources: &[
                INTERCOMPANY,
                BOOKS,
                MODEL,
                MOD,
                "examples/bikeshop/resources/views/multistore/books/index.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: the grid of entries, and the open sums per store pair",
                    region: "multistore.books.handler",
                },
                Code {
                    title: "Netting: each pair's entries, in the direction of the debt",
                    region: "multistore.books.netting",
                },
                Code {
                    title: "Template: your position, the open balances, then the grid",
                    region: "multistore.books.template",
                },
            ],
        },
        Explanation {
            route: "multistore.settlements",
            path: "/staff/books/settlements",
            title: "Monthly settlements",
            purpose: "On the 1st of each month, last month's entries are netted per pair of \
                      stores into one settlement, mailed to both stores' managers and the owner, \
                      and confirmed by both stores once the money has moved.",
            who: "Store managers (`intercompany.view`) and the owner.",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Schedule::monthly_on",
                    why: "`monthly_on(1, \"02:00\", \"books:settle\", …)`: `schedule:list` shows \
                          it, `schedule:run books:settle` runs it now, several servers run it \
                          once (each run is claimed first). The work is a plain function, \
                          `settlements::settle_month`, safe to run twice; the tests call it \
                          directly, and also travel to the 1st (`TestApp::travel`) and run the \
                          task with `Kernel::run_scheduled`.",
                },
                Feature {
                    api: "Queue",
                    why: "The statements go out as a **batch** (`state.queue.batch(…)`, one \
                          `SendStatement` job per store pair, `allow_failures`): the monthly \
                          task ends as soon as the books are settled, one slow or failing mail \
                          doesn't hold the others, and the batch's progress is counted in \
                          `job_batches`.",
                },
                SCOPES,
            ],
            under_hood: "Three queries a page: the count, the page (`access::visible`) and the \
                         store names. The monthly task: one transaction that sums the unsettled \
                         entries of the month per pair, creates or reuses the pair's settlement, \
                         points the entries at it and stores the net; then the batch.",
            docs: &[
                "docs/scheduling.md#scheduled-tasks",
                "docs/scheduling.md#testing-a-task",
                "docs/queue.md#chains-and-batches",
            ],
            sources: &[
                SETTLEMENTS,
                MODEL,
                "examples/bikeshop/resources/views/multistore/books/settlements.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Task: on the 1st, net last month per pair, then mail the statements",
                    region: "multistore.settlements.month",
                },
                Code {
                    title: "Handler: the statements the person may see, paginated",
                    region: "multistore.settlements.handler",
                },
                Code {
                    title: "Template: an `<rx-table>` with badges per side",
                    region: "multistore.settlements.template",
                },
            ],
        },
        Explanation {
            route: "multistore.settlements.show",
            path: "/staff/books/settlements/{settlement}",
            title: "A two-party statement",
            purpose: "One month between two stores: what each owes the other by kind, the net \
                      (who pays whom), every entry with its document, and each store's \
                      confirmation. The paying store confirms it paid, the other that it was \
                      paid; with both, the settlement is settled.",
            who: "Managers of the two stores and the owner.",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "has_permission_in",
                    why: "Each side's button needs `intercompany.settle` **in that side's \
                          store** (`access::require` on the settlement's debtor or creditor \
                          store). The owner, whose global role holds it everywhere, can confirm \
                          for either; nobody can confirm for a store they don't hold it in.",
                },
                Feature {
                    api: "Mail",
                    why: "The `SendStatement` job mails this statement (`mail_view` on Renox's \
                          mail layout and its `table` component) to the people who see either \
                          store's books, with the entries as a CSV attachment (`Mail::attach`): \
                          each store's accountant has the detail without logging in.",
                },
                AUDITED,
            ],
            under_hood: "Loading: the settlement (`access::find`, 404 for other stores), its \
                         entries, grouped by side and kind in Rust, the store names. Confirming: \
                         `UPDATE … WHERE status = 'open' AND <side>_confirmed_at IS NULL`, then \
                         settled when both sides are, and an audit row.",
            docs: &[
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/mail.md#sending-a-mail",
                "docs/mail.md#mail-views",
            ],
            sources: &[
                SETTLEMENTS,
                POLICY,
                MODEL,
                "examples/bikeshop/resources/views/multistore/books/statement.html",
                "examples/bikeshop/resources/views/mail/multistore/statement.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: `access::find`, then each side's lines by kind",
                    region: "multistore.settlements.show.handler",
                },
                Code {
                    title: "Confirm: a guarded update; both sides settle it",
                    region: "multistore.settlements.show.confirm",
                },
                Code {
                    title: "Template: one macro draws each store's side",
                    region: "multistore.settlements.show.template",
                },
            ],
        },
        Explanation {
            route: "multistore.fees",
            path: "/staff/books/fees",
            title: "Fee rates",
            purpose: "The fee each store earns for work done for another store (20 % by \
                      default): renting out another store's bike, selling its consigned goods. \
                      Only the owner changes a rate; every change is audited and listed here, \
                      and entries already booked keep the rate they were booked with.",
            who: "Managers see them; the owner (`settings.fees`) changes them.",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Routes::require_permission",
                    why: "The books' routes need `intercompany.view` (one \
                          `require_permission` on the group), so managers can read the rates. \
                          Changing one also checks `user.has_permission(\"settings.fees\")` in \
                          the handler, which only the owner's global role grants: a permission, \
                          never a role's name. The page hides the button for everyone else.",
                },
                Feature {
                    api: "UI kit: action_sheet",
                    why: "Each rate is changed in a sheet sent with htmx, next to the rate it \
                          changes: an error stays in the sheet, success reloads with a toast. \
                          No separate edit page for one number.",
                },
                AUDITED,
                Feature {
                    api: "renox-blocks",
                    why: "The changes are the `history` block over the audit log's \
                          `store.fee_rate_changed` rows (old and new rate, who, when): the audit \
                          trail already holds them, so no extra table is kept.",
                },
            ],
            under_hood: "Loading: the stores, the latest 200 audit rows (the last 20 rate \
                         changes kept) and their users. Changing: `Found<Store>`, \
                         `Valid<FeeForm>` (a percentage from 0 to 50, stored in basis points), \
                         one update, an audit row with the old and the new rate.",
            docs: &[
                "docs/routing.md#guards",
                "docs/authorization.md#roles-and-permissions",
                "docs/ui.md#actions",
                "docs/authorization.md#sensitive-actions-and-the-audit-trail",
                "docs/types.md#money",
            ],
            sources: &[
                INTERCOMPANY,
                MOD,
                "examples/bikeshop/resources/views/multistore/books/fees.html",
                "crates/renox-blocks/views/blocks.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: `Found<Store>` and `Valid<FeeForm>`, the change audited",
                    region: "multistore.fees.handler",
                },
                Code {
                    title: "Template: an `<rx-action-sheet>` per store to change its rate",
                    region: "multistore.fees.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![]
}
