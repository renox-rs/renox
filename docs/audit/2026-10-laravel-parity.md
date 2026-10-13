# Laravel parity review (October 2026, after M32; updated for M33 and M34)

**Question:** is Renox mature enough to compete with Laravel (and, for admin work, Filament), and
if not, what is still missing?
**Method:** the September review (at M17) listed Laravel 11/12's features area by area and rated
Renox against the code. This review takes every row of it again and checks the code on `main`
after M32 (`d316f30`): each "closed" row names the Renox API that closes it, found with grep in
`crates/renox-core/src`, `crates/renox-cli/src`, `crates/renox/src/lib.rs`,
`crates/renox-core/views/ui.html` and the examples, and the milestone that shipped it (from
CHANGELOG.md). It then adds what Renox gained since M17 that the old review didn't list, and
rows for Laravel and Filament features a Laravel developer would look for that are still
missing (each checked to be really absent). The Laravel side comes from knowledge of Laravel
11/12 and Filament 3, not from a fresh read of their docs.

Legend: ✅ equivalent · 🟡 partial · ❌ missing · ⛔ not planned (ROADMAP "Not planned") ·
n/a not needed in Rust or by design.
Impact, for a Laravel developer building a typical SaaS or business app: **B** blocker,
**Maj** major, **Min** minor. "Closed in" is the milestone that shipped the Renox side;
"post-M28 (#nn)" is one of the UI kit PRs between M28 and M29.

## Verdict

**For its niche, nearly there in features; not yet in maturity.** In September the answer was
"not yet" for two reasons: missing SaaS features and missing maturity. M18–M32 closed almost
all of the first kind:

- **Every September gap planned for M18–M21 is closed**, and so are the leftovers moved to
  M22–M25 (non-integer keys, savepoints, encrypted fields, domains, `route_is`, derived
  validation, `Accept-Language`). Of the 94 gap rows in the September tables, 63 were ✅
  after M32, 16 🟡 and 15 ❌/⛔; M33 and M34 then closed most of the minor ones (the tables
  below are current).
- **Renox went past the September scope** with what Laravel developers get from Filament: a data
  grid (M27–M28), form fields, infolists, actions, notifications and dashboard widgets in the UI
  kit (post-M28), navigation and a theme (M30–M31), and `examples/backoffice` built the way
  Filament's demo is (M29b).
- **For HTML-over-the-wire business and SaaS apps on one server or a few**, a Laravel developer
  now finds an equivalent for nearly everything they use daily. What is left in code is small:
  search and broadcasting (social login, 2FA and billing are done as plugins, `renox-oauth`, `renox-2fa` and `renox-billing`; M33 and M34 closed the
  small adds: route model binding, most missing rules, the breach check, several disks and
  mailers, ETags, the `XSRF-TOKEN` cookie, trusted hosts, error bags, `has_many_through`).
- **What really separates Renox from Laravel now is maturity:** no release (crates.io holds
  0.0.1 placeholders, no git tags), one maintainer, a week of public history, no third-party
  packages, no docs site, tutorial or community. Only v1.0 and time fix these, and the owner
  decides when v1.0 starts.
- **A separate `renox-admin` is less needed than planned.** The kit, the grid,
  `make:module --resource --fields` and `examples/backoffice` already give what Filament's
  panels give, except that pages are written (or generated once) instead of declared as a
  "resource" at run time. A plugin that turns a model into a whole admin section is still
  a convenience worth having, but it is no longer a gap that blocks an admin app. (Done
  since: `renox-admin`, #148.)

## Changes since the September review

| Milestone | What it closed from the September review |
|---|---|
| M18a | tenancy (`Model::default_scope`, scoped `unique`/`exists`), `require_gate`/`gate_before`, the `Permissions` module, token abilities |
| M18b | account pages, `Password` policy and confirmation, per-device logout, auth events, the `Audit` module, bcrypt import |
| M19a/M19b | raw fragments, `group_by`/`having_raw`, locks, `where_has`, count/sum loaders, cursor and simple pagination, transaction retries, `first_or_new`/`update_or_create`/`refresh`; model hooks, partial saves, pivot data, `Morph`, `state.encrypt` |
| M20a–M20c | cron/weekly/monthly schedules with IANA zones, cache locks and `add`/`pull`/`increment`/prune; unique, encrypted, prioritised jobs, job middleware, chains, batches; `renox::http` with a fake, schedule pings, the queue dashboard, localized mail and notifications |
| M21a–M21i | `renox::Path` 404s, components that see the request, the UI kit, toasts, `View::also`, live validation; `Routes::resource` and generators, `TestApp` assertions, time travel, fakes; `App::report`, JSON logs, request ids, named rate limiters, `/_renox/debug`; Tailwind, stacks, typed commands, prompts; form-request hooks and rules; database sessions and socket activation |
| M22–M25 | `Ulid`/`Uuid`/`String` keys; savepoints and `Encrypted<T>`; `Routes::domain`/`fallback`, `route_is`, `Redirect::route`/`intended`, session `push`/`increment`, factory states, plural ranges, `class_names`, `loop_controls`; `#[derive(Validate)]`, `App::detect_locale` |
| M26 | bug fixes, every public item documented (`missing_docs` enforced), CLI tests |
| M27–M28 | the data grid (Filament's tables) |
| post-M28 (#88–#99) | Filament's forms, infolists, notifications, widgets and actions in the kit; `renox::chart`; `renox::select` |
| M29–M32 | every example complete and on the kit, `examples/backoffice`, the kit's navigation, the warm theme, English only |
| M33 | 28 validation rules (`gt`/`lt`, `decimal`, `dimensions`, `json`, `prohibits`, …), `Found<M>` route model binding, `Routes::view`/`redirect`, named disks, `Routes::etag`, `App::xsrf_cookie`, `TRUSTED_HOSTS` |
| M34 | `current_password`, `Password::uncompromised`, session `keep`/`now`, named error bags, `App::mailer` + `MAIL_FAILOVER`, `has_many_through` |

The five bugs and traps the September review found are all fixed:

| Finding | Fixed by | In |
|---|---|---|
| `User::set_password` logged out the current session | `auth::change_password` (keeps this session, ends the others) | M18b |
| `auth::logout` ended every device | per-device logout with the `revoked_sessions` denylist; `logout_other_devices` | M18b |
| `APP_TIMEZONE` refused IANA names | `timezone.rs` (`Zone`, chrono-tz, DST) | M20a |
| A bad `Path<i64>` value gave a plain-text 400 | `renox::Path` (404 page; 500 for a parameter the route lacks) | M21a, M26c |
| Expired database cache rows never pruned | `Cache::prune`, `cache:prune` | M20a |

## Maturity signals (measured on 2026-10-03)

| Signal | At M17 | Now (after M32) |
|---|---|---|
| Releases | none | none: crates.io has 0.0.1 placeholders for `renox` and `renox-cli`, no git tags; apps pin a git commit |
| Public history | first commit 2026-09-27 | the same first commit (2026-09-27); 251 commits, PRs up to #108; one maintainer |
| Issues, stars, forks | 0 | 0 |
| Rust in `crates/` (stubs excluded) | ~34k lines | ~65.3k lines |
| Examples | 11, ~5.3k lines | 14, ~15.9k lines (api, backoffice, crud, fields, grid, hello, htmx-recipes, jobs, postgres, relations, shop, teams, uploads, webhooks) |
| Test functions (`#[test]`, `#[renox::test]`, `#[tokio::test]` in `crates/` and `examples/`) | ~400 | 763 |
| Doctest holders | README, CHEATSHEET, 6 guides | 15 in `crates/renox/src/lib.rs`: `ReadMe`, `CheatSheet`, 12 guides, `MacroCompileErrors` (`compile_fail` checks); plus every public item's doc example |
| Guides in `docs/` | 6 | 15 (authorization, development, grid, mail, operations, postgresql, queue, relations, routing, scheduling, stability, testing, types, ui, validation) and `docs/audit/` |
| CI (`.github/workflows/ci.yml`) | the same jobs | 11 jobs, 15 runs: lint, test ×3 OSes, PostgreSQL, chaos ×2, MSRV 1.94, feature matrix, cli ×2, Docker, S3, cargo-deny, coverage |
| `rnx make:*` generators | 9 | 16 (command, component, deploy, event, factory, job, mail, middleware, migration, model, module, notification, policy, rule, seeder, test), plus `make:module --resource --fields` and `make:model --key` |
| Validation rules | see the September row | 68 rule methods on `Field` (40 after M32, 28 more in M33), plus `rule`/`apply` for custom checks, `each`/`distinct` on `Validator`, the `Password` policy |
| UI kit | none | 64 macros in `views/ui.html`, plus the grid (`views/grid.html`) |
| Direct dependencies of renox-core | not measured | 44 (6 optional) |

## Where Renox is better

The September list still holds, and it grew:

- **Deploy and operations:** one binary runs the web server, queue workers and scheduler, with
  views, translations, public files, fonts and migrations embedded. No Redis, Supervisor, cron
  entry or Octane. `rnx make:deploy` writes a Dockerfile, a systemd service and **socket**
  (deploys without refused connections, M21g) and Litestream; CI builds the image and checks
  `/health`. A chaos test stops, pauses and locks the database under a running app.
- **Security by default:** CSP with nonces and security headers, uploads checked by content and
  served sandboxed, a three-way login lock shared across servers, constant-time login, verified
  and idempotent webhooks, typed forms (no mass assignment), same-site redirects only, encrypted
  columns whose key never leaves `Db`, per-device logout with a denylist.
- **Data layer:** no N+1 by design (no lazy relations; loaders take a page of rows), typed
  columns checked by the compiler and identical on SQLite and PostgreSQL, race-safe
  `first_or_create`, bind-limit-aware bulk inserts, transactional dispatch (`dispatch_in`), a
  migrator with a lock, checksums and all-or-nothing rollback.
- **A data grid in the framework:** Filament's table features (filters, search, bulk and row
  actions, summaries, groups, exports to CSV/Excel/print, column moving and resizing, cards on
  phones, polling) from one `Grid` value and one template call, with no JavaScript build.
- **htmx:** 422 errors placed next to fields with no per-form code, live validation against the
  same `Valid<T>` (Precognition without a package), `.fragment()`/`.also()`, toasts that survive
  redirects, htmx test helpers. SEO, sitemaps and GA4 built in. Live reload without Vite.
- **Tests:** every test gets its own migrated database, in parallel, with no `RefreshDatabase`;
  time travel reaches the queue, sessions and rate limits through one `clock`.
- **Coding agents:** llms.txt, a compiled cheat-sheet, and an AGENTS.md/CLAUDE.md in every new
  app linking the docs of the pinned Renox commit.

## 1. HTTP layer: routing, requests, responses, validation

**Equivalent (✅), as in September:** verbs and parameters, named routes, groups, `route:list`,
method spoofing, middleware, guards, extractors instead of DI, typed input, uploads,
`ClientIp`, old input, cookies, responses, downloads, `Back`, htmx helpers, flash, CSRF,
`route()`/`url`, signed URLs, hashed assets, error pages, shared rate limits, CORS per route,
maintenance mode.

| Gap (September) | Laravel | Renox now | St | Imp | Closed in |
|---|---|---|---|---|---|
| Scoped `unique`/`exists` | `Rule::unique()->where()->ignore()` | `unique(table, col).where_eq(..).where_null(..).ignore(id)`; default scopes apply | ✅ | – | M18a |
| Resource routes and scaffolding | `Route::resource`, `make:controller --resource` | `Routes::resource(path, name, Resource)`, `rnx make:module --resource --fields` | ✅ | – | M21c |
| Error reporting hook | `report()`, Sentry/Flare | `App::report` (`ErrorReport`: 500s, jobs failed for good, failed scheduled tasks) | ✅ | – | M21d |
| Named, dynamic rate limiters | `RateLimiter::for` | `App::rate_limiter` (`LimitRequest` → `Limit`), `Routes::throttle_by` | ✅ | – | M21d |
| Query parameters in `route()` | extra params → query string | the same | ✅ | – | M21d |
| Validation rules | ~100 rules | 40 rules: `required_with/without/if/unless`, `prohibited_if`, `alpha*`, `uuid`, `ip`, `starts_with`/`ends_with`, `size`, `same`/`different`, `digits*`, `lowercase`/`uppercase`, `password(&Password)`, `each`/`distinct`; Rust types cover `integer`/`numeric`/`boolean`/`array`/`date`. Missing: see the row below | 🟡 | Min | M18b, M21f, M25 |
| Form Request hooks | `authorize`, `prepareForValidation`, `after` | `Validate::prepare`/`authorize`/`after` with `FormContext` (async), `#[derive(Validate)]` + `#[validate(hooks)]` | ✅ | – | M21f, M25 |
| Log channels, JSON logs, request id | channels, formatters, `withContext` | `LOG_FORMAT=json`, `LOG_FILE`, `X-Request-Id` (`RequestId`) in every span; one sink, no per-channel routing | ✅ | – | M21d |
| Server-side sessions | file/db/redis | `SESSION_DRIVER=database` (`sessions` table, id rotation) | ✅ | – | M21g |
| Subdomain routing, fallback route | `Route::domain`, `Route::fallback` | `Routes::domain` (`DomainParams`), `Routes::fallback` | ✅ | – | M24 |
| Route model binding | implicit, custom keys, scoped | `Found<M>`: the parameter named after the table, else the only one; by key, or by a column named like the parameter (`{slug}`); default scopes and soft deletes apply | ✅ | – | M33 |
| Optional params, `where`, `any`, `redirect`/`view` routes | yes | bad values are 404s (`renox::Path`); `Routes::route(path, any(h))` takes any axum `MethodRouter`; `Routes::view`, `redirect`, `permanent_redirect`; no optional params or constraints | 🟡 | Min | M21a, M33 |
| `can:` middleware | `->can()` | `require_gate`, `require_role`, `require_permission`, `require_ability`, `require_password_confirmed` | ✅ | – | M18a, M18b |
| Terminable middleware, ETag, trusted hosts | yes | axum middleware can work after `next.run`; hashed assets are cached `immutable`; `Routes::etag()` (`304` on `If-None-Match`); `TRUSTED_HOSTS` | ✅ | – | M33 |
| `routeIs`, current route in views | yes | `route_is('orders.*')`, `request.route`, `CurrentRoute` | ✅ | – | M24 |
| `redirect()->route()`, `intended()` | yes | `Redirect::route`, `Redirect::intended` (`RedirectExt`) | ✅ | – | M24 |
| Session `push/increment/keep/now` | yes | `push`, `increment`, `pull`, `reflash`, `keep(&[keys])`, `now(key, value)` | ✅ | – | M24, M34 |
| Named error bags | yes | `Validate::ERROR_BAG` / `#[validate(bag = "…")]`, `error(field, bag=…)`, `errors_in(bag)`, `bag=` on the kit's fields | ✅ | – | M34 |
| `XSRF-TOKEN` cookie for SPAs | yes | `App::xsrf_cookie()`: the cookie, and `X-XSRF-TOKEN` accepted | ✅ | – | M33 |

New rows:

| Feature | Laravel | Renox | St | Imp | Since |
|---|---|---|---|---|---|
| Validation rules missing in September | `json`, `gt/gte/lt/lte` (field against field), `decimal`, `dimensions`, `prohibited`/`prohibited_unless`/`prohibits`, `required_with_all`/`required_without_all`, `max_digits`/`min_digits`, `multiple_of`, `mac_address`, `ulid`, `timezone`, `declined` | all of these, plus `accepted_if`/`declined_if`, `numeric`/`integer` for text, `ascii`, `hex_color`, `doesnt_start_with`/`doesnt_end_with`, `not_matches` (`Dimensions`) | ✅ | – | M33 |
| Validation rules still missing | `extensions`/`mimetypes`, `exclude*`/`missing*` | `mimes` covers file types by content; `rule(valid, message)` and `Rule` for the rest; `Password::uncompromised()` done (M34) | 🟡 | Min | open |
| `current_password` rule | yes | `Field::current_password()`, checked with the request's user by `Validator::finish_for` | ✅ | – | M18b, M34 |
| Live validation | Precognition | `<form data-live-validate>` against the same `Valid<T>` (`X-Renox-Validate`) | ✅ | – | M21b |
| Nested form input | `lines.*.qty` | `lines[0][qty]` read as a tree (`validation/nested.rs`), dotted error keys | ✅ | – | post-M28 (#88) |
| API resources (JSON transformers) | `JsonResource` | serde `Serialize` structs and `Json`; `Paginated` serializes | ✅ | – | n/a |
| Request context | `Context` (Laravel 11) | `renox::context`, `Current<T>` | ✅ | – | M18a, M21a |

## 2. Data layer: query builder, models, relations, migrations

| Gap (September) | Laravel | Renox now | St | Imp | Closed in |
|---|---|---|---|---|---|
| Global scopes (tenancy) | `addGlobalScope` | `Model::default_scope`, `unscoped()`, `renox::context` | ✅ | – | M18a |
| Raw fragments | `whereRaw`, `selectRaw`, `groupBy`, `having` | `where_raw`, `select_as`, `order_by_raw`, `group_by`, `having_raw` | ✅ | – | M19a |
| Pessimistic locking | `lockForUpdate`, `sharedLock` | `lock_for_update`, `shared_lock` | ✅ | – | M19a |
| Aggregate loaders | `withCount`, `withSum` | `count_many`/`sum_many` on relations, `Morph::count_many`; the grid's `count_of`/`sum_of` | ✅ | – | M19a, M21a, M28e |
| Existence queries | `whereHas`, `has('>', n)` | `where_has`, `where_doesnt_have` (EXISTS); a count threshold needs `where_raw` | ✅ | – | M19a |
| Non-integer keys | UUID/ULID/string | `Model::Key`: `i64`, `Ulid`, `Uuid`, `String`; `make:model --key` | ✅ | – | M22 |
| Model events, observers | `creating`, `saved`, observers | `#[model(hooks)]` + `ModelHooks` (`saving`/`saved`/`deleting`/`deleted`) | ✅ | – | M19b |
| Dirty tracking, partial updates | `isDirty`, `getChanges` | `save_only`, `save_changes`, `update_columns` | ✅ | – | M19b |
| Pivot data | `withPivot`, timestamps | `attach_with`, `load_with_pivot`, `update_pivot`, `toggle`, `Pivot<L, R>` | ✅ | – | M19b, M22 |
| Polymorphic relations | morphTo/morphMany/morphToMany | `Morph` (`parents`, `load`, `count_many`); no polymorphic many-to-many | 🟡 | Min | M19b |
| Cursor and simple pagination | yes | `cursor_paginate`, `simple_paginate` | ✅ | – | M19a |
| Transactions | `DB::transaction(fn, attempts)`, savepoints | `db.transaction`, `transaction_retrying`, `Db::retrying`, `Transaction::savepoint` | ✅ | – | M19a, M21a, M23 |
| hasOneOfMany, hasManyThrough | yes | `relations::has_many_through` (two queries); "latest of many" as a join with `fetch_as` | 🟡 | Min | M34 |
| `firstOrNew`, `updateOrCreate`, `refresh`, `touch` | yes | `first_or_new`, `update_or_create`, `refresh`, `touch` | ✅ | – | M19a, M19b |
| Factory states, sequences | yes | `Factory::factory()`, `state`, `sequence`, `count`; no `has()`/`for()` | ✅ | – | M24 |
| Query log, `toSql` | `DB::listen`, `toSql` | `to_sql`, `capture_queries`, SQL per request in `/_renox/debug` | ✅ | – | M19a, M21a, M21d |
| Schema builder | `Blueprint` | plain SQL, one file per database when they differ | ⛔ | Maj for some | – |
| MySQL, read/write split, several connections | yes | SQLite and PostgreSQL; one `Db` per app (`db::connect` is internal; `renox::db::sqlx` is re-exported for a second pool by hand) | 🟡 | Min | – |
| Redis | cache/queue/session | the database (`CACHE_STORE=database`) | ⛔ | Min (Maj at scale) | – |

New rows:

| Feature | Laravel | Renox | St | Imp | Since |
|---|---|---|---|---|---|
| Encrypted casts | `encrypted` cast | `db::Encrypted<T>` (key carried by `Db`/`Row`), `state.encrypt`/`decrypt` | ✅ | – | M19b, M23 |
| Lazy collections, streaming rows | `lazy()`, `cursor()` | `chunk` (by key), `cursor_paginate`; no row stream | 🟡 | Min | M19a |
| Model pruning | `Prunable`, `model:prune` | a scheduled `delete()` query; built-in `*:prune` commands for framework tables only | 🟡 | Min | – |
| Time series | Trend packages | `renox::chart::Trend::of(query, column)` (count/sum/average per day or month) | ✅ | – | post-M28 (#97) |

## 3. Authentication, authorization, security

| Gap (September) | Laravel | Renox now | St | Imp | Closed in |
|---|---|---|---|---|---|
| Roles and permissions | spatie/laravel-permission | the `Permissions` module (roles, permissions, `has_role`, `users_with_role`) | ✅ | – | M18a, M21a |
| Account pages | Breeze/Jetstream profile | `Auth::new().account()`: name, email (re-verify), password, delete account | ✅ | – | M18b |
| Route-level gate, `Gate::before` | `can:`, `before` | `require_gate`, `App::gate_before`; one check in `auth::Access::check` | ✅ | – | M18a |
| Password rules and confirmation | `Password::min()…`, `password.confirm` | `Password` (`min`, `letters`, `mixed_case`, `numbers`, `symbols`), `require_password_confirmed` | ✅ (no `uncompromised`) | – | M18b |
| Auth events, audit log | Login, Failed, Lockout…; activitylog | `auth::events` (`LoggedIn`, `LoginFailed`, `LockedOut`, …), the `Audit` module, `audit::record` | ✅ | – | M18b |
| Token abilities | `tokenCan` | token abilities, `require_ability` | ✅ | – | M18a |
| Log out this device / other devices | `logoutOtherDevices` | `logout` (this device), `logout_other_devices` | ✅ | – | M18b |
| Encryption API, encrypted fields | `Crypt`, casts | `state.encrypt`/`decrypt`, `Encrypted<T>` | ✅ | – | M19b, M23 |
| Importing Laravel users | bcrypt, `needsRehash` | bcrypt verified, rehashed to Argon2id at login | ✅ | – | M18b |
| 2FA (TOTP, recovery codes) | Fortify | the `renox-2fa` crate: TOTP from `/account`, the challenge, recovery codes, events and the audit log | ✅ | – | #146 |
| Social login | Socialite | the `renox-oauth` crate: Google, GitHub and a `Provider` trait, PKCE, linking by verified email, link/unlink from `/account` | ✅ | – | #147 |
| Several user types / guards | several providers | one `users` table + roles | ❌ | Min | "one table + roles" |
| Teams | Jetstream | `examples/teams` (default scopes, context, roles, domains); not a module | 🟡 | Min | M18a, M24 |
| Impersonation, HTTP Basic, OAuth2 server | packages, Passport | none (`http` client has `basic_auth`, the server doesn't) | ❌/⛔ | Min | – |

New rows:

| Feature | Laravel | Renox | St | Imp | Since |
|---|---|---|---|---|---|
| Breached-password check | `uncompromised()` | none (ROADMAP: optional HIBP check) | ❌ | Min | open |
| Branded auth pages | Breeze views | built-in pages on the kit, overridable by file (`renox/auth/layout.html`, shown in backoffice) | ✅ | – | M21f, M29b |
| Tenancy | stancl/tenancy | `default_scope` + `renox::context` + scoped rules + `Routes::domain` | ✅ | – | M18a, M24 |

## 4. Background work and services

| Gap (September) | Laravel | Renox now | St | Imp | Closed in |
|---|---|---|---|---|---|
| Schedules beyond daily, time zones | `cron()`, weekly, `timezone()` | `cron`, `weekly_on`, `monthly_on`, `weekdays`, `between`, `timezone` (IANA, DST) | ✅ | – | M20a |
| Atomic locks | `Cache::lock` | `cache.lock(..)`: `try_acquire`, `block`, `release` | ✅ | – | M20a |
| Unique jobs | `ShouldBeUnique` | `Job::UNIQUE_FOR` + `unique_id` | ✅ | – | M20b |
| Job middleware | `RateLimited`, `WithoutOverlapping` | `Job::middleware` (`rate_limited`, `without_overlapping`, `release_after`) | ✅ | – | M20b |
| Chains and batches | `Bus::chain`, `Bus::batch` | `queue.chain`, `queue.batch` (`then`/`catch`/`finally`, progress, `allow_failures`) | ✅ | – | M20b, M21a |
| Localized mail and notifications | `->locale()` | `Recipient::in_locale` / `locale` (or the user's `locale` column), `mail_view_in`, `t()` in mail views | ✅ | – | M20c |
| Per-recipient channels | `via($notifiable)` | `channels_for(&Recipient)` | ✅ | – | M20c |
| HTTP client with fakes | `Http::fake` | `renox::http` (`state.http`), `TestApp::fake_http` | ✅ | – | M20c |
| Queue dashboard and metrics | Horizon, Pulse | `renox::queue::Dashboard` at `/_renox/queue`, `/health` counts; no metrics history or alerts | 🟡 | Min | M20c |
| Schedule hooks and pings | `onFailure`, `thenPing` | `on_failure`, `on_success`, `ping_before`, `then_ping`, `ping_on_*` | ✅ | – | M20a, M20c |
| Queue priority | ordered queues | `queue:work high,default` drains in order | ✅ | – | M20b |
| Cache `add`/`pull`/`increment`, tags, pruning | yes | `add`, `pull`, `increment`/`decrement`, `prune`; no tags | ✅ (no tags) | Min | M20a |
| `dispatch_sync`, encrypted payloads, `failed()`, forget/prune | yes | `dispatch_sync`, `Job::ENCRYPTED`, `Job::failed`, `queue:forget`, `queue:prune-failed` | ✅ | – | M20b |
| Plural ranges, `Accept-Language` | yes | `{0}`/`[2,*]` ranges; `App::detect_locale` (with `Vary`) | ✅ | – | M24, M25 |
| Multiple mailers, failover, API drivers | SES, Postmark, Resend | `App::mailer` + `mailer_named`/`queue_mail_via`, `MAIL_FAILOVER`; drivers SMTP, log, memory (every provider offers SMTP; no HTTP API drivers) | 🟡 | Min | M34 |
| Several named disks, directory ops | yes | `list`, `copy`, `rename`, `size`, `delete_all`; `App::disk(name, …)` + `state.disk(name)` (local or S3 each, `StorageConfig::from_env`) | ✅ | – | M20c, M33 |
| Broadcasting (Reverb/Pusher) | yes | no general broadcasting; SSE for the notification bell (`/notifications/stream`) and live reload | ⛔/🟡 | Min–Maj | – |

New rows:

| Feature | Laravel | Renox | St | Imp | Since |
|---|---|---|---|---|---|
| Notification channels | mail, database, broadcast, Slack, SMS (packages) | `Channel::Mail`, `Database`, `Custom` (`App::channel`); no built-in Slack/SMS | 🟡 | Min | M14c |
| Database notifications UI | Filament database notifications | `Auth::new().notifications()`, the kit's `notification_bell`, `DatabaseMessage`, SSE stream | ✅ | – | post-M28 (#96) |
| Queued listeners | `ShouldQueue` listeners | listeners run in-process; a listener dispatches a job | 🟡 | Min | – |
| Webhooks in | Spatie webhook-client | `Webhook` trait, verified, stored, retried (`webhook:retry`) | ✅ | – | before M17 |
| Search | Scout (Meilisearch, Algolia, database) | `where_like` (ILIKE on PostgreSQL), the grid's `searchable`; no full-text index driver | ❌ | Maj for some | open |
| Billing | Cashier (Stripe, Paddle) | the `renox-billing` crate: plans in code, trials (also without a payment method), swap (prorated at Stripe), cancel with a grace period, resume, `require_subscription`, verified and idempotent webhooks; Stripe and Xendit, another gateway is one `Gateway` impl | ✅ | – | #155 |
| Feature flags | Pennant | none (a setting or a gate) | ❌ | Min | open |

## 5. Views and frontend

| Gap (September) | Laravel | Renox now | St | Imp | Closed in |
|---|---|---|---|---|---|
| Components that see the request | `<x-input>` with `old()`, `$errors` | imported macros see `old`, `error`, `t`, `csrf_field`, `auth`, `request` (`RequestGlobal`) | ✅ | – | M21b |
| UI kit and `make:component` | Flux, Filament, Breeze | the kit (`renox/ui.html`, 64 macros), `make:component`, `ui:publish` | ✅ | – | M21b, post-M28, M30 |
| Admin / CRUD scaffolding | Filament, Nova | the `renox-admin` crate (resources declared once), `make:module --resource --fields`, the grid, the kit, `examples/backoffice` | ✅ | – | M21c, M27–M30, #148 |
| Toasts over htmx | Filament notifications | `Toast` (body, actions, duration, id, position), `Renox.toast` | ✅ | – | M21b, post-M28 (#96) |
| `@push`/`@stack`/`@once` | yes | `push`, `prepend`, `stack`, `once` | ✅ | – | M21e |
| Error pages in the app layout | full context | `errors/{status}.html` with the page globals and `App::share` values | ✅ | – | M21d, M21h |
| Live validation | Precognition | `data-live-validate` | ✅ | – | M21b |
| Tailwind | Vite + Tailwind | `rnx new --tailwind`, `rnx tailwind`, `rnx serve` (pinned standalone CLI) | ✅ | – | M21e |
| Fragments, out-of-band swaps, htmx headers | `fragments()` | `.fragment()`, `View::also`, `HxRetarget`, `HxReswap`, `HxPushUrl` | ✅ | – | M21b |
| `@class`, `break`/`continue`, `truncate` | yes | `class_names`, `loop_controls`, the `words` filter | ✅ | – | M24, post-M28 (#94) |
| Livewire | stateful components | live components (`#[renox::live_component]`, `rx-click`/`rx-submit`/`rx-model`, `TestApp::live`; docs/live.md; the bike shop's `/about/htmx/live`) | ✅ | – | – |
| Inertia | SPA | htmx + Alpine (the bike shop's `/about/htmx`); a JSON API for a SPA | ⛔ | Maj for SPA teams | – |
| Markdown mail components | `x-mail::button/panel/table` | `renox/mail/components.html`: `button`, `panel`, `table`, `divider` | ✅ | – | M20c |

New rows (Filament as the yardstick):

| Feature | Filament | Renox | St | Imp | Since |
|---|---|---|---|---|---|
| Tables | Filament tables | `renox::grid`: filters by kind, chips, `advanced_filter`, `searchable`, `bulk_action`/`row_action`, `summary`, `group_by`, `exports` (CSV, Excel, print), `editable`, `reorder`, column moving/resizing, `remember`, `poll`, `cards_on_mobile`, `related`/`count_of`/`sum_of` | ✅ | – | M27a–M28e |
| Form fields | Filament forms | `radio`, `checkbox_list`, `toggle_buttons`, `file`, `date_picker`, `tags_input`, searchable/multiple `select` with `options_url` (`renox::select`), `repeater`, `key_value`, `wizard`, `show_when`/`hide_when`, `form_grid`/`fieldset` | ✅ | – | post-M28 (#88, #92) |
| Rich text, Markdown, code editors, colour picker | yes | none (ROADMAP: plugins, large JavaScript) | ❌ | Min | open |
| Infolists | infolists | `infolist`, `entry`, `repeatable`; `money`, `since`, `words`, `markdown` filters | ✅ | – | post-M28 (#94) |
| Actions | actions, modals | `action_sheet`, `slide_over`, `icon_button`, `confirm`, shortcuts (`data-rx-key`), tooltips; no action groups | ✅ | – | post-M28 (#99) |
| Widgets | stats, charts | `stat`/`stats`/`dashboard`/`widget`/`period_filter`, `chart(…)` (SVG, no library), `renox::chart` (`Period`, `Trend`, `Series`) | ✅ | – | post-M28 (#97) |
| Panel navigation | panels | `navbar`, `sidebar` + `rx-shell`, `page_header`, `toolbar`, `link_tabs`, `list`, `card_grid` | ✅ | – | M30 |
| Themes | themes, colours | warm default, `data-rx-theme="classic"`, `--rx-*` tokens and type scale, bundled Inter/Poppins | ✅ | – | M31 |
| Resources declared at run time | `Resource` classes | `renox-admin`: `impl AdminResource` (columns, fields, filters, actions, the model's policy), or pages generated once (`make:module --resource`) and edited | ✅ | – | #148 |
| Global search | yes | none | ❌ | Min | open |
| Demo app | Filament demo | `examples/backoffice` (invoices, stock ledger, payments, import, exports, roles, activity log) | ✅ | – | M29b |

## 6. Tooling, testing, operations, ecosystem

| Gap (September) | Laravel | Renox now | St | Imp | Closed in |
|---|---|---|---|---|---|
| Release, support policy, community | yearly majors, LTS, Laracasts | pre-1.0, git pins, one maintainer, 0 stars/issues | ❌ | **B** | v1.0 |
| Ecosystem packages | Cashier, Scout, Socialite… | none; many Laravel packages are built in (permissions, activity log, webhooks, tenancy, grid) | ❌ | **B**→Maj | plugins |
| JSON assertions | `assertJson`, `assertJsonPath` | `assert_json`, `assert_json_path`, `json_path`; no `assertJsonStructure` | ✅ | – | M21c |
| Session, auth, view assertions | `assertSessionHas`, `assertAuthenticated`, `assertViewHas` | `assert_session_has`/`missing`, `assert_authenticated`/`guest`, `assert_view`, `assert_hx_redirect` | ✅ | – | M21c |
| Time travel | `travelTo` | `TestApp::travel`/`travel_back` (`clock`) | ✅ | – | M21c, M21i |
| Fakes | Event, Notification, Http, Bus… | `fake_events`, `fake_notifications`, `fake_http`, memory mailer, `queued_jobs`, a temp disk | ✅ | – | M20c, M21c |
| Generators | ~35 `make:*` | 16 | 🟡 | Min | M21c |
| Observability | Telescope, Debugbar, Pulse | `/_renox/debug` (last 50 requests, SQL, view), `App::report`, the queue dashboard, `/health`; no Pulse-like metrics, no OpenTelemetry export | 🟡 | Min | M20c, M21d |
| Typed commands, prompts | signatures, Laravel Prompts | `AppCommand` (clap) + `App::typed_command`, `renox::prompt` (`ask`, `secret`, `confirm`, `choice`) | ✅ | – | M21e |
| Browser tests | Dusk | `TestApp::serve` + a CDP recipe (docs/testing.md); no browser API in Renox | 🟡 | Min | M21c |
| Docs site, API reference | laravel.com | 15 guides (12 compiled as doctests), README, CHEATSHEET; no site, not on docs.rs | 🟡 | Maj | v1.0 |
| Zero-downtime deploys, hosted platforms | Forge, Vapor, Cloud | systemd socket activation, Docker, Litestream from `make:deploy`; no hosted platform | 🟡 | Min | M21g |
| Tinker | REPL | `db:shell` + app commands | ⛔ | Min | – |

New rows:

| Feature | Laravel | Renox | St | Imp |
|---|---|---|---|---|
| Sail, Herd, Valet | local environments | n/a: `cargo run` with SQLite needs nothing installed | n/a | – |
| Octane | long-running workers | n/a: the binary is long-running | n/a | – |
| Envoy | SSH task runner | n/a: `rnx build` + `make:deploy` | n/a | – |
| Vapor (serverless) | Lambda | not planned | ⛔ | Min |
| Pint | formatter | rustfmt, clippy in CI | ✅ | – |
| Starter kits | Breeze/Jetstream, React/Vue/Livewire kits | `rnx new` (layout on the kit, auth pages, account pages, `--tailwind`); `rnx new --starter` (verification, roles, dashboard, users, activity log; V1e). No JavaScript-framework kits: htmx is the stack | ✅ | – |
| Agent support | Boost | llms.txt, CHEATSHEET, AGENTS.md/CLAUDE.md in new apps | ✅ | – |

## Still open, ranked

1. **Maturity (B):** a release on crates.io (the owner runs it, RELEASING.md); then users,
   contributors, community. Done for v1.0 (V1a–V1e): `cargo-semver-checks` in CI, the API
   audit, the docs site with a tutorial and a "Laravel → Renox" guide, the starter kit, the
   support policy and the semver promise (docs/stability.md, SECURITY.md).
2. **2FA (Maj):** TOTP and recovery codes (`renox-2fa`). Done (#146): docs/two-factor.md.
3. **Social login (Maj):** OAuth providers (`renox-oauth`). Done (#147): docs/oauth.md.
4. **Small adds (Min):** M33 and M34 closed the list the review started with (validation
   rules, route model binding, disks, `XSRF-TOKEN`, ETags, trusted hosts, view/redirect
   routes, `current_password`, the breach check, several mailers and failover, session
   `keep`/`now`, error bags, `has_many_through`). Left, all minor: polymorphic many-to-many;
   a row stream; cache tags; queued listeners; Slack/SMS channels; HTTP API mail drivers.
5. **Admin (Min):** `renox-admin`, a resource declared at run time over the grid and kit.
   Done (#148): docs/admin.md. Left: global search; rich text/Markdown/code editors are
   done as a plugin (`renox-editors`).
6. **Search (Maj for some):** a Scout-like full-text interface (SQLite FTS5, PostgreSQL
   `tsvector`, an external engine).
7. **Realtime (Min–Maj):** general SSE broadcasting from the notification `Hub` pattern
   (WebSockets stay not planned).
8. **Billing (Maj for SaaS):** subscriptions over Stripe or local gateways; feature flags.
   Subscriptions done (#155): `renox-billing` with Stripe and Xendit, docs/billing.md.

Not planned, unchanged: a schema builder, Redis, WebSockets, Inertia-style SPAs (Livewire-style live components are done, docs/live.md),
serverless, a REPL.

## Plan

In the order the owner ranked the open work (CLAUDE.md §7, "Still open"):

1. **Release and docs (v1.0):** API audit, `cargo-semver-checks` in CI, real crates
   (`renox`, `renox-core`, `renox-macros`, `renox-cli`) with docs.rs, the docs site built with
   Renox (tutorial, "Laravel → Renox" guide, API reference), a starter kit, the semver
   guarantee in docs/stability.md. Started 2026-10-03; everything but the release is done
   (V1a–V1e).
2. **2FA and social login:** `renox-2fa` (TOTP, recovery codes, password confirmation reused)
   and `renox-oauth` (Google, GitHub, … linked to `users`), as separate crates.
3. **Small adds:** done in M33 and M34. What's left is minor and can wait for users to ask:
   polymorphic many-to-many, a row stream, cache tags, queued listeners, HTTP API mail
   drivers.
4. **Admin:** `renox-admin` done (#148), next to the kit, the grid and the generators;
   editors done as a plugin (`renox-editors`); global search left.
5. **Search, realtime, billing:** a search interface over FTS5/`tsvector`; SSE broadcasting
   generalised from `auth::notifications::Hub`; billing done (`renox-billing`, #155).

The September version of this review (`2026-09-laravel-parity.md`) is in git history; this file
replaces it. [2026-10-laravel-gap-report.pdf](2026-10-laravel-gap-report.pdf) is a short summary of it.
