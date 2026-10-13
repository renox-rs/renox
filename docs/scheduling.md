# Scheduler, events, cache and commands

Not everything in an app happens because someone opened a page. This guide covers the parts
that work on their own: jobs that run on a timetable, code that reacts when something happens,
a place to keep results for later, and your own commands to type in a terminal.

### Words you'll meet

| Word | What it means |
|---|---|
| **scheduler** | A clock inside your app that starts **scheduled tasks** at set times ("every day at 02:00"). |
| **cron** | An old, short way to write a timetable, like `30 9 * * 1-5` ("9:30 on weekdays"). |
| **time zone** | Which clock the times mean: `UTC`, `+07:00`, or a place like `Asia/Jakarta`. |
| **daylight saving time** | In some places clocks jump forward an hour in spring and back in autumn. |
| **event** | A message that says "this just happened", like "an order was placed". |
| **listener** | A function that runs when a certain event happens. |
| **queue** and **job** | A to-do list of work done in the background, and one item on it. See [queue.md](queue.md). |
| **cache** | A place to keep a value for a while, so you don't have to work it out again. |
| **lock** | A "do not disturb" sign: while one process holds it, no other can take it. |
| **process** | One running copy of your program. A big app may run several, on several servers. |
| **command** | Something you type in a terminal, like `my-app migrate`. |
| **prune** | Delete old rows that aren't needed any more. |

### In this guide

- [Scheduled tasks](#scheduled-tasks): run code every minute, every day, or on a cron timetable.
- [Events](#events): let one part of the app react to another.
- [Cache](#cache): keep values for later, and use [locks](#locks).
- [Commands](#commands): add your own terminal commands, and ask questions in them.

All of these are written in your Rust code and built into your app's program. That one program
runs them:

- `serve` (the command that starts the web server) also runs the scheduler and the queue
  workers, side by side;
- `my-app <command>` runs a command.

Want working code to look at?

- The short version is in the [cheat-sheet](../CHEATSHEET.md) ("Jobs, events, schedule, mail"
  and "Cache, session, uploads, translations").
- The queue has its own guide, [queue.md](queue.md).
- [examples/hello](../examples/hello) has a typed `entries:prune` command and a scheduled task.
- [examples/bikeshop](../examples/bikeshop) has nine scheduled tasks (every minute to monthly), events that keep cached
  dashboards fresh, `App::report` and a typed `demo:seed` command.

## Scheduled tasks

You list your tasks in `App::schedule` (or, inside a module, in `Registry::schedule()` in the
module's `register`). Each task has:

- a **name**, which must be unique in the app;
- an **async function** that gets the `AppState` (your app's shared things: database, cache,
  mailer…) and does the work.

```rust
use renox::prelude::*;
use renox::chrono::Weekday;
use std::time::Duration;

/// A task: refresh the stock from a supplier. (Empty here.)
async fn sync_stock(_state: AppState) -> Result { Ok(()) }
/// A task: build a report. (Empty here.)
async fn report(_state: AppState) -> Result { Ok(()) }

/// The app, with one task for each kind of timetable.
fn app() -> App {
    App::new().schedule(|s| {
        // A task can also be written in place, as a closure.
        s.every_minute("heartbeat", |_state| async move { Ok(()) });
        s.every_minutes(5, "sync-stock", sync_stock) // :00, :05, :10 … on the clock
            .weekdays()
            .between("08:00", "17:00");
        s.every(Duration::from_secs(30 * 60), "refresh-rates", report);
        s.hourly("hourly-report", report);
        // Every night at 02:00, delete carts nobody touched for 30 days.
        s.daily_at("02:00", "cleanup", |state| async move {
            renox::db::sql("DELETE FROM carts WHERE updated_at < ?")
                .bind(renox::db::now() - renox::chrono::TimeDelta::days(30))
                .execute(&state.db)
                .await?;
            Ok(())
        });
        s.weekly_on(Weekday::Mon, "07:00", "weekly-report", report);
        s.monthly_on(1, "00:05", "invoices", report); // day 29–31 skips shorter months
        s.cron("30 9 * * 1-5", "standup", report) // minute hour day month weekday
            .timezone("Europe/Amsterdam");
    })
}
```

What's going on:

- `sync-stock` runs every 5 minutes, but only on weekdays, between 08:00 and 17:00.
- `cleanup` runs every night at 02:00 and deletes carts older than 30 days.
- `standup` uses a cron timetable: at 9:30, Monday to Friday, Amsterdam time.

Here is every way to say *when*:

| Method | Runs |
|---|---|
| `every_minute(name, task)` | every minute, on the minute |
| `every_minutes(n, name, task)` | every `n` minutes, lined up with the clock (every 15: :00, :15, …) |
| `every(duration, name, task)` | every `duration`, lined up with the clock |
| `hourly(name, task)` | every hour, on the hour |
| `daily_at("HH:MM", name, task)` | every day at that time |
| `weekly_on(Weekday, "HH:MM", name, task)` | once a week (`renox::chrono::Weekday`) |
| `monthly_on(day, "HH:MM", name, task)` | on that day of each month (use 28 or lower to run in every month) |
| `cron(expr, name, task)` | whenever the clock matches a cron expression (five fields) |

### Reading a cron expression

A cron expression has five fields, separated by spaces: **minute, hour, day of the month,
month, day of the week**. So `30 9 * * 1-5` means "minute 30, hour 9, any day, any month,
Monday to Friday". In each field you can write:

- `*`: any value;
- a range `a-b`, like `1-5`;
- a list `a,b`, like `1,15`;
- a step `*/15`: every 15th (0, 15, 30, 45);
- names like `MON` or `JAN`. Sunday can be `0`, `7` or `SUN`.

You can also write `@hourly`, `@daily`, `@weekly`, `@monthly` or `@yearly` instead of the five
fields.

### Narrowing a task, and hooks

Each method gives back a `ScheduledTask`. You can chain more methods on it, to narrow down when
it runs or to add things that happen around it:

- **Filters** skip some runs: `weekdays()`, `weekends()`, `days(&[Weekday::Sat])`, and
  `between("08:00", "17:00")`. Both ends of `between` are included, and `"22:00"` to
  `"06:00"` goes over midnight.
- **Time zone:** `timezone("Asia/Jakarta")` uses that zone for this task instead of
  `APP_TIMEZONE`. It also takes an offset like `+07:00`, or `UTC`.
- **Hooks** are functions that run after the task: `on_failure(|err, state| async move { … })`
  after a run that failed or panicked (crashed), and `on_success(|state| async move { … })`
  after one that worked.
- **Pings** tell an outside health-check service (such as Healthchecks.io, Cronitor or Better
  Stack) that your task ran. If the pings stop, the service warns you. The methods are
  `ping_before(url)`, `then_ping(url)` (after every run), `ping_on_success(url)` and
  `ping_on_failure(url)`.

> [!NOTE]
> Filters only *skip* runs; they never move them. `every_minutes(5, …).between(…)` still runs on
> the 5-minute marks, just fewer of them.

> [!TIP]
> Pings are GET requests sent through `state.http`, with a 10-second timeout and one retry. A
> ping that fails is written to the log, and never stops the task.

Here's a nightly backup that reports to a health-check service and sends a mail when it fails:

```rust
use renox::prelude::*;

/// The backup task. (Empty here.)
async fn backup(_state: AppState) -> Result { Ok(()) }

/// Runs when the backup fails: queue a mail to the address in ALERT_EMAIL.
async fn alert(err: Error, state: AppState) {
    let to = state.config.var("ALERT_EMAIL").unwrap_or_else(|| "ops@example.com".into());
    let mail = renox::mail::Mail::new(to, "The backup failed", format!("{err:?}"));
    // Even the alert can fail; then the best we can do is print the error.
    if let Err(err) = state.queue_mail(mail).await {
        eprintln!("could not queue the alert: {err:?}");
    }
}

/// The app, with the backup at 03:00, three pings and the alert.
fn app() -> App {
    App::new().schedule(|s| {
        s.daily_at("03:00", "backup", backup)
            .ping_before("https://hc-ping.com/your-uuid/start")
            .ping_on_success("https://hc-ping.com/your-uuid")
            .ping_on_failure("https://hc-ping.com/your-uuid/fail")
            .on_failure(alert);
    })
}
```

> [!IMPORTANT]
> Mistakes are caught when the app boots (starts), before anything runs. A bad time
> (`"25:00"`), a bad cron expression, an unknown zone name, or a name used twice all stop the
> app with an error, for example ``invalid schedule: task `x` is scheduled twice``.

> [!NOTE]
> **Coming from Laravel:** the method names match Laravel's, in snake_case: `everyFiveMinutes`
> is `every_minutes(5, …)`, `dailyAt` is `daily_at`, `onFailure` is `on_failure`. The table at
> the end of this page has the full list.

### Time zones and daylight saving time

Times are read in `APP_TIMEZONE`, which can be:

- `UTC` (the default);
- a fixed offset from UTC, such as `+07:00`;
- an IANA name (the standard list of place names), such as `Asia/Jakarta` or
  `Europe/Amsterdam`. These follow that place's daylight saving rules.

A task's own `timezone(…)` wins over `APP_TIMEZONE`.

Daylight saving time makes some clock times odd. Here's what Renox does:

- **In spring, an hour is skipped.** In Amsterdam the clock jumps from 02:00 straight to
  03:00, so 02:30 never happens. A run due then (02:30) runs right after the jump, at 03:00.
- **In autumn, an hour happens twice.** A run due in that hour runs once, the first time.
- **Interval tasks** (`every…`, `hourly`) line up with the local clock, using the offset in
  force at the time. So `every(Duration::from_secs(2 * 3600), …)` in `+07:00` runs at even
  local hours (00:00, 02:00, 04:00…).

### What runs them

| Command | What it does |
|---|---|
| `my-app serve` (`rnx serve`) | runs the scheduler beside the web server, unless `SCHEDULER=false` |
| `my-app schedule:work` | runs only the scheduler, until stopped (for setups with `SCHEDULER=false`) |
| `my-app schedule:list` | shows each task's next run, its zone and name (`never` if filters skip every run) |
| `my-app schedule:run NAME` | runs one task now, with its hooks and pings, whatever its timetable says |

#### Overlaps

What if a task is still running when its next turn comes? In the same process, the new run is
**skipped** (and written to the log), so it never runs twice at once.

> [!TIP]
> For longer work, or a task you might also start by hand with `schedule:run`, protect it with
> a cache lock (see [Locks](#locks) below).

#### Several servers

Several servers that share one database may all run the scheduler. They don't all run each
task, though. Before each run, the scheduler **claims** it: it adds a row
`renox:schedule:<task>:<slot>` to the `cache` table, with `INSERT … ON CONFLICT DO NOTHING`
(this happens whatever `CACHE_STORE` is). Only the first one to add the row wins, so exactly
one process runs each slot.

- If the database can't be reached, the task runs anyway, as it would with a single process.
- `schedule:run` doesn't claim: it always runs.

#### Failures

When a task returns an error, or panics:

1. the error is written to the log;
2. the error goes to every **reporter** you added with `App::report`. It arrives as an
   `ErrorReport` with `kind: ReportKind::ScheduledTask` and the task's name in `source`;
3. `on_failure` runs.

For example, to post it to a chat:

```rust
use renox::prelude::*;
use renox::report::{ErrorReport, ReportKind};

/// The app, with a reporter that hears about every failed scheduled task.
fn app() -> App {
    App::new().report(|report: ErrorReport, _state: AppState| async move {
        // Reports also come from other places; only look at scheduled tasks here.
        if report.kind == ReportKind::ScheduledTask {
            eprintln!("task {:?} failed: {}", report.source, report.message);
        }
    })
}
```

Two more things are true inside a task:

- It runs in its own `renox::context` scope, so `context::app()` works inside it.
- The clock it reads is `renox::db::now()`. In tests, `TestApp::travel` moves that clock.

### Housekeeping

Some tables keep growing until something **prunes** them (deletes the old rows). For each
there is a command:

- `cache:prune`, `session:prune`, `queue:prune-failed`, `queue:prune-batches`;
- and, from modules, `tokens:prune`, `notifications:prune` and `audit:prune`.

Each also has a function, so one daily task can do them all:

```rust
use renox::prelude::*;
use std::time::Duration;

/// One day, as a `Duration`.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// Deletes old rows from every table that grows, then prints how many.
async fn housekeeping(state: AppState) -> Result {
    let db = &state.db;
    let cache = state.cache.prune().await?; // expired rows (CACHE_STORE=database)
    let sessions = Session::prune_expired(db).await?; // SESSION_DRIVER=database
    let tokens = renox::auth::prune_expired_tokens(db, DAY).await?; // expired over a day ago
    let read = renox::auth::prune_read_notifications(db, 30 * DAY).await?; // read over 30 days ago
    let audit = renox::audit::prune(db, 365 * DAY).await?; // the Audit module's log
    let failed = state.queue.prune_failed(7 * DAY).await?; // failed_jobs
    let batches = state.queue.prune_batches(DAY).await?; // finished job_batches
    println!("pruned {cache} cache, {sessions} sessions, {tokens} tokens, {read} notifications, \
              {audit} audit entries, {failed} failed jobs, {batches} batches");
    Ok(())
}

/// The app, with the Auth and Audit modules, and housekeeping every day at 04:00.
fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(renox::audit::Audit)
        .schedule(|s| {
            s.daily_at("04:00", "housekeeping", housekeeping);
        })
}
```

Each function returns how many rows it deleted (`Result<u64>`).

> [!WARNING]
> Only call the ones whose tables your app has. The token and notification tables come with the
> `Auth` module, and `audit_logs` with `Audit`. The `sessions` and `cache` tables are in every
> app.

### Testing a task

`TestApp` (the test helper) doesn't start the scheduler. Instead, run a task by name with
`run_scheduled(name)` (the same as `schedule:run`, at the time `travel` moved the clock to),
then check what it did:

```rust
use renox::prelude::*;
use renox::testing::TestApp;

/// An app with one task, which counts its runs in the cache.
fn app() -> App {
    App::new().schedule(|s| {
        s.daily_at("02:00", "count", |state| async move {
            state.cache.increment("runs", 1).await?;
            Ok(())
        });
    })
}

/// Runs the task once, and checks that the count is 1.
async fn the_task_counts() -> Result {
    let app = TestApp::new(app()).await;
    app.run_scheduled("count").await?;
    assert_eq!(app.state().cache.get::<i64>("runs").await?, Some(1));
    Ok(())
}
```

## Events

An **event** says that something happened. **Listeners** decide what happens next.

Why bother? Imagine a shop. The module that places orders **emits** (sends out) an
`OrderPlaced` event. The stock module listens and lowers the stock; the mail module listens
and sends a receipt. The order module doesn't need to know either of them exists. You can add
a new listener later without touching the order code.

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// The event: an order was placed. It carries the order's id.
#[derive(Clone, Debug)]
struct OrderPlaced {
    order_id: i64,
}

impl Event for OrderPlaced {} // Clone + Send + Sync + 'static

/// A queue job that sends the receipt.
#[derive(Serialize, Deserialize)]
struct SendReceipt { order_id: i64 }

impl Job for SendReceipt {
    const NAME: &'static str = "send-receipt";
    async fn handle(self, _ctx: JobContext) -> Result { Ok(()) }
}

/// The stock module: it listens for orders and lowers the stock.
struct Stock;

impl Module for Stock {
    fn name(&self) -> &'static str { "stock" }

    /// Called at boot: this is where a module adds its listeners.
    fn register(&self, app: &mut Registry) {
        app.listen(|event: OrderPlaced, state| async move {
            renox::db::sql("UPDATE products SET stock = stock - 1 WHERE id IN \
                            (SELECT product_id FROM order_items WHERE order_id = ?)")
                .bind(event.order_id)
                .execute(&state.db)
                .await?;
            Ok(())
        });
    }
}

/// The app: a listener of its own (queue the receipt), plus the stock module's.
fn app() -> App {
    App::new()
        .job::<SendReceipt>()
        .listen(|event: OrderPlaced, state| async move {
            state.dispatch(SendReceipt { order_id: event.order_id }).await?; // slow work: queue it
            Ok(())
        })
        .module(Stock)
}

/// A handler that places an order: it emits the event, then goes to the orders page.
async fn place(State(state): State<AppState>) -> Result<Redirect> {
    state.emit(OrderPlaced { order_id: 1 }).await?;
    Ok(Redirect::to("/orders"))
}
```

What's going on:

- `OrderPlaced` is a plain struct. `impl Event for OrderPlaced {}` makes it an event. It must
  be `Clone`, `Send`, `Sync` and `'static`.
- The `Stock` module adds a listener in its `register`. The app adds one with `App::listen`.
- A listener says which event it wants by the type of its first argument (`event:
  OrderPlaced`).
- `place` emits the event. Both listeners run.

How emitting works, in detail:

- `state.emit(event)` runs every listener for that event type **right now, one after another,
  inside the code that called `emit`**. First those added with `App::listen`, then the
  modules' (registered at boot), each module's in its own order. Each listener gets its own
  clone (copy) of the event.
- If one listener fails (or panics), the others still run. `emit` returns the first error, so
  the `?` in a handler turns it into a 500 error page. Every failure is written to the log.
- Built-in events (`LoggedIn`, `LoginFailed`, `Registered`, …) are in `renox::auth::events`;
  see the cheat-sheet's "Auth events and the audit log".

> [!IMPORTANT]
> Listeners can't be queued. A listener that has slow work to do (sending mail, calling another
> service) should **dispatch a job** (put it on the queue), as the app's listener above does,
> and return right away.

> [!NOTE]
> **Coming from Laravel:** `state.emit(…)` is `event(…)`, and `App::listen` /
> `Registry::listen` replace the `EventServiceProvider`. Instead of a `ShouldQueue` listener,
> write a listener that dispatches a job.

To start a new event, `rnx make:event OrderPlaced --module orders` writes the event in the
module and registers a listener for it in the module's `register`.

### Testing events

`app.fake_events()` changes how events work in a test: from then on, events are **recorded**
instead of running their listeners. Then you check what was recorded:

```rust
use renox::prelude::*;
use renox::testing::TestApp;

/// A small event for the test.
#[derive(Clone, Debug)]
struct OrderPlaced { order_id: i64 }
impl Event for OrderPlaced {}

/// Checks that emitting the event is recorded, with the right order id.
async fn placing_an_order_emits_the_event() -> Result {
    let app = TestApp::new(App::new()).await;
    app.fake_events();
    app.state().emit(OrderPlaced { order_id: 7 }).await?; // usually: app.post("/orders", …)
    app.assert_emitted::<OrderPlaced>(|e| e.order_id == 7);
    assert_eq!(app.emitted::<OrderPlaced>().len(), 1);
    Ok(())
}
```

- `assert_emitted::<E>(check)` passes when an event of type `E` was emitted that matches the
  check.
- `emitted::<E>()` gives every recorded event of that type.
- `assert_not_emitted::<E>()` checks that none was emitted.

> [!TIP]
> To test a listener itself, don't fake events. Let them run for real, and check what the
> listener did: rows in the database, `app.queued_jobs()`, `app.sent_mail()`.

## Cache

Some answers take a long time to work out, like counting every product in a big table. The
**cache** keeps such a value for a while, so the next time you just read it.

`state.cache` stores values as JSON. `CACHE_STORE` picks the **store** (where values are kept):

| `CACHE_STORE` | Where | Shared with |
|---|---|---|
| `memory` (default) | inside this process | nothing: lost when the app restarts |
| `database` | the `cache` table | `queue:work` processes and other servers on the same database |

With `database`, a few other things are shared between servers too: rate limits, the login
lock (after too many wrong passwords), and the queue's job middleware.

Here is every cache method, one per line:

```rust
use renox::prelude::*;
use std::time::Duration;

/// Shows each cache method once.
async fn demo(state: AppState) -> Result {
    let cache = &state.cache;
    // remember: read "products.count"; if it's missing or too old, count again and keep it.
    let count: i64 = cache
        .remember("products.count", Duration::from_secs(60), || async {
            renox::db::sql("SELECT COUNT(*) FROM products").scalar(&state.db).await.map_err(Into::into)
        })
        .await?; // computed once a minute; errors are not cached

    cache.put("banner", &"Sale today", Some(Duration::from_secs(3600))).await?; // None: no expiry
    let banner: Option<String> = cache.get("banner").await?;
    let present = cache.has("banner").await?;
    let first = cache.add("welcome-sent:7", &true, None).await?; // false if already there
    let code: Option<String> = cache.pull("otp:7").await?; // read and remove
    let views = cache.increment("views:home", 1).await?; // atomic; 1 the first time
    cache.decrement("stock:42", 1).await?;
    cache.forget("banner").await?;
    cache.flush().await?; // everything the app cached; keeps the framework's `renox:` rows
    let _ = (count, banner, present, first, code, views);
    Ok(())
}
```

What each method does:

| Method | What it does |
|---|---|
| `remember(key, time, compute)` | Reads the value. If it isn't there, runs `compute`, keeps the result for `time`, and returns it. Errors are not kept. |
| `put(key, value, time)` | Keeps a value, for `time` (`None`: it never expires). |
| `get(key)` | Reads a value (`None` if it isn't there). |
| `has(key)` | Says whether a value is there. |
| `add(key, value, time)` | Keeps the value only if the key isn't there yet. Returns `false` if it was. |
| `pull(key)` | Reads a value and removes it. |
| `increment(key, n)`, `decrement(key, n)` | Adds or subtracts a number. The first `increment(…, 1)` gives 1. |
| `forget(key)` | Removes one value. |
| `flush()` | Removes everything the app cached, but keeps the framework's `renox:` rows. |

Good to know:

- `get` gives an error when the stored value is a different type from the one you asked for.
  `remember` computes the value again instead (useful after a new version of the app changed
  the type).
- If several calls to `remember` for the same key happen at once in one process, the value is
  computed only once.
- `add` and `increment` are **atomic**: they can't be cut in half by another caller. So when
  several callers `add` the same key, exactly one gets `true`, and no count is ever lost.
- When `increment` or `decrement` finds no value for the key, it starts from 0, and that new
  count never expires.
- With the database store, expired rows are deleted now and then while values are written, and
  by `cache.prune()` / `my-app cache:prune`.

> [!WARNING]
> Keys that start with `renox:` belong to the framework: scheduler claims, locks, counters,
> unique jobs, queue throughput. Don't start your own keys with `renox:`.

### Locks

A **lock** makes sure only one process at a time does something: sending a report, working on
one order. It's like a "do not disturb" sign that only one person can hang up at a time.

- With `CACHE_STORE=database`, a lock works across processes and servers.
- With `memory`, it works only within one process.

```rust
use renox::prelude::*;
use std::time::Duration;

/// Builds the sales report, unless someone else already is.
async fn sales_report(state: AppState) -> Result {
    // Held for 5 minutes at most, so a crashed holder doesn't block others forever.
    let lock = state.cache.lock("sales-report", Duration::from_secs(300));
    let Some(guard) = lock.try_acquire().await? else {
        return Ok(()); // someone else is on it
    };
    // … build and send the report …
    guard.release().await?; // or let it drop (released in the background)
    Ok(())
}

/// Works on one order, waiting a little if someone else is working on it.
async fn work_on_order(state: AppState, order_id: i64) -> Result {
    let lock = state.cache.lock(&format!("order:{order_id}"), Duration::from_secs(30));
    let guard = lock.block(Duration::from_secs(5)).await?; // waits up to 5 s, then 423 Locked
    // … one process at a time …
    drop(guard);
    Ok(())
}
```

What's going on:

- `state.cache.lock(name, ttl)` names a lock. The **ttl** ("time to live") is the longest the
  lock is held: if the holder crashes, the lock frees itself after that time.
- `try_acquire()` tries once. It gives a **guard** if it got the lock, or `None` if someone
  else has it.
- `block(time)` waits up to that time for the lock, then fails with `423 Locked`.
- The lock is freed by `guard.release()`, or when the guard is dropped (then the release
  happens in the background).

More lock methods:

- `lock.is_held()` says whether anyone holds the lock.
- `lock.force_release()` frees it, whoever holds it (useful in an admin command).
- `guard.release()` returns `false` when the ttl had already run out and someone else took the
  lock in the meantime.

> [!NOTE]
> **Coming from Laravel:** `Cache::lock()->get()` is `cache.lock(name, ttl).try_acquire()`, and
> `->block(5)` is `.block(Duration)`.

## Commands

Your app's program is also a **command-line tool**: you can type `my-app migrate`,
`my-app queue:work` or `my-app help` in a terminal. While you develop, `rnx <command>` does the
same, building the app first (through `cargo run`). You can add your own commands too.

> [!NOTE]
> **Coming from Laravel:** this is like `php artisan`.

### A plain command

`App::command(name, about, run)` adds a command (in a module, use `Registry::command`). The
words typed after the command's name arrive as `Args`:

```rust
use renox::prelude::*;
use renox::command::Args;

/// The command `admin:create --email E --password P`: makes an admin user.
async fn create_admin(args: Args, state: AppState) -> Result {
    // Both flags are needed; if one is missing, say how to use the command.
    let (Some(email), Some(password)) = (args.value("--email"), args.value("--password")) else {
        return Err(Error::BadRequest("usage: admin:create --email E --password P".into()));
    };
    User::register(&state.db, "Admin", email, password).await?;
    println!("Created {email}.");
    Ok(())
}

/// The app, with the command registered under its name and a one-line description.
fn app() -> App {
    App::new()
        .module(Auth::new())
        .command("admin:create", "Create an admin user (--email, --password)", create_admin)
}
```

`Args` can tell you:

- `value("--flag")`: the value after a flag, written `--flag v` or `--flag=v`;
- `has("--flag")`: whether the flag was typed;
- `positional()`: the words that aren't flags or their values;
- `all()`: every word.

### A typed command (clap)

For commands with several options, a **typed command** is easier. You describe the options
in a struct, and [clap](https://docs.rs/clap) (a popular Rust library for command-line
options, re-exported as `renox::clap`) reads and checks them for you. It also prints the usage
for `my-app entries:prune --help`.

The struct is an `AppCommand`. Its doc comment becomes its line in `my-app help`:

```rust
use renox::prelude::*;
use renox::clap;
use renox::command::AppCommand;

/// Delete entries older than --days (default 30).
#[derive(clap::Parser)]
#[command(name = "entries:prune")]
struct PruneEntries {
    /// Keep entries younger than this many days.
    #[arg(long, default_value_t = 30)]
    days: i64,
    /// Don't ask first.
    #[arg(long)]
    force: bool,
}

impl AppCommand for PruneEntries {
    /// What the command does. `self` holds the options, already read and checked.
    async fn run(self, state: AppState) -> Result {
        // Ask first, unless --force was given.
        let question = format!("Delete entries older than {} days?", self.days);
        if !self.force && !renox::prompt::confirm(&question, true).await? {
            return Ok(());
        }
        let cutoff = renox::db::now() - renox::chrono::TimeDelta::days(self.days);
        let deleted = renox::db::sql("DELETE FROM entries WHERE created_at < ?")
            .bind(cutoff)
            .execute(&state.db)
            .await?;
        println!("Deleted {deleted} entries.");
        Ok(())
    }
}

/// The app, with the typed command.
fn app() -> App {
    App::new().typed_command::<PruneEntries>()
}
```

What's going on:

- `#[command(name = "entries:prune")]` is the name you type.
- Each field is an option: `--days 7` fills `days` (30 if you leave it out), and `--force`
  sets `force` to `true`.
- The `///` lines on the fields become the help text for each option.
- `run` asks a yes/no question (see [Asking questions](#asking-questions)), then deletes the
  old entries.

To start a new one, `rnx make:command entries:prune --module guestbook` writes a typed command
and registers it in the module's `register`.

### How commands run

- **When:** after the app boots. The database is connected, but migrations are **not** run for
  you.
- **Context:** commands run in a `renox::context` scope, like jobs and requests.
- **Errors:** an error ends the command with `Error: …` and a non-zero exit code (which tells
  scripts it failed). A clap error prints what's wrong, and the usage.
- **Names:** a command's name can't clash. A name that is built in (`migrate`, `queue:work`,
  `serve`, `help`, …) or registered twice stops the app at boot. So does an empty name, or one
  with spaces.
- **From modules:** modules add their own: `tokens:prune` and `notifications:prune --days N`
  with `Auth`, `audit:prune --days N` with `Audit`.

### Asking questions

`renox::prompt` asks questions in the terminal and reads the answers:

```rust
use renox::prelude::*;
use renox::prompt;

/// Makes a user, asking for anything that wasn't given as a flag.
async fn create_user(args: renox::command::Args, state: AppState) -> Result {
    // Use --email if it was typed; otherwise ask for it.
    let email = match args.value("--email") {
        Some(email) => email.to_owned(),
        None => prompt::ask("Email").await?, // asks until something is typed
    };
    let name = prompt::ask_or("Name", "Admin").await?; // empty answer: the default
    let password = prompt::secret("Password").await?; // not echoed
    let role = prompt::choice("Role", &["admin", "staff"], Some("staff")).await?; // text or number
    if prompt::confirm(&format!("Create {email} as {role}?"), true).await? {
        User::register(&state.db, &name, &email, &password).await?;
    }
    Ok(())
}
```

| Function | What it does |
|---|---|
| `ask(question)` | Asks, and asks again until something is typed. |
| `ask_or(question, default)` | Asks; an empty answer gives the default. |
| `secret(question)` | Asks without showing what's typed (for passwords). |
| `choice(question, options, default)` | Asks to pick one option, by its text or its number. |
| `confirm(question, default)` | Asks a yes/no question. |

Questions are written to stderr (the error output), so they don't mix with the command's
normal output.

When there is no terminal (in a pipe, a cron job, or CI), answers are read from stdin (the
input), one line per question. If there's no answer there either:

- a question with a default takes the default;
- a question without one fails, with an error naming the missing answer.

> [!TIP]
> Give commands that run unattended a flag for everything, like the `--force` above, so they
> never need to ask.

> [!NOTE]
> **Coming from Laravel:** these are Laravel's `ask`, `secret`, `confirm` and `choice`.

### Testing commands

`app.kernel().call(name, args)` runs a command just as the program would.
`renox::prompt::answering` gives it the answers, one per question, in order:

```rust
use renox::prelude::*;
use renox::testing::TestApp;

/// Answers "no", checks nothing was deleted, then answers "yes".
async fn the_command_asks_first(app: TestApp) -> Result {
    renox::prompt::answering(["no"], app.kernel().call("entries:prune", ["--days", "7"])).await?;
    app.assert_database_count("entries", 2).await; // nothing deleted
    renox::prompt::answering(["yes"], app.kernel().call("entries:prune", ["--days", "7"])).await?;
    Ok(())
}

/// Runs a built-in command the way the real program does.
async fn the_binary_runs_a_built_in() -> Result {
    App::new().run_args(["migrate:status"]).await // any command, output on stdout
}
```

> [!TIP]
> If the test moved the clock with `travel`, wrap the call in `app.at_travelled_time(…)`, so
> the command sees the moved clock too. examples/hello's
> `the_prune_command_deletes_old_entries` does this.

## Coming from Laravel

If you know Laravel, this table maps what you know to Renox. If you don't, you can skip it.

| Laravel | Renox |
|---|---|
| `$schedule->command(…)->everyFiveMinutes()` | `s.every_minutes(5, name, task)` |
| `->hourly()`, `->dailyAt('02:00')`, `->weeklyOn(1, '7:00')`, `->monthlyOn(1, '00:05')` | `hourly`, `daily_at`, `weekly_on(Weekday::Mon, …)`, `monthly_on` |
| `->cron('30 9 * * 1-5')` | `s.cron("30 9 * * 1-5", name, task)` |
| `->weekdays()`, `->weekends()`, `->days([…])`, `->between()` | the same names on `ScheduledTask` |
| `->timezone()`, `schedule_timezone` | `.timezone(…)`, `APP_TIMEZONE` |
| `->onOneServer()`, `->withoutOverlapping()` | built in: each run is claimed in the database, and a task still running in the process is skipped (use a cache lock for more) |
| `->onFailure()`, `->onSuccess()` | `on_failure`, `on_success` |
| `->pingBefore()`, `->thenPing()`, `->pingOnSuccess()`, `->pingOnFailure()` | the same, snake_case |
| `schedule:work`, `schedule:list`, `schedule:test` / `schedule:run` | `schedule:work`, `schedule:list`, `schedule:run NAME` (no cron entry needed: `serve` runs it) |
| `Event::dispatch()`, `event(…)` | `state.emit(event).await?` |
| `Event::listen()`, `EventServiceProvider` | `App::listen`, `Registry::listen` in a module |
| `ShouldQueue` listeners | a listener that dispatches a job |
| `Event::fake()`, `Event::assertDispatched()` | `app.fake_events()`, `app.assert_emitted::<E>(…)` |
| `Cache::get/put/remember/add/pull/increment/forget/flush` | `state.cache.` the same names |
| `Cache::lock()->get()`, `->block(5)` | `cache.lock(name, ttl).try_acquire()`, `.block(Duration)` |
| `CACHE_DRIVER=database` | `CACHE_STORE=database` |
| `php artisan make:command`, `$signature` | `rnx make:command`, a clap `AppCommand` |
| `$this->ask/secret/confirm/choice` | `renox::prompt::ask/secret/confirm/choice` |
| `$this->artisan(…)->expectsQuestion()` | `prompt::answering([…], kernel.call(…))` |
| `Artisan::call()` | `kernel.call(name, args)`, `App::run_args` |
