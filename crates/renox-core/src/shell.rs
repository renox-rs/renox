//! `my-app db:shell`: a small SQL prompt on the app's database, so neither
//! `sqlite3` nor `psql` is needed (on servers or Windows).
//!
//! Statements end with `;` and may span lines. `.tables` lists tables and
//! `.quit` (or end of input) leaves. Input can be piped:
//! `echo "SELECT count(*) FROM users;" | my-app db:shell`.

use std::io::{IsTerminal, Write};

use sqlx::{Row as _, TypeInfo, ValueRef};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};

use crate::Result;
use crate::db::{Db, Dialect, Row, RowInner, script, sql};

/// Longest cell shown; longer values are cut.
const CELL_WIDTH: usize = 60;

pub(crate) async fn run(db: &Db) -> Result {
    let interactive = std::io::stdin().is_terminal();
    let mut out = std::io::stdout();
    run_with(
        db,
        BufReader::new(tokio::io::stdin()),
        &mut out,
        interactive,
    )
    .await
}

/// The shell over any input and output, e.g. for tests. `interactive` shows
/// a banner and prompts.
#[doc(hidden)]
pub async fn run_with(
    db: &Db,
    input: impl AsyncBufRead + Unpin,
    out: &mut impl Write,
    interactive: bool,
) -> Result {
    if interactive {
        writeln!(
            out,
            "{} shell. End statements with `;`. `.tables` lists tables, `.quit` leaves.",
            match db.dialect() {
                Dialect::Sqlite => "SQLite",
                Dialect::Postgres => "PostgreSQL",
            }
        )?;
    }
    let mut lines = input.lines();
    let mut statement = String::new();
    loop {
        if interactive {
            write!(
                out,
                "{}",
                if statement.is_empty() {
                    "sql> "
                } else {
                    "...> "
                }
            )?;
            out.flush()?;
        }
        let Some(line) = lines.next_line().await? else {
            break;
        };
        let trimmed = line.trim();
        if statement.is_empty() {
            match trimmed {
                "" => continue,
                ".quit" | ".exit" => break,
                ".tables" => {
                    let names: Vec<String> = sql(match db.dialect() {
                        Dialect::Sqlite => {
                            "SELECT name FROM sqlite_master \
                             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
                        }
                        Dialect::Postgres => {
                            "SELECT tablename::text FROM pg_tables \
                             WHERE schemaname = current_schema() ORDER BY tablename"
                        }
                    })
                    .scalars(db)
                    .await?;
                    writeln!(out, "{}", names.join("  "))?;
                    continue;
                }
                _ => {}
            }
        }
        statement.push_str(&line);
        statement.push('\n');
        if trimmed.ends_with(';') {
            let sql = std::mem::take(&mut statement);
            match execute(db, sql.trim()).await {
                Ok(text) => write!(out, "{text}")?,
                Err(err) => writeln!(out, "Error: {err}")?,
            }
        }
    }
    Ok(())
}

/// Runs one statement and returns what to print.
async fn execute(db: &Db, sql: &str) -> std::result::Result<String, crate::db::DbError> {
    let first = sql
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        first.as_str(),
        "select" | "pragma" | "with" | "explain" | "values" | "show"
    ) {
        let rows = self::sql(sql).fetch_all(db).await?;
        Ok(table(&rows))
    } else {
        let done = script(db, sql).await?;
        Ok(format!("OK ({done} row(s) affected)\n"))
    }
}

fn cell(row: &Row, index: usize) -> String {
    match &row.0 {
        RowInner::Sqlite(row) => sqlite_cell(row, index),
        #[cfg(feature = "postgres")]
        RowInner::Postgres(row) => postgres_cell(row, index),
    }
}

fn sqlite_cell(row: &sqlx::sqlite::SqliteRow, index: usize) -> String {
    let Ok(raw) = row.try_get_raw(index) else {
        return "?".into();
    };
    if raw.is_null() {
        return "NULL".into();
    }
    match raw.type_info().name() {
        "INTEGER" => row
            .try_get::<i64, _>(index)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        "REAL" => row
            .try_get::<f64, _>(index)
            .map(|v| v.to_string())
            .unwrap_or_default(),
        "BLOB" => row
            .try_get::<Vec<u8>, _>(index)
            .map(|v| format!("<{} bytes>", v.len()))
            .unwrap_or_default(),
        _ => row.try_get::<String, _>(index).unwrap_or_default(),
    }
}

#[cfg(feature = "postgres")]
fn postgres_cell(row: &sqlx::postgres::PgRow, index: usize) -> String {
    let Ok(raw) = row.try_get_raw(index) else {
        return "?".into();
    };
    if raw.is_null() {
        return "NULL".into();
    }
    fn show<T: ToString>(value: Result<T, sqlx::Error>) -> String {
        value.map(|v| v.to_string()).unwrap_or_else(|_| "?".into())
    }
    match raw.type_info().name() {
        "INT2" => show(row.try_get::<i16, _>(index)),
        "INT4" => show(row.try_get::<i32, _>(index)),
        "INT8" => show(row.try_get::<i64, _>(index)),
        "FLOAT4" => show(row.try_get::<f32, _>(index)),
        "FLOAT8" => show(row.try_get::<f64, _>(index)),
        "BOOL" => show(row.try_get::<bool, _>(index)),
        "BYTEA" => row
            .try_get::<Vec<u8>, _>(index)
            .map(|v| format!("<{} bytes>", v.len()))
            .unwrap_or_else(|_| "?".into()),
        "TIMESTAMPTZ" => show(row.try_get::<chrono::DateTime<chrono::Utc>, _>(index)),
        "TIMESTAMP" => show(row.try_get::<chrono::NaiveDateTime, _>(index)),
        "DATE" => show(row.try_get::<chrono::NaiveDate, _>(index)),
        _ => show(row.try_get::<String, _>(index)),
    }
}

/// Rows as an aligned text table with a header and a count.
fn table(rows: &[Row]) -> String {
    let Some(first) = rows.first() else {
        return "(no rows)\n".into();
    };
    let header: Vec<String> = first.columns().into_iter().map(str::to_owned).collect();
    let body: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            (0..header.len())
                .map(|i| cell(row, i).chars().take(CELL_WIDTH).collect())
                .collect()
        })
        .collect();
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for row in &body {
        for (width, value) in widths.iter_mut().zip(row) {
            *width = (*width).max(value.chars().count());
        }
    }
    let line = |cells: &[String]| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!("{c:<w$}"))
            .collect();
        padded.join(" | ").trim_end().to_owned()
    };
    let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    let mut out = format!("{}\n{}\n", line(&header), rule.join("-+-"));
    for row in &body {
        out.push_str(&line(row));
        out.push('\n');
    }
    out.push_str(&format!("({} row(s))\n", body.len()));
    out
}
