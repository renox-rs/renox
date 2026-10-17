//! Components with code generation of their own: `rx-page` and `rx-push` (Decisions 16 and 17 of
//! #372).

use super::Catalog;
use super::CompileError;
use super::attrs;
use super::contracts::Contract;
use super::contracts::Module;
use super::emit::{
    blank, check_attrs, cond_attr_code, emit_into, err, pad, passthrough_pairs, with_row,
};
use super::scan::Attr;
use super::tree::Node;
use std::ops::Range;

/// What nested code generation needs.
pub(super) struct Ctx<'a, 'b> {
    pub src: &'a str,
    pub catalog: &'a Catalog<'a>,
    pub used: &'b mut super::emit::Used,
    pub counter: &'b mut usize,
}

impl Ctx<'_, '_> {
    fn nodes(
        &mut self,
        nodes: &[Node<'_>],
        parent: &str,
        out: &mut String,
    ) -> Result<(), CompileError> {
        emit_into(
            self.src,
            nodes,
            self.catalog,
            Some(parent),
            self.used,
            self.counter,
            out,
        )
    }
}

fn bare(name: &str) -> &str {
    name.strip_prefix(':').unwrap_or(name)
}

fn attr<'a, 'b>(attrs: &'b [Attr<'a>], name: &str) -> Option<&'b Attr<'a>> {
    attrs.iter().find(|a| bare(a.name) == name)
}

fn text_prop(a: &Attr<'_>, tag: &str) -> Result<String, CompileError> {
    attrs::prop_expr(a, attrs::Kind::Text, &[]).map_err(|m| err(a.line, format!("<{tag}> {m}")))
}

/// `<rx-page>`: `{% extends %}`, the `seo` block, the content block, then one block per slot.
#[allow(clippy::too_many_arguments)]
pub(super) fn page(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let layout = attr(attrs, "layout").expect("checked: required");
    let value = layout.value.unwrap_or("");
    if layout.name.starts_with(':') || value.contains("{{") || value.contains("{%") {
        return Err(err(
            layout.line,
            "<rx-page> \"layout\" must be a file name, not {{ }}".to_owned(),
        ));
    }
    // Split the children: `<rx-slot name="X">` become blocks, the rest is the content.
    let mut content: Vec<Node<'_>> = Vec::new();
    let mut slots: Vec<(&str, &[Node<'_>])> = Vec::new();
    for child in children {
        if let Node::Element {
            name,
            attrs: sa,
            children: inner,
            line: at,
            ..
        } = child
            && name == "rx-slot"
        {
            let want = sa
                .iter()
                .find(|a| a.name == "name")
                .and_then(|a| a.value)
                .unwrap_or("");
            if want.is_empty() {
                return Err(err(*at, "<rx-slot> needs a name here".to_owned()));
            }
            if want == "content" {
                return Err(err(
                    *at,
                    "<rx-page> \"content\" is the page itself, not a slot".to_owned(),
                ));
            }
            if slots.iter().any(|(n, _)| *n == want) {
                return Err(err(*at, format!("<rx-page> has the slot \"{want}\" twice")));
            }
            slots.push((want, inner));
        } else {
            content.push(child.clone());
        }
    }
    let title = attr(attrs, "title");
    let description = attr(attrs, "description");
    let seo_slot = slots.iter().position(|(n, _)| *n == "seo");
    if title.is_some() && seo_slot.is_some() {
        return Err(err(line, "give title or a seo slot, not both".to_owned()));
    }
    out.push_str(&format!("{{% extends {} %}}", attrs::literal(value.trim())));
    out.push_str(&pad(&cx.src[open.clone()]));
    if let Some(t) = title {
        let mut args = format!("title={}", text_prop(t, "rx-page")?);
        if let Some(d) = description {
            args.push_str(&format!(", description={}", text_prop(d, "rx-page")?));
        }
        out.push_str(&format!(
            "{{% block seo %}}{{{{ seo({args}) }}}}{{% endblock %}}"
        ));
    }
    out.push_str("{% block content %}");
    cx.nodes(&content, "rx-page", out)?;
    out.push_str("{% endblock %}");
    // Error lines inside a moved slot can be off by the lines of the content above: accepted
    // (Decision 16).
    for (name, body) in slots {
        out.push_str(&format!("{{% block {name} %}}"));
        cx.nodes(body, "rx-page", out)?;
        out.push_str("{% endblock %}");
    }
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-push stack="…" once="…">`: `{% call push("S", once="O") %}…{% endcall %}`.
#[allow(clippy::too_many_arguments)]
pub(super) fn push(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    if children
        .iter()
        .any(|c| matches!(c, Node::Element { name, .. } if name == "rx-slot"))
    {
        return Err(err(line, "<rx-push> has no named slots".to_owned()));
    }
    let stack = text_prop(attr(attrs, "stack").expect("checked: required"), "rx-push")?;
    let mut args = stack;
    if let Some(o) = attr(attrs, "once") {
        args.push_str(&format!(", once={}", text_prop(o, "rx-push")?));
    }
    out.push_str(&format!("{{% call push({args}) %}}"));
    out.push_str(&pad(&cx.src[open.clone()]));
    cx.nodes(children, "rx-push", out)?;
    out.push_str("{% endcall %}");
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-block name="X">…</rx-block>`: `{% block X %}…{% endblock %}`; self-closing, an empty block.
#[allow(clippy::too_many_arguments)]
pub(super) fn block(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let a = attr(attrs, "name").expect("checked: required");
    named_block(cx, "rx-block", a, children, open, close, line, out)
}

/// `<rx-outlet name="X">` is a block (`{% block X %}`, the children its default content);
/// `<rx-outlet stack="X"/>` is `{{ stack("X") }}`. Exactly one of the two.
#[allow(clippy::too_many_arguments)]
pub(super) fn outlet(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    match (attr(attrs, "name"), attr(attrs, "stack")) {
        (Some(a), None) => named_block(cx, "rx-outlet", a, children, open, close, line, out),
        (None, Some(a)) => {
            if super::emit::has_content(cx.src, children) {
                return Err(err(
                    line,
                    "<rx-outlet stack=\"…\"> takes no content".to_owned(),
                ));
            }
            let stack = text_prop(a, "rx-outlet")?;
            out.push_str(&format!("{{{{ stack({stack}) }}}}"));
            out.push_str(&pad(&cx.src[open.clone()]));
            if let Some(c) = close {
                out.push_str(&pad(&cx.src[c.clone()]));
            }
            Ok(())
        }
        _ => Err(err(
            line,
            "<rx-outlet> needs exactly one of the attributes \"name\" (a layout block) or \"stack\" (a stack)"
                .to_owned(),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn named_block(
    cx: &mut Ctx<'_, '_>,
    tag: &str,
    a: &Attr<'_>,
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    let name = a.value.unwrap_or("");
    if a.name.starts_with(':') || name.contains("{{") || name.contains("{%") {
        return Err(err(a.line, format!("<{tag}> \"name\" must be plain text")));
    }
    let valid = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid {
        return Err(err(
            a.line,
            format!("<{tag}> \"name\" must be letters, digits and underscores, not \"{name}\""),
        ));
    }
    if children
        .iter()
        .any(|c| matches!(c, Node::Element { name, .. } if name == "rx-slot"))
    {
        return Err(err(line, format!("<{tag}> has no named slots")));
    }
    out.push_str(&format!("{{% block {name} %}}"));
    out.push_str(&pad(&cx.src[open.clone()]));
    cx.nodes(children, tag, out)?;
    out.push_str("{% endblock %}");
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-pagination :page="p"/>`: the numbered links for a `Paginated`, previous / next for a
/// `SimplePage` (a value with `has_next` and no `last_page`), like `<rx-table>`.
#[allow(clippy::too_many_arguments)]
pub(super) fn pagination(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    if super::emit::has_content(cx.src, children) {
        return Err(err(line, "<rx-pagination> takes no content".to_owned()));
    }
    let a = attr(attrs, "page").expect("checked: required");
    let page = attrs::prop_expr(a, attrs::Kind::Data, &[])
        .map_err(|m| err(a.line, format!("<rx-pagination> {m}")))?;
    cx.used.insert(super::emit::Import {
        alias: Module::Pagination.alias().to_owned(),
        path: Module::Pagination.path().to_owned(),
    });
    *cx.counter += 1;
    let n = *cx.counter;
    let alias = Module::Pagination.alias();
    out.push_str(&format!(
        "{{% set __rx_page_{n} = {page} %}}{{% if __rx_page_{n}.has_next is defined and __rx_page_{n}.last_page is not defined %}}{{{{ {alias}.simple_pagination(__rx_page_{n}) }}}}{{% else %}}{{{{ {alias}.pagination(__rx_page_{n}) }}}}{{% endif %}}"
    ));
    out.push_str(&pad(&cx.src[open.clone()]));
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// Names `<rx-let>` may not take: the template's own words and the functions and globals every
/// page has.
const RESERVED: &[&str] = &[
    "loop",
    "caller",
    "self",
    "super",
    "true",
    "false",
    "none",
    "True",
    "False",
    "None",
    "route",
    "asset",
    "storage_url",
    "stack",
    "push",
    "prepend",
    "page_url",
    "method_field",
    "renox_ui",
    "renox_theme",
    "renox_calendar",
    "renox_live",
    "renox_offline",
    "renox_grid",
    "renox_icon",
    "sparkline",
    "chart",
    "class_names",
    "query_fields",
    "query_with",
    "number",
    "styles",
];

/// The texts the outbox script reads from an offline form (attribute suffix, `ui.offline.*` key).
const OFFLINE_TEXTS: [(&str, &str); 11] = [
    ("waiting", "waiting"),
    ("attention", "attention"),
    ("saved", "saved"),
    ("sent", "sent"),
    ("sending", "sending"),
    ("signin", "sign_in"),
    ("failed", "failed"),
    ("review", "review"),
    ("resend", "resend"),
    ("discard", "discard"),
    ("problem", "problem"),
];

/// Splits the siblings after an `<rx-let>` at the end tag of the plain HTML element that holds
/// it (plain elements are not nodes of the tree, only text): what comes before that tag is the
/// let's scope, the rest (starting with the tag) is outside it.
fn split_scope<'a>(src: &str, rest: &[Node<'a>]) -> (Vec<Node<'a>>, Vec<Node<'a>>) {
    use super::scan::{Token, scan};
    let mut depth = 0usize;
    for (i, node) in rest.iter().enumerate() {
        let Node::Text(span) = node else { continue };
        let Ok(tokens) = scan(&src[span.clone()]) else {
            continue;
        };
        for token in tokens {
            match token {
                Token::Open {
                    name, self_closing, ..
                } if !self_closing && !super::tree::VOID.contains(&name.as_str()) => depth += 1,
                Token::Close { span: tag, .. } => {
                    if depth == 0 {
                        let at = span.start + tag.start;
                        let mut inside = rest[..i].to_vec();
                        if at > span.start {
                            inside.push(Node::Text(span.start..at));
                        }
                        let mut after = vec![Node::Text(at..span.end)];
                        after.extend(rest[i + 1..].iter().cloned());
                        return (inside, after);
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    (rest.to_vec(), Vec::new())
}

/// `<rx-let name="x" :value="expr"/>`: `{% with x = (expr) %}` around the rest of the siblings, so
/// the name ends with the enclosing element, slot or block (#511). `{% set %}` would not do: an
/// `{% if %}` does not end its scope, so the name would leak out of it.
#[allow(clippy::too_many_arguments)]
pub(super) fn let_tag(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    rest: &[Node<'_>],
    parent: Option<&str>,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    if super::emit::has_content(cx.src, children) {
        return Err(err(line, "<rx-let> takes no content".to_owned()));
    }
    let a = attr(attrs, "name").expect("checked: required");
    let name = a.value.unwrap_or("").trim();
    if a.name.starts_with(':') || name.contains("{{") || name.contains("{%") {
        return Err(err(
            a.line,
            "<rx-let> \"name\" must be plain text".to_owned(),
        ));
    }
    if !plain_name(name) || name.starts_with("__rx_") {
        return Err(err(
            a.line,
            format!("<rx-let> \"name\" must be letters, digits and underscores, not \"{name}\""),
        ));
    }
    if RESERVED.contains(&name) || crate::view::REQUEST_GLOBALS.contains(&name) {
        return Err(err(
            a.line,
            format!("<rx-let> \"{name}\" is a name every page already has; pick another"),
        ));
    }
    if super::emit::name_bound(name) {
        return Err(err(
            a.line,
            format!(
                "<rx-let> \"{name}\" is already taken here (by a loop, a table row or another rx-let); pick another"
            ),
        ));
    }
    let v = attr(attrs, "value").expect("checked: required");
    let value = attrs::prop_expr(v, attrs::Kind::Text, &[])
        .map_err(|m| err(v.line, format!("<rx-let> {m}")))?;
    out.push_str(&format!("{{% with {name} = {value} %}}"));
    out.push_str(&pad(&cx.src[open.clone()]));
    let (inside, after) = split_scope(cx.src, rest);
    super::emit::with_names(vec![name.to_owned()], || {
        emit_into(
            cx.src, &inside, cx.catalog, parent, cx.used, cx.counter, out,
        )
    })?;
    out.push_str("{% endwith %}");
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    emit_into(cx.src, &after, cx.catalog, parent, cx.used, cx.counter, out)
}

/// `<rx-script>` and `<rx-style>`: the same tag with `nonce="{{ csp_nonce() }}"` first (#512).
/// Other attributes are kept as written, `:name` ones become `name="{{ … }}"`.
#[allow(clippy::too_many_arguments)]
pub(super) fn nonce_tag(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let tag = contract.tag.strip_prefix("rx-").unwrap_or(contract.tag);
    if let Some(n) = attrs.iter().find(|a| bare(a.name) == "nonce") {
        return Err(err(
            n.line,
            format!("<{}> sets the nonce itself; remove \"nonce\"", contract.tag),
        ));
    }
    out.push_str(&format!("<{tag} nonce=\"{{{{ csp_nonce() }}}}\""));
    for a in attrs {
        if a.name.starts_with('?') {
            out.push_str(&cond_attr_code(a, contract.tag)?);
        } else if let Some(name) = a.name.strip_prefix(':') {
            let value = a.value.unwrap_or("").trim();
            if value.is_empty() {
                return Err(err(
                    a.line,
                    format!("<{}> \":{name}\" needs an expression", contract.tag),
                ));
            }
            out.push_str(&format!(
                " {name}=\"{{{{ {} }}}}\"",
                value.replace(['\n', '\r'], " ")
            ));
        } else {
            out.push(' ');
            out.push_str(&cx.src[a.span.clone()]);
        }
    }
    out.push('>');
    out.push_str(&pad(&cx.src[open.clone()]));
    cx.nodes(children, contract.tag, out)?;
    out.push_str(&format!("</{tag}>"));
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// A wizard step read from its tag: key, title, content, start tag, end tag.
type Step<'a, 'b> = (
    String,
    Option<String>,
    &'b [Node<'a>],
    &'b Range<usize>,
    Option<&'b Range<usize>>,
);

/// The `<rx-wizard-step>` children of a wizard (or wizard action) as steps, and the blank text
/// between them as line-keeping comments.
#[allow(clippy::type_complexity)]
fn wizard_steps<'a, 'b>(
    cx: &Ctx<'_, '_>,
    children: &'b [Node<'a>],
    line: usize,
    owner: &str,
) -> Result<(Vec<Step<'a, 'b>>, String), CompileError> {
    let holds = format!("<{owner}> holds only <rx-wizard-step>");
    let step_contract = cx
        .catalog
        .contracts
        .iter()
        .find(|c| c.tag == "rx-wizard-step")
        .expect("rx-wizard-step is built in");
    let mut steps: Vec<Step<'_, '_>> = Vec::new();
    let mut gaps = String::new();
    for child in children {
        match child {
            Node::Text(span) => {
                let text = &cx.src[span.clone()];
                if !text.trim().is_empty() {
                    return Err(err(line, holds));
                }
                gaps.push_str(&pad(text));
            }
            Node::Element {
                name,
                attrs: sa,
                children: inner,
                open: sopen,
                close: sclose,
                line: at,
            } => {
                if name != "rx-wizard-step" {
                    return Err(err(*at, holds));
                }
                check_attrs(step_contract, sa, &[]).map_err(|m| err(*at, m))?;
                let key = text_prop(attr(sa, "key").expect("checked: required"), name)?;
                let title = attr(sa, "title").map(|t| text_prop(t, name)).transpose()?;
                steps.push((key, title, inner, sopen, sclose.as_ref()));
            }
        }
    }
    Ok((steps, gaps))
}

/// The `wizard_step` calls for `steps`, built on the wizard's or sheet's `id`.
fn wizard_step_calls(
    cx: &mut Ctx<'_, '_>,
    id: &str,
    steps: Vec<Step<'_, '_>>,
    out: &mut String,
) -> Result<(), CompileError> {
    for (key, title, inner, sopen, sclose) in steps {
        let mut call = format!("id={id}, key={key}");
        if let Some(t) = title {
            call.push_str(&format!(", title={t}"));
        }
        out.push_str(&format!("{{% call __rx_ui.wizard_step({call}) %}}"));
        out.push_str(&pad(&cx.src[sopen.clone()]));
        cx.nodes(inner, "rx-wizard-step", out)?;
        out.push_str("{% endcall %}");
        if let Some(c) = sclose {
            out.push_str(&pad(&cx.src[c.clone()]));
        }
    }
    Ok(())
}

/// `<rx-wizard>`: the steps come from the `<rx-wizard-step>` children, and each gets the
/// wizard's id (Decision 3 of #374).
#[allow(clippy::too_many_arguments)]
pub(super) fn wizard(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let (steps, gaps) = wizard_steps(cx, children, line, "rx-wizard")?;
    cx.used.insert(super::emit::Import {
        alias: Module::Ui.alias().to_owned(),
        path: Module::Ui.path().to_owned(),
    });
    let id = text_prop(attr(attrs, "id").expect("checked: required"), "rx-wizard")?;
    let list: Vec<String> = steps
        .iter()
        .map(|(k, t, ..)| format!("[{k}, {}]", t.as_deref().unwrap_or(k)))
        .collect();
    let mut args = format!(
        "id={id}, steps=[{}], submit_label={}",
        list.join(", "),
        text_prop(
            attr(attrs, "submit-label").expect("checked: required"),
            "rx-wizard"
        )?
    );
    for (name, arg) in [
        ("back-label", "back_label"),
        ("next-label", "next_label"),
        ("cancel-label", "cancel_label"),
    ] {
        if let Some(a) = attr(attrs, name) {
            args.push_str(&format!(", {arg}={}", text_prop(a, "rx-wizard")?));
        }
    }
    if let Some(a) = attr(attrs, "cancel") {
        let e = attrs::prop_expr(a, attrs::Kind::Bool, &[])
            .map_err(|m| err(a.line, format!("<rx-wizard> {m}")))?;
        args.push_str(&format!(", cancel={e}"));
    }
    let extra = passthrough_pairs(contract, attrs)?;
    if !extra.is_empty() {
        args.push_str(&format!(", attrs={{{}}}", extra.join(", ")));
    }
    out.push_str(&format!("{{% call __rx_ui.wizard({args}) %}}"));
    out.push_str(&pad(&cx.src[open.clone()]));
    out.push_str(&gaps);
    wizard_step_calls(cx, &id, steps, out)?;
    out.push_str("{% endcall %}");
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-tabs>`: the tab list comes from the `<rx-tab>` children, and each child's content
/// becomes its panel, built on the tabs' id (Decision 3 of #375).
#[allow(clippy::too_many_arguments)]
pub(super) fn tabs(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    const HOLDS: &str = "<rx-tabs> holds only <rx-tab>";
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    if let Some(items) = attr(attrs, "items") {
        return tabs_from_data(cx, attrs, items, children, open, close, out);
    }
    let tab_contract = cx
        .catalog
        .contracts
        .iter()
        .find(|c| c.tag == "rx-tab")
        .expect("rx-tab is built in");
    let mut tabs: Vec<Step<'_, '_>> = Vec::new();
    let mut gaps = String::new();
    for child in children {
        match child {
            Node::Text(span) => {
                let text = &cx.src[span.clone()];
                if !text.trim().is_empty() {
                    return Err(err(line, HOLDS.to_owned()));
                }
                gaps.push_str(&pad(text));
            }
            Node::Element {
                name,
                attrs: ta,
                children: inner,
                open: topen,
                close: tclose,
                line: at,
            } => {
                if name != "rx-tab" {
                    return Err(err(*at, HOLDS.to_owned()));
                }
                check_attrs(tab_contract, ta, &[]).map_err(|m| err(*at, m))?;
                let key = text_prop(attr(ta, "key").expect("checked: required"), name)?;
                let label = text_prop(attr(ta, "label").expect("checked: required"), name)?;
                tabs.push((key, Some(label), inner, topen, tclose.as_ref()));
            }
        }
    }
    if tabs.is_empty() {
        return Err(err(
            line,
            "<rx-tabs> needs at least one <rx-tab>".to_owned(),
        ));
    }
    cx.used.insert(super::emit::Import {
        alias: Module::Ui.alias().to_owned(),
        path: Module::Ui.path().to_owned(),
    });
    let id = text_prop(attr(attrs, "id").expect("checked: required"), "rx-tabs")?;
    let selected = attr(attrs, "selected")
        .map(|a| text_prop(a, "rx-tabs"))
        .transpose()?;
    let current = selected.clone().unwrap_or_else(|| tabs[0].0.clone());
    let list: Vec<String> = tabs
        .iter()
        .map(|(k, l, ..)| format!("[{k}, {}]", l.as_deref().expect("checked: required")))
        .collect();
    let mut args = format!("id={id}, items=[{}]", list.join(", "));
    if let Some(s) = &selected {
        args.push_str(&format!(", selected={s}"));
    }
    if let Some(a) = attr(attrs, "label") {
        args.push_str(&format!(", label={}", text_prop(a, "rx-tabs")?));
    }
    let extra = passthrough_pairs(contract, attrs)?;
    if !extra.is_empty() {
        args.push_str(&format!(", attrs={{{}}}", extra.join(", ")));
    }
    out.push_str(&format!("{{{{ __rx_ui.tabs({args}) }}}}"));
    out.push_str(&pad(&cx.src[open.clone()]));
    out.push_str(&gaps);
    for (key, _, inner, topen, tclose) in tabs {
        out.push_str(&format!(
            "{{% call __rx_ui.tab_panel(id={id}, key={key}, selected=({key} == {current})) %}}"
        ));
        out.push_str(&pad(&cx.src[topen.clone()]));
        cx.nodes(inner, "rx-tab", out)?;
        out.push_str("{% endcall %}");
        if let Some(c) = tclose {
            out.push_str(&pad(&cx.src[c.clone()]));
        }
    }
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-tabs :items="…">`: the tab list from data, `[key, label]` pairs; the children are the
/// panels (`<rx-tab-panel>`, also under `rx-for`) and follow the list as written.
fn tabs_from_data(
    cx: &mut Ctx<'_, '_>,
    attrs: &[Attr<'_>],
    items: &Attr<'_>,
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    out: &mut String,
) -> Result<(), CompileError> {
    if let Some(Node::Element { line, .. }) = children
        .iter()
        .find(|c| matches!(c, Node::Element { name, .. } if name == "rx-tab"))
    {
        return Err(err(
            *line,
            "<rx-tabs> takes :items or <rx-tab> children, not both".to_owned(),
        ));
    }
    cx.used.insert(super::emit::Import {
        alias: Module::Ui.alias().to_owned(),
        path: Module::Ui.path().to_owned(),
    });
    let items = attrs::prop_expr(items, attrs::Kind::Data, &[])
        .map_err(|m| err(items.line, format!("<rx-tabs> {m}")))?;
    let id = text_prop(attr(attrs, "id").expect("checked: required"), "rx-tabs")?;
    let mut args = format!("id={id}, items={items}");
    for name in ["selected", "label"] {
        if let Some(a) = attr(attrs, name) {
            args.push_str(&format!(", {name}={}", text_prop(a, "rx-tabs")?));
        }
    }
    let extra = passthrough_pairs(
        cx.catalog
            .contracts
            .iter()
            .find(|c| c.tag == "rx-tabs")
            .expect("rx-tabs is built in"),
        attrs,
    )?;
    if !extra.is_empty() {
        args.push_str(&format!(", attrs={{{}}}", extra.join(", ")));
    }
    out.push_str(&format!("{{{{ __rx_ui.tabs({args}) }}}}"));
    out.push_str(&pad(&cx.src[open.clone()]));
    cx.nodes(children, "rx-tabs", out)?;
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

const TABLE_HOLDS: &str = "<rx-table> holds <rx-column>, <rx-row-actions>, <rx-slot name=\"empty\"> and <rx-slot name=\"footer\">";

fn plain_name(v: &str) -> bool {
    let mut chars = v.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `<rx-table>`: the columns become a `table(...)` call, a loop over the rows and, for a page of
/// rows, its pagination (Decision 18 of #372).
#[allow(clippy::too_many_arguments)]
pub(super) fn table(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let find = |tag: &str| {
        cx.catalog
            .contracts
            .iter()
            .find(|c| c.tag == tag)
            .copied()
            .unwrap_or_else(|| panic!("{tag} is built in"))
    };
    let (column, actions) = (find("rx-column"), find("rx-row-actions"));
    let data = |name: &str| -> Result<Option<String>, CompileError> {
        attr(attrs, name)
            .map(|a| {
                attrs::prop_expr(a, attrs::Kind::Data, &[])
                    .map_err(|m| err(a.line, format!("<rx-table> {m}")))
            })
            .transpose()
    };
    let rows = data("rows")?.expect("checked: required");
    let var = match attr(attrs, "as") {
        Some(a) => {
            let v = a.value.unwrap_or("").trim();
            if a.name.starts_with(':') || !plain_name(v) {
                return Err(err(
                    a.line,
                    "<rx-table> \"as\" must be a plain name, such as product".to_owned(),
                ));
            }
            v.to_owned()
        }
        None => "row".to_owned(),
    };
    let key = data("key")?.unwrap_or_else(|| format!("{var}.id"));
    let caption = attr(attrs, "caption")
        .map(|a| text_prop(a, "rx-table"))
        .transpose()?;
    for m in [Module::Ui, Module::Pagination] {
        cx.used.insert(super::emit::Import {
            alias: m.alias().to_owned(),
            path: m.path().to_owned(),
        });
    }
    *cx.counter += 1;
    let n = *cx.counter;
    let id = match attr(attrs, "id") {
        Some(a)
            if !a.name.starts_with(':')
                && a.value.is_some_and(|v| {
                    !v.is_empty()
                        && v.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "_-.:".contains(c))
                }) =>
        {
            a.value.unwrap_or("").to_owned()
        }
        Some(a) => format!("{{{{ {} }}}}", text_prop(a, "rx-table")?),
        None => format!("rx-table-{n}"),
    };
    let card = match attr(attrs, "card") {
        Some(a) => Some(
            attrs::prop_expr(a, attrs::Kind::Bool, &[])
                .map_err(|m| err(a.line, format!("<rx-table> {m}")))?,
        ),
        None => None,
    };

    let mut head: Vec<String> = Vec::new();
    let mut cells = String::new();
    let mut empty = String::new();
    let mut seen_empty = false;
    let mut footer = String::new();
    let mut seen_footer = false;
    for child in children {
        match child {
            Node::Text(span) => {
                let text = &cx.src[span.clone()];
                if !blank(text) {
                    return Err(err(line, TABLE_HOLDS.to_owned()));
                }
                cells.push_str(&pad(text));
            }
            Node::Element {
                name,
                attrs: ca,
                children: inner,
                open: copen,
                close: cclose,
                line: at,
            } => match name.as_str() {
                "rx-column" => {
                    check_attrs(&column, ca, &[]).map_err(|m| err(*at, m))?;
                    let label = text_prop(attr(ca, "label").expect("checked: required"), name)?;
                    let (mut num, mut narrow) = (false, false);
                    if let Some(a) = attr(ca, "align") {
                        match (a.name.starts_with(':'), a.value) {
                            (false, Some("num")) => num = true,
                            (false, Some("start")) => {}
                            _ => {
                                return Err(err(
                                    a.line,
                                    "<rx-column> \"align\" is start or num".to_owned(),
                                ));
                            }
                        }
                    }
                    if let Some(a) = attr(ca, "hide-narrow") {
                        match (a.name.starts_with(':'), a.value) {
                            (false, None | Some("true")) => narrow = true,
                            (false, Some("false")) => {}
                            _ => {
                                return Err(err(
                                    a.line,
                                    "<rx-column> \"hide-narrow\" is true or false: write hide-narrow"
                                        .to_owned(),
                                ));
                            }
                        }
                    }
                    let td = match (num, narrow) {
                        (false, false) => {
                            head.push(label);
                            "<td>"
                        }
                        (true, false) => {
                            head.push(format!("[{label}, \"num\"]"));
                            "<td class=\"rx-num\">"
                        }
                        (false, true) => {
                            head.push(format!("[{label}, \"hide-narrow\"]"));
                            "<td class=\"rx-hide-narrow\">"
                        }
                        (true, true) => {
                            head.push(format!("[{label}, \"num rx-hide-narrow\"]"));
                            "<td class=\"rx-num rx-hide-narrow\">"
                        }
                    };
                    cells.push_str(td);
                    cells.push_str(&pad(&cx.src[copen.clone()]));
                    with_row(&var, &key, || cx.nodes(inner, "rx-column", &mut cells))?;
                    cells.push_str("</td>");
                    if let Some(c) = cclose {
                        cells.push_str(&pad(&cx.src[c.clone()]));
                    }
                }
                "rx-row-actions" => {
                    check_attrs(&actions, ca, &[]).map_err(|m| err(*at, m))?;
                    head.push("[\"\", \"num\"]".to_owned());
                    cells.push_str("<td class=\"rx-num\">{% call __rx_ui.row_actions() %}");
                    cells.push_str(&pad(&cx.src[copen.clone()]));
                    with_row(&var, &key, || cx.nodes(inner, "rx-row-actions", &mut cells))?;
                    cells.push_str("{% endcall %}</td>");
                    if let Some(c) = cclose {
                        cells.push_str(&pad(&cx.src[c.clone()]));
                    }
                }
                "rx-slot" => {
                    let want = ca
                        .iter()
                        .find(|a| a.name == "name")
                        .and_then(|a| a.value)
                        .unwrap_or("");
                    if !matches!(want, "empty" | "footer") {
                        let hint = super::suggest::did_you_mean(want, ["empty", "footer"])
                            .map(|x| format!("; did you mean \"{x}\"?"))
                            .unwrap_or_default();
                        return Err(err(*at, format!("{TABLE_HOLDS}{hint}")));
                    }
                    let (seen, target) = if want == "empty" {
                        (&mut seen_empty, &mut empty)
                    } else {
                        (&mut seen_footer, &mut footer)
                    };
                    if *seen {
                        return Err(err(
                            *at,
                            format!("<rx-table> has the slot \"{want}\" twice"),
                        ));
                    }
                    *seen = true;
                    target.push_str(&pad(&cx.src[copen.clone()]));
                    cx.nodes(inner, "rx-table", target)?;
                    if let Some(c) = cclose {
                        target.push_str(&pad(&cx.src[c.clone()]));
                    }
                }
                _ => return Err(err(*at, TABLE_HOLDS.to_owned())),
            },
        }
    }
    if head.is_empty() {
        return Err(err(
            line,
            "<rx-table> needs at least one <rx-column>".to_owned(),
        ));
    }

    let r = &rows;
    let hx = format!(
        "<div hx-boost=\"true\" hx-target=\"#{id}\" hx-select=\"#{id}\" hx-swap=\"outerHTML\">"
    );
    out.push_str(&format!(
        "{{% set __rx_rows_{n} = ({r}.items if {r}.items is defined else {r}) %}}<div class=\"rx-stack\" id=\"{id}\">{{% if __rx_rows_{n} %}}"
    ));
    let card_open = match &card {
        None => String::new(),
        Some(c) if c == "true" => "<div class=\"rx-card\">".to_owned(),
        Some(c) if c == "false" => String::new(),
        Some(c) => format!("{{% if {c} %}}<div class=\"rx-card\">{{% endif %}}"),
    };
    let card_close = match &card {
        None => String::new(),
        Some(c) if c == "true" => "</div>".to_owned(),
        Some(c) if c == "false" => String::new(),
        Some(c) => format!("{{% if {c} %}}</div>{{% endif %}}"),
    };
    out.push_str(&card_open);
    let mut args = format!("head=[{}]", head.join(", "));
    if let Some(c) = caption {
        args.push_str(&format!(", caption={c}"));
    }
    if seen_footer {
        out.push_str(&format!("{{% set __rx_foot_{n} %}}{footer}{{% endset %}}"));
        args.push_str(&format!(", foot=__rx_foot_{n}"));
    }
    out.push_str(&format!("{{% call __rx_ui.table({args}) %}}"));
    out.push_str(&pad(&cx.src[open.clone()]));
    out.push_str(&format!("{{% for {var} in __rx_rows_{n} %}}<tr>"));
    out.push_str(&cells);
    out.push_str("</tr>{% endfor %}{% endcall %}");
    out.push_str(&card_close);
    out.push_str(&format!(
        "{{% if {r}.last_page is defined %}}{hx}{{{{ __rx_pagination.pagination({r}) }}}}</div>{{% elif {r}.has_next is defined %}}{hx}{{{{ __rx_pagination.simple_pagination({r}) }}}}</div>{{% endif %}}{{% else %}}"
    ));
    out.push_str(&empty);
    out.push_str("{% endif %}</div>");
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-form>`: a `<form>` with the CSRF field (not for GET) and the method field (PUT, PATCH,
/// DELETE). Every other attribute passes through as written (Decision 17 of #372).
#[allow(clippy::too_many_arguments)]
pub(super) fn form(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let methods = ["GET", "POST", "PUT", "PATCH", "DELETE"];
    let mut method = "POST".to_owned();
    if let Some(m) = attr(attrs, "method") {
        let v = m.value.unwrap_or("");
        if m.name.starts_with(':') || v.contains("{{") || v.contains("{%") {
            return Err(err(
                m.line,
                "<rx-form> \"method\" must be plain text".to_owned(),
            ));
        }
        attrs::prop_expr(m, attrs::Kind::Enum, &methods)
            .map_err(|e| err(m.line, format!("<rx-form> {e}")))?;
        method = v.to_owned();
    }
    let action = if let Some(r) = attr(attrs, "route") {
        Some(format!("route({})", text_prop(r, "rx-form")?))
    } else {
        attr(attrs, "action")
            .map(|a| text_prop(a, "rx-form"))
            .transpose()?
    };
    let live = match attr(attrs, "live") {
        Some(a) => Some(
            attrs::prop_expr(a, attrs::Kind::Bool, &[])
                .map_err(|m| err(a.line, format!("<rx-form> {m}")))?,
        ),
        None => None,
    };
    let offline = attr(attrs, "offline")
        .map(|a| text_prop(a, "rx-form"))
        .transpose()?;
    let get = method == "GET";
    if offline.is_some() && get {
        return Err(err(
            line,
            "<rx-form> \"offline\" needs a method that writes, not GET".to_owned(),
        ));
    }
    out.push_str(&format!(
        "<form method=\"{}\"",
        if get { "get" } else { "post" }
    ));
    if let Some(a) = action {
        out.push_str(&format!(" action=\"{{{{ {a} }}}}\""));
    }
    if let Some(name) = &offline {
        out.push_str(&format!(
            " data-rx-offline=\"{{{{ {name} }}}}\" data-rx-offline-csrf=\"/_renox/csrf\""
        ));
        for (attr, key) in OFFLINE_TEXTS {
            out.push_str(&format!(
                " data-rx-offline-{attr}=\"{{{{ t('ui.offline.{key}') }}}}\""
            ));
        }
    }
    match live.as_deref() {
        None | Some("false") => {}
        Some("true") => out.push_str(" data-live-validate novalidate"),
        Some(e) => out.push_str(&format!(
            "{{% if {e} %}} data-live-validate novalidate{{% endif %}}"
        )),
    }
    for a in attrs {
        let n = bare(a.name);
        if matches!(n, "action" | "route" | "method" | "live" | "offline") {
            continue;
        }
        if a.name.starts_with('?') {
            out.push_str(&cond_attr_code(a, "rx-form")?);
        } else if let Some(name) = a.name.strip_prefix(':') {
            let value = a.value.unwrap_or("").trim();
            if value.is_empty() {
                return Err(err(
                    a.line,
                    format!("<rx-form> \":{name}\" needs an expression"),
                ));
            }
            out.push_str(&format!(
                " {name}=\"{{{{ {} }}}}\"",
                value.replace(['\n', '\r'], " ")
            ));
        } else {
            out.push(' ');
            out.push_str(&cx.src[a.span.clone()]);
        }
    }
    out.push('>');
    out.push_str(&pad(&cx.src[open.clone()]));
    if !get {
        out.push_str("{{ csrf_field() }}");
    }
    if matches!(method.as_str(), "PUT" | "PATCH" | "DELETE") {
        out.push_str(&format!("{{{{ method_field(\"{method}\") }}}}"));
    }
    if offline.is_some() {
        out.push_str("{% if once('rx-offline') %}{{ renox_offline() }}{% endif %}");
    }
    cx.nodes(children, "rx-form", out)?;
    out.push_str("</form>");
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-mail>`: `{% extends %}` of the mail layout, the `title` block from `subject`, the
/// content block, and a `footer` block from `<rx-slot name="footer">`.
#[allow(clippy::too_many_arguments)]
pub(super) fn mail(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let layout = match attr(attrs, "layout") {
        Some(a) => {
            let value = a.value.unwrap_or("").trim();
            if a.name.starts_with(':') || value.is_empty() || value.contains("{{") {
                return Err(err(
                    a.line,
                    "<rx-mail> \"layout\" must be a file name, not {{ }}".to_owned(),
                ));
            }
            value.to_owned()
        }
        None => "renox/mail/layout.html".to_owned(),
    };
    let mut content: Vec<Node<'_>> = Vec::new();
    let mut footer: Option<&[Node<'_>]> = None;
    for child in children {
        if let Node::Element {
            name,
            attrs: sa,
            children: inner,
            line: at,
            ..
        } = child
            && name == "rx-slot"
        {
            let want = sa
                .iter()
                .find(|a| a.name == "name")
                .and_then(|a| a.value)
                .unwrap_or("");
            if want != "footer" {
                let hint = super::suggest::did_you_mean(want, ["footer"])
                    .map(|x| format!("; did you mean \"{x}\"?"))
                    .unwrap_or_else(|| ".".to_owned());
                let msg = if want.is_empty() {
                    "<rx-slot> needs a name here".to_owned()
                } else {
                    format!("<rx-mail> has no slot \"{want}\"{hint}")
                };
                return Err(err(*at, msg));
            }
            if footer.is_some() {
                return Err(err(
                    *at,
                    "<rx-mail> has the slot \"footer\" twice".to_owned(),
                ));
            }
            footer = Some(inner);
        } else {
            content.push(child.clone());
        }
    }
    out.push_str(&format!("{{% extends {} %}}", attrs::literal(&layout)));
    out.push_str(&pad(&cx.src[open.clone()]));
    if let Some(a) = attr(attrs, "subject") {
        out.push_str(&format!(
            "{{% block title %}}{{{{ {} }}}}{{% endblock %}}",
            text_prop(a, "rx-mail")?
        ));
    }
    out.push_str("{% block content %}");
    cx.nodes(&content, "rx-mail", out)?;
    out.push_str("{% endblock %}");
    if let Some(body) = footer {
        out.push_str("{% block footer %}");
        cx.nodes(body, "rx-mail", out)?;
        out.push_str("{% endblock %}");
    }
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-include template="x.html"/>` or `<rx-include :template="expr" optional/>`:
/// `{% include … %}` of another template, with the current variables.
#[allow(clippy::too_many_arguments)]
pub(super) fn include(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    if super::emit::has_content(cx.src, children) {
        return Err(err(line, "<rx-include> takes no content".to_owned()));
    }
    let template = text_prop(
        attr(attrs, "template").expect("checked: required"),
        "rx-include",
    )?;
    let optional = match attr(attrs, "optional") {
        None => false,
        Some(a) => match attrs::prop_expr(a, attrs::Kind::Bool, &[])
            .map_err(|m| err(a.line, format!("<rx-include> {m}")))?
            .trim_matches(['(', ')'])
        {
            "true" => true,
            "false" => false,
            _ => {
                return Err(err(
                    a.line,
                    "<rx-include> \"optional\" is true or false: write optional".to_owned(),
                ));
            }
        },
    };
    out.push_str(&format!(
        "{{% include {template}{} %}}",
        if optional { " ignore missing" } else { "" }
    ));
    out.push_str(&pad(&cx.src[open.clone()]));
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-link-tabs>`: with `items`, the kit's `link_tabs(...)`; with `<rx-link-tab>` children, the
/// same markup written out, so the links can be looped (`rx-for`) and conditional (`rx-if`).
#[allow(clippy::too_many_arguments)]
pub(super) fn link_tabs(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let items = attr(attrs, "items");
    let given = super::emit::has_content(cx.src, children);
    let label = attr(attrs, "label")
        .map(|a| text_prop(a, "rx-link-tabs"))
        .transpose()?;
    match (items, given) {
        (Some(_), true) => Err(err(
            line,
            "<rx-link-tabs> takes :items or <rx-link-tab> children, not both".to_owned(),
        )),
        (None, false) => Err(err(
            line,
            "<rx-link-tabs> needs :items or <rx-link-tab> children".to_owned(),
        )),
        (Some(items), false) => {
            cx.used.insert(super::emit::Import {
                alias: Module::Ui.alias().to_owned(),
                path: Module::Ui.path().to_owned(),
            });
            let mut args = format!(
                "items={}",
                attrs::prop_expr(items, attrs::Kind::Data, &[])
                    .map_err(|m| err(items.line, format!("<rx-link-tabs> {m}")))?
            );
            if let Some(a) = attr(attrs, "current") {
                args.push_str(&format!(", current={}", text_prop(a, "rx-link-tabs")?));
            }
            if let Some(l) = &label {
                args.push_str(&format!(", label={l}"));
            }
            out.push_str(&format!("{{{{ __rx_ui.link_tabs({args}) }}}}"));
            out.push_str(&pad(&cx.src[open.clone()]));
            for c in children {
                if let Node::Text(s) = c {
                    out.push_str(&pad(&cx.src[s.clone()]));
                }
            }
            if let Some(c) = close {
                out.push_str(&pad(&cx.src[c.clone()]));
            }
            Ok(())
        }
        (None, true) => {
            if let Some(a) = attr(attrs, "current") {
                return Err(err(
                    a.line,
                    "<rx-link-tabs> \"current\" goes with :items; with children, mark the link: <rx-link-tab current>"
                        .to_owned(),
                ));
            }
            const HOLDS: &str = "<rx-link-tabs> holds only <rx-link-tab>";
            for child in children {
                match child {
                    Node::Text(s) if !blank(&cx.src[s.clone()]) => {
                        return Err(err(line, HOLDS.to_owned()));
                    }
                    Node::Element { name, line: at, .. }
                        if name != "rx-link-tab" && name != "template" =>
                    {
                        return Err(err(*at, HOLDS.to_owned()));
                    }
                    _ => {}
                }
            }
            let name = match &label {
                Some(l) => format!("{{{{ ({l}) or t('ui.main_navigation') }}}}"),
                None => "{{ t('ui.main_navigation') }}".to_owned(),
            };
            out.push_str(&format!(
                "<nav class=\"rx-link-tabs\" aria-label=\"{name}\"><div class=\"rx-segmented\">"
            ));
            out.push_str(&pad(&cx.src[open.clone()]));
            cx.nodes(children, "rx-link-tabs", out)?;
            out.push_str("</div></nav>");
            if let Some(c) = close {
                out.push_str(&pad(&cx.src[c.clone()]));
            }
            Ok(())
        }
    }
}

/// `<rx-link-tab :href="…" :current="…">label</rx-link-tab>`: one link of `<rx-link-tabs>`.
#[allow(clippy::too_many_arguments)]
pub(super) fn link_tab(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let href = text_prop(
        attr(attrs, "href").expect("checked: required"),
        "rx-link-tab",
    )?;
    let content = super::emit::has_content(cx.src, children);
    let label = attr(attrs, "label");
    match (label, content) {
        (Some(_), true) => {
            return Err(err(
                line,
                "<rx-link-tab> give \"label\" or content, not both".to_owned(),
            ));
        }
        (None, false) => return Err(err(line, "<rx-link-tab> needs a label".to_owned())),
        _ => {}
    }
    let current = match attr(attrs, "current") {
        None => "false".to_owned(),
        Some(a) => attrs::prop_expr(a, attrs::Kind::Bool, &[])
            .map_err(|m| err(a.line, format!("<rx-link-tab> {m}")))?,
    };
    out.push_str(&format!(
        "<a class=\"rx-segmented__item\" href=\"{{{{ {href} }}}}\""
    ));
    match current.as_str() {
        "false" => {}
        "true" => out.push_str(" aria-current=\"page\""),
        e => out.push_str(&format!(
            "{{% if {e} %}} aria-current=\"page\"{{% endif %}}"
        )),
    }
    out.push('>');
    out.push_str(&pad(&cx.src[open.clone()]));
    match label {
        Some(l) => out.push_str(&format!("{{{{ {} }}}}", text_prop(l, "rx-link-tab")?)),
        None => cx.nodes(children, "rx-link-tab", out)?,
    }
    out.push_str("</a>");
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-mail-text>`: a mail's plain-text version. The text is kept as written, so its line
/// breaks are the mail's; the break right after the opening tag and the one right before the
/// closing tag are left out (they are the tags' own lines). Control flow is a `<template>` with
/// `rx-if` / `rx-for`, which draws no tag.
#[allow(clippy::too_many_arguments)]
pub(super) fn mail_text(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let mut nodes = children.to_vec();
    let (mut lead, mut trail) = (false, false);
    if let Some(Node::Text(r)) = nodes.first_mut() {
        let text = &cx.src[r.clone()];
        let cut = if text.starts_with("\r\n") {
            2
        } else {
            usize::from(text.starts_with('\n'))
        };
        if cut > 0 {
            r.start += cut;
            lead = true;
        }
    }
    if let Some(Node::Text(r)) = nodes.last_mut() {
        let text = &cx.src[r.clone()];
        let cut = if text.ends_with("\r\n") {
            2
        } else {
            usize::from(text.ends_with('\n'))
        };
        if cut > 0 {
            r.end -= cut;
            trail = true;
        }
    }
    out.push_str(&pad(&cx.src[open.clone()]));
    if lead {
        out.push_str("{#\n#}");
    }
    cx.nodes(&nodes, "rx-mail-text", out)?;
    if trail {
        out.push_str("{#\n#}");
    }
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}

/// `<rx-wizard-action>`: `wizard_action(...)` with the steps read from the
/// `<rx-wizard-step>` children, like `<rx-wizard>`.
#[allow(clippy::too_many_arguments)]
pub(super) fn wizard_action(
    cx: &mut Ctx<'_, '_>,
    contract: &Contract,
    attrs: &[Attr<'_>],
    children: &[Node<'_>],
    open: &Range<usize>,
    close: Option<&Range<usize>>,
    line: usize,
    out: &mut String,
) -> Result<(), CompileError> {
    check_attrs(contract, attrs, &[]).map_err(|m| err(line, m))?;
    let (steps, gaps) = wizard_steps(cx, children, line, "rx-wizard-action")?;
    if steps.is_empty() {
        return Err(err(
            line,
            "<rx-wizard-action> needs at least one <rx-wizard-step>".to_owned(),
        ));
    }
    cx.used.insert(super::emit::Import {
        alias: Module::Ui.alias().to_owned(),
        path: Module::Ui.path().to_owned(),
    });
    let id = text_prop(
        attr(attrs, "id").expect("checked: required"),
        "rx-wizard-action",
    )?;
    let list: Vec<String> = steps
        .iter()
        .map(|(k, t, ..)| format!("[{k}, {}]", t.as_deref().unwrap_or(k)))
        .collect();
    let mut args = vec![format!("steps=[{}]", list.join(", "))];
    for a in attrs {
        let n = bare(a.name);
        if n == "route" {
            let e = text_prop(a, "rx-wizard-action")?;
            let call = match super::emit::row() {
                Some((_, key)) => format!("route({e}, {key})"),
                None => format!("route({e})"),
            };
            args.push(format!("action={call}"));
        } else if let Some(p) = contract.props.iter().find(|p| p.name == n) {
            let e = attrs::prop_expr(a, p.kind, p.values)
                .map_err(|m| err(a.line, format!("<rx-wizard-action> {m}")))?;
            args.push(format!("{}={e}", attrs::snake(n)));
        }
    }
    let extra = passthrough_pairs(contract, attrs)?;
    if !extra.is_empty() {
        args.push(format!("attrs={{{}}}", extra.join(", ")));
    }
    out.push_str(&format!(
        "{{% call __rx_ui.wizard_action({}) %}}",
        args.join(", ")
    ));
    out.push_str(&pad(&cx.src[open.clone()]));
    out.push_str(&gaps);
    wizard_step_calls(cx, &id, steps, out)?;
    out.push_str("{% endcall %}");
    if let Some(c) = close {
        out.push_str(&pad(&cx.src[c.clone()]));
    }
    Ok(())
}
