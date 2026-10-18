# PDF documents

An invoice, a receipt, a certificate: sometimes the page has to be a file. Renox's `renox-pdf`
crate makes PDFs from [Typst](https://typst.app) templates. The Typst compiler runs inside your
app's process (it is a Rust library), so there is no headless browser to install and no separate
service to run. Text is shaped properly, so Arabic and other right-to-left languages come out
right, and the fonts are embedded in the file.

Most apps don't make PDFs, so this is its own crate and adds nothing to `renox`. (A table on
screen can still be printed from the browser with the grid's print page. This crate is for a file
you send, store or attach.)

In this guide:

- [Add it to your app](#add-it-to-your-app)
- [A template and a render](#a-template-and-a-render)
- [Sending it: download or mail](#sending-it-download-or-mail)
- [The kit](#the-kit)
- [Right-to-left and fonts](#right-to-left-and-fonts)
- [Images and other files](#images-and-other-files)
- [Templates in development and in production](#templates-in-development-and-in-production)
- [Limits](#limits)
- [When something goes wrong](#when-something-goes-wrong)
- [Testing](#testing)

### Words you'll meet

| Word | What it means |
|---|---|
| **template** | A `.typ` file in `resources/views/pdf`: the text of the document with a few `#` commands in it. |
| **Typst** | The markup language of the template. `= Title` is a heading, `#data.name` shows a value. Its docs are at typst.app/docs. |
| **kit** | Helpers every template can import: colours, `money()`, `date()`, a table and a page header. |
| **glyph** | The picture of one letter in a font. A character no font has a glyph for would print as an empty box. |

## Add it to your app

```toml
[dependencies]
renox-pdf = "1"
```

Register the module once. It carries the fonts and limits every PDF shares:

```rust
use renox::prelude::*;
use renox_pdf::Pdf;

let app = App::new().module(Pdf::new());
```

`rnx make:pdf invoice` writes a starter template, `resources/views/pdf/invoice.typ`, that already
renders: a header, a table of lines and a total.

## A template and a render

A template is a Typst file. The values you pass in are the dictionary `data`:

```typ
#import "@renox/kit:0.1.0": *
#show: base

#page-header("Invoice", meta: (("Number", data.number),))

Thank you, #data.customer.
```

`Pdf::document(&state, "invoice")` reads `resources/views/pdf/invoice.typ` (a name like
`receipts/sale` reads `receipts/sale.typ`). `.data(&value)` takes anything `Serialize`, and
`.render(&state).await` gives the bytes of the PDF. This example writes its template first so it
is complete:

```rust
use renox::prelude::*;
use renox::serde_json::json;
use renox::testing::TestApp;
use renox_pdf::Pdf;
use renox_pdf::testing::text_of;

# tokio::runtime::Runtime::new().unwrap().block_on(async {
# let dir = std::env::temp_dir().join(format!("renox-pdf-guide-{}", std::process::id()));
# std::fs::create_dir_all(dir.join("pdf")).unwrap();
# std::fs::write(
#     dir.join("pdf/invoice.typ"),
#     "#import \"@renox/kit:0.1.0\": *\n#show: base\n#page-header(\"Invoice\", meta: ((\"Number\", data.number),))\nThank you, #data.customer.\n",
# ).unwrap();
# let views = dir.clone();
# let app = TestApp::with_config(App::new().module(Pdf::new()), move |c| c.views_path = views.clone()).await;
let pdf: Vec<u8> = Pdf::document(app.state(), "invoice")
    .unwrap()
    .data(&json!({ "number": "INV-001", "customer": "Ben" }))
    .render(app.state())
    .await
    .unwrap();

assert!(pdf.starts_with(b"%PDF-"));
assert!(text_of(&pdf).contains("Thank you, Ben."));
# let _ = std::fs::remove_dir_all(dir);
# });
```

What's going on:

- `Pdf::document` fails with `PdfError::Setup` when the module isn't registered, and with
  `PdfError::Invalid` for a name with `..`, a leading `/` or a backslash. A template can't read
  files outside `resources/views/pdf` either.
- `.data(&value)` turns the value into JSON, so the template reads `data.number`,
  `data.customer`, or a list as `data.items`. The same JSON text is in `sys.inputs`.
- `.render(&state)` is a plain `fn` returning a `Send` future, so a handler that awaits it stays
  `Send`. It runs on a blocking thread: a slow render never stalls the async runtime.
- A missing template file, or a mistake in it, fails the render (not `Pdf::document`) with
  `PdfError::Compile`, which holds Typst's messages.

## Sending it: download or mail

The bytes are a normal `Vec<u8>`, so they go wherever a file goes. `PdfError` is a standard
error, so `?` turns it into a 500 response in a handler. As a download in the browser (see
[routing](routing.md#what-a-handler-can-return) for `Download`):

```rust,no_run
use renox::prelude::*;
use renox::Download;
use renox_pdf::Pdf;

/// Sends invoice `id` as a PDF, shown in the browser instead of saved.
async fn invoice(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Download> {
    let number = format!("INV-{id:03}");
    let pdf = Pdf::document(&state, "invoice")?
        .data(&renox::serde_json::json!({ "number": number, "customer": "Ben" }))
        .render(&state)
        .await?;
    Ok(Download::bytes(format!("{number}.pdf"), "application/pdf", pdf).inline())
}
```

Or attached to a mail (see [mail](mail.md#sending-a-mail) for `.attach`). A queued mail keeps its
attachment bytes, so render where you build the mail:

```rust,no_run
use renox::prelude::*;
use renox::mail::Mail;
use renox_pdf::Pdf;

/// Renders the invoice and queues a mail with it attached.
async fn mail_invoice(state: &AppState) -> Result {
    let pdf = Pdf::document(state, "invoice")?.render(state).await?;
    let mail = Mail::new("ben@example.com", "Invoice INV-001", "Your invoice is attached.")
        .attach("INV-001.pdf", "application/pdf", pdf);
    state.queue_mail(mail).await?;
    Ok(())
}
```

## The kit

`#import "@renox/kit:0.1.0": *` gives a template a small design system, served by `renox-pdf`
itself (no download, no network):

| Name | What it is |
|---|---|
| `base` | `#show: base` sets the body text size, colour and line spacing. |
| `colors`, `spacing`, `sizes` | The design tokens: `colors.brand`, `colors.muted`, `spacing.md`, `sizes.title`... |
| `money(cents, currency: "USD")` | An amount in the smallest unit, in the document's language: `$12,345.67`, and `$12.345,67` after `#set text(lang: "es")`. |
| `date(value, format: "[year]-[month]-[day]")` | A date from a `"2026-10-01"` text (or an RFC 3339 one) or a Typst `datetime`. |
| `page-header(title, subtitle:, meta:, logo:)` | A title, label/value pairs at the end, and a brand rule. |
| `data-table(headers, rows, aligns:, widths:, footer:)` | A table with a header row, thin rules and an optional bold footer for totals. |

```typ
#import "@renox/kit:0.1.0": *

#data-table(
  ("Item", "Qty", "Amount"),
  data.items.map(i => (i.name, str(i.quantity), money(i.quantity * i.price))),
  aligns: (start, end, end),
  footer: ("", "Total", money(data.total)),
)
```

`money()` takes an integer amount, like every price in Renox; a decimal fails the render with a
clear message. Its currency table is the one `renox::money::Currency` uses, so a yen has no
decimals and a dinar has three. The brand colours are plain constants; they don't follow the
app's theme.

Any other package import, `@preview/...` included, fails with an error: a render never touches
the network, and apps can't add packages.

## Right-to-left and fonts

`.locale("ar")` sets the text language and, for a language written from right to left (the same
rule as `app.dir` in views, see [right-to-left](ui.md#right-to-left)), the text direction too:

```rust,no_run
use renox::prelude::*;
use renox_pdf::{Pdf, PdfError};

/// An Arabic invoice: language and direction set, everything else as before.
async fn arabic(state: &AppState) -> Result<Vec<u8>, PdfError> {
    Pdf::document(state, "invoice")?
        .locale("ar")
        .render(state)
        .await
}
```

In the template, lay things out with `start` and `end` (never `left` and `right`) and the same
file reads right in English and in Arabic. Texts your app translates can come in through `data`
(the starter template reads `data.labels`).

`Pdf::new()` bundles Inter (Latin) and Noto Sans Arabic. Anything else needs a font you provide,
for every document:

```rust,no_run
use renox::prelude::*;
use renox_pdf::Pdf;

// TrueType, OpenType or a .ttc collection; WOFF is not supported.
let font = std::fs::read("fonts/Brand.ttf").unwrap();
let app = App::new().module(Pdf::new().font(font));
```

A character no registered font has a glyph for would print as an empty box, so by default the
render fails and names it (see below). `Pdf::lenient()` logs a warning instead.

## Images and other files

A document can carry files in memory that the template reads by name: a logo, a signature, a
photo a user uploaded. The name is flat (no `/`, no `..`):

```rust,no_run
use renox::prelude::*;
use renox_pdf::{Pdf, PdfError};

/// An invoice with the shop's logo. The template shows it with `#image("logo.png")`.
async fn with_logo(state: &AppState, logo: Vec<u8>) -> Result<Vec<u8>, PdfError> {
    Pdf::document(state, "invoice")?
        .asset("logo.png", logo)
        .render(state)
        .await
}
```

An image the document doesn't have fails the render.

## Templates in development and in production

While `APP_DEBUG` is on, the template is read from disk on every render, so you edit the `.typ`
file and reload the page. In a release build, register the files compiled into the binary, the
same switch as views:

```rust
use renox::prelude::*;
use renox_pdf::Pdf;

let app = App::new().module(Pdf::new().templates(renox::embedded!()));
```

## Limits

Making a PDF takes CPU, so renders are bounded (both are listed in [config](contributing/config.md)):

- `PDF_MAX_CONCURRENT` (default 2): how many renders run at once. The others wait for a free slot.
- `PDF_TIMEOUT` (seconds, default 30, 0 = no limit): the longest a render may wait and run. One
  that is not done fails with `PdfError::Timeout`. The slot is held until the render's thread
  really stops, so the cap stays honest.

## When something goes wrong

`render` returns a `PdfError`:

| Variant | Meaning |
|---|---|
| `MissingGlyphs(chars)` | No registered font has these characters. Add a font with `Pdf::font`, or call `lenient()`. |
| `Compile(messages)` | The template has a mistake; Typst's messages, one per line. |
| `Data(..)` | The value given to `.data` can't become JSON. |
| `Invalid(..)` | A template name, asset name or locale that isn't usable. |
| `Timeout(limit)` | Not done within `PDF_TIMEOUT`. |
| `Setup(..)` | The module isn't registered, or `PDF_TIMEOUT` / `PDF_MAX_CONCURRENT` isn't a number. |
| `InvalidFont`, `Export(..)`, `Failed(..)` | A font file Typst can't read, a PDF it can't write, a render thread that died. |

## Testing

`renox_pdf::testing` reads the text back out of the produced bytes, so a test doesn't compare
pixels. `PdfAssertions` works on the bytes `render` returns and on a `TestResponse` that
downloads a PDF:

```rust,no_run
use renox::prelude::*;
use renox::testing::TestApp;
use renox_pdf::testing::{PdfAssertions, text_of};

async fn invoice_download(app: &TestApp) {
    let res = app.get("/invoices/1/pdf").await;
    res.assert_ok().assert_pdf().assert_pdf_contains("INV-001").assert_pdf_pages(1);
    assert!(text_of(&res.body).contains("Thank you"));
}
```

`text_of(&bytes)` gives the text of every page; `assert_pdf_contains`, `assert_pdf_missing` and
`assert_pdf_pages(n)` chain.

A limit to know: Typst stores a shaped Arabic word in several pieces. The extracted text has every
letter, but a whole Arabic word is not one string, so assert on its letters or pieces, or compare
the text against a reference file. Latin text comes back in reading order.

Not covered: turning HTML into a PDF, signing, PDF/A, and filling in forms.
