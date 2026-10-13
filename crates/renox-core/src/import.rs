//! Imports: the rows of a CSV file, each checked with a form's rules, then
//! written in one transaction (Filament's import action).
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
//! UTF-8, no rows, too many rows) is a validation error on the form's
//! `file` field.

use std::collections::HashMap;
use std::fmt;

use axum::response::{IntoResponse, Response};
use futures_util::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::auth::User;
use crate::db::Transaction;
use crate::validation::extract::{Messages, Parsed, parse_pairs};
use crate::validation::{Errors, FormContext, Validate, ValidationError, Validator};
use crate::{AppState, Error, Lang, Result};

/// The most rows one import reads, unless [`Import::max_rows`] says otherwise.
pub const MAX_IMPORT_ROWS: usize = 10_000;

/// What writes one row (`run`'s closure), boxed.
type WriteRow<'a, T> =
    Box<dyn for<'t> FnMut(&'t mut Transaction, T) -> BoxFuture<'t, Result> + Send + 'a>;

/// A data row: its row number and its (column, value) pairs.
type Record = (usize, Vec<(String, String)>);

/// The most failed rows the report's table lists; the rest are counted.
const LISTED_FAILURES: usize = 100;

/// A CSV file to import; see the [module docs](self).
#[must_use = "an import does nothing until it is `run`"]
pub struct Import {
    data: Vec<u8>,
    field: String,
    delimiter: char,
    headers: Option<Vec<String>>,
    renames: Vec<(String, String)>,
    max_rows: usize,
    all_or_nothing: bool,
    lang: Option<Lang>,
    user: Option<User>,
}

impl Import {
    /// The bytes of a CSV file (an upload's, a file's): UTF-8, with or
    /// without a byte order mark, its first line naming the columns.
    pub fn csv(data: impl AsRef<[u8]>) -> Self {
        Self {
            data: data.as_ref().to_vec(),
            field: "file".into(),
            delimiter: ',',
            headers: None,
            renames: Vec::new(),
            max_rows: MAX_IMPORT_ROWS,
            all_or_nothing: false,
            lang: None,
            user: None,
        }
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
    /// `T`'s `authorize` hook isn't asked: authorize the import's route.
    pub fn run<'a, T, F>(
        self,
        state: &'a AppState,
        write: F,
    ) -> impl Future<Output = Result<ImportReport>> + Send + 'a
    where
        T: DeserializeOwned + Validate + Send + Sync + 'a,
        F: for<'t> FnMut(&'t mut Transaction, T) -> BoxFuture<'t, Result> + Send + 'a,
    {
        let lang = self
            .lang
            .clone()
            .unwrap_or_else(|| state.lang(&state.config.locale));
        // Boxed: a generic closure held across `.await`s keeps handler
        // futures from being `Send` (rustc #100013).
        let mut write: WriteRow<'a, T> = Box::new(write);
        async move {
            let records = self.records(&lang)?;
            let mut report = ImportReport {
                imported: 0,
                failed: Vec::new(),
                all_or_nothing: self.all_or_nothing,
                lang: lang.clone(),
            };
            let messages = Messages {
                texts: lang.texts(),
            };
            let mut valid = Vec::new();
            for (row, pairs) in records {
                match check::<T>(state, self.user.as_ref(), &messages, pairs).await? {
                    Ok(data) => valid.push((row, data)),
                    Err(errors) => report.failed.push(FailedRow { row, errors }),
                }
            }
            if self.all_or_nothing && !report.failed.is_empty() {
                return Ok(report);
            }
            let mut tx = state.db.begin().await?;
            let mut imported = 0;
            for (row, data) in valid {
                let outcome = tx.savepoint(|tx| write(tx, data)).await;
                match outcome {
                    Ok(()) => imported += 1,
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
            tx.commit().await?;
            report.imported = imported;
            report.failed.sort_by_key(|f| f.row);
            Ok(report)
        }
    }

    /// The data rows as (row number, column/value pairs), or the file's
    /// error on the form's field.
    fn records(&self, lang: &Lang) -> Result<Vec<Record>> {
        let refuse = |key: &str, count: usize| -> Error {
            let mut errors = Errors::new();
            errors.add(
                self.field.clone(),
                lang.t(key, &[("count", &count as &dyn fmt::Display)]),
            );
            ValidationError::new(errors).into()
        };
        let Ok(text) = std::str::from_utf8(&self.data) else {
            return Err(refuse("ui.import.not_utf8", 0));
        };
        let mut rows = parse_csv(text.trim_start_matches('\u{feff}'), self.delimiter);
        // A record's index from 0 is its spreadsheet row number minus one.
        let headers = match &self.headers {
            Some(headers) => headers.clone(),
            None if rows.is_empty() => return Err(refuse("ui.import.empty", 0)),
            None => rows.remove(0).1,
        };
        let headers: Vec<String> = headers
            .iter()
            .map(|h| {
                let key = field_name(h);
                self.renames
                    .iter()
                    .find(|(from, _)| *from == key)
                    .map_or(key, |(_, to)| to.clone())
            })
            .collect();
        let records: Vec<Record> = rows
            .into_iter()
            .filter(|(_, cells)| cells.iter().any(|c| !c.trim().is_empty()))
            .map(|(index, cells)| {
                let pairs = headers
                    .iter()
                    .zip(cells)
                    .filter(|(h, _)| !h.is_empty())
                    .map(|(h, c)| (h.clone(), c))
                    .collect();
                (index + 1, pairs)
            })
            .collect();
        if records.is_empty() {
            return Err(refuse("ui.import.empty", 0));
        }
        if records.len() > self.max_rows {
            return Err(refuse("ui.import.too_many", self.max_rows));
        }
        Ok(records)
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
/// open.
#[non_exhaustive]
pub struct ImportReport {
    /// Rows written.
    pub imported: usize,
    /// Rows left out, by row number.
    pub failed: Vec<FailedRow>,
    all_or_nothing: bool,
    lang: Lang,
}

impl fmt::Debug for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportReport")
            .field("imported", &self.imported)
            .field("failed", &self.failed)
            .field("all_or_nothing", &self.all_or_nothing)
            .finish()
    }
}

impl ImportReport {
    /// Every row was imported.
    pub fn is_clean(&self) -> bool {
        self.failed.is_empty()
    }

    /// A one-line summary: "3 rows imported.", "2 rows imported, 1 left
    /// out.", or "Nothing was imported: 1 row has errors.".
    pub fn summary(&self) -> String {
        let failed = self.failed.len() as i64;
        if self.failed.is_empty() {
            self.lang
                .choice("ui.import.done", self.imported as i64, &[])
        } else if self.imported == 0 {
            self.lang.choice("ui.import.nothing", failed, &[])
        } else {
            self.lang.choice(
                "ui.import.partial",
                failed,
                &[("imported", &self.imported as &dyn fmt::Display)],
            )
        }
    }
}

impl IntoResponse for ImportReport {
    fn into_response(self) -> Response {
        let summary = self.summary();
        if self.failed.is_empty() {
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
        crate::view(
            "renox/import_report.html",
            minijinja::context! {
                summary,
                imported => self.imported,
                failed,
                more => self.failed.len().saturating_sub(LISTED_FAILURES),
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
