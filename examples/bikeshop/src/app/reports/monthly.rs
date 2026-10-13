//! The monthly report: one Excel workbook per store, made by a queued
//! batch and mailed.
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/reports/monthly` | `reports.monthly` | `reports.view` |
//! | `POST /staff/reports/monthly` | `reports.monthly.run` | `reports.view` (for the stores where they hold it) |
//! | `GET /staff/reports/monthly/runs` | `reports.monthly.runs` | `reports.view` (the runs, a fragment the page polls) |
//! | `GET /staff/reports/monthly/{month}/{store}` | `reports.monthly.file` | `reports.view` in that store |
//!
//! On the 1st of each month at 03:00 (`reports:monthly`,
//! `Schedule::monthly_on`; after the books are settled at 02:00) the
//! previous month runs for every store. A run is one **queue batch**
//! (`monthly-report:2026-09:1-2-3`): one [`BuildStoreReport`] job per
//! store writes its workbook to storage (its income as owner and as
//! operator, its statement between stores), side by side; when all of
//! them succeeded, the batch's `then` job [`SendMonthlyReport`] mails each
//! person who holds `reports.view` the workbooks of their stores (the
//! owner gets all three, a manager their store's). The page starts a run
//! for any month and shows each run's progress from `job_batches`.

use std::collections::{BTreeMap, HashMap};

use renox::Download;
use renox::axum::body::Bytes;
use renox::chrono::{Duration, NaiveDate};
use renox::db::sql;
use renox::prelude::*;
use renox::schedule::Schedule;
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};
use serde::{Deserialize, Serialize};

use super::model::{RevenueLine, STREAMS};
use super::scope::Reach;
use crate::app::access::{can_in, catalogue};
use crate::app::multistore::help::day_start;
use crate::app::multistore::model::IntercompanyEntry;
use crate::app::multistore::settlements::{month_of, next_month};
use crate::app::rentals::notify::staff_with_permission;
use crate::app::staff::model::Store;

/// The batches' names start with this, then `YYYY-MM:` and the store ids.
pub const BATCH_PREFIX: &str = "monthly-report:";

/// Registers the monthly task.
pub fn schedule(s: &mut Schedule) {
    s.monthly_on(
        1,
        "03:00",
        "reports:monthly",
        |state: AppState| async move {
            let today =
                crate::app::rentals::booking::to_local(&state.config, renox::db::now()).date();
            let previous = month_of(month_of(today) - Duration::days(1));
            let stores: Vec<i64> = Store::all_by_name(&state.db)
                .await?
                .into_iter()
                .map(|s| s.id)
                .collect();
            start(&state, previous, &stores).await.map(|_| ())
        },
    );
}

/// Where a store's workbook for `month` is kept (the private disk).
pub fn file_key(month: NaiveDate, store_id: i64) -> String {
    format!("reports/monthly/{}/{store_id}.xlsx", month.format("%Y-%m"))
}

/// The attachment's name: `bikeshop-north-2026-09.xlsx`.
pub fn file_name(month: NaiveDate, slug: &str) -> String {
    format!("bikeshop-{slug}-{}.xlsx", month.format("%Y-%m"))
}

// [explain:reports.monthly.batch]
/// Starts a run for `month` over `stores`: the batch, its id.
pub async fn start(state: &AppState, month: NaiveDate, stores: &[i64]) -> Result<i64> {
    let month = month_of(month);
    let ids = stores
        .iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join("-");
    let mut batch = state
        .queue
        .batch(&format!("{BATCH_PREFIX}{}:{ids}", month.format("%Y-%m")));
    for store_id in stores {
        batch = batch.push(BuildStoreReport {
            month,
            store_id: *store_id,
        });
    }
    batch
        .then(SendMonthlyReport {
            month,
            stores: stores.to_vec(),
        })
        .dispatch()
        .await
}
// [/explain:reports.monthly.batch]

/// Writes one store's workbook for one month to storage.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildStoreReport {
    /// The month's first day.
    pub month: NaiveDate,
    pub store_id: i64,
}

impl Job for BuildStoreReport {
    const NAME: &'static str = "reports-build-store";

    async fn handle(self, ctx: JobContext) -> Result {
        let bytes = workbook(&ctx.state, self.store_id, self.month).await?;
        ctx.state
            .storage
            .put(&file_key(self.month, self.store_id), Bytes::from(bytes))
            .await
    }
}

/// Mails the month's workbooks, once every store's is written (the batch's
/// `then` job): each person holding `reports.view` gets the workbooks of
/// the stores where they hold it, in one mail.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SendMonthlyReport {
    pub month: NaiveDate,
    pub stores: Vec<i64>,
}

impl Job for SendMonthlyReport {
    const NAME: &'static str = "reports-send-monthly";

    async fn handle(self, ctx: JobContext) -> Result {
        let state = &ctx.state;
        let db = &state.db;
        let stores: HashMap<i64, Store> = Store::all_by_name(db)
            .await?
            .into_iter()
            .map(|s| (s.id, s))
            .collect();
        // Who gets which stores' workbooks.
        let mut people: BTreeMap<i64, (User, Vec<i64>)> = BTreeMap::new();
        for store in &self.stores {
            for user in staff_with_permission(db, catalogue::REPORTS_VIEW, *store).await? {
                people
                    .entry(user.id)
                    .or_insert_with(|| (user, Vec::new()))
                    .1
                    .push(*store);
            }
        }
        let lang = state.current_lang();
        let month = self.month.format("%B %Y").to_string();
        let url = crate::app::rentals::link(state, "reports.monthly", None::<i64>)?;
        for (user, theirs) in people.into_values() {
            let names: Vec<String> = theirs
                .iter()
                .filter_map(|id| stores.get(id).map(|s| s.name.clone()))
                .collect();
            let mut mail = state.mail_view(
                &user.email,
                lang.t(
                    "reports.mail.subject",
                    &[("month", &month as &dyn std::fmt::Display)],
                ),
                "mail/reports/monthly",
                context! { name => &user.name, month => &month, stores => &names, url => &url },
            )?;
            for id in &theirs {
                let Some(store) = stores.get(id) else {
                    continue;
                };
                let Some(bytes) = state.storage.get(&file_key(self.month, *id)).await? else {
                    continue;
                };
                mail = mail.attach(
                    file_name(self.month, &store.slug),
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                    bytes.to_vec(),
                );
            }
            state.mailer.send(mail).await?;
        }
        Ok(())
    }
}

/// One store's month as an Excel workbook: a summary, its income as the
/// owner (its books), its income as the operator (its work), and its
/// statement between stores.
pub async fn workbook(state: &AppState, store_id: i64, month: NaiveDate) -> Result<Vec<u8>> {
    let db = &state.db;
    let from = day_start(&state.config, month);
    let until = day_start(&state.config, next_month(month));
    let names: HashMap<i64, String> = Store::all_by_name(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    let in_month = |q: renox::db::Query<RevenueLine>| {
        q.where_(RevenueLine::BOOKED_AT.gte(from))
            .where_(RevenueLine::BOOKED_AT.lt(until))
            .order_by(RevenueLine::BOOKED_AT)
            .order_by(RevenueLine::ID)
    };
    let books = in_month(RevenueLine::query().where_(RevenueLine::OWNER_STORE_ID.eq(store_id)))
        .get(db)
        .await?;
    let work = in_month(RevenueLine::query().where_(RevenueLine::OPERATING_STORE_ID.eq(store_id)))
        .get(db)
        .await?;
    let entries = IntercompanyEntry::query()
        .where_any(|q| {
            q.where_(IntercompanyEntry::DEBTOR_STORE_ID.eq(store_id))
                .where_(IntercompanyEntry::CREDITOR_STORE_ID.eq(store_id))
        })
        .where_(IntercompanyEntry::BOOKED_AT.gte(from))
        .where_(IntercompanyEntry::BOOKED_AT.lt(until))
        .order_by(IntercompanyEntry::BOOKED_AT)
        .order_by(IntercompanyEntry::ID)
        .get(db)
        .await?;
    let report = Report {
        store: names.get(&store_id).cloned().unwrap_or_default(),
        store_id,
        month,
        names,
        books,
        work,
        entries,
        decimals: renox::currency_decimals(&state.config.currency),
        lang: state.current_lang(),
        zone: state.config.timezone,
    };
    report
        .write()
        .map_err(|e| Error::from(renox::anyhow::anyhow!("the workbook: {e}")))
}

/// What one store's workbook holds.
struct Report {
    store: String,
    store_id: i64,
    month: NaiveDate,
    names: HashMap<i64, String>,
    books: Vec<RevenueLine>,
    work: Vec<RevenueLine>,
    entries: Vec<IntercompanyEntry>,
    decimals: u32,
    lang: Lang,
    zone: renox::timezone::Zone,
}

impl Report {
    fn t(&self, key: &str) -> String {
        self.lang.t(&format!("reports.workbook.{key}"), &[])
    }

    fn money(&self, amount: i64) -> f64 {
        amount as f64 / 10f64.powi(self.decimals as i32)
    }

    fn write(&self) -> std::result::Result<Vec<u8>, XlsxError> {
        let bold = Format::new().set_bold();
        let money = Format::new().set_num_format(if self.decimals == 0 {
            "#,##0"
        } else {
            "#,##0.00"
        });
        let date = Format::new().set_num_format("yyyy-mm-dd hh:mm");
        let mut book = Workbook::new();

        // The summary: income per stream both ways, the books between stores.
        let sheet = book.add_worksheet();
        sheet.set_name(self.t("summary"))?;
        sheet.write_string_with_format(0, 0, &self.store, &bold)?;
        sheet.write_string(1, 0, self.month.format("%B %Y").to_string())?;
        let head = [self.t("stream"), self.t("books"), self.t("work")];
        for (col, label) in head.iter().enumerate() {
            sheet.write_string_with_format(3, col as u16, label, &bold)?;
        }
        let mut row = 4;
        let sum = |lines: &[RevenueLine], stream: &str| -> i64 {
            lines
                .iter()
                .filter(|l| l.stream == stream)
                .map(|l| l.amount)
                .sum()
        };
        for stream in STREAMS {
            sheet.write_string(
                row,
                0,
                self.lang.t(&format!("reports.stream.{stream}"), &[]),
            )?;
            sheet.write_number_with_format(row, 1, self.money(sum(&self.books, stream)), &money)?;
            sheet.write_number_with_format(row, 2, self.money(sum(&self.work, stream)), &money)?;
            row += 1;
        }
        let total = |lines: &[RevenueLine]| lines.iter().map(|l| l.amount).sum::<i64>();
        sheet.write_string_with_format(row, 0, self.t("total"), &bold)?;
        sheet.write_number_with_format(row, 1, self.money(total(&self.books)), &money)?;
        sheet.write_number_with_format(row, 2, self.money(total(&self.work)), &money)?;
        row += 2;
        let owed_to: i64 = self
            .entries
            .iter()
            .filter(|e| e.creditor_store_id == self.store_id)
            .map(|e| e.amount)
            .sum();
        let owed_by: i64 = self
            .entries
            .iter()
            .filter(|e| e.debtor_store_id == self.store_id)
            .map(|e| e.amount)
            .sum();
        for (label, amount) in [
            (self.t("owed_to"), owed_to),
            (self.t("owed_by"), owed_by),
            (self.t("net"), owed_to - owed_by),
        ] {
            sheet.write_string(row, 0, label)?;
            sheet.write_number_with_format(row, 1, self.money(amount), &money)?;
            row += 1;
        }
        sheet.set_column_width(0, 28)?;
        sheet.set_column_width(1, 16)?;
        sheet.set_column_width(2, 16)?;

        // Its books: income from its own bikes and goods, wherever earned.
        let sheet = book.add_worksheet();
        sheet.set_name(self.t("books"))?;
        self.lines(sheet, &self.books, "operating", &bold, &money, &date)?;
        // Its work: income it earned serving customers, whoever owns the bikes.
        let sheet = book.add_worksheet();
        sheet.set_name(self.t("work"))?;
        self.lines(sheet, &self.work, "owner_store", &bold, &money, &date)?;

        // Its statement between stores.
        let sheet = book.add_worksheet();
        sheet.set_name(self.t("intercompany"))?;
        let head = [
            self.t("date"),
            self.t("kind"),
            self.t("debtor"),
            self.t("creditor"),
            self.t("amount"),
            self.t("signed"),
            self.t("document"),
        ];
        for (col, label) in head.iter().enumerate() {
            sheet.write_string_with_format(0, col as u16, label, &bold)?;
        }
        for (i, e) in self.entries.iter().enumerate() {
            let row = i as u32 + 1;
            let local = self.zone.local(e.booked_at.timestamp());
            sheet.write_datetime_with_format(row, 0, local, &date)?;
            sheet.write_string(
                row,
                1,
                self.lang
                    .t(&format!("multistore.books.kind.{}", e.kind.as_str()), &[]),
            )?;
            sheet.write_string(row, 2, self.name(e.debtor_store_id))?;
            sheet.write_string(row, 3, self.name(e.creditor_store_id))?;
            sheet.write_number_with_format(row, 4, self.money(e.amount), &money)?;
            let signed = if e.creditor_store_id == self.store_id {
                e.amount
            } else {
                -e.amount
            };
            sheet.write_number_with_format(row, 5, self.money(signed), &money)?;
            sheet.write_string(row, 6, format!("{} #{}", e.source_type, e.source_id))?;
        }
        sheet.set_freeze_panes(1, 0)?;
        sheet.set_column_width(0, 18)?;
        sheet.set_column_width(1, 22)?;
        book.save_to_buffer()
    }

    fn name(&self, id: i64) -> String {
        self.names.get(&id).cloned().unwrap_or_default()
    }

    /// A sheet of income lines: when, stream, document, the other store, amount.
    fn lines(
        &self,
        sheet: &mut Worksheet,
        lines: &[RevenueLine],
        other: &str,
        bold: &Format,
        money: &Format,
        date: &Format,
    ) -> std::result::Result<(), XlsxError> {
        let head = [
            self.t("date"),
            self.t("stream"),
            self.t("document"),
            self.t(other),
            self.t("amount"),
        ];
        for (col, label) in head.iter().enumerate() {
            sheet.write_string_with_format(0, col as u16, label, bold)?;
        }
        for (i, line) in lines.iter().enumerate() {
            let row = i as u32 + 1;
            let local = self.zone.local(line.booked_at.timestamp());
            sheet.write_datetime_with_format(row, 0, local, date)?;
            sheet.write_string(
                row,
                1,
                self.lang.t(&format!("reports.stream.{}", line.stream), &[]),
            )?;
            sheet.write_string(row, 2, format!("{} #{}", line.source_type, line.source_id))?;
            let store = if other == "owner_store" {
                line.owner_store_id
            } else {
                line.operating_store_id
            };
            sheet.write_string(row, 3, self.name(store))?;
            sheet.write_number_with_format(row, 4, self.money(line.amount), money)?;
        }
        let last = lines.len() as u32 + 1;
        sheet.write_string_with_format(last, 0, self.t("total"), bold)?;
        sheet.write_number_with_format(
            last,
            4,
            self.money(lines.iter().map(|l| l.amount).sum()),
            money,
        )?;
        sheet.set_freeze_panes(1, 0)?;
        sheet.set_column_width(0, 18)?;
        sheet.set_column_width(2, 22)?;
        sheet.set_column_width(3, 18)?;
        sheet.set_column_width(4, 16)?;
        Ok(())
    }
}

/// One run of the monthly report, as the page shows it.
#[derive(Serialize, Debug, Clone)]
pub struct Run {
    pub id: i64,
    /// `2026-09`.
    pub month: String,
    /// `September 2026`.
    pub month_name: String,
    /// The run's stores the person may see, with their files.
    pub stores: Vec<RunStore>,
    pub total: i64,
    pub done: i64,
    pub failed: i64,
    pub finished: bool,
    pub cancelled: bool,
    pub started_at: DateTime,
}

/// A store of a run.
#[derive(Serialize, Debug, Clone)]
pub struct RunStore {
    pub id: i64,
    pub name: String,
}

/// A row of `job_batches` (Renox's queue table), as far as a run needs it.
#[derive(FromRow, Debug)]
struct BatchRow {
    id: i64,
    name: String,
    total: i64,
    pending: i64,
    failed: i64,
    cancelled_at: Option<i64>,
    finished_at: Option<i64>,
    created_at: i64,
}

/// The latest runs (at most 12) that include one of the person's stores,
/// read from `job_batches` in one query.
pub async fn runs(db: &Db, reach: &Reach) -> Result<Vec<Run>> {
    let rows: Vec<BatchRow> = sql(
        "SELECT id, name, total, pending, failed, cancelled_at, finished_at, created_at \
         FROM job_batches WHERE name LIKE ? ORDER BY id DESC LIMIT 24",
    )
    .bind(format!("{BATCH_PREFIX}%"))
    .fetch_as(db)
    .await?;
    let mut runs = Vec::new();
    for BatchRow {
        id,
        name,
        total,
        pending,
        failed,
        cancelled_at,
        finished_at,
        created_at,
    } in rows
    {
        let Some((month, stores)) = parse_name(&name) else {
            continue;
        };
        let stores: Vec<RunStore> = stores
            .into_iter()
            .filter(|s| reach.stores.iter().any(|r| r.id == *s))
            .map(|s| RunStore {
                id: s,
                name: reach.name_of(s),
            })
            .collect();
        if stores.is_empty() {
            continue;
        }
        runs.push(Run {
            id,
            month: month.format("%Y-%m").to_string(),
            month_name: month.format("%B %Y").to_string(),
            stores,
            total,
            done: total - pending,
            failed,
            finished: finished_at.is_some(),
            cancelled: cancelled_at.is_some(),
            started_at: renox::chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default(),
        });
        if runs.len() == 12 {
            break;
        }
    }
    Ok(runs)
}

/// `monthly-report:2026-09:1-2-3` → (2026-09-01, [1, 2, 3]).
pub fn parse_name(name: &str) -> Option<(NaiveDate, Vec<i64>)> {
    let rest = name.strip_prefix(BATCH_PREFIX)?;
    let (month, stores) = rest.split_once(':')?;
    let month = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").ok()?;
    let stores = stores.split('-').filter_map(|s| s.parse().ok()).collect();
    Some((month, stores))
}

/// `GET /staff/reports/monthly` (`reports.monthly`): start a run, follow
/// the runs.
pub async fn index(State(db): State<Db>, user: AuthUser) -> Result<View> {
    let reach = Reach::of(&db, &user, None).await?;
    let runs = runs(&db, &reach).await?;
    let running = runs.iter().any(|r| !r.finished && !r.cancelled);
    let today = renox::db::now().date_naive();
    let last = month_of(month_of(today) - Duration::days(1));
    Ok(view(
        "reports/monthly.html",
        context! {
            runs,
            running,
            reach,
            month => last.format("%Y-%m").to_string(),
            max => month_of(today).format("%Y-%m").to_string(),
            tab => "monthly",
            tabs => super::grids::tabs(),
        },
    ))
}

/// `GET /staff/reports/monthly/runs` (`reports.monthly.runs`): the runs'
/// list alone, which the page's widget reloads every few seconds while a
/// run is going.
pub async fn runs_fragment(State(db): State<Db>, user: AuthUser) -> Result<View> {
    let reach = Reach::of(&db, &user, None).await?;
    let runs = runs(&db, &reach).await?;
    Ok(view("reports/_runs.html", context! { runs }))
}

/// The month to run.
#[derive(Deserialize, Validate, Debug)]
pub struct RunForm {
    /// `2026-09` (an `<input type="month">`).
    #[validate(required, max = 7)]
    pub month: Option<String>,
}

/// `POST /staff/reports/monthly` (`reports.monthly.run`): runs the month
/// for the stores where the person holds `reports.view`.
// [explain:reports.monthly.handler]
pub async fn run(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    Valid(form): Valid<RunForm>,
) -> Result<(Toast, Redirect)> {
    let text = form.month.unwrap_or_default();
    let month = NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d").ok();
    let today = renox::db::now().date_naive();
    let Some(month) = month.filter(|m| *m <= month_of(today)) else {
        let mut errors = Errors::new();
        errors.add("month", lang.t("reports.monthly.invalid", &[]));
        return Err(ValidationError::new(errors).into());
    };
    let reach = Reach::of(&state.db, &user, None).await?;
    if reach.is_empty() {
        return Err(Error::Forbidden);
    }
    start(&state, month, &reach.chosen).await?;
    // [/explain:reports.monthly.handler]
    Ok((
        Toast::success(lang.t(
            "reports.monthly.started",
            &[(
                "month",
                &month.format("%B %Y").to_string() as &dyn std::fmt::Display,
            )],
        )),
        Redirect::route("reports.monthly", &[])?,
    ))
}

/// `GET /staff/reports/monthly/{month}/{store}` (`reports.monthly.file`):
/// one store's workbook, for people who hold `reports.view` in that store
/// (a 404 for anyone else, like any other store's record).
pub async fn file(
    State(state): State<AppState>,
    user: AuthUser,
    Path((month, store)): Path<(String, i64)>,
) -> Result<Download> {
    let month = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| Error::NotFound)?;
    if !can_in(&user, catalogue::REPORTS_VIEW, store) {
        return Err(Error::NotFound);
    }
    let store = Store::find_or_404(&state.db, store).await?;
    Download::from_storage(
        &state.storage,
        &file_key(month, store.id),
        file_name(month, &store.slug),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_named_after_its_month_and_stores() {
        let (month, stores) = parse_name("monthly-report:2026-09:1-2-3").unwrap();
        assert_eq!(month, NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());
        assert_eq!(stores, vec![1, 2, 3]);
        assert!(parse_name("settlement-statements").is_none());
        assert_eq!(
            file_key(month, 2),
            "reports/monthly/2026-09/2.xlsx".to_owned()
        );
    }
}
