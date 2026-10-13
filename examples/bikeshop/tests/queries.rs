//! No N+1 anywhere (#243): the main pages, walked as a guest, a customer and
//! the owner, cost the same number of queries on the small seed (`db:seed`)
//! as on the large one (`demo:seed --size large`: Pagila's volume, ~15,000
//! rentals, 5,000 orders, 3,000 work orders).
//!
//! A page that loads a relation row by row would run a query per row it
//! shows, so with 25 rows on a page instead of a handful its count would
//! jump; one that loads relations with `belongs_to` / `has_many` / `Pivot`
//! (one query per relation, whatever the page size) stays put. The test
//! allows a few queries of difference (a section that is empty on the small
//! shop and skips its query) and a ceiling per page.
//!
//! The large seed takes ~10 s in a debug build on SQLite, so this runs with
//! the rest of `cargo test -p bikeshop` (and `cargo test --workspace` in CI).
//! On PostgreSQL (`TEST_DATABASE_URL`) it seeds the large shop too; CI's
//! PostgreSQL job runs it.

use bikeshop::app::catalog::model::{Category, Product};
use bikeshop::seed::{self, Volume};
use renox::db::capture_queries;
use renox::prelude::*;
use renox::testing::TestApp;
use std::collections::BTreeMap;

/// More queries than this on one page is a smell even without growth.
const CEILING: usize = 60;
/// Queries a page may add between the small and the large shop.
const SLACK: usize = 3;

/// Who walks which pages.
#[derive(Clone, Copy, Debug)]
enum Who {
    Guest,
    Customer,
    Owner,
}

/// The pages walked, by who opens them. `{category}` and `{product}` are
/// filled from the seeded catalogue.
const PAGES: &[(Who, &str)] = &[
    (Who::Guest, "/"),
    (Who::Guest, "/shop"),
    (Who::Guest, "/shop/{category}"),
    (Who::Guest, "/products/{product}"),
    (Who::Guest, "/search?q=helmet"),
    (Who::Guest, "/plans"),
    (Who::Guest, "/rent"),
    (Who::Guest, "/about/pages"),
    (Who::Guest, "/about/data"),
    (Who::Customer, "/rentals"),
    (Who::Customer, "/bikes"),
    (Who::Customer, "/plans/mine"),
    (Who::Customer, "/notifications"),
    (Who::Customer, "/account"),
    (Who::Owner, "/staff"),
    (Who::Owner, "/staff/rentals"),
    (Who::Owner, "/staff/fleet"),
    (Who::Owner, "/staff/identities"),
    (Who::Owner, "/staff/workshop"),
    (Who::Owner, "/staff/orders"),
    (Who::Owner, "/staff/counter"),
    (Who::Owner, "/staff/stock"),
    (Who::Owner, "/staff/stock/fleet"),
    (Who::Owner, "/staff/consignments"),
    (Who::Owner, "/staff/suppliers"),
    (Who::Owner, "/staff/purchase-orders"),
    (Who::Owner, "/staff/help"),
    (Who::Owner, "/staff/help/hours"),
    (Who::Owner, "/staff/placements"),
    (Who::Owner, "/staff/books"),
    (Who::Owner, "/staff/books/settlements"),
    (Who::Owner, "/staff/reports"),
    (Who::Owner, "/staff/reports/orders"),
    (Who::Owner, "/staff/reports/rentals"),
    (Who::Owner, "/staff/reports/work-orders"),
    (Who::Owner, "/staff/reports/payments"),
    (Who::Owner, "/staff/reports/customers"),
    (Who::Owner, "/staff/reports/intercompany"),
    (Who::Owner, "/staff/stores"),
    (Who::Owner, "/staff/team"),
    (Who::Owner, "/staff/roles"),
    (Who::Owner, "/staff/audit"),
    (Who::Owner, "/admin"),
];

/// Seeds a fresh app with `volume` and counts each page's queries.
async fn walk(volume: Volume) -> BTreeMap<String, usize> {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db().clone();
    seed::shop::build(&db, volume).await.unwrap();

    let category = Category::query()
        .order_by(Category::ID)
        .first(&db)
        .await
        .unwrap()
        .unwrap()
        .slug;
    let product = Product::query()
        .order_by(Product::ID)
        .first(&db)
        .await
        .unwrap()
        .unwrap()
        .slug;
    let customer = User::find_by_email(&db, "customer@bikeshop.test")
        .await
        .unwrap()
        .unwrap();
    let owner = User::find_by_email(&db, "owner@bikeshop.test")
        .await
        .unwrap()
        .unwrap();

    let mut counts = BTreeMap::new();
    for (who, page) in PAGES {
        match who {
            Who::Guest => app.logout(),
            Who::Customer => app.acting_as(&customer),
            Who::Owner => app.acting_as(&owner),
        };
        let uri = page
            .replace("{category}", &category)
            .replace("{product}", &product);
        let (res, queries) = capture_queries(app.get(&uri)).await;
        assert_eq!(
            res.status.as_u16(),
            200,
            "{uri} as {who:?} answered {}",
            res.status
        );
        counts.insert(format!("{who:?} {page}"), queries.len());
    }
    counts
}

#[renox::test]
async fn main_pages_cost_the_same_queries_on_the_large_seed() {
    let small = walk(Volume::small()).await;
    let large = walk(Volume::large()).await;

    let mut problems = Vec::new();
    for (page, &few) in &small {
        let many = large[page];
        println!("{page:45} small {few:>3}  large {many:>3}");
        if many > few + SLACK {
            problems.push(format!(
                "{page}: {few} queries on the small shop, {many} on the large one"
            ));
        }
        if many > CEILING {
            problems.push(format!("{page}: {many} queries (more than {CEILING})"));
        }
    }
    assert!(
        problems.is_empty(),
        "N+1 suspects:\n{}",
        problems.join("\n")
    );
}
