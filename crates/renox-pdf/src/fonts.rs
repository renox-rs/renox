//! The fonts a render can use, and the check for characters none of them has.
//!
//! A render never scans the system for fonts: the font book holds exactly the
//! fonts registered here, so the same template gives the same PDF on every
//! machine. [`default_fonts`] is Inter (Latin) and Noto Sans Arabic, the
//! two OFL fonts the kit already uses on the web; an app adds its own with
//! [`Fonts::add`].
//!
//! Typst does not warn about a character a font lacks, it prints the font's
//! "notdef" box. [`check_glyphs`] finds those boxes in the laid-out document
//! and turns them into [`PdfError::MissingGlyphs`].

use std::sync::LazyLock;

use typst::foundations::Bytes;
use typst::layout::{Frame, FrameItem};
use typst::text::Font;
use typst_layout::PagedDocument;

use crate::error::PdfError;

/// Inter (variable weight, Latin), SIL OFL 1.1; the licence sits beside it.
/// The same face renox-core serves as WOFF2 to browsers, as TrueType here
/// because Typst reads no WOFF2.
const INTER: &[u8] = include_bytes!("../fonts/inter-latin-wght-4.1.ttf");
/// Noto Sans Arabic (variable weight), SIL OFL 1.1, as TrueType.
const NOTO_SANS_ARABIC: &[u8] = include_bytes!("../fonts/noto-sans-arabic-wght-5.3.0.ttf");

/// The fonts of a render, in registration order (a face registered first wins
/// when two match equally well).
#[derive(Clone, Default)]
pub(crate) struct Fonts {
    fonts: Vec<Font>,
}

impl Fonts {
    /// No fonts at all; a template then renders only what needs none.
    pub(crate) fn empty() -> Self {
        Fonts::default()
    }

    /// Registers every face in `data` (a `.ttc` collection holds several) and
    /// returns how many were added.
    pub(crate) fn add(
        &mut self,
        data: impl AsRef<[u8]> + Send + Sync + 'static,
    ) -> Result<usize, PdfError> {
        let before = self.fonts.len();
        self.fonts.extend(Font::iter(Bytes::new(data)));
        match self.fonts.len() - before {
            0 => Err(PdfError::InvalidFont),
            n => Ok(n),
        }
    }

    /// The registered faces, the order `World::font` indexes into.
    pub(crate) fn faces(&self) -> &[Font] {
        &self.fonts
    }
}

/// Inter and Noto Sans Arabic, parsed once per process.
pub(crate) fn default_fonts() -> Fonts {
    static DEFAULT: LazyLock<Fonts> = LazyLock::new(|| {
        let mut fonts = Fonts::empty();
        for data in [INTER, NOTO_SANS_ARABIC] {
            fonts.add(data).expect("the bundled fonts are valid");
        }
        fonts
    });
    DEFAULT.clone()
}

/// Every character of `document` that was shaped to a "notdef" glyph (glyph
/// 0, the empty box), sorted and without repeats. Whitespace and control
/// characters are not reported.
pub(crate) fn missing_glyphs(document: &PagedDocument) -> Vec<char> {
    let mut missing = Vec::new();
    for page in document.pages() {
        collect(&page.frame, &mut missing);
    }
    missing.retain(|c| !c.is_whitespace() && !c.is_control());
    missing.sort_unstable();
    missing.dedup();
    missing
}

fn collect(frame: &Frame, missing: &mut Vec<char>) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(group) => collect(&group.frame, missing),
            FrameItem::Text(text) => {
                for glyph in text.glyphs.iter().filter(|g| g.id == 0) {
                    if let Some(chars) = text.text.get(glyph.range()) {
                        missing.extend(chars.chars());
                    }
                }
            }
            _ => {}
        }
    }
}

/// Strict mode fails with [`PdfError::MissingGlyphs`]; lenient mode logs one
/// warning and lets the PDF through.
pub(crate) fn check_glyphs(document: &PagedDocument, lenient: bool) -> Result<(), PdfError> {
    let missing = missing_glyphs(document);
    if missing.is_empty() {
        return Ok(());
    }
    let error = PdfError::MissingGlyphs(missing);
    if lenient {
        tracing::warn!("pdf: {error}; they print as empty boxes");
        return Ok(());
    }
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::source::Sources;

    /// Laid out through `Document`, which makes the registered fonts the
    /// default families (a missing character is only drawn as a box then).
    fn compile(fonts: &Fonts, template: &str) -> PagedDocument {
        let sources = Sources::map([("main.typ", template.as_bytes().to_vec())]);
        Document::new(sources, "main.typ")
            .fonts(fonts.clone())
            .layout()
            .unwrap()
    }

    #[test]
    fn default_fonts_hold_inter_and_noto_sans_arabic() {
        let fonts = default_fonts();
        let families: Vec<_> = fonts
            .faces()
            .iter()
            .map(|f| f.info().family.clone())
            .collect();
        assert!(families.iter().any(|f| f == "Inter"), "{families:?}");
        assert!(
            families.iter().any(|f| f == "Noto Sans Arabic"),
            "{families:?}"
        );
    }

    #[test]
    fn latin_and_arabic_text_has_no_missing_glyphs() {
        let doc = compile(
            &default_fonts(),
            "Invoice 42 \u{2014} \u{0641}\u{0627}\u{062a}\u{0648}\u{0631}\u{0629}",
        );
        assert_eq!(missing_glyphs(&doc), Vec::<char>::new());
        check_glyphs(&doc, false).unwrap();
    }

    #[test]
    fn a_character_no_font_has_is_named() {
        let doc = compile(&default_fonts(), "ok \u{6f22}\u{5b57} \u{6f22}");
        assert_eq!(missing_glyphs(&doc), vec!['\u{5b57}', '\u{6f22}']);
        let error = check_glyphs(&doc, false).unwrap_err();
        assert_eq!(error, PdfError::MissingGlyphs(vec!['\u{5b57}', '\u{6f22}']));
        assert!(error.to_string().contains("U+6F22"));
    }

    #[test]
    fn lenient_lets_missing_glyphs_through() {
        let doc = compile(&default_fonts(), "\u{6f22}");
        check_glyphs(&doc, true).unwrap();
    }

    #[test]
    fn an_app_font_is_added_and_junk_is_refused() {
        let mut fonts = Fonts::empty();
        assert_eq!(fonts.add(NOTO_SANS_ARABIC).unwrap(), 1);
        assert_eq!(
            fonts.add(b"not a font".to_vec()),
            Err(PdfError::InvalidFont)
        );
        assert_eq!(fonts.faces().len(), 1);
        // Only the Arabic font is registered, so Latin text is missing.
        let doc = compile(&fonts, "\u{0627}A");
        assert_eq!(missing_glyphs(&doc), vec!['A']);
    }
}
