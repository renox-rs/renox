//! PDF documents from Typst templates, with right-to-left text and embedded
//! fonts, for Renox apps. The Typst compiler runs inside the process, so
//! there is no headless browser and no separate service.
//!
//! This is the scaffold of the crate (issue #30): the `typst::World` the
//! templates compile against exists, the public API follows in the next steps.

// The world and its sources are wired to the public `Document` in a later step.
#[allow(dead_code)]
mod source;
#[allow(dead_code)]
mod world;

mod error;
#[allow(dead_code)]
mod fonts;

pub use error::PdfError;
