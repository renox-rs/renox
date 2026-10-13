use std::marker::PhantomData;

use super::column::{CondKind, Condition, IntoColumn};
use super::paginate::{CursorPage, SimplePage};
use super::{Db, DbValue, Dialect, Executor, FromDb, Model, Paginated, ToDbValue, now, quote, sql};
use crate::Result;
use anyhow::anyhow;

const OPERATORS: &[&str] = &["=", "!=", "<>", "<", "<=", ">", ">=", "like", "not like"];

#[derive(Clone, Copy, PartialEq)]
enum Trashed {
    Without,
    With,
    Only,
}

/// `where_in` lists longer than this are sent as one JSON array.
const LARGE_IN: usize = 1000;

/// A number type `Query::sum` can return.
pub trait Number: FromDb + sealed::Sealed {
    #[doc(hidden)]
    const SQL_TYPE: &'static str;
}

impl Number for i64 {
    const SQL_TYPE: &'static str = "BIGINT";
}

impl Number for f64 {
    const SQL_TYPE: &'static str = "DOUBLE PRECISION";
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for i64 {}
    impl Sealed for f64 {}
}

/// One condition of a query, rendered per database.
#[derive(Clone)]
enum Filter {
    /// SQL with `?` placeholders.
    Sql(String),
    /// `column LIKE ?` (ILIKE on PostgreSQL); `not` for NOT LIKE.
    Like { column: String, not: bool },
    /// `column IN (…)` for a long list sent as one JSON array.
    JsonIn { kind: &'static str, column: String },
    /// Conditions joined with OR (`any`) or AND, in parentheses.
    Group { any: bool, filters: Vec<Filter> },
    /// `NOT (…)`.
    Not(Box<Filter>),
    /// `[NOT] EXISTS (SELECT 1 FROM table WHERE correlation AND …)`.
    Exists {
        not: bool,
        table: &'static str,
        correlation: String,
        filters: Vec<Filter>,
    },
    /// `column IN (SELECT sub_column FROM table WHERE …)`.
    InQuery {
        column: String,
        table: &'static str,
        sub_column: String,
        filters: Vec<Filter>,
    },
    /// A full-text search on a model's index, already rendered per
    /// database (`search::filter_sql`), with one `?`.
    Search { sqlite: String, postgres: String },
}

/// One `ORDER BY` term.
#[derive(Clone)]
enum Order {
    /// SQL without values.
    Sql(String),
    /// Full-text relevance (`search::rank_sql`) per database, with one `?`
    /// in `order_binds`.
    Relevance { sqlite: String, postgres: String },
}

impl Order {
    fn render(&self, dialect: Dialect) -> &str {
        match (self, dialect) {
            (Order::Sql(sql), _) => sql,
            (Order::Relevance { sqlite, .. }, Dialect::Sqlite) => sqlite,
            (Order::Relevance { postgres, .. }, Dialect::Postgres) => postgres,
        }
    }
}

impl Filter {
    fn render(&self, dialect: Dialect) -> String {
        match self {
            Filter::Sql(sql) => sql.clone(),
            Filter::Like { column, not } => {
                let op = match (dialect, not) {
                    (Dialect::Postgres, false) => "ILIKE",
                    (Dialect::Postgres, true) => "NOT ILIKE",
                    (_, false) => "LIKE",
                    (_, true) => "NOT LIKE",
                };
                format!("{column} {op} ?")
            }
            Filter::JsonIn { kind, column } => json_in(kind, column, dialect),
            Filter::Group { any, filters } => {
                if filters.is_empty() {
                    // Nothing to match for `any`, nothing to restrict for AND.
                    return if *any { "0 = 1".into() } else { "1 = 1".into() };
                }
                let joiner = if *any { " OR " } else { " AND " };
                let parts: Vec<String> = filters.iter().map(|f| f.render(dialect)).collect();
                format!("({})", parts.join(joiner))
            }
            Filter::Not(filter) => format!("NOT ({})", filter.render(dialect)),
            Filter::Exists {
                not,
                table,
                correlation,
                filters,
            } => {
                let mut parts = vec![correlation.clone()];
                parts.extend(filters.iter().map(|f| f.render(dialect)));
                format!(
                    "{}EXISTS (SELECT 1 FROM {} WHERE {})",
                    if *not { "NOT " } else { "" },
                    quote(table),
                    parts.join(" AND ")
                )
            }
            Filter::InQuery {
                column,
                table,
                sub_column,
                filters,
            } => {
                let condition = if filters.is_empty() {
                    String::new()
                } else {
                    let parts: Vec<String> = filters.iter().map(|f| f.render(dialect)).collect();
                    format!(" WHERE {}", parts.join(" AND "))
                };
                format!(
                    "{column} IN (SELECT {sub_column} FROM {}{condition})",
                    quote(table)
                )
            }
            Filter::Search { sqlite, postgres } => match dialect {
                Dialect::Sqlite => sqlite.clone(),
                Dialect::Postgres => postgres.clone(),
            },
        }
    }
}

/// A long list of integers or of strings, as a JSON array.
fn large_list(values: &[DbValue]) -> Option<(&'static str, serde_json::Value)> {
    if values.len() <= LARGE_IN {
        return None;
    }
    if let Some(ints) = values
        .iter()
        .map(|v| match v {
            DbValue::Integer(n) => Some(serde_json::Value::from(*n)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
    {
        return Some(("int", ints.into()));
    }
    values
        .iter()
        .map(|v| match v {
            DbValue::Text(s) => Some(serde_json::Value::from(s.clone())),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|texts| ("text", texts.into()))
}

fn json_in(kind: &str, column: &str, dialect: Dialect) -> String {
    match (dialect, kind) {
        (Dialect::Sqlite, _) => format!("{column} IN (SELECT value FROM json_each(?))"),
        (Dialect::Postgres, "int") => format!(
            "{column} IN (SELECT CAST(x AS BIGINT) FROM jsonb_array_elements_text(CAST(? AS JSONB)) AS t(x))"
        ),
        (Dialect::Postgres, _) => format!(
            "{column} IN (SELECT x FROM jsonb_array_elements_text(CAST(? AS JSONB)) AS t(x))"
        ),
    }
}

/// A query on a model's table, built with chained filters.
///
/// ```
/// # #[derive(Model, serde::Serialize, Default)]
/// # #[model(table = "products")]
/// # struct Product { id: i64, name: String, price: i64, category: Option<String>, user_id: i64 }
/// # use renox::prelude::*;
/// # async fn demo(db: Db, page: u32) -> Result {
/// let products = Product::query()
///     .where_eq("category", "coffee")
///     .where_op("price", "<", 25_000)
///     .order_by("name")
///     .paginate(&db, page, 20)
///     .await?;
/// # let _ = products; Ok(()) }
/// ```
///
/// Column names are checked against the model; an unknown column or operator
/// makes the query return an error instead of running.
pub struct Query<M> {
    filters: Vec<Filter>,
    binds: Vec<DbValue>,
    group: Vec<String>,
    having: Vec<String>,
    having_binds: Vec<DbValue>,
    order: Vec<Order>,
    /// Values of the `ORDER BY` terms, bound after the WHERE and HAVING ones.
    order_binds: Vec<DbValue>,
    limit: Option<u64>,
    offset: Option<u64>,
    lock: Option<&'static str>,
    trashed: Trashed,
    error: Option<String>,
    model: PhantomData<fn() -> M>,
}

impl<M> Clone for Query<M> {
    fn clone(&self) -> Self {
        Self {
            filters: self.filters.clone(),
            binds: self.binds.clone(),
            group: self.group.clone(),
            having: self.having.clone(),
            having_binds: self.having_binds.clone(),
            order: self.order.clone(),
            order_binds: self.order_binds.clone(),
            limit: self.limit,
            offset: self.offset,
            lock: self.lock,
            trashed: self.trashed,
            error: self.error.clone(),
            model: PhantomData,
        }
    }
}

impl<M: Model> Query<M> {
    pub(crate) fn new() -> Self {
        Self {
            filters: Vec::new(),
            binds: Vec::new(),
            group: Vec::new(),
            having: Vec::new(),
            having_binds: Vec::new(),
            order: Vec::new(),
            order_binds: Vec::new(),
            limit: None,
            offset: None,
            lock: None,
            trashed: Trashed::Without,
            error: None,
            model: PhantomData,
        }
    }

    fn column(&mut self, column: &str) -> Option<String> {
        let plain = !column.is_empty()
            && column
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if M::COLUMNS.contains(&column) || (M::SELECT_ALL && plain) {
            Some(quote(column))
        } else {
            self.error
                .get_or_insert_with(|| format!("`{}` has no column `{column}`", M::TABLE));
            None
        }
    }

    /// Filters on `column = value`.
    pub fn where_eq(self, column: &str, value: impl ToDbValue) -> Self {
        self.where_op(column, "=", value)
    }

    /// Filters on a typed [`Condition`] made by a [`Col`](super::Col), like
    /// `Product::PRICE.lt(20_000)`.
    pub fn where_(self, condition: Condition<M>) -> Self {
        match condition.kind {
            CondKind::Op(column, op, value) => self.where_op(column, op, value),
            CondKind::Null(column, true) => self.where_null(column),
            CondKind::Null(column, false) => self.where_not_null(column),
            CondKind::In(column, values) => self.where_in(column, values),
        }
    }

    /// Filters with a comparison: `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`, `like`, `not like`.
    /// `like` ignores ASCII case on both databases (`ILIKE` on PostgreSQL, as
    /// SQLite's `LIKE` already does).
    pub fn where_op(mut self, column: &str, op: &str, value: impl ToDbValue) -> Self {
        let op = op.to_ascii_lowercase();
        if !OPERATORS.contains(&op.as_str()) {
            self.error
                .get_or_insert_with(|| format!("unsupported operator `{op}`"));
            return self;
        }
        if let Some(column) = self.column(column) {
            self.filters.push(match op.as_str() {
                "like" => Filter::Like { column, not: false },
                "not like" => Filter::Like { column, not: true },
                _ => Filter::Sql(format!("{column} {} ?", op.to_uppercase())),
            });
            self.binds.push(value.to_db_value());
        }
        self
    }

    /// Filters on `column LIKE pattern`, ignoring ASCII case (`%` and `_` are wildcards).
    pub fn where_like(self, column: &str, pattern: impl ToDbValue) -> Self {
        self.where_op(column, "like", pattern)
    }

    /// Filters on `column IS NULL`.
    pub fn where_null(mut self, column: &str) -> Self {
        if let Some(column) = self.column(column) {
            self.filters.push(Filter::Sql(format!("{column} IS NULL")));
        }
        self
    }

    /// Filters on `column IS NOT NULL`.
    pub fn where_not_null(mut self, column: &str) -> Self {
        if let Some(column) = self.column(column) {
            self.filters
                .push(Filter::Sql(format!("{column} IS NOT NULL")));
        }
        self
    }

    /// Filters on `column IN (…)`; an empty list matches no rows.
    pub fn where_in<V: ToDbValue>(self, column: &str, values: impl IntoIterator<Item = V>) -> Self {
        self.in_list(column, values, false)
    }

    /// Filters on `column NOT IN (…)`; an empty list matches every row.
    pub fn where_not_in<V: ToDbValue>(
        self,
        column: &str,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        self.in_list(column, values, true)
    }

    fn in_list<V: ToDbValue>(
        mut self,
        column: &str,
        values: impl IntoIterator<Item = V>,
        not: bool,
    ) -> Self {
        let values: Vec<DbValue> = values.into_iter().map(|v| v.to_db_value()).collect();
        if let Some(column) = self.column(column) {
            let filter = if values.is_empty() {
                // Nothing is in an empty list.
                Filter::Sql("0 = 1".into())
            } else if let Some((kind, list)) = large_list(&values) {
                // Over the databases' bind limits: one JSON array instead.
                self.binds.push(DbValue::Json(list));
                Filter::JsonIn { kind, column }
            } else {
                let marks = vec!["?"; values.len()].join(", ");
                self.binds.extend(values);
                Filter::Sql(format!("{column} IN ({marks})"))
            };
            self.filters.push(if not {
                Filter::Not(Box::new(filter))
            } else {
                filter
            });
        }
        self
    }

    /// `low <= column <= high`.
    pub fn where_between(
        mut self,
        column: &str,
        low: impl ToDbValue,
        high: impl ToDbValue,
    ) -> Self {
        if let Some(column) = self.column(column) {
            self.filters
                .push(Filter::Sql(format!("{column} BETWEEN ? AND ?")));
            self.binds.push(low.to_db_value());
            self.binds.push(high.to_db_value());
        }
        self
    }

    /// Rows whose `column` is among `sub_column`'s values in another model's
    /// query, e.g. products in active categories:
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, category_id: i64 }
    /// # #[derive(Model, serde::Serialize, Default)] struct Category { id: i64, active: bool }
    /// # async fn demo(db: Db) -> Result {
    /// let products = Product::query()
    ///     .where_in_query("category_id", Category::where_eq("active", true), "id")
    ///     .get(&db)
    ///     .await?;
    /// # let _ = products; Ok(()) }
    /// ```
    pub fn where_in_query<N: Model>(
        mut self,
        column: &str,
        mut sub: Query<N>,
        sub_column: &str,
    ) -> Self {
        let sub_column = sub.column(sub_column);
        let column = self.column(column);
        if let Some(error) = sub.error.take() {
            self.error.get_or_insert(error);
        }
        if let (Some(column), Some(sub_column)) = (column, sub_column) {
            let trashed = sub.trashed_filter();
            let mut filters = sub.filters;
            filters.extend(trashed);
            self.filters.push(Filter::InQuery {
                column,
                table: N::TABLE,
                sub_column,
                filters,
            });
            self.binds.extend(sub.binds);
        }
        self
    }

    /// Like `where_in_query`, keeping the rows whose `column` is *not* among
    /// the sub-query's values.
    pub fn where_not_in_query<N: Model>(
        self,
        column: &str,
        sub: Query<N>,
        sub_column: &str,
    ) -> Self {
        let before = self.filters.len();
        let mut query = self.where_in_query(column, sub, sub_column);
        if query.filters.len() > before
            && let Some(last) = query.filters.pop()
        {
            query.filters.push(Filter::Not(Box::new(last)));
        }
        query
    }

    /// Rows that have at least one related row in `children`, joined by the
    /// children's `foreign_key` column to this model's `id` (`EXISTS`):
    /// products with a 5-star review.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, name: String }
    /// # #[derive(Model, serde::Serialize, Default)] struct Review { id: i64, product_id: i64, stars: i64 }
    /// # async fn demo(db: Db) -> Result {
    /// let loved = Product::query()
    ///     .where_has(Review::where_eq("stars", 5), "product_id")
    ///     .get(&db)
    ///     .await?;
    /// let unreviewed = Product::query().where_doesnt_have(Review::query(), "product_id").count(&db).await?;
    /// # let _ = (loved, unreviewed); Ok(()) }
    /// ```
    pub fn where_has<N: Model>(self, children: Query<N>, foreign_key: &str) -> Self {
        self.related(children, foreign_key, false)
    }

    /// Rows without any related row in `children` (`NOT EXISTS`).
    pub fn where_doesnt_have<N: Model>(self, children: Query<N>, foreign_key: &str) -> Self {
        self.related(children, foreign_key, true)
    }

    fn related<N: Model>(mut self, mut children: Query<N>, foreign_key: &str, not: bool) -> Self {
        let foreign_key = children.column(foreign_key);
        if let Some(error) = children.error.take() {
            self.error.get_or_insert(error);
        }
        if let Some(foreign_key) = foreign_key {
            let trashed = children.trashed_filter();
            let mut filters = children.filters;
            filters.extend(trashed);
            self.filters.push(Filter::Exists {
                not,
                table: N::TABLE,
                correlation: format!(
                    "{}.{foreign_key} = {}.\"id\"",
                    quote(N::TABLE),
                    quote(M::TABLE)
                ),
                filters,
            });
            self.binds.extend(children.binds);
        }
        self
    }

    /// Any of the conditions `group` adds must hold (`OR`), in parentheses:
    /// `.where_any(|q| q.where_eq("status", "new").where_op("total", ">", 100))`.
    pub fn where_any(self, group: impl FnOnce(Self) -> Self) -> Self {
        self.group(true, group)
    }

    /// All of the conditions `group` adds must hold, in parentheses; useful
    /// inside `where_any`: `.where_any(|q| q.where_eq("a", 1).where_all(|q| …))`.
    pub fn where_all(self, group: impl FnOnce(Self) -> Self) -> Self {
        self.group(false, group)
    }

    fn group(mut self, any: bool, group: impl FnOnce(Self) -> Self) -> Self {
        let mut inner = group(Self::new());
        if let Some(error) = inner.error.take() {
            self.error.get_or_insert(error);
        }
        self.filters.push(Filter::Group {
            any,
            filters: inner.filters,
        });
        self.binds.extend(inner.binds);
        self
    }

    /// Applies `add` only when `condition` holds, e.g. an optional search:
    /// `.when(!q.is_empty(), |query| query.where_like("name", format!("%{q}%")))`.
    pub fn when(self, condition: bool, add: impl FnOnce(Self) -> Self) -> Self {
        if condition { add(self) } else { self }
    }

    /// The rows matching a full-text search, best matches first (as
    /// [`where_search`](Self::where_search) then
    /// [`order_by_relevance`](Self::order_by_relevance)); more `order_by`
    /// calls break ties. The model needs `#[model(search = "…")]` and its
    /// index: see [`renox::db::search`](super::search). A text without any
    /// word changes nothing.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// #[derive(Model, serde::Serialize, Default)]
    /// #[model(table = "posts", search = "title, body", soft_deletes)]
    /// struct Post {
    ///     id: i64,
    ///     title: String,
    ///     body: String,
    ///     author_id: i64,
    ///     deleted_at: Option<DateTime>,
    /// }
    ///
    /// # async fn demo(db: Db, q: String) -> Result {
    /// let page = Post::query()
    ///     .where_eq("author_id", 7)
    ///     .search(&q)
    ///     .order_by_desc("id")
    ///     .paginate(&db, 1, 20)
    ///     .await?;
    /// # let _ = page; Ok(()) }
    /// ```
    pub fn search(self, words: &str) -> Self {
        self.where_search(words).order_by_relevance(words)
    }

    /// Keeps the rows matching a full-text search (every word, as a word
    /// or the start of one), without ordering them. User input is safe
    /// here: only its words reach the database, bound as a value.
    pub fn where_search(mut self, words: &str) -> Self {
        if let Some(problem) = super::search::unsearchable::<M>() {
            self.error.get_or_insert(problem);
            return self;
        }
        if let Some(terms) = super::search::bound(words) {
            self.filters.push(Filter::Search {
                sqlite: super::search::filter_sql::<M>(Dialect::Sqlite),
                postgres: super::search::filter_sql::<M>(Dialect::Postgres),
            });
            self.binds.push(DbValue::Text(terms));
        }
        self
    }

    /// Sorts by how well rows match a full-text search, best first (rows
    /// that don't match last); call `order_by` after it to break ties.
    pub fn order_by_relevance(mut self, words: &str) -> Self {
        if let Some(problem) = super::search::unsearchable::<M>() {
            self.error.get_or_insert(problem);
            return self;
        }
        if let Some(terms) = super::search::bound(words) {
            self.order.push(Order::Relevance {
                sqlite: super::search::rank_sql::<M>(Dialect::Sqlite),
                postgres: super::search::rank_sql::<M>(Dialect::Postgres),
            });
            self.order_binds.push(DbValue::Text(terms));
        }
        self
    }

    /// Sorts by `column`, ascending; call again to add tie-breakers.
    pub fn order_by(mut self, column: impl IntoColumn<M>) -> Self {
        if let Some(column) = self.column(column.column_name()) {
            self.order.push(Order::Sql(format!("{column} ASC")));
        }
        self
    }

    /// Sorts by `column`, descending; call again to add tie-breakers.
    pub fn order_by_desc(mut self, column: impl IntoColumn<M>) -> Self {
        if let Some(column) = self.column(column.column_name()) {
            self.order.push(Order::Sql(format!("{column} DESC")));
        }
        self
    }

    /// Newest first, by `created_at` when the model has it, otherwise by `id`.
    pub fn latest(self) -> Self {
        let column = if M::COLUMNS.contains(&"created_at") {
            "created_at"
        } else {
            "id"
        };
        self.order_by_desc(column).order_by_desc("id")
    }

    /// At most `limit` rows (a limit past `i64::MAX` means no limit).
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit.min(i64::MAX as u64));
        self
    }

    /// Skips the first `offset` rows.
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset.min(i64::MAX as u64));
        self
    }

    /// A condition in SQL, for what the builder doesn't cover (dates, JSON,
    /// full-text search, …), with `?` for each value. Column names are yours
    /// to quote; never build `sql` from user input.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, total: i64, created_at: Option<DateTime> }
    /// # async fn demo(db: Db) -> Result {
    /// let today = Order::query()
    ///     .where_raw("DATE(created_at) = DATE(?)", [renox::db::now()])
    ///     .order_by_raw("total DESC, id")
    ///     .get(&db)
    ///     .await?;
    /// # let _ = today; Ok(()) }
    /// ```
    pub fn where_raw<V: ToDbValue>(
        mut self,
        sql: &str,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        self.filters.push(Filter::Sql(format!("({sql})")));
        self.binds
            .extend(values.into_iter().map(|v| v.to_db_value()));
        self
    }

    /// An `ORDER BY` term in SQL, e.g. `"total DESC, id"` (no values; never
    /// from user input).
    pub fn order_by_raw(mut self, sql: &str) -> Self {
        self.order.push(Order::Sql(sql.to_owned()));
        self
    }

    /// Groups rows by `column`, for `select_as` and `count`:
    /// `.group_by("user_id").select_as::<(i64, i64), _>(&db, "user_id, COUNT(*)")`.
    pub fn group_by(mut self, column: &str) -> Self {
        if let Some(column) = self.column(column) {
            self.group.push(column);
        }
        self
    }

    /// Groups rows by a SQL expression (written by the app, never from a
    /// request): `.group_by_raw("CAST(\"price\" / 100 AS INTEGER)")`.
    pub fn group_by_raw(mut self, sql: &str) -> Self {
        self.group.push(sql.to_owned());
        self
    }

    /// A condition on the groups, in SQL with `?` for each value:
    /// `.having_raw("COUNT(*) > ?", [2])`.
    pub fn having_raw<V: ToDbValue>(
        mut self,
        sql: &str,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        self.having.push(format!("({sql})"));
        self.having_binds
            .extend(values.into_iter().map(|v| v.to_db_value()));
        self
    }

    /// Locks the matching rows until the transaction ends (`FOR UPDATE`),
    /// e.g. to read a balance and write it back safely. Use it on `&mut tx`.
    /// PostgreSQL only: on SQLite a write transaction already holds the whole
    /// database, so start it with `db.begin_immediate()` instead.
    pub fn lock_for_update(mut self) -> Self {
        self.lock = Some("FOR UPDATE");
        self
    }

    /// Like `lock_for_update`, but others may still read-lock (`FOR SHARE`).
    pub fn shared_lock(mut self) -> Self {
        self.lock = Some("FOR SHARE");
        self
    }

    /// Matches no rows at all, e.g. a default scope when no tenant is set.
    pub fn none(mut self) -> Self {
        self.filters.push(Filter::Sql("1 = 0".into()));
        self
    }

    /// Include soft-deleted rows.
    pub fn with_trashed(mut self) -> Self {
        self.trashed = Trashed::With;
        self
    }

    /// Only soft-deleted rows.
    pub fn only_trashed(mut self) -> Self {
        self.trashed = Trashed::Only;
        self
    }

    fn check(&self) -> Result {
        match &self.error {
            Some(error) => Err(anyhow!("invalid query: {error}").into()),
            None => Ok(()),
        }
    }

    /// The soft-delete condition, if the model has soft deletes.
    fn trashed_filter(&self) -> Option<Filter> {
        if !M::SOFT_DELETES {
            return None;
        }
        match self.trashed {
            Trashed::Without => Some(Filter::Sql("\"deleted_at\" IS NULL".into())),
            Trashed::Only => Some(Filter::Sql("\"deleted_at\" IS NOT NULL".into())),
            Trashed::With => None,
        }
    }

    fn where_sql(&self, dialect: Dialect) -> String {
        let mut filters: Vec<String> = self.filters.iter().map(|f| f.render(dialect)).collect();
        filters.extend(self.trashed_filter().map(|f| f.render(dialect)));
        if filters.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", filters.join(" AND "))
        }
    }

    fn select_sql(&self, dialect: Dialect) -> String {
        let columns: Vec<String> = if M::SELECT_ALL {
            vec!["*".to_owned()]
        } else {
            M::COLUMNS.iter().map(|c| quote(c)).collect()
        };
        self.select_columns_sql(dialect, &columns.join(", "))
    }

    fn select_columns_sql(&self, dialect: Dialect, columns: &str) -> String {
        let mut sql = format!(
            "SELECT {columns} FROM {}{}",
            quote(M::TABLE),
            self.where_sql(dialect)
        );
        sql.push_str(&self.group_sql());
        if !self.order.is_empty() {
            let terms: Vec<&str> = self.order.iter().map(|o| o.render(dialect)).collect();
            sql.push_str(&format!(" ORDER BY {}", terms.join(", ")));
        }
        match (self.limit, self.offset) {
            (Some(limit), Some(offset)) => sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}")),
            (Some(limit), None) => sql.push_str(&format!(" LIMIT {limit}")),
            // SQLite needs a LIMIT before OFFSET; -1 means none.
            (None, Some(offset)) if dialect == Dialect::Sqlite => {
                sql.push_str(&format!(" LIMIT -1 OFFSET {offset}"))
            }
            (None, Some(offset)) => sql.push_str(&format!(" OFFSET {offset}")),
            (None, None) => {}
        }
        if let (Some(lock), Dialect::Postgres) = (self.lock, dialect) {
            sql.push(' ');
            sql.push_str(lock);
        }
        sql
    }

    fn group_sql(&self) -> String {
        let mut sql = String::new();
        if !self.group.is_empty() {
            sql.push_str(&format!(" GROUP BY {}", self.group.join(", ")));
        }
        if !self.having.is_empty() {
            sql.push_str(&format!(" HAVING {}", self.having.join(" AND ")));
        }
        sql
    }

    /// The query's values in the order its SQL uses them (without the
    /// `ORDER BY` ones: see `select_binds`).
    fn all_binds(&self) -> Vec<DbValue> {
        let mut binds = self.binds.clone();
        binds.extend(self.having_binds.iter().cloned());
        binds
    }

    /// The values of a SELECT with its `ORDER BY` (`select_columns_sql`).
    fn select_binds(&self) -> Vec<DbValue> {
        let mut binds = self.all_binds();
        binds.extend(self.order_binds.iter().cloned());
        binds
    }

    fn clear_order(&mut self) {
        self.order.clear();
        self.order_binds.clear();
    }

    /// Records an error if `column` isn't one of the model's.
    pub(crate) fn check_column(mut self, column: &str) -> Self {
        let _ = self.column(column);
        self
    }

    /// The SELECT this query runs and its values, e.g. to log or debug it.
    pub fn to_sql(&self, dialect: Dialect) -> Result<(String, Vec<DbValue>)> {
        self.check()?;
        Ok((self.select_sql(dialect), self.select_binds()))
    }

    /// Selects `columns` (SQL, e.g. `"user_id, COUNT(*) AS orders"`) of the
    /// matching rows, with `group_by`/`having_raw`, read into a
    /// `#[derive(FromRow)]` struct or a tuple:
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, user_id: i64, total: i64, status: String }
    /// # async fn demo(db: Db) -> Result {
    /// let big_spenders: Vec<(i64, i64)> = Order::where_eq("status", "paid")
    ///     .group_by("user_id")
    ///     .having_raw("SUM(total) > ?", [1_000_000])
    ///     .order_by_raw("2 DESC")
    ///     .select_as(&db, "user_id, CAST(SUM(total) AS BIGINT)")
    ///     .await?;
    /// # let _ = big_spenders; Ok(()) }
    /// ```
    pub async fn select_as<'c, T: super::FromRow, E: Executor<'c>>(
        self,
        db: E,
        columns: &str,
    ) -> Result<Vec<T>> {
        self.check()?;
        let db = db.into_conn();
        let statement = self.select_columns_sql(db.dialect(), columns);
        Ok(sql(statement)
            .bind_all(self.select_binds())
            .fetch_as(db)
            .await?)
    }

    /// `bucket` and an aggregate per bucket (`chart::Trend`): the query's
    /// conditions, grouped by the first column; its order, limit, groups
    /// and lock don't apply.
    pub(crate) async fn buckets<'c, E: Executor<'c>>(
        mut self,
        db: E,
        bucket: &(dyn Fn(Dialect) -> String + Send + Sync),
        aggregate: &str,
    ) -> Result<Vec<(String, Option<f64>)>> {
        self.check()?;
        self.clear_order();
        self.group = vec!["1".to_owned()];
        self.having.clear();
        self.having_binds.clear();
        self.limit = None;
        self.offset = None;
        self.lock = None;
        let db = db.into_conn();
        let dialect = db.dialect();
        let statement = self.select_columns_sql(
            dialect,
            &format!("{}, CAST({aggregate} AS DOUBLE PRECISION)", bucket(dialect)),
        );
        Ok(sql(statement)
            .bind_all(self.all_binds())
            .fetch_as(db)
            .await?)
    }

    /// One aggregate over the matching rows (order and limit don't apply).
    async fn aggregate<'c, T: FromDb, E: Executor<'c>>(
        self,
        db: E,
        expression: String,
    ) -> Result<T> {
        self.check()?;
        let db = db.into_conn();
        let statement = format!(
            "SELECT {expression} FROM {}{}",
            quote(M::TABLE),
            self.where_sql(db.dialect())
        );
        Ok(sql(statement).bind_all(self.binds).scalar(db).await?)
    }

    /// The sum of `column`, 0 without rows: `sum::<i64>(…)` for whole
    /// numbers (money in its smallest unit), `sum::<f64>(…)` for measures.
    pub async fn sum<'c, T: Number, E: Executor<'c>>(mut self, db: E, column: &str) -> Result<T> {
        let Some(column) = self.column(column) else {
            return Err(self.check().unwrap_err());
        };
        let expression = format!("CAST(COALESCE(SUM({column}), 0) AS {})", T::SQL_TYPE);
        self.aggregate(db, expression).await
    }

    /// The average of `column`, or `None` without rows.
    pub async fn avg<'c, E: Executor<'c>>(mut self, db: E, column: &str) -> Result<Option<f64>> {
        let Some(column) = self.column(column) else {
            return Err(self.check().unwrap_err());
        };
        self.aggregate(db, format!("CAST(AVG({column}) AS DOUBLE PRECISION)"))
            .await
    }

    /// The smallest value of `column`, or `None` without rows.
    pub async fn min<'c, T: FromDb, E: Executor<'c>>(
        mut self,
        db: E,
        column: &str,
    ) -> Result<Option<T>>
    where
        Option<T>: FromDb,
    {
        let Some(column) = self.column(column) else {
            return Err(self.check().unwrap_err());
        };
        self.aggregate(db, format!("MIN({column})")).await
    }

    /// The largest value of `column`, or `None` without rows.
    pub async fn max<'c, T: FromDb, E: Executor<'c>>(
        mut self,
        db: E,
        column: &str,
    ) -> Result<Option<T>>
    where
        Option<T>: FromDb,
    {
        let Some(column) = self.column(column) else {
            return Err(self.check().unwrap_err());
        };
        self.aggregate(db, format!("MAX({column})")).await
    }

    /// One column of the matching rows, in the query's order:
    /// `Product::query().order_by("name").pluck::<String, _>(&db, "name")`.
    pub async fn pluck<'c, T: FromDb, E: Executor<'c>>(
        mut self,
        db: E,
        column: impl IntoColumn<M>,
    ) -> Result<Vec<T>> {
        let Some(column) = self.column(column.column_name()) else {
            return Err(self.check().unwrap_err());
        };
        self.check()?;
        let db = db.into_conn();
        let statement = self.select_columns_sql(db.dialect(), &column);
        Ok(sql(statement)
            .bind_all(self.select_binds())
            .scalars(db)
            .await?)
    }

    /// Sets columns on every matching row (and `updated_at` when the model
    /// has it); returns how many changed.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, status: String, paid_at: Option<DateTime> }
    /// # async fn demo(db: Db) -> Result {
    /// Order::where_eq("status", "pending")
    ///     .update(&db, &[("status", &"paid"), ("paid_at", &renox::db::now())])
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn update<'c, E: Executor<'c>>(
        mut self,
        db: E,
        values: &[(&str, &(dyn ToDbValue + Sync))],
    ) -> Result<u64> {
        let mut sets = Vec::new();
        let mut binds = Vec::new();
        for (column, value) in values {
            if *column == "id" {
                self.error
                    .get_or_insert_with(|| "update can't change `id`".into());
            }
            if let Some(quoted) = self.column(column) {
                sets.push(format!("{quoted} = ?"));
                binds.push(value.to_db_value());
            }
        }
        if M::COLUMNS.contains(&"updated_at") && !values.iter().any(|(c, _)| *c == "updated_at") {
            sets.push(format!("{} = ?", quote("updated_at")));
            binds.push(now().to_db_value());
        }
        if sets.is_empty() {
            self.check()?;
            return Ok(0);
        }
        self.set_rows(db, sets.join(", "), binds).await
    }

    /// Adds `by` to `column` on every matching row (negative to subtract),
    /// in the database, so concurrent changes aren't lost:
    /// `Product::where_eq("id", id).increment(&db, "stock", -1)`.
    pub async fn increment<'c, E: Executor<'c>>(
        mut self,
        db: E,
        column: &str,
        by: i64,
    ) -> Result<u64> {
        let Some(quoted) = self.column(column) else {
            return Err(self.check().unwrap_err());
        };
        let mut sets = format!("{quoted} = {quoted} + ?");
        let mut binds = vec![DbValue::Integer(by)];
        if M::COLUMNS.contains(&"updated_at") {
            sets.push_str(&format!(", {} = ?", quote("updated_at")));
            binds.push(now().to_db_value());
        }
        self.set_rows(db, sets, binds).await
    }

    async fn set_rows<'c, E: Executor<'c>>(
        self,
        db: E,
        sets: String,
        binds: Vec<DbValue>,
    ) -> Result<u64> {
        self.check()?;
        let db = db.into_conn();
        let statement = format!(
            "UPDATE {} SET {sets}{}",
            quote(M::TABLE),
            self.where_sql(db.dialect())
        );
        Ok(sql(statement)
            .bind_all(binds)
            .bind_all(self.binds)
            .execute(db)
            .await?)
    }

    /// Like `first`, but no row becomes a 404 response.
    pub async fn first_or_404<'c, E: Executor<'c>>(self, db: E) -> Result<M> {
        self.first(db).await?.ok_or(crate::Error::NotFound)
    }

    /// The first matching row, or `make()` saved as a new one. If another
    /// request creates it at the same moment (a unique index stops the
    /// second insert), the row it created is returned.
    //
    // Not an `async fn`: written that way, a handler awaiting it failed
    // axum's `Send` check (rustc issue #100013; see it/send_handlers.rs).
    #[allow(clippy::manual_async_fn)] // the `+ Send` in the signature is the point
    pub fn first_or_create<'a>(
        self,
        db: &'a Db,
        make: impl FnOnce() -> M + Send + 'a,
    ) -> impl Future<Output = Result<M>> + Send + 'a {
        async move {
            if let Some(found) = self.clone().first(db).await? {
                return Ok(found);
            }
            match M::create(db, make()).await {
                Ok(created) => Ok(created),
                Err(err) if err.is_unique_violation() => self.first(db).await?.ok_or(err),
                Err(err) => Err(err),
            }
        }
    }

    /// Runs `each` on the matching rows `size` at a time, in id order, so a
    /// large table never sits in memory at once. (The query's own order and
    /// limit don't apply.)
    pub async fn chunk<F, Fut>(self, db: &Db, size: u64, mut each: F) -> Result<u64>
    where
        F: FnMut(Vec<M>) -> Fut,
        Fut: std::future::Future<Output = Result>,
    {
        let mut last: Option<M::Key> = None;
        let mut seen = 0_u64;
        loop {
            let mut page = self.clone();
            page.clear_order();
            page.limit = None;
            page.offset = None;
            if let Some(last) = last.take() {
                page = page.where_op("id", ">", last);
            }
            let rows = page.order_by("id").limit(size.max(1)).get(db).await?;
            let Some(tail) = rows.last() else { break };
            last = Some(tail.id());
            let full = rows.len() as u64 == size.max(1);
            seen += rows.len() as u64;
            each(rows).await?;
            if !full {
                break;
            }
        }
        Ok(seen)
    }

    /// Runs the query and returns every matching row.
    pub async fn get<'c, E: Executor<'c>>(self, db: E) -> Result<Vec<M>> {
        self.check()?;
        let db = db.into_conn();
        let rows = sql(self.select_sql(db.dialect()))
            .bind_all(self.select_binds())
            .fetch_all(db)
            .await?;
        Ok(rows
            .iter()
            .map(M::from_row)
            .collect::<std::result::Result<_, _>>()?)
    }

    /// The first matching row (in the query's order), or `None`.
    pub async fn first<'c, E: Executor<'c>>(self, db: E) -> Result<Option<M>> {
        Ok(self.limit(1).get(db).await?.into_iter().next())
    }

    /// How many rows match (with `group_by`: how many groups).
    pub async fn count<'c, E: Executor<'c>>(self, db: E) -> Result<u64> {
        self.check()?;
        let db = db.into_conn();
        let statement = if self.group.is_empty() {
            format!(
                "SELECT COUNT(*) FROM {}{}",
                quote(M::TABLE),
                self.where_sql(db.dialect())
            )
        } else {
            format!(
                "SELECT COUNT(*) FROM (SELECT 1 AS one FROM {}{}{}) AS groups",
                quote(M::TABLE),
                self.where_sql(db.dialect()),
                self.group_sql()
            )
        };
        let count: i64 = sql(statement).bind_all(self.all_binds()).scalar(db).await?;
        Ok(count as u64)
    }

    /// Whether any row matches.
    pub async fn exists<'c, E: Executor<'c>>(self, db: E) -> Result<bool> {
        Ok(self.count(db).await? > 0)
    }

    /// One page of results plus the numbers needed to render page links.
    /// `page` starts at 1; `per_page` is capped at 1000.
    pub async fn paginate(self, db: &Db, page: u32, per_page: u32) -> Result<Paginated<M>> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 1000);
        let total = self.clone().count(db).await?;
        let items = self
            .limit(u64::from(per_page))
            .offset(u64::from(page - 1) * u64::from(per_page))
            .get(db)
            .await?;
        Ok(Paginated::new(items, page, per_page, total))
    }

    /// One page without counting the rows (one query instead of two): for
    /// "previous / next" links on large tables. `page` starts at 1.
    pub async fn simple_paginate(self, db: &Db, page: u32, per_page: u32) -> Result<SimplePage<M>> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 1000);
        let mut items = self
            .limit(u64::from(per_page) + 1)
            .offset(u64::from(page - 1) * u64::from(per_page))
            .get(db)
            .await?;
        let has_next = items.len() > per_page as usize;
        items.truncate(per_page as usize);
        Ok(SimplePage {
            items,
            page,
            per_page,
            has_prev: page > 1,
            has_next,
        })
    }

    /// The `per_page` newest rows after `cursor` (by id; the query's own
    /// order doesn't apply), for APIs and infinite scroll on big tables:
    /// unlike page numbers, rows added meanwhile don't shift the pages.
    /// "Newest" is the key's order: creation order for `i64`, ULIDs and
    /// UUID v7s; a `String` key pages in text order.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Event { id: i64, name: String }
    /// #[derive(serde::Deserialize)]
    /// struct Params { cursor: Option<String> }
    ///
    /// async fn events(State(db): State<Db>, Query(p): Query<Params>) -> Result<Json<renox::db::CursorPage<Event>>> {
    ///     Ok(Json(Event::query().cursor_paginate(&db, p.cursor.as_deref(), 50).await?))
    /// }
    /// ```
    pub async fn cursor_paginate(
        mut self,
        db: &Db,
        cursor: Option<&str>,
        per_page: u32,
    ) -> Result<CursorPage<M>> {
        let per_page = per_page.clamp(1, 1000);
        if let Some(cursor) = cursor {
            let Ok(after) = cursor.parse::<M::Key>() else {
                return Err(crate::Error::BadRequest("invalid cursor".into()));
            };
            self = self.where_op("id", "<", after);
        }
        self.clear_order();
        let mut items = self
            .order_by_desc("id")
            .limit(u64::from(per_page) + 1)
            .get(db)
            .await?;
        let more = items.len() > per_page as usize;
        items.truncate(per_page as usize);
        let next_cursor = more
            .then(|| items.last().map(|last| last.id().to_string()))
            .flatten();
        Ok(CursorPage {
            items,
            per_page,
            next_cursor,
        })
    }

    /// The first matching row, or `make()` unsaved (`firstOrNew`).
    pub async fn first_or_new(self, db: &Db, make: impl FnOnce() -> M + Send) -> Result<M> {
        Ok(match self.first(db).await? {
            Some(found) => found,
            None => make(),
        })
    }

    /// Changes the first matching row with `change`, or creates `make()`
    /// with `change` applied (`updateOrCreate`); returns it saved.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Setting { id: i64, user_id: i64, key: String, value: String }
    /// # async fn demo(db: Db) -> Result {
    /// let theme = Setting::where_eq("user_id", 7)
    ///     .where_eq("key", "theme")
    ///     .update_or_create(
    ///         &db,
    ///         || Setting { user_id: 7, key: "theme".into(), ..Default::default() },
    ///         |s| s.value = "dark".into(),
    ///     )
    ///     .await?;
    /// # let _ = theme; Ok(()) }
    /// ```
    #[allow(clippy::manual_async_fn)] // the `+ Send` in the signature is the point
    pub fn update_or_create<'a>(
        self,
        db: &'a Db,
        make: impl FnOnce() -> M + Send + 'a,
        change: impl FnOnce(&mut M) + Send + 'a,
    ) -> impl Future<Output = Result<M>> + Send + 'a {
        async move {
            let mut model = match self.first(db).await? {
                Some(found) => found,
                None => make(),
            };
            change(&mut model);
            model.save(db).await?;
            Ok(model)
        }
    }

    /// Deletes every matching row (soft-deletes them for models with soft deletes).
    pub async fn delete<'c, E: Executor<'c>>(self, db: E) -> Result<u64> {
        if M::SOFT_DELETES {
            self.check()?;
            let db = db.into_conn();
            return Ok(sql(format!(
                "UPDATE {} SET \"deleted_at\" = ?{}",
                quote(M::TABLE),
                self.where_sql(db.dialect())
            ))
            .bind(now())
            .bind_all(self.binds)
            .execute(db)
            .await?);
        }
        self.force_delete(db).await
    }

    /// Removes every matching row, even for models with soft deletes.
    pub async fn force_delete<'c, E: Executor<'c>>(self, db: E) -> Result<u64> {
        self.check()?;
        let db = db.into_conn();
        Ok(sql(format!(
            "DELETE FROM {}{}",
            quote(M::TABLE),
            self.where_sql(db.dialect())
        ))
        .bind_all(self.binds)
        .execute(db)
        .await?)
    }
}
