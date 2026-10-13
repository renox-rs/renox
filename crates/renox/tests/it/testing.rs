//! Renox's own test helpers, used the way an app would use them.

use renox::prelude::*;
use renox::testing::TestApp;
use serde::{Deserialize, Serialize};

#[derive(Model, Serialize, Deserialize, Default)]
#[model(table = "products", soft_deletes)]
struct Product {
    id: i64,
    name: String,
    price: i64,
    category: Option<String>,
    created_at: Option<DateTime>,
    updated_at: Option<DateTime>,
    deleted_at: Option<DateTime>,
}

#[derive(Deserialize, Serialize)]
struct ProductForm {
    name: String,
    price: i64,
}

impl Validate for ProductForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required();
        v.field("price", &self.price).min(1000);
    }
}

#[derive(Serialize, Deserialize)]
struct CountStock;

impl Job for CountStock {
    const NAME: &'static str = "count-stock";

    async fn handle(self, _: JobContext) -> Result {
        Ok(())
    }
}

struct Shop;

impl Module for Shop {
    fn name(&self) -> &'static str {
        "shop"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { view("shop/home.html", ()) })
            .name("home")
            .post("/products", store)
            .post("/stock", |State(state): State<AppState>| async move {
                state.dispatch(CountStock).await?;
                state
                    .queue_mail(renox::mail::Mail::new(
                        "warehouse@shop.test",
                        "Stock counted",
                        "ok",
                    ))
                    .await?;
                Ok::<_, Error>("queued")
            })
            .merge(
                Routes::new()
                    .get("/dashboard", |auth: AuthUser| async move {
                        format!("dashboard {}", auth.name)
                    })
                    .require_auth(),
            )
    }

    fn register(&self, app: &mut Registry) {
        app.job::<CountStock>();
    }
}

async fn store(State(db): State<Db>, Valid(form): Valid<ProductForm>) -> Result<Redirect> {
    Product::create(
        &db,
        Product {
            name: form.name,
            price: form.price,
            ..Default::default()
        },
    )
    .await?;
    Ok(Redirect::to("/"))
}

fn app() -> App {
    App::new()
        .migrations(renox::migrations!("tests/migrations"))
        .module(Auth::new())
        .module(Shop)
}

async fn test_app() -> TestApp {
    TestApp::with_config(app(), |c| c.views_path = "tests/views".into()).await
}

#[renox::test]
async fn pages_forms_and_the_database() {
    let app = test_app().await;
    app.get("/")
        .await
        .assert_ok()
        .assert_see("<h1>Shop</h1>")
        .assert_dont_see("Hello");

    app.post("/products", &[("name", "Coffee"), ("price", "18000")])
        .await
        .assert_redirect("/");
    app.assert_database_has("products", &[("name", &"Coffee"), ("price", &18000)])
        .await;
    app.assert_database_missing("products", &[("name", &"Tea")])
        .await;
    app.assert_database_count("products", 1).await;
    app.assert_database_has("products", &[("category", &None::<String>)])
        .await;
}

#[renox::test]
async fn csrf_and_validation() {
    let app = test_app().await;
    app.request()
        .without_csrf()
        .post("/products", &[("name", "Coffee"), ("price", "18000")])
        .await
        .assert_status(419);

    app.htmx()
        .post("/products", &[("name", ""), ("price", "500")])
        .await
        .assert_invalid("name")
        .assert_invalid("price");
    app.request()
        .json()
        .post("/products", &[("price", "10")])
        .await
        .assert_invalid("price");
    app.post_json(
        "/products",
        &serde_json::json!({ "name": "Tea", "price": 5000 }),
    )
    .await
    .assert_redirect("/");
    app.assert_database_count("products", 1).await;
}

#[renox::test]
async fn acting_as_a_user() {
    let app = test_app().await;
    let user = User::register(app.db(), "Alex", "alex@example.com", "letmein123")
        .await
        .unwrap();

    app.get("/dashboard").await.assert_redirect("/login");
    app.request()
        .json()
        .get("/dashboard")
        .await
        .assert_unauthorized();

    app.acting_as(&user)
        .get("/dashboard")
        .await
        .assert_ok()
        .assert_see("dashboard Alex");
    app.get("/").await.assert_see("Hello Alex");
    app.logout()
        .get("/dashboard")
        .await
        .assert_redirect("/login");
}

#[renox::test]
async fn queue_and_mail_fakes() {
    let app = test_app().await;
    app.post("/stock", &[])
        .await
        .assert_ok()
        .assert_see("queued");
    assert_eq!(app.queued_jobs().await, ["count-stock", "renox.send-mail"]);
    assert!(app.sent_mail().is_empty());
    assert_eq!(app.run_jobs().await, 2);
    app.assert_mail_sent("warehouse@shop.test", "Stock");
}

#[renox::test]
#[should_panic(expected = "expected to see \"Price\" in:")]
async fn failed_assertions_explain_themselves() {
    let app = test_app().await;
    app.get("/").await.assert_see("Price");
}
