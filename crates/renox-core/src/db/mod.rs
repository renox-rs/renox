//! Database access (SQLite, or PostgreSQL with the `postgres` feature): the
//! connection pool, raw SQL, models, queries, pagination, migrations and
//! factories.

pub(crate) mod auto_migration;
mod column;
mod conn;
mod encrypted;
mod error;
mod factory;
mod from_row;
mod json;
mod key;
mod migrate;
mod model;
mod paginate;
mod query;
mod query_log;
pub mod relations;
pub mod schema;
#[allow(dead_code)] // used by `db:check` (370.5)
pub(crate) mod schema_check;
pub(crate) mod schema_diff;
pub(crate) mod schema_sql;
pub mod search;
mod value;

use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use axum::extract::FromRef;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

pub use column::{Col, Comparable, Condition, IntoColumn};
pub use conn::Conn;
#[doc(hidden)]
pub use conn::bounds;
pub use conn::{Db, Dialect, Executor, FromDb, Row, RowIndex, Sql, Transaction, sql};
pub(crate) use conn::{RowInner, SchemaEpoch, script};
pub use encrypted::{Encrypted, Unsealed};
pub use error::DbError;
pub use factory::{Factory, FactoryBuilder};
pub use from_row::FromRow;
pub use json::Json;
pub use key::{InvalidUlid, ModelKey, Ulid};
pub use migrate::{Migration, MigrationStatus};
pub(crate) use migrate::{Migrator, framework_migration};
pub use model::{Model, ModelHooks};
pub use paginate::{CursorPage, Page, Paginated, SimplePage};
pub use query::{Number, Query};
pub use query_log::capture_queries;
pub use schema::{ColumnKind, ColumnType, ModelColumn, ModelIndex};
/// sqlx, for what Renox's own API doesn't cover: `Db::sqlite()`, `Db::postgres()`,
/// `Row::sqlite()`, `Row::postgres()` and `DbError::sqlx()` hand out its types.
/// sqlx may move to a new version in a minor Renox release; see docs/stability.md.
pub use sqlx;
pub use value::{DbValue, ToDbValue};

use crate::{AppState, Config};

/// The timestamp type for `created_at`, `updated_at` and `deleted_at`.
pub type DateTime = chrono::DateTime<chrono::Utc>;

/// Unix seconds (the queue's and webhooks' columns) as a `DateTime`.
pub(crate) fn from_unix(seconds: i64) -> DateTime {
    chrono::DateTime::from_timestamp(seconds, 0).unwrap_or_default()
}

/// The current time, as stored in timestamps: to the microsecond, which is
/// what PostgreSQL keeps, so a saved model equals the same row read back.
pub fn now() -> DateTime {
    use chrono::SubsecRound;
    chrono::DateTime::<chrono::Utc>::from(crate::clock::system_now()).trunc_subsecs(6)
}

impl FromRef<AppState> for Db {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

/// Opens the pool for `DATABASE_URL`. `postgres://` and `postgresql://` URLs
/// need the `postgres` feature, `sqlite:` URLs open SQLite, and any other
/// scheme is refused (so a typo doesn't become a SQLite file).
///
/// An in-memory SQLite database (what tests use) is swapped for a fresh
/// schema in `TEST_DATABASE_URL` when that is set to a PostgreSQL URL (in the
/// environment or in `.env`), so a test suite can run against PostgreSQL
/// unchanged.
pub(crate) async fn connect(config: &Config) -> anyhow::Result<Db> {
    let url = &config.database_url;
    if config.database_pool_size == 0 {
        anyhow::bail!("DATABASE_POOL_SIZE must be at least 1");
    }
    if is_postgres(url) {
        return connect_postgres(url, config, false).await;
    }
    if !url.starts_with("sqlite:") {
        // A typo such as `postgress://` must not become a SQLite file.
        anyhow::bail!(
            "DATABASE_URL `{}` must start with sqlite:, postgres:// or postgresql://",
            redact(url)
        );
    }
    if is_memory(url)
        && let Some(test_url) = test_database_url().filter(|u| is_postgres(u))
    {
        return connect_postgres(&test_url, config, true).await;
    }
    let schema = SchemaEpoch::default();
    let pool = connect_sqlite(config, schema.clone()).await?;
    Ok(Db::from_sqlite(pool, schema))
}

/// `TEST_DATABASE_URL` from the environment, else from `.env` in the current
/// directory (without loading the rest of `.env` into the environment).
fn test_database_url() -> Option<String> {
    test_database_url_in(
        std::env::var("TEST_DATABASE_URL").ok(),
        std::path::Path::new(".env"),
    )
}

/// `env` when set, else `TEST_DATABASE_URL` in the file `dotenv`.
fn test_database_url_in(env: Option<String>, dotenv: &std::path::Path) -> Option<String> {
    env.or_else(|| {
        dotenvy::from_path_iter(dotenv)
            .ok()?
            .flatten()
            .find(|(key, _)| key == "TEST_DATABASE_URL")
            .map(|(_, url)| url)
    })
}

fn is_postgres(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

fn is_memory(url: &str) -> bool {
    url.contains(":memory:") || url.contains("mode=memory")
}

/// A throwaway database for checks that run migrations: a fresh schema on
/// PostgreSQL, an in-memory database on SQLite.
pub(crate) async fn connect_scratch(config: &Config) -> anyhow::Result<Db> {
    if is_postgres(&config.database_url) {
        return connect_postgres(&config.database_url, config, true).await;
    }
    let mut scratch = config.clone();
    scratch.database_url = "sqlite::memory:".into();
    connect(&scratch).await
}

/// Drops the schema of a database made by [`connect_scratch`] (PostgreSQL only).
pub(crate) async fn drop_scratch(db: &Db) {
    if db.dialect() != Dialect::Postgres {
        return;
    }
    let Ok(schema) = sql("SELECT current_schema()").scalar::<String>(db).await else {
        return;
    };
    if schema.starts_with("renox_test_") {
        let _ = sql(format!("DROP SCHEMA {} CASCADE", quote(&schema)))
            .execute(db)
            .await;
    }
}

/// Connects to PostgreSQL; with `fresh_schema`, in a new, empty schema of its
/// own (named `renox_test_…`, left behind for inspection).
#[cfg(feature = "postgres")]
async fn connect_postgres(url: &str, config: &Config, fresh_schema: bool) -> anyhow::Result<Db> {
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::{ConnectOptions, Connection};

    let failed = || format!("could not connect to the database at `{}`", redact(url));
    let mut options = PgConnectOptions::from_str(url).with_context(failed)?;
    if fresh_schema {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let schema = format!(
            "renox_test_{}_{}_{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let mut conn = options.connect().await.with_context(failed)?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE SCHEMA {}",
            quote(&schema)
        )))
        .execute(&mut conn)
        .await
        .context("could not create a schema for the test")?;
        conn.close().await.ok();
        options = options.options([("search_path", schema)]);
    }
    if let Some(limit) = config.database_statement_timeout {
        options = options.options([("statement_timeout", format!("{}ms", limit.as_millis()))]);
    }
    let pool_size = config.database_pool_size;
    // Test suites boot many apps at once; a few connections each keeps them
    // under the server's limit (100 by default).
    let pool_size = if fresh_schema {
        pool_size.min(3)
    } else {
        pool_size
    };
    let schema = SchemaEpoch::default();
    let epoch = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(pool_size)
        .acquire_timeout(config.database_acquire_timeout)
        .before_acquire(move |_, meta| {
            let fresh = !epoch.is_stale(meta.age);
            Box::pin(async move { Ok(fresh) })
        })
        .connect_with(options)
        .await
        .with_context(failed)?;
    Ok(Db::from_postgres(pool, schema))
}

#[cfg(not(feature = "postgres"))]
async fn connect_postgres(_url: &str, _config: &Config, _fresh_schema: bool) -> anyhow::Result<Db> {
    anyhow::bail!(
        "the database URL points to PostgreSQL, but this build has no PostgreSQL support; \
         enable the `postgres` feature of `renox`"
    )
}

/// The URL with its password hidden, for error messages.
fn redact(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    match rest.split_once('@') {
        Some((user, host)) => {
            let user = user.split_once(':').map_or(user, |(name, _)| name);
            format!("{scheme}://{user}:***@{host}")
        }
        None => url.to_owned(),
    }
}

/// How long SQLite waits for a lock before answering "database is locked".
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens a SQLite pool, creating the file and its directory if needed. File
/// databases use WAL mode; every connection enforces foreign keys.
async fn connect_sqlite(config: &Config, schema: SchemaEpoch) -> anyhow::Result<sqlx::SqlitePool> {
    let url = &config.database_url;
    let in_memory = is_memory(url);

    let mut options = SqliteConnectOptions::from_str(url)
        .with_context(|| format!("DATABASE_URL `{url}` is not a valid SQLite URL"))?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT);
    // One busy wait can take the whole `BUSY_TIMEOUT`, so busy errors get that
    // much on top of the acquire timeout: a long wait still leaves room to
    // try again (#178).
    let busy_budget = config.database_acquire_timeout + BUSY_TIMEOUT;
    if !in_memory {
        options = options
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        if let Some(dir) = options
            .get_filename()
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
        {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        prepare_file(&options, busy_budget).await.with_context(|| {
            format!("could not open the database at `{url}` (switching it to WAL)")
        })?;
    }

    // Each connection to `:memory:` is a separate database, so keep exactly one.
    let pool = if in_memory {
        SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
    } else {
        // Connections opened before a migration would read the old schema.
        SqlitePoolOptions::new()
            .max_connections(config.database_pool_size)
            .before_acquire(move |_, meta| {
                let fresh = !schema.is_stale(meta.age);
                Box::pin(async move { Ok(fresh) })
            })
    }
    .acquire_timeout(if in_memory {
        // The one connection is busy for a moment at most, unless a task
        // waits for itself (a query through `&db` while its own transaction
        // is open): fail such a test fast instead of letting it hang.
        config.database_acquire_timeout.min(Duration::from_secs(2))
    } else {
        config.database_acquire_timeout
    });
    let pool = if in_memory {
        // sqlx opens the first connection within `acquire_timeout` too, and on a
        // busy machine that can take longer than the 2 s above: try again until
        // the configured timeout has passed.
        let budget = config.database_acquire_timeout;
        retry_while(budget, is_pool_timeout, || {
            pool.clone().connect_with(options.clone())
        })
        .await
    } else {
        // Another process opening or closing the same file can make SQLite
        // answer "database is locked" at once (#163), or wait in the busy
        // handler past the pool's deadline.
        retry_while(busy_budget, is_busy_or_pool_timeout, || {
            pool.clone().connect_with(options.clone())
        })
        .await
    };
    pool.with_context(|| format!("could not open the database at `{url}`"))
}

/// Opens a file database once before its pool does, so the file exists and is
/// in WAL mode. Switching to WAL takes an exclusive lock, so processes opening
/// a brand-new file together (`serve` and `route:list`, say) contend for it:
/// SQLite answers some of them "database is locked" at once, and a wait in the
/// busy handler could outlast the pool's acquire timeout (#163). Busy errors
/// are retried until `budget` has passed; this one connection has no acquire
/// deadline. Once the file is in WAL mode, the pool's connections don't need
/// the exclusive lock.
async fn prepare_file(options: &SqliteConnectOptions, budget: Duration) -> Result<(), sqlx::Error> {
    use sqlx::{ConnectOptions, Connection};

    // A short busy wait per try: connections switching a new file to WAL
    // together can each wait for the others' lock, and with the usual five
    // seconds the first try alone used up the whole budget (#163).
    let options = options.clone().busy_timeout(Duration::from_millis(100));
    let conn = retry_while(budget, is_busy, || options.connect()).await?;
    conn.close().await
}

/// Whether SQLite answered `SQLITE_BUSY` or `SQLITE_LOCKED` (or one of their
/// extended codes).
fn is_busy(error: &sqlx::Error) -> bool {
    let Some(code) = error
        .as_database_error()
        .and_then(|e| e.code())
        .and_then(|code| code.parse::<i32>().ok())
    else {
        return false;
    };
    matches!(code & 0xff, 5 | 6)
}

fn is_busy_or_pool_timeout(error: &sqlx::Error) -> bool {
    is_busy(error) || is_pool_timeout(error)
}

fn is_pool_timeout(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::PoolTimedOut)
}

/// Runs `open` again, after a short pause, while it fails with an error
/// `retryable` accepts and `budget` hasn't passed since the first try (but at
/// least three times); any other result is returned as it is.
async fn retry_while<T, F>(
    budget: Duration,
    retryable: fn(&sqlx::Error) -> bool,
    mut open: impl FnMut() -> F,
) -> Result<T, sqlx::Error>
where
    F: std::future::Future<Output = Result<T, sqlx::Error>>,
{
    let start = tokio::time::Instant::now();
    let mut tries = 0;
    loop {
        tries += 1;
        match open().await {
            // At least three tries: one try can wait out a whole busy timeout
            // (as long as the budget) before it fails (#163).
            Err(e) if retryable(&e) && (start.elapsed() < budget || tries < 3) => {
                tracing::debug!("opening the database failed ({e}); trying again");
                // 25–75 ms, so processes that collided don't collide again.
                let jitter = u64::from(rand::random::<u8>()) * 50 / 255;
                tokio::time::sleep(Duration::from_millis(25 + jitter)).await;
            }
            result => return result,
        }
    }
}

/// Quotes an identifier (the same on SQLite and PostgreSQL), e.g. `order` -> `"order"`.
pub(crate) fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Implements sqlx's traits for a type stored as text through `Display` and
/// `FromStr` (what `#[derive(DbEnum)]` generates), for every database this
/// build of Renox supports. Chosen when renox-core is compiled, so apps don't
/// need to know which features it has.
#[doc(hidden)]
#[cfg(not(feature = "postgres"))]
#[macro_export]
macro_rules! __db_text_type {
    ($t:ty) => {
        $crate::__db_text_type_for!($t, $crate::__sqlx::sqlite::Sqlite);
    };
}

#[doc(hidden)]
#[cfg(feature = "postgres")]
#[macro_export]
macro_rules! __db_text_type {
    ($t:ty) => {
        $crate::__db_text_type_for!($t, $crate::__sqlx::sqlite::Sqlite);
        $crate::__db_text_type_for!($t, $crate::__sqlx::postgres::Postgres);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __db_text_type_for {
    ($t:ty, $db:ty) => {
        impl $crate::__sqlx::Type<$db> for $t {
            fn type_info() -> <$db as $crate::__sqlx::Database>::TypeInfo {
                <::std::string::String as $crate::__sqlx::Type<$db>>::type_info()
            }

            fn compatible(ty: &<$db as $crate::__sqlx::Database>::TypeInfo) -> bool {
                <::std::string::String as $crate::__sqlx::Type<$db>>::compatible(ty)
            }
        }

        impl<'r> $crate::__sqlx::Decode<'r, $db> for $t {
            fn decode(
                value: <$db as $crate::__sqlx::Database>::ValueRef<'r>,
            ) -> ::std::result::Result<Self, $crate::__sqlx::error::BoxDynError> {
                let text = <::std::string::String as $crate::__sqlx::Decode<$db>>::decode(value)?;
                text.parse::<$t>().map_err(::std::convert::Into::into)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::{
        is_busy, is_busy_or_pool_timeout, is_pool_timeout, redact, retry_while,
        test_database_url_in,
    };
    use crate::db::error::fake;

    #[test]
    fn urls_lose_only_their_password() {
        assert_eq!(
            redact("postgres://app:secret@db:5432/app"),
            "postgres://app:***@db:5432/app"
        );
        // A user without a password still gets the mask; no `@` or no
        // scheme leaves the URL as it is.
        assert_eq!(redact("postgres://app@db/app"), "postgres://app:***@db/app");
        assert_eq!(redact("postgres://db/app"), "postgres://db/app");
        assert_eq!(redact("sqlite:app.db"), "sqlite:app.db");
    }

    #[test]
    fn busy_errors_and_pool_timeouts() {
        for code in ["5", "6", "517", "261"] {
            assert!(is_busy(&fake::coded(Some(code))), "{code}");
        }
        assert!(!is_busy(&fake::coded(Some("19"))));
        assert!(!is_busy(&fake::coded(Some("40001")))); // not a number
        assert!(!is_busy(&fake::coded(None)));
        assert!(!is_busy(&sqlx::Error::PoolTimedOut));
        assert!(is_busy_or_pool_timeout(&sqlx::Error::PoolTimedOut));
        assert!(is_busy_or_pool_timeout(&fake::coded(Some("5"))));
        assert!(!is_busy_or_pool_timeout(&sqlx::Error::PoolClosed));
    }

    /// The environment wins; else `.env` is read for that one key; a
    /// missing file or key gives nothing.
    #[test]
    fn the_test_database_url_comes_from_the_environment_or_dotenv() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv = dir.path().join(".env");
        assert_eq!(test_database_url_in(None, &dotenv), None);
        std::fs::write(
            &dotenv,
            "APP_NAME=x\nTEST_DATABASE_URL=postgres://t@localhost/t\n",
        )
        .unwrap();
        assert_eq!(
            test_database_url_in(None, &dotenv).as_deref(),
            Some("postgres://t@localhost/t")
        );
        assert_eq!(
            test_database_url_in(Some("postgres://env/e".into()), &dotenv).as_deref(),
            Some("postgres://env/e")
        );
        std::fs::write(&dotenv, "APP_NAME=x\n").unwrap();
        assert_eq!(test_database_url_in(None, &dotenv), None);
    }

    /// A file database in directories that don't exist yet: they're made.
    /// An in-memory one waits at most 2 s for its one connection.
    #[tokio::test]
    async fn sqlite_makes_directories_and_caps_the_in_memory_wait() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::Config {
            database_url: format!("sqlite://{}/a/b/app.db", dir.path().display()),
            ..crate::Config::default()
        };
        let pool = super::connect_sqlite(&config, Default::default())
            .await
            .unwrap();
        assert!(dir.path().join("a/b/app.db").exists());
        pool.close().await;

        let config = crate::Config {
            database_url: "sqlite::memory:".into(),
            database_acquire_timeout: Duration::from_secs(30),
            ..crate::Config::default()
        };
        let pool = super::connect_sqlite(&config, Default::default())
            .await
            .unwrap();
        assert_eq!(pool.options().get_acquire_timeout(), Duration::from_secs(2));
        assert_eq!(pool.options().get_max_connections(), 1);
    }

    /// Pools opening one brand-new file database at once all open it (#163).
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn pools_opening_a_new_file_together_all_open_it() {
        for _ in 0..10 {
            let dir = tempfile::tempdir().unwrap();
            let config = crate::Config {
                database_url: format!("sqlite://{}/app.db", dir.path().display()),
                database_acquire_timeout: Duration::from_secs(5),
                ..crate::Config::default()
            };
            let opening: Vec<_> = (0..8)
                .map(|_| {
                    let config = config.clone();
                    tokio::spawn(async move {
                        super::connect_sqlite(&config, Default::default())
                            .await
                            .map(|_| ())
                    })
                })
                .collect();
            for task in opening {
                task.await.unwrap().expect("every pool opens the new file");
            }
        }
    }

    /// A slow first connection doesn't fail an in-memory app's boot (#144).
    #[tokio::test]
    async fn opening_retries_pool_timeouts_within_the_budget() {
        let tries = AtomicU32::new(0);
        let opened = retry_while(Duration::from_secs(30), is_pool_timeout, || async {
            match tries.fetch_add(1, Ordering::SeqCst) {
                0 | 1 => Err(sqlx::Error::PoolTimedOut),
                _ => Ok("pool"),
            }
        })
        .await;
        assert_eq!(opened.unwrap(), "pool");
        assert_eq!(tries.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn opening_gives_up_after_the_budget_and_on_other_errors() {
        let tries = AtomicU32::new(0);
        let opened: Result<(), _> = retry_while(Duration::ZERO, is_pool_timeout, || async {
            tries.fetch_add(1, Ordering::SeqCst);
            Err(sqlx::Error::PoolTimedOut)
        })
        .await;
        assert!(matches!(opened, Err(sqlx::Error::PoolTimedOut)));
        // Three tries even with no budget left: one try can take a whole busy
        // timeout, which used to leave no time to try again (#163).
        assert_eq!(
            tries.load(Ordering::SeqCst),
            3,
            "three tries, then no retry past the budget"
        );

        let tries = AtomicU32::new(0);
        let opened: Result<(), _> =
            retry_while(Duration::from_secs(30), is_pool_timeout, || async {
                tries.fetch_add(1, Ordering::SeqCst);
                Err(sqlx::Error::PoolClosed)
            })
            .await;
        assert!(matches!(opened, Err(sqlx::Error::PoolClosed)));
        assert_eq!(
            tries.load(Ordering::SeqCst),
            1,
            "other errors aren't retried"
        );
    }
}
