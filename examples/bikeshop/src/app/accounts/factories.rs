//! Factories for places and customers (`Factory` + states), used by the
//! seeders and the tests: `customers().verified().count(3).create(&db)`.

use renox::db::FactoryBuilder;
use renox::fake::Fake;
use renox::fake::faker::address::en::{CityName, CountryName, PostCode, StreetName};
use renox::fake::faker::name::en::Name;
use renox::fake::faker::phone_number::en::PhoneNumber;
use renox::prelude::*;

use super::model::{Address, City, Country, Customer};
use crate::seed::unique;

impl Factory for Country {
    fn definition() -> Self {
        let n = unique();
        Country {
            name: format!("{} {n}", CountryName().fake::<String>()),
            code: format!("C{n}"),
            ..Default::default()
        }
    }
}

impl Factory for City {
    fn definition() -> Self {
        City {
            name: CityName().fake(),
            ..Default::default()
        }
    }
}

impl Factory for Address {
    fn definition() -> Self {
        Address {
            line1: format!(
                "{} {}",
                StreetName().fake::<String>(),
                (1..200).fake::<i64>()
            ),
            postal_code: Some(PostCode().fake()),
            ..Default::default()
        }
    }
}

impl Factory for Customer {
    fn definition() -> Self {
        let name: String = Name().fake();
        let n = unique();
        Customer {
            email: Some(format!("customer{n}@example.com")),
            phone: Some(PhoneNumber().fake()),
            name,
            active: true,
            ..Default::default()
        }
    }
}

/// `Customer::factory()`, named like the table.
pub fn customers() -> FactoryBuilder<Customer> {
    Customer::factory()
}

/// `Address::factory()` in a city.
pub fn addresses_in(city_id: i64) -> FactoryBuilder<Address> {
    Address::factory().state(move |a| a.city_id = city_id)
}

/// States of a customer.
pub trait CustomerStates {
    /// With an ID document number on file, checked by staff (may rent).
    fn verified(self) -> Self;
    /// With an ID number on file, not checked yet.
    fn unverified_id(self) -> Self;
    /// A walk-in: no email, no account.
    fn walk_in(self) -> Self;
    /// Living at `address_id`.
    fn living_at(self, address_id: i64) -> Self;
    /// Left the shop (soft deleted).
    fn left(self) -> Self;
}

impl CustomerStates for FactoryBuilder<Customer> {
    fn verified(self) -> Self {
        self.state(|c| {
            c.id_number = Some(id_number().into());
            c.id_verified_at = Some(renox::db::now() - renox::chrono::Duration::days(30));
        })
    }

    fn unverified_id(self) -> Self {
        self.state(|c| {
            c.id_number = Some(id_number().into());
            c.id_verified_at = None;
        })
    }

    fn walk_in(self) -> Self {
        self.state(|c| {
            c.email = None;
            c.user_id = None;
        })
    }

    fn living_at(self, address_id: i64) -> Self {
        self.state(move |c| c.address_id = Some(address_id))
    }

    fn left(self) -> Self {
        self.state(|c| {
            c.active = false;
            c.deleted_at = Some(renox::db::now());
        })
    }
}

/// A passport-like number: two letters and seven digits.
pub fn id_number() -> String {
    let letters = ["A", "B", "C", "E", "K", "P", "X"];
    format!(
        "{}{}{:07}",
        letters[(0..letters.len()).fake::<usize>()],
        letters[(0..letters.len()).fake::<usize>()],
        (0..10_000_000).fake::<u32>()
    )
}
