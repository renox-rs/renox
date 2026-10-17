//! A work order at the bench: the checklist of its tasks, notes and
//! before/after photos, the parts used (found by what fits the bike and
//! taken from the store's stock), extra work proposed to the customer, the
//! mechanic, the status, and the payment at the counter.
//!
//! Every action checks `workorders.update` (or `orders.sell` for the
//! payment) in the work order's **own** store, through the shared policy
//! helpers; staff of another store get a 404.

use renox::db::Json as DbJson;
use renox::db::relations::belongs_to;
use renox::prelude::*;
use renox::select::{OptionQuery, SelectOption};
use renox_billing::{Billing, BillingPayment, PaymentKind, PaymentLink, PaymentStatus};
use serde::{Deserialize, Serialize};

use super::approval;
use super::model::{
    CustomerBike, ExtraItem, ExtraStatus, ExtraWork, NoteKind, PartStatus, ServiceTask, WorkOrder,
    WorkOrderNote, WorkOrderPart, WorkOrderTask, WorkSource, WorkStatus,
};
use super::status::{self, allowed, from_key, key};
use crate::app::access::{self, StoreAttr, catalogue};
use crate::app::accounts::model::Customer;
use crate::app::accounts::preferences::Kind;
use crate::app::catalog::model::{Category, CategoryKind, FITTING_PARTS, Product, ProductVariant};
use crate::app::rentals::counter::{COUNTER_METHODS, counter_method};
use crate::app::rentals::model::RentalBike;
use crate::app::rentals::notify::{self, Notice, Tone};
use crate::app::rentals::reserve::{money, variant_names};
use crate::app::rentals::staff_id;
use crate::app::sales::payments::{self, Charge, Payable};
use crate::app::staff::model::Store;
use crate::app::stock::model::{MovementReason, StockLevel, StockMovement};

// [explain:workshop.order.policy]
/// The work order `id` for someone who may work on it (404 when it isn't
/// visible to them, 403 when they may only look).
async fn workable(db: &Db, user: &User, id: i64) -> Result<WorkOrder> {
    let order = access::find::<WorkOrder>(db, user, id).await?;
    access::require(
        user,
        catalogue::WORKORDERS_UPDATE,
        StoreAttr::Operating,
        &order,
    )?;
    Ok(order)
}
// [/explain:workshop.order.policy]

/// The catalogue product of the bike on the bench (a customer's bike's
/// model, or a fleet bike's), for "parts that fit".
async fn bike_product(db: &Db, order: &WorkOrder) -> Result<Option<i64>> {
    if let Some(id) = order.customer_bike_id {
        return Ok(CustomerBike::find(db, id).await?.and_then(|b| b.product_id));
    }
    if let Some(id) = order.rental_bike_id
        && let Some(bike) = RentalBike::find(db, id).await?
    {
        return Ok(ProductVariant::find(db, bike.variant_id)
            .await?
            .map(|v| v.product_id));
    }
    Ok(None)
}

/// A task of the checklist.
#[derive(Serialize)]
struct TaskRow {
    id: i64,
    name: String,
    minutes: i64,
    price: i64,
    done: bool,
}

/// A part on the order.
#[derive(Serialize)]
struct PartRow {
    id: i64,
    name: String,
    quantity: i64,
    total: i64,
    waiting: bool,
}

/// `GET /staff/workshop/{order}` (`workshop.order`): the work order's page.
pub async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let order = access::find::<WorkOrder>(db, &user, id).await?;
    let tasks = WorkOrderTask::query()
        .where_(WorkOrderTask::WORK_ORDER_ID.eq(order.id))
        .order_by(WorkOrderTask::ID)
        .get(db)
        .await?;
    let names = belongs_to::<ServiceTask, _, _>(db, &tasks, |t| t.service_task_id).await?;
    let tasks: Vec<TaskRow> = tasks
        .iter()
        .map(|t| TaskRow {
            id: t.id,
            name: names
                .get(&t.service_task_id)
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            minutes: t.minutes,
            price: t.price,
            done: t.done,
        })
        .collect();
    let parts = WorkOrderPart::query()
        .where_(WorkOrderPart::WORK_ORDER_ID.eq(order.id))
        .order_by(WorkOrderPart::ID)
        .get(db)
        .await?;
    let part_names = variant_names(db, parts.iter().map(|p| p.variant_id).collect()).await?;
    let parts: Vec<PartRow> = parts
        .iter()
        .map(|p| {
            let (name, size) = part_names.get(&p.variant_id).cloned().unwrap_or_default();
            PartRow {
                id: p.id,
                name: format!(
                    "{name}{}",
                    size.map(|s| format!(" ({s})")).unwrap_or_default()
                ),
                quantity: p.quantity,
                total: p.total,
                waiting: p.status == PartStatus::Waiting,
            }
        })
        .collect();
    let notes = WorkOrderNote::query()
        .where_(WorkOrderNote::WORK_ORDER_ID.eq(order.id))
        .order_by(WorkOrderNote::ID)
        .get(db)
        .await?;
    let note_rows: Vec<renox::serde_json::Value> = notes
        .iter()
        .map(|n| {
            json!({
                "id": n.id,
                "time": n.created_at,
                "kind": match n.kind { NoteKind::Note => "note", NoteKind::Before => "before", NoteKind::After => "after" },
                "body": n.body,
                "photo": n.photo_path.is_some(),
            })
        })
        .collect();
    let extras = ExtraWork::query()
        .where_(ExtraWork::WORK_ORDER_ID.eq(order.id))
        .order_by_desc(ExtraWork::ID)
        .get(db)
        .await?;
    let all_tasks: Vec<(i64, String)> = ServiceTask::query()
        .order_by(ServiceTask::NAME)
        .get(db)
        .await?
        .into_iter()
        .map(|t| (t.id, t.name))
        .collect();
    let (bike_name, customer) = match (order.customer_bike_id, order.rental_bike_id) {
        (Some(id), _) => {
            let bike = CustomerBike::find(db, id).await?;
            let customer = match &bike {
                Some(b) => Customer::find(db, b.customer_id).await?,
                None => None,
            };
            (bike.map(|b| b.name), customer)
        }
        (None, Some(id)) => (
            RentalBike::find(db, id).await?.map(|b| b.frame_number),
            None,
        ),
        _ => (None, None),
    };
    let store = Store::find(db, order.store_id).await?;
    let billed = match order.billed_store_id {
        Some(id) => Store::find(db, id).await?.map(|s| s.name),
        None => None,
    };
    let can_update = access::can(
        &user,
        catalogue::WORKORDERS_UPDATE,
        StoreAttr::Operating,
        &order,
    );
    let can_take_payment = access::can(&user, catalogue::ORDERS_SELL, StoreAttr::Operating, &order);
    let me = staff_id(db, &user).await?;
    let pay_link = if can_take_payment {
        latest_link(db, order.id).await?.map(|p| {
            json!({
                "url": p.url,
                "pending": p.status == PaymentStatus::Pending && !expired(&p),
                "failed": matches!(p.status, PaymentStatus::Failed | PaymentStatus::Expired)
                    || (p.status == PaymentStatus::Pending && expired(&p)),
            })
        })
    } else {
        None
    };
    let mechanic = match order.mechanic_id {
        Some(id) => match crate::app::staff::model::Staff::find(db, id).await? {
            Some(s) => User::find(db, s.user_id).await?.map(|u| u.name),
            None => None,
        },
        None => None,
    };
    let next: Vec<&str> = [
        WorkStatus::CheckedIn,
        WorkStatus::InProgress,
        WorkStatus::WaitingParts,
        WorkStatus::Ready,
        WorkStatus::Completed,
        WorkStatus::Cancelled,
    ]
    .into_iter()
    .filter(|to| allowed(order.status, *to))
    .map(key)
    .collect();
    let fits = bike_product(db, &order).await?.is_some();
    // The checklist's choices (id, name, minutes and price) and the ticked ones.
    let task_options: Vec<(i64, String, String)> = tasks
        .iter()
        .map(|t| {
            (
                t.id,
                t.name.clone(),
                format!("{} min · {}", t.minutes, money(&state, t.price)),
            )
        })
        .collect();
    let done_tasks: Vec<i64> = tasks.iter().filter(|t| t.done).map(|t| t.id).collect();
    // The notes as the timeline's items; a photo is a link in the text.
    let lang = state.current_lang();
    let mut note_history = Vec::new();
    for n in &notes {
        let kind = match n.kind {
            NoteKind::Note => "note",
            NoteKind::Before => "before",
            NoteKind::After => "after",
        };
        let mut body = n.body.clone().unwrap_or_default();
        if n.photo_path.is_some() {
            let url = state.url("workshop.order.photo", &[&order.id, &n.id])?;
            body.push_str(&format!(
                " [{}]({url})",
                lang.t("workshop.order.photo_link", &[])
            ));
        }
        note_history.push(json!({
            "time": n.created_at,
            "title": lang.t(&format!("workshop.notes.{kind}"), &[]),
            "body": body,
            "kind": if kind == "note" { "info" } else { "success" },
        }));
    }
    Ok(view(
        "workshop/order.html",
        context! {
            status => key(order.status),
            source => match order.source { WorkSource::WalkIn => "walk_in", WorkSource::Booking => "booking", WorkSource::Plan => "plan", WorkSource::Fleet => "fleet" },
            tasks,
            task_options,
            done_tasks,
            parts,
            notes => note_rows,
            note_history,
            extras,
            all_tasks,
            bike_name,
            customer,
            store,
            billed,
            mechanic,
            mine => me.is_some() && me == order.mechanic_id,
            can_update,
            can_take_payment,
            pay_link,
            payable => order.status == WorkStatus::Ready && order.paid_at.is_none() && order.total > 0 && order.source != WorkSource::Fleet,
            next,
            fits,
            methods => COUNTER_METHODS,
            order,
        },
    ))
}

/// A payment link's reference for a work order.
const LINK_REFERENCE: &str = "work_order";

/// The newest payment link made for the work order, if any.
async fn latest_link(db: &Db, order: i64) -> Result<Option<BillingPayment>> {
    Ok(
        BillingPayment::for_reference(db, LINK_REFERENCE, &order.to_string())
            .await?
            .into_iter()
            .find(|p| p.kind == PaymentKind::Link),
    )
}

/// Whether the link's time has run out.
fn expired(payment: &BillingPayment) -> bool {
    payment.expires_at.is_some_and(|at| at <= renox::db::now())
}

/// The checklist: the tasks ticked as done.
#[derive(Deserialize, Validate)]
pub struct ChecklistForm {
    #[serde(default)]
    pub done: Vec<i64>,
}

/// `POST /staff/workshop/{order}/tasks` (`workshop.order.tasks`).
pub async fn tasks(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<ChecklistForm>,
) -> Result<(Toast, Redirect)> {
    let order = workable(&state.db, &user, id).await?;
    let mut tx = state.db.begin().await?;
    WorkOrderTask::query()
        .where_(WorkOrderTask::WORK_ORDER_ID.eq(order.id))
        .update(&mut tx, &[("done", &false)])
        .await?;
    WorkOrderTask::query()
        .where_(WorkOrderTask::WORK_ORDER_ID.eq(order.id))
        .where_(WorkOrderTask::ID.is_in(form.done))
        .update(&mut tx, &[("done", &true)])
        .await?;
    tx.commit().await?;
    Ok((
        Toast::success(state.current_lang().t("workshop.order.saved", &[])),
        Redirect::route("workshop.order", &[&order.id])?,
    ))
}

/// A note, with a photo or not.
#[derive(Deserialize, Validate)]
pub struct NoteForm {
    #[validate(max = 1000)]
    pub body: Option<String>,
    #[validate(required, one_of(&["note", "before", "after"]))]
    pub kind: String,
    #[validate(image, max = 5120)]
    pub photo: Option<Upload>,
}

/// `POST /staff/workshop/{order}/notes` (`workshop.order.notes`): a note
/// or a before/after photo (a private upload).
pub async fn note(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<NoteForm>,
) -> Result<(Toast, Redirect)> {
    let order = workable(&state.db, &user, id).await?;
    let lang = state.current_lang();
    let body = form.body.filter(|b| !b.trim().is_empty());
    if body.is_none() && form.photo.is_none() {
        let mut errors = Errors::new();
        errors.add("body", lang.t("workshop.errors.empty_note", &[]));
        return Err(errors.into());
    }
    let photo_path = match &form.photo {
        Some(photo) => Some(photo.store(&state.storage, "workshop").await?),
        None => None,
    };
    WorkOrderNote::create(
        &state.db,
        WorkOrderNote {
            work_order_id: order.id,
            staff_id: staff_id(&state.db, &user).await?,
            kind: match form.kind.as_str() {
                "before" => NoteKind::Before,
                "after" => NoteKind::After,
                _ => NoteKind::Note,
            },
            body,
            photo_path,
            ..Default::default()
        },
    )
    .await?;
    Ok((
        Toast::success(lang.t("workshop.order.noted", &[])),
        Redirect::route("workshop.order", &[&order.id])?,
    ))
}

/// `GET /staff/workshop/{order}/notes/{note}/photo`
/// (`workshop.order.photo`): a note's photo through a signed temporary URL.
pub async fn photo(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, note)): Path<(i64, i64)>,
) -> Result<Redirect> {
    let order = access::find::<WorkOrder>(&state.db, &user, id).await?;
    let note = WorkOrderNote::query()
        .where_(WorkOrderNote::ID.eq(note))
        .where_(WorkOrderNote::WORK_ORDER_ID.eq(order.id))
        .first_or_404(&state.db)
        .await?;
    let path = note.photo_path.ok_or(Error::NotFound)?;
    let url = state
        .storage
        .temporary_url(&state, &path, super::bikes::PHOTO_LINK)
        .await?;
    Ok(Redirect::to(&url))
}

/// `GET /staff/workshop/{order}/parts` (`workshop.parts`): the parts for
/// the part select (`renox::select`): those that **fit** the bike on the
/// bench (`part_fits`, through `FITTING_PARTS`), or every spare part when
/// the bike isn't a catalogue model; each with the store's stock.
pub async fn part_options(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    query: OptionQuery,
) -> Result<Json<Vec<SelectOption>>> {
    let db = &state.db;
    let order = access::find::<WorkOrder>(db, &user, id).await?;
    let variants = if query.is_lookup() {
        ProductVariant::query()
            .where_(ProductVariant::ID.is_in(query.values_as::<i64>()))
            .get(db)
            .await?
    } else {
        let products: Vec<i64> = match bike_product(db, &order).await? {
            Some(bike) => FITTING_PARTS.ids(db, bike).await?,
            None => {
                let parts: Vec<i64> = Category::query()
                    .where_(Category::KIND.eq(CategoryKind::Part))
                    .pluck(db, Category::ID)
                    .await?;
                Product::query()
                    .where_(Product::CATEGORY_ID.is_in(parts))
                    .pluck(db, Product::ID)
                    .await?
            }
        };
        let q = query.q.trim().to_owned();
        let named: Vec<i64> = Product::query()
            .where_(Product::ID.is_in(products))
            .when(!q.is_empty(), |p| {
                p.where_(Product::NAME.like(&format!("%{q}%")))
            })
            .limit(40)
            .pluck(db, Product::ID)
            .await?;
        ProductVariant::query()
            .where_(ProductVariant::PRODUCT_ID.is_in(named))
            .order_by(ProductVariant::SKU)
            .limit(40)
            .get(db)
            .await?
    };
    let names = variant_names(db, variants.iter().map(|v| v.id).collect()).await?;
    let stock = StockLevel::query()
        .where_(StockLevel::VARIANT_ID.is_in(variants.iter().map(|v| v.id).collect::<Vec<_>>()))
        .where_(StockLevel::OWNER_STORE_ID.eq(order.store_id))
        .where_(StockLevel::LOCATION_STORE_ID.eq(order.store_id))
        .get(db)
        .await?;
    let lang = state.current_lang();
    Ok(Json(
        variants
            .iter()
            .map(|v| {
                let (name, size) = names.get(&v.id).cloned().unwrap_or_default();
                let left = stock
                    .iter()
                    .find(|s| s.variant_id == v.id)
                    .map(|s| s.available())
                    .unwrap_or(0);
                SelectOption::new(
                    v.id,
                    format!(
                        "{name}{} · {} · {}",
                        size.map(|s| format!(" ({s})")).unwrap_or_default(),
                        money(&state, v.price),
                        lang.t("workshop.parts.in_stock", &[("count", &left)])
                    ),
                )
            })
            .collect(),
    ))
}

/// Takes `quantity` of a part for `order` from the store's own stock in
/// one transaction (a ledger movement, reason `service`, pointing at the
/// work order), or records it as **waiting** and puts the order in
/// "waiting for parts" when the stock is short. `existing` is a waiting
/// line to fill now. Returns whether it was taken.
// [explain:workshop.order.part]
pub async fn take_part(
    state: &AppState,
    order: &mut WorkOrder,
    variant_id: i64,
    quantity: i64,
    existing: Option<WorkOrderPart>,
) -> Result<bool> {
    // [/explain:workshop.order.part]
    let db = &state.db;
    let variant = ProductVariant::find_or_404(db, variant_id).await?;
    // A plan's visit gets the plan's parts discount (#237).
    let unit_price = crate::app::plans::part_price(db, order, variant.price).await?;
    // [explain:workshop.order.part]
    let mut tx = db.begin_immediate().await?;
    let available = StockLevel::query()
        .where_(StockLevel::VARIANT_ID.eq(variant.id))
        .where_(StockLevel::OWNER_STORE_ID.eq(order.store_id))
        .where_(StockLevel::LOCATION_STORE_ID.eq(order.store_id))
        .first(&mut tx)
        .await?
        .map(|l| l.available())
        .unwrap_or(0);
    let taken = available >= quantity;
    if taken {
        StockMovement::record(
            &mut tx,
            StockMovement {
                variant_id: variant.id,
                owner_store_id: order.store_id,
                location_store_id: order.store_id,
                quantity: -quantity,
                reason: MovementReason::Service,
                reference_type: Some("work_orders".into()),
                reference_id: Some(order.id),
                note: Some(format!("Work order #{}", order.id)),
                ..Default::default()
            },
        )
        .await?;
    }
    // [/explain:workshop.order.part]
    let status = if taken {
        PartStatus::Used
    } else {
        PartStatus::Waiting
    };
    match existing {
        Some(mut line) => {
            line.status = status;
            line.save(&mut tx).await?;
        }
        None => {
            WorkOrderPart::create(
                &mut tx,
                WorkOrderPart {
                    work_order_id: order.id,
                    variant_id: variant.id,
                    quantity,
                    unit_price,
                    total: unit_price * quantity,
                    status,
                    ..Default::default()
                },
            )
            .await?;
        }
    }
    tx.commit().await?;
    status::recompute(db, order).await?;
    if !taken && allowed(order.status, WorkStatus::WaitingParts) {
        status::set_status(state, order, WorkStatus::WaitingParts).await?;
    }
    Ok(taken)
}

/// A part to use.
#[derive(Deserialize, Validate)]
pub struct PartForm {
    #[validate(required)]
    pub variant: Option<i64>,
    #[validate(required, min = 1, max = 20)]
    pub quantity: Option<i64>,
}

/// `POST /staff/workshop/{order}/parts` (`workshop.order.parts`).
pub async fn add_part(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<PartForm>,
) -> Result<(Toast, Redirect)> {
    let mut order = workable(&state.db, &user, id).await?;
    let taken = take_part(
        &state,
        &mut order,
        form.variant.unwrap_or_default(),
        form.quantity.unwrap_or(1),
        None,
    )
    .await?;
    let lang = state.current_lang();
    let toast = if taken {
        Toast::success(lang.t("workshop.parts.taken", &[]))
    } else {
        Toast::warning(lang.t("workshop.parts.short", &[]))
    };
    Ok((toast, Redirect::route("workshop.order", &[&order.id])?))
}

/// `POST /staff/workshop/{order}/parts/{part}/take`
/// (`workshop.order.parts.take`): a waited-for part has arrived.
pub async fn take_waiting(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, part)): Path<(i64, i64)>,
) -> Result<(Toast, Redirect)> {
    let mut order = workable(&state.db, &user, id).await?;
    let line = WorkOrderPart::query()
        .where_(WorkOrderPart::ID.eq(part))
        .where_(WorkOrderPart::WORK_ORDER_ID.eq(order.id))
        .where_(WorkOrderPart::STATUS.eq(PartStatus::Waiting))
        .first_or_404(&state.db)
        .await?;
    let taken = take_part(
        &state,
        &mut order,
        line.variant_id,
        line.quantity,
        Some(line),
    )
    .await?;
    let lang = state.current_lang();
    Ok((
        if taken {
            Toast::success(lang.t("workshop.parts.taken", &[]))
        } else {
            Toast::warning(lang.t("workshop.parts.short", &[]))
        },
        Redirect::route("workshop.order", &[&order.id])?,
    ))
}

/// Extra work to propose.
#[derive(Deserialize, Validate)]
pub struct ExtraForm {
    #[validate(required, max = 500)]
    pub description: String,
    #[serde(default)]
    pub tasks: Vec<i64>,
    pub part: Option<i64>,
    #[validate(min = 1, max = 20)]
    pub quantity: Option<i64>,
}

/// `POST /staff/workshop/{order}/extra` (`workshop.order.extra`): proposes
/// extra tasks and parts with their price; the customer gets a mail with a
/// signed link to approve or refuse it, and the order waits for the answer.
pub async fn propose(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<ExtraForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut order = workable(db, &user, id).await?;
    let lang = state.current_lang();
    let customer = status::customer_of(db, &order).await?;
    let mut items: Vec<ExtraItem> = ServiceTask::query()
        .where_(ServiceTask::ID.is_in(form.tasks.clone()))
        .get(db)
        .await?
        .into_iter()
        .map(|t| ExtraItem {
            kind: "task".into(),
            id: t.id,
            name: t.name,
            quantity: 1,
            price: t.price,
        })
        .collect();
    if let Some(variant) = form.part {
        let quantity = form.quantity.unwrap_or(1);
        if let Some(v) = ProductVariant::find(db, variant).await? {
            let (name, _) = variant_names(db, vec![v.id])
                .await?
                .remove(&v.id)
                .unwrap_or_default();
            items.push(ExtraItem {
                kind: "part".into(),
                id: v.id,
                name,
                quantity,
                price: v.price * quantity,
            });
        }
    }
    let mut errors = Errors::new();
    if items.is_empty() {
        errors.add("tasks", lang.t("workshop.errors.no_extra", &[]));
    }
    if customer.is_none() {
        errors.add("description", lang.t("workshop.errors.no_customer", &[]));
    }
    if !errors.is_empty() {
        return Err(errors.into());
    }
    let extra = ExtraWork::create(
        db,
        ExtraWork {
            work_order_id: order.id,
            description: form.description.trim().to_owned(),
            total: items.iter().map(|i| i.price).sum(),
            items: DbJson(items),
            status: ExtraStatus::Pending,
            expires_at: renox::db::now()
                + renox::chrono::Duration::seconds(approval::LINK_TTL.as_secs() as i64),
            created_by: staff_id(db, &user).await?,
            ..Default::default()
        },
    )
    .await?;
    if allowed(order.status, WorkStatus::WaitingApproval) {
        status::set_status(&state, &mut order, WorkStatus::WaitingApproval).await?;
    }
    if let Some(customer) = customer {
        let url = approval::signed_link(&state, &extra)?;
        notify::customer(
            &state,
            &customer,
            Kind::Workshop,
            &Notice::new(
                "workshop-extra",
                "workshop.mail.extra.title",
                "workshop.mail.extra.body",
            )
            .param("number", order.id)
            .param("description", &extra.description)
            .row("workshop.fields.extra_total", money(&state, extra.total))
            .tone(Tone::Warning)
            .view("mail/workshop/notice")
            .url(url),
        )
        .await?;
    }
    Ok((
        Toast::success(lang.t("workshop.order.proposed", &[])),
        Redirect::route("workshop.order", &[&order.id])?,
    ))
}

/// `POST /staff/workshop/{order}/assign` (`workshop.order.assign`): the
/// mechanic takes the job.
pub async fn assign(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut order = workable(&state.db, &user, id).await?;
    let lang = state.current_lang();
    let me = staff_id(&state.db, &user).await?.ok_or_else(|| {
        abort(
            StatusCode::FORBIDDEN,
            lang.t("rentals.errors.no_staff", &[]),
        )
    })?;
    order.mechanic_id = Some(me);
    order.save_only(&state.db, &["mechanic_id"]).await?;
    Ok((
        Toast::success(lang.t("workshop.order.assigned", &[])),
        Redirect::route("workshop.order", &[&order.id])?,
    ))
}

/// The status to move to.
#[derive(Deserialize, Validate)]
pub struct StatusForm {
    #[validate(required)]
    pub status: String,
}

/// `POST /staff/workshop/{order}/status` (`workshop.order.status`): one
/// step along the work order's life; collecting needs it paid (or a fleet
/// repair, billed between stores).
pub async fn change_status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<StatusForm>,
) -> Result<(Toast, Redirect)> {
    let mut order = workable(&state.db, &user, id).await?;
    let lang = state.current_lang();
    let to = from_key(&form.status).ok_or(Error::NotFound)?;
    if !allowed(order.status, to) {
        return Err(abort(
            StatusCode::UNPROCESSABLE_ENTITY,
            lang.t("workshop.errors.step", &[]),
        ));
    }
    if to == WorkStatus::Completed
        && order.source != WorkSource::Fleet
        && order.total > 0
        && order.paid_at.is_none()
    {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("workshop.errors.unpaid", &[]),
        ));
    }
    status::set_status(&state, &mut order, to).await?;
    Ok((
        Toast::success(lang.t(
            "workshop.order.moved",
            &[(
                "status",
                &lang.t(&format!("workshop.status.{}", key(to)), &[]),
            )],
        )),
        Redirect::route("workshop.order", &[&order.id])?,
    ))
}

/// Paying at the counter.
#[derive(Deserialize, Validate)]
pub struct PayForm {
    #[validate(required, one_of(&COUNTER_METHODS))]
    pub method: String,
}

/// `POST /staff/workshop/{order}/pay` (`workshop.order.pay`): the customer
/// pays the ready work order at the counter (`payments::record_counter`,
/// whose `PaymentSucceeded` marks it paid).
pub async fn pay(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<PayForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let order = access::find::<WorkOrder>(db, &user, id).await?;
    access::require(&user, catalogue::ORDERS_SELL, StoreAttr::Operating, &order)?;
    let lang = state.current_lang();
    if order.status != WorkStatus::Ready || order.paid_at.is_some() {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("workshop.errors.not_payable", &[]),
        ));
    }
    let staff = staff_id(db, &user).await?.ok_or_else(|| {
        abort(
            StatusCode::FORBIDDEN,
            lang.t("rentals.errors.no_staff", &[]),
        )
    })?;
    let customer = status::customer_of(db, &order).await?;
    payments::record_counter(
        &state,
        Charge {
            payable: Payable::WorkOrder(order.id),
            customer_id: customer.map(|c| c.id),
            store_id: order.store_id,
            amount: order.total,
        },
        counter_method(&form.method),
        staff,
    )
    .await?;
    Ok((
        Toast::success(lang.t("workshop.order.paid", &[])),
        Redirect::route("workshop.order", &[&order.id])?,
    ))
}

/// `POST /staff/workshop/{order}/pay-link` (`workshop.order.pay.link`):
/// sends the ready, unpaid work order's payment link (`orders.sell` in the
/// order's store). The payment is stored with the reference
/// `work_order` / the order's id; when the gateway confirms it,
/// renox-billing emits `PaymentSucceeded` with that reference and the
/// listener in [`super`] marks the order paid ([`super::booking::paid`]).
/// A link still open is shown again instead of making a second one.
pub async fn pay_link(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let order = access::find::<WorkOrder>(db, &user, id).await?;
    access::require(&user, catalogue::ORDERS_SELL, StoreAttr::Operating, &order)?;
    let lang = state.current_lang();
    if order.status != WorkStatus::Ready
        || order.paid_at.is_some()
        || order.total <= 0
        || order.source == WorkSource::Fleet
    {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("workshop.errors.not_payable", &[]),
        ));
    }
    let open = latest_link(db, order.id)
        .await?
        .is_some_and(|p| p.status == PaymentStatus::Pending && !expired(&p));
    if !open {
        let currency = renox::money::Currency::parse(&state.config.currency)
            .map_err(|e| renox::anyhow::anyhow!("APP_CURRENCY: {e}"))?;
        // Built before the await: the argument list isn't `Send`.
        let number = order.id.to_string();
        let link = PaymentLink::new(renox::money::Money::new(order.total, currency))
            .reference(LINK_REFERENCE, order.id)
            .description(lang.t("workshop.order.title", &[("number", &number)]));
        Billing::payment_link(&state, link).await?;
    }
    Ok((
        Toast::success(lang.t("workshop.order.link.made", &[])),
        Redirect::route("workshop.order", &[&order.id])?,
    ))
}
