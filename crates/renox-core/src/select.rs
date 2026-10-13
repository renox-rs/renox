//! Options a searchable select asks the server for: the endpoint behind the
//! UI kit's `select(…, options_url=…, editable=…)`.
//!
//! The kit talks to one URL:
//!
//! | Request | Answer |
//! |---|---|
//! | `GET ?q=kop` (what was typed) | the matching options, a JSON list of [`SelectOption`] |
//! | `GET ?values=3&values=5` (labels for values the page has) | those options |
//! | `POST label=Coffee` (`editable`: "Add “Coffee”") | the new option, auto-selected |
//! | `POST _method=PUT value=3&label=Iced Coffee` (`editable`: renaming the chosen one) | the option as saved |
//!
//! A 422 (from `Valid`) shows its first message under the field. Who may
//! add or rename options is the handler's call, like any route.
//!
//! ```
//! # use renox::prelude::*;
//! use renox::select::{OptionQuery, SelectOption};
//!
//! #[derive(Model, serde::Serialize, Default)]
//! struct Category { id: i64, name: String }
//!
//! async fn search(State(db): State<Db>, query: OptionQuery) -> Result<Json<Vec<SelectOption>>> {
//!     let rows = if query.is_lookup() {
//!         Category::query().where_in("id", query.values_as::<i64>()).get(&db).await?
//!     } else {
//!         Category::query()
//!             .where_op("name", "like", format!("%{}%", query.q))
//!             .order_by("name")
//!             .limit(20)
//!             .get(&db)
//!             .await?
//!     };
//!     Ok(Json(rows.iter().map(|c| SelectOption::new(c.id, &c.name)).collect()))
//! }
//!
//! #[derive(serde::Deserialize)]
//! struct NewCategory { label: String }
//!
//! impl Validate for NewCategory {
//!     fn rules(&self, v: &mut Validator) {
//!         v.field("label", &self.label).required().max(60).unique("category", "name");
//!     }
//! }
//!
//! async fn create(State(db): State<Db>, Valid(form): Valid<NewCategory>) -> Result<Json<SelectOption>> {
//!     let mut category = Category { name: form.label.trim().to_owned(), ..Default::default() };
//!     category.save(&db).await?;
//!     Ok(Json(SelectOption::new(category.id, &category.name)))
//! }
//! ```

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;

/// One option of a select: the value the form sends and the label people
/// see. Serialized as `{"value": "3", "label": "Coffee"}`; the value is
/// always text, as a form sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SelectOption {
    /// What the form sends when this option is chosen (e.g. an id).
    pub value: String,
    /// What the option reads.
    pub label: String,
}

impl SelectOption {
    /// An option from any value (an `i64` id, a `Ulid`…) and its label.
    pub fn new(value: impl ToString, label: impl Into<String>) -> Self {
        Self {
            value: value.to_string(),
            label: label.into(),
        }
    }
}

/// What a searchable select asks for, from the query string: `q`, the text
/// typed (trimmed; empty when the list just opened), or `values`, the
/// options whose labels the page needs (after a failed submit, the values
/// sent back have no label yet).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OptionQuery {
    /// The text typed, trimmed.
    pub q: String,
    /// Values to look up (`?values=3&values=5`), when this is a lookup.
    pub values: Vec<String>,
}

impl OptionQuery {
    /// Whether the select asks for labels of known values rather than
    /// searching: answer with exactly those options.
    pub fn is_lookup(&self) -> bool {
        !self.values.is_empty()
    }

    /// The looked-up values parsed as the key type (`i64` ids), skipping
    /// any that don't parse, so the query binds them typed (PostgreSQL won't
    /// compare a `BIGINT` with text).
    pub fn values_as<T: std::str::FromStr>(&self) -> Vec<T> {
        self.values.iter().filter_map(|v| v.parse().ok()).collect()
    }

    /// Reads a query string (`q=kop`, `values=3&values=5`).
    pub fn parse(query: &str) -> Self {
        let mut parsed = OptionQuery::default();
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            match key.as_ref() {
                "q" => parsed.q = value.trim().to_owned(),
                // `values[]` too, as some clients send it.
                "values" | "values[]" if !value.trim().is_empty() && parsed.values.len() < 100 => {
                    parsed.values.push(value.trim().to_owned());
                }
                _ => {}
            }
        }
        parsed
    }
}

impl<S: Send + Sync> FromRequestParts<S> for OptionQuery {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        Ok(OptionQuery::parse(parts.uri.query().unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_searches_and_lookups() {
        let search = OptionQuery::parse("q=%20Iced%20Coffee%20");
        assert_eq!(search.q, "Iced Coffee");
        assert!(!search.is_lookup());
        let lookup = OptionQuery::parse("values=3&values=&values%5B%5D=5&other=x");
        assert_eq!(lookup.values, ["3", "5"]);
        assert_eq!(
            OptionQuery::parse("values=3&values=x").values_as::<i64>(),
            [3]
        );
        assert!(lookup.is_lookup());
        let many = OptionQuery::parse(&"values=1&".repeat(500));
        assert_eq!(many.values.len(), 100);
    }

    #[test]
    fn options_serialize_with_text_values() {
        let json = serde_json::to_string(&SelectOption::new(3, "Coffee")).unwrap();
        assert_eq!(json, r#"{"value":"3","label":"Coffee"}"#);
    }
}
