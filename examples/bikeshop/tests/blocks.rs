//! The blocks of renox-blocks on `/about/blocks`:
//! every block renders with its demo data, the month calendar works out its
//! grid in the template, and the demo routes behind the blocks check what
//! they get on the server (the form blocks through `Valid<T>`, the variant
//! fragment, a kanban move). The blocks' behaviour in a browser is
//! `tests/browser/bikeshop-blocks.test.mjs`.

use bikeshop::app::about::blocks;
use renox::chrono::{Duration, NaiveDate};
use renox::testing::TestApp;

/// The shop's date in the test app (UTC: `Config::default()`).
fn today(app: &TestApp) -> NaiveDate {
    blocks::today(&app.state().config)
}

/// The day `from` days ahead.
fn open_day(app: &TestApp, from: i64) -> NaiveDate {
    today(app) + Duration::days(from)
}

#[renox::test]
async fn the_page_shows_every_block_with_its_signature() {
    let app = TestApp::new(bikeshop::app()).await;
    let page = app.get("/about/blocks").await;
    page.assert_ok().assert_view("about/blocks.html");
    for marker in [
        "data-rx-gallery",
        "data-rx-range",
        "data-rx-quantity",
        "data-rx-keypad=\"rx-paid\"",
        "data-rx-kanban",
        "class=\"rx-month\"",
        "class=\"rx-availability\"",
        "data-rx-datetime-range",
        "rx-swatches--size",
        "rx-swatches--colour",
        "class=\"rx-history\"",
        "class=\"rx-plans\"",
    ] {
        page.assert_see(marker);
    }
    for signature in [
        "gallery(photos, id=",
        "range_slider(name_min, name_max, min, max",
        "quantity(name, value=1",
        "keypad(target,",
        "kanban(id, columns, url",
        "kanban_card(card)",
        "month_calendar(month, events=[]",
        "availability(columns, rows",
        "datetime_range(name_start, name_end",
        "swatches(name, label, options",
        "history(items",
        "compare_plans(plans, features=[]",
    ] {
        page.assert_see(signature);
    }
    // The crate's stylesheet and loader, once; the layouts load the
    // showcase page's own styles.
    page.assert_see("/_renox/blocks/blocks-")
        .assert_see("data-renox-blocks")
        .assert_see("blocks/blocks.css")
        .assert_dont_see("blocks/blocks.js");
    assert_eq!(page.text().matches("data-renox-blocks").count(), 1);
    // Its "About this page" panel.
    page.assert_see("id=\"about-page\"")
        .assert_see("Bike shop blocks");
}

#[renox::test]
async fn the_blocks_render_their_fields_for_valid_t() {
    let app = TestApp::new(bikeshop::app()).await;
    let page = app.get("/about/blocks").await;
    // Plain fields: two range inputs, a number, the hidden date-times, radios.
    page.assert_see(r#"type="range" id="rx-price_min-range-min" name="price_min""#)
        .assert_see(r#"name="price_max""#)
        .assert_see(r#"name="quantity" type="number""#)
        .assert_see(r#"type="hidden" name="starts_at""#)
        .assert_see(r#"type="hidden" name="ends_at""#)
        .assert_see(r#"type="radio" name="size" value="M" checked"#)
        // The sold-out size is shown, disabled and said to be sold out.
        .assert_see(r#"name="size" value="XL" disabled"#)
        .assert_see("(sold out)")
        // The kanban's hidden form posts to the demo route with htmx.
        .assert_see(r#"hx-post="/about/blocks/kanban" hx-trigger="rx:kanban-move""#)
        // Every free slot says what it books.
        .assert_see("Book Trail 5 at 09:00")
        // The variant chips ask for the price and the stock.
        .assert_see(r#"hx-get="/about/blocks/variant""#);
}

#[renox::test]
async fn the_month_calendar_works_out_its_grid() {
    let app = TestApp::new(bikeshop::app()).await;
    // February 2026 has 28 days and starts on a Sunday: the seventh column
    // when weeks start on Monday.
    app.get("/about/blocks?month=2026-02")
        .await
        .assert_ok()
        .assert_see("February 2026")
        .assert_see(r#"<time datetime="2026-02-28">"#)
        .assert_dont_see(r#"datetime="2026-02-29""#)
        .assert_see("--rx-month-start: 7")
        .assert_see(r#"<span class="rx-month__weekday">Sunday </span><span class="rx-month__number">1</span>"#)
        // The months around it.
        .assert_see("/about/blocks?month=2026-01")
        .assert_see("/about/blocks?month=2026-03")
        .assert_see("Tune-up · City 3");
    // 2024 was a leap year; February started on a Thursday.
    app.get("/about/blocks?month=2024-02")
        .await
        .assert_see(r#"datetime="2024-02-29""#)
        .assert_see("--rx-month-start: 4");
    // December links to January of the next year.
    app.get("/about/blocks?month=2026-12")
        .await
        .assert_see("/about/blocks?month=2027-01")
        .assert_see("/about/blocks?month=2026-11");
    // Today is marked; a month that isn't one shows this month.
    let today = today(&app);
    app.get("/about/blocks?month=nope")
        .await
        .assert_ok()
        .assert_see(&format!(r#"datetime="{}""#, today))
        .assert_see("rx-month__day--today");
}

#[renox::test]
async fn the_price_filter_and_a_free_slot_come_back_in_the_address() {
    let app = TestApp::new(bikeshop::app()).await;
    app.get("/about/blocks?price_min=200000&price_max=600000")
        .await
        .assert_ok()
        .assert_see("Showing bikes from $2,000.00 to $6,000.00.")
        .assert_see(r#"value="200000""#)
        .assert_see(r#"value="600000""#);
    app.get("/about/blocks?slot=Trail%205%2010%3A00")
        .await
        .assert_see("Booking Trail 5 10:00");
}

#[renox::test]
async fn the_form_blocks_are_accepted_when_valid() {
    let app = TestApp::new(bikeshop::app()).await;
    let start = open_day(&app, 1);
    let starts_at = format!("{start}T10:00");
    let ends_at = format!("{}T12:30", start + Duration::days(1));
    app.post(
        "/about/blocks/form",
        &[
            ("quantity", "2"),
            ("paid", "350000"),
            ("starts_at", &starts_at),
            ("ends_at", &ends_at),
            ("size", "M"),
            ("colour", "sand"),
        ],
    )
    .await
    .assert_redirect("/about/blocks#form");
    app.get("/about/blocks")
        .await
        .assert_see("The server accepted: 2 × M sand, paid 350000");
}

#[renox::test]
async fn the_server_refuses_what_the_blocks_only_discourage() {
    let app = TestApp::new(bikeshop::app()).await;
    let today = today(&app);
    let start = open_day(&app, 1);
    let starts_at = format!("{start}T10:00");
    let before = format!("{start}T09:00");
    let good = |field: &'static str, value: String| {
        let mut form = vec![
            ("quantity", "1".to_owned()),
            ("paid", "1000".to_owned()),
            ("starts_at", starts_at.clone()),
            ("ends_at", format!("{start}T11:00")),
            ("size", "S".to_owned()),
            ("colour", "teal".to_owned()),
        ];
        for pair in form.iter_mut() {
            if pair.0 == field {
                pair.1 = value.clone();
            }
        }
        form
    };
    for (field, value) in [
        ("starts_at", format!("{}T10:00", today - Duration::days(1))), // the past
        ("size", "XL".to_owned()),                                     // sold out
        ("ends_at", before.clone()),                                   // ends before it starts
        ("quantity", "9".to_owned()),                                  // over the stepper's max
        ("paid", "12a".to_owned()),                                    // not digits
    ] {
        let form = good(field, value);
        let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
        app.htmx()
            .post("/about/blocks/form", &pairs)
            .await
            .assert_invalid(field);
    }
}

#[renox::test]
async fn a_variant_answers_with_its_price_and_stock() {
    let app = TestApp::new(bikeshop::app()).await;
    app.htmx()
        .get("/about/blocks/variant?size=M&colour=teal")
        .await
        .assert_ok()
        .assert_see("6 in stock")
        .assert_see(r#"id="variant""#);
    app.htmx()
        .get("/about/blocks/variant?size=XL&colour=teal")
        .await
        .assert_see("Out of stock");
}

#[renox::test]
async fn a_kanban_move_is_checked_on_the_server() {
    let app = TestApp::new(bikeshop::app()).await;
    app.htmx()
        .post(
            "/about/blocks/kanban",
            &[("card", "101"), ("column", "working"), ("position", "0")],
        )
        .await
        .assert_status(204);
    app.htmx()
        .post(
            "/about/blocks/kanban",
            &[("card", "101"), ("column", "nowhere"), ("position", "0")],
        )
        .await
        .assert_invalid("column");
    app.htmx()
        .post(
            "/about/blocks/kanban",
            &[("card", "999"), ("column", "ready"), ("position", "0")],
        )
        .await
        .assert_invalid("card");
}

#[renox::test]
async fn the_page_speaks_spanish() {
    let app = TestApp::new(bikeshop::app()).await;
    app.request()
        .header("Accept-Language", "es")
        .get("/about/blocks?month=2026-10")
        .await
        .assert_ok()
        .assert_see("Octubre 2026")
        .assert_see("Foto anterior")
        .assert_see("Reservar Trail 5 a las 09:00");
}
