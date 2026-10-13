//! Rental bikes placed at another store (#245).
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/placements` | `multistore.placements` | `fleet.view` (in the active store) |
//! | `GET /staff/placements/new`, `POST /staff/placements` | `multistore.placements.create`, `.store` | `fleet.place` in the active store |
//! | `POST /staff/placements/{placement}/decide` | `multistore.placements.decide` | `fleet.place` in the **owner** store |
//! | `POST /staff/placements/{placement}/move` | `multistore.placements.move` | `fleet.place` in the **owner** store |
//! | `POST /staff/placements/{placement}/recall` | `multistore.placements.recall` | `fleet.place` in the **owner** store |
//! | `POST /staff/placements/send-back/{bike}` | `multistore.placements.send_back` | `fleet.place` in the bike's **location** or owner store |
//!
//! A store with idle bikes places some at a store with customers; the bike
//! stays the owner's (its books, rates, retiring it) while its **location**
//! changes (renting it out is the location's business). The store that
//! wants a bike asks, the owner store approves, then moves it; the owner
//! may call it back. A bike out with a customer can't be called back: the
//! recall locks the bike's row and looks for open rentals in the same
//! transaction the booking uses, so one of them waits for the other and
//! the second is refused (never lost).
//!
//! A bike brought back at a third store stays there (its location
//! changes at the return): its **home** is where its last placement put it,
//! else its owner, and this page suggests sending it back ("send back to …").

use std::collections::HashMap;

use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::audit;
use crate::app::access::{self, StoreAttr, active_store, can_in, catalogue};
use crate::app::rentals::model::{
    BikePlacement, BikeStatus, PlacementStatus, Rental, RentalBike, RentalStatus,
};
use crate::app::rentals::notify::{self, Notice};
use crate::app::staff::model::{Staff, Store};
use crate::app::stock::ledger::variant_names;

/// Bikes that may move: standing, not out or booked.
fn movable(status: BikeStatus) -> bool {
    matches!(status, BikeStatus::Available | BikeStatus::Maintenance)
}

/// Each bike's **home**: where its latest `moved` placement put it, else
/// its owner store. One query for any number of bikes.
pub async fn homes(db: &Db, bikes: &[RentalBike]) -> Result<HashMap<i64, i64>> {
    let mut homes: HashMap<i64, i64> = bikes.iter().map(|b| (b.id, b.owner_store_id)).collect();
    let placements = BikePlacement::query()
        .where_(BikePlacement::RENTAL_BIKE_ID.is_in(bikes.iter().map(|b| b.id).collect::<Vec<_>>()))
        .where_(BikePlacement::STATUS.eq(PlacementStatus::Moved))
        .order_by(BikePlacement::MOVED_AT)
        .order_by(BikePlacement::ID)
        .get(db)
        .await?;
    for p in placements {
        homes.insert(p.rental_bike_id, p.to_store_id);
    }
    Ok(homes)
}

/// A placement as the page lists it.
#[derive(Serialize, Debug, Clone)]
pub struct Row {
    #[serde(flatten)]
    pub placement: BikePlacement,
    pub bike: String,
    pub frame: String,
    pub from: String,
    pub to: String,
    pub can_decide: bool,
    pub can_move: bool,
    pub can_recall: bool,
}

/// A bike away from its home.
#[derive(Serialize, Debug, Clone)]
pub struct SendBack {
    pub bike_id: i64,
    pub bike: String,
    pub frame: String,
    pub at: String,
    pub home: String,
    pub owner: String,
    pub can_send: bool,
}

// [explain:multistore.placements.handler]
/// `GET /staff/placements` (`multistore.placements`): placements to and
/// from the active store, and bikes away from their home with a "send back"
/// task. A fixed number of queries.
pub async fn index(State(db): State<Db>, user: AuthUser) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let placements = access::visible::<BikePlacement>(catalogue::FLEET_VIEW)
        .where_any(|q| {
            q.where_(BikePlacement::FROM_STORE_ID.eq(store))
                .where_(BikePlacement::TO_STORE_ID.eq(store))
        })
        .order_by_desc(BikePlacement::REQUESTED_AT)
        .order_by_desc(BikePlacement::ID)
        .limit(50)
        .get(&db)
        .await?;
    // [/explain:multistore.placements.handler]
    let bikes = RentalBike::find_many(
        &db,
        placements
            .iter()
            .map(|p| p.rental_bike_id)
            .collect::<Vec<_>>(),
    )
    .await?;
    // [explain:multistore.placements.handler]
    // Bikes here or ours, standing somewhere other than their home.
    let around = access::visible::<RentalBike>(catalogue::FLEET_VIEW)
        .where_any(|q| {
            q.where_(RentalBike::OWNER_STORE_ID.eq(store))
                .where_(RentalBike::LOCATION_STORE_ID.eq(store))
        })
        .where_(RentalBike::STATUS.is_in([BikeStatus::Available, BikeStatus::Maintenance]))
        .get(&db)
        .await?;
    let homes = homes(&db, &around).await?;
    // [/explain:multistore.placements.handler]
    let mut ids: Vec<i64> = bikes.iter().map(|b| b.variant_id).collect();
    ids.extend(around.iter().map(|b| b.variant_id));
    let names = variant_names(&db, ids).await?;
    let stores: HashMap<i64, String> = Store::all_by_name(&db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    let name = |id: i64| stores.get(&id).cloned().unwrap_or_default();
    let bikes: HashMap<i64, RentalBike> = bikes.into_iter().map(|b| (b.id, b)).collect();
    let rows: Vec<Row> = placements
        .into_iter()
        .map(|p| {
            let bike = bikes.get(&p.rental_bike_id);
            let owner_may = can_in(&user, catalogue::FLEET_PLACE, p.from_store_id);
            Row {
                bike: bike
                    .and_then(|b| names.get(&b.variant_id))
                    .map(|n| n.label())
                    .unwrap_or_default(),
                frame: bike.map(|b| b.frame_number.clone()).unwrap_or_default(),
                from: name(p.from_store_id),
                to: name(p.to_store_id),
                can_decide: owner_may && p.status == PlacementStatus::Requested,
                can_move: owner_may && p.status == PlacementStatus::Approved,
                can_recall: owner_may && p.status == PlacementStatus::Moved,
                placement: p,
            }
        })
        .collect();
    let send_back: Vec<SendBack> = around
        .iter()
        .filter_map(|b| {
            let home = *homes.get(&b.id)?;
            (home != b.location_store_id).then(|| SendBack {
                bike_id: b.id,
                bike: names
                    .get(&b.variant_id)
                    .map(|n| n.label())
                    .unwrap_or_default(),
                frame: b.frame_number.clone(),
                at: name(b.location_store_id),
                home: name(home),
                owner: name(b.owner_store_id),
                can_send: can_in(&user, catalogue::FLEET_PLACE, b.location_store_id)
                    || can_in(&user, catalogue::FLEET_PLACE, b.owner_store_id),
            })
        })
        .collect();
    Ok(view(
        "multistore/placements/index.html",
        context! { rows, send_back, store },
    ))
}

/// `?direction=place|ask&store=`.
#[derive(Deserialize, Default)]
pub struct NewQuery {
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub store: Option<i64>,
}

// [explain:multistore.placements.create.handler]
/// `GET /staff/placements/new` (`multistore.placements.create`): place one
/// of our bikes at another store (approved at once: it's ours), or ask
/// another store for one of its bikes standing at home.
pub async fn create(State(db): State<Db>, Query(query): Query<NewQuery>) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let direction = if query.direction.as_deref() == Some("ask") {
        "ask"
    } else {
        "place"
    };
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
    let owner = if direction == "place" { store } else { other };
    let bikes = RentalBike::query()
        .where_(RentalBike::OWNER_STORE_ID.eq(owner))
        .where_(RentalBike::LOCATION_STORE_ID.eq(owner))
        .where_(RentalBike::STATUS.eq(BikeStatus::Available))
        .order_by(RentalBike::FRAME_NUMBER)
        .limit(200)
        .get(&db)
        .await?;
    // [/explain:multistore.placements.create.handler]
    let names = variant_names(&db, bikes.iter().map(|b| b.variant_id).collect()).await?;
    let options: Vec<(i64, String)> = bikes
        .iter()
        .map(|b| {
            (
                b.id,
                format!(
                    "{} · {}",
                    names
                        .get(&b.variant_id)
                        .map(|n| n.label())
                        .unwrap_or_default(),
                    b.frame_number
                ),
            )
        })
        .collect();
    let store_options: Vec<(i64, String)> = others.iter().map(|s| (s.id, s.name.clone())).collect();
    Ok(view(
        "multistore/placements/new.html",
        context! { direction, store_options, other, options },
    ))
}

// [explain:multistore.placements.create.form]
/// The placement form.
#[derive(Deserialize, Validate, Debug)]
pub struct PlacementForm {
    #[validate(required)]
    pub bike: Option<i64>,
    /// `place` (our bike, to `store`) or `ask` (theirs, to us).
    #[validate(required, one_of(&["place", "ask"]))]
    pub direction: String,
    #[validate(required)]
    pub store: Option<i64>,
    #[validate(max = 300)]
    pub note: Option<String>,
}
// [/explain:multistore.placements.create.form]

/// `POST /staff/placements` (`multistore.placements.store`).
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<PlacementForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let store = active_store::current().ok_or(Error::Forbidden)?;
    if !can_in(&user, catalogue::FLEET_PLACE, store) {
        return Err(Error::Forbidden);
    }
    let bike = RentalBike::find_or_404(db, form.bike.unwrap_or_default()).await?;
    let other = form.store.unwrap_or_default();
    let (owner, to) = if form.direction == "place" {
        (store, other)
    } else {
        (other, store)
    };
    let lang = state.current_lang();
    if bike.owner_store_id != owner
        || bike.location_store_id != owner
        || owner == to
        || !movable(bike.status)
    {
        return Err(abort(
            StatusCode::UNPROCESSABLE_ENTITY,
            lang.t("multistore.placements.errors.bike", &[]),
        ));
    }
    // [explain:multistore.placements.create.store]
    let ours = owner == store;
    let staff = Staff::of_user(db, user.id).await?.map(|s| s.id);
    let now = renox::db::now();
    let placement = BikePlacement::create(
        db,
        BikePlacement {
            rental_bike_id: bike.id,
            from_store_id: owner,
            to_store_id: to,
            status: if ours {
                PlacementStatus::Approved
            } else {
                PlacementStatus::Requested
            },
            requested_at: now,
            approved_at: ours.then_some(now),
            requested_by: staff,
            approved_by: if ours { staff } else { None },
            note: form.note.clone().filter(|n| !n.trim().is_empty()),
            ..Default::default()
        },
    )
    .await?;
    // [/explain:multistore.placements.create.store]
    audit::record(
        &state,
        &user,
        if ours {
            "placement.created"
        } else {
            "placement.requested"
        },
        store,
        (BikePlacement::TABLE, placement.id),
        json!({ "bike": bike.id, "to_store_id": to }),
    )
    .await?;
    tell(&state, if ours { to } else { owner }, &placement).await?;
    Ok((
        Toast::success(lang.t("multistore.placements.created", &[])),
        Redirect::route("multistore.placements", &[])?,
    ))
}

async fn tell(state: &AppState, store: i64, placement: &BikePlacement) -> Result {
    let lang = state.current_lang();
    let notice = Notice::new(
        "bike-placement",
        "multistore.mail.placement.title",
        "multistore.mail.placement.body",
    )
    .param("number", placement.id)
    .param(
        "status",
        lang.t(
            &format!("multistore.placements.status.{}", placement.status.as_str()),
            &[],
        ),
    )
    .url(crate::app::rentals::link(
        state,
        "multistore.placements",
        None::<i64>,
    )?)
    .in_app_only();
    notify::staff(state, catalogue::FLEET_PLACE, &[store], &notice).await
}

fn conflict(state: &AppState, key: &str) -> Error {
    abort(StatusCode::CONFLICT, state.current_lang().t(key, &[]))
}

/// A decision.
#[derive(Deserialize, Validate, Debug)]
pub struct Decision {
    #[validate(required, one_of(&["approve", "refuse"]))]
    pub decision: String,
}

/// The placement `id`, for an action of the **owner** store.
async fn owned(db: &Db, user: &User, id: i64) -> Result<BikePlacement> {
    let placement = access::find::<BikePlacement>(db, user, id).await?;
    access::require(user, catalogue::FLEET_PLACE, StoreAttr::Owner, &placement)?;
    Ok(placement)
}

/// `POST /staff/placements/{placement}/decide` (`multistore.placements.decide`).
pub async fn decide(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<Decision>,
) -> Result<(Toast, Redirect)> {
    let mut placement = owned(&state.db, &user, id).await?;
    let to = if form.decision == "approve" {
        PlacementStatus::Approved
    } else {
        PlacementStatus::Refused
    };
    let staff = Staff::of_user(&state.db, user.id).await?.map(|s| s.id);
    let now = renox::db::now();
    let moved = BikePlacement::query()
        .where_(BikePlacement::ID.eq(placement.id))
        .where_(BikePlacement::STATUS.eq(PlacementStatus::Requested))
        .update(
            &state.db,
            &[
                ("status", &to as &(dyn renox::db::ToDbValue + Sync)),
                ("approved_at", &now),
                ("approved_by", &staff),
            ],
        )
        .await?;
    if moved == 0 {
        return Err(conflict(&state, "multistore.placements.errors.moved_on"));
    }
    placement.status = to;
    audit::record(
        &state,
        &user,
        &format!("placement.{}", to.as_str()),
        placement.from_store_id,
        (BikePlacement::TABLE, placement.id),
        json!({}),
    )
    .await?;
    tell(&state, placement.to_store_id, &placement).await?;
    Ok((
        Toast::success(state.current_lang().t("multistore.placements.decided", &[])),
        Redirect::route("multistore.placements", &[])?,
    ))
}

// [explain:multistore.placements.move]
/// `POST /staff/placements/{placement}/move` (`multistore.placements.move`):
/// the bike goes; its location is now the other store.
pub async fn move_bike(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut placement = owned(&state.db, &user, id).await?;
    let mut tx = state.db.begin_immediate().await?;
    let bike = RentalBike::query()
        .where_(RentalBike::ID.eq(placement.rental_bike_id))
        .lock_for_update()
        .first(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;
    if placement.status != PlacementStatus::Approved
        || bike.location_store_id != placement.from_store_id
        || !movable(bike.status)
        || booked(&mut tx, bike.id).await?
    {
        return Err(conflict(&state, "multistore.placements.errors.busy"));
    }
    RentalBike::query()
        .where_(RentalBike::ID.eq(bike.id))
        .update(&mut tx, &[("location_store_id", &placement.to_store_id)])
        .await?;
    placement.status = PlacementStatus::Moved;
    placement.moved_at = Some(renox::db::now());
    placement
        .save_only(&mut tx, &["status", "moved_at"])
        .await?;
    tx.commit().await?;
    // [/explain:multistore.placements.move]
    audit::record(
        &state,
        &user,
        "placement.moved",
        placement.from_store_id,
        (BikePlacement::TABLE, placement.id),
        json!({ "bike": bike.id }),
    )
    .await?;
    tell(&state, placement.to_store_id, &placement).await?;
    Ok((
        Toast::success(state.current_lang().t("multistore.placements.moved", &[])),
        Redirect::route("multistore.placements", &[])?,
    ))
}

/// Whether the bike has a rental not finished (booked, out, overdue), read
/// in `tx` after its row was locked.
async fn booked(tx: &mut renox::db::Transaction, bike: i64) -> Result<bool> {
    Rental::query()
        .where_(Rental::RENTAL_BIKE_ID.eq(bike))
        .where_(Rental::STATUS.is_in([
            RentalStatus::Reserved,
            RentalStatus::Active,
            RentalStatus::Overdue,
        ]))
        .exists(&mut *tx)
        .await
}

/// What a recall did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recall {
    /// The bike is back at its owner store.
    Done,
    /// It is out or booked at the other store: refused, nothing changed.
    Busy,
}

/// Calls a moved bike back to its owner store, in one transaction that
/// locks the bike's row first (`begin_immediate` + `lock_for_update`, as
/// a booking does), so a recall and a booking of the same bike run one
/// after the other: the bike is never both rented out there and back here.
pub async fn recall_bike(db: &Db, placement: &mut BikePlacement) -> Result<Recall> {
    let mut tx = db.begin_immediate().await?;
    let bike = RentalBike::query()
        .where_(RentalBike::ID.eq(placement.rental_bike_id))
        .lock_for_update()
        .first(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;
    if placement.status != PlacementStatus::Moved
        || !movable(bike.status)
        || booked(&mut tx, bike.id).await?
    {
        return Ok(Recall::Busy);
    }
    RentalBike::query()
        .where_(RentalBike::ID.eq(bike.id))
        .update(&mut tx, &[("location_store_id", &placement.from_store_id)])
        .await?;
    placement.status = PlacementStatus::Recalled;
    placement.recalled_at = Some(renox::db::now());
    placement
        .save_only(&mut tx, &["status", "recalled_at"])
        .await?;
    tx.commit().await?;
    Ok(Recall::Done)
}

/// `POST /staff/placements/{placement}/recall` (`multistore.placements.recall`).
pub async fn recall(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut placement = owned(&state.db, &user, id).await?;
    if recall_bike(&state.db, &mut placement).await? == Recall::Busy {
        return Err(conflict(&state, "multistore.placements.errors.busy"));
    }
    audit::record(
        &state,
        &user,
        "placement.recalled",
        placement.from_store_id,
        (BikePlacement::TABLE, placement.id),
        json!({}),
    )
    .await?;
    tell(&state, placement.to_store_id, &placement).await?;
    Ok((
        Toast::success(
            state
                .current_lang()
                .t("multistore.placements.recalled", &[]),
        ),
        Redirect::route("multistore.placements", &[])?,
    ))
}

/// `POST /staff/placements/send-back/{bike}` (`multistore.placements.send_back`):
/// a bike brought back at another store goes home (where its last
/// placement put it, else its owner). Recorded as a moved placement.
pub async fn send_back(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let bike = access::find::<RentalBike>(db, &user, id).await?;
    if !access::can(&user, catalogue::FLEET_PLACE, StoreAttr::Location, &bike)
        && !access::can(&user, catalogue::FLEET_PLACE, StoreAttr::Owner, &bike)
    {
        return Err(Error::Forbidden);
    }
    let home = *homes(db, std::slice::from_ref(&bike))
        .await?
        .get(&bike.id)
        .unwrap_or(&bike.owner_store_id);
    let staff = Staff::of_user(db, user.id).await?.map(|s| s.id);
    let mut tx = db.begin_immediate().await?;
    let locked = RentalBike::query()
        .where_(RentalBike::ID.eq(bike.id))
        .lock_for_update()
        .first(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;
    if locked.location_store_id == home
        || !movable(locked.status)
        || booked(&mut tx, bike.id).await?
    {
        return Err(conflict(&state, "multistore.placements.errors.busy"));
    }
    RentalBike::query()
        .where_(RentalBike::ID.eq(bike.id))
        .update(&mut tx, &[("location_store_id", &home)])
        .await?;
    let now = renox::db::now();
    let placement = BikePlacement::create(
        &mut tx,
        BikePlacement {
            rental_bike_id: bike.id,
            from_store_id: locked.location_store_id,
            to_store_id: home,
            status: PlacementStatus::Moved,
            requested_at: now,
            approved_at: Some(now),
            moved_at: Some(now),
            requested_by: staff,
            approved_by: staff,
            note: Some("Sent back home".into()),
            ..Default::default()
        },
    )
    .await?;
    tx.commit().await?;
    audit::record(
        &state,
        &user,
        "placement.sent_back",
        locked.location_store_id,
        (BikePlacement::TABLE, placement.id),
        json!({ "bike": bike.id, "home": home }),
    )
    .await?;
    Ok((
        Toast::success(
            state
                .current_lang()
                .t("multistore.placements.sent_back", &[]),
        ),
        Redirect::route("multistore.placements", &[])?,
    ))
}
