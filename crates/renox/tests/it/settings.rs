//! #29: typed settings kept in the database (`renox::settings`): defaults
//! until saved, one query per group, other servers noticing a write,
//! validation, sealed groups, the commands, the event and the audit entry,
//! the extractor, the views, the seed file and scopes.

use std::time::Duration;

use renox::audit::{self, Audit};
use renox::prelude::*;
use renox::settings::{Setting, Settings, SettingsChanged, SettingsGroup};
use renox::testing::TestApp;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Validate, Default, Clone, Debug, PartialEq)]
struct Company {
    #[validate(required, max(120))]
    name: String,
    tax_number: Option<String>,
    #[validate(max(40))]
    invoice_format: String,
    employees: i64,
    active: bool,
}

impl SettingsGroup for Company {
    const KEY: &'static str = "company";
}

#[derive(Serialize, Deserialize, Validate, Default, Clone, Debug, PartialEq)]
struct Gateway {
    secret_key: String,
    webhook_secret: String,
}

impl SettingsGroup for Gateway {
    const KEY: &'static str = "gateway";
    const ENCRYPTED: bool = true;
}

fn company(name: &str) -> Company {
    Company {
        name: name.into(),
        invoice_format: "INV-{yyyy}-{0000}".into(),
        ..Company::default()
    }
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/footer", |company: Setting<Company>| async move {
                format!("(c) {}", company.name)
            })
            .get("/about", || async { view("about.html", context! {}) })
    }

    fn register(&self, app: &mut Registry) {
        app.templates(|env| {
            env.add_template(
                "about.html",
                "{{ settings.company.name }} / {{ settings.company.invoice_format }}",
            )
            .unwrap();
        });
    }
}

fn build() -> App {
    App::new()
        .module(Auth::new())
        .module(Settings)
        .module(Audit)
        .module(Pages)
        .settings_in_views::<Company>()
        .settings::<Gateway>()
}

async fn test_app() -> TestApp {
    TestApp::new(build()).await
}

fn settings_queries(queries: &[String]) -> usize {
    queries.iter().filter(|q| q.contains("settings")).count()
}

#[renox::test]
async fn a_group_is_its_defaults_until_saved_and_what_was_saved_after() {
    let app = test_app().await;
    let state = app.state();
    assert_eq!(
        *state.settings::<Company>().await.unwrap(),
        Company::default()
    );
    assert!(state.settings_info::<Company>().await.unwrap().is_none());

    let user = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    state
        .save_settings(&company("Acme"), Some(&user))
        .await
        .unwrap();
    assert_eq!(state.settings::<Company>().await.unwrap().name, "Acme");
    let info = state.settings_info::<Company>().await.unwrap().unwrap();
    assert_eq!(info.updated_by, Some(user.id));

    // Another process (here: a fresh read of the table) sees the row too.
    let stored: String = renox::db::sql("SELECT value FROM settings WHERE key = 'company'")
        .scalar(app.db())
        .await
        .unwrap();
    assert!(stored.contains("Acme"));
}

#[renox::test]
async fn fields_added_to_a_group_later_take_their_defaults() {
    let app = test_app().await;
    renox::db::sql(
        "INSERT INTO settings (key, scope, value, updated_at) VALUES ('company', '', ?, ?)",
    )
    .bind(r#"{"name":"Old Co"}"#)
    .bind(renox::db::now())
    .execute(app.db())
    .await
    .unwrap();
    let read = app.state().settings::<Company>().await.unwrap();
    assert_eq!(read.name, "Old Co");
    assert_eq!(read.employees, 0);
}

#[renox::test]
async fn a_group_costs_one_query_until_a_write() {
    let app = test_app().await;
    let state = app.state();
    let (_, queries) = renox::db::capture_queries(async {
        for _ in 0..5 {
            state.settings::<Company>().await.unwrap();
        }
    })
    .await;
    assert_eq!(settings_queries(&queries), 1, "{queries:#?}");

    // A write refreshes this process's copy without another read.
    let (_, queries) = renox::db::capture_queries(async {
        state.save_settings(&company("Acme"), None).await.unwrap();
        assert_eq!(state.settings::<Company>().await.unwrap().name, "Acme");
        assert_eq!(state.settings::<Company>().await.unwrap().name, "Acme");
    })
    .await;
    let reads = queries
        .iter()
        .filter(|q| {
            q.trim_start().to_uppercase().starts_with("SELECT") && q.contains("FROM settings")
        })
        .count();
    // The one `SELECT` is the write's look at the previous value.
    assert_eq!(reads, 1, "{queries:#?}");
}

#[renox::test]
async fn another_server_sees_a_write_within_the_recheck_interval() {
    let dir = tempfile::tempdir().unwrap();
    let url = crate::commands::shared_database(dir.path());
    crate::commands::make_schema(dir.path()).await;
    let boot = || {
        let url = url.clone();
        TestApp::with_config(build(), move |c| c.database_url = url)
    };
    let (first, second) = (boot().await, boot().await);

    assert_eq!(second.state().settings::<Company>().await.unwrap().name, "");
    first
        .state()
        .save_settings(&company("Acme"), None)
        .await
        .unwrap();
    // The writer sees it at once.
    assert_eq!(
        first.state().settings::<Company>().await.unwrap().name,
        "Acme"
    );
    // The other server asks the version stamp at most once a second.
    second.travel(Duration::from_secs(2));
    let seen = second
        .at_travelled_time(second.state().settings::<Company>())
        .await
        .unwrap();
    assert_eq!(seen.name, "Acme");

    // A second write is noticed the same way.
    first
        .state()
        .save_settings(&company("Acme Ltd"), None)
        .await
        .unwrap();
    second.travel(Duration::from_secs(2));
    let seen = second
        .at_travelled_time(second.state().settings::<Company>())
        .await
        .unwrap();
    assert_eq!(seen.name, "Acme Ltd");
    drop((first, second));
    crate::commands::drop_schema(dir.path()).await;
}

#[renox::test]
async fn invalid_values_are_rejected_with_the_rules_messages() {
    let app = test_app().await;
    let state = app.state();
    let err = state
        .save_settings(&company(""), None)
        .await
        .expect_err("a name is required");
    let Error::Validation(err) = err else {
        panic!("expected a validation error, got {err:?}");
    };
    assert!(err.errors.has("name"));
    let long = Company {
        invoice_format: "x".repeat(41),
        ..company("Acme")
    };
    assert!(state.save_settings(&long, None).await.is_err());
    // Nothing was written.
    app.assert_database_count("settings", 0).await;
    assert_eq!(
        *state.settings::<Company>().await.unwrap(),
        Company::default()
    );
}

#[renox::test]
async fn an_encrypted_group_is_stored_sealed_and_read_back() {
    let app = test_app().await;
    let state = app.state();
    let gateway = Gateway {
        secret_key: "sk_live_very_secret".into(),
        webhook_secret: "whsec_123".into(),
    };
    state.save_settings(&gateway, None).await.unwrap();

    let stored: String = renox::db::sql("SELECT value FROM settings WHERE key = 'gateway'")
        .scalar(app.db())
        .await
        .unwrap();
    assert!(
        !stored.contains("sk_live"),
        "stored as plain text: {stored}"
    );
    assert!(
        !stored.contains("secret_key"),
        "stored as plain JSON: {stored}"
    );
    assert_eq!(*state.settings::<Gateway>().await.unwrap(), gateway);
}

#[renox::test]
async fn the_commands_set_get_and_list_and_set_validates() {
    let app = test_app().await;
    let kernel = app.kernel();
    kernel
        .call("settings:set", ["company.name", "Acme"])
        .await
        .unwrap();
    kernel
        .call("settings:set", ["company.employees", "42"])
        .await
        .unwrap();
    kernel
        .call("settings:set", ["company.active", "true"])
        .await
        .unwrap();
    // A number-looking name stays a string.
    kernel
        .call("settings:set", ["company.tax_number", "12345"])
        .await
        .unwrap();
    let company = app.state().settings::<Company>().await.unwrap();
    assert_eq!(company.name, "Acme");
    assert_eq!(company.employees, 42);
    assert!(company.active);
    assert_eq!(company.tax_number.as_deref(), Some("12345"));

    let state = app.state();
    assert_eq!(
        renox::settings::describe(state, "company.name", "", false)
            .await
            .unwrap(),
        "Acme"
    );
    assert!(
        renox::settings::describe(state, "company", "", false)
            .await
            .unwrap()
            .contains("\"employees\": 42")
    );
    let list = renox::settings::describe_all(state, "", false)
        .await
        .unwrap();
    assert!(list.contains("company = {"), "{list}");

    // `set` validates: an empty name breaks `required`, a name too long breaks `max`.
    let err = kernel
        .call("settings:set", ["company.name", ""])
        .await
        .expect_err("required");
    assert!(format!("{err:?}").contains("company.name"), "{err:?}");
    assert!(
        kernel
            .call("settings:set", ["company.invoice_format", &"x".repeat(50)])
            .await
            .is_err()
    );
    // A value of the wrong type is told so.
    assert!(
        kernel
            .call("settings:set", ["company.employees", "many"])
            .await
            .is_err()
    );
    assert!(
        kernel
            .call("settings:set", ["company.nope", "1"])
            .await
            .is_err()
    );
    assert!(
        kernel
            .call("settings:set", ["nobody.name", "1"])
            .await
            .is_err()
    );
    assert_eq!(
        app.state().settings::<Company>().await.unwrap().name,
        "Acme"
    );
    kernel.call("settings:get", ["company"]).await.unwrap();
    kernel
        .call("settings:list", Vec::<String>::new())
        .await
        .unwrap();
}

#[renox::test]
async fn sealed_groups_print_sealed_unless_revealed() {
    let app = test_app().await;
    let state = app.state();
    app.kernel()
        .call(
            "settings:set",
            ["gateway.secret_key", "sk_live_very_secret"],
        )
        .await
        .unwrap();
    let shown = renox::settings::describe(state, "gateway.secret_key", "", false)
        .await
        .unwrap();
    assert_eq!(shown, "<sealed>");
    let group = renox::settings::describe(state, "gateway", "", false)
        .await
        .unwrap();
    assert!(!group.contains("sk_live"));
    let list = renox::settings::describe_all(state, "", false)
        .await
        .unwrap();
    assert!(list.contains("gateway = <sealed>"), "{list}");
    assert!(!list.contains("sk_live"), "{list}");
    let revealed = renox::settings::describe(state, "gateway.secret_key", "", true)
        .await
        .unwrap();
    assert_eq!(revealed, "sk_live_very_secret");
}

#[renox::test]
async fn saving_emits_an_event_and_writes_an_audit_entry() {
    let app = test_app().await;
    app.fake_events();
    let user = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    app.state()
        .save_settings(&company("Acme"), Some(&user))
        .await
        .unwrap();
    app.assert_emitted(|e: &SettingsChanged| {
        e.key == "company"
            && e.actor == Some(user.id)
            && e.changed == ["invoice_format", "name"]
            && e.previous["name"] == ""
            && e.current["name"] == "Acme"
    });
    // Saving the same values again changes nothing: no event.
    app.state()
        .save_settings(&company("Acme"), Some(&user))
        .await
        .unwrap();
    assert_eq!(app.emitted::<SettingsChanged>().len(), 1);
}

#[renox::test]
async fn the_audit_log_records_a_change_and_hides_sealed_values() {
    let app = test_app().await;
    let user = User::register(app.db(), "Ana", "ana@example.com", "a long password 12")
        .await
        .unwrap();
    app.state()
        .save_settings(&company("Acme"), Some(&user))
        .await
        .unwrap();
    app.state()
        .save_settings(
            &Gateway {
                secret_key: "sk_live_very_secret".into(),
                webhook_secret: String::new(),
            },
            Some(&user),
        )
        .await
        .unwrap();
    let logs: Vec<_> = audit::for_user(app.db(), user.id, 10)
        .await
        .unwrap()
        .into_iter()
        .filter(|log| log.action == "settings.changed")
        .collect();
    assert_eq!(logs.len(), 2);
    let gateway = logs.iter().find(|l| l.data["key"] == "gateway").unwrap();
    assert_eq!(gateway.data["changed"], json!(["secret_key"]));
    assert!(!gateway.data.to_string().contains("sk_live"));
    let company = logs.iter().find(|l| l.data["key"] == "company").unwrap();
    assert_eq!(company.data["current"]["name"], "Acme");
}

#[renox::test]
async fn handlers_and_templates_read_the_group() {
    let app = test_app().await;
    app.get("/footer").await.assert_ok().assert_see("(c) ");
    app.state()
        .save_settings(&company("Acme"), None)
        .await
        .unwrap();
    app.get("/footer").await.assert_see("(c) Acme");
    app.get("/about")
        .await
        .assert_ok()
        .assert_see("Acme / INV-{yyyy}-{0000}");
}

#[renox::test]
async fn a_scope_keeps_its_own_copy_of_a_group() {
    let app = test_app().await;
    let state = app.state();
    state
        .save_settings(&company("Head office"), None)
        .await
        .unwrap();
    state
        .save_settings_in(&company("Branch 7"), "branch-7", None)
        .await
        .unwrap();
    assert_eq!(
        state.settings::<Company>().await.unwrap().name,
        "Head office"
    );
    assert_eq!(
        state.settings_in::<Company>("branch-7").await.unwrap().name,
        "Branch 7"
    );
    assert_eq!(
        state.settings_in::<Company>("branch-8").await.unwrap().name,
        ""
    );
    app.assert_database_count("settings", 2).await;
    app.kernel()
        .call(
            "settings:set",
            ["company.name", "Branch 7 Ltd", "--scope", "branch-7"],
        )
        .await
        .unwrap();
    assert_eq!(
        state.settings_in::<Company>("branch-7").await.unwrap().name,
        "Branch 7 Ltd"
    );
    assert_eq!(
        state.settings::<Company>().await.unwrap().name,
        "Head office"
    );
}

#[renox::test]
async fn a_seed_file_fills_groups_never_saved_and_never_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    std::fs::write(
        &file,
        r#"{"company": {"name": "Seeded Co", "employees": 3}, "gateway": {"secret_key": "sk_seed"}}"#,
    )
    .unwrap();
    let app = TestApp::new(build().settings_from(&file)).await;
    let state = app.state();
    let seeded = state.settings::<Company>().await.unwrap();
    assert_eq!((seeded.name.as_str(), seeded.employees), ("Seeded Co", 3));
    assert_eq!(
        state.settings::<Gateway>().await.unwrap().secret_key,
        "sk_seed"
    );
    let stored: String = renox::db::sql("SELECT value FROM settings WHERE key = 'gateway'")
        .scalar(app.db())
        .await
        .unwrap();
    assert!(
        !stored.contains("sk_seed"),
        "the seed is sealed too: {stored}"
    );

    // Saved values win over the file.
    state.save_settings(&company("Edited"), None).await.unwrap();
    let again = TestApp::new(build().settings_from(&file)).await;
    assert_eq!(
        again.state().settings::<Company>().await.unwrap().name,
        "Seeded Co"
    );

    // A missing file seeds nothing.
    let none = TestApp::new(app_without_file()).await;
    assert_eq!(
        *none.state().settings::<Company>().await.unwrap(),
        Company::default()
    );
}

fn app_without_file() -> App {
    build().settings_from("/nonexistent/settings.json")
}

#[renox::test]
async fn an_invalid_seed_file_is_reported_when_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    std::fs::write(&file, r#"{"company": {"name": ""}}"#).unwrap();
    let app = TestApp::new(build().settings_from(&file)).await;
    let err = app.state().settings::<Company>().await.unwrap_err();
    assert!(format!("{err:?}").contains("company"), "{err:?}");
}

#[renox::test]
async fn test_app_starts_with_settings() {
    let app = test_app()
        .await
        .with_settings(Company {
            name: "Acme".into(),
            ..Company::default()
        })
        .await;
    assert_eq!(
        app.state().settings::<Company>().await.unwrap().name,
        "Acme"
    );
}

#[renox::test]
async fn groups_need_the_module() {
    let err = App::new()
        .settings::<Company>()
        .config(Config::default())
        .boot()
        .await
        .err()
        .expect("the module is missing");
    assert!(format!("{err:?}").contains("Settings"), "{err:?}");
}
