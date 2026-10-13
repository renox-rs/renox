//! A grid's exports: every row its filters match, in the user's columns, as
//! CSV, an Excel workbook (the `xlsx` feature) or a page to print.

use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{Column, Grid, GridPrefs, GridRequest, Kind, Pin, header_rows, load_prefs};
use crate::db::{Model, Query};
use crate::{Error, Result};

/// The most rows one export holds.
pub const MAX_EXPORT_ROWS: u64 = 100_000;

/// The links of the export menu: the page's filters and sort, plus `export`.
pub(super) fn urls(path: &str, query: &[(String, String)], per_page: &str, export: &str) -> Value {
    let link = |format: &str| {
        let mut pairs: Vec<(&str, &str)> = query
            .iter()
            .filter(|(k, _)| k != per_page && k != export)
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        pairs.push((export, format));
        format!(
            "{path}?{}",
            serde_html_form::to_string(&pairs).unwrap_or_default()
        )
    };
    json!({
        "csv": link("csv"),
        "xlsx": cfg!(feature = "xlsx").then(|| link("xlsx")),
        "print": link("print"),
    })
}

impl Grid {
    /// Answers the export menu's links (`?export=csv|xlsx|print`): `None`
    /// when the request isn't an export, so the handler goes on to draw the
    /// page.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::grid::{Column, Grid, GridRequest};
    /// # #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, number: String }
    /// fn orders() -> Grid {
    ///     Grid::new("orders").column(Column::text("number", "Order")).exports()
    /// }
    ///
    /// async fn index(request: GridRequest) -> Result<Response> {
    ///     if let Some(file) = orders().export(Order::query(), &request).await? {
    ///         return Ok(file);
    ///     }
    ///     let page = orders().page(Order::query(), &request).await?;
    ///     Ok(view("orders/index.html", context! { orders => page }).into_response())
    /// }
    /// ```
    ///
    /// Custom columns are left out, and so are the columns the user hid on
    /// wide screens. Up to [`MAX_EXPORT_ROWS`] rows.
    pub async fn export<M: Model + Serialize>(
        &self,
        query: Query<M>,
        request: &GridRequest,
    ) -> Result<Option<Response>> {
        let Some(format) = request.param(&self.name("export")) else {
            return Ok(None);
        };
        let Some(format) = ExportFormat::parse(format) else {
            return Err(Error::BadRequest(format!(
                "`{format}` isn't an export (csv, xlsx, print)"
            )));
        };
        let prefs = load_prefs(request, &self.id).await;
        let wide = prefs
            .wide
            .clone()
            .unwrap_or_else(|| self.default_visible(false));
        let columns: Vec<(&Column, Option<Pin>)> = self
            .ordered(&prefs)
            .into_iter()
            .filter(|(c, _)| c.kind != Kind::Custom && wide.contains(&c.key))
            .collect();
        let query = self.filter(query, request);
        self.file(&columns, query, format, request).await.map(Some)
    }

    /// Every row `query` matches as a file, outside the grid's page: an
    /// "Export" action on a record's page or in a menu (the grid's own
    /// export menu uses [`Grid::export`]). The grid's columns that show by
    /// default on wide screens, in their order; the request's filters,
    /// sort and the user's column choices don't apply (sort the query
    /// yourself). Up to [`MAX_EXPORT_ROWS`] rows.
    ///
    /// ```
    /// # use renox::prelude::*;
    /// use renox::grid::{Column, ExportFormat, Grid, GridRequest};
    /// # #[derive(Model, serde::Serialize, Default)] struct Movement { id: i64, product_id: i64, quantity: i64 }
    /// fn ledger() -> Grid {
    ///     Grid::new("ledger").column(Column::number("quantity", "Quantity"))
    /// }
    ///
    /// // GET /products/{id}/ledger.csv
    /// async fn export_ledger(Path(id): Path<i64>, request: GridRequest) -> Result<Response> {
    ///     let movements = Movement::where_eq("product_id", id).order_by("id");
    ///     ledger().export_as(movements, ExportFormat::Csv, &request).await
    /// }
    /// ```
    pub async fn export_as<M: Model + Serialize>(
        &self,
        query: Query<M>,
        format: ExportFormat,
        request: &GridRequest,
    ) -> Result<Response> {
        let shown = self.default_visible(false);
        let columns: Vec<(&Column, Option<Pin>)> = self
            .ordered(&GridPrefs::default())
            .into_iter()
            .filter(|(c, _)| c.kind != Kind::Custom && shown.contains(&c.key))
            .collect();
        self.file(&columns, query, format, request).await
    }

    /// Every row of `query` (up to [`MAX_EXPORT_ROWS`]) with the related
    /// values the grid's columns show.
    async fn export_rows<M: Model + Serialize>(
        &self,
        query: Query<M>,
        request: &GridRequest,
    ) -> Result<Vec<Map<String, Value>>> {
        let items = query.limit(MAX_EXPORT_ROWS).get(&request.db).await?;
        let related = self.related_values(&request.db, &items).await?;
        Ok(items
            .iter()
            .zip(related)
            .map(|(item, related)| {
                let mut row = match serde_json::to_value(item) {
                    Ok(Value::Object(map)) => map,
                    _ => Map::new(),
                };
                row.extend(related);
                row
            })
            .collect())
    }

    async fn file<M: Model + Serialize>(
        &self,
        columns: &[(&Column, Option<Pin>)],
        query: Query<M>,
        format: ExportFormat,
        request: &GridRequest,
    ) -> Result<Response> {
        let rows = self.export_rows(query, request).await?;
        let today = crate::db::now().format("%Y-%m-%d").to_string();
        let name = format!("{}-{today}", self.id);
        let table = Table {
            columns,
            rows: &rows,
            request,
        };
        Ok(match format {
            ExportFormat::Csv => crate::Download::bytes(
                format!("{name}.csv"),
                "text/csv; charset=utf-8",
                table.csv(),
            )
            .into_response(),
            ExportFormat::Xlsx => xlsx(&table, &name)?,
            ExportFormat::Print => crate::view(
                "renox/grid_print.html",
                minijinja::context! {
                    title => self.title.clone().unwrap_or_else(|| self.id.clone()),
                    header => header_rows(columns),
                    columns => columns.iter().map(|(c, _)| json!({
                        "key": c.key,
                        "label": c.label,
                        "numeric": matches!(c.kind, Kind::Number | Kind::Money),
                    })).collect::<Vec<_>>(),
                    rows => rows.iter().map(|row| columns.iter().map(|(c, _)| table.display(c, row.get(&c.key))).collect::<Vec<_>>()).collect::<Vec<_>>(),
                    total => rows.len(),
                    printed => crate::db::now().to_rfc3339(),
                    back => format!("{}?{}", request.path, serde_html_form::to_string(
                        request.params.iter().filter(|(k, _)| *k != self.name("export")).collect::<Vec<_>>()
                    ).unwrap_or_default()),
                },
            )
            .into_response(),
        })
    }
}

/// The file an export makes. `Xlsx` needs renox's `xlsx` feature (without
/// it the export answers 400). It reads from a route or query string as
/// `csv`, `xlsx` or `print`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ExportFormat {
    /// CSV with a byte order mark, so Excel reads it as UTF-8.
    Csv,
    /// An Excel workbook (the `xlsx` feature).
    Xlsx,
    /// A page to print (or save as PDF from the browser).
    Print,
}

impl ExportFormat {
    /// `csv`, `xlsx` or `print`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "csv" => Some(Self::Csv),
            "xlsx" => Some(Self::Xlsx),
            "print" => Some(Self::Print),
            _ => None,
        }
    }

    /// Whether this build can make it (`Xlsx` needs the `xlsx` feature).
    pub fn available(self) -> bool {
        self != Self::Xlsx || cfg!(feature = "xlsx")
    }
}

/// An Excel workbook with several sheets, each one a grid's export of a
/// query (needs renox's `xlsx` feature; without it [`Workbook::sheet`]
/// answers 400, like the single-sheet export). Sheets appear in the order
/// they are added; each uses the grid's columns that show by default on
/// wide screens, with the same headings, number formats and frozen columns
/// as [`Grid::export_as`].
///
/// ```
/// # use renox::prelude::*;
/// use renox::grid::{Column, Grid, GridRequest, Workbook};
/// # #[derive(Model, serde::Serialize, Default)] struct Sale { id: i64, total: i64 }
/// # #[derive(Model, serde::Serialize, Default)] struct Refund { id: i64, amount: i64 }
/// // GET /reports/summary.xlsx
/// async fn summary(request: GridRequest) -> Result<Response> {
///     let sales = Grid::new("sales").column(Column::money("total", "Total"));
///     let refunds = Grid::new("refunds").column(Column::money("amount", "Amount"));
///     Workbook::new("summary")
///         .sheet("Sales", &sales, Sale::query().order_by("id"), &request).await?
///         .sheet("Refunds", &refunds, Refund::query().order_by("id"), &request).await?
///         .into_response()
/// }
/// ```
#[non_exhaustive]
pub struct Workbook {
    name: String,
    #[cfg(feature = "xlsx")]
    book: rust_xlsxwriter::Workbook,
}

impl Workbook {
    /// A workbook whose file is `{name}-{date}.xlsx`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            #[cfg(feature = "xlsx")]
            book: rust_xlsxwriter::Workbook::new(),
        }
    }

    /// Adds a sheet called `title` (at most 31 characters, none of
    /// `[]:*?/\`, unique in the workbook) with every row `query`
    /// matches, up to [`MAX_EXPORT_ROWS`]. The request's filters and the
    /// user's column choices don't apply (as in [`Grid::export_as`]).
    pub async fn sheet<M: Model + Serialize>(
        mut self,
        title: &str,
        grid: &Grid,
        query: Query<M>,
        request: &GridRequest,
    ) -> Result<Self> {
        let shown = grid.default_visible(false);
        let columns: Vec<(&Column, Option<Pin>)> = grid
            .ordered(&GridPrefs::default())
            .into_iter()
            .filter(|(c, _)| c.kind != Kind::Custom && shown.contains(&c.key))
            .collect();
        let rows = grid.export_rows(query, request).await?;
        let table = Table {
            columns: &columns,
            rows: &rows,
            request,
        };
        self.add(title, &table)?;
        Ok(self)
    }

    #[cfg(feature = "xlsx")]
    fn add(&mut self, title: &str, table: &Table<'_>) -> Result<()> {
        let sheet = self.book.add_worksheet();
        sheet
            .set_name(title)
            .map_err(|e| Error::BadRequest(format!("`{title}` can't name a sheet: {e}")))?;
        write_sheet(sheet, table)
    }

    #[cfg(not(feature = "xlsx"))]
    fn add(&mut self, _: &str, _: &Table<'_>) -> Result<()> {
        Err(Error::BadRequest(
            "Excel exports need renox's `xlsx` feature".into(),
        ))
    }

    /// The `.xlsx` download of the sheets added so far.
    pub fn into_response(self) -> Result<Response> {
        let today = crate::db::now().format("%Y-%m-%d").to_string();
        let name = format!("{}-{today}", self.name);
        self.finish(&name)
    }

    #[cfg(feature = "xlsx")]
    fn finish(mut self, name: &str) -> Result<Response> {
        let bytes = self
            .book
            .save_to_buffer()
            .map_err(|e| Error::Internal(anyhow::anyhow!(e)))?;
        Ok(xlsx_download(name, bytes))
    }

    #[cfg(not(feature = "xlsx"))]
    fn finish(self, _: &str) -> Result<Response> {
        Err(Error::BadRequest(
            "Excel exports need renox's `xlsx` feature".into(),
        ))
    }
}

/// The exported rows with their columns.
struct Table<'a> {
    columns: &'a [(&'a Column, Option<Pin>)],
    rows: &'a [Map<String, Value>],
    request: &'a GridRequest,
}

impl Table<'_> {
    /// A number as the grid shows it, with its decimals: money is divided
    /// from the smallest unit into whole units of `APP_CURRENCY`.
    fn figure(&self, column: &Column, n: f64) -> (f64, Option<u32>) {
        match column.kind {
            Kind::Money => {
                let decimals = self.request.money_decimals;
                (
                    n / 10f64.powi(decimals as i32),
                    Some(column.decimals.map_or(decimals, u32::from)),
                )
            }
            _ => (n, column.decimals.map(u32::from)),
        }
    }

    fn yes_no(&self, yes: bool) -> String {
        let key = if yes { "ui.grid.yes" } else { "ui.grid.no" };
        match &self.request.lang {
            Some(lang) => lang.t(key, &[]),
            None => (if yes { "Yes" } else { "No" }).to_owned(),
        }
    }

    /// One heading per column: its groups and its label, `Amounts / Total`.
    fn heading(column: &Column) -> String {
        let mut parts = column.under.clone();
        parts.push(column.label.clone());
        parts.join(" / ")
    }

    /// A value as text, the way the grid shows it (without number
    /// separators, so spreadsheets read numbers).
    fn text(&self, column: &Column, value: Option<&Value>) -> String {
        let label = |v: &str| {
            column
                .options
                .iter()
                .find(|(o, _)| o == v)
                .map_or_else(|| v.to_owned(), |(_, l)| l.clone())
        };
        match value {
            None | Some(Value::Null) => String::new(),
            Some(Value::Bool(b)) => self.yes_no(*b),
            Some(Value::Number(n)) if column.kind == Kind::Bool => {
                self.yes_no(n.as_i64() != Some(0))
            }
            Some(Value::Number(n)) => match n.as_f64().map(|f| self.figure(column, f)) {
                Some((f, Some(d))) => format!("{f:.*}", d as usize),
                _ => n.to_string(),
            },
            Some(Value::String(s)) => match column.kind {
                Kind::Select => label(s),
                Kind::DateTime => self.moment(s).unwrap_or_else(|| s.clone()),
                _ => s.clone(),
            },
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| v.as_str().map_or_else(|| v.to_string(), label))
                .collect::<Vec<_>>()
                .join(", "),
            Some(other) => other.to_string(),
        }
    }

    /// A value for people to read: numbers with the locale's separators.
    fn display(&self, column: &Column, value: Option<&Value>) -> String {
        match (column.kind, value.and_then(Value::as_f64)) {
            (Kind::Number | Kind::Money, Some(n)) if column.kind != Kind::Bool => {
                let locale = self
                    .request
                    .lang
                    .as_ref()
                    .map_or("en", |l| l.locale.as_str());
                let (n, decimals) = self.figure(column, n);
                crate::view_filters::format_number(n, decimals.unwrap_or(0), locale)
            }
            _ => self.text(column, value),
        }
    }

    /// A moment in `APP_TIMEZONE`: `2026-03-05 14:30`.
    fn moment(&self, text: &str) -> Option<String> {
        let at = chrono::DateTime::parse_from_rfc3339(text).ok()?;
        let local = self.request.zone.local(at.timestamp());
        Some(local.format("%Y-%m-%d %H:%M").to_string())
    }

    /// RFC 4180 CSV with a BOM (so Excel reads UTF-8). Text that a
    /// spreadsheet would run as a formula (`=`, `+`, `-`, `@` first) gets a
    /// `'` in front.
    fn csv(&self) -> Vec<u8> {
        let mut out = String::from('\u{feff}');
        let line = |cells: Vec<String>| -> String {
            cells
                .into_iter()
                .map(|cell| {
                    if cell.contains([',', '"', '\n', '\r']) {
                        format!("\"{}\"", cell.replace('"', "\"\""))
                    } else {
                        cell
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
                + "\r\n"
        };
        out.push_str(&line(
            self.columns.iter().map(|(c, _)| Self::heading(c)).collect(),
        ));
        for row in self.rows {
            out.push_str(&line(
                self.columns
                    .iter()
                    .map(|(c, _)| {
                        let text = self.text(c, row.get(&c.key));
                        let numeric = matches!(c.kind, Kind::Number | Kind::Money);
                        if !numeric && text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
                            format!("'{text}")
                        } else {
                            text
                        }
                    })
                    .collect(),
            ));
        }
        out.into_bytes()
    }
}

#[cfg(feature = "xlsx")]
fn xlsx(table: &Table<'_>, name: &str) -> Result<Response> {
    let mut book = rust_xlsxwriter::Workbook::new();
    write_sheet(book.add_worksheet(), table)?;
    let bytes = book
        .save_to_buffer()
        .map_err(|e| Error::Internal(anyhow::anyhow!(e)))?;
    Ok(xlsx_download(name, bytes))
}

#[cfg(feature = "xlsx")]
fn xlsx_download(name: &str, bytes: Vec<u8>) -> Response {
    crate::Download::bytes(
        format!("{name}.xlsx"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        bytes,
    )
    .into_response()
}

/// Writes a table into a worksheet: headings, rows, frozen panes.
#[cfg(feature = "xlsx")]
fn write_sheet(sheet: &mut rust_xlsxwriter::Worksheet, table: &Table<'_>) -> Result<()> {
    use rust_xlsxwriter::{Format, FormatAlign, FormatBorder};

    let fail = |e: rust_xlsxwriter::XlsxError| Error::Internal(anyhow::anyhow!(e));
    let head = Format::new()
        .set_bold()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_border(FormatBorder::Thin)
        .set_background_color("#F2F2F7");
    // The headings as on screen: groups span their columns, leaves span
    // down to the last heading row.
    let header = header_rows(table.columns);
    let depth = header.len() as u32;
    let mut column_of: std::collections::HashMap<String, u16> = Default::default();
    for (i, (c, _)) in table.columns.iter().enumerate() {
        column_of.insert(c.key.clone(), i as u16);
    }
    for (r, cells) in header.iter().enumerate() {
        for cell in cells {
            let keys: Vec<u16> = cell["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|k| k.as_str().and_then(|k| column_of.get(k).copied()))
                .collect();
            let (Some(&first), Some(&last)) = (keys.iter().min(), keys.iter().max()) else {
                continue;
            };
            let label = match cell["key"].as_str() {
                Some(key) => table
                    .columns
                    .iter()
                    .find(|(c, _)| c.key == key)
                    .map(|(c, _)| c.label.clone())
                    .unwrap_or_default(),
                None => cell["label"].as_str().unwrap_or_default().to_owned(),
            };
            let row = r as u32;
            let bottom = row + cell["rowspan"].as_u64().unwrap_or(1) as u32 - 1;
            if first == last && row == bottom {
                sheet
                    .write_string_with_format(row, first, &label, &head)
                    .map_err(fail)?;
            } else {
                sheet
                    .merge_range(row, first, bottom, last, &label, &head)
                    .map_err(fail)?;
            }
        }
    }
    let decimals = table.request.money_decimals as usize;
    let money = Format::new().set_num_format(if decimals == 0 {
        "#,##0".to_owned()
    } else {
        format!("#,##0.{}", "0".repeat(decimals))
    });
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let moment = Format::new().set_num_format("yyyy-mm-dd hh:mm");
    for (i, row) in table.rows.iter().enumerate() {
        let r = depth + i as u32;
        for (c, (column, _)) in table.columns.iter().enumerate() {
            let c = c as u16;
            let value = row.get(&column.key);
            match (column.kind, value) {
                (_, None | Some(Value::Null)) => {}
                (Kind::Number | Kind::Money, Some(Value::Number(n))) => {
                    let n = n.as_f64().unwrap_or_default();
                    match (column.kind, column.decimals) {
                        (Kind::Money, _) => {
                            let (n, _) = table.figure(column, n);
                            sheet.write_number_with_format(r, c, n, &money)
                        }
                        (_, Some(d)) if d > 0 => sheet.write_number_with_format(
                            r,
                            c,
                            n,
                            &Format::new()
                                .set_num_format(format!("#,##0.{}", "0".repeat(usize::from(d)))),
                        ),
                        _ => sheet.write_number(r, c, n),
                    }
                    .map_err(fail)?;
                }
                (Kind::Date, Some(Value::String(s))) => {
                    match s.parse::<chrono::NaiveDate>() {
                        Ok(d) => sheet.write_datetime_with_format(r, c, d, &date),
                        Err(_) => sheet.write_string(r, c, s),
                    }
                    .map_err(fail)?;
                }
                (Kind::DateTime, Some(Value::String(s))) => {
                    match chrono::DateTime::parse_from_rfc3339(s) {
                        Ok(at) => {
                            let local = table.request.zone.local(at.timestamp());
                            sheet.write_datetime_with_format(r, c, local, &moment)
                        }
                        Err(_) => sheet.write_string(r, c, s),
                    }
                    .map_err(fail)?;
                }
                _ => {
                    sheet
                        .write_string(r, c, table.text(column, value))
                        .map_err(fail)?;
                }
            }
        }
    }
    // Headings and the frozen-left columns stay put, as in the grid.
    let frozen = table
        .columns
        .iter()
        .take_while(|(_, pin)| *pin == Some(Pin::Left))
        .count() as u16;
    sheet.set_freeze_panes(depth, frozen).map_err(fail)?;
    sheet.autofit();
    Ok(())
}

#[cfg(not(feature = "xlsx"))]
fn xlsx(_: &Table<'_>, _: &str) -> Result<Response> {
    Err(Error::BadRequest(
        "Excel exports need renox's `xlsx` feature".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_links_keep_the_filters() {
        let urls = urls(
            "/orders",
            &[
                ("q.name".into(), "iced coffee".into()),
                ("per_page".into(), "25".into()),
            ],
            "per_page",
            "export",
        );
        assert_eq!(urls["csv"], "/orders?q.name=iced+coffee&export=csv");
        assert_eq!(urls["print"], "/orders?q.name=iced+coffee&export=print");
        assert_eq!(urls["xlsx"].is_string(), cfg!(feature = "xlsx"));
    }
}
