//! Where a document's template comes from: `resources/views/pdf` on disk
//! while `APP_DEBUG` is on (read again on every render, so an edit shows up
//! without a restart), the files `renox::embedded!()` compiled in otherwise.

use renox::{Config, Embedded};

use crate::error::PdfError;
use crate::source::Sources;

/// The folder of `VIEWS_PATH` (and of the embedded views) that holds the PDF
/// templates.
pub(crate) const FOLDER: &str = "pdf";

/// The extension a template name gets when it has none.
const EXTENSION: &str = ".typ";

/// The sources for a render: the embedded files in a release build that has
/// them, the directory otherwise. `embedded` is built once by the caller.
pub(crate) fn sources(config: &Config, embedded: Option<&Sources>) -> Sources {
    match embedded {
        Some(files) if !config.debug => files.clone(),
        _ => Sources::dir(config.views_path.join(FOLDER)),
    }
}

/// The embedded files below `pdf/`, keyed by their path below that folder.
pub(crate) fn embedded_sources(embedded: &Embedded) -> Sources {
    let prefix = format!("{FOLDER}/");
    Sources::map(embedded.views.iter().filter_map(|(path, text)| {
        let name = path.strip_prefix(&prefix)?;
        Some((name, *text))
    }))
}

/// The path of the template `name` below the template root: `invoice` and
/// `invoice.typ` both give `invoice.typ`, `receipts/sale` gives
/// `receipts/sale.typ`. A name that is empty, absolute or has a `..` (or `.`,
/// empty, backslash or control character) part is refused, so a name from a
/// request cannot reach a file outside the template folder.
pub(crate) fn template_path(name: &str) -> Result<String, PdfError> {
    let invalid = |why: &str| PdfError::Invalid(format!("invalid template name {name:?}: {why}"));
    if name.is_empty() {
        return Err(invalid("it is empty"));
    }
    if name.starts_with('/') {
        return Err(invalid("it must be relative to the template folder"));
    }
    if name.chars().any(|c| c == '\\' || c.is_control()) {
        return Err(invalid("it has a backslash or control character"));
    }
    if name
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("it has an empty, `.` or `..` part"));
    }
    if name.ends_with(EXTENSION) {
        Ok(name.to_owned())
    } else {
        Ok(format!("{name}{EXTENSION}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_get_the_extension() {
        assert_eq!(template_path("invoice").unwrap(), "invoice.typ");
        assert_eq!(template_path("invoice.typ").unwrap(), "invoice.typ");
        assert_eq!(template_path("a/b").unwrap(), "a/b.typ");
    }

    #[test]
    fn names_cannot_leave_the_folder() {
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a/..",
            "./a",
            "a//b",
            "a\\b",
            "a\0b",
            "a/",
        ] {
            assert!(
                matches!(template_path(bad), Err(PdfError::Invalid(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn only_the_pdf_folder_is_embedded() {
        let embedded = Embedded {
            views: &[
                ("pdf/invoice.typ", "= Hi"),
                ("pdf/parts/head.typ", "head"),
                ("home.html", "<p>"),
                ("pdfx/other.typ", "no"),
            ],
            ..Embedded::default()
        };
        let Sources::Map(files) = embedded_sources(&embedded) else {
            panic!("a map");
        };
        let mut keys: Vec<_> = files.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["/invoice.typ", "/parts/head.typ"]);
    }
}
