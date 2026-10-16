//! #29: settings pages in the admin panel (`Admin::settings`): the form
//! from the group's fields, the rules, the group's policy, read-only fields
//! and sealed groups that never print their values.

use renox::prelude::*;
use renox::testing::TestApp;
use renox_admin::{Admin, AdminSettings, Field};
use serde::{Deserialize, Serialize};

const PASSWORD: &str = "secret-password-1";
const VIEWER: &str = "viewer@example.com";

#[derive(Serialize, Deserialize, Validate, Default, Clone, Debug)]
struct Company {
    #[validate(required, max(40))]
    name: String,
    tax_number: Option<String>,
    employees: i64,
    online: bool,
    registered_on: Option<String>,
}

impl renox::settings::SettingsGroup for Company {
    const KEY: &'static str = "company";
}

/// Everyone may look; the viewer may not change it.
impl AdminSettings for Company {
    fn fields() -> Vec<Field> {
        vec![
            Field::text("name", "Company name").required(),
            Field::text("tax_number", "Tax number"),
            Field::number("employees", "Employees"),
            Field::toggle("online", "Sells online"),
            Field::text("registered_on", "Registered on").readonly_on_edit(),
        ]
    }

    fn allows(user: &AuthUser, ability: &str) -> bool {
        ability == "view" || user.email != VIEWER
    }
}

#[derive(Serialize, Deserialize, Validate, Default, Clone, Debug)]
struct Gateway {
    #[validate(required)]
    secret_key: String,
    webhook_secret: String,
    sandbox: bool,
}

impl renox::settings::SettingsGroup for Gateway {
    const KEY: &'static str = "gateway";
    const ENCRYPTED: bool = true;
}

impl AdminSettings for Gateway {
    fn fields() -> Vec<Field> {
        vec![
            Field::password("secret_key", "Secret key").required(),
            Field::password("webhook_secret", "Webhook secret"),
            Field::toggle("sandbox", "Sandbox"),
        ]
    }
}

async fn app() -> TestApp {
    TestApp::new(
        App::new()
            .module(Auth::new())
            .module(renox::settings::Settings)
            .module(renox::audit::Audit)
            .module(
                Admin::new()
                    .title("Back office")
                    .authorize(|user| user.email.ends_with("@example.com"))
                    .settings::<Company>("Company")
                    .settings::<Gateway>("Payment gateway"),
            ),
    )
    .await
}

async fn user(app: &TestApp, email: &str) -> User {
    User::register(app.db(), "Ana", email, PASSWORD)
        .await
        .unwrap()
}

#[renox::test]
async fn a_settings_page_shows_the_group_and_is_in_the_navigation() {
    let app = app().await;
    app.state()
        .save_settings(
            &Company {
                name: "Acme".into(),
                employees: 12,
                registered_on: Some("2020-01-02".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    let html = app
        .get("/admin/settings/company")
        .await
        .assert_ok()
        .assert_view("renox-admin/settings.html")
        .text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"hx-post="/admin/settings/company""#);
    has(r#"value="Acme""#);
    has(r#"value="12""#);
    has("Sells online");
    has("Registered on");
    // Every page links to the settings pages, under one heading.
    has(r#"href="/admin/settings/company""#);
    has(r#"href="/admin/settings/gateway""#);
    has("Payment gateway");
    app.get("/admin")
        .await
        .assert_see("/admin/settings/company");
}

#[renox::test]
async fn saving_a_settings_page_checks_the_rules_then_saves_and_audits() {
    let app = app().await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);

    let failed = app
        .htmx()
        .put(
            "/admin/settings/company",
            &[("name", ""), ("employees", "3")],
        )
        .await;
    failed.assert_status(422);
    assert!(failed.json_path("errors.name").is_array());
    app.assert_database_count("settings", 0).await;

    let saved = app
        .htmx()
        .put(
            "/admin/settings/company",
            &[
                ("name", "Acme"),
                ("tax_number", "12.345"),
                ("employees", "7"),
                ("online", "on"),
            ],
        )
        .await;
    saved.assert_hx_redirect("/admin/settings/company");
    let company = app.state().settings::<Company>().await.unwrap();
    assert_eq!(company.name, "Acme");
    assert_eq!(company.tax_number.as_deref(), Some("12.345"));
    assert_eq!(company.employees, 7);
    assert!(company.online);
    app.get("/admin/settings/company")
        .await
        .assert_see("Company saved.");

    // Audited with the user and the new values.
    let logs = renox::audit::for_user(app.db(), ana.id, 10).await.unwrap();
    let log = logs
        .iter()
        .find(|log| log.action == "settings.changed")
        .expect("an audit entry");
    assert_eq!(log.data["key"], "company");
    assert_eq!(log.data["current"]["name"], "Acme");
}

#[renox::test]
async fn a_read_only_field_keeps_its_stored_value() {
    let app = app().await;
    app.state()
        .save_settings(
            &Company {
                name: "Acme".into(),
                registered_on: Some("2020-01-02".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.htmx()
        .put(
            "/admin/settings/company",
            &[
                ("name", "Acme Ltd"),
                ("employees", "0"),
                ("registered_on", "1999-09-09"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/settings/company");
    let company = app.state().settings::<Company>().await.unwrap();
    assert_eq!(company.name, "Acme Ltd");
    assert_eq!(company.registered_on.as_deref(), Some("2020-01-02"));
}

#[renox::test]
async fn the_group_policy_decides_who_may_change_a_settings_page() {
    let app = app().await;
    let viewer = user(&app, VIEWER).await;
    app.acting_as(&viewer);
    app.get("/admin/settings/company")
        .await
        .assert_ok()
        .assert_dont_see("Save changes");
    app.htmx()
        .put("/admin/settings/company", &[("name", "Hacked")])
        .await
        .assert_forbidden();
    app.assert_database_count("settings", 0).await;

    let stranger = user(&app, "someone@elsewhere.test").await;
    app.acting_as(&stranger);
    app.get("/admin/settings/company").await.assert_forbidden();
    app.htmx()
        .put("/admin/settings/company", &[("name", "X")])
        .await
        .assert_forbidden();
}

#[renox::test]
async fn a_sealed_settings_page_never_prints_the_stored_values() {
    let app = app().await;
    let ana = user(&app, "ana@example.com").await;
    app.acting_as(&ana);
    app.get("/admin/settings/gateway")
        .await
        .assert_ok()
        .assert_see("Not set yet.");

    app.htmx()
        .put(
            "/admin/settings/gateway",
            &[
                ("secret_key", "sk_live_very_secret"),
                ("webhook_secret", "whsec_abc"),
                ("sandbox", "on"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/settings/gateway");
    let gateway = app.state().settings::<Gateway>().await.unwrap();
    assert_eq!(gateway.secret_key, "sk_live_very_secret");

    let page = app.get("/admin/settings/gateway").await.assert_ok().text();
    assert!(!page.contains("sk_live_very_secret"), "{page}");
    assert!(!page.contains("whsec_abc"), "{page}");
    assert!(page.contains("Set, last changed"), "{page}");

    // Empty fields keep what is stored; a filled one replaces it.
    app.htmx()
        .put(
            "/admin/settings/gateway",
            &[
                ("secret_key", ""),
                ("webhook_secret", "whsec_new"),
                ("sandbox", "on"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/settings/gateway");
    let gateway = app.state().settings::<Gateway>().await.unwrap();
    assert_eq!(gateway.secret_key, "sk_live_very_secret");
    assert_eq!(gateway.webhook_secret, "whsec_new");
    assert!(gateway.sandbox);

    // The audit entries name the fields, never the values.
    let logs = renox::audit::for_user(app.db(), ana.id, 10).await.unwrap();
    assert!(logs.iter().any(|l| l.action == "settings.changed"));
    for log in logs.iter().filter(|l| l.action == "settings.changed") {
        let text = log.data.to_string();
        assert!(
            !text.contains("sk_live") && !text.contains("whsec"),
            "{text}"
        );
    }
}
