//! The ID check before a first rental: the customer sends a photo of their
//! ID document and its number once; staff of the store they chose check it
//! and approve it; from then on they rent in any store.
//!
//! - The photo is a **private** upload (`Upload::store`, under the storage
//!   disk's `identity/`, never `public/`), checked to be an image by its
//!   content (`image()` sniffs the bytes, not the name).
//! - The number is the customer's `id_number`, an `Encrypted<String>`:
//!   sealed with `APP_KEY`, so the database holds unreadable text. Staff see
//!   it masked (`•••••678`).
//! - Staff open the photo through `GET /staff/identities/{document}/photo`,
//!   which checks `rentals.verify_id` in the document's store (or in a
//!   store serving one of the customer's open rentals) and then redirects
//!   to a **signed temporary URL** that works for five minutes. Nobody else
//!   ever gets a link to it.

use renox::db::Encrypted;
use renox::prelude::*;
use serde::Deserialize;
use std::time::Duration;

use super::model::{IdentityDocument, IdentityStatus, Rental};
use super::notify::{self, Notice, Tone};
use super::{booking, customer_of};
use crate::app::access::{self, catalogue};
use crate::app::accounts::model::Customer;
use crate::app::accounts::preferences::Kind;
use crate::app::staff::model::Store;

/// How long a staff member's link to an ID photo works.
pub const PHOTO_LINK: Duration = Duration::from_secs(5 * 60);

/// The customer's latest ID document, if they sent one.
pub async fn latest(db: &Db, customer_id: i64) -> Result<Option<IdentityDocument>> {
    IdentityDocument::query()
        .where_(IdentityDocument::CUSTOMER_ID.eq(customer_id))
        .order_by_desc(IdentityDocument::ID)
        .first(db)
        .await
}

/// Whether the customer may book: verified, or with a document waiting
/// for staff (pick-up still needs it approved).
pub async fn submitted(db: &Db, customer: &Customer) -> Result<bool> {
    if customer.id_verified() {
        return Ok(true);
    }
    Ok(latest(db, customer.id)
        .await?
        .is_some_and(|d| d.status == IdentityStatus::Pending))
}

/// `GET /rentals/identity` (`rentals.identity`): the customer's ID check,
/// its status, and the form to send (or send again) the document.
pub async fn edit(State(state): State<AppState>, user: AuthUser) -> Result<View> {
    let customer = customer_of(&state.db, &user).await?;
    let document = latest(&state.db, customer.id).await?;
    let stores = Store::all_by_name(&state.db).await?;
    Ok(view(
        "rentals/identity.html",
        context! {
            verified => customer.id_verified(),
            verified_at => customer.id_verified_at,
            masked => customer.masked_id_number(),
            document,
            stores => stores.iter().map(|s| (s.id, s.name.clone())).collect::<Vec<_>>(),
        },
    ))
}

/// The ID form.
// [explain:rentals.identity.form]
#[derive(Deserialize)]
pub struct IdentityForm {
    pub id_number: String,
    pub store: i64,
    pub photo: Option<Upload>,
}

impl Validate for IdentityForm {
    fn prepare(&mut self) {
        self.id_number = self.id_number.trim().to_uppercase();
    }

    fn rules(&self, v: &mut Validator) {
        v.field("id_number", &self.id_number)
            .required()
            .min(5)
            .max(30)
            .matches(r"^[A-Z0-9\- ]+$");
        v.field("store", &self.store).exists("stores", "id");
        // An image by its content (sniffed), at most 5 MB.
        v.field("photo", &self.photo)
            .required()
            .image()
            .max(5 * 1024);
    }
}
// [/explain:rentals.identity.form]

/// `POST /rentals/identity` (`rentals.identity.store`): stores the photo
/// privately, seals the number, and asks the chosen store's staff to check
/// it (an in-app notification to everyone there who may verify IDs).
// [explain:rentals.identity.store]
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    session: Session,
    Valid(form): Valid<IdentityForm>,
) -> Result<Redirect> {
    let mut customer = customer_of(&state.db, &user).await?;
    let photo = form.photo.ok_or(Error::BadRequest("photo".into()))?;
    let path = photo.store(&state.storage, "identity").await?;
    customer.id_number = Some(Encrypted::new(form.id_number));
    customer.id_verified_at = None;
    customer
        .save_only(&state.db, &["id_number", "id_verified_at"])
        .await?;
    // [/explain:rentals.identity.store]
    // Earlier pending documents are replaced by this one.
    IdentityDocument::query()
        .where_(IdentityDocument::CUSTOMER_ID.eq(customer.id))
        .where_(IdentityDocument::STATUS.eq(IdentityStatus::Pending))
        .update(&state.db, &[("status", &IdentityStatus::Refused)])
        .await?;
    // [explain:rentals.identity.store]
    IdentityDocument::create(
        &state.db,
        IdentityDocument {
            customer_id: customer.id,
            photo_path: path,
            store_id: form.store,
            status: IdentityStatus::Pending,
            submitted_at: renox::db::now(),
            ..Default::default()
        },
    )
    .await?;
    // [/explain:rentals.identity.store]
    notify::staff(
        &state,
        catalogue::RENTALS_VERIFY_ID,
        &[form.store],
        &Notice::new(
            "identity-submitted",
            "rentals.mail.identity_sent.title",
            "rentals.mail.identity_sent.body",
        )
        .param("name", &customer.name)
        .url(super::link(&state, "rentals.identities", None::<i64>)?),
    )
    .await?;
    session.flash(
        "status",
        state.current_lang().t("rentals.identity.sent", &[]),
    )?;
    Redirect::route("rentals.identity", &[])
}

/// The stores whose staff may look at `document`: the one the customer
/// chose, and the operating stores of their open rentals (the counter that
/// hands them a bike checks the ID there).
async fn checking_stores(db: &Db, document: &IdentityDocument) -> Result<Vec<i64>> {
    let mut stores: Vec<i64> = Rental::query()
        .where_(Rental::CUSTOMER_ID.eq(document.customer_id))
        .where_(Rental::STATUS.is_in(booking::HOLDING))
        .pluck(db, Rental::OPERATING_STORE_ID)
        .await?;
    stores.push(document.store_id);
    stores.sort_unstable();
    stores.dedup();
    Ok(stores)
}

/// `document` when `user` may check it, else a 404 (it doesn't exist for
/// them: ids can't be probed).
async fn checkable(db: &Db, user: &User, id: i64) -> Result<IdentityDocument> {
    let document = IdentityDocument::find_or_404(db, id).await?;
    let stores = checking_stores(db, &document).await?;
    if stores
        .iter()
        .any(|store| access::can_in(user, catalogue::RENTALS_VERIFY_ID, *store))
    {
        Ok(document)
    } else {
        Err(Error::NotFound)
    }
}

/// One waiting document as the staff list shows it.
#[derive(serde::Serialize)]
struct Waiting {
    document: IdentityDocument,
    customer: Option<Customer>,
    masked: Option<String>,
    store: Option<String>,
}

/// `GET /staff/identities` (`rentals.identities`): the ID documents
/// waiting at the stores where the user may verify IDs (`visible`, i.e.
/// `scopes_with` over the document's store), oldest first, each with a
/// button that opens the photo through a signed link. Three queries.
// [explain:rentals.identities.handler]
pub async fn index(State(state): State<AppState>) -> Result<View> {
    let documents = access::visible::<IdentityDocument>(catalogue::RENTALS_VERIFY_ID)
        .where_(IdentityDocument::STATUS.eq(IdentityStatus::Pending))
        .order_by(IdentityDocument::SUBMITTED_AT)
        .limit(100)
        .get(&state.db)
        .await?;
    let customers =
        renox::db::relations::belongs_to::<Customer, _, _>(&state.db, &documents, |d| {
            d.customer_id
        })
        .await?;
    let stores =
        renox::db::relations::belongs_to::<Store, _, _>(&state.db, &documents, |d| d.store_id)
            .await?;
    let waiting: Vec<Waiting> = documents
        .into_iter()
        .map(|document| {
            let customer = customers.get(&document.customer_id).cloned();
            Waiting {
                masked: customer.as_ref().and_then(|c| c.masked_id_number()),
                store: stores.get(&document.store_id).map(|s| s.name.clone()),
                customer,
                document,
            }
        })
        .collect();
    Ok(view("rentals/identities.html", context! { waiting }))
}
// [/explain:rentals.identities.handler]

/// `GET /staff/identities/{document}/photo` (`rentals.identities.photo`):
/// the photo, for staff who may check this document, through a signed
/// temporary URL (five minutes).
// [explain:rentals.identities.photo]
pub async fn photo(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Redirect> {
    let document = checkable(&state.db, &user, id).await?;
    let url = state
        .storage
        .temporary_url(&state, &document.photo_path, PHOTO_LINK)
        .await?;
    Ok(Redirect::to(&url))
}
// [/explain:rentals.identities.photo]

/// `POST /staff/identities/{document}/approve`: the customer is verified
/// (in every store) and told so.
pub async fn approve(
    State(state): State<AppState>,
    user: AuthUser,
    back: Back,
    Path(id): Path<i64>,
) -> Result<(Toast, Back)> {
    let mut document = checkable(&state.db, &user, id).await?;
    decide(&state, &user, &mut document, IdentityStatus::Approved, None).await?;
    Ok((
        Toast::success(state.current_lang().t("rentals.identities.approved", &[])),
        back,
    ))
}

/// Why a document is refused.
#[derive(Deserialize, Validate)]
pub struct RefuseForm {
    #[validate(required, max = 200)]
    pub note: String,
}

/// `POST /staff/identities/{document}/refuse`: refused with a note the
/// customer reads; they can send another.
pub async fn refuse(
    State(state): State<AppState>,
    user: AuthUser,
    back: Back,
    Path(id): Path<i64>,
    Valid(form): Valid<RefuseForm>,
) -> Result<(Toast, Back)> {
    let mut document = checkable(&state.db, &user, id).await?;
    decide(
        &state,
        &user,
        &mut document,
        IdentityStatus::Refused,
        Some(form.note),
    )
    .await?;
    Ok((
        Toast::info(state.current_lang().t("rentals.identities.refused", &[])),
        back,
    ))
}

/// Records the decision and tells the customer.
pub async fn decide(
    state: &AppState,
    user: &User,
    document: &mut IdentityDocument,
    status: IdentityStatus,
    note: Option<String>,
) -> Result {
    if document.status != IdentityStatus::Pending {
        return Ok(());
    }
    let now = renox::db::now();
    document.status = status;
    document.reviewed_by = Some(user.id);
    document.reviewed_at = Some(now);
    document.note = note.clone();
    document.save(&state.db).await?;
    let Some(mut customer) = Customer::find(&state.db, document.customer_id).await? else {
        return Ok(());
    };
    let notice = if status == IdentityStatus::Approved {
        customer.id_verified_at = Some(now);
        customer.save_only(&state.db, &["id_verified_at"]).await?;
        Notice::new(
            "identity-approved",
            "rentals.mail.identity_ok.title",
            "rentals.mail.identity_ok.body",
        )
        .tone(Tone::Success)
        .url(super::link(state, "rentals.create", None::<i64>)?)
    } else {
        Notice::new(
            "identity-refused",
            "rentals.mail.identity_refused.title",
            "rentals.mail.identity_refused.body",
        )
        .param("note", note.unwrap_or_default())
        .tone(Tone::Warning)
        .url(super::link(state, "rentals.identity", None::<i64>)?)
    };
    notify::customer(state, &customer, Kind::Rental, &notice).await
}
