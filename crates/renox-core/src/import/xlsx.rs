//! Excel files in imports (feature `xlsx`): reading a sheet as the rows a
//! CSV file would give, writing an error file and a template.

use std::io::Cursor;

use calamine::{Data, Reader, Xlsx, open_workbook_from_rs};
use chrono::{NaiveDate, NaiveDateTime, Timelike};
use rust_xlsxwriter::{Format, FormatAlign, FormatBorder, Workbook};

use crate::{Download, Error, Result};

const XLSX_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// Why a workbook couldn't give its rows.
pub(super) enum Problem {
    /// Not a readable `.xlsx` file.
    Unreadable,
    /// No sheet has this name.
    NoSheet(String),
}

/// The rows of a sheet (the first by default) as (index from 0, cells): the
/// index is the row's place in the sheet, so row numbers in a report match
/// the ones Excel shows. Every cell is text, as a CSV file's would be:
/// numbers without a needless `.0`, dates as `YYYY-MM-DD` (with the time
/// when there is one), booleans as `true` / `false`, empty cells as `""`.
pub(super) fn read(
    data: &[u8],
    sheet: Option<&str>,
) -> std::result::Result<Vec<(usize, Vec<String>)>, Problem> {
    let mut book: Xlsx<_> =
        open_workbook_from_rs(Cursor::new(data)).map_err(|_| Problem::Unreadable)?;
    let name = match sheet {
        Some(name) => {
            if !book.sheet_names().iter().any(|n| n == name) {
                return Err(Problem::NoSheet(name.to_owned()));
            }
            name.to_owned()
        }
        None => book
            .sheet_names()
            .first()
            .cloned()
            .ok_or(Problem::Unreadable)?,
    };
    let range = book
        .worksheet_range(&name)
        .map_err(|_| Problem::Unreadable)?;
    let Some((first_row, first_column)) = range.start() else {
        return Ok(Vec::new());
    };
    Ok(range
        .rows()
        .enumerate()
        .map(|(i, cells)| {
            // A used range may start at column C: keep the columns' places.
            let mut row = vec![String::new(); first_column as usize];
            row.extend(cells.iter().map(cell_text));
            (first_row as usize + i, row)
        })
        .collect())
}

/// A cell as the text a CSV file would hold.
fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(text) => text.clone(),
        Data::Int(n) => n.to_string(),
        Data::Float(f) => number_text(*f),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(moment) => {
            if moment.is_duration() {
                return match moment.as_duration() {
                    Some(d) => format!(
                        "{:02}:{:02}:{:02}",
                        d.num_hours(),
                        d.num_minutes() % 60,
                        d.num_seconds() % 60
                    ),
                    None => number_text(moment.as_f64()),
                };
            }
            let Some(at) = moment.as_datetime() else {
                return number_text(moment.as_f64());
            };
            if moment.as_f64() < 1.0 {
                // A time of day: Excel counts it from day zero.
                at.format("%H:%M:%S").to_string()
            } else if at.time().num_seconds_from_midnight() == 0 && at.nanosecond() == 0 {
                at.format("%Y-%m-%d").to_string()
            } else {
                at.format("%Y-%m-%dT%H:%M:%S").to_string()
            }
        }
        Data::DateTimeIso(text) | Data::DurationIso(text) => text.clone(),
        Data::Error(error) => error.to_string(),
    }
}

/// A number the way a person types it: `12`, not `12.0`.
fn number_text(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

fn fail(e: rust_xlsxwriter::XlsxError) -> Error {
    Error::Internal(anyhow::anyhow!(e))
}

fn heading_format() -> Format {
    Format::new()
        .set_bold()
        .set_align(FormatAlign::Center)
        .set_border(FormatBorder::Thin)
        .set_background_color("#F2F2F7")
}

/// A one-sheet workbook of text cells: headings, then rows.
pub(super) fn table(headings: &[String], rows: &[Vec<String>]) -> Result<Vec<u8>> {
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    let head = heading_format();
    for (c, heading) in headings.iter().enumerate() {
        sheet
            .write_string_with_format(0, c as u16, heading, &head)
            .map_err(fail)?;
        sheet
            .set_column_width(c as u16, (heading.chars().count() + 4).max(12) as f64)
            .map_err(fail)?;
    }
    for (r, cells) in rows.iter().enumerate() {
        for (c, cell) in cells.iter().enumerate() {
            // Text, so a code like 00123 stays what it was.
            sheet
                .write_string(r as u32 + 1, c as u16, cell)
                .map_err(fail)?;
        }
    }
    sheet.set_freeze_panes(1, 0).map_err(fail)?;
    book.save_to_buffer().map_err(fail)
}

pub(super) fn download(filename: &str, bytes: Vec<u8>) -> Download {
    Download::bytes(filename.to_owned(), XLSX_TYPE, bytes)
}

/// One column of [`template_xlsx`](super::template_xlsx): its heading, an example value and the
/// type the column's cells are formatted as, so what people type in is
/// read the way the import expects.
///
/// ```
/// use renox::import::TemplateColumn;
///
/// let columns = [
///     TemplateColumn::text("sku", "COFFEE-1"),
///     TemplateColumn::integer("stock", 12),
///     TemplateColumn::number("price", 4.5, 2),
///     TemplateColumn::date("available_on", chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
/// ];
/// # let _ = columns;
/// ```
#[derive(Debug, Clone)]
pub struct TemplateColumn {
    name: String,
    example: Example,
}

#[derive(Debug, Clone)]
enum Example {
    Text(String),
    Integer(i64),
    Number(f64, u8),
    Date(NaiveDate),
    DateTime(NaiveDateTime),
    Bool(bool),
}

impl TemplateColumn {
    fn new(name: &str, example: Example) -> Self {
        Self {
            name: name.to_owned(),
            example,
        }
    }

    /// A text column (formatted as text, so `00123` stays as typed).
    pub fn text(name: &str, example: &str) -> Self {
        Self::new(name, Example::Text(example.to_owned()))
    }

    /// A whole-number column.
    pub fn integer(name: &str, example: i64) -> Self {
        Self::new(name, Example::Integer(example))
    }

    /// A column of numbers shown with `decimals` decimal places.
    pub fn number(name: &str, example: f64, decimals: u8) -> Self {
        Self::new(name, Example::Number(example, decimals))
    }

    /// A date column (`yyyy-mm-dd`).
    pub fn date(name: &str, example: NaiveDate) -> Self {
        Self::new(name, Example::Date(example))
    }

    /// A date-and-time column (`yyyy-mm-dd hh:mm:ss`).
    pub fn datetime(name: &str, example: NaiveDateTime) -> Self {
        Self::new(name, Example::DateTime(example))
    }

    /// A true / false column.
    pub fn boolean(name: &str, example: bool) -> Self {
        Self::new(name, Example::Bool(example))
    }

    fn format(&self) -> Format {
        match &self.example {
            Example::Text(_) => Format::new().set_num_format("@"),
            Example::Integer(_) => Format::new().set_num_format("0"),
            Example::Number(_, 0) => Format::new().set_num_format("0"),
            Example::Number(_, d) => {
                Format::new().set_num_format(format!("0.{}", "0".repeat(*d as usize)))
            }
            Example::Date(_) => Format::new().set_num_format("yyyy-mm-dd"),
            Example::DateTime(_) => Format::new().set_num_format("yyyy-mm-dd hh:mm:ss"),
            Example::Bool(_) => Format::new(),
        }
    }
}

/// The template workbook: headings on row 1, an example row on row 2 (in
/// grey: people replace or delete it) and each column formatted for its type.
pub(super) fn template(columns: &[TemplateColumn]) -> Result<Vec<u8>> {
    let mut book = Workbook::new();
    let sheet = book.add_worksheet();
    let head = heading_format();
    for (c, column) in columns.iter().enumerate() {
        let c = c as u16;
        let format = column.format();
        sheet.set_column_format(c, &format).map_err(fail)?;
        sheet
            .set_column_width(c, (column.name.chars().count() + 4).max(14) as f64)
            .map_err(fail)?;
        sheet
            .write_string_with_format(0, c, &column.name, &head)
            .map_err(fail)?;
        let example = column.format().set_font_color("#8E8E93").set_italic();
        match &column.example {
            Example::Text(text) => sheet.write_string_with_format(1, c, text, &example),
            Example::Integer(n) => sheet.write_number_with_format(1, c, *n as f64, &example),
            Example::Number(n, _) => sheet.write_number_with_format(1, c, *n, &example),
            Example::Date(d) => sheet.write_datetime_with_format(1, c, *d, &example),
            Example::DateTime(d) => sheet.write_datetime_with_format(1, c, *d, &example),
            Example::Bool(b) => sheet.write_boolean_with_format(1, c, *b, &example),
        }
        .map_err(fail)?;
    }
    sheet.set_freeze_panes(1, 0).map_err(fail)?;
    book.save_to_buffer().map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dates, date-times, times of day, booleans and a used range that
    /// starts below and right of A1: read as the text a CSV file would
    /// hold, with the sheet's own row numbers.
    #[test]
    fn typed_cells_read_as_text_with_their_sheet_rows() {
        use rust_xlsxwriter::ExcelDateTime;

        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        let date = Format::new().set_num_format("yyyy-mm-dd");
        let moment = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
        let time = Format::new().set_num_format("hh:mm:ss");
        sheet.write_string(2, 1, "when").unwrap();
        sheet.write_string(2, 2, "flag").unwrap();
        sheet
            .write_datetime_with_format(3, 1, ExcelDateTime::from_ymd(2026, 1, 31).unwrap(), &date)
            .unwrap();
        sheet.write_boolean(3, 2, true).unwrap();
        sheet
            .write_datetime_with_format(
                4,
                1,
                ExcelDateTime::from_ymd(2026, 2, 1)
                    .unwrap()
                    .and_hms(13, 30, 5)
                    .unwrap(),
                &moment,
            )
            .unwrap();
        sheet
            .write_datetime_with_format(5, 1, ExcelDateTime::from_hms(8, 15, 0).unwrap(), &time)
            .unwrap();
        let bytes = book.save_to_buffer().unwrap();
        let rows = read(&bytes, None).ok().unwrap();
        assert_eq!(
            rows,
            vec![
                (2, vec!["".into(), "when".into(), "flag".into()]),
                (3, vec!["".into(), "2026-01-31".into(), "true".into()]),
                (4, vec!["".into(), "2026-02-01T13:30:05".into(), "".into()]),
                (5, vec!["".into(), "08:15:00".into(), "".into()]),
            ]
        );
        assert!(matches!(
            read(&bytes, Some("Nope")),
            Err(Problem::NoSheet(_))
        ));
        assert!(matches!(read(b"not a zip", None), Err(Problem::Unreadable)));
    }

    #[test]
    fn numbers_read_like_typed_text() {
        assert_eq!(number_text(12.0), "12");
        assert_eq!(number_text(-3.0), "-3");
        assert_eq!(number_text(12.5), "12.5");
        assert_eq!(number_text(0.1), "0.1");
        assert_eq!(number_text(1234567890123.0), "1234567890123");
    }
}
