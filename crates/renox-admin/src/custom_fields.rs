//! The "Custom fields" resource: the panel adds it by itself when a resource's
//! model declares `custom_fields`, so definitions (`renox::custom_fields`) are
//! managed from the panel. Whoever may open the panel may manage them; an app
//! that wants a stricter rule adds its own resource with the slug
//! `custom-fields`, which replaces this one.
//!
//! The model, key and kind of a stored definition never change (stored values
//! are keyed by them); everything else does. Definitions are deactivated, not
//! deleted, so the values stored for them are kept.

use renox::custom_fields::{
    ChoiceOption, CustomField, DEFAULT_LABEL_LOCALE, FieldKind as Kind, FieldRules, LocaleMap,
    is_valid_key,
};
use renox::grid::Column;
use renox::prelude::*;
use renox::serde_json::{self, Value, json};
use serde::{Deserialize, Serialize};

use crate::{AdminResource, Field, SaveContext};

/// The slug of the resource.
pub(crate) const SLUG: &str = "custom-fields";

const KINDS: [Kind; 7] = [
    Kind::Text,
    Kind::Number,
    Kind::Date,
    Kind::Bool,
    Kind::Choice,
    Kind::Reference,
    Kind::File,
];

/// A row of the `custom_fields` table, as the generic pages save it.
#[derive(Model, Debug, Clone)]
#[model(table = "custom_fields")]
pub(crate) struct Definition {
    id: i64,
    model: String,
    key: String,
    kind: String,
    label: String,
    rules: String,
    meta: String,
    position: i64,
    active: i64,
    created_at: Option<DateTime>,
    updated_at: Option<DateTime>,
}

impl Default for Definition {
    fn default() -> Self {
        Self {
            id: 0,
            model: String::new(),
            key: String::new(),
            kind: Kind::Text.as_str().into(),
            label: "{}".into(),
            rules: "{}".into(),
            meta: "null".into(),
            position: 0,
            active: 1,
            created_at: None,
            updated_at: None,
        }
    }
}

impl Policy for Definition {
    fn allows(&self, _user: &User, _ability: &str) -> bool {
        true
    }
}

impl Definition {
    fn label_map(&self) -> LocaleMap {
        serde_json::from_str(&self.label).unwrap_or_default()
    }

    fn field_rules(&self) -> FieldRules {
        serde_json::from_str(&self.rules).unwrap_or_default()
    }
}

/// The record as the form reads it: flat texts for the JSON columns.
impl Serialize for Definition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let label = self.label_map();
        let rules = self.field_rules();
        let translations = label
            .0
            .iter()
            .filter(|(locale, _)| locale.as_str() != DEFAULT_LABEL_LOCALE)
            .map(|(locale, text)| format!("{locale}: {text}"))
            .collect::<Vec<_>>()
            .join("\n");
        let options = rules
            .options
            .iter()
            .map(|o| match o.label.0.get(DEFAULT_LABEL_LOCALE) {
                Some(text) => format!("{}|{text}", o.value),
                None => o.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let meta: Value = serde_json::from_str(&self.meta).unwrap_or(Value::Null);
        json!({
            "id": self.id,
            "model": self.model,
            "key": self.key,
            "kind": self.kind,
            "label": label.0.get(DEFAULT_LABEL_LOCALE).cloned().unwrap_or_default(),
            "translations": translations,
            "required": rules.required,
            "min": rules.min.map(|n| n.to_string()).unwrap_or_default(),
            "max": rules.max.map(|n| n.to_string()).unwrap_or_default(),
            "options": options,
            "target": rules.target.unwrap_or_default(),
            "meta": if meta.is_null() { String::new() } else { meta.to_string() },
            "position": self.position,
            "active": self.active != 0,
        })
        .serialize(serializer)
    }
}

/// What the create and edit pages post.
#[derive(Debug, Default, Deserialize, Serialize, Validate)]
pub(crate) struct DefinitionForm {
    #[serde(default)]
    model: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    kind: String,
    #[validate(required, max(120))]
    label: String,
    #[serde(default)]
    translations: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    min: String,
    #[serde(default)]
    max: String,
    #[serde(default)]
    options: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    meta: String,
    #[serde(default)]
    position: String,
    #[serde(default)]
    active: bool,
}

/// The built-in resource: `models` are the `(table, name)` of the models that
/// have custom fields.
pub(crate) struct CustomFields {
    pub(crate) models: Vec<(String, String)>,
}

fn number(text: &str) -> Option<Option<i64>> {
    let text = text.trim();
    if text.is_empty() {
        Some(None)
    } else {
        text.parse().ok().map(Some)
    }
}

/// `value` or `value|Label` per line.
fn option_lines(text: &str) -> Vec<(String, Option<String>)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once('|') {
            Some((value, label)) => (value.trim().to_owned(), Some(label.trim().to_owned())),
            None => (line.to_owned(), None),
        })
        .collect()
}

impl AdminResource for CustomFields {
    type Model = Definition;
    type Form = DefinitionForm;

    fn label(&self) -> &str {
        "Custom field"
    }

    fn plural_label(&self) -> &str {
        "Custom fields"
    }

    fn slug(&self) -> &str {
        SLUG
    }

    fn deletable(&self) -> bool {
        false
    }

    fn columns(&self) -> Vec<Column> {
        vec![
            Column::text("model", "Model").searchable(),
            Column::text("key", "Key").searchable(),
            Column::text("kind", "Kind"),
            Column::text("position", "Position"),
            Column::bool("active", "Active"),
        ]
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::select("model", "Model", self.models.clone())
                .required()
                .readonly_on_edit(),
            Field::text("key", "Key")
                .required()
                .hint("Lower case letters, digits and _, starting with a letter. It never changes.")
                .readonly_on_edit(),
            Field::select("kind", "Kind", KINDS.map(|k| (k.as_str(), k.as_str())))
                .required()
                .readonly_on_edit(),
            Field::text("label", "Label").required(),
            Field::textarea("translations", "Other languages")
                .hint("One per line: `es: Segmento`.")
                .span_full(),
            Field::toggle("required", "Required"),
            Field::text("min", "Minimum").hint("Text length or number."),
            Field::text("max", "Maximum").hint("Text length or number."),
            Field::textarea("options", "Choices")
                .hint("A choice field: one per line, `value` or `value|Label`.")
                .span_full(),
            Field::text("target", "Target")
                .hint("A reference field: the table. A file field: the disk."),
            Field::textarea("meta", "Meta (JSON)")
                .hint("Free JSON your app reads; the framework ignores it.")
                .span_full(),
            Field::number("position", "Position"),
            Field::toggle("active", "Active").default_value(true),
        ]
    }

    fn rules(&self, form: &DefinitionForm, record: Option<&Definition>, v: &mut Validator) {
        let kind = match record {
            Some(record) => Kind::parse(&record.kind),
            None => {
                if !self.models.iter().any(|(table, _)| *table == form.model) {
                    v.error("model", "Choose one of the models.");
                }
                if !is_valid_key(&form.key) {
                    v.error(
                        "key",
                        "Use lower case letters, digits and _, starting with a letter (at most 64).",
                    );
                } else {
                    v.field("key", &form.key)
                        .unique("custom_fields", "key")
                        .where_eq("model", form.model.as_str());
                }
                let kind = Kind::parse(&form.kind);
                if kind.is_none() {
                    v.error("kind", "Choose a kind.");
                }
                kind
            }
        };
        let (min, max) = (number(&form.min), number(&form.max));
        for (name, parsed) in [("min", min), ("max", max)] {
            if parsed.is_none() {
                v.error(name, "Must be a whole number.");
            }
        }
        if let (Some(Some(min)), Some(Some(max))) = (min, max)
            && min > max
        {
            v.error("min", "The minimum is above the maximum.");
        }
        if kind == Some(Kind::Choice) && option_lines(&form.options).is_empty() {
            v.error("options", "A choice field needs at least one choice.");
        }
        if matches!(kind, Some(Kind::Reference | Kind::File)) && form.target.trim().is_empty() {
            v.error("target", "This kind needs a target.");
        }
        if number(&form.position).is_none() {
            v.error("position", "Must be a whole number.");
        }
        if !form.meta.trim().is_empty() && serde_json::from_str::<Value>(&form.meta).is_err() {
            v.error("meta", "Must be valid JSON.");
        }
    }

    fn fill(&self, record: &mut Definition, form: DefinitionForm) {
        if record.id == 0 {
            record.model = form.model.trim().to_owned();
            record.key = form.key.trim().to_owned();
            record.kind = form.kind.trim().to_owned();
        }
        let mut label = LocaleMap::default();
        label.set(DEFAULT_LABEL_LOCALE, form.label.trim());
        for line in form.translations.lines() {
            if let Some((locale, text)) = line.split_once(':')
                && !locale.trim().is_empty()
                && !text.trim().is_empty()
            {
                label.set(locale.trim(), text.trim());
            }
        }
        let old = record.field_rules();
        let mut options = Vec::new();
        for (value, text) in option_lines(&form.options) {
            // A kept choice keeps its translated labels.
            let mut choice = old
                .options
                .iter()
                .find(|o| o.value == value)
                .cloned()
                .unwrap_or_else(|| ChoiceOption {
                    value: value.clone(),
                    label: LocaleMap::default(),
                });
            if let Some(text) = text {
                choice.label.set(DEFAULT_LABEL_LOCALE, text);
            }
            options.push(choice);
        }
        let target = form.target.trim();
        let rules = FieldRules {
            required: form.required,
            min: number(&form.min).flatten(),
            max: number(&form.max).flatten(),
            options,
            target: (!target.is_empty()).then(|| target.to_owned()),
        };
        let meta: Value = serde_json::from_str(&form.meta).unwrap_or(Value::Null);
        record.label = serde_json::to_string(&label).unwrap_or_else(|_| "{}".into());
        record.rules = serde_json::to_string(&rules).unwrap_or_else(|_| "{}".into());
        record.meta = meta.to_string();
        record.position = number(&form.position).flatten().unwrap_or(0);
        record.active = i64::from(form.active);
    }

    /// The cached definitions of the model are stale now.
    async fn saved(&self, record: &Definition, cx: &SaveContext) -> Result {
        CustomField::forget(&cx.state.db, &record.model);
        Ok(())
    }
}
