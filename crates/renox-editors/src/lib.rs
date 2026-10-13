//! Rich text, Markdown and code editor fields, and a code entry for detail
//! pages, for Renox apps. They bring JavaScript the UI kit doesn't carry
//! (Trix, CodeJar and Prism, compiled into this crate), loaded only on pages
//! that use them.
//!
//! ```
//! use renox::prelude::*;
//! use renox_editors::Editors;
//!
//! # let _ =
//! App::new().module(Editors::new())
//! # ;
//! ```
//!
//! In a form, next to the kit's fields:
//!
//! ```html
//! <rx-form route="posts.store" live>
//!   <rx-rich-editor name="body" label="Body" :value="post.body" required/>
//!   <rx-markdown-editor name="notes" label="Notes" :value="post.notes"/>
//!   <rx-code-editor name="settings" label="Settings" :value="post.settings" language="json"/>
//! </rx-form>
//! ```
//!
//! Each sends a plain form field of its name, so `Valid<T>`, old input,
//! errors and live validation work as for the kit's own fields. Rich text
//! arrives as HTML: read it as a [`RichText`], which cleans it, and show it
//! with the `rich_text` filter, which cleans it again. On a detail page,
//! `<rx-code-entry>` shows code highlighted, with a copy button. The guide is
//! docs/editors.md in the Renox repository.

use renox::minijinja;
use renox::prelude::*;
use serde::Deserialize;

mod assets;
pub mod rich_text;

pub use assets::{CODEJAR_VERSION, PRISM_VERSION, TRIX_VERSION};
pub use rich_text::{RichText, sanitize};

/// The macros and the preview, compiled in. An app replaces one with a file
/// of the same name under its views directory
/// (`resources/views/renox-editors/editors.html`).
const VIEWS: &[(&str, &str)] = &[
    (
        "renox-editors/editors.html",
        include_str!("../views/editors.html"),
    ),
    (
        "renox-editors/preview.html",
        include_str!("../views/preview.html"),
    ),
];

/// The longest Markdown the preview renders, in bytes.
const PREVIEW_LIMIT: usize = 200_000;

/// The editors module: add it to the app, then import the macros from
/// `renox-editors/editors.html`.
#[derive(Debug, Clone, Default)]
pub struct Editors {}

impl Editors {
    /// The module with its defaults.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Module for Editors {
    fn name(&self) -> &'static str {
        "editors"
    }

    fn routes(&self) -> Routes {
        // The Markdown editor's Preview tab: the text rendered as the
        // `markdown` filter renders it on the page.
        Routes::new()
            .post("/_renox/editors/preview", preview)
            .name("editors.preview")
            .throttle(120, std::time::Duration::from_secs(60))
    }

    fn register(&self, app: &mut Registry) {
        assets::register(app);
        components(app);
        app.templates(|env| {
            for (name, source) in VIEWS {
                // The app's own file of that name wins.
                if env.get_template(name).is_err() {
                    renox::view::add_template(env, *name, *source)
                        .expect("a built-in template compiles");
                }
            }
            env.add_function("renox_editors", || {
                minijinja::Value::from_safe_string(assets::tags())
            });
            // `{{ post.body | rich_text }}`: stored rich text, cleaned again
            // and shown as HTML.
            env.add_filter("rich_text", |html: Option<String>| {
                minijinja::Value::from_safe_string(sanitize(html.as_deref().unwrap_or("")))
            });
            // The rich editor's starting value: cleaned, and escaped by the
            // template as an attribute.
            env.add_function("renox_editors_html", |html: Option<String>| {
                sanitize(html.as_deref().unwrap_or(""))
            });
            // A code entry's text: strings as they are, other values as
            // indented JSON.
            env.add_function("renox_editors_code", code_text);
        });
    }
}

/// What the Preview tab sends.
#[derive(Deserialize)]
struct Preview {
    #[serde(default)]
    text: String,
}

async fn preview(Form(form): Form<Preview>) -> Result<Response> {
    if form.text.len() > PREVIEW_LIMIT {
        return Ok(StatusCode::PAYLOAD_TOO_LARGE.into_response());
    }
    Ok(view("renox-editors/preview.html", context! { text => form.text }).into_response())
}

/// The text a code entry shows for `value`.
fn code_text(value: minijinja::Value) -> String {
    if value.is_undefined() || value.is_none() {
        return String::new();
    }
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    renox::serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
}

/// Compiles the Rust in docs/editors.md (the guide) as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/editors.md")]
pub struct Guide;

/// The editors as tags: `<rx-rich-editor>`, `<rx-markdown-editor>`,
/// `<rx-code-editor>` and `<rx-code-entry>`.
fn components(app: &mut Registry) {
    use renox::view::{Component, Prop};
    const T: &str = "renox-editors/editors.html";
    let field = |tag: &str, mac: &str, doc: &str| {
        Component::macro_call(tag, T, mac)
            .doc(doc)
            .content(false)
            .prop(Prop::text("name").required())
            .prop(Prop::text("label").required())
            .prop(Prop::text("value"))
            .prop(Prop::text("hint"))
            .prop(Prop::bool("required"))
            .prop(Prop::text("placeholder"))
            .prop(Prop::number("rows"))
            .prop(Prop::text("id"))
            .prop(Prop::bool("disabled"))
            .prop(Prop::text("span"))
            .prop(Prop::bool("hide-label"))
            .prop(Prop::text("bag"))
    };
    app.component(field(
        "rx-rich-editor",
        "rich_editor",
        "A rich text editor (Trix) that sends HTML.",
    ));
    app.component(
        field(
            "rx-markdown-editor",
            "markdown_editor",
            "A Markdown editor with a toolbar and a preview.",
        )
        .prop(Prop::bool("readonly")),
    );
    app.component(
        field(
            "rx-code-editor",
            "code_editor",
            "A code editor with syntax highlighting.",
        )
        .prop(Prop::text("language"))
        .prop(Prop::text("tab"))
        .prop(Prop::bool("readonly")),
    );
    app.component(
        Component::macro_call("rx-code-entry", T, "code_entry")
            .doc("Code or JSON shown in an infolist, with a copy button.")
            .content(false)
            .prop(Prop::text("label").required())
            .prop(Prop::data("value"))
            .prop(Prop::text("language"))
            .prop(Prop::bool("copyable"))
            .prop(Prop::text("hint"))
            .prop(Prop::text("placeholder"))
            .prop(Prop::text("max-height"))
            .prop(Prop::text("span"))
            .prop(Prop::bool("hide-label"))
            .prop(Prop::text("id")),
    );
}

#[cfg(test)]
mod tests {
    use super::{code_text, minijinja};

    // #261: what a code entry shows for nothing, text and other values.
    #[test]
    fn code_entries_show_text_json_or_nothing() {
        assert_eq!(code_text(minijinja::Value::UNDEFINED), "");
        assert_eq!(code_text(minijinja::Value::from(())), "");
        assert_eq!(
            code_text(minijinja::Value::from("let x = 1;")),
            "let x = 1;"
        );
        let shown = code_text(minijinja::Value::from_serialize(
            renox::serde_json::json!({ "a": 1 }),
        ));
        assert!(shown.contains("\"a\": 1"), "{shown}");
    }
}
