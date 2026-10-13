//! Stores, staff, RBAC + ABAC pages and the admin panel (#239): who may
//! open what in which store (one store, two stores, the global owner), the
//! matrix changing rights live, invitations, deactivation logging out
//! everywhere, two-factor login required for staff, the admin panel by
//! permission, and an audit row for each sensitive action.

use bikeshop::app::access::catalogue::{CASHIER, MANAGER, MECHANIC, OWNER, STAFF_MANAGE};
use bikeshop::app::access::policy::store_scope;
use bikeshop::app::catalog::factories::variants_of;
use bikeshop::app::catalog::model::{Brand, Category, CategoryKind, Product, ProductVariant};
use bikeshop::app::staff::model::{Staff, Store};
use bikeshop::seed::fixtures;
use renox::prelude::*;
use renox::testing::TestApp;
use renox_2fa::{TwoFactorCredential, totp};

async fn boot() -> TestApp {
    let app = TestApp::new(bikeshop::app()).await;
    fixtures::roles(app.db()).await.unwrap();
    app
}

async fn stores(app: &TestApp) -> (Store, Store) {
    (
        fixtures::store(app.db(), "North").await.unwrap(),
        fixtures::store(app.db(), "South").await.unwrap(),
    )
}

/// The audit rows with `action`, as (role, store_id).
async fn audited(app: &TestApp, action: &str) -> Vec<(Option<String>, Option<i64>)> {
    renox::db::sql("SELECT role, store_id FROM audit_logs WHERE action = ? ORDER BY id")
        .bind(action)
        .fetch_as(app.db())
        .await
        .unwrap()
}

#[renox::test]
async fn who_may_open_what_where() {
    let app = boot().await;
    let (north, south) = stores(&app).await;
    let db = app.db();
    let owner = fixtures::person(db, "owner@example.com", &[(OWNER, None)])
        .await
        .unwrap();
    let manager = fixtures::person(db, "manager@example.com", &[(MANAGER, Some(north.id))])
        .await
        .unwrap();
    let cashier = fixtures::person(db, "cashier@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    // Manager in North, cashier in South.
    let both = fixtures::person(
        db,
        "both@example.com",
        &[(MANAGER, Some(north.id)), (CASHIER, Some(south.id))],
    )
    .await
    .unwrap();

    // (who, page, expected status)
    let matrix: [(&User, &str, u16); 16] = [
        (&owner, "/staff/team", 200),
        (&owner, "/staff/stores", 200),
        (&owner, "/staff/roles", 200),
        (&owner, "/staff/audit", 200),
        (&owner, "/admin", 200),
        (&owner, "/admin/products", 200),
        (&manager, "/staff/team", 200),
        (&manager, "/staff/stores", 403),
        (&manager, "/staff/roles", 403),
        (&manager, "/staff/audit", 403),
        (&manager, "/admin", 200),
        (&manager, "/admin/service-plans", 200),
        (&manager, "/admin/products", 403),
        (&cashier, "/staff/team", 403),
        (&cashier, "/admin", 403),
        (&cashier, "/staff/roles", 403),
    ];
    for (user, page, status) in matrix {
        app.acting_as(user);
        app.get(page).await.assert_status(status);
    }

    // Two stores: rights follow the store being worked in.
    app.acting_as(&both);
    app.get("/staff/team")
        .await
        .assert_ok()
        .assert_see("Team of North");
    app.post(&format!("/staff/store/{}", south.id), &[]).await;
    app.get("/staff/team").await.assert_forbidden();
    app.get("/admin/service-plans").await.assert_forbidden();
    app.post(&format!("/staff/store/{}", north.id), &[]).await;
    app.get("/admin/service-plans").await.assert_ok();

    // A member of another store's team answers 404 in this one.
    let south_staff = fixtures::person(db, "s@example.com", &[(CASHIER, Some(south.id))])
        .await
        .unwrap();
    let theirs = Staff::of_user(db, south_staff.id).await.unwrap().unwrap();
    app.acting_as(&manager);
    app.get(&format!("/staff/team/{}", theirs.id))
        .await
        .assert_not_found();
    // The sidebar shows only what the person may open.
    app.get("/staff")
        .await
        .assert_see("/staff/team")
        .assert_dont_see("/staff/roles");
}

#[renox::test]
async fn the_matrix_changes_rights_live_and_is_audited() {
    let app = boot().await;
    let (north, _) = stores(&app).await;
    let db = app.db();
    let owner = fixtures::person(db, "owner@example.com", &[(OWNER, None)])
        .await
        .unwrap();
    let cashier = fixtures::person(db, "cashier@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    app.acting_as(&cashier);
    app.get("/staff/team").await.assert_forbidden();

    app.acting_as(&owner);
    app.get("/staff/roles")
        .await
        .assert_ok()
        .assert_see("rentals.checkout")
        .assert_see(r#"data-bs-cell="cashier:staff.manage""#);
    app.htmx()
        .post(
            &format!("/staff/roles/{CASHIER}/{STAFF_MANAGE}"),
            &[("granted", "on")],
        )
        .await
        .assert_status(204);
    app.acting_as(&cashier);
    app.get("/staff/team").await.assert_ok();

    // Revoking takes it away again.
    app.acting_as(&owner);
    app.htmx()
        .post(&format!("/staff/roles/{CASHIER}/{STAFF_MANAGE}"), &[])
        .await
        .assert_status(204);
    app.acting_as(&cashier);
    app.get("/staff/team").await.assert_forbidden();

    // The owner can't lock themselves out, and unknown names are 404s.
    app.acting_as(&owner);
    app.htmx()
        .post(&format!("/staff/roles/{OWNER}/roles.manage"), &[])
        .await
        .assert_forbidden();
    app.htmx()
        .post(&format!("/staff/roles/{CASHIER}/rockets.launch"), &[])
        .await
        .assert_not_found();
    // A cashier can't change the matrix at all.
    app.acting_as(&cashier);
    app.htmx()
        .post(
            &format!("/staff/roles/{CASHIER}/{STAFF_MANAGE}"),
            &[("granted", "on")],
        )
        .await
        .assert_forbidden();

    // The audit page shows it.
    app.acting_as(&owner);
    app.get("/staff/audit")
        .await
        .assert_ok()
        .assert_see("role.permission_granted");
    let granted = audited(&app, "role.permission_granted").await;
    assert_eq!(granted.len(), 1);
    assert_eq!(granted[0].0.as_deref(), Some(OWNER), "the role used");
    assert!(granted[0].1.is_some(), "the store worked in");
    assert_eq!(audited(&app, "role.permission_revoked").await.len(), 1);
}

#[renox::test]
async fn roles_are_given_per_store_with_dates_and_audited() {
    let app = boot().await;
    let (north, south) = stores(&app).await;
    let db = app.db();
    let manager = fixtures::person(db, "manager@example.com", &[(MANAGER, Some(north.id))])
        .await
        .unwrap();
    let mechanic = fixtures::person(db, "mech@example.com", &[(MECHANIC, Some(north.id))])
        .await
        .unwrap();
    let staff = Staff::of_user(db, mechanic.id).await.unwrap().unwrap();
    app.acting_as(&manager);
    let page = format!("/staff/team/{}", staff.id);
    // A manager may give the roles they hold every permission of, never the owner's.
    app.get(&page)
        .await
        .assert_ok()
        .assert_see(r#"value="cashier""#)
        .assert_dont_see(r#"value="owner""#);
    let today = renox::db::now().date_naive();
    let week = today + renox::chrono::Duration::days(6);
    app.post(
        &format!("{page}/roles"),
        &[
            ("role", CASHIER),
            ("starts_on", &today.to_string()),
            ("ends_on", &week.to_string()),
        ],
    )
    .await
    .assert_redirect(&page);
    let given = mechanic.assignments(db).await.unwrap();
    let cashier = given
        .iter()
        .find(|a| a.role == CASHIER)
        .expect("the role was given");
    assert_eq!(cashier.scope, store_scope(north.id));
    assert!(cashier.ends_at.is_some());
    // The owner's role can't be given by a manager.
    app.post(&format!("{page}/roles"), &[("role", OWNER)])
        .await
        .assert_forbidden();
    // Taking it away.
    app.delete(&format!("{page}/roles/{CASHIER}"))
        .await
        .assert_redirect(&page);
    assert!(
        !mechanic
            .assignments(db)
            .await
            .unwrap()
            .iter()
            .any(|a| a.role == CASHIER)
    );
    let _ = south;
    assert_eq!(
        audited(&app, "staff.role_assigned").await,
        vec![(Some(MANAGER.to_owned()), Some(north.id))]
    );
    assert_eq!(audited(&app, "staff.role_removed").await.len(), 1);
}

#[renox::test]
async fn an_invitation_makes_a_member_of_staff_once() {
    let app = boot().await;
    let (north, _) = stores(&app).await;
    let manager = fixtures::person(
        app.db(),
        "manager@example.com",
        &[(MANAGER, Some(north.id))],
    )
    .await
    .unwrap();
    app.acting_as(&manager);
    app.get("/staff/team/invite").await.assert_ok();
    app.post(
        "/staff/team/invite",
        &[("email", "new@example.com"), ("role", OWNER)],
    )
    .await
    .assert_forbidden();
    app.post(
        "/staff/team/invite",
        &[("email", "New@Example.com"), ("role", CASHIER)],
    )
    .await
    .assert_redirect("/staff/team");
    app.run_jobs().await;
    let mail = app
        .sent_mail()
        .into_iter()
        .find(|m| m.to.contains(&"new@example.com".to_owned()))
        .expect("the invitation was mailed");
    let html = mail.html.unwrap_or_default();
    let start = html.find("/staff/join/").expect("a join link");
    let end = start + html[start..].find('"').unwrap();
    let link = html[start..end].replace("&amp;", "&");

    app.logout();
    app.get(&link).await.assert_ok().assert_see("Join North");
    // The role in the link can't be changed.
    app.get(&link.replace("/cashier/", "/manager/"))
        .await
        .assert_forbidden();
    app.post(
        &link,
        &[
            ("name", "Noa"),
            ("password", "password123"),
            ("password_confirmation", "password123"),
        ],
    )
    .await
    .assert_redirect("/login");
    let noa = User::query()
        .where_eq("email", "new@example.com")
        .first(app.db())
        .await
        .unwrap()
        .expect("the account was made");
    assert!(noa.email_verified_at.is_some());
    let staff = Staff::of_user(app.db(), noa.id).await.unwrap().unwrap();
    assert_eq!(staff.home_store_id, north.id);
    assert!(
        noa.assignments(app.db())
            .await
            .unwrap()
            .iter()
            .any(|a| a.role == CASHIER && a.scope == store_scope(north.id))
    );
    // Once only.
    app.post(&link, &[]).await.assert_not_found();
    app.get(&link).await.assert_see("already accepted");
    assert_eq!(audited(&app, "staff.invited").await.len(), 1);
}

#[renox::test]
async fn deactivating_logs_them_out_everywhere() {
    let app = boot().await;
    let (north, _) = stores(&app).await;
    let db = app.db();
    let manager = fixtures::person(db, "manager@example.com", &[(MANAGER, Some(north.id))])
        .await
        .unwrap();
    let cashier = fixtures::person(db, "cashier@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    let staff = Staff::of_user(db, cashier.id).await.unwrap().unwrap();

    // The cashier is logged in on some device.
    app.acting_as(&cashier);
    app.get("/staff").await.assert_ok();
    let cashiers_device = app.session_cookie();

    app.acting_as(&manager);
    // Nobody deactivates themselves.
    let mine = Staff::of_user(db, manager.id).await.unwrap().unwrap();
    app.post(&format!("/staff/team/{}/deactivate", mine.id), &[])
        .await
        .assert_forbidden();
    app.post(&format!("/staff/team/{}/deactivate", staff.id), &[])
        .await
        .assert_redirect(&format!("/staff/team/{}", staff.id));
    let staff = Staff::find(db, staff.id).await.unwrap().unwrap();
    assert!(!staff.active);
    assert!(cashier.assignments(db).await.unwrap().is_empty());
    assert_eq!(audited(&app, "staff.deactivated").await.len(), 1);

    // The cashier's session is over.
    app.use_session_cookie(cashiers_device);
    app.get("/staff").await.assert_redirect("/login");
}

#[renox::test]
async fn staff_must_turn_on_two_factor_login_at_login() {
    let app = boot().await;
    let (north, _) = stores(&app).await;
    let cashier = fixtures::person(
        app.db(),
        "cashier@example.com",
        &[(CASHIER, Some(north.id))],
    )
    .await
    .unwrap();
    app.post(
        "/login",
        &[
            ("email", "cashier@example.com"),
            ("password", "password123"),
        ],
    )
    .await;
    app.assert_authenticated(None);
    app.get("/staff").await.assert_redirect("/account");
    app.get("/admin").await.assert_redirect("/account");
    // The account page and the setup are open.
    app.get("/account").await.assert_ok();

    // Turning it on lifts the block.
    app.confirm_password();
    app.post("/two-factor/enable", &[]).await;
    let secret = TwoFactorCredential::of(app.db(), cashier.id)
        .await
        .unwrap()
        .unwrap()
        .secret
        .to_string();
    let code = totp::code_at(&secret, totp::step_at(renox::db::now().timestamp())).unwrap();
    app.post("/two-factor/confirm", &[("code", code.as_str())])
        .await
        .assert_redirect("/two-factor/recovery-codes");
    app.get("/staff").await.assert_ok();

    // Customers are never asked.
    app.logout();
    User::register(app.db(), "Cleo", "cleo@example.com", "password123")
        .await
        .unwrap();
    app.post(
        "/login",
        &[("email", "cleo@example.com"), ("password", "password123")],
    )
    .await;
    app.get("/account").await.assert_ok();
    let noted: Option<bool> = app
        .state()
        .cache
        .get(&bikeshop::app::staff::two_factor::note_key(
            User::query()
                .where_eq("email", "cleo@example.com")
                .first(app.db())
                .await
                .unwrap()
                .unwrap()
                .id,
        ))
        .await
        .unwrap();
    assert!(noted.is_none());
}

#[renox::test]
async fn two_factor_for_staff_can_be_made_optional() {
    let app = TestApp::with_config(bikeshop::app(), |c| {
        c.vars
            .insert("BIKESHOP_STAFF_2FA".into(), "optional".into());
    })
    .await;
    fixtures::roles(app.db()).await.unwrap();
    let (north, _) = stores(&app).await;
    fixtures::person(
        app.db(),
        "cashier@example.com",
        &[(CASHIER, Some(north.id))],
    )
    .await
    .unwrap();
    app.post(
        "/login",
        &[
            ("email", "cashier@example.com"),
            ("password", "password123"),
        ],
    )
    .await;
    app.get("/staff").await.assert_ok();
}

#[renox::test]
async fn store_hours_and_fee_rate_are_saved_and_the_fee_is_audited() {
    let app = boot().await;
    let (north, _) = stores(&app).await;
    let owner = fixtures::person(app.db(), "owner@example.com", &[(OWNER, None)])
        .await
        .unwrap();
    app.acting_as(&owner);
    app.get("/staff/stores")
        .await
        .assert_ok()
        .assert_see("North");
    let edit = format!("/staff/stores/{}/edit", north.id);
    app.get(&edit).await.assert_ok().assert_see("fee_percent");
    let form = [
        ("_method", "PUT"),
        ("name", "North"),
        ("phone", "+34 900 000 001"),
        ("email", "north@bikeshop.test"),
        ("line1", "1 Main Street"),
        ("workshop_minutes_per_day", "480"),
        ("fee_percent", "17.5"),
        ("hours[0][day]", "mon"),
        ("hours[0][opens]", "09:00"),
        ("hours[0][closes]", "18:00"),
        ("hours[1][day]", "sat"),
        ("hours[1][opens]", "10:00"),
        ("hours[1][closes]", "14:00"),
    ];
    app.post(&format!("/staff/stores/{}", north.id), &form)
        .await
        .assert_redirect("/staff/stores");
    let store = Store::find(app.db(), north.id).await.unwrap().unwrap();
    assert_eq!(store.fee_rate_bp, 1_750);
    assert_eq!(store.workshop_minutes_per_day, 480);
    assert_eq!(store.opening_hours.0.len(), 2);
    assert_eq!(store.opening_hours.0[1].day, "sat");
    let fee = audited(&app, "store.fee_rate_changed").await;
    assert_eq!(fee.len(), 1);
    assert_eq!(fee[0].0.as_deref(), Some(OWNER));

    // A wrong time is refused, in its row.
    let mut bad = form.to_vec();
    bad[8] = ("hours[0][opens]", "9am");
    app.htmx()
        .post(&format!("/staff/stores/{}", north.id), &bad)
        .await
        .assert_invalid("hours.0.opens");
}

#[renox::test]
async fn the_admin_panel_edits_the_catalogue_by_permission() {
    let app = boot().await;
    let (north, _) = stores(&app).await;
    let db = app.db();
    let owner = fixtures::person(db, "owner@example.com", &[(OWNER, None)])
        .await
        .unwrap();
    let category = Category::create(
        db,
        Category {
            name: "Road".into(),
            slug: "road".into(),
            kind: CategoryKind::Bike,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let brand = Brand::create(
        db,
        Brand {
            name: "Velo".into(),
            slug: "velo".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    app.acting_as(&owner);
    // The description field is the Markdown editor.
    app.get("/admin/products/create")
        .await
        .assert_ok()
        .assert_see("rx-markdown")
        .assert_see("About this page");
    let category_id = category.id.to_string();
    let brand_id = brand.id.to_string();
    app.post(
        "/admin/products",
        &[
            ("name", "Velo Sprint"),
            ("slug", "velo-sprint"),
            ("category_id", &category_id),
            ("brand_id", &brand_id),
            ("description", "A **fast** road bike."),
        ],
    )
    .await;
    let product = Product::query()
        .where_(Product::SLUG.eq("velo-sprint"))
        .first(db)
        .await
        .unwrap()
        .expect("the product was saved");
    assert_eq!(product.description, "A **fast** road bike.");
    let variant = variants_of(product.id)
        .state(|v| v.price = 100_000)
        .create_one(db)
        .await
        .unwrap();

    // Prices +10 %, audited.
    let ids = product.id.to_string();
    app.post(
        "/admin/products/actions/prices",
        &[("ids", &ids), ("percent", "10")],
    )
    .await
    .assert_status(204);
    let variant = ProductVariant::find(db, variant.id).await.unwrap().unwrap();
    assert_eq!(variant.price, 110_000);
    assert_eq!(audited(&app, "catalog.prices_changed").await.len(), 1);

    // A manager (plans.manage and prices.change in North, no catalog.manage)
    // may open the panel's plans but not change the catalogue's prices.
    let manager = fixtures::person(db, "manager@example.com", &[(MANAGER, Some(north.id))])
        .await
        .unwrap();
    app.acting_as(&manager);
    app.get("/admin/service-plans").await.assert_ok();
    app.post(
        "/admin/products/actions/prices",
        &[("ids", &ids), ("percent", "10")],
    )
    .await
    .assert_forbidden();
    app.acting_as(&owner);

    // Discontinue: to the trash, restorable.
    app.post("/admin/products/actions/discontinue", &[("ids", &ids)])
        .await
        .assert_status(204);
    assert!(Product::find(db, product.id).await.unwrap().is_none());
    app.get("/admin/products?filter=trashed")
        .await
        .assert_ok()
        .assert_see("Velo Sprint");
}

#[renox::test]
async fn what_fits_is_edited_from_the_part() {
    let app = boot().await;
    let db = app.db();
    let owner = fixtures::person(db, "owner@example.com", &[(OWNER, None)])
        .await
        .unwrap();
    let part_category = Category::create(
        db,
        Category {
            name: "Chains".into(),
            slug: "chains".into(),
            kind: CategoryKind::Part,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let bike_category = Category::create(
        db,
        Category {
            name: "City".into(),
            slug: "city".into(),
            kind: CategoryKind::Bike,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let brand = Brand::create(
        db,
        Brand {
            name: "B".into(),
            slug: "b".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let chain = Product::create(
        db,
        Product {
            name: "Chain 11s".into(),
            slug: "chain-11s".into(),
            category_id: part_category.id,
            brand_id: brand.id,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let bike = Product::create(
        db,
        Product {
            name: "City One".into(),
            slug: "city-one".into(),
            category_id: bike_category.id,
            brand_id: brand.id,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    app.acting_as(&owner);
    let page = format!("/staff/catalog/fits/{}", chain.id);
    app.get(&page).await.assert_ok().assert_see("City One");
    let bike_id = bike.id.to_string();
    app.post(&page, &[("other_id", &bike_id), ("note", "11-speed only")])
        .await
        .assert_redirect(&page);
    // Seen from the bike too.
    app.get(&format!("/staff/catalog/fits/{}", bike.id))
        .await
        .assert_see("Chain 11s")
        .assert_see("11-speed only");
    app.delete(&format!("{page}/{}", bike.id))
        .await
        .assert_redirect(&page);
    let left: i64 = renox::db::sql("SELECT COUNT(*) FROM part_fits")
        .scalar(db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
