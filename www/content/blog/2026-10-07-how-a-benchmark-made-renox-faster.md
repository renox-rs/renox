---
title: How a benchmark made Renox 3.3× faster
description: Measuring Renox against bare Axum showed every request cloning a 35-field state tens of times. Making it one Arc tripled throughput.
date: 2026-10-07
author: Arif Rachim
tags: performance, rust, benchmarks
---
Before we put a single number on this site, we wanted to measure Renox properly against two things. The first is the floor it is built on: bare Axum, with the same crates and no middleware at all. The second is the framework it is modelled on: Laravel, both under PHP-FPM and under Octane. The [benchmark lives in the repository](https://github.com/renox-rs/renox/tree/main/benchmarks), so anyone can run it.

The first run surprised us.

## The first numbers

On two pinned CPU cores, 64 connections, a route that returns `Hello, World!`:

| | Requests per second | p50 latency |
|---|---:|---:|
| Axum (bare) | 105,216 | 0.61 ms |
| Renox | 9,265 | 7.16 ms |
| Laravel Octane | 1,403 | 45.22 ms |
| Laravel (PHP-FPM) | 722 | 88.96 ms |

Renox was well ahead of Laravel, as you would expect. But it was an eleventh of the speed of bare Axum. Renox does more on every request than bare Axum: security headers with a CSP nonce, a request id, an encrypted session cookie, CSRF, the user, the locale, the view layer, error pages and a timeout. Even so, that work shouldn't cost 90% of the throughput.

## Queueing, or work?

The first question was whether requests were waiting or working. With a single connection, Renox answered in 0.14 ms and bare Axum in 0.034 ms. So each request really did about four times as much work, and with 64 connections the two cores were simply full.

## Skipping the layers one by one

We added a temporary switch that skips a chosen middleware layer, then measured with each layer skipped in turn:

```text
none         9,049 rps
guard        9,324
maintenance 11,032
view        11,165
session     10,981
auth        11,006
…
all of them 65,181
```

No single layer was slow. Each one gave back about 10 to 20 percent, and all of them together gave back seven times the throughput. Whatever the cost was, every layer paid it.

## What every layer does

Each layer clones the application state. Axum's `from_fn_with_state` clones it for every call, it is put into the request's extensions, and each handler's `State` extractor clones it again. `AppState` was a plain struct of about 35 fields: the configuration, the database pool, the mailer, the queue, the cache, the route table, the translator and more. Most of them were `Arc`s, and one was a `Vec`.

A small program timed it:

```rust
for _ in 0..1_000_000 {
    std::hint::black_box(state.clone());
}
```

One clone took **382 ns** on one thread. With two threads cloning at the same time, it took **about 3 µs**. Every `Arc` clone is an atomic increment. When two cores increment the same counters, the cache lines holding them bounce between the cores, and 35 counters bounce together. A request did tens of clones, and as many drops.

## The fix

`AppState` is now one `Arc` around its fields, and `Deref` makes reading them look exactly the same:

```rust
#[derive(Clone)]
pub struct AppState(Arc<AppStateInner>);

impl std::ops::Deref for AppState {
    type Target = AppStateInner;
    fn deref(&self) -> &AppStateInner { &self.0 }
}
```

`state.db`, `state.config` and `state.queue` all work as before. The only code that changes is code that moved a field out of a state, like `let db = state.db;` in a seeder. That now reads `let db = state.db.clone();`, and the compiler tells you so.

## After

| | Before | After |
|---|---:|---:|
| `AppState::clone`, one thread | 382 ns | 12 ns |
| `AppState::clone`, two threads | ~1.6 µs | 79 ns |
| `/plaintext` | 9,240 rps | **30,535 rps** |
| `/page`: 20 rows from SQLite as HTML | 6,310 rps | **9,935 rps** |

That is **3.3× the requests** on a bare route. On a real page, with a query and a template, it is 1.6×, because there the database and the template do a larger share of the work.

## What's left

Bare Axum is still about 3.5× faster on a route that does nothing. The remaining cost is spread over the layers again. The session and the maintenance check, which reads a file on every request, cost about 10 percent each. Those are the next things to look at.

We'll keep the benchmark in the repository and rerun it as Renox changes.
