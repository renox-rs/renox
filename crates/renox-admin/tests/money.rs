//! `Field::money(..).currency_from(..)` and `Entry::currency_from(..)`:
//! each record's amount in its own currency in the list, the form and the
//! view page.

use renox::grid::Column;
use renox::prelude::*;
use renox::testing::TestApp;
use renox_admin::{Admin, AdminResource, Entry, Field};
use serde::{Deserialize, Serialize};

#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "invoices")]
struct Invoice {
    id: i64,
    total: i64,
    total_currency: String,
}

impl Policy for Invoice {
    fn allows(&self, _user: &User, _ability: &str) -> bool {
        true
    }
}

#[derive(Deserialize, Serialize, Validate)]
struct InvoiceForm {
    #[validate(required, min = 0)]
    total: i64,
    #[validate(required)]
    total_currency: String,
}

struct Invoices;

impl AdminResource for Invoices {
    type Model = Invoice;
    type Form = InvoiceForm;

    fn label(&self) -> &str {
        "Invoice"
    }

    fn plural_label(&self) -> &str {
        "Invoices"
    }

    fn columns(&self) -> Vec<Column> {
        vec![Column::money("total", "Total").currency_from("total_currency")]
    }

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::money("total", "Total")
                .required()
                .currency_from("total_currency"),
            Field::select(
                "total_currency",
                "Currency",
                [("USD", "USD"), ("JPY", "JPY"), ("KWD", "KWD")],
            )
            .default_value("JPY")
            .required(),
        ]
    }

    fn entries(&self) -> Vec<Entry> {
        self.columns()
            .iter()
            .filter_map(Entry::from_column)
            .collect()
    }

    fn fill(&self, invoice: &mut Invoice, form: InvoiceForm) {
        invoice.total = form.total;
        invoice.total_currency = form.total_currency;
    }
}

async fn app() -> TestApp {
    let app = TestApp::new(
        App::new()
            .module(Auth::new())
            .module(
                Admin::new()
                    .authorize(|user| user.email.ends_with("@example.com"))
                    .resource(Invoices),
            )
            .migrations(renox::migrations!("tests/migrations")),
    )
    .await;
    let ana = User::register(app.db(), "Ana", "ana@example.com", "secret-password-1")
        .await
        .unwrap();
    app.acting_as(&ana);
    app
}

async fn invoice(app: &TestApp, total: i64, currency: &str) -> Invoice {
    Invoice::create(
        app.db(),
        Invoice {
            total,
            total_currency: currency.into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

async fn last(app: &TestApp) -> Invoice {
    Invoice::query()
        .order_by_desc("id")
        .first(app.db())
        .await
        .unwrap()
        .unwrap()
}

#[renox::test]
async fn the_list_and_the_view_page_show_each_record_in_its_currency() {
    let app = app().await;
    let yen = invoice(&app, 1250, "JPY").await;
    let dollars = invoice(&app, 1250, "USD").await;
    app.get("/admin/invoices")
        .await
        .assert_ok()
        .assert_see("¥1,250")
        .assert_see("$12.50");
    app.get(&format!("/admin/invoices/{}", yen.id))
        .await
        .assert_ok()
        .assert_see("¥1,250")
        .assert_dont_see("$12.50");
    app.get(&format!("/admin/invoices/{}", dollars.id))
        .await
        .assert_ok()
        .assert_see("$12.50")
        .assert_dont_see("¥1,250");
}

#[renox::test]
async fn the_form_shows_the_amount_with_the_records_currency() {
    let app = app().await;
    let yen = invoice(&app, 1250, "JPY").await;
    let dollars = invoice(&app, 1250, "USD").await;
    let kuwait = invoice(&app, 1250, "KWD").await;
    app.get(&format!("/admin/invoices/{}/edit", yen.id))
        .await
        .assert_see(r#"value="1250""#)
        .assert_dont_see(r#"step="0.01""#);
    app.get(&format!("/admin/invoices/{}/edit", dollars.id))
        .await
        .assert_see(r#"value="12.50""#)
        .assert_see(r#"step="0.01""#);
    app.get(&format!("/admin/invoices/{}/edit", kuwait.id))
        .await
        .assert_see(r#"value="1.250""#)
        .assert_see(r#"step="0.001""#);
    // The create page starts with the currency field's default (JPY).
    app.get("/admin/invoices/create")
        .await
        .assert_ok()
        .assert_dont_see(r#"step="0.01""#);
}

#[renox::test]
async fn the_amount_is_read_with_the_posted_currency() {
    let app = app().await;
    for (total, currency, minor) in [
        ("12.50", "USD", 1250),
        ("1250", "JPY", 1250),
        ("1.250", "KWD", 1250),
        ("0.305", "USD", 31),
    ] {
        app.htmx()
            .post(
                "/admin/invoices",
                &[("total", total), ("total_currency", currency)],
            )
            .await
            .assert_hx_redirect("/admin/invoices");
        let saved = last(&app).await;
        assert_eq!(
            (saved.total, saved.total_currency.as_str()),
            (minor, currency)
        );
    }
    // JSON too.
    app.post_json(
        "/admin/invoices",
        &renox::serde_json::json!({ "total": 12.5, "total_currency": "USD" }),
    )
    .await;
    assert_eq!(last(&app).await.total, 1250);
}
