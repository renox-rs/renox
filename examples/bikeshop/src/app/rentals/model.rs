//! The rental fleet and rentals (Pagila's `inventory` and `rental`).
//!
//! Three store attributes meet here (#245):
//!
//! - a [`RentalBike`] has an **owner store** (whose books and asset it is:
//!   its rates, retiring or selling it are checked there) and a **location
//!   store** (where it stands now: renting it out is checked there);
//! - a [`Rental`] keeps the bike's owner store (for the books) and has an
//!   **operating store**, the one that served the customer (it earns a fee when the bike is another store's), and a
//!   return store when the bike came back elsewhere;
//! - a [`BikePlacement`] moves a bike's location to another store, on the
//!   owner store's approval, until it is called back.
//!
//! A rental's reservation code is a [`Ulid`]: short enough to read out at
//! the counter, sortable by time, and it doesn't reveal how many rentals
//! there are.
//!
//! Migration: `migrations/20260101000600_create_fleet_and_rentals_tables.*`.

use renox::chrono::NaiveDate;
use renox::db::relations::belongs_to;
use renox::db::{Json, Ulid};
use renox::prelude::*;
use serde::Serialize;

use crate::app::access::{StoreAttr, StoreRecord, catalogue};
use crate::app::accounts::model::Customer;
use crate::app::catalog::model::{Product, ProductVariant};
use crate::app::staff::model::Store;

/// The state a rental bike is in.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BikeCondition {
    /// Bought this season.
    New,
    #[default]
    Good,
    /// Rideable, with wear.
    Fair,
    /// Waiting for the workshop.
    NeedsRepair,
}

/// Whether a rental bike can be rented now.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BikeStatus {
    #[default]
    Available,
    /// Reserved for a rental that starts soon.
    Reserved,
    /// Out with a customer.
    Rented,
    /// Out with a customer and past its due time (set by the overdue job).
    Overdue,
    /// In the workshop.
    Maintenance,
    /// On its way to another store (a placement or a recall).
    InTransit,
    /// No longer rented out (decided by the owner store).
    Retired,
}

/// A bike of the rental fleet.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "rental_bikes")]
pub struct RentalBike {
    pub id: i64,
    /// The bike model and size (a catalogue variant).
    pub variant_id: i64,
    /// Whose books and asset it is.
    pub owner_store_id: i64,
    /// Where it is now.
    pub location_store_id: i64,
    pub frame_number: String,
    pub condition: BikeCondition,
    pub status: BikeStatus,
    /// Prices in the smallest unit of `APP_CURRENCY`.
    pub hourly_rate: i64,
    pub daily_rate: i64,
    pub deposit: i64,
    /// What it is worth in the owner's books.
    pub asset_value: i64,
    pub ridden_hours: i64,
    /// `ridden_hours` when the workshop last serviced it: the next service
    /// is due [`crate::app::rentals::pricing::SERVICE_EVERY_HOURS`] later.
    pub serviced_at_hours: i64,
    pub purchased_on: Option<NaiveDate>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl RentalBike {
    /// Whether it stands at another store than its owner's (placed there).
    pub fn placed_elsewhere(&self) -> bool {
        self.owner_store_id != self.location_store_id
    }
}

impl StoreRecord for RentalBike {
    const VIEW: &'static str = catalogue::FLEET_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["owner_store_id", "location_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.owner_store_id),
            StoreAttr::Location | StoreAttr::Operating => Some(self.location_store_id),
        }
    }
}

/// Where a placement stands.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlacementStatus {
    /// Asked by the store that wants the bike.
    #[default]
    Requested,
    /// Approved by the owner store, not moved yet.
    Approved,
    /// The bike is at the other store.
    Moved,
    /// Called back by the owner store.
    Recalled,
    /// The owner store said no.
    Refused,
}

/// A rental bike placed at another store (#245): requested, approved by
/// the owner store, moved, and maybe called back.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "bike_placements")]
pub struct BikePlacement {
    pub id: i64,
    pub rental_bike_id: i64,
    /// Where it comes from (usually the owner store).
    pub from_store_id: i64,
    /// Where it goes.
    pub to_store_id: i64,
    pub status: PlacementStatus,
    pub requested_at: DateTime,
    pub approved_at: Option<DateTime>,
    pub moved_at: Option<DateTime>,
    pub recalled_at: Option<DateTime>,
    pub requested_by: Option<i64>,
    pub approved_by: Option<i64>,
    pub note: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl StoreRecord for BikePlacement {
    const VIEW: &'static str = catalogue::FLEET_VIEW;
    const STORE_COLUMNS: &'static [&'static str] = &["from_store_id", "to_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Owner => Some(self.from_store_id),
            StoreAttr::Location | StoreAttr::Operating => Some(self.to_store_id),
        }
    }
}

/// How a rental is priced.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RentalRate {
    /// By the hour (`hourly_rate`).
    Hourly,
    /// By the day (`daily_rate`).
    #[default]
    Daily,
}

/// Where a rental stands.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RentalStatus {
    /// Booked, not picked up yet.
    #[default]
    Reserved,
    /// Out with the customer.
    Active,
    /// Out past its due time (set by the overdue job, #235).
    Overdue,
    /// Back at a store.
    Returned,
    /// Called off before pick-up.
    Cancelled,
    /// Not picked up within half an hour of its start (the no-show task).
    NoShow,
}

/// What happened to a rental's deposit.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DepositStatus {
    /// Not paid yet (a walk-in pays it at the counter).
    #[default]
    Unpaid,
    /// Paid online or at the counter, held by the operating store.
    Held,
    /// Settled at the return: fees taken from it, the rest given back.
    Settled,
    /// Given back whole (a cancellation in time).
    Refunded,
    /// Partly kept for a no-show.
    Forfeited,
}

/// A bike rented by a customer for some hours or days.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "rentals")]
pub struct Rental {
    pub id: i64,
    /// What the customer shows at the counter.
    pub reservation_code: Ulid,
    pub customer_id: i64,
    pub rental_bike_id: i64,
    /// The bike's owner store when it was rented: the rental's revenue is
    /// booked to it (copied from the bike, so the books never change if
    /// the bike does).
    pub owner_store_id: i64,
    /// The store that served the customer.
    pub operating_store_id: i64,
    /// Where the bike came back, when not where it left.
    pub return_store_id: Option<i64>,
    pub rate: RentalRate,
    pub starts_at: DateTime,
    pub due_at: DateTime,
    pub picked_up_at: Option<DateTime>,
    pub returned_at: Option<DateTime>,
    pub status: RentalStatus,
    /// Prices in the smallest unit of `APP_CURRENCY`.
    pub price: i64,
    /// Held by the operating store until the bike is back.
    pub deposit: i64,
    pub late_fee: i64,
    pub damage_fee: i64,
    /// Who handed it over (a `staff` row).
    pub served_by: Option<i64>,
    pub deposit_status: DepositStatus,
    /// What was given back of the deposit (at the return, a cancellation or
    /// a no-show).
    pub deposit_refunded: i64,
    /// The condition checklist at pick-up: the items found in order.
    pub pickup_checklist: Option<Json<Vec<String>>>,
    /// The same checklist at the return.
    pub return_checklist: Option<Json<Vec<String>>>,
    /// What was damaged, noted at the return.
    pub damage_note: Option<String>,
    /// Who took it back (a `staff` row).
    pub returned_by: Option<i64>,
    /// When the "an hour left" reminder went out.
    pub reminded_at: Option<DateTime>,
    pub cancelled_at: Option<DateTime>,
    /// From pick-up to return, added to the bike's `ridden_hours`.
    pub ridden_minutes: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl Rental {
    /// Out and past its due time.
    pub fn is_overdue(&self) -> bool {
        matches!(self.status, RentalStatus::Active | RentalStatus::Overdue)
            && self.due_at < renox::db::now()
    }

    /// Price plus fees.
    pub fn total(&self) -> i64 {
        self.price + self.late_fee + self.damage_fee
    }

    /// Late and damage fees together (what the deposit settles against).
    pub fn fees(&self) -> i64 {
        self.late_fee + self.damage_fee
    }

    /// Booked, not picked up yet.
    pub fn is_reserved(&self) -> bool {
        self.status == RentalStatus::Reserved
    }

    /// With the customer now (on time or not).
    pub fn is_out(&self) -> bool {
        matches!(self.status, RentalStatus::Active | RentalStatus::Overdue)
    }

    /// The customer may still cancel it: reserved, and more than
    /// [`crate::app::rentals::pricing::CANCEL_UNTIL_MINUTES`] before its start.
    pub fn cancellable(&self) -> bool {
        self.is_reserved()
            && renox::db::now()
                < self.starts_at
                    - renox::chrono::Duration::minutes(
                        crate::app::rentals::pricing::CANCEL_UNTIL_MINUTES,
                    )
    }
}

/// Where a customer's ID document stands.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdentityStatus {
    /// Sent, waiting for staff.
    #[default]
    Pending,
    /// Checked by staff: the customer's `id_verified_at` is set.
    Approved,
    /// Refused (a blurred photo, a number that doesn't match), with a note.
    Refused,
}

/// A customer's ID document, sent once before their first rental (#235).
///
/// The photo is a **private** upload (`Upload::store`, never under
/// `public/`): staff open it through a signed link that works for a few
/// minutes, and only staff who may check IDs (`rentals.verify_id`) in the
/// store the customer chose (`store_id`) or in a store serving one of their
/// rentals. The ID number itself is the customer's `id_number`, an
/// `Encrypted<String>`. Approving sets `customers.id_verified_at`, valid in
/// every store.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "identity_documents")]
pub struct IdentityDocument {
    pub id: i64,
    pub customer_id: i64,
    /// The private storage key of the photo (never shown: staff get a
    /// temporary signed link).
    #[serde(skip_serializing)]
    pub photo_path: String,
    /// The store that checks it (where the customer will pick up first).
    pub store_id: i64,
    pub status: IdentityStatus,
    pub submitted_at: DateTime,
    /// The user who approved or refused it.
    pub reviewed_by: Option<i64>,
    pub reviewed_at: Option<DateTime>,
    /// Why it was refused.
    pub note: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl StoreRecord for IdentityDocument {
    const VIEW: &'static str = catalogue::RENTALS_VERIFY_ID;
    const STORE_COLUMNS: &'static [&'static str] = &["store_id"];

    fn store_id(&self, _attr: StoreAttr) -> Option<i64> {
        Some(self.store_id)
    }
}

/// Why a rental photo was taken.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhotoKind {
    /// The bike at pick-up.
    #[default]
    Pickup,
    /// The bike at the return.
    Return,
    /// Damage found at the return.
    Damage,
}

/// A photo of a rental bike at the counter (a private upload).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "rental_photos")]
pub struct RentalPhoto {
    pub id: i64,
    pub rental_id: i64,
    pub kind: PhotoKind,
    /// The private storage key.
    #[serde(skip_serializing)]
    pub path: String,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl StoreRecord for Rental {
    const VIEW: &'static str = catalogue::RENTALS_VIEW;
    const STORE_COLUMNS: &'static [&'static str] =
        &["owner_store_id", "operating_store_id", "return_store_id"];

    fn store_id(&self, attr: StoreAttr) -> Option<i64> {
        match attr {
            StoreAttr::Operating => Some(self.operating_store_id),
            StoreAttr::Location => Some(self.return_store_id.unwrap_or(self.operating_store_id)),
            StoreAttr::Owner => Some(self.owner_store_id),
        }
    }
}

/// A rental as a list shows it: with its bike (and the bike's model), the
/// customer and the stores.
#[derive(Serialize, Debug, Clone)]
pub struct RentalRow {
    #[serde(flatten)]
    pub rental: Rental,
    pub bike: Option<RentalBike>,
    pub model: Option<String>,
    pub customer: Option<Customer>,
    pub operating_store: Option<Store>,
    pub owner_store: Option<Store>,
    pub overdue: bool,
}

impl RentalRow {
    // [explain:rentals.rows]
    /// Rows for a page of rentals in five queries, however many rentals:
    /// bikes, their variants, the variants' products, customers, and the
    /// stores (operating and owner together).
    pub async fn load(db: &Db, rentals: Vec<Rental>) -> Result<Vec<RentalRow>> {
        let bikes = belongs_to::<RentalBike, _, _>(db, &rentals, |r| r.rental_bike_id).await?;
        let bike_list: Vec<RentalBike> = bikes.values().cloned().collect();
        let variants = belongs_to::<ProductVariant, _, _>(db, &bike_list, |b| b.variant_id).await?;
        let variant_list: Vec<ProductVariant> = variants.values().cloned().collect();
        let products = belongs_to::<Product, _, _>(db, &variant_list, |v| v.product_id).await?;
        let customers = belongs_to::<Customer, _, _>(db, &rentals, |r| r.customer_id).await?;
        let mut store_ids: Vec<i64> = rentals.iter().map(|r| r.operating_store_id).collect();
        store_ids.extend(rentals.iter().map(|r| r.owner_store_id));
        store_ids.sort_unstable();
        store_ids.dedup();
        let stores: std::collections::HashMap<i64, Store> = Store::find_many(db, store_ids)
            .await?
            .into_iter()
            .map(|s| (s.id, s))
            .collect();
        // [/explain:rentals.rows]
        Ok(rentals
            .into_iter()
            .map(|rental| {
                let bike = bikes.get(&rental.rental_bike_id).cloned();
                let variant = bike.as_ref().and_then(|b| variants.get(&b.variant_id));
                let model = variant.and_then(|v| {
                    products.get(&v.product_id).map(|p| match &v.size {
                        Some(size) => format!("{} ({size})", p.name),
                        None => p.name.clone(),
                    })
                });
                RentalRow {
                    overdue: rental.is_overdue(),
                    owner_store: stores.get(&rental.owner_store_id).cloned(),
                    operating_store: stores.get(&rental.operating_store_id).cloned(),
                    customer: customers.get(&rental.customer_id).cloned(),
                    model,
                    bike,
                    rental,
                }
            })
            .collect())
    }
}
