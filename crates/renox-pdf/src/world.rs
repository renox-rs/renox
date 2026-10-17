//! The `typst::World` Renox compiles templates against.
//!
//! It is hand-written instead of using `typst-kit`, so a render has no
//! network access, no system font scan and no file read outside the template
//! root. Sources come from [`Sources`], binary assets from memory, the date
//! from Renox's clock (so `TestApp::travel` reaches `datetime.today()`).

use std::collections::HashMap;
use std::sync::Mutex;

use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

use crate::source::Sources;

/// One render's view of the world: the template, its assets and the fonts.
pub(crate) struct RenoxWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    sources: Sources,
    assets: HashMap<String, Bytes>,
    main: FileId,
    /// Parsed sources, so a file imported twice is parsed once per render.
    cache: Mutex<HashMap<FileId, Source>>,
}

impl RenoxWorld {
    /// A world whose entry file is `main` (a path below the template root).
    /// `assets` are flat names; see [`check_asset_name`].
    pub(crate) fn new(
        sources: Sources,
        main: &str,
        fonts: Vec<Font>,
        assets: HashMap<String, Bytes>,
    ) -> FileResult<Self> {
        let main =
            VirtualPath::new(main).map_err(|e| FileError::Other(Some(e.to_string().into())))?;
        Ok(RenoxWorld {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
            sources,
            assets,
            main: RootedPath::new(VirtualRoot::Project, main).intern(),
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Raw bytes for `id`: an in-memory asset first, then the template root.
    fn bytes(&self, id: FileId) -> FileResult<Bytes> {
        let rooted = id.get();
        if let VirtualRoot::Package(spec) = rooted.root() {
            return Err(FileError::Package(PackageError::Other(Some(
                format!(
                    "package {spec} is not available: PDF templates cannot use packages \
                     (only the built-in kit)"
                )
                .into(),
            ))));
        }
        let vpath = rooted.vpath();
        if let Some(asset) = self.assets.get(vpath.get_without_slash()) {
            return Ok(asset.clone());
        }
        self.sources.read(vpath)
    }
}

/// An asset name is a flat file name: no separators, no `.`/`..`, not empty.
pub(crate) fn check_asset_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(format!(
            "invalid asset name {name:?}: use a flat file name such as \"logo.png\""
        ));
    }
    Ok(())
}

impl World for RenoxWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if let Some(source) = self.cache.lock().ok().and_then(|c| c.get(&id).cloned()) {
            return Ok(source);
        }
        let bytes = self.bytes(id)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?;
        let source = Source::new(id, text.to_owned());
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(id, source.clone());
        }
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.bytes(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        let shift = offset.map_or(0, |o| (o.hours() * 3600.0) as i64);
        let seconds = renox::db::now().timestamp() + shift;
        let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
        Datetime::from_ymd(i32::try_from(year).ok()?, month, day)
    }
}

/// The proleptic Gregorian date of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use typst_layout::PagedDocument;

    use super::*;

    fn world(files: &[(&str, &str)], assets: &[(&str, &[u8])]) -> RenoxWorld {
        let sources = Sources::map(files.iter().map(|(k, v)| (*k, v.as_bytes().to_vec())));
        let assets = assets
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Bytes::new(v.to_vec())))
            .collect();
        RenoxWorld::new(sources, "main.typ", Vec::new(), assets).unwrap()
    }

    fn compile(world: &RenoxWorld) -> Result<PagedDocument, String> {
        let result = typst::compile::<PagedDocument>(world);
        result.output.map_err(|errors| {
            errors
                .iter()
                .map(|e| e.message.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    #[test]
    fn a_trivial_template_compiles() {
        let w = world(
            &[(
                "main.typ",
                "#set page(width: 100pt, height: 100pt)\n#rect()",
            )],
            &[],
        );
        assert_eq!(compile(&w).unwrap().pages().len(), 1);
    }

    #[test]
    fn imports_resolve_inside_the_root() {
        let w = world(
            &[
                (
                    "main.typ",
                    "#import \"parts/a.typ\": answer\n#assert.eq(answer, 42)",
                ),
                ("parts/a.typ", "#let answer = 42"),
            ],
            &[],
        );
        compile(&w).unwrap();
    }

    #[test]
    fn imports_cannot_climb_out_of_the_root() {
        let w = world(&[("main.typ", "#import \"../x.typ\": a")], &[]);
        assert!(compile(&w).is_err());
    }

    #[test]
    fn packages_are_refused_without_network() {
        for spec in [
            "@preview/cetz:0.3.0",
            "@local/mine:0.1.0",
            "@renox/other:1.0.0",
        ] {
            let w = world(&[("main.typ", &format!("#import \"{spec}\": *"))], &[]);
            let message = compile(&w).unwrap_err();
            assert!(message.contains("not available"), "{spec}: {message}");
        }
    }

    #[test]
    fn assets_are_served_from_memory() {
        let w = world(
            &[("main.typ", "#assert.eq(read(\"data.txt\"), \"hello\")")],
            &[("data.txt", b"hello")],
        );
        compile(&w).unwrap();
    }

    #[test]
    fn an_unknown_asset_is_an_error() {
        let w = world(&[("main.typ", "#image(\"logo.png\")")], &[]);
        assert!(compile(&w).unwrap_err().contains("logo.png"));
    }

    #[test]
    fn asset_names_are_flat() {
        assert!(check_asset_name("logo.png").is_ok());
        for bad in ["", ".", "..", "a/b.png", "../x.png", "a\\b.png"] {
            assert!(check_asset_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn today_reads_renox_time() {
        let w = world(&[("main.typ", "")], &[]);
        let (y, m, d) = civil_from_days(renox::db::now().timestamp().div_euclid(86_400));
        assert_eq!(w.today(None), Datetime::from_ymd(y as i32, m, d));
    }

    #[test]
    fn civil_dates_are_right() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}
