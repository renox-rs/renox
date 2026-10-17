//! Custom fields: fields an administrator adds to a model at run time (a
//! text, a number, a date, a choice, a yes/no, a reference, a file) without a
//! migration. This module holds the *definitions*, kept in the
//! `custom_fields` table; the values live in a JSON column of the model.
//!
//! ```
//! use renox::custom_fields::CustomField;
//!
//! # async fn demo(db: renox::db::Db) -> renox::Result<()> {
//! // A definition is a builder; `save` validates and stores it.
//! CustomField::text("parties", "passport_no")
//!     .label("Passport no.")
//!     .label_in("es", "N.º de pasaporte")
//!     .required()
//!     .max(20)
//!     .save(&db)
//!     .await?;
//! CustomField::choice("parties", "segment")
//!     .options(&["retail", "corporate"])
//!     .save(&db)
//!     .await?;
//!
//! // The active definitions of a model, in order (cached per model).
//! let fields = CustomField::for_model(&db, "parties").await?;
//! assert_eq!(fields[0].key, "passport_no");
//! # Ok(()) }
//! ```
//!
//! # Keys are stable
//!
//! A key matches `[a-z][a-z0-9_]*` (checked by [`CustomField::save`]) and
//! never changes: the stored values are keyed by it. Saving an existing
//! definition updates only the label, rules, meta, position and active flag,
//! so renaming a field is changing its label. Deactivating a field hides it
//! but keeps the values stored for it.
//!
//! # Values
//!
//! The values of a record are a [`CustomValues`] (a map from key to JSON),
//! the type of a `Json<CustomValues>` column. [`CustomValues::validate`] checks
//! submitted input against the active definitions of a model.
//!
//! # Cache
//!
//! [`CustomField::for_model`] keeps the active definitions of each model in
//! memory, per [`Db`]. Every write through this module drops that model's
//! entry in the process that wrote. Other servers sharing the database see
//! the change after a restart (or a call to [`CustomField::forget`]).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

mod values;

pub use values::CustomValues;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::{DateTime, Db, Migration, Row, now, sql};
use crate::{Error, Result};

pub(crate) const MIGRATION: Migration =
    crate::db::framework_migration!("custom_fields", "00010101000430_create_custom_fields_table");

/// The locale a plain `.label("…")` is stored under.
pub const DEFAULT_LABEL_LOCALE: &str = "en";

/// The longest key a definition may have.
const MAX_KEY: usize = 64;

/// The definitions cached for one [`Db`], by model.
#[derive(Clone, Default)]
pub(crate) struct Cache(Arc<Mutex<HashMap<String, Arc<Vec<CustomField>>>>>);

impl Cache {
    fn get(&self, model: &str) -> Option<Arc<Vec<CustomField>>> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(model)
            .cloned()
    }

    fn put(&self, model: &str, fields: Arc<Vec<CustomField>>) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(model.to_owned(), fields);
    }

    fn forget(&self, model: &str) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(model);
    }
}

/// What a custom field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FieldKind {
    /// Free text.
    Text,
    /// A number (integer or decimal).
    Number,
    /// A calendar date.
    Date,
    /// Yes or no.
    Bool,
    /// One of a list of options.
    Choice,
    /// The id of a record of another model (named in the rules' `target`).
    Reference,
    /// An uploaded file, stored on a disk (named in the rules' `target`).
    File,
}

impl FieldKind {
    /// The name stored in the `kind` column.
    pub fn as_str(self) -> &'static str {
        match self {
            FieldKind::Text => "text",
            FieldKind::Number => "number",
            FieldKind::Date => "date",
            FieldKind::Bool => "bool",
            FieldKind::Choice => "choice",
            FieldKind::Reference => "reference",
            FieldKind::File => "file",
        }
    }

    /// The kind named `name`, if there is one.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "text" => FieldKind::Text,
            "number" => FieldKind::Number,
            "date" => FieldKind::Date,
            "bool" => FieldKind::Bool,
            "choice" => FieldKind::Choice,
            "reference" => FieldKind::Reference,
            "file" => FieldKind::File,
            _ => return None,
        })
    }
}

/// A text per locale (`{"en": "Passport no.", "es": "N.º de pasaporte"}`),
/// for labels and choice labels.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocaleMap(pub BTreeMap<String, String>);

impl LocaleMap {
    /// The text for `locale`, else for `fallback`, else for
    /// [`DEFAULT_LABEL_LOCALE`], else any text; `None` for an empty map.
    ///
    /// ```
    /// use renox::custom_fields::LocaleMap;
    ///
    /// let mut label = LocaleMap::default();
    /// label.set("en", "Segment");
    /// label.set("es", "Segmento");
    /// assert_eq!(label.resolve("es", "en"), Some("Segmento"));
    /// assert_eq!(label.resolve("fr", "en"), Some("Segment"));
    /// assert_eq!(LocaleMap::default().resolve("en", "en"), None);
    /// ```
    pub fn resolve(&self, locale: &str, fallback: &str) -> Option<&str> {
        [locale, fallback, DEFAULT_LABEL_LOCALE]
            .into_iter()
            .find_map(|l| self.0.get(l))
            .or_else(|| self.0.values().next())
            .map(String::as_str)
    }

    /// Sets the text for `locale`.
    pub fn set(&mut self, locale: impl Into<String>, text: impl Into<String>) {
        self.0.insert(locale.into(), text.into());
    }
}

/// One option of a [`FieldKind::Choice`] field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    /// The value stored in the JSON column.
    pub value: String,
    /// What the user sees; empty means the value itself.
    #[serde(default)]
    pub label: LocaleMap,
}

/// The validation rules of a definition (the `rules` column).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FieldRules {
    /// The value must be filled in.
    #[serde(default)]
    pub required: bool,
    /// Text: at least this many characters. Number: at least this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    /// Text: at most this many characters. Number: at most this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    /// The options of a choice field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ChoiceOption>,
    /// A reference field: the referenced model's table. A file field: the
    /// name of the disk it is stored on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// The definition of one custom field of a model.
///
/// Build one with [`text`](CustomField::text), [`number`](CustomField::number),
/// [`date`](CustomField::date), [`bool`](CustomField::bool),
/// [`choice`](CustomField::choice), [`reference`](CustomField::reference) or
/// [`file`](CustomField::file), then [`save`](CustomField::save) it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CustomField {
    /// The row id (0 until saved).
    pub id: i64,
    /// The table name of the model the field belongs to.
    pub model: String,
    /// The stable key: `[a-z][a-z0-9_]*`.
    pub key: String,
    /// What the field holds.
    pub kind: FieldKind,
    /// The label per locale.
    pub label: LocaleMap,
    /// The validation rules.
    pub rules: FieldRules,
    /// Free JSON the framework does not interpret (read-only for templates
    /// and the admin).
    pub meta: Value,
    /// Order in forms and columns (lower first).
    pub position: i64,
    /// Inactive fields are hidden but keep their stored values.
    pub active: bool,
    /// When the row was created.
    pub created_at: DateTime,
    /// When the row was last written.
    pub updated_at: DateTime,
}

/// Whether `key` is a valid custom field key: `[a-z][a-z0-9_]*`, at most 64
/// characters.
///
/// ```
/// use renox::custom_fields::is_valid_key;
///
/// assert!(is_valid_key("passport_no"));
/// assert!(!is_valid_key("Passport"));
/// assert!(!is_valid_key("1st"));
/// assert!(!is_valid_key("a'b"));
/// ```
pub fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= MAX_KEY
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl CustomField {
    fn new(model: &str, key: &str, kind: FieldKind) -> Self {
        let at = now();
        CustomField {
            id: 0,
            model: model.to_owned(),
            key: key.to_owned(),
            kind,
            label: LocaleMap::default(),
            rules: FieldRules::default(),
            meta: Value::Null,
            position: 0,
            active: true,
            created_at: at,
            updated_at: at,
        }
    }

    /// A free text field.
    pub fn text(model: &str, key: &str) -> Self {
        Self::new(model, key, FieldKind::Text)
    }

    /// A number field.
    pub fn number(model: &str, key: &str) -> Self {
        Self::new(model, key, FieldKind::Number)
    }

    /// A date field.
    pub fn date(model: &str, key: &str) -> Self {
        Self::new(model, key, FieldKind::Date)
    }

    /// A yes/no field.
    pub fn bool(model: &str, key: &str) -> Self {
        Self::new(model, key, FieldKind::Bool)
    }

    /// A field with a list of options; add them with
    /// [`options`](CustomField::options).
    pub fn choice(model: &str, key: &str) -> Self {
        Self::new(model, key, FieldKind::Choice)
    }

    /// A field holding the id of a record of the model whose table is
    /// `target`.
    pub fn reference(model: &str, key: &str, target: &str) -> Self {
        let mut field = Self::new(model, key, FieldKind::Reference);
        field.rules.target = Some(target.to_owned());
        field
    }

    /// A field holding an uploaded file kept on the disk named `disk`.
    pub fn file(model: &str, key: &str, disk: &str) -> Self {
        let mut field = Self::new(model, key, FieldKind::File);
        field.rules.target = Some(disk.to_owned());
        field
    }

    /// Sets the label for [`DEFAULT_LABEL_LOCALE`] (the fallback).
    pub fn label(self, text: &str) -> Self {
        self.label_in(DEFAULT_LABEL_LOCALE, text)
    }

    /// Sets the label for one locale.
    pub fn label_in(mut self, locale: &str, text: &str) -> Self {
        self.label.set(locale, text);
        self
    }

    /// Makes the value required.
    pub fn required(mut self) -> Self {
        self.rules.required = true;
        self
    }

    /// Sets the maximum length (text) or value (number).
    pub fn max(mut self, max: i64) -> Self {
        self.rules.max = Some(max);
        self
    }

    /// Sets the minimum length (text) or value (number).
    pub fn min(mut self, min: i64) -> Self {
        self.rules.min = Some(min);
        self
    }

    /// Sets the options of a choice field (their labels are the values).
    pub fn options(mut self, values: &[&str]) -> Self {
        self.rules.options = values
            .iter()
            .map(|v| ChoiceOption {
                value: (*v).to_owned(),
                label: LocaleMap::default(),
            })
            .collect();
        self
    }

    /// Adds an option with a label per locale.
    pub fn option_labeled(mut self, value: &str, label: LocaleMap) -> Self {
        self.rules.options.push(ChoiceOption {
            value: value.to_owned(),
            label,
        });
        self
    }

    /// Attaches free JSON the framework does not interpret.
    pub fn meta(mut self, meta: Value) -> Self {
        self.meta = meta;
        self
    }

    /// Sets the order in forms and columns (lower first).
    pub fn position(mut self, position: i64) -> Self {
        self.position = position;
        self
    }

    /// Marks the definition inactive.
    pub fn inactive(mut self) -> Self {
        self.active = false;
        self
    }

    /// The label for `locale`, falling back to `fallback`, then to the key.
    pub fn label_for(&self, locale: &str, fallback: &str) -> String {
        self.label
            .resolve(locale, fallback)
            .unwrap_or(&self.key)
            .to_owned()
    }

    /// Checks the key and the rules fit the kind.
    fn check(&self) -> Result<()> {
        let bad = |msg: String| Err(Error::BadRequest(msg));
        if !is_valid_key(&self.key) {
            return bad(format!(
                "custom field key `{}` must match [a-z][a-z0-9_]* (at most {MAX_KEY} characters)",
                self.key.escape_debug()
            ));
        }
        if self.model.trim().is_empty() {
            return bad("custom field model must not be empty".into());
        }
        if self.kind == FieldKind::Choice && self.rules.options.is_empty() {
            return bad(format!("choice field `{}` needs options", self.key));
        }
        if matches!(self.kind, FieldKind::Reference | FieldKind::File)
            && self.rules.target.as_deref().is_none_or(str::is_empty)
        {
            return bad(format!(
                "{} field `{}` needs a target",
                self.kind.as_str(),
                self.key
            ));
        }
        if let (Some(min), Some(max)) = (self.rules.min, self.rules.max)
            && min > max
        {
            return bad(format!("field `{}`: min is above max", self.key));
        }
        Ok(())
    }

    /// Stores the definition: inserts it when it has no id, else updates the
    /// label, rules, meta, position and active flag of the row (the model,
    /// key and kind of a stored definition never change). Returns the stored
    /// definition and drops the model's cache entry.
    ///
    /// An invalid key (uppercase, space, quote, leading digit…) or a
    /// definition that does not fit its kind is rejected, as is a second
    /// definition of the same model and key.
    pub async fn save(&self, db: &Db) -> Result<CustomField> {
        self.check()?;
        let label = serde_json::to_string(&self.label).map_err(anyhow::Error::from)?;
        let rules = serde_json::to_string(&self.rules).map_err(anyhow::Error::from)?;
        let meta = serde_json::to_string(&self.meta).map_err(anyhow::Error::from)?;
        let at = now();
        let id = if self.id == 0 {
            sql("INSERT INTO custom_fields \
                 (model, key, kind, label, rules, meta, position, active, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id")
            .bind(self.model.as_str())
            .bind(self.key.as_str())
            .bind(self.kind.as_str())
            .bind(label)
            .bind(rules)
            .bind(meta)
            .bind(self.position)
            .bind(i64::from(self.active))
            .bind(at)
            .bind(at)
            .scalar::<i64>(db)
            .await?
        } else {
            let changed = sql("UPDATE custom_fields SET label = ?, rules = ?, meta = ?, \
                 position = ?, active = ?, updated_at = ? WHERE id = ?")
            .bind(label)
            .bind(rules)
            .bind(meta)
            .bind(self.position)
            .bind(i64::from(self.active))
            .bind(at)
            .bind(self.id)
            .execute(db)
            .await?;
            if changed == 0 {
                return Err(Error::NotFound);
            }
            self.id
        };
        // An update may belong to a model other than the builder's; drop both.
        db.custom_fields_cache().forget(&self.model);
        let stored = Self::find(db, id).await?.ok_or(Error::NotFound)?;
        db.custom_fields_cache().forget(&stored.model);
        Ok(stored)
    }

    /// The definition with this id.
    pub async fn find(db: &Db, id: i64) -> Result<Option<CustomField>> {
        let row = sql(SELECT_BY_ID).bind(id).fetch_optional(db).await?;
        row.as_ref().map(read).transpose()
    }

    /// The active definitions of `model`, ordered by position then id. The
    /// first call per model queries; later calls read the cache until a
    /// definition of the model is written through this module.
    pub async fn for_model(db: &Db, model: &str) -> Result<Arc<Vec<CustomField>>> {
        if let Some(cached) = db.custom_fields_cache().get(model) {
            return Ok(cached);
        }
        let fields = Arc::new(Self::list_for(db, model, false).await?);
        db.custom_fields_cache().put(model, fields.clone());
        Ok(fields)
    }

    /// Every definition of `model`, inactive ones too, ordered by position
    /// then id. Not cached (for the screen that manages definitions).
    pub async fn all_for_model(db: &Db, model: &str) -> Result<Vec<CustomField>> {
        Self::list_for(db, model, true).await
    }

    async fn list_for(db: &Db, model: &str, inactive: bool) -> Result<Vec<CustomField>> {
        let query = if inactive {
            "SELECT * FROM custom_fields WHERE model = ? ORDER BY position, id"
        } else {
            "SELECT * FROM custom_fields WHERE model = ? AND active = 1 ORDER BY position, id"
        };
        let rows = sql(query).bind(model).fetch_all(db).await?;
        rows.iter().map(read).collect()
    }

    /// Hides the definition with this id; the values stored for it are kept.
    pub async fn deactivate(db: &Db, id: i64) -> Result<()> {
        Self::set_active(db, id, false).await
    }

    /// Shows a deactivated definition again.
    pub async fn activate(db: &Db, id: i64) -> Result<()> {
        Self::set_active(db, id, true).await
    }

    async fn set_active(db: &Db, id: i64, active: bool) -> Result<()> {
        let found = Self::find(db, id).await?.ok_or(Error::NotFound)?;
        sql("UPDATE custom_fields SET active = ?, updated_at = ? WHERE id = ?")
            .bind(i64::from(active))
            .bind(now())
            .bind(id)
            .execute(db)
            .await?;
        db.custom_fields_cache().forget(&found.model);
        Ok(())
    }

    /// Deletes the definition with this id. The values stored in the model's
    /// JSON column are not touched; prefer [`deactivate`](CustomField::deactivate).
    pub async fn delete(db: &Db, id: i64) -> Result<()> {
        let found = Self::find(db, id).await?.ok_or(Error::NotFound)?;
        sql("DELETE FROM custom_fields WHERE id = ?")
            .bind(id)
            .execute(db)
            .await?;
        db.custom_fields_cache().forget(&found.model);
        Ok(())
    }

    /// Drops the cached definitions of `model` so the next read queries
    /// (after another server changed them).
    pub fn forget(db: &Db, model: &str) {
        db.custom_fields_cache().forget(model);
    }
}

const SELECT_BY_ID: &str = "SELECT * FROM custom_fields WHERE id = ?";

fn json<T: serde::de::DeserializeOwned>(row: &Row, column: &str) -> Result<T> {
    let text: String = row.try_get(column)?;
    Ok(serde_json::from_str(&text).map_err(anyhow::Error::from)?)
}

fn read(row: &Row) -> Result<CustomField> {
    let kind: String = row.try_get("kind")?;
    Ok(CustomField {
        id: row.try_get("id")?,
        model: row.try_get("model")?,
        key: row.try_get("key")?,
        kind: FieldKind::parse(&kind)
            .ok_or_else(|| anyhow::anyhow!("unknown custom field kind `{kind}`"))?,
        label: json(row, "label")?,
        rules: json(row, "rules")?,
        meta: json(row, "meta")?,
        position: row.try_get("position")?,
        active: row.try_get::<i64>("active")? != 0,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
