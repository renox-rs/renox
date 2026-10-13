//! Factories for the fleet and rentals:
//! `rental_bikes().owned_by(north).placed_at(south)`,
//! `rentals().of_bike(&bike).overdue()`…

use renox::chrono::Duration;
use renox::db::{FactoryBuilder, Ulid};
use renox::fake::Fake;
use renox::prelude::*;

use super::model::{
    BikeCondition, BikePlacement, BikeStatus, PlacementStatus, Rental, RentalBike, RentalRate,
    RentalStatus,
};
use crate::seed::unique;

impl Factory for RentalBike {
    fn definition() -> Self {
        let daily = (40..90).fake::<i64>() * 100;
        RentalBike {
            frame_number: format!("WBK{:08}", unique()),
            condition: BikeCondition::Good,
            status: BikeStatus::Available,
            hourly_rate: daily / 5,
            daily_rate: daily,
            deposit: daily * 5,
            asset_value: daily * 60,
            ridden_hours: (0..800).fake(),
            purchased_on: Some(crate::seed::today() - Duration::days((30..700).fake::<i64>())),
            ..Default::default()
        }
    }
}

/// `RentalBike::factory()`.
pub fn rental_bikes() -> FactoryBuilder<RentalBike> {
    RentalBike::factory()
}

/// States of a rental bike.
pub trait BikeStates {
    /// A bike of `variant_id`.
    fn model(self, variant_id: i64) -> Self;
    /// Owned by `store_id` and standing there.
    fn owned_by(self, store_id: i64) -> Self;
    /// Standing at `store_id` (placed there, whoever owns it).
    fn placed_at(self, store_id: i64) -> Self;
    /// Out with a customer.
    fn rented(self) -> Self;
    /// In the workshop.
    fn in_maintenance(self) -> Self;
    /// No longer rented out.
    fn retired(self) -> Self;
}

impl BikeStates for FactoryBuilder<RentalBike> {
    fn model(self, variant_id: i64) -> Self {
        self.state(move |b| b.variant_id = variant_id)
    }

    fn owned_by(self, store_id: i64) -> Self {
        self.state(move |b| {
            b.owner_store_id = store_id;
            b.location_store_id = store_id;
        })
    }

    fn placed_at(self, store_id: i64) -> Self {
        self.state(move |b| b.location_store_id = store_id)
    }

    fn rented(self) -> Self {
        self.state(|b| b.status = BikeStatus::Rented)
    }

    fn in_maintenance(self) -> Self {
        self.state(|b| {
            b.status = BikeStatus::Maintenance;
            b.condition = BikeCondition::NeedsRepair;
        })
    }

    fn retired(self) -> Self {
        self.state(|b| b.status = BikeStatus::Retired)
    }
}

impl Factory for BikePlacement {
    fn definition() -> Self {
        BikePlacement {
            status: PlacementStatus::Requested,
            requested_at: renox::db::now(),
            ..Default::default()
        }
    }
}

/// States of a placement.
pub trait PlacementStates {
    /// `bike` moving from its owner store to `to_store`.
    fn of(self, bike: &RentalBike, to_store: i64) -> Self;
    /// Approved and moved, `days` ago.
    fn moved(self, days: i64) -> Self;
}

impl PlacementStates for FactoryBuilder<BikePlacement> {
    fn of(self, bike: &RentalBike, to_store: i64) -> Self {
        let (bike_id, from) = (bike.id, bike.owner_store_id);
        self.state(move |p| {
            p.rental_bike_id = bike_id;
            p.from_store_id = from;
            p.to_store_id = to_store;
        })
    }

    fn moved(self, days: i64) -> Self {
        self.state(move |p| {
            let at = renox::db::now() - Duration::days(days);
            p.status = PlacementStatus::Moved;
            p.requested_at = at - Duration::days(2);
            p.approved_at = Some(at - Duration::days(1));
            p.moved_at = Some(at);
        })
    }
}

impl Factory for Rental {
    fn definition() -> Self {
        let starts = renox::db::now() + Duration::days(2);
        Rental {
            reservation_code: Ulid::new(),
            rate: RentalRate::Daily,
            starts_at: starts,
            due_at: starts + Duration::days(1),
            status: RentalStatus::Reserved,
            price: 6_000,
            deposit: 30_000,
            ..Default::default()
        }
    }
}

/// `Rental::factory()`.
pub fn rentals() -> FactoryBuilder<Rental> {
    Rental::factory()
}

/// States of a rental.
pub trait RentalStates {
    /// Of `bike`, served where it stands, at its daily rate.
    fn of_bike(self, bike: &RentalBike) -> Self;
    /// For `customer_id`.
    fn for_customer(self, customer_id: i64) -> Self;
    /// Picked up two hours ago, due tomorrow.
    fn active(self) -> Self;
    /// Picked up yesterday, due back later today.
    fn due_today(self) -> Self;
    /// Out and a day past its due time.
    fn overdue(self) -> Self;
    /// Back on time, three days ago.
    fn returned(self) -> Self;
    /// Back four hours late, with a late fee.
    fn returned_late(self) -> Self;
}

impl RentalStates for FactoryBuilder<Rental> {
    fn of_bike(self, bike: &RentalBike) -> Self {
        let bike = bike.clone();
        self.state(move |r| {
            r.rental_bike_id = bike.id;
            r.owner_store_id = bike.owner_store_id;
            r.operating_store_id = bike.location_store_id;
            r.price = bike.daily_rate;
            r.deposit = bike.deposit;
        })
    }

    fn for_customer(self, customer_id: i64) -> Self {
        self.state(move |r| r.customer_id = customer_id)
    }

    fn active(self) -> Self {
        self.state(|r| {
            let now = renox::db::now();
            r.status = RentalStatus::Active;
            r.starts_at = now - Duration::hours(2);
            r.picked_up_at = Some(r.starts_at);
            r.due_at = now + Duration::days(1);
        })
    }

    fn due_today(self) -> Self {
        self.state(|r| {
            let now = renox::db::now();
            r.status = RentalStatus::Active;
            let end_of_today = (now.date_naive() + Duration::days(1))
                .and_hms_opt(0, 0, 0)
                .expect("midnight")
                .and_utc();
            r.starts_at = now - Duration::hours(20);
            r.picked_up_at = Some(r.starts_at);
            // Halfway between now and midnight: later today, whatever the time.
            r.due_at = now + (end_of_today - now) / 2;
        })
    }

    fn overdue(self) -> Self {
        self.state(|r| {
            let now = renox::db::now();
            r.status = RentalStatus::Overdue;
            r.starts_at = now - Duration::days(3);
            r.picked_up_at = Some(r.starts_at);
            r.due_at = now - Duration::days(1);
        })
    }

    fn returned(self) -> Self {
        self.state(|r| {
            let now = renox::db::now();
            r.status = RentalStatus::Returned;
            r.starts_at = now - Duration::days(4);
            r.picked_up_at = Some(r.starts_at);
            r.due_at = now - Duration::days(3);
            r.returned_at = Some(r.due_at - Duration::hours(1));
        })
    }

    fn returned_late(self) -> Self {
        self.state(|r| {
            let now = renox::db::now();
            r.status = RentalStatus::Returned;
            r.starts_at = now - Duration::days(4);
            r.picked_up_at = Some(r.starts_at);
            r.due_at = now - Duration::days(3);
            r.returned_at = Some(r.due_at + Duration::hours(4));
            r.late_fee = r.price / 2;
        })
    }
}
