//! Custom fields in the admin: a model with `custom_fields = "extra"` gets
//! them in its create and edit forms, on its view page, as grid columns
//! with filters, and saved with the record.

use renox::custom_fields::CustomField;
use renox::db::Json;
use renox::grid::Column;
use renox::prelude::*;
use renox::testing::TestApp;
use renox_admin::{Admin, AdminResource, Field};
use serde::Serialize;

#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "parties", form, custom_fields = "extra")]
struct Party {
    id: i64,
    #[form(validate(required))]
    name: String,
    extra: Json<renox::custom_fields::CustomValues>,
}

impl Policy for Party {
    fn allows(&self, _user: &User, _ability: &str) -> bool {
        true
    }
}

struct Parties;

impl AdminResource for Parties {
    type Model = Party;
    type Form = PartyForm;

    fn label(&self) -> &str {
        "Party"
    }

    fn plural_label(&self) -> &str {
        "Parties"
    }

    fn columns(&self) -> Vec<Column> {
        vec![Column::text("name", "Name").searchable()]
    }

    fn fields(&self) -> Vec<Field> {
        vec![Field::text("name", "Name").required()]
    }

    fn fill(&self, party: &mut Party, form: PartyForm) {
        form.fill(party);
    }
}

async fn app() -> TestApp {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .module(
                Admin::new()
                    .authorize(|user| user.email.ends_with("@example.com"))
                    .resource(Parties),
            )
            .migrations(renox::migrations!("tests/migrations")),
    )
    .await;
    CustomField::text("parties", "segment")
        .label("Segment")
        .label_in("es", "Segmento")
        .save(app.db())
        .await
        .unwrap();
    CustomField::number("parties", "age")
        .label("Age")
        .max(120)
        .save(app.db())
        .await
        .unwrap();
    CustomField::choice("parties", "tier")
        .label("Tier")
        .options(&["gold", "basic"])
        .save(app.db())
        .await
        .unwrap();
    CustomField::bool("parties", "vip")
        .label("VIP")
        .save(app.db())
        .await
        .unwrap();
    let ana = User::register(app.db(), "Ana", "ana@example.com", "secret-password-1")
        .await
        .unwrap();
    app.acting_as(&ana);
    app
}

#[renox::test]
async fn forms_save_and_show_custom_fields() {
    let app = app().await;
    app.get("/admin/parties/create")
        .await
        .assert_ok()
        .assert_see(r#"name="extra[segment]""#)
        .assert_see(r#"name="extra[age]""#)
        .assert_see("VIP");
    app.htmx()
        .post(
            "/admin/parties",
            &[
                ("name", "Ana"),
                ("extra[segment]", "retail"),
                ("extra[age]", "34"),
            ],
        )
        .await
        .assert_hx_redirect("/admin/parties");
    let party = Party::query().first(app.db()).await.unwrap().unwrap();
    assert_eq!(
        party.extra.get("segment").and_then(|v| v.as_str()),
        Some("retail")
    );
    app.get(&format!("/admin/parties/{}/edit", party.id))
        .await
        .assert_ok()
        .assert_see(r#"value="retail""#);
    app.get(&format!("/admin/parties/{}", party.id))
        .await
        .assert_ok()
        .assert_see("Segment")
        .assert_see("retail")
        .assert_see("Age");
    // An invalid value is refused under its own input.
    app.htmx()
        .post("/admin/parties", &[("name", "Bo"), ("extra[age]", "500")])
        .await
        .assert_invalid("extra[age]");
}

#[renox::test]
async fn the_list_has_a_column_and_a_filter_per_custom_field() {
    let app = app().await;
    for (name, tier) in [("Ana", "gold"), ("Zelda", "basic")] {
        app.htmx()
            .post("/admin/parties", &[("name", name), ("extra[tier]", tier)])
            .await
            .assert_hx_redirect("/admin/parties");
    }
    app.get("/admin/parties")
        .await
        .assert_ok()
        .assert_see("Tier")
        .assert_see("basic");
    app.get("/admin/parties?in.extra_tier=gold")
        .await
        .assert_ok()
        .assert_see("Ana")
        .assert_dont_see("Zelda");
}

#[renox::test]
async fn inactive_fields_are_hidden() {
    let app = app().await;
    let fields = CustomField::for_model(app.db(), "parties").await.unwrap();
    let vip = fields.iter().find(|f| f.key == "vip").unwrap().id;
    CustomField::deactivate(app.db(), vip).await.unwrap();
    app.get("/admin/parties/create")
        .await
        .assert_ok()
        .assert_dont_see("VIP");
}
