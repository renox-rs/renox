//! Renox's own handle on the database, so the same code runs on SQLite and
//! PostgreSQL: [`Db`] (the pool), [`Transaction`], [`Row`] and raw SQL
//! through [`sql()`].
//!
//! SQL written for Renox uses `?` placeholders. On PostgreSQL they become
//! `$1`, `$2`, … before the query is sent (a `?` inside quotes or comments is
//! left alone).

#[cfg(any(feature = "postgres", test))]
use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use cookie::Key;

use sqlx::sqlite::{Sqlite, SqliteArguments, SqlitePool, SqliteRow};
use sqlx::{AssertSqlSafe, Column, Row as _};

use super::value::DbValue;
use super::{DbError, ToDbValue};

#[cfg(feature = "postgres")]
use sqlx::postgres::{PgArguments, PgPool, PgRow, Postgres};

/// Which database engine a [`Db`] talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// SQLite.
    Sqlite,
    /// PostgreSQL (the `postgres` feature).
    Postgres,
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Dialect::Sqlite => "sqlite",
            Dialect::Postgres => "postgres",
        })
    }
}

/// The database pool. Take it in a handler with `State(db): State<Db>`.
///
/// Models, queries and [`sql()`] take `&db` (or `&mut tx` inside a
/// [`Transaction`]). For something only sqlx can do, reach the underlying
/// pool with [`Db::sqlite`] or `Db::postgres` (with the `postgres` feature).
#[derive(Clone)]
pub struct Db {
    pool: Pool,
    schema: SchemaEpoch,
    /// Seals and opens `Encrypted` columns (`APP_KEY`, set at boot).
    key: Option<Arc<Key>>,
}

/// When this process last changed the schema (migrations ran).
///
/// A pooled connection keeps the schema it read, and prepares statements
/// against it until it next steps one. sqlx takes a statement's columns from
/// that first prepare, so after `ALTER TABLE users ADD COLUMN …` a
/// `SELECT *` on an older connection gets one column more than sqlx expects
/// and panics (sqlx-sqlite `row.rs`), or PostgreSQL refuses a cached plan.
/// The pools made by [`super::connect`] drop connections opened before the
/// last change instead of reusing them.
#[derive(Clone, Default)]
pub(crate) struct SchemaEpoch(std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>);

impl SchemaEpoch {
    pub(crate) fn changed(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
    }

    /// Whether a connection of this age was opened before the last change.
    pub(crate) fn is_stale(&self, age: std::time::Duration) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|at| age >= at.elapsed())
    }
}

#[derive(Clone)]
enum Pool {
    Sqlite(SqlitePool),
    #[cfg(feature = "postgres")]
    Postgres(PgPool),
}

impl fmt::Debug for Db {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Db")
            .field("dialect", &self.dialect())
            .finish_non_exhaustive()
    }
}

impl From<SqlitePool> for Db {
    fn from(pool: SqlitePool) -> Self {
        Self::with_epoch(Pool::Sqlite(pool), SchemaEpoch::default())
    }
}

#[cfg(feature = "postgres")]
impl From<PgPool> for Db {
    fn from(pool: PgPool) -> Self {
        Self::with_epoch(Pool::Postgres(pool), SchemaEpoch::default())
    }
}

impl Db {
    fn with_epoch(pool: Pool, schema: SchemaEpoch) -> Self {
        Self {
            pool,
            schema,
            key: None,
        }
    }

    /// The key `Encrypted` columns are sealed and opened with (the app's
    /// `APP_KEY`, set at boot).
    pub(crate) fn with_key(mut self, key: Key) -> Self {
        self.key = Some(Arc::new(key));
        self
    }

    pub(crate) fn from_sqlite(pool: SqlitePool, schema: SchemaEpoch) -> Self {
        Self::with_epoch(Pool::Sqlite(pool), schema)
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres(pool: PgPool, schema: SchemaEpoch) -> Self {
        Self::with_epoch(Pool::Postgres(pool), schema)
    }

    /// Records that the schema changed, so older pooled connections aren't
    /// reused (see `SchemaEpoch`).
    pub(crate) fn schema_changed(&self) {
        self.schema.changed();
    }

    /// The engine this pool talks to.
    pub fn dialect(&self) -> Dialect {
        match self.pool {
            Pool::Sqlite(_) => Dialect::Sqlite,
            #[cfg(feature = "postgres")]
            Pool::Postgres(_) => Dialect::Postgres,
        }
    }

    /// The sqlx pool, when the database is SQLite.
    pub fn sqlite(&self) -> Option<&SqlitePool> {
        match &self.pool {
            Pool::Sqlite(pool) => Some(pool),
            #[cfg(feature = "postgres")]
            Pool::Postgres(_) => None,
        }
    }

    /// The sqlx pool, when the database is PostgreSQL.
    #[cfg(feature = "postgres")]
    pub fn postgres(&self) -> Option<&PgPool> {
        match &self.pool {
            Pool::Postgres(pool) => Some(pool),
            Pool::Sqlite(_) => None,
        }
    }

    /// Starts a transaction. Pass `&mut tx` wherever a `&db` goes, then
    /// `tx.commit()`; dropping it without committing rolls it back.
    pub async fn begin(&self) -> Result<Transaction, DbError> {
        let inner = match &self.pool {
            Pool::Sqlite(pool) => TxInner::Sqlite(pool.begin().await?),
            #[cfg(feature = "postgres")]
            Pool::Postgres(pool) => TxInner::Postgres(pool.begin().await?),
        };
        Ok(Transaction {
            inner,
            key: self.key.clone(),
            savepoints: 0,
        })
    }

    /// Runs `work` in a transaction: committed when it returns `Ok`, rolled
    /// back when it returns `Err`.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # async fn demo(db: Db) -> Result {
    /// let moved = db
    ///     .transaction_retrying(3, |tx| {
    ///         Box::pin(async move {
    ///             renox::db::sql("UPDATE accounts SET balance = balance - 100 WHERE id = 1")
    ///                 .execute(&mut *tx)
    ///                 .await?;
    ///             renox::db::sql("UPDATE accounts SET balance = balance + 100 WHERE id = 2")
    ///                 .execute(&mut *tx)
    ///                 .await?;
    ///             Ok(100)
    ///         })
    ///     })
    ///     .await?;
    /// # let _: i64 = moved; Ok(()) }
    /// ```
    pub async fn transaction<T, F>(&self, work: F) -> crate::Result<T>
    where
        F: for<'t> FnMut(
            &'t mut Transaction,
        ) -> futures_util::future::BoxFuture<'t, crate::Result<T>>,
    {
        self.transaction_retrying(1, work).await
    }

    /// Like [`Db::transaction`], trying up to `attempts` times when another
    /// transaction got in the way (SQLite busy, PostgreSQL serialization
    /// failure or deadlock), with a short, growing pause in between. `work`
    /// must be safe to run again.
    pub async fn transaction_retrying<T, F>(&self, attempts: u32, mut work: F) -> crate::Result<T>
    where
        F: for<'t> FnMut(
            &'t mut Transaction,
        ) -> futures_util::future::BoxFuture<'t, crate::Result<T>>,
    {
        let attempts = attempts.max(1);
        let mut attempt = 1;
        loop {
            let outcome = async {
                let mut tx = self.begin().await?;
                let value = work(&mut tx).await?;
                tx.commit().await?;
                Ok::<T, crate::Error>(value)
            }
            .await;
            match outcome {
                Err(err) if attempt < attempts && err.is_retryable() => {
                    let pause = std::time::Duration::from_millis(20 * u64::from(attempt));
                    tokio::time::sleep(pause).await;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    /// Runs `work` again (up to `attempts` times, with a short, growing
    /// pause) while it fails because another transaction got in the way
    /// (SQLite busy, PostgreSQL serialization failure or deadlock). `work`
    /// opens and commits its own transaction, so unlike
    /// [`Db::transaction_retrying`] it can borrow from the caller, and it can
    /// roll back and still return a value (drop or `rollback` the
    /// transaction instead of committing).
    ///
    /// ```
    /// # use renox::prelude::*;
    /// enum Transfer { Done, Short }
    ///
    /// # async fn demo(db: Db, amount: i64, from: i64, to: i64) -> Result {
    /// let outcome = db
    ///     .retrying(3, || async {
    ///         let mut tx = db.begin().await?;
    ///         let taken = renox::db::sql(
    ///             "UPDATE accounts SET balance = balance - ? WHERE id = ? AND balance >= ?",
    ///         )
    ///         .bind(amount) // borrowed from the caller
    ///         .bind(from)
    ///         .bind(amount)
    ///         .execute(&mut tx)
    ///         .await?;
    ///         if taken == 0 {
    ///             tx.rollback().await?;
    ///             return Ok(Transfer::Short); // rolled back, with an answer
    ///         }
    ///         renox::db::sql("UPDATE accounts SET balance = balance + ? WHERE id = ?")
    ///             .bind(amount)
    ///             .bind(to)
    ///             .execute(&mut tx)
    ///             .await?;
    ///         tx.commit().await?;
    ///         Ok(Transfer::Done)
    ///     })
    ///     .await?;
    /// # let _ = outcome; Ok(()) }
    /// ```
    pub fn retrying<'a, T, F, Fut>(
        &'a self,
        attempts: u32,
        mut work: F,
    ) -> impl Future<Output = crate::Result<T>> + Send + 'a
    where
        T: Send + 'a,
        F: FnMut() -> Fut + Send + 'a,
        Fut: Future<Output = crate::Result<T>> + Send + 'a,
    {
        let attempts = attempts.max(1);
        async move {
            let mut attempt = 1;
            loop {
                match work().await {
                    Err(err) if attempt < attempts && err.is_retryable() => {
                        let pause = std::time::Duration::from_millis(20 * u64::from(attempt));
                        tokio::time::sleep(pause).await;
                        attempt += 1;
                    }
                    other => return other,
                }
            }
        }
    }

    /// A transaction that takes SQLite's write lock at once (`BEGIN
    /// IMMEDIATE`), so what it reads can't change before it writes: the
    /// SQLite counterpart of `lock_for_update`. Same as `begin` on PostgreSQL.
    pub async fn begin_immediate(&self) -> Result<Transaction, DbError> {
        let inner = match &self.pool {
            Pool::Sqlite(pool) => TxInner::Sqlite(pool.begin_with("BEGIN IMMEDIATE").await?),
            #[cfg(feature = "postgres")]
            Pool::Postgres(pool) => TxInner::Postgres(pool.begin().await?),
        };
        Ok(Transaction {
            inner,
            key: self.key.clone(),
            savepoints: 0,
        })
    }

    /// Closes every connection; later queries fail.
    pub async fn close(&self) {
        match &self.pool {
            Pool::Sqlite(pool) => pool.close().await,
            #[cfg(feature = "postgres")]
            Pool::Postgres(pool) => pool.close().await,
        }
    }
}

/// A database transaction, from [`Db::begin`].
pub struct Transaction {
    inner: TxInner,
    key: Option<Arc<Key>>,
    /// Savepoints open inside it (for their names).
    savepoints: u32,
}

enum TxInner {
    Sqlite(sqlx::Transaction<'static, Sqlite>),
    #[cfg(feature = "postgres")]
    Postgres(sqlx::Transaction<'static, Postgres>),
}

impl Transaction {
    /// The engine this transaction runs on.
    pub fn dialect(&self) -> Dialect {
        match self.inner {
            TxInner::Sqlite(_) => Dialect::Sqlite,
            #[cfg(feature = "postgres")]
            TxInner::Postgres(_) => Dialect::Postgres,
        }
    }

    /// Commits the transaction.
    pub async fn commit(self) -> Result<(), DbError> {
        match self.inner {
            TxInner::Sqlite(tx) => Ok(tx.commit().await?),
            #[cfg(feature = "postgres")]
            TxInner::Postgres(tx) => Ok(tx.commit().await?),
        }
    }

    /// Rolls the transaction back (dropping it uncommitted does the same).
    pub async fn rollback(self) -> Result<(), DbError> {
        match self.inner {
            TxInner::Sqlite(tx) => Ok(tx.rollback().await?),
            #[cfg(feature = "postgres")]
            TxInner::Postgres(tx) => Ok(tx.rollback().await?),
        }
    }

    /// Runs `work` inside a savepoint (a transaction inside the
    /// transaction): when it returns `Err`, only what it did is undone and
    /// the transaction goes on; when it returns `Ok`, its changes stay, to
    /// be committed (or rolled back) with the rest. Savepoints nest.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # async fn demo(db: Db) -> Result {
    /// let mut tx = db.begin().await?;
    /// renox::db::sql("INSERT INTO orders (total) VALUES (?)").bind(18_000).execute(&mut tx).await?;
    /// // Optional: a voucher that may be used up already. If it is, the order stays.
    /// let voucher = tx
    ///     .savepoint(|tx| {
    ///         Box::pin(async move {
    ///             let used = renox::db::sql("UPDATE vouchers SET used = used + 1 WHERE code = ? AND used < max_uses")
    ///                 .bind("KOPI10")
    ///                 .execute(&mut *tx)
    ///                 .await?;
    ///             renox::abort_if(used == 0, StatusCode::CONFLICT, "used up")?;
    ///             Ok(())
    ///         })
    ///     })
    ///     .await;
    /// tx.commit().await?;
    /// # let _ = voucher; Ok(()) }
    /// ```
    ///
    /// On PostgreSQL a failed statement stops the whole transaction until
    /// it rolls back; inside a savepoint only the savepoint rolls back, so
    /// the transaction can go on after an expected failure (a unique
    /// violation you handle, say).
    pub async fn savepoint<T, F>(&mut self, work: F) -> crate::Result<T>
    where
        F: for<'t> FnOnce(
            &'t mut Transaction,
        ) -> futures_util::future::BoxFuture<'t, crate::Result<T>>,
    {
        let name = format!("renox_savepoint_{}", self.savepoints + 1);
        sql(format!("SAVEPOINT {name}")).execute(&mut *self).await?;
        self.savepoints += 1;
        let outcome = work(self).await;
        self.savepoints -= 1;
        match outcome {
            Ok(value) => {
                sql(format!("RELEASE SAVEPOINT {name}"))
                    .execute(&mut *self)
                    .await?;
                Ok(value)
            }
            Err(err) => {
                sql(format!("ROLLBACK TO SAVEPOINT {name}"))
                    .execute(&mut *self)
                    .await?;
                sql(format!("RELEASE SAVEPOINT {name}"))
                    .execute(&mut *self)
                    .await?;
                Err(err)
            }
        }
    }
}

impl fmt::Debug for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transaction")
            .field("dialect", &self.dialect())
            .finish_non_exhaustive()
    }
}

/// Where a query runs: `&db` or `&mut tx`. Sealed: only those.
///
/// An executor is used up by one statement. A helper that runs several
/// statements on whichever it is given turns it into a [`Conn`] first and
/// [reborrows](Conn::reborrow) it for each one:
///
/// ```
/// # use renox::prelude::*;
/// use renox::db::{Executor, sql};
///
/// /// Records a movement and updates the level, on `&db` or in `&mut tx`.
/// async fn record<'c>(db: impl Executor<'c>, product: i64, quantity: i64) -> Result {
///     let mut conn = db.into_conn();
///     sql("INSERT INTO movements (product_id, quantity) VALUES (?, ?)")
///         .bind(product)
///         .bind(quantity)
///         .execute(conn.reborrow())
///         .await?;
///     sql("UPDATE levels SET quantity = quantity + ? WHERE product_id = ?")
///         .bind(quantity)
///         .bind(product)
///         .execute(conn.reborrow())
///         .await?;
///     Ok(())
/// }
/// # async fn demo(db: &renox::db::Db) -> Result {
/// record(db, 1, 5).await?;
/// let mut tx = db.begin().await?;
/// record(&mut tx, 1, -2).await?;
/// tx.commit().await?;
/// # Ok(()) }
/// ```
pub trait Executor<'c>: Send + executor::Sealed {
    /// This executor as a [`Conn`], which can run several statements.
    fn into_conn(self) -> Conn<'c>;
}

mod executor {
    pub trait Sealed {}
    impl Sealed for &super::Db {}
    impl Sealed for &mut super::Transaction {}
    impl Sealed for super::Conn<'_> {}
}

/// A connection for several statements: the pool (`&db`) or a transaction
/// (`&mut tx`), from [`Executor::into_conn`]. It is an `Executor` itself;
/// give each statement [`Conn::reborrow`] to keep it for the next one.
pub enum Conn<'c> {
    /// Statements run on the pool, each on any free connection.
    #[doc(hidden)]
    Pool(&'c Db),
    /// Statements run in the transaction.
    #[doc(hidden)]
    Tx(&'c mut Transaction),
}

impl<'c> Executor<'c> for &'c Db {
    fn into_conn(self) -> Conn<'c> {
        Conn::Pool(self)
    }
}

impl<'c> Executor<'c> for &'c mut Transaction {
    fn into_conn(self) -> Conn<'c> {
        Conn::Tx(self)
    }
}

impl<'c> Executor<'c> for Conn<'c> {
    fn into_conn(self) -> Conn<'c> {
        self
    }
}

impl Conn<'_> {
    /// The key for `Encrypted` columns of the database this runs on.
    fn key(&self) -> Option<Arc<Key>> {
        match self {
            Conn::Pool(db) => db.key.clone(),
            Conn::Tx(tx) => tx.key.clone(),
        }
    }

    /// The same connection for one more statement (a model's `save`, a
    /// query's `fetch_all`…), keeping this one for the next.
    pub fn reborrow(&mut self) -> Conn<'_> {
        match self {
            Conn::Pool(db) => Conn::Pool(db),
            Conn::Tx(tx) => Conn::Tx(tx),
        }
    }

    /// Which engine the statements run on, for SQL that differs.
    pub fn dialect(&self) -> Dialect {
        match self {
            Conn::Pool(db) => db.dialect(),
            Conn::Tx(tx) => tx.dialect(),
        }
    }
}

/// Runs `$body` against whichever backend `$conn` is: `$build` makes the
/// backend's query from `(sql, args)` and `$exec` is its sqlx executor.
macro_rules! dispatch {
    ($conn:expr, |$build:ident, $exec:ident| $body:expr) => {
        match $conn {
            Conn::Pool(db) => match &db.pool {
                Pool::Sqlite(pool) => {
                    #[allow(unused_variables)]
                    let $build = sqlite_query;
                    let $exec = pool;
                    ($body).map_err(DbError::from)
                }
                #[cfg(feature = "postgres")]
                Pool::Postgres(pool) => {
                    #[allow(unused_variables)]
                    let $build = postgres_query;
                    let $exec = pool;
                    ($body).map_err(DbError::from)
                }
            },
            Conn::Tx(tx) => match &mut tx.inner {
                TxInner::Sqlite(tx) => {
                    #[allow(unused_variables)]
                    let $build = sqlite_query;
                    let $exec = &mut **tx;
                    ($body).map_err(DbError::from)
                }
                #[cfg(feature = "postgres")]
                TxInner::Postgres(tx) => {
                    #[allow(unused_variables)]
                    let $build = postgres_query;
                    let $exec = &mut **tx;
                    ($body).map_err(DbError::from)
                }
            },
        }
    };
}

/// Seals the values of `Encrypted` fields with the database's key.
fn seal(key: Option<&Key>, args: Vec<DbValue>) -> Result<Vec<DbValue>, DbError> {
    args.into_iter()
        .map(|value| match value {
            DbValue::Encrypted(plain) => match key {
                Some(key) => Ok(DbValue::Text(super::encrypted::seal(key, &plain.0))),
                None => Err(DbError::from(sqlx::Error::Encode(
                    "an Encrypted value needs the app's Db (it has the APP_KEY); \
                     this Db was made outside App"
                        .into(),
                ))),
            },
            other => Ok(other),
        })
        .collect()
}

fn sqlite_query(
    sql: String,
    args: Vec<DbValue>,
) -> sqlx::query::Query<'static, Sqlite, SqliteArguments> {
    args.into_iter().fold(
        sqlx::query(AssertSqlSafe(sql)),
        |query, value| match value.for_sqlite() {
            DbValue::Integer(v) => query.bind(v),
            DbValue::Real(v) => query.bind(v),
            DbValue::Text(v) => query.bind(v),
            DbValue::Blob(v) => query.bind(v),
            _ => query.bind(None::<i64>),
        },
    )
}

#[cfg(feature = "postgres")]
fn postgres_query(
    sql: String,
    args: Vec<DbValue>,
) -> sqlx::query::Query<'static, Postgres, PgArguments> {
    let sql = numbered_placeholders(&sql).into_owned();
    args.into_iter().fold(
        sqlx::query(AssertSqlSafe(sql)),
        |query, value| match value {
            DbValue::Null => query.bind(UntypedNull),
            DbValue::Integer(v) => query.bind(v),
            DbValue::Real(v) => query.bind(v),
            DbValue::Text(v) => query.bind(v),
            DbValue::Blob(v) => query.bind(v),
            DbValue::Bool(v) => query.bind(v),
            DbValue::DateTime(v) => query.bind(v),
            DbValue::NaiveDateTime(v) => query.bind(v),
            DbValue::Date(v) => query.bind(v),
            DbValue::Time(v) => query.bind(v),
            DbValue::Json(v) => query.bind(sqlx::types::Json(v)),
            #[cfg(feature = "uuid")]
            DbValue::Uuid(v) => query.bind(v),
            // Sealed into `Text` before the statement is built.
            DbValue::Encrypted(_) => query.bind(UntypedNull),
        },
    )
}

/// A `NULL` parameter without a type, so PostgreSQL takes the type from
/// where it's used (a `NULL` sent as `BIGINT` can't go into a `TIMESTAMPTZ`).
#[cfg(feature = "postgres")]
struct UntypedNull;

#[cfg(feature = "postgres")]
impl sqlx::Type<Postgres> for UntypedNull {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        // OID 0 means "unspecified" in the protocol.
        sqlx::postgres::PgTypeInfo::with_oid(sqlx::postgres::types::Oid(0))
    }
}

#[cfg(feature = "postgres")]
impl sqlx::Encode<'_, Postgres> for UntypedNull {
    fn encode_by_ref(
        &self,
        _buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        Ok(sqlx::encode::IsNull::Yes)
    }
}

/// Rewrites `?` placeholders as PostgreSQL's `$1`, `$2`, …, skipping quoted
/// strings (`'…'`, `E'…'` with backslash escapes, dollar-quoted `$$…$$` and
/// `$tag$…$tag$`), quoted identifiers and comments.
#[cfg(any(feature = "postgres", test))]
pub(crate) fn numbered_placeholders(sql: &str) -> Cow<'_, str> {
    if !sql.contains('?') {
        return Cow::Borrowed(sql);
    }
    // Every delimiter is ASCII, so byte offsets always fall between characters.
    let bytes = sql.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 0;
    let mut copied = 0; // `sql[..copied]` is in `out`
    let mut i = 0;
    while i < bytes.len() {
        let at = i;
        let end = match bytes[i] {
            quote @ (b'\'' | b'"') => {
                // `E'…'` (or `e'…'`): a backslash escapes the next character.
                let escapes = quote == b'\''
                    && i > 0
                    && matches!(bytes[i - 1], b'E' | b'e')
                    && (i < 2 || !ident(bytes[i - 2]));
                let mut j = i + 1;
                // A doubled quote inside is an escaped quote: the loop ends at
                // the first one and the next one opens a new run.
                while j < bytes.len() && bytes[j] != quote {
                    j += if escapes && bytes[j] == b'\\' { 2 } else { 1 };
                }
                j + 1
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                sql[i..].find('\n').map_or(bytes.len(), |nl| i + nl + 1)
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => sql[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |e| i + 2 + e + 2),
            b'$' if i == 0 || !ident(bytes[i - 1]) => {
                // `$tag$` opens a dollar-quoted string (the tag may be empty
                // and doesn't start with a digit, unlike `$1`), closed by the
                // same `$tag$`.
                let tag_len = bytes[i + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                    .count();
                let starts_with_digit = bytes.get(i + 1).is_some_and(u8::is_ascii_digit);
                if !starts_with_digit && bytes.get(i + 1 + tag_len) == Some(&b'$') {
                    let tag = &sql[i..i + tag_len + 2];
                    sql[i + tag.len()..]
                        .find(tag)
                        .map_or(bytes.len(), |e| i + tag.len() + e + tag.len())
                } else {
                    i + 1
                }
            }
            b'?' => {
                n += 1;
                out.push_str(&sql[copied..i]);
                out.push_str(&format!("${n}"));
                copied = i + 1;
                i + 1
            }
            _ => i + 1,
        };
        i = end.min(bytes.len()).max(at + 1);
    }
    out.push_str(&sql[copied..]);
    Cow::Owned(out)
}

/// A raw SQL statement with `?` placeholders.
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(db: Db) -> Result {
/// let rows = renox::db::sql("SELECT name FROM products WHERE price < ?")
///     .bind(20_000)
///     .fetch_all(&db)
///     .await?;
/// let name: String = rows[0].try_get("name")?;
///
/// let total: i64 = renox::db::sql("SELECT COUNT(*) FROM products").scalar(&db).await?;
/// # let _ = (name, total); Ok(()) }
/// ```
pub fn sql(sql: impl Into<String>) -> Sql {
    Sql {
        sql: sql.into(),
        args: Vec::new(),
    }
}

/// A statement built by [`sql()`].
#[derive(Debug, Clone)]
#[must_use = "a statement does nothing until it is run"]
pub struct Sql {
    sql: String,
    args: Vec<DbValue>,
}

impl Sql {
    /// Binds the next `?`.
    pub fn bind(mut self, value: impl ToDbValue) -> Self {
        self.args.push(value.to_db_value());
        self
    }

    /// Binds several values in order.
    pub fn bind_all(mut self, values: impl IntoIterator<Item = DbValue>) -> Self {
        self.args.extend(values);
        self
    }

    /// Runs the statement and returns every row.
    pub async fn fetch_all<'c>(self, db: impl Executor<'c>) -> Result<Vec<Row>, DbError> {
        let Self { sql, args } = self;
        super::query_log::record(&sql);
        let conn = db.into_conn();
        let key = conn.key();
        let args = seal(key.as_deref(), args)?;
        let rows: Vec<Row> = dispatch!(conn, |build, exec| build(sql, args)
            .fetch_all(exec)
            .await
            .map(|rows| rows.into_iter().map(Row::from).collect()))?;
        Ok(rows
            .into_iter()
            .map(|row| row.with_key(key.clone()))
            .collect())
    }

    /// Runs the statement and returns the first row, if any.
    pub async fn fetch_optional<'c>(self, db: impl Executor<'c>) -> Result<Option<Row>, DbError> {
        let Self { sql, args } = self;
        super::query_log::record(&sql);
        let conn = db.into_conn();
        let key = conn.key();
        let args = seal(key.as_deref(), args)?;
        let row: Option<Row> = dispatch!(conn, |build, exec| build(sql, args)
            .fetch_optional(exec)
            .await
            .map(|row| row.map(Row::from)))?;
        Ok(row.map(|row| row.with_key(key)))
    }

    /// The first row; an error if there is none.
    pub async fn fetch_one<'c>(self, db: impl Executor<'c>) -> Result<Row, DbError> {
        self.fetch_optional(db)
            .await?
            .ok_or_else(|| DbError::from(sqlx::Error::RowNotFound))
    }

    /// Runs the statement and returns the number of rows it changed.
    pub async fn execute<'c>(self, db: impl Executor<'c>) -> Result<u64, DbError> {
        let Self { sql, args } = self;
        super::query_log::record(&sql);
        let conn = db.into_conn();
        let args = seal(conn.key().as_deref(), args)?;
        dispatch!(conn, |build, exec| build(sql, args)
            .execute(exec)
            .await
            .map(|done| done.rows_affected()))
    }

    /// Every row, read into `T`: a model, a `#[derive(FromRow)]` struct or a
    /// tuple; see [`super::FromRow`].
    pub async fn fetch_as<'c, T: super::FromRow>(
        self,
        db: impl Executor<'c>,
    ) -> Result<Vec<T>, DbError> {
        self.fetch_all(db).await?.iter().map(T::from_row).collect()
    }

    /// The first row read into `T`, or `None` if there is no row.
    pub async fn fetch_optional_as<'c, T: super::FromRow>(
        self,
        db: impl Executor<'c>,
    ) -> Result<Option<T>, DbError> {
        self.fetch_optional(db)
            .await?
            .as_ref()
            .map(T::from_row)
            .transpose()
    }

    /// The first row read into `T`; an error if there is none.
    pub async fn fetch_one_as<'c, T: super::FromRow>(
        self,
        db: impl Executor<'c>,
    ) -> Result<T, DbError> {
        T::from_row(&self.fetch_one(db).await?)
    }

    /// The first column of the first row; an error if there is no row.
    pub async fn scalar<'c, T: FromDb>(self, db: impl Executor<'c>) -> Result<T, DbError> {
        self.fetch_one(db).await?.try_get(0)
    }

    /// The first column of the first row, or `None` if there is no row.
    pub async fn scalar_optional<'c, T: FromDb>(
        self,
        db: impl Executor<'c>,
    ) -> Result<Option<T>, DbError> {
        self.fetch_optional(db)
            .await?
            .map(|row| row.try_get(0))
            .transpose()
    }

    /// The first column of every row.
    pub async fn scalars<'c, T: FromDb>(self, db: impl Executor<'c>) -> Result<Vec<T>, DbError> {
        self.fetch_all(db)
            .await?
            .iter()
            .map(|row| row.try_get(0))
            .collect()
    }
}

/// Runs SQL that may hold several statements and no parameters, e.g. a
/// migration file. Returns the number of rows changed.
pub(crate) async fn script<'c>(db: impl Executor<'c>, sql: &str) -> Result<u64, DbError> {
    let sql = sql.to_owned();
    dispatch!(db.into_conn(), |build, exec| sqlx::raw_sql(AssertSqlSafe(
        sql
    ))
    .execute(exec)
    .await
    .map(|done| done.rows_affected()))
}

/// One result row. Read columns by name or position with [`Row::try_get`].
pub struct Row(pub(crate) RowInner, Option<Arc<Key>>);

pub(crate) enum RowInner {
    Sqlite(SqliteRow),
    #[cfg(feature = "postgres")]
    Postgres(PgRow),
}

impl From<SqliteRow> for Row {
    fn from(row: SqliteRow) -> Self {
        Self(RowInner::Sqlite(row), None)
    }
}

#[cfg(feature = "postgres")]
impl From<PgRow> for Row {
    fn from(row: PgRow) -> Self {
        Self(RowInner::Postgres(row), None)
    }
}

impl Row {
    fn with_key(mut self, key: Option<Arc<Key>>) -> Self {
        self.1 = key;
        self
    }

    /// A column's value, by name (`"name"`) or position (`0`).
    pub fn try_get<T: FromDb>(&self, index: impl RowIndex) -> Result<T, DbError> {
        super::encrypted::reading(self.1.as_ref(), || match &self.0 {
            RowInner::Sqlite(row) => Ok(row.try_get(index)?),
            #[cfg(feature = "postgres")]
            RowInner::Postgres(row) => Ok(row.try_get(index)?),
        })
    }

    /// A column's value as JSON, whatever its type; `null` when it can't be
    /// read as a number, boolean, text, date, time, timestamp or JSON.
    pub(crate) fn json(&self, column: &str) -> serde_json::Value {
        use serde_json::Value;
        if let Ok(v) = self.try_get::<Option<i64>>(column) {
            return v.map_or(Value::Null, Value::from);
        }
        if let Ok(v) = self.try_get::<Option<i32>>(column) {
            return v.map_or(Value::Null, Value::from);
        }
        // PostgreSQL's SMALLINT and REAL decode only as these.
        if let Ok(v) = self.try_get::<Option<i16>>(column) {
            return v.map_or(Value::Null, Value::from);
        }
        if let Ok(v) = self.try_get::<Option<f64>>(column) {
            return v.map_or(Value::Null, Value::from);
        }
        if let Ok(v) = self.try_get::<Option<f32>>(column) {
            return v.map_or(Value::Null, |n| Value::from(f64::from(n)));
        }
        if let Ok(v) = self.try_get::<Option<bool>>(column) {
            return v.map_or(Value::Null, Value::from);
        }
        if let Ok(v) = self.try_get::<Option<chrono::DateTime<chrono::Utc>>>(column) {
            return v.map_or(Value::Null, |d| Value::from(d.to_rfc3339()));
        }
        if let Ok(v) = self.try_get::<Option<chrono::NaiveDateTime>>(column) {
            return v.map_or(Value::Null, |d| {
                Value::from(d.format("%Y-%m-%dT%H:%M:%S").to_string())
            });
        }
        if let Ok(v) = self.try_get::<Option<chrono::NaiveDate>>(column) {
            return v.map_or(Value::Null, |d| Value::from(d.to_string()));
        }
        if let Ok(v) = self.try_get::<Option<String>>(column) {
            return v.map_or(Value::Null, Value::from);
        }
        // After text, so SQLite text that looks like a time stays as written;
        // PostgreSQL's TIME decodes only as this.
        if let Ok(v) = self.try_get::<Option<chrono::NaiveTime>>(column) {
            return v.map_or(Value::Null, |t| Value::from(t.to_string()));
        }
        if let Ok(v) = self.try_get::<Option<serde_json::Value>>(column) {
            return v.unwrap_or(Value::Null);
        }
        Value::Null
    }

    /// The column names, in order.
    pub fn columns(&self) -> Vec<&str> {
        match &self.0 {
            RowInner::Sqlite(row) => row.columns().iter().map(Column::name).collect(),
            #[cfg(feature = "postgres")]
            RowInner::Postgres(row) => row.columns().iter().map(Column::name).collect(),
        }
    }

    /// The sqlx row, when the database is SQLite.
    pub fn sqlite(&self) -> Option<&SqliteRow> {
        match &self.0 {
            RowInner::Sqlite(row) => Some(row),
            #[cfg(feature = "postgres")]
            RowInner::Postgres(_) => None,
        }
    }

    /// The sqlx row, when the database is PostgreSQL.
    #[cfg(feature = "postgres")]
    pub fn postgres(&self) -> Option<&PgRow> {
        match &self.0 {
            RowInner::Postgres(row) => Some(row),
            RowInner::Sqlite(_) => None,
        }
    }
}

impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Row")
            .field("columns", &self.columns())
            .finish_non_exhaustive()
    }
}

/// A Rust type that can be read from a column on every enabled database:
/// the integer and float types, `bool`, `String`, `Vec<u8>`, the chrono date
/// and time types, `Option<T>` of those, `Json<T>`, `Encrypted<T>`, `Ulid`,
/// `Uuid` (the `uuid` feature) and `#[derive(DbEnum)]` enums. Those are the
/// types Renox promises; the trait is implemented through sqlx's own
/// traits, so other types sqlx decodes work too, without that promise (see
/// docs/stability.md).
pub trait FromDb: for<'r> sqlx::Decode<'r, Sqlite> + sqlx::Type<Sqlite> + bounds::Postgres {}

impl<T> FromDb for T where
    T: for<'r> sqlx::Decode<'r, Sqlite> + sqlx::Type<Sqlite> + bounds::Postgres
{
}

/// A column name (`&str`) or position (`usize`).
pub trait RowIndex: sqlx::ColumnIndex<SqliteRow> + bounds::PostgresIndex {}

impl<T> RowIndex for T where T: sqlx::ColumnIndex<SqliteRow> + bounds::PostgresIndex {}

/// Extra bounds that only apply when the `postgres` feature is on.
#[doc(hidden)]
pub mod bounds {
    #[cfg(feature = "postgres")]
    mod on {
        use sqlx::postgres::PgRow;

        pub trait Postgres:
            for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>
        {
        }
        impl<T> Postgres for T where T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>
        {}

        pub trait PostgresIndex: sqlx::ColumnIndex<PgRow> {}
        impl<T: sqlx::ColumnIndex<PgRow>> PostgresIndex for T {}
    }

    #[cfg(not(feature = "postgres"))]
    mod on {
        pub trait Postgres {}
        impl<T: ?Sized> Postgres for T {}

        pub trait PostgresIndex {}
        impl<T: ?Sized> PostgresIndex for T {}
    }

    pub use on::{Postgres, PostgresIndex};
}

#[cfg(test)]
mod tests {
    use super::{numbered_placeholders, seal};
    use crate::db::DbValue;

    #[test]
    fn numbers_placeholders_outside_quotes_and_comments() {
        assert_eq!(
            numbered_placeholders("SELECT * FROM t WHERE a = ? AND b IN (?, ?)"),
            "SELECT * FROM t WHERE a = $1 AND b IN ($2, $3)"
        );
        assert_eq!(
            numbered_placeholders("SELECT '?', \"a?\" -- why?\nFROM t /* ? */ WHERE x = ?"),
            "SELECT '?', \"a?\" -- why?\nFROM t /* ? */ WHERE x = $1"
        );
        assert_eq!(
            numbered_placeholders("SELECT 'it''s ?' WHERE y = ?"),
            "SELECT 'it''s ?' WHERE y = $1"
        );
        assert_eq!(numbered_placeholders("SELECT 1"), "SELECT 1");
    }

    /// PostgreSQL's other string forms keep their `?` (#219).
    #[test]
    fn skips_dollar_quoted_and_escaped_strings() {
        assert_eq!(
            numbered_placeholders("SELECT $$why?$$, $fn$ a ? b $fn$ WHERE x = ?"),
            "SELECT $$why?$$, $fn$ a ? b $fn$ WHERE x = $1"
        );
        assert_eq!(
            numbered_placeholders(r"SELECT E'it\'s ?', e'\\' WHERE y = ? AND z = ?"),
            r"SELECT E'it\'s ?', e'\\' WHERE y = $1 AND z = $2"
        );
        // A plain string keeps its backslash; `$1` and `a$b` aren't quotes.
        assert_eq!(
            numbered_placeholders(r"SELECT '\', a$b$ FROM t WHERE c = ?"),
            r"SELECT '\', a$b$ FROM t WHERE c = $1"
        );
        assert_eq!(
            numbered_placeholders("SELECT 'é?' WHERE ü = ?"),
            "SELECT 'é?' WHERE ü = $1"
        );
    }

    /// Unterminated quotes, comments and dollar quotes swallow the rest; a
    /// `$` at the very end or before a digit is no quote (#254).
    #[test]
    fn placeholders_after_unfinished_quotes_stay() {
        assert_eq!(numbered_placeholders("SELECT 'open ?"), "SELECT 'open ?");
        assert_eq!(numbered_placeholders("SELECT 1 -- ?"), "SELECT 1 -- ?");
        assert_eq!(numbered_placeholders("SELECT 1 /* ?"), "SELECT 1 /* ?");
        assert_eq!(numbered_placeholders("SELECT $tag$ ?"), "SELECT $tag$ ?");
        assert_eq!(numbered_placeholders("SELECT ? || $"), "SELECT $1 || $");
        // A `$1` doesn't open a quote, so the `?` after it is still seen.
        assert!(numbered_placeholders("SELECT $1, ?").ends_with(", $1"));
        assert_eq!(numbered_placeholders("?"), "$1");
    }

    /// `retrying` and `transaction_retrying` try again after a busy error,
    /// stop at the last attempt, and return other errors at once.
    #[tokio::test]
    async fn retrying_follows_the_error_kind() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        use crate::db::DbError;
        use crate::db::error::fake;

        fn busy() -> crate::Error {
            crate::Error::from(DbError::from(fake::coded(Some("5"))))
        }
        let db = super::super::connect(&crate::Config::default())
            .await
            .unwrap();

        let tries = Arc::new(AtomicU32::new(0));
        let counter = tries.clone();
        let value = db
            .retrying(3, move || {
                let counter = counter.clone();
                async move {
                    match counter.fetch_add(1, Ordering::SeqCst) {
                        0 | 1 => Err(busy()),
                        _ => Ok("done"),
                    }
                }
            })
            .await
            .unwrap();
        assert_eq!((value, tries.load(Ordering::SeqCst)), ("done", 3));

        let tries = Arc::new(AtomicU32::new(0));
        let counter = tries.clone();
        let err = db
            .retrying(2, move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async { Err::<(), _>(busy()) }
            })
            .await
            .unwrap_err();
        assert!(err.is_retryable());
        assert_eq!(
            tries.load(Ordering::SeqCst),
            2,
            "gives up at the last attempt"
        );

        let tries = Arc::new(AtomicU32::new(0));
        let counter = tries.clone();
        let err = db
            .retrying(5, move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async { Err::<(), _>(crate::Error::NotFound) }
            })
            .await
            .unwrap_err();
        assert!(matches!(err, crate::Error::NotFound));
        assert_eq!(tries.load(Ordering::SeqCst), 1, "not retried");

        let tries = Arc::new(AtomicU32::new(0));
        let counter = tries.clone();
        let value = db
            .transaction_retrying(3, move |tx| {
                let first = counter.fetch_add(1, Ordering::SeqCst) == 0;
                Box::pin(async move {
                    let one: i64 = super::sql("SELECT CAST(1 AS BIGINT)").scalar(tx).await?;
                    if first { Err(busy()) } else { Ok(one) }
                })
            })
            .await
            .unwrap();
        assert_eq!((value, tries.load(Ordering::SeqCst)), (1, 2));
    }

    // #254: an Encrypted value on a Db made outside App has no key to seal with.
    #[test]
    fn sealing_needs_the_apps_key() {
        let err = seal(
            None,
            vec![DbValue::Encrypted(super::super::encrypted::Unsealed(
                "x".into(),
            ))],
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("an Encrypted value needs the app's Db"),
            "{err}"
        );
        // Plain values pass through untouched.
        assert!(matches!(
            seal(None, vec![DbValue::Integer(1)]).unwrap()[..],
            [DbValue::Integer(1)]
        ));
    }
}
