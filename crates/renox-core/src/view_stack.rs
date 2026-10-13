//! `push` / `stack`: a page (or a component) adds markup to a named place in
//! the layout, e.g. a script at the end of `<body>` or a style in `<head>`:
//!
//! ```html
//! {# layouts/app.html #}
//! <head>… {{ stack('head') }}</head>
//! <body>… {{ stack('scripts') }}</body>
//!
//! {# a page or a component #}
//! {% call push('scripts') %}<script src="{{ asset('chart.js') }}"></script>{% endcall %}
//! {% call push('scripts', once='chart') %}…{% endcall %}   {# at most once per page #}
//! {% call prepend('head') %}<link rel="preload" …>{% endcall %}
//! ```
//!
//! The layout's head is rendered before the page's blocks, so `stack` leaves
//! a marker and the markers are filled in when the page is done. Rendering
//! is synchronous, so what's pushed is kept per thread while a page renders.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use minijinja::value::{Kwargs, Value};
use minijinja::{Environment, Error, State};

#[derive(Default)]
struct Stacks {
    /// Makes the markers of this render unguessable, so a user's text that
    /// looks like one is left alone.
    nonce: String,
    items: HashMap<String, Vec<String>>,
    once: HashSet<String>,
}

thread_local! {
    static CURRENT: RefCell<Option<Stacks>> = const { RefCell::new(None) };
}

/// Collects pushes while alive; `finish` fills the page's stacks.
pub(crate) struct Scope(Option<Stacks>);

impl Scope {
    pub(crate) fn begin() -> Self {
        let fresh = Stacks {
            nonce: crate::random_token()[..12].to_owned(),
            ..Stacks::default()
        };
        Self(CURRENT.with(|c| c.borrow_mut().replace(fresh)))
    }

    /// `html` with each `stack(name)` replaced by what was pushed to it.
    pub(crate) fn finish(self, html: String) -> String {
        let Some(stacks) = CURRENT.with(|c| c.borrow_mut().take()) else {
            return html;
        };
        let prefix = marker_prefix(&stacks.nonce);
        if !html.contains(&prefix) {
            return html;
        }
        let mut out = String::with_capacity(html.len());
        let mut rest = html.as_str();
        while let Some(start) = rest.find(&prefix) {
            out.push_str(&rest[..start]);
            let after = &rest[start + prefix.len()..];
            let Some(end) = after.find(END) else {
                out.push_str(&rest[start..]);
                rest = "";
                break;
            };
            if let Some(items) = stacks.items.get(&after[..end]) {
                out.push_str(&items.concat());
            }
            rest = &after[end + END.len()..];
        }
        out.push_str(rest);
        out
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT.with(|c| *c.borrow_mut() = previous);
    }
}

const END: &str = "-->";

fn marker_prefix(nonce: &str) -> String {
    format!("<!--renox-stack:{nonce}:")
}

/// Adds `stack`, `push` and `prepend` to the environment.
pub(crate) fn register(env: &mut Environment<'_>) {
    env.add_function("stack", |name: String| -> Value {
        CURRENT.with(|c| match &*c.borrow() {
            Some(stacks) => {
                Value::from_safe_string(format!("{}{name}{END}", marker_prefix(&stacks.nonce)))
            }
            // Outside a page (a mail, `views.render`): nothing to fill in.
            None => Value::from(""),
        })
    });
    env.add_function("push", |state: &State, name: String, kwargs: Kwargs| {
        add(state, name, kwargs, false)
    });
    env.add_function("prepend", |state: &State, name: String, kwargs: Kwargs| {
        add(state, name, kwargs, true)
    });
}

fn add(state: &State, name: String, kwargs: Kwargs, front: bool) -> Result<Value, Error> {
    let caller: Value = kwargs.get("caller").map_err(|_| {
        Error::new(
            minijinja::ErrorKind::MissingArgument,
            "use it as {% call push('name') %}…{% endcall %}",
        )
    })?;
    let once: Option<String> = kwargs.get("once")?;
    kwargs.assert_all_used()?;
    let body = caller.call(state, &[])?.to_string();
    CURRENT.with(|c| {
        if let Some(stacks) = &mut *c.borrow_mut() {
            if let Some(key) = once
                && !stacks.once.insert(format!("{name}\u{0}{key}"))
            {
                return;
            }
            let items = stacks.items.entry(name).or_default();
            if front {
                items.insert(0, body);
            } else {
                items.push(body);
            }
        }
    });
    Ok(Value::from(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(templates: &[(&'static str, &'static str)], name: &str) -> String {
        let mut env = Environment::new();
        register(&mut env);
        for (n, src) in templates {
            env.add_template(n, src).unwrap();
        }
        let scope = Scope::begin();
        let html = env.get_template(name).unwrap().render(()).unwrap();
        scope.finish(html)
    }

    #[test]
    fn pushes_reach_stacks_rendered_before_them() {
        let html = render(
            &[
                (
                    "layout.html",
                    "<head>{{ stack('head') }}</head>{% block body %}{% endblock %}<end>{{ stack('scripts') }}</end>{{ stack('empty') }}",
                ),
                (
                    "part.html",
                    "{% macro chart() %}{% call push('scripts', once='chart') %}<script src=c.js></script>{% endcall %}chart{% endmacro %}",
                ),
                (
                    "page.html",
                    "{% extends 'layout.html' %}{% from 'part.html' import chart %}{% block body %}{% call push('head') %}<style>a{}</style>{% endcall %}{{ chart() }}{{ chart() }}{% call push('scripts') %}<script>1</script>{% endcall %}{% call prepend('scripts') %}<script>0</script>{% endcall %}{% endblock %}",
                ),
            ],
            "page.html",
        );
        assert_eq!(
            html,
            "<head><style>a{}</style></head>chartchart<end><script>0</script><script src=c.js></script><script>1</script></end>"
        );
    }

    #[test]
    fn markers_without_a_scope_render_nothing() {
        let mut env = Environment::new();
        register(&mut env);
        let html = env
            .render_str("[{{ stack('x') }}]{% call push('x') %}y{% endcall %}", ())
            .unwrap();
        assert_eq!(html, "[]");
    }

    #[test]
    fn a_marker_cut_short_is_kept_as_text() {
        let scope = Scope::begin();
        let nonce = CURRENT.with(|c| c.borrow().as_ref().unwrap().nonce.clone());
        let html = format!("<p>a</p>{}scripts (no end)", marker_prefix(&nonce));
        assert_eq!(scope.finish(html.clone()), html);
    }

    #[test]
    fn a_scope_whose_stacks_are_gone_returns_the_page_as_it_is() {
        // Only a scope inside another's render could take them; the page
        // then comes back untouched rather than losing its markers.
        let scope = Scope::begin();
        CURRENT.with(|c| c.borrow_mut().take());
        assert_eq!(scope.finish("page".into()), "page");
    }

    #[test]
    fn push_outside_a_call_block_says_how_to_use_it() {
        let mut env = Environment::new();
        register(&mut env);
        let err = env.render_str("{{ push('scripts') }}", ()).unwrap_err();
        assert!(
            err.to_string()
                .contains("use it as {% call push('name') %}…{% endcall %}"),
            "{err}"
        );
    }
}
