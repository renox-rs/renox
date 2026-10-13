//! APIs examples/bikeshop needed and worked around (#301–#312): `Display`
//! for `Error`, seeders from modules, several statements on one executor,
//! signed URL or a session on one route, the session in shared view values,
//! users by permission (in scoped_roles.rs) and values hidden by imported
//! macros.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use renox::db::{Executor, sql};
use renox::prelude::*;
use renox::signed::ValidSignature;
use renox::testing::TestApp;

/// #301: `{}` prints the message alone, `{:?}` keeps the causes.
#[test]
fn errors_display_their_message_without_the_chain() {
    let internal =
        Error::Internal(renox::anyhow::anyhow!("disk full").context("saving the invoice"));
    assert_eq!(internal.to_string(), "saving the invoice");
    assert!(format!("{internal:?}").contains("disk full"));
    assert_eq!(
        renox::abort(StatusCode::CONFLICT, "Already rented.").to_string(),
        "Already rented."
    );
    assert_eq!(
        Error::BadRequest("No store.".into()).to_string(),
        "No store."
    );
    assert_eq!(Error::NotFound.to_string(), "Not Found");
    assert_eq!(Error::PageExpired.to_string(), "Page Expired");
    let mut errors = renox::Errors::new();
    errors.add("email", "The email is taken.");
    errors.add("name", "The name is required.");
    errors.add("name", "The name is too short.");
    assert_eq!(
        Error::from(errors).to_string(),
        "email: The email is taken.; name: The name is required.; name: The name is too short."
    );
    assert_eq!(
        Error::from(renox::Errors::new()).to_string(),
        "Unprocessable Entity"
    );
    assert_eq!(
        Error::permanent_message("card declined").to_string(),
        "card declined"
    );
}

/// #302: a module adds seeders; the app's run first, then the modules' in
/// the order they were added.
#[renox::test]
async fn modules_add_seeders_that_run_after_the_apps() {
    struct Area(&'static str, Arc<Mutex<Vec<String>>>);

    impl Module for Area {
        fn name(&self) -> &'static str {
            self.0
        }

        fn register(&self, registry: &mut Registry) {
            let (name, ran) = (self.0, self.1.clone());
            registry.seeder(move |_state| {
                let ran = ran.clone();
                async move {
                    ran.lock().unwrap().push(name.to_owned());
                    Ok(())
                }
            });
        }
    }

    let ran = Arc::new(Mutex::new(Vec::new()));
    let app_ran = ran.clone();
    let app = TestApp::new(
        App::new()
            .module(Area("catalog", ran.clone()))
            .module(Area("stock", ran.clone()))
            .seeder(move |_| {
                let ran = app_ran.clone();
                async move {
                    ran.lock().unwrap().push("app".to_owned());
                    Ok(())
                }
            }),
    )
    .await;
    app.kernel().seed().await.unwrap();
    assert_eq!(*ran.lock().unwrap(), ["app", "catalog", "stock"]);
}

/// #303: a helper takes `&db` or `&mut tx` and runs two statements on it.
async fn record<'c>(db: impl Executor<'c>, product: i64, quantity: i64) -> Result {
    let mut conn = db.into_conn();
    sql("INSERT INTO movements (product_id, quantity) VALUES (?, ?)")
        .bind(product)
        .bind(quantity)
        .execute(conn.reborrow())
        .await?;
    sql("UPDATE levels SET quantity = quantity + ? WHERE product_id = ?")
        .bind(quantity)
        .bind(product)
        .execute(conn.reborrow())
        .await?;
    assert_eq!(conn.dialect(), conn.reborrow().dialect());
    Ok(())
}

#[renox::test]
async fn one_helper_runs_several_statements_on_a_db_or_a_transaction() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    for statement in [
        "CREATE TABLE movements (product_id BIGINT NOT NULL, quantity BIGINT NOT NULL)",
        "CREATE TABLE levels (product_id BIGINT NOT NULL, quantity BIGINT NOT NULL)",
        "INSERT INTO levels (product_id, quantity) VALUES (1, 0)",
    ] {
        sql(statement).execute(db).await.unwrap();
    }
    record(db, 1, 5).await.unwrap();
    let mut tx = db.begin().await.unwrap();
    record(&mut tx, 1, -2).await.unwrap();
    tx.commit().await.unwrap();
    // Rolled back: neither statement stays.
    let mut tx = db.begin().await.unwrap();
    record(&mut tx, 1, 100).await.unwrap();
    tx.rollback().await.unwrap();
    let level: i64 = sql("SELECT quantity FROM levels").scalar(db).await.unwrap();
    let moves: i64 = sql("SELECT COUNT(*) FROM movements")
        .scalar(db)
        .await
        .unwrap();
    assert_eq!((level, moves), (3, 2));
}

/// #307: one route for a signed link or a logged-in user, inside a group
/// (whose prefix axum strips from the request's URI).
#[renox::test]
async fn a_route_takes_a_signed_link_or_a_session() {
    async fn receipt(
        signature: Option<ValidSignature>,
        user: Option<AuthUser>,
        Path(order): Path<i64>,
    ) -> Result<String> {
        let by = match (signature, user) {
            (Some(_), _) => "link",
            (None, Some(_)) => "session",
            (None, None) => return Err(Error::Forbidden),
        };
        Ok(format!("order {order} by {by}"))
    }

    async fn strict(_: ValidSignature) -> &'static str {
        "signed"
    }

    struct Orders;

    impl Module for Orders {
        fn name(&self) -> &'static str {
            "orders"
        }

        fn routes(&self) -> Routes {
            Routes::new().group(
                "/orders",
                "orders.",
                Routes::new()
                    .get("/{order}/receipt", receipt)
                    .get("/{order}/strict", strict),
            )
        }
    }

    let app = TestApp::new(App::new().module(Auth::new()).module(Orders)).await;
    let state = app.state();
    let local = |url: String| url.trim_start_matches(&state.config.url).to_owned();
    let link = local(
        state
            .sign_path("/orders/7/receipt", Duration::from_secs(60))
            .unwrap(),
    );

    app.get(&link)
        .await
        .assert_ok()
        .assert_see("order 7 by link");
    app.get("/orders/7/receipt").await.assert_forbidden();
    // Another order's link doesn't open this one.
    app.get(&link.replace("/7/", "/8/"))
        .await
        .assert_forbidden();
    let strict_link = local(
        state
            .sign_path("/orders/7/strict", Duration::from_secs(60))
            .unwrap(),
    );
    app.get(&strict_link).await.assert_ok();
    app.get("/orders/7/strict").await.assert_forbidden();

    // `verify` for any URL.
    assert!(renox::signed::verify(state, &link.parse().unwrap()));
    assert!(!renox::signed::verify(
        state,
        &"/orders/7/receipt".parse().unwrap()
    ));
    app.travel(Duration::from_secs(120));
    app.get(&link).await.assert_forbidden();
    assert!(
        !app.at_travelled_time(async { renox::signed::verify(state, &link.parse().unwrap()) })
            .await
    );

    let user = User::register(app.db(), "Ana", "ana@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.get("/orders/7/receipt")
        .await
        .assert_see("order 7 by session");
}

/// #309 and #312 need templates.
fn views(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

/// #309: a shared value read from the session (a guest's cart).
#[renox::test]
async fn shared_values_read_the_session() {
    struct Cart;

    impl Module for Cart {
        fn name(&self) -> &'static str {
            "cart"
        }

        fn routes(&self) -> Routes {
            Routes::new()
                .get("/", || async { view("page.html", ()) })
                .post("/cart", |session: Session| async move {
                    session.push("cart", 42)?;
                    Ok::<_, Error>(Redirect::to("/"))
                })
        }
    }

    let dir = views(&[("page.html", "items={{ cart_count }}")]);
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(
        App::new()
            .module(Cart)
            .share("cart_count", |ctx: renox::view::ViewContext| async move {
                let cart: Vec<i64> = ctx.session.and_then(|s| s.get("cart")).unwrap_or_default();
                Ok(cart.len())
            }),
        move |c| c.views_path = path,
    )
    .await;
    app.get("/").await.assert_see("items=0");
    app.post("/cart", &[]).await;
    app.post("/cart", &[]).await;
    app.get("/").await.assert_see("items=2");
}

/// #312: a value named like an imported macro is hidden by it; while
/// debugging Renox warns, and names the cause when the page fails.
#[renox::test]
async fn a_value_hidden_by_an_imported_macro_is_named() {
    struct Pages;

    impl Module for Pages {
        fn name(&self) -> &'static str {
            "pages"
        }

        fn routes(&self) -> Routes {
            Routes::new()
                .get("/history", || async {
                    view(
                        "history.html",
                        context! { history => [context! { label => "sold" }] },
                    )
                })
                .get("/renamed", || async {
                    view("renamed.html", context! { history => ["sold", "rented"] })
                })
                .get("/aliased", || async {
                    view("aliased.html", context! { blocks => 1, title => "x" })
                })
        }
    }

    let dir = views(&[
        (
            "macros.html",
            "{% macro history(items) %}{{ items | length }}{% endmacro %}{% macro title() %}T{% endmacro %}",
        ),
        (
            "history.html",
            "{% from \"macros.html\" import history %}{% for h in history %}[{{ h.label }}]{% endfor %}",
        ),
        (
            "renamed.html",
            "{% from 'macros.html' import history as timeline %}{% for h in history %}[{{ h }}]{% endfor %}",
        ),
        (
            "aliased.html",
            "{%- import \"macros.html\" as blocks -%}{% from \"macros.html\" import history, title with context %}ok",
        ),
    ]);
    let (logs, _logged) = crate::logs::capture();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Pages), move |c| c.views_path = path).await;

    app.get("/history")
        .await
        .assert_status(500)
        .assert_see("history.html imports a macro named like a value the view gets (history)");
    assert!(
        logs.has(&[
            "named like a macro the template imports",
            "history.html",
            "history"
        ]),
        "{}",
        logs.text()
    );

    app.get("/renamed")
        .await
        .assert_ok()
        .assert_see("[sold][rented]");
    assert!(!logs.has(&["renamed.html"]), "{}", logs.text());

    // `import … as` and a list with `with context`: no error, two warnings.
    app.get("/aliased").await.assert_ok().assert_see("ok");
    assert!(
        logs.has(&["aliased.html", "name=blocks"]),
        "{}",
        logs.text()
    );
    assert!(logs.has(&["aliased.html", "name=title"]), "{}", logs.text());

    // Without APP_DEBUG nothing is checked: undefined values print nothing.
    let path = dir.path().to_path_buf();
    let quiet = TestApp::with_config(App::new().module(Pages), move |c| {
        c.views_path = path;
        c.debug = false;
    })
    .await;
    quiet.get("/history").await.assert_ok().assert_see("[][][]");
}
