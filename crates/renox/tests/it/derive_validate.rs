//! M25: `#[derive(Validate)]` (with `ValidateHooks`) and the locale from
//! `Accept-Language` (`App::detect_locale`).

use renox::prelude::*;
use renox::testing::TestApp;
use renox::validation::{FormContext, ValidateHooks};

/// Rules only, from attributes.
#[derive(serde::Deserialize, Validate)]
struct Signup {
    #[validate(required, max = 20, label = "Full name")]
    name: String,
    #[validate(required, email, unique("users", "email"))]
    email: String,
    #[validate(required, min = 8, confirmed(&self.password_confirmation))]
    password: String,
    #[serde(default)]
    password_confirmation: String,
    #[serde(default)]
    #[validate(max = 3, each(required, max = 5), distinct)]
    tags: Vec<String>,
    #[validate(rename = "t_shirt", one_of(&["S", "M", "L"]))]
    #[serde(rename = "t_shirt")]
    size: String,
    // No rules: fine.
    #[serde(default)]
    note: String,
}

/// Rules from attributes, the rest in hooks.
#[derive(serde::Deserialize, Validate)]
#[validate(hooks)]
struct Invite {
    #[validate(required, email)]
    email: String,
}

impl ValidateHooks for Invite {
    fn prepare(&mut self) {
        self.email = self.email.trim().to_lowercase();
    }

    async fn authorize(&self, _form: &FormContext<'_>) -> Result<bool> {
        Ok(!self.email.ends_with("@blocked.test"))
    }

    async fn after(&self, _form: &FormContext<'_>, errors: &mut Errors) -> Result {
        if self.email.starts_with("taken@") {
            errors.add("email", "Already invited.");
        }
        Ok(())
    }
}

struct Forms;

impl Module for Forms {
    fn name(&self) -> &'static str {
        "forms"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .post("/signup", |Valid(form): Valid<Signup>| async move {
                format!("{} {} {}", form.name, form.tags.len(), form.note.len())
            })
            .post("/invite", |Valid(form): Valid<Invite>| async move {
                format!("invited {}", form.email)
            })
            .get("/locale", |lang: Lang| async move { lang.locale })
            .get(
                "/language/{code}",
                |session: Session, Path(code): Path<String>| async move {
                    renox::i18n::remember_locale(&session, &code)?;
                    Ok::<_, Error>("set")
                },
            )
    }
}

async fn app() -> TestApp {
    // `tests/lang` has `es.json`, so Spanish is available; other languages aren't.
    TestApp::with_config(
        App::new().module(Auth::new()).module(Forms).detect_locale(),
        |c| {
            c.lang_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lang");
        },
    )
    .await
}

fn signup<'a>(overrides: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut form = vec![
        ("name", "Alex"),
        ("email", "alex@example.com"),
        ("password", "password123"),
        ("password_confirmation", "password123"),
        ("t_shirt", "M"),
    ];
    for (key, value) in overrides {
        form.retain(|(k, _)| k != key);
        form.push((key, value));
    }
    form
}

#[renox::test]
async fn derived_rules_check_every_field() {
    let app = app().await;
    app.post("/signup", &signup(&[]))
        .await
        .assert_ok()
        .assert_see("Alex 0 0");

    let res = app
        .htmx()
        .post(
            "/signup",
            &signup(&[
                ("name", "A name far longer than twenty"),
                ("email", "not-an-email"),
                ("password_confirmation", "different1"),
                ("t_shirt", "XXL"),
            ]),
        )
        .await;
    res.assert_invalid("name")
        .assert_invalid("email")
        .assert_invalid("password")
        .assert_invalid("t_shirt");
    // `label` names the field in its message.
    assert!(
        res.json_path("errors.name.0")
            .as_str()
            .unwrap()
            .contains("Full name"),
        "{}",
        res.text()
    );

    // `each` checks items, `distinct` repeats, `max` the list.
    let mut form = signup(&[]);
    form.extend([("tags", "coffee"), ("tags", "coffee"), ("tags", "toolong")]);
    app.htmx()
        .post("/signup", &form)
        .await
        .assert_invalid("tags.1")
        .assert_invalid("tags.2");
    let mut many = signup(&[]);
    many.extend([("tags", "a"), ("tags", "b"), ("tags", "c"), ("tags", "d")]);
    app.htmx()
        .post("/signup", &many)
        .await
        .assert_invalid("tags");

    // `unique` reads the database.
    User::register(app.db(), "Alex", "alex@example.com", "password123")
        .await
        .unwrap();
    app.htmx()
        .post("/signup", &signup(&[]))
        .await
        .assert_invalid("email");
}

#[renox::test]
async fn derived_rules_work_with_hooks() {
    let app = app().await;
    app.post("/invite", &[("email", "  Ben@Example.COM ")])
        .await
        .assert_see("invited ben@example.com");
    app.post("/invite", &[("email", "x@blocked.test")])
        .await
        .assert_forbidden();
    app.htmx()
        .post("/invite", &[("email", "taken@example.com")])
        .await
        .assert_invalid("email")
        .assert_see("Already invited.");
    app.htmx()
        .post("/invite", &[("email", "nope")])
        .await
        .assert_invalid("email");
}

#[renox::test]
async fn the_browser_language_picks_the_locale() {
    let app = app().await;
    let locale = |header: &'static str| {
        let app = &app;
        async move {
            let res = app
                .request()
                .header("accept-language", header)
                .get("/locale")
                .await;
            res.assert_header("vary", "Accept-Language");
            res.text()
        }
    };
    assert_eq!(locale("es-MX,es;q=0.9,en;q=0.8").await, "es");
    assert_eq!(locale("fr-FR, en;q=0.5").await, "en");
    assert_eq!(locale("de").await, "en", "none available: APP_LOCALE");
    // Validation messages follow it.
    let res = app
        .request()
        .htmx()
        .header("accept-language", "es")
        .post("/invite", &[("email", "")])
        .await;
    assert!(
        res.json_path("errors.email.0")
            .as_str()
            .unwrap()
            .contains("es obligatorio"),
        "{}",
        res.text()
    );
    // A language the visitor chose wins over the browser's.
    app.get("/language/en").await.assert_see("set");
    assert_eq!(locale("es").await, "en");
}

#[renox::test]
async fn without_detect_locale_the_header_is_ignored() {
    let app = TestApp::new(App::new().module(Forms)).await;
    let res = app
        .request()
        .header("accept-language", "es")
        .get("/locale")
        .await;
    res.assert_see("en");
    assert!(
        res.header("vary")
            .is_none_or(|v| !v.contains("Accept-Language"))
    );
}

/// A raw identifier field is checked (and named in errors) as its plain name.
#[derive(serde::Deserialize, Validate)]
struct Kind {
    #[validate(required)]
    r#type: String,
}

#[renox::test]
async fn raw_identifiers_are_validated_under_their_plain_name() {
    let app = TestApp::new(App::new()).await;
    let errors = Validator::rules_of(&Kind {
        r#type: String::new(),
    })
    .finish(app.db())
    .await
    .unwrap();
    assert!(errors.has("type"), "{errors:?}");
    assert!(!errors.has("r#type"));
}

/// Live validation of a list field shows its items' errors (`each(…)` rules
/// report on `tags.0`, `tags.1`), found by a browser test (#264).
#[renox::test]
async fn live_validation_of_a_list_shows_its_items_errors() {
    let app = app().await;
    let res = app
        .request()
        .header("x-renox-validate", "tags")
        .post("/signup", &signup(&[("tags", "toolong")]))
        .await;
    res.assert_ok();
    let body: serde_json::Value = res.json();
    assert_eq!(body["field"], "tags");
    let first = body["errors"][0].as_str().unwrap_or_default();
    assert!(first.contains('5'), "{body}");
    // The list's own rule (max = 3 items) is reported too.
    let many = [("tags", "a"), ("tags", "b"), ("tags", "c"), ("tags", "d")];
    let mut form = signup(&[]);
    form.extend(many);
    let body: serde_json::Value = app
        .request()
        .header("x-renox-validate", "tags")
        .post("/signup", &form)
        .await
        .json();
    assert!(!body["errors"].as_array().unwrap().is_empty(), "{body}");
    // Another field isn't confused with the list (`tags_extra` ≠ `tags.N`).
    let body: serde_json::Value = app
        .request()
        .header("x-renox-validate", "name")
        .post("/signup", &signup(&[("tags", "toolong")]))
        .await
        .json();
    assert_eq!(body["errors"], serde_json::json!([]));
}
