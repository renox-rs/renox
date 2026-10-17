# Configuration (env vars)

> Every environment variable, parsed in `crates/renox-core/src/config.rs`. (Section 5 of the old CLAUDE.md.)
> Start at [CLAUDE.md](../../CLAUDE.md).

Parsed in `crates/renox-core/src/config.rs`; defaults in parentheses.
- **App:** `APP_NAME` (Renox), `APP_ENV` (local; accepts local|dev|development,
  testing|test, production|prod; anything else fails at boot), `APP_DEBUG` (on in local),
  `APP_URL`, `APP_KEY` (required in production; `base64:…`, `rnx key:generate`), `APP_HOST`
  (127.0.0.1, an IP), `APP_PORT` (3000), `APP_LOCALE` (en; English is built in, other locales come from the app's `lang/*.json`),
  `APP_FALLBACK_LOCALE` (en), `APP_CURRENCY` (USD; the `money` filter, the kit's money entries/charts and the grid's money columns, all in the smallest unit), `APP_TIMEZONE` (`UTC`, an offset like `+07:00`, or an IANA name like
  `Asia/Jakarta`, with DST).
- **Paths:** `VIEWS_PATH` (resources/views), `LANG_PATH` (resources/lang), `PUBLIC_PATH`
  (public), `STORAGE_PATH` (storage; holds `framework/down` for maintenance mode and `app/` for the
  local disk). Paths are relative to the working directory: run apps from their own directory.
- **Sessions:** `SESSION_DRIVER` (cookie|database), `SESSION_LIFETIME` (minutes, 120),
  `SESSION_COOKIE` (renox_session),
  `REMEMBER_LIFETIME` (minutes, 43200).
- **Database:** `DATABASE_URL` (sqlite://storage/app.db, or `postgres://…` with the `postgres`
  feature; other schemes fail), `DATABASE_POOL_SIZE` (8, at least 1), `DATABASE_ACQUIRE_TIMEOUT`
  (seconds, 5), `DATABASE_STATEMENT_TIMEOUT` (seconds, 30, PostgreSQL; 0 = none).
- **Requests:** `REQUEST_TIMEOUT` (seconds, 60; 0 = none), `UPLOAD_MAX_SIZE` (MB, 10),
  `TRUSTED_PROXIES` (addresses, CIDR ranges or `*`), `TRUSTED_HOSTS` (host names, `*.example.com`;
  other hosts get a 400; empty: any), `CSP` (relaxed|strict|off, also `false`/`none`
  for off; relaxed is Renoxium's chosen default), `LIVE_SNAPSHOT_MAX_SIZE` (KB, 64: the
  largest live component state a request may carry).
- **Logs:** `RUST_LOG` (default by command: `info,renox=debug` for `serve`/`queue:work`/
  `schedule:work` with debug on, `info` for them without, `warn` for every other command), `LOG_FORMAT` (text|json; anything else fails at boot),
  `LOG_FILE` (append there instead of stdout, no ANSI colors; falls back to stdout if it can't
  be opened).
- **Mail:** `MAIL_MAILER` (log; smtp|log|memory), `MAIL_HOST`, `MAIL_PORT`, `MAIL_ENCRYPTION`
  (starttls; tls|starttls|none), `MAIL_USERNAME`, `MAIL_PASSWORD`, `MAIL_FROM_ADDRESS`,
  `MAIL_FROM_NAME`, `MAIL_TIMEOUT` (seconds, 10, the whole send), `MAIL_FAILOVER` (names of
  `App::mailer` mailers tried in order when the default fails; an unknown name fails at boot).
- **Background:** `QUEUE_WORKERS` (2; 0 = none in serve), `SCHEDULER` (true), `CACHE_STORE`
  (memory|database; database also shares throttles and the login lock between servers).
- **Outbound webhooks:** `WEBHOOK_DISABLE_AFTER` (20, at least 1: consecutive failed deliveries after
  which an endpoint is disabled).
- **Storage:** `STORAGE_DISK` (local|s3), `S3_BUCKET`, `S3_REGION`, `S3_ENDPOINT`,
  `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY`, `STORAGE_URL`.
- **SEO/analytics** (used in production only): `GOOGLE_SITE_VERIFICATION`, `GA4_MEASUREMENT_ID`,
  `GA4_API_SECRET`, `GTM_CONTAINER_ID`.
- **Social login** (`renox-oauth`, read with `config.var`): `GOOGLE_CLIENT_ID`/`_SECRET`,
  `GITHUB_CLIENT_ID`/`_SECRET`, `MICROSOFT_CLIENT_ID`/`_SECRET`, `MICROSOFT_TENANT` (a tenant id or
  domain; `organizations` by default).
- **App-specific:** anything else through `config.var(name)` (`config.vars` first, then the
  environment; empty counts as missing).
- **Settings kept in the database** are not environment variables: values an administrator
  changes without a redeploy (a company name, a number format, feature switches) are typed
  groups of the `Settings` module (docs/settings.md, `state.settings::<T>()`), seeded from a JSON
  file with `App::settings_from(path)`. They add no variable; sealed groups use `APP_KEY`.
- **Set by others:** `LISTEN_FDS`/`LISTEN_PID` (systemd socket activation; `serve` uses that
  socket). For `rnx` itself: `TAILWIND_BIN` (a Tailwind binary to use instead of the pinned
  download) and `RNX_CACHE_DIR` (where downloads are kept).
- **Tests only:** `TEST_DATABASE_URL` (read in `db/mod.rs`, env or `.env`; see §4.7) and
  `TEST_S3_*` (the S3 test).
