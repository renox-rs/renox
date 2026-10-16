# renox-pdf

PDF documents from templates, with right-to-left text and embedded fonts, for
[Renox](https://github.com/renox-rs/renox) apps. Templates are
[Typst](https://typst.app) files compiled inside the process (the Typst compiler as a
library, pure Rust): no headless browser, no separate service.

This crate is being built up in steps (issue #30); the API lands with the next releases.
Most apps don't make PDFs, so it is its own crate and adds nothing to `renox`.
