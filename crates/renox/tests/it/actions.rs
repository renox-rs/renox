//! The UI kit's actions (Filament's as the yardstick): `action_sheet` (a
//! form in a sheet, sent with htmx), slide-overs and sheet widths,
//! `icon_button`, and what every button can carry: an icon, a count, a
//! keyboard shortcut and a reason it is disabled.

use renox::HxRefresh;
use renox::prelude::*;
use renox::testing::TestApp;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Stock {
    change: i64,
    #[serde(default)]
    reason: String,
}

impl Validate for Stock {
    fn rules(&self, v: &mut Validator) {
        v.field("change", &self.change)
            .required()
            .rule(self.change != 0, "Type how many.");
        v.field("reason", &self.reason).max(20);
    }
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "action-pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/products", || async { view("products.html", context! {}) })
            .put(
                "/products/{id}/stock",
                |Path(id): Path<i64>, Valid(form): Valid<Stock>| async move {
                    if form.change < -5 {
                        let mut errors = Errors::new();
                        errors.add("change", "Only 5 in stock.");
                        return Err(ValidationError::new(errors).with_input(&form).into());
                    }
                    Ok::<_, Error>((
                        Toast::success(format!("Product {id}: {:+}", form.change)),
                        HxRefresh,
                    ))
                },
            )
            .name("stock")
    }
}

const PRODUCTS: &str = r##"{% from "renox/ui.html" import action_sheet, icon_button, button, link_button, open_button, sheet, confirm, input %}
{% call action_sheet("stock-7", "Stock", route('stock', 7), "Adjust stock", description="Now 3.", submit_label="Save", method="PUT", icon="box", key="s", width="lg") %}
{{ input("change", "Change", type="number", id="change-7") }}
{% endcall %}
{% call action_sheet("note-7", "Note", "/notes", "Add a note", slide_over=true, modal_icon="info", danger=true) %}{{ input("note", "Note", id="note-7") }}{% endcall %}
{{ icon_button("edit", "Edit Coffee", href="/products/7/edit") }}
{{ icon_button("trash", "Delete Coffee", variant="danger", key="mod+backspace", attrs={"hx-delete": "/products/7"}) }}
{{ icon_button("external", "View Coffee", disabled_reason="Hidden from the shop.") }}
{{ icon_button("refresh", "Reload", disabled=true, badge=2) }}
{{ button("Save", key="mod+s", icon="check") }}
{{ button("Publish", disabled_reason="Add a photo first.") }}
{{ button("Ship", disabled=false, disabled_reason="Pick a carrier first.") }}
{{ button("Refund", disabled=true, disabled_reason="Paid by card: refund at the bank.") }}
{{ icon_button("trash", "Remove Coffee", disabled=false, disabled_reason="Sold already.") }}
{{ button("Archive", disabled=true) }}
{{ link_button("/orders", "Orders", icon="box", badge=4, new_tab=true) }}
{{ link_button("/orders", "None waiting", badge=0) }}
{{ link_button("/orders", "Blank", badge="") }}
{{ open_button("panel", "Filters", icon="settings", badge=3) }}
{% call sheet("panel", "Filters", slide_over=true, width="sm", icon="success") %}…{% endcall %}
{{ confirm("del-7", "Delete", "/products/7", "Delete Coffee?", "Gone for good.", icon="trash") }}
{{ confirm("del-8", "Delete", "/products/8", "Delete Tea?", "Gone for good.", modal_icon=none) }}
{{ toasts() }}"##;

async fn app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("products.html"), PRODUCTS).unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pages), move |c| {
        c.views_path = path;
    })
    .await;
    (app, dir)
}

#[renox::test]
async fn an_action_sheet_is_a_button_and_a_form_in_a_sheet() {
    let (app, _dir) = app().await;
    let page = app.get("/products").await;
    page.assert_ok()
        // The button: its icon, its shortcut, what it opens.
        .assert_see(r#"<button class="rx-button rx-button--secondary" type="button" data-rx-open="stock-7" aria-haspopup="dialog" data-rx-key="s"><span class="rx-button__icon" aria-hidden="true"><svg"#)
        // The sheet and its form: sent with htmx, PUT through _method.
        .assert_see(r#"<dialog class="rx-sheet rx-sheet--lg" id="stock-7" aria-labelledby="stock-7-title" aria-describedby="stock-7-message" data-rx-component="action-sheet">"#)
        .assert_see(r#"<form class="rx-sheet__inner" method="post" action="/products/7/stock" hx-post="/products/7/stock" hx-swap="none" data-rx-action novalidate>"#)
        .assert_see(r#"<input type="hidden" name="_method" value="PUT">"#)
        .assert_see(r#"<p class="rx-sheet__message" id="stock-7-message">Now 3.</p>"#)
        .assert_see(r#"<div class="rx-sheet__body">"#)
        .assert_see(r#"id="change-7""#)
        .assert_see(r#"<button class="rx-button rx-button--primary" type="submit"><span class="rx-button__label">Save</span></button>"#)
        // A slide-over, with an icon over the title and a red button.
        .assert_see(r#"<dialog class="rx-sheet rx-sheet--side" id="note-7""#)
        .assert_see(r#"<span class="rx-sheet__icon rx-sheet__icon--info" aria-hidden="true">"#)
        .assert_see(r#"<button class="rx-button rx-button--danger" type="submit"><span class="rx-button__label">Note</span></button>"#);
    // A POST action has no _method field.
    let body = page.text();
    let note = &body[body.find(r#"id="note-7""#).unwrap()..];
    let note = &note[..note.find("</dialog>").unwrap()];
    assert!(!note.contains("_method"), "{note}");
}

#[renox::test]
async fn icon_buttons_and_buttons_carry_icons_counts_keys_and_reasons() {
    let (app, _dir) = app().await;
    app.get("/products")
        .await
        // An icon link: its label is its name and its tooltip.
        .assert_see(r#"<a class="rx-icon-button rx-icon-button--plain" href="/products/7/edit" aria-label="Edit Coffee" data-rx-tip="Edit Coffee"><svg"#)
        .assert_see(r#"<button class="rx-icon-button rx-icon-button--danger" type="button" aria-label="Delete Coffee" data-rx-tip="Delete Coffee" data-rx-key="mod+backspace" hx-delete="/products/7"><svg"#)
        // Disabled with a reason: focusable, the reason as its tooltip.
        .assert_see(r#"<button class="rx-icon-button rx-icon-button--plain" type="button" aria-label="View Coffee" aria-disabled="true" data-rx-tip="Hidden from the shop."><svg"#)
        .assert_see(r#"aria-label="Reload" data-rx-tip="Reload" disabled><svg"#)
        .assert_see(r#"<span class="rx-button__badge">2</span></button>"#)
        .assert_see(r#"type="submit" data-rx-key="mod+s"><span class="rx-button__icon" aria-hidden="true"><svg"#)
        .assert_see(r#"type="submit" aria-disabled="true" data-rx-tip="Add a photo first."><span class="rx-button__label">Publish</span></button>"#)
        // #308: `disabled=false` wins over a reason (none shown); `disabled=true`
        // with one is focusable with its tooltip.
        .assert_see(r#"type="submit"><span class="rx-button__label">Ship</span></button>"#)
        .assert_dont_see("Pick a carrier first.")
        .assert_see(r#"type="submit" aria-disabled="true" data-rx-tip="Paid by card: refund at the bank."><span class="rx-button__label">Refund</span></button>"#)
        .assert_see(r#"aria-label="Remove Coffee" data-rx-tip="Remove Coffee"><svg"#)
        .assert_dont_see("Sold already.")
        .assert_see(r#"type="submit" disabled><span class="rx-button__label">Archive</span></button>"#)
        .assert_see(r#"<a class="rx-button rx-button--secondary" href="/orders" target="_blank" rel="noopener"><span class="rx-button__icon" aria-hidden="true"><svg"#)
        .assert_see(r#"<span class="rx-button__label">Orders</span><span class="rx-button__badge">4</span></a>"#)
        // Zero is a count; an empty string is none.
        .assert_see(r#"<span class="rx-button__label">None waiting</span><span class="rx-button__badge">0</span></a>"#)
        .assert_see(r#"<span class="rx-button__label">Blank</span></a>"#)
        .assert_see(r#"<span class="rx-button__label">Filters</span><span class="rx-button__badge">3</span></button>"#)
        .assert_see(r#"<dialog class="rx-sheet rx-sheet--side rx-sheet--sm" id="panel" aria-labelledby="panel-title" data-rx-component="sheet">"#)
        .assert_see(r#"<span class="rx-sheet__icon rx-sheet__icon--success" aria-hidden="true">"#);
}

#[renox::test]
async fn confirmations_show_a_warning_icon_unless_told_otherwise() {
    let (app, _dir) = app().await;
    let page = app.get("/products").await;
    let body = page.text();
    let sheet = |id: &str| {
        let at = body
            .find(&format!(r#"<dialog class="rx-sheet" id="{id}""#))
            .unwrap();
        body[at..at + body[at..].find("</dialog>").unwrap()].to_owned()
    };
    assert!(sheet("del-7").contains(r#"rx-sheet__icon--warning"#));
    assert!(!sheet("del-8").contains("rx-sheet__icon"));
    // The opener carries the icon it was given.
    page.assert_see(r#"data-rx-open="del-7" aria-haspopup="dialog"><span class="rx-button__icon" aria-hidden="true">"#);
}

#[renox::test]
async fn an_action_answers_422_into_the_sheet_and_closes_it_on_success() {
    let (app, _dir) = app().await;
    // A rule's message, as JSON for renox.js to put under the field.
    app.htmx()
        .put("/products/7/stock", &[("change", "0")])
        .await
        .assert_status(422)
        .assert_json_path("errors.change.0", "Type how many.");
    // The handler's own check answers the same way.
    app.htmx()
        .put("/products/7/stock", &[("change", "-9")])
        .await
        .assert_status(422)
        .assert_json_path("errors.change.0", "Only 5 in stock.");
    // A success: a toast and a refresh, which closes the sheet.
    let res = app
        .htmx()
        .put(
            "/products/7/stock",
            &[("change", "4"), ("reason", "Delivery")],
        )
        .await;
    res.assert_ok().assert_header("hx-refresh", "true");
    // HX-Refresh reloads the page, so the toast waits in the session.
    app.get("/products").await.assert_see("Product 7: +4");
}
