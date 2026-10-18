//! `Field::quantity(..).scale(..).unit_from(..)` and `Entry::unit_from(..)`:
//! each record's exact quantity with its own unit in the list, the form and
//! the view page.

use renox::grid::Column;
use renox::prelude::*;
use renox::quantity::Quantity;
use renox::testing::TestApp;
use renox_admin::{Admin, AdminResource, Entry, Field};
use serde::{Deserialize, Serialize};

type Q3 = Quantity<3>;

#[derive(Model, Serialize, Deserialize, Debug, Clone)]
#[model(table = "stock_items")]
struct StockItem {
    id: i64,
    stock: Q3,
    stock_unit: String,
}

impl Default for StockItem {
    fn default() -> Self {
        Self {
            id: 0,
            stock: Q3::ZERO,
            stock_unit: String::new(),
        }
    }
}

impl Policy for StockItem {
    fn allows(&self, _user: &User, _ability: &str) -> bool {
        true
    }
}

#[derive(Deserialize, Serialize, Validate)]
struct StockItemForm {
    #[validate(required, quantity(3))]
    stock: String,
    #[validate(required)]
    stock_unit: String,
}

struct StockItems;

impl AdminResource for StockItems {
    type Model = StockItem;
    type Form = StockItemForm;

    fn label(&self) -> &str {
        "Stock item"
    }

    fn plural_label(&self) -> &str {
        "Stock items"
    }

    fn columns(&self) -> Vec<Column> {
        vec![
            Column::quantity("stock", "Stock")
                .scale(3)
                .unit_from("stock_unit"),
        ]
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::quantity("stock", "Stock")
                .required()
                .scale(3)
                .unit_from("stock_unit"),
            Field::select("stock_unit", "Unit", [("kg", "kg"), ("g", "g")])
                .default_value("g")
                .required(),
        ]
    }

    fn entries(&self) -> Vec<Entry> {
        self.columns()
            .iter()
            .filter_map(Entry::from_column)
            .collect()
    }

    fn fill(&self, item: &mut StockItem, form: StockItemForm) {
        item.stock = Q3::parse_exact(&form.stock).unwrap_or(Q3::ZERO);
        item.stock_unit = form.stock_unit;
    }
}

async fn app() -> TestApp {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .module(
                Admin::new()
                    .authorize(|user| user.email.ends_with("@example.com"))
                    .resource(StockItems),
            )
            .migrations(renox::migrations!("tests/migrations")),
    )
    .await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "secret-password-1")
        .await
        .unwrap();
    app.acting_as(&ana);
    app
}

async fn item(app: &TestApp, stock: &str, unit: &str) -> StockItem {
    StockItem::create(
        app.db(),
        StockItem {
            id: 0,
            stock: Q3::parse_exact(stock).unwrap(),
            stock_unit: unit.into(),
        },
    )
    .await
    .unwrap()
}

async fn last(app: &TestApp) -> StockItem {
    StockItem::query()
        .order_by_desc("id")
        .first(app.db())
        .await
        .unwrap()
        .unwrap()
}

#[renox::test]
async fn the_list_and_the_view_page_show_each_record_with_its_unit() {
    let app = app().await;
    let kilos = item(&app, "2.5", "kg").await;
    let grams = item(&app, "1500", "g").await;
    app.get("/admin/stock_items")
        .await
        .assert_ok()
        .assert_see("2.500")
        .assert_see("kg")
        .assert_see("1,500.000");
    app.get(&format!("/admin/stock_items/{}", kilos.id))
        .await
        .assert_ok()
        .assert_see("2.500 kg")
        .assert_dont_see("1,500.000");
    app.get(&format!("/admin/stock_items/{}", grams.id))
        .await
        .assert_ok()
        .assert_see("1,500.000 g");
}

#[renox::test]
async fn the_form_shows_the_quantity_with_its_scale_and_unit() {
    let app = app().await;
    let kilos = item(&app, "2.5", "kg").await;
    app.get(&format!("/admin/stock_items/{}/edit", kilos.id))
        .await
        .assert_ok()
        .assert_see(r#"value="2.500""#)
        .assert_see(r#"step="0.001""#)
        .assert_see(r#"inputmode="decimal""#)
        .assert_see(">kg</span>");
    // The create page starts with the unit field's default (g).
    app.get("/admin/stock_items/create")
        .await
        .assert_ok()
        .assert_see(r#"step="0.001""#)
        .assert_see(">g</span>");
}

#[renox::test]
async fn the_quantity_is_saved_exactly() {
    let app = app().await;
    app.htmx()
        .post(
            "/admin/stock_items",
            &[("stock", "12.345"), ("stock_unit", "kg")],
        )
        .await
        .assert_hx_redirect("/admin/stock_items");
    let saved = last(&app).await;
    assert_eq!(saved.stock, Q3::parse_exact("12.345").unwrap());
    assert_eq!(saved.stock_unit, "kg");
    // JSON too.
    app.post_json(
        "/admin/stock_items",
        &renox::serde_json::json!({ "stock": "0.001", "stock_unit": "g" }),
    )
    .await;
    assert_eq!(last(&app).await.stock, Q3::parse_exact("0.001").unwrap());
}
