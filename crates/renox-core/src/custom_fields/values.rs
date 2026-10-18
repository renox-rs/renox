//! The values of the custom fields of one record, and their validation.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{CustomField, DEFAULT_LABEL_LOCALE, FieldKind};
use crate::AppState;
use crate::Result;
use crate::db::{Db, quote, sql};
use crate::upload::Upload;
use crate::validation::Errors;

/// What a sealed value starts with in the JSON column:
/// `rx-sealed:1:<token>`, where the token is the value's JSON text sealed
/// with `APP_KEY` (as an `Encrypted` column is).
pub const SEALED_PREFIX: &str = "rx-sealed:1:";

/// Whether a stored value is a sealed one (decided by its prefix, not by the
/// field's current flag).
///
/// ```
/// use renox::custom_fields::is_sealed;
/// use serde_json::json;
///
/// assert!(is_sealed(&json!("rx-sealed:1:abc")));
/// assert!(!is_sealed(&json!("retail")));
/// ```
pub fn is_sealed(value: &Value) -> bool {
    value.as_str().is_some_and(|s| s.starts_with(SEALED_PREFIX))
}

/// A stored value read back with [`CustomValues::open`]. `Debug` never shows
/// the value.
#[derive(Clone, PartialEq)]
pub enum Opened {
    /// The plain value (opened, or never sealed).
    Value(Value),
    /// Sealed, but it can't be opened with this `APP_KEY`.
    Unreadable,
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Opened::Value(_) => f.write_str("Opened::Value(..)"),
            Opened::Unreadable => f.write_str("Opened::Unreadable"),
        }
    }
}

/// Seals a plain value for storage.
fn seal_value(db: &Db, plain: &Value) -> Result<Value> {
    let key = db.seal_key().ok_or_else(|| {
        anyhow::anyhow!("a sensitive custom field needs the app's Db (it has the APP_KEY)")
    })?;
    let json = serde_json::to_string(plain).map_err(anyhow::Error::from)?;
    let token = crate::db::encrypted::seal(key, &json);
    Ok(Value::String(format!("{SEALED_PREFIX}{token}")))
}

/// A stored value, opened if it is sealed (see [`CustomValues::open`]).
pub(crate) fn open_stored(db: &Db, value: &Value) -> Opened {
    let Some(token) = value.as_str().and_then(|s| s.strip_prefix(SEALED_PREFIX)) else {
        return Opened::Value(value.clone());
    };
    let opened = db
        .seal_key()
        .and_then(|k| crate::db::encrypted::open(k, token))
        .and_then(|json| serde_json::from_str(&json).ok());
    opened.map_or(Opened::Unreadable, Opened::Value)
}

/// The custom field values of one record, by field key: the type of a
/// `Json<CustomValues>` column. Serializes as the plain JSON object.
///
/// ```
/// use renox::custom_fields::CustomValues;
///
/// let mut values = CustomValues::default();
/// values.set("segment", "retail");
/// assert_eq!(values.get("segment").and_then(|v| v.as_str()), Some("retail"));
/// assert_eq!(serde_json::to_string(&values).unwrap(), r#"{"segment":"retail"}"#);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CustomValues(pub BTreeMap<String, Value>);

impl CustomValues {
    /// The value stored under `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Stores `value` under `key`.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.0.insert(key.into(), value.into());
    }

    /// The value stored under `key`, opened if it is sealed. `None` when
    /// nothing is stored; [`Opened::Unreadable`] when `APP_KEY` changed since
    /// it was sealed. A plain value is returned as it is, whatever the field's
    /// flag is now.
    ///
    /// ```
    /// use renox::custom_fields::{CustomValues, Opened};
    ///
    /// # fn demo(db: &renox::db::Db) {
    /// let mut values = CustomValues::default();
    /// values.set("segment", "retail");
    /// assert_eq!(
    ///     values.open(db, "segment"),
    ///     Some(Opened::Value("retail".into()))
    /// );
    /// # }
    /// ```
    pub fn open(&self, db: &Db, key: &str) -> Option<Opened> {
        self.0.get(key).map(|value| open_stored(db, value))
    }

    /// The values as a template should get them (`<rx-custom-fields
    /// :values="…">`) for the `fields` of [`CustomField::views_for`]: the
    /// value of a masked sensitive field is left out, so the plain text never
    /// reaches the context; one the viewer may see is opened (an unopenable
    /// one becomes `{"unreadable": true}`); the others stay as stored.
    ///
    /// ```
    /// use renox::custom_fields::{CustomField, CustomValues};
    ///
    /// # async fn demo(db: renox::db::Db, auth: Option<renox::prelude::AuthUser>, values: CustomValues) -> renox::Result<()> {
    /// let fields = CustomField::views_for(&db, "parties", "en", auth.as_ref()).await?;
    /// let shown = values.shown(&db, &fields);
    /// # let _ = shown;
    /// # Ok(()) }
    /// ```
    pub fn shown(&self, db: &Db, fields: &[super::FieldView]) -> CustomValues {
        let mut out = self.clone();
        for field in fields.iter().filter(|f| f.sensitive) {
            if field.masked {
                out.0.remove(&field.key);
                continue;
            }
            match self.open(db, &field.key) {
                Some(Opened::Value(plain)) => {
                    out.0.insert(field.key.clone(), plain);
                }
                Some(Opened::Unreadable) => {
                    out.0
                        .insert(field.key.clone(), serde_json::json!({"unreadable": true}));
                }
                None => {}
            }
        }
        out
    }

    /// Removes the value stored under `key`.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.remove(key)
    }

    /// Whether no value is stored.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Validates the submitted `input` (key to value; form text or native
    /// JSON) against the active definitions of `model`.
    ///
    /// Returns the cleaned values, or the errors keyed `column[key]` (for
    /// example `extra[passport_no]`) with messages that name the field's label
    /// in `locale`. Keys without an active definition are dropped. Each kind
    /// is checked: required, text length / number range (`min`, `max`), a
    /// number, a `YYYY-MM-DD` date, a yes/no, membership of the options, a
    /// reference that exists as a row of the target table, and a file taken
    /// from `uploads` (by key) and stored on the target disk. Files are stored
    /// only when every field is valid; the stored key is the value kept.
    ///
    /// A sensitive field's value is validated as plain text and then sealed
    /// (see [`SEALED_PREFIX`]); a blank one is left out like any blank value,
    /// and the merge (the `#[model(form)]` one does it) keeps what is stored.
    ///
    /// A blank optional value is left out, so the result holds only what was
    /// filled in. Merging with the values already stored (to keep those of
    /// deactivated fields and of untouched files) is the caller's job.
    ///
    /// ```
    /// use renox::custom_fields::{CustomField, CustomValues};
    /// use serde_json::json;
    ///
    /// # async fn demo(state: renox::AppState) -> renox::Result<()> {
    /// CustomField::number("parties", "age").max(120).save(&state.db).await?;
    /// let input = json!({"age": "200", "other": "dropped"});
    /// let outcome = CustomValues::validate(
    ///     &state, "parties", "extra", input.as_object().unwrap(), &Default::default(), "en",
    /// ).await?;
    /// assert!(outcome.unwrap_err().has("extra[age]"));
    /// # Ok(()) }
    /// ```
    pub async fn validate(
        state: &AppState,
        model: &str,
        column: &str,
        input: &Map<String, Value>,
        uploads: &BTreeMap<String, Upload>,
        locale: &str,
    ) -> Result<std::result::Result<CustomValues, Errors>> {
        let fields = CustomField::for_model(&state.db, model).await?;
        let mut values = CustomValues::default();
        let mut errors = Errors::new();
        let mut files = Vec::new();
        for field in fields.iter() {
            let name = format!("{column}[{}]", field.key);
            let label = field.label_for(locale, DEFAULT_LABEL_LOCALE);
            let required = format!("The {label} field is required.");
            if field.kind == FieldKind::File {
                match uploads.get(&field.key).filter(|u| u.size() > 0) {
                    Some(upload) => files.push((field, upload)),
                    None if field.rules.required => errors.add(&name, required),
                    None => {}
                }
                continue;
            }
            match check(state, field, input.get(&field.key)).await? {
                Ok(Some(value)) if field.rules.sensitive => {
                    values.set(field.key.clone(), seal_value(&state.db, &value)?)
                }
                Ok(Some(value)) => values.set(field.key.clone(), value),
                Ok(None) if field.rules.required => errors.add(&name, required),
                Ok(None) => {}
                Err(message) => errors.add(&name, message.replace("{label}", &label)),
            }
        }
        if !errors.is_empty() {
            return Ok(Err(errors));
        }
        for (field, upload) in files {
            let disk = state.disk_named(field.rules.target.as_deref().unwrap_or_default())?;
            let dir = format!("custom-fields/{model}/{}", field.key);
            values.set(field.key.clone(), upload.store(disk, &dir).await?);
        }
        Ok(Ok(values))
    }
}

/// The text of a submitted value, if it is a scalar.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Checks one non-file field. `Ok(None)` is a blank value; `Err` is a message
/// (`{label}` stands for the field's label).
async fn check(
    state: &AppState,
    field: &CustomField,
    raw: Option<&Value>,
) -> Result<std::result::Result<Option<Value>, String>> {
    let rules = &field.rules;
    if field.kind == FieldKind::Bool {
        let on = match raw.and_then(text).map(|t| t.to_ascii_lowercase()) {
            None => false,
            Some(t) => match t.as_str() {
                "1" | "true" | "on" | "yes" => true,
                "" | "0" | "false" | "off" | "no" => false,
                _ => return Ok(Err("The {label} field must be yes or no.".into())),
            },
        };
        if rules.required && !on {
            return Ok(Err("The {label} field must be accepted.".into()));
        }
        return Ok(Ok(Some(Value::Bool(on))));
    }
    let Some(t) = raw.and_then(text).filter(|t| !t.is_empty()) else {
        return Ok(Ok(None));
    };
    let invalid = |message: String| Ok(Err(message));
    let value = match field.kind {
        FieldKind::Text => {
            let len = t.chars().count() as i64;
            if let Some(max) = rules.max
                && len > max
            {
                return invalid(format!(
                    "The {{label}} field must not be longer than {max} characters."
                ));
            }
            if let Some(min) = rules.min
                && len < min
            {
                return invalid(format!(
                    "The {{label}} field must be at least {min} characters."
                ));
            }
            Value::String(t)
        }
        FieldKind::Number => {
            let number = match t.parse::<i64>() {
                Ok(i) => Value::from(i),
                Err(_) => match t.parse::<f64>().ok().filter(|f| f.is_finite()) {
                    Some(f) => Value::from(f),
                    None => return invalid("The {label} field must be a number.".into()),
                },
            };
            let n = number.as_f64().unwrap_or_default();
            if let Some(max) = rules.max
                && n > max as f64
            {
                return invalid(format!("The {{label}} field must not be above {max}."));
            }
            if let Some(min) = rules.min
                && n < min as f64
            {
                return invalid(format!("The {{label}} field must be at least {min}."));
            }
            number
        }
        FieldKind::Date => match NaiveDate::parse_from_str(&t, "%Y-%m-%d") {
            Ok(date) => Value::String(date.format("%Y-%m-%d").to_string()),
            Err(_) => return invalid("The {label} field must be a date (YYYY-MM-DD).".into()),
        },
        FieldKind::Choice => {
            if !rules.options.iter().any(|o| o.value == t) {
                return invalid("The selected {label} is not valid.".into());
            }
            Value::String(t)
        }
        FieldKind::Reference => {
            let target = rules.target.as_deref().unwrap_or_default();
            let Ok(id) = t.parse::<i64>() else {
                return invalid("The selected {label} does not exist.".into());
            };
            let found: bool = sql(format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE \"id\" = ?)",
                quote(target)
            ))
            .bind(id)
            .scalar(&state.db)
            .await?;
            if !found {
                return invalid("The selected {label} does not exist.".into());
            }
            Value::from(id)
        }
        FieldKind::Bool | FieldKind::File => return Ok(Ok(None)),
    };
    Ok(Ok(Some(value)))
}
