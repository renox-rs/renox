# renox-pdf

PDF documents from templates, with right-to-left text and embedded fonts, for
[Renox](https://github.com/renox-rs/renox) apps. Templates are
[Typst](https://typst.app) files compiled inside the process (the Typst compiler as a
library, pure Rust): no headless browser, no separate service.

```text
App::new().module(Pdf::new());

let pdf: Vec<u8> = Pdf::document(&state, "invoice")? // resources/views/pdf/invoice.typ
    .locale("ar")
    .data(&order)
    .render(&state)
    .await?;
Ok(Download::bytes("INV-001.pdf", "application/pdf", pdf).inline())
```

- Fonts: Inter and Noto Sans Arabic are bundled; `Pdf::font` adds yours. A character no font has
  fails the render with `PdfError::MissingGlyphs` (`Pdf::lenient` logs instead).
- The kit, `#import "@renox/kit:0.1.0": *`: tokens, `money()`, `date()`, `page-header`, `data-table`.
- Renders run on blocking threads, at most `PDF_MAX_CONCURRENT` (2) at once, each within
  `PDF_TIMEOUT` seconds (30).
- Tests: `renox_pdf::testing::{text_of, PdfAssertions}`.
- `rnx make:pdf invoice` writes a starter template.

The guide is [docs/pdf.md](https://github.com/renox-rs/renox/blob/main/docs/pdf.md). Most apps
don't make PDFs, so this is its own crate and adds nothing to `renox`.
