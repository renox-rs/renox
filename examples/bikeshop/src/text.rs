//! Texts that come from Rust in English (the role and permission catalogue,
//! the data page's tables, renox-admin's labels), shown in the page's
//! language: `{{ tx(text) }}` looks the English text up under `text.<text>`
//! in `resources/lang/<locale>.json` and shows it as it is when the language
//! has no translation, as Laravel's JSON translations keyed by the English
//! text do. `{{ tk(key, fallback) }}` does the same for a key of the app's
//! own (`api.endpoints.<route>.purpose`).
//!
//! Both are template functions, so a template needs no import. They call the
//! page's own `t()`.

use renox::minijinja::{self, State, Value};

/// Registers `tx` and `tk` (`App::templates(text::register)`).
pub fn register(env: &mut minijinja::Environment<'static>) {
    env.add_function("tk", tk);
    env.add_function("tx", |state: &State, text: Value| {
        tk(state, text_key(&text), text)
    });
}

fn text_key(text: &Value) -> String {
    format!("text.{text}")
}

/// The translation of `key`, else `fallback`.
fn tk(state: &State, key: String, fallback: Value) -> Value {
    let Some(t) = state.lookup("t") else {
        return fallback;
    };
    match t.call(state, &[Value::from(key.as_str())]) {
        Ok(found) if found.to_string() != key => found,
        _ => fallback,
    }
}
