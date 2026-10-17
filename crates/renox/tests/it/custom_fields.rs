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

#[renox::test]
async fn meta_reaches_the_views_and_survives_a_new_label() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let saved = CustomField::text("parties", "passport_no")
        .label("Passport")
        .meta(serde_json::json!({"where": "sidebar", "order": [1, 2]}))
        .save(db)
        .await
        .unwrap();
    let views = CustomField::views(db, "parties", "es").await.unwrap();
    assert_eq!(views[0].meta["where"], "sidebar");
    assert_eq!(views[0].meta["order"][1], 2);
    // The label is the fallback when the locale has none.
    assert_eq!(views[0].label, "Passport");

    saved.label("Passport no.").save(db).await.unwrap();
    let views = CustomField::views(db, "parties", "en").await.unwrap();
    assert_eq!(views[0].label, "Passport no.");
    assert_eq!(views[0].meta["where"], "sidebar", "meta is kept");
    assert_eq!(views[0].key, "passport_no");
}

mod json_paths {
    use renox::custom_fields::{CustomField, CustomValues};
    use renox::db::{Dialect, Json, Query, json_index, sql};
    use renox::prelude::*;
    use renox::testing::TestApp;
    use serde_json::json;

    #[derive(Model, serde::Serialize, Default, Clone, Debug)]
    #[model(table = "cf_parties")]
    struct CfParty {
        id: i64,
        name: String,
        extra: Json<CustomValues>,
    }

    async fn parties() -> TestApp {
        let app = TestApp::new(App::new()).await;
        let db = app.db();
        let (id, column) = match db.dialect() {
            Dialect::Postgres => (
                "BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY",
                "JSONB",
            ),
            _ => ("INTEGER PRIMARY KEY", "TEXT"),
        };
        sql(format!(
            "CREATE TABLE cf_parties (id {id}, name TEXT NOT NULL, extra {column} NOT NULL)"
        ))
        .execute(db)
        .await
        .unwrap();
        for (name, extra) in [
            ("Ana", json!({"segment": "retail", "age": 9, "vip": true})),
            (
                "Bo",
                json!({"segment": "corporate", "age": 30, "vip": false}),
            ),
            ("Cy", json!({"segment": "retail", "age": 100})),
            ("Di", json!({})),
        ] {
            sql("INSERT INTO cf_parties (name, extra) VALUES (?, ?)")
                .bind(name)
                .bind(Json(extra))
                .execute(db)
                .await
                .unwrap();
        }
        CustomField::text("cf_parties", "segment")
            .save(db)
            .await
            .unwrap();
        CustomField::number("cf_parties", "age")
            .save(db)
            .await
            .unwrap();
        CustomField::bool("cf_parties", "vip")
            .save(db)
            .await
            .unwrap();
        app
    }

    async fn names(query: Query<CfParty>, db: &Db) -> Vec<String> {
        query
            .with_custom_fields(db)
            .await
            .unwrap()
            .order_by("id")
            .get(db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect()
    }

    #[renox::test]
    async fn conditions_are_typed_and_checked_against_the_definitions() {
        let app = parties().await;
        let db = app.db();
        let by = |c| CfParty::query().where_(c);
        assert_eq!(
            names(by(CfParty::EXTRA.json("segment").eq("retail")), db).await,
            ["Ana", "Cy"]
        );
        assert_eq!(
            names(by(CfParty::EXTRA.json("segment").like("CORP%")), db).await,
            ["Bo"]
        );
        // 9 < 30 < 100 only as numbers, not as text.
        assert_eq!(
            names(by(CfParty::EXTRA.json("age").gt(10)), db).await,
            ["Bo", "Cy"]
        );
        assert_eq!(
            names(by(CfParty::EXTRA.json("age").is_in([9, 100])), db).await,
            ["Ana", "Cy"]
        );
        assert_eq!(
            names(by(CfParty::EXTRA.json("vip").eq(true)), db).await,
            ["Ana"]
        );
        assert_eq!(
            names(by(CfParty::EXTRA.json("age").is_null()), db).await,
            ["Di"]
        );
        assert!(
            names(by(CfParty::EXTRA.json("age").is_in(Vec::<i64>::new())), db)
                .await
                .is_empty()
        );
    }

    #[renox::test]
    async fn ordering_by_a_key_is_numeric_for_number_fields() {
        let app = parties().await;
        let db = app.db();
        let found = CfParty::query()
            .where_(CfParty::EXTRA.json("age").is_not_null())
            .order_by_json_desc(CfParty::EXTRA.json("age"))
            .with_custom_fields(db)
            .await
            .unwrap()
            .get(db)
            .await
            .unwrap();
        let names: Vec<_> = found.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Cy", "Bo", "Ana"]);
    }

    #[renox::test]
    async fn unknown_or_hostile_keys_return_errors_and_never_reach_sql() {
        let app = parties().await;
        let db = app.db();
        let unknown = CfParty::query()
            .where_(CfParty::EXTRA.json("nope").eq("x"))
            .with_custom_fields(db)
            .await
            .unwrap();
        assert!(unknown.get(db).await.is_err());
        for key in ["a'b", "x') OR 1=1 --", "Seg", "", "1a"] {
            let query = CfParty::query().where_(CfParty::EXTRA.json(key).eq("x"));
            assert!(query.to_sql(Dialect::Sqlite).is_err(), "{key}");
            let query = CfParty::query().order_by_json(CfParty::EXTRA.json(key));
            assert!(query.get(db).await.is_err(), "{key}");
        }
    }

    #[renox::test]
    async fn a_sealed_key_is_refused_for_conditions_and_ordering() {
        let app = parties().await;
        let db = app.db();
        CustomField::text("cf_parties", "tax")
            .sensitive()
            .save(db)
            .await
            .unwrap();
        let conditions = [
            CfParty::query().where_(CfParty::EXTRA.json("tax").eq("x")),
            CfParty::query().where_(CfParty::EXTRA.json("tax").like("x%")),
            CfParty::query().where_(CfParty::EXTRA.json("tax").is_null()),
            CfParty::query().where_(CfParty::EXTRA.json("tax").is_in(["x"])),
            CfParty::query().order_by_json(CfParty::EXTRA.json("tax")),
            CfParty::query().order_by_json_desc(CfParty::EXTRA.json("tax")),
        ];
        for query in conditions {
            let err = query.with_custom_fields(db).await.err().expect("refused");
            assert_eq!(err.status(), renox::prelude::StatusCode::BAD_REQUEST);
            let sealed = renox::db::DbError::sealed_custom_field("tax");
            assert_eq!(err.to_string(), sealed.to_string());
            assert!(format!("{err:?}").contains("tax"));
        }
        // Other keys are unaffected, also next to a refused one in a new query.
        assert_eq!(
            names(
                CfParty::query().where_(CfParty::EXTRA.json("segment").eq("retail")),
                db
            )
            .await,
            ["Ana", "Cy"]
        );
    }

    #[test]
    fn sql_is_written_for_each_dialect_with_bound_values() {
        let query = CfParty::query()
            .where_(CfParty::EXTRA.json("age").gte(5))
            .where_(CfParty::EXTRA.json("segment").like("r%"))
            .order_by_json(CfParty::EXTRA.json("segment"));
        let (sqlite, binds) = query.to_sql(Dialect::Sqlite).unwrap();
        assert!(
            sqlite.contains("json_extract(\"extra\", '$.age') >= ?"),
            "{sqlite}"
        );
        assert!(sqlite.contains("json_extract(\"extra\", '$.segment') LIKE ?"));
        assert!(sqlite.ends_with("ORDER BY json_extract(\"extra\", '$.segment') ASC"));
        assert_eq!(binds.len(), 2);
        let (postgres, _) = query.to_sql(Dialect::Postgres).unwrap();
        assert!(
            postgres.contains("(\"extra\"::jsonb ->> 'age')::numeric >= ?"),
            "{postgres}"
        );
        assert!(postgres.contains("(\"extra\"::jsonb ->> 'segment') ILIKE ?"));
        assert!(postgres.ends_with("ORDER BY (\"extra\"::jsonb ->> 'segment') ASC"));
    }

    #[renox::test]
    async fn json_index_creates_an_expression_index() {
        let app = parties().await;
        let db = app.db();
        let statement = json_index(db.dialect(), "cf_parties", "extra", "segment").unwrap();
        sql(statement).execute(db).await.unwrap();
        assert!(json_index(db.dialect(), "cf_parties", "extra", "a b").is_err());
    }
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

#[renox::test]
async fn the_sensitive_flag_round_trips_and_only_fits_some_kinds() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let saved = CustomField::text("parties", "tax_id")
        .reveal_with("see-tax-ids")
        .save(db)
        .await
        .unwrap();
    let found = CustomField::find(db, saved.id).await.unwrap().unwrap();
    assert!(found.rules.sensitive);
    assert_eq!(found.rules.reveal.as_deref(), Some("see-tax-ids"));
    let plain = CustomField::text("parties", "nick").save(db).await.unwrap();
    assert!(!plain.rules.sensitive && plain.rules.reveal.is_none());
    assert!(
        CustomField::reference("parties", "boss", "people")
            .sensitive()
            .save(db)
            .await
            .is_err()
    );
    assert!(
        CustomField::file("parties", "scan", "docs")
            .sensitive()
            .save(db)
            .await
            .is_err()
    );
}

// Sealed storage of sensitive values (fake test values only).
mod sealed {
    use std::collections::BTreeMap;

    use renox::custom_fields::{CustomField, CustomValues, Opened, SEALED_PREFIX, is_sealed};
    use renox::prelude::*;
    use renox::testing::TestApp;
    use serde_json::{Value, json};

    async fn run(app: &TestApp, input: Value) -> Result<CustomValues, renox::validation::Errors> {
        CustomValues::validate(
            app.state(),
            "parties",
            "extra",
            input.as_object().unwrap(),
            &BTreeMap::new(),
            "en",
        )
        .await
        .unwrap()
    }

    #[renox::test]
    async fn a_value_is_validated_then_sealed_with_a_fresh_nonce() {
        let app = TestApp::new(App::new()).await;
        let db = app.db();
        CustomField::text("parties", "tax_id")
            .sensitive()
            .required()
            .max(8)
            .save(db)
            .await
            .unwrap();
        CustomField::number("parties", "salary")
            .sensitive()
            .save(db)
            .await
            .unwrap();
        let errors = run(&app, json!({"tax_id": "123456789"})).await.unwrap_err();
        assert!(errors.has("extra[tax_id]"));
        let errors = run(&app, json!({"tax_id": ""})).await.unwrap_err();
        assert!(errors.has("extra[tax_id]"));

        let first = run(&app, json!({"tax_id": "TX-1234", "salary": "5200"}))
            .await
            .unwrap();
        let again = run(&app, json!({"tax_id": "TX-1234", "salary": "5200"}))
            .await
            .unwrap();
        let stored = serde_json::to_string(&first).unwrap();
        assert!(!stored.contains("TX-1234") && !stored.contains("5200"));
        assert!(is_sealed(first.get("tax_id").unwrap()));
        assert!(
            first
                .get("tax_id")
                .and_then(Value::as_str)
                .unwrap()
                .starts_with(SEALED_PREFIX)
        );
        assert_ne!(first.get("tax_id"), again.get("tax_id"));
        assert_eq!(
            first.open(db, "tax_id"),
            Some(Opened::Value(json!("TX-1234")))
        );
        assert_eq!(first.open(db, "salary"), Some(Opened::Value(json!(5200))));
        assert_eq!(first.open(db, "missing"), None);
    }

    #[renox::test]
    async fn blank_is_left_out_and_plain_or_damaged_values_read_back_safely() {
        let app = TestApp::new(App::new()).await;
        let db = app.db();
        CustomField::text("parties", "tax_id")
            .sensitive()
            .save(db)
            .await
            .unwrap();
        let values = run(&app, json!({"tax_id": ""})).await.unwrap();
        assert!(values.is_empty());

        let mut stored = CustomValues::default();
        stored.set("old", "plain text");
        stored.set("broken", format!("{SEALED_PREFIX}not-a-token"));
        assert_eq!(
            stored.open(db, "old"),
            Some(Opened::Value(json!("plain text")))
        );
        assert_eq!(stored.open(db, "broken"), Some(Opened::Unreadable));
        assert!(!format!("{:?}", Opened::Value(json!("secret-x"))).contains("secret-x"));
    }
}

// The kit tag `<rx-custom-fields>`.
mod tag {
    use renox::custom_fields::{CustomField, CustomValues, LocaleMap};
    use renox::prelude::*;
    use renox::testing::TestApp;

    struct PartyPages;

    impl Module for PartyPages {
        fn name(&self) -> &'static str {
            "party-pages"
        }

        fn routes(&self) -> Routes {
            Routes::new().get(
                "/party/{locale}",
                |State(state): State<AppState>, Path(locale): Path<String>| async move {
                    let fields = CustomField::views(&state.db, "parties", &locale).await?;
                    let mut extra = CustomValues::default();
                    extra.set("passport_no", "X123");
                    extra.set("segment", "corporate");
                    extra.set("vip", true);
                    Ok::<_, renox::Error>(view("party.html", context! { fields, extra }))
                },
            )
        }
    }

    #[renox::test]
    async fn the_custom_fields_tag_draws_inputs_and_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("party.html"),
            r#"<rx-custom-fields :fields="fields" name="extra" :values="extra"/>
<hr>
<rx-custom-fields :fields="fields" :values="extra" mode="show"/>"#,
        )
        .unwrap();
        let path = dir.path().to_path_buf();
        let app =
            TestApp::with_config(App::new().module(PartyPages), move |c| c.views_path = path).await;
        let db = app.db();
        CustomField::text("parties", "passport_no")
            .label("Passport no.")
            .label_in("es", "N.º de pasaporte")
            .required()
            .max(20)
            .save(db)
            .await
            .unwrap();
        let mut corporate = LocaleMap::default();
        corporate.set("en", "Corporate");
        corporate.set("es", "Empresa");
        CustomField::choice("parties", "segment")
            .label("Segment")
            .option_labeled("retail", LocaleMap::default())
            .option_labeled("corporate", corporate)
            .position(1)
            .save(db)
            .await
            .unwrap();
        CustomField::bool("parties", "vip")
            .label("VIP")
            .position(2)
            .save(db)
            .await
            .unwrap();
        CustomField::number("parties", "credit")
            .label("Credit")
            .min(0)
            .position(3)
            .save(db)
            .await
            .unwrap();
        CustomField::date("parties", "since")
            .label("Since")
            .position(4)
            .save(db)
            .await
            .unwrap();
        CustomField::file("parties", "contract", "local")
            .label("Contract")
            .position(5)
            .save(db)
            .await
            .unwrap();

        app.get("/party/en")
            .await
            .assert_ok()
            .assert_see(r#"name="extra[passport_no]""#)
            .assert_see(r#"value="X123""#)
            .assert_see(r#"maxlength="20""#)
            .assert_see("Passport no.")
            .assert_see(r#"name="extra[segment]""#)
            .assert_see(r#"name="extra[vip]""#)
            .assert_see(r#"name="extra[credit]""#)
            .assert_see(r#"type="date""#)
            .assert_see(r#"name="extra[contract]""#)
            .assert_see(r#"type="file""#)
            .assert_see("Corporate");

        // The request locale picks the labels, with the fallback for the rest.
        app.get("/party/es")
            .await
            .assert_ok()
            .assert_see("N.º de pasaporte")
            .assert_see("Empresa")
            .assert_see("Segment");
        app.assert_views_compile();
    }
}
