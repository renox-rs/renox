//! Customer accounts (#238): signing up and in, "My account" and its
//! sections, notifications and their preferences, the language, a walk-in
//! claiming their record, and privacy (download my data, delete my account).
//!
//! Most of it is Renox's, switched on in `src/lib.rs`:
//!
//! - `Auth::new().account().verify_email().notifications().on_registered(…)`:
//!   login, register, password reset, email verification, the `/account`
//!   page, the notification list and its live stream;
//! - `renox_oauth::OAuth::new().google().github()`: "Continue with Google /
//!   GitHub" when their keys are set (buttons hidden otherwise);
//! - `renox_2fa::TwoFactor`: two-factor login, optional for customers,
//!   required for staff (`src/app/staff/two_factor.rs`).
//!
//! What this area adds:
//!
//! - [`registration`]: signing up makes a `customers` row;
//! - [`claim`]: staff invite a walk-in customer to claim their record;
//! - the account page's sections ([`Accounts::register`]), through Renox's
//!   `Registry::account_section`: contact details and address, ID check,
//!   notification preferences, language, privacy. **Other areas add theirs
//!   the same way** (orders, rentals, bikes, work orders, plan, payments),
//!   with an `order` from [`section_order`] so the page reads top to bottom;
//! - [`preferences`]: `channels_for(to, Kind)`, which **every notification
//!   to a customer** calls in `Notification::channels`;
//! - [`locale`]: the language kept on the account as well as the session;
//! - [`privacy`]: the export job and erasing a customer who leaves.
//!
//! Views: `resources/views/accounts/`, the account page itself
//! (`resources/views/renox/auth/account.html`, which replaces Renox's), the
//! mails in `resources/views/mail/accounts/`. Tests: `tests/accounts.rs`.

pub mod claim;
pub mod demo_logins;
pub mod explain;
pub mod factories;
pub mod locale;
pub mod model;
pub mod preferences;
pub mod privacy;
pub mod registration;

use renox::prelude::*;
use serde::Deserialize;

use model::{Address, City, Country, Customer, FullAddress};
use preferences::{Choice, Kind, Preferences};

/// Where each area's section goes on the account page (`order` in
/// `Registry::account_section`): smaller first. Renox's own cards (name and
/// email, password, other devices) come before all of them, "Delete
/// account" after.
pub mod section_order {
    /// Contact details and home address (accounts).
    pub const CONTACT: i32 = 10;
    /// Whether the ID document was checked (accounts).
    pub const ID_CHECK: i32 = 20;
    /// My bikes (workshop).
    pub const BIKES: i32 = 30;
    /// My orders (sales).
    pub const ORDERS: i32 = 40;
    /// My rentals (rentals).
    pub const RENTALS: i32 = 50;
    /// My service visits (workshop).
    pub const WORK_ORDERS: i32 = 60;
    /// My plan (plans).
    pub const PLAN: i32 = 70;
    /// My payments (sales).
    pub const PAYMENTS: i32 = 80;
    /// Two-factor authentication (renox-2fa uses 100).
    pub const TWO_FACTOR: i32 = 100;
    /// Linked Google / GitHub accounts (renox-oauth).
    pub const LINKED: i32 = 105;
    /// How notifications reach me (accounts).
    pub const NOTIFICATIONS: i32 = 110;
    /// My language (accounts).
    pub const LANGUAGE: i32 = 120;
    /// Download my data (accounts).
    pub const PRIVACY: i32 = 130;
}

/// The accounts area, registered in `src/lib.rs`.
pub struct Accounts;

impl Module for Accounts {
    fn name(&self) -> &'static str {
        "accounts"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .put("/account/contact", update_contact)
            .name("accounts.contact")
            .put("/account/notifications", update_preferences)
            .name("accounts.preferences")
            .put("/account/language", update_language)
            .name("accounts.language")
            .post("/account/export", privacy::request_export)
            .name("accounts.export")
            .require_auth()
            .merge(claim::routes())
            .merge(claim::staff_routes())
    }

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<crate::app::accounts::model::Country>()
            .model::<crate::app::accounts::model::City>()
            .model::<crate::app::accounts::model::Address>()
            .model::<crate::app::accounts::model::Customer>();
        app.job::<privacy::ExportMyData>();
        app.listen(privacy::on_account_deleted);
        app.schedule()
            .daily_at("03:30", "accounts:prune-exports", |state| async move {
                privacy::prune_exports(&state).await.map(|_| ())
            });

        // [explain:account.show.sections]
        app.account_section(
            "accounts/sections/contact.html",
            section_order::CONTACT,
            |user, state| async move { contact_section(&state.db, &user).await },
        );
        app.account_section(
            "accounts/sections/id_check.html",
            section_order::ID_CHECK,
            |user, state| async move {
                let customer = Customer::of_user(&state.db, user.id).await?;
                Ok(json!({
                    "customer": customer.is_some(),
                    "verified_at": customer.as_ref().and_then(|c| c.id_verified_at),
                    "on_file": customer.as_ref().is_some_and(|c| c.id_number.is_some()),
                    "masked": customer.as_ref().and_then(Customer::masked_id_number),
                }))
            },
        );
        // [/explain:account.show.sections]
        app.account_section(
            "accounts/sections/notifications.html",
            section_order::NOTIFICATIONS,
            |user, _state| async move {
                let rows: Vec<_> = Preferences::of(&user)
                    .rows()
                    .into_iter()
                    .map(|(kind, choice)| json!({ "kind": kind.key(), "choice": choice.key() }))
                    .collect();
                let choices: Vec<&str> = Choice::ALL.iter().map(|c| c.key()).collect();
                Ok(json!({ "rows": rows, "choices": choices }))
            },
        );
        app.account_section(
            "accounts/sections/language.html",
            section_order::LANGUAGE,
            |user, state| async move {
                let current =
                    locale::of(&user).unwrap_or_else(|| renox::i18n::current_locale(&state));
                Ok(json!({ "current": current, "locales": crate::app::home::LOCALES }))
            },
        );
        app.account_section(
            "accounts/sections/privacy.html",
            section_order::PRIVACY,
            |_user, _state| async move {
                Ok(json!({ "days": privacy::EXPORT_KEPT.as_secs() / 86_400 }))
            },
        );
    }
}

/// The contact section: the customer's phone and home address, and the
/// countries for the address's select. Nothing for a login without a
/// customer record (most staff).
async fn contact_section(db: &Db, user: &User) -> Result<renox::serde_json::Value> {
    let Some(customer) = Customer::of_user(db, user.id).await? else {
        return Ok(json!({ "customer": null }));
    };
    let address = match customer.address_id {
        Some(id) => FullAddress::load(db, vec![id]).await?.remove(&id),
        None => None,
    };
    let country_id = match &address {
        Some(a) => City::find(db, a.address.city_id)
            .await?
            .map(|c| c.country_id),
        None => None,
    };
    let countries: Vec<(String, String)> = Country::query()
        .order_by(Country::NAME)
        .get(db)
        .await?
        .into_iter()
        .map(|c| (c.id.to_string(), c.name))
        .collect();
    Ok(json!({
        "customer": customer,
        "address": address,
        "country_id": country_id.map(|id| id.to_string()),
        "countries": countries,
    }))
}

// [explain:account.show.contact]
/// The contact form: a phone number and the home address.
#[derive(Deserialize, Validate)]
pub struct ContactForm {
    #[validate(max = 40)]
    pub phone: Option<String>,
    #[validate(required, max = 200)]
    pub line1: String,
    #[validate(max = 200)]
    pub line2: Option<String>,
    #[validate(max = 100)]
    pub district: Option<String>,
    #[validate(max = 20)]
    pub postal_code: Option<String>,
    #[validate(required, max = 100)]
    pub city: String,
    #[validate(required, exists("countries", "id"))]
    pub country_id: i64,
}
// [/explain:account.show.contact]

/// `PUT /account/contact`: saves the phone and the home address (the city
/// is found by name in the country, or added).
pub async fn update_contact(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<ContactForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut customer = registration::customer_of(db, &user).await?;
    let city_name = form.city.trim().to_owned();
    let city = match City::query()
        .where_(City::COUNTRY_ID.eq(form.country_id))
        .where_(City::NAME.eq(city_name.clone()))
        .first(db)
        .await?
    {
        Some(city) => city,
        None => {
            City::create(
                db,
                City {
                    country_id: form.country_id,
                    name: city_name,
                    ..Default::default()
                },
            )
            .await?
        }
    };
    let mut address = match customer.address_id {
        Some(id) => Address::find(db, id).await?.unwrap_or_default(),
        None => Address::default(),
    };
    address.city_id = city.id;
    address.line1 = form.line1;
    address.line2 = form.line2;
    address.district = form.district;
    address.postal_code = form.postal_code;
    address.save(db).await?;
    customer.address_id = Some(address.id);
    customer.phone = form.phone;
    customer.save(db).await?;
    saved(&state, "accounts.contact.saved")
}

/// The notification preferences form: one choice per kind.
#[derive(Deserialize, Validate)]
pub struct PreferencesForm {
    #[validate(required, one_of(&["both", "mail", "in_app", "none"]))]
    pub order: String,
    #[validate(required, one_of(&["both", "mail", "in_app", "none"]))]
    pub rental: String,
    #[validate(required, one_of(&["both", "mail", "in_app", "none"]))]
    pub workshop: String,
    #[validate(required, one_of(&["both", "mail", "in_app", "none"]))]
    pub plan: String,
    #[validate(required, one_of(&["both", "mail", "in_app", "none"]))]
    pub marketing: String,
}

/// `PUT /account/notifications`: saves how each kind reaches the customer.
pub async fn update_preferences(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<PreferencesForm>,
) -> Result<(Toast, Redirect)> {
    let mut chosen = Preferences::default();
    for (kind, value) in [
        (Kind::Order, &form.order),
        (Kind::Rental, &form.rental),
        (Kind::Workshop, &form.workshop),
        (Kind::Plan, &form.plan),
        (Kind::Marketing, &form.marketing),
    ] {
        if let Some(choice) = Choice::from_key(value) {
            chosen.0.insert(kind, choice);
        }
    }
    let mut user = user.user().clone();
    preferences::save(&state.db, &mut user, &chosen).await?;
    saved(&state, "accounts.notifications.saved")
}

/// The language form.
#[derive(Deserialize, Validate)]
pub struct LanguageForm {
    #[validate(required, one_of(&["en", "es"]))]
    pub locale: String,
}

/// `PUT /account/language`: the language of the pages (from the next one
/// on, in this session) and of the mails (on the account).
pub async fn update_language(
    State(state): State<AppState>,
    user: AuthUser,
    session: Session,
    Valid(form): Valid<LanguageForm>,
) -> Result<(Toast, Redirect)> {
    let mut user = user.user().clone();
    user.set(&state.db, locale::COLUMN, form.locale.clone())
        .await?;
    renox::i18n::remember_locale(&session, &form.locale)?;
    // The toast is written in the new language.
    renox::i18n::set_current_locale(&form.locale);
    saved(&state, "accounts.language.saved")
}

/// Back to the account page with a "saved" toast.
fn saved(state: &AppState, key: &str) -> Result<(Toast, Redirect)> {
    Ok((
        Toast::success(state.current_lang().t(key, &[])),
        Redirect::to(&state.url("account.show", &[])?),
    ))
}
