//! Monthly settlements between stores: statements, mailed, confirmed by both.
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/books/settlements` | `multistore.settlements` | `intercompany.view` |
//! | `GET /staff/books/settlements/{settlement}` | `multistore.settlements.show` | `intercompany.view` in one of its two stores |
//! | `POST /staff/books/settlements/{settlement}/confirm` | `multistore.settlements.confirm` | `intercompany.settle` in the confirming store |
//!
//! On the 1st of each month at 02:00 (`books:settle`, `Schedule::monthly_on`)
//! the previous month's unsettled entries are netted **per store pair**
//! into one [`Settlement`] (the store owing on balance is its debtor), the
//! entries point at it (`settlement_id`), and a **queue batch** sends each
//! statement (one `SendStatement` job per pair) to the people who see the
//! books of either store, with the entries attached as a CSV file. Each
//! store then confirms: the paying store that it paid, the other that it
//! was paid; with both, the settlement is settled. The owner, whose global
//! role holds `intercompany.settle` everywhere, can confirm for either.

use std::collections::{BTreeMap, HashMap};

use renox::chrono::{Datelike, Duration, NaiveDate};
use renox::db::sql;
use renox::prelude::*;
use renox::schedule::Schedule;
use serde::{Deserialize, Serialize};

use super::audit;
use super::help::day_start;
use super::model::{IntercompanyEntry, Settlement, SettlementStatus};
use crate::app::access::{self, StoreAttr, can_in, catalogue};
use crate::app::rentals::notify::staff_with_permission;
use crate::app::staff::model::Store;

// [explain:multistore.settlements.month]
/// Registers the monthly task.
pub fn schedule(s: &mut Schedule) {
    s.monthly_on(1, "02:00", "books:settle", |state: AppState| async move {
        settle_previous_month(&state).await.map(|_| ())
    });
}
// [/explain:multistore.settlements.month]

/// The first day of `day`'s month.
pub fn month_of(day: NaiveDate) -> NaiveDate {
    day.with_day(1).expect("the first of the month")
}

/// The first day of the month after `first`.
pub fn next_month(first: NaiveDate) -> NaiveDate {
    month_of(month_of(first) + Duration::days(32))
}

/// Settles the month before today's (in `APP_TIMEZONE`).
pub async fn settle_previous_month(state: &AppState) -> Result<Vec<Settlement>> {
    let today = crate::app::rentals::booking::to_local(&state.config, renox::db::now()).date();
    let previous = month_of(month_of(today) - Duration::days(1));
    settle_month(state, previous).await
}

/// Nets the unsettled entries booked in the month starting `first` into one
/// settlement per store pair (in one transaction), then queues the
/// statements as a batch. Safe to run twice: a second run finds no
/// unsettled entries for the month, and adds to a pair's settlement only
/// entries that arrived since.
// [explain:multistore.settlements.month]
pub async fn settle_month(state: &AppState, first: NaiveDate) -> Result<Vec<Settlement>> {
    let db = &state.db;
    let from = day_start(&state.config, first);
    let until = day_start(&state.config, next_month(first));
    let mut tx = db.begin().await?;
    let sums: Vec<(i64, i64, i64)> = IntercompanyEntry::query()
        .where_(IntercompanyEntry::SETTLEMENT_ID.is_null())
        .where_(IntercompanyEntry::BOOKED_AT.gte(from))
        .where_(IntercompanyEntry::BOOKED_AT.lt(until))
        .group_by("debtor_store_id")
        .group_by("creditor_store_id")
        .select_as(
            &mut tx,
            "debtor_store_id, creditor_store_id, CAST(SUM(amount) AS BIGINT)",
        )
        .await?;
    // [/explain:multistore.settlements.month]
    let mut pairs: BTreeMap<(i64, i64), i64> = BTreeMap::new();
    for (debtor, creditor, amount) in sums {
        let (low, high) = (debtor.min(creditor), debtor.max(creditor));
        *pairs.entry((low, high)).or_default() += if debtor == low { amount } else { -amount };
    }
    let mut settled = Vec::new();
    for ((low, high), _) in pairs {
        let existing = Settlement::query()
            .where_any(|q| {
                q.where_raw("debtor_store_id = ? AND creditor_store_id = ?", [low, high])
                    .where_raw("debtor_store_id = ? AND creditor_store_id = ?", [high, low])
            })
            .where_(Settlement::PERIOD_START.eq(first))
            .first(&mut tx)
            .await?;
        let mut settlement = match existing {
            Some(s) => s,
            None => {
                Settlement::create(
                    &mut tx,
                    Settlement {
                        debtor_store_id: low,
                        creditor_store_id: high,
                        period_start: first,
                        period_end: next_month(first) - Duration::days(1),
                        status: SettlementStatus::Open,
                        ..Default::default()
                    },
                )
                .await?
            }
        };
        sql(
            "UPDATE intercompany_entries SET settlement_id = ? WHERE settlement_id IS NULL \
             AND booked_at >= ? AND booked_at < ? \
             AND ((debtor_store_id = ? AND creditor_store_id = ?) OR (debtor_store_id = ? AND creditor_store_id = ?))",
        )
        .bind(settlement.id)
        .bind(from)
        .bind(until)
        .bind(low)
        .bind(high)
        .bind(high)
        .bind(low)
        .execute(&mut tx)
        .await?;
        // The pair's net over everything the settlement holds now.
        let net = net_of(&mut tx, settlement.id, low).await?;
        let (debtor, creditor) = if net >= 0 { (low, high) } else { (high, low) };
        settlement.debtor_store_id = debtor;
        settlement.creditor_store_id = creditor;
        settlement.amount = net.abs();
        settlement
            .save_only(&mut tx, &["debtor_store_id", "creditor_store_id", "amount"])
            .await?;
        settled.push(settlement);
    }
    // [explain:multistore.settlements.month]
    tx.commit().await?;
    if !settled.is_empty() {
        let mut batch = state.queue.batch("settlement-statements");
        for s in &settled {
            batch = batch.push(SendStatement {
                settlement_id: s.id,
            });
        }
        batch.allow_failures().dispatch().await?;
    }
    Ok(settled)
}
// [/explain:multistore.settlements.month]

/// What `low` owes the other store of settlement `id`, net (negative: it
/// is owed).
async fn net_of(tx: &mut renox::db::Transaction, id: i64, low: i64) -> Result<i64> {
    let rows: Vec<(i64, i64)> = sql(
        "SELECT debtor_store_id, CAST(SUM(amount) AS BIGINT) FROM intercompany_entries \
         WHERE settlement_id = ? GROUP BY debtor_store_id",
    )
    .bind(id)
    .fetch_as(&mut *tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(debtor, sum)| if debtor == low { sum } else { -sum })
        .sum())
}

/// One settlement as the pages show it.
#[derive(Serialize, Debug, Clone)]
pub struct Statement {
    #[serde(flatten)]
    pub settlement: Settlement,
    pub debtor: String,
    pub creditor: String,
    pub debtor_confirmed: bool,
    pub creditor_confirmed: bool,
}

async fn statements(db: &Db, list: Vec<Settlement>) -> Result<Vec<Statement>> {
    let stores: HashMap<i64, String> = Store::all_by_name(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    Ok(list
        .into_iter()
        .map(|s| Statement {
            debtor: stores.get(&s.debtor_store_id).cloned().unwrap_or_default(),
            creditor: stores
                .get(&s.creditor_store_id)
                .cloned()
                .unwrap_or_default(),
            debtor_confirmed: s.debtor_confirmed(),
            creditor_confirmed: s.creditor_confirmed(),
            settlement: s,
        })
        .collect())
}

// [explain:multistore.settlements.handler]
/// `GET /staff/books/settlements` (`multistore.settlements`): the monthly
/// statements the person may see, newest first. Three queries a page.
pub async fn index(State(db): State<Db>, Page(page): Page) -> Result<View> {
    let list = access::visible::<Settlement>(catalogue::INTERCOMPANY_VIEW)
        .order_by_desc(Settlement::PERIOD_START)
        .order_by(Settlement::ID)
        .paginate(&db, page, 25)
        .await?;
    let rows = statements(&db, list.items.clone()).await?;
    Ok(view(
        "multistore/books/settlements.html",
        context! { rows, pages => list },
    ))
}
// [/explain:multistore.settlements.handler]

/// One side of a statement: what one store owes the other, by kind.
#[derive(Serialize, Debug, Clone, Default)]
pub struct Side {
    pub lines: Vec<(String, i64, i64)>,
    pub total: i64,
}

// [explain:multistore.settlements.show.handler]
/// `GET /staff/books/settlements/{settlement}` (`multistore.settlements.show`):
/// the two-party statement: each store's side by kind, the net, the
/// entries, and each store's confirmation.
pub async fn show(State(db): State<Db>, user: AuthUser, Path(id): Path<i64>) -> Result<View> {
    let settlement = access::find::<Settlement>(&db, &user, id).await?;
    let entries = IntercompanyEntry::query()
        .where_(IntercompanyEntry::SETTLEMENT_ID.eq(settlement.id))
        .order_by(IntercompanyEntry::BOOKED_AT)
        .order_by(IntercompanyEntry::ID)
        .get(&db)
        .await?;
    let mut sides: HashMap<i64, BTreeMap<String, (i64, i64)>> = HashMap::new();
    for e in &entries {
        let line = sides
            .entry(e.debtor_store_id)
            .or_default()
            .entry(e.kind.as_str().to_owned())
            .or_default();
        line.0 += 1;
        line.1 += e.amount;
    }
    let side = |store: i64| {
        let lines: Vec<(String, i64, i64)> = sides
            .get(&store)
            .map(|m| m.iter().map(|(k, (n, a))| (k.clone(), *n, *a)).collect())
            .unwrap_or_default();
        Side {
            total: lines.iter().map(|l| l.2).sum(),
            lines,
        }
    };
    let debtor_side = side(settlement.debtor_store_id);
    let creditor_side = side(settlement.creditor_store_id);
    // [/explain:multistore.settlements.show.handler]
    let can_debtor = !settlement.debtor_confirmed()
        && can_in(
            &user,
            catalogue::INTERCOMPANY_SETTLE,
            settlement.debtor_store_id,
        );
    let can_creditor = !settlement.creditor_confirmed()
        && can_in(
            &user,
            catalogue::INTERCOMPANY_SETTLE,
            settlement.creditor_store_id,
        );
    let entries: Vec<_> = entries
        .into_iter()
        .map(|e| {
            json!({
                "url": super::intercompany::source_url(&e.source_type, e.source_id),
                "entry": e,
            })
        })
        .collect();
    let mut statement = statements(&db, vec![settlement]).await?;
    let statement = statement.remove(0);
    Ok(view(
        "multistore/books/statement.html",
        context! { statement, debtor_side, creditor_side, entries, can_debtor, can_creditor },
    ))
}

/// Which side confirms.
#[derive(Deserialize, Validate, Debug)]
pub struct ConfirmForm {
    #[validate(required, one_of(&["debtor", "creditor"]))]
    pub side: String,
}

/// Marks `side` of `settlement` confirmed by `user` (checked:
/// `intercompany.settle` in that side's store); settled once both are.
pub async fn confirm_side(
    state: &AppState,
    user: &User,
    settlement: &mut Settlement,
    side: &str,
) -> Result {
    let (store, at, by) = if side == "debtor" {
        (
            settlement.debtor_store_id,
            "debtor_confirmed_at",
            "debtor_confirmed_by",
        )
    } else {
        (
            settlement.creditor_store_id,
            "creditor_confirmed_at",
            "creditor_confirmed_by",
        )
    };
    let attr = if side == "debtor" {
        StoreAttr::Location
    } else {
        StoreAttr::Owner
    };
    // [explain:multistore.settlements.show.confirm]
    access::require(user, catalogue::INTERCOMPANY_SETTLE, attr, &*settlement)?;
    let now = renox::db::now();
    let changed = Settlement::query()
        .where_(Settlement::ID.eq(settlement.id))
        .where_(Settlement::STATUS.eq(SettlementStatus::Open))
        .where_null(at)
        .update(
            &state.db,
            &[
                (at, &now as &(dyn renox::db::ToDbValue + Sync)),
                (by, &user.id),
            ],
        )
        .await?;
    if changed == 0 {
        return Err(abort(
            StatusCode::CONFLICT,
            state
                .current_lang()
                .t("multistore.settlements.errors.done", &[]),
        ));
    }
    *settlement = Settlement::find_or_404(&state.db, settlement.id).await?;
    if settlement.debtor_confirmed_at.is_some() && settlement.creditor_confirmed_at.is_some() {
        settlement.status = SettlementStatus::Settled;
        settlement.settled_at = Some(now);
        settlement.settled_by = Some(user.id);
        settlement
            .save_only(&state.db, &["status", "settled_at", "settled_by"])
            .await?;
    }
    // [/explain:multistore.settlements.show.confirm]
    audit::record(
        state,
        user,
        &format!("settlement.confirmed_by_{side}"),
        store,
        (Settlement::TABLE, settlement.id),
        json!({ "amount": settlement.amount, "status": settlement.status.as_str() }),
    )
    .await
}

/// `POST /staff/books/settlements/{settlement}/confirm` (`multistore.settlements.confirm`).
pub async fn confirm(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<ConfirmForm>,
) -> Result<(Toast, Redirect)> {
    let mut settlement = access::find::<Settlement>(&state.db, &user, id).await?;
    confirm_side(&state, &user, &mut settlement, &form.side).await?;
    Ok((
        Toast::success(state.current_lang().t(
            if settlement.status == SettlementStatus::Settled {
                "multistore.settlements.settled"
            } else {
                "multistore.settlements.confirmed"
            },
            &[],
        )),
        Redirect::route("multistore.settlements.show", &[&settlement.id])?,
    ))
}

/// Mails one statement to everyone who sees either store's books (the two
/// managers and the owner), with its entries as a CSV attachment. One job
/// per store pair, run as a batch.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SendStatement {
    pub settlement_id: i64,
}

impl Job for SendStatement {
    const NAME: &'static str = "books-send-statement";

    async fn handle(self, ctx: JobContext) -> Result {
        let state = &ctx.state;
        let db = &state.db;
        let Some(settlement) = Settlement::find(db, self.settlement_id).await? else {
            return Ok(());
        };
        let entries = IntercompanyEntry::query()
            .where_(IntercompanyEntry::SETTLEMENT_ID.eq(settlement.id))
            .order_by(IntercompanyEntry::BOOKED_AT)
            .get(db)
            .await?;
        let stores: HashMap<i64, String> = Store::all_by_name(db)
            .await?
            .into_iter()
            .map(|s| (s.id, s.name))
            .collect();
        let name = |id: i64| stores.get(&id).cloned().unwrap_or_default();
        let csv = statement_csv(&entries, &stores);
        let lang = state.current_lang();
        let url =
            crate::app::rentals::link(state, "multistore.settlements.show", Some(settlement.id))?;
        let mut sent: Vec<i64> = Vec::new();
        for store in [settlement.debtor_store_id, settlement.creditor_store_id] {
            for user in staff_with_permission(db, catalogue::INTERCOMPANY_VIEW, store).await? {
                if sent.contains(&user.id) {
                    continue;
                }
                sent.push(user.id);
                let mail = state
                    .mail_view(
                        &user.email,
                        lang.t(
                            "multistore.mail.statement.subject",
                            &[
                                (
                                    "debtor",
                                    &name(settlement.debtor_store_id) as &dyn std::fmt::Display,
                                ),
                                ("creditor", &name(settlement.creditor_store_id)),
                                ("month", &settlement.period_start.format("%B %Y")),
                            ],
                        ),
                        "mail/multistore/statement",
                        context! {
                            debtor => name(settlement.debtor_store_id),
                            creditor => name(settlement.creditor_store_id),
                            month => settlement.period_start.format("%B %Y").to_string(),
                            amount => settlement.amount,
                            entries => entries.len(),
                            url => &url,
                        },
                    )?
                    .attach(
                        format!("statement-{}.csv", settlement.period_start.format("%Y-%m")),
                        "text/csv; charset=utf-8",
                        csv.clone(),
                    );
                state.mailer.send(mail).await?;
            }
        }
        Settlement::query()
            .where_(Settlement::ID.eq(settlement.id))
            .update(db, &[("mailed_at", &renox::db::now())])
            .await?;
        Ok(())
    }
}

/// The entries of a statement as CSV: date, kind, who owes whom, amount, rate, source.
pub fn statement_csv(entries: &[IntercompanyEntry], stores: &HashMap<i64, String>) -> String {
    let name = |id: i64| {
        stores
            .get(&id)
            .cloned()
            .unwrap_or_default()
            .replace(',', " ")
    };
    let mut csv =
        String::from("\u{feff}booked_at,kind,debtor,creditor,amount,fee_rate_bp,source\r\n");
    for e in entries {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{} #{}\r\n",
            e.booked_at.format("%Y-%m-%d %H:%M"),
            e.kind.as_str(),
            name(e.debtor_store_id),
            name(e.creditor_store_id),
            e.amount,
            e.fee_rate_bp.map(|r| r.to_string()).unwrap_or_default(),
            e.source_type,
            e.source_id
        ));
    }
    csv
}
