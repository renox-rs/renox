//! Actions beyond one step (#153, Filament's as the yardstick): action
//! groups, an action that opens a wizard, and the replicate, import and
//! export actions (`Model::replicate`, `renox::import`, `Grid::export_as`).

use renox::HxRefresh;
use renox::grid::{Column, ExportFormat, Grid, GridRequest};
use renox::import::{Import, ImportReport};
use renox::prelude::*;
use renox::testing::TestApp;
use serde::{Deserialize, Serialize};

#[derive(Model, Serialize, Deserialize, Default, Clone, Debug)]
#[model(table = "items", soft_deletes)]
struct Item {
    id: i64,
    name: String,
    stock: i64,
    created_at: Option<DateTime>,
    updated_at: Option<DateTime>,
    deleted_at: Option<DateTime>,
}

/// The "new item" form, and one row of an import.
#[derive(Deserialize, Validate)]
struct ItemForm {
    #[validate(required, max = 20, unique("items", "name"))]
    name: String,
    #[validate(required, min = 0)]
    stock: i64,
}

#[derive(Deserialize, Validate)]
struct ImportForm {
    #[validate(required)]
    file: Option<Upload>,
}

async fn store(State(db): State<Db>, Valid(form): Valid<ItemForm>) -> Result<(Toast, HxRefresh)> {
    let item = Item::create(
        &db,
        Item {
            name: form.name,
            stock: form.stock,
            ..Default::default()
        },
    )
    .await?;
    Ok((Toast::success(format!("{} added.", item.name)), HxRefresh))
}

fn import_items(file: &Upload) -> Import {
    Import::csv(file.bytes())
}

async fn write(tx: &mut renox::db::Transaction, row: ItemForm) -> Result {
    if row.name == "refused" {
        return Err(Error::BadRequest("Not this one.".into()));
    }
    Item::create(
        tx,
        Item {
            name: row.name,
            stock: row.stock,
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

async fn import(
    State(state): State<AppState>,
    lang: Lang,
    Valid(form): Valid<ImportForm>,
) -> Result<ImportReport> {
    let file = form.file.ok_or(Error::NotFound)?;
    import_items(&file)
        .lang(&lang)
        .run(&state, |tx, row: ItemForm| Box::pin(write(tx, row)))
        .await
}

async fn import_strict(
    State(state): State<AppState>,
    Valid(form): Valid<ImportForm>,
) -> Result<ImportReport> {
    let file = form.file.ok_or(Error::NotFound)?;
    import_items(&file)
        .all_or_nothing()
        .delimiter(';')
        .rename("Item name", "name")
        .run(&state, |tx, row: ItemForm| Box::pin(write(tx, row)))
        .await
}

fn grid() -> Grid {
    Grid::new("items")
        .column(Column::text("name", "Name").searchable())
        .column(Column::number("stock", "Stock"))
        .column(Column::number("id", "Number").hidden())
}

async fn export(Path(format): Path<ExportFormat>, request: GridRequest) -> Result<Response> {
    grid()
        .export_as(Item::query().order_by_desc("stock"), format, &request)
        .await
}

async fn workbook(request: GridRequest) -> Result<Response> {
    renox::grid::Workbook::new("items")
        .sheet(
            "By stock",
            &grid(),
            Item::query().order_by_desc("stock"),
            &request,
        )
        .await?
        .sheet("By name", &grid(), Item::query().order_by("name"), &request)
        .await?
        .into_response()
}

async fn bad_sheet(request: GridRequest) -> Result<Response> {
    renox::grid::Workbook::new("items")
        .sheet("Bad/name", &grid(), Item::query(), &request)
        .await?
        .into_response()
}

async fn copy(State(db): State<Db>, Path(id): Path<i64>) -> Result<Json<Item>> {
    let item = Item::find_or_404(&db, id).await?;
    Ok(Json(item.replicate()))
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "action-kinds"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/items", || async { view("items.html", context! {}) })
            .post("/items", store)
            .post("/items/import", import)
            .post("/items/import-strict", import_strict)
            .get("/items/template.csv", || async {
                renox::import::template("items.csv", &["name", "stock"])
            })
            .get("/items/export/{format}", export)
            .get("/items.xlsx", workbook)
            .get("/items-bad.xlsx", bad_sheet)
            .get("/items/{id}/copy", copy)
    }
}

const ITEMS: &str = r##"{% from "renox/ui.html" import action_group, menu_link, menu_open, menu_action, menu_section, action_sheet, confirm, wizard_action, wizard_step, import_action, input %}
{% call action_group() %}
{{ menu_link("/items/7/copy", "Duplicate", icon="copy") }}
{{ menu_open("adjust-7", "Adjust stock", icon="edit") }}
{{ menu_link("/items/export/csv", "Export", icon="download", download=true) }}
{% call menu_section("Danger zone") %}{{ menu_open("delete-7", "Delete", icon="trash", danger=true) }}{% endcall %}
{% endcall %}
{% call action_group("More", id="more-7", icon="settings") %}{{ menu_action("/items/7/archive", "Archive") }}{% endcall %}
{% call action_sheet("adjust-7", "Adjust stock", "/items/7/stock", "Adjust stock", button=false) %}{{ input("change", "Change", type="number") }}{% endcall %}
{{ confirm("delete-7", "Delete", "/items/7", "Delete it?", "Gone for good.", button=false) }}
{% call wizard_action("new-item", "New item", "/items", "New item", [["name", "Name"], ["stock", "Stock"]], submit_label="Add item", variant="primary") %}
{% call wizard_step("new-item", "name") %}{{ input("name", "Name", required=true) }}{% endcall %}
{% call wizard_step("new-item", "stock") %}{{ input("stock", "Stock", type="number", required=true) }}{% endcall %}
{% endcall %}
{{ import_action("import-items", "Import", "/items/import", "Import items", columns=["name", "stock"], template_url="/items/template.csv") }}
{{ toasts() }}"##;

const TABLE_SQLITE: &str = "CREATE TABLE items (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE, stock BIGINT NOT NULL, created_at TEXT, updated_at TEXT, deleted_at TEXT)";
const TABLE_POSTGRES: &str = "CREATE TABLE items (id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, name TEXT NOT NULL UNIQUE, stock BIGINT NOT NULL, created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, deleted_at TIMESTAMPTZ)";

async fn app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("items.html"), ITEMS).unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pages), move |c| {
        c.views_path = path;
    })
    .await;
    let table = match app.db().dialect() {
        renox::db::Dialect::Postgres => TABLE_POSTGRES,
        _ => TABLE_SQLITE,
    };
    renox::db::sql(table).execute(app.db()).await.unwrap();
    (app, dir)
}

async fn names(app: &TestApp) -> Vec<String> {
    Item::query()
        .order_by("id")
        .get(app.db())
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.name)
        .collect()
}

#[renox::test]
async fn an_action_group_is_a_menu_whose_items_open_sheets() {
    let (app, _dir) = app().await;
    let page = app.get("/items").await;
    page.assert_ok()
        // No label: an icon button named "Actions".
        .assert_see(r#"<div class="rx-menu rx-action-group" data-rx-menu>"#)
        .assert_see(r#"<button class="rx-icon-button rx-icon-button--plain rx-icon-button--small" type="button" aria-label="Actions" data-rx-tip="Actions" aria-haspopup="menu" aria-expanded="false" aria-controls="rx-actions-actions"><svg"#)
        .assert_see(r#"<div class="rx-menu__list" id="rx-actions-actions" role="menu" aria-label="Actions" hidden>"#)
        // Items with icons: a link, a download, one that opens a sheet.
        .assert_see(r#"<a class="rx-menu__item" role="menuitem" href="/items/7/copy"><span class="rx-menu__icon" aria-hidden="true"><svg"#)
        .assert_see(r#"<a class="rx-menu__item" role="menuitem" href="/items/export/csv" download><span class="rx-menu__icon""#)
        .assert_see(r#"<button class="rx-menu__item" role="menuitem" type="button" data-rx-open="adjust-7" aria-haspopup="dialog"><span class="rx-menu__icon""#)
        .assert_see(r#"<div class="rx-menu__section" role="group" aria-label="Danger zone"><div class="rx-menu__heading" aria-hidden="true">Danger zone</div><button class="rx-menu__item rx-menu__item--danger" role="menuitem" type="button" data-rx-open="delete-7""#)
        // A label: a button with its icon and a chevron.
        .assert_see(r#"aria-controls="more-7"><span class="rx-button__icon" aria-hidden="true"><svg"#)
        .assert_see(r#"<span class="rx-button__label">More</span><span class="rx-button__chevron""#)
        // The sheets the items open, without buttons of their own.
        .assert_see(r#"<dialog class="rx-sheet" id="adjust-7""#)
        .assert_see(r#"<dialog class="rx-sheet" id="delete-7""#);
    let body = page.text();
    assert!(
        !body.contains(r#"data-rx-open="adjust-7" aria-haspopup="dialog"><span class="rx-button"#)
    );
    assert_eq!(body.matches(r#"data-rx-open="delete-7""#).count(), 1);
}

#[renox::test]
async fn a_wizard_action_is_a_wizard_in_a_sheet_sent_with_htmx() {
    let (app, _dir) = app().await;
    app.get("/items")
        .await
        .assert_see(r#"<button class="rx-button rx-button--primary" type="button" data-rx-open="new-item" aria-haspopup="dialog"><span class="rx-button__label">New item</span></button>"#)
        .assert_see(r#"<dialog class="rx-sheet rx-sheet--wizard rx-sheet--lg" id="new-item" aria-labelledby="new-item-title" data-rx-component="wizard-action">"#)
        .assert_see(r#"<form class="rx-sheet__inner" method="post" action="/items" hx-post="/items" hx-swap="none" data-live-validate data-rx-action novalidate>"#)
        .assert_see(r#"<div class="rx-wizard" id="new-item-wizard" data-rx-wizard data-rx-component="wizard">"#)
        .assert_see(r#"<section class="rx-wizard__panel" id="new-item-step-stock" data-rx-step="stock">"#)
        // Cancel closes the sheet; the last step's button sends the form.
        .assert_see(r#"<button class="rx-button rx-button--plain" type="button" data-rx-close><span class="rx-button__label">Cancel</span></button>"#)
        .assert_see(r#"type="submit" data-rx-wizard-submit><span class="rx-button__label">Add item</span></button>"#);
    // A 422 names the step-2 field (renox-ui.js opens that step).
    app.htmx()
        .post("/items", &[("name", "Coffee"), ("stock", "-1")])
        .await
        .assert_status(422)
        .assert_json_path("errors.stock.0", "The stock must be at least 0.");
    app.htmx()
        .post("/items", &[("name", "Coffee"), ("stock", "4")])
        .await
        .assert_ok()
        .assert_header("hx-refresh", "true");
    assert_eq!(names(&app).await, ["Coffee"]);
}

#[renox::test]
async fn an_import_action_is_a_file_field_in_a_sheet() {
    let (app, _dir) = app().await;
    app.get("/items")
        .await
        .assert_see(r#"data-rx-open="import-items" aria-haspopup="dialog"><span class="rx-button__icon""#)
        .assert_see(r##"hx-post="/items/import" hx-swap="innerHTML" hx-target="#import-items-result" enctype="multipart/form-data" hx-encoding="multipart/form-data" data-rx-action novalidate>"##)
        .assert_see(r#"Columns: <code>name</code>, <code>stock</code>"#)
        .assert_see(r#"<a class="rx-link" href="/items/template.csv" download>"#)
        .assert_see(r#"name="file""#)
        .assert_see(r#"accept=".csv,text/csv""#)
        .assert_see(r#"<div class="rx-import__result" id="import-items-result" data-rx-action-result aria-live="polite"></div>"#);
    let template = app.get("/items/template.csv").await;
    template.assert_ok();
    assert!(
        template
            .header("content-disposition")
            .unwrap()
            .starts_with("attachment; filename=\"items.csv\"")
    );
    assert_eq!(template.text(), "\u{feff}name,stock\r\n");
}

#[renox::test]
async fn an_import_checks_every_row_and_writes_the_good_ones() {
    let (app, _dir) = app().await;
    Item::create(
        app.db(),
        Item {
            name: "Tea".into(),
            stock: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let csv = "\u{feff}Name , stock\r\nCoffee,4\n\"Milk, oat\",2\n,3\nSugar,lots\nTea,9\n\nCoffee,1\nrefused,1\n";
    let res = app
        .htmx()
        .post_multipart(
            "/items/import",
            &[],
            &[("file", "items.csv", csv.as_bytes())],
        )
        .await;
    // Some rows refused: the report, which keeps the sheet open and
    // reloads the page when it closes.
    res.assert_ok()
        .assert_see(r#"<div class="rx-import-report" data-rx-keep-open data-rx-refresh-on-close>"#)
        .assert_see("2 imported, 5 rows left out.")
        .assert_see(r#"<td class="rx-num">4</td><td>The name field is required.</td>"#)
        .assert_see(r#"<td class="rx-num">5</td><td>The stock must be a number.</td>"#)
        .assert_see(r#"<td class="rx-num">6</td><td>The name has already been taken.</td>"#)
        // A blank line is skipped, not counted; a repeat within the file
        // fails at the database's unique index.
        .assert_see(r#"<td class="rx-num">8</td><td>A value of this row must be unique, and is already taken.</td>"#)
        .assert_see(r#"<td class="rx-num">9</td><td>Not this one.</td>"#);
    assert_eq!(names(&app).await, ["Tea", "Coffee", "Milk, oat"]);

    // Every row good: a toast and a refresh, which closes the sheet.
    let res = app
        .htmx()
        .post_multipart(
            "/items/import",
            &[],
            &[("file", "more.csv", b"name,stock\nSalt,1\nPepper,2\n")],
        )
        .await;
    res.assert_ok().assert_header("hx-refresh", "true");
    assert_eq!(res.text(), "");
    app.get("/items").await.assert_see("2 rows imported.");
    assert_eq!(names(&app).await.len(), 5);
}

#[renox::test]
async fn an_unreadable_file_is_the_file_fields_error() {
    let (app, _dir) = app().await;
    for (bytes, message) in [
        (&b"\xff\xfe\x00"[..], "The file must be CSV text (UTF-8)."),
        (&b"name,stock\n\n"[..], "The file has no rows to import."),
        (&b""[..], "The file has no rows to import."),
    ] {
        app.htmx()
            .post_multipart("/items/import", &[], &[("file", "x.csv", bytes)])
            .await
            .assert_status(422)
            .assert_json_path("errors.file.0", message);
    }
}

#[renox::test]
async fn all_or_nothing_writes_nothing_when_a_row_fails() {
    let (app, _dir) = app().await;
    // `;` between cells, a heading renamed to the field.
    let bad = b"Item name;stock\nCoffee;1\nTea;-2\n";
    app.htmx()
        .post_multipart("/items/import-strict", &[], &[("file", "a.csv", bad)])
        .await
        .assert_ok()
        .assert_see(r#"<div class="rx-import-report" data-rx-keep-open>"#)
        .assert_see("Nothing was imported: one row has errors.")
        .assert_see(r#"<td class="rx-num">3</td><td>The stock must be at least 0.</td>"#);
    assert!(names(&app).await.is_empty());
    // Valid rows, but the database refuses one: everything rolls back.
    let refused = b"Item name;stock\nCoffee;1\nrefused;2\n";
    app.htmx()
        .post_multipart("/items/import-strict", &[], &[("file", "b.csv", refused)])
        .await
        .assert_see("Nothing was imported: one row has errors.")
        .assert_see("Not this one.");
    assert!(names(&app).await.is_empty());
    let good = b"Item name;stock\nCoffee;1\nTea;2\n";
    app.htmx()
        .post_multipart("/items/import-strict", &[], &[("file", "c.csv", good)])
        .await
        .assert_header("hx-refresh", "true");
    assert_eq!(names(&app).await, ["Coffee", "Tea"]);
}

#[renox::test]
async fn too_many_rows_are_refused_before_anything_is_written() {
    let (app, _dir) = app().await;
    let state = app.state().clone();
    let err = Import::csv("name,stock\nA,1\nB,2\nC,3\n")
        .max_rows(2)
        .run(&state, |tx, row: ItemForm| Box::pin(write(tx, row)))
        .await
        .unwrap_err();
    match err {
        Error::Validation(invalid) => {
            assert_eq!(
                invalid.errors.first("file"),
                Some("The file has more than 2 rows.")
            )
        }
        other => panic!("{other:?}"),
    }
    // A file without a line of column names.
    let report = Import::csv("A,1\nB,2\n")
        .headers(&["name", "stock"])
        .run(&state, |tx, row: ItemForm| Box::pin(write(tx, row)))
        .await
        .unwrap();
    assert!(report.is_clean());
    assert_eq!(report.imported, 2);
    assert_eq!(report.summary(), "2 rows imported.");
    assert!(names(&app).await.contains(&"B".to_owned()));
}

#[renox::test]
async fn replicate_copies_a_model_into_an_unsaved_one() {
    let (app, _dir) = app().await;
    let mut item = Item::create(
        app.db(),
        Item {
            name: "Coffee".into(),
            stock: 4,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    item.delete(app.db()).await.unwrap();
    let original = Item::query()
        .with_trashed()
        .first(app.db())
        .await
        .unwrap()
        .unwrap();
    assert!(original.deleted_at.is_some() && original.created_at.is_some());

    let mut copy = original.replicate();
    assert_eq!((copy.id, copy.name.as_str(), copy.stock), (0, "Coffee", 4));
    assert!(copy.created_at.is_none() && copy.updated_at.is_none() && copy.deleted_at.is_none());
    copy.name = "Coffee (copy)".into();
    copy.save(app.db()).await.unwrap();
    assert!(copy.id > original.id && copy.created_at.is_some());
    assert_eq!(names(&app).await, ["Coffee (copy)"]);

    // The copy as the "new" form would show it.
    let json: serde_json::Value = app.get(&format!("/items/{}/copy", copy.id)).await.json();
    assert_eq!(json["id"], 0);
    assert_eq!(json["name"], "Coffee (copy)");
    assert_eq!(json["created_at"], serde_json::Value::Null);
}

#[renox::test]
async fn export_as_makes_a_file_of_a_query_outside_the_grid() {
    let (app, _dir) = app().await;
    for (name, stock) in [("Coffee", 4), ("Tea", 9), ("=cmd", 1)] {
        Item::create(
            app.db(),
            Item {
                name: name.into(),
                stock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    // The grid's default columns (not the hidden one), the query's order;
    // the request's filters don't apply.
    let res = app.get("/items/export/csv?q.name=Tea").await;
    res.assert_ok()
        .assert_header("content-type", "text/csv; charset=utf-8");
    assert!(
        res.header("content-disposition")
            .unwrap()
            .starts_with("attachment; filename=\"items-")
    );
    assert_eq!(
        res.text(),
        "\u{feff}Name,Stock\r\nTea,9\r\nCoffee,4\r\n'=cmd,1\r\n"
    );
    app.get("/items/export/print")
        .await
        .assert_ok()
        .assert_see("Coffee");
    // Not a format: 404 from the route's parameter.
    app.get("/items/export/pdf").await.assert_status(404);
    assert_eq!(ExportFormat::parse("xlsx"), Some(ExportFormat::Xlsx));
    assert_eq!(ExportFormat::Xlsx.available(), cfg!(feature = "xlsx"));
    assert!(ExportFormat::Csv.available());
}

#[cfg(feature = "xlsx")]
#[renox::test]
async fn a_workbook_holds_one_sheet_per_query() {
    use std::io::Read;
    let (app, _dir) = app().await;
    for (name, stock) in [("Coffee", 4), ("Tea", 9)] {
        Item::create(
            app.db(),
            Item {
                name: name.into(),
                stock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let res = app.get("/items.xlsx").await;
    res.assert_ok().assert_header(
        "content-type",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    );
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(res.body.to_vec())).unwrap();
    let mut read = |name: &str| {
        let mut text = String::new();
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    };
    let book = read("xl/workbook.xml");
    assert!(book.contains(r#"name="By stock""#) && book.contains(r#"name="By name""#));
    let strings = read("xl/sharedStrings.xml");
    assert!(strings.contains("Coffee") && strings.contains("Tea"));
    assert!(read("xl/worksheets/sheet2.xml").contains("<row"));
    // A title Excel refuses is a 400, not a corrupt file.
    app.get("/items-bad.xlsx").await.assert_status(400);
}

#[cfg(not(feature = "xlsx"))]
#[renox::test]
async fn a_workbook_needs_the_xlsx_feature() {
    let (app, _dir) = app().await;
    app.get("/items.xlsx").await.assert_status(400);
}
