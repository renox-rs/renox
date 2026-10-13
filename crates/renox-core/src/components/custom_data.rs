//! The editor data file: the component contracts in VS Code's `html.customData` format, so the
//! editor completes `<rx-…>` tags, their attributes and values.

use serde_json::{Value, json};

use super::app::AppContract;
use super::attrs::Kind;
use super::contracts::Contract;

/// The attributes any element may carry.
fn global_attributes() -> Value {
    json!([
        {"name": "rx-if", "description": "Renders the element only when the expression is true."},
        {"name": "rx-elif", "description": "Renders the element when the preceding rx-if (or rx-elif) was false and this expression is true."},
        {"name": "rx-else", "description": "Renders the element when the preceding rx-if was false."},
        {"name": "rx-for", "description": "Repeats the element: \"item in list\"."},
        {"name": "can", "description": "Renders the element only when the user may do this (a gate or an ability)."}
    ])
}

/// One attribute entry.
fn attribute(name: String, description: String, values: &[&str]) -> Value {
    let values: Vec<Value> = values.iter().map(|v| json!({ "name": v })).collect();
    json!({"name": name, "description": description, "values": values})
}

fn built_in(c: &Contract) -> Value {
    let mut attributes: Vec<Value> = c
        .props
        .iter()
        .map(|p| {
            let name = if p.kind == Kind::Data {
                format!(":{}", p.name)
            } else {
                p.name.to_owned()
            };
            let mut description = p.doc.to_owned();
            if p.required {
                description = format!("(required) {description}");
            }
            let values: &[&str] = match p.kind {
                Kind::Bool => &["true", "false"],
                Kind::Enum => p.values,
                _ => &[],
            };
            attribute(name, description, values)
        })
        .collect();
    for event in c.events {
        attributes.push(attribute(
            format!("@{event}"),
            format!("Runs when the component sends {event}."),
            &[],
        ));
    }
    json!({"name": c.tag, "description": c.doc, "attributes": attributes})
}

fn app_component(tag: &str, c: &AppContract) -> Value {
    let attributes: Vec<Value> = c
        .props
        .iter()
        .map(|p| {
            let description = if p.required {
                "(required)".to_owned()
            } else if let Some(d) = &p.default {
                format!("Default: {d}")
            } else {
                String::new()
            };
            attribute(p.name.clone(), description, &[])
        })
        .collect();
    json!({"name": tag, "description": "An app component.", "attributes": attributes})
}

/// The data file for `contracts` and the app's components (their tags with their contracts).
pub(crate) fn custom_data(contracts: &[Contract], app: &[(String, AppContract)]) -> Value {
    let mut tags: Vec<Value> = contracts.iter().map(built_in).collect();
    tags.extend(app.iter().map(|(tag, c)| app_component(tag, c)));
    json!({"version": 1.1, "tags": tags, "globalAttributes": global_attributes()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::BUILTIN;

    #[test]
    fn matches_the_snapshot() {
        let mut text = serde_json::to_string_pretty(&custom_data(BUILTIN, &[])).unwrap();
        text.push('\n');
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/components/custom-data.json"
        );
        if std::env::var_os("RENOX_UPDATE_SNAPSHOT").is_some() {
            std::fs::write(path, &text).unwrap();
        }
        let saved = std::fs::read_to_string(path).unwrap_or_default();
        assert_eq!(
            saved.replace("\r\n", "\n"),
            text,
            "run with RENOX_UPDATE_SNAPSHOT=1 to rewrite custom-data.json"
        );
    }

    #[test]
    fn data_props_start_with_a_colon_and_globals_are_listed() {
        let v = custom_data(BUILTIN, &[]);
        let table = v["tags"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "rx-table")
            .unwrap();
        assert!(
            table["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["name"].as_str().unwrap().starts_with(':'))
        );
        assert_eq!(v["globalAttributes"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn app_components_are_listed() {
        let c = AppContract::default();
        let v = custom_data(&[], &[("app-price-tag".to_owned(), c)]);
        assert_eq!(v["tags"][0]["name"], "app-price-tag");
    }
}
