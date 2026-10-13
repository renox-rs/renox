//! The admin panel for the catalogue and the shop's reference data (#239):
//! `renox-admin` resources for categories, brands, products, their variants
//! and photos, service tasks, service plans, suppliers and stores.
//!
//! - **Authorized by permission**, never a role's name: the panel opens for
//!   anyone holding `staff.access` and one of the permissions below in the
//!   store they work in ([`panel`]); each resource then asks for its own
//!   ([`AdminResource::allows`] here, and each model's `Policy`):
//!
//!   | Resource | Permission |
//!   |---|---|
//!   | categories, brands, products, variants, photos | `catalog.manage` |
//!   | the products' "change prices" actions | `catalog.manage` and `prices.change` |
//!   | service tasks, service plans | `plans.manage` |
//!   | suppliers | `purchasing.manage` |
//!   | stores | `stores.manage` (edit only: a store is opened with its address on the stores page) |
//!
//! - **The active store**: `/admin` isn't under `access::staff_routes`, so
//!   [`layer`] (an `App::layer` in `src/lib.rs`) runs the same active-store
//!   middleware for `/admin…`: a manager with `plans.manage` in North can
//!   edit plans while working in North.
//! - **Product descriptions** are Markdown, written with `renox-editors`'
//!   `markdown_editor` (toolbar and preview): the shop shows them with the
//!   `markdown` filter, which prints any HTML as text, so nothing typed can
//!   run in a customer's browser. The panel's field macro is replaced
//!   (`resources/views/components/admin_field.html`) to draw that editor for
//!   `description` textareas.
//! - **Discontinued products** go to the trash (products are soft deleted),
//!   restorable from the "Trash" tab.
//! - **Bulk actions** on products: change prices by a percentage typed in
//!   the action's sheet (`AdminAction::form`; every variant, audited), move
//!   to another category (a page to pick it), and
//!   discontinue. (The move keeps its extra page: it was written before
//!   actions could take input; see the area's notes in `explain.rs`.)

use renox::auth::Policy;
use renox::grid::Column;
use renox::prelude::*;
use renox_admin::{ActionContext, Admin, AdminAction, AdminResource, Entry, Field, Filter};
use serde::{Deserialize, Serialize};

use super::audit;
use super::model::Store;
use crate::app::access::catalogue::{
    CATALOG_MANAGE, PLANS_MANAGE, PRICES_CHANGE, PURCHASING_MANAGE, STAFF_ACCESS, STORES_MANAGE,
};
use crate::app::catalog::model::BrandForm;
use crate::app::catalog::model::{
    Brand, Category, CategoryKind, Product, ProductPhoto, ProductVariant,
};
use crate::app::plans::model::{Frequency, ServicePlan};
use crate::app::stock::model::Supplier;
use crate::app::workshop::model::ServiceTask;

/// The permissions that open the panel (with `staff.access`).
pub const PANEL_PERMISSIONS: [&str; 4] = [
    CATALOG_MANAGE,
    PLANS_MANAGE,
    PURCHASING_MANAGE,
    STORES_MANAGE,
];

// [explain:admin.panel]
/// The panel, registered in `src/lib.rs`.
pub fn panel() -> Admin {
    Admin::new()
        .title("Bike Shop admin")
        .authorize(|user| {
            user.allows(STAFF_ACCESS) && PANEL_PERMISSIONS.iter().any(|p| user.allows(p))
        })
        .resource(Categories)
        .resource(Brands)
        .resource(Products)
        .resource(Variants)
        .resource(Photos)
        .resource(ServiceTasks)
        .resource(ServicePlans)
        .resource(Suppliers)
        .resource(Stores)
}
// [/explain:admin.panel]

// [explain:admin.layer]
/// Runs the active-store middleware for `/admin…` (see the module docs).
pub async fn layer(
    user: Option<AuthUser>,
    session: Session,
    req: renox::axum::extract::Request,
    next: renox::axum::middleware::Next,
) -> Response {
    let path = req.uri().path();
    if path == "/admin" || path.starts_with("/admin/") {
        return crate::app::access::active_store::middleware(user, session, req, next).await;
    }
    next.run(req).await
}
// [/explain:admin.layer]

// [explain:admin.policy]
/// Whether `user` may do `ability` on a resource managed with `manage`:
/// that permission, and for the catalogue's prices `prices.change` as well
/// (the catalogue is the company's, so a store's `prices.change` alone isn't
/// enough).
fn may(user: &User, manage: &'static str, ability: &str) -> bool {
    user.has_permission(manage) && (ability != "changePrices" || user.has_permission(PRICES_CHANGE))
}

/// Every model in the panel answers by permission (in the active store).
macro_rules! policy_by_permission {
    ($($model:ty => $permission:expr),* $(,)?) => {$(
        impl Policy for $model {
            fn allows(&self, user: &User, ability: &str) -> bool {
                may(user, $permission, ability)
            }
        }
    )*};
}

policy_by_permission! {
    Category => CATALOG_MANAGE,
    Brand => CATALOG_MANAGE,
    Product => CATALOG_MANAGE,
    ProductVariant => CATALOG_MANAGE,
    ProductPhoto => CATALOG_MANAGE,
    ServiceTask => PLANS_MANAGE,
    ServicePlan => PLANS_MANAGE,
    Supplier => PURCHASING_MANAGE,
    Store => STORES_MANAGE,
}
// [/explain:admin.policy]

/// The kinds of category, for selects.
fn kinds() -> Vec<(String, String)> {
    CategoryKind::ALL
        .iter()
        .map(|k| (k.as_str().to_owned(), capitalize(k.as_str())))
        .collect()
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect::<String>())
        .unwrap_or_default()
        .replace('_', " ")
}

// --- Categories ---

/// The categories resource.
pub struct Categories;

// [explain:admin.categories.form]
/// A category's form.
#[derive(Deserialize, Serialize, Validate)]
pub struct CategoryForm {
    #[validate(required, max = 100)]
    pub name: String,
    #[validate(required, max = 100, alpha_dash)]
    pub slug: String,
    pub kind: CategoryKind,
    #[validate(exists("categories", "id"))]
    pub parent_id: Option<i64>,
    #[validate(min = 0)]
    pub position: i64,
}
// [/explain:admin.categories.form]

// [explain:admin.categories.show]
impl AdminResource for Categories {
    type Model = Category;
    type Form = CategoryForm;

    fn label(&self) -> &str {
        "Category"
    }
    fn plural_label(&self) -> &str {
        "Categories"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Catalogue")
    }
    fn record_title(&self, record: &Category) -> String {
        record.name.clone()
    }
    // [/explain:admin.categories.show]
    // [explain:admin.categories.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::select("kind", "Kind", kinds()),
            Column::related("parent", "Parent", "categories", "parent_id", "name"),
            Column::text("slug", "Slug"),
            Column::number("position", "Position"),
        ]
    }
    // [/explain:admin.categories.columns]
    // [explain:admin.categories.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::text("slug", "Slug")
                .required()
                .hint("Letters, digits and dashes: road-bikes."),
            Field::select("kind", "Kind", kinds())
                .required()
                .default_value("bike"),
            Field::belongs_to("parent_id", "Parent", "categories", "name"),
            Field::number("position", "Position")
                .min(0)
                .default_value(0),
        ]
    }
    fn rules(&self, form: &CategoryForm, record: Option<&Category>, v: &mut Validator) {
        let rule = v.field("slug", &form.slug).unique("categories", "slug");
        if let Some(record) = record {
            rule.ignore(record.id);
        }
    }
    // [/explain:admin.categories.form]
    fn fill(&self, category: &mut Category, form: CategoryForm) {
        category.name = form.name;
        category.slug = form.slug;
        category.kind = form.kind;
        category.parent_id = form.parent_id;
        category.position = form.position;
    }
    fn grid(&self, grid: renox::grid::Grid) -> renox::grid::Grid {
        grid.sort_by("kind,position,name")
    }
}

// --- Brands ---

/// The brands resource.
pub struct Brands;

// [explain:admin.brands.show]
impl AdminResource for Brands {
    type Model = Brand;
    type Form = BrandForm;

    fn label(&self) -> &str {
        "Brand"
    }
    fn plural_label(&self) -> &str {
        "Brands"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Catalogue")
    }
    fn record_title(&self, record: &Brand) -> String {
        record.name.clone()
    }
    // [/explain:admin.brands.show]
    // [explain:admin.brands.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::text("slug", "Slug"),
            Column::text("website", "Website"),
        ]
    }
    // [/explain:admin.brands.columns]
    // [explain:admin.brands.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::text("slug", "Slug").required(),
            Field::url("website", "Website").placeholder("https://"),
        ]
    }
    fn rules(&self, form: &BrandForm, record: Option<&Brand>, v: &mut Validator) {
        let rule = v.field("slug", &form.slug).unique("brands", "slug");
        if let Some(record) = record {
            rule.ignore(record.id);
        }
    }
    // [/explain:admin.brands.form]
    fn fill(&self, brand: &mut Brand, form: BrandForm) {
        form.fill(brand);
        brand.website = brand.website.take().filter(|w| !w.trim().is_empty());
    }
}

// --- Products ---

/// The products resource: bikes, gear and parts.
pub struct Products;

// [explain:admin.products.form]
/// A product's form. The description is Markdown (`markdown_editor`).
#[derive(Deserialize, Serialize, Validate)]
pub struct ProductForm {
    #[validate(required, max = 150)]
    pub name: String,
    #[validate(required, max = 150, alpha_dash)]
    pub slug: String,
    #[validate(required, exists("categories", "id"))]
    pub category_id: i64,
    #[validate(required, exists("brands", "id"))]
    pub brand_id: i64,
    #[validate(max = 20000)]
    pub description: Option<String>,
}
// [/explain:admin.products.form]

/// Raises or lowers every variant's price of `products` by `percent`,
/// rounded to whole units, and records it.
async fn change_prices(products: Vec<Product>, cx: ActionContext, percent: i64) -> Result<Toast> {
    let db = &cx.state.db;
    let ids: Vec<i64> = products.iter().map(|p| p.id).collect();
    let mut tx = db.begin().await?;
    let changed = renox::db::sql(format!(
        "UPDATE product_variants SET price = (price * {} + 50) / 100, updated_at = ? \
         WHERE product_id IN ({})",
        100 + percent,
        vec!["?"; ids.len()].join(", ")
    ))
    .bind(renox::db::now())
    .bind_all(
        ids.iter()
            .map(renox::db::ToDbValue::to_db_value)
            .collect::<Vec<_>>(),
    )
    .execute(&mut tx)
    .await?;
    tx.commit().await?;
    audit::record(db, &cx.user, PRICES_CHANGE, "catalog.prices_changed")
        .data(json!({ "products": ids, "percent": percent, "variants": changed }))
        .save()
        .await?;
    Ok(Toast::success(format!(
        "Prices of {changed} variants changed by {percent:+} %."
    )))
}

/// "Change prices by %": the percentage is typed in the action's sheet.
fn price_action() -> AdminAction<Product> {
    AdminAction::new("prices", "Change prices by %", |products, cx| async move {
        let percent = cx
            .input
            .parse::<f64>("percent")
            .map_or(0, |p| p.round() as i64);
        change_prices(products, cx, percent).await
    })
    .ability("changePrices")
    .form(vec![
        Field::number("percent", "Percent")
            .required()
            .step("1")
            .min(-90)
            .max(500)
            .suffix("%")
            .hint("Negative lowers the prices."),
    ])
    .description("Every variant's price of the selected products changes by this much.")
    .submit_label("Change prices")
}

// [explain:admin.products.show]
impl AdminResource for Products {
    type Model = Product;
    type Form = ProductForm;

    fn label(&self) -> &str {
        "Product"
    }
    fn plural_label(&self) -> &str {
        "Products"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Catalogue")
    }
    fn record_title(&self, record: &Product) -> String {
        record.name.clone()
    }
    // [/explain:admin.products.show]
    // [explain:admin.products.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::related("category", "Category", "categories", "category_id", "name"),
            Column::related("brand", "Brand", "brands", "brand_id", "name"),
            Column::text("slug", "Slug"),
            Column::datetime("updated_at", "Changed"),
            // Drawn by resources/views/renox-admin/products/cells.html.
            Column::custom("fits", "What fits"),
        ]
    }
    // [/explain:admin.products.columns]
    // [explain:admin.products.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::text("slug", "Slug").required(),
            Field::belongs_to("category_id", "Category", "categories", "name").required(),
            Field::belongs_to("brand_id", "Brand", "brands", "name").required(),
            Field::textarea("description", "Description")
                .rows(10)
                .span_full()
                .hint("Markdown: **bold**, lists, links. Shown on the product page."),
        ]
    }
    // [/explain:admin.products.form]
    // [explain:admin.products.show]
    fn entries(&self) -> Vec<Entry> {
        vec![
            Entry::text("name", "Name"),
            Entry::text("slug", "Slug").copyable(),
            Entry::new("description", "Description")
                .format("markdown")
                .span_full(),
            Entry::new("updated_at", "Last changed").format("since"),
        ]
    }
    // [/explain:admin.products.show]
    fn rules(&self, form: &ProductForm, record: Option<&Product>, v: &mut Validator) {
        let rule = v.field("slug", &form.slug).unique("products", "slug");
        if let Some(record) = record {
            rule.ignore(record.id);
        }
    }
    fn fill(&self, product: &mut Product, form: ProductForm) {
        product.name = form.name;
        product.slug = form.slug;
        product.category_id = form.category_id;
        product.brand_id = form.brand_id;
        product.description = form.description.unwrap_or_default();
    }
    fn filters(&self) -> Vec<Filter<Product>> {
        CategoryKind::ALL
            .iter()
            .map(|kind| {
                let kind = *kind;
                Filter::new(
                    kind.as_str(),
                    &format!("{}s", capitalize(kind.as_str())),
                    move |q| {
                        q.where_raw(
                            "category_id IN (SELECT id FROM categories WHERE kind = ?)",
                            vec![renox::db::ToDbValue::to_db_value(&kind)],
                        )
                    },
                )
            })
            .collect()
    }
    // [explain:admin.products.actions]
    fn actions(&self) -> Vec<AdminAction<Product>> {
        vec![
            price_action(),
            AdminAction::new(
                "move-category",
                "Move to a category…",
                |products, cx| async move { super::catalog_tools::start_move(products, cx).await },
            ),
            AdminAction::new(
                "discontinue",
                "Discontinue",
                |products: Vec<Product>, cx| async move {
                    let n = products.len();
                    for mut product in products {
                        product.delete(&cx.state.db).await?;
                    }
                    Ok(Toast::success(format!(
                        "{n} discontinued: they're in the Trash, restorable."
                    )))
                },
            )
            .ability("delete")
            .confirm("Discontinue the selected products? They leave the shop and go to the Trash.")
            .danger()
            .row(),
        ]
    }
    // [/explain:admin.products.actions]
}

// --- Variants ---

/// The variants resource: SKUs with their price and cost.
pub struct Variants;

// [explain:admin.product_variants.form]
/// A variant's form. Money in the smallest unit (`money` fields).
#[derive(Deserialize, Serialize, Validate)]
pub struct VariantForm {
    #[validate(required, exists("products", "id"))]
    pub product_id: i64,
    #[validate(required, max = 64)]
    pub sku: String,
    #[validate(max = 20)]
    pub size: Option<String>,
    #[validate(max = 40)]
    pub colour: Option<String>,
    #[validate(required, min = 0)]
    pub price: i64,
    #[validate(required, min = 0)]
    pub cost: i64,
    #[validate(min = 0)]
    pub reorder_level: i64,
}
// [/explain:admin.product_variants.form]

// [explain:admin.product_variants.show]
impl AdminResource for Variants {
    type Model = ProductVariant;
    type Form = VariantForm;

    fn label(&self) -> &str {
        "Variant"
    }
    fn plural_label(&self) -> &str {
        "Variants"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Catalogue")
    }
    fn record_title(&self, record: &ProductVariant) -> String {
        record.sku.clone()
    }
    // [/explain:admin.product_variants.show]
    // [explain:admin.product_variants.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("sku", "SKU").searchable(),
            Column::related("product", "Product", "products", "product_id", "name"),
            Column::text("size", "Size"),
            Column::text("colour", "Colour"),
            Column::money("price", "Price"),
            Column::money("cost", "Cost"),
            Column::number("reorder_level", "Reorder at"),
        ]
    }
    // [/explain:admin.product_variants.columns]
    // [explain:admin.product_variants.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::belongs_to("product_id", "Product", "products", "name").required(),
            Field::text("sku", "SKU").required(),
            Field::text("size", "Size"),
            Field::text("colour", "Colour"),
            Field::money("price", "Price").required(),
            Field::money("cost", "Cost").required(),
            Field::number("reorder_level", "Reorder at")
                .min(0)
                .default_value(0),
        ]
    }
    fn rules(&self, form: &VariantForm, record: Option<&ProductVariant>, v: &mut Validator) {
        let rule = v.field("sku", &form.sku).unique("product_variants", "sku");
        if let Some(record) = record {
            rule.ignore(record.id);
        }
    }
    // [/explain:admin.product_variants.form]
    fn fill(&self, variant: &mut ProductVariant, form: VariantForm) {
        variant.product_id = form.product_id;
        variant.sku = form.sku;
        variant.size = form.size.filter(|s| !s.trim().is_empty());
        variant.colour = form.colour.filter(|c| !c.trim().is_empty());
        variant.price = form.price;
        variant.cost = form.cost;
        variant.reorder_level = form.reorder_level;
    }
}

// --- Photos ---

/// The product photos resource.
pub struct Photos;

// [explain:admin.product-photos.form]
/// A photo's form: where the file is and its alternative text.
#[derive(Deserialize, Serialize, Validate)]
pub struct PhotoForm {
    #[validate(required, exists("products", "id"))]
    pub product_id: i64,
    #[validate(required, max = 255)]
    pub path: String,
    #[validate(required, max = 200)]
    pub alt: String,
    #[validate(min = 0)]
    pub position: i64,
}
// [/explain:admin.product-photos.form]

// [explain:admin.product-photos.show]
impl AdminResource for Photos {
    type Model = ProductPhoto;
    type Form = PhotoForm;

    fn label(&self) -> &str {
        "Photo"
    }
    fn plural_label(&self) -> &str {
        "Photos"
    }
    fn slug(&self) -> &str {
        "product-photos"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Catalogue")
    }
    // [/explain:admin.product-photos.show]
    // [explain:admin.product-photos.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::related("product", "Product", "products", "product_id", "name").searchable(),
            Column::text("path", "File"),
            Column::text("alt", "Description"),
            Column::number("position", "Position"),
        ]
    }
    // [/explain:admin.product-photos.columns]
    // [explain:admin.product-photos.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::belongs_to("product_id", "Product", "products", "name").required(),
            Field::text("path", "File")
                .required()
                .hint("Under public/ (blocks/demo/city-side.svg) or on the storage disk."),
            Field::text("alt", "Description")
                .required()
                .hint("What the photo shows, for screen readers."),
            Field::number("position", "Position")
                .min(0)
                .default_value(0),
        ]
    }
    fn fill(&self, photo: &mut ProductPhoto, form: PhotoForm) {
        photo.product_id = form.product_id;
        photo.path = form.path;
        photo.alt = form.alt;
        photo.position = form.position;
    }
    // [/explain:admin.product-photos.form]
}

// --- Service tasks ---

/// The workshop's service tasks.
pub struct ServiceTasks;

// [explain:admin.service-tasks.form]
/// A service task's form.
#[derive(Deserialize, Serialize, Validate)]
pub struct ServiceTaskForm {
    #[validate(required, max = 100)]
    pub name: String,
    #[validate(required, max = 100, alpha_dash)]
    pub slug: String,
    #[validate(required, min = 5, max = 600)]
    pub minutes: i64,
    #[validate(required, min = 0)]
    pub price: i64,
}
// [/explain:admin.service-tasks.form]

// [explain:admin.service-tasks.show]
impl AdminResource for ServiceTasks {
    type Model = ServiceTask;
    type Form = ServiceTaskForm;

    fn label(&self) -> &str {
        "Service task"
    }
    fn plural_label(&self) -> &str {
        "Service tasks"
    }
    fn slug(&self) -> &str {
        "service-tasks"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Workshop")
    }
    fn record_title(&self, record: &ServiceTask) -> String {
        record.name.clone()
    }
    // [/explain:admin.service-tasks.show]
    // [explain:admin.service-tasks.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::number("minutes", "Minutes"),
            Column::money("price", "Price"),
        ]
    }
    // [/explain:admin.service-tasks.columns]
    // [explain:admin.service-tasks.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::text("slug", "Slug").required(),
            Field::number("minutes", "Minutes")
                .required()
                .min(5)
                .max(600),
            Field::money("price", "Price").required(),
        ]
    }
    fn rules(&self, form: &ServiceTaskForm, record: Option<&ServiceTask>, v: &mut Validator) {
        let rule = v.field("slug", &form.slug).unique("service_tasks", "slug");
        if let Some(record) = record {
            rule.ignore(record.id);
        }
    }
    // [/explain:admin.service-tasks.form]
    fn fill(&self, task: &mut ServiceTask, form: ServiceTaskForm) {
        task.name = form.name;
        task.slug = form.slug;
        task.minutes = form.minutes;
        task.price = form.price;
    }
}

// --- Service plans ---

/// The service plans.
pub struct ServicePlans;

// [explain:admin.service-plans.form]
/// A plan's form.
#[derive(Deserialize, Serialize, Validate)]
pub struct ServicePlanForm {
    #[validate(required, max = 100)]
    pub name: String,
    #[validate(required, max = 100, alpha_dash)]
    pub slug: String,
    pub frequency: Frequency,
    #[validate(required, min = 0)]
    pub price: i64,
    #[validate(max = 5000)]
    pub description: Option<String>,
    pub active: bool,
}
// [/explain:admin.service-plans.form]

fn frequencies() -> Vec<(String, String)> {
    Frequency::ALL
        .iter()
        .map(|f| (f.as_str().to_owned(), capitalize(f.as_str())))
        .collect()
}

// [explain:admin.service-plans.show]
impl AdminResource for ServicePlans {
    type Model = ServicePlan;
    type Form = ServicePlanForm;

    fn label(&self) -> &str {
        "Service plan"
    }
    fn plural_label(&self) -> &str {
        "Service plans"
    }
    fn slug(&self) -> &str {
        "service-plans"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Workshop")
    }
    fn record_title(&self, record: &ServicePlan) -> String {
        record.name.clone()
    }
    // [/explain:admin.service-plans.show]
    // [explain:admin.service-plans.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::select("frequency", "Every", frequencies()),
            Column::money("price", "Price"),
            Column::bool("active", "On sale"),
        ]
    }
    // [/explain:admin.service-plans.columns]
    // [explain:admin.service-plans.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::text("slug", "Slug").required(),
            Field::select("frequency", "Every", frequencies())
                .required()
                .default_value("monthly"),
            Field::money("price", "Price per visit").required(),
            Field::textarea("description", "Description")
                .rows(6)
                .span_full()
                .hint("Markdown."),
            Field::toggle("active", "On sale").default_value(true),
        ]
    }
    fn rules(&self, form: &ServicePlanForm, record: Option<&ServicePlan>, v: &mut Validator) {
        let rule = v.field("slug", &form.slug).unique("service_plans", "slug");
        if let Some(record) = record {
            rule.ignore(record.id);
        }
    }
    // [/explain:admin.service-plans.form]
    fn fill(&self, plan: &mut ServicePlan, form: ServicePlanForm) {
        plan.name = form.name;
        plan.slug = form.slug;
        plan.frequency = form.frequency;
        plan.price = form.price;
        plan.description = form.description.unwrap_or_default();
        plan.active = form.active;
    }
}

// --- Suppliers ---

/// The suppliers the stores buy from.
pub struct Suppliers;

// [explain:admin.suppliers.form]
/// A supplier's form.
#[derive(Deserialize, Serialize, Validate)]
pub struct SupplierForm {
    #[validate(required, max = 150)]
    pub name: String,
    #[validate(email, max = 255)]
    pub email: Option<String>,
    #[validate(max = 40)]
    pub phone: Option<String>,
    #[validate(required, min = 0, max = 365)]
    pub lead_days: i64,
}
// [/explain:admin.suppliers.form]

// [explain:admin.suppliers.show]
impl AdminResource for Suppliers {
    type Model = Supplier;
    type Form = SupplierForm;

    fn label(&self) -> &str {
        "Supplier"
    }
    fn plural_label(&self) -> &str {
        "Suppliers"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Purchasing")
    }
    fn record_title(&self, record: &Supplier) -> String {
        record.name.clone()
    }
    // [/explain:admin.suppliers.show]
    // [explain:admin.suppliers.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::text("email", "Email"),
            Column::text("phone", "Phone"),
            Column::number("lead_days", "Lead time (days)"),
        ]
    }
    // [/explain:admin.suppliers.columns]
    // [explain:admin.suppliers.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::email("email", "Email"),
            Field::tel("phone", "Phone"),
            Field::number("lead_days", "Lead time (days)")
                .required()
                .min(0)
                .max(365)
                .default_value(7),
        ]
    }
    fn fill(&self, supplier: &mut Supplier, form: SupplierForm) {
        supplier.name = form.name;
        supplier.email = form.email.filter(|e| !e.trim().is_empty());
        supplier.phone = form.phone.filter(|p| !p.trim().is_empty());
        supplier.lead_days = form.lead_days;
    }
    // [/explain:admin.suppliers.form]
}

// --- Stores ---

/// The stores (edit only; the hours and the fee rate are on the stores page).
pub struct Stores;

// [explain:admin.stores.form]
/// A store's form in the panel.
#[derive(Deserialize, Serialize, Validate)]
pub struct StoreForm {
    #[validate(required, max = 100)]
    pub name: String,
    #[validate(required, max = 40)]
    pub phone: String,
    #[validate(required, email, max = 255)]
    pub email: String,
    #[validate(required, min = 0, max = 1440)]
    pub workshop_minutes_per_day: i64,
}
// [/explain:admin.stores.form]

// [explain:admin.stores.show]
impl AdminResource for Stores {
    type Model = Store;
    type Form = StoreForm;

    fn label(&self) -> &str {
        "Store"
    }
    fn plural_label(&self) -> &str {
        "Stores"
    }
    fn navigation_group(&self) -> Option<&str> {
        Some("Stores")
    }
    fn record_title(&self, record: &Store) -> String {
        record.name.clone()
    }
    // [/explain:admin.stores.show]
    // [explain:admin.stores.columns]
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::text("phone", "Phone"),
            Column::text("email", "Email"),
            Column::number("workshop_minutes_per_day", "Workshop minutes / day"),
        ]
    }
    // [/explain:admin.stores.columns]
    // [explain:admin.stores.form]
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::tel("phone", "Phone").required(),
            Field::email("email", "Email").required(),
            Field::number("workshop_minutes_per_day", "Workshop minutes / day")
                .required()
                .min(0)
                .max(1440),
        ]
    }
    fn fill(&self, store: &mut Store, form: StoreForm) {
        store.name = form.name;
        store.phone = form.phone;
        store.email = form.email;
        store.workshop_minutes_per_day = form.workshop_minutes_per_day;
    }
    // [/explain:admin.stores.form]
    fn creatable(&self) -> bool {
        false
    }
    fn deletable(&self) -> bool {
        false
    }
    // [explain:admin.stores.allows]
    /// A store is made with its address and opening hours, not here;
    /// deleting one would orphan its stock and books.
    fn allows(&self, user: &AuthUser, ability: &str, record: Option<&Store>) -> bool {
        match ability {
            "create" | "delete" | "deleteAny" | "forceDelete" | "forceDeleteAny" => false,
            _ => match record {
                Some(store) => crate::app::access::can_in(user, STORES_MANAGE, store.id),
                None => user.allows(STORES_MANAGE),
            },
        }
    }
    // [/explain:admin.stores.allows]
}
