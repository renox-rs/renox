//! The test helpers on real PDFs, and the `Send` check for the render future.

use renox::prelude::*;
use renox::testing::TestApp;
use renox_pdf::testing::{PdfAssertions, page_count, text_of};
use renox_pdf::{Document, Pdf};

const ARABIC: &str = "\u{634}\u{643}\u{631}\u{627}\u{64b}";

async fn render(template: &str, data: Option<&str>) -> Vec<u8> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("renox-pdf-testing-{}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("pdf")).unwrap();
    std::fs::write(dir.join("pdf/doc.typ"), template).unwrap();
    let views = dir.clone();
    let app = TestApp::with_config(App::new().module(Pdf::new()), move |config| {
        config.views_path = views.clone();
    })
    .await;
    let mut document = Pdf::document(app.state(), "doc").unwrap();
    if let Some(text) = data {
        document = document.data(&text);
    }
    let bytes = document.render(app.state()).await.unwrap();
    let _ = std::fs::remove_dir_all(dir);
    bytes
}

#[tokio::test]
async fn text_comes_back_in_reading_order_over_pages() {
    let bytes = render(
        "Receipt\n\nThank you for your order.\n\nTotal: #data\n#pagebreak()\nSecond page",
        Some("$12.50"),
    )
    .await;
    let text = text_of(&bytes);
    assert_eq!(
        text, "Receipt\nThank you for your order.\nTotal: $12.50\n\nSecond page",
        "{text:?}"
    );
    assert_eq!(page_count(&bytes), 2);
    bytes
        .assert_pdf()
        .assert_pdf_contains("Thank you  for\nyour order")
        .assert_pdf_missing("refund")
        .assert_pdf_pages(2);
}

#[tokio::test]
async fn text_with_parentheses_and_arabic_is_read_back() {
    let template = format!("A (nested (parens)) and a \\\\ backslash\n\n{ARABIC}");
    let bytes = render(&template, None).await;
    let text = text_of(&bytes);
    assert!(
        text.contains("A (nested (parens)) and a \\ backslash"),
        "{text:?}"
    );
    // The Arabic letters are all there. Typst stores a shaped Arabic word in
    // pieces, so the word itself does not come back as one string.
    for letter in ARABIC.chars() {
        assert!(text.contains(letter), "{letter:?} in {text:?}");
    }
}

#[tokio::test]
async fn failed_assertions_name_what_was_missing() {
    let bytes = render("Hello", None).await;
    let missing = std::panic::catch_unwind(|| {
        bytes.assert_pdf_contains("Goodbye");
    })
    .unwrap_err();
    let message = missing.downcast_ref::<String>().unwrap();
    assert!(
        message.contains("\"Goodbye\"") && message.contains("Hello"),
        "{message}"
    );
    let pages = std::panic::catch_unwind(|| {
        bytes.assert_pdf_pages(3);
    })
    .unwrap_err();
    assert!(pages.downcast_ref::<String>().unwrap().contains("got 1"));
    let present = std::panic::catch_unwind(|| {
        bytes.assert_pdf_missing("Hello");
    });
    assert!(present.is_err());
}

fn assert_send<T: Send>(_: &T) {}

/// A handler that renders a PDF must stay `Send` (axum needs it).
#[tokio::test]
async fn the_render_future_is_send() {
    let app = TestApp::new(App::new().module(Pdf::new())).await;
    let document: Document = Pdf::document(app.state(), "missing").unwrap();
    let future = document.render(app.state());
    assert_send(&future);
    // Also inside an async block that holds the document across the await,
    // as a handler does.
    let state = app.state().clone();
    let handler = async move {
        let document = Pdf::document(&state, "missing").unwrap();
        document.render(&state).await.map(|bytes| bytes.len())
    };
    assert_send(&handler);
    assert!(handler.await.is_err());
}
