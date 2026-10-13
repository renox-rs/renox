//! Privacy: a customer downloads their data, and leaves.
//!
//! **Download my data.** The privacy section's button posts to
//! [`request_export`], which only queues [`ExportMyData`] and says "we'll
//! mail you a link": gathering years of orders and rentals is work for a
//! queue worker, not for the request. The job writes one JSON file to the
//! storage disk (`exports/…`, a random name), then mails a temporary link
//! to it (`Storage::temporary_url`: signed, works for seven days), in the
//! customer's language. A daily task deletes exports older than that.
//!
//! **Delete my account.** Renox's account page deletes the login after
//! the password is typed again, then announces `AccountDeleted`. The
//! listener here ([`on_account_deleted`]) makes the customer anonymous
//! ([`erase`]):
//!
//! - removed: name (→ "Deleted customer #id"), email, phone, home address,
//!   ID number and its check, the ID document's files
//!   (`customers/{id}/` on the storage disk), notes on work orders;
//! - kept, pointing at the anonymous record: orders, payments, rentals and
//!   work orders, which the books need (totals, dates, stores);
//! - cancelled: service plans still running.
//!
//! The record itself stays, soft deleted, so every order still has a
//! customer. Renox already removed what hangs off the login (sessions,
//! tokens, notifications) and the audit log keeps "account deleted" with
//! the user's id only.

use renox::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::model::Customer;
use crate::app::plans::model::PlanSubscription;
use crate::app::rentals::model::Rental;
use crate::app::sales::model::{Order, OrderItem, Payment};
use crate::app::workshop::model::{CustomerBike, WorkOrder};

/// How long an export's link works, and how long the file is kept.
pub const EXPORT_KEPT: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Where exports are written on the storage disk.
pub const EXPORT_DIR: &str = "exports";

/// Where a customer's files (their ID document) live on the storage disk:
/// `customers/{id}/`. Deleted with the account.
pub fn files_of(customer_id: i64) -> String {
    format!("customers/{customer_id}/")
}

/// `POST /account/export`: queues the export and says a mail will follow.
pub async fn request_export(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<(Toast, Redirect)> {
    state.dispatch(ExportMyData { user_id: user.id }).await?;
    let queued = state
        .current_lang()
        .t("accounts.privacy.export_queued", &[]);
    Ok((
        Toast::success(queued),
        Redirect::to(&state.url("account.show", &[])?),
    ))
}

/// Builds a customer's data as one JSON file and mails them a link to it.
#[derive(Serialize, Deserialize, Debug)]
pub struct ExportMyData {
    /// The login asking.
    pub user_id: i64,
}

impl Job for ExportMyData {
    const NAME: &'static str = "accounts-export-my-data";
    const MAX_ATTEMPTS: u32 = 3;

    async fn handle(self, ctx: JobContext) -> Result {
        let state = &ctx.state;
        let Some(user) = User::find(&state.db, self.user_id).await? else {
            return Ok(()); // deleted in the meantime: nothing to send
        };
        let data = export(&state.db, &user).await?;
        let key = format!("{EXPORT_DIR}/{}-{}.json", user.id, renox::random_token());
        let bytes = renox::serde_json::to_vec_pretty(&data)?;
        state.storage.put(&key, bytes.into()).await?;
        let link = state
            .storage
            .temporary_url(state, &key, EXPORT_KEPT)
            .await?;
        let to = renox::auth::Recipient::for_user(&user);
        let locale = to.locale().unwrap_or_else(|| state.config.locale.clone());
        let mail = state.mail_view_in(
            &locale,
            &user.email,
            state.lang(&locale).t("accounts.privacy.mail.subject", &[]),
            "mail/accounts/export_ready",
            context! { name => user.name, link, days => EXPORT_KEPT.as_secs() / 86_400 },
        )?;
        state.mailer.send(mail).await
    }
}

/// Everything the shop keeps about `user`, as JSON: the login, the
/// customer record (the ID number shown masked), their address, bikes,
/// orders with their lines, rentals, payments, work orders, plans and
/// notification choices. A fixed number of queries.
pub async fn export(db: &Db, user: &User) -> Result<renox::serde_json::Value> {
    let customer = Customer::of_user(db, user.id).await?;
    let account = json!({
        "name": user.name,
        "email": user.email,
        "email_verified_at": user.email_verified_at,
        "created_at": user.created_at,
        "language": super::locale::of(user),
        "notification_preferences": super::preferences::Preferences::of(user),
    });
    let Some(customer) = customer else {
        return Ok(json!({ "account": account }));
    };
    let address = match customer.address_id {
        Some(id) => super::model::FullAddress::load(db, vec![id])
            .await?
            .remove(&id)
            .map(|a| a.line()),
        None => None,
    };
    let orders = Order::query()
        .where_(Order::CUSTOMER_ID.eq(customer.id))
        .order_by(Order::ID)
        .get(db)
        .await?;
    let order_ids: Vec<i64> = orders.iter().map(|o| o.id).collect();
    let items = if order_ids.is_empty() {
        Vec::new()
    } else {
        OrderItem::query()
            .where_(OrderItem::ORDER_ID.is_in(order_ids))
            .order_by(OrderItem::ID)
            .get(db)
            .await?
    };
    let bikes = CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .order_by(CustomerBike::ID)
        .get(db)
        .await?;
    let bike_ids: Vec<i64> = bikes.iter().map(|b| b.id).collect();
    let (work_orders, plans) = if bike_ids.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        (
            WorkOrder::query()
                .where_(WorkOrder::CUSTOMER_BIKE_ID.is_in(bike_ids.clone()))
                .order_by(WorkOrder::ID)
                .get(db)
                .await?,
            PlanSubscription::query()
                .where_(PlanSubscription::CUSTOMER_BIKE_ID.is_in(bike_ids))
                .order_by(PlanSubscription::ID)
                .get(db)
                .await?,
        )
    };
    Ok(json!({
        "account": account,
        "customer": {
            "name": customer.name,
            "email": customer.email,
            "phone": customer.phone,
            "address": address,
            "id_number": customer.masked_id_number(),
            "id_verified_at": customer.id_verified_at,
            "created_at": customer.created_at,
        },
        "bikes": bikes,
        "orders": orders,
        "order_items": items,
        "rentals": Rental::query().where_(Rental::CUSTOMER_ID.eq(customer.id)).order_by(Rental::ID).get(db).await?,
        "payments": Payment::query().where_(Payment::CUSTOMER_ID.eq(customer.id)).order_by(Payment::ID).get(db).await?,
        "work_orders": work_orders,
        "plan_subscriptions": plans,
    }))
}

/// `AccountDeleted` (Renox's account page): makes the customer who had
/// this login anonymous. The login is already gone (the foreign key set
/// `customers.user_id` to `NULL`), so the record is found by the address
/// it had.
pub async fn on_account_deleted(
    event: renox::auth::events::AccountDeleted,
    state: AppState,
) -> Result {
    let email = super::claim::normalize_email(&event.email);
    let ids: Vec<i64> = renox::db::sql(
        "SELECT id FROM customers WHERE deleted_at IS NULL \
         AND (user_id = ? OR (user_id IS NULL AND lower(email) = ?))",
    )
    .bind(event.user_id)
    .bind(email)
    .scalars(&state.db)
    .await?;
    for id in ids {
        erase(&state, id).await?;
        renox::audit::record(
            &state.db,
            renox::audit::Entry::new("customer.erased")
                .user(event.user_id)
                .subject("customers", id),
        )
        .await?;
    }
    Ok(())
}

/// Makes customer `id` anonymous (see the module docs). Safe to run twice.
pub async fn erase(state: &AppState, id: i64) -> Result {
    let Some(customer) = Customer::query()
        .with_trashed()
        .where_(Customer::ID.eq(id))
        .first(&state.db)
        .await?
    else {
        return Ok(());
    };
    let now = renox::db::now();
    let mut tx = state.db.begin().await?;
    renox::db::sql(
        "UPDATE customers SET name = ?, email = NULL, phone = NULL, address_id = NULL, \
         id_number = NULL, id_verified_at = NULL, user_id = NULL, active = ?, \
         deleted_at = COALESCE(deleted_at, ?), updated_at = ? WHERE id = ?",
    )
    .bind(format!("Deleted customer #{id}"))
    .bind(false)
    .bind(now)
    .bind(now)
    .bind(id)
    .execute(&mut tx)
    .await?;
    // Notes the customer wrote about their bike's repair are theirs.
    renox::db::sql(
        "UPDATE work_orders SET customer_note = NULL \
         WHERE customer_bike_id IN (SELECT id FROM customer_bikes WHERE customer_id = ?)",
    )
    .bind(id)
    .execute(&mut tx)
    .await?;
    // The home address goes, unless an order was delivered there (the
    // order keeps it: the books need where goods went).
    if let Some(address_id) = customer.address_id {
        let used: i64 = renox::db::sql("SELECT COUNT(*) FROM orders WHERE delivery_address_id = ?")
            .bind(address_id)
            .scalar(&mut tx)
            .await?;
        let elsewhere: i64 = renox::db::sql(
            "SELECT (SELECT COUNT(*) FROM stores WHERE address_id = ?) \
                  + (SELECT COUNT(*) FROM suppliers WHERE address_id = ?) \
                  + (SELECT COUNT(*) FROM customers WHERE address_id = ? AND id <> ?)",
        )
        .bind(address_id)
        .bind(address_id)
        .bind(address_id)
        .bind(id)
        .scalar(&mut tx)
        .await?;
        if used == 0 && elsewhere == 0 {
            renox::db::sql("DELETE FROM addresses WHERE id = ?")
                .bind(address_id)
                .execute(&mut tx)
                .await?;
        }
    }
    tx.commit().await?;
    // A plan still running stops (nobody will bring the bike any more), with
    // its upcoming visits and their work orders, through the plans area so
    // the workshop's slots are freed too.
    let running: Vec<crate::app::plans::model::PlanSubscription> =
        crate::app::plans::model::PlanSubscription::query()
            .where_(
                PlanSubscription::STATUS
                    .ne(crate::app::plans::model::SubscriptionStatus::Cancelled),
            )
            .where_raw(
                "customer_bike_id IN (SELECT id FROM customer_bikes WHERE customer_id = ?)",
                [id],
            )
            .get(&state.db)
            .await?;
    for mut sub in running {
        crate::app::plans::visits::end(&state.db, &mut sub).await?;
    }
    // Files can't roll back with the transaction: removed once it committed.
    state.storage.delete_all(&files_of(id)).await?;
    state.emit(CustomerErased { customer_id: id }).await?;
    Ok(())
}

/// A customer was made anonymous ([`erase`]). Areas that keep personal
/// data of their own elsewhere listen to it and remove theirs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerErased {
    /// The anonymous record's id.
    pub customer_id: i64,
}

impl Event for CustomerErased {}

/// Deletes exports older than [`EXPORT_KEPT`] (a daily scheduled task).
pub async fn prune_exports(state: &AppState) -> Result<usize> {
    let cutoff =
        renox::db::now() - renox::chrono::Duration::from_std(EXPORT_KEPT).unwrap_or_default();
    let mut deleted = 0;
    for file in state.storage.list(&format!("{EXPORT_DIR}/")).await? {
        if file.modified.is_some_and(|at| at < cutoff) {
            state.storage.delete(&file.key).await?;
            deleted += 1;
        }
    }
    Ok(deleted)
}
