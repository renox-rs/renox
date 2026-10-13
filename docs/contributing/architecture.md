# Architecture and the decisions behind it

> The request pipeline, the key decisions and the database layer. (Section 3 of the old CLAUDE.md.)
> Start at [CLAUDE.md](../../CLAUDE.md).

## 3.1 Request pipeline (outermost first; `build_router` in app.rs)
1. `security::middleware`: resolves `ClientIp`, puts a `CspNonce` in extensions, adds nosniff /
   Referrer-Policy / X-Frame-Options / HSTS / CSP to every response unless the handler set them
   (policy built once at boot in `security::Security`). Outermost so even the next layer's 413
   gets the headers.
2. `method::middleware`: method spoofing (`_method=PUT|PATCH|DELETE` in a POST, or
   `X-HTTP-Method-Override`). It wraps the whole router via
   `Router::new().fallback_service(layer(router))`, because route layers run after axum has
   matched the method.
3. `request_id::middleware` (keeps an acceptable incoming `X-Request-Id` or makes one; sets it
   on the request, the response and as the `RequestId` extension), then `TraceLayer` (span
   `request` with id, method, uri, client IP; `LOG_FORMAT=json` prints it as `span`).
4. `DefaultBodyLimit` (`UPLOAD_MAX_SIZE`).
5. Merged at this level, so they skip everything below (sessions, maintenance): `assets::router()`
   (`/_renox/*.js`), `/health`, `/robots.txt` (unless `public/robots.txt` exists), `/favicon.ico`
   (204 unless `public/favicon.ico` exists), `/_renox/live`
   (debug + local only) and the local disk's public files (`/storage/...`, sandboxed headers).
6. `inspector` (only with debug + local: records the request's status, time, view and SQL via
   `capture_queries`, skipping `/_renox/*`) → `context` (`renox::context`: a fresh task-local context per request holding the `AppState`
   for `context::app()`, and the `RequestInfo` error reports read; jobs, scheduled tasks and app commands get one too via `scope_app`) → `session` (cookie or database driver) → `i18n` (`RequestLocale`: session `_locale`, else
   `Accept-Language` with `App::detect_locale` (adds `Vary`), else `APP_LOCALE`) → `auth` (loads the user once from session or `Authorization: Bearer`, with the
   token's abilities, and the user's roles/permissions when the `Permissions` module is on;
   inserts `CurrentUser` and `AppState` into extensions) → `csrf` → `view` (renders `View`s and error
   pages; `ValidationError` → redirect back for plain forms) → `maintenance` (503 while
   `storage/framework/down` exists; inside `view` so the 503 uses the error template) → `guard`
   (a handler panic or a run past `REQUEST_TIMEOUT` becomes a 500 with the error page).
7. `App::layer` layers (first added = outermost), around the modules' routes only.
8. Routes. Also inside the layers: `/_renox/files` (storage), `/_renox/grid/{grid}/prefs` (data
   grid preferences), `POST /_renox/live/{component}/{action}` (live components: session,
   auth and CSRF apply as on any page), `/_renox/mail` and
   `/_renox/debug` (only with `APP_DEBUG`; the inspector answers 404 unless also local), and the fallback: `public/` (`ServeDir`, or the embedded files) with a 404.

With `Routes::domain`, `build_router` runs once per domain pattern (each with the whole stack
above and its own fallback), and `domain::dispatch` picks one router per request by `Host`.

Because `AppState` and `CurrentUser` are in request extensions, guards (`require_auth`, …) are
plain `from_fn` middlewares with no state parameter and can be added from `Module::routes()`.

## 3.2 Key decisions (also in ROADMAP "Decisions")
- **One runtime crate (`renox-core`)**, not renox-http/-db/-view: those would all need `AppState`
  and `App` would need all of them (circular). `renox` is a thin facade.
- **Cargo features:** `renox` defaults to `fake`, `http` (`renox::http` sends real requests
  through reqwest; the test fake works without it) and `server-events` (needs `http`); optional
  `postgres`, `s3`, `uuid`, `xlsx` (Excel exports of data grids). renox-core is `default-features = false` in the workspace deps; the `renox` crate owns
  the defaults.
- **Sessions are an encrypted, signed cookie by default** (`cookie` PrivateJar, key derived from
  `APP_KEY`), so no DB is needed; keep that small (a warning is logged over 4 KB).
  `SESSION_DRIVER=database` (M21g) puts only an id in the cookie and the session in `sessions`
  (keyed by sha256(id), a new id at each login and logout). Flash data lives
  one request. A per-session lifetime override powers "remember me". Logout ends this device
  only (its session id goes to the `revoked_sessions` denylist, so a copied cookie dies too);
  `logout_other_devices` / `User::revoke_sessions` bump `users.sessions_revoked_at`, checked on
  every request, to end the user's other sessions.
- **Auth sessions store the user id + a fingerprint of the password hash** (no remember-token
  column): changing the password logs out other sessions.
- **Views: MiniJinja** (runtime, overridable, autoreload in debug via `minijinja-autoreload`).
  App templates override built-ins by file name (loader checks `VIEWS_PATH` first, then `BUILTIN`).
- **HTML escaping uses a custom formatter** (`view.rs::format_value`): escapes `& < > " '` but not
  `/` (MiniJinja's default escapes `/` as `&#x2f;`, which uglified URLs in pages and mail).
- **The app binary is its own CLI** (like artisan): `my-app migrate|migrate:rollback|migrate:fresh|
  migrate:status|db:seed|queue:work|queue:failed|queue:retry|queue:flush|queue:forget|
  queue:prune-failed|queue:prune-batches|webhook:failed|webhook:retry|cache:prune|session:prune|
  ui:publish|schedule:list|schedule:run|schedule:work|route:list|db:shell|db:check|db:diff|
  view:check|view:data|down|up|help`, default `serve` (modules add more: `tokens:prune` and `notifications:prune` from Auth, `audit:prune` from Audit),
  plus the app's own commands (`App::command`, or `App::typed_command` for a clap `AppCommand`;
  names can't clash with built-ins). Migrations and
  jobs are compiled into the app, so only the app can run them. `rnx <anything unknown>` forwards
  to `cargo run --quiet -- <args>`.
- **Own migrator** (table `renox_migrations`, Laravel-style batches) instead of sqlx's, to support
  batches and module-owned migrations. `migrations!()` embeds `*.up.sql`/`*.down.sql` (or plain
  `*.sql`, not reversible). Apps need `build.rs` with `cargo:rerun-if-changed=migrations` so new
  files are picked up (`rnx new` writes it).
- **Models:** `#[derive(Model)]` generates `impl ::renox::db::Model` (and `FromRow`) using
  `::renox::…` paths; values go through `DbValue`/`ToDbValue`, rows decode via `Row::try_get`, so
  apps don't need sqlx directly. The primary key column is always `id`; its type is the key
  (`Model::Key`, sealed `ModelKey`: `i64`, `Ulid`, `Uuid`, `String`, M22); an empty key (`0`,
  nil, `""`) = unsaved, ULIDs/UUID v7s are made on insert. Table name =
  snake_case struct name (no pluralisation in the derive: not every language plurals with
  "s"); the generators write the plural explicitly (`make:model`, `--resource`: the model's
  name through `scaffold::plural`, #127), as the docs and examples have it. The query
  builder validates column names against `COLUMNS` and operators against a whitelist, so SQL
  injection via names is an error.
- **Relations are explicit loaders, no lazy relations** (`db::relations`: `belongs_to`,
  `has_many`, `Pivot`, `Morph`; each loads a page's related rows in one query). See
  docs/relations.md.
- **Model hooks are opt-in:** `#[model(hooks)]` makes the derive forward `Model::saving/saved/
  deleting/deleted` to `impl ModelHooks`. Only `save`, `insert` (and `create`), `save_only`,
  `save_changes`, `delete` and `force_delete` call them; bulk query methods never do. Keep it that way (documented).
- **Validation:** fluent rules in `impl Validate`, or `#[derive(Validate)]` (M25:
  each `#[validate(item)]` becomes a call on the field's rules, so every `Field` rule works;
  `label` goes first; `each`/`distinct`/`rename` are special; `#[validate(hooks)]` forwards
  to `ValidateHooks`). `Valid<T>` handles form, JSON
  and GET query. HTMX/JSON failures → `422 {"message","errors"}`; the bundled `renox.js` places
  errors next to inputs (`data-error-for` slots or inserted `<p class="error">`), sets
  `aria-invalid`, focuses the first invalid input in *page* order. Plain posts → 303 back with
  errors + old input flashed (never passwords).
- **Queue is Renox's own** (`jobs`, `failed_jobs`, `job_batches`, unix-second integers) on SQLite
  and PostgreSQL; chains ride in `jobs.chain`, batches count in `job_batches` inside the same
  transaction that finishes a job (`worker.rs::record`), unique claims are `renox:unique:*` cache
  rows, encrypted payloads start with `enc:`;
  PostgreSQL workers reserve with `FOR UPDATE SKIP LOCKED`. apalis was the plan but its stable SQL
  backend needs sqlx 0.8 (can't link next to our 0.9: both link `libsqlite3-sys`).
- **Workers and scheduler run inside `serve`** by default (single-process deploys). Several
  instances may share one database: every scheduled run is claimed first (`schedule::claim`:
  insert `renox:schedule:<task>:<slot>` into `cache` with `ON CONFLICT DO NOTHING`, kept for the
  interval + 1 min, or an hour for cron-style tasks such as `daily_at`; expired claims are pruned at most once a minute).
- **Single-file deploys:** `App::embed(renox::embedded!())` bakes views, lang files and `public/`
  into the binary; they're used only when `APP_DEBUG` is off (debug keeps disk + live reload).
  New built-in behaviour that reads `VIEWS_PATH`/`LANG_PATH`/`PUBLIC_PATH` must also handle the
  embedded source.
- **Mail:** lettre with rustls (no OpenSSL). Drivers `smtp`, `log` (default), `memory` (tests).
- **Uploads are form fields:** `Valid<T>` turns multipart files into tokens that `Upload`'s
  `Deserialize` resolves from a thread-local during the synchronous serde pass (`upload.rs`), so
  `struct Form { photo: Option<Upload> }` works with the normal validation path. `image()`/`mimes()`
  sniff the content; stored names are random with an extension from the content.
- **S3 is the opt-in `s3` feature** (object_store pulls reqwest + aws-lc-rs). It is tested against
  SeaweedFS in the `s3` CI job (`crates/renox/tests/it/s3.rs`).
- **No C crypto in default builds:** reqwest uses `rustls-no-provider` and `http.rs` installs
  rustls' ring provider when it builds the client, which lettre uses too. CI checks `cargo tree -p hello -e normal -i aws-lc-rs` is
  empty. `deny.toml` doesn't ban aws-lc, since the `s3` feature brings it.

## 3.3 The database layer (SQLite and PostgreSQL)
SQLite is the default; PostgreSQL (the `postgres` feature, shipped in M9b) is for apps that
outgrow one server. `Db` is Renox's own type (`db/conn.rs`): a private enum over `SqlitePool` and
`PgPool`, chosen by `DATABASE_URL`'s scheme in `db::connect`. User-facing guide: docs/postgresql.md.
- All framework SQL goes through `crate::db::sql("… ? …").bind(v)` and `.fetch_all/
  fetch_optional/fetch_one/execute/scalar/scalar_optional/scalars/fetch_as(executor)`; migration
  files go through `db::script` (multi-statement, no params). Never call `sqlx::query` on `&Db`.
- Executors are `&Db` or `&mut Transaction` (trait `db::Executor`; `db.begin()` returns
  `Transaction`, pass `&mut tx`, **not** `&mut *tx`). Models take `E: Executor<'c>`. Statements
  that run several times on one executor use `Conn::reborrow()`.
- Rows are `db::Row` (`try_get::<T>(name_or_index)`, `columns()`); `T: FromDb` means "decodes on
  every enabled backend". `from_row` lives in `db::FromRow` (supertrait of `Model`).
- Always write `?` placeholders; `numbered_placeholders` turns them into `$n` on PostgreSQL
  (quotes/comments skipped). Engine-specific code matches on `db.dialect()`, or on
  `db.sqlite()` / `db.postgres()` for raw sqlx (see `migrate.rs` `drop_all_*`, `shell.rs` cells).
- `DbValue` has typed variants (Bool, DateTime, NaiveDateTime, Date, Time, Json, Uuid). SQLite
  binds them via `DbValue::for_sqlite()` (0/1, text formats); PostgreSQL binds them typed, and
  `Null` as an OID-0 `UntypedNull`.
- Query conditions are `query::Filter` values (Sql, Like, JsonIn, Group, Not, InQuery) rendered
  per dialect at execution, with binds kept in render order. `like` → `ILIKE` on PostgreSQL;
  `OFFSET` without `LIMIT -1` there. Aggregate sums are cast (`db::Number`, sealed) so
  PostgreSQL's NUMERIC doesn't leak.
- `Db` (and every `Transaction` it begins) carries the `APP_KEY`-derived key (`with_key` at
  boot); `Sql::execute`/`fetch_*` seal `DbValue::Encrypted` with it and give each `Row` the key
  for `Encrypted` columns. A new place that makes rows or binds values must keep that.
- Public database APIs return `db::DbError`, never `sqlx::Error`; macros reach sqlx through the
  hidden `renox::__sqlx`.
- Emails: `auth::user::normalize_email` (trim + lowercase) on register, lookup, reset and the
  registration `unique` check; PostgreSQL has a unique index on `lower(email)`.
- `cfg(feature = "postgres")` arms: check both `cargo clippy --all-targets` and `--all-features`.
