# Views, components and the UI kit

This part of Renox makes the pages people see in their browser. Your Rust code picks a
template, fills in the blanks, and Renox sends the result as HTML.

Renox pages are **MiniJinja templates** (HTML files with blanks). Renox turns them into HTML
and sends them to the browser. Then **htmx** updates parts of the page in place, without a
full reload.

### Words you'll meet

| Word | What it means |
|---|---|
| **template** (or view) | An HTML file with blanks, like `{{ product.name }}`, that the app fills in. Renox uses MiniJinja for this. |
| **layout** | The frame around every page: the `<head>`, the navigation bar, the footer. Each page fills in the middle. |
| **macro** | A small piece of template you can reuse, like a function: you call it with values and it writes HTML. |
| **component** | A macro kept in a file of its own, such as a price field, and imported where it's used. |
| **the UI kit** | The set of ready-made components that comes with Renox (`renox/ui.html`): fields, buttons, menus, cards, tables and more. |
| **htmx** | A small script (it comes with Renox) that sends a request and swaps part of the page with the answer, with no full reload. |
| **fragment** | A piece of a page (one block of a template) sent alone, for htmx to swap in. |
| **toast** | A short message that pops up at the edge of the screen ("Product saved.") and goes away. |
| **Alpine.js** | Another small script that comes with Renox. It adds small bits of behaviour in HTML attributes (open a menu, show a field). |
| **CSS token** | A named setting for the look, like `--rx-accent` (the accent colour). Change it once and every component follows. |

### In this guide

- [Components](#components-see-the-request): your own reusable pieces, which can see the
  request (old input, errors, translations).
- [Typed view contexts](#typed-view-contexts): a struct per page, checked against its
  template by `view:check`.
- [Components as tags](#components-as-tags): the kit written as `<rx-…>` HTML tags.
  Includes [editor autocomplete](#editor-autocomplete) for VS Code.
- [The UI kit](#the-ui-kit) that ships with Renox (`renox/ui.html`):
  - form fields;
  - the page's frame (navigation bar, sidebar, page headers);
  - themes and type, and [brand themes](#brand-themes) (one colour in, readable accent tokens out);
  - [right-to-left](#right-to-left) pages: `app.dir`, logical CSS, the Arabic font;
  - infolists (read-only details);
  - actions (action groups, wizards, import, duplicate, export) and dashboards.
- [Menus that modules add to](#menus-that-modules-and-the-app-add-to): `App::nav`, `NavLink`,
  `<rx-sidebar menu="…">`.
- [Toasts](#toasts): short pop-up messages.
- [Fragments and out-of-band swaps](#fragments-and-out-of-band-swaps): sending only part of a
  page.
- [htmx response headers](#htmx-response-headers): telling htmx what to do next.
- Data grids (see [docs/grid.md](grid.md)).
- Live components, for a part of a page with state: see [live.md](live.md).
- [Live validation](#live-validation): checking a form while people fill it in.
- [Stacks](#stacks) (`push` / `stack`): adding scripts and styles to the layout from a page.
- [Tailwind CSS](#tailwind-css).

## Components see the request

A **component** is a MiniJinja macro in a file of its own. You import it where you use it.

Inside a component, the same request helpers work as in the page itself:

- `old()`, `has_old()`, `error()`, `errors`: what the person typed last time, and what was wrong
  with it; `errors_in('login')` lists every error of a named error bag
  ([validation.md](validation.md));
- `t()`, `can()`, `auth`, `request`: translations, permission checks, the logged-in user, and
  the request;
- `csrf_field()`, `flash`: the hidden security field every form needs, and one-time messages;
  `csrf_token` is the same token as plain text (for a `<meta>` tag or a script);
- `method_field('PUT')`: the hidden `_method` field, for a form that sends PUT, PATCH or
  DELETE;
- `seo(title=…, description=…, image=…, type=…, canonical=…)`: the page's `<title>`, its
  description, its canonical link and the tags social sites read. `canonical` is this page's
  address on `APP_URL` unless you give one;
- `csp_nonce()`: the page's nonce (a one-time code), for an inline `<script nonce="…">` that
  the Content Security Policy should allow;
- `once()`: "only the first time on this page".

And these work in any template:

- `asset('css/app.css')`: the address of a file in `public/`, with `?v=…` added, which
  changes whenever the file does (so browsers never keep an old copy);
- `storage_url(key)`: the address of a file stored with `Storage`;
- `page_url(3)`: this page's address with `page=3`, keeping the rest of the query (`?q=…`);
  `query_with(period="7d")` does the same for any keys.

Here is a small component, a price field. A file in `resources/views/components/` is a tag, and
`<rx-props>` lists its parameters (more in [Your own components](#your-own-components)):

```html
{# resources/views/components/price_field.html (rnx make:component price_field) #}
<rx-props name label>
<div class="rx-field">
  <label class="rx-label" for="rx-{{ name }}">{{ label }}</label>
  <input class="rx-input" id="rx-{{ name }}" name="{{ name }}" inputmode="numeric" value="{{ old(name) }}"
    {%- if error(name) %} aria-invalid="true"{% endif %}>
  <p class="rx-error" data-error-for="{{ name }}">{{ error(name) }}</p>
</div>
```

What it does:

- It writes a label and an input.
- `old(name)` puts back what the person typed, if the form came back with errors.
- `error(name)` shows the error message for this field, if there is one. It also marks the
  input as invalid (`aria-invalid`), so screen readers know.

`rnx make:component price_field` writes the empty file for you. A page uses it like this:

```html
<app-price-field name="price" label="Price"/>
```

The tag's name is the file's name with dashes, and its attributes are the props.

### Checking templates

`rnx view:check` (or `app.assert_views_compile()` in a test) compiles every template and checks
each literal `route('name')` against the route table, printing `template:line: message` per
problem. It also checks [typed view contexts](#typed-view-contexts). See
[the testing guide](testing.md#templates).

### Typed view contexts

`view("x.html", context! { … })` hands the template whatever the map holds: a field renamed in
the handler, or a typo in the template, only shows as an empty spot on the page. A **typed view
context** is a struct for one page instead. It names its template, its fields are the
template's variables, and `view:check` reports a variable the template uses that the struct
lacks:

```rust
use renox::prelude::*;

/// What `suppliers/index.html` gets.
#[derive(serde::Serialize, ViewData)]
#[view(template = "suppliers/index.html")]
struct SupplierList {
    suppliers: Vec<String>,
    #[serde(rename = "total")] // the template says `total`
    count: usize,
}

async fn index() -> View {
    let suppliers = vec!["Velo Parts".to_owned()];
    // The template comes from the type.
    View::typed(SupplierList { count: suppliers.len(), suppliers })
}

fn app() -> App {
    // Registered, so `view:check` connects the template to the struct.
    // In a module: `app.view_data::<SupplierList>()` in `register`.
    App::new().view_data::<SupplierList>()
}
```

With `{{ suplliers }}` in the template, `rnx view:check` prints
`suppliers/index.html:4: "suplliers" isn't a field of SupplierList`. Besides the fields
(their serialized names: `#[serde(rename)]` counts, `skip` fields don't), a template may use
the request globals (`auth`, `request`, `t()`, `old()`…), every function and global of the
environment (`route()`, `asset()`, your `App::templates` ones), the `App::share` keys, and
the names it sets itself (`{% set %}`, loop variables, imports). `View::typed(page)` returns
a normal `View`: `.fragment(…)`, `.also(…)` and `.status(…)` work as before.

What the check can't see: it reads the page's own template as MiniJinja loads it (after the
`<rx-…>` tags are compiled), so it doesn't follow `{% extends %}`, `{% include %}` or
imported macros. A variable used only in the layout or a partial isn't checked, and a struct
that isn't registered isn't checked at all. `#[serde(flatten)]` and `rename_all` are
compile errors, since the derive couldn't list the names the template sees.

### Only once per page

`{% if once('datepicker') %}<script …>{% endif %}` is true only the first time a key is asked
for on a page. Use it for a component's script or style when the component appears several
times. That way the script is added once, not once per field.

> [!NOTE]
> **Coming from Laravel:** components are like Blade components (`<x-input>`), and `once()`
> is like `@once`. See the table at the [end of this page](#coming-from-laravel).

## The UI kit

`renox/ui.html` is a set of ready-made components. You import them into your templates, the
same way as your own.

The kit follows the principles of Apple's Human Interface Guidelines: hierarchy (the most
important thing stands out), clarity, and accessibility (it works for everyone, including
people using a keyboard or a screen reader).

Its default look is called "warm":

- Inter for text, and Poppins for titles and figures (both come with Renox);
- a paper-white page;
- white surfaces with a hairline (a very thin border);
- an indigo accent colour;
- a type scale that makes the important thing the largest (see
  [Themes and type](#themes-and-type)).

### Turning the kit on

Add the kit's stylesheet and script to the layout, and the toast region to the body:

```html
<head>
  {{ renox_head() }}
  {{ renox_ui() }}
</head>
<body class="rx-page">
  <main class="rx-container">{% block content %}{% endblock %}</main>
  {{ toasts() }}
</body>
```

- `renox_head()` adds what every Renox page needs in `<head>`.
- `renox_ui()` adds the kit's stylesheet and script. The script loads the parts few pages
  have (charts and the period filter, the wizard, the repeater and key/value, tags) only when
  a page, or what htmx swaps in, has one.
- `{{ toasts() }}` is the place where toasts appear.

### A first form

You write the kit's components as `<rx-…>` tags, with nothing to import:

```html
<rx-form route="products.store" live novalidate>
  <rx-card title="New product">
    <rx-form-errors/>
    <rx-input name="name" label="Name" required hint="Shown on the product list."/>
    <rx-select name="size" label="Size" :options="[['s', 'Small'], ['m', 'Medium']]" selected="m"/>
    <rx-checkbox name="featured" label="Feature it on the home page" switch/>
    <div class="rx-card__footer">
      <rx-link-button route="products.index" label="Cancel" variant="plain"/>
      <rx-button label="Create product"/>
    </div>
  </rx-card>
</rx-form>
```

What this page shows:

- a card titled "New product";
- a summary of errors at the top (only after a failed submit);
- a name field with a hint under it, a size choice, and an on/off switch;
- a Cancel link and a "Create product" button at the bottom.

Each field comes with its label, its hint and its error message. You don't write that HTML
yourself. `<rx-form>` writes the CSRF field (and the `_method` field) for you, and `live` turns
on live validation.

Every tag is compiled when the template loads (see [Components as tags](#components-as-tags)).

> [!TIP]
> A tag's content goes inside the component: the card puts what is between `<rx-card>` and
> `</rx-card>` inside itself.
> Many kit components work this way.

### Form fields

Here is every form field, by its macro signature. Each is also a tag, `<rx-input name label …>`
(the macro's name with `rx-` in front and dashes for underscores; the arguments become
attributes). The arguments with `=…` are optional: leave them out and the field uses a
sensible default.

| Component | What it is |
|---|---|
| `<rx-input name label type value hint required autocomplete placeholder attrs id prefix suffix datalist disabled readonly span>` | A text field with its label, hint and error. After a failed submit it is filled in again with what was typed (but never for passwords). `id` tells two fields with the same name apart on one page. `prefix`/`suffix` put a short text before or after the field ("Rp", "kg"). `datalist` suggests values, but any value is accepted. |
| `<rx-textarea name label value rows="4" hint required placeholder attrs>` | The same, for longer text. |
| `<rx-select name label options selected hint required placeholder attrs>` | The same, for a choice from a list. Each option is a value, or a `[value, label]` pair. |
| `<rx-checkbox name label checked hint value="on" switch attrs>` | A checkbox, or (with `switch`) an iOS-style on/off switch for settings. The whole row can be clicked. After a failed submit it shows what was sent, so a box that was left unticked stays unticked. |
| `<rx-radio name label options selected inline columns>` | One choice out of a few, all visible at once. They sit in a `fieldset`, with the label as its title (`legend`). An option is a value, `[value, label]` or `[value, label, description]`. |
| `<rx-checkbox-list name label options selected inline columns>` | Several choices. Each ticked option sends `name` once. So the form's field in Rust is a `Vec`, with `#[serde(default)]` (when nothing is ticked, nothing is sent). |
| `<rx-toggle-buttons name label options selected multiple>` | The options as a row of buttons; the pressed ones are filled and checked. Without `multiple`, one choice (a radio group underneath). With `multiple`, several (checkboxes underneath). |
| `<rx-file name label accept multiple preview current current-name>` | An upload area: drop files on it, or click it to pick them. The chosen files are listed under it; images get a small picture when `preview` is on. `current` is the URL of the file stored now, and `current_name` the name shown for it. The form needs `enctype="multipart/form-data"`. The Rust field is an `Upload` when the file is required, an `Option<Upload>` when it may be left out (an edit form keeping the current file), and a `Vec<Upload>` with `multiple`. |
| `<rx-date-picker name label value min max placeholder="YYYY-MM-DD" readonly disabled-dates closed-weekdays>` | A date: typed as `2026-10-02`, or picked from a calendar. The calendar is Cally, opening in a small box under the field, with the month named in the page's language. The date is sent as `YYYY-MM-DD`, like `<input type="date">`, so the Rust field is a `NaiveDate`. `disabled_dates` and `closed_weekdays` are days that can't be chosen: see [Days that can't be chosen](#days-that-cant-be-chosen). Without JavaScript it is a plain text field. |
| `<rx-show-when field values>` + `<rx-hide-when field values>` | Fields shown (or hidden) while another field has one of `values`. Hidden fields are disabled, so the form doesn't send them. Check them on the server with `required_if`. Without JavaScript they stay visible. |
| `<rx-select multiple searchable>` | Pick several values (a `Vec`), with a box to type in that filters the options. The chosen ones show as chips (small rounded labels). The browser's own select stays underneath: the form sends the same thing, and it works without JavaScript. |
| `<rx-select options-url editable>` | The options come from the server as you type (`renox::select`), for lists too long to put in the page. With `editable`, what was typed can be added ("Add “…”") and is chosen at once, and the chosen option can be renamed (with the pencil). See [Options from the server](#options-from-the-server) below. |
| `<rx-tags-input name label value suggestions>` | Free text as chips. Enter or a comma adds one; Backspace in the empty box removes the last one. The Rust field is a `Vec<String>`. |
| `<rx-repeater name label rows min max item-label add-label reorderable>` | Rows that people add, remove and move. `:reorderable="false"` keeps their order fixed. Each row has the fields in the tag's content, named `name[0][field]`, and they are renumbered when rows move. The Rust field is a `Vec` of a struct, checked with `v.nested`. Adding a row needs JavaScript. |
| `<rx-key-value name label value key-label value-label add-label hint>` | Pairs of text: a repeater with a key and a value on each row. `key_label` and `value_label` name the two columns, `add_label` the add button. The Rust field is `KeyValues`. |
| `<rx-wizard id submit-label back-label next-label>` with `<rx-wizard-step key title>` children` | A form in steps. Next checks the current step first: the browser's own rules, then the server's (with `data-live-validate`). After a failed submit, the first step with an error opens. Without JavaScript all steps show at once. |
| `<rx-form-grid columns="2">` + `<rx-fieldset legend hint columns>` | `rx-form-grid` puts fields side by side from tablet width up (one column on phones). `rx-fieldset` is a group of fields with a title, for long forms. A field's `span="2"` or `span="full"` makes it wider. |

A few options work on many fields:

- Every field takes `id` and `span`.
- Every field but `repeater` and `key_value` takes `disabled` (the field isn't sent).
- `input`, `textarea` and `date_picker` take `readonly`: the field is shaded and can't be
  changed, but it is still sent and can be read.
- `input` takes `revealable`: a button that shows the password. Renox's own sign-in
  pages use it.
- `input` takes `copyable`: a button that copies the value, handy for keys and links.

The error summary links to each field by its name. So it finds a field with its own `id`, and
a radio group, too.

Here are fields side by side, and a group with a title:

```html
<rx-form-grid columns="2">
  <rx-input name="price" label="Price" type="number" prefix="$" required step="0.01"/>
  <rx-input name="total" label="Total" type="money" currency="AED" hint="Steps by the currency's smallest unit."/>
  <rx-input name="weight" label="Weight" type="number" suffix="kg"/>
  <rx-input name="city" label="City" :datalist="['Jakarta', 'Bandung', 'Surabaya']" span="full"/>
</rx-form-grid>
<rx-fieldset legend="Delivery">
  <rx-radio name="speed" label="Speed" required selected="regular"
            :options="[['regular', 'Regular', '2–3 days'], ['express', 'Express', 'Tomorrow']]"/>
  <rx-checkbox-list name="extras" label="Extras" :options="[['gift', 'Gift wrap'], ['note', 'Card']]" inline/>
</rx-fieldset>
```

Price and weight sit next to each other. City takes the whole row (`span="full"`) and
suggests three cities. Below, a "Delivery" group asks for a speed (one choice) and extras
(any number).

These fields as tags (`name` and `label` are required; names with an underscore use a dash, so
`current_name` is `current-name`; lists and other values are given with a colon,
`:options="…"`). `<rx-input>`, `<rx-textarea>`, `<rx-select>` and `<rx-checkbox>` follow the
same rule, so the table lists the ones that take lists and dates:

| Tag | Macro |
|---|---|
| `<rx-radio>` | `radio(name, label, options, selected, hint, required, inline, columns, …)` |
| `<rx-checkbox-list>` | `checkbox_list(name, label, options, selected, hint, required, inline, columns, …)` |
| `<rx-toggle-buttons>` | `toggle_buttons(name, label, options, selected, multiple, …)` |
| `<rx-file>` | `file(name, label, accept, multiple, current, current_name, preview, …)` |
| `<rx-date-picker>` | `date_picker(name, label, value, min, max, placeholder, readonly, disabled_dates, closed_weekdays, …)` |
| `<rx-tags-input>` | `tags_input(name, label, value, suggestions, placeholder, …)` |

Each also takes `hint`, `required`, `id`, `disabled`, `span` and `bag`, and passes any other
attribute to the field.

### Rows of fields

Some forms have a list inside them: the people to invite to a team, the lines of an order.
The bike shop's store hours do this ([staff/stores/edit.html](../examples/bikeshop/resources/views/staff/stores/edit.html),
a repeater row per day, named `hours[0][day]`, `hours[0][opens]`…). In the example below
each row's input is named `invites[0][email]`, `invites[1][email]`, and so on. `Valid` reads
them into a `Vec`.

`<rx-wizard>` takes the steps from its `<rx-wizard-step>` children
(`key` is required, `title` is the name in the progress list) and gives its `id` to each step,
so you do not repeat it. `<rx-repeater>` hands `row` and `prefix` to its content:

```html
<rx-wizard id="new-team" submit-label="Create team">
  <rx-wizard-step key="name" title="Name">
    <rx-input name="name" label="Name" required/>
  </rx-wizard-step>
  <rx-wizard-step key="members" title="Members">
    <rx-repeater name="invites" label="Members" item-label="Member" max="10">
      <rx-input name="{{ prefix }}[email]" label="Email" type="email" :value="row.email" required/>
    </rx-repeater>
  </rx-wizard-step>
</rx-wizard>
```

The wizard has two steps: first the team's name, then its members. In the second step, the
repeater lets people add up to 10 rows, each with an email field. `prefix` is the row's part
of the name (`invites[0]`), so `prefix ~ "[email]"` gives `invites[0][email]`.

On the Rust side, the form is a struct with a `Vec` of rows:

```rust
# use renox::prelude::*;
/// The whole "New team" form: a name and a list of invites.
#[derive(serde::Deserialize)]
struct NewTeam {
    name: String,
    #[serde(default)]
    invites: Vec<Invite>,
}

/// One row of the repeater: one person to invite.
#[derive(serde::Deserialize)]
struct Invite {
    email: String,
}

/// The rules for one row.
impl Validate for Invite {
    fn rules(&self, v: &mut Validator) {
        v.field("email", &self.email).required().email();
    }
}

/// The rules for the whole form.
impl Validate for NewTeam {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required();
        // Each row's rules; errors keyed `invites.0.email`, shown in that row.
        v.nested("invites", &self.invites);
    }
}
```

`#[serde(default)]` means "no rows sent" becomes an empty list instead of an error.
`v.nested` checks every row with `Invite`'s rules. An error in the first row's email is
stored as `invites.0.email`, and the kit shows it in that row.

### Options from the server

Some lists are too long to put in the page: all your customers, all your categories. A
select over such a list asks the server as people type. It uses one URL for everything.

In the template, `options` only needs the option (or options) chosen now:

```html
<rx-select name="category_id" label="Category" :options="[[product.category_id, category_name]] if product.category_id else []"
           :selected="product.category_id" placeholder="None"
           options-url="{{ route('admin.categories.options') }}" editable/>
```

Here is what the kit sends to that URL, and what the handler answers:

| The kit sends | The handler answers |
|---|---|
| `GET ?q=te` (typed, after a short pause) | the matches: `Json(Vec<SelectOption>)` |
| `GET ?values=4` (a value sent back without its label, after a failed submit) | those options (`query.is_lookup()`, `query.values_as::<i64>()`) |
| `POST label=Juice` (`editable`: "Add “Juice”") | the new option, which is chosen at once |
| `POST _method=PUT value=4&label=Juice & Smoothies` (`editable`: renaming the chosen one) | the option as saved |

If adding or renaming fails, a 422 answer from `Valid` (say, "that name is already taken")
shows its first message under the field. Escape gives up a rename.

Who may add or rename is up to the route: put the three handlers inside a group that only
the people allowed can reach (an admin group, a `require_permission`).

Here is the handler that answers the `GET`s:

```rust
# use renox::prelude::*;
use renox::select::{OptionQuery, SelectOption};

/// A category of products.
#[derive(Model, serde::Serialize, Default)]
struct Category { id: i64, name: String }

/// Answers the select's searches and lookups with a list of options.
async fn options(State(db): State<Db>, query: OptionQuery) -> Result<Json<Vec<SelectOption>>> {
    // A lookup asks for options by their values (after a failed submit).
    let rows = if query.is_lookup() {
        Category::query().where_(Category::ID.is_in(query.values_as::<i64>())).get(&db).await?
    } else {
        // A search: the first 20 categories whose name contains what was typed.
        Category::query()
            .where_(Category::NAME.like(&format!("%{}%", query.q)))
            .order_by(Category::NAME)
            .limit(20)
            .get(&db)
            .await?
    };
    // Each option is a value (the id) and the label people see (the name).
    Ok(Json(rows.iter().map(|c| SelectOption::new(c.id, &c.name)).collect()))
}
// Routes: .get(url, options), .post(url, create), .put(url, rename); the POST and PUT take
// `Valid<…>` forms with `label` (and `value`), checked like any form (`unique`, `max`).
```

Without JavaScript, the browser's own select shows only the options in the page.

### A field that depends on another

Sometimes a field matters only when another field has a certain value. The bike shop's
checkout ([sales/checkout/show.html](../examples/bikeshop/resources/views/sales/checkout/show.html)) asks
for an address only for delivery, and requires it only then:

```html
<rx-toggle-buttons name="delivery" label="Delivery" :options="[['courier', 'Courier'], ['pickup', 'Pick up']]"
                   selected="courier" required/>
<rx-show-when field="delivery" values="courier">
  <rx-textarea name="address" label="Address" required/>
  <rx-date-picker name="deliver_on" label="Deliver on" min="2026-10-03"/>
</rx-show-when>
```

The address and the date show only while "Courier" is pressed. When "Pick up" is pressed,
they are hidden and not sent. So the server must also know the rule:

```rust
# use renox::prelude::*;
# struct Checkout { delivery: String, address: String }
/// The checkout form's rules: the address is needed only for the courier.
impl Validate for Checkout {
    fn rules(&self, v: &mut Validator) {
        // A pickup sends no address at all: the hidden group is disabled.
        v.field("address", &self.address).required_if(self.delivery == "courier");
    }
}
```

`required_if(…)` makes the field required only when the condition is true.

### Days that can't be chosen

A booking form shouldn't offer a day the shop is closed or already full. `date_picker` takes
those days, and examples/bikeshop's workshop booking uses them:

```html
<rx-date-picker name="day" label="Visit day" min="2026-10-06" max="2026-11-30" required
                :disabled-dates="['2026-10-12', '2026-10-13']" :closed-weekdays="[0]"/>
```

- `disabled_dates` is a list of days (`"YYYY-MM-DD"`, or `NaiveDate`s from the handler).
- `closed_weekdays` are the weekdays that are never open: 0 is Sunday, 1 Monday … 6 Saturday,
  as chrono's `weekday().num_days_from_sunday()` counts them.

The calendar shows those days faint and struck through, and they can't be picked. The arrow
keys step over them: from a Tuesday before two closed days, → lands on the Friday after. A
closed day typed or pasted into the field is pointed out at once, under the field ("That day
can't be chosen: pick another one", the text `ui.date_unavailable`), and the browser won't
send the form until it's changed.

That is only a help: anyone can send any date. The server refuses them too:

```rust
# use renox::prelude::*;
# use renox::chrono::{Datelike, NaiveDate};
# struct Booking { day: Option<NaiveDate>, full: Vec<NaiveDate> }
/// The booking form: a day that is open and not full.
impl Validate for Booking {
    fn rules(&self, v: &mut Validator) {
        let sunday = self.day.is_some_and(|d| d.weekday().num_days_from_sunday() == 0);
        v.field("day", &self.day)
            .required()
            .none_of(&self.full)
            .rule(!sunday, "We're closed on Sundays.");
    }
}
```

When the full days come from the database, check them in the form's `after` hook instead
(see [docs/validation.md](validation.md#hooks-prepare-authorize-after)).

### Buttons, surfaces and other parts

The rest of the kit's everyday components. A page uses them as tags, for example
`<rx-button label="Save" icon="check"/>`, `<rx-link-button route="products.index" label="Back"/>`,
`<rx-alert kind="warning" message="Low stock"/>` or `<rx-empty title="No products yet"/>`;
the table shows the macros the tags compile to, and the tag's attributes are the macro's
arguments.

| Component | What it is |
|---|---|
| `<rx-button label variant="primary" type="submit" name value size block attrs icon badge key disabled disabled-reason>` | A button. Variants (styles): `primary`, `secondary`, `plain`, `danger` and `plain-danger`. It shows a spinner while its form or htmx request is being sent. For `icon`, `badge`, `key` and `disabled_reason`, see [Actions](#actions). |
| `<rx-link-button href label variant="secondary" size attrs icon badge key new-tab>` | A link that looks like a button. |
| `<rx-icon-button icon label href variant="plain" type="button" size attrs key badge disabled disabled-reason new-tab>` | A button that shows only an icon (or, with `href`, a link). `label` is the name screen readers say, and its tooltip. Variants: `plain`, `primary`, `danger`; also `size="small"`. |
| `card(title=…, subtitle=…)`, `group(title=…, footer=…)` | A card is a surface (a white box) for content. A group is an inset list of rows, like the iPhone's Settings. |
| `<rx-alert message kind title>` + `<rx-badge text kind>` | Kinds: `info`, `success`, `warning`, `error`. An alert has an icon for each kind, so colour is never the only sign. A badge is colour only, so its text must carry the meaning ("Paid", not "●"). |
| `<rx-form-errors title>` | Every error of the last submit, above the form. Each links to its field. |
| `<rx-sheet id title message slide-over width icon>` + `<rx-open-button id label variant="secondary" size icon key badge>` | A sheet is a modal dialog: a box over the page that you must close before going on. It closes on Esc and on a click outside it (on the backdrop), and focus goes back to the button that opened it. On phones it rises from the bottom edge. `slide_over=true` puts it at the side, full height. `width` is `sm`, `md` (default), `lg` or `xl`. `icon` (`info`, `success`, `warning`, `error`) shows above the title. Any element with `data-rx-open="<sheet id>"` opens the sheet too, not only `open_button`. A button with `data-rx-close` inside the sheet closes it. |
| `<rx-action-sheet id label action title>` | A button that opens a sheet with a form sent by htmx. See [Actions](#actions). |
| `<rx-confirm id label action title message confirm-label method="DELETE" size icon modal-icon="warning" key>` | A destructive action (like delete) that asks first, in a sheet. Cancel has the focus first, so pressing Enter by mistake does nothing harmful. `icon` goes on the button, `modal_icon` above the sheet's title (`none` for no icon). |
| `<rx-menu label id variant="secondary" size="small">` + `<rx-menu-link href label icon download new-tab>` + `<rx-menu-action action label method="POST" danger icon>` + `<rx-menu-open id label>` + `<rx-menu-section title>` + `<rx-menu-separator>` | A drop-down menu of actions. The arrow keys move, Esc closes. `menu_action` sends a form (with `_method` for methods other than POST); `menu_open` opens a sheet. |
| `<rx-action-group label id icon variant size>`, `<rx-wizard-action id label route title submit-label …>` with `<rx-wizard-step key title>` children, `<rx-import-action id label route title columns template-url check …>` | Several actions behind one button, an action with steps, a CSV or Excel import. See [Actions](#action-groups). |
| `<rx-tabs id selected label>` with `<rx-tab key label>` children | A segmented control: tabs that switch between panels on one page; each panel is the content of its `<rx-tab>`. The arrow keys, Home and End move between tabs. |
| `<rx-table :rows caption as key card>` with `<rx-column label align hide-narrow>` | A table in a card, one row per item of `rows`. `align="num"` lines a column up on the right (for numbers), and `hide-narrow` hides the column on phones. |
| `<rx-empty title message action-href action-label icon>` | What an empty list says; `icon` draws one of the kit's icons in a tinted circle above it. With `action_href` (and its `action_label`), it adds a link to add the first item. |
| `<rx-notification-bell count id="rx-notifications">` | The logged-in user's notifications, in the navigation bar: a badge with the unread count, a panel, and new ones arriving live as toasts. Needs `Auth::new().notifications()`; pass `unread_notifications`. See [docs/mail.md](mail.md#the-bell). |
| `<rx-event-stream/>` | Opens the same live stream on a page without the bell, so the app's events (`state.broadcast(…)`) arrive as DOM events. Nothing for guests. See [docs/mail.md](mail.md#your-own-live-events). |

### Icons

The kit draws its icons from [Lucide](https://lucide.dev)'s line icons (ISC licence), inline
in the page, in the text's colour:

```html
{% from "renox/ui.html" import icon %}
<h2>{{ icon("bike") }} Workshop</h2>
{{ icon("truck", size=16) }} Free delivery over $50
{{ icon("lock", label="Private") }}
```

As a tag: `<rx-icon name="bike"/>`, `<rx-icon name="truck" size="16"/>`,
`<rx-icon name="lock" label="Private"/>`. A tag can't be written inside an attribute's
expression (`badge={"icon": icon("wrench")}`): there the function call stays, or pass the
icon's name and draw it with `<rx-icon :name="badge.icon"/>` where it is shown.

- `size` is in pixels (20 by default); the stroke is 1.75 at every size.
- An icon is decorative: screen readers skip it (`aria-hidden`), since the text beside it says
  the same. An icon that stands alone and means something gets a `label`, which makes it an
  image with that name (`role="img"` and `aria-label`). A button with only an icon is an
  `icon_button`, whose label does that.
- Buttons, menu items and infolist actions take the same names (`icon="truck"`). The kit's
  older short names still work there: `edit`, `trash`, `close`, `more`, `external`,
  `refresh`, `prev`, `next`, `up`, `down`, `box` and `home`.
- An unknown name draws nothing.

The set has 112 icons, the ones a shop or a back office needs:

- Navigation and actions: `menu`, `x`, `search`, `plus`, `minus`, `pencil`, `trash-2`, `check`, `chevron-left`, `chevron-right`, `chevron-up`, `chevron-down`, `arrow-left`, `arrow-right`, `external-link`, `download`, `upload`, `refresh-cw`, `settings`, `ellipsis`, `filter`, `sliders-horizontal`, `eye`, `eye-off`, `copy`, `log-in`, `log-out`, `house`, `bell`, `heart`, `star`, `printer`, `list`, `layout-dashboard`, `scan-barcode`, `inbox`.
- Shop and money: `shopping-cart`, `shopping-bag`, `tag`, `credit-card`, `receipt`, `package`, `boxes`, `truck`, `store`, `banknote`, `percent`, `gift`, `hand-coins`, `shirt`, `backpack`, `award`, `sparkles`.
- Bikes, service and places: `bike`, `wrench`, `hammer`, `gauge`, `droplets`, `ruler`, `key-round`, `lock`, `battery-charging`, `mountain`, `baby`, `hard-hat`, `lightbulb`, `building-2`, `route`, `map`, `map-pin`, `calendar`, `calendar-days`, `calendar-check`, `clock`, `timer`, `history`, `zap`, `flashlight`, `link`, `cog`, `disc-3`, `circle-dot`, `armchair`, `train-front`.
- People: `user`, `users`, `circle-user`, `shield`, `shield-check`, `id-card`, `handshake`.
- Charts: `chart-column`, `chart-line`, `chart-pie`, `trending-up`, `trending-down`.
- Files and messages: `file`, `file-text`, `folder`, `image`, `paperclip`, `clipboard-list`, `mail`, `phone`, `book-open`, `tablet-smartphone`.
- Status: `info`, `circle-check`, `circle-alert`, `triangle-alert`, `circle-x`, `circle-help`.

Lucide has no bike types; for categories use the closest: `bike`, `mountain`, `building-2`
(city), `train-front` (folding), `zap` or `battery-charging` (e-bikes), `baby` (kids),
`hard-hat` (helmets), `flashlight` (lights), `lock` (locks), `shirt` (clothing), `backpack`
(bags), `circle-dot` (tyres), `link` (chains), `disc-3` (brakes), `cog` (drivetrain),
`armchair` (saddles).

`sidebar_link`, `stat` and `empty` take an `icon` too: beside the link's label, and in a
tinted circle on a stat card's corner or above an empty state's title. Another Lucide icon is a line in `crates/renox-core/src/icons.rs`,
copied from lucide-static's `icons/<name>.svg`.

### Navigation and page structure

The frame of a page comes from the kit too. So an app writes no CSS for its navigation bar,
its sidebar or its page headings.

Both examples are built this way. [examples/bikeshop](../examples/bikeshop) has a public
navigation bar with links, a cart count and menus, and a staff sidebar; its pages use
renox-blocks for what the kit lacks. [examples/hello](../examples/hello) has a navigation bar.

A navigation bar on top looks like this:

```html
<rx-navbar brand="{{ app.name }}" href="{{ route('home') }}" width="wide">
  <rx-nav-links>
    <rx-nav-link route="products.index" label="Products" :active="route_is('products.*')"/>
    <rx-nav-link route="cart.show" label="Cart" :active="route_is('cart.*')" :badge="cart_count"/>
  </rx-nav-links>
  {# rx-spacer pushes what follows to the right end of the bar #}
  <span class="rx-spacer"></span>
  <rx-menu label="{{ auth.user.name }}" id="account-menu" variant="plain">
    <rx-menu-link route="account.show" label="Account"/>
    <rx-menu-action route="logout" label="Log out"/>
  </rx-menu>
</rx-navbar>
<main class="rx-container rx-container--wide" id="main">…</main>
```

The bar shows the app's name (a link home), two links (Products, and Cart with a count), and
on the right a menu with the user's name. `active=route_is('products.*')` highlights
"Products" on every products page.

### Menus that modules and the app add to

A module's links should not have to be copied into the layout. Instead, each module (and the
app) adds its links to a **named menu** at boot, and the layout draws the menu:

```rust
use renox::prelude::*;
use renox::nav::NavLink;

struct Billing;

impl Module for Billing {
    fn name(&self) -> &'static str { "billing" }

    fn register(&self, app: &mut Registry) {
        app.nav(
            "staff",
            NavLink::route("invoices.index", "Invoices")
                .icon("receipt")
                .group("Billing")
                .order(10)
                .permission("invoices.view")
                // Runs only when a page draws the menu, and only for a link this user sees.
                .badge(|_state: AppState, _user: AuthUser| async move {
                    Ok(Some("3".to_owned()))
                }),
        );
    }
}

# fn app() -> App {
App::new()
    .module(Billing)
    // The app adds links too, and can hide a module's link by its key.
    .nav("staff", NavLink::url("/help", "Help").key("help").order(90))
    .nav_hide("staff", "invoices.index")
# }
```

Menu names are free strings. Then the layout draws one:

```html
<rx-sidebar brand="{{ app.name }}" menu="staff"/>
<rx-nav-links menu="portal"/>
```

- `NavLink::route(name, label)` points at a named route; `NavLink::url(url, label)` at a
  path or address. The key defaults to the route name; `.key("…")` sets another one (a URL
  link needs it). Two links with the same key in one menu stop the app at boot.
  `nav_hide(menu, key)` removes a link whichever module added it.
- `<rx-sidebar menu>` draws the links under `<rx-sidebar-section>` headings (the `.group`),
  with their icons and badges; links without a group come first. Groups are ordered by their
  lowest `.order`, links by `.order`, then by the order they were added. Hand-written
  `<rx-sidebar-link>` children are drawn after the menu, so both can be mixed.
- `<rx-nav-links menu>` draws plain `<rx-nav-link>` items: groups and icons are ignored.
- Group and link labels go through `t()`; a label with no translation shows as written.
- The active link follows the route: a route link is active on its own route and on the
  routes under its prefix (`invoices.show` marks `invoices.index`), and `aria-current="page"`
  is set. `.active("invoices.*")` sets another pattern; a URL link is active only on its exact
  path.
- A link can be restricted (see [visibility](authorization.md#showing-links-only-to-who-may-use-them)):
  `.permission`, `.gate` and `.when`. Without a restriction, everyone sees it; an anonymous
  visitor sees only those.
- `.badge` returns `Result<Option<String>>`. It is called only for visible links and only on a
  page that draws the menu. An `Err` is logged and the link is drawn without a badge; the page
  still answers 200.
- In your own markup, `nav("staff")` gives the groups
  (`{% for group in nav('staff') %}` with `group.label`, which is empty for ungrouped links, and
  `group.items`; each item has `key`, 
  `label`, `url`, `icon`, `badge` and `active`). An unknown or empty menu draws nothing.
  `view:check` knows the `menu` prop and `nav()`.

Links that depend on the page (a link with an id in it, a count you already have in the
context) stay hand-written next to the menu.

#### Tabs at the bottom on phones

On a phone, a shop's main sections are easier to reach at the bottom of the screen, under
the thumb. Give `navbar` its `tabs`, and under 36rem (the kit's phone width) the bar on top
keeps one row while the sections become a tab bar floating at the bottom. examples/bikeshop's
public pages work this way:

```html
{%- set tabs = [
  {"href": route('home'), "label": "Home", "icon": "house", "active": route_is('home')},
  {"href": route('catalog.index'), "label": "Shop", "icon": "shopping-bag", "active": route_is('catalog.*')},
  {"href": route('orders.index'), "label": "Orders", "icon": "receipt", "active": route_is('orders.*'), "badge": open_orders},
  {"open": "site-menu", "label": "Menu", "icon": "menu"},
] %}
<rx-navbar :brand="app.name" href="{{ route('home') }}" width="wide" :tabs="tabs">
  <rx-nav-links>…the same sections, for wider screens…</rx-nav-links>
  <span class="rx-spacer"></span>
  <rx-nav-search label="Search">
    <form role="search" method="get" action="{{ route('catalog.search') }}">…</form>
  </rx-nav-search>
  <span class="rx-hide-narrow"><rx-link-button route="login" label="Log in" variant="plain" size="small"/></span>
</rx-navbar>
<rx-sheet id="site-menu" title="Menu" slide-over width="sm">
  <rx-sidebar-link route="login" label="Log in"/>
</rx-sheet>
```

- Each tab is a link (`href`, `label`, an [icon](#icons), `active`), or, with `open`, a button
  that opens the `sheet` of that id: the place for everything the tabs leave out. Four or
  five tabs fit a phone; a long label is cut short. `badge` shows a count.
- `active` marks the current tab with `aria-current="page"`, as `nav_link` does; the current
  tab's icon sits on the accent colour, and its label is bold (not colour alone).
- `nav_search` holds a search form: in the bar on wide screens; on phones behind a search
  button, opening on a row under the bar with its field focused. Escape, or the button again,
  closes it and puts the focus back on the button.
- `nav_links` leave the phone's bar (the tabs replace them), and so does anything in an
  element with `rx-hide-narrow` (a language menu, the account menu: put them in the sheet).
- The tab bar keeps above the home indicator (`env(safe-area-inset-bottom)`; add
  `viewport-fit=cover` to the viewport meta tag to use the whole screen), the page gets room
  at its end so nothing hides behind the bar, and nothing is wider than the screen.
- The tabs are links and buttons in the page's order, after the bar on top: Tab reaches each
  one, with the focus ring on the dark bar. `tabs_label` names the tab bar for screen readers
  ("Sections", the kit text `ui.sections`).
- Without `tabs`, `navbar` is as before.

A back office puts its sections down the side instead, in the kit's app shell, `<rx-shell>`.
A dashboard app writes it `<rx-shell fill>`: that is the standard for dashboard apps
([below](#the-dashboard-standard-rx-shell-fill)).

```html
<body class="rx-page">
  <rx-shell fill>
    <rx-sidebar :brand="company.name">
      <rx-sidebar-link route="home" label="Dashboard" :active="route_is('home')"/>
      <rx-sidebar-link route="invoices.index" label="Invoices" :active="route_is('invoices.*')"/>
      <rx-sidebar-section title="Admin"/>
      <rx-sidebar-link rx-if="can('staff.manage')" route="staff.index" label="Staff" :active="route_is('staff.*')"/>
    </rx-sidebar>
    <div class="rx-shell__main">
      <rx-navbar width="full" :skip="false"><span class="rx-spacer"></span><rx-notification-bell :count="unread_notifications"/></rx-navbar>
      <main class="rx-shell__content" id="main">…</main>
    </div>
  </rx-shell>
</body>
```

The sidebar lists Dashboard and Invoices, then an "Admin" heading. The "Staff" link shows only
to people allowed to manage staff (`can('staff.manage')`). Next to the sidebar, a thin bar on
top holds the notification bell, and the page's content goes under it.

#### The dashboard standard: `<rx-shell fill>`

This is the standard mechanism of a Renox dashboard application: a back office, an admin
panel, any app people use as a daily workspace with a sidebar and many modules. Its layout is
`<rx-shell fill>`. On a desktop screen (at least 64rem, 1024 px, wide and 36rem tall):

- **The document never scrolls.** The shell is exactly as tall as the window. The sidebar, the
  navbar and the page header stay where they are.
- **Every panel scrolls on its own.** The sidebar, the content under the navbar, a data grid's
  rows and each `rx-pane` have their own scroll bar, and scrolling one leaves the others alone.
- **The panels fill the whole screen.** The content has no maximum width, and the element that
  holds the page's work takes the height that is left: a `<rx-grid>` written straight in the
  content does it by itself, any other element takes the class `rx-fill`. No panel is left as an
  empty box: a panel with nothing to show holds its empty state (`<rx-empty>`), which sits in
  its middle.

On tablets and phones the same pages switch by themselves to normal document scroll, as a
plain `<rx-shell>` does: nothing scrolls inside something else on a touch screen. There a
`rx-fill` element (and the grid) keeps a minimum height, `--rx-fill-min` (24rem, or 60% of the
screen's height if that is more), so a grid filtered down to two rows is not a flat box.
Printing is a document too.

Don't give pages fixed heights or write viewport CSS per page: the shell does it. Public pages
(a home page, a shop, a blog) are not dashboards: they use the navbar layout and scroll as a
document.

```html
<rx-page layout="layouts/app.html" title="Orders">
  <rx-page-header title="Orders"/>
  {#- The grid takes the height left; only its rows scroll. -#}
  <rx-grid :page="orders"/>
</rx-page>
```

A page with panels side by side puts them in `rx-panes` and makes that the `rx-fill` element.
Each `rx-pane` is a panel (the kit's surface, with padding) that scrolls on its own:

```html
<rx-page layout="layouts/app.html" title="Order 1042">
  <rx-page-header title="Order 1042"/>
  <div class="rx-panes rx-fill" style="--rx-panes: 2fr 1fr 1fr">
    <section class="rx-pane" aria-label="Items">
      <h2 class="rx-title">Items</h2>
      <rx-list label="Items"><li rx-for="item in order.items">{{ item.name }}</li></rx-list>
    </section>
    <section class="rx-pane" aria-label="History">…</section>
    <section class="rx-pane" aria-label="Notes">
      <rx-empty rx-if="not notes" title="No notes yet" message="Notes on the order show here."/>
    </section>
  </div>
</rx-page>
```

| Class or property | What it does |
|---|---|
| `fill` on `<rx-shell>` (`rx-shell--fill`) | The dashboard standard described above. |
| `rx-fill` | On a desktop screen, takes the height left in the content (put it on an element written straight in `main.rx-shell__content`); when the screen is short it keeps 16rem (`--rx-fill-floor`) and the content scrolls instead. On tablets and phones, at least `--rx-fill-min` tall. A `<rx-grid>` in the content behaves like this without the class. |
| `rx-pane` | A panel that scrolls on its own on a desktop screen and grows with what it holds on smaller ones. A list in it uses the pane as its surface; an empty state alone in it sits in its middle. Use it as the `rx-fill` element, or in `rx-panes`. |
| `rx-panes` | Panes side by side from 64rem up, each as tall as the row; one column below. `--rx-panes` sets the columns' widths (`2fr 1fr`); by default they share the width. |

`rx-fill` and `rx-pane` work in `<rx-shell fill>` only; a page with the navbar layout that must
fill the screen (a data grid as the whole page) uses `rx-page--fill` instead
([More classes and options](#more-classes-and-options)).

With the content as the scroller, the rest works as before: a link to `#anchor` scrolls the
panel that holds it, focus on the first wrong field after a failed form scrolls it into view,
`hx-trigger="revealed"` fires when a pane or the content scrolls the element into view (htmx
listens to the window, so the kit passes the shell's scrolling on to it), and htmx's history
brings back where the content and each pane were. Toasts and sheets sit over the whole window.
A data grid's column headings stick to the top of its rows on a desktop screen; on tablets and
phones, where the page scrolls, they scroll with the page (the grid's sideways-scrolling area is
what they stick to), and `Grid::cards_on_mobile` shows rows as cards with their labels.

| Component | What it is |
|---|---|
| `<rx-navbar brand href="/" logo mark width="narrow" label skip tabs tabs-label>` | The bar on top: see-through, it stays at the top while you scroll ("sticky"), with a hairline under it. `brand` links to `href` (`none` for no brand). `logo` is an image URL; `mark=true` shows the name's first letter in the accent colour. `width` matches the page: `narrow` (`rx-container`), `wide` (`rx-container--wide`) or `full`. It starts with a "Skip to content" link to `#main`, for keyboard users. `tabs`: a tab bar at the bottom on phones ([above](#tabs-at-the-bottom-on-phones)). |
| `<rx-nav-search label id>` | A search form in the bar; on phones behind a search button that opens it on a row under the bar. |
| `<rx-nav-links>` with `<rx-nav-link href label :active badge>` | The bar's sections. `active` (usually `route_is('….*')`) marks the current one, with `aria-current` for screen readers. `badge` shows a count. On phones the links get a row of their own that scrolls sideways (with `tabs`, the tab bar replaces them). |
| `<rx-shell fill>` | The app shell: the sidebar and the main column beside it. `fill` is the standard for dashboard apps ([above](#the-dashboard-standard-rx-shell-fill)). |
| `<rx-sidebar brand href="/" logo mark label skip>` + `<rx-sidebar-link href label active badge icon>` + `<rx-sidebar-section title>` | Sections down the side, for back offices. The page is built like this: `<rx-shell fill>` in `<body class="rx-page">`, the sidebar, then `rx-shell__main` holding a full-width `navbar` and `<main class="rx-shell__content">`. On phones the sidebar becomes a bar of links on top. |
| `<rx-page-header title subtitle back back-label badge badge-kind>` | A page's heading: the title (with a badge), a line under it, a link back (`back`), and the call block's buttons at the end of the row (under the title on phones). |
| `<rx-toolbar>` | Filter fields side by side. They wrap onto more lines on narrow screens, and line up with their buttons. There are no "(optional)" marks, since filters are all optional. Put it inside the `<form>`. |
| `<rx-row-actions>` | A table row's buttons, at the end of the row. On phones only the icons show (the labels stay for screen readers). |
| `<rx-list id label>` | Rows in a surface; each row is an `<li>` you write. Put `rx-list__main` on the part that takes the space left (`rx-list__main--done` strikes it through, for a finished task). Rows can be fragments that htmx adds and swaps. |
| `<rx-columns count="2">` | Columns from tablet width up, one column on phones (a photo next to its details, say). |
| `<rx-card-grid>` + `<rx-media-card href title image image-alt="" subtitle note dimmed>` | Cards with a picture, in a grid that fills the row. Each card is at least 13rem wide; set `--rx-card-min` for another width. |
| `<rx-link-tabs items current label>` | Links that look like a segmented control, for sections or filters that have their own URLs. `items` are `[href, label]` pairs. (`tabs` switches panels on one page instead.) |
| `<rx-thumbnail src alt="" href>` | A small square picture, for example in a table row. |
| `<rx-progress value max="100" label show-value>` | A progress bar (the browser's `<progress>`) in the kit's colours, with the percentage next to it. |
| `<rx-menu-button label icon danger>` | A menu item that is a plain button. Its other attributes say what it does, usually with htmx (`hx-get="{{ url }}"`, `hx-delete`…): they are the macro's `attrs`. |
| `<rx-tab-panel id key selected>` | One panel for a `tabs(…)` written as a macro; with tags, `<rx-tabs>` and `<rx-tab>` make the panels for you. |

### Layout fields as tags

| Tag | What it does |
| --- | --- |
| `<rx-repeater name label :rows add-label item-label min max :reorderable hint id span>` with `row` and `prefix` in its content | Rows that people add, remove and move. `prefix` is the row's name part, like `lines[0]`, so a field is `name="{{ prefix }}[qty]"`. |
| `<rx-key-value name label :value key-label value-label add-label hint id span>` | Pairs of a key and a value. |
| `<rx-fieldset legend hint columns>…</rx-fieldset>` | A group of fields under a legend. |
| `<rx-show-when field values>…</rx-show-when>` | Shows its fields only while another field has one of the values. |
| `<rx-hide-when field values>…</rx-hide-when>` | The opposite. |
| `<rx-wizard id submit-label back-label next-label cancel cancel-label>` with `<rx-wizard-step key title>` children | A form in steps. The steps list and each step's `id` come from the children. |

`values` is one word (`values="courier"`) or a list (`:values="['courier', 'post']"`).
A field that shows only when another has a value:

```html
<rx-radio name="delivery" label="Delivery" :options="[['post', 'Post'], ['courier', 'Courier']]" selected="post"/>
<rx-show-when field="delivery" values="courier">
  <rx-input name="address" label="Courier address"/>
</rx-show-when>
```

`<rx-select>` takes `options-url` (where the server answers searches) and `editable` (people can add
and rename options), like `<rx-select options-url editable>`.

### More classes and options

Some things are classes or options, not macros:

- `rx-page--fill` on `<body>` makes the page as tall as the screen, with `<main>` taking the
  rest. Use it for a data grid that fills the screen (`rx-grid-fill`) in the navbar layout. In
  a dashboard app the shell does this: `<rx-shell fill>`, with `rx-fill`, `rx-pane` and
  `rx-panes` ([the dashboard standard](#the-dashboard-standard-rx-shell-fill)).
- `rx-image` is a picture as wide as its column.
- `input`, `textarea`, `select` and `checkbox` take `hide-label`. The label is hidden on
  screen but stays for screen readers (for example a quantity field in a table row).
- `confirm` takes `cancel_label` ("Keep order") and `fields`: hidden values sent with it, such
  as `{"status": "cancelled"}`.
- Every form field (`input`, `textarea`, `select`, `checkbox`, `radio`, `checkbox_list`,
  `toggle_buttons`, `file`, `date_picker`, `tags_input`) takes `bag="login"`. It shows the
  errors of a named error bag. That's for a page with two forms that share field names
  ([validation.md](validation.md)).

> [!TIP]
> Build pages from these and the components above, instead of writing your own. An app's
> `public/app.css` holds its brand tokens and what is truly its own (a printed invoice, say),
> not a navigation bar or a card.

### Renox's own pages

Renox's own pages use the kit too:

- the sign-in pages (`renox/auth/*`): login, registration, password reset, email
  verification, password confirmation, and the account page;
- the error page.

The sign-in pages' layout has `stack('head')` and `stack('scripts')` (see [Stacks](#stacks)).
Renox's own error page has no stacks.

To change one of those pages, put a file with the same name under
`resources/views/renox/auth/`. Your file is used instead of Renox's.

### The kit's texts

The kit's own texts ("optional", "Cancel", the error summary's title) are in English. An app
can change or translate them in `lang/<locale>.json` (for example `es.json` for Spanish):

- the kit: `ui.optional`, `ui.cancel`, `ui.close`, `ui.dismiss`, `ui.more`, `ui.skip`
  ("Skip to content"), `ui.main_navigation`, `ui.sections` (the tab bar), `ui.errors_title`, `ui.loading`, `ui.back` (the
  wizard and `page_header`'s back link) and `ui.next` (the wizard);
- the fields: `ui.show_password`, `ui.hide_password`, `ui.copy`, `ui.copied`,
  `ui.choose_file`, `ui.choose_files`, `ui.current_file`, `ui.choose_date`, `ui.date_unavailable`,
  `ui.previous_month`, `ui.next_month`, `ui.remove`, `ui.add_row`, `ui.move_up`,
  `ui.move_down`, `ui.key` and `ui.value`;
- the searchable select: `ui.search`, `ui.no_results`, `ui.searching`, `ui.load_failed`,
  `ui.add_option`, `ui.edit`, `ui.editing` and `ui.save_failed`;
- the infolist: `ui.yes`, `ui.no`, `ui.show_more` and `ui.since.*` (`now`, `past`, `future`,
  `minutes`, `hours`, `days`, `months`, `years`);
- dashboards: `ui.chart.show_data`, `ui.chart.other`, `ui.stat.vs_previous` and
  `ui.period.*` (`label`, `7d`, `30d`, `90d`, `12m`, `mtd`, `ytd`);
- the notification bell: `ui.notifications.*`;
- the data grid: `ui.grid.*` ([docs/grid.md](grid.md#translations)).

The full list, with the English texts, is in `crates/renox-core/src/i18n.rs` (`builtin`).

### Changing the kit itself

To change the kit's HTML or its styles, copy the kit into the app:

```sh
rnx make:component --ui   # my-app ui:publish: components/ui.html and public/css/renox-ui.css
```

Then import from `"components/ui.html"` instead of `"renox/ui.html"`. In the layout, load the
copied stylesheet, and only the kit's script, so the styles aren't loaded twice:

```html
<link rel="stylesheet" href="{{ asset('css/renox-ui.css') }}">
{{ renox_ui(styles=false) }}
```

> [!WARNING]
> Once you copy the kit, it's yours: Renox updates no longer change your copy. To only change
> colours, override tokens instead (next paragraph).

Every class starts with `rx-`, and nothing in the kit styles bare elements (a plain `<h1>` or
`<a>` with no class). So it sits happily next to an app's own CSS.

To rebrand the kit, override its tokens on `:root`, for example `--rx-accent: #0a7d5a;` for a
green accent.

### Themes and type

The look is a set of tokens on `:root` (in renox-ui.css):

- the colours;
- the fonts;
- radii (how round the corners are) and shadows;
- the hairline on surfaces;
- the button shape;
- and a type scale (the sizes of text).

The default theme is **"editorial"** (from the bike shop example):
- a cream page and white surfaces;
- a teal accent with a terracotta second accent;
- large Poppins headlines;
- pill buttons and generous corners.

Inside an app shell (`rx-shell`: a sidebar and dense pages) the buttons are rounded rectangles.

Two other themes are a `data-rx-theme` on `<html>` away:
- `"warm"` is the default before 1.1: an indigo accent on warm paper, rounded-rectangle
  buttons.
- `"classic"` is the kit's first look: system fonts, Apple's web blue, cool greys, pill-shaped
  buttons, no hairlines and no small capitals.

```html
<html lang="{{ app.locale }}" data-rx-theme="warm">
```

Besides the colours every theme has, the editorial theme brings tokens for pages that sell or
tell a story. Every theme defines them:

| Token | For |
|---|---|
| `--rx-accent-2`, `--rx-accent-2-text` | a second accent (a badge, a word in a headline) |
| `--rx-tint` | a sand-coloured band or photo background |
| `--rx-accent-soft` | a soft accent background (an icon's circle) |
| `--rx-ink` | a dark band, such as a footer |
| `--rx-type-hero`, `--rx-type-section` | a landing page's headline, a section's heading |

Every text colour keeps 4.5:1 on every background (WCAG AA), in light and dark.

An app's own `:root` tokens, in a stylesheet loaded after `renox_ui()`, win over either
theme. So a brand colour stays when the theme changes. The bike shop keeps its teal and
terracotta this way ([public/theme.css](../examples/bikeshop/public/theme.css)).

### Brand themes

To give an app (or each store, tenant or company) its own colour, derive the kit's accent
tokens from one brand colour instead of writing them by hand. `Theme::from_brand` does the
colour maths and refuses a colour whose text could not be read:

```rust
use renox::prelude::*;
use renox::theme::{Brand, Theme};

fn app() -> App {
    let brand = Brand::new("#1E3A5F").second("#B8532E").display_font("Poppins");
    let theme = Theme::from_brand(&brand).expect("a readable brand");
    App::new().theme(theme)
}
```

Put `<rx-theme/>` (or `{{ renox_theme() }}`) right after `<rx-ui/>` in the layout. The layouts
`rnx new` writes already have it. It links a stylesheet at `/_renox/theme/{hash}.css` and
renders nothing when the request has no theme, so it is safe to leave in.

What the theme sets, for light and dark mode (the same selectors the kit uses):

| Token | Meaning |
|---|---|
| `--rx-accent`, `-hover`, `-pressed` | the brand colour and its states |
| `--rx-accent-text` | the colour for links and accent-coloured text, 4.5:1 on the page and surfaces |
| `--rx-on-accent` | the label on an accent button (white or near-black), 3:1 on the accent |
| `--rx-accent-soft`, `--rx-focus`, `--rx-chart-1` | soft background, focus ring, first chart colour |
| `--rx-accent-2`, `--rx-accent-2-text` | with `.second(..)`; without it the kit's second accent stays |
| `--rx-font-display` | with `.display_font(..)`; only sets the name, load the font yourself |

Status colours, spacing, radii and the type scale are not touched. The stylesheet loads after
the kit's, so the brand wins over `data-rx-theme="warm"` and `"classic"` too.

**Colours that can't be read.** A colour that cannot reach contrast (pale yellow `#FFF59D`) is
refused with a `ContrastError`; its `suggestion` is the nearest colour that works, and a
malformed colour is a different `kind` (`InvalidColor`) with no suggestion. Show the message
next to a colour field:

```rust
use renox::theme::{Brand, ContrastErrorKind, Theme};

let err = Theme::from_brand(&Brand::new("#FFF59D")).unwrap_err();
assert_eq!(err.kind, ContrastErrorKind::LowContrast);
// err.to_string(): "`#FFF59D` cannot reach readable contrast; try #…"
assert!(Theme::from_brand(&Brand::new(err.suggestion.as_str())).is_ok());
```

**A brand per request.** `App::theme_for` runs for each rendered page and may return
`Ok(None)` (use the app's `theme`, or the kit's look). `ThemeRequest` has `host`, `path`,
`locale`, `user` and `state`. The bike shop picks a store's brand by host
([stores.rs](../examples/bikeshop/src/app/home/stores.rs)). To let an administrator choose the
colour, keep it in a [settings](settings.md) group and read it in the chooser:

```rust
use renox::prelude::*;
use renox::settings::SettingsGroup;
use renox::theme::{Brand, Theme, ThemeRequest};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Validate, Default, Clone)]
pub struct Appearance {
    /// `#rrggbb`; empty means the kit's look.
    pub brand_color: String,
}

impl SettingsGroup for Appearance {
    const KEY: &'static str = "appearance";
}

fn app() -> App {
    App::new().settings::<Appearance>().theme_for(|req: ThemeRequest| async move {
        let appearance = req.state.settings::<Appearance>().await?;
        if appearance.brand_color.is_empty() {
            return Ok(None);
        }
        // A bad saved colour falls back to the kit (the error is logged).
        Ok(Some(Theme::from_brand(&Brand::new(appearance.brand_color.as_str()))?))
    })
}
```

Validate the colour when the form is saved (call `Theme::from_brand` in an `after` rule or the
handler and show `err.suggestion`), so the chooser seldom fails. Themes are kept by hash, so
the URL changes when the colour does and a browser never shows an old brand. A theme that has
been served stays available under its hash.

**CSP.** The stylesheet is a same-origin `<link>`, never inline `<style>`, so it works under
`CSP=strict` without a nonce. It is cached for a year (`immutable`); the hash in the URL is the
cache key.

**Tokens and mail.** `theme.tokens()` and `theme.dark_tokens()` return the values as a
`BTreeMap` (for a PDF, a chart library or a #30 template). `Mail::theme(&theme)` colours the
mail components (`rx-mail-button`, `rx-mail-panel`) with the brand. The colours are written
into the mail, so a queued mail needs no theme when it is sent:

```rust
use renox::mail::Mail;
use renox::theme::{Brand, Theme};

let theme = Theme::from_brand(&Brand::new("#1E3A5F")).unwrap();
let accent = theme.tokens()["--rx-accent"].clone();
let html = r#"<a style="background:#1c1917;color:#ffffff;">Pay</a>"#;
let mail = Mail::new("ana@example.com", "Invoice", "Invoice").html(html).theme(&theme);
assert!(mail.html.unwrap().contains(&accent));
```

### The type scale

The type scale has eight roles. Each one is a whole `font` (weight, size, line height,
family) in `rem`. Because it uses `rem`, it follows the reader's own text size setting.

The kit's components use these roles, and an app can too: `font: var(--rx-type-heading)`.

| Token | For | Editorial and warm | Classic |
|---|---|---|---|
| `--rx-type-display` | the figure that matters: a stat, a total | Poppins 600, 32 px | system 600, 28 px |
| `--rx-type-title` | a page's title (`rx-title`, `page_header`) | Poppins 700, 30–40 px (warm: 600, 28 px) | system 700, 22 px |
| `--rx-type-heading` | a card, widget, sheet or grid title | Poppins 600, 17 px | system 600, 19 px |
| `--rx-type-lead` | the line under a page's title | Inter 400, 16 px | system 400, 15 px |
| `--rx-type-body` | text, table cells, fields | Inter 400, 15 px | system 400, 17 px |
| `--rx-type-label` | a field's label | Inter 600, 13 px | system 600, 15 px |
| `--rx-type-note` | hints, descriptions | Inter 400, 13 px | system 400, 13 px |
| `--rx-type-caption` | small capitals over figures and table columns | Inter 600, 11 px, uppercase | system 500, 13 px |

(600 and 700 are font weights: how bold the text is. 400 is normal.)

What the editorial and warm themes make stand out:

- A stat's figure is the largest thing on its card. On a small card it shrinks rather than
  breaking in the middle of the number. Its change is shown in a tinted pill.
- Table and grid headings, and stat and infolist labels, are in small capitals.
- A table's total row and a card's price use the title font.

### Fonts

The fonts are Latin subsets under the SIL Open Font License 1.1, about 72 KB in all. Renox
serves them from `/_renox/fonts/…`, and browsers keep them in their cache for good.
`renox_ui()` asks the browser to load the text font early (it "preloads" it).

Arabic script has a font of its own, Noto Sans Arabic (about 70 KB, also OFL-1.1), which
comes after the Latin ones in `--rx-font` and `--rx-font-display`. Its `@font-face` has a
`unicode-range` for the Arabic blocks, so a page without Arabic text never downloads it.
Other writing systems fall back to the system font. An app that wants other fonts sets
`--rx-font` and `--rx-font-display` on `:root`, and also the `--rx-type-*` tokens, since they
name the font family.

### Right-to-left

Arabic, Hebrew, Persian, Urdu and a few other languages are written from right to left.
A Renox page switches by itself: `app.dir` is `"rtl"` or `"ltr"` for the visitor's locale,
and every layout Renox ships (and every one `rnx new` writes) puts it on the root element.

```html
<html lang="{{ app.locale }}" dir="{{ app.dir }}">
```

Nothing else changes in your templates. The kit's stylesheets use logical properties
(`margin-inline-start`, `inset-inline-end`, `text-align: start`), so the sidebar moves to
the right, a slide-over sheet comes from the left, tables and forms mirror, and chevrons
and arrows (icons named `chevron-*` and `arrow-*`, class `rx-icon--directional`) turn
around. The keyboard follows what is drawn: in a right-to-left page the arrow keys of tabs,
the calendar, the week grid, the gallery and the kanban go the way the page reads, and the
grid freezes its first column on the right. Charts, code, phone numbers, quantities and
the keypad stay left to right on purpose.

The locale decides, the same way as for `app.locale`. A mail is judged by its own locale
(`Mail::locale`, a recipient's), so an Arabic mail queued from an English request is
`dir="rtl"` when the worker renders it. The error and debug pages are English and stay
`dir="ltr"`.

The default list is `ar`, `he`, `fa`, `ur`, `ps`, `yi`, `dv` and `ckb`, matched by language
(`ar-AE` and `ar_AE` are Arabic too). `App::rtl_locales` replaces it, and
`state.direction(locale)` asks with that list; the free function `i18n::direction` always
uses the default one.

```rust
# use renox::prelude::*;
use renox::i18n::{direction, Direction};

assert_eq!(direction("ar-AE"), Direction::Rtl);
assert_eq!(direction("en").as_str(), "ltr");

// An app that serves a language the default list does not know:
# let _ =
App::new().rtl_locales(&["ar", "he", "sd"])
# ;
```

In your own CSS, write what you mean in the reading direction and the page mirrors without
a second stylesheet: `margin-inline-start` instead of `margin-left`, `padding-inline` instead
of `padding-left` and `-right`, `inset-inline-end` instead of `right`, `text-align: start`
instead of `left`, `border-start-start-radius` instead of `border-top-left-radius`. Wrap
something that must keep its direction (a chart, a code sample, a number you format
yourself) in `dir="ltr"`, or give it `unicode-bidi: isolate`. A part of your own that
reads the direction in JavaScript asks when it is used, not once at load:
`getComputedStyle(el).direction === "rtl"`.

Renox ships English texts only, so Arabic translations of the kit's texts are yours to add
(see [The kit's texts](#the-kits-texts)). Arabic-Indic digits and the Hijri calendar are not
part of the kit. The kit's own stylesheets are checked by a lint test
(`crates/renox/tests/it/css_logical.rs`) that fails on a physical `left` or `margin-right`
unless the entry has a reason, and `tests/browser/rtl.test.mjs` checks the pages in a
browser.

### Infolists: read-only details

A record's page (an order, a customer) is mostly labels and values: "Status: Paid",
"Total: $75.00". The kit calls this an **infolist**.

- `infolist` lays the entries out in a grid: one column on phones, `columns` from tablet
  width up.
- `entry` formats each value by its kind: a date, money, a badge, a link…

So a page doesn't need to write that HTML by hand:

```html
<rx-card title="Order #{{ order.id }}">
  <rx-infolist columns="2">
    <rx-entry label="Status" :value="order.status" :badge="{'paid': 'success', 'pending': 'warning'}"
              :labels="{'paid': 'Paid', 'pending': 'Waiting for payment'}"/>
    <rx-entry label="Placed" :value="order.created_at" format="since"/>
    <rx-entry label="Total" :value="order.total" format="money"/>
    <rx-entry label="Invoice" :value="order.invoice" copyable :url="route('invoices.show', order.id)"/>
    <rx-entry label="Tags" :value="order.tags" :badge="true" :limit-list="3"/>
    <rx-entry label="Note" :value="order.note" format="markdown" span="full" placeholder="No note"/>
    <rx-entry label="Customer"><a class="rx-link" href="/customers/{{ order.customer_id }}">{{ order.customer }}</a></rx-entry>
    <rx-repeatable label="Items" :items="order.lines" columns="3">
      <rx-entry label="Product" :value="item.name"/>
      <rx-entry label="Quantity" :value="item.quantity" format="number"/>
      <rx-entry label="Subtotal" :value="item.price * item.quantity" format="money"/>
    </rx-repeatable>
  </rx-infolist>
</rx-card>
```

This card shows an order in two columns:

- the status as a coloured badge, with a friendly name ("Waiting for payment");
- when it was placed, as "3 hours ago";
- the total as money;
- the invoice number, with a copy button and a link;
- up to three tags as badges;
- the note as Markdown, across the whole width, or "No note";
- the customer, as a link written by hand;
- and the order's lines, each in its own small box.

> [!NOTE]
> **Coming from Laravel:** "infolist" is Filament's name for it, and the kit's version works
> much the same way.

| | |
|---|---|
| `<rx-infolist columns="1" inline>` | The `<dl>` (description list) around the entries. `inline` puts each label beside its value (from tablet width up). |
| `<rx-entry label :value format span inline hide-label hint tooltip id>` | A label and its value. With content between the tags, the content is the value. `span="2"` or `"full"` makes it wider. `inline` puts this label beside its value. `hide-label` keeps the label for screen readers only. `hint` adds a line under the value, `tooltip` a title shown on hover. `id` sets the entry's `id`, for a link or a script to find it. |
| `format` | `"date"` and `"datetime"` (with `date_format`, using chrono's codes; in `APP_TIMEZONE`). `"since"`: "3 hours ago", with the date as its tooltip. `"money"`: an amount in the smallest unit (cents), shown in `APP_CURRENCY` like the `money` filter, or `currency="EUR"`, `divide_by=1` for whole units. `"number"` (with `decimals`). `"markdown"`. `"bool"`: a check and "Yes", or a cross and "No". `"color"`: a colour sample and its code. `"image"`: a URL (with `image_size`, `circular`). `"key_value"`: pairs or a map, such as `KeyValues`, as a table. |
| `badge`, `labels` | `badge=true`, a kind (`"success"`), or kinds by value (`{"paid": "success"}`). `labels` gives raw values friendly names (`{"paid": "Paid"}`), with or without a badge. |
| `url`, `new_tab`, `copyable` | A link; a copy button (it copies the raw value). |
| `prefix_actions`, `suffix_actions` | Buttons before or after the value, a list of mappings: `label` (also the tooltip of an icon-only button), `icon` (a kit icon such as `"edit"`, `"external"`, `"refresh"`, `"trash"`, `"download"`; without one the label is shown), and what it does: `url` (a link, with `new_tab`), `action` (a small form posted there, with the CSRF token; `method` `"PUT"`, `"PATCH"` or `"DELETE"` for the others), or neither, with `attrs` for htmx (`{"hx-post": …, "hx-confirm": "Sure?"}`). `variant` (`"plain"` by default) and `disabled_reason` as for buttons. |
| `prefix`, `suffix`, `limit`, `words`, `placeholder` | Text before or after the value. At most `limit` letters, or `words` words, then "…". `placeholder` is what an empty value (none, `""`, an empty list) shows instead (by default `—`). |
| A list as `value` | Each item is formatted the same way. By default they're joined with commas; `list="lines"` puts one per line, `list="bullets"` makes a bullet list. Badges, colour samples and images sit in a row. `limit_list=3` shows three and folds the rest behind "Show 2 more" (a `<details>` element, no script needed). |
| `<rx-repeatable label :items columns="1" placeholder="—" span="full" hide-label>` | A list of records inside the record (an order's lines). Each item is a small infolist with a border, made of the tag's `<rx-entry>` children (the item is `item`). `placeholder` is shown when there are no items. |

An entry with buttons beside its value, such as a link to edit it and a form that acts on it:

```html
<rx-entry label="Email" :value="user.email" copyable :suffix-actions="[
  {'label': 'Write to them', 'icon': 'external', 'url': 'mailto:' ~ user.email},
  {'label': 'Send the verification again', 'icon': 'refresh', 'action': route('users.verify', user.id)}]"/>
```

Code shown coloured, read-only and copyable (`code_entry`), and editors for rich text, Markdown
and code, come from the `renox-editors` crate: see [editors.md](editors.md).

For sections and tabs, use the kit's own `card`, `fieldset` and `tabs`, and put an infolist
in each. The bike shop's order page and the sample page of its `/about/fields`
([about/fields_show.html](../examples/bikeshop/resources/views/about/fields_show.html)) are built this way.

### Formatting values

Every template gets these **filters**. A filter changes a value before it's printed, written
after a `|`: `{{ order.total | money }}`. The infolist uses them too.

| Filter | Gives |
|---|---|
| `number`, `number(2)` | `75,000` in `en`, `75.000` in `es` or `de`: the page's language picks the separators. |
| `quantity`, `quantity(decimals=2, unit="kg")` | An exact decimal such as a `renox::quantity::Quantity` (it serializes to the text `"2.500"`), a decimal string or a number, with the page's separators: `2,500.000` in `en`, `2.500,000` in `es`. The digits are read as text, never through a float. Every digit of the value is kept unless `decimals=n` pads or rounds it (half-up, for display only); `unit="kg"` is appended: `2,500.000 kg`. Text that isn't a plain decimal is an error. |
| `money` | An amount in the currency's smallest unit (cents), as the model stores it and as the data grid's `money` columns take it, shown in `APP_CURRENCY` (default `USD`): `7500` is `$75.00` (en), `$75,00` (es). It divides by the currency's usual decimals (`renox::currency_decimals`: 100 for `USD`, 1 for `IDR` or `JPY`, so `75000` is `Rp 75,000` with `IDR`). Options: `currency="EUR"` for another currency, `decimals=0`, `divide_by=…` to divide by something else (`divide_by=1` for an amount already in whole units). A `renox::money::Money` value is shown in its own currency with that currency's decimals (`{{ invoice.total | money }}` gives `AED 125.00` for `Money::aed(12500)`); `currency=` is ignored for it, `decimals` and `divide_by` still apply. `renox::format_money` formats in Rust, but takes **whole units** (`12.99`), not cents. |
| `date`, `date('%d/%m/%Y %H:%M')` | A date, with chrono's format codes. A moment in time (`created_at`) is shown in `APP_TIMEZONE`. |
| `since` | "3 hours ago", "in 2 days", "just now" (translated with `ui.since.*`). It uses the clock that `TestApp::travel` moves, so tests can check it. |
| `words(20)` | The first 20 words, then "…" (change it with `end="…"`). |
| `markdown` | Markdown (CommonMark, tables, strikethrough, task lists) turned into HTML. HTML inside the text is shown as text. A link or image to anything but `http(s)`, `mailto`, `tel` or a relative URL points nowhere. So it is safe for text that people typed. |

### Design principles

The kit follows the principles of the Human Interface Guidelines. These are the rules its
components follow by themselves. They're worth keeping in an app's own pages too:

- **Hierarchy** (the important thing stands out).
  - One primary button per form or page: the action people came to take.
  - Destructive actions in lists are red text (`plain-danger`). The filled red button waits
    in the confirmation sheet.
  - Secondary text is lighter and smaller, but never lighter than WCAG AA allows. (WCAG is
    the web's accessibility standard; AA is the level most sites aim for.)
- **Clarity.**
  - Labels are always visible. Placeholders are examples, not labels.
  - A hint says what's expected before anyone gets it wrong.
  - Errors are plain sentences, next to the field and in a summary above the form.
  - Optional fields are marked, since required ones are the norm.
- **Feedback** (people see that something happened).
  - A button shows it's working (a spinner, `aria-busy`) and can't be pressed twice.
  - A toast confirms what happened and names the thing ("“Espresso” moved to the trash").
  - Success and info toasts go away by themselves, but wait while the mouse is over them or
    they have focus. Error toasts stay until dismissed. Escape closes the toast that has the
    focus, else the newest one, once nothing else that Escape closes is open.
- **Forgiveness** (mistakes are easy to avoid and undo).
  - Destructive actions ask first, with Cancel focused.
  - Live validation checks a field when you leave it, then again as you correct it. It never
    complains about a field someone is still typing in.
- **Deference** (the design steps back so the content comes first).
  - Content stays plain.
  - See-through ("translucent") surfaces are only for what floats over the content: the
    navigation bar, menus, toasts, and the dark layer behind a sheet.
- **Accessibility.**
  - Text contrast is at least 4.5:1, in both light and dark mode. The default accent, indigo
    #4F46E5, keeps 6.3:1 under white text. (The classic theme's is #0071E3, since Apple's
    system blue falls short.)
  - Every control has a 44 × 44 pt target (big enough for a finger) and a visible focus ring
    for keyboard users.
  - Hints and errors are tied to their fields with `aria-describedby`, and errors are
    announced by screen readers.
  - Menus, tabs and sheets work with the keyboard.
- **Respect for settings** (the person's device settings win).
  - Dark mode follows the system. `data-theme="light|dark"` on `<html>` overrides it.
  - Reduce Motion stops the animations, Reduce Transparency makes see-through surfaces solid,
    and Increase Contrast darkens separators and secondary text.
  - Text sizes are in `rem`, so they follow the reader's settings.
  - Sheets and toasts stay clear of the phone's notch and home indicator (the "safe areas").

### Error pages

When a page isn't found (404), isn't allowed (403) or something breaks (500), the app shows
an error page.

`rnx new` writes `resources/views/errors/default.html` for this. It's your layout with the
kit's `empty` component and a link home, so a 404 or a 403 keeps the navigation bar.

- `errors/<status>.html` (for example `errors/404.html`) replaces it for one status.
- The page gets `status`, `reason` and `detail`, besides the usual globals. With `APP_DEBUG`
  on, it also gets `request_line` and `template`.
- Renox's own error page (used when an app has none) is built on the kit too.

## Components as tags

The UI kit's parts are written as HTML tags: `<rx-card>`, `<rx-input>`, `<rx-table>`. A small
compiler in Renox rewrites the tags into plain MiniJinja when a template loads, so there is
nothing to install and nothing to run at build time. A few parts have no tag (they are named
where they appear) and are called as macros.

A template with no `<rx-…>` or `<app-…>` tag, and no `rx-if`, `rx-else` or `rx-for` attribute, is
not touched at all.

```html
<rx-page layout="layouts/app.html" title="Products">
  <rx-stack>
    <rx-page-header title="Products" subtitle="All items">
      <rx-link-button route="products.create" label="New product" variant="primary" icon="plus"/>
    </rx-page-header>
    <rx-table :rows="products" caption="Products">
      <rx-column label="Name"><a href="{{ route('products.show', row.id) }}">{{ row.name }}</a></rx-column>
      <rx-column label="Price" align="num">{{ row.price | money }}</rx-column>
      <rx-row-actions>
        <rx-icon-button can="update" route="products.edit" icon="pencil" label="Edit"/>
      </rx-row-actions>
      <rx-slot name="empty"><rx-empty title="No products yet"/></rx-slot>
    </rx-table>
  </rx-stack>
</rx-page>
```

### Spacing: put siblings in `rx-stack` or `rx-row`

The kit's components have no outer margin. The space between two of them comes from the element
they sit in, so the same card looks right in a list and in a sheet. Whitespace between the tags
doesn't add any: Renox keeps it, but HTML gives none between blocks.

Two tags exist only to space their children:

- `<rx-stack>`: one below the other, with the kit's section spacing (`--rx-space-5`).
- `<rx-row>`: side by side, wrapping on narrow screens (`--rx-space-3`); `end` pushes them to
  the right.

Some kit containers space their children too: `rx-card`, `rx-sheet`, `rx-form-grid`,
`rx-fieldset`, `rx-dashboard`, `rx-card-grid`, and the starter's sidebar layout
(`main.rx-shell__content`). Others don't, and their children touch:

- the `<main class="rx-container">` of `rnx new`'s layout, so a page starts with `<rx-stack>`;
- `<rx-form>`, a plain `<form>`: put its fields in an `rx-card`, an `rx-form-grid` or an
  `rx-stack`;
- your own `<div>`s and `<app-…>` components.

```html
<!-- Wrong: in rx-container the alert, the card and the table touch. -->
<rx-page layout="layouts/app.html" title="Orders">
  <rx-page-header title="Orders"/>
  <rx-alert kind="info" message="Orders placed before noon ship today."/>
  <rx-table :rows="orders" caption="Orders">…</rx-table>
</rx-page>

<!-- Right: rx-stack spaces them, rx-row spaces the buttons. -->
<rx-page layout="layouts/app.html" title="Orders">
  <rx-stack>
    <rx-page-header title="Orders"/>
    <rx-alert kind="info" message="Orders placed before noon ship today."/>
    <rx-table :rows="orders" caption="Orders">…</rx-table>
    <rx-row end>
      <rx-link-button route="orders.export" label="Export" variant="secondary"/>
      <rx-link-button route="orders.create" label="New order" variant="primary"/>
    </rx-row>
  </rx-stack>
</rx-page>
```

`class` isn't allowed on kit tags, so don't reach for a margin class (`mt-4`): wrap the siblings
instead. `rx-stack` and `rx-row` do take `class` and any attribute, and nest as deep as needed.

### Attribute values

| You write | It means |
|---|---|
| `title="Products"` | The text `Products`. |
| `title="{{ t('products.title') }}"` | The value of the expression, as it is (not escaped twice). |
| `title="Page {{ n }} of {{ last }}"` | Text mixed with expressions: the pieces are joined. |
| `:rows="products"` | A MiniJinja expression, with no `{{ }}`. Use it for lists, numbers, objects and tests. |
| `required` (no value) | `true`. |

More rules:

- `kebab-case` names become `snake_case` parameters: `hide-label` is `hide_label`.
- A yes/no prop takes a bare attribute, `"true"`, `"false"` or a `:` expression. A number prop
  takes a number or a `:` expression. A prop that takes a list or an object takes a `:` expression
  only. A prop with a fixed set of words (`variant="primary"`) checks the word.
- `{% %}` inside an attribute is an error. Use `{{ }}` or `:`, or put the condition in
  `rx-if`.
- **`?name="expression"` is an attribute that may be absent.** The value is left out when the
  expression is `false`, `none` or undefined; `true` writes the bare attribute; any other value
  (text, a number) is written as `name="value"`, escaped. It works on a plain HTML element, on
  `rx-stack`, `rx-row`, `rx-form`, `rx-script` and `rx-style`, and on the kit's components that
  take extra attributes (which then go to the element only when set):
  `<a href="…" ?aria-current="'page' if current">` is `aria-current="page"` when `current` is
  true and nothing otherwise, and `<li ?data-done="step.done">` is `data-done` or nothing.
  `:name="…"` stays the old rule (always written, as `name="value"`), and on a plain element a
  `:name` is Alpine's `x-bind`, so it is never touched. A `?` in front of the name is the only
  difference: `?hx-swap-oob="'true' if oob"` replaces `{{ ' hx-swap-oob="true"' | safe if oob }}`.
- Components made from a macro accept extra attributes whose name has a `-` or starts with `@`
  (`hx-post`, `data-id`, `@click`), plus a few plain ones (`min`, `max`, `step`, `pattern`,
  `minlength`, `maxlength`, `inputmode`, `autofocus`, `tabindex`, `title`, `form`, `accept`).
  They go to the element as HTML attributes. `class` is not allowed on them. `rx-stack`, `rx-row`
  and `rx-form` are plain elements: every extra attribute is passed on, and `class` is merged.

### Content and slots

What you write between the tags is the component's content. Where a component has more than one
place for content, name it with `<rx-slot name="…">`. `rx-table` has one for what shows when there
are no rows:

```html
<rx-card title="Orders">
  <rx-table :rows="orders">
    <rx-column label="Number">{{ row.number }}</rx-column>
    <rx-slot name="empty">No orders yet.</rx-slot>
  </rx-table>
</rx-card>
```

A button's content is its label: `<rx-button>Save</rx-button>` is `label="Save"`. A component
whose content is a list of rows (such as `rx-repeater`) gives the content its row variables;
they are named in the component's entry in the reference.

### Control flow

Four attributes work on any element, a component or plain HTML:

```html
<li rx-for="p in products">{{ p.name }}</li>

<rx-alert rx-if="product.stock == 0" kind="warning">Out of stock.</rx-alert>

<rx-badge rx-if="p.stock == 0" kind="error">Sold out</rx-badge>
<rx-badge rx-elif="p.stock < 5" kind="warning">Low</rx-badge>
<rx-badge rx-else>In stock</rx-badge>
```

- `rx-elif` and `rx-else` go on the very next sibling element (only spaces or comments between).
  A chain is one `rx-if`, any number of `rx-elif`, and at most one `rx-else`.
- With `rx-for` and `rx-if` together, the loop is outside: the test runs for each item.
- `rx-else` or `rx-elif` without an `rx-if` before it, or after an `rx-for` element, is an
  error with the file and line.
- `loop.index`, `loop.index0`, `loop.first`, `loop.last` and `loop.length` work inside `rx-for`.

To show several siblings, or plain text, under one condition or in one loop, use `<template>`.
It writes its children and no element of its own:

```html
<template rx-if="order.paid">
  <rx-badge text="Paid"/> on {{ order.paid_at | date }}
</template>
<template rx-elif="order.cancelled">Cancelled</template>
<template rx-else>Waiting for payment</template>

<template rx-for="tag in post.tags">{{ tag }}{% if not loop.last %}, {% endif %}</template>
```

A `<template>` takes no other attribute, and one with no `rx-` attribute (Alpine's `x-if`,
`x-for`) is left as it is.
- `can="update"` on a component shows it only if the user may do that. `can="update"` inside an
  `rx-table` asks about the row: `can('update', row)`.
- `route="products.edit"` fills the component's link prop with `route("products.edit")`. Inside an
  `rx-table` it adds the row's key: `route("products.edit", row.id)`. It works on components
  that have a link (`rx-link-button`, `rx-icon-button`, `rx-empty`'s action, `rx-form`'s `action`).
  To pass other arguments, write `href="{{ route(…) }}"`.

### Pages, forms and tables

- **`rx-page`** must be the root element. It is `{% extends %}` plus the blocks: `layout`,
  `title` and `description` (the page's SEO tags). Each `<rx-slot name="X">` inside it becomes the
  layout's `{% block X %}`. Give `title` or a `seo` slot, not both.
- **`rx-push`** adds its content to a stack (`stack="head"`, with `once="key"` to add it once).
- **`rx-let`** gives a name to a value for the rest of the element, slot or block it is in:
  `<rx-let name="label" :value="order.number ~ ' / ' ~ order.customer"/>` is
  `{% with label = (…) %}` around everything after it, so the name ends where its element ends
  (a `{% set %}` inside an `{% if %}` would stay set after it). `value="Text {{ x }}"` takes text,
  `:value="…"` an expression. The name must be free: a loop variable, an `rx-table` row, another
  `rx-let` or one of the names every page has (`loop`, `auth`, `request`, `route`…) is an error
  from the compiler and `view:check`. It is for local aliases, not for counters that change in a
  loop. It takes no `rx-if`, `rx-for` or `can`, and a name set in it is not seen by an
  `<rx-block>` it holds.
- **`rx-script`** and **`rx-style`** write `<script>` and `<style>` with the page's CSP nonce
  (`nonce="{{ csp_nonce() }}"`), so they run under `CSP=strict`. Other attributes pass through
  (`type="module"`, `defer`, `:src="asset('app.js')"`); `nonce` is an error, since the tag
  sets it. The code between the tags is left exactly as written, so `<`, `>` and `&&` are safe,
  and `{{ value | tojson }}` still works. They sit inside `rx-push` too:
  `<rx-push stack="scripts" once="chart"><rx-script>…</rx-script></rx-push>`.
- **`rx-block`** is a named block: `<rx-block name="bookmarks">…</rx-block>` becomes
  `{% block bookmarks %}…{% endblock %}`, and self-closing (`<rx-block name="content"/>`) it
  leaves an empty one for a page to fill. `name` is plain text: letters, digits and underscores.
  `.fragment("bookmarks")` in Rust keeps working, because it is a Jinja block.
  In a layout, write the same hole as `<rx-outlet name="content"/>`; its children are the
  block's default content (`<rx-outlet name="seo">{{ seo() }}</rx-outlet>`). `rx-outlet` takes
  exactly one of `name` or `stack`, and it is an error to give both or neither. (Deviation from
  #516: `<rx-block>` is also allowed inside `<rx-page>`, where the examples use it.)
- **`rx-mail`** is a mail's layout (see [Mails as tags](#mails-as-tags)).
- **`rx-pagination`** writes the page links: `<rx-pagination :page="bookmarks"/>` is
  `pagination(bookmarks)` of `renox/pagination.html`, or `simple_pagination` for a `SimplePage`
  (a value with `has_next` and no `last_page`), the same choice `rx-table` makes.
- **The layout's plumbing** has tags too, with no content: `<rx-head/>` is `{{ renox_head() }}`,
  `<rx-ui/>` is `{{ renox_ui() }}` (`<rx-ui :styles="false"/>` leaves out the stylesheet),
  `<rx-toasts/>` is `{{ toasts() }}`, and `<rx-outlet stack="scripts"/>` is
  `{{ stack('scripts') }}` (and `<rx-outlet name="content"/>` is a layout block), where `rx-push` content ends up. (The tag is not `rx-stack`: that is
  the vertical layout.)
- **`rx-form`** writes the `<form>`, the CSRF field, and `_method` for `PUT`, `PATCH` and `DELETE`.
  A `GET` form gets neither. `live` turns on live validation. Its fields are `rx-input`,
  `rx-textarea`, `rx-select`, `rx-checkbox`, `rx-radio` and the others in the table below;
  `rx-form-errors` shows the form's error summary, and `rx-form-grid` lays fields in
  columns.
- **`rx-table`** loops the rows for you. `rows` takes a list or a page (`Paginated`,
  `SimplePage`): for a page, the links are added under the table and they swap only the table
  (htmx), so the rows' own links are not boosted. `as` names the row (`row` by default), `key`
  is what `route` and `can` use (`row.id` by default), and `card` puts the table in a card. The
  children are `rx-column` (`label`, `align="start|num"`, `hide-narrow`), `rx-row-actions`,
  `<rx-slot name="empty">` and `<rx-slot name="footer">`: rows written by hand (`<tr>`) that
  are drawn in a `<tfoot>` after the body, such as a total row.
- **`rx-simple-table`** is the kit's table around rows you write yourself, for a table that is
  not one row per record (a total, cells looped inside a row): `:head` is the heading cells (a
  text, or `[text, "num"]` to align figures to the end), `caption` is for screen readers, the
  content is the body rows (`<tr rx-for="line in lines">…</tr>`) and `<rx-slot name="footer">`
  holds the `<tfoot>` rows. It is the `table` macro with its `foot` parameter.
- **`rx-include`** draws another template with the current variables, what `{% include %}` does.
  `template="parts/card.html"` is checked by `view:check` (a template that is not there is a
  problem); `:template="section.template"` takes a name worked out at run time, and `optional`
  is `ignore missing`, so a name with no file draws nothing:
  `<rx-include :template="'admin/' ~ slug ~ '/cells.html'" optional/>`.
- **`rx-tabs`** takes its tabs from `<rx-tab>` children, or from data with `:items="tabs"`, a
  list of `[key, label]` pairs: the tab list is drawn from it and the children are the panels,
  as `<rx-tab-panel>` tags that can sit under `rx-for`. Give `:items` or `<rx-tab>`s, not both.
- **`rx-link-tabs`** takes `:items` (a list of `[href, label]` pairs and `current`) or
  `<rx-link-tab :href :current>label</rx-link-tab>` children, which can sit under `rx-for` and
  `rx-if`: `<rx-link-tab rx-for="v in views" :href="route('stock.index', view=v)" :current="v == view">{{ t(v) }}</rx-link-tab>`.
  With children, `current` goes on each link.

### Your own components

A file in `resources/views/components/` is a tag. `components/price_tag.html` is
`<app-price-tag>`. Make one with `rnx make:component price_tag`. The file starts with
`<rx-props>`, which lists its parameters:

```html
<rx-props amount currency="USD" :muted="false">
<span class="price">{{ amount | money }} {{ currency }}</span>
<rx-slot />
```

A bare name is required, `name="text"` has a text default and `:name="expression"` has an
expression default. `<rx-slot />` is where the tag's content goes; `<rx-slot name="x" />`
is where the slot `x` goes. Using it: `<app-price-tag :amount="p.price" currency="EUR"/>`.
The compiler checks the attributes against `<rx-props>`, like it does for the kit. 

### When something is wrong

Mistakes stop at load time, with the file and the line, and a suggestion when a name is close:

```text
resources/views/products/index.html:3: unknown component <rx-stak>; did you mean <rx-stack>?
resources/views/products/index.html:7: <rx-button> has no attribute "varient"; did you mean "variant"? It takes: label, variant, type, ...
resources/views/products/index.html:9: <rx-button> "variant" must be one of: primary, secondary, plain, danger, plain-danger; did you mean "danger"?
```

Line numbers are kept, so MiniJinja's own errors point at the line you wrote.

### Mails as tags

A mail view is a template like a page, and it compiles with the same checks (`view:check`,
`assert_views_compile`): a typo in a tag names the file and line. `<rx-mail>` is the mail's
layout, what `{% extends "renox/mail/layout.html" %}` does, and its parts are the mail kit's
macros:

```html
<rx-mail subject="{{ t('mail.receipt.subject', number=order.number) }}">
  <p>{{ t('mail.thanks') }}</p>
  <rx-mail-panel>Order #{{ order.number }} ships tomorrow.</rx-mail-panel>
  <rx-mail-table :rows="rows" :head="['Item', 'Price']" :total="['Total', total]"/>
  <rx-mail-divider/>
  <rx-mail-button :url="url" label="View your order"/>
  <rx-slot name="footer">Questions? Reply to this mail.</rx-slot>
</rx-mail>
```

| Tag | What it is |
|---|---|
| `<rx-mail subject layout>` | The whole template, like `rx-page`. `subject` fills the layout's `title` block (the HTML `<title>`; the subject line the recipient sees is the one you give `mail_view`). The content is the layout's `content` block, and `<rx-slot name="footer">` replaces its footer. `layout` names another layout file (plain text; `renox/mail/layout.html` by default, which an app can replace by file name). |
| `<rx-mail-button url label>` | `button(url, label)`; the label can be the content. |
| `<rx-mail-panel>` | `{% call panel() %}`: a highlighted box. |
| `<rx-mail-table :rows :head :total>` | `table(rows, head, total)`. The rows, the heading and the total are data (lists of cells): a mail with a row per order line builds them in Rust or with a filter, not with a counter in the template. |
| `<rx-mail-divider/>` | `divider()`. |
| `<rx-mail-text>` | A `.txt` view (the mail's text version), see below. |

The text version of a mail, the `.txt` view next to the `.html` one, is written with
`<rx-mail-text>`. Its text is kept as written, line breaks included, and nothing is escaped (it
is plain text): `{{ value }}` prints values, and a loop or a condition is a `<template rx-for>` /
`<template rx-if>`, which draws no tag of its own. The line break right after the opening tag and
the one right before the closing tag belong to the tags, not to the text:

```html
<rx-mail-text>
{{ title }}

{{ body }}
<template rx-for="row in rows">
{{ row.label }}: {{ row.value }}</template>
<template rx-if="url">
{{ action }}: {{ url }}</template>
</rx-mail-text>
```

renders `Title`, a blank line, the body, then one `label: value` line per row and the link line
when there is a `url`: the same bytes as the `{% for %}` / `{% if %}` version. Write a `<` that
is text as `{{ '<' }}`. Like `<rx-mail>`, the tag holds the whole template.

The framework's own sign-in mails (`renox/mail/auth/verify-email.html` and
`reset-password.html`) are written this way; a test checks that their HTML is byte for byte what
the `{% extends %}` version made. `preheader` is not a slot: the layout has no such block.

### Upgrading from 1.0

A 1.0 template keeps working: its `{% from "renox/ui.html" import … %}` and `{% call %}` macros still compile and mix with tags in one file. New pages use tags. To move a page over, find the tag for the macro below (the macro names are also the names the kit's reference tables use). Older component files without `<rx-props>` are macros too: `rnx make:component --macro` writes one. An imported macro takes its name in the template, so a value the handler passes under the same name is hidden by it (while debugging, Renox logs a warning naming both); rename the value or import the macro `as` another name.

| Tag | Macro it replaces |
|---|---|
| `rx-card`, `rx-toolbar`, `rx-page-header`, `rx-badge` | `card`, `toolbar`, `page_header`, `badge` |
| `rx-button`, `rx-link-button`, `rx-icon-button` | `button`, `link_button`, `icon_button` |
| `rx-confirm`, `rx-alert`, `rx-empty` | `confirm`, `alert`, `empty` |
| `rx-input`, `rx-textarea`, `rx-select`, `rx-checkbox`, `rx-radio`, `rx-checkbox-list`, `rx-toggle-buttons`, `rx-file`, `rx-date-picker`, `rx-tags-input` | the macro of the same name (`input`, `textarea`, `select`, ...) |
| `rx-repeater`, `rx-key-value`, `rx-fieldset`, `rx-show-when`, `rx-hide-when`, `rx-form-errors`, `rx-form-grid` | `repeater`, `key_value`, `fieldset`, `show_when`, `hide_when`, `form_errors`, `form_grid` |
| `rx-wizard`, `rx-wizard-step` | the `wizard` macros (a special tag) |
| `rx-infolist`, `rx-entry`, `rx-repeatable` | `infolist`, `entry`, `repeatable` |
| `rx-list`, `rx-card-grid`, `rx-media-card`, `rx-progress` | `list`, `card_grid`, `media_card`, `progress` |
| `rx-icon`, `rx-columns`, `rx-thumbnail`, `rx-tab-panel` | `icon`, `columns`, `thumbnail`, `tab_panel` |
| `rx-stats`, `rx-stat`, `rx-dashboard`, `rx-widget`, `rx-period-filter` | `stats`, `stat`, `dashboard`, `widget`, `period_filter` |
| `rx-chart` | the `chart(...)` template function (not a macro) |
| `rx-grid` | `grid(page, tools=…)` of `renox/grid.html` |
| `rx-rich-editor`, `rx-markdown-editor`, `rx-code-editor`, `rx-code-entry` | the macros of `renox-editors/editors.html` (registered by that plugin) |
| `rx-navbar`, `rx-nav-links`, `rx-nav-link`, `rx-nav-search`, `rx-sidebar`, `rx-sidebar-link`, `rx-sidebar-section` | `navbar`, `nav_links`, `nav_link`, `nav_search`, `sidebar`, `sidebar_link`, `sidebar_section` |
| `rx-menu`, `rx-menu-link`, `rx-menu-action`, `rx-menu-section`, `rx-menu-separator`, `rx-menu-open`, `rx-menu-button`, `rx-link-tabs` | `menu`, `menu_link`, `menu_action`, `menu_section`, `menu_separator`, `menu_open`, `menu_button`, `link_tabs` |
| `rx-action-group`, `rx-import-action`, `rx-event-stream` | `action_group`, `import_action`, `event_stream` |
| `rx-wizard-action`, `rx-wizard-step` | the `wizard_action` macro and its `wizard_step`s (a special tag) |
| `rx-mail`, `rx-mail-button`, `rx-mail-panel`, `rx-mail-table`, `rx-mail-divider` | `{% extends "renox/mail/layout.html" %}` with its blocks, and `button`, `panel`, `table`, `divider` of `renox/mail/components.html` |
| `rx-tabs`, `rx-tab` | the `tabs` macros (a special tag); `:items` gives the list as data |
| `rx-link-tab` | the `[href, label]` pairs of `link_tabs`, as children that can be looped |
| `rx-simple-table` | `{% call table(head, caption=…, foot=…) %}` |
| `rx-include` | `{% include "name" %}`, `{% include name ignore missing %}` |
| `rx-mail-text` | a text mail written with `{% for %}` / `{% if %}` |
| `?name="expr"` | `{{ ' name="…"' \| safe if expr }}` inside a start tag |
| `rx-sheet`, `rx-open-button`, `rx-action-sheet`, `rx-notification-bell` | `sheet`, `open_button`, `action_sheet`, `notification_bell` |
| `rx-shell` | none: a `div.rx-shell` (`fill` adds `rx-shell--fill`) |
| `rx-stack`, `rx-row` | none: a `div` with the kit's class |
| `rx-block` | `{% block name %}…{% endblock %}` |
| `rx-pagination` | `pagination(page)` or `simple_pagination(page)` of `renox/pagination.html` |
| `rx-head`, `rx-ui`, `rx-toasts` | the template functions `renox_head()`, `renox_ui()`, `toasts()` |
| `rx-outlet` | `name`: `{% block name %}…{% endblock %}` (the children are its default); `stack`: `stack(name)`. Exactly one |
| `rx-let` | `{% with name = value %}` around the rest of the element |
| `rx-script`, `rx-style` | `<script nonce="{{ csp_nonce() }}">`, `<style nonce="{{ csp_nonce() }}">` |
| `<template rx-if>`, `rx-elif`, `rx-else`, `rx-for` | `{% if %}`, `{% elif %}`, `{% else %}`, `{% for %}` around the children |
| `rx-page`, `rx-push`, `rx-form`, `rx-table` | `{% extends %}`/`{% block %}`, `push`, a `<form>`, a loop with `table` markup (`<rx-slot name="footer">` is the `foot` of `table`) |

Every macro of the kit has a tag now, with one exception on purpose: `page_hero` is not the kit's. It is the bike shop's own macro (`layouts/_hero.html`, with its `bs-hero` classes), so its tag is the app's: write it as a component, `components/hero.html` with `<rx-props title accent lead eyebrow photo compact>`, and use `<app-hero …>` (see [Your own components](#your-own-components)). The `{% call(row, column) grid(page) %}` form needs nothing new: `<rx-grid :page="page">…</rx-grid>` already receives `row` and `column` in its content.

Navigation, menus, tabs and sheets as tags. A link takes `href` or `route`, and its label as an
attribute or as content. `<rx-tabs>` takes its tab list from the `<rx-tab key label>` children
and gives each panel an id built from its own `id` (like `rx-wizard`):

```html
<rx-navbar brand="{{ app.name }}" href="{{ route('home') }}">
  <rx-nav-links>
    <rx-nav-link route="orders.index" label="Orders" :active="route_is('orders.*')"/>
  </rx-nav-links>
  <span class="rx-spacer"></span>
  <rx-notification-bell/>
  <rx-menu label="{{ auth.user.name }}" id="account-menu" variant="plain">
    <rx-menu-link route="account.show" label="Account"/>
    <rx-menu-separator/>
    <rx-menu-action route="logout" label="Log out"/>
  </rx-menu>
</rx-navbar>

<rx-shell fill>
  <rx-sidebar brand="Back office">
    <rx-sidebar-section title="Sales"/>
    <rx-sidebar-link route="orders.index" label="Orders" icon="receipt"/>
  </rx-sidebar>
  <div class="rx-shell__main">
    <rx-navbar width="full" :skip="false"/>
    <main class="rx-shell__content" id="main">
      <rx-tabs id="order-tabs" selected="items">
        <rx-tab key="items" label="Items">…</rx-tab>
        <rx-tab key="history" label="History">…</rx-tab>
      </rx-tabs>
      <rx-open-button id="note" label="Add note" variant="primary"/>
      <rx-sheet id="note" title="Note">…</rx-sheet>
    </main>
  </div>
</rx-shell>
```

`<rx-shell>` is a `<div class="rx-shell">`, and `<rx-shell fill>` a
`<div class="rx-shell rx-shell--fill">`, the standard for dashboard apps. `<rx-sheet>` and
`<rx-action-sheet>` take the sheet events (`@closed` and others) like any component.

A dashboard written with tags. `rx-chart` calls the `chart(...)` function, so it takes the same
arguments (`kind`, `labels`, `series`, `format`, `title`, `id`):

```html
<rx-period-filter :selected="period" :options="periods" label="Period"/>
<rx-stats columns="4">
  <rx-stat label="Revenue" :value="revenue | money" :delta="change" icon="banknote"/>
  <rx-stat label="Orders" :value="orders | number" route="orders.index"/>
</rx-stats>
<rx-dashboard columns="3">
  <rx-widget title="Revenue" span="2">
    <rx-chart kind="bar" :labels="labels" :series="series" format="money" title="Revenue" id="chart-revenue"/>
  </rx-widget>
</rx-dashboard>
```

A data grid is one tag. Its content draws the `custom` columns, with `row` and `column` in it:

```html
<rx-grid :page="orders">
  <a rx-if="column.key == 'actions'" class="rx-link" href="{{ route('orders.show', row.id) }}">Open</a>
</rx-grid>
```

### Tags from a plugin

A plugin's templates are compiled like the app's: add them with `renox::view::add_template`
(inside `Registry::templates`), not `env.add_template_owned`. It compiles the tags first, and a
mistake is an error naming the template and line. A plugin can also give its macros a tag with
`Registry::component`:

```rust
use renox::prelude::*;
use renox::view::{Component, Prop, add_template};

struct Hello;

impl Module for Hello {
    fn name(&self) -> &'static str {
        "hello"
    }

    fn register(&self, app: &mut Registry) {
        app.templates(|env| {
            add_template(
                env,
                "hello/ui.html",
                "{% macro hello(name) %}<b>Hello {{ name }}</b>{% endmacro %}",
            )
            .unwrap();
        });
        app.component(
            Component::macro_call("rx-hello", "hello/ui.html", "hello")
                .prop(Prop::text("name").required())
                .content(false),
        );
    }
}
```

The tag must start with `rx-` and not clash with a built-in or another module's tag (a clash
panics at boot). `renox-editors` does this for `rx-rich-editor`, `rx-markdown-editor`,
`rx-code-editor` and `rx-code-entry`.

### Editor autocomplete

VS Code can suggest the kit's tags and their attributes while you type a template. Renox writes
the list for it: `<app> view:data` (or `rnx view:data`) writes `.vscode/renox-components.json`,
built from the components your app has, the kit's and your own. `rnx new` already points
VS Code at that file (`.vscode/settings.json` sets `html.customData`), keeps the file out of
git, and `rnx serve` refreshes it after every successful build, so a new component shows up
without any step from you. If a refresh fails, `rnx serve` prints a warning and carries on.

To set it up in an older app, run `rnx view:data` once and add this to `.vscode/settings.json`:

```json
{ "html.customData": ["./.vscode/renox-components.json"] }
```

An app command named `view:data` replaces the built-in one. `--out PATH` writes the file
somewhere else.

> [!NOTE]
> JetBrains IDEs don't read this file: they use their own format (Web Types), which Renox
> doesn't write yet.

## Toasts

A **toast** is a short message that pops up and then goes away, like "Product saved.". To
show one, return a `Toast` with the response:

```rust
use renox::prelude::*;

/// Saves (in a real app), then goes back to the list with a "saved" toast.
async fn save() -> (Toast, Redirect) {
    (Toast::success("Product saved."), Redirect::to("/products"))
}
```

The handler returns two things at once (a tuple): the toast and a redirect.

How it reaches the screen:

- After a redirect, the toast waits in the session. `{{ toasts() }}` shows it on the next
  page, once.
- For an htmx request, it rides in the `HX-Trigger` header (as the event `renox:toast`) and
  appears at once.

Kinds: `success`, `info`, `warning` and `error`. An `error` toast is announced as an alert
by screen readers, and stays until dismissed.

### A toast that says more

A toast can carry more: a second line, links, buttons, a duration.

```rust
use renox::prelude::*;
use renox::ToastAction;

/// Places an order, then shows a toast with links and an Undo button.
async fn place() -> (Toast, Redirect) {
    let toast = Toast::success("Order #7 placed")
        .body("We'll email you when it ships.")            // a second, lighter line
        .link("View order", "/orders/7")                    // links under the text
        .action(ToastAction::link("Help", "https://example.com/help").new_tab())
        .action(ToastAction::event("Undo", "order-undo"))   // a DOM event on `document`
        .seconds(8)                                          // or .persistent()
        .id("order-7");                                      // Renox.dismissToast("order-7")
    (toast, Redirect::to("/orders"))
}
```

> [!NOTE]
> **Coming from Laravel:** this is like Filament's notifications. Instead of
> `session()->flash('status')` and a toast library, you return a `Toast`.

The details:

- Every action closes the toast.
- A link goes only to http(s), mailto, tel or a relative URL.
- An event action sends ("dispatches") its event on `document`, and `detail.toast` is the
  toast's id. So an htmx element can listen for it with
  `hx-trigger="order-undo from:document"`.
- `seconds(n)` sets how long it stays (for errors too). `persistent()` keeps it until
  dismissed. Every toast waits while the mouse is over it or it has focus.
- A toast with an `id` replaces an earlier one with the same id.
- `{{ toasts(position="bottom-end") }}` moves them: `top` (the default, centred),
  `top-start`, `top-end`, `bottom`, `bottom-start` or `bottom-end`. Phones keep them centred.
- From the page's own JavaScript: `Renox.toast({kind: "success", message: "Copied", body: "…",
  actions: [{label: "Open", url: "/x"}], duration: 3000, id: "copy"})` and
  `Renox.dismissToast("copy")`.

### A button that sends a request

A link only goes somewhere. A **request action** does something: "Undo", "Retry", "Approve".
It sends a `POST`, `PUT`, `PATCH` or `DELETE` to a path of your app, with the CSRF token, and
stays on the page.

```rust
use renox::prelude::*;
use renox::ToastAction;

/// Archives an order (htmx), with an Undo button in the toast.
async fn archive(Path(id): Path<i64>) -> Toast {
    // … archive …
    Toast::info(format!("Order #{id} archived"))
        .action(ToastAction::delete("Undo", format!("/orders/{id}/archive")))
}

/// What Undo sends: `DELETE /orders/{id}/archive`. The answer is a toast too.
async fn unarchive(Path(id): Path<i64>) -> Toast {
    // … restore …
    Toast::success(format!("Order #{id} is back"))
}
```

The details:

- `ToastAction::post(label, url)`, `put`, `patch` and `delete` make one. In JSON (for
  `Renox.toast` or a stored notification) it is `{"label": "Undo", "url": "/orders/7/archive",
  "method": "DELETE"}`.
- Only paths of your own site (`/orders/7`) are sent, never another address: the request
  carries the CSRF token, which must not leave the site. Others aren't shown.
- renox-ui.js sends it with htmx, swapping nothing. So answer it like any htmx request that
  doesn't swap: a `Toast` (a `204` carrying it), `(Toast, HxRefresh)` to reload the page, an
  `HxRedirect`, or an `HxTrigger` for an event of your own. A plain `Redirect` would be followed
  quietly, and its toast lost.
- If the request fails (a 4xx or 5xx, or no network) and the answer brought no toast of its
  own, the page shows an error toast: "That didn't work. Try again." (`ui.request_failed`).
- The same button works in the bell's list (a `DatabaseMessage` action); without JavaScript
  it is a plain form there.
- From the page's own JavaScript: `Renox.request("POST", "/orders/7/retry")`.
- A toast pushed from the server to open pages (`state.broadcast_to(user_id, "renox:toast",
  json!({ "toasts": [toast] }))`, see [mail.md](mail.md#your-own-live-events)) can carry one
  too, say a "Reopen" button on a failed charge.

Toasts go away. For notifications that stay (a bell in the navigation bar, with new ones
arriving live), see [mail.md](mail.md#the-bell).

## Components and the page: three directions

A kit component talks to the page it sits in, and the page talks back, in three directions.

### Page → component: `attrs`

Every macro takes `attrs={…}`: attributes that are not props go on the component's root element
(for a field, the control itself). With tags you just write them:

```html
<rx-input name="qty" label="Quantity" class="wide" x-bind:disabled="busy" data-test="qty"/>
```

- They are printed **after** the component's own attributes. For a duplicate the browser keeps
  the first one, so the component's value wins.
- `class` is the exception: it is appended to the root's class list.
- On a component tag `:name` is always a prop (`:value="row.qty"`). To bind an Alpine value to
  an attribute, write `x-bind:name`.

### Component → page: events

Components with behaviour in the browser send a bubbling `CustomEvent` named
`rx:<component>:<event>` from one root element, which carries `data-rx-component="<name>"`.

| Component | Event | `detail` | Fires on |
|---|---|---|---|
| `sheet`, `action-sheet`, `wizard-action` | `opened` | `{id}` | the `<dialog>` |
| | `closed` | `{id}` | the `<dialog>` |
| `action-sheet`, `wizard-action` (the form inside) | `saved` | `{id, status}` | the `<dialog>` |
| | `failed` (any non-2xx answer) | `{id, status}` | the `<dialog>` |
| `confirm` | `confirmed` | `{id}` | the `<dialog>` |
| | `cancelled` (closed without confirming; `closed` follows) | `{id}` | the `<dialog>` |
| `tabs` | `changed` | `{name}` | the `[role=tablist]` element |
| `wizard` | `changed` | `{name, index}` | `.rx-wizard` |
| `select` | `changed` | `{value}` (an array when `multiple`) | the `<select>` |
| `tags-input` | `changed` | `{value}` (an array of tags) | the tags entry input |
| `date-picker` | `changed` | `{value}` | the `data-rx-date` input |
| `repeater` | `added` | `{index}` | the `<fieldset>` |
| | `removed` | `{index}` | the `<fieldset>` |
| `grid` | `selected` | `{ids, all}` | `form.rx-grid` |
| | `sorted` | `{sort}` | `form.rx-grid` |
| | `filtered` | `{query}` (the form as a query string) | `form.rx-grid` |

`changed` fires only when the value really changed, and never on first setup (tabs and wizard
compare with the previous selection; tags with the last list sent).

Listen on the component with Alpine, or from anywhere with `.window`:

```html
{# on the component itself: the shorthand compiles to x-on:rx:sheet:closed.self #}
<rx-action-sheet id="new-item" title="New item" action="{{ route('items.store') }}" @saved="reload = true">…</rx-action-sheet>

{# anywhere on the page #}
<div x-data="{ tab: '' }" @rx:tabs:changed.window="tab = $event.detail.name">…</div>

{# htmx: refresh a part when a component says so #}
<div hx-get="/cart/summary" hx-trigger="rx:select:changed from:body">…</div>
```

`@name` on an `<rx-…>` tag works for the events the component declares (a misspelt one is a
load error that lists them). Because the events bubble, `hx-trigger="… from:body"` sees every
one on the page.

### Server → browser

The server reaches the page with the tools described elsewhere in this guide:

- `HxTrigger` sends your own event with an htmx response (see
  [htmx response headers](#htmx-response-headers)); listen with `@my-event.window` or
  `hx-trigger="my-event from:body"`.
- [Toasts](#toasts) show a message, and can carry an action that dispatches an event.
- `event_stream()` (see the `Auth` notes above) opens the live stream, so `state.broadcast(…)`
  events arrive as DOM events.

## Fragments and out-of-band swaps

When htmx asks for a page, it often needs only one part of it: the new rows of a table, say.
That part is a **fragment**.

- `view(…).fragment("rows")` sends only the block named `rows`, for htmx requests that aren't boosted (`hx-boost` requests get the whole page, since htmx swaps the body).
- `.also("count")` adds more blocks after it.
- `.status(StatusCode::CREATED)` sends the page (or the fragment) with another status than
  200.

An extra block can update a different place on the page. This is an **out-of-band swap**:
"out of band" means outside the main spot htmx was going to update. Give the extra block's
outer element an `id` and `hx-swap-oob="true"`. htmx then puts it into the element on the page
with that same id:

```html
{% block rows %}<tr id="order-{{ order.id }}">…</tr>{% endblock %}
{% block count %}<span id="order-count" hx-swap-oob="true">{{ count }}</span>{% endblock %}
```

```rust
# use renox::prelude::*;
# fn demo(count: i64) -> View {
// Send the `rows` block, plus the `count` block, which htmx swaps into #order-count.
view("orders/index.html", context! { count }).fragment("rows").also("count")
# }
```

So one answer updates the table row and the order count at the top of the page.

> [!NOTE]
> **Coming from Laravel:** this is like `@fragment` / `fragments([...])`.

## htmx response headers

htmx reads special headers in the answer to decide what to do next. In Renox these headers
are types. Return them in a tuple with the response:

- `HxRetarget("#errors".into())`: swap the answer into another element;
- `HxReswap("outerHTML".into())`: swap it in a different way;
- `HxPushUrl("/orders?status=open".into())`: change the address in the browser's address bar;
- `HxRedirect`, `HxRefresh` and `HxTrigger`: go to another page, reload this one, or send an
  event to the page.

Each of them holds a `String`, so write `.into()` after a text in quotes. `HxRetarget`,
`HxReswap` and `HxPushUrl` aren't in the prelude: import them with
`use renox::{HxPushUrl, HxReswap, HxRetarget};`.

```rust
# use renox::prelude::*;
use renox::{HxReswap, HxRetarget};

/// Puts the order list in place of the whole `#orders` element.
async fn reload() -> (HxRetarget, HxReswap, View) {
    (
        HxRetarget("#orders".into()),
        HxReswap("outerHTML".into()),
        view("orders/index.html", context! {}).fragment("orders"),
    )
}
```

### What htmx sent: the `Htmx` extractor

The other way round, `Htmx` (in the prelude) tells a handler what htmx sent with the request.
Ask for it as an argument. Its fields:

- `request`: `true` when htmx made the request (the `HX-Request` header);
- `boosted`: `true` when it came from an `hx-boost` link or form, which expects a whole page;
- `target`: the id of the element the answer goes into (`HX-Target`), if any;
- `trigger`: the id of the element that started the request (`HX-Trigger`), if any;
- `current_url`: the address the browser shows now (`HX-Current-URL`), if any.

And two helpers:

- `htmx.wants_fragment()` is `true` for an htmx request that isn't boosted: one that wants a
  part of the page, not all of it. That's the same test `.fragment(…)` makes.
- `htmx.redirect("/orders")` goes to another page after a form is sent: an `HX-Redirect`
  header for htmx (the browser loads the whole page), a `303 See Other` redirect otherwise.

```rust
# use renox::prelude::*;
/// Sends only the list to htmx, and the whole page to everyone else.
async fn index(htmx: Htmx) -> View {
    let page = view("products/index.html", context! {});
    if htmx.wants_fragment() { page.fragment("list") } else { page }
}

/// After saving, goes to the list, whether htmx sent the form or not.
async fn store(htmx: Htmx) -> Response {
    htmx.redirect("/products")
}
```

### Going back

`Back` (in the prelude) is both an extractor (an argument the handler asks for) and a
response. It redirects to the previous page (from the `Referer` header). When there's no
previous page, or it's on another site, it goes to `/` instead. So nobody can use it to send
your visitors to a stranger's site (an "open redirect").

```rust
# use renox::prelude::*;
/// Saves (in a real app), flashes "Saved", and goes back to the page the form was on.
async fn store(back: Back, session: Session) -> Result<Back> {
    // A flash message lives for one request: the next page can show it.
    session.flash("status", "Saved")?;
    Ok(back)
}
```

## Live validation

Live validation checks a form's fields while people fill it in, before they press Submit.

Add `data-live-validate` to a form that `Valid<T>` handles. Then:

1. When someone leaves a field, the kit's script sends that field's value (with the rest of
   the form), with the header `X-Renox-Validate: <field name>`. It sends it again as the field is
   corrected.
2. The `Valid` extractor answers with that field's errors as JSON, and **the handler doesn't
   run**. So nothing is saved until the form is really submitted.
3. The kit shows the errors next to the field.

The rules are the form's own, including database checks such as `unique`. You don't write
them twice.

> [!NOTE]
> **Coming from Laravel:** this is like Precognition.

## The current route, conditional classes, loops

A navigation bar marks the current section with `route_is`. It takes the route's name, and
`*` stands for anything.

`class_names` builds a `class` attribute. It always includes the plain classes, and each
class in the `{…}` map only when its condition is true:

```html
<a href="{{ route('admin.products.index') }}"
   class="{{ class_names('tab', {'tab-active': route_is('admin.products.*')}) }}"
   {% if route_is('admin.products.*') %}aria-current="page"{% endif %}>Products</a>
{# request.route is the name itself, e.g. "admin.products.edit" #}
```

On any admin products page, this link gets `class="tab tab-active"` and `aria-current`.
Elsewhere, it's just `class="tab"`.

- `route_is` also takes several patterns: `route_is('orders.*', 'checkout')`.
- In Rust, the `CurrentRoute` extractor does the same: `route.is("admin.*")`, `route.name()`.

### Loops that stop early

Loops can stop early with `{% break %}`, or skip an item with `{% continue %}`:

```html
{% for product in recently_viewed %}{% if loop.index > 4 %}{% break %}{% endif %}…{% endfor %}
```

This shows at most four products: on the fifth, the loop stops.

### Texts that depend on a number

Some texts change with a count: "Sold out", "Only one left", "12 in stock". The lang file
holds all the versions in one line, using Laravel's plural ranges:

- `{n}` for exactly n;
- `[a,b]` for a range from a to b;
- `*` for "no end".

For example: `"{0} Sold out|{1} Only one left|[2,5] Only :count
left|[6,*] :count in stock"`. Print it with
`{{ t('products.in_stock', count=product.stock) }}`. The bike shop counts its search results
this way (`catalog.found` in [resources/lang/en.json](../examples/bikeshop/resources/lang/en.json)).

## Actions

An **action** is a button that does one thing, often after asking for a few values. Think
"Adjust stock", which asks how many to add or take away.

`action_sheet` is all of it at once: the button, a sheet, and the form inside the sheet. The
fields go in its content:

```html
<rx-action-sheet :id="'stock-' ~ product.id" label="Stock" :action="route('admin.products.stock', product.id)"
                 :title="'Adjust stock of ' ~ product.name" description="A negative number takes away."
                 submit-label="Adjust stock" method="PUT" icon="box" size="small">
  <rx-input name="change" label="Change" type="number" required :id="'stock-change-' ~ product.id"/>
</rx-action-sheet>
```

This makes a small "Stock" button with a box icon. Clicking it opens a sheet titled "Adjust
stock of …", with one number field and an "Adjust stock" button. The form is sent with
`PUT` to the product's stock route.

> [!NOTE]
> **Coming from Laravel:** these are Filament's actions, as the kit has them.

What happens when the form is sent:

- It's sent with htmx. `PUT`, `PATCH` and `DELETE` go through `_method`.
- A 422 answer (the form had errors) shows its messages under the fields, inside the sheet,
  which stays open.
- Any other success closes the sheet and resets the form. Cancel and Esc do that too.

The handler is an ordinary one. It can answer with:

- a `Toast`, to say what happened;
- `HxRefresh`, to reload the page (the toast waits in the session);
- `HxTrigger`, to tell other parts of the page;
- or, with `target` and `swap` on the action sheet, HTML that replaces part of the page.

Some checks can't be written as rules, because they need the database. The handler can
answer with a `ValidationError`, which shows up under its field the same way:

```rust
use renox::prelude::*;
use renox::HxRefresh;

/// The action's form: how much to add (or, if negative, take away).
#[derive(serde::Deserialize, serde::Serialize)]
struct StockForm { change: i64 }

/// The form's rules: a number is required, and it can't be 0.
impl Validate for StockForm {
    fn rules(&self, v: &mut Validator) {
        v.field("change", &self.change).required().rule(self.change != 0, "Type how many.");
    }
}

/// Changes a product's stock, refuses to go below zero, and reloads the page.
async fn adjust_stock(Path(id): Path<i64>, Valid(form): Valid<StockForm>) -> Result<(Toast, HxRefresh)> {
    let stock = 3; // the product's, from the database
    // Taking away more than there is: answer with an error under the "change" field.
    if stock + form.change < 0 {
        let mut errors = Errors::new();
        errors.add("change", format!("Only {stock} in stock to take away."));
        return Err(ValidationError::new(errors).with_input(&form).into());
    }
    // All good: a toast says what happened, and the page reloads.
    Ok((Toast::success(format!("Product {id}: {} in stock.", stock + form.change)), HxRefresh))
}
```

The action sheet's other options:

- for the button: `variant`, `size`, `icon` and `key`;
- for the sheet: `slide_over`, `width` and `modal_icon`;
- `danger=true` for a red submit button;
- `target`/`swap` for htmx;
- `enctype="multipart/form-data"` for a file field.

> [!IMPORTANT]
> When a page has one action sheet per row, give each its own `id`, and its fields their own
> `id=`, as in the example above. Otherwise two rows' sheets get mixed up.

### What every button can carry

These work on every button (`button`, `link_button`, `open_button`, `icon_button`), except
the disabled reason at the end of the list:

- **An icon** before the label, by name: `icon="plus"`. Any name of the kit's
  [icons](#icons) works, and so do the status marks `info`, `success`, `warning` and
  `error`.
- **A count**: `badge=3`. A 0 is shown; an empty string or `none` isn't.
- **A keyboard shortcut**: `key="mod+s"`.
  - `mod` is ⌘ on a Mac and Ctrl elsewhere. You can also use `ctrl`, `alt`, `shift`, and keys
    like `enter`, `esc`, `backspace`.
  - The shortcut clicks the first visible element with that key (inside the open sheet, when
    there is one).
  - A key without `mod`, `ctrl` or `alt` doesn't fire while someone is typing in a field.
  - The button gets `aria-keyshortcuts`, and its tooltip (or `title`) names the shortcut.
- **A reason it's disabled**: `disabled_reason="Add a photo first."` The button can still get
  focus and shows the reason as its tooltip (on a tap too), but a click does nothing. Plain
  `disabled=true` instead takes it out of the tab order entirely. A reason alone disables the
  button; with a condition, pass both: `disabled=(not photo), disabled_reason="Add a photo
  first."` (when `disabled` is false the button works and the reason isn't shown). Only `button` and
  `icon_button` (without `href`) take `disabled` and `disabled_reason`; `link_button` and
  `open_button` take neither.

An `icon_button`'s `label` is read out by screen readers. It's also shown as a tooltip after
a short hover, or at once when the button gets keyboard focus. Any element can have a tooltip
with `data-rx-tip="…"`.

The bike shop has all of these:

- a "Write off" `action_sheet` on a stock level's page
  ([stock/ledger.html](../examples/bikeshop/resources/views/stock/ledger.html));
- the counter's buttons on keys, and "Pay" disabled with a reason while the sale is empty
  ([sales/counter/show.html](../examples/bikeshop/resources/views/sales/counter/show.html));
- "New item" on the `n` key on `/about/htmx`
  ([about/htmx.html](../examples/bikeshop/resources/views/about/htmx.html)).

### Action groups

When a page has more actions than it has room for, put the rarer ones behind one button.
`action_group` is a `menu` made for that (Filament's ActionGroup): with no label it's a "⋯"
icon button that screen readers call "Actions".

```html
{% from "renox/ui.html" import action_group, menu_link, menu_open, menu_action, menu_separator, confirm %}
{% call action_group() %}
  {{ menu_link(route('products.replicate', product.id), "Duplicate", icon="copy") }}
  {{ menu_link(route('products.ledger', product.id), "Export ledger", icon="download", download=true) }}
  {{ menu_action(route('products.archive', product.id), "Archive") }}
  {{ menu_separator() }}
  {{ menu_open("delete-product", "Delete", icon="trash", danger=true) }}
{% endcall %}
{{ confirm("delete-product", "Delete", route('products.destroy', product.id),
           "Delete " ~ product.name ~ "?", "This can't be undone.", button=false) }}
```

As tags the same group is `<rx-action-group>` around `<rx-menu-link>`, `<rx-menu-action>`,
`<rx-menu-button>`, `<rx-menu-open>`, `<rx-menu-separator/>` and `<rx-menu-section>`:

```html
<rx-action-group>
  <rx-menu-link :href="route('products.replicate', product.id)" label="Duplicate" icon="copy"/>
  <rx-menu-open id="delete-product" label="Delete" icon="trash" danger/>
</rx-action-group>
<rx-confirm id="delete-product" label="Delete" :action="route('products.destroy', product.id)"
            title="Delete {{ product.name }}?" message="This can't be undone." :button="false"/>
```

- `action_group("More")` (`<rx-action-group label="More">`) shows a labelled button instead; `icon` changes the icon.
- Its items: `menu_link` (a link; `download=true` for a file to save, `new_tab=true`),
  `menu_action` (sends a form), `menu_button` (a button with htmx `attrs`), `menu_open`
  (opens a sheet), `menu_separator()`, and `menu_section(title)` around items that belong
  together. Each takes an `icon`; `danger=true` shows it in red.
- `menu_open(id, label)` opens a sheet made elsewhere on the page with `button=false`:
  an `action_sheet`, `confirm`, `wizard_action` or `import_action` without its own button.
  The menu closes, and when the sheet closes the focus goes back to the group's button.
- The keyboard works as in every kit menu: Enter, Space or ↓ opens it on the first item,
  ↑ on the last; the arrows, Home and End move; Esc closes it and returns to the button.

### An action with steps

`wizard_action` is an `action_sheet` whose form is a [`wizard`](#form-fields): one step at a
time, Next checks the step's fields (with the server's rules too: the form is
`data-live-validate`, `live=false` turns that off), and the last step's button sends the
form with htmx.

```html
{% from "renox/ui.html" import wizard_action, wizard_step, input %}
{% call wizard_action("new-product", "New product", route('products.store'), "New product",
                      [["details", "Details"], ["stock", "Price and stock"]],
                      submit_label="Add product", variant="primary", icon="plus") %}
  {% call wizard_step("new-product", "details") %}
    {{ input("sku", "SKU", required=true, id="new-product-sku") }}
    {{ input("name", "Name", required=true, id="new-product-name") }}
  {% endcall %}
  {% call wizard_step("new-product", "stock") %}
    {{ input("price", "Price", type="number", required=true, id="new-product-price") }}
  {% endcall %}
{% endcall %}
```

As a tag, the steps are the `<rx-wizard-step>` children, as in `<rx-wizard>`:

```html
<rx-wizard-action id="new-product" label="New product" route="products.store" title="New product"
                  submit-label="Add product" variant="primary" icon="plus">
  <rx-wizard-step key="details" title="Details">
    <rx-input name="sku" label="SKU" required id="new-product-sku"/>
  </rx-wizard-step>
  <rx-wizard-step key="stock" title="Price and stock">
    <rx-input name="price" label="Price" type="number" required id="new-product-price"/>
  </rx-wizard-step>
</rx-wizard-action>
```

The handler is the same as for any form. A 422 after the last step opens the first step with
an error and focuses the field, so a SKU someone else took in the meantime sends the user back
to step 1. A success closes the sheet; closing it any way starts the wizard over.

### Import

`renox::import` reads a CSV file row by row, **as if each row were a form**: the columns, named
by the file's first line, fill a struct with `Deserialize` and `Validate`, and its rules (and
its `prepare` and `after` hooks) check the row. Rows that pass are written by your closure in
one transaction, a savepoint each, so a row the database refuses (a unique index, a foreign
key) undoes only its own writes. The others are reported with their row number, as a
spreadsheet counts (the line of column names is row 1).

```rust
# use renox::prelude::*;
use renox::import::{Import, ImportReport};
# #[derive(Model, serde::Serialize, Default)]
# #[model(table = "products")]
# struct Product { id: i64, sku: String, name: String, price: i64 }

/// One row: `sku,name,price`.
#[derive(serde::Deserialize, Validate)]
struct ProductRow {
    #[validate(required, max = 30, alpha_dash)]
    sku: String,
    #[validate(required, max = 100)]
    name: String,
    #[validate(required, min = 0)]
    price: i64,
}

/// The import sheet's form: one CSV file.
#[derive(serde::Deserialize, Validate)]
struct ImportForm {
    #[validate(required, mimes(&["csv", "txt"]))]
    file: Option<Upload>,
}

async fn import(
    State(state): State<AppState>,
    lang: Lang,
    Valid(form): Valid<ImportForm>,
) -> Result<ImportReport> {
    let file = form.file.ok_or(Error::NotFound)?;
    Import::csv(file.bytes())
        .lang(&lang) // the rules' messages in the user's language
        .run(&state, |tx, row: ProductRow| {
            Box::pin(async move {
                let product = Product { sku: row.sku, name: row.name, price: row.price, ..Default::default() };
                Product::create(tx, product).await?;
                Ok(())
            })
        })
        .await
}

/// The sheet's "Download a template" link: the columns, nothing else.
async fn template() -> renox::Download {
    renox::import::template("products.csv", &["sku", "name", "price"])
}
```

The page's side is `import_action`, a sheet with a file field and room for the report:

```html
{% from "renox/ui.html" import import_action %}
{{ import_action("import-products", "Import", route('products.import'), "Import products",
                 columns=["sku", "name", "price"], template_url=route('products.template')) }}
```

As a tag: `<rx-import-action id="import-products" label="Import" route="products.import"
title="Import products" :columns="['sku', 'name', 'price']" :template-url="route('products.template')"/>`
(`:button="false"` for a sheet opened from an action group).

What the person sees:

- Every row imported: the sheet closes, the page reloads, and a toast says how many.
- Some rows refused: a table of row numbers and messages appears in the sheet, which stays
  open. Closing it reloads the page, if some rows went in.
- A file that can't be read (not UTF-8 text, no rows, too many rows) is an error under the
  file field, like any other.

`Import`'s options:

- `.all_or_nothing()`: one bad row and nothing is written;
- `.delimiter(';')`: for files from a spreadsheet set to a European language;
- `.headers(&["sku", "name"])`: for a file without a line of column names;
- `.rename("Description", "name")`: a column under another name. Headings are matched
  ignoring case, and spaces and dashes become `_` ("Unit price" fills `unit_price`);
- `.max_rows(n)`: 10,000 by default;
- `.user(&user)`: the user that the rows' `after` hooks see.

The `ImportReport` has `imported`, `failed` (each with `row` and `errors`), `is_clean()` and
`summary()`, for a command or a job that imports without a page. Rows are checked before the
transaction starts, so a `unique` rule can't see a repeat within the same file: the database's
unique index catches that one, and the report says the row repeats a unique value.

#### Excel files

With the `xlsx` feature (the one that gives grids their Excel export), `Import::xlsx(bytes)` reads
a workbook the way `Import::csv` reads a file: the first row of the sheet names the columns, the
rows below are the records. Every cell is read as the text a CSV file would hold (`12`, not
`12.0`; a date as `2026-01-31`, with the time when it has one; empty cells empty), so the same
row struct and rules check both, and a workbook imports like the equivalent CSV. Row numbers in
the report are the sheet's own. `.sheet("Items")` picks a sheet by name (the first by default);
a workbook without it, or a file that isn't one, is an error under the file field.
`Import::from_upload(&upload)` picks CSV or Excel by the file's content (a workbook is a zip
archive), so one route takes both; give the sheet `accept=".csv,.xlsx"` (`:accept` in the tag).
Without the feature, CSV imports are unchanged and an Excel file is a clear error asking for CSV.

`renox::import::template_xlsx` is the matching template to download: headings, an example row (in
grey) and every column formatted for its type, so what people type in is read as the import
expects.

```rust
# use renox::prelude::*;
# #[derive(serde::Deserialize, Validate)]
# struct ProductRow { #[validate(required)] sku: String }
# #[derive(serde::Deserialize, Validate)]
# struct ImportForm { #[validate(required)] file: Option<Upload> }
# async fn import(State(state): State<AppState>, Valid(form): Valid<ImportForm>) -> Result<renox::import::ImportReport> {
let file = form.file.ok_or(Error::NotFound)?;
renox::import::Import::from_upload(&file) // CSV or Excel, by content
#     .run(&state, |_tx, _row: ProductRow| Box::pin(async { Ok(()) })).await
# }

/// The Excel template: `sku` as text, `price` with two decimals, `stock` whole numbers.
# #[cfg(feature = "xlsx")]
async fn template_xlsx() -> Result<renox::Download> {
    use renox::import::TemplateColumn;
    renox::import::template_xlsx(
        "products.xlsx",
        &[
            TemplateColumn::text("sku", "COFFEE-1"),
            TemplateColumn::number("price", 4.5, 2),
            TemplateColumn::integer("stock", 12),
            TemplateColumn::date("available_on", renox::chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
        ],
    )
}
```

#### Check first (dry run)

`.dry_run()` reads and validates every row and runs your closure for the valid ones in the
transaction, then rolls everything back. The report lists the same errors a real run would (a
repeat inside the file, a row the database refuses), says "3 rows can be imported", and the
database is unchanged. (A PostgreSQL sequence still advances.) The kit adds the step for you:
`check` on `<rx-import-action>` (`check=true` on the macro) adds a ticked "Check the file first"
box sent as `dry_run`, and the handler passes it on with `.dry_run_if(form.dry_run)`. The report of
a check keeps the sheet open (and doesn't reload the page), so the person unticks the box and
imports for real.

#### Undo

`.undoable(Duration::from_secs(86_400))` keeps what the import wrote for that long, and the report's
`batch_id` names it. Renox can't see SQL your closure runs, so write through the closure's
`ImportTx` (it stands in for the transaction; pass it where a transaction goes):
`tx.insert(&mut model)`, `tx.update(&mut model)` and `tx.save(&mut model)` write through the model
and record the table, the key and the row as it was. Anything written another way (raw SQL,
`Query::update`) is not undone.

```rust
# use renox::prelude::*;
use std::time::Duration;
use renox::import::{Import, ImportReport, ImportTx};
# #[derive(Model, serde::Serialize, Default)]
# #[model(table = "products")]
# struct Product { id: i64, sku: String, name: String, updated_at: Option<DateTime> }
# #[derive(serde::Deserialize, Validate)]
# struct ProductRow { #[validate(required)] sku: String, #[validate(required)] name: String }
/// Inserts the product, or updates the one with that SKU.
async fn write(tx: &mut ImportTx<'_>, row: ProductRow) -> Result {
    match Product::where_eq("sku", row.sku.as_str()).first(&mut *tx).await? {
        Some(mut product) => {
            product.name = row.name;
            tx.update(&mut product).await
        }
        None => tx.insert(&mut Product { sku: row.sku, name: row.name, ..Default::default() }).await,
    }
}

async fn import(state: AppState, bytes: Vec<u8>) -> Result<ImportReport> {
    Import::csv(bytes)
        .undoable(Duration::from_secs(86_400))
        .run(&state, |tx, row: ProductRow| Box::pin(write(tx, row)))
        .await
}

/// POST /imports/{batch}/undo
async fn undo(State(state): State<AppState>, Path(batch): Path<i64>) -> Result<Toast> {
    let undone = renox::import::undo(&state, batch).await?;
    Ok(Toast::success(format!("{} deleted, {} restored.", undone.deleted, undone.restored)))
}
```

`undo` deletes the rows the batch inserted and puts back the rows it updated (their before-image,
every column), in one transaction: either all of it or nothing. It refuses, with a message that is
shown like any `Error::BadRequest`, when

- the window has passed, or the batch was already undone;
- a row it would touch was deleted since, or **changed** since: the row's current `updated_at` is
  not the one the import left (a model without `updated_at` is undone without this check);
- other rows still refer to a row it would delete: the message names the table that does
  ("the products row 7 is still used by order_lines");
- your rule says no: `Undo::batch(id).can_undo(|tx, change| Box::pin(async move { … }))` asks about
  each row (`change.table()`, `change.key()`, `change.is_insert()`) in the undo's transaction;
  return `Err(Error::BadRequest("…".into()))` to refuse ("used by an issued document").

Encrypted columns are not kept in the before-image (it would hold the plaintext), so an undo
doesn't restore them. `my-app import:prune` (`renox::import::prune(&state, age)`) deletes the
records of finished windows; the tables are `import_batches` and `import_changes`.

#### Error files

A report that has failed rows can hand the file back for people to fix. `report.error_csv("errors.csv",
ErrorFile::WholeFile)` is the whole uploaded file with an added `Error` column (empty for the good
rows): fix it in place and upload it again. `ErrorFile::FailedRows` has only the rows that failed,
for large files. With the `xlsx` feature, `report.error_xlsx(…)` makes a workbook. The kit's
report links both CSV files as downloads (as `data:` links, up to 1 MiB each), and an app can
serve its own from a route.

### Duplicate (replicate)

`Model::replicate()` copies a record into one that isn't saved yet: the same values, with no
id, no `deleted_at` and no timestamps. Show it in the "new" form for someone to finish:

```rust
# use renox::prelude::*;
# #[derive(Model, serde::Serialize, Default, Clone)]
# #[model(table = "products")]
# struct Product { id: i64, sku: String, name: String, price: i64 }
/// GET /products/{id}/replicate: the new-product form, filled from a copy.
async fn replicate(State(db): State<Db>, Path(id): Path<i64>) -> Result<View> {
    let original = Product::find_or_404(&db, id).await?;
    let mut product = original.replicate();
    product.sku.clear(); // unique: the person types a new one
    product.name = format!("{} (copy)", original.name);
    Ok(view("products/replicate.html", context! { original, product }))
}
```

The form's fields take their values from it (`input("name", "Name", value=product.name)`) and
post to the ordinary "store" route. Code that saves the copy itself calls `copy.save(&db)`. In a
data grid, it's a row action: `Action::link("Duplicate", "/products/{id}/replicate")`.

### Export

Data grids export what they show ([docs/grid.md](grid.md#exports)). For a file outside a grid's
page, such as one product's ledger from its page, `Grid::export_as` takes any query and a
format. The grid only says which columns: those shown by default, in their order. See
[docs/grid.md](grid.md#exports-outside-the-grids-page).

```html
{{ menu_link(route('products.ledger', product.id), "Export ledger (CSV)", icon="download", download=true) }}
```

The bike shop's stock page has an `action_group`
([stock/index.html](../examples/bikeshop/resources/views/stock/index.html)), and a supplier's page
the `import_action` with its template
([stock/suppliers/show.html](../examples/bikeshop/resources/views/stock/suppliers/show.html)).

## Dashboards

A dashboard shows figures, charts and the period of time they cover. Renox draws them on the
server as plain HTML and SVG (a format for drawings). There's no chart library and nothing
extra for the browser to load.

> [!NOTE]
> **Coming from Laravel:** these play the role of Filament's widgets.

First, the handler collects the numbers:

```rust
use renox::prelude::*;
use renox::chart::{Period, Trend};

/// An order.
#[derive(Model, serde::Serialize, Default)]
struct Order { id: i64, total: i64, status: String, created_at: Option<renox::db::DateTime> }

// `?period=30d`: 7d, 30d, 90d (any number of days up to 366), 12m (months up
// to 36), mtd, ytd; 30 days without one.
/// The dashboard page: sales, sales in the period before, and the number of orders.
async fn dashboard(State(state): State<AppState>, period: Period) -> Result<View> {
    // A query for paid orders, made fresh each time it's needed.
    let paid = || Order::query().where_(Order::STATUS.eq("paid"));
    // The sum of `total` per day (or month) in the chosen period, and in the one before.
    let sales = Trend::of(paid(), "created_at").over(period).sum(&state, "total").await?;
    let before = Trend::of(paid(), "created_at").over(period.previous()).sum(&state, "total").await?;
    // How many paid orders there were per day (or month).
    let orders = Trend::of(paid(), "created_at").over(period).count(&state).await?;
    Ok(view("dashboard.html", context! {
        period,
        revenue => sales.total(),
        change => sales.change_from(&before), // percent; None when the period before was 0
        sales => sales.named("Sales"),
        orders,
    }))
}
```

`Period` reads the period from the address: a preset (`?period=30d`, `12w`, `12m`, `ytd`…) or
a custom range (`?period=custom&from=2026-09-01&to=2026-09-30`, what `period_filter`'s
"Custom" form sends). Then the template draws them:

```html
<rx-period-filter :selected="period"/>
<rx-stats columns="4">
  <rx-stat label="Revenue" :value="revenue | money" :delta="change" :trend="sales.values"/>
  <rx-stat label="Refunds" :value="refunds" :delta="refund_change" good="down"/>
</rx-stats>
<rx-dashboard columns="3">
  <rx-widget title="Sales" description="Paid orders" span="2">
    <rx-chart kind="area" :data="sales" format="money"/>
  </rx-widget>
  <rx-widget title="By status" url="{{ route('dashboard.statuses') }}" :poll="60"/>
  <rx-widget title="Orders" span="full"><rx-chart kind="bar" :data="orders" name="Orders"/></rx-widget>
</rx-dashboard>
```

This page shows, from top to bottom:

- a row of buttons to pick the period;
- a row of figures: the revenue (with its change and a small trend line) and the refunds
  (where going down is good);
- a grid of cards: a sales chart, the orders "by status" (loaded from another URL and
  refreshed every 60 seconds), and a bar chart of orders across the full width.

The parts, one by one:

- **`Trend::of(query, column)`** places a model's rows in time, by a date-time column.
  - It gives a `Series` per day (for up to 92 days, and for `mtd`), per week (for `12w`-style
    periods, and custom ranges up to 26 weeks) or per month. `period.per(Bucket::Week)` picks
    the step yourself: `Period::days(90).per(Bucket::Week)`.
  - It can `count`, `sum(column)` or `average(column)`.
  - The query's conditions apply.
  - Days are cut in `APP_TIMEZONE` (at its offset at the end of the period). Empty days are 0.
  - Weeks are ISO weeks: Monday to Sunday, in those same local days. Each is labelled by its
    Monday (`2026-09-28`, shown as `Sep 28`). `12w` starts on the Monday eleven weeks before
    this week's and ends today; in a custom range the first and last weeks may be cut short
    by the range.
  - `Period::previous()` is the period just before, as long, for `Series::change_from` (a
    percent). For a custom range that's as many days, just before it.
  - `Period::between(from, to)` is a custom range in code (both days included, at most
    three years; `None` otherwise). Its key is `2026-09-01..2026-09-30`, which `?period=`
    takes too. A range the extractor refuses (`from` after `to`, a date that isn't one, more
    than three years) gives the default 30 days.
  - A `Series` has `labels` (`2026-10-02`, the Monday per week, or `2026-10` per month),
    `values`, `total()` and `named(…)`. Build one by hand with `Series::new(labels, values)`.
- **`chart(kind, data, …)`** draws a chart.
  - Kinds: `line`, `area`, `bar` (`stacked=true` piles bars on top of each other),
    `pie`/`doughnut`, `scatter`/`bubble` or `heatmap` (below).
  - `data` is a `Series`, a list of numbers, or a list of series (`{name, values}` maps).
    Or pass `labels=…` with `series=[…]` or `values=[…]`.
  - Options: `format` (`number`, `money` in `APP_CURRENCY` or `currency=…`, `percent`;
    money data is in the smallest unit, cents, like the `money` filter's, or say
    `divide_by=1`),
    `decimals`, `height` (240 px), `title` (for screen readers), `name` (one series' name),
    `x_format` (chrono's codes for date labels; otherwise `Oct 2`, `Oct 2026`),
    `x_title` and `y_title` (shown along the axes), `legend=false`, `table=false`, `id`.
  - Plain numbers get as many decimals as the data needs (up to 2) in tooltips and the
    data table, unless `decimals` says; `money` uses the currency's, `percent` one.
- **Scatter and bubble charts** place points by two numbers (and a bubble's size by a third):
  ```html
  {{ chart("scatter", points=orders, x_title="Items", y_title="Total", format="money") }}
  {{ chart("bubble", points=products, x_title="Price", y_title="Units", size_title="Revenue",
           x_format="money", size_format="money") }}
  ```
  - A point is `{x, y, size, label}` (a map from the handler, e.g. `json!({…})`), or `[x, y]`
    and `[x, y, size]`. A point without both numbers is left out.
  - Several series: `series=[{"name": "Coffee", "points": […]}, …]` (or the same list as
    `data`), each in its own colour with a legend.
  - `format` is for y, `x_format` for x and `size_format` for sizes: `number`, `money` or
    `percent`. Numbers get as many decimals as the data needs (up to 2) unless `decimals` says.
  - The axes fit the data (they start at 0 only when the data comes close to it), with
    grid lines both ways.
  - A bubble's area follows its size (6 to 40 px across); bubbles are see-through with a
    solid edge, and small ones are drawn over big ones.
  - The tooltip names the point (`label`), its series, and each value with its axis title;
    the arrow keys move from point to point, left to right. The "Show the data" table lists
    every point.
- **Heatmaps** shade a grid of cells by value, such as rentals by weekday and hour:
  ```html
  {{ chart("heatmap", columns=hours, rows=["Mon", "Tue", "Wed"], cells=picked_up,
           x_title="Hour", y_title="Weekday", title="Rentals by weekday and hour") }}
  {{ chart("heatmap", columns=["Q1", "Q2"], rows=["Coffee", "Tea"], values=[[4, 7], [0, 3]]) }}
  ```
  - `columns` and `rows` are labels. The values are `values` (one list per row, one number per
    column, `null` for none) or `cells`: `[column, row, value]` or `{x, y, value, label}`
    maps, with `x` and `y` as indexes counting from 0 (so a Monday is row 0). Cells without a
    value stay empty; at most 100 rows and 100 columns.
  - Five shades by a cell's share of the largest value (zero and negative values stay
    empty); the legend runs "Less" to "More". `format` and `decimals` work as on other
    charts.
  - Accessibility: the grid has the `title` as its name, each cell's `title` says
    `Mon 09:00: 5` (a cell's `label` replaces the `Mon 09:00` part), and "Show the data"
    lists every number, so no value is only a shade. There is no script: it works in mails
    and print too.
- **How the charts look.**
  - One axis that starts at 0, with clean tick labels (`12.5K`, or `2,5M` where the language
    writes a decimal comma).
  - A legend for two series or more.
  - A hairline grid; 2 px lines with a dot at the end.
  - Bars at most 24 px wide, with rounded ends.
  - A pie or doughnut shows at most six slices. With more, it keeps the first five and folds
    the rest into one "Other" slice.
  - The six series colours come in a fixed order, checked for colour blindness in both
    light and dark mode (`--rx-chart-1` … `--rx-chart-6`). A seventh series is grey.
- **Hover and keyboard.**
  - On line and area charts, a crosshair and one tooltip with every series at the nearest
    date. On bars and slices, a tooltip for each.
  - The chart can take focus, and the arrow keys, Home and End move along it.
  - Every chart also has a "Show the data" table, so no value is only in a colour or a
    tooltip.
- **`stat(label, value, delta=…, delta_label=…, good="up", trend=…, url=…, hint=…,
  decimals=1, icon=…)`** is one figure.
  - `value` is written as it should read: `total | money`.
  - `delta` is its change in percent, with an arrow and its sign. It's green when it goes the
    `good` way: `"up"`, `"down"` or `"none"`. `decimals` is how many decimals the percent
    shows (1 by default).
  - `trend` draws a sparkline (a tiny line chart); `url` makes it a link.
  - `icon` is one of the kit's icons, in a tinted circle at the card's corner.
  - `stats(columns)` sets stats side by side (two per row on phones).
- **`dashboard(columns)` + `widget(title, description=…, span=…, url=…, poll=…, id=…)`**:
  cards in a grid (one column on phones; `span=2` or `"full"` for wider cards). `id` names the
  widget's body (by default it is made from the title).
  - A widget's content is its call block, or what `url` answers (a small template, or a
    `View` fragment).
  - With `url`, the content is loaded after the page, and again every `poll` seconds. The old
    content stays, dimmed, until the new one arrives.
- **`period_filter(selected, options=…, label=…, custom=true)`**: one row of preset periods,
  over everything it applies to. `selected` is the handler's `Period` (the one now shown).
  `options` are `[key, label]` pairs; by default 7 days, 30 days, 90 days, 12 months and this
  year (`7d`, `30d`, `90d`, `12m`, `ytd`; `12w`, 12 weeks, is another). `label` is the name
  screen readers say ("Period"). Its links set `?period=` and keep the rest of the address's
  query. `query_with(period="7d")` builds such links in any template.
  - Next to the presets, a "Custom" button opens a small form with two of the kit's date
    fields (typed, or picked in the calendar) and "Apply". It sends
    `?period=custom&from=…&to=…`, keeping the rest of the query (`query_fields(…)`, below).
  - The button shows the range while one is chosen (`Sep 1 – Sep 30, 2026`). A range the
    extractor refuses opens the form again with what was typed and a message.
  - It works from the keyboard: Enter on the button opens it and puts the cursor in the first
    date, Tab moves on, Escape (or a click elsewhere) closes it.
  - `custom=false` leaves the button out.
  - `query_fields("period", "from", "to")` writes the current query as hidden inputs, without
    `page` and the keys named, for any GET form that should keep the page's other filters.

The bike shop's reports dashboard ([reports/dashboard.html](../examples/bikeshop/resources/views/reports/dashboard.html))
uses most of it: the period (with a custom range), figures against the period before with
their trend, revenue as stacked bars, the mix of income as a doughnut and rental hours as
a heatmap.

## Data grids

`renox::grid` is a data grid for dashboards and back offices: a big table that works on the
server. It fills its container and has:

- a filter in every column heading, sorting and pagination;
- frozen columns (they stay put when you scroll sideways) and grouped headings;
- columns each user picks, per screen size;
- editing in place, and actions on selected rows;
- summaries, groups and exports.

You define it in Rust and draw it with the `grid` macro of `renox/grid.html`.
[docs/grid.md](grid.md) is its guide, and the bike shop's report grids
([src/app/reports/grids.rs](../examples/bikeshop/src/app/reports/grids.rs)) are built on it.

`{{ sparkline(values) }}` (a small line or bar chart as inline SVG) works in any template,
not only in a grid.

> [!NOTE]
> **Coming from Laravel:** this plays the role of Filament's tables.

## Stacks

A page or a component often needs to add something to another part of the layout: a script
at the end of `<body>`, or a style or a `<meta>` tag in `<head>`.

Stacks solve this. The layout names the places with `stack`. Anything rendered for the page
adds to them with `push`.

The layout:

```html
{# layouts/app.html (rnx new's layout has both) #}
<head>… {{ stack('head') }}</head>
<body>… {{ stack('scripts') }}</body>
```

With tags, the places are `<rx-outlet stack="head"/>` and `<rx-outlet stack="scripts"/>`, and
`<rx-push stack="scripts">` adds to them; write the script as `<rx-script>` and it carries the
CSP nonce by itself.

A component that needs a script:

```html
{# components/chart.html #}
{% macro chart(id, data) -%}
{% call push('scripts', once='chart') %}
  <script src="{{ asset('js/chart.js') }}" nonce="{{ csp_nonce() }}"></script>
{% endcall %}
<canvas id="{{ id }}" data-points="{{ data | tojson }}"></canvas>
{%- endmacro %}
```

The `<canvas>` goes where the component is used. The `<script>` goes to the end of `<body>`,
where the layout has `stack('scripts')`.

The rules:

- `push(name)` adds to the end of the stack, `prepend(name)` to the front.
- `once='key'` adds it only the first time that key is pushed to that stack on the page. So
  the script is added once, however many charts there are.
- Pushes work from the page's blocks, from included templates and from imported components.
- They even reach a stack that was written earlier, such as the one in `<head>`. The layout's
  head is written before the page's blocks run, so `stack` leaves a marker that is filled in
  once the page is done.
- Error pages (`errors/*.html`) have stacks too. Mails don't.

> [!WARNING]
> htmx fragments (`.fragment("rows")`) have no layout, so what they push goes nowhere. Put a
> fragment's script in the fragment itself.

> [!NOTE]
> **Coming from Laravel:** this is `@push('scripts')` / `@stack('scripts')`, `@pushOnce`
> and `@prepend`.

## Tailwind CSS

Tailwind is a CSS tool: you style things with small classes like `mt-4` (margin on top) or
`text-sm` (small text), and it builds a stylesheet with just the classes you used.

`rnx new shop --tailwind` sets it up in a new app. For an existing app, create
`resources/css/app.css`:

```css
@import "tailwindcss";
@source "../views";
```

The first line brings in Tailwind. The second tells it to look for class names in your
templates.

Then link the output in the layout: `<link rel="stylesheet" href="{{ asset('css/app.css') }}">`.

How it runs:

- `rnx serve` runs Tailwind in watch mode next to the app. It rebuilds `public/css/app.css`
  when a view changes, and the page reloads.
- `rnx build` builds it minified (made as small as possible) before compiling, so an
  embedded binary carries it. `rnx tailwind` builds it once (`--minify`, `--watch`).
- There's no Node.js needed. `rnx` downloads Tailwind's standalone program (v4, the version
  `rnx` pins) once into your cache, and checks its SHA-256 (a fingerprint that proves it's
  the right file). `rnx tailwind:install` does that ahead of time.
  `TAILWIND_BIN=/path/to/tailwindcss` uses another binary, and `RNX_CACHE_DIR` moves the
  cache.

> [!IMPORTANT]
> Commit `public/css/app.css` to git. The Dockerfile from `make:deploy` builds the app
> without Tailwind, so it needs the built file.

Tailwind and the kit together:

- The kit's `rx-*` rules sit outside Tailwind's cascade layers (its system for deciding which
  rule wins). So the components keep their look, and utilities (`mt-4`, `text-sm`,
  `md:grid-cols-2`) lay things out around them.
- Tailwind's reset changes bare elements (headings, lists, links without a class) in your own
  markup. Style those with utilities, or with the kit's classes (`rx-title`, `rx-link`).

## Coming from Laravel

If you know Laravel, this table maps what you know to Renox:

| Laravel | Renox |
|---|---|
| Blade components (`<x-input>`) | Macros in `resources/views/components/*.html` (`rnx make:component`) that see `old`, `error`, `t`… |
| Breeze's components | `renox/ui.html` (`rnx make:component --ui` to copy it) |
| `session()->flash('status')` + a toast library | `Toast::success(…)` and `{{ toasts() }}` |
| `@once` | `{% if once('key') %}` |
| `@push('scripts')` / `@stack('scripts')`, `@pushOnce`, `@prepend` | `{% call push('scripts') %}…{% endcall %}` / `{{ stack('scripts') }}`, `push(…, once='key')`, `prepend` |
| Vite + Tailwind | `rnx new --tailwind`: Tailwind's standalone CLI in `rnx serve` / `rnx build` |
| `@fragment` / `fragments([...])` | `.fragment("rows").also("count")` |
| Precognition (live validation) | `data-live-validate` and `Valid<T>` |
| `@class(['tab', 'active' => $on])` | `class_names('tab', {'active': on})` |
| `request()->routeIs('admin.*')` | `route_is('admin.*')` (and `request.route`, the route's name) |
| `@break` / `@continue` in `@foreach` | `{% break %}` / `{% continue %}` |
| `back()` / `redirect()->back()` | the `Back` extractor, returned as the response |
| Filament's tables | `renox::grid` ([docs/grid.md](grid.md)) |
| Filament's ActionGroup, wizard actions | `action_group`, `wizard_action` |
| Filament's ImportAction, ReplicateAction, ExportAction | `renox::import` + `import_action`, `Model::replicate`, `Grid::export_as` |

## No raw MiniJinja

A page, a layout or a partial can be written with tags alone. Expressions stay: `{{ value }}`,
the filters (`money`, `date`, `number`…), `t()`, `route()`, `asset()`, `can()` and an inline
`x if y else z` are not statements. What replaces each `{% … %}` statement:

| Instead of | Write |
|---|---|
| `{% if a %}…{% elif b %}…{% else %}…{% endif %}` | `rx-if`, `rx-elif`, `rx-else` on an element or a component, or on `<template>` for text and several siblings |
| `{% for x in xs %}…{% endfor %}` | `rx-for="x in xs"`, on an element, a component or `<template>`; `loop.index` and the others still work |
| `{% set label = … %}` | `<rx-let name="label" :value="…"/>` for a local alias |
| `<script nonce="{{ csp_nonce() }}">`, `<style nonce="…">` | `<rx-script>`, `<rx-style>` |
| `{% extends %}`, `{% block %}` | `<rx-page layout="…">`, `<rx-outlet name="…"/>` in a layout, `<rx-block name="…">` |
| `{% call push('scripts') %}`, `{{ stack('scripts') }}` | `<rx-push stack="scripts">`, `<rx-outlet stack="scripts"/>` |
| `{% from "renox/ui.html" import card %}` and `{% call card() %}` | the kit's tags, `<rx-card>`; your own components as `<app-…>` |
| `{{ icon("bike") }}`, `{% call columns(2) %}`, `{{ thumbnail(src) }}` | `<rx-icon name="bike"/>`, `<rx-columns count="2">`, `<rx-thumbnail :src="src"/>` |
| `{% call action_group() %}`, `{% call wizard_action(…) %}`, `{{ import_action(…) }}`, `{{ event_stream() }}` | `<rx-action-group>`, `<rx-wizard-action>` with `<rx-wizard-step>`s, `<rx-import-action>`, `<rx-event-stream/>` |
| `{% call(row, column) grid(page) %}` | `<rx-grid :page="page">…</rx-grid>`: `row` and `column` are in scope in its content |
| `{% extends "renox/mail/layout.html" %}` and the mail kit's `button`, `panel`, `table`, `divider` | `<rx-mail>` and `<rx-mail-button>`, `<rx-mail-panel>`, `<rx-mail-table>`, `<rx-mail-divider/>` ([Mails as tags](#mails-as-tags)) |
| `{% for %}` and `{% if %}` in a `.txt` mail | `<rx-mail-text>` with `<template rx-for>` / `<template rx-if>` |
| `{% include "x.html" %}`, `{% include name ignore missing %}` | `<rx-include template="x.html"/>`, `<rx-include :template="name" optional/>` |
| `{{ ' hidden' if x }}`, `{{ ' aria-current="page"' \| safe if x }}` inside a tag | `?hidden="x"`, `?aria-current="'page' if x"` |
| `{% call table(head) %}` with a total row | `<rx-simple-table :head="…">` with `<rx-slot name="footer">` |
| `tabs(id, items)` from data, `link_tabs(items)` built in a loop | `<rx-tabs :items>` with `<rx-tab-panel rx-for>`, `<rx-link-tabs>` with `<rx-link-tab rx-for>` |
| an app macro such as the bike shop's `page_hero` | a component file: `components/hero.html`, used as `<app-hero>` |

A `{% set %}` that adds up a total across a loop (a `namespace`) has no tag on purpose: prepare
that value in Rust, in the handler's data, or with a filter.

Nothing else is left: the `{% include %}` of a name computed at run time, a total row in a hand
written table, a tab bar from data, an attribute that is sometimes there, and the `.txt` version
of a mail all have a tag now (`rx-include`, `rx-simple-table` or the `footer` slot of `rx-table`,
`rx-tabs :items` and `rx-link-tab`, `?attr="…"`, `rx-mail-text`). The bike shop and the guestbook
have no `{% %}` in their views. The one thing that still needs a statement is what has no tag on
purpose: a `{% set %}` that changes across a loop (above) and a macro written by hand for
another template to import.
