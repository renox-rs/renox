//! The route that runs a live component's actions.

use axum::Router;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::Value;

use super::LiveContext;
use crate::{AppState, Error, Result};

/// `POST /_renox/live/{component}/{action}`.
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/_renox/live/{component}/{action}", post(call))
}

async fn call(
    axum::extract::Path((component, action)): axum::extract::Path<(String, String)>,
    ctx: LiveContext,
    axum::Form(fields): axum::Form<Vec<(String, String)>>,
) -> Result<Response> {
    let run = ctx
        .state
        .live_components
        .get(component.as_str())
        .cloned()
        .ok_or(Error::NotFound)?;
    run(ctx, action, fields).await
}

fn bad(message: String) -> Error {
    Error::BadRequest(message)
}

/// Writes posted fields over the state's top-level keys, converting each text
/// to the type the field holds now. Keys the state lacks and `_` names are skipped.
pub(crate) fn merge_fields(state: &mut Value, fields: &[(String, String)]) -> Result {
    let Some(map) = state.as_object_mut() else {
        return Ok(());
    };
    for (name, text) in fields {
        if name.starts_with('_') {
            continue;
        }
        let Some(slot) = map.get_mut(name) else {
            continue;
        };
        let new = match slot {
            Value::Number(_) => {
                if let Ok(i) = text.trim().parse::<i64>() {
                    Value::from(i)
                } else if let Some(n) = text
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                {
                    Value::Number(n)
                } else {
                    return Err(bad(format!("`{name}` must be a number.")));
                }
            }
            Value::Bool(_) => Value::Bool(matches!(text.as_str(), "true" | "1" | "on")),
            Value::Null if text.is_empty() => Value::Null,
            Value::Null | Value::String(_) => Value::String(text.clone()),
            // Arrays and objects are not form fields.
            _ => continue,
        };
        *slot = new;
    }
    Ok(())
}

/// Builds the entry point registered for `C`.
pub(crate) async fn run<C: super::LiveComponent>(
    mut ctx: LiveContext,
    action: String,
    fields: Vec<(String, String)>,
) -> Result<Response> {
    {
        ctx.component = C::NAME;
        let field = |name: &str| fields.iter().find(|(n, _)| n == name).map(|(_, v)| v);
        let sealed = field("_snapshot")
            .ok_or_else(|| bad("The live snapshot is missing.".into()))?
            .clone();
        let args: Vec<Value> = match field("_args") {
            None => Vec::new(),
            Some(text) => match serde_json::from_str::<Value>(text) {
                Ok(Value::Array(a)) => a,
                _ => return Err(bad("`_args` must be a JSON array.".into())),
            },
        };
        let (id, mut value) = super::snapshot::open(
            ctx.state.key.signing(),
            ctx.state.config.live_snapshot_max,
            C::NAME,
            &sealed,
        )?;
        merge_fields(&mut value, &fields)?;
        let mut c: C = serde_json::from_value(value).map_err(|e| bad(e.to_string()))?;
        if action != "_refresh" {
            if action.starts_with('_') {
                return Err(Error::NotFound);
            }
            c.call(&action, args, &mut ctx).await?;
        }
        let m = super::render(&ctx, &c, id, true).await?;
        let mut res = (
            ctx.toast.take(),
            ctx.redirect.take().map(crate::htmx::HxRedirect),
            crate::view::view("renox/live.html", minijinja::context! { component => m }),
        )
            .into_response();
        for (name, detail) in std::mem::take(&mut ctx.events) {
            crate::htmx::add_trigger(&mut res, &name, detail);
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn f(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn numbers_convert() {
        let mut s = json!({"n": 1, "x": 1.5});
        merge_fields(&mut s, &f(&[("n", "7"), ("x", "2.5")])).unwrap();
        assert_eq!(s, json!({"n": 7, "x": 2.5}));
        assert!(merge_fields(&mut s, &f(&[("n", "abc")])).is_err());
    }

    #[test]
    fn bools_and_strings() {
        let mut s = json!({"a": false, "b": true, "t": "x", "z": null, "w": null});
        merge_fields(
            &mut s,
            &f(&[("a", "on"), ("b", "no"), ("t", ""), ("z", ""), ("w", "hi")]),
        )
        .unwrap();
        assert_eq!(
            s,
            json!({"a": true, "b": false, "t": "", "z": null, "w": "hi"})
        );
    }

    #[test]
    fn unknown_underscore_and_nested_are_skipped() {
        let mut s = json!({"list": [1], "_k": 1});
        merge_fields(&mut s, &f(&[("list", "x"), ("_k", "2"), ("other", "1")])).unwrap();
        assert_eq!(s, json!({"list": [1], "_k": 1}));
    }
}
