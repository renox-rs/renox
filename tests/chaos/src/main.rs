//! The app `tests/chaos/run.sh` injects faults into while it serves: the
//! database stopped, paused or locked, and panics in handlers, listeners,
//! jobs and scheduled tasks. `/stats` reports what happened, as JSON.

use std::time::Duration;

use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// Records that something ran (`events.kind`), for `/stats`.
async fn record(db: &Db, kind: &str) -> Result {
    renox::db::sql("INSERT INTO events (kind) VALUES (?)")
        .bind(kind)
        .execute(db)
        .await?;
    Ok(())
}

/// Sleeps `secs`, then records `job done`.
#[derive(Serialize, Deserialize)]
struct Work {
    secs: u64,
}

impl Job for Work {
    const NAME: &'static str = "chaos-work";
    fn backoff(_: u32) -> Duration {
        Duration::from_secs(1)
    }
    async fn handle(self, ctx: JobContext) -> Result {
        renox::tokio::time::sleep(Duration::from_secs(self.secs)).await;
        record(&ctx.state.db, "job done").await
    }
}

/// Panics on every attempt, so it ends in `failed_jobs`.
#[derive(Serialize, Deserialize)]
struct Panicky;

impl Job for Panicky {
    const NAME: &'static str = "chaos-panicky";
    const MAX_ATTEMPTS: u32 = 2;
    fn backoff(_: u32) -> Duration {
        Duration::ZERO
    }
    async fn handle(self, _: JobContext) -> Result {
        panic!("the job panicked");
    }
}

#[derive(Clone)]
struct Poke;
impl Event for Poke {}

struct Chaos;

impl Module for Chaos {
    fn name(&self) -> &'static str {
        "chaos"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/read", read)
            .get("/write", write)
            .get("/sleep", sleep)
            .get("/stats", stats)
            .get("/panic", panic_handler)
            .get("/emit", emit)
            .get("/dispatch", dispatch)
            .get("/dispatch-panicky", dispatch_panicky)
    }

    fn register(&self, app: &mut Registry) {
        app.job::<Work>()
            .job::<Panicky>()
            .listen(|_: Poke, _| async move {
                if true {
                    panic!("the listener panicked");
                }
                Ok(())
            })
            .listen(|_: Poke, state| async move { record(&state.db, "listener ran").await });
    }
}

async fn read(State(db): State<Db>) -> Result<String> {
    let n: i64 = renox::db::sql("SELECT COUNT(*) FROM items")
        .scalar(&db)
        .await?;
    Ok(format!("{n}\n"))
}

async fn write(State(db): State<Db>) -> Result<String> {
    let id: i64 = renox::db::sql("INSERT INTO items (name) VALUES (?) RETURNING id")
        .bind("x")
        .scalar(&db)
        .await?;
    Ok(format!("{id}\n"))
}

/// A query that takes 3 s (PostgreSQL only).
async fn sleep(State(db): State<Db>) -> Result<&'static str> {
    renox::db::sql("SELECT pg_sleep(3)").execute(&db).await?;
    Ok("slept\n")
}

async fn count(db: &Db, sql: &str) -> Result<i64> {
    Ok(renox::db::sql(sql).scalar(db).await?)
}

async fn stats(State(db): State<Db>) -> Result<Json<renox::serde_json::Value>> {
    let events = |kind: &'static str| {
        let db = db.clone();
        async move {
            renox::db::sql("SELECT COUNT(*) FROM events WHERE kind = ?")
                .bind(kind)
                .scalar::<i64>(&db)
                .await
        }
    };
    Ok(Json(json!({
        "jobs_done": events("job done").await?,
        "listeners": events("listener ran").await?,
        "ticks": events("tick").await?,
        "task_panics": events("task panicked").await?,
        "pending": count(&db, "SELECT COUNT(*) FROM jobs").await?,
        "failed": count(&db, "SELECT COUNT(*) FROM failed_jobs").await?,
    })))
}

async fn panic_handler() -> &'static str {
    if true {
        panic!("the handler panicked");
    }
    "unreachable"
}

async fn emit(State(state): State<AppState>) -> Result<&'static str> {
    state.emit(Poke).await?;
    Ok("emitted\n")
}

#[derive(Deserialize)]
struct Secs {
    secs: Option<u64>,
}

async fn dispatch(State(state): State<AppState>, Query(q): Query<Secs>) -> Result<String> {
    let id = state
        .dispatch(Work {
            secs: q.secs.unwrap_or(0),
        })
        .await?;
    Ok(format!("{id}\n"))
}

async fn dispatch_panicky(State(state): State<AppState>) -> Result<String> {
    Ok(format!("{}\n", state.dispatch(Panicky).await?))
}

fn main() -> renox::Result {
    App::new()
        .migrations(renox::migrations!())
        .module(Chaos)
        .schedule(|s| {
            s.every(Duration::from_secs(1), "tick", |state| async move {
                record(&state.db, "tick").await
            });
            s.every(Duration::from_secs(1), "panicky-task", |state| async move {
                record(&state.db, "task panicked").await?;
                panic!("the task panicked");
            });
        })
        .run()
}
