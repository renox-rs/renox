//! Bikes between sale stock and the rental fleet.
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/stock/fleet` | `stock.fleet` | `fleet.manage` (in the active store) |
//! | `POST /staff/stock/fleet` | `stock.fleet.store` | `fleet.manage` in the bike's **owner** store |
//! | `POST /staff/stock/fleet/{bike}/retire` | `stock.fleet.retire` | `fleet.manage` in the bike's **owner** store |
//!
//! A new bike from the shelf joins the rental fleet: a `to_fleet` movement
//! takes it out of sale stock and a `rental_bikes` row is made, owned by
//! the same store, in one transaction. A rental bike at the end of its
//! rental life goes the other way, to be sold as **used**: its own variant
//! of the same model (`SKU-USED-<bike>`, priced at the bike's book value)
//! gets a `from_fleet` movement, and the bike is retired. Both are the
//! **owner** store's decision (ABAC, #245): a bike placed at another store
//! can't be retired by that store.

use renox::prelude::*;
use serde::Deserialize;

use super::ledger;
use super::model::{MovementReason, StockMovement, StockRow};
use crate::app::access::{self, StoreAttr, active_store, catalogue};
use crate::app::catalog::model::{CategoryKind, ProductVariant};
use crate::app::multistore::audit;
use crate::app::rentals::model::{BikeCondition, BikeStatus, Rental, RentalBike, RentalStatus};
use crate::app::staff::model::Staff;

/// `GET /staff/stock/fleet` (`stock.fleet`): the store's new bikes on the
/// shelf (that could join the fleet), and its rental bikes standing here
/// (that could be retired to sale stock).
pub async fn index(State(db): State<Db>) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let shelf = StockRow::query()
        .where_(StockRow::OWNER_STORE_ID.eq(store))
        .where_(StockRow::LOCATION_STORE_ID.eq(store))
        .where_(StockRow::CATEGORY_KIND.eq(CategoryKind::Bike))
        .where_(StockRow::AVAILABLE.gt(0))
        .where_raw("sku NOT LIKE ?", ["%-USED-%"])
        .order_by(StockRow::PRODUCT)
        .order_by(StockRow::SKU)
        .get(&db)
        .await?;
    let bikes = RentalBike::query()
        .where_(RentalBike::OWNER_STORE_ID.eq(store))
        .where_(RentalBike::LOCATION_STORE_ID.eq(store))
        .where_(RentalBike::STATUS.is_in([BikeStatus::Available, BikeStatus::Maintenance]))
        .order_by(RentalBike::FRAME_NUMBER)
        .get(&db)
        .await?;
    let names = ledger::variant_names(&db, bikes.iter().map(|b| b.variant_id).collect()).await?;
    let bikes: Vec<_> = bikes
        .into_iter()
        .map(|bike| {
            json!({
                "name": names.get(&bike.variant_id).map(|n| n.label()).unwrap_or_default(),
                "bike": bike,
            })
        })
        .collect();
    let options: Vec<(i64, String)> = shelf
        .iter()
        .map(|r| {
            (
                r.id,
                format!(
                    "{} {} · {} ({})",
                    r.product,
                    r.size.clone().unwrap_or_default(),
                    r.sku,
                    r.available
                ),
            )
        })
        .collect();
    Ok(view("stock/fleet.html", context! { shelf, options, bikes }))
}

// [explain:stock.fleet.form]
/// A new bike for the fleet.
#[derive(Deserialize, Validate, Debug)]
pub struct ToFleetForm {
    /// The stock level it comes from (the store's own bikes on its shelf).
    #[validate(required)]
    pub level: Option<i64>,
    #[validate(required, max = 40, unique("rental_bikes", "frame_number"))]
    pub frame_number: String,
    /// Amounts in whole units as typed (`15.00`), stored in the smallest
    /// unit ([`crate::money::from_form`]).
    #[validate(required, min = 0)]
    pub hourly_rate: Option<f64>,
    #[validate(required, min = 0)]
    pub daily_rate: Option<f64>,
    #[validate(required, min = 0)]
    pub deposit: Option<f64>,
}
// [/explain:stock.fleet.form]

/// `POST /staff/stock/fleet` (`stock.fleet.store`).
// [explain:stock.fleet.handler]
pub async fn to_fleet(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<ToFleetForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let level =
        access::find::<super::model::StockLevel>(db, &user, form.level.unwrap_or_default()).await?;
    access::require(&user, catalogue::FLEET_MANAGE, StoreAttr::Owner, &level)?;
    // [/explain:stock.fleet.handler]
    let lang = state.current_lang();
    if level.consigned() {
        // Another store's goods: only their owner may put them in a fleet.
        return Err(Error::Forbidden);
    }
    let variant = ProductVariant::find_or_404(db, level.variant_id).await?;
    let staff = Staff::of_user(db, user.id).await?.map(|s| s.id);
    // [explain:stock.fleet.handler]
    let mut tx = db.begin().await?;
    let taken = ledger::take(
        &mut tx,
        StockMovement {
            variant_id: level.variant_id,
            owner_store_id: level.owner_store_id,
            location_store_id: level.location_store_id,
            quantity: -1,
            reason: MovementReason::ToFleet,
            staff_id: staff,
            note: Some(format!("Frame {}", form.frame_number.trim())),
            ..Default::default()
        },
    )
    .await?;
    // [/explain:stock.fleet.handler]
    let Some(mut movement) = taken else {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("stock.ledger.not_enough", &[]),
        ));
    };
    let bike = RentalBike::create(
        &mut tx,
        RentalBike {
            variant_id: level.variant_id,
            owner_store_id: level.owner_store_id,
            location_store_id: level.location_store_id,
            frame_number: form.frame_number.trim().to_owned(),
            condition: BikeCondition::New,
            status: BikeStatus::Available,
            hourly_rate: crate::money::from_form(form.hourly_rate),
            daily_rate: crate::money::from_form(form.daily_rate),
            deposit: crate::money::from_form(form.deposit),
            asset_value: variant.cost,
            purchased_on: Some(crate::seed::today()),
            ..Default::default()
        },
    )
    .await?;
    // [explain:stock.fleet.handler]
    movement.reference_type = Some(RentalBike::TABLE.into());
    movement.reference_id = Some(bike.id);
    movement
        .save_only(&mut tx, &["reference_type", "reference_id"])
        .await?;
    tx.commit().await?;
    // [/explain:stock.fleet.handler]
    audit::record(
        &state,
        &user,
        "fleet.added_from_stock",
        level.owner_store_id,
        (RentalBike::TABLE, bike.id),
        json!({ "level": level.id, "frame": bike.frame_number }),
    )
    .await?;
    Ok((
        Toast::success(lang.t("stock.fleet.added", &[])),
        Redirect::route("stock.fleet", &[])?,
    ))
}

/// `POST /staff/stock/fleet/{bike}/retire` (`stock.fleet.retire`): the
/// bike leaves the fleet for sale stock, as used. Refused while it is out
/// or booked.
pub async fn retire(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut bike = access::find::<RentalBike>(db, &user, id).await?;
    access::require(&user, catalogue::FLEET_MANAGE, StoreAttr::Owner, &bike)?;
    let lang = state.current_lang();
    let booked = Rental::query()
        .where_(Rental::RENTAL_BIKE_ID.eq(bike.id))
        .where_(Rental::STATUS.is_in([
            RentalStatus::Reserved,
            RentalStatus::Active,
            RentalStatus::Overdue,
        ]))
        .exists(db)
        .await?;
    if booked || !matches!(bike.status, BikeStatus::Available | BikeStatus::Maintenance) {
        return Err(abort(StatusCode::CONFLICT, lang.t("stock.fleet.busy", &[])));
    }
    let model = ProductVariant::find_or_404(db, bike.variant_id).await?;
    let staff = Staff::of_user(db, user.id).await?.map(|s| s.id);
    let mut tx = db.begin().await?;
    let used = ProductVariant::create(
        &mut tx,
        ProductVariant {
            product_id: model.product_id,
            sku: format!("{}-USED-{}", model.sku, bike.id),
            size: model.size.clone(),
            colour: model.colour.clone(),
            price: bike.asset_value.max(1),
            cost: bike.asset_value,
            reorder_level: 0,
            ..Default::default()
        },
    )
    .await?;
    StockMovement::record(
        &mut tx,
        StockMovement {
            variant_id: used.id,
            owner_store_id: bike.owner_store_id,
            location_store_id: bike.location_store_id,
            quantity: 1,
            reason: MovementReason::FromFleet,
            reference_type: Some(RentalBike::TABLE.into()),
            reference_id: Some(bike.id),
            staff_id: staff,
            note: Some(format!("Used, frame {}", bike.frame_number)),
            ..Default::default()
        },
    )
    .await?;
    bike.status = BikeStatus::Retired;
    bike.save_only(&mut tx, &["status"]).await?;
    tx.commit().await?;
    audit::record(
        &state,
        &user,
        "fleet.retired_to_stock",
        bike.owner_store_id,
        (RentalBike::TABLE, bike.id),
        json!({ "variant": used.id }),
    )
    .await?;
    Ok((
        Toast::success(lang.t("stock.fleet.retired", &[])),
        Redirect::route("stock.fleet", &[])?,
    ))
}
