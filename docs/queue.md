# The queue: jobs, retries, chains and batches

Some work is too slow to do while a visitor waits for a page: sending mail, calling another
service, making a report. The **queue** lets your app write that work down as a to-do item and
do it a moment later, in the background, while the visitor already sees their page.

In this guide:

- [A job](#a-job): write one, register it and send it to the queue
- [Failures and retries](#failures-and-retries): what happens when a job goes wrong
- [Queuing only if the data is saved](#queuing-only-if-the-data-is-saved)
- [Queues and priority](#queues-and-priority): urgent work first
- [Unique jobs, rate limits and overlapping](#unique-jobs-rate-limits-and-overlapping)
- [Encrypted payloads](#encrypted-payloads), [chains and batches](#chains-and-batches),
  [testing](#testing-jobs) and [the dashboard](#the-dashboard)

### Words you'll meet

| Word | What it means |
|---|---|
| **job** | One piece of background work, like "send the invoice for order 7". In Renox it's a struct. |
| **queue** | The waiting line of jobs. A job is **dispatched** (put in the line) and waits its turn. |
| **worker** | A loop that takes the next job from the queue and runs it. |
| **payload** | The job's data (its struct's fields), stored as JSON while it waits. |
| **attempt** | One try at running a job. A job that fails may get more attempts. |
| **retry** | Running a failed job again. |
| **backoff** | How long to wait before the next retry. |
| **failed for good** | Out of attempts. The job moves to a list of failed jobs. |
| **transaction** | A group of database changes that are saved all together, or not at all. |
| **unique job** | A job that's only queued once, even if you dispatch it twice. |
| **middleware** | A check that runs before each attempt, like "only one at a time". |
| **chain** | Jobs that run one after another, in order. |
| **batch** | Many jobs that run side by side, tracked as one group. |
| **idempotent** | Safe to run twice: running it again does no extra harm. Good for jobs that may retry. |

### Where the queue lives

Renox's queue lives in your app's own database, in the tables `jobs`, `failed_jobs` and
`job_batches`. That works on SQLite and on PostgreSQL. So there is no extra queue server (like
Redis) to run.

`serve` runs the workers itself. How many is set by `QUEUE_WORKERS`, 2 by default.

More to read:

- the short version of every API: the [cheat-sheet](../CHEATSHEET.md) ("Jobs, events, schedule,
  mail");
- a whole business: [examples/bikeshop](../examples/bikeshop) (batches for its monthly report
  and statements, queued mails, listeners);
- running it in production (failed jobs, pruning): [operations.md](operations.md).

> [!NOTE]
> **Coming from Laravel:** this is Laravel's `database` queue driver, built in. There is no
> separate `queue:work` process to start unless you want one: `serve` runs the workers.

## A job

A **job** is a struct that can be turned into JSON (serializable), with a name that never
changes.

> [!TIP]
> Keep a job small: store ids, not whole models. The row may change before the job runs, and
> the payload is stored as JSON. Load the fresh row inside `handle`.

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The job: "send the invoice of this order". Only the order's id is stored.
#[derive(Serialize, Deserialize)]
struct SendInvoice {
    order_id: i64,
}

impl Job for SendInvoice {
    const NAME: &'static str = "send-invoice"; // stored with each job: don't rename while queued
    const MAX_ATTEMPTS: u32 = 5;                // default 3
    const TIMEOUT: Duration = Duration::from_secs(30); // per attempt; default 60 s

    /// The work itself. A worker calls this when the job's turn comes.
    async fn handle(self, ctx: JobContext) -> Result {
        // `ctx.state` is the app's state: database, mailer, settings and more.
        let mail = ctx.state.mail_view(
            "buyer@example.com",
            "Your invoice",
            "mail/invoice",
            context! { order_id => self.order_id },
        )?;
        ctx.state.mailer.send(mail).await
    }
}

/// Builds the app and tells it about the job.
fn app() -> App {
    App::new().job::<SendInvoice>() // every job type is registered once
}

/// A handler that queues two jobs: one now, one in an hour.
async fn paid(State(state): State<AppState>) -> Result<String> {
    let id = state.dispatch(SendInvoice { order_id: 1 }).await?;
    // `dispatch_after` waits the given time before the job may run.
    state.queue.dispatch_after(SendInvoice { order_id: 2 }, Duration::from_secs(3600)).await?;
    Ok(format!("queued as job {id}"))
}
```

What's going on:

- `NAME` is saved with each queued job, so the worker knows which struct to rebuild. Don't
  rename it while jobs with the old name are still waiting.
- `MAX_ATTEMPTS` is how many tries the job gets (3 if you leave it out). `TIMEOUT` is how long
  one try may take (60 seconds if you leave it out).
- `handle` does the work. It returns `Result`: `Ok(())` means done, an error means this attempt
  failed.
- `App::new().job::<SendInvoice>()` registers the job type. Each job type is registered once.
- `state.dispatch(…)` puts a job in the queue and returns its id. The handler doesn't wait for
  the job to run.

You don't have to type this yourself: `rnx make:job SendInvoice --module orders` writes it and
registers it.

## Failures and retries

Jobs sometimes fail: a mail server is down, an API answers slowly. Here's what Renox does:

- **A failed attempt** is an error, a panic or a run that takes longer than `TIMEOUT`.
- **Then it waits and tries again.** The wait is `backoff(attempt)`: 10 seconds × the attempt
  number by default. It keeps trying up to `MAX_ATTEMPTS`.
- **Out of attempts:** the job moves to `failed_jobs` (it has failed for good), and its
  `failed` hook runs once. The error is also sent to your error reporters (`App::report`).
- **Jobs that can't run at all** fail for good on the first try, with no retries: a job whose
  name has no registered job type, and a payload that can't be read back (it doesn't decode as
  the struct, or it was sealed with another `APP_KEY`).
- **No retries for hopeless errors:** an error made with `Error::permanent(e)` skips the
  retries. Retrying can't fix a bad card number.
- **A worker that dies mid-job** (a crash, `SIGKILL`) leaves the job reserved (marked as taken).
  Another worker takes it again after 15 minutes, or after `TIMEOUT` + 1 minute for longer
  jobs.
- **A worker that dies during the last attempt:** the job has no attempts left, so a worker
  later moves it to `failed_jobs` with the error "the worker stopped during the last attempt".
  As for any job that fails for good, its `failed` hook then runs once (in the worker that
  found it) and the error goes to your error reporters (`App::report`).

> [!TIP]
> Because a job may run more than once, try to make it **idempotent**: safe to run twice. For
> example, check "is this invoice already sent?" before sending it.

Commands for failed jobs:

| Command | What it does |
|---|---|
| `queue:failed` | lists the failed jobs |
| `queue:retry ID` or `queue:retry all` | puts them back in the queue |
| `queue:forget ID` | deletes one |
| `queue:flush` | deletes them all |
| `queue:prune-failed --hours 168` | deletes the ones older than that |

The same things from code, on `state.queue`:

- `failed()` lists the failed jobs (`FailedJob`: id, queue, job, payload, error, failed_at);
- `retry(id)` and `retry_all()` put them back in the queue;
- `forget_failed(id)` deletes one, `flush_failed()` deletes them all;
- `pending()` counts the jobs waiting or running;
- `recent_batches(n)` returns the newest `n` batches with their progress.

Inside `handle`, `ctx.attempt` is the attempt number (1 on the first try) and `ctx.id` is the
job's id (0 when it runs through `dispatch_sync`).

Here's a job that sets its own backoff, refuses to retry bad input, and does something when it
fails for good:

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Charges a card for an order.
#[derive(Serialize, Deserialize)]
struct Charge {
    order_id: i64,
    card: String,
}

impl Job for Charge {
    const NAME: &'static str = "charge";

    /// How long to wait before the next try: one more minute each time.
    fn backoff(attempt: u32) -> Duration {
        Duration::from_secs(60 * u64::from(attempt)) // 1, 2, 3 minutes
    }

    /// A card number that isn't a number fails for good, with no retries.
    async fn handle(self, _ctx: JobContext) -> Result {
        let _card: u64 = self.card.parse().map_err(Error::permanent)?; // no retry for bad input
        Ok(())
    }

    /// Runs once, when the job has failed for good. `error` says what went wrong.
    async fn failed(self, ctx: JobContext, error: String) {
        let state = ctx.state;
        // Once, after the last attempt: tell someone.
        eprintln!("charge {} failed for good: {error}", self.order_id);
        let _ = state; // e.g. notify the customer through state.mailer
    }
}
```

## Queuing only if the data is saved

Here's a sneaky bug. You save an order inside a **transaction** (a group of changes that are
saved together, or not at all), and you dispatch a job for it. If the job runs before the
transaction is saved (commits), it looks for the order and finds nothing. Or the transaction is
cancelled (rolled back), and the job runs for an order that doesn't exist.

`dispatch_in` fixes this. It writes the job inside the same transaction, so the job exists only
if the transaction commits:

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// A small job, just for this example.
#[derive(Serialize, Deserialize)]
struct SendInvoice { order_id: i64 }

impl Job for SendInvoice {
    const NAME: &'static str = "send-invoice";
    async fn handle(self, _ctx: JobContext) -> Result { Ok(()) }
}

/// Saves an order and queues its invoice, all or nothing.
async fn checkout(State(state): State<AppState>) -> Result {
    // Start a transaction: nothing below is saved until `commit`.
    let mut tx = state.db.begin().await?;
    let order_id: i64 = renox::db::sql("INSERT INTO orders (total) VALUES (?) RETURNING id")
        .bind(75_000)
        .scalar(&mut tx)
        .await?;
    // The job is written in the same transaction as the order.
    state.queue.dispatch_in(&mut tx, SendInvoice { order_id }).await?;
    tx.commit().await?; // no commit, no job
    Ok(())
}
```

## Queues and priority

You can have more than one waiting line. Each job goes into one queue, picked by:

- the job's `QUEUE` constant (`"default"` if you leave it out), or
- `dispatch_on(queue, job)`, which picks the queue for this one dispatch.

Workers started with `queue:work --queue high,default` always take jobs from `high` first, and
only then from `default`:

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// The "reset your password" mail: a person is waiting for it.
#[derive(Serialize, Deserialize)]
struct ResetPasswordMail { user_id: i64 }

impl Job for ResetPasswordMail {
    const NAME: &'static str = "reset-password-mail";
    const QUEUE: &'static str = "high"; // someone is waiting for it
    async fn handle(self, _ctx: JobContext) -> Result { Ok(()) }
}

/// Sends the same job to another queue, just this once.
async fn resend(State(state): State<AppState>) -> Result {
    state.queue.dispatch_on("default", ResetPasswordMail { user_id: 1 }).await?; // not urgent this time
    Ok(())
}
```

> [!IMPORTANT]
> The workers that `serve` runs (`QUEUE_WORKERS`) take jobs from **every** queue, oldest job
> first, with no queue before another. So does `queue:work` without `--queue`. Queue names
> alone don't give you priority.

To get priority, do one of these:

- set `QUEUE_WORKERS=0` and run `queue:work --queue high,default` next to `serve`;
- or give the urgent queue workers of its own.

`queue:work` takes these options:

- `--queue a,b`: only these queues; the first one listed is emptied first;
- `--workers N`: how many jobs run at once (1 by default);
- `--once`: run the jobs available now, then exit (for example from cron or a test script).

## Unique jobs, rate limits and overlapping

Sometimes a job should not run twice, or not too often. Renox has three tools for that.

**Unique jobs.** Give the job `UNIQUE_FOR`. Then a second dispatch, while a job with the same
`unique_id` is still queued or running, doesn't add another job: it returns the id of the one
already queued. Think of a "remind me" button pressed twice. The claim is dropped when the job
finishes, and in any case after `UNIQUE_FOR`, even if the job is still waiting.

**Middleware.** These checks run before each attempt. A job that can't run yet is put back in
the queue, without using up one of its attempts:

- `Middleware::without_overlapping(key)`: only one job with that key runs at a time. A job that
  would overlap is put back for 5 seconds; change that with `.release_after(wait)`;
- `Middleware::rate_limited(key, max, per)`: at most `max` attempts per time window, for
  example to stay inside another service's limit (its quota). The window is fixed (it starts
  over every `per`, it doesn't slide) and is at least 1 second; a job over the limit is put back
  until the window ends.

> [!WARNING]
> Both middlewares keep their notes in the cache. With `CACHE_STORE=memory`, they only work
> within one process. When several servers run workers, use `CACHE_STORE=database`. Unique
> claims always use the database.

```rust
use renox::prelude::*;
use renox::queue::Middleware;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Updates a shop's stock from its supplier.
#[derive(Serialize, Deserialize)]
struct SyncStock { shop_id: i64 }

impl Job for SyncStock {
    const NAME: &'static str = "sync-stock";
    const UNIQUE_FOR: Option<Duration> = Some(Duration::from_secs(600));

    /// What makes two jobs "the same": here, the shop.
    fn unique_id(&self) -> String {
        self.shop_id.to_string() // one queued sync per shop
    }

    /// The checks to pass before each attempt.
    fn middleware(&self) -> Vec<Middleware> {
        // One sync per shop at a time (try again 10 s later), and at most 60 a minute in all.
        vec![
            Middleware::without_overlapping(format!("shop:{}", self.shop_id))
                .release_after(Duration::from_secs(10)),
            Middleware::rate_limited("supplier-api", 60, Duration::from_secs(60)),
        ]
    }

    async fn handle(self, _ctx: JobContext) -> Result { Ok(()) }
}
```

What's going on:

- `UNIQUE_FOR` of 600 seconds and `unique_id` = the shop's id: one queued sync per shop.
- `without_overlapping` keeps two syncs of the same shop from running at once.
  `release_after(10 s)` sets how long a job that would overlap waits before it tries again.
- `rate_limited("supplier-api", 60, 60 s)` allows at most 60 attempts a minute, shared by every
  job that uses the key `supplier-api`.

## Encrypted payloads

Some jobs carry personal data or secrets. Add `const ENCRYPTED: bool = true` to such a job, and
its payload is stored sealed (encrypted) with `APP_KEY`, in `jobs` and in `failed_jobs` alike.

> [!WARNING]
> A payload sealed with another key can't be opened, so the job fails for good. Keep `APP_KEY`
> the same (see the "Keys" paragraph in [operations.md, Backups](operations.md#backups)).

## Chains and batches

A **chain** runs jobs one after another, in order. If one fails for good, the rest wait in
`failed_jobs`, and `queue:retry` picks the chain up where it stopped.

A **batch** runs many jobs side by side and keeps count of them. You can add jobs to run when
it's over:

- `then`: runs if every job succeeded;
- `catch`: runs at the first failure, which also cancels the rest, unless you call
  `allow_failures()`;
- `finally`: runs either way.

These callbacks are jobs too, not closures, because a closure can't be stored in the database.

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// A job that just prints its name, to show the order things run in.
#[derive(Serialize, Deserialize)]
struct Step { name: String }

impl Job for Step {
    const NAME: &'static str = "step";
    async fn handle(self, ctx: JobContext) -> Result {
        eprintln!("{} (batch {:?})", self.name, ctx.batch_id);
        Ok(())
    }
}

/// A shorthand to make a `Step`.
fn step(name: &str) -> Step {
    Step { name: name.to_owned() }
}

/// A chain: charge, then send the receipt, then ship.
async fn fulfil(State(state): State<AppState>) -> Result {
    state.queue.chain().then(step("charge")).then(step("receipt")).then(step("ship")).dispatch().await?;
    Ok(())
}

/// A batch of 100 statements, run side by side.
async fn statements(State(state): State<AppState>) -> Result<String> {
    let mut batch = state.queue.batch("monthly-statements");
    for customer in 1..=100 {
        batch = batch.push(step(&format!("statement {customer}")));
    }
    // Callbacks for the end; `allow_failures` keeps the batch going after a failure.
    let id = batch
        .then(step("all sent"))
        .catch(step("tell the admin"))
        .allow_failures()
        .dispatch()
        .await?;
    Ok(format!("/batches/{id}"))
}

// Poll it from the page (`hx-get` with `hx-trigger="every 2s"`) for a progress bar.
async fn progress(State(state): State<AppState>, Path(id): Path<i64>) -> Result<String> {
    let status = state.queue.batch_status(id).await?.ok_or(Error::NotFound)?;
    Ok(format!("{}% ({} failed)", status.progress(), status.failed))
}
```

What's going on:

- `fulfil` dispatches a chain. "receipt" starts only after "charge" is done, and "ship" after
  "receipt".
- `statements` builds a batch with `push`, one job per customer, then adds `then` and `catch`
  jobs and dispatches it. It returns the batch's id.
- `progress` reads the batch's status: how far along it is, and how many jobs failed. A page can
  ask for it every 2 seconds to show a progress bar.

Two more:

- `queue.cancel_batch(id)` skips the batch's jobs that haven't started yet.
- `queue:prune-batches --hours 24` deletes finished batches.

## Testing jobs

`TestApp` doesn't start workers, so jobs only run when your test says so:

- `app.queued_jobs()` lists what was queued (names, in order).
- `app.run_jobs().await` runs every job that is due, until none is left. That includes the next
  job of a chain and a batch's callbacks. A retry still waiting for its backoff is not due.
- `app.run_all_jobs().await` runs delayed jobs and retries too. It stops after 1,000 rounds, so a
  job that keeps queuing itself can't hang the test.
- `state.dispatch_sync(job)` runs one job now, in the caller.

Then check what the job did: the rows it wrote, or the mail it sent with `app.sent_mail()`.

A few more facts that help when writing jobs and tests:

- A batch's `then`, `catch` and `finally` jobs get the batch's id in `ctx.batch_id` (they aren't
  counted in the batch themselves). So `finally` can read `queue.batch_status(id)`.
- `Error::permanent_message("…")` fails a job for good, without an error type of your own.
- `renox::anyhow` is re-exported, for errors with extra context.

## The dashboard

The queue has a web page that shows what it's doing. `.module(renox::queue::Dashboard)` adds it
at `/_renox/queue`. It shows:

- jobs ready, delayed and running, per queue;
- how long the oldest job has waited;
- jobs done and failed in the last hour;
- the failed jobs, with buttons to retry one, retry all, or forget;
- recent batches with their progress.

It refreshes itself every 5 seconds.

> [!IMPORTANT]
> Only users who pass the `view-queue-dashboard` gate see it, in development too. So define the
> gate (see [authorization.md](authorization.md) for gates):

```rust
# use renox::prelude::*;
# let _ =
App::new()
    .module(renox::queue::Dashboard)
    .gate(renox::queue::DASHBOARD_GATE, |user| user.email == "ops@example.com")
# ;
```

For your own monitoring, `state.queue.stats()` returns the same numbers (`QueueStats`). For
example, send an alert when `oldest_wait` grows.

The counts of jobs done and failed are stored as one row per minute in the `cache` table
(`renox:queue:done:*`, `renox:queue:failed:*`). Those rows expire after two hours.

## Coming from Laravel

> [!NOTE]
> **Coming from Laravel:** the queue works much like Laravel's. This table maps the names.

| Laravel | Renox |
|---|---|
| `dispatch($job)`, `->delay()`, `->onQueue()` | `state.dispatch(job)`, `dispatch_after`, `dispatch_on` / `const QUEUE` |
| `$tries`, `$timeout`, `backoff()` | `MAX_ATTEMPTS`, `TIMEOUT`, `fn backoff` |
| `failed()` | `async fn failed(self, ctx, error)` |
| `ShouldBeUnique`, `uniqueFor`, `uniqueId` | `UNIQUE_FOR`, `fn unique_id` |
| `ShouldBeEncrypted` | `const ENCRYPTED: bool = true` |
| `WithoutOverlapping`, `RateLimited` middleware | `Middleware::without_overlapping`, `Middleware::rate_limited` |
| `Bus::chain`, `Bus::batch`, `then/catch/finally` | `queue.chain()`, `queue.batch(name)`, jobs as callbacks |
| `afterCommit` | `dispatch_in(&mut tx, job)` |
| `dispatchSync` | `state.dispatch_sync(job)` |
| Horizon | `.module(renox::queue::Dashboard)` (see "The dashboard") |
