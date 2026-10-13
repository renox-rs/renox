//! The stock take: counting a shelf and correcting the books.
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/stock/take` | `stock.take` | `stock.adjust` (in the active store) |
//! | `POST /staff/stock/take` | `stock.take.store` | `stock.adjust` (in the active store) |
//!
//! A take counts what stands **at** the active store (the location), one
//! category at a time: the store's own goods and the goods it holds for
//! other stores, listed apart (#245). Counted quantities that differ from
//! the books become `adjustment` movements with the reason. The location
//! store may count consigned goods; when it finds fewer, it owes their
//! owner their cost (`consignment_loss`, booked by `multistore::books`)
//! and the owner store's staff are told. The take is audited.

use std::collections::HashMap;

use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::model::{MovementReason, StockLevel, StockMovement, StockRow};
use super::notify;
use crate::app::access::{self, active_store, catalogue};
use crate::app::multistore::{audit, books};
use crate::app::rentals::notify::{Notice, Tone};
use crate::app::staff::model::{Staff, Store};

/// Why counted stock differs.
pub const REASONS: [&str; 4] = ["count", "damage", "loss", "found"];

/// `?category=` on the page.
#[derive(Deserialize, Default)]
pub struct TakeQuery {
    #[serde(default)]
    pub category: Option<String>,
}

/// A row of the count sheet.
#[derive(Serialize, Debug, Clone)]
pub struct SheetRow {
    #[serde(flatten)]
    pub row: StockRow,
    pub owner: String,
}

/// `GET /staff/stock/take` (`stock.take`): the count sheet of one category
/// at the active store, own goods first, then goods held for other stores.
pub async fn sheet(
    State(db): State<Db>,
    lang: Lang,
    Query(query): Query<TakeQuery>,
) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let here = || {
        access::visible::<StockRow>(catalogue::STOCK_VIEW)
            .where_(StockRow::LOCATION_STORE_ID.eq(store))
    };
    // The categories with stock here, for the picker (one query).
    let mut categories: Vec<String> = here()
        .get(&db)
        .await?
        .into_iter()
        .map(|r| r.category)
        .collect();
    categories.sort();
    categories.dedup();
    let category = query
        .category
        .filter(|c| categories.contains(c))
        .or_else(|| categories.first().cloned());
    let rows = here()
        .where_(StockRow::CATEGORY.eq(category.clone().unwrap_or_default()))
        .order_by(StockRow::PRODUCT)
        .order_by(StockRow::SKU)
        .get(&db)
        .await?;
    let stores: HashMap<i64, String> = Store::all_by_name(&db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    let (own, held): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .map(|row| SheetRow {
            owner: stores.get(&row.owner_store_id).cloned().unwrap_or_default(),
            row,
        })
        .partition(|r| r.row.owner_store_id == store);
    // The reasons as (key, label) pairs for the select.
    let reasons: Vec<(&str, String)> = REASONS
        .iter()
        .map(|key| (*key, lang.t(&format!("stock.take.reason.{key}"), &[])))
        .collect();
    Ok(view(
        "stock/take.html",
        context! { own, held, categories, category, reasons },
    ))
}

// [explain:stock.take.form]
/// One counted line: `lines[3][level]`, `lines[3][counted]`.
#[derive(Deserialize, Debug, Clone)]
pub struct TakeLine {
    pub level: i64,
    /// Blank: not counted (left as it is).
    pub counted: Option<i64>,
}

impl Validate for TakeLine {
    fn rules(&self, v: &mut Validator) {
        v.field("counted", &self.counted).min(0).max(100_000);
    }
}

/// The count sheet as sent.
#[derive(Deserialize, Debug)]
pub struct TakeForm {
    pub reason: String,
    pub note: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    pub lines: Vec<TakeLine>,
}

impl Validate for TakeForm {
    fn rules(&self, v: &mut Validator) {
        v.field("reason", &self.reason).required().one_of(&REASONS);
        v.field("note", &self.note).max(300);
        v.field("lines", &self.lines).max(2_000);
        v.nested("lines", &self.lines);
    }
}
// [/explain:stock.take.form]

/// What a take changed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TakeResult {
    /// Lines whose count differed from the books.
    pub adjusted: usize,
    /// Consigned units missing, per owner store.
    pub missing: HashMap<i64, Vec<(i64, i64)>>,
}

/// Applies a count at `store` in one transaction: each differing line an
/// `adjustment` movement (the level follows, `StockMovement::record`), and
/// for consigned goods found short a `consignment_loss` entry at cost.
/// Lines for levels not at `store` are refused (a 404, as if they didn't
/// exist).
pub async fn apply(
    state: &AppState,
    user: &User,
    store: i64,
    form: &TakeForm,
) -> Result<TakeResult> {
    let db = &state.db;
    let staff = Staff::of_user(db, user.id).await?.map(|s| s.id);
    let counted: Vec<&TakeLine> = form.lines.iter().filter(|l| l.counted.is_some()).collect();
    let costs: HashMap<i64, i64> = {
        let levels =
            StockLevel::find_many(db, counted.iter().map(|l| l.level).collect::<Vec<_>>()).await?;
        let names =
            super::ledger::variant_names(db, levels.iter().map(|l| l.variant_id).collect()).await?;
        levels
            .iter()
            .map(|l| (l.id, names.get(&l.variant_id).map(|n| n.cost).unwrap_or(0)))
            .collect()
    };
    let note = |diff: i64| {
        let mut text = format!("Stock take ({}): {diff:+}", form.reason);
        if let Some(extra) = form.note.as_deref().filter(|n| !n.trim().is_empty()) {
            text.push_str(" · ");
            text.push_str(extra.trim());
        }
        text
    };
    let mut result = TakeResult::default();
    // [explain:stock.take.apply]
    let mut tx = db.begin().await?;
    for line in counted {
        let Some(level) = StockLevel::find(&mut tx, line.level).await? else {
            return Err(Error::NotFound);
        };
        if level.location_store_id != store || !access::can_see(user, &level) {
            return Err(Error::NotFound);
        }
        let diff = line.counted.unwrap_or(level.on_hand) - level.on_hand;
        if diff == 0 {
            continue;
        }
        let movement = StockMovement::record(
            // [/explain:stock.take.apply]
            &mut tx,
            StockMovement {
                variant_id: level.variant_id,
                owner_store_id: level.owner_store_id,
                location_store_id: level.location_store_id,
                quantity: diff,
                reason: MovementReason::Adjustment,
                staff_id: staff,
                note: Some(note(diff)),
                ..Default::default()
            },
        )
        .await?;
        // [explain:stock.take.apply]
        result.adjusted += 1;
        if level.consigned() && diff < 0 {
            let cost = costs.get(&level.id).copied().unwrap_or(0) * -diff;
            books::consignment_loss(
                &mut tx,
                movement.id,
                level.location_store_id,
                level.owner_store_id,
                cost,
            )
            .await?;
            result
                .missing
                .entry(level.owner_store_id)
                .or_default()
                .push((level.variant_id, -diff));
        }
    }
    tx.commit().await?;
    // [/explain:stock.take.apply]
    Ok(result)
}

/// `POST /staff/stock/take` (`stock.take.store`).
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    back: Back,
    Valid(form): Valid<TakeForm>,
) -> Result<(Toast, Back)> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    if !access::can_in(&user, catalogue::STOCK_ADJUST, store) {
        return Err(Error::Forbidden);
    }
    let result = apply(&state, &user, store, &form).await?;
    audit::record(
        &state,
        &user,
        "stock.take",
        store,
        ("stores", store),
        json!({
            "category": form.category,
            "reason": form.reason,
            "counted": form.lines.iter().filter(|l| l.counted.is_some()).count(),
            "adjusted": result.adjusted,
        }),
    )
    .await?;
    tell_owners(&state, store, &result).await?;
    let lang = state.current_lang();
    let toast = Toast::success(lang.t(
        "stock.take.done",
        &[("count", &result.adjusted as &dyn std::fmt::Display)],
    ));
    Ok((toast, back))
}

/// The owner stores of consigned goods found short are told (mail and the
/// bell), with what is missing.
async fn tell_owners(state: &AppState, holder: i64, result: &TakeResult) -> Result {
    if result.missing.is_empty() {
        return Ok(());
    }
    let holder_name = Store::find(&state.db, holder)
        .await?
        .map(|s| s.name)
        .unwrap_or_default();
    let ids: Vec<i64> = result
        .missing
        .values()
        .flat_map(|v| v.iter().map(|(id, _)| *id))
        .collect();
    let names = super::ledger::variant_names(&state.db, ids).await?;
    for (owner, lines) in &result.missing {
        let mut notice = Notice::new(
            "stock-consigned-short",
            "stock.mail.short.title",
            "stock.mail.short.body",
        )
        .param("store", &holder_name)
        .tone(Tone::Warning)
        .view("mail/stock/notice")
        .url(crate::app::rentals::link(state, "stock.index", None::<i64>)? + "?view=away");
        for (variant, units) in lines {
            let name = names.get(variant).map(|n| n.label()).unwrap_or_default();
            notice = notice.row("stock.fields.missing", format!("{units} × {name}"));
        }
        notify::store_staff(state, catalogue::CONSIGNMENT_MANAGE, *owner, &notice).await?;
    }
    Ok(())
}
