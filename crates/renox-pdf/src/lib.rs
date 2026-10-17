//! PDF documents from Typst templates, with right-to-left text and embedded
//! fonts, for Renox apps. The Typst compiler runs inside the process, so
//! there is no headless browser and no separate service.
//!
//! Register [`Pdf`] with `App::module`, then make a PDF with [`Document::render`]
//! (issue #30; the kit and the rest of the API follow in later steps).

mod document;
mod error;
mod fonts;
mod module;
mod pool;
mod source;
mod templates;
mod world;

pub use document::Document;
pub use error::PdfError;
pub use module::Pdf;
