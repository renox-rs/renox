//! What can go wrong while making a PDF.

use std::fmt;

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
        }
    }
}

impl std::error::Error for PdfError {}
