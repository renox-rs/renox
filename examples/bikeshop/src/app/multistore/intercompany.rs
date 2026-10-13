//! The books between stores, as pages: balances, the entries, and the
//! stores' fee rates.
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/books` | `multistore.books` | `intercompany.view` |
//! | `GET /staff/books/fees` | `multistore.fees` | `intercompany.view` (changing a rate: `settings.fees`) |
//! | `POST /staff/books/fees/{store}` | `multistore.fees.update` | `settings.fees` (the owner's global role) |
//!
//! Every list goes through `access::visible::<IntercompanyEntry>`: a store
//! sees the entries where it owes or is owed; the owner (a global role)
//! sees all of them.

use std::collections::{BTreeMap, HashMap};

use renox::grid::{Column, Grid, GridRequest, Summary};
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::audit;
use super::model::{EntryKind, IntercompanyEntry};
use crate::app::access::{self, active_store, catalogue};
use crate::app::staff::model::Store;

/// The entries grid: when, why, who owes whom, how much (summed), the
/// rate in force, the document.
pub fn entries_grid(lang: &Lang) -> Grid {
    let t = |key: &str| lang.t(&format!("multistore.books.fields.{key}"), &[]);
    let kinds: Vec<(String, String)> = EntryKind::ALL
        .iter()
        .map(|k| {
            (
                k.as_str().to_owned(),
                lang.t(&format!("multistore.books.kind.{}", k.as_str()), &[]),
            )
        })
        .collect();
    Grid::new("books")
        .title(&lang.t("multistore.books.entries", &[]))
        .column(Column::datetime("booked_at", &t("booked_at")).mobile())
        .column(Column::select("kind", &t("kind"), kinds).mobile())
        .column(Column::related(
            "debtor",
            &t("debtor"),
            "stores",
            "debtor_store_id",
            "name",
        ))
        .column(Column::related(
            "creditor",
            &t("creditor"),
            "stores",
            "creditor_store_id",
            "name",
        ))
        .column(
            Column::money("amount", &t("amount"))
                .summary(Summary::Sum)
                .mobile(),
        )
        .column(Column::number("fee_rate_bp", &t("rate")))
        .column(Column::custom("source", &t("source")))
        .groups(&["kind"])
        .sort_by("-booked_at")
        .per_page(25)
        .exports()
        .cards_on_mobile()
        .empty_state(&lang.t("multistore.books.empty", &[]), None)
}

/// What one store pair owes, net, over entries not settled yet.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Balance {
    pub debtor_store_id: i64,
    pub creditor_store_id: i64,
    pub debtor: String,
    pub creditor: String,
    pub amount: i64,
}

// [explain:multistore.books.netting]
/// The open balance of every store pair in `entries` (unsettled ones):
/// each pair's entries netted, in the direction of the debt. Pairs that net
/// to zero are left out.
pub fn balances(entries: &[(i64, i64, i64)], stores: &HashMap<i64, String>) -> Vec<Balance> {
    let mut nets: BTreeMap<(i64, i64), i64> = BTreeMap::new();
    for (debtor, creditor, amount) in entries {
        let (low, high) = (*debtor.min(creditor), *debtor.max(creditor));
        *nets.entry((low, high)).or_default() += if debtor == &low { *amount } else { -amount };
    }
    nets.into_iter()
        .filter(|(_, net)| *net != 0)
        .map(|((low, high), net)| {
            let (debtor, creditor) = if net > 0 { (low, high) } else { (high, low) };
            Balance {
                debtor_store_id: debtor,
                creditor_store_id: creditor,
                debtor: stores.get(&debtor).cloned().unwrap_or_default(),
                creditor: stores.get(&creditor).cloned().unwrap_or_default(),
                amount: net.abs(),
            }
        })
        .collect()
}
// [/explain:multistore.books.netting]

/// Each store's position (owed to it minus owed by it) over `entries`:
/// added up over every store it is zero, the company's own total.
pub fn positions(entries: &[(i64, i64, i64)]) -> BTreeMap<i64, i64> {
    let mut positions: BTreeMap<i64, i64> = BTreeMap::new();
    for (debtor, creditor, amount) in entries {
        *positions.entry(*debtor).or_default() -= amount;
        *positions.entry(*creditor).or_default() += amount;
    }
    positions
}

// [explain:multistore.books.handler]
/// `GET /staff/books` (`multistore.books`): what the active store owes and
/// is owed, pair by pair (entries not settled yet), and every entry it may
/// see in a grid with the total under the amounts.
pub async fn index(
    State(state): State<AppState>,
    lang: Lang,
    request: GridRequest,
) -> Result<Response> {
    let db = &state.db;
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let entries = || access::visible::<IntercompanyEntry>(catalogue::INTERCOMPANY_VIEW);
    let grid = entries_grid(&lang);
    if let Some(file) = grid.export(entries(), &request).await? {
        return Ok(file);
    }
    let page = grid.page(entries(), &request).await?;
    let page = page.extend(|e| {
        json!({ "source_url": source_url(&e.source_type, e.source_id), "source_label": format!("{} #{}", e.source_type, e.source_id) })
    });
    let open: Vec<(i64, i64, i64)> = entries()
        .where_(IntercompanyEntry::SETTLEMENT_ID.is_null())
        .group_by("debtor_store_id")
        .group_by("creditor_store_id")
        .select_as(
            db,
            "debtor_store_id, creditor_store_id, CAST(SUM(amount) AS BIGINT)",
        )
        .await?;
    // [/explain:multistore.books.handler]
    let stores: HashMap<i64, String> = Store::all_by_name(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    let balances = balances(&open, &stores);
    let mine = positions(&open).get(&store).copied().unwrap_or(0);
    Ok(view(
        "multistore/books/index.html",
        context! { entries => page, balances, mine, store_name => stores.get(&store).cloned() },
    )
    .into_response())
}

/// The staff page of what an entry was booked for.
pub fn source_url(kind: &str, id: i64) -> Option<String> {
    match kind {
        "orders" => Some(format!("/staff/orders/{id}")),
        "rentals" => Some(format!("/staff/rentals/{id}")),
        "work_orders" => Some(format!("/staff/workshop/{id}")),
        "stock_movements" => None,
        _ => None,
    }
}

/// `GET /staff/books/fees` (`multistore.fees`): each store's fee rate (what
/// it earns for work done for another store); only the owner changes them.
pub async fn fees(State(db): State<Db>, user: AuthUser, lang: Lang) -> Result<View> {
    let stores = Store::all_by_name(&db).await?;
    let history = renox::audit::latest(&db, 200)
        .await?
        .into_iter()
        .filter(|e| e.action == "store.fee_rate_changed")
        .take(20)
        .collect::<Vec<_>>();
    let users = User::find_many(
        &db,
        history.iter().filter_map(|e| e.user_id).collect::<Vec<_>>(),
    )
    .await?;
    let names: HashMap<i64, String> = users.into_iter().map(|u| (u.id, u.name)).collect();
    let bp = |value: &renox::serde_json::Value| percent(value.as_i64().unwrap_or(0));
    // The timeline block's items (`<rx-history :items>`), titled in the page's language.
    let timeline: Vec<_> = history
        .into_iter()
        .map(|e| {
            let store = e.data["store"].as_str().unwrap_or_default().to_owned();
            let (from, to) = (bp(&e.data["from_bp"]), bp(&e.data["to_bp"]));
            json!({
                "time": e.created_at,
                "title": lang.t("multistore.fees.changed", &[("store", &store), ("from", &from), ("to", &to)]),
                "by": e.user_id.and_then(|id| names.get(&id).cloned()),
                "kind": "info",
            })
        })
        .collect();
    let rates: Vec<_> = stores
        .iter()
        .map(|s| json!({ "store": s, "percent": percent(s.fee_rate_bp), "value": s.fee_rate_bp as f64 / 100.0 }))
        .collect();
    let can_change = user.has_permission(catalogue::SETTINGS_FEES);
    Ok(view(
        "multistore/books/fees.html",
        context! { rates, timeline, can_change },
    ))
}

/// Basis points as a percentage for people: `2000` → `20`, `2250` → `22.5`.
pub fn percent(bp: i64) -> String {
    let text = format!("{:.2}", bp as f64 / 100.0);
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A new fee rate, in percent with up to two decimals (`20`, `22.5`).
#[derive(Deserialize, Validate, Debug)]
pub struct FeeForm {
    #[validate(required, min = 0, max = 50)]
    pub percent: Option<f64>,
}

// [explain:multistore.fees.handler]
/// `POST /staff/books/fees/{store}` (`multistore.fees.update`): the owner
/// changes a store's fee rate; the change is audited (old and new rate).
/// Entries already booked keep the rate they were booked with.
pub async fn update_fee(
    State(state): State<AppState>,
    user: AuthUser,
    Found(store): Found<Store>,
    Valid(form): Valid<FeeForm>,
) -> Result<(Toast, Redirect)> {
    if !user.has_permission(catalogue::SETTINGS_FEES) {
        return Err(Error::Forbidden);
    }
    let rate = (form.percent.unwrap_or(0.0) * 100.0).round() as i64;
    let old = store.fee_rate_bp;
    Store::query()
        .where_(Store::ID.eq(store.id))
        .update(&state.db, &[("fee_rate_bp", &rate)])
        .await?;
    audit::record(
        &state,
        &user,
        "store.fee_rate_changed",
        store.id,
        (Store::TABLE, store.id),
        json!({ "store": store.name, "from_bp": old, "to_bp": rate }),
    )
    .await?;
    Ok((
        Toast::success(state.current_lang().t("multistore.fees.saved", &[])),
        Redirect::route("multistore.fees", &[])?,
    ))
}
// [/explain:multistore.fees.handler]

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_nets_both_ways_and_the_company_adds_up_to_nothing() {
        let entries = [(2, 1, 900), (1, 2, 180), (3, 1, 50), (1, 3, 50)];
        let stores = HashMap::new();
        let b = balances(&entries, &stores);
        assert_eq!(b.len(), 1);
        assert_eq!(
            (b[0].debtor_store_id, b[0].creditor_store_id, b[0].amount),
            (2, 1, 720)
        );
        let p = positions(&entries);
        assert_eq!(p[&1], 720);
        assert_eq!(p.values().sum::<i64>(), 0);
    }
}
