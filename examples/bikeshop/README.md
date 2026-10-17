# examples/bikeshop

**Renox's flagship example** (epic #231): a whole business, built the way Renox recommends,
with real data volume. If you want to know how Renox does something in a real app, start here.

A bike shop with **three stores** (North, South, West) where customers:

- **buy** bikes and gear (helmets, lights, locks, clothing, spare parts) online or at a store's
  counter;
- **rent** a bike by the hour or the day, picked up and returned at a store, with a deposit and
  a late fee;
- have their bikes **serviced**, once or on a **service plan** (a weekly check, a monthly
  tune-up…) whose visits are booked for them automatically.

The three meet: a bike bought here is registered to the customer, who is offered a plan;
services use spare parts from the stock; parts are found by the bike models they fit. And the
**stores work together** (#245): staff help another store for a while with a role given there;
bikes and goods owned by one store are rented out or sold at another (consignment) while the
owner keeps them in its books; the store that did the work earns a fee (20% by default), and the
stores settle every month.

It keeps the shape of the Sakila/Pagila sample database (stock per store → rented for a period →
returned, maybe late → paid), grown into a bike shop: see [`/about/data`](src/app/about/data.rs)
in the app.

**Every page explains itself, side by side with its code.** On a wide screen (1200 px and
up) a panel beside the page says what the page is for, who uses it, which Renox features it
uses **and why**, and shows the code that makes it work: the handler, the key part of the
template, a query, a policy or a test, one tab each, coloured, with the file's lines on
GitHub and a copy button. Then what happens under the hood (queries, transactions, jobs,
events, mails) and links to the guide. The panel folds to a slim rail (remembered in a
cookie); on a narrower screen the "About this page" button opens the same content in a
sheet. The code is cut from the shop's own files when it is built, so it never goes stale
(see [Adding a page](#adding-a-page)).

> **Start on [`/about/pages`](src/app/about/mod.rs)**: every page of the shop, filtered by
> Renox feature ("which pages use `renox::grid`?", "where is `Pivot`?") or by role (visitor,
> customer, cashier, mechanic, manager, owner). Each line opens the page and its explanation.

## Run it

```bash
cd examples/bikeshop
cp .env.example .env     # then: rnx key:generate (or cargo run -p renox-cli -- key:generate)
cargo run -- migrate
cargo run -- db:seed     # a small shop, in a second
cargo run                # http://127.0.0.1:3000 (queue workers and the scheduler run inside)
```

For Pagila's volume (18 months of a busy shop: ~1,000 product variants, 600 customers,
~15,000 rentals, 5,000 orders, 3,000 work orders; about 4 s in a release build, 8 s in a debug
one), start from an empty database:

```bash
cargo run -- migrate:fresh
cargo run -- demo:seed --size large
```

Both seeds make the same shop (only the volume differs), with dates counted back from today, so
a rental due today is due today whenever you seed. Neither runs twice: `db:seed` on a seeded
database does nothing, `demo:seed` refuses and tells you to `migrate:fresh` first.

### Demo users

The seed prints them at the end. The password is `password` for all. The login page
lists the main ones too (a tap fills the form) while they exist; `BIKESHOP_DEMO_LOGINS=false`
hides that list ([src/app/accounts/demo_logins.rs](src/app/accounts/demo_logins.rs)).

| Login | Who |
|---|---|
| `owner@bikeshop.test` | The owner: every permission in every store (a global role) |
| `manager.north@bikeshop.test`, `manager.south@…`, `manager.west@…` | Each store's manager |
| `cashier.north@bikeshop.test`, `cashier.south@…`, `cashier.west@…` | Each store's cashier |
| `mechanic.north@bikeshop.test`, `mechanic2.north@…` (and for south and west) | Mechanics |
| `mechanic.west@bikeshop.test` | A West mechanic **helping South this week** (a role in South with dates) |
| `staff.north@bikeshop.test`, `staff.south@…`, `staff.west@…` | Floor staff |
| `floater@bikeshop.test` | Cashier of North **and** of South (roles in two stores; the store switcher) |
| `customer@bikeshop.test` | A customer with two bikes, a monthly plan, a rental due today and past rentals |

Staff must turn on two-factor login before the staff side opens (`/account` asks them). For a
demo without an authenticator app, set `BIKESHOP_STAFF_2FA=optional` in `.env`.

## Feature → page → file

Every page below has its full explanation in the app ("About this page", and `/about/pages`).
Paths are relative to this folder.

### The public shop and customers

| Feature | Page | Files |
|---|---|---|
| Home page: the kit's `navbar`, `card_grid` + `media_card`, `Routes::etag`, `seo()`, Motion | `/` | [src/app/home/mod.rs](src/app/home/mod.rs), [resources/views/home/index.html](resources/views/home/index.html) |
| Catalogue with filters and sorting, categories (`Found<M>`), the `range_slider` block | `/shop`, `/shop/{slug}` | [src/app/catalog/browse.rs](src/app/catalog/browse.rs), [src/app/catalog/filters.rs](src/app/catalog/filters.rs) |
| Models checked against the migrations (`app.model::<…>()`, `rnx db:check`, `assert_models_match_schema`) | `rnx db:check` | [src/app/catalog/mod.rs](src/app/catalog/mod.rs) (`register`), [tests/catalog.rs](tests/catalog.rs) |
| A typed view context (`#[derive(ViewData)]`, `View::typed`, `app.view_data::<…>()`; `rnx view:check` reports a template variable the struct lacks) | `/staff/suppliers` | [src/app/stock/purchasing.rs](src/app/stock/purchasing.rs) (`SupplierList`), [src/app/stock/mod.rs](src/app/stock/mod.rs) (`register`), [tests/about.rs](tests/about.rs) (`assert_views_compile`) |
| Migrations written from the models: `index(..)` on `Category`, `Brand` and `Product`; `db:diff index_brands_name` wrote the `brands(name)` index (`db:diff` again prints `Nothing to change.`) | `rnx make:migration --auto` | [src/app/catalog/model.rs](src/app/catalog/model.rs), [migrations/](migrations/) (`…_index_brands_name.*`) |
| Full-text search (`#[model(search)]`: FTS5 / `tsvector`), suggestions in the navbar | `/search?q=helmet` | [src/app/catalog/model.rs](src/app/catalog/model.rs), [migrations/](migrations/) (`…_search_products.*`) |
| Product page: `gallery` and `swatches` blocks, what fits (`Pivot`), stock per store, recently viewed (session) | `/products/{slug}` | [src/app/catalog/product.rs](src/app/catalog/product.rs), [resources/views/catalog/show.html](resources/views/catalog/show.html) |
| Sitemap and robots.txt | `/sitemap.xml` | [src/app/catalog/mod.rs](src/app/catalog/mod.rs) |
| Cart (session for guests, a table for customers), `View::also`, toasts | `/cart` | [src/app/sales/cart.rs](src/app/sales/cart.rs) |
| Checkout: the kit's `wizard`, `#[derive(Validate)]`, live validation, stock reserved in one transaction | `/checkout` | [src/app/sales/checkout.rs](src/app/sales/checkout.rs) |
| Payment: Midtrans through `state.http` and its webhook (`renox::webhook`), a demo gateway without keys | `/pay/{payment}` | [src/app/sales/gateway.rs](src/app/sales/gateway.rs), [src/app/sales/payments.rs](src/app/sales/payments.rs) |
| Orders and invoices (signed links for guests, print styles); a counter sale's receipt is the `receipt` block (80 mm, #48) | `/orders/{order}` | [src/app/sales/orders.rs](src/app/sales/orders.rs) |
| Renting: `datetime_range` and `availability` blocks, two overlap checks (form hook + `Db::begin_immediate` + `lock_for_update`) | `/rent`, `/rentals`, `/rentals/{code}` | [src/app/rentals/reserve.rs](src/app/rentals/reserve.rs), [src/app/rentals/booking.rs](src/app/rentals/booking.rs), [src/app/rentals/pricing.rs](src/app/rentals/pricing.rs) |
| Rental status as a state machine (`#[model(state_machine)]`, `Machine`, `Transition` with guards, `transition(..).run_in(&mut tx)`, the history in `state_transitions`, `rnx state:diagram Rental`) | `/staff/rentals/{rental}`, `/rentals/{code}` | [src/app/rentals/machine.rs](src/app/rentals/machine.rs), [counter.rs](src/app/rentals/counter.rs), [tasks.rs](src/app/rentals/tasks.rs), [docs/state-machines.md](../../docs/state-machines.md) |
| ID check: `Upload`, `Encrypted<String>`, reviewed by staff | `/rentals/identity` | [src/app/rentals/identity.rs](src/app/rentals/identity.rs) |
| My bikes and their service history (`has_many`, the `history` block) | `/bikes`, `/bikes/{bike}` | [src/app/workshop/bikes.rs](src/app/workshop/bikes.rs) |
| Booking a service within the workshop's capacity (the kit's `date_picker` with `disabled_dates` and `closed_weekdays`) | `/service/book`, `/service/{order}` | [src/app/workshop/booking.rs](src/app/workshop/booking.rs), [src/app/workshop/capacity.rs](src/app/workshop/capacity.rs) |
| Approving extra work from a signed, single-use link | `/service/approve/{extra}` | [src/app/workshop/approval.rs](src/app/workshop/approval.rs) |
| Service plans with renox-billing (Stripe, Xendit, a demo gateway), the `compare_plans` and `month_calendar` blocks | `/plans`, `/plans/subscribe`, `/plans/mine`, `/plans/mine/{subscription}` | [src/app/plans/](src/app/plans/) |
| Pay a ready work order by payment link: `Billing::payment_link` with the reference `work_order`, the demo gateway's `payment_link` and signed page, a `PaymentSucceeded` listener that marks it paid | `/staff/workshop/{order}/pay-link`, `/plans/demo-pay/link/{payment}` | [src/app/workshop/order.rs](src/app/workshop/order.rs), [src/app/plans/demo.rs](src/app/plans/demo.rs), [src/app/workshop/mod.rs](src/app/workshop/mod.rs) |
| Accounts: Renox's `Auth` (login, register, reset, verification), account sections (`Registry::account_section`), language, privacy (download, delete) | `/login`, `/register`, `/account` | [src/app/accounts/](src/app/accounts/) |
| Notifications: the bell, `DatabaseMessage`, live over Server-Sent Events; a reservation's confirmation mail carries a calendar file (`Mail::attach_ics`, #48) | `/notifications` | [src/app/accounts/preferences.rs](src/app/accounts/preferences.rs), each area's `notify.rs`, [src/app/rentals/reserve.rs](src/app/rentals/reserve.rs) |
| Social login (renox-oauth) and two-factor login (renox-2fa) | `/login`, `/two-factor/setup` | [src/lib.rs](src/lib.rs), [src/app/staff/two_factor.rs](src/app/staff/two_factor.rs) |
| A walk-in customer claiming their record (signed URL) | `/claim/{customer}/{email}` | [src/app/accounts/claim.rs](src/app/accounts/claim.rs) |
| Personal API tokens with abilities | `/account/api-tokens` | [src/app/api/tokens.rs](src/app/api/tokens.rs) |
| Each store's own page on its own host: `Routes::domain`, `DomainParams`, a domain `fallback` | `north.localhost:3000/` | [src/app/home/stores.rs](src/app/home/stores.rs), [resources/views/home/store.html](resources/views/home/store.html), [layouts/store.html](resources/views/layouts/store.html) |
| Each store's own brand colour by host: `Brand` → `Theme::from_brand` (contrast-guarded), `App::theme_for` choosing by `ThemeRequest::host`, `<rx-theme/>` after `<rx-ui/>` | `north.localhost:3000/` (navy), `south.…` (rust), `west.…` (violet); the shop's host keeps the default | [src/app/home/stores.rs](src/app/home/stores.rs), [src/lib.rs](src/lib.rs), [resources/views/components/head.html](resources/views/components/head.html) |
| Every form input ↔ Rust ↔ SQLite ↔ PostgreSQL, a form that tries them all (files public and private, a photo resized with a thumbnail by the upload pipeline (`renox::upload::images`), `Uuid` keys, a deposit as a `Money` pair in USD, JPY or KWD (`#[model(money)]`, `<rx-input type="money">`), a load limit as a `Quantity<3>` with its unit in kg, g or t (`<rx-input type="quantity">`), the editors, live validation), read back on an infolist | `/about/fields` | [src/app/about/fields.rs](src/app/about/fields.rs), [resources/views/components/about_fields_form.html](resources/views/components/about_fields_form.html), [tests/fields.rs](tests/fields.rs) |
| htmx recipes, live: a modal form, out-of-band swaps (`.also`), `HxRetarget`/`HxReswap`, inline edit, Alpine tabs, `HxRefresh`/`HxRedirect`, toasts, infinite scroll | `/about/htmx` | [src/app/about/htmx.rs](src/app/about/htmx.rs), [resources/views/about/htmx.html](resources/views/about/htmx.html), [tests/htmx.rs](tests/htmx.rs) |
| The same checklist as a live component: a struct and a method per action, `rx-click`, `rx-model`, `rx-submit`, a signed snapshot | `/about/htmx/live` | [src/app/about/live.rs](src/app/about/live.rs), [resources/views/about/live/_checklist.html](resources/views/about/live/_checklist.html), [tests/htmx.rs](tests/htmx.rs) |

`/about/htmx/live` rebuilds the pre-ride checklist as a live component (same session list, so both pages show the same items) and leaves out what the checklist alone doesn't need: the bikes' infinite scroll, the duplicate retargeted onto its row (`HxRetarget`/`HxReswap`), Clear's `HxRefresh` and Ready's `HX-Redirect`, the double-click and Escape of the inline edit, and `hx-confirm` on delete. Lines of code (`wc -l`):

| | htmx | live component |
|---|---|---|
| Rust (`htmx.rs` / `live.rs`) | 382 (293 without the bikes and Ready) | 170 |
| Templates (`htmx.html` + `htmx/*.html` / `live.html` + `live/*.html`) | 170 (about 150 without the bikes) | 60 |
| Total | 552 (about 443 for the checklist alone) | 230 |

The live version has no routes, fragments or out-of-band parts of its own: `live.rs` defines one route (the page) and the component, and its 170 lines include doc comments and the two small validation structs.

### The staff side

| Feature | Page | Files |
|---|---|---|
| The staff shell (`sidebar menu="staff"` fed by `app.nav`, + `<rx-shell fill>`, the dashboard standard), the active store and its switcher, `permissions::set_scope` | `/staff` | [src/app/access/active_store.rs](src/app/access/active_store.rs), [src/app/staff/nav.rs](src/app/staff/nav.rs), [resources/views/layouts/staff.html](resources/views/layouts/staff.html) |
| Rental counter, walk-ins (`renox::select`), pick-up and return with damage photos, receipts (`Morph`) | `/staff/rentals`, `/staff/rentals/{rental}` | [src/app/rentals/counter.rs](src/app/rentals/counter.rs) |
| Fleet board: `renox::grid` with `poll`, `cards_on_mobile`, badges, related columns | `/staff/fleet` | [src/app/rentals/fleet.rs](src/app/rentals/fleet.rs) |
| Workshop board: the `kanban` block (drag or keyboard), the bench, parts from stock | `/staff/workshop`, `/staff/workshop/{order}` | [src/app/workshop/board.rs](src/app/workshop/board.rs), [src/app/workshop/order.rs](src/app/workshop/order.rs) |
| Point of sale: `keypad` and `quantity` blocks, keyboard shortcuts (`data-rx-key`), gapless receipt numbers (`Sequence`) | `/staff/counter` | [src/app/sales/counter.rs](src/app/sales/counter.rs) |
| Orders: ready, handed over, cancelled, returned (`action_sheet`) | `/staff/orders` | [src/app/sales/staff.rs](src/app/sales/staff.rs) |
| Stock: one ledger (`stock_movements`, `Morph` to its cause), a database view as a model, stock takes | `/staff/stock`, `/staff/stock/{level}`, `/staff/stock/take` | [src/app/stock/levels.rs](src/app/stock/levels.rs), [src/app/stock/ledger.rs](src/app/stock/ledger.rs), [src/app/stock/take.rs](src/app/stock/take.rs) |
| Consignment shipments between stores | `/staff/consignments` | [src/app/stock/consignment.rs](src/app/stock/consignment.rs) |
| Suppliers, a CSV or Excel price list, checked first (`renox::import`), purchase orders, daily reordering | `/staff/suppliers`, `/staff/purchase-orders` | [src/app/stock/purchasing.rs](src/app/stock/purchasing.rs), [src/app/stock/import.rs](src/app/stock/import.rs), [src/app/stock/reorder.rs](src/app/stock/reorder.rs) |
| Staff helping another store: `assign_role_in(…).from(…).until(…)` | `/staff/help` | [src/app/multistore/help.rs](src/app/multistore/help.rs) |
| Bikes placed at another store | `/staff/placements` | [src/app/multistore/placements.rs](src/app/multistore/placements.rs) |
| The books between stores, fees, monthly settlements (`Schedule::monthly_on`) | `/staff/books`, `/staff/books/settlements` | [src/app/multistore/books.rs](src/app/multistore/books.rs), [src/app/multistore/settlements.rs](src/app/multistore/settlements.rs) |
| Dashboard: the kit's `dashboard`/`widget`/`stats`, `renox::chart`, `Cache::remember` invalidated by events | `/staff/reports` | [src/app/reports/dashboard.rs](src/app/reports/dashboard.rs), [src/app/reports/numbers.rs](src/app/reports/numbers.rs) |
| Report grids grouped, summed and exported (CSV, Excel) | `/staff/reports/orders` and five more | [src/app/reports/grids.rs](src/app/reports/grids.rs) |
| The monthly report: a queue batch, workbooks, a mail with attachments, progress polled | `/staff/reports/monthly` | [src/app/reports/monthly.rs](src/app/reports/monthly.rs) |
| Stores (opening hours in a `repeater`), the team, invitations, roles per store with dates | `/staff/stores`, `/staff/team` | [src/app/staff/stores.rs](src/app/staff/stores.rs), [src/app/staff/team.rs](src/app/staff/team.rs) |
| The role × permission matrix, edited live | `/staff/roles` | [src/app/staff/roles.rs](src/app/staff/roles.rs) |
| The audit log (`Audit` module, store and role on each entry) | `/staff/audit` | [src/app/staff/audit.rs](src/app/staff/audit.rs) |
| The admin panel (renox-admin) for the catalogue, workshop, suppliers and stores, with renox-editors | `/admin` | [src/app/staff/admin.rs](src/app/staff/admin.rs), [src/app/staff/catalog_tools.rs](src/app/staff/catalog_tools.rs); the suppliers resource is contributed by its own module, [src/app/purchasing/mod.rs](src/app/purchasing/mod.rs) (`renox_admin::contribute`) |
| Kiosk tokens for the JSON API | `/staff/api-tokens` | [src/app/api/tokens.rs](src/app/api/tokens.rs) |
| Every mail, previewed | `/sales/mails`, `/plans/mails` | [src/app/sales/mails.rs](src/app/sales/mails.rs), [src/app/plans/mails.rs](src/app/plans/mails.rs) |

### Behind the pages

| Feature | Where |
|---|---|
| The app: modules, plugins, layers, the reporter | [src/lib.rs](src/lib.rs) |
| "About this page": `Explanation`, the registry, the docked panel and the sheet, `BIKESHOP_EXPLAIN` | [src/explain.rs](src/explain.rs), each area's `explain.rs`, [about_explain.html](resources/views/components/about_explain.html), [about_dock.html](resources/views/components/about_dock.html), [about_panel.html](resources/views/components/about_panel.html), [public/explain.css](public/explain.css), [public/explain.js](public/explain.js) |
| Its code samples: regions marked in the source, cut out by `build.rs`, coloured on the server | [src/code.rs](src/code.rs), [build.rs](build.rs), [src/highlight.rs](src/highlight.rs) (a copy of the docs site's) |
| Models and factories per area, migrations for SQLite and PostgreSQL, money as integers (US dollars in cents, `APP_CURRENCY=USD`; typed amounts converted in [src/money.rs](src/money.rs); a record with its own currency is a `Money` pair, as the deposit of `/about/fields`) | `src/app/<area>/model.rs`, `src/app/<area>/factories.rs`, [migrations/](migrations/) |
| Seeds (`db:seed`, the typed `demo:seed` command) and test fixtures | [src/seed/](src/seed/) |
| Scheduled tasks (below), jobs, events and listeners | each area's `tasks.rs` / `mod.rs` |
| Error reports (`App::report`) to `storage/logs/errors.log` | [src/report.rs](src/report.rs) |
| Error pages in the shop's layout, maintenance mode | [resources/views/errors/](resources/views/errors/) |
| English and Spanish, the browser's language on a first visit (`App::detect_locale`), the account's language everywhere | [resources/lang/](resources/lang/), [src/app/accounts/locale.rs](src/app/accounts/locale.rs) |
| Renox's sign-in pages in the shop's look | [resources/views/renox/auth/](resources/views/renox/auth/) |

## Access: RBAC + ABAC

Code checks **permissions**, never role names: [tests/access.rs](tests/access.rs) scans `src/`
and `resources/` and fails on `has_role`, `require_role`, `auth.roles` or a role's name. A
role is only a named set of permissions, so the owner changes what a cashier may do on
`/staff/roles` without a deploy.

**Roles are given in a store**, optionally between two dates (Renox's `Permissions` module with
`Scope`, #244); only the owner's role is global. Each request has an **active store** (the
session, checked against today's roles, then `permissions::set_scope`), and the staff side asks
for permissions there.

**Records carry store attributes**, and each action is checked against the one that matters:

| Attribute | Means | Checked for |
|---|---|---|
| **owner** | whose books the bike or goods are in | prices, retiring or selling a bike, recalling consigned goods |
| **location** | where the bike or goods are now | renting out, the counter, stock takes, repairs at the bench |
| **operating** | the store that served the customer | the rental or order itself: returns, refunds, fees earned |

| What | Where |
|---|---|
| The permission catalogue (`rentals.checkout`, `stock.adjust`, `prices.change`…) and the roles: owner, manager, cashier, mechanic, staff | [src/app/access/catalogue.rs](src/app/access/catalogue.rs) |
| `access::staff_routes(routes)`: login, the active store, `staff.access`; `POST /staff/store/{store}` switches | [src/app/access/active_store.rs](src/app/access/active_store.rs) |
| `StoreAttr`, `StoreRecord`, `require` (404 when the record isn't visible, 403 when the action isn't allowed in the store that matters), `find`, `visible::<M>(permission)` | [src/app/access/policy.rs](src/app/access/policy.rs) |
| The store switcher (the kit's `menu`) | [resources/views/components/store_switcher.html](resources/views/components/store_switcher.html) |

A staff page looks like this:

```rust
use bikeshop::app::access::{self, StoreAttr, catalogue};
use bikeshop::app::rentals::model::RentalBike;
use renox::prelude::*;

fn routes() -> Routes {
    access::staff_routes(
        Routes::new()
            .post("/staff/fleet/{id}/rate", change_rate)
            .name("fleet.rate")
            .require_permission(catalogue::FLEET_VIEW),
    )
}

async fn change_rate(State(db): State<Db>, user: AuthUser, Path(id): Path<i64>) -> Result<&'static str> {
    let bike = access::find::<RentalBike>(&db, &user, id).await?; // 404 for another store's bike
    access::require(&user, catalogue::PRICES_CHANGE, StoreAttr::Owner, &bike)?; // the owner store decides
    Ok("changed")
}
```

## The UI rules

The owner's rules for every page (#231):

1. **Renox's UI kit only** (`renox/ui.html`, `renox/grid.html`, the plugins' views), plus the
   blocks below for what the kit doesn't have.
2. **CSS Grid** for the layouts and for arranging elements ([public/app.css](public/app.css)).
3. **The kit's tokens** for type and spacing (`--rx-type-*`, the kit's spacing): no font sizes or
   spacing of the app's own.
4. **Motion with [motion.dev](https://motion.dev)**, vendored (served from the app, no CDN, so it
   works under the CSP: [public/vendor/motion/](public/vendor/motion/), see its `NOTICE`),
   animating `transform` first, never layout properties, and nothing moves under
   `prefers-reduced-motion` ([public/app.js](public/app.js)).
5. Professional, modern and tidy, at 390 px and on a desktop, light and dark.

**The storefront's look (#328).** The owner compared three mockups (bold sport, warm
editorial, playful bento) and chose **warm editorial**: a cream page, the shop's teal with a
terracotta accent, large Poppins headlines with one coloured phrase, pill buttons, rounder
corners, and real photos. It lives in [public/theme.css](public/theme.css), loaded after
`app.css`: the kit's own tokens set to the shop's values (`--rx-bg`, `--rx-surface`,
`--rx-radius-*`, `--rx-type-title`, pill `--rx-button-radius` on public and sign-in pages), so
every kit component follows in light and dark mode, plus four tokens of the shop's
(`--bs-clay`, `--bs-sand`, `--bs-hero`, `--bs-section`) and the public pages' parts: the hero
([components/hero.html](resources/views/components/hero.html), on the home, rent and plans pages),
category chips and photo tiles, service tiles, the stores strip, product cards as photos with
their text under them, the sign-in pages' photo. The phone's floating tab bar is the kit's
(`navbar(…, tabs=…)`, #346). This relaxes
rule 3 for those brand tokens only (recorded in #231); spacing stays the kit's.

**Photos.** 68 free [Unsplash](https://unsplash.com/license) photos in WebP (3.6 MB):
`public/images/products/{category}-{n}.webp` (4:3, 880 × 660) and `public/images/site/` (the
hero, the stores, rentals, the workshop), each credited in
[public/images/CREDITS.md](public/images/CREDITS.md). The seeders give each product a photo of
its category (`content::product_photo`: photo `(id % n) + 1`); migration
`20260108000000_use_product_photos` points a database seeded before then at the same photos.

### Blocks

The epic's rule was that a missing basic component stops the page and goes to the Renox team.
**The owner decided otherwise for this example** (2026-10-06, a comment on #231): the components
the kit didn't have were built here first, as **blocks**, written like a small library so they
could move to a crate. They since have (#347): twelve of them are the
[renox-blocks](../../crates/renox-blocks) plugin (guide: [docs/blocks.md](../../docs/blocks.md)),
which the app adds with `.module(renox_blocks::Blocks::new())` and a page uses like a kit
component (a tag such as `<rx-gallery …/>`). Their classes are `rx-` ones
on the kit's tokens, and the crate's script loads a block's code only on pages that have it
(Web Animations on `transform`, nothing under `prefers-reduced-motion`, no inline handlers, so
`CSP=strict` works).

The workshop's booking days (full and closed days greyed out) are the kit's own `date_picker`
with `disabled_dates` and `closed_weekdays`. Form blocks send plain fields, so `Valid<T>` reads them; the server still checks every value.
[`/about/blocks`](src/app/about/blocks.rs) shows each one working.

| Macro | What it is | Used on |
|---|---|---|
| `gallery(photos, id, label, enlarge)` | Product photos: arrows, thumbnails, swipe, arrow keys, enlarged in the kit's `sheet` | product page |
| `range_slider(name_min, name_max, min, max, step, value_min, value_max, label, format)` | Two handles on one track, sent as two fields | catalogue filters |
| `quantity(name, value, min, max, step, label)` | − / number / + stepper | product page, cart, counter, staff orders |
| `keypad(target, label, decimal, zeros, enter_label)` | A counter's number pad | point of sale |
| `kanban(id, columns, url, label, values)` + `kanban_card(card)` | Columns of cards, dragged or moved by keyboard; each move an htmx POST, put back if refused | workshop board |
| `month_calendar(month, events, url, param, today, first_day, label, heading)` | A month of events, a list on phones | a plan's visits |
| `availability(columns, rows, label, corner)` | Resources × hours/days, booked and free slots | renting |
| `datetime_range(name_start, name_end, label, …)` | Two date pickers + time selects, the duration shown | renting, walk-ins |
| `swatches(name, label, options, selected, kind, attrs)` | Size or colour chips as radios | product page |
| `history(items, label, date_format)` | A vertical timeline | a bike's service history |
| `compare_plans(plans, features, highlight, …)` | Pricing cards and a comparison table | service plans |
| `receipt(lines, totals, width, title, header, number, date, footer, …)` | An 80 mm or 58 mm till receipt that prints alone, with a code slot (#48) | a counter sale's receipt |

## The JSON API (#241)

For the self-service kiosks next to the bike racks and the shop's mobile app. Every endpoint,
its ability, request and answer is on [`/about/api`](src/app/api/endpoints.rs); the code is in
[src/app/api/](src/app/api/), the tests in [tests/api.rs](tests/api.rs).

- **Tokens.** A manager makes a kiosk's token on `/staff/api-tokens` (abilities
  `rentals:read`, `rentals:checkout`, `rentals:return`; the kiosk sees only its store); a
  customer makes theirs on `/account/api-tokens` (`read`, `rent`, `order`). A token is shown
  once. Send it as `Authorization: Bearer …`: none is a 401, one without the endpoint's ability
  a 403, another store's reservation a 404.
- **Same rules as the website**: the API calls the same functions (`booking::book`,
  `reserve::cancel_rental`, `counter::hand_over`, `counter::take_back`) and reads the same
  forms, so a refusal is Renox's `422 {"message", "errors"}`.
- **Limits**: 120 requests a minute per token (429 after), CORS for
  `https://app.bikeshop.example`. Lists are paginated with `links`; rentals are found by their
  `Ulid` code.

```bash
URL=http://127.0.0.1:3000
TOKEN='12|…'            # from /staff/api-tokens (kiosk) or /account/api-tokens (customer)

# Kiosk
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/kiosk/bikes
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/kiosk/rentals/$CODE
curl -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
     -d '{"checklist":["frame","brakes"],"method":"card"}' $URL/api/v1/kiosk/rentals/$CODE/checkout
curl -X POST -H "Authorization: Bearer $TOKEN" -F method=card \
     -F damaged=true -F 'damage_note=Bent rim' -F photos=@rim.jpg $URL/api/v1/kiosk/rentals/$CODE/return

# Customer app
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/me
curl -H "Authorization: Bearer $TOKEN" "$URL/api/v1/products?q=helmet&page=2"
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/products/trek-fx-3
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/me/bikes
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/me/work-orders
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/me/plan
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/me/rentals
curl -X POST -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
     -d '{"store":1,"bike":12,"starts_at":"2026-10-09T10:00:00","ends_at":"2026-10-09T14:00:00"}' \
     $URL/api/v1/me/rentals
curl -X DELETE -H "Authorization: Bearer $TOKEN" $URL/api/v1/me/rentals/$CODE
curl -H "Authorization: Bearer $TOKEN" $URL/api/v1/me/orders
```

## Settings

Renox's settings are in [.env.example](.env.example), with a comment for each. The shop's own:

| Setting | What |
|---|---|
| `BIKESHOP_EXPLAIN=false` | Hides the "About this page" panels for a clean demo (`/about/pages` stays) |
| `BIKESHOP_STAFF_2FA=optional` | Staff may use the staff side without two-factor login (demos, browser tests) |
| `BIKESHOP_DEMO_LOGINS=false` | Hides the demo accounts on the login page (shown only while the seeded users exist) |
| `BIKESHOP_STORE_DOMAIN` | The domain each store's own page lives under, `{slug}.<domain>` (`localhost` by default: `north.localhost:3000`) |
| `STRIPE_SECRET`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_PRICE_<PLAN>`, `XENDIT_SECRET_KEY`, `XENDIT_CALLBACK_TOKEN` | Service plans through Stripe or Xendit; without them a demo gateway of the shop's own stands in (never in production). Webhooks come to `/billing/webhooks/stripe` and `/billing/webhooks/xendit`. Card plans are charged in USD; Xendit only charges rupiah, so its plans carry their own IDR price (`RUPIAH_PER_DOLLAR` in src/app/plans/billing.rs) |
| `MIDTRANS_SERVER_KEY` | Online orders paid through Midtrans; without it a demo payment page stands in. Midtrans only charges rupiah, so a real key needs `APP_CURRENCY=IDR`; the demo works in dollars |
| `GOOGLE_CLIENT_ID`/`_SECRET`, `GITHUB_CLIENT_ID`/`_SECRET` | "Continue with Google / GitHub" (hidden when unset) |

## Operations

**One process.** `cargo run` (or the binary's `serve`) also runs two queue workers and the
scheduler (`QUEUE_WORKERS`, `SCHEDULER`), so the shop works as one process; several servers can
share one database (each job and each scheduled run is taken once). `schedule:list` shows every
scheduled task with its next run (here at 22:38 UTC):

```text
$ cargo run -- schedule:list
  2026-10-07 03:30  UTC  accounts:prune-exports
  2026-11-01 02:00  UTC  books:settle
  2026-10-07 06:00  UTC  plans:visits
  2026-10-06 22:45  UTC  rentals:watch
  2026-10-07 06:00  UTC  rentals:service
  2026-11-01 03:00  UTC  reports:monthly
  2026-10-06 22:39  UTC  sales:expire-orders
  2026-10-07 06:30  UTC  stock:reorder
  2026-10-07 18:00  UTC  workshop:reminders
```

| Task | When (`APP_TIMEZONE`) | What | Where |
|---|---|---|---|
| `accounts:prune-exports` | daily at 03:30 | deletes "download my data" files older than a week | [src/app/accounts/privacy.rs](src/app/accounts/privacy.rs) |
| `books:settle` | monthly, the 1st at 02:00 | last month's settlements between the stores | [src/app/multistore/settlements.rs](src/app/multistore/settlements.rs) |
| `plans:visits` | daily at 06:00 | books plan visits a week ahead, within the workshop's capacity | [src/app/plans/tasks.rs](src/app/plans/tasks.rs) |
| `rentals:watch` | every 15 minutes | unpaid reservations lapse, no-shows, reminders, overdue rentals | [src/app/rentals/tasks.rs](src/app/rentals/tasks.rs) |
| `rentals:service` | daily at 06:00 | fleet bikes due for a service go to the workshop | [src/app/rentals/tasks.rs](src/app/rentals/tasks.rs) |
| `reports:monthly` | monthly, the 1st at 03:00 | the monthly report: a queue batch, workbooks, a mail | [src/app/reports/monthly.rs](src/app/reports/monthly.rs) |
| `sales:expire-orders` | every minute | online orders still unpaid are cancelled and their stock released | [src/app/sales/orders.rs](src/app/sales/orders.rs) |
| `stock:reorder` | daily at 06:30 | suggested purchase orders for stock under its reorder level | [src/app/stock/reorder.rs](src/app/stock/reorder.rs) |
| `workshop:reminders` | daily at 18:00 | reminds customers of tomorrow's bookings | [src/app/workshop/tasks.rs](src/app/workshop/tasks.rs) |

- **Errors.** `App::report` hands every 500, every job that failed for good and every failed
  scheduled task to [src/report.rs](src/report.rs), which logs it with its request id and keeps
  it as a line of JSON in `storage/logs/errors.log`. In development, `/_renox/debug` lists the
  last requests with their queries.
- **Error pages** are in the shop's layout ([resources/views/errors/](resources/views/errors/):
  `default.html` for every status, `503.html` for maintenance), with no error detail outside
  debug.
- **Maintenance:** `cargo run -- down` (503 with `Retry-After` and the shop's page; `/health`
  keeps answering), `cargo run -- up`.
- **Deploy:** [Dockerfile](Dockerfile) and [deploy/](deploy/) (a systemd service and socket,
  Litestream) are exactly what `rnx make:deploy` writes; read [deploy/README.md](deploy/README.md).
  As in every example, `renox.workspace = true` means the Dockerfile builds in an app made by
  `rnx new`, not in this repository's folder.
- **CSP:** every page works under `CSP=strict` (scripts load from the app with the page's nonce,
  behaviour lives in the app's scripts under [public/](public/) (`app.js`, one per area; renox-blocks' module), no inline handlers;
  `tests/browser/bikeshop-walk.test.mjs` walks the main pages under it with a clean console).

## Tests

| What | Run |
|---|---|
| Every area's acceptance tests, the walker (every GET route has an explanation whose guide anchors and files exist), the seeds, access rules, operations | `cargo test -p bikeshop` |
| No N+1: the main pages cost the same queries on the small and the large seed ([tests/queries.rs](tests/queries.rs)) | `cargo test -p bikeshop --test queries -- --nocapture` |
| The same on PostgreSQL (CI's PostgreSQL job) | `TEST_DATABASE_URL=postgres://postgres:postgres@localhost:55432/renox_test cargo test -p bikeshop --features renox/postgres` |
| `/about/fields`' files on S3 (CI's s3 job, SeaweedFS; the commands are in [tests/fields.rs](tests/fields.rs)) | `TEST_S3_ENDPOINT=… cargo test -p bikeshop --features s3 --test fields` |
| In headless Chrome: each area's flows at 1280 and 390 px, light and dark, both CSPs; the main pages under `CSP=strict` | `tests/browser/run.sh 'bikeshop-*'` (or one: `tests/browser/run.sh bikeshop-rentals`) |
| The binary served, every page asked as a guest and as the owner | `python3 tests/process/examples.py bikeshop` (after `cargo build -p bikeshop`) |

The Rust tests are in [tests/](tests/), one file per area; the browser tests are
`tests/browser/bikeshop-*.test.mjs` at the repository's root.

**Forms from models.** The admin's brand form is not written by hand: `Brand` in [src/app/catalog/model.rs](src/app/catalog/model.rs) is `#[model(table = "brands", form)]` with `#[form(validate(...))]` rules, which generates `BrandForm` and `fill` (docs/validation.md, "Forms from models").

## Adding a page

1. Add the route to the area's `routes()` in `src/app/<area>/mod.rs`, with a `.name(…)`.
2. Add its `Explanation` to `src/app/<area>/explain.rs`: the route's name and path, purpose,
   who uses it, the Renox features and why, what happens under the hood, `docs/*.md#anchor`
   links and the source files. Use the same `api` text as other pages for the same feature
   (`/about/pages` groups by it).
3. Give it one to three code samples (`code`): mark each region in its file, then name it
   with a title that reads "Tab: what it shows":

   ```rust
   // [explain:rentals.create.handler]
   async fn create(…) -> Result<View> {
       …
   }
   // [/explain:rentals.create.handler]
   ```

   In a template the markers are comments, `{# [explain:rentals.create.form] #}` …
   `{# [/explain:rentals.create.form] #}`; in SQL `-- [explain:…]`, in CSS
   `/* [explain:…] */`. Opening the same name again later in the file adds a part (joined
   with a `…` line), to skip what doesn't matter. Then, in the explanation:
   ``code: &[Code { title: "Handler: `Valid<Booking>` checks the form", region: "rentals.create.handler" }]``.
   Keep a sample short (5–30 lines).
4. A GET route that isn't a page (JSON, a file, a stream) goes in `not_pages()` with the reason.
5. `cargo test -p bikeshop --test about` tells you what is missing: a page without an
   explanation, a link to a guide section or a file that doesn't exist, a marker never closed,
   a sample naming a region no file marks, or a marked region no page shows.

Spanish texts for an explanation go in `resources/lang/es.json` under
`about_page.<route name>` (`title`, `purpose`, `who`, `under_hood`, `features.<n>`, and,
optionally, `code.<n>` for a sample's title); whatever is missing there is shown in English.

## What building it found in Renox

Building a whole business on Renox found these; each is a Renox issue, worked around in the
example where it had to be:

| Issue | What |
|---|---|
| #299 | `request.route` / `route_is` give the wrong name on a path shared by several methods |
| #300 | An htmx.rs doc example had Indonesian text |
| #301 | `renox::Error` has no `Display` (fixed in Renox 1.1) |
| #302 | Modules can't add seeders (`App::seeder` has no `Registry` counterpart) (fixed in Renox 1.1) |
| #303 | App code can't run two statements on one generic `Executor` (fixed in Renox 1.1) |
| #304 | Full-text search: hyphenated codes match on SQLite but not when typed whole on PostgreSQL |
| #305 | tests/browser lib: `press()` has no Delete key; clicks after scrolling in phone emulation land on `<html>` |
| #306 | `Routes::etag` never answers 304 for HTML pages: the CSP nonce changes the body every request |
| #307 | Apps can't accept "signed URL or logged-in session" on one route (fixed in Renox 1.1) |
| #308 | A button's `disabled_reason` renders `aria-disabled` even when `disabled` is false |
| #309 | `App::share` / `Registry::share` closures can't read the session (fixed in Renox 1.1) |
| #310 | Permissions: no `users_with_permission_in` (who may do X in store S) (fixed in Renox 1.1) |
| #311 | `Kernel::run_scheduled` doesn't run at a travelled time |
| #312 | A template variable named like an imported macro is shadowed silently (fixed in Renox 1.1) |
| #313 | `renox::grid` can't sum or group by joined or computed columns |
| #314 | renox-admin: actions with input, an after-save hook with the user, relation managers, an About-this-page slot |
| #315 | renox-billing: guards for named subscriptions, pause, gateway webhook context |
| #316 | Auth: API tokens for a device (a kiosk) without a placeholder user; public `RequestLocale` and `normalize_email` |
| #317 | tests/browser: synthetic clicks stop reaching a phone-emulated tab after several reloads |
| #318 | Translations: `count` can't be a plain parameter (`t(key, count=…)` prints `:count`) |
| #319 | `renox::grid`: select filters bind text (integer columns can't be filtered on PostgreSQL); one-sheet exports; no heatmap chart |
