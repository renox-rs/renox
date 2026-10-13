//! Booking a service (customers) and following it: choose a bike, a store,
//! a package or tasks and a day that has room; the estimate shows as the
//! form changes; the work order's page follows the work, lets the customer
//! reschedule or cancel until 24 hours before, and pay online when it is
//! ready.

use renox::chrono::NaiveDate;
use renox::prelude::*;
use renox::validation::FormContext;
use serde::{Deserialize, Serialize};

use super::capacity::{self, NewBooking, PACKAGES, chosen_tasks, estimate};
use super::model::{
    CustomerBike, ExtraStatus, ExtraWork, ServiceTask, WorkOrder, WorkOrderNote, WorkOrderPart,
    WorkOrderTask, WorkSource, WorkStatus,
};
use super::status::{self, key};
use crate::app::accounts::model::Customer;
use crate::app::accounts::preferences::Kind;
use crate::app::rentals::booking::to_local;
use crate::app::rentals::notify::{self, Notice, Tone};
use crate::app::rentals::{customer_of, link, reserve::money};
use crate::app::sales::payments::{self, Charge, Payable};
use crate::app::staff::model::Store;

/// The booking form, also read from the query string while it changes
/// (`Valid<T>` reads a GET's query too, with repeated `tasks`).
#[derive(Deserialize, Default, Debug)]
pub struct BookForm {
    pub bike: Option<i64>,
    pub store: Option<i64>,
    pub package: Option<String>,
    #[serde(default)]
    pub tasks: Vec<i64>,
    pub day: Option<NaiveDate>,
    pub note: Option<String>,
}

/// The GET form has no rules: every value is optional while choosing.
#[derive(Deserialize, Default, Debug)]
pub struct BookQuery {
    pub bike: Option<i64>,
    pub store: Option<i64>,
    pub package: Option<String>,
    #[serde(default)]
    pub tasks: Vec<i64>,
    pub day: Option<NaiveDate>,
}

impl Validate for BookQuery {
    fn rules(&self, _v: &mut Validator) {}
}

/// `GET /service/book` (`workshop.book`): the booking form. As it changes,
/// htmx asks the same page again and swaps the estimate and the day picker
/// (whose full days depend on the store and the minutes chosen).
// [explain:workshop.book.handler]
pub async fn form(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(query): Valid<BookQuery>,
) -> Result<View> {
    // [/explain:workshop.book.handler]
    let db = &state.db;
    let customer = customer_of(db, &user).await?;
    let bikes: Vec<(i64, String)> = CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .order_by(CustomerBike::NAME)
        .get(db)
        .await?
        .into_iter()
        .map(|b| (b.id, b.name))
        .collect();
    let stores = Store::all_by_name(db).await?;
    let store = query
        .store
        .and_then(|id| stores.iter().find(|s| s.id == id))
        .or_else(|| stores.first())
        .cloned();
    // [explain:workshop.book.handler]
    let all = ServiceTask::query()
        .order_by(ServiceTask::NAME)
        .get(db)
        .await?;
    let tasks = chosen_tasks(&all, query.package.as_deref(), &query.tasks);
    let est = estimate(&tasks);
    let full = match &store {
        Some(store) => capacity::full_days(db, &state.config, store, est.minutes).await?,
        None => Vec::new(),
    };
    // [/explain:workshop.book.handler]
    let today = to_local(&state.config, renox::db::now()).date();
    let task_options: Vec<(i64, String, String)> = all
        .iter()
        .map(|t| {
            (
                t.id,
                t.name.clone(),
                format!("{} min · {}", t.minutes, money(&state, t.price)),
            )
        })
        .collect();
    // The package choices: "none" first, then each package with its text.
    let lang = state.current_lang();
    let mut package_options: Vec<Vec<String>> =
        vec![vec!["none".into(), lang.t("workshop.book.no_package", &[])]];
    for (key, _) in PACKAGES {
        package_options.push(vec![
            key.to_string(),
            lang.t(&format!("workshop.packages.{key}.name"), &[]),
            lang.t(&format!("workshop.packages.{key}.text"), &[]),
        ]);
    }
    Ok(view(
        "workshop/book.html",
        context! {
            bikes,
            bike => query.bike,
            stores => stores.iter().map(|s| (s.id, s.name.clone())).collect::<Vec<_>>(),
            store_id => store.as_ref().map(|s| s.id),
            capacity => store.as_ref().map(|s| s.workshop_minutes_per_day),
            closed => store.as_ref().map(capacity::closed_weekdays).unwrap_or_default(),
            package_options,
            package => query.package.unwrap_or_default(),
            task_options,
            ticked => query.tasks,
            chosen => tasks,
            estimate => est,
            full,
            day => query.day.map(|d| d.to_string()),
            min_day => today.to_string(),
            max_day => (today + renox::chrono::Duration::days(capacity::BOOK_AHEAD_DAYS)).to_string(),
        },
    ))
}

impl Validate for BookForm {
    fn rules(&self, v: &mut Validator) {
        v.field("bike", &self.bike).required();
        v.field("store", &self.store)
            .required()
            .exists("stores", "id");
        v.field("day", &self.day).required();
        v.field("note", &self.note).max(500);
        let mut packages: Vec<&str> = PACKAGES.iter().map(|(k, _)| *k).collect();
        packages.push("none");
        v.field("package", &self.package).one_of(&packages);
    }

    /// The bike is the customer's, something is chosen, and the day has
    /// room (the first of two capacity checks; [`capacity::book`] checks
    /// again in its transaction).
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let db = &form.state.db;
        let lang = form.state.current_lang();
        let Some(user) = form.user else {
            return Ok(());
        };
        let customer = customer_of(db, user).await?;
        let owns = CustomerBike::query()
            .where_(CustomerBike::ID.eq(self.bike.unwrap_or_default()))
            .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
            .exists(db)
            .await?;
        if !owns {
            errors.add("bike", lang.t("workshop.errors.bike", &[]));
        }
        let all = ServiceTask::query().get(db).await?;
        let tasks = chosen_tasks(&all, self.package.as_deref(), &self.tasks);
        if tasks.is_empty() {
            errors.add("tasks", lang.t("workshop.errors.no_tasks", &[]));
            return Ok(());
        }
        if let (Some(store), Some(day)) = (
            Store::find(db, self.store.unwrap_or_default()).await?,
            self.day,
        ) && let Some(problem) = capacity::check_day(
            db,
            &form.state.config,
            &store,
            day,
            estimate(&tasks).minutes,
        )
        .await?
        {
            errors.add("day", lang.t(problem.key(), &[]));
        }
        Ok(())
    }
}

/// `POST /service/book` (`workshop.book.store`): books the work order in
/// one transaction (capacity checked again), mails the confirmation, and
/// opens the work order's page.
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<BookForm>,
) -> Result<Response> {
    let db = &state.db;
    let customer = customer_of(db, &user).await?;
    let all = ServiceTask::query().get(db).await?;
    let tasks = chosen_tasks(&all, form.package.as_deref(), &form.tasks);
    let booked = capacity::book(
        db,
        &state.config,
        NewBooking {
            bike_id: form.bike.unwrap_or_default(),
            store_id: form.store.unwrap_or_default(),
            day: form.day.unwrap_or_default(),
            tasks,
            package: form
                .package
                .clone()
                .filter(|p| !p.is_empty() && p != "none"),
            note: form.note.clone().filter(|n| !n.trim().is_empty()),
            source: WorkSource::Booking,
            checked_in: false,
        },
    )
    .await?;
    let order = match booked {
        Ok(order) => order,
        Err(problem) => {
            let mut errors = Errors::new();
            errors.add("day", state.current_lang().t(problem.key(), &[]));
            return Err(errors.into());
        }
    };
    confirm(&state, &customer, &order).await?;
    Ok((
        Toast::success(state.current_lang().t("workshop.book.booked", &[])),
        Redirect::route("workshop.service.show", &[&order.id])?,
    )
        .into_response())
}

/// The booking's confirmation mail and notification.
async fn confirm(state: &AppState, customer: &Customer, order: &WorkOrder) -> Result {
    let store = Store::find(&state.db, order.store_id).await?;
    let day = to_local(&state.config, order.scheduled_for)
        .format("%Y-%m-%d")
        .to_string();
    let url = link(state, "workshop.service.show", Some(order.id))?;
    notify::customer(
        state,
        customer,
        Kind::Workshop,
        &Notice::new(
            "workshop-booked",
            "workshop.mail.booked.title",
            "workshop.mail.booked.body",
        )
        .param("number", order.id)
        .param("day", &day)
        .param("store", store.map(|s| s.name).unwrap_or_default())
        .row("workshop.fields.day", &day)
        .row("workshop.fields.minutes", order.minutes)
        .row("workshop.fields.estimate", money(state, order.total))
        .tone(Tone::Success)
        .view("mail/workshop/notice")
        .url(url),
    )
    .await
}

/// The customer's own work order, or a 404 (fleet repairs and other
/// customers' bikes don't exist for them).
// [explain:workshop.service.show.owner]
pub async fn own_order(db: &Db, user: &User, id: i64) -> Result<WorkOrder> {
    let customer = customer_of(db, user).await?;
    let order = WorkOrder::find_or_404(db, id).await?;
    let bike_id = order.customer_bike_id.ok_or(Error::NotFound)?;
    let owns = CustomerBike::query()
        .where_(CustomerBike::ID.eq(bike_id))
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .exists(db)
        .await?;
    if owns {
        Ok(order)
    } else {
        Err(Error::NotFound)
    }
}
// [/explain:workshop.service.show.owner]

/// A line of the work order's page.
#[derive(Serialize)]
struct Line {
    name: String,
    detail: String,
    amount: i64,
    done: bool,
}

/// `GET /service/{order}` (`workshop.service.show`): the work order for
/// its customer: status, tasks (ticked as the mechanic does them), parts,
/// notes, extra work and its answer, the total; reschedule or cancel until
/// 24 hours before; pay online when it's ready.
// [explain:workshop.service.show.handler]
pub async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let order = own_order(db, &user, id).await?;
    // [/explain:workshop.service.show.handler]
    let tasks = WorkOrderTask::query()
        .where_(WorkOrderTask::WORK_ORDER_ID.eq(order.id))
        .get(db)
        .await?;
    let names =
        renox::db::relations::belongs_to::<ServiceTask, _, _>(db, &tasks, |t| t.service_task_id)
            .await?;
    let parts = WorkOrderPart::query()
        .where_(WorkOrderPart::WORK_ORDER_ID.eq(order.id))
        .get(db)
        .await?;
    let part_names = crate::app::rentals::reserve::variant_names(
        db,
        parts.iter().map(|p| p.variant_id).collect(),
    )
    .await?;
    let lang = state.current_lang();
    let mut lines: Vec<Line> = tasks
        .iter()
        .map(|t| Line {
            name: names
                .get(&t.service_task_id)
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            detail: format!("{} min", t.minutes),
            amount: t.price,
            done: t.done,
        })
        .collect();
    lines.extend(parts.iter().map(|p| {
        let (name, size) = part_names.get(&p.variant_id).cloned().unwrap_or_default();
        Line {
            name: format!(
                "{name}{}",
                size.map(|s| format!(" ({s})")).unwrap_or_default()
            ),
            detail: format!(
                "{} × {}{}",
                p.quantity,
                money(&state, p.unit_price),
                if p.status == super::model::PartStatus::Waiting {
                    format!(" · {}", lang.t("workshop.parts.waiting", &[]))
                } else {
                    String::new()
                }
            ),
            amount: p.total,
            done: p.status == super::model::PartStatus::Used,
        }
    }));
    let notes = WorkOrderNote::query()
        .where_(WorkOrderNote::WORK_ORDER_ID.eq(order.id))
        .where_(WorkOrderNote::BODY.is_not_null())
        .order_by(WorkOrderNote::ID)
        .get(db)
        .await?;
    let extras = ExtraWork::query()
        .where_(ExtraWork::WORK_ORDER_ID.eq(order.id))
        .order_by_desc(ExtraWork::ID)
        .get(db)
        .await?;
    let bike = match order.customer_bike_id {
        Some(id) => CustomerBike::find(db, id).await?,
        None => None,
    };
    let store = Store::find(db, order.store_id).await?;
    // [explain:workshop.service.show.handler]
    let steps: Vec<(&str, bool)> = status::BOARD
        .iter()
        .filter(|s| !matches!(s, WorkStatus::WaitingParts | WorkStatus::WaitingApproval))
        .chain([WorkStatus::Completed].iter())
        .map(|s| (key(*s), position(*s) <= position(order.status)))
        .collect();
    // [/explain:workshop.service.show.handler]
    Ok(view(
        "workshop/service.html",
        context! {
            status => key(order.status),
            changeable => order.changeable(),
            payable => order.status == WorkStatus::Ready && order.paid_at.is_none() && order.total > 0,
            pending_extra => extras.iter().any(|e| e.status == ExtraStatus::Pending),
            lines,
            notes,
            extras,
            bike,
            store,
            steps,
            today => to_local(&state.config, renox::db::now()).date().to_string(),
            day => to_local(&state.config, order.scheduled_for).date().to_string(),
            order,
        },
    ))
}

/// Where a status sits on the customer's progress line.
fn position(status: WorkStatus) -> u8 {
    match status {
        WorkStatus::Booked | WorkStatus::Cancelled => 0,
        WorkStatus::CheckedIn => 1,
        WorkStatus::InProgress | WorkStatus::WaitingParts | WorkStatus::WaitingApproval => 2,
        WorkStatus::Ready => 3,
        WorkStatus::Completed => 4,
    }
}

/// `POST /service/{order}/cancel` (`workshop.service.cancel`): until 24
/// hours before.
pub async fn cancel(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut order = own_order(&state.db, &user, id).await?;
    let lang = state.current_lang();
    if !order.changeable() {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("workshop.service.too_late", &[]),
        ));
    }
    status::set_status(&state, &mut order, WorkStatus::Cancelled).await?;
    Ok((
        Toast::info(lang.t("workshop.service.cancelled", &[])),
        Redirect::route("workshop.service.show", &[&order.id])?,
    ))
}

/// The new day.
#[derive(Deserialize, Validate)]
pub struct RescheduleForm {
    #[validate(required)]
    pub day: Option<NaiveDate>,
}

/// `POST /service/{order}/reschedule` (`workshop.service.reschedule`):
/// another day with room, until 24 hours before.
pub async fn reschedule(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<RescheduleForm>,
) -> Result<(Toast, Redirect)> {
    let db = &state.db;
    let mut order = own_order(db, &user, id).await?;
    let lang = state.current_lang();
    if !order.changeable() {
        return Err(abort(
            StatusCode::CONFLICT,
            lang.t("workshop.service.too_late", &[]),
        ));
    }
    let day = form.day.unwrap_or_default();
    if let Some(problem) = capacity::move_booking(db, &state.config, &mut order, day).await? {
        let mut errors = Errors::new();
        errors.add("day", lang.t(problem.key(), &[]));
        return Err(errors.into());
    }
    Ok((
        Toast::success(lang.t("workshop.service.rescheduled", &[("day", &day)])),
        Redirect::route("workshop.service.show", &[&order.id])?,
    ))
}

/// `POST /service/{order}/pay` (`workshop.service.pay`): pays a ready
/// work order online (the shared payments contract); the gateway's webhook
/// emits `PaymentSucceeded`, which marks it paid.
pub async fn pay(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<Redirect> {
    let order = own_order(&state.db, &user, id).await?;
    if order.status != WorkStatus::Ready || order.paid_at.is_some() {
        return Redirect::route("workshop.service.show", &[&order.id]);
    }
    let customer = customer_of(&state.db, &user).await?;
    let checkout = payments::start(
        &state,
        Charge {
            payable: Payable::WorkOrder(order.id),
            customer_id: Some(customer.id),
            store_id: order.store_id,
            amount: order.total,
        },
    )
    .await?;
    Ok(Redirect::to(&checkout.redirect_url))
}

/// `PaymentSucceeded` for a work order (online or at the counter): paid.
pub async fn paid(state: &AppState, order_id: i64) -> Result {
    if let Some(mut order) = WorkOrder::find(&state.db, order_id).await?
        && order.paid_at.is_none()
    {
        order.paid_at = Some(renox::db::now());
        order.save_only(&state.db, &["paid_at"]).await?;
    }
    Ok(())
}
