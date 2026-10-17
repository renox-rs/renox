# Workspace layout

> Every crate, folder and file that matters, and what it is for. (Section 2 of the old CLAUDE.md; the numbers are kept so references like "§2" still make sense.)
> Start at [CLAUDE.md](../../CLAUDE.md).

```
Cargo.toml                 workspace: members crates/*, examples/*, site, www, learn, tests/browser/fixture, tests/chaos
                           (learn/course excluded: its own workspace); shared package
                           metadata; rust-version (MSRV); argon2 opt-level for dev builds
README.md                  front page (compiled as doctests: `ReadMe`)
ROADMAP.md, CHANGELOG.md   plan + decisions; changes per milestone
CONTRIBUTING.md            checks every change needs; SECURITY.md: reporting vulnerabilities;
                           RELEASING.md: publishing to crates.io (Renoxium runs it)
CHEATSHEET.md              one-page patterns for app authors/agents (compiled: `CheatSheet`)
llms.txt                   map for agents: which example/guide file shows what
deny.toml                  cargo-deny: licenses, advisories, banned crates, sources
crates/renox/              facade crate apps depend on: re-exports renox-core, the macros, prelude
  src/lib.rs               `pub use renox_core::*`, macros (DbEnum, FromRow, Model, Validate, embedded!,
                           migrations!, #[renox::test]), prelude, and cfg(doctest) holders:
                           ReadMe, CheatSheet, TypesGuide, RelationsGuide, AuthorizationGuide,
                           QueueGuide, UiGuide, GridGuide, ValidationGuide, RoutingGuide,
                           MailGuide, SchedulingGuide, TestingGuide, OperationsGuide,
                           TutorialGuide, LaravelGuide, LiveGuide, StateMachinesGuide,
                           MacroCompileErrors
  tests/it/                ONE integration-test binary (main.rs + a module per area); add new areas
                           as `mod x;` in main.rs. Notable modules: send_handlers.rs (every data
                           API in a routed handler), web_security.rs, data_resilience.rs,
                           background_resilience.rs, direct.rs (APIs otherwise tested only
                           indirectly), database.rs, postgres.rs (`postgres` feature), s3.rs
                           (`s3` feature), extension_points.rs, api_foundations.rs, data_layer.rs,
                           commands.rs (the binary's built-in commands via `App::run_args`)
  tests/migrations/, migrations_plain/, migrations_types/, views/   fixtures (not under it/)
crates/renox-core/         ALL runtime code (see §3 for why one crate)
  src/app.rs               App builder, boot(), Kernel, router assembly, app-binary commands;
                           serve takes systemd's socket (listenfd, LISTEN_FDS) before binding
  src/config.rs            Config from env/.env (see §5)
  src/state.rs             AppState (Clone): config, routes, views, db, mailer, queue, cache,
                           storage, translator, listeners, key, gates/async gates, shares,
                           channels, provided values, throttle, security, webhooks, live
  src/module.rs            Module trait: name, routes, routes_with, migrations, register
  src/contributions.rs     Contributions: typed, ordered lists modules add with `Registry::contribute` (#31)
  src/registry.rs          Registry: jobs, listeners, schedule, commands, channels, shares, templates,
                           assets (`Registry::asset`: files served before the session),
                           provide (a module's values for `state.provided`, under the app's),
                           seeders (`App::seeder` lands here too: the app's run first)
  src/routing.rs           Routes builder (get/post/…/name/group/require_auth/guest_only/
                           require_verified/throttle/cors/route_layer/merge/domain/fallback),
                           RouteTable + URLs (name_of for route_is), CurrentRoute
  src/domain.rs            Routes::domain: DomainPattern, host dispatch between routers,
                           DomainParams / MatchedDomain
  src/redirect.rs          RedirectExt: Redirect::route / Redirect::intended
  src/session.rs           session + middleware: an encrypted cookie holding the whole session
                           (cookie driver) or its id (`Stored::Handle`, database driver:
                           `sessions` table keyed by sha256(id), rotation on login/logout,
                           push/increment helpers,
                           a test mirror in `AppState::session_mirror`)
  src/csrf.rs              CSRF middleware; the XSRF-TOKEN cookie (App::xsrf_cookie) and
                           X-XSRF-TOKEN header
  src/view.rs              MiniJinja env, View response (fragment/also), render middleware, globals
                           (request.route, route_is, loop controls), RequestGlobal (request globals
                           inside imported macros), BUILTIN views
  src/toast.rs             Toast (body, ToastAction, duration, id) response part, the toast
                           region markup (positions), safe_url for links built from data
  src/clock.rs             the current time with a test offset (TestApp::travel); Stamp for
                           in-memory windows (rate limits, login lock), never Instant
  views/ui.html            the UI kit (renox/ui.html): fields, buttons, sheets, menus, tables,
                           infolists, dashboards, and the page's frame (navbar, sidebar +
                           rx-shell, page_header, toolbar, list, card_grid…); assets/renox-ui.css|js
                           (+ renox-ui-{chart,wizard,repeater,tags}.js: modules renox-ui.js loads
                           when it finds their markup; assets.rs fills their hashed URLs in, #349)
  views/grid.html          the data grid macro (renox/grid.html), grid_print.html (its print
                           export); assets/renox-grid.css|js, and
                           assets/cally.js (Cally 0.9.2, MIT: the date range calendar)
  src/components/          the tag compiler: `<rx-…>`/`<app-…>` and `rx-if`/`rx-else`/`rx-for` to MiniJinja
                           when a template loads (scan.rs, tree.rs, attrs.rs, contracts.rs: the static
                           component table, emit.rs, special.rs: rx-page/push/form/table, app.rs, suggest.rs);
                           plugins compile theirs through `view::add_template` and register
                           tags with `Registry::component`
  src/live_component/      live components (#386): mod.rs (LiveComponent, registration),
                           context.rs (LiveContext), snapshot.rs (signed state, LIVE_SNAPSHOT_MAX_SIZE),
                           route.rs (POST /_renox/live/{component}/{action}); views/live.html,
                           assets/renox-live.js + idiomorph-0.7.3.min.js (0BSD, idiomorph-LICENSE)
  src/view_stack.rs        push/prepend/stack: markers filled in after the page renders (Scope)
  src/icons.rs             the kit's icons: Lucide paths (lucide-static 1.53.0, ISC; assets/NOTICE,
                           assets/lucide-LICENSE) drawn by `renox_icon`, behind ui.html's `icon(…)`
  src/view_filters.rs      built-in template filters `number`, `money`, `date`, `since`, `words`,
                           `markdown`; pub format_number, format_money (whole units;
                           the `money` filter takes the smallest unit, like the grid)
  src/htmx.rs              Htmx extractor, HxRedirect/HxRefresh/HxTrigger/HxRetarget/HxReswap/
                           HxPushUrl, Back; add_trigger (JSON HX-Trigger, non-ASCII \u-escaped)
  src/assets.rs            embedded htmx/Alpine/renox.js with hashed URLs; renox.js source lives here
  src/error.rs             Error enum, IntoResponse, error pages (the app's errors/{status}.html,
                           errors/default.html with the page globals, else renox/error.html),
                           Debug for main(), panic_message
  src/crypto.rs            APP_KEY parsing/generation, seal/open, encrypted files (RNXE), random tokens, constant_time_eq
  src/signed.rs            signed URLs (HMAC-SHA256) + ValidSignature extractor
  src/db/                  conn.rs (Db/Transaction/Row/sql(), Executor, SchemaEpoch, savepoints,
                           the key for Encrypted columns), key.rs (ModelKey, Ulid), encrypted.rs
                           (Encrypted<T>, Unsealed), mod.rs
                           (connect, TEST_DATABASE_URL), model.rs, query.rs, from_row.rs,
                           relations.rs (belongs_to/has_many/has_many_through/Pivot/Morph), value.rs (DbValue),
                           paginate.rs, migrate.rs (migrator), factory.rs, json.rs, error.rs,
                           query_log.rs (capture_queries: a task-local statement log, also
                           feeding /_renox/debug), search.rs (full-text search: the index
                           migration (FTS5 + triggers / generated tsvector + GIN), the
                           filter and rank SQL behind Query::search, rebuild), column.rs
                           (typed columns: Col, Condition, IntoColumn, Query::where_),
                           schema.rs + schema_check.rs (db:check: models vs tables),
                           schema_diff.rs + schema_sql.rs + auto_migration.rs (db:diff,
                           make:migration --auto: the SQL per dialect, SQLite rebuilds)
  src/path.rs              renox::Path: axum's Path with a 404 (not 400) when a value won't parse
                           (a parameter the route lacks is a 500); Found<M> (route model binding:
                           the parameter named after the table, else the only one; by key or
                           by a column named like the parameter)
  src/validation/          Validator/rules (mod.rs, ValidateHooks for the derive), Valid<T>
                           (extract.rs: prepare → authorize → rules → after, FormContext), English
                           messages, nested.rs (form names like `lines[0][qty]` read as a
                           tree), key_values.rs (KeyValues)
  src/auth/                User, hashing (Argon2id + bcrypt import), login/logout (per device),
                           change_password, CurrentUser middleware, AuthUser, guards, Access::check,
                           Policy/gates (mod.rs), Auth module + pages (module.rs), account.rs
                           (account pages and the sections modules add to them, #168; password
                           confirmation), passwords.rs (reset),
                           permissions.rs (Permissions module: roles, permissions, roles
                           per scope with dates: Scope, set_scope, #244), events.rs
                           (LoggedIn, LoginFailed, …), second_factor.rs (Registry::second_factor:
                           a module's step after the password; pending_login/complete_login,
                           #167), external.rs (sign_in / register_verified /
                           registration_open / confirm_identity: logging in another way than
                           the password, for renox-oauth, #147), device.rs (DeviceToken, the
                           Device extractor: tokens owned by a kiosk or till, #316),
                           verification, tokens.rs (API tokens,
                           abilities, prune), LoginThrottle (pair/account/IP), notifications
                           (Recipient, Channel::Custom, notify/notify_later,
                           SendToChannel job, DatabaseMessage, Hub), inbox.rs (the
                           notifications.* routes and the SSE stream of `.notifications()`,
                           which also carries `AppState::broadcast` events through the
                           in-process `Hub`)
  src/audit.rs             Audit module (audit_logs table, records auth events), audit::record,
                           audit:prune
  src/settings.rs          Settings module (settings table), SettingsGroup, SettingsChanged, Setting<T>,
                           AppState::settings/save_settings (+ _in scopes), the per-process cache with
                           a version row in `cache`, settings:get|set|list, App::settings_from (#29)
  src/context.rs           renox::context: task-local values per request/job/task/command
                           (default scopes, context::app())
  src/queue/               Job trait, Queue (dispatch, chain, batch), Middleware, Worker,
                           dashboard.rs (Dashboard module, stats)
  src/schedule.rs          Schedule + runner, ScheduledTask builder, own cron parser, run claims
  src/timezone.rs          Zone (UTC / fixed offset / IANA via chrono-tz) for APP_TIMEZONE
  src/events.rs            Event, listeners, AppState::emit
  src/grid/                mod.rs: renox::grid: Grid/Column, GridRequest, filters from the query string,
                           header rows, GridPage (serialized for renox/grid.html), preferences
                           (grid_preferences / session) and their /_renox/grid/{grid}/prefs route,
                           merged cells, RowOrder; searchable/prefix/row_url/empty_state;
                           Action/Selection (bulk_action, row_action, selected); Summary
                           (Column::summary), groups/group_by; cards_on_mobile and cell kinds
                           (image, color, badges, icons, description, tooltip, wrap, limit,
                           link, copyable); related/count_of/sum_of, advanced_filter,
                           remember, poll; export.rs: CSV, Excel (`xlsx`), print page,
                           export_as (any query) + ExportFormat
  src/import.rs            renox::import: Import (CSV or Excel → rows checked as forms via
                           validation::extract::parse_pairs, written in savepoints; dry_run,
                           undoable), ImportReport (IntoResponse: toast or
                           views/import_report.html; error files), template(), template_xlsx()
  src/import/xlsx.rs       (feature `xlsx`) reading a sheet with calamine as the text a CSV
                           would hold, the error workbook, TemplateColumn
  src/import/undo.rs       ImportTx (records inserts/updates), import_batches /
                           import_changes, undo / Undo (one transaction, updated_at check,
                           FK refusal, can_undo hook), prune
  src/mail.rs              Mail (recipients, cc/bcc/reply_to/from, attachments), Mailer
                           (smtp/log/memory, MAIL_FAILOVER), named mailers (App::mailer,
                           mailer_named, queue_mail_via), mail_view, queue_mail, /_renox/mail
  src/cache.rs             Cache (memory / database store), remember(), add/pull/increment,
                           Lock/LockGuard (`renox:lock:*` rows), prune
  src/counters.rs          counters in the cache table (renox:count:…) for shared throttles/login lock
  src/provided.rs          App::provide values: Provided<T> extractor, AppState::provided
  src/cookies.rs           Cookies extractor (plain / encrypted), SetCookie response part
  src/calendar.rs          Ics/Event: iCalendar (RFC 5545) files, UTC, folding, escaping; Mail::attach_ics
                           and Download::ics
  src/download.rs          Download: bytes, streamed file, Storage key, stream; safe Content-Disposition
  src/command.rs           app commands: Args, Command; App::command / Registry::command, Kernel::call;
                           AppCommand (a clap Parser) + App::typed_command (renox::clap re-exported)
  src/prompt.rs            ask/ask_or/secret/confirm/choice for commands; `answering` for tests
  src/rate_limit.rs        Limiter + middleware behind Routes::throttle; named limiters
                           (App::rate_limiter: LimitRequest → Limit, Routes::throttle_by)
  src/request_id.rs        RequestId extractor + middleware (X-Request-Id)
  src/report.rs            ErrorReport/ReportKind, App::report reporters (500s, jobs failed for
                           good, failed scheduled tasks), sent in the background
  src/inspector.rs         /_renox/debug: ring buffer of the last 50 requests (views/debug.html)
  src/client_ip.rs         ClientIp extractor + TrustedProxies (TRUSTED_PROXIES); resolved once in
                           security::middleware, read by throttle, login lock, trace span
  src/maintenance.rs       down/up/status + middleware (bypass cookie)
  src/health.rs            GET /health
  src/upload.rs            Upload (multipart file field), sniffing, store/store_public, token registry
  src/upload/pipeline.rs  Pipeline, UploadStep, StepOutcome, UploadContext (App::uploads, run by Valid)
  src/upload/clamav.rs     ClamAv step (clamd INSTREAM over TCP/Unix, fails closed)
  src/upload/images.rs     Images step, behind the `images` feature (the `image` crate)
  src/storage.rs           Storage (local disk; S3 with the `s3` feature), temporary URLs, /_renox/files,
                           list/copy/rename/size/delete_all; named disks (App::disk,
                           state.disk_named, StorageConfig::from_env) served at /_renox/disks/<name>
  src/http.rs              renox::http client (reqwest behind the `http` feature) + FakeHttp
  src/i18n.rs              Translator (lang JSON files), format() with plurals and ranges,
                           RequestLocale middleware (Accept-Language with App::detect_locale), Lang
  src/live.rs              live reload: file-time polling, /_renox/live SSE, stop() on shutdown
  src/shell.rs             db:shell (run_with takes any input/output, for tests)
  src/testing.rs           TestApp / TestRequest / TestResponse for apps' tests
  src/seo.rs               seo() tags, head tags (noindex / verification / GA4 / GTM), robots.txt, Sitemap
  src/analytics.rs         analytics::event, GaClientId, ServerEvent job (`server-events` feature,
                           sent through `state.http`)
  src/webhook/             mod.rs re-exports the public paths; inbound.rs: Webhook trait, receive route,
                           webhook_calls store/retry, ProcessWebhook job, HMAC helpers; outbound.rs:
                           Endpoint, EndpointDisabled, emit_in, DeliverWebhook job, Delivery log;
                           signing.rs: Standard Webhooks sign/verify, whsec_ secrets, SignedRequest
  src/security.rs          security headers + CSP (+ nonce), csrf-exempt and webhook route sets,
                           TRUSTED_HOSTS (400 for other hosts), ETags for Routes::etag (hashed
                           here, after the view layer rendered the page)
  src/chart.rs             renox::chart: Period (extractor), Trend (count/sum/average per day or
                           month via Query::buckets), Series; the `chart(…)` template function
  src/select.rs            renox::select: SelectOption, OptionQuery (the kit's select with
                           options_url / editable)
  src/method.rs            method spoofing layer (in front of the router)
  src/embedded.rs          Embedded (views/lang/public compiled in), public-file serving + content types
  assets/                  vendored htmx.min.js (2.0.11), alpine.min.js (3.17.4) + Alpine CSP build,
                           cally.js (0.9.2); fonts/ (Inter 4.1 and Poppins 4.003 Latin woff2,
                           OFL 1.1, served by assets.rs from /_renox/fonts)
  views/                   built-in templates (error, pagination, auth/*, mail/*, ui.html (the kit),
                           debug.html (/_renox/debug), queue/dashboard.html,
                           notifications.html (the bell's page and panel)); see §4.3
  migrations/              framework-owned migrations (auth/, permissions/, audit/, settings/, queue/,
                           cache/, session/, grid/, webhook/); see §4.5
  tests/                   core-only integration tests (support/mod.rs has a small TestApp)
crates/renox-macros/       proc macros: derive Model (typed column constants, `#[model(form)]`,
                           schema attributes), FromRow, DbEnum, Validate (validate.rs), ViewData
                           (view_data.rs); #[renox::live_component] (live.rs); embedded!(),
                           migrations!(), #[renox::test]
crates/renox-2fa/          the first plugin (#146), a separate crate versioned with renox:
                           TwoFactor module (lib.rs: second_factor, account_section, its
                           views registered in `templates` unless the app has a file of the
                           name, events to the audit log when `audit_logs` exists),
                           handlers.rs (/two-factor/*), views/, two_factor table
                           (migrations/ there, prefix 00010101000700), totp.rs (RFC 6238 +
                           base32), recovery.rs (8 codes, SHA-256), qr.rs (SVG); its own
                           tests/ (it can use the macros: it depends on renox); guide
                           docs/two-factor.md (doctested from lib.rs `Guide`)
crates/renox-editors/      the editors plugin (#149, #150): Editors module (lib.rs: views
                           registered in `templates`, the `rich_text` filter, POST
                           /_renox/editors/preview), rich_text.rs (RichText, sanitize:
                           ammonia allowlist), assets.rs (files served with
                           `Registry::asset`), views/editors.html (rich_editor,
                           markdown_editor, code_editor, code_entry), assets/editors.js (one
                           module that imports Trix / Prism + CodeJar when a page needs them)
                           and editors.css, assets/vendor/ (pinned files + NOTICE + licences);
                           guide docs/editors.md (doctested from lib.rs `Guide`)
crates/renox-blocks/       the blocks plugin (#347): Blocks module (lib.rs: views registered
                           in `templates` unless the app has a file of the name,
                           `renox_blocks()` (the tags, once per page via `once`),
                           `renox_blocks_t(key, **params)`: the app's `t`, else the English
                           in texts.rs `TEXTS`, keys `blocks.*`), week.rs
                           (`renox_week_grid`: the week calendar's cells, lanes and free
                           slots, times through the `date` filter), assets.rs (hashed files
                           served with `Registry::asset` under /_renox/blocks/),
                           views/blocks.html (quantity, range_slider, keypad, swatches,
                           datetime_range, gallery, history, compare_plans, status_stepper,
                           month_calendar, availability, week_calendar, kanban + kanban_card,
                           receipt; `rx-` classes,
                           `data-rx-<block>-*` attributes: the kit owns data-rx-key/-step),
                           assets/blocks.css, assets/blocks.js (the loader: imports
                           assets/parts/<block>.js only when a page has that block; Web
                           Animations, no library); its own tests/; guide docs/blocks.md
                           (doctested from lib.rs `Guide`); browser tests
                           tests/browser/blocks.test.mjs on examples/bikeshop's /about/blocks
crates/renox-oauth/        the social login plugin (#147): OAuth module (lib.rs: providers,
                           `oauth_providers` shared with every view for the buttons, the
                           account section, events to the audit log when `audit_logs`
                           exists, views registered in `templates` unless the app has a file
                           of the name: renox/auth/login_options.html, oauth/section.html),
                           handlers.rs (/auth/{provider}/redirect|callback, DELETE
                           /auth/{provider}; state + PKCE S256 in the session key `_oauth`,
                           single use; linking rules), provider.rs (Provider trait, BoxFuture,
                           Credentials, Token, Profile, exchange_code), google.rs, github.rs,
                           model.rs (OAuthAccount, `oauth_accounts`, prefix 00010101000800,
                           no tokens stored); its own tests/ on FakeHttp; guide docs/oauth.md
                           (doctested from lib.rs `Guide`)
crates/renox-admin/        the admin panel plugin (#148, Filament's resources): Admin module
                           (lib.rs: path, title, authorize/gate, resources; views registered
                           in `templates` unless the app has a file of the name),
                           resource.rs (AdminResource trait: Model + Policy + Default, Form:
                           Validate + Serialize, columns/fields/fill/rules/entries/filters/
                           actions/query/grid/allows; AdminAction, ActionContext, Filter,
                           BoxFuture; ActionInput, SaveContext), field.rs (Field, FieldKind),
                           entry.rs (Entry), relation.rs (RelationManager: child rows),
                           texts.rs (`renox.admin.*` defaults), `Admin::slot` (#314),
                           panel.rs (routes `admin.{slug}.*` in a Routes::group, generic
                           handlers behind `Extension(Ctx<R>)`; the panel's gate then the
                           resource's `allows` before `Valid<T>` is read; bulk/row actions
                           with the built-ins delete/restore/force-delete; the trash is
                           `?filter=trashed`), views/ (renox-admin/layout, dashboard, index,
                           form, fields, show, settings; renox-admin/{slug}/cells.html from the app for
                           custom columns); settings.rs (`Admin::settings`, `AdminSettings`: a form
                           for a settings group at `{path}/settings/{key}`, tests/settings.rs, #29);
                           its own tests/ (+ tests/migrations); guide
                           docs/admin.md (doctested from lib.rs `Guide`)
crates/renox-billing/      the subscriptions plugin (#155, Laravel's Cashier): Billing module
                           (lib.rs: plans, gateways, `Setup` given to the app with
                           `Registry::provide`, the account section, listeners: AccountDeleted
                           cancels at the gateway and deletes the rows, audit entries; views
                           registered in `templates` unless the app has a file of the name),
                           events.rs (SubscriptionPaused/Unpaused, …; pause and named guards #315),
                           plan.rs (Plan, Interval), customer.rs (Billing::of → Customer,
                           Billable, Owner `kind:id`), gateway.rs (Gateway trait, BoxFuture,
                           Remote: only what's set changes a row, Notice, Payment, Checkout,
                           CheckoutRequest), stripe.rs (Checkout Sessions, Subscriptions API,
                           Stripe-Signature), xendit.rs (recurring plans, x-callback-token),
                           model.rs (Subscription, BillingCustomer, SubscriptionStatus;
                           tables `subscriptions` + `billing_customers`, prefix
                           00010101000900), sync.rs (apply a Remote: metadata or customer →
                           owner, `synced_at` orders webhooks, events), webhook/inbound.rs (one
                           Webhook `billing` at /billing/webhooks/{gateway}; a route layer
                           names the gateway and the event id in headers, the event id stored
                           as `{gateway}:{id}`), handlers.rs (billing.* pages), guard.rs
                           (SubscriptionRoutes); its own tests/ on FakeHttp + signed
                           webhooks; guide docs/billing.md (doctested from lib.rs `Guide`)
crates/renox-pdf/          the PDF plugin (#30), Typst compiled in the process: Pdf module (module.rs;
                           `Pdf::document(&state, "invoice")`, `Pdf::templates(embedded!())`),
                           document.rs (Document: data, locale, assets, fonts, lenient; render),
                           world.rs + source.rs (the `typst::World`: confined template root or
                           embedded map, in-memory assets, packages refused, `today()` through
                           the clock), fonts.rs (font book, missing-glyph check), pool.rs
                           (blocking threads, `PDF_MAX_CONCURRENT`, `PDF_TIMEOUT`), kit.rs +
                           kit/kit.typ (`@renox/kit:0.1.0`), testing.rs (`text_of`,
                           `PdfAssertions`), fonts/ (Inter, Noto Sans Arabic, OFL texts); its own
                           tests/; guide docs/pdf.md (doctested from lib.rs `Guide`)
crates/renox-cli/          `rnx`: main.rs (key:generate, forwarding), new.rs, template.rs (`new --from`:
                           renox-template.toml, the `Source` enum (path / git+url#ref), the copy
                           walker, `--set` answers), serve.rs, make.rs +
                           generate.rs (make:*), scaffold.rs (make:module --resource --fields),
                           deploy.rs (build, make:deploy), tailwind.rs (the pinned
                           standalone CLI: download via curl + SHA-256 check, build/watch; used by
                           new --tailwind, serve, build), doctor.rs (rnx doctor),
                           tools.rs (programs on PATH, the fast linker rnx new writes),
                           finish.rs (after make:module --resource: migrate, the URL, --open)
  build.rs                 sets RENOX_GIT_REV (the commit `rnx new` pins apps to)
  stubs/starter/           `rnx new --starter`: the starter kit's files, written over the
                           stubs below (same path) or next to them (`STARTER` in new.rs)
  stubs/                   the files `rnx new` writes (Cargo.toml.stub, env.stub, build.rs, src/,
                           resources/, migrations/, tests/, AGENTS.md.stub + CLAUDE.md.stub: the
                           new app's agent guide, named .stub so agents in this repo don't load
                           it); stubs/deploy/: Dockerfile, systemd service and socket (socket
                           activation), Litestream templates
examples/                  two workspace members (#351 removed the other fifteen), each with a
                           README.md and its own tests:
  hello/                   the smallest app: a guestbook (derive Validate, detect_locale, an
                           upload, a typed command); the quick start and the README's GIF
  bikeshop/                THE FLAGSHIP (epic #231): three bike stores that sell, rent and
                           service, working together; every page explains itself. src/lib.rs
                           (modules, plugins, layers, `App::report` → src/report.rs);
                           src/explain.rs (the "About this page" registry and panel) + one
                           folder per area in src/app/<area>/ (mod.rs routes, model.rs,
                           factories.rs, explain.rs: an entry per GET route, tests/about.rs
                           fails without one): about, access (permission catalogue, active
                           store, ABAC policy: owner/location/operating store), accounts, api,
                           catalog, home, multistore, plans, purchasing (contributes the suppliers
                           admin resource, #31), rentals, reports, sales, staff,
                           stock, workshop; src/seed/ (`db:seed`, `demo:seed --size large`,
                           test fixtures); the blocks from renox-blocks (#347; /about/blocks
                           shows them; public/blocks/blocks.css styles that page);
                           public/vendor/motion (motion.dev, vendored); errors/
                           (default + 503); migrations for SQLite and PostgreSQL;
                           Dockerfile + deploy/ (make:deploy); tests/<area>.rs,
                           tests/queries.rs (main pages on the large seed, no N+1),
                           tests/operations.rs; browser tests tests/browser/bikeshop-*.test.mjs.
                           Code checks permissions, never role names (tests/access.rs).
                           What the removed examples showed lives here too: each store's
                           page on its own host (home/stores.rs, `Routes::domain`,
                           `BIKESHOP_STORE_DOMAIN`), /about/fields (about/fields.rs: every
                           input ↔ Rust ↔ SQLite ↔ PostgreSQL, files public and private,
                           `Uuid` keys; docs/types.md points there; tests/fields.rs, also on
                           S3 with the `s3` feature) and /about/htmx (about/htmx.rs: the
                           htmx recipes, live)
site/                      the documentation site (package `renox-site`, publish = false): a Renox
                           app that compiles the repo's Markdown in (src/content.rs lists the
                           pages, src/render.rs: pulldown-cmark, anchors, TOC, hidden doctest
                           lines, links → /docs/{slug} or GitHub, callouts, code panels),
                           src/highlight.rs (its own syntax colours, no dependency),
                           src/icons.rs (Lucide SVGs, `icon()` in templates; each page in
                           content.rs has an icon and a one-line blurb), search, sitemap, ETags;
                           deploy/ has its systemd units; .github/workflows/release-site.yml
                           builds it for https://docs.renox.rs, whose server pulls
                           each new build (#141). A new guide in docs/ needs a line in
                           content.rs PAGES (and site/build.rs already watches docs/).
                           Renoxium hosts it on their own server (not GitHub Pages: the
                           account's user site maps project sites to a personal domain)
www/                       renox.rs: the landing page and blog (package `renox-www`, publish =
                           false; #337), a Renox app compiled into one binary. src/landing.rs (the
                           page's content as data, code coloured on the server with the docs'
                           highlighter), src/blog.rs (posts = content/blog/*.md with front matter,
                           compiled in by build.rs; adding a file adds the page, feed and sitemap
                           entries), src/lib.rs (routes, JSON-LD, Atom feed, sitemap, llms.txt,
                           llms-full.txt, /blog/{slug}.md), public/www.js (motion only: every word
                           is in the HTML), content/benchmarks.json (from benchmarks/run.sh; the
                           section hides while it is null), deploy/ (systemd, port 3090);
                           .github/workflows/release-www.yml builds it for Renoxium's server.
                           Links into the docs must name a page in site/src/content.rs PAGES
                           (tests/www.rs checks)
learn/                     learn.renox.rs: the course (package `renox-learn`, publish = false; epic
                           #538, skeleton #523), a Renox app compiled into one binary like www.
                           src/course.rs (the stages: number, slug, title, summary, topics, the
                           guide that goes deeper), src/lib.rs (routes, JSON-LD `Course`,
                           sitemap, llms.txt), deploy/ (systemd, port 3100);
                           .github/workflows/release-learn.yml builds it for Renoxium's server.
                           Coming (#524-#536): chapters in content/, the course app in
                           learn/course/ (its own workspace, excluded from the root one), a
                           CodeMirror 6 editor, challenges checked in a sandbox, screenshots and
                           videos. Links into the docs must name a page in PAGES (tests/learn.rs)
benchmarks/                Renox vs bare Axum vs Laravel (FPM and Octane) in Docker, pinned to
                           2 cores and 1 GB, driven by oha (#333): its own cargo workspace
                           (renox-app, axum-app; not a member of the root one), laravel/
                           (Dockerfile + overlay), run.sh, summarize.py, results/*.json,
                           RESULTS.md (the latest run; www/content/benchmarks.json copies it)
tests/chaos/               app + run.sh (postgres|sqlite) that the `chaos` CI job injects faults
                           into (docker pause/stop/restart, python3 holding SQLite's lock)
tests/cli/run.sh           `rnx new` + every `make:*`, then build and test the app (CI `cli`/`docker`);
                           with SQLite it serves the apps and drives them with tests/cli/smoke.py
                           (every GET page, a `--resource` module's forms, the starter's sign-up,
                           verification and roles over HTTP, #142), and the `rnx new` option matrix (plain, --tailwind, --starter, names
                           on both sides of "renox", #143)
tests/browser/            browser tests (#262, CI `browser`): run.sh builds the binaries one at a
                           time, then `node --test` (Node 24, no npm packages) drives headless
                           Chrome over CDP. lib/cdp.mjs (pages, clicks, keys, waitFor, settle,
                           problems: console errors, exceptions, CSP violations), lib/app.mjs
                           (start an app binary on a free port, migrated/seeded), fixture/ (a
                           workspace member: pages using the kit, Auth with notifications, jobs,
                           commands `jobs:push`/`jobs:nap`/`ask:me`, a route on its own host, a
                           per-second task with FIXTURE_TICK, and the data grid's pages at
                           /grid (src/grid.rs, its own migrations and `db:seed`: 480 orders,
                           demo@example.com); *.test.mjs for renox.js, the kit's forms and
                           overlays, the grid (grid, grid-more: the fixture's /grid), the
                           editors (on bikeshop's /about/fields), the guestbook (examples,
                           examples-flows), a11y (an accessibility smoke), assets.test.mjs
                           (no top-level JS function declared twice) and bikeshop-*.test.mjs
                           (each area of examples/bikeshop; bikeshop-walk: the main pages
                           under CSP=strict). `run.sh 'bikeshop-*'` takes quoted patterns
tests/process/            process e2e (#270): process.py (signals with a request in flight,
                           queue:work, two schedule:work, systemd's socket, LOG_FORMAT/LOG_FILE,
                           APP_KEY in production, db:shell, prompts from a pipe and a terminal
                           via `script`, the commands' output, `rnx serve`/forwarding/Tailwind
                           with a stand-in TAILWIND_BIN, `rnx build` with RNX_BUILD=1) on the
                           fixture; examples.py serves both example binaries and GETs their
                           pages as a guest and logged in; `run.sh postgres` with
                           PROCESS_POSTGRES runs the database checks and bikeshop on
                           PostgreSQL (EXTRA_ENV: per-example variables, e.g. bikeshop's
                           BIKESHOP_STAFF_2FA=optional)
tests/tutorial/           run.sh + follow.py: docs/tutorial.md followed as a reader does (steps
                           found by their lead-in sentence, never line numbers), then fmt,
                           clippy, the tutorial's tests, seed, the app answering (CI `tutorial`).
                           Changing a tutorial step's wording may need follow.py changed too
docs/ui.md                 components (as rx-* tags first), the UI kit, toasts, fragments, htmx
                           headers, live validation, stacks, view:check, typed views, editor
                           autocomplete, Tailwind (doctest `UiGuide`)
docs/live.md               live components (doctest `LiveGuide`)
docs/state-machines.md     state machines for models: Machine, transitions, guards, the guarded status
                           column, history, interceptor, diagrams (doctest `StateMachinesGuide`)
docs/grid.md               the data grid (renox::grid): columns, filters, actions, summaries,
                           exports (doctest `GridGuide`)
docs/routing.md            routes, groups, domains, extractors, middleware, guards, sessions,
                           CSRF, cookies, signed URLs (doctest `RoutingGuide`)
docs/validation.md         Valid<T>, rules, derive Validate, hooks, messages (doctest
                           `ValidationGuide`)
docs/mail.md               mail and notifications (doctest `MailGuide`)
docs/scheduling.md         scheduler, events, cache and locks, app commands (doctest
                           `SchedulingGuide`)
docs/testing.md            TestApp: requests, assertions, fakes, time travel, browser tests
                           (doctest `TestingGuide`)
docs/tutorial.md           one app (Stash) from `rnx new` to deploy (doctest `TutorialGuide`)
docs/laravel.md            Laravel → Renox, concept by concept (doctest `LaravelGuide`)
docs/types.md              HTML input ↔ Rust ↔ SQLite ↔ PostgreSQL (doctest `TypesGuide`)
docs/relations.md          relations without N+1, fetch_as/FromRow (doctest `RelationsGuide`)
docs/search.md             full-text search: #[model(search)], the index migration, ranking
                           (doctest `SearchGuide`)
docs/authorization.md      gates, policies, roles/permissions, token abilities, tenants, the
                           second login step, logging in another way (doctest
                           `AuthorizationGuide`)
docs/oauth.md              social login with renox-oauth (doctest: renox-oauth's `Guide`)
docs/blocks.md             blocks beyond the kit with renox-blocks (doctest: its `Guide`)
docs/admin.md              the admin panel with renox-admin (doctest: renox-admin's `Guide`)
docs/billing.md            subscriptions with renox-billing (doctest: renox-billing's `Guide`)
docs/pdf.md                PDF documents with renox-pdf (doctest: its `Guide`)
docs/settings.md           typed settings in the database (doctest: renox-admin's `SettingsGuide`,
                           since it shows the admin page)
docs/queue.md              jobs, retries, priority, unique, middleware, chains, batches (doctest
                           `QueueGuide`)
docs/postgresql.md         PostgreSQL guide for app authors
docs/development.md        faster builds: profiles, linker, default features, sccache, cargo-chef
docs/stability.md          semver scope, #[non_exhaustive] types, public-dependency policy
docs/operations.md         production: timeouts, proxies, /health, failure table (kept in sync
                           with tests/chaos/run.sh), failed jobs/webhooks, backups, deploys
                           without refused connections, sessions, logs, error reports, error
                           pages, the debug inspector (doctest `OperationsGuide`)
docs/audit/                pre-1.0 audit (2026-09-pre-1.0.md, closed), Laravel parity review
                           (2026-10-laravel-parity.md, current as of M32) and its English
                           summary (2026-10-laravel-gap-report.pdf, printed from HTML by
                           headless Chrome)
docs/assets/demo.gif       the README's demo (see §4.10)
.github/workflows/ci.yml   CI jobs (see §4.11)
.github/ISSUE_TEMPLATE/    issue forms: bug.yml, story.yml (user story + acceptance criteria),
                           task.yml; config.yml (no blank issues; Discussions, security, docs)
.github/                   pull_request_template.md (the PR body's sections), dependabot.yml
                           (grouped cargo/actions updates), release.yml (release-note sections)
```
