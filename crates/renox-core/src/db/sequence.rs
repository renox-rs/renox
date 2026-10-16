//! Gapless numbering for documents: invoices, credit notes, receipts.
//!
//! A [`Sequence`] is a counter per series, scope and period, kept in the
//! `sequences` table. [`Sequence::next`] takes the next number **inside the
//! caller's transaction**, so a rollback gives the number back: no gaps, and
//! no duplicates however many tasks or processes issue numbers at once (the
//! row lock holds until commit on PostgreSQL; SQLite's write lock does the
//! same). Counters that expire or live outside the transaction (the cache's
//! `increment`, rate limits) cannot offer that.
//!
//! ```
//! use renox::db::{NumberFormat, Period, Sequence};
//! # async fn demo(db: renox::db::Db) -> renox::Result {
//! let today = renox::db::now().date_naive();
//! let mut tx = db.begin().await?;
//! let n = Sequence::new("invoice")
//!     .scope(7) // the company
//!     .period(Period::of_year(today))
//!     .next(&mut tx)
//!     .await?;
//! let number = Sequence::format("INV-{yyyy}-{0000}", n, today); // "INV-2026-0042"
//! // …insert the invoice with `number` through `&mut tx`…
//! tx.commit().await?; // a rollback instead would hand `n` out again
//! # let _ = (number, NumberFormat::new("{0000}")); Ok(()) }
//! ```

use chrono::{Datelike, NaiveDate};

use super::{Db, DbError, Executor, Migration, Row, Transaction, sql};
use crate::Result;

pub(crate) const MIGRATION: Migration =
    crate::db::framework_migration!("sequence", "00010101000420_create_sequences_table");

/// How often a series starts over.
///
/// Stored as text in the `sequences` table: `""`, `"2026"` or `"2026-10"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Period {
    /// One counter for ever.
    None,
    /// A counter per calendar year.
    Year(i32),
    /// A counter per calendar month (year, month 1 to 12).
    Month(i32, u32),
}

impl Period {
    /// The year of `date`.
    pub fn of_year(date: NaiveDate) -> Self {
        Period::Year(date.year())
    }

    /// The month of `date`.
    pub fn of_month(date: NaiveDate) -> Self {
        Period::Month(date.year(), date.month())
    }

    /// The text stored in the table: `""`, `"2026"`, `"2026-10"`.
    pub fn key(&self) -> String {
        match self {
            Period::None => String::new(),
            Period::Year(y) => format!("{y:04}"),
            Period::Month(y, m) => format!("{y:04}-{m:02}"),
        }
    }
}

/// A numbering series: `series`, narrowed by `scope` (a company, a branch)
/// and `period` (a year, a month). Each combination counts from 1 on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a sequence does nothing until you ask it for a number"]
pub struct Sequence {
    series: String,
    scope: String,
    period: Period,
}

/// One row of the `sequences` table, from [`Sequence::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SequenceRow {
    /// The series name.
    pub series: String,
    /// Its scope (`""` when none).
    pub scope: String,
    /// Its period as stored (`""`, `"2026"`, `"2026-10"`).
    pub period: String,
    /// The last number issued.
    pub last: i64,
}

impl Sequence {
    /// A series, with no scope and no period (one counter for ever).
    pub fn new(series: impl Into<String>) -> Self {
        Sequence {
            series: series.into(),
            scope: String::new(),
            period: Period::None,
        }
    }

    /// Counts separately per `scope`: a company id, a branch code.
    pub fn scope(mut self, scope: impl ToString) -> Self {
        self.scope = scope.to_string();
        self
    }

    /// Starts over at 1 with each `period`.
    pub fn period(mut self, period: Period) -> Self {
        self.period = period;
        self
    }

    /// Takes the next number (1, 2, 3… for this series) in `tx`.
    ///
    /// The number belongs to the transaction: commit keeps it, rollback
    /// returns it, and concurrent callers queue on the row until then.
    /// There is deliberately no overload for a plain `&Db`: a number taken
    /// outside the transaction that stores the document could be lost, which
    /// is the gap this API exists to prevent (see [`next_allowing_gaps`]
    /// when gaps are fine).
    ///
    /// [`next_allowing_gaps`]: Sequence::next_allowing_gaps
    pub async fn next(&self, tx: &mut Transaction) -> Result<i64> {
        let last: i64 = sql(
            "INSERT INTO sequences (series, scope, period, last) VALUES (?, ?, ?, 1) \
             ON CONFLICT (series, scope, period) DO UPDATE SET last = sequences.last + 1 \
             RETURNING last",
        )
        .bind(self.series.as_str())
        .bind(self.scope.as_str())
        .bind(self.period.key())
        .scalar(tx)
        .await?;
        Ok(last)
    }

    /// Takes the next number in a transaction of its own, committed at once.
    ///
    /// For series where gaps are acceptable (order references, ticket
    /// numbers) and many inserts would otherwise queue on one row for the
    /// length of their transactions. A number taken here stays taken if the
    /// caller's own work later fails. For ids that need no order at all, use
    /// the primary key.
    pub async fn next_allowing_gaps(&self, db: &Db) -> Result<i64> {
        let mut tx = db.begin().await?;
        let n = self.next(&mut tx).await?;
        tx.commit().await?;
        Ok(n)
    }

    /// The number [`next`](Sequence::next) would give, without taking it
    /// (for previews). Another transaction may take it first.
    pub async fn peek<'c>(&self, db: impl Executor<'c>) -> Result<i64> {
        Ok(self.last(db).await? + 1)
    }

    /// The last number issued (0 when none was).
    pub async fn last<'c>(&self, db: impl Executor<'c>) -> Result<i64> {
        let last: Option<i64> =
            sql("SELECT last FROM sequences WHERE series = ? AND scope = ? AND period = ?")
                .bind(self.series.as_str())
                .bind(self.scope.as_str())
                .bind(self.period.key())
                .scalar_optional(db)
                .await?;
        Ok(last.unwrap_or(0))
    }

    /// Continues from an old system: sets the last number issued to `last`,
    /// so the next one is `last + 1`. Creates the series when it is new, and
    /// can also move an existing one (take care not to go backwards past
    /// numbers already used). Run it in the transaction that migrates the
    /// data, or on `&Db`.
    pub async fn set_start<'c>(&self, db: impl Executor<'c>, last: i64) -> Result<()> {
        sql(
            "INSERT INTO sequences (series, scope, period, last) VALUES (?, ?, ?, ?) \
             ON CONFLICT (series, scope, period) DO UPDATE SET last = excluded.last",
        )
        .bind(self.series.as_str())
        .bind(self.scope.as_str())
        .bind(self.period.key())
        .bind(last)
        .execute(db)
        .await?;
        Ok(())
    }

    /// Every series in the table, in name order (the `sequence:list` command).
    pub async fn list<'c>(db: impl Executor<'c>) -> Result<Vec<SequenceRow>> {
        let rows = sql("SELECT series, scope, period, last FROM sequences \
                        ORDER BY series, scope, period")
        .fetch_all(db)
        .await?;
        rows.iter()
            .map(|row: &Row| -> Result<SequenceRow, DbError> {
                Ok(SequenceRow {
                    series: row.try_get("series")?,
                    scope: row.try_get("scope")?,
                    period: row.try_get("period")?,
                    last: row.try_get("last")?,
                })
            })
            .collect::<Result<_, _>>()
            .map_err(Into::into)
    }

    /// Writes number `n` as `pattern` says, for the date `date`.
    ///
    /// Tokens: `{yyyy}` (year), `{yy}` (two digits), `{mm}` (month) and
    /// `{0000}` (the number zero-padded to as many digits as there are
    /// zeros); anything else is literal text.
    ///
    /// ```
    /// use renox::db::Sequence;
    /// let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    /// assert_eq!(Sequence::format("INV-{yyyy}-{0000}", 42, date), "INV-2026-0042");
    /// assert_eq!(Sequence::format("{yy}{mm}/{000}", 7, date), "2610/007");
    /// ```
    pub fn format(pattern: &str, n: i64, date: NaiveDate) -> String {
        NumberFormat::new(pattern).render(n, date)
    }
}

/// A number pattern with named tokens of its own, for patterns kept in
/// settings: `{prefix}-{yyyy}-{0000}` with `prefix` filled in by the app.
///
/// Values are inserted as they are and never read as tokens again, so a
/// prefix containing `{` or `}` is safe. A `{name}` with no value stays as
/// written.
///
/// ```
/// use renox::db::NumberFormat;
/// let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
/// let number = NumberFormat::new("{prefix}-{entity}-{yyyy}-{0000}")
///     .token("prefix", "ACM-INV")
///     .token("entity", "ACME")
///     .render(42, date);
/// assert_eq!(number, "ACM-INV-ACME-2026-0042");
/// ```
#[derive(Debug, Clone)]
pub struct NumberFormat {
    pattern: String,
    tokens: Vec<(String, String)>,
}

impl NumberFormat {
    /// A pattern (see [`Sequence::format`] for the built-in tokens).
    pub fn new(pattern: impl Into<String>) -> Self {
        NumberFormat {
            pattern: pattern.into(),
            tokens: Vec::new(),
        }
    }

    /// Gives `{name}` a value.
    pub fn token(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.tokens.push((name.into(), value.into()));
        self
    }

    /// Writes number `n` for `date`.
    pub fn render(&self, n: i64, date: NaiveDate) -> String {
        let mut out = String::with_capacity(self.pattern.len() + 8);
        let mut rest = self.pattern.as_str();
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let Some(close) = after.find('}') else {
                out.push_str(&rest[open..]);
                return out;
            };
            let name = &after[..close];
            match self.expand(name, n, date) {
                Some(text) => {
                    out.push_str(&text);
                    rest = &after[close + 1..];
                }
                None => {
                    out.push('{');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    fn expand(&self, name: &str, n: i64, date: NaiveDate) -> Option<String> {
        match name {
            "yyyy" => Some(format!("{:04}", date.year())),
            "yy" => Some(format!("{:02}", date.year().rem_euclid(100))),
            "mm" => Some(format!("{:02}", date.month())),
            zeros if !zeros.is_empty() && zeros.bytes().all(|b| b == b'0') => {
                Some(format!("{n:0width$}", width = zeros.len()))
            }
            other => self
                .tokens
                .iter()
                .rev()
                .find(|(name, _)| name == other)
                .map(|(_, value)| value.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()
    }

    #[test]
    fn formats_tokens() {
        assert_eq!(
            Sequence::format("INV-{yyyy}-{0000}", 42, date()),
            "INV-2026-0042"
        );
        assert_eq!(Sequence::format("{yy}{mm}-{00}", 5, date()), "2603-05");
        assert_eq!(Sequence::format("{0}", 12345, date()), "12345", "no cut");
        assert_eq!(Sequence::format("no tokens", 1, date()), "no tokens");
        assert_eq!(Sequence::format("{unknown}-{0", 1, date()), "{unknown}-{0");
        assert_eq!(Sequence::format("{{0000}", 1, date()), "{0001");
    }

    #[test]
    fn named_tokens_are_not_rescanned() {
        let text = NumberFormat::new("{p}-{0000}")
            .token("p", "{yyyy}")
            .render(3, date());
        assert_eq!(text, "{yyyy}-0003");
        let later = NumberFormat::new("{p}")
            .token("p", "a")
            .token("p", "b")
            .render(1, date());
        assert_eq!(later, "b", "the last value wins");
    }

    #[test]
    fn period_keys() {
        assert_eq!(Period::None.key(), "");
        assert_eq!(Period::Year(2026).key(), "2026");
        assert_eq!(Period::Month(2026, 3).key(), "2026-03");
        assert_eq!(Period::of_month(date()), Period::Month(2026, 3));
    }
}
