//! Places and customers (Pagila's `country`, `city`, `address`, `customer`).
//!
//! Customers belong to the company, not to a store: anyone can buy in one
//! store, rent in another and have their bike serviced in the third. A
//! walk-in customer has no user account (`user_id` is `None`); one who
//! registers online has one.
//!
//! Migrations: `migrations/20260101000100_create_places_tables.*` and
//! `migrations/20260101000300_create_customers_table.*`.

use renox::db::Encrypted;
use renox::db::relations::belongs_to;
use renox::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

/// A country (Pagila's `country`).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "countries")]
pub struct Country {
    pub id: i64,
    pub name: String,
    /// ISO 3166-1 alpha-2, e.g. `ES`.
    pub code: String,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// A city in a country (Pagila's `city`).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "cities")]
pub struct City {
    pub id: i64,
    pub country_id: i64,
    pub name: String,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// A street address in a city (Pagila's `address`): of a store, a
/// customer, a supplier, or where an order is delivered.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "addresses")]
pub struct Address {
    pub id: i64,
    pub city_id: i64,
    pub line1: String,
    pub line2: Option<String>,
    pub district: Option<String>,
    pub postal_code: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// An address with its city and country, for showing it in one line.
#[derive(Serialize, Debug, Clone)]
pub struct FullAddress {
    #[serde(flatten)]
    pub address: Address,
    pub city: String,
    pub country: String,
}

impl FullAddress {
    /// "Calle Mayor 1, Madrid, Spain".
    pub fn line(&self) -> String {
        let mut parts = vec![self.address.line1.clone()];
        parts.extend(self.address.line2.clone());
        parts.push(self.city.clone());
        parts.push(self.country.clone());
        parts.join(", ")
    }

    /// The addresses with these ids, each with its city and country: three
    /// queries however many there are (addresses, their cities, their
    /// countries), keyed by address id.
    pub async fn load(db: &Db, ids: Vec<i64>) -> Result<HashMap<i64, FullAddress>> {
        let addresses = Address::find_many(db, ids).await?;
        let cities = belongs_to::<City, _, _>(db, &addresses, |a| a.city_id).await?;
        let city_list: Vec<City> = cities.values().cloned().collect();
        let countries = belongs_to::<Country, _, _>(db, &city_list, |c| c.country_id).await?;
        Ok(addresses
            .into_iter()
            .map(|address| {
                let city = cities.get(&address.city_id);
                let country = city.and_then(|c| countries.get(&c.country_id));
                let full = FullAddress {
                    city: city.map(|c| c.name.clone()).unwrap_or_default(),
                    country: country.map(|c| c.name.clone()).unwrap_or_default(),
                    address,
                };
                (full.address.id, full)
            })
            .collect())
    }
}

/// A customer of the company (Pagila's `customer`).
///
/// - `id_number` (a passport or ID card number, needed to rent a bike) is an
///   [`Encrypted<String>`]: sealed with `APP_KEY` when written, so the
///   table holds unreadable text; the app reads the plain value. It is
///   skipped when the customer is serialized, so it never reaches a
///   template or an API answer by accident.
/// - `id_verified_at`: when a member of staff checked the document.
/// - Soft deletes: a customer who leaves is hidden, not removed, because
///   rentals, orders and payments still point at them.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "customers", soft_deletes)]
pub struct Customer {
    pub id: i64,
    /// Their login, when they registered online; walk-ins have none.
    pub user_id: Option<i64>,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address_id: Option<i64>,
    #[serde(skip_serializing)]
    pub id_number: Option<Encrypted<String>>,
    pub id_verified_at: Option<DateTime>,
    pub active: bool,
    pub deleted_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

impl Customer {
    /// Whether staff have checked their ID document (needed to rent).
    pub fn id_verified(&self) -> bool {
        self.id_verified_at.is_some()
    }

    /// The ID number with all but its last three characters hidden
    /// (`•••••678`), for showing to staff who may not see it whole.
    pub fn masked_id_number(&self) -> Option<String> {
        self.id_number.as_ref().map(|n| {
            let chars: Vec<char> = n.chars().collect();
            let keep = chars.len().min(3);
            let hidden = "•".repeat(chars.len() - keep);
            format!(
                "{hidden}{}",
                chars[chars.len() - keep..].iter().collect::<String>()
            )
        })
    }

    /// The customer record of a logged-in user, if they have one.
    pub async fn of_user(db: &Db, user_id: i64) -> Result<Option<Customer>> {
        Customer::query()
            .where_(Customer::USER_ID.eq(user_id))
            .first(db)
            .await
    }
}
