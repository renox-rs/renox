//! The panel's own words, and how an app translates them.
//!
//! Every text the panel prints has a key under `renox.admin.` in the app's
//! lang files (`resources/lang/es.json`); without one, the English default
//! below is used. The words an app gives its resources (labels, column and
//! field names, filters, actions) are looked up too, by their English text:
//! first `renox.admin.<slug>.<text>`, then `renox.admin.<text>`.

use renox::prelude::*;
use renox::serde_json::{Map, Value};

/// The built-in texts: key (after `renox.admin.`) and English default.
/// `:name` marks a value the panel fills in.
pub(crate) const DEFAULTS: &[(&str, &str)] = &[
    ("dashboard", "Dashboard"),
    ("nothing_yet", "Nothing here yet"),
    ("no_resources", "No resources are open to you."),
    ("admin_navigation", "Admin navigation"),
    ("account", "Account"),
    ("log_out", "Log out"),
    ("account_label", "Account"),
    ("new", "New :label"),
    ("create", "Create :label"),
    ("edit_title", "Edit :title"),
    ("save_changes", "Save changes"),
    ("cancel", "Cancel"),
    ("edit", "Edit"),
    ("view", "View"),
    ("details", "Details"),
    ("delete", "Delete"),
    ("restore", "Restore"),
    ("delete_for_good", "Delete for good"),
    ("deleted_badge", "Deleted"),
    ("in_trash", "This :label is in the trash."),
    ("delete_question", "Delete this :label?"),
    ("delete_question_for_good", "Delete this :label for good?"),
    (
        "delete_hint_soft",
        "It goes to the trash, where it can be restored.",
    ),
    ("delete_hint_hard", "This can't be undone."),
    ("all", "All"),
    ("trash", "Trash"),
    ("filters_label", ":plural filters"),
    ("empty", "No :plural yet"),
    ("record_title", ":label #:id"),
    ("created", ":label created."),
    ("saved", ":label saved."),
    ("deleted", ":label deleted."),
    ("nothing_selected", "Nothing was selected."),
    (
        "bulk_delete_soft",
        "Delete the selected :plural? They go to the trash.",
    ),
    (
        "bulk_delete_hard",
        "Delete the selected :plural? This can't be undone.",
    ),
    (
        "bulk_delete_for_good",
        "Delete the selected :plural for good? This can't be undone.",
    ),
    (
        "row_delete_for_good",
        "Delete this :label for good? This can't be undone.",
    ),
    ("n_deleted", ":what deleted."),
    ("n_restored", ":what restored."),
    ("n_deleted_for_good", ":what deleted for good."),
    ("required", "The :field field is required."),
    ("number", "The :field field must be a number."),
    ("min", "The :field field must be at least :min."),
    ("max", "The :field field must be at most :max."),
    ("choice", "The :field field isn't one of the choices."),
    ("date", "The :field field must be a date."),
    ("email", "The :field field must be an email address."),
    ("select_placeholder", "Select…"),
    ("attach", "Attach :label"),
    ("attach_none", "Nothing left to attach."),
    ("attached", ":label attached."),
    ("detach", "Detach"),
    ("detach_question", "Detach this :label?"),
    ("detached", ":label detached."),
    ("n_detached", ":what detached."),
    ("relation_empty", "No :plural yet"),
    ("back_to", "Back to :label"),
];

/// The texts of the request's language.
#[derive(Clone)]
pub(crate) struct Texts {
    lang: Lang,
}

impl Texts {
    pub(crate) fn new(state: &AppState) -> Self {
        Self {
            lang: state.current_lang(),
        }
    }

    /// `renox.admin.<key>` if the app translated it.
    fn lookup(&self, key: &str) -> Option<String> {
        let full = format!("renox.admin.{key}");
        let found = self.lang.t(&full, &[]);
        (found != full).then_some(found)
    }

    /// A built-in text with its `:name` values filled in.
    pub(crate) fn get(&self, key: &str, values: &[(&str, &str)]) -> String {
        let template = self
            .lookup(key)
            .or_else(|| {
                DEFAULTS
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, text)| (*text).to_owned())
            })
            .unwrap_or_else(|| key.to_owned());
        fill(&template, values)
    }

    /// An app's own word (a label), translated by its English text.
    pub(crate) fn label(&self, scope: &str, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        self.lookup(&format!("{scope}.{text}"))
            .or_else(|| self.lookup(text))
            .unwrap_or_else(|| text.to_owned())
    }

    /// Every built-in text, for the templates (`admin.text`).
    pub(crate) fn all(&self) -> Value {
        let mut map = Map::new();
        for (key, _) in DEFAULTS {
            map.insert((*key).to_owned(), Value::String(self.get(key, &[])));
        }
        Value::Object(map)
    }
}

/// Replaces `:name` with its value (longest names first, so `:plural`
/// doesn't lose to `:p`).
pub(crate) fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut sorted: Vec<&(&str, &str)> = values.iter().collect();
    sorted.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut out = template.to_owned();
    for (name, value) in sorted {
        out = out.replace(&format!(":{name}"), value);
    }
    out
}

/// The label in lower case, as sentences use it ("New product").
pub(crate) fn lower(text: &str) -> String {
    text.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_replace_longest_names_first() {
        assert_eq!(
            fill(":plural of :p", &[("p", "x"), ("plural", "Products")]),
            "Products of x"
        );
        assert_eq!(fill("No :nothing", &[]), "No :nothing");
    }

    #[test]
    fn defaults_have_unique_keys() {
        let mut keys: Vec<&str> = DEFAULTS.iter().map(|(k, _)| *k).collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len());
    }
}
