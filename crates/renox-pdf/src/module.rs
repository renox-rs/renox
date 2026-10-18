//! The `Pdf` module: the fonts, strictness and render limits an app's PDFs
//! share, registered once with `App::module`.

use std::sync::{Arc, OnceLock};

use renox::{AppState, Embedded, Module, Registry};

use crate::document::Document;
use crate::error::PdfError;
use crate::fonts::{Fonts, default_fonts};
use crate::pool::Pool;
use crate::source::Sources;
use crate::templates;

/// The module that makes PDFs possible in an app. Register it, then render
/// with [`Document::render`](crate::Document::render):
///
/// ```
/// use renox::prelude::*;
/// use renox_pdf::Pdf;
///
/// let app = App::new().module(Pdf::new().lenient());
/// ```
///
/// Renders run on blocking threads, at most `PDF_MAX_CONCURRENT` (default 2)
/// at a time, and fail with [`PdfError::Timeout`] after `PDF_TIMEOUT` seconds
/// (default 30, 0 = no limit).
pub struct Pdf {
    fonts: Fonts,
    lenient: bool,
    embedded: Option<Embedded>,
    /// The first mistake made by a builder call, reported by every render.
    error: Option<PdfError>,
}

impl Default for Pdf {
    fn default() -> Self {
        Self::new()
    }
}

impl Pdf {
    /// The module with the bundled fonts (Inter and Noto Sans Arabic) and
    /// strict glyph checking.
    pub fn new() -> Self {
        Pdf {
            fonts: default_fonts(),
            lenient: false,
            embedded: None,
            error: None,
        }
    }

    /// Registers an app font (TrueType or OpenType, or a `.ttc` collection)
    /// next to the bundled ones, for every document. A file Typst cannot read
    /// makes every render fail with [`PdfError::InvalidFont`].
    pub fn font(mut self, data: impl AsRef<[u8]> + Send + Sync + 'static) -> Self {
        if let Err(e) = self.fonts.add(data) {
            self.error.get_or_insert(e);
        }
        self
    }

    /// Serves the templates from the files `renox::embedded!()` compiled into
    /// the binary when `APP_DEBUG` is off. Debug builds, and release builds
    /// without this call, read them from disk.
    ///
    /// ```
    /// use renox::prelude::*;
    /// use renox_pdf::Pdf;
    ///
    /// let app = App::new().module(Pdf::new().templates(renox::embedded!()));
    /// ```
    pub fn templates(mut self, embedded: Embedded) -> Self {
        self.embedded = Some(embedded);
        self
    }

    /// The document made from the template `name` in `resources/views/pdf`
    /// (`VIEWS_PATH` plus `pdf`): `Pdf::document(&state, "invoice")` reads
    /// `invoice.typ`, and a name like `receipts/sale` reads
    /// `receipts/sale.typ`. A name with `..`, a leading `/` or a backslash is
    /// refused with [`PdfError::Invalid`], and a template cannot read files
    /// outside the folder.
    ///
    /// While `APP_DEBUG` is on the file is read again on every render, so an
    /// edit shows without a restart; otherwise the templates come from
    /// [`Pdf::templates`] when it was called. A missing template fails the
    /// render, not this call. Fails with [`PdfError::Setup`] when the module
    /// is not registered.
    pub fn document(state: &AppState, name: &str) -> Result<Document, PdfError> {
        let setup = Setup::of(state)?;
        let main = templates::template_path(name)?;
        let embedded = setup.embedded.as_ref().map(|files| {
            setup
                .embedded_sources
                .get_or_init(|| templates::embedded_sources(files))
        });
        Ok(Document::new(
            templates::sources(&state.config, embedded),
            main,
        ))
    }

    /// Logs a warning for a character no font has a glyph for instead of
    /// failing the render with [`PdfError::MissingGlyphs`].
    pub fn lenient(mut self) -> Self {
        self.lenient = true;
        self
    }
}

/// What the module provides to the app: read by `Document::render`.
pub(crate) struct Setup {
    pub(crate) fonts: Fonts,
    pub(crate) lenient: bool,
    embedded: Option<Embedded>,
    /// `embedded` as sources, made on first use.
    embedded_sources: OnceLock<Sources>,
    error: Option<PdfError>,
    pub(crate) pool: Pool,
}

impl Setup {
    /// The module's settings from the app's state, or why there are none.
    pub(crate) fn of(state: &AppState) -> Result<Arc<Setup>, PdfError> {
        let setup = state.provided::<Setup>().ok_or_else(|| {
            PdfError::Setup("renox-pdf: add the module with App::module(Pdf::new())".to_owned())
        })?;
        match &setup.error {
            Some(error) => Err(error.clone()),
            None => Ok(setup),
        }
    }
}

impl Module for Pdf {
    fn name(&self) -> &'static str {
        "pdf"
    }

    fn register(&self, app: &mut Registry) {
        app.provide(Setup {
            fonts: self.fonts.clone(),
            lenient: self.lenient,
            embedded: self.embedded,
            embedded_sources: OnceLock::new(),
            error: self.error.clone(),
            pool: Pool::default(),
        });
    }
}
