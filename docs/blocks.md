# Blocks: interactive components beyond the kit

The UI kit covers forms, tables, sheets and pages. Some screens need more: a stepper for a
quantity, a price filter with two handles, photos in a carousel, a month of bookings, a board
of cards to drag. The `renox-blocks` crate adds twenty-one such components, as template macros
built on the kit:

| Input | Showing data | Scheduling and work | Printing and maps |
|---|---|---|---|
| [`quantity`](#quantity): a stepper | [`gallery`](#gallery): photos in a carousel | [`month_calendar`](#month_calendar): a month of events | [`receipt`](#receipt): a till receipt for a thermal printer |
| [`range_slider`](#range_slider): two handles on one track | [`history`](#history): a timeline | [`availability`](#availability): resources against hours | |
| [`keypad`](#keypad): a point-of-sale number pad | [`compare_plans`](#compare_plans): pricing cards and a table | [`week_calendar`](#week_calendar): resources against a week, click to book | |
| [`swatches`](#swatches): variant chips | [`status_stepper`](#status_stepper): a record's progress through its statuses | [`kanban`](#kanban): cards moved between columns | |
| [`datetime_range`](#datetime_range): a start and an end, with times | | | [`map_picker`](#map_picker): a point, circle or polygon on a map |
| | | | [`map`](#map): markers and areas on a map |
| [`signature_pad`](#signature_pad): a drawn or typed signature, sent as a PNG | | | |
| [`camera_capture`](#camera_capture): photos from the camera, sent as JPEG files | | | |
| [`rating`](#rating): stars, a radio group | | | |
| [`geolocate`](#geolocate): the device's position, on a button press | | | |
| [`barcode_scan`](#barcode_scan): a code typed, scanned or read by the camera | | | |

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
- [Maps](#maps)
- [Printing](#printing)
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
renox = "1.2"
renox-blocks = "1.2"
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

### signature_pad

A signature drawn with a finger, a pen or the mouse, or the name typed instead. It sends a
real file, so the form must be `multipart/form-data`:

```html
<form method="post" enctype="multipart/form-data">
  {{ csrf_field() }}
  <rx-signature-pad name="signature" label="Customer signature" required/>
  <rx-button label="Sign off"/>
</form>
```

Exactly one of two things is sent: `signature`, a PNG named `signature.png` on a white
background (so it reads on any page), or `signature_name`, the typed text (`{name}_name`),
with no file. The last way used wins and clears the other: typing drops a drawing, and
drawing empties the name. Read both as options, and let either satisfy `required`:

```rust
use renox::prelude::*;

#[derive(serde::Deserialize)]
struct SignOff {
    signature: Option<Upload>,
    signature_name: Option<String>,
}

impl Validate for SignOff {
    fn rules(&self, v: &mut Validator) {
        v.field("signature", &self.signature)
            .image()
            .mimes(&["png"])
            .max(1024); // kilobytes
        v.field("signature_name", &self.signature_name).max(120);
        // Either way of signing is enough.
        if self.signature.is_none() {
            v.field("signature_name", &self.signature_name).required();
        }
    }
}
```

The pad is a canvas drawn with pointer events (mouse, pen and touch alike; it is sized for the
screen's pixel density and redrawn when it resizes). **Undo** takes back the last stroke and
**Clear** empties the pad; the file follows each change. The pad is white with dark ink in light and
dark mode alike, and so is the exported picture. With `required`, the typed field is `required` only
while the pad is empty, and the file input itself never is, so the browser does not stop a
submit that has a drawing.

The typed name is a plain text field beside the pad, always there and without JavaScript, so a
signature can be completed with the keyboard alone: Tab goes past the pad to the field. The
canvas is a labelled region (it never traps keys), and the help text that says so is linked
with `aria-describedby`. A change is said in a live region ("Signature drawn.", "Last stroke
removed."). Nothing is animated.

| Option | Default | What it does |
|---|---|---|
| `name` | | the file field's name; the typed one is `{name}_name` |
| `label` | "Signature" | the group's label |
| `name_label` | "Or type your name" | the typed field's label |
| `required` | `false` | a drawing or a name is needed |
| `width`, `height` | 400, 160 | the pad's size in CSS pixels at most (it shrinks to fit a phone) |
| `hint`, `id`, `bag` | | as for the kit's fields (`id` defaults to `rx-{name}`) |

A failed submit shows the old typed name and the error of `name` or `{name}_name` under the
block. A drawing is not put back (a file cannot be): the person draws again.

### camera_capture

Photos taken with the device's camera, sent as files, with a plain file input when there is no
camera. Like the pad it needs a `multipart/form-data` form:

```html
<rx-camera-capture name="photos" label="Photos of the work" :max="3" facing="environment"/>
```

The shutter takes a frame from a live preview, draws it at most `max_width` pixels wide (the
aspect kept) and keeps it as `photo-N.jpg`, a JPEG of the given `quality`. The photos are put
on a file input named `name` (with `multiple` when `max` is above 1), each with a thumbnail and
a **Remove** button; the shutter greys out at `max`. Read them as `Vec<Upload>` (or
`Option<Upload>` for one photo) and count them with `max`:

```rust
use renox::prelude::*;

#[derive(serde::Deserialize)]
struct Photos {
    #[serde(default)]
    photos: Vec<Upload>,
}

impl Validate for Photos {
    fn rules(&self, v: &mut Validator) {
        v.field("photos", &self.photos).max(3);
        v.each("photos", &self.photos, |photo| {
            photo.image().mimes(&["jpg", "jpeg"]).max(5120)
        });
    }
}
```

Storing them (`Upload::store`) and any processing are the handler's; the block does neither.

**When there is no camera.** Without JavaScript the same file input is simply visible, and
on a phone `capture` opens the camera app. With JavaScript, when the browser has no camera API,
the person says no to the permission prompt, there is no device or it fails, the preview and
the shutter give way to that same file input, made visible, with a message that says why
("Camera access was denied. Choose a photo from your device instead."). What it picks is
downsized the same way and shown as thumbnails, so the server sees the same kind of file
either way. The browser only offers the camera on a secure page (HTTPS, or `localhost`). The
camera is switched off when the block is removed (an htmx swap) or the page is left.

| Option | Default | What it does |
|---|---|---|
| `name` | | the file field's name |
| `label` | "Photos" | the group's label |
| `facing` | `"environment"` | `"user"` (the front camera) or `"environment"` (the back one); also the input's `capture` |
| `max` | 1 | how many photos (above 1, the input is `multiple`) |
| `max_width` | 2048 | the widest a photo is kept, in pixels |
| `quality` | 0.85 | the JPEG quality, 0.1 to 1 |
| `accept` | `"image/*"` | the file input's `accept` |
| `required` | `false` | a photo is needed (the visible input is `required` only while it is the way in) |
| `hint`, `id`, `bag` | | as for the kit's fields |

The bike shop uses both on a work order's bench page (a customer sign-off, kept as notes with
private photos) and shows them on `/about/blocks`.

### rating

A score as stars, for how a customer rated a repair or a product. It is a group of radio
buttons under the stars, so it works without JavaScript (and has no code of its own):

```html
<rx-rating name="score" label="Your rating" required/>
```

It sends the chosen number in `name`, and nothing when none was chosen, so the form's struct
reads an `Option<u8>` (or any integer):

```rust
use renox::prelude::*;

#[derive(serde::Deserialize)]
struct Review {
    score: Option<u8>,
}

impl Validate for Review {
    fn rules(&self, v: &mut Validator) {
        v.field("score", &self.score).required().between(1, 5);
    }
}
```

Tab reaches the group once and the arrow keys change the choice, as in any radio group (in a
right-to-left page the arrows swap direction with the text). Each star is named "3 of 5" for a
screen reader. The stars up to the chosen one are solid and the others outlines, so the state
does not rest on colour (under forced colours too), and hovering a star previews it. After a
failed submit the chosen star is checked again, and the error is said under the group.

`readonly` shows a value as text for a screen reader and stars for the eye, and sends nothing:
`<rx-rating name="average" :value="3" :max="4" label="Average" readonly/>`.

| Option | Default | What it does |
|---|---|---|
| `name` | | the field's name |
| `max` | 5 | how many stars, 1 to 10 |
| `value` | | the number chosen to begin with (old input wins) |
| `label` | "Rating" | the group's label |
| `required` | `false` | a star is needed (without it the label says "optional") |
| `readonly` | `false` | display only: stars and text, no input |
| `hint`, `id`, `bag` | | as for the kit's fields |

### geolocate

The device's position, asked for only when the person presses a button:

```html
<rx-geolocate name="location" :accuracy="50" label="Where is the bike?"/>
```

It sends four fields, `location[lat]`, `location[lng]`, `location[accuracy]` (metres) and
`location[at]` (the time of the fix, RFC 3339 in UTC), which `Valid<T>` reads as a nested
struct. The same shape is what `renox::geo::Point` reads for the latitude and longitude, so a
handler can also take the position as a `Point` and ignore the rest:

```rust
use renox::prelude::*;

#[derive(serde::Deserialize)]
struct Position {
    lat: f64,
    lng: f64,
    accuracy: f64,
}

#[derive(serde::Deserialize)]
struct Visit {
    location: Option<Position>,
}

impl Validate for Visit {
    fn rules(&self, v: &mut Validator) {
        v.field("location", &self.location.as_ref().map(|p| p.lat))
            .required()
            .between(-90, 90);
        v.field("location", &self.location.as_ref().map(|p| p.lng))
            .between(-180, 180);
        v.field("location", &self.location.as_ref().map(|p| p.accuracy))
            .min(0);
    }
}
```

The four inputs are disabled until there is a position, so nothing is sent before the press, or
after an error. The permission is asked for in the click and nowhere else, never on load. A
polite status says what is going on: locating, found (with the numbers), denied, not available
and timed out (after 15 seconds; a cached position is never used), and the button then reads
"Try again". `accuracy` is the accuracy you want, in metres: it asks the device for its best
position, and a worse one is still kept, with the status saying so ("about 120 m, wanted 50
m"); the server decides from the posted `accuracy`. With `required`, a submit without a
position is stopped and the status says to press the button, and the server refuses it with
the field's error. After a failed submit the position comes back, enabled. Without JavaScript
the block says it needs it; make the field optional or have a typed alternative where that
matters.

> [!IMPORTANT]
> **Allow it in the Permissions-Policy.** Renox sends a `Permissions-Policy` header that
> switches `geolocation` and `camera` off, and a block cannot change that: the browser then
> refuses the position and the status says it is not available. Allow what your app uses:
>
> ```rust
> use renox::prelude::*;
>
> # let _ =
> App::new().permissions_policy(|p| {
>     p.allow("geolocation", "self").allow("camera", "self");
> })
> # ;
> ```
>
> The browser also offers the position only on a secure page (HTTPS, or `localhost`).

| Option | Default | What it does |
|---|---|---|
| `name` | | the prefix of the four fields |
| `accuracy` | | the accuracy wanted, in metres: turns on high accuracy, and a worse fix is flagged |
| `label` | "Location" | the group's label |
| `button_label` | "Use my location" | the button's text |
| `required` | `false` | a position is needed |
| `hint`, `id`, `bag` | | as for the kit's fields |

### barcode_scan

One text field that a code can fill three ways: typing, a barcode scanner that types like a
keyboard, or the device's camera:

```html
<rx-barcode-scan name="sku" label="Part number" formats="ean_13,code_128"/>
```

The form posts the field as a plain string, so the server never knows which way it was filled
and reads a `String`:

```rust
use renox::prelude::*;

#[derive(serde::Deserialize)]
struct Lookup {
    sku: String,
}

impl Validate for Lookup {
    fn rules(&self, v: &mut Validator) {
        v.field("sku", &self.sku).required().max(64);
    }
}
```

Without JavaScript it is the field alone. With it:

- **A scanner** (a "keyboard wedge") sends its keys fast and ends with Enter. Keys less than 50
  ms apart ending in Enter count as one scan: the value is set, `input` and `change` fire, and
  the Enter is not sent as a submit, so a scan never posts a half-filled form. With `submit`
  the scan submits the form.
- **Typing** is kept as typed. Enter in the field does not post the form either, unless the
  block was given `submit` (then it submits like any form).
- **The camera** opens in the block, with a Close button (Escape closes it too), and fills the
  field with the first code it reads, closes and puts the focus back on the field. The camera
  button appears only where the browser can open a camera. The stream is stopped when the
  camera is closed, the page is hidden or left, or the block is removed. If the camera is
  denied or missing, the status says so and the field still works.

The camera path uses the browser's `BarcodeDetector` where it exists. Where it does not (Firefox,
Safari, desktop Chrome on some systems), the block loads a bundled decoder, [@zxing/library]
0.23.0 (Apache-2.0, about 360 KB, in `assets/vendor/zxing/`), served from your own app like the
rest, and only when the camera is opened: a page without the block, and a browser with
`BarcodeDetector`, never download it. `formats` is a comma list of `BarcodeDetector` format
names (`ean_13`, `ean_8`, `upc_a`, `code_128`, `code_39`, `itf`, `qr_code`, `data_matrix`,
`pdf417`, `aztec`…), which the decoder maps to its own; a name it does not know is ignored, with
a warning in the console. The default, `ean_13,qr_code`, reads shop barcodes and QR codes.

[@zxing/library]: https://github.com/zxing-js/library

The camera needs `camera` in the Permissions-Policy (see the note under [geolocate](#geolocate))
and a secure page.

| Option | Default | What it does |
|---|---|---|
| `name` | | the field's name |
| `value` | | the code to begin with (old input wins) |
| `label` | "Code" | the field's label |
| `formats` | `"ean_13,qr_code"` | the code types to read, as `BarcodeDetector` names |
| `required` | `false` | a code is needed |
| `submit` | `false` | a scanned code (or typed Enter) submits the form |
| `hint`, `id`, `bag` | | as for the kit's fields |

The bike shop shows all three on `/about/blocks` (a rating, a position and a code, posted to a
demo route that says what arrived) and allows `geolocation` and `camera` for its own pages.

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

### status_stepper

Where a record is in its statuses (an order, a repair, an approval), as steps in a row or in a
column. Like every block it is also a tag:

```html
<rx-status-stepper
  :steps="[
    {'key': 'draft', 'label': 'Draft'},
    {'key': 'approved', 'label': 'Approved', 'tone': 'success'},
    {'key': 'shipped', 'label': 'Shipped'},
    {'key': 'cancelled', 'label': 'Cancelled', 'tone': 'error', 'terminal': true}
  ]"
  current="approved" orientation="vertical" label="Order status"/>
```

In a real page `steps` comes from the handler (a `Vec` of a `Serialize` struct with `key`,
`label`, `tone` and `terminal`, in a `#[derive(ViewData)]` context), and `current` is the
record's status. It is an ordered list: the steps before `current` are done (a tick), `current`
is the current one (`aria-current="step"`, bold, filled) and the rest are upcoming. Each step
says so in words for a screen reader ("Draft (done)", "Approved (current step)", "Shipped (not
reached yet)"), so colour is never the only sign.

A `terminal` step is an ending: Cancelled, or Delivered. Its marker is square, a screen reader
hears "final step", and it never counts as done unless it is the current one: put the endings
last, and a record that was cancelled shows every step before Cancelled as done. `tone` (`info`,
`success`, `warning`, `error`) colours a step's marker and the line behind it once it is
reached; an upcoming step stays grey. A `current` that matches no step leaves every step
upcoming. The block only displays: it posts nothing and needs no JavaScript.

| Option | Default | What it does |
|---|---|---|
| `steps` | | a list of `{key, label, tone?, terminal?}`, in order |
| `current` | none | the `key` of the step the record is at |
| `orientation` | `"horizontal"` | `"horizontal"` (a column below 40rem) or `"vertical"` |
| `label` | "Status" | the list's name for screen readers |
| `id` | none | the list's id |

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

### week_calendar

A week of bookings: resources (bays, rooms, people) as rows, days and hours as columns, the
bookings on them, and a click on a free slot to book it:

```html
<rx-week-calendar :resources="bays" :events="bookings" start="2026-10-12"
  slot-url="/bookings" slot-target="#week" id="week" label="Workshop bays"/>
```

`bays` is a list of `{id, label, note?}` and `bookings` one of `{resource, start, end, label,
tone?, href?}`: `resource` is a bay's `id`, `start` and `end` are RFC 3339 times, `tone` is
`info`, `success`, `warning` or `error`, and `href` makes the booking a link (a booking of no
listed resource is left out).

```rust
#[derive(serde::Serialize)]
struct Booking {
    resource: String,
    start: String, // "2026-10-12T09:30:00Z"
    end: String,
    label: String,
    tone: Option<String>,
    href: Option<String>,
}
```

Times are read and shown in the app's time zone (`APP_TIMEZONE`). `start` is the first day
shown, `days` how many (7), `from` and `to` the hours shown (8 and 18), `step` the minutes a
slot lasts (30). A booking is placed on the cells it covers, even by a few minutes; one that
crosses midnight or the edge of the hours is cut to each day (and says "started earlier" or
"continues later"), and bookings that overlap are stacked in lanes, the row growing taller.

With `slot_url`, every free slot is a cell that sends an htmx `POST` to it with three fields,
`resource`, `start` and `end` (RFC 3339, with the zone's offset on that day). They are static
`hx-vals` on the slot (also `data-resource`, `data-start`, `data-end`), so nothing needs
`hx-vals='js:…'`, which `CSP=strict` forbids:

```rust
use renox::prelude::*;

#[derive(serde::Deserialize)]
struct Slot {
    resource: String,
    start: renox::chrono::DateTime<renox::chrono::Utc>,
    end: renox::chrono::DateTime<renox::chrono::Utc>,
}

async fn book(Form(slot): Form<Slot>) -> Result<Redirect> {
    // Check the person may book `slot.resource`, that nothing overlaps, then save it.
    # let _ = slot;
    Ok(Redirect::to("/bookings"))
}
```

`slot_target` (a selector) and `slot_swap` (`outerHTML` by default) say what the answer
replaces; without a target nothing is swapped, so the handler can redirect (`HX-Redirect`) or
send an `HX-Trigger`. Without `slot_url` the free slots are not drawn: the calendar only shows
what is booked.

It is a grid for the keyboard, with one Tab stop. The arrow keys move between a row's slots
and bookings (left and right) or to the same time in the row above or below; Home and End go
to the row's first and last cell, Ctrl+Home and Ctrl+End to the grid's. Enter or Space books
the slot or follows the booking's link. Every cell says its resource, day and time ("Book Bay
1, Monday 12 October, 09:00 to 09:30"). Free slots tint on hover and focus, bookings are
striped with a bar at their start, so no state rests on colour alone. On a phone the grid
scrolls sideways inside its frame with the resources staying put. Dragging a booking to
another time is not part of the block.

| Option | Default | What it does |
|---|---|---|
| `resources` | | a list of `{id, label, note?}`, one row each |
| `events` | `[]` | a list of `{resource, start, end, label, tone?, href?}` |
| `start` | | the first day shown, `2026-10-12` (or a moment) |
| `days` | 7 | how many days |
| `from`, `to` | 8, 18 | the first and last hour shown (`to` up to 24) |
| `step` | 30 | minutes a slot lasts (5 to 240) |
| `slot_url` | none | the URL free slots `POST` to |
| `slot_target`, `slot_swap` | none, `outerHTML` | what the answer replaces |
| `label` | "Week calendar" | the grid's name for screen readers |
| `id` | `"rx-week"` | the frame's id |

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

## Maps

Two blocks put a place on a map: `map_picker` chooses one (a point, a circle or a polygon) and
`map` shows markers and areas. The map library is [Leaflet](https://leafletjs.com) 1.9.4
(BSD-2-Clause), vendored in the crate with its licence; the drawing is the crate's own. A page
fetches `map.js` only when it has a map block, and Leaflet only when that block has tiles
to show: a page without a map block requests neither.

**The tiles are configuration, not a default.** Renox does not choose a tile service for you.
Set a URL template, and the text the map shows for its source:

```text
BLOCKS_MAP_TILES=https://tiles.example.com/{z}/{x}/{y}.png
BLOCKS_MAP_ATTRIBUTION=© Example maps contributors
```

or `Blocks::new().map_tiles("…").map_attribution("…")`. The template must be an `http(s)` URL
(anything else counts as unset); `{s}` is a subdomain if your provider has them. Check your
provider's terms: the public OpenStreetMap servers, for one, are not meant for an app's
traffic. Without tiles each block says so in a note that names `BLOCKS_MAP_TILES`, and the
picker still works from its typed coordinates, so entering a place never depends on a tile
server.

**Content-Security-Policy.** The default policy lets images come from `'self'`, `data:`,
`blob:` and any `https:` host, so an `https` tile host needs nothing. Leaflet's code and
stylesheet are served by your app (`'self'`), with no inline script, so `CSP=strict` works. A
tile host that is not `https` (a local or internal server) must be added:

```rust
use renox::prelude::*;

fn app() -> App {
    App::new()
        .module(renox_blocks::Blocks::new())
        .csp(|csp| {
            csp.allow("img-src", "http://tiles.internal:8080");
        })
}
```

### map_picker

```html
<form method="post" action="/deliveries">
  {{ csrf_field() }}
  <rx-map-picker name="spot" label="Meeting point"/>
  <rx-map-picker name="zone" mode="polygon" label="Delivery zone"/>
  <rx-map-picker name="range" mode="circle" label="Service circle"/>
</form>
```

| Mode | What you do | What is posted |
|---|---|---|
| `point` (the default) | click the map, or drag the pin | two number fields, `spot[lat]` and `spot[lng]` |
| `circle` | click the centre, then click again for the radius | `range`: a GeoJSON `Point` with a `radius` in metres |
| `polygon` | click a vertex per click; double-click or Enter closes it | `zone`: a GeoJSON `Polygon` |

**The keyboard.** Everything the pointer does has a typed way: a point has latitude and
longitude fields; a circle one line, `lat, lng, radius` (metres); a polygon one `lat, lng`
line per vertex. They stay in step with the map in both directions, and give the same value
as drawing does. On the map itself, Tab to it, then Enter closes a polygon, Backspace removes
the last vertex (or the circle or the point), Escape discards the drawing, and the arrow keys
and `+` / `-` move and zoom as in any Leaflet map. A shape that is not complete (a polygon with
fewer than three vertices, a line that does not parse) posts nothing, so a `required` rule
catches it.

GeoJSON order is `[lng, lat]`; Renox's API and the typed fields are `(lat, lng)`. The posted
values read with `renox::geo` ([types.md](types.md#places-points-circles-and-areas)), and
the server checks them again, as for every block:

```rust
use renox::geo::{Point, Polygon};
use renox::prelude::*;

#[derive(serde::Deserialize, Validate)]
struct Delivery {
    spot: Option<Point>, // spot[lat] and spot[lng]: a bad coordinate is a validation error
    #[validate(required, json)]
    zone: String, // the GeoJSON text
}

/// Is the meeting point inside the delivery zone?
async fn check(Valid(form): Valid<Delivery>) -> Result<String> {
    let zone: Polygon = renox::serde_json::from_str(&form.zone)
        .map_err(|_| Error::BadRequest("The zone is not a polygon.".into()))?;
    let inside = form.spot.is_some_and(|spot| zone.contains(spot));
    Ok(format!("inside: {inside}"))
}
```

| Option | Default | What it does |
|---|---|---|
| `name` | | the field name (`name[lat]` and `name[lng]` for a point) |
| `mode` | `"point"` | `"point"`, `"circle"` or `"polygon"` |
| `value` | none | the place to start from: a `Point` (or `{lat, lng}`), `Circle` or `Polygon`, or its GeoJSON |
| `label` | "Location" | the picker's label |
| `height` | 320 | the map's height in CSS pixels |
| `hint`, `id`, `bag` | | as for the kit's fields; the error shows under the block |

### map

```html
<rx-map id="places" label="Our stores"
  :markers="[{'lat': 40.4168, 'lng': -3.7038, 'label': 'Workshop'}]"
  :shapes="[delivery_zone]"/>
```

A read-only map that fits its view to what it shows. A marker is `{lat, lng, label?}` (the
label is its tooltip and its popup); a shape is a `Circle` or `Polygon` value or its GeoJSON
(a circle as a `Point` with `radius`). The markers are also a list for screen readers, and
the only thing shown when there are no tiles. The attribution for the tiles sits in the
corner, as the tile providers require.

| Option | Default | What it does |
|---|---|---|
| `markers` | `[]` | a list of `{lat, lng, label?}` |
| `shapes` | `[]` | a list of circles and polygons |
| `label` | "Map" | the map's name for screen readers |
| `height` | 320 | the map's height in CSS pixels |
| `id` | `"rx-map"` | the block's id |

The texts are `blocks.map.*` keys ([Texts in other languages](#texts-in-other-languages)).
[examples/bikeshop](../examples/bikeshop)'s `/about/blocks` shows both; set `BLOCKS_MAP_TILES`
there to see the tiles.

## Printing

### receipt

A till receipt for a thermal printer, on screen and on a roll of 80 mm or 58 mm paper. It is the
page a counter sale ends on: the lines, the totals, a place for a code, a footer and a "Print"
button.

```html
{% call receipt(lines, totals=totals, title="Bike Shop", header=["Calle Mayor 12", "+34 600 000 000"],
                number="R-1042", date="2026-10-16 10:32", footer="Thank you!") %}
  {{ order.number }}
{% endcall %}
```

Or as a tag, where the tag's content is the code slot:

```html
<rx-receipt width="58mm" title="Bike Shop" :header="store.lines" number="R-1042"
            :lines="lines" :totals="totals" :footer="['Thank you!']">
  <svg>…</svg>
</rx-receipt>
```

The data is plain lists, with amounts in the currency's smallest unit (cents), as the `money`
filter takes them:

- `lines`: `{name, quantity?, price?, total?, note?}`. A line with a quantity other than 1 shows
  "2 × $49.00" under its name; `total` defaults to `quantity * price` (give `total` alone for
  a line with no unit price, such as a repair).
- `totals`: `{label, amount, strong?}`, under the lines; mark the amount due `strong`. A
  discount is a negative `amount`.
- `header` and `footer` are lists of lines of text (a single text works too).

What is between `call` and `endcall` (or inside the tag) is printed under the totals, centred:
the **code slot**. Renox has no QR or barcode generator, so put markup of your own there: an
`<svg>` from a QR library, an `<img>`, a barcode font, or just the receipt number as text, as the
bike shop does.

| Option | Default | What it does |
|---|---|---|
| `lines` | | the items, as above |
| `totals` | `[]` | the rows under them |
| `width` | `"80mm"` | the roll: `"80mm"` or `"58mm"` |
| `title` | | the shop's name, in capitals at the top |
| `header`, `footer` | `[]` | lines of text above and below |
| `number`, `date` | | one line under the header: the receipt's number and its date |
| `print_button` | `true` | the "Print receipt" button (not printed itself) |
| `print_label` | "Print receipt" | its text |
| `label` | the title, else "Receipt" | the receipt's name for screen readers |
| `id` | `"rx-receipt"` | the frame's id (give each receipt on a page its own) |

**Printing.** The paper is always white with black monospace text, whatever the theme, and has
the roll's width on screen too (80 mm is about 302 pixels, 58 mm about 219). When the page is
printed, everything except the receipt is dropped (the navigation, the other cards, the
buttons) and the paper starts at the corner of the page, so a counter page needs no print
stylesheet of its own. The page size is set with CSS named pages:

```css
@page rx-receipt-80 { size: 80mm 297mm; margin: 0; }
@page rx-receipt-58 { size: 58mm 297mm; margin: 0; }
```

CSS has no "auto" height for `size`, so the page is as long as an A4 sheet and a roll printer
cuts after the content; a shop that prints longer receipts, or sets the paper in the printer's
driver, can override these two rules in its own stylesheet. Named pages are supported by
Chrome, Edge and Firefox; where they aren't (Safari), the receipt keeps its width on the page
the browser picks. The button calls `window.print()` from the blocks' script (no inline
handler, so it works under `CSP=strict`); without JavaScript the browser's own Print command
prints the same receipt.

> [!NOTE]
> Printing straight to a printer (ESC/POS commands, opening a cash drawer) is not part of
> Renox. The browser prints, and the printer's driver does the rest.

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
`status_stepper`, `month_calendar` and `availability` need no code at all; `week_calendar` loads
`week.js` for its keyboard, `map_picker` and `map` load `map.js` (and Leaflet, only when they have tiles to show), and `receipt` has a few lines, for its Print button. `signature_pad` loads `signature.js` and
`camera_capture` loads `camera.js`, so a page with neither downloads neither. `rating` needs no code; `geolocate` loads `geolocate.js` and `barcode_scan` loads `barcode.js`, and `barcode.js` fetches the bundled zxing decoder only when the camera is opened in a browser without `BarcodeDetector`.

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
