//! M21b: components that see the request, the UI kit, toasts, fragments
//! with out-of-band swaps, htmx response headers, live validation.

use renox::prelude::*;
use renox::testing::TestApp;
use renox::{HxPushUrl, HxReswap, HxRetarget};

#[derive(serde::Deserialize)]
struct Signup {
    name: String,
    email: String,
}

impl Validate for Signup {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required().max(10);
        v.field("email", &self.email).required().email();
    }
}

#[derive(serde::Deserialize)]
struct Profile {
    name: String,
    plan: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    news: Option<String>,
}

impl Validate for Profile {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required();
        v.field("plan", &self.plan).required();
        let _ = (&self.tags, &self.news);
    }
}

#[derive(serde::Deserialize)]
struct Delivery {
    method: Option<String>,
    #[serde(default)]
    extras: Vec<String>,
    address: Option<String>,
    on: Option<renox::chrono::NaiveDate>,
}

impl Validate for Delivery {
    fn rules(&self, v: &mut Validator) {
        let courier = self.method.as_deref() == Some("courier");
        v.field("method", &self.method).required();
        v.field("address", &self.address).required_if(courier);
        let _ = (&self.extras, &self.on);
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Line {
    name: String,
    qty: i64,
}

impl Validate for Line {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required();
        v.field("qty", &self.qty).min(1);
    }
}

#[derive(serde::Deserialize)]
struct Order {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    lines: Vec<Line>,
    #[serde(default)]
    meta: renox::KeyValues,
    #[serde(default)]
    sizes: Vec<String>,
}

impl Validate for Order {
    fn rules(&self, v: &mut Validator) {
        v.field("tags", &self.tags).max(3);
        v.field("lines", &self.lines).required();
        v.nested("lines", &self.lines);
        v.field("meta", &self.meta).max(2);
    }
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/form", || async { view("form.html", context! {}) })
            .post("/signup", |Valid(form): Valid<Signup>| async move {
                (
                    Toast::success(format!("Welcome, {}!", form.name)),
                    Redirect::to("/form"),
                )
            })
            .post("/save", || async {
                (
                    Toast::info("Saved <draft>"),
                    Toast::error("But the mail bounced"),
                    HxRetarget("#summary".into()),
                    HxReswap("outerHTML".into()),
                    HxPushUrl("/saved".into()),
                    "ok",
                )
            })
            .post("/clear", || async {
                (Toast::success("Cleared"), HxRefresh)
            })
            .get("/list", || async {
                view("list.html", context! { count => 3 })
                    .fragment("rows")
                    .also("count")
            })
            .get("/fields", || async { view("fields.html", context! {}) })
            .get("/more", || async { view("more.html", context! {}) })
            .get("/stage3", || async { view("stage3.html", context! {}) })
            .get("/remote", || async {
                view(
                    "remote.html",
                    context! { chosen => vec![renox::select::SelectOption::new(7, "Coffee")] },
                )
            })
            .post("/order", |Valid(order): Valid<Order>| async move {
                renox::axum::Json(renox::serde_json::json!({
                    "tags": order.tags,
                    "lines": order.lines,
                    "meta": order.meta,
                    "sizes": order.sizes,
                }))
            })
            .post("/delivery", |Valid(_): Valid<Delivery>| async {
                Redirect::to("/more")
            })
            .post("/profile", |Valid(_): Valid<Profile>| async {
                Redirect::to("/fields")
            })
            .get("/components", || async {
                view("components.html", context! { note => "hi" })
            })
            .get("/icons", || async { view("icons.html", context! {}) })
            .get("/marks", || async { view("marks.html", context! {}) })
            .get("/passthrough", || async {
                view("passthrough.html", context! {})
            })
    }
}

fn views() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("components")).unwrap();
    let write = |name: &str, body: &str| std::fs::write(dir.path().join(name), body).unwrap();
    write(
        "form.html",
        r#"{% from "renox/ui.html" import input, button, form_errors %}{{ renox_ui() }}
<form method="post" action="/signup" data-live-validate>{{ csrf_field() }}{{ form_errors() }}
{{ input("name", "Name", required=true, hint="Up to 10 letters") }}
{{ input("email", "Email", type="email", required=true) }}
{{ input("nickname", "Nickname") }}{{ button("Sign up") }}</form>{{ toasts() }}"#,
    );
    write(
        "passthrough.html",
        r##"<rx-button class="mine" id="go" hx-target="#x" data-n="1" autofocus>Go</rx-button>"##,
    );
    write(
        "marks.html",
        r#"{% from "renox/ui.html" import sheet, action_sheet, confirm, wizard_action, wizard, wizard_step, tabs, repeater, select, tags_input, date_picker %}
{% set a = {"class": "extra", "hx-get": "/x", "data-test": "1"} %}
{% call sheet("s1", "Sheet", attrs=a) %}x{% endcall %}
{% call action_sheet("s2", "Act", "/x", "Act", attrs=a) %}x{% endcall %}
{{ confirm("s3", "Delete", "/x", "Sure", "Really?", attrs=a) }}
{% call wizard_action("s4", "Wiz", "/x", "Wiz", [["one", "One"]], attrs=a) %}{% call wizard_step("w4", "one") %}y{% endcall %}{% endcall %}
{% call wizard("w1", [["one", "One"]], "Go", attrs=a) %}{% call wizard_step("w1", "one") %}y{% endcall %}{% endcall %}
{{ tabs("t1", [["a", "A"], ["b", "B"]], attrs=a) }}
{% call(row, prefix) repeater("lines", "Lines", attrs=a) %}z{% endcall %}
{{ select("size", "Size", ["s"]) }}{{ tags_input("tags", "Tags") }}{{ date_picker("day", "Day") }}"#,
    );
    write(
        "fields.html",
        r#"{% from "renox/ui.html" import input, textarea, select, checkbox, radio, checkbox_list, form_grid, fieldset, form_errors %}
<form method="post" action="/profile">{{ csrf_field() }}{{ form_errors() }}
{% call form_grid(2) %}
{{ input("name", "Name", id="profile-name", required=true) }}
{{ input("price", "Price", type="number", prefix="$", suffix=".00", span=2) }}
{{ input("code", "Code", value="X1", readonly=true, datalist=["X1", ["X2", "Second"]]) }}
{{ textarea("bio", "Bio", id="profile-bio", disabled=true, span="full") }}
{{ select("size", "Size", ["s", "m"], id="profile-size", span=2) }}
{% endcall %}
{% call fieldset("Preferences", columns=2) %}
{{ radio("plan", "Plan", [["free", "Free"], ["pro", "Pro", "For teams"]], selected="free", required=true, inline=true) }}
{{ checkbox_list("tags", "Tags", [["a", "Alpha"], ["b", "Beta"], ["c", "Gamma"]], selected=["b"], columns=2) }}
{{ checkbox("news", "Email me news", checked=true) }}
{% endcall %}
</form>"#,
    );
    write(
        "more.html",
        r#"{% from "renox/ui.html" import input, toggle_buttons, file, date_picker, show_when, hide_when %}
<form method="post" action="/delivery" enctype="multipart/form-data">{{ csrf_field() }}
{{ input("password", "Password", type="password", value="secret", revealable=true) }}
{{ input("token", "Token", value="abc&1", readonly=true, copyable=true) }}
{{ toggle_buttons("method", "Method", [["pickup", "Pickup"], ["courier", "Courier", "Next day"]], selected="pickup", required=true) }}
{{ toggle_buttons("extras", "Extras", [["gift", "Gift wrap"], ["card", "Card"]], selected=["card"], multiple=true) }}
{% call show_when("method", "courier") %}{{ input("address", "Address") }}{% endcall %}
{% call hide_when("extras", ["gift", "card"]) %}<p>no extras</p>{% endcall %}
{{ file("photo", "Photo", accept="image/*", preview=true, current="/storage/photos/a.png", hint="PNG or JPEG") }}
{{ file("docs", "Documents", multiple=true, required=true) }}
{{ date_picker("on", "Delivery date", value="2026-10-02", min="2026-10-01", max="2026-12-31") }}
{{ date_picker("back", "Return date") }}
{{ date_picker("visit", "Visit", disabled_dates=["2026-10-07", "2026-10-08"], closed_weekdays=[0, 6]) }}
</form>"#,
    );
    write(
        "stage3.html",
        r#"{% from "renox/ui.html" import input, tags_input, repeater, key_value, select, wizard, wizard_step, form_errors %}
<form method="post" action="/order" data-live-validate>{{ csrf_field() }}{{ form_errors() }}
{% call wizard("w", [["items", "Items"], ["extra", "Extras"]], submit_label="Place order") %}
{% call wizard_step("w", "items") %}
{% call(row, prefix) repeater("lines", "Lines", rows=[{"name": "Coffee", "qty": 2}], item_label="Line", min=1, max=5) %}
{{ input(prefix ~ "[name]", "Name", value=row.name, required=true) }}
{{ input(prefix ~ "[qty]", "Quantity", type="number", value=row.qty) }}
{% endcall %}
{% call(row, prefix) repeater("pair", "Pair", rows=[{"n": "a"}, {"n": "b"}], max=2) %}
{{ input(prefix ~ "[n]", "N", value=row.n) }}
{% endcall %}
{% endcall %}
{% call wizard_step("w", "extra") %}
{{ tags_input("tags", "Tags", value=["new", "sale"], suggestions=["gift"]) }}
{{ key_value("meta", "Meta", value={"Color": "Red"}) }}
{{ select("sizes", "Sizes", [["s", "Small"], ["m", "Medium"], ["l", "Large"]], selected=["m"], multiple=true, searchable=true) }}
{% endcall %}
{% endcall %}
</form>"#,
    );
    write(
        "remote.html",
        r#"{% from "renox/ui.html" import select %}
<form method="post" action="/order">{{ csrf_field() }}
{{ select("category", "Category", chosen, selected=7, options_url="/options", editable=true, placeholder="None") }}
{{ select("sizes", "Sizes", [], multiple=true, options_url="/options") }}
</form>"#,
    );
    write(
        "list.html",
        r#"<h1>List</h1>{% block rows %}<tr id="r1"><td>one</td></tr>{% endblock %}
{% block count %}<span id="count" hx-swap-oob="true">{{ count }}</span>{% endblock %}"#,
    );
    // A component of the app's own: a macro in another file using request
    // helpers directly.
    write(
        "components/greeting.html",
        r#"{% macro greeting() %}<p class="greet">{{ t('ui.cancel') }}|{% if auth.check %}in{% else %}out{% endif %}|{{ request.path }}|{{ csrf_field() }}|{% if once('x') %}first{% endif %}{% if once('x') %}again{% endif %}</p>{% endmacro %}"#,
    );
    write(
        "components.html",
        r#"{% from "components/greeting.html" import greeting %}{{ greeting() }}{{ greeting() }}
{% from "renox/ui.html" import alert, badge, confirm, menu, menu_link, tabs, tab_panel, table, empty, checkbox, select, textarea %}
{{ alert("Heads up", kind="warning", title="Note") }}{{ badge("new", kind="success") }}
{{ confirm("del-1", "Delete", "/items/1", "Delete it?", "Can't be undone.") }}
{% call menu("More", id="m1") %}{{ menu_link("/a", "Open") }}{% endcall %}
{{ tabs("t", [["a", "A"], ["b", "B"]], selected="b") }}{% call tab_panel("t", "a") %}pa{% endcall %}{% call tab_panel("t", "b", selected=true) %}pb{% endcall %}
{% call table(["Name", ["Total", "num"]]) %}<tr><td>x</td><td class="rx-num">1</td></tr>{% endcall %}
{{ empty("Nothing yet", "Add one") }}{{ checkbox("agree", "I agree", switch=true) }}
{{ select("size", "Size", [["s", "Small"], ["m", "Medium"]], selected="m") }}{{ textarea("bio", "Bio", value="about") }}"#,
    );
    write(
        "icons.html",
        r#"{% from "renox/ui.html" import icon, button %}
<p id="plain">{{ icon("bike") }}</p>
<p id="small">{{ icon("truck", size=16) }}</p>
<p id="named">{{ icon("lock", label="Private & \"locked\"") }}</p>
<p id="old">{{ icon("trash") }}</p>
<p id="unknown">{{ icon("no-such-icon") }}</p>
{{ button("Ship", icon="truck") }}
{% from "renox/ui.html" import navbar, nav_search %}
{% call navbar("Shop", tabs=[{"href": "/", "label": "Home", "icon": "house", "active": true}, {"href": "/orders", "label": "Orders", "icon": "receipt", "badge": 2}, {"open": "more", "label": "More", "icon": "menu"}]) %}
{% call nav_search(label="Find") %}<form role="search"><input name="q"></form>{% endcall %}
{% endcall %}
{{ navbar("Plain") }}
{% from "renox/ui.html" import sidebar_link, stat, empty %}
<nav id="side">{{ sidebar_link("/stock", "Stock", icon="boxes") }}{{ sidebar_link("/plain", "Plain") }}</nav>
<div id="stat">{{ stat("Rentals", 4, icon="bike") }}</div>
<div id="empty">{{ empty("Nothing here", icon="inbox") }}</div>"#,
    );
    dir
}

async fn app() -> (TestApp, tempfile::TempDir) {
    let dir = views();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Auth::new()).module(Pages), move |c| {
        c.views_path = path
    })
    .await;
    (app, dir)
}

#[renox::test]
async fn components_see_the_request_and_refill_forms() {
    let (app, _dir) = app().await;
    let page = app.get("/form").await;
    page.assert_ok()
        .assert_see(r#"<label class="rx-label" for="rx-name">Name</label>"#)
        .assert_see(r#"aria-describedby="rx-name-hint rx-name-error""#)
        .assert_see(r#"<span class="rx-required">(optional)</span>"#)
        .assert_see(r#"/_renox/ui-"#);
    // A failed submit: the kit's fields show the errors and old input.
    app.request()
        .header("referer", "/form")
        .post(
            "/signup",
            &[("name", "Much too long a name"), ("email", "nope")],
        )
        .await
        .assert_redirect("/form");
    let page = app.get("/form").await;
    page.assert_see(r#"value="Much too long a name""#)
        .assert_see(r#"aria-invalid="true""#)
        .assert_see("data-rx-error-summary")
        .assert_see(r##"href="#rx-email""##);
    // The kit's CSS and JS are served, cached for good.
    let css = page.text();
    let url = css
        .split("href=\"")
        .filter_map(|rest| rest.split('"').next())
        .find(|url| url.starts_with("/_renox/ui-") && url.ends_with(".css"))
        .unwrap()
        .to_owned();
    let res = app.get(&url).await;
    res.assert_ok()
        .assert_header("cache-control", "public, max-age=31536000, immutable");
    assert!(res.text().contains("--rx-accent"));
    // The script names its parts loaded on demand (charts, the wizard, the
    // repeater, tags), each a module at a hashed URL, cached for good too.
    let url = css
        .split("src=\"")
        .filter_map(|rest| rest.split('"').next())
        .find(|url| url.starts_with("/_renox/ui-") && url.ends_with(".js"))
        .unwrap()
        .to_owned();
    let res = app.get(&url).await;
    res.assert_ok()
        .assert_header("cache-control", "public, max-age=31536000, immutable");
    let script = res.text();
    assert!(
        !script.contains("renox:parts"),
        "the parts' URLs are filled in"
    );
    for part in ["chart", "wizard", "repeater", "tags"] {
        let start = script
            .find(&format!("/_renox/ui-{part}-"))
            .unwrap_or_else(|| panic!("{part}'s URL in the script"));
        let part_url = &script[start..start + script[start..].find('"').unwrap()];
        assert!(part_url.ends_with(".js"), "{part_url}");
        let res = app.get(part_url).await;
        res.assert_ok()
            .assert_header("cache-control", "public, max-age=31536000, immutable")
            .assert_header("content-type", "text/javascript; charset=utf-8");
        assert!(
            res.text().contains(&format!("kit.ready(\"{part}\"")),
            "{part} says it's ready"
        );
    }

    let res = app.get("/components").await;
    let html = res.text();
    let greeting = html.split("<p class=\"greet\">").nth(1).unwrap();
    assert!(
        greeting.starts_with("Cancel|out|/components|<input type=\"hidden\" name=\"_token\""),
        "{greeting}"
    );
    assert_eq!(html.matches("first").count(), 1, "once() is per page");
    assert!(!html.contains("again"));
    res.assert_see(r#"role="alertdialog""#)
        .assert_see(r#"<input type="hidden" name="_method" value="DELETE">"#)
        .assert_see(r#"aria-controls="m1""#)
        .assert_see(
            r#"id="t-tab-b" data-rx-tab="b" aria-controls="t-panel-b" aria-selected="true""#,
        )
        .assert_see(r#"role="switch""#)
        .assert_see(r#"<option value="m" selected>Medium</option>"#)
        .assert_see(">about</textarea>");
}

#[renox::test]
async fn toasts_follow_the_next_page_or_ride_htmx() {
    let (app, _dir) = app().await;
    app.post("/signup", &[("name", "Ana"), ("email", "ana@example.test")])
        .await
        .assert_redirect("/form");
    let page = app.get("/form").await;
    page.assert_see("Welcome, Ana!")
        .assert_see(r#"role="status""#);
    app.get("/form").await.assert_dont_see("Welcome, Ana!");

    let res = app.htmx().post("/save", &[]).await;
    res.assert_ok()
        .assert_header("hx-retarget", "#summary")
        .assert_header("hx-reswap", "outerHTML")
        .assert_header("hx-push-url", "/saved");
    let trigger: renox::serde_json::Value =
        renox::serde_json::from_str(res.header("hx-trigger").unwrap()).unwrap();
    assert_eq!(
        trigger["renox:toast"]["toasts"][0]["message"],
        "Saved <draft>"
    );
    assert_eq!(trigger["renox:toast"]["toasts"][1]["kind"], "error");
    // An htmx refresh reloads the page, which would lose a toast sent in
    // HX-Trigger: it waits in the session for that page instead.
    let res = app.htmx().post("/clear", &[]).await;
    res.assert_header("hx-refresh", "true");
    assert!(res.header("hx-trigger").is_none());
    app.get("/form").await.assert_see("Cleared");
    // Not htmx: saved for the next page, escaped there.
    app.post("/save", &[]).await.assert_ok();
    app.get("/form")
        .await
        .assert_see("Saved &lt;draft&gt;")
        .assert_see(r#"role="alert""#)
        .assert_see("data-sticky");
}

#[renox::test]
async fn fragments_carry_out_of_band_blocks() {
    let (app, _dir) = app().await;
    let res = app.htmx().get("/list").await;
    let body = res.text();
    assert!(body.starts_with(r#"<tr id="r1">"#), "{body}");
    assert!(body.contains(r#"<span id="count" hx-swap-oob="true">3</span>"#));
    assert!(!body.contains("<h1>"));
    app.get("/list").await.assert_see("<h1>List</h1>");
}

#[renox::test]
async fn live_validation_checks_one_field_without_the_handler() {
    let (app, _dir) = app().await;
    let res = app
        .request()
        .header("x-renox-validate", "email")
        .post("/signup", &[("name", ""), ("email", "nope")])
        .await;
    res.assert_ok();
    let body: renox::serde_json::Value = res.json();
    assert_eq!(body["field"], "email");
    assert!(body["errors"][0].as_str().unwrap().contains("email"));
    // A valid field: no errors, and still the handler didn't run.
    let res = app
        .request()
        .header("x-renox-validate", "name")
        .post("/signup", &[("name", "Ana"), ("email", "ana@example.test")])
        .await;
    let body: renox::serde_json::Value = res.json();
    assert_eq!(body["errors"], renox::serde_json::json!([]));
    app.get("/form").await.assert_dont_see("Welcome");
}

#[renox::test]
async fn kit_texts_follow_the_locale() {
    let dir = views();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Auth::new()).module(Pages), move |c| {
        c.views_path = path;
        c.locale = "es".into();
        c.lang_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lang");
    })
    .await;
    // The app's `tests/lang/es.json` translates the kit's texts.
    app.get("/form").await.assert_see("(opcional)");
    app.get("/components").await.assert_see("Cancelar|");
}

#[renox::test]
async fn form_fields_choices_affixes_and_layout() {
    let (app, _dir) = app().await;
    let page = app.get("/fields").await;
    page.assert_ok()
        // Layout: a two-column grid, a field spanning both, a titled group.
        .assert_see(r#"<div class="rx-form-grid rx-cols-2">"#)
        .assert_see(r#"<div class="rx-field rx-span-2">"#)
        .assert_see(r#"<div class="rx-field rx-span-full">"#)
        .assert_see(r#"<legend class="rx-fieldset__legend">Preferences</legend>"#)
        // Every field takes its own id.
        .assert_see(r#"<label class="rx-label" for="profile-name">"#)
        .assert_see(r#"id="profile-bio""#)
        .assert_see(r#"id="profile-size""#)
        // Prefix and suffix are joined to the input and described by it.
        .assert_see(r#"<span class="rx-affix__text" id="rx-price-prefix">$</span>"#)
        .assert_see(r#"aria-describedby="rx-price-prefix rx-price-suffix rx-price-error""#)
        // Read-only, disabled, suggestions.
        .assert_see(r#"value="X1" list="rx-code-list" readonly"#)
        .assert_see(r#"<option value="X2">Second</option>"#)
        .assert_see(r#"id="profile-bio" name="bio" rows="4" disabled"#)
        // A radio group in a fieldset: the first option carries the group's id
        // (the error summary links to it), the others are numbered.
        .assert_see(r#"<legend class="rx-label">Plan</legend>"#)
        .assert_see(r#"<div class="rx-choices rx-choices--inline">"#)
        .assert_see(r#"id="rx-plan" name="plan" value="free" checked required"#)
        .assert_see(r#"id="rx-plan-2" name="plan" value="pro" required aria-describedby="rx-plan-2-detail""#)
        .assert_see(r#"<span class="rx-hint" id="rx-plan-2-detail">For teams</span>"#)
        // A checkbox list, ticked from `selected`.
        .assert_see(r#"<div class="rx-choices rx-cols-2">"#)
        .assert_see(r#"name="tags" value="a">"#)
        .assert_see(r#"name="tags" value="b" checked>"#)
        .assert_see(r#"id="rx-news" name="news" value="on" checked"#);

    // A failed submit refills what was sent: the other plan, two tags, and
    // the checkbox left unticked (which sends nothing) stays unticked.
    app.request()
        .header("referer", "/fields")
        .post(
            "/profile",
            &[("name", ""), ("plan", "pro"), ("tags", "a"), ("tags", "c")],
        )
        .await
        .assert_redirect("/fields");
    let page = app.get("/fields").await;
    page.assert_see(r#"name="plan" value="pro" checked"#)
        .assert_dont_see(r#"value="free" checked"#)
        .assert_see(r#"name="tags" value="a" checked>"#)
        .assert_dont_see(r#"name="tags" value="b" checked>"#)
        .assert_see(r#"name="tags" value="c" checked>"#)
        .assert_see(r#"id="rx-news" name="news" value="on" aria-describedby"#)
        // The summary links to the field by its name, whatever its id.
        .assert_see(r##"href="#rx-name" data-rx-field="name""##)
        .assert_see(r#"id="profile-name" name="name" type="text" value="" required aria-required="true" aria-invalid="true""#);

    // One tag only comes back as a string, not a list.
    app.request()
        .header("referer", "/fields")
        .post("/profile", &[("name", ""), ("tags", "b"), ("news", "on")])
        .await;
    let page = app.get("/fields").await;
    page.assert_see(r#"name="tags" value="b" checked>"#)
        .assert_dont_see(r#"name="tags" value="a" checked>"#)
        .assert_see(r#"id="rx-news" name="news" value="on" checked"#)
        .assert_see(r#"id="rx-plan" name="plan" value="free" required aria-invalid="true""#);
}

#[renox::test]
async fn the_icon_macro_draws_lucide_icons_hidden_unless_labelled() {
    let (app, _dir) = app().await;
    let page = app.get("/icons").await;
    let html = page.text();
    page.assert_ok()
        // Decorative by default: hidden from screen readers, 20 px, the text's colour.
        .assert_see(r#"<p id="plain"><svg class="rx-icon" xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" focusable="false" aria-hidden="true"><circle cx="18.5" cy="17.5" r="3.5"/>"#)
        .assert_see(r#"<p id="small"><svg class="rx-icon" xmlns="http://www.w3.org/2000/svg" width="16" height="16""#)
        // With a label: an image with that name, escaped.
        .assert_see(r#"focusable="false" role="img" aria-label="Private &amp; &quot;locked&quot;">"#)
        // The kit's older names still work; an unknown one draws nothing.
        .assert_see(r#"<p id="old"><svg class="rx-icon""#)
        .assert_see(r#"<p id="unknown"></p>"#)
        // A button's icon comes from the same set.
        .assert_see(r#"<span class="rx-button__icon" aria-hidden="true"><svg class="rx-icon""#);
    // A navbar with tabs: a tab bar after the header, the current tab marked,
    // a button for a sheet; the search behind its button on phones.
    page.assert_see(r#"<header class="rx-navbar rx-navbar--tabs">"#)
        .assert_see(r#"<nav class="rx-tabbar" aria-label="Sections">"#)
        .assert_see(r#"<a class="rx-tabbar__tab" href="/" aria-current="page"><span class="rx-tabbar__icon"><svg class="rx-icon""#)
        .assert_see(r#"<a class="rx-tabbar__tab" href="/orders"><span class="rx-tabbar__icon">"#)
        .assert_see(r#"<span class="rx-tabbar__label">Orders</span><span class="rx-tabbar__badge">2</span>"#)
        .assert_see(r#"<button class="rx-tabbar__tab" type="button" data-rx-open="more" aria-haspopup="dialog">"#)
        .assert_see(r#"aria-label="Find" aria-controls="rx-nav-search" aria-expanded="false" data-rx-search-toggle>"#)
        .assert_see(r#"<div class="rx-navbar__search" id="rx-nav-search"><form role="search">"#);
    // A sidebar link, a stat and an empty state take an icon too; without
    // one they are as before.
    page.assert_see(r#"<a class="rx-sidebar__link" href="/stock"><svg class="rx-icon" xmlns="http://www.w3.org/2000/svg" width="18" height="18""#)
        .assert_see(r#"<a class="rx-sidebar__link" href="/plain"><span>Plain</span></a>"#)
        .assert_see(r#"<div class="rx-stat rx-stat--icon"><span class="rx-stat__icon"><svg class="rx-icon""#)
        .assert_see(r#"<div class="rx-empty"><span class="rx-empty__icon"><svg class="rx-icon""#);
    // Without tabs the navbar is as before.
    assert_eq!(html.matches("rx-tabbar\"").count(), 1, "{html}");
    assert!(html.contains("<header class=\"rx-navbar\">"), "{html}");
    let named = html.split(r#"<p id="named">"#).nth(1).unwrap();
    assert!(
        !named[..named.find("</svg>").unwrap()].contains("aria-hidden"),
        "{named}"
    );
}

#[renox::test]
async fn form_fields_buttons_files_dates_and_conditions() {
    let (app, _dir) = app().await;
    let page = app.get("/more").await;
    let html = page.text();
    page.assert_ok()
        // A password is never printed, even with the reveal button.
        .assert_dont_see("secret")
        .assert_see(r#"data-rx-reveal="rx-password" aria-controls="rx-password" aria-pressed="false" aria-label="Show password" data-label-hide="Hide password""#)
        // Copy: the button copies the field's value, escaped in the page.
        .assert_see(r#"value="abc&amp;1""#)
        .assert_see(r#"data-rx-copy="rx-token" aria-label="Copy" data-label-done="Copied""#)
        // Toggle buttons: radios (or checkboxes) drawn as buttons.
        .assert_see(r#"<div class="rx-toggles">"#)
        .assert_see(r#"class="rx-toggle__input" type="radio" id="rx-method" name="method" value="pickup" checked required"#)
        .assert_see(r#"<span class="rx-visually-hidden" id="rx-method-2-detail">Next day</span>"#)
        .assert_see(r#"type="checkbox" id="rx-extras-2" name="extras" value="card" checked>"#)
        // Conditional groups carry the field and the values as JSON.
        .assert_see(r#"data-rx-show-when="method" data-rx-values='["courier"]'>"#)
        .assert_see(r#"data-rx-hide-when="extras" data-rx-values='["gift","card"]'>"#)
        // Files: the drop zone, the current file, required only without one.
        .assert_see(r#"<div class="rx-file" data-rx-file data-rx-preview>"#)
        .assert_see(r#"name="photo" type="file" accept="image/*""#)
        .assert_see(r#"<img class="rx-file__thumb" src="/storage/photos/a.png" alt="">"#)
        .assert_see(r#">a.png</a>"#)
        .assert_see("Choose a file or drop it here")
        .assert_see(r#"name="docs" type="file" multiple required"#)
        .assert_see("Choose files or drop them here")
        // The date picker: a text field, a button and a calendar in a popover.
        .assert_see(r#"name="on" type="text" inputmode="numeric" autocomplete="off" value="2026-10-02""#)
        .assert_see(r#"popovertarget="rx-on-calendar""#)
        .assert_see(r#"value="2026-10-02" min="2026-10-01" max="2026-12-31">"#)
        .assert_see(r#"<calendar-date class="rx-calendar" locale="en" first-day-of-week="1">"#)
        // Days that can't be chosen, for the script, with the message it shows.
        .assert_see(r#"data-rx-disabled-dates='["2026-10-07","2026-10-08"]' data-rx-closed-weekdays='[0,6]' data-rx-unavailable="That day can&#39;t be chosen: pick another one.""#);
    // A picker without them carries none.
    assert_eq!(html.matches("data-rx-disabled-dates").count(), 1, "{html}");
    // The calendar's script once per page, however many pickers.
    assert_eq!(html.matches("/_renox/cally-").count(), 1, "{html}");
    let tail = html.split("src=\"/_renox/cally-").nth(1).unwrap();
    let url = format!("/_renox/cally-{}", tail.split('"').next().unwrap());
    app.get(&url).await.assert_ok();

    // A failed submit: the courier pressed, no extras, the date kept.
    app.request()
        .header("referer", "/more")
        .post("/delivery", &[("method", "courier"), ("on", "2026-11-05")])
        .await
        .assert_redirect("/more");
    let page = app.get("/more").await;
    page.assert_see(r#"name="method" value="courier" checked"#)
        .assert_dont_see(r#"value="pickup" checked"#)
        .assert_dont_see(r#"name="extras" value="card" checked"#)
        .assert_see(r#"value="2026-11-05""#)
        .assert_see("The address field is required.");
}

#[renox::test]
async fn stage_two_texts_follow_the_locale() {
    let dir = views();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Auth::new()).module(Pages), move |c| {
        c.views_path = path;
        c.locale = "es".into();
        c.lang_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lang");
    })
    .await;
    app.get("/more")
        .await
        .assert_see("Mostrar contraseña")
        .assert_see(r#"aria-label="Copiar""#)
        .assert_see("Elige un archivo o suéltalo aquí")
        .assert_see("Archivo actual")
        .assert_see(r#"aria-label="Elige una fecha""#)
        .assert_see(r#"locale="es""#);
}

#[renox::test]
async fn repeaters_tags_key_values_and_wizards() {
    let (app, _dir) = app().await;
    let page = app.get("/stage3").await;
    page.assert_ok()
        // The repeater: a row from `rows`, numbered names, error slots keyed
        // with dots, and a template row for the script to copy.
        .assert_see(r#"data-rx-repeater="lines" data-rx-min="1" data-rx-max="5""#)
        .assert_see(r#"data-rx-row data-rx-index="0""#)
        .assert_see(r#"id="rx-lines-0-name" name="lines[0][name]" type="text" value="Coffee""#)
        .assert_see(r#"name="lines[0][qty]" type="number" value="2""#)
        .assert_see(r#"data-error-for="lines.0.name""#)
        .assert_see(r#"name="lines[__INDEX__][name]""#)
        .assert_see(r#"aria-label="Move up""#)
        // Under its `max`, the add button works; at it, it is disabled
        // before the repeater's script loads (#349).
        .assert_see(r#"type="button" data-rx-row-add><svg"#)
        .assert_see(r#"type="button" data-rx-row-add disabled><svg"#)
        // Tags: a chip and a hidden input per tag, then the box to type in.
        .assert_see(r#"<span class="rx-tag__text">new</span>"#)
        .assert_see(r#"<input type="hidden" name="tags" value="sale">"#)
        .assert_see(
            r#"name="tags" type="text" autocomplete="off" data-rx-tags-entry list="rx-tags-list""#,
        )
        // Key-value: a repeater over the map's pairs.
        .assert_see(r#"name="meta[0][key]" type="text" value="Color""#)
        .assert_see(r#"name="meta[0][value]" type="text" value="Red""#)
        // The searchable multiple select keeps a native select underneath.
        .assert_see(r#"name="sizes" multiple data-rx-combobox"#)
        .assert_see(r#"<option value="m" selected>Medium</option>"#)
        // The wizard: steps, panels and the three buttons.
        .assert_see(r#"data-rx-step-tab="extra""#)
        .assert_see(r#"id="w-step-items" data-rx-step="items""#)
        .assert_see("data-rx-wizard-next")
        .assert_see("Place order");

    // Nested names read into a Vec of structs, a KeyValues and lists.
    let res = app
        .post(
            "/order",
            &[
                ("tags", "a"),
                ("tags", "b"),
                ("tags", ""),
                ("lines[0][name]", "Coffee"),
                ("lines[0][qty]", "2"),
                ("lines[1][name]", "Tea"),
                ("lines[1][qty]", " 1 "),
                ("meta[0][key]", "Color"),
                ("meta[0][value]", "Red"),
                ("meta[1][key]", ""),
                ("meta[1][value]", ""),
                ("sizes", "s"),
                ("sizes", "l"),
            ],
        )
        .await;
    res.assert_ok();
    let body: renox::serde_json::Value = res.json();
    assert_eq!(
        body,
        renox::serde_json::json!({
            "tags": ["a", "b"],
            "lines": [{"name": "Coffee", "qty": 2}, {"name": "Tea", "qty": 1}],
            "meta": [["Color", "Red"]],
            "sizes": ["s", "l"],
        })
    );

    // Errors are keyed by the row: `lines.1.name`, labelled "name".
    let res = app
        .htmx()
        .post(
            "/order",
            &[
                ("lines[0][name]", "Coffee"),
                ("lines[0][qty]", "abc"),
                ("lines[1][name]", ""),
                ("lines[1][qty]", "0"),
            ],
        )
        .await;
    res.assert_status(422);
    let body: renox::serde_json::Value = res.json();
    assert_eq!(
        body["errors"]["lines.1.name"][0],
        "The name field is required."
    );
    assert!(
        body["errors"]["lines.0.qty"][0]
            .as_str()
            .unwrap()
            .contains("qty")
    );
    assert!(
        body["errors"]["lines.1.qty"][0]
            .as_str()
            .unwrap()
            .contains("at least 1")
    );

    // A plain post goes back with the rows sent, refilled, and their errors.
    app.request()
        .header("referer", "/stage3")
        .post(
            "/order",
            &[
                ("lines[0][name]", "Coffee"),
                ("lines[0][qty]", "2"),
                ("lines[1][name]", ""),
                ("lines[1][qty]", "3"),
                ("lines[2][name]", "Milk"),
                ("lines[2][qty]", "1"),
                ("meta[0][key]", "a"),
                ("meta[0][value]", "1"),
                ("meta[1][key]", "b"),
                ("meta[1][value]", "2"),
                ("meta[2][key]", "c"),
                ("meta[2][value]", "3"),
            ],
        )
        .await
        .assert_redirect("/stage3");
    let page = app.get("/stage3").await;
    page.assert_see(r#"name="lines[2][name]" type="text" value="Milk""#)
        .assert_see(r#"name="lines[1][qty]" type="number" value="3""#)
        .assert_see(r#"name="lines[1][name]" type="text" value="" required aria-required="true" aria-invalid="true""#)
        .assert_see("The name field is required.")
        .assert_see(r#"name="meta[2][key]" type="text" value="c""#)
        // The tags left as they were sent: none.
        .assert_dont_see(r#"<span class="rx-tag__text">new</span>"#)
        // The summary links to the row's field.
        .assert_see(r##"href="#rx-lines-1-name" data-rx-field="lines.1.name""##)
        // A row's error shows in the row only, not again under the list.
        .assert_see(r#"data-error-for="lines" aria-live="polite"></p>"#);
    // The list's own error (no rows at all) shows under it.
    app.request()
        .header("referer", "/stage3")
        .post("/order", &[("tags", "a")])
        .await
        .assert_redirect("/stage3");
    app.get("/stage3").await.assert_see(
        r#"data-error-for="lines" aria-live="polite">The lines field is required.</p>"#,
    );

    // Live validation names the row's field the way the page does.
    let res = app
        .request()
        .header("x-renox-validate", "lines[0][name]")
        .post("/order", &[("lines[0][name]", ""), ("lines[0][qty]", "1")])
        .await;
    let body: renox::serde_json::Value = res.json();
    assert_eq!(body["field"], "lines[0][name]");
    assert_eq!(body["errors"][0], "The name field is required.");
}

#[renox::test]
async fn selects_ask_the_server_for_options() {
    let (app, _dir) = app().await;
    let page = app.get("/remote").await;
    page.assert_ok()
        // Options as `SelectOption`s; the URL and texts for the script.
        .assert_see(r#"<option value="7" selected>Coffee</option>"#)
        .assert_see(r#"data-rx-options-url="/options""#)
        .assert_see(r#"data-rx-editable data-add="Add “:value”""#)
        .assert_see(r#"data-editing="Editing “:value”: Enter saves, Esc cancels.""#)
        .assert_see(r#"data-searching="Searching…""#);
    // `editable` only with an options URL.
    let html = page.text();
    assert_eq!(html.matches("data-rx-editable").count(), 1);

    // After a failed submit, values the page has no label for are kept, to
    // be looked up.
    app.request()
        .header("referer", "/remote")
        .post(
            "/order",
            &[("category", "9"), ("sizes", "s"), ("sizes", "l")],
        )
        .await;
    app.get("/remote")
        .await
        .assert_see(r#"<option value="7">Coffee</option>"#)
        .assert_see(r#"<option value="9" selected data-rx-unresolved>9</option>"#)
        .assert_see(r#"<option value="l" selected data-rx-unresolved>l</option>"#);
}

#[renox::test]
async fn the_kit_serves_its_fonts_and_preloads_the_text_one() {
    let (app, _dir) = app().await;
    let page = app.get("/form").await.text();
    assert!(
        page.contains(r#"<link rel="preload" href="/_renox/fonts/inter-latin-wght-4.1.woff2" as="font" type="font/woff2" crossorigin>"#),
        "{page}"
    );
    for font in [
        "/_renox/fonts/inter-latin-wght-4.1.woff2",
        "/_renox/fonts/poppins-latin-500-4.003.woff2",
        "/_renox/fonts/poppins-latin-600-4.003.woff2",
        "/_renox/fonts/poppins-latin-700-4.003.woff2",
    ] {
        let res = app.get(font).await;
        res.assert_ok()
            .assert_header("content-type", "font/woff2")
            .assert_header("cache-control", "public, max-age=31536000, immutable");
        assert!(res.body.starts_with(b"wOF2"), "{font} is a woff2 file");
    }
    // The stylesheet names them, and keeps the first look as a theme.
    let css_url = page
        .split("href=\"")
        .find_map(|rest| {
            rest.split('"')
                .next()
                .filter(|u| u.starts_with("/_renox/ui-") && u.ends_with(".css"))
        })
        .unwrap()
        .to_owned();
    let css = app.get(&css_url).await.text();
    assert!(css.contains("/_renox/fonts/poppins-latin-600-4.003.woff2"));
    assert!(css.contains(r#":root:where([data-rx-theme="classic"])"#));
    assert!(css.contains(r#":root:where([data-rx-theme="warm"])"#));
    assert!(css.contains("--rx-type-hero:"));
    assert!(css.contains("--rx-type-display:"));
}

#[renox::test]
async fn kit_components_mark_their_root_and_take_attrs() {
    let (app, _dir) = app().await;
    let page = app.get("/marks").await;
    page.assert_ok();
    let body = page.text();
    for name in [
        "sheet",
        "action-sheet",
        "confirm",
        "wizard-action",
        "wizard",
        "tabs",
        "repeater",
        "select",
        "tags-input",
        "date-picker",
    ] {
        assert!(
            body.contains(&format!(r#"data-rx-component="{name}""#)),
            "missing {name}"
        );
    }
    // `class` is merged into the root's own, the rest lands as attributes.
    page.assert_see(r#"class="rx-sheet extra""#)
        .assert_see(r#"class="rx-wizard extra""#)
        .assert_see(r#"class="rx-segmented extra""#)
        .assert_see(r#"hx-get="/x""#)
        .assert_see(r#"data-test="1""#)
        .assert_see(r#"data-rx-tab="a""#);
    for tag in body
        .split('<')
        .filter(|t| t.contains("data-rx-component=\"action-sheet\""))
    {
        assert_eq!(tag.matches("class=").count(), 1, "{tag}");
        assert!(tag.contains(r#"class="rx-sheet extra""#), "{tag}");
        assert!(tag.contains(r#"hx-get="/x""#), "{tag}");
    }
}

#[renox::test]
async fn compiled_components_pass_other_attributes_to_the_root() {
    let (app, _dir) = app().await;
    let body = app.get("/passthrough").await.text();
    let tag = body.split('<').find(|t| t.starts_with("button")).unwrap();
    assert_eq!(tag.matches("class=").count(), 1, "{tag}");
    assert!(
        tag.contains(r#"class="rx-button rx-button--primary mine""#),
        "{tag}"
    );
    for a in [
        r##"hx-target="#x""##,
        r#"id="go""#,
        r#"data-n="1""#,
        " autofocus",
    ] {
        assert!(tag.contains(a), "{a} in {tag}");
    }
}
