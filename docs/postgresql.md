# PostgreSQL

Every Renox app keeps its data in a database. This guide shows how to use **PostgreSQL** instead
of the default, **SQLite**, and what changes when you do.

**In this guide**

- [Starting a new app on PostgreSQL](#starting-a-new-app-on-postgresql)
- [Switching an existing app](#switching-an-existing-app)
- [Migrations](#migrations) that work on both databases
- [Writing SQL that runs on both](#writing-sql-that-runs-on-both)
- [Things that behave differently (on purpose)](#things-that-behave-differently-on-purpose)
- [Tests](#tests) on PostgreSQL
- [Moving the data](#moving-the-data) from SQLite to PostgreSQL
- [Deploying](#deploying)

### Words you'll meet

| Word | What it means |
|---|---|
| **database** | The place where the app keeps its data (users, orders…), in tables of rows. |
| **SQLite** | A database that is just one file next to your app. Nothing else to install or run. |
| **PostgreSQL** | A database that runs as its own program (a *database server*), often on another machine. Many apps can talk to it at once. |
| **server** | A computer (or a program on it) that waits for others to ask it things and answers them. |
| **migration** | A small SQL file that creates or changes a table. |
| **connection pool** | A few open connections to the database that the app keeps ready and shares between requests, so it doesn't have to connect again every time. |
| **transaction** | A group of SQL statements that either all happen or none do. |
| **schema** | In PostgreSQL, a named folder of tables inside one database. |
| **managed database** | A database server that a hosting company runs and backs up for you. |

### When to switch

SQLite is the default, and it's the right choice for an app that runs on one server:

- there is no database server to install and look after;
- a backup is just a copy of one file (or Litestream, a tool that copies it as it changes).

Switch to PostgreSQL when the app outgrows that:

- you want to run the app on several servers at once,
- many people write data at the same moment (lots of concurrent writes),
- or you want a managed database.

Your code stays the same on both. Models, the query builder, `renox::db::sql()`, auth, the
queue, the cache, the scheduler, `db:shell` and `migrate:*` all work on both databases.

[examples/bikeshop](../examples/bikeshop) shows this: one app on both, where only `DATABASE_URL` and the
migrations' `.postgres.up.sql` twins differ ([migrations/](../examples/bikeshop/migrations)). CI runs its
tests on PostgreSQL too. Its `/about/fields` page shows, for every field type, which column
type it uses on each database ([src/app/about/fields.rs](../examples/bikeshop/src/app/about/fields.rs)).

## Starting a new app on PostgreSQL

```bash
rnx new shop --database postgres
createdb shop && createdb shop_test     # or edit the URLs in .env
cd shop
rnx serve
```

What these commands do:

- `rnx new shop --database postgres` makes a new app called `shop`, set up for PostgreSQL.
- `createdb shop && createdb shop_test` makes two empty databases on your PostgreSQL server:
  one for the app, one for its tests. (If yours have other names or live elsewhere, change the
  URLs in `.env` instead.)
- `cd shop` and `rnx serve` go into the app's folder and start it.

The `--database postgres` option does three things for you:

- it turns on renox's `postgres` **feature** in `Cargo.toml` (a feature is an optional part of a
  crate that you switch on);
- it sets `DATABASE_URL` in `.env`: the address of the app's database;
- it sets `TEST_DATABASE_URL` in `.env`: the database the tests use (see [Tests](#tests)).

After that, `rnx make:migration` and `rnx make:model -m` write PostgreSQL SQL for you. They
read `DATABASE_URL` to know which database you use. So do `rnx make:migration --auto` (and
`db:diff`), which write PostgreSQL SQL from the models, and `rnx db:check`, which compares the
models with the tables the migrations build on PostgreSQL.

## Switching an existing app

Already have an app on SQLite? Four steps move it to PostgreSQL.

1. Turn on the feature in `Cargo.toml`:

   ```toml
   renox = { git = "https://github.com/renox-rs/renox", rev = "…", features = ["postgres"] }  # keep your rev
   ```

   Keep the `rev` (or version) you already have; only add `features = ["postgres"]`.

2. Point `DATABASE_URL` at the server:
   `DATABASE_URL=postgres://user:password@host:5432/shop`.

   The start of the URL (its *scheme*) decides which database Renox opens:

   - `postgres://` and `postgresql://` open a PostgreSQL pool;
   - `sqlite:` opens a SQLite one;
   - any other URL is refused when the app starts. So a typo like `postgress://` stops the app
     with an error, instead of quietly making a SQLite file.

   `DATABASE_POOL_SIZE` (default 8) sets how many connections the pool keeps, on both databases.

3. Make your migrations run on PostgreSQL. See [Migrations](#migrations) below.

   You only need to do this for your own tables. Every one of Renox's own tables already has a
   PostgreSQL version: users, tokens, notifications, roles and permissions, audit logs, jobs,
   cache, sessions, grid preferences and webhook calls.

4. Run `my-app migrate` to create the tables. If the app already has data, then
   [copy the data](#moving-the-data).

## Migrations

A migration file runs on every database, unless there is a version written for one database.
`renox::migrations!()` reads the `migrations` folder. To read another folder, give its path
from the crate's root: `renox::migrations!("db/migrations")`. When a migration runs, Renox picks
the right file by its name:

| File | Used on |
|---|---|
| `20260101000000_create_products.up.sql` | every database without its own version |
| `20260101000000_create_products.postgres.up.sql` | PostgreSQL |
| `20260101000000_create_products.sqlite.up.sql` | SQLite |
| `20260101000000_create_products.sql` | every database without its own version; it can't be undone (no `down`) |
| `20260101000000_create_products.postgres.sql`, `….sqlite.sql` | that database only; can't be undone either |

`.down.sql` files (the ones that undo a migration) work the same way, with one catch: a
database's own `down` is used only together with its own `up`.

The rules in full:

- A database-specific `up` without its own `down` uses the plain `.down.sql`. That is usually
  just a `DROP TABLE`, which is the same on both.
- A database-specific `down` without its own `up` (say, a `.postgres.down.sql` next to a plain
  `.up.sql`) doesn't compile: `migrations!` refuses it, since it would never run. When the undo
  differs on one database, give that database its own `up` as well.
- You may leave out the plain `up` when both databases have their own.
- A migration without any `down` can't be rolled back (see `migrate:rollback` in
  [operations.md](operations.md#deploys-and-migrations)).

### SQL that differs between the two

Most SQL is the same, but a few column types are written differently:

| SQLite | PostgreSQL |
|---|---|
| `id INTEGER PRIMARY KEY AUTOINCREMENT` | `id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY` |
| `id BLOB PRIMARY KEY` for an `id: Uuid` key | `id UUID PRIMARY KEY` |
| `id TEXT PRIMARY KEY` for `Ulid` and `String` keys | the same |
| `INTEGER` (always 64-bit) | `BIGINT` for `i64` fields; `INTEGER` is 32-bit and won't read into an `i64` |
| `TEXT` for `DateTime` fields | `TIMESTAMPTZ` (and `TIMESTAMP`, `DATE`, `TIME` for the naive chrono types) |
| `INTEGER` 0/1 for `bool` fields | `BOOLEAN` |
| `BLOB` | `BYTEA` |
| `REAL` | `DOUBLE PRECISION` |
| a `Money` field: `BIGINT` amount plus a `TEXT` currency | the same (the amount must be `BIGINT`, not `INTEGER`) |

These are the same on both: plain `TEXT`, `NOT NULL`, `UNIQUE`,
`REFERENCES … ON DELETE CASCADE`, `CREATE INDEX` and `DROP TABLE`.

> [!WARNING]
> On PostgreSQL, an `i64` field needs a `BIGINT` column. A plain `INTEGER` there holds only 32
> bits, and reading it into an `i64` fails.

### Transactions and migrations

Each migration runs inside a transaction: if one statement fails, none of it is kept.

A migration runs **without** that transaction when any of these is true:

- it has a line that is exactly `-- renox:no-transaction`;
- it starts its own transaction (a `BEGIN`, `BEGIN TRANSACTION` or `BEGIN IMMEDIATE`
  statement), so it decides itself what to commit;
- it contains the word `CONCURRENTLY` (as in `CREATE INDEX CONCURRENTLY`, which builds an index without
  blocking the table and can't run in a transaction).

How such a migration then runs:

- On PostgreSQL, a migration with the word `CONCURRENTLY` is split and sent statement by statement.
- Any other one is sent as one script. On PostgreSQL, a script of several statements still
  runs as one implicit transaction (unless it has its own `BEGIN` … `COMMIT`). On SQLite, each
  statement is kept as soon as it runs.

> [!IMPORTANT]
> Keep such a migration to that one change. If it fails halfway, the statements that already
> ran stay done.

### Several servers, one deploy

Several servers can run `migrate` during the same deploy. An *advisory lock* (a "wait your
turn" flag in PostgreSQL) makes them take turns. The later ones find nothing left to do.

`migrate:fresh` drops everything in the schema and starts again. That includes enum types,
sequences and functions, but not what extensions created.

## Writing SQL that runs on both

When you write your own SQL with `renox::db::sql()`, these rules make it work on both databases.

**Placeholders.** Always write `?` where a value goes. On PostgreSQL, Renox turns them into
`$1`, `$2`, … before the query is sent. A `?` inside a string, a quoted name or a comment is
left alone.

> [!WARNING]
> Because `?` always means "a value goes here", you can't write PostgreSQL's JSON operators
> `?`, `?|` and `?&` through `renox::db::sql()`. For those, use `db.postgres()` and sqlx
> directly.

**Values keep their types.** On PostgreSQL, each value is sent with its own type:

- `bool` as `BOOLEAN`,
- `DateTime` as `TIMESTAMPTZ`,
- `NaiveDate` as `DATE`,
- and so on.

`None` is sent as an untyped `NULL`, so it fits any column.

**Results must match the Rust type exactly.** PostgreSQL is strict about this:

- `SUM` of a `BIGINT` is a `NUMERIC`, so write `CAST(SUM(price) AS BIGINT)` to read an `i64`;
- `SELECT 1` is a 32-bit integer.

`COUNT(*)` is a `BIGINT` on both.

**Searching text.** `where_like` and `where_op(col, "like", …)` ignore ASCII upper and lower
case on both databases. On PostgreSQL, Renox sends `ILIKE` to do that.

**Time.** Timestamps from `renox::db::now()` are cut to the microsecond, as PostgreSQL stores
them. So a model you saved equals the row you read back.

**Database-only code.** For anything that only one database has, check `db.dialect()`, or reach
sqlx directly through `db.sqlite()` / `db.postgres()`.

## Things that behave differently (on purpose)

**Emails.** Emails are stored trimmed (no spaces around them) and in lowercase, both at
registration and in `User::register`. They are looked up the same way. So logins and password
resets ignore upper and lower case on both databases. On PostgreSQL, the `users` table also has
a unique index on `lower(email)`, so two accounts can't differ only by case.

**Queue workers.** A worker is the part of the app that runs background jobs. On PostgreSQL,
workers reserve a job with `FOR UPDATE SKIP LOCKED`. That means workers on several servers never
take the same job, and they don't wait for each other either.

**Scheduled tasks.** Before a scheduled task runs, it claims that run in the `cache` table. So
when `serve` runs on several servers, each task runs once, not once per server. You no longer
need `SCHEDULER=false` on the extra servers for that.

**Row locks.** `Query::lock_for_update()` and `shared_lock()` lock the rows you read, so no one
else changes them until your transaction ends. They add `FOR UPDATE` / `FOR SHARE` on PostgreSQL
and do nothing on SQLite. SQLite locks the whole database for a write instead.

Only calls that read rows take the lock: `get`, `first`, `pluck`, `select_as` and the items of
a `paginate`. `count`, `exists`, `sum` and the other aggregates, `update`, `increment` and
`delete` ignore it. So to lock a row and then change it, read it with
`.lock_for_update().first(&mut tx)` first.

> [!TIP]
> On SQLite, start the transaction with `db.begin_immediate()`. It then takes the write lock
> before it reads, which gives you the same safety.

**A failed statement ends the transaction** on PostgreSQL: every later statement fails until
the transaction rolls back. SQLite carries on.

So when a step may fail on purpose (for example, an insert that may hit a unique index), run it
inside `tx.savepoint(|tx| …)`. If it returns `Err`, only the savepoint rolls back and the
transaction goes on. This works the same on both databases. `renox::import` does this for
each row of a file (the bike shop's price lists:
[src/app/stock/import.rs](../examples/bikeshop/src/app/stock/import.rs)).

## Tests

Tests should never touch your real data. With PostgreSQL, Renox gives each test its own empty
**schema**:

- `renox::testing::TestApp` (and every app started with an in-memory SQLite URL) checks
  `TEST_DATABASE_URL`;
- if that is a PostgreSQL URL, it makes a new, empty schema there and uses it.

`TEST_DATABASE_URL` is read from the environment or from `.env`. Tests stay apart from each
other and from your development data.

```bash
TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/shop_test cargo test
```

This runs the tests against the `shop_test` database on your own machine.

The schemas (named `renox_test_…`) are left behind, so you can look inside them after a test.
To get rid of them all, drop the test database and create it again.

Need a PostgreSQL server for tests on your machine? With Docker, this starts one:

```bash
docker run -d --rm --name pg -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=shop_test \
  -p 5432:5432 postgres:17-alpine
```

It runs PostgreSQL 17 in the background on port 5432, with the password `postgres` and an
empty `shop_test` database. `--rm` removes it when you stop it.

## Moving the data

Migrations create the tables, but they don't copy your rows. You copy the data separately,
once, while nothing writes to the old database.

1. Stop the app on the old server, then copy `storage/app.db` (the SQLite file).

   > [!WARNING]
   > `my-app down` (maintenance mode) is not enough on its own. It only stops visitors' pages:
   > queue workers and scheduled tasks keep running and writing, and webhook calls still
   > arrive. If the app must stay up, run it with `QUEUE_WORKERS=0 SCHEDULER=false` as well,
   > and copy the file with `sqlite3 storage/app.db ".backup app-copy.db"` instead of `cp`.
2. Create the tables on PostgreSQL:
   `DATABASE_URL=postgres://… my-app migrate`.
3. Copy the rows. [pgloader](https://pgloader.io) does this in one step. Its **data only**
   option keeps the tables Renox created and only fills them:

   ```
   LOAD DATABASE
     FROM sqlite:///path/to/app.db
     INTO postgresql://user:password@host/shop
   WITH data only, reset sequences, truncate
   CAST column job_batches.allow_failures to boolean using tinyint-to-boolean
   EXCLUDING TABLE NAMES LIKE 'renox_migrations';
   ```

   What to know about this step:

   - Timestamps written by Renox are text like `2026-10-04 12:00:00.123456+00:00` (date, time
     with up to six decimals, and the offset). PostgreSQL reads them into `TIMESTAMPTZ` as they
     are.
   - Booleans stored as 0/1 need a `CAST` rule, like the one above. Renox's own
     `job_batches.allow_failures` is `INTEGER` on SQLite and `BOOLEAN` on PostgreSQL. Add a
     rule like it for each boolean column of your own tables.
4. If some stored emails have capital letters, make them lowercase:
   `UPDATE users SET email = lower(email);`.
5. Point `DATABASE_URL` at PostgreSQL, deploy, then run `my-app up` to leave maintenance mode.

## Deploying

A database on another machine can be slow, or unreachable. Three **timeouts** (time limits)
stop that from holding up your app's requests forever:

| Setting | What it limits | Default |
|---|---|---|
| `DATABASE_ACQUIRE_TIMEOUT` | how long a query waits for a free connection | 5 s |
| `DATABASE_STATEMENT_TIMEOUT` | how long one statement may run | 30 s |
| `REQUEST_TIMEOUT` | how long a request may take before it gets an answer | 60 s |

`DATABASE_STATEMENT_TIMEOUT=0` means no limit, for the whole app. To lift the limit for one
long report job only, run `SET LOCAL statement_timeout = 0` inside that job's transaction.
(A plain `SET` would stay on the pooled connection after the transaction, so later queries
would keep the changed limit.)

The statement limit is set on every connection, so it also covers commands: `migrate`,
`db:seed`, `db:shell` and `queue:work`. A migration that runs longer than 30 s (a big
`CREATE INDEX`, a backfill) is cancelled. Raise the limit for that run only:
`DATABASE_STATEMENT_TIMEOUT=0 my-app migrate`.

`rnx make:deploy` writes the same files as for SQLite: a Dockerfile, a systemd unit and a
socket unit. To use PostgreSQL:

- set `DATABASE_URL` to the PostgreSQL server in the server's environment. The Dockerfile's
  SQLite address is only a fallback;
- skip the Litestream part of `deploy/README.md`, which is for SQLite only;
- back up PostgreSQL with your provider's backups or with `pg_dump`.

[operations.md](operations.md) has more on running an app in production.
