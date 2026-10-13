# Data grids

A data grid is a table of records (orders, customers, products…) that people can sort, filter,
search, edit and export. `renox::grid` gives you one, ready to drop into a dashboard or a back
office (the pages where staff manage the business).

Here is what it does for you:

- It fills the space around it. Only the rows scroll; the toolbar on top and the page buttons
  at the bottom stay put, on a phone too.
- Each column heading has a filter that fits the column: a text box for names, a date range for
  dates, and so on.
- The server does the paging, sorting and filtering, and keeps them in the page's address
  (the URL). So a filtered page has a link of its own that you can bookmark or send.

[examples/bikeshop](../examples/bikeshop) uses grids for a whole back office: a polled fleet board
with cards on phones ([src/app/rentals/fleet.rs](../examples/bikeshop/src/app/rentals/fleet.rs)), and report grids
grouped, summed, filtered with rules, remembered and exported
([src/app/reports/grids.rs](../examples/bikeshop/src/app/reports/grids.rs)).

### In this guide

- defining a grid, the handler and the template;
- the kinds of columns and what they show;
- filters, search, the active-filter chips and the advanced filter;
- sorting, pages and the user's column choices;
- row details, editing in place, row order and merged cells;
- actions on rows and on a selection;
- summaries and groups;
- cards on phones, columns from other tables, exports and polling;
- several grids on one page, the query string, translations;
- how it maps to Laravel and Filament.

### Words you'll meet

| Word | What it means |
|---|---|
| **handler** | The Rust function that runs when someone opens a page, and returns the page. |
| **template** | An HTML file with blanks that the app fills in. The grid is drawn by one. |
| **tag** | A reusable piece of template written like HTML: `<rx-grid :page="orders"/>`. |
| **extractor** | A handler argument that Renox fills in from the request, like `GridRequest`. |
| **query** | A database question built in Rust, like `Order::query()` ("all orders"). |
| **query string** | The part of a URL after `?`, like `?page=2&sort=-total`. The grid keeps its state there. |
| **model** | A Rust struct that matches a database table: one value = one row. |
| **toast** | A small message that pops up in a corner of the page, like "Order saved." |
| **chip** | A small rounded label. The grid shows one per active filter, with a × to remove it. |

> [!NOTE]
> **Coming from Laravel:** the grid plays the part of Filament's tables. The last section,
> [Coming from Laravel and Filament](#coming-from-laravel-and-filament), maps one to the other.

## Defining a grid

A grid is a `Grid` value. It has three parts: an id, a list of columns, and some options.

Building a `Grid` is cheap, so the usual place for one is a small function that returns it.
Then the page's handler, the export and the bulk actions can all call that same function and
get the same grid.

```rust
# use renox::prelude::*;
use renox::grid::{Column, Grid, GridRequest};
# #[derive(Model, serde::Serialize, Default)]
# struct Order { id: i64, number: String, customer: String, status: String, total: i64,
#     ordered_on: renox::chrono::NaiveDate, trend: renox::db::Json<Vec<i64>> }

/// The orders grid: its id, its columns and how it's sorted at first.
fn orders_grid() -> Grid {
    Grid::new("orders")
        .title("Orders")
        .column(Column::text("number", "Order").frozen().mobile())
        .column(Column::text("customer", "Name").under(["Customer"]).mobile())
        .column(Column::select("status", "Status", [("new", "New"), ("paid", "Paid")]).mobile())
        .column(Column::money("total", "Total").under(["Amounts"]))
        .column(Column::date("ordered_on", "Ordered"))
        .column(Column::custom("trend", "Last 7 days"))
        .column(Column::custom("actions", "").frozen_right())
        .sort_by("-ordered_on")
}

/// The page that shows the grid.
async fn index(request: GridRequest) -> Result<View> {
    // Get one page of orders, with the filters and sort from the URL applied.
    // Then add an `up` value to each row: did the trend go up this week?
    let page = orders_grid()
        .page(Order::query(), &request)
        .await?
        .extend(|order| json!({ "up": order.trend.last() >= order.trend.first() }));
    Ok(view("orders/index.html", context! { orders => page }))
}
```

What's going on:

- `orders_grid()` lists the columns, left to right. Each column gets a key (the model's field,
  like `"number"`) and a heading label (like `"Order"`).
- `index` is the handler. It asks the grid for one page of orders and hands it to the template.
- The calls after each column (`.frozen()`, `.mobile()`, `.under(…)`) are column options. They
  are explained in [Columns](#columns).

The grid's own options:

- `Grid::new(id)` names the grid. The id may only have letters, digits, `_` and `-`, and at
  most 64 of them. It is used to store each user's column choices, and for the HTML element's
  id (`grid-orders`). With any other id, `page` returns an error.
- `.title("Orders")` puts a heading above the grid, next to the row count.
- `.per_page(n)` sets how many rows a page has before the user picks a size. The default is 25,
  and `n` is kept between 1 and 500. The page-size menu offers 10, 25, 50 and 100, plus `n`.
- `.sort_by("-ordered_on")` sets the order before the user sorts. Give a column key, with `-` in
  front for descending (biggest or newest first). You can give several keys, separated by
  commas: `"region,city,-total"`. When two rows tie, `id` always decides.
- `grid.id()` and `grid.columns()` read the definition back. For one column, `column.key()`,
  `column.label()`, `column.kind()` (a `Kind`) and `column.options()` (a select's or tags'
  choices) do the same.

## The handler

`GridRequest` is an extractor: put it in your handler's arguments and Renox fills it in. It
holds what the grid needs from the request:

- the query string (the filters, sort and page the user picked);
- the session (where a guest's choices are kept);
- the logged-in user (for their column choices);
- the database.

`grid.page(query, &request)` does the work. It takes your `query`, applies the request's
filters, search, sort and page to it, and returns a `GridPage`. Pass the `GridPage` to the
view as it is.

Any `Query` works as the starting point. The grid only ever shows what that query allows. For
example, `Order::query().where_(Order::TEAM_ID.eq(team))` shows only one team's orders, and a model's
default scope applies too.

What else you can do with the page and the grid:

- `page.extend(|order| json!({ … }))` adds values to each row that aren't columns of the
  model, such as a chart's points or a link. The values must be a JSON object; its keys are
  added to the row's own. `custom` columns read them.
- `page.items()` returns the models on this page. `page.total()` returns how many rows match
  the filters, across all pages.
- `grid.filter(query, &request)` returns a `Query` with the same filters and sort, but without
  the paging. Use it for totals, or for your own export over every filtered row.
- `GridRequest::new(&db, path, &params)` builds a request by hand, for tests and commands. It
  acts as a guest, in UTC. `request.param("export")` reads one value from the query string.
  It doesn't read the app's `APP_CURRENCY`: money columns use the decimals of the default
  currency (USD, two). So with a currency of other decimals (IDR, JPY), money columns are
  scaled wrongly there.

```rust
# use renox::prelude::*;
use renox::grid::{Column, Grid, GridRequest};
# #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, status: String }
# fn orders_grid() -> Grid {
#     Grid::new("orders").column(Column::select("status", "Status", [("new", "New"), ("paid", "Paid")]))
# }
# async fn demo(db: Db) -> Result {
// A request as if the URL were /orders?in.status=paid&sort=-id
let request = GridRequest::new(&db, "/orders", &[("in.status", "paid"), ("sort", "-id")]);
let page = orders_grid().page(Order::query(), &request).await?;
println!("{} of {} paid orders on this page", page.items().len(), page.total());
// The same filter as a plain query: count every paid order, not just one page.
let paid = orders_grid().filter(Order::query(), &request).count(&db).await?;
# let _ = paid;
# Ok(())
# }
```

What's going on: we build a request that filters on status "paid" and sorts by newest id. Then
we get one page of matching orders, and finally count all of them with `filter`.

## The template

The template draws the grid with the `<rx-grid>` tag (from `renox/grid.html`).

Most columns draw themselves. `custom` columns are different: you draw them, in the tag's
content. The grid runs the content once for each custom cell, and gives it two things:

- `row`: the row's values (the model's fields, the values from related tables, and what
  `extend` added);
- `column`: the column being drawn (`column.key` tells you which one).

```html
<main class="rx-grid-fill">
  <rx-grid :page="orders">
    {% if column.key == "trend" %}<span class="{{ 'rx-up' if row.up else 'rx-down' }}">{{ sparkline(row.trend) }}</span>
    {% elif column.key == "actions" %}<rx-link-button href="{{ route('orders.show', row.id) }}" label="Open" size="small"/>{% endif %}
  </rx-grid>
</main>
```

What's going on: for the `trend` column, the content draws a small chart, colored by whether the
trend went up. For the `actions` column, it draws an "Open" button that links to the order.

More about the template:

- Without content, `custom` columns stay empty: `<rx-grid :page="orders"/>`.
- `tools=` puts your own HTML in the toolbar, next to the grid's own buttons. A
  "New" button is a common example. Capture the HTML with a block `set`, so it isn't escaped
  (shown as text instead of HTML), and pass it as data:

  ```html
  {% set new_order %}<rx-link-button route="orders.create" label="New order" variant="primary" size="small"/>{% endset %}
  <rx-grid :page="orders" :tools="new_order">…</rx-grid>
  ```

- The content is also asked for two special keys:
  - `column.key == "_details"` is the row's details (see [Row details](#row-details)).
  - `column.key == "_empty"` gives the buttons under an empty grid's message (see
    [Empty grids](#empty-grids)).
- The tag adds the grid's stylesheet and script itself (`renox_grid()`). Your layout (the
  frame around every page) should have:
  - `{{ renox_ui() }}`, because the grid uses the UI kit's buttons;
  - `{{ toasts() }}`, for actions and edits that answer with a `Toast`.

> [!IMPORTANT]
> The grid needs the element around it to have a height, or it has nothing to fill. Put
> `rx-page--fill` on `<body>` (this makes the page as tall as the screen, with its `<main>`
> taking the rest under the kit's `navbar`) and `rx-grid-fill` on the `<main>`.

### Sparklines

`{{ sparkline(values) }}` draws a small chart as inline SVG (a picture made of HTML-like tags,
right in the page):

- It draws a line, or bars with `kind="bars"`.
- `width` and `height` are in pixels (96 × 28 by default).
- `label` is what screen readers say out loud (by default "first → last").
- It uses the text color, so wrapping it in `rx-up` or `rx-down` colors it.

## Columns

Each column constructor takes two things: the model's column key, and the heading's label.
The kind of column decides how values look and which filter the heading gets.

| Column | Shows | Heading filter |
|---|---|---|
| `text` | the text | contains / starts with / ends with / equals, or a pattern with `%` (`cof%`) |
| `number` (`.decimals(n)`), `money` | right-aligned, with the locale's separators; `money` is an amount in the smallest unit (cents, fils), shown in whole units of `APP_CURRENCY` with its usual decimals (`400000` is `4,000.00` in AED, `400,000` in IDR; no currency symbol, unlike the template filter `money`). Summaries and exports show whole units too; an inline edit sends the stored value | from–to (whole units for `money`) |
| `date`, `datetime` | `2026-03-05`; `datetime` adds the time, shown in `APP_TIMEZONE` | a date range: two date fields and a calendar ([Cally](https://wicky.nillia.ms/cally/), bundled); a `datetime` range takes whole days of `APP_TIMEZONE` |
| `bool` | Yes / No (`.icons()`: ✓ / ✗) | yes, no |
| `select(key, label, options)` | the option's label | pick some |
| `tags(key, label, options)` | a JSON array (`Json<Vec<String>>`) as tags | rows with any of the picked |
| `image` | the image at the URL in the value (`.round()` for avatars) | none |
| `color` | a swatch of the CSS color, with its code | like text |
| `custom` | whatever the tag's content draws | none |

A few things to know about the table:

- `options` are `(value, label)` pairs: the value stored in the database, and the text people
  see. For example `("paid", "Paid")`.
- Text filters ignore case: "cof" finds "Coffee".
- Every column kind except `tags`, `image` and `custom` can be sorted by clicking its heading.
- "Smallest unit" for `money` means you store a whole number of, say, cents. The grid divides
  it for you and shows whole units of `APP_CURRENCY`: `12550` cents shows as `125.50` in USD.
  Currencies without decimals (IDR, JPY) show the number as it is. There's no currency symbol,
  and the from–to filter takes whole units too. The `money` template filter takes the same
  smallest unit, so a cell and a page written with `| money` agree.

### Options every column takes

- `.sortable(false)` and `.filterable(false)` turn off the heading's sort or filter. `custom`
  columns never have either.
- `.mobile()` shows the column on phones (screens under 768 px wide) until the user picks
  otherwise. When no column has it, the first three visible columns show. `.hidden()` hides the
  column on wide screens too, until the user turns it on.
- `.frozen()` and `.frozen_right()` keep the column at the left or right edge while the grid
  scrolls sideways. A column of actions on the right is a typical use.
- `.under(["Amounts"])` puts the column under a grouped heading: one wider heading above
  several columns. Headings can nest (`.under(["Sales", "Q1"])`). Neighbouring columns under the
  same headings share them. A grouped heading never spans a frozen edge.
- `.width("14rem")` gives a minimum width in CSS units. `.decimals(n)` shows a number with
  exactly `n` decimals.
- `.badges(&[("paid", "success"), ("cancelled", "danger")])` shows `text`, `select` and `tags`
  values as badges (small colored labels). The tones are `success`, `warning`, `danger`,
  `info` and `neutral`. Values not listed get a neutral badge, and `.badges(&[])` makes every
  value a neutral badge.
- `.description("email")` shows another of the row's values in small type under this one.
  `.tooltip("email")` shows it when the mouse rests on the cell instead.
- `.wrap()` lets long text wrap onto more lines instead of widening the column. `.limit(40)`
  cuts text after 40 characters with `…` (the whole text shows on hover).
- `.link("/orders/{id}")` makes the value a link (`{id}` becomes the row's id). `.copyable()`
  adds a copy button.
- `.searchable()`, `.editable()`, `.merge()` and `.summary(…)` are covered below.

## Filters, search and chips

Each filterable heading opens a filter that suits its kind (see the table above). When several
filters are set, a row must pass all of them to show.

On top of the heading filters, there are:

- **Search.** If any columns are marked `.searchable()`, a search box appears in the toolbar.
  Every word typed must appear in one of those columns (as text, upper or lower case); only the
  first eight words count. The grid searches as you type. When the grid's model has a full-text index
  (`#[model(search = "title, body")]`, [search.md](search.md)), the box searches through it:
  all the index's columns count, also those the grid doesn't show, words match their prefixes
  and other forms, and rows come best match first until the user sorts by a heading.
- **Chips.** The active filters, the advanced filter's rules and the search show as chips under
  the toolbar. × clears one, and "Clear filters" clears them all.

> [!NOTE]
> **Only the grid's columns count.** The server reads only names that belong to the grid's
> filterable columns, and only the filters that fit each column's kind. (The query builder
> also checks column names again.) Anything else in the query string is ignored, not an error.
> So a user can't filter on a column you didn't put in the grid.

### The advanced filter

Heading filters are always "this AND that". For anything more, `.advanced_filter()` adds a
button to the toolbar. It opens a list of **rules**. Each rule has three parts:

1. a column;
2. a condition that fits the column's kind (like "greater than" for a number);
3. a value.

The rows must match **all** of the rules, or **any** of them: the user picks.

| Column kind | Conditions (`r.N.o`) |
|---|---|
| text, color, tags | `contains`, `not_contains`, `equals`, `not_equals`, `starts`, `ends`, `empty`, `not_empty` |
| number, money | `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `empty`, `not_empty` |
| date, datetime | `on`, `before`, `after`, `empty`, `not_empty` (whole days; of `APP_TIMEZONE` for datetime) |
| bool | `is_true`, `is_false` |
| select | `is`, `is_not`, `empty`, `not_empty` |

(`eq` is "equals", `ne` "not equal", `gt` "greater than", `gte` "greater than or equal",
`lt` "less than", `lte` "less than or equal".)

The rules live in the query string, like this: `match=any&r.0.c=total&r.0.o=gt&r.0.v=1000`
("any rule may match; rule 0: column `total`, condition `gt`, value 1000"). They apply together
with the headings' filters.

There are at most 20 rules. A rule is dropped (left out quietly) when:

- its column is unknown;
- its condition doesn't exist for that column's kind;
- its value doesn't fit (for example, a number that isn't one).

## Sorting and pages

Click a sortable heading to sort by it:

- first click: ascending (smallest or oldest first);
- second click: descending;
- third click: back to the grid's own order.

In the query string, `sort=-total` sorts by `total`, descending. Any sortable column is accepted,
and so is `id` (`sort=id` or `sort=-id`).

Empty values sort last in either direction, on SQLite and PostgreSQL alike.

The page buttons at the bottom also have the page-size menu.

## The user's choices

Each user can arrange the grid the way they like. The column menu (top right) can:

- show and hide columns, for the current screen size;
- move columns;
- freeze them at the left or right edge.

With a mouse, people can also:

- drag a heading to move its column;
- drag a heading's edge to resize the column;
- double-click the edge to get the automatic width back.

Once a heading's edge has keyboard focus, the arrow keys resize the column. "Reset columns"
returns to the grid's defaults.

Where the choices are kept:

- A logged-in user's choices are kept in the `grid_preferences` table (every app has it). A
  guest's are kept in the session.
- The page saves them with `POST /_renox/grid/{grid}/prefs` (a JSON `GridPrefs`).
  `DELETE /_renox/grid/{grid}/prefs` forgets them; that's what "Reset columns" does.
- `GridPrefs` has these fields. `None` in any of them means "use the grid's default".
  - `order`: the column order;
  - `left`, `right`: the columns frozen at each edge;
  - `compact` and `wide`: the columns shown under and over 768 px;
  - `widths`: column widths in CSS pixels, 40 to 2000.

`grid::save_prefs` writes them for a user. For example, a seeder (code that fills the database
with starting data) can give a user a default set of columns:

```rust
# use renox::prelude::*;
use renox::grid::{self, GridPrefs};
# async fn demo(db: Db, user_id: i64) -> Result {
let mut prefs = GridPrefs::default();
// On wide screens, show only these three columns.
prefs.wide = Some(vec!["number".into(), "customer".into(), "total".into()]);
grid::save_prefs(&db, user_id, "orders", &prefs).await?;
# Ok(())
# }
```

### Remembering filters

By default, leaving the page and coming back starts the grid fresh. `.remember()` changes that.
It keeps the filters, the search, the sort, the grouping and the page size in the session. So
coming back shows the grid as the user left it (and a filter they cleared stays cleared).

How it works:

- The grid's form then sends `state=1`.
- A request **with** `state` saves its values.
- A request **without** it uses the saved ones.

> [!TIP]
> A link into a remembered grid that carries its own filters should include `state=1`.
> Without it, the grid uses the remembered values (when there are any) instead of the link's.

## Row details

A row can do something when it's clicked: open a page, or open a details panel under it.

```rust
# use renox::prelude::*;
use renox::grid::{Column, Grid};
/// A grid whose rows open a details panel when clicked.
fn orders_grid() -> Grid {
    Grid::new("orders")
        .column(Column::text("number", "Order"))
        .row_url("/orders/{id}")
        .audit()   // created/updated by and at
        .details() // ...and the page's own `_details`
}
```

- `.row_url("/orders/{id}")` makes a click on a row (anywhere outside its buttons and links)
  open that URL. Ctrl-click (Cmd-click on a Mac) opens it in a new tab, and Enter opens the row
  that has keyboard focus.
- `.audit()` makes a click on a row (or on the chevron in the row tools) open a panel under it.
  The panel shows the row's audit values (who changed it, and when), if it has them:
  `created_by`, `created_at`, `updated_by` and `updated_at`.
- `.details()` adds to that panel whatever the tag's content draws for
  `column.key == "_details"`, after the audit values when there are any.
- With details on, a click on the row opens the details, and `row_url` becomes a link in the
  row tools instead.

## Editing in place

Editing in place means changing a value right in the grid, without opening a form page.

```rust
# use renox::prelude::*;
use renox::grid::{Column, Grid};
# #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, customer: String, items: i64 }

/// A grid with two editable columns, saved through `edit_url`.
fn orders_grid() -> Grid {
    Grid::new("orders")
        .column(Column::text("customer", "Name").editable())
        .column(Column::number("items", "Items").editable())
        .edit_url("/orders/{id}") // PATCH with the changed fields
}

/// Only the edited fields arrive, so every field is an `Option`.
#[derive(serde::Deserialize, Validate)]
struct OrderEdit {
    #[validate(max = 100)]
    customer: Option<String>,
    #[validate(between(1, 999))]
    items: Option<i64>,
}

/// Saves an edit from the grid. Renox checks the rules first (`Valid`).
async fn update(State(db): State<Db>, Path(id): Path<i64>, Valid(edit): Valid<OrderEdit>) -> Result<Toast> {
    let mut order = Order::find_or_404(&db, id).await?;
    // Change only the fields that were sent; leave the others alone.
    if let Some(customer) = edit.customer {
        order.customer = customer;
    }
    if let Some(items) = edit.items {
        order.items = items;
    }
    order.save(&db).await?;
    // The grid shows this message and reloads its page.
    Ok(Toast::success("Order saved."))
}
```

What's going on: two columns are marked `.editable()`, and `.edit_url(…)` says where to send
changes. The `update` handler receives only the fields that changed, checks them, saves the
order and answers with a toast.

How people edit:

- Double-click an editable cell, or press Enter or F2 on it, to open an editor that suits its
  kind. Enter, or leaving the cell, saves. Escape cancels.
- The pencil in the row tools edits all of the row's editable cells at once (✓ saves, ✕
  cancels).

What gets sent:

- Saving sends `PATCH {edit_url}` with only the edited fields, as a form (`customer=…`,
  `tags=a&tags=b`, `paid=true`).
- A 2xx answer (a success; its `Toast` shows) reloads the grid's page.
- A 422 answer from `Valid<T>` (the rules failed) shows the errors in the cells.

> [!WARNING]
> Cells are editable only when the grid has an `edit_url`. `custom` and merged columns can't be
> edited.

## Row order

Sometimes rows have an order that people set by hand, kept in a number column such as
`position`. `.reorder("position", "/orders/reorder")` lets people drag rows into order.

- It works while the grid is sorted by that column, ascending (`sort=position`). When the grid
  is sorted any other way, or grouped, the drag handle is off.
- The handle in the row tools drags a row, with a mouse or by touch. The arrow keys move it too.
- The page's new order is posted to the URL as `ids=4,2,9&offset=25`.
- `RowOrder::save` writes it in one transaction (all at once, or not at all). It leaves
  `updated_at` alone.

```rust
# use renox::prelude::*;
use renox::grid::RowOrder;
# #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, position: i64 }
/// Receives the new order of a page of rows and saves it in `position`.
async fn reorder(State(db): State<Db>, Form(order): Form<RowOrder>) -> Result<StatusCode> {
    order.save::<Order>(&db, "position").await?;
    // 204 No Content: it worked, and there's nothing to send back.
    Ok(StatusCode::NO_CONTENT)
}
```

## Merged cells

`.merge()` on a column draws one tall cell for neighbouring rows that have the same value. For
example, ten orders from "North" in a row show "North" once.

- Merged columns nest from left to right: a column merges only inside the groups of the merged
  columns before it (Region, then City).
- Sort by them, as in `sort_by("region,city")`, so equal values sit next to each other.
- Merging is off while rows can be dragged and while rows are grouped.
- Merged columns aren't editable.

```rust
# use renox::prelude::*;
use renox::grid::{Column, Grid};
/// Orders by region and city, with equal regions and cities merged.
fn regions_grid() -> Grid {
    Grid::new("regions")
        .column(Column::text("region", "Region").merge()) // equal neighbours share a cell...
        .column(Column::text("city", "City").merge())     // ...nested in their region
        .column(Column::text("number", "Order"))
        .sort_by("region,city")
}
```

## Actions and selection

Actions are buttons that do something to rows: mark them paid, delete them, and so on.

- A **row action** works on one row. It lives in the row's ⋯ menu.
- A **bulk action** works on all the rows the user has selected with checkboxes.

```rust
# use renox::prelude::*;
use renox::grid::{Action, Column, Grid, GridRequest, Selection};
# #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, status: String }

/// A grid with two bulk actions and two actions in each row's menu.
fn orders_grid() -> Grid {
    Grid::new("orders")
        .column(Column::text("status", "Status"))
        .bulk_action(Action::new("Mark paid", "/orders/paid"))
        .bulk_action(Action::new("Delete", "/orders/bulk-delete").confirm("Delete the selected orders?").danger())
        .row_action(Action::link("Open", "/orders/{id}"))
        .row_action(Action::new("Delete", "/orders/{id}").method("DELETE").confirm("Delete this order?").danger())
}

/// The "Mark paid" bulk action: sets the status of every selected order.
async fn mark_paid(State(db): State<Db>, request: GridRequest, Form(selection): Form<Selection>) -> Result<Toast> {
    // `selected` gives a query for exactly the chosen rows; `update` changes them all.
    let changed = orders_grid()
        .selected(Order::query(), &request, &selection)?
        .update(&db, &[("status", &"paid")])
        .await?;
    Ok(Toast::success(format!("{changed} orders paid.")))
}
```

How bulk actions work:

- `.bulk_action(…)` puts a checkbox at the start of each row, and one in the heading for the
  whole page. Selecting rows shows the actions above the grid.
- Once the whole page is selected, "Select all N matching" selects every row the filters
  match, on every page.
- A bulk action sends `ids=4,7` and `all=true|false` to its URL, along with the grid's query
  string. Read them with `Form<Selection>`.
- `grid.selected(query, &request, &selection)?` is the query for exactly those rows (at most
  10,000 ticked ids are read; more need `all`). When `all` is set, it's every row the filters
  match.

How row actions work:

- `.row_action(…)` puts actions in each row's ⋯ menu. `{id}` in the URL becomes the row's id.

What every action can do:

- `Action::new(label, url)` sends a request. It's a `POST` unless `.method("PATCH" | "PUT" |
  "DELETE")` says otherwise.
- `Action::link(label, url)` is a plain link.
- `.confirm("…")` asks first, in a dialog.
- `.danger()` shows the action in red.
- After a 2xx answer (a `Toast` shows), the grid reloads its page.

## Summaries and groups

A **summary** is a figure under a column, like a total. A **group** gathers rows with the same
value (say, the same region) under a heading of their own.

- A column's `.summary(Summary::Sum)` puts a figure in a footer that stays at the bottom of the
  grid. The figure covers every row the filters match, not just the page.
- `Sum`, `Average` and `Range` (smallest–largest) are for `number` and `money` columns. `Count`
  (rows with a value) works on any column.
- A column can have several (`.summary(Summary::Sum).summary(Summary::Average)`). A summary that
  doesn't fit its column is ignored.
- `.groups(&["region", "status"])` adds a "Group" choice to the toolbar, and
  `.group_by("region")` starts out grouped.
- Grouped rows come group by group. Each group has a heading that shows its value and its row
  count (a click folds the group away), and a subtotal row with the group's own summaries.
- Group by the model's own columns.
- Merged cells and dragging rows are off while the grid is grouped.

## Empty grids

`.empty_state("No orders yet", Some("Orders show up here as customers buy."))` says what to show
in a grid that has no rows yet: a heading, and an optional line under it.

The tag's content can add buttons there, for `column.key == "_empty"` (a "New order" button, for
example).

A grid that is empty because of its filters is different: it says nothing matches, and offers
to clear the filters.

## Cards on phones

Wide tables are hard to read on a small screen. `.cards_on_mobile()` turns each row into a card
on screens under 768 px wide.

- A card shows the columns picked for small screens, each as a label and a value, with the row
  tools underneath.
- The headings are hidden there, so sorting and the column filters move into the toolbar.
- Wider screens keep the table.

## Columns from other tables

A column can show data from another table, without you writing a join (SQL that combines
tables).

```rust
# use renox::prelude::*;
use renox::grid::{Column, Grid};
/// Orders, plus three columns computed from other tables.
fn orders_grid() -> Grid {
    Grid::new("orders")
        .column(Column::text("number", "Order"))
        // customers.tier where customers.id = orders.customer_id
        .column(Column::related("tier", "Tier", "customers", "customer_id", "tier"))
        // how many order_notes have order_id = orders.id
        .column(Column::count_of("notes", "Notes", "order_notes", "order_id"))
        // the sum of order_lines.weight over the order's lines
        .column(Column::sum_of("weight", "Weight", "order_lines", "order_id", "weight"))
}
```

What's going on:

- `Column::related` shows a value from the row this one belongs to: here, the tier of the
  order's customer. It behaves like a text column; `.numeric()` makes it a number column
  (filtered with a from–to range).
- `Column::count_of` counts the rows that point at this one: here, the order's notes.
- `Column::sum_of` adds up one column of those rows: here, the weight of the order's lines.
  Both `count_of` and `sum_of` are number columns.

Good to know:

- They are SQL subqueries (small queries inside the main one). So they sort, filter, search and
  take advanced-filter rules like other columns.
- A page fetches each one in a single query, not one query per row.
- That query reads exactly the rows the page shows, without the model's default scope. So a
  staff page over tenant data, which starts from `Model::unscoped()`, still fills them in.
- Table and column names may only have letters, digits and `_`.

- `.summary(…)` and `Grid::groups` work on them too: the footer and each group's subtotal are
  worked out by the database over every row the filters match, like for the model's own columns.

> [!WARNING]
> `tags` filters need a column of the model itself.

### A computed column

`Column::computed(sql)` makes any column's value a SQL expression of your own, where `{T}` stands
for the model's table. A stock grid can show the stock value (what is on hand times the variant's
cost) with no database view:

```rust
# use renox::grid::{Column, Summary};
# fn demo() -> Column {
Column::money("value", "Stock value")
    .computed(
        "{T}.\"on_hand\" * (SELECT \"cost\" FROM \"variants\" WHERE \"variants\".\"id\" = {T}.\"variant_id\")",
    )
    .summary(Summary::Sum)
# }
```

It sorts, filters, searches, is summed and grouped by like the related columns above. Write the
expression in your code, never from a request: values you need go into the model's columns or a
subquery, not into the string.

A `select` column whose options are all whole numbers (a `store_id`) filters an integer column:
the grid compares the column as text, which both databases accept.

## Exports

`.exports()` adds an export menu to the toolbar. Its links come back to the same handler, with
`?export=…` in the URL. So the handler checks for an export first, before it draws the page:

```rust
# use renox::prelude::*;
# use renox::grid::{Column, Grid, GridRequest};
# #[derive(Model, serde::Serialize, Default)] struct Order { id: i64, number: String }
# fn orders_grid() -> Grid { Grid::new("orders").column(Column::text("number", "Order")).exports() }
/// The grid's page, which also answers the export menu's links.
async fn index(request: GridRequest) -> Result<Response> {
    // If the URL asks for an export, send the file instead of the page.
    if let Some(file) = orders_grid().export(Order::query(), &request).await? {
        return Ok(file); // ?export=csv|xlsx|print
    }
    let page = orders_grid().page(Order::query(), &request).await?;
    Ok(view("orders/index.html", context! { orders => page }).into_response())
}
```

What goes into an export:

- every row the filters match (not just the page), up to `grid::MAX_EXPORT_ROWS` (100,000);
- sorted as on screen;
- the columns the user shows on wide screens. `custom` columns are left out.

The three formats:

- **CSV** (a plain text file that spreadsheets open):
  - UTF-8 with a BOM (a few marker bytes at the start, so Excel reads it as UTF-8);
  - headings like `Amounts / Total` for grouped headings;
  - labels for choices and tags, and numbers without separators;
  - text that starts with `=`, `+`, `-`, `@`, a tab or a carriage return gets a leading `'`, so
    a spreadsheet doesn't run it as a formula (number and money columns are left alone).
- **Excel** (needs the `xlsx` feature: `renox = { …, features = ["xlsx"] }`; it uses the
  `rust_xlsxwriter` crate):
  - grouped headings merged as on screen;
  - numbers and dates stored as real numbers and dates;
  - the headings and the frozen-left columns stay frozen.

  Without the feature, the menu has no Excel link, and `export=xlsx` answers 400 (bad request).
- **Print**: a plain page with a button to print it or save it as a PDF. It prints landscape,
  with the headings repeated on every printed page.

### Exports outside the grid's page

`Grid::export_as(query, format, &request)` makes the same files from any query, without the
grid's page: an "Export" link on a record's page, in an [action group](ui.md#action-groups), or
behind its own route. It always answers with the file.

```rust
# use renox::prelude::*;
use renox::grid::{Column, ExportFormat, Grid, GridRequest};
# #[derive(Model, serde::Serialize, Default)]
# #[model(table = "stock_movements")]
# struct StockMovement { id: i64, product_id: i64, quantity: i64, note: String }
fn ledger() -> Grid {
    Grid::new("ledger")
        .column(Column::number("quantity", "Change"))
        .column(Column::text("note", "Note"))
}

/// GET /products/{id}/ledger.csv: one product's movements, newest first.
async fn export_ledger(Path(id): Path<i64>, request: GridRequest) -> Result<Response> {
    let movements = StockMovement::query().where_(StockMovement::PRODUCT_ID.eq(id)).order_by_desc(StockMovement::ID);
    ledger().export_as(movements, ExportFormat::Csv, &request).await
}
```

- The columns are the ones the grid shows by default on wide screens, in their order
  (`hidden()` and `custom` columns are left out). The user's column choices, the URL's
  filters and its sort don't apply: the query decides which rows and in what order.
- `ExportFormat` is `Csv`, `Xlsx` (the `xlsx` feature) or `Print`. It reads from a route
  parameter too, so `GET /orders/export/{format}` with `Path(format): Path<ExportFormat>` takes
  `csv`, `xlsx` or `print` (anything else is a 404). `ExportFormat::Xlsx.available()` says
  whether this build can make Excel files.
- `grid::Workbook` puts several queries in one Excel file, a sheet each (the `xlsx` feature;
  without it `sheet` answers 400). Each sheet takes a grid's default wide columns like
  `export_as`; the title must be a valid sheet name (31 characters at most, none of
  `[ ] : * ? / \`, unique), else 400.

  ```rust
  # use renox::prelude::*;
  use renox::grid::{Column, Grid, GridRequest, Workbook};
  # #[derive(Model, serde::Serialize, Default)] struct Sale { id: i64, total: i64 }
  # #[derive(Model, serde::Serialize, Default)] struct Refund { id: i64, amount: i64 }
  /// GET /reports/summary.xlsx
  async fn summary(request: GridRequest) -> Result<Response> {
      let sales = Grid::new("sales").column(Column::money("total", "Total"));
      let refunds = Grid::new("refunds").column(Column::money("amount", "Amount"));
      Workbook::new("summary")
          .sheet("Sales", &sales, Sale::query().order_by(Sale::ID), &request).await?
          .sheet("Refunds", &refunds, Refund::query().order_by(Refund::ID), &request).await?
          .into_response()
  }
  ```
- A large export can run in a job instead: build a `GridRequest::new(…)` there from the
  filters the page had, export into storage, and tell the user when the file is ready.

## Polling

Polling means checking again and again for new data. `.poll(30)` reloads the grid's page every
30 seconds (the smallest you can set is 5).

It only reloads while the browser tab is visible, and while nobody is editing, selecting or
filtering in the grid, so it never interrupts the user.

## Several grids on a page

Two grids on one page would fight over the same names in the query string (both want `page`,
for example). `.prefix("orders")` fixes that: it puts a prefix on the grid's query string names,
so they become `orders.page`, `orders.q.number`, and so on.

Each grid also keeps the page's other query string values (another grid's, a tab's) in its own
links and forms. So the grids page and filter independently.

> [!IMPORTANT]
> Give each grid its own id as well, not just its own prefix.

## The query string

This table lists every value the grid reads from the URL. You rarely write them by hand, but
they're handy for links ("show the paid orders") and for tests.

Every value belongs to one grid. With `.prefix("orders")`, each name starts with `orders.`.

| Name | What it does |
|---|---|
| `page=2` | the page |
| `per_page=50` | rows per page; only the sizes the grid offers (10, 25, 50, 100 and its `.per_page(n)`) |
| `sort=-total` | sort by a sortable column or `id`; `-` for descending |
| `search=iced coffee` | the toolbar search; every word must appear in a searchable column |
| `q.number=A%` | a text filter (`%` makes it a pattern) |
| `m.number=starts` | how `q.` matches: `contains` (default), `starts`, `ends`, `equals` |
| `min.total=1000`, `max.total=…` | a number or money range (money in whole units: `min.total=40.5` is 4050 cents) |
| `from.ordered_on=2026-01-01`, `to.ordered_on=…` | a date range (`to` includes its day) |
| `in.status=paid` (repeated) | select and tags choices; bool uses `in.paid=1` / `in.paid=0` |
| `group=region` | group by one of `.groups`; empty for no groups |
| `match=all\|any`, `r.0.c=total`, `r.0.o=gt`, `r.0.v=1000` | the advanced filter's rules: column, condition, value |
| `export=csv\|xlsx\|print` | an export ([Exports](#exports)) |
| `state=1` | with `.remember()`: these values replace the remembered ones |

Only the grid's columns filter or sort. Anything else is ignored.

## Translations

The grid's texts (the toolbar, the filters, the column menu, the advanced filter's conditions,
the page buttons) come in English.

To change them, or translate them into another language, add keys under `ui.grid.*` to your
app's `lang/<locale>.json` (for example `lang/es.json` for Spanish). Some of the keys:

- `ui.grid.columns`, `ui.grid.filter`, `ui.grid.search`;
- `ui.grid.clear_all`, `ui.grid.empty`;
- `ui.grid.row_word` (`"row|rows"`: the word for one row, then for several);
- `ui.grid.op.contains` (the name of a filter condition).

The full list is in `crates/renox-core/src/i18n.rs`.

## Coming from Laravel and Filament

> [!NOTE]
> **Coming from Laravel:** Filament is a popular Laravel package for admin pages, and its
> tables are what Renox's grid is measured against. If you know Filament, this table says what
> each of its table features is called in Renox. If you don't, you can skip it.

| Filament tables | Renox |
|---|---|
| `Table::make()->columns([...])` | `Grid::new("orders").column(…)` |
| `TextColumn::make('number')` | `Column::text("number", "Order")` (also `number`, `money`, `date`, `datetime`, `bool`, `select`, `tags`) |
| `ImageColumn`, `ColorColumn`, `IconColumn::boolean()` | `Column::image` (`.round()`), `Column::color`, `.icons()` |
| `->badge()->color(fn …)` | `.badges(&[("paid", "success")])` |
| `->description()`, `->tooltip()`, `->wrap()`, `->limit(40)`, `->url()`, `->copyable()` | `.description("email")`, `.tooltip("email")`, `.wrap()`, `.limit(40)`, `.link("/orders/{id}")`, `.copyable()` |
| `->searchable()`, `->sortable()`, `->toggleable()` | `.searchable()`, sortable by default (`.sortable(false)`), the column menu |
| `->defaultSort('ordered_on', 'desc')`, `->paginated([10, 25])` | `.sort_by("-ordered_on")`, `.per_page(n)` |
| `Filter` / `SelectFilter` / `TernaryFilter` | a filter in every heading, by kind |
| `QueryBuilder` filter | `.advanced_filter()` |
| `->persistFiltersInSession()`, `->poll('30s')` | `.remember()`, `.poll(30)` |
| `->recordUrl(…)`, `->emptyStateHeading(…)` | `.row_url("/orders/{id}")`, `.empty_state(…)` |
| `Action`, `BulkAction`, `->requiresConfirmation()` | `.row_action(Action::…)`, `.bulk_action(Action::…)`, `.confirm("…")` |
| `TextInputColumn`, `SelectColumn` | `.editable()` + `.edit_url(…)` |
| `->reorderable('position')` | `.reorder("position", url)` + `RowOrder::save` |
| `->summarize(Sum::make())`, `->groups([...])` | `.summary(Summary::Sum)`, `.groups(&[…])` / `.group_by(…)` |
| `->contentGrid([...])` (cards) | `.cards_on_mobile()` |
| `TextColumn::make('customer.tier')`, `->counts('notes')`, `->sum('lines', 'weight')` | `Column::related`, `Column::count_of`, `Column::sum_of` |
| `ExportAction` | `.exports()` + `grid.export(…)` |
| Column groups (`ColumnGroup`) | `.under(["Amounts"])` |
