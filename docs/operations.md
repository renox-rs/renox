# Running a Renox app in production

Your app works on your own computer. This guide is about what comes next: keeping it running
well on a real server, where real people use it, and fixing things when they go wrong.

It covers the settings for slow or broken services, the proxy in front of the app, health
checks, backups, and how to recover jobs and webhooks that failed. To set up the server itself,
see the `deploy/README.md` file that `rnx make:deploy` writes for you.

**In this guide**

- [The app's commands](#the-apps-commands): what you can run on the server
- [Timeouts](#timeouts) and [Behind a reverse proxy](#behind-a-reverse-proxy)
- [Several servers](#several-servers) and [`/health`](#health)
- [When a dependency fails](#when-a-dependency-fails): what the app does when something breaks
- [Failed jobs](#failed-jobs) and [Failed webhook calls](#failed-webhook-calls)
- [Outbound webhooks](#outbound-webhooks): telling other systems about events
- [Backups](#backups), [Deploys and migrations](#deploys-and-migrations) and
  [Deploys without refused connections](#deploys-without-refused-connections)
- [Sessions](#sessions), [Scheduled tasks and housekeeping](#scheduled-tasks-and-housekeeping)
  and [Maintenance mode](#maintenance-mode)
- [All settings](#all-settings)
- [Logs](#logs), [Error reports](#error-reports), [Error pages](#error-pages) and the
  [Debug inspector](#debug-inspector)

### Words you'll meet

| Word | What it means |
|---|---|
| **production** | The real, live copy of your app that visitors use (not the one on your laptop). |
| **server** | A computer that runs your app all the time and answers visitors' browsers. |
| **deploy** | Putting a new version of your app on the server and starting it. |
| **dependency** | Something the app needs to work, like the database or the mail server. |
| **timeout** | A time limit. When something takes longer, the app gives up and moves on. |
| **reverse proxy** | A program (like Caddy or nginx) that sits in front of your app, takes visitors' requests and passes them on to it. |
| **TLS / HTTPS** | The encryption that puts the lock in the browser's address bar. The proxy usually handles it. |
| **health check** | A small address that a monitor calls again and again to ask "are you OK?". |
| **queue, job, worker** | A job is a piece of work done in the background (like sending a mail). Jobs wait in a queue; workers take them out and run them. |
| **webhook** | A request another service (a payment provider, say) sends to your app to tell it something happened. |
| **backup** | A copy of your data kept somewhere else, so you can get it back if something goes wrong. |
| **log** | The lines the app writes about what it is doing, to read later when something goes wrong. |
| **connection pool** | A few open database connections that the app keeps ready and shares. |
| **systemd** | The program on most Linux servers that starts other programs and restarts them when they stop. |
| **migration** | A small SQL file that creates or changes a table. |

## The app's commands

Your app is also its own command-line tool. `my-app help` lists its commands. While you develop,
`rnx <command>` runs one through `cargo run` for you.

Run without a command, the app runs `serve`. That starts three things at once: the web server,
the queue workers and the scheduler.

| Command | What it does |
|---|---|
| `migrate`, `migrate:rollback [--step N]`, `migrate:fresh [--seed]`, `migrate:status` | Migrations (see Deploys and migrations). |
| `db:seed` | Runs the seeders (code that fills the database with starting data). |
| `db:shell` | Type SQL against the app's database (`.tables`, `.quit`), on SQLite or PostgreSQL. |
| `queue:work [--queue a,b] [--workers N] [--once]` | Runs workers in a process of their own. |
| `queue:failed`, `queue:retry <id\|all>`, `queue:forget <id>`, `queue:flush`, `queue:prune-failed [--hours N]`, `queue:prune-batches [--hours N]` | Failed jobs and finished batches (see Failed jobs). |
| `webhook:failed`, `webhook:retry <id>` | Failed webhook calls (see Failed webhook calls). |
| `webhook:endpoints`, `webhook:deliveries [--endpoint N] [--failed]`, `webhook:redeliver <id>`, `webhook:prune-deliveries [--days N]` | Outbound webhooks (see Outbound webhooks). |
| `sequence:list` | Lists the numbering series (`Sequence`, docs/relations.md "Numbering documents") and the last number of each. |
| `cache:prune`, `session:prune` | Deletes expired cache rows and database sessions. |
| `import:prune [--days N]` | Deletes the records of imports whose undo window ended more than N days ago (0): `renox::import::prune(&state, age)`. |
| `schedule:list`, `schedule:run <task>`, `schedule:work` | The scheduler (`schedule:work` runs it alone, when `serve` runs with `SCHEDULER=false`). |
| `db:check` | Compares the models registered with `App::model` with the tables the migrations build, and fails on a mismatch. |
| `db:diff [name] [--yes]` | Writes a migration for what changed in those models (`rnx make:migration --auto`). Review it before `migrate`. |
| `state:diagram [Model]` | Prints the Mermaid diagram of a state machine registered with `App::state_machine::<M>()` (the model or table name, ignoring case); without a name it lists the registered machines. |
| `view:check` | Compiles every template and checks its components and route names. |
| `route:list` | Every route with its name, module and guards. |
| `ui:publish [--force]` | Copies the UI kit into the app (`resources/views/components/ui.html`, `public/css/renox-ui.css`), so you can change it there. |
| `down [--secret S] [--retry N]`, `up` | Maintenance mode. |
| `tokens:prune`, `notifications:prune [--days N]` | Added by the `Auth` module: deletes expired API tokens (and device tokens, and expired login codes and links) and read notifications. |
| `audit:prune [--days N]` | Added by the `Audit` module. |
| `permissions:prune [--days N]` | Added by the `Permissions` module: deletes role assignments that ended more than N days ago (30). |

You can add your own commands (`App::command`, `App::typed_command`). They sit next to these,
but they can't use the same names.

> [!NOTE]
> **Coming from Laravel:** this is `php artisan`. Your app's binary plays that role, because
> your migrations and jobs are compiled into it.

## Timeouts

Sometimes a service the app needs stops answering. Without a time limit, every request would
wait for it, and requests would pile up until the app is stuck. With timeouts, requests fail
quickly instead, and the app keeps going.

All timeouts are in seconds. You set them in `.env`. (Every setting, with a comment, is in
the `.env.example` that `rnx new` writes; see also [All settings](#all-settings) below.)

| Setting | Default | What it limits |
|---|---|---|
| `DATABASE_ACQUIRE_TIMEOUT` | 5 | How long a query waits for a database connection. After that, the request answers 500 (an error). |
| `DATABASE_STATEMENT_TIMEOUT` | 30 | How long one PostgreSQL statement may run (`0` = no limit). It covers commands too: `migrate`, `db:seed`, `db:shell` and `queue:work`. For a long report, run `SET LOCAL statement_timeout = 0` inside its transaction. |
| `REQUEST_TIMEOUT` | 60 | How long a handler may take to answer (`0` = no limit). After that, the request answers 500. Work before the handler (loading the session and the user) is bounded too, one second later. Streaming a response and waiting for a slow visitor's connection don't count. |
| `MAIL_TIMEOUT` | 10 | How long sending one mail over SMTP may take, from connecting to the last reply. |
| (fixed) | 2 | How long `/health` waits for the database. |
| (fixed) | 5 | How long SQLite waits for another writer to finish (`busy_timeout`). |
| (fixed) | 30 | How long `serve` waits for running jobs after `SIGTERM` (the "please stop" signal), before stopping. |

"(fixed)" means you can't change it with a setting.

Jobs have their own limit: `Job::TIMEOUT` (60 s by default). A job that takes longer is
stopped and tried again later.

> [!WARNING]
> On PostgreSQL, a migration is cancelled when one of its statements runs longer than
> `DATABASE_STATEMENT_TIMEOUT` (a `CREATE INDEX` on a big table, a backfill). Raise the limit
> for that run only: `DATABASE_STATEMENT_TIMEOUT=0 my-app migrate`.

### Size limits

Two more settings decide how big things may get:

| Setting | Default | What it limits |
|---|---|---|
| `UPLOAD_MAX_SIZE` | 10 | The largest request body, in MB (uploads, forms, JSON). A larger request answers 413 ("too large"). A proxy in front has its own limit (nginx's `client_max_body_size` is 1 MB by default). |
| `DATABASE_POOL_SIZE` | 8 | How many database connections the app keeps open, shared by requests, queue workers and the scheduler (at least 1). With PostgreSQL, keep the total over all servers below the database server's `max_connections`. |

> [!TIP]
> If uploads over 1 MB fail behind nginx, raise nginx's `client_max_body_size`. The app's
> `UPLOAD_MAX_SIZE` doesn't help when nginx says no first.

## Behind a reverse proxy

On a server, the app doesn't usually face the internet by itself. Instead:

- the app listens only on `127.0.0.1` (`APP_HOST=127.0.0.1`), which means "this machine only";
- a **reverse proxy** such as Caddy or nginx faces the internet. It handles TLS (the `https://`
  part) and passes each request on to the app.

Here is a whole Caddy setup for that:

```text
# Caddyfile
shop.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

It says: requests for `shop.example.com` go to the app on port 3000 of this machine. Caddy
also gets and renews the HTTPS certificate by itself.

### `TRUSTED_PROXIES`: the visitor's real address

Behind a proxy, every connection seems to come from the proxy's address. The proxy writes the
visitor's real address in a header called `X-Forwarded-For` (or the standard `Forwarded`
header; the app reads `X-Forwarded-For` first, then `Forwarded`). Set `TRUSTED_PROXIES` so the
app believes that header when it comes from your proxy:

```text
TRUSTED_PROXIES=127.0.0.1          # Caddy or nginx on the same machine
TRUSTED_PROXIES=10.0.0.0/8         # a load balancer on a private network
TRUSTED_PROXIES=*                  # whoever connects (a platform whose proxy IPs you can't list)
```

Pick the one line that matches your setup. More about the value:

- it is a comma-separated list of addresses and CIDR ranges
  (`TRUSTED_PROXIES=127.0.0.1,10.0.0.0/8`);
- with a list, the app walks the forwarded addresses from the nearest hop back, skipping the
  proxies you listed, and takes the first address that isn't one of them;
- `*` trusts only the last hop: the address the connecting proxy added;
- `*` together with addresses (`*,10.0.0.1`), a bad address or a bad prefix stops the app at
  boot with an error.

Without this setting:

- `Routes::throttle` and named limiters (`throttle_by`) count every guest as one person;
- the login lock counts per email only;
- logs show the proxy's address instead of the visitor's.

> [!WARNING]
> Don't set `TRUSTED_PROXIES` when people can reach the app directly, without the proxy.
> Anyone could then send a fake `X-Forwarded-For` and pretend to be someone else.

In a handler, `ClientIp` gives you the same address the app uses.

### `TRUSTED_HOSTS`: the names your app answers to

Set `TRUSTED_HOSTS` to the host names the app serves (`example.com,*.example.com`). Requests
for any other `Host` then get a 400 ("bad request").

Why it matters: links in mails, like a password reset link, are built from the host of the
request. This setting stops an attacker from making your app send a mail whose link points to
their own site.

Two exceptions:

- `APP_URL`'s host is always allowed;
- `/health` answers for any host, because load balancers check it by IP address.

### `PERMISSIONS_POLICY`: browser features the app switches off

Every response carries a `Permissions-Policy` header. By default it turns off the browser
features a typical app never uses: accelerometer, autoplay, bluetooth, camera, display capture,
geolocation, gyroscope, HID, magnetometer, microphone, MIDI, payment, serial, USB and WebXR. A
compromised page, or a script injected into it, can't then switch on the camera or read the
location. The features Renox's own scripts use (the copy buttons' clipboard write, for
example) are not in the list.

- `PERMISSIONS_POLICY=default`: that list (also when the variable is empty or missing).
- `PERMISSIONS_POLICY=off` (also `false`, `none`): no header.
- `PERMISSIONS_POLICY=geolocation=(self), camera=()`: your own header value instead of the list.
  Write origins in quotes: `payment=(self "https://pay.example.com")`. A value the header can't
  carry stops the app at boot, naming the variable.

An app that really uses a feature allows it in code, on top of whichever base is set:
`app.permissions_policy(|p| { p.allow("geolocation", "self"); })`. A handler that sets its own
`Permissions-Policy` keeps it. `CSP=off` doesn't switch this header off.

An app that asked for the location without this header will now be refused it until it allows
`geolocation`: check before you deploy.

### Headers the proxy must keep

Routes for other hosts (`Routes::domain("admin.example.com", …)`) are picked by the `Host`
header, so the proxy must pass it on. Caddy does. With nginx, add
`proxy_set_header Host $host`.

An app with `App::detect_locale()` (which picks the language from the browser) answers with
`Vary: Accept-Language`. A CDN or caching proxy in front must keep that header. Otherwise it
could show one visitor's language to another.

### The notification bell's stream

The notification bell (`Auth::new().notifications()`) keeps a **Server-Sent Events** stream open
on each page: a request that stays open so the server can push news to the browser. It lives at
`/notifications/stream`. Each stream lasts at most five minutes; then the browser opens a new one.

Caddy passes it through as it is. nginx *buffers* responses (it waits to collect them before
sending), which would hold the news back. Turn that off for this address:

```nginx
location /notifications/stream {
    proxy_pass http://127.0.0.1:3000;
    proxy_buffering off;
    proxy_read_timeout 10m;
}
```

This sends `/notifications/stream` to the app without buffering, and lets a stream stay open for
up to ten minutes.

Each open stream asks the database for news every 15 seconds (two small queries for its user).
It also checks at once when something changes in the same process. When the app shuts down
gracefully, it ends them all.

The app's own live events (`state.broadcast`, see [mail.md](mail.md#your-own-live-events)) use
the same streams but aren't stored: they reach only the pages connected to the process that
sends them. With several servers, or with workers in a separate `queue:work` process, pages
connected elsewhere miss them. Keep broadcasts for "look again" hints, and store a database
notification for anything someone must see.

### `APP_URL`

Also set `APP_URL` to the public `https://` address, so links in mails point to the right place.

An `https://` `APP_URL` does two more things:

- it marks cookies `Secure`, so browsers send them over HTTPS only: the session, the app's
  `SetCookie`s and the maintenance bypass;
- with `APP_ENV=production` as well, every response sends HSTS
  (`Strict-Transport-Security`), which tells browsers to always use HTTPS for your site.

## Several servers

When one server isn't enough, several app servers can share one PostgreSQL database. Renox
makes sure they don't get in each other's way:

- the queue hands each job to one worker only (`FOR UPDATE SKIP LOCKED`);
- each scheduled run is claimed once, so it doesn't run on every server;
- migrations take turns.

Set `CACHE_STORE=database` as well. Then these live in the `cache` table, so they work across
servers: the cache, `Routes::throttle` and `throttle_by` limits, the login lock and cache locks
(`state.cache.lock`).

> [!WARNING]
> With the default `memory` store, each server counts on its own. So N servers allow N times
> the limit, and a lock only keeps out other tasks of the same process.

Sessions live in their encrypted cookie, or in the `sessions` table with
`SESSION_DRIVER=database` (see Sessions). Either way, any server can answer any request. You
don't need "sticky sessions" (sending a visitor always to the same server).

Uploaded files must be on shared storage (`STORAGE_DISK=s3`, see [File storage on
S3](#file-storage-on-s3)), or on the one server that has the disk.

### File storage on S3

S3 storage needs renox's `s3` feature (`features = ["s3"]` in `Cargo.toml`). It works with AWS
S3 and with services that speak the same language, such as Cloudflare R2 or MinIO. The settings:

| Setting | What it is |
|---|---|
| `STORAGE_DISK` | `local` (the default: files in `STORAGE_PATH/app`) or `s3` |
| `S3_BUCKET` | the bucket's name (required for `s3`) |
| `S3_REGION` | the region, e.g. `eu-west-1` (`auto` for R2) |
| `S3_ENDPOINT` | the service's address, for anything that isn't AWS (e.g. `https://<account>.r2.cloudflarestorage.com`) |
| `S3_ACCESS_KEY_ID`, `S3_SECRET_ACCESS_KEY` | the access key |
| `STORAGE_URL` | the public address of files under `public/` (the bucket's URL or a CDN) |

A disk added with `App::disk("backups", |config| StorageConfig::from_env(config, "BACKUPS"))`
reads its own settings, starting with the prefix you give:

- `BACKUPS_DISK` (`local` or `s3`, default `local`), `BACKUPS_PATH` (a local disk's folder,
  default `STORAGE_PATH/backups`), `BACKUPS_BUCKET` and `BACKUPS_URL`;
- `BACKUPS_REGION`, `BACKUPS_ENDPOINT`, `BACKUPS_ACCESS_KEY_ID` and
  `BACKUPS_SECRET_ACCESS_KEY`, which fall back to the `S3_*` settings when unset.

### Scanning uploads with ClamAV

`renox::upload::ClamAv` (see [Uploads](validation.md#steps-before-storing-scanning-images-encryption))
asks ClamAV's daemon, `clamd`, about each uploaded file. Renox links no ClamAV library; it speaks
`clamd`'s `INSTREAM` protocol on a socket.

```text
# Debian/Ubuntu
sudo apt install clamav-daemon clamav-freshclam   # freshclam keeps the signatures current
# /etc/clamav/clamd.conf: listen on TCP (or keep the default Unix socket and use ClamAv::unix)
TCPSocket 3310
TCPAddr 127.0.0.1
StreamMaxLength 25M      # at least UPLOAD_MAX_SIZE, or larger files are refused
```

```text
App::new().uploads(Pipeline::new().step(ClamAv::tcp("127.0.0.1:3310")))
```

- Bind `clamd` to `127.0.0.1` (or a private network): the protocol has no authentication.
- **A daemon that can't scan means the upload is refused** (it fails closed): `clamd` down, over
  `StreamMaxLength`, or silent for longer than the step's timeout (10 seconds; `.timeout(..)`).
  Renox logs a warning with the reason, so alert on it. `.fail_open()` accepts such files instead.
- `clamd` needs a few hundred MB of memory for its signatures and starts slowly; with Docker,
  `clamav/clamav` runs it as a service for the app to reach.
- In tests, point the step at a fake daemon that answers `stream: OK` or
  `stream: Eicar-Test-Signature FOUND` (crates/renox/tests/it/upload_pipeline.rs has one).

### Encrypted files and rotating `APP_KEY`

Files stored with `Upload::store_encrypted` or `Storage::put_encrypted` are sealed with
AES-256-GCM, each under its own data key; that key is sealed with a key derived from `APP_KEY` and
kept in the file's header. So **without the `APP_KEY` the files are unreadable**: keep it with
the backups (see [Backups](#backups)).

To rotate the key without losing the files, only the headers need re-sealing, not the contents:

1. Keep the old value, set the new `APP_KEY` (`rnx key:generate`).
2. Run a one-off command (an `AppCommand`, or a scheduled task) that calls
   `state.storage.rewrap_encrypted("", &old_app_key).await?` for the disk (and each
   `state.disk_named(..)` that holds encrypted files). It returns how many files it re-sealed,
   skips files that aren't encrypted or are already under the new key, and can be run again if it
   stops half-way.
3. Remove the old key once the count is 0 and the files read back.

Until step 2 has finished, files not yet re-sealed can't be read. Take a backup of the
bucket or folder first. Other things sealed with `APP_KEY` (sessions, signed links,
`Encrypted<T>` columns) are covered in [Backups](#backups).

## `/health`

`GET /health` is a small address for machines that check on your app: load balancers, uptime
monitors and container health checks.

- **200** with `"status": "ok"` when the database answered within 2 seconds.
- **503** with `"status": "error"`, and the reason in `"database"`, when it didn't.

The JSON answer also has:

- `queue.pending` and `queue.failed`: how many jobs are waiting and how many failed. Set up an
  alert for when `failed` grows.
- `maintenance`: whether the app is in maintenance mode.

Maintenance mode doesn't change the status code. That's on purpose: the load balancer keeps
sending visitors to the app, and they see the maintenance page.

## When a dependency fails

What does the app do when something it needs breaks? The table below answers that.

How each row is checked:

- the database and panic rows are checked on every change by the chaos test
  (`tests/chaos/run.sh`, run in CI on SQLite and PostgreSQL). It breaks things on purpose and
  watches what happens;
- the SMTP and killed-process rows are covered by the integration tests
  (`crates/renox/tests/it/background_resilience.rs`);
- the `SIGTERM` row is how `serve` shuts down, and has no automated test yet.

A **panic** is when Rust code crashes in the middle of running.

| Fault | What the app does |
|---|---|
| PostgreSQL stops | Requests get 500 within 8 s, and `/health` 503 within 4 s. A request that dispatches a job gets 500 too. When PostgreSQL is back, the app and its workers recover by themselves, and dispatching works again. No restart needed. |
| PostgreSQL hangs (paused, or the network is cut) | New requests get 500 within 8 s, and `/health` 503 within 4 s. A request already waiting on it gets 500 at `REQUEST_TIMEOUT`. |
| PostgreSQL restarts while a job runs | The job's result is written once the database is back, or the job is tried again. |
| SQLite locked by another process (a backup, a `sqlite3` shell) | Reads and `/health` keep working (thanks to SQLite's WAL mode). Writes get 500 within 9 s while the lock is held (SQLite first waits 5 s for it). Workers try again to write a job's result, so no job is left stuck. |
| A handler panics | That request gets a 500 error page, and the app keeps serving. |
| A job panics | The attempt counts as failed and the job is tried again. After the last attempt, it goes to `failed_jobs`. The worker keeps running. |
| A scheduled task or event listener panics | It's logged. The task runs again at its next time, and the other listeners still run. |
| The SMTP (mail) server is down or doesn't answer | A direct `mailer.send` fails within `MAIL_TIMEOUT`. Queued mail (`queue_mail`) and queued notifications get five attempts, then go to `failed_jobs`. With `MAIL_FAILOVER`, the next mailer in the list sends it first. |
| The process is killed (`SIGKILL`, out of memory) | Jobs it was running are tried again after 15 minutes, or at `TIMEOUT` + 1 minute for longer jobs. If that was their last attempt, they go to `failed_jobs`. |
| The process gets `SIGTERM` (a deploy, a restart) | It stops taking requests and waits up to 30 s for running jobs to finish. |

## Failed jobs

There are two ways to look after failed jobs:

- in the browser: `/_renox/queue` (the `renox::queue::Dashboard` module, behind the
  `view-queue-dashboard` gate) shows the queue and the failed jobs, with retry and forget
  buttons;
- in a shell: the commands below do the same.

For monitoring, `state.queue.stats()` has the numbers: ready jobs, the oldest wait, and jobs
done and failed in the last hour.

### When a job counts as failed

A job is tried several times (`Job::MAX_ATTEMPTS`, 3 by default). If every attempt fails, it
moves to the `failed_jobs` table, with its error.

Some jobs go there after the first attempt, because trying again can't help:

- a job whose error was made with `Error::permanent`;
- a job whose saved data (its *payload*) can no longer be read.

### The commands

```bash
my-app queue:failed          # list them, with their errors
my-app queue:retry 12        # put one back on the queue with fresh attempts
my-app queue:retry all
my-app queue:flush           # delete them all
my-app queue:forget 12       # delete one
my-app queue:prune-failed --hours 168    # delete those older than a week (the default)
my-app queue:prune-batches --hours 24    # delete finished batches
```

> [!IMPORTANT]
> Fix the cause first, then retry. Otherwise the job just fails again.

What a retry does:

- `queue:retry` runs the same payload again;
- a job that was part of a chain carries on with the chain;
- a job that was part of a batch counts in its batch again.

A job's `failed` hook runs once, when the job fails for good (not again on `queue:retry`).

To delete old failed jobs on a schedule, call the functions behind the prune commands from a
task (see Scheduled tasks and housekeeping).

### Workers in their own process

By default, `serve` runs the workers too. To run them in a separate process, start
`my-app queue:work` and set `QUEUE_WORKERS=0` for `serve`.

`queue:work` takes these options:

- `--queue a,b`: only these queues, and the first one listed is emptied first. Without it,
  every queue, in the order jobs arrived;
- `--workers N`: how many jobs run at once (1 by default);
- `--once`: run what is waiting now, then exit.

See [queue.md](queue.md#queues-and-priority).

## Failed webhook calls

Every webhook call your app accepts is saved in the `webhook_calls` table before it is handled.
The provider got its 200 ("OK"), so it won't send the call again. If handling it fails, it's
up to you to try again.

```bash
my-app webhook:failed        # calls whose handler failed, with the error
my-app webhook:retry 7       # process one again, e.g. after a fix
```

Each call has a status:

- `received` until its handler is done;
- `processed` when the handler succeeded;
- `failed` when it failed. A handler that panics also marks its call `failed`.

The handler runs as a queued job with five attempts. Each new attempt waits 30 s times the
attempt number. So a `failed` call may still succeed on a later attempt. After the fifth, it
stays failed until you run `webhook:retry`.

The saved payload is the exact body the provider sent, so its signature can be checked again.

The `renox-billing` crate's webhooks (Stripe, Xendit) are saved under the provider `billing`,
with event ids like `stripe:evt_…`. Retrying one is safe: applying an event twice changes
nothing, and an event older than what a subscription already shows is ignored.

## Outbound webhooks

An outbound webhook is the other direction: your app tells another system that something
happened, by POSTing a signed JSON message to a URL that system registered. Renox keeps the
registered URLs (endpoints), signs each message, sends it from the queue, retries what fails
and switches off an endpoint that keeps failing.

The format is [Standard Webhooks](https://www.standardwebhooks.com), so receivers can verify it
with any of the existing libraries. Each request is a JSON `POST` with three headers:

| Header | Value |
|---|---|
| `webhook-id` | `msg_…`, unique per event and the same on every retry (receivers use it to ignore duplicates). |
| `webhook-timestamp` | Unix seconds when this attempt was sent. |
| `webhook-signature` | `v1,<base64>`: HMAC-SHA256 over `{id}.{timestamp}.{body}`. Several signatures may be listed, separated by spaces. |

The body is `{ "id", "type", "created_at", "data" }`. An endpoint's secret is written
`whsec_<base64>`; the HMAC key is the decoded part. This is the specification's own test vector,
which Renox's signing reproduces exactly:

```rust
use std::time::Duration;
use renox::webhook::signing;

let secret = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
let id = "msg_p5jXN8AQM9LWM0D4loKWxJek";
let body = br#"{"test": 2432232314}"#;

let signature = signing::sign(secret, id, 1614265330, body)?;
assert_eq!(signature, "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE=");

// A timestamp far from now is refused, so a captured request can't be replayed later.
assert!(!signing::verify(secret, id, "1614265330", &signature, body, Duration::from_secs(300)));
# Ok::<(), renox::Error>(())
```

### Endpoints

`Endpoint::create` registers a URL for some event names (none: every event) and returns the
secret **once**. It is stored sealed with `APP_KEY`; `list` and `find` never show it, so give it
to the receiver now.

```rust
# use renox::prelude::*;
# async fn demo(state: AppState) -> Result {
use renox::webhook::outbound::Endpoint;

let (endpoint, secret) =
    Endpoint::create(&state, "https://example.com/hooks", "Billing", &["invoice.paid"]).await?;
assert!(secret.starts_with("whsec_")); // shown only now
assert!(endpoint.subscribes_to("invoice.paid"));
Endpoint::disable(&state.db, endpoint.id, "paused").await?; // `enable` turns it back on
# Ok(()) }
```

The URL must be `https`, except when `APP_ENV` is `local` or `test`. Renox sends with a 15 s
timeout and follows no redirects. It does not block private addresses, so don't let untrusted
people register endpoint URLs.

### Sending events

Name the events you publish once, in `Module::register` or on the `App`: the event type and a
function that makes the payload's `data`.

```rust
# use renox::prelude::*;
# #[derive(Clone)] struct InvoicePaid { id: i64, cents: i64 }
# impl Event for InvoicePaid {}
# let _ =
App::new().webhook_event::<InvoicePaid>("invoice.paid", |e| {
    serde_json::json!({ "invoice": e.id, "amount": e.cents })
})
# ;
```

After that a plain `state.emit(InvoicePaid { .. })` queues one delivery per enabled endpoint
subscribed to `invoice.paid`.

**Use `emit_in` when the event comes from a database change.** `emit_in` queues the deliveries
inside your transaction, so they are sent only if it commits and never if it rolls back. With
`state.emit` after the commit, a crash between the two loses the webhook; with `state.emit`
before it, a rollback sends a webhook for something that never happened.

```rust
# use renox::prelude::*;
# #[derive(Clone)] struct InvoicePaid { id: i64, cents: i64 }
# impl Event for InvoicePaid {}
# async fn demo(state: AppState) -> Result {
use renox::webhook::outbound::emit_in;

let mut tx = state.db.begin().await?;
// … update the invoice with `tx` …
emit_in(&state, &mut tx, &InvoicePaid { id: 7, cents: 4900 }).await?;
tx.commit().await?; // the deliveries are sent after this
# Ok(()) }
```

### Retries and switching off

Each delivery is a queued job, `DeliverWebhook`, with eight tries: at once, then after 5 s, 5 min,
30 min, 2 h, 5 h and 10 h (twice). A `5xx`, `429`, timeout or connection error is tried again;
any other `4xx` fails the delivery without a retry. A `2xx` is a success and resets the
endpoint's failure count.

- An answer of `410 Gone` disables the endpoint at once.
- A delivery that fails for good counts once. After `WEBHOOK_DISABLE_AFTER` (20) of them in a row
  the endpoint is disabled, and the event `EndpointDisabled { endpoint_id }` is emitted: listen to
  it to tell the owner.
- Every attempt writes a row in `webhook_deliveries` (status, response code, duration, attempt).
  The body is stored once there, so a redelivery sends the identical payload.

### Commands

```bash
my-app webhook:endpoints                       # every endpoint, whether it is enabled, its failures
my-app webhook:deliveries --failed             # newest first; also --endpoint 3
my-app webhook:redeliver 12                    # same body and webhook-id, fresh timestamp
my-app webhook:prune-deliveries --days 30      # delete delivery rows older than 30 days (the default)
```

`webhook:redeliver` refuses a disabled endpoint: enable it first. Run `webhook:prune-deliveries`
from a scheduled task, as the table grows with every event.

### Signed requests between your own apps

`Request::sign` signs an outgoing request in the same format, and `SignedRequest<T>` checks it
on the other side. The receiver registers each sender's key under a key id.

```rust
# use renox::prelude::*;
#[derive(serde::Deserialize)]
struct Ping { note: String }

async fn receive(SignedRequest(ping): SignedRequest<Ping>) -> String {
    ping.note
}

# let _ =
App::new().signing_key("billing", "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw")
# ;
// Sender: state.http.post(url).json(&body).sign("billing", &secret).send().await?
```

The extractor answers `401` with the same generic message for an unknown key id, a wrong
signature, a timestamp more than `WEBHOOK_TOLERANCE_SECS` (300) away from now, and a `webhook-id`
it has already seen inside that window. Call `.sign` after the body is set. Registering a key
again under the same id replaces it, which is how a key is rotated.

## Backups

A backup is only useful if it has everything you need to start again. Here is what to keep.

**SQLite.** The database is `storage/app.db`. While the app runs, it also has two helper files
next to it: `-wal` and `-shm`.

- For a continuous backup (copied all the time, as it changes), use Litestream (see
  `deploy/README.md`).
- For a one-off copy while the app runs: `sqlite3 storage/app.db ".backup backup.db"`.

> [!WARNING]
> Don't copy the database file with `cp` while the app runs. The copy can come out broken.

**PostgreSQL.** Use your provider's backups or `pg_dump`.

**Files.** Uploads live in `STORAGE_PATH/app` (or the S3 bucket). Each disk added with
`App::disk` lives in `STORAGE_PATH/<name>` (or its own bucket). Back those directories up too.

**Keys.** Keep `.env`'s `APP_KEY` with the backups. It's the app's secret key, and a lot
depends on it. Without it:

- sessions end, so everyone is logged out;
- signed links stop working (`signed_url`, the local disk's `temporary_url`s, email
  verification links);
- the app's encrypted cookies (`SetCookie::encrypted`, read with `Cookies::get_encrypted`) read
  as missing;
- maintenance bypass cookies stop letting people through;
- login links and codes that were sent but not used yet stop working (`one_time_codes` keeps
  keyed hashes; users ask for a new one).

> [!IMPORTANT]
> Some data is lost for good without the old key: `Encrypted<T>` model fields, values the app
> sealed with `state.encrypt`, and queued jobs with encrypted payloads (`const ENCRYPTED`).
> They can't be read any more unless you still have the old key.
>
> Changing (rotating) the key has the same effect. So decrypt such values and encrypt them
> again with the new key before you switch.

## Deploys and migrations

When you deploy, you usually run `migrate` first. Here is how it behaves:

- `migrate` runs all pending migrations as one **batch** (a group it can undo together).
  Several servers can run it during the same deploy: they take turns, and the later ones find
  nothing left to do.
- Each migration runs in a transaction, so a failed one leaves nothing behind. Migrations with
  `CREATE INDEX CONCURRENTLY` or a `-- renox:no-transaction` line run without one. Keep those
  to a single change.
- On PostgreSQL, `DATABASE_STATEMENT_TIMEOUT` (30 s) applies to migrations too. For a slow one,
  run `DATABASE_STATEMENT_TIMEOUT=0 my-app migrate`.
- `migrate:status` shows each migration's batch. It also flags migrations that already ran but
  whose file was edited or deleted since.

> [!IMPORTANT]
> To change the schema, add a new migration. Never edit one that has already run.

- Before shipping, run `db:check` and `view:check` (CI is a good place), or call
  `assert_models_match_schema` and `assert_views_compile` in a test (see [testing.md](testing.md)).
  `rnx doctor` says what an environment is missing: `.env`, `APP_KEY`, the database, pending
  migrations.
- Restart the app after `migrate`. (The Dockerfile and `deploy/README.md` already do: migrate,
  then start.) Migrations run inside the app itself (tests, `migrate:fresh`) are safe:
  connections opened before them are dropped. But a server that keeps running while another
  process migrates may keep connections that read the old schema, until it restarts.
- `migrate:rollback` undoes the last batch only if every migration in it has a `.down.sql`.
  Otherwise it undoes nothing.

## Deploys without refused connections

### The problem

A restart has a short gap. The old process stops (after finishing its running jobs), and the
new one isn't listening yet. During that gap, the port is closed, and visitors get
"connection refused".

### The fix: systemd socket activation

With **socket activation**, systemd holds the port instead of the app. While the app restarts,
systemd keeps new connections waiting in a line. So a deploy only makes visitors wait a moment;
nobody is turned away.

`rnx make:deploy` writes the file for this, `deploy/<app>.socket`. To turn it on:

```bash
sudo cp deploy/shop.socket /etc/systemd/system/ && sudo systemctl daemon-reload
sudo systemctl stop shop                  # it holds the port; the socket takes it over
sudo systemctl enable --now shop.socket
sudo systemctl start shop
sudo systemctl restart shop               # each deploy: migrate, then take over the socket
```

Step by step: copy the socket file where systemd looks and reload it; stop the app; switch on
the socket (now and at every boot); start the app again. The last line is what you run at each
deploy.

The app then uses the socket systemd passes to it (`LISTEN_FDS`) instead of opening
`APP_HOST:APP_PORT` itself. Keep the socket's `ListenStream` at that same address.

> [!WARNING]
> Stop the service before you enable the socket the first time. While the app holds the port,
> systemd can't listen on it, and you get "Failed to listen".

Does it help? Measured on a laptop with Renox's hello example, sending requests one after
another through three restarts: without the socket, 280 of 1,288 requests were refused; with
it, all 1,350 got an answer.

### Migrations both versions accept

During a deploy, the old code runs against the new migrations for a moment (and for longer
with several servers). So write migrations that both the old and the new version accept:

- add columns as nullable or with a default. Fill them in (backfill) later, in a job or a
  command;
- rename or drop a column over two deploys: first stop reading it, then drop it;
- add an index with `CREATE INDEX CONCURRENTLY` on PostgreSQL (see above).

### When a restart must not pause at all

Sometimes even a short wait is too much (a slow start, a cache that needs to warm up). Then run
two copies of the app on two ports behind Caddy, and restart them one at a time.
`deploy/README.md` has the Caddyfile for it (`lb_policy first` with `health_uri /health`).

The two copies can share a SQLite file on one machine. Set `CACHE_STORE=database` (and
`SESSION_DRIVER=database` for large sessions), so both see the same rate limits, locks and
sessions.

## Sessions

A **session** is what the app remembers about a visitor between pages: that they're logged in,
flashed messages, old form input and the CSRF token (which protects forms).

### The default: in the cookie

By default, the whole session is encrypted into its cookie. Good: there's nothing to store on
the server. But:

- browsers drop cookies over about 4 KB;
- a copied cookie stays valid until it expires or the user logs out (Renox then refuses it).

### `SESSION_DRIVER=database`

`SESSION_DRIVER=database` keeps sessions in the `sessions` table, and only an id in the cookie:

- there's no size limit (a large cart, long old input);
- each login and logout gives the session a new id and deletes the old row, so a copied cookie
  stops working at once;
- the table has `user_id`, so you can, say, show users their sessions or end them from an
  admin page;
- rows are keyed by the id's SHA-256 (a fingerprint of it), so someone with a copy of the table
  alone can't take over a session.

Each request reads its row. It writes the row only when the session changed (or once a minute,
to keep it from expiring).

Switching from `cookie` to `database` logs nobody out: a cookie written by the cookie driver is
read and moved into the table. Switching back, from `database` to `cookie`, ends every session:
the cookie then holds only an id, which the cookie driver can't use, so everyone has to log in
again.

Expired rows are deleted now and then by requests, and by `my-app session:prune`
(`Session::prune_expired(&db)`).

In tests (`APP_ENV=testing`), database sessions are kept in memory instead of the `sessions`
table, so `TestApp`'s session helpers keep working.

## Scheduled tasks and housekeeping

The scheduler runs tasks at set times, like a timer. It runs inside `serve` (or alone with
`schedule:work`).

Its times follow `APP_TIMEZONE`. That can be `UTC`, a fixed offset like `+07:00`, or an IANA
name like `Asia/Jakarta` (with daylight saving handled). A task can also set its own
`timezone`.

```bash
my-app schedule:list          # each task's next run, in its time zone
my-app schedule:run backup    # run one task now, e.g. to check it after a deploy
```

### Tables that keep growing

Some tables grow until something cleans them up (prunes them). You can run the commands by
hand, or call the functions behind them from a scheduled task. (A task runs Rust code, not
commands, so it calls the function.)

| Table | Grows with | Pruned by |
|---|---|---|
| `cache` (database store) | expired entries | itself, at most once an hour per process; `cache:prune` (`state.cache.prune()`) when you ask |
| `personal_access_tokens` | expired API tokens | `tokens:prune` (Auth module): tokens that expired more than a day ago (`renox::auth::prune_expired_tokens(&state.db, grace)`) |
| `device_tokens` | expired device tokens (kiosks, tills) | `tokens:prune` too (`renox::auth::DeviceToken::prune_expired(&state.db, grace)`) |
| `one_time_codes` | expired login codes and magic links (`renox::auth::OneTimeCode`) | issuing a code deletes the expired ones; `tokens:prune` too (`renox::auth::prune_expired_codes(&state.db)`) |
| `audit_logs` | every audited action (Audit module) | `audit:prune --days 365` (`renox::audit::prune(&state.db, age)`) |
| `role_user` | role assignments with an end date (`assign_role_in(…).until(…)`, Permissions module) | `permissions:prune --days 30` (`renox::auth::permissions::prune_ended_assignments(&state.db, age)`); ended ones already don't count |
| `revoked_sessions` | logouts | itself, on each logout |
| `sessions` (`SESSION_DRIVER=database`) | visits | itself, now and then; `session:prune` (`Session::prune_expired(&state.db)`) when you ask |
| `failed_jobs` | jobs that failed for good | `queue:prune-failed --hours 168` (`state.queue.prune_failed(age)`), `queue:flush` (see Failed jobs) |
| `job_batches` | every dispatched batch | `queue:prune-batches --hours 24` (`state.queue.prune_batches(age)`, finished batches) |
| `webhook_calls` | every received webhook | nothing yet: delete old `processed` rows yourself if it matters |
| `notifications` | every database notification | `notifications:prune --days 30` (Auth module): notifications read more than that long ago; unread ones stay (`renox::auth::prune_read_notifications(&state.db, age)`). A user's rows are also deleted with the user (`ON DELETE CASCADE`) |
| `import_batches`, `import_changes` | one batch per undoable import (`Import::undoable`) and a row per record it inserted or updated, with the row's before-image | `import:prune` (`renox::import::prune(&state, age)`): batches whose undo window ended. Until then they are what lets `renox::import::undo` put the import back |
| `grid_preferences` | one small row per user and data grid | `User::delete_account` (the account page's "delete account") deletes the user's rows. The table has no foreign key to `users` (every app has it, not every app has `users`), so an app that deletes users another way must delete these rows too |
| `subscriptions`, `billing_customers` (`renox-billing`) | every subscription an owner takes (kept as history) | nothing: they're small. The account page's "delete account" cancels the user's running subscriptions at the gateway and deletes their rows (a listener on `AccountDeleted`); an app that deletes users (or teams) another way must do the same |

Each function returns how many rows it deleted. Where the command takes `--hours` or `--days`,
the function takes a `std::time::Duration`.

For example, a daily task at 3 in the morning that deletes failed jobs older than a week:
`s.daily_at("03:00", "prune-failed", |state| async move { state.queue.prune_failed(Duration::from_secs(7 * 86_400)).await.map(drop) })`.

## Maintenance mode

Maintenance mode shows visitors a "back soon" page while you work on the app.

```bash
my-app down --secret let-me-in --retry 60   # 503 page with Retry-After; /let-me-in lets you through
my-app up
```

- `down` turns it on. Visitors get a 503 page. `--retry 60` tells browsers and bots to try again
  in 60 seconds (the `Retry-After` header). `--secret let-me-in` means that opening
  `/let-me-in` lets *you* through, so you can check the site.
- `up` turns it off.

The switch is a small file in `STORAGE_PATH`, so every process that shares that directory sees
it. `/health` and every webhook route keep working, so payment providers' calls are still saved.

> [!NOTE]
> Maintenance mode only affects web requests. Queue workers and scheduled tasks keep running,
> and keep writing to the database. To pause them too, stop the app, or start it with
> `QUEUE_WORKERS=0 SCHEDULER=false` (and stop any separate `queue:work` or `schedule:work`).

## All settings

Every setting is read from the environment or from `.env`, when the app starts. The full list,
each with a comment and its default, is the `.env.example` that `rnx new` writes (its source is
[`crates/renox-cli/stubs/env.stub`](../crates/renox-cli/stubs/env.stub)). This guide covers the
ones for running in production. The others, in short:

| Setting | What it is |
|---|---|
| `APP_FALLBACK_LOCALE` | the language used for texts missing in the visitor's language (`en`) |
| `SESSION_COOKIE` | the session cookie's name (`renox_session`); give each app its own when several share a domain |
| `VIEWS_PATH`, `LANG_PATH`, `PUBLIC_PATH` | where views, translations and public files are (`resources/views`, `resources/lang`, `public`), relative to the directory the app runs from |
| `GOOGLE_SITE_VERIFICATION`, `GA4_MEASUREMENT_ID`, `GTM_CONTAINER_ID` | search engine verification and analytics tags, added to pages with `APP_ENV=production` only |
| `GA4_API_SECRET` | for analytics events sent from the server (`renox::analytics::ServerEvent`) |

Your app's own settings need no code in Renox: `state.config.var("NAME")` reads any of them.

## Logs

Logs are the app's diary: what it did, and what went wrong.

**Where they go.** Logs go to stdout (the terminal's normal output).

**How much.** Set the level with `RUST_LOG` (for example `RUST_LOG=info,sqlx=warn`). Without
it:

- `serve`, `queue:work` and `schedule:work` log at `info` (`info,renox=debug` with
  `APP_DEBUG`);
- every other command logs only warnings.

**JSON logs.** `LOG_FORMAT=json` writes one JSON object per line. Log tools like Loki, Datadog,
CloudWatch or `jq` read that easily. The request's fields are in `span`:

  ```json
  {"timestamp":"…","level":"ERROR","fields":{"message":"request failed","error":"…"},
   "target":"renox_core::error","span":{"id":"k3J9x2mQpL0aB7cDe4Fg","ip":"203.0.113.9","method":"POST","uri":"/checkout","name":"request"}}
  ```

This line says: a `POST` to `/checkout` from `203.0.113.9` failed, and its request id is
`k3J9x2mQpL0aB7cDe4Fg`.

**Log to a file.** `LOG_FILE=storage/logs/app.log` appends to that file instead (without
colors). If the file can't be opened (a missing directory, no permission), the app says so on
stderr and logs to stdout. Rotate the file with logrotate's `copytruncate`, or leave logs on
stdout for journald or Docker to collect.

**Request ids.** Each request gets an id:

- the `X-Request-Id` a proxy sent, kept when it's 8 to 64 characters of
  `A-Z a-z 0-9 . _ -`;
- or a new one.

The id is in every log line of the request, in the response's `X-Request-Id` header, in error
reports, and in handlers as the `RequestId` extractor. A visitor who tells you the id points you
at the exact log lines.

**Errors and jobs.**

- Errors are logged with their cause. With `APP_DEBUG=false`, visitors see only the error page.
- Job attempts log `job done`, `job failed, will retry` and `job failed for good`.

## Error reports

Logs only help if someone reads them. `App::report` sends you every error a person should look
at. It runs in the background, after the response has been sent, for:

- a request that answered 500 (with its method, path, request id, IP and user id);
- a job that failed for good (`source` is its name and id, like `send-invoice #42`);
- a scheduled task that failed or panicked.

```rust
# use renox::prelude::*;
use renox::report::ErrorReport;

# let _ =
App::new().report(|report: ErrorReport, state: AppState| async move {
    // Sentry, Honeybadger, a Slack webhook… The report serializes to JSON.
    // Here: if the app has an ERROR_WEBHOOK setting, post the report there as JSON.
    if let Some(url) = state.config.var("ERROR_WEBHOOK") {
        // The `let _ =` ignores a failed send: a broken reporter must not cause new errors.
        let _ = state.http.post(url).json(&report).send().await;
    }
})
# ;
```

This reporter reads an `ERROR_WEBHOOK` address from the app's settings, and posts each report
there as JSON.

You can add several reporters. One that fails or panics doesn't stop the others. 4xx errors
(a 404, a failed validation) aren't reported: those are the visitor's mistakes, not the app's.

## Error pages

When something goes wrong, visitors see an error page. `rnx new` writes
`resources/views/errors/default.html` for you. It extends the app's layout, so an error page
keeps the navigation bar and the signed-in user's menu.

The template gets:

- the same globals as any page (`auth`, `request`, `t()`, `route()`…);
- `status`, `reason` and `detail`. `detail` is the message of an `abort(…)`; for a 500, it's
  the cause, but only with `APP_DEBUG`;
- with `APP_DEBUG` only: `debug`, `request_line` (the request that failed) and `template`
  (where a template failed), for a developer box on the page.

Want a special page for one status? Write `errors/404.html` (or any status); it wins for that
status.

If the app's error page itself fails to render, Renox shows its own. So a mistake in the layout
can't hide the original error. Clients that ask for JSON get JSON.

## Debug inspector

While you develop, Renox can show you what each request did. With `APP_DEBUG=true` and
`APP_ENV=local`, `/_renox/debug` lists the last 50 requests (newest first), with:

- their status and time;
- the view they rendered;
- their SQL.

A statement run three or more times in one request is flagged as a likely **N+1** (a query run
once per row in a loop, instead of once for all rows).

The page is never there in production or in tests.

Mail sent while you develop is at `/_renox/mail`.
