//! Suppliers and purchase orders.
//!
//! | Route | Name | Needs |
//! |---|---|---|
//! | `GET /staff/suppliers` | `stock.suppliers` | `purchasing.manage` |
//! | `GET /staff/suppliers/new`, `POST /staff/suppliers` | `stock.suppliers.create`, `.store` | `purchasing.manage` |
//! | `GET /staff/suppliers/{supplier}` | `stock.suppliers.show` | `purchasing.manage` |
//! | `GET /staff/suppliers/{supplier}/edit`, `POST …` | `stock.suppliers.edit`, `.update` | `purchasing.manage` |
//! | `GET /staff/purchase-orders` | `stock.purchasing` | `purchasing.manage` |
//! | `GET /staff/purchase-orders/new`, `POST /staff/purchase-orders` | `stock.purchasing.create`, `.store` | `purchasing.manage` |
//! | `GET /staff/purchase-orders/{order}` | `stock.purchasing.show` | `stock.view` in the order's store |
//! | `POST …/send`, `…/cancel` | `stock.purchasing.send`, `.cancel` | `purchasing.manage` in the order's store |
//! | `POST …/receive` | `stock.purchasing.receive` | `stock.receive` in the order's store |
//! | `GET /purchase-orders/{order}/print` (signed) | `stock.purchasing.print` | a signed link (the supplier has no account) |
//!
//! A purchase order belongs to one store, which orders, receives and
//! **owns** what arrives. Its life: draft → ordered (mailed to the supplier
//! with a link to a printable page) → partial (some lines arrived) →
//! received; or cancelled before anything arrived. Receiving writes
//! `purchase` movements and updates each variant's **average cost**
//! ([`average_cost`]).

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use renox::chrono::Duration as Days;
use renox::prelude::*;
use renox::signed::ValidSignature;
use renox::validation::FormContext;
use serde::{Deserialize, Serialize};

use super::ledger::{store_names, variant_names};
use super::model::{
    MovementReason, PurchaseOrder, PurchaseOrderLine, PurchaseStatus, StockMovement, StockRow,
    Supplier, SupplierItem,
};
use crate::app::access::{self, StoreAttr, active_store, catalogue};
use crate::app::multistore::audit;
use crate::app::staff::model::{Staff, Store};
use crate::app::workshop::model::{PartStatus, WorkOrder, WorkOrderPart, WorkStatus};

// --- Suppliers ---

// [explain:stock.suppliers.page]
/// What `stock/suppliers/index.html` gets: a typed view context. The template is named
/// once, here, and `view:check` (registered in `Stock::register`) reports a variable the
/// template uses that this struct lacks.
#[derive(Serialize, ViewData)]
#[view(template = "stock/suppliers/index.html")]
pub struct SupplierList {
    /// A page of suppliers, each with `supplier`, `items` (its price list's size) and
    /// `open` (the active store's open orders with it).
    pub suppliers: Paginated<renox::serde_json::Value>,
}
// [/explain:stock.suppliers.page]

/// `GET /staff/suppliers` (`stock.suppliers`): every supplier, with how
/// many items their price list has and the active store's open orders.
// [explain:stock.suppliers.handler]
pub async fn suppliers(State(db): State<Db>, Page(page): Page) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let suppliers = Supplier::query()
        .order_by(Supplier::NAME)
        .paginate(&db, page, 25)
        .await?;
    let ids: Vec<i64> = suppliers.items.iter().map(|s| s.id).collect();
    let items: Vec<(i64, i64)> = SupplierItem::query()
        .where_(SupplierItem::SUPPLIER_ID.is_in(ids.clone()))
        .group_by("supplier_id")
        .select_as(&db, "supplier_id, COUNT(*)")
        .await?;
    // [/explain:stock.suppliers.handler]
    let open: Vec<(i64, i64)> = PurchaseOrder::query()
        .where_(PurchaseOrder::SUPPLIER_ID.is_in(ids))
        .where_(PurchaseOrder::STORE_ID.eq(store))
        .where_(PurchaseOrder::STATUS.is_in([
            PurchaseStatus::Draft,
            PurchaseStatus::Ordered,
            PurchaseStatus::Partial,
        ]))
        .group_by("supplier_id")
        .select_as(&db, "supplier_id, COUNT(*)")
        .await?;
    // [explain:stock.suppliers.handler]
    let items: HashMap<i64, i64> = items.into_iter().collect();
    let open: HashMap<i64, i64> = open.into_iter().collect();
    let suppliers = suppliers.map(|s| {
        json!({
            "items": items.get(&s.id).copied().unwrap_or(0),
            "open": open.get(&s.id).copied().unwrap_or(0),
            "supplier": s,
        })
    });
    Ok(View::typed(SupplierList { suppliers }))
}
// [/explain:stock.suppliers.handler]

// [explain:stock.suppliers.form]
/// The supplier form.
#[derive(Deserialize, Serialize, Validate, Debug, Default)]
pub struct SupplierForm {
    #[validate(required, max = 120)]
    pub name: String,
    #[validate(email, max = 160)]
    pub email: Option<String>,
    #[validate(max = 40)]
    pub phone: Option<String>,
    #[validate(required, min = 0, max = 180)]
    pub lead_days: Option<i64>,
}

impl SupplierForm {
    fn apply(self, supplier: &mut Supplier) {
        supplier.name = self.name.trim().to_owned();
        supplier.email = self.email.filter(|e| !e.trim().is_empty());
        supplier.phone = self.phone.filter(|p| !p.trim().is_empty());
        supplier.lead_days = self.lead_days.unwrap_or(7);
    }
}
// [/explain:stock.suppliers.form]

// [explain:stock.suppliers.create.handler]
/// `GET /staff/suppliers/new` (`stock.suppliers.create`).
pub async fn supplier_create() -> View {
    view(
        "stock/suppliers/form.html",
        context! { supplier => Supplier { lead_days: 7, ..Default::default() } },
    )
}

/// `POST /staff/suppliers` (`stock.suppliers.store`).
pub async fn supplier_store(
    State(state): State<AppState>,
    Valid(form): Valid<SupplierForm>,
) -> Result<(Toast, Redirect)> {
    let mut supplier = Supplier::default();
    form.apply(&mut supplier);
    let supplier = Supplier::create(&state.db, supplier).await?;
    Ok((
        Toast::success(state.current_lang().t("stock.suppliers.saved", &[])),
        Redirect::route("stock.suppliers.show", &[&supplier.id])?,
    ))
}
// [/explain:stock.suppliers.create.handler]

// [explain:stock.suppliers.edit.handler]
/// `GET /staff/suppliers/{supplier}/edit` (`stock.suppliers.edit`).
pub async fn supplier_edit(Found(supplier): Found<Supplier>) -> View {
    view("stock/suppliers/form.html", context! { supplier })
}

/// `POST /staff/suppliers/{supplier}` (`stock.suppliers.update`).
pub async fn supplier_update(
    State(state): State<AppState>,
    Found(mut supplier): Found<Supplier>,
    Valid(form): Valid<SupplierForm>,
) -> Result<(Toast, Redirect)> {
    form.apply(&mut supplier);
    supplier.save(&state.db).await?;
    Ok((
        Toast::success(state.current_lang().t("stock.suppliers.saved", &[])),
        Redirect::route("stock.suppliers.show", &[&supplier.id])?,
    ))
}
// [/explain:stock.suppliers.edit.handler]

/// `GET /staff/suppliers/{supplier}` (`stock.suppliers.show`): the
/// supplier, their price list (from the CSV import) and the active store's
/// orders with them.
pub async fn supplier_show(
    State(db): State<Db>,
    Page(page): Page,
    Found(supplier): Found<Supplier>,
) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let items = SupplierItem::query()
        .where_(SupplierItem::SUPPLIER_ID.eq(supplier.id))
        .order_by(SupplierItem::VARIANT_ID)
        .paginate(&db, page, 50)
        .await?;
    let names = variant_names(&db, items.items.iter().map(|i| i.variant_id).collect()).await?;
    let items = items.map(|item| {
        let name = names.get(&item.variant_id).cloned().unwrap_or_default();
        json!({ "name": name.label(), "sku": name.sku, "price": name.price, "item": item })
    });
    let orders = PurchaseOrder::query()
        .where_(PurchaseOrder::SUPPLIER_ID.eq(supplier.id))
        .where_(PurchaseOrder::STORE_ID.eq(store))
        .order_by_desc(PurchaseOrder::ID)
        .limit(10)
        .get(&db)
        .await?;
    Ok(view(
        "stock/suppliers/show.html",
        context! { supplier, items, orders, columns => super::import::COLUMNS },
    ))
}

// --- Purchase orders ---

/// The list's tabs.
pub const TABS: [&str; 3] = ["open", "received", "all"];

/// `?status=`.
#[derive(Deserialize, Default)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<String>,
}

/// `GET /staff/purchase-orders` (`stock.purchasing`): the active store's
/// orders. Three queries a page.
// [explain:stock.purchasing.handler]
pub async fn index(
    State(db): State<Db>,
    Page(page): Page,
    Query(query): Query<ListQuery>,
) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let tab = query
        .status
        .filter(|s| TABS.contains(&s.as_str()))
        .unwrap_or_else(|| "open".to_owned());
    let orders = access::visible::<PurchaseOrder>(catalogue::STOCK_VIEW)
        .where_(PurchaseOrder::STORE_ID.eq(store));
    // [/explain:stock.purchasing.handler]
    let orders = match tab.as_str() {
        "open" => orders.where_(PurchaseOrder::STATUS.is_in([
            PurchaseStatus::Draft,
            PurchaseStatus::Ordered,
            PurchaseStatus::Partial,
        ])),
        "received" => orders.where_(PurchaseOrder::STATUS.eq(PurchaseStatus::Received)),
        _ => orders,
    };
    // [explain:stock.purchasing.handler]
    let orders = orders
        .order_by_desc(PurchaseOrder::ID)
        .paginate(&db, page, 25)
        .await?;
    let suppliers =
        renox::db::relations::belongs_to::<Supplier, _, _>(&db, &orders.items, |o| o.supplier_id)
            .await?;
    // [/explain:stock.purchasing.handler]
    let orders = orders.map(|o| {
        json!({
            "supplier": suppliers.get(&o.supplier_id).map(|s| s.name.clone()),
            "order": o,
        })
    });
    Ok(view(
        "stock/purchase_orders/index.html",
        context! { orders, tab, tabs => TABS },
    ))
}

/// A variant the store needs, and why.
#[derive(Serialize, Debug, Clone, Default)]
pub struct Need {
    pub variant_id: i64,
    pub name: String,
    pub sku: String,
    /// What to order.
    pub quantity: i64,
    pub unit_cost: i64,
    /// Available at the store now (own and consigned).
    pub available: i64,
    pub reorder_level: i64,
    /// Work orders waiting for it ("waiting for parts").
    pub work_orders: Vec<i64>,
    /// Whether the supplier sells it (their price list).
    pub sold_by_supplier: bool,
}

// [explain:stock.purchasing.needs]
/// What `store` needs: parts that work orders at the store wait for
/// (#236's "waiting for parts"), and its own goods under their reorder
/// level, topped up to twice the level. One map by variant.
pub async fn needs(db: &Db, store: i64) -> Result<BTreeMap<i64, Need>> {
    let mut needs: BTreeMap<i64, Need> = BTreeMap::new();
    let waiting: Vec<i64> = WorkOrder::query()
        .where_(WorkOrder::STORE_ID.eq(store))
        .where_(WorkOrder::STATUS.eq(WorkStatus::WaitingParts))
        .get(db)
        .await?
        .into_iter()
        .map(|o| o.id)
        .collect();
    for part in WorkOrderPart::query()
        .where_(WorkOrderPart::WORK_ORDER_ID.is_in(waiting))
        .where_(WorkOrderPart::STATUS.eq(PartStatus::Waiting))
        .get(db)
        .await?
    {
        let need = needs.entry(part.variant_id).or_default();
        need.variant_id = part.variant_id;
        need.quantity += part.quantity;
        if !need.work_orders.contains(&part.work_order_id) {
            need.work_orders.push(part.work_order_id);
        }
    }
    // [/explain:stock.purchasing.needs]
    let here: Vec<StockRow> = StockRow::query()
        .where_(StockRow::LOCATION_STORE_ID.eq(store))
        .where_(StockRow::REORDER_LEVEL.gt(0))
        .get(db)
        .await?;
    let mut available: HashMap<i64, (i64, i64)> = HashMap::new();
    for row in &here {
        let entry = available
            .entry(row.variant_id)
            .or_insert((0, row.reorder_level));
        entry.0 += row.available;
    }
    for (variant, (have, level)) in &available {
        if have < level {
            let need = needs.entry(*variant).or_default();
            need.variant_id = *variant;
            need.quantity = need.quantity.max(level * 2 - have);
        }
    }
    let names = variant_names(db, needs.keys().copied().collect()).await?;
    for need in needs.values_mut() {
        let name = names.get(&need.variant_id).cloned().unwrap_or_default();
        let (have, level) = available
            .get(&need.variant_id)
            .copied()
            .unwrap_or((0, name.reorder_level));
        need.available = have;
        need.reorder_level = level;
        need.unit_cost = name.cost;
        need.name = name.label();
        need.sku = name.sku;
    }
    Ok(needs)
}

/// `?supplier=`.
#[derive(Deserialize, Default)]
pub struct NewQuery {
    #[serde(default)]
    pub supplier: Option<i64>,
}

/// `GET /staff/purchase-orders/new` (`stock.purchasing.create`): pick a
/// supplier; the lines start with what the store **needs** (parts work
/// orders wait for, goods under their reorder level), then the rest of the
/// supplier's price list. Quantities left blank aren't ordered.
// [explain:stock.purchasing.create.handler]
pub async fn create(State(db): State<Db>, Query(query): Query<NewQuery>) -> Result<View> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let suppliers = Supplier::query().order_by(Supplier::NAME).get(&db).await?;
    let supplier = query
        .supplier
        .and_then(|id| suppliers.iter().find(|s| s.id == id).cloned());
    let supplier_options: Vec<(i64, String)> =
        suppliers.iter().map(|s| (s.id, s.name.clone())).collect();
    let mut needs = needs(&db, store).await?;
    let mut rest = Vec::new();
    if let Some(supplier) = &supplier {
        let items = SupplierItem::query()
            .where_(SupplierItem::SUPPLIER_ID.eq(supplier.id))
            .get(&db)
            .await?;
        // [/explain:stock.purchasing.create.handler]
        let names = variant_names(&db, items.iter().map(|i| i.variant_id).collect()).await?;
        for item in items {
            if let Some(need) = needs.get_mut(&item.variant_id) {
                need.unit_cost = item.cost;
                need.sold_by_supplier = true;
                continue;
            }
            let name = names.get(&item.variant_id).cloned().unwrap_or_default();
            rest.push(Need {
                variant_id: item.variant_id,
                name: name.label(),
                sku: name.sku,
                unit_cost: item.cost,
                reorder_level: name.reorder_level,
                sold_by_supplier: true,
                ..Default::default()
            });
        }
        rest.sort_by(|a, b| a.name.cmp(&b.name));
    }
    // [explain:stock.purchasing.create.handler]
    let needs: Vec<Need> = needs.into_values().collect();
    Ok(view(
        "stock/purchase_orders/new.html",
        context! { suppliers, supplier_options, supplier, needs, rest },
    ))
}
// [/explain:stock.purchasing.create.handler]

/// A line of a new order.
#[derive(Deserialize, Debug, Clone)]
pub struct OrderLine {
    pub variant: i64,
    pub quantity: Option<i64>,
    /// In whole units as typed (`12.50`); stored in the smallest unit.
    pub unit_cost: Option<f64>,
}

impl Validate for OrderLine {
    fn rules(&self, v: &mut Validator) {
        v.field("quantity", &self.quantity).min(0).max(100_000);
        v.field("unit_cost", &self.unit_cost).min(0);
    }
}

/// The new-order form.
#[derive(Deserialize, Debug)]
pub struct OrderForm {
    pub supplier: Option<i64>,
    pub note: Option<String>,
    #[serde(default)]
    pub lines: Vec<OrderLine>,
}

impl Validate for OrderForm {
    fn rules(&self, v: &mut Validator) {
        v.field("supplier", &self.supplier)
            .required()
            .exists("suppliers", "id");
        v.field("note", &self.note).max(500);
        v.nested("lines", &self.lines);
    }

    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        if !self.lines.iter().any(|l| l.quantity.unwrap_or(0) > 0) {
            errors.add(
                "lines",
                form.state
                    .current_lang()
                    .t("stock.purchasing.errors.empty", &[]),
            );
        }
        Ok(())
    }
}

/// `POST /staff/purchase-orders` (`stock.purchasing.store`): a draft for
/// the active store.
pub async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    Valid(form): Valid<OrderForm>,
) -> Result<(Toast, Redirect)> {
    let store = active_store::current().ok_or(Error::Forbidden)?;
    let staff = Staff::of_user(&state.db, user.id).await?.map(|s| s.id);
    let lines: Vec<(i64, i64, i64)> = form
        .lines
        .iter()
        .filter(|l| l.quantity.unwrap_or(0) > 0)
        .map(|l| {
            (
                l.variant,
                l.quantity.unwrap_or(0),
                crate::money::from_form(l.unit_cost),
            )
        })
        .collect();
    let order = draft(
        &state.db,
        store,
        form.supplier.unwrap_or_default(),
        staff,
        form.note.clone().filter(|n| !n.trim().is_empty()),
        &lines,
        false,
    )
    .await?;
    audit::record(
        &state,
        &user,
        "purchase_order.created",
        store,
        (PurchaseOrder::TABLE, order.id),
        json!({ "supplier": order.supplier_id, "total": order.total }),
    )
    .await?;
    Ok((
        Toast::success(state.current_lang().t("stock.purchasing.created", &[])),
        Redirect::route("stock.purchasing.show", &[&order.id])?,
    ))
}

/// Writes a draft order with `lines` (`(variant, quantity, unit cost)`)
/// in one transaction.
pub async fn draft(
    db: &Db,
    store: i64,
    supplier: i64,
    staff: Option<i64>,
    note: Option<String>,
    lines: &[(i64, i64, i64)],
    suggested: bool,
) -> Result<PurchaseOrder> {
    let mut tx = db.begin().await?;
    let order = PurchaseOrder::create(
        &mut tx,
        PurchaseOrder {
            supplier_id: supplier,
            store_id: store,
            status: PurchaseStatus::Draft,
            total: lines.iter().map(|(_, q, c)| q * c).sum(),
            created_by: staff,
            note,
            suggested,
            ..Default::default()
        },
    )
    .await?;
    for (variant, quantity, cost) in lines {
        PurchaseOrderLine::create(
            &mut tx,
            PurchaseOrderLine {
                purchase_order_id: order.id,
                variant_id: *variant,
                quantity: *quantity,
                unit_cost: *cost,
                ..Default::default()
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(order)
}

/// A line as the order's pages show it.
#[derive(Serialize, Debug, Clone)]
pub struct LineView {
    #[serde(flatten)]
    pub line: PurchaseOrderLine,
    pub name: String,
    pub sku: String,
    pub open: i64,
    pub total: i64,
}

async fn lines_of(db: &Db, order: &PurchaseOrder) -> Result<Vec<LineView>> {
    let lines = PurchaseOrderLine::query()
        .where_(PurchaseOrderLine::PURCHASE_ORDER_ID.eq(order.id))
        .order_by(PurchaseOrderLine::ID)
        .get(db)
        .await?;
    let names = variant_names(db, lines.iter().map(|l| l.variant_id).collect()).await?;
    Ok(lines
        .into_iter()
        .map(|line| {
            let name = names.get(&line.variant_id).cloned().unwrap_or_default();
            LineView {
                name: name.label(),
                sku: name.sku,
                open: line.quantity - line.received_quantity,
                total: line.quantity * line.unit_cost,
                line,
            }
        })
        .collect())
}

// [explain:stock.purchasing.print.handler]
/// The printable page's signed link (30 days: what a supplier needs).
pub fn print_link(state: &AppState, order: &PurchaseOrder) -> Result<String> {
    state.signed_url(
        "stock.purchasing.print",
        &[&order.id],
        Duration::from_secs(30 * 24 * 60 * 60),
    )
}
// [/explain:stock.purchasing.print.handler]

/// `GET /staff/purchase-orders/{order}` (`stock.purchasing.show`).
// [explain:stock.purchasing.show.handler]
pub async fn show(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<View> {
    let db = &state.db;
    let order = access::find::<PurchaseOrder>(db, &user, id).await?;
    let supplier = Supplier::find_or_404(db, order.supplier_id).await?;
    let lines = lines_of(db, &order).await?;
    let manage = access::can(
        &user,
        catalogue::PURCHASING_MANAGE,
        StoreAttr::Owner,
        &order,
    );
    let receive = access::can(&user, catalogue::STOCK_RECEIVE, StoreAttr::Owner, &order);
    let s = order.status;
    let nothing_in = lines.iter().all(|l| l.line.received_quantity == 0);
    let actions = json!({
        "send": manage && s == PurchaseStatus::Draft,
        "cancel": manage && matches!(s, PurchaseStatus::Draft | PurchaseStatus::Ordered) && nothing_in,
        "receive": receive && matches!(s, PurchaseStatus::Ordered | PurchaseStatus::Partial),
    });
    // [/explain:stock.purchasing.show.handler]
    // Work orders at the store waiting for these parts.
    let needs = needs(db, order.store_id).await?;
    let waiting: Vec<Need> = lines
        .iter()
        .filter_map(|l| needs.get(&l.line.variant_id))
        .filter(|n| !n.work_orders.is_empty())
        .cloned()
        .collect();
    let steps: Vec<_> = ["draft", "ordered", "partial", "received"]
        .iter()
        .enumerate()
        .map(|(n, key)| {
            let reached = match s {
                PurchaseStatus::Draft | PurchaseStatus::Cancelled => 0,
                PurchaseStatus::Ordered => 1,
                PurchaseStatus::Partial => 2,
                PurchaseStatus::Received => 3,
            };
            json!({ "key": key, "done": n <= reached && s != PurchaseStatus::Cancelled, "current": n == reached })
        })
        .collect();
    let stores = store_names(db).await?;
    Ok(view(
        "stock/purchase_orders/show.html",
        context! {
            store => stores.get(&order.store_id).cloned().unwrap_or_default(),
            print_url => print_link(&state, &order)?,
            status => s.as_str(),
            supplier,
            lines,
            actions,
            waiting,
            steps,
            order,
        },
    ))
}

// [explain:stock.purchasing.print.handler]
/// `GET /purchase-orders/{order}/print` (`stock.purchasing.print`): the
/// order as the supplier prints it. A **signed** link (`ValidSignature`),
/// mailed to the supplier, who has no account: anyone with the link may
/// read this one order, for 30 days; a changed link is a 403.
pub async fn print(
    _signed: ValidSignature,
    State(db): State<Db>,
    Path(id): Path<i64>,
) -> Result<View> {
    let order = PurchaseOrder::find_or_404(&db, id).await?;
    let supplier = Supplier::find_or_404(&db, order.supplier_id).await?;
    let store = Store::find_or_404(&db, order.store_id).await?;
    let lines = lines_of(&db, &order).await?;
    Ok(view(
        "stock/purchase_orders/print.html",
        context! { order, supplier, store, lines },
    ))
}
// [/explain:stock.purchasing.print.handler]

async fn manageable(db: &Db, user: &User, id: i64) -> Result<PurchaseOrder> {
    let order = access::find::<PurchaseOrder>(db, user, id).await?;
    access::require(user, catalogue::PURCHASING_MANAGE, StoreAttr::Owner, &order)?;
    Ok(order)
}

/// `POST /staff/purchase-orders/{order}/send` (`stock.purchasing.send`):
/// the draft is ordered: expected after the supplier's lead time, mailed
/// to the supplier with the printable page's signed link.
pub async fn send(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let mut order = manageable(&state.db, &user, id).await?;
    let supplier = Supplier::find_or_404(&state.db, order.supplier_id).await?;
    let lang = state.current_lang();
    let moved = PurchaseOrder::query()
        .where_(PurchaseOrder::ID.eq(order.id))
        .where_(PurchaseOrder::STATUS.eq(PurchaseStatus::Draft))
        .update(&state.db, &[("status", &PurchaseStatus::Ordered)])
        .await?;
    abort_if(
        moved == 0,
        StatusCode::CONFLICT,
        lang.t("stock.purchasing.errors.moved_on", &[]),
    )?;
    order.status = PurchaseStatus::Ordered;
    order.ordered_at = Some(renox::db::now());
    order.expected_on = Some(crate::seed::today() + Days::days(supplier.lead_days));
    order
        .save_only(&state.db, &["ordered_at", "expected_on"])
        .await?;
    if let Some(email) = supplier.email.as_deref().filter(|e| !e.is_empty()) {
        let store = Store::find_or_404(&state.db, order.store_id).await?;
        let lines = lines_of(&state.db, &order).await?;
        let money =
            |amount: i64| crate::money::format(amount, &state.config.currency, &lang.locale);
        let mut table: Vec<[String; 2]> = lines
            .iter()
            .map(|l| {
                [
                    format!("{} × {} ({})", l.line.quantity, l.name, l.sku),
                    money(l.total),
                ]
            })
            .collect();
        table.push([lang.t("stock.purchasing.total", &[]), money(order.total)]);
        let mail = state.mail_view(
            email,
            lang.t(
                "stock.mail.purchase_order.subject",
                &[
                    ("number", &order.id as &dyn std::fmt::Display),
                    ("store", &store.name),
                ],
            ),
            "mail/stock/purchase_order",
            context! {
                order => &order,
                supplier => &supplier,
                store => &store,
                lines,
                table,
                url => print_link(&state, &order)?,
            },
        )?;
        state.queue_mail(mail).await?;
    }
    audit::record(
        &state,
        &user,
        "purchase_order.sent",
        order.store_id,
        (PurchaseOrder::TABLE, order.id),
        json!({ "supplier": supplier.id }),
    )
    .await?;
    Ok((
        Toast::success(lang.t("stock.purchasing.sent", &[])),
        Redirect::route("stock.purchasing.show", &[&order.id])?,
    ))
}

/// `POST /staff/purchase-orders/{order}/cancel` (`stock.purchasing.cancel`).
pub async fn cancel(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> Result<(Toast, Redirect)> {
    let order = manageable(&state.db, &user, id).await?;
    let received: i64 = PurchaseOrderLine::query()
        .where_(PurchaseOrderLine::PURCHASE_ORDER_ID.eq(order.id))
        .sum(&state.db, "received_quantity")
        .await?;
    let lang = state.current_lang();
    let moved = if received == 0 {
        PurchaseOrder::query()
            .where_(PurchaseOrder::ID.eq(order.id))
            .where_(PurchaseOrder::STATUS.is_in([PurchaseStatus::Draft, PurchaseStatus::Ordered]))
            .update(&state.db, &[("status", &PurchaseStatus::Cancelled)])
            .await?
    } else {
        0
    };
    abort_if(
        moved == 0,
        StatusCode::CONFLICT,
        lang.t("stock.purchasing.errors.moved_on", &[]),
    )?;
    Ok((
        Toast::success(lang.t("stock.purchasing.cancelled", &[])),
        Redirect::route("stock.purchasing.show", &[&order.id])?,
    ))
}

/// One received line.
#[derive(Deserialize, Debug, Clone)]
pub struct ReceivedLine {
    pub line: i64,
    pub received: Option<i64>,
}

impl Validate for ReceivedLine {
    fn rules(&self, v: &mut Validator) {
        v.field("received", &self.received).min(0).max(100_000);
    }
}

/// The receiving form.
#[derive(Deserialize, Debug, Default)]
pub struct ReceiveForm {
    #[serde(default)]
    pub lines: Vec<ReceivedLine>,
}

impl Validate for ReceiveForm {
    fn rules(&self, v: &mut Validator) {
        v.nested("lines", &self.lines);
    }
}

// [explain:stock.purchasing.show.receive]
/// The new average cost after `received` units at `unit_cost` join
/// `on_hand` units at `cost`: `(on_hand × cost + received × unit_cost) /
/// (on_hand + received)`, rounded half up. Stock below zero counts as none.
pub fn average_cost(on_hand: i64, cost: i64, received: i64, unit_cost: i64) -> i64 {
    let on_hand = on_hand.max(0);
    let units = on_hand + received;
    if units <= 0 {
        return cost;
    }
    (on_hand * cost + received * unit_cost + units / 2) / units
}
// [/explain:stock.purchasing.show.receive]

/// Receives `form`'s quantities on `order` in one transaction: a
/// `purchase` movement per line into the order's store (owner = location
/// = the store), each variant's average cost updated over the whole
/// company's stock, the order partial or received.
pub async fn receive_lines(
    db: &Db,
    order: &PurchaseOrder,
    form: &ReceiveForm,
    staff: Option<i64>,
) -> Result<PurchaseStatus> {
    let counted: HashMap<i64, i64> = form
        .lines
        .iter()
        .map(|l| (l.line, l.received.unwrap_or(0)))
        .collect();
    let mut tx = db.begin().await?;
    let lines = PurchaseOrderLine::query()
        .where_(PurchaseOrderLine::PURCHASE_ORDER_ID.eq(order.id))
        .get(&mut tx)
        .await?;
    let mut open = 0;
    // [explain:stock.purchasing.show.receive]
    for mut line in lines {
        let left = line.quantity - line.received_quantity;
        let now_in = counted.get(&line.id).copied().unwrap_or(0).clamp(0, left);
        if now_in > 0 {
            let on_hand: i64 = renox::db::sql(
                "SELECT CAST(COALESCE(SUM(on_hand), 0) AS BIGINT) FROM stock_levels WHERE variant_id = ?",
            )
            .bind(line.variant_id)
            .scalar(&mut tx)
            .await?;
            let cost: i64 = renox::db::sql("SELECT cost FROM product_variants WHERE id = ?")
                .bind(line.variant_id)
                .scalar(&mut tx)
                .await?;
            renox::db::sql("UPDATE product_variants SET cost = ?, updated_at = ? WHERE id = ?")
                .bind(average_cost(on_hand, cost, now_in, line.unit_cost))
                .bind(renox::db::now())
                .bind(line.variant_id)
                .execute(&mut tx)
                .await?;
            // [/explain:stock.purchasing.show.receive]
            StockMovement::record(
                &mut tx,
                StockMovement {
                    variant_id: line.variant_id,
                    owner_store_id: order.store_id,
                    location_store_id: order.store_id,
                    quantity: now_in,
                    reason: MovementReason::Purchase,
                    reference_type: Some(PurchaseOrder::TABLE.into()),
                    reference_id: Some(order.id),
                    staff_id: staff,
                    ..Default::default()
                },
            )
            .await?;
            line.received_quantity += now_in;
            line.save_only(&mut tx, &["received_quantity"]).await?;
        }
        open += line.quantity - line.received_quantity;
    }
    let status = if open == 0 {
        PurchaseStatus::Received
    } else {
        PurchaseStatus::Partial
    };
    let moved = PurchaseOrder::query()
        .where_(PurchaseOrder::ID.eq(order.id))
        .where_(PurchaseOrder::STATUS.is_in([PurchaseStatus::Ordered, PurchaseStatus::Partial]))
        .update(&mut tx, &[("status", &status)])
        .await?;
    if moved == 0 {
        return Err(abort(StatusCode::CONFLICT, "This order isn't open."));
    }
    if status == PurchaseStatus::Received {
        PurchaseOrder::query()
            .where_(PurchaseOrder::ID.eq(order.id))
            .update(&mut tx, &[("received_at", &renox::db::now())])
            .await?;
    }
    tx.commit().await?;
    Ok(status)
}

/// `POST /staff/purchase-orders/{order}/receive` (`stock.purchasing.receive`):
/// a delivery, whole or part; the store receiving owns what arrives.
pub async fn receive(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Valid(form): Valid<ReceiveForm>,
) -> Result<(Toast, Redirect)> {
    let order = access::find::<PurchaseOrder>(&state.db, &user, id).await?;
    access::require(&user, catalogue::STOCK_RECEIVE, StoreAttr::Owner, &order)?;
    let staff = Staff::of_user(&state.db, user.id).await?.map(|s| s.id);
    let status = receive_lines(&state.db, &order, &form, staff).await?;
    audit::record(
        &state,
        &user,
        "purchase_order.received",
        order.store_id,
        (PurchaseOrder::TABLE, order.id),
        json!({ "status": status.as_str() }),
    )
    .await?;
    Ok((
        Toast::success(state.current_lang().t(
            &format!("stock.purchasing.received_{}", status.as_str()),
            &[],
        )),
        Redirect::route("stock.purchasing.show", &[&order.id])?,
    ))
}
