//! `<live-NAME :component="x" @event="…" class="…" />`: the include of `renox/live.html`.
//!
//! The component comes from `:component`; every other attribute lands on the wrapper through
//! `live_attrs`, and `@name` becomes `x-on:rx:NAME:name.self`. The `rx-click`, `rx-submit` and
//! `rx-model` attributes of the elements inside a live view are plain attributes: the compiler
//! never reads them (only `rx-if`, `rx-else` and `rx-for` are directives).

use super::CompileError;
use super::attrs::{self, Kind};
use super::emit::{self, pad};
use super::scan::Attr;
use super::tree::Node;

/// A `<live-…>` usage, written as the include form.
pub(super) fn usage(
    src: &str,
    node: &Node<'_>,
    attrs: &[Attr<'_>],
    out: &mut String,
) -> Result<(), CompileError> {
    let Node::Element {
        name,
        children,
        open,
        close,
        line,
        ..
    } = node
    else {
        return Ok(());
    };
    let err = |line: usize, m: String| CompileError { line, message: m };
    let short = name.strip_prefix("live-").unwrap_or(name);
    if short.is_empty() {
        return Err(err(
            *line,
            "<live-> needs a name after \"live-\"".to_owned(),
        ));
    }
    if emit::has_content(src, children) {
        return Err(err(*line, format!("<{name}> takes no content")));
    }
    let mut component = None;
    let mut pairs: Vec<String> = Vec::new();
    for a in attrs {
        let bare = a.name.strip_prefix(':').unwrap_or(a.name);
        if bare == "component" {
            if !a.name.starts_with(':') {
                return Err(err(
                    a.line,
                    format!("<{name}> \"component\" takes data: write :component=\"…\""),
                ));
            }
            component = Some(
                attrs::prop_expr(a, Kind::Data, &[])
                    .map_err(|m| err(a.line, format!("<{name}> {m}")))?,
            );
            continue;
        }
        if let Some(event) = a.name.strip_prefix('@') {
            if event.is_empty() {
                return Err(err(a.line, format!("<{name}> has an \"@\" without a name")));
            }
            let value = a.value.unwrap_or("");
            if value.trim().is_empty() {
                return Err(err(
                    a.line,
                    format!("<{name}> \"@{event}\" needs an expression"),
                ));
            }
            let key = format!("x-on:rx:{short}:{event}.self");
            pairs.push(format!(
                "{}: {}",
                attrs::literal(&key),
                attrs::literal(value)
            ));
            continue;
        }
        let value = attrs::prop_expr(a, Kind::Text, &[])
            .map_err(|m| err(a.line, format!("<{name}> {m}")))?;
        pairs.push(format!("{}: {value}", attrs::literal(bare)));
    }
    let Some(component) = component else {
        return Err(err(
            *line,
            format!("<{name}> needs the attribute \"component\""),
        ));
    };
    let with = if pairs.is_empty() {
        format!("component = {component}")
    } else {
        format!(
            "component = {component}, live_attrs = {{{}}}",
            pairs.join(", ")
        )
    };
    out.push_str(&format!(
        "{{% with {with} %}}{{% include \"renox/live.html\" %}}{{% endwith %}}"
    ));
    out.push_str(&pad(&src[open.clone()]));
    if let Some(c) = close {
        out.push_str(&pad(&src[c.clone()]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{Catalog, compile};

    fn run(src: &str) -> Result<String, String> {
        let catalog = Catalog {
            contracts: super::super::BUILTIN,
            lookup: &|_| None,
        };
        compile("a.html", src, &catalog).map_err(|e| e.message)
    }

    #[test]
    fn the_tag_becomes_the_include() {
        assert_eq!(
            run("<live-checklist :component=\"list\" />").unwrap(),
            "{% with component = (list) %}{% include \"renox/live.html\" %}{% endwith %}"
        );
    }

    #[test]
    fn events_and_other_attributes_go_to_the_wrapper() {
        let out = run(
            "<live-checklist :component=\"list\" @saved=\"open = false\" class=\"box\" data-x=\"{{ n }}\" hidden></live-checklist>",
        )
        .unwrap();
        assert_eq!(
            out,
            "{% with component = (list), live_attrs = {\"x-on:rx:checklist:saved.self\": \"open = false\", \"class\": \"box\", \"data-x\": (n), \"hidden\": true} %}{% include \"renox/live.html\" %}{% endwith %}"
        );
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(
            run("<live-x />").unwrap_err(),
            "<live-x> needs the attribute \"component\""
        );
        assert_eq!(
            run("<live-x component=\"a\" />").unwrap_err(),
            "<live-x> \"component\" takes data: write :component=\"…\""
        );
        assert_eq!(
            run("<live-x :component=\"a\">hi</live-x>").unwrap_err(),
            "<live-x> takes no content"
        );
    }

    #[test]
    fn rx_attributes_are_left_alone() {
        let src = "<button rx-click=\"add(1)\">+</button><form rx-submit=\"save\"><input rx-model=\"t\"><input rx-model.live=\"q\"><input rx-model.blur=\"n\"></form>";
        assert_eq!(run(src).unwrap(), src);
        let mixed = "<rx-row rx-click=\"go\"><p rx-model.live=\"q\" rx-if=\"a\">x</p></rx-row>";
        let out = run(mixed).unwrap();
        assert!(out.contains("rx-click=\"go\""), "{out}");
        assert!(out.contains("rx-model.live=\"q\""), "{out}");
        assert!(!out.contains("rx-if"), "{out}");
    }
}
