//! Template components: `<rx-…>` and `<app-…>` tags compiled to plain MiniJinja when a template
//! loads. This step scans, builds the tree and reports unknown components; code generation comes
//! in later steps, so a template without components is returned as it is.

use std::sync::LazyLock;

mod app;
#[allow(dead_code)]
mod attrs;
mod contracts;
mod custom_data;
mod emit;
mod live;
mod plugin;
mod scan;
mod special;
pub(crate) mod suggest;
mod tree;

pub(crate) use contracts::{BUILTIN, Contract};
pub use plugin::{Component, Prop};

/// The editor data file for `contracts` and the app's component files (`(file name, source)`
/// pairs). A file whose `<rx-props>` can't be read is left out.
pub(crate) fn custom_data_for(
    contracts: &[Contract],
    files: &[(String, String)],
) -> serde_json::Value {
    let app: Vec<(String, app::AppContract)> = files
        .iter()
        .filter_map(|(file, src)| {
            let stem = file.strip_prefix("components/")?.strip_suffix(".html")?;
            let tag = format!("app-{}", stem.replace('_', "-"));
            Some((tag, app::contract_of(file, src).ok()?))
        })
        .collect();
    custom_data::custom_data(contracts, &app)
}

/// A mistake found while compiling a template, with the line it is on (1-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompileError {
    /// The line of the template source.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

/// The components the compiler knows, and a way to read other templates.
pub(crate) struct Catalog<'a> {
    /// The known components.
    pub contracts: &'a [Contract],
    /// Reads a template's source by name.
    #[allow(dead_code)]
    pub lookup: &'a dyn Fn(&str) -> Option<String>,
}

/// Templates that need no compiling: no component tag, no `rx-if` / `rx-elif` / `rx-else` / `rx-for`
/// and no conditional `?attr="…"` attribute.
static NEEDS_COMPILE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"</?(?:rx|app|live)-|\srx-(?:if|elif|else|for)\b|\s\?[A-Za-z][\w:.-]*\s*=")
        .expect("a valid pattern")
});

/// Compiles the template `file` (its source is `src`) to plain MiniJinja.
pub(crate) fn compile(_file: &str, src: &str, catalog: &Catalog) -> Result<String, CompileError> {
    if !NEEDS_COMPILE.is_match(src) {
        return Ok(src.to_owned());
    }
    if let Some((header, body)) = app::component_file(src)? {
        let inner = compile(_file, &body, catalog)?;
        return Ok(format!("{header}{inner}{{% endmacro %}}"));
    }
    let nodes = tree::build(scan::scan(src)?)?;
    emit::emit(src, &nodes, catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog<'static> {
        Catalog {
            contracts: &[],
            lookup: &|_| None,
        }
    }

    #[test]
    fn builtins_compile_to_themselves() {
        // The mail templates written with tags (`<rx-mail>`) compile; every other one is left
        // as it is.
        let tagged = |source: &str| source.contains("<rx-");
        for (name, source) in crate::view::BUILTIN {
            if tagged(source) {
                let builtin = Catalog {
                    contracts: BUILTIN,
                    lookup: &|_| None,
                };
                let out = compile(name, source, &builtin).unwrap();
                assert!(!tagged(&out), "{name}: {out}");
                continue;
            }
            assert_eq!(
                compile(name, source, &catalog()).unwrap(),
                *source,
                "{name}"
            );
        }
    }

    #[test]
    fn plain_templates_pass_through() {
        let src = "<div class=\"rx-card\">{{ x }}</div> <a rx-ifx=\"1\">";
        assert_eq!(compile("a.html", src, &catalog()).unwrap(), src);
    }

    #[test]
    fn unknown_components_name_the_line() {
        let e = compile(
            "bad.html",
            "a\nb\n<rx-tabel :rows=\"x\">\n</rx-tabel>",
            &catalog(),
        )
        .unwrap_err();
        assert_eq!(e.line, 3);
        assert_eq!(e.message, "unknown component <rx-tabel>");
    }

    #[test]
    fn tree_errors_come_through() {
        let e = compile("bad.html", "<app-x>", &catalog()).unwrap_err();
        assert_eq!(e.message, "<app-x> opened here is never closed");
    }

    fn builtin() -> Catalog<'static> {
        Catalog {
            contracts: BUILTIN,
            lookup: &|_| None,
        }
    }

    #[test]
    fn element_components_keep_attributes_and_merge_classes() {
        let out = compile(
            "a.html",
            "<rx-row end data-bs-reveal class=\"x\" :title=\"t\">\n<p>a</p></rx-row>",
            &builtin(),
        )
        .unwrap();
        assert_eq!(
            out,
            "<div class=\"rx-row rx-row--end x\" data-bs-reveal title=\"{{ t }}\">\n<p>a</p></div>"
        );
        let out = compile(
            "a.html",
            "<rx-row end data-bs-reveal class=\"x\">\n<p>a</p></rx-row>",
            &builtin(),
        )
        .unwrap();
        assert_eq!(
            out,
            "<div class=\"rx-row rx-row--end x\" data-bs-reveal>\n<p>a</p></div>"
        );
        assert_eq!(
            compile("a.html", "<rx-stack />", &builtin()).unwrap(),
            "<div class=\"rx-stack\"></div>"
        );
    }

    #[test]
    fn the_dashboard_shell_is_rx_shell_with_fill() {
        assert_eq!(
            compile(
                "a.html",
                "<rx-shell fill>\n<main></main></rx-shell>",
                &builtin()
            )
            .unwrap(),
            "<div class=\"rx-shell rx-shell--fill\">\n<main></main></div>"
        );
        assert_eq!(
            compile("a.html", "<rx-shell :fill=\"wide\"></rx-shell>", &builtin()).unwrap(),
            "<div class=\"rx-shell{% if (wide) %} rx-shell--fill{% endif %}\"></div>"
        );
        assert_eq!(
            compile("a.html", "<rx-shell></rx-shell>", &builtin()).unwrap(),
            "<div class=\"rx-shell\"></div>"
        );
    }

    #[test]
    fn macro_components_call_the_kit() {
        let out = compile(
            "a.html",
            "<rx-card title=\"Contact\" subtitle=\"{{ t('a') }}\">body</rx-card>",
            &builtin(),
        )
        .unwrap();
        assert_eq!(
            out,
            "{% import \"renox/ui.html\" as __rx_ui %}{% call __rx_ui.card(title=\"Contact\", subtitle=(t('a'))) %}body{% endcall %}"
        );
        let out = compile(
            "a.html",
            "<rx-page-header title=\"T\" badge-kind=\"info\"> </rx-page-header><rx-badge text=\"x\"/><rx-badge text=\"y\"/>",
            &builtin(),
        )
        .unwrap();
        assert_eq!(
            out,
            "{% import \"renox/ui.html\" as __rx_ui %}{{ __rx_ui.page_header(title=\"T\", badge_kind=\"info\") }}{{ __rx_ui.badge(text=\"x\") }}{{ __rx_ui.badge(text=\"y\") }}"
        );
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            msg("<rx-badge lable=\"x\"/>"),
            "<rx-badge> has no attribute \"lable\". It takes: text, kind"
        );
        assert_eq!(
            msg("<rx-badge/>"),
            "<rx-badge> needs the attribute \"text\""
        );
    }

    #[test]
    fn slots_fill_props_and_pass_arguments() {
        use contracts::{Module, Render, Slot};
        static T: &[Contract] = &[Contract {
            tag: "rx-t",
            doc: "t",
            render: Render::Macro {
                module: Module::Ui,
                name: "t",
            },
            props: &[],
            slots: &[
                Slot {
                    name: "",
                    into: None,
                    args: &["row", "prefix"],
                    optional: false,
                    doc: "x",
                },
                Slot {
                    name: "footer",
                    into: None,
                    args: &[],
                    optional: true,
                    doc: "x",
                },
            ],
            events: &[],
            route_prop: None,
            attrs: false,
            parent: None,
        }];
        let all: Vec<Contract> = BUILTIN.iter().chain(T).copied().collect();
        let c = Catalog {
            contracts: &all,
            lookup: &|_| None,
        };
        let imp = "{% import \"renox/ui.html\" as __rx_ui %}";
        let ok = |s: &str| compile("a.html", s, &c).unwrap();
        let msg = |s: &str| compile("a.html", s, &c).unwrap_err().message;
        assert_eq!(
            ok("<rx-badge kind=\"info\">New {{ n }}</rx-badge>"),
            format!(
                "{imp}{{% set __rx_slot_1 %}}New {{{{ n }}}}{{% endset %}}{{{{ __rx_ui.badge(kind=\"info\", text=__rx_slot_1) }}}}"
            )
        );
        assert_eq!(
            ok("<rx-t>{{ row }}<rx-slot name=\"footer\">F</rx-slot></rx-t>"),
            format!(
                "{imp}{{% set __rx_slot_1 %}}F{{% endset %}}{{% call(row, prefix) __rx_ui.t(footer=__rx_slot_1) %}}{{{{ row }}}}{{% endcall %}}"
            )
        );
        assert_eq!(
            msg("<rx-t><rx-slot name=\"fotter\">F</rx-slot></rx-t>"),
            "<rx-t> has no slot \"fotter\"; did you mean \"footer\"?"
        );
        assert_eq!(
            msg("<rx-badge text=\"a\">b</rx-badge>"),
            "<rx-badge> give \"text\" or content, not both"
        );
        assert_eq!(
            msg("<rx-badge><rx-slot>b</rx-slot></rx-badge>"),
            "<rx-slot> needs a name here"
        );
        assert_eq!(
            msg("<rx-badge/>"),
            "<rx-badge> needs the attribute \"text\""
        );
    }

    #[test]
    fn typos_get_a_suggestion() {
        let e = compile("a.html", "<rx-stak></rx-stak>", &builtin()).unwrap_err();
        assert_eq!(
            e.message,
            "unknown component <rx-stak>; did you mean <rx-stack>?"
        );
    }

    #[test]
    fn padding_keeps_the_line_count() {
        let src = "<rx-row\n  end\n  class=\"x\">\na</rx-row>";
        let out = compile("a.html", src, &builtin()).unwrap();
        assert_eq!(src.lines().count(), out.lines().count());
        assert_eq!(emit::pad("a\nb\n"), "{#\n\n#}");
        assert_eq!(emit::pad("ab"), "");
    }

    #[test]
    fn control_flow_wraps_elements_and_components() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        assert_eq!(
            c("<tr class=\"a\" rx-for=\"s in xs\">x</tr>"),
            "{% for s in xs %}<tr class=\"a\">x</tr>{% endfor %}"
        );
        assert_eq!(
            c("<p rx-if=\"a\">A</p>\n<p rx-else>B</p>"),
            "{% if a %}<p>A</p>\n{% else %}<p>B</p>{% endif %}"
        );
        assert_eq!(
            c("<rx-badge can=\"update\" text=\"x\"/>"),
            "{% import \"renox/ui.html\" as __rx_ui %}{% if can(\"update\") %}{{ __rx_ui.badge(text=\"x\") }}{% endif %}"
        );
        assert_eq!(
            c("<rx-badge rx-for=\"s in xs\" rx-if=\"s.ok\" can=\"a\" text=\"x\"/>"),
            "{% import \"renox/ui.html\" as __rx_ui %}{% for s in xs %}{% if s.ok and can(\"a\") %}{{ __rx_ui.badge(text=\"x\") }}{% endif %}{% endfor %}"
        );
    }

    #[test]
    fn templates_render_their_children_without_a_wrapper() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        assert_eq!(
            c("<template rx-if=\"a\">text <b>x</b></template>"),
            "{% if a %}text <b>x</b>{% endif %}"
        );
        assert_eq!(
            c("<template rx-if=\"a\">A</template>\n<template rx-else>B</template>"),
            "{% if a %}A\n{% else %}B{% endif %}"
        );
        assert_eq!(
            c("<template rx-for=\"x in xs\">{{ loop.index }}. {{ x }}, </template>"),
            "{% for x in xs %}{{ loop.index }}. {{ x }}, {% endfor %}"
        );
        // A plain template (Alpine's) is left alone, inside one with a directive too.
        assert_eq!(
            c("<template x-if=\"a\"><i>x</i></template>"),
            "<template x-if=\"a\"><i>x</i></template>"
        );
        assert_eq!(
            c("<template rx-if=\"a\"><template x-if=\"b\">y</template>z</template>"),
            "{% if a %}<template x-if=\"b\">y</template>z{% endif %}"
        );
        assert_eq!(
            c(
                "<template rx-if=\"a\"><rx-badge text=\"x\"/><template rx-for=\"y in ys\">{{ y }}</template></template>"
            ),
            "{% import \"renox/ui.html\" as __rx_ui %}{% if a %}{{ __rx_ui.badge(text=\"x\") }}{% for y in ys %}{{ y }}{% endfor %}{% endif %}"
        );
    }

    #[test]
    fn elif_chains_end_with_one_endif() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        assert_eq!(
            c(
                "<p rx-if=\"a\">A</p>\n<p rx-elif=\"b\">B</p> <!-- c -->\n<p rx-elif=\"c or d\">C</p>\n<p rx-else>D</p>"
            ),
            "{% if a %}<p>A</p>\n{% elif b %}<p>B</p> <!-- c -->\n{% elif c or d %}<p>C</p>\n{% else %}<p>D</p>{% endif %}"
        );
        assert_eq!(
            c("<p rx-if=\"a\">A</p><p rx-elif=\"b\">B</p>"),
            "{% if a %}<p>A</p>{% elif b %}<p>B</p>{% endif %}"
        );
        assert_eq!(
            c(
                "<template rx-if=\"a\">A</template><template rx-elif=\"b\">B</template><template rx-else>C</template>"
            ),
            "{% if a %}A{% elif b %}B{% else %}C{% endif %}"
        );
        // On components, with `can`.
        assert_eq!(
            c(
                "<rx-badge rx-if=\"a\" text=\"x\"/><rx-badge rx-elif=\"b\" can=\"edit\" text=\"y\"/><rx-badge rx-else text=\"z\"/>"
            ),
            "{% import \"renox/ui.html\" as __rx_ui %}{% if a %}{{ __rx_ui.badge(text=\"x\") }}{% elif b and can(\"edit\") %}{{ __rx_ui.badge(text=\"y\") }}{% else %}{{ __rx_ui.badge(text=\"z\") }}{% endif %}"
        );
        // Two chains in a row.
        assert_eq!(
            c("<p rx-if=\"a\">A</p><p rx-else>B</p><p rx-if=\"c\">C</p><p rx-elif=\"d\">D</p>"),
            "{% if a %}<p>A</p>{% else %}<p>B</p>{% endif %}{% if c %}<p>C</p>{% elif d %}<p>D</p>{% endif %}"
        );
    }

    #[test]
    fn elif_and_template_errors_name_the_line() {
        let err = |s: &str| compile("a.html", s, &builtin()).unwrap_err();
        let msg = |s: &str| err(s).message;
        assert_eq!(
            msg("<p rx-elif=\"a\">B</p>"),
            "rx-elif must follow an element with rx-if"
        );
        let e = err("<p>x</p>\n\n<template rx-elif=\"a\">B</template>");
        assert_eq!(
            (e.line, e.message.as_str()),
            (3, "rx-elif must follow an element with rx-if")
        );
        let e = err("<p rx-if=\"a\">A</p>\n<p rx-else>B</p>\n<p rx-else>C</p>");
        assert_eq!(
            (e.line, e.message.as_str()),
            (3, "rx-else must follow an element with rx-if")
        );
        assert_eq!(
            msg("<p rx-if=\"a\">A</p><p rx-else>B</p><p rx-elif=\"b\">C</p>"),
            "rx-elif must follow an element with rx-if"
        );
        assert_eq!(
            msg("<p rx-for=\"a in b\">A</p><template rx-else>B</template>"),
            "rx-else can't follow an element with rx-for"
        );
        assert_eq!(
            msg("<p rx-for=\"a in b\">A</p><p rx-elif=\"a\">B</p>"),
            "rx-elif can't follow an element with rx-for"
        );
        assert_eq!(
            msg("<p rx-if=\"a\">A</p><p rx-elif>B</p>"),
            "rx-elif needs a condition"
        );
        assert_eq!(
            msg("<p rx-if=\"a\">A</p><p rx-elif=\"b\" rx-if=\"c\">B</p>"),
            "rx-elif can't be combined with rx-if or rx-for"
        );
        assert_eq!(
            msg("<p rx-if=\"a\">A</p><p rx-elif=\"b\" rx-else>B</p>"),
            "rx-else can't be combined with rx-if, rx-elif or rx-for"
        );
        assert_eq!(
            msg("<template rx-if=\"a\" class=\"x\">A</template>"),
            "<template> with rx-if, rx-elif, rx-else or rx-for takes no other attribute, not \"class\""
        );
        assert_eq!(
            msg("<template rx-if>A</template>"),
            "rx-if needs a condition"
        );
    }

    #[test]
    fn let_wraps_the_rest_of_the_siblings_in_a_with() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        assert_eq!(
            c(
                "<p>{{ a }}</p><rx-let name=\"label\" :value=\"o.name | upper\"/><p>{{ label }}</p><i>{{ label }}</i>"
            ),
            "<p>{{ a }}</p>{% with label = (o.name | upper) %}<p>{{ label }}</p><i>{{ label }}</i>{% endwith %}"
        );
        assert_eq!(
            c("<rx-let name=\"title\" value=\"Hello {{ who }}\"></rx-let>x"),
            "{% with title = \"Hello \" ~ (who) %}x{% endwith %}"
        );
        // The scope is the enclosing element or slot: it ends there.
        assert_eq!(
            c("<rx-card><rx-let name=\"a\" :value=\"1\"/>{{ a }}</rx-card>{{ a }}"),
            "{% import \"renox/ui.html\" as __rx_ui %}{% call __rx_ui.card() %}{% with a = (1) %}{{ a }}{% endwith %}{% endcall %}{{ a }}"
        );
        // Several lets nest, and the next sibling after the element is outside.
        assert_eq!(
            c(
                "<div rx-if=\"x\"><rx-let name=\"a\" :value=\"1\"/><rx-let name=\"b\" :value=\"a + 1\"/>{{ b }}</div>{{ a }}"
            ),
            "{% if x %}<div>{% with a = (1) %}{% with b = (a + 1) %}{{ b }}{% endwith %}{% endwith %}</div>{% endif %}{{ a }}"
        );
        // A plain HTML element ends the scope too, at its end tag.
        assert_eq!(
            c(
                "<div><p>{{ a }}</p><rx-let name=\"b\" :value=\"1\"/><p><i>{{ b }}</i></p></div><p>{{ b }}</p><section><rx-let name=\"b\" :value=\"2\"/><br>{{ b }}</section>"
            ),
            "<div><p>{{ a }}</p>{% with b = (1) %}<p><i>{{ b }}</i></p>{% endwith %}</div><p>{{ b }}</p><section>{% with b = (2) %}<br>{{ b }}{% endwith %}</section>"
        );
        // A loop variable named after a let is not a clash once the loop is over.
        assert_eq!(
            c("<li rx-for=\"s in xs\">{{ s }}</li><rx-let name=\"s\" :value=\"1\"/>"),
            "{% for s in xs %}<li>{{ s }}</li>{% endfor %}{% with s = (1) %}{% endwith %}"
        );
    }

    #[test]
    fn let_keeps_the_line_count_and_reports_mistakes() {
        let src = "<rx-let\n name=\"a\"\n :value=\"b\"\n/>\n<p>{{ a }}</p>";
        let out = compile("a.html", src, &builtin()).unwrap();
        assert_eq!(src.lines().count(), out.lines().count(), "{out}");
        let err = |s: &str| compile("a.html", s, &builtin()).unwrap_err();
        let msg = |s: &str| err(s).message;
        assert!(msg("<rx-let :value=\"1\"/>").contains("needs the attribute \"name\""));
        assert!(msg("<rx-let name=\"a\"/>").contains("needs the attribute \"value\""));
        assert!(msg("<rx-let name=\"a\" :value=\"1\">x</rx-let>").contains("takes no content"));
        assert!(
            msg("<rx-let name=\"a-b\" :value=\"1\"/>").contains("letters, digits and underscores")
        );
        assert!(msg("<rx-let :name=\"n\" :value=\"1\"/>").contains("plain text"));
        assert!(msg("<rx-let name=\"a\" :valu=\"1\"/>").contains("did you mean \"value\""));
        assert!(msg("<rx-let name=\"a\" :value=\"1\" rx-if=\"x\"/>").contains("takes no rx-if"));
        assert!(
            msg("<rx-let name=\"loop\" :value=\"1\"/>").contains("name every page already has")
        );
        assert!(
            msg("<rx-let name=\"auth\" :value=\"1\"/>").contains("name every page already has")
        );
        assert!(msg("<rx-let name=\"__rx_x\" :value=\"1\"/>").contains("letters, digits"));
        // Clashes: another let, a loop variable, a table row.
        let e = err("<rx-let name=\"a\" :value=\"1\"/>\n<rx-let name=\"a\" :value=\"2\"/>");
        assert_eq!(e.line, 2);
        assert!(
            e.message.contains("\"a\" is already taken here"),
            "{}",
            e.message
        );
        assert!(
            msg("<li rx-for=\"s in xs\"><rx-let name=\"s\" :value=\"1\"/></li>")
                .contains("already taken")
        );
        assert!(
            msg("<li rx-for=\"k, v in xs\"><rx-let name=\"v\" :value=\"1\"/></li>")
                .contains("already taken")
        );
        assert!(
            msg("<rx-table :rows=\"xs\" as=\"p\"><rx-column label=\"A\"><rx-let name=\"p\" :value=\"1\"/></rx-column></rx-table>")
                .contains("already taken")
        );
        // An else can't follow a let.
        assert_eq!(
            msg("<p rx-if=\"a\">A</p><rx-let name=\"b\" :value=\"1\"/><p rx-else>B</p>"),
            "rx-else must follow an element with rx-if"
        );
    }

    #[test]
    fn script_and_style_carry_the_nonce() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        assert_eq!(
            c("<rx-script>if (a<b && c) { go(\"x\"); }</rx-script>"),
            "<script nonce=\"{{ csp_nonce() }}\">if (a<b && c) { go(\"x\"); }</script>"
        );
        assert_eq!(
            c("<rx-script type=\"module\" defer :src=\"asset('a.js')\"/>"),
            "<script nonce=\"{{ csp_nonce() }}\" type=\"module\" defer src=\"{{ asset('a.js') }}\"></script>"
        );
        assert_eq!(
            c("<rx-style>a > b { color: red }</rx-style>"),
            "<style nonce=\"{{ csp_nonce() }}\">a > b { color: red }</style>"
        );
        assert_eq!(
            c(
                "<rx-push stack=\"scripts\" once=\"k\"><rx-script>var n = {{ n|tojson }};</rx-script></rx-push>"
            ),
            "{% call push(\"scripts\", once=\"k\") %}<script nonce=\"{{ csp_nonce() }}\">var n = {{ n|tojson }};</script>{% endcall %}"
        );
        let src = "<rx-script\n type=\"module\">\nx();\n</rx-script>";
        assert_eq!(
            c(src),
            "<script nonce=\"{{ csp_nonce() }}\" type=\"module\">{#\n#}\nx();\n</script>"
        );
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            msg("<rx-script nonce=\"x\"></rx-script>"),
            "<rx-script> sets the nonce itself; remove \"nonce\""
        );
        assert_eq!(
            msg("<rx-style :media=\"\"></rx-style>"),
            "<rx-style> \":media\" needs an expression"
        );
        assert!(msg("<rx-scrpt></rx-scrpt>").contains("did you mean <rx-script>"));
    }

    #[test]
    fn control_flow_keeps_the_line_count() {
        let src = "<p\n  class=\"a\"\n  rx-if=\"a\"\n>A</p>\n<!-- c -->\n<p rx-else>B</p>\n<tr\n rx-for=\"s in xs\">x</tr>";
        let out = compile("a.html", src, &builtin()).unwrap();
        assert_eq!(src.lines().count(), out.lines().count(), "{out}");
    }

    #[test]
    fn control_flow_errors() {
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            msg("<p rx-else>B</p>"),
            "rx-else must follow an element with rx-if"
        );
        assert_eq!(
            msg("<p rx-for=\"a in b\" rx-if=\"a\">A</p><p rx-else>B</p>"),
            "rx-else can't follow an element with rx-for"
        );
        assert_eq!(msg("<p rx-if>A</p>"), "rx-if needs a condition");
        assert_eq!(
            msg("<p rx-for=\"xs\">A</p>"),
            "rx-for needs \"item in list\""
        );
    }

    #[test]
    fn contract_errors_use_the_fixed_texts() {
        use contracts::{Prop, Render};
        const PROPS: &[Prop] = &[
            Prop {
                name: "label",
                kind: attrs::Kind::Text,
                required: true,
                values: &[],
                doc: "x",
            },
            Prop {
                name: "size",
                kind: attrs::Kind::Text,
                required: false,
                values: &[],
                doc: "x",
            },
        ];
        static T: &[Contract] = &[Contract {
            tag: "rx-t",
            doc: "t",
            render: Render::Element {
                tag: "p",
                class: "t",
            },
            props: PROPS,
            slots: &[],
            events: &[],
            route_prop: None,
            attrs: false,
            parent: Some("rx-stack"),
        }];
        let all: Vec<Contract> = BUILTIN.iter().chain(T).copied().collect();
        let c = Catalog {
            contracts: &all,
            lookup: &|_| None,
        };
        let msg = |s: &str| compile("a.html", s, &c).unwrap_err().message;
        assert_eq!(
            msg("<rx-stack><rx-t label=\"a\" sise=\"b\"/></rx-stack>"),
            "<rx-t> has no attribute \"sise\"; did you mean \"size\"? It takes: label, size"
        );
        assert_eq!(
            msg("<rx-stack><rx-t/></rx-stack>"),
            "<rx-t> needs the attribute \"label\""
        );
        assert_eq!(
            msg("<rx-stack><rx-t label=\"a\">x</rx-t></rx-stack>"),
            "<rx-t> takes no content"
        );
        assert_eq!(
            msg("<rx-t label=\"a\"/>"),
            "<rx-t> belongs inside <rx-stack>"
        );
    }

    #[test]
    fn pages_extend_a_layout_and_move_slots() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c(
                "<rx-page layout=\"layouts/staff.html\" title=\"{{ t('x') }}\">\n<p>a</p>\n</rx-page>"
            ),
            "{% extends \"layouts/staff.html\" %}{% block seo %}{{ seo(title=(t('x'))) }}{% endblock %}{% block content %}\n<p>a</p>\n{% endblock %}"
        );
        assert_eq!(
            c(
                "{# c #}\n<rx-page layout=\"l.html\" title=\"T\" description=\"D\">x<rx-slot name=\"scripts\">s</rx-slot></rx-page>\n"
            ),
            "{# c #}\n{% extends \"l.html\" %}{% block seo %}{{ seo(title=\"T\", description=\"D\") }}{% endblock %}{% block content %}x{% endblock %}{% block scripts %}s{% endblock %}\n"
        );
        assert_eq!(
            c("<rx-page layout=\"l.html\"><rx-slot name=\"seo\">S</rx-slot>x</rx-page>"),
            "{% extends \"l.html\" %}{% block content %}x{% endblock %}{% block seo %}S{% endblock %}"
        );
        assert_eq!(
            msg("<rx-page layout=\"{{ l }}\">x</rx-page>"),
            "<rx-page> \"layout\" must be a file name, not {{ }}"
        );
        assert_eq!(
            msg("<p>a</p><rx-page layout=\"l.html\">x</rx-page>"),
            "<rx-page> must hold the whole template"
        );
        assert_eq!(
            msg("<rx-stack><rx-page layout=\"l.html\">x</rx-page></rx-stack>"),
            "<rx-page> must hold the whole template"
        );
        assert_eq!(
            msg(
                "<rx-page layout=\"l.html\" title=\"T\"><rx-slot name=\"seo\">S</rx-slot></rx-page>"
            ),
            "give title or a seo slot, not both"
        );
        assert_eq!(
            msg("<rx-page title=\"T\">x</rx-page>"),
            "<rx-page> needs the attribute \"layout\""
        );
    }

    #[test]
    fn push_calls_the_stack_function() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        assert_eq!(
            c("<rx-push stack=\"scripts\">\n<script></script>\n</rx-push>"),
            "{% call push(\"scripts\") %}\n<script></script>\n{% endcall %}"
        );
        assert_eq!(
            c("<rx-push stack=\"scripts\" once=\"chart\">x</rx-push>"),
            "{% call push(\"scripts\", once=\"chart\") %}x{% endcall %}"
        );
    }

    #[test]
    fn block_makes_a_jinja_block() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c("<rx-block name=\"bookmarks\">\n<p>x</p>\n</rx-block>"),
            "{% block bookmarks %}\n<p>x</p>\n{% endblock %}"
        );
        assert_eq!(
            c("<rx-block name=\"content\"/>"),
            "{% block content %}{% endblock %}"
        );
        assert!(msg("<rx-block>x</rx-block>").contains("needs the attribute \"name\""));
        assert!(msg("<rx-block name=\"a-b\"/>").contains("letters, digits and underscores"));
        assert!(msg("<rx-block name=\"1a\"/>").contains("letters, digits and underscores"));
        assert!(msg("<rx-block name=\"\"/>").contains("letters, digits and underscores"));
        assert!(msg("<rx-block :name=\"n\"/>").contains("plain text"));
        assert!(msg("<rx-block name=\"{{ n }}\"/>").contains("plain text"));
        assert!(msg("<rx-block name=\"a\" id=\"b\"/>").contains("no attribute \"id\""));
    }

    #[test]
    fn pagination_picks_the_macro_and_imports_it() {
        let out = compile("a.html", "<rx-pagination :page=\"items\"/>", &builtin()).unwrap();
        assert!(
            out.contains("{% import \"renox/pagination.html\" as __rx_pagination %}"),
            "{out}"
        );
        assert!(out.contains("{% set __rx_page_1 = (items) %}"), "{out}");
        assert!(
            out.contains("{% if __rx_page_1.has_next is defined and __rx_page_1.last_page is not defined %}{{ __rx_pagination.simple_pagination(__rx_page_1) }}{% else %}{{ __rx_pagination.pagination(__rx_page_1) }}{% endif %}"),
            "{out}"
        );
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert!(msg("<rx-pagination/>").contains("needs the attribute \"page\""));
        assert!(msg("<rx-pagination page=\"items\"/>").contains("takes data"));
        assert!(msg("<rx-pagination :page=\"p\">x</rx-pagination>").contains("takes no content"));
        assert!(msg("<rx-pagination :page=\"p\" size=\"3\"/>").contains("no attribute \"size\""));
    }

    #[test]
    fn page_plumbing_tags_call_template_functions() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(c("<rx-head/>"), "{{ renox_head() }}");
        assert_eq!(c("<rx-ui/>"), "{{ renox_ui() }}");
        assert_eq!(c("<rx-theme/>"), "{{ renox_theme() }}");
        assert_eq!(
            c("<rx-ui :styles=\"false\"/>"),
            "{{ renox_ui(styles=(false)) }}"
        );
        assert_eq!(
            c("<rx-ui styles=\"false\"/>"),
            "{{ renox_ui(styles=false) }}"
        );
        assert_eq!(c("<rx-toasts/>"), "{{ toasts() }}");
        assert_eq!(
            c("<rx-outlet stack=\"scripts\"/>"),
            "{{ stack(\"scripts\") }}"
        );
        assert!(msg("<rx-outlet/>").contains("exactly one of"));
        assert!(msg("<rx-outlet name=\"a\" stack=\"b\"/>").contains("\"name\""));
        assert!(msg("<rx-outlet name=\"a\" stack=\"b\"/>").contains("\"stack\""));
        assert!(msg("<rx-outlet stack=\"a\">x</rx-outlet>").contains("takes no content"));
        assert!(msg("<rx-outlet name=\"a-b\"/>").contains("letters, digits and underscores"));
        assert_eq!(
            c("<rx-outlet name=\"content\"/>"),
            "{% block content %}{% endblock %}"
        );
        assert_eq!(
            c("<rx-outlet name=\"seo\">{{ seo() }}</rx-outlet>"),
            "{% block seo %}{{ seo() }}{% endblock %}"
        );
        assert!(msg("<rx-head>x</rx-head>").contains("takes no content"));
        assert!(msg("<rx-head foo=\"1\"/>").contains("has no"));
        assert!(msg("<rx-outlt stack=\"a\"/>").contains("did you mean <rx-outlet>"));
        assert!(msg("<rx-pagnation :page=\"p\"/>").contains("did you mean <rx-pagination>"));
        assert!(msg("<rx-blok name=\"a\"/>").contains("did you mean <rx-block>"));
    }

    #[test]
    fn actions_use_route_and_pass_attributes_through() {
        let out = compile(
            "a.html",
            "<rx-link-button route=\"stock.suppliers.create\" variant=\"primary\" icon=\"plus\">{{ t('n') }}</rx-link-button>",
            &builtin(),
        )
        .unwrap();
        assert_eq!(
            out,
            "{% import \"renox/ui.html\" as __rx_ui %}{% set __rx_slot_1 %}{{ t('n') }}{% endset %}{{ __rx_ui.link_button(href=route(\"stock.suppliers.create\"), variant=\"primary\", icon=\"plus\", label=__rx_slot_1) }}"
        );
        let out = compile(
            "a.html",
            "<rx-button hx-post=\"/x\" hx-vals=\"{{ v }}\" @click=\"go\" autofocus>Save</rx-button>",
            &builtin(),
        )
        .unwrap();
        assert!(
            out.ends_with(
                "label=__rx_slot_1, attrs={\"hx-post\": \"/x\", \"hx-vals\": (v), \"@click\": \"go\", \"autofocus\": true}) }}"
            ),
            "{out}"
        );
    }

    #[test]
    fn other_attributes_go_to_attrs() {
        let out = compile(
            "a.html",
            "<rx-button hx-target=\"#x\" class=\"a\" @click=\"go\" id=\"i\" data-n=\"{{ n }}\">x</rx-button>",
            &builtin(),
        )
        .unwrap();
        assert!(
            out.contains(
                "attrs={\"hx-target\": \"#x\", \"class\": \"a\", \"@click\": \"go\", \"id\": \"i\", \"data-n\": "
            ),
            "{out}"
        );
    }

    #[test]
    fn declared_events_become_alpine_listeners() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let out = c(
            "<rx-action-sheet id=\"s\" label=\"L\" action=\"/a\" title=\"T\" @saved=\"mark\">x</rx-action-sheet>",
        );
        assert!(
            out.contains("\"x-on:rx:action-sheet:saved.self\": \"mark\""),
            "{out}"
        );
        // Modifiers are kept after .self.
        let out = c(
            "<rx-action-sheet id=\"s\" label=\"L\" action=\"/a\" title=\"T\" @closed.once.window=\"m\">x</rx-action-sheet>",
        );
        assert!(
            out.contains("\"x-on:rx:action-sheet:closed.self.once.window\": \"m\""),
            "{out}"
        );
        // Tabs and wizards pass them to their macro.
        let out = c(
            "<rx-tabs id=\"t\" @changed=\"go\"><rx-tab key=\"a\" label=\"A\">x</rx-tab></rx-tabs>",
        );
        assert!(
            out.contains("attrs={\"x-on:rx:tabs:changed.self\": \"go\"}"),
            "{out}"
        );
    }

    #[test]
    fn dom_events_pass_through_and_typos_fail() {
        let c = |s: &str| compile("a.html", s, &builtin());
        let out = c("<rx-action-sheet id=\"s\" label=\"L\" action=\"/a\" title=\"T\" @click=\"go\">x</rx-action-sheet>").unwrap();
        assert!(out.contains("\"@click\": \"go\""), "{out}");
        let out = c("<rx-button @clsoed=\"go\">x</rx-button>").unwrap();
        assert!(
            out.contains("\"@clsoed\": \"go\""),
            "no events declared: {out}"
        );
        let msg = c("<rx-action-sheet id=\"s\" label=\"L\" action=\"/a\" title=\"T\" @clsoed=\"go\">x</rx-action-sheet>")
            .unwrap_err()
            .message;
        assert_eq!(
            msg,
            "<rx-action-sheet> has no event \"clsoed\"; did you mean \"@closed\"? It sends: opened, closed, saved, failed"
        );
        // Far from every declared event: passed through.
        let out = c("<rx-action-sheet id=\"s\" label=\"L\" action=\"/a\" title=\"T\" @whatever=\"go\">x</rx-action-sheet>").unwrap();
        assert!(out.contains("\"@whatever\""), "{out}");
    }

    #[test]
    fn route_and_class_errors() {
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            msg("<rx-card route=\"a\">x</rx-card>"),
            "<rx-card> has no route shortcut"
        );
        assert_eq!(
            msg("<rx-link-button route=\"a\" href=\"/b\">x</rx-link-button>"),
            "<rx-link-button> takes route or \"href\", not both"
        );
        assert!(msg("<rx-button :foo=\"x\">x</rx-button>").contains("has no attribute \"foo\""),);
        assert_eq!(
            msg("<rx-link-button>x</rx-link-button>"),
            "<rx-link-button> needs the attribute \"href\""
        );
    }
    const UI_IMPORT: &str = "{% import \"renox/ui.html\" as __rx_ui %}";
    const MAIL_IMPORT: &str = "{% import \"renox/mail/components.html\" as __rx_mail %}";

    #[test]
    fn mail_extends_the_layout_and_fills_its_blocks() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        assert_eq!(
            c("<rx-mail subject=\"{{ t('hi') }}\">\n<p>a</p>\n</rx-mail>"),
            "{% extends \"renox/mail/layout.html\" %}{% block title %}{{ (t('hi')) }}{% endblock %}{% block content %}\n<p>a</p>\n{% endblock %}"
        );
        assert_eq!(
            c("<rx-mail>x<rx-slot name=\"footer\">Bye</rx-slot></rx-mail>"),
            "{% extends \"renox/mail/layout.html\" %}{% block content %}x{% endblock %}{% block footer %}Bye{% endblock %}"
        );
        assert_eq!(
            c("<rx-mail layout=\"mail/own.html\" subject=\"S\">x</rx-mail>"),
            "{% extends \"mail/own.html\" %}{% block title %}{{ \"S\" }}{% endblock %}{% block content %}x{% endblock %}"
        );
        // The macros inside are imported once, ahead of the extends.
        let out = c("<rx-mail><rx-mail-divider/><rx-mail-divider/></rx-mail>");
        assert!(
            out.starts_with(&format!("{MAIL_IMPORT}{{% extends")),
            "{out}"
        );
        assert_eq!(out.matches("{% import").count(), 1, "{out}");
    }

    #[test]
    fn mail_errors() {
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            msg("<rx-mail><rx-slot name=\"foter\">x</rx-slot></rx-mail>"),
            "<rx-mail> has no slot \"foter\"; did you mean \"footer\"?"
        );
        assert_eq!(
            msg("<rx-mail><rx-slot>x</rx-slot></rx-mail>"),
            "<rx-slot> needs a name here"
        );
        assert_eq!(
            msg(
                "<rx-mail><rx-slot name=\"footer\">a</rx-slot><rx-slot name=\"footer\">b</rx-slot></rx-mail>"
            ),
            "<rx-mail> has the slot \"footer\" twice"
        );
        assert_eq!(
            msg("<p>a</p><rx-mail>x</rx-mail>"),
            "<rx-mail> must hold the whole template"
        );
        assert_eq!(
            msg("<rx-stack><rx-mail>x</rx-mail></rx-stack>"),
            "<rx-mail> must hold the whole template"
        );
        assert_eq!(
            msg("<rx-mail layout=\"{{ l }}\">x</rx-mail>"),
            "<rx-mail> \"layout\" must be a file name, not {{ }}"
        );
        assert!(msg("<rx-mail subjct=\"a\">x</rx-mail>").contains("did you mean \"subject\""));
    }

    #[test]
    fn mail_parts_call_the_mail_kit() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c("<rx-mail-button :url=\"link\" label=\"Pay\"/>"),
            format!("{MAIL_IMPORT}{{{{ __rx_mail.button(url=(link), label=\"Pay\") }}}}")
        );
        assert_eq!(
            c("<rx-mail-button :url=\"link\">{{ t('pay') }}</rx-mail-button>"),
            format!(
                "{MAIL_IMPORT}{{% set __rx_slot_1 %}}{{{{ t('pay') }}}}{{% endset %}}{{{{ __rx_mail.button(url=(link), label=__rx_slot_1) }}}}"
            )
        );
        assert_eq!(
            c("<rx-mail-panel>\nNote\n</rx-mail-panel>"),
            format!("{MAIL_IMPORT}{{% call __rx_mail.panel() %}}\nNote\n{{% endcall %}}")
        );
        assert_eq!(
            c("<rx-mail-table :rows=\"rows\" :head=\"['A', 'B']\" :total=\"['T', 1]\"/>"),
            format!(
                "{MAIL_IMPORT}{{{{ __rx_mail.table(rows=(rows), head=(['A', 'B']), total=(['T', 1])) }}}}"
            )
        );
        assert_eq!(
            c("<rx-mail-divider/>"),
            format!("{MAIL_IMPORT}{{{{ __rx_mail.divider() }}}}")
        );
        assert!(msg("<rx-mail-button label=\"x\"/>").contains("needs the attribute \"url\""));
        assert!(msg("<rx-mail-button :url=\"u\"/>").contains("needs the attribute \"label\""));
        assert!(
            msg("<rx-mail-button :url=\"u\" label=\"a\">b</rx-mail-button>").contains("not both")
        );
        assert!(msg("<rx-mail-table rows=\"r\"/>").contains("takes data"));
        assert!(msg("<rx-mail-table/>").contains("needs the attribute \"rows\""));
        assert!(msg("<rx-mail-table :rows=\"r\" :hed=\"h\"/>").contains("did you mean \"head\""));
        assert!(msg("<rx-mail-divider>x</rx-mail-divider>").contains("takes no content"));
        assert!(msg("<rx-mail-divider size=\"2\"/>").contains("It takes no attributes"));
        assert!(msg("<rx-mail-pannel/>").contains("did you mean <rx-mail-panel>"));
    }

    #[test]
    fn small_kit_macros_have_tags() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c("<rx-icon name=\"bike\"/><rx-icon name=\"lock\" size=\"16\" label=\"Private\"/>"),
            format!(
                "{UI_IMPORT}{{{{ __rx_ui.icon(name=\"bike\") }}}}{{{{ __rx_ui.icon(name=\"lock\", size=16, label=\"Private\") }}}}"
            )
        );
        assert_eq!(
            c("<rx-columns count=\"3\"><div>a</div></rx-columns>"),
            format!("{UI_IMPORT}{{% call __rx_ui.columns(count=3) %}}<div>a</div>{{% endcall %}}")
        );
        assert_eq!(
            c("<rx-thumbnail :src=\"p.image\" alt=\"{{ p.name }}\" href=\"/x\"/>"),
            format!(
                "{UI_IMPORT}{{{{ __rx_ui.thumbnail(src=(p.image), alt=(p.name), href=\"/x\") }}}}"
            )
        );
        assert_eq!(
            c("<rx-tab-panel id=\"t\" key=\"a\" selected>x</rx-tab-panel>"),
            format!(
                "{UI_IMPORT}{{% call __rx_ui.tab_panel(id=\"t\", key=\"a\", selected=true) %}}x{{% endcall %}}"
            )
        );
        assert_eq!(
            c("<rx-menu-button hx-get=\"{{ url }}\" danger icon=\"edit\">Edit</rx-menu-button>"),
            format!(
                "{UI_IMPORT}{{% set __rx_slot_1 %}}Edit{{% endset %}}{{{{ __rx_ui.menu_button(danger=true, icon=\"edit\", label=__rx_slot_1, attrs={{\"hx-get\": (url)}}) }}}}"
            )
        );
        assert_eq!(
            c(
                "<rx-action-group label=\"More\"><rx-menu-link href=\"/a\" label=\"A\"/></rx-action-group>"
            ),
            format!(
                "{UI_IMPORT}{{% call __rx_ui.action_group(label=\"More\") %}}{{{{ __rx_ui.menu_link(href=\"/a\", label=\"A\") }}}}{{% endcall %}}"
            )
        );
        assert_eq!(
            c("<rx-event-stream/>"),
            format!("{UI_IMPORT}{{{{ __rx_ui.event_stream() }}}}")
        );
        assert_eq!(
            c(
                "<rx-import-action id=\"imp\" label=\"Import\" route=\"products.import\" title=\"Import products\" :columns=\"['sku', 'name']\" template-url=\"/t\" :button=\"false\"/>"
            ),
            format!(
                "{UI_IMPORT}{{{{ __rx_ui.import_action(id=\"imp\", label=\"Import\", action=route(\"products.import\"), title=\"Import products\", columns=(['sku', 'name']), template_url=\"/t\", button=(false)) }}}}"
            )
        );
        assert!(msg("<rx-icon/>").contains("needs the attribute \"name\""));
        assert!(msg("<rx-icon name=\"a\" size=\"big\"/>").contains("takes a number"));
        assert!(msg("<rx-icon name=\"a\">x</rx-icon>").contains("takes no content"));
        assert!(msg("<rx-columns cuont=\"2\"/>").contains("did you mean \"count\""));
        assert!(msg("<rx-thumbnail alt=\"x\"/>").contains("needs the attribute \"src\""));
        assert!(msg("<rx-tab-panel id=\"t\"/>").contains("needs the attribute \"key\""));
        assert!(
            msg("<rx-menu-button :hx-get=\"u\">x</rx-menu-button>").contains("has no attribute")
        );
        assert!(
            msg("<rx-import-action id=\"i\" label=\"L\" title=\"T\"/>")
                .contains("needs the attribute \"action\"")
        );
        assert!(
            msg("<rx-import-action id=\"i\" label=\"L\" title=\"T\" columns=\"a\" route=\"x\"/>")
                .contains("takes data")
        );
        assert!(msg("<rx-event-stream x=\"1\"/>").contains("It takes no attributes"));
        assert!(msg("<rx-thumbnial :src=\"a\"/>").contains("did you mean <rx-thumbnail>"));
    }

    #[test]
    fn wizard_action_builds_the_steps_from_its_children() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c(
                "<rx-wizard-action id=\"np\" label=\"New\" route=\"products.store\" title=\"New product\" submit-label=\"Add\" variant=\"primary\" @saved=\"go\">\n<rx-wizard-step key=\"a\" title=\"One\">A</rx-wizard-step>\n<rx-wizard-step key=\"b\">B</rx-wizard-step>\n</rx-wizard-action>"
            ),
            format!(
                "{UI_IMPORT}{{% call __rx_ui.wizard_action(steps=[[\"a\", \"One\"], [\"b\", \"b\"]], id=\"np\", label=\"New\", action=route(\"products.store\"), title=\"New product\", submit_label=\"Add\", variant=\"primary\", attrs={{\"x-on:rx:wizard-action:saved.self\": \"go\"}}) %}}{{#\n#}}{{#\n#}}{{#\n#}}{{% call __rx_ui.wizard_step(id=\"np\", key=\"a\", title=\"One\") %}}A{{% endcall %}}{{% call __rx_ui.wizard_step(id=\"np\", key=\"b\") %}}B{{% endcall %}}{{% endcall %}}"
            )
        );
        assert_eq!(
            msg("<rx-wizard-action id=\"i\" label=\"L\" action=\"/a\" title=\"T\"/>"),
            "<rx-wizard-action> needs at least one <rx-wizard-step>"
        );
        assert_eq!(
            msg(
                "<rx-wizard-action id=\"i\" label=\"L\" action=\"/a\" title=\"T\">text</rx-wizard-action>"
            ),
            "<rx-wizard-action> holds only <rx-wizard-step>"
        );
        assert_eq!(
            msg(
                "<rx-wizard-action id=\"i\" label=\"L\" action=\"/a\" title=\"T\"><p>x</p></rx-wizard-action>"
            ),
            "<rx-wizard-action> holds only <rx-wizard-step>"
        );
        assert!(msg("<rx-wizard-action label=\"L\" action=\"/a\" title=\"T\"><rx-wizard-step key=\"a\">x</rx-wizard-step></rx-wizard-action>").contains("needs the attribute \"id\""));
        assert!(msg("<rx-wizard-action id=\"i\" label=\"L\" route=\"r\" action=\"/a\" title=\"T\"><rx-wizard-step key=\"a\">x</rx-wizard-step></rx-wizard-action>").contains("not both"));
        assert!(
            msg("<rx-wizard-step key=\"a\">x</rx-wizard-step>")
                .contains("belongs inside <rx-wizard>")
        );
    }

    // #517–#521: the tags that end the raw MiniJinja in the examples.

    #[test]
    fn include_names_the_template() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c("<rx-include template=\"parts/x.html\"/>"),
            "{% include \"parts/x.html\" %}"
        );
        assert_eq!(
            c("<rx-include :template=\"section.template\"/>"),
            "{% include (section.template) %}"
        );
        assert_eq!(
            c("<rx-include :template=\"'a/' ~ slug ~ '/cells.html'\" optional/>"),
            "{% include ('a/' ~ slug ~ '/cells.html') ignore missing %}"
        );
        assert_eq!(
            c("<rx-include template=\"x.html\" :optional=\"false\"/>"),
            "{% include \"x.html\" %}"
        );
        assert_eq!(
            msg("<rx-include/>"),
            "<rx-include> needs the attribute \"template\""
        );
        assert_eq!(
            msg("<rx-include template=\"x\" :optional=\"a\"/>"),
            "<rx-include> \"optional\" is true or false: write optional"
        );
        assert_eq!(
            msg("<rx-include template=\"x\">y</rx-include>"),
            "<rx-include> takes no content"
        );
        assert!(
            msg("<rx-include templat=\"x\"/>").contains("did you mean \"template\""),
            "{}",
            msg("<rx-include templat=\"x\"/>")
        );
    }

    #[test]
    fn conditional_attributes_on_elements_and_components() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err();
        // A plain element: the attribute is replaced where it stood.
        assert_eq!(
            c("<a href=\"/x\" ?aria-current=\"'page' if on\" class=\"y\">x</a>"),
            "<a href=\"/x\"{{ rx_attr(\"aria-current\", ('page' if on)) }} class=\"y\">x</a>"
        );
        // Without any component tag the template is still compiled.
        assert!(c("<li ?data-done=\"step.done\">x</li>").contains("rx_attr(\"data-done\""));
        // Together with rx-if on the same element.
        assert_eq!(
            c("<p rx-if=\"a\" ?hidden=\"b\">x</p>"),
            "{% if a %}<p{{ rx_attr(\"hidden\", (b)) }}>x</p>{% endif %}"
        );
        // A component drawn as an element.
        assert_eq!(
            c("<rx-stack ?hx-swap-oob=\"'true' if oob\">x</rx-stack>"),
            "<div class=\"rx-stack\"{{ rx_attr(\"hx-swap-oob\", ('true' if oob)) }}>x</div>"
        );
        // A macro component: a pair of `attrs`, left out by the kit when it is none.
        assert_eq!(
            c("<rx-input name=\"a\" label=\"A\" ?data-x=\"on\"/>"),
            format!(
                "{UI_IMPORT}{{{{ __rx_ui.input(name=\"a\", label=\"A\", attrs={{\"data-x\": rx_attr_value((on))}}) }}}}"
            )
        );
        let e = msg("<a ?href=\"\">x</a>");
        assert_eq!(
            e.message,
            "<a> \"?href\" needs an expression: ?href=\"expr\""
        );
        let e = msg("<a rx-if=\"a\" ?x@y=\"z\">x</a>");
        assert!(e.message.contains("needs an attribute name"), "{e:?}");
    }

    #[test]
    fn tabs_from_data() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c(
                "<rx-tabs id=\"code\" :items=\"tabs\" label=\"Code\">\n<rx-tab-panel rx-for=\"s in samples\" id=\"code\" :key=\"loop.index0 | string\" :selected=\"loop.first\">x</rx-tab-panel>\n</rx-tabs>"
            ),
            format!(
                "{UI_IMPORT}{{{{ __rx_ui.tabs(id=\"code\", items=(tabs), label=\"Code\") }}}}\n{{% for s in samples %}}{{% call __rx_ui.tab_panel(id=\"code\", key=(loop.index0 | string), selected=(loop.first)) %}}x{{% endcall %}}{{% endfor %}}\n"
            )
        );
        assert_eq!(
            msg(
                "<rx-tabs id=\"t\" :items=\"x\"><rx-tab key=\"a\" label=\"A\">1</rx-tab></rx-tabs>"
            ),
            "<rx-tabs> takes :items or <rx-tab> children, not both"
        );
        assert_eq!(
            msg("<rx-tabs id=\"t\" items=\"x\"/>"),
            "<rx-tabs> \"items\" takes data: write :items=\"…\""
        );
    }

    #[test]
    fn link_tabs_from_children() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c(
                "<rx-link-tabs label=\"Views\"><rx-link-tab rx-for=\"k in keys\" :href=\"'/?v=' ~ k\" :current=\"k == cur\">{{ k }}</rx-link-tab></rx-link-tabs>"
            ),
            "<nav class=\"rx-link-tabs\" aria-label=\"{{ (\"Views\") or t('ui.main_navigation') }}\"><div class=\"rx-segmented\">{% for k in keys %}<a class=\"rx-segmented__item\" href=\"{{ ('/?v=' ~ k) }}\"{% if (k == cur) %} aria-current=\"page\"{% endif %}>{{ k }}</a>{% endfor %}</div></nav>"
        );
        assert_eq!(
            c("<rx-link-tabs><rx-link-tab href=\"/a\" label=\"A\" current/></rx-link-tabs>"),
            "<nav class=\"rx-link-tabs\" aria-label=\"{{ t('ui.main_navigation') }}\"><div class=\"rx-segmented\"><a class=\"rx-segmented__item\" href=\"{{ \"/a\" }}\" aria-current=\"page\">{{ \"A\" }}</a></div></nav>"
        );
        // The data form is the kit's macro, as before.
        assert_eq!(
            c("<rx-link-tabs :items=\"t\" :current=\"cur\"/>"),
            format!("{UI_IMPORT}{{{{ __rx_ui.link_tabs(items=(t), current=(cur)) }}}}")
        );
        assert_eq!(
            msg("<rx-link-tabs/>"),
            "<rx-link-tabs> needs :items or <rx-link-tab> children"
        );
        assert_eq!(
            msg("<rx-link-tabs :items=\"t\"><rx-link-tab href=\"/\" label=\"A\"/></rx-link-tabs>"),
            "<rx-link-tabs> takes :items or <rx-link-tab> children, not both"
        );
        assert_eq!(
            msg("<rx-link-tabs><p>x</p></rx-link-tabs>"),
            "<rx-link-tabs> holds only <rx-link-tab>"
        );
        assert_eq!(
            msg("<rx-link-tab href=\"/\" label=\"A\"/>"),
            "<rx-link-tab> belongs inside <rx-link-tabs>"
        );
        assert_eq!(
            msg("<rx-link-tabs><rx-link-tab href=\"/\"/></rx-link-tabs>"),
            "<rx-link-tab> needs a label"
        );
        assert_eq!(
            msg("<rx-link-tabs><rx-link-tab href=\"/\" label=\"A\">B</rx-link-tab></rx-link-tabs>"),
            "<rx-link-tab> give \"label\" or content, not both"
        );
        assert!(
            msg("<rx-link-tabs><rx-link-tab hre=\"/\" label=\"A\"/></rx-link-tabs>")
                .contains("did you mean \"href\"")
        );
    }

    #[test]
    fn tables_by_hand_and_footers() {
        let c = |s: &str| compile("a.html", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.html", s, &builtin()).unwrap_err().message;
        assert_eq!(
            c(
                "<rx-simple-table :head=\"['A', ['B', 'num']]\" caption=\"Totals\"><tr rx-for=\"r in rows\"><td>{{ r }}</td></tr><rx-slot name=\"footer\"><tr><th>T</th></tr></rx-slot></rx-simple-table>"
            ),
            format!(
                "{UI_IMPORT}{{% set __rx_slot_1 %}}<tr><th>T</th></tr>{{% endset %}}{{% call __rx_ui.table(head=(['A', ['B', 'num']]), caption=\"Totals\", foot=__rx_slot_1) %}}{{% for r in rows %}}<tr><td>{{{{ r }}}}</td></tr>{{% endfor %}}{{% endcall %}}"
            )
        );
        let out = c(
            "<rx-table :rows=\"xs\" as=\"x\"><rx-column label=\"N\">{{ x }}</rx-column><rx-slot name=\"footer\"><tr><th>T</th></tr></rx-slot></rx-table>",
        );
        assert!(
            out.contains("{% set __rx_foot_1 %}<tr><th>T</th></tr>{% endset %}{% call __rx_ui.table(head=[\"N\"], foot=__rx_foot_1) %}"),
            "{out}"
        );
        assert_eq!(
            msg(
                "<rx-table :rows=\"xs\"><rx-column label=\"N\">1</rx-column><rx-slot name=\"foot\">x</rx-slot></rx-table>"
            ),
            "<rx-table> holds <rx-column>, <rx-row-actions>, <rx-slot name=\"empty\"> and <rx-slot name=\"footer\">; did you mean \"footer\"?"
        );
        assert_eq!(
            msg(
                "<rx-simple-table :head=\"h\"><rx-slot name=\"foter\">x</rx-slot></rx-simple-table>"
            ),
            "<rx-simple-table> has no slot \"foter\"; did you mean \"footer\"?"
        );
        assert_eq!(
            msg("<rx-simple-table><tr></tr></rx-simple-table>"),
            "<rx-simple-table> needs the attribute \"head\""
        );
    }

    #[test]
    fn mail_text_keeps_the_text_as_written() {
        let c = |s: &str| compile("a.txt", s, &builtin()).unwrap();
        let msg = |s: &str| compile("a.txt", s, &builtin()).unwrap_err().message;
        // The same text as `{{ t }}\n\n{% for r in rows %}\n{{ r }}{% endfor %}\n{% if u %}\n{{ u }}{% endif %}`.
        assert_eq!(
            c(
                "<rx-mail-text>\n{{ t }}\n\n<template rx-for=\"r in rows\">\n{{ r }}</template>\n<template rx-if=\"u\">\n{{ u }}</template>\n</rx-mail-text>\n"
            ),
            "{#\n#}{{ t }}\n\n{% for r in rows %}\n{{ r }}{% endfor %}\n{% if u %}\n{{ u }}{% endif %}{#\n#}\n"
        );
        assert_eq!(c("<rx-mail-text>one</rx-mail-text>"), "one");
        assert_eq!(
            msg("<p/><rx-mail-text>x</rx-mail-text>"),
            "<rx-mail-text> must hold the whole template"
        );
        assert_eq!(
            msg("<rx-stack><rx-mail-text>x</rx-mail-text></rx-stack>"),
            "<rx-mail-text> must hold the whole template"
        );
        assert!(msg("<rx-mail-text title=\"x\">x</rx-mail-text>").contains("has no attribute"));
    }
}
