//! Idempotency key storage (`renox::idempotency`), on SQLite and (with
//! TEST_DATABASE_URL) PostgreSQL.

use renox::chrono::TimeDelta;
use renox::idempotency::{self, Claim};
use renox::prelude::*;
use renox::testing::TestApp;

const ROUTE: &str = "/stops/{id}/complete";

fn headers() -> Vec<(String, String)> {
    vec![("content-type".to_owned(), "text/plain".to_owned())]
}

#[test]
fn body_hash_is_sha256_hex() {
    assert_eq!(
        idempotency::body_hash(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[renox::test]
async fn a_key_is_claimed_once_then_replays_its_response() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let hash = idempotency::body_hash(b"qty=3");

    let first = idempotency::claim(db, "user:1", ROUTE, "k-1", &hash)
        .await
        .unwrap();
    assert_eq!(first, Claim::Claimed);
    // Claimed but unanswered: a second request must wait or retry.
    let second = idempotency::claim(db, "user:1", ROUTE, "k-1", &hash)
        .await
        .unwrap();
    assert_eq!(second, Claim::InProgress);

    idempotency::complete(db, "user:1", ROUTE, "k-1", 201, &headers(), b"\x00done\xff")
        .await
        .unwrap();
    let Claim::Replay(stored) = idempotency::claim(db, "user:1", ROUTE, "k-1", &hash)
        .await
        .unwrap()
    else {
        panic!("expected a replay");
    };
    assert_eq!(stored.status, 201);
    assert_eq!(stored.headers, headers());
    assert_eq!(stored.body, b"\x00done\xff");
}

#[renox::test]
async fn another_body_under_one_key_is_a_mismatch() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let one = idempotency::body_hash(b"one");
    let two = idempotency::body_hash(b"two");
    idempotency::claim(db, "", ROUTE, "k", &one).await.unwrap();
    // Also while the first is still running.
    assert_eq!(
        idempotency::claim(db, "", ROUTE, "k", &two).await.unwrap(),
        Claim::Mismatch
    );
    idempotency::complete(db, "", ROUTE, "k", 200, &[], b"")
        .await
        .unwrap();
    assert_eq!(
        idempotency::claim(db, "", ROUTE, "k", &two).await.unwrap(),
        Claim::Mismatch
    );
}

#[renox::test]
async fn keys_are_per_scope_and_route() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let hash = idempotency::body_hash(b"x");
    for (scope, route) in [("user:1", "/a"), ("user:2", "/a"), ("user:1", "/b")] {
        assert_eq!(
            idempotency::claim(db, scope, route, "same", &hash)
                .await
                .unwrap(),
            Claim::Claimed,
            "{scope} {route}"
        );
    }
}

#[renox::test]
async fn concurrent_claims_have_one_winner() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let hash = idempotency::body_hash(b"x");
    let claims = futures_util::future::join_all(
        (0..8).map(|_| idempotency::claim(db, "user:1", ROUTE, "race", &hash)),
    )
    .await;
    let won = claims
        .into_iter()
        .filter(|c| matches!(c, Ok(Claim::Claimed)))
        .count();
    assert_eq!(won, 1);
}

#[renox::test]
async fn release_frees_an_unanswered_key_but_keeps_an_answered_one() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let hash = idempotency::body_hash(b"x");
    idempotency::claim(db, "", ROUTE, "a", &hash).await.unwrap();
    idempotency::release(db, "", ROUTE, "a").await.unwrap();
    assert_eq!(
        idempotency::claim(db, "", ROUTE, "a", &hash).await.unwrap(),
        Claim::Claimed
    );

    idempotency::complete(db, "", ROUTE, "a", 200, &[], b"ok")
        .await
        .unwrap();
    idempotency::release(db, "", ROUTE, "a").await.unwrap();
    assert!(matches!(
        idempotency::claim(db, "", ROUTE, "a", &hash).await.unwrap(),
        Claim::Replay(_)
    ));
    // Completing a key that is gone is an error.
    assert!(
        idempotency::complete(db, "", ROUTE, "missing", 200, &[], b"")
            .await
            .is_err()
    );
}

#[renox::test]
async fn prune_removes_only_older_keys() {
    let app = TestApp::new(App::new()).await;
    let db = app.db();
    let hash = idempotency::body_hash(b"x");
    idempotency::claim(db, "", ROUTE, "old", &hash)
        .await
        .unwrap();
    // A cutoff in the past keeps it; one in the future removes it.
    let week_ago = renox::db::now() - TimeDelta::days(7);
    assert_eq!(idempotency::prune(db, week_ago).await.unwrap(), 0);
    let tomorrow = renox::db::now() + TimeDelta::days(1);
    assert_eq!(idempotency::prune(db, tomorrow).await.unwrap(), 1);
    // The key is free again.
    assert_eq!(
        idempotency::claim(db, "", ROUTE, "old", &hash)
            .await
            .unwrap(),
        Claim::Claimed
    );
}
