//! "About this page" entries for the stock area's pages (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

const MOD: &str = "examples/bikeshop/src/app/stock/mod.rs";
const MODEL: &str = "examples/bikeshop/src/app/stock/model.rs";
const LEDGER: &str = "examples/bikeshop/src/app/stock/ledger.rs";
const LEVELS: &str = "examples/bikeshop/src/app/stock/levels.rs";
const TAKE: &str = "examples/bikeshop/src/app/stock/take.rs";
const CONSIGNMENT: &str = "examples/bikeshop/src/app/stock/consignment.rs";
const PURCHASING: &str = "examples/bikeshop/src/app/stock/purchasing.rs";
const IMPORT: &str = "examples/bikeshop/src/app/stock/import.rs";
const REORDER: &str = "examples/bikeshop/src/app/stock/reorder.rs";
const FLEET: &str = "examples/bikeshop/src/app/stock/fleet.rs";
const NOTIFY: &str = "examples/bikeshop/src/app/stock/notify.rs";
const ASSETS: &str = "examples/bikeshop/resources/views/components/stock_assets.html";
const SHIPMENT_STATUS: &str =
    "examples/bikeshop/resources/views/components/stock_shipment_status.html";
const STEPS: &str = "examples/bikeshop/resources/views/components/stock_steps.html";
const BOOKS: &str = "examples/bikeshop/src/app/multistore/books.rs";
const AUDIT: &str = "examples/bikeshop/src/app/multistore/audit.rs";
const POLICY: &str = "examples/bikeshop/src/app/access/policy.rs";
const VIEW_MIGRATION: &str = "examples/bikeshop/migrations/20260101002400_add_stock_details.up.sql";
const TESTS: &str = "examples/bikeshop/tests/stock.rs";
const BROWSER: &str = "tests/browser/bikeshop-stock.test.mjs";

const ABAC: Feature = Feature {
    api: "access policy helpers",
    why: "Goods have an **owner** store (whose books) and a **location** store (where they \
          are). `access::find` gives a 404 to anyone who may see the record in neither (so ids \
          of another store's goods can't be probed), and \
          `access::require(user, permission, StoreAttr::…, &record)` checks each action in the \
          store that matters (`has_permission_in` from #244): selling and counting where the \
          goods are, writing off and recalling where they are owned. A role check alone \
          couldn't say which of the two stores the person works for.",
};

const SCOPES: Feature = Feature {
    api: "scopes_with",
    why: "Lists start from `access::visible::<M>(permission)`: Renox's `scopes_with` turns the \
          person's roles into the stores they may see that in, and `Scopes::apply` makes the \
          query \"owner store in (…) **or** location store in (…)\", so goods of North held at \
          South show up for both stores and for no one else. A global role (the owner) sees \
          everything, without any check on a role's name.",
};

const LEDGER_PATTERN: Feature = Feature {
    api: "StockMovement::record",
    why: "**Levels only change with a movement, in the same transaction.** \
          `StockMovement::record` writes the ledger row and upserts the level together; \
          `ledger::take` takes goods out only if they are there (`UPDATE … WHERE on_hand - \
          reserved >= ?`), so two people taking the last unit can't both succeed. \
          `ledger::mismatches` proves every level equals the sum of its movements (the tests \
          run it after concurrent sales).",
};

const AUDITED: Feature = Feature {
    api: "Audit module",
    why: "Renox's `Audit` module keeps `audit_logs`; `multistore::audit::record` adds the store \
          the person was working in and the roles they held **there** (from `assignments`, \
          never a role-name check), so \"who did this, as what, where\" has an answer.",
};

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "stock.index",
            path: "/staff/stock",
            title: "Stock",
            purpose: "The active store's stock, one row per variant × owner store × location \
                      store: on hand, reserved, available, the reorder level and the value at \
                      cost. Tabs keep the store's own goods apart from goods it holds for other \
                      stores (consigned here), its own goods at other stores, and what is under \
                      its reorder level. Each row opens its ledger.",
            who: "Everyone on a store's staff (`stock.view`): the counter checks what is \
                  available, managers watch the low tab and take the export.",
            audience: &[Audience::Staff, Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "renox::grid",
                    why: "One `Grid` gives search, filters per column, sorting, column choices, \
                          paging and cards on phones, so a stock list of thousands of rows \
                          needed no hand-written table, filter form or pager. \
                          `Column::summary(Summary::Sum)` puts the totals of on hand, reserved, \
                          available and value under the columns (over every filtered row, not \
                          the page), `.groups(&[\"category\"])` adds \"Group by category\" with \
                          subtotals, and `.exports()` gives CSV, Excel (`xlsx` feature) and a \
                          print page from the same handler and the same filters.",
                },
                Feature {
                    api: "Database view as a model",
                    why: "The grid filters, sums and groups a model's own columns, so the rows \
                          come from a database view, `stock_overview` (the level joined with \
                          its variant, product and category, with `available` and \
                          `value_at_cost` worked out), read through `#[derive(Model)] \
                          StockRow`: no copy of the numbers to keep in step. The owner and \
                          location store names are `Column::related` columns, read in the \
                          page's own query.",
                },
                SCOPES,
                Feature {
                    api: "UI kit: link_tabs",
                    why: "The four views are links (`?view=own|held|away|low`), so each is a \
                          shareable address and the grid's own filters stay apart from it; \
                          the counts on the tabs are four small `count` queries, the same \
                          whatever the store holds.",
                },
                Feature {
                    api: "push + stack, csp_nonce()",
                    why: "The stock pages add their script with `<rx-push stack=\"scripts\" \
                          once=\"stock-js\">` from `<app-stock-assets/>` \
                          (`components/stock_assets.html`): the layout prints it where its \
                          `<rx-outlet stack=\"scripts\"/>` is, once however many parts ask \
                          for it, and only on the pages that use it. The `<rx-script>` inside \
                          carries the page's nonce, so it runs under `CSP=strict` too.",
                },
            ],
            under_hood: "One request: the tab's query (`access::visible` then the active store's \
                         owner/location condition), the grid's count, page (with the store \
                         names) and summaries, and four counts for the tabs. With \
                         `?export=csv|xlsx|print` the same handler answers with the file \
                         instead (`Grid::export`), over every filtered row.",
            docs: &[
                "docs/grid.md#summaries-and-groups",
                "docs/grid.md#exports",
                "docs/grid.md#columns-from-other-tables",
                "docs/authorization.md#lists-scopes_with",
            ],
            sources: &[
                LEVELS,
                MODEL,
                VIEW_MIGRATION,
                "examples/bikeshop/resources/views/stock/index.html",
                ASSETS,
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Grid: `renox::grid` with sums, groups, exports and phone cards",
                    region: "stock.index.grid",
                },
                Code {
                    title: "Handler: each tab a scoped query, the same one for the page and the export",
                    region: "stock.index.handler",
                },
                Code {
                    title: "Template: `<rx-link-tabs>` with counts over the kit's `<rx-grid>`",
                    region: "stock.index.template",
                },
            ],
        },
        Explanation {
            route: "stock.ledger",
            path: "/staff/stock/{level}",
            title: "Stock level ledger",
            purpose: "Why a number is what it is: one variant owned by one store at one store, \
                      and every movement that made it, newest first, with the level after each, \
                      who did it and a link to the document that caused it (an order, a work \
                      order, a consignment shipment, a purchase order, a fleet bike). Also what \
                      each store may do with these goods, and the owner's write-off.",
            who: "Staff of the owner store and of the location store; nobody else (a 404).",
            audience: &[Audience::Staff, Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                LEDGER_PATTERN,
                Feature {
                    api: "Morph",
                    why: "A movement points at what caused it with `reference_type` + \
                          `reference_id`, a polymorphic relation (`REFERENCE: Morph`). \
                          `Morph::parents::<Order, _>` loads the orders of a page of movements \
                          in one query, then the work orders, shipments, purchase orders and \
                          bikes: five queries whatever the page holds, where a lookup per row \
                          would cost 25. One pair of columns serves every kind of document, \
                          so a new kind needs no migration.",
                },
                ABAC,
                Feature {
                    api: "UI kit: action_sheet",
                    why: "The write-off is a form in a sheet sent with htmx, so it needs no \
                          page of its own: a validation error stays in the sheet, success \
                          brings the ledger back with a toast. It is only drawn for people who \
                          may write off in the **owner** store (the handler checks it again).",
                },
                AUDITED,
            ],
            under_hood: "Loading: the level (`access::find`, 404 unless seen), its variant and \
                         product, the store names, a page of movements, the sum of the newer \
                         ones (to show the running level), the staff and their users \
                         (`belongs_to`), and the source documents. Writing off: \
                         `Valid<WriteOffForm>`, then `ledger::take` with an `adjustment` in one \
                         transaction (a 409 if the units aren't there), then an audit row.",
            docs: &[
                "docs/relations.md#polymorphic-relations",
                "docs/relations.md#more-of-the-query-builder",
                "docs/ui.md#actions",
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/authorization.md#sensitive-actions-and-the-audit-trail",
            ],
            sources: &[
                LEVELS,
                LEDGER,
                MODEL,
                POLICY,
                AUDIT,
                "examples/bikeshop/resources/views/stock/ledger.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: `access::find`, then what each store may do with these goods",
                    region: "stock.ledger.handler",
                },
                Code {
                    title: "Query: the movements' documents with `Morph::parents`, one query per kind",
                    region: "stock.ledger.sources",
                },
                Code {
                    title: "Action: only the owner store writes goods off, in a transaction",
                    region: "stock.ledger.write_off",
                },
            ],
        },
        Explanation {
            route: "stock.take",
            path: "/staff/stock/take",
            title: "Stock take",
            purpose: "Counting a shelf: one category at the active store, its own goods and the \
                      goods it holds for other stores listed apart. Counted quantities that \
                      differ from the books become adjustments with a reason; consigned goods \
                      found short are owed to their owner store at cost, and that store is told.",
            who: "Managers (`stock.adjust` in the store being counted).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Valid<T>",
                    why: "The sheet is one form with nested names (`lines[3][level]`, \
                          `lines[3][counted]`), which `Valid<TakeForm>` reads as a list of \
                          `TakeLine`; `v.nested(\"lines\", …)` checks each row and keys its \
                          errors `lines.3.counted`, where the kit shows them. One form for the \
                          whole shelf means one save and one transaction, not a request per \
                          row. A blank count leaves the line alone.",
                },
                Feature {
                    api: "Routes::require_permission",
                    why: "The routes need `stock.adjust` **in the active store**: the store \
                          switcher sets the scope (`permissions::set_scope`), so a cashier who \
                          is a manager elsewhere can't count here. The handler asks again \
                          (`access::can_in`), and each line must stand at this store, or the \
                          whole count is a 404: a changed level id can't reach another \
                          store's shelf.",
                },
                Feature {
                    api: "Transactions",
                    why: "The whole count is one transaction: every adjustment movement (and \
                          its level) and every `consignment_loss` entry of the books between \
                          stores (`multistore::books`, the one place that writes them) commit \
                          together or not at all: a count can't fix the levels and forget \
                          what the store owes for missing consigned goods.",
                },
                Feature {
                    api: "notify",
                    why: "The owner stores of goods found short get a mail and an in-app \
                          notification (`Notification` with the database and mail channels) \
                          sent to whoever holds `consignment.manage` there: they lost goods \
                          they never saw go, so they hear it from the app, not by chance.",
                },
                AUDITED,
            ],
            under_hood: "Loading: the stock rows at the store (one query, for the category \
                         picker), the chosen category's rows and the store names. Saving: \
                         `Valid<TakeForm>`, the levels and costs of the counted lines, then one \
                         transaction with an `adjustment` per differing line \
                         (`StockMovement::record`) and a `consignment_loss` entry for consigned \
                         shortfalls, then the audit row and the owner stores' notifications.",
            docs: &[
                "docs/types.md#nested-names-rows-inside-a-form",
                "docs/relations.md#more-of-the-query-builder",
                "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
                "docs/mail.md#notifications",
                "docs/authorization.md#sensitive-actions-and-the-audit-trail",
            ],
            sources: &[
                TAKE,
                BOOKS,
                NOTIFY,
                AUDIT,
                "examples/bikeshop/resources/views/stock/take.html",
                "examples/bikeshop/resources/views/components/stock_take_rows.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Form: nested `lines[3][counted]` checked with `v.nested`",
                    region: "stock.take.form",
                },
                Code {
                    title: "Transaction: each difference an adjustment; consigned shortfalls booked",
                    region: "stock.take.apply",
                },
                Code {
                    title: "Template: a component writes the nested field names",
                    region: "stock.take.template",
                },
            ],
        },
        Explanation {
            route: "stock.consignments",
            path: "/staff/consignments",
            title: "Consignment shipments",
            purpose: "Goods sent between stores, both ways: what the active store sent out on \
                      consignment, what it asked for or holds for others, and what is in transit \
                      right now. Goods sent between stores are always consigned, never \
                      transferred: they stay their owner's until a customer buys them.",
            who: "Staff of either store (`stock.view`); managers act on them.",
            audience: &[Audience::Staff, Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                SCOPES,
                Feature {
                    api: "Query<T>",
                    why: "\"Ours or at our store\": `where_any(|q| q.where_(RentalBike::OWNER_STORE_ID.eq(store)).where_(RentalBike::LOCATION_STORE_ID.eq(store)))` inside what \
                          `access::visible` allows, so the OR can never widen what the person \
                          may see; the tabs narrow by status with `is_in`.",
                },
                Feature {
                    api: "UI kit: link_tabs + table",
                    why: "Open / in transit / all as links, the list as the kit's table with \
                          status badges (`<app-stock-shipment-status>`) whose \
                          colour and word agree, so the colour is never the only clue.",
                },
            ],
            under_hood: "Three queries a page: the count, the page of shipments, and every line \
                         of those shipments at once (for units and what is in transit), plus \
                         one for the store names. The test checks the count stays the same \
                         with more shipments.",
            docs: &[
                "docs/authorization.md#lists-scopes_with",
                "docs/relations.md#more-of-the-query-builder",
                "docs/ui.md#navigation-and-page-structure",
            ],
            sources: &[
                CONSIGNMENT,
                MODEL,
                "examples/bikeshop/resources/views/stock/consignments/index.html",
                SHIPMENT_STATUS,
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: shipments where the store is owner or location",
                    region: "stock.consignments.handler",
                },
                Code {
                    title: "Template: tabs, then one row per shipment",
                    region: "stock.consignments.template",
                },
            ],
        },
        Explanation {
            route: "stock.consignments.create",
            path: "/staff/consignments/new",
            title: "New consignment shipment",
            purpose: "Send our goods to another store (approved at once: we own them), or ask \
                      another store for its goods (the request waits for that store's \
                      approval). The rows are the owner's own goods on its shelf, with what is \
                      available.",
            who: "Managers (`consignment.manage` in the active store).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Valid<T> + after hook",
                    why: "`NewShipment`'s rules check the fields and each nested line \
                          (`lines[0][quantity]`); its `after` hook then checks the other store \
                          exists and isn't this one, that something is asked for, and reads \
                          the owner's stock to put \"only 3 available\" next to each line that \
                          asks for more, with the form kept. The database check runs only \
                          once the plain rules pass, and its errors land where the rules' do.",
                },
                Feature {
                    api: "UI kit: toggle_buttons + select",
                    why: "Direction, other store and a search are a small GET form above the \
                          sheet, so changing them re-asks the page with the right owner's \
                          goods, without a line of JavaScript.",
                },
                Feature {
                    api: "Routes::require_permission",
                    why: "The page and its form need `consignment.manage` **in the active \
                          store** (the handler asks again with `access::can_in`); approving a \
                          request is then the owner store's own decision, on the shipment's \
                          page.",
                },
            ],
            under_hood: "Loading: the stores and up to 100 of the owner's stock rows with \
                         something available (searchable). Sending: `Valid<NewShipment>` \
                         (rules, then the `after` hook's checks), then the shipment and its \
                         lines in one transaction, an audit row, and an in-app notice to the \
                         other store's `consignment.manage` holders.",
            docs: &[
                "docs/validation.md#hooks-prepare-authorize-after",
                "docs/types.md#nested-names-rows-inside-a-form",
                "docs/ui.md#form-fields",
                "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
            ],
            sources: &[
                CONSIGNMENT,
                MOD,
                "examples/bikeshop/resources/views/stock/consignments/new.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: the owner store's goods on its shelf, searched",
                    region: "stock.consignments.create.handler",
                },
                Code {
                    title: "Rules: no line asks for more than is available now",
                    region: "stock.consignments.create.rules",
                },
                Code {
                    title: "Template: one quantity field per row, as `lines[i][quantity]`",
                    region: "stock.consignments.create.template",
                },
            ],
        },
        Explanation {
            route: "stock.consignments.show",
            path: "/staff/consignments/{shipment}",
            title: "A consignment shipment",
            purpose: "One shipment's life: requested → approved by the owner store → shipped \
                      (leaves the owner's shelf) → received at the other store, also partly → \
                      asked back → sent back → home. The page shows its steps, its lines (sent, \
                      received, sent back) and only the buttons the person may press.",
            who: "Staff of the owner and of the location store.",
            audience: &[Audience::Staff, Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                ABAC,
                Feature {
                    api: "Transactions",
                    why: "Each step is one transaction that first moves the status with \
                          `UPDATE … WHERE status = ?` (two people pressing \"ship\" ship once), \
                          then writes the ledger: `consign_out` at the owner with a guarded \
                          `ledger::take` (a line whose stock ran out ships short), `consign_in` \
                          at the location on receipt, `recall` both ways. A step that fails \
                          half-way leaves neither a new status nor a stray movement; one that \
                          lost the race answers 409.",
                },
                LEDGER_PATTERN,
                Feature {
                    api: "Bike shop blocks",
                    why: "The kit has no stepper or timeline: the steps are an ordered list \
                          styled as a stepper (`<app-stock-steps>`, the \
                          current step marked `aria-current`; the done steps slide in through \
                          Motion, not under reduced motion), and the dated events are the \
                          `history` block of renox-blocks.",
                },
                AUDITED,
            ],
            under_hood: "Loading: the shipment (`access::find`), its lines and their names, the \
                         store names. Each action: the status guard, the ledger movements and \
                         line updates in one transaction, an audit row, and an in-app notice \
                         to the other store. A receipt marked \"nothing more will come\" \
                         closes the shipment: goods never received are the owner's loss \
                         (still theirs), and the owner store is told by mail and in the app.",
            docs: &[
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/relations.md#more-of-the-query-builder",
                "docs/mail.md#database-notifications",
            ],
            sources: &[
                CONSIGNMENT,
                LEDGER,
                MODEL,
                "examples/bikeshop/resources/views/stock/consignments/show.html",
                SHIPMENT_STATUS,
                STEPS,
                "crates/renox-blocks/views/blocks.html",
                "examples/bikeshop/public/areas/stock.js",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: each button checked in the store that matters",
                    region: "stock.consignments.show.handler",
                },
                Code {
                    title: "Guard: a status moves only from what it still is",
                    region: "stock.consignments.show.advance",
                },
                Code {
                    title: "Template: the actions the person may take, in the page header",
                    region: "stock.consignments.show.template",
                },
            ],
        },
        Explanation {
            route: "stock.suppliers",
            path: "/staff/suppliers",
            title: "Suppliers",
            purpose: "Who the shop buys from: contact, lead time, how big their price list is \
                      and the active store's open orders with them.",
            who: "Buyers: managers and the owner (`purchasing.manage`).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Query<T>",
                    why: "The counts beside each supplier are two `GROUP BY` queries \
                          (`group_by` + `select_as`) over the page's suppliers, not one query \
                          per row, so the page costs the same with 5 suppliers or 500.",
                },
                Feature {
                    api: "Routes::require_permission",
                    why: "Suppliers are the company's, not a store's, so there is no record to \
                          check store by store: the routes only need `purchasing.manage` in \
                          the active store.",
                },
                Feature {
                    api: "View::typed",
                    why: "The page's data is a struct deriving `ViewData`, which names its \
                          template; `view:check` (and the tests' `assert_views_compile`) \
                          reports a variable the template uses that the struct lacks, so a \
                          renamed field can't leave the page printing nothing.",
                },
            ],
            under_hood: "Four queries: the page of suppliers (with its count), their price-list \
                         sizes and their open orders (draft, ordered, partial) at this store.",
            docs: &[
                "docs/relations.md#more-of-the-query-builder",
                "docs/routing.md#guards",
                "docs/ui.md#typed-view-contexts",
            ],
            sources: &[
                PURCHASING,
                "examples/bikeshop/resources/views/stock/suppliers/index.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: counts per supplier with `group_by` + `select_as`",
                    region: "stock.suppliers.handler",
                },
                Code {
                    title: "Page data: a struct that names its template and fields",
                    region: "stock.suppliers.page",
                },
                Code {
                    title: "Template: the kit's `<rx-table>`, one row per supplier",
                    region: "stock.suppliers.template",
                },
            ],
        },
        Explanation {
            route: "stock.suppliers.create",
            path: "/staff/suppliers/new",
            title: "New supplier",
            purpose: "Add a supplier: name, email (purchase orders are mailed there), phone, \
                      lead time.",
            who: "Buyers (`purchasing.manage`).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[Feature {
                api: "#[derive(Validate)]",
                why: "`SupplierForm`'s rules are attributes on its fields (`required`, `email`, \
                      `min`/`max`), next to the fields they check, with no `impl Validate` to \
                      keep in step; a failed plain form goes back with the errors and the \
                      input kept.",
            }],
            under_hood: "`Valid<SupplierForm>`, then one insert and a redirect to the supplier's \
                         page with a toast.",
            docs: &["docs/validation.md#derivevalidate"],
            sources: &[
                PURCHASING,
                "examples/bikeshop/resources/views/stock/suppliers/form.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: an empty form, then `Valid<SupplierForm>` creates",
                    region: "stock.suppliers.create.handler",
                },
                Code {
                    title: "Form: `#[derive(Validate)]` rules, tidied before saving",
                    region: "stock.suppliers.form",
                },
                Code {
                    title: "Template: one form for new and edit",
                    region: "stock.suppliers.form.template",
                },
            ],
        },
        Explanation {
            route: "stock.suppliers.edit",
            path: "/staff/suppliers/{supplier}/edit",
            title: "Edit a supplier",
            purpose: "Change a supplier's contact or lead time (the next orders' expected date \
                      follows it).",
            who: "Buyers (`purchasing.manage`).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Found<M>",
                    why: "Route model binding: `Found<Supplier>` loads the supplier the \
                          route's `{supplier}` parameter names, or answers 404, so the \
                          handler starts with the record instead of a lookup and a check.",
                },
                Feature {
                    api: "#[derive(Validate)]",
                    why: "The same `SupplierForm` and template as the new-supplier page: one \
                          set of rules for both.",
                },
            ],
            under_hood: "Loading: one query. Saving: the supplier again (`Found`), \
                         `Valid<SupplierForm>`, then `save` and a redirect with a toast.",
            docs: &[
                "docs/routing.md#route-model-binding-foundm",
                "docs/validation.md#derivevalidate",
            ],
            sources: &[
                PURCHASING,
                "examples/bikeshop/resources/views/stock/suppliers/form.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: `Found<Supplier>` binds the route's model",
                    region: "stock.suppliers.edit.handler",
                },
                Code {
                    title: "Form: the same rules as for a new supplier",
                    region: "stock.suppliers.form",
                },
                Code {
                    title: "Template: one form for new and edit",
                    region: "stock.suppliers.form.template",
                },
            ],
        },
        Explanation {
            route: "stock.suppliers.show",
            path: "/staff/suppliers/{supplier}",
            title: "A supplier and their price list",
            purpose: "A supplier's contact, their price list (what they sell us and at what \
                      cost), the active store's orders with them, and the import of a new price \
                      list from their CSV file.",
            who: "Buyers (`purchasing.manage`).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "renox::import",
                    why: "The kit's `import_action` sheet (a file field and room for the report) \
                          posts the CSV; `Import::csv(…).run(…)` reads each row **as a form** \
                          (`PriceRow`'s `#[derive(Validate)]`), writes the good ones each in its \
                          own savepoint of one transaction, and answers with an `ImportReport`: \
                          a toast when every row went in, else a table of row numbers and \
                          messages. A row naming an unknown product undoes only itself, so one \
                          bad line never throws away a whole price list, and the same rules \
                          and messages as a form apply without a CSV parser of the shop's own.",
                },
                Feature {
                    api: "Queue",
                    why: "Files over 200 rows don't make anyone wait: the file goes to private \
                          storage and the `ImportPriceList` job runs the same import in the \
                          background, in the sender's language, then mails the report \
                          (`queue_mail`) and deletes the file.",
                },
                Feature {
                    api: "renox::import::template",
                    why: "\"Download a template\" is `import::template(…)`: a CSV with only the \
                          columns, so suppliers fill the right ones.",
                },
            ],
            under_hood: "Loading: the supplier (`Found<Supplier>`), a page of their price list \
                         (count and page) with the variants' names (two queries) and the last \
                         ten orders at this store. Importing: `Valid<ImportForm>` (a `.csv` \
                         file), then the import (each row: find the SKU, update \
                         cost/price/barcode or create the variant, upsert `supplier_items`), or \
                         for a large file a stored file and a queued job.",
            docs: &[
                "docs/ui.md#import",
                "docs/queue.md#a-job",
                "docs/mail.md#localized-mail",
            ],
            sources: &[
                PURCHASING,
                IMPORT,
                "examples/bikeshop/resources/views/stock/suppliers/show.html",
                "examples/bikeshop/resources/views/mail/stock/import_report.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Import: `renox::import` checks each row as a form; large files go to a job",
                    region: "stock.suppliers.show.import",
                },
                Code {
                    title: "Job: the same import in the background, then the report by mail",
                    region: "stock.suppliers.show.job",
                },
                Code {
                    title: "Template: the kit's `import_action` sheet",
                    region: "stock.suppliers.show.template",
                },
            ],
        },
        Explanation {
            route: "stock.purchasing",
            path: "/staff/purchase-orders",
            title: "Purchase orders",
            purpose: "The active store's orders to suppliers: drafts (some suggested by the daily \
                      reorder check), ordered, partly received, received. Every morning at 06:30 \
                      the reorder check (`stock:reorder`) finds the variants under their reorder \
                      level at each store, drafts a suggested order per supplier, notes when \
                      another store has spare (a consignment instead), and tells the store's \
                      buyers by mail and in the app.",
            who: "Buyers (`purchasing.manage`).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Schedule::daily_at",
                    why: "`daily_at(\"06:30\", \"stock:reorder\", …)` in the module's `register`: \
                          `schedule:list` shows it, `schedule:run stock:reorder` runs it now, and \
                          several servers sharing the database run it once (Renox claims each \
                          run), so no cron line on the server and no double orders. The steps \
                          are plain functions, so tests call them after `TestApp::travel`.",
                },
                Feature {
                    api: "notify",
                    why: "The alert is a `Notification` on the mail and database channels, sent \
                          to whoever holds `purchasing.manage` at the store (a permission, \
                          never a role's name), each in their own language: a role renamed or \
                          added later still gets it.",
                },
                SCOPES,
            ],
            under_hood: "Three queries a page: the count, the page and the suppliers \
                         (`belongs_to`). The reorder check: per store, the stock rows under their \
                         level, other stores' spare, the cheapest supplier per variant; old \
                         untouched suggestions are replaced by today's drafts, then the store's \
                         buyers are told.",
            docs: &[
                "docs/scheduling.md#scheduled-tasks",
                "docs/scheduling.md#several-servers",
                "docs/mail.md#notifications",
                "docs/relations.md#the-loaders",
                "docs/authorization.md#lists-scopes_with",
            ],
            sources: &[
                PURCHASING,
                REORDER,
                NOTIFY,
                MOD,
                "examples/bikeshop/resources/views/stock/purchase_orders/index.html",
                "examples/bikeshop/resources/views/mail/stock/notice.html",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: the store's orders by tab, suppliers by `belongs_to`",
                    region: "stock.purchasing.handler",
                },
                Code {
                    title: "Schedule: the daily reorder check drafts suggested orders",
                    region: "stock.purchasing.reorder",
                },
                Code {
                    title: "Template: tabs and a badge on suggested orders",
                    region: "stock.purchasing.template",
                },
            ],
        },
        Explanation {
            route: "stock.purchasing.create",
            path: "/staff/purchase-orders/new",
            title: "New purchase order",
            purpose: "Order from a supplier for the active store. The lines start with what the \
                      store needs: parts that work orders at this store wait for (\"waiting for \
                      parts\", #236) and goods under their reorder level, topped up to twice the \
                      level; then the rest of the supplier's price list. A blank quantity isn't \
                      ordered; the draft can be checked before it is sent.",
            who: "Buyers (`purchasing.manage`).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Valid<T> + after hook",
                    why: "Nested names (`lines[4][quantity]`, `lines[4][unit_cost]`) read as \
                          `Vec<OrderLine>`, each checked by `v.nested`; the supplier must exist \
                          (`exists` rule), and the `after` hook refuses an order without a \
                          single quantity, a rule about the whole form that no single field \
                          could carry.",
                },
                Feature {
                    api: "Query<T>",
                    why: "The needs are two plain queries (the store's work orders waiting for \
                          parts, then their waiting parts) and the stock rows under their level, \
                          joined in Rust by variant: no query per line, and the buyer starts \
                          from what is missing instead of a blank list.",
                },
            ],
            under_hood: "Loading: suppliers, the store's needs (three queries and the variants' \
                         names), the supplier's price list and its variants' names. Saving: \
                         `Valid<OrderForm>`, the draft and its lines in one transaction, and an \
                         audit row.",
            docs: &[
                "docs/validation.md#hooks-prepare-authorize-after",
                "docs/types.md#nested-names-rows-inside-a-form",
                "docs/validation.md#database-rules-unique-and-exists",
                "docs/relations.md#more-of-the-query-builder",
            ],
            sources: &[
                PURCHASING,
                "examples/bikeshop/src/app/workshop/model.rs",
                "examples/bikeshop/resources/views/stock/purchase_orders/new.html",
                "examples/bikeshop/resources/views/components/stock_po_line.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: what the store needs first, then the supplier's price list",
                    region: "stock.purchasing.create.handler",
                },
                Code {
                    title: "Query: parts work orders wait for, and goods under their level",
                    region: "stock.purchasing.needs",
                },
                Code {
                    title: "Template: a component for one line with its reasons",
                    region: "stock.purchasing.create.template",
                },
            ],
        },
        Explanation {
            route: "stock.purchasing.show",
            path: "/staff/purchase-orders/{order}",
            title: "A purchase order",
            purpose: "One order: its steps, its lines, the work orders waiting for its parts, and \
                      what may be done next: send it (mailed to the supplier with a link to a \
                      printable page), receive a delivery (whole or in part: the receiving store \
                      owns what arrives), or cancel it before anything arrived.",
            who: "Buyers (`purchasing.manage`) send and cancel; whoever receives deliveries \
                  (`stock.receive`) receives.",
            audience: &[Audience::Manager, Audience::Mechanic, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Average cost",
                    why: "Receiving updates each variant's cost to the average over the whole \
                          company's stock: `(on hand × cost + received × unit cost) / (on hand + \
                          received)`, rounded half up, in integers (`purchasing::average_cost`, \
                          unit-tested), so the value of the stock and the margins stay right \
                          when a supplier's price changes, without float rounding.",
                },
                LEDGER_PATTERN,
                Feature {
                    api: "Signed URLs",
                    why: "The supplier has no account, so the mail carries \
                          `state.signed_url(\"stock.purchasing.print\", …, 30 days)`: an \
                          HMAC-signed link that opens this one order and nothing else, with no \
                          login to create or token table to keep.",
                },
                Feature {
                    api: "Mail",
                    why: "When the supplier has an address, the order goes out through \
                          `state.mail_view` (Renox's mail layout and its `table` component) \
                          and `queue_mail`, so a slow mail server never holds the page.",
                },
                ABAC,
            ],
            under_hood: "Loading: the order (`access::find`), the supplier, the lines and names, \
                         the store's needs, the store names and the signed print link. Sending: \
                         the status guard, the dates (expected after the supplier's lead time), \
                         a queued mail, an audit row. Receiving: one transaction with a \
                         `purchase` movement, the new average cost and the line's received \
                         quantity per line, then partial or received, and an audit row. \
                         Cancelling: refused once anything has arrived.",
            docs: &[
                "docs/routing.md#signed-urls",
                "docs/mail.md#sending-a-mail",
                "docs/types.md#money",
                "docs/authorization.md#checking-one-record-has_permission_in",
            ],
            sources: &[
                PURCHASING,
                MODEL,
                POLICY,
                "examples/bikeshop/resources/views/stock/purchase_orders/show.html",
                "examples/bikeshop/resources/views/mail/stock/purchase_order.html",
                TESTS,
                BROWSER,
            ],
            code: &[
                Code {
                    title: "Handler: `access::can` per action, in the order's store",
                    region: "stock.purchasing.show.handler",
                },
                Code {
                    title: "Receive: a purchase movement per line, the average cost updated",
                    region: "stock.purchasing.show.receive",
                },
                Code {
                    title: "Template: the actions, and a receiving form while the order is open",
                    region: "stock.purchasing.show.template",
                },
            ],
        },
        Explanation {
            route: "stock.purchasing.print",
            path: "/purchase-orders/{order}/print",
            title: "A purchase order, printable",
            purpose: "The order as the supplier prints it or saves it as a PDF: the shop, the \
                      store to deliver to, the dates, the lines and the total, without the \
                      shop's menus.",
            who: "The supplier, through the signed link in their mail (and staff, from the \
                  order's page).",
            audience: &[Audience::Visitor, Audience::Manager],
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Signed URLs",
                    why: "The handler takes `ValidSignature`: a link that wasn't signed with the \
                          app's key, was changed or has expired is a 403. The route sits \
                          outside the staff guards, so no login is needed, and no other order \
                          can be reached from it (changing the id breaks the signature).",
                },
                Feature {
                    api: "UI kit: infolist + entry",
                    why: "The kit's infolist and table on a page of its own (no shop layout, \
                          so it prints clean), with a print button handled in \
                          `public/areas/stock.js` (no inline script, so `CSP=strict` works).",
                },
            ],
            under_hood: "Six queries: the order, the supplier, the store, the lines, and their \
                         variants' and products' names. Nothing is written.",
            docs: &[
                "docs/routing.md#signed-urls",
                "docs/ui.md#infolists-read-only-details",
            ],
            sources: &[
                PURCHASING,
                MOD,
                "examples/bikeshop/resources/views/stock/purchase_orders/print.html",
                "examples/bikeshop/public/areas/stock.js",
                TESTS,
            ],
            code: &[
                Code {
                    title: "Handler: a signed link for 30 days; `ValidSignature` guards the page",
                    region: "stock.purchasing.print.handler",
                },
                Code {
                    title: "Template: its own page, no shop chrome, for the supplier to print",
                    region: "stock.purchasing.print.template",
                },
            ],
        },
        Explanation {
            route: "stock.fleet",
            path: "/staff/stock/fleet",
            title: "Fleet and stock",
            purpose: "Bikes between sale stock and the rental fleet: a new bike from the store's \
                      shelf joins the fleet (same owner), and a rental bike at the end of its \
                      rental life goes back to the shelf to be sold as used, on its own used \
                      variant priced at its book value. Both leave a ledger movement.",
            who: "Managers of the owner store (`fleet.manage`).",
            audience: &[Audience::Manager, Audience::Owner],
            flow: Flow::BackOffice,
            features: &[
                LEDGER_PATTERN,
                Feature {
                    api: "has_permission_in",
                    why: "Both directions are the **owner** store's decision: \
                          `access::require(…, FLEET_MANAGE, StoreAttr::Owner, …)`. A bike placed \
                          at another store can be rented out there, but not retired by it, and \
                          goods held on consignment can't be put in the holder's fleet.",
                },
                Feature {
                    api: "Valid<T> + #[derive(Validate)]",
                    why: "The new bike's frame number must be unique \
                          (`#[validate(unique(\"rental_bikes\", \"frame_number\"))]`) and its \
                          rates and deposit present: a second bike with the same frame number \
                          is refused next to the field, before anything is written.",
                },
                AUDITED,
            ],
            under_hood: "Loading: the store's own new bikes on its shelf with something \
                         available (the stock view) and its fleet bikes standing here with their \
                         names. Adding: `Valid<ToFleetForm>`, a guarded `to_fleet` movement and \
                         the `rental_bikes` row (at the variant's cost as its book value) in one \
                         transaction, then an audit row. Retiring: refused while the bike is \
                         out or booked; else a used variant, a `from_fleet` movement and the \
                         bike retired, in one transaction, then an audit row.",
            docs: &[
                "docs/authorization.md#checking-one-record-has_permission_in",
                "docs/validation.md#database-rules-unique-and-exists",
            ],
            sources: &[
                FLEET,
                LEDGER,
                POLICY,
                AUDIT,
                "examples/bikeshop/resources/views/stock/fleet.html",
                TESTS,
                MOD,
            ],
            code: &[
                Code {
                    title: "Form: `unique(\"rental_bikes\", \"frame_number\")` and money as typed",
                    region: "stock.fleet.form",
                },
                Code {
                    title: "Handler: one bike off the shelf and into the fleet, in one transaction",
                    region: "stock.fleet.handler",
                },
                Code {
                    title: "Template: a searchable `<rx-select>` and an `<rx-form-grid>`",
                    region: "stock.fleet.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![NotAPage {
        route: "stock.suppliers.template",
        reason: "a CSV file with the price list's columns (renox::import::template)",
    }]
}
