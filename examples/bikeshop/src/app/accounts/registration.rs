//! Signing up makes a customer.
//!
//! Renox's `Auth` module owns `/register` (and `renox-oauth` makes accounts
//! for a first social login); [`on_registered`] is its
//! `Auth::on_registered` hook, given in `src/lib.rs`. It runs after the
//! user is saved and before they are logged in, for both ways in, and adds
//! the `customers` row that orders, rentals and bikes point at. If it fails,
//! Renox deletes the new user again, so no half-made account is left.
//!
//! It never links an existing walk-in record by email: the address isn't
//! verified yet when the hook runs, and anyone could type someone else's.
//! A walk-in claims their record through an invitation instead
//! ([`super::claim`]).

use renox::auth::Registration;
use renox::prelude::*;

use super::model::Customer;

// [explain:register.hook]
/// `Auth::on_registered`: a `customers` row for the new user, and the
/// language they signed up in saved on the account.
pub async fn on_registered(mut user: User, _form: Registration, state: AppState) -> Result {
    Customer::create(
        &state.db,
        Customer {
            user_id: Some(user.id),
            name: user.name.clone(),
            email: Some(user.email.clone()),
            active: true,
            ..Default::default()
        },
    )
    .await?;
    let locale = renox::i18n::current_locale(&state);
    if super::locale::supported(&locale) {
        user.set(&state.db, super::locale::COLUMN, locale).await?;
    }
    Ok(())
}
// [/explain:register.hook]

/// The customer record of `user`, made now if they have none (an account
/// that existed before the shop had customers, or a member of staff who
/// shops too).
pub async fn customer_of(db: &Db, user: &User) -> Result<Customer> {
    if let Some(customer) = Customer::of_user(db, user.id).await? {
        return Ok(customer);
    }
    Customer::create(
        db,
        Customer {
            user_id: Some(user.id),
            name: user.name.clone(),
            email: Some(user.email.clone()),
            active: true,
            ..Default::default()
        },
    )
    .await
}
