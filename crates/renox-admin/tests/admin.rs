//! The admin panel (#148): who gets in, the list (grid, filters, exports,
//! actions), create, edit, view, delete with soft deletes and the trash,
//! and the pages an app replaces.

use renox::chrono::NaiveDate;
use renox::grid::Column;
use renox::prelude::*;
use renox::testing::TestApp;
use renox_admin::{
    Admin, AdminAction, AdminResource, Entry, Field, Filter, RelationManager, SaveContext, Slot,
};
use serde::{Deserialize, Serialize};

const PASSWORD: &str = "secret-password-1";
const VIEWER: &str = "viewer@example.com";

#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "products", soft_deletes)]
struct Product {
    id: i64,
    name: String,
    sku: String,
    price: i64,
    status: String,
    active: bool,
    category_id: Option<i64>,
    released_on: Option<NaiveDate>,
    created_at: Option<DateTime>,
    updated_at: Option<DateTime>,
    deleted_at: Option<DateTime>,
}

/// The viewer looks; everyone else in the panel may do anything.
impl Policy for Product {
    fn allows(&self, user: &User, ability: &str) -> bool {
        matches!(ability, "viewAny" | "view") || user.email != VIEWER
    }
}

#[derive(Deserialize, Serialize, Validate)]
struct ProductForm {
    #[validate(required, max = 100)]
    name: String,
    #[validate(required, max = 20)]
    sku: String,
    #[validate(required, min = 0)]
    price: i64,
    #[validate(required, one_of(&["draft", "live"]))]
    status: String,
    active: bool,
    #[validate(exists("categories", "id"))]
    category_id: Option<i64>,
    released_on: Option<NaiveDate>,
}

struct Products;

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
        Some("Shop")
    }

    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::text("sku", "SKU").searchable(),
            Column::money("price", "Price"),
            Column::select("status", "Status", [("draft", "Draft"), ("live", "Live")]),
            Column::bool("active", "Active"),
            Column::custom("stock", "Stock"),
        ]
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::text("sku", "SKU").required().readonly_on_edit(),
            Field::money("price", "Price").required(),
            Field::select("status", "Status", [("draft", "Draft"), ("live", "Live")]).required(),
            Field::toggle("active", "Active").default_value(true),
            Field::belongs_to("category_id", "Category", "categories", "name"),
            Field::date("released_on", "Released"),
        ]
    }

    fn entries(&self) -> Vec<Entry> {
        let mut entries: Vec<Entry> = self
            .columns()
            .iter()
            .filter_map(Entry::from_column)
            .collect();
        entries.push(Entry::new("released_on", "Released").format("date"));
        entries
    }

    fn fill(&self, product: &mut Product, form: ProductForm) {
        product.name = form.name;
        product.sku = form.sku.to_uppercase();
        product.price = form.price;
        product.status = form.status;
        product.active = form.active;
        product.category_id = form.category_id;
        product.released_on = form.released_on;
    }

    fn rules(&self, form: &ProductForm, record: Option<&Product>, v: &mut Validator) {
        let upper = form.sku.to_uppercase();
        let sku = v
            .field("sku", &upper)
            .label("SKU")
            .unique("products", "sku");
        if let Some(product) = record {
            sku.ignore(product.id);
        }
    }

    fn filters(&self) -> Vec<Filter<Product>> {
        vec![Filter::new("live", "Live", |q| {
            q.where_eq("status", "live")
        })]
    }

    fn relations(&self) -> Vec<RelationManager> {
        vec![
            RelationManager::has_many("variants", "Variants", Variants, "product_id"),
            RelationManager::belongs_to_many(
                "tags",
                "Tags",
                Tags,
                "product_tag",
                "product_id",
                "tag_id",
            ),
        ]
    }

    async fn saved(&self, product: &Product, cx: &SaveContext) -> Result {
        let was = cx
            .previous
            .as_ref()
            .and_then(|old| old["price"].as_i64())
            .map_or_else(|| "-".to_owned(), |price| price.to_string());
        let note = format!(
            "{} {} by {}: {was} -> {}",
            if cx.created { "created" } else { "updated" },
            product.name,
            cx.user.name,
            product.price
        );
        renox::db::sql("INSERT INTO audits (note) VALUES (?)")
            .bind(note)
            .execute(&cx.state.db)
            .await?;
        Ok(())
    }

    fn actions(&self) -> Vec<AdminAction<Product>> {
        vec![
            AdminAction::new(
                "reprice",
                "Change price by %",
                |products: Vec<Product>, cx| async move {
                    let percent: f64 = cx.input.parse("percent").unwrap_or(0.0);
                    let note = cx.input.text("note").to_owned();
                    for product in &products {
                        let price = (product.price as f64 * (1.0 + percent / 100.0)).round() as i64;
                        Product::query()
                            .where_eq("id", product.id)
                            .update(&cx.state.db, &[("price", &price)])
                            .await?;
                    }
                    Ok(Toast::success(format!(
                        "{} repriced by {percent} % {note}",
                        products.len()
                    )))
                },
            )
            .form(vec![
                Field::number("percent", "Percent")
                    .required()
                    .min(-90)
                    .max(500),
                Field::text("note", "Note"),
                Field::select("mode", "Mode", [("all", "All"), ("some", "Some")]),
            ])
            .description("Every selected product changes by this much.")
            .submit_label("Change prices")
            .row(),
            AdminAction::new(
                "publish",
                "Publish",
                |products: Vec<Product>, cx| async move {
                    let ids: Vec<i64> = products.iter().map(|p| p.id).collect();
                    let n = Product::query()
                        .where_in("id", ids)
                        .update(&cx.state.db, &[("status", &"live")])
                        .await?;
                    Ok(Toast::success(format!(
                        "{n} published by {}.",
                        cx.user.name
                    )))
                },
            )
            .confirm("Publish the selected products?")
            .row(),
        ]
    }
}

#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "categories")]
struct Category {
    id: i64,
    name: String,
}

/// Nobody may see categories but the owner.
impl Policy for Category {
    fn allows(&self, user: &User, _ability: &str) -> bool {
        user.email == "owner@example.com"
    }
}

#[derive(Deserialize, Serialize, Validate)]
struct CategoryForm {
    #[validate(required, max = 50)]
    name: String,
}

struct Categories;

impl AdminResource for Categories {
    type Model = Category;
    type Form = CategoryForm;

    fn label(&self) -> &str {
        "Category"
    }

    fn plural_label(&self) -> &str {
        "Categories"
    }

    fn columns(&self) -> Vec<Column> {
        vec![Column::text("name", "Name")]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("name", "Name").required()]
    }

    /// No view page: rows open the edit page.
    fn entries(&self) -> Vec<Entry> {
        Vec::new()
    }

    fn fill(&self, category: &mut Category, form: CategoryForm) {
        category.name = form.name;
    }
}

fn admin() -> Admin {
    Admin::new()
        .title("Back office")
        .authorize(|user| user.email.ends_with("@example.com"))
        .resource(Products)
        .resource(Categories)
}

async fn app_with(admin: Admin) -> TestApp {
    TestApp::new(
        App::new()
            .module(Auth::new())
            .module(admin)
            .migrations(renox::migrations!("tests/migrations")),
    )
    .await
}

async fn app() -> TestApp {
    app_with(admin()).await
}

async fn user(app: &TestApp, email: &str) -> User {
    User::register(app.db(), "Ana", email, PASSWORD)
        .await
        .unwrap()
}

async fn product(app: &TestApp, name: &str, sku: &str, status: &str) -> Product {
    Product::create(
        app.db(),
        Product {
            name: name.into(),
            sku: sku.into(),
            price: 75_000,
            status: status.into(),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[renox::test]
async fn guests_log_in_and_others_are_refused() {
    let app = app().await;
    app.get("/admin").await.assert_redirect("/login");
    app.get("/admin/products").await.assert_redirect("/login");

    let stranger = user(&app, "someone@elsewhere.test").await;
    app.acting_as(&stranger);
    app.get("/admin").await.assert_forbidden();
    app.get("/admin/products").await.assert_forbidden();
    app.get("/admin/products/create").await.assert_forbidden();
}

#[renox::test]
async fn nobody_gets_in_until_the_app_says_who_may() {
    let app = app_with(Admin::new().resource(Products)).await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.get("/admin").await.assert_forbidden();
    app.get("/admin/products").await.assert_forbidden();
}

#[renox::test]
async fn the_dashboard_counts_what_the_user_may_see() {
    let app = app().await;
    product(&app, "Coffee", "C-1", "live").await;
    product(&app, "Tea", "T-1", "draft").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let page = app.get("/admin").await;
    page.assert_ok()
        .assert_view("renox-admin/dashboard.html")
        .assert_see("Back office")
        .assert_see(r#"href="/admin/products""#)
        .assert_see("Shop")
        // Categories' policy lets only the owner see them.
        .assert_dont_see("Categories");
    assert!(page.text().contains("rx-shell"), "{}", page.text());

    let owner = user(&app, "owner@example.com").await;
    app.acting_as(&owner);
    app.get("/admin").await.assert_see("Categories");
    app.get("/admin/categories").await.assert_ok();
    app.acting_as(&ana);
    app.get("/admin/categories").await.assert_forbidden();
}

#[renox::test]
async fn the_list_is_a_grid_with_search_filters_and_actions() {
    let app = app().await;
    product(&app, "Iced coffee", "C-1", "live").await;
    product(&app, "Green tea", "T-1", "draft").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);

    let page = app.get("/admin/products").await;
    page.assert_ok()
        .assert_view("renox-admin/index.html")
        .assert_see("Iced coffee")
        .assert_see("Green tea")
        .assert_see(r#"href="/admin/products/create""#)
        .assert_see("New product")
        .assert_see("Publish")
        .assert_see("/admin/products/actions/delete")
        .assert_see("/admin/products/actions/publish")
        // Tabs: all, the named filter, the trash.
        .assert_see(r#"href="/admin/products?filter=live""#)
        .assert_see(r#"href="/admin/products?filter=trashed""#);

    // The grid's own search and the named filter.
    app.get("/admin/products?search=iced")
        .await
        .assert_see("Iced coffee")
        .assert_dont_see("Green tea");
    app.get("/admin/products?filter=live")
        .await
        .assert_see("Iced coffee")
        .assert_dont_see("Green tea");
    app.get("/admin/products?in.status=draft")
        .await
        .assert_see("Green tea")
        .assert_dont_see("Iced coffee");
}

#[renox::test]
async fn the_list_exports_csv() {
    let app = app().await;
    product(&app, "Iced coffee", "C-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let file = app.get("/admin/products?export=csv").await;
    file.assert_ok();
    assert!(
        file.header("content-type").unwrap().starts_with("text/csv"),
        "{:?}",
        file.header("content-type")
    );
    let text = file.text();
    assert!(text.contains("Name,SKU,Price,Status,Active"), "{text}");
    assert!(text.contains("Iced coffee,C-1,750.00,Live,Yes"), "{text}");
}

#[renox::test]
async fn a_viewer_sees_no_buttons_and_may_change_nothing() {
    let app = app().await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    let viewer = user(&app, VIEWER).await;
    app.acting_as(&viewer);
    app.get("/admin/products")
        .await
        .assert_ok()
        .assert_dont_see("New product")
        .assert_dont_see("/admin/products/actions/delete")
        .assert_dont_see("/admin/products/actions/publish");
    app.get(&format!("/admin/products/{}", coffee.id))
        .await
        .assert_ok()
        .assert_dont_see("/edit");
    app.get("/admin/products/create").await.assert_forbidden();
    app.get(&format!("/admin/products/{}/edit", coffee.id))
        .await
        .assert_forbidden();
    app.post("/admin/products", &[("name", "X")])
        .await
        .assert_forbidden();
    app.delete(&format!("/admin/products/{}", coffee.id))
        .await
        .assert_forbidden();
    app.htmx()
        .post(
            "/admin/products/actions/publish",
            &[("ids", &coffee.id.to_string())],
        )
        .await
        .assert_forbidden();
    app.assert_database_count("products", 1).await;
}

#[renox::test]
async fn the_create_page_draws_the_fields() {
    let app = app().await;
    Category::create(
        app.db(),
        Category {
            name: "Drinks".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let page = app.get("/admin/products/create").await;
    let html = page.assert_ok().assert_view("renox-admin/form.html").text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has("New product");
    has(r#"hx-post="/admin/products""#);
    has(r#"data-live-validate"#);
    has(r#"name="name" type="text""#);
    // Money: a number with the currency's code before it.
    has(r#"<span class="rx-affix__text" id="rx-price-prefix">USD</span>"#);
    has(r#"name="price" type="number""#);
    // The select and the belongs-to choices.
    has(r#"<option value="live">Live</option>"#);
    has(r#">Drinks</option>"#);
    has("data-rx-combobox");
    // A toggle on by default, and the date picker.
    has(r#"role="switch" id="rx-active" name="active" value="on" checked"#);
    has(r#"name="released_on""#);
    has("data-rx-date");
}

#[renox::test]
async fn creating_checks_the_form_then_saves() {
    let app = app().await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);

    // htmx: errors stay in the form (422).
    let failed = app
        .htmx()
        .post("/admin/products", &[("name", ""), ("price", "-1")])
        .await;
    failed.assert_status(422);
    assert!(failed.json_path("errors.name").is_array());
    assert!(failed.json_path("errors.price").is_array());
    app.assert_database_count("products", 0).await;

    let saved = app
        .htmx()
        .post(
            "/admin/products",
            &[
                ("name", "Iced coffee"),
                ("sku", "c-1"),
                ("price", "250"),
                ("status", "live"),
                ("active", "on"),
                ("released_on", "2026-10-01"),
            ],
        )
        .await;
    saved.assert_hx_redirect("/admin/products");
    let coffee = Product::query().first(app.db()).await.unwrap().unwrap();
    assert_eq!(coffee.sku, "C-1");
    assert_eq!(coffee.price, 25_000);
    assert!(coffee.active);
    assert_eq!(coffee.released_on, NaiveDate::from_ymd_opt(2026, 10, 1));
    // The toast waits for the list.
    app.get("/admin/products")
        .await
        .assert_see("Product created.");

    // The resource's own rule: the SKU is taken.
    let taken = app
        .htmx()
        .post(
            "/admin/products",
            &[
                ("name", "Other"),
                ("sku", "C-1"),
                ("price", "1"),
                ("status", "draft"),
            ],
        )
        .await;
    taken.assert_status(422);
    assert!(taken.json_path("errors.sku").is_array());

    // Without htmx: a redirect back with the errors and the input.
    app.post(
        "/admin/products",
        &[
            ("name", "Other"),
            ("sku", "C-1"),
            ("price", "1"),
            ("status", "draft"),
        ],
    )
    .await
    .assert_status(303);
    app.assert_database_count("products", 1).await;
}

/// Money fields show and take whole units of `APP_CURRENCY` (`12.99`) and
/// keep the smallest unit (`1299`); a currency without decimals is as typed.
#[renox::test]
async fn money_fields_take_whole_units() {
    let app = app().await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let fields = |sku: &'static str, price: &'static str| {
        [
            ("name", "Tea"),
            ("sku", sku),
            ("price", price),
            ("status", "live"),
        ]
    };
    app.htmx()
        .post("/admin/products", &fields("t-1", "12.99"))
        .await
        .assert_hx_redirect("/admin/products");
    let tea = Product::query().first(app.db()).await.unwrap().unwrap();
    assert_eq!(tea.price, 1299);
    let edit = format!("/admin/products/{}/edit", tea.id);
    app.get(&edit).await.assert_see(r#"value="12.99""#);
    // JSON too, and rounding to the cent; text that isn't an amount fails.
    app.post_json(
        "/admin/products",
        &json!({ "name": "Green tea", "sku": "t-3", "price": 0.305, "status": "live", "active": true }),
    )
    .await;
    let tea = Product::find(app.db(), tea.id + 1).await.unwrap().unwrap();
    assert_eq!(tea.price, 31);
    let edit = format!("/admin/products/{}/edit", tea.id);
    app.get(&edit).await.assert_see(r#"value="0.31""#);
    app.htmx()
        .post("/admin/products", &fields("t-2", "lots"))
        .await
        .assert_status(422);
    let shown = app.get(&format!("/admin/products/{}", tea.id)).await;
    shown.assert_see("$0.31");

    // IDR has no cents: the amount is as typed.
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(admin())
            .migrations(renox::migrations!("tests/migrations")),
        |c| c.currency = "IDR".into(),
    )
    .await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.htmx()
        .post("/admin/products", &fields("t-1", "75000"))
        .await
        .assert_hx_redirect("/admin/products");
    let tea = Product::query().first(app.db()).await.unwrap().unwrap();
    assert_eq!(tea.price, 75_000);
    let html = app
        .get(&format!("/admin/products/{}/edit", tea.id))
        .await
        .text();
    assert!(html.contains(r#"value="75000""#), "{html}");
    assert!(!html.contains(r#"step="0.01""#), "{html}");
    app.get(&format!("/admin/products/{}", tea.id))
        .await
        .assert_see("Rp 75,000");
}

#[renox::test]
async fn editing_shows_the_record_and_saves_it() {
    let app = app().await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let url = format!("/admin/products/{}", coffee.id);
    let html = app.get(&format!("{url}/edit")).await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"value="Coffee""#);
    // Money in whole units: 75000 cents.
    has(r#"value="750.00""#);
    has(r#"step="0.01""#);
    has(r#"<option value="live" selected>Live</option>"#);
    // The SKU can't change here, and the page can delete.
    has(r#"name="sku" type="text" value="C-1" required aria-required="true" readonly"#);
    has(r#"name="_method" value="PUT""#);
    has("Delete this product?");

    app.htmx()
        .put(
            &url,
            &[
                ("name", "Hot coffee"),
                ("sku", "C-1"),
                ("price", "300.00"),
                ("status", "draft"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/products");
    let saved = Product::find(app.db(), coffee.id).await.unwrap().unwrap();
    assert_eq!(saved.name, "Hot coffee");
    assert_eq!(saved.price, 30_000);
    assert_eq!(saved.status, "draft");
    // An unticked toggle is `false`.
    assert!(!saved.active);

    app.get("/admin/products/999/edit").await.assert_not_found();
    app.get("/admin/products/abc/edit").await.assert_not_found();
}

#[renox::test]
async fn the_view_page_lists_the_entries() {
    let app = app().await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let page = app.get(&format!("/admin/products/{}", coffee.id)).await;
    page.assert_ok()
        .assert_view("renox-admin/show.html")
        .assert_see("Product #")
        .assert_see("Coffee")
        .assert_see("$750.00")
        .assert_see(r#"<span class="rx-badge">Live</span>"#)
        .assert_see(&format!("/admin/products/{}/edit", coffee.id))
        .assert_see("Released");

    // A resource without entries has no view page: rows open the edit page.
    let owner = user(&app, "owner@example.com").await;
    app.acting_as(&owner);
    let drinks = Category::create(
        app.db(),
        Category {
            name: "Drinks".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    app.get(&format!("/admin/categories/{}", drinks.id))
        .await
        .assert_not_found();
    app.get("/admin/categories")
        .await
        .assert_see(&format!("/admin/categories/{}/edit", drinks.id))
        .assert_dont_see(&format!(r#"href="/admin/categories/{}""#, drinks.id));
}

#[renox::test]
async fn deleting_moves_to_the_trash_where_records_come_back() {
    let app = app().await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    let tea = product(&app, "Tea", "T-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);

    // From the grid (htmx): a toast, the grid reloads itself.
    app.htmx()
        .delete(&format!("/admin/products/{}", coffee.id))
        .await
        .assert_status(204);
    // From the edit page's form: back to the list.
    app.delete(&format!("/admin/products/{}", tea.id))
        .await
        .assert_redirect("/admin/products");
    app.assert_database_count("products", 2).await;
    assert_eq!(Product::query().count(app.db()).await.unwrap(), 0);

    let trash = app.get("/admin/products?filter=trashed").await;
    trash
        .assert_ok()
        .assert_see("Coffee")
        .assert_see("/admin/products/actions/restore")
        .assert_see("/admin/products/actions/force-delete")
        .assert_dont_see("New product");
    app.get(&format!("/admin/products/{}", coffee.id))
        .await
        .assert_ok()
        .assert_see("is in the trash")
        .assert_see("Restore");

    app.htmx()
        .post(
            "/admin/products/actions/restore?filter=trashed",
            &[("ids", &coffee.id.to_string())],
        )
        .await
        .assert_status(204);
    assert_eq!(Product::query().count(app.db()).await.unwrap(), 1);
    // Restoring outside the trash isn't offered.
    app.htmx()
        .post(
            "/admin/products/actions/restore",
            &[("ids", &tea.id.to_string())],
        )
        .await
        .assert_not_found();

    app.post(
        &format!("/admin/products/{}/actions/force-delete", tea.id),
        &[],
    )
    .await
    .assert_redirect("/admin/products");
    app.assert_database_count("products", 1).await;
}

#[renox::test]
async fn bulk_actions_run_on_the_selection_or_everything_filtered() {
    let app = app().await;
    let a = product(&app, "Coffee", "C-1", "draft").await;
    let b = product(&app, "Tea", "T-1", "draft").await;
    let c = product(&app, "Juice", "J-1", "draft").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);

    let ids = format!("{},{}", a.id, b.id);
    let done = app
        .htmx()
        .post("/admin/products/actions/publish", &[("ids", &ids)])
        .await;
    done.assert_status(204);
    let live = Product::where_eq("status", "live")
        .count(app.db())
        .await
        .unwrap();
    assert_eq!(live, 2);

    // One row from its menu.
    app.htmx()
        .post(&format!("/admin/products/{}/actions/publish", c.id), &[])
        .await
        .assert_status(204);
    assert_eq!(
        Product::where_eq("status", "live")
            .count(app.db())
            .await
            .unwrap(),
        3
    );

    // "Select all matching": the grid's filters choose the rows.
    app.htmx()
        .post(
            "/admin/products/actions/delete?search=tea",
            &[("ids", ""), ("all", "true")],
        )
        .await
        .assert_status(204);
    assert_eq!(Product::query().count(app.db()).await.unwrap(), 2);
    assert!(Product::find(app.db(), b.id).await.unwrap().is_none());

    app.htmx()
        .post("/admin/products/actions/nothing", &[("ids", &ids)])
        .await
        .assert_not_found();
}

#[renox::test]
async fn categories_have_no_soft_deletes_and_no_trash() {
    let app = app().await;
    let owner = user(&app, "owner@example.com").await;
    app.acting_as(&owner);
    app.post("/admin/categories", &[("name", "Drinks")])
        .await
        .assert_redirect("/admin/categories");
    let drinks = Category::query().first(app.db()).await.unwrap().unwrap();
    app.get("/admin/categories")
        .await
        .assert_see("Drinks")
        .assert_dont_see("filter=trashed");
    app.htmx()
        .post(
            "/admin/categories/actions/restore",
            &[("ids", &drinks.id.to_string())],
        )
        .await
        .assert_not_found();
    app.htmx()
        .delete(&format!("/admin/categories/{}", drinks.id))
        .await
        .assert_status(204);
    app.assert_database_count("categories", 0).await;
}

#[renox::test]
async fn the_app_replaces_pages_and_draws_custom_cells() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .module(admin().path("/backoffice").title("Shop admin"))
            .migrations(renox::migrations!("tests/migrations"))
            .templates(|env| {
                env.add_template(
                    "renox-admin/products/cells.html",
                    r#"{% if column.key == "stock" %}<b class="stock">{{ row.sku }} in stock</b>{% endif %}"#,
                )
                .unwrap();
                env.add_template(
                    "renox-admin/dashboard.html",
                    r#"{% extends "renox-admin/layout.html" %}{% block content %}<p>Hello from the app</p>{% endblock %}"#,
                )
                .unwrap();
            }),
    )
    .await;
    product(&app, "Coffee", "C-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.get("/backoffice")
        .await
        .assert_ok()
        .assert_see("Hello from the app")
        .assert_see("Shop admin");
    app.get("/backoffice/products")
        .await
        .assert_ok()
        .assert_see(r#"<b class="stock">C-1 in stock</b>"#)
        .assert_see("/backoffice/products/create");
    app.get("/admin").await.assert_not_found();
}

#[renox::test]
async fn routes_are_named_and_behind_a_login() {
    let app = app().await;
    let url =
        |name: &str, params: &[&dyn std::fmt::Display]| app.state().url(name, params).unwrap();
    assert_eq!(url("admin.dashboard", &[]), "/admin");
    assert_eq!(url("admin.products.index", &[]), "/admin/products");
    assert_eq!(url("admin.products.create", &[]), "/admin/products/create");
    assert_eq!(url("admin.products.edit", &[&7]), "/admin/products/7/edit");
    assert_eq!(url("admin.products.show", &[&7]), "/admin/products/7");
    assert_eq!(url("admin.categories.index", &[]), "/admin/categories");
}

// ---------- #260: every field kind and modifier, entries, actions, the gate ----------

/// The products again, drawn with every kind of field (the pages are only
/// shown here, not saved).
struct Specs;

impl AdminResource for Specs {
    type Model = Product;
    type Form = ProductForm;

    fn label(&self) -> &str {
        "Spec"
    }

    fn plural_label(&self) -> &str {
        "Specs"
    }

    fn slug(&self) -> &str {
        "specs"
    }

    /// Next to Products, so the group has two links.
    fn navigation_group(&self) -> Option<&str> {
        Some("Shop")
    }

    /// The policy, except that a product named "Locked" can't be deleted.
    fn allows(&self, user: &AuthUser, ability: &str, record: Option<&Product>) -> bool {
        if ability == "delete" && record.is_some_and(|p| p.name == "Locked") {
            return false;
        }
        match record {
            Some(record) => user.can(ability, record),
            None => user.can(ability, &Product::default()),
        }
    }

    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name"),
            Column::number("price", "Price"),
        ]
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::email("name", "Contact email")
                .hint("We never share it.")
                .placeholder("ana@example.com")
                .autocomplete("email"),
            Field::password("sku", "Secret"),
            Field::url("website", "Site").prefix("https://"),
            Field::tel("phone", "Phone").suffix("ext"),
            Field::textarea("notes", "Notes")
                .rows(4)
                .span_full()
                .only_on_edit(),
            Field::number("price", "Price").step("0.5").min(1).max(120),
            Field::datetime("created_at", "Created"),
            Field::checkbox("active", "Active"),
            Field::select("status", "Status", [("draft", "Draft"), ("live", "Live")]).searchable(),
            Field::text("coupon", "Coupon").only_on_create(),
        ]
    }

    fn entries(&self) -> Vec<Entry> {
        let mut entries = vec![
            Entry::text("name", "Name").copyable(),
            Entry::new("sku", "SKU").span_full(),
        ];
        entries.extend(self.columns().iter().filter_map(Entry::from_column));
        entries
    }

    fn fill(&self, product: &mut Product, form: ProductForm) {
        product.name = form.name;
    }

    fn actions(&self) -> Vec<AdminAction<Product>> {
        vec![
            AdminAction::new("archive", "Archive", |_: Vec<Product>, _cx| async {
                Ok(Toast::success("Archived."))
            })
            .danger()
            .ability("update"),
            AdminAction::new("print", "Print label", |_: Vec<Product>, _cx| async {
                Ok(Toast::success("Printed."))
            })
            .row_only(),
        ]
    }
}

async fn specs() -> (TestApp, Product) {
    let app = app_with(admin().resource(Specs)).await;
    let coffee = product(&app, "Coffee", "C-SECRET", "live").await;
    renox::db::sql("UPDATE products SET created_at = ? WHERE id = ?")
        .bind(
            renox::chrono::DateTime::parse_from_rfc3339("2026-10-05T09:30:45Z")
                .unwrap()
                .with_timezone(&renox::chrono::Utc),
        )
        .bind(coffee.id)
        .execute(app.db())
        .await
        .unwrap();
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    (app, coffee)
}

#[renox::test]
async fn every_field_kind_and_modifier_is_drawn() {
    let (app, coffee) = specs().await;
    let create = app.get("/admin/specs/create").await;
    create.assert_ok();
    let html = create.text();
    for needle in [
        r#"type="email""#,
        "We never share it.",
        r#"placeholder="ana@example.com""#,
        r#"autocomplete="email""#,
        r#"type="password""#,
        r#"type="url""#,
        "https://",
        r#"type="tel""#,
        "ext",
        r#"type="number""#,
        r#"step="0.5""#,
        r#"min="1""#,
        r#"max="120""#,
        r#"type="datetime-local""#,
        r#"type="checkbox""#,
        r#"name="coupon""#,
    ] {
        assert!(html.contains(needle), "the create page lacks {needle}");
    }
    assert!(
        !html.contains(r#"name="notes""#),
        "notes are for editing only"
    );

    let edit = app.get(&format!("/admin/specs/{}/edit", coffee.id)).await;
    edit.assert_ok();
    let html = edit.text();
    assert!(html.contains(r#"name="notes""#) && html.contains(r#"rows="4""#));
    assert!(
        !html.contains(r#"name="coupon""#),
        "the coupon is for creating only"
    );
    // A password is never filled in, whatever the record holds.
    assert!(
        !html.contains("C-SECRET"),
        "the stored value leaked into the form"
    );
    // A moment fits datetime-local: minutes, no zone.
    assert!(
        html.contains(r#"value="2026-10-05T09:30""#),
        "the date-time value"
    );
}

#[renox::test]
async fn entries_and_actions_follow_their_options() {
    let (app, coffee) = specs().await;
    let show = app.get(&format!("/admin/specs/{}", coffee.id)).await;
    show.assert_ok().assert_see("Coffee").assert_see("C-SECRET");
    let list = app.get("/admin/specs").await.text();
    // A danger action is a red bulk button; a row-only one is only in each
    // row's menu, never offered for a selection.
    assert!(list.contains(
        r#"class="rx-button rx-button--small rx-button--danger" data-grid-bulk-action data-url="/admin/specs/actions/archive""#
    ));
    assert!(list.contains(&format!(
        r#"data-url="/admin/specs/{}/actions/print""#,
        coffee.id
    )));
    assert!(!list.contains(r#"data-url="/admin/specs/actions/print""#));
    // An action the resource doesn't have is a 404.
    app.post(&format!("/admin/specs/{}/actions/nope", coffee.id), &[])
        .await
        .assert_not_found();
}

#[renox::test]
async fn a_gate_can_guard_the_panel() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .module(Admin::new().gate("back-office").resource(Products))
            .migrations(renox::migrations!("tests/migrations"))
            .gate("back-office", |user| user.email == "owner@example.com"),
    )
    .await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.get("/admin").await.assert_forbidden();
    let owner = user(&app, "owner@example.com").await;
    app.acting_as(&owner);
    app.get("/admin").await.assert_ok();
}

/// The products with the defaults: entries from the columns, no filters.
struct Plain;

impl AdminResource for Plain {
    type Model = Product;
    type Form = ProductForm;

    fn label(&self) -> &str {
        "Plain"
    }

    fn plural_label(&self) -> &str {
        "Plains"
    }

    fn slug(&self) -> &str {
        "plain"
    }

    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name"),
            Column::number("price", "Price"),
        ]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("name", "Name")]
    }

    fn fill(&self, product: &mut Product, form: ProductForm) {
        product.name = form.name;
    }
}

/// A `belongs_to` naming a table that isn't a plain name.
struct Broken;

impl AdminResource for Broken {
    type Model = Category;
    type Form = CategoryForm;

    fn label(&self) -> &str {
        "Broken"
    }

    fn plural_label(&self) -> &str {
        "Brokens"
    }

    fn slug(&self) -> &str {
        "broken"
    }

    fn columns(&self) -> Vec<Column> {
        vec![Column::text("name", "Name")]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::belongs_to(
            "parent_id",
            "Parent",
            "categories; --",
            "name",
        )]
    }

    fn fill(&self, category: &mut Category, form: CategoryForm) {
        category.name = form.name;
    }

    fn allows(&self, _user: &AuthUser, _ability: &str, _record: Option<&Category>) -> bool {
        true
    }
}

#[renox::test]
async fn every_field_kind_is_drawn_on_the_edit_page_too() {
    let (app, coffee) = specs().await;
    let html = app
        .get(&format!("/admin/specs/{}/edit", coffee.id))
        .await
        .text();
    for needle in [
        r#"type="email""#,
        r#"type="password""#,
        r#"type="url""#,
        r#"type="tel""#,
        r#"type="number""#,
        r#"type="datetime-local""#,
        r#"type="checkbox""#,
        "<textarea",
        "rx-span-full",
        "data-rx-combobox",
    ] {
        assert!(html.contains(needle), "the edit page lacks {needle}");
    }
    // Fields the record has no column for are drawn empty.
    assert!(
        html.contains(r#"name="website""#)
            && !html.contains(r#"name="website" type="url" value="h"#)
    );
    // The create-only coupon sent with an edit is ignored; the edit saves.
    app.htmx()
        .put(
            &format!("/admin/specs/{}", coffee.id),
            &[
                ("name", "Strong coffee"),
                ("sku", "C-SECRET"),
                ("price", "1"),
                ("status", "live"),
                ("coupon", "FREE"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/specs");
    let saved = Product::find(app.db(), coffee.id).await.unwrap().unwrap();
    assert_eq!(saved.name, "Strong coffee");
    // And the other way: the edit-only notes sent with a create are ignored
    // (what is read is the form, not what the page drew); the record is made.
    app.htmx()
        .post(
            "/admin/specs",
            &[
                ("name", "Green tea"),
                ("sku", "T-1"),
                ("price", "3"),
                ("status", "draft"),
                ("notes", "not on this page"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/specs");
    app.assert_database_has("products", &[("name", &"Green tea")])
        .await;
    // A form that doesn't pass: the answer is the form's (422 for htmx).
    app.htmx()
        .put(
            &format!("/admin/specs/{}", coffee.id),
            &[
                ("name", ""),
                ("sku", "C"),
                ("price", "-1"),
                ("status", "nope"),
            ],
        )
        .await
        .assert_status(422);
}

#[renox::test]
async fn dates_keep_their_day_and_bad_relations_say_why() {
    let app = app_with(admin().resource(Broken)).await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    renox::db::sql("UPDATE products SET released_on = ? WHERE id = ?")
        .bind(NaiveDate::from_ymd_opt(2026, 4, 1).unwrap())
        .bind(coffee.id)
        .execute(app.db())
        .await
        .unwrap();
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.get(&format!("/admin/products/{}/edit", coffee.id))
        .await
        .assert_see(r#"value="2026-04-01""#);
    app.get("/admin/broken/create")
        .await
        .assert_status(500)
        .assert_see("may only have letters, digits and");
}

#[renox::test]
async fn entries_show_their_options_and_default_to_the_columns() {
    let (app, coffee) = specs().await;
    let html = app.get(&format!("/admin/specs/{}", coffee.id)).await.text();
    assert!(html.contains("rx-entry__copied"), "copyable");
    assert!(html.contains("rx-entry rx-span-full"), "span_full");
    // The price column is a number: its entry too.
    assert!(
        html.contains("rx-entry--numeric") && html.contains("75,000"),
        "{html}"
    );
    assert_eq!(Entry::text("name", "Name").key(), "name");
    assert_eq!(Entry::new("sku", "SKU").key(), "sku");

    let app = app_with(admin().resource(Plain)).await;
    let tea = product(&app, "Tea", "T-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.get(&format!("/admin/plain/{}", tea.id))
        .await
        .assert_ok()
        .assert_see("Tea")
        .assert_see("75,000");
    // A filter the resource doesn't have shows everything.
    app.get("/admin/products?filter=nope")
        .await
        .assert_ok()
        .assert_see("Tea");
}

#[renox::test]
async fn actions_check_each_record_and_say_what_they_did() {
    let (app, coffee) = specs().await;
    let locked = product(&app, "Locked", "L-1", "live").await;
    let tea = product(&app, "Tea", "T-1", "live").await;
    // One of the selected records may not be deleted: nothing is.
    app.htmx()
        .post(
            "/admin/specs/actions/delete",
            &[("ids", &format!("{},{}", coffee.id, locked.id))],
        )
        .await
        .assert_forbidden();
    app.htmx()
        .post(&format!("/admin/specs/{}/actions/delete", locked.id), &[])
        .await
        .assert_forbidden();
    app.assert_database_count("products", 3).await;
    // Two deleted: the toast counts them with the plural label.
    let done = app
        .htmx()
        .post(
            "/admin/products/actions/delete",
            &[("ids", &format!("{},{}", coffee.id, tea.id))],
        )
        .await;
    done.assert_status(204);
    let trigger = done.header("hx-trigger").unwrap_or_default().to_owned();
    assert!(trigger.contains("2 products deleted."), "{trigger}");
    // Nothing matching the selection.
    let none = app
        .htmx()
        .post("/admin/products/actions/publish", &[("ids", "987654")])
        .await;
    assert!(
        none.header("hx-trigger")
            .unwrap_or_default()
            .contains("Nothing was selected."),
        "{:?}",
        none.header("hx-trigger")
    );

    // The viewer: an action needing `update`, and force-deleting from the
    // trash (forceDeleteAny), are refused.
    let viewer = user(&app, VIEWER).await;
    app.acting_as(&viewer);
    app.htmx()
        .post(
            "/admin/specs/actions/archive",
            &[("ids", &locked.id.to_string())],
        )
        .await
        .assert_forbidden();
    app.htmx()
        .post(
            "/admin/products/actions/force-delete?filter=trashed",
            &[("ids", &coffee.id.to_string())],
        )
        .await
        .assert_forbidden();
}

#[renox::test]
async fn the_gate_guards_every_page_and_groups_share_a_heading() {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .module(Admin::default().gate("back-office").resource(Products))
            .migrations(renox::migrations!("tests/migrations"))
            .gate("back-office", |user| user.email == "owner@example.com"),
    )
    .await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    for page in [
        "/admin/products".to_owned(),
        "/admin/products/create".to_owned(),
        format!("/admin/products/{}", coffee.id),
        format!("/admin/products/{}/edit", coffee.id),
    ] {
        app.get(&page).await.assert_forbidden();
    }
    let shown = format!("{:?}", Admin::default().resource(Products).path("/staff"));
    assert!(
        shown.contains("/staff") && shown.contains("products"),
        "{shown}"
    );

    let (app, _) = specs().await;
    let html = app.get("/admin").await.text();
    let shop = html.find(">Shop<").expect("the group's heading");
    let rest = &html[shop..];
    assert!(
        rest.contains("/admin/products") && rest.contains("/admin/specs"),
        "{html}"
    );
}

// --- Relation managers, input actions, hooks, slots, edit-only, texts ---

#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "variants")]
struct Variant {
    id: i64,
    product_id: i64,
    name: String,
    stock: i64,
}

impl Policy for Variant {
    fn allows(&self, user: &User, ability: &str) -> bool {
        // The viewer looks at variants; nobody changes a "locked" one.
        (matches!(ability, "viewAny" | "view") || user.email != VIEWER)
            && !(ability == "update" && self.name == "locked")
    }
}

#[derive(Deserialize, Serialize, Validate)]
struct VariantForm {
    #[validate(required)]
    product_id: i64,
    #[validate(required, max = 50)]
    name: String,
    #[validate(min = 0)]
    stock: i64,
}

struct Variants;

impl AdminResource for Variants {
    type Model = Variant;
    type Form = VariantForm;

    fn label(&self) -> &str {
        "Variant"
    }

    fn plural_label(&self) -> &str {
        "Variants"
    }

    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::number("stock", "Stock"),
        ]
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            // The manager leaves the foreign key out of the form.
            Field::belongs_to("product_id", "Product", "products", "name"),
            Field::text("name", "Name").required(),
            Field::number("stock", "Stock"),
        ]
    }

    fn entries(&self) -> Vec<Entry> {
        Vec::new()
    }

    fn fill(&self, variant: &mut Variant, form: VariantForm) {
        variant.product_id = form.product_id;
        variant.name = form.name;
        variant.stock = form.stock;
    }
}

#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "tags")]
struct Tag {
    id: i64,
    name: String,
}

impl Policy for Tag {
    fn allows(&self, _user: &User, _ability: &str) -> bool {
        true
    }
}

#[derive(Deserialize, Serialize, Validate)]
struct TagForm {
    #[validate(required)]
    name: String,
}

struct Tags;

impl AdminResource for Tags {
    type Model = Tag;
    type Form = TagForm;

    fn label(&self) -> &str {
        "Tag"
    }

    fn plural_label(&self) -> &str {
        "Tags"
    }

    fn columns(&self) -> Vec<Column> {
        vec![Column::text("name", "Name")]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("name", "Name").required()]
    }

    fn fill(&self, tag: &mut Tag, form: TagForm) {
        tag.name = form.name;
    }
}

#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "settings")]
struct Setting {
    id: i64,
    name: String,
    value: String,
}

impl Policy for Setting {
    fn allows(&self, _user: &User, _ability: &str) -> bool {
        true
    }
}

#[derive(Deserialize, Serialize, Validate)]
struct SettingForm {
    #[validate(required)]
    value: String,
}

/// Edited, never created or deleted from the panel.
struct Settings;

impl AdminResource for Settings {
    type Model = Setting;
    type Form = SettingForm;

    fn label(&self) -> &str {
        "Setting"
    }

    fn plural_label(&self) -> &str {
        "Settings"
    }

    fn creatable(&self) -> bool {
        false
    }

    fn deletable(&self) -> bool {
        false
    }

    fn columns(&self) -> Vec<Column> {
        vec![Column::text("name", "Name"), Column::text("value", "Value")]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("value", "Value").required()]
    }

    fn fill(&self, setting: &mut Setting, form: SettingForm) {
        setting.value = form.value;
    }
}

async fn full_app() -> TestApp {
    app_with(
        Admin::new()
            .title("Back office")
            .authorize(|user| user.email.ends_with("@example.com"))
            .resource(Products)
            .resource(Categories)
            .resource(Settings)
            .slot(
                Slot::AfterContent,
                r#"<aside id="about">About {{ request.route }}</aside>"#,
            )
            .slot(Slot::BeforeContent, r#"<p id="before">Before</p>"#),
    )
    .await
}

async fn notes(app: &TestApp) -> Vec<String> {
    renox::db::sql("SELECT note FROM audits ORDER BY id")
        .scalars(app.db())
        .await
        .unwrap()
}

#[renox::test]
async fn actions_can_ask_for_input_in_a_sheet() {
    let app = full_app().await;
    let a = product(&app, "Coffee", "C-1", "live").await;
    let b = product(&app, "Tea", "T-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);

    // The list has the action's sheet, and the grid opens it.
    let page = app.get("/admin/products").await;
    page.assert_ok()
        .assert_see(r#"data-sheet="rx-admin-action-reprice""#)
        .assert_see(r#"id="rx-admin-action-reprice""#)
        .assert_see("Every selected product changes by this much.")
        .assert_see("Change prices")
        .assert_see(r#"name="percent""#)
        .assert_see(r#"name="note""#);
    // An action without input still asks with the grid's dialog.
    page.assert_see(r#"data-confirm="Publish the selected products?""#);

    let ids = format!("{},{}", a.id, b.id);
    // Missing and out-of-range input stops the action.
    app.htmx()
        .post(
            "/admin/products/actions/reprice",
            &[("ids", &ids), ("percent", "")],
        )
        .await
        .assert_status(422)
        .assert_invalid("percent");
    app.htmx()
        .post(
            "/admin/products/actions/reprice",
            &[("ids", &ids), ("percent", "900")],
        )
        .await
        .assert_status(422)
        .assert_see("at most 500");
    app.htmx()
        .post(
            "/admin/products/actions/reprice",
            &[("ids", &ids), ("percent", "abc")],
        )
        .await
        .assert_invalid("percent");
    app.htmx()
        .post(
            "/admin/products/actions/reprice",
            &[("ids", &ids), ("percent", "10"), ("mode", "none")],
        )
        .await
        .assert_invalid("mode");
    assert_eq!(
        Product::find(app.db(), a.id).await.unwrap().unwrap().price,
        75_000
    );

    // A valid answer runs it on the selection, and the page reloads.
    app.htmx()
        .post(
            "/admin/products/actions/reprice",
            &[("ids", &ids), ("percent", "10"), ("note", "spring")],
        )
        .await
        .assert_ok()
        .assert_header("hx-refresh", "true");
    assert_eq!(
        Product::find(app.db(), a.id).await.unwrap().unwrap().price,
        82_500
    );
    assert_eq!(
        Product::find(app.db(), b.id).await.unwrap().unwrap().price,
        82_500
    );

    // From a row's menu.
    app.htmx()
        .post(
            &format!("/admin/products/{}/actions/reprice", a.id),
            &[("percent", "-50")],
        )
        .await
        .assert_ok();
    assert_eq!(
        Product::find(app.db(), a.id).await.unwrap().unwrap().price,
        41_250
    );
    assert_eq!(
        Product::find(app.db(), b.id).await.unwrap().unwrap().price,
        82_500
    );
    app.htmx()
        .post(&format!("/admin/products/{}/actions/reprice", a.id), &[])
        .await
        .assert_invalid("percent");

    // The viewer may not.
    let viewer = user(&app, VIEWER).await;
    app.acting_as(&viewer);
    app.htmx()
        .post(
            "/admin/products/actions/reprice",
            &[("ids", &ids), ("percent", "10")],
        )
        .await
        .assert_forbidden();
    app.get("/admin/products")
        .await
        .assert_dont_see("rx-admin-action-reprice");
}

#[renox::test]
async fn the_saved_hook_gets_the_user_and_the_record_as_it_was() {
    let app = full_app().await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.htmx()
        .post(
            "/admin/products",
            &[
                ("name", "Coffee"),
                ("sku", "c-1"),
                ("price", "7.50"),
                ("status", "live"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/products");
    let coffee = Product::query().first(app.db()).await.unwrap().unwrap();
    app.htmx()
        .put(
            &format!("/admin/products/{}", coffee.id),
            &[
                ("name", "Coffee"),
                ("sku", "c-1"),
                ("price", "9.00"),
                ("status", "live"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/products");
    assert_eq!(
        notes(&app).await,
        vec![
            "created Coffee by Ana: - -> 750".to_owned(),
            "updated Coffee by Ana: 750 -> 900".to_owned()
        ]
    );
    // A form that fails doesn't run it.
    app.htmx()
        .put(
            &format!("/admin/products/{}", coffee.id),
            &[
                ("name", ""),
                ("sku", "c-1"),
                ("price", "1"),
                ("status", "live"),
            ],
        )
        .await
        .assert_status(422);
    assert_eq!(notes(&app).await.len(), 2);
}

#[renox::test]
async fn relation_managers_are_tabs_with_their_own_lists_and_forms() {
    let app = full_app().await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    let tea = product(&app, "Tea", "T-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let base = format!("/admin/products/{}", coffee.id);

    // The tabs on the view and the edit pages.
    for page in [&base, &format!("{base}/edit")] {
        app.get(page)
            .await
            .assert_ok()
            .assert_see(&format!(r#"href="{base}/relations/variants""#))
            .assert_see(&format!(r#"href="{base}/relations/tags""#));
    }

    // A has-many: empty, then create (the foreign key is the record's even
    // when the browser sends another).
    app.get(&format!("{base}/relations/variants"))
        .await
        .assert_ok()
        .assert_view("renox-admin/relation.html")
        .assert_see("No variants yet")
        .assert_see(&format!(r#"href="{base}/relations/variants/create""#));
    let form = app.get(&format!("{base}/relations/variants/create")).await;
    form.assert_ok().assert_see("New variant");
    assert!(
        !form.text().contains(r#"name="product_id""#),
        "{}",
        form.text()
    );
    app.htmx()
        .post(
            &format!("{base}/relations/variants"),
            &[
                ("name", "Large"),
                ("stock", "4"),
                ("product_id", &tea.id.to_string()),
            ],
        )
        .await
        .assert_hx_redirect(&format!("{base}/relations/variants"));
    let large = Variant::query().first(app.db()).await.unwrap().unwrap();
    assert_eq!(
        (large.product_id, large.name.as_str(), large.stock),
        (coffee.id, "Large", 4)
    );
    app.htmx()
        .post(
            &format!("{base}/relations/variants"),
            &[("name", ""), ("stock", "1")],
        )
        .await
        .assert_invalid("name");
    app.get(&format!("{base}/relations/variants"))
        .await
        .assert_see("Large");
    // Not another product's.
    app.get(&format!("/admin/products/{}/relations/variants", tea.id))
        .await
        .assert_dont_see("Large");

    // Edit, update (re-parenting is ignored), search in the list.
    let edit = format!("{base}/relations/variants/{}", large.id);
    app.get(&format!("{edit}/edit"))
        .await
        .assert_ok()
        .assert_see("Edit Variant #")
        .assert_see(r#"value="Large""#)
        .assert_see(&format!(r#"hx-post="{edit}""#));
    app.htmx()
        .put(
            &edit,
            &[
                ("name", "XL"),
                ("stock", "9"),
                ("product_id", &tea.id.to_string()),
            ],
        )
        .await
        .assert_hx_redirect(&format!("{base}/relations/variants"));
    let xl = Variant::find(app.db(), large.id).await.unwrap().unwrap();
    assert_eq!(
        (xl.product_id, xl.name.as_str(), xl.stock),
        (coffee.id, "XL", 9)
    );
    app.get(&format!(
        "/admin/products/{}/relations/variants/{}/edit",
        tea.id, large.id
    ))
    .await
    .assert_not_found();
    app.get(&format!("{base}/relations/variants?search=xl"))
        .await
        .assert_see("XL");

    // The hook of the managed resource, a locked row, delete.
    Variant::create(
        app.db(),
        Variant {
            product_id: coffee.id,
            name: "locked".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let locked = Variant::where_eq("name", "locked")
        .first(app.db())
        .await
        .unwrap()
        .unwrap();
    app.get(&format!("{base}/relations/variants/{}/edit", locked.id))
        .await
        .assert_forbidden();
    app.htmx().delete(&edit).await.assert_status(204);
    assert!(Variant::find(app.db(), large.id).await.unwrap().is_none());
    app.htmx()
        .post(
            &format!("{base}/relations/variants/actions/remove"),
            &[("ids", &locked.id.to_string())],
        )
        .await
        .assert_status(204);
    assert_eq!(Variant::query().count(app.db()).await.unwrap(), 0);
    app.get(&format!("{base}/relations/nothing"))
        .await
        .assert_not_found();
}

#[renox::test]
async fn pivot_managers_attach_and_detach() {
    let app = full_app().await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    let mut tags = Vec::new();
    for name in ["Hot", "Cold", "Sweet"] {
        tags.push(
            Tag::create(
                app.db(),
                Tag {
                    name: name.into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap(),
        );
    }
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let url = format!("/admin/products/{}/relations/tags", coffee.id);

    let page = app.get(&url).await;
    page.assert_ok()
        .assert_see("Attach tag")
        .assert_see("No tags yet");
    // The sheet offers every tag, by name.
    for name in ["Hot", "Cold", "Sweet"] {
        page.assert_see(&format!(">{name}</option>"));
    }
    app.htmx()
        .post(&url, &[("attach", &tags[0].id.to_string())])
        .await
        .assert_ok()
        .assert_header("hx-refresh", "true");
    app.htmx()
        .post(&url, &[("attach", &tags[1].id.to_string())])
        .await
        .assert_ok();
    // Twice is once.
    app.htmx()
        .post(&url, &[("attach", &tags[1].id.to_string())])
        .await
        .assert_ok();
    app.assert_database_count("product_tag", 2).await;
    let page = app.get(&url).await;
    page.assert_see("Hot").assert_see("Cold");
    // Only the rest is offered.
    assert!(
        !page
            .text()
            .contains(&format!(r#"<option value="{}""#, tags[0].id)),
        "{}",
        page.text()
    );
    app.htmx()
        .post(&url, &[("attach", "9999")])
        .await
        .assert_status(422)
        .assert_invalid("attach");
    app.htmx().post(&url, &[]).await.assert_status(422);

    app.htmx()
        .delete(&format!("{url}/{}", tags[0].id))
        .await
        .assert_status(204);
    app.assert_database_count("product_tag", 1).await;
    assert!(Tag::find(app.db(), tags[0].id).await.unwrap().is_some());
    app.htmx()
        .post(
            &format!("{url}/actions/remove"),
            &[("ids", &tags[1].id.to_string())],
        )
        .await
        .assert_status(204);
    app.assert_database_count("product_tag", 0).await;
    // Not attached: nothing there to detach.
    app.htmx()
        .delete(&format!("{url}/{}", tags[2].id))
        .await
        .assert_not_found();

    // Changing what is attached takes the record's `update`.
    let viewer = user(&app, VIEWER).await;
    app.acting_as(&viewer);
    app.get(&url)
        .await
        .assert_ok()
        .assert_dont_see("Attach tag");
    app.htmx()
        .post(&url, &[("attach", &tags[2].id.to_string())])
        .await
        .assert_forbidden();
    app.assert_database_count("product_tag", 0).await;
    // The viewer sees variants but has no buttons for them.
    app.get(&format!("/admin/products/{}/relations/variants", coffee.id))
        .await
        .assert_ok()
        .assert_dont_see("/create");
    app.get(&format!(
        "/admin/products/{}/relations/variants/create",
        coffee.id
    ))
    .await
    .assert_forbidden();
}

#[renox::test]
async fn the_layout_has_slots_for_the_apps_content() {
    let app = full_app().await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    for page in ["/admin", "/admin/products", "/admin/products/create"] {
        let html = app.get(page).await.assert_ok().text();
        assert!(
            html.contains(r#"<p id="before">Before</p>"#),
            "{page}\n{html}"
        );
        assert!(
            html.contains(r#"<aside id="about">About admin."#),
            "{page}\n{html}"
        );
        // After the content, which is after "before".
        let (before, content, about) = (
            html.find(r#"id="before""#).unwrap(),
            html.find(r#"id="main""#).unwrap(),
            html.find(r#"id="about""#).unwrap(),
        );
        assert!(content < before && before < about, "{page}");
    }
    // Without any, nothing is drawn.
    let plain = app_with(admin()).await;
    let ana = user(&plain, "ana@example.com").await;
    plain.acting_as(&ana);
    plain
        .get("/admin")
        .await
        .assert_ok()
        .assert_dont_see("id=\"about\"");
}

#[renox::test]
async fn an_edit_only_resource_has_no_create_and_no_delete() {
    let app = full_app().await;
    let theme = Setting::create(
        app.db(),
        Setting {
            name: "theme".into(),
            value: "light".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);

    let list = app.get("/admin/settings").await;
    list.assert_ok()
        .assert_see("theme")
        .assert_see(&format!("/admin/settings/{}/edit", theme.id))
        .assert_dont_see("New setting")
        .assert_dont_see("/actions/delete");
    app.get(&format!("/admin/settings/{}/edit", theme.id))
        .await
        .assert_ok()
        .assert_dont_see("rx-admin-delete");
    app.get("/admin/settings/create").await.assert_not_found();
    app.post("/admin/settings", &[("value", "x")])
        .await
        .assert_not_found();
    app.delete(&format!("/admin/settings/{}", theme.id))
        .await
        .assert_not_found();
    app.htmx()
        .post(
            "/admin/settings/actions/delete",
            &[("ids", &theme.id.to_string())],
        )
        .await
        .assert_not_found();
    app.htmx()
        .post(&format!("/admin/settings/{}/actions/delete", theme.id), &[])
        .await
        .assert_not_found();
    app.htmx()
        .put(
            &format!("/admin/settings/{}", theme.id),
            &[("value", "dark")],
        )
        .await
        .assert_hx_redirect("/admin/settings");
    assert_eq!(
        Setting::find(app.db(), theme.id)
            .await
            .unwrap()
            .unwrap()
            .value,
        "dark"
    );
    app.assert_database_count("settings", 1).await;
}

#[renox::test]
async fn the_panels_own_words_follow_the_apps_translations() {
    let app = TestApp::with_config(
        App::new()
            .module(Auth::new())
            .module(admin())
            .migrations(renox::migrations!("tests/migrations")),
        |c| {
            c.locale = "es".into();
            c.lang_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lang");
        },
    )
    .await;
    let coffee = product(&app, "Coffee", "C-1", "live").await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);

    app.get("/admin/products")
        .await
        .assert_ok()
        // Heading, button, tab, column names, action names, row actions.
        .assert_see("Productos")
        .assert_see("Nuevo producto")
        .assert_see("Todos")
        .assert_see("Nombre")
        .assert_see("Precio")
        .assert_see("Publicar")
        .assert_see("Eliminar")
        .assert_see("Editar")
        .assert_see("Ver")
        .assert_dont_see(">Name<");
    app.get("/admin").await.assert_see("Panel");
    app.get(&format!("/admin/products/{}", coffee.id))
        .await
        .assert_see(&format!("Producto n.º {}", coffee.id));
    app.get(&format!("/admin/products/{}/edit", coffee.id))
        .await
        .assert_see(&format!("Editar Producto n.º {}", coffee.id))
        .assert_see("Guardar cambios");
    app.get("/admin/products/create")
        .await
        .assert_see("Nuevo producto")
        .assert_see("Crear producto");
    // Without a translation, the English text stays.
    app.get("/admin/products/create").await.assert_see("Cancel");
    // Toasts too.
    app.htmx()
        .put(
            &format!("/admin/products/{}", coffee.id),
            &[
                ("name", "Coffee"),
                ("sku", "c-1"),
                ("price", "1"),
                ("status", "live"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/products");
    let toast = app.get("/admin/products").await.text();
    assert!(toast.contains("Producto guardado."), "{toast}");
}
