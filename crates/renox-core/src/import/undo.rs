//! What an undoable import wrote ([`ImportTx`]), kept in `import_batches` /
//! `import_changes`, and putting it back ([`undo`], [`Undo`], [`prune`]).
//!
//! A row closure can run any SQL, so Renox can't see what it wrote. The
//! closure of an undoable import gets an [`ImportTx`]: its `insert`, `update`
//! and `save` write through the model and record the table, the key and the
//! row as it was before. Writes made any other way (raw SQL, `Query::update`)
//! are not recorded, so they are not undone.

use std::collections::HashSet;
use std::ops::{Deref, DerefMut};
use std::time::Duration;

use anyhow::anyhow;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};

use crate::db::{Conn, DbValue, Executor, Model, ModelKey, ToDbValue, Transaction, quote, sql};
use crate::{AppState, Error, Lang, Result, clock};

/// One row an import inserted or updated, as recorded for [`undo`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ImportChange {
    table: String,
    key: DbValue,
    before: Option<Vec<(String, DbValue)>>,
    stamp: Option<String>,
}

impl ImportChange {
    /// The table the row is in.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The row's `id`.
    pub fn key(&self) -> &DbValue {
        &self.key
    }

    /// Whether the import inserted the row (undoing deletes it); else it
    /// updated it (undoing restores [`ImportChange::before`]).
    pub fn is_insert(&self) -> bool {
        self.before.is_none()
    }

    /// The row's columns (but `id`) as they were before an update; `None`
    /// for an insert.
    pub fn before(&self) -> Option<&[(String, DbValue)]> {
        self.before.as_deref()
    }

    /// The key as text, for messages.
    fn key_text(&self) -> String {
        match self.key.to_json() {
            Value::String(text) => text,
            other => other.to_string(),
        }
    }
}

/// The transaction an import's row closure writes in. It derefs to the
/// [`Transaction`] and works wherever one is taken as a query's executor
/// (`Product::create(tx, …)`, `sql(…).execute(tx)`), so closures written for
/// a plain transaction keep compiling.
///
/// In an [`undoable`](super::Import::undoable) import, write through
/// [`insert`](ImportTx::insert), [`update`](ImportTx::update) and
/// [`save`](ImportTx::save): they record what is needed to undo the row.
/// Writes made any other way are not undoable. Without `undoable` (and in a
/// dry run) nothing is recorded and they just write.
pub struct ImportTx<'a> {
    tx: &'a mut Transaction,
    changes: &'a mut Vec<ImportChange>,
    track: bool,
}

impl<'a> ImportTx<'a> {
    pub(super) fn new(
        tx: &'a mut Transaction,
        changes: &'a mut Vec<ImportChange>,
        track: bool,
    ) -> Self {
        Self { tx, changes, track }
    }

    /// Inserts `model` as a new row (`Model::insert`) and records it, so
    /// undoing the import deletes it.
    #[allow(clippy::manual_async_fn)] // the `+ Send` in the signature is the point
    pub fn insert<'m, M: Model>(
        &'m mut self,
        model: &'m mut M,
    ) -> impl Future<Output = Result> + Send + 'm {
        async move {
            model.insert(&mut *self.tx).await?;
            if self.track {
                let key = model.id().to_db_value();
                let stamp = stamp_of(&mut *self.tx, M::TABLE, M::COLUMNS, &key).await?;
                self.changes.push(ImportChange {
                    table: M::TABLE.to_owned(),
                    key,
                    before: None,
                    stamp,
                });
            }
            Ok(())
        }
    }

    /// Saves the changes to an existing row (`Model::save`) and records the
    /// row as it was, so undoing the import restores it. An unsaved model
    /// is an error; a row that doesn't exist is a 404.
    #[allow(clippy::manual_async_fn)] // the `+ Send` in the signature is the point
    pub fn update<'m, M: Model>(
        &'m mut self,
        model: &'m mut M,
    ) -> impl Future<Output = Result> + Send + 'm {
        async move {
            let id = model.id();
            if id.is_unsaved() {
                return Err(anyhow!(
                    "`update` on an unsaved {} row; use `insert` or `save`",
                    M::TABLE
                )
                .into());
            }
            let before = if self.track {
                let existing = M::unscoped()
                    .with_trashed()
                    .where_eq("id", id)
                    .first(&mut *self.tx)
                    .await?
                    .ok_or(Error::NotFound)?;
                Some(
                    M::COLUMNS
                        .iter()
                        .filter(|c| **c != "id")
                        .map(|c| (*c).to_owned())
                        .zip(existing.values())
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            };
            model.save(&mut *self.tx).await?;
            if let Some(before) = before {
                let key = model.id().to_db_value();
                let stamp = stamp_of(&mut *self.tx, M::TABLE, M::COLUMNS, &key).await?;
                self.changes.push(ImportChange {
                    table: M::TABLE.to_owned(),
                    key,
                    before: Some(before),
                    stamp,
                });
            }
            Ok(())
        }
    }

    /// [`insert`](ImportTx::insert) when the model is unsaved (its id is
    /// `0`, an empty ULID…), else [`update`](ImportTx::update).
    #[allow(clippy::manual_async_fn)] // the `+ Send` in the signature is the point
    pub fn save<'m, M: Model>(
        &'m mut self,
        model: &'m mut M,
    ) -> impl Future<Output = Result> + Send + 'm {
        async move {
            if model.id().is_unsaved() {
                self.insert(model).await
            } else {
                self.update(model).await
            }
        }
    }
}

impl Deref for ImportTx<'_> {
    type Target = Transaction;

    fn deref(&self) -> &Transaction {
        self.tx
    }
}

impl DerefMut for ImportTx<'_> {
    fn deref_mut(&mut self) -> &mut Transaction {
        self.tx
    }
}

impl<'c> Executor<'c> for &'c mut ImportTx<'_> {
    fn into_conn(self) -> Conn<'c> {
        Conn::Tx(&mut *self.tx)
    }
}

/// The row's `updated_at` right after a write, as text (the same cast on
/// both databases), or `None` for a model without that column.
async fn stamp_of(
    tx: &mut Transaction,
    table: &str,
    columns: &[&str],
    key: &DbValue,
) -> Result<Option<String>> {
    if !columns.contains(&"updated_at") {
        return Ok(None);
    }
    stamp_text(tx, table, key).await
}

async fn stamp_text(tx: &mut Transaction, table: &str, key: &DbValue) -> Result<Option<String>> {
    let found = sql(format!(
        "SELECT CAST(updated_at AS TEXT) FROM {} WHERE id = ?",
        quote(table)
    ))
    .bind_all([key.clone()])
    .scalar_optional::<Option<String>>(tx)
    .await?;
    Ok(found.flatten())
}

// ---- a value as JSON, with its type, so a before-image restores exactly ----

/// A value as `{"t": type, "v": value}`.
fn encode(value: &DbValue) -> Value {
    match value {
        DbValue::Null => json!({ "t": "null" }),
        DbValue::Integer(v) => json!({ "t": "int", "v": v }),
        DbValue::Real(v) => json!({ "t": "real", "v": v }),
        DbValue::Text(v) => json!({ "t": "text", "v": v }),
        DbValue::Blob(v) => json!({ "t": "blob", "v": BASE64.encode(v) }),
        DbValue::Bool(v) => json!({ "t": "bool", "v": v }),
        DbValue::DateTime(v) => json!({ "t": "datetime", "v": v.to_rfc3339() }),
        DbValue::NaiveDateTime(v) => {
            json!({ "t": "naive", "v": v.format("%Y-%m-%dT%H:%M:%S%.f").to_string() })
        }
        DbValue::Date(v) => json!({ "t": "date", "v": v.to_string() }),
        DbValue::Time(v) => json!({ "t": "time", "v": v.to_string() }),
        DbValue::Json(v) => json!({ "t": "json", "v": v }),
        #[cfg(feature = "uuid")]
        DbValue::Uuid(v) => json!({ "t": "uuid", "v": v.to_string() }),
        // Plaintext must not land in a plain table: such a column is left
        // out of the before-image (and so not restored).
        DbValue::Encrypted(_) => json!({ "t": "skip" }),
    }
}

/// The value `encode` wrote; `None` for a column to leave alone.
fn decode(value: &Value) -> Result<Option<DbValue>> {
    let bad = || Error::from(anyhow!("an import change holds a value that can't be read"));
    let text = || value["v"].as_str().ok_or_else(bad);
    Ok(Some(match value["t"].as_str().ok_or_else(bad)? {
        "skip" => return Ok(None),
        "null" => DbValue::Null,
        "int" => DbValue::Integer(value["v"].as_i64().ok_or_else(bad)?),
        "real" => DbValue::Real(value["v"].as_f64().ok_or_else(bad)?),
        "text" => DbValue::Text(text()?.to_owned()),
        "blob" => DbValue::Blob(BASE64.decode(text()?).map_err(|_| bad())?),
        "bool" => DbValue::Bool(value["v"].as_bool().ok_or_else(bad)?),
        "datetime" => {
            DbValue::DateTime(chrono::DateTime::parse_from_rfc3339(text()?).map_err(|_| bad())?)
        }
        "naive" => DbValue::NaiveDateTime(
            chrono::NaiveDateTime::parse_from_str(text()?, "%Y-%m-%dT%H:%M:%S%.f")
                .map_err(|_| bad())?,
        ),
        "date" => DbValue::Date(text()?.parse().map_err(|_| bad())?),
        "time" => DbValue::Time(text()?.parse().map_err(|_| bad())?),
        "json" => DbValue::Json(value["v"].clone()),
        #[cfg(feature = "uuid")]
        "uuid" => DbValue::Uuid(text()?.parse().map_err(|_| bad())?),
        _ => return Err(bad()),
    }))
}

/// Writes a finished import's batch and changes (in its transaction).
pub(super) async fn record(
    tx: &mut Transaction,
    user_id: Option<i64>,
    window: Duration,
    changes: &[ImportChange],
) -> Result<i64> {
    let now = clock::unix_secs();
    let expires = now.saturating_add(i64::try_from(window.as_secs()).unwrap_or(i64::MAX));
    let batch: i64 = sql(
        "INSERT INTO import_batches (user_id, rows_written, created_at, expires_at) \
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(changes.len() as i64)
    .bind(now)
    .bind(expires)
    .scalar(&mut *tx)
    .await?;
    for change in changes {
        let before = change.before.as_ref().map(|columns| {
            Value::Array(
                columns
                    .iter()
                    .map(|(column, value)| json!([column, encode(value)]))
                    .collect(),
            )
            .to_string()
        });
        sql(
            "INSERT INTO import_changes (batch_id, table_name, row_key, before_image, stamp) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(batch)
        .bind(change.table.as_str())
        .bind(encode(&change.key).to_string())
        .bind(before)
        .bind(change.stamp.clone())
        .execute(&mut *tx)
        .await?;
    }
    Ok(batch)
}

/// What [`undo`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct UndoReport {
    /// Rows deleted (the import had inserted them).
    pub deleted: usize,
    /// Rows put back as they were (the import had updated them).
    pub restored: usize,
}

type Veto =
    Box<dyn for<'t> FnMut(&'t mut Transaction, &'t ImportChange) -> BoxFuture<'t, Result> + Send>;

/// Undoes an import made with [`Import::undoable`](super::Import::undoable);
/// [`undo`] is the short form. All in one transaction: either every row is
/// put back or nothing changes.
///
/// ```
/// # use renox::prelude::*;
/// use renox::import::Undo;
/// # async fn demo(state: AppState, batch_id: i64) -> Result {
/// Undo::batch(batch_id)
///     .can_undo(|tx, change| {
///         Box::pin(async move {
///             if change.table() == "items" && change.is_insert() {
///                 let sold: i64 = renox::db::sql("SELECT COUNT(*) FROM invoice_lines WHERE item_id = ?")
///                     .bind(match change.key() { renox::db::DbValue::Integer(id) => *id, _ => 0 })
///                     .scalar(tx)
///                     .await?;
///                 if sold > 0 {
///                     return Err(Error::BadRequest("An imported item is on an invoice.".into()));
///                 }
///             }
///             Ok(())
///         })
///     })
///     .run(&state)
///     .await?;
/// # Ok(()) }
/// ```
#[must_use = "an undo does nothing until it is `run`"]
pub struct Undo {
    batch: i64,
    lang: Option<Lang>,
    veto: Option<Veto>,
}

impl Undo {
    /// Undoes the batch an import reported as [`ImportReport::batch_id`](super::ImportReport::batch_id).
    pub fn batch(batch_id: i64) -> Self {
        Self {
            batch: batch_id,
            lang: None,
            veto: None,
        }
    }

    /// Messages in this language; the app's `APP_LOCALE` otherwise.
    pub fn lang(mut self, lang: &Lang) -> Self {
        self.lang = Some(lang.clone());
        self
    }

    /// Asks `check` about each row before it is touched, in the undo's
    /// transaction. Return an error to refuse the whole undo for an app
    /// rule ("used by an issued document"): an `Error::BadRequest`'s text
    /// reaches the person undoing.
    pub fn can_undo<F>(mut self, check: F) -> Self
    where
        F: for<'t> FnMut(&'t mut Transaction, &'t ImportChange) -> BoxFuture<'t, Result>
            + Send
            + 'static,
    {
        self.veto = Some(Box::new(check));
        self
    }

    /// Undoes the batch, or refuses with an `Error::BadRequest` saying why:
    /// the batch is unknown, was undone already, its window has ended, a row
    /// was deleted or changed since (the row's `updated_at` is not what the
    /// import left), another table's rows still refer to a row to delete
    /// (named), or `can_undo` said no. A refusal changes nothing.
    pub fn run<'a>(
        self,
        state: &'a AppState,
    ) -> impl Future<Output = Result<UndoReport>> + Send + 'a {
        let lang = self
            .lang
            .clone()
            .unwrap_or_else(|| state.lang(&state.config.locale));
        let batch = self.batch;
        let mut veto = self.veto;
        async move {
            let refuse = |key: &str, params: &[(&str, &str)]| {
                let params: Vec<(&str, &dyn std::fmt::Display)> = params
                    .iter()
                    .map(|(name, value)| (*name, value as &dyn std::fmt::Display))
                    .collect();
                Error::BadRequest(lang.t(key, &params))
            };
            let row = sql("SELECT expires_at, undone_at FROM import_batches WHERE id = ?")
                .bind(batch)
                .fetch_optional(&state.db)
                .await?;
            let Some(row) = row else {
                return Err(refuse("ui.import.undo.unknown", &[]));
            };
            if row.try_get::<Option<i64>>("undone_at")?.is_some() {
                return Err(refuse("ui.import.undo.done", &[]));
            }
            if row.try_get::<i64>("expires_at")? < clock::unix_secs() {
                return Err(refuse("ui.import.undo.expired", &[]));
            }
            let rows = sql(
                "SELECT table_name, row_key, before_image, stamp FROM import_changes \
                 WHERE batch_id = ? ORDER BY id DESC",
            )
            .bind(batch)
            .fetch_all(&state.db)
            .await?;
            let mut changes = Vec::with_capacity(rows.len());
            for row in &rows {
                let key = decode(
                    &serde_json::from_str(&row.try_get::<String>("row_key")?)
                        .map_err(anyhow::Error::from)?,
                )?
                .ok_or_else(|| Error::from(anyhow!("an import change has no key")))?;
                let before = match row.try_get::<Option<String>>("before_image")? {
                    None => None,
                    Some(text) => {
                        let list: Vec<(String, Value)> =
                            serde_json::from_str(&text).map_err(anyhow::Error::from)?;
                        let mut columns = Vec::new();
                        for (column, value) in list {
                            if let Some(value) = decode(&value)? {
                                columns.push((column, value));
                            }
                        }
                        Some(columns)
                    }
                };
                changes.push(ImportChange {
                    table: row.try_get("table_name")?,
                    key,
                    before,
                    stamp: row.try_get("stamp")?,
                });
            }

            let mut tx = state.db.begin().await?;
            let mut report = UndoReport {
                deleted: 0,
                restored: 0,
            };
            // Newest change first. A row written twice by the batch is
            // checked once, against its last write.
            let mut checked = HashSet::new();
            for change in &changes {
                let target = (change.table.clone(), encode(&change.key).to_string());
                if checked.insert(target)
                    && let Some(stamp) = &change.stamp
                {
                    let key = change.key_text();
                    let params = [("table", change.table.as_str()), ("key", key.as_str())];
                    match stamp_text(&mut tx, &change.table, &change.key).await? {
                        None => {
                            return Err(refuse("ui.import.undo.deleted", &params));
                        }
                        Some(now) if now != *stamp => {
                            return Err(refuse("ui.import.undo.changed", &params));
                        }
                        Some(_) => {}
                    }
                }
                if let Some(veto) = veto.as_mut() {
                    veto(&mut tx, change).await?;
                }
                match &change.before {
                    None => {
                        let deleted =
                            sql(format!("DELETE FROM {} WHERE id = ?", quote(&change.table)))
                                .bind_all([change.key.clone()])
                                .execute(&mut tx)
                                .await;
                        match deleted {
                            Ok(count) => report.deleted += count as usize,
                            Err(err) if err.is_foreign_key_violation() => {
                                tx.rollback().await?;
                                let by = referenced_by(state, &change.table, &change.key).await?;
                                return Err(refuse(
                                    "ui.import.undo.in_use",
                                    &[
                                        ("table", change.table.as_str()),
                                        ("key", change.key_text().as_str()),
                                        ("by", by.as_str()),
                                    ],
                                ));
                            }
                            Err(err) => return Err(err.into()),
                        }
                    }
                    Some(columns) => {
                        if columns.is_empty() {
                            continue;
                        }
                        let sets: Vec<String> = columns
                            .iter()
                            .map(|(c, _)| format!("{} = ?", quote(c)))
                            .collect();
                        let changed = sql(format!(
                            "UPDATE {} SET {} WHERE id = ?",
                            quote(&change.table),
                            sets.join(", ")
                        ))
                        .bind_all(columns.iter().map(|(_, v)| v.clone()))
                        .bind_all([change.key.clone()])
                        .execute(&mut tx)
                        .await?;
                        report.restored += changed as usize;
                    }
                }
            }
            sql("UPDATE import_batches SET undone_at = ? WHERE id = ?")
                .bind(clock::unix_secs())
                .bind(batch)
                .execute(&mut tx)
                .await?;
            tx.commit().await?;
            Ok(report)
        }
    }
}

/// Which table still refers to the row (the foreign key that stopped its
/// delete), for the refusal's message; every table with a key to `table`
/// when none holds a row for it.
async fn referenced_by(state: &AppState, table: &str, key: &DbValue) -> Result<String> {
    let keys = crate::db::schema_check::foreign_keys_to(&state.db, table).await?;
    let mut holding = Vec::new();
    for fk in &keys {
        let found = sql(format!(
            "SELECT 1 FROM {} WHERE {} = ? LIMIT 1",
            quote(&fk.table),
            quote(&fk.column)
        ))
        .bind_all([key.clone()])
        .fetch_optional(&state.db)
        .await?;
        if found.is_some() && !holding.contains(&fk.table) {
            holding.push(fk.table.clone());
        }
    }
    if holding.is_empty() {
        holding = keys.iter().map(|fk| fk.table.clone()).collect();
        holding.dedup();
    }
    Ok(holding.join(", "))
}

/// Undoes the import whose report said `batch_id`; see [`Undo`] for what
/// refuses it. Deletes the rows the import inserted and restores the rows it
/// updated, all in one transaction.
///
/// ```
/// # use renox::prelude::*;
/// # async fn demo(state: AppState, batch_id: i64) -> Result {
/// let undone = renox::import::undo(&state, batch_id).await?;
/// println!("{} deleted, {} restored", undone.deleted, undone.restored);
/// # Ok(()) }
/// ```
pub fn undo(
    state: &AppState,
    batch_id: i64,
) -> impl Future<Output = Result<UndoReport>> + Send + '_ {
    Undo::batch(batch_id).run(state)
}

/// Deletes the records of imports whose undo window has ended (and so can
/// no longer be undone), `age` after it ended; returns the batches deleted.
/// `my-app import:prune` runs it.
pub async fn prune(state: &AppState, age: Duration) -> Result<u64> {
    let before =
        clock::unix_secs().saturating_sub(i64::try_from(age.as_secs()).unwrap_or(i64::MAX));
    let mut tx = state.db.begin().await?;
    sql("DELETE FROM import_changes WHERE batch_id IN \
         (SELECT id FROM import_batches WHERE expires_at < ?)")
    .bind(before)
    .execute(&mut tx)
    .await?;
    let batches = sql("DELETE FROM import_batches WHERE expires_at < ?")
        .bind(before)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A before-image restores the type it had, not just its text.
    #[test]
    fn values_round_trip_with_their_type() {
        let values = [
            DbValue::Null,
            DbValue::Integer(-4),
            DbValue::Real(1.5),
            DbValue::Text("a \"b\"".into()),
            DbValue::Blob(vec![0, 255, 7]),
            DbValue::Bool(true),
            DbValue::DateTime(
                chrono::DateTime::parse_from_rfc3339("2026-01-02T03:04:05.123456+02:00").unwrap(),
            ),
            DbValue::NaiveDateTime(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 2)
                    .unwrap()
                    .and_hms_micro_opt(3, 4, 5, 6)
                    .unwrap(),
            ),
            DbValue::Date(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()),
            DbValue::Time(chrono::NaiveTime::from_hms_opt(3, 4, 5).unwrap()),
            DbValue::Json(json!({ "a": [1, 2] })),
        ];
        for value in values {
            assert_eq!(decode(&encode(&value)).unwrap(), Some(value));
        }
        assert_eq!(decode(&json!({ "t": "skip" })).unwrap(), None);
        assert!(decode(&json!({ "t": "int", "v": "x" })).is_err());
        assert!(decode(&json!({ "t": "other" })).is_err());
    }
}
