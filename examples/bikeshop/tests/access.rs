//! RBAC + ABAC (#232, #239, #245): the permission catalogue, the staff
//! guard and the active store, the store switcher, the policy helpers
//! against the store attribute that matters (owner / location /
//! operating), dated roles under `TestApp::travel`, lists filtered to "mine
//! or at my store", and no role-name checks anywhere in the app.

use bikeshop::app::access::catalogue::{
    self, CASHIER, FLEET_PLACE, FLEET_VIEW, MANAGER, MECHANIC, OWNER, PRICES_CHANGE,
    RENTALS_CHECKOUT, STAFF, STAFF_ACCESS,
};
use bikeshop::app::access::{self, StoreAttr, active_store};
use bikeshop::app::rentals::factories::{RentalStates, rentals};
use bikeshop::app::rentals::model::{Rental, RentalBike};
use bikeshop::app::sales::factories::{OrderStates, orders};
use bikeshop::app::sales::model::Order;
use bikeshop::app::staff::model::Store;
use bikeshop::app::stock::factories::{StockStates, stock_levels};
use bikeshop::app::stock::model::StockLevel;
use bikeshop::app::workshop::factories::{WorkOrderStates, work_orders};
use bikeshop::app::workshop::model::WorkOrder;
use bikeshop::seed::fixtures;
use renox::prelude::*;
use renox::testing::TestApp;
use std::path::{Path as FsPath, PathBuf};
use std::time::Duration;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Routes only the tests have, to drive the helpers through real requests.
struct Probe;

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn routes(&self) -> Routes {
        let staff = access::staff_routes(
            Routes::new()
                .get("/probe/active", active)
                .name("probe.active"),
        );
        let records = Routes::new()
            .get("/probe/bikes/{id}/{permission}/{attr}", act_on_bike)
            .name("probe.bike")
            .get("/probe/bikes", list_bikes)
            .name("probe.bikes")
            .get("/probe/records/{kind}/{id}", see_record)
            .name("probe.record")
            .require_auth();
        staff.merge(records)
    }
}

/// The active store and what the person may do there.
async fn active(user: AuthUser) -> String {
    format!(
        "store={:?} refund={} checkout={}",
        active_store::current(),
        user.allows(catalogue::ORDERS_REFUND),
        user.allows(RENTALS_CHECKOUT),
    )
}

/// `require(permission, attr)` on one bike: 404, 403 or "ok".
async fn act_on_bike(
    State(db): State<Db>,
    user: AuthUser,
    Path((id, permission, attr)): Path<(i64, String, String)>,
) -> Result<&'static str> {
    let bike = access::find::<RentalBike>(&db, &user, id).await?;
    let attr = match attr.as_str() {
        "owner" => StoreAttr::Owner,
        "location" => StoreAttr::Location,
        _ => StoreAttr::Operating,
    };
    access::require(&user, &permission, attr, &bike)?;
    Ok("ok")
}

/// Whether the person may see one record of any kind: 404 or "seen".
async fn see_record(
    State(db): State<Db>,
    user: AuthUser,
    Path((kind, id)): Path<(String, i64)>,
) -> Result<&'static str> {
    match kind.as_str() {
        "rentals" => access::find::<Rental>(&db, &user, id)
            .await
            .map(|_: Rental| ())?,
        "orders" => access::find::<Order>(&db, &user, id)
            .await
            .map(|_: Order| ())?,
        "work_orders" => access::find::<WorkOrder>(&db, &user, id)
            .await
            .map(|_: WorkOrder| ())?,
        "stock_levels" => access::find::<StockLevel>(&db, &user, id)
            .await
            .map(|_: StockLevel| ())?,
        _ => access::find::<RentalBike>(&db, &user, id)
            .await
            .map(|_: RentalBike| ())?,
    }
    Ok("seen")
}

/// The ids of the bikes the person may see.
async fn list_bikes(State(db): State<Db>) -> Result<String> {
    let ids: Vec<i64> = access::visible::<RentalBike>(FLEET_VIEW)
        .order_by(RentalBike::ID)
        .pluck(&db, RentalBike::ID)
        .await?;
    Ok(format!("{ids:?}"))
}

async fn boot() -> TestApp {
    let app = TestApp::new(bikeshop::app().module(Probe)).await;
    fixtures::roles(app.db()).await.unwrap();
    app
}

/// Three stores: A (north), B (south), C (west).
async fn stores(app: &TestApp) -> (Store, Store, Store) {
    let db = app.db();
    (
        fixtures::store(db, "North").await.unwrap(),
        fixtures::store(db, "South").await.unwrap(),
        fixtures::store(db, "West").await.unwrap(),
    )
}

#[test]
fn the_catalogue_is_consistent() {
    let names = catalogue::all_permission_names();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "a permission is listed twice");
    for role in catalogue::roles() {
        for permission in role.permissions {
            assert!(
                names.contains(permission),
                "{} grants unknown {permission}",
                role.name
            );
        }
        if role.name != OWNER {
            assert!(!role.global, "only the owner's role is global");
        }
        assert!(
            role.permissions.contains(&STAFF_ACCESS),
            "{} can't open the staff side",
            role.name
        );
    }
    let owner = catalogue::roles()
        .into_iter()
        .find(|r| r.name == OWNER)
        .unwrap();
    let mut granted: Vec<&str> = owner.permissions.to_vec();
    granted.sort_unstable();
    assert_eq!(granted, sorted, "the owner's role grants every permission");
}

#[renox::test]
async fn the_staff_side_needs_staff_access_in_a_store() {
    let app = boot().await;
    let (north, _, _) = stores(&app).await;
    app.get("/staff").await.assert_redirect("/login");

    // A customer with a login is not staff.
    let customer = User::register(app.db(), "Cleo", "cleo@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&customer);
    app.get("/staff").await.assert_forbidden();

    let cashier = fixtures::person(
        app.db(),
        "cashier@example.com",
        &[(CASHIER, Some(north.id))],
    )
    .await
    .unwrap();
    app.acting_as(&cashier);
    app.get("/staff")
        .await
        .assert_ok()
        // One store: the switcher is a label, not a menu.
        .assert_see("Working in North")
        .assert_dont_see("id=\"store-menu\"");
    app.get("/probe/active").await.assert_see(&format!(
        "store=Some({}) refund=false checkout=true",
        north.id
    ));
}

#[renox::test]
async fn rights_come_from_the_active_store_only() {
    let app = boot().await;
    let (north, south, west) = stores(&app).await;
    // Manager of North, staff at South (#245's "manager of A helping B").
    let ana = fixtures::person(
        app.db(),
        "ana@example.com",
        &[(MANAGER, Some(north.id)), (STAFF, Some(south.id))],
    )
    .await
    .unwrap();
    app.acting_as(&ana);

    // Starts in the home store, as its manager.
    app.get("/probe/active").await.assert_see(&format!(
        "store=Some({}) refund=true checkout=true",
        north.id
    ));
    let page = app.get("/staff").await;
    page.assert_see("id=\"store-menu\"")
        .assert_see("Working in North")
        .assert_see(&format!("/staff/store/{}", south.id))
        .assert_dont_see(&format!("/staff/store/{}\"", west.id));

    // Switched to South: staff rights there, no refunds.
    app.post(&format!("/staff/store/{}", south.id), &[])
        .await
        .assert_status(303);
    app.get("/probe/active").await.assert_see(&format!(
        "store=Some({}) refund=false checkout=true",
        south.id
    ));
    app.get("/staff").await.assert_see("Working in South");

    // A store without a role can't be picked.
    app.post(&format!("/staff/store/{}", west.id), &[])
        .await
        .assert_forbidden();
    app.get("/probe/active")
        .await
        .assert_see(&format!("store=Some({})", south.id));
}

#[renox::test]
async fn the_owner_works_everywhere_without_a_role_name_check() {
    let app = boot().await;
    let (north, south, west) = stores(&app).await;
    let owner = fixtures::person(app.db(), "boss@example.com", &[(OWNER, None)])
        .await
        .unwrap();
    app.acting_as(&owner);
    let page = app.get("/staff").await;
    page.assert_ok();
    for store in [&north, &south, &west] {
        page.assert_see(&format!("/staff/store/{}", store.id));
    }
    app.post(&format!("/staff/store/{}", west.id), &[])
        .await
        .assert_status(303);
    app.get("/probe/active").await.assert_see(&format!(
        "store=Some({}) refund=true checkout=true",
        west.id
    ));
}

#[renox::test]
async fn each_action_is_checked_in_the_store_that_matters() {
    let app = boot().await;
    let db = app.db();
    let (a, b, c) = stores(&app).await;
    // A bike owned by A, placed at B.
    let bike = fixtures::bike(db, a.id, b.id).await.unwrap();
    let manager_a = fixtures::person(db, "ma@example.com", &[(MANAGER, Some(a.id))])
        .await
        .unwrap();
    let manager_b = fixtures::person(db, "mb@example.com", &[(MANAGER, Some(b.id))])
        .await
        .unwrap();
    let cashier_b = fixtures::person(db, "cb@example.com", &[(CASHIER, Some(b.id))])
        .await
        .unwrap();
    let manager_c = fixtures::person(db, "mc@example.com", &[(MANAGER, Some(c.id))])
        .await
        .unwrap();
    let owner = fixtures::person(db, "o@example.com", &[(OWNER, None)])
        .await
        .unwrap();

    let url =
        |permission: &str, attr: &str| format!("/probe/bikes/{}/{permission}/{attr}", bike.id);
    // (who, change the price [owner store], place it elsewhere [owner store],
    //  rent it out [location store]) → status.
    let cases = [
        (&manager_a, 200, 200, 403),
        (&manager_b, 403, 403, 200),
        (&cashier_b, 403, 403, 200),
        (&manager_c, 404, 404, 404),
        (&owner, 200, 200, 200),
    ];
    for (user, price, place, rent) in cases {
        app.acting_as(user);
        app.get(&url(PRICES_CHANGE, "owner"))
            .await
            .assert_status(price);
        app.get(&url(FLEET_PLACE, "owner"))
            .await
            .assert_status(place);
        app.get(&url(RENTALS_CHECKOUT, "location"))
            .await
            .assert_status(rent);
    }
}

#[renox::test]
async fn records_of_other_stores_answer_404() {
    let app = boot().await;
    let db = app.db();
    let (a, _, c) = stores(&app).await;
    let bike = fixtures::bike(db, a.id, a.id).await.unwrap();
    let customer = bikeshop::app::accounts::factories::customers()
        .create_one(db)
        .await
        .unwrap();
    let rental = rentals()
        .of_bike(&bike)
        .for_customer(customer.id)
        .active()
        .create_one(db)
        .await
        .unwrap();
    let order = orders().at(a.id).paid().create_one(db).await.unwrap();
    let work = work_orders()
        .fleet_repair(&bike, a.id)
        .waiting_parts()
        .create_one(db)
        .await
        .unwrap();
    let level = stock_levels()
        .of(bike.variant_id, a.id)
        .low()
        .create_one(db)
        .await
        .unwrap();
    let records = [
        ("rental_bikes", bike.id),
        ("rentals", rental.id),
        ("orders", order.id),
        ("work_orders", work.id),
        ("stock_levels", level.id),
    ];
    let of_a = fixtures::person(db, "a@example.com", &[(STAFF, Some(a.id))])
        .await
        .unwrap();
    let of_c = fixtures::person(db, "c@example.com", &[(MANAGER, Some(c.id))])
        .await
        .unwrap();
    for (kind, id) in records {
        let url = format!("/probe/records/{kind}/{id}");
        app.acting_as(&of_a);
        app.get(&url).await.assert_ok().assert_see("seen");
        app.acting_as(&of_c);
        app.get(&url).await.assert_not_found();
    }
}

#[renox::test]
async fn lists_show_mine_or_at_my_store() {
    let app = boot().await;
    let db = app.db();
    let (a, b, c) = stores(&app).await;
    let at_a = fixtures::bike(db, a.id, a.id).await.unwrap();
    let a_at_b = fixtures::bike(db, a.id, b.id).await.unwrap();
    let at_c = fixtures::bike(db, c.id, c.id).await.unwrap();

    let ids = |list: &[&RentalBike]| format!("{:?}", list.iter().map(|b| b.id).collect::<Vec<_>>());
    for (email, roles, expected) in [
        (
            "a@example.com",
            vec![(MECHANIC, Some(a.id))],
            ids(&[&at_a, &a_at_b]),
        ),
        (
            "b@example.com",
            vec![(CASHIER, Some(b.id))],
            ids(&[&a_at_b]),
        ),
        ("c@example.com", vec![(MANAGER, Some(c.id))], ids(&[&at_c])),
        (
            "ac@example.com",
            vec![(CASHIER, Some(a.id)), (STAFF, Some(c.id))],
            ids(&[&at_a, &a_at_b, &at_c]),
        ),
        (
            "boss@example.com",
            vec![(OWNER, None)],
            ids(&[&at_a, &a_at_b, &at_c]),
        ),
    ] {
        let user = fixtures::person(db, email, &roles).await.unwrap();
        app.acting_as(&user);
        app.get("/probe/bikes")
            .await
            .assert_ok()
            .assert_see(&expected);
    }
    // A user with no role sees nothing (fails closed).
    let nobody = User::register(db, "N", "n@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&nobody);
    app.get("/probe/bikes").await.assert_see("[]");
}

#[renox::test]
async fn a_dated_role_works_only_between_its_dates() {
    let app = boot().await;
    let db = app.db();
    let (north, south, _) = stores(&app).await;
    let at_south = fixtures::bike(db, south.id, south.id).await.unwrap();
    // A cashier of North helps South for three days, from tomorrow.
    let helper = fixtures::person(db, "help@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    fixtures::dated_role(db, &helper, STAFF, south.id, 1, 3)
        .await
        .unwrap();
    app.acting_as(&helper);
    let bike = format!("/probe/bikes/{}/{RENTALS_CHECKOUT}/location", at_south.id);
    let switch = format!("/staff/store/{}", south.id);

    // Before: South's bikes don't exist for them, and South can't be picked.
    app.get(&bike).await.assert_not_found();
    app.get("/staff").await.assert_dont_see(&switch);
    app.post(&switch, &[]).await.assert_forbidden();

    // During: South is in the switcher, and its counter is theirs. (Two
    // days on, the session has expired: log in again.)
    app.travel(DAY * 2);
    app.acting_as(&helper);
    app.get(&bike).await.assert_ok();
    app.get("/staff").await.assert_see(&switch);
    app.post(&switch, &[]).await.assert_status(303);
    app.get("/probe/active")
        .await
        .assert_see(&format!("store=Some({})", south.id));

    // After: it ended by itself; the active store falls back home.
    app.travel(DAY * 3);
    app.acting_as(&helper);
    app.get(&bike).await.assert_not_found();
    app.get("/probe/active")
        .await
        .assert_see(&format!("store=Some({})", north.id));
}

/// Every file of the example's code and views.
fn files(dir: &FsPath, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs" || e == "html") {
            found.push(path);
        }
    }
}

/// Code checks permissions, never a role's name: the access catalogue is
/// the only place role names may appear, and nothing may ask whether
/// someone *has a role*.
#[test]
fn no_code_checks_a_role_by_name() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    files(&root.join("src"), &mut found);
    files(&root.join("resources"), &mut found);
    let allowed = [
        // The catalogue names the roles.
        "src/app/access/catalogue.rs",
    ];
    // Asking about roles instead of permissions.
    let checks = [
        "has_role(",
        "has_role_in(",
        "require_role(",
        "role_names(",
        "auth.roles",
        ".roles(&",
        "users_with_role(",
        "users_with_role_in(",
        "gate_before(",
    ];
    let names = ["owner", "manager", "cashier", "mechanic"];
    let mut problems = Vec::new();
    for path in found {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if allowed.contains(&relative.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with("{#") {
                continue;
            }
            for check in checks {
                if line.contains(check) {
                    problems.push(format!("{relative}:{}: `{check}`", n + 1));
                }
            }
            // A role's name as a value: `"manager"` or `'manager'`.
            // (`src/explain.rs` names audiences with the same words, as
            // keys of the /about/pages filter: not a check.)
            if relative != "src/explain.rs" {
                for name in names {
                    if line.contains(&format!("\"{name}\"")) || line.contains(&format!("'{name}'"))
                    {
                        problems.push(format!("{relative}:{}: the role name `{name}`", n + 1));
                    }
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "check permissions (access::catalogue), not role names:\n{}",
        problems.join("\n")
    );
}
