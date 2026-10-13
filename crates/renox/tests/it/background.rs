//! M20a: cache counters, one-time values, atomic locks and pruning, on both
//! stores.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use renox::prelude::*;
use renox::testing::TestApp;

async fn app(store: &str) -> TestApp {
    let store = match store {
        "database" => renox::CacheStore::Database,
        _ => renox::CacheStore::Memory,
    };
    TestApp::with_config(App::new(), move |c| c.cache_store = store).await
}

#[renox::test]
async fn counters_and_one_time_values() {
    for store in ["memory", "database"] {
        let app = app(store).await;
        let cache = &app.state().cache;
        assert_eq!(cache.increment("hits", 1).await.unwrap(), 1, "{store}");
        assert_eq!(cache.increment("hits", 5).await.unwrap(), 6);
        assert_eq!(cache.decrement("hits", 2).await.unwrap(), 4);
        assert_eq!(cache.get::<i64>("hits").await.unwrap(), Some(4));
        cache.put("name", &"Ana", None).await.unwrap();
        assert!(
            cache.increment("name", 1).await.is_err(),
            "{store}: not a number"
        );

        // Concurrent increments don't lose counts.
        let tasks: Vec<_> = (0..20)
            .map(|_| {
                let cache = cache.clone();
                tokio::spawn(async move { cache.increment("race", 1).await.unwrap() })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(cache.get::<i64>("race").await.unwrap(), Some(20), "{store}");

        // An expired counter starts again at 0.
        cache
            .put("old", &41, Some(Duration::from_millis(1)))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert_eq!(cache.increment("old", 1).await.unwrap(), 1, "{store}");

        assert!(cache.add("once", &"first", None).await.unwrap());
        assert!(!cache.add("once", &"second", None).await.unwrap());
        assert_eq!(
            cache.get::<String>("once").await.unwrap().as_deref(),
            Some("first")
        );
        cache
            .put("stale", &1, Some(Duration::from_millis(1)))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(
            cache.add("stale", &2, None).await.unwrap(),
            "{store}: expired counts as free"
        );

        assert_eq!(
            cache.pull::<String>("once").await.unwrap().as_deref(),
            Some("first")
        );
        assert_eq!(cache.pull::<String>("once").await.unwrap(), None);
        assert!(!cache.has("once").await.unwrap());

        cache
            .put("gone", &1, Some(Duration::from_millis(1)))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert_eq!(cache.pull::<i64>("gone").await.unwrap(), None, "{store}");
        cache
            .put("short", &1, Some(Duration::from_millis(1)))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let pruned = cache.prune().await.unwrap();
        if store == "database" {
            assert_eq!(pruned, 1);
            let rows: i64 = renox::db::sql("SELECT COUNT(*) FROM cache WHERE key = 'short'")
                .scalar(app.db())
                .await
                .unwrap();
            assert_eq!(rows, 0);
        }
    }
}

#[renox::test]
async fn locks_let_one_holder_in() {
    for store in ["memory", "database"] {
        let app = app(store).await;
        let cache = &app.state().cache;
        let lock = cache.lock("order:1", Duration::from_secs(30));
        let guard = lock.try_acquire().await.unwrap().expect("free");
        assert!(lock.is_held().await.unwrap());
        assert!(lock.try_acquire().await.unwrap().is_none(), "{store}: held");
        let waited = std::time::Instant::now();
        let err = lock
            .block(Duration::from_millis(200))
            .await
            .err()
            .expect("times out");
        assert!(waited.elapsed() >= Duration::from_millis(200));
        assert!(format!("{err:?}").contains("order:1"));
        assert!(guard.release().await.unwrap());
        assert!(!lock.is_held().await.unwrap());

        // Dropping a guard releases it in the background. `block` returns as
        // soon as it's free; the long limit is slack for a loaded PostgreSQL
        // (2 s failed once with the whole suite running).
        drop(lock.try_acquire().await.unwrap().unwrap());
        let again = lock.block(Duration::from_secs(10)).await.unwrap();

        // A lock whose ttl ran out can be taken; the old guard then can't
        // release the new holder's lock.
        let short = cache.lock("short", Duration::from_secs(1));
        let stale = short.try_acquire().await.unwrap().unwrap();
        assert!(
            short.try_acquire().await.unwrap().is_none(),
            "held for its ttl"
        );
        tokio::time::sleep(Duration::from_millis(3100)).await;
        // The new holder's ttl is long, so a slow run can't expire it too.
        let fresh = cache
            .lock("short", Duration::from_secs(30))
            .try_acquire()
            .await
            .unwrap()
            .expect("expired");
        assert!(!stale.release().await.unwrap(), "{store}");
        assert!(short.is_held().await.unwrap());
        drop(fresh);

        // `flush` keeps locks; `force_release` doesn't care who holds it.
        cache.flush().await.unwrap();
        assert!(lock.is_held().await.unwrap());
        lock.force_release().await.unwrap();
        assert!(!lock.is_held().await.unwrap());
        drop(again); // no longer the holder: releases nothing

        // Many workers, one at a time.
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let (lock, inside, most) = (lock.clone(), inside.clone(), most.clone());
                tokio::spawn(async move {
                    // Generous: this checks one-at-a-time, not how fast eight
                    // waiters get through (slow on PostgreSQL under a full run).
                    let guard = lock.block(Duration::from_secs(60)).await.unwrap();
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    inside.fetch_sub(1, Ordering::SeqCst);
                    guard.release().await.unwrap();
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 1, "{store}");
    }
}
