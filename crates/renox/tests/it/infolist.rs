//! The UI kit's infolists (`infolist`, `entry`, `repeatable`) and the
//! filters behind them: `money`, `since`, `words`, `markdown`.

use renox::prelude::*;
use renox::testing::TestApp;

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "infolist-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/order", || async {
            let placed =
                (renox::chrono::Utc::now() - renox::chrono::TimeDelta::hours(3)).to_rfc3339();
            let meta: renox::KeyValues = [("Origin", "Colombia"), ("Roast", "Medium")]
                .into_iter()
                .collect();
            view(
                "order.html",
                context! {
                    placed => placed,
                    meta => meta,
                    lines => vec![
                        context! { name => "Coffee", qty => 2 },
                        context! { name => "Tea", qty => 1 },
                    ],
                },
            )
        })
    }
}

const ORDER: &str = r##"{% from "renox/ui.html" import infolist, entry, repeatable %}
{% call infolist(columns=2) %}
{{ entry("Number", "INV-<1>", copyable=true) }}
{{ entry("Status", "paid", badge={"paid": "success"}, labels={"paid": "Paid"}) }}
{{ entry("Placed", placed, format="since") }}
{{ entry("Day", "2026-10-02", format="date", date_format="%d/%m/%Y") }}
{{ entry("Total", 125050, format="money") }}
{{ entry("Price", 75000, format="money", currency="idr") }}
{{ entry("Weight", 1234.5, format="number", decimals=1, suffix="kg") }}
{{ entry("Note", "**Hi** <b>x</b>", format="markdown", span="full") }}
{{ entry("Nothing", none) }}
{{ entry("Blank", "", placeholder="Not set") }}
{{ entry("Tags", ["a", "b", "c", "d"], badge=true, limit_list=2) }}
{{ entry("Steps", ["x", "y"], list="bullets") }}
{{ entry("Names", ["Ana", "Ben"]) }}
{{ entry("Active", true, format="bool") }}
{{ entry("Archived", false, format="bool") }}
{{ entry("Color", "#ff0000", format="color") }}
{{ entry("Avatar", "/a.png", format="image", circular=true) }}
{{ entry("Meta", meta, format="key_value") }}
{{ entry("Site", "renox.dev", url="https://renox.dev", new_tab=true, hint="The docs") }}
{{ entry("Summary", "one two three four", words=2) }}
{{ entry("Long", "abcdefghij", limit=4, tooltip="abcdefghij") }}
{% call entry("Custom") %}<em>custom</em>{% endcall %}
{% call(line) repeatable("Items", lines, columns=2) %}{{ entry("Product", line.name) }}{{ entry("Qty", line.qty) }}{% endcall %}
{% call(line) repeatable("Refunds", []) %}{{ entry("Product", line.name) }}{% endcall %}
{% endcall %}
{% call infolist(inline=true) %}{{ entry("Inline", "yes", hide_label=true) }}{% endcall %}"##;

async fn app(locale: &str) -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("order.html"), ORDER).unwrap();
    let (path, locale) = (dir.path().to_path_buf(), locale.to_owned());
    let app = TestApp::with_config(App::new().module(Pages), move |c| {
        c.views_path = path;
        c.locale = locale;
        c.lang_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lang");
    })
    .await;
    (app, dir)
}

#[renox::test]
async fn entries_format_their_values() {
    let (app, _dir) = app("en").await;
    let page = app.get("/order").await;
    let html = page.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"<dl class="rx-infolist rx-cols-2">"#);
    has(r#"<dt class="rx-entry__label">Number</dt>"#);
    has("INV-&lt;1&gt;");
    has(r#"data-rx-copy-text="INV-&lt;1&gt;""#);
    has(r#"<span class="rx-badge rx-badge--success">Paid</span>"#);
    has(">3 hours ago</time>");
    has(">02/10/2026<");
    has("$1,250.50");
    has("Rp 75,000");
    has("1,234.5<span class=\"rx-entry__affix\">kg</span>");
    has("<strong>Hi</strong> &lt;b&gt;x&lt;/b&gt;");
    has(r#"<span class="rx-entry__empty">—</span>"#);
    has(r#"<span class="rx-entry__empty">Not set</span>"#);
    has(r#"<span class="rx-badge">a</span><span class="rx-badge">b</span>"#);
    has("Show 2 more</summary>");
    has(r#"<span class="rx-badge">c</span><span class="rx-badge">d</span>"#);
    has(r#"<ul class="rx-entry__list rx-entry__list--bullets"><li>x</li><li>y</li></ul>"#);
    has("Ana, Ben");
    has("rx-entry__bool--yes");
    has("</svg>Yes</span>");
    has("</svg>No</span>");
    has(r#"style="background: #ff0000""#);
    has(r#"class="rx-entry__image rx-entry__image--circular" src="/a.png""#);
    has(r#"<tr><th scope="row">Origin</th><td>Colombia</td></tr>"#);
    has(
        r#"<a class="rx-link" href="https://renox.dev" target="_blank" rel="noopener">renox.dev</a>"#,
    );
    has(r#"<dd class="rx-hint rx-entry__hint">The docs</dd>"#);
    has("one two…");
    has(r#"title="abcdefghij">abcd…"#);
    has("<em>custom</em>");
    has(r#"<li class="rx-repeatable__item"><dl class="rx-infolist rx-cols-2">"#);
    has("Coffee");
    has("Tea");
    has(r#"<dl class="rx-infolist rx-cols-1 rx-infolist--inline">"#);
    has(r#"<dt class="rx-entry__label rx-visually-hidden">Inline</dt>"#);
    assert_eq!(html.matches("rx-repeatable__item").count(), 2);

    // `since` reads the clock tests move.
    app.travel(std::time::Duration::from_secs(2 * 86_400));
    app.get("/order").await.assert_see(">2 days ago</time>");
}

#[renox::test]
async fn entries_follow_the_locale() {
    // Texts from the app's `tests/lang/es.json`, numbers in Spanish style.
    let (app, _dir) = app("es").await;
    app.get("/order")
        .await
        .assert_see(">hace 3 horas</time>")
        .assert_see("$1.250,50")
        .assert_see("Rp 75.000")
        .assert_see("1.234,5")
        .assert_see("</svg>Sí</span>")
        .assert_see("</svg>No</span>")
        .assert_see("Mostrar 2 más");
}

#[renox::test]
async fn the_currency_comes_from_the_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("price.html"),
        "{{ 1250 | money }}|{{ 1250 | money(currency='IDR') }}|{{ 1299 | money(currency='usd') }}|{{ 500 | money(currency='JPY') }}|{{ 125050 | money(divide_by=100) }}|{{ 1250 | money(divide_by=1) }}|{{ 300 | money(decimals=0) }}",
    )
    .unwrap();
    struct Price;
    impl Module for Price {
        fn name(&self) -> &'static str {
            "price"
        }
        fn routes(&self) -> Routes {
            Routes::new().get("/price", || async { view("price.html", context! {}) })
        }
    }
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Price), move |c| {
        c.views_path = path;
        c.currency = "EUR".into();
    })
    .await;
    app.get("/price")
        .await
        .assert_see("€12.50|Rp 1,250|$12.99|¥500|€1,250.50|€1,250.00|€3");
}

#[renox::test]
async fn entries_carry_actions_beside_their_value() {
    // Prefix and suffix actions (#150): a link, a form posted with its
    // method, an htmx button and a labelled one.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("actions.html"),
        r##"{% from "renox/ui.html" import infolist, entry %}
{% call infolist() %}
{{ entry("Email", "ana@example.com", copyable=true,
     prefix_actions=[{"label": "Open profile", "icon": "external", "url": "/users/1", "new_tab": true}],
     suffix_actions=[
       {"label": "Verify", "icon": "settings", "action": "/users/1/verify"},
       {"label": "Remove", "icon": "trash", "action": "/users/1/email", "method": "delete", "variant": "danger"},
       {"label": "Refresh", "icon": "refresh", "attrs": {"hx-post": "/users/1/refresh", "hx-target": "closest dl"}},
       {"label": "Resend", "disabled_reason": "Sent a minute ago"}]) }}
{{ entry("Plain", "x") }}
{% call entry("Called", suffix_actions=[{"label": "Edit", "url": "/edit"}]) %}<em>called</em>{% endcall %}
{% endcall %}"##,
    )
    .unwrap();
    struct Actions;
    impl Module for Actions {
        fn name(&self) -> &'static str {
            "actions"
        }
        fn routes(&self) -> Routes {
            Routes::new().get("/actions", || async { view("actions.html", context! {}) })
        }
    }
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Actions), move |c| c.views_path = path).await;
    // Git on Windows checks the kit out with CRLF line endings.
    let html = app
        .get("/actions")
        .await
        .assert_ok()
        .text()
        .replace("\r\n", "\n");
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"<div class="rx-entry rx-entry--actions">"#);
    has(r#"<span class="rx-entry__actions rx-entry__actions--prefix">"#);
    has(
        r#"href="/users/1" aria-label="Open profile" data-rx-tip="Open profile" target="_blank" rel="noopener""#,
    );
    has(
        r#"<form class="rx-entry__action-form" method="post" action="/users/1/verify"><input type="hidden" name="_token""#,
    );
    has(r#"type="submit" aria-label="Verify""#);
    has(r#"action="/users/1/email">"#);
    has(r#"<input type="hidden" name="_method" value="DELETE">"#);
    has("rx-icon-button--danger");
    has(r#"hx-post="/users/1/refresh" hx-target="closest dl""#);
    has(r#"aria-disabled="true" data-rx-tip="Sent a minute ago""#);
    has(r#"<span class="rx-button__label">Resend</span>"#);
    // The prefix comes before the value, the suffix after the copy button.
    let (prefix, value, copy, suffix) = (
        html.find("rx-entry__actions--prefix").unwrap(),
        html.find("ana@example.com").unwrap(),
        html.find("rx-entry__copy").unwrap(),
        html.find("rx-entry__actions--suffix").unwrap(),
    );
    assert!(prefix < value && value < copy && copy < suffix, "{html}");
    // Entries without actions are unchanged; a called entry gets them too.
    has(r#"<div class="rx-entry">
  <dt class="rx-entry__label">Plain</dt>"#);
    has(r#"<em>called</em><span class="rx-entry__actions rx-entry__actions--suffix">"#);
    has(r#"href="/edit""#);
}
