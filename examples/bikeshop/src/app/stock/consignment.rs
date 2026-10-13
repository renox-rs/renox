//! Consignment between stores: goods sent from the store that owns them
//! to another store that sells them, and called back (#240, #245).
//!
//! The owner's decision: goods sent between stores are **always
//! consigned, never transferred**. A shipment changes the goods'
//! **location**; their **owner** stays the store that sent them, until a
//! customer buys them (then the seller owes the owner, minus its fee:
//! `multistore::books::consigned_sale`).
//!
//! | Step | Who (permission, in which store) | Ledger |
//! |---|---|---|
//! | request | the location store (`consignment.manage` there); the owner's own shipment starts approved | – |
//! | approve / refuse | the **owner** store (`consignment.manage`) | – |
//! | ship | the **owner** store (`consignment.manage`) | `consign_out` −q at (owner, owner), guarded |
//! | receive (also partly) | the **location** store (`stock.receive`) | `consign_in` +r at (owner, location) |
//! | ask back (recall) | the **owner** store (`consignment.manage`) | – |
//! | send back | the **location** store (`consignment.manage`) | `recall` −q at (owner, location), guarded |
//! | receive back | the **owner** store (`stock.receive`) | `recall` +q at (owner, owner) |
//!
//! Between "ship" and "receive" the goods are **in transit**: out of the
//! owner's shelf, not yet on the other's; the shipment's lines say how
//! many (`quantity − received_quantity`). Anything never received is the
//! owner's loss (it was still theirs), and they are told.
//!
//! Every step checks the shipment's status in its `UPDATE … WHERE status
//! = ?`, so two people pressing "ship" at once ship once.

use std::collections::HashMap;

use renox::prelude::*;
use renox::validation::FormContext;
use serde::{Deserialize, Serialize};

use super::ledger::{self, store_names, variant_names};
use super::model::{
    ConsignmentShipment, ConsignmentShipmentLine, MovementReason, ShipmentStatus, StockMovement,
    StockRow,
};
use crate::app::access::{self, StoreAttr, active_store, catalogue};
use crate::app::multistore::audit;
use crate::app::rentals::notify::{self, Notice, Tone};
use crate::app::staff::model::{Staff, Store};

/// The list's tabs (`?view=`).
pub const VIEWS: [&str; 3] = ["open", "transit", "all"];

/// The steps a shipment goes through, for the page's stepper.
pub const STEPS: [ShipmentStatus; 7] = [
    ShipmentStatus::Requested,
    ShipmentStatus::Approved,
    ShipmentStatus::Sent,
    ShipmentStatus::Received,
    ShipmentStatus::RecallRequested,
    ShipmentStatus::RecallSent,
    ShipmentStatus::Recalled,
];

fn rank(status: ShipmentStatus) -> usize {
    match status {
        ShipmentStatus::Draft | ShipmentStatus::Requested | ShipmentStatus::Refused => 0,
        ShipmentStatus::Approved => 1,
        ShipmentStatus::Sent | ShipmentStatus::PartlyReceived => 2,
        ShipmentStatus::Received => 3,
        ShipmentStatus::RecallRequested => 4,
        ShipmentStatus::RecallSent => 5,
        ShipmentStatus::Recalled => 6,
    }
}

/// `?view=` on the list.
#[derive(Deserialize, Default)]
pub struct ListQuery {
    #[serde(default)]
    pub view: String,
}

/// A row of the list.
#[derive(Serialize, Debug, Clone)]
pub struct Row {
    #[serde(flatten)]
    pub shipment: ConsignmentShipment,
    pub owner: String,
    pub location: String,
    /// Units on the lines (sent, or asked for).
    pub units: i64,
    /// Units between the stores now.
    pub in_transit: i64,
    /// Whether the active store owns the goods.
    pub ours: bool,
}

fn open_statuses() -> Vec<ShipmentStatus> {
    vec![
        ShipmentStatus::Requested,
        ShipmentStatus::Approved,
        ShipmentStatus::Sent,
        ShipmentStatus::PartlyReceived,
        ShipmentStatus::RecallRequested,
        ShipmentStatus::RecallSent,
    ]
}

/// `GET /staff/consignments` (`stock.consignments`): the active store's
/// shipments, both ways. Three queries a page (count, page, line sums).
// [explain:stock.consignments.handler]
pub async fn index(
    State(db): State<Db>,
    Page(page): Page,
    Query(query): Query<ListQuery>,
) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let tab = if VIEWS.contains(&query.view.as_str()) {
        query.view.clone()
    } else {
        "open".to_owned()
    };
    let shipments = access::visible::<ConsignmentShipment>(catalogue::STOCK_VIEW).where_any(|q| {
        q.where_(ConsignmentShipment::OWNER_STORE_ID.eq(store))
            .where_(ConsignmentShipment::LOCATION_STORE_ID.eq(store))
    });
    // [/explain:stock.consignments.handler]
    let shipments = match tab.as_str() {
        "open" => shipments.where_(ConsignmentShipment::STATUS.is_in(open_statuses())),
        "transit" => shipments.where_(ConsignmentShipment::STATUS.is_in([
            ShipmentStatus::Sent,
            ShipmentStatus::PartlyReceived,
            ShipmentStatus::RecallSent,
        ])),
        _ => shipments,
    };
    // [explain:stock.consignments.handler]
    let shipments = shipments
        .order_by_desc(ConsignmentShipment::UPDATED_AT)
        .order_by_desc(ConsignmentShipment::ID)
        .paginate(&db, page, 25)
        .await?;
    // [/explain:stock.consignments.handler]
    let lines = ConsignmentShipmentLine::query()
        .where_(
            ConsignmentShipmentLine::SHIPMENT_ID
                .is_in(shipments.items.iter().map(|s| s.id).collect::<Vec<_>>()),
        )
        .get(&db)
        .await?;
    let mut sums: HashMap<i64, (i64, i64)> = HashMap::new();
    for line in &lines {
        let entry = sums.entry(line.shipment_id).or_default();
        entry.0 += line.quantity;
        entry.1 += line.quantity - line.received_quantity;
    }
    let stores = store_names(&db).await?;
    let name = |id: i64| stores.get(&id).cloned().unwrap_or_default();
    let shipments = shipments.map(|s| {
        let (units, open) = sums.get(&s.id).copied().unwrap_or_default();
        let in_transit = match s.status {
            ShipmentStatus::Sent | ShipmentStatus::PartlyReceived => open,
            ShipmentStatus::RecallSent => lines
                .iter()
                .filter(|l| l.shipment_id == s.id)
                .map(|l| l.returned_quantity)
                .sum(),
            _ => 0,
        };
        Row {
            owner: name(s.owner_store_id),
            location: name(s.location_store_id),
            ours: s.owner_store_id == store,
            units,
            in_transit,
            shipment: s,
        }
    });
    Ok(view(
        "stock/consignments/index.html",
        context! { shipments, tab, views => VIEWS },
    ))
}

/// `?direction=send|ask&store=…&q=…` on the new-shipment page.
#[derive(Deserialize, Default, Debug)]
pub struct NewQuery {
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub store: Option<i64>,
    #[serde(default)]
    pub q: Option<String>,
    /// A variant to start with (from the reorder mail's "another store has spare").
    #[serde(default)]
    pub variant: Option<i64>,
}

/// `GET /staff/consignments/new` (`stock.consignments.create`): send our
/// goods to another store, or ask another store for its goods. The rows are
/// the owner store's own goods on its shelf, with what is available.
// [explain:stock.consignments.create.handler]
pub async fn create(State(db): State<Db>, Query(query): Query<NewQuery>) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let direction = match query.direction.as_deref() {
        Some("ask") => "ask",
        _ => "send",
    };
    // [/explain:stock.consignments.create.handler]
    let others: Vec<Store> = Store::all_by_name(&db)
        .await?
        .into_iter()
        .filter(|s| s.id != store)
        .collect();
    let other = query
        .store
        .filter(|id| others.iter().any(|s| s.id == *id))
        .or_else(|| others.first().map(|s| s.id))
        .unwrap_or_default();
    // The other stores as (id, name) pairs for the select.
    let stores: Vec<(i64, String)> = others.iter().map(|s| (s.id, s.name.clone())).collect();
    // [explain:stock.consignments.create.handler]
    let owner = if direction == "send" { store } else { other };
    let search = query.q.clone().unwrap_or_default();
    let rows = StockRow::query()
        .where_(StockRow::OWNER_STORE_ID.eq(owner))
        .where_(StockRow::LOCATION_STORE_ID.eq(owner))
        .where_(StockRow::AVAILABLE.gt(0))
        .when(!search.trim().is_empty(), |q| {
            q.where_any(|any| {
                any.where_(StockRow::PRODUCT.like(&format!("%{}%", search.trim())))
                    .where_(StockRow::SKU.like(&format!("%{}%", search.trim())))
            })
        })
        .when(query.variant.is_some(), |q| {
            q.where_(StockRow::VARIANT_ID.eq(query.variant.unwrap_or_default()))
        })
        .order_by(StockRow::CATEGORY)
        .order_by(StockRow::PRODUCT)
        .limit(100)
        .get(&db)
        .await?;
    // [/explain:stock.consignments.create.handler]
    Ok(view(
        "stock/consignments/new.html",
        context! { direction, stores, other, rows, q => search, variant => query.variant },
    ))
}

/// A line of a new shipment: `lines[0][variant]`, `lines[0][quantity]`.
#[derive(Deserialize, Debug, Clone)]
pub struct NewLine {
    pub variant: i64,
    pub quantity: Option<i64>,
}

impl Validate for NewLine {
    fn rules(&self, v: &mut Validator) {
        v.field("quantity", &self.quantity).min(0).max(10_000);
    }
}

/// The new-shipment form.
#[derive(Deserialize, Debug)]
pub struct NewShipment {
    /// `send` (ours, to `store`) or `ask` (theirs, to us).
    pub direction: String,
    /// The other store.
    pub store: Option<i64>,
    pub note: Option<String>,
    #[serde(default)]
    pub lines: Vec<NewLine>,
}

impl NewShipment {
    /// `(owner, location)` seen from `store`, the active store.
    pub fn stores(&self, store: i64) -> (i64, i64) {
        let other = self.store.unwrap_or_default();
        if self.direction == "ask" {
            (other, store)
        } else {
            (store, other)
        }
    }

    fn wanted(&self) -> impl Iterator<Item = (usize, &NewLine)> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.quantity.unwrap_or(0) > 0)
    }
}

// [explain:stock.consignments.create.rules]
impl Validate for NewShipment {
    fn rules(&self, v: &mut Validator) {
        v.field("direction", &self.direction)
            .required()
            .one_of(&["send", "ask"]);
        v.field("store", &self.store).required();
        v.field("note", &self.note).max(300);
        v.nested("lines", &self.lines);
    }

    // [/explain:stock.consignments.create.rules]
    /// The other store exists and isn't this one; something is asked for;
    /// no line wants more than the owner has available now.
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let lang = form.state.current_lang();
        let Some(store) = active_store::current() else {
            return Ok(());
        };
        let (owner, location) = self.stores(store);
        if owner == location
            || Store::find(&form.state.db, self.store.unwrap_or_default())
                .await?
                .is_none()
        {
            errors.add("store", lang.t("stock.consignments.errors.store", &[]));
            return Ok(());
        }
        if self.wanted().next().is_none() {
            errors.add("lines", lang.t("stock.consignments.errors.empty", &[]));
            return Ok(());
        }
        let levels: HashMap<i64, i64> = StockRow::query()
            .where_(StockRow::OWNER_STORE_ID.eq(owner))
            .where_(StockRow::LOCATION_STORE_ID.eq(owner))
            .where_(
                StockRow::VARIANT_ID
                    .is_in(self.wanted().map(|(_, l)| l.variant).collect::<Vec<_>>()),
            )
            .get(&form.state.db)
            .await?
            .into_iter()
            .map(|r| (r.variant_id, r.available))
            .collect();
        // [explain:stock.consignments.create.rules]
        for (i, line) in self.wanted() {
            let available = levels.get(&line.variant).copied().unwrap_or(0);
            if line.quantity.unwrap_or(0) > available {
                errors.add(
                    format!("lines.{i}.quantity"),
                    lang.t(
                        "stock.consignments.errors.available",
                        &[("count", &available as &dyn std::fmt::Display)],
                    ),
                );
            }
        }
        Ok(())
    }
    // [/explain:stock.consignments.create.rules]
}

/// `POST /staff/consignments` (`stock.consignments.store`): a request
/// (we ask for another store's goods) waits for the owner store's
/// approval; our own goods start approved.
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<NewShipment>,
) -> Result<(Toast, Redirect)> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    if !access::can_in(&user, catalogue::CONSIGNMENT_MANAGE, store) {
        return Err(Error::Forbidden);
    }
    let (owner, location) = form.stores(store);
    let ours = owner == store;
    let now = renox::db::now();
    let staff = Staff::of_user(&state.db, user.id).await?.map(|s| s.id);
    let mut tx = state.db.begin().await?;
    let shipment = ConsignmentShipment::create(
        &mut tx,
        ConsignmentShipment {
            owner_store_id: owner,
            location_store_id: location,
            status: if ours {
                ShipmentStatus::Approved
            } else {
                ShipmentStatus::Requested
            },
            created_by: staff,
            requested_by: Some(user.id),
            approved_by: ours.then_some(user.id),
            approved_at: ours.then_some(now),
            note: form.note.clone().filter(|n| !n.trim().is_empty()),
            ..Default::default()
        },
    )
    .await?;
    for (_, line) in form.wanted() {
        ConsignmentShipmentLine::create(
            &mut tx,
            ConsignmentShipmentLine {
                shipment_id: shipment.id,
                variant_id: line.variant,
                quantity: line.quantity.unwrap_or(0),
                ..Default::default()
            },
        )
        .await?;
    }
    tx.commit().await?;
    audit::record(
        &state,
        &user,
        if ours {
            "consignment.created"
        } else {
            "consignment.requested"
        },
        store,
        (ConsignmentShipment::TABLE, shipment.id),
        json!({ "owner_store_id": owner, "location_store_id": location }),
    )
    .await?;
    let (other, key) = if ours {
        (location, "stock.mail.consignment.coming")
    } else {
        (owner, "stock.mail.consignment.requested")
    };
    tell(&state, &shipment, other, key, catalogue::CONSIGNMENT_MANAGE).await?;
    Ok((
        Toast::success(state.current_lang().t("stock.consignments.created", &[])),
        Redirect::route("stock.consignments.show", &[&shipment.id])?,
    ))
}

/// A line as the shipment's page shows it.
#[derive(Serialize, Debug, Clone)]
pub struct LineView {
    #[serde(flatten)]
    pub line: ConsignmentShipmentLine,
    pub name: String,
    pub sku: String,
    /// Still to arrive.
    pub open: i64,
}

/// `GET /staff/consignments/{shipment}` (`stock.consignments.show`): the
/// shipment, its lines and its steps, and the buttons the person may press
/// (each checked in the store that matters: approving and shipping in the
/// owner store, receiving in the location store).
// [explain:stock.consignments.show.handler]
pub async fn show(
    State(db): State<Db>,
    user: AuthUser,
    lang: Lang,
    Path(id): Path<i64>,
) -> Result<View> {
    let shipment = access::find::<ConsignmentShipment>(&db, &user, id).await?;
    // [/explain:stock.consignments.show.handler]
    let lines = ConsignmentShipmentLine::query()
        .where_(ConsignmentShipmentLine::SHIPMENT_ID.eq(shipment.id))
        .order_by(ConsignmentShipmentLine::ID)
        .get(&db)
        .await?;
    let names = variant_names(&db, lines.iter().map(|l| l.variant_id).collect()).await?;
    let lines: Vec<LineView> = lines
        .into_iter()
        .map(|line| {
            let name = names.get(&line.variant_id).cloned().unwrap_or_default();
            LineView {
                name: name.label(),
                sku: name.sku,
                open: line.quantity - line.received_quantity,
                line,
            }
        })
        .collect();
    // [explain:stock.consignments.show.handler]
    let stores = store_names(&db).await?;
    let name = |id: i64| stores.get(&id).cloned().unwrap_or_default();
    let owner_may = |p: &str| access::can(&user, p, StoreAttr::Owner, &shipment);
    let location_may = |p: &str| access::can(&user, p, StoreAttr::Location, &shipment);
    let s = shipment.status;
    let actions = json!({
        "approve": s == ShipmentStatus::Requested && owner_may(catalogue::CONSIGNMENT_MANAGE),
        "ship": s == ShipmentStatus::Approved && owner_may(catalogue::CONSIGNMENT_MANAGE),
        "receive": matches!(s, ShipmentStatus::Sent | ShipmentStatus::PartlyReceived) && location_may(catalogue::STOCK_RECEIVE),
        "recall": s == ShipmentStatus::Received && owner_may(catalogue::CONSIGNMENT_MANAGE),
        "send_back": s == ShipmentStatus::RecallRequested && location_may(catalogue::CONSIGNMENT_MANAGE),
        "receive_back": s == ShipmentStatus::RecallSent && owner_may(catalogue::STOCK_RECEIVE),
    });
    // [/explain:stock.consignments.show.handler]
    let reached = rank(s);
    let steps: Vec<_> = STEPS
        .iter()
        .enumerate()
        .map(|(n, step)| json!({ "key": step.as_str(), "done": n <= reached && s != ShipmentStatus::Refused, "current": n == reached }))
        .collect();
    let mut events = vec![json!({
        "time": shipment.created_at,
        "title": lang.t("stock.consignments.status.requested", &[]),
        "kind": "info",
    })];
    for (time, title, kind) in [
        (shipment.approved_at, "approved", "success"),
        (shipment.sent_at, "sent", "info"),
        (shipment.received_at, "received", "success"),
        (shipment.recalled_at, "recalled", "warning"),
    ] {
        if let Some(time) = time {
            let title = lang.t(&format!("stock.consignments.status.{title}"), &[]);
            events.push(json!({ "time": time, "title": title, "kind": kind }));
        }
    }
    Ok(view(
        "stock/consignments/show.html",
        context! {
            owner => name(shipment.owner_store_id),
            location => name(shipment.location_store_id),
            status => s.as_str(),
            in_transit => s.in_transit(),
            actions,
            steps,
            events,
            lines,
            shipment,
        },
    ))
}

// [explain:stock.consignments.show.advance]
/// Moves `shipment` from `from` to `to` (setting `columns` too), only if it
/// is still `from`: `false` when someone was quicker.
async fn advance(
    tx: &mut renox::db::Transaction,
    shipment: &mut ConsignmentShipment,
    from: &[ShipmentStatus],
    to: ShipmentStatus,
) -> Result<bool> {
    let moved = ConsignmentShipment::query()
        .where_(ConsignmentShipment::ID.eq(shipment.id))
        .where_(ConsignmentShipment::STATUS.is_in(from.to_vec()))
        .update(&mut *tx, &[("status", &to)])
        .await?;
    if moved == 0 {
        return Ok(false);
    }
    shipment.status = to;
    Ok(true)
}
// [/explain:stock.consignments.show.advance]

fn conflict(state: &AppState) -> Error {
    abort(
        StatusCode::CONFLICT,
        state
            .current_lang()
            .t("stock.consignments.errors.moved_on", &[]),
    )
}

/// The decision on a request.
#[derive(Deserialize, Validate, Debug)]
pub struct Decision {
    #[validate(required, one_of(&["approve", "refuse"]))]
    pub decision: String,
}

/// `POST /staff/consignments/{shipment}/decide` (`stock.consignments.decide`):
/// the owner store approves or refuses a request for its goods.
pub async fn decide(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<Decision>,
) -> Result<(Toast, Redirect)> {
    let mut shipment = access::find::<ConsignmentShipment>(&state.db, &user, id).await?;
    access::require(
        &user,
        catalogue::CONSIGNMENT_MANAGE,
        StoreAttr::Owner,
        &shipment,
    )?;
    let approve = form.decision == "approve";
    let mut tx = state.db.begin().await?;
    let to = if approve {
        ShipmentStatus::Approved
    } else {
        ShipmentStatus::Refused
    };
    if !advance(&mut tx, &mut shipment, &[ShipmentStatus::Requested], to).await? {
        return Err(conflict(&state));
    }
    shipment.approved_by = Some(user.id);
    shipment.approved_at = Some(renox::db::now());
    shipment
        .save_only(&mut tx, &["approved_by", "approved_at"])
        .await?;
    tx.commit().await?;
    finish(
        &state,
        &user,
        &shipment,
        if approve { "approved" } else { "refused" },
        shipment.owner_store_id,
        shipment.location_store_id,
        catalogue::CONSIGNMENT_MANAGE,
    )
    .await
}

/// `POST /staff/consignments/{shipment}/ship` (`stock.consignments.ship`):
/// the owner store sends the goods. Each line leaves the owner's shelf
/// with a guarded `consign_out` (only what is there); a line whose stock
/// ran out since the request ships what is left (the line says so).
pub async fn ship(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut shipment = access::find::<ConsignmentShipment>(&state.db, &user, id).await?;
    access::require(
        &user,
        catalogue::CONSIGNMENT_MANAGE,
        StoreAttr::Owner,
        &shipment,
    )?;
    let staff = Staff::of_user(&state.db, user.id).await?.map(|s| s.id);
    let owner = shipment.owner_store_id;
    let mut tx = state.db.begin().await?;
    if !advance(
        &mut tx,
        &mut shipment,
        &[ShipmentStatus::Approved],
        ShipmentStatus::Sent,
    )
    .await?
    {
        return Err(conflict(&state));
    }
    let lines = ConsignmentShipmentLine::query()
        .where_(ConsignmentShipmentLine::SHIPMENT_ID.eq(shipment.id))
        .get(&mut tx)
        .await?;
    let mut shipped = 0;
    for mut line in lines {
        let available: i64 = StockRow::query()
            .where_(StockRow::VARIANT_ID.eq(line.variant_id))
            .where_(StockRow::OWNER_STORE_ID.eq(owner))
            .where_(StockRow::LOCATION_STORE_ID.eq(owner))
            .sum(&mut tx, "available")
            .await?;
        let send = line.quantity.min(available).max(0);
        let sent = send > 0
            && ledger::take(
                &mut tx,
                movement(
                    &shipment,
                    line.variant_id,
                    owner,
                    -send,
                    MovementReason::ConsignOut,
                    staff,
                ),
            )
            .await?
            .is_some();
        let quantity = if sent { send } else { 0 };
        if quantity != line.quantity {
            // Shipped short: the line says what really left.
            line.quantity = quantity;
            line.save_only(&mut tx, &["quantity"]).await?;
        }
        shipped += quantity;
    }
    if shipped == 0 {
        return Err(abort(
            StatusCode::CONFLICT,
            state
                .current_lang()
                .t("stock.consignments.errors.nothing", &[]),
        ));
    }
    shipment.sent_at = Some(renox::db::now());
    shipment.save_only(&mut tx, &["sent_at"]).await?;
    tx.commit().await?;
    finish(
        &state,
        &user,
        &shipment,
        "sent",
        owner,
        shipment.location_store_id,
        catalogue::STOCK_RECEIVE,
    )
    .await
}

fn movement(
    shipment: &ConsignmentShipment,
    variant_id: i64,
    owner: i64,
    quantity: i64,
    reason: MovementReason,
    staff: Option<i64>,
) -> StockMovement {
    let location = if matches!(reason, MovementReason::ConsignIn)
        || (reason == MovementReason::Recall && quantity < 0)
    {
        shipment.location_store_id
    } else {
        owner
    };
    StockMovement {
        variant_id,
        owner_store_id: owner,
        location_store_id: location,
        quantity,
        reason,
        reference_type: Some(ConsignmentShipment::TABLE.into()),
        reference_id: Some(shipment.id),
        staff_id: staff,
        ..Default::default()
    }
}

/// One line of a receipt: `lines[0][line]`, `lines[0][received]`.
#[derive(Deserialize, Debug, Clone)]
pub struct ReceiptLine {
    pub line: i64,
    pub received: Option<i64>,
}

impl Validate for ReceiptLine {
    fn rules(&self, v: &mut Validator) {
        v.field("received", &self.received).min(0).max(10_000);
    }
}

/// What arrived.
#[derive(Deserialize, Debug)]
pub struct Receipt {
    #[serde(default)]
    pub lines: Vec<ReceiptLine>,
    /// Nothing more will come: what didn't arrive is missing.
    #[serde(default)]
    pub close: bool,
}

impl Validate for Receipt {
    fn rules(&self, v: &mut Validator) {
        v.nested("lines", &self.lines);
    }
}

/// `POST /staff/consignments/{shipment}/receive` (`stock.consignments.receive`):
/// the location store counts what arrived, line by line; less than sent is
/// a partial receipt (the rest stays in transit), unless "nothing more will
/// come" is ticked: then the shipment is received and the owner is told
/// what went missing on the way (theirs to bear).
pub async fn receive(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<Receipt>,
) -> Result<(Toast, Redirect)> {
    let mut shipment = access::find::<ConsignmentShipment>(&state.db, &user, id).await?;
    access::require(
        &user,
        catalogue::STOCK_RECEIVE,
        StoreAttr::Location,
        &shipment,
    )?;
    let staff = Staff::of_user(&state.db, user.id).await?.map(|s| s.id);
    let counted: HashMap<i64, i64> = form
        .lines
        .iter()
        .map(|l| (l.line, l.received.unwrap_or(0)))
        .collect();
    let mut tx = state.db.begin().await?;
    let lines = ConsignmentShipmentLine::query()
        .where_(ConsignmentShipmentLine::SHIPMENT_ID.eq(shipment.id))
        .get(&mut tx)
        .await?;
    let mut open = 0;
    let mut missing = Vec::new();
    for mut line in lines {
        let left = line.quantity - line.received_quantity;
        let now_in = counted.get(&line.id).copied().unwrap_or(0).clamp(0, left);
        if now_in > 0 {
            StockMovement::record(
                &mut tx,
                movement(
                    &shipment,
                    line.variant_id,
                    shipment.owner_store_id,
                    now_in,
                    MovementReason::ConsignIn,
                    staff,
                ),
            )
            .await?;
            line.received_quantity += now_in;
            line.save_only(&mut tx, &["received_quantity"]).await?;
        }
        let still = line.quantity - line.received_quantity;
        if still > 0 {
            open += still;
            missing.push((line.variant_id, still));
        }
    }
    let done = open == 0 || form.close;
    let to = if done {
        ShipmentStatus::Received
    } else {
        ShipmentStatus::PartlyReceived
    };
    if !advance(
        &mut tx,
        &mut shipment,
        &[ShipmentStatus::Sent, ShipmentStatus::PartlyReceived],
        to,
    )
    .await?
    {
        return Err(conflict(&state));
    }
    if done {
        shipment.received_at = Some(renox::db::now());
        shipment.save_only(&mut tx, &["received_at"]).await?;
    }
    tx.commit().await?;
    if done && !missing.is_empty() {
        let names = variant_names(&state.db, missing.iter().map(|m| m.0).collect()).await?;
        let mut notice = Notice::new(
            "stock-transit-missing",
            "stock.mail.missing.title",
            "stock.mail.missing.body",
        )
        .param("number", shipment.id)
        .tone(Tone::Warning)
        .view("mail/stock/notice")
        .url(crate::app::rentals::link(
            &state,
            "stock.consignments.show",
            Some(shipment.id),
        )?);
        for (variant, units) in &missing {
            let name = names.get(variant).map(|n| n.label()).unwrap_or_default();
            notice = notice.row("stock.fields.missing", format!("{units} × {name}"));
        }
        super::notify::store_staff(
            &state,
            catalogue::CONSIGNMENT_MANAGE,
            shipment.owner_store_id,
            &notice,
        )
        .await?;
    }
    finish(
        &state,
        &user,
        &shipment,
        to.as_str(),
        shipment.location_store_id,
        shipment.owner_store_id,
        catalogue::CONSIGNMENT_MANAGE,
    )
    .await
}

/// `POST /staff/consignments/{shipment}/recall` (`stock.consignments.recall`):
/// the owner store asks for what is left of its goods back.
pub async fn recall(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut shipment = access::find::<ConsignmentShipment>(&state.db, &user, id).await?;
    access::require(
        &user,
        catalogue::CONSIGNMENT_MANAGE,
        StoreAttr::Owner,
        &shipment,
    )?;
    let mut tx = state.db.begin().await?;
    if !advance(
        &mut tx,
        &mut shipment,
        &[ShipmentStatus::Received],
        ShipmentStatus::RecallRequested,
    )
    .await?
    {
        return Err(conflict(&state));
    }
    tx.commit().await?;
    finish(
        &state,
        &user,
        &shipment,
        "recall_requested",
        shipment.owner_store_id,
        shipment.location_store_id,
        catalogue::CONSIGNMENT_MANAGE,
    )
    .await
}

/// `POST /staff/consignments/{shipment}/send-back` (`stock.consignments.send_back`):
/// the location store sends back what is left of the shipment: per line,
/// what arrived and wasn't sent back yet, as far as the shelf still has it
/// (sold units stay sold; the owner was paid for them).
pub async fn send_back(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut shipment = access::find::<ConsignmentShipment>(&state.db, &user, id).await?;
    access::require(
        &user,
        catalogue::CONSIGNMENT_MANAGE,
        StoreAttr::Location,
        &shipment,
    )?;
    let staff = Staff::of_user(&state.db, user.id).await?.map(|s| s.id);
    let (owner, location) = (shipment.owner_store_id, shipment.location_store_id);
    let mut tx = state.db.begin().await?;
    if !advance(
        &mut tx,
        &mut shipment,
        &[ShipmentStatus::RecallRequested],
        ShipmentStatus::RecallSent,
    )
    .await?
    {
        return Err(conflict(&state));
    }
    let lines = ConsignmentShipmentLine::query()
        .where_(ConsignmentShipmentLine::SHIPMENT_ID.eq(shipment.id))
        .get(&mut tx)
        .await?;
    for mut line in lines {
        let available: i64 = StockRow::query()
            .where_(StockRow::VARIANT_ID.eq(line.variant_id))
            .where_(StockRow::OWNER_STORE_ID.eq(owner))
            .where_(StockRow::LOCATION_STORE_ID.eq(location))
            .sum(&mut tx, "available")
            .await?;
        let back = (line.received_quantity - line.returned_quantity).min(available);
        if back > 0
            && ledger::take(
                &mut tx,
                movement(
                    &shipment,
                    line.variant_id,
                    owner,
                    -back,
                    MovementReason::Recall,
                    staff,
                ),
            )
            .await?
            .is_some()
        {
            line.returned_quantity += back;
            line.save_only(&mut tx, &["returned_quantity"]).await?;
        }
    }
    tx.commit().await?;
    finish(
        &state,
        &user,
        &shipment,
        "recall_sent",
        location,
        owner,
        catalogue::STOCK_RECEIVE,
    )
    .await
}

/// `POST /staff/consignments/{shipment}/receive-back` (`stock.consignments.receive_back`):
/// the owner store has its goods back on its own shelf.
pub async fn receive_back(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut shipment = access::find::<ConsignmentShipment>(&state.db, &user, id).await?;
    access::require(&user, catalogue::STOCK_RECEIVE, StoreAttr::Owner, &shipment)?;
    let staff = Staff::of_user(&state.db, user.id).await?.map(|s| s.id);
    let owner = shipment.owner_store_id;
    let mut tx = state.db.begin().await?;
    if !advance(
        &mut tx,
        &mut shipment,
        &[ShipmentStatus::RecallSent],
        ShipmentStatus::Recalled,
    )
    .await?
    {
        return Err(conflict(&state));
    }
    let lines = ConsignmentShipmentLine::query()
        .where_(ConsignmentShipmentLine::SHIPMENT_ID.eq(shipment.id))
        .get(&mut tx)
        .await?;
    for line in lines.iter().filter(|l| l.returned_quantity > 0) {
        StockMovement::record(
            &mut tx,
            movement(
                &shipment,
                line.variant_id,
                owner,
                line.returned_quantity,
                MovementReason::Recall,
                staff,
            ),
        )
        .await?;
    }
    shipment.recalled_at = Some(renox::db::now());
    shipment.save_only(&mut tx, &["recalled_at"]).await?;
    tx.commit().await?;
    finish(
        &state,
        &user,
        &shipment,
        "recalled",
        owner,
        shipment.location_store_id,
        catalogue::CONSIGNMENT_MANAGE,
    )
    .await
}

/// After a step: audited (working in `acting`), the other store told
/// (holders of `permission` in `other`), back to the shipment.
async fn finish(
    state: &AppState,
    user: &User,
    shipment: &ConsignmentShipment,
    step: &str,
    acting: i64,
    other: i64,
    permission: &str,
) -> Result<(Toast, Redirect)> {
    audit::record(
        state,
        user,
        &format!("consignment.{step}"),
        acting,
        (ConsignmentShipment::TABLE, shipment.id),
        json!({ "status": shipment.status.as_str() }),
    )
    .await?;
    tell(
        state,
        shipment,
        other,
        "stock.mail.consignment.moved",
        permission,
    )
    .await?;
    let lang = state.current_lang();
    Ok((
        Toast::success(lang.t(
            "stock.consignments.moved",
            &[(
                "status",
                &lang.t(&format!("stock.consignments.status.{step}"), &[])
                    as &dyn std::fmt::Display,
            )],
        )),
        Redirect::route("stock.consignments.show", &[&shipment.id])?,
    ))
}

/// Tells `store`'s holders of `permission` (in the app) where a shipment stands.
async fn tell(
    state: &AppState,
    shipment: &ConsignmentShipment,
    store: i64,
    body: &'static str,
    permission: &str,
) -> Result {
    let lang = state.current_lang();
    let notice = Notice::new("stock-consignment", "stock.mail.consignment.title", body)
        .param("number", shipment.id)
        .param(
            "status",
            lang.t(
                &format!("stock.consignments.status.{}", shipment.status.as_str()),
                &[],
            ),
        )
        .url(crate::app::rentals::link(
            state,
            "stock.consignments.show",
            Some(shipment.id),
        )?)
        .in_app_only();
    notify::staff(state, permission, &[store], &notice).await
}
