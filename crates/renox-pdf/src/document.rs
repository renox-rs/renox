//! A PDF in the making: a template, the data it shows, its assets and
//! language, compiled to PDF bytes by [`Document::render`].
//!
//! The template sees the data as the dictionary `data` (and as the JSON text
//! `sys.inputs.data`). Assets are in-memory files read with `image("logo.png")`
//! or `read(..)`. A locale sets the text language, and the direction for the
//! right-to-left languages.

use std::collections::HashMap;

use std::future::Future;

use renox::AppState;
use renox::serde::Serialize;
use typst::foundations::{Bytes, Dict, IntoValue, Str};
use typst_layout::PagedDocument;

use crate::error::PdfError;
use crate::fonts::{Fonts, check_glyphs, default_fonts};
use crate::module::Setup;
use crate::pool::Limits;
use crate::source::Sources;
use crate::world::{RenoxWorld, check_asset_name};

/// The asset the data is served as; an app asset cannot use the name.
const DATA_FILE: &str = "renox-data.json";

/// One PDF: where its template comes from and everything it shows.
#[derive(Clone)]
pub struct Document {
    sources: Sources,
    main: String,
    fonts: Fonts,
    assets: HashMap<String, Bytes>,
    data: Option<String>,
    locale: Option<String>,
    lenient: bool,
    /// The first mistake made by a builder call, reported by `render` (the
    /// builder methods are chained, so they cannot return a `Result`).
    error: Option<PdfError>,
}

impl Document {
    /// A document whose entry file is `main`, a path below the template root
    /// held by `sources`. It starts with [`default_fonts`] and no data.
    pub(crate) fn new(sources: Sources, main: impl Into<String>) -> Self {
        Document {
            sources,
            main: main.into(),
            fonts: default_fonts(),
            assets: HashMap::new(),
            data: None,
            locale: None,
            lenient: false,
            error: None,
        }
    }

    fn fail(&mut self, error: PdfError) {
        self.error.get_or_insert(error);
    }

    /// The value the template reads as `data`.
    pub fn data<T: Serialize + ?Sized>(mut self, value: &T) -> Self {
        match renox::serde_json::to_string(value) {
            Ok(json) => self.data = Some(json),
            Err(e) => self.fail(PdfError::Data(e.to_string())),
        }
        self
    }

    /// Sets the text language (`ar`, `en-US`, `pt_BR`) and, for a right-to-left
    /// language, the text direction, as `renox::i18n::direction` decides.
    pub fn locale(mut self, locale: &str) -> Self {
        match typst_language(locale) {
            Ok(_) => self.locale = Some(locale.to_owned()),
            Err(e) => self.fail(e),
        }
        self
    }

    /// Adds a file the template reads by `name` (`image("logo.png")`). The
    /// name is flat: no `/`, no `..`.
    pub fn asset(mut self, name: &str, bytes: impl Into<Vec<u8>>) -> Self {
        match check_asset_name(name) {
            Err(message) => self.fail(PdfError::Invalid(message)),
            Ok(()) if name == DATA_FILE => self.fail(PdfError::Invalid(format!(
                "the asset name {name:?} is reserved"
            ))),
            Ok(()) => {
                self.assets
                    .insert(name.to_owned(), Bytes::new(bytes.into()));
            }
        }
        self
    }

    /// Replaces the registered fonts.
    #[allow(dead_code)]
    pub(crate) fn fonts(mut self, fonts: Fonts) -> Self {
        self.fonts = fonts;
        self
    }

    /// Logs missing glyphs instead of failing the render.
    pub fn lenient(mut self) -> Self {
        self.lenient = true;
        self
    }

    /// The code run before the template's own text, on a line of its own.
    fn prelude(&self) -> String {
        let data = if self.data.is_some() {
            format!("json({DATA_FILE:?})")
        } else {
            "(:)".to_owned()
        };
        let mut code = format!("#let data = {data};");
        // Typst's built-in default family is not in the font book, and it
        // only draws a missing character's empty box when some font was tried
        // first; without this the character would silently vanish and
        // `check_glyphs` could not name it. A template's own `set text(font:
        // ..)` replaces the list.
        let mut families: Vec<String> = Vec::new();
        for font in self.fonts.faces() {
            let family = font.info().family.to_string();
            if !families.contains(&family) {
                families.push(family);
            }
        }
        if !families.is_empty() {
            let list: Vec<String> = families.iter().map(|f| format!("{f:?}")).collect();
            code.push_str(&format!("#set text(font: ({},));", list.join(", ")));
        }
        if let Some(locale) = &self.locale
            && let Ok((lang, region)) = typst_language(locale)
        {
            code.push_str(&format!("#set text(lang: {lang:?}"));
            if let Some(region) = region {
                code.push_str(&format!(", region: {region:?}"));
            }
            if renox::i18n::direction(locale).as_str() == "rtl" {
                code.push_str(", dir: rtl");
            }
            code.push_str(");");
        }
        code
    }

    /// Compiles the template into Typst's laid-out document and checks that
    /// every character has a glyph.
    pub(crate) fn compile(&self) -> Result<PagedDocument, PdfError> {
        let document = self.layout()?;
        check_glyphs(&document, self.lenient)?;
        Ok(document)
    }

    /// Compiles the template into Typst's laid-out document, without the
    /// glyph check.
    pub(crate) fn layout(&self) -> Result<PagedDocument, PdfError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let mut assets = self.assets.clone();
        let mut inputs = Dict::new();
        if let Some(json) = &self.data {
            assets.insert(DATA_FILE.to_owned(), Bytes::from_string(json.clone()));
            inputs.insert(Str::from("data"), json.as_str().into_value());
        }
        let world = RenoxWorld::new(
            self.sources.clone(),
            &self.main,
            self.fonts.faces().to_vec(),
            assets,
        )
        .map_err(|e| PdfError::Compile(e.to_string()))?
        .with_prelude(self.prelude())
        .with_inputs(inputs);
        let result = typst::compile::<PagedDocument>(&world);
        let document = result.output.map_err(|errors| {
            PdfError::Compile(
                errors
                    .iter()
                    .map(|e| world.describe(e))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        })?;
        Ok(document)
    }

    /// Makes the PDF on a blocking thread and returns its bytes.
    ///
    /// The fonts and strictness come from the registered [`Pdf`](crate::Pdf)
    /// module. At most `PDF_MAX_CONCURRENT` renders run at once (the others
    /// wait), and one that is not done within `PDF_TIMEOUT` seconds fails with
    /// [`PdfError::Timeout`]. Fails with [`PdfError::Setup`] when the module is
    /// not registered.
    ///
    /// This is a plain `fn` returning `impl Future + Send`, so a handler that
    /// awaits it stays `Send`.
    pub fn render(
        &self,
        state: &AppState,
    ) -> impl Future<Output = Result<Vec<u8>, PdfError>> + Send + use<> {
        let prepared = Setup::of(state).and_then(|setup| {
            let limits = Limits::from_config(&state.config)?;
            Ok((setup, limits))
        });
        let mut document = self.clone();
        async move {
            let (setup, limits) = prepared?;
            document.fonts = setup.fonts.clone();
            document.lenient |= setup.lenient;
            setup
                .pool
                .run(limits, move || document.render_blocking())
                .await
        }
    }

    /// Compiles the template and writes the PDF.
    pub(crate) fn render_blocking(&self) -> Result<Vec<u8>, PdfError> {
        let document = self.compile()?;
        typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|errors| {
            PdfError::Export(
                errors
                    .iter()
                    .map(|e| e.message.to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        })
    }
}

/// The Typst language (two or three lowercase letters) and region (two
/// uppercase letters) of a locale such as `ar`, `en-US` or `pt_BR`.
fn typst_language(locale: &str) -> Result<(String, Option<String>), PdfError> {
    let invalid = || {
        PdfError::Invalid(format!(
            "invalid locale {locale:?}: use \"ar\" or \"en-US\""
        ))
    };
    let mut parts = locale.split(['-', '_']);
    let lang = parts.next().unwrap_or_default();
    if !(2..=3).contains(&lang.len()) || !lang.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Err(invalid());
    }
    let region = parts
        .find(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase);
    Ok((lang.to_ascii_lowercase(), region))
}

#[cfg(test)]
mod tests {
    use typst::layout::{Frame, FrameItem};

    use super::*;

    const ARABIC_TEMPLATE: &str = include_str!("../tests/assets/arabic.typ");
    const ARABIC_TEXT: &str = include_str!("../tests/assets/arabic.expected.txt");

    /// Every text run of the document in reading order, one per line: Typst
    /// keeps each run in logical order, so this is what the PDF's text layer
    /// holds, without comparing pixels.
    fn text_runs(document: &PagedDocument) -> String {
        let runs: Vec<String> = positioned_runs(document).into_iter().map(|r| r.1).collect();
        runs.join("\n")
    }

    /// The same runs with the horizontal position of each on its page.
    fn positioned_runs(document: &PagedDocument) -> Vec<(f64, String)> {
        fn walk(frame: &Frame, x: f64, out: &mut Vec<(f64, String)>) {
            for (pos, item) in frame.items() {
                let x = x + pos.x.to_pt();
                match item {
                    FrameItem::Group(group) => walk(&group.frame, x, out),
                    FrameItem::Text(text) => out.push((x, text.text.to_string())),
                    _ => {}
                }
            }
        }
        let mut runs = Vec::new();
        for page in document.pages() {
            walk(&page.frame, 0.0, &mut runs);
        }
        runs
    }

    fn document(template: &str) -> Document {
        Document::new(
            Sources::map([("main.typ", template.as_bytes().to_vec())]),
            "main.typ",
        )
    }

    #[derive(Serialize)]
    #[serde(crate = "renox::serde")]
    struct Receipt<'a> {
        title: &'a str,
        english: &'a str,
        arabic: &'a str,
        mixed: &'a str,
        number: u32,
    }

    fn receipt() -> Receipt<'static> {
        Receipt {
            title: "Receipt",
            english: "Thank you for your order.",
            arabic: "\u{634}\u{643}\u{631}\u{627}\u{64b} \u{644}\u{637}\u{644}\u{628}\u{643}\u{645}.",
            mixed: "Order 42 \u{631}\u{642}\u{645} \u{627}\u{644}\u{637}\u{644}\u{628} 42 is paid",
            number: 42,
        }
    }

    #[test]
    fn arabic_english_and_mixed_text_keep_logical_order() {
        let doc = document(ARABIC_TEMPLATE).data(&receipt()).locale("en");
        let compiled = doc.compile().unwrap();
        assert_eq!(compiled.pages().len(), 1);
        // The reference holds the text runs Typst shaped, line by line; a
        // change in shaping, bidi or the bundled fonts shows up as a diff.
        // (Compared trimmed per line: editors strip the spaces some runs hold.)
        let trimmed =
            |text: &str| -> Vec<String> { text.lines().map(|l| l.trim().to_owned()).collect() };
        assert_eq!(trimmed(&text_runs(&compiled)), trimmed(ARABIC_TEXT));
        // Arabic reads right to left: its first word sits right of its last.
        let runs = positioned_runs(&compiled);
        let x_of = |needle: char| runs.iter().find(|r| r.1.contains(needle)).unwrap().0;
        assert!(x_of('\u{634}') > x_of('\u{644}'), "{runs:?}");
        // English stays in one run, in reading order.
        assert!(runs.iter().any(|r| r.1 == "Thank you for your order."));
        let pdf = doc.render_blocking().unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn rendering_twice_gives_the_same_bytes() {
        let doc = document(ARABIC_TEMPLATE).data(&receipt());
        assert_eq!(
            doc.render_blocking().unwrap(),
            doc.render_blocking().unwrap()
        );
    }

    #[test]
    fn data_is_a_dictionary_and_sys_inputs_json() {
        let doc = document(
            "#assert.eq(data.number, 42)\n#assert.eq(json(bytes(sys.inputs.data)).title, \"Receipt\")",
        )
        .data(&receipt());
        doc.compile().unwrap();
    }

    #[test]
    fn locale_sets_language_and_direction() {
        let probe = "#context [#text.lang;#text.region;#text.dir]";
        let ar = document(probe).locale("ar-AE").compile().unwrap();
        let ar = text_runs(&ar);
        for part in ["ar", "AE", "rtl"] {
            assert!(ar.contains(part), "{ar:?}");
        }
        let en = document(probe).locale("en").compile().unwrap();
        let en = text_runs(&en);
        assert!(en.contains("en") && !en.contains("rtl"), "{en:?}");
        let bad = document(probe).locale("not a locale").compile();
        assert!(matches!(bad, Err(PdfError::Invalid(_))));
    }

    #[test]
    fn an_asset_is_read_by_name() {
        let ok = document("#assert.eq(read(\"note.txt\"), \"hi\")").asset("note.txt", "hi");
        ok.compile().unwrap();
        let unknown = document("#read(\"missing.txt\")").compile();
        assert!(matches!(unknown, Err(PdfError::Compile(m)) if m.contains("missing.txt")));
        for bad in ["a/b.png", "..", DATA_FILE] {
            let doc = document("x").asset(bad, "x");
            assert!(matches!(doc.compile(), Err(PdfError::Invalid(_))), "{bad}");
        }
    }

    #[test]
    fn a_template_may_start_with_markup() {
        let doc = document("= Heading\nbody").compile().unwrap();
        let text = text_runs(&doc);
        assert!(
            text.contains("Heading") && !text.contains("= Heading"),
            "{text:?}"
        );
    }

    #[test]
    fn an_error_names_the_template_line() {
        let Err(PdfError::Compile(message)) =
            document("= Title\n\n#unknown-function()\n").compile()
        else {
            panic!("expected a compile error");
        };
        assert!(message.contains("main.typ:3:"), "{message}");
        let first = document("#unknown-function()").compile();
        assert!(matches!(first, Err(PdfError::Compile(m)) if m.contains("main.typ:1:")));
    }

    #[test]
    fn a_template_error_and_missing_glyphs_are_reported() {
        let broken = document("#unknown-function()").compile();
        assert!(matches!(broken, Err(PdfError::Compile(_))));
        let strict = document("\u{6f22}").compile();
        assert_eq!(
            strict.unwrap_err(),
            PdfError::MissingGlyphs(vec!['\u{6f22}'])
        );
        assert!(document("\u{6f22}").lenient().render_blocking().is_ok());
    }

    fn assert_send<T: Send>(_: &T) {}

    #[tokio::test]
    async fn render_goes_through_the_module() {
        use renox::prelude::*;
        use renox::testing::TestApp;

        let app = TestApp::new(App::new().module(crate::Pdf::new())).await;
        let render = document("Hello").render(app.state());
        assert_send(&render);
        let pdf = render.await.unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        // Strict by default; the module's `lenient` applies to every document.
        let strict = document("\u{6f22}").render(app.state()).await;
        assert_eq!(strict, Err(PdfError::MissingGlyphs(vec!['\u{6f22}'])));
        let lenient = TestApp::new(App::new().module(crate::Pdf::new().lenient())).await;
        assert!(document("\u{6f22}").render(lenient.state()).await.is_ok());
        // A bad font given to the module fails every render.
        let bad = TestApp::new(App::new().module(crate::Pdf::new().font(vec![0u8; 8]))).await;
        let error = document("x").render(bad.state()).await;
        assert_eq!(error, Err(PdfError::InvalidFont));
        // Without the module there is a clear error.
        let none = TestApp::new(App::new()).await;
        let error = document("x").render(none.state()).await;
        assert!(matches!(error, Err(PdfError::Setup(m)) if m.contains("Pdf::new()")));
    }

    #[test]
    fn an_app_font_replaces_the_defaults() {
        let mut arabic_only = Fonts::empty();
        arabic_only
            .add(&include_bytes!("../fonts/noto-sans-arabic-wght-5.3.0.ttf")[..])
            .unwrap();
        let doc = document("\u{627}A").fonts(arabic_only);
        assert_eq!(
            doc.compile().unwrap_err(),
            PdfError::MissingGlyphs(vec!['A'])
        );
    }
}
