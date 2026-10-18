//! The template `rnx make:pdf` writes renders, in English and in Arabic.

use renox::prelude::*;
use renox::serde_json::{Value, json};
use renox::testing::TestApp;
use renox_pdf::Pdf;
use renox_pdf::testing::PdfAssertions;

const STUB: &str = include_str!("../../renox-cli/stubs/pdf.typ.stub");

async fn render(locale: &str, data: Value) -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("renox-pdf-stub-{}-{locale}", std::process::id()));
    std::fs::create_dir_all(dir.join("pdf")).unwrap();
    let template = STUB
        .replace("{{name}}", "invoice")
        .replace("{{title}}", "Invoice");
    std::fs::write(dir.join("pdf/invoice.typ"), template).unwrap();
    let views = dir.clone();
    let app = TestApp::with_config(App::new().module(Pdf::new()), move |config| {
        config.views_path = views.clone();
    })
    .await;
    let bytes = Pdf::document(app.state(), "invoice")
        .unwrap()
        .locale(locale)
        .data(&data)
        .render(app.state())
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
    bytes
}

#[tokio::test]
async fn the_stub_renders_without_data() {
    render("en", json!({}))
        .await
        .assert_pdf()
        .assert_pdf_contains("Invoice");
}

#[tokio::test]
async fn the_stub_renders_in_english() {
    let pdf = render(
        "en",
        json!({
            "number": "A-17", "date": "2026-10-01", "customer": "Ada Lovelace",
            "items": [{"name": "Tune-up", "quantity": 2, "price": 4500}],
        }),
    )
    .await;
    pdf.assert_pdf()
        .assert_pdf_contains("Invoice")
        .assert_pdf_contains("A-17")
        .assert_pdf_contains("2026-10-01")
        .assert_pdf_contains("Tune-up")
        .assert_pdf_contains("$90.00")
        .assert_pdf_contains("Total");
}

#[tokio::test]
async fn the_stub_renders_in_arabic() {
    let pdf = render(
        "ar",
        json!({
            "title": "\u{641}\u{627}\u{62a}\u{648}\u{631}\u{629}",
            "labels": {"total": "\u{627}\u{644}\u{625}\u{62c}\u{645}\u{627}\u{644}\u{64a}"},
            "customer": "\u{623}\u{62d}\u{645}\u{62f}",
            "items": [{"name": "\u{635}\u{64a}\u{627}\u{646}\u{629}", "quantity": 1, "price": 1000}],
        }),
    )
    .await;
    // The strict glyph check passed, so the Arabic is drawn with the bundled
    // font; the text layer holds shaped glyphs, so only the Latin parts are
    // compared.
    pdf.assert_pdf()
        .assert_pdf_contains("Number")
        .assert_pdf_contains("$10.00");
}
