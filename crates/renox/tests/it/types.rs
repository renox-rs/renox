//! Every supported type from a browser form to the database and back, on
//! SQLite and (with TEST_DATABASE_URL) PostgreSQL.

use renox::chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use renox::db::Json;
use renox::prelude::*;
use renox::testing::TestApp;
use serde::{Deserialize, Serialize};

#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Default)]
enum Status {
    #[default]
    Draft,
    InStock,
    #[db(rename = "gone")]
    SoldOut,
}

#[derive(Model, Serialize, Default, Debug, Clone, PartialEq)]
#[model(table = "items")]
struct Item {
    id: i64,
    name: String,
    quantity: i64,
    weight: f64,
    active: bool,
    status: Status,
    tags: Json<Vec<String>>,
    extra: Option<renox::serde_json::Value>,
    opens_at: Option<NaiveTime>,
    starts_at: Option<NaiveDateTime>,
    released_on: Option<NaiveDate>,
    thumbnail: Option<Vec<u8>>,
    created_at: Option<DateTime>,
    updated_at: Option<DateTime>,
}

/// What an HTML form with every input type sends.
#[derive(Deserialize, Serialize)]
struct ItemForm {
    name: String,   // <input>
    quantity: i64,  // <input type="number">
    weight: f64,    // <input type="number" step="0.01">
    active: bool,   // <input type="checkbox"> (sends "on", or nothing)
    status: Status, // <select>
    #[serde(default)]
    tags: Vec<String>, // <select multiple> / checkboxes named tags
    opens_at: Option<NaiveTime>, // <input type="time">
    starts_at: Option<NaiveDateTime>, // <input type="datetime-local">
    released_on: Option<NaiveDate>, // <input type="date">
}

impl Validate for ItemForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required();
    }
}

struct Shop;

impl Module for Shop {
    fn name(&self) -> &'static str {
        "shop"
    }

    fn routes(&self) -> Routes {
        Routes::new().post(
            "/items",
            |State(db): State<Db>, Valid(form): Valid<ItemForm>| async move {
                let item = Item {
                    name: form.name,
                    quantity: form.quantity,
                    weight: form.weight,
                    active: form.active,
                    status: form.status,
                    tags: Json(form.tags),
                    opens_at: form.opens_at,
                    starts_at: form.starts_at,
                    released_on: form.released_on,
                    ..Default::default()
                };
                Ok::<_, Error>(Item::create(&db, item).await?.id.to_string())
            },
        )
    }
}

async fn app() -> TestApp {
    TestApp::new(
        App::new()
            .migrations(renox::migrations!("tests/migrations_types"))
            .module(Shop),
    )
    .await
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[renox::test]
async fn every_type_round_trips_through_the_database() {
    let app = app().await;
    let item = Item {
        name: "Coffee".into(),
        quantity: 3,
        weight: 0.25,
        active: true,
        status: Status::SoldOut,
        tags: Json(vec!["hot".into(), "sweet".into()]),
        extra: Some(json!({ "origin": "Kenya", "grade": 1 })),
        opens_at: NaiveTime::from_hms_opt(7, 30, 0),
        starts_at: date(2026, 10, 1).and_hms_opt(9, 15, 0),
        released_on: Some(date(2026, 9, 27)),
        thumbnail: Some(vec![0, 1, 2, 255]),
        ..Default::default()
    };
    let saved = Item::create(app.db(), item).await.unwrap();
    let loaded = Item::find(app.db(), saved.id).await.unwrap().unwrap();
    assert_eq!(loaded, saved);

    // The enum is stored as its text, and can be queried by value.
    let stored: String = renox::db::sql("SELECT status FROM items")
        .scalar(app.db())
        .await
        .unwrap();
    assert_eq!(stored, "gone");
    assert_eq!(
        Item::where_eq("status", Status::SoldOut)
            .count(app.db())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Item::where_eq("active", true)
            .count(app.db())
            .await
            .unwrap(),
        1
    );
    // Enums serialize as their text for templates and JSON.
    assert_eq!(
        renox::serde_json::to_value(&loaded).unwrap()["status"],
        "gone"
    );
    assert_eq!(
        renox::serde_json::to_value(&loaded).unwrap()["tags"],
        json!(["hot", "sweet"])
    );
    assert_eq!(
        Status::ALL,
        [Status::Draft, Status::InStock, Status::SoldOut]
    );
    assert_eq!("in_stock".parse::<Status>(), Ok(Status::InStock));
    assert!(
        "nope"
            .parse::<Status>()
            .unwrap_err()
            .contains("expected one of: draft, in_stock, gone")
    );
}

#[renox::test]
async fn forms_send_every_input_type() {
    let app = app().await;
    // Exactly what a browser sends: checkbox "on", datetime-local without
    // seconds, a multi-select as repeated names, an enum from a <select>.
    let res = app
        .post(
            "/items",
            &[
                ("name", "Tea"),
                ("quantity", "2"),
                ("weight", "1.5"),
                ("active", "on"),
                ("status", "in_stock"),
                ("tags", "cold"),
                ("tags", "sweet"),
                ("opens_at", "08:00"),
                ("starts_at", "2026-10-01T10:30"),
                ("released_on", "2026-09-30"),
            ],
        )
        .await;
    res.assert_ok();
    let item = Item::find(app.db(), res.text().parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(item.active);
    assert_eq!(item.status, Status::InStock);
    assert_eq!(*item.tags, ["cold", "sweet"]);
    assert_eq!(item.weight, 1.5);
    assert_eq!(item.opens_at, NaiveTime::from_hms_opt(8, 0, 0));
    assert_eq!(item.starts_at, date(2026, 10, 1).and_hms_opt(10, 30, 0));

    // An unchecked checkbox and an empty multi-select send nothing.
    let res = app
        .post(
            "/items",
            &[
                ("name", "Water"),
                ("quantity", "1"),
                ("weight", "1"),
                ("status", "draft"),
            ],
        )
        .await;
    res.assert_ok();
    let item = Item::find(app.db(), res.text().parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(!item.active);
    assert!(item.tags.is_empty());

    // A value that isn't one of the enum's is a validation error, not a 400.
    app.htmx()
        .post(
            "/items",
            &[
                ("name", "X"),
                ("quantity", "1"),
                ("weight", "1"),
                ("status", "lost"),
            ],
        )
        .await
        .assert_invalid("status");
}

#[derive(Deserialize, Validate)]
struct Toggle {
    #[serde(default)]
    active: bool,
}

struct Toggles;

impl Module for Toggles {
    fn name(&self) -> &'static str {
        "toggles"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .post("/form", |Form(t): Form<Toggle>| async move {
                t.active.to_string()
            })
            .post("/valid", |Valid(t): Valid<Toggle>| async move {
                Ok::<_, Error>(t.active.to_string())
            })
    }
}

/// A ticked checkbox sends `on`: `Valid<T>` reads it as `true` even without
/// rules, while the prelude's `Form<T>` (axum's) refuses it, as docs/validation.md
/// "Browser values" says (#174).
#[renox::test]
async fn only_valid_reads_a_ticked_checkbox() {
    let app = TestApp::new(App::new().module(Toggles)).await;

    let res = app.post("/valid", &[("active", "on")]).await;
    res.assert_ok();
    assert_eq!(res.text(), "true");
    assert_eq!(app.post("/valid", &[]).await.text(), "false");

    app.post("/form", &[("active", "on")])
        .await
        .assert_status(422);
}
