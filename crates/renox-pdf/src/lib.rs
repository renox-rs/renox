//! PDF documents from Typst templates, with right-to-left text and embedded
//! fonts, for Renox apps. The Typst compiler runs inside the process, so
//! there is no headless browser and no separate service.
//!
//! Register [`Pdf`] with `App::module`, then make a PDF with [`Document::render`].
//! Templates can `#import "@renox/kit:0.1.0": *` for design tokens, `money()`,
//! `date()`, a table and a page header. The guide is docs/pdf.md in the repository.

mod document;
mod error;
mod fonts;
mod kit;
mod module;
mod pool;
mod source;
mod templates;
pub mod testing;
mod world;

pub use document::Document;
pub use error::PdfError;
pub use module::Pdf;

/// Compiles the Rust in docs/pdf.md (the guide) as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/pdf.md")]
pub struct Guide;
