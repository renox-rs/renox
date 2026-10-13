//! Code generation: the tree back to template source, components replaced by plain MiniJinja or
//! HTML. Generated code never holds a line break; a replaced span that had some keeps them inside
//! a `{# … #}` comment, so the lines of the source stay where they were (Decision 8 of #372).

use super::attrs::{self, Kind};
use super::contracts::{Contract, Render, Slot, Special};
use super::scan::Attr;
use super::special::{self, Ctx};
use super::tree::{self, Node};
use super::{Catalog, CompileError, suggest};
use std::collections::BTreeSet;

/// A template module imported at the top of the output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Import {
    pub alias: String,
    pub path: String,
}

pub(super) type Used = BTreeSet<Import>;

/// A comment holding as many line breaks as `span_text` has, or nothing.
pub(super) fn pad(span_text: &str) -> String {
    let k = span_text.matches('\n').count();
    if k == 0 {
        String::new()
    } else {
        format!("{{#{}#}}", "\n".repeat(k))
    }
}

thread_local! {
    /// The row variable and key expression while a table's cells are emitted (Decision 18).
    static ROW: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` with `var` as the row variable and `key` as the row's key: `route="n"` becomes
/// `route("n", key)` and `can="a"` becomes `can("a", var)`.
pub(super) fn with_row<T>(var: &str, key: &str, f: impl FnOnce() -> T) -> T {
    let before = ROW.with(|r| r.replace(Some((var.to_owned(), key.to_owned()))));
    let result = with_names(vec![var.to_owned()], f);
    ROW.with(|r| *r.borrow_mut() = before);
    result
}

thread_local! {
    /// The names a template has bound at this point (loop variables, table rows, `rx-let`), so
    /// `<rx-let>` can refuse a name that is taken (#511).
    static NAMES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Runs `f` with `names` bound.
pub(super) fn with_names<T>(names: Vec<String>, f: impl FnOnce() -> T) -> T {
    let len = NAMES.with(|n| {
        let mut n = n.borrow_mut();
        let len = n.len();
        n.extend(names);
        len
    });
    let result = f();
    NAMES.with(|n| n.borrow_mut().truncate(len));
    result
}

/// Whether `name` is bound by an enclosing loop, table row or `rx-let`.
pub(super) fn name_bound(name: &str) -> bool {
    NAMES.with(|n| n.borrow().iter().any(|x| x == name))
}

/// The names an `rx-for` value (`"a, b in xs"`) binds.
fn loop_names(each: &str) -> Vec<String> {
    let head = each.split(" in ").next().unwrap_or("");
    head.split(',')
        .map(|n| n.trim().trim_matches(['(', ')']).trim().to_owned())
        .filter(|n| !n.is_empty())
        .collect()
}

pub(super) fn row() -> Option<(String, String)> {
    ROW.with(|r| r.borrow().clone())
}

fn bare(name: &str) -> &str {
    name.strip_prefix(':').unwrap_or(name)
}

/// Checks the attributes against a list of prop names; `open` lets other attributes through.
pub(super) fn check_names(
    tag: &str,
    names: &[&str],
    required: &[&str],
    open: bool,
    attrs: &[Attr<'_>],
    filled: &[&str],
) -> Result<(), String> {
    for a in attrs {
        let n = bare(a.name);
        if names.contains(&n) || open {
            continue;
        }
        let hint = suggest::did_you_mean(n, names.iter().copied())
            .map(|x| format!("; did you mean \"{x}\"?"))
            .unwrap_or_else(|| ".".to_owned());
        let takes = if names.is_empty() {
            "It takes no attributes".to_owned()
        } else {
            format!("It takes: {}", names.join(", "))
        };
        return Err(format!("<{tag}> has no attribute \"{n}\"{hint} {takes}"));
    }
    for p in required {
        if !attrs.iter().any(|a| bare(a.name) == *p) && !filled.contains(p) {
            return Err(format!("<{tag}> needs the attribute \"{p}\""));
        }
    }
    Ok(())
}

/// The DOM events `@name` passes through even when a declared event is spelt almost the same.
const DOM_EVENTS: &[&str] = &[
    "click",
    "dblclick",
    "keydown",
    "keyup",
    "input",
    "change",
    "submit",
    "focus",
    "blur",
    "focusin",
    "focusout",
    "mouseenter",
    "mouseleave",
    "mouseover",
    "mouseout",
    "pointerdown",
    "pointerup",
    "scroll",
    "load",
    "toggle",
    "close",
    "cancel",
];

/// The event name of `@name.mod1.mod2`.
fn event_name(spec: &str) -> &str {
    spec.split('.').next().unwrap_or("")
}

/// The Alpine key of `@name[.mods]` when `name` is an event the component declares:
/// `x-on:rx:<component>:<name>.self[.mods]`.
fn event_key(contract: &Contract, spec: &str) -> Option<String> {
    let name = event_name(spec);
    if !contract.events.contains(&name) {
        return None;
    }
    let component = contract.tag.strip_prefix("rx-").unwrap_or(contract.tag);
    let mods = &spec[name.len()..];
    Some(format!("x-on:rx:{component}:{name}.self{mods}"))
}

/// Decision 7 of #373: `@x` on a component is a mistake when `x` is neither a declared event nor
/// a DOM event, but is close to a declared one.
fn check_event(contract: &Contract, spec: &str) -> Result<(), String> {
    let name = event_name(spec);
    if contract.events.is_empty()
        || contract.events.contains(&name)
        || DOM_EVENTS.contains(&name)
        || !suggest::near(name, contract.events.iter().copied())
    {
        return Ok(());
    }
    let hint = suggest::did_you_mean(name, contract.events.iter().copied())
        .map(|x| format!("; did you mean \"@{x}\"?"))
        .unwrap_or_else(|| ".".to_owned());
    Err(format!(
        "<{}> has no event \"{name}\"{hint} It sends: {}",
        contract.tag,
        contract.events.join(", ")
    ))
}

/// A conditional attribute, `?name="expression"` (#518): its name and expression. The attribute
/// is left out when the value is `false`, `none` or undefined, written bare for `true`, and
/// `name="text"` for anything else.
pub(super) fn cond_attr(a: &Attr<'_>, tag: &str) -> Result<(String, String), CompileError> {
    let name = a.name.strip_prefix('?').unwrap_or(a.name);
    let value = a.value.unwrap_or("").trim();
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'));
    if !valid {
        return Err(err(
            a.line,
            format!("<{tag}> \"?{name}\" needs an attribute name of letters, digits, - _ : ."),
        ));
    }
    if value.is_empty() {
        return Err(err(
            a.line,
            format!("<{tag}> \"?{name}\" needs an expression: ?{name}=\"expr\""),
        ));
    }
    Ok((name.to_owned(), value.replace(['\n', '\r'], " ")))
}

/// What `?name="expr"` is replaced with inside a start tag.
pub(super) fn cond_attr_code(a: &Attr<'_>, tag: &str) -> Result<String, CompileError> {
    let (name, expr) = cond_attr(a, tag)?;
    Ok(format!(
        "{{{{ rx_attr({}, ({expr})) }}}}",
        attrs::literal(&name)
    ))
}

/// The `attrs` pairs for the attributes that are not props (`"key": value`), in order. A declared
/// `@event` gets its Alpine key; everything else keeps its name.
pub(super) fn passthrough_pairs(
    contract: &Contract,
    attrs: &[Attr<'_>],
) -> Result<Vec<String>, CompileError> {
    let mut pairs = Vec::new();
    for a in attrs {
        let n = bare(a.name);
        if n == "route" || contract.props.iter().any(|p| p.name == n) {
            continue;
        }
        if let Some(spec) = a.name.strip_prefix('@')
            && let Some(key) = event_key(contract, spec)
        {
            pairs.push(format!(
                "{}: {}",
                attrs::literal(&key),
                attrs::literal(a.value.unwrap_or(""))
            ));
            continue;
        }
        if a.name.starts_with('?') {
            let (name, expr) = cond_attr(a, contract.tag)?;
            pairs.push(format!(
                "{}: rx_attr_value(({expr}))",
                attrs::literal(&name)
            ));
            continue;
        }
        let e = attrs::prop_expr(a, Kind::Text, &[])
            .map_err(|m| err(a.line, format!("<{}> {m}", contract.tag)))?;
        pairs.push(format!("{}: {e}", attrs::literal(n)));
    }
    Ok(pairs)
}

/// Checks the attributes against the contract.
pub(super) fn check_attrs(
    contract: &Contract,
    attrs: &[Attr<'_>],
    filled: &[&str],
) -> Result<(), String> {
    let tag = contract.tag;
    let element = matches!(
        contract.render,
        Render::Element { .. } | Render::Special(Special::Form | Special::Script | Special::Style)
    );
    let route = attrs.iter().find(|a| bare(a.name) == "route");
    if route.is_some() {
        let Some(prop) = contract.route_prop else {
            return Err(format!("<{tag}> has no route shortcut"));
        };
        if attrs.iter().any(|a| bare(a.name) == prop) {
            return Err(format!("<{tag}> takes route or \"{prop}\", not both"));
        }
    }
    for a in attrs {
        let n = bare(a.name);
        if n == "route" {
            continue;
        }
        if let Some(spec) = a.name.strip_prefix('@') {
            if event_name(spec).is_empty() {
                return Err(format!("<{tag}> has an \"@\" without a name"));
            }
            check_event(contract, spec)?;
            if event_key(contract, spec).is_some() || contract.attrs {
                continue;
            }
        }
        if contract.props.iter().any(|p| p.name == n)
            || (contract.attrs && (element || !a.name.starts_with(':')))
        {
            continue;
        }
        let names: Vec<&str> = contract.props.iter().map(|p| p.name).collect();
        let hint = suggest::did_you_mean(n, names.iter().copied())
            .map(|x| format!("; did you mean \"{x}\"?"))
            .unwrap_or_else(|| ".".to_owned());
        let takes = if names.is_empty() {
            "It takes no attributes".to_owned()
        } else {
            format!("It takes: {}", names.join(", "))
        };
        return Err(format!("<{tag}> has no attribute \"{n}\"{hint} {takes}"));
    }
    for p in contract.props.iter().filter(|p| p.required) {
        let by_route = route.is_some() && contract.route_prop == Some(p.name);
        if !attrs.iter().any(|a| bare(a.name) == p.name) && !filled.contains(&p.name) && !by_route {
            return Err(format!("<{tag}> needs the attribute \"{}\"", p.name));
        }
    }
    Ok(())
}

pub(super) fn err(line: usize, message: String) -> CompileError {
    CompileError { line, message }
}

/// The tree as template source.
pub(crate) fn emit(
    src: &str,
    nodes: &[Node<'_>],
    catalog: &Catalog,
) -> Result<String, CompileError> {
    let mut out = String::with_capacity(src.len());
    let mut used = Used::new();
    let mut counter = 0;
    let root_of = |n: &Node<'_>| match n {
        Node::Element { name, .. } if name == "rx-page" => Some("rx-page"),
        Node::Element { name, .. } if name == "rx-mail" => Some("rx-mail"),
        Node::Element { name, .. } if name == "rx-mail-text" => Some("rx-mail-text"),
        _ => None,
    };
    let is_page = |n: &Node<'_>| root_of(n).is_some();
    if let Some(root) = nodes.iter().find_map(root_of) {
        let stray = nodes.iter().find_map(|n| match n {
            Node::Text(s) if !blank(&src[s.clone()]) => {
                Some(1 + src[..s.start].matches('\n').count())
            }
            Node::Element { line, .. } if !is_page(n) => Some(*line),
            _ => None,
        });
        let second = nodes
            .iter()
            .filter(|n| is_page(n))
            .nth(1)
            .and_then(|n| match n {
                Node::Element { line, .. } => Some(*line),
                _ => None,
            });
        if let Some(line) = stray.or(second) {
            return Err(err(line, format!("<{root}> must hold the whole template")));
        }
    }
    emit_into(src, nodes, catalog, None, &mut used, &mut counter, &mut out)?;
    if used.is_empty() {
        return Ok(out);
    }
    let mut head = String::new();
    for m in used {
        head.push_str(&format!("{{% import \"{}\" as {} %}}", m.path, m.alias));
    }
    head.push_str(&out);
    Ok(head)
}

/// The control-flow attributes of one element, taken out of its attribute list.
struct Flow<'a> {
    each: Option<String>,
    cond: Option<String>,
    elif: Option<String>,
    otherwise: bool,
    can: Option<String>,
    /// The attributes that stay.
    attrs: Vec<Attr<'a>>,
    /// Where the removed attributes were, with the whitespace before each.
    removed: Vec<std::ops::Range<usize>>,
}

impl Flow<'_> {
    /// The condition of the `{% if %}` (or `{% elif %}`), from `rx-if` / `rx-elif` and `can`.
    fn condition(&self) -> Option<String> {
        let can = self.can.as_ref().map(|a| match row() {
            Some((var, _)) => format!("can({a:?}, {var})"),
            None => format!("can({a:?})"),
        });
        match (self.cond.as_ref().or(self.elif.as_ref()), can) {
            (Some(c), Some(can)) if c.contains(" or ") => Some(format!("({c}) and {can}")),
            (Some(c), Some(can)) => Some(format!("{c} and {can}")),
            (Some(c), None) => Some(c.clone()),
            (None, can) => can,
        }
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn flow_of<'a>(
    src: &str,
    component: bool,
    attrs: &[Attr<'a>],
    open: &std::ops::Range<usize>,
) -> Result<Flow<'a>, CompileError> {
    let mut f = Flow {
        each: None,
        cond: None,
        elif: None,
        otherwise: false,
        can: None,
        attrs: Vec::new(),
        removed: Vec::new(),
    };
    for a in attrs {
        match a.name {
            "rx-for" => {
                let v = one_line(a.value.unwrap_or(""));
                let words: Vec<&str> = v.split(' ').collect();
                let ok = words
                    .iter()
                    .position(|w| *w == "in")
                    .is_some_and(|p| p > 0 && p + 1 < words.len());
                if !ok {
                    return Err(err(a.line, "rx-for needs \"item in list\"".to_owned()));
                }
                f.each = Some(v);
            }
            "rx-if" => {
                let v = one_line(a.value.unwrap_or(""));
                if v.is_empty() {
                    return Err(err(a.line, "rx-if needs a condition".to_owned()));
                }
                f.cond = Some(v);
            }
            "rx-elif" => {
                let v = one_line(a.value.unwrap_or(""));
                if v.is_empty() {
                    return Err(err(a.line, "rx-elif needs a condition".to_owned()));
                }
                f.elif = Some(v);
            }
            "rx-else" => f.otherwise = true,
            "can" if component => {
                let v = a.value.unwrap_or("").trim();
                if v.is_empty() {
                    return Err(err(a.line, "can needs an ability".to_owned()));
                }
                f.can = Some(v.to_owned());
            }
            _ => {
                f.attrs.push(a.clone());
                continue;
            }
        }
        let mut start = a.span.start;
        while start > open.start + 1 && src.as_bytes()[start - 1].is_ascii_whitespace() {
            start -= 1;
        }
        f.removed.push(start..a.span.end);
    }
    let line = attrs.first().map_or(0, |a| a.line);
    if f.otherwise && (f.cond.is_some() || f.each.is_some() || f.elif.is_some()) {
        return Err(err(
            line,
            "rx-else can't be combined with rx-if, rx-elif or rx-for".to_owned(),
        ));
    }
    if f.elif.is_some() && (f.cond.is_some() || f.each.is_some()) {
        return Err(err(
            line,
            "rx-elif can't be combined with rx-if or rx-for".to_owned(),
        ));
    }
    Ok(f)
}

/// Whether the text holds only whitespace and comments.
pub(super) fn blank(text: &str) -> bool {
    let mut t = text.trim_start();
    loop {
        if let Some(r) = t.strip_prefix("<!--") {
            let Some(p) = r.find("-->") else { return false };
            t = r[p + 3..].trim_start();
        } else if let Some(r) = t.strip_prefix("{#") {
            let Some(p) = r.find("#}") else { return false };
            t = r[p + 2..].trim_start();
        } else {
            return t.is_empty();
        }
    }
}

fn element_attrs<'n, 'a>(node: &'n Node<'a>) -> Option<(&'n [Attr<'a>], usize)> {
    match node {
        Node::Element { attrs, line, .. } => Some((attrs, *line)),
        Node::Text(_) => None,
    }
}

fn has_attr(node: &Node<'_>, name: &str) -> bool {
    element_attrs(node).is_some_and(|(a, _)| a.iter().any(|x| x.name == name))
}

pub(super) fn emit_into(
    src: &str,
    nodes: &[Node<'_>],
    catalog: &Catalog,
    parent: Option<&str>,
    used: &mut Used,
    counter: &mut usize,
    out: &mut String,
) -> Result<(), CompileError> {
    let mut i = 0;
    while i < nodes.len() {
        let node = &nodes[i];
        let Node::Element {
            name, attrs, open, ..
        } = node
        else {
            if let Node::Text(span) = node {
                out.push_str(&src[span.clone()]);
            }
            i += 1;
            continue;
        };
        let flow = flow_of(src, tree::is_component(name), attrs, open)?;
        if name == "rx-let"
            && let Some(contract) = catalog.contracts.iter().find(|c| c.tag == "rx-let")
        {
            // The tag holds the rest of its siblings: that is its scope.
            let Node::Element {
                children,
                close,
                line,
                ..
            } = node
            else {
                unreachable!("matched as an element above")
            };
            if flow.cond.is_some()
                || flow.each.is_some()
                || flow.elif.is_some()
                || flow.otherwise
                || flow.can.is_some()
            {
                return Err(err(
                    *line,
                    "<rx-let> takes no rx-if, rx-elif, rx-else, rx-for or can".to_owned(),
                ));
            }
            let mut cx = Ctx {
                src,
                catalog,
                used,
                counter,
            };
            return special::let_tag(
                &mut cx,
                contract,
                &flow.attrs,
                children,
                open,
                close.as_ref(),
                *line,
                &nodes[i + 1..],
                parent,
                out,
            );
        }
        if flow.otherwise || flow.elif.is_some() {
            let word = if flow.otherwise { "rx-else" } else { "rx-elif" };
            let line = attrs.first().map_or(0, |a| a.line);
            let prev = nodes[..i]
                .iter()
                .rev()
                .find(|n| !matches!(n, Node::Text(s) if blank(&src[s.clone()])));
            let msg = if prev.is_some_and(|p| has_attr(p, "rx-for")) {
                format!("{word} can't follow an element with rx-for")
            } else {
                format!("{word} must follow an element with rx-if")
            };
            return Err(err(line, msg));
        }
        // An `rx-if` element followed by `rx-elif` elements and an `rx-else` one.
        let mut branches: Vec<usize> = Vec::new();
        if flow.cond.is_some() && flow.each.is_none() {
            let mut j = i + 1;
            loop {
                let mut k = j;
                if let Some(Node::Text(s)) = nodes.get(k)
                    && blank(&src[s.clone()])
                {
                    k += 1;
                }
                match nodes.get(k) {
                    Some(n) if has_attr(n, "rx-elif") => {
                        branches.push(k);
                        j = k + 1;
                    }
                    Some(n) if has_attr(n, "rx-else") => {
                        branches.push(k);
                        break;
                    }
                    _ => break,
                }
            }
        }
        let loop_vars = flow.each.as_deref().map(loop_names).unwrap_or_default();
        if let Some(each) = &flow.each {
            out.push_str(&format!("{{% for {each} %}}"));
        }
        let cond = flow.condition();
        if let Some(c) = &cond {
            out.push_str(&format!("{{% if {c} %}}"));
        }
        with_names(loop_vars, || {
            emit_node(src, node, &flow, catalog, parent, used, counter, out)
        })?;
        if branches.is_empty() {
            if cond.is_some() {
                out.push_str("{% endif %}");
            }
            if flow.each.is_some() {
                out.push_str("{% endfor %}");
            }
            i += 1;
            continue;
        }
        let mut last = i;
        for k in branches {
            for n in &nodes[last + 1..k] {
                if let Node::Text(s) = n {
                    out.push_str(&src[s.clone()]);
                }
            }
            let b = &nodes[k];
            let Node::Element {
                name, attrs, open, ..
            } = b
            else {
                unreachable!("has_attr is true only for elements")
            };
            let bf = flow_of(src, tree::is_component(name), attrs, open)?;
            let bc = bf.condition();
            if bf.elif.is_some() {
                out.push_str(&format!(
                    "{{% elif {} %}}",
                    bc.as_deref().expect("an elif has a condition")
                ));
                emit_node(src, b, &bf, catalog, parent, used, counter, out)?;
            } else {
                out.push_str("{% else %}");
                if let Some(each) = &bf.each {
                    out.push_str(&format!("{{% for {each} %}}"));
                }
                if let Some(c) = &bc {
                    out.push_str(&format!("{{% if {c} %}}"));
                }
                emit_node(src, b, &bf, catalog, parent, used, counter, out)?;
                if bc.is_some() {
                    out.push_str("{% endif %}");
                }
                if bf.each.is_some() {
                    out.push_str("{% endfor %}");
                }
            }
            last = k;
        }
        out.push_str("{% endif %}");
        i = last + 1;
    }
    Ok(())
}

/// One element: a component replaced, or a plain tag without its control-flow attributes.
#[allow(clippy::too_many_arguments)]
fn emit_node(
    src: &str,
    node: &Node<'_>,
    flow: &Flow<'_>,
    catalog: &Catalog,
    parent: Option<&str>,
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
    let attrs = &flow.attrs;
    if name == "template" {
        // `<template rx-if>`, `<template rx-else>`, `<template rx-for>`: the children, no wrapper.
        if let Some(a) = attrs.first() {
            return Err(err(
                a.line,
                format!(
                    "<template> with rx-if, rx-elif, rx-else or rx-for takes no other attribute, not \"{}\"",
                    bare(a.name)
                ),
            ));
        }
        out.push_str(&pad(&src[open.clone()]));
        emit_into(src, children, catalog, parent, used, counter, out)?;
        if let Some(c) = close {
            out.push_str(&pad(&src[c.clone()]));
        }
        return Ok(());
    }
    if !tree::is_component(name) {
        // The removed control-flow attributes, and each `?attr` replaced by its code.
        let mut edits: Vec<(std::ops::Range<usize>, String)> = flow
            .removed
            .iter()
            .map(|r| (r.clone(), String::new()))
            .collect();
        for a in attrs.iter().filter(|a| a.name.starts_with('?')) {
            let mut start = a.span.start;
            while start > open.start + 1 && src.as_bytes()[start - 1].is_ascii_whitespace() {
                start -= 1;
            }
            edits.push((start..a.span.end, cond_attr_code(a, name)?));
        }
        edits.sort_by_key(|(r, _)| r.start);
        let mut pos = open.start;
        let mut gone = String::new();
        for (r, code) in &edits {
            out.push_str(&src[pos..r.start]);
            out.push_str(code);
            gone.push_str(&src[r.clone()]);
            pos = r.end;
        }
        out.push_str(&src[pos..open.end]);
        out.push_str(&pad(&gone));
        emit_into(src, children, catalog, parent, used, counter, out)?;
        if let Some(c) = close {
            out.push_str(&src[c.clone()]);
        }
        return Ok(());
    }
    if name.starts_with("app-") {
        return super::app::usage(src, node, attrs, catalog, used, counter, out);
    }
    if name.starts_with("live-") {
        return super::live::usage(src, node, attrs, out);
    }
    let Some(contract) = catalog.contracts.iter().find(|c| c.tag == name) else {
        let hint = suggest::did_you_mean(name, catalog.contracts.iter().map(|c| c.tag))
            .map(|x| format!("; did you mean <{x}>?"))
            .unwrap_or_default();
        return Err(err(*line, format!("unknown component <{name}>{hint}")));
    };
    if let Some(p) = contract.parent
        && parent != Some(p)
    {
        return Err(err(*line, format!("<{name}> belongs inside <{p}>")));
    }
    if let Render::Special(sp) = contract.render {
        let mut cx = Ctx {
            src,
            catalog,
            used,
            counter,
        };
        let close = close.as_ref();
        return match sp {
            Special::Page => {
                if parent.is_some() {
                    return Err(err(
                        *line,
                        "<rx-page> must hold the whole template".to_owned(),
                    ));
                }
                special::page(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Push => {
                special::push(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Block => {
                special::block(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Outlet => {
                special::outlet(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Pagination => {
                special::pagination(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Script | Special::Style => {
                special::nonce_tag(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Wizard => {
                special::wizard(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::WizardAction => {
                special::wizard_action(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Mail => {
                if parent.is_some() {
                    return Err(err(
                        *line,
                        "<rx-mail> must hold the whole template".to_owned(),
                    ));
                }
                special::mail(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Include => {
                special::include(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::LinkTabs => {
                special::link_tabs(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::LinkTab => {
                special::link_tab(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::MailText => {
                if parent.is_some() {
                    return Err(err(
                        *line,
                        "<rx-mail-text> must hold the whole template".to_owned(),
                    ));
                }
                special::mail_text(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Tabs => {
                special::tabs(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Table => {
                special::table(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            Special::Form => {
                special::form(&mut cx, contract, attrs, children, open, close, *line, out)
            }
            _ => Err(err(*line, format!("<{name}> is not supported yet"))),
        };
    }
    let parts = split_slots(src, contract, children, *line)?;
    let filled: Vec<&str> = parts.iter().filter_map(|(slot, _)| slot.into).collect();
    for (slot, _) in &parts {
        if let Some(p) = slot.into
            && attrs.iter().any(|a| bare(a.name) == p)
        {
            return Err(err(
                *line,
                format!("<{name}> give \"{p}\" or content, not both"),
            ));
        }
    }
    check_attrs(contract, attrs, &filled).map_err(|m| err(*line, m))?;
    if parts.is_empty() && has_content(src, children) {
        return Err(err(*line, format!("<{name}> takes no content")));
    }
    match contract.render {
        Render::Element { tag, class } => {
            element(src, contract, tag, class, attrs, open, *line, out)?;
            emit_into(src, children, catalog, Some(name), used, counter, out)?;
            out.push_str(&format!("</{tag}>"));
            if let Some(c) = close {
                out.push_str(&pad(&src[c.clone()]));
            }
        }
        Render::Macro { module, name: mac } => {
            used.insert(Import {
                alias: module.alias().to_owned(),
                path: module.path().to_owned(),
            });
            // Each argument with its place: props as written, then slots, then `attrs`.
            let mut args: Vec<(usize, String)> = Vec::new();
            let extra = passthrough_pairs(contract, attrs)?;
            for (order, a) in attrs.iter().enumerate() {
                let n = bare(a.name);
                if n == "route" {
                    let e = attrs::prop_expr(a, Kind::Text, &[])
                        .map_err(|m| err(a.line, format!("<{}> {m}", contract.tag)))?;
                    let prop = contract.route_prop.expect("checked against the contract");
                    let at = contract
                        .props
                        .iter()
                        .position(|p| p.name == prop)
                        .unwrap_or(0);
                    let call = match row() {
                        Some((_, key)) => format!("route({e}, {key})"),
                        None => format!("route({e})"),
                    };
                    args.push((at, format!("{}={call}", attrs::snake(prop))));
                    continue;
                }
                let Some(p) = contract.props.iter().find(|p| p.name == n) else {
                    continue;
                };
                let e = attrs::prop_expr(a, p.kind, p.values)
                    .map_err(|m| err(a.line, format!("<{}> {m}", contract.tag)))?;
                args.push((order, format!("{}={e}", attrs::snake(n))));
            }
            let mut pre = String::new();
            let mut caller: Option<(&Slot, &[Node<'_>])> = None;
            let mut consumed_default = false;
            for (slot, nodes) in &parts {
                if slot.name.is_empty() && slot.into.is_none() {
                    caller = Some((slot, nodes));
                    continue;
                }
                *counter += 1;
                let n = *counter;
                let mut body = String::new();
                if slot.name.is_empty() {
                    consumed_default = true;
                }
                emit_into(src, nodes, catalog, Some(name), used, counter, &mut body)?;
                pre.push_str(&format!("{{% set __rx_slot_{n} %}}{body}{{% endset %}}"));
                let target = slot.into.unwrap_or(slot.name);
                args.push((
                    attrs.len() + n,
                    format!("{}=__rx_slot_{n}", attrs::snake(target)),
                ));
            }
            out.push_str(&pre);
            args.sort_by_key(|(at, _)| *at);
            let mut args: Vec<String> = args.into_iter().map(|(_, a)| a).collect();
            if !extra.is_empty() {
                args.push(format!("attrs={{{}}}", extra.join(", ")));
            }
            let call = format!("{}.{mac}({})", module.alias(), args.join(", "));
            let caller = caller.filter(|(slot, nodes)| !slot.optional || has_content(src, nodes));
            if let Some((slot, nodes)) = caller {
                let params = slot.args.join(", ");
                if slot.args.is_empty() {
                    out.push_str(&format!("{{% call {call} %}}"));
                } else {
                    out.push_str(&format!("{{% call({params}) {call} %}}"));
                }
                out.push_str(&pad(&src[open.clone()]));
                emit_into(src, nodes, catalog, Some(name), used, counter, out)?;
                out.push_str("{% endcall %}");
            } else {
                out.push_str(&format!("{{{{ {call} }}}}"));
                out.push_str(&pad(&src[open.clone()]));
                if !consumed_default {
                    for c in children {
                        if let Node::Text(span) = c {
                            out.push_str(&pad(&src[span.clone()]));
                        }
                    }
                }
            }
            if let Some(c) = close {
                out.push_str(&pad(&src[c.clone()]));
            }
        }
        Render::Function(function) => {
            let mut positional: Vec<(usize, String)> = Vec::new();
            let mut named: Vec<String> = Vec::new();
            for a in attrs {
                let n = bare(a.name);
                let Some((at, p)) = contract.props.iter().enumerate().find(|(_, p)| p.name == n)
                else {
                    return Err(err(a.line, format!("<{}> has no \"{n}\"", contract.tag)));
                };
                let e = attrs::prop_expr(a, p.kind, p.values)
                    .map_err(|m| err(a.line, format!("<{}> {m}", contract.tag)))?;
                if p.required {
                    positional.push((at, e));
                } else {
                    named.push(format!("{}={e}", attrs::snake(n)));
                }
            }
            positional.sort_by_key(|(at, _)| *at);
            let mut args: Vec<String> = positional.into_iter().map(|(_, e)| e).collect();
            args.extend(named);
            out.push_str(&format!("{{{{ {function}({}) }}}}", args.join(", ")));
            out.push_str(&pad(&src[open.clone()]));
            if let Some(c) = close {
                out.push_str(&pad(&src[c.clone()]));
            }
        }
        _ => {
            return Err(err(*line, format!("<{name}> is not supported yet")));
        }
    }
    Ok(())
}

/// Whether the children hold anything but whitespace.
pub(super) fn has_content(src: &str, children: &[Node<'_>]) -> bool {
    children.iter().any(|c| match c {
        Node::Text(s) => !src[s.clone()].trim().is_empty(),
        Node::Element { .. } => true,
    })
}

/// Writes the start tag of an `Element` component.
#[allow(clippy::too_many_arguments)]
fn element(
    src: &str,
    contract: &Contract,
    tag: &str,
    class: &str,
    attrs: &[Attr<'_>],
    open: &std::ops::Range<usize>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    let mut classes = class.to_owned();
    let mut extra: Vec<String> = Vec::new();
    let mut passthrough = String::new();
    for a in attrs {
        let n = bare(a.name);
        if let Some(p) = contract.props.iter().find(|p| p.name == n) {
            let e = attrs::prop_expr(a, p.kind, p.values)
                .map_err(|m| err(a.line, format!("<{}> {m}", contract.tag)))?;
            let modifier = format!("{class}--{n}");
            match (p.kind, e.as_str()) {
                (Kind::Bool, "true") => classes.push_str(&format!(" {modifier}")),
                (Kind::Bool, "false") => {}
                (Kind::Bool, _) => extra.push(format!("{{% if {e} %}} {modifier}{{% endif %}}")),
                _ => {}
            }
        } else if n == "class" {
            let value = a.value.unwrap_or("");
            if a.name.starts_with(':') {
                extra.push(format!(" {{{{ {} }}}}", value.trim()));
            } else if !value.is_empty() {
                extra.push(format!(" {value}"));
            }
        } else if a.name.starts_with('?') {
            passthrough.push_str(&cond_attr_code(a, contract.tag)?);
        } else if let Some(name) = a.name.strip_prefix(':') {
            let value = a.value.unwrap_or("").trim();
            if value.is_empty() {
                return Err(err(
                    a.line,
                    format!("<{}> \":{name}\" needs an expression", contract.tag),
                ));
            }
            passthrough.push_str(&format!(" {name}=\"{{{{ {value} }}}}\""));
        } else {
            passthrough.push(' ');
            passthrough.push_str(&src[a.span.clone()]);
        }
    }
    let _ = line;
    // Plain extra classes join the attribute value; conditional ones follow.
    for e in &extra {
        if e.starts_with(' ') {
            classes.push_str(e);
        }
    }
    out.push_str(&format!("<{tag} class=\"{classes}"));
    for e in extra.iter().filter(|e| !e.starts_with(' ')) {
        out.push_str(e);
    }
    out.push('"');
    out.push_str(&passthrough);
    out.push('>');
    out.push_str(&pad(&src[open.clone()]));
    Ok(())
}

/// The content for each slot: `<rx-slot name="x">` children for named slots, the rest for the
/// default slot. Slots without content are left out.
fn split_slots<'c, 'a>(
    src: &str,
    contract: &'c Contract,
    children: &[Node<'a>],
    line: usize,
) -> Result<Vec<(&'c Slot, Vec<Node<'a>>)>, CompileError> {
    let mut parts: Vec<(&Slot, Vec<Node<'a>>)> = Vec::new();
    let mut rest: Vec<Node<'a>> = Vec::new();
    for child in children {
        let Node::Element {
            name,
            attrs,
            children: inner,
            open,
            close,
            line: at,
        } = child
        else {
            rest.push(child.clone());
            continue;
        };
        if name != "rx-slot" {
            rest.push(child.clone());
            continue;
        }
        let want = attrs
            .iter()
            .find(|a| a.name == "name")
            .and_then(|a| a.value)
            .unwrap_or("");
        let tag = contract.tag;
        let named: Vec<&str> = contract
            .slots
            .iter()
            .map(|s| s.name)
            .filter(|n| !n.is_empty())
            .collect();
        if want.is_empty() {
            return Err(err(*at, "<rx-slot> needs a name here".to_owned()));
        }
        let Some(slot) = contract.slots.iter().find(|s| s.name == want) else {
            let hint = suggest::did_you_mean(want, named.iter().copied())
                .map(|y| format!("; did you mean \"{y}\"?"))
                .unwrap_or_else(|| ".".to_owned());
            return Err(err(*at, format!("<{tag}> has no slot \"{want}\"{hint}")));
        };
        let mut body: Vec<Node<'a>> = Vec::new();
        let _ = (open, close);
        body.extend(inner.iter().cloned());
        parts.push((slot, body));
    }
    if let Some(slot) = contract.slots.iter().find(|s| s.name.is_empty()) {
        if has_content(src, &rest) || (slot.into.is_none() && !slot.optional) {
            parts.push((slot, rest));
        }
    } else if has_content(src, &rest) {
        return Err(err(line, format!("<{}> takes no content", contract.tag)));
    }
    Ok(parts)
}
