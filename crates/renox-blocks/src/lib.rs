//! Interactive blocks for Renox apps: components the UI kit doesn't carry,
//! as template macros on the kit's tokens, with their JavaScript loaded only
//! on pages that use them.
//!
//! ```
//! use renox::prelude::*;
//! use renox_blocks::Blocks;
//!
//! # let _ =
//! App::new().module(Blocks::new())
//! # ;
//! ```
//!
//! Then, in a template:
//!
//! ```html
//! {% from "renox-blocks/blocks.html" import quantity, swatches, gallery %}
//! <form method="post" action="/cart">
//!   {{ csrf_field() }}
//!   {{ swatches("size", "Frame size", sizes, selected="M", required=true) }}
//!   {{ quantity("qty", 1, min=1, max=10) }}
//! </form>
//! ```
//!
//! Each block is also a tag: `<rx-quantity name="qty" :value="1" :min="1" :max="10"/>`,
//! `<rx-swatches>`, `<rx-history :items="events"/>`… (the macro's name with dashes, its
//! parameters as attributes: `hide-label`, `value-start`).
//!
//! - Input: `quantity` (a stepper), `range_slider` (two handles),
//!   `keypad` (a point-of-sale number pad), `swatches` (variant chips),
//!   `datetime_range` (a start and an end, each a day and a time),
//!   `signature_pad` (a drawn or typed signature, sent as a PNG file or a name),
//!   `camera_capture` (photos from the camera, sent as JPEG files),
//!   `map_picker` (a point, circle or polygon on a map, with typed coordinates).
//! - Showing data: `gallery` (a carousel), `history` (a timeline),
//!   `compare_plans` (a pricing table), `status_stepper` (a record's
//!   progress through its statuses), `map` (markers and shapes).
//! - Scheduling and work: `month_calendar`, `availability` (resources
//!   against hours), `week_calendar` (resources against a week, click a
//!   free slot to book it), `kanban` (with `kanban_card`).
//! - Printing: `receipt` (a till receipt for an 80 mm or 58 mm thermal printer).
//!
//! The form blocks send plain fields, so `Valid<T>` reads them as any
//! other. Each block works with the keyboard, follows
//! `prefers-reduced-motion` and works under `CSP=strict`. Its texts are
//! English, and an app translates them in its lang files ([`TEXTS`] lists
//! the keys). The guide is docs/blocks.md in the Renox repository.

#![warn(missing_docs)]

use renox::minijinja::value::{Kwargs, Value};
use renox::minijinja::{self, State};
use renox::prelude::*;

mod assets;
mod map;
mod texts;
mod week;

pub use texts::TEXTS;

/// The macros, compiled in. An app replaces the file with one of the same
/// name under its views directory (`resources/views/renox-blocks/blocks.html`).
const VIEWS: &[(&str, &str)] = &[(
    "renox-blocks/blocks.html",
    include_str!("../views/blocks.html"),
)];

/// The blocks module: add it to the app, then import the macros from
/// `renox-blocks/blocks.html`.
#[derive(Debug, Clone, Default)]
pub struct Blocks {
    map_tiles: Option<String>,
    map_attribution: Option<String>,
}

impl Blocks {
    /// The module with its defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// The map blocks' tile URL template (`https://tiles.example.com/{z}/{x}/{y}.png`),
    /// instead of the `BLOCKS_MAP_TILES` variable. Without either, the map
    /// blocks show an explanatory note and the picker its typed fields.
    pub fn map_tiles(mut self, template: impl Into<String>) -> Self {
        self.map_tiles = Some(template.into());
        self
    }

    /// The text the maps show for their tiles' source, instead of the
    /// `BLOCKS_MAP_ATTRIBUTION` variable.
    pub fn map_attribution(mut self, text: impl Into<String>) -> Self {
        self.map_attribution = Some(text.into());
        self
    }
}

impl Module for Blocks {
    fn name(&self) -> &'static str {
        "blocks"
    }

    fn register(&self, app: &mut Registry) {
        assets::register(app);
        let (tiles, attribution) = (self.map_tiles.clone(), self.map_attribution.clone());
        app.templates(move |env| {
            for (name, source) in VIEWS {
                // The app's own file of that name wins.
                if env.get_template(name).is_err() {
                    let _ = env.add_template(name, source);
                }
            }
            env.add_function("renox_blocks", || {
                minijinja::Value::from_safe_string(assets::tags())
            });
            env.add_function("renox_blocks_t", text);
            env.add_function("renox_week_grid", week::week_grid);
            let (tiles, attribution) = (tiles.clone(), attribution.clone());
            env.add_function("renox_blocks_map", move || {
                map::settings(tiles.as_deref(), attribution.as_deref())
            });
        });
        components(app);
    }
}

/// The blocks as tags: `<rx-quantity>`, `<rx-range-slider>`, `<rx-keypad>`,
/// `<rx-swatches>`, `<rx-datetime-range>`, `<rx-gallery>`, `<rx-history>`,
/// `<rx-compare-plans>`, `<rx-status-stepper>`, `<rx-month-calendar>`,
/// `<rx-availability>`, `<rx-week-calendar>`, `<rx-signature-pad>`, `<rx-camera-capture>`, `<rx-map-picker>`, `<rx-map>`, `<rx-kanban>` and `<rx-receipt>`. Each is the macro of
/// the same name, with its parameters as
/// attributes (`value-start`, `hide-label`…).
fn components(app: &mut Registry) {
    use renox::view::{Component, Prop};
    const T: &str = "renox-blocks/blocks.html";
    let block = |tag: &str, mac: &str, doc: &str| {
        Component::macro_call(tag, T, mac).doc(doc).content(false)
    };
    app.component(
        block(
            "rx-quantity",
            "quantity",
            "A number with minus and plus buttons.",
        )
        .attrs(true)
        .prop(Prop::text("name").required())
        .prop(Prop::number("value"))
        .prop(Prop::number("min"))
        .prop(Prop::number("max"))
        .prop(Prop::number("step"))
        .prop(Prop::text("label"))
        .prop(Prop::bool("hide-label"))
        .prop(Prop::text("hint"))
        .prop(Prop::text("id"))
        .prop(Prop::text("bag")),
    );
    app.component(
        block("rx-range-slider", "range_slider", "A two-handle range.")
            .prop(Prop::text("name-min").required())
            .prop(Prop::text("name-max").required())
            .prop(Prop::number("min").required())
            .prop(Prop::number("max").required())
            .prop(Prop::number("step"))
            .prop(Prop::number("value-min"))
            .prop(Prop::number("value-max"))
            .prop(Prop::text("label"))
            .prop(Prop::text("format"))
            .prop(Prop::text("prefix"))
            .prop(Prop::text("suffix"))
            .prop(Prop::text("hint"))
            .prop(Prop::text("id"))
            .prop(Prop::text("bag")),
    );
    app.component(
        block("rx-keypad", "keypad", "A point-of-sale number pad.")
            .prop(Prop::text("target").required())
            .prop(Prop::text("label"))
            .prop(Prop::bool("decimal"))
            .prop(Prop::bool("zeros"))
            .prop(Prop::text("enter-label")),
    );
    app.component(
        block(
            "rx-swatches",
            "swatches",
            "Variant chips: sizes or colours.",
        )
        .prop(Prop::text("name").required())
        .prop(Prop::text("label").required())
        .prop(Prop::data("options").required())
        .prop(Prop::text("selected"))
        .prop(Prop::choice("kind", &["size", "colour"]))
        .prop(Prop::data("attrs"))
        .prop(Prop::bool("required"))
        .prop(Prop::text("hint"))
        .prop(Prop::text("id"))
        .prop(Prop::text("bag")),
    );
    app.component(
        block(
            "rx-datetime-range",
            "datetime_range",
            "A start and an end, each a day and a time.",
        )
        .prop(Prop::text("name-start").required())
        .prop(Prop::text("name-end").required())
        .prop(Prop::text("label"))
        .prop(Prop::text("value-start"))
        .prop(Prop::text("value-end"))
        .prop(Prop::text("min"))
        .prop(Prop::text("max"))
        .prop(Prop::number("step"))
        .prop(Prop::number("opens"))
        .prop(Prop::number("closes"))
        .prop(Prop::bool("required"))
        .prop(Prop::text("hint"))
        .prop(Prop::text("id"))
        .prop(Prop::text("bag")),
    );
    app.component(
        block(
            "rx-signature-pad",
            "signature_pad",
            "A signature drawn or typed, sent as a PNG file or a name.",
        )
        .prop(Prop::text("name").required())
        .prop(Prop::text("label"))
        .prop(Prop::text("name-label"))
        .prop(Prop::bool("required"))
        .prop(Prop::number("width"))
        .prop(Prop::number("height"))
        .prop(Prop::text("hint"))
        .prop(Prop::text("id"))
        .prop(Prop::text("bag")),
    );
    app.component(
        block(
            "rx-camera-capture",
            "camera_capture",
            "Photos taken with the camera, sent as files; a file input when there is no camera.",
        )
        .prop(Prop::text("name").required())
        .prop(Prop::text("label"))
        .prop(Prop::text("facing"))
        .prop(Prop::number("max"))
        .prop(Prop::number("max-width"))
        .prop(Prop::number("quality"))
        .prop(Prop::text("accept"))
        .prop(Prop::bool("required"))
        .prop(Prop::text("hint"))
        .prop(Prop::text("id"))
        .prop(Prop::text("bag")),
    );
    app.component(
        block(
            "rx-map-picker",
            "map_picker",
            "A point, circle or polygon chosen on a map, with typed coordinates.",
        )
        .prop(Prop::text("name").required())
        .prop(Prop::choice("mode", &["point", "circle", "polygon"]))
        .prop(Prop::data("value"))
        .prop(Prop::text("label"))
        .prop(Prop::number("height"))
        .prop(Prop::text("hint"))
        .prop(Prop::text("id"))
        .prop(Prop::text("bag")),
    );
    app.component(
        block("rx-map", "map", "Markers and shapes on a map.")
            .prop(Prop::data("markers"))
            .prop(Prop::data("shapes"))
            .prop(Prop::text("label"))
            .prop(Prop::number("height"))
            .prop(Prop::text("id")),
    );
    app.component(
        block("rx-gallery", "gallery", "A photo carousel with a zoom.")
            .prop(Prop::data("photos").required())
            .prop(Prop::text("id"))
            .prop(Prop::text("label"))
            .prop(Prop::bool("enlarge")),
    );
    app.component(
        block("rx-history", "history", "A timeline.")
            .prop(Prop::data("items").required())
            .prop(Prop::text("label"))
            .prop(Prop::text("date-format"))
            .prop(Prop::text("id")),
    );
    app.component(
        block("rx-compare-plans", "compare_plans", "A pricing table.")
            .prop(Prop::data("plans").required())
            .prop(Prop::data("features"))
            .prop(Prop::text("highlight"))
            .prop(Prop::text("highlight-label"))
            .prop(Prop::text("label"))
            .prop(Prop::bool("table"))
            .prop(Prop::text("id")),
    );
    app.component(
        block(
            "rx-month-calendar",
            "month_calendar",
            "A month grid of events.",
        )
        .prop(Prop::data("month").required())
        .prop(Prop::data("events"))
        .prop(Prop::text("url"))
        .prop(Prop::text("param"))
        .prop(Prop::text("today"))
        .prop(Prop::number("first-day"))
        .prop(Prop::text("label"))
        .prop(Prop::text("heading"))
        .prop(Prop::text("id")),
    );
    app.component(
        block(
            "rx-availability",
            "availability",
            "Resources against hours.",
        )
        .prop(Prop::data("columns").required())
        .prop(Prop::data("rows").required())
        .prop(Prop::text("label"))
        .prop(Prop::text("corner"))
        .prop(Prop::text("id")),
    );
    app.component(
        block(
            "rx-status-stepper",
            "status_stepper",
            "Where a record is in its statuses.",
        )
        .prop(Prop::data("steps").required())
        .prop(Prop::text("current"))
        .prop(Prop::choice("orientation", &["horizontal", "vertical"]))
        .prop(Prop::text("label"))
        .prop(Prop::text("id")),
    );
    app.component(
        block(
            "rx-week-calendar",
            "week_calendar",
            "Resources against a week, with click-to-book slots.",
        )
        .prop(Prop::data("resources").required())
        .prop(Prop::data("events"))
        .prop(Prop::text("start").required())
        .prop(Prop::number("days"))
        .prop(Prop::number("from"))
        .prop(Prop::number("to"))
        .prop(Prop::number("step"))
        .prop(Prop::text("slot-url"))
        .prop(Prop::text("slot-target"))
        .prop(Prop::text("slot-swap"))
        .prop(Prop::text("label"))
        .prop(Prop::text("id")),
    );
    app.component(
        block(
            "rx-kanban",
            "kanban",
            "A board of cards moved between columns.",
        )
        .prop(Prop::text("id").required())
        .prop(Prop::data("columns").required())
        .prop(Prop::text("url").required())
        .prop(Prop::text("label"))
        .prop(Prop::data("values")),
    );
    // Content: what is between the tags is the receipt's code slot (a QR code, a number).
    app.component(
        Component::macro_call(
            "rx-receipt",
            T,
            "receipt",
        )
        .doc("A till receipt for an 80 mm or 58 mm thermal printer; its content is the code slot.")
        .prop(Prop::data("lines").required())
        .prop(Prop::data("totals"))
        .prop(Prop::choice("width", &["80mm", "58mm"]))
        .prop(Prop::text("title"))
        .prop(Prop::data("header"))
        .prop(Prop::text("number"))
        .prop(Prop::text("date"))
        .prop(Prop::data("footer"))
        .prop(Prop::bool("print-button"))
        .prop(Prop::text("print-label"))
        .prop(Prop::text("label"))
        .prop(Prop::text("id")),
    );
}

/// `renox_blocks_t('blocks.gallery.position', n=1, total=4)`: the app's
/// translation of a block's text (through the page's `t`), else the
/// English one, with its `:name` placeholders filled.
fn text(state: &State, key: &str, kwargs: Kwargs) -> Result<String, minijinja::Error> {
    let mut params: Vec<(String, Value)> = Vec::new();
    for name in kwargs.args() {
        params.push((name.to_owned(), kwargs.get::<Value>(name)?));
    }
    kwargs.assert_all_used()?;
    let translated = state
        .lookup("t")
        .and_then(|t| {
            let args = Kwargs::from_iter(params.iter().map(|(k, v)| (k.clone(), v.clone())));
            t.call(state, &[Value::from(key), Value::from(args)]).ok()
        })
        .and_then(|said| said.as_str().map(str::to_owned));
    if let Some(said) = translated
        && said != key
    {
        return Ok(said);
    }
    let english = texts::english(key).unwrap_or(key);
    let params: Vec<(&str, String)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v.to_string()))
        .collect();
    Ok(renox::i18n::format(english, &params, None))
}

/// Compiles the Rust in docs/blocks.md (the guide) as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/blocks.md")]
pub struct Guide;
