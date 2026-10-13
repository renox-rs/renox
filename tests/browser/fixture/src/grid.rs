//! The data grid's pages for tests/browser/grid*.test.mjs (they were
//! the grid example's until #351): an orders grid with every option `renox::grid`
//! has, a second grid with merged cells and row details, and two prefixed
//! grids on one page, at `/grid`, `/grid/regions` and `/grid/follow-up`.
//! `db:seed` makes 480 orders and demo@example.com / password.

use renox::chrono::{Duration, NaiveDate};
use renox::db::Json as DbJson;
use renox::fake::Fake;
use renox::fake::faker::internet::en::SafeEmail;
use renox::fake::faker::name::en::Name;
use renox::grid::{Action, Column, Grid, GridRequest, RowOrder, Selection, Summary};
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// An order on the sales dashboard. `tags` and `trend` are JSON arrays
/// (a `tags` column and a chart in the grid); `created_by`/`updated_by` are
/// names, for the audit details.
#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "orders")]
pub struct Order {
    pub id: i64,
    pub number: String,
    pub customer: String,
    pub email: String,
    pub region: String,
    pub city: String,
    pub status: String,
    pub tags: DbJson<Vec<String>>,
    pub items: i64,
    /// In cents (`APP_CURRENCY`'s smallest unit).
    pub total: i64,
    /// Percent.
    pub discount: f64,
    pub ordered_on: NaiveDate,
    pub paid: bool,
    /// Units sold on each of the last seven days.
    pub trend: DbJson<Vec<i64>>,
    /// Percent of the items shipped.
    pub fulfilled: i64,
    /// Where the row sits when sorted by hand (`Grid::reorder`).
    pub position: i64,
    /// The `customers` row (its tier shows through `Column::related`).
    pub customer_id: Option<i64>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// Regions and their cities.
pub const PLACES: &[(&str, &[&str])] = &[
    ("java", &["Jakarta", "Bandung", "Surabaya", "Semarang"]),
    ("sumatra", &["Medan", "Palembang", "Padang"]),
    ("bali_nusa", &["Denpasar", "Mataram"]),
    ("sulawesi", &["Makassar", "Manado"]),
];

pub const REGIONS: [(&str, &str); 4] = [
    ("java", "Java"),
    ("sumatra", "Sumatra"),
    ("bali_nusa", "Bali & Nusa Tenggara"),
    ("sulawesi", "Sulawesi"),
];

pub const STATUSES: [(&str, &str); 4] = [
    ("new", "New"),
    ("paid", "Paid"),
    ("shipped", "Shipped"),
    ("cancelled", "Cancelled"),
];

pub const TAGS: [(&str, &str); 4] = [
    ("online", "Online"),
    ("store", "In store"),
    ("promo", "Promo"),
    ("wholesale", "Wholesale"),
];

const STAFF: [&str; 4] = ["Alex", "Diana", "Ben", "Sarah"];

fn pick<T: Copy>(items: &[T]) -> T {
    items[(0..items.len()).fake::<usize>()]
}

impl Factory for Order {
    fn definition() -> Self {
        let (region, cities) = pick(PLACES);
        let status = pick(&STATUSES).0;
        let tags: Vec<String> = TAGS
            .iter()
            .filter(|_| (0..3).fake::<u8>() == 0)
            .map(|(v, _)| (*v).to_owned())
            .collect();
        let items: i64 = (1..40).fake();
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or_default();
        Order {
            number: format!("SO-{:05}", (1..99_999).fake::<u32>()),
            customer: Name().fake(),
            email: SafeEmail().fake(),
            region: region.to_owned(),
            city: pick(cities).to_owned(),
            status: status.to_owned(),
            tags: DbJson(tags),
            items,
            // In cents: $4.99 to $149.99 an item.
            total: items * ((5..150).fake::<i64>() * 100 - 1),
            discount: f64::from((0..250).fake::<u32>()) / 10.0,
            ordered_on: start + Duration::days((0..270).fake::<i64>()),
            paid: status != "new" && status != "cancelled",
            trend: DbJson((0..7).map(|_| (0..30).fake::<i64>()).collect()),
            fulfilled: match status {
                "shipped" => 100,
                "paid" => (20..100).fake(),
                _ => 0,
            },
            created_by: pick(&STAFF).to_owned(),
            updated_by: pick(&STAFF).to_owned(),
            ..Default::default()
        }
    }
}

pub struct Orders;

impl Module for Orders {
    fn name(&self) -> &'static str {
        "orders"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/grid", index)
            .name("orders.index")
            .get("/grid/regions", regions)
            .name("orders.regions")
            .get("/grid/follow-up", follow_up)
            .name("orders.follow_up")
            .get("/grid/orders/{id}", show)
            .name("orders.show")
            .merge(writes())
    }
}

/// Changing orders needs a login (the grids show their edit tools only
/// then, see `orders_grid`).
fn writes() -> Routes {
    Routes::new()
        .patch("/grid/orders/{id}", update)
        .name("orders.update")
        .post("/grid/orders/reorder", reorder)
        .name("orders.reorder")
        .delete("/grid/orders/{id}", destroy)
        .name("orders.destroy")
        .post("/grid/orders/bulk/status/{status}", bulk_status)
        .name("orders.bulk_status")
        .post("/grid/orders/bulk/note", bulk_note)
        .name("orders.bulk_note")
        .post("/grid/orders/bulk/delete", bulk_delete)
        .name("orders.bulk_delete")
        .require_auth()
}

/// The orders grid: which columns, how they group and filter, which show
/// on a phone (`mobile`), which stay put while scrolling (`frozen`), which
/// can be edited in place, and the details under a row (`audit`).
/// `can_edit` (a logged-in user) adds in-place editing, dragging rows into
/// order and the bulk and delete actions.
pub fn orders_grid(can_edit: bool) -> Grid {
    let grid = Grid::new("orders")
        .title("Orders")
        .column(Column::number("position", "#").hidden())
        // `searchable`: the toolbar's search box looks in these.
        .column(
            Column::text("number", "Order")
                .frozen()
                .mobile()
                .searchable()
                .copyable(),
        )
        // Drawn from a value `extend` adds (an SVG with the initials).
        .column(Column::image("avatar", "").round().under(["Customer"]))
        .column(
            Column::text("customer", "Name")
                .under(["Customer"])
                .mobile()
                .editable()
                .searchable()
                // The email, small under the name.
                .description("email"),
        )
        .column(
            Column::text("email", "Email")
                .under(["Customer"])
                .hidden()
                .searchable(),
        )
        .column(Column::select("region", "Region", REGIONS).under(["Location"]))
        .column(
            Column::text("city", "City")
                .under(["Location"])
                .searchable(),
        )
        .column(
            Column::select("status", "Status", STATUSES)
                .mobile()
                .editable()
                .badges(&[
                    ("new", "info"),
                    ("paid", "success"),
                    ("shipped", "neutral"),
                    ("cancelled", "danger"),
                ]),
        )
        .column(Column::tags("tags", "Tags", TAGS).editable())
        // From other tables: the customer's tier, and how many notes the
        // order has (sorted, filtered and searched like the others).
        .column(
            Column::related("tier", "Tier", "customers", "customer_id", "tier")
                .under(["Customer"])
                .hidden()
                .badges(&[
                    ("gold", "warning"),
                    ("silver", "info"),
                    ("bronze", "neutral"),
                ]),
        )
        .column(Column::count_of(
            "notes",
            "Notes",
            "order_notes",
            "order_id",
        ))
        .column(
            Column::number("items", "Items")
                .under(["Amounts"])
                .editable()
                .summary(Summary::Sum),
        )
        .column(
            Column::money("total", "Total ($)")
                .under(["Amounts"])
                .editable()
                // In the footer (every filtered row) and under each group.
                .summary(Summary::Sum)
                .summary(Summary::Average),
        )
        .column(
            Column::number("discount", "Discount %")
                .decimals(1)
                .under(["Amounts"])
                .editable()
                .summary(Summary::Range),
        )
        .column(Column::date("ordered_on", "Ordered").editable())
        .column(Column::bool("paid", "Paid").editable().icons())
        .column(Column::custom("trend", "Last 7 days").under(["Charts"]))
        .column(
            Column::custom("fulfilled", "Shipped")
                .under(["Charts"])
                .width("9rem"),
        )
        .column(Column::custom("actions", "Actions").frozen_right())
        .sort_by("-ordered_on")
        // A "Group" choice in the toolbar.
        .groups(&["region", "status", "paid"])
        // On phones each order is a card.
        .cards_on_mobile()
        // Rules on any column (and/or), the filters kept for the next visit,
        // and a fresh page every 30 seconds while nobody's busy with it.
        .advanced_filter()
        .remember()
        .poll(30)
        // The chevron opens the audit details; the link icon opens the order.
        .row_url("/grid/orders/{id}")
        .empty_state(
            "No orders yet",
            Some("Orders show up here as customers buy."),
        )
        .audit()
        // CSV, Excel and a print page of every filtered row (`export` below).
        .exports()
        // Each row's ⋯ menu.
        .row_action(Action::link("Open", "/grid/orders/{id}"));
    if !can_edit {
        return grid;
    }
    grid.edit_url("/grid/orders/{id}")
        // Sorted by # (ascending), rows can be dragged into order.
        .reorder("position", "/grid/orders/reorder")
        // Checkboxes, and these over the selected rows (or all matching).
        .bulk_action(Action::new("Mark paid", "/grid/orders/bulk/status/paid"))
        // Asks for input first: the grid opens the kit's sheet `orders-note`.
        .bulk_action(Action::new("Add note", "/grid/orders/bulk/note").sheet("orders-note"))
        .bulk_action(Action::new(
            "Mark shipped",
            "/grid/orders/bulk/status/shipped",
        ))
        .bulk_action(
            Action::new("Delete", "/grid/orders/bulk/delete")
                .confirm("Delete the selected orders? This can't be undone.")
                .danger(),
        )
        .row_action(
            Action::new("Delete", "/grid/orders/{id}")
                .method("DELETE")
                .confirm("Delete this order?")
                .danger(),
        )
}

/// The same orders by region and city: equal neighbours share one cell
/// (`merge`), cities nested in their region.
pub fn regions_grid() -> Grid {
    Grid::new("regions")
        .title("Orders by region")
        .column(
            Column::select("region", "Region", REGIONS)
                .merge()
                .frozen()
                .mobile(),
        )
        .column(Column::text("city", "City").merge().frozen().mobile())
        .column(Column::text("number", "Order").mobile().searchable())
        .column(Column::text("customer", "Customer").searchable())
        .column(Column::select("status", "Status", STATUSES))
        .column(Column::money("total", "Total ($)"))
        .column(Column::date("ordered_on", "Ordered"))
        .sort_by("region,city,-total")
        .row_url("/grid/orders/{id}")
        .audit()
        .details()
        .exports()
}

/// Unpaid orders, the first of two grids on `/follow-up`. `prefix` gives its
/// query string names their own start (`unpaid.page`, `unpaid.q.city`), so
/// it pages and filters apart from the grid next to it.
pub fn unpaid_grid() -> Grid {
    Grid::new("unpaid")
        .prefix("unpaid")
        .title("Unpaid")
        .per_page(10)
        // The number opens the order; the email shows on hover.
        .column(
            Column::text("number", "Order")
                .link("/grid/orders/{id}")
                .mobile(),
        )
        .column(
            Column::text("customer", "Customer")
                .tooltip("email")
                .limit(16)
                .mobile(),
        )
        .column(Column::text("city", "City"))
        .column(Column::money("total", "Total ($)").mobile())
        .column(Column::date("ordered_on", "Ordered"))
        .sort_by("ordered_on")
}

/// The largest orders, the second grid on `/follow-up` (prefix `largest`).
pub fn largest_grid() -> Grid {
    Grid::new("largest")
        .prefix("largest")
        .title("Largest orders")
        .per_page(10)
        .column(
            Column::text("number", "Order")
                .link("/grid/orders/{id}")
                .mobile(),
        )
        // Long names wrap instead of widening the column.
        .column(Column::text("customer", "Customer").wrap().width("8rem"))
        // Shown, but neither sorted nor filtered from its heading.
        .column(
            Column::text("email", "Email")
                .sortable(false)
                .filterable(false),
        )
        // A value of the order's `customers` row; `numeric` filters it with a
        // range instead of text.
        .column(Column::related("account", "Account #", "customers", "customer_id", "id").numeric())
        .column(
            Column::money("total", "Total ($)")
                .filterable(false)
                .mobile(),
        )
        .sort_by("-total")
}

/// Two grids on one page, each with its own page, sort and filters.
async fn follow_up(request: GridRequest) -> Result<View> {
    let unpaid = unpaid_grid()
        .page(Order::where_eq("paid", false), &request)
        .await?;
    let largest = largest_grid().page(Order::query(), &request).await?;
    Ok(view("grid/follow_up.html", context! { unpaid, largest }))
}

async fn index(request: GridRequest, user: Option<AuthUser>) -> Result<Response> {
    let grid = orders_grid(user.is_some());
    // `?export=csv|xlsx|print`: the file, not the page.
    if let Some(file) = grid.export(Order::query(), &request).await? {
        return Ok(file);
    }
    let page = grid
        .page(Order::query(), &request)
        .await?
        // Values the template's custom cells read: where the trend went.
        .extend(|order| {
            let trend = &order.trend.0;
            let up = trend.last() >= trend.first();
            json!({ "up": up, "avatar": avatar(&order.customer) })
        });
    Ok(view("grid/index.html", context! { orders => page }).into_response())
}

/// A small SVG with the customer's initials, as a data URL.
fn avatar(name: &str) -> String {
    let initials: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    let hue = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
        % 360;
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 36 36'><rect width='36' height='36' fill='hsl({hue},55%,55%)'/><text x='18' y='23' font-family='sans-serif' font-size='14' fill='white' text-anchor='middle'>{initials}</text></svg>"
    );
    let encoded = svg
        .replace('%', "%25")
        .replace('#', "%23")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace(' ', "%20");
    format!("data:image/svg+xml,{encoded}")
}

async fn regions(request: GridRequest) -> Result<Response> {
    if let Some(file) = regions_grid().export(Order::query(), &request).await? {
        return Ok(file);
    }
    let page = regions_grid().page(Order::query(), &request).await?;
    Ok(view("grid/regions.html", context! { orders => page }).into_response())
}

async fn show(State(state): State<AppState>, Path(id): Path<i64>) -> Result<View> {
    let order = Order::find_or_404(&state.db, id).await?;
    Ok(view("grid/show.html", context! { order }))
}

/// What the grid sends when cells are edited: only the changed fields.
#[derive(Deserialize, Validate)]
pub struct OrderEdit {
    #[validate(max = 100)]
    pub customer: Option<String>,
    #[validate(one_of(&["new", "paid", "shipped", "cancelled"]))]
    pub status: Option<String>,
    /// Unknown tags are dropped (`update`).
    pub tags: Option<Vec<String>>,
    #[validate(between(1, 999))]
    pub items: Option<i64>,
    #[validate(min = 0)]
    pub total: Option<i64>,
    #[validate(between(0, 100))]
    pub discount: Option<f64>,
    pub ordered_on: Option<NaiveDate>,
    pub paid: Option<bool>,
}

/// Whether an order in `status` is paid (`None`: it can be either).
fn paid_for(status: &str) -> Option<bool> {
    match status {
        "paid" | "shipped" => Some(true),
        "new" => Some(false),
        _ => None,
    }
}

async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(edit): Valid<OrderEdit>,
) -> Result<Toast> {
    let mut order = Order::find_or_404(&state.db, id).await?;
    if let Some(v) = edit.customer {
        order.customer = v;
    }
    if let Some(v) = edit.status {
        // Paid and shipped orders are paid; new ones aren't.
        if let Some(paid) = paid_for(&v) {
            order.paid = paid;
        }
        order.status = v;
    }
    if let Some(mut v) = edit.tags {
        v.retain(|tag| TAGS.iter().any(|(known, _)| known == tag));
        order.tags.0 = v;
    }
    if let Some(v) = edit.items {
        order.items = v;
    }
    if let Some(v) = edit.total {
        order.total = v;
    }
    if let Some(v) = edit.discount {
        order.discount = v;
    }
    if let Some(v) = edit.ordered_on {
        order.ordered_on = v;
    }
    if let Some(v) = edit.paid {
        order.paid = v;
    }
    order.updated_by = user.name.clone();
    order.save(&state.db).await?;
    Ok(Toast::success(format!("{} saved.", order.number)))
}

async fn destroy(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Toast> {
    let mut order = Order::find_or_404(&state.db, id).await?;
    order.delete(&state.db).await?;
    Ok(Toast::success(format!("{} deleted.", order.number)))
}

/// A bulk action: the selected orders, or all the filters match.
async fn bulk_status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(status): Path<String>,
    request: GridRequest,
    Form(selection): Form<Selection>,
) -> Result<Toast> {
    if !STATUSES.iter().any(|(s, _)| *s == status) {
        return Err(Error::NotFound);
    }
    let selected = orders_grid(true).selected(Order::query(), &request, &selection)?;
    let changed = match paid_for(&status) {
        Some(paid) => {
            selected
                .update(
                    &state.db,
                    &[
                        ("status", &status),
                        ("paid", &paid),
                        ("updated_by", &user.name),
                    ],
                )
                .await?
        }
        None => {
            selected
                .update(
                    &state.db,
                    &[("status", &status), ("updated_by", &user.name)],
                )
                .await?
        }
    };
    Ok(Toast::success(format!("{changed} orders marked {status}.")))
}

/// A bulk action with a sheet: the selection and the typed `note` come in
/// one form.
async fn bulk_note(
    State(state): State<AppState>,
    request: GridRequest,
    Form(posted): Form<Vec<(String, String)>>,
) -> Result<Toast> {
    let mut selection = Selection::default();
    let mut note = String::new();
    for (name, value) in posted {
        match name.as_str() {
            "ids" => selection.ids = value.split(',').map(str::to_owned).collect(),
            "all" => selection.all = value == "true",
            "note" => note = value.trim().to_owned(),
            _ => {}
        }
    }
    if note.is_empty() {
        let mut errors = renox::validation::Errors::new();
        errors.add("note", "The note field is required.");
        return Err(ValidationError::new(errors).into());
    }
    let changed = orders_grid(true)
        .selected(Order::query(), &request, &selection)?
        .update(&state.db, &[("updated_by", &note)])
        .await?;
    Ok(Toast::success(format!("{changed} orders noted: {note}")))
}

async fn bulk_delete(
    State(state): State<AppState>,
    request: GridRequest,
    Form(selection): Form<Selection>,
) -> Result<Toast> {
    let deleted = orders_grid(true)
        .selected(Order::query(), &request, &selection)?
        .delete(&state.db)
        .await?;
    Ok(Toast::success(format!("{deleted} orders deleted.")))
}

async fn reorder(State(state): State<AppState>, Form(order): Form<RowOrder>) -> Result<StatusCode> {
    order.save::<Order>(&state.db, "position").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `db:seed`: 480 orders, customers in tiers, notes, and the demo user.
/// Seeding twice is harmless: a seeded database stays as it is.
pub async fn seed(state: AppState) -> Result {
    let db = state.db.clone();
    if User::find_by_email(&db, "demo@example.com")
        .await?
        .is_some()
    {
        return Ok(());
    }
    User::register(&db, "Demo", "demo@example.com", "password").await?;
    Order::factory().count(480).create(&db).await?;
    // Hand-sorted order starts as the order they were made in.
    renox::db::sql("UPDATE orders SET position = id")
        .execute(&db)
        .await?;
    for i in 0..60 {
        let tier = ["gold", "silver", "bronze", "bronze"][i % 4];
        renox::db::sql("INSERT INTO customers (name, tier) VALUES (?, ?)")
            .bind(format!("Customer {i}"))
            .bind(tier)
            .execute(&db)
            .await?;
    }
    renox::db::sql("UPDATE orders SET customer_id = (id % 60) + 1")
        .execute(&db)
        .await?;
    for order in 1..=480_i64 {
        for n in 0..(order * 7 % 4) {
            renox::db::sql("INSERT INTO order_notes (order_id, body) VALUES (?, ?)")
                .bind(order)
                .bind(format!("Note {n}"))
                .execute(&db)
                .await?;
        }
    }
    Ok(())
}
