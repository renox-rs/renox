# Build your first Renox app

In this tutorial you build one small, real web app, from an empty folder to a server on the
internet. The app is called **Stash**: a place to keep links you want to read later.

When it's done, Stash can:

- let people sign up and log in;
- save a link without reloading the page;
- let each person edit and delete **only their own** links;
- send everyone an email every Monday with the links they saved that week;
- check itself with automatic tests;
- run on a server as **one single file**.

You don't need to know Laravel or any other web framework. You need Rust 1.94 or later, and a
little Rust: what a `struct` and a `fn` are, and what `async` and `?` do. Every piece of code
comes with an explanation of what it does and why.

> [!TIP]
> Know Laravel already? Look for the **Coming from Laravel** notes: they say where Renox does
> the same thing, and where it's different. [Coming from Laravel](laravel.md) has the full list.

| Step | What you'll learn |
|---|---|
| [1. Create the app](#1-create-the-app) | make an app with one command, and what its files are for |
| [2. A model and its table](#2-a-model-and-its-table) | store bookmarks in a database table |
| [3. A list page](#3-a-list-page) | show a page with your bookmarks, for logged-in people only |
| [4. A form that saves without a reload](#4-a-form-that-saves-without-a-reload) | check what people type, and save it |
| [5. Edit and delete, only your own](#5-edit-and-delete-only-your-own) | decide who may change what |
| [6. A weekly digest mail](#6-a-weekly-digest-mail) | do work in the background, on a timetable |
| [7. Tests and demo data](#7-tests-and-demo-data) | check the app automatically, and fill it with fake data |
| [8. Deploy](#8-deploy) | put the app on a real server |

### Words you'll meet

Web apps have their own words. Here are the ones this tutorial uses, in plain English:

| Word | What it means |
|---|---|
| **request** and **response** | The browser asks for a page (a request); your app answers (a response). |
| **route** | A rule that says "when someone opens `/bookmarks`, run this function". |
| **handler** | The function a route runs. It gets the request's details and returns the page. |
| **module** | A folder of code for one feature (here, everything about bookmarks). |
| **model** | A Rust struct that matches a table in the database: one struct = one row. |
| **migration** | A small SQL file that creates or changes a table. |
| **template** (or view) | An HTML file with blanks that the app fills in, like `{{ bookmark.title }}`. |
| **htmx** | A small script (it comes with Renox) that updates part of a page without a reload. |

## 1. Create the app

First install `rnx`, Renox's command-line tool. It makes new apps and writes code for you:

```bash
cargo install renox-cli
```

If something doesn't work later, `rnx doctor` says what this machine or app is missing (a
linker, a database, a setting).

Then make the app and run it:

```bash
rnx new stash
cd stash
rnx serve
```

What these three commands do:

- `rnx new stash` makes a folder called `stash` with a working app inside.
- `cd stash` goes into that folder.
- `rnx serve` builds the app and starts it.

An app's name may use lowercase letters, digits, `-` and `_`, and must start with a letter.
Rust keywords and the names of crates the app uses (`renox`, `serde`, `tokio`, `std`, `test`,
…) aren't allowed, so `rnx new MyApp` and `rnx new test` fail with a message that says why.

The first build downloads and compiles everything Renox needs, so it takes a few minutes. After
that, builds take seconds ([development.md](development.md) has tips to make them faster).

Now open <http://127.0.0.1:3000> in your browser. You'll see a home page with a navigation bar
and **Log in** and **Register** buttons, and they already work. Register an account: you come back
to the home page, logged in, with your name in a menu on the right.

> [!NOTE]
> Leave `rnx serve` running while you work. When you change a `.rs` file, it rebuilds and restarts
> the app by itself. When you change a template (an `.html` file), the browser reloads by itself.
> It also updates the database (runs the migrations) before each start.

### What `rnx new` made

```text
stash/
├── Cargo.toml              the project's settings; one dependency: renox (plus serde)
├── .env, .env.example      settings like the database address and the secret key
├── .gitignore              keeps target/, storage/, .env and dist/ out of Git
├── build.rs                tells Rust to rebuild when migrations, views or public files change
├── src/
│   ├── main.rs             starts the app
│   ├── lib.rs              puts the app together: its modules, migrations and seeders
│   └── app/
│       ├── mod.rs          the list of modules
│       └── home/mod.rs     the home page: its route and its handler
├── resources/
│   ├── views/layouts/app.html     the frame around every page: navigation bar, account menu
│   ├── views/home/index.html      the home page
│   ├── views/errors/default.html  the "not found" (404) and "error" (500) pages
│   └── lang/en.json        the app's texts
├── public/app.css          your own styles (the UI kit brings the rest)
├── migrations/             SQL migrations (empty for now, only a .gitkeep)
├── tests/home.rs           tests that start the whole app and click around
└── AGENTS.md, CLAUDE.md    notes for AI coding assistants working on the app
```

The data lives in a **SQLite** database: one file, `storage/app.db`, made the first time it's
needed. (Want PostgreSQL instead? Run `rnx new stash --database postgres`; see
[postgresql.md](postgresql.md).)

> [!TIP]
> `rnx new stash --starter` starts from a bigger app that already has email verification, roles,
> a dashboard and a users page. To be an admin there, register and run
> `rnx users:admin you@example.com`, or run `rnx db:seed` and log in as `admin@example.com` with
> the password `password123`. This tutorial starts from the plain app, so you see every step.

Two files are worth a look now. The first is `src/main.rs`, which is almost empty:

```rust,no_run
# mod stash { pub fn app() -> renox::App { renox::App::new() } }
/// The program's starting point: build the app (from `src/lib.rs`) and run it.
fn main() -> renox::Result {
    stash::app().run()
}
```

Why so short? The real app is built in `src/lib.rs`, so that the tests (step 7) can start
exactly the same app.

`run()` does more than start the web server. The app is also its own command-line tool:
`stash migrate` updates the database, `stash route:list` lists every address the app answers.
While you develop, type `rnx` in front instead (`rnx route:list`); it builds and runs it for you.

The second file is `src/app/home/mod.rs`, the home page. It's a **module**: a feature's name
and its routes.

```rust
use renox::prelude::*;

/// The home page module. It holds no data, so it's an empty struct.
pub struct Home;

impl Module for Home {
    /// The module's name, used in logs and error messages.
    fn name(&self) -> &'static str {
        "home"
    }

    /// The addresses this module answers: `/` runs `index`. The route is
    /// named "home", so templates can link to it with `route('home')`.
    fn routes(&self) -> Routes {
        Routes::new().get("/", index).name("home")
    }
}

/// The handler for `/`: fill in the template `home/index.html` and send it.
async fn index() -> View {
    // `context! {}` holds the values the template can use; there are none yet.
    view("home/index.html", context! {})
}
```

The line `use renox::prelude::*;` brings in everything an app usually needs (`Routes`, `View`,
`view`, `Module` and more), so you don't import them one by one.

A **handler** is a plain `async` function. Its arguments say what it needs (the database, the
logged-in user, a checked form…) and Renox hands them over. It returns the answer: here a
`View`, a page made from a template. More about routes and handlers in [routing.md](routing.md).

> [!NOTE]
> **Coming from Laravel:** there is no `routes/web.php`. Each module keeps its routes next to
> its handlers. Templates use MiniJinja, which looks a lot like Blade.

## 2. A model and its table

A bookmark has a title, an address (URL), an optional note, and an owner. Let `rnx` write the
code: first a module for everything about bookmarks, then the model in it:

```bash
rnx make:module bookmarks
rnx make:model Bookmark --module bookmarks --migration
```

What they made:

- `make:module bookmarks` wrote `src/app/bookmarks/mod.rs` (with a placeholder page) and added
  the module to `src/lib.rs`, so the app uses it.
- `make:model Bookmark` wrote `src/app/bookmarks/model.rs`.
- `--migration` added two files to `migrations/`, named after the current time:
  `…_create_bookmarks_table.up.sql` and `…_create_bookmarks_table.down.sql`.

The table is called `bookmarks` (the plural), and the model says so with
`#[model(table = "bookmarks")]`.

### The migration

A **migration** is a SQL file that changes the database: here, it creates the table. Renox
uses plain SQL, the same `CREATE TABLE` you'd type into the database yourself. Fill in
`migrations/<timestamp>_create_bookmarks_table.up.sql`:

```sql
-- A table for the bookmarks: one row per saved link.
CREATE TABLE "bookmarks" (
    -- A number the database counts up for each new row: 1, 2, 3…
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Who saved it. Deleting a user deletes their bookmarks too (CASCADE).
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    -- No NOT NULL here: the note may be left empty.
    note TEXT,
    created_at TEXT,
    updated_at TEXT
);
-- An index makes "the bookmarks of user 7" fast to find.
CREATE INDEX bookmarks_user_id ON bookmarks (user_id);
```

and the `.down.sql` next to it, which undoes it (`rnx migrate:rollback` runs it):

```sql
DROP TABLE "bookmarks";
```

Where does the `users` table come from? From Renox's `Auth` module (the login pages). It has
its own migrations, and they run before yours.

### The model

A **model** is a Rust struct that matches the table: each field is a column, and each value
of the struct is one row. Replace `src/app/bookmarks/model.rs` with:

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};

/// One saved link: a row of the `bookmarks` table.
///
/// `Model` gives it database methods (`create`, `find`, `save`, `delete`
/// and queries). `Serialize` lets templates read its fields. `Default`
/// makes an empty bookmark to start from.
#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "bookmarks", form)]
pub struct Bookmark {
    /// The row's number. `0` means "not saved yet".
    pub id: i64,
    /// The user who saved it. The app sets it, so it is not in the form.
    #[form(skip)]
    pub user_id: i64,
    /// Must be filled in, at most 200 characters.
    #[form(validate(required, max = 200))]
    pub title: String,
    /// Must be a web address (`http://` or `https://`). The label makes the
    /// error say "The address field…" instead of "The url field…".
    #[form(validate(required, url, max = 2000, label = "address"))]
    pub url: String,
    /// `Option` because the column may be empty (`NULL`): `None` means no note.
    /// When it's there, at most 1000 characters.
    #[form(validate(max = 1000))]
    pub note: Option<String>,
    /// Filled in by Renox when the bookmark is first saved.
    pub created_at: Option<DateTime>,
    /// Filled in by Renox every time the bookmark is saved.
    pub updated_at: Option<DateTime>,
}
```

With `#[derive(Model)]`, you can now write things like:

- `Bookmark::find(&db, 7)`: get bookmark number 7;
- `Bookmark::query().where_(Bookmark::USER_ID.eq(7)).latest().get(&db)`: user 7's bookmarks,
  newest first;
- `bookmark.save(&db)` and `bookmark.delete(&db)`.

`Bookmark::USER_ID` is a **typed column**: the derive writes one constant per field, and it
knows the field's Rust type. A column that doesn't exist, or a value of the wrong type
(`Bookmark::USER_ID.eq("seven")`), is a compile error, so a typo is caught before the app even
runs.

`form` in `#[model(table = "bookmarks", form)]` makes a second struct, `BookmarkForm`, from the
model: the fields people type in (every field except `id`, the dates and any marked
`#[form(skip)]`), with the rules written in `#[form(validate(…))]`. Step 4 uses it, so a new
field is added in one place. Loading related rows (say, a bookmark's owner) is done explicitly, one query per page of
rows; see [relations.md](relations.md).

Save the files. `rnx serve` notices and runs the new migration (or run `rnx migrate`
yourself; `rnx migrate:status` shows what has run). Curious? `rnx db:shell` opens a SQL prompt
on the database.

Now check that the model and the table agree:

```bash
rnx db:check
```

`make:model` registered the model with the app (`app.model::<…>()`, which step 3 keeps), and
`db:check` compares its fields with the table the migrations build. Misspell a column in the
SQL and it tells you which one. Later, when you add a field to a model, `rnx make:migration
--auto add_something` writes the SQL for the difference, ready for you to read and run.

> [!NOTE]
> **Coming from Laravel:** a model is a plain struct, not an Eloquent class with magic
> attributes, and migrations are SQL instead of a schema builder.

## 3. A list page

Now a page that lists your bookmarks. Open `src/app/bookmarks/mod.rs`, keep the `pub mod model;`
line at the top, and replace everything below the `pub mod model;` line with:

```rust
# fn main() {}
# mod bookmarks {
# pub mod model {
#     use renox::prelude::*;
#     #[derive(Model, serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
#     #[model(table = "bookmarks")]
#     pub struct Bookmark {
#         pub id: i64, pub user_id: i64, pub title: String, pub url: String,
#         pub note: Option<String>, pub created_at: Option<DateTime>, pub updated_at: Option<DateTime>,
#     }
# }
use renox::prelude::*;

use model::Bookmark;

/// Everything about bookmarks: its routes now, more later.
pub struct Bookmarks;

impl Module for Bookmarks {
    fn name(&self) -> &'static str {
        "bookmarks"
    }

    /// Tells the app about this module's model (for `rnx db:check`) and the
    /// page's data (for `rnx view:check`).
    fn register(&self, app: &mut Registry) {
        app.model::<Bookmark>();
        app.view_data::<BookmarkList>();
    }

    /// `GET /bookmarks` shows the list (the `index` handler). Only for
    /// people who are logged in.
    fn routes(&self) -> Routes {
        Routes::new()
            .resource("/bookmarks", "bookmarks", Resource::new().index(index))
            .require_auth()
    }
}

/// What `bookmarks/index.html` gets: its fields are the template's variables.
#[derive(serde::Serialize, ViewData)]
#[view(template = "bookmarks/index.html")]
struct BookmarkList {
    bookmarks: Paginated<Bookmark>,
}

/// Shows one page of the logged-in user's bookmarks.
///
/// Each argument asks Renox for something: `db` is the database, `user`
/// the person who is logged in, `page` the page number from the address
/// (`/bookmarks?page=2`), or 1 when there is none.
async fn index(State(db): State<Db>, user: AuthUser, Page(page): Page) -> Result<View> {
    let bookmarks = page_of(&db, user.id, page).await?;
    // Fill in the template with the bookmarks and send the page.
    Ok(View::typed(BookmarkList { bookmarks }))
}

/// One page (20 rows) of the user's bookmarks, newest first.
async fn page_of(db: &Db, user_id: i64, page: u32) -> Result<Paginated<Bookmark>> {
    Bookmark::query()
        .where_(Bookmark::USER_ID.eq(user_id))
        .latest()
        .paginate(db, page, 20)
        .await
}
# }
```

Let's go through it:

- **`resource("/bookmarks", "bookmarks", …)`** adds the usual addresses of a "resource" (a
  list, a form to create, a page to edit…), but only the ones you name. For now that's
  `index`: `GET /bookmarks`, named `bookmarks.index`. A template links to it with
  `route('bookmarks.index')`, so the address can change later without breaking links.
- **`.require_auth()`** protects the routes above it. Someone who isn't logged in is sent to
  `/login`, and comes back here after logging in.
- **The handler's arguments** are called *extractors*: each one takes something out of the
  request. `State(db): State<Db>` is the database, `AuthUser` the logged-in user (`user.id`,
  `user.name`, `user.email`), `Page(page)` the page number.
- **`paginate`** gets one page of rows, plus the total, so the template can show page links.
- **`BookmarkList`** is the page's data as a struct (`#[derive(ViewData)]`): it names its
  template, and `View::typed` sends it. A template that uses a variable the struct lacks is
  reported by `rnx view:check`, instead of silently showing an empty spot.
- **`register`** tells the app about the model (`rnx db:check`) and about `BookmarkList`.
- **`Result<View>`**: the function returns either a page or an error. `?` means "if this
  failed, stop here and return the error". Renox then shows an error page (with the details
  while you develop, and without them for visitors).

### The view

A **view** (or template) is an HTML file with blanks. `{{ … }}` prints a value, `{% … %}` is
logic like `if` and `for`. Replace `resources/views/bookmarks/index.html`:

```html
<rx-page layout="layouts/app.html" title="Bookmarks">
  <rx-stack>
    <rx-page-header title="Bookmarks" subtitle="Links worth keeping."/>

    {% block bookmarks %}
    <section id="bookmarks" class="rx-stack">
      {% if bookmarks.items %}
        <rx-list label="Your bookmarks">
          <li rx-for="bookmark in bookmarks.items">
            <span class="rx-list__main">
              <a class="rx-link" href="{{ bookmark.url }}" target="_blank" rel="noopener">{{ bookmark.title }}</a>
              {% if bookmark.note %}<br><span class="rx-subtitle">{{ bookmark.note }}</span>{% endif %}
            </span>
          </li>
        </rx-list>
        {% from "renox/pagination.html" import pagination %}
        {{ pagination(bookmarks) }}
      {% else %}
        <rx-empty title="No bookmarks yet" message="Links you save show up here, newest first."/>
      {% endif %}
    </section>
    {% endblock %}
  </rx-stack>
</rx-page>
```

What's going on:

- `<rx-page layout="layouts/app.html" title="Bookmarks">` puts this page inside the layout (the
  navigation bar and the rest), and sets the page's title for search engines. What is inside it
  is the part of the layout this page fills in.
- `<rx-page-header>`, `<rx-list>` and `<rx-empty>` are tags from Renox's **UI kit**: ready-made
  parts for a page's title, a list of rows and the "nothing here yet" message. They bring their
  own look, dark mode and keyboard support, so you write no CSS. Each tag is a call of a kit
  tag or attribute that doesn't exist, or a route name that doesn't, is reported with the file
  and line when the page loads. [ui.md](ui.md) lists them all.
- `rx-for="bookmark in bookmarks.items"` repeats the `<li>` for every bookmark, and
  `{{ bookmark.title }}` prints its title.
- The page links are the one part without a tag: `pagination` is imported from
  `renox/pagination.html` inside the block (so the block can also be rendered on its own, as step 4 does).
- Values are **escaped**: a title with `<script>` in it shows up as text and never runs.
- `{% block bookmarks %}` gives the list a name. In step 4, the app sends back just this part.

Last, a link in the navigation bar. In `resources/views/layouts/app.html`, add the section
links inside the `rx-navbar` tag, before the spacer:

```html
<rx-navbar brand="{{ app.name }}" href="{{ route('home') }}">
  {% if auth.check %}
    <rx-nav-links>
      <rx-nav-link route="bookmarks.index" label="Bookmarks" :active="route_is('bookmarks.*')"/>
    </rx-nav-links>
  {% endif %}
  <span class="rx-spacer"></span>
  …
```

`auth` is available in every template: `auth.check` is true when someone is logged in, and
`auth.user.name` is their name. `route_is('bookmarks.*')` highlights the link on every
bookmarks page.

Check the pages: `rnx view:check` loads every template and checks its tags, its route names
and `BookmarkList`'s fields; it prints nothing wrong here, and `cargo test` runs the same check
(`rnx new` made that test). Reload the browser: **Bookmarks** in the bar leads to an empty list. Now log out and open
<http://127.0.0.1:3000/bookmarks>: you're sent to the login page, and back after logging in.

## 4. A form that saves without a reload

Next, a form above the list to save a new link. It's sent with **htmx**: instead of loading a
whole new page, the browser gets back only the updated list and swaps it in. You write no
JavaScript for this; htmx comes with Renox.

### The handler

Add this to `src/app/bookmarks/mod.rs`, with `use renox::Toast;` next to the other `use` lines,
and `BookmarkForm` next to `Bookmark` in `use model::Bookmark;` (so it reads
`use model::{Bookmark, BookmarkForm};`):

```rust
# fn main() {}
# mod bookmarks {
# pub mod model {
#     use renox::prelude::*;
#     #[derive(Model, serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
#     #[model(table = "bookmarks", form)]
#     pub struct Bookmark {
#         pub id: i64,
#         #[form(skip)]
#         pub user_id: i64,
#         #[form(validate(required, max = 200))]
#         pub title: String,
#         #[form(validate(required, url, max = 2000, label = "address"))]
#         pub url: String,
#         #[form(validate(max = 1000))]
#         pub note: Option<String>,
#         pub created_at: Option<DateTime>, pub updated_at: Option<DateTime>,
#     }
# }
# use renox::prelude::*;
# use renox::Toast;
# use model::{Bookmark, BookmarkForm};
# #[derive(serde::Serialize, ViewData)]
# #[view(template = "bookmarks/index.html")]
# struct BookmarkList { bookmarks: Paginated<Bookmark> }
# async fn page_of(db: &Db, user_id: i64, page: u32) -> Result<Paginated<Bookmark>> {
#     Bookmark::query().where_(Bookmark::USER_ID.eq(user_id)).latest().paginate(db, page, 20).await
# }
/// Saves a new bookmark from the form, then answers with the new list.
///
/// `BookmarkForm` is the struct `#[model(form)]` made in step 2.
/// `Valid(form)` only lets the function run when every rule passed, so
/// `form` is always good input here. It must be the last argument.
async fn store(
    State(db): State<Db>,
    user: AuthUser,
    htmx: Htmx,
    Valid(form): Valid<BookmarkForm>,
) -> Result<Response> {
    // Make the row: the owner is the logged-in user, and `fill` copies the
    // form's fields onto it. Then save it.
    let mut bookmark = Bookmark { user_id: user.id, ..Default::default() };
    form.fill(&mut bookmark);
    let bookmark = Bookmark::create(&db, bookmark).await?;
    // A small message that pops up on the page.
    let toast = Toast::success(format!("Saved “{}”.", bookmark.title));

    if htmx.request {
        // htmx sent the form: answer with only the `bookmarks` block of
        // the page, which htmx swaps in.
        let bookmarks = page_of(&db, user.id, 1).await?;
        let list = View::typed(BookmarkList { bookmarks }).fragment("bookmarks");
        return Ok((toast, list).into_response());
    }
    // Without JavaScript the form still works: go back to the list.
    Ok((toast, Redirect::route("bookmarks.index", &[])?).into_response())
}
# }
```

And give the resource its `store` action:

```rust
# async fn index() -> &'static str { "" }
# async fn store() -> &'static str { "" }
# use renox::prelude::*;
# fn routes() -> Routes {
Routes::new()
    .resource("/bookmarks", "bookmarks", Resource::new().index(index).store(store))
    .require_auth()
# }
```

Now `POST /bookmarks` (named `bookmarks.store`) runs `store`.

How it fits together:

- **The rules** live on the model's fields (`#[form(validate(…))]`, step 2), and `BookmarkForm`
  carries them. `url` accepts only `http://` and `https://` addresses, so nobody can save a
  `javascript:` link that would run code when clicked.
- **`form.fill(&mut bookmark)`** copies the form's fields onto the row. Add a field to the
  model and the form, the rules and `fill` all follow.
- **`Valid<BookmarkForm>`** reads the form and checks the rules *before* your function runs.
  If something is wrong, `store` never runs, and the person sees what to fix:
  - with htmx, each error appears under its field and the page doesn't move;
  - with a plain form, the browser goes back to the form, with the errors and with what was
    typed still filled in.
- **`Htmx`** tells you whether htmx sent the request (`htmx.request`).
- **`.fragment("bookmarks")`** sends only the `{% block bookmarks %}` part of the page. It's the
  same template as the full page, so there's nothing extra to keep in sync.
- **`Toast::success`** is a short message that pops up. It rides along with the answer; after a
  redirect it waits for the next page, where `{{ toasts() }}` in the layout shows it.

> [!NOTE]
> **Coming from Laravel:** the generated form struct works like a Form Request, and
> `.fragment()` replaces the partial views you'd write for Livewire or htmx.

### The form in the view

In `resources/views/bookmarks/index.html`, put the form between the `<rx-page-header>` and
`{% block bookmarks %}`:

```html
<rx-form route="bookmarks.store" hx-post="{{ route('bookmarks.store') }}"
         hx-target="#bookmarks" hx-swap="outerHTML"
         x-data="{}" @htmx:after-request="if ($event.detail.successful) $el.reset()" novalidate>
  <rx-card title="Save a link">
    <rx-input name="url" label="Address" type="url" required placeholder="https://…"/>
    <rx-input name="title" label="Title" required/>
    <rx-textarea name="note" label="Note" rows="2" hint="Why it's worth reading."/>
    <div class="rx-card__footer"><rx-button label="Save"/></div>
  </rx-card>
</rx-form>
```

Line by line:

- The form posts to `route="bookmarks.store"`, so it is a normal form and works even without
  JavaScript.
- `hx-post` makes htmx send it instead. `hx-target="#bookmarks"` and `hx-swap="outerHTML"`
  say what to do with the answer: replace the `<section id="bookmarks">` with it.
- `<rx-form>` writes the `<form>` for you, with `method="post"` and the `action` of the named
  route. It also adds a hidden security token. The token
  proves the form came from your own site, so another website can't post it in your name (this
  is called CSRF protection).
- `x-data @htmx:after-request=…` is a little Alpine.js (it also comes with Renox): after a
  *successful* save, empty the form. When there are errors, what you typed stays.
- `novalidate` turns off the browser's own checks, so the server's messages are the only ones,
  and they're the same everywhere.
- `<rx-card>`, `<rx-input>`, `<rx-textarea>` and `<rx-button>` come from the UI kit. Each field
  has a label, and a place under it where its error appears.

Try it. Press **Save** with the form empty: "The title field is required." appears under the
title, the address gets its own message, and the page doesn't move. Fill it in properly and
the bookmark appears at the top of the list, with a message. [validation.md](validation.md)
lists every rule; [ui.md](ui.md) explains fragments and messages.

## 5. Edit and delete, only your own

Every bookmark belongs to someone. The list only shows yours, but anyone could type the address
of an edit page, like `/bookmarks/7/edit`, and guess numbers. So the app must check: *is this
bookmark yours?*

That check lives in a **policy**: a function that answers "may this user do this to this
bookmark?" Make one, to say who may do what to a row:

```bash
rnx make:policy Bookmark --module bookmarks
```

The rules are up to you. Fill in `src/app/bookmarks/policy.rs`:

```rust
# fn main() {}
# mod model {
#     use renox::prelude::*;
#     #[derive(Model, serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
#     #[model(table = "bookmarks")]
#     pub struct Bookmark {
#         pub id: i64, pub user_id: i64, pub title: String, pub url: String,
#         pub note: Option<String>, pub created_at: Option<DateTime>, pub updated_at: Option<DateTime>,
#     }
# }
# mod policy {
use renox::prelude::*;

use super::model::Bookmark;

impl Policy for Bookmark {
    /// May `user` do `ability` ("update", "delete"…) to this bookmark?
    fn allows(&self, user: &User, ability: &str) -> bool {
        match ability {
            // Only the person who saved it may change or delete it.
            "update" | "delete" => self.user_id == user.id,
            // Anything else: no.
            _ => false,
        }
    }
}
# }
```

A handler then asks: `user.authorize("update", &bookmark)?`. If the policy says no, the handler
stops there and the visitor gets a **403 Forbidden** page. Anything the policy doesn't allow is
refused, so forgetting a case is safe.

### The whole module

Here is `src/app/bookmarks/mod.rs` with every action. The new parts are `edit`, `update` and
`destroy`; the `pub mod model;` and `pub mod policy;` lines stay at the top.

```rust
# fn main() {}
# mod bookmarks {
# pub mod model {
#     use renox::prelude::*;
#     #[derive(Model, serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
#     #[model(table = "bookmarks", form)]
#     pub struct Bookmark {
#         pub id: i64,
#         #[form(skip)]
#         pub user_id: i64,
#         #[form(validate(required, max = 200))]
#         pub title: String,
#         #[form(validate(required, url, max = 2000, label = "address"))]
#         pub url: String,
#         #[form(validate(max = 1000))]
#         pub note: Option<String>,
#         pub created_at: Option<DateTime>, pub updated_at: Option<DateTime>,
#     }
#     impl Policy for Bookmark {
#         fn allows(&self, user: &User, ability: &str) -> bool {
#             matches!(ability, "update" | "delete") && self.user_id == user.id
#         }
#     }
# }
use renox::Toast;
use renox::prelude::*;

use model::{Bookmark, BookmarkForm};

/// Everything about bookmarks.
pub struct Bookmarks;

impl Module for Bookmarks {
    fn name(&self) -> &'static str {
        "bookmarks"
    }

    fn register(&self, app: &mut Registry) {
        app.model::<Bookmark>();
        app.view_data::<BookmarkList>();
        app.view_data::<BookmarkEdit>();
    }

    /// Every address of the bookmarks, for logged-in people only. The
    /// comments show the method, the address and the route's name.
    fn routes(&self) -> Routes {
        Routes::new()
            .resource(
                "/bookmarks",
                "bookmarks",
                Resource::new()
                    .index(index) //     GET    /bookmarks            bookmarks.index
                    .store(store) //     POST   /bookmarks            bookmarks.store
                    .edit(edit) //       GET    /bookmarks/{id}/edit  bookmarks.edit
                    .update(update) //   PUT    /bookmarks/{id}       bookmarks.update
                    .destroy(destroy), // DELETE /bookmarks/{id}      bookmarks.destroy
            )
            .require_auth()
    }
}

/// What `bookmarks/index.html` gets.
#[derive(serde::Serialize, ViewData)]
#[view(template = "bookmarks/index.html")]
struct BookmarkList {
    bookmarks: Paginated<Bookmark>,
}

/// What `bookmarks/edit.html` gets.
#[derive(serde::Serialize, ViewData)]
#[view(template = "bookmarks/edit.html")]
struct BookmarkEdit {
    bookmark: Bookmark,
}

/// Shows one page of the logged-in user's bookmarks.
async fn index(State(db): State<Db>, user: AuthUser, Page(page): Page) -> Result<View> {
    let bookmarks = page_of(&db, user.id, page).await?;
    Ok(View::typed(BookmarkList { bookmarks }))
}

/// Saves a new bookmark from the form, then answers with the new list.
async fn store(
    State(db): State<Db>,
    user: AuthUser,
    htmx: Htmx,
    Valid(form): Valid<BookmarkForm>,
) -> Result<Response> {
    let mut bookmark = Bookmark { user_id: user.id, ..Default::default() };
    form.fill(&mut bookmark);
    let bookmark = Bookmark::create(&db, bookmark).await?;
    let toast = Toast::success(format!("Saved “{}”.", bookmark.title));

    if htmx.request {
        let bookmarks = page_of(&db, user.id, 1).await?;
        let list = View::typed(BookmarkList { bookmarks }).fragment("bookmarks");
        return Ok((toast, list).into_response());
    }
    Ok((toast, Redirect::route("bookmarks.index", &[])?).into_response())
}

/// Shows the edit form for one bookmark.
///
/// `Found<Bookmark>` loads the bookmark whose number is in the address
/// (`/bookmarks/7/edit` → bookmark 7). If there's no such bookmark, the
/// visitor gets a 404 page and this function never runs.
async fn edit(user: AuthUser, Found(bookmark): Found<Bookmark>) -> Result<View> {
    // Not yours? Stop here with a 403 page.
    user.authorize("update", &bookmark)?;
    Ok(View::typed(BookmarkEdit { bookmark }))
}

/// Saves the edit form's changes, then goes back to the list.
async fn update(
    State(db): State<Db>,
    user: AuthUser,
    Found(mut bookmark): Found<Bookmark>,
    Valid(form): Valid<BookmarkForm>,
) -> Result<(Toast, Redirect)> {
    // Check first, before changing anything.
    user.authorize("update", &bookmark)?;
    form.fill(&mut bookmark);
    // Write the changes to the database (and set `updated_at`).
    bookmark.save(&db).await?;
    Ok((
        Toast::success("Bookmark updated."),
        Redirect::route("bookmarks.index", &[])?,
    ))
}

/// Deletes one bookmark, then goes back to the list.
async fn destroy(
    State(db): State<Db>,
    user: AuthUser,
    Found(mut bookmark): Found<Bookmark>,
) -> Result<(Toast, Redirect)> {
    user.authorize("delete", &bookmark)?;
    bookmark.delete(&db).await?;
    Ok((
        Toast::success(format!("Deleted “{}”.", bookmark.title)),
        Redirect::route("bookmarks.index", &[])?,
    ))
}

/// One page (20 rows) of the user's bookmarks, newest first.
async fn page_of(db: &Db, user_id: i64, page: u32) -> Result<Paginated<Bookmark>> {
    Bookmark::query()
        .where_(Bookmark::USER_ID.eq(user_id))
        .latest()
        .paginate(db, page, 20)
        .await
}
# }
```

The new ideas:

- **`Found<Bookmark>`** reads the number from the address (the `{id}` part), loads that
  bookmark, and answers **404 Not Found** when there's none. You don't write that lookup
  yourself.
- **`authorize` comes first**, right after loading and before anything changes. Someone
  else's bookmark gets a 403 page, shown inside your layout
  (`resources/views/errors/default.html`).
- **`form.fill(&mut bookmark)`** puts the form's values on the loaded row, so `update` doesn't
  list the fields again.
- **`save`** writes every field back to the row and updates `updated_at`; **`delete`** removes
  the row.
- **`(Toast, Redirect)`**: a function can return several things at once. Here: a message, and
  "go to the list page", where the message then shows.

> [!NOTE]
> **Coming from Laravel:** `Found<Bookmark>` is route model binding. Laravel matches the
> parameter's *name*; Renox goes by the *type* you ask for.

### The edit page and the row buttons

Make `resources/views/bookmarks/edit.html`. It's a plain form, without htmx: when something is
wrong, the browser goes back to the form. That's the other way errors come back:

```html
<rx-page layout="layouts/app.html" title="Edit bookmark">
  <rx-stack>
    <rx-page-header title="Edit bookmark" :back="route('bookmarks.index')" back-label="Bookmarks"/>

    <rx-form :action="route('bookmarks.update', bookmark.id)" method="PUT" live novalidate>
      <rx-card>
        <rx-form-errors/>
        <rx-input name="url" label="Address" type="url" :value="bookmark.url" required/>
        <rx-input name="title" label="Title" :value="bookmark.title" required/>
        <rx-textarea name="note" label="Note" :value="bookmark.note" rows="2"/>
        <div class="rx-card__footer">
          <rx-link-button route="bookmarks.index" label="Cancel" variant="plain"/>
          <rx-button label="Save changes"/>
        </div>
      </rx-card>
    </rx-form>
  </rx-stack>
</rx-page>
```

- **`method="PUT"`**: browsers can only send forms as `GET` or `POST`. `<rx-form>` sends a
  `POST` with a hidden field that tells Renox "treat this as a `PUT`", so the form reaches
  `update`.
- **`<rx-form-errors/>`** lists all the errors at the top of the form. Each field also shows its
  own, and keeps what was typed.
- **`:value="bookmark.url"`** fills the field with the saved value (a colon means the value is
  an expression, not text). After a failed save, what you typed wins over the saved value.
- **`live`** checks each field as soon as you leave it, using the same rules on the
  server. Nothing is saved until you press **Save changes**.

In `index.html`, give each row its buttons. Replace the `<li>`:

```html
<li rx-for="bookmark in bookmarks.items">
  <span class="rx-list__main">
    <a class="rx-link" href="{{ bookmark.url }}" target="_blank" rel="noopener">{{ bookmark.title }}</a>
    {% if bookmark.note %}<br><span class="rx-subtitle">{{ bookmark.note }}</span>{% endif %}
  </span>
  <rx-row-actions>
    <rx-link-button :href="route('bookmarks.edit', bookmark.id)" label="Edit" variant="plain" size="small"/>
    <rx-confirm id="delete-{{ bookmark.id }}" label="Delete" :action="route('bookmarks.destroy', bookmark.id)"
                title="Delete “{{ bookmark.title }}”?" message="This can't be undone." size="small"/>
  </rx-row-actions>
</li>
```

- **`<rx-link-button>`** is a link that looks like a button: **Edit** opens the edit page.
- **`<rx-confirm>`** is a **Delete** button that first asks "Delete …? This can't be undone." Only
  the button in that question actually deletes (it sends a `DELETE` request).
- `{{ … }}` inside an attribute prints a value into the text (`id="delete-{{ bookmark.id }}"`
  → `delete-7`).
- **`<rx-row-actions>`** puts the buttons at the end of the row; on a phone they shrink to icons.

### Where to land after logging in

`rnx new` already added Renox's `Auth` module to `src/lib.rs`. It brings the login,
registration, password-reset and account pages, stores passwords safely (hashed with
Argon2id), and slows down people who guess passwords. After logging in, it sends people to the
`home` route. Send them to their bookmarks instead:

```rust
# use renox::prelude::*;
# let _ = App::new()
// `.account()` adds the /account page (name, email, password, devices, deleting
// the account); `.redirect_to(…)` is where people go after logging in.
.module(Auth::new().account().redirect_to("/bookmarks"))
# ;
```

Try it with two accounts (use a private browser window for the second): each person sees only
their own list, and the second gets a 403 page when opening the first one's
`/bookmarks/1/edit`. [authorization.md](authorization.md) covers more ways to decide who may do
what: roles, permissions and API tokens.

> [!TIP]
> **The shortcut.** `rnx make:module bookmarks --resource --fields "title:string url:string
> note:text"` writes a module like this one in one go: model, migration, form, the seven
> handlers, the pages and tests. It also runs the migration and prints the page's address
> (`--open` opens it in the browser, `--no-migrate` leaves the migration for later). Building
> it by hand once shows you what those files do.

## 6. A weekly digest mail

Every Monday morning, each person gets an email with the links they saved that week. Two parts
do this:

- a **job**: a piece of work that runs in the background, here "mail one person";
- a **scheduled task**: something that runs on a timetable, here "every Monday at 8:00, make a
  job for each user".

Why a job per user? Sending mail takes time and can fail. Each job is tried on its own, so one
failing address doesn't stop the others, and the web pages stay fast.

Renox keeps its list of jobs in your own database, and the workers that run them live inside
`rnx serve`, as does the scheduler: no Redis, no extra process, no cron entry.

```bash
rnx make:job SendDigest --module bookmarks
rnx make:mail digest
```

### The job

`make:job` wrote `src/app/bookmarks/send_digest.rs`; replace it with:

```rust
# fn main() {}
# mod model {
#     use renox::prelude::*;
#     #[derive(Model, serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
#     #[model(table = "bookmarks")]
#     pub struct Bookmark {
#         pub id: i64, pub user_id: i64, pub title: String, pub url: String,
#         pub note: Option<String>, pub created_at: Option<DateTime>, pub updated_at: Option<DateTime>,
#     }
# }
# mod send_digest {
use renox::chrono::TimeDelta;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

use super::model::Bookmark;

/// The job: mail one user the bookmarks they saved in the last seven days.
///
/// The queue stores the job as JSON until a worker runs it, so it holds
/// only the user's id: the worker loads fresh data when it runs.
#[derive(Serialize, Deserialize)]
pub struct SendDigest {
    pub user_id: i64,
}

impl Job for SendDigest {
    /// The job's name in the database. Don't change it once jobs are queued.
    const NAME: &'static str = "send-digest";

    /// What the worker does. Returning an error means "try again later".
    async fn handle(self, ctx: JobContext) -> Result {
        // `state` holds the app's shared parts: the database, the mailer…
        let state = &ctx.state;
        let Some(user) = User::find(&state.db, self.user_id).await? else {
            return Ok(()); // the account was deleted after the job was queued
        };
        // A week ago. `renox::db::now()` is "now" as the app sees it, which
        // tests can move forward (step 7).
        let since = renox::db::now() - TimeDelta::days(7);
        let bookmarks = Bookmark::query()
            .where_(Bookmark::USER_ID.eq(user.id))
            .where_(Bookmark::CREATED_AT.gte(since))
            .latest()
            .get(&state.db)
            .await?;
        if bookmarks.is_empty() {
            return Ok(()); // nothing new: no mail
        }
        // Fill in the mail templates (`mail/digest.html` and `.txt`)…
        let mail = state.mail_view(
            &user.email,
            "Your week in bookmarks",
            "mail/digest",
            context! { name => user.name, bookmarks },
        )?;
        // …and send it.
        state.mailer.send(mail).await
    }
}

/// The scheduled task: queue one `SendDigest` job per user.
pub async fn send_digests(state: AppState) -> Result {
    for user in User::all(&state.db).await? {
        state.dispatch(SendDigest { user_id: user.id }).await?;
    }
    Ok(())
}
# }
```

What to know about jobs:

- **`state.dispatch(job)`** puts a job on the queue (a table in your database). A worker picks
  it up a moment later and calls its `handle`.
- **When `handle` fails**, the job is tried again a bit later, up to three times. After that it
  goes to the `failed_jobs` table: `rnx queue:failed` lists those (each with its id), and
  `rnx queue:retry <id>` tries one again (`rnx queue:retry all`: every one).
- **Sending mail from a web page?** Use `state.queue_mail(mail)` there, so the visitor doesn't
  wait for the mail server. A job already runs in the background, so it sends directly.

### The mail

`make:mail` wrote `resources/views/mail/digest.html` and `digest.txt`: the same mail as HTML
(for most mail apps) and as plain text (for the rest). Replace the HTML one:

```html
{% extends "renox/mail/layout.html" %}
{% from "renox/mail/components.html" import button %}
{% block content %}
<p>Hi {{ name }},</p>
<p>Here's what you saved this week:</p>
<ul>
  {% for bookmark in bookmarks %}
  <li><a href="{{ bookmark.url }}">{{ bookmark.title }}</a></li>
  {% endfor %}
</ul>
{{ button(app.url ~ "/bookmarks", "Open your bookmarks") }}
{% endblock %}
```

and the text one:

```text
Hi {{ name }},

Here's what you saved this week:
{% for bookmark in bookmarks %}
- {{ bookmark.title }}: {{ bookmark.url }}
{% endfor %}
{{ app.url }}/bookmarks
```

`name` and `bookmarks` are the values the job passed in `context! { … }`. Renox's mail layout and
`button` are already styled the way mail apps need. [mail.md](mail.md) covers other mail parts,
attachments and notifications.

### The schedule

`make:job` also added a line to the module's `register` method (the one from step 3), which
tells the app about the job. Add the task to it, in `src/app/bookmarks/mod.rs`:

```rust
# fn main() {}
# mod bookmarks {
# pub mod send_digest {
#     use renox::prelude::*;
#     #[derive(serde::Serialize, serde::Deserialize)]
#     pub struct SendDigest { pub user_id: i64 }
#     impl Job for SendDigest {
#         const NAME: &'static str = "send-digest";
#         async fn handle(self, _ctx: JobContext) -> Result { Ok(()) }
#     }
#     pub async fn send_digests(_state: AppState) -> Result { Ok(()) }
# }
# use renox::prelude::*;
# async fn index() -> &'static str { "" }
# pub struct Bookmarks;
use renox::chrono::Weekday;

impl Module for Bookmarks {
    fn name(&self) -> &'static str {
        "bookmarks"
    }

    /// Tells the app about this module's jobs and scheduled tasks.
    fn register(&self, app: &mut Registry) {
        // … the model and the page data, as before
        // Workers can only run jobs they know about.
        app.job::<send_digest::SendDigest>();
        // Mondays at 08:00 (in APP_TIMEZONE), run `send_digests`. The task is
        // named "weekly-digest", for commands like `rnx schedule:run`.
        app.schedule()
            .weekly_on(Weekday::Mon, "08:00", "weekly-digest", send_digest::send_digests);
    }

    fn routes(&self) -> Routes {
        // … as before
#         Routes::new().get("/bookmarks", index)
    }
}
# }
```

Times use `APP_TIMEZONE` from `.env`: `UTC` unless you set one, such as `Europe/Amsterdam`
(summer time included).

You don't have to wait for Monday. `rnx schedule:list` shows the task and when it runs next, and

```bash
rnx schedule:run weekly-digest
```

runs it right now. The running `rnx serve` picks up the jobs it queued.

Where does the mail go? While you develop, `.env` has `MAIL_MAILER=log`: mails are written to
the server's log instead of being sent. Open <http://127.0.0.1:3000/_renox/mail> to see each one
as the recipient would. More on the scheduler in [scheduling.md](scheduling.md), and on jobs in
[queue.md](queue.md).

## 7. Tests and demo data

### A factory and a seeder

A **factory** makes bookmarks filled with made-up data. Make one, for tests and for a database
to click around in:

```bash
rnx make:factory Bookmark --module bookmarks
```

Replace `src/app/bookmarks/bookmark_factory.rs`:

```rust
# fn main() {}
# mod model {
#     use renox::prelude::*;
#     #[derive(Model, serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
#     #[model(table = "bookmarks")]
#     pub struct Bookmark {
#         pub id: i64, pub user_id: i64, pub title: String, pub url: String,
#         pub note: Option<String>, pub created_at: Option<DateTime>, pub updated_at: Option<DateTime>,
#     }
# }
# mod bookmark_factory {
use renox::fake::Fake;
use renox::fake::faker::lorem::en::{Sentence, Word};
use renox::prelude::*;

use super::model::Bookmark;

impl Factory for Bookmark {
    /// A made-up bookmark: a title of 2 to 4 random words and a random address.
    fn definition() -> Self {
        let path: String = Word().fake();
        Bookmark {
            title: Sentence(2..5).fake(),
            url: format!("https://example.com/{path}"),
            ..Default::default()
        }
    }
}
# }
```

Now you can write `Bookmark::factory().count(20).create(&db)` to save twenty fake bookmarks.
`.state(…)` changes each one before saving (for example, to set its owner), and `.make_one()`
gives you one without saving it.

A **seeder** fills the database with starting data. Replace `src/lib.rs`, with a seeder and the
model exported for the tests (files in `tests/` can only use what the library makes `pub`):

```rust
# fn main() {}
# mod app {
#     pub mod home {
#         pub struct Home;
#         impl renox::Module for Home { fn name(&self) -> &'static str { "home" } }
#     }
#     pub mod bookmarks {
#         pub struct Bookmarks;
#         impl renox::Module for Bookmarks { fn name(&self) -> &'static str { "bookmarks" } }
#         pub mod model {
#             use renox::prelude::*;
#             #[derive(Model, serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
#             #[model(table = "bookmarks")]
#             pub struct Bookmark {
#                 pub id: i64, pub user_id: i64, pub title: String, pub url: String,
#                 pub note: Option<String>, pub created_at: Option<DateTime>, pub updated_at: Option<DateTime>,
#             }
#             impl Factory for Bookmark {
#                 fn definition() -> Self { Bookmark { title: "Rust".into(), url: "https://www.rust-lang.org".into(), ..Default::default() } }
#             }
#         }
#     }
# }
use renox::prelude::*;

pub use app::bookmarks::model::Bookmark;

/// The application: its modules, migrations and seeders. `main.rs` runs it,
/// and tests start it with `renox::testing::TestApp`.
pub fn app() -> App {
    App::new()
        // Put the templates, texts and public files inside the program.
        .embed(renox::embedded!())
        // Put the migrations (the files in `migrations/`) inside the program.
        .migrations(renox::migrations!())
        .module(Auth::new().account().redirect_to("/bookmarks"))
        .module(app::home::Home)
        .module(app::bookmarks::Bookmarks)
        // `rnx db:seed`: a demo account with some bookmarks. Running it twice changes nothing.
        .seeder(|state| async move {
            // Already seeded? Then stop.
            if User::find_by_email(&state.db, "demo@example.com")
                .await?
                .is_some()
            {
                return Ok(());
            }
            let demo = User::register(&state.db, "Demo", "demo@example.com", "password123").await?;
            let owner = demo.id;
            // 25 fake bookmarks, all owned by the demo account.
            Bookmark::factory()
                .count(25)
                .state(move |b: &mut Bookmark| b.user_id = owner)
                .create(&state.db)
                .await?;
            Ok(())
        })
}
```

Keep the `mod app;` line at the top of the file. Run `rnx db:seed`, log in as
`demo@example.com` with `password123`, and the list has two pages. (`rnx migrate:fresh --seed`
starts over: it deletes every table, runs the migrations and seeds again.)

### The tests

A **test** is a function that uses your app and checks the result, automatically. Once
written, `cargo test` runs them all in seconds, so you know a change didn't break anything.

`TestApp` starts the whole app in memory for each test, with its own empty database. Requests
go through the same routes, logins and security checks as in a real browser. Make
`tests/bookmarks.rs`:

```rust
# fn main() {}
# mod stash {
#     use renox::prelude::*;
#     #[derive(Model, serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
#     #[model(table = "bookmarks")]
#     pub struct Bookmark {
#         pub id: i64, pub user_id: i64, pub title: String, pub url: String,
#         pub note: Option<String>, pub created_at: Option<DateTime>, pub updated_at: Option<DateTime>,
#     }
#     impl Factory for Bookmark {
#         fn definition() -> Self { Bookmark { title: "Rust".into(), url: "https://www.rust-lang.org".into(), ..Default::default() } }
#     }
#     pub fn app() -> App { App::new().module(Auth::new()) }
# }
use renox::prelude::*;
use renox::testing::TestApp;
use stash::Bookmark;
use std::time::Duration;

/// Makes a user with this email address (and the password "password123").
async fn user(app: &TestApp, email: &str) -> User {
    User::register(app.db(), "Test", email, "password123")
        .await
        .unwrap()
}

/// Someone who isn't logged in can't see the bookmarks.
#[renox::test]
async fn guests_are_sent_to_the_login_page() {
    let app = TestApp::new(stash::app()).await;
    app.get("/bookmarks").await.assert_redirect("/login");
}

/// A logged-in user saves a bookmark, and it's in the database.
#[renox::test]
async fn users_save_bookmarks() {
    let app = TestApp::new(stash::app()).await;
    let ana = user(&app, "ana@example.com").await;
    // Log in as Ana for the requests that follow.
    app.acting_as(&ana);

    // Post the form as htmx would: the answer is the list, with the new bookmark.
    app.htmx()
        .post(
            "/bookmarks",
            &[
                ("title", "The Rust Book"),
                ("url", "https://doc.rust-lang.org/book/"),
            ],
        )
        .await
        .assert_ok()
        .assert_see("The Rust Book");
    app.assert_database_has(
        "bookmarks",
        &[("title", &"The Rust Book"), ("user_id", &ana.id)],
    )
    .await;
}

/// Bad input is refused, and nothing is saved.
#[renox::test]
async fn invalid_bookmarks_are_refused() {
    let app = TestApp::new(stash::app()).await;
    app.acting_as(&user(&app, "ana@example.com").await);

    app.htmx()
        .post(
            "/bookmarks",
            &[("title", ""), ("url", "javascript:alert(1)")],
        )
        .await
        .assert_invalid("title")
        .assert_invalid("url");
    app.assert_database_count("bookmarks", 0).await;
}

/// Ben can't touch Ana's bookmark; Ana can edit and delete it.
#[renox::test]
async fn only_the_owner_edits_and_deletes() {
    let app = TestApp::new(stash::app()).await;
    let ana = user(&app, "ana@example.com").await;
    let owner = ana.id;
    let bookmark = Bookmark::factory()
        .state(move |b: &mut Bookmark| b.user_id = owner)
        .create_one(app.db())
        .await
        .unwrap();
    let edit = format!("/bookmarks/{}/edit", bookmark.id);
    let member = format!("/bookmarks/{}", bookmark.id);

    // Ben gets "403 Forbidden" for both.
    app.acting_as(&user(&app, "ben@example.com").await);
    app.get(&edit).await.assert_forbidden();
    app.delete(&member).await.assert_forbidden();

    app.acting_as(&ana);
    app.put(
        &member,
        &[("title", "Renamed"), ("url", "https://example.com")],
    )
    .await
    .assert_redirect("/bookmarks");
    app.get("/bookmarks").await.assert_see("Bookmark updated."); // the toast
    app.delete(&member).await.assert_redirect("/bookmarks");
    app.assert_database_count("bookmarks", 0).await;
}

/// The Monday mail goes only to people who saved something that week.
#[renox::test]
async fn the_weekly_digest_mails_what_is_new() {
    let app = TestApp::new(stash::app()).await;
    let ana = user(&app, "ana@example.com").await;
    let owner = ana.id;
    Bookmark::factory()
        .count(2)
        .state(move |b: &mut Bookmark| b.user_id = owner)
        .create(app.db())
        .await
        .unwrap();
    user(&app, "ben@example.com").await; // saved nothing: gets no mail

    // Run the task now, then the jobs it queued.
    app.run_scheduled("weekly-digest").await.unwrap();
    assert_eq!(app.run_jobs().await, 2); // a job per user
    app.assert_mail_sent("ana@example.com", "Your week in bookmarks");
    assert_eq!(app.sent_mail().len(), 1);

    // Eight days later nothing is new, so nobody gets mail.
    app.travel(Duration::from_secs(8 * 24 * 60 * 60));
    app.run_scheduled("weekly-digest").await.unwrap();
    app.run_jobs().await;
    assert_eq!(app.sent_mail().len(), 1);
}
```

Run them with `cargo test`. One test that came with `rnx new` now fails: `guests_can_register`
in `tests/home.rs` expects new users to land on `/`. Since step 5 they land on their bookmarks,
so change its `.assert_redirect("/")` to `.assert_redirect("/bookmarks")`.

The test tools you used:

- `cargo test` also runs the tests `rnx new` wrote: every template compiles
  (`assert_views_compile`, the same check as `rnx view:check`), and the models match their tables (`assert_models_match_schema`, the same as `rnx db:check`), so a wrong tag or route name
  fails the build of your CI, not a visitor's page.
- `#[renox::test]` marks a function as a test (an `async` one).
- `acting_as(&user)` logs that user in for the next requests; `htmx()` sends the next request
  the way htmx would.
- `assert_…` methods check the answer: `assert_redirect("/login")`, `assert_see("text")`,
  `assert_forbidden()` (a 403), `assert_invalid("title")` (that field had an error). If a check
  fails, the test fails and tells you what it got instead.
- `assert_database_has` and `assert_database_count` look in the database.
- The test doesn't run the scheduler by itself: `run_scheduled` runs a task now (at the
  travelled time), `run_jobs` runs what it queued, and `sent_mail()` lists the mails, which are
  kept in memory.
- `travel` moves the app's clock forward, so "eight days later" takes no time at all.

[testing.md](testing.md) lists every check, and how to fake mail, events and calls to other
websites.

## 8. Deploy

A Renox app goes on a server as **one file**. The templates, texts, public files and migrations
are all compiled into the program: that's what `.embed(renox::embedded!())` and
`renox::migrations!()` in `src/lib.rs` do. Next to the program you only need a `.env` file
(the settings) and a `storage/` folder (for the SQLite database). The job workers and the
scheduler run inside the same program.

### Build

```bash
rnx build          # a release build, copied to dist/stash
rnx make:deploy    # Dockerfile, .dockerignore, deploy/stash.service, deploy/stash.socket, deploy/litestream.yml, deploy/README.md
```

- `rnx build` makes the optimized program, `dist/stash`.
- `rnx make:deploy` writes ready-made files for the server: a Dockerfile (and a
  `.dockerignore`, which keeps `target/`, `.env` and the database out of the image), the
  systemd service (which starts the app when the server boots, and restarts it if it stops),
  and backups.

Build on the same kind of system as the server (for example, Linux on an Intel/AMD processor),
or build the Docker image. `deploy/README.md` is the full recipe for your app; here is the short
version.

### On a Linux server with systemd

```bash
sudo useradd --system --home /opt/stash stash   # a user that only runs the app
sudo mkdir -p /opt/stash/storage
sudo cp dist/stash /opt/stash/
sudo cp .env.example /opt/stash/.env          # then edit it, below
sudo chown -R stash /opt/stash
sudo cp deploy/stash.service /etc/systemd/system/
sudo systemctl enable --now stash              # start it now, and at every boot
```

The service runs `stash migrate` before each start. So a new version is just: copy the new
program over the old one, then `sudo systemctl restart stash`.

### The production `.env`

```bash
APP_NAME="Stash"
APP_ENV=production
APP_DEBUG=false
APP_URL=https://stash.example.com
# A new key, from `rnx key:generate --show`. Keep it safe: it encrypts sessions.
APP_KEY=base64:...
APP_HOST=127.0.0.1
APP_PORT=3000
# Caddy or nginx on the same machine sends the visitor's address.
TRUSTED_PROXIES=127.0.0.1
# Other Host headers get a 400.
TRUSTED_HOSTS=stash.example.com
APP_TIMEZONE=Europe/Amsterdam
DATABASE_URL=sqlite://storage/app.db

MAIL_MAILER=smtp
MAIL_HOST=smtp.your-provider.com
MAIL_PORT=587
MAIL_ENCRYPTION=starttls
MAIL_USERNAME=...
MAIL_PASSWORD=...
MAIL_FROM_ADDRESS=hello@stash.example.com
```

The important ones:

- **`APP_ENV=production`** turns on production behaviour: the app refuses to start without an
  `APP_KEY`, and lets search engines index it.
- **`APP_DEBUG=false`** hides error details from visitors (they could reveal how your app works).
- **`APP_KEY`** is a secret that protects logins. Make a new one for the server, and never share
  it.
- **`TRUSTED_PROXIES`** says the web server in front (below) may tell the app each visitor's real
  address. Without it, every visitor looks the same, and the protection against password
  guessing would lock everyone out at once.
- **`APP_URL`** is the address people use; links in mails are built from it.

In front of the app, put a web server that handles HTTPS (the padlock in the browser). With
Caddy, this is the whole configuration, and it gets the certificate by itself:

```text
stash.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

### Restarts without refused connections

While the app restarts (a second or two), visitors would get an error. With systemd's
**socket activation**, systemd holds the port and makes visitors wait until the app is back:

```bash
sudo cp deploy/stash.socket /etc/systemd/system/ && sudo systemctl daemon-reload
sudo systemctl stop stash                 # it holds the port; the socket takes it over
sudo systemctl enable --now stash.socket
sudo systemctl start stash
```

From then on, `sudo systemctl restart stash` makes connections wait instead of failing.

> [!WARNING]
> For a moment during a restart, the old program runs with the new database. So add new columns
> as optional (nullable) or with a default value, and only remove a column once no running
> version uses it.

### Backups

The whole database is one file, `storage/app.db`. **Litestream** copies every change to cloud
storage (S3, R2, MinIO) as it happens: install it, fill in your bucket in
`deploy/litestream.yml`, and copy that file to `/etc/litestream.yml`. `deploy/README.md` has the
steps, including restoring onto a new server.

For a one-off copy while the app runs, use `sqlite3 storage/app.db ".backup backup.db"`. Never
plain `cp`: it can copy the file halfway through a write.

> [!IMPORTANT]
> Keep `APP_KEY` with your backups. Without it, logins, sessions and signed links stop working
> on the restored server.

`GET /health` answers `200` while the database is reachable: point an uptime monitor at it.
[operations.md](operations.md) covers timeouts, failed jobs, logs, error reports and running
several servers.

## Where to go next

You've built a complete app, and used most of what a typical app needs. From here:

- [The cheat sheet](../CHEATSHEET.md): every common task in a few lines.
- [examples/bikeshop](../examples/bikeshop): a whole business (a shop with a checkout, rentals,
  a workshop, an admin area, translations), where every page explains the Renox features it
  uses; start on its `/about/pages`.
- The guides: [routing](routing.md), [validation](validation.md), [views and the UI
  kit](ui.md), [mail and notifications](mail.md), [the queue](queue.md),
  [scheduling](scheduling.md), [relations](relations.md), [testing](testing.md) and
  [production](operations.md).
