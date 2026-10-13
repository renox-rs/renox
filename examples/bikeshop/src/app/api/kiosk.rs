//! The kiosk API (`/api/v1/kiosk/…`): a self-service kiosk next to a
//! store's bike racks checks reserved bikes out and takes them back.
//!
//! A kiosk is a row of `kiosks` and a Renox device token (`DeviceToken`,
//! owner key `kiosk:<id>`: no placeholder user), made by a manager on
//! `/staff/api-tokens` ([`super::tokens`]) with some of the abilities
//! `rentals:read`, `rentals:checkout` and `rentals:return`. Every endpoint:
//!
//! 1. `require_device`: no valid `Authorization: Bearer` device token → 401;
//! 2. `require_device_ability(…)`: a token without the endpoint's ability → 403;
//! 3. [`of`]: the device must be a kiosk, not revoked (403 otherwise),
//!    and the kiosk only sees **its own store**: another store's
//!    reservation answers 404, as on the staff side.
//!
//! The rules are the counter's own functions: [`counter::hand_over`]
//! (verified customer, payments recorded) and [`counter::take_back`] (late
//! fee, damage, deposit, the workshop told), with the counter's own forms
//! (`PickupForm`, `ReturnForm`) read by `Valid<T>` from JSON or multipart.

use renox::auth::Device;
use renox::db::{Json as DbJson, Ulid};
use renox::prelude::*;
use renox::serde_json::Value;
use serde::Serialize;

use crate::app::rentals::booking::{free_bikes, to_local};
use crate::app::rentals::counter::{self, PickupForm, ReturnForm};
use crate::app::rentals::model::Rental;
use crate::app::rentals::reserve::variant_names;

/// A store's kiosk.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "kiosks")]
pub struct Kiosk {
    pub id: i64,
    pub store_id: i64,
    pub name: String,
    /// Its device token (`device_tokens`, owner `kiosk:<id>`), `None` once revoked.
    pub token_id: Option<i64>,
    pub abilities: DbJson<Vec<String>>,
    /// The manager who made it.
    pub created_by: Option<i64>,
    pub revoked_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// The owner key of a kiosk's device tokens.
pub fn device_key(kiosk_id: i64) -> String {
    format!("kiosk:{kiosk_id}")
}

/// The kiosk this request's device token belongs to: a 403 for anyone else
/// (another kind of device, a revoked kiosk).
pub async fn of(db: &Db, device: &Device) -> Result<Kiosk> {
    let id: i64 = device
        .id_of("kiosk")
        .and_then(|id| id.parse().ok())
        .ok_or(Error::Forbidden)?;
    let kiosk = Kiosk::query()
        .where_(Kiosk::ID.eq(id))
        .where_(Kiosk::REVOKED_AT.is_null())
        .first(db)
        .await?;
    kiosk.ok_or(Error::Forbidden)
}

/// A reservation of the kiosk's store by its code, or a 404 (another
/// store's, or no such code).
async fn own_rental(db: &Db, kiosk: &Kiosk, code: &str) -> Result<Rental> {
    let code: Ulid = code.parse().map_err(|_| Error::NotFound)?;
    Rental::query()
        .where_(Rental::RESERVATION_CODE.eq(code))
        .where_(Rental::OPERATING_STORE_ID.eq(kiosk.store_id))
        .first(db)
        .await?
        .ok_or(Error::NotFound)
}

/// A bike as the kiosk lists it.
#[derive(Serialize, Debug, Clone)]
pub struct BikeJson {
    pub id: i64,
    pub frame_number: String,
    pub model: String,
    pub size: Option<String>,
    pub hourly_rate: i64,
    pub daily_rate: i64,
}

/// `GET /api/v1/kiosk/bikes` (`api.kiosk.bikes`, `rentals:read`): the
/// bikes free at the kiosk's store for the next hour, cheapest first (the
/// rentals area's own `free_bikes`). Two queries plus the models' names.
pub async fn bikes(State(state): State<AppState>, device: Device) -> Result<Json<Value>> {
    let db = &state.db;
    let kiosk = of(db, &device).await?;
    let now = renox::db::now();
    let free = free_bikes(
        db,
        kiosk.store_id,
        now,
        now + renox::chrono::Duration::hours(1),
        None,
    )
    .await?;
    let names = variant_names(db, free.iter().map(|(b, _)| b.variant_id).collect()).await?;
    let data: Vec<BikeJson> = free
        .into_iter()
        .map(|(bike, _)| {
            let (model, size) = names.get(&bike.variant_id).cloned().unwrap_or_default();
            BikeJson {
                id: bike.id,
                frame_number: bike.frame_number.clone(),
                model,
                size,
                hourly_rate: bike.hourly_rate,
                daily_rate: bike.daily_rate,
            }
        })
        .collect();
    Ok(Json(json!({ "store_id": kiosk.store_id, "data": data })))
}

/// A rental as the API answers it (customers and kiosks).
pub fn rental_json(state: &AppState, rental: &Rental) -> Value {
    let local = |at: DateTime| {
        to_local(&state.config, at)
            .format("%Y-%m-%dT%H:%M")
            .to_string()
    };
    json!({
        "code": rental.reservation_code.to_string(),
        "status": rental.status.as_str(),
        "bike_id": rental.rental_bike_id,
        "store_id": rental.operating_store_id,
        "starts_at": local(rental.starts_at),
        "due_at": local(rental.due_at),
        "price": rental.price,
        "deposit": rental.deposit,
        "deposit_status": rental.deposit_status.as_str(),
        "picked_up_at": rental.picked_up_at,
        "returned_at": rental.returned_at,
        "ridden_minutes": rental.ridden_minutes,
        "late_fee": rental.late_fee,
        "damage_fee": rental.damage_fee,
        "deposit_refunded": rental.deposit_refunded,
    })
}

/// `GET /api/v1/kiosk/rentals/{code}` (`api.kiosk.rental`,
/// `rentals:read`): a reservation of the kiosk's store, by its code.
pub async fn rental(
    State(state): State<AppState>,
    device: Device,
    Path(code): Path<String>,
) -> Result<Json<Value>> {
    let kiosk = of(&state.db, &device).await?;
    let rental = own_rental(&state.db, &kiosk, &code).await?;
    Ok(Json(json!({ "data": rental_json(&state, &rental) })))
}

/// `POST /api/v1/kiosk/rentals/{code}/checkout` (`api.kiosk.checkout`,
/// `rentals:checkout`): the bike leaves. The body is the counter's
/// `PickupForm` (`checklist`, `method`), checked the same way; the
/// reservation must be the kiosk's store's and still reserved (409
/// otherwise); an unverified customer gets the counter's 422.
pub async fn checkout(
    State(state): State<AppState>,
    device: Device,
    Path(code): Path<String>,
    Valid(form): Valid<PickupForm>,
) -> Result<Json<Value>> {
    let db = &state.db;
    let kiosk = of(db, &device).await?;
    let mut rental = own_rental(db, &kiosk, &code).await?;
    if !rental.is_reserved() {
        return Err(abort(
            StatusCode::CONFLICT,
            state.current_lang().t("rentals.desk.not_here", &[]),
        ));
    }
    counter::hand_over(&state, &mut rental, &form, None).await?;
    Ok(Json(json!({ "data": rental_json(&state, &rental) })))
}

/// `POST /api/v1/kiosk/rentals/{code}/return` (`api.kiosk.return`,
/// `rentals:return`): the bike is back at the kiosk's store. The body is
/// the counter's `ReturnForm`, as JSON or, to report damage with photos,
/// as multipart (`damaged`, `damage_note`, `damage_fee`, `photos`); the
/// answer has the minutes ridden, the late fee and how the deposit
/// settled. A bike can come back at any store, so the rental may be
/// another store's, but it must be out (409 otherwise).
pub async fn give_back(
    State(state): State<AppState>,
    device: Device,
    Path(code): Path<String>,
    Valid(form): Valid<ReturnForm>,
) -> Result<Json<Value>> {
    let db = &state.db;
    let kiosk = of(db, &device).await?;
    let code: Ulid = code.parse().map_err(|_| Error::NotFound)?;
    let mut rental = Rental::query()
        .where_(Rental::RESERVATION_CODE.eq(code))
        .first(db)
        .await?
        .filter(|r| r.is_out() || r.operating_store_id == kiosk.store_id)
        .ok_or(Error::NotFound)?;
    if !rental.is_out() {
        return Err(abort(
            StatusCode::CONFLICT,
            state.current_lang().t("rentals.desk.not_out", &[]),
        ));
    }
    let settlement = counter::take_back(&state, &mut rental, &form, kiosk.store_id, None).await?;
    Ok(Json(json!({
        "data": rental_json(&state, &rental),
        "settlement": settlement,
    })))
}
