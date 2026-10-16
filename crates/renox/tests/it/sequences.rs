//! Gapless numbering (`renox::db::Sequence`), on SQLite and (with
//! TEST_DATABASE_URL) PostgreSQL. The concurrency test uses a file database
//! with several connections: with one connection it would prove nothing.

use renox::db::{Period, Sequence};
use renox::prelude::*;
use renox::testing::TestApp;

async fn app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}/seq.db", dir.path().display());
    // With TEST_DATABASE_URL the in-memory default becomes a PostgreSQL schema.
    let on_postgres = std::env::var("TEST_DATABASE_URL").is_ok();
    let app = TestApp::with_config(App::new(), |c| {
        if !on_postgres {
            c.database_url = url;
        }
        c.database_pool_size = 8;
    })
    .await;
    (app, dir)
}

#[renox::test]
async fn fifty_concurrent_tasks_get_one_to_fifty() {
    let (app, _dir) = app().await;
    let mut tasks = Vec::new();
    for _ in 0..50 {
        let db = app.db().clone();
        tasks.push(tokio::spawn(async move {
            let mut tx = db.begin().await.unwrap();
            let n = Sequence::new("invoice").next(&mut tx).await.unwrap();
            tokio::task::yield_now().await;
            tx.commit().await.unwrap();
            n
        }));
    }
    let mut got = Vec::new();
    for task in tasks {
        got.push(task.await.unwrap());
    }
    got.sort_unstable();
    assert_eq!(got, (1..=50).collect::<Vec<i64>>());
    assert_eq!(Sequence::new("invoice").last(app.db()).await.unwrap(), 50);
}

#[renox::test]
async fn a_rolled_back_number_is_issued_again() {
    let (app, _dir) = app().await;
    let seq = Sequence::new("receipt");
    let mut tx = app.db().begin().await.unwrap();
    assert_eq!(seq.next(&mut tx).await.unwrap(), 1);
    assert_eq!(seq.next(&mut tx).await.unwrap(), 2);
    tx.rollback().await.unwrap();
    assert_eq!(seq.peek(app.db()).await.unwrap(), 1, "nothing was kept");

    let mut tx = app.db().begin().await.unwrap();
    assert_eq!(seq.next(&mut tx).await.unwrap(), 1);
    tx.commit().await.unwrap();
    let mut tx = app.db().begin().await.unwrap();
    assert_eq!(seq.next(&mut tx).await.unwrap(), 2);
    tx.commit().await.unwrap();
}

#[renox::test]
async fn series_are_independent_by_scope_and_period() {
    let (app, _dir) = app().await;
    let db = app.db();
    let mut tx = db.begin().await.unwrap();
    let a26 = Sequence::new("inv").scope(1).period(Period::Year(2026));
    let a27 = Sequence::new("inv").scope(1).period(Period::Year(2027));
    let b26 = Sequence::new("inv").scope(2).period(Period::Year(2026));
    let credit = Sequence::new("credit").scope(1).period(Period::Year(2026));
    let monthly = Sequence::new("inv").period(Period::Month(2026, 10));
    assert_eq!(a26.next(&mut tx).await.unwrap(), 1);
    assert_eq!(a26.next(&mut tx).await.unwrap(), 2);
    assert_eq!(
        a27.next(&mut tx).await.unwrap(),
        1,
        "a new period starts at 1"
    );
    assert_eq!(b26.next(&mut tx).await.unwrap(), 1);
    assert_eq!(credit.next(&mut tx).await.unwrap(), 1);
    assert_eq!(monthly.next(&mut tx).await.unwrap(), 1);
    tx.commit().await.unwrap();

    let rows = Sequence::list(db).await.unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].series, "credit");
    assert_eq!(rows[0].scope, "1");
    assert_eq!(rows[0].period, "2026");
}

#[renox::test]
async fn peek_does_not_consume_and_set_start_continues() {
    let (app, _dir) = app().await;
    let db = app.db();
    let seq = Sequence::new("order").period(Period::Year(2026));
    assert_eq!(seq.peek(db).await.unwrap(), 1);
    assert_eq!(seq.peek(db).await.unwrap(), 1);
    seq.set_start(db, 1200).await.unwrap();
    assert_eq!(seq.peek(db).await.unwrap(), 1201);
    let mut tx = db.begin().await.unwrap();
    assert_eq!(seq.next(&mut tx).await.unwrap(), 1201);
    tx.commit().await.unwrap();
    assert_eq!(seq.last(db).await.unwrap(), 1201);
    // A period that was not started explicitly still begins at 1.
    let next_year = Sequence::new("order").period(Period::Year(2027));
    assert_eq!(next_year.peek(db).await.unwrap(), 1);
}

#[renox::test]
async fn the_gaps_variant_commits_at_once() {
    let (app, _dir) = app().await;
    let seq = Sequence::new("ticket");
    assert_eq!(seq.next_allowing_gaps(app.db()).await.unwrap(), 1);
    assert_eq!(seq.next_allowing_gaps(app.db()).await.unwrap(), 2);
    assert_eq!(seq.peek(app.db()).await.unwrap(), 3);
}
