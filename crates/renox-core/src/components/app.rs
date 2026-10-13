//! App components: `<app-price-tag>` is `components/price_tag.html`, a template that starts with
//! `<rx-props …>` and compiles to one macro called `component`.

use super::attrs::{self, Kind};
use super::emit::{self, Import, Used, pad};
use super::scan::{self, Attr, Token};
use super::tree::Node;
use super::{Catalog, CompileError, suggest};

/// The template of an app component: `app-price-tag` gives `components/price_tag.html`.
pub(super) fn file_for(tag: &str) -> String {
    let name = tag.strip_prefix("app-").unwrap_or(tag);
    format!("components/{}.html", attrs::snake(name))
}

/// One prop declared by `<rx-props>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AppProp {
    /// The name as written in the usage (kebab case).
    pub name: String,
    /// Whether the usage must give it.
    pub required: bool,
    /// The default, as a MiniJinja expression.
    pub default: Option<String>,
}

/// What a component file promises.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct AppContract {
    /// The props, in the order written.
    pub props: Vec<AppProp>,
    /// Whether the file has an unnamed `<rx-slot />`.
    pub default_slot: bool,
    /// The names of its named outlets.
    pub slots: Vec<String>,
}

fn compile_err(e: CompileError, file: &str) -> String {
    format!("{file}:{}: {}", e.line, e.message)
}

fn is_outlet(name: &str, self_closing: bool) -> bool {
    name == "rx-slot" && self_closing
}

fn slot_name<'a>(attrs: &[Attr<'a>]) -> &'a str {
    attrs
        .iter()
        .find(|a| a.name == "name")
        .and_then(|a| a.value)
        .unwrap_or("")
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reads the first `<rx-props>` and the outlets of a component file.
pub(super) fn contract_of(file: &str, src: &str) -> Result<AppContract, String> {
    let tokens = scan::scan(src).map_err(|e| compile_err(e, file))?;
    let mut contract = AppContract::default();
    let mut found = false;
    for t in &tokens {
        let Token::Open {
            name,
            attrs,
            self_closing,
            line,
            ..
        } = t
        else {
            continue;
        };
        if name == "rx-props" && !found {
            found = true;
            for a in attrs {
                let (n, kind) = match a.name.strip_prefix(':') {
                    Some(n) => (n, true),
                    None => (a.name, false),
                };
                let (required, default) = match (a.value, kind) {
                    (None, _) => (true, None),
                    (Some(v), true) => {
                        let v = one_line(v);
                        if v.is_empty() {
                            return Err(format!(
                                "{file}:{}: <rx-props> \":{n}\" needs an expression",
                                a.line
                            ));
                        }
                        (false, Some(format!("({v})")))
                    }
                    (Some(v), false) => (
                        false,
                        Some(
                            attrs::text_expr(v)
                                .map_err(|m| format!("{file}:{}: <rx-props> {m}", a.line))?,
                        ),
                    ),
                };
                contract.props.push(AppProp {
                    name: n.to_owned(),
                    required,
                    default,
                });
            }
        } else if is_outlet(name, *self_closing) {
            let n = slot_name(attrs);
            if n.is_empty() {
                contract.default_slot = true;
            } else if !n
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(format!(
                    "{file}:{line}: <rx-slot> name \"{n}\" is not a name"
                ));
            } else if !contract.slots.iter().any(|s| s == n) {
                contract.slots.push(n.to_owned());
            }
        }
    }
    if !found {
        return Err(format!(
            "{file} has no <rx-props>: it holds macros; use {{% from \"{file}\" import … %}}"
        ));
    }
    Ok(contract)
}

/// For a component file: the `{% macro %}` header and the body with `<rx-props>` and the
/// outlets replaced. `None` when the source has no `<rx-props>`.
pub(super) fn component_file(src: &str) -> Result<Option<(String, String)>, CompileError> {
    if !src.contains("<rx-props") {
        return Ok(None);
    }
    let tokens = scan::scan(src)?;
    let mut body = String::with_capacity(src.len());
    let mut at = 0;
    let mut contract = None;
    for t in &tokens {
        let Token::Open {
            name,
            attrs,
            self_closing,
            span,
            line,
        } = t
        else {
            continue;
        };
        let replacement = if name == "rx-props" {
            if contract.is_some() {
                return Err(CompileError {
                    line: *line,
                    message: "a component has one <rx-props>".to_owned(),
                });
            }
            contract = Some(contract_of("", src).map_err(|m| CompileError {
                line: *line,
                message: m.trim_start_matches(':').to_owned(),
            })?);
            String::new()
        } else if is_outlet(name, *self_closing) {
            let n = slot_name(attrs);
            if n.is_empty() {
                "{{ caller() if caller is defined else \"\" }}".to_owned()
            } else {
                format!("{{{{ {} or \"\" }}}}", attrs::snake(n))
            }
        } else {
            continue;
        };
        body.push_str(&src[at..span.start]);
        body.push_str(&replacement);
        body.push_str(&pad(&src[span.clone()]));
        at = span.end;
    }
    body.push_str(&src[at..]);
    let Some(contract) = contract else {
        return Ok(None);
    };
    let mut params: Vec<String> = Vec::new();
    for p in contract.props.iter().filter(|p| p.required) {
        params.push(attrs::snake(&p.name));
    }
    for p in contract.props.iter().filter(|p| !p.required) {
        let d = p.default.as_deref().unwrap_or("none");
        params.push(format!("{}={d}", attrs::snake(&p.name)));
    }
    for s in &contract.slots {
        params.push(format!("{}=none", attrs::snake(s)));
    }
    Ok(Some((
        format!("{{% macro component({}) %}}", params.join(", ")),
        body,
    )))
}

/// An `<app-…>` usage: validated against its file's `<rx-props>`, imported and called.
pub(super) fn usage(
    src: &str,
    node: &Node<'_>,
    attrs: &[Attr<'_>],
    catalog: &Catalog,
    used: &mut Used,
    counter: &mut usize,
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
    let err = |m: String| CompileError {
        line: *line,
        message: m,
    };
    let file = file_for(name);
    let Some(source) = (catalog.lookup)(&file) else {
        return Err(err(format!("<{name}> needs resources/views/{file}")));
    };
    let contract = contract_of(&file, &source).map_err(err)?;

    let mut named: Vec<(String, Vec<Node<'_>>)> = Vec::new();
    let mut rest: Vec<Node<'_>> = Vec::new();
    for child in children {
        if let Node::Element {
            name: n,
            attrs: a,
            children: inner,
            line: at,
            ..
        } = child
            && n == "rx-slot"
        {
            let want = slot_name(a);
            if want.is_empty() {
                return Err(CompileError {
                    line: *at,
                    message: "<rx-slot> needs a name here".to_owned(),
                });
            }
            if !contract.slots.iter().any(|s| s == want) {
                let hint = suggest::did_you_mean(want, contract.slots.iter().map(String::as_str))
                    .map(|y| format!("; did you mean \"{y}\"?"))
                    .unwrap_or_else(|| ".".to_owned());
                return Err(CompileError {
                    line: *at,
                    message: format!("<{name}> has no slot \"{want}\"{hint}"),
                });
            }
            named.push((want.to_owned(), inner.clone()));
        } else {
            rest.push(child.clone());
        }
    }
    let has_default = emit::has_content(src, &rest);
    if has_default && !contract.default_slot {
        return Err(err(format!("<{name}> takes no content")));
    }

    let names: Vec<&str> = contract.props.iter().map(|p| p.name.as_str()).collect();
    let required: Vec<&str> = contract
        .props
        .iter()
        .filter(|p| p.required)
        .map(|p| p.name.as_str())
        .collect();
    emit::check_names(name, &names, &required, false, attrs, &[]).map_err(err)?;

    let mut args = Vec::new();
    for a in attrs {
        let n = a.name.strip_prefix(':').unwrap_or(a.name);
        let e = attrs::prop_expr(a, Kind::Text, &[]).map_err(|m| CompileError {
            line: a.line,
            message: format!("<{name}> {m}"),
        })?;
        args.push(format!("{}={e}", attrs::snake(n)));
    }
    let mut pre = String::new();
    for (slot, nodes) in &named {
        *counter += 1;
        let n = *counter;
        let mut body = String::new();
        emit::emit_into(src, nodes, catalog, Some(name), used, counter, &mut body)?;
        pre.push_str(&format!("{{% set __rx_slot_{n} %}}{body}{{% endset %}}"));
        args.push(format!("{}=__rx_slot_{n}", attrs::snake(slot)));
    }
    let alias = format!(
        "__app_{}",
        attrs::snake(name.strip_prefix("app-").unwrap_or(name))
    );
    used.insert(Import {
        alias: alias.clone(),
        path: file,
    });
    out.push_str(&pre);
    let call = format!("{alias}.component({})", args.join(", "));
    if has_default {
        out.push_str(&format!("{{% call {call} %}}"));
        out.push_str(&pad(&src[open.clone()]));
        emit::emit_into(src, &rest, catalog, Some(name), used, counter, out)?;
        out.push_str("{% endcall %}");
    } else {
        out.push_str(&format!("{{{{ {call} }}}}"));
        out.push_str(&pad(&src[open.clone()]));
        for c in children {
            if let Node::Text(span) = c {
                out.push_str(&pad(&src[span.clone()]));
            }
        }
    }
    if let Some(c) = close {
        out.push_str(&pad(&src[c.clone()]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(file_for("app-price-tag"), "components/price_tag.html");
        assert_eq!(file_for("app-x"), "components/x.html");
    }

    #[test]
    fn contract_reads_props_and_outlets() {
        let c = contract_of(
            "components/x.html",
            "<rx-props amount currency=\"USD\" :n=\"1 + 1\">\n<rx-slot /><rx-slot name=\"foot\" />",
        )
        .unwrap();
        assert_eq!(c.props.len(), 3);
        assert!(c.props[0].required);
        assert_eq!(c.props[1].default.as_deref(), Some("\"USD\""));
        assert_eq!(c.props[2].default.as_deref(), Some("(1 + 1)"));
        assert!(c.default_slot);
        assert_eq!(c.slots, ["foot"]);
    }

    #[test]
    fn no_props_is_an_error() {
        let e = contract_of("components/x.html", "{% macro a() %}{% endmacro %}").unwrap_err();
        assert_eq!(
            e,
            "components/x.html has no <rx-props>: it holds macros; use {% from \"components/x.html\" import … %}"
        );
    }
}
