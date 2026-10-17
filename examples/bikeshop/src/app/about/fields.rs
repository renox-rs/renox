//! `/about/fields`: every kind of form field, from the browser to the
//! database and back into the edit form, on SQLite and PostgreSQL (#351).
//!
//! The page is the reference `docs/types.md` points at: a table of every
//! HTML input with the Rust type it is read into, the model's type and the
//! column on each database ([`REFERENCE`]), then a form that exercises
//! every row. A logged-in visitor fills it in; the sample is saved, shown
//! read-only on the kit's infolist, and can be edited and deleted. Samples
//! belong to the person who made them, so the demo shop's visitors never
//! see each other's.
//!
//! - `GET /about/fields` (`about.fields`): the reference, your samples and
//!   the form;
//! - `POST /about/fields` (`about.fields.store`), `PUT` / `DELETE
//!   /about/fields/{sample}`: save, change, delete;
//! - `GET /about/fields/{sample}` (`about.fields.show`) and
//!   `/about/fields/{sample}/edit` (`about.fields.edit`);
//! - `GET /about/fields/{sample}/manual` (`about.fields.manual`): the
//!   private PDF, sent by the app inline with its original name.
//!
//! The sample's key is a `Uuid` (renox's `uuid` feature): the table is the
//! migration `…_create_field_samples_table` (`BLOB` on SQLite, `UUID` on
//! PostgreSQL).

use renox::Download;
use renox::chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use renox::db::Json as DbJson;
use renox::money::{Currency, Money};
use renox::prelude::*;
use renox::uuid::Uuid;
use renox::validation::Dimensions;
use renox_editors::RichText;
use serde::{Deserialize, Serialize};

use crate::app::catalog::model::Brand;
use crate::explain::NotAPage;

/// A frame size: one choice of a few, the kit's `radio`, stored as a word.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// The currencies a deposit can be in: two decimals, none and three, so the
/// form shows what changes from one currency to the next.
pub const DEPOSIT_CURRENCIES: &[&str] = &["USD", "JPY", "KWD"];

/// The colours a sample can come in: CSS colour names, so the read-only
/// page draws them as swatches (`entry(…, format="color")`).
pub const COLOURS: &[&str] = &["teal", "tan", "slategray", "black"];

/// A sample bike whose every field is one kind of form input.
#[derive(Model, Serialize, Default, Debug, Clone, PartialEq)]
#[model(table = "field_samples")]
pub struct FieldSample {
    /// The key: a UUID v7 made on insert. Safe in URLs, unlike a counter.
    pub id: Uuid,
    /// Who made it: the demo keeps everyone's samples apart.
    pub user_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub stock: i64,
    pub weight_kg: f64,
    /// Money in the smallest unit (cents with USD), never a float.
    pub price: i64,
    /// A deposit in its own currency: two columns, `deposit` (the amount in
    /// the currency's smallest unit) and `deposit_currency` (its ISO 4217
    /// code), both NULL when there is none.
    #[model(money)]
    pub deposit: Option<Money>,
    pub available: bool,
    pub size: FrameSize,
    pub colors: DbJson<Vec<String>>,
    pub tags: DbJson<Vec<String>>,
    /// Pairs typed by the user ("Frame": "Aluminium"), in their order.
    pub specs: DbJson<KeyValues>,
    /// Rich text, cleaned when the form was read.
    pub details: Option<String>,
    /// JSON typed in the code editor, kept as typed.
    pub settings: Option<String>,
    /// Free text with suggestions (the catalogue's brands).
    pub brand: Option<String>,
    pub pickup_at: Option<NaiveTime>,
    pub launch_at: Option<NaiveDateTime>,
    pub released_on: Option<NaiveDate>,
    /// The photo's storage key, on the public part of the disk.
    pub photo: Option<String>,
    /// The manual's storage key, private.
    pub manual: Option<String>,
    /// The name the manual was uploaded with, for its download.
    pub manual_name: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// One row of the reference table: an input, the Rust types it goes
/// through and the column it lands in.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TypeRow {
    /// The form field's name on this page.
    pub field: &'static str,
    /// The kit's macro or the HTML input.
    pub input: &'static str,
    /// The type in the form struct (what `Valid<T>` reads).
    pub form: &'static str,
    /// The type in the model.
    pub model: &'static str,
    /// The column on SQLite.
    pub sqlite: &'static str,
    /// The column on PostgreSQL.
    pub postgres: &'static str,
}

const fn row(
    field: &'static str,
    input: &'static str,
    form: &'static str,
    model: &'static str,
    sqlite: &'static str,
    postgres: &'static str,
) -> TypeRow {
    TypeRow {
        field,
        input,
        form,
        model,
        sqlite,
        postgres,
    }
}

/// Every pairing this page shows, in the form's order.
pub const REFERENCE: &[TypeRow] = &[
    row(
        "id",
        "input(readonly, copyable)",
        "—",
        "Uuid",
        "BLOB PRIMARY KEY",
        "UUID PRIMARY KEY",
    ),
    row(
        "name",
        "input",
        "String",
        "String",
        "TEXT NOT NULL",
        "TEXT NOT NULL",
    ),
    row(
        "brand",
        "input(datalist=[…])",
        "Option<String>",
        "Option<String>",
        "TEXT",
        "TEXT",
    ),
    row(
        "description",
        "markdown_editor",
        "Option<String>",
        "Option<String>",
        "TEXT",
        "TEXT",
    ),
    row(
        "stock",
        "input(type=\"number\")",
        "i64",
        "i64",
        "INTEGER",
        "BIGINT",
    ),
    row(
        "weight_kg",
        "input(type=\"number\", step 0.01)",
        "f64",
        "f64",
        "REAL",
        "DOUBLE PRECISION",
    ),
    row(
        "price",
        "input(type=\"number\", prefix=\"$\")",
        "f64 (dollars)",
        "i64 (cents)",
        "INTEGER",
        "BIGINT",
    ),
    row(
        "deposit",
        "input(type=\"money\", currency=…) + select",
        "Option<String> + String (code)",
        "Option<Money>",
        "INTEGER + TEXT (deposit_currency)",
        "BIGINT + TEXT (deposit_currency)",
    ),
    row(
        "available",
        "checkbox(switch=true)",
        "bool",
        "bool",
        "INTEGER (0/1)",
        "BOOLEAN",
    ),
    row(
        "size",
        "radio",
        "FrameSize (DbEnum)",
        "FrameSize",
        "TEXT",
        "TEXT",
    ),
    row(
        "colors",
        "checkbox_list",
        "Vec<String>",
        "Json<Vec<String>>",
        "TEXT (JSON)",
        "JSONB",
    ),
    row(
        "tags",
        "tags_input",
        "Vec<String>",
        "Json<Vec<String>>",
        "TEXT (JSON)",
        "JSONB",
    ),
    row(
        "specs",
        "key_value",
        "KeyValues",
        "Json<KeyValues>",
        "TEXT (JSON)",
        "JSONB",
    ),
    row(
        "details",
        "rich_editor (renox-editors)",
        "Option<RichText>",
        "Option<String>",
        "TEXT",
        "TEXT",
    ),
    row(
        "settings",
        "code_editor (renox-editors)",
        "Option<String> + .json()",
        "Option<String>",
        "TEXT",
        "TEXT",
    ),
    row(
        "pickup_at",
        "input(type=\"time\")",
        "Option<NaiveTime>",
        "Option<NaiveTime>",
        "TEXT",
        "TIME",
    ),
    row(
        "launch_at",
        "input(type=\"datetime-local\")",
        "Option<NaiveDateTime>",
        "Option<NaiveDateTime>",
        "TEXT",
        "TIMESTAMP",
    ),
    row(
        "released_on",
        "date_picker",
        "Option<NaiveDate>",
        "Option<NaiveDate>",
        "TEXT",
        "DATE",
    ),
    row(
        "photo",
        "file(accept=\"image/*\", preview=true)",
        "Option<Upload>",
        "Option<String> (key)",
        "TEXT",
        "TEXT",
    ),
    row(
        "manual",
        "file(accept=\"application/pdf\")",
        "Option<Upload>",
        "Option<String> (key)",
        "TEXT",
        "TEXT",
    ),
    row(
        "created_at",
        "—",
        "—",
        "Option<DateTime>",
        "TEXT",
        "TIMESTAMPTZ",
    ),
];

/// The routes of the page and its form (added to `About::routes`).
pub fn routes() -> Routes {
    let public = Routes::new()
        .get("/about/fields", index)
        .name("about.fields");
    let own = Routes::new()
        .post("/about/fields", store)
        .name("about.fields.store")
        .get("/about/fields/{sample}", show)
        .name("about.fields.show")
        .get("/about/fields/{sample}/edit", edit)
        .name("about.fields.edit")
        .put("/about/fields/{sample}", update)
        .name("about.fields.update")
        .delete("/about/fields/{sample}", destroy)
        .name("about.fields.destroy")
        .get("/about/fields/{sample}/manual", manual)
        .name("about.fields.manual")
        .require_auth();
    public.merge(own)
}

/// The GET routes here that aren't pages.
pub fn not_pages() -> Vec<NotAPage> {
    vec![NotAPage {
        route: "about.fields.manual",
        reason: "a PDF sent inline (Download::from_storage(…).inline())",
    }]
}

/// The form: one field per row of [`REFERENCE`]. The comments name the
/// input each field comes from.
#[derive(Deserialize, Serialize)]
pub struct SampleForm {
    name: String,                // <input>
    brand: Option<String>,       // <input list="…">: free text with suggestions
    description: Option<String>, // the Markdown editor (a <textarea>), empty → None
    stock: i64,                  // <input type="number">
    weight_kg: f64,              // <input type="number" step="0.01">
    price: f64,                  // <input type="number" step="0.01">: dollars, stored as cents
    deposit: Option<String>,     // <rx-input type="money">: whole units, the currency's decimals
    #[serde(default = "usd")]
    deposit_currency: String, // the select beside it: an ISO 4217 code
    #[serde(default)]
    available: bool, // <input type="checkbox">: "on", or nothing → false
    size: FrameSize,             // the kit's radio group: one value of the enum
    #[serde(default)]
    colors: Vec<String>, // checkboxes named "colors": one value each
    #[serde(default)]
    tags: Vec<String>, // the kit's tags_input: one hidden "tags" input per tag
    // The kit's key_value: specs[0][key], specs[0][value]… A nested name
    // makes `Valid` read the whole form as a tree; every type here still
    // parses from its text.
    #[serde(default)]
    specs: KeyValues,
    details: Option<RichText>, // the rich text editor: HTML, cleaned as it is read
    settings: Option<String>,  // the code editor: the text as typed (JSON here)
    pickup_at: Option<NaiveTime>, // <input type="time">
    launch_at: Option<NaiveDateTime>, // <input type="datetime-local">
    released_on: Option<NaiveDate>, // the kit's date_picker: YYYY-MM-DD
    #[serde(skip_serializing)]
    photo: Option<Upload>, // <input type="file" accept="image/*">
    #[serde(skip_serializing)]
    manual: Option<Upload>, // <input type="file" accept="application/pdf">
}

fn usd() -> String {
    "USD".into()
}

/// A deposit as the form takes it: whole units with the currency's decimals
/// (`12.50` USD, `1250` JPY, `1.250` KWD), from the minor amount without a float.
fn whole_units(deposit: Money) -> String {
    let places = deposit.currency.decimals();
    let scale = 10_i64.pow(places);
    let abs = deposit.minor.unsigned_abs();
    let sign = if deposit.minor < 0 { "-" } else { "" };
    if places == 0 {
        return format!("{sign}{abs}");
    }
    let scale = scale as u64;
    format!(
        "{sign}{}.{:0width$}",
        abs / scale,
        abs % scale,
        width = places as usize
    )
}

/// The name of the thumbnail the upload pipeline derives from a photo (src/lib.rs).
const THUMB: &str = "thumb";

// [explain:fields.rules]
impl Validate for SampleForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required().max(100);
        v.field("brand", &self.brand).max(60);
        v.field("description", &self.description).max(2000);
        v.field("stock", &self.stock).min(0).max(100_000);
        v.field("weight_kg", &self.weight_kg).min(0).max(100);
        v.field("price", &self.price)
            .min(0)
            .max(100_000)
            .decimal(0, 2);
        // The deposit follows its currency's decimals: "10.5" JPY is refused.
        v.field("deposit_currency", &self.deposit_currency)
            .one_of(DEPOSIT_CURRENCIES);
        v.field("deposit", &self.deposit)
            .money(&self.deposit_currency);
        // Rules for each item of a list: errors are keyed `colors.0`,
        // `colors.1`…, and `error('colors')` shows the first.
        v.each("colors", &self.colors, |colour| colour.one_of(COLOURS));
        // A colour sent twice (a crafted request): the repeat gets the error.
        v.distinct("colors", &self.colors);
        v.field("tags", &self.tags).max(5);
        v.each("tags", &self.tags, |tag| tag.max(20));
        v.field("specs", &self.specs).max(8);
        // Letters of text, not of markup.
        v.field("details", &self.details).max(5000);
        v.field("settings", &self.settings).json().max(5000);
        // Files checked by their content, not their name: a text file
        // called x.png is refused. The photo's size in pixels comes from
        // the image's header.
        let size = Dimensions::new().max_width(6000).max_height(6000);
        v.field("photo", &self.photo)
            .image()
            .max(2048)
            .dimensions(&size);
        v.field("manual", &self.manual).mimes(&["pdf"]).max(5120);
    }
}
// [/explain:fields.rules]

impl SampleForm {
    /// Copies the form into the sample, storing the files: the photo on
    /// the public part of the disk (`/storage/…`, or the bucket's address
    /// on S3), the manual privately. Returns the keys of files it replaced,
    /// for deleting once the row is saved.
    async fn apply(self, state: &AppState, sample: &mut FieldSample) -> Result<Vec<String>> {
        sample.name = self.name.trim().to_owned();
        sample.brand = self
            .brand
            .map(|b| b.trim().to_owned())
            .filter(|b| !b.is_empty());
        sample.description = self.description;
        sample.stock = self.stock;
        sample.weight_kg = self.weight_kg;
        sample.price = crate::money::from_whole(self.price, &state.config.currency);
        // The rules checked the amount against the code; an empty amount
        // clears both columns together.
        sample.deposit = match (
            self.deposit.as_deref().map(str::trim),
            Currency::parse(&self.deposit_currency),
        ) {
            (Some(text), Ok(currency)) if !text.is_empty() => Money::parse(text, currency).ok(),
            _ => None,
        };
        sample.available = self.available;
        sample.size = self.size;
        sample.colors = DbJson(self.colors);
        sample.tags = DbJson(self.tags);
        sample.specs = DbJson(self.specs);
        // An emptied editor (`<div><br></div>`) stores nothing.
        sample.details = self
            .details
            .filter(|details| !details.is_empty())
            .map(RichText::into_string);
        sample.settings = self.settings;
        sample.pickup_at = self.pickup_at;
        sample.launch_at = self.launch_at;
        sample.released_on = self.released_on;
        let mut replaced = Vec::new();
        if let Some(photo) = self.photo {
            let key = photo.store_public(&state.storage, "samples").await?;
            // The old photo goes with its thumbnail.
            if let Some(old) = sample.photo.replace(key) {
                replaced.push(Upload::derived_key(&old, THUMB));
                replaced.push(old);
            }
        }
        if let Some(manual) = self.manual {
            let key = manual.store(&state.storage, "samples").await?;
            replaced.extend(sample.manual.replace(key));
            sample.manual_name = Some(manual.file_name().to_owned());
        }
        Ok(replaced)
    }
}

/// The page's data for the form: the stored values (or none), the options.
async fn form_context(
    state: &AppState,
    sample: Option<&FieldSample>,
) -> Result<renox::minijinja::Value> {
    // The datalist's suggestions: the catalogue's brands.
    let brands: Vec<String> = crate::app::catalog::model::Brand::query()
        .order_by(Brand::NAME)
        .limit(50)
        .get(&state.db)
        .await?
        .into_iter()
        .map(|b| b.name)
        .collect();
    let scale = crate::money::scale(&state.config.currency) as f64;
    let stored = sample.and_then(|s| s.deposit);
    Ok(context! {
        sample,
        // The deposit in whole units, and its currency (USD for a new one).
        deposit => stored.map(whole_units),
        deposit_currency => stored.map(|m| m.currency.code()).unwrap_or("USD"),
        deposit_currencies => DEPOSIT_CURRENCIES,
        // The price in dollars, as the form takes it.
        price => sample.map(|s| format!("{:.2}", s.price as f64 / scale)),
        colours => COLOURS,
        brands,
    })
}

/// One sample of the person's, or a 404 (someone else's is "not found" too).
async fn own(db: &Db, user: &User, id: Uuid) -> Result<FieldSample> {
    FieldSample::query()
        .where_(FieldSample::ID.eq(id))
        .where_(FieldSample::USER_ID.eq(user.id))
        .first(db)
        .await?
        .ok_or(Error::NotFound)
}

/// `GET /about/fields`: the reference table, the person's samples and the
/// form for a new one (a guest is asked to log in to try it).
async fn index(State(state): State<AppState>, user: Option<AuthUser>) -> Result<View> {
    let samples = match &user {
        Some(user) => {
            FieldSample::query()
                .where_(FieldSample::USER_ID.eq(user.id))
                .order_by_desc(FieldSample::CREATED_AT)
                .get(&state.db)
                .await?
        }
        None => Vec::new(),
    };
    let form = form_context(&state, None).await?;
    Ok(view(
        "about/fields.html",
        context! {
            reference => REFERENCE,
            samples,
            logged_in => user.is_some(),
            photo_url => None::<String>,
            ..form
        },
    ))
}

/// `POST /about/fields`: saves a new sample and shows it read-only.
async fn store(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    Valid(form): Valid<SampleForm>,
) -> Result<(Toast, Redirect)> {
    let mut sample = FieldSample {
        user_id: user.id,
        ..Default::default() // the nil UUID: not saved yet
    };
    form.apply(&state, &mut sample).await?;
    let sample = FieldSample::create(&state.db, sample).await?;
    Ok((
        Toast::success(lang.t("fields.saved", &[])),
        Redirect::route("about.fields.show", &[&sample.id])?,
    ))
}

/// `GET /about/fields/{sample}`: every field read back, on the infolist.
async fn show(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> Result<View> {
    let sample = own(&state.db, &user, id).await?;
    let photo_url = sample.photo.as_deref().map(|key| state.storage.url(key));
    // The pipeline's thumbnail, stored next to the photo; a photo saved before
    // the pipeline existed shows itself.
    let mut thumb_url = photo_url.clone();
    if let Some(key) = sample.photo.as_deref() {
        let thumb = Upload::derived_key(key, THUMB);
        if state.storage.exists(&thumb).await? {
            thumb_url = Some(state.storage.url(&thumb));
        }
    }
    Ok(view(
        "about/fields_show.html",
        context! { sample, photo_url, thumb_url },
    ))
}

/// `GET /about/fields/{sample}/edit`: the form with the stored values.
async fn edit(State(state): State<AppState>, user: AuthUser, Path(id): Path<Uuid>) -> Result<View> {
    let sample = own(&state.db, &user, id).await?;
    let photo_url = sample.photo.as_deref().map(|key| state.storage.url(key));
    let form = form_context(&state, Some(&sample)).await?;
    Ok(view(
        "about/fields_edit.html",
        context! { photo_url, ..form },
    ))
}

/// `PUT /about/fields/{sample}`: saves the changes; a new photo or manual
/// replaces the old file, which is deleted once the row is saved.
// [explain:fields.update]
async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    Path(id): Path<Uuid>,
    Valid(form): Valid<SampleForm>,
) -> Result<(Toast, Redirect)> {
    let mut sample = own(&state.db, &user, id).await?;
    let replaced = form.apply(&state, &mut sample).await?;
    sample.save(&state.db).await?;
    for key in replaced {
        state.storage.delete(&key).await?;
    }
    Ok((
        Toast::success(lang.t("fields.updated", &[])),
        Redirect::route("about.fields.show", &[&id])?,
    ))
}
// [/explain:fields.update]

/// `DELETE /about/fields/{sample}`, behind the kit's `confirm` sheet: the
/// row, then its files (a file left behind by a failed delete is only
/// wasted space; a row pointing at a missing file would be a broken link).
async fn destroy(
    State(state): State<AppState>,
    user: AuthUser,
    lang: Lang,
    Path(id): Path<Uuid>,
) -> Result<(Toast, Redirect)> {
    let mut sample = own(&state.db, &user, id).await?;
    sample.delete(&state.db).await?;
    for key in sample.photo.iter().chain(sample.manual.iter()) {
        state.storage.delete(key).await?;
    }
    if let Some(photo) = &sample.photo {
        state
            .storage
            .delete(&Upload::derived_key(photo, THUMB))
            .await?;
    }
    Ok((
        Toast::success(lang.t("fields.deleted", &[("name", &sample.name)])),
        Redirect::route("about.fields", &[])?,
    ))
}

/// `GET /about/fields/{sample}/manual`: the private PDF, sent by the app
/// with its original name, shown in the browser (`inline`). Only its owner
/// gets it: the file has no public address.
// [explain:fields.manual]
async fn manual(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Download> {
    let sample = own(&state.db, &user, id).await?;
    let key = sample.manual.ok_or(Error::NotFound)?;
    let name = sample.manual_name.unwrap_or_else(|| "manual.pdf".into());
    Ok(Download::from_storage(&state.storage, &key, name)
        .await?
        .inline())
}
// [/explain:fields.manual]
