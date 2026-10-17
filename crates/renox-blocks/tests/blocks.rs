//! The blocks (#347): what each renders, the files a page loads, the form
//! blocks' round trip through `Valid<T>` (with errors and old input), the
//! month calendar's arithmetic, the English texts and an app's
//! translations of them, and an app's own copy of the macros. Their
//! behaviour in a browser is tests/browser/blocks.test.mjs.

use renox::prelude::*;
use renox::testing::TestApp;
use renox_blocks::{Blocks, TEXTS};
use serde::{Deserialize, Serialize};

const PAGE: &str = r##"{% from "renox-blocks/blocks.html" import quantity, range_slider, keypad, swatches, datetime_range, gallery, history, compare_plans, month_calendar, availability, kanban, status_stepper, week_calendar, receipt %}
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
        "week",
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
    assert!(used > 80, "{used}");
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
<rx-status-stepper :steps="[{'key': 'draft', 'label': 'Draft'}, {'key': 'sent', 'label': 'Sent'}]" current="sent" orientation="vertical"/>
<rx-week-calendar :resources="[{'id': 'bay-1', 'label': 'Bay 1'}]" start="2026-10-12" :days="2" :from="9" :to="11" :step="60" slot-url="/book"/>
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
        r#"class="rx-stepper rx-stepper--vertical""#,
        "data-rx-week",
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

// ---------------------------------------------------------------------
// status_stepper and week_calendar, with typed data.

#[derive(Serialize)]
struct Step {
    key: &'static str,
    label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tone: Option<&'static str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    terminal: bool,
}

#[derive(Serialize)]
struct Bay {
    id: &'static str,
    label: &'static str,
}

#[derive(Serialize)]
struct Booking {
    resource: &'static str,
    start: &'static str,
    end: &'static str,
    label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tone: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    href: Option<&'static str>,
}

fn step(key: &'static str, label: &'static str) -> Step {
    Step {
        key,
        label,
        tone: None,
        terminal: false,
    }
}

fn booking(
    resource: &'static str,
    start: &'static str,
    end: &'static str,
    label: &'static str,
) -> Booking {
    Booking {
        resource,
        start,
        end,
        label,
        tone: None,
        href: None,
    }
}

const DISPLAY: &str = r##"{% from "renox-blocks/blocks.html" import status_stepper, week_calendar %}
<html><head>{{ renox_head() }}</head><body>
{{ status_stepper(steps, current=current, orientation=orientation) }}
{{ week_calendar(bays, bookings, start="2026-10-12", days=2, from=9, to=12, step=60, slot_url="/book", slot_target="#week", label="Bays") }}
{{ week_calendar(bays, bookings, start="2026-10-12", days=1, from=9, to=12, step=60, id="plain") }}
</body></html>"##;

#[derive(Deserialize)]
struct DisplayQuery {
    current: Option<String>,
    orientation: Option<String>,
    /// Bookings made in Jakarta's wall clock (UTC+7), for the zone test.
    jakarta: Option<bool>,
}

#[derive(Deserialize)]
struct Slot {
    resource: String,
    start: String,
    end: String,
}

struct Display;

impl Module for Display {
    fn name(&self) -> &'static str {
        "display"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/display", |Query(q): Query<DisplayQuery>| async move {
                let steps = vec![
                    step("draft", "Draft"),
                    Step {
                        tone: Some("success"),
                        ..step("approved", "Approved")
                    },
                    step("shipped", "Shipped"),
                    Step {
                        tone: Some("error"),
                        terminal: true,
                        ..step("cancelled", "Cancelled")
                    },
                ];
                let bays = vec![
                    Bay {
                        id: "bay-1",
                        label: "Bay 1",
                    },
                    Bay {
                        id: "bay-2",
                        label: "Bay 2",
                    },
                ];
                // Bay 1: 09:30-11:00 and 10:00-12:00 overlap. Bay 2: one
                // booking from Monday 11:00 to Tuesday 10:00. Times are UTC,
                // or the same wall-clock times as Jakarta's (+07:00) offset.
                let hour = |h: u8| -> &'static str {
                    match (q.jakarta == Some(true), h) {
                        (false, 9) => "2026-10-12T09:30:00Z",
                        (false, 10) => "2026-10-12T10:00:00Z",
                        (false, 11) => "2026-10-12T11:00:00Z",
                        (false, 12) => "2026-10-12T12:00:00Z",
                        (false, _) => "2026-10-13T10:00:00Z",
                        (true, 9) => "2026-10-12T02:30:00Z",
                        (true, 10) => "2026-10-12T03:00:00Z",
                        (true, 11) => "2026-10-12T04:00:00Z",
                        (true, 12) => "2026-10-12T05:00:00Z",
                        (true, _) => "2026-10-13T03:00:00Z",
                    }
                };
                let bookings = vec![
                    Booking {
                        tone: Some("info"),
                        href: Some("/jobs/4"),
                        ..booking("bay-1", hour(9), hour(11), "Tune-up")
                    },
                    booking("bay-1", hour(10), hour(12), "Brakes"),
                    Booking {
                        tone: Some("warning"),
                        ..booking("bay-2", hour(11), hour(13), "Overnight")
                    },
                    booking("nowhere", hour(10), hour(11), "Lost"),
                ];
                view(
                    "display.html",
                    context! {
                        steps,
                        current => q.current,
                        orientation => q.orientation,
                        bays,
                        bookings,
                    },
                )
            })
            .post("/book", |Form(slot): Form<Slot>| async move {
                Json(json!({"resource": slot.resource, "start": slot.start, "end": slot.end}))
            })
    }
}

fn display_app() -> App {
    App::new()
        .module(Blocks::new())
        .module(Display)
        .templates(|env| env.add_template("display.html", DISPLAY).unwrap())
}

/// The `<li>` of the step labelled `label`.
fn step_of(list: &str, label: &str) -> String {
    let at = list.find(&format!(">{label}</span>")).unwrap();
    let start = list[..at].rfind("<li ").unwrap();
    let end = list[at..].find("</li>").unwrap() + at;
    list[start..end].to_owned()
}

#[renox::test]
async fn the_status_stepper_says_which_step_is_current() {
    let app = TestApp::new(display_app()).await;
    let html = app
        .get("/display?current=approved&orientation=vertical")
        .await
        .assert_ok()
        .text();
    let list = between(&html, r#"<ol class="rx-stepper"#, "</ol>");
    assert!(list.contains("rx-stepper--vertical"), "{list}");
    assert_eq!(list.matches(r#"aria-current="step""#).count(), 1, "{list}");
    assert_eq!(list.matches("<li ").count(), 4, "{list}");
    // Draft is done, Approved is the current step (tinted by its tone), the
    // others are ahead; the ending is said to be final.
    let draft = step_of(&list, "Draft");
    assert!(draft.contains("rx-stepper__step--done"), "{draft}");
    assert!(draft.contains("(done)") && !draft.contains("aria-current"));
    let approved = step_of(&list, "Approved");
    assert!(
        approved.contains("rx-stepper__step--current rx-stepper__step--success"),
        "{approved}"
    );
    assert!(approved.contains(r#"aria-current="step""#) && approved.contains("(current step)"));
    assert!(step_of(&list, "Shipped").contains("(not reached yet)"));
    let cancelled = step_of(&list, "Cancelled");
    assert!(
        cancelled.contains("rx-stepper__step--terminal"),
        "{cancelled}"
    );
    assert!(
        cancelled.contains("(not reached yet, final step)"),
        "{cancelled}"
    );
    assert!(
        !cancelled.contains("rx-stepper__step--error"),
        "a tone shows once the step is reached: {cancelled}"
    );

    // Reaching the ending: the steps before it are done, it is current.
    let html = app.get("/display?current=cancelled").await.text();
    let list = between(&html, r#"<ol class="rx-stepper"#, "</ol>");
    assert!(list.contains("rx-stepper--horizontal"), "{list}");
    assert_eq!(list.matches("rx-stepper__step--done").count(), 3, "{list}");
    assert!(
        list.contains(
            "rx-stepper__step--current rx-stepper__step--error rx-stepper__step--terminal"
        ),
        "{list}"
    );
    assert!(list.contains("(current step, final step)"));

    // A current that is no step leaves every step ahead.
    let html = app.get("/display?current=lost").await.text();
    assert!(!html.contains("aria-current"), "{html}");
}

#[renox::test]
async fn the_week_calendar_places_events_in_their_cells() {
    let app = TestApp::new(display_app()).await;
    let html = app.get("/display").await.assert_ok().text();
    let week = between(&html, r#"<div class="rx-week" id="rx-week""#, "\n</div>\n");
    // 2 days x 3 hourly slots = 6 columns; a header row and a row a bay.
    assert!(
        week.contains("--rx-week-columns: 6; --rx-week-per-day: 3"),
        "{week}"
    );
    assert!(week.contains(r#"role="grid" aria-label="Bays""#));
    assert!(week.contains(r#"aria-label="Monday 12 October""#));
    assert!(week.contains(r#"aria-label="Tuesday 13 October""#));
    assert_eq!(week.matches(r#"role="row""#).count(), 3, "{week}");
    let row = |label: &str| {
        let at = week.find(&format!(">{label}</span>")).unwrap();
        let end = week[at..]
            .find("<div class=\"rx-week__row\"")
            .map_or(week.len(), |n| n + at);
        week[at..end].to_owned()
    };

    // Bay 1: 09:30-11:00 takes the slots of 09 and 10 (columns 1-2) in the
    // first lane, 10:00-12:00 those of 10 and 11 (columns 2-3) in the second.
    let bay1 = row("Bay 1");
    assert!(bay1.contains("--rx-week-lanes: 2"), "{bay1}");
    assert!(bay1.contains("rx-week__event--info"), "{bay1}");
    assert!(
        bay1.contains("grid-column: 1 / span 2; grid-row: 1"),
        "{bay1}"
    );
    assert!(
        bay1.contains("grid-column: 2 / span 2; grid-row: 2"),
        "{bay1}"
    );
    assert!(
        bay1.contains(r#"aria-label="Tune-up, Bay 1, Monday 12 October, 09:30 to 11:00""#),
        "{bay1}"
    );
    assert!(
        bay1.contains(r#"<a class="rx-week__event-link" href="/jobs/4" tabindex="-1">Tune-up</a>"#)
    );
    // Under the events there are no free slots; Tuesday's three each span both lanes.
    assert!(!bay1.contains(r#"data-start="2026-10-12T"#), "{bay1}");
    assert_eq!(bay1.matches("rx-week__slot").count(), 3, "{bay1}");
    assert!(
        bay1.contains(r#"style="grid-column: 4; grid-row: 1 / span 2""#),
        "{bay1}"
    );
    assert!(
        bay1.contains(r#"aria-label="Book Bay 1, Tuesday 13 October, 09:00 to 10:00""#),
        "{bay1}"
    );

    // A free slot carries what a click sends: data-* and a static hx-vals.
    assert!(bay1.contains(
        r#"data-resource="bay-1" data-start="2026-10-13T09:00:00Z" data-end="2026-10-13T10:00:00Z""#
    ), "{bay1}");
    assert!(bay1.contains(r#"hx-post="/book""#));
    assert!(bay1.contains(
        r#"hx-vals='{"end":"2026-10-13T10:00:00Z","resource":"bay-1","start":"2026-10-13T09:00:00Z"}'"#
    ), "{bay1}");
    assert!(bay1.contains(r##"hx-target="#week" hx-swap="outerHTML""##));

    // Bay 2: the booking from Monday 11:00 to Tuesday 10:00 is cut to each
    // day's hours: Monday 11:00-12:00 (column 3, continues later) and
    // Tuesday 09:00-10:00 (column 4, started earlier).
    let bay2 = row("Bay 2");
    assert!(bay2.contains("--rx-week-lanes: 1"), "{bay2}");
    assert!(
        bay2.contains("rx-week__event--warning rx-week__event--after"),
        "{bay2}"
    );
    assert!(
        bay2.contains("rx-week__event--warning rx-week__event--before"),
        "{bay2}"
    );
    assert!(bay2.contains("grid-column: 3 / span 1"), "{bay2}");
    assert!(bay2.contains("grid-column: 4 / span 1"), "{bay2}");
    assert!(bay2.contains("(continues later)") && bay2.contains("(started earlier)"));
    assert!(
        !html.contains("Lost"),
        "an event of no listed resource is dropped"
    );

    // One Tab stop: the first cell only. The help says how to use the grid.
    assert_eq!(
        week.matches(r#"role="gridcell" tabindex="0""#).count(),
        1,
        "{week}"
    );
    assert!(week.contains("Enter or Space books a free slot"));

    // Without slot_url the free slots are not cells; the bookings still are.
    let plain = between(&html, r#"<div class="rx-week" id="plain""#, "\n</div>\n");
    assert!(!plain.contains("rx-week__slot"), "{plain}");
    assert!(!plain.contains("hx-post"));
    assert!(plain.contains("rx-week__event"));
    assert!(!plain.contains("Enter or Space books"));
}

#[renox::test]
async fn the_week_calendar_reads_times_in_the_apps_zone() {
    // In UTC+7, 02:30Z is 09:30: the same cells as above, and the slots go
    // out with the zone's offset.
    let app = TestApp::with_config(display_app(), |c| c.timezone = "+07:00".parse().unwrap()).await;
    let html = app.get("/display?jakarta=true").await.assert_ok().text();
    let week = between(&html, r#"<div class="rx-week" id="rx-week""#, "\n</div>\n");
    assert!(
        week.contains("grid-column: 1 / span 2; grid-row: 1"),
        "{week}"
    );
    assert!(week.contains("09:30 to 11:00"), "{week}");
    assert!(
        week.contains(
            r#"data-start="2026-10-13T09:00:00+07:00" data-end="2026-10-13T10:00:00+07:00""#
        ),
        "{week}"
    );
    // What a click sends is what the handler reads back.
    let posted = app
        .post(
            "/book",
            &[
                ("resource", "bay-1"),
                ("start", "2026-10-13T09:00:00+07:00"),
                ("end", "2026-10-13T10:00:00+07:00"),
            ],
        )
        .await
        .assert_ok()
        .text();
    assert!(posted.contains("2026-10-13T09:00:00+07:00"), "{posted}");
}

#[renox::test]
async fn a_week_calendar_with_bad_data_says_what_is_wrong() {
    let app = TestApp::new(
        App::new()
            .module(Blocks::new())
            .templates(|env| {
                env.add_template(
                    "bad.html",
                    r#"{% from "renox-blocks/blocks.html" import week_calendar %}{{ week_calendar([{"id": "a", "label": "A"}], [{"resource": "a", "start": "soon", "end": "later", "label": "x"}], start="2026-10-12") }}"#,
                )
                .unwrap()
            })
            .module(Bad),
    )
    .await;
    app.get("/bad")
        .await
        .assert_status(500)
        .assert_see("`soon` is not a time");
}

struct Bad;

impl Module for Bad {
    fn name(&self) -> &'static str {
        "bad"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/bad", || async { view("bad.html", context! {}) })
    }
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRsmall-but-valid-enough-for-sniffing";

const SIGN_MACRO: &str = r##"{% from "renox-blocks/blocks.html" import signature_pad %}
<html><head>{{ renox_head() }}</head><body>
<form method="post" action="/sign" enctype="multipart/form-data">
{{ csrf_field() }}
{{ signature_pad("signature", required=true, width=320, height=120, hint="Sign for the repair.") }}
</form></body></html>"##;

const SIGN_TAG: &str = r##"<html><head><rx-head/></head><body>
<form method="post" action="/sign" enctype="multipart/form-data">
<rx-signature-pad name="sig" label="Customer" name-label="Print your name"/>
</form></body></html>"##;

#[derive(Debug, Deserialize)]
struct SignOff {
    signature: Option<Upload>,
    signature_name: Option<String>,
}

impl Validate for SignOff {
    fn rules(&self, v: &mut Validator) {
        v.field("signature", &self.signature)
            .image()
            .mimes(&["png"])
            .max(100);
        if self.signature.is_none() {
            v.field("signature_name", &self.signature_name).required();
        }
    }
}

struct Signing;

impl Module for Signing {
    fn name(&self) -> &'static str {
        "signing"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/sign", || async { view("sign.html", ()) })
            .get("/sign-tag", || async { view("sign-tag.html", ()) })
            .post("/sign", |Valid(form): Valid<SignOff>| async move {
                Json(json!({
                    "file": form.signature.as_ref().map(|f| f.file_name().to_owned()),
                    "size": form.signature.as_ref().map(|f| f.bytes().len()),
                    "name": form.signature_name,
                }))
            })
    }
}

async fn signing_app() -> TestApp {
    TestApp::new(
        App::new()
            .module(Blocks::new())
            .module(Signing)
            .templates(|env| {
                env.add_template("sign.html", SIGN_MACRO).unwrap();
                renox::view::add_template(env, "sign-tag.html", SIGN_TAG).unwrap();
            }),
    )
    .await
}

/// The signature pad (#53): the macro and the tag print the canvas, its
/// tools, a typed-name field named `{name}_name` linked to the help text, a
/// hidden PNG file input and the part's file, only on that page.
#[renox::test]
async fn the_signature_pad_renders_as_a_macro_and_a_tag() {
    let app = signing_app().await;
    let html = app.get("/sign").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"data-rx-signature data-rx-signature-width="320" data-rx-signature-height="120""#);
    has(r#"aria-describedby="rx-signature-help rx-signature-hint rx-signature-error""#);
    has(
        r#"<canvas class="rx-signature__canvas" tabindex="0" role="img" aria-label="Signature area""#,
    );
    has("data-rx-signature-undo>Undo</button>");
    has("data-rx-signature-clear>Clear</button>");
    has(r#"<input type="file" hidden name="signature" accept="image/png" data-rx-signature-file>"#);
    has(
        r#"id="rx-signature-name" name="signature_name" value="" autocomplete="off" required data-rx-signature-required"#,
    );
    has(r#"<p class="rx-hint" id="rx-signature-help">Draw your signature"#);
    has("Sign for the repair.");
    has(r#"data-signature="/_renox/blocks/signature-"#);
    // No handlers or style attributes: the size goes through the script (CSP=strict).
    assert!(!html.contains("onclick") && !html.contains(r#" style="--rx-signature"#));

    let html = app.get("/sign-tag").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"id="rx-sig-name" name="sig_name""#);
    has(r#"name="sig" accept="image/png""#);
    has(">Print your name</label>");
    has("Customer");
    has(r#"<span class="rx-required">(optional)</span>"#);
    assert!(!html.contains("data-rx-signature-required"), "{html}");
}

/// A drawn signature arrives as an `Upload` that passes `image()` and
/// `mimes(&["png"])`; a typed name arrives alone; neither, or a file that
/// isn't an image, fails with the rule's message.
#[renox::test]
async fn a_drawn_or_typed_signature_arrives_through_valid() {
    let app = signing_app().await;
    app.get("/sign").await;

    app.post_multipart("/sign", &[], &[("signature", "signature.png", PNG)])
        .await
        .assert_ok()
        .assert_json_path("file", "signature.png")
        .assert_json_path("size", PNG.len());

    // The typed way: the name, and no file.
    app.post_multipart("/sign", &[("signature_name", "Marta Ruiz")], &[])
        .await
        .assert_ok()
        .assert_json_path("name", "Marta Ruiz");

    app.request()
        .json()
        .post_multipart(
            "/sign",
            &[],
            &[("signature", "signature.png", b"MZ not an image")],
        )
        .await
        .assert_status(422)
        .assert_invalid("signature")
        .assert_see("must be an image");
    app.request()
        .json()
        .post_multipart("/sign", &[], &[])
        .await
        .assert_status(422)
        .assert_invalid("signature_name");
}

const JPEG: &[u8] = b"\xff\xd8\xff\xe0\0\x10JFIF\0small-but-valid-enough-for-sniffing";

const CAMERA_MACRO: &str = r##"{% from "renox-blocks/blocks.html" import camera_capture %}
<html><head>{{ renox_head() }}</head><body>
<form method="post" action="/photos" enctype="multipart/form-data">
{{ csrf_field() }}
{{ camera_capture("photo", facing="environment", max=3, max_width=1024, required=true, hint="Show the repair.") }}
</form></body></html>"##;

const CAMERA_TAG: &str = r##"<html><head><rx-head/></head><body>
<form method="post" action="/photos" enctype="multipart/form-data">
<rx-camera-capture name="pic" label="Bike" facing="user" :max="1"/>
</form></body></html>"##;

#[derive(Debug, Deserialize)]
struct Photos {
    #[serde(default)]
    photo: Vec<Upload>,
}

impl Validate for Photos {
    fn rules(&self, v: &mut Validator) {
        v.field("photo", &self.photo).required().max(3);
        v.each("photo", &self.photo, |photo| {
            photo.image().mimes(&["jpg", "jpeg"]).max(100)
        });
    }
}

struct Photographing;

impl Module for Photographing {
    fn name(&self) -> &'static str {
        "photographing"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/photos", || async { view("photos.html", ()) })
            .get("/photos-tag", || async { view("photos-tag.html", ()) })
            .post("/photos", |Valid(form): Valid<Photos>| async move {
                Json(json!({
                    "files": form.photo.iter().map(|f| f.file_name().to_owned()).collect::<Vec<_>>(),
                }))
            })
    }
}

async fn camera_app() -> TestApp {
    TestApp::new(
        App::new()
            .module(Blocks::new())
            .module(Photographing)
            .templates(|env| {
                env.add_template("photos.html", CAMERA_MACRO).unwrap();
                renox::view::add_template(env, "photos-tag.html", CAMERA_TAG).unwrap();
            }),
    )
    .await
}

/// The camera capture (#53): the macro and the tag print a live area (hidden
/// until the camera starts), the list of photos, a file input that is the
/// fallback (visible without the script, `capture` set), and the part's file,
/// only on that page.
#[renox::test]
async fn the_camera_capture_renders_as_a_macro_and_a_tag() {
    let app = camera_app().await;
    let html = app.get("/photos").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(
        r#"data-rx-camera data-rx-camera-max="3" data-rx-camera-width="1024" data-rx-camera-quality="0.85" data-rx-camera-facing="environment""#,
    );
    has(r#"<div class="rx-camera__live" data-rx-camera-live hidden>"#);
    has("data-rx-camera-shutter>Take photo</button>");
    has(
        r#"name="photo" accept="image/*" capture="environment" multiple required data-rx-camera-required"#,
    );
    has(r#"id="rx-photo-help" role="status" data-rx-camera-status>Take a photo with the camera"#);
    has("Show the repair.");
    has("Camera access was denied.");
    has(r#"data-camera="/_renox/blocks/camera-"#);
    assert!(
        !html.contains("onclick") && !html.contains(" style="),
        "{html}"
    );

    let html = app.get("/photos-tag").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"name="pic" accept="image/*" capture="user""#);
    has("Bike");
    has(r#"<span class="rx-required">(optional)</span>"#);
    assert!(
        !html.contains(" multiple") && !html.contains("data-rx-camera-required"),
        "{html}"
    );
}

/// One to three JPEGs arrive as `Vec<Upload>` and pass `image()` and
/// `mimes`; more than the limit, a non-image or none at all fail.
#[renox::test]
async fn camera_photos_arrive_through_valid() {
    let app = camera_app().await;
    app.get("/photos").await;

    app.post_multipart("/photos", &[], &[("photo", "photo-1.jpg", JPEG)])
        .await
        .assert_ok()
        .assert_json_path("files.0", "photo-1.jpg");

    let three = [
        ("photo", "photo-1.jpg", JPEG),
        ("photo", "photo-2.jpg", JPEG),
        ("photo", "photo-3.jpg", JPEG),
    ];
    app.post_multipart("/photos", &[], &three)
        .await
        .assert_ok()
        .assert_json_path("files.2", "photo-3.jpg");

    let four = [
        ("photo", "photo-1.jpg", JPEG),
        ("photo", "photo-2.jpg", JPEG),
        ("photo", "photo-3.jpg", JPEG),
        ("photo", "photo-4.jpg", JPEG),
    ];
    app.request()
        .json()
        .post_multipart("/photos", &[], &four)
        .await
        .assert_status(422)
        .assert_invalid("photo");

    app.request()
        .json()
        .post_multipart(
            "/photos",
            &[],
            &[("photo", "photo-1.jpg", b"MZ not an image")],
        )
        .await
        .assert_status(422)
        .assert_invalid("photo.0");
    app.request()
        .json()
        .post_multipart("/photos", &[], &[])
        .await
        .assert_status(422)
        .assert_invalid("photo");
}

const MAP_MACRO: &str = r##"{% from "renox-blocks/blocks.html" import map_picker, map %}
<html><head>{{ renox_head() }}</head><body>
<form method="post" action="/place">
{{ csrf_field() }}
{{ map_picker("pickup", value={"lat": 40.4168, "lng": -3.7038}, hint="Where we meet.") }}
{{ map_picker("zone", mode="polygon", value={"type": "Polygon", "coordinates": [[[-3.7, 40.4], [-3.6, 40.4], [-3.6, 40.5], [-3.7, 40.4]]]}) }}
{{ map_picker("area", mode="circle", value={"center": {"lat": 40.4, "lng": -3.7}, "radius": 500}) }}
</form>
{{ map(markers=[{"lat": 40.4168, "lng": -3.7038, "label": "Workshop <Main>"}], shapes=[{"center": {"lat": 40.4, "lng": -3.7}, "radius": 500}]) }}
</body></html>"##;

const MAP_TAG: &str = r##"<html><head><rx-head/></head><body>
<form method="post" action="/place">
<rx-map-picker name="spot" label="Spot"/>
<rx-map :markers="[{'lat': 1.5, 'lng': 2.5}]" id="places"/>
</form></body></html>"##;

struct Maps;

impl Module for Maps {
    fn name(&self) -> &'static str {
        "maps"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/map", || async { view("map.html", ()) })
            .get("/map-tag", || async { view("map-tag.html", ()) })
    }
}

async fn maps_app(blocks: Blocks) -> TestApp {
    TestApp::new(App::new().module(blocks).module(Maps).templates(|env| {
        env.add_template("map.html", MAP_MACRO).unwrap();
        renox::view::add_template(env, "map-tag.html", MAP_TAG).unwrap();
    }))
    .await
}

/// The map blocks (#47) without tiles: an explanatory note naming the
/// variable, the picker's typed fields and the markers as text.
#[renox::test]
async fn the_map_blocks_explain_themselves_without_tiles() {
    let app = maps_app(Blocks::new()).await;
    let html = app.get("/map").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(
        r#"<p class="rx-map__empty" role="note">The map is not shown: no map tiles are configured. Set BLOCKS_MAP_TILES"#,
    );
    assert!(!html.contains("data-rx-map-canvas"), "{html}");
    assert!(!html.contains("data-rx-map-tiles"), "{html}");
    // The point picker is two plain number fields in `name[lat]`, `name[lng]`.
    has(r#"id="rx-pickup-lat" name="pickup[lat]" value="40.4168" min="-90" max="90" step="any""#);
    has(r#"id="rx-pickup-lng" name="pickup[lng]" value="-3.7038" min="-180" max="180" step="any""#);
    has("Where we meet.");
    has(r#"data-rx-map-picker data-rx-map-mode="point""#);
    // A polygon: typed `lat, lng` lines (closing vertex dropped) and a GeoJSON value in [lng, lat].
    has("40.4, -3.7\n40.4, -3.6\n40.5, -3.6");
    has(r#"name="zone" data-rx-map-value value='{"coordinates":[[[-3.7,40.4],"#);
    // A circle: one typed line and a GeoJSON Point with a radius.
    has("40.4, -3.7, 500");
    has(
        r#"name="area" data-rx-map-value value='{"coordinates":[-3.7,40.4],"radius":500,"type":"Point"}'"#,
    );
    // The read-only map lists its markers as text, escaped.
    has("<li>Workshop &lt;Main&gt; (40.4168, -3.7038)</li>");
    has(r#"data-rx-map-markers='[{"label":"Workshop "#);
    assert!(!html.contains("Workshop <Main>"), "{html}");
}

/// With tiles (from the module's setting), the canvas and the tile data
/// attributes appear, an unusable URL counts as unset, and the markers
/// stay as a list for screen readers.
#[renox::test]
async fn the_map_blocks_carry_the_tiles_and_attribution() {
    let tiles = "https://tiles.example.com/{z}/{x}/{y}.png";
    let app = maps_app(
        Blocks::new()
            .map_tiles(tiles)
            .map_attribution("Map data \"Example\""),
    )
    .await;
    let html = app.get("/map").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(&format!(
        r#"data-rx-map-tiles="{tiles}" data-rx-map-attribution="Map data &quot;Example&quot;""#
    ));
    has(
        r#"role="application" aria-label="Map" aria-describedby="rx-pickup-help" data-rx-map-canvas"#,
    );
    has(r#"<ul class="rx-map__list rx-visually-hidden""#);
    assert!(!html.contains("no map tiles are configured"), "{html}");
    // No inline handlers or style attributes (CSP=strict): the height is data.
    assert!(
        !html.contains(" onclick=") && !html.contains(" style="),
        "{html}"
    );

    let app = maps_app(Blocks::new().map_tiles("ftp://tiles.example.com/{z}/{x}/{y}.png")).await;
    let html = app.get("/map").await.assert_ok().text();
    assert!(html.contains("no map tiles are configured"), "{html}");
}

/// The tags `<rx-map-picker>` and `<rx-map>` call the macros.
#[renox::test]
async fn the_map_blocks_are_tags_too() {
    let app = maps_app(Blocks::new()).await;
    let html = app.get("/map-tag").await.assert_ok().text();
    for needle in [
        r#"id="rx-spot-lat" name="spot[lat]""#,
        r#"<span class="rx-label" id="rx-spot-label">Spot</span>"#,
        r#"id="places" data-rx-map"#,
        "<li>Place (1.5, 2.5)</li>",
    ] {
        assert!(html.contains(needle), "missing {needle}\n{html}");
    }
}

const RATING_MACRO: &str = r##"{% from "renox-blocks/blocks.html" import rating %}
<html><head>{{ renox_head() }}</head><body>
<form method="post" action="/review">
{{ csrf_field() }}
{{ rating("score", max=5, label="Your rating", required=true, hint="Stars, one to five.") }}
</form>
{{ rating("seen", max=4, value=3, label="Average", readonly=true) }}
{{ rating("none_yet", readonly=true) }}
</body></html>"##;

const RATING_TAG: &str = r##"<html><head><rx-head/></head><body>
<form method="post" action="/review">
<rx-rating name="stars" :max="3" :value="2"/>
</form></body></html>"##;

#[derive(Debug, Deserialize, Validate)]
struct Review {
    #[validate(required, between(1, 5))]
    score: Option<u8>,
    #[validate(required)]
    title: Option<String>,
}

struct Reviews;

impl Module for Reviews {
    fn name(&self) -> &'static str {
        "reviews"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/review", || async { view("review.html", ()) })
            .get("/review-tag", || async { view("review-tag.html", ()) })
            .post("/review", |Valid(form): Valid<Review>| async move {
                Json(json!({ "score": form.score }))
            })
    }
}

async fn reviews_app() -> TestApp {
    TestApp::new(
        App::new()
            .module(Blocks::new())
            .module(Reviews)
            .templates(|env| {
                env.add_template("review.html", RATING_MACRO).unwrap();
                renox::view::add_template(env, "review-tag.html", RATING_TAG).unwrap();
            }),
    )
    .await
}

/// The rating (#54): a fieldset of `max` radios with one name, each named
/// "N of MAX" for a screen reader, with no script of its own; the readonly
/// form is text and sends nothing; the tag calls the macro.
#[renox::test]
async fn the_rating_renders_as_a_radio_group() {
    let app = reviews_app().await;
    let html = app.get("/review").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(
        r#"<fieldset class="rx-field rx-rating" id="rx-score" data-rx-rating aria-describedby="rx-score-hint rx-score-error">"#,
    );
    has(r#"<legend class="rx-label">Your rating</legend>"#);
    for n in 1..=5 {
        has(&format!(
            r#"type="radio" id="rx-score-{n}" name="score" value="{n}""#
        ));
        has(&format!(
            r#"<span class="rx-visually-hidden">{n} of 5</span>"#
        ));
    }
    assert_eq!(html.matches(r#"name="score""#).count(), 5, "{html}");
    // `required` sits on the first radio only; nothing is checked.
    has(r#"id="rx-score-1" name="score" value="1" required>"#);
    assert!(!html.contains(" checked"), "{html}");
    has("Stars, one to five.");
    has(r#"data-error-for="score" aria-live="polite"></p>"#);
    // No handlers or style attributes (CSP=strict) and no part of its own.
    assert!(
        !html.contains("onclick") && !html.contains(" style="),
        "{html}"
    );
    assert!(!html.contains("data-rating"), "{html}");

    // Readonly: text for a screen reader, and no input to send.
    has(r#"<span class="rx-label" id="rx-seen-label">Average</span>"#);
    has(r#"role="img" aria-labelledby="rx-seen-label rx-seen-value""#);
    has(r#"id="rx-seen-value">3 of 4</span>"#);
    assert_eq!(html.matches("rx-rating__item--on").count(), 3, "{html}");
    has(r#"id="rx-none_yet-value">Not rated</span>"#);
    assert!(!html.contains(r#"name="seen""#) && !html.contains(r#"name="none_yet""#));

    let html = app.get("/review-tag").await.assert_ok().text();
    assert_eq!(html.matches(r#"name="stars""#).count(), 3, "{html}");
    assert!(
        html.contains(r#"id="rx-stars-2" name="stars" value="2" checked>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<span class="rx-required">(optional)</span>"#),
        "{html}"
    );
}

/// The chosen number arrives as an integer; none at all fails `required`
/// with the field's message; a value out of range fails `between`, and the
/// radio chosen is checked again on the page that shows the error.
#[renox::test]
async fn a_rating_posts_a_plain_integer_through_valid() {
    let app = reviews_app().await;
    app.get("/review").await;
    app.post("/review", &[("score", "4"), ("title", "Fast")])
        .await
        .assert_ok()
        .assert_json_path("score", 4);

    app.request()
        .json()
        .post("/review", &[("title", "Fast")])
        .await
        .assert_status(422)
        .assert_invalid("score");

    // Out of range: the message comes back to the group.
    app.request()
        .header("referer", "/review")
        .post("/review", &[("score", "9"), ("title", "Fast")])
        .await
        .assert_redirect("/review");
    let html = app.get("/review").await.text();
    assert!(html.contains(r#"<p class="rx-error" id="rx-score-error" data-error-for="score" aria-live="polite">The score must be between 1 and 5.</p>"#), "{html}");
    assert!(html.contains(r#"aria-invalid="true""#), "{html}");

    // Another field fails: the radio chosen is checked again.
    app.request()
        .header("referer", "/review")
        .post("/review", &[("score", "3")])
        .await
        .assert_redirect("/review");
    let html = app.get("/review").await.text();
    assert!(
        html.contains(r#"id="rx-score-3" name="score" value="3" checked>"#),
        "{html}"
    );
    assert_eq!(html.matches(" checked").count(), 1, "{html}");
}

/// Every text of the signature pad and the camera capture is a `blocks.*`
/// key (#53): an app's lang file replaces each one, the texts the script
/// speaks (`data-rx-blocks-texts`) included, and the rest stays English.
#[renox::test]
async fn an_app_translates_the_signature_and_camera_texts() {
    let dir = std::env::temp_dir().join(format!("renox-blocks-{}", renox::random_token()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("es.json"),
        r#"{"blocks": {
            "signature": {"area": "Zona de firma", "undo": "Deshacer", "clear": "Borrar",
                "typed": "O escribe tu nombre", "drawn": "Firma hecha.", "help": "Dibuja la firma."},
            "camera": {"shutter": "Hacer foto", "denied": "Sin permiso de cámara."}
        }}"#,
    )
    .unwrap();
    let lang = dir.clone();
    let app = TestApp::with_config(
        App::new()
            .module(Blocks::new())
            .module(Signing)
            .module(Photographing)
            .templates(|env| {
                env.add_template("sign.html", SIGN_MACRO).unwrap();
                renox::view::add_template(env, "sign-tag.html", SIGN_TAG).unwrap();
                env.add_template("photos.html", CAMERA_MACRO).unwrap();
                renox::view::add_template(env, "photos-tag.html", CAMERA_TAG).unwrap();
            }),
        move |c| {
            c.locale = "es".into();
            c.lang_path = lang;
        },
    )
    .await;
    let html = app.get("/sign").await.assert_ok().text();
    for text in [
        r#"aria-label="Zona de firma""#,
        "data-rx-signature-undo>Deshacer</button>",
        "data-rx-signature-clear>Borrar</button>",
        ">O escribe tu nombre</label>",
        "Dibuja la firma.",
        "Firma hecha.",
        // Not translated by this app: English.
        "Last stroke removed.",
    ] {
        assert!(html.contains(text), "missing {text}\n{html}");
    }
    let html = app.get("/photos-tag").await.assert_ok().text();
    for text in [
        "data-rx-camera-shutter>Hacer foto</button>",
        "Sin permiso de cámara.",
        "Photo removed.",
    ] {
        assert!(html.contains(text), "missing {text}\n{html}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
