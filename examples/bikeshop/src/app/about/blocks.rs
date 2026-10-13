//! `/about/blocks`: the UI blocks beyond Renox's kit, each one working.
//!
//! The blocks are the pieces the kit doesn't have (a photo gallery, a
//! two-handle range, a kanban board, a month calendar…). They were built in
//! this example first, then moved to the `renox-blocks` crate (#347):
//! macros in `renox-blocks/blocks.html`, a script and a stylesheet the
//! crate serves to the pages that use them. The date picker with closed
//! days became the kit's own `date_picker` (`disabled_dates`,
//! `closed_weekdays`, #345).
//!
//! This page shows each block with demo data and its macro's signature.
//! The routes behind the demos:
//! - `POST /about/blocks/form`: the form blocks (quantity, keypad, date and
//!   time range, variant chips), read with `Valid<T>` and checked again on
//!   the server;
//! - `GET /about/blocks/variant`: the price and stock of a variant, the
//!   fragment the variant chips ask for with htmx;
//! - `POST /about/blocks/kanban`: a card moved on the demo board (nothing is
//!   saved: the answer only says whether the move is allowed).

use crate::explain::NotAPage;
use renox::chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use renox::prelude::*;
use renox::validation::FormContext;
use serde::{Deserialize, Serialize};

/// The routes of the blocks page and its demos (added to `About::routes`).
pub fn routes() -> Routes {
    Routes::new()
        .get("/about/blocks", page)
        .name("about.blocks")
        .post("/about/blocks/form", submit)
        .name("about.blocks.submit")
        .get("/about/blocks/variant", variant)
        .name("about.blocks.variant")
        .post("/about/blocks/kanban", move_card)
        .name("about.blocks.move")
}

/// The GET routes here that aren't pages.
pub fn not_pages() -> Vec<NotAPage> {
    vec![NotAPage {
        route: "about.blocks.variant",
        reason: "an htmx fragment (the variant's price and stock) swapped into /about/blocks",
    }]
}

/// The shop's date today, in `APP_TIMEZONE` (through Renox's clock, so a
/// test's `travel` moves it).
pub fn today(config: &renox::Config) -> NaiveDate {
    config.timezone.local(renox::db::now().timestamp()).date()
}

/// The variants of the demo bike, and the one sold out.
const SIZES: [(&str, &str, &str); 4] = [
    ("S", "S", "150–165 cm"),
    ("M", "M", "165–178 cm"),
    ("L", "L", "178–190 cm"),
    ("XL", "XL", "190+ cm"),
];
const SOLD_OUT: &str = "XL";
const COLOURS: [(&str, &str, &str); 3] = [
    ("teal", "Teal", "#0b6e66"),
    ("sand", "Sand", "#d8c7a3"),
    ("graphite", "Graphite", "#3f3f46"),
];

/// The price (cents, the `money` filter's smallest unit) and stock of a
/// variant.
pub fn variant_of(size: &str, colour: &str) -> (i64, i64) {
    let price = match size {
        "S" => 149_900,
        "M" => 159_900,
        "L" => 169_900,
        _ => 179_900,
    };
    let stock = match (size, colour) {
        (SOLD_OUT, _) => 0,
        (_, "graphite") => 1,
        ("M", _) => 6,
        _ => 3,
    };
    (price, stock)
}

/// The demo board's columns, as `kanban` takes them.
pub const COLUMNS: [(&str, &str); 3] = [
    ("waiting", "Waiting"),
    ("working", "In the stand"),
    ("ready", "Ready for pickup"),
];

#[derive(Debug, Serialize)]
struct Card {
    id: i64,
    title: &'static str,
    subtitle: &'static str,
    badge: Option<&'static str>,
    badge_kind: Option<&'static str>,
}

/// A demo card: id, column, title, subtitle, and a badge with its kind.
type DemoCard = (
    i64,
    &'static str,
    &'static str,
    &'static str,
    Option<(&'static str, &'static str)>,
);

const CARDS: [DemoCard; 6] = [
    (101, "waiting", "Tune-up", "City 3 · Ana R.", None),
    (
        102,
        "waiting",
        "Flat tyre",
        "Kids 20 · Budi S.",
        Some(("Today", "warning")),
    ),
    (103, "waiting", "Brake bleed", "Trail 5 · Carla M.", None),
    (
        104,
        "working",
        "Chain and cassette",
        "Road 7 · Dewi K.",
        Some(("Parts in", "info")),
    ),
    (105, "working", "Wheel truing", "City 3 · Eko P.", None),
    (
        106,
        "ready",
        "Full service",
        "E-City · Fatima L.",
        Some(("Paid", "success")),
    ),
];

fn board() -> Vec<renox::minijinja::Value> {
    COLUMNS
        .iter()
        .map(|(key, title)| {
            let cards: Vec<Card> = CARDS
                .iter()
                .filter(|c| c.1 == *key)
                .map(|&(id, _, title, subtitle, badge)| Card {
                    id,
                    title,
                    subtitle,
                    badge: badge.map(|b| b.0),
                    badge_kind: badge.map(|b| b.1),
                })
                .collect();
            context! { key, title, cards }
        })
        .collect()
}

/// `?month=2026-10` (the calendar), `?price_min=…&price_max=…` (the price
/// filter) and `?slot=…` (a free slot of the availability timeline).
#[derive(Debug, Default, Deserialize)]
pub struct PageQuery {
    month: Option<String>,
    price_min: Option<i64>,
    price_max: Option<i64>,
    slot: Option<String>,
}

/// The price filter's scale (cents).
pub const PRICE_MIN: i64 = 0;
/// The top of the price filter's scale.
pub const PRICE_MAX: i64 = 1_000_000;

/// `YYYY-MM` when `text` is a month the calendar can show.
fn month_param(text: &str) -> Option<String> {
    let day = NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d").ok()?;
    (1900..=2999)
        .contains(&day.year())
        .then(|| day.format("%Y-%m").to_string())
}

/// Text for a query string: letters, digits and `-._~` as they are, the rest
/// percent-encoded.
fn query_text(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn page(State(state): State<AppState>, Query(query): Query<PageQuery>) -> Result<View> {
    let today = today(&state.config);
    let month = query
        .month
        .as_deref()
        .and_then(month_param)
        .unwrap_or_else(|| today.format("%Y-%m").to_string());

    // The calendar's demo visits, on fixed days of whichever month is shown.
    let visit = |day: u32, time: Option<&str>, title: &str, kind: &str| {
        context! { date => format!("{month}-{day:02}"), time, title, kind }
    };
    let events = vec![
        visit(3, Some("09:30"), "Tune-up · City 3", "info"),
        visit(6, Some("10:00"), "Brake bleed · Trail 5", "info"),
        visit(6, None, "Rental: Road 7, all day", "success"),
        visit(12, Some("14:00"), "Plan visit · Rider", "info"),
        visit(18, Some("11:30"), "Wheel truing · Kids 20", "warning"),
        visit(24, None, "Group ride rentals (6 bikes)", "success"),
        visit(27, Some("16:00"), "Warranty check · E-City", "error"),
    ];

    // Bikes against today's hours: booked slots span the hours they take.
    let hours: Vec<String> = (9..=17).map(|h| format!("{h:02}:00")).collect();
    let slot_url = |bike: &str, hour: u32| {
        format!(
            "{}?slot={}#availability",
            state.url("about.blocks", &[]).unwrap_or_default(),
            query_text(&format!("{bike} {hour:02}:00"))
        )
    };
    let free = |bike: &str, hour: u32| context! { state => "free", url => slot_url(bike, hour) };
    let booked = |span: u32, title: &str| context! { state => "booked", span, title };
    let rows = vec![
        context! { label => "City 3", note => "M · #04", slots => vec![
        booked(2, "Ana R."), free("City 3", 11), free("City 3", 12), booked(3, "Group ride"),
        free("City 3", 16), free("City 3", 17)] },
        context! { label => "Trail 5", note => "L · #12", slots => vec![
        free("Trail 5", 9), free("Trail 5", 10), booked(4, "Carla M."), free("Trail 5", 15),
        free("Trail 5", 16), context! { state => "closed" }] },
        context! { label => "Road 7", note => "M · #21", slots => vec![booked(9, "Dewi K., all day")] },
        context! { label => "E-City", note => "S · #30", slots => vec![
        free("E-City", 9), booked(1, "Eko P."), free("E-City", 11), free("E-City", 12),
        free("E-City", 13), booked(2, "Fatima L."), free("E-City", 16), free("E-City", 17)] },
    ];

    let at = |hour: u32, minute: u32| {
        today
            .and_hms_opt(hour, minute, 0)
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string())
    };
    let timeline = vec![
        context! { time => at(9, 12), title => "Checked in at the Harbour store", body => "Brakes squeal; the chain is dry. The customer wants it **by Friday**.", kind => "info", by => "Marta" },
        context! { time => at(9, 40), title => "Quote accepted", body => "Brake pads and a new chain: `$84.00`.", kind => "success", by => "Ana R." },
        context! { time => at(11, 5), title => "Waiting for parts", body => "The chain comes from the Hill store this afternoon.", kind => "warning", by => "Marta" },
        context! { time => at(15, 30), title => "Serviced", kind => "success", by => "Joko" },
        context! { time => at(16, 2), title => "Ready for pickup", body => "A text message went to the customer.", kind => "info" },
    ];

    let photo = |file: &str, alt: &str, caption: Option<&str>| {
        context! { src => format!("/blocks/demo/{file}.svg"), alt, caption }
    };
    let photos = vec![
        photo(
            "city-side",
            "The City 3 from the side, in teal",
            Some("City 3, teal, size M"),
        ),
        photo(
            "city-front",
            "The City 3 from the front, with its lamp",
            None,
        ),
        photo(
            "city-detail",
            "A wheel's hub and spokes, close up",
            Some("Sealed hubs, 32 spokes"),
        ),
        photo("city-ride", "The City 3 on a road", None),
    ];

    let plans = vec![
        context! { key => "basic", name => "Basic", price => 2_900, interval => "month",
        description => "For a bike ridden at weekends.", perks => vec!["A check-up every month", "10% off parts"],
        url => "#plans" },
        context! { key => "rider", name => "Rider", price => 4_900, interval => "month",
        description => "For the daily commute.", perks => vec!["Two visits a month", "Pickup and delivery", "15% off parts"],
        url => "#plans" },
        context! { key => "pro", name => "Pro", price => 7_900, interval => "month",
        description => "For a fleet, or a racer.", perks => vec!["Unlimited visits", "A loan bike while yours is in", "20% off parts"],
        url => "#plans" },
    ];
    let feature = |label: &str,
                   basic: renox::minijinja::Value,
                   rider: renox::minijinja::Value,
                   pro: renox::minijinja::Value| {
        context! { label, values => context! { basic, rider, pro } }
    };
    let yes = || renox::minijinja::Value::from(true);
    let no = || renox::minijinja::Value::from(false);
    let text = |t: &str| renox::minijinja::Value::from(t);
    let features = vec![
        feature("Visits a month", text("1"), text("2"), text("Unlimited")),
        feature("Discount on parts", text("10%"), text("15%"), text("20%")),
        feature("Pickup and delivery", no(), yes(), yes()),
        feature("Loan bike", no(), no(), yes()),
        feature("Priority in the workshop", no(), yes(), yes()),
    ];

    let sizes: Vec<_> = SIZES
        .iter()
        .map(|(value, label, note)| context! { value, label, note, disabled => *value == SOLD_OUT })
        .collect();
    let colours: Vec<_> = COLOURS
        .iter()
        .map(|(value, label, color)| context! { value, label, color })
        .collect();
    let (price, stock) = variant_of("M", "teal");

    let price_filter = match (query.price_min, query.price_max) {
        (Some(low), Some(high)) => Some(context! { low, high }),
        _ => None,
    };

    Ok(view(
        "about/blocks.html",
        context! {
            today => today.to_string(),
            max_day => (today + Duration::days(60)).to_string(),
            month,
            events,
            hours,
            rows,
            timeline,
            photos,
            plans,
            features,
            sizes,
            colours,
            price,
            stock,
            columns => board(),
            price_filter,
            price_min => PRICE_MIN,
            price_max => PRICE_MAX,
            slot => query.slot,
        },
    ))
}

/// The form blocks, as `Valid<T>` reads them: every block sends plain fields.
#[derive(Debug, Deserialize)]
pub struct BlocksForm {
    /// From `quantity`.
    pub quantity: Option<i64>,
    /// From the field the `keypad` types into.
    pub paid: Option<String>,
    /// From `datetime_range` (`YYYY-MM-DDTHH:MM`).
    pub starts_at: Option<NaiveDateTime>,
    /// From `datetime_range`.
    pub ends_at: Option<NaiveDateTime>,
    /// From the size `swatches`.
    pub size: Option<String>,
    /// From the colour `swatches`.
    pub colour: Option<String>,
}

// [explain:about.blocks.rules]
impl Validate for BlocksForm {
    fn rules(&self, v: &mut Validator) {
        v.field("quantity", &self.quantity).required().between(1, 5);
        v.field("paid", &self.paid).required().digits_between(1, 9);
        v.field("starts_at", &self.starts_at).required();
        v.field("ends_at", &self.ends_at)
            .required()
            .gt("starts_at", &self.starts_at);
        // [/explain:about.blocks.rules]
        // The sold-out size can't be bought, whatever the page sent.
        let sizes: Vec<&str> = SIZES
            .iter()
            .map(|s| s.0)
            .filter(|s| *s != SOLD_OUT)
            .collect();
        v.field("size", &self.size).required().one_of(&sizes);
        let colours: Vec<&str> = COLOURS.iter().map(|c| c.0).collect();
        v.field("colour", &self.colour).required().one_of(&colours);
    }

    // [explain:about.blocks.rules]
    // The check that needs the shop's date: anyone can send any date.
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        let today = today(&form.state.config);
        let lang = form.state.current_lang();
        if let Some(start) = self.starts_at
            && start.date() < today
        {
            errors.add("starts_at", lang.t("blocks.page.errors.past_start", &[]));
        }
        Ok(())
    }
    // [/explain:about.blocks.rules]
}

async fn submit(session: Session, lang: Lang, Valid(form): Valid<BlocksForm>) -> Result<Response> {
    let summary = format!(
        "{} × {} {}, paid {}, rental {} → {}",
        form.quantity.unwrap_or_default(),
        form.size.as_deref().unwrap_or_default(),
        form.colour.as_deref().unwrap_or_default(),
        form.paid.as_deref().unwrap_or_default(),
        form.starts_at
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default(),
        form.ends_at
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default(),
    );
    session.flash(
        "status",
        lang.t("blocks.page.received", &[("summary", &summary)]),
    )?;
    Ok(Redirect::to("/about/blocks#form").into_response())
}

/// `?size=M&colour=teal` from the variant chips (`hx-include` sends both).
#[derive(Debug, Deserialize)]
struct VariantQuery {
    size: Option<String>,
    colour: Option<String>,
}

/// The price and stock of a variant: the fragment the chips swap in.
async fn variant(Query(query): Query<VariantQuery>) -> View {
    let size = query.size.unwrap_or_else(|| "M".into());
    let colour = query.colour.unwrap_or_else(|| "teal".into());
    let (price, stock) = variant_of(&size, &colour);
    view("about/_variant.html", context! { price, stock })
}

/// A card moved on the demo board.
#[derive(Debug, Deserialize)]
pub struct MoveForm {
    /// The card's id.
    pub card: Option<i64>,
    /// The column's key.
    pub column: Option<String>,
    /// Its place in the column, from 0.
    pub position: Option<i64>,
}

impl Validate for MoveForm {
    fn rules(&self, v: &mut Validator) {
        let ids: Vec<String> = CARDS.iter().map(|c| c.0.to_string()).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let card = self.card.map(|c| c.to_string());
        v.field("card", &card).required().one_of(&ids);
        let keys: Vec<&str> = COLUMNS.iter().map(|c| c.0).collect();
        v.field("column", &self.column).required().one_of(&keys);
        v.field("position", &self.position).required().min(0);
    }
}

// [explain:about.blocks.kanban]
/// Accepts a move (204: nothing to swap; the board has already moved the
/// card). A real board would save the card's column and order here, after
/// checking the person may move it. An invalid move gets Renox's 422, and
/// the board puts the card back.
async fn move_card(Valid(_form): Valid<MoveForm>) -> StatusCode {
    StatusCode::NO_CONTENT
}
// [/explain:about.blocks.kanban]
