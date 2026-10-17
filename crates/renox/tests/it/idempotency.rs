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

// The `.offline()` middleware.

mod offline {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use renox::auth::User;
    use renox::prelude::*;
    use renox::testing::{TestApp, TestResponse};

    static RUNS: AtomicUsize = AtomicUsize::new(0);
    static FLAKY: AtomicUsize = AtomicUsize::new(0);
    static RACE: AtomicUsize = AtomicUsize::new(0);
    static KEYS: AtomicUsize = AtomicUsize::new(0);

    struct Field;

    impl Module for Field {
        fn name(&self) -> &'static str {
            "field"
        }

        fn routes(&self) -> Routes {
            Routes::new()
                .post(
                    "/visits",
                    |Form(form): Form<serde_json::Value>| async move {
                        let n = RUNS.fetch_add(1, Ordering::SeqCst) + 1;
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        (
                            StatusCode::CREATED,
                            [("x-visit", n.to_string())],
                            format!("visit {n} {}", form["note"].as_str().unwrap_or_default()),
                        )
                    },
                )
                .post("/race", || async {
                    RACE.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    "raced"
                })
                .post("/flaky", || async {
                    if FLAKY.fetch_add(1, Ordering::SeqCst) == 0 {
                        (StatusCode::INTERNAL_SERVER_ERROR, "boom")
                    } else {
                        (StatusCode::OK, "fine")
                    }
                })
                .post("/when", describe)
                .offline()
                .post("/plain", || async { "plain" })
        }
    }

    /// "<recorded_at> <client> <skew_exceeded> <recorded_at differs from received_at>"
    async fn describe(offline: Offline) -> String {
        format!(
            "{} {} {} {}",
            offline.recorded_at.to_rfc3339(),
            offline.client_id.unwrap_or_default(),
            offline.skew_exceeded,
            offline.recorded_at != offline.received_at
        )
    }

    struct Tight;

    impl Module for Tight {
        fn name(&self) -> &'static str {
            "tight"
        }

        fn routes(&self) -> Routes {
            Routes::new()
                .post("/when-tight", describe)
                .offline_with(renox::idempotency::Skew::new().past(Duration::from_secs(60)))
        }
    }

    async fn post(app: &TestApp, uri: &str, key: Option<&str>, note: &str) -> TestResponse {
        let mut request = app.request();
        if let Some(key) = key {
            request = request.header("idempotency-key", key);
        }
        request.post(uri, &[("note", note)]).await
    }

    #[renox::test]
    async fn a_key_is_required_and_the_handler_runs_once_per_key() {
        let app = TestApp::new(App::new().module(Field)).await;
        post(&app, "/visits", None, "a").await.assert_status(400);
        let first = post(&app, "/visits", Some("k-1"), "a").await;
        first.assert_status(201);
        let first_visit = first.header("x-visit").unwrap().to_owned();
        assert_eq!(first.header("idempotent-replayed"), None);

        let again = post(&app, "/visits", Some("k-1"), "a").await;
        again.assert_status(201);
        assert_eq!(again.text(), first.text());
        assert_eq!(again.header("x-visit"), Some(first_visit.as_str()));
        assert_eq!(again.header("idempotent-replayed"), Some("true"));

        // Another key is another request.
        let other = post(&app, "/visits", Some("k-2"), "a").await;
        assert_ne!(other.header("x-visit"), Some(first_visit.as_str()));
    }

    #[renox::test]
    async fn the_same_key_with_another_body_is_422() {
        let app = TestApp::new(App::new().module(Field)).await;
        post(&app, "/visits", Some("m-1"), "one")
            .await
            .assert_status(201);
        let other = post(&app, "/visits", Some("m-1"), "two").await;
        other.assert_status(422);
        other.assert_see("different request");
    }

    #[renox::test]
    async fn a_server_error_is_not_stored() {
        let app = TestApp::new(App::new().module(Field)).await;
        post(&app, "/flaky", Some("f-1"), "x")
            .await
            .assert_status(500);
        let retry = post(&app, "/flaky", Some("f-1"), "x").await;
        retry.assert_status(200);
        assert_eq!(retry.text(), "fine");
        assert_eq!(retry.header("idempotent-replayed"), None);
        // Now the 200 is what replays.
        let replay = post(&app, "/flaky", Some("f-1"), "x").await;
        assert_eq!(replay.text(), "fine");
        assert_eq!(replay.header("idempotent-replayed"), Some("true"));
    }

    #[renox::test]
    async fn concurrent_requests_with_one_key_run_the_handler_once() {
        let app = TestApp::new(App::new().module(Field)).await;
        let (a, b) = tokio::join!(
            post(&app, "/race", Some("c-1"), "x"),
            post(&app, "/race", Some("c-1"), "x"),
        );
        a.assert_ok();
        b.assert_ok();
        assert_eq!(a.text(), "raced");
        assert_eq!(b.text(), "raced");
        assert_eq!(RACE.load(Ordering::SeqCst), 1);
    }

    async fn seen(app: &TestApp, recorded_at: Option<String>, client: Option<&str>) -> String {
        let key = format!("seen-{}", KEYS.fetch_add(1, Ordering::SeqCst));
        let mut request = app.request().header("idempotency-key", &key);
        if let Some(at) = recorded_at {
            request = request.header("x-recorded-at", &at);
        }
        if let Some(client) = client {
            request = request.header("x-client-id", client);
        }
        let res = request.post("/when", &[("note", "x")]).await;
        res.assert_ok();
        res.text().to_owned()
    }

    fn ago(seconds: i64) -> String {
        (renox::db::now() - renox::chrono::TimeDelta::seconds(seconds)).to_rfc3339()
    }

    #[renox::test]
    async fn offline_reads_the_device_time_and_client_id() {
        let app = TestApp::new(App::new().module(Field)).await;
        let at = ago(3600);
        let out = seen(&app, Some(at.clone()), Some("dev-1")).await;
        let parsed = renox::chrono::DateTime::parse_from_rfc3339(&at).unwrap();
        assert_eq!(
            out,
            format!("{} dev-1 false true", parsed.to_utc().to_rfc3339())
        );
    }

    #[renox::test]
    async fn offline_flags_times_beyond_the_skew_limits() {
        let app = TestApp::new(App::new().module(Field)).await;
        // More than 24 h old, and more than 5 min ahead: still the device time.
        let old = seen(&app, Some(ago(25 * 3600)), None).await;
        assert!(old.ends_with(" true true"), "{old}");
        let ahead = seen(&app, Some(ago(-600)), None).await;
        assert!(ahead.ends_with(" true true"), "{ahead}");
        let near = seen(&app, Some(ago(-60)), None).await;
        assert!(near.ends_with(" false true"), "{near}");
    }

    #[renox::test]
    async fn offline_honours_configured_limits() {
        let app = TestApp::new(App::new().module(Tight)).await;
        let out = app
            .request()
            .header("idempotency-key", "tight-1")
            .header("x-recorded-at", &ago(120))
            .post("/when-tight", &[("note", "x")])
            .await;
        out.assert_ok();
        assert!(out.text().ends_with(" true true"), "{}", out.text());
    }

    #[renox::test]
    async fn offline_without_a_usable_time_falls_back_to_the_server() {
        let app = TestApp::new(App::new().module(Field)).await;
        for header in [None, Some("yesterday-ish".to_owned())] {
            let out = seen(&app, header, None).await;
            assert!(out.ends_with(" false false"), "{out}");
        }
    }

    #[renox::test]
    async fn routes_without_offline_ignore_the_header() {
        let app = TestApp::new(App::new().module(Field)).await;
        post(&app, "/plain", None, "x").await.assert_ok();
    }

    #[renox::test]
    async fn one_users_key_is_not_anothers() {
        let app = TestApp::new(App::new().module(renox::auth::Auth::new()).module(Field)).await;
        let ana = User::register(app.db(), "Ana", "ana@example.com", "password123")
            .await
            .unwrap();
        let bo = User::register(app.db(), "Bo", "bo@example.com", "password123")
            .await
            .unwrap();
        app.acting_as(&ana);
        let one = post(&app, "/visits", Some("shared"), "x").await;
        app.acting_as(&bo);
        let two = post(&app, "/visits", Some("shared"), "x").await;
        assert_ne!(one.text(), two.text());
        assert_eq!(two.header("idempotent-replayed"), None);
    }
}
