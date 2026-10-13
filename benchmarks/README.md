# Benchmarks

What a Renox app costs and gives, measured, next to the floor it is built on (bare Axum) and
the framework it is modelled on (Laravel). The numbers on renox.rs come from here. Run it
yourself; send a pull request if something here is unfair.

```
benchmarks/run.sh                       # everything, about 8 minutes
DURATION=30s CONNECTIONS=128 benchmarks/run.sh
ONLY="renox axum" benchmarks/run.sh
```

Needs Docker, [oha](https://github.com/hatoo/oha) (`cargo install oha`), Python 3 and cargo.
Results land in `results/<date>.json`; `summarize.py <file>` prints them as the tables in
[RESULTS.md](RESULTS.md).

## What is compared

Four servers, each with the same four endpoints over the same SQLite database (`seed.py`:
10,000 rows in `items`):

| Endpoint | What it does |
|---|---|
| `/plaintext` | `Hello, World!`: the framework's own cost per request |
| `/json` | a small JSON body |
| `/db?id=N` | one row by its key, as JSON |
| `/page?id=N` | 20 rows through a template: a page as an app serves it |

| Server | Built as |
|---|---|
| **Renox** ([renox-app/](renox-app)) | a release build, production settings (`APP_ENV=production`, `APP_DEBUG=false`, no workers, no scheduler, `RUST_LOG=warn`), with everything Renox puts in front of a route by default: security headers and a CSP nonce, the request id, sessions (an encrypted cookie), CSRF, the user, the locale, the view layer, error pages, the request timeout |
| **Axum (bare)** ([axum-app/](axum-app)) | the crates Renox is built on (axum, sqlx, MiniJinja) wired by hand, with no middleware at all: the floor. The gap to Renox is what Renox's defaults cost |
| **Laravel (PHP-FPM + nginx)** ([laravel/](laravel)) | a fresh `laravel/laravel` 12 with the same endpoints (a controller, the query builder, Blade) in the `web` group (cookies, the session as a cookie, CSRF), `APP_ENV=production`, `APP_DEBUG=false`, OPcache on, config, routes, events and views cached (`artisan optimize`), the nginx access log off; the usual deploy |
| **Laravel Octane (FrankenPHP)** | the same app on Laravel Octane, Laravel's fastest server |

## How it is measured

- One server at a time, in Docker, on the host's network (no port proxy in the way), pinned to
  two CPU cores (`--cpuset-cpus 0,1`) with 1 GB of memory and no swap. The Rust apps run in an
  `ubuntu:24.04` container; Laravel in `serversideup/php` images.
- oha runs on other cores (`taskset -c 4-7`): 64 connections, a 5 s warm-up, then 15 s per
  endpoint. Requests per second, p50 and p99 latency come from oha; requests still in flight
  when the time is up are not counted as errors.
- **Cold start**: from `docker run` to the first `200` from `/plaintext` (so it includes the
  container's own start, the same for all).
- **Memory**: the container's, from `docker stats`, at rest and the peak while loaded.
- **What you deploy**: the binary for the Rust apps; the app's directory with `vendor/` for
  Laravel, which also needs PHP and a web server on the machine.

## What it doesn't say

- One machine, two cores. Absolute numbers depend on the hardware; compare the rows, not the
  figures to another run.
- SQLite in the same container. A database over the network adds the same round trip to
  every server and narrows the gaps on `/db` and `/page`.
- Small, synthetic pages. A real page does more work in the app's own code, which Renox and
  Laravel don't speed up or slow down.
- Laravel could be tuned further (more FPM children, Octane workers, Swoole/RoadRunner); these
  are its documented production defaults.

## Finding a slow path

`renox-app/examples/clone_cost.rs` times `AppState::clone`, on one thread and two at once. It
found #334: every middleware layer cloned a 35-field state, about 3 µs a clone on two busy
cores. Run it with `cargo run --release -p bench-renox --example clone_cost`.
