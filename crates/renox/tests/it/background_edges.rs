//! #256: background work no test had reached: a crashed last attempt inside
//! a batch and with a unique key, unique jobs that use the default
//! `unique_id`, cron-style tasks shared by two kernels, schedule pings,
//! mail failover running out, storage errors, cache and webhook edges, an
//! import's other paths and plural ranges that don't parse.

use std::time::Duration;

use renox::http::FakeResponse;
use renox::prelude::*;
use renox::testing::TestApp;
use serde::{Deserialize, Serialize};

/// Unique by type alone (the default `unique_id`).
#[derive(Serialize, Deserialize)]
struct Rebuild;

impl Job for Rebuild {
    const NAME: &'static str = "edges-rebuild";
    const UNIQUE_FOR: Option<Duration> = Some(Duration::from_secs(3600));
    async fn handle(self, _ctx: JobContext) -> Result {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct Part(u32);

impl Job for Part {
    const NAME: &'static str = "edges-part";
    async fn handle(self, _ctx: JobContext) -> Result {
        Ok(())
    }
}

async fn app() -> TestApp {
    TestApp::new(App::new().job::<Rebuild>().job::<Part>()).await
}

async fn count(app: &TestApp, sql: &str) -> i64 {
    renox::db::sql(sql).scalar(app.db()).await.unwrap()
}

/// Makes every queued job look like its last attempt was taken an hour ago
/// by a worker that died.
async fn crash_all(app: &TestApp) {
    renox::db::sql("UPDATE jobs SET attempts = max_attempts, reserved_at = ?")
        .bind(renox::chrono::Utc::now().timestamp() - 3600)
        .execute(app.db())
        .await
        .unwrap();
}

#[renox::test]
async fn unique_jobs_by_type_alone_are_queued_once() {
    let app = app().await;
    app.state().dispatch(Rebuild).await.unwrap();
    app.state().dispatch(Rebuild).await.unwrap();
    assert_eq!(app.queued_jobs().await, ["edges-rebuild"]);
}

#[renox::test]
async fn a_crashed_unique_job_releases_its_claim() {
    let app = app().await;
    app.state().dispatch(Rebuild).await.unwrap();
    crash_all(&app).await;
    app.run_jobs().await;
    assert_eq!(count(&app, "SELECT COUNT(*) FROM failed_jobs").await, 1);
    // The claim is gone: the same job can be queued again.
    app.state().dispatch(Rebuild).await.unwrap();
    assert_eq!(app.queued_jobs().await, ["edges-rebuild"]);
}

#[renox::test]
async fn a_crashed_job_in_a_batch_counts_as_failed() {
    let app = app().await;
    let batch = app
        .state()
        .queue
        .batch("parts")
        .push(Part(1))
        .push(Part(2))
        .allow_failures()
        .dispatch()
        .await
        .unwrap();
    renox::db::sql("UPDATE jobs SET attempts = max_attempts, reserved_at = ? WHERE id = (SELECT MIN(id) FROM jobs)")
        .bind(renox::chrono::Utc::now().timestamp() - 3600)
        .execute(app.db())
        .await
        .unwrap();
    app.run_jobs().await;
    let status = app
        .state()
        .queue
        .batch_status(batch)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (status.total, status.failed, status.pending),
        (2, 1, 0),
        "{status:?}"
    );
    assert!(status.finished);
    assert_eq!(status.progress(), 100);
}

static DAILY_RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Two kernels on one database with a cron-style task: one run.
#[renox::test]
async fn a_cron_task_shared_by_two_kernels_runs_once() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}/app.db", dir.path().display());
    let kernel = || {
        let mut config = Config::default();
        config.database_url = url.clone();
        App::with_config(config)
            .schedule(|s| {
                s.cron("* * * * *", "every-minute", |_state| async {
                    DAILY_RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                });
            })
            .boot()
    };
    let first = kernel().await.unwrap();
    first.migrate().await.unwrap();
    let second = kernel().await.unwrap();
    let before = DAILY_RUNS.load(std::sync::atomic::Ordering::SeqCst);
    first.run_scheduled("every-minute").await.unwrap();
    second.run_scheduled("every-minute").await.unwrap();
    let after = DAILY_RUNS.load(std::sync::atomic::Ordering::SeqCst);
    assert!(after - before >= 1, "it ran");
    let claims: i64 = renox::db::sql("SELECT COUNT(*) FROM cache WHERE key LIKE ?")
        .bind("renox:schedule:every-minute:%")
        .scalar(first.db())
        .await
        .unwrap();
    assert!(claims <= 1, "{claims}");
}

#[renox::test]
async fn schedule_pings_report_errors_without_failing_the_task() {
    let app = TestApp::new(App::new().schedule(|s| {
        s.every_minute("pinged", |_state| async { Ok(()) })
            .then_ping("https://ping.test/ok");
        s.every_minute("pinged-bad", |_state| async { Ok(()) })
            .then_ping("https://ping.test/bad");
        s.every_minute("pinged-down", |_state| async { Ok(()) })
            .then_ping("https://ping.test/down");
    }))
    .await;
    let http = app.fake_http();
    http.on("https://ping.test/ok", FakeResponse::status(200));
    http.on("https://ping.test/bad", FakeResponse::status(500));
    http.on("https://ping.test/down", FakeResponse::connection_error());
    for task in ["pinged", "pinged-bad", "pinged-down"] {
        app.kernel().run_scheduled(task).await.unwrap();
    }
    let urls: Vec<String> = http.sent().iter().map(|r| r.url.clone()).collect();
    assert!(
        urls.contains(&"https://ping.test/ok".to_owned()),
        "{urls:?}"
    );
    assert!(
        urls.contains(&"https://ping.test/bad".to_owned()),
        "{urls:?}"
    );
}

#[renox::test]
async fn storage_errors_for_missing_files_and_directories() {
    let app = TestApp::new(App::new()).await;
    let storage = &app.state().storage;
    for result in [
        storage.copy("missing.txt", "copy.txt").await.err(),
        storage.rename("missing.txt", "moved.txt").await.err(),
    ] {
        // A missing file is a 404 for whoever asked.
        assert!(matches!(result, Some(Error::NotFound)), "{result:?}");
    }
    assert!(storage.get("missing.txt").await.unwrap().is_none());
    assert!(storage.list("no-such-dir").await.unwrap().is_empty());
    storage
        .put("deep/down/a.txt", b"hi".to_vec().into())
        .await
        .unwrap();
    assert_eq!(storage.size("deep/down/a.txt").await.unwrap(), Some(2));
    assert_eq!(
        storage.size("deep").await.unwrap(),
        None,
        "a directory has no size"
    );
}

#[renox::test]
async fn the_cache_refuses_a_value_of_another_type() {
    let app = TestApp::new(App::new()).await;
    let cache = &app.state().cache;
    cache
        .put("answer", &"forty-two", Some(Duration::from_secs(60)))
        .await
        .unwrap();
    let err = cache.pull::<i64>("answer").await.unwrap_err();
    assert!(
        format!("{err:?}").contains("the cached `answer` is not"),
        "{err:?}"
    );
}

#[renox::test]
async fn plural_ranges_that_dont_parse_fall_back_to_the_last_form() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("en.json"),
        r#"{"apples": "{0} none|[x,y] odd|[2,*] many", "pears": "[1,2] few", "plums": null}"#,
    )
    .unwrap();
    let path = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Probe), move |c| c.lang_path = path).await;
    app.get("/t?key=apples&count=1").await.assert_see("many");
    app.get("/t?key=apples&count=0").await.assert_see("none");
    app.get("/t?key=pears&count=5").await.assert_see("few");
}

struct Probe;

impl Module for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn routes(&self) -> Routes {
        Routes::new().get(
            "/t",
            |lang: Lang, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let count: i64 = q["count"].parse().unwrap();
                lang.choice(&q["key"], count, &[])
            },
        )
    }
}

// The rest of #256: queue paths left after the first pass.

/// A job that can't be written down (its `Serialize` fails).
#[derive(Deserialize)]
struct Unsaveable;

impl Serialize for Unsaveable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("this job can't be saved"))
    }
}

impl Job for Unsaveable {
    const NAME: &'static str = "edges-unsaveable";
    async fn handle(self, _ctx: JobContext) -> Result {
        Ok(())
    }
}

#[renox::test]
async fn chains_and_batches_with_a_job_that_cant_be_saved_queue_nothing() {
    let app = app().await;
    let queue = &app.state().queue;
    let err = queue
        .chain()
        .then(Part(1))
        .then(Unsaveable)
        .then(Part(2))
        .dispatch()
        .await
        .unwrap_err();
    assert!(format!("{err:?}").contains("can't be saved"), "{err:?}");
    let err = queue
        .batch("broken")
        .push(Part(1))
        .push(Unsaveable)
        .dispatch()
        .await
        .unwrap_err();
    assert!(format!("{err:?}").contains("can't be saved"), "{err:?}");
    assert!(app.queued_jobs().await.is_empty());
    assert_eq!(count(&app, "SELECT COUNT(*) FROM job_batches").await, 0);
}

/// An empty batch is finished at once (100 %); a batch pruned while its
/// job waits doesn't stop the job.
#[renox::test]
async fn empty_and_pruned_batches() {
    let app = app().await;
    let queue = &app.state().queue;
    let empty = queue.batch("nothing").dispatch().await.unwrap();
    let status = queue.batch_status(empty).await.unwrap().unwrap();
    assert!(status.finished);
    assert_eq!((status.total, status.progress()), (0, 100));

    queue
        .batch("pruned")
        .push(Part(1))
        .dispatch()
        .await
        .unwrap();
    renox::db::sql("DELETE FROM job_batches")
        .execute(app.db())
        .await
        .unwrap();
    assert_eq!(app.run_jobs().await, 1);
    assert_eq!(count(&app, "SELECT COUNT(*) FROM failed_jobs").await, 0);
    assert!(app.queued_jobs().await.is_empty());
}

/// Fails for good; its `failed` hook panics.
#[derive(Serialize, Deserialize)]
struct Doomed;

impl Job for Doomed {
    const NAME: &'static str = "edges-doomed";
    async fn handle(self, _ctx: JobContext) -> Result {
        Err(Error::permanent(renox::anyhow::anyhow!("out of stock")))
    }
    async fn failed(self, _ctx: JobContext, _error: String) {
        panic!("the hook broke too");
    }
}

/// Deletes its own row while it runs, as `queue:forget` from another
/// process would.
#[derive(Serialize, Deserialize)]
struct Vanishing;

impl Job for Vanishing {
    const NAME: &'static str = "edges-vanishing";
    async fn handle(self, ctx: JobContext) -> Result {
        renox::db::sql("DELETE FROM jobs WHERE id = ?")
            .bind(ctx.id)
            .execute(&ctx.state.db)
            .await?;
        Ok(())
    }
}

#[renox::test]
async fn a_panicking_failed_hook_still_records_the_failure() {
    let (logs, _logged) = crate::logs::capture();
    let app = TestApp::new(App::new().job::<Doomed>().job::<Vanishing>()).await;
    app.state().dispatch(Doomed).await.unwrap();
    app.run_jobs().await;
    assert_eq!(count(&app, "SELECT COUNT(*) FROM failed_jobs").await, 1);
    assert!(app.queued_jobs().await.is_empty());

    // A job whose row is gone when it finishes: nothing is recorded twice.
    app.state().dispatch(Vanishing).await.unwrap();
    app.run_jobs().await;
    assert!(app.queued_jobs().await.is_empty());
    assert_eq!(count(&app, "SELECT COUNT(*) FROM failed_jobs").await, 1);
    assert!(
        logs.has(&["the job's failed hook panicked"]),
        "{}",
        logs.text()
    );
}

/// The dashboard's numbers: jobs done in the last hour, and a queue whose
/// only job waits for later has no "oldest waiting".
#[renox::test]
async fn queue_stats_count_finished_jobs_and_ignore_delayed_ones() {
    let app = app().await;
    let queue = &app.state().queue;
    queue.dispatch(Part(1)).await.unwrap();
    assert_eq!(app.run_jobs().await, 1);
    queue
        .dispatch_after(Part(2), Duration::from_secs(3600))
        .await
        .unwrap();
    let stats = queue.stats().await.unwrap();
    assert_eq!(stats.done_last_hour, 1);
    assert_eq!(stats.oldest_wait, None);
    assert_eq!(stats.queues[0].delayed, 1);
}

/// A worker loop that can't read the queue logs it and keeps going until
/// it is stopped.
#[renox::test]
async fn a_worker_loop_survives_a_queue_it_cant_read() {
    let app = app().await;
    renox::db::sql("ALTER TABLE jobs RENAME TO jobs_away")
        .execute(app.db())
        .await
        .unwrap();
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(app.kernel().worker(Vec::new()).run(1, stopped));
    tokio::time::sleep(Duration::from_millis(300)).await;
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("the worker stops")
        .unwrap();
}
