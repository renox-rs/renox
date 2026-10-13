//! "About this page" entries for the reports area's pages (see `crate::explain`).

use crate::explain::{Audience, Code, Explanation, Feature, Flow, NotAPage};

const WHO: &str = "The owner, who sees every store and compares them, and store managers, who \
                   see the stores where a role gives them `reports.view` (a manager helping \
                   another store this month sees that one too, for as long as the role lasts).";

const AUDIENCE: &[Audience] = &[Audience::Owner, Audience::Manager];

// What every report grid shares (one `Feature` per line, reused by the six grid pages).
const GRID: Feature = Feature {
    api: "renox::grid",
    why: "A whole reporting table from one definition: the server pages, sorts and filters \
          (a filter in each heading that fits the column, dates in `APP_TIMEZONE`), and the \
          state lives in the address, so a filtered report is a link to send.",
};
const SEARCH: Feature = Feature {
    api: "Grid::searchable",
    why: "The search box looks in the text columns marked `.searchable()`: every word typed must \
          appear in one of them.",
};
const ADVANCED: Feature = Feature {
    api: "Grid::advanced_filter",
    why: "Questions the heading filters can't ask (\"over $1,000 **or** cancelled\"): rules \
          per column, matching all or any. Only the grid's own columns count, so nobody can \
          filter on a column the page doesn't show.",
};
const GROUPS: Feature = Feature {
    api: "Column::summary + Grid::groups",
    why: "Sums and averages under the money columns cover every row the filters match, not \
          just the page; grouping by store, status or category adds a subtotal per group. \
          Both work on the model's own columns, which is why the store's *name* is a column of \
          the view.",
};
const REMEMBER: Feature = Feature {
    api: "Grid::remember",
    why: "A manager who leaves the report and comes back finds it as they left it (filters, \
          search, sort, grouping, page size, kept in the session); column choices (order, \
          width, frozen columns) are kept per person in `grid_preferences`.",
};
const CARDS: Feature = Feature {
    api: "Grid::cards_on_mobile",
    why: "On a phone each row becomes a card with the columns marked `.mobile()`; the first \
          column is `.frozen()` on wide screens so it stays put while the grid scrolls.",
};
const EXPORTS: Feature = Feature {
    api: "Grid::exports (CSV, xlsx, print)",
    why: "The toolbar's export menu makes a CSV, an Excel file (the `xlsx` feature, real numbers \
          and dates, frozen headings) or a print page of every row the filters match, sorted as \
          on screen: the handler answers `grid.export(…)` before drawing the page.",
};
const VIEW_MODEL: Feature = Feature {
    api: "Database view as a model",
    why: "The grid reads a database view (`migrations/20260102001000_create_report_views.*`) \
          that joins the record with its stores', customer's and product's names. A view is a \
          saved query, so nothing is copied and nothing can drift; a read-only `#[derive(Model)]` \
          over it gives the grid plain columns to filter, group and sum.",
};
const VISIBLE: Feature = Feature {
    api: "scopes_with",
    why: "`access::visible::<M>(\"reports.view\")` turns the person's roles into the list query's \
          filter (`permissions::scopes_with` + `Scopes::apply`, #244): rows of the stores where \
          they hold `reports.view` today; every row for the owner's global role.",
};

const GRID_DOCS: &[&str] = &[
    "docs/grid.md#filters-search-and-chips",
    "docs/grid.md#the-advanced-filter",
    "docs/grid.md#summaries-and-groups",
    "docs/grid.md#remembering-filters",
    "docs/grid.md#cards-on-phones",
    "docs/grid.md#exports",
    "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
];

/// The explanation of every page in this area.
pub fn entries() -> Vec<Explanation> {
    vec![
        Explanation {
            route: "reports.dashboard",
            path: "/staff/reports",
            title: "Reports dashboard",
            purpose: "How the business is doing, for any period: revenue by stream (sales, \
                      rentals, workshop, plans) against the period before, orders and the \
                      average order, the fleet's use and overdue rentals, the workshop's output \
                      and turnaround, active plans, what they bring a month and their churn, fees \
                      and balances between the stores, staff help, trends, the best sellers and \
                      customers, and the stores side by side. A switch counts income in the \
                      books of the store that **owns** the bike or goods, or at the store that \
                      **did the work**, since the two differ once bikes and goods are placed at \
                      other stores (#245).",
            who: WHO,
            audience: AUDIENCE,
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "UI kit: dashboard + widget + stats + stat",
                    why: "Figures in `stats` rows (each `stat` with its change against the \
                          period before, a sparkline, a link to its grid) and the charts and \
                          lists in a `dashboard` of `widget`s: a CSS grid, one column on phones, \
                          wide widgets with `span`.",
                },
                Feature {
                    api: "UI kit: period_filter",
                    why: "One row of periods (7 days to this year, and 12 weeks for weekly \
                          buckets) plus a custom range with two dates; it only sets `?period=` \
                          and keeps the rest of the address, so the store and the switch stay.",
                },
                Feature {
                    api: "renox::chart (Period, Trend, Series)",
                    why: "`Period` reads the address; `Trend::of(query, \"booked_at\").over(period)\
                          .sum(&state, \"amount\")` buckets each stream per day, ISO week or month \
                          in `APP_TIMEZONE` (`Query::buckets` underneath, one query per stream), \
                          with empty days as 0. `period.previous()` gives the comparison.",
                },
                Feature {
                    api: "chart(…) template function",
                    why: "Server-drawn SVG, no chart library: a stacked bar chart of revenue by \
                          stream, a doughnut of the mix, a **heatmap** of rentals by weekday \
                          and hour (`cells` of column = hour, row = weekday, value = rentals), and a stacked bar per \
                          store. Every chart has a \"Show the data\" table and keyboard focus.",
                },
                Feature {
                    api: "Cache::remember",
                    why: "The numbers take about thirty queries; they are cached under a key of \
                          the period, the stores, the switch, today and a generation number. \
                          Listeners on `RentalClosed`, `WorkOrderClosed` and `PaymentSucceeded` \
                          bump the generation, so a change shows at the next visit; ten minutes \
                          is the safety net.",
                },
                Feature {
                    api: "Events and listeners",
                    why: "The reports area doesn't change how rentals, the workshop or payments \
                          work: it listens to the events they already emit.",
                },
                Feature {
                    api: "scopes_with",
                    why: "`AuthUser::scopes_with::<Store>(\"reports.view\")` lists the stores the \
                          person may report on (all of them for the owner); `?store=` narrows \
                          to one of them and anything else is ignored. The store comparison \
                          appears when more than one store is shown.",
                },
                Feature {
                    api: "Database view as a model",
                    why: "`report_revenue` is one view of every line of income (paid order \
                          lines, returned rentals with their fees, completed work orders, plan \
                          visits and payments) with **both** store attributes, so \"by books\" \
                          and \"by work\" are the same query on another column, and the stores \
                          add up to the company either way.",
                },
                Feature {
                    api: "motion.dev (vendored)",
                    why: "The figures and widgets slide in one after the other when the page or \
                          a new period loads (`data-bs-reveal`, `transform` and `opacity` only); \
                          nothing moves under `prefers-reduced-motion`.",
                },
            ],
            under_hood: "`Reach::of` reads the person's stores (one query for their names). \
                         `Numbers::for_page` reads the generation from the cache and, on a miss, \
                         computes: income per stream for the period and the one before (two \
                         grouped queries on `report_revenue`), four `Trend` series, the rentals \
                         overlapping the period (fleet use and the weekday × hour points, in \
                         Rust), the fleet's size, overdue rentals, completed work orders (their \
                         turnaround), active plans per plan with the plans' prices (monthly \
                         revenue: a weekly plan's price × 30 ÷ 7), cancellations, fees and open \
                         balances from `intercompany_entries` (netted by the books' own \
                         `intercompany::balances`), help hours, the top products, categories \
                         and customers (grouped, then their names in one query each) and, for \
                         several stores, income per store and stream. The same fixed set of \
                         queries runs whatever the data (`tests/reports.rs` counts them).",
            docs: &[
                "docs/ui.md#dashboards",
                "docs/scheduling.md#cache",
                "docs/scheduling.md#events",
                "docs/authorization.md#roles-per-branch-a-role-in-one-store-for-a-while",
            ],
            sources: &[
                "examples/bikeshop/src/app/reports/dashboard.rs",
                "examples/bikeshop/src/app/reports/numbers.rs",
                "examples/bikeshop/src/app/reports/scope.rs",
                "examples/bikeshop/src/app/reports/model.rs",
                "examples/bikeshop/resources/views/reports/dashboard.html",
                "examples/bikeshop/migrations/20260102001000_create_report_views.up.sql",
                "examples/bikeshop/tests/reports.rs",
            ],
            code: &[
                Code {
                    title: "Handler: who may see which stores, then the cached numbers",
                    region: "reports.dashboard.handler",
                },
                Code {
                    title: "Cache: one key per period, store and day, dropped when income changes",
                    region: "reports.dashboard.cache",
                },
                Code {
                    title: "Template: the kit's `stats` and a `chart(…)` in a `widget`",
                    region: "reports.dashboard.template",
                },
            ],
        },
        Explanation {
            route: "reports.orders",
            path: "/staff/reports/orders",
            title: "Orders report",
            purpose: "Every order of the person's stores, online and at the counter, to filter, \
                      group by store, status or channel, sum and export: the answer to \"how much \
                      did South sell online last month?\" without asking anyone.",
            who: WHO,
            audience: AUDIENCE,
            flow: Flow::BackOffice,
            features: &[
                GRID, SEARCH, ADVANCED, GROUPS, REMEMBER, CARDS, EXPORTS, VIEW_MODEL, VISIBLE,
            ],
            under_hood: "One page is three queries on `report_orders` (the count, the rows, the \
                         sums under Units and Total for every matching row), plus the stores' \
                         names for the Store filter. An export runs the same filtered query \
                         without paging (up to 100,000 rows). A row opens the order's staff \
                         page (`row_url`).",
            docs: GRID_DOCS,
            sources: &[
                "examples/bikeshop/src/app/reports/grids.rs",
                "examples/bikeshop/resources/views/reports/grid.html",
                "examples/bikeshop/migrations/20260102001000_create_report_views.up.sql",
            ],
            code: &[
                Code {
                    title: "Grid: one shared toolbox, then the order columns with sums",
                    region: "reports.orders.grid",
                },
                Code {
                    title: "Handler: only the stores the person may see, export or page",
                    region: "reports.orders.handler",
                },
                Code {
                    title: "Template: `renox/grid.html` draws the whole grid",
                    region: "reports.grid.template",
                },
            ],
        },
        Explanation {
            route: "reports.rentals",
            path: "/staff/reports/rentals",
            title: "Rentals report",
            purpose: "Every rental served by the person's stores **or** of their stores' bikes, \
                      with both stores side by side, the bike's model and category, and what \
                      each rental brought in (price, late and damage fees).",
            who: WHO,
            audience: AUDIENCE,
            flow: Flow::BackOffice,
            features: &[
                GRID, SEARCH, ADVANCED, GROUPS, REMEMBER, CARDS, EXPORTS, VIEW_MODEL, VISIBLE,
            ],
            under_hood: "`report_rentals` joins the rental with both stores, the customer, the \
                         bike, its product and category. A rental has two store attributes, so \
                         the list filter matches either: North's manager sees North's bikes \
                         rented out at South too. Grouping by owner store against store shows \
                         who earns from whose fleet.",
            docs: GRID_DOCS,
            sources: &[
                "examples/bikeshop/src/app/reports/grids.rs",
                "examples/bikeshop/resources/views/reports/grid.html",
                "examples/bikeshop/migrations/20260102001000_create_report_views.up.sql",
            ],
            code: &[
                Code {
                    title: "Grid: both stores as columns, sums under the money",
                    region: "reports.rentals.grid",
                },
                Code {
                    title: "Model: a view with two store columns, read through `StoreRecord`",
                    region: "reports.rentals.model",
                },
                Code {
                    title: "Handler: `access::visible` keeps the person's stores",
                    region: "reports.rentals.handler",
                },
            ],
        },
        Explanation {
            route: "reports.work_orders",
            path: "/staff/reports/work-orders",
            title: "Work orders report",
            purpose: "The workshops' work: walk-ins, bookings, plan visits and fleet repairs, \
                      their status, mechanic, labour and parts, summed per store, status or \
                      source.",
            who: WHO,
            audience: AUDIENCE,
            flow: Flow::BackOffice,
            features: &[
                GRID, SEARCH, ADVANCED, GROUPS, REMEMBER, CARDS, EXPORTS, VIEW_MODEL, VISIBLE,
            ],
            under_hood: "`report_work_orders` joins the work order with the workshop's store, \
                         the customer's bike and the customer, or the fleet bike, and the \
                         mechanic's name. Labour, parts and total are summed for every matching \
                         row; grouping by source sets plan visits apart from paid work.",
            docs: GRID_DOCS,
            sources: &[
                "examples/bikeshop/src/app/reports/grids.rs",
                "examples/bikeshop/resources/views/reports/grid.html",
                "examples/bikeshop/migrations/20260102001000_create_report_views.up.sql",
            ],
            code: &[
                Code {
                    title: "Grid: labour and parts summed, grouped by store or status",
                    region: "reports.work_orders.grid",
                },
                Code {
                    title: "Handler: one function answers the page and its exports",
                    region: "reports.work_orders.handler",
                },
                Code {
                    title: "Template: `renox/grid.html` draws the whole grid",
                    region: "reports.grid.template",
                },
            ],
        },
        Explanation {
            route: "reports.payments",
            path: "/staff/reports/payments",
            title: "Payments report",
            purpose: "The money received at the person's stores: what for (an order, a rental, a \
                      work order, a plan), how (cash, card, online) and whether it went through, \
                      for reconciling the till and the gateway.",
            who: WHO,
            audience: AUDIENCE,
            flow: Flow::BackOffice,
            features: &[
                GRID, SEARCH, ADVANCED, GROUPS, REMEMBER, CARDS, EXPORTS, VIEW_MODEL, VISIBLE,
            ],
            under_hood: "`report_payments` adds the store's and the customer's names to \
                         `payments` (a `Morph` to what was paid). Grouped by method with the \
                         sum under Amount, it is the day's cash-up; the gateway reference is a \
                         hidden, searchable column.",
            docs: GRID_DOCS,
            sources: &[
                "examples/bikeshop/src/app/reports/grids.rs",
                "examples/bikeshop/resources/views/reports/grid.html",
                "examples/bikeshop/migrations/20260102001000_create_report_views.up.sql",
            ],
            code: &[
                Code {
                    title: "Grid: payments by kind, method and status, amounts summed",
                    region: "reports.payments.grid",
                },
                Code {
                    title: "Handler: the visible rows, exported or paged",
                    region: "reports.payments.handler",
                },
                Code {
                    title: "Template: `renox/grid.html` draws the whole grid",
                    region: "reports.grid.template",
                },
            ],
        },
        Explanation {
            route: "reports.customers",
            path: "/staff/reports/customers",
            title: "Customers report",
            purpose: "Customers with their **lifetime value**: purchases, rentals, services and \
                      plans, added up across the company, with their visits and their first and \
                      last time, best first.",
            who: WHO,
            audience: AUDIENCE,
            flow: Flow::BackOffice,
            features: &[
                GRID,
                SEARCH,
                ADVANCED,
                GROUPS,
                REMEMBER,
                CARDS,
                EXPORTS,
                Feature {
                    api: "Database view as a model",
                    why: "`report_customers` sums `report_revenue` per customer and stream in \
                          one grouped subquery joined to the customers (not a subquery per \
                          row), so the lifetime value is a plain column the grid sorts, \
                          filters and sums.",
                },
                Feature {
                    api: "scopes_with",
                    why: "Customers belong to the company, not to a store: the owner sees all of \
                          them; a manager sees those with income at one of their stores (a \
                          `where_raw` subquery built from `permissions::scopes_with`), each with \
                          their whole lifetime value.",
                },
            ],
            under_hood: "A page is the count, the rows and the sums, each over the view. The \
                         grouped headings put the four values and the lifetime value under one \
                         \"Value\" heading (`Column::under`).",
            docs: &[
                "docs/grid.md#filters-search-and-chips",
                "docs/grid.md#options-every-column-takes",
                "docs/grid.md#summaries-and-groups",
                "docs/grid.md#exports",
            ],
            sources: &[
                "examples/bikeshop/src/app/reports/grids.rs",
                "examples/bikeshop/src/app/reports/model.rs",
                "examples/bikeshop/resources/views/reports/grid.html",
                "examples/bikeshop/migrations/20260102001000_create_report_views.up.sql",
            ],
            code: &[
                Code {
                    title: "Query: customers who did business with the person's stores",
                    region: "reports.customers.query",
                },
                Code {
                    title: "Grid: lifetime value per stream, under one grouped heading",
                    region: "reports.customers.grid",
                },
                Code {
                    title: "Template: `renox/grid.html` draws the whole grid",
                    region: "reports.grid.template",
                },
            ],
        },
        Explanation {
            route: "reports.entries",
            path: "/staff/reports/intercompany",
            title: "Between stores report",
            purpose: "The books between stores for the accountant: every entry (rental revenue \
                      and operating fees, sales of consigned goods and selling fees, late and \
                      damage fees, repairs, lost consigned goods), who owes whom, and whether \
                      its month is settled.",
            who: WHO,
            audience: AUDIENCE,
            flow: Flow::BackOffice,
            features: &[
                GRID, SEARCH, ADVANCED, GROUPS, REMEMBER, CARDS, EXPORTS, VIEW_MODEL, VISIBLE,
            ],
            under_hood: "`report_entries` adds both stores' names and the settlement's status to \
                         `intercompany_entries` (written only by the books area). The Source \
                         column is drawn by the page (`Column::custom`): a link to the rental, \
                         order or work order the entry was booked for.",
            docs: GRID_DOCS,
            sources: &[
                "examples/bikeshop/src/app/reports/grids.rs",
                "examples/bikeshop/resources/views/reports/grid.html",
                "examples/bikeshop/src/app/multistore/model.rs",
            ],
            code: &[
                Code {
                    title: "Handler: each row links to the record it came from (`extend`)",
                    region: "reports.entries.handler",
                },
                Code {
                    title: "Grid: debtor, creditor and settlement as filters and groups",
                    region: "reports.entries.grid",
                },
                Code {
                    title: "Template: a custom cell for the entry's source",
                    region: "reports.grid.template",
                },
            ],
        },
        Explanation {
            route: "reports.monthly",
            path: "/staff/reports/monthly",
            title: "Monthly report",
            purpose: "The month's records for the accountant: one Excel workbook per store (a \
                      summary, its books as the owner, its work as the operator, its statement \
                      between stores), mailed to the owner (every store) and each manager (their \
                      store). It runs by itself on the 1st; here it runs for any month, with its \
                      progress.",
            who: WHO,
            audience: AUDIENCE,
            flow: Flow::BackOffice,
            features: &[
                Feature {
                    api: "Schedule::monthly_on",
                    why: "`monthly_on(1, \"03:00\", \"reports:monthly\", …)` runs the previous \
                          month on the 1st, after the books are settled at 02:00; the run is \
                          claimed first, so two servers never both send it.",
                },
                Feature {
                    api: "Queue batches",
                    why: "A run is one batch (`queue.batch(\"monthly-report:2026-09:1-2-3\")`): \
                          one job per store builds its workbook, side by side, and the batch's \
                          `then` job mails them once all succeeded. The progress is the batch's \
                          own count of jobs run, read from `job_batches`.",
                },
                Feature {
                    api: "Mail attachments",
                    why: "`Mail::attach` sends each person the workbooks of the stores where \
                          they hold `reports.view`: one mail with three files for the owner, \
                          one with their store's for a manager.",
                },
                Feature {
                    api: "UI kit: widget (url + poll) + progress",
                    why: "While a run is going, the runs' widget reloads itself every three \
                          seconds from `reports.monthly.runs` (a fragment), each run with the \
                          kit's `progress` bar and, once done, a download per store.",
                },
                Feature {
                    api: "Storage + Download",
                    why: "Workbooks are kept on the private disk \
                          (`reports/monthly/2026-09/<store>.xlsx`) and downloaded through a \
                          route that checks `reports.view` in that store (another store's file \
                          is a 404).",
                },
                Feature {
                    api: "rust_xlsxwriter",
                    why: "A workbook with four sheets is more than `Grid::export_as` (one sheet \
                          per file) makes, so the jobs write it with the crate Renox's `xlsx` \
                          feature already builds: real numbers in the currency's decimals, \
                          dates in `APP_TIMEZONE`, frozen headings.",
                },
            ],
            under_hood: "The form checks the month (up to this one) and starts a batch for the \
                         stores in the person's reach. Each `BuildStoreReport` job reads the \
                         month's `report_revenue` lines by owner and by operator and the \
                         store's `intercompany_entries` (three queries), writes the workbook and \
                         stores it. `SendMonthlyReport` finds who holds `reports.view` in each \
                         store (the owner's global role counts everywhere) and sends one mail \
                         per person. The page lists the latest runs that touch the person's \
                         stores in one query on `job_batches`.",
            docs: &[
                "docs/scheduling.md#scheduled-tasks",
                "docs/queue.md#chains-and-batches",
                "docs/mail.md#sending-a-mail",
                "docs/ui.md#dashboards",
            ],
            sources: &[
                "examples/bikeshop/src/app/reports/monthly.rs",
                "examples/bikeshop/resources/views/reports/monthly.html",
                "examples/bikeshop/resources/views/reports/_runs.html",
                "examples/bikeshop/resources/views/components/reports_runs.html",
                "examples/bikeshop/resources/views/mail/reports/monthly.html",
                "examples/bikeshop/tests/reports.rs",
            ],
            code: &[
                Code {
                    title: "Job: one batch per month, a job per store, then the mail",
                    region: "reports.monthly.batch",
                },
                Code {
                    title: "Handler: `Valid<RunForm>` checks the month, then starts the batch",
                    region: "reports.monthly.handler",
                },
                Code {
                    title: "Template: a form, and a `widget` that polls while a run is going",
                    region: "reports.monthly.template",
                },
            ],
        },
    ]
}

/// GET routes of this area that aren't pages (JSON, files, streams).
pub fn not_pages() -> Vec<NotAPage> {
    vec![
        NotAPage {
            route: "reports.monthly.runs",
            reason: "the monthly report's runs as a fragment, which that page's widget polls",
        },
        NotAPage {
            route: "reports.monthly.file",
            reason: "a store's monthly workbook (an Excel download)",
        },
    ]
}
