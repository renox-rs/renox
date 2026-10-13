# Routing, middleware and requests

This guide is about the road a visit takes through your app. Someone opens an address in their
browser; Renox finds the function that answers it, hands that function what it asks for, and
sends its answer back.

On the way, you'll see:

- how to give addresses to your pages (**routes**) and give those routes names;
- what a **handler** (the function that answers) can ask for, and what it can send back;
- how to run code around many routes at once (**middleware**), for example "logged-in users
  only";
- the plumbing underneath: **sessions**, **CSRF** protection, **cookies** and **signed URLs**.

### Words you'll meet

| Word | What it means |
|---|---|
| **request** and **response** | The browser asks for something (a request); your app answers (a response). |
| **route** | A rule like "when someone opens `/products` with GET, run this function". |
| **HTTP method** | The kind of request: `GET` reads a page, `POST` sends a form, `PUT`/`PATCH` change something, `DELETE` removes it. |
| **handler** | The `async` function a route runs. It returns the answer. |
| **module** | A piece of your app for one feature (say, products): its routes and more. |
| **extractor** | A handler argument that pulls something out of the request: the logged-in user, a form, a number from the address. |
| **middleware** (or **layer**) | Code that runs before and after a handler, for many routes at once. |
| **guard** | Middleware that lets some visitors through and turns others away. |
| **session** | What the app remembers about one visitor between requests (their cart, that they're logged in). |
| **cookie** | A small piece of text the browser stores and sends back with every request. |
| **CSRF** | An attack where another website makes your browser send a form to this app. A secret token stops it. |
| **signed URL** | A link with a seal on it: if someone changes it, or it's too old, it stops working. |
| **status code** | A number in every response: 200 is "OK", 404 "not found", 403 "not allowed", 500 "the app broke". |

For the short version of every API, see the [cheat-sheet](../CHEATSHEET.md) (the sections "App,
module, routes", "Your own shared values and middleware", "Cookies and downloads", "Signed URLs
and private files" and "Security").

Some topics have their own guides:

- forms and the rules that check them: [validation.md](validation.md);
- htmx responses and toasts (small pop-up messages): [ui.md](ui.md);
- who may do what: [authorization.md](authorization.md).

### Quick lookup

| You want to… | Use |
|---|---|
| Add a page | `Routes::new().get("/path", handler).name("x")` in a `Module` |
| Prefix paths and names | `.group("/admin", "admin.", routes)` |
| The seven CRUD routes | `.resource("/products", "products", Resource::new()…)` |
| Another host | `.domain("admin.example.com", routes)`, `DomainParams` |
| Link to a route | `route('x', id)` in templates, `state.url("x", &[&id])` in Rust |
| Send somewhere | `Redirect::route("x", &[&id])`, `Redirect::intended`, `Back` |
| Run code around requests | `App::layer` (every route), `Routes::route_layer` (some) |
| Guard routes | `.require_auth()`, `.guest_only()`, `.require_gate(…)`, … |
| Limit request rates | `.throttle(60, …)`, `.throttle_by("api")` + `App::rate_limiter` |
| Remember something per visitor | `Session` (`put`, `flash`, `push`, `increment`) |
| A link that can't be altered | `state.signed_url(…)` + `ValidSignature` |

(CRUD means Create, Read, Update, Delete: the usual things you do with a list of records.)

## Apps, modules and routes

A Renox app is a list of **modules**. Each module is one feature of the app. A module has:

- a name;
- its routes;
- its migrations (the SQL files that make its database tables);
- the jobs, listeners and commands it registers (background work; see the other guides).

The command `route:list` shows every route, and which module it came from.

Here is an app with two modules: Renox's own login pages, and a `Products` module of yours.

```rust
use renox::prelude::*;

/// Builds the whole app: a list of modules.
pub fn app() -> App {
    App::new()
        .module(Auth::new()) // Renox's login, registration and password pages
        .module(Products)
}

/// The products feature. It holds no data, so it's an empty struct.
pub struct Products;

impl Module for Products {
    /// The module's name, shown by `route:list` and in logs.
    fn name(&self) -> &'static str {
        "products"
    }

    /// The addresses this module answers, and the handler each one runs.
    fn routes(&self) -> Routes {
        Routes::new()
            .get("/products", index).name("products.index")
            .get("/products/{id}", show).name("products.show") // {id}; a wildcard is {*rest}
            .post("/products", store).name("products.store")
            .put("/products/{id}", update)
            .patch("/products/{id}", update)
            .delete("/products/{id}", destroy)
    }
}

/// `GET /products`: a page made from a template.
async fn index() -> View {
    view("products/index.html", context! {})
}

/// `GET /products/7`: `Path` reads the `{id}` part of the address as a number.
async fn show(Path(id): Path<i64>) -> String {
    format!("product {id}")
}

/// `POST /products`: after saving, send the browser back to the list.
async fn store() -> Redirect {
    Redirect::to("/products")
}

/// `PUT` or `PATCH /products/7`: change product 7.
async fn update(Path(id): Path<i64>) -> String {
    format!("updated {id}")
}

/// `DELETE /products/7`: answer "204 No Content", a success with nothing to show.
async fn destroy(Path(id): Path<i64>) -> StatusCode {
    let _ = id;
    StatusCode::NO_CONTENT
}
```

What's going on:

- `.get("/products", index)` means: a GET request for `/products` runs the `index` function.
  `.post`, `.put`, `.patch` and `.delete` do the same for the other HTTP methods.
- `{id}` in a path is a blank that matches one part of the address: `/products/7` fills it with
  `7`. `{*rest}` is a wildcard: it matches everything after it, slashes included.
- A handler returns its answer. Here that's a `View` (a page from a template), a `String`
  (plain text), a `Redirect` (go to another address) or a `StatusCode` alone.

A few more things to know:

- `src/main.rs` only says `fn main() -> renox::Result { my_app::app().run() }`. The app itself
  lives in the library (`src/lib.rs`), so tests can start the very same app.
- The program is also the app's command-line tool: `migrate`, `route:list`, `down`, …
- `.name("…")` names the route added just before it. Use dotted names, like `products.show`.
  With a name, you can link to a route without typing its path (see
  [Route URLs](#route-urls-and-the-current-route) below).
- `.route(path, method_router)` takes any axum method router, for example
  `renox::axum::routing::get(show).post(update)`. (axum is the library Renox is built on.)
  `route:list` shows the method of such a route as `*`.

> [!WARNING]
> Inside a group, the group's own index page is `.get("/", …)`. Never write `""`: it panics.

> [!NOTE]
> **Coming from Laravel:** route names are dotted, as in Laravel. There is no `routes/web.php`:
> each module keeps its routes next to its handlers.

### Groups, resources and merging

Three tools help when you have many routes:

- a **resource** makes the usual list/show/create/edit/delete routes in one go;
- a **group** puts a shared prefix in front of a set of paths and names;
- **merge** joins two sets of routes into one.

```rust
use renox::prelude::*;

# async fn index() -> &'static str { "" }
# async fn create() -> &'static str { "" }
# async fn store() -> &'static str { "" }
# async fn show(Path(id): Path<i64>) -> String { id.to_string() }
# async fn edit(Path(id): Path<i64>) -> String { id.to_string() }
# async fn update(Path(id): Path<i64>) -> String { id.to_string() }
# async fn destroy(Path(id): Path<i64>) -> String { id.to_string() }
# async fn dashboard() -> &'static str { "" }
/// The shop's product routes plus an admin area, joined into one set.
fn routes() -> Routes {
    // A resource: each action you give it becomes one route, with a name.
    let shop = Routes::new().resource(
        "/products",
        "products",
        Resource::new()
            .index(index)     // GET    /products            products.index
            .create(create)   // GET    /products/new        products.create
            .store(store)     // POST   /products            products.store
            .show(show)       // GET    /products/{id}       products.show
            .edit(edit)       // GET    /products/{id}/edit  products.edit
            .update(update)   // PUT and PATCH /products/{id}  products.update
            .destroy(destroy), // DELETE /products/{id}      products.destroy
    );
    // A group: every path inside starts with /admin, every name with "admin.".
    let admin = Routes::new().group(
        "/admin", // path prefix: starts with `/`, never ends with one
        "admin.", // name prefix
        Routes::new()
            .get("/", dashboard).name("dashboard") // GET /admin, `admin.dashboard`
            .require_auth(),                       // covers the group's routes only
    );
    shop.merge(admin) // the other routes, with their names and layers
}
```

What's going on:

- The resource made seven routes, and the comments show each one's method, path and name.
- A resource has only the actions you give it. Leave out `.edit(…)` and there's no edit page.
- The group's `"/"` became `/admin`, and its name `dashboard` became `admin.dashboard`.
- `.require_auth()` is a guard ("logged-in users only"). Here it covers only the group's routes.
- `merge` keeps each route's name and layers.

> [!WARNING]
> `group` panics when the routes inside it have a `.domain(…)` or a `.fallback(…)`: put those
> outside the group. `merge` panics when both sides have a fallback.

> [!TIP]
> `rnx make:module products --resource` writes a whole resource for you: its handlers, views and
> tests.

Some pages need no handler at all: a page that only shows a template, or an old address that
should send people to a new one.

```rust
use renox::prelude::*;

/// Routes that need no handler of their own.
fn routes() -> Routes {
    Routes::new()
        .view("/about", "pages/about.html").name("about") // GET, rendered with the globals
        .redirect("/about-us", "/about")                  // 302, any method
        .permanent_redirect("/old-shop", "/products")     // 301
}
```

What's going on:

- `.view` answers GET with a template. The template still gets the usual global values (the
  ones every page gets, like the app's name and the logged-in user).
- `.redirect` sends any method to another address with status 302 ("found it over there, for
  now").
- `.permanent_redirect` uses 301 ("moved for good"). Browsers and search engines remember it.

> [!NOTE]
> **Coming from Laravel:** these are Laravel's `Route::view` and `Route::redirect`.

### Other hosts and the fallback

The **host** is the name part of an address: in `https://acme.example.com/`, the host is
`acme.example.com`. One app can answer several hosts, with different pages on each.

`Routes::domain` serves some routes only on hosts that match a pattern. In the pattern,
`{name}` matches one label of the host (one piece between dots). You read it with the
`DomainParams` extractor.

The rules:

- A host that matches a domain gets **only** that domain's routes. It also gets Renox's own
  (`/health`, the scripts, public files), but never the other routes.
- Every other host gets the routes without a domain.

So `/` can be a different page on each host.

The **fallback** is what answers when nothing else does.

```rust
use renox::prelude::*;
use renox::DomainParams;
use renox::axum::http::Uri;

/// The home page of the main site.
async fn home() -> &'static str {
    "the main site"
}

/// The home page of a team's own host, like `acme.example.com`.
async fn team_home(domain: DomainParams) -> String {
    // "account" is the `{account}` part of the host: "acme" for acme.example.com.
    format!("Welcome to {}", domain.get("account").unwrap_or("?"))
}

/// Answers when no route matches: a 404 with a friendlier message.
async fn missing(uri: Uri) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("Nothing at {}. Try the search.", uri.path()))
}

/// The main site's routes, the team hosts' routes, and the fallback.
fn routes() -> Routes {
    Routes::new()
        .get("/", home).name("home")
        .domain("{account}.example.com", Routes::new().get("/", team_home).name("team.home"))
        .fallback(missing) // when no route and no public file answers (instead of the 404 page)
}
```

What's going on:

- `example.com/` shows "the main site".
- `acme.example.com/` shows "Welcome to acme".
- An address nothing answers (no route, no public file) runs `missing` instead of showing the
  usual 404 page.

Good to know:

- Matching uses the `Host` header, which the browser sends with every request.
- There is one fallback per app, or one per domain inside `Routes::domain`.
- [examples/bikeshop](../examples/bikeshop) serves each store's page on its own host,
  `{store}.localhost` in development ([src/app/home/stores.rs](../examples/bikeshop/src/app/home/stores.rs)).

> [!WARNING]
> Behind a proxy (a server like Caddy or nginx in front of your app), the proxy must pass the
> `Host` header through. Caddy does. nginx needs `proxy_set_header Host $host`.

> [!NOTE]
> **Coming from Laravel:** this is `Route::domain` and `Route::fallback`. One difference: a host
> that matches a domain gets that domain's routes only. It never falls through to the others.

### Route URLs and the current route

Once a route has a name, you can build its address from the name. If you later change the
path, the links follow by themselves.

| Where | Path of a named route |
|---|---|
| Templates | `{{ route('products.show', product.id) }}`; named arguments become the query string: `route('products.index', page=2)` → `/products?page=2` |
| Handlers, jobs, commands | `state.url("products.show", &[&id])?`, `state.absolute_url(…)` (with `APP_URL`) |
| Redirects | `Redirect::route("products.show", &[&id])?` |

How the arguments work:

- They fill the `{…}` blanks of the path, in order.
- They are percent-encoded: characters that aren't safe in an address (like spaces) are
  written as codes such as `%20`.
- A missing argument, an extra one, or an unknown route name is an error.
- `state.absolute_url` gives the full address, starting with `APP_URL` (like
  `https://example.com/products/42`).

You can also ask which route is answering right now:

```rust
use renox::prelude::*;
use renox::CurrentRoute;

/// Shows the route that is answering this request.
async fn menu(route: CurrentRoute) -> String {
    // The matched route's name and path pattern; `*` in `is` stands for anything.
    format!("{} {} {}", route.name().unwrap_or("-"), route.path().unwrap_or("-"), route.is("admin.*"))
}

/// Builds the address of product `id` from the route's name.
async fn links(State(state): State<AppState>, Path(id): Path<i64>) -> Result<String> {
    state.url("products.show", &[&id]) // "/products/42"
}
```

In templates, use `{% if route_is('admin.*') %}` (you can pass several patterns) and
`request.route`. That's handy for highlighting the current page in a menu.

To see every route, run `my-app route:list` (or `rnx route:list` while you develop). It prints
each route's method, path, name, module and guards (`auth`, `throttle:60/60s`, …). When there
are domains, it shows the domain too.

> [!NOTE]
> **Coming from Laravel:** `route_is` is `request()->routeIs()`, `CurrentRoute` is
> `Route::currentRouteName()`, and `route:list` works like `php artisan route:list`.

## What a handler can take

A handler is an `async` function. Each of its arguments is an **extractor**: a type that says
"give me this part of the request". Renox fills them in before the handler runs. If one can't
be filled (say, the visitor isn't logged in), Renox answers for you and the handler doesn't
run.

The order of the arguments doesn't matter, with one exception.

> [!IMPORTANT]
> An extractor that reads the request's body (`Valid<T>`, `Form`, `Json`) must be the **last**
> argument. The body can only be read once.

| Extractor | Gives | Notes |
|---|---|---|
| `Path<T>` | route parameters: `Path(id): Path<i64>`, `Path((a, b)): Path<(i64, i64)>` | `renox::Path` (in the prelude): a value that doesn't parse (`/products/abc`) is a **404**, not axum's 400; a parameter the route lacks is a 500 (the app's bug) |
| `Found<M>` | the model a route parameter names, loaded (route model binding) | 404 when there's no such row; below |
| `Query<T>` | the query string, deserialized | axum's; `Option` fields for optional ones |
| `Valid<T>` | a validated form, JSON body or (for GET) query string | errors: redirect back, or 422 for htmx/JSON; see [validation.md](validation.md) |
| `Form<T>`, `Json<T>` | the body, unvalidated | axum's |
| `AuthUser` | the logged-in user (derefs to `User`) | guests: redirect to `login`, or 401 JSON for API clients (`Accept: application/json`, a JSON body or an `Authorization` header); `Option<AuthUser>` for either |
| `Session` | the visitor's session | below |
| `Htmx` | `request`, `boosted`, `target`, `trigger`, `current_url` | see [ui.md](ui.md) |
| `Lang` | the request's locale (`lang.locale`), `t`, `choice` | |
| `ClientIp` | `ClientIp(Option<IpAddr>)` | through trusted proxies, below |
| `renox::RequestId` | this request's id | below |
| `renox::CurrentRoute` | the matched route | above |
| `renox::DomainParams` | `{name}` parts of the host | above |
| `renox::Provided<T>` | a value given to `App::provide` | 500 naming the type if none was |
| `renox::Cookies` | the request's own cookies | below |
| `renox::signed::ValidSignature` | proof the URL was signed and hasn't expired | 403 otherwise |
| `State<AppState>` | the whole app: `db`, `config`, `cache`, `storage`, `url`, … | `State<Db>` for just the database |

A few words from the table, explained:

- The **query string** is the part of an address after `?`: in `/search?q=shoes`, it's
  `q=shoes`. "Deserialized" means Renox turns it into your struct.
- The **body** is the data a form or a script sends with a POST.
- **Locale** is the visitor's language, like `en` or `es`.
- An **API client** is a program (not a person in a browser) that talks to your app, usually
  in JSON.

Here is a handler that asks for many things at once:

```rust
use renox::prelude::*;
use renox::{Provided, RequestId};

/// The search form's fields, read from the query string (`?q=…`).
#[derive(serde::Deserialize)]
struct Filter {
    q: Option<String>,
}

/// Settings for a payment service, given to the app once at start-up.
#[derive(Clone)]
struct Payments {
    api_key: String,
}

/// Each argument is an extractor; Renox fills them all in before this runs.
async fn search(
    user: Option<AuthUser>,
    Query(filter): Query<Filter>,
    ClientIp(ip): ClientIp,
    id: RequestId,
    lang: Lang,
    payments: Provided<Payments>,
) -> String {
    format!(
        "{} searched {:?} from {:?} ({id}, {}, key {}…)",
        user.map_or("a guest".to_owned(), |u| u.email.clone()),
        filter.q,
        ip,
        lang.locale,
        &payments.api_key[..3],
    )
}

/// Builds the app and hands it a `Payments` value that handlers can ask for.
fn app() -> App {
    // `Provided<T>` in handlers; `state.provided::<T>()` in jobs, listeners and commands.
    App::new().provide(Payments { api_key: "sk_test_123".into() })
}
```

What's going on:

- `Option<AuthUser>` is the logged-in user, or `None` for a guest. Plain `AuthUser` would send
  guests to the login page instead.
- `Query(filter)` reads `?q=…` into the `Filter` struct. `q` is an `Option`, so it may be left
  out.
- `ClientIp`, `RequestId` and `Lang` give the visitor's IP address, this request's id and the
  visitor's language.
- `Provided<Payments>` gives the value passed to `App::provide`. That's how you share your own
  things (a client for a payment service, settings) with every handler. A module provides
  its own from `register` with `app.provide(…)` (`Registry::provide`); the app's value of the
  same type wins.

> [!NOTE]
> **Coming from Laravel:** `App::provide` + `Provided<T>` stand in for service container
> bindings. `Query<T>` and `ClientIp` are `$request->query()` and `$request->ip()`.

### Route model binding: `Found<M>`

Many routes are about one row of a table: `/posts/7` shows post 7. `Found<M>` loads that row
for you. ("Route model binding" is the name for this: the route's parameter is bound to a
model.)

`Found(product): Found<Product>` loads the row the route parameter names. If there is no such
row, the answer is a 404, just like an address with no route.

Which parameter does it use?

- The one named after the model's table: `{product}` for `Product`.
- Otherwise, the route's only parameter.

How is the value read?

- As the model's key: `{product}`, `{id}`, and ULID or UUID keys too.
- Or, when the parameter's name is a column, by that column: `/blog/{slug}` finds the post
  whose `slug` matches.

The query is the model's own, so its rules apply. A default scope (for example, "only the
current team's rows") and soft deletes (rows marked as deleted but kept) still count: another
team's id, or a deleted row, is a 404.

```rust
use renox::prelude::*;
# #[derive(Model, serde::Serialize, Default)] struct Team { id: i64, name: String }
# #[derive(Model, serde::Serialize, Default)] struct Post { id: i64, slug: String, title: String }

// GET /teams/{team}/posts/{post}: each model takes the parameter named after it.
async fn show(Found(team): Found<Team>, Found(post): Found<Post>) -> String {
    format!("{}: {}", team.name, post.title)
}

// GET /blog/{slug}: `slug` is a column, so the post is found by it.
async fn by_slug(Found(post): Found<Post>) -> String {
    post.title
}
```

> [!IMPORTANT]
> `Found` only loads the row. Checking whether this user may see or change it is still the
> handler's job (`user.authorize("update", &post)?`), as with `find_or_404`. See
> [authorization.md](authorization.md).

> [!NOTE]
> **Coming from Laravel:** this is route model binding. Laravel's `{post:slug}` becomes a
> parameter named after the column, such as `{slug}`.

## What a handler can return

A handler can return anything axum knows how to turn into a response, plus Renox's own types:

| Return | Response |
|---|---|
| `View` (`view("x.html", context! {…})`) | a rendered page; `.fragment("block")` for htmx, `.also(…)` for out-of-band swaps |
| `Redirect::to("/x")` | 303 |
| `Redirect::route("x", &[&id])?` | 303 to a named route |
| `Redirect::intended(&session, "/dashboard")` | 303 to the page a guard sent the user away from (once, same site only), else the fallback |
| `Back` (extractor and response) | 303 to the `Referer`, or `/` when it's missing or on another site |
| `Json(json!({…}))`, `String`, `Html(…)`, `StatusCode` | axum's |
| `renox::Download` | a file: `bytes`, `file` (streamed), `from_storage`, `stream`; `.inline()` to show it in the browser (never for HTML, XML or JavaScript, which are always downloads) |
| `renox::Toast::success("…")` in a tuple | a toast with the response (or the next page after a redirect) |
| `HxRedirect`, `HxRefresh`, `HxTrigger`, `HxRetarget`, `HxReswap`, `HxPushUrl` | htmx response headers; see [ui.md](ui.md) |
| `Err(…)` from `Result<T>` | an error page (`errors/{status}.html`), or JSON for API clients |

Some words from the table:

- A **303 redirect** tells the browser "now go to this other address with GET". It's the usual
  answer after a form is saved.
- **`Redirect::intended`**: say a guest opens `/dashboard` and gets sent to the login page.
  After logging in, `intended` takes them back to `/dashboard`. It works once, and only for an
  address on your own site; if there is none, it goes to the fallback you give it.
- **`Back`** returns to the page the visitor came from (the `Referer` header). If that's
  missing or on another site, it goes to `/`.
- A **fragment** is one block of a template, sent alone so htmx can swap just that part of the
  page. `.also(…)` sends extra pieces to swap elsewhere on the page ("out-of-band").
- To return two things at once (a toast and a redirect), put them in a tuple.

```rust
use renox::prelude::*;
use renox::Download;

/// After saving: a "Saved" message, and a redirect to the product's page.
async fn store(session: Session) -> Result<(Toast, Redirect)> {
    session.flash("status", "Saved")?; // or a toast, which needs no template code
    let _back = Redirect::intended(&session, "/products");
    Ok((Toast::success("Product saved."), Redirect::route("products.show", &[&42])?))
}

/// Sends invoice `id` as a PDF, shown in the browser instead of saved.
async fn invoice(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Download> {
    abort_if(id <= 0, StatusCode::NOT_FOUND, "No such invoice.")?; // any status with a message
    Download::from_storage(&state.storage, &format!("invoices/{id}.pdf"), "invoice.pdf")
        .await
        .map(Download::inline) // a missing key is a 404
}

/// A tiny JSON answer: `{"ok": true}`.
async fn ping() -> Json<serde_json::Value> {
    Json(json!({ "ok": true }))
}
```

What's going on:

- `store` returns a toast and a redirect together. The toast shows on the page the redirect
  leads to.
- `invoice` stops early with a 404 when the id makes no sense, then sends the file from
  storage. If the file isn't there, that's a 404 too.
- `ping` returns JSON, as an API would.

Good to know:

- A filename given to `Download` is made safe for the `Content-Disposition` header (the header
  that tells the browser the file's name).
- `abort(status, msg)`, `abort_if` and `abort_unless` stop a handler with a status and a
  message that visitors see.

> [!NOTE]
> **Coming from Laravel:** `Redirect::route`, `Redirect::intended` and `Back` are
> `redirect()->route()`, `->intended()` and `back()`. `Download` is `response()->download()`.

## Middleware

**Middleware** is code that runs around a handler: before it, after it, or both. It's the
place for things that many routes share, like "only for logged-in users" or "add this header
to every answer". Middleware is also called a **layer**, because each one wraps the ones
inside it, like the layers of an onion.

### The order around your handler

Every request passes through these layers, outermost first. You rarely need the details. They
tell you what your own middleware can count on.

1. **Security headers** (nosniff, Referrer-Policy, X-Frame-Options, Permissions-Policy, HSTS, the CSP) and the
   client IP. (These headers tell browsers to block common tricks; the CSP, or Content
   Security Policy, says which scripts a page may run.)
2. **Method spoofing**: a POST with `_method=PUT|PATCH|DELETE` is routed as that method (see
   [Method spoofing](#method-spoofing)).
3. **Request id**, then the request's log span (the request's details, attached to its log
   lines); then the **body limit** (`UPLOAD_MAX_SIZE`).
4. Renox's scripts, `/health`, `/robots.txt`, `/favicon.ico`, the live-reload stream
   (`/_renox/live`, only while debugging locally) and the local disk's public files answer here,
   before sessions and maintenance mode.
5. Then, in this order:
   - the debug inspector, which records each request for `/_renox/debug` (only with
     `APP_DEBUG` on and `APP_ENV=local`; it skips `/_renox/*` addresses);
   - the request's context (`renox::context`);
   - the **session**;
   - the **locale** (the visitor's language);
   - the **user** (from the session or an `Authorization: Bearer` token);
   - **CSRF**;
   - **views** (renders `View`s and error pages);
   - **maintenance mode**;
   - a guard that turns a panic or a run past `REQUEST_TIMEOUT` into a 500 page.
6. Your `App::layer` layers, first added = outermost.
7. The route's own layers (guards, throttles, `route_layer`), then the handler.

What this means for you: an `App::layer` middleware already sees the session, `AuthUser`,
`Lang` and the context. And its response still gets the security headers and the error pages.

### Your own middleware

A middleware is a function with axum's `from_fn` signature: it takes extractors (if any), the
request, and `next`, which runs the rest. Then you attach it in one of two ways:

- `App::layer` wraps every route of the app's modules.
- `Routes::route_layer` wraps only the routes added **before** it in that `Routes`. Every
  guard below works the same way.

```rust
use renox::prelude::*;
use renox::axum::extract::Request;
use renox::axum::middleware::{Next, from_fn};

/// Middleware: adds an `x-visitor` header saying "member" or "guest" to the answer.
async fn stamp(user: Option<AuthUser>, req: Request, next: Next) -> Response {
    let mut res = next.run(req).await; // the handler (and inner layers) run here
    // From here on, the handler has answered; we can change its response.
    let who = if user.is_some() { "member" } else { "guest" };
    res.headers_mut().insert("x-visitor", who.parse().unwrap());
    res
}

/// Wraps every route of the app in `stamp`.
fn app() -> App {
    App::new().layer(from_fn(stamp)) // every route
}

/// Wraps only `/reports` in `stamp`.
fn routes() -> Routes {
    Routes::new()
        .get("/reports", || async { "reports" })
        .route_layer(from_fn(stamp)) // only /reports
        .get("/about", || async { "about" }) // not wrapped: added after
}
```

What's going on:

- Code before `next.run(req).await` runs before the handler; code after it runs after.
- `app()` puts `stamp` around every route.
- In `routes()`, `stamp` covers `/reports` but not `/about`, because `/about` comes after the
  `route_layer` line.

> [!WARNING]
> Don't keep a closure that borrows `req` alive across `next.run(req).await` (put it in its own
> `{ … }` block so it ends first). Otherwise the handler's future isn't `Send`, and it won't
> compile as a route.

> [!TIP]
> To hand a value from middleware to handlers, call `renox::context::set(value)` in the
> middleware and take the `Current<T>` extractor in the handler (see
> [authorization.md](authorization.md) "Tenants").

> [!NOTE]
> **Coming from Laravel:** global middleware (`Kernel::$middleware`) is `App::layer(from_fn(…))`.
> Route middleware is `.route_layer(…)`, written after the routes.

### Guards

A **guard** is middleware that decides who gets in.

> [!IMPORTANT]
> Each guard covers the routes added **before** it, so put it after them.

What happens to visitors who are turned away:

- Guests asking for a page are sent to the `login` route. For a GET request, Renox remembers
  the page, and `Redirect::intended` takes them back to it after they log in (a guest who sent a
  form isn't sent back to it). An htmx request gets an `HX-Redirect` header instead of a
  redirect, so the whole page goes to the login page.
- API clients (requests with `Accept: application/json`, a JSON body or an `Authorization`
  header) get a 401 JSON answer instead.

| Guard | Lets through | Others |
|---|---|---|
| `.require_auth()` | logged-in users | guests → `login` |
| `.require_verified()` | users with a verified email | guests → `login`; others → `verification.notice`, or 403 JSON for JSON and API-token requests |
| `.guest_only()` | guests (login, register pages) | users → `home` |
| `.require_gate("x")` | users a gate (or permission) allows | guests → `login`; others 403 |
| `.require_role("admin")`, `.require_permission("x")` | the `Permissions` module | guests → `login`; others 403 |
| `.require_ability("orders:write")` | API tokens with the ability; sessions and unrestricted tokens | guests → `login`; others 403 |
| `.require_password_confirmed()` | users who typed their password in the last three hours | → `/confirm-password` |

(A **gate** is a named yes/no rule, like "may manage billing". Roles, permissions and token
abilities are explained in [authorization.md](authorization.md).)

```rust
use renox::prelude::*;

# async fn index() -> &'static str { "" }
# async fn store() -> &'static str { "" }
# async fn login() -> &'static str { "" }
/// Three sets of routes, each with its own guard, joined at the end.
fn routes() -> Routes {
    let public = Routes::new().get("/posts", index);
    let members = Routes::new().post("/posts", store).require_auth(); // POST /posts only
    let guests = Routes::new().get("/signin", login).guest_only();
    public.merge(members).merge(guests)
}
```

What's going on:

- Anyone can read `/posts`.
- Only logged-in users can send a new post (`POST /posts`).
- Only guests see `/signin`; a logged-in user is sent to `home`.

> [!WARNING]
> A route added **after** a guard in the same `Routes` is **not** covered. Splitting routes into
> separate `Routes` values and merging them, as above, makes that easy to see.

> [!NOTE]
> **Coming from Laravel:** the `auth`, `guest`, `verified`, `can:` and `password.confirm`
> middleware are `.require_auth()`, `.guest_only()`, `.require_verified()`, `.require_gate(…)`
> and `.require_password_confirmed()`.

### Rate limits

A **rate limit** caps how often someone may call a route, say 60 times a minute. It protects
your app from scripts that hammer it.

`.throttle(max, per)` counts per logged-in user, or per IP address for guests.

- Over the limit, the answer is **429 Too Many Requests**, with a `Retry-After` header that
  says how long to wait.
- Allowed answers carry `X-RateLimit-Limit` and `X-RateLimit-Remaining` headers.

When the limit should depend on the request (partners get more, guests less), give a limiter
a name:

```rust
use renox::prelude::*;
use renox::rate_limit::Limit;
use std::time::Duration;

# async fn orders() -> &'static str { "" }
# async fn login() -> &'static str { "" }
/// Defines a limiter called "api" with a different limit for each kind of visitor.
fn app() -> App {
    App::new().rate_limiter("api", |req| match req.user {
        Some(user) if user.has_role("partner") => Limit::none(),
        Some(_) => Limit::per_minute(600),
        None => Limit::per_minute(60).by(format!("ip:{:?}", req.ip)), // `by`: your own key
    })
}

/// One route uses the named limiter; the other a simple 5-per-minute limit.
fn routes() -> Routes {
    Routes::new()
        .get("/api/orders", orders).throttle_by("api")
        .post("/contact", login).throttle(5, Duration::from_secs(60))
}
```

What's going on:

- Partners have no limit, other logged-in users get 600 a minute, guests 60 a minute.
- `.by(…)` sets what to count by. Here, guests are counted by their IP address.
- `/api/orders` uses the "api" limiter. `/contact` allows 5 posts a minute.

A `Limit` is one of:

- `Limit::per_minute(n)`: at most `n` requests a minute;
- `Limit::per_hour(n)`: at most `n` an hour;
- `Limit::per(n, duration)`: at most `n` per any length of time (`n` is at least 1, the time at
  least a second);
- `Limit::none()`: no limit.

A rule can look at `req.user`, `req.ip`, `req.method`, `req.path` and `req.headers`.

> [!WARNING]
> `.throttle_by("api")` needs an `App::rate_limiter("api", …)`. Without one, the app stops at
> start-up with an error naming the route.

> [!NOTE]
> The counters live in memory, so each server counts on its own. With `CACHE_STORE=database`,
> they are shared between servers through the `cache` table (and so is the login lock, which
> blocks someone after too many wrong passwords).

> [!NOTE]
> **Coming from Laravel:** `throttle:60,1` is `.throttle(60, Duration::from_secs(60))`, and
> `RateLimiter::for` is `App::rate_limiter` + `.throttle_by`.

### CORS

Browsers stop a page on one site from reading answers from another site, unless that other
site says it's fine. **CORS** (Cross-Origin Resource Sharing) is how a site says so. An
**origin** is a site's scheme and host, like `https://app.example.com`.

`.cors(&["https://app.example.com"])` (or `&["*"]` for any origin) lets browsers on those
origins `fetch` the routes added before it. Renox answers the browser's "preflight" check (a
question it asks before the real request) and adds the `Access-Control-Allow-*` headers.

What `.cors` allows:

- the methods GET, POST, PUT, PATCH, DELETE and OPTIONS;
- only the request headers `Content-Type`, `Authorization`, `Accept` and `X-CSRF-Token` (not
  `X-XSRF-TOKEN`);
- no cookies;
- browsers may keep the preflight's answer for an hour.

For credentials (cookies) or other headers, build a `renox::cors::CorsLayer` and pass it to
`.cors_layer(…)`.

> [!NOTE]
> **Coming from Laravel:** this replaces `config/cors.php`.

### ETags

An **ETag** is a fingerprint of a page. The browser keeps it, and next time asks "has the page
changed since this fingerprint?". If not, the app answers "no" without sending the page again,
which saves time and data.

`.etag()` gives the routes added before it an `ETag` header: a hash of the page as it is sent.
When the browser's `If-None-Match` header names that same hash, the answer is
**304 Not Modified**, with no body.

It's made for feeds, sitemaps, catalogues and API lists that are fetched again and again but
rarely change.

Only these answers get one: `GET`/`HEAD` requests, status 200, and a body of at most 2 MB.

HTML pages work too: the request's CSP nonce (in every layout with scripts) changes on each
request, so it's left out of the hash, and a 304 carries no `Content-Security-Policy`, so the
browser keeps the policy it stored with the page, whose nonce matches that page.

```rust
use renox::prelude::*;
# async fn feed() -> &'static str { "" }
# async fn sitemap() -> &'static str { "" }

/// A feed and a sitemap, both with ETags.
fn routes() -> Routes {
    Routes::new()
        .get("/feed.xml", feed)
        .get("/sitemap.xml", sitemap)
        .etag() // covers the two routes above
}
```

> [!NOTE]
> **Coming from Laravel:** this is the `SetCacheHeaders` middleware with `etag`.

## Sessions

The web forgets: each request arrives on its own. A **session** is how the app remembers one
visitor from one request to the next: that they're logged in, what's in their cart, a message
to show on the next page.

`Session` is the visitor's session, as an extractor.

Where the session is kept:

- **By default, in a cookie.** The cookie is encrypted and signed, with a key made from
  `APP_KEY`: the visitor can't read it or change it. It needs no database.
- **With `SESSION_DRIVER=database`**, the cookie holds only an id, and the data lives in the
  `sessions` table. About one request in 50 also deletes expired rows in the background;
  `my-app session:prune` does it on demand (handy for a quiet app, or on a schedule).

> [!WARNING]
> Keep a cookie session small. Browsers drop cookies over 4 KB, and Renox logs a warning when a
> session gets that big.

```rust
use renox::prelude::*;

/// Shows the main things a session can do.
async fn cart(session: Session) -> Result<String> {
    session.put("cart", vec![1, 2, 3])?;
    session.push("recent", 42)?;                 // append to a list
    let visits = session.increment("visits", 1)?; // 1, 2, …
    let cart: Option<Vec<i64>> = session.get("cart");
    let coupon: Option<String> = session.pull("coupon"); // read and remove
    session.flash("status", "Added to the cart")?; // the next request only
    Ok(format!("{visits} {cart:?} {coupon:?}"))
}
```

What's going on:

- `put` stores a value under a name; `get` reads it back (`None` if it isn't there).
- `push` adds to a list; `increment` adds to a number and returns the new total.
- `pull` reads a value and removes it. `pull` and `remove` only touch values stored with
  `put` (or `push`, `increment`): a flashed value stays, and `pull` on it gives `None`.
- `flash` stores a value for the next request only, like "Added to the cart" shown after a
  redirect.

| Method | Does |
|---|---|
| `get`, `has`, `put`, `remove`, `pull` | read and write values (any `Serialize` type); `get` and `has` also see flashed values, `remove` and `pull` don't |
| `push`, `increment` | append to a list; add to a number |
| `flash`, `reflash`, `flashed` | values for the next request only ("Saved!" after a redirect) |
| `keep(&["status"])`, `flash_now(key, value)` | keep some flashed values for one more request; a flash value for the page rendered now only (Laravel's `flash()->now()`) |
| `old(field)`, `errors()` | the previous form's input and validation errors (filled by `Valid<T>`; what old input keeps is in [validation.md](validation.md#answer-2-plain-form-posts-get-a-redirect-back)) |
| `has_old_input` | whether the previous request flashed its input (a failed submit), even with no field in it |
| `set_lifetime(duration)` | this session uses this lifetime instead of `SESSION_LIFETIME` (whole minutes, rounded up, at least one) |
| `token`, `regenerate_token`, `flush` | the CSRF token; a new one; empty everything |

How long a session lasts, and what logging in and out does:

- A session ends after `SESSION_LIFETIME` minutes without a visit (120).
- "Remember me" on Renox's login page makes it last `REMEMBER_LIFETIME` instead (30 days),
  using `set_lifetime`.
- Logging in and out gives the session a new id (with the database driver) and a new CSRF
  token.
- Logging out ends the session on **this** device only. The user's other devices stay logged
  in; `auth::logout_other_devices` ends those.
- Changing the password ends the user's other sessions.

In templates: `flash.status`, `old('email')`, `errors`, `csrf_field()`.

> [!NOTE]
> **Coming from Laravel:** `session()->put/push/increment/flash` are
> `Session::put/push/increment/flash`.

## CSRF

Imagine a bad website with a hidden form that posts to your app's "delete my account" page.
If you're logged in, your browser would send your cookie along. That's a **CSRF** attack
(Cross-Site Request Forgery).

The fix: every form carries a secret **token** that only your app's own pages know. Every
POST, PUT, PATCH and DELETE must carry the session's token. Without it, the answer is
**419 Page Expired**.

To find the `_token` field, Renox reads the form's body first. A normal form over 2 MB (or a
file-upload form over `UPLOAD_MAX_SIZE`) gets **400** "The form is too large." instead.

How to send the token, depending on what sends the request:

- **Forms:** put `{{ csrf_field() }}` inside the `<form>`. It adds a hidden `_token` field.
  Both normal (urlencoded) and file-upload (multipart) forms work.
- **htmx:** nothing to do. `{{ renox_head() }}` puts the token in a `<meta name="csrf-token">`
  tag, and Renox's script sends it as `X-CSRF-Token` on every htmx request.
- **`fetch`:** send the `X-CSRF-Token` header yourself (read it from the meta tag).
- **API tokens** (`Authorization: Bearer`) skip CSRF: browsers never send them on their own,
  so the attack can't happen. A Bearer token that doesn't log anyone in gets 401.
- **No session at all** (for example, a payment gateway calling your app): use
  `.without_csrf()` on those routes, and check the request's signature instead.
  `Routes::webhook::<W>("/webhooks/stripe")` does both for you. It also needs
  `app.webhook::<W>()` in the module's `register`; without it, the app stops at start-up.
- **A JavaScript client on the same site** (axios, a small single-page app):
  `App::new().xsrf_cookie()` also sends the token as an `XSRF-TOKEN` cookie that scripts can
  read, and accepts it back in an `X-XSRF-TOKEN` header, as Laravel does. axios sends it by
  itself.

> [!NOTE]
> **Coming from Laravel:** `@csrf` is `{{ csrf_field() }}`, `VerifyCsrfToken::$except` is
> `.without_csrf()`, and the `XSRF-TOKEN` cookie is `App::xsrf_cookie()`.

## Method spoofing

HTML forms can only send GET and POST. So how does a form send a PUT or a DELETE? It pretends
("spoofs").

A POST with `_method=PUT|PATCH|DELETE` is routed as that method. The `_method` can be a form
field or the `X-HTTP-Method-Override` header. This happens before routing, so your `.put(…)`
and `.delete(…)` routes match.

```html
{# A form that updates a product: it's sent as POST, but routed as PUT. #}
<form method="post" action="{{ route('products.update', product.id) }}">
  {{ csrf_field() }}{{ method_field('PUT') }}
  …
</form>
```

`{{ method_field('PUT') }}` adds the hidden `_method` field.

> [!TIP]
> htmx doesn't need this: it can send the real method (`hx-delete`).

> [!NOTE]
> **Coming from Laravel:** `@method('PUT')` is `{{ method_field('PUT') }}`.

## Cookies

The session covers most needs. For a cookie of your own (a colour theme, a "cookies OK"
banner), read it with the `Cookies` extractor and set it by returning `SetCookie` in the
response:

```rust
use renox::prelude::*;
use renox::{Cookies, SetCookie};
use std::time::Duration;

/// Sets a `theme` cookie that lasts a year, then goes to the home page.
async fn remember_theme(State(state): State<AppState>) -> (SetCookie, Redirect) {
    let cookie = SetCookie::new(&state, "theme", "dark").max_age(Duration::from_secs(365 * 86_400));
    (cookie, Redirect::to("/"))
}

/// Reads the `theme` cookie ("" when there is none).
async fn theme(cookies: Cookies) -> String {
    cookies.get("theme").unwrap_or_default()
}

/// Reads and sets an encrypted cookie: the browser can't read or change it.
async fn secret(State(state): State<AppState>, cookies: Cookies) -> (SetCookie, String) {
    let seen = cookies.get_encrypted("seen").unwrap_or_default(); // tampered: None
    (SetCookie::encrypted(&state, "seen", "yes"), seen)
}
```

A new cookie starts with these settings:

- `Path=/`: sent with every address on the site;
- `HttpOnly`: page scripts can't read it;
- `SameSite=Lax`: not sent along when another site posts a form to yours;
- `Secure` when `APP_URL` is https: sent only over https;
- it lasts until the browser closes.

Change them with:

- `max_age` (how long it lasts);
- `path`;
- `readable_by_scripts` (no `HttpOnly`);
- `strict` (`SameSite=Strict`).

`SetCookie::remove(&state, name)` deletes a cookie. `SetCookie::encrypted` seals the value with
`APP_KEY`: the browser can neither read nor change it. If someone tampers with it,
`get_encrypted` gives `None`.

> [!NOTE]
> **Coming from Laravel:** `Cookie::queue` and encrypted cookies are `SetCookie::new` /
> `SetCookie::encrypted`, read with `Cookies`.

## Signed URLs

A **signed URL** is a link with a seal. Nobody can change it without breaking the seal, and it
stops working after a while. Use it for download links in mail, unsubscribe links and one-off
invitations.

```rust
use renox::prelude::*;
use renox::signed::ValidSignature;
use std::time::Duration;

/// The route that the signed link points to.
fn routes() -> Routes {
    Routes::new().get("/invoices/{id}", show_invoice).name("invoices.show")
}

/// Makes a signed link to invoice `id`.
async fn share(State(state): State<AppState>, Path(id): Path<i64>) -> Result<String> {
    // An absolute URL (with APP_URL) valid for an hour.
    state.signed_url("invoices.show", &[&id], Duration::from_secs(3600))
}

/// Shows the invoice, but only through a valid signed link.
async fn show_invoice(_: ValidSignature, Path(id): Path<i64>) -> String {
    format!("Invoice {id}") // an altered or expired link gets 403
}
```

What's going on:

- `share` makes a full link to `/invoices/{id}` that works for one hour.
- `show_invoice` takes `ValidSignature`. With a changed or expired link, Renox answers 403 and
  the handler never runs.

Good to know:

- The signature covers the path and the expiry time (it's an HMAC-SHA256 made with `APP_KEY`).
  So the link can't carry any other query string.
- `state.sign_path(path, ttl)` signs a path that isn't a named route.
- `renox::signed::verify(&state, &uri)` checks any URL (a path with its query) in a handler or
  middleware, and answers `true` or `false`.
- Changing (rotating) `APP_KEY` breaks every signed link already sent.
- Private storage files get temporary links with `state.storage.temporary_url(…)`.

### A signed link or a logged-in user

Some pages are for both: a guest opens them from a mail (a signed link), a customer from their
account. Take `Option<ValidSignature>`: it never refuses, and the handler decides.

```rust
use renox::prelude::*;
use renox::signed::ValidSignature;

async fn receipt(
    signature: Option<ValidSignature>,
    user: Option<AuthUser>,
    Path(order): Path<i64>,
) -> Result<String> {
    if signature.is_none() && user.is_none() {
        return Err(Error::Forbidden);
    }
    // A logged-in user still needs a check that the order is theirs (a policy).
    Ok(format!("Order {order}"))
}
```

The signature is checked against the whole URL, also inside a `Routes::group` (whose prefix
axum takes off the request's path).

> [!NOTE]
> **Coming from Laravel:** `URL::temporarySignedRoute` and the `signed` middleware are
> `state.signed_url(…)` and `ValidSignature`; `$request->hasValidSignature()` is
> `Option<ValidSignature>` or `renox::signed::verify`.

## Maintenance mode

When you update the app, you may want to close it for a moment and show "back soon" to
everyone.

```text
my-app down --secret s3cr3t --retry 60   # every request answers 503 (Retry-After: 60)
my-app up                                # back to normal
```

While the app is down:

- Every request gets **503 Service Unavailable**, using `errors/503.html` (or the built-in
  error page). `Retry-After: 60` tells clients to try again in 60 seconds.
- Visiting `/s3cr3t` (your secret) sets a cookie that lets **you** in, so you can check the
  site.
- `/health`, Renox's scripts and webhook routes keep working.

"Down" is a file, `storage/framework/down`, so every process on the machine sees it.

> [!NOTE]
> **Coming from Laravel:** this is `php artisan down --secret`.

## Request ids and client IPs

**Request id.** Every request gets an id, so you can find all the log lines of one request.

- If the request arrives with an `X-Request-Id` header that looks sane (8 to 64 letters,
  digits, `.`, `_`, `-`), Renox keeps it. Otherwise it makes one up.
- The id is sent back in the response's `X-Request-Id` header, written in every log line of the
  request, and included in error reports.
- Read it with the `RequestId` extractor, for example to show it on an error page.

**Client IP.** Behind Caddy, nginx or a load balancer, every connection seems to come from
that proxy. The proxy passes the visitor's real address in an `X-Forwarded-For` or `Forwarded`
header.

Anyone could send those headers, though. So Renox believes them only from `TRUSTED_PROXIES`:
addresses and CIDR ranges (a way to write a block of addresses, like `10.0.0.0/8`), or `*` for
whoever connects (for example, a hosting platform's load balancer).

```text
TRUSTED_PROXIES=127.0.0.1,10.0.0.0/8
```

`ClientIp`, rate limits, the login lock and the logs all use this same address.

**Trusted hosts.** With `TRUSTED_HOSTS=example.com,*.example.com`, a request for any other host
gets a 400. This matters because some links are built from the `Host` header (password reset
mails, redirects): without the check, a trick request could make them point at someone else's
site.

- `APP_URL`'s host is always allowed.
- `/health` answers whatever the host, since load balancers check it by IP address.
- When `TRUSTED_HOSTS` is not set, any host is allowed.

See [operations.md](operations.md) for timeouts, proxies and `/health` in production.

> [!NOTE]
> **Coming from Laravel:** `TrustProxies` is `TRUSTED_PROXIES`, and `TrustHosts` is
> `TRUSTED_HOSTS`.

## Coming from Laravel

If you know Laravel, this table maps what you know to Renox:

| Laravel | Renox |
|---|---|
| `routes/web.php` | `Module::routes()`, one module per area |
| `Route::get('/x', …)->name('x')` | `.get("/x", handler).name("x")` |
| `Route::prefix('admin')->name('admin.')->group(…)` | `.group("/admin", "admin.", routes)` |
| `Route::resource('products', …)` | `.resource("/products", "products", Resource::new()…)` |
| `Route::domain('{account}.example.com')` | `.domain("{account}.example.com", routes)` + `DomainParams` |
| `Route::fallback` | `.fallback(handler)` |
| `route('x', $id)`, `url()` | `route('x', id)` in templates, `state.url("x", &[&id])` |
| `request()->routeIs('admin.*')`, `Route::currentRouteName()` | `route_is('admin.*')`, `CurrentRoute` |
| `php artisan route:list` | `my-app route:list` |
| Route model binding, `{post:slug}` | `Found<Post>` (`{post}`, `{id}`, or a column's name such as `{slug}`) |
| `Route::view`, `Route::redirect`, `Route::permanentRedirect` | `.view(path, template)`, `.redirect(from, to)`, `.permanent_redirect(from, to)` |
| `SetCacheHeaders` with `etag` | `.etag()` |
| The `XSRF-TOKEN` cookie | `App::xsrf_cookie()` |
| `TrustHosts` | `TRUSTED_HOSTS` |
| `$request->query()`, `$request->ip()` | `Query<T>`, `ClientIp` |
| Middleware, `Kernel::$middleware` | `App::layer(from_fn(…))` |
| Route middleware | `.route_layer(…)` after the routes |
| `auth`, `guest`, `verified`, `can:`, `password.confirm` | `.require_auth()`, `.guest_only()`, `.require_verified()`, `.require_gate(…)`, `.require_password_confirmed()` |
| `throttle:60,1`, `RateLimiter::for` | `.throttle(60, Duration::from_secs(60))`, `App::rate_limiter` + `.throttle_by` |
| `config/cors.php` | `.cors(&[…])`, `.cors_layer(…)` |
| `redirect()->route()`, `->intended()`, `back()` | `Redirect::route`, `Redirect::intended`, `Back` |
| `response()->download()` | `Download` |
| `session()->put/push/increment/flash` | `Session::put/push/increment/flash` |
| `@csrf`, `@method('PUT')` | `{{ csrf_field() }}`, `{{ method_field('PUT') }}` |
| `VerifyCsrfToken::$except` | `.without_csrf()` |
| `Cookie::queue`, encrypted cookies | `SetCookie::new` / `SetCookie::encrypted`, `Cookies` |
| `URL::temporarySignedRoute`, `signed` middleware | `state.signed_url(…)`, `ValidSignature` |
| `php artisan down --secret` | `my-app down --secret …` |
| `TrustProxies` | `TRUSTED_PROXIES` |
| Service container bindings | `App::provide` + `Provided<T>` |
