//! Issue #30's acceptance criteria through the public API only: the checks
//! that the unit tests next to the code make on private types, made again the
//! way an app calls them. (The timeout and the concurrency cap are proven on
//! the pool itself in `src/pool.rs`; here the configuration that feeds them
//! is checked, and that a render does not hold up the async runtime.)

use std::path::PathBuf;
use std::time::Duration;

use renox::prelude::*;
use renox::testing::TestApp;
use renox_pdf::testing::{PdfAssertions, page_count, text_of};
use renox_pdf::{Pdf, PdfError};

const INTER: &[u8] = include_bytes!("../fonts/inter-latin-wght-4.1.ttf");

/// An app whose `resources/views/pdf/doc.typ` is `template`.
async fn app(pdf: Pdf, vars: &[(&str, &str)], template: &str) -> (TestApp, PathBuf) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("renox-pdf-accept-{}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("pdf")).unwrap();
    std::fs::write(dir.join("pdf/doc.typ"), template).unwrap();
    let (views, vars) = (
        dir.clone(),
        vars.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<Vec<_>>(),
    );
    let app = TestApp::with_config(App::new().module(pdf), move |config| {
        config.views_path = views.clone();
        config.vars.extend(vars.clone());
    })
    .await;
    (app, dir)
}

async fn render(app: &TestApp, locale: Option<&str>) -> Result<Vec<u8>, PdfError> {
    let mut document = Pdf::document(app.state(), "doc")?;
    if let Some(locale) = locale {
        document = document.locale(locale);
    }
    document.render(app.state()).await
}

/// AC2 and AC8: an Arabic paragraph, an English one and a mixed line render;
/// every Arabic letter and the English words come back, and `.locale("ar")`
/// changes the layout (right-to-left), not just the language tag.
#[tokio::test]
async fn arabic_english_and_mixed_text_render_right_to_left() {
    let arabic = "\u{627}\u{644}\u{633}\u{644}\u{627}\u{645}";
    let template = format!(
        "{arabic} \u{639}\u{644}\u{64a}\u{643}\u{645}\n\nThe order is ready.\n\n\
         {arabic} order 42 {arabic}\n\n#align(start)[Start]"
    );
    let (app, dir) = app(Pdf::new(), &[], &template).await;
    let rtl = render(&app, Some("ar")).await.unwrap();
    let ltr = render(&app, Some("en")).await.unwrap();

    rtl.assert_pdf().assert_pdf_contains("The order is ready.");
    let text = text_of(&rtl);
    for letter in arabic.chars() {
        assert!(text.contains(letter), "{letter:?} in {text:?}");
    }
    assert!(text.contains("order 42"), "{text:?}");
    assert_ne!(rtl, ltr, "the direction changes the layout");
    // The Arabic font is embedded (a PDF names the fonts it carries).
    assert!(
        rtl.windows(10).any(|w| w == b"NotoSansAr"),
        "Noto Sans Arabic is embedded"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// AC3: a character no registered font has fails the render and names it;
/// `Pdf::lenient()` lets it through; an app font added with `.font(bytes)`
/// works and a file that is not a font is refused.
#[tokio::test]
async fn a_missing_glyph_is_an_error_unless_lenient() {
    let template = "Order \u{6f22}\u{5b57}";
    let (strict, dir) = app(Pdf::new(), &[], template).await;
    match render(&strict, None).await {
        Err(PdfError::MissingGlyphs(chars)) => {
            assert_eq!(chars, vec!['\u{5b57}', '\u{6f22}']);
        }
        other => panic!("expected MissingGlyphs, got {other:?}"),
    }
    let message = render(&strict, None).await.unwrap_err().to_string();
    assert!(message.contains("U+6F22"), "{message}");
    let _ = std::fs::remove_dir_all(dir);

    let (lenient, dir) = app(Pdf::new().lenient(), &[], template).await;
    render(&lenient, None).await.unwrap().assert_pdf();
    let _ = std::fs::remove_dir_all(dir);

    let (with_font, dir) = app(Pdf::new().font(INTER), &[], "Plain text").await;
    render(&with_font, None)
        .await
        .unwrap()
        .assert_pdf_contains("Plain text");
    let _ = std::fs::remove_dir_all(dir);

    let (junk, dir) = app(Pdf::new().font(vec![1u8, 2, 3]), &[], "Plain text").await;
    assert!(matches!(
        render(&junk, None).await,
        Err(PdfError::InvalidFont)
    ));
    let _ = std::fs::remove_dir_all(dir);
}

/// AC4: the limits come from `PDF_TIMEOUT` and `PDF_MAX_CONCURRENT`, a value
/// that isn't a number is reported, and the async runtime keeps running while
/// a render is busy (this runtime has one thread).
#[tokio::test(flavor = "current_thread")]
async fn the_limits_are_configured_and_the_runtime_stays_responsive() {
    let (bad, dir) = app(Pdf::new(), &[("PDF_TIMEOUT", "soon")], "x").await;
    assert!(
        matches!(render(&bad, None).await, Err(PdfError::Setup(m)) if m.contains("PDF_TIMEOUT"))
    );
    let _ = std::fs::remove_dir_all(dir);
    let (zero, dir) = app(Pdf::new(), &[("PDF_MAX_CONCURRENT", "0")], "x").await;
    assert!(
        matches!(render(&zero, None).await, Err(PdfError::Setup(m)) if m.contains("PDF_MAX_CONCURRENT"))
    );
    let _ = std::fs::remove_dir_all(dir);

    // A document of many pages takes a while; a ticker on the same runtime
    // keeps ticking during it only if the render runs on a blocking thread.
    let template = "#for i in range(150) [Page #i #lorem(60) #pagebreak()]";
    let (busy, dir) = app(
        Pdf::new(),
        &[("PDF_TIMEOUT", "120"), ("PDF_MAX_CONCURRENT", "1")],
        template,
    )
    .await;
    let ticks = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ticker = {
        let ticks = ticks.clone();
        renox::tokio::spawn(async move {
            loop {
                renox::tokio::time::sleep(Duration::from_millis(5)).await;
                ticks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        })
    };
    let pdf = render(&busy, None).await.unwrap();
    ticker.abort();
    assert!(
        ticks.load(std::sync::atomic::Ordering::SeqCst) >= 3,
        "the runtime was blocked by the render"
    );
    assert!(page_count(&pdf) >= 150);
    let _ = std::fs::remove_dir_all(dir);
}
