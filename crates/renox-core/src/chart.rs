//! Dashboards: numbers over time from the database ([`Trend`] over a
//! [`Period`], giving a [`Series`]), and the `chart(…)` template function
//! that draws them (line, area, bar, pie, doughnut, and scatter or bubble
//! charts of points) as plain HTML and SVG, with the UI kit's `stat`,
//! `widget` and `period_filter` around them.
//!
//! ```
//! # use renox::prelude::*;
//! use renox::chart::{Period, Trend};
//!
//! #[derive(Model, serde::Serialize, Default)]
//! struct Order { id: i64, total: i64, status: String, created_at: Option<renox::db::DateTime> }
//!
//! // `?period=30d` (7d, 30d, 90d, 12w, 12m, mtd, ytd, or
//! // `?period=custom&from=2026-09-01&to=2026-09-30`; 30 days without one).
//! async fn dashboard(State(state): State<AppState>, period: Period) -> Result<View> {
//!     let paid = || Order::where_eq("status", "paid");
//!     let sales = Trend::of(paid(), "created_at").over(period).sum(&state, "total").await?;
//!     let before = Trend::of(paid(), "created_at").over(period.previous()).sum(&state, "total").await?;
//!     let orders = Trend::of(Order::query(), "created_at").over(period).count(&state).await?;
//!     Ok(view("dashboard.html", context! {
//!         period,
//!         revenue => sales.total(),
//!         change => sales.change_from(&before), // percent, None without a base
//!         sales => sales.named("Sales"),
//!         orders,
//!     }))
//! }
//! ```
//!
//! ```text
//! {% from "renox/ui.html" import stats, stat, dashboard, widget, period_filter %}
//! {{ period_filter(period) }}
//! {% call stats(3) %}
//!   {{ stat("Revenue", revenue | money, delta=change, trend=sales.values) }}
//! {% endcall %}
//! {% call dashboard(2) %}
//!   {% call widget("Sales", span=2) %}{{ chart("area", sales, format="money") }}{% endcall %}
//! {% endcall %}
//! ```

use std::fmt::Write as _;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use minijinja::value::{Kwargs, Value, ValueKind};
use minijinja::{Error, ErrorKind, State};
use serde::{Deserialize, Serialize, Serializer};

use crate::db::{DateTime, Dialect, Model, Query};
use crate::timezone::Zone;
use crate::toast::escape;
use crate::view_filters::{format_money, format_number, money_divisor};
use crate::{AppState, Result};

/// How long a dashboard looks back: `7d`, `30d`, `90d` (any number of days
/// up to 366), `12w` (weeks up to 104), `12m` (months up to 36), `mtd` (this
/// month so far), `ytd` (this year so far), or a custom range of dates
/// (`2026-09-01..2026-09-30`, both days included, at most 1,096 days), in
/// `APP_TIMEZONE`.
///
/// As an extractor it reads `?period=`, or `?period=custom&from=2026-09-01&to=2026-09-30`
/// (what the kit's `period_filter` sends for a custom range); anything it
/// can't read (an unknown key, a date that isn't one, `from` after `to`, a
/// range over three years) gives the default, 30 days. It serializes as its
/// key (`"30d"`, `"2026-09-01..2026-09-30"`), which `period_filter` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    span: Span,
    back: u32,
    per: Option<Bucket>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Span {
    Days(u32),
    Weeks(u32),
    Months(u32),
    MonthToDate,
    YearToDate,
    /// The first day and the day after the last.
    Between(NaiveDate, NaiveDate),
}

/// The longest custom range, in days (three years and a leap day).
const MAX_RANGE_DAYS: i64 = 1096;

/// The step of a [`Series`]: one value per day, week or month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Bucket {
    /// Labels `2026-10-02`.
    Day,
    /// Labels `2026-09-28`: the Monday each week starts on (ISO 8601
    /// weeks, Monday to Sunday, in `APP_TIMEZONE`). The first and last
    /// weeks of a period may be cut short by it.
    Week,
    /// Labels `2026-10`.
    Month,
}

impl Default for Period {
    fn default() -> Self {
        Period::days(30)
    }
}

impl Period {
    fn of(span: Span) -> Self {
        Self {
            span,
            back: 0,
            per: None,
        }
    }

    /// The last `n` days, today included.
    pub fn days(n: u32) -> Self {
        Self::of(Span::Days(n.clamp(1, 366)))
    }

    /// The last `n` weeks, this one so far included: from the Monday `n - 1`
    /// weeks before this week's Monday, to today. Per week.
    pub fn weeks(n: u32) -> Self {
        Self::of(Span::Weeks(n.clamp(1, 104)))
    }

    /// The last `n` months, this one included.
    pub fn months(n: u32) -> Self {
        Self::of(Span::Months(n.clamp(1, 36)))
    }

    /// This month so far.
    pub fn month_to_date() -> Self {
        Self::of(Span::MonthToDate)
    }

    /// This year so far.
    pub fn year_to_date() -> Self {
        Self::of(Span::YearToDate)
    }

    /// From `from` to `to`, both days included (local days in
    /// `APP_TIMEZONE`). `None` when `from` is after `to` or the range is
    /// longer than 1,096 days (three years).
    pub fn between(from: NaiveDate, to: NaiveDate) -> Option<Self> {
        let end = to.succ_opt()?;
        let days = (end - from).num_days();
        (1..=MAX_RANGE_DAYS)
            .contains(&days)
            .then(|| Self::of(Span::Between(from, end)))
    }

    /// The same period, but per `bucket` instead of the step it picks
    /// itself ([`bucket`](Period::bucket)): `Period::days(90).per(Bucket::Week)`.
    pub fn per(mut self, bucket: Bucket) -> Self {
        self.per = Some(bucket);
        self
    }

    /// `"7d"`, `"12w"`, `"12m"`, `"mtd"`, `"ytd"` or `"2026-09-01..2026-09-30"`;
    /// `None` for anything else.
    pub fn parse(key: &str) -> Option<Self> {
        let key = key.trim().to_ascii_lowercase();
        match key.as_str() {
            "mtd" => return Some(Self::month_to_date()),
            "ytd" => return Some(Self::year_to_date()),
            _ => {}
        }
        if let Some((from, to)) = key.split_once("..") {
            return Self::between(date(from)?, date(to)?);
        }
        let (number, unit) = key.split_at(key.len().checked_sub(1)?);
        let n: u32 = number.parse().ok()?;
        match unit {
            "d" if (1..=366).contains(&n) => Some(Self::days(n)),
            "w" if (1..=104).contains(&n) => Some(Self::weeks(n)),
            "m" if (1..=36).contains(&n) => Some(Self::months(n)),
            _ => None,
        }
    }

    /// The period a query string asks for: `period=7d`, or
    /// `period=custom&from=…&to=…`.
    fn from_query(query: &str) -> Option<Self> {
        let mut period = None;
        let (mut from, mut to) = (None, None);
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            match &*key {
                "period" if period.is_none() => period = Some(value.into_owned()),
                "from" if from.is_none() => from = Some(value.into_owned()),
                "to" if to.is_none() => to = Some(value.into_owned()),
                _ => {}
            }
        }
        let period = period?;
        if period.trim().eq_ignore_ascii_case("custom") {
            Self::between(date(from.as_deref()?)?, date(to.as_deref()?)?)
        } else {
            Self::parse(&period)
        }
    }

    /// The key it parses from: `"30d"`, or `"2026-09-01..2026-09-30"` for a
    /// custom range.
    pub fn key(&self) -> String {
        match self.span {
            Span::Days(n) => format!("{n}d"),
            Span::Weeks(n) => format!("{n}w"),
            Span::Months(n) => format!("{n}m"),
            Span::MonthToDate => "mtd".into(),
            Span::YearToDate => "ytd".into(),
            Span::Between(start, end) => format!(
                "{}..{}",
                start.format("%Y-%m-%d"),
                (end - Duration::days(1)).format("%Y-%m-%d")
            ),
        }
    }

    /// Whether it is a custom range of dates ([`between`](Period::between)).
    pub fn is_custom(&self) -> bool {
        matches!(self.span, Span::Between(..))
    }

    /// The period just before, as long: the 30 days before the last 30, last
    /// month to the same day, last year to the same day, as many days just
    /// before a custom range. For comparisons ([`Series::change_from`]).
    pub fn previous(self) -> Self {
        Self {
            back: self.back + 1,
            ..self
        }
    }

    /// Per day up to 92 days (and month to date), per week for `12w`-style
    /// periods and custom ranges up to 26 weeks, else per month; or what
    /// [`per`](Period::per) set.
    pub fn bucket(&self) -> Bucket {
        if let Some(bucket) = self.per {
            return bucket;
        }
        match self.span {
            Span::Days(n) if n <= 92 => Bucket::Day,
            Span::MonthToDate => Bucket::Day,
            Span::Weeks(_) => Bucket::Week,
            Span::Between(start, end) => match (end - start).num_days() {
                ..=92 => Bucket::Day,
                93..=182 => Bucket::Week,
                _ => Bucket::Month,
            },
            _ => Bucket::Month,
        }
    }

    /// The first local day and the day after the last, in `zone`'s calendar.
    fn days_in(&self, zone: Zone) -> (NaiveDate, NaiveDate) {
        let today = zone.local(crate::clock::unix_secs()).date();
        let back = self.back;
        match self.span {
            Span::Days(n) => {
                let end = today + Duration::days(1) - Duration::days(i64::from(n * back));
                (end - Duration::days(i64::from(n)), end)
            }
            Span::Weeks(n) => {
                let shift = Duration::days(7 * i64::from(n * back));
                let start = monday_of(today) - Duration::days(7 * i64::from(n - 1));
                (start - shift, today + Duration::days(1) - shift)
            }
            Span::Months(n) => {
                let this_month = first_of_month(today);
                let end = add_months(this_month, 1 - (n * back) as i32);
                (add_months(end, -(n as i32)), end)
            }
            Span::MonthToDate => {
                let start = add_months(first_of_month(today), -(back as i32));
                let length = i64::from(today.day());
                let next = add_months(start, 1);
                (start, (start + Duration::days(length)).min(next))
            }
            Span::YearToDate => {
                let year = today.year() - back as i32;
                let start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(today);
                let length = i64::from(today.ordinal());
                let next = NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap_or(today);
                (start, (start + Duration::days(length)).min(next))
            }
            Span::Between(start, end) => {
                let shift = (end - start) * back as i32;
                (start - shift, end - shift)
            }
        }
    }

    /// The moments the period starts at and ends before, in `zone`.
    pub fn range(&self, zone: Zone) -> (DateTime, DateTime) {
        let (start, end) = self.days_in(zone);
        (moment(zone, start), moment(zone, end))
    }

    /// The labels of its buckets, in order: `2026-10-02` per day, the
    /// week's Monday (`2026-09-28`) per week, `2026-10` per month.
    pub fn labels(&self, zone: Zone) -> Vec<String> {
        let (start, end) = self.days_in(zone);
        let mut labels = Vec::new();
        match self.bucket() {
            Bucket::Day => {
                let mut day = start;
                while day < end && labels.len() < 400 {
                    labels.push(day.format("%Y-%m-%d").to_string());
                    day += Duration::days(1);
                }
            }
            Bucket::Week => {
                let mut week = monday_of(start);
                while week < end && labels.len() < 400 {
                    labels.push(week.format("%Y-%m-%d").to_string());
                    week += Duration::days(7);
                }
            }
            Bucket::Month => {
                let mut month = first_of_month(start);
                while month < end && labels.len() < 400 {
                    labels.push(month.format("%Y-%m").to_string());
                    month = add_months(month, 1);
                }
            }
        }
        labels
    }
}

impl Serialize for Period {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.key())
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Period {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let query = parts.uri.query().unwrap_or_default();
        Ok(Period::from_query(query).unwrap_or_default())
    }
}

/// `2026-09-01`, strictly.
fn date(text: &str) -> Option<NaiveDate> {
    let text = text.trim();
    (text.len() == 10)
        .then(|| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
        .flatten()
}

fn first_of_month(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

/// The Monday of `day`'s ISO week.
fn monday_of(day: NaiveDate) -> NaiveDate {
    day - Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

fn add_months(day: NaiveDate, months: i32) -> NaiveDate {
    let total = day.year() * 12 + day.month0() as i32 + months;
    NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1).unwrap_or(day)
}

/// Local midnight of `day` in `zone`, as a moment.
fn moment(zone: Zone, day: NaiveDate) -> DateTime {
    let local = day.and_time(NaiveTime::MIN);
    let unix = zone
        .resolve(local)
        .unwrap_or_else(|| local.and_utc().timestamp());
    DateTime::from_timestamp(unix, 0).unwrap_or_default()
}

/// Values over time: one per label (`2026-10-02` or `2026-10`), buckets
/// without rows at 0. Serialized as `{name, labels, values}` for the
/// `chart(…)` template function and `stat(trend=…)`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Series {
    /// The series' name in a legend (`named`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// One label per value.
    pub labels: Vec<String>,
    /// The values.
    pub values: Vec<f64>,
}

impl Series {
    /// A series from labels and values (as many of each).
    pub fn new(labels: Vec<String>, values: Vec<f64>) -> Self {
        Self {
            name: None,
            labels,
            values,
        }
    }

    /// Names it, for a chart's legend.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The sum of the values.
    pub fn total(&self) -> f64 {
        self.values.iter().sum()
    }

    /// The change from `before`'s total, in percent (`12.5` for +12.5%);
    /// `None` when `before` adds up to 0.
    pub fn change_from(&self, before: &Series) -> Option<f64> {
        let base = before.total();
        (base != 0.0).then(|| (self.total() - base) / base.abs() * 100.0)
    }
}

/// A [`Series`] from a model's rows: how many (`count`), or the `sum` or
/// `average` of a column, per day, week or month of a [`Period`] (its
/// [`bucket`](Period::bucket)), by the moment in `column` (a `DateTime`
/// column such as `created_at`). The query's conditions apply
/// (`Order::where_eq("status", "paid")`); days are cut in `APP_TIMEZONE`, at
/// its offset at the end of the period, and weeks run Monday to Sunday in
/// those local days (labelled by their Monday).
pub struct Trend<M> {
    query: Query<M>,
    column: String,
    period: Period,
}

impl<M: Model> Trend<M> {
    /// The rows of `query`, placed in time by `column`; 30 days unless
    /// [`over`](Trend::over) says otherwise.
    pub fn of(query: Query<M>, column: &str) -> Self {
        Self {
            query,
            column: column.to_owned(),
            period: Period::default(),
        }
    }

    /// Over `period`.
    pub fn over(mut self, period: Period) -> Self {
        self.period = period;
        self
    }

    /// How many rows per bucket.
    pub async fn count(self, state: &AppState) -> Result<Series> {
        self.run(state, "COUNT(*)".to_owned()).await
    }

    /// The sum of `column` per bucket.
    pub async fn sum(self, state: &AppState, column: &str) -> Result<Series> {
        let column = checked::<M>(column)?;
        self.run(state, format!("SUM({column})")).await
    }

    /// The average of `column` per bucket (0 where there are no rows).
    pub async fn average(self, state: &AppState, column: &str) -> Result<Series> {
        let column = checked::<M>(column)?;
        self.run(state, format!("AVG({column})")).await
    }

    async fn run(self, state: &AppState, aggregate: String) -> Result<Series> {
        let zone: Zone = state.config.timezone;
        let column = checked::<M>(&self.column)?;
        let column = column.as_str();
        let (start, end) = self.period.range(zone);
        // Days are cut at the zone's offset at the end of the period.
        let minutes = zone.offset_at(end.timestamp()) / 60;
        let step = self.period.bucket();
        // The moment moved to local time, then cut to its day, the Monday of
        // its week (SQLite: on to the next Sunday, back six days), or month.
        let bucket = move |dialect: Dialect| match (dialect, step) {
            (Dialect::Sqlite, Bucket::Week) => {
                format!("date({column}, '{minutes:+} minutes', 'weekday 0', '-6 days')")
            }
            (Dialect::Sqlite, step) => format!(
                "strftime('{}', {column}, '{minutes:+} minutes')",
                if step == Bucket::Month {
                    "%Y-%m"
                } else {
                    "%Y-%m-%d"
                }
            ),
            (Dialect::Postgres, Bucket::Week) => format!(
                "to_char(date_trunc('week', ({column} AT TIME ZONE 'UTC') + interval '{minutes} minutes'), 'YYYY-MM-DD')"
            ),
            (Dialect::Postgres, step) => format!(
                "to_char(({column} AT TIME ZONE 'UTC') + interval '{minutes} minutes', '{}')",
                if step == Bucket::Month {
                    "YYYY-MM"
                } else {
                    "YYYY-MM-DD"
                }
            ),
        };
        let condition = format!("{column} >= ? AND {column} < ?");
        let rows = self
            .query
            .where_raw(&condition, [start, end])
            .buckets(&state.db, &bucket, &aggregate)
            .await?;
        let found: std::collections::HashMap<String, f64> = rows
            .into_iter()
            .map(|(label, value)| (label, value.unwrap_or(0.0)))
            .collect();
        let labels = self.period.labels(zone);
        let values = labels
            .iter()
            .map(|label| found.get(label).copied().unwrap_or(0.0))
            .collect();
        Ok(Series::new(labels, values))
    }
}

/// `column`, quoted, if the model has it.
fn checked<M: Model>(column: &str) -> Result<String> {
    if M::COLUMNS.contains(&column) {
        Ok(format!("\"{column}\""))
    } else {
        Err(anyhow::anyhow!("`{}` has no column `{column}`", M::TABLE).into())
    }
}

// ---------------------------------------------------------------------------
// The `chart(…)` template function.

/// One line, area or set of bars.
struct Line {
    name: String,
    values: Vec<Option<f64>>,
}

/// What a chart draws.
struct Data {
    labels: Vec<String>,
    series: Vec<Line>,
}

fn number_of(value: &Value) -> Option<f64> {
    if value.is_none() || value.is_undefined() {
        return None;
    }
    if let Ok(i) = i64::try_from(value.clone()) {
        return Some(i as f64);
    }
    if let Some(text) = value.as_str() {
        return text.trim().parse().ok();
    }
    f64::try_from(value.clone()).ok()
}

fn strings(value: &Value) -> Vec<String> {
    match value.try_iter() {
        Ok(items) if value.kind() == ValueKind::Seq => items
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn numbers(value: &Value) -> Vec<Option<f64>> {
    match value.try_iter() {
        Ok(items) if value.kind() == ValueKind::Seq => items.map(|v| number_of(&v)).collect(),
        _ => Vec::new(),
    }
}

fn attr(value: &Value, key: &str) -> Option<Value> {
    value
        .get_attr(key)
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_none())
}

/// A series from `{name, values}` (or a [`Series`]), or a list of numbers.
fn line_of(value: &Value, fallback: &str) -> Line {
    if value.kind() == ValueKind::Map {
        Line {
            name: attr(value, "name")
                .and_then(|n| n.as_str().map(str::to_owned))
                .unwrap_or_else(|| fallback.to_owned()),
            values: attr(value, "values")
                .map(|v| numbers(&v))
                .unwrap_or_default(),
        }
    } else {
        Line {
            name: fallback.to_owned(),
            values: numbers(value),
        }
    }
}

fn read_data(
    data: Option<Value>,
    kwargs: &Kwargs,
    title: &str,
) -> std::result::Result<Data, Error> {
    let labels: Option<Value> = kwargs.get("labels")?;
    let series: Option<Value> = kwargs.get("series")?;
    let values: Option<Value> = kwargs.get("values")?;
    let name: Option<String> = kwargs.get("name")?;
    let fallback = name.clone().unwrap_or_else(|| title.to_owned());
    let mut out = Data {
        labels: labels.as_ref().map(strings).unwrap_or_default(),
        series: Vec::new(),
    };
    let list_of_series = |value: &Value, out: &mut Data| {
        if let Ok(items) = value.try_iter() {
            for (i, item) in items.enumerate() {
                out.series
                    .push(line_of(&item, &format!("{} {}", fallback, i + 1)));
            }
        }
    };
    match data {
        Some(data) if data.kind() == ValueKind::Map => {
            if out.labels.is_empty()
                && let Some(labels) = attr(&data, "labels")
            {
                out.labels = strings(&labels);
            }
            match attr(&data, "series") {
                Some(series) => list_of_series(&series, &mut out),
                None => out.series.push(line_of(&data, &fallback)),
            }
        }
        Some(data) if data.kind() == ValueKind::Seq => {
            let first_is_map = data
                .try_iter()
                .ok()
                .and_then(|mut items| items.next())
                .is_some_and(|v| v.kind() == ValueKind::Map);
            if first_is_map {
                list_of_series(&data, &mut out);
            } else {
                out.series.push(line_of(&data, &fallback));
            }
        }
        _ => {}
    }
    if let Some(series) = series {
        list_of_series(&series, &mut out);
    }
    if let Some(values) = values {
        out.series.push(Line {
            name: fallback.clone(),
            values: numbers(&values),
        });
    }
    if let Some(name) = name
        && out.series.len() == 1
    {
        out.series[0].name = name;
    }
    let longest = out.series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    if out.labels.len() < longest {
        for i in out.labels.len()..longest {
            out.labels.push((i + 1).to_string());
        }
    }
    let n = out.labels.len();
    for line in &mut out.series {
        line.values.resize(n, None);
    }
    Ok(out)
}

/// The page's language, for separators and month names.
fn locale(state: &State) -> String {
    state
        .lookup("app")
        .and_then(|app| app.get_attr("locale").ok())
        .and_then(|l| l.as_str().map(str::to_owned))
        .unwrap_or_else(|| "en".into())
}

fn text(state: &State, key: &str, fallback_locale: &str) -> String {
    state
        .lookup("t")
        .and_then(|t| t.call(state, &[Value::from(key)]).ok())
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| crate::i18n::builtin_text(fallback_locale, key))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `2026-10-02` → `Oct 2`, `2026-10` → `Oct 2026`, or with `format`
/// (chrono's codes, e.g. `%d/%m`); other labels as they are.
fn display_label(label: &str, format: Option<&str>) -> String {
    if let Ok(day) = NaiveDate::parse_from_str(label, "%Y-%m-%d") {
        return match format {
            Some(format) => day.format(format).to_string(),
            None => format!("{} {}", MONTHS[day.month0() as usize], day.day()),
        };
    }
    if let Ok(month) = NaiveDate::parse_from_str(&format!("{label}-01"), "%Y-%m-%d") {
        return match format {
            Some(format) => month.format(format).to_string(),
            None => format!("{} {}", MONTHS[month.month0() as usize], month.year()),
        };
    }
    label.to_owned()
}

/// How values read in tooltips and the table.
struct Formatter {
    format: String,
    decimals: Option<u32>,
    currency: String,
    locale: String,
    /// What money is divided by: data is in the currency's smallest unit,
    /// as for the `money` filter (100 for `USD`, `divide_by` to change it).
    divisor: f64,
}

impl Formatter {
    /// `value` in the unit it is shown in (money in whole units).
    fn shown(&self, value: f64) -> f64 {
        if self.format == "money" {
            value / self.divisor
        } else {
            value
        }
    }

    fn full(&self, value: f64) -> String {
        let value = self.shown(value);
        match self.format.as_str() {
            "money" => format_money(value, &self.currency, self.decimals, &self.locale),
            "percent" => format!(
                "{}%",
                format_number(value, self.decimals.unwrap_or(1), &self.locale)
            ),
            _ => format_number(value, self.decimals.unwrap_or(0), &self.locale),
        }
    }

    /// Axis ticks: `12.5K` (`12,5K` where the locale writes a decimal
    /// comma), plain under 10,000.
    fn tick(&self, value: f64) -> String {
        let value = self.shown(value);
        let units: [(f64, &str); 4] = [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")];
        let suffix = if self.format == "percent" { "%" } else { "" };
        if value.abs() >= 10_000.0 {
            for (size, unit) in units {
                if value.abs() >= size {
                    let scaled = value / size;
                    let decimals = if scaled.fract().abs() < 1e-9 { 0 } else { 1 };
                    return format!(
                        "{}{unit}{suffix}",
                        format_number(scaled, decimals, &self.locale)
                    );
                }
            }
        }
        let decimals = if value.fract().abs() < 1e-9 { 0 } else { 1 };
        format!("{}{suffix}", format_number(value, decimals, &self.locale))
    }
}

/// Clean ticks from `lo` to `hi` (0 always included): about four steps of
/// 1, 2, 2.5 or 5 × 10ⁿ.
fn scale(lo: f64, hi: f64) -> (f64, f64, f64) {
    nice(lo.min(0.0), hi.max(0.0))
}

/// Clean ticks covering `lo` to `hi`, without forcing 0 in.
fn nice(mut lo: f64, mut hi: f64) -> (f64, f64, f64) {
    if (hi - lo).abs() < f64::EPSILON {
        if lo == 0.0 {
            hi = lo + 1.0;
        } else {
            let pad = lo.abs() / 10.0;
            (lo, hi) = (lo - pad, hi + pad);
        }
    }
    let raw = (hi - lo) / 4.0;
    let magnitude = 10f64.powf(raw.log10().floor());
    let normal = raw / magnitude;
    let nice = if normal <= 1.0 {
        1.0
    } else if normal <= 2.0 {
        2.0
    } else if normal <= 2.5 {
        2.5
    } else if normal <= 5.0 {
        5.0
    } else {
        10.0
    };
    let step = nice * magnitude;
    lo = (lo / step).floor() * step;
    hi = (hi / step).ceil() * step;
    (lo, hi, step)
}

fn slot(i: usize) -> String {
    if i < 6 {
        format!("rx-series-{}", i + 1)
    } else {
        "rx-series-other".into()
    }
}

fn pct(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    format!("{rounded}")
}

/// `chart(kind, data, …)`: see docs/ui.md "Charts".
pub(crate) fn chart(
    currency: String,
) -> impl Fn(&State, String, Option<Value>, Kwargs) -> std::result::Result<Value, Error>
+ Send
+ Sync
+ 'static {
    move |state: &State, kind: String, data: Option<Value>, kwargs: Kwargs| {
        // `data=` is the same as the second positional argument (`<rx-chart :data="…">`).
        let data = match data {
            Some(d) => Some(d),
            None => kwargs.get::<Option<Value>>("data")?,
        };
        let title: Option<String> = kwargs.get("title")?;
        let height: Option<u32> = kwargs.get("height")?;
        let format: Option<String> = kwargs.get("format")?;
        let decimals: Option<u32> = kwargs.get("decimals")?;
        let currency_kw: Option<String> = kwargs.get("currency")?;
        let divide_by: Option<f64> = kwargs.get("divide_by")?;
        let stacked: Option<bool> = kwargs.get("stacked")?;
        let legend: Option<bool> = kwargs.get("legend")?;
        let table: Option<bool> = kwargs.get("table")?;
        let x_format: Option<String> = kwargs.get("x_format")?;
        let x_title: Option<String> = kwargs.get("x_title")?;
        let y_title: Option<String> = kwargs.get("y_title")?;
        let id: Option<String> = kwargs.get("id")?;
        let locale = locale(state);
        let title = title.unwrap_or_default();
        let currency = currency_kw
            .map(|c| c.trim().to_ascii_uppercase())
            .unwrap_or_else(|| currency.clone());
        let divisor = money_divisor(&currency, divide_by);
        // Money keeps its currency's decimals unless `decimals` says
        // otherwise; other formats take what the data needs.
        let formatter = |format: Option<String>, given: Option<u32>, auto: Option<u32>| {
            let format = format.unwrap_or_else(|| "number".into());
            Formatter {
                decimals: if format == "money" {
                    given
                } else {
                    given.or(auto)
                },
                format,
                currency: currency.clone(),
                locale: locale.clone(),
                divisor,
            }
        };
        let mut options = Options {
            kind: kind.clone(),
            title: title.clone(),
            height: height.unwrap_or(240).clamp(80, 800),
            stacked: stacked.unwrap_or(false),
            legend: legend.unwrap_or(true),
            table: table.unwrap_or(true),
            labels: Vec::new(),
            id,
            show_data: text(state, "ui.chart.show_data", &locale),
            other: text(state, "ui.chart.other", &locale),
            series_head: text(state, "ui.chart.series", &locale),
            axes: text(state, "ui.chart.axes", &locale),
            x_title,
            y_title,
        };
        let html = match kind.as_str() {
            "scatter" | "bubble" => {
                let size_format: Option<String> = kwargs.get("size_format")?;
                let size_title: Option<String> = kwargs.get("size_title")?;
                let clouds = read_points(data, &kwargs, &title)?;
                kwargs.assert_all_used()?;
                let all = || clouds.iter().flat_map(|c| c.points.iter());
                let formats = Formats {
                    x: formatter(x_format, None, auto_decimals(all().map(|p| p.x))),
                    y: formatter(format, decimals, auto_decimals(all().map(|p| p.y))),
                    size: formatter(
                        size_format,
                        None,
                        auto_decimals(all().filter_map(|p| p.size)),
                    ),
                    size_title: size_title.unwrap_or_else(|| text(state, "ui.chart.size", &locale)),
                };
                render_points(&clouds, &options, &formats)
            }
            "heatmap" => {
                let heat = read_heat(data, &kwargs)?;
                kwargs.assert_all_used()?;
                let auto = match format.as_deref() {
                    None | Some("number") => auto_decimals(heat.cells.iter().map(|c| c.value)),
                    _ => None,
                };
                let formatter = formatter(format, decimals, auto);
                let less = text(state, "ui.chart.less", &locale);
                let more = text(state, "ui.chart.more", &locale);
                render_heat(&heat, &options, &formatter, &less, &more)
            }
            "line" | "area" | "bar" | "pie" | "doughnut" => {
                let data = read_data(data, &kwargs, &title)?;
                kwargs.assert_all_used()?;
                options.labels = data
                    .labels
                    .iter()
                    .map(|l| display_label(l, x_format.as_deref()))
                    .collect();
                // Plain numbers get as many decimals as the data needs, as
                // on scatter charts; money and percent keep their own.
                let auto = match format.as_deref() {
                    None | Some("number") => auto_decimals(
                        data.series
                            .iter()
                            .flat_map(|s| s.values.iter().flatten().copied()),
                    ),
                    _ => None,
                };
                let formatter = formatter(format, decimals, auto);
                if kind == "pie" || kind == "doughnut" {
                    render_pie(&data, &options, &formatter)
                } else {
                    render_xy(&data, &options, &formatter)
                }
            }
            other => {
                return Err(Error::new(
                    ErrorKind::InvalidOperation,
                    format!(
                        "chart: unknown kind `{other}` (line, area, bar, pie, doughnut, scatter, bubble or heatmap)"
                    ),
                ));
            }
        };
        Ok(Value::from_safe_string(html))
    }
}

struct Options {
    kind: String,
    title: String,
    height: u32,
    stacked: bool,
    legend: bool,
    table: bool,
    labels: Vec<String>,
    id: Option<String>,
    show_data: String,
    other: String,
    /// The data table's heading over series names.
    series_head: String,
    /// `:y by :x`, for a scatter chart's accessible name.
    axes: String,
    /// The axes' titles, shown along them.
    x_title: Option<String>,
    y_title: Option<String>,
}

/// The data the page's script reads for tooltips (and a table reads too).
#[derive(Serialize)]
struct Hover<'a> {
    kind: &'a str,
    labels: &'a [String],
    series: Vec<HoverSeries>,
}

#[derive(Serialize)]
struct HoverSeries {
    name: String,
    slot: String,
    values: Vec<Option<String>>,
}

fn open_figure(out: &mut String, options: &Options, hover: &Hover) {
    let json = serde_json::to_string(hover).unwrap_or_default();
    let _ = write!(
        out,
        r#"<figure class="rx-chart rx-chart--{kind}"{id} data-rx-chart="{json}">"#,
        kind = escape(&options.kind),
        id = options
            .id
            .as_deref()
            .map(|id| format!(r#" id="{}""#, escape(id)))
            .unwrap_or_default(),
        json = escape(&json),
    );
    if !options.title.is_empty() {
        let _ = write!(
            out,
            r#"<figcaption class="rx-visually-hidden">{}</figcaption>"#,
            escape(&options.title)
        );
    }
}

fn legend(out: &mut String, names: &[(usize, &str)], key: &str) {
    out.push_str(r#"<ul class="rx-chart__legend" role="list">"#);
    for (i, name) in names {
        let _ = write!(
            out,
            r#"<li><span class="rx-chart__key rx-chart__key--{key} {slot}" aria-hidden="true"></span>{name}</li>"#,
            slot = slot(*i),
            name = escape(name),
        );
    }
    out.push_str("</ul>");
}

fn table(out: &mut String, options: &Options, heads: &[String], rows: &[(String, Vec<String>)]) {
    if !options.table {
        return;
    }
    let _ = write!(
        out,
        r#"<details class="rx-chart__table"><summary>{}</summary><div class="rx-table-wrap"><table class="rx-table"><thead><tr><th scope="col"></th>"#,
        escape(&options.show_data)
    );
    for head in heads {
        let _ = write!(
            out,
            r#"<th scope="col" class="rx-num">{}</th>"#,
            escape(head)
        );
    }
    out.push_str("</tr></thead><tbody>");
    for (label, cells) in rows {
        let _ = write!(out, r#"<tr><th scope="row">{}</th>"#, escape(label));
        for cell in cells {
            let _ = write!(out, r#"<td class="rx-num">{}</td>"#, escape(cell));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div></details>");
}

fn render_xy(data: &Data, options: &Options, formatter: &Formatter) -> String {
    let n = data.labels.len();
    let bar = options.kind == "bar";
    let stacked = bar && options.stacked;
    // The value range: per label sums for stacked bars.
    let (mut lo, mut hi) = (0.0f64, 0.0f64);
    for i in 0..n {
        let (mut up, mut down) = (0.0, 0.0);
        for line in &data.series {
            if let Some(v) = line.values[i] {
                if stacked {
                    if v >= 0.0 { up += v } else { down += v }
                } else {
                    hi = hi.max(v);
                    lo = lo.min(v);
                }
            }
        }
        hi = hi.max(up);
        lo = lo.min(down);
    }
    let (lo, hi, step) = scale(lo, hi);
    let y = |v: f64| (v - lo) / (hi - lo) * 100.0;
    let zero = y(0.0);

    let hover = Hover {
        kind: &options.kind,
        labels: &options.labels,
        series: data
            .series
            .iter()
            .enumerate()
            .map(|(i, line)| HoverSeries {
                name: line.name.clone(),
                slot: slot(i),
                values: line
                    .values
                    .iter()
                    .map(|v| v.map(|v| formatter.full(v)))
                    .collect(),
            })
            .collect(),
    };
    let mut out = String::new();
    open_figure(&mut out, options, &hover);
    if options.legend && data.series.len() > 1 {
        let names: Vec<(usize, &str)> = data
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.name.as_str()))
            .collect();
        legend(&mut out, &names, if bar { "box" } else { "line" });
    }
    y_title(&mut out, options);
    let _ = write!(
        out,
        r#"<div class="rx-chart__frame" style="--rx-chart-h: {}px"><div class="rx-chart__y" aria-hidden="true">"#,
        options.height
    );
    let mut ticks = Vec::new();
    let mut tick = lo;
    while tick <= hi + step / 2.0 && ticks.len() < 12 {
        ticks.push(tick);
        tick += step;
    }
    for t in &ticks {
        let _ = write!(
            out,
            r#"<span style="bottom: {}%">{}</span>"#,
            pct(y(*t)),
            escape(&formatter.tick(*t))
        );
    }
    let label = if options.title.is_empty() {
        data.series
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        options.title.clone()
    };
    let _ = write!(
        out,
        r#"</div><div class="rx-chart__plot" tabindex="0" role="img" aria-label="{}"><div class="rx-chart__grid" aria-hidden="true">"#,
        escape(&label)
    );
    for t in &ticks {
        let base = if t.abs() < step / 1e6 {
            " rx-chart__rule--base"
        } else {
            ""
        };
        let _ = write!(
            out,
            r#"<span class="rx-chart__rule{base}" style="bottom: {}%"></span>"#,
            pct(y(*t))
        );
    }
    out.push_str("</div>");

    if bar {
        out.push_str(r#"<div class="rx-chart__bars" aria-hidden="true">"#);
        for i in 0..n {
            let _ = write!(out, r#"<div class="rx-chart__band" data-index="{i}">"#);
            if stacked {
                out.push_str(r#"<div class="rx-chart__column">"#);
                let (mut up, mut down) = (0.0, 0.0);
                let tops: Vec<usize> = {
                    // The outermost segment on each side gets the rounded end.
                    let mut last_up = None;
                    let mut last_down = None;
                    for (s, line) in data.series.iter().enumerate() {
                        match line.values[i] {
                            Some(v) if v > 0.0 => last_up = Some(s),
                            Some(v) if v < 0.0 => last_down = Some(s),
                            _ => {}
                        }
                    }
                    last_up.into_iter().chain(last_down).collect()
                };
                for (s, line) in data.series.iter().enumerate() {
                    let Some(v) = line.values[i].filter(|v| *v != 0.0) else {
                        continue;
                    };
                    let (from, to) = if v > 0.0 {
                        let from = up;
                        up += v;
                        (from, up)
                    } else {
                        let from = down;
                        down += v;
                        (down, from)
                    };
                    let classes = format!(
                        "rx-chart__bar {}{}{}",
                        slot(s),
                        if v < 0.0 { " rx-chart__bar--neg" } else { "" },
                        if tops.contains(&s) {
                            " rx-chart__bar--end"
                        } else {
                            ""
                        },
                    );
                    let _ = write!(
                        out,
                        r#"<span class="{classes}" style="bottom: {}%; height: {}%"></span>"#,
                        pct(y(from)),
                        pct(y(to) - y(from)),
                    );
                }
                out.push_str("</div>");
            } else {
                for (s, line) in data.series.iter().enumerate() {
                    out.push_str(r#"<div class="rx-chart__column">"#);
                    if let Some(v) = line.values[i].filter(|v| *v != 0.0) {
                        let (from, to) = if v > 0.0 { (0.0, v) } else { (v, 0.0) };
                        let _ = write!(
                            out,
                            r#"<span class="rx-chart__bar rx-chart__bar--end {}{}" style="bottom: {}%; height: {}%"></span>"#,
                            slot(s),
                            if v < 0.0 { " rx-chart__bar--neg" } else { "" },
                            pct(y(from)),
                            pct(y(to) - y(from)),
                        );
                    }
                    out.push_str("</div>");
                }
            }
            out.push_str("</div>");
        }
        out.push_str("</div>");
    } else {
        let x = |i: usize| {
            if n > 1 {
                i as f64 / (n - 1) as f64 * 1000.0
            } else {
                500.0
            }
        };
        let sy = |v: f64| 1000.0 - y(v) * 10.0;
        out.push_str(r#"<svg class="rx-chart__svg" viewBox="0 0 1000 1000" preserveAspectRatio="none" aria-hidden="true" focusable="false">"#);
        for (s, line) in data.series.iter().enumerate() {
            // Runs of values; a missing one breaks the line.
            let mut runs: Vec<Vec<(f64, f64)>> = vec![Vec::new()];
            for (i, value) in line.values.iter().enumerate() {
                match (value, runs.last_mut()) {
                    (Some(v), Some(run)) => run.push((x(i), sy(*v))),
                    _ => runs.push(Vec::new()),
                }
            }
            for run in runs.iter().filter(|r| !r.is_empty()) {
                let path: String = run
                    .iter()
                    .enumerate()
                    .map(|(k, (px, py))| {
                        format!("{}{:.1},{:.1}", if k == 0 { "M" } else { "L" }, px, py)
                    })
                    .collect();
                if options.kind == "area" {
                    let base = 1000.0 - zero * 10.0;
                    let _ = write!(
                        out,
                        r#"<path class="rx-chart__area {}" d="{path}L{:.1},{base:.1}L{:.1},{base:.1}Z"/>"#,
                        slot(s),
                        run.last().map(|p| p.0).unwrap_or(0.0),
                        run.first().map(|p| p.0).unwrap_or(0.0),
                    );
                }
                let _ = write!(
                    out,
                    r#"<path class="rx-chart__line {}" d="{path}" vector-effect="non-scaling-stroke"/>"#,
                    slot(s)
                );
            }
        }
        out.push_str("</svg>");
        // The last point of each line, as a dot that stays round.
        for (s, line) in data.series.iter().enumerate() {
            if let Some((i, v)) = line
                .values
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, v)| v.map(|v| (i, v)))
            {
                let _ = write!(
                    out,
                    r#"<span class="rx-chart__dot {}" style="left: {}%; bottom: {}%" aria-hidden="true"></span>"#,
                    slot(s),
                    pct(x(i) / 10.0),
                    pct(y(v)),
                );
            }
        }
    }
    out.push_str(r#"<span class="rx-chart__cross" hidden></span><div class="rx-chart__tip" hidden></div></div>"#);

    // X labels: at most 8, the first and the last always.
    out.push_str(r#"<div class="rx-chart__x" aria-hidden="true">"#);
    let every = n.div_ceil(8).max(1);
    let mut shown_at: Vec<usize> = (0..n).step_by(every).collect();
    if let Some(&last) = shown_at.last()
        && last + 1 < n
    {
        // The last label always shows; one too close before it gives way.
        if n - 1 - last < every.div_ceil(2) && shown_at.len() > 1 {
            shown_at.pop();
        }
        shown_at.push(n - 1);
    }
    for (shown, &i) in shown_at.iter().enumerate() {
        let label = &options.labels[i];
        let left = if bar {
            (i as f64 + 0.5) / n as f64 * 100.0
        } else if n > 1 {
            i as f64 / (n - 1) as f64 * 100.0
        } else {
            50.0
        };
        // On phones every other label hides; the first and the last stay.
        let narrow = if shown % 2 == 1 && shown + 1 < shown_at.len() {
            " rx-chart__x--odd"
        } else {
            ""
        };
        let _ = write!(
            out,
            r#"<span class="{narrow}" style="left: {}%">{}</span>"#,
            pct(left),
            escape(label)
        );
    }
    out.push_str("</div></div>");
    x_title(&mut out, options);

    let heads: Vec<String> = data.series.iter().map(|s| s.name.clone()).collect();
    let rows: Vec<(String, Vec<String>)> = (0..n)
        .map(|i| {
            (
                options.labels[i].clone(),
                data.series
                    .iter()
                    .map(|s| {
                        s.values[i]
                            .map(|v| formatter.full(v))
                            .unwrap_or_else(|| "—".into())
                    })
                    .collect(),
            )
        })
        .collect();
    table(&mut out, options, &heads, &rows);
    out.push_str("</figure>");
    out
}

fn render_pie(data: &Data, options: &Options, formatter: &Formatter) -> String {
    let values = data
        .series
        .first()
        .map(|s| s.values.clone())
        .unwrap_or_default();
    // Six slices at most: the rest is "Other".
    let mut slices: Vec<(String, f64)> = data
        .labels
        .iter()
        .zip(values)
        .map(|(label, v)| (label.clone(), v.unwrap_or(0.0).max(0.0)))
        .collect();
    let display: Vec<String> = options.labels.clone();
    for (slice, label) in slices.iter_mut().zip(&display) {
        slice.0 = label.clone();
    }
    if slices.len() > 6 {
        let rest: f64 = slices[5..].iter().map(|s| s.1).sum();
        slices.truncate(5);
        slices.push((options.other.clone(), rest));
    }
    let total: f64 = slices.iter().map(|s| s.1).sum();
    let share = |v: f64| if total > 0.0 { v / total * 100.0 } else { 0.0 };
    let percent = |v: f64| format!("{}%", format_number(share(v), 1, &formatter.locale));
    let labels: Vec<String> = slices.iter().map(|s| s.0.clone()).collect();
    let hover = Hover {
        kind: &options.kind,
        labels: &labels,
        series: vec![HoverSeries {
            name: options.title.clone(),
            slot: String::new(),
            values: slices
                .iter()
                .map(|s| Some(format!("{} ({})", formatter.full(s.1), percent(s.1))))
                .collect(),
        }],
    };
    let mut out = String::new();
    open_figure(&mut out, options, &hover);
    let label = if options.title.is_empty() {
        labels.join(", ")
    } else {
        options.title.clone()
    };
    let _ = write!(
        out,
        r#"<div class="rx-chart__pie"><div class="rx-chart__plot rx-chart__plot--pie" tabindex="0" role="img" aria-label="{}"><svg viewBox="-50 -50 100 100" aria-hidden="true" focusable="false">"#,
        escape(&label)
    );
    let inner = if options.kind == "doughnut" {
        30.0
    } else {
        0.0
    };
    let outer = 48.0;
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, (_, v)) in slices.iter().enumerate() {
        if *v <= 0.0 || total <= 0.0 {
            continue;
        }
        let sweep = v / total * std::f64::consts::TAU;
        let class = format!("rx-chart__slice {}", slot(i));
        if sweep >= std::f64::consts::TAU - 1e-9 {
            let _ = write!(
                out,
                r#"<circle class="{class}" data-index="{i}" r="{}" fill-rule="evenodd"/>"#,
                outer
            );
            if inner > 0.0 {
                let _ = write!(out, r#"<circle class="rx-chart__hole" r="{inner}"/>"#);
            }
        } else {
            let (a0, a1) = (angle, angle + sweep);
            let large = if sweep > std::f64::consts::PI { 1 } else { 0 };
            let p = |r: f64, a: f64| (r * a.cos(), r * a.sin());
            let (x0, y0) = p(outer, a0);
            let (x1, y1) = p(outer, a1);
            let d = if inner > 0.0 {
                let (x2, y2) = p(inner, a1);
                let (x3, y3) = p(inner, a0);
                format!(
                    "M{x0:.3},{y0:.3}A{outer},{outer} 0 {large} 1 {x1:.3},{y1:.3}L{x2:.3},{y2:.3}A{inner},{inner} 0 {large} 0 {x3:.3},{y3:.3}Z"
                )
            } else {
                format!("M0,0L{x0:.3},{y0:.3}A{outer},{outer} 0 {large} 1 {x1:.3},{y1:.3}Z")
            };
            let _ = write!(out, r#"<path class="{class}" data-index="{i}" d="{d}"/>"#);
        }
        angle += sweep;
    }
    out.push_str(r#"</svg><div class="rx-chart__tip" hidden></div></div>"#);
    if options.legend {
        out.push_str(r#"<ul class="rx-chart__legend rx-chart__legend--pie" role="list">"#);
        for (i, (name, v)) in slices.iter().enumerate() {
            let _ = write!(
                out,
                r#"<li><span class="rx-chart__key rx-chart__key--box {}" aria-hidden="true"></span><span class="rx-chart__name">{}</span><span class="rx-chart__value">{} <span class="rx-chart__share">{}</span></span></li>"#,
                slot(i),
                escape(name),
                escape(&formatter.full(*v)),
                escape(&percent(*v)),
            );
        }
        out.push_str("</ul>");
    }
    out.push_str("</div>");
    let heads = vec![
        if options.title.is_empty() {
            "".into()
        } else {
            options.title.clone()
        },
        "%".into(),
    ];
    let rows: Vec<(String, Vec<String>)> = slices
        .iter()
        .map(|(name, v)| (name.clone(), vec![formatter.full(*v), percent(*v)]))
        .collect();
    table(&mut out, options, &heads, &rows);
    out.push_str("</figure>");
    out
}

// ---------------------------------------------------------------------------
// Heatmaps.

/// One filled cell of a heatmap.
struct Cell {
    column: usize,
    row: usize,
    value: f64,
    label: Option<String>,
}

/// What `chart("heatmap", …)` draws: labelled rows and columns and the
/// cells that have a value.
struct Heat {
    columns: Vec<String>,
    rows: Vec<String>,
    cells: Vec<Cell>,
}

/// The most rows or columns a heatmap draws.
const HEAT_MAX: usize = 100;

/// Reads `chart("heatmap", …)`'s data: `columns` and `rows` (labels) with
/// either `values` (one list per row, one number per column) or `cells`
/// (`[column, row, value]` or `{x, y, value, label}`, indexes counting from
/// 0). `data` may be a map holding any of these.
fn read_heat(data: Option<Value>, kwargs: &Kwargs) -> std::result::Result<Heat, Error> {
    let mut columns: Option<Value> = kwargs.get("columns")?;
    let mut rows: Option<Value> = kwargs.get("rows")?;
    let mut values: Option<Value> = kwargs.get("values")?;
    let mut cells: Option<Value> = kwargs.get("cells")?;
    if let Some(data) = data.filter(|d| d.kind() == ValueKind::Map) {
        columns = columns.or_else(|| attr(&data, "columns"));
        rows = rows.or_else(|| attr(&data, "rows"));
        values = values.or_else(|| attr(&data, "values"));
        cells = cells.or_else(|| attr(&data, "cells"));
    }
    let mut heat = Heat {
        columns: columns.as_ref().map(strings).unwrap_or_default(),
        rows: rows.as_ref().map(strings).unwrap_or_default(),
        cells: Vec::new(),
    };
    if let Some(values) = values
        && let Ok(lines) = values.try_iter()
    {
        for (row, line) in lines.enumerate() {
            for (column, value) in numbers(&line).into_iter().enumerate() {
                if let Some(value) = value {
                    heat.cells.push(Cell {
                        column,
                        row,
                        value,
                        label: None,
                    });
                }
            }
        }
    }
    if let Some(cells) = cells
        && let Ok(items) = cells.try_iter()
    {
        for item in items {
            let index = |v: Option<Value>| {
                v.as_ref()
                    .and_then(number_of)
                    .filter(|n| *n >= 0.0 && n.fract() == 0.0)
                    .map(|n| n as usize)
            };
            let cell = if item.kind() == ValueKind::Map {
                let value = attr(&item, "value").or_else(|| attr(&item, "size"));
                (
                    index(attr(&item, "x")),
                    index(attr(&item, "y")),
                    value.as_ref().and_then(number_of),
                    attr(&item, "label").map(|l| {
                        l.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| l.to_string())
                    }),
                )
            } else {
                let parts: Vec<Value> = item.try_iter().map(|i| i.collect()).unwrap_or_default();
                (
                    index(parts.first().cloned()),
                    index(parts.get(1).cloned()),
                    parts.get(2).and_then(number_of),
                    None,
                )
            };
            if let (Some(column), Some(row), Some(value), label) = cell {
                heat.cells.push(Cell {
                    column,
                    row,
                    value,
                    label,
                });
            }
        }
    }
    let width = heat.cells.iter().map(|c| c.column + 1).max().unwrap_or(0);
    let height = heat.cells.iter().map(|c| c.row + 1).max().unwrap_or(0);
    for (labels, n) in [(&mut heat.columns, width), (&mut heat.rows, height)] {
        for i in labels.len()..n {
            labels.push((i + 1).to_string());
        }
        labels.truncate(HEAT_MAX);
    }
    let (w, h) = (heat.columns.len(), heat.rows.len());
    heat.cells.retain(|c| c.column < w && c.row < h);
    Ok(heat)
}

/// How many shades a heatmap has besides "nothing".
const HEAT_LEVELS: usize = 5;

/// A heatmap as an HTML grid of cells (no script): each cell has its value
/// in a `title` and a shade by its share of the largest one; the legend
/// and the data table repeat the numbers in text.
fn render_heat(
    heat: &Heat,
    options: &Options,
    formatter: &Formatter,
    less: &str,
    more: &str,
) -> String {
    let (w, h) = (heat.columns.len(), heat.rows.len());
    let mut grid: Vec<Option<(f64, Option<&str>)>> = vec![None; w * h];
    for cell in &heat.cells {
        grid[cell.row * w + cell.column] = Some((cell.value, cell.label.as_deref()));
    }
    let max = grid
        .iter()
        .flatten()
        .map(|(v, _)| *v)
        .fold(0.0_f64, f64::max);
    let level = |v: f64| {
        if v <= 0.0 || max <= 0.0 {
            0
        } else {
            ((v / max * HEAT_LEVELS as f64).ceil() as usize).clamp(1, HEAT_LEVELS)
        }
    };
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<figure class="rx-chart rx-chart--heatmap"{id}>"#,
        id = options
            .id
            .as_deref()
            .map(|id| format!(r#" id="{}""#, escape(id)))
            .unwrap_or_default(),
    );
    if !options.title.is_empty() {
        let _ = write!(
            out,
            r#"<figcaption class="rx-visually-hidden">{}</figcaption>"#,
            escape(&options.title)
        );
    }
    y_title(&mut out, options);
    let label = if options.title.is_empty() {
        format!("{} × {}", heat.rows.len(), heat.columns.len())
    } else {
        options.title.clone()
    };
    let _ = write!(
        out,
        r#"<div class="rx-chart__heat" role="img" aria-label="{}" style="--rx-heat-cols:{w}"><span class="rx-chart__heat-corner"></span>"#,
        escape(&label)
    );
    // About twelve column labels fit; the rest are left blank.
    let step = w.div_ceil(12).max(1);
    for (i, name) in heat.columns.iter().enumerate() {
        let shown = if i % step == 0 {
            escape(name)
        } else {
            String::new()
        };
        let _ = write!(out, r#"<span class="rx-chart__heat-col">{shown}</span>"#);
    }
    for (r, row) in heat.rows.iter().enumerate() {
        let _ = write!(
            out,
            r#"<span class="rx-chart__heat-row">{}</span>"#,
            escape(row)
        );
        for (c, column) in heat.columns.iter().enumerate() {
            match grid[r * w + c] {
                Some((value, custom)) => {
                    let name = custom
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{row} {column}"));
                    let _ = write!(
                        out,
                        r#"<span class="rx-chart__cell" data-level="{}" title="{}"></span>"#,
                        level(value),
                        escape(&format!("{name}: {}", formatter.full(value))),
                    );
                }
                None => out.push_str(r#"<span class="rx-chart__cell" data-level="0"></span>"#),
            }
        }
    }
    out.push_str("</div>");
    x_title(&mut out, options);
    let _ = write!(
        out,
        r#"<div class="rx-chart__legend rx-chart__legend--heat" aria-hidden="true"><span>{}</span>"#,
        escape(less)
    );
    for l in 0..=HEAT_LEVELS {
        let _ = write!(
            out,
            r#"<span class="rx-chart__cell" data-level="{l}"></span>"#
        );
    }
    let _ = write!(out, "<span>{}</span></div>", escape(more));
    let rows: Vec<(String, Vec<String>)> = heat
        .rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            (
                row.clone(),
                (0..w)
                    .map(|c| grid[r * w + c].map_or_else(String::new, |(v, _)| formatter.full(v)))
                    .collect(),
            )
        })
        .collect();
    table(&mut out, options, &heat.columns, &rows);
    out.push_str("</figure>");
    out
}

/// The y axis' title, over the axis.
fn y_title(out: &mut String, options: &Options) {
    if let Some(title) = options.y_title.as_deref().filter(|t| !t.is_empty()) {
        let _ = write!(
            out,
            r#"<p class="rx-chart__axis-title rx-chart__axis-title--y" aria-hidden="true">{}</p>"#,
            escape(title)
        );
    }
}

/// The x axis' title, under its labels.
fn x_title(out: &mut String, options: &Options) {
    if let Some(title) = options.x_title.as_deref().filter(|t| !t.is_empty()) {
        let _ = write!(
            out,
            r#"<p class="rx-chart__axis-title rx-chart__axis-title--x" aria-hidden="true">{}</p>"#,
            escape(title)
        );
    }
}

// ---------------------------------------------------------------------------
// Scatter and bubble charts.

/// One point of a scatter (x, y) or bubble (x, y, size) chart.
struct Point {
    x: f64,
    y: f64,
    size: Option<f64>,
    label: Option<String>,
}

/// The points of one series.
struct Cloud {
    name: String,
    points: Vec<Point>,
}

/// `[x, y]`, `[x, y, size]` or `{x, y, size, label}`; `None` without both
/// numbers.
fn point_of(value: &Value) -> Option<Point> {
    if value.kind() == ValueKind::Map {
        return Some(Point {
            x: attr(value, "x").as_ref().and_then(number_of)?,
            y: attr(value, "y").as_ref().and_then(number_of)?,
            size: attr(value, "size").as_ref().and_then(number_of),
            label: attr(value, "label").map(|l| {
                l.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| l.to_string())
            }),
        });
    }
    let items: Vec<Value> = value.try_iter().ok()?.collect();
    Some(Point {
        x: number_of(items.first()?)?,
        y: number_of(items.get(1)?)?,
        size: items.get(2).and_then(number_of),
        label: None,
    })
}

fn points_of(value: &Value) -> Vec<Point> {
    match value.try_iter() {
        Ok(items) if value.kind() == ValueKind::Seq => {
            items.filter_map(|item| point_of(&item)).collect()
        }
        _ => Vec::new(),
    }
}

/// A series `{name, points}`, or a list of points.
fn cloud_of(value: &Value, fallback: &str) -> Cloud {
    if value.kind() == ValueKind::Map {
        Cloud {
            name: attr(value, "name")
                .and_then(|n| n.as_str().map(str::to_owned))
                .unwrap_or_else(|| fallback.to_owned()),
            points: attr(value, "points")
                .map(|p| points_of(&p))
                .unwrap_or_default(),
        }
    } else {
        Cloud {
            name: fallback.to_owned(),
            points: points_of(value),
        }
    }
}

/// The points of `chart("scatter" | "bubble", …)`: `data` (a series with
/// `points`, a list of them, or a list of points), `series=[…]`,
/// `points=[…]`.
fn read_points(
    data: Option<Value>,
    kwargs: &Kwargs,
    title: &str,
) -> std::result::Result<Vec<Cloud>, Error> {
    let series: Option<Value> = kwargs.get("series")?;
    let points: Option<Value> = kwargs.get("points")?;
    let name: Option<String> = kwargs.get("name")?;
    let fallback = name.clone().unwrap_or_else(|| title.to_owned());
    let mut clouds = Vec::new();
    let list_of_series = |value: &Value, clouds: &mut Vec<Cloud>| {
        if let Ok(items) = value.try_iter() {
            for (i, item) in items.enumerate() {
                clouds.push(cloud_of(&item, &format!("{} {}", fallback, i + 1)));
            }
        }
    };
    match data {
        Some(data) if data.kind() == ValueKind::Map => match attr(&data, "series") {
            Some(series) => list_of_series(&series, &mut clouds),
            None => clouds.push(cloud_of(&data, &fallback)),
        },
        Some(data) if data.kind() == ValueKind::Seq => {
            // A list of series has maps with `points`; a list of points has
            // pairs, triples or maps with `x`.
            let first = data.try_iter().ok().and_then(|mut items| items.next());
            if first.is_some_and(|v| v.kind() == ValueKind::Map && attr(&v, "points").is_some()) {
                list_of_series(&data, &mut clouds);
            } else {
                clouds.push(cloud_of(&data, &fallback));
            }
        }
        _ => {}
    }
    if let Some(series) = series {
        list_of_series(&series, &mut clouds);
    }
    if let Some(points) = points {
        clouds.push(Cloud {
            name: fallback.clone(),
            points: points_of(&points),
        });
    }
    if let Some(name) = name
        && clouds.len() == 1
    {
        clouds[0].name = name;
    }
    Ok(clouds)
}

/// As many decimals as the values need, up to 2.
fn auto_decimals(values: impl Iterator<Item = f64>) -> Option<u32> {
    let mut decimals = 0;
    for value in values {
        let fract = (value.abs() * 100.0).round() as i64 % 100;
        if fract % 10 != 0 {
            return Some(2);
        }
        if fract != 0 {
            decimals = 1;
        }
    }
    Some(decimals)
}

/// How a scatter or bubble chart's values read.
struct Formats {
    x: Formatter,
    y: Formatter,
    size: Formatter,
    size_title: String,
}

/// The tooltips' data for points.
#[derive(Serialize)]
struct HoverPoints<'a> {
    kind: &'a str,
    points: Vec<HoverPoint>,
}

#[derive(Serialize)]
struct HoverPoint {
    /// The point's label, or "".
    title: String,
    /// Its series' name with more than one series, or "".
    name: String,
    slot: String,
    /// `[axis title, value]`.
    rows: Vec<(String, String)>,
    /// Where it is, in percent of the plot.
    left: f64,
    bottom: f64,
}

/// The value range of points, padded for bubbles so they stay inside, with
/// 0 in when the data starts near it.
fn point_scale(values: impl Iterator<Item = f64>, pad: bool) -> (f64, f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in values {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if !lo.is_finite() {
        return scale(0.0, 0.0);
    }
    if pad {
        // Room around the data, but not across 0 (no "-20 units").
        let room = ((hi - lo) * 0.08).max(hi.abs().max(lo.abs()) * 0.02);
        lo = if lo >= 0.0 {
            (lo - room).max(0.0)
        } else {
            lo - room
        };
        hi = if hi <= 0.0 {
            (hi + room).min(0.0)
        } else {
            hi + room
        };
    }
    if lo >= 0.0 && lo <= hi * 0.25 {
        lo = 0.0;
    }
    if hi <= 0.0 && hi >= lo * 0.25 {
        hi = 0.0;
    }
    nice(lo, hi)
}

fn ticks(lo: f64, hi: f64, step: f64) -> Vec<f64> {
    let mut ticks = Vec::new();
    let mut tick = lo;
    while tick <= hi + step / 2.0 && ticks.len() < 12 {
        ticks.push(tick);
        tick += step;
    }
    ticks
}

fn render_points(clouds: &[Cloud], options: &Options, formats: &Formats) -> String {
    let bubble = options.kind == "bubble";
    let all = || clouds.iter().flat_map(|c| c.points.iter());
    let (x_lo, x_hi, x_step) = point_scale(all().map(|p| p.x), bubble);
    let (y_lo, y_hi, y_step) = point_scale(all().map(|p| p.y), bubble);
    let px = |v: f64| (v - x_lo) / (x_hi - x_lo) * 100.0;
    let py = |v: f64| (v - y_lo) / (y_hi - y_lo) * 100.0;
    // Bubbles: the area follows the size, up to 40 px across, 6 px at least.
    let biggest = all()
        .filter_map(|p| p.size)
        .fold(0.0f64, |a, b| a.max(b.abs()));
    let diameter = |size: Option<f64>| {
        let share = match (size, biggest > 0.0) {
            (Some(size), true) => (size.abs() / biggest).sqrt(),
            _ => 0.0,
        };
        (6.0 + share * 34.0).round()
    };
    let x_name = options.x_title.clone().unwrap_or_else(|| "x".into());
    let y_name = options.y_title.clone().unwrap_or_else(|| "y".into());
    let several = clouds.len() > 1;

    // Every point, left to right, as the arrow keys visit them.
    let mut order: Vec<(usize, &Point)> = clouds
        .iter()
        .enumerate()
        .flat_map(|(s, c)| c.points.iter().map(move |p| (s, p)))
        .collect();
    order.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.y.total_cmp(&b.1.y)));

    let rows_of = |point: &Point| {
        let mut rows = vec![
            (x_name.clone(), formats.x.full(point.x)),
            (y_name.clone(), formats.y.full(point.y)),
        ];
        if bubble && let Some(size) = point.size {
            rows.push((formats.size_title.clone(), formats.size.full(size)));
        }
        rows
    };
    let hover = HoverPoints {
        kind: &options.kind,
        points: order
            .iter()
            .map(|(s, point)| HoverPoint {
                title: point.label.clone().unwrap_or_default(),
                name: if several {
                    clouds[*s].name.clone()
                } else {
                    String::new()
                },
                slot: slot(*s),
                rows: rows_of(point),
                left: (px(point.x) * 100.0).round() / 100.0,
                bottom: (py(point.y) * 100.0).round() / 100.0,
            })
            .collect(),
    };
    let json = serde_json::to_string(&hover).unwrap_or_default();
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<figure class="rx-chart rx-chart--{kind} rx-chart--points"{id} data-rx-chart="{json}">"#,
        kind = escape(&options.kind),
        id = options
            .id
            .as_deref()
            .map(|id| format!(r#" id="{}""#, escape(id)))
            .unwrap_or_default(),
        json = escape(&json),
    );
    if !options.title.is_empty() {
        let _ = write!(
            out,
            r#"<figcaption class="rx-visually-hidden">{}</figcaption>"#,
            escape(&options.title)
        );
    }
    if options.legend && several {
        let names: Vec<(usize, &str)> = clouds
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.name.as_str()))
            .collect();
        legend(&mut out, &names, "dot");
    }
    y_title(&mut out, options);
    let _ = write!(
        out,
        r#"<div class="rx-chart__frame" style="--rx-chart-h: {}px"><div class="rx-chart__y" aria-hidden="true">"#,
        options.height
    );
    let y_ticks = ticks(y_lo, y_hi, y_step);
    for t in &y_ticks {
        let _ = write!(
            out,
            r#"<span style="bottom: {}%">{}</span>"#,
            pct(py(*t)),
            escape(&formats.y.tick(*t))
        );
    }
    let label = if options.title.is_empty() {
        clouds
            .iter()
            .map(|c| c.name.as_str())
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        options.title.clone()
    };
    let label = match (&options.x_title, &options.y_title) {
        (Some(x), Some(y)) => {
            let axes = options.axes.replace(":y", y).replace(":x", x);
            if label.is_empty() {
                axes
            } else {
                format!("{label}: {axes}")
            }
        }
        _ => label,
    };
    let _ = write!(
        out,
        r#"</div><div class="rx-chart__plot" tabindex="0" role="img" aria-label="{}"><div class="rx-chart__grid" aria-hidden="true">"#,
        escape(&label)
    );
    for t in &y_ticks {
        let base = if t.abs() < y_step / 1e6 {
            " rx-chart__rule--base"
        } else {
            ""
        };
        let _ = write!(
            out,
            r#"<span class="rx-chart__rule{base}" style="bottom: {}%"></span>"#,
            pct(py(*t))
        );
    }
    let x_ticks = ticks(x_lo, x_hi, x_step);
    for t in &x_ticks {
        let base = if t.abs() < x_step / 1e6 {
            " rx-chart__rule--base"
        } else {
            ""
        };
        let _ = write!(
            out,
            r#"<span class="rx-chart__rule rx-chart__rule--x{base}" style="left: {}%"></span>"#,
            pct(px(*t))
        );
    }
    out.push_str(r#"</div><div class="rx-chart__points" aria-hidden="true">"#);
    // Big bubbles first, so small ones stay on top and reachable.
    let mut drawn: Vec<(usize, &(usize, &Point))> = order.iter().enumerate().collect();
    if bubble {
        drawn.sort_by(|a, b| {
            let size = |p: &Point| p.size.map(f64::abs).unwrap_or(0.0);
            size(b.1.1).total_cmp(&size(a.1.1))
        });
    }
    for (index, (s, point)) in drawn {
        let size = if bubble {
            format!("; --rx-point: {}px", diameter(point.size))
        } else {
            String::new()
        };
        let _ = write!(
            out,
            r#"<span class="rx-chart__point {}" data-index="{index}" style="left: {}%; bottom: {}%{size}"></span>"#,
            slot(*s),
            pct(px(point.x)),
            pct(py(point.y)),
        );
    }
    out.push_str(r#"</div><div class="rx-chart__tip" hidden></div></div>"#);
    out.push_str(r#"<div class="rx-chart__x" aria-hidden="true">"#);
    for (shown, t) in x_ticks.iter().enumerate() {
        let narrow = if shown % 2 == 1 && shown + 1 < x_ticks.len() {
            " rx-chart__x--odd"
        } else {
            ""
        };
        let _ = write!(
            out,
            r#"<span class="{narrow}" style="left: {}%">{}</span>"#,
            pct(px(*t)),
            escape(&formats.x.tick(*t))
        );
    }
    out.push_str("</div></div>");
    x_title(&mut out, options);

    let mut heads = Vec::new();
    if several {
        heads.push(options.series_head.clone());
    }
    heads.push(x_name.clone());
    heads.push(y_name.clone());
    if bubble {
        heads.push(formats.size_title.clone());
    }
    let rows: Vec<(String, Vec<String>)> = order
        .iter()
        .enumerate()
        .map(|(i, (s, point))| {
            let mut cells = Vec::new();
            if several {
                cells.push(clouds[*s].name.clone());
            }
            cells.push(formats.x.full(point.x));
            cells.push(formats.y.full(point.y));
            if bubble {
                cells.push(
                    point
                        .size
                        .map(|v| formats.size.full(v))
                        .unwrap_or_else(|| "—".into()),
                );
            }
            (
                point.label.clone().unwrap_or_else(|| (i + 1).to_string()),
                cells,
            )
        })
        .collect();
    table(&mut out, options, &heads, &rows);
    out.push_str("</figure>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_parse_and_step_back() {
        assert_eq!(Period::parse("30d"), Some(Period::days(30)));
        assert_eq!(Period::parse("12M").map(|p| p.key()), Some("12m".into()));
        assert_eq!(Period::parse("ytd"), Some(Period::year_to_date()));
        for bad in [
            "",
            "0d",
            "400d",
            "40m",
            "105w",
            "d",
            "-3d",
            "2026-09-30..2026-09-01",
            "2020-01-01..2026-01-01",
            "2026-9-1..2026-09-30",
        ] {
            assert_eq!(Period::parse(bad), None, "{bad}");
        }
        assert_eq!(Period::days(30).bucket(), Bucket::Day);
        assert_eq!(Period::days(365).bucket(), Bucket::Month);
        let zone = Zone::UTC;
        let (start, end) = Period::days(7).days_in(zone);
        assert_eq!((end - start).num_days(), 7);
        let (before_start, before_end) = Period::days(7).previous().days_in(zone);
        assert_eq!(
            (before_end, (before_end - before_start).num_days()),
            (start, 7)
        );
        assert_eq!(Period::days(7).labels(zone).len(), 7);
        assert_eq!(Period::months(12).labels(zone).len(), 12);
        let (m0, m1) = Period::months(3).previous().days_in(zone);
        assert_eq!((m0.day(), m1.day()), (1, 1));
        assert_eq!(
            add_months(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), -1),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
    }

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn custom_ranges_parse_and_step_back() {
        let range = Period::between(day("2026-09-01"), day("2026-09-30")).unwrap();
        assert!(range.is_custom() && !Period::days(30).is_custom());
        assert_eq!(range.key(), "2026-09-01..2026-09-30");
        assert_eq!(Period::parse(&range.key()), Some(range));
        assert_eq!(
            range.days_in(Zone::UTC),
            (day("2026-09-01"), day("2026-10-01"))
        );
        // The period before: as many days, just before.
        assert_eq!(
            range.previous().days_in(Zone::UTC),
            (day("2026-08-02"), day("2026-09-01"))
        );
        // One day is fine; backwards or over three years is not.
        assert!(Period::between(day("2026-09-01"), day("2026-09-01")).is_some());
        assert!(Period::between(day("2026-09-02"), day("2026-09-01")).is_none());
        assert!(Period::between(day("2024-01-01"), day("2026-12-31")).is_some());
        assert!(Period::between(day("2024-01-01"), day("2027-01-01")).is_none());
        // From a query string.
        let query = "q=x&period=custom&from=2026-09-01&to=2026-09-30";
        assert_eq!(Period::from_query(query), Some(range));
        assert_eq!(
            Period::from_query("period=12w").map(|p| p.key()),
            Some("12w".into())
        );
        for bad in [
            "period=custom",
            "period=custom&from=2026-09-01",
            "period=custom&from=2026-09-31&to=2026-10-01",
            "period=custom&from=2026-10-01&to=2026-09-01",
            "from=2026-09-01&to=2026-09-30",
        ] {
            assert_eq!(Period::from_query(bad), None, "{bad}");
        }
        // Per day up to 92 days, per week up to 26 weeks, then per month.
        let span = |days: i64| {
            let from = day("2026-01-01");
            Period::between(from, from + Duration::days(days - 1))
                .unwrap()
                .bucket()
        };
        assert_eq!(
            (span(92), span(93), span(182), span(183)),
            (Bucket::Day, Bucket::Week, Bucket::Week, Bucket::Month)
        );
    }

    #[test]
    fn weeks_start_on_monday() {
        assert_eq!(monday_of(day("2026-10-04")), day("2026-09-28")); // a Sunday
        assert_eq!(monday_of(day("2026-09-28")), day("2026-09-28"));
        let weeks = Period::weeks(4);
        assert_eq!(weeks.bucket(), Bucket::Week);
        let (start, end) = weeks.days_in(Zone::UTC);
        assert_eq!(start.weekday(), chrono::Weekday::Mon);
        assert!((end - start).num_days() > 21 && (end - start).num_days() <= 28);
        let labels = weeks.labels(Zone::UTC);
        assert_eq!(labels.len(), 4);
        assert_eq!(labels[0], start.format("%Y-%m-%d").to_string());
        // The four weeks before end where these start, at the same weekday.
        let (before_start, before_end) = weeks.previous().days_in(Zone::UTC);
        assert_eq!(
            (before_start, end - before_end),
            (start - Duration::days(28), Duration::days(28))
        );
        // A custom range per week: the first label is the Monday before.
        let range = Period::between(day("2026-09-02"), day("2026-09-30"))
            .unwrap()
            .per(Bucket::Week);
        assert_eq!(
            range.labels(Zone::UTC),
            [
                "2026-08-31",
                "2026-09-07",
                "2026-09-14",
                "2026-09-21",
                "2026-09-28"
            ]
        );
        assert_eq!(range.previous().bucket(), Bucket::Week);
        assert_eq!(Period::days(365).per(Bucket::Day).bucket(), Bucket::Day);
    }

    #[test]
    fn series_add_up_and_compare() {
        let now = Series::new(vec!["a".into(), "b".into()], vec![30.0, 30.0]);
        let before = Series::new(vec!["a".into(), "b".into()], vec![20.0, 20.0]);
        assert_eq!(now.change_from(&before), Some(50.0));
        assert_eq!(now.change_from(&Series::default()), None);
        assert_eq!(
            serde_json::to_value(now.named("Sales")).unwrap()["name"],
            "Sales"
        );
    }

    #[test]
    fn heatmaps_shade_cells_and_say_their_values() {
        let html = render(
            r#"{{ chart("heatmap", columns=["Mon", "Tue"], rows=["09:00", "10:00"], cells=[[0, 0, 1], [1, 0, 5], {"x": 1, "y": 1, "value": 3, "label": "Tue late"}], title="Rentals") }}"#,
        );
        assert!(
            html.contains(r#"class="rx-chart rx-chart--heatmap""#),
            "{html}"
        );
        assert!(html.contains(r#"aria-label="Rentals""#));
        assert!(
            html.contains(r#"data-level="1" title="09:00 Mon: 1""#),
            "{html}"
        );
        assert!(html.contains(r#"data-level="5" title="09:00 Tue: 5""#));
        assert!(html.contains(r#"data-level="3" title="Tue late: 3""#));
        // The cell nobody filled is empty, and the numbers are in the table.
        assert!(html.contains(r#"<span class="rx-chart__cell" data-level="0"></span>"#));
        assert!(html.contains("Show the data") && html.contains("<td"));
        let dense = render(
            r#"{{ chart("heatmap", columns=["a"], rows=["x", "y"], values=[[2], [null]]) }}"#,
        );
        assert!(
            dense.contains(r#"data-level="5" title="x a: 2""#),
            "{dense}"
        );
        assert!(!dense.contains("y a:"));
        // No data: still a figure, not an error.
        assert!(render(r#"{{ chart("heatmap") }}"#).contains("rx-chart--heatmap"));
    }

    #[test]
    fn scales_are_clean() {
        assert_eq!(scale(0.0, 87.0), (0.0, 100.0, 25.0));
        assert_eq!(scale(0.0, 1234.0), (0.0, 1500.0, 500.0));
        assert_eq!(scale(-30.0, 70.0), (-50.0, 75.0, 25.0));
        assert_eq!(scale(0.0, 0.0), (0.0, 1.0, 0.25));
        // Scatter axes needn't start at 0, unless the data comes close.
        assert_eq!(
            point_scale([52.0, 87.0].into_iter(), false),
            (50.0, 90.0, 10.0)
        );
        assert_eq!(
            point_scale([3.0, 87.0].into_iter(), false),
            (0.0, 100.0, 25.0)
        );
        assert_eq!(point_scale([-5.0, -80.0].into_iter(), false).1, 0.0);
        assert_eq!(point_scale(std::iter::empty(), true), (0.0, 1.0, 0.25));
        assert_eq!(
            auto_decimals([1.0, 2.5].into_iter()),
            Some(1),
            "as many decimals as needed"
        );
        assert_eq!(auto_decimals([1.25].into_iter()), Some(2));
        assert_eq!(auto_decimals([3.0].into_iter()), Some(0));
    }

    #[test]
    fn labels_and_ticks_read_well() {
        assert_eq!(display_label("2026-10-02", None), "Oct 2");
        assert_eq!(display_label("2026-08", None), "Aug 2026");
        assert_eq!(display_label("2026-10-02", Some("%d/%m")), "02/10");
        assert_eq!(display_label("Coffee", None), "Coffee");
        let f = |locale: &str| Formatter {
            format: "number".into(),
            decimals: None,
            currency: "USD".into(),
            locale: locale.into(),
            divisor: 1.0,
        };
        assert_eq!(f("en").tick(12_500.0), "12.5K");
        assert_eq!(f("es").tick(2_500_000.0), "2,5M");
        assert_eq!(f("en").tick(750.0), "750");
        assert_eq!(f("en").tick(2_000_000_000.0), "2B");
    }

    // #255: "so far" periods on awkward days, and charts with awkward data.

    /// Runs `f` with the clock on `date` at noon UTC.
    fn on<T>(date: &str, f: impl FnOnce() -> T) -> T {
        let at = day(date)
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        let now = chrono::Utc::now().timestamp();
        crate::clock::with_offset_sync(at - now, f)
    }

    #[test]
    fn month_and_year_to_date_compare_with_as_many_days_before() {
        let zone = Zone::UTC;
        let mtd = Period::month_to_date();
        assert_eq!(mtd.key(), "mtd");
        assert_eq!(mtd.bucket(), Bucket::Day);
        // The 31st: the month so far is the whole month; the month before
        // (February) is cut at its own end.
        on("2026-03-31", || {
            assert_eq!(mtd.days_in(zone), (day("2026-03-01"), day("2026-04-01")));
            assert_eq!(
                mtd.previous().days_in(zone),
                (day("2026-02-01"), day("2026-03-01"))
            );
        });
        // 29 February: the year so far is 60 days; the year before has
        // as many days, from 1 January.
        on("2024-02-29", || {
            let ytd = Period::year_to_date();
            assert_eq!(ytd.days_in(zone), (day("2024-01-01"), day("2024-03-01")));
            assert_eq!(
                ytd.previous().days_in(zone),
                (day("2023-01-01"), day("2023-03-02"))
            );
        });
        // The last day of a leap year: the year before is cut at its end.
        on("2024-12-31", || {
            assert_eq!(
                Period::year_to_date().previous().days_in(zone),
                (day("2023-01-01"), day("2024-01-01"))
            );
        });
    }

    fn render(source: &str) -> String {
        let mut env = minijinja::Environment::new();
        env.add_function("chart", chart("USD".into()));
        env.render_str(source, minijinja::context! {})
            .unwrap_or_else(|err| panic!("{err:#}"))
    }

    #[test]
    fn awkward_data_still_draws() {
        // A gap in a line, values all below zero, all the same.
        let gap =
            render(r#"{{ chart("line", labels=["a", "b", "c", "d"], values=[1, none, 3, 4]) }}"#);
        assert!(gap.contains("<figure class=\"rx-chart"), "{gap}");
        for source in [
            r#"{{ chart("bar", [-5, -2, -9]) }}"#,
            r#"{{ chart("line", [3, 3, 3]) }}"#,
            r#"{{ chart("line", [0, 0]) }}"#,
        ] {
            assert!(
                render(source).contains("<figure class=\"rx-chart"),
                "{source}"
            );
        }
        // More series than colours: the rest share one.
        let many = render(
            r#"{{ chart("line", labels=["a"], series=[{"name": "1", "values": [1]}, {"name": "2", "values": [1]}, {"name": "3", "values": [1]}, {"name": "4", "values": [1]}, {"name": "5", "values": [1]}, {"name": "6", "values": [1]}, {"name": "7", "values": [1]}]) }}"#,
        );
        assert!(many.contains("rx-series-other"), "{many}");
    }

    #[test]
    fn pies_skip_empty_slices_and_draw_a_whole_one_as_a_circle() {
        let pie = render(r#"{{ chart("pie", labels=["none", "all"], values=[0, 5]) }}"#);
        assert!(
            pie.contains(r#"<circle class="rx-chart__slice rx-series-2""#),
            "{pie}"
        );
        assert!(!pie.contains("rx-chart__hole"));
        assert!(pie.contains(r#"aria-label="none, all""#), "{pie}");
        let ring = render(r#"{{ chart("doughnut", labels=["all"], values=[5], legend=false) }}"#);
        assert!(ring.contains("rx-chart__hole"), "{ring}");
        assert!(!ring.contains("rx-chart__legend"));
        let empty = render(r#"{{ chart("pie", labels=["a"], values=[0]) }}"#);
        assert!(!empty.contains("rx-chart__slice"), "{empty}");
    }

    /// Data given as a list of series, as one series object, as an object
    /// holding labels and series, and values that aren't numbers.
    #[test]
    fn data_comes_in_every_shape() {
        let list = render(
            r#"{{ chart("line", [{"name": "North", "values": [1, 2]}, {"values": [3, 4]}], labels=["x", "y"], name="Sales") }}"#,
        );
        assert!(list.contains(">North<"), "{list}");
        assert!(
            list.contains(">Sales 2<"),
            "a series without a name: {list}"
        );
        let one = render(r#"{{ chart("bar", {"name": "Visits", "values": [1, 2]}) }}"#);
        assert!(one.contains(">Visits<"), "{one}");
        let nested = render(
            r#"{{ chart("line", {"labels": ["Mon", "Tue"], "series": [{"name": "Cups", "values": [5, 6]}]}) }}"#,
        );
        assert!(
            nested.contains(">Mon<") && nested.contains(">Cups<"),
            "{nested}"
        );
        // Labels that aren't text are written as text; a number in text is a
        // number, other values are gaps.
        let odd = render(r#"{{ chart("line", labels=[1, true], values=["3", "x", 2.5]) }}"#);
        assert!(odd.contains(">True<"), "{odd}");
        assert!(
            odd.contains(">3.0</td>") && odd.contains(">—</td>"),
            "{odd}"
        );
        // Three values but two labels: the third gets its number.
        assert!(odd.contains(r#"<th scope="row">3</th>"#), "{odd}");
    }

    #[test]
    fn axes_for_negative_and_flat_values() {
        assert_eq!(scale(-9.0, -2.0), (-10.0, 0.0, 2.5));
        let (lo, hi, step) = nice(3.0, 3.0);
        assert!(lo < 3.0 && hi > 3.0 && step > 0.0, "{lo} {hi} {step}");
        let (lo, hi, _) = nice(-4.0, -4.0);
        assert!(lo < -4.0 && hi > -4.0, "{lo} {hi}");
        let flat = render(r#"{{ chart("bar", [-4, -4]) }}"#);
        assert!(flat.contains(">-4<") || flat.contains(">−4<"), "{flat}");
    }

    #[test]
    fn lines_break_at_gaps_and_keep_their_last_label() {
        let gap = render(r#"{{ chart("line", [1, none, 3, 4]) }}"#);
        assert_eq!(gap.matches(r#"class="rx-chart__line "#).count(), 2, "{gap}");
        // Twenty points: every third label, the last always, the one just
        // before it giving way.
        let many = render(r#"{{ chart("line", values=range(20)|list) }}"#);
        let x = many.split(r#"<div class="rx-chart__x""#).nth(1).unwrap();
        let x = x.split("</div>").next().unwrap();
        assert!(x.contains(">20</span>") && !x.contains(">19</span>"), "{x}");
        assert!(x.contains(">1</span>") && x.contains(">4</span>"), "{x}");
        // One point sits in the middle.
        let one = render(r#"{{ chart("line", [5]) }}"#);
        assert!(one.contains(r#"style="left: 50%">1</span>"#), "{one}");
    }

    /// Line, bar and pie values with fractions keep them in the tooltip
    /// data and the table (they were rounded to whole numbers); whole
    /// numbers stay whole, and `decimals` still decides.
    #[test]
    fn fractions_keep_their_decimals() {
        let bars = render(r#"{{ chart("bar", [1.5, 2.25, 3]) }}"#);
        let table = bars.split("<tbody>").nth(1).unwrap();
        assert!(
            table.contains(">1.50<") && table.contains(">2.25<") && table.contains(">3.00<"),
            "{table}"
        );
        let pie = render(r#"{{ chart("pie", labels=["a", "b"], values=[4.5, 5.5]) }}"#);
        assert!(pie.contains(">4.5<"), "{pie}");
        let whole = render(r#"{{ chart("line", [1, 2]) }}"#);
        assert!(
            whole.split("<tbody>").nth(1).unwrap().contains(">2<"),
            "{whole}"
        );
        let chosen = render(r#"{{ chart("bar", [1.5], decimals=0) }}"#);
        assert!(
            chosen.split("<tbody>").nth(1).unwrap().contains(">2<"),
            "{chosen}"
        );
    }

    /// Scatter and bubble data in every shape `read_points` takes, and a
    /// line chart's `name` for its one series.
    #[test]
    fn points_come_in_every_shape() {
        let nested = render(
            r#"{{ chart("scatter", {"series": [{"name": "North", "points": [[1, 2]]}, {"name": "South", "points": [[3, 4]]}]}) }}"#,
        );
        assert!(
            nested.contains(">North<") && nested.contains(">South<"),
            "{nested}"
        );
        let listed = render(r#"{{ chart("scatter", [{"name": "East", "points": [[1, 2]]}]) }}"#);
        // One series: drawn, without a legend to name it.
        assert!(listed.contains(r#"data-index="0""#), "{listed}");
        let pairs = render(r#"{{ chart("scatter", [[1, 2], [3, 4]], name="Pairs") }}"#);
        assert!(pairs.contains(r#"data-index="1""#), "two points: {pairs}");
        let one =
            render(r#"{{ chart("bubble", {"name": "Sizes", "points": [[1, 2, 3], [2, 3, 9]]}) }}"#);
        assert!(one.contains(r#"data-index="1""#), "{one}");
        let kwarg = render(
            r#"{{ chart("scatter", series=[{"name": "West", "points": [[5, 5]]}, {"name": "Far", "points": [[6, 6]]}]) }}"#,
        );
        assert!(
            kwarg.contains(">West<") && kwarg.contains(">Far<"),
            "{kwarg}"
        );
        let named = render(r#"{{ chart("line", [1, 2], name="Only") }}"#);
        assert!(named.contains(">Only<"), "{named}");
    }

    #[test]
    fn scatter_marks_the_zero_line() {
        let html = render(r#"{{ chart("scatter", points=[[-5, -10], [8, 20]]) }}"#);
        assert!(html.contains("rx-chart__rule--base"), "{html}");
    }

    #[test]
    fn scatter_with_negative_values_only() {
        let html = render(r#"{{ chart("scatter", points=[[-5, -10], [-1, -3]]) }}"#);
        // The axes reach below the lowest point and up to zero.
        assert!(
            html.contains(r#"<span style="bottom: 0%">-10</span>"#),
            "{html}"
        );
        assert!(html.contains(r#"style="left: 100%">0</span>"#), "{html}");
    }
}
