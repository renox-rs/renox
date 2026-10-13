# Testing a Renox app

Tests are small programs that check your app for you. You run them with `cargo test`, and they
tell you at once if a change broke something, so you don't have to click through every page by
hand. This guide shows how to write them with `TestApp`.

### In this guide

- a first test: start the app, open a page, check the answer;
- sending requests and checking responses;
- changing the settings, and faking outside services;
- logging in and checking the session;
- checking the database, and filling it with fake rows;
- jobs, events, notifications, mail and commands;
- moving the clock forward;
- testing in a real browser.

### Words you'll meet

| Word | What it means |
|---|---|
| **test** | A function marked `#[renox::test]`. It passes if it runs to the end, and fails if something panics. |
| **assert** | A check. `assert_ok()` says "the page must have loaded fine"; if it didn't, the test fails with a message. |
| **request** and **response** | The browser asks for a page (a request); your app answers (a response). |
| **status** | A number in every response: 200 means OK, 303 a redirect, 404 not found, 422 invalid input. |
| **middleware** | Code that runs around every request: sessions, login, security checks. |
| **session** | What the app remembers about one visitor between requests, like who is logged in. |
| **CSRF token** | A secret code every form must send back, so other sites can't post forms as your users. |
| **migration** | A SQL file that creates or changes a table. |
| **fake** | A stand-in that records what your app does (like sending mail) instead of really doing it. |
| **htmx** | A small script that updates part of a page without a reload. Its requests get some different answers. |

### A first test

`TestApp` starts your whole app inside the test, with no real server and no browser. You then
talk to it the way a browser does.

Every request goes through the same router (the part that picks a handler for each address),
the same middleware, sessions and CSRF checks as in production. So a passing test means the real
app works the same way.

The database is a fresh one for each test: SQLite in memory, or PostgreSQL when
`TEST_DATABASE_URL` is set. The migrations run on it before the test starts.

```rust
use renox::prelude::*;
use renox::testing::TestApp;

/// The app under test: here just Renox's login pages (the `Auth` module), with its
/// account page (`.account()` adds `/account`).
fn app() -> App {
    App::new().module(Auth::new().account())
}

/// Guests are sent to the login page; a logged-in member sees their account.
#[renox::test]
async fn members_see_their_account() {
    // Start the app, with a fresh database.
    let app = TestApp::new(app()).await;
    // Make a user to log in with.
    let user = User::register(app.db(), "Ana", "ana@example.com", "password123").await.unwrap();
    // Not logged in yet: the account page must redirect to /login.
    app.get("/account").await.assert_redirect("/login");
    // Log in as Ana, then open the page again.
    app.acting_as(&user);
    app.get("/account").await.assert_ok().assert_see("ana@example.com");
}
```

What's going on:

- `#[renox::test]` marks the function as a test. Use it instead of `#[tokio::test]`.
- `TestApp::new(app())` boots the app.
- `app.get("/account")` opens a page, like typing the address in a browser. It returns the
  response, and the `assert_…` methods check it.
- `acting_as(&user)` logs the user in, without filling in the login form.

> [!TIP]
> You don't have to start from an empty file. `rnx make:test checkout` writes a test file to
> start from. `rnx make:module products --resource` writes the tests of a whole resource
> (create, list, show, edit, update, delete, invalid input).

> [!NOTE]
> **Coming from Laravel:** this is Laravel's feature tests: `$this->get(…)->assertOk()`
> becomes `app.get(…).await.assert_ok()`, and `actingAs` becomes `acting_as`.

## Requests and responses

### Sending requests

- **Requests:** `get`, `post(uri, &[(field, value)])`, `put`, `patch`, `delete`, `post_json`,
  `post_multipart`, `post_body`. A `post` sends a form: a list of field names and values.
- **Request options:**
  - `app.htmx()` sends the request the way htmx does.
  - `app.request().header(…)` adds headers (extra details sent with a request).
  - `.json()` asks for JSON (it sends `Accept: application/json`). Then a guest gets a 401 and
    invalid input a 422, instead of redirects.
  - `.without_csrf()` sends without the CSRF token. (`TestApp` sends it for you otherwise.)
  - `app.csrf_token()` is the session's token, for a request you build yourself.

### Checking responses

- **Status:** `assert_ok`, `assert_status(n)`, `assert_redirect(to)`, `assert_hx_redirect(to)`,
  `assert_not_found`, `assert_forbidden`, `assert_unauthorized`.
- **Body** (the page or data that came back):
  - `assert_see` / `assert_dont_see` look for text in the HTML exactly as it was sent. Special
    characters are escaped there (`&` is `&amp;`), so write them that way.
  - `assert_invalid("field")`, `assert_header`.
  - `text()` returns the body as text, `json::<T>()` reads it as JSON, and `header(name)`
    returns a header (`Option<&str>`).
  - The fields `status`, `headers`, `body` and `view` are public too.
- **Invalid input:** `assert_invalid("field")` expects a 422 with an error on that field.
  - Only htmx and JSON requests get a 422, so send with `app.htmx()` or `app.request().json()`.
  - A plain form post is answered differently: with a 303 back to the form (the errors and the
    old input are flashed, which means kept in the session for the next page). So for that
    one, check `assert_status(303)`, then `get` the form and `assert_see` the message.
- **JSON:**
  - `assert_json_path("data.0.name", "Coffee")` checks the value at a path of keys and indexes
    (here: the key `data`, its first item, that item's `name`);
  - `json_path(path)` reads it;
  - `assert_json(json!({ … }))` checks the body contains the expected keys (other keys may
    be there too). Arrays are stricter: they must have the same number of items, in the same
    order (each item is then checked the same way, so an object in an array may have extra
    keys).
- **Views:** `assert_view("products/index.html")` checks the template a page was rendered from.

## Configuration and the app's parts

`TestApp::new` doesn't read your `.env` file, with one exception: `TEST_DATABASE_URL` (from the
environment or `.env`). It starts from `Config::default()`, which has:

- an in-memory database;
- `APP_ENV=testing` (so `SESSION_DRIVER=database` keeps sessions in memory, where the session
  helpers read them);
- the memory mailer (mail is kept in a list, not sent);
- no queue workers and no scheduler (so nothing runs in the background by surprise);
- debug on;
- a 30-second wait for a database connection (the `.env` default is 5: parallel tests open many
  SQLite files at once; an in-memory database still gives up after 2 seconds).

Every other setting has the default `.env.example` lists.

Change any of it with `TestApp::with_config(app, |c| …)`.

To reach the app's parts from a test:

- `app.state()` is the `AppState` that handlers get;
- `app.db()` is its database;
- `app.mailer()` is its mailer (`mailer().sent()` lists the mail).

The test below changes two settings, then fakes an outside web service:

```rust
use renox::prelude::*;
use renox::http::FakeResponse;
use renox::testing::TestApp;

/// Reads a setting of the test's own, and fakes an outside web service.
#[renox::test]
async fn rates_come_from_the_api() {
    // Start with a setting of our own, and with debug off.
    let app = TestApp::with_config(App::new(), |c| {
        c.vars.insert("RATES_KEY".into(), "test-key".into()); // what config.var reads
        c.debug = false; // production error pages
    })
    .await;
    assert_eq!(app.state().config.var("RATES_KEY").as_deref(), Some("test-key"));

    // From here on, web requests get fake answers instead of reaching the internet.
    let http = app.fake_http(); // `*` matches anything; "POST https://…" for one method
    http.on("https://api.example.com/*", FakeResponse::json(200, json!({ "eur": 0.92 })));
    let res = app.state().http.get("https://api.example.com/rates").send().await.unwrap();
    assert_eq!(res.status(), 200);
    // Check which requests were sent, and how many.
    http.assert_sent(|r| r.method == "GET" && r.url.starts_with("https://api.example.com/rates"));
    http.assert_not_sent(|r| r.method == "POST");
    http.assert_sent_count(1); // http.sent() lists them: method, url, headers, body, json()
}
```

What's going on:

- `with_config` sets an app-specific value (`RATES_KEY`) and turns debug off, so errors show
  the production error pages.
- `fake_http()` stops real web requests. `http.on(pattern, answer)` says what to answer for
  addresses that match the pattern.
- `assert_sent`, `assert_not_sent` and `assert_sent_count` check what the app asked for.

Other fake answers: `FakeResponse::text(status, body)`, `FakeResponse::status(n)`,
`.header(…)`, and `FakeResponse::connection_error()` (as if the service were down). Give one
pattern several answers, and they're used in turn: the first request gets the first, and so on.

## Session and login

- `acting_as(&user)` logs in. `logout()` forgets the session, like a browser with its cookies
  cleared.
- `confirm_password()` passes `require_password_confirmed` (pages that ask for the password
  again before something risky).
- `assert_authenticated(Some(&user))` checks that this user is logged in,
  `assert_authenticated(None)` that someone is (any user), and `assert_guest()` that no one is.
- `assert_session_has("cart")`, `assert_session_missing("cart")` and
  `session_get::<T>("cart")` check and read values in the session.
- `session_cookie()` and `use_session_cookie(…)` let one test play a second device (say, a
  phone and a laptop logged in as the same user).

## The database

- `assert_database_has("orders", &[("status", &"paid")])` checks a matching row exists,
  `assert_database_missing(…)` that none does, and `assert_database_count("orders", 3)` how
  many rows a table has.
- `renox::db::capture_queries(future)` returns every SQL query the future ran, requests
  included. A test can use it to catch an N+1: a page that runs one more query for every row,
  so it gets slower the more rows there are:
  `let (res, queries) = capture_queries(app.get("/posts")).await; assert!(queries.len() <= 3);`

### Models and the schema

`app.assert_models_match_schema().await` checks every model registered with `App::model`
against the tables your migrations built in the test's own database, and panics with the
report (the one `rnx db:check` prints). Put it in one test per app:

```rust
use renox::prelude::*;
use renox::testing::TestApp;

# #[derive(Model, serde::Serialize, Default)]
# struct Product { id: i64 }
#[renox::test]
async fn the_models_match_the_schema() {
    let app = TestApp::new(App::new().model::<Product>()).await;
    app.assert_models_match_schema().await;
}
```

See [Checking models against the tables](types.md#checking-models-against-the-tables).

### Templates

`app.assert_views_compile()` compiles every template (the app's, the built-in ones and the
plugins') and checks each literal `route('name')` against the route table, and the
variables of [typed view contexts](ui.md#typed-view-contexts) against their structs. It panics with one
`template:line: message` line per problem, the same list `rnx view:check` prints. `rnx new`
apps have this test already:

```rust
use renox::prelude::*;
use renox::testing::TestApp;

#[renox::test]
async fn every_view_compiles() {
    TestApp::new(App::new()).await.assert_views_compile();
}
```

### Factories

Factories fill tables with made-up rows, so a test doesn't have to type every field:

- one row: `Product::factory().create_one(app.db()).await`;
- twenty rows: `Product::factory().count(20).create(app.db()).await`;
- with states (named changes, like `sold_out`) and sequences (a change that differs per row):
  `Product::factory().count(3).state(sold_out).sequence(|i, p| p.name = format!("Coffee {i}")).create(app.db()).await`;
- `make()` instead of `create(…)` builds unsaved models;
- for a single one: `factory().state(…).make_one()` / `.create_one(db)`.

## Jobs, events, notifications, mail, HTTP

Apps do work outside the request too: jobs (tasks put in a queue to run in the background),
events, mail, scheduled tasks and commands. In a test nothing runs in the background, so these
tools let you run the work, or record it and check it.

| Tool | What it does |
|---|---|
| `app.queued_jobs()` | The names of the queued jobs. |
| `app.run_jobs()` | Runs the jobs that are due. |
| `app.run_all_jobs()` | Also runs delayed jobs and retries still waiting for their backoff (the pause before a failed job is tried again), until no job is left. It stops after 1,000 rounds, so a job that queues itself again forever can't hang the test. |
| `app.fake_events()` | Records events instead of running their listeners. Check them with `assert_emitted::<OrderPlaced>(\|e\| e.id == 7)`, `emitted::<E>()` or `assert_not_emitted::<E>()`. |
| `app.fake_notifications()` | Records notifications instead of sending them. Check them with `assert_notified(&user, "order-shipped")`, `assert_notified_to("a@b.c", kind)`, `notifications()` or `assert_nothing_notified()`. |
| `app.fake_broadcasts()` | Records `state.broadcast` / `broadcast_to` instead of sending them to open pages. Check them with `assert_broadcast("order-updated", \|b\| b.data["id"] == 7)` or `broadcasts()` (each a `SentBroadcast`: `user_id`, `event`, `data`). |
| `app.sent_mail()`, `app.assert_mail_sent(to, subject)` | The mail sent so far (the test mailer keeps it). |
| `app.fake_http()` | Answers `state.http` requests with fakes and records them. A request without a fake is an error, so nothing reaches the network. |
| `app.run_scheduled("report")` | Runs a scheduled task now, at the travelled time. |
| `app.kernel().call("products:import", ["a.csv"])` | Runs one of the app's own commands. |
| `renox::prompt::answering(["a.csv", "yes"], app.kernel().call("products:import", [""; 0])).await` | Runs a command that asks questions (`renox::prompt::ask`, `confirm`, …), answering them in order. |
| `my_app::app().run_args(["migrate:status"]).await` | Runs any command the binary has, built-ins included (`migrate`, `queue:failed`, `down`…), as `my-app migrate:status` would. Give the app a file database: each call boots it anew. |

> [!WARNING]
> `run_args` boots a new app on every call. With an in-memory database, each boot gets an
> empty one, so give the app a file database for these tests.

## Live components

`app.live(component)` tests a live component without a page: it seals the state the way a mount
does and keeps the snapshot between calls, as the browser does.

```rust
# use renox::prelude::*;
# use renox::testing::TestApp;
# #[derive(serde::Serialize, serde::Deserialize, Default)]
# struct Counter { count: i64, search: String }
# impl LiveComponent for Counter {
#     const NAME: &'static str = "counter";
#     const VIEW: &'static str = "live/counter.html";
#     async fn call(&mut self, _: &str, _: Vec<serde_json::Value>,
#         _: &mut renox::live_component::LiveContext) -> Result { Ok(()) }
# }
# async fn demo(app: TestApp) {
let mut counter = app.live(Counter::default());
counter.set("search", "tea");
counter.call("increment").await.assert_ok();
assert_eq!(counter.component().search, "tea");
# }
```

`set` fields are sent with every call. `call_with("add", json!([5]))` passes arguments, and
`component()` reads the state from the last answer. The component must be registered with
`App::live_component`.

## Time

Some things depend on time: a session that ends after two hours, a password confirmation that
lasts three. Waiting for real in a test would be far too slow. Instead, `TestApp` can move its
clock forward. This is called **travel**.

- `app.travel(Duration::from_secs(3600))` moves the clock one hour forward for what the
  `TestApp` does next: requests, `run_jobs` and `run_all_jobs`.
- `app.at_travelled_time(fut)` runs other code at that time, such as a model call, a scheduled
  task (`app.run_scheduled(..)`) or a command.
- `app.travel_back()` returns to the present.
- Travel adds up: two hours, then one more, is three hours ahead.

What follows the travelled clock: `renox::db::now()`, sessions, signed URLs, the queue, the
cache, rate limits, the login lock and password confirmation. `TestApp`'s own session helpers do
too: past `SESSION_LIFETIME`, the next request starts a new session with its own CSRF token.

Say the app's billing page asks for the password again (and the app has the `Auth` module, which
gives the confirm-password page):

```rust
# use renox::prelude::*;
# async fn billing() -> &'static str { "Billing" }
# fn routes() -> Routes {
Routes::new()
    .get("/settings/billing", billing)
    .require_password_confirmed()
# }
```

A test can then check that the confirmation runs out:

```rust
# use renox::prelude::*;
# use std::time::Duration;
# async fn demo(app: renox::testing::TestApp, user: User) {
app.acting_as(&user).confirm_password();
app.get("/settings/billing").await.assert_ok();
// Jump four hours ahead: the confirmation has run out.
app.travel(Duration::from_secs(4 * 60 * 60)); // confirmation lasts three hours
app.get("/settings/billing").await.assert_redirect("/confirm-password");
# }
```

What's going on: the user logs in and confirms their password, so the billing page opens. Then
four hours pass. The page needs a fresh confirmation now, so the app sends them to the
confirm-password page.

> [!IMPORTANT]
> Travel reaches only time read through Renox: `renox::db::now()` in app code, not
> `SystemTime::now()` or `chrono::Utc::now()`. Use travel rather than rewriting `created_at`
> with SQL or sleeping; examples/hello and the bike shop's tests (rentals, plans, stock) show it.

[examples/bikeshop](../examples/bikeshop) has tests per area, a walk of its main pages counting queries on a large seed, and
browser tests (`tests/browser/bikeshop-*.test.mjs`).

## Browser tests

Most of an app is covered by `TestApp` requests. Some things only a real browser shows: a sheet
(a panel that slides in) opening, live validation as you type, a layout at phone width. For
those, serve the app on a real port and drive a browser over the Chrome DevTools Protocol (CDP:
the way programs remote-control Chrome).

```rust
# async fn demo(app: renox::testing::TestApp) {
let url = app.serve().await; // http://127.0.0.1:PORT, until the test ends
// Point a WebDriver or CDP client at `url` (fantoccini, chromiumoxide, or a Node script),
// e.g. open `{url}/products/new`, type, press Tab, and read `[data-error-for=price]`.
# let _ = url; }
```

`app.serve()` starts a real server on a free port and returns its address. The server keeps
running until the test ends.

Renox tests its own JavaScript this way, in `tests/browser/` (run them with
`tests/browser/run.sh`, or one file: `tests/browser/run.sh grid`). It is a small harness you
can copy into an app: Node's own test runner and WebSocket speaking CDP to headless Chrome
(Chrome without a window), with no npm packages.

- `lib/cdp.mjs` starts Chrome with a throwaway profile and gives each test a fresh page:
  `goto`, real mouse clicks (on the element once it stands still; a click Chrome didn't
  deliver is sent again, then through the DOM with a line on stderr), typing (`Input.insertText`), key presses
  (`Input.dispatchKeyEvent`: events made up by a script don't trigger every handler),
  `waitFor` a condition, `settle` until htmx is idle. It collects console errors, uncaught
  exceptions and CSP violations (scripts or styles the page's security policy blocked), and
  `assertClean()` fails the test when there are any.
- `lib/app.mjs` starts an app binary on a free port with its own database and storage,
  migrated and seeded, and stops it; `restart()` stops it and starts it again on the same
  port and database (what a page sees when the server restarts).

```js
test('a 422 keeps what was typed', () =>
  browser.with(async (page) => {
    await page.goto(`${app.url}/products/new`);
    await page.type('[name=price]', '-5');
    await page.click('form button[type=submit]');
    await page.waitFor(() => document.querySelector('[name=price]').getAttribute('aria-invalid'));
    assert.equal(await page.eval(() => document.querySelector('[name=price]').value), '-5');
    page.assertClean({ allow: [/422/] });
  }));
```

Things that cost time to find:

- Close every page (`browser.with` does it, even when the test fails): each open tab keeps its
  live-reload stream, and Chrome allows six connections per host, so later pages hang.
- Headless Chrome has no mouse that hovers (`(hover: hover)` is false), so hover-only behaviour
  (tooltips) never runs. Start it with
  `--blink-settings=primaryHoverType=2,availableHoverTypes=2,primaryPointerType=4,availablePointerTypes=4`.
- The browser checks `required` and `type="email"` itself and won't send an invalid form. To
  test the server's errors, give the form `novalidate` (the kit's sheets have it).
- `hx-confirm` opens a native `confirm()` that blocks the page; accept it with
  `Page.handleJavaScriptDialog` (the harness does, and records it in `page.dialogs`).
- Take screenshots at 1100 px, 390 px (a phone) and in dark mode, and look at them: layout
  problems (an element pushed off screen, a misaligned dialog) pass every assertion. The
  harness saves one for every test that fails.

> [!TIP]
> Look at the screenshots, not only the numbers. Layout problems (an element pushed off
> screen, a misaligned dialog) pass every assertion.
