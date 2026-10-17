//! What can go wrong while making a PDF.

use std::fmt;
use std::time::Duration;

/// Why a PDF could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PdfError {
    /// The template uses characters that none of the registered fonts has a
    /// glyph for. They would print as empty boxes, so the render stops and
    /// names them (sorted, each once). Register a font that covers them with
    /// `Pdf::font`, or call `Pdf::lenient` to log a warning instead.
    MissingGlyphs(Vec<char>),
    /// The bytes passed to `Pdf::font` are not a font file Typst can read
    /// (TrueType, OpenType or a collection of them; WOFF is not supported).
    InvalidFont,
    /// The template did not compile; holds Typst's messages, one per line.
    Compile(String),
    /// The value given to `Document::data` cannot be turned into JSON.
    Data(String),
    /// A name or value given to `Document::asset` or `Document::locale` is
    /// not usable; holds the reason.
    Invalid(String),
    /// Typst could not write the PDF; holds its messages.
    Export(String),
    /// The render did not finish within `PDF_TIMEOUT`, or waited that long
    /// for a free slot (`PDF_MAX_CONCURRENT`). Holds the limit.
    Timeout(Duration),
    /// The module is not registered, or `PDF_TIMEOUT` / `PDF_MAX_CONCURRENT`
    /// is not a valid number; holds the reason.
    Setup(String),
    /// The render thread panicked or the pool shut down; holds the reason.
    Failed(String),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfError::MissingGlyphs(chars) => {
                write!(f, "no registered font has a glyph for: ")?;
                for (i, c) in chars.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{c} (U+{:04X})", u32::from(*c))?;
                }
                Ok(())
            }
            PdfError::InvalidFont => {
                f.write_str("the bytes are not a TrueType or OpenType font file")
            }
            PdfError::Compile(messages) => write!(f, "the template did not compile: {messages}"),
            PdfError::Data(message) => write!(f, "the template data is not valid JSON: {message}"),
            PdfError::Invalid(message) => f.write_str(message),
            PdfError::Export(messages) => write!(f, "the PDF could not be written: {messages}"),
            PdfError::Timeout(limit) => write!(
                f,
                "the PDF was not ready within {} seconds (PDF_TIMEOUT)",
                limit.as_secs()
            ),
            PdfError::Setup(message) | PdfError::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PdfError {}
