# Changelog

Notable changes to Renox. From 1.0 the project follows [semantic versioning]; see
[docs/stability.md](docs/stability.md) for what counts as a breaking change. Until the first
release on crates.io, apps made by `rnx new` are pinned to a commit, and this file lists
changes by milestone (each one pull request; details in its description and in
[ROADMAP.md](ROADMAP.md)).

[semantic versioning]: https://semver.org

## Unreleased

Right-to-left pages and Arabic, PDFs, installable and offline apps, and the building blocks a
business app keeps rebuilding: money, quantities with units, custom fields, state machines,
gapless numbers, typed settings, outbound webhooks, one-off payments and company sign-in. There
is one new crate, `renox-pdf` (the eleventh), and every change to the existing ones is
additive: `cargo semver-checks` against 1.1.1 finds nothing breaking, and 1.1 apps keep
compiling. Install it with `cargo install renox-cli`, or set `renox = "1.2"` (and
`renox-pdf = "1.2"` for PDFs).

**Behaviour changes**

- **New framework migrations:** run `migrate` after upgrading. Every app gets
  `00010101000230_create_import_batches_table` (#50),
  `00010101000310_create_webhook_endpoints_table` and
  `00010101000311_create_webhook_deliveries_table` (#38),
  `00010101000410_create_state_transitions_table` (#39),
  `00010101000420_create_sequences_table` (#40),
  `00010101000430_create_custom_fields_table` (#41) and
  `00010101000440_create_idempotency_keys_table` (#52). Apps with the `Auth` module also get
  `00010101000008_create_one_time_codes_table` (#42). The opt-in `Settings` module brings
  `00010101000400_create_settings_table` (#29), and `renox-billing` brings
  `00010101000910_create_billing_payments` (#44) and
  `00010101000920_add_gateway_intent_to_billing_payments`. Each has a SQLite file, a
  PostgreSQL file and a down file.
- **New environment variables, all optional:** `WEBHOOK_DISABLE_AFTER` (20) and
  `WEBHOOK_TOLERANCE_SECS` (300) for webhooks (#38); `IDEMPOTENCY_RETENTION_DAYS` (7) for
  offline forms (#52); `PDF_TIMEOUT` (30) and `PDF_MAX_CONCURRENT` (2) for `renox-pdf` (#30);
  `BLOCKS_MAP_TILES` and `BLOCKS_MAP_ATTRIBUTION` for the map blocks, which have no default
  tile server (#47); `MICROSOFT_TENANT` for Microsoft sign-in (#51); and the keys of the
  N-Genius gateway in `renox-billing`, `NGENIUS_API_KEY`, `NGENIUS_OUTLET_REF` and
  `NGENIUS_WEBHOOK_SECRET`, where a named account appends its name to each key
  (`STRIPE_SECRET_ACME_LLC`) (#44). `rnx new` lists them, commented out, in `.env.example`.
- **`nav()` is a request global (#37):** a template can call `nav("staff")`. A view that
  passes its own value named `nav` still wins, so existing pages are unaffected.
- **`app.dir` is available in every view (#28):** `ltr` or `rtl`, from the locale. The
  framework's own pages write `<html dir="{{ app.dir }}">`; an app's layout should do the
  same. Nothing changes for left-to-right apps.
- **The default Permissions-Policy is unchanged (#54):** geolocation and the camera stay off.
  The geolocation, barcode and camera blocks need the app to allow them with
  `App::permissions_policy`.
- **An import's row closure receives `&mut ImportTx` (#50):** it derefs to the transaction, so
  closures written for `&mut Transaction` compile unchanged.
- **New layouts use `<rx-shell fill>` (#24):** `rnx new`'s signed-in layout and renox-admin's
  layout stop the window scrolling on a desktop screen, and each panel scrolls on its own. An
  app that copied the old layout keeps it.
- **`renox-pdf` is a dependency of its own (#30):** it is not a feature of `renox` and adds
  nothing to an app that does not use it.
- **Dependencies:** `rustls` 0.23.46 (RUSTSEC-2026-0346 to 0349, #26); `calamine` for the
  `xlsx` feature (#50); a new `images` feature for upload image steps (#43). The map block
  vendors Leaflet 1.9.4 and the barcode block zxing 0.23.0, each loaded only on a page that
  uses it.

**Added: pages and the browser**

- **Right-to-left and Arabic (#28):** `i18n::direction` and `Direction`, `App::rtl_locales`
  (Arabic, Hebrew, Persian, Urdu and the other usual languages by default), `app.dir` in views
  and mail, Noto Sans Arabic bundled, arrows and chevrons that flip, and logical CSS across the
  kit, the grid, the blocks and the editors (a lint test keeps it that way).
  The guide is [docs/ui.md](docs/ui.md#right-to-left).
- **Installable apps (#33):** `App::pwa(Pwa::new(…))` serves the manifest, a versioned service
  worker and an offline page; `App::pwa_for` picks a `PwaBrand` per host; `<rx-pwa/>`
  registers the worker; `rnx make:pwa` writes the offline page, the icons and the call.
  Navigations are never cached.
- **Offline forms (#52):** `Routes::offline()` and `offline_with(Skew)` make a route
  idempotent through an `Idempotency-Key`; the `Offline` extractor gives the handler
  `recorded_at`, `received_at` and `skew_exceeded`; `<rx-form offline="name">` keeps a failed
  post in the browser and replays it in order. `rnx idempotency:prune` deletes old keys, and
  runs daily when the app has such routes.
- **Brand themes (#36):** `Theme::from_brand(&Brand::new("#1E3A5F").second(…))` makes the
  accent tokens for light and dark mode from one or two colours, and refuses a brand that
  fails WCAG contrast with the nearest passing colour (`ContrastError`). `App::theme`, or
  `App::theme_for` per host or tenant; `<rx-theme/>` links it; `Mail::theme` colours mail
  buttons and panels, queued mail included.
- **Menus from modules (#37):** `NavLink` with `App::nav` / `Registry::nav` and `nav_hide`;
  a link carries `.permission`, `.gate`, `.when`, `.group`, `.order` and a `.badge`.
  `menu="staff"` on `<rx-sidebar>` and `<rx-nav-links>`, or `nav("staff")` in a template.
- **Modules contribute to the app (#31):** `Registry::contribute`, read through
  `Contributions` in `Module::routes_with` or with `AppState::contributions`. renox-admin builds
  on it: `renox_admin::contribute` adds a resource to a panel from a module, `Admin::named`
  names a panel and `Admin::only_own_resources` ignores contributions.
- **Two named admin panels in one app (#70):** each has its own route names and module name.
- **The dashboard standard (#24):** `<rx-shell fill>` with `rx-fill`, `rx-pane` and `rx-panes`
  makes a layout whose window does not scroll on a desktop while each panel does.
- **Blocks, now twenty-one in renox-blocks:** `signature_pad` and `camera_capture` (#53);
  `rating`, `geolocate` and `barcode_scan` (#54); `status_stepper` and `week_calendar` (#55);
  `map` and `map_picker` (#47); `receipt` for 80 mm and 58 mm till printers (#48). Each is
  also a tag (`<rx-signature-pad>`, `<rx-map-picker>`…), posts plain fields or files that
  `Valid<T>` reads like any others, and works from the keyboard.

**Added: data and business logic**

- **Money (#45):** `renox::money::{Money, Currency, Rounding, Rate}` with exact arithmetic,
  `allocate`, `convert` and an explicit rounding. `#[model(money)]` stores an amount and its
  currency in two columns; `Validator::currency` and `money`, `<rx-input type="money">`, the
  `money` filter, `Column::currency_from` in the grid (a summary per currency) and
  `Field::currency_from` in the admin.
- **Quantities with units (#69):** `Quantity<SCALE>` is an exact decimal in one `BIGINT`, and
  `Units` converts between units without floats. Rules `quantity`, `quantity_min`,
  `quantity_max` and `unit`, `<rx-input type="quantity">`, the `quantity` filter,
  `Column::unit_from` and `Field::unit_from`.
- **Custom fields (#41):** administrators add fields at run time, with no migration.
  `CustomField` definitions (text, number, date, choice, bool, reference and file) with labels
  per locale and rules; the values live in one `Json<CustomValues>` column named by
  `#[model(custom_fields = "extra")]`; typed JSON paths in queries
  (`Party::EXTRA.json("segment")`, `Query::with_custom_fields`, `json_index`);
  `<rx-custom-fields>`; `Column::custom_field` in the grid; and forms, lists, filters and a
  "Custom fields" resource in renox-admin.
- **Sensitive custom fields (#71):** `CustomField::sensitive()` seals a value with `APP_KEY`,
  shows `••••••` unless the viewer holds an ability (`reveal_with`), and keeps the field out of
  sorting, filtering and exports.
- **State machines (#39):** `impl StateMachine for Order` with `Machine` and `Transition`
  (guards, permissions, required reasons), `record.transition(name)`, a `state_transitions`
  history, `#[model(state_machine)]` so `save` cannot change the status, `App::state_machine`,
  `rnx state:diagram`, `AdminAction::transitions()` and `<rx-transition-actions>`. The guide is
  [docs/state-machines.md](docs/state-machines.md).
- **Gapless sequences (#40):** `Sequence::next` numbers invoices and receipts inside the
  caller's transaction, so a rollback gives the number back; `Period` restarts a series each
  year or month, `NumberFormat` formats it, and `rnx sequence:list` shows them.
- **Typed settings in the database (#29):** `SettingsGroup`, the opt-in `Settings` module,
  `App::settings`, `settings_in_views` and `settings_from`, `AppState::settings` and
  `save_settings`, the `Setting<T>` extractor, the `SettingsChanged` event,
  `rnx settings:get|set|list`, `TestApp::with_settings` and `Admin::settings` for an admin
  page. The guide is [docs/settings.md](docs/settings.md).
- **Places (#47):** `renox::geo::{Point, Circle, Polygon}` with haversine distance,
  containment and GeoJSON, and the rules `latitude`, `longitude` and `point`.
- **Imports beyond CSV (#50):** `Import::xlsx` (the `xlsx` feature) and `Import::from_upload`
  (CSV or Excel, by content), `template_xlsx`, `.dry_run()`, `.undoable(window)` with
  `renox::import::undo`, and `rnx import:prune`.
- **Arabic-aware search (#49):** `#[model(search = "…", normalize = "arabic")]` and the
  `renox::text` helpers `normalize_arabic`, `transliterate` and `match_key`, on both databases.
- **Calendar files (#48):** `renox::calendar::{Ics, Event}`, `Mail::attach_ics` and
  `Download::ics` make invitations and cancellations (RFC 5545).

**Added: web, auth and integrations**

- **Outbound webhooks (#38):** `Endpoint::create` registers a URL and returns its `whsec_`
  secret once; `emit_in` sends signed Standard Webhooks messages from the queue with retries,
  and an endpoint is switched off after `WEBHOOK_DISABLE_AFTER` failures in a row
  (`EndpointDisabled`). `rnx webhook:endpoints`, `webhook:deliveries`, `webhook:redeliver` and
  `webhook:prune-deliveries`.
- **Signed requests (#38):** requests from `renox::http` take `.sign(key_id, secret)`, and the
  `SignedRequest<T>` extractor verifies one, refusing a replay inside `WEBHOOK_TOLERANCE_SECS`.
- **One-off payments in renox-billing (#44):** `Customer::charge(Charge)`,
  `Billing::payment_link`, `refund`, `payment_status` and `expire_links` (also the hourly
  `billing:expire-links` task), disputes, the events `PaymentSucceeded`, `PaymentFailed`,
  `PaymentRefunded`, `RefundSucceeded`, `DisputeOpened`, `DisputeWon`, `DisputeLost` and
  `PaymentLinkExpired`, gateway keys resolved per installation, several accounts per gateway
  (`Charge::account`), and a third gateway, `Billing::ngenius()`.
- **Passwordless login (#42):** `Auth::magic_link()`, `same_browser()`,
  `register_on_first_use()` and `login_codes(LoginCodes::new()…)` for codes sent by mail, SMS
  or WhatsApp; `OneTimeCode` for any other action; `LoggedIn::via` says how someone logged in.
- **Company sign-in (#51):** the `Microsoft` provider, `Google::hosted_domain` and
  `OAuth::allow_domains`; a refused account emits `OAuthRefused`.
- **Upload pipeline (#43):** `App::uploads` runs `UploadStep`s in a `Pipeline` after
  validation: `ClamAv` scanning and, with the `images` feature, `Images` (size limits,
  orientation, thumbnails). `Storage::put_encrypted` and `get_decrypted` keep a file sealed
  with `APP_KEY`.
- **Rate limits for queued sends (#46):** `App::channel_rate_limit` and
  `App::mailer_rate_limit` cap how fast queued notifications and mail go to a provider.

**Added: PDFs and tooling**

- **`renox-pdf` (#30):** `Pdf::new()` and `Document` make PDFs from Typst templates compiled
  in the process (no browser to install), with Inter and Noto Sans Arabic embedded, the
  `@renox/kit` library, a render pool capped by `PDF_MAX_CONCURRENT` and `PDF_TIMEOUT`,
  `renox_pdf::testing` and `rnx make:pdf`. The guide is [docs/pdf.md](docs/pdf.md).
- **`rnx make:package` (#34):** a reusable crate (module, migrations, views, texts and tests)
  in the workspace, registered in the app.
- **`rnx new --from` (#35):** starts an app from your own template, a directory or
  `git+<url>`, with the questions in `renox-template.toml`. Nothing from the template is
  executed.

**Fixed**

- **#26:** the `cargo-deny` job passes again, with rustls 0.23.46.
- **#66:** the semver job resolves the yanked `zip` that `calamine` needs.
- **#19:** the guides say that kit parts have no outer margin and that siblings go in
  `rx-stack` or `rx-row`.

## 1.1.1 · 2026-10-14

A patch release with no API changes: the first one published from
[renox-rs/renox](https://github.com/renox-rs/renox), so the crates' pages on crates.io link to
it.

- renox-editors uses ammonia 4.2.3: RUSTSEC-2021-0157 (CSS `url()` quoting) and
  RUSTSEC-2021-0158 (SVG animation tags) now cover every ammonia before 4.1.7 and 4.2.0–4.2.2.
  Neither could be reached through renox-editors' cleaner (no `style` attribute, no SVG), but
  `cargo audit` reported both against apps that use it (#7, #15).
- Releases are published by pushing a `v*` tag: `.github/workflows/release.yml` checks the tag
  against the workspace version, publishes the ten crates with crates.io Trusted Publishing (no
  stored token) and creates the GitHub release from this file (#3). RELEASING.md describes the
  flow and the fallback by hand.
- The landing page's canonical address is https://www.renox.rs, also the crates' homepage on
  crates.io (#1).

## 1.1.0 · 2026-10-11

Pages are written with `rx-*` tags, parts of a page can be live
components, and mistakes surface before a page is opened: `db:check`, `view:check`, typed
columns and migrations generated from the models. It also fills the gaps the bike shop had to
work around. Everything is additive: `cargo semver-checks` against 1.0.0 finds nothing
breaking, and 1.0 apps keep compiling. Install it with `cargo install renox-cli`, or set
`renox = "1.1"`. It is the code of `1.1.0-rc.1`, published earlier the same day, plus the
guides, the tutorial and the examples moved to the 1.1 way and a shorter CLAUDE.md.

**Behaviour changes**

- **Templates are compiled when they load (#372):** `<rx-…>` and `<app-…>` tags and the
  `rx-if` / `rx-else` / `rx-for` attributes now mean components. A template without them is
  left exactly as it was. One that already used such names for its own markup needs them
  renamed.
- **`#[derive(Model)]` writes a constant per column (#380):** `Product::PRICE`,
  `Product::NAME`. A model with its own associated constants of those names adds
  `#[model(no_typed_columns)]`.
- **A new framework migration (#316):** `00010101000007_create_device_tokens_table`, for apps
  with the `Auth` module. Run `migrate` after upgrading.

**Added: pages**

- **Components as tags (#372–#376, #384):** `<rx-page layout=… title=…>`, `<rx-form>`,
  `<rx-input>`, `<rx-table :rows=…>` with `<rx-column>`, `<rx-sheet>`, `<rx-navbar>`,
  `<rx-sidebar>`, `<rx-tabs>`, `<rx-wizard>`, `<rx-repeater>`, `<rx-chart>`, `<rx-grid>`
  and the rest of the kit, plus `rx-if` / `rx-else` / `rx-for` on any element and the
  `can=` / `route=` shortcuts.
  - Tags become MiniJinja macro calls when the template loads, so live reload, overriding
    built-in views, `embedded!()` and every existing template keep working.
  - A wrong tag or attribute fails with the file, the line and "did you mean".
  - The macros underneath (`renox/ui.html`) stay supported for anything without a tag.
  - The guide is [docs/ui.md](docs/ui.md#components-as-tags).
- **Your own components:** `<app-price-tag>` is `components/price_tag.html`, which declares
  its props with `<rx-props>`. `rnx make:component` writes that form; `--macro` writes the
  old one.
- **Plugins compile their templates too:** `view::add_template`, and they register their own
  tags with `Registry::component`. The editors register theirs as tags.
- **Component events (#373):** components dispatch `rx:<component>:<event>` (a sheet's
  `opened`, `closed`, `saved` and `failed`; `changed` on fields, tabs and the wizard; a
  repeater's `added` / `removed`; the grid's `selected`, `sorted` and `filtered`).
  `@closed="…"` on a tag listens to them. Attributes that aren't props pass through to the
  component's root element.
- **Live components (#386), Renox's answer to Livewire:** a part of a page whose state lives
  in a Rust struct.
  - `impl LiveComponent` (or `#[renox::live_component]`), registered with
    `App::live_component`.
  - In the template: `<live-NAME>`, `rx-click`, `rx-submit`, and `rx-model` (`.live` /
    `.blur`).
  - The browser posts to `/_renox/live/{component}/{action}`, the server runs the method and
    renders the component again, and idiomorph (0.7.3, 0BSD, vendored) morphs the page into
    it.
  - The state travels signed; `LIVE_SNAPSHOT_MAX_SIZE` caps it (64 KB).
  - `LiveContext` gives the action the session, the user, toasts, redirects and validation.
  - Tested with `TestApp::live`.
  - The guide is [docs/live.md](docs/live.md); the bike shop's `/about/htmx/live` shows one.
- **Template checks (#377):** `rnx view:check` loads every template and checks its
  components and route names; `TestApp::assert_views_compile` does the same in a test.
- **Editor autocomplete for the tags (#378):** `rnx view:data` writes the HTML custom data
  that VS Code and other editors read.
- **Typed view contexts (#383):** `#[derive(ViewData)]` and `View::typed`.
- **Charts:** `chart("heatmap", …)` (#319).

**Added: data**

- **`rnx db:check` (#370):** compares every model registered with `App::model` with its
  table and fails on a mismatch. `TestApp::assert_models_match_schema` does the same in a
  test.
- **Migrations from the models (#371):** `rnx make:migration --auto add_sku` (or `db:diff`)
  writes the difference between the models and the migrations as a plain SQL migration to
  review.
  - It writes SQL for SQLite (rebuilding the table when needed) and for PostgreSQL.
  - The model's fields can carry `index`, `unique`, `default` and `references`.
- **Typed columns (#380):** `Product::query().where_(Product::PRICE.lt(20_000))` (`eq`, `ne`,
  `lt`, `lte`, `gt`, `gte`, `is_null`, `is_not_null`, `is_in`, `like`). `order_by`,
  `order_by_desc` and `pluck` take a typed column or a name, as before.
- **Forms from models (#381):** `#[model(form)]` makes the model's form struct and `fill`,
  so a new field is added in one place.
- **Queries:** `Query::group_by_raw` (#313). `db::Conn` is public, with
  `Executor::into_conn` and `Conn::reborrow` (#303).
- **The data grid:**
  - `Column::computed(sql)` (#313): a column worked out in SQL that sorts, filters,
    searches, sums and groups like the others.
  - `grid::Workbook` (#319): several queries in one Excel file, a sheet each (the `xlsx`
    feature).

**Added: auth, plugins and the rest**

- **Device tokens (#316):** `auth::DeviceToken` and the `Device` extractor, for kiosks and
  tills that belong to a place rather than a person. `i18n::RequestLocale` (with
  `RequestLocale::switch`) and `auth::normalize_email` are public.
- **Permissions:** `permissions::users_with_permission` / `users_with_permission_in` (#310).
- **Seeders from modules:** `Registry::seeder` (#302); the app's seeders run first.
- **Signed URLs:** `signed::verify` is public, and `Option<ValidSignature>` works (#307).
- **Views:**
  - `ViewContext::session` (#309).
  - While debugging, a warning when a value passed to a view is named like a macro the
    template imports (#312).
- **Errors:** `Display` for `Error`, the message without its chain (#301).
- **renox-billing (#315):**
  - named guards: `require_subscription_named`, `require_plan_named`;
  - paused subscriptions: `Customer::pause` / `unpause` / `can_pause`,
    `SubscriptionStatus::Paused`, and the `SubscriptionPaused` / `SubscriptionUnpaused`
    events;
  - webhook hooks with the app's state: `Gateway::verify_webhook_with` /
    `parse_webhook_with`.
- **renox-admin (#314):** actions that ask for input (`ActionInput`), an after-save hook
  (`SaveContext`), relation managers for child rows (`RelationManager`), layout slots
  (`Admin::slot`), and its texts translatable as `renox.admin.*`.
- **`rnx doctor` (#379):** says what this machine or this app is missing. `rnx new` writes
  `.cargo/config.toml` when it finds a fast linker.

**Changed**

- **`rx-*` tags are the default way to write pages (#384, #493–#500):** `rnx new`, the
  starter kit, `make:module --resource`, examples/hello, every bike shop page, the guides,
  CHEATSHEET.md, llms.txt and the app agent guides all use them.
- **`make:module --resource` (#382):** runs the migration, prints the page's address, and
  `--open` opens it (`--no-migrate` leaves the migration for later).

**Fixed**

- **#307:** a signed URL checked inside a `Routes::group` was refused, because the group's
  prefix was missing from the path it checked.
- **#319:** a data grid's `select` filter on a whole-number column failed on PostgreSQL.
- **#354, browser tests:** the Spanish staff test logs in with fetch, so the Escape key
  reaches the page in CI. The cause inside Chrome is still open.
- **CI:** the semver job compares with the latest release on crates.io (#139); the browser
  suite runs in two halves (#262); and the tutorial's navigation step uses the new layout's
  tags (#501).

## 1.0.0 · 2026-10-08

The first stable release. From here Renox follows semantic versioning
([docs/stability.md](docs/stability.md)). Since rc.6 it has gained a flagship example, the
bike shop, whose look and components became Renox's design system.

**Breaking changes and behaviour changes**

- **`AppState` is one `Arc` (#334):** a clone takes about 12 ns, and a bare route answers 3.3×
  the requests per second. Moving a field out of a state no longer compiles:
  `let db = state.db;` becomes `let db = state.db.clone();` (or `&state.db`).
- **USD is the default currency (#335):** `APP_CURRENCY` defaults to `USD`.
  - Template money formatting (`| money`, `entry(format="money")`, `chart(format="money")`)
    takes the currency's smallest unit, as the data grid already did.
  - An app with a 2-decimal currency that passed whole units adds `divide_by=1`, or stores
    cents.
  - An app that relied on the old IDR default sets `APP_CURRENCY=IDR`.
- **The kit's default theme is "editorial" (#344):** a cream page, a teal accent with a
  terracotta second accent, large Poppins headlines, and pill buttons (rounded rectangles
  inside `rx-shell`). The look before is `data-rx-theme="warm"` on `<html>`; `classic` is
  unchanged.
- **`disabled_reason` (#308):** `disabled=false` now leaves the button enabled, without the
  reason.

**Added**

- **renox-blocks (#347):** a new plugin crate with eleven interactive components. Each
  block's code loads only on pages that use it. The guide is [docs/blocks.md](docs/blocks.md).
  - Input: `quantity`, `range_slider`, `keypad`, `swatches`, `datetime_range`.
  - Display: `gallery`, `history`, `compare_plans`.
  - Scheduling: `month_calendar`, `availability`, `kanban`.
- **Icons (#346):** `icon(name, size, label)` draws 112 Lucide icons (ISC licence).
  `sidebar_link`, `stat` and `empty` take `icon=`.
- **The navbar on phones (#346):** `navbar(…, tabs=…, tabs_label=…)` adds a bottom tab bar
  under 36rem; `nav_search` puts search behind a button there.
- **Closed days in the date picker (#345):** `date_picker(…, disabled_dates=…,
  closed_weekdays=…)`. The keyboard skips those days, and a typed one is refused.
- **New theme tokens:** `--rx-accent-2`, `--rx-accent-2-text`, `--rx-tint`, `--rx-accent-soft`,
  `--rx-ink`, `--rx-type-hero`, `--rx-type-section`.
- **`rnx new --starter` page patterns (#348):** hero, feature cards, figures, steps, empty
  states and stat cards, owned by the app (`resources/views/patterns.html`,
  `public/patterns.css`). It also gets a public home page with the phone tab bar, a dashboard
  welcome card, sign-in pages beside CSS art, and the seeded accounts on /login in local only.
- **Testing:** `TestApp::run_scheduled(name)` runs a task at the travelled time (#311). Templates
  get an `app.currency` global.
- **Benchmarks (#333):** benchmarks/ compares Renox, bare Axum and Laravel (FPM and Octane),
  reproducibly. Results are in benchmarks/RESULTS.md.

**Changed**

- **The kit loads rarely used parts on demand (#349):** charts and the period filter, the
  wizard, the repeater/key-value and tags. A plain page's `renox-ui.js` shrinks from 97.8 to
  76.0 KB (24.5 to 20.3 KB gzipped). A repeater already at its `max` renders its add button
  disabled.
- **One example (#351):** examples/bikeshop is the complete use case, and examples/hello the
  smallest app. The other fifteen examples are removed. Before they went, what they showed and
  bikeshop lacked moved into it:
  - each store's page on its own host (`Routes::domain`);
  - `/about/fields`, every input type ↔ Rust ↔ SQLite ↔ PostgreSQL, with files on the local
    disk or S3;
  - `/about/htmx`, every htmx recipe live.

  Every bikeshop page explains itself beside its code: the samples are cut from the source at
  build time.
- **The docs live at [docs.renox.rs](https://docs.renox.rs) (#331),** and the landing page and
  blog at [renox.rs](https://renox.rs) (`www/`, #337).

**Fixed**

- **#299:** the current route's name (`request.route`, `route_is`, `CurrentRoute`) follows the
  request's method.
- **#306:** `Routes::etag` answers 304 for pages that carry a CSP nonce.
- **#304:** a code typed whole with hyphens (`GIR-JER-0001-1`) is found by search on
  PostgreSQL.
- **#318:** `t(key, count=…)` with a count that isn't a whole number prints it instead of
  `:count`.
- **#327:** browser tests: clicks that never reached the page are retried, then sent through
  the DOM.
- **Flaky tests:** several in the bike shop and the scheduler.

## 1.0.0-rc.6 · 2026-10-06

The fixes found by testing every untested path (the coverage epic #246), and two UI kit
behaviours.

- **Back closes open sheets and menus:** a page shown again from the browser's
  back/forward cache has its sheets and menus closed, as after any other way back (a sheet's
  action form starts fresh, as after Escape).
- **Escape closes a toast (#266):** the toast that has the focus, else the newest one, once
  nothing else that Escape closes (a sheet, a menu, a popover, a tooltip) is open.
- **Charts show fractions (#282):** line, area, bar, pie and doughnut charts gave plain
  numbers 0 decimals in their tooltips and data table, so 4.5 read "5". They now get the
  decimals the data needs (up to 2), as scatter charts did; `decimals=` still decides, and
  money and percent keep their own.
- **`rnx serve` stops its app (#288):** a signal sent to `rnx` alone (`kill`, an editor's
  stop button, a closed terminal) left the app running on its port; `rnx` now passes
  SIGINT/SIGTERM/SIGHUP on to the app and Tailwind's watcher.
- **Fixed:**
  - The data grid's column menu (show/hide, freeze, move, reset) threw and saved nothing
    (#275); a date range picked on the grid's calendar wasn't applied (#285); two grids on a
    page lost each other's page and sort (#286); `match=all` and the default rows per page
    stayed in the grid's URL (#287); a poll answering during an inline edit wiped what was
    typed (#289).
  - Validation: `before`/`after` ignored a date limit given as text (#271); live
    validation of a list field didn't show its items' errors (#274).
  - Views: a failing `App::share` value showed a bare 500 instead of the error page (#272);
    an app error page that doesn't parse showed the bare fallback instead of Renox's page
    (#278).
  - PostgreSQL: a user's extra `REAL`/`SMALLINT` (#273) and `TIME` (#281) columns read as
    `null`.
  - Mail: the plain-text part repeated the text before a `<` that opens no tag (#279).
  - UI kit: a required searchable select left empty focused its hidden native select (#283);
    on phones, the `rx-shell` sidebar grew to half the screen on short pages (#284).
  - renox-editors: a code editor removed right after typing threw in the console (#290).
  - examples/hello threw under `CSP=strict` when the guestbook was sent (#276).
- **Tests:** browser tests for renox.js, the UI kit, the grid, the editors and every
  example's main flow (`tests/browser`, CI job `browser`), the binary and `rnx` as real
  processes on SQLite and PostgreSQL (`tests/process`, CI job `process`), and the coverage
  job fails under 90 % of lines (#246, #247).

## 1.0.0-rc.5 · 2026-10-05

- **Roles per branch (#244):** a role can be given in one record (a store, a team, a school)
  and for a period: `user.assign_role_in(&db, "manager", &Scope::of(&store))` with optional
  `.from(date)` / `.until(date)`, `remove_role_in`, `sync_roles_in`, `assignments`,
  `users_with_role_in`. Each request picks its scope with `permissions::set_scope(…)`, and
  the existing checks (`require_role`, `require_permission`, `has_role`, `has_permission`,
  `can()`) count the global roles plus the roles in that scope that are within their dates.
  For policies and lists: `has_permission_in(permission, scope)` (the record's scope) and
  `scopes_with::<Store>(permission)` (`All`, or the stores where the user has it;
  `Scopes::apply` filters a query on one or more columns). Migration
  `00010101000510_add_scope_to_role_user` (Permissions module); existing assignments stay
  global with no dates, so apps with global roles only see no change. `permissions:prune`
  deletes ended assignments. `route:list` marks role and permission guards that count the
  active scope. Guide: docs/authorization.md "Roles per branch".
- **New plugin: renox-oauth (#147):** sign in with Google and GitHub (another provider is one
  `Provider` impl). Accounts link by an address both sides verified; new users can sign up
  without a password and set one later; providers are unlinked from `/account` (never the
  last way in); state and PKCE are checked; the second login step (renox-2fa) still applies.
  renox-core gains `auth::sign_in`, `register_verified`, `registration_open`,
  `confirm_identity` and `User::has_password`; the account page handles users without a
  password. Guide: docs/oauth.md; examples/teams uses it.
- **New plugin: renox-admin (#148):** an admin panel generated from the models (Filament's
  resources): `Admin::new().authorize(…).resource(…)` with an `AdminResource` per model
  (grid columns, form fields, filters, actions, policy). Lists with search, filters, bulk
  actions and exports, create/edit forms, delete, a trash tab for soft deletes, a view page.
  Closed until `authorize` or `gate` is set. Guide: docs/admin.md; new example examples/admin.
- **New plugin: renox-billing (#155):** subscriptions with Stripe and Xendit (Laravel
  Cashier): plans in code, trials (with or without a payment method), checkout, swap,
  cancel, resume; the gateways' webhooks through `renox::webhook` (signatures checked, each
  event applied once, older events ignored); `require_subscription` / `require_plan`
  guards; a plans page and an account card. renox-core gains `Registry::provide`. Guide:
  docs/billing.md; new example examples/billing.
- **Fixed (#219), from the docs audit's list of suspected bugs:**
  - `.without_csrf()` didn't exempt a `Routes::resource` update route from CSRF for PUT or
    PATCH (it is listed as `PUT|PATCH`).
  - On PostgreSQL, `?` inside dollar-quoted (`$$…$$`, `$tag$…$tag$`) and `E'…'` strings was
    turned into a placeholder.
  - A migration with `CONCURRENTLY` followed by a line break or tab ran inside a transaction
    and failed on PostgreSQL.
  - `migrations!` ignored a `.postgres.down.sql` / `.sqlite.down.sql` without its own
    `.up.sql`; it is now a compile error (**breaking** for an app that has such a file:
    give it its own `.up.sql`, or delete it).
  - A job whose worker died during its last attempt went to `failed_jobs` without its
    `failed` hook or an error report; both now run.
  - `REQUEST_TIMEOUT` didn't bound the session and auth layers (database sessions waiting for
    a connection); they're now cut off one second after the handler's limit.
  - Registration skipped a second login step that every user must pass.
  - Docs: `i8` and bare `serde_json::Value` on PostgreSQL, the `file` field's Rust type,
    `.fragment()` and hx-boost, TestApp's defaults, the grid's search and selection limits,
    the 1.x promise for new commands, RELEASING.md after the first release.
- **Fixed (#226):** with live reload on, pages kept in the browser's back/forward cache held
  their `/_renox/live` stream open, and after a few navigations requests hung. Live reload
  and the notification bell's stream now close when a page is hidden and reopen when it
  comes back.
- **Fixed:** `rnx new --notifications` panicked when `rnx` was built from a checkout with
  CRLF line endings (Git on Windows).
- **Dashboards (#152):** `chart("scatter", …)` and `chart("bubble", …)` place points by two
  numbers (a bubble's area by a third), with axes fitted to the data, `x_title`/`y_title`
  (also on line, area and bar charts), `x_format`/`size_format`, a tooltip per point (the
  arrow keys move between points) and the data table. `period_filter` has a "Custom" button:
  two of the kit's date fields sent as `?period=custom&from=…&to=…` (keyboard-usable; a refused
  range shows a message), and `custom=false` to leave it out. `Period::between`,
  `Period::weeks` (`12w`), `Period::per`, `Period::is_custom`, `Bucket::Week`: `Trend` sums
  per ISO week (Monday to Sunday in `APP_TIMEZONE`, labelled by the Monday) on SQLite and
  PostgreSQL. A new template function `query_fields(…)` writes the page's query as hidden
  inputs. examples/shop's dashboard uses them.
- **Full-text search (#154):** one API on SQLite FTS5 and PostgreSQL `tsvector` (Laravel
  Scout's database engine). `#[model(search = "title, body")]` names the searched columns
  (the first weighs most in the ranking; `search_language = "simple"`, `"spanish"`, … changes
  the stemming from English); `renox::db::search::migration::<Post>(name)` is the migration
  that creates the index (an external-content FTS5 table with insert/update/delete triggers,
  or a generated `search_vector` column with a GIN index) and fills it from existing rows.
  `Model::search(words)` / `Query::search` give the matches best first, and combine with
  filters, default scopes, soft deletes and pagination; `Query::where_search` and
  `Query::order_by_relevance` are the two halves. Every word must match, as a word, a prefix
  or another form of it; only letters and digits of the input are used, bound as one value,
  so search syntax in user input is plain text. The database keeps the index current on
  every write (bulk updates, `insert_many` and raw SQL included, which skip model hooks);
  `renox::db::search::rebuild::<Post>(&db)` refills it. The data grid's search box uses the
  index when the grid's model has one (also for columns the grid doesn't show), best match
  first until the user sorts. `migrate:fresh` drops virtual tables first. New consts
  `Model::SEARCHABLE` and `Model::SEARCH_LANGUAGE` (with defaults). Guide: docs/search.md;
  examples/relations' blog search uses it.
- **Live pages (#151):** three additions around the notification bell.
  - `rnx new my-app --notifications`: the plain app with `Auth::new().notifications()` and
    the kit's `notification_bell` in its layout's bar, and a test for it (the starter kit
    already had them).
  - The app's own events over the bell's Server-Sent Events stream:
    `state.broadcast(event, data)` (every open page) and `state.broadcast_to(user_id, event,
    data)` (one user's pages) arrive as DOM events on `document`, for
    `hx-trigger="order-updated from:document"`, Alpine's `x-on:….document` or a script;
    `renox:toast` with `{"toasts": [toast]}` shows toasts. Pages without the bell open the
    stream with the kit's new `event_stream()`; one stream per page either way. Broadcasts
    are fire-and-forget and stay in the process that sends them (not stored, not seen by
    other servers or a separate `queue:work`): docs/mail.md "Your own live events" says
    when to store a database notification instead. Tests: `TestApp::fake_broadcasts`,
    `broadcasts`, `assert_broadcast` and `SentBroadcast`.
  - Toast actions that send a request: `ToastAction::post`, `put`, `patch` and `delete` (the
    new `method` field) make a button that sends it through htmx with the CSRF token, only
    to the site's own paths; a failure without a toast of its own shows "That didn't work.
    Try again." (`ui.request_failed`). They work in stored notifications too (a form in the
    bell's list) and from scripts (`Renox.request(method, url)`). New toasts from the bell's
    stream now also show the notification's own actions.
  - examples/jobs uses all three: the bell, `order-updated` broadcasts that reload the staff's
    order list, and a failed charge broadcast as a toast whose "Reopen" button posts to
    `/orders/{id}/reopen`.
- **`rnx new` (#221):** the AGENTS.md of a new app says exactly where Renox's docs are offline:
  for an `rnx` from crates.io, the `git clone --depth 1 --branch v<version> …` of the app's
  version (the downloaded crates hold only the source); for a Git pin, Cargo's checkout of that
  commit; for `--renox-path`, the local checkout.
- **Two-factor authentication (#146, #170–#173):** the `renox-2fa` crate is finished. With
  `.module(TwoFactor::new())` next to `Auth::new().account()`, users turn it on from `/account`
  (their password, a QR code, a code to confirm), get eight recovery codes shown once, and are
  asked for a code after their password from then on (`Registry::second_factor`): codes from
  the step before or after now are accepted, each works once, wrong ones count towards the
  login throttle. New recovery codes and turning it off ask for the password too. Events
  `TwoFactorEnabled`, `TwoFactorDisabled`, `RecoveryCodeUsed`, recorded by the `Audit` module
  when the app has it. The pages can be replaced from the app's views. Guide:
  docs/two-factor.md (on the docs site); examples/teams uses it.
- **Docs match the code (#208–#217, #219):** the docs-gap audit's 140 items fixed across the
  README, CHEATSHEET, llms.txt, SECURITY, RELEASING, CONTRIBUTING, every guide in docs/, the
  AGENTS.md and .env new apps get, the examples' READMEs and renox-2fa's README. Among them:
  `make:command`/`make:model` examples that failed without `--module`, test examples that
  needed `.account()`, `HxRetarget("…")` needing `.into()`, row locks that only apply to
  row-reading calls, `SET LOCAL statement_timeout`, maintenance mode not pausing jobs, the
  `Auth` module's routes, and renox-2fa described as in progress.
- **Fixed (#218):** examples/grid's Open links pointed at `/orders/%7B…%7D` (`route()` was
  given a map); they now use the id.
- **Fixed (#185):** a grid's `Column::related` (and `count_of`/`sum_of`) cells were empty
  when the page's query was `Model::unscoped()` of a model with a default scope, as staff
  pages over tenant data are, and in exports too: the related values were read again through
  the scope, which outside a tenant gives no rows. They're now read for exactly the rows the
  page shows, without the scope.
- **Fixed (#184):** a grid's `Column::money` showed the stored smallest unit as is, so with
  `APP_CURRENCY=AED` (or USD, EUR…) 4,000.00 showed as 400,000, in cells, summaries, group
  subtotals and exports, and `min.`/`max.` filters compared the typed number with cents.
  Money columns now show whole units with the currency's usual decimals, filters take whole
  units, and CSV, Excel and print exports match. Currencies without decimals (IDR, JPY) look
  the same as before. An inline edit still sends the stored value.
- **Guides for beginners, part 3 (#193):** grid.md, testing.md, postgresql.md, operations.md,
  development.md, stability.md and laravel.md rewritten in plain words; README.md and
  CHEATSHEET.md made plainer (an opening for each section, comments in the samples).
- **Guides for beginners, part 2 (#193):** relations.md, types.md, authorization.md, queue.md,
  mail.md and scheduling.md rewritten in plain words, like part 1.
- **Guides for beginners, part 1 (#193):** routing.md, validation.md and ui.md rewritten in
  plain words: what the page is for, the words it uses, short paragraphs, "what's going on"
  after each example, comments in the code samples, and Laravel notes, tips and warnings as
  callouts. Every heading (and so every link to one) is kept.
- **The tutorial, for beginners (#192):** docs/tutorial.md rewritten in plain words: what you'll
  build and learn, a table of the words web apps use, a doc comment on every function and
  comments on the lines that need one in every code sample, "what's going on" after each
  sample, and callouts for Laravel users, tips and warnings. It installs `rnx` from crates.io.
- **`Toast` is in the prelude (#175):** `use renox::prelude::*;` now brings `Toast`, which
  nearly every handler that changes something returns (`HxRefresh` already was). The
  examples, the generators and the guides drop their `use renox::Toast;`; an existing one
  still compiles.
- **Docs links (#141):** the crates' `homepage` is now https://renox.renoxium.com, and the
  README, llms.txt and new apps' `AGENTS.md` point readers there.

## 1.0.0-rc.4 · 2026-10-04

The fourth release candidate: fixes found by using rc.3, and two extension points for
plugins. Install it with `cargo install renox-cli --version 1.0.0-rc.4`; an rc.3 app moves
over by changing `renox = "1.0.0-rc.3"` to `"1.0.0-rc.4"`. Nothing in the API changed
incompatibly.

- For rc.3 apps: the kit's sheets and dialogs sit in the middle again under Tailwind's reset
  (#133), and an app no longer fails to start now and then with "pool timed out" or
  "database is locked" on SQLite (#144, #163, #178).
- For plugins: `Registry::second_factor` (a step after the password, #167) and
  `Registry::account_section` (cards on `/account`, #168), which `renox-2fa` (#146) is built
  on.
- In the repository, not yet on crates.io: `crates/renox-2fa` (#169), the first plugin
  crate, and the docs site's deploys (#141) and new look (#189–#191).
- Dependencies: `syn` 3 (renox-macros) and `serde_html_form` 0.4; the checkbox regression the
  latter brought (#160) was fixed before this release.

- **Docs site (#189, #190, #191):** code samples in coloured panels (a small highlighter in
  `site/src/highlight.rs` for Rust, templates, shell, TOML, SQL, JSON, PHP, CSS and config
  files, rendered on the server), JetBrains Mono, a copy button; the pages use the whole
  window with no sideways scrolling on phones; icons for every page (Lucide, inlined),
  a page header with what the page is for and its reading time, `> [!TIP]`-style
  callouts, and a new home page. The quick start shows the workspace's version.

- **`renox-2fa` (#169), the first plugin crate, started:** `crates/renox-2fa`, versioned with
  `renox`. `TwoFactor` (a module) brings the `two_factor` table (SQLite and PostgreSQL: one
  row per user, the TOTP secret sealed with `APP_KEY`, when it was confirmed, the last code
  used, hashed recovery codes; deleted with its user); `renox_2fa::totp` (RFC 6238 codes,
  base32, `otpauth://` URIs, one-step clock drift, a code works once), tested against the RFC
  6238 and RFC 4648 test vectors; `renox_2fa::qr::svg` (the QR code, via the `qrcode` crate,
  MIT OR Apache-2.0, no image crates). The account page, the login challenge and recovery
  codes follow (#170–#173).
- **The docs site's deploys (#141):** https://renox.renoxium.com, on the owner's Ubuntu 24.04
  server. `.github/workflows/release-site.yml` builds the site on Ubuntu 24.04 when the docs
  change on `main` and uploads the binary as an artifact; the server pulls each new one, checks
  `/health` and goes back to the previous release if it fails. GitHub holds no key to the
  server.

- **Fixed (#163, again):** opening a new SQLite file from several pools or processes at once
  could still fail with "database is locked". One try could wait out the whole busy timeout
  (five seconds, as long as the budget), leaving no time to try again. The connection that
  switches the file to WAL now waits 100 ms per try, retries are jittered (25–75 ms) so
  processes that collided don't collide again, and opening tries at least three times. The
  error now says which step failed ("switching it to WAL").
- **Account page sections (#168):** a module adds a card to `/account` with
  `Registry::account_section(template, order, |user, state| …)`; the template reads what the
  closure returns as `section.data`. They show after the built-in cards, before "Delete
  account"; a page that replaces `renox/auth/account.html` keeps them with
  `{% include "renox/auth/account_sections.html" %}`. For `renox-2fa` (#146) and
  `renox-oauth` (#147).

- **A second login step (#167):** a module can ask for something after the password, such as
  a code from an authenticator app (the coming `renox-2fa`, #146).
  `Registry::second_factor(challenge_route, |user, state| …)` says who must pass it; the
  login then waits in the session for ten minutes (`auth::pending_login`) and the browser
  goes to the challenge, whose handler finishes it with `auth::complete_login`. Wrong codes
  count towards the login throttle (`PendingLogin::failed`, `locked_out`), which isn't
  cleared until the step is passed. Two second steps, or a challenge route that doesn't
  exist, fail at boot.

- **CI (#143):** `tests/cli/run.sh` also makes apps with the `rnx new` options people
  combine, with names before and after "renox": plain, `--tailwind`, `--starter --tailwind`,
  and `--starter` with the database; each passes `cargo fmt --check`, clippy and its tests.
  The `cli` job's postgres run now runs the apps' tests and commands against a PostgreSQL
  service (`E2E_POSTGRES`), where it only built before. `__pycache__/` is ignored.
- **`tests/cli/smoke.py`** (#142), run by tests/cli/run.sh: the generated apps are used over
  HTTP the way a browser does (cookies, CSRF tokens, Referer). Every GET page answers a guest
  and a logged-in user without a server error; a `--resource` module is created (an invalid
  form first: errors and old input), listed, shown, edited (an unticked checkbox must store
  false, checked in the database) and deleted, with its toasts and a 404. On the starter app:
  sign-up lands on email verification, the seeded admin uses the dashboard, users page and
  activity log and changes a member's roles (recorded), and a member gets 403.

- **Fixed (#160):** with serde_html_form 0.4 (#137), an unchecked checkbox no longer read as
  `false`: its field was "required" or kept its old value. `Valid<T>` recognises the bool
  error by its wording, which 0.4 changed (`expected "true", "on" or "false"`); both wordings
  are recognised now, with a test that asks the installed serde_html_form.
- **Fixed (#158):** the MSRV CI job is back on Rust 1.94 (Dependabot's #135 had moved it to
  1.120); Dependabot ignores that action (#156).

- Work is tracked on GitHub: issue forms for bugs, user stories (with acceptance criteria)
  and tasks; a pull request template with the sections every PR has; Dependabot (grouped,
  weekly for crates, monthly for Actions); release-note sections. CONTRIBUTING.md "How work is
  tracked"; ROADMAP.md keeps principles, records and decisions, open work moves to issues.
- **Fixed (#133):** with `rnx new --tailwind` (or any `* { margin: 0 }` reset), the kit's
  sheets and confirm dialogs (`sheet`, `action_sheet`, `confirm`) and the data grid's confirm
  dialog opened in the top-left corner: they relied on the browser's own `margin: auto` for
  modal dialogs, which any author rule beats. `.rx-sheet` and `.rx-grid__dialog` now set
  `margin: auto` themselves; slide-overs and the phone bottom sheet keep their own margins.
- **Fixed (#144):** on a busy machine, booting an app on an in-memory SQLite database (every
  `TestApp`) could fail with "pool timed out while waiting for an open connection": sqlx opens
  the first connection within the pool's acquire timeout, which is capped at 2 s for in-memory
  databases. Opening now tries again until `DATABASE_ACQUIRE_TIMEOUT` has passed; queries keep
  the 2 s cap, so a test waiting on its own transaction still fails fast.
- **Fixed (#163):** processes opening a brand-new SQLite file database together (`serve` and
  `route:list` right after `migrate` made the file) could fail with "database is locked" or
  "pool timed out": creating the file and switching it to WAL take an exclusive lock, inside
  the pool's acquire timeout. A file database is now opened once with a plain connection
  before its pool, and both that connection and the pool retry "database is locked" until
  `DATABASE_ACQUIRE_TIMEOUT` has passed. (#178: plus one busy wait, 5 s, so a long wait for
  the lock still leaves time for another try; the pool's open also retries a pool timeout.)
- **Docs (#174):** the CHEATSHEET promised that a checkbox's `on` reads as `true` without
  saying only `Valid<T>` does that; the prelude's `Form<T>` (axum's) answers 422. The
  CHEATSHEET, docs/validation.md "Browser values" and a new app's AGENTS.md now say to read
  forms with `Valid<T>` even when they have no rules; a test pins both behaviours.

## 1.0.0-rc.3 · 2026-10-04

The third release candidate: the same API as rc.2, with three fixes found by using it and a CI
job that follows the tutorial. Install it with `cargo install renox-cli --version 1.0.0-rc.3`;
an rc.2 app moves over by changing `renox = "1.0.0-rc.2"` to `"1.0.0-rc.3"`. Files the
generators already wrote are the app's and don't change: only new `rnx make:model` and
`--resource` runs name their tables in the plural (#127).

- **Fixed:** `TestApp::travel` didn't reach jobs: `run_jobs()` ran each job in a task of its
  own (`tokio::spawn`), which starts without the task-local clock offset, so a job saw the
  real time. Jobs, their `failed` hooks, webhook handlers and scheduled runs now keep the
  clock of the code that started them (`clock::carry`). Found by following the tutorial: its
  digest test, which travels a week, failed.
- docs/tutorial.md: the code is as rustfmt prints it (a reader's `cargo fmt --check` failed
  on the seeder and the tests), and the address field is labelled "address" so its messages
  match the form ("The address field is required.", not "url").
- **`tests/tutorial/run.sh`** (CI job `tutorial`): follows the tutorial step by step, as a
  reader does (steps found by their sentences, `rnx` commands from its bash blocks), then
  runs `cargo fmt --check`, clippy, the tutorial's tests, migrate, the seeder twice, and
  checks the app answers. A tutorial that can no longer be followed fails CI.
- **Fixed (#127):** `rnx make:model --migration` named the table after the model in the
  singular (`WaitlistSignup` → `waitlist_signup`), unlike `--resource`, the docs and the
  examples. `make:model` now writes the plural (`waitlist_signups`, `categories`) in both
  `#[model(table = …)]` and the migration. `make:module <name> --resource --model <Model>`
  names the table after the model as well (`news --model Article` → `articles`, it was
  `news`); with the usual plural module (`products`) nothing changes. `rnx make:migration
  create_x_table` keeps the name it is given.
- **Fixed (#128):** a kit `card` stretched to its row's height (two cards side by side in a
  grid, one longer than the other) put the extra space between its title and its body, so
  the shorter card's body sat lower than its neighbour's. `.rx-card` now has
  `align-content: start`: the card keeps its own spacing and the extra height stays below.

## 1.0.0-rc.2 · 2026-10-04

The second release candidate: the same API as rc.1, with `rnx` fixed so a new app passes
`cargo fmt --check` (#124). Install it with `cargo install renox-cli --version 1.0.0-rc.2`;
an app made by rc.1 moves over by changing `renox = "1.0.0-rc.1"` to `"1.0.0-rc.2"`.

- **Fixed (#124):** a new app failed `cargo fmt --check` before anyone wrote a line: the
  starter kit's `tests/home.rs` sorted its imports for names before `renox` only
  (`renoxium` failed, `desk` passed), the plain app's `tests/home.rs` had two chains over
  rustfmt's width, and almost every `rnx make:*` generator wrote or edited files rustfmt
  would change (where a `mod` line or `.module(…)` lands depends on the name). `rnx new` and
  every `rnx make:*` now put the Rust files they wrote or edited through rustfmt (each file
  on its own, so the rest of the app is left alone; the app's `rustfmt.toml` applies;
  nothing happens without rustfmt), and both stubs are formatted as they are.
- README: install from crates.io, the first steps after `rnx new`, and how to use Renox with
  Claude Code or another coding agent (the `AGENTS.md`/`CLAUDE.md` every new app has, and a
  snippet for other projects). The crates.io and docs.rs badges.
- The docs site: search results styled as the list they are; an empty favicon (no 404 in
  the console).

## 1.0.0-rc.1 · 2026-10-03

The first release on crates.io, a release candidate for 1.0.0: `renox`, `renox-core`,
`renox-macros` and `renox-cli`, all at this version. Everything below, from M0 to the v1.0
steps, is in it. Try it with `cargo install renox-cli --version 1.0.0-rc.1`; apps made by
that `rnx new` depend on `renox = "1.0.0-rc.1"` (Cargo picks a pre-release only when asked).
If nothing turns up, the same code becomes 1.0.0, and the semver promise
([docs/stability.md](docs/stability.md)) starts there.

- `rnx new` from crates.io writes a pre-release version whole (`"1.0.0-rc.1"`, not `"1.0"`,
  which would never pick it).

### v1.0 · The starter kit and the semver promise (V1e)

- **`rnx new <name> --starter`:** the starter kit (Breeze and Jetstream as the yardstick),
  written over the plain app: the kit's sidebar layout with the notification bell; `Auth`
  with email verification, the account pages and notifications, landing on `/dashboard`;
  roles (`admin`, `member`) with the `Permissions` module, every sign-up a member; a dashboard
  (sign-ups over a period, a chart, the person's recent activity); a users page where admins
  change roles (recorded in the activity log); the activity log page (`Audit`, a grid with
  exports); a `users:admin <email>` command for the first admin; a seeder; seven tests.
  Works with `--database postgres` and `--tailwind`.
- **docs/stability.md** states the 1.x promise: code keeps compiling (checked by
  cargo-semver-checks), deprecation before removal in 2.0, data and sessions survive
  upgrades, jobs queued by one 1.x release run on later ones, settings keep their names and
  defaults, security fixes for the previous minor release for six months (SECURITY.md too),
  the MSRV rises only to a Rust release six months old, generated files are the app's.
- `tests/cli/run.sh` makes, lints and tests a starter app (CI's `cli` job).

### v1.0 · The documentation site, the tutorial and the Laravel guide (V1d)

- **`site/`** (package `renox-site`, not published): the documentation site, a Renox app on
  the UI kit. It compiles README.md, CHEATSHEET.md, every guide in `docs/`, the changelog and
  the project files into one binary; links between the files become site addresses, the
  doctests' hidden lines are left out, headings get anchors and a table of contents. Search,
  `/sitemap.xml`, ETags on the pages, systemd service and socket units and a Caddy recipe in
  `site/README.md`.
- **docs/tutorial.md:** one app built from `rnx new` to a server (Stash, a bookmarks app):
  a model and migration, a list page, an htmx form with toasts, a policy, a weekly mail from a
  scheduled job, tests with a factory and time travel, deploying with systemd and Litestream.
- **docs/laravel.md:** Laravel → Renox, concept by concept.
- Both guides are compiled as doctests (`TutorialGuide`, `LaravelGuide`).
- docs/scheduling.md named `on_failure`'s arguments in the old order.

### v1.0 · The API audit's should-fix items (V1c)

Cheap now, breaking later: the rest of the audit, as the owner chose (all but renaming `gate`
to `authorize_gate`). **Breaking**, item by item:

- **The state comes last in every closure:** `App::command(name, about, |args, state| …)`,
  `App::channel(name, |to, message, state| …)`, `on_failure(|err, state| …)`,
  `Auth::on_registered(|user, form, state| …)`, like `listen` and `report` already were. A
  seeder gets the `AppState` (`|state| … &state.db`), not just the `Db`.
- `AppState::routes`, `views` and `translator` are internal (`state.url(…)`, `view()`, `Lang`
  and `t()` cover them); `RouteTable` and `Views` are no longer exported.
- `state.disk(name)` is `state.disk_named(name)`, like `mailer_named`.
- `Factory` keeps `definition()` and `factory()`; `Model::make()`, `create_one(&db)` and
  `create_many(&db, n)` are gone: `factory().make_one()`, `.create_one(&db)`,
  `.count(n).create(&db)`.
- Timestamps are `DateTime`: `FailedJob::failed_at`, `BatchStatus::created_at`,
  `WebhookCall::received_at` / `processed_at`, `maintenance::Down::since` (still unix seconds
  in its file).
- `Recipient`'s fields are private: `user()`, `address(channel)`, `locale()`.
- `Session::now` is `Session::flash_now`; `i18n::set_locale` is `i18n::remember_locale`.
- `Config::load` / `from_env` / `from_vars` and `TrustedProxies::parse` return
  `renox::Result`; `Schedule::upcoming` returns `UpcomingRun`s (`name`, `at`, `zone`).
- `timezone::Zone` is opaque: `Zone::UTC`, `Zone::fixed(seconds)`, `"Asia/Jakarta".parse()`;
  `chrono-tz` is no longer part of the API.
- Sealed: `auth::Viewer`, `db::Executor`, `relations::ForeignKey`.
- `Model`'s `values`, `set_id`, `touch`, `set_deleted_at` are `#[doc(hidden)]` (implement
  `Model` with the derive); `queue::handler` / `JobHandler` are internal; `shell::run_with`
  is hidden; `auth::Can` is `#[non_exhaustive]`.
- `maintenance::down(storage, DownOptions::new().secret(…).retry(…))`.
- `Queue::retry(id) -> bool` and `Queue::retry_all() -> u64` replace `retry(Option<i64>)`.
- `Debug` for `Config`, `AnalyticsConfig`, `MailConfig` and `StorageConfig` hides `APP_KEY`,
  passwords and secrets (and `Config::vars`' values).
- `renox::minijinja` is re-exported; docs/stability.md lists `bytes` and `http` among the
  public dependencies and no longer lists `chrono-tz`.

### v1.0 · The API audit's must-fix items (V1b)

A read-only review of every public item before 1.0 found nine things that would be breaking
to change later; the owner chose to fix all of them (and the should-fix ones, next). **Every
item here is breaking.**

- **Settings are enums**, not strings: `Config::session_driver` (`SessionDriver`),
  `log_format` (`LogFormat`), `cache_store` (`CacheStore`), `MailConfig::mailer`
  (`mail::MailDriver`) and `encryption` (`mail::MailEncryption`), `StorageConfig::disk`
  (`storage::DiskDriver`), `WebhookCall::status` (`webhook::WebhookStatus`); each
  `#[non_exhaustive]` with `as_str()` and `Display`. `Config::timezone` is a
  `timezone::Zone`. An unknown value now fails when the config is read, naming the variable.
- `MailConfig::from_env` and `StorageConfig::from_env` return `Result` (an unknown driver or a
  port that isn't a number is an error naming `<PREFIX>_…`); `App::mailer` and `App::disk`
  closures return `Result` too (`|c| MailConfig::from_env(c, "NEWS")` is unchanged).
- **Lifetimes are `Duration`s:** `Config::session_lifetime`, `remember_lifetime`,
  `Session::set_lifetime(Duration)`, `auth::login(session, user, Some(duration))`. The
  `.env` values stay in minutes.
- **`Notification`:** `channels(&self, to: &Recipient)` (Laravel's `via`; `channels_for` is
  gone), `to_database(&self, to, state) -> Result<Value>`, `to_channel(&self, channel, to,
  state)`.
- **`AppState::notify(to: impl Into<Recipient>, …)`** takes a user (`&User`, `&AuthUser`) or a
  `Recipient`; `notify_to` is gone.
- **`Job::failed(self, ctx: JobContext, error)`**, like `handle`: the hook sees the job's id,
  attempt and batch.
- **`validation::Locale` is gone** (English was its only variant): `Validator::new()`,
  `Validator::rules_of(&form)`, and `Validator::new().in_lang(&lang)` for the app's
  translations; `Validator::locale()` is gone.
- **`Upload`'s fields are private:** `file_name()`, `content_type()`, `bytes()`, and
  `Upload::new(name, type, bytes)` for tests; `#[non_exhaustive]`.
- **`Migration` is built with `const` methods:** `Migration::new(name, up, down)
  .sqlite(up, down).postgres(up, down)`, `name()`; its fields are private and `db::Scripts`
  is gone. `migrations!()` writes the same calls.
- `FromDb`: docs/stability.md and the trait's docs list the types Renox promises; others that
  sqlx decodes still work, as an escape hatch tied to sqlx.

### v1.0 · Release readiness (V1a)

The first step towards 1.0, which the owner started after M34: everything needed to publish
the crates, without publishing them yet.

- `renox`, `renox-core` and `renox-macros` pin each other exactly (`=` in the workspace's
  dependencies): they're released in lockstep, as the macros write code against renox-core's
  items of the same release (docs/stability.md says so).
- `renox-macros` keeps `renox` as a path-only dev-dependency, like `renox-core`, so
  `cargo publish` drops it and the publish order (macros → core → renox → cli) has no cycle.
- docs.rs builds `renox` and `renox-core` with `postgres`, `uuid` and `xlsx`.
- `rnx new` from an `rnx` installed with `cargo install renox-cli` makes apps depend on that
  release (`renox = { version = "1.2" }`), with AGENTS.md linking to the docs at its tag; an
  `rnx` installed from git still pins the commit.
- A `semver checks` CI job compares pull requests' public API with their base branch
  (`cargo semver-checks --release-type minor`), informational until the first release.
- RELEASING.md: the release checklist (crates.io account, `cargo login`, version, changelog,
  dry runs, publish order, tag, checks, a release candidate first, yanking).

### M34 · The rest of the parity review's small additions

- `Field::current_password()`: the logged-in user's password (Laravel's `current_password`).
- `Password::uncompromised()`: refuses passwords from known breaches through Have I Been
  Pwned's range API (only five characters of the SHA-1 leave the server; allowed with a
  warning when the service can't be reached). New dependency: `sha1` (RustCrypto, the same
  `digest` as `sha2`).
- `Validator::finish_for(&state, user)` runs those two with the database checks; `Valid<T>`
  uses it. `finish(&db)` still works, without them.
- `Session::keep(&[keys])` and `Session::now(key, value)` (Laravel's `flash()->now()`).
- **Named error bags:** `Validate::ERROR_BAG` (`#[validate(bag = "login")]` with the derive),
  `ValidationError::in_bag`/`bag`, `Session::flash_errors_in`/`errors_in`/`error_bag`, the
  template functions `error('email', bag='login')` and `errors_in('login')`, and `bag=` on the
  kit's form fields. `errors` and `error()` show the default bag only.
- **Breaking:** `ValidationError` has a private field (its bag), so it can't be built with a
  struct literal any more; use `ValidationError::new`.
- **Several mailers:** `App::mailer(name, settings)`, `MailConfig::from_env(config, prefix)`
  (its driver defaults to the app's `MAIL_MAILER`), `state.mailer_named(name)`,
  `state.queue_mail_via(name, mail)` (the `renox.send-mail-via` job).
- `MAIL_FAILOVER` (`MailConfig::failover`): mailers tried in order when the default one
  fails; a permanent error (a bad address) isn't handed on; an unknown name fails at boot.
- `relations::has_many_through`: a parent's children through a middle model, in two queries.
- Examples: jobs sends its sales reports through a `reports` mailer; the relations blog's
  category page lists the latest comments of its posts with `has_many_through`.
- Docs: validation.md (bags, `uncompromised`, `current_password`), mail.md (more mailers,
  failover), relations.md (`has_many_through`), routing.md (`keep`, `now`), ui.md (`bag=`),
  operations.md, CHEATSHEET, README, the parity review and its PDF.

### M33 · Small additions from the parity review

The "small adds" the Laravel parity review still listed, before 1.0 freezes the API.

- **Validation rules:** `gt`, `gte`, `lt`, `lte` against another field (numbers, dates typed
  or as text, text by length, lists, files; two texts that read as numbers compare as
  numbers); `decimal(min, max)`; `dimensions(&Dimensions)` (pixels, read from the PNG, JPEG,
  GIF or WebP header); `prohibited`, `prohibited_unless`, `prohibits`; `required_with_all`,
  `required_without_all`; `accepted_if`, `declined`, `declined_if`; `numeric`, `integer`,
  `multiple_of`, `min_digits`, `max_digits`; `json`, `ulid`, `timezone`, `mac_address`,
  `ascii`, `hex_color`, `doesnt_start_with`, `doesnt_end_with`, `not_matches`. Each has an
  English message (`renox.validation.<key>` translates it), and the derive takes them
  (`#[validate(gt("min", &self.min))]`).
- `Upload::dimensions()`: an image's width and height.
- **Breaking:** `Inspected::File` has a `dimensions` field and is `#[non_exhaustive]`; a
  pattern on it needs `..`.
- **Route model binding:** `Found<M>` loads the row a route parameter names (the one named
  after the table, else the only one), by key or, when the parameter is named after a column
  (`{slug}`), by that column; a missing row is a 404 and default scopes apply. In the prelude.
- `Routes::view(path, template)`, `Routes::redirect(from, to)` (302) and
  `Routes::permanent_redirect` (301).
- `Routes::etag()`: an `ETag` on the routes before it, and `304 Not Modified` for a matching
  `If-None-Match`. Hashed after the page is rendered, so it works for views too.
- **Named disks:** `App::disk(name, settings)`, `state.disk(name)`, `Storage::name`,
  `StorageConfig::from_env(config, "PREFIX")` (`<PREFIX>_DISK`, `_PATH`, `_BUCKET`, `_URL`,
  the rest falling back to `S3_*`), `StorageConfig::root`. A named disk's public files are
  served at `/_renox/disks/<name>/public/…`, private ones through `temporary_url`.
- `App::xsrf_cookie()`: the CSRF token as an `XSRF-TOKEN` cookie scripts can read, accepted
  back in `X-XSRF-TOKEN` (`XSRF_COOKIE`, `XSRF_HEADER`).
- `TRUSTED_HOSTS` (`Config::trusted_hosts`): other `Host`s get a 400; `APP_URL`'s host is
  always allowed and `/health` answers any host.
- Examples: crud and the relations blog load models with `Found`; the blog's feed and sitemap
  have ETags; backoffice's exports go to an `exports` disk; uploads refuses photos over
  6000 pixels.
- Docs: validation.md (the new rules), routing.md (`Found`, view and redirect routes, ETags,
  `XSRF-TOKEN`, trusted hosts), operations.md, CHEATSHEET.md (more disks), README, the parity
  review and its PDF.

### Docs refresh after M32

- The Laravel parity review, until now the snapshot taken after M17, is rewritten for the code
  after M32: `docs/audit/2026-10-laravel-parity.md` (was `2026-09-laravel-parity.md`), with
  the milestone that closed each gap, Filament's features next to the kit and the grid, the
  gaps still open, and measured numbers.
- `docs/audit/2026-10-laravel-gap-report.pdf`, an English summary of it, replaces the
  Indonesian `2026-09-laravel-gap-report.pdf` (from M20b).
- `docs/audit/2026-09-pre-1.0.md` says every finding in it is closed.
- README: the status after M32, what the examples show today, Filament's forms, infolists,
  actions, widgets and demo in the Laravel table.
- The guides, CHEATSHEET.md, llms.txt, CONTRIBUTING.md, the example READMEs and the new
  app's AGENTS.md checked against the code. Corrections: docs/operations.md lists every
  built-in command with its options (`migrate:fresh`, `db:seed`, `db:shell`, `route:list`,
  `ui:publish`, `schedule:work` were never mentioned) and each command's default `RUST_LOG`;
  docs/ui.md's sidebar example imports `notification_bell` (it failed to render), names the
  four fields that take `hide_label`, and lists the missing `repeater`, `file`, `wizard` and
  `media_card` parameters; `Routes::webhook` takes a path in docs/routing.md;
  AGENTS.md.stub's `action_sheet` takes `action`, not `url`, and a cut sentence is repaired;
  CONTRIBUTING.md lists every CI check, the browser-check rule and the English-only rule;
  the last Indonesian words ("UMKM", `create_produk`) are gone.

### M32 · English only

Everything in the repository is now English: code, comments, docs, tests, and the examples'
seed data, names, pages and mails. Renox no longer ships a built-in Indonesian locale.

- **Breaking:** the built-in `id` texts are gone. Validation messages, auth pages and mails,
  and the kit's texts are built in for English only. Apps add any other language with
  `resources/lang/<locale>.json` (`renox.validation.*`, `renox.auth.*`, `ui.*`), as before.
  An app that relied on `APP_LOCALE=id` without a lang file now gets English.
- **Breaking:** `validation::Locale` has only `En` (still `#[non_exhaustive]`);
  `Locale::parse` returns `En` for every input.
- `rnx new` no longer writes `resources/lang/id.json`.
- The password check on the account pages names the field with the app's
  `renox.validation.attributes.<field>`, when there is one.
- `chart(…)` month labels are English for every locale; axis ticks keep the locale's
  number separators.
- Command prompts accept only English answers ("tidak" is no longer a no).
- examples/hello and examples/shop use Spanish (`es.json`) as their second language;
  hello's routes are `/hello/{name}` and `/language/{locale}`. Every example's seed data,
  names and tests are in English. Place names, `Asia/Jakarta`, phone numbers and the
  rupiah currency stay: they are data, not language.
- The framework's tests use a Spanish fixture (`crates/renox/tests/lang/es.json`) for
  everything the Indonesian built-ins used to cover.

### M31 · A warm default theme and a type scale

The owner found the kit's look plain. Three directions were tried on the examples (the
look as it was, "Warm", and a neumorphism-like "Soft"), and then Warm with a fixed type
scale. The owner chose Warm with the scale as the default.
- A new default look in renox-ui.css:
  - Inter for text and Poppins for titles and figures, bundled as SIL OFL 1.1 Latin
    subsets (about 72 KB), served from `/_renox/fonts/…` with immutable caching.
    `renox_ui()` preloads the text font.
  - A warm paper page, white surfaces with a hairline and a soft shadow, rounded-rectangle
    buttons, a white secondary button with a border, and an indigo accent. Dark mode
    included; text keeps WCAG AA.
- A type scale: `--rx-type-display`, `-title`, `-heading`, `-lead`, `-body`, `-label`,
  `-note` and `-caption`, each a whole `font`, used by every component and available to
  apps. Small capitals for table, grid, stat and infolist labels; a stat's figure is the
  largest thing on its card and shrinks with it; changes are tinted pills; a table's total
  and a card's price use the title face.
- New tokens: `--rx-font-display`, `--rx-surface-border`, `--rx-button-radius`,
  `--rx-button-padding`, `--rx-button-secondary-*`, `--rx-input-shadow`,
  `--rx-badge-radius` and `--rx-sidebar-bg`. The `--rx-text-*` sizes follow the new scale.
- `data-rx-theme="classic"` on `<html>` keeps the previous look. Its selectors use `:where`,
  so an app's own `:root` tokens (a brand colour) still win.
- **Changed look:** apps get the new theme when they update; `data-rx-theme="classic"`
  restores the old one.
- docs/ui.md "Themes and type"; the README's demo GIF re-recorded.

### M30 · Every example on the UI kit

The owner asked for every example to use Renox's own UI kit instead of markup and CSS of
their own. The audit found what the kit lacked (each kit app had copied a navigation bar
from the `rnx new` stub, the back offices had their own sidebars and headers) and five
examples that didn't use the kit at all.
- New in the kit (`renox/ui.html`, docs/ui.md "Navigation and page structure"):
  - `navbar`, `nav_links` and `nav_link`, with a "Skip to content" link and a scrolling
    links row on phones;
  - `sidebar`, `sidebar_link` and `sidebar_section` with the `rx-shell`, `rx-shell__main`
    and `rx-shell__content` layout;
  - `page_header`, `toolbar` (filters, no "optional" marks), `row_actions`, `list`,
    `columns`, `card_grid` + `media_card`, `link_tabs`, `thumbnail`, `progress`,
    `menu_button` (a menu item for htmx);
  - the classes `rx-page--fill` (a page as tall as the screen, for a grid that fills it)
    and `rx-image`;
  - `hide_label` on `input`, `select`, `textarea` and `checkbox`;
  - `cancel_label` and `fields` on `confirm`;
  - `[hidden]` always wins inside `rx-page`;
  - the texts `ui.skip` and `ui.main_navigation` (en, id).
- `rnx new` writes the layout with the kit's `navbar`; its `public/app.css` starts empty.
- Every example's pages are on the kit:
  - hello, jobs, postgres and htmx-recipes moved from their own CSS. htmx-recipes'
    recipes are now htmx attributes on kit components: `action_sheet` with
    `target`/`swap`, `menu_button`, a `checkbox` with `attrs`, a `list` of fragment rows.
  - relations uses the kit, plus Tailwind for the Markdown bodies' `prose`.
  - api's browser client is a kit page.
  - grid's frame is `rx-page--fill`, a `navbar` and `rx-grid-fill`, with the kit's
    `progress` in a cell.
  - backoffice uses `rx-shell`, `sidebar` and `page_header`; its sign-in layout is the
    built-in one with the brand mark.
  - shop, teams, crud, fields, uploads and webhooks dropped their copied navbar CSS. Shop's
    cards, filters, admin sections and row buttons are kit components.
  - shop's and jobs' mails use the kit's mail layout and components.
  - What is left in the apps' `public/app.css`: shop's brand colour tokens and backoffice's
    printed invoice.
- `AGENTS.md.stub` tells app agents to build pages from the kit.
- The README's demo GIF is re-recorded on the kit's guestbook.
- examples/grid: a filter test no longer fails on a fake name with an apostrophe.

### M29c · Examples extended

Smaller additions to four examples, the last of the M29 plan. No framework API changed.
- relations is also a public blog:
  - Markdown bodies.
  - A title and description per post (`seo()` with `Post::summary`).
  - Search (`?q=`, every word in the title or body), an RSS feed (`/feed.xml`) and a
    sitemap (`/sitemap.xml`).
  - Styled with Tailwind: the input is in resources/css/app.css, and the built
    public/css/app.css is committed.
- api has a browser client: a static page in public/ that logs in for a token and uses
  the API as a mobile app or single-page app would. `GET /api/me` says who the token
  belongs to and what it may do.
- uploads runs on S3 with its `s3` feature: public photos at `STORAGE_URL`, private
  invoices behind presigned links. A test runs it against a real server, and CI's `s3` job
  runs it on SeaweedFS.
- jobs' mail:
  - The monthly statement attaches the customer's orders as CSV and sends a hidden
    copy to the books (`bcc`).
  - Replies to receipts go to support (`reply_to`).
  - The warehouse mail copies the manager (`cc`).

### M29b · examples/backoffice

A new example: the back office of a small business, in the style of Filament's demo, built
from Renox's parts. No framework API changed.
- Grids for customers (edited in place), products (bulk activate, a stock level cell),
  invoices (grouped by status, totals, remembered filters, row menu), each product's stock
  ledger, staff (roles in a sheet) and the activity log (a model over `audit_logs`).
- Invoices: a form with line items (the kit's `repeater`, each line validated with
  `Validator::nested`), drafts that take no stock, issuing that takes it through the ledger
  in one transaction (409 and a rollback when something is short), voiding that returns it,
  "Paid in cash", and a print page.
- Online payments: Midtrans Snap or a Xendit invoice made through `state.http` (tested with
  `FakeHttp`), and their webhooks marking the invoice paid and telling the cashiers.
- A stock ledger (`stock_movements`): received, damaged, counted (the difference to the
  shelf), sold, returned, imported; stock never below zero.
- Products imported from a CSV file in a sheet (multipart), a savepoint per line.
- Invoice exports in the background: a bulk action queues a job that applies the grid's
  filters, stores the CSV and puts a link valid for a day in the bell.
- Roles (admin, cashier, warehouse) with permissions on every change, staff added by an
  admin (no registration) who verify their email, the activity log, typed company
  settings shared with every page, and sign-in pages in the company's colours.
- A dashboard: billed and collected against the period before, unpaid, overdue invoices,
  products running low. 16 tests.

### M29a · The examples checked and fixed

An audit of the 13 examples (README against code, tests, seeders, patterns) found bugs and
gaps; this fixes them. No framework API changed.
- Fixed in the examples:
  - grid: guests could edit, reorder, bulk-change and delete orders; those routes need a
    login now and the tools show only then (`orders_grid(can_edit)`). "Mark paid" and a
    status edit now set `paid` too.
  - jobs: anyone could register and then get the admin mail; registration is off.
  - shop: mail subjects and texts, and the admin's "new order" notification, were English
    only; they follow the recipient's `users.locale` (set by the language switch) now. A
    pickup order's mail no longer says "We'll send it to: Pick up at the store"
    (`orders.pickup`). `shop:make-admin` finds an email in any case; an admin's cancel that
    lost a race is a 409, not an audited no-op.
  - teams: the public team page linked to app pages its host doesn't serve; it has its own
    layout with absolute links.
  - htmx-recipes: a duplicate task was added by a plain form post; edits weren't trimmed;
    the "nothing to do" note stayed after the first add; infinite scroll repeated a row
    after an add (it goes by id now, `?before=`).
- Tests for what READMEs claimed but nothing tested: webhook repeats for every provider,
  missing secrets and unknown orders; the blog index's query count; hello's photo
  sniffing; expiring invoice links and upload limits; crud's live validation, "Nothing
  changed" and restore by a non-owner; api's CORS and nightly token pruning; postgres'
  validation, overdue boundary and NULL ordering.
- Every seeder can run twice (a seeded database is left as it is), each example tests it,
  and fields and postgres have seeders now. Every example has a `.env.example`.
- fields, webhooks and uploads are on the UI kit (tables, empty states, toasts); fields and
  uploads can delete (uploads removes the stored file too).
- Current patterns: `Routes::resource` in crud, `Redirect::route` where a route has a name,
  `#[derive(Validate)]` for plain forms, the `money` filter (and `APP_CURRENCY` in Rust
  mail) instead of hand-written "Rp", the infolist on grid's order page, dead flash lines
  and CSS gone; module docs name the generators that match the code.

### Actions: forms in sheets, icon buttons, shortcuts

- New kit component `action_sheet(id, label, action, title, …)` (Filament's actions as the
  yardstick): a button that opens a sheet with a form sent by htmx. A 422 shows its messages
  under the fields inside the sheet; a success closes the sheet and resets the form (so do
  Cancel and Esc); the handler answers with a `Toast` and `HxRefresh` (or `HxTrigger`, or a
  fragment for `target`/`swap`).
- Sheets: `slide_over=true` (at the side, full height), `width` (`sm`, `md`, `lg`, `xl`) and
  `icon`. `confirm` takes `icon` (on its button) and `modal_icon` (over its title,
  `"warning"` by default: confirmation sheets now show a warning icon).
- New `icon_button(icon, label, href=…)`: only an icon, `label` as its accessible name and
  tooltip. `button`, `link_button` and `open_button` take `icon`, `badge` (a count) and
  `key`; `button` and `icon_button` take `disabled` and `disabled_reason` (focusable, the
  reason as tooltip); `link_button` takes `new_tab`.
- Keyboard shortcuts with `key="mod+s"` (`data-rx-key`; ⌘ on a Mac, Ctrl elsewhere; plain
  keys don't fire while typing), with `aria-keyshortcuts`. Tooltips from any
  `data-rx-tip`. New icons: `edit`, `more`, `download`, `external`, `refresh`, `search`,
  `settings`, `box`.
- examples/shop: the admin product list has an "Adjust stock" action per row
  (`PUT /admin/products/{id}/stock`), edit and "view in the shop" icon buttons (disabled
  with a reason for hidden products), "New product" on `n`; the product form saves on
  ⌘S / Ctrl+S. On phones the row's actions show as icons and the price column is hidden.

### Dashboards: figures, charts and periods (renox::chart)

- New module `renox::chart` (Filament's widgets as the yardstick): `Period` (an extractor
  for `?period=7d|30d|90d|12m|mtd|ytd`, 30 days by default; `previous()`, `range`,
  `labels`, `bucket`), `Trend::of(query, column).over(period)` with `count`, `sum` and
  `average` (grouped per day or month in SQL, in `APP_TIMEZONE`, empty buckets at 0), and
  `Series` (`labels`, `values`, `total`, `change_from`, `named`).
- New template function `chart(kind, data, …)`: line, area, bar (grouped or stacked), pie
  and doughnut, drawn on the server as HTML and SVG with no JavaScript library: clean
  ticks, legends, a crosshair tooltip and keyboard navigation (in renox-ui.js), a "Show the
  data" table, six series colours validated for colour blindness in light and dark
  (`--rx-chart-1` … `--rx-chart-6`).
- New kit components: `stats` + `stat` (a figure, its change with an arrow, a sparkline, a
  link), `dashboard` + `widget` (cards in a grid; lazy `url` and `poll`), `period_filter`.
  New template function `query_with(key=value)` (the query string with keys set, `page`
  dropped). Kit texts `ui.chart.*`, `ui.period.*`, `ui.stat.vs_previous`, `ui.loading`.
- `.rx-segmented__item` also styles links with `aria-current="page"`.
- examples/shop: the admin dashboard has a period filter, four figures, revenue against the
  period before, orders per day and orders by status (loaded on their own, every minute).

### Notifications: richer toasts, and a live notification bell

- Toasts (Filament's notifications as the yardstick): `Toast::body`, `Toast::link`,
  `Toast::action` with `ToastAction::link` / `ToastAction::event` (`new_tab`), `seconds`,
  `persistent` and `id` (a new toast with the same id replaces it). `toasts(position=…)`:
  `top` (default), `top-start`, `top-end`, `bottom`, `bottom-start`, `bottom-end`. In the
  browser, `Renox.toast({…})` and `Renox.dismissToast(id)`. `ToastAction` and `ToastKind` are
  exported. Toasts already in a session still read; the JSON of a plain toast is unchanged.
- New: `DatabaseMessage` (`success`/`info`/`warning`/`error`, `body`, `url`, `link`,
  `action`, `with` for the app's own keys) to return from `Notification::to_database`, and
  `DatabaseNotification::message()`. `to_database` now runs in the recipient's language,
  like `to_mail`.
- New: `Auth::new().notifications()`: `unread_notifications` in every view, the
  `notifications.*` routes (a page that is also the bell's panel, read/unread/open/delete,
  read all, clear) and `/notifications/stream`, a Server-Sent Events stream that pushes the
  unread count and new notifications (woken at once in the same process, and looking at the
  table every 15 s for other servers; each stream lasts five minutes; all end at shutdown).
- New kit component `notification_bell(unread_notifications)`: a badge, a panel loaded on
  open, new notifications as toasts with an "Open" link; a link to the page without
  JavaScript. Built-in view `renox/notifications.html` (in `layouts/app.html` when the app has
  it). Kit texts `ui.notifications.*` (en/id).
- New `User` methods: `notifications_before`, `notification`, `mark_notification_unread`,
  `delete_notification`, `delete_notifications`.
- examples/shop: the bell in its bar, its three notifications as `DatabaseMessage`s in the
  recipient's language, a toast with a link after creating a product. examples/jobs: its admin
  notification as a `DatabaseMessage`.

### UI kit: infolists, and the money, since, words and markdown filters

- New: `infolist(columns=…, inline=…)` and `entry(label, value, …)` in `renox/ui.html`
  (Filament's infolists): read-only labels and values in a grid. `format` is `date`,
  `datetime`, `since`, `money`, `number`, `markdown`, `bool`, `color`, `image` or `key_value`;
  `badge` (a kind, or kinds by value) and `labels`; `url`, `copyable`, `tooltip`, `hint`,
  `prefix`/`suffix`, `limit`/`words`, `placeholder`; lists as commas, lines or bullets, with
  `limit_list` folding the rest behind "Show N more"; `{% call entry(label) %}` for any
  markup. `repeatable(label, items, columns=…)` shows a list of records, each a small infolist.
- New template filters: `money` (with `currency`, `decimals`, `divide_by`), `since` ("3 hours
  ago", following `TestApp::travel`), `words(n)` and `markdown` (pulldown-cmark; HTML typed in
  is shown as text and only http(s), mailto, tel and relative links are kept).
  `renox::format_money` formats an amount in Rust.
- New setting: `APP_CURRENCY` (an ISO 4217 code, `IDR` by default; anything else fails at
  boot), `Config::currency`.
- New kit texts (en/id): `ui.yes`, `ui.no`, `ui.show_more`, `ui.since.*`. The kit's copy button
  also copies a `data-rx-copy-text`.
- New dependency: `pulldown-cmark` (pure Rust, without its default features).
- examples/shop: the order page is an infolist with a `repeatable` of its lines, and every
  price uses `money` (the app's `rupiah` filter is gone, so English pages show `Rp 75,000`
  and Indonesian ones `Rp 75.000`). examples/fields: a read-only product page
  (`/products/{id}`) showing every field kind.

### UI kit: options from the server, added and renamed in the select

- New: `select(…, options_url=…)`: the options come from the server as you type (`GET ?q=…`,
  after a short pause, older requests cancelled), for lists too long for the page; a value sent
  back after a failed submit gets its label from `GET ?values=…`.
- New: `select(…, options_url=…, editable=true)`: what was typed can be added ("Add “…”",
  `POST label=…`) and is chosen at once; the chosen option (or a chip, with `multiple`) can be
  renamed in place (`POST _method=PUT value=…&label=…`; Enter saves, Escape gives up). A 422
  shows its message under the field.
- New: `renox::select`: `SelectOption` (`{value, label}`, `SelectOption::new(id, name)`) and the
  `OptionQuery` extractor (`q`, `values`, `is_lookup()`, `values_as::<i64>()`). Options in the
  template may also be `SelectOption`s (maps with `value` and `label`).
- New kit texts (en/id): `ui.searching`, `ui.load_failed`, `ui.add_option`, `ui.edit`,
  `ui.editing`, `ui.save_failed`.
- examples/shop: the admin's category select asks the server, adds categories and renames
  them (`admin/categories.rs`); the product form no longer loads every category.

### Examples: the kit's tabs and datalist

- examples/shop's admin dashboard puts low stock, notifications and recent activity on the
  kit's `tabs` + `tab_panel`; examples/teams' project name suggests common names with
  `input(…, datalist=…)` (#90).

### Docs: catch-up after the UI kit form fields

- `has_old()` in docs/validation.md and CHEATSHEET.md, `Session::has_old_input` in
  docs/routing.md, `renox_calendar()` in CHEATSHEET.md's template functions.

### UI kit: more form fields

Closer to Filament's form fields, on the kit's own rules (labels, errors, keyboard, no
JavaScript needed).

Stage 3:

- New: nested form names. A form with a name like `lines[0][name]` is read as a tree, so
  `Valid` fills a `Vec` of structs or a map; errors are keyed `lines.0.name`, and `error()`,
  `old()`, the kit's slots, live validation and `renox.js` take either spelling. A nested field
  is labelled by its own name (`lines.0.name` → "name"; translations: the full name, then
  `lines.*.name`, then `name`). Plain forms are read as before.
- New: `renox::KeyValues` (also in the prelude): ordered pairs from the kit's `key_value`,
  stored as a JSON list of pairs (a JSON object loses its order in PostgreSQL's `JSONB`).
- New kit fields: `tags_input`; `select(…, multiple=true, searchable=true)` (an ARIA
  combobox over the native select); `repeater` (rows added, removed and moved, renumbered;
  a `{% call(row, prefix) %}` block draws a row); `key_value`; `wizard` + `wizard_step`.
- New kit texts (en/id): `ui.search`, `ui.no_results`, `ui.remove`, `ui.add_row`,
  `ui.move_up`, `ui.move_down`, `ui.key`, `ui.value`, `ui.back`, `ui.next`.
- Examples: teams' "New team" is a wizard with a repeater of members (`v.nested` + `after`
  per row); fields gets tags and specifications (`Json<KeyValues>`, a new migration); shop's
  admin picks a category in a searchable select.

Stage 2:

- New: `toggle_buttons` (one or, with `multiple`, several choices as a row of buttons).
- New: `file`: a drop zone listing the chosen files, with image previews (`preview`) and the
  stored file (`current`).
- New: `date_picker`: a date typed or picked in a Cally calendar in a popover, sent as
  `YYYY-MM-DD`; the template function `renox_calendar()` loads Cally (the picker does, once).
- New: `show_when` / `hide_when`: fields shown while another field has a value; hidden ones
  are disabled, so they aren't sent.
- New: `input(…, revealable=true)` (show the password) and `copyable=true` (copy the value).
  Renox's own sign-in, registration, reset, confirmation and account pages use `revealable`.
- New kit texts (en/id): `ui.show_password`, `ui.hide_password`, `ui.copy`, `ui.copied`,
  `ui.choose_file`, `ui.choose_files`, `ui.current_file`, `ui.choose_date`,
  `ui.previous_month`, `ui.next_month`.
- Examples: shop's checkout asks courier or pickup (`toggle_buttons` + `show_when` +
  `required_if`); uploads on the kit's `file`; fields with `date_picker` and a copyable key.

Stage 1:

- New: `radio` (a radio group in a `fieldset`, options with descriptions) and
  `checkbox_list` (ticked values sent as a list), both `inline` or in `columns`.
- New: `form_grid(columns)` and `fieldset(legend)` for form layout; every field takes `span`.
- New: `input` takes `prefix`, `suffix` and `datalist`; `input`/`textarea` take `readonly`;
  every field takes `disabled`; `textarea`, `select` and `checkbox` take `id`.
- New: the template helper `has_old()` (the previous request was a failed submit) and
  `Session::has_old_input`.
- Fixed: after a failed submit, a checkbox the user unticked came back ticked when its
  default was `checked=true`.
- Fixed: the error summary's links missed a field given its own `id`; they now find the
  field by its name.

### Examples: the rest of docs/grid.md

examples/grid gets `/follow-up`: two grids on one page with their own query string prefixes
(`prefix`), and the column options docs/grid.md describes that no example used yet: `link`,
`tooltip`, `wrap`, `limit`, `sortable(false)`, `filterable(false)`, `per_page` and
`Column::related(…).numeric()`.

### Guides for routing, validation, mail and scheduling

- New guides, each compiled as a doctest: docs/routing.md (`RoutingGuide`: routes, groups,
  domains, extractors and responses, middleware and guards, sessions, CSRF, cookies, signed
  URLs), docs/validation.md (`ValidationGuide`: `Valid<T>`, every rule, `#[derive(Validate)]`,
  hooks, messages), docs/mail.md (`MailGuide`: mail and notifications) and docs/scheduling.md
  (`SchedulingGuide`: the scheduler, events, the cache and locks, app commands). Before, these
  areas were only in CHEATSHEET.md.
- README, llms.txt and CLAUDE.md list the new guides.

### Docs audit after M28

Fixes found by the audit:
- Data grid: date-time filters (the date range and the advanced filter's `on`/`before`/`after`)
  take whole days of `APP_TIMEZONE`, the zone the cells are shown in. They took UTC days, so
  with `Asia/Jakarta` a filtered day was off by seven hours.
- `User::delete_account` (the account page's "delete account") also deletes the user's
  `grid_preferences` rows, which used to stay behind.
- New: `notifications:prune [--days 30]` (Auth module) and
  `renox::auth::prune_read_notifications(db, age)` delete notifications read more than that
  ago; unread ones stay. Nothing pruned them before.

Docs:
- New guide: docs/grid.md for `renox::grid` (compiled as the `GridGuide` doctest).
- CHEATSHEET: the missing patterns filled in (queries, the UI kit's macros, the data grid,
  commands, config).
- docs/postgresql.md: an unknown `DATABASE_URL` scheme is refused at boot.
  docs/operations.md: when HSTS is sent, housekeeping for `notifications` and
  `grid_preferences`, the prune functions. docs/authorization.md: async gates and `can()`.
  docs/stability.md: the `#[non_exhaustive]` list.
- README and CLAUDE.md: the `renox` crate's default features include `http`.
- ROADMAP: the default scope API (`#[model(default_scope = "…")]`, `Model::unscoped()`),
  `gate_before`'s closure, the principles and app layout, and the audit's open issues.
- The examples' READMEs and module docs, llms.txt, and the new-app `AGENTS.md.stub` and
  `env.stub` brought in line with the code.

### M28e · Data grid: related columns, an advanced filter, remembered state, polling

- New: `Column::related`, `Column::count_of`, `Column::sum_of` (values from other tables,
  sorted, filtered and searched), `Grid::advanced_filter` (rules with operators per kind, all
  or any), `Grid::remember` (the grid's state in the session) and `Grid::poll`.
- Changed: grid sorting puts empty values last in either direction, on both databases.

### M28d · Data grid: cards on phones, more kinds of cells

- New: `Grid::cards_on_mobile()`: rows as cards on phones, with sorting and filters in the
  toolbar.
- New: `Column::image`, `Column::color`, and `badges`, `icons`, `description`, `tooltip`,
  `wrap`, `limit`, `link`, `copyable` and `round` on columns (`grid::Kind::Image`/`Color`).

### M28c · Data grid: summaries and groups

- New: `Column::summary` (`grid::Summary`: sum, average, range, count) in a footer that stays
  at the bottom, over every filtered row; `Grid::groups` / `Grid::group_by`: rows grouped with
  a folding heading and the group's own summaries.
- Fixed: a grid column whose value the row doesn't have shows empty instead of failing the
  page.

### M28b · Data grid: selecting rows, bulk and row actions

- New: `Grid::bulk_action` with row checkboxes, a "select all matching" choice and
  `Grid::selected` (`grid::Selection` from the form); `Grid::row_action` for each row's menu;
  `grid::Action` (`new`, `link`, `method`, `confirm`, `danger`) with a confirmation dialog.

### M28a · Data grid: search, filter chips, row links, several grids on a page

- New: `Column::searchable()` and the toolbar's search box (every word, any searchable column,
  as you type); the active filters as chips that clear one at a time.
- New: `Grid::row_url` (a click on a row opens it), `Grid::empty_state`, and `Grid::prefix`
  for several grids on one page; a grid keeps the page's other query string values.

### M27d · Data grid: moving and resizing columns

- New: drag a heading to move its column; drag a heading's edge to resize it (double-click for
  the automatic width, arrow keys from the keyboard). Widths are kept per user with the other
  column choices (`GridPrefs::widths`).

### M27c · Data grid exports

- New: `Grid::exports()` and `Grid::export`: CSV, Excel and a print page (for PDF) of every
  row the filters match, in the user's columns.
- New: the `xlsx` feature (`rust_xlsxwriter`, off by default) for Excel exports, with merged
  headings, typed numbers and dates, and frozen panes.

### M27b · Data grid: details, editing, row order, merged cells

- New: `Grid::audit()` opens who created and last changed a row (and when) under it;
  `Grid::details()` adds what the page draws for `_details`.
- New: `Column::editable()` with `Grid::edit_url`: cells edited in place (double-click, Enter,
  F2) or a whole row in edit mode, sent as `PATCH` and validated by the app's `Valid<T>`,
  errors shown in the cells.
- New: `Grid::reorder(column, url)`: rows dragged (or moved with the arrow keys) into order
  while sorted by that column, saved with `grid::RowOrder::save`.
- New: `Column::merge()`: neighbouring equal values share one cell, nested from left to right.
- `Grid::sort_by` takes several keys (`"region,city,-total"`); the default sort no longer
  goes into the URL.

### M27a · Data grid

- New: `renox::grid`, a data grid for dashboards: `Grid` and `Column` in Rust (text, number,
  money, date, datetime, bool, select, tags and custom columns), `GridRequest`, and the `grid`
  macro of `renox/grid.html`. It fills its container with only the rows scrolling, filters each
  column by its kind from the query string (text with `%` patterns, ranges, a date range
  calendar, choices), sorts and pages on the server, groups headings (`Column::under`), shows
  different columns on phones and desktops, and freezes columns left or right.
- New: the column menu's choices are kept per user in the new framework table
  `grid_preferences` (migration `00010101000220_create_grid_preferences_table`, in every app),
  or in the session for guests (`POST`/`DELETE /_renox/grid/{grid}/prefs`).
- New: `{{ sparkline(values) }}`, a small line or bar chart as inline SVG.
- Bundled: Cally 0.9.2 (MIT), the calendar web components the date filters use
  (`renox::CALLY_VERSION`).
- New example: examples/grid, a sales dashboard.

### M26c · Tests for the CLI and the weak spots

- New: `App::run_args(["migrate:status"])` runs any command of the app binary from code (a
  test, or a program driving the app), as `my-app migrate:status` would.
- Fixed: a handler taking `renox::Path` on a route without that parameter answered 404; that
  is the app's mistake, so it is a 500 now. A value that doesn't parse is still a 404.
- Tests for `rnx`: argument parsing, `make:module --resource` (fields, plurals, every file
  written and registered), `rnx new` (every placeholder filled, PostgreSQL apps), the
  `serve` fingerprint, `key:generate`'s `.env` edit, Tailwind detection. Tests for the
  binary's built-in commands, `DbError`'s questions and `db::Json`.

### M26b · Docs brought up to date

- Every public item has a doc comment now (379 were missing, mostly struct fields and
  methods); `#![warn(missing_docs)]` keeps it that way in CI.
- `RedirectExt` is sealed and `db::InvalidUlid` is `#[non_exhaustive]` (neither was meant to
  be implemented or built by apps).
- Guides, README, CHEATSHEET, llms.txt, the example READMEs and the files `rnx new` writes
  now cover M22–M25: model keys, savepoints, `Encrypted`, domain and fallback routes,
  `route_is`, `class_names`, loops, plural ranges, factory states, `#[derive(Validate)]` and
  `detect_locale`.

### M26a · Key fixes, and examples for M22–M25

- Fixed: `insert_many` and `upsert` left the key out for `Ulid`, `Uuid` and `String` keys (a
  NULL key on SQLite, an error on PostgreSQL). They now make ULIDs and UUIDs, write `String`
  keys (a missing one is an error), and `upsert` may use `id` as its conflict target for
  such keys.
- Fixed: `unique(…).ignore(id)` took only an `i64`; it takes any key now.
- Fixed: a `Routes::fallback` answer became a 404 whenever the app had a `public/`
  directory (tower-http's `not_found_service` overrides the status); a fallback's own
  status (a redirect, a 200 page) is kept now.
- examples/crud: the form derives `Validate` with a `prepare` hook; `products:import` imports
  a CSV in one transaction with a savepoint per line; the seeder uses factory states and a
  sequence.
- examples/api: products are keyed by `Ulid` (public ids, and the list's cursors).
- examples/shop: "recently viewed" with `session.push` and `{% break %}`, stock texts with
  plural ranges, `class_names` on sold-out cards, `Redirect::route` after checkout and in the
  admin.
- examples/teams: each team's public page on its own host (`Routes::domain("{team}.localhost",
  …)`, `DomainParams`, a domain fallback), with a `slug` column (a new migration).

### M25 · Derived validation and the browser's language

- `#[derive(Validate)]`: rules as attributes on the form's fields,
  `#[validate(required, max = 100, unique("users", "email"))]`; `each(…)`, `distinct`,
  `rename = "…"`, `label = "…"`. `#[validate(hooks)]` with `impl ValidateHooks` for
  `prepare`, `authorize` and `after`. `impl Validate` by hand still works.
- `App::detect_locale()`: visitors who haven't chosen a language get their browser's
  (`Accept-Language`) when the app has texts for it; responses carry
  `Vary: Accept-Language`.
- `rnx make:module --resource` writes the form with `#[derive(Validate)]`.
- examples/hello: its form uses the derive, and the guestbook follows the browser's
  language until a visitor picks one.

### M24 · Laravel's leftovers from M21

- `Routes::domain("admin.example.com", routes)` and `Routes::domain("{account}.example.com",
  …)` with the `DomainParams` extractor: routes for other hosts, where the same path may
  mean another page. A host that matches a domain gets that domain's routes (plus Renox's
  own and the public files); other hosts get the routes without a domain. `route:list`
  shows a DOMAIN column when there are any.
- `Routes::fallback(handler)`: what answers when no route and no public file does.
- `route_is('admin.*', …)` and `request.route` in views; the `CurrentRoute` extractor.
- `Redirect::route("products.show", &[&id])?` and `Redirect::intended(&session, "/")`
  (`RedirectExt`, in the prelude).
- `session.push(key, value)` and `session.increment(key, by)`.
- `Product::factory().count(3).state(f).sequence(|i, p| …).create(&db)` (and `make`,
  `make_one`, `create_one`).
- Plural ranges in translations: `"{0} Sold out|[1,5] Only :count left|[6,*] In stock"`.
- `{% break %}` and `{% continue %}` in templates, and `class_names('tab', {'active': on})`.
- `RouteInfo` has a `domain` field (it's `#[non_exhaustive]`); `RouteTable::name_of`
  (new) takes the domain.
- examples/shop's admin nav marks its section with `route_is`.

### M23 · Savepoints and encrypted fields

- `Transaction::savepoint(|tx| Box::pin(async move { … }))`: a transaction inside the
  transaction; on `Err` only its changes are undone and the transaction goes on (on
  PostgreSQL too, after a failed statement). Savepoints nest.
- `renox::db::Encrypted<T>`: a model field stored encrypted with `APP_KEY` (AES-256-GCM,
  sealed JSON in a `TEXT` column) and read as `T`; `Option<Encrypted<T>>` for nullable
  columns. Works in handlers, jobs, commands, seeders and tests alike: the app's `Db` carries
  the key. Its `Debug` hides the value; the column can't be searched.
- `DbValue` has a new variant, `Encrypted` (it's `#[non_exhaustive]`).
- examples/teams keeps its webhook secret in an `Encrypted<String>` field.

### M22 · Model keys other than integers

- A model's key is its `id` field's type: `i64` as before, or `renox::db::Ulid` and `Uuid`
  (renox's `uuid` feature; a v7), both made on insert, or a `String` the app sets. `Model` has
  `type Key: ModelKey`; `id()`, `set_id`, `find`, `find_or_404` and `find_many` use it.
- `renox::db::Ulid`: sortable 26-character ids (monotonic within a millisecond), stored as
  text, serialized as text; a malformed one in `Path<Ulid>` is a 404. `renox::uuid` is
  re-exported with the `uuid` feature.
- `Model::insert`: always an INSERT, with the key set or a new one.
- Relation loaders are generic over keys: `belongs_to`, `has_many`, `count_many`, `sum_many`
  and `Morph` return maps keyed by the parent's key; `Pivot<L = i64, R = i64>` names the two
  sides' key types (`Pivot` alone is unchanged).
- `chunk` and `cursor_paginate` work with any key type (cursors are the key's text).
- `rnx make:model Invoice --key ulid|uuid|string [-m]` writes the model and its migration.
- examples/fields keys its products by `Uuid` instead of an extra `public_id` column (run
  `migrate:fresh` there).
- **Breaking:** generic code over models that uses `id()` as an `i64` needs
  `M: Model<Key = i64>`. `ForeignKey` takes the key type (`ForeignKey<K>`). Hand-written
  `impl Model` blocks add `type Key = i64;`. `create` now always inserts: a model created with
  a non-zero `id` keeps it (it used to update that row).

### After M21 · Small fixes

- `/favicon.ico` answers `204 No Content` (cached for a day) unless the app has
  `public/favicon.ico`. Browsers ask for it on every site; the 404 it got before ran the
  whole middleware stack, rendered the error page and logged a console error each time.
- The workspace dev profile uses `debug = "line-tables-only"` (#68): full debug info made the
  test binary 415 MB and workspace builds ran out of memory.

### M21i · The examples' tests, typed commands and the stubs

- Fixed: `TestApp::travel` didn't reach the in-memory rate limits (`Routes::throttle`,
  `throttle_by`) or the login lock: they measured time with `Instant`. They now read Renox's
  clock.
- Fixed: after `TestApp::travel` past the session lifetime, `TestApp`'s CSRF token and
  `acting_as` still used the old session, so the next form post got a 419; they now see the
  moved clock, as the server does.
- examples/jobs: `App::report` posts errors to a chat webhook (`ERROR_WEBHOOK_URL`); tests use
  time travel for retries and unique jobs, `fake_events`, `fake_notifications` and
  `fake_http` for the reporter.
- examples/api, shop, hello: tests use `travel` instead of rewriting dates in SQL, plus
  `assert_view`, `assert_json_path` and `assert_json`; shop runs its daily task by name.
- examples/hello: `entries:prune` is a typed command that asks before deleting (`--force`).
- examples/fields: colours checked with `each` + `one_of` and `distinct`.
- `rnx new`: `tests/home.rs` shows `assert_view` and time travel; `.env` lists `APP_HOST`,
  `DATABASE_POOL_SIZE`, `SESSION_LIFETIME`, `REMEMBER_LIFETIME`, `SESSION_COOKIE` and the
  paths; `AGENTS.md` lists every guide and adds traps about the clock and test fakes.

### M21h · The examples on the UI kit

- examples/shop and examples/teams: every page on the UI kit (navigation bar with an account
  menu, kit fields and tables, badges, toasts instead of flashed messages, confirmation sheets
  for deleting and cancelling, live validation, an error page in the layout); the shop
  rebrands the kit's accent. `route()` query arguments for filter links; teams' secret uses
  `renox::random_token()`.
- examples/htmx-recipes: an out-of-band count (`.fragment("row").also("count")`),
  `HxRetarget`/`HxReswap` for a duplicate task, toasts over htmx, `route()` for the scroll
  loader.
- Fixed: toasts with non-ASCII text (curly quotes, accents, emoji) were dropped from htmx
  responses, because a header can't hold them raw; `HX-Trigger` now escapes them in its JSON.
- Fixed: a toast returned with `HxRefresh` was sent in `HX-Trigger` and lost on the reload; it
  now waits in the session, as with `HxRedirect`.
- Fixed: error pages didn't get `App::share` values, so a layout using one (a cart count)
  failed to render them with `APP_DEBUG` (Renox's page showed instead).

### Docs and examples checked against M21 (#63)

- The socket recipe stops the service before enabling the socket (systemd can't listen while
  the app holds the port); the shop admin's product delete asks in the kit's confirmation
  sheet (its `hx-confirm` never ran); the shop's order policy checks the admin role itself and
  uses `users_with_role`; crud's layout shows flashed messages; `rnx make:mail` prints how to
  actually send the mail; authorization.md, stability.md, the README, llms.txt and the ROADMAP
  no longer describe pre-M21 limits.

### M21g · Database sessions and deploys without refused connections

- `SESSION_DRIVER=database`: sessions in a `sessions` table (new framework migration
  `00010101000210`), the cookie holds only an id; a new id at each login and logout; rows keyed
  by the id's SHA-256; `user_id` column. Cookie sessions carry over when switching.
  `session:prune`, `Session::prune_expired(&db)`.
- Every app gets the `sessions` table (with either driver): run `migrate` after upgrading.
- `serve` accepts a listening socket from systemd (socket activation, `LISTEN_FDS`). `rnx make:deploy` writes
  `deploy/<app>.socket`; its README explains deploys without refused connections, migrations
  that old and new code both accept, and two copies behind Caddy.
- `Toast` and `ToastKind` are `#[non_exhaustive]` (build toasts with `Toast::success(…)` and
  friends).

### M21f · Form requests, more rules, auth pages on the kit

- Form requests: `Validate` gains optional `prepare`, `authorize` (403 before the rules) and
  `after` (async checks once the rules pass, errors shown on the field), with `FormContext`.
- Rules: `alpha`, `alpha_num`, `alpha_dash`, `lowercase`, `uppercase`, `starts_with`,
  `ends_with`, `uuid`, `ip`, `size`, `required_without`, `prohibited_if`,
  `Validator::distinct`; English and Indonesian messages.
- Renox's sign-in and account pages use the UI kit (and have stacks). Deleting the account asks
  in a kit sheet instead of a browser dialog. The kit's `input` takes `id=`.
- examples/teams adds members through a form request.
- **Changed:** apps that override `renox/auth/*.html` keep their files; apps that styled the
  old markup (`.card`, `.error`, `.status` in `renox/auth/layout.html`) should restyle for the
  kit's classes.

### M21e · Tailwind, stacks and typed commands

- Tailwind CSS without Node: `rnx new --tailwind`; an app with `resources/css/app.css` gets
  Tailwind in `rnx serve` (watch) and `rnx build` (minified) into `public/css/app.css`. `rnx
  tailwind [--watch] [--minify]`, `rnx tailwind:install`. The standalone CLI (v4.3.3) is
  downloaded once and checked by SHA-256; `TAILWIND_BIN` and `RNX_CACHE_DIR` override.
- Stacks: `{{ stack('scripts') }}` in the layout; `{% call push('scripts') %}…{% endcall %}`,
  `prepend`, and `once='key'` from pages, blocks and components. `rnx new`'s layout has
  `stack('head')` and `stack('scripts')`.
- Typed commands: `impl AppCommand` on a clap `Parser`, `App::typed_command::<T>()`; `--help`,
  argument errors with the usage. `renox::clap` is re-exported. `rnx make:command` writes one.
- `renox::prompt`: `ask`, `ask_or`, `secret`, `confirm`, `choice`; `answering(…)` for tests.
- examples: shop's `shop:make-admin` is typed and asks for a missing email; crud's form pushes
  into the head.

### M21d · Errors, logs and debugging

- A request id per request: kept from a proxy's `X-Request-Id` when it looks like one, else
  generated; in the log span, the response header, error reports and the `RequestId` extractor.
- `LOG_FORMAT=json` (one object per line) and `LOG_FILE` (append to a file).
- `App::report(|report: ErrorReport, state| async { … })` for 500s, jobs that failed for good and
  failed scheduled tasks, run in the background.
- Error pages use the app's layout: `errors/{status}.html`, then `errors/default.html` (new in
  `rnx new`), with every page global plus `status`, `reason` and `detail`. Renox's own error
  page uses the UI kit.
- `route('name', id, q=…, page=2)`: named arguments become the query string.
- Named rate limiters: `App::rate_limiter("api", |req| Limit::per_minute(60).by(…))` and
  `Routes::throttle_by("api")`; `renox::rate_limit` is public.
- `/_renox/debug` while developing (`APP_DEBUG` and `APP_ENV=local`): the last 50 requests with
  status, time, view and SQL, flagging likely N+1 queries.
- Fixed: JSON error responses dropped the headers of the error, e.g. `Retry-After` on a 429.
- examples/crud has an error page in its layout; examples/api uses a named limiter (per user,
  per IP for guests).
- **Changed:** in the error template's context, the debug request line is `request_line` (it
  was `request`, which hid the `request` global). An app's own `renox/error.html` that printed
  `{{ request }}` should print `{{ request_line }}`. An existing `errors/default.html` is now
  used for every error status.

### M21c · Scaffolding and test tools

- `Routes::resource(path, name, Resource::new().index(..).create(..).store(..).show(..).edit(..)
  .update(..).destroy(..))` with Laravel's route names.
- `rnx make:module products --resource --fields "name:string price:money …"`: model, factory,
  migration, validated form, handlers, UI-kit views and tests. New generators: `make:factory`,
  `make:seeder`, `make:test`, `make:notification`, `make:event`, `make:rule`,
  `make:middleware`.
- `rnx new` apps use the UI kit (a navigation bar, an account menu, toasts).
- `TestApp`: `travel`, `travel_back`, `at_travelled_time`; `fake_events` (`emitted`,
  `assert_emitted`, `assert_not_emitted`); `fake_notifications` (`notifications`,
  `assert_notified`, `assert_notified_to`, `assert_nothing_notified`); `assert_session_has`,
  `assert_session_missing`, `session_get`, `assert_authenticated`, `assert_guest`; `serve()` for
  browser tests.
- `TestResponse`: `assert_view`, `json_path`, `assert_json_path`, `assert_json`, and a `view`
  field. **Changed:** code building a `TestResponse` literal must add `view`.
- docs/testing.md.

### M21b · Views, components and the UI kit

- Components see the request: `old`, `error`, `errors`, `t`, `can`, `auth`, `request`, `flash`,
  `csrf_field` work inside imported macros; `once(key)`; `rnx make:component name`.
- The UI kit `renox/ui.html` after Apple's Human Interface Guidelines, with its styles and
  script (`{{ renox_ui() }}`):
  - fields (`input`, `textarea`, `select`, `checkbox` with `switch`) and `button`/`link_button`;
  - `card`, `group`, `alert`, `badge`, `form_errors`;
  - `sheet`/`open_button`, `confirm`, `menu`, `tabs`/`tab_panel`, `table`, `empty`;
  - dark mode, WCAG AA contrast, 44 pt targets, keyboard support, reduced motion.
  `rnx make:component --ui` (`ui:publish`) copies the kit into the app. See docs/ui.md.
- `Toast` (`success`, `info`, `warning`, `error`) as a response part, and `{{ toasts() }}`.
- `View::also(block)` for out-of-band fragments; `HxRetarget`, `HxReswap`, `HxPushUrl`.
- Live validation: `<form data-live-validate>` checks fields against `Valid<T>` without running
  the handler.
- Built-in `ui.*` texts in English and Indonesian.
- examples/crud uses the kit and toasts.

### M21a · Rough edges

- `User::has_role` / `User::has_permission` (the current request's roles) for policies and
  `gate_before`; `permissions::users_with_role(&db, role)`.
- `/confirm-password` returns a guarded POST/PUT/DELETE to the page its form was on;
  `TestApp::confirm_password()`.
- `Db::retrying(n, || async { … })`: retried on conflicts, borrows from the caller, may roll
  back and return a value.
- A batch's `then`/`catch`/`finally` jobs see the batch in `JobContext::batch_id` (framework
  migration `00010101000120_add_callback_of_to_jobs`: run `migrate`); `TestApp::run_all_jobs()`.
- `renox::anyhow` re-exported; `Error::permanent_message`.
- `renox::db::capture_queries(future)` returns the SQL a future ran (requests through `TestApp`
  included).
- `Morph::count_many`; `renox::random_token()`; `renox::context::Current<T>` as a handler
  argument; seeders run in the app's context (`renox::context::app()`).
- A `ValidationError` from a model hook or a handler keeps the old input on plain forms.
- **Changed:** the prelude's `Path` is `renox::Path`: a route value that doesn't parse
  (`/orders/abc` for `Path<i64>`) is a 404 page instead of a plain-text 400.
- Examples: shop's checkout uses `db.retrying`; relations counts queries with
  `capture_queries` and likes with `Morph::count_many`; jobs uses `run_all_jobs`.

### M20c · Dashboard, localized mail, HTTP client, storage

- `renox::http` (`state.http`): get/post/put/patch/delete with query, headers, bearer/basic auth,
  JSON/form/raw bodies, timeouts, retries; `Response::json/text/error_for_status`. A new default
  feature `http` (`server-events` needs it).
- `TestApp::fake_http()`: `on(pattern, FakeResponse)`, `sent()`, `assert_sent`,
  `assert_not_sent`, `assert_sent_count`; requests without a fake fail.
- Schedule pings: `ping_before`, `then_ping`, `ping_on_success`, `ping_on_failure`.
- The queue dashboard: `.module(renox::queue::Dashboard)` at `/_renox/queue`, gated by
  `view-queue-dashboard`; `Queue::stats()` (`QueueStats`) and `Queue::recent_batches`.
- Localized mail and notifications: `t()` and `app.locale` in mail views,
  `Recipient::in_locale` / `Recipient::locale()` (a `users.locale` column is read if present),
  `Notification::channels_for(to)`, `state.mail_view_in(locale, …)`, `state.lang(locale)`,
  `state.current_lang()`, `renox::i18n::{current_locale, with_locale, set_current_locale}`.
- Mail components: `renox/mail/components.html` with `button`, `panel`, `table`, `divider`.
- Storage: `list(prefix)` (`FileInfo`), `copy`, `rename`, `size`, `delete_all(prefix)`, on the
  local disk and S3.
- examples/jobs: the charge job calls the payment gateway with `state.http` (basic auth, an
  idempotency key, 402 → permanent, 5xx → retried); unset, `PAYMENT_GATEWAY_URL` is a sandbox
  route in the example; the tests fake it with `app.fake_http()`. The admin gets the queue
  dashboard. README, docs/queue.md, docs/operations.md, llms.txt and the new-app `AGENTS.md`
  cover the HTTP client, the dashboard and localized notifications.

### Guides and examples for M18–M20b

- Guides, compiled as doctests: [docs/authorization.md](docs/authorization.md) (gates,
  policies, roles and permissions, `gate_before`, token abilities, tenants, password
  confirmation, audit) and [docs/queue.md](docs/queue.md) (retries, `dispatch_in`, priority,
  unique jobs, middleware, encrypted payloads, chains, batches, testing).
- New example `examples/teams`: a multi-tenant SaaS with a default scope that fails closed,
  the current team in `renox::context`, per-team roles on the pivot, `unscoped()` for admin
  code, names unique per team, `gate_before`, and an encrypted secret behind password
  confirmation.
- examples/jobs: payment as a chain with an encrypted, rate-limited job and a `failed` hook, a
  `high` queue, unique reminders, and statements as a batch with an htmx progress bar.
  **Changed:** the receipt goes out after payment, not when the order is placed.
- examples/crud: model hooks (a slug, a cached count) and `save_changes`. examples/relations:
  likes through `Morph`, and a query-count test. examples/shop: product delete behind
  `require_password_confirmed`, checkout in `transaction_retrying`.
- Fixed: `User::get::<bool>` on an app's own BOOLEAN column returned `None` on SQLite (stored
  as 0/1), so a `gate_before` built on it never fired.
- CLAUDE.md §4.11 and CONTRIBUTING ask every milestone to keep README, the guides, the agent
  stub and the examples in step; the parity review got a status note; the gap report PDF
  covers M20b; ROADMAP M21 lists the rough edges the examples ran into.

### Docs and examples catch-up after M20b

- Docs brought in line with M18–M20a: the README's feature tour, Laravel table and status;
  docs/operations.md (what `APP_KEY` now protects, which failure rows the chaos test really
  checks, the scheduler's time zone, tables to prune, cache locks across servers);
  docs/postgresql.md (row locks); docs/stability.md (`chrono-tz`); SECURITY.md (raw SQL
  fragments); the new-app `AGENTS.md` (traps from M18–M20a); llms.txt; CLAUDE.md.
- ROADMAP: M18/M19 items ticked but not built are corrected (no `exists_many`, no savepoints,
  no breached-password check yet) and API names fixed (`EmailVerified`, `load_with_pivot`,
  `transaction_retrying`, `create_token_with`).
- `User::set_password`'s doc says it ends this session too (use `auth::change_password`).
- Examples use the new APIs:
  - shop: roles from the `Permissions` module (`require_role`), the `Audit` module on order
    status changes, `save_only`. **Changed:** the `users.role` migration is removed; recreate a
    local shop database (`migrate:fresh --seed`).
  - api: tokens with abilities and an expiry (`create_token_with`, `require_ability`), a nightly
    token prune, `cursor_paginate`, `DELETE /api/products/{id}`.
  - jobs: daily (weekdays) and weekly (`cron`) reports in `Asia/Jakarta` with `on_failure` and
    a cache lock; counts and sums in SQL. The mail template is now `mail/sales.html`.
  - relations: `count_many`, `where_has`, a `group_by` + `select_as` report, pivot columns
    (pinned, timestamps) with `attach_with` / `update_pivot` / `load_with_pivot`.
  - hello: the account page (`Auth::new().account()`).
- Tests: M20a's cache APIs are routed in `send_handlers.rs`; the read-only storage test skips
  when run as root (root ignores directory permissions).
- `docs/audit/2026-09-laravel-gap-report.pdf`: what Renox still lacks against Laravel after
  M20a (in Indonesian).

### M20b · Queue

- Priority: `queue:work --queue high,default` drains `high` first; `dispatch_on(queue, job)`.
- Unique jobs: `const UNIQUE_FOR` and `fn unique_id`; a second dispatch returns the queued id.
- Encrypted payloads: `const ENCRYPTED: bool = true`.
- Middleware: `fn middleware(&self) -> Vec<Middleware>` with `Middleware::without_overlapping(key)`
  and `Middleware::rate_limited(key, max, per)`; a held-back job keeps its attempts.
- `async fn failed(self, state, error)` on `Job`, run once a job fails for good.
- Chains (`state.queue.chain().then(a).then(b).dispatch()`) and batches
  (`state.queue.batch(name).push(job)…then/catch/finally/allow_failures().dispatch()`,
  `batch_status` with `progress()`, `cancel_batch`).
- `state.dispatch_sync(job)`; `queue:forget`, `queue:prune-failed`, `queue:prune-batches`.
- `JobContext` has `id` and `batch_id`.
- **Migration:** a new framework migration adds `chain`/`batch_id` columns and `job_batches`;
  run `migrate`.

### M20a · Scheduler, locks and cache

- `APP_TIMEZONE` takes IANA names (`Asia/Jakarta`, `Europe/Amsterdam`) with daylight saving
  time, as well as offsets and `UTC`; `renox::timezone::Zone`. The `date` filter uses it too.
- Schedules: `cron("30 9 * * 1-5")`, `weekly_on`, `monthly_on`; per task `.weekdays()`,
  `.weekends()`, `.days(&[…])`, `.between("08:00", "17:00")`, `.timezone("…")`,
  `.on_failure(|state, err| …)`, `.on_success(…)`. `schedule:run NAME` and
  `Kernel::run_scheduled`. A duplicate task name is a boot error.
- **Changed:** schedule methods return `ScheduledTask` (derefs to `Schedule`);
  `Schedule::upcoming` takes a `Zone` and returns `(name, at, zone)`.
- Cache: `add`, `pull`, `increment`/`decrement` (atomic), `prune` and `cache:prune`; the database
  store prunes expired rows hourly on its own.
- Locks: `state.cache.lock(name, ttl)` with `try_acquire`, `block(wait)` (423 on timeout),
  `is_held`, `force_release`; `LockGuard::release` or drop.

### M19b · Model features

- Model hooks: `#[model(hooks)]` and `impl renox::db::ModelHooks` with `saving` (may stop the
  save or fill fields), `saved`, `deleting` (may stop the delete) and `deleted`. Bulk queries
  don't run them.
- `renox::context::app()`: the `AppState` of the current request, job, scheduled task or
  command.
- `Model::save_only(&db, &["col"])` and `Model::save_changes(&db, &original)` (the columns that
  differ; returns whether anything was written).
- `AppState::encrypt` / `AppState::decrypt` (AES-256-GCM under `APP_KEY`).
- Pivot data: `Pivot::with_timestamps()`, `attach_with`, `update_pivot`, `toggle`,
  `load_with_pivot::<T, PivotRow>`.
- Polymorphic relations: `relations::Morph` with `of`, `load_many` and `parents`.
- **Deferred:** non-integer primary keys and an `Encrypted<T>` field type (ROADMAP explains why).

### M19a · Query builder

- `where_raw(sql, values)`, `order_by_raw(sql)`, `group_by(col)`, `having_raw(sql, values)` and
  `select_as::<T>(db, "col, COUNT(*)")` into a `FromRow` struct or tuple; `count` counts groups.
- `to_sql(dialect)` returns the SELECT and its values.
- `lock_for_update()` / `shared_lock()` (PostgreSQL); `Db::begin_immediate()` is public (SQLite).
- `where_has(children, fk)` / `where_doesnt_have` (EXISTS), `where_not_in_query`.
- `relations::count_many` and `sum_many` (withCount / withSum, 0 for rows without children).
- `simple_paginate` (`SimplePage`, `simple_pagination` macro) and `cursor_paginate`
  (`CursorPage`, keyset on id).
- `first_or_new`, `update_or_create`, `Model::refresh`.
- `Db::transaction(|tx| Box::pin(async move { … }))` and `transaction_retrying(n, …)`;
  `DbError::is_retryable`, `Error::is_retryable`.

### M18b · Accounts and security

- `Auth::account()`: `/account` with profile (a new email is verified again), password change,
  "log out other devices" and account deletion; overridable `renox/auth/account.html`.
- `Auth::password_rules(Password::min(12).mixed_case().numbers().symbols())` for the register,
  reset and account forms; `Field::password(&policy)` for any form; `Password::min(8)` stays the
  default.
- `Routes::require_password_confirmed()` and `/confirm-password` (three hours, like Laravel).
- **Behaviour change:** `auth::logout` ends this device only (a copied cookie dies too); new
  `auth::logout_other_devices` and `auth::change_password` keep this session; `User::delete_account`.
- Auth events in `renox::auth::events` (`Registered`, `LoggedIn`, `LoginFailed`, `LockedOut`,
  `LoggedOut`, `PasswordReset`, `PasswordChanged`, `EmailVerified`, `ProfileUpdated`,
  `OtherDevicesLoggedOut`, `AccountDeleted`).
- The `Audit` module (`renox::audit`): an `audit_logs` table recording every auth event,
  `audit::record(Entry::new(..).user(..).subject(..).data(..).ip(..))`, `latest`, `for_user`,
  `for_subject`, `prune`, and `rnx audit:prune --days N`.
- Users imported from Laravel log in with their bcrypt hashes and are rehashed to Argon2id
  (`auth::needs_rehash`); `User::attempt` rehashes too.
- `TestApp::session_cookie` / `use_session_cookie` to play several devices in tests.
- New apps from `rnx new` turn the account page on and link it from the layout.
- New framework migrations: `…000006_create_revoked_sessions_table` (Auth) and
  `…000600_create_audit_logs_table` (Audit).

### M18a · Tenancy, roles, gates, token abilities

- `renox::context`: values for the current request, job, task or command (e.g. the current
  team), with `set`, `get`, `remove` and `scope`.
- Default scopes: `#[model(default_scope = "team_only")]`, applied by `query()`, `find`,
  `where_eq` and the relation loaders; `Model::unscoped()`; `Query::none()` to fail closed.
- `unique`/`exists` rules take `.where_eq(col, value)`, `.where_null(col)` and
  `.where_not_null(col)`, e.g. unique per team and ignoring soft-deleted rows.
- `Routes::require_gate("admin")` (async gates too), `App::gate_before(|user, ability| …)` for
  super-admins, applied to gates, permissions and policies.
- The `Permissions` module: roles and permissions tables, `permissions::define_role`, `grant`,
  `revoke`, `delete_role`, `roles`; `user.assign_role`, `remove_role`, `sync_roles`, `roles`,
  `permissions`; `AuthUser::has_role`, `has_permission`, `role_names`;
  `Routes::require_role`, `require_permission`; `can('…')` and `auth.roles` in templates.
- API token abilities: `create_token_with(&db, name, &["orders:read"], expires)`,
  `AuthUser::token_can`, `Routes::require_ability`, `AccessToken.abilities`; `tokens:prune` and
  `auth::prune_expired_tokens` delete expired tokens.
- `Can::new` takes a `User` or an `AuthUser` (the latter applies `gate_before`).
- examples/shop guards `/admin` with `require_gate("admin")` instead of its own extractor.
- New framework migrations: `…000005_add_abilities_to_personal_access_tokens` (Auth) and
  `…000500_create_roles_and_permissions_tables` (Permissions).

### Laravel parity review

- docs/audit/2026-09-laravel-parity.md: every Laravel feature area compared with Renox, the
  verdict, and the gaps; ROADMAP plans them as M18–M21 before v1.0.
- CHEATSHEET: `set_password` ends every session, this one too (log it in again).

### Docs refresh

- The `AGENTS.md` that `rnx new` writes links the cheat-sheet and llms.txt of the Renox commit the
  app is pinned to (not `main`), points to examples/shop, htmx-recipes, relations and the guides,
  and lists the traps agents hit (`{id}` routes, `require_auth` order, handlers that aren't `Send`).
- CHEATSHEET: path parameters, route guards, `unique(…).ignore(id)`, signed URLs, queues and
  delays, plurals and language switching, auth options, seeders and factories, template globals
  and overridable pages, more test helpers, every `.env` setting.
- README: a "Coming from Laravel" table, a Documentation section, an updated status and upgrade
  path, prerequisites, and a corrected comparison with Loco.
- CLAUDE.md rewritten for accuracy (pipeline, CI jobs, env vars, conventions), without personal
  or session details. ROADMAP: finished items ticked, a "Not planned" section.
- Crate descriptions and keywords mention PostgreSQL.

### M17b · More examples, a README for each

- `examples/htmx-recipes` (modal form, inline edit, toggle, dropdown with delete, infinite
  scroll, tabs, `HxRefresh`, `HxRedirect`) and `examples/relations` (belongs to, has many,
  many to many with `sync` and `inverse`, no N+1, SQL reports).
- A README for every example.
- `AuthUser::token_id()`: the API token the request logged in with, to revoke just that one.
- `rnx key:generate` creates `.env` (from `.env.example` when there is one) instead of failing.
- examples/api: `DELETE /api/tokens/current` revokes only the token used; `DELETE /api/tokens`
  revokes all.

### M17a · examples/shop

- `examples/shop`: a whole online shop with its README (catalog, cart, checkout in one
  transaction, queued mail and notifications, a daily task, an admin with photo uploads, en/id,
  deploy files).
- **Fix:** `relations::belongs_to`, `has_many`, `Pivot::load`/`load_for` and
  `Query::first_or_create` couldn't be awaited in a routed handler ("implementation of `Send` is
  not general enough"). They now return `impl Future + Send` and hold no closure across an
  `.await`; a test routes every data API so this can't come back unnoticed.
- `first_or_create`'s closure must be `Send` (closures that capture only data are).

### Fixes

- **Stale schema in pooled connections.** After a migration added a column, a connection opened
  before it could run `SELECT *` on that table with the old column list. sqlx-sqlite then
  panicked, and the query returned no rows; PostgreSQL could refuse its cached plan. Pools now
  drop connections opened before the last migration batch run in the process. Seen as a flaky
  macOS failure of the multi-server test.

### M16b · CI and trust

- CI builds and tests an app made by `rnx new` with every `make:*` generator, on SQLite and
  PostgreSQL, and builds and starts the Docker image from `make:deploy`.
- CI also checks the MSRV (Rust 1.94, now `rust-version` in `Cargo.toml`), every feature on its
  own (`cargo hack`), S3 storage against a real S3 server (SeaweedFS), licenses and advisories
  (`cargo deny`), and reports line coverage.
- `rnx make:job` and `rnx make:command` register what they create in the module's `register`.
- `rnx make:migration` never gives two migrations the same timestamp, so migrations made in
  the same second run in the order they were made.
- `Config::from_vars` builds a config from any source of variables (tests, secrets managers).
- **Fix:** `STORAGE_DISK=s3` with an `http://` `S3_ENDPOINT` (a local MinIO or SeaweedFS) failed
  on every request.
- The macros' docs compile, and their misuse is tested to fail with a clear error.
- SECURITY.md, CONTRIBUTING.md and this changelog.

### M16a · Lighter builds (#40)

- No C crypto library in a default build: reqwest and lettre use rustls with ring.
- `fake` and `server-events` are features (on by default).
- `asset()` adds `?v=<content hash>`, and versioned assets are cached for a year.
- The Dockerfile from `make:deploy` uses cargo-chef: a code change rebuilds in seconds.
- With `CACHE_STORE=database`, rate limits and the login lock hold across servers.
- **Breaking:** `renox::fake` and `analytics::ServerEvent` need their features.

### M15b · Validation and requests (#39)

- Rules: regex, dates (`before`/`after`), digits, `in`/`not_in`, `required_if`/`required_with`,
  `same`/`different`, custom `Rule`s, per-item rules for lists.
- Several files in one field (`Vec<Upload>`), cookies (plain and encrypted), downloads and
  streamed responses.

### M15a · Query builder and relations (#38)

- `select`, `where_any`/`where_all`, `where_between`, `where_not_in`, `where_in_query`,
  `when`, aggregates, `pluck`, bulk `update`/`increment`, `insert_many`/`upsert`,
  `first_or_create`, `chunk`.
- `sql(…).fetch_as::<T>()` into a `#[derive(FromRow)]` struct, a model or a tuple.
- Explicit relations without N+1: `belongs_to`, `has_many`, `Pivot` (see `docs/relations.md`).
- **Breaking:** `Model::from_row` moved to the `FromRow` trait (only manual `impl Model`
  blocks change).

### M14c · Mail and notifications (#37)

- Mail with cc, bcc, reply-to and attachments; custom notification channels, notifications
  to people who aren't users, and queued notifications.
- **Breaking:** `Mail.to` is a `Vec<String>`; `Notification` methods take `&Recipient`.

### M14b · Extension points (#36)

- Template filters and functions, data shared with every view, `App::layer`, typed state,
  async gates, extra `User` columns and a registration hook.
- Templates are strict about undefined variables while debugging, and the debug error page
  shows the error chain and the request.

### M14a · API foundations (#35)

- `#[non_exhaustive]` on public structs and enums (see `docs/stability.md`), `Error::Status`
  and `abort`, route groups with prefixes, app commands (`App::command`, `rnx make:command`),
  and `rnx new` pins the app to a Renox commit.
- **Breaking:** struct literals and exhaustive `match`es on those types no longer compile;
  database methods return `DbError`; `renox::sqlx` moved to `renox::db::sqlx`.

### Earlier milestones

- **M13 (#31–#33):** fixes from the pre-1.0 security and resilience audit
  (`docs/audit/2026-09-pre-1.0.md`), a chaos test in CI, `docs/operations.md`.
- **M12 (#29):** every field type from the form to the database and back.
- **M11 (#23, #25, #26):** CSP with nonces, security headers, CORS per route; webhooks
  (verify, store once, process in the queue); SEO, sitemaps and analytics.
- **M10 (#21, #22, #27, #28):** examples, CHEATSHEET.md, llms.txt, compiled doc examples.
- **M9 (#19, #20):** Renox's own database layer, and PostgreSQL next to SQLite.
- **M0–M8:** the framework itself: routing, views, database and migrations, validation,
  authentication and policies, queue, scheduler and events, mail, cache, storage, i18n,
  the `rnx` CLI, test helpers, and single-binary deploys.
