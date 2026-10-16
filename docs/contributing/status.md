# Where things stand

> What each milestone and issue added, with the decisions behind it. Update this file when it changes. (Section 7 of the old CLAUDE.md.)
> Start at [CLAUDE.md](../../CLAUDE.md).

- **All milestones M0–M34 are merged to `main`** (M34: #114); Renoxium's B/C/D before
  1.0 were M23–M25. History:
  `CHANGELOG.md` (per milestone) and `ROADMAP.md` (per-milestone notes and decisions).
- After M17: a docs refresh (#45) and the Laravel parity review with M18–M21 planned (#46).
  After M20a: a docs and examples catch-up (branch `claude/laravel-project-feature-report-i6wgz0`:
  README, operations, examples moved to the M18–M20a APIs, the gap report PDF).
- **M18a** (tenancy: `renox::context`, default scopes, scoped `unique`/`exists`; `require_gate`,
  `gate_before`; the `Permissions` module; token abilities): merged (#47). Authorization is in one place: `auth::Access::check` (gate_before →
  gate → permission); `gate_before` doesn't answer role membership.
- **M18b** (account pages, `Password` policy, password confirmation, per-device logout with a
  `revoked_sessions` denylist, auth events + the `Audit` module, bcrypt import): merged (#48).
  Sessions: each login stores `_auth_session_id`; `resolve` checks the password
  fingerprint, `sessions_revoked_at` and the denylist in one query.
- **M19a** (query builder: raw fragments, group/having/select_as, locks, EXISTS, count/sum
  loaders, simple/cursor pagination, update_or_create, refresh, transaction helpers): merged
  (#49). `Query` keeps `having_binds` apart and `all_binds()` joins them
  after the WHERE binds; use it in every terminal method.
- **M19b** (model hooks, `context::app()`, `save_only`/`save_changes`, `state.encrypt`/
  `decrypt`, pivot data/timestamps/toggle, `Morph`): merged (#50). Non-integer
  keys and `Encrypted<T>` were deferred there and done in M22/M23.
- **M20a** (scheduler: cron/weekly/monthly, filters, IANA zones with DST, on_failure/on_success,
  `schedule:run`; cache add/pull/increment, locks, prune): merged (#51). A cron
  time skipped by DST runs right after the jump; intervals follow the current offset. Schedule
  methods return `ScheduledTask` (DerefMut to `Schedule`) so add-chains still compile.
- **M20b** (queue: priority, unique, encrypted, middleware, failed hook, chains, batches,
  dispatch_sync, forget/prune): merged (#52). Adds framework migration
  `00010101000110`; tests that count framework migrations must follow it.
- **M20c** (`renox::http` + fake + schedule pings, queue dashboard module, localized mail and
  notifications, mail components, storage list/copy/rename): merged (#54). The
  dashboard page was browser-checked (desktop, 390 px, dark). M20 is done.
- **M21a** (rough edges from #53/#55: `User::has_role` via context grants, `users_with_role`,
  confirm-password return for forms, `Db::retrying`, batch callbacks' `batch_id`
  (`callback_of` migration), `run_all_jobs`, `capture_queries`, `Morph::count_many`,
  `Current<T>`, seeders in context, old input on hook errors, `renox::Path` 404s): merged (#56).
- **M21b** (components that see the request via `RequestGlobal` + a thread-local of the page's
  globals, the HIG-style kit `views/ui.html` + `assets/renox-ui.{css,js}`, `toast.rs`,
  `View::also`, Hx headers, live validation via `X-Renox-Validate`, `ui:publish`,
  `make:component`): merged (#57), browser-checked on the crud example. Keep new kit
  components `rx-`-prefixed, keyboard-usable, and at WCAG AA contrast (docs/ui.md rules).
- **M21c** (`Routes::resource`, `make:module --resource` in renox-cli/src/scaffold.rs, new
  generators, `rnx new` layout on the kit, `clock.rs` + `TestApp::travel`, event and
  notification fakes in `AppState::fakes`, test assertions, `TestApp::serve`): merged (#58). Time must be read through `clock` (`db::now`, `queue::unix_now`), not
  `SystemTime::now`, so travel reaches it.
- **M21d** (request id outside `TraceLayer`, `LOG_FORMAT`/`LOG_FILE`, `App::report`, error pages
  in the app layout, `route()` query strings, named limiters, `/_renox/debug`): merged (#59).
  A context key that is `None` hides a global of the same name (`merge_maps` takes the first
  map's value, even `None`): the error page's debug key became `request_line` for that.
- **M21e** (Tailwind's standalone CLI pinned in renox-cli/src/tailwind.rs with SHA-256 sums,
  `push`/`stack` markers filled after the render, `AppCommand` + `typed_command`,
  `renox::prompt`): merged (#60). Stacks are dropped in fragments and mails (no scope there).
- **M21f** (`Validate` hooks `prepare`/`authorize`/`after` + `FormContext`, new rules, Renox's
  auth pages on the kit): merged (#61).
- **M21g** (`SESSION_DRIVER=database`: `Stored::Handle` cookies, rows keyed by sha256(id), id
  rotation at login/logout, `AppState::session_mirror` under `APP_ENV=testing`; systemd socket
  activation via `listenfd` + `deploy/<app>.socket`): merged (#62). A session given a new id must
  always be INSERTed, whatever its content.
- **Docs and examples audit** (#63): three read-only audits compared every doc, example and stub
  with M21; fixed the socket recipe's order, the shop admin delete that asked nothing, crud's
  missing flash, `make:mail`'s hint, and many pre-M21 statements.
- **M21h** (shop, teams and htmx-recipes on the kit; htmx-recipes adds `.also()`,
  `HxRetarget`/`HxReswap` and toasts): merged (#64). Three framework fixes: `add_trigger`
  `\u`-escapes non-ASCII (a raw UTF-8 `HX-Trigger` failed and the toast was silently dropped),
  toasts with `HxRefresh` wait in the session like `HxRedirect`, and error pages get `App::share`
  values (a layout using one failed under strict undefined). Toasts must survive any response
  htmx turns into a new page: they go to the session there.
- **M21i** (the examples' tests on `travel`, fakes, `assert_view`/`assert_json_path`;
  `App::report` in the jobs example; hello's typed `entries:prune`; fields with `each` +
  `one_of` + `distinct`; the stubs): merged (#66). Two framework fixes found by
  travelling: in-memory rate limits and the login lock used `Instant` (now `clock::Stamp`), and
  `TestApp`'s session helpers read the cookie on the real clock (now the travelled one, via
  `clock::with_offset_sync`). Anything timed in memory must use `clock`, not `Instant`.
  The offset is a task-local, which `tokio::spawn` doesn't inherit: wrap spawned app code in
  `clock::carry(fut)` (the worker's jobs and hooks, webhook handlers, scheduled runs), or
  travel doesn't reach it (found by the tutorial e2e, 2026-10-04).
- **After M21, small fixes:** `/favicon.ico` answers 204 unless the app ships one (#67; every
  example logged a 404 console error when run in a browser); the workspace dev profile uses
  `debug = "line-tables-only"` (#68, after two OOM kills during workspace builds).
- **M22** (model keys: `Model::Key` from the `id` field's type, `Ulid`, `Model::insert`,
  relations generic over keys, `Pivot<L, R>`, `make:model --key`, the fields example on `Uuid`):
  merged (#69). Generic code over models that needs an integer id says
  `M: Model<Key = i64>`.
- **M23** (B of Renoxium's B/C/D before 1.0: `Transaction::savepoint`, `db::Encrypted<T>`
  with the key carried by `Db`/`Transaction`/`Row`, the teams example on it): merged (#70).
- **M24** (C: `Routes::domain`/`fallback`, `route_is`/`CurrentRoute`, `Redirect::route`/
  `intended`, session `push`/`increment`, `Factory::factory()` states and sequences, plural
  ranges, `loop_controls`, `class_names`): merged (#71). A domain's host
  gets only that domain's routes (no fall-through, unlike Laravel).
- **M25** (D: `#[derive(Validate)]` + `ValidateHooks`, `App::detect_locale` for
  `Accept-Language` with `Vary`; hello and `make:module --resource` use the derive): merged
  (#72).
- **M26** (completeness before 1.0, three PRs): M26a (key bugs in `insert_many`/`upsert` and
  `unique().ignore()`, the fallback status bug, examples for M22–M25): merged (#73). M26b (docs
  brought up to date, the 380 undocumented public items documented, `missing_docs` enforced,
  `RedirectExt` sealed, `InvalidUlid` non-exhaustive): merged (#74). M26c (tests for `renox-cli` and weak core files, `App::run_args`,
  `renox::Path` answers 500 for a parameter the route lacks): merged (#75). M26 is done.
- **M27** (a data grid, asked by Renoxium before v1.0; three PRs): M27a (`renox::grid` +
  `renox/grid.html`, Cally, `grid_preferences`, `sparkline`, the grid example): merged (#76). M27b
  (`audit`/`details`, `editable` + `edit_url`, `reorder` + `RowOrder`, `merge`, several
  default sort keys): merged (#77). M27c (`exports`/`export`: CSV, Excel behind
  the `xlsx` feature, a print page): merged (#78). M27d (move columns by their
  heading, resize by its edge, `GridPrefs::widths`): merged (#79).
- **M28** (the grid next to Filament's tables, five PRs, Renoxium asked for all): M28a
  (`prefix`, `searchable` + search box, filter chips, `row_url`, `empty_state`): merged (#80). M28b (`bulk_action`/`row_action`, `Action`, `Selection`, `selected`):
  merged (#81). M28c (`Column::summary`/`Summary`, `groups`/`group_by`): merged (#82). M28d (`cards_on_mobile`, image/color columns, badges, icons,
  description, tooltip, wrap, limit, link, copyable): merged (#83). M28e
  (`related`/`count_of`/`sum_of`, `advanced_filter`, `remember`, `poll`, NULLs last): merged
  (#84). M28 is done (#80–#84).
- **Docs audit after M28** (merged, #85): every doc checked against the code,
  `docs/grid.md` added (doctest `GridGuide`). Three fixes it found: grid date-time filters take
  `APP_TIMEZONE` days (`day_start` in grid/mod.rs), `delete_account` also deletes
  `grid_preferences` rows, and `notifications:prune` / `auth::prune_read_notifications`.
- **Guides for the remaining areas**: docs/routing.md, docs/validation.md, docs/mail.md and
  docs/scheduling.md, which before lived only in CHEATSHEET.md: merged (#86). The grid example's
  `/follow-up` page (two prefixed grids, the rest of docs/grid.md's column options): merged
  (#87).
- **UI kit form fields** (Filament's forms as the yardstick; stage 1 of 3): `radio`,
  `checkbox_list`, `form_grid`/`fieldset`, `span`, `prefix`/`suffix`, `datalist`,
  `disabled`/`readonly`, `has_old()` (a checkbox or radio missing from `old()` after a
  failed submit was sent empty); the fields example on the kit. Stage 2: `revealable`/`copyable`
  inputs (auth pages reveal passwords), `toggle_buttons`, `file`, `date_picker` (Cally in a
  popover; its `change` doesn't bubble, so listen in the capture phase), `show_when`/
  `hide_when` (hidden groups are disabled fieldsets); shop checkout, uploads and fields use
  them. Stage 3: nested form names (`validation/nested.rs`: a form with a `[` in a name is
  read as a tree by its own small deserializer; errors and `old()` use dotted keys; labels
  are the last part), `KeyValues` (stored as a list of pairs: JSONB reorders object keys),
  `tags_input`, searchable/multiple `select` (a combobox over the native select, kept
  visually hidden so `required` still works), `repeater` (`{% call(row, prefix) %}`,
  renumbered by rewriting attributes), `key_value`, `wizard`; teams, fields and shop use
  them. Merged (#88, one PR for the three stages). Then the kit's `tabs` in shop's admin
  dashboard and `datalist` in teams, the two kit parts no example used yet (#90;
  `hide_when` has none, as it mirrors `show_when`). Then options from the server:
  `select(…, options_url=…, editable=true)` with `renox::select` (`SelectOption`,
  `OptionQuery`), one URL for search/lookup/add/rename; the combobox adds fetched options to
  the native select as they're chosen; shop's admin categories use it. Merged (#92).
  Then infolists (Filament's as the yardstick): `infolist`/`entry`/`repeatable` in the kit,
  the filters `money` (`APP_CURRENCY`, default `IDR` then, `USD` since #335), `since`, `words`, `markdown`
  (pulldown-cmark, raw HTML shown as text); shop's order page and fields' product page use
  them. Merged (#94). Then notifications (Filament's as the yardstick): toasts with a body,
  actions, a duration, an id and a position (`Renox.toast`), `DatabaseMessage`,
  `Auth::new().notifications()` with the kit's `notification_bell`, `renox/notifications.html`
  and `/notifications/stream` (SSE woken by `auth::notifications::Hub` in-process, polling the
  table every 15 s for other processes, five-minute streams, stopped at shutdown); shop and
  jobs use them. Merged (#96). Then dashboards (Filament's widgets as the yardstick, B before
  A): `renox::chart` (`Period`, `Trend`, `Series`), the `chart(…)` template function (SVG +
  HTML, no library), the kit's `stat`/`stats`/`dashboard`/`widget`/`period_filter`,
  `query_with`; shop's admin dashboard uses them. Merged (#97). Then actions (Filament's
  as the yardstick, the "A" after the widgets): `action_sheet` (a form in a sheet sent with
  htmx; a 422 stays in the sheet, a success closes it), `slide_over`/`width`/`icon` on
  sheets, `icon_button`, `icon`/`badge`/`key`/`disabled_reason` on buttons, keyboard
  shortcuts (`data-rx-key`), tooltips (`data-rx-tip`, one element placed by renox-ui.js:
  a CSS `::before` was dimmed by disabled buttons and widened phone tables while hidden);
  shop's admin products use them. Merged (#99).
- **M29** (examples complete; three PRs after an audit of all 13 examples): M29a (example
  bugs, tests for README claims, seeders that run twice, `.env.example` everywhere,
  fields/webhooks/uploads on the kit, current patterns): merged (#101). M29b
  (the backoffice example: invoices with a stock ledger, Midtrans/Xendit payment pages and
  webhooks, CSV import, exports from a job with the grid's filters, roles, activity log,
  settings, branded sign-in): merged (#102). M29c (relations as a public blog on Tailwind
  with Markdown/SEO/RSS/sitemap/search, an api browser client + `GET /api/me`, uploads on S3
  tested in CI's `s3` job, mail cc/bcc/reply_to/attachments in jobs): merged (#104). M29 is
  done.
- **M30** (every example on the UI kit, asked by Renoxium: the kit's navigation
  (`navbar`, `sidebar` + `rx-shell`), `page_header`, `toolbar`, `row_actions`, `list`,
  `card_grid`/`media_card`, `link_tabs`, `thumbnail`, `progress`, `menu_button`,
  `rx-page--fill`, `hide_label`; `rnx new`'s layout on `navbar`; all fourteen examples and
  their mails on the kit): merged (#105). New pages in examples and stubs
  use kit components only; `public/app.css` holds brand tokens and what is truly the app's.
- **M31** (the warm default theme and the `--rx-type-*` type scale, chosen by Renoxium after
  screenshot comparisons; Inter and Poppins bundled in assets/fonts and served from
  `/_renox/fonts`; `data-rx-theme="classic"` for the old look): merged (#107).
- **M32** (English only, asked by Renoxium: no Indonesian anywhere in the repo, the built-in
  `id` locale dropped, Spanish as the examples' second language; framework tests use
  `crates/renox/tests/lang/es.json`): merged (#108).
- **Docs refresh after M32** (asked by Renoxium: the README, the guides and the Laravel parity
  review, which was still the M17 snapshot): the review rewritten for today
  (`docs/audit/2026-10-laravel-parity.md`), the Indonesian PDF replaced by an English one,
  every guide, CHEATSHEET, llms.txt and example README checked against the code: merged
  (#110).
- **M33** (the parity review's small adds, chosen by Renoxium before v1.0: 28
  validation rules with `Dimensions`, `Found<M>` route model binding, `Routes::view`/
  `redirect`, named disks, `Routes::etag`, `App::xsrf_cookie`, `TRUSTED_HOSTS`): merged
  (#112). Layers like `.etag()` cover only the routes added before them.
- **M34** (the rest of the small adds: `current_password`, `Password::uncompromised` (HIBP),
  `Validator::finish_for`, session `keep`/`flash_now`, named error bags, `App::mailer` +
  `MAIL_FAILOVER`, `has_many_through`): merged (#114). Async rule checks live in
  `Validator::checks` and run in `finish_with` after the database ones.
- **v1.0 started** (Renoxium, 2026-10-03, after M34). V1a (release readiness: lockstep `=`
  versions, path-only dev-deps, docs.rs metadata, `rnx new` from crates.io, the semver CI
  job, RELEASING.md): merged (#115). V1b (the API audit's must-fix items: settings as enums with
  `setting_enum!` in lib.rs, `Duration` lifetimes, `Notification::channels(to)` and
  `to_database(to, state) -> Result`, `notify(impl Into<Recipient>)`, `Job::failed(ctx, …)`,
  no `Locale`, private `Upload` fields, `Migration` builders): merged (#117). V1c
  (the should-fix items; Renoxium chose all but `gate` → `authorize_gate`: the state last in
  every closure, seeders get `AppState`, `disk_named`, builder-only `Factory`, `DateTime`
  timestamps, opaque `Zone`, sealed `Viewer`/`Executor`/`ForeignKey`, `DownOptions`,
  `retry`/`retry_all`, secrets hidden from `Debug`): merged (#118). V1d (the docs site `site/`, docs/tutorial.md,
  docs/laravel.md; Renoxium hosts it on their own server): merged (#120). V1e
  (`rnx new --starter`, the 1.x promise in docs/stability.md): merged (#121). Then the release
  candidate `1.0.0-rc.1`: merged (#122), published to crates.io on 2026-10-03 with the
  Renoxium's `cargo login` token after Renoxium confirmed (tag `v1.0.0-rc.1`; docs.rs built,
  `cargo install renox-cli --version 1.0.0-rc.1` + `rnx new` checked). Then the README's
  start and coding-agent sections: merged (#123). GitHub Pages was tried and dropped
  (Renoxium's account serves project sites on a personal domain): the docs site runs on the
  Renoxium's own server, built with Renox: https://docs.renox.rs (renox.renoxium.com from
  2026-10-04, the project's own domain since 2026-10-07, #331). The project's domain is
  renox.rs: the landing page at https://www.renox.rs (the bare domain too, its own app, planned), the docs
  at docs.renox.rs, the bikeshop demo at https://bikeshop.renox.rs (#141;
  the server pulls each build of `.github/workflows/release-site.yml`).
  Then #124 (new apps failed `cargo fmt --check`): `rnx` formats what it writes, merged
  (#125); GitHub Pages switched off (2026-10-04). Release candidate `1.0.0-rc.2` with that
  fix: published 2026-10-04 (#126, tag `v1.0.0-rc.2`). Then #127 → #129 (plural tables),
  #128 → #130 (another session), #131 (the tutorial followed in CI; `clock::carry`), and
  `1.0.0-rc.3` with them: published 2026-10-04 (#132). Then the work moved to GitHub
  issues and the project board (#134); fixes #133/#144/#160/#163/#178 and the plugin extension
  points #167/#168, released as `1.0.0-rc.4` (tag `v1.0.0-rc.4`). Then #219, #226, the
  plugins renox-oauth/-admin/-billing (#147/#148/#155) and roles per branch (#244), with the
  workspace at `1.0.0-rc.5`, published (tag `v1.0.0-rc.5`). Then the coverage epic (#246:
  #277, #280, #291, #293, #295) and its fixes, with the workspace set to `1.0.0-rc.6`
  (published). Then the bike shop (#231), its design system and one example (#343, #351), and
  **1.0.0: released 2026-10-08** (#356, tag `v1.0.0`, the ten crates on crates.io including
  the new renox-blocks, a GitHub release; #138). After it, #139: no `--version` anywhere, the
  semver job against crates.io. Ask Renoxium before every `cargo publish`.
- **1.1, wave 1** (the gaps examples/bikeshop worked around, all additive: `cargo semver-checks`
  against 1.0.0 passes): `Display` for `Error` (the message without the chain; no
  `std::error::Error`, which would clash with the blanket `From`) #301, `Registry::seeder` #302,
  `db::Conn` public with `Executor::into_conn` + `Conn::reborrow` #303, `signed::verify` public
  and `Option<ValidSignature>`, which (like `ValidSignature`) now reads `OriginalUri` so a
  signed route inside a `Routes::group` works #307, `ViewContext::session` #309,
  `permissions::users_with_permission(_in)` #310, and, while debugging, a warning (and the
  cause on a failed render) when a view value is named like a macro the template imports #312
  (`view.rs::hidden_by_imports`). Tests in `crates/renox/tests/it/app_gaps.rs` (#310 in
  scoped_roles.rs).
- **1.1, wave 2** (grid; additive): `Column::computed(sql)` (an expression with `{T}` for the
  table: sorts, filters, searches, summaries and groups work on it and on the related columns,
  `summarize` reads each column's SQL through `sql_of`) and `Query::group_by_raw` #313; a
  `select` column with whole-number options filters with `CAST(col AS TEXT) IN (…)` so integer
  columns work on PostgreSQL #319. Tests in grid.rs
  `computed_columns_are_summed_grouped_and_integers_filtered_by_select`. Then the rest of #319:
  `grid::Workbook` (several sheets in one Excel file, `xlsx` feature) and `chart("heatmap", …)`.
- **1.1, plugins and auth** (additive): renox-billing named guards
  (`require_subscription_named`/`require_plan_named`), `Customer::pause`/`unpause`,
  `SubscriptionStatus::Paused` and webhook state hooks #315; renox-admin actions with input
  (`ActionInput`), the after-save hook (`SaveContext`), relation managers, `Admin::slot` and
  `renox.admin.*` texts #314; `auth::DeviceToken` + the `Device` extractor (framework migration
  `00010101000007`), public `i18n::RequestLocale` and `auth::normalize_email` #316.
- **1.1, the developer-experience epic #385** (PRs #492 and #502; every sub-issue closed, the
  epic itself and #354 still open): `db:check` + `TestApp::assert_models_match_schema` (models
  registered with `App::model`) #370; `db:diff` / `rnx make:migration --auto` from the models'
  schema attributes (`index`, `unique`, `default`, `references`) #371; the `rx-*` tag stages
  #372–#376 (forms, navigation, overlays, dashboards, the grid, plugin tags through
  `view::add_template` + `Registry::component`); component events and `@event` #373;
  `view:check` + `TestApp::assert_views_compile` #377; `view:data` (editor custom data) #378;
  `rnx doctor` and the fast linker `rnx new` writes #379; typed columns (`Product::PRICE.lt(…)`,
  `Query::where_`, `#[model(no_typed_columns)]`) #380; `#[model(form)]` #381;
  `make:module --resource` migrates, prints the URL, `--open` #382; `#[derive(ViewData)]` +
  `View::typed` #383; live components (`LiveComponent`, `#[renox::live_component]`,
  `<live-NAME>`, `rx-click`/`rx-submit`/`rx-model`, `TestApp::live`; docs/live.md) #386. #384:
  `rx-*` tags are the default in `rnx new`, the starter, both examples and the guides (#493–#500);
  the kit's macros stay supported underneath. The `browser (bikeshop)` job failed on main
  after #492 ("Esc closes it" on the About sheet, phone): the Spanish staff test logged in by
  clicking the form, #354's trigger; it logs in with `logInWithFetch(…, { keepCookies: true })`
  now. Then the workspace was set to `1.1.0-rc.1` and published on 2026-10-11 (all ten
  crates, tag `v1.1.0-rc.1`). #505: the guides, the tutorial, hello and bikeshop teach only
  the 1.1 way (tags, typed columns, ViewData; the old forms stay in the API but are not shown),
  CLAUDE.md became a short guide with the rest in docs/contributing/, and the workspace
  moved to `1.1.0` (the landing page, the plugin guides, the tutorial, llms.txt and the README
  name 1.1.0; publishing it is Renoxium's `cargo publish`, in RELEASING.md's order, then the tag
  `v1.1.0`).
- **1.1.1** (#5, 2026-10-14; a patch with no API change): the first release from
  github.com/renox-rs/renox. The crates are owned on crates.io by `renox-rs`, and pushing the
  tag publishes them through `.github/workflows/release.yml` with Trusted Publishing (#3,
  RELEASING.md); it also carries the landing page's canonical address www.renox.rs, the
  crates' homepage (#1), and renox-editors on ammonia 4.2.3 for RUSTSEC-2021-0157/0158, with
  cssparser and dtoa-short (MPL-2.0) as cargo-deny license exceptions (#7; a Dependabot bump to
  4.2.1, inside the advisories, was undone by #15). The workspace, its nine `=` pins, the landing page's badge and
  terminal and the README name 1.1.1.
- **#24, the dashboard standard** (Renoxium, 2026-10-15): `<rx-shell fill>` (`rx-shell--fill`,
  a `flag` on the `rx-shell` contract) is the standard mechanism of a Renox dashboard
  application. From 64rem wide and 36rem tall (screen only) the shell is `100dvh` with
  `overflow: hidden`, `rx-shell__content` becomes a flex column without `max-width` that
  scrolls on its own, and a `rx-fill` element or a `.rx-grid` written straight in it takes the
  height left (`flex: 1 1 0`, a `--rx-fill-floor` of 16rem); `rx-pane` (a surface that scrolls
  on its own) and `rx-panes` (panes side by side, `--rx-panes`) are new. Below that the page
  scrolls as a plain `rx-shell` and `rx-fill` keeps `--rx-fill-min` (`max(24rem, 60svh)`).
  renox-ui.js passes a scroll inside the shell on to the window (htmx 2.0.11's `revealed`
  listens to the window only) and keeps the content's and panes' places in htmx's history
  (`data-rx-scroll`, set on `htmx:beforeHistorySave`). `rnx new`'s signed-in layout,
  renox-admin's layout and the bike shop's staff and admin layouts use it, with the tag in
  `<body class="rx-page">` (the docs taught `rx-shell` on `<body>` before). Rule text in
  llms.txt "Start here" and AGENTS.md.stub; tests/browser/ui-shell.test.mjs.
- **#55, status stepper and week calendar** (part 3 of #32; parts #53 and #54 are separate PRs):
  renox-blocks gets `status_stepper` (steps `{key, label, tone?, terminal?}`, `aria-current="step"`,
  state said in words, horizontal or vertical, CSS only) and `week_calendar` (resources as rows,
  `days` x `from`..`to` by `step` minutes as columns; `week.rs` works out each event's cells and
  lane, the free slots and their RFC 3339 start and end through the `date` filter, so times follow
  `APP_TIMEZONE`; a free slot is a `role=gridcell` with `hx-post`, static `hx-vals` and
  `data-resource/-start/-end`; `assets/parts/week.js` is the grid's roving-tabindex keyboard,
  right-to-left aware). `blocks.week.*` and `blocks.stepper.*` texts; shown on the bike shop's
  /about/blocks; tests in crates/renox-blocks/tests/blocks.rs and
  tests/browser/blocks.test.mjs.
- **Earlier plan for v1.0:** v1.0 (API audit, `cargo-semver-checks`, real
  crates.io releases (Renoxium runs `cargo login`), a docs site with a tutorial and a
  Laravel guide, a starter kit). 
- **#147, `renox-oauth`** (social login: Google, GitHub, the `Provider` trait, PKCE + a
  single-use `state` in the session, linking only by a verified address and never to an
  account whose own address is unverified, unlink never removes the last way in): branch
  `ccr-f926b004-17j95n`. It added to renox-core `auth::sign_in`, `register_verified`,
  `registration_open`, `confirm_identity` (auth/external.rs; the `Auth` module's settings now
  ride in `AppState::auth`), `User::has_password` (users made by a social login have an empty
  `password`: the account page lets them set one and asks them for a recent confirmation
  instead of a password), and the login/register/confirm pages'
  `{% include "renox/auth/login_options.html" ignore missing %}`.
- **#51, company sign-in in `renox-oauth`**: `Microsoft` (OpenID Connect against
  `login.microsoftonline.com/{tenant}/v2.0`; the ID token from the token endpoint, unsigned-checked
  per OIDC Core §3.1.3.7, with `iss`/`aud`/`exp`/`tid` checked; a domain tenant is resolved to its
  id from the discovery document and cached; the consumer tenant is always refused; the email is
  verified only for a specific tenant), `Google::hosted_domain` (sends `hd`, checks the userinfo
  `hd`), `OAuth::allow_domains` (verified email domain, any provider), `Refused` (an error type
  `Provider::profile` returns to turn an account away), `Provider::with_config` (settings read
  from the configuration, here `MICROSOFT_TENANT`) and the `OAuthRefused` event (`oauth.refused`).
- **#148, `renox-admin`** (the admin panel: `Admin::new().authorize(…).resource(…)`,
  `impl AdminResource` with grid columns, form fields, filters, actions and the model's
  `Policy`; lists, forms, view pages, the trash, exports; the admin example): branch
  `ccr-f926b004-17j95n`. It added to renox-core `Column::label`/`options` and
  `renox::currency_decimals`, and fixed `GridRequest`'s path inside a `Routes::group`
  (axum's nesting strips the prefix from `uri.path()`; it reads `OriginalUri` now, so a
  grid's form, links and exports keep `/admin/products`). Other code that reads
  `uri().path()` in a nested route sees the shorter path too (`FormContext::path`; signed
  URLs checked inside a group are worth a test).
  Template context keys named like a request global (`can`, `auth`, `errors`…) are hidden
  by the global (`merge_maps`, the last map wins): the panel passes `allowed`, not `can`.
- **#155, `renox-billing`** (subscriptions: `Billing::new().plan(…).stripe().xendit()`,
  `Billing::of(&state, &user)` with `subscribed`/`checkout`/`swap`/`cancel`/`resume`, the
  `Gateway` trait, Stripe and Xendit, webhooks through `renox::webhook`, guards, pages;
  the billing example): branch `ccr-f926b004-17j95n`. It added `Registry::provide` to renox-core
  (a module's settings for code without a request: `Webhook::verify`/`handle` get only the
  state). `Webhook::event_id` gets no state either: a plugin whose webhooks depend on its
  settings computes the id in a route layer and passes it in a header (billing's webhook.rs).
  `FakeHttp` answers in turn but repeats the last one: queue every answer a test needs before
  the first call, or a single answer keeps being given.
- **#244, roles per branch** (a role given in one record and for a period: `Scope`,
  `assign_role_in(..).from(..).until(..)`, `remove_role_in`, `sync_roles_in`, `assignments`,
  `users_with_role_in`, `permissions::set_scope` in `renox::context`, `has_role_in`/
  `has_permission_in`, `scopes_with` + `Scopes::apply` for default scopes,
  `permissions:prune`, `route:list` marks role/permission guards with `*`): branch
  `ccr-f926b004-17j95n`. `Grants` (permissions.rs) holds every assignment not ended at load
  time and filters by the active scope and `db::now()` at each check; `AuthUser::role_names`
  is worked out the first time it's asked. The global `assign_role`/`remove_role`/`sync_roles`
  and `users_with_role` only touch global rows.
- **#246, test coverage** (epic with sub-issues #247–#270, one PR): the coverage job measures
  every crate on SQLite and PostgreSQL; tests for every item the issues list (macros' compile
  errors, `renox::testing`'s messages, validation, views, request helpers, auth, the data
  layer, grid and charts, background work, commands, the CLI, the plugins); browser tests in
  tests/browser (CI `browser`) and process tests in tests/process. Fixes it found: date limits
  given as text, a failing `App::share` showing a bare 500, REAL/SMALLINT extra columns on
  PostgreSQL, live validation of lists, the grid's column menu (two JS `save`s), examples/hello
  under `CSP=strict`. Small refactors for testability in renox-cli (`asset_for`,
  `cache_dir_with`, `check_sum`, `app_root_in`, `key_generate_in`). Merged (#277). #280 (another
  session) tested what #277 left in #250–#253/#256 (fixes #278, #279). The rest of every
  sub-issue in a third PR: PostgreSQL races/retries/connections/migrator, grid rules and
  exports read back (zip), charts, the commands' output (tests/process), the CLI, the plugins,
  browser tests for every remaining kit/grid/editor item and each example's main flow, process
  tests on PostgreSQL; the coverage job fails under 90 % of lines. Fixes it found: #281
  (PostgreSQL TIME extra columns), #282 (charts rounded fractions), #283 (a required combobox
  focused its hidden select), #284 (`rx-shell` on phones), #285–#287 (grid date range, two
  grids on a page, default parameters in the URL), #288 (`rnx serve` left the app running on a
  signal). What stays untested is listed with its reason in that PR.
- **#231, examples/bikeshop, the flagship example** (stories #232–#243, #245; one branch
  `feat/issue-231-bikeshop`, one PR, a commit per story): a bike shop with three stores on
  Pagila's shape and volume, every page explained ("About this page", `/about/pages`), RBAC +
  ABAC on #244's roles per store, the plugins, a JSON API, dashboards and reports; the UI kit
  plus the example's own "blocks" (Renoxium's decision on #231), Motion vendored. #243 wired
  it into CI (`-p bikeshop` in the PostgreSQL job, tests/browser/run.sh, tests/process) and
  added `tests/queries.rs` (the main pages on the large seed: same query count as the small
  one). What Renox lacked became issues #299–#319 (listed in its README), worked around in the
  example. Open in #243: the Spanish pass, the explanations' review, Renoxium's sign-off.
- **#343, the bike shop's design system in Renox** (stories #344–#349 and #351, one branch
  `feat/issue-343-design-system`, one PR; Renoxium chose where each part goes):
  - #344: the kit's default theme is the bike shop's "editorial" look. `data-rx-theme="warm"`
    is the look before 1.1, `classic` the first one. New tokens `--rx-accent-2`, `--rx-tint`,
    `--rx-accent-soft`, `--rx-ink`, `--rx-type-hero`, `--rx-type-section`. Pill buttons,
    except in `rx-shell`.
  - #345: `date_picker(disabled_dates=…, closed_weekdays=…)`.
  - #346: `navbar(tabs=…)` (a phone tab bar), `nav_search`, and the public `icon()` macro
    (Lucide, crates/renox-core/src/icons.rs; `sidebar_link`, `stat` and `empty` take `icon=`).
  - #347: crates/renox-blocks.
  - #348: the starter's page patterns (stubs/starter: patterns.html + patterns.css, owned by
    the app).
  - #349: the kit's rarely used parts load on demand.
  - The bike shop: "About this page" is docked beside every page from 1200 px, with code
    samples cut from the source at build time (`[explain:name]` markers, build.rs →
    `code::REGIONS`, checked by tests/about.rs). A visual pass added icons, Unsplash photos
    (CREDITS.md) and initials avatars. The grid, charts and the kit's basics stay in core
    (Renoxium's decision).
- **#351, two examples** (Renoxium's choice, after the design-system work #343): only
  examples/bikeshop and examples/hello remain. Before the other fifteen went, what they showed
  and the bike shop lacked moved in: each store's page on its own host (home/stores.rs,
  `Routes::domain`), `/about/fields` (the types reference docs/types.md points at; files public
  and private; tests/fields.rs on SQLite, PostgreSQL and S3) and `/about/htmx` (the htmx
  recipes). Their browser tests moved to bikeshop-*.test.mjs, and the grid example's pages to the
  fixture's `/grid`. CI's PostgreSQL job tests bikeshop, its s3 job bikeshop's
  tests/fields.rs. The entries above name removed examples: they are history. In headless
  Chrome the tab that sent the bike shop's login form gets no key presses afterwards; tests
  that type after logging in log in from a tab of their own.
- **#372, components as tags** (epic #385; tasks #443–#458): `components/` compiles `<rx-…>`,
  `<app-…>` and `rx-if`/`rx-else`/`rx-for` to MiniJinja at load (the loader hook, a static contract
  table, `make:component` writing `<rx-props>`); bikeshop's supplier and field pages use it; guide:
  docs/ui.md "Components as tags". Plugin templates are compiled too since e94a7e6e
  (`view::add_template`).
- **#538, learn.renox.rs** (epic; Renoxium's decisions of 2026-10-11): a course that builds one
  app (a product catalog with stock) stage by stage, as its own app `learn/` next to site/ and
  www/ (not docs/: it needs images, videos and an editor; not examples/, which stays at two).
  The course app will live in `learn/course/` (its own cargo workspace, one app per stage), the
  editor is CodeMirror 6 with completions from Renox's API (extracted with `syn`) and
  `view:data`, challenges are compiled and tested in a container sandbox, videos are recorded
  by script and served from the server's disk. #523 is the skeleton (port 3100).
- **#46, rate limits for queued channel and mail sends**: `App::channel_rate_limit` /
  `Registry::channel_rate_limit` and `App::mailer_rate_limit` (`"default"` = the default mailer).
  `Job::middleware` has no state, so the limit is copied into the payload of `SendToChannel`,
  `SendMail` and `SendMailVia` when they are queued (`queue::Limit`, optional fields; `SendMail`
  flattens its `Mail`, so old payloads still decode) and the job returns
  `Middleware::rate_limited("channel:<name>" | "mailer:<name>", …)`. Unknown names fail at boot.
  Tests: tests/it/rate_limits.rs.
- **#49, Arabic-aware search**: `normalize = "arabic"` on `#[model(search)]`, `renox::text`
  (`normalize_arabic`, `transliterate`, `match_key`) and `db::search::arabic_sql`. One rule
  table (`text.rs`) feeds the Rust function and both databases' SQL. A normalising model's
  SQLite FTS5 table has its own content (no `content=`), so `rebuild` refills it with
  `DELETE` + `INSERT … SELECT`. Tests in tests/it/search.rs (the PostgreSQL run needs
  TEST_DATABASE_URL, as for the rest of that file). See ROADMAP "Decisions".
- **#29, typed settings in the database** (`renox::settings`, docs/settings.md): the opt-in
  `Settings` module (migration `00010101000400_create_settings_table`, commands `settings:get|set|
  list`), `SettingsGroup` (`KEY`, `ENCRYPTED`) for a plain struct with defaults and `Validate`
  rules, `App::settings::<T>()` / `settings_in_views` / `settings_from(path)`,
  `AppState::settings` / `save_settings` (+ `_in` for a scope, `settings_info`), the `Setting<T>`
  extractor, the event `SettingsChanged` and an `audit_logs` entry when `Audit` is on,
  `TestApp::with_settings`, and `Admin::settings` / `AdminSettings` in renox-admin. Decisions: the
  group trait is `SettingsGroup` and the module `Settings` (the issue used one name for both); the
  table is keyed `(key, scope)` with `''` as the default scope so a scope needs no later migration;
  an encrypted group is sealed the way `db::Encrypted` is; the per-process cache re-checks a
  version row in `cache` (`renox:settings:<key>`) at most once a second per group, so one query per
  group until a write and about a second for other servers to notice.
- **#50, imports beyond CSV** (Excel files, dry run, undo; one branch): `Import::xlsx` (feature
  `xlsx`, now also `calamine` 0.36, MIT, pure Rust; its `zip` and `quick-xml` were already in the
  tree, new are `atoi_simd`, `fast-float2`, `codepage`, `debug_unsafe`), `Import::from_upload`
  (CSV or Excel by content: a zip is a workbook), `.sheet`, `template_xlsx` + `TemplateColumn`;
  `.dry_run()` / `.dry_run_if(bool)` (the row closures run in the transaction, which is rolled
  back); `.undoable(window)` with `ImportTx` (`insert` / `update` / `save` record the table, the
  key, a typed JSON before-image and the row's `updated_at` as text in `import_changes`),
  `renox::import::undo` / `Undo::can_undo`, `prune` and `import:prune`; `ImportReport::{batch_id,
  dry_run, error_csv, error_xlsx}` and `ErrorFile`; `<rx-import-action check>`. One framework
  migration, `00010101000230_create_import_batches_table` (twenty-one in all; database.rs counts it).
  The bike shop's price list uses it. The row closure's first argument is now `&mut ImportTx`
  (derefs to the transaction and is an `Executor`, so closures written for `&mut Transaction`
  compile unchanged). Not done: mapping columns in the browser (a follow-up); `ImportTx` has no
  `delete`.
- **#42, passwordless login**: `Auth::magic_link()` / `same_browser()` / `magic_link_ttl()` /
  `login_codes(LoginCodes)` / `register_on_first_use()`; routes `login.link.request|send|show|consume`
  and `login.code.request|send|verify|resend`; `auth/passwordless.rs` (handlers, the encrypted
  `renox.send-login-secret` job, `LoginCodes`, `LoginCodeDelivery`), `auth/one_time.rs`
  (`OneTimeCode`, `CodeCheck`, the `one_time_codes` table, framework migration `…000008`);
  `LoggedIn::via` (`password`, `magic_link`, `code`, `external`; kept in the pending second-step
  login); `TestApp::last_login_link/last_login_code[_on]` read a debug-mode log of what was
  delivered; the login page lists the options (`magic_link`/`login_codes` in its context, no
  `login_options.html` so it doesn't clash with renox-oauth's). Tests: tests/it/passwordless.rs,
  tests/browser/passwordless.test.mjs; guide: docs/authorization.md.
- **Still open** (ROADMAP `- [ ]`): none of the plugins; `renox-2fa` (#146),
  `renox-oauth` (#147), `renox-admin` (#148) and `renox-billing` (#155) are done. A Laravel gap review after M25 (in the
  conversation that planned M26) ranked them: release and docs first, then 2FA and social
  login, then small adds (validation rules like `json`/`gt`/`decimal`/`dimensions`, several
  storage disks, route model binding), then admin, search, realtime (SSE) and billing.
- As of M32: ~65k lines of Rust in `crates/` (stubs excluded) and ~16k in the fourteen
  examples, ~770 `#[test]`/`#[renox::test]`/`#[tokio::test]` functions in `crates/` and
  `examples/` (plus doctests), and 44 direct dependencies in renox-core (6 optional;
  pulldown-cmark came with the `markdown` filter, #94).
  Keep dependencies lean and remove unused ones.
