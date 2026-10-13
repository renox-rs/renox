//! The workshop board: the active store's work orders in columns by
//! status, moved by dragging (or the keyboard) with the `kanban` block,
//! each move checked on the server; filters by mechanic and day; and the
//! walk-in form for staff.

use renox::chrono::NaiveDate;
use renox::db::relations::belongs_to;
use renox::prelude::*;
use renox::select::{OptionQuery, SelectOption};
use renox::validation::FormContext;
use serde::Deserialize;
use std::collections::HashMap;

use super::capacity::{self, NewBooking, chosen_tasks};
use super::model::{CustomerBike, ServiceTask, WorkOrder, WorkSource, WorkStatus};
use super::status::{self, BOARD, allowed, from_key, key};
use crate::app::access::{self, StoreAttr, catalogue};
use crate::app::accounts::model::Customer;
use crate::app::rentals::booking::to_local;
use crate::app::rentals::model::RentalBike;
use crate::app::rentals::notify::staff_with_permission;
use crate::app::rentals::{active_store, reserve::variant_names};
use crate::app::staff::model::Staff;

/// The mechanics of a store: everyone who may work on its work orders
/// now, as (staff id, name). Two queries.
pub async fn mechanics(db: &Db, store_id: i64) -> Result<Vec<(i64, String)>> {
    let users = staff_with_permission(db, catalogue::WORKORDERS_UPDATE, store_id).await?;
    let names: HashMap<i64, String> = users.iter().map(|u| (u.id, u.name.clone())).collect();
    let staff = Staff::query()
        .where_(Staff::USER_ID.is_in(users.iter().map(|u| u.id).collect::<Vec<_>>()))
        .get(db)
        .await?;
    let mut list: Vec<(i64, String)> = staff
        .into_iter()
        .map(|s| (s.id, names.get(&s.user_id).cloned().unwrap_or_default()))
        .collect();
    list.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(list)
}

/// A work order as a card on the board.
#[derive(serde::Serialize, Debug, Clone)]
pub struct Card {
    pub id: i64,
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub badge_kind: &'static str,
    pub url: String,
}

/// Cards for `orders`: the bikes (customers' and the fleet's), their
/// owners or models, the mechanics. A fixed number of queries.
pub async fn cards(state: &AppState, orders: &[WorkOrder]) -> Result<Vec<(WorkStatus, Card)>> {
    let db = &state.db;
    let lang = state.current_lang();
    let bikes = belongs_to::<CustomerBike, _, _>(db, orders, |o| o.customer_bike_id).await?;
    let bike_list: Vec<CustomerBike> = bikes.values().cloned().collect();
    let customers = belongs_to::<Customer, _, _>(db, &bike_list, |b| b.customer_id).await?;
    let fleet = belongs_to::<RentalBike, _, _>(db, orders, |o| o.rental_bike_id).await?;
    let models = variant_names(db, fleet.values().map(|b| b.variant_id).collect()).await?;
    let staff = belongs_to::<Staff, _, _>(db, orders, |o| o.mechanic_id).await?;
    let staff_list: Vec<Staff> = staff.values().cloned().collect();
    let users = belongs_to::<User, _, _>(db, &staff_list, |s| s.user_id).await?;
    Ok(orders
        .iter()
        .map(|order| {
            let (title, who) = match (order.customer_bike_id, order.rental_bike_id) {
                (Some(id), _) => {
                    let bike = bikes.get(&id);
                    let customer = bike.and_then(|b| customers.get(&b.customer_id));
                    (
                        bike.map(|b| b.name.clone()).unwrap_or_default(),
                        customer.map(|c| c.name.clone()).unwrap_or_default(),
                    )
                }
                (None, Some(id)) => {
                    let bike = fleet.get(&id);
                    let (model, _) = bike
                        .and_then(|b| models.get(&b.variant_id))
                        .cloned()
                        .unwrap_or_default();
                    (
                        model,
                        bike.map(|b| b.frame_number.clone()).unwrap_or_default(),
                    )
                }
                _ => (String::new(), String::new()),
            };
            let mechanic = order
                .mechanic_id
                .and_then(|id| staff.get(&id))
                .and_then(|s| users.get(&s.user_id))
                .map(|u| u.name.clone())
                .unwrap_or_else(|| lang.t("workshop.board.unassigned", &[]));
            let when = to_local(&state.config, order.scheduled_for).format("%d %b");
            let source = match order.source {
                WorkSource::WalkIn => "walk_in",
                WorkSource::Booking => "booking",
                WorkSource::Plan => "plan",
                WorkSource::Fleet => "fleet",
            };
            (
                order.status,
                Card {
                    id: order.id,
                    title: format!("#{} · {title}", order.id),
                    subtitle: format!("{who} · {when} · {mechanic}"),
                    badge: lang.t(&format!("workshop.source.{source}"), &[]),
                    badge_kind: match order.source {
                        WorkSource::Fleet => "warning",
                        WorkSource::Plan => "info",
                        _ => "neutral",
                    },
                    url: format!("/staff/workshop/{}", order.id),
                },
            )
        })
        .collect())
}

/// The board's filters.
#[derive(Deserialize, Default)]
pub struct BoardQuery {
    /// A mechanic's staff id, `none` for unassigned, empty for everyone.
    #[serde(default)]
    pub by: String,
    /// One day (`YYYY-MM-DD`), empty for every open work order.
    #[serde(default)]
    pub day: String,
}

/// `GET /staff/workshop` (`workshop.board`): the active store's open work
/// orders by status. Customers' bookings, walk-ins, plan visits (#237) and
/// fleet repairs (#235) share the board, labelled by source.
// [explain:workshop.board.handler]
pub async fn index(State(state): State<AppState>, Query(query): Query<BoardQuery>) -> Result<View> {
    let store = active_store()?;
    let db = &state.db;
    let mut orders = WorkOrder::query()
        .where_(WorkOrder::STORE_ID.eq(store))
        .where_(WorkOrder::STATUS.is_in(BOARD));
    match query.by.as_str() {
        "" => {}
        "none" => orders = orders.where_(WorkOrder::MECHANIC_ID.is_null()),
        id => {
            orders = orders.where_(WorkOrder::MECHANIC_ID.eq(id.parse::<i64>().unwrap_or_default()))
        }
    }
    let day = NaiveDate::parse_from_str(&query.day, "%Y-%m-%d").ok();
    if let Some(day) = day {
        let (from, to) = capacity::day_bounds(&state.config, day);
        orders = orders
            .where_(WorkOrder::SCHEDULED_FOR.gte(from))
            .where_(WorkOrder::SCHEDULED_FOR.lt(to));
    }
    let orders = orders
        .order_by(WorkOrder::SCHEDULED_FOR)
        .limit(300)
        .get(db)
        .await?;
    let cards = cards(&state, &orders).await?;
    // [/explain:workshop.board.handler]
    let lang = state.current_lang();
    let columns: Vec<renox::serde_json::Value> = BOARD
        .iter()
        .map(|status| {
            let list: Vec<&Card> = cards
                .iter()
                .filter(|(s, _)| s == status)
                .map(|(_, c)| c)
                .collect();
            json!({
                "key": key(*status),
                "title": lang.t(&format!("workshop.status.{}", key(*status)), &[]),
                "cards": list,
            })
        })
        .collect();
    let mechanics = mechanics(db, store).await?;
    Ok(view(
        "workshop/board.html",
        context! {
            columns,
            mechanics,
            by => query.by,
            day => day.map(|d| d.to_string()).unwrap_or_default(),
            today => to_local(&state.config, renox::db::now()).date().to_string(),
            count => orders.len(),
        },
    ))
}

/// A card moved on the board (the `kanban` block's htmx POST).
#[derive(Deserialize, Validate)]
pub struct Move {
    pub card: i64,
    #[validate(required, one_of(&["booked", "checked_in", "in_progress", "waiting_parts", "waiting_approval", "ready"]))]
    pub column: String,
    #[serde(default)]
    pub position: i64,
}

/// `POST /staff/workshop/move` (`workshop.move`): moves a work order to
/// another column, if that step is allowed and the person may work on it
/// in its store (`workorders.update`, checked in the work order's store);
/// any other answer puts the card back.
// [explain:workshop.board.move]
pub async fn move_card(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<Move>,
) -> Result<StatusCode> {
    let mut order = access::find::<WorkOrder>(&state.db, &user, form.card).await?;
    access::require(
        &user,
        catalogue::WORKORDERS_UPDATE,
        StoreAttr::Operating,
        &order,
    )?;
    let to = from_key(&form.column).ok_or(Error::NotFound)?;
    if to == order.status {
        return Ok(StatusCode::NO_CONTENT);
    }
    if !allowed(order.status, to) {
        return Err(abort(
            StatusCode::UNPROCESSABLE_ENTITY,
            state.current_lang().t("workshop.errors.step", &[]),
        ));
    }
    status::set_status(&state, &mut order, to).await?;
    Ok(StatusCode::NO_CONTENT)
}
// [/explain:workshop.board.move]

/// `GET /staff/workshop/customers` (`workshop.customers`): customers for
/// the walk-in form's searchable select.
pub async fn customer_options(
    State(db): State<Db>,
    query: OptionQuery,
) -> Result<Json<Vec<SelectOption>>> {
    let customers = if query.is_lookup() {
        Customer::query()
            .where_(Customer::ID.is_in(query.values_as::<i64>()))
            .get(&db)
            .await?
    } else {
        let q = query.q.trim().to_owned();
        Customer::query()
            .where_any(|any| {
                any.where_(Customer::NAME.like(&format!("%{q}%")))
                    .where_(Customer::EMAIL.like(&format!("%{q}%")))
                    .where_(Customer::PHONE.like(&format!("%{q}%")))
            })
            .order_by(Customer::NAME)
            .limit(20)
            .get(&db)
            .await?
    };
    Ok(Json(
        customers
            .iter()
            .map(|c| {
                SelectOption::new(
                    c.id,
                    format!(
                        "{} · {}",
                        c.name,
                        c.email.clone().or(c.phone.clone()).unwrap_or_default()
                    ),
                )
            })
            .collect(),
    ))
}

/// `GET /staff/workshop/new` (`workshop.walkin`): a work order for someone
/// at the counter: a known customer or a new one, the bike, the tasks, the
/// day (today by default).
pub async fn walk_in(State(state): State<AppState>) -> Result<View> {
    let store = active_store()?;
    let tasks: Vec<(i64, String)> = ServiceTask::query()
        .order_by(ServiceTask::NAME)
        .get(&state.db)
        .await?
        .into_iter()
        .map(|t| (t.id, t.name))
        .collect();
    let store = crate::app::staff::model::Store::find_or_404(&state.db, store).await?;
    let full = capacity::full_days(&state.db, &state.config, &store, 30).await?;
    Ok(view(
        "workshop/walk_in.html",
        context! {
            tasks,
            full,
            closed => capacity::closed_weekdays(&store),
            today => to_local(&state.config, renox::db::now()).date().to_string(),
        },
    ))
}

/// The walk-in form.
#[derive(Deserialize, Debug)]
pub struct WalkInForm {
    pub customer: Option<i64>,
    pub new_name: Option<String>,
    pub new_phone: Option<String>,
    pub bike: Option<String>,
    #[serde(default)]
    pub tasks: Vec<i64>,
    pub day: Option<NaiveDate>,
    pub note: Option<String>,
}

// [explain:workshop.walkin.form]
impl Validate for WalkInForm {
    fn rules(&self, v: &mut Validator) {
        v.field("new_name", &self.new_name)
            .required_if(self.customer.is_none())
            .max(120);
        v.field("new_phone", &self.new_phone).max(40);
        v.field("bike", &self.bike).required().max(120);
        v.field("tasks", &self.tasks).required();
        v.field("day", &self.day).required();
        v.field("note", &self.note).max(500);
    }
    // [/explain:workshop.walkin.form]

    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let db = &form.state.db;
        let lang = form.state.current_lang();
        let store_id = crate::app::access::active_store::current().unwrap_or_default();
        let Some(store) = crate::app::staff::model::Store::find(db, store_id).await? else {
            return Ok(());
        };
        let all = ServiceTask::query().get(db).await?;
        let tasks = chosen_tasks(&all, None, &self.tasks);
        if let Some(day) = self.day
            && let Some(problem) = capacity::check_day(
                db,
                &form.state.config,
                &store,
                day,
                capacity::estimate(&tasks).minutes,
            )
            .await?
        {
            errors.add("day", lang.t(problem.key(), &[]));
        }
        Ok(())
    }
}

/// `POST /staff/workshop/new` (`workshop.walkin.store`): the customer
/// (found, or made without an account), the bike registered to them, and
/// the work order booked in the same capacity transaction as online
/// bookings; checked in at once when it is for today.
// [explain:workshop.walkin.store]
pub async fn walk_in_store(
    State(state): State<AppState>,
    Valid(form): Valid<WalkInForm>,
) -> Result<Response> {
    let store = active_store()?;
    let db = &state.db;
    let customer = match form.customer {
        Some(id) => Customer::find_or_404(db, id).await?,
        None => {
            Customer::create(
                db,
                Customer {
                    name: form.new_name.clone().unwrap_or_default(),
                    phone: form.new_phone.clone().filter(|p| !p.is_empty()),
                    active: true,
                    ..Default::default()
                },
            )
            .await?
        }
    };
    // [/explain:workshop.walkin.store]
    let bike_name = form.bike.clone().unwrap_or_default();
    let bike = match CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .where_(CustomerBike::NAME.eq(bike_name.trim()))
        .first(db)
        .await?
    {
        Some(bike) => bike,
        None => {
            CustomerBike::create(
                db,
                CustomerBike {
                    customer_id: customer.id,
                    name: bike_name.trim().to_owned(),
                    ..Default::default()
                },
            )
            .await?
        }
    };
    let all = ServiceTask::query().get(db).await?;
    let day = form.day.unwrap_or_default();
    let today = to_local(&state.config, renox::db::now()).date();
    let booked = capacity::book(
        db,
        &state.config,
        NewBooking {
            bike_id: bike.id,
            store_id: store,
            day,
            tasks: chosen_tasks(&all, None, &form.tasks),
            package: None,
            note: form.note.clone().filter(|n| !n.trim().is_empty()),
            source: WorkSource::WalkIn,
            checked_in: day == today,
        },
    )
    .await?;
    match booked {
        Ok(order) => Ok(Redirect::route("workshop.order", &[&order.id])?.into_response()),
        Err(problem) => {
            let mut errors = Errors::new();
            errors.add("day", state.current_lang().t(problem.key(), &[]));
            Err(errors.into())
        }
    }
}
