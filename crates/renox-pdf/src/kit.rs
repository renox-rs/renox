//! The Renox kit: the one package a template can import,
//! `#import "@renox/kit:0.1.0": *`.
//!
//! It lives in `kit/kit.typ`, is compiled into the binary and served by the
//! world as a virtual package (its manifest and its entry file); no other
//! package, and no other version, is available, and nothing is downloaded.

use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::Bytes;
use typst::syntax::package::PackageSpec;

/// The kit's package spec, as a template writes it.
pub(crate) const KIT_SPEC: &str = "@renox/kit:0.1.0";

/// The kit's source.
const KIT_SOURCE: &str = include_str!("../kit/kit.typ");

/// The manifest Typst reads to find the entry file.
const KIT_MANIFEST: &str =
    "[package]\nname = \"kit\"\nversion = \"0.1.0\"\nentrypoint = \"kit.typ\"\n";

/// A file of the package `spec`: the kit's, or an error saying why not.
pub(crate) fn file(spec: &PackageSpec, path: &str) -> FileResult<Bytes> {
    if spec.to_string() != KIT_SPEC {
        let hint = if spec.namespace == "renox" && spec.name == "kit" {
            format!("the built-in kit is {KIT_SPEC}")
        } else {
            format!("PDF templates cannot use packages (only the built-in kit {KIT_SPEC})")
        };
        return Err(FileError::Package(PackageError::Other(Some(
            format!("package {spec} is not available: {hint}").into(),
        ))));
    }
    match path {
        "typst.toml" => Ok(Bytes::from_string(KIT_MANIFEST)),
        "kit.typ" => Ok(Bytes::from_string(KIT_SOURCE)),
        other => Err(FileError::NotFound(other.into())),
    }
}

#[cfg(test)]
mod tests {
    use typst::layout::{Frame, FrameItem};
    use typst_layout::PagedDocument;

    use super::*;
    use crate::document::Document;
    use crate::source::Sources;

    /// The text runs of one frame, in order.
    fn frame_runs(frame: &Frame, out: &mut Vec<String>) {
        for (_, item) in frame.items() {
            match item {
                FrameItem::Group(group) => frame_runs(&group.frame, out),
                FrameItem::Text(text) => out.push(text.text.to_string()),
                _ => {}
            }
        }
    }

    /// Every text run of the pages.
    fn runs(document: &PagedDocument) -> Vec<String> {
        let mut out = Vec::new();
        for page in document.pages() {
            frame_runs(&page.frame, &mut out);
        }
        out
    }

    /// The runs of a template that starts with the kit import.
    fn layout(body: &str) -> PagedDocument {
        let template = format!(
            "#import \"{KIT_SPEC}\": *\n#set page(width: 300pt, height: auto, margin: 10pt)\n{body}"
        );
        Document::new(
            Sources::map([("main.typ", template.into_bytes())]),
            "main.typ",
        )
        .layout()
        .unwrap_or_else(|e| panic!("{e}"))
    }

    fn text(body: &str) -> String {
        runs(&layout(body)).concat()
    }

    #[test]
    fn money_follows_the_currency_decimals() {
        let cases = [
            ("1234567", "USD", "$12,345.67"),
            ("5", "USD", "$0.05"),
            ("0", "USD", "$0.00"),
            ("-250", "USD", "-$2.50"),
            ("1500", "JPY", "\u{a5}1,500"),
            ("12345", "KWD", "KWD 12.345"),
            ("7", "KWD", "KWD 0.007"),
            ("999", "EUR", "\u{20ac}9.99"),
            ("7500000", "IDR", "Rp 7,500,000"),
            ("100", "ZZZ", "ZZZ 1.00"),
        ];
        for (amount, currency, expected) in cases {
            let got = text(&format!("#money({amount}, currency: \"{currency}\")"));
            assert_eq!(got, expected, "{amount} {currency}");
        }
    }

    #[test]
    fn money_uses_the_language_separators() {
        assert_eq!(
            text("#set text(lang: \"es\")\n#money(1234567)"),
            "$12.345,67"
        );
        assert_eq!(
            text("#set text(lang: \"fr\")\n#money(1234567, currency: \"EUR\")"),
            "\u{20ac}12\u{202f}345,67"
        );
        assert_eq!(text("#money(1234567, lang: \"de\")"), "$12.345,67");
        assert_eq!(text("#money(1234567)"), "$12,345.67");
    }

    #[test]
    fn money_rejects_a_non_integer_amount() {
        let template = format!("#import \"{KIT_SPEC}\": *\n#money(1.5)");
        let error = Document::new(
            Sources::map([("main.typ", template.into_bytes())]),
            "main.typ",
        )
        .layout()
        .unwrap_err();
        assert!(error.to_string().contains("minor units"), "{error}");
    }

    #[test]
    fn the_kit_table_matches_the_currency_table() {
        // Rows look like `  "USD": ("$", 2),`.
        let mut seen = 0;
        for line in KIT_SOURCE.lines() {
            let Some(rest) = line.trim().strip_prefix('"') else {
                continue;
            };
            let Some((code, rest)) = rest.split_once("\": (\"") else {
                continue;
            };
            let (symbol, digits) = rest.split_once("\", ").unwrap();
            let digits: u32 = digits.trim_end_matches("),").parse().unwrap();
            let currency = renox::money::Currency::parse(code).unwrap();
            assert_eq!(currency.symbol(), symbol, "{code}");
            assert_eq!(currency.decimals(), digits, "{code}");
            assert_eq!(renox::currency_decimals(code), digits, "{code}");
            seen += 1;
        }
        // Nothing missing from the kit either.
        assert_eq!(seen, 49);
        for code in ["AED", "BHD", "JPY", "KWD", "USD", "ZAR", "VND", "TWD"] {
            assert!(KIT_SOURCE.contains(&format!("\"{code}\": (")), "{code}");
        }
    }

    #[test]
    fn dates_are_formatted() {
        assert_eq!(text("#date(\"2026-10-01\")"), "2026-10-01");
        assert_eq!(
            text("#date(\"2026-10-01T09:30:00Z\", format: \"[day]/[month]/[year]\")"),
            "01/10/2026"
        );
        assert_eq!(
            text("#date(datetime(year: 2024, month: 2, day: 29))"),
            "2024-02-29"
        );
        let error = Document::new(
            Sources::map([(
                "main.typ",
                format!("#import \"{KIT_SPEC}\": *\n#date(\"soon\")").into_bytes(),
            )]),
            "main.typ",
        )
        .layout()
        .unwrap_err();
        assert!(error.to_string().contains("not a date"), "{error}");
    }

    #[test]
    fn a_header_and_a_table_lay_out() {
        let document = layout(
            "#show: base\n\
             #page-header(\"Invoice\", subtitle: \"Bikeshop\", meta: ((\"No\", \"42\"), (\"Date\", date(\"2026-10-01\"))))\n\
             #data-table(\n\
               (\"Item\", \"Qty\", \"Price\"),\n\
               ((\"Chain\", \"2\", money(1250)), (\"Tyre\", \"1\", money(3999))),\n\
               aligns: (start, end, end),\n\
               footer: (\"Total\", \"3\", money(6499)),\n\
             )",
        );
        assert_eq!(document.pages().len(), 1);
        let text = runs(&document);
        for expected in [
            "Invoice", "Bikeshop", "Item", "Chain", "$12.50", "$39.99", "$64.99",
        ] {
            assert!(text.iter().any(|r| r == expected), "{expected}: {text:?}");
        }
        // The header comes before the table.
        let position = |needle: &str| text.iter().position(|r| r == needle).unwrap();
        assert!(position("Invoice") < position("Item"));
        assert!(position("Chain") < position("Total"));
    }

    #[test]
    fn a_long_table_breaks_over_pages() {
        let rows: String = (0..80)
            .map(|i| format!("(\"Row {i}\", money({})),", i * 100))
            .collect();
        let template = format!(
            "#import \"{KIT_SPEC}\": *\n#set page(height: 200pt)\n#data-table((\"Name\", \"Amount\"), ({rows}))"
        );
        let document = Document::new(
            Sources::map([("main.typ", template.into_bytes())]),
            "main.typ",
        )
        .layout()
        .unwrap();
        assert!(document.pages().len() > 2, "{}", document.pages().len());
        // The header row repeats on every page.
        let names = document
            .pages()
            .iter()
            .filter(|p| {
                let mut out = Vec::new();
                frame_runs(&p.frame, &mut out);
                out.iter().any(|r| r == "Name")
            })
            .count();
        assert_eq!(names, document.pages().len());
    }

    #[test]
    fn a_row_of_the_wrong_length_is_an_error() {
        let template =
            format!("#import \"{KIT_SPEC}\": *\n#data-table((\"A\", \"B\"), ((\"x\",),))");
        let error = Document::new(
            Sources::map([("main.typ", template.into_bytes())]),
            "main.typ",
        )
        .layout()
        .unwrap_err();
        assert!(error.to_string().contains("1 cells"), "{error}");
    }

    #[test]
    fn only_the_kit_is_a_package() {
        for (spec, hint) in [
            ("@renox/kit:0.2.0", "the built-in kit is @renox/kit:0.1.0"),
            ("@renox/other:0.1.0", "cannot use packages"),
            ("@preview/cetz:0.3.0", "cannot use packages"),
        ] {
            let template = format!("#import \"{spec}\": *");
            let error = Document::new(
                Sources::map([("main.typ", template.into_bytes())]),
                "main.typ",
            )
            .layout()
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("not available") && error.contains(hint),
                "{spec}: {error}"
            );
        }
    }
}
