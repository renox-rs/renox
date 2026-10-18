//! Template loading through the `Pdf` module: disk in debug, embedded files in
//! release, names confined to the template folder.

use renox::prelude::*;
use renox::testing::TestApp;
use renox_pdf::{Pdf, PdfError};

fn views_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("renox-pdf-tpl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("pdf")).unwrap();
    dir
}

async fn app(pdf: Pdf, views: &std::path::Path, debug: bool) -> TestApp {
    let views = views.to_path_buf();
    TestApp::with_config(App::new().module(pdf), move |config| {
        config.views_path = views;
        config.debug = debug;
    })
    .await
}

async fn render(app: &TestApp, name: &str) -> Result<Vec<u8>, PdfError> {
    Pdf::document(app.state(), name)?.render(app.state()).await
}

#[tokio::test]
async fn debug_reads_the_disk_on_every_render() {
    let views = views_dir("debug");
    let file = views.join("pdf/note.typ");
    std::fs::write(&file, "= First").unwrap();
    // The embedded files are ignored while debugging.
    let embedded = renox::Embedded {
        views: &[("pdf/note.typ", "#panic(\"not me\")")],
        ..Default::default()
    };
    let app = app(Pdf::new().templates(embedded), &views, true).await;
    assert!(render(&app, "note").await.unwrap().starts_with(b"%PDF-"));

    // An edit shows without a restart.
    std::fs::write(&file, "#panic(\"edited\")").unwrap();
    let error = render(&app, "note.typ").await.unwrap_err();
    assert!(
        matches!(&error, PdfError::Compile(m) if m.contains("edited")),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(views);
}

#[tokio::test]
async fn release_uses_the_embedded_files() {
    // Nothing on disk: only the embedded copy can render.
    let views = views_dir("release");
    let embedded = renox::Embedded {
        views: &[
            ("pdf/note.typ", "#include \"parts/head.typ\""),
            ("pdf/parts/head.typ", "= Embedded"),
            ("home.html", "<p>not a template</p>"),
        ],
        ..Default::default()
    };
    let app = app(Pdf::new().templates(embedded), &views, false).await;
    assert!(render(&app, "note").await.unwrap().starts_with(b"%PDF-"));
    // Missing in the binary, and the disk is not consulted.
    std::fs::write(views.join("pdf/disk.typ"), "= Disk").unwrap();
    assert!(matches!(
        render(&app, "disk").await,
        Err(PdfError::Compile(_))
    ));
    assert!(matches!(
        render(&app, "home").await,
        Err(PdfError::Compile(_))
    ));
    let _ = std::fs::remove_dir_all(views);
}

#[tokio::test]
async fn release_without_embedded_files_reads_the_disk() {
    let views = views_dir("plain");
    std::fs::write(views.join("pdf/note.typ"), "= Disk").unwrap();
    let app = app(Pdf::new(), &views, false).await;
    assert!(render(&app, "note").await.unwrap().starts_with(b"%PDF-"));
    let _ = std::fs::remove_dir_all(views);
}

#[tokio::test]
async fn names_and_imports_stay_inside_the_folder() {
    let views = views_dir("confined");
    std::fs::write(views.join("secret.typ"), "= Secret").unwrap();
    std::fs::write(views.join("pdf/peek.typ"), "#include \"../secret.typ\"").unwrap();
    let app = app(Pdf::new(), &views, true).await;
    for name in ["../secret", "/secret", "a/../../secret", "", "a\\b"] {
        assert!(
            matches!(Pdf::document(app.state(), name), Err(PdfError::Invalid(_))),
            "{name:?}"
        );
    }
    // A template cannot import a file above its folder either.
    assert!(matches!(
        render(&app, "peek").await,
        Err(PdfError::Compile(_))
    ));
    // The module must be registered.
    let none = TestApp::new(App::new()).await;
    assert!(matches!(
        Pdf::document(none.state(), "note"),
        Err(PdfError::Setup(_))
    ));
    let _ = std::fs::remove_dir_all(views);
}
