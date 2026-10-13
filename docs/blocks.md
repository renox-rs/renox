# Blocks: interactive components beyond the kit

The UI kit covers forms, tables, sheets and pages. Some screens need more: a stepper for a
quantity, a price filter with two handles, photos in a carousel, a month of bookings, a board
of cards to drag. The `renox-blocks` crate adds eleven such components, as template macros
built on the kit:

| Input | Showing data | Scheduling and work |
|---|---|---|
| [`quantity`](#quantity): a stepper | [`gallery`](#gallery): photos in a carousel | [`month_calendar`](#month_calendar): a month of events |
| [`range_slider`](#range_slider): two handles on one track | [`history`](#history): a timeline | [`availability`](#availability): resources against hours |
| [`keypad`](#keypad): a point-of-sale number pad | [`compare_plans`](#compare_plans): pricing cards and a table | [`kanban`](#kanban): cards moved between columns |
| [`swatches`](#swatches): variant chips | | |
| [`datetime_range`](#datetime_range): a start and an end, with times | | |

They're a separate crate, not part of the kit, so the kit stays small: a page loads the
blocks' script only when it has a block, and then only the code of the blocks it has.
[examples/bikeshop](../examples/bikeshop) uses every one of them; its `/about/blocks` page
shows them side by side.

In this guide:

- [Add it to your app](#add-it-to-your-app)
- [How blocks behave](#how-blocks-behave)
- [Input blocks](#input-blocks)
- [Showing data](#showing-data)
- [Scheduling and work](#scheduling-and-work)
- [Texts in other languages](#texts-in-other-languages)
- [Changing the look](#changing-the-look)
- [How the files load](#how-the-files-load)
- [Testing](#testing)

### Words you'll meet

| Word | What it means |
|---|---|
| **block** | One of this crate's components: a template macro, plus a little JavaScript when it needs some. |
| **macro** | A MiniJinja function that prints HTML, imported with `{% from "…" import … %}`. |
| **plain field** | A normal form field (`<input name="qty">`), so the server reads it as any other. |
| **live region** | Part of a page a screen reader reads out when its text changes. |

> [!NOTE]
> **Coming from Laravel:** Laravel has no such components of its own; these are the kind of
> Blade or Livewire components a team writes once per project (a quantity stepper, a pricing
> table, a kanban board), here written once, on Renox's kit.

## Add it to your app

Add the crate next to `renox`, at the same version:

```toml
[dependencies]
renox = "1.1"
renox-blocks = "1.1"
```

Then add its module:

```rust
use renox::prelude::*;
use renox_blocks::Blocks;

pub fn app() -> App {
    App::new().module(Blocks::new())
}
```

The module adds the macros (`renox-blocks/blocks.html`) and serves the blocks' stylesheet and
scripts under `/_renox/blocks/`. The blocks are built on the UI kit, so the page's layout needs
`{{ renox_ui() }}` as for the kit's own components. Import what a page uses:

```html
{% from "renox-blocks/blocks.html" import quantity, swatches, gallery %}
```

## How blocks behave

Every block follows the same rules, so you can rely on them without reading each one:

- **Plain fields.** The input blocks send normal form fields, so `Valid<T>` reads them, a failed
  submit shows the old input and the error under the block (the kit's `rx-error` place), and
  `bag=` names an error bag as for the kit's fields. Every value is checked on the server
  again: a block that greys something out is a help, not a guarantee.
- **The keyboard.** Everything a pointer does, the keyboard does too; each block below says
  how.
- **Screen readers.** Every control has a name, states are said in words (never by colour
  alone), and the kanban board announces each move.
- **The kit's look.** Classes start with `rx-` like the kit's, colours, type, spacing and radii
  are the kit's tokens (`--rx-*`), and layouts are CSS grid, so themes, dark mode, the accent
  and the type scale follow the kit. Text meets WCAG AA contrast in light and dark.
- **Motion.** The gallery slides, a moved card glides into place and a timeline fades in, with
  the browser's own Web Animations on `transform` and `opacity`. Under
  `prefers-reduced-motion` nothing moves.
- **CSP.** No inline scripts or handlers: the blocks work under `CSP=strict`.
- **htmx.** A block that htmx swaps into the page later (a sheet, a fragment) is set up when it
  arrives.

## Input blocks

### quantity

A number with − and + buttons, for how many of something (a cart line, a stock count). It
sends one field, `name`, so read it as an integer:

```html
{{ quantity("qty", 1, min=1, max=10, label="Quantity") }}
```

The field is a native number input: Tab reaches it, the arrow keys step it and typing works.
The buttons step it too (Tab skips them, since the arrow keys in the field do the same), grey
out at the limits, and send `input` and `change` on each step, so htmx triggers and live
validation see it. A cart that saves each change:

```html
{{ quantity("quantity", line.quantity, min=1, max=20, hide_label=true,
            label="Quantity of " ~ line.name, id="qty-" ~ line.id,
            attrs={"hx-patch": route('cart.update', line.id), "hx-trigger": "change delay:300ms"}) }}
```

| Option | Default | What it does |
|---|---|---|
| `name` | | the field's name |
| `value` | `1` | the number at first (old input after a failed submit wins) |
| `min`, `max`, `step` | `0`, none, `1` | the limits and the step (`max=none` for no upper limit) |
| `label` | "Quantity" | the field's label |
| `hide_label` | `false` | keep the label for screen readers only (a cart row) |
| `attrs` | `{}` | extra attributes for the input, e.g. htmx ones |
| `hint`, `id`, `bag` | | as for the kit's fields (`id` defaults to `rx-{name}`) |

### range_slider

A range with two handles, such as a price filter. It sends two fields, `name_min` and
`name_max`:

```html
{{ range_slider("price_min", "price_max", 0, 1000000, step=25000,
                value_min=100000, value_max=500000, label="Price", format="money") }}
```

It is two native range inputs on one track, so each handle is a real slider: Tab reaches it,
the arrow keys, Page Up/Down, Home and End move it, and a screen reader says its value in
words. The handles never cross: the one moving stops at the other. Without JavaScript they
are still two sliders that send both fields. Check `min <= max` on the server too (`lte`).

| Option | Default | What it does |
|---|---|---|
| `name_min`, `name_max` | | the two fields' names |
| `min`, `max`, `step` | step `1` | the scale |
| `value_min`, `value_max` | the scale's ends | where the handles start (old input wins) |
| `label` | "Range" | the group's legend |
| `format` | none | `"money"` (the `money` filter, `APP_CURRENCY`), `"number"`, or none (as they are) |
| `prefix`, `suffix` | none | around the values when `format` is none, e.g. `suffix=" km"` |
| `hint`, `id`, `bag` | | as for the kit's fields |

### keypad

A point-of-sale number pad that types into a field of the form, for a touch screen at a
counter:

```html
<rx-input name="paid" label="Amount paid" inputmode="numeric" maxlength="9"/>
{{ keypad("rx-paid", decimal=true, enter_label="Pay") }}
```

The keys are one Tab stop: the arrow keys move between them, Home and End go to the first and
last, Enter or Space presses one. While the pad has the focus, typing digits, the decimal
point, Backspace and Delete (clear) works as on its keys. Each press sends `input` to the
field (its `maxlength` is respected); the Enter key submits the field's form
(`requestSubmit`, so `required` and htmx run as for a click). The decimal key shows the page
language's separator.

| Option | Default | What it does |
|---|---|---|
| `target` | | the id of the input it types into (the kit's `input("paid", …)` is `rx-paid`) |
| `label` | "Number pad" | what screen readers call the pad |
| `decimal` | `false` | add a decimal-point key |
| `zeros` | `true` | add a "00" key |
| `enter_label` | "Enter" | the Enter key's text |

### swatches

Variants (a frame size, a colour) as chips: a group of radio buttons, so one field, `name`:

```html
{{ swatches("size", "Frame size", [
     {"value": "S", "label": "S", "note": "150–165 cm"},
     {"value": "M", "label": "M"},
     {"value": "XL", "label": "XL", "disabled": true}
   ], selected="M", required=true) }}
{{ swatches("colour", "Colour", [
     {"value": "teal", "label": "Teal", "color": "#0b6e66"},
     {"value": "sand", "label": "Sand", "color": "#d8c7a3"}
   ], kind="colour", attrs={"hx-get": route('product.variant', product.id),
     "hx-trigger": "change", "hx-target": "#price", "hx-include": "closest form"}) }}
```

Tab reaches the group and the arrow keys choose, as for any radios. A chosen chip has an
accent border and a tick (a shape, not only a colour); a disabled one is crossed out and
"sold out" for screen readers. With `kind="colour"` each chip shows its colour beside the
label, so the colour is never the only way to tell. Works without JavaScript.

| Option | Default | What it does |
|---|---|---|
| `name`, `label` | | the field's name and the group's legend |
| `options` | | a list of `{value, label, note?, color?, disabled?}` |
| `selected` | none | the value chosen at first (old input wins) |
| `kind` | `"size"` | `"size"` (text chips) or `"colour"` (a colour beside the label) |
| `attrs` | `{}` | attributes for the group, e.g. htmx ones to ask the server for a price |
| `required`, `hint`, `id`, `bag` | | as for the kit's fields |

### datetime_range

A start and an end, each a day and a time, such as a rental from Friday 10:00 to Sunday 18:00.
It sends two fields, `name_start` and `name_end`, each `YYYY-MM-DDTHH:MM`:

```html
{{ datetime_range("starts_at", "ends_at", label="Rental", min="2026-10-06",
                  max="2026-12-31", step=30, opens=9, closes=19, required=true) }}
```

Each end is the kit's `date_picker` (type a day or pick it on the calendar) beside a select of
times. The script writes the two fields that are sent and says how long the range is ("1 day
2 hours"); the end's calendar starts at the start's day, and an end before the start is
pointed out at once. Read them as `NaiveDateTime` (Renox adds the seconds) and check the order
on the server:

```rust
use renox::chrono::NaiveDateTime;
use renox::prelude::*;

#[derive(serde::Deserialize)]
struct RentalForm {
    starts_at: Option<NaiveDateTime>,
    ends_at: Option<NaiveDateTime>,
}

impl Validate for RentalForm {
    fn rules(&self, v: &mut Validator) {
        v.field("starts_at", &self.starts_at).required();
        v.field("ends_at", &self.ends_at)
            .required()
            .gt("starts_at", &self.starts_at);
    }
}
```

This block needs JavaScript (the sent fields are written by it).

| Option | Default | What it does |
|---|---|---|
| `name_start`, `name_end` | | the two fields' names |
| `label` | none | the group's legend |
| `value_start`, `value_end` | none | `YYYY-MM-DDTHH:MM` at first (old input wins) |
| `min`, `max` | none | the first and last day that can be picked (`YYYY-MM-DD`) |
| `step` | `60` | minutes between the times offered |
| `opens`, `closes` | `8`, `20` | the first and last hour offered |
| `required`, `hint`, `id`, `bag` | | as for the kit's fields |

## Showing data

### gallery

Photos as a carousel, with thumbnails under it and an enlarged view in the kit's `sheet`:

```html
{{ gallery([
     {"src": "/img/city-1.jpg", "alt": "The City 3 from the side", "caption": "Teal, size M"},
     {"src": "/img/city-2.jpg", "alt": "Its handlebar", "large": "/img/city-2-big.jpg"}
   ], id="city-photos", label="Photos of the City 3") }}
```

The arrows, the thumbnails, a swipe, or the keyboard (Left/Right, Home/End while the focus is
in the gallery) change the photo, and a screen reader hears which photo of how many it is.
Only the current photo can be reached; "Enlarge" opens it in a sheet (Escape closes it and the
focus comes back). Without JavaScript the photos scroll sideways and the thumbnails are links.

| Option | Default | What it does |
|---|---|---|
| `photos` | | a list of `{src, alt, thumb?, large?, caption?}`: `thumb` a smaller file for the thumbnail, `large` the file shown enlarged |
| `id` | `"rx-gallery"` | the gallery's id (two on a page need their own) |
| `label` | "Photos" | what screen readers call it |
| `enlarge` | `true` | the Enlarge button and its sheet |

### history

A vertical timeline of what happened to something: an order, a repair, a document.

```html
{{ history([
     {"time": "2026-10-06T09:12:00Z", "title": "Checked in", "body": "Brakes **squeal**.", "kind": "info"},
     {"time": "2026-10-06T11:40:00Z", "title": "Ready for pickup", "kind": "success", "by": "Marta"}
   ], label="Service history", date_format="%d %b %H:%M") }}
```

An ordered list, each event with its time first. `kind` sets the marker's colour *and* its
icon, and screen readers hear it in words ("done", "needs attention"). `body` is Markdown
(the `markdown` filter: raw HTML is shown as text). The events fade in one after the other the
first time the list scrolls into view.

| Option | Default | What it does |
|---|---|---|
| `items` | | a list of `{time, title, body?, kind?, by?, when?}`, in the order shown |
| `time` | | an RFC 3339 time or a date, shown with the `date` filter |
| `when` | | replaces the shown time, e.g. "2 hours ago" from the `since` filter |
| `kind` | none | `info`, `success`, `warning` or `error`; none is a plain dot |
| `label` | "History" | the list's name for screen readers |
| `date_format` | `"%Y-%m-%d %H:%M"` | chrono's format for the times |
| `id` | none | the list's id |

### compare_plans

Pricing cards side by side, then a table comparing every feature, with one plan highlighted:

```html
{{ compare_plans([
     {"key": "basic", "name": "Basic", "price": 2900, "interval": "month",
      "description": "For weekend rides.", "perks": ["A check-up a month"], "url": "/plans/basic"},
     {"key": "rider", "name": "Rider", "price": 4900, "interval": "month", "url": "/plans/rider"},
     {"key": "fleet", "name": "Fleet", "price_label": "Ask us", "url": "/contact"}
   ], [
     {"label": "Visits a month", "values": {"basic": "1", "rider": "2", "fleet": "Unlimited"}},
     {"label": "Pickup and delivery", "values": {"basic": false, "rider": true, "fleet": true}}
   ], highlight="rider", label="Service plans") }}
```

The cards sit on a CSS grid (one column on a phone, up to four). The highlighted card has the
accent border and a "Most popular" badge, and its column in the table is tinted. In the table
`true` is a tick and `false` a dash, with "Included" and "Not included" for screen readers.
On a phone the table scrolls sideways inside its frame (the keyboard can scroll it too), with
the feature names staying put.

| Option | Default | What it does |
|---|---|---|
| `plans` | | a list of `{key, name, price, interval?, description?, perks?, url?, cta?, price_label?}`: `price` goes through the `money` filter, `price_label` replaces it, `interval` is `"month"` or `"year"`, `url` is the card's button (`cta` its text) |
| `features` | `[]` | the table's rows: `{label, values: {plan key: true, false or text}}` |
| `highlight` | none | the key of the plan to stand out |
| `highlight_label` | "Most popular" | the badge's text |
| `label` | "Plans" | the section's name for screen readers |
| `table` | `true` | `false` shows only the cards |
| `id` | `"rx-plans"` | the section's id |

## Scheduling and work

### month_calendar

One month as a grid of days with what happens on each, and links to the months around it. On a
phone it becomes a list of the days that have something.

```html
{{ month_calendar("2026-10", [
     {"date": "2026-10-06", "time": "09:30", "title": "Tune-up · City 3", "url": "/visits/4", "kind": "info"},
     {"date": "2026-10-06", "title": "Rental: Trail 5", "kind": "success"}
   ], url=route('visits.index'), today="2026-10-06", label="Service visits") }}
```

The days are an ordered list on a seven-column grid, so a screen reader reads them in order,
each with its full weekday and "Today" when it is. The previous and next months are links
(`?month=YYYY-MM` on `url`), so each month has its own address; pair them with
`hx-boost` and `hx-select` to change only the calendar. The template has no clock: pass
`today` from the handler. The month is worked out in the template, so no Rust helper is
needed.

| Option | Default | What it does |
|---|---|---|
| `month` | | `"YYYY-MM"` |
| `events` | `[]` | a list of `{date, title, time?, url?, kind?}`; `date` is a day or a date-time (its day counts); `kind` as for the kit's badges |
| `url` | none | the page's address; without it there are no month links |
| `param` | `"month"` | the query parameter of the month links (`url` may already have a query) |
| `today` | none | `"YYYY-MM-DD"`, marked in the grid |
| `first_day` | `1` | `1` for Monday, `0` for Sunday |
| `label` | none | the calendar's name for screen readers |
| `heading` | `"h2"` | the month title's heading level |
| `id` | `"rx-month-{month}"` | the section's id |

### availability

A timeline of resources (bikes, rooms, people) against hours or days, with the booked and free
slots; a free slot is a link or a small form that books it:

```html
{{ availability(["09:00", "10:00", "11:00", "12:00"], [
     {"label": "Trail 5", "note": "M · #12", "slots": [
        {"state": "booked", "span": 2, "title": "Ana R."},
        {"state": "free", "url": "/rent?bike=12&at=11"},
        {"state": "free", "action": "/rentals", "fields": {"bike": 12, "at": "12:00"}}]}
   ], label="Bikes free today", corner="Bike") }}
```

It is a table: each resource is a row heading and each time a column heading, so a screen
reader reads "Trail 5, 11:00, Book". Every free slot says what it books ("Book Trail 5 at
11:00"). Booked slots are striped and closed ones dashed, each with its word, so no state
rests on colour alone; a legend says what they mean. On a phone the table scrolls sideways
inside its frame with the resources staying put. A slot with `action` is a form POSTed with
its `fields` and the CSRF field, so booking needs no JavaScript.

| Option | Default | What it does |
|---|---|---|
| `columns` | | the time labels across the top |
| `rows` | | a list of `{label, note?, slots}`; `slots` fill the row from the left |
| a slot | | `{state, span?, title?, url?, action?, fields?}`: `state` is `free`, `booked` or `closed`; `span` how many columns it covers; `title` who has it |
| `label` | "Availability" | the table's caption for screen readers |
| `corner` | "Resource" | the top-left heading |
| `id` | `"rx-availability"` | the frame's id |

### kanban

Columns of cards moved between columns by dragging, or with the keyboard, each move sent to
the server with htmx:

```html
{{ kanban("jobs", [
     {"key": "waiting", "title": "Waiting", "cards": [
        {"id": 7, "title": "Tune-up", "subtitle": "City 3 · Ana", "badge": "Today", "badge_kind": "warning"}]},
     {"key": "working", "title": "In the stand", "cards": []},
     {"key": "ready", "title": "Ready", "cards": []}
   ], url=route('jobs.move'), values={"store": 2}, label="Workshop jobs") }}
```

Each move is an htmx POST to `url` with the fields `card` (the card's id), `column` (the new
column's key) and `position` (0 for the top), plus `values` and the CSRF header htmx gets from
Renox. Answer 2xx to keep the move; any other status puts the card back and says so:

```rust
use renox::prelude::*;

#[derive(serde::Deserialize)]
struct Move {
    card: Option<i64>,
    column: Option<String>,
    position: Option<i64>,
}

impl Validate for Move {
    fn rules(&self, v: &mut Validator) {
        v.field("card", &self.card).required();
        v.field("column", &self.column)
            .required()
            .one_of(&["waiting", "working", "ready"]);
        v.field("position", &self.position).required().min(0);
    }
}

/// Saves the card's column and order (after checking the person may move it).
async fn move_card(Valid(_form): Valid<Move>) -> StatusCode {
    StatusCode::NO_CONTENT
}
```

With a mouse, drag a card anywhere; with a finger or a pen, drag it by its handle (the dots),
so a swipe elsewhere still scrolls the page. With the keyboard, Tab to a card, then Space or
Enter picks it up, the arrow keys move it (Up/Down in its column, Left/Right to the next
column), Space or Enter drops it and Escape puts it back; without a card picked up the arrow
keys move between cards. Every step is announced. A card's own link (`url`) still opens it.
After the server answered, the board sends `rx:kanban-moved` with
`{card, column, position, ok}` in `detail`. Without JavaScript the board is a read-only list.

| Option | Default | What it does |
|---|---|---|
| `id` | | the board's id |
| `columns` | | a list of `{key, title, cards}` |
| a card | | `{id, title, subtitle?, badge?, badge_kind?, url?}` |
| `url` | | where each move is POSTed |
| `label` | "Board" | the board's name for screen readers |
| `values` | `{}` | extra fields sent with every move |

`kanban_card(card)` prints one card, for a fragment that adds a card to a board already on the
page; the board sets it up when htmx swaps it in.

## Texts in other languages

The blocks' texts are English unless your app's lang files have them, under the same keys
(`renox_blocks::TEXTS` lists them all, with the English):

```json
{
  "blocks": {
    "quantity": { "label": "Cantidad", "increase": "Uno más (:label)", "decrease": "Uno menos (:label)" },
    "gallery": { "next": "Foto siguiente", "previous": "Foto anterior", "position": "Foto :n de :total" },
    "calendar": { "today": "Hoy", "month_10": "Octubre" }
  }
}
```

`:name` placeholders are filled as the `t` function fills them. Texts you pass to a macro
(`label=`, `enter_label=`, a plan's `cta`) are yours, so translate them with `t` as usual.

## Changing the look

The blocks use the kit's tokens, so a theme that changes the accent, the surfaces or the radii
changes them too. To change one block's markup, copy the macros into your app as
`resources/views/renox-blocks/blocks.html`: your file replaces the crate's.

## How the files load

The first block on a page prints two tags: the blocks' stylesheet and `blocks.js`, a
JavaScript module. The module looks at the page (and at whatever htmx swaps in later) and
imports a block's code only when the page has that block: a product page with a gallery and a
stepper loads `gallery.js` and `quantity.js`, nothing else. `swatches`, `compare_plans`,
`month_calendar` and `availability` need no code at all.

The files are compiled into the crate, with no library and no build step. The app serves them
itself from `/_renox/blocks/`, under names that carry a hash of their content, with a
year-long cache and no session cookie. A browser runs each module once per page.

## Testing

The input blocks are plain form fields, so test your handlers by posting what the browser
would send:

```rust
use renox::prelude::*;
use renox::testing::TestApp;

#[derive(serde::Deserialize, Validate)]
struct CartLine {
    #[validate(required, between(1, 20))]
    quantity: Option<i64>,
    #[validate(required, one_of(&["S", "M", "L"]))]
    size: Option<String>,
}

struct Cart;

impl Module for Cart {
    fn name(&self) -> &'static str { "cart" }

    fn routes(&self) -> Routes {
        Routes::new().post("/cart", |Valid(line): Valid<CartLine>| async move {
            format!("{} × {}", line.quantity.unwrap_or(0), line.size.unwrap_or_default())
        })
    }
}

#[renox::test]
async fn a_line_is_added() {
    let app = TestApp::new(App::new().module(renox_blocks::Blocks::new()).module(Cart)).await;
    app.post("/cart", &[("quantity", "2"), ("size", "M")])
        .await
        .assert_ok()
        .assert_see("2 × M");
    // The stepper stops at its max in the browser; the server stops it too.
    app.htmx()
        .post("/cart", &[("quantity", "21"), ("size", "M")])
        .await
        .assert_invalid("quantity");
}
```

`TestApp` doesn't run JavaScript, so check the blocks themselves in a browser (see
[testing.md](testing.md)); the crate's own checks are `tests/browser/blocks.test.mjs`.
