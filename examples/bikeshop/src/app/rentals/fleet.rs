//! The fleet board: the active store's rental bikes at a glance, and one
//! bike's page with its history.
//!
//! "The active store's bikes" means two kinds (#245): bikes **standing**
//! here (whoever owns them) and bikes **owned** here (wherever they stand).
//! The tabs narrow it: mine here, placed here by others, mine elsewhere.

use renox::grid::{Column, Grid, GridRequest};
use renox::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::active_store;
use super::counter::home_store;
use super::model::{BikePlacement, BikeStatus, Rental, RentalBike, RentalRow};
use super::pricing;
use super::reserve::variant_names;
use crate::app::access::{self, StoreAttr, catalogue};
use crate::app::staff::model::Store;
use crate::app::workshop::model::WorkOrder;

/// The board's tabs (`?view=`).
pub const VIEWS: [&str; 4] = ["all", "mine_here", "others_here", "mine_elsewhere"];

/// The status column's options, with their badge tones.
fn status_column(lang: &Lang) -> Column {
    let statuses = [
        ("available", "success"),
        ("reserved", "info"),
        ("rented", "neutral"),
        ("overdue", "danger"),
        ("maintenance", "warning"),
        ("in_transit", "info"),
        ("retired", "neutral"),
    ];
    let options: Vec<(String, String)> = statuses
        .iter()
        .map(|(key, _)| {
            (
                (*key).to_owned(),
                lang.t(&format!("rentals.status.bike.{key}"), &[]),
            )
        })
        .collect();
    Column::select("status", &lang.t("rentals.fields.status", &[]), options)
        .badges(&statuses)
        .mobile()
}

/// The fleet board's grid: columns, filters, badges, cards on phones, and a
/// reload every 30 seconds (`poll`), so a bike coming back at the counter
/// shows up without anyone refreshing.
// [explain:rentals.fleet.grid]
pub fn fleet_grid(lang: &Lang) -> Grid {
    Grid::new("fleet")
        .title(&lang.t("rentals.fleet.title", &[]))
        .column(
            Column::text("frame_number", &lang.t("rentals.fields.frame", &[]))
                .frozen()
                .mobile()
                .searchable()
                .link("/staff/fleet/{id}"),
        )
        .column(Column::custom("model", &lang.t("rentals.fields.bike", &[])).mobile())
        .column(status_column(lang))
        .column(Column::related(
            "owner_store",
            &lang.t("rentals.fields.owner", &[]),
            "stores",
            "owner_store_id",
            "name",
        ))
        // [/explain:rentals.fleet.grid]
        .column(
            Column::related(
                "location_store",
                &lang.t("rentals.fields.location", &[]),
                "stores",
                "location_store_id",
                "name",
            )
            .mobile(),
        )
        .column(Column::number(
            "ridden_hours",
            &lang.t("rentals.fields.ridden", &[]),
        ))
        .column(Column::money(
            "daily_rate",
            &lang.t("rentals.fields.daily_rate", &[]),
        ))
        .column(Column::custom(
            "service",
            &lang.t("rentals.fields.service", &[]),
        ))
        // [explain:rentals.fleet.grid]
        .sort_by("frame_number")
        .per_page(25)
        .row_url("/staff/fleet/{id}")
        .cards_on_mobile()
        .poll(30)
        .empty_state(&lang.t("rentals.fleet.empty", &[]), None)
}
// [/explain:rentals.fleet.grid]

/// `?view=` on the board.
#[derive(Deserialize, Default)]
pub struct FleetQuery {
    #[serde(default)]
    pub view: String,
}

/// `GET /staff/fleet` (`rentals.fleet`): the board.
// [explain:rentals.fleet.handler]
pub async fn index(
    State(state): State<AppState>,
    lang: Lang,
    request: GridRequest,
    Query(query): Query<FleetQuery>,
) -> Result<View> {
    let store = active_store()?;
    // [/explain:rentals.fleet.handler]
    let tab = if VIEWS.contains(&query.view.as_str()) {
        query.view.clone()
    } else {
        "all".to_owned()
    };
    // [explain:rentals.fleet.handler]
    // Always within what the person may see ("mine or at my store",
    // `scopes_with`), then narrowed to the active store and the tab.
    let bikes = access::visible::<RentalBike>(catalogue::FLEET_VIEW);
    let bikes = match tab.as_str() {
        "mine_here" => bikes
            .where_(RentalBike::OWNER_STORE_ID.eq(store))
            .where_(RentalBike::LOCATION_STORE_ID.eq(store)),
        "others_here" => bikes
            .where_(RentalBike::LOCATION_STORE_ID.eq(store))
            .where_(RentalBike::OWNER_STORE_ID.ne(store)),
        "mine_elsewhere" => bikes
            .where_(RentalBike::OWNER_STORE_ID.eq(store))
            .where_(RentalBike::LOCATION_STORE_ID.ne(store)),
        _ => bikes.where_any(|any| {
            any.where_(RentalBike::OWNER_STORE_ID.eq(store))
                .where_(RentalBike::LOCATION_STORE_ID.eq(store))
        }),
    };
    let grid = fleet_grid(&lang);
    let page = grid.page(bikes, &request).await?;
    // [/explain:rentals.fleet.handler]
    let names = variant_names(
        &state.db,
        page.items().iter().map(|b| b.variant_id).collect(),
    )
    .await?;
    let page = page.extend(|bike| {
        let (model, size) = names.get(&bike.variant_id).cloned().unwrap_or_default();
        json!({
            "model": model,
            "size": size,
            "service_due": pricing::service_due(bike),
            "hours_to_service": pricing::SERVICE_EVERY_HOURS - (bike.ridden_hours - bike.serviced_at_hours),
        })
    });
    // The counts on the tabs: three small queries.
    let base = || access::visible::<RentalBike>(catalogue::FLEET_VIEW);
    let counts = json!({
        "mine_here": base().where_(RentalBike::OWNER_STORE_ID.eq(store)).where_(RentalBike::LOCATION_STORE_ID.eq(store)).count(&state.db).await?,
        "others_here": base().where_(RentalBike::LOCATION_STORE_ID.eq(store)).where_(RentalBike::OWNER_STORE_ID.ne(store)).count(&state.db).await?,
        "mine_elsewhere": base().where_(RentalBike::OWNER_STORE_ID.eq(store)).where_(RentalBike::LOCATION_STORE_ID.ne(store)).count(&state.db).await?,
    });
    // The tabs: each view's address and its label, with the count on all but "all".
    let mut tab_items = Vec::new();
    for key in VIEWS {
        let mut label = lang.t(&format!("rentals.fleet.view.{key}"), &[]);
        if key != "all" {
            label = format!("{label} ({})", counts[key]);
        }
        tab_items.push([
            format!("{}?view={key}", state.url("rentals.fleet", &[])?),
            label,
        ]);
    }
    let current_tab = format!("{}?view={tab}", state.url("rentals.fleet", &[])?);
    Ok(view(
        "rentals/fleet.html",
        context! { fleet => page, tab_items, current_tab },
    ))
}

/// What a store may do with one bike, as the bike's page lists it.
#[derive(Serialize, Debug, Clone)]
struct Ability {
    /// The translation key of the action.
    action: &'static str,
    /// The permission it needs…
    permission: &'static str,
    /// …in the store this attribute names.
    attribute: &'static str,
    /// The store's name.
    store: String,
    /// Whether the person looking may do it.
    allowed: bool,
}

/// `GET /staff/fleet/{bike}` (`rentals.fleet.show`): one bike: its owner
/// and location, what each store may do with it (ABAC: renting it out is
/// the location's business, its rates and retiring it the owner's), and
/// its history: rentals, placements, work orders. Visible to staff of the
/// owner **and** of the location store (`access::find`).
// [explain:rentals.fleet.show.handler]
pub async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let bike = access::find::<RentalBike>(db, &user, id).await?;
    // [/explain:rentals.fleet.show.handler]
    let home = home_store(db, &bike).await?;
    let stores: HashMap<i64, Store> =
        Store::find_many(db, [bike.owner_store_id, bike.location_store_id, home])
            .await?
            .into_iter()
            .map(|s| (s.id, s))
            .collect();
    let name = |id: i64| stores.get(&id).map(|s| s.name.clone()).unwrap_or_default();
    let actions: [(&str, &str, StoreAttr); 6] = [
        (
            "rentals.abilities.rent_out",
            catalogue::RENTALS_CHECKOUT,
            StoreAttr::Location,
        ),
        (
            "rentals.abilities.take_back",
            catalogue::RENTALS_RETURN,
            StoreAttr::Location,
        ),
        (
            "rentals.abilities.repair",
            catalogue::WORKORDERS_UPDATE,
            StoreAttr::Location,
        ),
        (
            "rentals.abilities.rates",
            catalogue::PRICES_CHANGE,
            StoreAttr::Owner,
        ),
        (
            "rentals.abilities.place",
            catalogue::FLEET_PLACE,
            StoreAttr::Owner,
        ),
        (
            "rentals.abilities.retire",
            catalogue::FLEET_MANAGE,
            StoreAttr::Owner,
        ),
    ];
    // [explain:rentals.fleet.show.handler]
    let abilities: Vec<Ability> = actions
        .iter()
        .map(|(action, permission, attr)| {
            let store = bike.store_id_of(*attr);
            Ability {
                action,
                permission,
                attribute: if *attr == StoreAttr::Owner {
                    "owner_store"
                } else {
                    "location_store"
                },
                store: name(store),
                allowed: access::can(&user, permission, *attr, &bike),
            }
        })
        .collect();
    // [/explain:rentals.fleet.show.handler]
    let rentals = Rental::query()
        .where_(Rental::RENTAL_BIKE_ID.eq(bike.id))
        .order_by_desc(Rental::STARTS_AT)
        .limit(20)
        .get(db)
        .await?;
    let rentals = RentalRow::load(db, rentals).await?;
    let placements = BikePlacement::query()
        .where_(BikePlacement::RENTAL_BIKE_ID.eq(bike.id))
        .order_by_desc(BikePlacement::REQUESTED_AT)
        .limit(20)
        .get(db)
        .await?;
    let work_orders = WorkOrder::query()
        .where_(WorkOrder::RENTAL_BIKE_ID.eq(bike.id))
        .order_by_desc(WorkOrder::SCHEDULED_FOR)
        .limit(20)
        .get(db)
        .await?;
    let all_stores: HashMap<i64, String> = Store::all_by_name(db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();
    let store_name = |id: i64| all_stores.get(&id).cloned().unwrap_or_default();
    let placements: Vec<renox::serde_json::Value> = placements
        .into_iter()
        .map(|p| json!({ "from": store_name(p.from_store_id), "to": store_name(p.to_store_id), "placement": p }))
        .collect();
    let work_orders: Vec<renox::serde_json::Value> = work_orders
        .into_iter()
        .map(|w| json!({ "store": store_name(w.store_id), "billed": w.billed_store_id.map(store_name), "order": w }))
        .collect();
    let (model, size) = variant_names(db, vec![bike.variant_id])
        .await?
        .remove(&bike.variant_id)
        .unwrap_or_default();
    Ok(view(
        "rentals/fleet_show.html",
        context! {
            model,
            size,
            owner => name(bike.owner_store_id),
            location => name(bike.location_store_id),
            send_back => (bike.location_store_id != home && bike.status != BikeStatus::Rented).then(|| name(home)),
            service_due => pricing::service_due(&bike),
            hours_to_service => pricing::SERVICE_EVERY_HOURS - (bike.ridden_hours - bike.serviced_at_hours),
            abilities,
            rentals,
            placements,
            work_orders,
            bike,
        },
    ))
}

impl RentalBike {
    /// The store of `attr` (owner, or location for the rest).
    pub fn store_id_of(&self, attr: StoreAttr) -> i64 {
        match attr {
            StoreAttr::Owner => self.owner_store_id,
            _ => self.location_store_id,
        }
    }
}
