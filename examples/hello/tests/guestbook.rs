use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

const DAY: u64 = 24 * 60 * 60;

/// Tests don't read `.env`; the guestbook's default language is English.
async fn app() -> TestApp {
    TestApp::with_config(hello::app(), |config| config.locale = "en".into()).await
}

#[renox::test]
async fn the_guestbook_lists_entries() {
    let app = app().await;
    app.get("/")
        .await
        .assert_ok()
        .assert_see("Guestbook")
        .assert_see("No messages yet.");
}

#[renox::test]
async fn signing_the_guestbook_thanks_the_owner_by_mail() {
    let app = app().await;
    app.post(
        "/entries",
        &[("name", "Alex"), ("message", "The coffee is great")],
    )
    .await
    .assert_status(303);
    app.assert_database_has(
        "entries",
        &[("name", &"Alex"), ("message", &"The coffee is great")],
    )
    .await;
    app.get("/").await.assert_see("The coffee is great");

    assert_eq!(app.queued_jobs().await, ["thank-guest"]);
    app.run_jobs().await;
    app.assert_mail_sent("owner@example.com", "Alex signed the guestbook");
}

#[renox::test]
async fn the_browser_language_is_used_until_one_is_chosen() {
    let app = app().await; // APP_LOCALE=en
    app.request()
        .header("accept-language", "es-ES,es;q=0.9")
        .get("/")
        .await
        .assert_see("Libro de visitas")
        .assert_see("Todavía no hay mensajes.");
    app.get("/language/en").await;
    app.request()
        .header("accept-language", "es-ES,es;q=0.9")
        .get("/")
        .await
        .assert_see("Guestbook");
}

#[renox::test]
async fn spanish_visitors_get_spanish_messages_and_pages() {
    let app = app().await;
    app.get("/language/es").await;
    let res = app
        .htmx()
        .post("/entries", &[("name", ""), ("message", "x")])
        .await;
    res.assert_invalid("name")
        .assert_json_path("errors.name.0", "El campo nombre es obligatorio.")
        .assert_json_path(
            "errors.message.0",
            "El campo mensaje debe tener entre 3 y 280 caracteres.",
        );
    app.get("/login")
        .await
        .assert_ok()
        .assert_see("Iniciar sesión")
        .assert_see("Recordarme");
    let user = User::register(app.db(), "Ben", "ben@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.get("/account")
        .await
        .assert_ok()
        .assert_see("Tu cuenta")
        .assert_see("Cerrar sesión en otros dispositivos");
}

#[renox::test]
async fn empty_entries_are_rejected() {
    let app = app().await;
    app.htmx()
        .post("/entries", &[("name", ""), ("message", "x")])
        .await
        .assert_invalid("name")
        .assert_invalid("message");
    app.assert_database_count("entries", 0).await;
}

#[renox::test]
async fn the_prune_command_deletes_old_entries() {
    let app = app().await;
    app.post(
        "/entries",
        &[("name", "Alex"), ("message", "The coffee is great")],
    )
    .await;
    // Forty days later, another entry; then the command, on the moved clock.
    app.travel(Duration::from_secs(40 * DAY));
    app.post("/entries", &[("name", "Ben"), ("message", "The tea too")])
        .await;
    let prune = |args: &'static [&'static str], answer: &'static str| {
        app.at_travelled_time(renox::prompt::answering(
            [answer],
            app.kernel().call("entries:prune", args.iter().copied()),
        ))
    };
    // "Delete 1 entries older than 30 days?" → no.
    prune(&[], "no").await.unwrap();
    app.assert_database_count("entries", 2).await;
    prune(&["--days", "30"], "yes").await.unwrap();
    app.assert_database_count("entries", 1).await;
    app.assert_database_has("entries", &[("name", &"Ben")])
        .await;
    // clap checks the arguments.
    assert!(prune(&["--days", "x"], "yes").await.is_err());
}

#[renox::test]
async fn a_logged_in_guest_can_open_their_account() {
    let app = app().await;
    app.get("/account").await.assert_redirect("/login");

    let user = User::register(app.db(), "Ben", "ben@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&user);
    // The layout links the name to the account page.
    app.get("/")
        .await
        .assert_ok()
        .assert_see(r#"href="/account">Ben</a>"#);
    app.get("/account")
        .await
        .assert_ok()
        .assert_see("Your account")
        .assert_see("ben@example.com");
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR not a real image, but it starts like one";

#[renox::test]
async fn photos_are_checked_by_their_content() {
    let app = app().await;
    // A text file named .png is refused (the content is sniffed): the
    // regression a browser check once caught.
    app.htmx()
        .post_multipart(
            "/entries",
            &[("name", "Alex"), ("message", "The coffee is great")],
            &[("photo", "fake.png", b"just text")],
        )
        .await
        .assert_invalid("photo");
    app.assert_database_count("entries", 0).await;
    // A real image is stored and shown.
    app.post_multipart(
        "/entries",
        &[("name", "Alex"), ("message", "The coffee is great")],
        &[("photo", "coffee.png", PNG)],
    )
    .await
    .assert_status(303);
    let photo: String = renox::db::sql("SELECT photo FROM entries")
        .scalar(app.db())
        .await
        .unwrap();
    assert!(
        photo.starts_with("public/entries/") && photo.ends_with(".png"),
        "{photo}"
    );
    app.get("/").await.assert_see(".png");
}

#[renox::test]
async fn htmx_gets_the_new_list_and_an_event() {
    let app = app().await;
    let res = app
        .htmx()
        .post("/entries", &[("name", "Ben"), ("message", "Back again")])
        .await;
    res.assert_ok()
        .assert_header("hx-trigger", "entry-added")
        .assert_see("Back again")
        .assert_dont_see("<html");
}

/// `embedded!()` walks resources/views (nested), resources/lang and public,
/// so a release build carries every file (#258).
#[test]
fn the_release_build_embeds_every_resource() {
    let embedded = renox::embedded!();
    let views: Vec<&str> = embedded.views.iter().map(|(name, _)| *name).collect();
    assert!(views.contains(&"layouts/app.html"), "{views:?}");
    assert!(
        views.iter().any(|v| v.starts_with("guestbook/")),
        "{views:?}"
    );
    let lang: Vec<&str> = embedded.lang.iter().map(|(name, _)| *name).collect();
    assert_eq!(lang, ["en.json", "es.json"]);
    assert!(
        embedded
            .public
            .iter()
            .any(|(name, bytes)| *name == "app.css" && !bytes.is_empty())
    );
    assert!(
        views
            .iter()
            .all(|v| !v.split('/').any(|part| part.starts_with('.')))
    );
}

#[renox::test]
async fn the_models_and_views_are_sound() {
    let app = app().await;
    app.assert_models_match_schema().await;
    app.assert_views_compile();
}

/// The templates are written with tags only: no `{% ... %}` statement is left.
#[test]
fn the_templates_have_no_raw_statements() {
    fn scan(dir: &std::path::Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, found);
            } else if std::fs::read_to_string(&path).is_ok_and(|text| text.contains("{%")) {
                found.push(path.display().to_string());
            }
        }
    }
    let views = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/views");
    let mut found = Vec::new();
    scan(&views, &mut found);
    assert!(found.is_empty(), "raw {{% %}} statements in: {found:?}");
}
