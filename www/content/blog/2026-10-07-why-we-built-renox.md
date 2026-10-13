---
title: Why we built Renox
description: Rust gives web apps speed and a compiler that catches bugs early, but you assemble twenty crates. Renox makes it one, the way Laravel does.
date: 2026-10-07
author: Arif Rachim
tags: rust, laravel, design
---
Rust is a great language for web applications. It is fast, it uses little memory, and its compiler catches whole classes of bugs before a user ever sees them. Yet most teams that start a web app pick something else. The reason isn't the language. It's everything around it.

## Twenty crates and a week of glue

A web app needs much more than a router. It needs sessions, CSRF protection, a template engine, a database layer, migrations, validation with messages a person can read, login and password resets, roles and permissions, background jobs, a scheduler, mail, file uploads, a cache, error pages, and a way to deploy it all.

In Rust each of those is a crate, usually a good one. But you choose them, you read how each one wants to be wired, you write the glue, and you keep them in step as they release. When something doesn't fit, say a session store that wants a different async runtime, or a job queue that needs Redis when you wanted SQLite, the glue grows. Every team builds its own framework, and every one of them is a little different.

Laravel solved this problem for PHP more than a decade ago. You install one thing and everything is there, it fits together, and the names are the same in every project. That is why people are productive in it on the first day.

## One dependency

Renox is that idea for Rust. You add one crate:

```toml
[dependencies]
renox = "1"
```

and import one prelude:

```rust
use renox::prelude::*;
```

Routing, sessions, CSRF, views, models and migrations, validation, auth with roles and permissions, a queue, a scheduler, mail, notifications, cache, storage, translations, webhooks, SEO tags and a UI kit are all there, tested together. You don't copy framework code into your app: your app depends on Renox, so an upgrade is a version bump.

If you know Laravel, you already know the shape of it. There are routes with names, form requests (`Valid<T>`), models with relations, jobs, mailables, policies and `artisan`-style commands. The difference is that the compiler now checks your work. The [Laravel → Renox guide](https://docs.renox.rs/docs/laravel) goes through it concept by concept.

## Choices we made

**One runtime crate.** Everything lives in `renox-core`, re-exported by `renox`. Splitting it into an HTTP crate, a database crate and a view crate sounds tidy, but they all need the same application state, so they would all depend on each other.

**Our own migrator and queue.** We wanted Laravel-style migration batches, migrations that modules own, and a queue that runs on the database you already have, SQLite or PostgreSQL, with retries, chains, batches and unique jobs. No Redis is required.

**Sessions in a cookie by default.** The session is encrypted and signed, so a small app needs no session table. A database driver is one setting away when you outgrow it.

**HTMX and Alpine.js, not a JavaScript build.** Pages are rendered on the server with MiniJinja. HTMX swaps fragments, and a validation error comes back as a 422 that Renox's own script places next to the field. There is no `node_modules` and no bundler.

**One binary.** Views, translations, migrations and public files are compiled in. The binary is also its own CLI: `migrate`, `queue:work`, `schedule:work`, `db:seed`. You copy one file to a server, and with systemd's socket activation a restart doesn't refuse a single connection.

## Who it is for

Renox is for people who want to build a product, not a framework: a SaaS, a back office, a shop, an internal tool. You get Rust's speed and its compiler, without first spending a week deciding how sessions should work.

It is open source, MIT OR Apache-2.0, and close to 1.0. The best way to see what it can do is the [tutorial](https://docs.renox.rs/docs/tutorial). It builds one app from `rnx new` to deploy. Or open the [bike shop](https://bikeshop.renox.rs), a whole business built with Renox, where every page explains the features it uses.
