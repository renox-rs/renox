//! A record's details on its view page: what [`AdminResource::entries`]
//! returns, drawn with the UI kit's `infolist` and `entry`.
//!
//! [`AdminResource::entries`]: crate::AdminResource::entries

use std::collections::BTreeMap;

use renox::grid::{Column, Kind};
use serde::Serialize;

/// One labelled value on a record's view page. `format` is one of the
/// kit's `entry` formats: `date`, `datetime`, `since`, `money`, `number`,
/// `bool`, `color`, `image`, `markdown`, or none for text.
///
/// ```
/// use renox_admin::Entry;
///
/// let entries = vec![
///     Entry::text("name", "Name").copyable(),
///     Entry::new("status", "Status").labels([("live", "Live")]).badge(),
///     Entry::new("price", "Price").format("money"),
///     Entry::new("notes", "Notes").format("markdown").span_full(),
/// ];
/// # assert_eq!(entries.len(), 4);
/// ```
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct Entry {
    key: String,
    label: String,
    format: Option<String>,
    labels: BTreeMap<String, String>,
    badge: bool,
    copyable: bool,
    span: Option<String>,
}

impl Entry {
    /// The record's `key`, shown as text under `label` until `format` says
    /// otherwise.
    pub fn new(key: &str, label: &str) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            format: None,
            labels: BTreeMap::new(),
            badge: false,
            copyable: false,
            span: None,
        }
    }

    /// The record's `key`, as text.
    pub fn text(key: &str, label: &str) -> Self {
        Self::new(key, label)
    }

    /// How the value is shown: `date`, `datetime`, `since`, `money` (an
    /// amount in the smallest unit, like the grid's money columns),
    /// `number`, `bool`, `color`, `image` or `markdown`.
    pub fn format(mut self, format: &str) -> Self {
        self.format = Some(format.to_owned());
        self
    }

    /// Names for values (`("live", "Live")`), as a select's options.
    pub fn labels<V: Into<String>, L: Into<String>>(
        mut self,
        labels: impl IntoIterator<Item = (V, L)>,
    ) -> Self {
        self.labels = labels
            .into_iter()
            .map(|(value, label)| (value.into(), label.into()))
            .collect();
        self
    }

    /// Shows the value as a badge.
    pub fn badge(mut self) -> Self {
        self.badge = true;
        self
    }

    /// Adds a copy button.
    pub fn copyable(mut self) -> Self {
        self.copyable = true;
        self
    }

    /// Takes the list's whole width.
    pub fn span_full(mut self) -> Self {
        self.span = Some("full".into());
        self
    }

    /// The entry a grid column suggests: its key, its heading and a format
    /// that fits its kind (`None` for `custom` columns, which only the page
    /// can draw).
    pub fn from_column(column: &Column) -> Option<Self> {
        let entry = Self::new(column.key(), column.label());
        let entry = match column.kind() {
            Kind::Custom => return None,
            Kind::Number => entry.format("number"),
            Kind::Money => entry.format("money"),
            Kind::Date => entry.format("date"),
            Kind::DateTime => entry.format("datetime"),
            Kind::Bool => entry.format("bool"),
            Kind::Image => entry.format("image"),
            Kind::Color => entry.format("color"),
            Kind::Select | Kind::Tags => entry.labels(column.options().iter().cloned()).badge(),
            _ => entry,
        };
        Some(entry)
    }

    /// The entry's key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Runs the entry's label and its value names through `translate`.
    pub(crate) fn translate(&mut self, translate: &dyn Fn(&str) -> String) {
        self.label = translate(&self.label);
        for label in self.labels.values_mut() {
            *label = translate(label);
        }
    }
}
