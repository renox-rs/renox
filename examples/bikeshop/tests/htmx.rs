//! `/about/htmx` (#351, from the htmx-recipes example): each recipe answers
//! htmx with the smallest fragment and a plain request with a redirect. The
//! checklist lives in the session and starts with five items, the first
//! done (`Checklist::default`).

use bikeshop::app::about::htmx::{Checklist, MAX_ITEMS, PER_LOAD};
use bikeshop::app::catalog::model::Product;
use renox::prelude::*;
use renox::testing::{TestApp, TestResponse};

fn toast(res: &TestResponse) -> String {
    let trigger: renox::serde_json::Value =
        renox::serde_json::from_str(res.header("hx-trigger").unwrap()).unwrap();
    trigger["renox:toast"]["toasts"][0]["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[renox::test]
async fn the_page_shows_the_checklist_and_loads_more_bikes_as_you_scroll() {
    let app = TestApp::new(bikeshop::app()).await;
    bikeshop::seed::run(app.state().clone()).await.unwrap();
    let ids: Vec<i64> = Product::query()
        .order_by_desc(Product::ID)
        .get(app.db())
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert!(ids.len() as u64 > PER_LOAD * 2, "the seed has enough bikes");
    let cursor = ids[PER_LOAD as usize - 1];

    app.get("/about/htmx")
        .await
        .assert_ok()
        .assert_view("about/htmx.html")
        .assert_see(">Pump the tyres<")
        .assert_see(r#"x-data="checklist""#)
        .assert_see(">4 to do<")
        .assert_see(&format!(
            r#"hx-get="/about/htmx/bikes?before={cursor}" hx-trigger="revealed""#
        ));

    // The loader asks with htmx and gets only the next bikes (by id, so a
    // bike added meanwhile never repeats a row), and no loader after the last.
    let more = app
        .htmx()
        .get(&format!("/about/htmx/bikes?before={cursor}"))
        .await;
    more.assert_ok().assert_dont_see("<html");
    let last = *ids.last().unwrap();
    let tail = app
        .htmx()
        .get(&format!("/about/htmx/bikes?before={}", ids[ids.len() - 2]))
        .await;
    let last_slug = Product::find_or_404(app.db(), last).await.unwrap().slug;
    tail.assert_see(&format!(r#"href="/products/{last_slug}""#))
        .assert_dont_see("before=");
}

#[renox::test]
async fn the_modal_adds_a_row_and_a_repeat_is_retargeted() {
    let app = TestApp::new(bikeshop::app()).await;
    let res = app
        .htmx()
        .post("/about/htmx/items", &[("title", "Fill the bottle")])
        .await;
    res.assert_ok()
        .assert_header("hx-trigger", "item-added")
        .assert_see(r#"<li id="item-6""#)
        .assert_see(">Fill the bottle<")
        .assert_see(r#"id="open-count" hx-swap-oob="true">5 to do</span>"#)
        .assert_dont_see("<html");

    // The same item again: the server retargets the answer to the existing
    // row (HX-Retarget, HX-Reswap) instead of adding a copy, and says why in
    // a toast, all in one HX-Trigger.
    let res = app
        .htmx()
        .post("/about/htmx/items", &[("title", " fill the BOTTLE ")])
        .await;
    res.assert_ok()
        .assert_header("hx-retarget", "#item-6")
        .assert_header("hx-reswap", "outerHTML")
        .assert_see(r#"<li id="item-6""#);
    let trigger: renox::serde_json::Value =
        renox::serde_json::from_str(res.header("hx-trigger").unwrap()).unwrap();
    assert!(trigger.get("item-added").is_some(), "{trigger}");
    assert_eq!(toast(&res), "That item is already on the list.");

    // Errors come back as 422 JSON, shown in the modal's form.
    app.htmx()
        .post("/about/htmx/items", &[("title", "")])
        .await
        .assert_invalid("title");
    // Without JavaScript it's a normal form post, duplicates skipped too.
    app.post("/about/htmx/items", &[("title", "Wipe the saddle")])
        .await
        .assert_redirect("/about/htmx");
    app.post("/about/htmx/items", &[("title", "Wipe the saddle")])
        .await
        .assert_redirect("/about/htmx");
    app.get("/about/htmx")
        .await
        .assert_see(">Wipe the saddle<")
        .assert_see(">6 to do<");

    // The list is a cookie: it holds at most twelve items.
    for n in 0..MAX_ITEMS {
        app.htmx()
            .post("/about/htmx/items", &[("title", &format!("Extra {n}"))])
            .await;
    }
    app.htmx()
        .post("/about/htmx/items", &[("title", "One too many")])
        .await
        .assert_invalid("title");
}

#[renox::test]
async fn items_are_renamed_in_place() {
    let app = TestApp::new(bikeshop::app()).await;
    app.htmx()
        .get("/about/htmx/items/2/edit")
        .await
        .assert_see(r#"value="Check the brakes""#)
        .assert_see(r#"hx-patch="/about/htmx/items/2""#)
        .assert_see(" data-bs-cancel ")
        .assert_see(r#"hx-get="/about/htmx/items/2""#)
        .assert_see(r#"hx-trigger="bs-cancel""#);
    app.htmx()
        .patch("/about/htmx/items/2", &[("title", "  Check both brakes ")])
        .await
        .assert_ok()
        .assert_see(">Check both brakes<")
        .assert_see(r#"hx-trigger="dblclick""#);
    app.htmx()
        .patch("/about/htmx/items/2", &[("title", &"x".repeat(61))])
        .await
        .assert_invalid("title");
    // Escape: the row as it is.
    app.htmx()
        .get("/about/htmx/items/2")
        .await
        .assert_see(">Check both brakes<");
    app.htmx()
        .get("/about/htmx/items/99/edit")
        .await
        .assert_not_found();
}

#[renox::test]
async fn checkboxes_toggle_and_rows_delete_in_place() {
    let app = TestApp::new(bikeshop::app()).await;
    app.htmx()
        .patch("/about/htmx/items/2/toggle", &[])
        .await
        .assert_see(r#"class="rx-list__main bs-checklist__done""#)
        .assert_see(r#"x-show="showsDone""#)
        .assert_see("checked")
        .assert_see(">3 to do<");
    app.htmx()
        .patch("/about/htmx/items/2/toggle", &[])
        .await
        .assert_dont_see("checked")
        .assert_see(">4 to do<");

    // The row's part of the answer is empty, so htmx swaps the row for
    // nothing; the count and the empty note come along out of band, and a
    // toast says it.
    let res = app.htmx().delete("/about/htmx/items/2").await;
    res.assert_ok();
    assert_eq!(
        res.text(),
        r#"<span class="rx-badge rx-badge--info" id="open-count" hx-swap-oob="true">3 to do</span><p id="checklist-empty" class="rx-subtitle" hidden hx-swap-oob="true">Nothing on the list. Add an item.</p>"#
    );
    assert_eq!(toast(&res), "“Check the brakes” deleted.");
    app.delete("/about/htmx/items/3")
        .await
        .assert_redirect("/about/htmx");
    // The last one gone, the empty note shows again.
    for id in [4, 5] {
        app.htmx().delete(&format!("/about/htmx/items/{id}")).await;
    }
    app.htmx()
        .delete("/about/htmx/items/1")
        .await
        .assert_see(r#"<p id="checklist-empty" class="rx-subtitle" hx-swap-oob="true">"#);
}

#[renox::test]
async fn bulk_actions_refresh_or_redirect() {
    let app = TestApp::new(bikeshop::app()).await;
    app.htmx().patch("/about/htmx/items/2/toggle", &[]).await;
    app.htmx()
        .post("/about/htmx/clear-done", &[])
        .await
        .assert_header("hx-refresh", "true");
    // The toast waited in the session for the reloaded page.
    app.get("/about/htmx")
        .await
        .assert_see("2 done items cleared.")
        .assert_dont_see(">Pump the tyres<");

    app.htmx().patch("/about/htmx/items/3/toggle", &[]).await;
    app.htmx()
        .post("/about/htmx/ready", &[])
        .await
        .assert_hx_redirect("/shop");
    app.get("/shop")
        .await
        .assert_see("One item checked. Have a good ride.");
    // The list starts afresh, and a plain form gets a 303.
    app.get("/about/htmx").await.assert_see(">Pump the tyres<");
    app.post("/about/htmx/ready", &[])
        .await
        .assert_redirect("/shop");
    assert_eq!(Checklist::default().items.len(), 5);
}

#[renox::test]
async fn the_live_checklist_shares_the_session_list_and_runs_its_actions() {
    use bikeshop::app::about::live::LiveChecklist;
    let app = TestApp::new(bikeshop::app()).await;
    app.get("/about/htmx/live")
        .await
        .assert_ok()
        .assert_view("about/live.html")
        .assert_see(">Pump the tyres<")
        .assert_see("data-rx-live=\"checklist\"");

    let mut live = app.live(LiveChecklist::default());
    live.set("title", "Fill the bottle");
    live.call("add")
        .await
        .assert_ok()
        .assert_see(">Fill the bottle<")
        .assert_see(">5 to do<");
    assert_eq!(live.component().title, "");
    // The htmx page reads the same list.
    app.get("/about/htmx").await.assert_see(">Fill the bottle<");

    // Empty: a 422 under the field.
    live.set("title", "  ");
    live.call("add").await.assert_invalid("title");

    live.call_with("toggle", renox::serde_json::json!([2]))
        .await
        .assert_ok()
        .assert_see(">4 to do<");

    live.call_with("edit", renox::serde_json::json!([2]))
        .await
        .assert_ok()
        .assert_see(r#"value="Check the brakes""#);
    assert_eq!(live.component().editing, Some(2));
    live.set("edit_title", "");
    live.call("save_edit").await.assert_invalid("edit_title");
    live.set("edit_title", " Check both brakes ");
    live.call("save_edit")
        .await
        .assert_ok()
        .assert_see(">Check both brakes<");
    assert_eq!(live.component().editing, None);

    live.call_with("show", renox::serde_json::json!(["done"]))
        .await
        .assert_ok()
        .assert_see(">Check both brakes<")
        .assert_dont_see(">Oil the chain<");

    live.call_with("delete", renox::serde_json::json!([2]))
        .await
        .assert_ok()
        .assert_dont_see(">Check both brakes<");
    live.call_with("delete", renox::serde_json::json!([99]))
        .await
        .assert_not_found();
}
