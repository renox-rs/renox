# Admin panel

`renox-admin` makes the back office for you: you declare each model once, as a **resource**
(its list's columns, its form's fields, who may do what), and the panel gives it a list with
search, filters, sorting, bulk actions and exports, and pages to create, view, edit and delete
records. It is built on the [UI kit](ui.md) and the [data grid](grid.md), so it looks and
works like the rest of a Renox app.

[examples/bikeshop](../examples/bikeshop) puts its catalogue, workshop, suppliers and stores in a
panel authorized by permission, with a column of its own and the Markdown editor
([src/app/staff/admin.rs](../examples/bikeshop/src/app/staff/admin.rs)).

### In this guide

- adding the panel and saying who may open it;
- declaring a resource: columns, fields, the form and how it fills the model;
- the policy behind every page and button;
- filters, actions (also ones that ask for input), soft deletes and the trash, exports;
- what runs after a save, child rows managed from a record (relation managers), and resources
  that are only edited;
- resources a module contributes to the panel from its own `register`;
- settings pages, the view page, the routes, words in another language, and adding to or replacing the panel's
  pages;
- how it maps to Laravel's Filament.

### Words you'll meet

| Word | What it means |
|---|---|
| **panel** | The admin pages: a sidebar of resources, a dashboard, and each resource's pages. |
| **resource** | One model in the panel, declared with the `AdminResource` trait. |
| **policy** | The model's `Policy`: whether a user may do an ability (`update`, `delete`…) to a record. |
| **ability** | A word for an action people take: `viewAny` (see the list), `create`, `update`… |
| **soft delete** | Marking a row deleted (`deleted_at`) instead of removing it, so it can come back. |

> [!NOTE]
> **Coming from Laravel:** this is Filament's resources. The last section,
> [Coming from Laravel and Filament](#coming-from-laravel-and-filament), maps one to the other.

## Adding the panel

Add the crate next to `renox`:

```toml
[dependencies]
renox = "1.1"
renox-admin = "1.1"
```

Then add the `Admin` module, after `Auth` (the panel is behind a login):

```rust
use renox::prelude::*;
use renox_admin::Admin;
# use renox::grid::Column;
# use renox_admin::{AdminResource, Field};
# #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, name: String }
# impl Policy for Product { fn allows(&self, _: &User, _: &str) -> bool { true } }
# #[derive(serde::Deserialize, serde::Serialize, Validate)] struct ProductForm { #[validate(required)] name: String }
# struct ProductResource;
# impl AdminResource for ProductResource {
#     type Model = Product;
#     type Form = ProductForm;
#     fn label(&self) -> &str { "Product" }
#     fn plural_label(&self) -> &str { "Products" }
#     fn columns(&self) -> Vec<Column> { vec![Column::text("name", "Name")] }
#     fn fields(&self) -> Vec<Field> { vec![Field::text("name", "Name")] }
#     fn fill(&self, p: &mut Product, f: ProductForm) { p.name = f.name; }
# }

/// The app, with the panel at /admin.
pub fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(Admin::new()
            .title("Corner Shop")                     // the sidebar's name
            .authorize(|user| user.has_role("staff")) // who may open the panel
            .resource(ProductResource))
}
```

What each part does:

- `Admin::new()` makes a panel at `/admin`. `.path("/backoffice")` moves it; the route names
  stay `admin.*`. A panel named with `.named("ops")` has the route names `admin.ops.*`
  (`admin.ops.dashboard`, `admin.ops.products.index`) and the module name `admin.ops`, so two
  panels at different paths can run in one app.
- `.title(…)` names the panel in its sidebar and page titles.
- `.authorize(|user| …)` says who may open the panel at all. **Nobody may until you say so**:
  without it every page answers 403 (forbidden). Guests are sent to log in. `.gate("admin")`
  is the same with a gate or a permission of that name ([authorization.md](authorization.md)).
- `.resource(…)` adds a resource. They show in the sidebar in the order you add them.

The panel's pages:

- `/admin`: the dashboard, with how many records each resource has;
- `/admin/products`: the list;
- `/admin/products/create` and `/admin/products/{id}/edit`: the forms;
- `/admin/products/{id}`: the view page.

## Declaring a resource

A resource is a type that implements `AdminResource`. It names two types:

- `Model`: the model it manages. It needs `Serialize` (the pages read its fields), `Default` (a
  new record starts from it) and `Policy` (see [The policy](#the-policy)).
- `Form`: the create and edit form, with its own rules (`Validate`), exactly as a handler's
  `Valid<T>` would read it. It needs `Serialize` too, to refill the form after an error.

```rust
use renox::chrono::NaiveDate;
use renox::grid::Column;
use renox::prelude::*;
use renox_admin::{AdminResource, Field};
use serde::{Deserialize, Serialize};

#[derive(Model, Serialize, Default)]
#[model(table = "products")]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub price: i64,
    pub status: String,
    pub active: bool,
    pub category_id: Option<i64>,
    pub released_on: Option<NaiveDate>,
}

impl Policy for Product {
    fn allows(&self, user: &User, ability: &str) -> bool {
        // Everyone in the panel looks; managers change things.
        matches!(ability, "viewAny" | "view") || user.has_role("manager")
    }
}

/// The create and edit form: Renox checks these rules before anything is saved.
#[derive(Deserialize, Serialize, Validate)]
pub struct ProductForm {
    #[validate(required, max = 100)]
    pub name: String,
    #[validate(required, min = 0)]
    pub price: i64,
    #[validate(required, one_of(&["draft", "live"]))]
    pub status: String,
    pub active: bool,
    #[validate(exists("categories", "id"))]
    pub category_id: Option<i64>,
    pub released_on: Option<NaiveDate>,
}

pub struct ProductResource;

impl AdminResource for ProductResource {
    type Model = Product;
    type Form = ProductForm;

    fn label(&self) -> &str { "Product" }
    fn plural_label(&self) -> &str { "Products" }

    /// The list: a data grid with these columns.
    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("name", "Name").searchable(),
            Column::money("price", "Price"),
            Column::select("status", "Status", [("draft", "Draft"), ("live", "Live")]),
            Column::bool("active", "Active"),
            Column::related("category", "Category", "categories", "category_id", "name"),
        ]
    }

    /// The create and edit pages: these fields, in a two-column form.
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name", "Name").required(),
            Field::money("price", "Price").required(),
            Field::select("status", "Status", [("draft", "Draft"), ("live", "Live")])
                .required()
                .default_value("draft"),
            Field::toggle("active", "Active").default_value(true),
            Field::belongs_to("category_id", "Category", "categories", "name"),
            Field::date("released_on", "Released"),
        ]
    }

    /// A valid form, copied into the record before it is saved.
    fn fill(&self, product: &mut Product, form: ProductForm) {
        product.name = form.name;
        product.price = form.price;
        product.status = form.status;
        product.active = form.active;
        product.category_id = form.category_id;
        product.released_on = form.released_on;
    }
}
```

What's going on:

- `label` and `plural_label` name one record and several. Renox doesn't make plurals for you
  (not every language adds an "s").
- `columns()` are [grid](grid.md) columns, with every option the grid has: `.searchable()`
  puts a search box over the list, each heading filters and sorts by its kind, and
  `Column::related` shows a value from another table.
- `fields()` are the form's fields, left to right, top to bottom.
- `fill` copies a valid form into the record. Creating starts from `Product::default()`;
  editing starts from the record as it is. Then the panel saves it (model hooks run).

The rest of the trait has defaults you can change:

| Method | Default | What it's for |
|---|---|---|
| `slug()` | the model's table | the resource's part of the address and route names: `/admin/products`, `admin.products.index` |
| `navigation_group()` | none | a heading in the sidebar ("Catalog") |
| `record_title(record)` | "Product #7" | what the view and edit pages call a record (its name reads better) |
| `rules(form, record, v)` | none | rules that need the record (below) |
| `entries()` | one per column | the view page's details; none means no view page |
| `filters()` | none | named filters, as tabs over the list |
| `actions()` | none | the resource's own actions on selected rows |
| `query()` | `Model::query()` | where every query starts: narrow what the panel may ever show |
| `grid(grid)` | as built | changes to the list's grid: `grid.sort_by("name").cards_on_mobile()` |
| `allows(user, ability, record)` | the model's policy | who may do what ([The policy](#the-policy)) |

### Fields

Each `Field` constructor takes the form's field name (the same as the model's, usually) and its
label. The kind decides which of the kit's fields draws it:

| Field | Draws | In the form |
|---|---|---|
| `text`, `email`, `url`, `tel` | a text box of that type | `String` |
| `password` | a password box with a "show" button, empty on the edit page | `Option<String>` (change it only when one was typed) |
| `textarea` | several lines (`.rows(n)`) | `String` |
| `number` | a number box (`.step("0.01")`, `.min(0)`, `.max(99)`) | a number |
| `money` | a number box with `APP_CURRENCY`'s code before it, in whole units (`12.99`) | the smallest unit (`1299` cents) |
| `date` | the kit's date picker | `NaiveDate` |
| `datetime` | a date-and-time box | `NaiveDateTime` |
| `select(name, label, options)` | a select of `(value, label)` pairs (`.searchable()`) | the value |
| `checkbox`, `toggle` | a checkbox, or a switch | `bool` (unticked is `false`) |
| `belongs_to(name, label, table, title)` | a searchable select of `table`'s rows, by `title` | the row's id |

> [!NOTE]
> The model keeps money in the smallest unit, as the grid's `money` columns read it: cents
> with the default `USD`. The form shows and takes whole units: a price of `1299` shows as
> `12.99`, and `12.99` typed in reaches the form's rules and `fill` as `1299` (rounded to the
> cent). For a currency without decimals (IDR, JPY) the amount is as typed. The view page's
> `money` entries show `$12.99` too.

A record that keeps its own currency (the `<field>_currency` column of a `#[model(money)]`
field) says so with `currency_from`, on the field, the column and the entry:

```rust,no_run
# use renox_admin::{Entry, Field};
# use renox::grid::Column;
let column = Column::money("total", "Total").currency_from("total_currency");
let entry = Entry::new("total", "Total").format("money").currency_from("total_currency");
let fields = vec![
    Field::money("total", "Total").currency_from("total_currency"),
    Field::select("total_currency", "Currency", [("USD", "USD"), ("JPY", "JPY")])
        .default_value("USD"),
];
```

The list shows each row in its currency, the view page shows `¥1,250` or `$12.50`, and the
form shows the amount with that currency's decimals and code (the create page uses the
currency field's default value). On save the amount is read with the decimals of the
currency posted with it, so `1250` is 1250 JPY and `1.250` is 1250 fils in KWD. The
currency itself is an ordinary field of the form (a `select` of codes); validate it with
`.currency()`. Without `currency_from` nothing changes: `APP_CURRENCY` applies.
`Entry::from_column` carries a column's `currency_from` over.

Options every field takes:

- `.required()` marks it for people and the browser. The form's rules decide what's valid.
- `.hint("…")`, `.placeholder("…")`, `.prefix("https://")`, `.suffix("kg")`.
- `.span_full()` takes the form's whole width.
- `.default_value(…)` is what the create page starts with.
- `.only_on_create()`, `.only_on_edit()`, and `.readonly_on_edit()` (shown, still sent, but
  not changeable once the record exists).
- `.autocomplete("off")`.

A `belongs_to` field lists the first 1,000 rows of its table. Pair it with
`#[validate(exists("categories", "id"))]` in the form, so only real rows are accepted.

### Rules that need the record

The form's own rules can't see the record being edited. A unique value is the usual case: a
product's SKU may be its own, but not another product's. `rules` runs after the form's rules,
with the record when editing (`None` while creating):

```rust
# use renox::prelude::*;
# use renox::grid::Column;
# use renox_admin::{AdminResource, Field};
# #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, sku: String }
# impl Policy for Product { fn allows(&self, _: &User, _: &str) -> bool { true } }
# #[derive(serde::Deserialize, serde::Serialize, Validate)] struct ProductForm { #[validate(required)] sku: String }
# struct ProductResource;
# impl AdminResource for ProductResource {
#     type Model = Product;
#     type Form = ProductForm;
#     fn label(&self) -> &str { "Product" }
#     fn plural_label(&self) -> &str { "Products" }
#     fn columns(&self) -> Vec<Column> { vec![] }
#     fn fields(&self) -> Vec<Field> { vec![] }
#     fn fill(&self, p: &mut Product, f: ProductForm) { p.sku = f.sku; }
/// The SKU is unique among the products, but a product keeps its own.
fn rules(&self, form: &ProductForm, product: Option<&Product>, v: &mut Validator) {
    let rule = v.field("sku", &form.sku).label("SKU").unique("products", "sku");
    if let Some(product) = product {
        rule.ignore(product.id);
    }
}
# }
```

A failure answers as the form's own rules do: the errors under their fields.

### How the forms behave

- The forms are sent with htmx. Errors show under their fields (a 422 answer) and the form
  keeps what was typed. A success goes back to the list, with a toast ("Product saved.").
- They check each field as you leave it (live validation), with the form's rules.
- Without JavaScript they are plain forms: an error comes back to the form with what was typed.
- Ctrl+S (⌘S on a Mac) saves; N on the list opens the create page.

## The policy

Every page and button asks `AdminResource::allows(user, ability, record)`. By default it asks
the model's `Policy` (after `App::gate_before`), with these abilities:

| Ability | Asked before |
|---|---|
| `viewAny` | the list, its exports, the resource's link in the sidebar and its dashboard figure |
| `view` | a record's view page |
| `create` | the create page, and the list's "New" button |
| `update` | the edit page and saving it; the list's "Edit" link; an action's default |
| `delete` | deleting a record |
| `deleteAny` | offering "Delete" over selected rows (each record is asked `delete` too) |
| `restore`, `restoreAny` | bringing back deleted records; the "Trash" tab |
| `forceDelete`, `forceDeleteAny` | deleting records for good, from the trash |

A record-level ability gets the record. For the list-level questions (`viewAny`, `create`,
`deleteAny`, and whether the list offers "Edit" or "Delete" at all), there's no record yet,
so the policy gets a blank one (`Product::default()`, id 0). A policy that decides by role
works for both:

```rust
# use renox::prelude::*;
# #[derive(Model, serde::Serialize, Default)] struct Product { id: i64 }
impl Policy for Product {
    fn allows(&self, user: &User, ability: &str) -> bool {
        match ability {
            "viewAny" | "view" => true,
            "create" | "update" => user.has_permission("catalog.manage"),
            _ => user.has_permission("records.delete"), // delete, restore, …
        }
    }
}
```

A policy that decides by the record (only the author edits) also works: each record is
checked when it's opened or changed. The list asks about a blank record, though, so it would
hide its "Edit" link; override `allows` on the resource for a different answer when there's
no record.

What's refused answers 403, and the pages leave out the buttons the user may not use. Bulk
actions check every selected record: one refusal and nothing runs.

## Filters

Each column's heading already filters by its kind (see [grid.md](grid.md)). On top of that,
`filters()` adds named filters, shown as tabs over the list ("All", then yours):

```rust
# use renox::prelude::*;
use renox_admin::Filter;
# #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, status: String, stock: i64 }
/// The products resource's tabs.
fn filters() -> Vec<Filter<Product>> {
    vec![
        Filter::new("live", "Live", |q| q.where_(Product::STATUS.eq("live"))),
        Filter::new("low", "Low stock", |q| q.where_(Product::STOCK.lt(5))),
    ]
}
# let _ = filters();
```

A tab is `?filter=live` in the address, so it has a link of its own. The grid's search,
filters and exports work inside it.

## Actions

The list has these actions out of the box, each shown only to those allowed:

- in each row's ⋯ menu: **View**, **Edit** and **Delete** (with a confirmation);
- over the selected rows: **Delete** (a checkbox per row, "Select all N matching" for every
  row the filters match).

`actions()` adds the resource's own. An action gets the selected records and answers with a
toast:

```rust
# use renox::prelude::*;
use renox_admin::AdminAction;
# #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, status: String }
/// "Publish" over the selection, and in each row's menu.
fn publish() -> AdminAction<Product> {
    AdminAction::new("publish", "Publish", |products: Vec<Product>, cx| async move {
        let ids: Vec<i64> = products.iter().map(|p| p.id).collect();
        let n = Product::query()
            .where_(Product::ID.is_in(ids))
            .update(&cx.state.db, &[("status", &"live")])
            .await?;
        Ok(Toast::success(format!("{n} published.")))
    })
    .confirm("Publish the selected products?") // asks first
    .row() // also in each row's menu
}
# let _ = publish();
```

- The key (`"publish"`) is part of the action's address: `POST /admin/products/actions/publish`
  (selected rows) and `POST /admin/products/{id}/actions/publish` (one row).
- `cx` is an `ActionContext`: `cx.state` (the database, the queue, the mailer…) and `cx.user`.
- `.ability("publish")` changes the ability each record must allow (`update` by default).
- `.danger()` shows it in red; `.row_only()` puts it only in the rows' menus.
- At most 10,000 records are loaded for one action.

### Actions that ask for input

Give an action a `form` and its button opens a sheet with those fields (any `Field` but
`belongs_to`). The action runs once they are valid, and `cx.input` holds what was typed:

```rust
# use renox::prelude::*;
use renox_admin::{AdminAction, Field};
# #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, price: i64 }
/// "Change price by %": the percentage is typed in the sheet.
fn reprice() -> AdminAction<Product> {
    AdminAction::new("reprice", "Change price by %", |products: Vec<Product>, cx| async move {
        let percent: f64 = cx.input.parse("percent").unwrap_or(0.0);
        for product in &products {
            let price = (product.price as f64 * (1.0 + percent / 100.0)).round() as i64;
            Product::query()
                .where_(Product::ID.eq(product.id))
                .update(&cx.state.db, &[("price", &price)])
                .await?;
        }
        Ok(Toast::success(format!("{} prices changed by {percent} %.", products.len())))
    })
    .form(vec![Field::number("percent", "Percent").required().min(-90).max(500).suffix("%")])
    .description("Every selected product's price changes by this much.")
    .submit_label("Change prices")
    .row()
}
# let _ = reprice();
```

- The input is checked as far as the fields say: `required`, numbers and dates that read as
  such, `min` and `max`, a select's choices. A failure shows under the field in the sheet (a 422
  answer) and nothing runs.
- `cx.input` is an `ActionInput`: `get(name)`, `text(name)` (empty when missing), `parse::<T>(name)`,
  `bool(name)` for checkboxes and toggles. Money fields arrive in the currency's smallest unit.
- The sheet replaces the `.confirm(…)` question; when the action is done, the page reloads
  with its toast.
- The sheet is the kit's `action_sheet` after the grid, and the grid opens it
  (`grid::Action::sheet(id)`), with the selection.

## After a record is saved

`saved` runs after the create or edit page saved a record (Filament's `afterSave`). It gets the
record, who saved it and, on an edit, the record as it was before the form changed it (its
fields as JSON): audit an exact price change, refresh search keywords.

```rust
# use renox::prelude::*;
# use renox::grid::Column;
# use renox_admin::{AdminResource, Field, SaveContext};
# #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, name: String, price: i64 }
# impl Policy for Product { fn allows(&self, _: &User, _: &str) -> bool { true } }
# #[derive(serde::Deserialize, serde::Serialize, Validate)] struct ProductForm { #[validate(required)] name: String }
# struct ProductResource;
# impl AdminResource for ProductResource {
#     type Model = Product;
#     type Form = ProductForm;
#     fn label(&self) -> &str { "Product" }
#     fn plural_label(&self) -> &str { "Products" }
#     fn columns(&self) -> Vec<Column> { vec![] }
#     fn fields(&self) -> Vec<Field> { vec![] }
#     fn fill(&self, p: &mut Product, f: ProductForm) { p.name = f.name; }
/// Writes who changed a price, from what to what.
async fn saved(&self, product: &Product, cx: &SaveContext) -> Result {
    let before = cx.previous.as_ref().and_then(|old| old["price"].as_i64());
    if before.is_some_and(|before| before != product.price) {
        renox::db::sql("INSERT INTO audit_logs (note) VALUES (?)")
            .bind(format!("{} set {} to {}", cx.user.name, product.name, product.price))
            .execute(&cx.state.db)
            .await?;
    }
    Ok(())
}
# }
```

`cx.created` says whether the record is new. The record is saved when the hook runs, so an
error from it answers the request with that error: do work that must go in with the record in a
transaction of your own. Only the create and edit forms (a relation manager's too) call it;
bulk actions don't.

## Child rows: relation managers

A record often owns rows of other tables: a product's variants and photos, the bike models a
part fits. `relations()` lists them, and each becomes a tab of the record's view and edit
pages, with its own list:

```rust
# use renox::prelude::*;
# use renox::grid::Column;
# use renox_admin::{AdminResource, Field, RelationManager};
# #[derive(Model, serde::Serialize, Default)] struct Product { id: i64, name: String }
# impl Policy for Product { fn allows(&self, _: &User, _: &str) -> bool { true } }
# #[derive(Model, serde::Serialize, Default)] struct Variant { id: i64, product_id: i64, name: String }
# impl Policy for Variant { fn allows(&self, _: &User, _: &str) -> bool { true } }
# #[derive(Model, serde::Serialize, Default)] struct BikeModel { id: i64, name: String }
# impl Policy for BikeModel { fn allows(&self, _: &User, _: &str) -> bool { true } }
# #[derive(serde::Deserialize, serde::Serialize, Validate)] struct ProductForm { #[validate(required)] name: String }
/// The variants' form has the foreign key: the panel fills it in.
#[derive(serde::Deserialize, serde::Serialize, Validate)]
struct VariantForm {
    product_id: i64,
    #[validate(required, max = 50)]
    name: String,
}
# #[derive(serde::Deserialize, serde::Serialize, Validate)] struct BikeModelForm { #[validate(required)] name: String }

struct Variants;

impl AdminResource for Variants {
    type Model = Variant;
    type Form = VariantForm;
    fn label(&self) -> &str { "Variant" }
    fn plural_label(&self) -> &str { "Variants" }
    fn columns(&self) -> Vec<Column> { vec![Column::text("name", "Name")] }
    fn fields(&self) -> Vec<Field> { vec![Field::text("name", "Name").required()] }
    fn fill(&self, v: &mut Variant, f: VariantForm) {
        v.product_id = f.product_id;
        v.name = f.name;
    }
}
# struct BikeModels;
# impl AdminResource for BikeModels {
#     type Model = BikeModel;
#     type Form = BikeModelForm;
#     fn label(&self) -> &str { "Bike model" }
#     fn plural_label(&self) -> &str { "Bike models" }
#     fn columns(&self) -> Vec<Column> { vec![Column::text("name", "Name")] }
#     fn fields(&self) -> Vec<Field> { vec![Field::text("name", "Name").required()] }
#     fn fill(&self, m: &mut BikeModel, f: BikeModelForm) { m.name = f.name; }
# }
# struct ProductResource;
# impl AdminResource for ProductResource {
#     type Model = Product;
#     type Form = ProductForm;
#     fn label(&self) -> &str { "Product" }
#     fn plural_label(&self) -> &str { "Products" }
#     fn columns(&self) -> Vec<Column> { vec![] }
#     fn fields(&self) -> Vec<Field> { vec![] }
#     fn fill(&self, p: &mut Product, f: ProductForm) { p.name = f.name; }
fn relations(&self) -> Vec<RelationManager> {
    vec![
        // Rows holding the product's id: created, edited and deleted under it.
        RelationManager::has_many("variants", "Variants", Variants, "product_id"),
        // Rows joined by a pivot table: attached and detached.
        RelationManager::belongs_to_many(
            "fits", "Fits", BikeModels, "product_fits", "product_id", "bike_model_id",
        ),
    ]
}
# }
```

- **`has_many(key, label, resource, foreign_key)`**: the tab lists the rows whose
  `foreign_key` column holds the record's id. It has its own create, edit and delete pages
  (under `/admin/products/{id}/relations/variants`) and a bulk delete. The child's form needs the
  foreign key as a field (`product_id` above): the panel leaves it out of the form and fills
  it with the record's id, whatever the browser sends, so a row can't be moved to another
  record. The child's `fill`, `rules` and `saved` run as in its own resource.
- **`belongs_to_many(key, label, resource, pivot_table, parent_key, related_key)`**: the tab
  lists the rows joined to the record by the pivot table, with **Attach** (a select of the
  rows not attached yet, named by their `name` column, or `.title_column("title")`) and
  **Detach** per row and over the selection. The pivot rows hold the two ids only.
- The managed resource decides who sees the tab and what the buttons are (its `allows`:
  `viewAny`, `create`, `update`, `delete`, `deleteAny`, and its `creatable`/`deletable`).
  Attaching and detaching also need the record's `update`.
- A managed resource needn't be in the navigation: a resource only used as a relation isn't
  registered with `Admin::resource`.
- Routes are named `admin.products.variants.index`, `.create`, `.store`, `.edit`, `.update`,
  `.destroy`, `.bulk`; a pivot has `.index`, `.attach`, `.detach` and `.bulk`. A custom column
  of the child is drawn by `renox-admin/{child slug}/cells.html`.

## Resources that are only edited

Some records are changed but never made or removed from the panel: a settings row, a store, a
record other code creates. Say so, and the panel has no "New" button, no delete buttons or
bulk delete, no trash, and those addresses answer 404:

```rust
# use renox::prelude::*;
# use renox::grid::Column;
# use renox_admin::{AdminResource, Field};
# #[derive(Model, serde::Serialize, Default)] struct Store { id: i64, name: String }
# impl Policy for Store { fn allows(&self, _: &User, _: &str) -> bool { true } }
# #[derive(serde::Deserialize, serde::Serialize, Validate)] struct StoreForm { #[validate(required)] name: String }
# struct Stores;
# impl AdminResource for Stores {
#     type Model = Store;
#     type Form = StoreForm;
#     fn label(&self) -> &str { "Store" }
#     fn plural_label(&self) -> &str { "Stores" }
#     fn columns(&self) -> Vec<Column> { vec![] }
#     fn fields(&self) -> Vec<Field> { vec![] }
#     fn fill(&self, s: &mut Store, f: StoreForm) { s.name = f.name; }
fn creatable(&self) -> bool { false }
fn deletable(&self) -> bool { false }
# }
```

## Soft deletes and the trash

When the model has soft deletes (`#[model(soft_deletes)]`, a `deleted_at` column), deleting
puts a record in the trash instead of removing it:

- the list gets a **Trash** tab (for those allowed `restoreAny`) listing the deleted records,
  with **Restore** and **Delete for good** (`restore`, `forceDelete`) per row and over the
  selection;
- a deleted record's view page says so, and offers the same.

Without soft deletes, "Delete" removes the row, and the confirmation says it can't be undone.

## Exports

Every list has the grid's export menu: CSV, a print page, and Excel when the app turns on the
`xlsx` feature (`renox-admin = { version = "…", features = ["xlsx"] }`, or renox's own `xlsx`
feature). An export holds every row the filters, the search and the tab match, in the columns
the user shows.

## The view page

A record's view page lists its details in the kit's infolist. By default there is one entry
per column, formatted by the column's kind (money, dates, yes/no, a select's labels as badges).
`entries()` sets them yourself:

```rust
use renox_admin::Entry;

/// The products' view page.
fn entries() -> Vec<Entry> {
    vec![
        Entry::text("name", "Name"),
        Entry::text("sku", "SKU").copyable(),
        Entry::new("price", "Price").format("money"),
        Entry::new("status", "Status").labels([("live", "Live")]).badge(),
        Entry::new("updated_at", "Last changed").format("since"),
        Entry::new("notes", "Notes").format("markdown").span_full(),
    ]
}
# assert_eq!(entries().len(), 6);
```

The formats are the kit's `entry` formats: `date`, `datetime`, `since`, `money`, `number`,
`bool`, `color`, `image` and `markdown`. Return no entries for a resource without a view page:
its rows open the edit page instead.

## Settings pages

A form for a group of [settings kept in the database](settings.md) (a company's name, a gateway's
keys) is not a resource: there is one record, with no list. `Admin::settings::<T>(label)` adds a
page for the group `T`, under a "Settings" heading of the navigation. `T` says which fields the
form has and who may change it:

```rust
use renox::prelude::*;
use renox::settings::{Settings, SettingsGroup};
use renox_admin::{Admin, AdminSettings, Field};

#[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
struct Company {
    #[validate(required, max(120))]
    name: String,
    tax_number: Option<String>,
}

impl SettingsGroup for Company {
    const KEY: &'static str = "company";
}

impl AdminSettings for Company {
    fn fields() -> Vec<Field> {
        vec![
            Field::text("name", "Company name").required(),
            // Shown, but its stored value is kept whatever is sent.
            Field::text("tax_number", "Tax number").readonly_on_edit(),
        ]
    }

    // "view" opens the page, "update" saves it; the panel's own gate comes first.
    fn allows(user: &AuthUser, ability: &str) -> bool {
        ability == "view" || user.has_role("owner")
    }
}

fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(Settings) // the group's table; the page declares the group
        .module(Admin::new().authorize(|user| user.has_role("staff")).settings::<Company>("Company"))
}
```

The page is `GET /admin/settings/company` and `PUT` to the same path (routes
`admin.settings.company.show` and `.update`). Saving runs the group's `#[validate]` rules (errors
show under the fields, like a resource's form) and goes through `AppState::save_settings`, so it
is audited and emits `SettingsChanged`. A user who may `view` but not `update` sees the form
disabled without a button. A group marked `ENCRYPTED` never prints its values: its fields start
empty with a note "Set, last changed …" (or "Not set yet."), and a field left empty keeps its
stored value. The page is `renox-admin/settings.html`; the words are `renox.admin.sealed_set`
and `renox.admin.sealed_unset`.

## Resources from modules

A module can add a resource to the app's panel from its own `register`, so the panel's builder
never names it. This is `Registry::contribute` (a typed list any module can add to and read) with
`renox_admin::contribute` on top:

```rust
use renox::grid::Column;
use renox::prelude::*;
use renox_admin::{Admin, AdminEntry, AdminResource, Field};
# #[derive(Model, serde::Serialize, Default)] struct Supplier { id: i64, name: String }
# impl Policy for Supplier { fn allows(&self, _: &User, _: &str) -> bool { true } }
# #[derive(serde::Deserialize, serde::Serialize, Validate)] struct SupplierForm { #[validate(required)] name: String }
# struct SupplierResource;
# impl AdminResource for SupplierResource {
#     type Model = Supplier;
#     type Form = SupplierForm;
#     fn label(&self) -> &str { "Supplier" }
#     fn plural_label(&self) -> &str { "Suppliers" }
#     fn columns(&self) -> Vec<Column> { vec![Column::text("name", "Name")] }
#     fn fields(&self) -> Vec<Field> { vec![Field::text("name", "Name")] }
#     fn fill(&self, s: &mut Supplier, f: SupplierForm) { s.name = f.name; }
# }

/// A module that adds suppliers to the admin panel.
pub struct Purchasing;

impl Module for Purchasing {
    fn name(&self) -> &'static str { "purchasing" }

    fn register(&self, app: &mut Registry) {
        renox_admin::contribute(app, SupplierResource);
        // Or, for a panel with a name (see below):
        // app.contribute(AdminEntry::new(SupplierResource).panel("staff"));
    }
}

/// The panel lists no supplier; the module adds it.
pub fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(Admin::new().authorize(|user| user.has_role("staff")))
        .module(Purchasing)
}
```

The order of the `.module(...)` lines does not matter: every module's `register` runs before any
panel builds its routes. The contributed resource behaves like one added with `Admin::resource`:

- it has the same pages, the `admin.{slug}.*` routes (`rnx route:list` shows them), the panel's
  `authorize` or `gate`, the model's `Policy` and the resource's `allows`, and its navigation
  group (`group(...)` on the resource);
- it is listed after the panel's own resources: the app's contributions first, then the modules'
  in the order they were added.

Which panel takes it:

- By default an `Admin` takes the entries that name no panel. With two panels, give one a name
  with `Admin::named("staff")` and aim an entry at it with `AdminEntry::new(r).panel("staff")`; a
  named panel takes only entries for its name. An entry for a name no `Admin` has stops the boot
  with an error.
- `Admin::new().only_own_resources()` ignores what modules contribute: only the resources added
  to it with `.resource(...)`.

Two resources with the same slug stop the boot, and the message names who added each:
`"suppliers" is contributed by purchasing and by billing`; a clash with the panel's own resource
names "the app's Admin". Settings pages and dashboard widgets are not contributed this way yet.

## Routes

The panel's routes are named, so templates and redirects can reach them with `route(…)`:

| Route | Name |
|---|---|
| `GET /admin` | `admin.dashboard` |
| `GET /admin/products` (and its exports) | `admin.products.index` |
| `GET /admin/products/create`, `POST /admin/products` | `admin.products.create`, `admin.products.store` |
| `GET /admin/products/{id}` | `admin.products.show` |
| `GET /admin/products/{id}/edit`, `PUT /admin/products/{id}` | `admin.products.edit`, `admin.products.update` |
| `DELETE /admin/products/{id}` | `admin.products.destroy` |
| `POST /admin/products/actions/{action}` | `admin.products.bulk` |
| `POST /admin/products/{id}/actions/{action}` | `admin.products.action` |
| `GET /admin/products/{id}/relations/{key}` (a relation manager's list, and its create, edit, attach… pages) | `admin.products.{key}.index`, `.create`, `.store`, `.edit`, `.update`, `.destroy`, `.attach`, `.detach`, `.bulk` |

The built-in actions are `delete`, `restore` and `force-delete`. Every route needs a login,
and `rnx route:list` (the app's `route:list`) shows them.

## Words in another language

The panel's own words (the buttons, the tabs, the toasts, "Save changes", "Product #7") are
English by default. An app translates any of them with a key under `renox.admin.` in its lang
files (`resources/lang/es.json`), as it does for Renox's auth pages:

```json
{
  "renox": {
    "admin": {
      "new": "Nuevo :label",
      "edit": "Editar",
      "delete": "Eliminar",
      "save_changes": "Guardar cambios",
      "record_title": ":label n.º :id",
      "Product": "Producto",
      "Products": "Productos",
      "products": { "Name": "Nombre", "Price": "Precio", "Publish": "Publicar" }
    }
  }
}
```

- The built-in keys are `dashboard`, `new`, `create`, `edit_title`, `save_changes`, `cancel`,
  `edit`, `view`, `details`, `delete`, `restore`, `delete_for_good`, `all`, `trash`, `empty`,
  `record_title`, `created`, `saved`, `deleted`, `n_deleted`, `n_restored`, `attach`, `detach` and
  more; `:label`, `:plural`, `:title`, `:id` and `:what` stand for the values the panel fills in.
  The full list, with the English defaults, is `DEFAULTS` in the crate's `texts.rs`.
- The words you give your resources are looked up by their English text, first under
  `renox.admin.<slug>.` (a column or field name, an action or filter label, in one resource), then
  under `renox.admin.` itself (a label shared by all): `label()`, `plural_label()`,
  `navigation_group()`, column and field labels (and their hints, placeholders and select
  options), filter, action and entry labels. A text with no translation stays as written.
- The language is the request's (`APP_LOCALE`, `Accept-Language` with `App::detect_locale`, the
  session).

## Adding to the panel's layout

To put something in every page (an "About this page" panel, a notice, a link in the sidebar)
without replacing `layout.html`, use a **slot**: `Admin::slot(Slot::AfterContent, template)`,
or a file of the slot's name under the app's views. The slot's template sees the page's values
(`admin`, `resource`, `record`, `title`…) and the request's globals (`request.route`, `auth`…):

```rust
use renox_admin::{Admin, Slot};

fn panel() -> Admin {
    Admin::new().slot(
        Slot::AfterContent,
        r#"<aside class="about"><h2>About this page</h2><p>{{ request.route }}</p></aside>"#,
    )
}
# let _ = panel();
```

| Slot | File under the app's views | Where |
|---|---|---|
| `Slot::Head` | `renox-admin/slots/head.html` | in `<head>` |
| `Slot::SidebarStart` / `SidebarEnd` | `renox-admin/slots/sidebar_start.html` / `sidebar_end.html` | top and bottom of the sidebar |
| `Slot::NavbarEnd` | `renox-admin/slots/navbar_end.html` | in the top bar, before the account menu |
| `Slot::BeforeContent` / `AfterContent` | `renox-admin/slots/before_content.html` / `after_content.html` | above and below the page's content |

Several `slot` calls for one place are drawn in order; a file of the app's replaces them.

## Replacing the panel's pages

The pages are templates of the crate. An app replaces one by having a file of the same name
under its views directory:

| Template | What it is |
|---|---|
| `renox-admin/layout.html` | the frame: sidebar, account menu, toasts (brand the panel here) |
| `renox-admin/dashboard.html` | the dashboard (`cards`: each resource's `label`, `count`, `url`) |
| `renox-admin/index.html` | a list |
| `renox-admin/form.html` | the create and edit page |
| `renox-admin/fields.html` | the macro that draws one field |
| `renox-admin/show.html` | the view page |
| `renox-admin/relation.html` | a relation manager's list |

Every page gets `admin` (the panel: `title`, `nav`, `text`: its words in the request's
language, …) and `resource` (`slug`, `label`,
`plural_label`, `index_url`, `create_url`).

Grid columns of the `custom` kind are drawn by the page. The list includes
`renox-admin/{slug}/cells.html` when the app has one, with `row` and `column`:

```html
{#- resources/views/renox-admin/products/cells.html -#}
{%- if column.key == "level" -%}
  {{ row.stock | number }} <rx-badge rx-if="row.stock < 5" kind="warning">Low</rx-badge>
{%- endif -%}
```

## Coming from Laravel and Filament

> [!NOTE]
> **Coming from Laravel:** if you know Filament, this table says what each of its resource
> features is called here. If you don't, you can skip it.

| Filament | renox-admin |
|---|---|
| `Panel::make()->path('admin')` | `Admin::new().path("/admin")` |
| `canAccessPanel()` | `Admin::authorize(\|user\| …)` or `.gate("admin")` |
| `class ProductResource extends Resource` | `impl AdminResource for ProductResource` |
| `$model`, `$navigationGroup`, `getRecordTitle()` | `type Model`, `navigation_group()`, `record_title()` |
| `table()` with `columns([...])` | `columns()` (the grid's `Column`s) |
| `form()` with `TextInput`, `Select`, `Toggle`, `DatePicker`, `Select::relationship()` | `fields()`: `Field::text`, `select`, `toggle`, `date`, `belongs_to` |
| the form's `->rules()`, `->unique(ignoreRecord: true)` | the form type's `Validate`, `rules()` with `.ignore(record.id)` |
| `mutateFormDataBeforeSave()` | `fill()` |
| `infolist()` | `entries()` |
| `getTabs()`, `filters()` | `filters()` (tabs); each column's own filter |
| `BulkAction`, `Action` | `AdminAction` (`.row()` for the row menu) |
| an action's `->form([...])` and `$data` | `AdminAction::form(fields)` and `cx.input` |
| `afterSave()`, `afterCreate()` | `AdminResource::saved` (`cx.created` tells which) |
| `RelationManager`, `HasManyRelationManager`, `AttachAction`, `DetachAction` | `RelationManager::has_many`, `belongs_to_many` |
| `canCreate()`, `canDelete()` returning false | `creatable()`, `deletable()` |
| render hooks (`PanelsRenderHook::CONTENT_END`) | `Admin::slot(Slot::AfterContent, …)` |
| `__('filament-panels::...')` lang files | the `renox.admin.*` keys |
| `DeleteBulkAction`, `RestoreAction`, `ForceDeleteAction`, `TrashedFilter` | built in, with soft deletes |
| `ExportAction` | built in (CSV, print, Excel with `xlsx`) |
| model policies (`viewAny`, `create`, `update`, `delete`, `deleteAny`, …) | the model's `Policy`, same ability names |
| `getEloquentQuery()` | `query()` |
