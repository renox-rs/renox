//! Small building blocks for tests: a store, a person with a role in a
//! store (or everywhere), a bike of the fleet. Every story's tests use
//! them, so a test makes exactly the rows it needs:
//!
//! ```no_run
//! use bikeshop::seed::fixtures;
//! use bikeshop::app::access::catalogue::{CASHIER, MANAGER};
//! # async fn demo(db: &renox::db::Db) -> renox::Result {
//! fixtures::roles(db).await?;
//! let north = fixtures::store(db, "North").await?;
//! let ana = fixtures::person(db, "ana@example.com", &[(MANAGER, Some(north.id))]).await?;
//! # let _ = (ana, CASHIER); Ok(()) }
//! ```

use renox::auth::User;
use renox::chrono::Duration;
use renox::db::Db;
use renox::prelude::*;

use super::shop::slugify;
use super::unique;
use crate::app::access::catalogue;
use crate::app::access::policy::store_scope;
use crate::app::accounts::model::{Address, City, Country};
use crate::app::catalog::factories::{ProductStates, variants_of};
use crate::app::catalog::model::{Brand, Category, CategoryKind, Product};
use crate::app::rentals::factories::{BikeStates, rental_bikes};
use crate::app::rentals::model::RentalBike;
use crate::app::staff::factories::stores_at;
use crate::app::staff::model::{Staff, Store};

/// Defines the catalogue's roles (`catalogue::define_roles`).
pub async fn roles(db: &Db) -> Result {
    catalogue::define_roles(db).await
}

/// A store called `name`, with an address in a city of its own.
pub async fn store(db: &Db, name: &str) -> Result<Store> {
    let n = unique();
    let country = Country::create(
        db,
        Country {
            name: format!("Country {n}"),
            code: format!("Q{n}"),
            ..Default::default()
        },
    )
    .await?;
    let city = City::create(
        db,
        City {
            country_id: country.id,
            name: format!("City {n}"),
            ..Default::default()
        },
    )
    .await?;
    let address = Address::create(
        db,
        Address {
            city_id: city.id,
            line1: format!("{n} Main Street"),
            ..Default::default()
        },
    )
    .await?;
    let mut store = stores_at(address.id).make_one();
    store.name = name.into();
    store.slug = format!("{}-{n}", slugify(name));
    store.insert(db).await?;
    Ok(store)
}

/// A person on the staff with these roles: `(role, Some(store))` in a
/// store, `(role, None)` everywhere. Their home store is the first store
/// listed.
pub async fn person(db: &Db, email: &str, roles: &[(&str, Option<i64>)]) -> Result<User> {
    let user = User::register(db, email, email, "password123").await?;
    if let Some(home) = roles.iter().find_map(|(_, store)| *store) {
        Staff::create(
            db,
            Staff {
                user_id: user.id,
                home_store_id: home,
                active: true,
                ..Default::default()
            },
        )
        .await?;
    }
    for (role, store) in roles {
        match store {
            Some(store) => user.assign_role_in(db, role, &store_scope(*store)).await?,
            None => user.assign_role(db, role).await?,
        }
    }
    Ok(user)
}

/// Gives `user` `role` in `store` from `days_from_now` for `days` days.
pub async fn dated_role(
    db: &Db,
    user: &User,
    role: &str,
    store: i64,
    days_from_now: i64,
    days: i64,
) -> Result {
    let start = renox::db::now() + Duration::days(days_from_now);
    user.assign_role_in(db, role, &store_scope(store))
        .from(start)
        .until(start + Duration::days(days))
        .await
}

/// A bike of the fleet owned by `owner` and standing at `location`, of a
/// fresh city-bike model.
pub async fn bike(db: &Db, owner: i64, location: i64) -> Result<RentalBike> {
    let n = unique();
    let category = Category::create(
        db,
        Category {
            name: format!("City bikes {n}"),
            slug: format!("city-bikes-{n}"),
            kind: CategoryKind::Bike,
            ..Default::default()
        },
    )
    .await?;
    let brand = Brand::create(
        db,
        Brand {
            name: format!("Brand {n}"),
            slug: format!("brand-{n}"),
            ..Default::default()
        },
    )
    .await?;
    let product: Product = Product::factory()
        .of(category.id, brand.id)
        .create_one(db)
        .await?;
    let variant = variants_of(product.id).create_one(db).await?;
    rental_bikes()
        .model(variant.id)
        .owned_by(owner)
        .placed_at(location)
        .create_one(db)
        .await
}
