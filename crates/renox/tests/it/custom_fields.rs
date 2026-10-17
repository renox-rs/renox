//! Custom field definitions (`renox::custom_fields`), on SQLite and (with
//! TEST_DATABASE_URL) PostgreSQL.

use renox::custom_fields::{CustomField, FieldKind, LocaleMap, is_valid_key};
use renox::prelude::*;
use renox::testing::TestApp;

fn keys(fields: &[CustomField]) -> Vec<String> {
    fields.iter().map(|f| f.key.clone()).collect()
}

#[renox::test]
async fn a_definition_round_trips() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let mut retail = LocaleMap::default();
    retail.set("en", "Retail");
    retail.set("es", "Minorista");
    let saved = CustomField::choice("parties", "segment")
        .label("Segment")
        .label_in("es", "Segmento")
        .required()
        .option_labeled("retail", retail)
        .option_labeled("corporate", LocaleMap::default())
        .meta(serde_json::json!({"help": "pick one"}))
        .position(2)
        .save(db)
        .await
        .unwrap();
    assert!(saved.id > 0);
    let found = CustomField::find(db, saved.id).await.unwrap().unwrap();
    assert_eq!(found.kind, FieldKind::Choice);
    assert_eq!(found.label_for("es", "en"), "Segmento");
    assert_eq!(found.label_for("fr", "en"), "Segment");
    assert_eq!(found.meta["help"], "pick one");
    assert_eq!(found.position, 2);
    assert!(found.rules.required && found.active);
    assert_eq!(found.rules.options.len(), 2);
    assert_eq!(
        found.rules.options[0].label.resolve("es", "en"),
        Some("Minorista")
    );
}

#[renox::test]
async fn invalid_keys_and_rules_are_rejected() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    for key in [
        "Passport",
        "has space",
        "a'b",
        "1st",
        "",
        "a-b",
        "x; DROP TABLE users",
    ] {
        assert!(!is_valid_key(key), "{key}");
        assert!(CustomField::text("parties", key).save(db).await.is_err());
    }
    assert!(
        CustomField::choice("parties", "ok").save(db).await.is_err(),
        "a choice needs options"
    );
    assert!(
        CustomField::reference("parties", "ok", "")
            .save(db)
            .await
            .is_err()
    );
    assert!(
        CustomField::text("parties", "ok")
            .min(5)
            .max(2)
            .save(db)
            .await
            .is_err()
    );
    assert!(
        CustomField::all_for_model(db, "parties")
            .await
            .unwrap()
            .is_empty()
    );
}

#[renox::test]
async fn a_key_is_unique_per_model() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    CustomField::text("parties", "note").save(db).await.unwrap();
    assert!(CustomField::text("parties", "note").save(db).await.is_err());
    CustomField::text("orders", "note").save(db).await.unwrap();
}

#[renox::test]
async fn the_cache_is_ordered_active_only_and_invalidated() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let b = CustomField::text("parties", "b")
        .position(2)
        .save(db)
        .await
        .unwrap();
    CustomField::text("parties", "a")
        .position(1)
        .save(db)
        .await
        .unwrap();
    let first = CustomField::for_model(db, "parties").await.unwrap();
    assert_eq!(keys(&first), ["a", "b"]);
    let again = CustomField::for_model(db, "parties").await.unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&first, &again),
        "the second read is cached"
    );

    CustomField::deactivate(db, b.id).await.unwrap();
    let active = CustomField::for_model(db, "parties").await.unwrap();
    assert_eq!(keys(&active), ["a"]);
    let all = CustomField::all_for_model(db, "parties").await.unwrap();
    assert_eq!(all.len(), 2);

    CustomField::text("parties", "c")
        .position(0)
        .save(db)
        .await
        .unwrap();
    let active = CustomField::for_model(db, "parties").await.unwrap();
    assert_eq!(keys(&active), ["c", "a"]);
    CustomField::activate(db, b.id).await.unwrap();
    let active = CustomField::for_model(db, "parties").await.unwrap();
    assert_eq!(active.len(), 3);
}

#[renox::test]
async fn saving_again_changes_the_label_but_not_the_key_or_kind() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let saved = CustomField::text("parties", "passport_no")
        .label("Passport")
        .save(db)
        .await
        .unwrap();
    let mut edited = saved.clone().label("Passport no.");
    edited.key = "renamed".into();
    edited.kind = FieldKind::Number;
    let stored = edited.save(db).await.unwrap();
    assert_eq!(stored.key, "passport_no");
    assert_eq!(stored.kind, FieldKind::Text);
    assert_eq!(stored.label_for("en", "en"), "Passport no.");
    assert!(stored.updated_at >= saved.updated_at);
    CustomField::delete(db, saved.id).await.unwrap();
    assert!(CustomField::find(db, saved.id).await.unwrap().is_none());
}

mod values {
    use std::collections::BTreeMap;

    use renox::custom_fields::{CustomField, CustomValues};
    use renox::db::sql;
    use renox::prelude::*;
    use renox::storage::StorageConfig;
    use renox::testing::TestApp;
    use renox::upload::Upload;
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    async fn run(
        app: &TestApp,
        input: Value,
        uploads: &BTreeMap<String, Upload>,
    ) -> Result<CustomValues, renox::validation::Errors> {
        CustomValues::validate(
            app.state(),
            "parties",
            "extra",
            &object(input),
            uploads,
            "es",
        )
        .await
        .unwrap()
    }

    #[renox::test]
    async fn each_kind_is_checked_and_errors_are_keyed_by_column_and_key() {
        let app = TestApp::new(App::new().disk("docs", |_| Ok(StorageConfig::default()))).await;
        let db = app.db();
        sql("CREATE TABLE cf_companies (id INTEGER PRIMARY KEY)")
            .execute(db)
            .await
            .unwrap();
        sql("INSERT INTO cf_companies (id) VALUES (7)")
            .execute(db)
            .await
            .unwrap();
        CustomField::text("parties", "passport_no")
            .label("Passport")
            .label_in("es", "Pasaporte")
            .required()
            .max(5)
            .save(db)
            .await
            .unwrap();
        CustomField::number("parties", "age")
            .min(1)
            .max(120)
            .save(db)
            .await
            .unwrap();
        CustomField::date("parties", "born").save(db).await.unwrap();
        CustomField::bool("parties", "vip").save(db).await.unwrap();
        CustomField::choice("parties", "segment")
            .options(&["retail", "corporate"])
            .save(db)
            .await
            .unwrap();
        CustomField::reference("parties", "company", "cf_companies")
            .save(db)
            .await
            .unwrap();
        CustomField::file("parties", "scan", "docs")
            .required()
            .save(db)
            .await
            .unwrap();
        CustomField::text("parties", "hidden")
            .inactive()
            .save(db)
            .await
            .unwrap();

        let bad = run(
            &app,
            json!({"passport_no": "toolong", "age": "x", "born": "2026-02-30",
                   "vip": "maybe", "segment": "other", "company": "8", "hidden": "x"}),
            &BTreeMap::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            bad.first("extra[passport_no]"),
            Some("The Pasaporte field must not be longer than 5 characters.")
        );
        for key in ["age", "born", "vip", "segment", "company", "scan"] {
            assert!(bad.has(&format!("extra[{key}]")), "{key}");
        }
        assert!(!bad.has("extra[hidden]"));

        let missing = run(&app, json!({}), &BTreeMap::new()).await.unwrap_err();
        assert_eq!(
            missing.first("extra[passport_no]"),
            Some("The Pasaporte field is required.")
        );

        let mut uploads = BTreeMap::new();
        uploads.insert(
            "scan".to_owned(),
            Upload::new("id.txt", "text/plain", "hello"),
        );
        let good = run(
            &app,
            json!({"passport_no": "AB123", "age": "42", "born": "1984-05-06", "vip": "on",
                   "segment": "retail", "company": 7, "unknown": "dropped", "hidden": "x"}),
            &uploads,
        )
        .await
        .unwrap();
        assert_eq!(good.get("passport_no"), Some(&json!("AB123")));
        assert_eq!(good.get("age"), Some(&json!(42)));
        assert_eq!(good.get("born"), Some(&json!("1984-05-06")));
        assert_eq!(good.get("vip"), Some(&json!(true)));
        assert_eq!(good.get("company"), Some(&json!(7)));
        assert!(good.get("unknown").is_none() && good.get("hidden").is_none());
        let key = good.get("scan").and_then(Value::as_str).unwrap();
        assert!(
            app.state()
                .disk_named("docs")
                .unwrap()
                .exists(key)
                .await
                .unwrap()
        );
    }

    #[renox::test]
    async fn blank_optional_values_are_left_out() {
        let app = TestApp::new(App::new()).await;
        CustomField::number("parties", "age")
            .save(app.db())
            .await
            .unwrap();
        CustomField::choice("parties", "segment")
            .options(&["retail"])
            .save(app.db())
            .await
            .unwrap();
        let values = run(&app, json!({"age": "  ", "segment": ""}), &BTreeMap::new())
            .await
            .unwrap();
        assert!(values.is_empty());
    }
}
