//! The blocks (#347): what each renders, the files a page loads, the form
//! blocks' round trip through `Valid<T>` (with errors and old input), the
//! month calendar's arithmetic, the English texts and an app's
//! translations of them, and an app's own copy of the macros. Their
//! behaviour in a browser is tests/browser/blocks.test.mjs.

use renox::prelude::*;
use renox::testing::TestApp;
use renox_blocks::{Blocks, TEXTS};
use serde::Deserialize;

const PAGE: &str = r##"{% from "renox-blocks/blocks.html" import quantity, range_slider, keypad, swatches, datetime_range, gallery, history, compare_plans, month_calendar, availability, kanban, receipt %}
<html><head>{{ renox_head() }}</head><body>
<form method="post" action="/order">
{{ csrf_field() }}
{{ quantity("qty", 2, min=1, max=5, label="Bikes") }}
{{ range_slider("low", "high", 0, 1000, step=50, value_min=100, value_max=600, label="Price", suffix=" km") }}
<input id="rx-paid" name="paid">
{{ keypad("rx-paid", decimal=true, enter_label="Pay") }}
{{ swatches("size", "Frame size", [{"value": "S", "label": "S", "note": "150–165 cm"}, {"value": "M", "label": "M"}, {"value": "XL", "label": "XL", "disabled": true}], selected="M", required=true) }}
{{ swatches("colour", "Colour", [{"value": "teal", "label": "Teal", "color": "#0b6e66"}], kind="colour") }}
{{ datetime_range("starts_at", "ends_at", label="Rental", min="2026-10-01", max="2026-12-31", step=30, opens=9, closes=10) }}
</form>
{{ gallery([{"src": "/a.svg", "alt": "Side", "caption": "From the side"}, {"src": "/b.svg", "alt": "Front"}], id="photos") }}
{{ history([{"time": "2026-10-06T09:12:00Z", "title": "Checked in", "body": "Brakes **squeal**", "kind": "warning", "by": "Marta"}, {"time": "2026-10-06", "title": "Done", "when": "an hour ago"}], label="Service") }}
{{ compare_plans([{"key": "basic", "name": "Basic", "price": 2900, "interval": "month", "url": "/p/basic", "perks": ["One visit"]}, {"key": "pro", "name": "Pro", "price_label": "Ask us"}], [{"label": "Loan bike", "values": {"basic": false, "pro": true}}, {"label": "Visits", "values": {"basic": "1", "pro": "4"}}], highlight="pro") }}
{{ month_calendar(month, [{"date": "2026-02-03", "time": "09:30", "title": "Tune-up", "url": "/v/1", "kind": "info"}, {"date": "2026-02-03T12:00:00", "title": "Rental"}], url="/visits?store=2", today="2026-02-03") }}
{{ availability(["09:00", "10:00", "11:00"], [{"label": "Trail 5", "note": "M", "slots": [{"state": "booked", "span": 2, "title": "Ana R."}, {"state": "free", "action": "/book", "fields": {"bike": 12}}]}, {"label": "City 3", "slots": [{"state": "free", "url": "/rent?at=9"}, {"state": "closed", "span": 2}]}], corner="Bike") }}
{{ kanban("jobs", [{"key": "waiting", "title": "Waiting", "cards": [{"id": 7, "title": "Tune-up", "subtitle": "City 3", "badge": "Today", "badge_kind": "warning"}]}, {"key": "ready", "title": "Ready", "cards": []}], url="/jobs/move", values={"store": 2}) }}
{% call receipt([{"name": "Helmet", "quantity": 1, "price": 4900}], totals=[{"label": "Total", "amount": 4900, "strong": true}], title="Bike Shop", number="R-1") %}R-1{% endcall %}
</body></html>"##;

#[derive(Debug, Deserialize, Validate)]
struct Order {
    #[validate(required, between(1, 5))]
    qty: Option<i64>,
    #[validate(required)]
    low: Option<i64>,
    #[validate(required)]
    high: Option<i64>,
    #[validate(required, one_of(&["S", "M"]))]
    size: Option<String>,
    starts_at: Option<renox::chrono::NaiveDateTime>,
    ends_at: Option<renox::chrono::NaiveDateTime>,
}

#[derive(Deserialize)]
struct MonthQuery {
    month: Option<String>,
}

struct Shop;

impl Module for Shop {
    fn name(&self) -> &'static str {
        "shop"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/order", |Query(q): Query<MonthQuery>| async move {
                view(
                    "page.html",
                    context! { month => q.month.unwrap_or_else(|| "2026-02".into()) },
                )
            })
            .post("/order", |Valid(order): Valid<Order>| async move {
                Json(json!({
                    "qty": order.qty,
                    "low": order.low,
                    "high": order.high,
                    "size": order.size,
                    "starts_at": order.starts_at.map(|t| t.to_string()),
                    "ends_at": order.ends_at.map(|t| t.to_string()),
                }))
            })
    }
}

async fn app() -> TestApp {
    TestApp::new(page_app()).await
}

fn page_app() -> App {
    App::new()
        .module(Blocks::new())
        .module(Shop)
        .templates(|env| env.add_template("page.html", PAGE).unwrap())
}

#[renox::test]
async fn every_block_renders_and_the_files_load_once() {
    let app = app().await;
    let html = app.get("/order").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");

    // quantity: a number field of the name, the buttons out of the Tab order.
    has(
        r#"<input class="rx-quantity__input" id="rx-qty" name="qty" type="number" inputmode="numeric" value="2" min="1" max="5" step="1""#,
    );
    has(r#"data-rx-quantity-step="1" aria-label="One more (Bikes)" aria-controls="rx-qty""#);
    has(r#"tabindex="-1" data-rx-quantity-step="-1""#);
    // range_slider: two range inputs, the chosen part drawn from 0.1 to 0.6.
    has(r#"style="--rx-range-from: 0.1; --rx-range-to: 0.6""#);
    has(
        r#"type="range" id="rx-low-range-min" name="low" min="0" max="1000" step="50" value="100""#,
    );
    has(r#"aria-label="Highest Price" aria-valuetext="600 km""#);
    // keypad: one Tab stop, a decimal key, the Enter key's own text.
    has(r#"data-rx-keypad="rx-paid""#);
    has(r#"data-rx-keypad-key="7" tabindex="0""#);
    has(r#"data-rx-keypad-key="decimal" tabindex="-1" aria-label="Decimal point""#);
    has(r#"data-rx-keypad-key="enter" tabindex="-1">Pay</button>"#);
    assert_eq!(
        html.matches(r#"data-rx-keypad-key=""#).count(),
        html.matches(r#"data-rx-keypad-key="7" tabindex="0">"#)
            .count()
            + html.matches(r#"" tabindex="-1""#).count()
            - 2,
        "one key in the Tab order (the stepper's two buttons are the other -1s)"
    );
    // swatches: radios, the chosen one checked, the sold-out one said so.
    has(r#"type="radio" name="size" value="M" checked required"#);
    has(r#"name="size" value="XL" disabled required"#);
    has("(sold out)");
    has(r##"<circle cx="12" cy="12" r="11" fill="#0b6e66""##);
    has(r#"<span class="rx-required">(optional)</span>"#);
    // datetime_range: two hidden fields, visible ones outside the form.
    has(r#"<input type="hidden" name="starts_at" value="" data-rx-datetime-value="start">"#);
    has(r#"form="rx-starts_at-range-none" data-rx-datetime-time="end""#);
    has(r#"<option value="09:30">09:30</option>"#);
    has(r#"<option value="10:00">10:00</option>"#);
    assert!(
        !html.contains(r#"<option value="10:30">"#),
        "the last time is `closes`"
    );
    has("From (day)");
    // gallery: a carousel with thumbnails and an enlarged view in a sheet.
    has(r#"aria-roledescription="carousel""#);
    has(r#"aria-label="Photo 2 of 2""#);
    has(r#"aria-label="Photo 2: Front""#);
    has(r#"data-rx-open="photos-zoom""#);
    has(r#"<p class="rx-gallery__caption">From the side</p>"#);
    // history: a list with times, a kind said in words, Markdown bodies.
    has(r#"<ol class="rx-history" role="list" aria-label="Service" data-rx-history>"#);
    has(r#"<time datetime="2026-10-06T09:12:00Z">2026-10-06 09:12</time>"#);
    has("needs attention");
    has("Brakes <strong>squeal</strong>");
    has(">an hour ago</time>");
    // compare_plans: the highlighted plan, a price label, ticks said in words.
    has(r#"<article class="rx-plan rx-plan--highlight""#);
    has("Most popular");
    has(r#"<span class="rx-plan__amount">Ask us</span>"#);
    has(r#"<span class="rx-plan__interval"> / month</span>"#);
    has("Not included");
    has(r#"<td class="rx-plans__col rx-plans__col--highlight">4</td>"#);
    // availability: a table whose free slots say what they book.
    has(r#"<td class="rx-availability__slot rx-availability__slot--booked" colspan="2">"#);
    has(r#"<form class="rx-availability__form" method="post" action="/book">"#);
    has(r#"<input type="hidden" name="bike" value="12">"#);
    has(r#"aria-label="Book Trail 5 at 11:00""#);
    has(r#"aria-label="Book City 3 at 09:00""#);
    has(r#"<th scope="col" class="rx-availability__corner">Bike</th>"#);
    // kanban: the hidden htmx form, the cards, the texts for the live region.
    has(r#"hx-post="/jobs/move" hx-trigger="rx:kanban-move" hx-swap="none" data-rx-kanban-form>"#);
    has(r#"<input type="hidden" name="store" value="2">"#);
    has(r#"<li class="rx-kanban__card" data-rx-kanban-card="7">"#);
    has(r#"<span class="rx-badge rx-badge--warning">Today</span>"#);
    has("Picked up :card, in :column, position :position of :total.");

    // The files load once, however many blocks the page has.
    assert_eq!(html.matches("data-renox-blocks").count(), 1, "{html}");
    assert_eq!(html.matches("/_renox/blocks/blocks-").count(), 2);
    let css = between(
        &html,
        "<link rel=\"stylesheet\" href=\"/_renox/blocks/",
        "\"",
    );
    let js = between(&html, "<script type=\"module\" src=\"/_renox/blocks/", "\"");
    let mut files = vec![
        (
            format!("/_renox/blocks/{css}"),
            "text/css; charset=utf-8",
            ".rx-kanban__card",
        ),
        (
            format!("/_renox/blocks/{js}"),
            "text/javascript; charset=utf-8",
            "data-renox-blocks",
        ),
    ];
    for part in [
        "gallery", "range", "quantity", "keypad", "kanban", "datetime", "history", "receipt",
    ] {
        let path = between(&html, &format!(" data-{part}=\""), "\"");
        assert!(
            path.starts_with(&format!("/_renox/blocks/{part}-")),
            "{path}"
        );
        files.push((
            path,
            "text/javascript; charset=utf-8",
            "export function setup",
        ));
    }
    for (path, kind, text) in files {
        let res = app.get(&path).await;
        res.assert_ok()
            .assert_header("content-type", kind)
            .assert_header("cache-control", "public, max-age=31536000, immutable")
            .assert_see(text);
        assert_eq!(res.header("set-cookie"), None, "{path}");
    }
}

#[renox::test]
async fn the_month_calendar_works_out_its_grid() {
    let app = app().await;
    // February 2026 has 28 days and starts on a Sunday: the seventh column
    // when weeks start on Monday.
    app.get("/order?month=2026-02")
        .await
        .assert_ok()
        .assert_see("February 2026")
        .assert_see(r#"<time datetime="2026-02-28">"#)
        .assert_dont_see(r#"datetime="2026-02-29""#)
        .assert_see("--rx-month-start: 7")
        .assert_see(r#"<span class="rx-month__weekday">Sunday </span><span class="rx-month__number">1</span>"#)
        // Two events on the 3rd (a date-time counts by its day), today marked.
        .assert_see(r#"<a class="rx-month__event-link" href="/v/1"><span class="rx-month__time">09:30</span> <span class="rx-month__event-title">Tune-up</span></a>"#)
        .assert_see(r#"<span class="rx-month__event-title">Rental</span>"#)
        .assert_see("rx-month__day--today")
        .assert_see(r#"<span class="rx-month__today">Today</span>"#)
        // The months around it keep the URL's own query.
        .assert_see("/visits?store=2&amp;month=2026-01")
        .assert_see("/visits?store=2&amp;month=2026-03")
        .assert_dont_see("Nothing booked this month.");
    // 2024 was a leap year; February started on a Thursday. Another month
    // links back to today's and says nothing is booked.
    app.get("/order?month=2024-02")
        .await
        .assert_see(r#"datetime="2024-02-29""#)
        .assert_see("--rx-month-start: 4")
        .assert_see("/visits?store=2&amp;month=2026-02\">Today</a>")
        .assert_see("Nothing booked this month.");
    // December links to January of the next year.
    app.get("/order?month=2026-12")
        .await
        .assert_see("month=2027-01")
        .assert_see("month=2026-11");
}

#[renox::test]
async fn the_form_blocks_send_plain_fields() {
    let app = app().await;
    app.get("/order").await;
    app.post(
        "/order",
        &[
            ("qty", "3"),
            ("low", "100"),
            ("high", "650"),
            ("size", "S"),
            ("starts_at", "2026-10-09T10:00"),
            ("ends_at", "2026-10-10T12:30"),
        ],
    )
    .await
    .assert_ok()
    .assert_json_path("qty", 3)
    .assert_json_path("high", 650)
    .assert_json_path("size", "S")
    .assert_json_path("starts_at", "2026-10-09 10:00:00")
    .assert_json_path("ends_at", "2026-10-10 12:30:00");
}

#[renox::test]
async fn errors_and_old_input_come_back_to_the_blocks() {
    let app = app().await;
    app.get("/order").await;
    app.request()
        .header("referer", "/order")
        .post(
            "/order",
            &[
                ("qty", "9"),
                ("low", "250"),
                ("high", "300"),
                ("size", "XL"),
            ],
        )
        .await
        .assert_redirect("/order");
    let html = app.get("/order").await.text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    // The stepper keeps what was sent, marked invalid with its message.
    has(r#"name="qty" type="number" inputmode="numeric" value="9""#);
    has(r#"step="1" aria-invalid="true""#);
    has(r#"data-error-for="qty" aria-live="polite">The qty must be between 1 and 5.</p>"#);
    // The range keeps its handles where they were.
    has(r#"name="low" min="0" max="1000" step="50" value="250""#);
    has(r#"name="high" min="0" max="1000" step="50" value="300""#);
    // The chips: the value sent is chosen again, and said to be wrong.
    has(r#"name="size" value="XL" checked disabled required aria-invalid="true""#);
    has(r#"<p class="rx-error" id="rx-size-error" data-error-for="size" aria-live="polite">"#);
}

#[renox::test]
async fn an_app_translates_the_blocks_texts() {
    let dir = std::env::temp_dir().join(format!("renox-blocks-{}", renox::random_token()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("es.json"),
        r#"{"blocks": {"quantity": {"increase": "Uno más (:label)"}, "gallery": {"position": "Foto :n de :total"}, "calendar": {"month_2": "Febrero"}}}"#,
    )
    .unwrap();
    let lang = dir.clone();
    let app = TestApp::with_config(page_app(), move |c| {
        c.locale = "es".into();
        c.lang_path = lang;
    })
    .await;
    app.get("/order")
        .await
        .assert_see(r#"aria-label="Uno más (Bikes)""#)
        .assert_see(r#"aria-label="Foto 2 de 2""#)
        .assert_see("Febrero 2026")
        // What the app doesn't translate stays English.
        .assert_see(r#"aria-label="One less (Bikes)""#)
        .assert_see("Most popular");
    let _ = std::fs::remove_dir_all(dir);
}

struct Mine;

impl Module for Mine {
    fn name(&self) -> &'static str {
        "mine"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/mine", || async { view("mine.html", context! {}) })
    }
}

#[renox::test]
async fn an_apps_own_file_replaces_the_macros() {
    let app = TestApp::new(
        App::new()
            .templates(|env| {
                env.add_template(
                    "renox-blocks/blocks.html",
                    r#"{% macro quantity(name) %}<b>mine: {{ name }}</b>{% endmacro %}"#,
                )
                .unwrap();
                env.add_template(
                    "mine.html",
                    r#"{% from "renox-blocks/blocks.html" import quantity %}{{ quantity("qty") }}"#,
                )
                .unwrap();
            })
            .module(Blocks::new())
            .module(Mine),
    )
    .await;
    app.get("/mine")
        .await
        .assert_ok()
        .assert_see("<b>mine: qty</b>");
}

// Every text a block shows has an English version: the keys written out in
// the macros, and those they build (`blocks.plans.` ~ interval, …).
#[test]
fn every_text_the_macros_use_is_in_english() {
    let source = include_str!("../views/blocks.html");
    let keys: Vec<&str> = TEXTS.iter().map(|(k, _)| *k).collect();
    let mut used = 0;
    for part in source.split("renox_blocks_t('").skip(1) {
        let key = &part[..part.find('\'').unwrap()];
        if part[key.len()..].starts_with("' ~") {
            assert!(
                keys.iter().any(|k| k.starts_with(key)),
                "no text starts with {key}"
            );
        } else {
            assert!(keys.contains(&key), "no English text for {key}");
        }
        used += 1;
    }
    assert!(used > 60, "{used}");
    for key in [
        "blocks.plans.month",
        "blocks.plans.year",
        "blocks.history.kind_error",
    ] {
        assert!(keys.contains(&key), "{key}");
    }
    for n in 1..=12 {
        assert!(keys.contains(&format!("blocks.calendar.month_{n}").as_str()));
    }
    for n in 0..7 {
        assert!(keys.contains(&format!("blocks.calendar.day_{n}").as_str()));
        assert!(keys.contains(&format!("blocks.calendar.day_long_{n}").as_str()));
    }
}

fn between(html: &str, start: &str, end: &str) -> String {
    let from = html.find(start).unwrap_or_else(|| panic!("no {start}")) + start.len();
    let to = html[from..].find(end).unwrap() + from;
    html[from..to].to_owned()
}

const TAGS: &str = r##"<html><head><rx-head/></head><body>
<form method="post" action="/order">
<rx-quantity name="qty" :value="2" :min="1" :max="5" label="Bikes"/>
<rx-range-slider name-min="low" name-max="high" :min="0" :max="1000" :step="50" :value-min="100" :value-max="600" label="Price" suffix=" km"/>
<rx-keypad target="rx-paid" decimal enter-label="Pay"/>
<rx-swatches name="size" label="Frame size" :options="[{'value': 'S', 'label': 'S'}, {'value': 'M', 'label': 'M'}]" selected="M" required/>
<rx-datetime-range name-start="starts_at" name-end="ends_at" label="Rental" min="2026-10-01" :step="30" :opens="9" :closes="10"/>
</form>
<rx-gallery :photos="[{'src': '/a.svg', 'alt': 'Side'}]" id="photos"/>
<rx-history :items="[{'time': '2026-10-06', 'title': 'Done'}]" label="Service" date-format="%d %b"/>
<rx-compare-plans :plans="[{'key': 'basic', 'name': 'Basic', 'price': 2900}]" highlight="basic"/>
<rx-month-calendar :month="month" today="2026-02-03"/>
<rx-availability :columns="['09:00']" :rows="[{'label': 'Trail 5', 'slots': [{'state': 'free'}]}]" corner="Bike"/>
<rx-kanban id="jobs" :columns="[{'key': 'waiting', 'title': 'Waiting', 'cards': []}]" url="/jobs/move"/>
</body></html>"##;

#[renox::test]
async fn the_blocks_are_tags_too() {
    let app = TestApp::new(
        App::new()
            .module(Blocks::new())
            .module(Shop)
            .templates(|env| renox::view::add_template(env, "page.html", TAGS).unwrap()),
    )
    .await;
    let html = app.get("/order").await.assert_ok().text();
    for needle in [
        r#"<input class="rx-quantity__input" id="rx-qty" name="qty""#,
        "rx-range",
        "data-rx-keypad",
        "rx-swatches",
        "rx-datetime",
        "rx-gallery",
        r#"class="rx-history""#,
        "rx-plans",
        "rx-month",
        "rx-availability",
        "rx-kanban",
    ] {
        assert!(html.contains(needle), "missing {needle}\n{html}");
    }
}

const RECEIPT: &str = r##"<html><head><rx-head/></head><body>
<rx-receipt id="till" width="58mm" title="Bike Shop" :header="['Calle Mayor 12', '+34 600 000 000']" number="R-1042" date="2026-10-16 10:32"
  :lines="[{'name': 'Helmet <Trail>', 'quantity': 2, 'price': 4900, 'note': 'Black'}, {'name': 'Bell', 'quantity': 1, 'price': 800}, {'name': 'Repair, 1 h', 'total': 3500}]"
  :totals="[{'label': 'Subtotal', 'amount': 14100}, {'label': 'Total', 'amount': 14100, 'strong': true}]"
  :footer="['Thank you!', 'Returns within 14 days']">
  <span data-code>R-1042</span>
</rx-receipt>
<rx-receipt id="plain" :lines="[]" :print-button="false"/>
</body></html>"##;

/// The receipt block (#48), as a tag: its lines, amounts and code slot, the
/// paper's width class, escaping, and the print button with its script.
#[renox::test]
async fn the_receipt_prints_its_lines_totals_and_code_slot() {
    let app = TestApp::new(
        App::new()
            .module(Blocks::new())
            .module(Shop)
            .templates(|env| renox::view::add_template(env, "page.html", RECEIPT).unwrap()),
    )
    .await;
    let html = app.get("/order").await.assert_ok().text();
    let till = between(
        &html,
        r#"<div class="rx-receipt rx-receipt--w58" id="till""#,
        "</article>",
    );
    let has = |needle: &str| assert!(till.contains(needle), "missing {needle}\n{till}");
    has("data-rx-receipt>");
    has(r#"aria-label="Bike Shop""#);
    has(r#"<p class="rx-receipt__title">Bike Shop</p>"#);
    has(r#"<p class="rx-receipt__text">Calle Mayor 12</p>"#);
    has("<span>R-1042</span><span>2026-10-16 10:32</span>");
    // A quantity of 2 shows "2 × unit price" under the name, the total in its column.
    has("Helmet &lt;Trail&gt;");
    has(r#"<span class="rx-receipt__note">2 × $49.00 · Black</span>"#);
    has("<td>$98.00</td>");
    // One bell has no "1 ×" line; a line with only a total keeps it.
    has("<td>$8.00</td>");
    has("<td>$35.00</td>");
    assert!(!till.contains("1 × $8.00"), "{till}");
    // Totals in the table's footer, the last one strong.
    has(
        r#"<tr class="rx-receipt__sum rx-receipt__sum--strong"><th scope="row">Total</th><td>$141.00</td></tr>"#,
    );
    // The slot comes after the table, then the footer.
    assert!(
        till.find("</table>").unwrap() < till.find("<span data-code>R-1042</span>").unwrap(),
        "{till}"
    );
    has(r#"<div class="rx-receipt__code">"#);
    has(r#"<p class="rx-receipt__text">Returns within 14 days</p>"#);
    // The print button needs the script, which the page loads for it.
    assert!(
        html.contains(r#" data-receipt="/_renox/blocks/receipt-"#),
        "{html}"
    );
    // The other receipt: 80 mm by default, no button, no slot.
    assert!(html.contains(r#"class="rx-receipt rx-receipt--w80" id="plain""#));
    assert_eq!(html.matches("data-rx-receipt-print").count(), 1, "{html}");
    assert_eq!(html.matches("rx-receipt__code").count(), 1, "{html}");
}
