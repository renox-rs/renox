//! The stores: address, phone, opening hours, workshop time and the fee
//! rate for work done for other stores (#239).
//!
//! - `GET /staff/stores` (`staff.stores.index`): the stores the person may
//!   manage (`stores.manage`, which only the owner's role grants by
//!   default), as cards.
//! - `GET /staff/stores/{store}/edit` + `PUT` (`staff.stores.edit`,
//!   `staff.stores.update`): one store's form. Opening hours are the kit's
//!   `repeater` (a row per day: day, opens, closes), stored as JSON in
//!   `stores.opening_hours`.
//! - The fee rate is a separate permission (`settings.fees`): without it
//!   the field isn't shown and a value sent anyway is ignored. A change is
//!   written to the audit log with the old and new rate.
//!
//! Each check is "the permission **in this store**" (`access::can_in`), so
//! a manager given `stores.manage` in North can't open South's form (404).

use renox::db::Json;
use renox::prelude::*;
use serde::Deserialize;

use super::audit;
use super::model::{OpeningHours, Store};
use crate::app::access::catalogue::{SETTINGS_FEES, STORES_MANAGE};
use crate::app::access::{active_store, can_in};
use crate::app::accounts::model::{Address, FullAddress};

/// The days, in order, with their translation keys (`staff.stores.days.mon`).
pub const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// The routes (staff, `stores.manage` in the active store to reach them;
/// each store is checked again in its own scope).
pub fn routes() -> Routes {
    active_store::staff_routes(
        Routes::new()
            .get("/staff/stores", index)
            .name("staff.stores.index")
            .get("/staff/stores/{store}/edit", edit)
            .name("staff.stores.edit")
            .put("/staff/stores/{store}", update)
            .name("staff.stores.update")
            .require_permission(STORES_MANAGE),
    )
}

// [explain:staff.stores.index.handler]
/// The stores the user manages: every store for a global role.
async fn manageable(db: &Db, user: &User) -> Result<Vec<Store>> {
    Ok(Store::all_by_name(db)
        .await?
        .into_iter()
        .filter(|s| can_in(user, STORES_MANAGE, s.id))
        .collect())
}
// [/explain:staff.stores.index.handler]

/// One store the user manages, or a 404.
async fn find(db: &Db, user: &User, id: i64) -> Result<Store> {
    let store = Store::find_or_404(db, id).await?;
    if !can_in(user, STORES_MANAGE, store.id) {
        return Err(Error::NotFound);
    }
    Ok(store)
}

// [explain:staff.stores.index.handler]
/// `GET /staff/stores`: a card per store, with its address and hours.
pub async fn index(State(db): State<Db>, user: AuthUser) -> Result<View> {
    let stores = manageable(&db, &user).await?;
    let addresses = FullAddress::load(&db, stores.iter().map(|s| s.address_id).collect()).await?;
    let cards: Vec<_> = stores
        .iter()
        .map(|store| {
            json!({
                "store": store,
                "address": addresses.get(&store.address_id).map(FullAddress::line),
                "fee_percent": store.fee_rate_percent(),
            })
        })
        .collect();
    Ok(view(
        "staff/stores/index.html",
        context! { cards, can_fees => user.allows(SETTINGS_FEES) },
    ))
}
// [/explain:staff.stores.index.handler]

/// `GET /staff/stores/{store}/edit`.
pub async fn edit(State(db): State<Db>, user: AuthUser, Path(id): Path<i64>) -> Result<View> {
    let store = find(&db, &user, id).await?;
    let address = Address::find_or_404(&db, store.address_id).await?;
    Ok(view(
        "staff/stores/edit.html",
        context! {
            store => &store,
            address,
            hours => &store.opening_hours.0,
            fee_percent => format!("{:.2}", store.fee_rate_percent()),
            can_fees => can_in(&user, SETTINGS_FEES, store.id),
            days => DAYS,
        },
    ))
}

// [explain:staff.stores.edit.form]
/// One row of the opening hours repeater.
#[derive(Deserialize, Debug)]
pub struct HoursRow {
    pub day: String,
    pub opens: String,
    pub closes: String,
}

impl Validate for HoursRow {
    fn rules(&self, v: &mut Validator) {
        v.field("day", &self.day).required().one_of(&DAYS);
        v.field("opens", &self.opens)
            .required()
            .matches(r"^([01][0-9]|2[0-3]):[0-5][0-9]$");
        v.field("closes", &self.closes)
            .required()
            .matches(r"^([01][0-9]|2[0-3]):[0-5][0-9]$");
    }
}
// [/explain:staff.stores.edit.form]

/// The store form.
#[derive(Deserialize, Debug)]
pub struct StoreForm {
    pub name: String,
    pub phone: String,
    pub email: String,
    pub line1: String,
    pub line2: Option<String>,
    pub postal_code: Option<String>,
    pub workshop_minutes_per_day: i64,
    /// A percentage (`20`, `17.5`); only read with `settings.fees`.
    pub fee_percent: Option<f64>,
    #[serde(default)]
    pub hours: Vec<HoursRow>,
}

// [explain:staff.stores.edit.form]
impl Validate for StoreForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required().max(100);
        v.field("phone", &self.phone).required().max(40);
        v.field("email", &self.email).required().email().max(255);
        v.field("line1", &self.line1).required().max(200);
        v.field("workshop_minutes_per_day", &self.workshop_minutes_per_day)
            .required()
            .between(0, 24 * 60);
        v.field("fee_percent", &self.fee_percent).between(0, 100);
        v.nested("hours", &self.hours);
    }
}
// [/explain:staff.stores.edit.form]

/// `PUT /staff/stores/{store}`: saves the store; a fee change is audited.
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<StoreForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut store = find(db, &user, id).await?;
    let mut address = Address::find_or_404(db, store.address_id).await?;
    address.line1 = form.line1;
    address.line2 = form.line2.filter(|l| !l.trim().is_empty());
    address.postal_code = form.postal_code.filter(|p| !p.trim().is_empty());
    address.save(db).await?;

    let old_fee = store.fee_rate_bp;
    store.name = form.name;
    store.phone = form.phone;
    store.email = form.email;
    store.workshop_minutes_per_day = form.workshop_minutes_per_day;
    store.opening_hours = Json(
        form.hours
            .into_iter()
            .map(|h| OpeningHours {
                day: h.day,
                opens: h.opens,
                closes: h.closes,
            })
            .collect(),
    );
    // [explain:staff.stores.edit.fee]
    let fee_changed = match form.fee_percent {
        Some(percent) if can_in(&user, SETTINGS_FEES, store.id) => {
            store.fee_rate_bp = (percent * 100.0).round() as i64;
            store.fee_rate_bp != old_fee
        }
        _ => false,
    };
    store.save(db).await?;

    audit::record(db, &user, STORES_MANAGE, "store.updated")
        .subject("stores", store.id)
        .save()
        .await?;
    if fee_changed {
        audit::record(db, &user, SETTINGS_FEES, "store.fee_rate_changed")
            .subject("stores", store.id)
            .data(json!({ "from_bp": old_fee, "to_bp": store.fee_rate_bp }))
            .save()
            .await?;
    }
    // [/explain:staff.stores.edit.fee]
    Ok((
        Toast::success(
            state
                .current_lang()
                .t("staff.stores.saved", &[("store", &store.name)]),
        ),
        Redirect::to(&state.url("staff.stores.index", &[])?),
    ))
}
