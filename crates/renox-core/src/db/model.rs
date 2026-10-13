use std::future::Future;

use super::{
    DateTime, Db, DbValue, Executor, ModelColumn, ModelIndex, ModelKey, Query, ToDbValue, now,
    quote, sql,
};
use crate::{Error, Result};
use anyhow::anyhow;

/// A struct stored as a row in a table. Derive it with `#[derive(Model)]`.
///
/// ```
/// # use renox::prelude::*;
/// # use serde::Serialize;
/// #[derive(Model, Serialize, Default)]
/// #[model(table = "products", soft_deletes)]
/// struct Product {
///     id: i64,
///     name: String,
///     price: i64,
///     created_at: Option<DateTime>,
///     updated_at: Option<DateTime>,
///     deleted_at: Option<DateTime>,
/// }
///
/// # async fn demo(db: Db) -> Result {
/// let mut coffee = Product { name: "Coffee".into(), price: 18_000, ..Default::default() };
/// coffee.save(&db).await?;                     // INSERT, sets id and timestamps
/// let cheap = Product::query().where_op("price", "<", 20_000).get(&db).await?;
/// # let _ = cheap; Ok(()) }
/// ```
///
/// Implement it with `#[derive(Model)]`: the hidden items it writes may
/// change in a minor release.
///
/// The primary key is the `id` column. Its type is the `id` field's: `i64`
/// (numbered by the database; `0` means "not saved yet"), or a
/// [`Ulid`](super::Ulid), a UUID or a `String` (see [`ModelKey`]).
pub trait Model: super::FromRow + Sized + Send + Sync + Unpin + 'static {
    /// The table name (the snake_case struct name unless `#[model(table = …)]`).
    const TABLE: &'static str;
    /// Every column, including `id`.
    const COLUMNS: &'static [&'static str];
    /// `delete()` sets `deleted_at` instead of removing the row, and queries
    /// skip deleted rows unless asked with `with_trashed()` / `only_trashed()`.
    const SOFT_DELETES: bool = false;
    /// Select every column (`*`) instead of `COLUMNS`, so `from_row` also
    /// sees columns the struct doesn't list, and queries may filter on them.
    /// The built-in `User` does this to keep the app's own columns.
    const SELECT_ALL: bool = false;
    /// The text columns full-text search looks in, most important first
    /// (they weigh more in the ranking); empty: the model can't be
    /// searched. Set it with `#[model(search = "title, body")]`; see
    /// [`renox::db::search`](super::search).
    const SEARCHABLE: &'static [&'static str] = &[];
    /// The language full-text search stems words for: `english` (the
    /// default) or a PostgreSQL text search configuration such as `simple`
    /// (no stemming) or `spanish`. Set it with
    /// `#[model(search_language = "simple")]`.
    const SEARCH_LANGUAGE: &'static str = "english";

    /// The type of the `id` field.
    type Key: ModelKey;

    /// The primary key.
    fn id(&self) -> Self::Key;
    /// Sets the primary key (after an insert, for keys the database numbers).
    #[doc(hidden)]
    fn set_id(&mut self, id: Self::Key);
    /// Values of every column except `id`, in `COLUMNS` order.
    #[doc(hidden)]
    fn values(&self) -> Vec<DbValue>;
    /// Updates `created_at` / `updated_at` if the model has them.
    #[doc(hidden)]
    fn touch(&mut self, _now: DateTime, _creating: bool) {}
    /// Updates `deleted_at` if the model has it.
    #[doc(hidden)]
    fn set_deleted_at(&mut self, _at: Option<DateTime>) {}
    /// Empties `created_at` / `updated_at` when they are `Option`s.
    #[doc(hidden)]
    fn forget_timestamps(&mut self) {}

    /// The model's columns with the kind of each field's type. The derive
    /// fills this in; a hand-written `impl Model` returns none.
    fn column_info() -> Vec<ModelColumn> {
        Vec::new()
    }

    /// The indexes declared with `#[model(index(…))]` and `#[model(unique(…))]`.
    fn indexes() -> Vec<ModelIndex> {
        Vec::new()
    }

    /// A copy of the model that isn't saved yet (Laravel's `replicate`):
    /// the same values, with an unsaved id, no `deleted_at` and, when they
    /// are `Option`s, no timestamps, so `save` inserts a new row. Change
    /// what must differ (a unique SKU, a name) before saving it, or show it
    /// in the "new" form for someone to finish ("Duplicate").
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default, Clone)]
    /// # #[model(table = "products")]
    /// # struct Product { id: i64, sku: String, name: String, created_at: Option<DateTime> }
    /// # async fn demo(db: Db) -> Result {
    /// let original = Product::find_or_404(&db, 1).await?;
    /// let mut copy = original.replicate();
    /// copy.sku = format!("{}-COPY", original.sku);
    /// copy.save(&db).await?; // a new row, with its own id
    /// # Ok(()) }
    /// ```
    fn replicate(&self) -> Self
    where
        Self: Clone,
    {
        let mut copy = self.clone();
        copy.set_id(Self::Key::default());
        copy.set_deleted_at(None);
        copy.forget_timestamps();
        copy
    }

    /// Conditions every query of this model starts with, e.g. the current
    /// tenant, read from [`renox::context`](mod@crate::context). `query()`,
    /// `find`, `all`, `where_eq` and the relation loaders apply it;
    /// [`Model::unscoped`] doesn't. Saving, deleting and restoring a loaded
    /// model work by its id. Set it with `#[model(default_scope = "…")]`:
    ///
    /// ```
    /// # use renox::prelude::*;
    /// #[derive(Clone)]
    /// struct CurrentTeam(i64);
    ///
    /// #[derive(Model, serde::Serialize, Default)]
    /// #[model(table = "projects", default_scope = "team_only")]
    /// struct Project { id: i64, team_id: i64, name: String }
    ///
    /// fn team_only(query: renox::db::Query<Project>) -> renox::db::Query<Project> {
    ///     match renox::context::get::<CurrentTeam>() {
    ///         Some(team) => query.where_eq("team_id", team.0),
    ///         None => query.none(), // no team, no rows: fail closed
    ///     }
    /// }
    /// ```
    fn default_scope(query: Query<Self>) -> Query<Self> {
        query
    }

    /// Runs before the row is written; an error stops the save. Implement
    /// [`ModelHooks`] and add `#[model(hooks)]` rather than overriding it.
    fn saving(&mut self, _creating: bool) -> Result {
        Ok(())
    }

    /// Runs after the row is written (inside the caller's transaction, if
    /// any); an error is returned by `save`. See [`ModelHooks`].
    fn saved(&self, _created: bool) -> impl Future<Output = Result> + Send {
        async { Ok(()) }
    }

    /// Runs before `delete`/`force_delete`; an error stops it. See [`ModelHooks`].
    fn deleting(&self) -> Result {
        Ok(())
    }

    /// Runs after `delete`/`force_delete`. See [`ModelHooks`].
    fn deleted(&self) -> impl Future<Output = Result> + Send {
        async { Ok(()) }
    }

    /// A query with the default scope applied (see [`Model::default_scope`]).
    fn query() -> Query<Self> {
        Self::default_scope(Query::new())
    }

    /// A query without the default scope, e.g. for an admin who sees every
    /// tenant. Soft-deleted rows stay hidden unless asked for.
    fn unscoped() -> Query<Self> {
        Query::new()
    }

    /// Reloads the model's row (e.g. after an `increment` or another request
    /// changed it); a deleted row is a 404.
    fn refresh(&mut self, db: &Db) -> impl Future<Output = Result<()>> + Send {
        async move {
            let id = self.id();
            *self = Self::unscoped()
                .with_trashed()
                .where_eq("id", id)
                .first_or_404(db)
                .await?;
            Ok(())
        }
    }

    /// The rows matching a full-text search, best matches first:
    /// shorthand for `query().search(words)`. The model needs
    /// `#[model(search = "…")]` and its index (see
    /// [`renox::db::search`](super::search)).
    ///
    /// ```
    /// # use renox::prelude::*;
    /// #[derive(Model, serde::Serialize, Default)]
    /// #[model(table = "posts", search = "title, body")]
    /// struct Post { id: i64, title: String, body: String }
    ///
    /// # async fn demo(db: Db, q: String) -> Result {
    /// let posts = Post::search(&q).limit(20).get(&db).await?;
    /// # let _ = posts; Ok(()) }
    /// ```
    fn search(words: &str) -> Query<Self> {
        Self::query().search(words)
    }

    /// Shorthand for `query().where_eq(column, value)`.
    fn where_eq(column: &str, value: impl ToDbValue) -> Query<Self> {
        Self::query().where_eq(column, value)
    }

    /// Every row, in id order.
    fn all<'c, E: Executor<'c>>(db: E) -> impl Future<Output = Result<Vec<Self>>> + Send {
        Self::query().order_by("id").get(db)
    }

    /// The row with this id, or `None`.
    fn find<'c, E: Executor<'c>>(
        db: E,
        id: Self::Key,
    ) -> impl Future<Output = Result<Option<Self>>> + Send {
        Self::query().where_eq("id", id).first(db)
    }

    /// The rows with these ids, in id order (missing ids are skipped).
    fn find_many<'c, E: Executor<'c>>(
        db: E,
        ids: impl IntoIterator<Item = Self::Key>,
    ) -> impl Future<Output = Result<Vec<Self>>> + Send {
        let ids: Vec<Self::Key> = ids.into_iter().collect();
        Self::query().where_in("id", ids).order_by("id").get(db)
    }

    /// Inserts many new models with a few statements (ids aren't returned;
    /// use `create` when you need them). Timestamps are set; unsaved ULID
    /// and UUID keys are made, `String` keys must be set, `i64` keys come
    /// from the database. Returns the number of rows inserted.
    fn insert_many<'c, E: Executor<'c>>(
        db: E,
        models: Vec<Self>,
    ) -> impl Future<Output = Result<u64>> + Send {
        async move { write_many::<Self>(db.into_conn(), models, None).await }
    }

    /// Inserts `models`, or updates the rows they clash with on the
    /// `unique_by` columns (which need a unique index), setting `update`
    /// columns (and `updated_at` when the model has it). Returns the rows
    /// written.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)] struct Stock { id: i64, sku: String, qty: i64 }
    /// # async fn demo(db: Db) -> Result {
    /// let feed = vec![Stock { sku: "COFFEE-1".into(), qty: 12, ..Default::default() }];
    /// Stock::upsert(&db, feed, &["sku"], &["qty"]).await?;
    /// # Ok(()) }
    /// ```
    fn upsert<'c, E: Executor<'c>>(
        db: E,
        models: Vec<Self>,
        unique_by: &[&str],
        update: &[&str],
    ) -> impl Future<Output = Result<u64>> + Send {
        let unique_by: Vec<String> = unique_by.iter().map(|c| (*c).to_owned()).collect();
        let update: Vec<String> = update.iter().map(|c| (*c).to_owned()).collect();
        async move {
            // Keys the app writes (ULID, UUID, String) may be the conflict
            // target; an `i64` id isn't written, and no key is ever updated.
            let key_target = !<Self::Key as ModelKey>::AUTO_INCREMENT;
            for column in unique_by.iter().chain(&update) {
                let id_allowed =
                    key_target && unique_by.contains(column) && !update.contains(column);
                if !Self::COLUMNS.contains(&column.as_str()) || (column == "id" && !id_allowed) {
                    return Err(
                        anyhow!("`{}` has no column `{column}` to upsert", Self::TABLE).into(),
                    );
                }
            }
            if unique_by.is_empty() {
                return Err(anyhow!("upsert needs at least one `unique_by` column").into());
            }
            write_many::<Self>(db.into_conn(), models, Some((unique_by, update))).await
        }
    }

    /// Like `find`, but a missing row becomes a 404 response.
    fn find_or_404<'c, E: Executor<'c>>(
        db: E,
        id: Self::Key,
    ) -> impl Future<Output = Result<Self>> + Send {
        let found = Self::find(db, id);
        async move { found.await?.ok_or(Error::NotFound) }
    }

    /// Inserts a new model and returns it with its id and timestamps.
    fn create<'c, E: Executor<'c>>(
        db: E,
        mut model: Self,
    ) -> impl Future<Output = Result<Self>> + Send {
        async move {
            model.insert(db).await?;
            Ok(model)
        }
    }

    /// Inserts the model as a new row, whatever its id: an unsaved id gets a
    /// new key (from the database for `i64`, a new ULID or UUID v7), a set
    /// one is written as it is (a `String` key must be set).
    fn insert<'c, E: Executor<'c>>(&mut self, db: E) -> impl Future<Output = Result> + Send {
        async move {
            self.saving(true)?;
            self.touch(now(), true);
            if self.id().is_unsaved()
                && let Some(key) = Self::Key::generate()
            {
                self.set_id(key);
            }
            let key = self.id();
            let mut columns: Vec<String> = Self::COLUMNS
                .iter()
                .filter(|c| **c != "id")
                .map(|c| quote(c))
                .collect();
            let mut values = self.values();
            let table = quote(Self::TABLE);
            if key.is_unsaved() {
                if !<Self::Key as ModelKey>::AUTO_INCREMENT {
                    return Err(anyhow!(
                        "set the `id` of a new {} row before inserting it",
                        Self::TABLE
                    )
                    .into());
                }
                let sql_text = if columns.is_empty() {
                    format!("INSERT INTO {table} DEFAULT VALUES RETURNING id")
                } else {
                    let marks = vec!["?"; columns.len()].join(", ");
                    format!(
                        "INSERT INTO {table} ({}) VALUES ({marks}) RETURNING id",
                        columns.join(", ")
                    )
                };
                let id: Self::Key = sql(sql_text).bind_all(values).scalar(db).await?;
                self.set_id(id);
            } else {
                columns.insert(0, quote("id"));
                values.insert(0, key.to_db_value());
                let marks = vec!["?"; columns.len()].join(", ");
                sql(format!(
                    "INSERT INTO {table} ({}) VALUES ({marks})",
                    columns.join(", ")
                ))
                .bind_all(values)
                .execute(db)
                .await?;
            }
            self.saved(true).await
        }
    }

    /// Inserts the model if its id is unsaved (`0`, an empty ULID…),
    /// otherwise updates its row.
    fn save<'c, E: Executor<'c>>(&mut self, db: E) -> impl Future<Output = Result> + Send {
        async move {
            if self.id().is_unsaved() {
                return self.insert(db).await;
            }
            self.saving(false)?;
            self.touch(now(), false);
            let columns: Vec<String> = Self::COLUMNS
                .iter()
                .filter(|c| **c != "id")
                .map(|c| quote(c))
                .collect();
            let values = self.values();
            let table = quote(Self::TABLE);
            if columns.is_empty() {
                return self.saved(false).await;
            }
            let sets: Vec<String> = columns.iter().map(|c| format!("{c} = ?")).collect();
            let changed = sql(format!(
                "UPDATE {table} SET {} WHERE id = ?",
                sets.join(", ")
            ))
            .bind_all(values)
            .bind(self.id())
            .execute(db)
            .await?;
            if changed == 0 {
                return Err(Error::NotFound);
            }
            self.saved(false).await
        }
    }

    /// Updates only `columns` (and `updated_at`, if the model has it) of a
    /// saved model, so a concurrent change to another column isn't
    /// overwritten. A column the `saving` hook changes is saved only if
    /// it's listed.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default)]
    /// # #[model(table = "posts")]
    /// # struct Post { id: i64, title: String, views: i64 }
    /// # async fn demo(db: Db, mut post: Post) -> Result {
    /// post.title = "New title".into();
    /// post.save_only(&db, &["title"]).await?; // leaves `views` alone
    /// # Ok(()) }
    /// ```
    fn save_only<'c, E: Executor<'c>>(
        &mut self,
        db: E,
        columns: &[&str],
    ) -> impl Future<Output = Result> + Send {
        let columns: Vec<String> = columns.iter().map(|c| (*c).to_owned()).collect();
        async move {
            self.saving(false)?;
            update_columns(self, db, columns).await
        }
    }

    /// Saves the columns that differ from `original` (the model as it was
    /// loaded), including those the `saving` hook changes, and returns
    /// whether anything was written. When nothing changed there's no query
    /// and no `saved` hook.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// # #[derive(Model, serde::Serialize, Default, Clone)]
    /// # #[model(table = "posts")]
    /// # struct Post { id: i64, title: String, views: i64 }
    /// # async fn demo(db: Db) -> Result {
    /// let original = Post::find_or_404(&db, 1).await?;
    /// let mut post = original.clone();
    /// post.title = "New title".into();
    /// post.save_changes(&db, &original).await?; // UPDATE posts SET title = ?
    /// # Ok(()) }
    /// ```
    fn save_changes<'c, E: Executor<'c>>(
        &mut self,
        db: E,
        original: &Self,
    ) -> impl Future<Output = Result<bool>> + Send {
        let hooked = self.saving(false);
        let changed: Vec<String> = Self::COLUMNS
            .iter()
            .filter(|c| **c != "id")
            .zip(self.values().into_iter().zip(original.values()))
            .filter(|(_, (now, before))| now != before)
            .map(|(column, _)| (*column).to_owned())
            .collect();
        async move {
            hooked?;
            if changed.is_empty() {
                return Ok(false);
            }
            update_columns(self, db, changed).await?;
            Ok(true)
        }
    }

    /// Deletes the row, or marks it deleted for models with soft deletes.
    fn delete<'c, E: Executor<'c>>(&mut self, db: E) -> impl Future<Output = Result> + Send {
        async move {
            if !Self::SOFT_DELETES {
                return self.force_delete(db).await;
            }
            self.deleting()?;
            let at = now();
            sql(format!(
                "UPDATE {} SET deleted_at = ? WHERE id = ?",
                quote(Self::TABLE)
            ))
            .bind(at)
            .bind(self.id())
            .execute(db)
            .await?;
            self.set_deleted_at(Some(at));
            self.deleted().await
        }
    }

    /// Removes the row, even for models with soft deletes.
    fn force_delete<'c, E: Executor<'c>>(&self, db: E) -> impl Future<Output = Result> + Send {
        async move {
            self.deleting()?;
            sql(format!("DELETE FROM {} WHERE id = ?", quote(Self::TABLE)))
                .bind(self.id())
                .execute(db)
                .await?;
            self.deleted().await
        }
    }

    /// Brings back a soft-deleted row.
    fn restore<'c, E: Executor<'c>>(&mut self, db: E) -> impl Future<Output = Result> + Send {
        async move {
            if !Self::SOFT_DELETES {
                return Err(anyhow!("{} does not use soft deletes", Self::TABLE).into());
            }
            sql(format!(
                "UPDATE {} SET deleted_at = NULL WHERE id = ?",
                quote(Self::TABLE)
            ))
            .bind(self.id())
            .execute(db)
            .await?;
            self.set_deleted_at(None);
            Ok(())
        }
    }
}

/// `save_only` after the `saving` hook: writes `columns` and `updated_at`,
/// then runs `saved`.
async fn update_columns<'c, M: Model, E: Executor<'c>>(
    model: &mut M,
    db: E,
    columns: Vec<String>,
) -> Result {
    if model.id().is_unsaved() {
        return Err(anyhow!("save_only on an unsaved {} row", M::TABLE).into());
    }
    for column in &columns {
        if column == "id" || !M::COLUMNS.contains(&column.as_str()) {
            return Err(anyhow!("{} has no column `{column}` to save", M::TABLE).into());
        }
    }
    model.touch(now(), false);
    let mut sets = Vec::new();
    let mut binds = Vec::new();
    for (column, value) in M::COLUMNS
        .iter()
        .filter(|c| **c != "id")
        .zip(model.values())
    {
        if columns.iter().any(|c| c == column) || *column == "updated_at" {
            sets.push(format!("{} = ?", quote(column)));
            binds.push(value);
        }
    }
    if !sets.is_empty() {
        let changed = sql(format!(
            "UPDATE {} SET {} WHERE id = ?",
            quote(M::TABLE),
            sets.join(", ")
        ))
        .bind_all(binds)
        .bind(model.id())
        .execute(db)
        .await?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
    }
    model.saved(false).await
}

/// Code that runs around a model's writes, like Laravel's model events:
/// fill a slug, check an invariant, forget a cache key, emit an event. Add
/// `#[model(hooks)]` and implement the ones you need:
///
/// ```
/// # use renox::prelude::*;
/// use renox::db::ModelHooks;
///
/// #[derive(Model, serde::Serialize, Default)]
/// #[model(table = "posts", hooks)]
/// struct Post { id: i64, title: String, slug: String }
///
/// impl ModelHooks for Post {
///     fn saving(&mut self, _creating: bool) -> Result {
///         self.slug = self.title.to_lowercase().replace(' ', "-");
///         Ok(())
///     }
///
///     async fn saved(&self, _created: bool) -> Result {
///         if let Some(state) = renox::context::app() { // the request's or job's app
///             state.cache.forget("posts.latest").await?;
///         }
///         Ok(())
///     }
/// }
/// ```
///
/// They run for `save`, `save_only`, `save_changes`, `create`, `insert`, `delete` and
/// `force_delete`, not for `restore` or bulk `Query::update`/`delete` and
/// `insert_many`, which write many rows in one statement.
pub trait ModelHooks {
    /// Runs before the row is written (`creating` is true for an insert); an error cancels it.
    fn saving(&mut self, _creating: bool) -> Result {
        Ok(())
    }

    /// Runs after the row is written (`created` is true for an insert).
    fn saved(&self, _created: bool) -> impl Future<Output = Result> + Send {
        async { Ok(()) }
    }

    /// Runs before the row is deleted; an error cancels the delete.
    fn deleting(&self) -> Result {
        Ok(())
    }

    /// Runs after the row is deleted.
    fn deleted(&self) -> impl Future<Output = Result> + Send {
        async { Ok(()) }
    }
}

/// Most parameters one statement binds; both databases allow more
/// (SQLite 32,766, PostgreSQL 65,535).
const MAX_BINDS: usize = 30_000;

/// `insert_many` / `upsert`: multi-row INSERTs in chunks under the bind limit.
async fn write_many<M: Model>(
    mut conn: super::Conn<'_>,
    mut models: Vec<M>,
    upsert: Option<(Vec<String>, Vec<String>)>,
) -> Result<u64> {
    let at = now();
    // `i64` keys come from the database; other keys are written, made here
    // (ULID, UUID) when unsaved.
    let with_keys = !<M::Key as super::ModelKey>::AUTO_INCREMENT;
    for model in &mut models {
        model.touch(at, true);
        if with_keys && model.id().is_unsaved() {
            match <M::Key as super::ModelKey>::generate() {
                Some(key) => model.set_id(key),
                None => {
                    return Err(anyhow!(
                        "set the `id` of every new {} row before inserting them",
                        M::TABLE
                    )
                    .into());
                }
            }
        }
    }
    let mut columns: Vec<&str> = M::COLUMNS.iter().copied().filter(|c| *c != "id").collect();
    if with_keys {
        columns.insert(0, "id");
    }
    if columns.is_empty() || models.is_empty() {
        return Ok(0);
    }
    let quoted: Vec<String> = columns.iter().map(|c| quote(c)).collect();
    let row_marks = format!("({})", vec!["?"; columns.len()].join(", "));
    let conflict = upsert.map(|(unique_by, update)| {
        let targets: Vec<String> = unique_by.iter().map(|c| quote(c)).collect();
        let mut sets: Vec<String> = update
            .iter()
            .map(|c| format!("{0} = excluded.{0}", quote(c)))
            .collect();
        if columns.contains(&"updated_at") && !update.iter().any(|c| c == "updated_at") {
            sets.push(format!("{0} = excluded.{0}", quote("updated_at")));
        }
        if sets.is_empty() {
            format!(" ON CONFLICT ({}) DO NOTHING", targets.join(", "))
        } else {
            format!(
                " ON CONFLICT ({}) DO UPDATE SET {}",
                targets.join(", "),
                sets.join(", ")
            )
        }
    });
    let per_statement = (MAX_BINDS / columns.len()).max(1);
    let mut written = 0;
    for chunk in models.chunks(per_statement) {
        let marks = vec![row_marks.as_str(); chunk.len()].join(", ");
        let statement = format!(
            "INSERT INTO {} ({}) VALUES {marks}{}",
            quote(M::TABLE),
            quoted.join(", "),
            conflict.as_deref().unwrap_or_default()
        );
        let values = chunk.iter().flat_map(|model| {
            let key = with_keys.then(|| model.id().to_db_value());
            key.into_iter().chain(model.values())
        });
        written += sql(statement)
            .bind_all(values)
            .execute(conn.reborrow())
            .await?;
    }
    Ok(written)
}
