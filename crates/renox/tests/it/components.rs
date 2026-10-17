//! Template components at load time: unknown tags fail with `file:line`, plain pages are untouched.

use renox::prelude::*;
use renox::testing::TestApp;

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/plain", || async { view("plain.html", context! {}) })
            .get("/stack", || async { view("stack.html", context! {}) })
            .get("/page", || async { view("page.html", context! {}) })
    }
}

async fn app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write(
        "plain.html",
        "<h1 class=\"rx-title\">Plain {{ 1 + 1 }}</h1>",
    );
    write("stack.html", "<rx-stack>\n<p>in</p>\n</rx-stack>");
    write("page.html", "<h1>Page</h1>\n<rx-nope>x</rx-nope>\n");
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pages), move |c| c.views_path = path).await;
    (app, dir)
}

#[renox::test]
async fn an_unknown_component_names_file_and_line() {
    let (app, _dir) = app().await;
    let res = app.get("/page").await;
    res.assert_status(500);
    assert!(
        res.text()
            .contains("page.html:2: unknown component &lt;rx-nope&gt;"),
        "{}",
        res.text()
    );
}

#[renox::test]
async fn pages_without_components_render_as_before() {
    let (app, _dir) = app().await;
    app.get("/plain")
        .await
        .assert_ok()
        .assert_see("<h1 class=\"rx-title\">Plain 2</h1>");
}

#[renox::test]
async fn rx_stack_renders_a_div() {
    let (app, _dir) = app().await;
    app.get("/stack")
        .await
        .assert_ok()
        .assert_see("<div class=\"rx-stack\">\n<p>in</p>\n</div>");
}

/// Routes `/m` and `/r` render the two templates with the same data.
struct Pair(serde_json::Value);

impl Module for Pair {
    fn name(&self) -> &'static str {
        "pair"
    }

    fn routes(&self) -> Routes {
        let (a, b) = (self.0.clone(), self.0.clone());
        Routes::new()
            .get("/m", move || {
                let ctx = a.clone();
                async move { view("m.html", ctx) }
            })
            .get("/r", move || {
                let ctx = b.clone();
                async move { view("r.html", ctx) }
            })
    }
}

/// Collapses whitespace runs and drops whitespace between tags.
fn normalize(html: &str) -> String {
    let collapsed = html.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.replace("> <", "><")
}

/// A component page renders the same HTML as the macro calls it stands for.
async fn same(macro_src: &str, rx_src: &str, ctx: serde_json::Value) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("m.html"), macro_src).unwrap();
    std::fs::write(dir.path().join("r.html"), rx_src).unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(Pair(ctx)), move |c| c.views_path = path).await;
    let (m, r) = (app.get("/m").await, app.get("/r").await);
    m.assert_ok();
    r.assert_ok();
    let (m, r) = (normalize(&m.text()), normalize(&r.text()));
    assert!(!m.is_empty());
    assert_eq!(m, r);
}

const UI: &str = "{% import \"renox/ui.html\" as ui %}";

#[renox::test]
async fn rx_card_matches_the_macro() {
    same(
        &format!("{UI}{{% call ui.card(title=\"Contact\", subtitle=(sub)) %}}<p>body</p>{{% endcall %}}{{% call ui.card() %}}bare{{% endcall %}}"),
        "<rx-card title=\"Contact\" subtitle=\"{{ sub }}\"><p>body</p></rx-card><rx-card>bare</rx-card>",
        serde_json::json!({"sub": "Reach us"}),
    )
    .await;
}

#[renox::test]
async fn rx_toolbar_matches_the_macro() {
    same(
        &format!("{UI}{{% call ui.toolbar() %}}<a href=\"/x\">x</a>{{% endcall %}}"),
        "<rx-toolbar><a href=\"/x\">x</a></rx-toolbar>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_badge_matches_the_macro() {
    same(
        &format!(
            "{UI}{{{{ ui.badge(text=\"New\") }}}}{{{{ ui.badge(text=(label), kind=\"success\") }}}}"
        ),
        "<rx-badge text=\"New\"/><rx-badge text=\"{{ label }}\" kind=\"success\"/>",
        serde_json::json!({"label": "Paid"}),
    )
    .await;
}

#[renox::test]
async fn rx_badge_content_matches_the_prop() {
    same(
        &format!("{UI}{{{{ ui.badge(text=\"New\") }}}}"),
        "<rx-badge>New</rx-badge>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_page_header_matches_the_macro_with_and_without_actions() {
    same(
        &format!(
            "{UI}{{{{ ui.page_header(title=\"Orders\", subtitle=\"All\", back=\"/\", badge=\"3\", badge_kind=\"info\") }}}}\
             {{% call ui.page_header(title=(name)) %}}<a href=\"/new\">New</a>{{% endcall %}}"
        ),
        "<rx-page-header title=\"Orders\" subtitle=\"All\" back=\"/\" badge=\"3\" badge-kind=\"info\"> </rx-page-header>\
         <rx-page-header title=\"{{ name }}\"><a href=\"/new\">New</a></rx-page-header>",
        serde_json::json!({"name": "Items"}),
    )
    .await;
}

#[renox::test]
async fn control_flow_renders_lists_and_empty_lists() {
    let src =
        "<ul><li rx-for=\"x in xs\">{{ x }}</li></ul><p rx-if=\"xs\">some</p><p rx-else>none</p>";
    let mac = "<ul>{% for x in xs %}<li>{{ x }}</li>{% endfor %}</ul>{% if xs %}<p>some</p>{% else %}<p>none</p>{% endif %}";
    same(mac, src, serde_json::json!({"xs": ["a", "b"]})).await;
    same(mac, src, serde_json::json!({"xs": []})).await;
    let badge = "{% for x in xs %}{{ ui.badge(text=(x)) }}{% endfor %}";
    same(
        &format!("{UI}{badge}"),
        "<rx-badge rx-for=\"x in xs\" :text=\"x\"/>",
        serde_json::json!({"xs": ["a", "b"]}),
    )
    .await;
}

struct One;

impl Module for One {
    fn name(&self) -> &'static str {
        "one"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/p", || async { view("p.html", context! {}) })
    }
}

/// Writes the files (paths relative to the views directory) and gets `/p`.
async fn render_files(files: &[(&str, &str)]) -> renox::testing::TestResponse {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(One), move |c| c.views_path = path).await;
    app.get("/p").await
}

const PRICE_TAG: (&str, &str) = (
    "components/price_tag.html",
    "<rx-props amount currency=\"USD\">\n<b>{{ amount }} {{ currency }}</b><rx-slot />",
);

#[renox::test]
async fn an_app_component_renders_with_defaults_and_content() {
    let res = render_files(&[
        PRICE_TAG,
        ("p.html", "<app-price-tag :amount=\"3\">!</app-price-tag>"),
    ])
    .await;
    res.assert_ok();
    assert_eq!(normalize(&res.text()), "<b>3 USD</b>!");
    let res = render_files(&[
        PRICE_TAG,
        ("p.html", "<app-price-tag :amount=\"3\" currency=\"EUR\"/>"),
    ])
    .await;
    assert_eq!(normalize(&res.text()), "<b>3 EUR</b>");
}

#[renox::test]
async fn an_app_component_with_a_missing_required_prop_fails() {
    let res = render_files(&[PRICE_TAG, ("p.html", "\n<app-price-tag/>")]).await;
    res.assert_status(500);
    assert!(
        res.text()
            .contains("p.html:2: &lt;app-price-tag&gt; needs the attribute &quot;amount&quot;"),
        "{}",
        res.text()
    );
}

#[renox::test]
async fn an_unknown_prop_of_an_app_component_gets_a_suggestion() {
    let res = render_files(&[
        PRICE_TAG,
        ("p.html", "<app-price-tag :amount=\"1\" curency=\"EUR\"/>"),
    ])
    .await;
    res.assert_status(500);
    assert!(
        res.text()
            .contains("has no attribute &quot;curency&quot;; did you mean &quot;currency&quot;?"),
        "{}",
        res.text()
    );
}

#[renox::test]
async fn an_app_component_takes_named_slots() {
    let res = render_files(&[
        (
            "components/panel.html",
            "<rx-props title=\"T\">\n<h2>{{ title }}</h2><rx-slot /><footer><rx-slot name=\"foot\" /></footer>",
        ),
        (
            "p.html",
            "<app-panel title=\"Hi\">body<rx-slot name=\"foot\">bye</rx-slot></app-panel>",
        ),
    ])
    .await;
    res.assert_ok();
    assert_eq!(
        normalize(&res.text()),
        "<h2>Hi</h2>body<footer>bye</footer>"
    );
}

#[renox::test]
async fn a_macro_file_used_as_a_component_says_what_to_do() {
    let res = render_files(&[
        ("components/legacy.html", "{% macro a() %}x{% endmacro %}"),
        ("p.html", "<app-legacy/>"),
    ])
    .await;
    res.assert_status(500);
    assert!(
        res.text()
            .contains("components/legacy.html has no &lt;rx-props&gt;: it holds macros"),
        "{}",
        res.text()
    );
    let res = render_files(&[("p.html", "<app-absent/>")]).await;
    assert!(
        res.text()
            .contains("&lt;app-absent&gt; needs resources/views/components/absent.html"),
        "{}",
        res.text()
    );
}

#[renox::test]
async fn app_components_can_use_kit_components() {
    let res = render_files(&[
        (
            "components/note.html",
            "<rx-props text>\n<rx-badge :text=\"text\"/>",
        ),
        ("p.html", "<app-note text=\"Hello\"/>"),
    ])
    .await;
    res.assert_ok();
    assert!(res.text().contains("Hello"), "{}", res.text());
}

#[renox::test]
async fn rx_page_and_rx_push_match_extends_and_push() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write(
        "layout.html",
        "<head>{% block seo %}<title>none</title>{% endblock %}</head><main>{% block content %}{% endblock %}</main>[{% block scripts %}{% endblock %}]{{ stack('js') }}",
    );
    write(
        "m.html",
        "{% extends \"layout.html\" %}{% block seo %}{{ seo(title=(name)) }}{% endblock %}{% block content %}<p>{{ name }}</p>\
         {% call push(\"js\", once=\"a\") %}<i>a</i>{% endcall %}{% call push(\"js\", once=\"a\") %}<i>a</i>{% endcall %}\
         {% endblock %}{% block scripts %}S{% endblock %}",
    );
    write(
        "r.html",
        "<rx-page layout=\"layout.html\" :title=\"name\">\n<p>{{ name }}</p>\n\
         <rx-push stack=\"js\" once=\"a\"><i>a</i></rx-push><rx-push stack=\"js\" once=\"a\"><i>a</i></rx-push>\n\
         <rx-slot name=\"scripts\">S</rx-slot></rx-page>",
    );
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new().module(Pair(serde_json::json!({"name": "Items"}))),
        move |c| c.views_path = path,
    )
    .await;
    let (m, r) = (app.get("/m").await, app.get("/r").await);
    m.assert_ok();
    r.assert_ok();
    let (m, r) = (normalize(&m.text()), normalize(&r.text()));
    assert_eq!(m.replace("/m\"", "/r\""), r);
    assert!(r.contains("<title>Items"), "{r}");
    assert_eq!(r.matches("<i>a</i>").count(), 1, "{r}");
    assert!(r.contains("[S]"), "{r}");
}

#[renox::test]
async fn layout_tags_match_the_template_plumbing() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    // The same layout twice: in tags, and as raw template code.
    write(
        "tags.html",
        "<html><head><rx-head/><rx-ui/></head><body><rx-outlet name=\"content\"/><rx-outlet name=\"extra\"><i>default</i></rx-outlet><rx-outlet stack=\"js\"/><rx-toasts/></body></html>",
    );
    write(
        "raw.html",
        "<html><head>{{ renox_head() }}{{ renox_ui() }}</head><body>{% block content %}{% endblock %}{% block extra %}<i>default</i>{% endblock %}{{ stack('js') }}{{ toasts() }}</body></html>",
    );
    let body = "<rx-push stack=\"js\" once=\"a\"><b>js</b></rx-push><p>{{ name }}</p>\
                <rx-pagination :page=\"page\"/><rx-pagination :page=\"simple\"/>";
    write(
        "m.html",
        "{% extends \"raw.html\" %}{% block content %}{% call push(\"js\", once=\"a\") %}<b>js</b>{% endcall %}<p>{{ name }}</p>\
         {% from \"renox/pagination.html\" import pagination, simple_pagination %}{{ pagination(page) }}{{ simple_pagination(simple) }}{% endblock %}",
    );
    write(
        "r.html",
        &format!("<rx-page layout=\"tags.html\">{body}</rx-page>"),
    );
    let ctx = serde_json::json!({
        "name": "Items",
        "page": {"page": 1, "last_page": 3, "has_prev": false, "has_next": true, "pages": [1, 2, 3]},
        "simple": {"page": 2, "has_prev": true, "has_next": true},
    });
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(Pair(ctx)), move |c| c.views_path = path).await;
    let (m, r) = (app.get("/m").await, app.get("/r").await);
    m.assert_ok();
    r.assert_ok();
    let (m, r) = (normalize(&m.text()), normalize(&r.text()));
    assert_eq!(m, r);
    assert!(r.contains("<i>default</i>"), "{r}");
    assert!(r.contains("<b>js</b>"), "{r}");
    assert!(r.contains("rel=\"next\""), "{r}");
}

/// #508: groups and text under `rx-if` / `rx-elif` / `rx-else` / `rx-for`, with no wrapper.
#[renox::test]
async fn template_groups_and_elif_match_the_template_statements() {
    let tags = "<template rx-if=\"n == 1\">one <b>text</b> and <i>markup</i></template>\
                <template rx-elif=\"n == 2\">two</template>\
                <p rx-elif=\"n == 3\">three</p>\
                <template rx-else>many</template>|\
                <ul><template rx-for=\"x in xs\"><li>{{ loop.index }}/{{ loop.length }} {{ x }}{% if loop.first %} first{% endif %}{% if loop.last %} last{% endif %}</li>, </template></ul>|\
                <rx-card><template rx-for=\"x in xs\">{{ loop.index }}={{ x }};</template></rx-card>|\
                <rx-badge rx-if=\"n == 1\" text=\"one\"/><rx-badge rx-elif=\"n == 2\" text=\"two\"/><rx-badge rx-else text=\"more\"/>|\
                <template rx-if=\"xs\">has</template><template rx-else>none</template>";
    let raw = format!(
        "{UI}{{% if n == 1 %}}one <b>text</b> and <i>markup</i>{{% elif n == 2 %}}two{{% elif n == 3 %}}<p>three</p>{{% else %}}many{{% endif %}}|\
         <ul>{{% for x in xs %}}<li>{{{{ loop.index }}}}/{{{{ loop.length }}}} {{{{ x }}}}{{% if loop.first %}} first{{% endif %}}{{% if loop.last %}} last{{% endif %}}</li>, {{% endfor %}}</ul>|\
         {{% call ui.card() %}}{{% for x in xs %}}{{{{ loop.index }}}}={{{{ x }}}};{{% endfor %}}{{% endcall %}}|\
         {{% if n == 1 %}}{{{{ ui.badge(text=\"one\") }}}}{{% elif n == 2 %}}{{{{ ui.badge(text=\"two\") }}}}{{% else %}}{{{{ ui.badge(text=\"more\") }}}}{{% endif %}}|\
         {{% if xs %}}has{{% else %}}none{{% endif %}}"
    );
    for n in 1..=4 {
        same(
            &raw,
            tags,
            serde_json::json!({"n": n, "xs": ["a", "b", "c"]}),
        )
        .await;
    }
    same(&raw, tags, serde_json::json!({"n": 9, "xs": []})).await;
}

/// #511: `rx-let` is a scoped `{% with %}`.
#[renox::test]
async fn rx_let_matches_with_and_does_not_leak() {
    let tags = "<rx-let name=\"top\" :value=\"n * 2\"/><p>{{ top }}</p>\
                <div><rx-let name=\"label\" value=\"Item {{ n }}\"/><span>{{ label }}</span>\
                <rx-let name=\"twice\" :value=\"label ~ label\"/><b>{{ twice }}</b></div>\
                <p rx-if=\"n\"><rx-let name=\"inner\" :value=\"n + 1\"/>{{ inner }}</p>\
                <ul><li rx-for=\"x in xs\"><rx-let name=\"shown\" :value=\"x | upper\"/>{{ loop.index }}{{ shown }}</li></ul>\
                <rx-card><rx-let name=\"in_card\" :value=\"n\"/>{{ in_card }}<template rx-for=\"x in xs\">{{ in_card }}{{ x }}</template></rx-card>\
                [{% if label is defined or inner is defined or shown is defined or in_card is defined %}leak{% endif %}]";
    let raw = format!(
        "{UI}{{% with top = (n * 2) %}}<p>{{{{ top }}}}</p>\
         <div>{{% with label = \"Item \" ~ (n) %}}<span>{{{{ label }}}}</span>\
         {{% with twice = (label ~ label) %}}<b>{{{{ twice }}}}</b>{{% endwith %}}{{% endwith %}}</div>\
         {{% if n %}}<p>{{% with inner = (n + 1) %}}{{{{ inner }}}}{{% endwith %}}</p>{{% endif %}}\
         <ul>{{% for x in xs %}}<li>{{% with shown = (x | upper) %}}{{{{ loop.index }}}}{{{{ shown }}}}{{% endwith %}}</li>{{% endfor %}}</ul>\
         {{% call ui.card() %}}{{% with in_card = (n) %}}{{{{ in_card }}}}{{% for x in xs %}}{{{{ in_card }}}}{{{{ x }}}}{{% endfor %}}{{% endwith %}}{{% endcall %}}\
         [{{% if label is defined or inner is defined or shown is defined or in_card is defined %}}leak{{% endif %}}]\
         {{% endwith %}}"
    );
    same(&raw, tags, serde_json::json!({"n": 3, "xs": ["a", "b"]})).await;
    // Nothing leaks out of its element (the `[…]` at the end stays empty).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("m.html"), "unused").unwrap();
    std::fs::write(dir.path().join("r.html"), tags).unwrap();
    let path = dir.path().to_path_buf();
    let ctx = serde_json::json!({"n": 3, "xs": ["a", "b"]});
    let app =
        TestApp::with_config(App::new().module(Pair(ctx)), move |c| c.views_path = path).await;
    let page = app.get("/r").await;
    page.assert_ok()
        .assert_see("<p>6</p>")
        .assert_see("Item 3Item 3");
    assert!(!page.text().contains("leak"), "{}", page.text());
}

/// #512: `rx-script` and `rx-style` use the nonce of the page's CSP header.
#[renox::test]
async fn rx_script_and_rx_style_use_the_pages_nonce_under_strict_csp() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write(
        "layout.html",
        "<html><head><rx-outlet stack=\"head\"/></head><body><rx-block name=\"content\"/><rx-outlet stack=\"scripts\"/></body></html>",
    );
    write(
        "p.html",
        "<rx-page layout=\"layout.html\">\
         <rx-style>.a > .b { color: red }</rx-style>\
         <script nonce=\"{{ csp_nonce() }}\">var by_hand = 1;</script>\
         <rx-script>if (1 < 2 && \"x\" > \"a\") { window.inline = {{ 3 + 4 }}; }</rx-script>\
         <rx-push stack=\"scripts\" once=\"k\"><rx-script type=\"module\">window.pushed = 1;</rx-script></rx-push>\
         <rx-push stack=\"head\"><rx-style id=\"pushed\">.p { margin: 0 }</rx-style></rx-push>\
         </rx-page>",
    );
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(One), move |c| {
        c.views_path = path;
        c.csp = renox::CspMode::Strict;
    })
    .await;
    let res = app.get("/p").await;
    res.assert_ok();
    let csp = res.header("content-security-policy").expect("a CSP header");
    let nonce = csp
        .split("'nonce-")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .expect("a nonce in the policy")
        .to_owned();
    assert!(!nonce.is_empty());
    let html = res.text();
    for tag in [
        format!("<style nonce=\"{nonce}\">.a > .b {{ color: red }}</style>"),
        format!("<script nonce=\"{nonce}\">var by_hand = 1;</script>"),
        format!(
            "<script nonce=\"{nonce}\">if (1 < 2 && \"x\" > \"a\") {{ window.inline = 7; }}</script>"
        ),
        format!("<script nonce=\"{nonce}\" type=\"module\">window.pushed = 1;</script>"),
        format!("<style nonce=\"{nonce}\" id=\"pushed\">.p {{ margin: 0 }}</style>"),
    ] {
        assert!(html.contains(&tag), "{tag}\n{html}");
    }
    assert_eq!(html.matches("<script nonce=").count(), 3, "{html}");
}

/// #517: `view:check` reports a literal `<rx-include>` of a template that is not there, unless it
/// is `optional` or its name is computed.
#[renox::test]
async fn view_check_reports_an_include_of_a_missing_template() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write("part.html", "<i>part</i>");
    write(
        "fine.html",
        "<rx-include template=\"part.html\"/><rx-include template=\"gone.html\" optional/><rx-include :template=\"name\"/>",
    );
    write("bad.html", "<p>a</p>\n<rx-include template=\"prt.html\"/>");
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new(), move |c| c.views_path = path).await;
    let message = view_check_problems(&app);
    assert!(message.starts_with("1 template problem(s)"), "{message}");
    assert!(
        message.contains("bad.html:2: <rx-include> (or include) names a template that does not exist: \"prt.html\"; did you mean \"part.html\"?"),
        "{message}"
    );
}

/// `view:check` compiles every template: the new tags' mistakes are reported with file and line.
#[renox::test]
async fn view_check_reports_mistakes_in_the_flow_and_scope_tags() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write(
        "fine.html",
        "<template rx-if=\"a\">x</template><template rx-else>y</template><rx-let name=\"v\" :value=\"1\"/>{{ v }}<rx-script>1</rx-script>",
    );
    write(
        "elif.html",
        "<p>a</p>\n\n<template rx-elif=\"x\">b</template>",
    );
    write(
        "else.html",
        "<p rx-if=\"a\">a</p>\n<p rx-else>b</p>\n<p rx-else>c</p>",
    );
    write(
        "let.html",
        "<div rx-for=\"s in xs\">\n<rx-let name=\"s\" :value=\"1\"/></div>",
    );
    write(
        "let2.html",
        "<rx-let name=\"a\" :value=\"1\"/>\n<rx-let name=\"a\" :value=\"2\"/>",
    );
    write("nonce.html", "\n<rx-script nonce=\"x\"></rx-script>");
    write("tpl.html", "<template rx-if=\"a\" class=\"x\">a</template>");
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new(), move |c| c.views_path = path).await;
    let message = view_check_problems(&app);
    assert!(message.starts_with("6 template problem(s)"), "{message}");
    for expected in [
        "elif.html:3: rx-elif must follow an element with rx-if",
        "else.html:3: rx-else must follow an element with rx-if",
        "let.html:2: <rx-let> \"s\" is already taken here",
        "let2.html:2: <rx-let> \"a\" is already taken here",
        "nonce.html:2: <rx-script> sets the nonce itself",
        "tpl.html:1: <template> with rx-if, rx-elif, rx-else or rx-for takes no other attribute",
    ] {
        assert!(message.contains(expected), "{expected}\n{message}");
    }
}

#[renox::test]
async fn a_page_block_renders_as_a_fragment() {
    struct Frag;
    impl Module for Frag {
        fn name(&self) -> &'static str {
            "frag"
        }
        fn routes(&self) -> Routes {
            Routes::new().get("/f", || async {
                view("r.html", context! {}).fragment("content")
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("layout.html"),
        "<html>{% block content %}{% endblock %}</html>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-page layout=\"layout.html\"><p>inner</p></rx-page>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Frag), move |c| c.views_path = path).await;
    let res = app.htmx().get("/f").await;
    res.assert_ok().assert_see("<p>inner</p>");
    assert!(!res.text().contains("<html>"), "{}", res.text());
}

#[renox::test]
async fn rx_wizard_matches_the_macro() {
    same(
        &format!("{UI}{{% call ui.wizard(id=\"w\", steps=[[\"a\", \"First\"], [\"b\", (second)]], submit_label=\"Save\", cancel=true, back_label=\"Prev\") %}}{{% call ui.wizard_step(id=\"w\", key=\"a\", title=\"First\") %}}<p>one</p>{{% endcall %}}{{% call ui.wizard_step(id=\"w\", key=\"b\", title=(second)) %}}<p>two</p>{{% endcall %}}{{% endcall %}}"),
        "<rx-wizard id=\"w\" submit-label=\"Save\" cancel back-label=\"Prev\">\n<rx-wizard-step key=\"a\" title=\"First\"><p>one</p></rx-wizard-step>\n<rx-wizard-step key=\"b\" title=\"{{ second }}\"><p>two</p></rx-wizard-step>\n</rx-wizard>",
        serde_json::json!({"second": "Second"}),
    )
    .await;
}

#[renox::test]
async fn rx_wizard_holds_only_steps() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        "<rx-wizard id=\"w\" submit-label=\"Save\"><p>x</p></rx-wizard>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-wizard-step key=\"a\">x</rx-wizard-step>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pair(serde_json::json!({}))), move |c| {
        c.views_path = path
    })
    .await;
    let m = app.get("/m").await.text();
    assert!(m.contains("holds only &lt;rx-wizard-step&gt;"), "{m}");
    let r = app.get("/r").await.text();
    assert!(r.contains("belongs inside &lt;rx-wizard&gt;"), "{r}");
}

#[renox::test]
async fn rx_tabs_match_the_macros() {
    same(
        &format!("{UI}{{{{ ui.tabs(id=\"t\", items=[[\"a\", \"First\"], [\"b\", (second)]], selected=\"b\", label=\"Sections\") }}}}{{% call ui.tab_panel(id=\"t\", key=\"a\", selected=false) %}}<p>one</p>{{% endcall %}}{{% call ui.tab_panel(id=\"t\", key=\"b\", selected=true) %}}<p>two</p>{{% endcall %}}"),
        "<rx-tabs id=\"t\" selected=\"b\" label=\"Sections\">\n<rx-tab key=\"a\" label=\"First\"><p>one</p></rx-tab>\n<rx-tab key=\"b\" :label=\"second\"><p>two</p></rx-tab>\n</rx-tabs>",
        serde_json::json!({"second": "Second"}),
    )
    .await;
    same(
        &format!("{UI}{{{{ ui.tabs(id=\"t\", items=[[\"a\", \"First\"]]) }}}}{{% call ui.tab_panel(id=\"t\", key=\"a\", selected=true) %}}x{{% endcall %}}"),
        "<rx-tabs id=\"t\"><rx-tab key=\"a\" label=\"First\">x</rx-tab></rx-tabs>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_tabs_hold_only_tabs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        "<rx-tabs id=\"t\"><p>x</p></rx-tabs>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-tab key=\"a\" label=\"A\">x</rx-tab>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pair(serde_json::json!({}))), move |c| {
        c.views_path = path
    })
    .await;
    let m = app.get("/m").await.text();
    assert!(m.contains("holds only &lt;rx-tab&gt;"), "{m}");
    let r = app.get("/r").await.text();
    assert!(r.contains("belongs inside &lt;rx-tabs&gt;"), "{r}");
}

#[renox::test]
async fn rx_button_matches_the_macro_and_passes_hx_attributes() {
    same(
        &format!(
            "{UI}{{{{ ui.button(label=\"Save\", variant=\"danger\", size=\"small\", icon=\"plus\", attrs={{\"hx-post\": \"/x\"}}) }}}}"
        ),
        "<rx-button variant=\"danger\" size=\"small\" icon=\"plus\" hx-post=\"/x\">Save</rx-button>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_link_button_and_icon_button_match_the_macros() {
    same(
        &format!(
            "{UI}{{{{ ui.link_button(href=\"/a\", label=\"Go\", variant=\"primary\", new_tab=true) }}}}\
             {{{{ ui.icon_button(icon=\"plus\", label=\"Add\", href=\"/b\", variant=\"primary\") }}}}\
             {{{{ ui.icon_button(icon=\"plus\", label=\"Add\", attrs={{\"hx-get\": \"/c\"}}) }}}}"
        ),
        "<rx-link-button href=\"/a\" variant=\"primary\" new-tab>Go</rx-link-button>\
         <rx-icon-button icon=\"plus\" label=\"Add\" href=\"/b\" variant=\"primary\"/>\
         <rx-icon-button icon=\"plus\" label=\"Add\" hx-get=\"/c\"/>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_confirm_matches_the_macro() {
    same(
        &format!(
            "{UI}{{{{ ui.confirm(id=\"del\", label=\"Delete\", action=\"/x/1\", title=\"Sure?\", message=\"It goes.\", fields={{\"a\": \"b\"}}) }}}}"
        ),
        "<rx-confirm id=\"del\" label=\"Delete\" action=\"/x/1\" title=\"Sure?\" :fields=\"{'a': 'b'}\">It goes.</rx-confirm>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_sheet_and_rx_open_button_match_the_macros() {
    same(
        &format!(
            "{UI}{{{{ ui.open_button(\"s1\", \"Filters\", variant=\"primary\", icon=\"plus\") }}}}\
             {{% call ui.sheet(\"s1\", \"Filters\", message=\"Narrow it\", slide_over=true, width=\"lg\") %}}<p>body</p>{{% endcall %}}"
        ),
        "<rx-open-button id=\"s1\" label=\"Filters\" variant=\"primary\" icon=\"plus\" />\
         <rx-sheet id=\"s1\" title=\"Filters\" message=\"Narrow it\" slide-over width=\"lg\"><p>body</p></rx-sheet>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_action_sheet_matches_the_macro() {
    same(
        &format!(
            "{UI}{{% call ui.action_sheet(\"as1\", \"Restock\", \"/stock/7\", \"Restock Coffee\", method=\"PUT\", danger=true) %}}<input name=\"q\">{{% endcall %}}"
        ),
        "<rx-action-sheet id=\"as1\" label=\"Restock\" action=\"/stock/7\" title=\"Restock Coffee\" method=\"PUT\" danger><input name=\"q\"></rx-action-sheet>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_notification_bell_matches_the_macro() {
    same(
        &format!("{UI}{{{{ ui.notification_bell(count=3) }}}}x"),
        "<rx-notification-bell :count=\"3\" />x",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_alert_and_rx_empty_match_the_macros() {
    same(
        &format!(
            "{UI}{{{{ ui.alert(message=\"Heads up\", kind=\"warning\", title=\"Note\") }}}}\
             {{{{ ui.empty(title=\"Nothing\", message=\"Add one\", action_href=\"/new\", action_label=\"New\", icon=\"plus\") }}}}"
        ),
        "<rx-alert kind=\"warning\" title=\"Note\">Heads up</rx-alert>\
         <rx-empty title=\"Nothing\" action-href=\"/new\" action-label=\"New\" icon=\"plus\">Add one</rx-empty>",
        serde_json::json!({}),
    )
    .await;
}

/// Serves `m.html` / `r.html` with `ctx`, plus a named route with a parameter.
struct Tables(serde_json::Value);

impl Module for Tables {
    fn name(&self) -> &'static str {
        "tables"
    }

    fn routes(&self) -> Routes {
        let (a, b) = (self.0.clone(), self.0.clone());
        Routes::new()
            .get("/m", move || {
                let ctx = a.clone();
                async move { view("m.html", ctx) }
            })
            .get("/r", move || {
                let ctx = b.clone();
                async move { view("r.html", ctx) }
            })
            .get("/items/{id}", || async { "item" })
            .name("items.show")
    }
}

async fn tables(rx_src: &str, ctx: serde_json::Value) -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("m.html"), "").unwrap();
    std::fs::write(dir.path().join("r.html"), rx_src).unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(Tables(ctx)), move |c| c.views_path = path).await;
    (app, dir)
}

#[renox::test]
async fn rx_table_over_a_list_matches_the_macro() {
    let ctx = serde_json::json!({"items": [
        {"id": 1, "name": "Ann", "qty": 3},
        {"id": 2, "name": "Bob", "qty": 4},
    ]});
    same(
        &format!(
            "{UI}<div class=\"rx-stack\" id=\"rx-table-1\">{{% if items %}}{{% call ui.table(head=[\"Name\", [\"Qty\", \"num\"], [\"Note\", \"hide-narrow\"], [\"Both\", \"num rx-hide-narrow\"], [\"\", \"num\"]], caption=\"People\") %}}\
             {{% for p in items %}}<tr><td>{{{{ p.name }}}}</td><td class=\"rx-num\">{{{{ p.qty }}}}</td>\
             <td class=\"rx-hide-narrow\">n</td><td class=\"rx-num rx-hide-narrow\">b</td>\
             <td class=\"rx-num\">{{% call ui.row_actions() %}}<a href=\"/x\">x</a>{{% endcall %}}</td></tr>{{% endfor %}}{{% endcall %}}{{% endif %}}</div>"
        ),
        "<rx-table :rows=\"items\" as=\"p\" caption=\"People\">\n\
         <rx-column label=\"Name\">{{ p.name }}</rx-column>\n\
         <rx-column label=\"Qty\" align=\"num\">{{ p.qty }}</rx-column>\n\
         <rx-column label=\"Note\" hide-narrow>n</rx-column>\n\
         <rx-column label=\"Both\" align=\"num\" hide-narrow>b</rx-column>\n\
         <rx-row-actions><a href=\"/x\">x</a></rx-row-actions>\n\
         </rx-table>",
        ctx,
    )
    .await;
}

#[renox::test]
async fn rx_table_pages_get_links_and_the_empty_slot_shows() {
    let items: Vec<serde_json::Value> = (11..=20).map(|i| serde_json::json!({"id": i})).collect();
    let page = Paginated::new(items, 2, 10, 25);
    let src = "<rx-table :rows=\"rows\" id=\"things\" card>\
               <rx-column label=\"Id\">{{ row.id }}</rx-column>\
               <rx-slot name=\"empty\"><p>Nothing yet</p></rx-slot></rx-table>";
    let (app, _dir) = tables(
        src,
        serde_json::json!({"rows": serde_json::to_value(&page).unwrap()}),
    )
    .await;
    let r = app.get("/r").await;
    r.assert_ok();
    let t = r.text();
    assert!(t.contains("id=\"things\""), "{t}");
    assert!(
        t.contains("<div class=\"rx-card\"><div class=\"rx-table-wrap\">"),
        "{t}"
    );
    assert!(t.contains("hx-target=\"#things\""), "{t}");
    assert!(t.contains("hx-select=\"#things\""), "{t}");
    assert!(t.contains("class=\"pagination\""), "{t}");
    assert!(t.contains("<td>20</td>"), "{t}");
    assert!(!t.contains("Nothing yet"), "{t}");

    let (app, _dir) = tables(src, serde_json::json!({"rows": []})).await;
    let t = app.get("/r").await.text();
    assert!(t.contains("<p>Nothing yet</p>"), "{t}");
    assert!(!t.contains("<table"), "{t}");
}

#[renox::test]
async fn rx_table_row_routes_and_abilities() {
    let ctx = serde_json::json!({"rows": [
        {"id": 7, "_can": {"edit": true}},
        {"id": 8, "_can": {"edit": false}},
    ]});
    let (app, _dir) = tables(
        "<rx-table :rows=\"rows\"><rx-column label=\"Id\">{{ row.id }}</rx-column>\
         <rx-row-actions><rx-link-button route=\"items.show\" can=\"edit\">Edit</rx-link-button></rx-row-actions></rx-table>",
        ctx,
    )
    .await;
    let t = app.get("/r").await.text();
    assert!(t.contains("href=\"/items/7\""), "{t}");
    assert!(!t.contains("/items/8"), "{t}");
}

#[renox::test]
async fn rx_table_holds_columns_only() {
    let (app, _dir) = tables(
        "<rx-table :rows=\"[]\"><p>no</p></rx-table>",
        serde_json::json!({}),
    )
    .await;
    let t = app.get("/r").await.text();
    assert!(
        t.contains("&lt;rx-table&gt; holds &lt;rx-column&gt;, &lt;rx-row-actions&gt;, &lt;rx-slot name=&quot;empty&quot;&gt; and &lt;rx-slot name=&quot;footer&quot;&gt;"),
        "{t}"
    );
    let (app, _dir) = tables(
        "<rx-column label=\"x\">y</rx-column>",
        serde_json::json!({}),
    )
    .await;
    let t = app.get("/r").await.text();
    assert!(t.contains("belongs inside &lt;rx-table&gt;"), "{t}");
}

#[derive(serde::Deserialize)]
struct Fields {
    name: String,
    bio: Option<String>,
    size: Option<String>,
    agree: Option<String>,
}

impl Validate for Fields {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required().max(10);
        v.field("bio", &self.bio).max(5);
        v.field("size", &self.size).required();
        let _ = &self.agree;
    }
}

struct FormPages;

impl Module for FormPages {
    fn name(&self) -> &'static str {
        "form-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/m", || async { view("m.html", context! {}) })
            .get("/r", || async { view("r.html", context! {}) })
            .post("/save", |Valid(_): Valid<Fields>| async { "ok" })
    }
}

#[renox::test]
async fn form_fields_match_the_macros_after_a_failed_submit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        format!(
            "{UI}{{{{ ui.form_errors(title=\"Fix these\") }}}}\
             {{% call ui.form_grid(columns=3) %}}\
             {{{{ ui.input(name=\"name\", label=\"Name\", type=\"text\", hint=\"Short\", required=true, span=\"full\", attrs={{\"maxlength\": \"20\", \"data-x\": \"1\"}}) }}}}\
             {{{{ ui.textarea(name=\"bio\", label=\"Bio\", rows=2, value=\"hi\") }}}}\
             {{% endcall %}}\
             {{{{ ui.select(name=\"size\", label=\"Size\", options=[[\"s\", \"Small\"], [\"m\", \"Medium\"]], required=true) }}}}\
             {{{{ ui.checkbox(name=\"agree\", label=\"Agree\", switch=true) }}}}"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-form-errors title=\"Fix these\"/>\
         <rx-form-grid columns=\"3\">\
         <rx-input name=\"name\" label=\"Name\" type=\"text\" hint=\"Short\" required span=\"full\" maxlength=\"20\" data-x=\"1\"/>\
         <rx-textarea name=\"bio\" label=\"Bio\" rows=\"2\" value=\"hi\"/>\
         </rx-form-grid>\
         <rx-select name=\"size\" label=\"Size\" :options=\"[['s', 'Small'], ['m', 'Medium']]\" required/>\
         <rx-checkbox name=\"agree\" label=\"Agree\" switch/>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(FormPages), move |c| c.views_path = path).await;
    let bad = [
        ("name", "Much too long a name"),
        ("bio", "far too long"),
        ("agree", "on"),
    ];
    let mut pages = Vec::new();
    for page in ["/m", "/r"] {
        app.request()
            .header("referer", page)
            .post("/save", &bad)
            .await
            .assert_redirect(page);
        let r = app.get(page).await;
        r.assert_ok();
        pages.push(normalize(&r.text()));
    }
    assert!(pages[0].contains("aria-invalid=\"true\""), "{}", pages[0]);
    assert!(pages[0].contains("value=\"Much too long a name\""));
    assert!(pages[0].contains("data-rx-error-summary"));
    assert_eq!(pages[0], pages[1]);
}

#[renox::test]
async fn rx_form_writes_the_csrf_and_method_fields() {
    let (put, get, post) = (
        "<rx-form action=\"/items/1\" method=\"PUT\" live class=\"box\" id=\"f\" :data-n=\"1 + 1\" hx-boost=\"true\">x</rx-form>",
        "<rx-form action=\"/search\" method=\"GET\"><input name=\"q\"></rx-form>",
        "<rx-form route=\"home\"></rx-form><rx-form></rx-form>",
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("r.html"), format!("{put}{get}{post}")).unwrap();
    std::fs::write(dir.path().join("m.html"), "").unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(FormPages).module(HomeName), move |c| {
        c.views_path = path
    })
    .await;
    let html = app.get("/r").await.assert_ok().text();
    let forms: Vec<&str> = html.split("</form>").collect();
    let first = forms[0];
    assert!(first.starts_with(
        "<form method=\"post\" action=\"/items/1\" data-live-validate novalidate class=\"box\" id=\"f\" data-n=\"2\" hx-boost=\"true\">"
    ), "{first}");
    assert!(first.contains("name=\"_token\""), "{first}");
    assert!(first.contains("name=\"_method\" value=\"PUT\""), "{first}");
    let second = forms[1];
    assert!(
        second.starts_with("<form method=\"get\" action=\"/search\">"),
        "{second}"
    );
    assert!(
        !second.contains("_token") && !second.contains("_method"),
        "{second}"
    );
    assert!(
        forms[2].starts_with("<form method=\"post\" action=\"/home\">"),
        "{}",
        forms[2]
    );
    assert!(forms[2].contains("_token") && !forms[2].contains("_method"));
    assert!(
        forms[3].starts_with("<form method=\"post\">"),
        "{}",
        forms[3]
    );
}

struct HomeName;

impl Module for HomeName {
    fn name(&self) -> &'static str {
        "home-name"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/home", || async { "home" }).name("home")
    }
}

#[renox::test]
async fn rx_form_rejects_a_bad_method() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-form method=\"PUTT\"></rx-form>",
    )
    .unwrap();
    std::fs::write(dir.path().join("m.html"), "").unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(FormPages), move |c| c.views_path = path).await;
    let r = app.get("/r").await.text();
    assert!(r.contains("must be one of"), "{r}");
}

#[renox::test]
async fn rx_form_offline_marks_the_form_and_loads_the_outbox_script_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-form action=\"/save\" offline=\"visits\" class=\"a\">x</rx-form>\
         <rx-form action=\"/save\" offline=\"notes\">y</rx-form>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        "<rx-form action=\"/save\">z</rx-form>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(FormPages), move |c| c.views_path = path).await;
    let html = app.get("/r").await.assert_ok().text();
    assert!(
        html.contains(
            "<form method=\"post\" action=\"/save\" data-rx-offline=\"visits\" data-rx-offline-csrf=\"/_renox/csrf\" class=\"a\">"
        ),
        "{html}"
    );
    assert!(html.contains("data-rx-offline=\"notes\""), "{html}");
    assert_eq!(html.matches("/_renox/offline-").count(), 1, "{html}");
    let start = html.find("/_renox/offline-").unwrap();
    let url = &html[start..start + html[start..].find('"').unwrap()];
    let script = app.get(url).await.assert_ok().text();
    assert!(script.contains("data-rx-offline"), "{script}");

    let plain = app.get("/m").await.assert_ok().text();
    assert!(!plain.contains("offline"), "{plain}");
}

#[renox::test]
async fn rx_form_offline_needs_a_writing_method() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-form method=\"GET\" offline=\"q\"></rx-form>",
    )
    .unwrap();
    std::fs::write(dir.path().join("m.html"), "").unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(FormPages), move |c| c.views_path = path).await;
    let r = app.get("/r").await.text();
    assert!(r.contains("needs a method that writes"), "{r}");
}

#[derive(serde::Deserialize)]
struct Choices {
    plan: String,
    #[serde(default)]
    extras: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

impl Validate for Choices {
    fn rules(&self, v: &mut Validator) {
        v.field("plan", &self.plan)
            .one_of(&["free".to_string(), "pro".to_string()]);
        let _ = (&self.extras, &self.tags);
    }
}

struct ChoicePages;

impl Module for ChoicePages {
    fn name(&self) -> &'static str {
        "choice-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/m", || async { view("m.html", context! {}) })
            .get("/r", || async { view("r.html", context! {}) })
            .post("/save", |Valid(_): Valid<Choices>| async { "ok" })
    }
}

#[renox::test]
async fn choice_fields_match_the_macros_and_refill_after_a_failed_submit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        format!(
            "{UI}{{{{ ui.radio(name=\"plan\", label=\"Plan\", options=[[\"free\", \"Free\"], [\"pro\", \"Pro\", \"Teams\"]], hint=\"Pick\", required=true, inline=true, attrs={{\"data-x\": \"1\"}}) }}}}\
             {{{{ ui.checkbox_list(name=\"extras\", label=\"Extras\", options=[\"gift\", [\"note\", \"Card\"]], selected=[\"gift\"], columns=2) }}}}\
             {{{{ ui.tags_input(name=\"tags\", label=\"Tags\", value=[\"a\"], suggestions=[\"b\", \"c\"], placeholder=\"Add\") }}}}"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-radio name=\"plan\" label=\"Plan\" :options=\"[['free', 'Free'], ['pro', 'Pro', 'Teams']]\" hint=\"Pick\" required inline data-x=\"1\"/>\
         <rx-checkbox-list name=\"extras\" label=\"Extras\" :options=\"['gift', ['note', 'Card']]\" :selected=\"['gift']\" columns=\"2\"/>\
         <rx-tags-input name=\"tags\" label=\"Tags\" :value=\"['a']\" :suggestions=\"['b', 'c']\" placeholder=\"Add\"/>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(ChoicePages), move |c| c.views_path = path).await;
    let bad = [
        ("plan", "gold"),
        ("extras", "note"),
        ("tags", "x"),
        ("tags", "y"),
    ];
    let mut pages = Vec::new();
    for page in ["/m", "/r"] {
        app.request()
            .header("referer", page)
            .post("/save", &bad)
            .await
            .assert_redirect(page);
        let r = app.get(page).await;
        r.assert_ok();
        pages.push(normalize(&r.text()));
    }
    assert!(pages[0].contains("aria-invalid=\"true\""), "{}", pages[0]);
    assert!(pages[0].contains("value=\"x\""), "{}", pages[0]);
    assert_eq!(pages[0], pages[1]);
}

#[renox::test]
async fn rx_toggle_buttons_and_rx_file_match_the_macros() {
    same(
        &format!(
            "{UI}{{{{ ui.toggle_buttons(name=\"mode\", label=\"Mode\", options=[[\"a\", \"A\"], [\"b\", \"B\"]], selected=\"b\", hint=\"One\") }}}}\
             {{{{ ui.toggle_buttons(name=\"days\", label=\"Days\", options=[\"mon\", \"tue\"], selected=[\"tue\"], multiple=true) }}}}\
             {{{{ ui.file(name=\"doc\", label=\"Document\", accept=\"image/*\", multiple=true, current=\"/f/a.png\", current_name=\"a.png\", preview=true, required=true) }}}}"
        ),
        "<rx-toggle-buttons name=\"mode\" label=\"Mode\" :options=\"[['a', 'A'], ['b', 'B']]\" :selected=\"'b'\" hint=\"One\"/>\
         <rx-toggle-buttons name=\"days\" label=\"Days\" :options=\"['mon', 'tue']\" :selected=\"['tue']\" multiple/>\
         <rx-file name=\"doc\" label=\"Document\" accept=\"image/*\" multiple current=\"/f/a.png\" current-name=\"a.png\" preview required/>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_date_picker_matches_the_macro() {
    same(
        &format!(
            "{UI}{{{{ ui.date_picker(name=\"day\", label=\"Day\", value=\"2026-01-05\", min=\"2026-01-01\", max=\"2026-12-31\", hint=\"Pick\", required=true, readonly=true, disabled_dates=[\"2026-01-06\"], closed_weekdays=[0, 6]) }}}}"
        ),
        "<rx-date-picker name=\"day\" label=\"Day\" :value=\"'2026-01-05'\" min=\"2026-01-01\" max=\"2026-12-31\" hint=\"Pick\" required readonly :disabled-dates=\"['2026-01-06']\" :closed-weekdays=\"[0, 6]\"/>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_layout_fields_match_the_macros() {
    same(
        &format!(
            "{UI}{{% call ui.fieldset(legend=\"Shipping\", hint=\"Where to\", columns=2) %}}<p>a</p>{{% endcall %}}\
             {{% call ui.show_when(field=\"delivery\", values=\"courier\") %}}<p>b</p>{{% endcall %}}\
             {{% call ui.show_when(field=\"delivery\", values=[\"courier\", \"post\"]) %}}<p>c</p>{{% endcall %}}\
             {{% call ui.hide_when(field=\"pickup\", values=\"on\") %}}<p>d</p>{{% endcall %}}\
             {{{{ ui.key_value(name=\"headers\", label=\"Headers\", value=[[\"a\", \"1\"]], key_label=\"Name\", value_label=\"Val\", add_label=\"More\", hint=\"Sent\", span=\"full\") }}}}\
             {{{{ ui.key_value(name=\"plain\", label=\"Plain\") }}}}"
        ),
        "<rx-fieldset legend=\"Shipping\" hint=\"Where to\" columns=\"2\"><p>a</p></rx-fieldset>\
         <rx-show-when field=\"delivery\" values=\"courier\"><p>b</p></rx-show-when>\
         <rx-show-when field=\"delivery\" :values=\"['courier', 'post']\"><p>c</p></rx-show-when>\
         <rx-hide-when field=\"pickup\" values=\"on\"><p>d</p></rx-hide-when>\
         <rx-key-value name=\"headers\" label=\"Headers\" :value=\"[['a', '1']]\" key-label=\"Name\" value-label=\"Val\" add-label=\"More\" hint=\"Sent\" span=\"full\"/>\
         <rx-key-value name=\"plain\" label=\"Plain\"/>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_select_options_url_and_editable_match_the_macro() {
    same(
        &format!(
            "{UI}{{{{ ui.select(name=\"cat\", label=\"Category\", options=[], options_url=\"/options/cats\", editable=true, searchable=true) }}}}\
             {{{{ ui.select(name=\"tags\", label=\"Tags\", options=[[\"a\", \"A\"]], selected=[\"a\"], multiple=true, hide_label=true) }}}}"
        ),
        "<rx-select name=\"cat\" label=\"Category\" :options=\"[]\" options-url=\"/options/cats\" editable searchable/>\
         <rx-select name=\"tags\" label=\"Tags\" :options=\"[['a', 'A']]\" :selected=\"['a']\" multiple hide-label/>",
        serde_json::json!({}),
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-select name=\"cat\" label=\"Category\" :options=\"[]\" options-url=\"/options/cats\" editable/>",
    )
    .unwrap();
    std::fs::write(dir.path().join("m.html"), "").unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(FormPages), move |c| c.views_path = path).await;
    let html = app.get("/r").await.assert_ok().text();
    assert!(
        html.contains("data-rx-options-url=\"/options/cats\""),
        "{html}"
    );
    assert!(html.contains("data-rx-editable"), "{html}");
}

#[derive(serde::Deserialize)]
struct OrderLine {
    #[allow(dead_code)]
    name: String,
    qty: i64,
}

#[derive(serde::Deserialize)]
struct Order {
    lines: Vec<OrderLine>,
}

impl Validate for OrderLine {
    fn rules(&self, v: &mut Validator) {
        v.field("qty", &self.qty).min(1);
    }
}

impl Validate for Order {
    fn rules(&self, v: &mut Validator) {
        v.nested("lines", &self.lines);
    }
}

struct RepeaterPages;

impl Module for RepeaterPages {
    fn name(&self) -> &'static str {
        "repeater-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/m", || async {
                view(
                    "m.html",
                    context! { lines => vec![context! { name => "Tea", qty => 1 }] },
                )
            })
            .get("/r", || async {
                view(
                    "r.html",
                    context! { lines => vec![context! { name => "Tea", qty => 1 }] },
                )
            })
            .post("/save", |Valid(_): Valid<Order>| async { "ok" })
    }
}

#[renox::test]
async fn rx_repeater_matches_the_macro_and_shows_errors_on_nested_names() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        format!(
            "{UI}{{% call(row, prefix) ui.repeater(name=\"lines\", label=\"Lines\", rows=(lines), min=1, max=5, item_label=\"Line\", add_label=\"More\", hint=\"Items\", reorderable=false) %}}\
             {{{{ ui.input(name=(prefix) ~ \"[name]\", label=\"Name\", value=(row.name)) }}}}\
             {{{{ ui.input(name=(prefix) ~ \"[qty]\", label=\"Qty\", type=\"number\", value=(row.qty)) }}}}{{% endcall %}}"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-repeater name=\"lines\" label=\"Lines\" :rows=\"lines\" min=\"1\" max=\"5\" item-label=\"Line\" add-label=\"More\" hint=\"Items\" :reorderable=\"false\">\
         <rx-input name=\"{{ prefix }}[name]\" label=\"Name\" :value=\"row.name\"/>\
         <rx-input name=\"{{ prefix }}[qty]\" label=\"Qty\" type=\"number\" :value=\"row.qty\"/></rx-repeater>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(RepeaterPages), move |c| {
        c.views_path = path
    })
    .await;
    let bad = [
        ("lines[0][name]", "Tea"),
        ("lines[0][qty]", "1"),
        ("lines[1][name]", "Cake"),
        ("lines[1][qty]", "0"),
    ];
    let mut pages = Vec::new();
    for page in ["/m", "/r"] {
        let fresh = normalize(&app.get(page).await.assert_ok().text());
        assert!(fresh.contains("name=\"lines[0][qty]\""), "{fresh}");
        app.request()
            .header("referer", page)
            .post("/save", &bad)
            .await
            .assert_redirect(page);
        let r = app.get(page).await;
        r.assert_ok();
        pages.push(normalize(&r.text()));
    }
    assert!(pages[0].contains("name=\"lines[1][qty]\""), "{}", pages[0]);
    assert!(pages[0].contains("value=\"Cake\""), "{}", pages[0]);
    assert!(pages[0].contains("aria-invalid=\"true\""), "{}", pages[0]);
    assert_eq!(pages[0], pages[1]);
}

/// `<rx-grid>` over a real grid page of users, with a `custom` column drawn by the slot.
#[renox::test]
async fn rx_grid_matches_the_macro() {
    use renox::grid::{Column, Grid, GridRequest};

    async fn show(name: &'static str, request: GridRequest) -> Result<View> {
        let page = Grid::new("people")
            .column(Column::text("name", "Name"))
            .column(Column::custom("note", "Note"))
            .page(User::query(), &request)
            .await?;
        Ok(view(name, context! { page => page }))
    }
    struct People;
    impl Module for People {
        fn name(&self) -> &'static str {
            "people"
        }
        fn routes(&self) -> Routes {
            Routes::new()
                .get("/m", |request: GridRequest| show("m.html", request))
                .get("/r", |request: GridRequest| show("r.html", request))
        }
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.html"),
        "{% from \"renox/grid.html\" import grid %}{% call(row, column) grid(page) %}<b>{{ column.key }}:{{ row.name }}</b>{% endcall %}",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("r.html"),
        "<rx-grid :page=\"page\"><b>{{ column.key }}:{{ row.name }}</b></rx-grid>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Auth::new()).module(People), move |c| {
        c.views_path = path
    })
    .await;
    for name in ["Ann", "Ben"] {
        User::register(
            app.db(),
            name,
            &format!("{name}@example.com"),
            "password123",
        )
        .await
        .unwrap();
    }
    let (m, r) = (app.get("/m").await, app.get("/r").await);
    m.assert_ok();
    r.assert_ok();
    // The two pages differ only in their own URL.
    let (m, r) = (
        normalize(&m.text()),
        normalize(&r.text()).replace("=\"/r\"", "=\"/m\""),
    );
    assert!(m.contains("note:Ann") && m.contains("data-rx-component=\"grid\""));
    assert_eq!(m, r);
}

#[renox::test]
async fn rx_infolist_and_entry_match_the_macros() {
    same(
        &format!(
            "{UI}{{% call ui.infolist(columns=2, inline=true) %}}\
             {{{{ ui.entry(label=\"Number\", value=(order.number), copyable=true) }}}}\
             {{{{ ui.entry(label=\"Status\", value=(order.status), badge={{\"paid\": \"success\"}}, span=\"full\") }}}}\
             {{{{ ui.entry(label=\"Total\", value=(order.total), format=\"money\", hint=\"Tax in\", limit=5, hide_label=true) }}}}\
             {{{{ ui.entry(label=\"Tags\", value=(order.tags), list=\"bullets\", limit_list=2, url=\"/t\", new_tab=true) }}}}\
             {{{{ ui.entry(label=\"Empty\", value=none, placeholder=\"n/a\") }}}}\
             {{% call ui.entry(label=\"Custom\") %}}<b>x</b>{{% endcall %}}\
             {{% endcall %}}{{% call ui.infolist() %}}{{% endcall %}}"
        ),
        "<rx-infolist columns=\"2\" inline>\
         <rx-entry label=\"Number\" :value=\"order.number\" copyable/>\
         <rx-entry label=\"Status\" :value=\"order.status\" :badge=\"{'paid': 'success'}\" span=\"full\"/>\
         <rx-entry label=\"Total\" :value=\"order.total\" format=\"money\" hint=\"Tax in\" limit=\"5\" hide-label/>\
         <rx-entry label=\"Tags\" :value=\"order.tags\" list=\"bullets\" limit-list=\"2\" url=\"/t\" new-tab/>\
         <rx-entry label=\"Empty\" :value=\"none\" placeholder=\"n/a\"/>\
         <rx-entry label=\"Custom\"><b>x</b></rx-entry>\
         </rx-infolist><rx-infolist></rx-infolist>",
        serde_json::json!({"order": {"number": "A-1", "status": "paid", "total": 12345, "tags": ["a", "b", "c"]}}),
    )
    .await;
}

#[renox::test]
async fn rx_repeatable_matches_the_macro() {
    same(
        &format!(
            "{UI}{{% call(line) ui.repeatable(label=\"Items\", items=(lines), columns=3, hide_label=true) %}}\
             {{{{ ui.entry(label=\"Name\", value=(line.name)) }}}}{{% endcall %}}\
             {{% call(line) ui.repeatable(label=\"None\", items=[], placeholder=\"nothing\", span=\"2\") %}}x{{% endcall %}}"
        ),
        "<rx-repeatable label=\"Items\" :items=\"lines\" columns=\"3\" hide-label>\
         <rx-entry label=\"Name\" :value=\"item.name\"/></rx-repeatable>\
         <rx-repeatable label=\"None\" :items=\"[]\" placeholder=\"nothing\" span=\"2\">x</rx-repeatable>",
        serde_json::json!({"lines": [{"name": "Bell"}, {"name": "Lock"}]}),
    )
    .await;
}

#[renox::test]
async fn rx_list_and_cards_match_the_macros() {
    same(
        &format!(
            "{UI}{{% call ui.list(id=\"l\", label=\"Rows\") %}}<li>a</li>{{% endcall %}}{{% call ui.list() %}}<li>b</li>{{% endcall %}}\
             {{% call ui.card_grid() %}}\
             {{{{ ui.media_card(href=\"/p/1\", title=(title), image=\"/i.png\", subtitle=\"$5\", note=\"Sold out\", dimmed=true, image_alt=\"Bell\") }}}}\
             {{{{ ui.media_card(href=\"/p/2\", title=\"Plain\") }}}}\
             {{% endcall %}}"
        ),
        "<rx-list id=\"l\" label=\"Rows\"><li>a</li></rx-list><rx-list><li>b</li></rx-list>\
         <rx-card-grid>\
         <rx-media-card href=\"/p/1\" title=\"{{ title }}\" image=\"/i.png\" subtitle=\"$5\" note=\"Sold out\" dimmed image-alt=\"Bell\"/>\
         <rx-media-card href=\"/p/2\" title=\"Plain\"/>\
         </rx-card-grid>",
        serde_json::json!({"title": "Bell <b>"}),
    )
    .await;
}

#[renox::test]
async fn rx_progress_matches_the_macro() {
    same(
        &format!(
            "{UI}{{{{ ui.progress(value=(done), max=8, label=\"Steps\") }}}}\
             {{{{ ui.progress(value=3, show_value=false) }}}}{{{{ ui.progress(value=0, max=0) }}}}"
        ),
        "<rx-progress :value=\"done\" max=\"8\" label=\"Steps\"/>\
         <rx-progress value=\"3\" :show-value=\"false\"/><rx-progress value=\"0\" max=\"0\"/>",
        serde_json::json!({"done": 2}),
    )
    .await;
}

#[renox::test]
async fn rx_stats_and_stat_match_the_macros() {
    same(
        &format!(
            "{UI}{{% call ui.stats(columns=3) %}}{{{{ ui.stat(label=\"Revenue\", value=(total), delta=12.5, good=\"up\", trend=(trend), decimals=2) }}}}\
             {{{{ ui.stat(label=\"Open\", value=\"4\", hint=\"today\", url=\"/open\", icon=\"home\") }}}}{{% endcall %}}"
        ),
        "<rx-stats columns=\"3\"><rx-stat label=\"Revenue\" value=\"{{ total }}\" :delta=\"12.5\" good=\"up\" :trend=\"trend\" decimals=\"2\"/>\
         <rx-stat label=\"Open\" value=\"4\" hint=\"today\" url=\"/open\" icon=\"home\"/></rx-stats>",
        serde_json::json!({"total": "$10", "trend": [1, 3, 2]}),
    )
    .await;
}

#[renox::test]
async fn rx_dashboard_and_widget_match_the_macros() {
    same(
        &format!(
            "{UI}{{% call ui.dashboard(columns=2) %}}{{% call ui.widget(title=\"Sales\", description=\"Per day\", span=2, id=\"w1\") %}}<p>in</p>{{% endcall %}}\
             {{{{ ui.widget(title=\"Later\", url=\"/later\", poll=30, id=\"w2\") }}}}{{% endcall %}}"
        ),
        "<rx-dashboard columns=\"2\"><rx-widget title=\"Sales\" description=\"Per day\" span=\"2\" id=\"w1\"><p>in</p></rx-widget>\
         <rx-widget title=\"Later\" url=\"/later\" poll=\"30\" id=\"w2\"/></rx-dashboard>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_period_filter_matches_the_macro() {
    same(
        &format!(
            "{UI}{{{{ ui.period_filter(selected=(period)) }}}}{{{{ ui.period_filter(selected=(period), options=[[\"1d\", \"Day\"]], custom=false, label=\"Range\") }}}}"
        ),
        "<rx-period-filter :selected=\"period\"/><rx-period-filter :selected=\"period\" :options=\"[['1d', 'Day']]\" :custom=\"false\" label=\"Range\"/>",
        serde_json::json!({"period": "30d"}),
    )
    .await;
}

#[renox::test]
async fn rx_chart_calls_the_function() {
    same(
        "{{ chart(\"bar\", sales, title=\"Sales\", height=200, stacked=true) }}{{ chart(\"heatmap\", columns=[\"a\"], rows=[\"x\"], values=[[1]]) }}",
        "<rx-chart kind=\"bar\" :data=\"sales\" title=\"Sales\" height=\"200\" stacked/>\
         <rx-chart kind=\"heatmap\" :columns=\"['a']\" :rows=\"['x']\" :values=\"[[1]]\"/>",
        serde_json::json!({"sales": [3, 5, 4]}),
    )
    .await;
}

#[renox::test]
async fn rx_chart_needs_its_kind() {
    let (app, _dir) = tables("<rx-chart :data=\"[1]\"/>", serde_json::json!({})).await;
    let r = app.get("/r").await;
    assert_eq!(r.status.as_u16(), 500);
}

#[renox::test]
async fn rx_menu_parts_match_the_macros() {
    same(
        &format!(
            "{UI}{{% call ui.menu(\"More\", variant=\"primary\") %}}\
             {{{{ ui.menu_link(href=\"/a\", label=\"Edit\", icon=\"plus\", download=true, new_tab=true, danger=true) }}}}\
             {{{{ ui.menu_open(id=\"s1\", label=\"Open\", danger=true) }}}}\
             {{% call ui.menu_section(\"Export\") %}}{{{{ ui.menu_action(action=\"/x\", label=\"Delete\", method=\"DELETE\", danger=true) }}}}{{% endcall %}}\
             {{{{ ui.menu_separator() }}}}\
             {{{{ ui.menu_action(action=\"/y\", label=\"Post\") }}}}\
             {{% endcall %}}"
        ),
        "<rx-menu label=\"More\" variant=\"primary\">\
         <rx-menu-link href=\"/a\" icon=\"plus\" download new-tab danger>Edit</rx-menu-link>\
         <rx-menu-open id=\"s1\" danger>Open</rx-menu-open>\
         <rx-menu-section title=\"Export\"><rx-menu-action action=\"/x\" method=\"DELETE\" danger>Delete</rx-menu-action></rx-menu-section>\
         <rx-menu-separator/>\
         <rx-menu-action action=\"/y\" label=\"Post\"/>\
         </rx-menu>",
        serde_json::json!({}),
    )
    .await;
}

#[renox::test]
async fn rx_link_tabs_matches_the_macro() {
    same(
        &format!(
            "{UI}{{{{ ui.link_tabs(items=(tabs), current=\"/b\", label=\"Views\") }}}}\
             {{{{ ui.link_tabs(items=(tabs)) }}}}"
        ),
        "<rx-link-tabs :items=\"tabs\" current=\"/b\" label=\"Views\"/><rx-link-tabs :items=\"tabs\"/>",
        serde_json::json!({"tabs": [["/a", "A"], ["/b", "B"]]}),
    )
    .await;
}

#[renox::test]
async fn rx_navbar_and_links_match_the_macros() {
    same(
        &format!(
            "{UI}{{% call ui.navbar(\"Shop\", href=\"/home\", mark=true, width=\"full\", tabs=(tabs)) %}}\
             {{% call ui.nav_links() %}}{{{{ ui.nav_link(\"/a\", \"Alpha\", active=true, badge=3) }}}}{{{{ ui.nav_link(\"/b\", \"Beta\") }}}}{{% endcall %}}\
             {{% call ui.nav_search(label=\"Find\", id=\"s1\") %}}<input name=\"q\">{{% endcall %}}{{% endcall %}}\
             {{% call ui.navbar(none, skip=false) %}}x{{% endcall %}}"
        ),
        "<rx-navbar brand=\"Shop\" href=\"/home\" mark width=\"full\" :tabs=\"tabs\">\
         <rx-nav-links><rx-nav-link href=\"/a\" :active=\"true\" badge=\"3\">Alpha</rx-nav-link><rx-nav-link href=\"/b\" label=\"Beta\"/></rx-nav-links>\
         <rx-nav-search label=\"Find\" id=\"s1\"><input name=\"q\"></rx-nav-search></rx-navbar>\
         <rx-navbar :skip=\"false\">x</rx-navbar>",
        serde_json::json!({"tabs": [{"label": "Home", "href": "/"}]}),
    )
    .await;
}

#[renox::test]
async fn rx_sidebar_and_shell_match_the_macros() {
    same(
        &format!(
            "{UI}<div class=\"rx-shell\" id=\"sh\">{{% call ui.sidebar(\"Admin\", logo=\"/l.png\") %}}\
             {{{{ ui.sidebar_section(\"Manage\") }}}}{{{{ ui.sidebar_link(\"/o\", \"Orders\", active=true, badge=2, icon=\"home\") }}}}\
             {{{{ ui.sidebar_link(\"/p\", \"Products\") }}}}{{% endcall %}}<div class=\"rx-shell__main\">hi</div></div>"
        ),
        "<rx-shell id=\"sh\"><rx-sidebar brand=\"Admin\" logo=\"/l.png\">\
         <rx-sidebar-section>Manage</rx-sidebar-section>\
         <rx-sidebar-link href=\"/o\" :active=\"true\" badge=\"2\" icon=\"home\">Orders</rx-sidebar-link>\
         <rx-sidebar-link href=\"/p\" label=\"Products\"/></rx-sidebar><div class=\"rx-shell__main\">hi</div></rx-shell>",
        serde_json::json!({}),
    )
    .await;
}

/// #383: `view:check` on page structs registered with `App::view_data` / `Registry::view_data`.
#[derive(serde::Serialize, ViewData)]
#[view(template = "suppliers.html")]
struct SupplierList {
    suppliers: Vec<String>,
    #[serde(rename = "total")]
    count: usize,
}

#[derive(serde::Serialize, ViewData)]
#[view(template = "reports.html")]
struct Reports {
    rows: Vec<String>,
}

#[derive(serde::Serialize, ViewData)]
#[view(template = "gone.html")]
struct Gone {}

struct Typed;

impl Module for Typed {
    fn name(&self) -> &'static str {
        "typed"
    }

    fn routes(&self) -> Routes {
        Routes::new()
    }

    fn register(&self, registry: &mut Registry) {
        registry.view_data::<Reports>();
    }
}

fn view_check_problems(app: &TestApp) -> String {
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.assert_views_compile();
    }));
    match failed {
        Ok(_) => String::new(),
        Err(panic) => panic.downcast_ref::<String>().cloned().unwrap_or_default(),
    }
}

#[renox::test]
async fn view_check_reports_variables_a_typed_page_lacks() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    // Fields, request globals, environment functions, a shared key, names the template
    // defines, and a typo inside a component's body (checked after the rx-* compilation).
    write(
        "suppliers.html",
        "<h1>{{ app.name }} {{ total }} {{ brand }}</h1>\n\
         {% set shown = 1 %}{{ shown }}{{ csrf_field() }}{{ t('Suppliers') }}{{ auth.check }}\n\
         {% for s in suppliers %}{{ loop.index }} {{ s | upper }}{% endfor %}{{ range(2) }}\n\
         <rx-card title=\"Suppliers\">\n{{ suplliers }} {{ old('q') }}\n</rx-card>\n",
    );
    write(
        "reports.html",
        "{% for r in rows %}{{ r }}{% endfor %}{{ row_count }}",
    );
    // Not registered: nothing is checked against a struct.
    write("loose.html", "{{ anything }}");
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Typed)
            .view_data::<SupplierList>()
            .view_data::<SupplierList>()
            .share("brand", |_ctx| async { Ok("Velo") }),
        move |c| c.views_path = path,
    )
    .await;
    let message = view_check_problems(&app);
    assert!(message.starts_with("2 template problem(s)"), "{message}");
    assert!(
        message.contains("suppliers.html:5: \"suplliers\" isn't a field of SupplierList"),
        "{message}"
    );
    assert!(
        message.contains("reports.html:1: \"row_count\" isn't a field of Reports"),
        "{message}"
    );
}

#[renox::test]
async fn view_check_reports_a_typed_page_without_its_template() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().view_data::<Gone>(), move |c| c.views_path = path).await;
    let message = view_check_problems(&app);
    assert!(
        message.contains("gone.html: not found, but Gone renders it"),
        "{message}"
    );
}

const MAIL_PARTS: &str =
    "{% from \"renox/mail/components.html\" import button, panel, table, divider %}";

/// #509: `<rx-mail>` and its parts render the same mail as `{% extends %}` and the macros.
#[renox::test]
async fn mail_tags_match_the_mail_layout_and_macros() {
    let ctx = serde_json::json!({
        "subj": "Your order", "name": "Ben", "link": "https://shop.test/o/1",
        "rows": [["Coffee", "$4.50"], ["Tea", "$3.00"]],
    });
    same(
        &format!(
            "{{% extends \"renox/mail/layout.html\" %}}{MAIL_PARTS}\
             {{% block title %}}{{{{ subj }}}}{{% endblock %}}\
             {{% block content %}}<p>Hi {{{{ name }}}}</p>{{{{ button(link, \"Pay now\") }}}}\
             {{% call panel() %}}Ships tomorrow.{{% endcall %}}\
             {{{{ table(rows, head=[\"Item\", \"Price\"], total=[\"Total\", \"$7.50\"]) }}}}{{{{ divider() }}}}\
             {{% endblock %}}{{% block footer %}}Thanks, {{{{ name }}}}{{% endblock %}}"
        ),
        "<rx-mail subject=\"{{ subj }}\">\n<p>Hi {{ name }}</p>\n\
         <rx-mail-button :url=\"link\" label=\"Pay now\"/>\n\
         <rx-mail-panel>Ships tomorrow.</rx-mail-panel>\n\
         <rx-mail-table :rows=\"rows\" :head=\"['Item', 'Price']\" :total=\"['Total', '$7.50']\"/>\n\
         <rx-mail-divider/>\n\
         <rx-slot name=\"footer\">Thanks, {{ name }}</rx-slot>\n</rx-mail>",
        ctx.clone(),
    )
    .await;
    // The default layout's footer and title stay when the tag gives none, and a button can
    // take its label as content.
    same(
        &format!(
            "{{% extends \"renox/mail/layout.html\" %}}{MAIL_PARTS}\
             {{% block content %}}{{{{ button(link, \"Open\") }}}}{{% endblock %}}"
        ),
        "<rx-mail><rx-mail-button :url=\"link\">Open</rx-mail-button></rx-mail>",
        ctx,
    )
    .await;
}

/// #510: the small kit macros as tags render what the macros do.
#[renox::test]
async fn small_kit_tags_match_the_macros() {
    let ctx = serde_json::json!({"p": {"image": "/p.png", "name": "Bike"}, "url": "/edit/1"});
    same(
        "{% from \"renox/ui.html\" import icon, columns, thumbnail, tab_panel, tabs %}\
         {{ icon(\"bike\") }}{{ icon(\"lock\", size=16, label=\"Private\") }}\
         {% call columns(3) %}<i>a</i><i>b</i>{% endcall %}\
         {{ thumbnail(p.image, alt=p.name, href=\"/x\") }}{{ thumbnail(p.image) }}\
         {{ tabs(\"t\", [[\"a\", \"A\"], [\"b\", \"B\"]]) }}\
         {% call tab_panel(\"t\", \"a\", selected=true) %}One{% endcall %}{% call tab_panel(\"t\", \"b\") %}Two{% endcall %}",
        "<rx-icon name=\"bike\"/><rx-icon name=\"lock\" size=\"16\" label=\"Private\"/>\
         <rx-columns count=\"3\"><i>a</i><i>b</i></rx-columns>\
         <rx-thumbnail :src=\"p.image\" :alt=\"p.name\" href=\"/x\"/><rx-thumbnail :src=\"p.image\"/>\
         <rx-tabs id=\"t\"><rx-tab key=\"a\" label=\"A\">One</rx-tab><rx-tab key=\"b\" label=\"B\">Two</rx-tab></rx-tabs>",
        ctx.clone(),
    )
    .await;
    // `<rx-tab-panel>` is the macro for a `tabs(...)` written by hand.
    same(
        "{% from \"renox/ui.html\" import tab_panel %}{% call tab_panel(\"t\", \"a\", selected=true) %}One{% endcall %}{% call tab_panel(\"t\", \"b\") %}Two{% endcall %}",
        "<rx-tab-panel id=\"t\" key=\"a\" selected>One</rx-tab-panel><rx-tab-panel id=\"t\" key=\"b\">Two</rx-tab-panel>",
        ctx.clone(),
    )
    .await;
    same(
        "{% from \"renox/ui.html\" import action_group, menu_link, menu_button, event_stream %}\
         {% call action_group() %}{{ menu_link(\"/a\", \"A\", icon=\"copy\") }}\
         {{ menu_button(\"Edit\", attrs={\"hx-get\": url}, icon=\"edit\") }}\
         {{ menu_button(\"Drop\", attrs={\"hx-delete\": url}, danger=true) }}{% endcall %}\
         {% call action_group(\"More\", icon=\"plus\") %}{{ menu_link(\"/b\", \"B\") }}{% endcall %}\
         [{{ event_stream() }}]",
        "<rx-action-group><rx-menu-link href=\"/a\" label=\"A\" icon=\"copy\"/>\
         <rx-menu-button hx-get=\"{{ url }}\" icon=\"edit\">Edit</rx-menu-button>\
         <rx-menu-button hx-delete=\"{{ url }}\" danger>Drop</rx-menu-button></rx-action-group>\
         <rx-action-group label=\"More\" icon=\"plus\"><rx-menu-link href=\"/b\" label=\"B\"/></rx-action-group>\
         [<rx-event-stream/>]",
        ctx.clone(),
    )
    .await;
    same(
        "{% from \"renox/ui.html\" import import_action, wizard_action, wizard_step, input %}\
         {{ import_action(\"imp\", \"Import\", \"/import\", \"Import products\", columns=[\"sku\", \"name\"], template_url=\"/t\", description=\"CSV\") }}\
         {{ import_action(\"imp2\", \"Import\", \"/import\", \"Import\", button=false) }}\
         {% call wizard_action(\"np\", \"New\", \"/products\", \"New product\", [[\"a\", \"One\"], [\"b\", \"b\"]], submit_label=\"Add\", variant=\"primary\", icon=\"plus\", live=false) %}\
         {% call wizard_step(\"np\", \"a\", title=\"One\") %}A{% endcall %}{% call wizard_step(\"np\", \"b\") %}B{% endcall %}{% endcall %}",
        "<rx-import-action id=\"imp\" label=\"Import\" action=\"/import\" title=\"Import products\" :columns=\"['sku', 'name']\" template-url=\"/t\" description=\"CSV\"/>\
         <rx-import-action id=\"imp2\" label=\"Import\" action=\"/import\" title=\"Import\" :button=\"false\"/>\
         <rx-wizard-action id=\"np\" label=\"New\" action=\"/products\" title=\"New product\" submit-label=\"Add\" variant=\"primary\" icon=\"plus\" :live=\"false\">\
         <rx-wizard-step key=\"a\" title=\"One\">A</rx-wizard-step><rx-wizard-step key=\"b\">B</rx-wizard-step></rx-wizard-action>",
        ctx,
    )
    .await;
}

/// `route` fills `action` of the new action tags, like `rx-action-sheet`.
#[renox::test]
async fn the_import_and_wizard_action_tags_take_a_route() {
    struct Routed;
    impl Module for Routed {
        fn name(&self) -> &'static str {
            "routed"
        }
        fn routes(&self) -> Routes {
            Routes::new()
                .get("/p", || async { view("p.html", context! {}) })
                .post("/import", || async { "" })
                .name("things.import")
                .post("/new", || async { "" })
                .name("things.new")
        }
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("p.html"),
        "<rx-import-action id=\"i\" label=\"Import\" route=\"things.import\" title=\"Import\"/>\
         <rx-wizard-action id=\"w\" label=\"New\" route=\"things.new\" title=\"New\"><rx-wizard-step key=\"a\">A</rx-wizard-step></rx-wizard-action>",
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Routed), move |c| c.views_path = path).await;
    let res = app.get("/p").await;
    res.assert_ok();
    assert!(res.text().contains("action=\"/import\""), "{}", res.text());
    assert!(res.text().contains("action=\"/new\""), "{}", res.text());
}

/// `view:check` finds mistakes in mail templates and in the new tags with file and line.
#[renox::test]
async fn view_check_reports_mistakes_in_mail_and_kit_tags() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, body: &str| {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    write(
        "mail/fine.html",
        "<rx-mail subject=\"S\"><rx-mail-button url=\"/x\" label=\"Go\"/><rx-mail-divider/></rx-mail>",
    );
    write(
        "mail/button.html",
        "<rx-mail>\n<rx-mail-button label=\"Go\"/>\n</rx-mail>",
    );
    write(
        "mail/slot.html",
        "<rx-mail>x\n<rx-slot name=\"foter\">y</rx-slot></rx-mail>",
    );
    write("mail/loose.html", "<p>before</p>\n<rx-mail>x</rx-mail>");
    write(
        "mail/table.html",
        "<rx-mail>\n\n<rx-mail-table rows=\"r\"/></rx-mail>",
    );
    write("kit.html", "<rx-icon/>\n<rx-columns cuont=\"2\"/>");
    write(
        "wizard.html",
        "<rx-wizard-action id=\"w\" label=\"L\" action=\"/a\" title=\"T\">\n</rx-wizard-action>",
    );
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new(), move |c| c.views_path = path).await;
    let message = view_check_problems(&app);
    assert!(message.starts_with("6 template problem(s)"), "{message}");
    for expected in [
        "mail/button.html:2: <rx-mail-button> needs the attribute \"url\"",
        "mail/slot.html:2: <rx-mail> has no slot \"foter\"; did you mean \"footer\"?",
        "mail/loose.html:1: <rx-mail> must hold the whole template",
        "mail/table.html:3: <rx-mail-table> \"rows\" takes data",
        "kit.html:1: <rx-icon> needs the attribute \"name\"",
        "wizard.html:1: <rx-wizard-action> needs at least one <rx-wizard-step>",
    ] {
        assert!(message.contains(expected), "{expected}\n{message}");
    }
}

/// #517: `<rx-include>` is `{% include %}`: the same page, a missing optional one draws nothing.
#[renox::test]
async fn rx_include_matches_the_include_statement() {
    let files: &[(&str, &str)] = &[
        ("part.html", "<i>{{ who }}</i>"),
        (
            "m.html",
            "{% include \"part.html\" %}{% include name ignore missing %}{% include \"nope/\" ~ name ignore missing %}",
        ),
        (
            "r.html",
            "<rx-include template=\"part.html\"/><rx-include :template=\"name\" optional/><rx-include :template=\"'nope/' ~ name\" optional/>",
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    let path = dir.path().to_path_buf();
    let ctx = serde_json::json!({"who": "Ben", "name": "part.html"});
    let app =
        TestApp::with_config(App::new().module(Pair(ctx)), move |c| c.views_path = path).await;
    let (m, r) = (app.get("/m").await, app.get("/r").await);
    assert_eq!(m.text(), r.text());
    assert_eq!(r.text(), "<i>Ben</i><i>Ben</i>");
}

/// #518: `?attr="expr"` leaves the attribute out for false / none, writes it bare for true and
/// as text otherwise, on elements, on element components and on kit components.
#[renox::test]
async fn conditional_attributes_match_the_raw_expressions() {
    let ctx = serde_json::json!({"on": true, "off": false, "nothing": null, "path": "pay?a=1&b=2"});
    same(
        "<a href=\"/x\"{{ ' aria-current=\"page\"' | safe if on }}>a</a>\
         <a href=\"/y\"{{ ' aria-current=\"page\"' | safe if off }}>b</a>\
         <input{{ ' disabled' if on }}{{ ' readonly' if off }}>\
         <div{{ (' hx-get=\"' ~ (path | e) ~ '\"') | safe if on }} class=\"z\">c</div>\
         <div class=\"rx-stack\"{{ ' hx-swap-oob=\"true\"' | safe if on }}>d</div>",
        "<a href=\"/x\" ?aria-current=\"'page' if on\">a</a>\
         <a href=\"/y\" ?aria-current=\"'page' if off\">b</a>\
         <input ?disabled=\"on\" ?readonly=\"off\">\
         <div ?hx-get=\"path if on\" class=\"z\">c</div>\
         <rx-stack ?hx-swap-oob=\"'true' if on\">d</rx-stack>",
        ctx.clone(),
    )
    .await;
    // Undefined and none leave it out; text is escaped; the kit's own `attrs` skip them too.
    let page = render_with(
        "<p ?title=\"nothing\" ?lang=\"missing\" ?id=\"q\" ?hidden=\"on\">x</p>\
         <rx-input name=\"a\" label=\"A\" ?data-x=\"nothing\" ?data-y=\"'a&b'\" ?data-z=\"on\"/>",
        serde_json::json!({"nothing": null, "q": "a\"b<c", "on": true}),
    )
    .await;
    assert!(
        page.contains("<p id=\"a&quot;b&lt;c\" hidden>x</p>"),
        "{page}"
    );
    assert!(!page.contains("data-x"), "{page}");
    assert!(page.contains("data-y=\"a&amp;b\" data-z>"), "{page}");
}

/// #519: `<rx-tabs :items>` draws the tab list from data, `<rx-link-tab>` children can be looped.
#[renox::test]
async fn tabs_from_data_and_link_tab_children_match_the_macros() {
    let ctx = serde_json::json!({
        "tab_list": [["a", "Alpha"], ["b", "Beta"]],
        "keys": ["own", "all"], "cur": "all", "base": "/stock",
    });
    same(
        "{% from \"renox/ui.html\" import tabs, tab_panel, link_tabs %}\
         {{ tabs(\"t\", tab_list, label=\"Code\") }}\
         {% for tab in tab_list %}{% call tab_panel(\"t\", tab[0], selected=loop.first) %}{{ tab[1] }} panel{% endcall %}{% endfor %}\
         {{ link_tabs([[base ~ '?view=own', 'own'], [base ~ '?view=all', 'all']], current=base ~ '?view=all', label='Views') }}",
        "<rx-tabs id=\"t\" :items=\"tab_list\" label=\"Code\">\
         <rx-tab-panel rx-for=\"tab in tab_list\" id=\"t\" :key=\"tab[0]\" :selected=\"loop.first\">{{ tab[1] }} panel</rx-tab-panel>\
         </rx-tabs>\
         <rx-link-tabs label=\"Views\">\
         <rx-link-tab rx-for=\"k in keys\" :href=\"base ~ '?view=' ~ k\" :current=\"k == cur\">{{ k }}</rx-link-tab>\
         </rx-link-tabs>",
        ctx,
    )
    .await;
}

/// #520: a hand-written table with a footer is the kit's `table` with `foot`, and the same
/// footer in `<rx-table>` follows the body.
#[renox::test]
async fn simple_tables_and_footers_match_the_macro() {
    let ctx = serde_json::json!({"rows": [{"n": "Tyre", "q": 2}, {"n": "Bell", "q": 5}]});
    same(
        "{% from \"renox/ui.html\" import table %}\
         {% set foot %}<tr><th scope=\"row\">Total</th><td class=\"rx-num\">7</td></tr>{% endset %}\
         {% call table([\"Item\", [\"Qty\", \"num\"]], caption=\"Lines\", foot=foot) %}\
         {% for r in rows %}<tr><td>{{ r.n }}</td><td class=\"rx-num\">{{ r.q }}</td></tr>{% endfor %}{% endcall %}",
        "<rx-simple-table :head=\"['Item', ['Qty', 'num']]\" caption=\"Lines\">\
         <tr rx-for=\"r in rows\"><td>{{ r.n }}</td><td class=\"rx-num\">{{ r.q }}</td></tr>\
         <rx-slot name=\"footer\"><tr><th scope=\"row\">Total</th><td class=\"rx-num\">7</td></tr></rx-slot>\
         </rx-simple-table>",
        ctx.clone(),
    )
    .await;
    let page = render_with(
        "<rx-table :rows=\"rows\" as=\"r\"><rx-column label=\"Item\">{{ r.n }}</rx-column>\
         <rx-slot name=\"footer\"><tr><th>Total</th></tr></rx-slot></rx-table>",
        ctx,
    )
    .await;
    let page = normalize(&page);
    assert!(
        page.contains("</tbody><tfoot><tr><th>Total</th></tr></tfoot></table>"),
        "{page}"
    );
}

/// Renders `r.html` (the tags) with `ctx`.
async fn render_with(rx_src: &str, ctx: serde_json::Value) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("m.html"), "").unwrap();
    std::fs::write(dir.path().join("r.html"), rx_src).unwrap();
    let path = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(Pair(ctx)), move |c| c.views_path = path).await;
    let r = app.get("/r").await;
    r.assert_ok();
    r.text()
}
