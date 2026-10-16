//! Imports: the rows of a CSV or Excel file, each checked with a form's
//! rules, then written in one transaction (Filament's import action).
//!
//! A row is read like a form: its columns, named by the file's first line,
//! fill a struct that derives `Deserialize` and `Validate`, so the same
//! rules (and messages, in the user's language) check a row as check the
//! form that adds one record. Rows that fail are reported with their row
//! number; the others are written by your closure, each in a savepoint of
//! one transaction, so a row the database refuses undoes only its own
//! writes.
//!
//! ```
//! # use renox::prelude::*;
//! use renox::import::{Import, ImportReport};
//! # #[derive(Model, serde::Serialize, Default)]
//! # #[model(table = "products")]
//! # struct Product { id: i64, sku: String, name: String, price: i64 }
//!
//! /// One line of `sku,name,price`.
//! #[derive(serde::Deserialize, Validate)]
//! struct ProductRow {
//!     #[validate(required, max = 30, alpha_dash)]
//!     sku: String,
//!     #[validate(required, max = 100)]
//!     name: String,
//!     #[validate(required, min = 0)]
//!     price: i64,
//! }
//!
//! #[derive(serde::Deserialize, Validate)]
//! struct ImportForm {
//!     #[validate(required, mimes(&["csv", "txt"]))]
//!     file: Option<Upload>,
//! }
//!
//! async fn import(
//!     State(state): State<AppState>,
//!     lang: Lang,
//!     Valid(form): Valid<ImportForm>,
//! ) -> Result<ImportReport> {
//!     let file = form.file.ok_or(Error::NotFound)?;
//!     Import::csv(file.bytes())
//!         .lang(&lang)
//!         .run(&state, |tx, row: ProductRow| {
//!             Box::pin(async move {
//!                 let product = Product { sku: row.sku, name: row.name, price: row.price, ..Default::default() };
//!                 Product::create(tx, product).await?;
//!                 Ok(())
//!             })
//!         })
//!         .await
//! }
//! ```
//!
//! The handler answers with the [`ImportReport`]: a toast when every row
//! was imported, else a table of the rows that weren't, which the kit's
//! `import_action` sheet shows. A file that can't be read at all (not
//! UTF-8, not a workbook, no rows, too many rows) is a validation error on
//! the form's `file` field.
//!
//! # Excel files
//!
//! With the `xlsx` feature, `Import::xlsx` reads a workbook the way
//! [`Import::csv`] reads a file: the first row of the sheet names the
//! columns, the rows below it are the records. Every cell is read as the
//! text a CSV file would hold (`12`, not `12.0`; dates as `2026-01-31`), so
//! the same row struct and rules check both. [`Import::sheet`] picks a sheet
//! by name (the first one by default), and [`Import::from_upload`] picks CSV
//! or Excel from the file's content. `template_xlsx` makes the matching
//! empty workbook to download. Builds without the feature keep CSV imports;
//! an Excel file is then a validation error asking for CSV.
//!
//! # Checking first, and undoing
//!
//! [`Import::dry_run`] reads, validates and writes every row (your closure
//! runs, in a transaction) and then rolls everything back, so the report
//! shows what a real run would do and the database is unchanged. (A
//! PostgreSQL sequence still advances.)
//!
//! [`Import::undoable`] keeps what the import wrote for a time, so
//! [`undo`] can put it back. Write through the closure's [`ImportTx`]
//! (`insert`, `update`, `save`) so Renox sees the rows; SQL run any other
//! way is not undone:
//!
//! ```
//! # use renox::prelude::*;
//! use std::time::Duration;
//! use renox::import::{Import, ImportReport};
//! # #[derive(Model, serde::Serialize, Default)]
//! # #[model(table = "products")]
//! # struct Product { id: i64, sku: String, name: String, updated_at: Option<DateTime> }
//! # #[derive(serde::Deserialize, Validate)]
//! # struct ProductRow { #[validate(required)] sku: String, #[validate(required)] name: String }
//! # async fn import(state: AppState, bytes: Vec<u8>) -> Result<ImportReport> {
//! let report = Import::csv(bytes)
//!     .undoable(Duration::from_secs(86_400))
//!     .run(&state, |tx, row: ProductRow| {
//!         Box::pin(async move {
//!             let mut product = Product { sku: row.sku, name: row.name, ..Default::default() };
//!             tx.insert(&mut product).await?; // recorded
//!             Ok(())
//!         })
//!     })
//!     .await?;
//! if let Some(batch) = report.batch_id {
//!     // Later, within a day: deletes the inserted rows, restores the updated ones.
//!     renox::import::undo(&state, batch).await?;
//! }
//! # Ok(report) }
//! ```
//!
//! An undo refuses, with a message, when a row it would touch was deleted
//! or changed since (its `updated_at` is not the one the import left; a
//! model without `updated_at` is undone without that check), when the
//! window has ended, when other tables still refer to a row to delete, or
//! when [`Undo::can_undo`] says no. It runs in one transaction. `my-app
//! import:prune` deletes the records of finished windows.
//!
//! # Error files
//!
//! [`ImportReport::error_csv`] (and `error_xlsx` with the feature) give the
//! file back for people to fix: the rows that failed, or the whole file,
//! each with an added error column. The kit's report links both as
//! downloads.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use futures_util::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::auth::User;
use crate::db::sql;
use crate::validation::extract::{Messages, Parsed, parse_pairs};
use crate::validation::{Errors, FormContext, Validate, ValidationError, Validator};
use crate::{AppState, Error, Lang, Result};

mod undo;
#[cfg(feature = "xlsx")]
mod xlsx;

/// The framework migration creating `import_batches` and `import_changes`
/// (every app gets them).
pub(crate) const MIGRATION: crate::db::Migration =
    crate::db::framework_migration!("import", "00010101000230_create_import_batches_table");

pub use undo::{ImportChange, ImportTx, Undo, UndoReport, prune, undo};
#[cfg(feature = "xlsx")]
pub use xlsx::TemplateColumn;

/// The most rows one import reads, unless [`Import::max_rows`] says otherwise.
pub const MAX_IMPORT_ROWS: usize = 10_000;

/// What writes one row (`run`'s closure), boxed.
type WriteRow<'a, T> =
    Box<dyn for<'t, 'x> FnMut(&'t mut ImportTx<'x>, T) -> BoxFuture<'t, Result> + Send + 'a>;

/// A data row: its row number and its cells.
type Record = (usize, Vec<String>);

/// The most failed rows the report's table lists; the rest are counted.
const LISTED_FAILURES: usize = 100;

/// The largest error file the report embeds as a download link.
const MAX_LINKED_ERROR_FILE: usize = 1 << 20;

/// Where an import's rows come from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Csv,
    Excel,
}

/// A CSV or Excel file to import; see the [module docs](self).
#[must_use = "an import does nothing until it is `run`"]
pub struct Import {
    data: Vec<u8>,
    source: Source,
    sheet: Option<String>,
    field: String,
    delimiter: char,
    headers: Option<Vec<String>>,
    renames: Vec<(String, String)>,
    max_rows: usize,
    all_or_nothing: bool,
    dry_run: bool,
    undo_window: Option<Duration>,
    lang: Option<Lang>,
    user: Option<User>,
}

/// A file read into rows: the columns' headings as the file has them, the
/// fields they fill and the data rows.
struct Table {
    headings: Vec<String>,
    fields: Vec<String>,
    rows: Vec<Record>,
}

impl Import {
    fn from_source(data: &[u8], source: Source) -> Self {
        Self {
            data: data.to_vec(),
            source,
            sheet: None,
            field: "file".into(),
            delimiter: ',',
            headers: None,
            renames: Vec::new(),
            max_rows: MAX_IMPORT_ROWS,
            all_or_nothing: false,
            dry_run: false,
            undo_window: None,
            lang: None,
            user: None,
        }
    }

    /// The bytes of a CSV file (an upload's, a file's): UTF-8, with or
    /// without a byte order mark, its first line naming the columns.
    pub fn csv(data: impl AsRef<[u8]>) -> Self {
        Self::from_source(data.as_ref(), Source::Csv)
    }

    /// The bytes of an Excel workbook (`.xlsx`): the first row of the
    /// sheet names the columns. Cells are read as the text a CSV file
    /// would hold; see the [module docs](self). A file that isn't a
    /// workbook is reported by `run` as an error on the form's field.
    #[cfg(feature = "xlsx")]
    pub fn xlsx(data: impl AsRef<[u8]>) -> Self {
        Self::from_source(data.as_ref(), Source::Excel)
    }

    /// An upload, read as Excel when its content is a workbook (a zip
    /// archive) and as CSV otherwise, whatever its name says.
    pub fn from_upload(upload: &crate::Upload) -> Self {
        let bytes = upload.bytes();
        let source = if bytes.starts_with(b"PK\x03\x04") {
            Source::Excel
        } else {
            Source::Csv
        };
        Self::from_source(bytes, source)
    }

    /// Reads the sheet called `name` of a workbook (the first by default);
    /// a workbook without it is reported as an error on the form's field.
    /// Ignored for CSV.
    pub fn sheet(mut self, name: &str) -> Self {
        self.sheet = Some(name.to_owned());
        self
    }

    /// Cells are separated by `delimiter` (`;` in files from a spreadsheet
    /// set to a European language) instead of `,`.
    pub fn delimiter(mut self, delimiter: char) -> Self {
        self.delimiter = delimiter;
        self
    }

    /// The file has no line of column names: these are its columns, in order.
    pub fn headers(mut self, names: &[&str]) -> Self {
        self.headers = Some(names.iter().map(|n| (*n).to_owned()).collect());
        self
    }

    /// Reads the column headed `heading` as the field `field`, e.g.
    /// `rename("Description", "name")`. Without one, a heading names its
    /// field in lowercase with `_` for spaces and dashes: "Unit price" is
    /// `unit_price`.
    pub fn rename(mut self, heading: &str, field: &str) -> Self {
        self.renames.push((field_name(heading), field.to_owned()));
        self
    }

    /// At most `rows` rows (default [`MAX_IMPORT_ROWS`]); a longer file is
    /// refused before anything is written.
    pub fn max_rows(mut self, rows: usize) -> Self {
        self.max_rows = rows;
        self
    }

    /// Writes nothing unless every row is valid and saved: one bad row and
    /// the whole file is refused (the report lists why). By default the
    /// good rows are written and the bad ones reported.
    pub fn all_or_nothing(mut self) -> Self {
        self.all_or_nothing = true;
        self
    }

    /// Checks the file completely and writes nothing. Every row is read and
    /// validated and your closure runs for the valid ones, in the
    /// transaction, which is then rolled back: the report lists the same
    /// errors a real run would (a duplicate inside the file, a row the
    /// database refuses) and [`ImportReport::imported`] counts the rows that
    /// would be written. Nothing is recorded for undo.
    pub fn dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    /// [`dry_run`](Import::dry_run) when `check` is true: for the kit's
    /// "check first" box (`Import::from_upload(&file).dry_run_if(form.dry_run)`).
    pub fn dry_run_if(mut self, check: bool) -> Self {
        self.dry_run |= check;
        self
    }

    /// Keeps what the import wrote for `window`, so [`undo`] can put it back
    /// (the report's [`batch_id`](ImportReport::batch_id) names it). Needs
    /// the closure to write through its [`ImportTx`]; see the
    /// [module docs](self).
    pub fn undoable(mut self, window: Duration) -> Self {
        self.undo_window = Some(window);
        self
    }

    /// Messages (and the report) in this language; the app's
    /// `APP_LOCALE` otherwise.
    pub fn lang(mut self, lang: &Lang) -> Self {
        self.lang = Some(lang.clone());
        self
    }

    /// The user importing, whom the rows' `after` hooks see as
    /// `form.user` (and the `current_password` rule checks).
    pub fn user(mut self, user: &User) -> Self {
        self.user = Some(user.clone());
        self
    }

    /// The form field the file came in (`file` by default): a file that
    /// can't be read is reported as that field's error.
    pub fn field(mut self, name: &str) -> Self {
        self.field = name.to_owned();
        self
    }

    /// Reads every row into a `T` and checks it with `T`'s rules (its
    /// `prepare` and `after` hooks too), then calls `write` with each valid
    /// row inside one transaction, a savepoint per row. An error from
    /// `write` is that row's failure: a [`ValidationError`] shows its
    /// messages, `Error::BadRequest` its text, a unique-constraint
    /// violation says the row repeats a unique value.
    ///
    /// The closure's first argument is an [`ImportTx`], which stands in for
    /// the transaction (pass it where a transaction or `&db` goes) and
    /// records writes made through it when the import is
    /// [`undoable`](Import::undoable).
    ///
    /// `T`'s `authorize` hook isn't asked: authorize the import's route.
    pub fn run<'a, T, F>(
        self,
        state: &'a AppState,
        write: F,
    ) -> impl Future<Output = Result<ImportReport>> + Send + 'a
    where
        T: DeserializeOwned + Validate + Send + Sync + 'a,
        F: for<'t, 'x> FnMut(&'t mut ImportTx<'x>, T) -> BoxFuture<'t, Result> + Send + 'a,
    {
        let lang = self
            .lang
            .clone()
            .unwrap_or_else(|| state.lang(&state.config.locale));
        // Boxed: a generic closure held across `.await`s keeps handler
        // futures from being `Send` (rustc #100013).
        let mut write: WriteRow<'a, T> = Box::new(write);
        async move {
            let table = self.table(&lang)?;
            let mut report = ImportReport {
                imported: 0,
                failed: Vec::new(),
                all_or_nothing: self.all_or_nothing,
                dry_run: self.dry_run,
                batch_id: None,
                headings: table.headings,
                rows: Vec::new(),
                lang: lang.clone(),
            };
            let messages = Messages {
                texts: lang.texts(),
            };
            let mut valid = Vec::new();
            for (row, cells) in table.rows {
                let pairs = table
                    .fields
                    .iter()
                    .zip(&cells)
                    .filter(|(field, _)| !field.is_empty())
                    .map(|(field, cell)| (field.clone(), cell.clone()))
                    .collect();
                report.rows.push((row, cells));
                match check::<T>(state, self.user.as_ref(), &messages, pairs).await? {
                    Ok(data) => valid.push((row, data)),
                    Err(errors) => report.failed.push(FailedRow { row, errors }),
                }
            }
            if self.all_or_nothing && !report.failed.is_empty() {
                return Ok(report);
            }
            let track = self.undo_window.is_some() && !self.dry_run;
            let mut tx = state.db.begin().await?;
            let mut changes = Vec::new();
            let mut imported = 0;
            for (row, data) in valid {
                // Our own savepoint (not `Transaction::savepoint`, whose
                // closure can't borrow `write`); the row's closure may open
                // its own, which are named differently.
                sql("SAVEPOINT renox_import_row").execute(&mut tx).await?;
                let mut row_changes = Vec::new();
                let outcome = {
                    let mut itx = ImportTx::new(&mut tx, &mut row_changes, track);
                    write(&mut itx, data).await
                };
                if outcome.is_err() {
                    sql("ROLLBACK TO SAVEPOINT renox_import_row")
                        .execute(&mut tx)
                        .await?;
                }
                sql("RELEASE SAVEPOINT renox_import_row")
                    .execute(&mut tx)
                    .await?;
                match outcome {
                    Ok(()) => {
                        imported += 1;
                        changes.append(&mut row_changes);
                    }
                    Err(err) => {
                        report.failed.push(FailedRow {
                            row,
                            errors: row_errors(err, &lang)?,
                        });
                        if self.all_or_nothing {
                            // Dropping the transaction rolls it back.
                            return Ok(report);
                        }
                    }
                }
            }
            if self.dry_run {
                tx.rollback().await?;
            } else {
                if let Some(window) = self.undo_window
                    && !changes.is_empty()
                {
                    let user = self.user.as_ref().map(|u| u.id);
                    report.batch_id = Some(undo::record(&mut tx, user, window, &changes).await?);
                }
                tx.commit().await?;
            }
            report.imported = imported;
            report.failed.sort_by_key(|f| f.row);
            Ok(report)
        }
    }

    /// An error on the form's field.
    fn refuse(&self, lang: &Lang, key: &str, params: &[(&str, &dyn fmt::Display)]) -> Error {
        let mut errors = Errors::new();
        errors.add(self.field.clone(), lang.t(key, params));
        ValidationError::new(errors).into()
    }

    /// The file's rows: (row number minus one, cells), CSV or Excel.
    fn raw_rows(&self, lang: &Lang) -> Result<Vec<(usize, Vec<String>)>> {
        match self.source {
            Source::Csv => {
                let Ok(text) = std::str::from_utf8(&self.data) else {
                    return Err(self.refuse(lang, "ui.import.not_utf8", &[]));
                };
                Ok(parse_csv(
                    text.trim_start_matches('\u{feff}'),
                    self.delimiter,
                ))
            }
            #[cfg(feature = "xlsx")]
            Source::Excel => {
                xlsx::read(&self.data, self.sheet.as_deref()).map_err(|problem| match problem {
                    xlsx::Problem::Unreadable => self.refuse(lang, "ui.import.not_xlsx", &[]),
                    xlsx::Problem::NoSheet(name) => {
                        self.refuse(lang, "ui.import.no_sheet", &[("sheet", &name)])
                    }
                })
            }
            #[cfg(not(feature = "xlsx"))]
            Source::Excel => Err(self.refuse(lang, "ui.import.xlsx_unsupported", &[])),
        }
    }

    /// The file as a table, or the file's error on the form's field.
    fn table(&self, lang: &Lang) -> Result<Table> {
        let mut rows = self.raw_rows(lang)?;
        // A row's index from 0 is its spreadsheet row number minus one. A
        // sheet may start below row 1: its first row still names the columns.
        let headings = match &self.headers {
            Some(headers) => headers.clone(),
            None if rows.is_empty() => return Err(self.refuse(lang, "ui.import.empty", &[])),
            None => rows.remove(0).1,
        };
        let fields: Vec<String> = headings
            .iter()
            .map(|h| {
                let key = field_name(h);
                self.renames
                    .iter()
                    .find(|(from, _)| *from == key)
                    .map_or(key, |(_, to)| to.clone())
            })
            .collect();
        let rows: Vec<Record> = rows
            .into_iter()
            .filter(|(_, cells)| cells.iter().any(|c| !c.trim().is_empty()))
            .map(|(index, cells)| (index + 1, cells))
            .collect();
        if rows.is_empty() {
            return Err(self.refuse(lang, "ui.import.empty", &[]));
        }
        if rows.len() > self.max_rows {
            return Err(self.refuse(
                lang,
                "ui.import.too_many",
                &[("count", &self.max_rows as &dyn fmt::Display)],
            ));
        }
        Ok(Table {
            headings,
            fields,
            rows,
        })
    }
}

/// A heading as a field's name: "Unit price " → `unit_price`.
fn field_name(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .split([' ', '-'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

/// A row read and checked as a form is: `Ok` with the data, or its errors.
async fn check<T: DeserializeOwned + Validate + Send + Sync>(
    state: &AppState,
    user: Option<&User>,
    messages: &Messages,
    pairs: Vec<(String, String)>,
) -> Result<std::result::Result<T, Errors>> {
    let (parsed, _) = parse_pairs::<T>(pairs, &HashMap::new(), messages);
    let (mut data, mut errors) = match parsed {
        Parsed::Ok(data, errors) => (data, errors),
        Parsed::Invalid(errors) => return Ok(Err(errors)),
    };
    data.prepare();
    let rule_errors = Validator::rules_with_texts(&data, messages.texts.clone())
        .finish_for(state, user)
        .await?;
    for (field, list) in rule_errors.iter() {
        if !errors.has(field) {
            for message in list {
                errors.add(field, message.clone());
            }
        }
    }
    if errors.is_empty() {
        let form = FormContext {
            state,
            user,
            method: &axum::http::Method::POST,
            path: "",
        };
        data.after(&form, &mut errors).await?;
    }
    Ok(if errors.is_empty() {
        Ok(data)
    } else {
        Err(errors)
    })
}

/// Why `write` refused a row, as messages for the report.
fn row_errors(err: Error, lang: &Lang) -> Result<Errors> {
    let mut errors = Errors::new();
    match err {
        Error::Validation(invalid) => return Ok(invalid.errors),
        Error::BadRequest(message) | Error::Status(_, message) => errors.add("row", message),
        err if err.is_unique_violation() => errors.add("row", lang.t("ui.import.duplicate", &[])),
        Error::Internal(err) => {
            tracing::warn!(?err, "an imported row could not be saved");
            errors.add("row", lang.t("ui.import.not_saved", &[]));
        }
        // Not a row's fault (403, 404…): the import stops.
        other => return Err(other),
    }
    Ok(errors)
}

/// What an import did: the rows written and the rows that weren't, with
/// why. As a handler's answer it is a success toast (and `HX-Refresh`)
/// when every row was imported, else the `renox/import_report.html`
/// fragment listing the failed rows, which keeps the kit's import sheet
/// open. The fragment also links the error files (see
/// [`ImportReport::error_csv`]). The report of a [dry run](Import::dry_run)
/// is always the fragment, so the sheet stays open to import for real.
#[non_exhaustive]
pub struct ImportReport {
    /// Rows written (for a dry run, rows that would be).
    pub imported: usize,
    /// Rows left out, by row number.
    pub failed: Vec<FailedRow>,
    /// Whether the import was a [dry run](Import::dry_run): nothing was
    /// written.
    pub dry_run: bool,
    /// The batch to give [`undo`], when the import was
    /// [`undoable`](Import::undoable) and wrote rows through its
    /// [`ImportTx`].
    pub batch_id: Option<i64>,
    all_or_nothing: bool,
    /// The file's headings, and its data rows (row number, cells), for the
    /// error files.
    headings: Vec<String>,
    rows: Vec<Record>,
    lang: Lang,
}

impl fmt::Debug for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportReport")
            .field("imported", &self.imported)
            .field("failed", &self.failed)
            .field("all_or_nothing", &self.all_or_nothing)
            .field("dry_run", &self.dry_run)
            .field("batch_id", &self.batch_id)
            .finish()
    }
}

/// Which rows an error file holds; see [`ImportReport::error_csv`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorFile {
    /// Only the rows that failed (for large files).
    FailedRows,
    /// Every row of the file, the error column empty for the good ones, so
    /// people fix the file in place and upload it again.
    WholeFile,
}

impl ImportReport {
    /// Every row was imported.
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty()
    }

    /// A one-line summary: "3 rows imported.", "2 rows imported, 1 left
    /// out.", or "Nothing was imported: 1 row has errors."; for a dry run
    /// "3 rows can be imported." and so on.
    pub fn summary(&self) -> String {
        let failed = self.failed.len() as i64;
        let prefix = if self.dry_run {
            "ui.import.check"
        } else {
            "ui.import"
        };
        let (done, partial, nothing) = (
            format!("{prefix}.done"),
            format!("{prefix}.partial"),
            format!("{prefix}.nothing"),
        );
        if self.failed.is_empty() {
            self.lang.choice(&done, self.imported as i64, &[])
        } else if self.imported == 0 {
            self.lang.choice(&nothing, failed, &[])
        } else {
            self.lang.choice(
                &partial,
                failed,
                &[("imported", &self.imported as &dyn fmt::Display)],
            )
        }
    }

    /// The file's headings and rows with an error column added, as text:
    /// the failed rows, or every row with the column empty for the good ones.
    fn error_table(&self, which: ErrorFile) -> (Vec<String>, Vec<Vec<String>>) {
        let mut headings = self.headings.clone();
        let width = headings.len();
        headings.push(self.lang.t("ui.import.error_column", &[]));
        let messages: HashMap<usize, String> = self
            .failed
            .iter()
            .map(|f| (f.row, f.messages().join(" ")))
            .collect();
        let rows = self
            .rows
            .iter()
            .filter(|(row, _)| which == ErrorFile::WholeFile || messages.contains_key(row))
            .map(|(row, cells)| {
                let mut cells = cells.clone();
                cells.resize(width, String::new());
                cells.push(messages.get(row).cloned().unwrap_or_default());
                cells
            })
            .collect();
        (headings, rows)
    }

    /// The error file as CSV (UTF-8 with a byte order mark, so Excel opens
    /// it right): the file's own columns and an added error column, for
    /// the rows `which` says. Serve it from a route (an app keeps the report
    /// or runs the import again), or let the kit's report link it.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::import::{ErrorFile, ImportReport};
    /// # async fn demo(report: ImportReport) -> renox::Download {
    /// report.error_csv("items-errors.csv", ErrorFile::WholeFile)
    /// # }
    /// ```
    pub fn error_csv(&self, filename: &str, which: ErrorFile) -> crate::Download {
        crate::Download::bytes(
            filename.to_owned(),
            "text/csv; charset=utf-8",
            self.error_csv_bytes(which),
        )
    }

    fn error_csv_bytes(&self, which: ErrorFile) -> Vec<u8> {
        let (headings, rows) = self.error_table(which);
        let mut out = String::from('\u{feff}');
        for line in std::iter::once(&headings).chain(&rows) {
            let cells: Vec<String> = line.iter().map(|c| quote_cell(c)).collect();
            out.push_str(&cells.join(","));
            out.push_str("\r\n");
        }
        out.into_bytes()
    }

    /// The error file as an Excel workbook (every cell text): like
    /// [`error_csv`](ImportReport::error_csv).
    #[cfg(feature = "xlsx")]
    pub fn error_xlsx(&self, filename: &str, which: ErrorFile) -> Result<crate::Download> {
        let (headings, rows) = self.error_table(which);
        Ok(xlsx::download(filename, xlsx::table(&headings, &rows)?))
    }

    /// A `data:` link to an error file for the report's fragment; none when
    /// it is too large to embed.
    fn error_link(&self, which: ErrorFile) -> Option<String> {
        let bytes = self.error_csv_bytes(which);
        (bytes.len() <= MAX_LINKED_ERROR_FILE).then(|| {
            format!(
                "data:text/csv;charset=utf-8;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        })
    }
}

impl IntoResponse for ImportReport {
    fn into_response(self) -> Response {
        let summary = self.summary();
        if self.failed.is_empty() && !self.dry_run {
            return (
                crate::Toast::success(summary),
                crate::HxRefresh,
                String::new(),
            )
                .into_response();
        }
        let failed: Vec<_> = self
            .failed
            .iter()
            .take(LISTED_FAILURES)
            .map(|f| json!({ "row": f.row, "messages": f.messages() }))
            .collect();
        let (failed_link, whole_link) = if self.failed.is_empty() {
            (None, None)
        } else {
            (
                self.error_link(ErrorFile::FailedRows),
                self.error_link(ErrorFile::WholeFile),
            )
        };
        crate::view(
            "renox/import_report.html",
            minijinja::context! {
                summary,
                imported => self.imported,
                dry_run => self.dry_run,
                clean => self.failed.is_empty(),
                failed,
                more => self.failed.len().saturating_sub(LISTED_FAILURES),
                failed_link,
                whole_link,
            },
        )
        .into_response()
    }
}

/// A row that wasn't imported.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FailedRow {
    /// Its number as a spreadsheet shows it: the line of column names is
    /// row 1, the first data row is row 2.
    pub row: usize,
    /// Its errors, by field.
    pub errors: Errors,
}

impl FailedRow {
    /// Every message, in field order.
    pub fn messages(&self) -> Vec<String> {
        self.errors
            .iter()
            .flat_map(|(_, list)| list.iter().cloned())
            .collect()
    }
}

/// A CSV file with only its line of column names, for people to fill in:
/// the import sheet's "Download a template" link.
///
/// ```
/// # use renox::prelude::*;
/// async fn template() -> renox::Download {
///     renox::import::template("products.csv", &["sku", "name", "price", "stock"])
/// }
/// ```
pub fn template(filename: &str, columns: &[&str]) -> crate::Download {
    let line = columns
        .iter()
        .map(|c| quote_cell(c))
        .collect::<Vec<_>>()
        .join(",");
    crate::Download::bytes(
        filename.to_owned(),
        "text/csv; charset=utf-8",
        format!("\u{feff}{line}\r\n"),
    )
}

/// An Excel template (feature `xlsx`) to match [`Import::xlsx`]: the
/// headings on row 1, an example row under them (in grey; people replace
/// or delete it) and every column formatted for its type, so what is
/// typed in reads as the import expects.
///
/// ```
/// # use renox::prelude::*;
/// use renox::import::TemplateColumn;
///
/// async fn template() -> Result<renox::Download> {
///     renox::import::template_xlsx(
///         "products.xlsx",
///         &[
///             TemplateColumn::text("sku", "COFFEE-1"),
///             TemplateColumn::text("name", "House blend"),
///             TemplateColumn::number("price", 4.5, 2),
///             TemplateColumn::integer("stock", 12),
///         ],
///     )
/// }
/// ```
#[cfg(feature = "xlsx")]
pub fn template_xlsx(filename: &str, columns: &[TemplateColumn]) -> Result<crate::Download> {
    Ok(xlsx::download(filename, xlsx::template(columns)?))
}

fn quote_cell(cell: &str) -> String {
    if cell.contains([',', '"', '\n', '\r', ';']) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_owned()
    }
}

/// RFC 4180 records with their index (from 0): `"…"` around cells holding
/// the delimiter, quotes (doubled) or line breaks; `\r\n` or `\n` ends a
/// record.
fn parse_csv(text: &str, delimiter: char) -> Vec<(usize, Vec<String>)> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut started = false;
    while let Some(c) = chars.next() {
        started = true;
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                c => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            c if c == delimiter => record.push(std::mem::take(&mut cell)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                record.push(std::mem::take(&mut cell));
                records.push((records.len(), std::mem::take(&mut record)));
                started = false;
            }
            c => cell.push(c),
        }
    }
    if started {
        record.push(cell);
        records.push((records.len(), record));
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(text: &str) -> Vec<Vec<String>> {
        parse_csv(text, ',').into_iter().map(|(_, r)| r).collect()
    }

    #[test]
    fn reads_rfc_4180() {
        assert_eq!(
            cells("a,b\r\n\"x, y\",\"say \"\"hi\"\"\"\n\"two\nlines\",\n"),
            vec![
                vec!["a".to_owned(), "b".into()],
                vec!["x, y".into(), "say \"hi\"".into()],
                vec!["two\nlines".into(), String::new()],
            ]
        );
        assert_eq!(cells("a,b"), vec![vec!["a".to_owned(), "b".into()]]);
        assert_eq!(parse_csv("a;b\n", ';')[0].1, vec!["a", "b"]);
        assert!(cells("").is_empty());
    }

    #[test]
    fn headings_name_fields() {
        assert_eq!(field_name(" Unit price "), "unit_price");
        assert_eq!(field_name("e-mail"), "e_mail");
        assert_eq!(field_name("SKU"), "sku");
    }

    #[test]
    fn templates_quote_their_cells() {
        assert_eq!(quote_cell("name"), "name");
        assert_eq!(quote_cell("a,b"), "\"a,b\"");
    }

    #[derive(serde::Deserialize)]
    struct Row {
        name: String,
        qty: i64,
        #[allow(dead_code)]
        due: Option<chrono::NaiveDate>,
    }

    impl Validate for Row {
        fn rules(&self, v: &mut Validator) {
            v.field("name", &self.name).required();
            v.field("qty", &self.qty).min(1);
        }
    }

    async fn app() -> crate::testing::TestApp {
        crate::testing::TestApp::new(crate::App::new()).await
    }

    /// Rows that don't read: a date that no placeholder makes readable
    /// fails the row; a number that doesn't read is reported once, not
    /// again by its `min` rule.
    #[tokio::test]
    async fn rows_that_dont_read_are_reported_once() {
        let app = app().await;
        let csv = "name,qty,due\nTea,2,someday\nCoffee,lots,2026-01-01\nCake,3,\n";
        let user = User::default();
        let report = Import::csv(csv)
            .user(&user)
            .run(app.state(), |_tx, _row: Row| Box::pin(async { Ok(()) }))
            .await
            .unwrap();
        assert_eq!(report.imported, 1);
        let rows: Vec<_> = report.failed.iter().map(|f| f.row).collect();
        assert_eq!(rows, [2, 3]);
        assert!(report.failed[0].errors.has("due"));
        let qty: Vec<_> = report.failed[1]
            .errors
            .iter()
            .filter(|(field, _)| *field == "qty")
            .flat_map(|(_, messages)| messages.iter())
            .collect();
        assert_eq!(qty, ["The qty must be a number."]);
        let debug = format!("{report:?}");
        assert!(
            debug.starts_with("ImportReport { imported: 1, failed: [")
                && debug.contains("all_or_nothing: false"),
            "{debug}"
        );
    }

    /// Why `write` refused a row: its validation messages, its text, a
    /// repeat, "not saved" for an internal error; any other error (a 403)
    /// stops the import.
    #[tokio::test]
    async fn writes_that_fail_are_reported_by_kind() {
        let app = app().await;
        let csv = "name,qty\ninvalid,1\nbad,1\nboom,1\nfine,1\n";
        let report = Import::csv(csv)
            .run(app.state(), |_tx, row: Row| {
                Box::pin(async move {
                    match row.name.as_str() {
                        "invalid" => {
                            let mut errors = Errors::new();
                            errors.add("name", "The name is taken.");
                            Err(ValidationError::new(errors).into())
                        }
                        "bad" => Err(Error::BadRequest("Unknown supplier.".into())),
                        "boom" => Err(Error::Internal(anyhow::anyhow!("disk full"))),
                        _ => Ok(()),
                    }
                })
            })
            .await
            .unwrap();
        assert_eq!(report.imported, 1);
        let first = |i: usize, field: &str| report.failed[i].errors.first(field).map(str::to_owned);
        assert_eq!(first(0, "name").as_deref(), Some("The name is taken."));
        assert_eq!(first(1, "row").as_deref(), Some("Unknown supplier."));
        assert!(first(2, "row").is_some_and(|m| !m.contains("disk full")));

        let stopped = Import::csv(csv)
            .run(app.state(), |_tx, _row: Row| {
                Box::pin(async { Err(Error::Forbidden) })
            })
            .await;
        assert!(matches!(stopped, Err(Error::Forbidden)));
    }

    /// A file that can't be read is an error on the form's field, named
    /// with `field`.
    #[tokio::test]
    async fn an_unreadable_file_is_an_error_on_its_field() {
        let app = app().await;
        let err = Import::csv([0xFF, 0xFE, 0x00])
            .field("upload")
            .run(app.state(), |_tx, _row: Row| Box::pin(async { Ok(()) }))
            .await
            .unwrap_err();
        let Error::Validation(invalid) = err else {
            panic!("a validation error");
        };
        assert!(invalid.errors.has("upload"));
    }
}
