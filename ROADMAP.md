# Renox Roadmap

Renox aims to be for Rust what Laravel is for PHP: a batteries-included web
framework where a new app works out of the box.

> **Open work is tracked on GitHub** since 1.0.0-rc.3: issues (bugs, user stories, tasks),
> milestones (releases) and the project board; see "How work is tracked" in
> [CONTRIBUTING.md](CONTRIBUTING.md). This file keeps the principles, the record of what each
> milestone built and the decisions behind it. An unticked box below is history unless an
> issue links to it.

## Principles

1. **Convention over configuration.** Folder layout, naming and defaults are decided for you.
2. **One dependency, every battery.** `renox = "x.y"` and `use renox::prelude::*`. Behind it:
   the `renox` facade, one runtime crate `renox-core` (see Decisions), `renox-macros` and the
   `rnx` CLI (`renox-cli`); heavy batteries are switched on with cargo features.
3. **Stand on mature crates.** Renox is the glue, conventions and developer experience on top of
   axum, sqlx, minijinja, lettre and friends (the queue is Renox's own, see Decisions).
4. **HTMX-first.** Response helpers know whether a request wants a full page or a fragment.
5. **Single-binary deploys.** SQLite, templates and htmx/Alpine assets ship inside the binary.
6. **Dogfooded.** Features are exercised by the apps in `examples/` (the bike shop and hello,
   #351), the guides' doctests and the integration tests.

## Application layout

```
my-app/
├── Cargo.toml              # renox pinned to a git rev (until real crates.io releases)
├── .env
├── build.rs                # rerun when migrations/ changes
├── src/
│   ├── lib.rs              # pub fn app() -> App { App::new().module(...) }
│   ├── main.rs             # my_app::app().run()
│   └── app/
│       └── products/       # one module per folder
│           ├── mod.rs      # impl Module
│           ├── routes.rs
│           ├── handlers.rs
│           ├── model.rs
│           ├── policy.rs
│           └── jobs.rs
├── migrations/
├── resources/
│   ├── views/              # layouts/, components/, products/
│   └── lang/               # en.json, es.json (other locales)
├── public/
├── storage/                # app.db, uploads/, logs/
└── tests/
```

## Milestones

### M0 · v0.1: Foundation
- [x] Workspace: `renox` (facade + prelude), `renox-core`, `renox-cli`
- [x] `App` builder and `Module` trait (routes)
- [x] Typed `Config` from `.env` (`APP_NAME`, `APP_ENV`, `APP_DEBUG`, `APP_URL`, `APP_KEY`, `APP_HOST`, `APP_PORT`)
- [x] `Error` type with error pages; details only when `APP_DEBUG` is on
- [x] Request logging with `tracing`, graceful shutdown (Ctrl-C / SIGTERM)
- [x] `examples/hello`

### M1 · v0.2: Web layer
- [x] `Routes` builder with named routes; URL generation with `state.url()` and `route()` in templates
- [x] Sessions in an encrypted cookie (`APP_KEY`), flash messages, old input, flashed errors
- [x] CSRF protection: `X-CSRF-Token` header (sent automatically for HTMX) or `_token` field; 419 Page Expired
- [x] MiniJinja views: layouts, request globals (`app`, `request`, `flash`, `errors`, `old()`, `csrf_field()`, `renox_head()`), reload in dev
- [x] Built-in error page, overridable per status with `errors/{status}.html`
- [x] HTMX helpers: `Htmx` extractor, `view().fragment()`, `HxRedirect`, `HxRefresh`, `HxTrigger`, `Back`
- [x] htmx 2.0.11 and Alpine.js 3.17.4 embedded with cache-forever URLs; `public/` served at the root
- [x] CLI: `rnx new`, `rnx serve` (rebuild and restart on change), `rnx key:generate`
- [x] Route groups with a shared prefix and name prefix (M14a, `Routes::group`)
- [x] CSRF token in multipart forms (M6b)

### M2 · v0.3: Database
- [x] SQLite pool from `DATABASE_URL` with WAL, foreign keys, a busy timeout; `State(db): State<Db>` in handlers
- [x] Migrations in `migrations/*.up.sql` / `.down.sql`, embedded with `renox::migrations!()`, per app or per module
- [x] Batches like Laravel: `migrate`, `migrate:rollback [--step N]`, `migrate:fresh [--seed]`, `migrate:status`
- [x] The app binary is its own command line (`my-app migrate`); `rnx migrate` forwards to it
- [x] `rnx make:migration`; `rnx serve` migrates before each restart and rebuilds when migrations change
- [x] `#[derive(Model)]`: `find`, `find_or_404`, `all`, `create`, `save`, `delete`, `force_delete`, `restore`, timestamps, soft deletes, skipped fields
- [x] Query builder: `where_eq/op/like/in/null/not_null`, `order_by`, `latest`, `limit`, `offset`, `get`, `first`, `count`, `exists`, bulk `delete`; unknown columns and operators are errors
- [x] Pagination: `Page` extractor, `paginate()`, built-in `renox/pagination.html` macro
- [x] Transactions via `db.begin()`, seeders (`App::seeder`, `db:seed`), factories (`Factory`, `fake`)
- [x] Server-side sessions: revocation on logout came first, as a session version (M13a, W2);
      the store itself is `SESSION_DRIVER=database` (M21g)
- [x] Pagination links that keep other query parameters (M10b)

### M3 · v0.4: Forms and validation
- [x] `Valid<T>` extractor for forms, JSON bodies and GET query strings
- [x] Regular posts are redirected back with errors and old input flashed (passwords never are)
- [x] HTMX and JSON requests get `422 {"message", "errors"}`; the bundled script shows the errors next
      to the inputs (`data-error-for` slots or inserted `<p class="error">`), sets `aria-invalid`,
      focuses the first invalid input and leaves what the user typed in place
- [x] Rules: `required`, `min`, `max`, `between`, `email`, `url`, `one_of`, `confirmed`, `accepted`,
      custom `rule(bool, msg)`, and database-backed `unique` (with `ignore(id)`) and `exists`
- [x] `label()` and `message()` per field; empty inputs count as missing; wrong types become field errors
- [x] Messages in English and Indonesian (`APP_LOCALE=en|id`); `error('field')` in templates
- [x] Handlers can return their own `ValidationError` / `Errors`
- [x] `#[derive(Validate)]` with attribute rules, for forms that only need the basics (M25)
- [x] More rules: `regex`, dates, `digits` (M15b), file uploads (M6b)

### M4 · v0.5: Authentication and authorization
M4a (done):
- [x] `Auth` module: `users` table, `/login`, `/register`, `/logout`, Argon2id password hashing
- [x] Remember me (session lasts `REMEMBER_LIFETIME`), new CSRF token on login
- [x] Changing a password logs out the user's other sessions; deleted users are logged out
- [x] Login throttling: 5 failures per email and IP per minute; unknown emails take as long as known ones
- [x] `AuthUser` / `Option<AuthUser>` extractors; `Routes::require_auth()` and `guest_only()`;
      guests are sent to `login` and back to the page they wanted (HTMX via `HX-Redirect`, JSON gets 401)
- [x] Policies (`impl Policy for Model`, `auth.authorize("update", &model)?`) and gates
      (`App::gate("admin", |user| ...)`, `auth.gate("admin")?`, `can('admin')` in templates)
- [x] `auth.check` / `auth.user` in templates (the password hash is never serialized)
- [x] Built-in pages in English and Indonesian, overridable per file (`renox/auth/*.html`)
- [x] `rnx new` apps come with `Auth` and a login/logout nav

M4b (done):
- [x] Password reset by email: one-time links valid for 60 minutes, one email per address per
      minute, the same reply for unknown addresses; resetting ends other sessions
- [x] Email verification with signed links (`Auth::new().verify_email()`,
      `Routes::require_verified()`), resend, and `verification.notice`
- [x] Signed URLs for any route (`state.signed_url(..)`, `ValidSignature` extractor)
- [x] `log` and `memory` mail drivers (`MAIL_MAILER`), so both flows work before SMTP arrives in M5
- [x] API tokens: `user.create_token()`, `tokens()`, `revoke_token()`; `Authorization: Bearer id|secret`
      skips CSRF, answers 401 JSON when invalid, can expire, and records `last_used_at`

### M5 · v0.6: Background work
M5a (done):
- [x] Queue in SQLite: `Job` trait (name, queue, attempts, timeout, backoff), `state.dispatch()`,
      `dispatch_after()`, retries, a `failed_jobs` table, abandoned reservations released after 15 minutes
- [x] Workers inside `serve` (`QUEUE_WORKERS`, woken immediately on dispatch) or on their own with
      `queue:work [--queue a,b] [--workers N] [--once]`; running jobs finish on shutdown
- [x] `queue:failed`, `queue:retry <id|all>`, `queue:flush`
- [x] Scheduler defined in code: `every`, `every_minute(s)`, `hourly`, `daily_at("HH:MM")` in
      `APP_TIMEZONE`; overlapping runs are skipped; `schedule:list`, `schedule:work`
- [x] Events and listeners (`Event`, `App::listen`, `state.emit`); queue slow work from a listener
- [x] `Module::register` for a module's jobs, listeners and scheduled tasks
- [x] `rnx` forwards any other command to the app (`rnx queue:work`, `rnx schedule:list`, ...)

M5b (done):
- [x] SMTP driver (lettre with rustls: `tls`, `starttls` or `none`), plus `log` and `memory`
- [x] Mail templates: `state.mail_view(to, subject, "mail/x", ctx)` renders `x.html` and `x.txt`
      (or text made from the HTML), a built-in `renox/mail/layout.html` and `button` macro
- [x] `state.queue_mail(mail)` sends through the queue with retries
- [x] `/_renox/mail` lists recent mail with HTML and text previews while `APP_DEBUG` is on
- [x] Notifications: `Notification` with mail and database channels, `state.notify(&user, &n)`,
      and `user.notifications()`, `unread_notifications()`, `mark_notification_read()`
- [x] Password reset and verification emails use the templates (HTML + text, English and Indonesian)
- [x] HTML escaping leaves `/` alone (like Jinja2), so URLs in pages and mail stay readable

### M6 · v0.7: Infrastructure
M6a (done):
- [x] Cache: `state.cache` with `get`, `put` (TTL), `has`, `remember`, `forget`, `flush`; `memory`
      or `database` store (`CACHE_STORE`, table `cache`); values stored as JSON
- [x] Rate limiting: `Routes::throttle(max, per)` per user or IP; 429 with `Retry-After`,
      `X-RateLimit-Limit` / `X-RateLimit-Remaining` on allowed responses
- [x] Maintenance mode: `down [--secret S] [--retry N]` / `up`; 503 through the error page
      (`errors/503.html` overrides it); `/S` sets a bypass cookie; state in `STORAGE_PATH`
- [x] `GET /health`: database ping, queue counts, maintenance flag; 503 when the database is down;
      unaffected by maintenance mode and sessions

M6b (done):
- [x] `Upload` form fields (`Option<Upload>`) through `Valid<T>` for `multipart/form-data`
- [x] File rules: `image()` (checked from the content, not the name), `mimes(&[..])`, `min`/`max`/
      `between` in kilobytes; "must be a file" for text sent to a file field; empty file inputs are
      missing
- [x] `state.storage`: `put`, `get`, `exists`, `delete`, `url` (public keys under `public/`),
      `temporary_url` (signed for the local disk, presigned for S3); keys can't escape the root
- [x] `upload.store(..)` / `store_public(..)` with random names and extensions from the content
- [x] Local disk (`STORAGE_PATH/app`, public files at `/storage/...` without sessions) and
      S3/R2/MinIO behind the `s3` cargo feature (object_store)
- [x] CSRF `_token` read from multipart forms; `UPLOAD_MAX_SIZE` body limit (413 above it)
- [x] `storage_url(key)` in templates; the guestbook takes an optional photo
- [x] Several files in one field (`Vec<Upload>`), with per-file rules (M15b)

M6c (done):
- [x] `resources/lang/{locale}.json` (`LANG_PATH`), nested or flat; reloaded on change in debug;
      invalid JSON fails at boot
- [x] `t('key', name='…', count=n)` in templates and `Lang` (`lang.t`, `lang.choice`) in handlers;
      `:name`/`:Name` placeholders, `one|many` plurals, fallback to `APP_FALLBACK_LOCALE`, then the key
- [x] Per-visitor language: `i18n::set_locale(&session, "en")`, else `APP_LOCALE`; `app.locale` follows it
- [x] Built-in texts are translatable from the same files: `renox.validation.*` messages,
      `renox.validation.attributes.*` field names, `renox.auth.*` page texts — so languages
      beyond en/id work without changing Renox
- [x] Validation messages and auth pages follow the visitor's language
- [x] The guestbook and `rnx new` apps ship `en.json` and `id.json`; the guestbook has an ID | EN switch
- [x] Choosing the language from `Accept-Language` (opt-in): `App::detect_locale()` (M25)

### M7 · v0.8: CLI and developer experience
- [x] Generators: `rnx make:module` (routes, view, `pub mod` and `.module(..)` in src/lib.rs,
      or main.rs for older apps),
      `make:model [-m]`, `make:migration`, `make:job`, `make:policy`, `make:mail`; they never
      overwrite files
- [x] `route:list`: method, path, name, module and guards (`auth`, `guest`, `throttle:…`) of every
      route, the framework's included
- [x] `db:shell`: a built-in SQL prompt (`.tables`, `.quit`, piped input), no `sqlite3` needed
- [x] `db:seed`, `key:generate` (earlier milestones)
- [x] Browser live reload while developing locally: views, public and lang files trigger a reload
      over `/_renox/live` (SSE); a restart by `rnx serve` reloads after reconnecting; open streams
      end on shutdown so it stays graceful

### M8 · v0.9: Testing and deployment
M8a (done):
- [x] `renox::testing::TestApp` (a module, not a separate crate: it needs the kernel's internals):
      boots the app with an in-memory, migrated database, the memory mailer, no workers, a temporary
      storage directory; keeps the session cookie and sends CSRF tokens itself
- [x] Requests: `get`, `post`, `put`, `patch`, `delete`, `post_json`, `htmx()`, `json()`,
      `header()`, `without_csrf()`; `acting_as(&user)`, `logout()`
- [x] Assertions: `assert_ok/status/redirect/hx_redirect/not_found/forbidden/unauthorized`,
      `assert_see/dont_see`, `assert_header`, `assert_invalid(field)`, `json()`;
      `assert_database_has/missing/count`, `queued_jobs()`, `run_jobs()`, `assert_mail_sent()`
- [x] `#[renox::test]` (Tokio test through Renox's re-export, so apps don't depend on tokio)
- [x] Apps are a library plus a tiny `main.rs` (`rnx new` writes `src/lib.rs` with `pub fn app()`
      and `tests/home.rs`), because tests can't import from `main.rs`; the guestbook has tests too

M8b (done):
- [x] `renox::embedded!()` + `App::embed(..)`: `resources/views`, `resources/lang` and `public/` are
      compiled into the binary and used when `APP_DEBUG` is off; debug builds keep reading the disk
      (live reload). A release build runs from one file plus `.env`
- [x] `rnx build`: release build copied to `dist/<name>`
- [x] `rnx make:deploy`: a multi-stage `Dockerfile` (+ `.dockerignore`), a systemd unit that migrates
      before starting and stops gracefully, a Litestream config for continuous SQLite backups to
      S3/R2, and `deploy/README.md` with the steps
- [x] `rnx new` apps embed their files and ignore `dist/`

### M9 · v0.10: PostgreSQL (before 1.0)
SQLite stays the default and is right for an app on one server. PostgreSQL is for apps that
outgrow that: several app servers, heavy concurrent writes, or a managed database. It has to land
before 1.0 because `Db` was a plain `SqlitePool` alias; turning it into a type that can hold
either pool after 1.0 would break every app. Split in two PRs: M9a (Renox's own database layer,
still SQLite underneath) and M9b (the PostgreSQL backend proper).

- [x] `Db` becomes Renox's own type (holding a SQLite or a PostgreSQL pool), chosen at boot from
      the `DATABASE_URL` scheme (`sqlite://…` or `postgres://…`); `State(db): State<Db>` stays the
      same, with `db.sqlite()` / `db.postgres()` for raw sqlx queries (M9a)
- [x] `Transaction` (`db.begin()`, pass `&mut tx` where `&db` goes), `Row` (`try_get` by name or
      position) and raw SQL with `renox::db::sql("… ?").bind(v).fetch_all(&db)` / `scalar` /
      `execute`; models, queries and all framework code go through them (M9a)
- [x] PostgreSQL behind a `postgres` cargo feature, like `s3`, so SQLite-only apps don't compile it
      (M9a: the feature, the pool and the connection; M9b makes the framework's own SQL work on it)
- [x] `?` placeholders rewritten to `$1`, `$2`, … on PostgreSQL, skipping quotes and comments (M9a)
- [x] `DbValue` binding for PostgreSQL's stricter types: untyped `NULL`, `BOOLEAN`, `TIMESTAMPTZ`,
      `TIMESTAMP`, `DATE`, `TIME` (new `DbValue` variants, `#[non_exhaustive]`); `OFFSET` without
      `LIMIT`; `like` sent as `ILIKE` so it ignores case on both (M9b)
- [x] Framework migrations in both dialects: `BIGINT GENERATED BY DEFAULT AS IDENTITY`,
      `TIMESTAMPTZ`, `BIGINT` counters; emails stored lowercased with a unique index on
      `lower(email)` (chosen over `citext`: a `citext` column compared with a text parameter is
      case-sensitive again) (M9b)
- [x] App migrations per dialect when SQL differs: `migrations/*.up.sql` for both, with optional
      `*.sqlite.up.sql` / `*.postgres.up.sql` overrides picked by `migrations!()` at run time (M9b)
- [x] `migrate:fresh` and `db:shell` (`.tables`, cell display, banner) handle both (M9a)
- [x] No SQLite-only statements left in the framework's own SQL (`ON CONFLICT … DO UPDATE/NOTHING`
      and `RETURNING` work on both) (M9b)
- [x] Queue workers on PostgreSQL reserve jobs with `FOR UPDATE SKIP LOCKED`, so workers on
      several servers never take the same job (M9b)
- [x] Scheduled runs are claimed in the `cache` table (insert-if-absent per task and time slot),
      so several app servers can run `serve` without running a task twice; works on SQLite too and
      lifts the `SCHEDULER=false` workaround. Chosen over advisory locks, which would need a
      held connection per run and still let a slower server run the same slot after the first
      finished (M9b)
- [x] `/health`, `db:shell`, `route:list` and the `rnx` generators work with both (M9b;
      `make:migration` writes SQL for the app's `DATABASE_URL`)
- [x] `rnx new --database postgres`; `.env` templates document `DATABASE_URL` and
      `TEST_DATABASE_URL` (M9b)
- [x] CI runs the whole test suite against SQLite and against PostgreSQL (a service container);
      `TEST_DATABASE_URL` gives every in-memory test app a fresh PostgreSQL schema (M9b)
- [x] A guide: [docs/postgresql.md](docs/postgresql.md): new apps, switching, portable SQL,
      migrations, tests, moving the data with pgloader, deploying (M9b)

### M10 · v0.11: Examples and docs for people and coding agents (before 1.0)
An agent (or a person) building on Renox should be able to find the idiomatic way to do something by
opening one short file, not by reading the framework. That saves tokens and stops agents copying
outdated APIs. Split in PRs: M10a (cheat-sheet, llms.txt, agent files in new apps,
`examples/crud`), M10b (the framework gaps the examples exposed, below), M10c (the other
examples, and the gaps they exposed) and M10d (doctests on public APIs).

- [x] Small, focused examples, one pattern each, every one compiled and tested in CI (an untested
      example goes stale, and a stale example is worse than none):
  - [x] `examples/hello`: routes, views, forms, validation, uploads (exists)
  - [x] `examples/crud`: model, migration, pagination, soft deletes and a trash, policy, flash (M10a)
  - [x] `examples/api`: API tokens (Bearer), JSON validation errors, CORS, rate limit (M10c)
  - [x] `examples/jobs`: events, queued mail, notifications, a scheduled task (M10c)
  - [x] `examples/uploads`: file rules, public files, private files behind expiring links (M10c)
  - [x] `examples/postgres`: one app on PostgreSQL and SQLite, tested on both in CI (M10c)
  - [x] `examples/webhooks`: payment gateway webhooks (M11b)
- [x] Short files, no decorative code, comments only where something isn't obvious; the official way
      only (when there are two ways, show the main one)
- [x] Each example names the generator commands that made its files (`rnx make:model Product`, …),
      so agents know not to type the boilerplate
- [x] `CHEATSHEET.md`: one page of the most common patterns (commands, app/module/routes, views,
      forms + validation, model + migration + queries, pagination, auth/policies/gates, HTMX, raw
      SQL + transactions, jobs/events/schedule/mail, cache/session/uploads/i18n, tests, `.env`);
      every Rust block is compiled by `cargo test --doc -p renox` (M10a)
- [x] `llms.txt` at the repo root: what each example and guide covers, file by file (M10a)
- [x] Doc comments on public APIs get doctests instead of `ignore`: all 35 examples in renox-core
      compile (renox is a dev-dependency so they're written the way apps write them; setup lines
      hidden with `# `); writing them found a stale one (`Session::prune`) (M10d)
- [x] Apps from `rnx new` ship an `AGENTS.md` (and a `CLAUDE.md` importing it) with the layout,
      the generators, where the cheat-sheet and examples are, and the checks to run (M10a)

Gaps found while writing examples/api, jobs, uploads and postgres (M10c, closed):
- [x] Errors for API clients (`Accept: application/json` or a JSON body) are `{"message": …}` with
      the status, also for errors raised outside the view layer (CSRF's 419)
- [x] JSON bodies report every field's errors at once, like forms (missing fields, wrong types
      with placeholders, rules)
- [x] `User::attempt(db, email, password)` for token endpoints, as slow for unknown emails as for
      wrong passwords
- [x] `TestApp::post_multipart(uri, fields, files)` for testing uploads

Gaps found while writing the first example, to close before 1.0:
- [x] Method spoofing (`_method=PUT|PATCH|DELETE` in plain HTML forms, urlencoded or multipart, or
      the `X-HTTP-Method-Override` header), with `{{ method_field('PUT') }}`; a layer in front of
      the whole router, since route layers run after the method is matched (M10b)
- [x] When a field fails to parse (e.g. `price=abc` for an `i64`), it gets its error and a
      placeholder (`0`, then `false`) so the rest parses and every other field's rules still run;
      a placeholder's own rule errors are dropped (M10b; urlencoded and multipart forms — JSON
      bodies still report the first parse error)
- [x] `can('update', product)` in templates for policies: the handler wraps models in
      `auth::Can::new(model, user, &["update", …])` (`Paginated::map` for pages), which adds `_can`
      (M10b)
- [x] Pagination links keep the other query parameters: `page_url(n)` in templates reads
      `request.query` (M10b)

### M11 · v0.12: Web essentials (before 1.0)
What most real apps need on day one besides pages and a database: safe defaults in the browser,
payment gateway callbacks, and being found and measured. Owner's request, done before M10c so the
remaining examples use the final APIs. Order: M11a → M11b → M11c → M10c → v1.0.

M11a · security (done):
- [x] Security headers on every response: `X-Content-Type-Options: nosniff`,
      `Referrer-Policy: strict-origin-when-cross-origin`, `X-Frame-Options: SAMEORIGIN`, and HSTS in
      production over https; a header the handler set is kept
- [x] Content-Security-Policy, `CSP=relaxed|strict|off`. **relaxed is the default** (owner's
      choice): this site's scripts, inline scripts and `eval` (Alpine's standard build) are allowed;
      other sites' scripts, framing by other sites and plugins are not. `strict` allows scripts only
      from this site or with the request's nonce (`csp_nonce()` in templates), switches to Alpine's
      CSP build (vendored) and turns off htmx's `eval`; Alpine expressions must then be simple
      (move statements into `Alpine.data(...)`)
- [x] `App::csp(|csp| { csp.allow("script-src", "https://…"); })` adds sources; a new directive keeps
      `'self'`
- [x] `Routes::cors(&["https://app.example.com"])` (or `"*"`) answers preflights and adds the
      CORS headers for the routes added so far; `cors_layer(renox::cors::CorsLayer)` for anything
      else; `route:list` shows `cors`
- [x] `Routes::without_csrf()` for callers without a session (webhooks); `route:list` shows `no-csrf`

M11b · webhooks (done):
- [x] `impl Webhook` (`PROVIDER`, `verify`, `event_id`, `handle`) + `Routes::webhook::<W>(path)` +
      `app.webhook::<W>()` (boot fails if a webhook route's provider isn't registered)
- [x] `renox::webhook` helpers: `sha256_hex`, `sha512_hex`, `hmac_sha256_hex`, `hmac_sha512_hex`,
      `verify_hmac_sha256` (`sha256=` prefix, any hex case), `verify_timestamped` (Stripe-style
      `t=…,v1=…` with a tolerance against replays), `same` (constant time), `ensure`, `secret`
- [x] A `webhook_calls` table (both databases), unique per (provider, event id): a call is stored
      and its processing queued in one transaction, answered 200 at once; duplicates are answered
      200 and not processed again; forged calls 401, calls without an event id 400
- [x] Processing in the queue (`renox:webhook`, 5 attempts, backoff): status `processed` or
      `failed` with the error; `webhook:failed`, `webhook:retry <id>`, `webhook::retry(state, id)`
- [x] Webhook routes skip CSRF, keep working in maintenance mode, and show in `route:list`
- [x] `Config::var(name)` / `config.vars` for secrets (tests set them without touching the
      process environment); `TestApp::post_body` / `TestRequest::post_body` for exact bytes
- [x] `examples/webhooks`: Midtrans (`signature_key` SHA-512), Xendit (`x-callback-token`), Stripe
      (`Stripe-Signature`), each tested with good, forged and repeated calls

M11c · SEO and analytics (done):
- [x] `seo(title=…, description=…, image=…, type=…, canonical=…)` in templates (a Rust function,
      since imported macros can't see the page's `app`/`request`): title, description, canonical
      (APP_URL + path, no query), OpenGraph, Twitter cards; `<html lang="{{ app.locale }}">` and a
      `{% block seo %}` in the generated layout; `noindex, nofollow` outside production
- [x] `/favicon.ico` answers 204 (cached a day) unless `public/favicon.ico` exists (after M21)
- [x] `/robots.txt` generated unless `public/robots.txt` exists (production: allow + the sitemap
      when a route is named `sitemap`; elsewhere disallow); `renox::seo::Sitemap` builder
- [x] Search Console verification meta (`GOOGLE_SITE_VERIFICATION`)
- [x] GA4 (`GA4_MEASUREMENT_ID`) and Tag Manager (`GTM_CONTAINER_ID`) tags in `renox_head()` with
      the CSP nonce, and their hosts added to the CSP; production only
- [x] `renox::analytics::event(&session, name, params)`: delivered in the htmx swap's `HX-Trigger`,
      the page's head, or the next page after a redirect; renox.js calls `gtag('event', …)` and
      pushes to the GTM `dataLayer`. Page views on `hx-boost` need nothing (GA4 enhanced
      measurement counts history changes; GTM has a History Change trigger)
- [x] `ServerEvent` job for GA4's Measurement Protocol (`GA4_API_SECRET`), `GaClientId` from the
      `_ga` cookie; only sent in production
- [x] `renox::serde_json` re-export and `json!` in the prelude (apps from `rnx new` had no
      `serde_json` for event parameters)

### M12 · v0.13: Types from the form to the database (before 1.0)
Asked by the owner after M10: do the examples cover every type in the database, in Renox and in
the UI? They didn't, and a probe of what browsers send showed three inputs that didn't work.

- [x] Checkboxes: `on` (and `1`, `yes`) read as `true`, an unchecked box (nothing sent) as `false`
- [x] `<input type="datetime-local">` without seconds reads as `NaiveDateTime`
- [x] Multi-selects and checkbox groups (repeated names) read into `Vec<T>`: forms are
      deserialized with `serde_html_form` instead of `serde_urlencoded`
- [x] `#[derive(DbEnum)]`: text-backed enums for models, forms, JSON and templates (`ALL`,
      `as_str`, `Display`, `FromStr`, serde, `ToDbValue`, sqlx decoding on every enabled database
      through `__db_text_type!`, chosen when renox-core compiles)
- [x] `renox::db::Json<T>` fields (TEXT on SQLite, JSONB/JSON/TEXT on PostgreSQL) and
      `serde_json::Value` read back (sqlx `json` feature); `DbValue::Json`
- [x] UUID fields with renox's `uuid` feature (BLOB on SQLite, UUID on PostgreSQL); `DbValue::Uuid`
- [x] A field that doesn't parse as an enum gets the enum's first variant as its placeholder, so
      every other field's errors still show
- [x] `examples/fields`: one form with every input type, saved and shown back in the edit form,
      tested on SQLite and PostgreSQL (in the PostgreSQL CI job); [docs/types.md](docs/types.md)
      maps HTML input ↔ Rust type ↔ SQLite ↔ PostgreSQL and is compiled as a doctest
- Decimals: not added. sqlx deliberately has no decimal type on SQLite, so money stays `i64` in
  the smallest unit, as the guide explains

### M13 · v0.14: Hardening from the pre-1.0 audit
The audit in [docs/audit/2026-09-pre-1.0.md](docs/audit/2026-09-pre-1.0.md) probed negative
flows and injected faults into running apps. IDs below refer to it. Each fix lands with its probe
(on the local `probe-web` / `probe-data` branches) moved to main as a passing regression test.

M13a · web security:
- [x] W1 Uploads can't become active content: html/xml/js/php/… extensions are stored as
      `.txt`; `/storage` and `public/` answer with `CSP: sandbox`, `nosniff`, and `attachment`
      for HTML/XML/JavaScript (an SVG stays an image, sandboxed)
- [x] W18 / A5 `TRUSTED_PROXIES` (addresses, CIDR ranges or `*`) and one `ClientIp` extractor
      (X-Forwarded-For / Forwarded, walked from the right) used by throttles, the login lock
      and request logs
- [x] W2 Logging out ends every session of the user (`users.sessions_revoked_at`, checked on
      each request; `auth::logout` is now async and takes the `Db`)
- [x] W3 Only a valid Bearer token skips CSRF; a wrong one answers 401
- [x] W4 Multipart CSRF / `_method` fields read up to `UPLOAD_MAX_SIZE` (with `multer`)
- [x] W5 Old input kept under 2 KB; password and `_` fields never flashed
- [x] W6, W7 Redirects (`Back`, after validation, intended URL) stay on the app's origin
- [x] W8, W9 Login lock per email+IP, per account and per IP; password reset revokes API tokens
- [x] W10–W17 Maintenance cookie as an HMAC with `Secure`; security headers on 413; empty files
      fail type rules; long paths 404; unique races 422; finite floats only; 415 for unknown
      bodies; normalized emails on login and reset
- [x] The web probes are in main as `crates/renox/tests/it/web_security.rs`

M13b · data and background resilience:
- [x] D1, D3, D9 Panics are contained: a job runs in its own task, so a panic is a failed attempt
      and the worker keeps going (a webhook handler's panic marks its call failed); a task panic
      doesn't stop its schedule (a guard clears `running`); a handler panic answers 500 with the
      error page; one listener's panic doesn't skip the others
- [x] D2 Only jobs with attempts left are reserved; a job whose last attempt never finished
      (crash, kill, OOM) goes to `failed_jobs` (checked at most once a minute)
- [x] D4, D22 Timeouts in `.env`: `DATABASE_ACQUIRE_TIMEOUT` (5 s), `DATABASE_STATEMENT_TIMEOUT`
      (30 s, PostgreSQL), `REQUEST_TIMEOUT` (60 s, a 500), `MAIL_TIMEOUT` (10 s, the whole send)
- [x] D5 `DATABASE_URL` accepts only `sqlite:`, `postgres://` and `postgresql://` (the error
      hides the password)
- [x] D6, D7 A job's outcome is written with retries (about 10 s) so a database blip doesn't
      strand it; a job whose `TIMEOUT` is longer than the 15-minute reservation keeps its
      reservation for `TIMEOUT` + 1 min (instead of refusing such jobs)
- [x] D8 `state.queue.dispatch_in(&mut tx, job)`: the job exists only if the transaction
      commits; workers see it within a second. Plain `dispatch` while SQLite's one write
      transaction is open fails with "database is locked" (documented)
- [x] D10, D15, D16, D17 `unique`/`exists` refuse an unknown column on SQLite (checked with
      `pragma_table_info`, instead of turning SQLite's double-quoted-string fallback off, which
      could break apps' own SQL); text input is compared as text on PostgreSQL; `where_in`
      with over 1,000 ints or strings sends one JSON array; `limit`/`offset` clamp to `i64::MAX`
- [x] D11, D12, D13, D26, D27 Migrations: `fresh` on PostgreSQL drops types, sequences,
      functions and materialized views too; runs take turns (an in-process lock, an advisory
      lock on PostgreSQL, `BEGIN IMMEDIATE` plus a re-check per migration on SQLite); rollback
      checks every `down` before undoing anything and forgets unregistered migrations with a
      warning; `-- renox:no-transaction`, `CONCURRENTLY` or an own `BEGIN` run without the
      wrapper; checksums flag edited migrations, and `migrate:status` lists missing ones
- [x] D14 Webhook payloads are stored as bytes (BLOB/BYTEA; `WebhookCall::payload: Vec<u8>`,
      `text()`); event ids over 200 bytes are stored as `sha256:<hash>`
- [x] D18, D21, D23 A unique violation answers 409; `Error::permanent` (payloads that no longer
      decode, invalid mail addresses) skips the retries; notifications write the database row
      before sending mail
- [x] D19, D20, D24, D25, D28, D29, D30 Saturating signed-URL expiry (S3 presigns for at most
      7 days); `DATABASE_POOL_SIZE=0`, a huge `UPLOAD_MAX_SIZE` and a path defined by two
      modules are boot errors; cache: TTL 0 stores nothing, `remember` computes once per key in
      a process, a wrong type is an error (`remember` recomputes), `flush` keeps `renox:` rows;
      scheduler claims last the interval + 1 min and are pruned once a minute; the CLI refuses
      Rust keywords and `renox`, quotes table names, rejects `create__table`, keeps `export
      APP_KEY=`; `assert_database_has` is `Send`; storage errors name the path, and an invalid
      key is a 400
- [x] The data probes are in main as `tests/it/data_resilience.rs` and
      `tests/it/background_resilience.rs`

M13c · keep it that way:
- [x] Every probe passes on main (web, data, background), on SQLite and PostgreSQL: they run in
      the `test` and `test (PostgreSQL)` CI jobs as `tests/it/web_security.rs`,
      `data_resilience.rs` and `background_resilience.rs`
- [x] A chaos job in CI (`chaos (sqlite)`, `chaos (postgres)`): `tests/chaos/run.sh` runs the
      `tests/chaos` app and checks, with time limits, that PostgreSQL stopped, paused (also during
      a request) and restarted gives fast 500s and a 503 `/health`, and that the app and workers
      recover without a restart; that SQLite held locked keeps reads up and strands no job; and
      that panics in handlers, listeners, jobs and scheduled tasks are contained
- [x] [docs/operations.md](docs/operations.md): timeouts, proxies, `/health`, failure behaviour,
      failed jobs and webhook calls, backups, deploys and migrations, maintenance mode, logs

### M14 · v0.15: API freeze
What would be a breaking change after 1.0, settled now (IDs from the audit), in three PRs.

M14a · API foundations:
- [x] A1 `#[non_exhaustive]` on the public structs and enums that will grow (list in
      [docs/stability.md](docs/stability.md)); `Dialect` stays exhaustive on purpose
- [x] A2 [docs/stability.md](docs/stability.md): semver scope and public dependencies. axum,
      tower(-http), minijinja, tokio, serde, chrono and fake are public (Renox's major follows
      theirs); sqlx is not: queries fail with `db::DbError`, and sqlx is reachable only through
      escape hatches (`Db::sqlite()`, `renox::db::sqlx`, …)
- [x] A3 `Error::Status(code, message)` and `abort`, `abort_if`, `abort_unless` (in the prelude)
- [x] A10 `Routes::group(path_prefix, name_prefix, routes)`
- [x] A9 `App::command` / `Registry::command` with `command::Args`, listed in `help`,
      `Kernel::call` for tests; `rnx make:command`; `examples/hello` has `entries:prune`
- [x] `rnx new` pins Renox to the commit `rnx` was built from (`rev = …`, also through
      `cargo install --git`), falling back to `branch = "main"` without git

M14b · extension points:
- [x] A8 `App::templates(|env| …)` for filters, functions and globals, plus built-in `number`
      (locale separators) and `date` (chrono format, `APP_TIMEZONE`) filters; `App::share(key,
      async fn(ViewContext))` for data every view gets (the handler's context wins); typed values
      with `App::provide(value)`, the `Provided<T>` extractor and `state.provided::<T>()`;
      `App::layer(..)` around the app's own routes (after the session and user are loaded)
- [x] A4 The app's own `users` columns are kept on `User` (`user.get::<T>("role")`,
      `user.set(&db, "role", "admin")`, `User::where_eq("role", …)`, `{{ auth.user.role }}`);
      `Auth::registration_rules` and `Auth::on_registered` for extra registration fields (a
      failing hook undoes the sign-up); `App::gate_async` with `auth.gate_async(..).await?`
- [x] Templates are semi-strict while `APP_DEBUG` is on (printing a missing variable fails;
      `if` and `flash.x` don't); the debug error page shows the request, the error chain and the
      template with its line, also when a view fails to render (it used to fall back to a bare page)

M14c · mail and notifications:
- [x] A6 `Mail` with several recipients (`to: Vec<String>`, `.also_to`), `.cc`, `.bcc`,
      `.reply_to`, `.from` (instead of `MAIL_FROM_*`) and `.attach(name, type, bytes)`
      (base64 in the queue); invalid addresses fail permanently; the preview page lists them all;
      `Mail::is_for(address)`
- [x] A7 `Channel::Custom(name)` with `App::channel(name, handler)` and
      `Notification::to_channel`; `Recipient` for users or plain addresses
      (`Recipient::to("mail", …).and("whatsapp", …)`) with `state.notify_to`; `state.notify_later`
      (database row now, each other channel as its own queued job with retries)
- [x] The cheat-sheet, README, docs and examples follow the frozen API (`Notification` methods
      take a `&Recipient`)

### M15 · v0.16: The data layer
The biggest day-to-day gap for developers coming from Laravel (see "Readiness vs Laravel" in the
audit). Explicit, typed, no magic; settled before the 1.0 freeze. Two PRs.

M15a · queries and relations:
- [x] Query builder: `where_any` / `where_all` (nested OR / AND groups), `where_between`,
      `where_not_in`, `when()`, `where_in_query` (a sub-query instead of a join),
      `sum::<i64|f64>` / `avg` / `min` / `max`, `pluck`, bulk `update` (sets `updated_at`),
      `increment`, `first_or_404`, `first_or_create` (race-safe with a unique index), `chunk` (by
      id); `Model::find_many`, `insert_many` and `upsert` (chunked under the bind limit). A
      column-picking `select` isn't offered on model queries (a model needs its columns); joins
      and projections use `fetch_as`
- [x] `sql(..).fetch_as::<T>()` / `fetch_one_as` / `fetch_optional_as` with the new `FromRow`
      trait: `#[derive(FromRow)]` (`rename`, `skip`), models (`Model: FromRow`) and tuples
- [x] Relations as explicit helpers (`db::relations`): `belongs_to`, `has_many` (with the
      children's query for order and filters), `Pivot` with `attach` / `detach` / `sync` / `ids` /
      `load` / `load_for` / `inverse`; each loads a page's related rows in one query;
      [docs/relations.md](docs/relations.md) (compiled as a doctest)

M15b · validation and requests:
- [x] Validation: `matches` (regex, cached), `digits`, `digits_between`, `date`, `before` /
      `after` / `before_or_equal` / `after_or_equal` (text, `NaiveDate`, `NaiveDateTime`,
      `DateTime`), `none_of` (Laravel's `not_in`; `in` is `one_of`), `required_if` /
      `required_unless` / `required_with`, `same` / `different`, `Validator::each` for every
      item of a list (errors on `name.0`, …), `Validator::nested` for a list of structs (errors on
      `name.0.field`), the `Rule` trait with `.apply(&rule)`; English and Indonesian messages
- [x] Requests and responses: `Vec<Upload>` for `<input type="file" multiple>` (checked per file
      with `each`), `Cookies` / `SetCookie` (plain or encrypted with `APP_KEY`, safe defaults),
      `Download` (bytes, a streamed file, a `Storage` key, any stream; `inline()`, but never for
      HTML/XML/JS; UTF-8 file names)
- [x] Around them: `Htmx::redirect(to)` (HX-Redirect or 303); list-item errors (`photos.1`) show at
      the list's input and slot (renox.js) and in `error('photos')`; `examples/uploads` takes several
      photos at once through htmx and shows invoices with `Download`

### M16 · v0.17: Developer experience and trust
Two PRs.

M16a · lighter builds and several servers:
- [x] Lighter builds: reqwest uses rustls with the `ring` provider (as lettre does), so
      `aws-lc-sys` (C, CMake) is no longer built; new default features `fake` (the `renox::fake`
      re-export) and `server-events` (`analytics::ServerEvent` and its HTTP client) can be turned
      off; `rnx new` sets `debug = "line-tables-only"` for dev builds;
      [docs/development.md](docs/development.md) (linker, features, sccache, Docker)
- [x] Cache-busting `asset()`: `/app.css?v=<content hash>` (hashed at boot for embedded files, by
      modified time from disk), and versioned URLs are cached for a year (`immutable`)
- [x] `make:deploy` Dockerfile with dependency caching (cargo-chef)
- [x] With `CACHE_STORE=database`, `Routes::throttle` limits and the login lock are counted in the
      `cache` table (`renox:count:…`), so several servers share them; a "Several servers" section
      in docs/operations.md

M16b · CI and trust:
- [x] CI: `rnx new` → every `make:*` → `cargo build` and `cargo test` on the result (SQLite and
      PostgreSQL, `tests/cli/run.sh`); compile-fail tests for the macros; a feature matrix
      (cargo-hack) including a SQLite-only build; S3 against a real S3 server; MSRV; cargo-deny;
      coverage; a Docker build of `make:deploy`, started and checked on `/health`
- [x] Direct tests for APIs covered only indirectly (session `pull`/`reflash`/`set_lifetime`,
      `HxRedirect`/`HxRefresh`, `Validator::rule`, `fetch_optional`/`bind_all`, schedule
      constructors and offsets, signed URL tampering and expiry, plural 0, `Config::load`)
- [x] SECURITY.md, CONTRIBUTING.md, CHANGELOG.md

Deviations in M16b:
- The macros' compile errors are `compile_fail` doctests, not trybuild: the same check without
  another dev-dependency or `.stderr` snapshots that change with every Rust release.
- S3 runs against SeaweedFS: MinIO no longer publishes Docker images.
- Semver checks (cargo-semver-checks) move to v1.0: before a first release there's no published
  baseline to compare with, and 0.x allows breaking changes anyway.
- The job found two generator gaps and one bug, fixed here: `make:job`/`make:command` now register
  what they create, migrations made in the same second keep their order, and S3 over `http://`
  (a local MinIO/SeaweedFS) failed on every request.

### M17 · v0.18: Examples of real apps
- [x] `examples/shop`: auth with gates and policies, an admin with search, sort and pagination,
      uploads, checkout in a transaction, cache, mail and notifications, queue, i18n with
      plurals, SEO, and its `make:deploy` output (M17a)
- [x] `examples/htmx-recipes`: inline edit (`hx-patch`), infinite scroll, modal forms, delete
      with `HxRefresh`/`HxRedirect`, Alpine dropdown/tabs/modal
- [x] `examples/relations`: one-to-many and many-to-many with joins and eager loading
- [x] A README for every example (M17b)

Notes from M17a:
- Split in two: M17a is `examples/shop` (and its README); M17b the other examples and READMEs.
- The shop exposed a framework bug, fixed there: `relations::belongs_to`, `has_many`,
  `Pivot::load` and `Query::first_or_create` compiled in doctests but not in a routed handler
  (their futures held a closure or a generic iterator across an `.await`, which fails axum's
  `Send` check; rustc issue #100013). `it/send_handlers.rs` now routes every data API.
- Admin routes used an `Admin` extractor that checks the gate (replaced by `require_gate` in
  M18a). A route-level
  `require_gate("admin")` (like `require_auth`) would also show in `route:list`; it's a
  candidate for later.

Notes from M17b:
- READMEs of the eight older examples were written by reading their code; two things they
  turned up were fixed: `rnx key:generate` now creates `.env` (from `.env.example`) when it's
  missing, and examples/api's `DELETE /api/tokens/current` revoked every token; there is now
  `AuthUser::token_id()`, so it revokes only the one used, and `DELETE /api/tokens` all.

### M18 · v0.19: SaaS foundations

From the Laravel parity review of September (after M17; brought up to date after M32 as
docs/audit/2026-10-laravel-parity.md). What a typical SaaS needs
before it can start: tenants, roles, accounts.

- [x] Tenancy: a default scope on models (`#[model(default_scope = "…")]` naming a
      `fn(Query<Self>) -> Query<Self>`, bypassed with `Model::unscoped()`), e.g. filtering by a
      task-local current team
- [x] Scoped `unique`/`exists`: `.unique("products", "sku").ignore(id).where_eq("team_id", t)`,
      `.where_null("deleted_at")`
- [x] Roles and permissions (opt-in module): tables, `user.has_role`, `user.has_permission`,
      `Routes::require_role` / `require_permission`, permissions usable as gates
- [x] `Routes::require_gate("admin")` (async gates too, shown in `route:list`) and
      `App::gate_before(|user, ability| -> Option<bool>)` for super-admins
- [x] Account pages in `Auth`: profile (name, email with re-verification), password (checks the
      current one and keeps this session logged in), delete account; `auth::change_password`
- [x] Password rules (`Password::min(12).mixed_case().numbers().symbols()`) used by
      register/reset/change; `require_password_confirmed`
- [x] Optional breached-password check (HIBP), once the HTTP client (M20c) exists: done in
      M34 as `Password::uncompromised()`
- [x] Auth events (`Registered`, `LoggedIn`, `LoginFailed`, `LockedOut`, `LoggedOut`,
      `PasswordReset`, `EmailVerified`) and an opt-in audit log (the `Audit` module,
      `audit::record(&db, Entry::new(action).user(id).subject(table, id))`)
- [x] API token abilities (`create_token_with(.., &["orders:read"], ..)`, `token_can`,
      `Routes::require_ability`), expired-token pruning
- [x] Log out this device only; log out other devices (keeps this one)
- [x] Accept bcrypt hashes from imported Laravel users and rehash to Argon2id at login

Notes from M18a (tenancy, roles, gates, token abilities):
- `renox::context`: every request, job, scheduled task and app command runs in its own
  task-local context, so a default scope can read the current tenant without it being passed
  around. `tokio::spawn` starts without one (`context::scope`).
- A default scope is a plain function named in `#[model(default_scope = "…")]`; `unscoped()`
  skips it and `none()` fails closed. Saving and deleting a loaded model work by id.
- `gate_before` answers gates, permissions and policies (`authorize`, `Can::new` with an
  `AuthUser`), not role membership.
- Roles and permissions are loaded once per request (two queries) only when the `Permissions`
  module is registered. `require_role/permission/gate/ability` show in `route:list`.
- Remaining M18 items move to M18b: account pages, password rules and confirmation, auth events
  and audit log, per-device logout, bcrypt import.

Notes from M18b (accounts and security):
- Per-device logout keeps sessions in cookies: each login gets a random id, and `logout` puts it
  on a short denylist (`revoked_sessions`, kept until a copy of the cookie would expire). "Log
  out other devices" reuses the `sessions_revoked_at` cut-off and logs this session in after it.
  `logout` used to end every session of the user; now it ends this device only.
- Password policy: `Password::min(n).letters().mixed_case().numbers().symbols()`, used by the
  built-in register, reset and account forms through `Auth::password_rules`. The optional
  breached-password check (HIBP) is left out: it needs an HTTP client, which M20 adds.
- The account page is opt-in (`Auth::account()`) so existing apps' routes don't clash; new apps
  from `rnx new` turn it on and link it from the layout.
- Password confirmation lasts three hours, like Laravel; a login through the form counts.
- Auth events are emitted by the built-in pages only; a listener's failure is logged and never
  fails the login. The `Audit` module records them all and gives apps `audit::record`.

### M19 · v0.20: Data layer 2
- [x] Raw fragments in the builder: `where_raw`, `order_by_raw`, `select_as` +
      `group_by`/`having_raw` read into `FromRow`; `to_sql()` for debugging
- [x] `lock_for_update()` / `shared_lock()` (PostgreSQL), a public `begin_immediate` (SQLite)
- [x] Aggregate loaders: `relations::count_many`, `sum_many` (one GROUP BY; "has any" is a
      count above zero, or `where_has` to filter)
- [x] `where_has` / `where_doesnt_have` (EXISTS), `where_not_in_query`
- [x] Non-integer keys (M22): the `id` field's type is the key (`i64`, `Ulid`, `Uuid`, `String`),
      loaders generic over it
- [x] Model hooks: `saving`/`saved`/`deleting`/`deleted` trait methods called by `save`/`delete`
- [x] Partial saves: `save_only(&["price"])` and change tracking against the loaded row
- [x] Pivot data and timestamps (`attach_with`, `load_with_pivot::<T, PivotRow>`); polymorphic
      relations (`Morph`)
- [x] `simple_paginate` and `cursor_paginate`; `update_or_create`, `first_or_new`, `refresh`
- [x] `db.transaction(|tx| …)` and `db.transaction_retrying(3, |tx| …)` (SQLite busy,
      PostgreSQL serialization)
- [x] Savepoints (nested transactions): `tx.savepoint(|tx| …)` (M23)
- [x] A public encrypt/decrypt API (`state.encrypt` / `state.decrypt`, AES-GCM under `APP_KEY`)
- [x] `Encrypted<T>` field type (M23; the key travels with the `Db`, see the notes from M23)

Notes from M19a (query builder):
- M19 is split: M19a is the query builder (above, ticked); M19b the model features (non-integer
  keys, hooks, partial saves, pivot data, polymorphic relations, `Encrypted<T>`).
- Transactions take a closure returning `Box::pin(async move { … })` (the usual way to lend a
  `&mut Transaction` to async code); `transaction_retrying(n, …)` retries on `DbError::
  is_retryable` (SQLite busy/locked, PostgreSQL 40001/40P01). Savepoints are not done.
- `cursor_paginate` is keyset on `id`, newest first; other orders use `paginate`.
- `where_raw`/`order_by_raw`/`having_raw`/`select_as` take SQL as written: identifiers are the
  app's to quote, and values go through `?`.

Notes from M19b (model features):
- Hooks are opt-in, `#[model(hooks)]` + `impl ModelHooks`, so a model without them pays
  nothing and one with them can't forget the attribute silently (the default methods on `Model`
  are no-ops). `saving`/`deleting` are sync and return `Result` (they check and fill fields);
  `saved`/`deleted` are async. Bulk `Query::update`/`delete`, `insert_many`, `upsert` and
  `restore` skip them, like Laravel's mass updates.
- `renox::context::app()` returns the `AppState` in a request, job, scheduled task and app
  command, so a hook can reach the cache, events or mail. Outside those (a test calling models
  directly) it's `None`, so hooks must not rely on it for correctness.
- `save_changes` runs `saving` first and then diffs against the original, so columns a hook
  fills (a slug) are saved; `save_only` saves only the listed columns plus `updated_at`.
- Polymorphic relations store `P::TABLE` as the type (no morph map; renaming a table means an
  UPDATE of the type column). `Morph::parents` loads one parent type per call.
- Pivot data goes in as `&[(&str, &dyn ToDbValue)]` and comes out as a `FromRow` struct read
  from `SELECT *` on the pivot table.
- **Deferred: non-integer keys.** `Model::id() -> i64`, `find_many`, every loader's
  `HashMap<i64, _>`, `ForeignKey`, route model lookups and the `User` model all assume an
  integer id, and 0 means "not saved". A key type parameter would touch every one and every
  app's code; it wants its own milestone with a migration guide. Until then a UUID column with a
  unique index next to the integer id (`where_eq("uuid", …)`) covers public ids. (Done in
  M22, with the key type read from the `id` field.)
- **Deferred: `Encrypted<T>`.** Encoding a field needs the key, and sqlx's `Encode`/`Decode`
  get no context; reading it from `renox::context` would make `save` fail (or store plain text)
  in code without an app context, such as tests and seeders. `state.encrypt`/`decrypt` cover
  the need explicitly; a field type comes back if the key can be made reachable everywhere.
  (Done in M23: the `Db` carries the key, so no context is needed.)

### M20 · v0.21: Background 2
- [x] Schedules: `cron("0 9 * * 1-5")`, `weekly_on`, `monthly_on`, `weekdays`, `between`;
      IANA time zones with DST (`APP_TIMEZONE=Asia/Jakarta`, per-task `timezone`)
- [x] Schedule hooks: `on_failure`, `schedule:run NAME` (`on_success` too)
- [x] Schedule pings: `ping_before`/`then_ping` (health checks), with the HTTP client below
- [x] Atomic locks: `state.cache.lock(key, ttl)`, `.block(wait)`
- [x] Unique jobs (`UNIQUE_FOR`, `unique_id`), job middleware (rate limited, without
      overlapping), chains and batches with progress
- [x] Queue priority (`--queue high,default` drains in order), `dispatch_sync`, a `failed` hook,
      `queue:forget`, `queue:prune-failed`, encrypted payloads
- [x] A queue dashboard (`/_renox/queue`, gated) with pending, failed, throughput and wait time
- [x] Localized mail and notifications (`t()` in mail templates, a recipient's locale),
      per-recipient channels, mail components (panel, table)
- [x] An HTTP client for apps (`renox::http`: timeouts, retries) with `TestApp::fake_http`
- [x] Cache `add`/`pull`/`increment`, pruning expired rows of the database store
- [x] Storage listing/copy/move

Notes from M20a (scheduler, locks, cache):
- M20 is split: M20a is the scheduler, locks and cache (ticked above); M20b the queue (unique
  jobs, middleware, chains/batches, priority, `dispatch_sync`, failed hook, prune/forget,
  encrypted payloads); M20c the dashboard, localized mail, the HTTP client (and schedule pings
  on it) and storage listing.
- `renox::timezone::Zone` is UTC, a fixed offset or an IANA zone (`chrono-tz`). Cron-style
  tasks (`cron`, `daily_at`, `weekly_on`, `monthly_on`) follow the wall clock: a time skipped in
  spring runs right after the jump, a repeated one runs once. Intervals (`every*`, `hourly`)
  follow the current offset, so an hour of every-minute runs isn't lost in autumn.
- The cron parser is Renox's own (5 fields, names, ranges, lists, steps, `@daily`…; day of
  month and day of week OR-ed when both are set, as in every cron). An expression that never
  matches, a bad zone or `between` time, and a duplicate task name are boot errors.
- Adding a task returns `ScheduledTask`, which derefs to the `Schedule` so chains of adds keep
  compiling. `Schedule::upcoming` now takes a `Zone` and returns the zone too.
- Locks are `renox:lock:*` cache rows holding a random owner: `add` takes them, the owner's
  `DELETE … WHERE value = ?` releases them, a dropped guard releases in the background, and the
  ttl bounds a crashed holder. With `CACHE_STORE=memory` they only span one process.
- The database store deletes expired rows at most once an hour per process as it writes, and
  `cache:prune` does it on demand.

Notes from M20b (queue):
- One migration (`00010101000110_add_chains_and_batches_to_jobs`): `chain` and `batch_id`
  columns on `jobs` and `failed_jobs`, and a `job_batches` table. Apps get it with `migrate`.
- A chain is a JSON array of the remaining jobs carried by the running one; the worker queues
  the next in the same transaction that deletes the finished job. A failure keeps the rest in
  `failed_jobs.chain`, so `queue:retry` resumes it.
- A batch's counters (`pending`, `failed`) change in the transaction that finishes a job.
  `then`/`catch`/`finally` are jobs stored with the batch (closures can't be stored); the first
  failure cancels the batch unless `allow_failures()`, and jobs of a cancelled batch are
  skipped (counted, not run). Retrying a batch job puts it back in the counts.
- Unique jobs claim `renox:unique:NAME:ID` in the `cache` table whatever `CACHE_STORE` is (like
  schedule claims), holding the job id so a second dispatch returns it; the claim goes when the
  job finishes for good or its `UNIQUE_FOR` runs out. Chains and batches don't check it.
- Encrypted payloads are `enc:` + the `state.encrypt` format; JSON never starts with `enc:`.
  A payload that doesn't decrypt (another `APP_KEY`) fails for good with that reason.
- Middleware uses the cache store: with `memory`, `without_overlapping` and `rate_limited` hold
  per process. A held-back job goes back with its attempt uncounted.
- `dispatch_sync` runs `handle` only: no retries, middleware or `failed` hook.
- `Batch::push` (not `add`, which clippy reads as `Add::add`).

Notes from M20c (dashboard, mail, HTTP, storage):
- `renox::http` wraps reqwest (one client per process, ring for TLS) behind a new default
  feature `http`; `server-events` now depends on it and GA4 events go through `state.http`,
  so they're faked in tests too. Responses are read whole (no streaming yet). The fake answers
  by glob pattern (`*`, optional method), in turn with the last answer repeating, and records
  every request; a request with no fake is an error rather than a real call.
- Schedule pings are GETs with a 10 s timeout and one retry; a failing ping is logged only.
- The dashboard is a module (`renox::queue::Dashboard`) behind the `view-queue-dashboard` gate,
  with no exception for local development: define the gate. It polls itself every 5 s with htmx
  (`hx-select`). Throughput comes from per-minute `renox:queue:done|failed:*` counters in the
  `cache` table, written in the worker's finishing transaction and kept two hours.
- The current locale is, in order: the recipient's while a notification builds its messages
  (a thread-local, since `to_mail` is synchronous), the request's (now also in
  `renox::context`), or `APP_LOCALE`. `Recipient::locale()` reads `in_locale(..)` or a
  `locale` column on `users` if the app added one. `to_channel` gets no state, so channel
  messages read `to.locale()` themselves.
- Storage `rename` is Laravel's `move` (`move` is a Rust keyword). `delete_all("")` is refused.
- M20 is complete. M21 (views and developer experience) is next.

### M21 · v0.22: Views and developer experience
- [x] Components that see the request (`old`, `error`, `t`, `csrf_field`, `can`, `auth` inside
      imported macros), the `renox/ui.html` kit (input, textarea, select, checkbox, button,
      card, group, alert, badge, form_errors, sheet, confirm, menu, tabs, table, empty) and
      `make:component`
- [x] Toasts over htmx; `once`; several fragments and out-of-band swaps;
      `HxRetarget`/`HxReswap`/`HxPushUrl`
- [x] Error pages rendered in the app layout (M21d)
- [x] `push`/`stack` (M21e)
- [x] Live validation over htmx (validate one field without running the handler)
- [x] Tailwind with its standalone CLI in `rnx serve` / `rnx build`, `rnx new --tailwind` (M21e)
- [x] Resource scaffolding: `Routes::resource`, `rnx make:module --resource` (handlers, views,
      tests); `make:factory`, `make:seeder`, `make:test`, `make:notification`, `make:event`,
      `make:rule`, `make:middleware`
- [x] Tests: `assert_json_path`/`assert_json`, session/auth/view assertions, time travel,
      event and notification fakes, a browser-test recipe
- [x] Errors and logs: `App::report(…)` (e.g. Sentry), `LOG_FORMAT=json`, log files, a request id;
      a debug inspector (`/_renox/debug`: requests, views and queries; mail stays at
      `/_renox/mail`, jobs on the queue dashboard)
- [x] `Path` rejections as 404 (M21a)
- [x] Named, dynamic rate limiters; `route()` with query parameters (M21d)
- [x] More validation rules and form-request hooks (`authorize`, `prepare`, `after`, async
      rules) (M21f)
- [x] Typed app commands (a clap parser), prompts (M21e)
- [x] Zero-downtime deploy recipes; an opt-in server-side session store (M21g)
- [x] Deferred from M21 (small items the Laravel parity review planned here), built in M24:
      subdomain and fallback routes (`Routes::domain`, `Routes::fallback`); the current route's
      name in views (`route_is`, `request.route`, `CurrentRoute`); `Redirect::route` and
      `Redirect::intended`; session `push`/`increment`; factory states and sequences
      (`Factory::factory()`); plural ranges; MiniJinja `loop_controls` and `class_names`.
      The locale from `Accept-Language` moved to D (with `#[derive(Validate)]`)
- [x] Renox's own auth pages (`renox/auth/*`: login, register, reset, verify) on the UI kit:
      they still have their pre-kit look (a black button, default links) in apps built on the kit
      (found in the M21e browser check) (M21f)
- [x] Authorization gaps found while moving examples/shop to `Permissions` (#53): policies
      and `gate_before` see a plain `User` without its roles (`Policy::allows` can't say
      "admins see all", a super-admin can't be a role), and there is no loader for the users
      with a role (`permissions::users_with_role`)
- [x] Rough edges found while writing examples/teams, jobs, crud, relations and shop (#55):
      - after `/confirm-password`, only a GET is remembered as the page to return to, so a
        guarded DELETE/PUT lands on `/` (`auth/account.rs`); `TestApp` has no way to mark the
        password as confirmed
      - `transaction_retrying` closures can't borrow from the caller and can't roll back with
        a value (examples/shop downcasts an `Error::Internal`)
      - a batch's `then`/`catch`/`finally` jobs get no `batch_id`, so `finally` can't read the
        batch's status; `TestApp` can't run jobs still waiting for their backoff
      - `Error::permanent` needs an `anyhow`-compatible error and `anyhow` isn't re-exported
      - no query counter in `TestApp` (examples/relations counts sqlx tracing events)
      - `Morph` has no `count_many`; a `saving` hook's error loses the old input on plain forms
      - seeders get only a `Db` (no `state.encrypt`, config or `context::app()`); no public
        random-token helper; reading a context value in a handler needs a hand-written
        extractor

Notes from M21a (rough edges):
- M21 is split: M21a (this: the authorization gaps and rough edges from #53/#55, `Path` 404s);
  M21b views (components that see the request, a `renox/ui` kit, toasts, fragments and
  out-of-band swaps, live validation); M21c scaffolding (`Routes::resource`, generators,
  Tailwind); M21d errors, logs, a debug inspector, rate limiters, typed commands.
- `User::has_role` / `has_permission` read the grants the auth middleware put in
  `renox::context` for the logged-in user, so they're synchronous and work in `Policy::allows`
  and `gate_before`; for any other user, or outside a request, they're `false` (documented).
  `permissions::users_with_role`.
- After `/confirm-password`, a guarded POST/PUT/DELETE returns to the page the form was on
  (`same_site_referer`), a GET to itself. `TestApp::confirm_password()`.
- `Db::retrying(n, || async { … })`: the attempt opens and commits its own transaction, so it
  borrows from the caller and can roll back with a value; `transaction_retrying` stays for the
  closure-with-`tx` style. examples/shop's checkout uses it (no clones, no downcast).
- A batch's `then`/`catch`/`finally` jobs carry `callback_of` (a new framework migration,
  `00010101000120`) and see the batch in `JobContext::batch_id` without counting in it.
  `TestApp::run_all_jobs()` runs retries and delayed jobs too.
- `renox::anyhow` is re-exported; `Error::permanent_message`.
- `renox::db::capture_queries(fut)` records a future's statements through a task-local, so
  requests sent with `TestApp` count (spawned tasks and workers don't). examples/relations
  dropped its tracing subscriber for it.
- `Morph::count_many`; seeders run in the app's context (`context::app()`);
  `renox::random_token()`; `context::Current<T>` (and `Option<Current<T>>`) as a handler
  argument. examples/teams keeps its own extractor because it redirects to `/teams`.
- A `ValidationError` raised after `Valid` (a model's `saving` hook, the handler) refills the
  form with what `Valid` read (kept in the context as `SubmittedInput`).
- `renox::Path` replaces axum's in the prelude: the same tuple struct, a 404 for values that
  don't parse. Code naming `axum::extract::Path` keeps the old 400.
- Seen twice and not reproduced since: the crud example's tests failed to boot during a full
  `cargo test --workspace` right after a large rebuild (23 s, `testing.rs:67` "the app
  boots"); the boot error wasn't captured. If it recurs, capture the panic message.

Notes from M21b (views):
- Components: the request's globals (`old`, `error`, `errors`, `t`, `can`, `auth`, `request`,
  `flash`, `csrf_field`, `once`, `toasts`…) are also environment globals (`RequestGlobal`) that
  forward to the page being rendered, kept in a thread-local while it renders (rendering is
  synchronous). So an imported macro sees what the page sees; the page's own context still wins.
- The kit (`renox/ui.html`, `/_renox/ui-<hash>.css|js` via `renox_ui()`) follows Apple's HIG,
  with web adjustments for WCAG AA (accent #0071E3, a darker red and secondary label). Rules in
  docs/ui.md. The classes are `rx-*` only; no bare element is styled.
- Built-in texts `ui.*` (en, id) are the translator's last fallback, after the app's files.
- Toasts: `Toast` is a response part; the view middleware sends it in `HX-Trigger`
  (`renox:toast`) for htmx swaps, or keeps it in the session (`_toasts`) for the next page. An
  htmx redirect keeps it too.
- Live validation: `X-Renox-Validate: field` makes `Valid<T>` answer `{field, errors}` and stop,
  the handler never runs; the kit's script checks on blur, then on input while invalid.
- `ui:publish` (app command; `rnx make:component --ui`) copies the kit from renox-core, so the
  CLI crate carries no copy of its own.
- `push`/`stack` aren't done: a layout renders its head before a child's blocks run, so a
  stack would need a two-pass render. Error pages in the app layout move to M21d.
- Browser-checked on examples/crud (desktop, 390 px, dark), which caught a sheet inheriting a
  table cell's alignment, red row buttons out-shouting the primary action, a menu too
  transparent over a button, and a table 8–21 px too wide on phones (actions now stack).

Notes from M21c (scaffolding and tests):
- `Routes::resource(path, name, Resource::new().index(..)…)` registers only the actions given,
  with Laravel's names; create is at `/{path}/new` (as in examples/crud); update answers PUT
  and PATCH.
- `rnx make:module <plural> --resource --fields "…"` (types string, text, int, money, float,
  bool, date; `--model` when the singular guess is wrong) writes the model with a factory, a
  migration for the app's database, a validated form, the seven handlers behind `require_auth`
  with toasts, UI-kit views (list with a confirmation sheet, a form with live validation, a
  grouped details page) and HTTP tests. tests/cli/run.sh generates two resources and runs their
  tests in CI. `rnx new`'s layout now uses the kit (navigation bar, account menu, toasts).
- `make:seeder` and `make:middleware` add `mod seeders;` / `mod middleware;` and register
  `.seeder(…)` / `.layer(from_fn(…))` next to the modules.
- Time: `renox::db::now()`, sessions, signed URLs, the queue, the cache and password
  confirmation read one clock (`clock.rs`) with a task-local offset; `TestApp::travel` sets it
  around its requests and job runs. Code spawned elsewhere doesn't see it.
- Fakes live in `AppState::fakes`; faked events skip their listeners and faked notifications
  skip every channel (mail, database, custom).
- `TestResponse` gained a public `view` field (the rendered template).
- Tailwind moves to M21d with the rest of the tooling.

Notes from M21d (errors, logs, debugging):
- Tailwind, `push`/`stack`, typed commands and prompts, the server-side session store,
  zero-downtime recipes and the validation additions move to M21e, so this PR stays about
  errors, logs and debugging.
- Request id: the `request_id` layer runs outside `TraceLayer` so the span has it; an incoming
  `X-Request-Id` is kept only when it's 8–64 of `[A-Za-z0-9._-]` (no log injection), else a
  20-character random one is made.
- `LOG_FORMAT=json` uses tracing-subscriber's JSON layer with the current span (`span`) and no
  span list; `LOG_FILE` is opened in append mode behind a `Mutex<File>`; if it can't be opened,
  logs go to stdout with a warning on stderr.
- Reports: `report::send` spawns each reporter in `context::scope_app`, awaited in a second
  task so a panic is logged, not propagated. Request reports read `RequestInfo`, which the
  context middleware stores. A scheduled task's message is the first line of `{err:?}`
  (`Error` has no `Display`). A job's `source` is `name #id`. Only `Error::Internal` 500s are
  reported, not 4xx.
- Error pages: `render_error` tries `errors/{status}.html`, then `errors/default.html` with the
  page globals (`CurrentGlobals`, so imported macros see them too), then `renox/error.html`. If
  the app's page fails to render, Renox's page is shown and the failure logged. The context's
  debug request line was renamed `request_line`: `merge_maps` let the context's `None` hide the
  `request` global, which broke `request.path` in layouts (found by the test).
- Named limiters count in memory per process, or in the `cache` table (`named:{name}` keys)
  with `CACHE_STORE=database`. A `throttle:name` mark without its limiter fails at boot.
- The inspector records only while `APP_DEBUG` and `APP_ENV=local` (like live reload), outside
  the context layer so the session's and user's SQL count, skipping `/_renox/*`. It keeps 50
  requests and 200 statements each. Deviation: jobs and mail aren't repeated in it; the page
  links to `/_renox/mail`, and the queue dashboard covers jobs.
- JSON error responses (`page.json`) now keep the error response's headers; before, API
  clients never got `Retry-After` from `throttle`. Found by examples/api's new limiter test.
- `background::locks_let_one_holder_in` failed once under the full PostgreSQL run: the
  re-taken lock had a 1 s ttl and could expire before `is_held`. It now uses 30 s.

Notes from M21e (Tailwind, stacks, typed commands):
- M21e as planned held six items; it's split. M21e: Tailwind, `push`/`stack`, typed commands and
  prompts. M21f: the server-side session store, the validation additions and form-request
  hooks, zero-downtime deploy recipes.
- Tailwind: `rnx` pins v4.3.3 with the SHA-256 of each release asset
  (`crates/renox-cli/src/tailwind.rs`), downloads with the system `curl` (Windows 10+ has it)
  so the CLI gets no HTTP client, into `RNX_CACHE_DIR` / the platform cache; `TAILWIND_BIN`
  overrides it. An app "uses Tailwind" when `resources/css/app.css` exists. `serve` runs
  `--watch=always` as a child killed on exit; `build` minifies before `cargo build`. The
  output is committed because the `make:deploy` Dockerfile doesn't run Tailwind. To update:
  bump `VERSION` and copy the new `sha256sums.txt`.
- Stacks: `stack(name)` renders `<!--renox-stack:{nonce}:{name}-->`, a nonce per render so
  text that looks like a marker is left alone; `push`/`prepend` are functions used with
  `{% call %}` (MiniJinja passes `caller` as a kwarg to any callable) and collect into a
  thread-local `Scope` set around `render_view` and the app's error pages. Fragments and mails
  have no scope, so pushes there are dropped.
- Typed commands: `App::command` stays as it was. Making it generic over the argument type
  would break every unannotated closure (`|state, args| …`), so typed commands are a trait,
  `AppCommand: clap::Parser`, registered with `App::typed_command::<T>()`; the name and the
  help line come from clap. `--help` prints and succeeds; a parse error returns clap's message
  plus the usage. clap is a renox-core dependency (no default features) re-exported as
  `renox::clap`, so derives resolve through `use renox::clap;`. `rnx make:command` writes a
  typed command now.
- Prompts go to stderr; without a terminal they read lines from stdin, and at EOF a question
  with a default takes it, one without fails naming the question. `prompt::answering` feeds
  answers through a task-local. `secret` uses `rpassword` (new dependency, small).
- examples/shop's `shop:make-admin` is typed and asks for a missing email; examples/crud's
  form pushes a robots `noindex` into the head.

Notes from M21f (forms and the auth pages):
- M21f is split again: M21f the forms side (rules, form-request hooks, the auth pages on the
  kit); M21g the operations side (the server-side session store, zero-downtime recipes).
- Form-request hooks are default methods on `Validate` (non-breaking): `prepare(&mut self)`,
  `authorize(&self, &FormContext) -> impl Future<Output = Result<bool>> + Send` and
  `after(&self, &FormContext, &mut Errors)`. The defaults return `std::future::ready`, so they
  capture nothing and stay `Send` for any form; an app's `async fn` override needs the form to
  be `Sync` (plain data is). `Valid` runs parse → `prepare` → `authorize` (403) → rules →
  `after` (only when the rules passed) → live-validation answer or handler. `authorize` needs
  the input, so a form that doesn't parse gets its 422 before `authorize` runs.
  `FormContext` (`#[non_exhaustive]`) carries the state, user, method and path. "Async rules"
  are `after`: `rules` stays synchronous (its database checks are already deferred).
- New rules: `alpha`, `alpha_num`, `alpha_dash` (Unicode letters count), `lowercase`,
  `uppercase`, `starts_with`, `ends_with`, `uuid`, `ip`, `size`, `required_without`,
  `prohibited_if`, and `Validator::distinct` (trimmed, case-insensitive; the repeat gets the
  error). `none_of` already was Laravel's `not_in`.
- Auth pages: `renox/auth/layout.html` uses `renox_ui()`, the kit's `rx-auth*` classes (new in
  renox-ui.css), toasts and stacks; every form is a kit `card` with `input`s (`block` primary
  buttons, as on iOS sign-in screens). The account page's delete moved from `hx-confirm` (a
  browser dialog) to a kit sheet with the password and the filled red button; the three
  `password` fields on that page use the kit `input`'s new `id=` parameter.
- examples/teams' `MemberForm` is a form request: the owner check and the "nobody has this
  email" lookup moved out of the handler.

Notes from M21g (sessions and deploys):
- `SESSION_DRIVER=database`: the encrypted cookie holds `{"sid"}` instead of the payload
  (`Stored`, untagged); rows are keyed by `sha256(sid)`. A request loads its row and writes it
  when the payload's fingerprint (data, flash, token, lifetime) changed, when the id is new, or
  once a minute to move `expires_at`. `regenerate_token` (login) and `flush` (logout) set
  `rotate`: the old row is deleted and a new id issued. A whole-session cookie is still read in
  database mode, so switching drivers keeps everyone logged in, and `TestApp` writes those.
  Expired rows: a 1-in-50 request lottery spawns a prune; `session:prune` /
  `Session::prune_expired`. The table's migration (`00010101000210`) is always installed.
- Tests can't await the database from `TestApp`'s synchronous session helpers on a
  current-thread runtime, so with `APP_ENV=testing` the database driver keeps sessions in
  `AppState::session_mirror` (same code path, keyed the same). Tests that set another env use
  the real table (tests/it/sessions.rs does both).
- Found by the tests: a session given a new id (rotation, or a cookie converted from the
  cookie driver) whose content hadn't changed was only `UPDATE`d, so no row existed; a new id
  now always inserts.
- Zero downtime: `serve` takes a socket from systemd (`listenfd`, `LISTEN_FDS`) before binding
  its own; `make:deploy` writes `deploy/<app>.socket` and the README's recipe (plus expand /
  contract migrations and a two-copy Caddy setup). Measured with transient user units
  (`systemd-run --user --socket-property=…`) and back-to-back requests over three restarts:
  0 of 1,350 refused with the socket, 280 of 1,288 without.
- M21 is complete.

Notes from M21h (the examples on the kit):
- After the audit of docs and examples against M21 (#63), the examples that still had
  hand-written markup moved to the kit: shop, teams, and htmx-recipes (which keeps its
  hand-rolled Alpine modal, dropdown and tabs as the recipes, and points to the kit's versions).
  The tests, `hello`'s command and the stubs follow in M21i.
- Moving them found three framework bugs, fixed here: `add_trigger` put raw UTF-8 in
  `HX-Trigger` (`HeaderValue::from_str` failed and the toast was silently dropped: any
  non-ASCII toast over htmx), now `\u`-escaped JSON; toasts with `HX-Refresh` went to
  `HX-Trigger` and the reload lost them, now they go to the session like `HX-Redirect`; error
  pages lacked `App::share` values, which broke a layout using one under strict undefined
  (debug), now shares are computed for error pages too and a failing one is only logged.
- `background::locks_let_one_holder_in` flaked again on PostgreSQL under the full run, in its
  eight-waiter part (10 s each); it checks one-at-a-time, so waiters now wait up to 60 s.
- The shop's brown accent was checked for WCAG AA with the kit's formula: white on the button
  colour 7.8:1 (light) and 6.5:1 (dark), the link colour 7:1 or more on the page.

Notes from M21i (the examples' tests, hello's command, the stubs):
- The examples' tests fake time and side effects instead of rewriting rows or waiting:
  `app.travel` in jobs (backoff between attempts, a unique job's `UNIQUE_FOR`, the daily
  report), api (a token's 30 days, the guests' rate limit), hello (the prune command) and shop
  (unpaid orders cancelled by the scheduled task, run with `run_scheduled` on the moved
  clock); `fake_events` / `fake_notifications` in jobs and shop; `assert_view`,
  `assert_json_path`, `assert_json` in shop, jobs and api.
- examples/jobs reports errors with `App::report(post_to_chat)`: a line per error to
  `ERROR_WEBHOOK_URL` over `state.http`, tested with `fake_http` (a declined card).
- examples/hello's `entries:prune` is a typed command (clap) that asks before deleting
  (`renox::prompt::confirm`, `--force` skips it); the test answers with `prompt::answering`.
- examples/fields checks its colours with `v.each(.., |c| c.one_of(COLORS))` and
  `v.distinct`; errors are keyed `colors.1`.
- Stubs: `tests/home.rs` uses `assert_view` and time travel (a session past its lifetime);
  `env.stub` lists `APP_HOST`, `DATABASE_POOL_SIZE`, the session lifetimes and cookie, and the
  paths; the AGENTS stub lists all the guides and adds traps about the clock, fakes and
  `each` keys.
- Using `travel` in the examples found two framework bugs, fixed here: the in-memory rate
  limiters and the login lock timed their windows with `Instant`, so travel didn't reach
  them (a limited guest stayed limited); they now use `clock::Stamp`. And `TestApp`'s own
  session helpers (the CSRF token, `acting_as`, `assert_guest`) read the cookie on the real
  clock while the server read it on the moved one, so any form post after travelling past
  `SESSION_LIFETIME` got a 419; they now run on the travelled clock too.

### M22 · Model keys other than integers
The one pre-1.0 change the owner picked from the open items: it changes `Model`'s API, so it
comes before 1.0; savepoints and `Encrypted<T>` only add and can wait.
- [x] `Model::Key` (the `id` field's type, `ModelKey`: `i64`, `renox::db::Ulid`, `uuid::Uuid`
      with the `uuid` feature, `String`); `id()`/`set_id`/`find`/`find_many`/`find_or_404` use it
- [x] `Ulid`: 26-character Crockford text, monotonic within a millisecond, serde as text, a 404
      from `Path<Ulid>` when malformed; `Uuid` keys are v7 (time-ordered)
- [x] `Model::insert` (always an INSERT, with a set key or a new one); `create` inserts;
      `save` inserts when the key is unsaved, else updates (a `String` key must be set)
- [x] Relations generic over keys: `belongs_to`, `has_many`, `count_many`, `sum_many`,
      `ForeignKey<K>`, `Pivot<L = i64, R = i64>`, `Morph` (the parents' key)
- [x] `chunk` and `cursor_paginate` by any key; `renox::uuid` re-exported
- [x] `rnx make:model … --key integer|ulid|uuid|string` (model and migration)
- [x] examples/fields keys products by `Uuid` (its `public_id` workaround is gone)

Notes from M22:
- The ROADMAP asked for `#[model(key = "uuid")]`. Reading the key from the `id` field's type
  needs no attribute and can't disagree with the field; the column name stays `id`.
- Existing code keeps compiling: `id: i64` models, `Pivot` constants (`Pivot<i64, i64>` by
  default, so `sync(&db, id, [1, 2])` still infers `i64`), and the loaders. What breaks is
  generic code over models that assumed an `i64` id: add `M: Model<Key = i64>` (one change in
  examples/relations). `find(db, id)` takes `Self::Key`, not `impl Into<Self::Key>`: the
  latter broke inference for `find(db, row.try_get("user_id")?)` (in Renox's own tokens.rs).
- `create` now always inserts, so `Tag::create(db, Tag { id: 42, … })` keeps id 42 instead of
  updating row 42. `save` of a set key that isn't in the table is still a 404, never a silent
  insert: a `String`-keyed row goes in with `insert`/`create`.
- `ModelKey` is sealed (like `db::Number`), so it may grow without breaking apps; its
  `#[diagnostic::on_unimplemented]` note lists the key types when an `id` is an `f64`.
- ULIDs are made by Renox (no new dependency): 48-bit milliseconds from `clock` (so
  `TestApp::travel` reaches them) and 80 random bits, incremented within a millisecond so a
  process's ULIDs sort in order; `chunk`/`cursor_paginate` rely on that order.
- Not changed: `User` keeps `i64` ids; `audit_logs.subject_id`, `notifications` and
  `personal_access_tokens` refer to users or integer subjects (`audit::record` takes an `i64`).
  Audit subjects with other keys would need a text column; not asked for yet.
- The workspace dev profile uses `debug = "line-tables-only"` since a separate PR (#68) merged
  just before this milestone: full debug info ran a 15 GB machine out of memory during
  workspace builds.

### M23 · Savepoints and encrypted fields
The first of three follow-ups the owner asked for before 1.0 (B: this; C: the rest of M21's
deferred items; D: `#[derive(Validate)]` and `Accept-Language`).
- [x] `Transaction::savepoint(|tx| Box::pin(async move { … }))`: `SAVEPOINT` before, `RELEASE`
      on `Ok`, `ROLLBACK TO` + `RELEASE` on `Err` (the error is returned); nests; works inside
      `db.transaction(…)`
- [x] `renox::db::Encrypted<T>` (any serde `T`, stored as sealed JSON text; `Option` for
      nullable columns), `Deref`/`DerefMut`/`new`/`into_inner`, transparent serde, a `Debug`
      that hides the value
- [x] The `Db` built at boot carries a key derived from `APP_KEY` (and so do its
      transactions); statements seal `DbValue::Encrypted` just before they run, rows open
      `Encrypted` columns with the key of the `Db` that read them
- [x] examples/teams stores its webhook secret in an `Encrypted<String>` field (no
      `state.encrypt`/`decrypt` in its handlers)

Notes from M23:
- Why this works now when M19b deferred it: M19b looked for the key at encode/decode time
  (sqlx gives no context there) or in `renox::context` (absent in tests and bare code). The
  key now travels with the data instead: `Db` → `Transaction` → `Conn` for writes, and each
  `Row` keeps the key of the `Db` that fetched it; `Row::try_get` sets it in a thread-local
  for the synchronous decode only. Two apps with different keys in one process each use
  their own.
- `DbValue::Encrypted(Unsealed)` holds the plain JSON until the statement runs, so
  `save_changes` compares plain values (a fresh nonce would make every save a change) and
  `Debug` of a `DbValue` or `Sql` never shows it (`Unsealed` prints `..`).
- Sealing uses the same AES-256-GCM as `state.encrypt` with other associated data
  (`renox.column`), so a sealed column can't be passed off as a `state.encrypt` value or an
  encrypted cookie. `APP_KEY` rotation isn't handled (docs/operations.md says what's lost).
- A `Db` made outside `App` (`Db::from(pool)`) has no key: writing or reading an
  `Encrypted` column fails with a message saying so, never stores plain text.
- Savepoints are plain SQL on both databases (sqlx's nested `begin` would need a lifetime
  on `Transaction`, a breaking change). A cancelled savepoint future leaves it open until
  the transaction ends, which rolls it back with the rest.

### M24 · Laravel's leftovers from M21
C of the owner's B/C/D before 1.0: the small items M21 deferred.
- [x] `Routes::domain("admin.example.com" | "{account}.example.com", routes)`, the
      `DomainParams` extractor, a DOMAIN column in `route:list`
- [x] `Routes::fallback(handler)` (per app, or per domain)
- [x] `route_is(pattern, …)` and `request.route` in views, the `CurrentRoute` extractor
- [x] `Redirect::route(name, params)` and `Redirect::intended(&session, fallback)`
      (`RedirectExt`, in the prelude)
- [x] `Session::push` and `Session::increment`
- [x] `Factory::factory()` → `FactoryBuilder`: `count`, `state`, `sequence`, `make`,
      `make_one`, `create`, `create_one`
- [x] Plural ranges in translations: `{0} …|[1,5] …|[6,*] …`
- [x] `{% break %}` / `{% continue %}` (MiniJinja's `loop_controls`) and `class_names(…)`
- [x] examples/shop's admin nav marks its section with `route_is`

Notes from M24:
- Domains: each pattern gets a whole router of its own (the same middleware stack, Renox's
  routes, public files and its own fallback), and the outermost service picks one by the
  `Host` header (port ignored, case-insensitive). A host that matches a domain gets only
  that domain's routes; other hosts get the routes without a domain. Unlike Laravel, a
  domain's host doesn't fall through to the routes without one: that's what lets the same
  path mean different pages, which one axum router can't hold. `App::layer` layers are
  applied to every router (`AppLayer` became `Fn`).
- Route names stay global; `route()` gives the path. `RouteTable::name_of(path, domain)`
  needs the domain: `/menu` exists on two hosts in the tests and got the other host's name
  until the dispatcher marked the matched domain (`MatchedDomain`) for the lookup.
- `Routes::fallback` replaces the 404 after public files (disk or embedded); a second one
  for the same app or domain is a boot error, as are domains inside domains and `domain`
  or `fallback` inside a path `group`.
- `Redirect::route` needs the app, which `renox::context::app()` gives in handlers,
  middleware, jobs and commands; `intended` is the one the login page used.
- Factory closures run while building the models, before the returned future, so a
  handler that uses them stays `Send` (checked in `send_handlers.rs`).
- Plural ranges follow Laravel: `{n}` exactly, `[a,b]` inclusive, `*` open; no match takes
  the last text; texts without ranges keep the `one|many` rule.

### M25 · Derived validation and the browser's language
D of the owner's B/C/D before 1.0.
- [x] `#[derive(Validate)]`: `#[validate(required, max = 100, unique("users", "email"), …)]`
      on fields, each item a call on the field's rules; `each(…)`, `distinct`, `rename`;
      `label` applied first; generic structs work
- [x] `#[validate(hooks)]` on the struct + `validation::ValidateHooks` (`prepare`,
      `authorize`, `after`)
- [x] `App::detect_locale()`: the first `Accept-Language` language the app has texts for
      (whole tag, then its language), after the session's choice and before `APP_LOCALE`;
      `Vary: Accept-Language` on responses
- [x] `rnx make:module --resource` writes a derived form; examples/hello uses the derive
      and `detect_locale`

Notes from M25:
- The derive doesn't know the rules: `max = 100` becomes `.max(100)` on the field, so any
  rule of `Field` (and future ones) works and a typo is a compile error that names the
  missing method. Arguments are pasted inside `rules(&self, …)`, so `confirmed(&self.x)`
  and `same("again", &self.again)` work.
- Found by its test: a `label` after `required` didn't reach the messages (they're built
  when a rule fails), so the derive moves `label` to the front.
- `Accept-Language`: qualities are honoured (`q=0` refused), ties keep the header's order,
  `*` is ignored; available means `en`/`id` or a lang file. The middleware computes the
  locale in a block (§4.2: a closure borrowing `req` across `next.run` isn't `Send`).

### M26 · Completeness before 1.0
An audit after M25 (asked by the owner: are the examples, tests and docs complete?) found
two bugs in M22's keys, M22–M25 features no example shows, stale or missing docs, 379
public items without a doc comment, and weak coverage in `renox-cli`. Three PRs:
- [x] M26a: the key bugs (`insert_many`/`upsert`, `unique().ignore()`), examples for M22–M25
      (crud: derive + hooks, a CSV import with savepoints, factory states; api: `Ulid` keys;
      shop: recently viewed, plural ranges, `class_names`, `Redirect::route`; teams: public
      pages with `Routes::domain` and a fallback)
- [x] M26b: docs brought up to date, every public item documented, `missing_docs` in CI
- [x] M26c: tests for `renox-cli` (`serve`, `scaffold`, `new`, `tailwind`) and the weak core
      files (`db/error.rs`, `path.rs`, `db/json.rs`, `app.rs` commands)

Notes from M26a:
- Found by writing the examples: a `Routes::fallback` that redirects (teams) answered 404
  with a `Location` header. `ServeDir::not_found_service` forces 404; `ServeDir::fallback`
  keeps the status. The framework test had a fallback that answered 404 anyway, so it passed;
  it now redirects.
- `write_many` writes the `id` column for keys the app owns and makes missing ULIDs/UUIDs
  first, before any statement; an `i64` id is still left to the database.
- `lock_for_update` still has no example (the CHEATSHEET shows it); the shop's checkout
  takes stock with a conditional `UPDATE`, which is the better pattern there.

Notes from M26b:
- 379 public items in renox-core (and `renox::prelude`) had no doc comment: struct fields,
  methods, enum variants. They were written in four parallel batches by file (no two batches
  on one file, no builds meanwhile), then the claims marked uncertain were checked against
  the code (error statuses, S3 defaults, lettre's ports, email normalization, migration
  names; one reworded). `#![warn(missing_docs)]` in `renox`, `renox-core` and `renox-macros`
  makes CI's clippy (`-D warnings`) refuse an undocumented public item from now on.
- The docs audit's findings: CLAUDE.md (§2 files, §3 i18n and hooks, §7 status and counts),
  the guides (ui: the current route, `class_names`, loops, plural ranges; testing: factory
  states; authorization: derived form requests; postgresql: key columns, savepoints after a
  failed statement; relations: ULID/UUID keys, `insert`, `encrypted`, savepoints; operations:
  `Vary` behind a CDN), README (Web tour, Laravel table), llms.txt, the hello README, the
  AGENTS stub and `env.stub`, CHEATSHEET (hooks on `insert`, `detect_locale`).
- `RedirectExt` is sealed (only axum's `Redirect` implements it) and `InvalidUlid` is
  `#[non_exhaustive]`, so both may grow after 1.0; docs/stability.md lists them.

Notes from M26c:
- Coverage before (CI's llvm-cov): renox-cli's `serve.rs` and `scaffold.rs` 0%, `main.rs`
  10%, `tailwind.rs` 15%, `new.rs` 38%; in renox-core `db/error.rs` 48%, `path.rs` 56%,
  `db/json.rs` 63%, `app.rs` 66% (its commands ran only from the CLI job).
- The CLI's untested parts were split from their side effects so they test without a
  network or a working directory: `new::run_in(parent, …)`, `with_key(env, key)` for
  `key:generate`; `serve`'s fingerprint is tested through `collect`, Tailwind's download
  isn't (the CLI job runs it).
- `App::run_args` is new public API, added so the binary's built-in commands can be tested
  from `it/commands.rs` (and useful for programs that drive an app).
- Found by the tests: `renox::Path` turned every rejection into a 404, including a handler
  asking for a parameter its route doesn't have (axum's `WrongNumberOfParameters`) and an
  unsupported type. Those are the app's bugs and now answer 500.

### M27 · Data grid
Asked by the owner before v1.0: an enterprise-style data grid as a showcase of what Renox can
do, responsive and dashboard-like. Agreed with the owner (2026-10-01): a framework component
(`renox::grid` + `renox/grid.html`, in every app), built on htmx and Alpine-free plain JS rather
than wrapping a JS grid (Tabulator lacks row spans and brings ~400 KB; AG Grid's column groups
and Excel export are Enterprise-only), Cally for date ranges (MIT, accessible web components),
preferences per user in the database, cell and row editing both, automatic hierarchical merged
cells, exports on the server (CSV, Excel, PDF through a print page). Three PRs were planned;
a fourth (M27d) was added after M27c:
- [x] M27a: the grid: `Grid`/`Column` (text, number, money, date, datetime, bool, select, tags,
      custom), `GridRequest`, filters per kind from the query string, sorting, server pages,
      grouped headings (`Column::under`), columns per screen size (`mobile`, `hidden`) with
      the column menu, frozen columns left and right, `grid_preferences` (users) or the
      session (guests), Cally's date range calendar, custom cells (`caller(row, column)`,
      `GridPage::extend`, `sparkline`), the dashboard layout (`rx-grid-fill`), examples/grid
- [x] M27b: row details with audit fields (created/updated by and at), cell and row edit
      modes (validated, saved per cell or per row), drag-and-drop row order, merged cells
      (same values in a row of cells, hierarchical between columns)
- [x] M27c: exports of every filtered row: CSV, Excel (`rust_xlsxwriter`, headings merged
      as on screen) and a print page for PDF

- [x] M27d (asked for after M27c): columns moved by dragging their heading and resized by
      dragging its edge, widths kept in `GridPrefs::widths`

Notes from M27a:
- Columns per screen size are applied by the script (classes on the cells), so the server
  renders every column and a resize needs no request; order and frozen columns change the
  markup, so they're saved first and the grid reloads.
- Guests keep their columns in the session instead of localStorage (the owner's choice was
  localStorage for guests): the server needs the order and frozen columns to draw the grid.
- The grid is a GET form swapped by htmx (`hx-select` of its own id), so a page needs no
  fragment handling, and without htmx it still works as a plain form.
- Tags filter with `CAST(column AS TEXT) LIKE '%"value"%'`, which works on SQLite's TEXT and
  PostgreSQL's JSONB alike.

Notes from M27b:
- Details rows are made by the script from a `<template>` in each row, not by the server, so
  merged cells can make room: the cells spanning past the row grow by one row, and the details
  fill the runs of columns between them (one cell per run, the content in the widest).
- Saving an edit reloads the grid's page instead of replacing the row: merged cells, sorting
  and filters may all change, and a page is one request. The request goes through htmx, so
  renox.js shows 422 errors next to the editors and toasts work as everywhere.
- `RowOrder` takes the ids as one comma-separated field: axum's `Form` (serde_urlencoded)
  can't read a repeated field into a `Vec`.
- Merged columns need the rows sorted by them; with a user's own sort they still merge equal
  neighbours, by value. The tools column (drag, details, edit) is frozen at the left edge,
  before the user's frozen columns.

Notes from M27c:
- Excel is the opt-in `xlsx` feature, not a default: `rust_xlsxwriter` (pure Rust, MIT/Apache)
  and its zip dependency would otherwise build in every app. CSV and the print page need
  nothing.
- PDF is the browser's: the print page is plain HTML with print CSS (landscape, headings
  repeated per page), as agreed with the owner, so no PDF crate or fonts.
- Exports use the columns the user shows on wide screens, in their order, and leave custom
  columns out (they're drawn by templates); CSV guards against formula injection.

Notes from M27d:
- Moving a column by its heading needs a mouse or pen: on a touch screen, dragging a heading
  would fight the sideways scroll, so touch users move columns in the column menu. Resizing
  works with touch (the handle takes the pointer).
- A drag that ends on a heading isn't a click, so it doesn't sort.

### M28 · The data grid next to Filament's tables
The owner compared the grid with Filament 5's tables (2026-10-02) and asked for all of the
missing pieces, in this order:
- [x] M28a: query string names per grid (`prefix`) and keeping the page's other values, the
      search box (`Column::searchable`), active filter chips, `row_url`, `empty_state`
- [x] M28b: selecting rows and bulk actions, row actions with a confirmation
- [x] M28c: summaries (sum, average, count, range) in the footer, grouped rows with their
      own summaries
- [x] M28d: a card layout on phones, more column kinds (badges with colors, icons, images,
      descriptions, tooltips)
- [x] M28e: relationship columns, an advanced filter (and/or, operators), polling, filters
      and sort kept in the session

Notes from M28a:
- Before `prefix`, two grids on a page shared `page`, `sort` and `q.*`, and a grid's request
  dropped every query string value that wasn't its own (a tab, the other grid). Each grid now
  writes the others' values as hidden fields, so both survive either grid's requests.
- The search matches every word in any searchable column with
  `LOWER(CAST(column AS TEXT)) LIKE`, the same on SQLite and PostgreSQL.

Notes from M28b:
- "All matching" sends `all=true` with the grid's query string, so the handler's
  `Grid::selected` applies the same filters as the page the user saw; ids that don't parse as
  the model's key are a 400.
- The confirmation is the grid's own `<dialog>` (focus on Continue, Escape cancels), not
  `window.confirm`, and the Continue button turns red for `danger()` actions.

Notes from M28c:
- Summaries are SQL aggregates over the filtered query without its order (PostgreSQL refuses
  an `ORDER BY` next to aggregates), cast to `DOUBLE PRECISION` so integer and decimal columns
  decode alike on both databases. Group figures are one `GROUP BY` query per summarized column,
  so a group's subtotal covers all its rows, also those on other pages.
- A group's values are compared as text (`CAST(column AS TEXT)`); booleans read back as `1`/`0`
  on SQLite and `true`/`false` on PostgreSQL, so both are looked up.

Notes from M28d:
- Cards are CSS only (the table's rows and cells as blocks and flex rows under 768 px, each
  cell labelled from its `data-label`), so the server draws one page for every screen and the
  column menu's phone choice decides what a card shows. The headings are hidden there, so the
  toolbar gets a sort choice and a list that opens each column's filter.

Notes from M28e:
- Related values are correlated subqueries (`{T}` stands for the model's table, names
  checked as plain identifiers), so filtering, sorting and searching need no join and the
  model stays as it is; the page fetches the values with `id IN (…)`, one query per column.
- The advanced filter's rules become one `where_raw` with the rules' SQL joined by AND/OR;
  their values are bound typed (numbers, dates, booleans), so PostgreSQL accepts them.
- `remember` needs a marker (`state=1`, a hidden field) to tell "the user cleared every
  filter" from "a fresh visit": without it, a cleared grid came back with the old filters.
  Bulk "all matching" and exports go through the same remembered state.
- Sorting adds `(column IS NULL)` before each key: PostgreSQL puts NULLs first when
  descending and SQLite last; the grid now puts them last on both.

Notes from the docs audit after M28:
- Every guide, the README, CHEATSHEET, llms.txt, the examples' READMEs and module docs, and
  the stubs (`AGENTS.md.stub`, `env.stub`) were checked against the code after M28 and
  fixed where they had drifted. `docs/grid.md` is new: a guide for `renox::grid`, compiled
  as the `GridGuide` doctest.
- Three code issues found by the audit were fixed on the same branch: the grid's date-time
  filters (`from.`/`to.` and the advanced filter's `on`/`before`/`after`) took UTC days and
  now take days of `APP_TIMEZONE`, like the cells; `User::delete_account` also deletes the
  user's `grid_preferences` rows (the table can't have a foreign key: not every app has
  `users`); and the Auth module's `notifications:prune [--days 30]` /
  `auth::prune_read_notifications` delete notifications read long ago.

### M29 · Examples complete
An audit after M28 (asked by the owner: are the examples complete?) compared every example
with its README, the features and the common business apps. Three PRs:
- [x] M29a: the bugs it found (grid edits by guests, jobs' open registration, shop's English-only
      mail and pickup text, teams' public links, htmx-recipes' plain-form duplicates and
      repeated rows), tests for every README claim, seeders that run twice, `.env.example`
      files, fields/webhooks/uploads on the kit, current patterns (`Routes::resource`,
      `Redirect::route`, derive, `money`)
- [x] M29b: `examples/backoffice`, a small business's back office in the style of Filament's demo: grids
      everywhere (customers, products, invoices, the stock ledger, staff, activity), invoices
      with line items (`repeater`) issued from stock, voided and printed, Midtrans/Xendit
      payment pages and webhooks end to end, a stock ledger that never goes below zero, CSV
      import from the browser, background exports with a notification, roles/permissions
      and audit-log screens, typed settings, email verification for staff added by an admin,
      and sign-in pages in the company's colours
- [x] M29c: smaller extensions: relations as a public blog (Markdown, `seo()` per post,
      search, RSS, `Sitemap`, Tailwind), a browser client for the api (public/client.html,
      `GET /api/me`), uploads on S3 (the `s3` feature, tested on SeaweedFS in CI), and mail
      attachments, `cc`, `bcc` and `reply_to` in jobs

Notes from M29a:
- Guests could change orders in examples/grid since M27: the grid's edit routes had no
  `require_auth`. Grids now take a `can_edit` flag in the example, the pattern for apps:
  build the grid with or without its edit tools for the current user.
- shop's "in the recipient's language" needed a `locale` column on `users`
  (`Recipient::locale` reads it); without one, a notification is written in the language
  of the request that caused it (the buyer's, for the admin's "new order").
- Infinite scroll by page number repeats a row when one is added at the top meanwhile;
  htmx-recipes pages by id now (`?before=`).

Notes from M29b:
- No point-of-sale screen: an invoice issued and "Paid in cash" is the counter sale; a POS
  view (a cart in the session, a barcode field) is left for an app to build on it.
- The stock ledger doesn't use `lock_for_update`: a conditional `UPDATE … WHERE stock >= ?`
  through `Query::increment` takes stock safely on SQLite and PostgreSQL without reading the
  row first, and its "no row changed" is the "not enough" answer. `lock_for_update` stays
  shown in the docs for read-then-write cases.
- Staff are added with a first password and a verification mail (`send_verification`);
  Renox has no public "send a password reset link" call, so an invitation link would need
  one (a small addition for later, with a `renox-admin` plugin in mind).
- Bulk actions carry the grid's query string, so a job can rebuild the grid's filters with
  `GridRequest::new` and export what the user saw (`Grid::export` on that request).
- Toasts answered with `HxRefresh` wait in the session for the refreshed page, so tests
  read them from the next page, not from `HX-Trigger`.

Notes from M29c:
- An example on Tailwind commits its built CSS (public/css/app.css), so `cargo run` and CI
  need no download; `rnx tailwind --minify` rebuilds it after views change. The
  typography plugin (`prose`) is in the standalone CLI.
- The base layer of the Tailwind input styles plain elements (`h1`, `td`, inputs), which
  keeps templates short and tests able to look for `<h1>Coffee</h1>`.
- examples/postgres stays separate from examples/fields. Merging them was considered,
  but fields is about input types and postgres about running one app on both databases.

### M30 · Every example on the UI kit
Asked by the owner after M29 ("make sure the examples use Renox's standard kit, don't make
things up"). An audit found five examples off the kit (hello, jobs, postgres, relations,
api's client), htmx-recipes hand-making its modal, menu and tabs, and every kit example
carrying a copy of the `rnx new` stub's navigation bar CSS, because the kit had no
navigation. The owner chose to add navigation to the kit (rather than keep app CSS for it)
and to keep Tailwind next to the kit in relations.
- [x] Kit: `navbar`/`nav_links`/`nav_link`, `sidebar`/`sidebar_link`/`sidebar_section` +
      `rx-shell`, `page_header`, `toolbar`, `row_actions`, `list`, `columns`, `card_grid` +
      `media_card`, `link_tabs`, `thumbnail`, `progress`, `menu_button`, `rx-page--fill`,
      `rx-image`, `hide_label`, `confirm(cancel_label, fields)`
- [x] `rnx new`'s layout on `navbar`; AGENTS.md.stub's rule to build pages from the kit
- [x] All fourteen examples on the kit; mails on the kit's mail layout and components
- [x] Re-record docs/assets/demo.gif on the kit's guestbook (CLAUDE.md §4.10)

Notes from M30:
- An imported macro's name hides a context value of the same name: jobs' statements page
  has a `progress` number, so it imports the kit's macro `as progress_bar`.
- Imports a block needs when htmx asks for that block alone (`View::fragment`) go at the
  top of the template, outside `{% block content %}`, or the fragment fails with an undefined
  macro (shop's product results, hello's entries).
- A flex item whose width comes from its content gives an `overflow-x: auto` child no width:
  `page_header`'s buttons take the rest of the row (`flex: 1 1 auto`) so a `period_filter` in
  them shows.
- What stays app CSS: brand tokens, and documents meant for paper (backoffice's printed
  invoice).

### M31 · A warm default theme and a type scale
The owner asked for a better-looking default and floated neumorphism with Poppins. Three
directions were compared on the examples' pages, in light, dark and phone widths: the look
as it was, "Warm" (Poppins and Inter, warm paper, hairlines, indigo), and "Soft"
(neumorphism kept readable). Then Warm with a fixed type scale was compared too. The owner
chose Warm with the scale.
- [x] Warm as the default theme, its tokens in renox-ui.css, dark mode
- [x] Inter and Poppins bundled (OFL 1.1, Latin subsets), served from `/_renox/fonts`,
      the text font preloaded by `renox_ui()`
- [x] The type scale (`--rx-type-*`, eight roles) on every component; small capitals,
      large figures, pills for changes
- [x] `data-rx-theme="classic"` for the previous look
- [x] docs/ui.md "Themes and type", AGENTS.md.stub, the demo GIF

Notes from M31:
- Full neumorphism was not the default: shadow-only edges miss WCAG AA for controls, make
  buttons and fields look alike, disappear in dark mode, and crowd data grids. The Soft
  prototype kept hairlines for that reason and still lost its effect in dark mode.
- A stat's figure is sized from its card's width (`container-type` on `.rx-stat`,
  `font-size: clamp(1rem, 12cqi, 2rem)`), so "Rp 17,469,180" stays on one line wherever it
  fits. On a phone's half-width card it wraps between words instead of being cut.
- Theme selectors use `:where([data-rx-theme=…])` so they weigh no more than `:root`: an
  app's brand tokens, loaded after the kit, keep winning in any theme.
- All examples share cookies on 127.0.0.1 during browser checks (one cookie name for every
  port), so a check that visits several examples has to log in again before each app's
  pages.

### M32 · English only
The owner asked for the whole repository in English (discussion with the owner stays in
Indonesian), including dropping the built-in Indonesian locale; Spanish replaces it where an
example shows a second language.
- [x] Framework: `id` texts removed from `i18n.rs`, `validation/messages.rs` (`Locale::Id`)
      and `auth/module.rs`; doc examples and unit tests in English
- [x] CLI: no `id.json` stub; generators' help and tests in English (`make:rule TaxId`)
- [x] Tests: a Spanish fixture (`crates/renox/tests/lang/es.json`) for translated kit,
      auth and validation texts, plurals and `detect_locale`; fixture tables `products` and
      `notes`
- [x] Examples: English seed data, names, pages, mails and tests; hello and shop in
      English and Spanish
- [x] Docs: README, CHEATSHEET, llms.txt, docs/*.md, stubs, CLAUDE.md

Notes from M32:
- Kept on purpose: IDR/"Rp", `Asia/Jakarta`, Indonesian place names and phone numbers, and
  the `id` row of the number-separator table in `view_filters.rs` (formatting data).
- Spanish uses "." for thousands like Indonesian did, so the shop's prices read the same
  ("Rp 25.000") in both locales.
- Older milestone notes above still say "English and Indonesian": they record what was
  true then.

### M33 · Small additions from the parity review
The owner chose the "small adds" of the parity review before v1.0, so they land while the
API may still change.
- [x] Validation rules: `gt`/`gte`/`lt`/`lte`, `decimal`, `dimensions` (`Dimensions`),
      `prohibited`/`prohibited_unless`/`prohibits`, `required_with_all`/
      `required_without_all`, `accepted_if`/`declined`/`declined_if`, `numeric`, `integer`,
      `multiple_of`, `min_digits`/`max_digits`, `json`, `ulid`, `timezone`, `mac_address`,
      `ascii`, `hex_color`, `doesnt_start_with`/`doesnt_end_with`, `not_matches`
- [x] Route model binding: `Found<M>`
- [x] `Routes::view`, `Routes::redirect`, `Routes::permanent_redirect`
- [x] Several disks: `App::disk`, `state.disk`, `StorageConfig::from_env`
- [x] `Routes::etag()`, `App::xsrf_cookie()`, `TRUSTED_HOSTS`
- [x] Examples (crud, relations, backoffice, uploads), guides, the parity review
- [x] Still open from the review's small adds after M33 (`current_password`, the HIBP check,
      several mailers, session `keep`/`now`, named error bags, `hasManyThrough`): done in M34

Notes from M33:
- `Found` picks the parameter named after the table, else the only one; a parameter named
  after a column (`{slug}`) is matched against it. Several parameters and none named after
  the model is the app's mistake (500), not a guess.
- ETags are computed in the outermost layer: a handler returns an unrendered `View`, so a
  route layer would hash an empty body. The route only marks the response.
- `gt`/`lt` compare two texts as numbers when both read as one: forms often keep prices in a
  `String`, and comparing their lengths would surprise.
- A named local disk serves its public files at `/_renox/disks/<name>/public/…` rather than
  under `/storage`, which belongs to the default disk.

### M34 · The rest of the parity review's small additions
- [x] `current_password()`, `Password::uncompromised()` (Have I Been Pwned, k-anonymity),
      `Validator::finish_for`
- [x] Session `keep` and `now`
- [x] Named error bags: `Validate::ERROR_BAG`, `#[validate(bag = "…")]`, `error(…, bag=…)`,
      `errors_in`, `bag=` on the kit's fields
- [x] Several mailers (`App::mailer`, `MailConfig::from_env`, `mailer_named`,
      `queue_mail_via`) and `MAIL_FAILOVER`
- [x] `relations::has_many_through`
- [x] Examples (jobs, relations), guides, the parity review

Notes from M34:
- The breach check fails open: a sign-up shouldn't stop because a third-party service is
  slow. It runs after the other rules, so short passwords never reach it.
- Failover tries the other mailers' drivers but records the mail as the app's, so the preview
  page and `app.sent_mail()` show it whoever sent it.
- An error bag only changes where a redirect flashes the errors; htmx and JSON answers are the
  usual 422, since their errors are placed in the form that sent them.
- `MailConfig::from_env` defaults its driver to `MAIL_MAILER`: a named mailer then logs while
  developing and keeps mail in memory in tests, instead of trying an SMTP server that isn't
  configured.

### Plugins (separate crates, after M18)
- [x] `renox-2fa` (TOTP and recovery codes): #146 (#169–#173), docs/two-factor.md
- [x] `renox-editors` (rich text, Markdown and code editors, `code_entry`): #149, #150,
  docs/editors.md; the kit's entries got `prefix_actions`/`suffix_actions` and modules
  `Registry::asset` with it
- [x] `renox-oauth` (social login): #147, docs/oauth.md; `OAuth::new().google().github()
  .provider(…)`, the `Provider` trait (`authorize_endpoint`, `token_endpoint`, `scopes`,
  `authorize_params`, `exchange` with the standard `exchange_code`, `profile` → `Profile`),
  `Credentials::from_config`, `oauth.redirect`/`oauth.callback`/`oauth.unlink`, PKCE (S256)
  and a single-use session-bound `state`, the `oauth_accounts` table (`OAuthAccount`, no
  tokens), linking by verified email only, `AccountLinked`/`AccountUnlinked`/`LoggedInWith`;
  in renox-core: `auth::sign_in`, `register_verified`, `registration_open`,
  `confirm_identity`, `User::has_password` (passwordless users: an empty `password`; the
  account page lets them set one and confirms them another way), and the login pages'
  `renox/auth/login_options.html`; used in examples/teams
- [x] `renox-oauth` company sign-in: #51, docs/oauth.md; `Microsoft` (`tenant`, `MICROSOFT_TENANT`),
  `Google::hosted_domain`, `OAuth::allow_domains`, `Refused` from `Provider::profile`,
  `Provider::with_config`, the `OAuthRefused` event (`oauth.refused`)
- [x] `renox-admin` (resource tables and forms): #148, docs/admin.md;
  `Admin::new().path(…).title(…).authorize(…)/gate(…).resource(…)`, the `AdminResource`
  trait (`Model`, `Form`, `label`, `plural_label`, `slug`, `navigation_group`,
  `record_title`, `columns` → grid `Column`s, `fields` → `Field`s (`text`, `email`,
  `password`, `url`, `tel`, `textarea`, `number`, `money`, `date`, `datetime`, `select`,
  `checkbox`, `toggle`, `belongs_to`), `fill`, `rules` (record-aware, e.g.
  `unique(…).ignore(id)`), `entries` → `Entry`s (the view page), `filters` → `Filter`s
  (tabs), `actions` → `AdminAction`s (bulk, `.row()`), `query`, `grid`, `allows` (the
  model's `Policy`: `viewAny`, `view`, `create`, `update`, `delete`, `deleteAny`,
  `restore(Any)`, `forceDelete(Any)`)), soft deletes with a trash, exports, routes
  `admin.{slug}.*`, views `renox-admin/*.html` (+ `renox-admin/{slug}/cells.html` for custom
  columns); examples/admin. In renox-core: `Column::label`/`options`,
  `renox::currency_decimals`, and `GridRequest` reads the whole path in a `Routes::group`
- [x] `renox-billing` (subscriptions, Laravel's Cashier): #155, docs/billing.md;
  `Billing::new().plan(Plan::new(key, label).price(amount, currency, Interval::Month)
  .trial_days(n).feature(…).via(gateway).price_id(gateway, id)).stripe().xendit()
  .gateway(…).generic_trials().without_proration().redirect_to(…)`; `Billing::of(&state,
  &billable)` → `Customer` (`named`, `subscription`, `subscriptions`, `subscribed`,
  `subscribed_to`, `on_trial`, `on_grace_period`, `trial_available`, `customer_id`,
  `checkout`, `start_trial`, `swap`, `cancel`, `cancel_now`, `can_resume`, `resume`);
  `Billable`/`Owner` (users, or a team); the `Gateway` trait (`create_customer`, `checkout`,
  `swap`, `cancel`, `resume`, `verify_webhook`, `webhook_event_id`, `parse_webhook` →
  `Notice::Subscription(Remote)`/`Notice::Payment(Payment)`), `Stripe` (Checkout Sessions,
  prorated swaps, cancel/resume, `Stripe-Signature`) and `Xendit` (recurring plans,
  `x-callback-token`); the `subscriptions` and `billing_customers` tables (prefix
  00010101000900); webhooks at `/billing/webhooks/{gateway}` through `renox::webhook`
  (once per event, older events ignored by `synced_at`); `SubscriptionRoutes`
  (`require_subscription`, `require_plan`); pages `billing.*` (`billing/plans.html`, an
  account section `billing/section.html`); events `SubscriptionCreated`/`Updated`/
  `Canceled`, `PaymentSucceeded`/`Failed`; examples/billing. In renox-core:
  `Registry::provide`

### v1.0
Started by the owner on 2026-10-03, after M34. In steps, one PR each:
- [x] V1a, release readiness: lockstep versions (`=`), path-only dev-dependencies (no publish
      cycle), docs.rs metadata, `rnx new` from crates.io, RELEASING.md
- [x] cargo-semver-checks in CI (moved from M16b): against the base branch with
      `--release-type minor` until the first release, then against crates.io
- [x] V1b, the audit's must-fix (A) items: enums for settings, `Duration` lifetimes,
      `Notification` and `Job::failed` signatures, `notify` for users and recipients,
      `Upload`/`Migration` encapsulated, no `Locale`, `FromDb`'s promise written down
- [x] V1c, the audit's should-fix (B) items (all but renaming `gate` to `authorize_gate`):
      closure argument order, private `AppState` internals, `disk_named`, one `Factory`
      API, `DateTime` timestamps, private `Recipient` fields, `flash_now`/`remember_locale`,
      `renox::Result` returns, `UpcomingRun`, opaque `Zone`, sealed traits, hidden derive
      plumbing, `DownOptions`, `retry`/`retry_all`, secrets hidden from `Debug`
- [x] V1d, the documentation site built with Renox (`site/`, package `renox-site`): the
      repository's Markdown compiled into one binary, a sidebar, a table of contents per
      page, search, a sitemap, ETags, systemd units for the owner's own server;
      docs/tutorial.md (Stash, a bookmarks app, `rnx new` to deploy) and docs/laravel.md,
      both compiled as doctests (`TutorialGuide`, `LaravelGuide`). The API reference is
      docs.rs, linked from the site once crates are published
- [x] V1e, the starter kit: `rnx new --starter` (stubs/starter/: sidebar layout, email
      verification, roles with `Permissions`, dashboard, users page, activity log,
      `users:admin`, seeder, tests; in tests/cli/run.sh), and the semver promise in
      docs/stability.md ("The promise": deprecation policy, data compatibility across 1.x,
      support window for the previous minor, MSRV rule) with SECURITY.md's supported versions
- [x] `1.0.0-rc.1`: the version set and the changelog dated (#122), published on
      2026-10-03 after the owner confirmed, tag `v1.0.0-rc.1`; checked: docs.rs built,
      `cargo install renox-cli --version 1.0.0-rc.1`, `rnx new` (plain and `--starter`) pass
      their tests against the published crates
- [x] The README's quick start from crates.io and its "Use Renox with Claude Code" section
- [ ] The docs site online on the owner's server, at Renox's own domain (GitHub Pages was
      tried and dropped: the account maps project sites to a personal domain)
- [x] `1.0.0-rc.2`: `rnx` formats what it writes (#124, #125); published 2026-10-04 (#126,
      tag `v1.0.0-rc.2`), checked from crates.io (fmt and tests of new apps)
- [x] The tutorial followed in CI (`tests/tutorial/`, #131), which found that
      `TestApp::travel` didn't reach jobs; plural tables from the generators (#127, #129); the
      kit's stretched card (#128, #130)
- [x] `1.0.0-rc.3` with those fixes: published 2026-10-04 (#132, tag `v1.0.0-rc.3`)
- [x] `1.0.0-rc.4`: the fixes found using rc.3 (#133, #144, #163, #178) and the extension
      points for plugins (#167, #168); tag `v1.0.0-rc.4`
- [x] `1.0.0-rc.5`: #219's checks, #226, the plugins renox-oauth (#147), renox-admin (#148)
      and renox-billing (#155), roles per branch (#244), CI on rustc 1.99; tag `v1.0.0-rc.5`
- [ ] `1.0.0-rc.6`: the fixes the coverage epic found (#246: #271–#290, #278, #279, #292,
      #294), Back closing sheets and menus, Escape closing toasts (#266), the browser and
      process suites and the coverage floor in CI; the version is set on main, published once
      the owner runs `cargo publish` (RELEASING.md)
- [ ] 1.0.0
- [ ] Real crates published to crates.io (`renox`, `renox-core`, `renox-macros`, `renox-cli`;
      only 0.0.1 placeholders exist before the release candidate; the owner runs `cargo login` and RELEASING.md), then
      crates.io/docs.rs badges and `cargo install renox-cli` in the README

### UI kit · Form fields next to Filament's forms

Filament's form fields (https://filamentphp.com/docs/5.x/forms/overview) as the yardstick,
asked by the owner; three stages on branch `ui-form`, one PR.

- [x] Stage 1: `radio`, `checkbox_list`, `form_grid` / `fieldset` with `span`, `input`'s
  `prefix` / `suffix` / `datalist`, `disabled` / `readonly`, `id` on every field,
  `has_old()`; fixed: an unticked checkbox (and an unpicked radio) came back with its default
  after a failed submit, and the error summary missed fields with their own `id`.
- [x] Stage 2: `input(…, revealable=true)` (Renox's sign-in pages use it) and
  `copyable=true`, `toggle_buttons`, `file` (drop zone, previews, `current`), `date_picker`
  (Cally in a popover, `renox_calendar()`), `show_when` / `hide_when`.
- [x] Stage 3: nested form names in `Valid` (`lines[0][name]` → `Vec<Line>`), `KeyValues`,
  `tags_input`, `select(…, multiple=true, searchable=true)`, `repeater`, `key_value`,
  `wizard` + `wizard_step`.
- [x] Examples: fields (every kind, tags and specifications), shop (checkout courier or
  pickup, the admin's searchable category), uploads (`file`), teams ("New team" wizard with a
  repeater of members).
- [x] Options from the server as you type: `select(…, options_url=…)` with `renox::select`
  (`SelectOption`, `OptionQuery`); `editable=true` adds what was typed (chosen at once) and
  renames the chosen option; examples/shop's admin categories.
- [ ] Later: rich text, Markdown and code editors (large JavaScript: plugins rather than the
  kit).

Notes:
- Cally's `calendar-date` dispatches a `change` that doesn't bubble: the kit listens in the
  capture phase. Its header shows the year and each month its own name (made for several
  months); for one month the kit hides the month's name and fills the header's slot with
  "October 2026" from `Intl.DateTimeFormat`, updated on `focusday` (whose detail is a `Date`).
- `show_when` disables a `<fieldset>` rather than each input, so a field disabled on purpose
  stays disabled when its group shows; without JavaScript the group stays visible.
- The searchable select keeps the native select, visually hidden rather than `display: none`,
  so the browser can still check `required` (its `invalid` event moves focus to the box).
- The repeater renumbers a row by rewriting every attribute that holds `lines[2]`,
  `lines.2.` or `rx-lines-2-`, so any field inside (date pickers, combobox, show_when) follows.
- A repeater's own error slot shows only the list's error: `error()` falls back to `name.*`,
  which showed a row's error twice.
- Options from the server: one URL for searching (`GET ?q=`), labels (`GET ?values=`), adding
  (`POST`) and renaming (`PUT` through `_method`), so an app writes three small handlers and
  keeps its own rules and permissions. The page holds only the chosen options; the script
  adds the server's ones to the native select as they're chosen, so the form posts as before.
  Searches wait 250 ms and cancel the previous request (`AbortController`).
- Found by tests: PostgreSQL's `JSONB` reorders object keys, so `KeyValues` is stored as a list
  of pairs; and the audit probe `validation_array_where_scalar_expected` caught `name[]=a`
  filling a text field once `[]` was read as nested, so a `[]` name is always a list.

### UI kit · Infolists next to Filament's

Filament's infolists (https://filamentphp.com/docs/5.x/infolists/overview) as the yardstick,
asked by the owner: read-only details of a record, which apps wrote by hand until now.

- [x] `infolist(columns, inline)`, `entry(label, value, …)` with `format` (`date`,
  `datetime`, `since`, `money`, `number`, `markdown`, `bool`, `color`, `image`,
  `key_value`), `badge`/`labels`, `url`, `copyable`, `tooltip`, `hint`, `prefix`/`suffix`,
  `limit`/`words`, `placeholder`, lists (`list`, `limit_list`), a call block as the value;
  `repeatable(label, items, columns)`.
- [x] Filters `money` (`APP_CURRENCY`, `renox::format_money`), `since`, `words`, `markdown`
  (pulldown-cmark).
- [x] Examples: shop's order page, fields' read-only product page.
- [ ] Later: a code entry with syntax highlighting (a plugin, next to the editors), entry
  actions (Filament's prefix/suffix actions: buttons beside a value).

Notes:
- Sections and tabs are the kit's `card`, `fieldset` and `tabs`, so there is no infolist
  layout of its own; `repeatable` is a nested `<dl>` per item.
- "Show N more" is a `<details>`: it works without the script.
- The owner chose a global currency (`APP_CURRENCY`, default `IDR`) over a per-call one;
  `money(currency=…)` still covers a page with several. Separators follow the page's locale
  like `number`, so English pages show `Rp 75,000`.

### Notifications next to Filament's

Filament's notifications (https://filamentphp.com/docs/5.x/notifications/overview) and its
database notifications as the yardstick, asked by the owner; both stages in one PR, live
updates over Server-Sent Events (the owner's choice over polling).

- [x] Stage 1, toasts: `body`, `link`/`action` (`ToastAction::link`, `ToastAction::event`,
  `new_tab`), `seconds`/`persistent`, `id`; `toasts(position=…)`; `Renox.toast` and
  `Renox.dismissToast` in the browser.
- [x] Stage 2, the bell: `DatabaseMessage`, `Auth::new().notifications()` (the
  `notifications.*` routes, `unread_notifications`), `notification_bell`,
  `renox/notifications.html`, `/notifications/stream` (SSE), new `User` methods
  (`notifications_before`, `notification`, `mark_notification_unread`,
  `delete_notification`, `delete_notifications`).
- [x] Examples: shop (the bell, `DatabaseMessage`s in the recipient's language, a toast with
  a link), jobs (`DatabaseMessage`).
- [ ] Later: a bell in the `rnx new` layout (it needs `.notifications()` in the stub);
  broadcasting other events over the same stream; toast actions that send a request.

Notes:
- The stream is woken by a broadcast channel in the process (`auth::notifications::Hub`,
  touched when a notification is stored and after each action on the list) and looks at the
  table every 15 s anyway, so several servers and a separate `queue:work` need nothing more
  (no Redis, no websockets). It ends after five minutes (the browser reconnects through the
  auth middleware again, so a revoked session loses it) and at shutdown, like live reload.
- The unread count is an `App::share` from the `Auth` module: one `COUNT` per rendered view
  for a logged-in user. Macros can't see shared values, so the layout passes it:
  `notification_bell(unread_notifications)`.
- The panel is the notifications page's `panel` block, fetched with `HX-Request` (a
  `View::fragment`): one template for the page, the panel and the answers to its actions.
- `to_database` now runs in the recipient's language (`with_locale`), as `to_mail` did.

### Dashboards next to Filament's widgets

Filament's widgets (https://filamentphp.com/docs/5.x/widgets/overview) as the yardstick,
asked by the owner: "B first" (widgets before actions), with charts as Renox's own SVG
rather than Chart.js.

- [x] `renox::chart`: `Period` (extractor, `previous`), `Trend` (`count`/`sum`/`average`
  per day or month, SQLite and PostgreSQL), `Series`.
- [x] `chart(kind, data, …)`: line, area, bar (stacked), pie, doughnut; tooltips, keyboard,
  data table, validated palette.
- [x] Kit: `stats`/`stat`, `dashboard`/`widget` (lazy `url`, `poll`), `period_filter`;
  `query_with`.
- [x] Example: shop's admin dashboard.
- [x] A, the actions: see "Actions next to Filament's" below.
- [ ] Later: scatter and bubble charts; a custom date range in `period_filter`; per-week
  buckets.

Notes:
- Lines and areas are SVG stretched over the plot (`preserveAspectRatio="none"`,
  `vector-effect: non-scaling-stroke`), while bars, dots, labels and ticks are HTML placed
  in percent, so text and round ends never stretch and the chart follows its container.
- The palette is the dataviz reference's first six slots, checked with its validator on the
  kit's surfaces (#ffffff, #1c1c1e): CVD ΔE ≥ 8.4 between neighbours; three light slots are
  under 3:1 against white, so every chart has a legend (two series or more) and a table.
- Buckets are made in SQL (`strftime` / `to_char`) at the zone's offset at the end of the
  period, so a period that crosses a daylight-saving change is off by an hour at its edges.
- A widget's `url` answer is its own small template (the page's template would need the whole
  page's context to render one block).
- Found in the browser: percent padding on bar bands made a 30-bar chart 6,500 px wide, and
  the period filter's min-content width widened the phone layout (fixed with
  `contain: inline-size`).

### Actions next to Filament's

Filament's actions (https://filamentphp.com/docs/5.x/actions/overview) as the yardstick, the
"A" after the widgets.

- [x] `action_sheet`: a button, a sheet and a form sent with htmx; a 422 shows its errors in
  the sheet, a success closes and resets it; `method`, `danger`, `target`/`swap`,
  `enctype`, `modal_icon`.
- [x] Sheets: `slide_over`, `width` (`sm`/`md`/`lg`/`xl`), `icon`; `confirm(icon=…,
  modal_icon="warning")`.
- [x] Buttons: `icon_button`; `icon`, `badge`, `key`, `disabled_reason` on `button`,
  `link_button` (`new_tab`) and `open_button`; tooltips from `data-rx-tip`; eight new icons
  (`edit`, `more`, `download`, `external`, `refresh`, `search`, `settings`, `box`).
- [x] Keyboard shortcuts: `data-rx-key` (`mod` is ⌘ on a Mac, Ctrl elsewhere), with
  `aria-keyshortcuts`.
- [x] Example: shop's admin products (an "Adjust stock" action, edit and view icon buttons,
  `n` for a new product, ⌘S / Ctrl+S on the form).
- [ ] Later: action groups (a `menu` already holds links and forms), an action that opens a
  wizard, and replicate/import/export actions (the grid has exports).

Notes:
- Tooltips are one element placed by renox-ui.js, not CSS `::before`: a pseudo-element was
  dimmed by a disabled button's opacity, could be clipped by a table's scroll box, and
  while hidden it still widened what scrolled (a phone table scrolled sideways).
- A disabled button with a reason uses `aria-disabled` (still focusable, so the reason can be
  read); a capture-phase click listener stops htmx and the kit from acting on it.
- An action sheet's form doesn't show `form_errors`: with one sheet per row, a plain post's
  errors would show in every sheet.

### The flagship example: examples/bikeshop (#231)

A bike shop with three stores (buy, rent, service, stores working together) on Pagila's shape
and volume, every page explaining itself ("About this page", `/about/pages`). One branch, one
PR, one commit (or more) per story; needs #244 (roles per store with dates).

- [x] #232 the app, its data model and a Pagila-sized seeder (`db:seed`, `demo:seed --size
  large`), with the RBAC + ABAC foundations (#239, #245) built first so every story uses them
- [x] #233 catalogue, product pages and full-text search
- [x] #234 cart, checkout, payments (Midtrans + webhook, a demo gateway) and counter sales
- [x] #235 bike rentals (two overlap checks, late fees, the ID check)
- [x] #236 the workshop (my bikes, bookings within capacity, the board, extra work approval)
- [x] #237 service plans on renox-billing (a named subscription per bike)
- [x] #238 customer accounts (account sections, notifications, language, privacy)
- [x] #239 stores, staff, roles per store, the audit log, the admin panel
- [x] #240 stock (one ledger), consignment, suppliers and purchasing
- [x] #241 the JSON API for kiosks and a customer app
- [x] #242 dashboards, report grids, exports and the monthly report
- [x] #245 multi-store operations: staff helping, placements, books between stores, settlements
- [ ] #243 translations, docs, CI, deploy and the final browser check: the README, llms.txt,
  the guides' links, `make:deploy`, `App::report`, error and maintenance pages, CI on
  PostgreSQL, tests/browser and tests/process, the large-seed query walk
  (`tests/queries.rs`) and a `CSP=strict` walk are in; the Spanish pass, the explanations'
  review and the owner's sign-off on screenshots are open

Notes:
- What the kit lacked was built in the example as "blocks" (the owner's decision, below);
  what Renox lacked or got wrong became issues #299–#319, worked around in the example.
- The large seed is fast enough to run in every CI test job (~7 s on SQLite, ~27 s on
  PostgreSQL for the query walk), so nothing is `#[ignore]`d.

## Decisions

- **Rate limits for queued channel and mail sends are separate methods** (#46):
  `App::channel_rate_limit(name, …)` and `App::mailer_rate_limit(name, …)`, not a builder
  returned by `channel`/`mailer` (their return types stay, for semver). The limit travels in
  the job payload because `Job::middleware` has no `AppState`; it reuses
  `Middleware::rate_limited` (a fixed window). A sliding window or token bucket stays a follow-up.
- **`rx-*` tags are the default way to write pages** in `rnx new`, the docs and the examples (owner, 2026-10-11, #384); kit macros stay supported and are what the tags compile to.
- **Browser tests on Node's own test runner and raw CDP** (#262, 2026-10): `tests/browser/`
  drives headless Chrome over the DevTools protocol with Node 24's `node:test` and global
  `WebSocket`, no npm packages, rather than a Rust CDP client (a large dev-dependency tree) or
  Playwright (npm, browsers to download). The harness is small enough to copy into an app.
  Process e2e (`tests/process/`) is Python like `tests/cli/smoke.py`.
- **Markdown in templates:** `pulldown-cmark` without its default features renders it; instead
  of a sanitizer (ammonia pulls in html5ever) raw HTML events become text and link and image
  URLs other than http(s), mailto, tel and relative ones become `#`. That is enough because
  pulldown-cmark escapes everything else it writes.

- **Nested forms:** a form whose names have a `[` is read by Renox's own small deserializer
  (`validation/nested.rs`) instead of `serde_html_form`, which has no nesting: a tree of text,
  parsed when the target type asks, empty values kept so rows keep their numbers. Errors are
  keyed with dots (`lines.0.name`, as `v.nested` already did), and templates accept either
  spelling. Plain forms keep `serde_html_form`, so nothing changes for them. Names deeper than
  32 levels are ignored.
- **Crates:** runtime code lives in one crate, `renox-core`, organised in modules and gated by cargo
  features where dependencies are heavy. Separate `renox-http`/`-db`/`-view` crates would all need
  `AppState` and `App` would need all of them, so splitting now only adds indirection. Revisit if
  compile times demand it.
- **Sessions:** by default stored in an encrypted, signed cookie (AES-256-GCM via `APP_KEY`), so
  M1 needed no database; keep those small (old input is capped in M13a, W5). Since M21g,
  `SESSION_DRIVER=database` keeps only an id in the cookie and the session in the `sessions`
  table (keyed by sha256(id), a new id at each login and logout).
- **Templates:** MiniJinja (runtime, overridable, reloadable). Askama may be offered later.
- **Auth sessions:** the session stores the user id and a fingerprint of the password hash, so a
  password change ends other sessions without a separate token column. "Remember me" makes the
  (encrypted cookie) session itself longer-lived.
- **Uploads as form fields:** multipart files are swapped for tokens that `Upload`'s `Deserialize`
  resolves from a thread-local during the (synchronous) deserialization, so a plain
  `#[derive(Deserialize)]` struct can hold files and share the validation path with text forms.
- **Storage:** the local disk is Renox's own code; S3 is an opt-in `s3` feature because object_store's
  AWS support pulls in reqwest and aws-lc-rs, which most apps on one server don't need.
- **i18n:** plain JSON files, loaded into memory at boot and reloaded in debug. Renox's own texts
  stay in code in English (so apps work without lang files) but every one of them can be overridden
  by key, which is also how other languages are added. Until M32 Indonesian was built in too; it
  was dropped when the owner made the whole repo English (examples use Spanish as the second
  language).
- **Live reload polls file times** (500 ms) instead of using a watcher, avoiding the watcher-event
  pitfalls `rnx serve` hit, and is only compiled into responses when `APP_ENV=local` with debug on.
- **HTMX validation errors:** returned as 422 JSON and placed by the bundled script, rather than
  re-rendering a form fragment. It works for any form without a per-form partial, and the form
  keeps the user's input, focus and Alpine state.
- **Migrations:** Renox runs its own migrator (table `renox_migrations`) instead of sqlx's, to get
  Laravel-style batches and module-owned migrations. Migrations are compiled into the app, so the
  app binary runs them; `rnx` forwards to it.
- **Models:** values are bound through `DbValue`/`ToDbValue` and rows decoded with sqlx, so the
  derive only needs `renox` as a dependency.
- **Model keys (M22):** the key is the `id` field's type, `Model::Key` (sealed `ModelKey`: `i64`,
  `Ulid`, `Uuid`, `String`), not an attribute: the type is already written, and the compiler
  checks it. The column is always `id`. An empty key (`0`, nil, `""`) means "not saved";
  ULIDs and UUID v7s are made in Rust on insert, so the row's key is known before the write.
- **Databases:** SQLite first (one file, no server, WAL; enough for one server). PostgreSQL (M9) is
  an opt-in `postgres` feature with the same `Db` API; one app uses one database, picked from
  `DATABASE_URL`. sqlx's `Any` driver was not chosen as the plan because
  it supports fewer types (e.g. chrono timestamps) than the typed pools.
- **Named routes:** implemented in Renox; axum does not provide them.
- **Queue:** Renox's own queue in the app's database (tables `jobs`, `failed_jobs` and
  `job_batches`, SQLite or PostgreSQL with `FOR UPDATE SKIP LOCKED`), so no Redis is required.
  apalis was the plan, but its stable SQL backend needs sqlx 0.8 (which can't link next to our 0.9)
  and the 0.9 backend is still a release candidate; a small queue on our own pool also keeps
  dispatch, retries and the `queue:*` commands Laravel-like.
- **Scheduler and workers run inside `serve`** by default, keeping deploys to one process. Running
  several instances share the database: each scheduled run is claimed in the `cache` table first,
  so only one instance runs it (since M9b), and queue workers are safe to run anywhere.
- **Relations:** Rust has no runtime reflection, so there is no full Eloquent. `derive(Model)` covers
  CRUD; relations are explicit methods for one row and loaders for a page of rows
  (`relations::belongs_to`, `has_many`, `Pivot`, M15a; `Morph`, M19b; `count_many`/`sum_many`;
  one query each); joins and reports use `renox::db::sql(…).fetch_as` (portable) or sqlx
  directly through `db.sqlite()` / `db.postgres()`.
- **Service container:** replaced by typed `AppState` and extractors.
- **No REPL:** `rnx db:shell`, and app commands (`App::command`, M14a) instead of Tinker.
- **Social login (#147) links accounts only by a verified address**, and never to an account
  whose own address is unverified (someone may have registered it in another person's name);
  a user it makes has an empty `password` (no column, no flag: no typed password matches it,
  `User::has_password` is false), sets one on `/account` without a current one, and proves who
  they are with a linked provider where others type their password. Provider tokens aren't
  stored: the crate is for logging in, not for calling the providers' APIs.
- **The admin panel (#148) is declared at run time, without a proc macro:** a resource is a
  trait impl naming its model and its form type; the form's `Validate` rules check it
  (`Valid<T>`, run after the panel's authorization), and `fill` copies it into the record.
  Nobody may open the panel until `Admin::authorize` says who may; inside it, every page and
  action asks the model's `Policy` with Laravel's ability names, with a blank
  (`Default`) record for the list-level questions. Pages are the crate's templates, replaced
  by app files of the same name, rather than generated code: `make:module --resource` stays
  the way to own the pages.
- **Billing (#155) uses Stripe and Xendit**, not Midtrans, for Indonesia: Xendit has recurring
  plans that keep the customer's card or e-wallet and charge each cycle themselves, where
  Midtrans' subscriptions need a saved card token the app collects first. Plans are declared in
  code (no plans table), amounts in the currency's smallest unit; Stripe charges the Price
  named for the plan (`STRIPE_PRICE_<KEY>`), Xendit the plan's amount. Subscriptions arrive by
  webhook (the checkout makes nothing): the subscription carries the owner, name and plan as
  metadata, so any event can make or find its row, and each row keeps the newest gateway
  event's time (`synced_at`) so a late, older event changes nothing. Xendit can't prorate,
  change an interval or resume: canceling stops its charges at once and access lasts to the
  period's end.
- **Roles per branch (#244) scope the assignment, not the role:** a role grants the same
  permissions everywhere; `role_user` rows gain `scope_type` (the model's table) + `scope_id`
  (its key as text) and optional `starts_at`/`ends_at`. A global row has empty strings, not
  NULLs, so (role, user, scope) stays unique on both databases and old rows need no change.
  The auth middleware still loads the grants once per request (every assignment not ended
  yet), and each check filters them by `permissions::set_scope` (read from `renox::context`)
  and `db::now()` at check time, since the app's scope middleware runs after the auth one and
  `TestApp::travel` must reach the dates. Ended rows are ignored, and pruned by
  `permissions:prune`.

- **The flagship example's missing UI components live in the example** (#231, the owner,
  2026-10-06): galleries, a range slider, a quantity stepper, a keypad, a kanban board, a month
  calendar, an availability timeline, a date-time range, a date picker with blocked days,
  swatches, a timeline and a plan comparison are examples/bikeshop's "blocks"
  (`resources/views/blocks/`, `public/blocks/`), written like a library on the kit's tokens so
  they can move to a crate later; Renox's kit isn't changed for them.
- **Two examples: the bike shop and hello** (#351, the owner, 2026-10-08): the sixteen
  examples became two, examples/bikeshop (the one complete use case) and examples/hello (the
  smallest app: the quick start and the README's GIF). What the other fifteen showed and the
  bike shop lacked moved into it first: each store's page on its own host (`Routes::domain`),
  `/about/fields` (every input ↔ Rust ↔ SQLite ↔ PostgreSQL, files public and private, S3 in
  CI) and `/about/htmx` (the htmx recipes, live). The data grid's pages that the browser tests
  drive moved into tests/browser/fixture (`/grid`). APIs no example shows any more (default
  scopes, model hooks, `gate_before`, job chains, cursor pagination…) are shown by the guides'
  doctests and tested in `crates/renox/tests/it`. The milestone notes above keep the old
  examples' names: they are history.

- **Components as tags** (#372, 2026-10): a small compiler in renox-core (`components/`) rewrites
  `<rx-…>`/`<app-…>` tags and `rx-if`/`rx-else`/`rx-for` to MiniJinja in the loader (MiniJinja
  has no source hook); a hand-written scanner, no new dependency (html5ever would rebuild the
  document, quick-xml fails on plain HTML); contracts are a static Rust table so `view:check`,
  editor data and plugins can read them; errors carry `file:line` with a "did you mean"; generated
  code never adds a newline, so runtime errors keep their line; plugin templates (`add_template`)
  are not compiled yet (#376); `rx-else-if` and route arguments are left out on purpose.
- **Live components** (#386, 2026-10, reverses "not planned" for Livewire; Inertia stays out):
  `renox::live_component` (`LiveComponent` trait, `#[renox::live_component]` on the impl block
  because a derive cannot see the methods, `LiveContext`), `POST /_renox/live/{component}/{action}`
  merged next to the grid's router (session, auth, CSRF, view and maintenance layers; no
  `App::layer` layers or throttle in v1). The state travels in a signed, not encrypted, snapshot
  (HMAC-SHA256 keyed from `APP_KEY`, `LIVE_SNAPSHOT_MAX_SIZE`, 64 KB), so secrets stay out of
  fields. The browser side is one classic script (`renox-live.js`) over htmx plus idiomorph 0.7.3
  (vendored), so CSRF, toasts and `HX-*` headers work unchanged. Names starting with `_` are
  reserved (`_refresh` is built in). Left out of v1: nested components, files, `rx-click`
  modifiers; the `<live-…>` tag follows #372. Guide: docs/live.md.
- **Typed view contexts** (#383, the owner approved them on 2026-10-11 in #487, after looking at
  what #372's contracts and #377's route checks already catch): `#[derive(ViewData)]` with
  `#[view(template = "…")]` implements `renox::view::ViewData` (`TEMPLATE`, `FIELDS`: the
  serialized names, so `#[serde(rename)]` counts and `skip` fields don't; `flatten` and
  `rename_all` are compile errors). `View::typed(page)` takes the template from the type (not
  `View::typed("x.html", page)`), so `view:check` can find it; a struct is checked only once
  registered (`App::view_data::<T>()`, `Registry::view_data`). The check is
  `Template::undeclared_variables(false)` on what MiniJinja loads (after the `rx-*` compiler)
  minus the fields, the request globals, the environment's globals and functions, the
  `App::share` keys (and the names the template defines, which MiniJinja already leaves out):
  `"{var}" isn't a field of {Type}`. It doesn't follow `extends`, `include` or macros, so a
  variable used only in a layout or a partial is not checked (accepted false negatives).
  `context!` views stay as they are. Shown on the bike shop's supplier list.
- **The dashboard standard** (#24, Renoxium, 2026-10-15): a dashboard app (a back office, an
  admin panel, a daily workspace with a sidebar) uses `<rx-shell fill>`. On a desktop screen
  the document never scrolls, every panel scrolls on its own, and the panels fill the screen
  (a grid or one `rx-fill` element takes the height left; an empty panel holds its empty
  state). Tablets and phones scroll the page, and `rx-fill` keeps a minimum height there.
  It extends `rx-shell` with an opt-in prop rather than adding a second shell: the structure
  is the same and only what scrolls changes, so the phone layout, the theme and the sidebar
  stay in one place, and plain `rx-shell` (document scroll) stays as it was (semver). The
  rule is written into llms.txt and the new apps' AGENTS.md so agents follow it.
- **Display blocks: status stepper and week calendar** (#55, part 3 of #32): `status_stepper` and
  `week_calendar` in renox-blocks, with tags `<rx-status-stepper>` and `<rx-week-calendar>`. The
  stepper is CSS only; its terminal steps are alternative endings that never count as done unless
  current. The week calendar's layout (cells, lanes for overlapping events, free slots, RFC 3339
  times for a slot) is worked out in Rust (`renox_week_grid`, a template function), not in the
  template, and reads times through the `date` filter, so it follows `APP_TIMEZONE`: there is
  no per-request time zone in Renox. A free slot posts with `hx-post` and a static `hx-vals`
  JSON, not `hx-vals='js:…'` (CSP strict). Drag-to-reschedule stays out of scope.

- **Company sign-in in `renox-oauth`** (#51): the ID token of `Microsoft` is read from the token
  endpoint's answer and its signature is not verified (OpenID Connect Core §3.1.3.7: received
  directly over TLS), so no JWT/RSA crate is added; `iss`, `aud`, `exp` and `tid` are checked.
  Personal Microsoft accounts are always refused, and the email is verified only for a specific
  tenant. Group/role mapping, SAML and a per-installation resolver for the settings (as in #44)
  are follow-ups.

## Not planned

Kept out on purpose, so the framework stays small; some are good candidates for separate crates:

- Eloquent-style relations resolved at run time (`$post->comments`): Rust has no reflection, and
  hidden queries are where N+1 problems come from. Use the explicit loaders instead.
- A schema builder for migrations: migrations are plain SQL, one file per database when they
  differ.
- Redis (queue, cache, sessions): the app's database covers them; `CACHE_STORE=database` for
  several servers.
- WebSockets and broadcasting.
- Inertia-style SPAs (a JSON API covers them). Livewire-style live components were here and
  are done: see the Decisions.
- OAuth/social login and two-factor authentication (plugin candidates).
- A Node/Vite build pipeline: htmx and Alpine are bundled, and Tailwind runs through its
  standalone CLI (`rnx new --tailwind`, `rnx tailwind`, M21e). Other front-end tooling is the
  app's own.
