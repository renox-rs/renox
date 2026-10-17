# Types: from the form to the database and back

A value in a web app travels a long way: someone types it into a form, your Rust code reads
it, and the database stores it. This page tells you which type to use at each step, so the
value arrives safely at every one.

### In this guide

- one big table: for each kind of form field, the Rust type and the database column to use;
- enums (a fixed list of choices, like a status);
- what browsers really send from forms, and how Renox reads it;
- advice for the tricky cases: money, quantities, places, custom fields, time zones, keys, deleted rows and PostgreSQL.

### Words you'll meet

| Word | What it means |
|---|---|
| **form field** (or input) | A box, checkbox or menu in an HTML form, like `<input type="number">`. |
| **model** | A Rust struct that matches a database table: each field is a column, each value one row. |
| **column type** | What kind of value a database column holds: `TEXT`, `INTEGER`, `DATE`… |
| **SQLite**, **PostgreSQL** | The two databases Renox supports. SQLite is one file; PostgreSQL is a server. They name some types differently. |
| **migration** | A SQL file that creates or changes a table. It's where you write the column types. |
| **NULL**, **nullable** | NULL means "no value". A nullable column may hold NULL; in Rust that's `Option<T>`, and NULL is `None`. |
| **serde** | The Rust library that turns form data and JSON into structs and back. |
| **JSON** | A text format for structured data, like `["red", "blue"]` or `{"size": "L"}`. |
| **key** (or id) | The column that names a row, always called `id` in a Renox model. |

### The table

The form inputs are exercised by the bike shop's `/about/fields` page
([src/app/about/fields.rs](../examples/bikeshop/src/app/about/fields.rs): a form with every input, samples keyed
by `Uuid`, its tests run on SQLite and PostgreSQL). Every row is checked by Renox's own
tests, on SQLite and PostgreSQL.

How to read it: find the form field you're adding in the first column. The second column is
the type to give the field in your Rust form struct **and** in your model. The last two
columns are what to write in your migration's `CREATE TABLE`, for SQLite or PostgreSQL.

| HTML input | Rust type (form and model) | SQLite column | PostgreSQL column |
|---|---|---|---|
| `<input>`, `type=email/url/tel/search/password` | `String` | `TEXT` | `TEXT` |
| `<textarea>` | `String`, or `Option<String>` (empty → `None`) | `TEXT` | `TEXT` |
| `type=number` | `i64` (use `i64` for whole numbers; PostgreSQL has no unsigned types) | `INTEGER` | `BIGINT` |
| `type=number`, smaller ranges | `i16`, `i32` (also `i8` on SQLite: on PostgreSQL `i8` reads only the one-byte `"CHAR"` type, not `SMALLINT`; `u8`, `u16`, `u32` only in builds without the `postgres` feature, whatever database the app runs on: PostgreSQL has no unsigned columns) | `INTEGER` | `SMALLINT`, `INTEGER` |
| `type=number step=0.01` (measures) | `f64` | `REAL` | `DOUBLE PRECISION` |
| `type=number step=0.01`, single precision | `f32` | `REAL` | `REAL` |
| money | `i64` in the smallest unit (cents), never `f64` | `INTEGER` | `BIGINT` |
| money in its own currency (`type=money` plus a currency `<select>`) | `Money` (`renox::money`), with `#[model(money)]`: a string and a code in the form (`#[model(form)]` writes both) | `INTEGER` amount + `TEXT` code | `BIGINT` amount + `TEXT` code |
| an exact decimal with units (`type=quantity scale=3 unit=kg`, plus a unit `<select>`) | `Quantity<3>` (`renox::quantity`): a string in the form (`#[model(form)]` writes it); the unit is an ordinary `String` field | `INTEGER` (thousandths) | `BIGINT` (thousandths) |
| `type=checkbox` (one) | `bool`: with `Valid<T>`, `on` (or `1`, `yes`, `checked`) → `true`, unchecked (nothing sent; or `off`, `0`, `no`) → `false` (see [Checkboxes](#checkboxes)) | `INTEGER` 0/1 | `BOOLEAN` |
| `<select>` | an enum with `#[derive(DbEnum)]` | `TEXT` | `TEXT` |
| `<select multiple>`, checkboxes sharing a name | `Vec<String>` with `#[serde(default)]`; stored as `Json<Vec<String>>` | `TEXT` | `JSONB` |
| `type=date` | `NaiveDate` | `TEXT` | `DATE` |
| `type=time` | `NaiveTime` | `TEXT` | `TIME` |
| `type=datetime-local` (no seconds needed with `Valid<T>`) | `NaiveDateTime` | `TEXT` | `TIMESTAMP` |
| (set by Renox) `created_at`, `updated_at` | `Option<DateTime>` (UTC), or a plain `DateTime` | `TEXT` | `TIMESTAMPTZ` |
| (set by Renox) `deleted_at`, with `#[model(soft_deletes)]` | `Option<DateTime>` (`None` = not deleted) | `TEXT` | `TIMESTAMPTZ` |
| `type=file` | `Upload`; store it and keep its key as `String` | `TEXT` | `TEXT` |
| the kit's `tags_input` | `Vec<String>` with `#[serde(default)]`; stored as `Json<Vec<String>>` | `TEXT` | `JSONB` |
| the kit's `key_value` (`meta[0][key]`, `meta[0][value]`) | `KeyValues` with `#[serde(default)]`; stored as `Json<KeyValues>` (a list of pairs, so the order holds) | `TEXT` | `JSONB` |
| the kit's `repeater` (`lines[0][name]`, `lines[0][qty]`) | `Vec<Line>` of a `Deserialize` struct, checked with `v.nested("lines", &self.lines)` | (a table of its own, or `Json<Vec<Line>>`) | |
| (structured data) | `Json<T>` for any serde type (`Json<serde_json::Value>` for free-form data) | `TEXT` | `JSONB` (or `JSON`, `TEXT`); a bare `serde_json::Value` field needs `JSONB` or `JSON` |
| (public ids) | `uuid::Uuid`, with renox's `uuid` feature (`renox::uuid::Uuid`) | `BLOB` | `UUID` |
| (sortable public ids) | `renox::db::Ulid` | `TEXT` | `TEXT` |
| (bytes) | `Vec<u8>` | `BLOB` | `BYTEA` |
| (secrets: an id number, a bank account, an API key) | `Encrypted<T>` (`renox::db::Encrypted`, any serde `T`), sealed with `APP_KEY` | `TEXT` | `TEXT` |
| any optional field | `Option<T>` | nullable | nullable |

Some words in the table, explained:

- **Rows in brackets**, like "(public ids)", aren't a form field: they're values your code or
  Renox sets.
- **"the kit's"** means a field from Renox's UI kit (ready-made form parts; see [ui.md](ui.md)).
- **`Option<T>`** is for any field that may be left empty. In the migration, leave out
  `NOT NULL` for that column.
- **`#[serde(default)]`** tells serde "if this field is missing, use an empty value". A list
  where nothing was picked is not sent at all; with `#[serde(default)]` it becomes an empty
  list.
- **`Json<T>`** stores any Rust value as JSON text in one column.
- **`JSONB`** is PostgreSQL's JSON column type.
- **`Upload`** is a file sent with the form. You store the file, then save its key (a name
  that finds it again) in a `String` column.
- **`Encrypted<T>`** is scrambled with the app's secret key (`APP_KEY`) before it's stored, so
  someone who reads the database can't read the value. In your app it's the plain value again,
  and it serializes as the plain value too: a model sent as `Json(model)` or passed to a
  template shows the secret. Only `Debug` hides it (`Encrypted(..)`). Keep such a field out of
  API answers and views (a separate struct for the output, or `#[serde(skip_serializing)]`).
- **A model field that isn't a column** gets `#[model(skip)]`: the model never writes it, and
  after a load it holds its `Default` value. Use it for values you fill in yourself, like a
  count loaded separately.
- **UTC** is the world's reference time zone, with no summer time.

## Enums

An enum is the right type for a `<select>` with a fixed list of choices, like a product's
status. Add `#[derive(DbEnum)]` and Renox can store it, read it from forms and print it:

```rust
use renox::prelude::*;

/// A product's status. Each variant is stored as text, in snake_case.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Default)]
enum Status {
    /// The status a new product starts with.
    #[default]
    Draft,          // stored as "draft"
    InStock,        // "in_stock"
    /// `rename` picks the stored text yourself.
    #[db(rename = "gone")]
    SoldOut,
}
```

What's going on:

- Each variant is stored as text: `Draft` as `"draft"`, `InStock` as `"in_stock"`.
- `#[db(rename = "gone")]` stores `SoldOut` as `"gone"` instead of `"sold_out"`.
- `#[default]` makes `Draft` the value of `Status::default()`.

The same enum is a model field, a form field (`<option value="{{ s }}">` for `s in Status::ALL`),
a JSON value and a template value, always as its text.

A value that isn't a variant is a validation error: someone who sends `status=stolen` gets an
error message, not a crash.

## Forms

Browsers don't always send what you'd expect. Here is what they send, and how Renox reads it.

### Checkboxes

Browsers send only checked checkboxes, and send them as `on`. `Valid<T>` (Renox's form checker)
reads that as `true`, and a missing one as `false`.

With `Valid<T>`, a `bool` field also accepts `1`, `yes`, `checked` and `true` (and `off`, `0`,
`no`, `false` or an empty value for `false`). So a hidden input or an API client (a program,
not a browser) can send either.

> [!WARNING]
> This is `Valid<T>`'s work. The plain `Form<T>` extractor (from the prelude) accepts only
> `true` and `false`: it refuses a checkbox's `on`, and a missing checkbox is an error unless
> the field has `#[serde(default)]`. Read forms with checkboxes through `Valid<T>`.

### Lists: multi-selects and checkbox groups

A multi-select or checkbox group repeats its name (`colors=black&colors=red`).

Declare the field as `Vec<T>` with `#[serde(default)]`, so that nothing chosen becomes an empty
list.

### Dates and times

`datetime-local` sends `2026-10-01T10:30`, without seconds. `Valid<T>` accepts it as a
`NaiveDateTime` (it adds `:00`); the plain `Form<T>` refuses it.

To show values back in an edit form, write them as they come. Dates, times and date-times
serialize in the formats their inputs expect (`2026-10-01`, `07:30:00`,
`2026-10-01T10:30:00`).

### Wrong values

A field that doesn't parse (`weight=heavy`, `size=huge`) is reported together with every other
field's errors. The person sees all their mistakes at once, not one at a time.

### Nested names: rows inside a form

Some fields have names with brackets: `lines[0][name]`, `meta[1][key]`. The kit's `repeater`
and `key_value` send them, one numbered row after another.

When a form has such names, `Valid` (Renox's form checker) reads the whole form as a tree:

- a `Vec` of structs, maps, and every type above still parsing from its text;
- errors are keyed with dots (`lines.0.name`), which is what `error('lines[0][name]')`,
  `old('lines[0][name]')` and the kit's error slots use;
- a row's field is labelled by its own name ("The name field is required."); translate it as
  `renox.validation.attributes.lines.*.name` or `.name`;
- empty values stay (an `Option` reads "" as `None`), so rows keep their numbers.

More on checking forms in [validation.md](validation.md).

## Choosing

Advice for the cases where the wrong type causes trouble later.

### Money

For a shop with one currency, use `i64` in the smallest unit, `price: i64 // cents`, and format
it for display (the `money` template filter, in `APP_CURRENCY`, which is `USD` by default). So
12.50 dollars is stored as `1250` cents, and `{{ 1250 | money }}` shows `$12.50`. A currency
without decimals (IDR, JPY) stores the amount as it is.

When records carry their own currency (invoices in dollars and dirhams, a deposit in yen), use
`Money` from `renox::money`: the amount in the smallest unit plus its ISO 4217 `Currency`.
`#[model(money)]` stores it in two columns, `<field>` (`BIGINT`) and `<field>_currency`
(`TEXT`); `Option<Money>` makes both columns nullable, set or cleared together. Several fields
can share one currency column with `#[model(money(currency = "currency"))]`.

```rust
# use renox::prelude::*;
use renox::money::{Money, Rounding};

#[derive(Model, serde::Serialize, Debug)]
#[model(table = "invoices", form)]
struct Invoice {
    id: i64,
    #[model(money(currency = "currency"))]
    subtotal: Money,
    #[model(money(currency = "currency"))]
    tax: Money,
    #[model(money)]
    deposit: Option<Money>, // deposit + deposit_currency, both NULL when none
}

# fn demo() -> Result<(), renox::money::MoneyError> {
let subtotal = Money::aed(10_000);
let tax = subtotal.percent(500, Rounding::HalfUp)?; // 5%, the rounding is always your choice
assert_eq!(subtotal.checked_add(tax)?.to_string(), "AED 105.00");
// Different currencies never add up by accident.
assert!(Money::usd(1).checked_add(Money::jpy(1)).is_err());
// A split loses no minor unit: 100 in three parts is 34, 33, 33.
assert_eq!(Money::usd(100).allocate(3)?[0], Money::usd(34));
# Ok(())
# }
```

Show it with `{{ invoice.total | money }}` (its own currency and decimals: `¥1,250`,
`KWD 12.345`), `Column::money(..).currency_from("total_currency")` in a grid and
`Field::money(..).currency_from(..)` in renox-admin. In a form, `<rx-input type="money"
currency="JPY"/>` steps by the currency's smallest unit and the server rejects too many
decimals (`10.5` yen) instead of rounding. The bike shop's [`/about/fields`](../examples/bikeshop/src/app/about/fields.rs)
keeps a deposit this way, in USD, JPY or KWD.

Why not a float (`f64`)? Floats round: `0.1 + 0.2 != 0.3`. And why not a decimal type? sqlx
(the database library Renox uses) deliberately has no decimal type on SQLite, so a `Decimal`
would behave differently on the two databases. A fixed currency stays a plain `i64`.

### Quantities and units

Weights, lengths and volumes need decimals but not floats. `Quantity<SCALE>` from
`renox::quantity` is an exact decimal kept as an `i64` in units of 10^-`SCALE`: `Quantity<3>` counts
thousandths, so 2.5 kg is `2500` and shows as `2.500`. It is one `BIGINT` column on SQLite and
PostgreSQL; `Option<Quantity<3>>` makes it nullable. A quantity carries no unit: a record keeps
its unit in a `String` column of its own (`stock` + `stock_unit`), and a `UnitTable` (`Units`)
converts between units with exact decimal factors.

```rust
# use renox::prelude::*;
use renox::money::Rounding;
use renox::quantity::{Quantity, QuantityError, Units};

#[derive(Model, serde::Serialize, Debug)]
#[model(table = "bins", form)] // BinForm: "12.5" texts checked with quantity(3); fill() builds the Quantity
struct Bin {
    id: i64,
    stock: Quantity<3>,         // BIGINT NOT NULL, thousandths
    stock_unit: String,         // "kg", "g", "t"
    spare: Option<Quantity<3>>, // BIGINT, NULL when none
}

# fn demo() -> Result<(), QuantityError> {
// Parsing says what happens to extra digits: a rounding, or an error. There is no default.
let kg = Quantity::<3>::parse_exact("2.5")?;
assert_eq!(kg.to_string(), "2.500");
assert_eq!(Quantity::<3>::parse("0.0005", Rounding::HalfUp)?.raw(), 1);
assert_eq!(Quantity::<3>::parse_exact("0.0005"), Err(QuantityError::Inexact));
// Arithmetic is checked and exact: 0.1 + 0.2 is 0.3.
let sum = Quantity::<3>::parse_exact("0.1")?.checked_add(Quantity::parse_exact("0.2")?)?;
assert_eq!(sum.raw(), 300);
// Units convert through their base, with the rounding you choose.
let units = Units::new().base("kg").unit("g", "kg", "0.001").unit("t", "kg", "1000").build()?;
assert_eq!(units.convert(kg, "kg", "g", Rounding::HalfEven)?.to_string(), "2500.000");
assert_eq!(units.convert(Quantity::<3>::parse_exact("1500")?, "g", "kg", Rounding::HalfEven)?.to_string(), "1.500");
# Ok(())
# }
```

Typed columns compare the scaled integer, so `Bin::STOCK.lt(Quantity::<3>::parse_exact("5")?)`,
`order_by` and `sum` are exact on both databases (5.000 is not 4.999; `sum` returns a `Quantity`).
Show it with `{{ bin.stock | quantity(unit=bin.stock_unit) }}` (all three digits, the locale's
separators), `Column::quantity("stock", "Stock").unit_from("stock_unit")` in a grid (one summary per
unit) and `Field::quantity(..).unit_from(..)` in renox-admin. In a form, `<rx-input type="quantity"
scale="3" unit="kg"/>` steps by the scale and the server refuses a fourth decimal (`2.5004`)
instead of rounding; `.quantity(3)`, `.quantity_min("0")` and `.unit(&units)` are the rules.

Two pieces of advice. The scale lives in the Rust type, not in the schema: `db:check` and
`make:migration --auto` see a `BIGINT` either way, so note the scale in a comment on the column
(`-- thousandths`). And pick the scale by the precision you measure, not by the currency: cents
(`Money`, `i64`) are the smallest unit of a price, thousandths are a quantity's chosen precision.
The bike shop's [`/about/fields`](../examples/bikeshop/src/app/about/fields.rs) keeps a load limit
this way, in kg, g or t.

### Places: points, circles and areas

A check-in, a delivery stop or a service area is a place on the earth. `renox::geo` has the
three shapes, with no GIS stack behind them: a `Point` (latitude and longitude, checked), a
`Circle` (a centre and a radius in metres) and a `Polygon` (an outer ring and optional holes).
Everything is plain Rust and runs on the server, so a reported location can be verified
instead of trusted.

```rust
use renox::geo::{Circle, Point, Polygon};

# fn demo() -> Result<(), renox::geo::GeoError> {
// The API order is (lat, lng). GeoJSON is the other way round: [lng, lat].
let here = Point::new(25.2048, 55.2708)?;
assert!(Point::new(91.0, 0.0).is_err()); // out of range or not finite: an error, never a panic

// Distances are haversine, in metres, the short way round (also across the antimeridian).
let site = Circle::new(Point::new(25.2050, 55.2710)?, 150.0)?;
assert!(site.contains(here)); // on the radius counts as inside

// A GeoJSON Polygon (or a Feature holding one) reads as is; holes are supported.
let area: Polygon = renox::serde_json::from_str(
    r#"{"type":"Polygon","coordinates":[[[55.0,25.0],[56.0,25.0],[56.0,26.0],[55.0,26.0],[55.0,25.0]]]}"#,
)
.unwrap();
assert!(area.contains(here));
assert!(here.distance_to(Point::new(25.3, 55.3)?) < 15_000.0);
# Ok(())
# }
```

The edges are decided once: a point exactly on a circle's radius, on a polygon's edge or on a
vertex is inside; the boundary of a hole is inside the polygon too (the hole is open). A
polygon is tested on the flat plane of longitude and latitude, with longitudes shifted
relative to its first vertex, so one that crosses the antimeridian works; one that spans more
than 180 degrees of longitude is not supported. A ring needs three distinct vertices, and a
radius must be positive and finite.

A `Point` reads from the JSON `{"lat": .., "lng": ..}`, from a GeoJSON `Point` and from the
form fields `lat` and `lng`, so a struct field `spot: Option<Point>` is filled by the fields
`spot[lat]` and `spot[lng]` that the `map_picker` block posts ([blocks.md](blocks.md#maps)).
A `Circle` is `{center, radius}` and a `Polygon` the GeoJSON `Polygon`; a picker posts a circle
as a GeoJSON `Point` with a `radius` in metres and a polygon as a GeoJSON string, which you
read with `serde_json::from_str`. A bad coordinate is a validation error: `.latitude()`,
`.longitude()` and `.point()` (a JSON or GeoJSON point as text) are rules
([validation.md](validation.md)). Store a point as two `REAL` columns (`lat`, `lng`) or as
JSON text; there is no column type and no migration for it.

### Custom fields: columns an administrator adds

Sometimes the person who runs the app needs a field you did not plan: a passport number on a
party, a segment on a customer. A migration is not the answer when they add it themselves.
Custom fields (`renox::custom_fields`) keep the *definitions* in a `custom_fields` table
(migration `00010101000430`, created with the framework's tables) and the *values* in one JSON
column of the model. A definition has a model (the table name), a stable `key`
(`[a-z][a-z0-9_]*`), a kind (text, number, date, bool, choice, reference to a record of another
model, or a file stored on a disk), rules (required, min, max, options), a label per locale, a
free `meta` JSON that the framework never interprets, a position and an active flag.

```rust
# use renox::prelude::*;
use renox::custom_fields::{CustomField, CustomValues};
use renox::db::Json;

#[derive(Model, serde::Serialize, Default, Debug)]
#[model(table = "parties", form, custom_fields = "extra")] // must name a Json<CustomValues> column
struct Party {
    id: i64,
    #[form(validate(required))]
    name: String,
    extra: Json<CustomValues>, // TEXT on SQLite, JSONB on PostgreSQL
}

# async fn demo(db: &Db) -> Result {
CustomField::text("parties", "passport_no")
    .label("Passport no.")              // the fallback locale ("en")
    .label_in("es", "N.º de pasaporte") // the request's locale wins; then the fallback; then the key
    .required()
    .max(20)
    .save(db)
    .await?;
CustomField::choice("parties", "segment").options(&["retail", "corporate"]).save(db).await?;

// Filter and sort on a key; `with_custom_fields` checks the keys against the definitions.
let retail = Party::query()
    .where_(Party::EXTRA.json("segment").eq("retail"))
    .order_by_json_desc(Party::EXTRA.json("segment"))
    .with_custom_fields(db)
    .await?
    .get(db)
    .await?;
# let _ = retail;
# Ok(())
# }
```

A key never changes, because the stored values are keyed by it: saving a definition again
updates its label, rules, `meta`, position and active flag, so renaming a field is changing its
label, and deactivating one hides it but keeps what was stored. `CustomField::for_model` is
cached per model and dropped on every write (other servers pick a change up after a restart, or
`CustomField::forget`).

With `custom_fields = "extra"` the generated form takes the values as `extra[passport_no]`.
`Valid<Party>` checks them against the active definitions after the struct's own rules (required,
length, number range, date, choice membership, that a reference exists, a file upload), reports
errors under the same names (`extra[passport_no]`) and drops keys that have no definition;
`fill` stores the cleaned values. In a template, `CustomField::views(&db, "parties", locale)`
gives the fields with their labels already in the request's locale, and one tag draws them:

```html
<rx-custom-fields :fields="fields" name="extra" :values="party.extra"/>          {# inputs #}
<rx-custom-fields :fields="fields" :values="party.extra" mode="show"/>           {# entries #}
```

A grid column is `Column::custom_field("extra", "segment")`; it takes its label, kind and choices
from the definition and sorts, filters, searches and exports (CSV, xlsx, print) on the JSON
value (`Grid::page` and `export` resolve it; call `grid.with_custom_fields::<Party>(&request)`
before the sync `filter`). renox-admin does all of this by itself for a resource whose model
declares `custom_fields`: inputs on the forms, entries on the view page, a column and a filter
per definition, and a "Custom fields" resource to manage the definitions.

Two things to know about the queries. A key is never put into SQL unless it matches the key
shape, and values are always bound, so `Party::EXTRA.json("x'; --")` is an error, not an
injection. And `.json(key)` is not indexed: for a key you filter on often, run
`db::json_index(dialect, "parties", "extra", "segment")` from a migration and it gives you the
`CREATE INDEX` statement for SQLite or PostgreSQL. A sensitive flag that seals a value with
`APP_KEY` is not built yet.

### Time zones

- `DateTime` (UTC) is for moments something happened (`created_at`, `paid_at`).
- `NaiveDateTime` is for what a user typed in their local time (`launch_at`). "Naive" means it
  doesn't know its time zone. Convert it with `APP_TIMEZONE` when you need a moment.

### Keys

A model's `id` is its key, and its type picks how rows are numbered:

- `i64`: the database counts 1, 2, 3… (`INTEGER PRIMARY KEY AUTOINCREMENT` /
  `BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY`);
- `Ulid` (`TEXT PRIMARY KEY`) or `Uuid` (`BLOB` / `UUID PRIMARY KEY`): long random-looking
  ids, both made on insert and sortable by creation time;
- `String` (`TEXT PRIMARY KEY`), set by the app.

Pick a ULID or UUID when ids show up in URLs or APIs and shouldn't reveal how many rows there
are. (With counted ids, `/orders/52` tells everyone you have about 52 orders.)
The bike shop's `/about/fields` keys its samples by `Uuid`, and its rentals carry a `Ulid` code.

### Soft deletes

A soft delete hides a row instead of removing it, so it can be brought back.

Use `#[model(table = "products", soft_deletes)]` with a `deleted_at: Option<DateTime>` field
(the derive refuses `soft_deletes` without it). Then:

- `delete` sets `deleted_at` instead of removing the row;
- queries skip those rows (`with_trashed()` / `only_trashed()` to see them);
- `restore` clears `deleted_at`;
- `force_delete` really removes the row.

### Typed columns

`#[derive(Model)]` makes a constant for every field, named in SHOUTY_SNAKE_CASE. Use them in
queries instead of strings: a misspelt column or a value of the wrong type no longer compiles.
`#[model(no_typed_columns)]` turns them off.

```rust
# use renox::prelude::*;
#[derive(Model, Default)]
struct Product {
    id: i64,
    name: String,
    price: i64,
}

# async fn demo(db: &Db) -> Result {
let cheap = Product::query()
    .where_(Product::PRICE.lt(20_000))
    .order_by(Product::NAME)
    .get(db)
    .await?;
# Ok(())
# }
```

### PostgreSQL

> [!WARNING]
> Use `BIGINT` for `i64` columns. A plain `INTEGER` is 32-bit and won't read into an `i64`.

## Checking models against the tables

A model and its migration are written by hand, so they can drift apart: a column renamed in
one and not the other, `Option<String>` over a `NOT NULL` column, a `String` over a number.
`rnx db:check` builds the schema from your migrations on a scratch database and compares
every registered model with its table. Register the models you want checked:

```rust
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "products")]
struct Product {
    id: i64,
    name: String,
    price: i64,
    note: Option<String>,
}

fn app() -> App {
    // Registering the same model twice keeps one. In a module, use
    // `app.model::<Product>()` inside `register`.
    App::new().model::<Product>()
}
```

What is compared, for each model:

- the table exists, and every model column is in it;
- the table has no `NOT NULL` column without a default (and not the primary key) that the
  model lacks, because an insert would fail;
- the column's type fits the field's (table below);
- an `Option<T>` field sits over a nullable column, and a plain field over a `NOT NULL` one
  (`id` is exempt);
- a hand-written `impl Model` that gives no column info is reported, since there is nothing
  to compare.

| Field kind | SQLite (by affinity) | PostgreSQL |
|---|---|---|
| `i64`, `u32` / `i32`, `u16` / `i8`, `i16`, `u8` | INTEGER, NUMERIC | `bigint` / `integer` / `smallint` |
| `f64` / `f32` | REAL, NUMERIC | `double precision` / `real` |
| `String`, enums, `Ulid`, `Encrypted<T>` | TEXT | `text`, `character varying`, `character` |
| `Vec<u8>` | BLOB | `bytea` |
| `bool` | INTEGER, NUMERIC | `boolean` |
| `DateTime` | TEXT, NUMERIC | `timestamp with time zone` |
| `NaiveDateTime` | TEXT, NUMERIC | `timestamp without time zone` |
| `NaiveDate` | TEXT, NUMERIC | `date` |
| `NaiveTime` | TEXT, NUMERIC | `time without time zone` |
| `Json<T>`, `serde_json::Value` | TEXT, NUMERIC | `jsonb`, `json`, `text`, `character varying` |
| `Uuid` | BLOB, NUMERIC | `uuid` |

The command prints the problems grouped by model and exits with 1 when there are any, so it
fits a CI step. When all is well it prints `N model(s) match the schema.`

The scratch database depends on `DATABASE_URL`. With PostgreSQL, the migrations run in a fresh
schema on that server, dropped after the check. Otherwise they run on an in-memory SQLite
database. Your real data is never touched, and there is no flag to point it elsewhere.

A field whose type Renox does not know (a type of your own) skips the type check, and the
rest still runs. To have it checked too, say which kind of column it fills:

```rust
use renox::db::{ColumnKind, ColumnType};

struct Sku(String);

impl ColumnType for Sku {
    const KIND: ColumnKind = ColumnKind::Text;
}
```

`#[derive(DbEnum)]` already counts as text. To run the same check inside your tests, see
`assert_models_match_schema` in [the testing guide](testing.md).

## Migrations from models

Instead of writing a migration by hand, change the model and let Renox write the SQL:

```bash
rnx make:migration --auto add_sku     # or, in the app binary: my-app db:diff add_sku
```

The command builds the schema from your existing migrations on a scratch database (as
`db:check` does), compares it with the registered models and writes the difference as plain SQL
files you can read and edit before running `rnx migrate`. When nothing differs it prints
`Nothing to change.` and writes no file. Register every model with `App::model::<T>()`.

Indexes, defaults and foreign keys are declared on the model:

```rust
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "posts", index(user_id), unique(slug), index(user_id, created_at))]
struct Post {
    id: i64,
    #[model(references = "users")]
    user_id: i64,
    slug: String,
    #[model(default = "0")]
    views: i64,
    created_at: Option<DateTime>,
}

fn app() -> App {
    App::new().model::<Post>()
}
```

- `index(a, b)` and `unique(a)` on the struct, repeatable, with one or more columns. They are
  named `{table}_{columns}_index` or `…_unique`, like `posts_user_id_index`.
- `default = "…"` is raw SQL, written as given on both databases.
- `references = "users"` is a foreign key to `users(id)` with no `ON DELETE` action.

What it handles: new tables for models that have none, added and dropped columns, a changed
type or nullability, renamed columns, and conventionally named indexes. Defaults and foreign
keys are written when a table or column is created; a change to either on an existing column
is not detected, so write that migration yourself. Tables are never dropped, and the `id`
column is never altered. A `NOT NULL` column added to an existing table needs a `default`
(or make the field an `Option`). A field of a type Renox does not know is an error: implement
`ColumnType` for it. Two models on one table are an error.

Anything that could lose data is asked first: `` Was `t.old` renamed to `t.new`? `` and
`` Drop column `t.c` (its data is lost)? ``, and `` Drop index `name`? `` for a convention-named
index the model no longer declares. The answer defaults to no, and no leaves the change out. With
`--yes` (or no terminal) nothing is dropped; `--yes` turns any rename or drop into an error that
lists them. Indexes with other names and `UNIQUE` column constraints are never touched.

PostgreSQL gets `ALTER TABLE` statements. SQLite adds a column in place when it is nullable (or
has a default and no `references`) and otherwise rebuilds the table: a new table, the shared
columns copied, the old one dropped and the new one renamed, with the indexes recreated. A
rebuild is refused, and you are asked to write it by hand, when the table has a `CHECK` or
`CONSTRAINT`, a table-level `UNIQUE (` or `PRIMARY KEY (`, or when another table references it
with an `ON DELETE` action other than `NO ACTION` or `RESTRICT`.

Files: `NAME.up.sql` and `NAME.down.sql`. In an app on SQLite they are SQLite SQL, plus
`NAME.postgres.up.sql` and `NAME.postgres.down.sql` when the PostgreSQL text differs. In an app
on PostgreSQL there are only the two, in PostgreSQL SQL. `--path DIR` writes somewhere other
than `migrations`. The scratch database follows `DATABASE_URL`, so your own data is never
touched.
