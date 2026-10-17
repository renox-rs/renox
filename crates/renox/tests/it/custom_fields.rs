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
