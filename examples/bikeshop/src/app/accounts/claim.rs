//! A walk-in customer claims their record.
//!
//! The cashier knows Ana: she bought a bike at the counter last year and
//! rents one now and then, but she has never signed up, so her `customers`
//! row has no login. From her record, staff send her an invitation
//! ([`create`], [`send`]): a mail with a **signed link** (`renox::signed`,
//! valid for seven days) to `/claim/{customer}/{email}`. The signature
//! covers the customer's id and the address it was sent to, so the link
//! can't be changed to claim someone else's record.
//!
//! Opening it ([`show`]) asks her to log in or sign up first (the route
//! needs a login; Renox brings her back here afterwards), with the address
//! the invitation went to. Then one button ([`claim`]) links the record to
//! her login, in a transaction:
//!
//! - anything her new account already had (an order placed before she
//!   claimed) moves to the walk-in record, and the empty row made at
//!   sign-up goes;
//! - the walk-in row gets her login and address.
//!
//! **Once only:** a record that already has a login can't be claimed again
//! (the second click, or a forwarded mail, finds it taken).

use renox::axum::extract::OriginalUri;
use renox::prelude::*;
use renox::signed::ValidSignature;
use std::time::Duration;

use super::model::Customer;
use crate::app::access::catalogue::CUSTOMERS_MANAGE;

/// How long an invitation link works.
pub const VALID_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The tables whose rows belong to a customer, moved when a record is claimed.
pub const CUSTOMER_TABLES: [&str; 4] = ["orders", "payments", "rentals", "customer_bikes"];

// [explain:accounts.invite.routes]
/// Staff routes: the invitation form and sending it (`customers.manage`
/// in the active store).
pub fn staff_routes() -> Routes {
    crate::app::access::staff_routes(
        Routes::new()
            .get("/staff/customers/{customer}/invite", create)
            .name("accounts.invite")
            .post("/staff/customers/{customer}/invite", send)
            .name("accounts.invite.send")
            .require_permission(CUSTOMERS_MANAGE),
    )
}
// [/explain:accounts.invite.routes]

/// The customer's routes: the signed link, for someone logged in.
pub fn routes() -> Routes {
    Routes::new()
        .get("/claim/{customer}/{email}", show)
        .name("accounts.claim")
        .post("/claim/{customer}/{email}", claim)
        .name("accounts.claim.store")
        .require_auth()
}

/// An address as Renox stores it on `users`: trimmed, lowercase.
pub fn normalize_email(email: &str) -> String {
    renox::auth::normalize_email(email)
}

/// A walk-in record that can still be invited: not claimed, not deleted.
async fn walk_in(db: &Db, id: i64) -> Result<Customer> {
    let customer = Customer::find_or_404(db, id).await?;
    if customer.user_id.is_some() || customer.deleted_at.is_some() {
        return Err(Error::NotFound);
    }
    Ok(customer)
}

/// `GET /staff/customers/{customer}/invite`: who to invite, at which address.
pub async fn create(State(db): State<Db>, Path(id): Path<i64>) -> Result<View> {
    let customer = walk_in(&db, id).await?;
    Ok(view("accounts/invite.html", context! { customer }))
}

// [explain:accounts.invite.send]
/// The invitation form.
#[derive(serde::Deserialize, Validate)]
pub struct InviteForm {
    #[validate(required, email, max = 255)]
    pub email: String,
}
// [/explain:accounts.invite.send]

// [explain:accounts.invite.send]
/// `POST /staff/customers/{customer}/invite`: saves the address on the
/// record and mails the signed link (queued: the counter doesn't wait for
/// the mail server).
pub async fn send(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Valid(form): Valid<InviteForm>,
) -> Result<(Toast, Redirect)> {
    let mut customer = walk_in(&state.db, id).await?;
    let email = normalize_email(&form.email);
    customer.email = Some(email.clone());
    customer.save(&state.db).await?;
    let link = state.signed_url("accounts.claim", &[&customer.id, &email], VALID_FOR)?;
    let mail = state.mail_view(
        &email,
        state.current_lang().t("accounts.claim.mail.subject", &[]),
        "mail/accounts/claim_invitation",
        context! { customer, link, days => VALID_FOR.as_secs() / 86_400 },
    )?;
    state.queue_mail(mail).await?;
    // [/explain:accounts.invite.send]
    let sent = state
        .current_lang()
        .t("accounts.invite.sent", &[("email", &email)]);
    Ok((
        Toast::success(sent),
        Redirect::to(&state.url("accounts.invite", &[&id])?),
    ))
}

/// What the claim page shows about the record.
#[derive(serde::Serialize)]
struct Summary {
    name: String,
    orders: i64,
    rentals: i64,
    bikes: i64,
}

async fn summary(db: &Db, customer: &Customer) -> Result<Summary> {
    let count = |table: &'static str| {
        renox::db::sql(format!(
            "SELECT COUNT(*) FROM {table} WHERE customer_id = ?"
        ))
        .bind(customer.id)
        .scalar::<i64>(db)
    };
    Ok(Summary {
        name: customer.name.clone(),
        orders: count("orders").await?,
        rentals: count("rentals").await?,
        bikes: count("customer_bikes").await?,
    })
}

// [explain:accounts.claim.show]
/// `GET /claim/{customer}/{email}` (signed): the record and a "This is me"
/// button, or why it can't be claimed.
pub async fn show(
    _: ValidSignature,
    State(db): State<Db>,
    user: AuthUser,
    OriginalUri(uri): OriginalUri,
    Path((id, email)): Path<(i64, String)>,
) -> Result<View> {
    let customer = Customer::find_or_404(&db, id).await?;
    let problem = problem(&user, &customer, &email);
    let summary = summary(&db, &customer).await?;
    Ok(view(
        "accounts/claim.html",
        context! {
            summary,
            email,
            problem,
            action => uri.to_string(),
        },
    ))
}
// [/explain:accounts.claim.show]

/// Why `user` can't claim `customer` with an invitation sent to `email`.
fn problem(user: &User, customer: &Customer, email: &str) -> Option<&'static str> {
    if customer.user_id.is_some() || customer.deleted_at.is_some() {
        Some("taken")
    } else if normalize_email(email) != normalize_email(&user.email) {
        Some("other_email")
    } else {
        None
    }
}

/// `POST /claim/{customer}/{email}` (signed): links the record to the
/// logged-in user (see the module docs).
pub async fn claim(
    _: ValidSignature,
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, email)): Path<(i64, String)>,
) -> Result<(Toast, Redirect)> {
    let customer = Customer::find_or_404(&state.db, id).await?;
    if let Some(problem) = problem(&user, &customer, &email) {
        return Err(match problem {
            "taken" => Error::NotFound,
            _ => Error::Forbidden,
        });
    }
    link(&state.db, &user, customer.id).await?;
    renox::audit::record(
        &state.db,
        renox::audit::Entry::new("customer.claimed")
            .user(user.id)
            .subject("customers", id),
    )
    .await?;
    Ok((
        Toast::success(state.current_lang().t("accounts.claim.done", &[])),
        Redirect::to(&state.url("account.show", &[])?),
    ))
}

// [explain:accounts.claim.link]
/// Links the walk-in record `walk_in_id` to `user`, moving what their own
/// record had onto it. In one transaction; the `user_id IS NULL` condition
/// makes a second, concurrent claim change nothing.
pub async fn link(db: &Db, user: &User, walk_in_id: i64) -> Result {
    let own = Customer::of_user(db, user.id).await?;
    let mut tx = db.begin().await?;
    let claimed = renox::db::sql(
        "UPDATE customers SET user_id = ?, email = ?, updated_at = ? \
         WHERE id = ? AND user_id IS NULL AND deleted_at IS NULL",
    )
    .bind(user.id)
    .bind(user.email.clone())
    .bind(renox::db::now())
    .bind(walk_in_id)
    .execute(&mut tx)
    .await?;
    if claimed == 0 {
        return Err(Error::NotFound);
    }
    // [/explain:accounts.claim.link]
    if let Some(own) = own.filter(|own| own.id != walk_in_id) {
        for table in CUSTOMER_TABLES {
            renox::db::sql(format!(
                "UPDATE {table} SET customer_id = ? WHERE customer_id = ?"
            ))
            .bind(walk_in_id)
            .bind(own.id)
            .execute(&mut tx)
            .await?;
        }
        renox::db::sql("DELETE FROM customers WHERE id = ?")
            .bind(own.id)
            .execute(&mut tx)
            .await?;
    }
    // [explain:accounts.claim.link]
    tx.commit().await?;
    Ok(())
}
// [/explain:accounts.claim.link]
