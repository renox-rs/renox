# Conventions

> The conventions every change follows: public API and docs, `Send` futures, templates, config, migrations, tests, forms, the CLI, docs and examples. (Sections 4.1 to 4.10 of the old CLAUDE.md.)
> Start at [CLAUDE.md](../../CLAUDE.md).

## 4.1 Public API and docs
- Laravel naming where it maps cleanly (route names like `password.reset`, `verification.notice`,
  commands like `queue:work`), Rust idioms otherwise.
- Every public item gets a doc comment (`#![warn(missing_docs)]` in the three library crates;
  clippy `-D warnings` fails without one). Doc examples are doctests: never write ```` ```ignore ````.
  renox is a dev-dependency of renox-core and renox-macros, so examples `use renox::prelude::*`
  as apps do. Hide setup with `# ` lines and wrap statements in
  `# async fn demo(..) -> Result { … # Ok(()) }`; examples that would start a server are `no_run`.
- Handlers return `renox::Result<T>`; any `anyhow`-compatible error converts with `?`.
- New public structs/enums that may grow get `#[non_exhaustive]` and a line in docs/stability.md
  (also keep its public-dependency list in sync when adding re-exports).
- When a public API changes, fix CHEATSHEET.md, llms.txt, the guides and the examples in the same
  PR (not README.md or CHANGELOG.md: §4.11).

## 4.2 Handler futures must be `Send`
axum needs `Send` handler futures, and rustc (issue #100013) can't prove it when a generic future
holds a closure over `&T`, or a generic iterator, across an `.await`. Doctests and plain tests
don't route handlers, so they miss it. Data APIs that take closures or iterators are plain `fn`s
returning `impl Future<Output = …> + Send + 'a` that read what they need first (`relations.rs`,
`Query::first_or_create`). Trait methods returning futures are declared `-> impl Future + Send`.
In Rust 2024 such a return type captures every lifetime in the arguments (`children: &[C]` too),
so a helper can't build a temporary `Vec` and pass `&tmp` to a loader: inline the query instead
(see `Morph::parents`).
**Add every new data API to `crates/renox/tests/it/send_handlers.rs`.** In middleware, don't keep
a closure borrowing `req` alive across `next.run(req).await` (scope it in a block).

## 4.3 Built-in templates, texts and template helpers
- Templates are compiled by `components::compile` in the loader (`view.rs`): tags become macro calls
  on `renox/ui.html`, and a new kit macro gets a `Contract` in `components/contracts.rs`. Plugin
  templates go through `view::add_template`, which compiles them too (never call
  `env.add_template` directly), and plugins register their own tags with
  `Registry::component`. Errors read `file:line: message`; line numbers are kept.
- Add the file under `crates/renox-core/views/…` **and** register it in `BUILTIN` in `view.rs`.
  Built-in names are prefixed `renox/` (e.g. `renox/auth/login.html`). Forgetting `BUILTIN` gives
  "template not found" at runtime only.
- Templates are semi-strict while debugging, and the whole test suite runs with `debug = true`,
  so built-in templates must only print defined values (`flash` is an object returning "" for
  missing keys).
- Template context: `merge_maps([shared, view ctx, globals])`; the **last** map wins (shared
  values, then the handler's, then Renox's globals).
- Auth pages/mails take a `text` object from `auth/module.rs::texts(&lang)`: the built-in
  English dictionary with the app's `renox.auth.*` translations on top. Auth handlers take a
  `Lang` extractor (background code uses `Lang::of(state, &state.config.locale)`).
- Money in templates is in the currency's smallest unit everywhere: the `money` filter,
  `entry(…, format="money")`, `chart(…, format="money")` and the grid's `Column::money` divide
  by `10^currency_decimals(code)` (`view_filters::money_divisor`; `divide_by=` overrides it,
  `divide_by=1` for whole units). `renox::format_money` (Rust) takes whole units. renox-admin's
  money fields show and take whole units and convert in `read_form` (#335).
- Validation messages live in `validation/messages.rs` (keys like `required`, `min.string`,
  `max.file`, `auth.failed`); apps override them with `renox.validation.<key>` and name fields with
  `renox.validation.attributes.<field>` (`messages::template_for`). Built-in labels for Renox's own
  forms use `Field::fallback_label`, so an app's attribute translation wins.
- Helpers that need the page's `app`/`request` (`seo()`, `page_url()`) are Rust functions
  registered per render, not MiniJinja macros: an imported macro can't see the caller's context.
  `page_url(n)` reads `request.query` through `minijinja::State::lookup`, which works inside
  imported macros. `can(ability, target)` reads `target._can` (from `auth::Can`), `can(gate)` asks
  a gate; `method_field('PUT')`.
- Analytics events live in the session (`_renox_analytics`) until the view middleware delivers
  them: htmx 2xx swap → `HX-Trigger` `{"renox:analytics":{"events":[…]}}` (merged with the
  handler's own), full page → `<meta name="renox-analytics">` in `renox_head()`, anything else
  (redirects) → kept for the next page.
- Whether a 500 page shows the error chain is decided per app (`ErrorPage::shown_detail(debug)`);
  there is no process-wide debug flag, so apps with and without debug can share a test binary.

## 4.4 Adding config
Add the field to `Config` (its doc names the env var), parse it in `Config::from_vars`, add a case
to the config tests there, give it a test-friendly value in `Default`, and document it in
`crates/renox-cli/stubs/env.stub` (and `examples/hello/.env.example` when the guestbook uses it).
App-specific settings need no field: `config.var(name)` reads `config.vars`, then the environment.
A setting that takes one of a few words (`SESSION_DRIVER=database`) is an enum made with
`setting_enum!` (lib.rs: `#[non_exhaustive]`, `as_str`, `Display`, `Serialize`, and
`parse`/`parse_as` that name the variable in errors), never a `String` compared in code.
Durations are `Duration` fields even when `.env` has minutes or seconds.

## 4.5 Migrations owned by the framework
Names start with `0001…` so they sort before app migrations (`2026…`). There are nineteen:
- Auth module (`auth/module.rs` `MIGRATIONS`): `00010101000000_create_users_table`,
  `…000001_create_password_reset_tokens_table`, `…000002_create_personal_access_tokens_table`,
  `…000003_create_notifications_table`, `…000004_add_sessions_revoked_at_to_users`,
  `…000005_add_abilities_to_personal_access_tokens`, `…000006_create_revoked_sessions_table`,
  `…000007_create_device_tokens_table` (tokens owned by a device, `auth/device.rs`).
- Permissions module (`auth/permissions.rs`): `00010101000500_create_roles_and_permissions_tables`
  (roles, permissions, permission_role, role_user) and `00010101000510_add_scope_to_role_user`
  (scope_type/scope_id, '' for global, and starts_at/ends_at; SQLite rebuilds the table to
  replace its UNIQUE, so it is a `Migration` literal with its own PostgreSQL down, #244).
- Audit module (`audit.rs`): `00010101000600_create_audit_logs_table`.
- Every app (registered in `App::boot`; eight, which tests/it/database.rs lists):
  `00010101000100_create_jobs_table`, `00010101000110_add_chains_and_batches_to_jobs` and
  `00010101000120_add_callback_of_to_jobs` (queue), `00010101000200_create_cache_table` (cache),
  `00010101000210_create_sessions_table` (session.rs, installed whatever `SESSION_DRIVER` is),
  `00010101000220_create_grid_preferences_table` (grid/mod.rs),
  `00010101000300_create_webhook_calls_table` and
  `00010101000301_store_webhook_payloads_as_bytes` (webhook.rs `MIGRATIONS`).

Each is `NAME.up.sql` (SQLite) + `NAME.postgres.up.sql` + a shared `NAME.down.sql`, included with
`db::framework_migration!(dir, name)`. PostgreSQL versions use BIGINT identity ids, BIGINT
integers and TIMESTAMPTZ dates. A migration that needs its own PostgreSQL `down` is written as a
`Migration` literal (see `webhook::MIGRATIONS`), since `framework_migration!` has none. Apps get
the same through `migrations!()` (`*.postgres.up.sql` / `*.sqlite.up.sql` overrides →
`Migration::up_for(dialect)`). Adding a framework migration changes migration counts asserted in
`crates/renox/tests/it/database.rs`.

**Schema changes vs pooled connections:** `Db` carries a `SchemaEpoch`; the migrator marks it once
per batch (each rollback step, `fresh`), and pools from `db::connect` drop connections opened
before it (`before_acquire`). Without it a pre-migration connection's `SELECT *` panicked in
sqlx-sqlite and returned no rows. Mark per batch, not per migration: per migration made the
PostgreSQL suite 2.5x slower (reconnects).

## 4.6 Background work and errors
- Framework rows in the `cache` table start with `renox:` (`Cache::flush` keeps them).
- `Error::permanent` marks errors the queue won't retry; `error::panic_message` formats caught
  panics. Jobs run in their own task, so a panic is a failed attempt.
- `App::listen` listeners run before modules' (modules register at boot).
- `Error`'s `Debug` is hand-written so `fn main() -> renox::Result` prints readable errors.
- Throttle ids come from the covered routes, so they're the same in every process.

## 4.7 Tests
- Tests using the macros must live in `crates/renox/tests/it/` (the macros emit `::renox::` paths)
  as a module of `it/main.rs`. **Don't add new top-level `tests/*.rs` files**: each becomes its own
  binary linked against sqlx/axum, which made builds 4x slower before they were merged. Core-only
  tests go in `crates/renox-core/tests/` or unit tests.
- Prefer `renox::testing::TestApp` (keeps cookies, sends CSRF, has assertions); older tests drive
  `kernel.router()` with `tower::ServiceExt::oneshot` and keep the session cookie by hand (update
  it from **every** response: flashed errors and old input live in the redirect's cookie). For
  CSRF there, add a route returning `session.token()`; `/login` redirects logged-in users.
- `#[renox::test]` replaces `#[tokio::test]`.
- `TestApp` does **not** read `.env` (except `TEST_DATABASE_URL`): it starts from
  `Config::default()` (en locale, memory mail, in-memory DB, no workers, no scheduler, debug on)
  plus a temp storage dir; change it with `TestApp::with_config`. Set `debug: false` explicitly to
  test production behaviour.
- Build configs by mutation (`let mut c = Config::default(); c.x = …;`); `Config` is
  `#[non_exhaustive]`. Test env parsing through `Config::from_vars(|name| …)`, never by setting
  env vars (tests run in parallel).
- `Config::default()` keeps a 30 s acquire timeout (parallel tests open many SQLite files; the
  `.env` default is 5 s), while in-memory SQLite caps it at 2 s so a task waiting on its own
  transaction fails fast.
- `Kernel` helpers: `migrate()`, `run_jobs()` (drains the queue), `mailer().sent()` (memory
  driver), `state()`, `db()`, `worker(queues)`, `call(command, args)`.
- Macro misuse is tested with `compile_fail` doctests on `#[cfg(doctest)] struct
  MacroCompileErrors` in `crates/renox/src/lib.rs` (not trybuild, and not in renox-macros: a
  proc-macro crate can't export that struct).
- Timing-based tests need slack on PostgreSQL (a round trip is ~0.2 s under a parallel suite).
- **On PostgreSQL:** `TEST_DATABASE_URL` (env or `.env`, read in `db/mod.rs`) makes `db::connect`
  swap any in-memory SQLite URL for a fresh `renox_test_…` schema (pool capped at 3). Run:
  ```
  docker run -d --rm --name renox-pg --shm-size=512m -e POSTGRES_PASSWORD=postgres \
      -e POSTGRES_DB=renox_test -p 55432:5432 postgres:17-alpine
  TEST_DATABASE_URL=postgres://postgres:postgres@localhost:55432/renox_test \
      cargo test -p renox -p renox-core -p renox-cli -p bikeshop --features renox/postgres
  ```
  Not examples/hello (SQLite migrations); examples/bikeshop has both.
- **S3:** `cargo test -p renox --features s3 --test it s3` with `TEST_S3_ENDPOINT`,
  `TEST_S3_BUCKET`, `TEST_S3_ACCESS_KEY_ID`, `TEST_S3_SECRET_ACCESS_KEY` (without them the tests do
  nothing). The SeaweedFS commands are at the top of `it/s3.rs`. With the same variables,
  `cargo test -p bikeshop --features s3 --test fields` runs the bike shop's /about/fields files
  on that bucket (CI's `s3` job runs both).
- Don't run tests with `--release` (slow compile, no debug assertions).
- Run tests scoped while working (`cargo test -p renox --test it -- module::name`, `-p
  renox-core --lib path`, a plugin's `--test file name`), not the whole workspace: a full run
  rebuilds and fills the disk, and CI runs everything. `cargo test --doc -p renox
  MacroCompileErrors` runs the macros' `compile_fail` blocks; a `compile_fail` passes for any
  error, so check a new one fails with its own message.
- JavaScript (renox.js, renox-ui.js, renox-grid.js, editors.js) is tested in a browser:
  `tests/browser/run.sh [file…]` (§2). Processes (`serve` and signals, workers, the scheduler,
  `rnx serve`, every example served): `tests/process/run.sh [fixture|examples|postgres]`.
- A warning that matters is asserted: `crate::logs::capture()` in `crates/renox/tests/it/`
  (`crate::test_logs::capture()` in renox-core's unit tests) collects what the test's thread
  logs; check it with `logs.has(&[…])`.
- Coverage: the CI `coverage` job (every crate, SQLite + PostgreSQL merged, HTML artifact);
  locally see CONTRIBUTING.md "Coverage". Stable `llvm-cov` doesn't count doctests.

## 4.8 Forms and validation internals
- Forms are deserialized with `serde_html_form` (repeated names → `Vec`), or, when a name has
  a `[` (`lines[0][name]`), with `validation/nested.rs` (a tree of text, numbers parsed when
  asked for, "" → `None`, empty values kept so rows keep their numbers). The retry loop in
  `validation/extract.rs` first rewrites browser values (`coerce_browser_value`: checkbox
  `on`/missing → bool, datetime-local + `:00`), then uses placeholders (`PLACEHOLDERS`: an enum's
  first variant from the error's "expected one of", then `0`, `false`), so the other rules still
  run; rule errors on placeholder fields are dropped. `Parsed::Ok(T, Errors)` carries the parse
  errors. `Valid`'s logic is `validation::extract::validate_request` (with an extra-rules hook,
  used by `/register`).
- `Validator::rules_of` applies rules synchronously before the async DB checks, so `Valid<T>`
  doesn't require `T: Sync`.
- `User` selects `*` (`Model::SELECT_ALL`) and keeps unknown columns in `extra` (never
  password/sessions_revoked_at); `User::register` reads the row back.
- `#[derive(DbEnum)]` emits `::renox::__db_text_type!(T)`; that macro_rules is defined twice in
  renox-core (with/without `postgres`) so the sqlx impls match renox-core's features, not the
  app's. Don't use `#[cfg(feature)]` inside exported macros: it would test the caller's features.

## 4.9 The CLI (`rnx`)
- Apps are lib + bin: `src/lib.rs` has `pub fn app() -> App`, `main.rs` runs it, `tests/` boot it.
  `rnx make:module` registers modules in `src/lib.rs` (falls back to `main.rs` for older apps).
- `rnx make:job` / `make:command` insert their `app.job::<…>()` / `app.typed_command::<…>()` into the
  module's `fn register` (creating it before `fn routes`); `make:migration` bumps the timestamp
  past the newest migration.
- `tests/cli/run.sh [postgres]` makes an app with every generator and builds/tests it
  (`FROM_GIT=1 DOCKER=1` for the Docker job). **Add every new `make:*` there.**
  It also makes apps with the option combinations people use, with names before and after
  "renox" (#143): `atlas` (plain), `pulse` (`--notifications`, #151), `studio` (`--starter`, with the database) and, with
  sqlite, `site` (`--tailwind`: downloads the pinned Tailwind CLI with `curl`, so it needs
  the network) and `desk` (`--starter --tailwind`); each passes `cargo fmt --check`, clippy
  and its tests (the every-generator app skips clippy: its output is dead code until used).
  With SQLite, `tests/cli/smoke.py` serves the shop and starter apps and drives them over
  HTTP (#142; it reads their SQLite file, so not on PostgreSQL). On postgres,
  `E2E_POSTGRES=postgres://…:5432` (CI sets it) runs the apps' tests and commands against
  that server (each app in a fresh `renox_e2e_<app>` database, tests in `renox_test`);
  without it, build and lint only.
- Every `.rs` file a command writes or edits goes through `format::touched` (in
  `write_new` and the in-place edits of generate.rs, and `rnx new`'s stubs); `main` runs
  rustfmt on them at the end, one file at a time through stdin (with a path, rustfmt
  follows `mod` lines into the rest of the crate). New apps and generator output must pass
  `cargo fmt --check`: tests/cli/run.sh checks it, and new.rs has a test with names on both
  sides of `renox` (#124). A new place that writes Rust files must call `touched`.
- `rnx new` pins `rev` from `renox-cli/build.rs` (`git rev-parse HEAD`, else the cargo checkout
  directory's short rev). Testing it through `cargo install --git` needs the change committed,
  since that builds the committed tree.

## 4.10 Docs and examples for app authors and agents
- `README.md`: agents don't edit it (§4.11); Renoxium keeps it. What follows is for Renoxium.
  It is compiled (`ReadMe`): keep its Rust blocks complete. It's the front page, so it
  sells: tagline, why, GIF, 3-line quick start, a short taste, fold-out feature tour, comparison
  with Loco/Axum, then status. Keep claims true (checked against the code). It has
  the crates.io/docs.rs badges, installs with `cargo install renox-cli` (no `--version` since
  1.0.0, #139) and has a "Use Renox with Claude Code" section that clones the release's tag. Its docs links
  stay repository files (they work on GitHub and crates.io, and the site rewrites them to its
  own pages); the top and "Documentation" point readers at https://docs.renox.rs (#141),
  which is also the crates' `homepage`.
- `CHEATSHEET.md` is compiled: every ```rust block must build on its own (visible `use` lines, no
  `# ` hidden lines since GitHub shows them; define items only, no top-level statements, so the
  doctest's `main` does nothing). Check with `cargo test --doc -p renox`.
- There are two examples (#351, Renoxium's choice): examples/bikeshop, the one complete use
  case, and examples/hello, the smallest app (the quick start and the README's GIF). Both are
  workspace members with their own tests and a README.md; module docs name the `rnx make:*`
  commands that made them. A feature worth showing goes into the bike shop, as a page with its
  "About this page" entry (`/about/fields` and `/about/htmx` are references of that kind),
  not into a new example. Keep `llms.txt` and the two READMEs in step when adding or changing
  example files; a docs link to an example must point at a file that exists.
- **Write the 1.1 way, and teach only that** (Renoxium, 2026-10-11): new pages, guides,
  examples, stubs and generator output use `<rx-*>` / `<app-*>` tags (never a kit macro call
  when a tag exists; macros without a tag in `contracts.rs` stay macros), own components with
  `<rx-props>`, typed columns in queries (`Product::PRICE.lt(..)`, `order_by(Product::NAME)`;
  raw fragments stay raw), `#[model(form)]` instead of a hand-copied form struct,
  `#[derive(ViewData)]` / `View::typed` for a page's context, a live component
  (`#[renox::live_component]`, docs/live.md) for an interactive part whose state would
  otherwise need a route and `hx-target` glue per action (htmx stays right for fragments,
  boosting and stateless swaps), `make:migration --auto` after a model changes, and the checks:
  `rnx db:check` / `rnx view:check` and `TestApp::assert_models_match_schema` /
  `assert_views_compile` in an app's tests. The old forms stay in the API (semver) but are
  not shown as the way: don't add a macro-call or string-column example next to the new one.
- A route group's index is `.get("/", …)` (not `""`, which panics).
- Examples use `renox.workspace = true`, so their `make:deploy` Dockerfile builds only in a copy
  made by `rnx new` (the CI docker job covers that).
- Examples without a `.env` run with `APP_DEBUG` off, i.e. with the views embedded at build time:
  restart after editing templates.
- UI changes get a browser test in tests/browser (it replaced checking by hand; `run.sh <file>`
  runs one file). Found by hand earlier in the crud example (removed in #351): `hx-boost` on a whole section also
  boosts its edit links and delete forms (scope it to the page links), and boosted requests get
  full pages (by design, `Htmx::wants_fragment`), so pair them with `hx-select`.
- Under `CSP=strict`, Alpine's CSP build rejects statements in attributes (examples/hello's
  `@htmx:after-request="if ($event.detail.successful) $el.reset()"` threw "CSP Parser Error:
  Unexpected token: $el" when the form was sent, found by tests/browser). That's why relaxed is
  the default; strict apps move logic into `Alpine.data` in a nonce'd script, as hello now does
  (`x-data="guestbookForm" @htmx:after-request="clearOnSuccess"`). The handlers only run on
  their events, so a page that loads cleanly can still break: tests/browser/examples.test.mjs
  sends hello's and crud's forms under `CSP=strict`.
- `docs/assets/demo.gif` was made by driving examples/hello (`APP_LOCALE=en`, a fresh database)
  in headless Chrome over CDP (a 760 × 752 viewport with `Emulation.setScrollbarsHidden`,
  `Page.captureScreenshot` per typed character, `Input.insertText`), then composing the frames
  with Pillow (a browser bar on top, a caption bar under the page, one 128-colour palette for
  every frame, ~73 KB). Last recorded in M31 on the warm theme. Re-record it when the guestbook's
  look changes.
