# Laravel → Renox

This guide is for people who know Laravel and want to work the same way in Rust. It takes each
Laravel idea in turn and shows what it's called in Renox, and *why* some things look different.

You already know how Laravel works: generators, migrations, models, form requests, queues, mail,
a login out of the box, pages rendered on the server. Renox gives you most of that, under names
you'll recognise. Once you know *why* a thing is different, you can stop looking for the Laravel
version of it.

This guide assumes you have read the README's quick start. Each section links to the guide with
the details. [CHEATSHEET.md](../CHEATSHEET.md) has one example of every pattern, and the
[Laravel parity review](audit/2026-10-laravel-parity.md) rates every Laravel and Filament
feature against Renox.

### Words you'll meet

You know the Laravel words. These are the Rust and Renox words this guide uses:

| Word | What it means |
|---|---|
| **compile** | Turn the source code into a program before it runs. Rust checks a lot of mistakes at this step. |
| **compile time** / **run time** | "While building the program" / "while the program is running". |
| **crate** | A Rust package, like a Composer package. `renox` is one; your app is one too. |
| **binary** | The program file the build makes. You copy it to the server and run it. |
| **struct** | A type with named fields, like a PHP class with only typed properties. |
| **enum** | A type that is one of a fixed list of values (`Draft` or `Published`). |
| **trait** and **`impl`** | A trait is like a PHP interface. `impl Trait for Type` makes a type implement it. |
| **derive** | `#[derive(Model)]` asks the compiler to write the trait's code for you, from the struct. |
| **macro** | Code that writes code at compile time. Names end in `!` (`context! { … }`) or come in `#[…]`. |
| **handler** | The `async fn` a route calls. It's the controller method. |
| **extractor** | A handler argument whose type knows how to read itself from the request (`Session`, `Found<Post>`). |
| **`async` / `.await`** | `async fn` can pause while waiting (for the database, say); `.await` is where it waits. |
| **`Result` and `?`** | A function returns `Ok(value)` or `Err(error)`. `?` after a call means "if it failed, return the error now". |
| **`Option`** | A value that may be missing: `Some(x)` or `None`. Rust's way of writing "nullable". |
| **closure** | An inline function: `\|user\| user.is_admin()`. Like a PHP arrow function. |
| **generic** (`<T>`) | A type with a slot for another type: `Vec<String>` is a list of strings. |
| **feature** | A switch in `Cargo.toml` that turns an optional part of a crate on, like `postgres`. |

## The big differences, on one page

These are the ideas that make Renox feel different from Laravel. Each one is short; the rest of
the guide shows them in code.

### Mistakes show up before anything runs

**It's compiled.** Laravel finds things at run time: a method name in a route file, a column
through `__get`, a class through the container. Renox finds them at compile time.

So a typo in a handler's name, a column type that doesn't match, or a form field you forgot to
declare: the compiler tells you before anything runs.

The price is compile time. `rnx serve` rebuilds on save, usually in a few seconds. There is also
some ceremony: types are written out.

### Types instead of magic

**Types instead of magic.** A model is a struct with one field per column.

A form is a struct with one field per input. So there is no `$fillable` and no mass assignment:
a field the struct doesn't declare can't reach the database.

`$casts` become field types: `bool`, `Json<T>`, an enum, `NaiveDate`, `Encrypted<T>`.

### Extractors and fields, not the container and facades

**Extractors instead of the container.** A Laravel controller method asks the container for
`Request $request`, `Post $post`, `Auth::user()`.

A Renox handler lists what it needs as arguments. Each argument's type knows how to pull itself
out of the request: `Session`, `AuthUser`, `Valid<StorePost>`, `Found<Post>`,
`State<AppState>`.

There are no facades either. The database, cache, queue, mailer and storage are fields of
`AppState` (the app's shared state, which every handler can ask for).

### No lazy relations

**No runtime reflection, so no lazy relations.** `$post->author` can't quietly run a query,
because Rust has no `__get`.

Renox makes that a feature. You get a method for one related row, and *loaders* that fetch the
related rows of a whole page in one query. So N+1 can't happen by accident. (And `/_renox/debug`
flags a statement run three times or more.)

### Migrations are plain SQL

**Migrations are SQL files.** There is no schema builder. A migration is
`migrations/2026…_create_posts.up.sql` and `.down.sql`, plus `.postgres.up.sql` when PostgreSQL
needs different SQL.

You see exactly what runs, and the same migrations are compiled into the binary.

### One file to deploy

**One binary.** `rnx build` makes one file. It holds the web server, the queue workers, the
scheduler, the CLI (your `artisan`), the migrations, the templates, the translations and the
public files.

No PHP-FPM, no Nginx config for PHP, no Supervisor, no cron entry, no Redis. SQLite is the
default database; PostgreSQL is a feature flag away.

### htmx and Alpine.js for the front end

**htmx and Alpine.js, and live components, instead of Livewire.** Pages are rendered on the
server. htmx swaps parts of them, and handlers return just a block of a template
(`.fragment("list")`). For a part of a page with state and several actions, a live component is
a Rust struct with a method per action (see [live.md](live.md)). Inertia has no counterpart.

Validation errors land next to the right inputs, with no JavaScript to write per form.

### Templates in MiniJinja

**MiniJinja instead of Blade.** It uses Jinja2 syntax (`{{ }}`, `{% if %}`, `{% extends %}`,
`{% block %}`), auto-escaped. Laravel's helpers are there as functions: `route()`, `old()`,
`error()`, `csrf_field()`, `t()`, `can()`, `asset()`.

### Errors are ordinary return values

**Errors are values.** Handlers return `Result<T>`. `?` passes an error up, and Renox turns it
into the right page:

- a 404 for `find_or_404`;
- a 403 for `authorize`;
- a redirect back with errors for invalid input;
- a 500 with the error page otherwise.

`abort(StatusCode, "…")` is Laravel's `abort()`.

## Projects and commands

`rnx new shop` makes an app. Here is where Laravel's files live in Renox:

| Laravel | Renox |
|---|---|
| `composer.json`, `vendor/` | `Cargo.toml` (one dependency: `renox`) |
| `artisan` | `rnx` during development; the app binary itself in production |
| `public/index.php`, `bootstrap/app.php`, service providers | `src/main.rs` (one line) and `src/lib.rs` (`pub fn app() -> App`) |
| `routes/web.php`, `app/Http/Controllers/*` | a module per area: `src/app/products/mod.rs` with its routes and handlers |
| `app/Models/Product.php` | `src/app/products/model.rs` (struct, factory, policy) |
| `database/migrations/*.php` | `migrations/*.up.sql` / `*.down.sql` |
| `resources/views/*.blade.php` | `resources/views/*.html` |
| `lang/es.json` | `resources/lang/es.json` |
| `config/*.php` | `.env`, read once into `Config` |
| `storage/` | `storage/` (the SQLite file, uploads, the maintenance flag) |
| `tests/Feature/*` | `tests/*.rs` with `TestApp` |

The app is a library (`src/lib.rs`) plus a one-line binary. That way, tests can boot the same
app the binary runs.

The binary is also the app's CLI, like `artisan`. Renox's commands and yours are compiled into
it, because migrations, jobs and commands are code. While you develop, `rnx <command>` runs the
same thing through `cargo run`.

```bash
rnx serve                         # php artisan serve + Vite's reload: rebuilds and reloads on save
rnx make:module products --resource --fields "name:string price:money active:bool"
                                  # make:model -mfc + resource controller + views + tests
rnx make:model Product --module products --migration
rnx make:migration add_sku_to_products
rnx make:migration --auto add_sku     # SQL written from the models' changes (db:diff); rnx db:check compares models and tables
rnx make:job SendReceipt --module products   # also make:mail, make:notification, make:policy,
                                             # make:command, make:event, make:rule, make:test, …
rnx migrate                       # migrate:rollback, migrate:fresh --seed, migrate:status, db:seed
rnx route:list
rnx db:shell                      # the closest thing to tinker (see "What Renox doesn't have")
rnx queue:failed                  # queue:retry <id|all>, queue:forget, queue:flush
rnx down --secret s3cret          # up
rnx build                         # one release binary in dist/
./dist/shop migrate               # in production: the binary is the CLI
```

## Routes and controllers

There are no controller classes. A **module** groups an area's routes, and the handlers are
plain `async fn`s next to them.

Here is a small product area in Laravel:

```php
// routes/web.php
Route::get('/products', [ProductController::class, 'index'])->name('products.index');
Route::get('/products/{product}', [ProductController::class, 'show'])->name('products.show');
Route::post('/products', [ProductController::class, 'store'])->name('products.store')
    ->middleware('auth');
```

And the same in Renox, with the model, the module, the handlers and the form in one file:

```rust
use renox::prelude::*;

/// The model: one field per column of the `products` table.
#[derive(Model, serde::Serialize, Default)]
#[model(table = "products")]
struct Product {
    id: i64,
    name: String,
    price: i64,
}

/// The module: Renox's stand-in for a route file plus a controller.
pub struct Products;

impl Module for Products {
    fn name(&self) -> &'static str {
        "products"
    }

    /// The module's routes, like `routes/web.php` for this area.
    fn routes(&self) -> Routes {
        let public = Routes::new()
            .get("/products", index).name("products.index")
            .get("/products/{id}", show).name("products.show"); // {id}, not :id
        let members = Routes::new()
            .post("/products", store).name("products.store")
            .require_auth(); // the `auth` middleware, for the routes added before it
        public.merge(members)
    }
}

/// `State<Db>` hands the handler the database; there is no global connection.
async fn index(State(db): State<Db>) -> Result<View> {
    let products = Product::query().order_by(Product::NAME).get(&db).await?;
    Ok(view("products/index.html", context! { products }))
}

// Route model binding: a missing row (or `/products/abc`) is a 404.
async fn show(Found(product): Found<Product>) -> View {
    view("products/show.html", context! { product })
}

/// The form request: the fields the form sends, and their rules.
#[derive(serde::Deserialize, Validate)]
struct ProductForm {
    #[validate(required, max = 100)]
    name: String,
    #[validate(min = 0)]
    price: i64,
}

/// `Valid<ProductForm>` checks the form first; the handler only runs when it passes.
async fn store(State(db): State<Db>, Valid(form): Valid<ProductForm>) -> Result<Redirect> {
    let product = Product { name: form.name, price: form.price, ..Default::default() };
    let product = Product::create(&db, product).await?;
    Redirect::route("products.show", &[&product.id]) // redirect()->route(...)
}

/// The app: Renox's auth module plus ours (like registering service providers).
pub fn app() -> App {
    App::new().module(Auth::new()).module(Products)
}
```

What happens here, in plain words:

- `routes()` builds two lists of routes. `public` is open to everyone. `members` holds the
  `store` route and then calls `.require_auth()`, so only logged-in users reach it. `merge`
  joins the two lists.
- `index` loads all products, sorted by name, and renders a template with them.
- `show` gets its product from `Found<Product>`. That is route model binding: Renox reads the
  `{id}` from the address and loads the row.
- `store` gets a checked form from `Valid<ProductForm>`, saves a new product, and redirects to
  its page by route name.

Other things you'll look for:

- `Route::resource` is `.resource("/products", "products", Resource::new().index(…).store(…)…)`
  with the same seven names. `rnx make:module products --resource` writes all of it.
- `Route::prefix('admin')->name('admin.')->group(…)` is `.group("/admin", "admin.", routes)`.
  `Route::domain`, `Route::fallback`, `Route::view` and `Route::redirect` keep their names.
- `$request->input('q')` becomes a typed extractor:
  - `Query<Search>` for the query string;
  - `Form<T>` or `Json<T>` for a body (or `Valid<T>` to validate it);
  - `Path<i64>` for a parameter;
  - `ClientIp` for `$request->ip()`;
  - `Htmx` for htmx headers.
- A handler returns anything that becomes a response: `View`, `Redirect`, `Json<T>`, a
  `String`, a `StatusCode`, `Download`, or a tuple adding headers, cookies or a `Toast`.

Details: [routing.md](routing.md).

## Middleware and guards

Laravel's named middleware (`auth`, `verified`, `can:`, `throttle`) are methods on `Routes`.

Your own middleware is an `async fn`, wrapped with axum's `from_fn`. (axum is the web library
Renox is built on.) Then:

- `App::layer` adds it to every route (Laravel's global middleware);
- `Routes::route_layer` adds it to some routes.

```rust
use renox::prelude::*;
use renox::axum::extract::Request;
use renox::axum::middleware::{Next, from_fn};
use std::time::Duration;

// Laravel's handle($request, Closure $next)
async fn stamp(user: Option<AuthUser>, req: Request, next: Next) -> Response {
    // Let the request go on to the route, and wait for its response.
    let mut res = next.run(req).await;
    let who = if user.is_some() { "member" } else { "guest" };
    res.headers_mut().insert("x-visitor", who.parse().unwrap());
    res
}

/// Three groups of routes, each with its own guard, merged into one.
fn routes() -> Routes {
    let orders = Routes::new()
        .get("/orders", || async { "your orders" })
        .require_verified(); // ['auth', 'verified']
    let checkout = Routes::new()
        .post("/checkout", || async { "paid" })
        .require_auth()
        .throttle(10, Duration::from_secs(60)); // throttle:10,1
    let admin = Routes::new()
        .get("/admin", || async { "admin" })
        .require_role("admin")        // role:admin (the Permissions module)
        .route_layer(from_fn(stamp)); // ->middleware(Stamp::class)
    orders.merge(checkout).merge(admin)
}

fn app() -> App {
    App::new().layer(from_fn(stamp)) // every route of the app's modules
}
```

What this does:

- `stamp` is a middleware. It lets the request through with `next.run(req).await`, then adds
  an `x-visitor` header to the response: `member` when someone is logged in, `guest` when not.
- `orders` is only for users with a verified email. `checkout` is for logged-in users, at most
  10 times a minute. `admin` is for users with the `admin` role, and also runs `stamp`.
- `app()` runs `stamp` on every route of the app's modules.

More guards on `Routes`:

| Renox | Laravel |
|---|---|
| `.guest_only()` | `guest` |
| `.require_gate("…")` | `can:` |
| `.require_permission("…")` | a permission check |
| `.require_ability("…")` | Sanctum abilities |
| `.require_password_confirmed()` | `password.confirm` |
| `.without_csrf()` | `VerifyCsrfToken::$except` |
| `.cors(&[…])` | CORS |
| `.throttle_by("api")` with `App::rate_limiter` | `RateLimiter::for` |

`route:list` shows each route's guards.

## Requests and validation

A form request is a struct. Its fields say what the form sends, and their types. Its rules say
what is allowed.

Put `Valid<T>` in a handler's arguments, and it:

1. reads the body (form, JSON or multipart) or the query string;
2. runs the rules;
3. only calls the handler when everything passes.

When something fails:

- a plain form post goes back, with the errors and the old input (never passwords);
- an htmx or JSON request gets a 422 with the errors, and the bundled script places them next
  to the inputs.

Here is a form request in Laravel:

```php
class StorePost extends FormRequest
{
    public function authorize(): bool { return $this->user()->hasRole('editor'); }

    protected function prepareForValidation(): void
    {
        $this->merge(['title' => trim($this->title)]);
    }

    public function rules(): array
    {
        return [
            'title' => 'required|max:200|unique:posts,title',
            'tags.*' => 'max:30',
            'published_on' => 'nullable|date',
        ];
    }
}
```

And in Renox:

```rust
use renox::chrono::NaiveDate;
use renox::prelude::*;
use renox::validation::FormContext;
use serde::Deserialize;

/// The form's fields. Their types already check a lot (a date must be a date).
#[derive(Deserialize)]
struct StorePost {
    title: String,
    #[serde(default)]
    tags: Vec<String>,            // tags=a&tags=b; nothing chosen → []
    published_on: Option<NaiveDate>, // nullable|date: the type does it
}

impl Validate for StorePost {
    /// prepareForValidation: tidy the input before the rules run.
    fn prepare(&mut self) {
        self.title = self.title.trim().to_string();
    }

    /// authorize(): may this user send the form at all?
    async fn authorize(&self, form: &FormContext<'_>) -> Result<bool> {
        Ok(form.user.is_some_and(|u| u.has_role("editor"))) // false → 403
    }

    /// rules(): one chain of rules per field.
    fn rules(&self, v: &mut Validator) {
        v.field("title", &self.title).required().max(200).unique("posts", "title");
        v.each("tags", &self.tags, |tag| tag.max(30));
    }
}

/// By the time this runs, the form is valid.
async fn store(Valid(post): Valid<StorePost>) -> Result<Redirect> {
    let _ = (post.title, post.tags, post.published_on);
    Ok(Redirect::to("/posts"))
}
```

The three parts map one to one:

- `prepare` is `prepareForValidation`: it trims the title.
- `authorize` is `authorize()`: only editors may post. `false` gives a 403.
- `rules` is `rules()`. `v.field(…)` starts the rules for one field; `v.each(…)` applies a rule
  to every item of a list (Laravel's `tags.*`).

`nullable|date` has no rule at all: `Option<NaiveDate>` already says "may be empty, and must be a
date".

### Rules as attributes

When a form needs only rules, derive them, as attributes:

```rust
use renox::prelude::*;

/// `#[derive(Validate)]` writes the `rules` method from the `#[validate(…)]` lines.
#[derive(serde::Deserialize, Validate)]
struct ContactForm {
    #[validate(required, max = 100, label = "Your name")]
    name: String,
    #[validate(required, email)]
    email: String,
}
```

Each `#[validate(…)]` line is the rules for the field under it. `label` is the name used in
error messages.

> [!TIP]
> A derived form can still have `prepare`/`authorize`/`after`: add `#[validate(hooks)]` and
> write `impl ValidateHooks` for it.

### Rule names

- `Rule::unique('users')->ignore($id)->where(…)` is
  `.unique("users", "email").ignore(id).where_eq("team_id", team)`. `exists:` is `.exists(…)`.
- `in:`/`not_in:`/`regex:` are `one_of`, `none_of`, `matches`.
- `messages()` and `attributes()` are `.message(…)` and `.label(…)` (or `renox.validation.*` in
  a lang file).
- `after()`/`withValidator` is `async fn after(&self, form, errors)`, which may query the
  database.
- `ValidationException::withMessages` is `Err(ValidationError::new(errors).into())`.
- Most of Laravel's rules exist under the same name, in snake_case. integer, numeric, boolean,
  array and date are the field's type, not a rule.

Details: [validation.md](validation.md).

## Eloquent → models

### A model

An Eloquent model in Laravel:

```php
class Post extends Model
{
    use SoftDeletes;
    protected $fillable = ['title', 'status', 'tags'];
    protected $hidden = ['api_secret'];
    protected $casts = ['status' => Status::class, 'tags' => 'array',
                        'featured' => 'boolean', 'api_secret' => 'encrypted'];
}
```

The same model in Renox:

```rust
use renox::db::{Encrypted, Json};
use renox::prelude::*;

/// An enum column: stored as text, read back as one of these values.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Default)]
enum Status {
    #[default]
    Draft,     // stored as the text "draft"
    Published,
}

/// The `posts` table, one field per column. `soft_deletes` is Laravel's SoftDeletes.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "posts", soft_deletes)]
struct Post {
    id: i64,                  // the key is always `id`; 0 = not saved yet
    user_id: i64,
    title: String,
    status: Status,           // an enum cast
    tags: Json<Vec<String>>,  // 'array' (TEXT on SQLite, JSONB on PostgreSQL)
    featured: bool,           // 'boolean'
    #[serde(skip)]            // $hidden: never reaches a template or JSON
    api_secret: Encrypted<String>, // 'encrypted': sealed with APP_KEY on save
    created_at: Option<DateTime>,  // filled on save, like Laravel's timestamps
    updated_at: Option<DateTime>,
    deleted_at: Option<DateTime>,  // soft deletes
}
```

The struct is the whole definition. There is nothing to keep in sync except the migration.

Each `$casts` entry became a field type, and `$hidden` became `#[serde(skip)]`. (serde is the
library that turns values into JSON and template data; `skip` leaves the field out.)

> [!WARNING]
> Table names are not pluralised. Say `table = "posts"`. Without it, the struct `Post` maps to
> a table `post`.

The key's type is the `id` field's type:

| Field type | Laravel |
|---|---|
| `i64` | an auto-increment id |
| `Ulid` | `HasUlids` |
| `Uuid` | `HasUuids` (the `uuid` feature) |
| `String` | a string key |

**Accessors** are methods (`fn excerpt(&self) -> String`). A **mutator** is the `saving` hook
(below), or plain code before `save`.

Values a template needs that aren't columns go in one of two places:

- a struct made for the view, with `#[serde(flatten)]` for the model (see "Relations" below);
- a `#[model(skip)]` field on the model.

### Queries

The query builder reads like Eloquent's. One difference: every terminal method (the one that
runs the query, like `get` or `count`) takes the database, or a transaction, as an argument.
There is no global connection.

```rust
use renox::db::Query;
use renox::prelude::*;
# #[derive(DbEnum, Debug, Clone, Copy, PartialEq, Default)]
# enum Status { #[default] Draft, Published }
# #[derive(Model, serde::Serialize, Default, Clone)]
# #[model(table = "posts", soft_deletes)]
# struct Post {
#     id: i64, user_id: i64, title: String, status: Status, views: i64,
#     created_at: Option<DateTime>, updated_at: Option<DateTime>, deleted_at: Option<DateTime>,
# }

impl Post {
    /// A local scope (scopePublished) is a function returning a query.
    fn published() -> Query<Post> {
        Post::query().where_(Post::STATUS.eq(Status::Published))
    }
}

/// A tour of the query builder. `db` is passed to every method that runs SQL.
async fn queries(db: &Db) -> Result {
    let latest = Post::published().latest().limit(10).get(db).await?; // ->latest()->take(10)->get()
    let post = Post::find_or_404(db, 1).await?;                        // findOrFail → 404 page
    let popular = Post::query().where_(Post::USER_ID.eq(7)).where_(Post::VIEWS.gt(100)).count(db).await?; // typed columns: checked at compile time
    let titles: Vec<String> = Post::published().pluck(db, Post::TITLE).await?;
    let page = Post::published().paginate(db, 1, 20).await?;          // ->paginate(20)
    Post::query().where_(Post::USER_ID.eq(7)).update(db, &[("status", &Status::Draft)]).await?; // mass update
    // Change one model and save it.
    let mut post = post;
    post.title = "New title".into();
    post.save(db).await?;   // UPDATE, sets updated_at
    post.delete(db).await?; // soft delete; Post::query().with_trashed() / only_trashed()
    let _ = (latest, popular, titles, page);
    Ok(())
}
```

Each line has its Laravel twin in the comment next to it. `published()` is a local scope: a
plain function that returns a query you can keep adding to.

More from Eloquent and the `DB` facade:

- **Global scopes** (for tenancy) are `#[model(default_scope = "team_only")]`: a function that
  gets every query of the model first. `unscoped()` skips it.
- **Raw SQL** is `renox::db::sql("… ? …").bind(v)`. Read it with `fetch_all`, `scalar`, or
  `fetch_as::<T>()` into a `#[derive(FromRow)]` struct.
- **Transactions** are `db.begin()` / `tx.commit()` or `db.transaction(…)`, with savepoints and
  retries.
- `DB::listen` is `renox::db::capture_queries`.

### Relations

In Laravel, you eager-load relations like this:

```php
$posts = Post::with(['author', 'comments'])->latest()->paginate(20);
```

In Renox, you call a **loader** per relation:

```rust
use renox::db::relations::{belongs_to, has_many};
use renox::prelude::*;

/// An author has many posts.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "authors")]
struct Author { id: i64, name: String }

/// A post belongs to an author and has many comments.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "posts")]
struct Post { id: i64, author_id: i64, title: String }

/// A comment belongs to a post.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "comments")]
struct Comment { id: i64, post_id: i64, body: String }

impl Post {
    /// `$post->author`, for one post: one visible query.
    async fn author(&self, db: &Db) -> Result<Option<Author>> {
        Author::find(db, self.author_id).await
    }
}

/// What the template gets for each post.
#[derive(serde::Serialize)]
struct PostCard {
    #[serde(flatten)]
    post: Post,
    author: Option<Author>,
    comments: Vec<Comment>,
}

/// `with(['author', 'comments'])`: one query per relation for the whole page.
async fn index(State(db): State<Db>, Page(page): Page) -> Result<View> {
    let posts = Post::query().order_by_desc(Post::ID).paginate(&db, page, 20).await?;
    // One query for all the page's authors, one for all their comments.
    let mut authors = belongs_to::<Author, _, _>(&db, &posts.items, |p| p.author_id).await?;
    let mut comments = has_many(&db, &posts.items, Comment::query(), "post_id", |c| c.post_id).await?;
    // Put each post together with its author and comments.
    let cards = posts.map(|post| PostCard {
        author: authors.remove(&post.author_id),
        comments: comments.remove(&post.id).unwrap_or_default(),
        post,
    });
    Ok(view("posts/index.html", context! { posts => cards }))
}
```

Step by step:

1. `paginate` loads one page of posts.
2. `belongs_to` loads the authors of all those posts in **one** query, keyed by author id.
3. `has_many` loads the comments of all those posts in **one** query, grouped by post id.
4. `posts.map` builds a `PostCard` per post: the post's fields (thanks to `flatten`), plus its
   author and its comments.

It is more typing than `with()`, and that is the point: the queries are where you can see them.

The other relations:

| Laravel | Renox |
|---|---|
| many-to-many (`belongsToMany`) | a `Pivot` (`attach`, `detach`, `sync`, `toggle`, pivot columns) |
| `withCount` / `withSum` | `count_many` / `sum_many` |
| `whereHas` | `where_has` |
| `morphTo` / `morphMany` | `Morph` |
| `hasManyThrough` | `has_many_through` |

Details: [relations.md](relations.md).

### Model events, factories and seeders

**Model events.** `creating`/`saved`/`deleting` observers are `#[model(hooks)]` plus
`impl ModelHooks`, with the methods `saving`, `saved`, `deleting` and `deleted`.

Like Laravel's, they run for single-model calls (`save`, `create`, `delete`), and not for bulk
query updates.

**Factories and seeders.** A factory is `impl Factory` on the model. States are plain
functions. Seeders are closures.

```rust
use renox::fake::{Fake, faker::lorem::en::Word};
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "products")]
struct Product { id: i64, name: String, price: i64 }

/// The factory: how to make one product with fake data.
impl Factory for Product {
    fn definition() -> Self {
        Product { name: Word().fake(), price: (5_000..50_000).fake(), ..Default::default() }
    }
}

/// A state: a function that changes a product the factory made.
fn premium(p: &mut Product) {
    p.price = 250_000; // Laravel's ->state([...])
}

async fn in_a_test(db: &Db) -> Result {
    let draft = Product::factory().make_one();             // Product::factory()->make()
    let saved = Product::factory().create_one(db).await?;  // ->create()
    // Three premium products, named "Coffee 1", "Coffee 2" and "Coffee 3".
    let three = Product::factory()
        .count(3)
        .state(premium)
        .sequence(|i, p| p.name = format!("Coffee {}", i + 1))
        .create(db)
        .await?;
    let _ = (draft, saved, three);
    Ok(())
}

fn seeders(app: App) -> App {
    // DatabaseSeeder: `rnx db:seed` or `rnx migrate:fresh --seed`
    app.seeder(|state| async move {
        Product::factory().count(50).create(&state.db).await?;
        Ok(())
    })
}
```

- `definition` is the factory's `definition()`: a word for the name, a price between 5,000 and
  50,000.
- `make_one` builds a product without saving it; `create_one` saves it too.
- `sequence` changes each product by its position (`i` starts at 0).
- `seeders` registers a seeder that creates 50 products. It runs on `rnx db:seed`.

## Blade → MiniJinja

A page in Blade:

```blade
@extends('layouts.app')
@section('content')
  <h1>{{ $title }}</h1>
  @foreach ($products as $product)
    <a href="{{ route('products.show', $product) }}">{{ $product->name }}</a>
  @empty
    <p>No products.</p>
  @endforeach
  @can('create', App\Models\Product::class) … @endcan
  <form method="POST" action="{{ route('products.store') }}">
    @csrf
    <input name="name" value="{{ old('name') }}">
    @error('name') <p>{{ $message }}</p> @enderror
  </form>
@endsection
```

The same page in MiniJinja:

```html
{% extends "layouts/app.html" %}
{% block content %}
  <h1>{{ title }}</h1>
  {% for product in products %}
    <a href="{{ route('products.show', product.id) }}">{{ product.name }}</a>
  {% else %}
    <p>No products.</p>
  {% endfor %}
  {% if can('create-products') %} … {% endif %}
  <form method="post" action="{{ route('products.store') }}">
    {{ csrf_field() }}
    <input name="name" value="{{ old('name') }}">
    <p class="error" data-error-for="name">{{ error('name') }}</p>
  </form>
{% endblock %}
```

The differences to notice:

- Variables have no `$`, and fields use a dot: `product.name`.
- `@empty` is `{% else %}` inside the `for`.
- `route()` takes the id (`product.id`), not the model.
- The error slot has `data-error-for="name"`, so Renox's script can fill it in after an htmx
  request too.

The rest of Blade:

| Blade | MiniJinja in Renox |
|---|---|
| `{{ $x }}`, `{!! $x !!}` | `{{ x }}` (escaped), `{{ x \| safe }}` |
| `@include('partials.x')` | `{% include "partials/x.html" %}` |
| `@auth`, `auth()->user()->name` | `{% if auth.check %}`, `auth.user.name` |
| `@method('PUT')` | `{{ method_field('PUT') }}` |
| `__('shop.welcome', ['name' => $n])` | `t('shop.welcome', name=n)` |
| `@class([...])`, `@push`/`@stack`, `@once` | `class_names(…)`, `{% call push('scripts') %}` / `stack('scripts')`, `once('key')` |
| `@vite(...)` | `{{ renox_head() }}` (htmx, Alpine, renox.js) and `asset('app.css')` (hashed URL) |
| `View::share` / view composers | `App::share(key, \|ctx\| async { … })` |
| `<x-alert type="error">…</x-alert>` | `<rx-alert kind="error">…</rx-alert>`, or your own `<app-alert>` (`components/alert.html`) |

### Components

Components are written as tags, as Blade components are: `<rx-input name="email" label="Email"
required/>`, `<rx-table :rows="products">`, and `rx-if` / `rx-else` / `rx-for` on any element.
Your own components are `<app-*>` files that start with `<rx-props>`
(`rnx make:component price_tag` writes one). They still see `old()`, `error()`, `t()`, `auth`
and `csrf_field()`.

The tags become MiniJinja macro calls when a template loads, and a wrong tag or attribute fails
with the file, the line and "did you mean"; `rnx view:check` checks every template at once.
The macros underneath (`renox/ui.html`) still work for what has no tag.

Renox ships a whole kit of them: inputs, selects, date pickers, repeaters, sheets, tables,
navigation. That's what Breeze's Blade components and Filament's forms give you
([ui.md](ui.md#components-as-tags)).

Views get the data you pass with `context! { … }`.

> [!IMPORTANT]
> Anything you pass to a view must implement `serde::Serialize`. Add it to the struct's
> `#[derive(…)]`.

Details: [ui.md](ui.md).

### Livewire → live components

A Livewire component is a class with public properties and methods, and a view. A Renox live
component is a struct with serde fields (the state), methods marked `#[live(action)]`, and a
view. The browser holds a signed snapshot of the state and morphs the answer into the page.

| Livewire | Renox |
|---|---|
| `wire:click="toggle(3)"` | `rx-click="toggle(3)"` |
| `wire:submit="save"` | `rx-submit="save"` |
| `wire:model`, `.live`, `.blur` | `rx-model`, `rx-model.live`, `rx-model.blur` |
| `$this->validate()` | `ctx.validate(&value).await?` |
| `$this->dispatch('saved')` | `ctx.dispatch("saved", json!({}))` |
| `$this->redirect('/x')` | `ctx.redirect("/x")` |
| `Livewire::test(Counter::class)` | `app.live(Counter::default())` |

Differences to know: the snapshot is signed, not encrypted (no secrets in fields); there is no
nesting of components; a call without JavaScript does nothing. The guide:
[live.md](live.md). The bike shop's checklist is written both ways:
[`/about/htmx`](../examples/bikeshop/src/app/about/htmx.rs) and
[`/about/htmx/live`](../examples/bikeshop/src/app/about/live.rs).

### Inertia → htmx

The handler that renders a page can also answer an htmx request with just one block of it:

```rust
use renox::prelude::*;

/// One handler for both: htmx gets a block of the page, a normal post gets a redirect.
async fn add_todo(htmx: Htmx) -> Result<Response> {
    if htmx.request {
        // {% block list %} of the page, and a toast.
        let list = view("todos/index.html", context! {}).fragment("list");
        return Ok((Toast::success("Added."), list).into_response());
    }
    Ok(Redirect::to("/todos").into_response())
}
```

`htmx.request` is true when htmx sent the request. Then the handler renders only the
`{% block list %}` of the page, and shows a toast ("Added.") next to it. A normal form post
gets a redirect instead.

More htmx tools:

- `.also("count")` adds out-of-band blocks (other parts of the page to update at the same time).
- `HxRedirect`, `HxRefresh`, `HxTrigger`, `HxRetarget` are the response headers.
- `<form data-live-validate>` is Precognition.
- Alpine.js covers client-side state.

The bike shop's `/about/htmx` page ([src/app/about/htmx.rs](../examples/bikeshop/src/app/about/htmx.rs))
has modals, inline editing, infinite scroll and tabs, live.

## Auth: Breeze, Sanctum, gates and policies

`Auth::new()` is a module with Breeze's pages built in: login, registration, password reset,
email verification, and an account page. They use the kit, and you can override each one by
file. Sanctum's tokens are part of it too.

For what Breeze and Jetstream scaffold around those pages, `rnx new desk --starter` writes the
starter kit:

- a sidebar layout with the notification bell, and the kit's tab bar on phones;
- a public home page and branded sign-in pages, made of page patterns the app owns
  (`resources/views/components/` and `public/patterns.css`: a hero, section headings,
  feature cards, a figures strip, a summary with totals, steps, empty states);
- a dashboard with a welcome card and stat cards;
- roles (`admin`, `member`);
- a users page where admins change roles;
- the activity log;
- a `users:admin` command for the first admin;
- a seeder, and the tests for all of it.

```rust
use renox::auth::Permissions;
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "posts")]
struct Post { id: i64, user_id: i64, title: String }

/// The policy: who may do what with a post.
impl Policy for Post {
    fn allows(&self, user: &User, ability: &str) -> bool {
        match ability {
            "update" | "delete" => self.user_id == user.id || user.has_role("admin"),
            _ => false,
        }
    }
}

pub fn app() -> App {
    App::new()
        .module(Auth::new().account().verify_email()) // Breeze, with profile and verification
        .module(Permissions)                          // spatie/laravel-permission
        .gate("see-reports", |user| user.email.ends_with("@shop.example")) // Gate::define
}

// `AuthUser` is Auth::user(); a guest is sent to the login page (Option<AuthUser> if optional).
async fn edit(user: AuthUser, Found(post): Found<Post>) -> Result<View> {
    user.authorize("update", &post)?; // $this->authorize('update', $post): 403 unless allowed
    Ok(view("posts/edit.html", context! { post }))
}

// Sanctum's createToken with abilities; used as `Authorization: Bearer …`.
async fn issue_token(State(db): State<Db>, user: AuthUser) -> Result<String> {
    Ok(user.create_token_with(&db, "mobile", &["posts:read"], None).await?.plain)
}
```

What each part does:

- `impl Policy for Post` is the policy. A post's owner may update or delete it, and so may an
  admin. Anything else is refused (`_ => false`).
- `app()` turns on the auth pages (with the account page and email verification), roles and
  permissions, and a gate called `see-reports`.
- `edit` stops with a 403 unless the user may update this post.
- `issue_token` makes an API token that may only read posts, and returns it as text.

More auth:

- `Gate::before` is `App::gate_before`. A gate that needs the database is `gate_async`.
- `@can('update', $post)` needs the answer in the data. Wrap rows in
  `Can::new(post, user, &["update"])`, then write `{% if can('update', post) %}`.
- Roles and permissions: `user.assign_role(&db, "editor")`, `has_role`, `has_permission`,
  `.require_role(…)`, `.require_permission(…)`.
- spatie's Teams (`setPermissionsTeamId`) and Bouncer's scopes are roles given in one record:
  `user.assign_role_in(&db, "manager", &Scope::of(&store))` (with `.from` / `.until` dates),
  `permissions::set_scope(…)` per request (like `setPermissionsTeamId` in a middleware),
  `has_permission_in` for one row and `scopes_with` for lists.
- `logoutOtherDevices` is `auth::logout_other_devices`. Logout ends this device only.
- Users imported from Laravel keep their bcrypt hashes, and move to Argon2id at their next
  login.

Details: [authorization.md](authorization.md).

## Queues, events, mail, notifications and scheduling

All of it runs inside `serve`. The queue workers (`QUEUE_WORKERS`, 2 by default) and the
scheduler start with the web server. So there is no `queue:work` under Supervisor, and no cron
entry.

`queue:work` and `schedule:work` still exist, for running them in their own processes. The
queue lives in the database (`jobs`, `failed_jobs`), on SQLite as on PostgreSQL.

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A job. Its fields are saved as JSON in the `jobs` table.
#[derive(Serialize, Deserialize)]
struct SendReceipt {
    order_id: i64, // ids, not models (SerializesModels isn't a thing here)
}

impl Job for SendReceipt {
    const NAME: &'static str = "send-receipt"; // how a worker finds the type again
    const MAX_ATTEMPTS: u32 = 5;               // $tries

    /// The job's work, run later by a worker.
    async fn handle(self, ctx: JobContext) -> Result {
        let mail = ctx.state.mail_view(
            "buyer@example.com",
            "Your receipt",
            "mail/receipt", // resources/views/mail/receipt.html (+ .txt): a Mailable's view
            context! { order_id => self.order_id },
        )?;
        ctx.state.mailer.send(mail).await
    }
}

/// An event: a plain struct.
#[derive(Clone)]
struct OrderPlaced {
    order_id: i64,
}

impl Event for OrderPlaced {}

/// Register the job, a listener for the event, and a scheduled task.
pub fn app() -> App {
    App::new()
        .job::<SendReceipt>()
        .listen(|event: OrderPlaced, state| async move {
            // Listeners run in the request; queue the slow part (ShouldQueue).
            state.dispatch(SendReceipt { order_id: event.order_id }).await?;
            Ok(())
        })
        .schedule(|s| {
            // $schedule->call(...)->dailyAt('02:00'), in APP_TIMEZONE
            s.daily_at("02:00", "prune-carts", |state| async move {
                renox::db::sql("DELETE FROM carts WHERE updated_at < ?")
                    .bind(renox::db::now() - renox::chrono::TimeDelta::days(30))
                    .execute(&state.db)
                    .await?;
                Ok(())
            });
        })
}

async fn place(State(state): State<AppState>) -> Result<Redirect> {
    state.emit(OrderPlaced { order_id: 1 }).await?; // event(new OrderPlaced(...))
    let later = SendReceipt { order_id: 1 };
    state.queue.dispatch_after(later, Duration::from_secs(3600)).await?; // ->delay(...)
    Ok(Redirect::to("/orders"))
}
```

What happens:

- `SendReceipt` is a job. Its `handle` builds a mail from a template and sends it. If it fails,
  the worker tries again, up to 5 times.
- `OrderPlaced` is an event. In `app()`, a listener answers it by queueing a `SendReceipt`.
- The schedule deletes carts older than 30 days, every day at 02:00.
- `place` fires the event, then queues a second receipt to be sent in one hour (3600 seconds).

> [!NOTE]
> `const NAME` is the name saved in the `jobs` table. A worker uses it to know which type to
> rebuild, so register each job with `.job::<T>()`.

### Notifications

Notifications keep Laravel's shape, with `channels` for `via`:

```rust
use renox::auth::{Channel, Notification, Recipient};
use renox::mail::Mail;
use renox::prelude::*;

/// A notification, with the data it needs.
struct OrderShipped {
    order_id: i64,
}

impl Notification for OrderShipped {
    /// A name for this kind of notification, saved with it.
    fn kind(&self) -> &'static str {
        "order-shipped"
    }
    /// via(): send it by mail and save it in the database.
    fn channels(&self, _to: &Recipient) -> Vec<Channel> {
        vec![Channel::Mail, Channel::Database]
    }
    /// toMail().
    fn to_mail(&self, to: &Recipient, _state: &AppState) -> Result<Mail> {
        let subject = format!("Order #{} shipped", self.order_id);
        Ok(Mail::new(to.email().unwrap_or_default(), subject, "On its way."))
    }
    /// toDatabase(): the data the notification bell shows.
    fn to_database(&self, _to: &Recipient, _state: &AppState) -> Result<renox::serde_json::Value> {
        Ok(json!({ "order_id": self.order_id }))
    }
}

async fn ship(State(state): State<AppState>, user: AuthUser) -> Result<StatusCode> {
    // $user->notify(new OrderShipped(7)) on a ShouldQueue notification
    state.notify_later(user.user(), &OrderShipped { order_id: 7 }).await?;
    Ok(StatusCode::NO_CONTENT)
}
```

`notify_later` queues the notification, like a `ShouldQueue` notification in Laravel. The
handler then answers "204 No Content" (done, nothing to show).

### More on queues, mail and the scheduler

- **Mail.** `Mail::to()->cc()->bcc()->attach()` is the `Mail` builder (`also_to`, `cc`, `bcc`,
  `reply_to`, `from`, `attach`). `Mail::queue` is `state.queue_mail(mail)`. Markdown mail
  components are `renox/mail/components.html`. Mail previews are at `/_renox/mail`.
- **Queues.** `Bus::chain`/`Bus::batch` are `queue.chain()`/`queue.batch(name)`.
  `ShouldBeUnique`, `ShouldBeEncrypted`, `WithoutOverlapping`, `afterCommit`
  (`dispatch_in(&mut tx, job)`) and `dispatchSync` all exist. Horizon is
  `.module(renox::queue::Dashboard)`.
- **Scheduler.** It has `cron`, `hourly`, `weekly_on`, `monthly_on`, `weekdays`, `between`,
  `timezone`, `on_failure`, `then_ping`. Several servers can share one database: each run is
  claimed first, so `onOneServer` is built in.

Details: [queue.md](queue.md), [mail.md](mail.md), [scheduling.md](scheduling.md).

## Cache, storage, sessions, cookies and translations

The facades are fields of `AppState`: `state.cache`, `state.storage`, `state.db`,
`state.queue`, `state.mailer`, `state.http`. The session and the request's language are
extractors.

```rust
use renox::prelude::*;
use renox::{Cookies, SetCookie};
use std::time::Duration;

/// The cache, the session and translations in one handler.
async fn misc(State(state): State<AppState>, session: Session, lang: Lang) -> Result<String> {
    // Cache::remember
    let count: i64 = state
        .cache
        .remember("products.count", Duration::from_secs(60), || async {
            renox::db::sql("SELECT COUNT(*) FROM products").scalar(&state.db).await.map_err(Into::into)
        })
        .await?;
    session.put("cart", vec![1, 2, 3])?; // session()->put(...)
    session.flash("status", "Saved.")?;  // session()->flash(...)
    let cart: Option<Vec<i64>> = session.get("cart");
    let _ = cart;
    // __('cart.count', ...) with plural forms
    Ok(lang.choice("cart.count", count, &[]))
}

/// An upload is a form field of type `Upload`.
#[derive(serde::Deserialize)]
struct AvatarForm {
    photo: Upload, // $request->file('photo')
}

impl Validate for AvatarForm {
    fn rules(&self, v: &mut Validator) {
        v.field("photo", &self.photo).required().image().max(2048); // KB, checked by content
    }
}

/// Store the photo on the public disk and return its URL.
async fn upload(State(state): State<AppState>, Valid(form): Valid<AvatarForm>) -> Result<String> {
    let key = form.photo.store_public(&state.storage, "avatars").await?; // ->store('avatars', 'public')
    Ok(state.storage.url(&key)) // Storage::url; storage_url(key) in templates
}

/// Read a cookie, and set a new value for it.
async fn theme(State(state): State<AppState>, cookies: Cookies) -> (SetCookie, String) {
    let current = cookies.get("theme").unwrap_or_default(); // $request->cookie('theme')
    (SetCookie::new(&state, "theme", "dark"), current)    // Cookie::queue(...)
}
```

In plain words:

- `misc` counts the products, keeping the answer in the cache for 60 seconds. Then it puts a
  cart in the session, flashes a message for the next page, reads the cart back, and returns a
  translated text with the right plural form.
- `upload` takes a checked photo (an image, at most 2048 KB, checked by its content, not its
  name), stores it, and returns its public URL.
- `theme` reads the `theme` cookie and sets it to `dark`. Returning the `SetCookie` with the
  text is what sends the cookie.

More:

- **Cache stores.** `CACHE_STORE=memory|database`. There is no Redis: the `database` store
  shares the cache, locks, rate limits and the login lock between servers. `Cache::lock` is
  `state.cache.lock(name, ttl)`.
- **Disks.** For `Storage::disk('s3')`: `STORAGE_DISK=s3` for the default disk, and
  `App::disk(name, …)` plus `state.disk_named(name)` for more. `temporaryUrl` is
  `temporary_url`.
- **Sessions** are an encrypted cookie by default (no table needed; keep them small).
  `SESSION_DRIVER=database` keeps them in a table.
- **Translations** are `resources/lang/<locale>.json`, with Laravel's `:name` placeholders, `|`
  plurals and `{0}`/`[1,5]` ranges. Renox's own texts are English; your lang files translate
  them too (`renox.validation.*`, `renox.auth.*`, `ui.*`).

## Testing

Feature tests boot the whole app in memory. Each test gets a fresh, migrated database (no
`RefreshDatabase` needed), and the tests run in parallel. `TestApp` keeps cookies and sends the
CSRF token, like Laravel's test client.

```rust
use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

#[derive(Clone)]
struct OrderPlaced {
    id: i64,
}
impl Event for OrderPlaced {}

fn app() -> App {
    App::new().module(Auth::new())
}

/// A feature test: register a user, log in, then check the result.
#[renox::test]
async fn members_log_in() {
    let app = TestApp::new(app()).await;
    let user = User::register(app.db(), "Anna", "anna@example.com", "password123").await.unwrap();

    app.get("/login").await.assert_ok().assert_see("Log in");
    app.post("/login", &[("email", "anna@example.com"), ("password", "password123")])
        .await
        .assert_status(303);
    app.assert_authenticated(Some(&user)); // assertAuthenticatedAs
    app.assert_database_has("users", &[("email", &"anna@example.com")]).await;

    app.fake_events();                       // Event::fake()
    app.travel(Duration::from_secs(3600));   // $this->travel(1)->hours()
    app.assert_not_emitted::<OrderPlaced>(); // Event::assertNotDispatched
}
```

`#[renox::test]` marks a test function (it takes the place of `#[tokio::test]`). The test opens
the login page, posts the form, and expects a 303 redirect. Then it checks that Anna is logged
in and that her row is in `users`. The last three lines fake events, move the clock one hour
forward, and check that no `OrderPlaced` was fired.

The rest of Laravel's test helpers:

- `actingAs` is `app.acting_as(&user)`. `assertInvalid` is `assert_invalid("field")`, for htmx
  and JSON requests (`app.htmx().post(…)`). `assertJsonPath` is `assert_json_path`.
- `Mail::fake()` is the memory mailer (`sent_mail()`, `assert_mail_sent`).
  `Notification::fake()` is `fake_notifications()`. `Http::fake()` is `fake_http()`.
  `Queue::fake()` is `queued_jobs()`, and `run_jobs()` runs them.
- Time travel reaches the queue, sessions and rate limits, as long as your code reads the time
  through `renox::db::now()`.

Details: [testing.md](testing.md).

## Configuration and `.env`

`.env` looks like Laravel's: `APP_ENV`, `APP_KEY`, `APP_DEBUG`, `DATABASE_URL`, `MAIL_*`,
`QUEUE_WORKERS`, and so on. But there is no `config/*.php`.

Renox reads `.env` once, at boot, into a typed `Config`. Settings with a fixed set of values are
enums, not strings. A value it can't read stops the boot, with a message naming the variable,
instead of misbehaving later:

```rust
use renox::prelude::*;
use renox::{CacheStore, SessionDriver};

/// Settings are typed: compare them with enum values, not strings.
fn describe(config: &Config) -> String {
    let shared_cache = config.cache_store == CacheStore::Database;
    let in_table = config.session_driver == SessionDriver::Database;
    let live = config.env == Environment::Production;
    // Lifetimes are Durations (SESSION_LIFETIME stays in minutes in .env).
    format!("{shared_cache} {in_table} {live} {:?}", config.session_lifetime)
}

/// An unknown value is an error, not a silent default.
fn a_typo_stops_the_boot() {
    let config = Config::from_vars(|name| (name == "SESSION_DRIVER").then(|| "redis".to_string()));
    assert!(config.is_err()); // SESSION_DRIVER must be cookie or database
}

async fn pay(State(state): State<AppState>) -> Result<String> {
    // config('services.stripe.key'): your own settings are plain .env lines.
    let key = state.config.var("STRIPE_KEY").unwrap_or_default();
    Ok(format!("{} characters", key.len()))
}
```

- `describe` reads three settings as enums, and one lifetime as a `Duration`.
- `a_typo_stops_the_boot` shows what happens with `SESSION_DRIVER=redis`: reading the config
  fails.
- `pay` reads a setting of your own (`STRIPE_KEY`) with `config.var`.

`config:cache` has no equivalent and needs none. In tests, `TestApp::with_config(app, |c| …)`
changes the config without touching the environment. The full list is in the cheat-sheet's
"Configuration" section.

## Deployment

Forge, Envoyer, Supervisor and the cron entry are replaced by one binary and systemd (the
service manager built into most Linux servers):

```bash
rnx build          # dist/shop: server, workers, scheduler, CLI, views, migrations, public files
rnx make:deploy    # Dockerfile, a systemd service and socket, Litestream for SQLite backups
./shop migrate && ./shop     # on the server: migrate, then serve
```

- **Zero-downtime deploys** (Envoyer): the systemd *socket* holds the port while the app
  restarts. Visitors wait for a moment instead of getting "connection refused".
- **Backups:** Litestream streams the SQLite file to S3-compatible storage. On PostgreSQL, use
  your provider's backups.
- **Health checks:** `GET /health` (database and queue).
- **Several servers:** PostgreSQL (or one SQLite file on one machine) plus
  `CACHE_STORE=database`. The scheduler claims each run, and the workers share the queue.
- **Octane** isn't needed: the binary is a long-running server already.

Details: [operations.md](operations.md).

## Filament → the UI kit and the grid

Filament's panels are declared at run time. In Renox there are two ways. The `renox-admin`
crate does the same: a resource declares its grid columns, its form fields and its policy
once, and the panel gives it a list, forms, a view page, filters, bulk actions and exports
([admin.md](admin.md); the bike shop's panel is
[src/app/staff/admin.rs](../examples/bikeshop/src/app/staff/admin.rs)). Or the pages are generated once and then edited:
`rnx make:module orders --resource --fields "…"` writes the model, form, pages and tests on the
kit. The bike shop's staff side is written by hand on the kit, the way Filament's demo is.

A Filament table:

```php
public static function table(Table $table): Table
{
    return $table->columns([
        TextColumn::make('number')->searchable(),
        TextColumn::make('status')->badge(),
        TextColumn::make('total')->money('USD', divideBy: 100)->summarize(Sum::make()),
    ])->defaultSort('total', 'desc');
}
```

The same table as a Renox grid:

```rust
use renox::grid::{Column, Grid, GridRequest, Summary};
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "orders")]
struct Order { id: i64, number: String, status: String, total: i64 }

/// The grid: its columns, and the default sort (`-` means largest first).
fn orders_grid() -> Grid {
    Grid::new("orders")
        .column(Column::text("number", "Order").searchable())
        .column(Column::select("status", "Status", [("new", "New"), ("paid", "Paid")])
            .badges(&[("paid", "success")]))
        .column(Column::money("total", "Total").summary(Summary::Sum))
        .sort_by("-total")
}

async fn index(request: GridRequest) -> Result<View> {
    // Filters, search, sort and page come from the query string.
    let page = orders_grid().page(Order::query(), &request).await?;
    Ok(view("orders/index.html", context! { orders => page }))
}
```

The grid has three columns: a searchable order number, a status shown as a badge, and a total
with a sum at the bottom. `index` reads the visitor's filters, search, sort and page from the
address, loads that page of orders, and passes it to the template.

| Filament | Renox |
|---|---|
| Tables | `renox::grid` + `renox/grid.html`: filters, search, bulk and row actions, summaries, groups, exports (CSV, Excel, print), inline editing, reordering ([grid.md](grid.md)) |
| Forms | the kit's fields: `select` (searchable, options from the server), `date_picker`, `file`, `tags_input`, `repeater`, `key_value`, `wizard`, `show_when` |
| Rich text, Markdown and code editors | `rich_editor`, `markdown_editor`, `code_editor` from the `renox-editors` crate ([editors.md](editors.md)) |
| Infolists | `infolist`, `entry` (with `prefix_actions`/`suffix_actions`), `repeatable`; `code_entry` from `renox-editors` |
| Actions and modals | `action_sheet`, `slide_over`, `confirm`, `icon_button`, keyboard shortcuts |
| Notifications | `Toast` and the `notification_bell` over Server-Sent Events |
| Widgets | `stat`/`stats`, `chart(…)`, `renox::chart::Trend` |
| Panel navigation | `<rx-navbar>`, `<rx-sidebar>` in `<rx-shell fill>` (the dashboard standard: the window never scrolls, each panel does), `<rx-page-header>` |
| Resources (`ProductResource`), policies, `DeleteBulkAction`, `TrashedFilter` | `impl AdminResource` with the `renox-admin` crate: columns, fields, `filters`, `actions`, the model's `Policy`, soft deletes ([admin.md](admin.md)) |

Details: [ui.md](ui.md) and [grid.md](grid.md).

## What Renox doesn't have

Some of these are planned, and some are left out on purpose. The parity review keeps the full
list.

| Laravel | Status in Renox | Instead |
|---|---|---|
| Redis (cache, queue, sessions) | not planned | the database: `CACHE_STORE=database`, the database queue, `SESSION_DRIVER=database` |
| Broadcasting, Reverb, WebSockets | WebSockets not planned | Server-Sent Events power the notification bell; poll with htmx (`hx-trigger="every 10s"`) or the grid's `.poll(30)` for the rest |
| Schema builder (`Blueprint`) | not planned | SQL migrations, written for you from the models by `rnx make:migration --auto` (`db:diff`) and checked by `rnx db:check`; a `.postgres.up.sql` file when the two databases differ |
| Tinker | not planned | `db:shell` for SQL, and your own commands (`App::command`, `typed_command`) for code |
| MySQL, several connections, read/write split | SQLite and PostgreSQL, one `Db` per app | `renox::db::sql` for reports; a second sqlx pool by hand if you must |
| Livewire | live components | `#[renox::live_component]`, `rx-click`, `rx-model` ([live.md](live.md)) |
| Inertia | not planned | htmx + Alpine; for a SPA, a JSON API with tokens (the bike shop's `/api/v1`, [src/app/api/mod.rs](../examples/bikeshop/src/app/api/mod.rs)) |
| Fortify 2FA, Socialite | `renox-2fa` and `renox-oauth` (optional plugin crates) | `renox-2fa`: TOTP turned on from `/account`, the code after the password, recovery codes ([two-factor.md](two-factor.md)); `renox-oauth`: Google and GitHub (another provider is one `Provider` impl), PKCE, linking by verified email, link and unlink from `/account` ([oauth.md](oauth.md)) |
| Scout | the database engine, built in | `#[model(search = "title, body")]`, `renox::db::search::migration`, `Post::search(&q)`: SQLite FTS5 or PostgreSQL `tsvector`, kept current by the database, ranked, also behind the grid's search box ([search.md](search.md)); no Algolia/Meilisearch engines |
| Cashier | the `renox-billing` crate (an optional plugin) | `Billing::new().plan(…).stripe().xendit()`: plans declared in code, trials (also without a payment method), `swap`, `cancel` with a grace period, `resume`, `require_subscription`, the gateways' webhooks verified and applied once; Stripe and Xendit (another gateway is one `Gateway` impl) ([billing.md](billing.md); the bike shop's service plans, [src/app/plans/billing.rs](../examples/bikeshop/src/app/plans/billing.rs)). One-off payment pages and webhooks by hand: the bike shop's checkout ([src/app/sales/gateway.rs](../examples/bikeshop/src/app/sales/gateway.rs)) |
| Pennant feature flags | open | a setting or a gate |
| Several guards / user tables | one `users` table | roles from the `Permissions` module |
| Queued listeners | not built in | a listener that dispatches a job |
| Cache tags | missing | key prefixes, or `cache.forget` per key |
| Polymorphic many-to-many | missing | `Morph` covers morphTo/morphMany; a pivot per type |
| Slack and SMS channels, SES/Postmark API mailers | missing | `Channel::Custom` + `App::channel` with `state.http`; every provider offers SMTP |
| `model:prune` (`Prunable`) | partial | a scheduled `delete()` query; built-in `*:prune` commands for Renox's own tables |
| Optional route parameters, `where` constraints | partial | two routes; a value that doesn't parse is already a 404 |
| Dusk | partial | `TestApp::serve` plus a headless Chrome recipe (testing.md) |
| Telescope, Pulse | partial | `/_renox/debug` (last requests, SQL, views), the queue dashboard, `App::report`, JSON logs |
| Vapor (serverless) | not planned | one binary on a VM or in a container |
| An ecosystem of packages | none yet | many common packages are built in: permissions, activity log (`Audit`), webhooks, tenancy (`default_scope`), tables |

> [!IMPORTANT]
> And honestly: Renox is new (1.0 release candidates), with one maintainer and no community
> yet. Laravel's maturity, docs, courses and packages are years ahead. Check that the trade
> fits your project.

## Gotchas Laravel developers hit

These are the surprises people meet most often. Each one starts with the short rule in bold.

### Routes

**A route layer covers only the routes added before it.** `.require_auth()`, `.throttle(…)` and
`.route_layer(…)` wrap the routes already in that `Routes` value, not the ones after.

So a route added below `.require_auth()` is public. Build a separate `Routes` per guard and
`merge` them, as the examples above do.

**Paths are axum's.** Write `/products/{id}`; a wildcard is `{*rest}`. `:id` doesn't work.

A group's prefix starts with `/` and doesn't end with one. The group's index route is `"/"`,
never `""`.

### Compiler errors

**Handler futures must be `Send`.** axum runs handlers on a multi-threaded runtime, so a
handler must be safe to move between threads (that is what `Send` means).

If a handler won't compile as a route, you'll see "the trait `Handler<_, _>` is not
implemented", or "`Send` is not general enough". Look for an iterator or a closure that borrows
local data and is still alive across an `.await`. The fix: collect into a `Vec` first, then
await.

Holding a `std::sync::MutexGuard` across an `.await` causes the same error.

### Models and forms

**No lazy relations.** `post.author` is a field holding an id, not a query. Load related rows
with a method for one row, or a loader for a page of rows (see "Relations").

**Table names aren't pluralised.** Write `#[model(table = "posts")]`. Without it, the table is
the struct's name in snake_case. The key column is always `id`.

**Forms send text; your struct decides the type.**

- An `i64` field receiving `"abc"` is a validation error ("must be a number").
- An empty number field is "required": make optional inputs `Option<T>`.
- A checkbox is a `bool` (unticked sends nothing, read as `false`).
- A multi-select or checkbox group is a `Vec<T>` with `#[serde(default)]`.
- Keep money in integers (`i64` in the smallest unit), not floats.

**Model hooks skip bulk queries.** `saving`/`saved`/`deleting` run for `save`, `create`,
`insert`, `save_only`, `save_changes` and `delete`. They don't run for `Query::update`,
`Query::delete`, `insert_many` or `upsert`. Laravel behaves the same, but it's easy to forget.

### Settings

**`.env` is read once, strictly.** These stop the app at boot, with the variable's name:

- `APP_ENV=staging`;
- `SESSION_DRIVER=redis`;
- a port that isn't a number;
- a `throttle_by` without its `rate_limiter`;
- an unknown `MAIL_FAILOVER` name.

That is on purpose: a typo should not reach production as a silent default. Restart after
changing `.env`.

**Closures take the state last.** What the callback is about comes first, then the `AppState`:

- `listen(|event, state| …)`
- `App::command(name, about, |args, state| …)`
- `App::channel(name, |to, message, state| …)`
- `on_failure(|err, state| …)`
- `Auth::on_registered(|user, form, state| …)`
- `report(|report, state| …)`

### Templates

**Templates are strict while debugging.** With `APP_DEBUG` on, printing a variable that doesn't
exist is an error page showing the template line. (`{% if x %}` on a missing one is fine.)

Booleans print as `True`/`False`, so test them with `{% if %}`. Anything you pass to a view
must be `serde::Serialize`.

**Without `APP_DEBUG`, views are compiled in.** A release build (or an app without `.env`)
serves the templates, translations and public files embedded at build time. Restart (or
rebuild) after editing them.

**`hx-boost` gets whole pages.** A boosted link or form is a normal page request for Renox. So
pair `hx-boost` with `hx-select` when only part of the page should change.

### Databases

**SQLite has one writer.** While a transaction is open, write through `&mut tx`, not `db`.
Otherwise the second write waits for the first and fails with "database is locked".

**PostgreSQL is strict about types.** An `i64` field needs a `BIGINT` column (`INTEGER` is
32-bit there). `SUM(bigint)` is `NUMERIC`, so write `CAST(SUM(x) AS BIGINT)`. The query
builder's `sum` does that for you.

### Background work and time

**Read the time through Renox.** Use `renox::db::now()`, not `SystemTime::now()` or
`Utc::now()`. Then `TestApp::travel` reaches your code as it reaches Renox's.

**Jobs are serialized.** A job's fields go into the `jobs` table as JSON. Keep them small (ids,
not models). Register each job type with `App::job::<T>()` (or `Registry::job` in a module), or
the worker can't rebuild it.

**Listeners run inside `emit`.** They are awaited before `emit` returns, in the request. Put
slow work in a job.

**Sessions live in a cookie by default.** Browsers drop cookies over about 4 KB, and Renox logs
a warning before that. Store ids in the session, not whole objects, or use
`SESSION_DRIVER=database`.

### Debugging and logs

**`dd()` is `dbg!`, `Log::info` is `tracing`.** Renox logs through the `tracing` crate. Add
`tracing = "0.1"` to your `Cargo.toml` to use `tracing::info!` in your own code.
`LOG_FORMAT=json` and `LOG_FILE` replace Laravel's log channels.

## Where to go next

- [CHEATSHEET.md](../CHEATSHEET.md): one example of every pattern.
- The guides: [routing](routing.md), [validation](validation.md), [relations](relations.md),
  [types](types.md), [authorization](authorization.md), [queue](queue.md), [mail](mail.md),
  [scheduling](scheduling.md), [ui](ui.md), [grid](grid.md), [testing](testing.md),
  [operations](operations.md), [PostgreSQL](postgresql.md).
- The examples ([llms.txt](../llms.txt) says which shows what): `crud` is the reference CRUD
  module, `shop` a whole shop, `backoffice` the Filament-style admin, `admin` a panel made by
  `renox-admin`, `billing` subscriptions with `renox-billing`.
