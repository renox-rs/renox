//! The customer's bikes: registered by hand (a catalogue model or free
//! text, a frame number, a size, a photo) or by themselves when bought in
//! the shop (#234 writes the `customer_bikes` row with its order), and each
//! bike's service history.
//!
//! Only the owner sees a bike: every page looks it up by id **and** the
//! customer (another customer's id answers 404).

use renox::db::relations::{belongs_to, has_many};
use renox::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::model::{
    CustomerBike, ServiceTask, WorkOrder, WorkOrderNote, WorkOrderPart, WorkOrderTask,
};
use super::status::key;
use crate::app::catalog::model::{Category, CategoryKind, Product};
use crate::app::rentals::customer_of;
use crate::app::staff::model::Store;

/// How long a link to a bike's photo works.
pub const PHOTO_LINK: Duration = Duration::from_secs(10 * 60);

/// The customer's bike `id`, or a 404.
pub async fn own_bike(db: &Db, user: &User, id: i64) -> Result<CustomerBike> {
    let customer = customer_of(db, user).await?;
    CustomerBike::query()
        .where_(CustomerBike::ID.eq(id))
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .first(db)
        .await?
        .ok_or(Error::NotFound)
}

/// The catalogue's bike models, for the "model" select (id, name).
async fn bike_models(db: &Db) -> Result<Vec<(i64, String)>> {
    let categories: Vec<i64> = Category::query()
        .where_(Category::KIND.eq(CategoryKind::Bike))
        .pluck(db, Category::ID)
        .await?;
    Ok(Product::query()
        .where_(Product::CATEGORY_ID.is_in(categories))
        .order_by(Product::NAME)
        .limit(300)
        .get(db)
        .await?
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect())
}

/// `GET /bikes` (`workshop.bikes`): the customer's bikes, each with its
/// last service, and the form to add one.
// [explain:workshop.bikes.handler]
pub async fn index(State(state): State<AppState>, user: AuthUser) -> Result<View> {
    let db = &state.db;
    let customer = customer_of(db, &user).await?;
    let bikes = CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .order_by(CustomerBike::NAME)
        .get(db)
        .await?;
    let orders = has_many::<WorkOrder, _, _>(
        db,
        &bikes,
        WorkOrder::query().order_by_desc(WorkOrder::SCHEDULED_FOR),
        "customer_bike_id",
        |o: &WorkOrder| o.customer_bike_id.unwrap_or_default(),
    )
    .await?;
    // [/explain:workshop.bikes.handler]
    let rows: Vec<renox::serde_json::Value> = bikes
        .iter()
        .map(|bike| {
            let list = orders.get(&bike.id).cloned().unwrap_or_default();
            json!({
                "bike": bike,
                "services": list.len(),
                "last": list.first().map(|o| json!({ "id": o.id, "at": o.scheduled_for, "status": key(o.status) })),
                "has_photo": bike.photo_path.is_some(),
            })
        })
        .collect();
    let models = bike_models(db).await?;
    Ok(view("workshop/bikes.html", context! { rows, models }))
}

/// The form to add a bike.
// [explain:workshop.bikes.form]
#[derive(Deserialize, Validate)]
pub struct BikeForm {
    /// A catalogue model, or none for free text.
    pub product: Option<i64>,
    #[validate(required, max = 120)]
    pub name: String,
    #[validate(max = 60)]
    pub brand: Option<String>,
    #[validate(max = 40)]
    pub frame_number: Option<String>,
    #[validate(max = 20)]
    pub size: Option<String>,
    #[validate(image, max = 5120)]
    pub photo: Option<Upload>,
}
// [/explain:workshop.bikes.form]

/// `POST /bikes` (`workshop.bikes.store`): registers a bike (the photo is
/// a private upload).
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<BikeForm>,
) -> Result<(Toast, Redirect)> {
    let customer = customer_of(&state.db, &user).await?;
    let product = match form.product.filter(|p| *p > 0) {
        Some(id) => Product::find(&state.db, id).await?.map(|p| p.id),
        None => None,
    };
    let photo_path = match &form.photo {
        Some(photo) => Some(photo.store(&state.storage, "bikes").await?),
        None => None,
    };
    let bike = CustomerBike::create(
        &state.db,
        CustomerBike {
            customer_id: customer.id,
            product_id: product,
            name: form.name.trim().to_owned(),
            brand: form.brand.filter(|b| !b.trim().is_empty()),
            frame_number: form.frame_number.filter(|f| !f.trim().is_empty()),
            size: form.size.filter(|s| !s.trim().is_empty()),
            photo_path,
            ..Default::default()
        },
    )
    .await?;
    Ok((
        Toast::success(state.current_lang().t("workshop.bikes.added", &[])),
        Redirect::route("workshop.bikes.show", &[&bike.id])?,
    ))
}

/// `GET /bikes/{bike}/photo` (`workshop.bikes.photo`): the bike's photo
/// for its owner, through a signed temporary URL.
pub async fn photo(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Redirect> {
    let bike = own_bike(&state.db, &user, id).await?;
    let path = bike.photo_path.ok_or(Error::NotFound)?;
    let url = state
        .storage
        .temporary_url(&state, &path, PHOTO_LINK)
        .await?;
    Ok(Redirect::to(&url))
}

/// One event of a bike's history (the `history` block's item).
#[derive(Serialize)]
struct Event {
    time: DateTime,
    title: String,
    body: Option<String>,
    kind: Option<&'static str>,
}

/// `GET /bikes/{bike}` (`workshop.bikes.show`): the bike and its service
/// history: every work order with its tasks, parts and notes, as a
/// timeline. Seven queries however long the history is.
// [explain:workshop.bikes.show.handler]
pub async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let bike = own_bike(db, &user, id).await?;
    let orders = WorkOrder::query()
        .where_(WorkOrder::CUSTOMER_BIKE_ID.eq(bike.id))
        .order_by_desc(WorkOrder::SCHEDULED_FOR)
        .get(db)
        .await?;
    let tasks = has_many::<WorkOrderTask, _, _>(
        db,
        &orders,
        WorkOrderTask::query(),
        "work_order_id",
        |t: &WorkOrderTask| t.work_order_id,
    )
    .await?;
    // [/explain:workshop.bikes.show.handler]
    let all_tasks: Vec<WorkOrderTask> = tasks.values().flatten().cloned().collect();
    let names = belongs_to::<ServiceTask, _, _>(db, &all_tasks, |t| t.service_task_id).await?;
    let parts = has_many::<WorkOrderPart, _, _>(
        db,
        &orders,
        WorkOrderPart::query(),
        "work_order_id",
        |p: &WorkOrderPart| p.work_order_id,
    )
    .await?;
    // [explain:workshop.bikes.show.handler]
    let notes = has_many::<WorkOrderNote, _, _>(
        db,
        &orders,
        WorkOrderNote::query().where_(WorkOrderNote::BODY.is_not_null()),
        "work_order_id",
        |n: &WorkOrderNote| n.work_order_id,
    )
    .await?;
    let stores = belongs_to::<Store, _, _>(db, &orders, |o| o.store_id).await?;
    // [/explain:workshop.bikes.show.handler]
    let all_parts: Vec<WorkOrderPart> = parts.values().flatten().cloned().collect();
    let part_names = crate::app::rentals::reserve::variant_names(
        db,
        all_parts.iter().map(|p| p.variant_id).collect(),
    )
    .await?;
    let lang = state.current_lang();
    let mut history = Vec::new();
    for order in &orders {
        let mut lines: Vec<String> = tasks
            .get(&order.id)
            .into_iter()
            .flatten()
            .map(|t| {
                let name = names
                    .get(&t.service_task_id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                format!("- {}{name}", if t.done { "✓ " } else { "" })
            })
            .collect();
        for part in parts.get(&order.id).into_iter().flatten() {
            let (name, size) = part_names
                .get(&part.variant_id)
                .cloned()
                .unwrap_or_default();
            lines.push(format!(
                "- {} × {name}{}",
                part.quantity,
                size.map(|s| format!(" ({s})")).unwrap_or_default()
            ));
        }
        for note in notes.get(&order.id).into_iter().flatten() {
            lines.push(format!("> {}", note.body.clone().unwrap_or_default()));
        }
        let store = stores
            .get(&order.store_id)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        history.push(Event {
            time: order.completed_at.unwrap_or(order.scheduled_for),
            title: lang.t(
                "workshop.bikes.event",
                &[
                    ("number", &order.id),
                    ("store", &store),
                    (
                        "status",
                        &lang.t(&format!("workshop.status.{}", key(order.status)), &[]),
                    ),
                ],
            ),
            body: (!lines.is_empty()).then(|| lines.join("\n")),
            kind: match order.status {
                super::model::WorkStatus::Completed => Some("success"),
                super::model::WorkStatus::Cancelled => None,
                super::model::WorkStatus::WaitingParts
                | super::model::WorkStatus::WaitingApproval => Some("warning"),
                _ => Some("info"),
            },
        });
    }
    let model = match bike.product_id {
        Some(id) => Product::find(db, id).await?.map(|p| p.name),
        None => None,
    };
    let open: Vec<&WorkOrder> = orders.iter().filter(|o| o.is_open()).collect();
    Ok(view(
        "workshop/bike.html",
        context! {
            has_photo => bike.photo_path.is_some(),
            model,
            events => history,
            open => open.into_iter().cloned().collect::<Vec<_>>(),
            spent => orders.iter().filter(|o| o.status == super::model::WorkStatus::Completed).map(|o| o.total).sum::<i64>(),
            bike,
        },
    ))
}
