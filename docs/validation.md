# Forms and validation

People type all sorts of things into forms: empty names, prices like `abc`, emails without an
`@`. This part of Renox checks what they typed **before** your code uses it, and shows them
friendly error messages when something is wrong.

In Renox, a form is a plain Rust struct. You write down the rules for its fields ("the name is
required", "the price is at least 1000"). If every rule passes, your handler gets the filled-in
struct. If one fails, your handler never runs, and the person sees what to fix.

Want the short version? It's in the [cheat-sheet](../CHEATSHEET.md) ("Form + validation").
Complete, working forms are in:

- [examples/hello](../examples/hello): an upload and translated labels;
- the bike shop's checkout: `#[derive(Validate)]` with hooks, checked live
  ([src/app/sales/checkout.rs](../examples/bikeshop/src/app/sales/checkout.rs));
- the bike shop's `/about/fields`: every input type, files, `each`, `one_of`, `distinct`
  ([src/app/about/fields.rs](../examples/bikeshop/src/app/about/fields.rs)).

### Words you'll meet

| Word | What it means |
|---|---|
| **validation** | Checking that what someone typed is acceptable before you use it. |
| **rule** | One check on one field, like "required" or "at most 100 characters". |
| **handler** | The function a route runs for a request. |
| **extractor** | A handler argument that Renox fills from the request for you. `Valid<T>` is one. |
| **htmx** | A small script (bundled with Renox) that sends forms without reloading the page. |
| **422** | The HTTP status code meaning "I understood you, but the data is wrong". |
| **redirect** | An answer that tells the browser "go to this other page now". A `303` is one. |
| **session** and **flash** | The session is a small store the app keeps per visitor. A *flashed* value lives there for just the next page. |
| **old input** | What the person typed, kept so the form can be filled in again after an error. |
| **hook** | An optional method Renox calls at a fixed moment, like "just before the rules run". |

## A form and its handler

Here is a whole form: the struct, its rules, and the handler that uses it.

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// The fields of the "new product" form. Each field matches an input's `name`.
#[derive(Deserialize, Serialize)]
struct ProductForm {
    name: String,
    price: i64,
    email: Option<String>, // optional: an empty input becomes None
}

impl Validate for ProductForm {
    /// The rules: one line per field.
    fn rules(&self, v: &mut Validator) {
        // Must be filled in, at most 100 characters, and not already used by another product.
        v.field("name", &self.name).required().max(100).unique("products", "name");
        // At least 1000. Error messages call this field "selling price".
        v.field("price", &self.price).label("selling price").min(1_000);
        // If filled in, it must look like an email address.
        v.field("email", &self.email).email();
    }
}

/// Runs only when every rule above passed.
async fn store(session: Session, Valid(form): Valid<ProductForm>) -> Result<Redirect> {
    // Every rule passed.
    let _ = (form.name, form.price, form.email);
    session.flash("status", "Product saved.")?;
    Ok(Redirect::to("/products"))
}
```

What's going on:

- `ProductForm` is the form. `Deserialize` lets Renox fill it from the request.
- `impl Validate` holds the rules, in the `rules` method.
- `Valid(form): Valid<ProductForm>` in the handler is the extractor. It reads the request,
  fills the struct, and checks the rules. Only if all of them pass does `store` run.
- So inside `store`, you can trust `form`: invalid input never reaches your code.

### How `v.field` works

`v.field(name, &value)` starts the rules for one field.

- `name` is the input's name in the HTML form (`<input name="price">`).
- Errors are filed under that name.
- In messages, the field is called by its *label*. By default the label is the name with `_`
  turned into spaces (`first_name` becomes "first name"). `.label(…)` or a translation can give
  it another one (see "Messages and translations").
- `.label(…)` only changes the messages of rules written **after** it: a message is made when
  its rule fails. So call it right after `v.field(…)`. With
  `v.field("price", &self.price).min(1_000).label("selling price")`, a failed `min` still says
  "The price must be at least 1000.". (The derive always puts `label` first for you.)

### The order rules run in

- Rules run in the order you wrote them.
- A field stops at its first failed rule, so it shows one error at a time.
- Most rules **skip a missing value**. "Missing" means `None`, text that is blank once
  spaces are trimmed, or an empty list (`Vec`, `KeyValues`). Only these still run on a missing
  value: `required` and its conditional forms (`required_if`, `required_with`, …), `accepted`
  and `accepted_if`, `declined` and `declined_if` (which fail on it), and `rule`.
- So `v.field("tags", &self.tags).min(1)` passes on an empty list. To ask for at least one
  item, use `required()`.

That last point matters: `.email()` alone accepts an empty field (it's optional).
`.required().email()` doesn't.

## What `Valid<T>` reads

`Valid<T>` looks at the kind of request and reads the data from the right place:

| Request | Read from |
|---|---|
| `GET` / `HEAD` | the query string, the part after `?` in the address (search filters, a report's dates) |
| `application/x-www-form-urlencoded` | the form body (a normal HTML form) |
| `multipart/form-data` | the form body; file inputs become `Upload` fields |
| `application/json` | the JSON body (it must be an object, `{ … }`) |

Some details:

- Any other content type gets a `415` error. Broken JSON gets a `400`.
- A request with a body (a POST, PUT, …) but no `Content-Type` header at all is read as a
  normal form (urlencoded).
- Form bodies are read with `serde_html_form`. When a name appears more than once (a
  multi-select, a group of checkboxes), it fills a `Vec<T>`.
- A form can use nested names like `lines[0][qty]` (the UI kit's `repeater` and `key_value`
  send these). Then it's read as a tree: `lines` fills a `Vec` of a struct. Every field is
  still read from text, as usual (see "Browser values").

### The steps, in order

1. **Parse** the input into `T`. A field that can't be read (`abc` for a number) becomes an
   error on that field (see "Browser values").
2. **`prepare`** tidies the data (for example, trims spaces).
3. **`authorize`** may refuse the request with a `403` ("not allowed"), before any rule runs.
4. **`rules`** run, then the database checks (`unique`, `exists`), all in one pass.
5. **`after`** runs, but only when everything so far passed.
6. **Your handler** runs, if no step added an error.

Steps 2, 3 and 5 are optional hooks. You'll meet them in "Hooks: `prepare`, `authorize`,
`after`".

## When validation fails

The same failure gets one of two answers. Which one depends on who sent the form.

### Answer 1: htmx and JSON requests get a 422

A request counts as htmx or JSON when it has the `HX-Request` header, `Accept:
application/json`, or a JSON body. It gets `422 Unprocessable Entity` with the errors:

```text
{"message": "The name field is required.", "errors": {"name": ["The name field is required."], "price": ["The selling price must be at least 1000."]}}
```

- `message` is the first error, taking the fields in alphabetical order (not page order).
- `errors` lists every field's errors.

The page doesn't reload. Instead, the bundled `renox.js` script (added by `renox_head()`)
puts each error next to its input:

- in the form's element with `data-error-for="field"`, if there is one;
- otherwise in a `<p class="error">` it inserts after the input.

It also sets `aria-invalid="true"` on the wrong inputs (screen readers announce that) and
moves the cursor to the first wrong one, in page order. The form is not replaced or cleared,
so what the person typed stays. Errors on list items (`tags.1`) go to that item's input, or to
the list's slot.

### Answer 2: plain form posts get a redirect back

A normal form post (no htmx, no JSON) gets a `303` redirect back to the form's page. That's
the page in the `Referer` header if it's on the same site, otherwise `/`.

The errors and what was typed are *flashed* to the session, so the form page can show them
once.

> [!IMPORTANT]
> Passwords are never flashed. Every field whose name contains `password` (any case:
> `password`, `password_confirmation`, `current_password`, `new_password`, also inside nested
> rows) is dropped, and so is every field whose name starts with `_` (`_token`, `_method`). So
> they don't sit in the session.

Old input is also capped at about 2 KB, because the session lives in a cookie and browsers drop
cookies over 4 KB. When the form is bigger, the largest values are dropped first, until the
rest fits. So a long text in a `<textarea>` may not be filled in again after an error, while the
short fields are.

In both answers, every field's errors are reported at once, including fields that couldn't be
read.

### Showing errors and old input in templates

Templates get these helpers. Components imported from other files see them too.

| Helper | What it gives |
|---|---|
| `error('name')` | The field's first error, or `""`. For a list, `error('tags')` also shows the first error of an item (`tags.0`, `tags.1`, …). |
| `errors` | Every error: a map of field → list of messages. Empty (counts as false in an `if`) when there are none. |
| `old('name', default)` | What was typed into the field before the redirect, else `default`, else `""`. |
| `has_old()` | Whether the previous request was a failed submit. An unticked checkbox sends nothing, so `old()` alone can't tell "unticked" from "not sent yet". The UI kit's checkboxes and radios use it. |

Here they are in a hand-written form:

```html
<form method="post" action="{{ route('products.store') }}">
  {{ csrf_field() }}
  {# A message at the top when anything is wrong #}
  {% if errors %}<p role="alert">Please fix the fields below.</p>{% endif %}
  <label for="name">Name</label>
  {# Refill with what was typed, else the product's saved name #}
  <input id="name" name="name" value="{{ old('name', product.name) }}"
    {%- if error('name') %} aria-invalid="true"{% endif %}>
  <p class="error" data-error-for="name">{{ error('name') }}</p>
  <button>Save</button>
</form>
```

The `<p data-error-for="name">` slot works for both answers:

- after a redirect, the page fills it when it renders;
- after an htmx post, `renox.js` fills it.

> [!TIP]
> You rarely need to write this by hand. The UI kit's fields (`input`, `select`, …) do all of
> it themselves, and `form_errors()` lists every error at the top of the form with links to the
> fields ([ui.md](ui.md)).

#### Two forms on one page: error bags

Sometimes a page has two plain forms with the same field names. For example, a sign-in form
next to a newsletter form, both with an `email` field. An error on one would show up on both.

The fix is a named *error bag* for one of them: a separate box its errors go into.

```rust
use renox::prelude::*;
use serde::Deserialize;

/// The sign-in form. Its errors go into the "login" bag.
#[derive(Deserialize, Validate)]
#[validate(bag = "login")] // or `const ERROR_BAG: Option<&'static str> = Some("login");` in `impl Validate`
struct SignIn {
    #[validate(required, email)]
    email: String,
}
```

```html
<rx-input name="email" label="Email" bag="login" />       {# the sign-in form #}
<rx-input name="email" label="Your email" />               {# the newsletter form #}
```

What's going on:

- The sign-in form's errors are flashed into the `login` bag.
- `error('email', bag='login')` shows them. `error('email')` (the newsletter form) stays empty.
- `errors_in('login')` lists all of the bag's errors.
- The UI kit's fields take `bag="login"`.
- A `ValidationError` you return yourself goes into a bag with `.in_bag("login")`.

Bags only matter for redirects. htmx and JSON requests get the usual 422.

> [!NOTE]
> **Coming from Laravel:** these are Laravel's named error bags.

### Errors found after validation

Some checks only the handler can make. Say the stock ran out while the person was typing. For
those, return a `ValidationError`. It is answered exactly like a failed rule:

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// An order: which product, and how many.
#[derive(Deserialize, Serialize)]
struct OrderForm {
    product_id: i64,
    quantity: i64,
}

impl Validate for OrderForm {
    fn rules(&self, v: &mut Validator) {
        v.field("quantity", &self.quantity).min(1);
    }
}

/// Places the order, unless there isn't enough stock.
async fn order(State(db): State<Db>, Valid(form): Valid<OrderForm>) -> Result<Redirect> {
    // Ask the database how many are left.
    let stock: i64 = renox::db::sql("SELECT stock FROM products WHERE id = ?")
        .bind(form.product_id)
        .scalar(&db)
        .await?;
    if stock < form.quantity {
        // Build an error on the "quantity" field and send it back like a failed rule.
        let mut errors = Errors::new();
        errors.add("quantity", format!("Only {stock} left."));
        return Err(ValidationError::new(errors).with_input(&form).into());
    }
    Ok(Redirect::to("/cart"))
}
```

`with_input(&form)` fills the form in again with what was typed. Without it, Renox uses the
input that `Valid` read for this request. That's why a `ValidationError` from a model's
`saving` hook also sends the person back to a filled-in form.

## Rules with `impl Validate`

`rules` is plain Rust. You can use `if`, loops and helper functions in it. Each rule is a
method you call on the field:

```rust
use renox::chrono::NaiveDate;
use renox::prelude::*;
use serde::Deserialize;

/// A form to create an event.
#[derive(Deserialize)]
struct EventForm {
    title: String,
    kind: String,
    company: Option<String>,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
    phone: Option<String>,
    email: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    terms: bool,
}

impl Validate for EventForm {
    fn rules(&self, v: &mut Validator) {
        // 3 to 120 characters, with our own message if it isn't.
        v.field("title", &self.title)
            .required()
            .between(3, 120)
            .message("Give the event a title of 3 to 120 characters.");
        // Must be one of these three words.
        v.field("kind", &self.kind).required().one_of(&["public", "private", "company"]);
        // Needed only when the kind is "company".
        v.field("company", &self.company).required_if(self.kind == "company");
        // Can't end before it starts.
        v.field("ends_on", &self.ends_on).after_or_equal(self.starts_on);
        // 9 to 13 digits, starting with "08".
        v.field("phone", &self.phone).digits_between(9, 13).starts_with(&["08"]);
        // Needed when there's no phone number.
        v.field("email", &self.email).required_without(&self.phone).email();
        // At most 5 tags; each one filled in, letters/digits/dashes, at most 20 characters.
        v.field("tags", &self.tags).max(5);
        v.each("tags", &self.tags, |tag| tag.required().alpha_dash().max(20));
        // No tag twice.
        v.distinct("tags", &self.tags);
        // The "I agree" checkbox must be ticked.
        v.field("terms", &self.terms).accepted();
    }
}
```

`.message("…")` replaces the field's error so far, whichever rule made it. Here an empty title
fails `required` (and `between` is then skipped), and still shows the text written for
`between` above.
For `unique`, `exists` and the other checks made later, it replaces the message of the check
just before it. The text is used exactly as written.

### Every rule

First, a note on sizes. `min`, `max`, `between` and `size` measure whatever the value is:

- text: its number of characters;
- a number: its value;
- a `Vec`: its number of items (an empty `Vec` counts as missing, so the size rules skip it;
  add `required()` to ask for at least one);
- an `Upload` (a file): its size in kilobytes.

| Group | Rules |
|---|---|
| Presence | `required()`, `required_if(cond)`, `required_unless(cond)`, `required_with(&other)`, `required_without(&other)`, `required_with_all(&[&a, &b])`, `required_without_all(&[&a, &b])`, `prohibited()`, `prohibited_if(cond)`, `prohibited_unless(cond)` (must be empty), `prohibits("other", &other)` (this one or the other, not both), `accepted()` / `accepted_if(cond)` (a ticked checkbox), `declined()` / `declined_if(cond)` (unticked, or `no`/`off`/`0`/`false`) |
| Size | `min(n)`, `max(n)`, `between(min, max)`, `size(n)` |
| Text | `email()`, `url()` (`http://` or `https://`), `matches(r"^[A-Z]{2}\d{4}$")` (a regex, a text pattern; anchor it with `^` and `$`), `not_matches(pattern)`, `alpha()`, `alpha_num()`, `alpha_dash()`, `ascii()`, `lowercase()`, `uppercase()`, `starts_with(&[…])`, `ends_with(&[…])`, `doesnt_start_with(&[…])`, `doesnt_end_with(&[…])`, `uuid()`, `ulid()`, `ip()`, `mac_address()`, `json()`, `timezone()` (an IANA name such as `Asia/Jakarta`), `hex_color()` (`#4f46e5`) |
| Numbers | `numeric()` and `integer()` (for numbers typed into text fields), `decimal(min, max)` (how many decimal places: `decimal(2, 2)` for `12.50`), `multiple_of(n)`, `min_digits(n)`, `max_digits(n)` |
| Quantity | `quantity(3)` (a decimal typed as `12.5` with at most that many decimals, the scale of a `Quantity<3>`; never rounded, so `12.5001` fails; limits with `quantity_min("0")` / `quantity_max("100")`, which compare the number: `min`/`max` count characters of text), `unit(&table)` (a code the `UnitTable` from `Units::build` knows) |
| Money | `currency()` (an ISO 4217 code such as `USD`), `money(&self.currency)` (an amount typed in whole units, `12.50`, with at most the currency's decimals: JPY 0, USD 2, KWD 3; never rounded) |
| Digits | `digits(n)` (exactly `n` digits, e.g. a PIN), `digits_between(min, max)` (e.g. a phone number) |
| Dates | `date()` (text such as `2026-10-01` or `2026-10-01T10:30`), `before(d)`, `before_or_equal(d)`, `after(d)`, `after_or_equal(d)`, where `d` is a `NaiveDate`, `NaiveDateTime` or `DateTime`, or text in the same forms as `date()` (a limit that isn't a date leaves the rule out) |
| Places | `latitude()` (a number or text such as `25.2048`, from -90 to 90), `longitude()` (from -180 to 180), `point()` (JSON text, `{"lat": 25.2, "lng": 55.3}` or a GeoJSON `Point` with `[lng, lat]`; an out-of-range coordinate gets the `latitude` or `longitude` message). The values are `renox::geo`'s ([types.md](types.md#places-points-circles-and-areas)) |
| Choices | `one_of(&[…])` (Laravel's `in`), `none_of(&[…])` (`not_in`) |
| Other fields | `confirmed(&self.password_confirmation)`, `same("email", &self.email)`, `different("old_email", &self.old_email)`, `gt("min_price", &self.min_price)`, `gte(…)`, `lt(…)`, `lte(…)` |
| Database | `unique(table, column)`, `exists(table, column)`, then `ignore(id)`, `where_eq(column, value)`, `where_null(column)`, `where_not_null(column)` |
| Files | `image()`, `mimes(&["pdf", "jpg"])`, `dimensions(&Dimensions::new().min_width(1200).ratio(3, 1))` (pixels, read from the image's header; `Dimensions` has `min_width`, `max_width`, `min_height`, `max_height`, `width`, `height` and `ratio`), plus the size rules in kilobytes |
| Passwords | `password(&policy)` with a `Password` policy (`.uncompromised()` checks known breaches), `current_password()` (the logged-in user's) |
| Your own | `rule(valid, message)`, `apply(&MyRule)` with a `Rule` |

### Comparing two fields: `gt`, `gte`, `lt`, `lte`

These compare a field with another one ("greater than", "greater or equal", "less than", "less
or equal"). The message names the other field. How they compare depends on the values:

- numbers by value;
- dates by date (real date types, or dates written as text);
- other text by length;
- lists by their number of items;
- files by their size.

Two texts that both read as numbers compare as numbers, so a price kept in a `String` works.
When the other field is empty, the rule is skipped (add `required` to that field if it must be
there).

```rust
# use renox::prelude::*;
use renox::validation::Dimensions;
# struct Promo { min_order: i64, max_discount: i64, starts: String, ends: String, code: Option<String>, gift_card: Option<String>, banner: Option<Upload> }
# impl Validate for Promo {
/// Rules for a promotion.
fn rules(&self, v: &mut Validator) {
    // The discount must be less than the minimum order.
    v.field("max_discount", &self.max_discount).lt("min_order", &self.min_order);
    // The end date must come after the start date.
    v.field("ends", &self.ends).required().date().gt("starts", &self.starts);
    // A promo code or a gift card, not both.
    v.field("code", &self.code).prohibits("gift_card", &self.gift_card);
    // An image, at most 2 MB, at least 1200 pixels wide, three times as wide as it's tall.
    let banner = Dimensions::new().min_width(1200).ratio(3, 1);
    v.field("banner", &self.banner).image().max(2048).dimensions(&banner);
}
# }
```

> [!WARNING]
> `decimal` counts the decimal places as they were typed. So check a price as text: an `f64`
> field has already lost a trailing zero (`12.50` becomes `12.5`).

### Rules on the validator itself

Some rules are called on `v`, not on a field:

- `v.each(name, &list, |item| item.…)`: rules for every item of a list. Errors are filed as
  `name.0`, `name.1`, … and labelled "name #1", "name #2".
- `v.distinct(name, &list)`: no item may appear twice. Text is compared trimmed and in
  lowercase. Every repeat gets the error.
- `v.nested(name, &lines)`: runs each item's own `Validate` rules, for a list of structs. For
  example, the lines of an order sent as JSON, or a form's rows named `lines[0][quantity]`.
  Errors are filed as `lines.0.quantity` and labelled by the item's own field ("The quantity
  field is required.").
- `v.error(field, message)`: adds an error that no rule covers.
- `Validator::new().in_lang(&lang)`: takes messages and field names from the app's language
  file for that language. `Valid<T>` does this for you, in the request's language.

### Which types rules can check

A value a rule can check implements `FieldValue`. These do: `String`, `&str`, the integer and
float types, `bool`, `NaiveDate`, `NaiveDateTime`, `DateTime<Utc>`, `Upload`, `KeyValues`, and
`Option<T>` / `Vec<T>` of those.

### Database rules: `unique` and `exists`

These two rules look in the database:

- `unique` fails when a row **already has** the value (two products can't share a SKU).
- `exists` fails when **no row has** the value (the chosen category must be real).

They run after the other rules, together, and only for fields that passed those. An unknown
table or column is an error (a `500`), not a rule that quietly always passes.

```rust
use renox::prelude::*;
use serde::Deserialize;

/// A product form used both to create and to edit.
#[derive(Deserialize)]
struct ProductForm {
    #[serde(default)]
    id: i64, // a hidden input on the edit form; 0 when creating
    sku: String,
    category_id: i64,
    team_id: i64,
}

impl Validate for ProductForm {
    fn rules(&self, v: &mut Validator) {
        v.field("sku", &self.sku)
            .required()
            .unique("products", "sku")
            .ignore(self.id) // the row being edited doesn't count
            .where_eq("team_id", self.team_id) // unique per team
            .where_null("deleted_at"); // soft-deleted rows don't count
        v.field("category_id", &self.category_id)
            .exists("categories", "id")
            .where_eq("team_id", self.team_id) // not another team's category
            .message("Pick a category.");
    }
}
```

What's going on:

- `.ignore(self.id)` skips the row you're editing. Otherwise saving a product without changing
  its SKU would fail, because the SKU "already exists" (in itself).
- `ignore` takes any key type: `i64`, `Ulid`, `Uuid`, `String`.
- `.where_eq`, `.where_null` and `.where_not_null` narrow which rows count.

> [!WARNING]
> These checks query the table directly. A model's default scope (for example "only this
> tenant's rows", see [authorization.md](authorization.md)) does **not** apply. Say it
> yourself with `where_eq`.

### Passwords

```rust
use renox::prelude::*;
use renox::validation::Password;
use serde::Deserialize;

/// A form to choose a new password, typed twice.
#[derive(Deserialize)]
struct PasswordForm {
    password: String,
    password_confirmation: String,
}

impl Validate for PasswordForm {
    fn rules(&self, v: &mut Validator) {
        // At least 12 characters, with letters, upper and lower case, digits and symbols.
        let policy = Password::min(12).letters().mixed_case().numbers().symbols();
        v.field("password", &self.password)
            .required()
            .password(&policy)
            // Must match the "type it again" field.
            .confirmed(&self.password_confirmation);
    }
}
```

A `Password` *policy* is the list of things a good password needs. Renox's own register, reset
and account pages use the app's policy: `Password::min(8)`, unless
`Auth::new().password_rules(…)` sets another.

### Refusing leaked passwords

`Password::min(12).uncompromised()` also refuses passwords that showed up in known data
breaches (leaks from other websites). It asks the
[Have I Been Pwned](https://haveibeenpwned.com/API/v3#PwnedPasswords) service.

- The password itself never leaves your server. Only the first five characters of its SHA-1
  hash are sent (this trick is called k-anonymity), through `state.http`.
- It runs once the other rules pass.
- When the service can't be reached, the password is allowed and a warning is logged, so
  sign-ups keep working.
- In tests, answer it with `app.fake_http()` (see `parity_more.rs` in Renox's tests for an
  example).

### Checking the current password

`current_password()` checks a field against the logged-in user's password. Use it before a
risky change, like a new email address. It fails when nobody is logged in.

```rust
use renox::prelude::*;
use serde::Deserialize;

/// Changing the email address needs the current password.
#[derive(Deserialize, Validate)]
struct ChangeEmail {
    #[validate(required, email)]
    email: String,
    #[validate(required, current_password)]
    current_password: String,
}
```

> [!NOTE]
> **Coming from Laravel:** this is Laravel's `current_password` rule.

`uncompromised` and `current_password` both run with the database checks, after the other
rules. `Valid<T>` gives them the user and the HTTP client.

Outside a request, `Validator::finish_for(&state, Some(&user))` does the same. `finish(&db)`
can't: it has no user, so `current_password` fails, and the breach check is skipped.

### Your own rules

For a one-off check, use `rule(valid, message)`: pass `true` when the value is fine, plus the
message to show when it isn't.

```rust
# use renox::prelude::*;
# struct Order { quantity: i64 }
# impl Validate for Order {
/// Eggs come in boxes of six.
fn rules(&self, v: &mut Validator) {
    v.field("quantity", &self.quantity)
        .min(1)
        // Fails unless the quantity divides by 6.
        .rule(self.quantity % 6 == 0, "Eggs are sold by the half dozen.");
}
# }
```

Unlike the other rules, `rule` also runs on a missing value. So it can say "required when …"
too.

### A rule many forms share

A rule used by several forms implements the `Rule` trait. `rnx make:rule TaxId --module
invoices` writes one for you.

- Its message may use `:attribute`, which becomes the field's label.
- It is skipped for missing values.

```rust
use renox::prelude::*;
use renox::validation::{Inspected, Rule};

/// A tax ID: 15 or 16 digits.
struct TaxId;

impl Rule for TaxId {
    /// `Ok(())` means the value is fine; `Err(message)` means it isn't.
    fn check(&self, value: &Inspected) -> std::result::Result<(), String> {
        // Only text is checked; anything else passes.
        let Inspected::Text(text) = value else { return Ok(()) };
        // Count only the digits; anything else (spaces, dashes) is ignored.
        match text.chars().filter(char::is_ascii_digit).count() {
            15 | 16 => Ok(()),
            _ => Err("The :attribute must be a valid tax ID.".into()),
        }
    }
}

# struct Form { tax_id: String }
# impl Validate for Form {
/// Use the shared rule with `apply`.
fn rules(&self, v: &mut Validator) {
    v.field("tax_id", &self.tax_id).required().apply(&TaxId);
}
# }
```

A check that needs the database, or several fields at once, goes in the `after` hook (see the
next sections).

## `#[derive(Validate)]`

If a form only needs rules, you can skip `impl Validate` and write the rules as attributes on
the fields. Each item becomes a call on the field's rules, in order:

- `required` becomes `.required()`;
- `max = 100` becomes `.max(100)`;
- `unique("users", "email")` becomes `.unique("users", "email")`.

So every rule above works here. The arguments are Rust expressions, and they may use `self`.

```rust
use renox::prelude::*;
use serde::Deserialize;

/// A sign-up form, with its rules written as attributes.
#[derive(Deserialize, Validate)]
struct Signup {
    #[validate(required, max = 100, label = "Full name")]
    name: String,
    #[validate(required, email, unique("users", "email"))]
    email: String,
    #[validate(required, min = 8, confirmed(&self.password_confirmation))]
    password: String,
    password_confirmation: String,
    #[serde(default)]
    #[validate(max = 5, each(required, max = 20), distinct)]
    tags: Vec<String>,
    #[serde(rename = "t-shirt")]
    #[validate(rename = "t-shirt", one_of(&["S", "M", "L"]), message = "Pick a size.")]
    size: String,
    #[validate(image, max = 2048)] // kilobytes; the content is checked, not the name
    photo: Option<Upload>,
}
```

| Attribute item | Becomes |
|---|---|
| `required`, `email`, `image`, … | `.required()`, `.email()`, … |
| `max = 100`, `label = "Full name"`, `message = "…"` | `.max(100)`, `.label("Full name")`, `.message("…")` |
| `between(3, 280)`, `unique("users", "email")`, `confirmed(&self.x)` | the same call with those arguments |
| `label = "…"` | always applied first, wherever it's written; it names the field's own errors only, while `each` and `distinct` errors keep the default label ("tags #1") |
| `each(rule, …)` | `v.each(name, &self.field, \|item\| item.rule()…)` |
| `distinct` | `v.distinct(name, &self.field)` |
| `rename = "t-shirt"` | the name errors are filed under, when the form's input name differs from the field's (pair it with `#[serde(rename)]`) |
| `#[validate(hooks)]` on the struct | forwards `prepare`, `authorize` and `after` to `impl ValidateHooks` |

Fields without a `#[validate]` attribute have no rules.

A rule may depend on another field, since the arguments can use `self`:
`confirmed(&self.password_confirmation)`, `required_if(self.kind == "company")`. What the
attributes can't say is `nested` or a loop. For those, write `impl Validate` by hand.

## Forms from models

When a form mirrors a model field for field, let the model make it. `#[model(form)]` on a
`#[derive(Model)]` struct generates `<Model>Form` (with the model's visibility): it derives
`Deserialize`, `Serialize` and `Validate`, so it also works as a renox-admin `Form`. Adding a
column then means editing the model only.

- Every field is included except `id`, `created_at`, `updated_at` and `deleted_at`; fields keep
  their type. `#[form(skip)]` leaves one out.
- `#[form(validate(required, max = 100))]` copies its items as `#[validate(...)]`. Nothing is
  guessed from the types. `bool` fields get `#[serde(default)]`.
- `#[form(upload)]` on a `String` or `Option<String>` column makes the form field an
  `Option<Upload>`, left out of `fill`: the handler stores the file and sets the column.
- `form.fill(&mut record)` assigns every non-upload field.
- A `#[model(money)]` field (`Money` or `Option<Money>`) becomes a text field with the amount in
  whole units (`12.50`) plus a text currency field (`<field>_currency`, or the shared column's
  name, once). They are checked together (`required`, `money(&self.<currency>)`, `currency`) and
  `fill` builds the `Money`; an empty `Option<Money>` amount clears it. Show it with
  `<rx-input type="money" currency="…"/>`.
- A `Quantity<S>` or `Option<Quantity<S>>` field becomes a text field checked with `quantity(S)`
  (`required` unless it is an `Option`); `fill` parses it exactly and an empty `Option` clears it.
  Show it with `<rx-input type="quantity" scale="3" unit="kg"/>`.
- Nested names (`lines[0][qty]`) work through the field types, such as `Json<Vec<Line>>` and
  `KeyValues`.
- `#[form(...)]` without `#[model(form)]` is a compile error.

```rust
use renox::prelude::*;
use serde::Serialize;

/// A brand. `BrandForm` is generated from it.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "brands", form)]
pub struct Brand {
    pub id: i64,
    #[form(validate(required, max = 100))]
    pub name: String,
    #[form(validate(url, max = 255))]
    pub website: Option<String>,
    #[form(upload)]
    pub logo: Option<String>,
    #[form(skip)]
    pub internal_note: String,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

async fn update(Valid(form): Valid<BrandForm>) -> Result<()> {
    let mut brand = Brand::default();
    form.fill(&mut brand);
    Ok(())
}
```

Write the form by hand (`#[derive(Validate)]` or `impl Validate`) when it needs hooks
(`prepare`, `authorize`, `after`) or differs from the model.

## Hooks: `prepare`, `authorize`, `after`

A form can have three optional methods around its rules. They run at fixed moments:

| Hook | When it runs | What it's for |
|---|---|---|
| `prepare` | first | tidy the input (trim spaces, lowercase an email) |
| `authorize` | before the rules | refuse the request (a `403`) if this person isn't allowed |
| `after` | only when the rules passed | checks that need the database or several fields |

> [!NOTE]
> **Coming from Laravel:** a form with these hooks is what Laravel calls a *form request*.

```rust
use renox::prelude::*;
use renox::validation::FormContext;
use serde::Deserialize;

/// Invite someone to a team.
#[derive(Deserialize)]
struct Invite {
    email: String,
    team_id: i64,
}

impl Validate for Invite {
    // First: tidy the input. The form refilled after an error still shows what was typed.
    fn prepare(&mut self) {
        self.email = self.email.trim().to_lowercase();
    }

    // `false` answers 403 before any rule runs.
    async fn authorize(&self, form: &FormContext<'_>) -> Result<bool> {
        // Only a logged-in team owner may invite.
        Ok(form.user.is_some_and(|user| user.has_role("owner")))
    }

    fn rules(&self, v: &mut Validator) {
        v.field("email", &self.email).required().email();
    }

    // Only when the rules passed: checks that need the database or several fields.
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result {
        // Count the team's members.
        let members: i64 = renox::db::sql("SELECT COUNT(*) FROM team_user WHERE team_id = ?")
            .bind(self.team_id)
            .scalar(&form.state.db)
            .await?;
        if members >= 10 {
            // Shown like any rule's error.
            errors.add("email", "This team is full (10 members).");
        }
        Ok(())
    }
}
```

`FormContext` gives the hooks what they need about the request:

- `state`: the app's state (database, mailer, …);
- `user`: the logged-in user, an `Option<&User>`;
- `method` and `path`: the request's method and address.

Errors added in `after` are shown like any rule's.

### Hooks with the derive

With `#[derive(Validate)]`, add `#[validate(hooks)]` on the struct. Then implement only the
hooks you need, on `ValidateHooks`:

```rust
use renox::prelude::*;
use renox::validation::ValidateHooks;
use serde::{Deserialize, Serialize};

/// Rules as attributes, plus a `prepare` hook.
#[derive(Deserialize, Serialize, Validate)]
#[validate(hooks)]
struct ProductForm {
    #[validate(required, max = 100)]
    name: String,
    #[validate(min = 0)]
    price: i64,
}

impl ValidateHooks for ProductForm {
    /// "  Iced   Coffee " is checked and saved as "Iced Coffee".
    fn prepare(&mut self) {
        self.name = self.name.split_whitespace().collect::<Vec<_>>().join(" ");
    }
}
```

## Browser values

Browsers send everything as text, and some inputs send odd things. Before the rules run,
`Valid` smooths that over, as described below.

> [!WARNING]
> Only `Valid` does this. The prelude's `Form<T>` is axum's, and it reads the body as it is.
> So with `Form<T>`, a ticked checkbox's `on` in a `bool` field answers `422`.
> A form with no rules (a toggle, a filter, a settings switch) should still use `Valid<T>`,
> with `#[derive(Deserialize, Validate)]` and no `#[validate]` attributes.

- **Empty inputs count as missing**, as in Laravel.
  - An `Option<T>` field left empty is `None`.
  - A `String` left empty is `""`, and only `required` complains about it.
  - A number, date or other non-text field left empty gets "The … field is required.". Make
    it an `Option<T>` if it's optional.
- **Checkboxes:** a ticked box sends `on`; an unticked one sends nothing at all. A `bool` field
  reads `on`, `1`, `yes`, `checked` and `true` as `true`. It reads `off`, `0`, `no`, `false`,
  empty or missing as `false`.
- **`datetime-local`** inputs send `2026-10-01T10:30`, without seconds. A `NaiveDateTime`
  field accepts it.
- **Lists:** a multi-select or a group of checkboxes repeats its name. Declare the field as
  `Vec<T>` with `#[serde(default)]`, so that "nothing chosen" is an empty list. A name ending
  in `[]` (`tags[]`) is a list even when it's sent only once, so it never fills a text field.
- **Rows:** `lines[0][name]=Coffee&lines[0][qty]=2&lines[1][name]=Tea…` reads into
  `lines: Vec<Line>`. `meta[0][key]`/`meta[0][value]` reads into a `renox::KeyValues` (ordered
  pairs; rows without a key are skipped).
  - Empty values stay, so rows keep their numbers and their errors (`lines.1.name`). An
    `Option` reads `""` as `None`.
  - `error()` and `old()` in templates take either spelling: `lines[1][name]` or
    `lines.1.name`.
  - Names nested deeper than 32 levels are ignored.
- **Values that can't be read** (`price=abc` for an `i64`, `size=huge` for an enum) become an
  error on that field ("The price must be a number.", "The size is invalid."),
  instead of a `400` for the whole request.
  - A stand-in value (an enum's first variant, `0`, `false`) takes its place, so the rest of
    the form can still be read and every other field's rules run.
  - Rule errors on the stand-in itself are dropped.
  - JSON bodies get the same treatment.

Which Rust type to use for each HTML input, and which SQLite and PostgreSQL column goes with
it, is in [types.md](types.md).

## Messages and translations

Error messages come in the request's language. Renox picks it in this order: the session's
locale, then the browser's `Accept-Language` header (with `App::detect_locale`), else
`APP_LOCALE`. Your own middleware (`App::layer`) can read the choice from the request's
`renox::i18n::RequestLocale` extension and change it for this request with
`RequestLocale::switch(&mut req, "es")`, for example to use a language saved on the account.

Renox ships English messages. For another language, the app translates them in its own
language file (for example `resources/lang/es.json`). Anything it leaves out stays English.

### Placeholders in messages

A message template can use these placeholders:

- `:attribute`: the field's label;
- `:Attribute`: the same, with a capital first letter;
- the rule's numbers or values: `:min`, `:max`, `:size`, `:digits`, `:date`, `:values`,
  `:other`, `:value` (`multiple_of`), `:decimal` (`decimal`) and `:seconds` (`auth.throttle`).

### Overriding messages and field names

In its language files (`resources/lang/<locale>.json`), the app can:

- replace any built-in message, under `renox.validation.<key>`;
- name fields, under `renox.validation.attributes.<field>`.

```text
{
  "renox": {
    "validation": {
      "required": "El campo :attribute es obligatorio.",
      "min.string": ":Attribute debe tener al menos :min caracteres.",
      "attributes": { "name": "nombre", "message": "mensaje", "photo": "foto" }
    }
  }
}
```

The keys:

| Keys | Rule |
|---|---|
| `required`, `accepted`, `prohibited` | presence rules (`required_if`, `required_with`… use `required`) |
| `min.string`, `min.numeric`, `min.array`, `min.file` (and the same for `max`, `between`, `size`) | size rules, by what is measured |
| `email`, `url`, `regex`, `alpha`, `alpha_num`, `alpha_dash`, `lowercase`, `uppercase`, `starts_with`, `ends_with`, `uuid`, `ip` | text rules (`regex` is `matches`) |
| `numeric`, `integer`, `decimal`, `multiple_of`, `digits`, `digits_between`, `min_digits`, `max_digits` | number and digit rules |
| `gt.numeric`, `gt.string`, `gt.array`, `gt.file`, `gt.date` (and the same for `gte`, `lt`, `lte`) | comparisons with another field (`:other` is its label) |
| `json`, `ulid`, `timezone`, `mac_address`, `ascii`, `hex_color`, `doesnt_start_with`, `doesnt_end_with`, `not_regex` | more text rules (`not_regex` is `not_matches`) |
| `latitude`, `longitude`, `point` | place rules |
| `prohibits`, `declined` | presence rules |
| `dimensions` | an image's size in pixels |
| `date`, `before`, `before_or_equal`, `after`, `after_or_equal` | date rules |
| `in`, `not_in` | `one_of`, `none_of` |
| `confirmed`, `same`, `different`, `distinct` | rules across fields |
| `unique`, `exists` | database rules |
| `file`, `image`, `mimes` | uploads |
| `min.string`, `password.letters`, `password.mixed`, `password.numbers`, `password.symbols`, `password.uncompromised` | the `Password` policy (its minimum length uses `min.string`) |
| `invalid` | a value that can't be read, of no type above |
| `auth.failed`, `auth.throttle`, `current_password` | Renox's login and account pages |

### Which label a message uses

The first one that exists wins:

1. `.label(…)` (or `label = "…"` with the derive);
2. the app's `renox.validation.attributes.<field>`;
3. for a nested field such as `items.0.name`: the app's `renox.validation.attributes.items.*.name`
   (numbers become `*`), then the attribute for its last part (`name`);
4. the field name (for a nested field, its last part), with `_` turned into spaces.

Labels on Renox's own forms give way to the app's translations.

> [!NOTE]
> `.message(…)` and `rule(…)` messages are used exactly as written; they are not translated.
> If you need them in several languages, translate them in Rust with the `Lang` extractor.

## Live validation

Live validation checks each field while the person fills in the form, not only when they
press Save.

Add `data-live-validate` to a form that `Valid<T>` handles. The UI kit's script then checks
each field when the person leaves it, and again as they correct it.

How it works:

- The script posts the form with the header `X-Renox-Validate: <field>`.
- `Valid` answers `200 {"field": …, "errors": […]}` with just that field's errors.
- **Your handler doesn't run**, so nothing is saved.
- The rules are the form's own, `unique` included. The `after` hook runs only when the whole
  form has no errors, so it may not run while other fields are still empty or wrong.

See [ui.md](ui.md), "Live validation".

> [!NOTE]
> **Coming from Laravel:** this plays the part of Laravel Precognition.

## Uploads

A file input is a form field of type `Upload`. The form must be sent as `multipart/form-data`
(`<form enctype="multipart/form-data">`). Files go through the same rules and the same error
handling as text fields:

```rust
use renox::prelude::*;
use serde::Deserialize;

/// A form with one optional photo and up to five scanned documents.
#[derive(Deserialize)]
struct EntryForm {
    name: String,
    photo: Option<Upload>, // <input type="file" name="photo">; no file chosen → None
    #[serde(default)]
    scans: Vec<Upload>, // <input type="file" name="scans" multiple>
}

impl Validate for EntryForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required().max(50);
        // Must really be an image, at most 2 MB.
        v.field("photo", &self.photo).image().max(2048); // KB
        // At most 5 files; each a PDF, JPEG or PNG of at most 10 MB.
        v.field("scans", &self.scans).max(5);
        v.each("scans", &self.scans, |scan| scan.mimes(&["pdf", "jpg", "png"]).max(10_240));
    }
}

/// Saves the files once every rule passed.
async fn store(State(state): State<AppState>, Valid(form): Valid<EntryForm>) -> Result<Redirect> {
    if let Some(photo) = &form.photo {
        // Public: anyone with the link can see it.
        let key = photo.store_public(&state.storage, "entries").await?; // public/entries/<random>.jpg
        let _ = (key, &form.name);
    }
    for scan in &form.scans {
        scan.store(&state.storage, "scans").await?; // private
    }
    Ok(Redirect::to("/"))
}
```

Some things to know:

- `image()` and `mimes()` look at the file's **content**, not its name, for the formats Renox
  recognizes: PNG, JPEG, GIF, WebP and PDF. (For other types, the name decides.) So a text file
  renamed to `.png` is refused.
- Stored files get a random name, with the extension that matches their content.
- The size of the whole request is capped by `UPLOAD_MAX_SIZE`. Above it, the answer is a
  `413` ("too large").

### Steps before storing: scanning, images, encryption

Rules say whether a file is acceptable. Some jobs belong between receiving a file and storing it
instead: a malware scan, shrinking a photo, dropping its GPS position, encrypting an identity
document. Give the app an upload pipeline once, and every form that takes an `Upload` gets the
same treatment:

```rust
use renox::prelude::*;
use renox::upload::{ClamAv, Pipeline, StepOutcome, UploadContext, UploadStep};
use renox::upload::BoxFuture;
use std::time::Duration;

/// Your own step: any `impl UploadStep`.
struct NoExecutables;

impl UploadStep for NoExecutables {
    fn process<'a>(
        &'a self,
        file: &'a mut Upload,
        ctx: &'a UploadContext,
    ) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            Ok(if file.bytes().starts_with(b"MZ") {
                // Becomes an error on the field the file came from (`ctx.field()`).
                StepOutcome::Reject("Programs can't be uploaded.".into())
            } else {
                StepOutcome::Keep
            })
        })
    }
}

fn app() -> App {
    let uploads = Pipeline::new()
        .step(ClamAv::tcp("127.0.0.1:3310").timeout(Duration::from_secs(20)))
        .step(NoExecutables);
    # #[cfg(feature = "images")]
    let uploads = uploads.step(
        renox::upload::images::Images::new() // feature `images`
            .max_dimensions(2560, 2560)
            .strip_metadata()
            .thumbnail("thumb", 320, 320),
    );
    App::new().uploads(uploads)
}

/// An identity document: encrypted at rest, read back decrypted.
async fn keep(state: AppState, id_card: Upload) -> Result<Vec<u8>> {
    let key = id_card.store_encrypted(&state.storage, "ids").await?;
    Ok(state.storage.get_decrypted(&key).await?.unwrap_or_default().to_vec())
}
```

- **When it runs.** `Valid<T>` runs the steps on every file of a multipart form **after** the
  form's rules (and `after`) passed, so a file the rules refuse is never scanned. A step that
  returns `StepOutcome::Reject(message)` puts the message on the form field the file came from
  (`ctx.field()`: `photo`, `scans`, `documents.0.file`): a plain post is sent back with the error
  and old input, an HTMX or JSON request gets a `422`, like any rule. The handler receives the
  files as the steps left them. Live validation (`data-live-validate`) does not run the steps.
- **Outside a form**, call `file.process(&state).await?` (a refusal is a validation error on
  `file`; `process_field` names another field). The pipeline is `App::uploads`; there is one
  per app.
- **Writing a step.** The trait is object-safe (`BoxFuture`, as Renox's other callbacks). Change
  the file with `file.set_bytes(..)`, `set_content_type`, `set_file_name`; return
  `StepOutcome::Derived(name, upload)` (or call `file.derive(name, upload)`) for an extra file such
  as a thumbnail. `store`, `store_public` and `store_encrypted` write derived files next to the
  original, at `Upload::derived_key(&key, "thumb")` (`public/products/abc.jpg` becomes
  `public/products/abc.thumb.jpg`). An `Err` from a step is a 500; a step that must fail closed
  returns `Reject` instead.
- **`ClamAv`** streams the file to `clamd` (`INSTREAM`, over TCP or `ClamAv::unix(path)`); no
  library is linked. It **fails closed**: a daemon that is down, too slow (10 seconds unless
  `timeout`) or answers with an error refuses the file with "could not be checked for malware",
  and logs why. `.fail_open()` lets such files through. Setting up the daemon:
  [Operations](operations.md#scanning-uploads-with-clamav).
- **`Images`** (feature `images`, pure Rust, JPEG, PNG and WebP): `max_dimensions` shrinks (never
  enlarges), the EXIF orientation is applied to the pixels, `strip_metadata()` re-encodes so no
  EXIF, GPS position or comment is left, `thumbnail(name, w, h)` derives a smaller copy in the
  same format. A file that claims to be an image but can't be decoded, or is over 16,384 pixels a
  side or 100 megapixels, is refused. Other types pass through untouched, so keep `image()` in the
  rules. **HEIC and AVIF are not decoded**: write an `UploadStep` that calls an external converter
  (`heif-convert`, `magick`, or a service) and sets the result with `set_bytes`,
  `set_content_type("image/jpeg")` and `set_file_name("photo.jpg")`; put it before `Images`.
- **Encrypted files.** `store_encrypted` (and `storage.put_encrypted(key, bytes)`) seal each file
  with AES-256-GCM under a fresh data key, which is itself sealed with a key derived from
  `APP_KEY` and kept in the file's header (`RNXE`, a version, the sealed key, the nonce, then the
  ciphertext). `storage.get_decrypted(&key)` and `Download::decrypted(&storage, &key, name)` read
  them. A local disk's `temporary_url` and `/_renox/files` decrypt on the fly for the request
  that holds the signature. On S3 a presigned link serves what is stored, the ciphertext: send
  the file through your own route with `Download::decrypted`. Keep encrypted files out of
  `public/` (`/storage` serves files as stored). Rotating `APP_KEY`:
  [Operations](operations.md#encrypted-files-and-rotating-app_key).

## Validating outside a request

Sometimes you want to check data that didn't come from a form, for example in a command that
imports rows from a file. `Validator::rules_of` runs a form's rules anywhere. `finish` then
runs the database checks and returns the errors.

```rust
# use renox::prelude::*;
# #[derive(serde::Deserialize, Validate)]
# struct ProductForm { #[validate(required)] name: String }
# async fn demo(db: Db, form: ProductForm) -> Result {
// Run the rules and the database checks, and collect the errors.
let errors = Validator::rules_of(&form).finish(&db).await?;
// The first error on "name", if there is one.
if let Some(message) = errors.first("name") {
    eprintln!("skipped: {message}");
}
# Ok(()) }
```

> [!IMPORTANT]
> Here the hooks `prepare`, `authorize` and `after` are **not** called. Only the rules and the
> database checks run.

## Testing validation

`assert_invalid("field")` expects a `422` with an error on that field. Only htmx and JSON
requests get a 422, so send the request with `app.htmx()` or `app.request().json()` (or
`post_json`). A plain post is answered with a `303` redirect back to the form instead. More in
[testing.md](testing.md).

```rust
use renox::prelude::*;
use renox::testing::TestApp;
use serde::Deserialize;

/// The form under test.
#[derive(Deserialize, Validate)]
struct ProductForm {
    #[validate(required, max = 100)]
    name: String,
    #[validate(min = 0)]
    price: i64,
}

/// The handler under test.
async fn store(Valid(form): Valid<ProductForm>) -> Result<Redirect> {
    let _ = (form.name, form.price);
    Ok(Redirect::to("/products"))
}

/// A tiny module with one route, so the test has something to post to.
struct Products;

impl Module for Products {
    fn name(&self) -> &'static str {
        "products"
    }

    fn routes(&self) -> Routes {
        Routes::new().post("/products", store)
    }
}

/// Bad input is refused; good input is saved.
#[renox::test]
async fn invalid_products_are_refused() {
    let app = TestApp::new(App::new().module(Products)).await;
    // An htmx post: empty name and a price that isn't a number give two errors.
    app.htmx()
        .post("/products", &[("name", ""), ("price", "abc")])
        .await
        .assert_invalid("name")
        .assert_invalid("price");
    // A JSON post: a negative price.
    app.post_json("/products", &json!({ "name": "Coffee", "price": -1 }))
        .await
        .assert_invalid("price");
    // A plain post that fails is redirected back (303).
    app.post("/products", &[("name", ""), ("price", "1")]).await.assert_status(303);
    // A plain post that passes reaches the handler.
    app.post("/products", &[("name", "Coffee"), ("price", "9000")])
        .await
        .assert_redirect("/products");
}
```

## Coming from Laravel

If you know Laravel, this table maps its validation features to Renox's:

| Laravel | Renox |
|---|---|
| `$request->validate([...])`, `FormRequest` | `Valid<T>` with `impl Validate` or `#[derive(Validate)]` |
| `'name' => 'required\|max:100'` | `v.field("name", &self.name).required().max(100)` or `#[validate(required, max = 100)]` |
| `prepareForValidation`, `authorize`, `after` / `withValidator` | `prepare`, `authorize`, `after` (`ValidateHooks` with the derive) |
| `in:a,b`, `not_in`, `regex` | `one_of(&[…])`, `none_of(&[…])`, `matches(r"…")` |
| `Rule::unique('users')->ignore($id)->where(...)` | `.unique("users", "email").ignore(id).where_eq(col, value)` |
| `exists:categories,id` | `.exists("categories", "id")` |
| `tags.*` rules, `distinct` | `v.each("tags", …)`, `v.distinct("tags", …)` |
| `Password::min(8)->mixedCase()->numbers()->symbols()` | `Password::min(8).mixed_case().numbers().symbols()` |
| `->uncompromised()`, `current_password` | `.uncompromised()` on the policy, `.current_password()` |
| Named error bags (`validateWithBag`, `@error('email', 'login')`) | `#[validate(bag = "login")]`, `error('email', bag='login')` |
| `Rule` classes, closures, `make:rule` | `impl Rule` + `apply`, `rule(valid, message)`, `rnx make:rule` |
| `nullable` | an `Option<T>` field |
| `messages()`, `attributes()` | `.message(…)`, `.label(…)`; `renox.validation.*` in lang files |
| `ValidationException::withMessages([...])` | `Err(ValidationError::new(errors).into())` |
| `@error('name')`, `old('name')`, `$errors` | `error('name')`, `old('name')`, `errors` (`errors_in('bag')`) |
| Precognition | `data-live-validate` |
| `assertInvalid`, `assertSessionHasErrors` | `assert_invalid` (htmx/JSON); `assert_status(303)` for plain posts |
