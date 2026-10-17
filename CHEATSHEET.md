# Renox cheat-sheet

This page is a quick reference: one short example for each common task, written the way Renox
recommends. It's for when you know roughly what you want and need to see how it's written.
Every Rust example below is compiled and checked by `cargo test --doc -p renox`, so none of
them can drift out of date. For whole apps, see [`examples/`](examples): `hello`, the smallest, and `bikeshop`, a whole
business ([llms.txt](llms.txt) maps its files).

> [!TIP]
> New to Renox? Start with [the tutorial](docs/tutorial.md). It builds one small app step by
> step and explains every word. Come back here when you need a reminder.

## Commands

`rnx` is Renox's command-line tool. Type these in a terminal, inside your app's folder.

```bash
rnx new shop                         # or: --database postgres, --tailwind (Tailwind CSS, no Node)
rnx new desk --starter               # the starter kit: verification, roles, dashboard, users, activity log
rnx new acme --from ../tpl          # from your template: a directory or git+<url>#<tag or branch>; fills {{name}} {{title}}
                                     # {{crate_name}} {{database_url}} {{renox_dependency}} {{app_key}} (only .env) and each
                                     # [[ask]] of renox-template.toml (--set key=value in scripts); nothing is executed
rnx key:generate                     # APP_KEY into .env (made from .env.example if missing); --show only prints it
rnx doctor                           # checks Rust, linker, sccache, .env, APP_KEY, database, migrations
rnx serve                            # run, rebuild and reload on changes
rnx make:module products             # routes + view, registered in src/lib.rs
rnx make:module products --resource --fields "name:string price:money notes:text active:bool due_on:date"
                                     # model, migration, factory, form, 7 handlers, UI-kit views, tests
                                     # runs migrate (asks outside APP_ENV=local; --no-migrate), prints the URL; --open opens it
rnx make:package invoicing           # a reusable crate (Module, migrations, views, texts, tests) in crates/invoicing,
                                     # joined to the workspace and registered in the app; --path, --resource invoices
                                     # --fields "..", --with assets,commands, --register <crate> (virtual workspace)
rnx make:model Product --module products --migration   # --key ulid|uuid|string for other ids
rnx make:migration add_sku_to_products
rnx state:diagram Order             # a state machine as a Mermaid diagram (no name: lists the registered ones)
rnx make:migration --auto add_sku     # SQL from what changed in the models (db:diff); --yes skips questions
rnx make:policy Product --module products
rnx make:job SendReceipt --module products
rnx make:command products:import --module products  # typed (clap); `rnx products:import --help`
rnx make:mail order_shipped
rnx make:factory Product --module products  # also make:seeder DemoData, make:test checkout
rnx make:notification OrderShipped --module orders  # also make:event, make:rule (--module)
rnx make:middleware StampRequests    # on every route (App::layer)
rnx make:component price_tag         # --ui copies the UI kit into the app
rnx migrate                          # migrate:status, migrate:fresh --seed, db:seed
rnx migrate:rollback --step 2        # the last 2 batches (default 1)
rnx db:check                         # every App::model against the tables the migrations build; exits 1 on a mismatch
rnx route:list                       # db:shell, schedule:list, schedule:run NAME, cache:prune, session:prune, sequence:list
rnx queue:work --queue mail --workers 2  # --once: run what is queued, then stop
rnx queue:failed                     # queue:retry <id|all>, queue:forget <id>, queue:flush (deletes them)
rnx queue:prune-failed --hours 168   # failed jobs older than that (the default); queue:prune-batches [--hours 24]
rnx ui:publish                       # the UI kit (components/ui.html + its CSS) into the app; --force replaces
rnx schedule:work                    # tasks in their own process (SCHEDULER=false for serve)
rnx webhook:failed                   # webhook:retry <id>
rnx webhook:endpoints                # webhook:deliveries [--endpoint N] [--failed], webhook:redeliver <id>, webhook:prune-deliveries [--days 30]
rnx down --secret s3cret --retry 60  # 503 for everyone but visitors of /s3cret; `rnx up` ends it
rnx build                            # one release binary in dist/; rnx make:deploy
rnx make:pwa                         # installable on phones: offline page, icons, .pwa(..) (--force replaces)
rnx tailwind --watch                 # resources/css/app.css -> public/css/app.css (serve/build do it)
rnx tailwind --minify                # one build; rnx tailwind:install downloads the pinned CLI
```

`serve` already runs the queue workers and the scheduler. `queue:work` and `schedule:work` are for
running them in separate processes.

## App, module, routes (details in [docs/routing.md](docs/routing.md))

An app is made of modules, and each module is one feature (here, products). Its routes say
which function (a handler) answers which address (URL).

```rust
use renox::prelude::*;
use renox::clap;
use renox::command::AppCommand;
use std::time::Duration;

/// Builds the whole app. `src/main.rs` runs it, and tests start it too.
pub fn app() -> App {
    App::new()
        .embed(renox::embedded!())         // views, lang files, public/ inside the binary
        .migrations(renox::migrations!())  // migrations/*.sql
        .module(Auth::new())               // login, register, password reset
        .module(Products)
}

/// One feature of the app: everything about products.
pub struct Products;

impl Module for Products {
    fn name(&self) -> &'static str {
        "products"
    }

    /// The URLs this module answers, and the handler that runs for each.
    fn routes(&self) -> Routes {
        let public = Routes::new()
            .get("/products", index).name("products.index")
            .get("/products/{id}", show).name("products.show"); // axum 0.8: {id}, not :id; wildcard {*rest}
        let members = Routes::new()
            .post("/products", store)
            .name("products.store")
            .require_auth(); // covers the routes added before it
        let checkout = Routes::new()
            .post("/checkout", store).name("checkout")
            .require_verified()                     // logged in, with a verified email
            .throttle(10, Duration::from_secs(60)); // 10 a minute per user or IP, then 429
        let guests = Routes::new()
            .get("/welcome", index).name("welcome")
            .guest_only(); // logged-in users go to the `home` route
        public
            .merge(members)
            .merge(checkout)
            .merge(guests)
            .group(
                "/admin",   // path prefix: starts with `/`, never ends with one
                "admin.",   // name prefix
                Routes::new()
                    .get("/", index).name("dashboard")        // GET /admin, `admin.dashboard`
                    .get("/products", index).name("products") // GET /admin/products, `admin.products`
                    .require_auth(),                          // only the group's routes
            )
            // Another host: its own routes (the same paths may mean other pages there).
            // `{account}.example.com` works too; read it with the `renox::DomainParams` extractor.
            .domain("partners.example.com", Routes::new().get("/", index).name("partners.home"))
            // What answers when no route and no public file does (instead of the 404 page).
            .fallback(|| async { (StatusCode::NOT_FOUND, "Nothing here. Try the search.") })
    }

    fn register(&self, app: &mut Registry) {
        // `my-app products:import file.csv --dry-run` (or `rnx products:import …`)
        app.typed_command::<ImportProducts>();
        // Untyped: app.command("name", "about", |args: renox::command::Args, state| async { … })
    }
}

/// A handler: it answers a request with a page (a view) and the values it shows.
async fn index() -> View {
    view("products/index.html", context! { title => "Products" })
}

/// `Path(id)` reads the `{id}` part of the URL, such as 7 in `/products/7`.
async fn show(Path(id): Path<i64>) -> Result<View> {
    abort_if(id > 1_000_000, StatusCode::NOT_FOUND, "No such product.")?; // a status with a message
    Ok(view("products/show.html", context! { id }))
}

async fn store(session: Session) -> Result<Redirect> {
    let _back = Redirect::intended(&session, "/products"); // where a guard sent them from, else /products
    Redirect::route("products.show", &[&42]) // a named route: /products/42
}

async fn menu(route: renox::CurrentRoute) -> String {
    // The matched route's name; in templates `route_is('admin.*')` and `request.route`.
    format!("{} {}", route.name().unwrap_or("-"), route.is("products.*"))
}

/// Import products from a CSV file.     (the line in `my-app help`)
#[derive(clap::Parser)]
#[command(name = "products:import")]
struct ImportProducts {
    /// The CSV file; asked for when it's left out.
    file: Option<String>,
    #[arg(long)]
    dry_run: bool,
}

impl AppCommand for ImportProducts {
    async fn run(self, state: AppState) -> Result {
        let file = match self.file {
            Some(file) => file,
            None => renox::prompt::ask("CSV file").await?, // also ask_or, secret, confirm, choice
        };
        if !self.dry_run && !renox::prompt::confirm(&format!("Import {file}?"), true).await? {
            return Ok(());
        }
        let _ = state;
        Ok(())
    }
}

async fn download(order_paid: bool) -> Result<&'static str> {
    // Any status with a message visitors see (page or JSON `message`).
    abort_unless(order_paid, StatusCode::PAYMENT_REQUIRED, "Pay for the order first.")?;
    // or: return Err(abort(StatusCode::GONE, "This offer ended."));
    Ok("the file")
}
```

Wrong arguments print what's wrong and the usage; `--help` lists them. In tests:
`app.kernel().call("products:import", ["a.csv", "--dry-run"]).await`, and
`renox::prompt::answering(["a.csv", "yes"], app.kernel().call("products:import", [""; 0])).await`
answers the questions.

`src/main.rs` is only `fn main() -> renox::Result { shop::app().run() }`. The app lives in the
library, so tests can boot it.

When a handler won't compile as a route ("the trait `Handler<_, _>` is not implemented", or
"`Send` is not general enough"), look for a closure or an iterator that borrows local variables
and is still alive across an `.await`. Collect into a `Vec` first, then await. Also,
`.require_auth()` covers only the routes added before it: a route added after it is public.

## Views (MiniJinja)

Views (templates) are HTML files with blanks the app fills in. `{{ … }}` prints a value,
`{% … %}` runs logic such as `if` and `for`, and `{# … #}` is a comment.

```html
{% extends "layouts/app.html" %}
{% block content %}
<h1>{{ title }}</h1>
<a href="{{ route('products.edit', product.id) }}">Edit</a>   {# named route with parameters #}
<a href="{{ route('products.index', sort='price', q=q) }}">…</a> {# named arguments: ?q=…&sort=price (none is left out) #}
<img src="{{ asset('logo.png') }}">                           {# /logo.png?v=hash: cached a year, new URL on change #}
<p>{{ t('shop.welcome', name=auth.user.name) if auth.check }}</p>
{% if flash.status %}<p class="flash">{{ flash.status }}</p>{% endif %}
{% if can('admin') %}<a href="/admin">Admin</a>{% endif %}    {# gate #}
<p>{{ product.price | number }}</p>                            {# 75,000 (en) / 75.000 (es); number(2) #}
<p>{{ order.created_at | date('%d/%m/%Y %H:%M') }}</p>        {# in APP_TIMEZONE; default %Y-%m-%d #}
<p>{{ order.total | money }}</p>                               {# cents → $75.00 (APP_CURRENCY=USD); money(currency='EUR'), money(divide_by=1) for whole units #}
<p>{{ order.created_at | since }}</p>                          {# 3 hours ago / in 2 days #}
<p>{{ post.body | words(30) }}</p>                             {# the first 30 words… #}
<div>{{ post.body | markdown }}</div>                          {# HTML typed in is shown as text #}
<span>{{ cart_count }}</span>                                  {# from App::share #}
<a class="{{ class_names('tab', {'tab-active': route_is('products.*')}) }}">Products</a> {# @class #}
{% for p in products %}{% if loop.index > 3 %}{% break %}{% endif %}{{ p.name }}{% endfor %} {# also continue #}
{% endblock %}
```

With `APP_DEBUG` on, printing a variable that doesn't exist (`{{ prodcut.name }}`) is an error
page showing the request, the error and the template line; `{% if x %}` on a missing one is fine,
and so is `{{ flash.anything }}`.

Every view also gets: `request.path`, `request.query`, `request.route` (the route's name),
`request.htmx`, `request.boosted` (`hx-boost`), `app.name`, `app.env`,
`app.debug`, `app.url`, `app.locale`, `app.dir` (`ltr` or `rtl`), `app.currency` (`APP_CURRENCY`), `auth.check`, `auth.user`, `auth.roles`, `flash`, `errors`,
`csrf_token` (the CSRF token as text, for a header or your own hidden field),
and the functions `old()`, `has_old()`, `error(field)` (also `error(field, bag='login')`),
`errors_in('login')` (every error of a named bag), `csrf_field()`,
`method_field('PUT')` (the hidden `_method` field), `route()`, `asset(path)` (a `public/` file's
URL with `?v=hash`), `storage_url(key)` (a stored file's URL on the default disk),
`t()`, `can()`, `route_is(pattern, …)`, `class_names(…)`, `page_url(n)` (this page's query
with `page=n`),
`query_with(key=value)` (this page's query with those keys set, or removed with `none`; `page` dropped),
`query_fields("key", …)` (this page's query as hidden inputs, without `page` and those keys, for GET forms),
`chart(kind, data, …)` (an SVG chart: line, area, bar, pie, doughnut, scatter, bubble, heatmap),
`renox_head()`, `csp_nonce()` (this request's nonce for `<script nonce=…>` under `CSP=strict`),
`seo(title=…, description=…, image=…, type=…, canonical=…)` (see "SEO and analytics" below),
`renox_ui()` (the UI kit), `renox_grid()` (the data grid's assets), `renox_calendar()` (the
date picker's calendar; the kit's `date_picker` adds it once), `sparkline(values)`, `toasts()`, `once(key)`, `stack(name)` and, with `{% call %}`,
`push(name)` / `prepend(name)`.

Error pages are the app's own: `rnx new` writes `resources/views/errors/default.html`, which
extends the layout and gets every global above plus `status`, `reason` and `detail`;
`errors/404.html` (any status) wins for one status. Without them, Renox shows its own.

Renox's own pages are templates you can replace: create the same file under `resources/views/`,
e.g. `renox/error.html`, `renox/auth/login.html` (also
`register`, `forgot-password`, `reset-password`, `verify-email`, `account`, `confirm-password`,
`layout`), `renox/mail/layout.html`, `renox/mail/button.html`, `renox/mail/components.html`,
`renox/mail/auth/reset-password.html` / `.txt`, `renox/mail/auth/verify-email.html` / `.txt`,
`renox/queue/dashboard.html`, `renox/notifications.html`, `renox/grid.html` and
`renox/pagination.html` (`rnx ui:publish` copies the kit, `renox/ui.html`, as `components/ui.html`). The pagination macros must be imported:
`{% from "renox/pagination.html" import pagination, simple_pagination %}` (`simple_pagination`
for a `simple_paginate` page).

```rust
use renox::prelude::*;
use renox::view::ViewContext;

/// Adds a template filter, and a value every view can use.
fn view_extras(app: App) -> App {
    app.templates(|env| {
        // Your own filters and functions (MiniJinja's API).
        env.add_filter("initials", |name: String| {
            name.split_whitespace().filter_map(|w| w.chars().next()).collect::<String>()
        });
    })
    // In every view, computed per request (cache what doesn't change per request).
    .share("cart_count", |ctx: ViewContext| async move {
        // A guest's cart kept in the session: ctx.session.and_then(|s| s.get::<Vec<i64>>("cart"))
        let Some(user) = ctx.user else { return Ok(0) };
        let n: i64 = renox::db::sql("SELECT COUNT(*) FROM cart_items WHERE user_id = ?")
            .bind(user.id)
            .scalar(&ctx.state.db)
            .await?;
        Ok(n)
    })
}
```

A typed view context: a struct per page that names its template. `rnx view:check` (and
`assert_views_compile`) reports a variable the template uses that the struct lacks (besides
the globals above, the `App::share` keys and what the template sets itself); it doesn't follow
`extends`/`include`/macros. Details in [docs/ui.md](docs/ui.md#typed-view-contexts).

```rust
use renox::prelude::*;

#[derive(serde::Serialize, ViewData)]
#[view(template = "suppliers/index.html")]
struct SupplierList {
    suppliers: Vec<String>,
    #[serde(rename = "total")] // the template's name for it; `#[serde(skip)]` fields aren't variables
    count: usize,
}

async fn suppliers() -> View {
    View::typed(SupplierList { suppliers: vec![], count: 0 }) // the template comes from the type
}

/// Registered so `view:check` knows the struct (in a module: `app.view_data::<T>()` in `register`).
fn typed_views(app: App) -> App {
    app.view_data::<SupplierList>()
}
```

## Components and the UI kit (details in docs/ui.md)

The UI kit is a set of ready-made page parts: fields, buttons, menus, tables and more. **Write
pages with `rx-*` tags** (the next section lists them); a kit macro is for the few parts that
have no tag (marked below). Tag props are in `crates/renox-core/src/components/contracts.rs`,
and `rnx view:check` names a wrong one with its file and line.

```html
{# layout: {{ renox_head() }}{{ renox_ui() }} in <head>, <body class="rx-page">, {{ toasts() }} #}
{# The page's frame: public pages use rx-navbar; a dashboard app (rnx new's signed-in layout) uses rx-sidebar in <rx-shell fill> #}
{# Spacing: kit parts have no outer margin, their container spaces them. Start a page with <rx-stack>,
   put buttons in <rx-row> (<rx-row end> for the right), fields in rx-card / rx-form-grid / rx-stack;
   main.rx-container and <rx-form> add no space (docs/ui.md#spacing-put-siblings-in-rx-stack-or-rx-row) #}
<rx-navbar brand="{{ app.name }}" :href="route('home')">
  <rx-nav-links><rx-nav-link route="products.index" label="Products" :active="route_is('products.*')"/></rx-nav-links>
</rx-navbar>
<rx-page-header title="Products" subtitle="All items" :back="route('home')"/>
<rx-toolbar><rx-link-button route="products.create" label="New product"/></rx-toolbar>   {# above a table #}
{# in a table row: <rx-row-actions>…<rx-icon-button/>…</rx-row-actions>; also <rx-list>, <rx-card-grid> + <rx-media-card href title image>,
   <rx-link-tabs :items current>, <rx-progress value="42"/> #}
{# Phones: <rx-navbar :tabs="[{'href': …, 'label': 'Shop', 'icon': 'shopping-bag', 'active': route_is('shop.*')}, {'open': 'menu-sheet', 'label': 'Menu', 'icon': 'menu'}]"> = a bottom tab bar under 36rem;
   <rx-nav-search><form role="search">…</form></rx-nav-search> = search behind a button there; rx-hide-narrow leaves the phone's bar #}
{# Icons (Lucide, docs/ui.md#icons): <rx-icon name="bike"/> <rx-icon name="truck" size="16"/>;
   hidden from screen readers unless <rx-icon name="lock" label="Private"/>; the same names in icon="…" on buttons and menus #}
{# A dashboard app (the standard: docs/ui.md#the-dashboard-standard-rx-shell-fill): <body class="rx-page"><rx-shell fill>,
   (modules' links: <rx-sidebar brand="{{ app.name }}" menu="staff"/>, <rx-nav-links menu="portal"/>; `nav('staff')` in custom markup),
   then <rx-sidebar brand="{{ app.name }}"><rx-sidebar-section title="Sales"/><rx-sidebar-link route="…" label="…"/></rx-sidebar>
   and <div class="rx-shell__main"><rx-navbar width="full" :skip="false"/><main class="rx-shell__content" id="main">…</main></div></rx-shell>.
   On a desktop the window never scrolls, each panel does, and the page's work fills the screen: <rx-grid> straight in the
   content takes the height left; else <div class="rx-panes rx-fill"><section class="rx-pane">…</section>…</div>
   (--rx-panes: 2fr 1fr) or one <section class="rx-pane rx-fill">; an empty one holds <rx-empty>. Tablets and phones scroll the page.
   <rx-thumbnail :src="p.image"/>, <rx-columns count="2">, <rx-menu-button hx-get="{{ url }}">Edit</rx-menu-button> in a menu,
   <rx-action-group>, <rx-wizard-action> + <rx-wizard-step>, <rx-import-action route="…">, <rx-event-stream/>.
   Tabs from data: <rx-tabs id="t" :items="[['a', 'A']]"><rx-tab-panel rx-for=… id="t" :key=… :selected=…>; <rx-link-tabs><rx-link-tab rx-for="v in views" :href :current>label</rx-link-tab></rx-link-tabs>.
   A table written by hand with a total: <rx-simple-table :head="['Item', ['Qty', 'num']]"><tr rx-for=…>…</tr><rx-slot name="footer"><tr>…</tr></rx-slot></rx-simple-table>
   (<rx-table> has the same footer slot). A text mail: <rx-mail-text>…{{ x }}…<template rx-for="r in rows">…</template></rx-mail-text>.
   Mails: <rx-mail subject="…"> + <rx-mail-button :url label/>, <rx-mail-panel>, <rx-mail-table :rows :head :total/>, <rx-mail-divider/>
   (<rx-slot name="footer"> replaces the footer). hide-label on rx-input, rx-textarea,
   rx-select or rx-checkbox (label kept for screen readers; the other fields don't take it),
   <body class="rx-page rx-page--fill"> for a navbar page that fills the window (a full-screen grid).
   Themes: the editorial default (teal, cream, pill buttons; Inter + Poppins, served by Renox);
   data-rx-theme="warm" (indigo, before 1.1) or "classic" (the first look) on <html>; tokens
   --rx-accent-2, --rx-tint, --rx-accent-soft, --rx-ink, --rx-type-hero, --rx-type-section; your CSS uses the type scale: font: var(--rx-type-heading) (also -display,
   -title, -lead, -body, -label, -note, -caption) and the brand token --rx-accent. #}
{# Several steps: <rx-wizard id="signup" submit-label="Save"><rx-wizard-step key="a" title="First">…</rx-wizard-step>…</rx-wizard> #}
<rx-form action="/products" live novalidate>        {# method post and csrf_field() are added; a file field needs enctype="multipart/form-data" #}
  <rx-card title="New product">
    <rx-form-errors/>
    <rx-input name="name" label="Name" required hint="Shown on the list"/>
    <rx-select name="size" label="Size" :options="[['s', 'Small'], ['m', 'Medium']]"/>
    <rx-checkbox name="featured" label="Featured" switch/>
    <rx-form-grid columns="2">
      <rx-input name="price" label="Price" type="number" prefix="$" :attrs="{'step': '0.01'}"/>
      <rx-radio name="unit" label="Unit" :options="['pcs', 'kg']" selected="pcs" inline/>
    </rx-form-grid>
    <rx-checkbox-list name="tags" label="Tags" :options="[['new', 'New'], ['sale', 'On sale']]"/>   {# Vec + #[serde(default)] #}
    <rx-toggle-buttons name="delivery" label="Delivery" :options="[['courier', 'Courier'], ['pickup', 'Pickup']]" selected="courier"/>
    <rx-show-when field="delivery" values="courier"><rx-input name="address" label="Address"/></rx-show-when>   {# required_if #}
    <rx-date-picker name="ships_on" label="Ships on"/>                {# NaiveDate #}
    <rx-date-picker name="visit_on" label="Visit" :disabled-dates="full_days" :closed-weekdays="[0]"/>   {# 0 = Sunday; also none_of on the server #}
    <rx-file name="photo" label="Photo" accept="image/*" preview/>    {# the form needs enctype="multipart/form-data" #}
    <rx-input name="api_key" label="API key" :value="key" readonly copyable/>
    <rx-tags-input name="keywords" label="Keywords"/>                 {# Vec<String> + #[serde(default)] #}
    <rx-select name="cats" label="Categories" :options="cats" multiple searchable/>
    <rx-select name="customer_id" label="Customer" :options="[[c.id, c.name]]" :selected="c.id" options-url="{{ route('customers.options') }}" editable/>   {# renox::select: GET ?q= / ?values=, POST label, PUT value+label #}
    <rx-key-value name="specs" label="Specs" :value="product.specs"/>   {# renox::KeyValues #}
    <rx-repeater name="lines" label="Lines" :rows="order.lines">
      <rx-input name="{{ prefix }}[name]" label="Name" :value="row.name"/>   {# lines[0][name] → Vec<Line>, v.nested #}
    </rx-repeater>
    <rx-button label="Create product"/>
  </rx-card>
</rx-form>
<rx-confirm id="del-7" label="Delete" action="/products/7" title="Delete “Coffee”?" message="It goes to the trash."/>

{# Actions: a form in a sheet, sent with htmx (422 → errors in the sheet, success → closes).
   The handler answers (Toast, HxRefresh), or a ValidationError for its own checks. #}
<rx-action-sheet id="stock-7" label="Stock" action="{{ route('products.stock', 7) }}" title="Adjust stock" method="PUT" icon="box">
  <rx-input name="change" label="Change" type="number" id="stock-change-7"/>   {# slide-over, width="lg" #}
</rx-action-sheet>
<rx-icon-button icon="edit" label="Edit Coffee" href="/products/7/edit"/>     {# label = aria-label + tooltip #}
<rx-button label="Save" key="mod+s" icon="check"/>                            {# ⌘S / Ctrl+S; also badge="3" #}
<rx-button label="Publish" disabled-reason="Add a photo first."/>             {# focusable, reason as tooltip #}
{# More actions behind one "⋯" button (label → a labelled button); menu_open opens a sheet made with button=false.
   action_group, wizard_action and import_action have no tag: macros. #}
{% from "renox/ui.html" import action_group, wizard_action, wizard_step, import_action %}
{% call action_group() %}
  <rx-menu-link href="{{ route('products.replicate', 7) }}" label="Duplicate" icon="copy"/>
  <rx-menu-link href="{{ route('products.ledger', 7) }}" label="Export ledger" icon="download" download/>
  <rx-menu-open id="import-products" label="Import…" icon="upload"/>   {# also <rx-menu-action>, <rx-menu-section title> #}
{% endcall %}
{{ import_action("import-products", "Import", route('products.import'), "Import products", columns=["sku", "name"], template_url=route('products.template'), button=false) }}
{% call wizard_action("new-product", "New product", route('products.store'), "New product", [["a", "Details"], ["b", "Price"]], submit_label="Add") %}
  {% call wizard_step("new-product", "a") %}<rx-input name="name" label="Name" id="np-name"/>{% endcall %}
  {% call wizard_step("new-product", "b") %}<rx-input name="price" label="Price" type="number" id="np-price"/>{% endcall %}
{% endcall %} {# a 422 opens the step with the error #}

{# Read-only details (an infolist): labels and values, formatted #}
<rx-infolist columns="2">
  <rx-entry label="Number" :value="order.number" copyable/>
  <rx-entry label="Status" :value="order.status" :badge="{'paid': 'success', 'pending': 'warning'}" :labels="{'paid': 'Paid'}"/>
  <rx-entry label="Placed" :value="order.created_at" format="since"/>   {# date, datetime, money, number, bool, color, image, markdown, key_value #}
  <rx-entry label="Total" :value="order.total" format="money"/>
  <rx-entry label="Tags" :value="order.tags" badge :limit-list="3"/>   {# lists: list="lines" / "bullets" #}
  <rx-entry label="Customer"><a href="{{ route('customers.show', order.customer_id) }}">{{ order.customer }}</a></rx-entry>
  <rx-repeatable label="Items" :items="order.lines" columns="3"><rx-entry label="Product" :value="line.name"/></rx-repeatable>
  {# Buttons beside a value: :suffix-actions="[{'label': 'Edit', 'icon': 'edit', 'url': route('users.edit', user.id)}]" (url, or action for a posted form, or attrs for htmx) #}
</rx-infolist>

{# Rich text, Markdown and code editors, and a code entry: the renox-editors crate,
   .module(renox_editors::Editors::new()) (docs/editors.md). Each sends a plain field. #}
<rx-rich-editor name="body" label="Body" :value="post.body" required/>   {# HTML: read it as RichText #}
<rx-markdown-editor name="notes" label="Notes" :value="post.notes"/>     {# toolbar + Preview #}
<rx-code-editor name="config" label="Config" :value="post.config" language="json"/>
{{ post.body | rich_text }}                                              {# stored HTML, cleaned again #}
<rx-code-entry label="Payload" :value="call.payload" language="json"/>   {# in an infolist: coloured, copyable #}
{# Your own: rnx make:component price_tag -> components/price_tag.html, used as <app-price-tag>; it can use
   old(), error(), t(), can(), auth, csrf_field() like the page. rnx make:component --ui copies
   the kit into the app. {% if once('x') %} renders once per page.
   Also tags: rx-textarea, rx-link-button, rx-alert, rx-badge, rx-sheet + rx-open-button,
   rx-menu / rx-menu-link / rx-menu-action / rx-menu-separator, rx-tabs / rx-tab, rx-empty.
   Macros with no tag: group, tab_panel. renox_ui(styles=false) loads only the script (styles published with ui:publish). #}

{# The layout's plumbing as tags: <rx-head/> {{ renox_head() }}, <rx-ui/> {{ renox_ui() }} (<rx-ui :styles="false"/>), <rx-theme/> {{ renox_theme() }} (the brand stylesheet, after rx-ui),
   <rx-toasts/> {{ toasts() }}, <rx-outlet stack="scripts"/> {{ stack('scripts') }}; <rx-outlet name="content"/> a layout block
   a page fills (children = its default; name or stack, not both); <rx-pagination :page="products"/> the page links. #}

{# Control flow: rx-if / rx-elif / rx-else / rx-for on any element or component; <template rx-if="a">text and siblings</template>
   <template rx-else>…</template> and <template rx-for="x in xs">…</template> write their children with no wrapper.
   <rx-let name="label" :value="a ~ b"/> names a value for the rest of its element (a {% with %}; the name must be free).
   <rx-include template="parts/card.html"/> / <rx-include :template="name" optional/> draws another template ({% include %}; optional = ignore missing).
   ?name="expr" is an attribute that is left out when expr is false / none, bare for true, name="text" otherwise:
   <a href="/x" ?aria-current="'page' if current">, <li ?data-done="step.done">; works on plain elements and on the kit's tags.
   <rx-script>…</rx-script> / <rx-style>…</rx-style> are <script> / <style> with nonce="{{ csp_nonce() }}". #}

{# Stacks: the layout has {{ stack('head') }} in <head> and {{ stack('scripts') }} before
   </body>; any page, block or component adds to them, even after the head was rendered
   (<rx-push>; `prepend` has no tag, so it stays a macro): #}
<rx-push stack="scripts" once="chart"><rx-script src="{{ asset('chart.js') }}"></rx-script></rx-push>   {# <rx-script>/<rx-style> add nonce="{{ csp_nonce() }}" #}
{% call prepend('head') %}<meta name="robots" content="noindex">{% endcall %}
```

Import, duplicate and export (Filament's import, replicate and export actions):

```rust
use renox::grid::{Column, ExportFormat, Grid, GridRequest};
use renox::import::{Import, ImportReport};
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "products")]
pub struct Product { pub id: i64, pub sku: String, pub name: String }

/// One CSV row, checked like a form (its rules, prepare and after hooks).
#[derive(serde::Deserialize, Validate)]
pub struct ProductRow {
    #[validate(required, alpha_dash)]
    pub sku: String,
    #[validate(required, max = 100)]
    pub name: String,
}

#[derive(serde::Deserialize, Validate)]
pub struct ImportForm {
    #[validate(required, mimes(&["csv", "txt"]))]
    pub file: Option<Upload>,
}

/// Good rows written in one transaction (a savepoint each); the answer is a toast, or the
/// refused rows with their row numbers in the import_action sheet.
/// Also .all_or_nothing(), .delimiter(';'), .rename("Description", "name"), .max_rows(n).
pub async fn import(State(state): State<AppState>, lang: Lang, Valid(form): Valid<ImportForm>) -> Result<ImportReport> {
    let file = form.file.ok_or(Error::NotFound)?;
    Import::csv(file.bytes()).lang(&lang).run(&state, |tx, row: ProductRow| Box::pin(async move {
        Product::create(tx, Product { sku: row.sku, name: row.name, ..Default::default() }).await?;
        Ok(())
    })).await
}

pub async fn template() -> renox::Download {
    renox::import::template("products.csv", &["sku", "name"])
}

/// Excel too: `from_upload` picks CSV or Excel by content (`Import::xlsx(bytes)` and `.sheet("Items")`
/// with the `xlsx` feature; cells read as the text a CSV holds, so the same rules check both).
/// `.dry_run_if(true)` checks every row and writes nothing (`<rx-import-action check>` adds the box).
/// `.undoable(window)` keeps what the closure wrote through its `tx.insert/update/save(&mut model)`.
pub async fn import_any(State(state): State<AppState>, Valid(form): Valid<ImportForm>) -> Result<ImportReport> {
    let file = form.file.ok_or(Error::NotFound)?;
    Import::from_upload(&file)
        .dry_run_if(false)
        .undoable(std::time::Duration::from_secs(86_400))
        .run(&state, |tx, row: ProductRow| Box::pin(async move {
            tx.insert(&mut Product { sku: row.sku, name: row.name, ..Default::default() }).await
        }))
        .await // report.batch_id; report.error_csv("errors.csv", ErrorFile::WholeFile) is the file to fix
}

/// Puts the batch back: deletes what it inserted, restores what it updated, in one transaction.
/// Refuses with a message when the window passed, a row changed or vanished since (`updated_at`),
/// or another table still uses a row. `Undo::batch(id).can_undo(|tx, change| …)` adds your rule.
pub async fn undo_import(State(state): State<AppState>, Path(batch): Path<i64>) -> Result<Toast> {
    let undone = renox::import::undo(&state, batch).await?; // `my-app import:prune` forgets old batches
    Ok(Toast::success(format!("{} deleted, {} restored.", undone.deleted, undone.restored)))
}

/// With the `xlsx` feature: the Excel template (headings, an example row, typed columns).
#[cfg(feature = "xlsx")]
pub async fn template_xlsx() -> Result<renox::Download> {
    use renox::import::TemplateColumn;
    renox::import::template_xlsx("products.xlsx", &[TemplateColumn::text("sku", "COFFEE-1"), TemplateColumn::text("name", "Coffee")])
}

/// "Duplicate": a copy with no id and no timestamps, for the new-product form.
pub async fn replicate(State(db): State<Db>, Path(id): Path<i64>) -> Result<View> {
    let mut product = Product::find_or_404(&db, id).await?.replicate();
    product.sku.clear();
    Ok(view("products/create.html", context! { product }))
}

/// A file of any query, outside a grid's page (the grid only names the columns).
pub async fn export(Path(format): Path<ExportFormat>, request: GridRequest) -> Result<Response> {
    let grid = Grid::new("products").column(Column::text("sku", "SKU")).column(Column::text("name", "Name"));
    grid.export_as(Product::query().order_by(Product::NAME), format, &request).await
}
```

Tailwind: `rnx new shop --tailwind` (or create `resources/css/app.css` with
`@import "tailwindcss"; @source "../views";`). `rnx serve` rebuilds `public/css/app.css` as views
change, `rnx build` minifies it; link it with `{{ asset('css/app.css') }}`. The kit's `rx-*`
classes keep working next to the utilities.

### Components as tags (details in docs/ui.md "Components as tags")

```html
{# <rx-…> and <app-…> tags are rewritten to MiniJinja when a template loads (not compiled: the file is html) #}
<rx-page layout="layouts/app.html" title="{{ t('products.title') }}">
  <rx-page-header title="Products"><rx-link-button route="products.create" label="New" variant="primary"/></rx-page-header>
  <rx-table :rows="products" caption="Products">                  {# list or page; pagination is added #}
    <rx-column label="Name">{{ row.name }}</rx-column>
    <rx-column label="Price" align="num">{{ row.price | money }}</rx-column>
    <rx-row-actions><rx-icon-button can="update" route="products.edit" icon="pencil" label="Edit"/></rx-row-actions>
    <rx-slot name="empty"><rx-empty title="No products"/></rx-slot>
  </rx-table>
  <rx-form action="{{ route('products.store') }}"><rx-input name="name" label="Name" required/><rx-button>Save</rx-button></rx-form>
  <rx-repeater name="hours" label="Hours" :rows="hours" max="7">  {# row and prefix in the content #}
    <rx-input name="{{ prefix }}[opens]" label="Opens" type="time" :value="row.opens"/>
  </rx-repeater>
  <rx-wizard id="signup" submit-label="Create"><rx-wizard-step key="a" title="Name"><rx-input name="name" label="Name"/></rx-wizard-step></rx-wizard>
  <rx-navbar brand="Shop"><rx-nav-links><rx-nav-link route="orders.index" label="Orders" :active="route_is('orders.*')"/></rx-nav-links></rx-navbar>
  <rx-menu label="Account" id="acct"><rx-menu-link route="account.show" label="Account"/><rx-menu-separator/><rx-menu-action route="logout" label="Log out"/></rx-menu>
  <rx-tabs id="t"><rx-tab key="a" label="Items">…</rx-tab><rx-tab key="b" label="History">…</rx-tab></rx-tabs>   {# tabs come from the children #}
  <rx-open-button id="note" label="Add note"/><rx-sheet id="note" title="Note">…</rx-sheet>
  <rx-stats columns="3"><rx-stat label="Revenue" :value="revenue | money" :delta="change"/></rx-stats>
  <rx-chart kind="line" :labels="labels" :series="series" title="Sales" id="sales"/>   {# calls chart(...) #}
  <rx-grid :page="orders"><template rx-if="column.key == 'actions'">…</template></rx-grid>        {# row, column in the content #}
  <li rx-for="p in products" rx-if="p.active">{{ p.name }}</li>   {# rx-else on the next sibling #}
  <app-price-tag :amount="p.price"/>                              {# components/price_tag.html, starts with <rx-props amount> #}
</rx-page>
{# value forms: x="text"  x="{{ expr }}"  x="a {{ b }}"  :x="expr"  bare = true. Errors show file:line. #}
```

The renox-blocks crate (`.module(Blocks::new())`, guide docs/blocks.md) adds tags of its own, among them two displays:

```html
<rx-status-stepper :steps="states" current="approved" orientation="vertical"/>   {# steps: [{key, label, tone?, terminal?}]; aria-current="step" #}
<rx-week-calendar :resources="bays" :events="bookings" start="2026-10-12"        {# resources: [{id, label}]; events: [{resource, start, end, label, tone?, href?}], RFC 3339 #}
  slot-url="/bookings" slot-target="#week" id="week"/>                           {# a free slot POSTs resource, start, end (hx-post + static hx-vals); days, from, to, step #}
```

### Component events (details in docs/ui.md "Components and the page: three directions")

```html
{# kit components send bubbling rx:<component>:<event> events: sheet/action-sheet opened closed saved failed,
   confirm confirmed cancelled, tabs/wizard/select/tags-input/date-picker changed, repeater added removed,
   grid selected sorted filtered. Extra attributes go to the root (class is merged; use x-bind:name, not :name). #}
<rx-action-sheet id="new-item" title="New item" action="{{ route('items.store') }}" @saved="reload = true">…</rx-action-sheet>
<div x-data="{ tab: '' }" @rx:tabs:changed.window="tab = $event.detail.name">…</div>
<div hx-get="/cart/summary" hx-trigger="rx:select:changed from:body">…</div>
```

## Your own shared values and middleware

Give every handler the same service (here, a payment client), and run your own code around
each request. Code that runs around requests is called middleware.

```rust
use renox::prelude::*;
use renox::Provided;
use renox::axum::extract::Request;
use renox::axum::middleware::{Next, from_fn};

#[derive(Clone)]
struct Payments { api_key: String }

/// `Provided<T>` hands the handler the value given to `App::provide`.
async fn pay(payments: Provided<Payments>) -> String {
    // In jobs, listeners, commands and tasks: state.provided::<Payments>()
    format!("key starts with {}", &payments.api_key[..3])
}

#[derive(Clone)]
struct Plan(&'static str);

/// A middleware: code that runs before and after the handler of each request.
async fn stamp(user: Option<AuthUser>, req: Request, next: Next) -> Response {
    renox::context::set(Plan(if user.is_some() { "member" } else { "guest" }));
    let mut res = next.run(req).await; // runs after the session and user are loaded
    res.headers_mut().insert("x-member", user.is_some().to_string().parse().unwrap());
    res
}

// A context value as an argument (500 if missing; `Option<Current<Plan>>` when optional).
async fn plan(renox::context::Current(plan): renox::context::Current<Plan>) -> &'static str {
    plan.0
}

/// Registers the shared value and the middleware on the app.
fn wiring(app: App) -> App {
    let _ = plan;
    app.provide(Payments { api_key: "sk_test_123".into() })
        .layer(from_fn(stamp)) // every route of the app's modules
}

fn plan_routes() -> Routes {
    // Only these routes: like require_auth, it wraps the routes added before it.
    Routes::new().get("/plan", plan).route_layer(from_fn(stamp))
}
```

## Form + validation (details in [docs/validation.md](docs/validation.md))

Validation checks what people send before your code uses it. Write the rules once; `Valid<T>`
runs them and, when something is wrong, shows the errors next to the fields.

```rust
use renox::prelude::*;
use serde::Deserialize;

/// What the form sends: one field per input, matched by the input's `name`.
#[derive(Deserialize)]
struct ProductForm {
    name: String,
    price: i64,
    email: Option<String>,
    sku: String,
    kind: String,
    brand: Option<String>,
    launch: renox::chrono::NaiveDate,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    photos: Vec<Upload>, // <input type="file" name="photos" multiple>
}

/// The rules each field must pass. One field can have several rules in a row.
impl Validate for ProductForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required().max(100).unique("products", "name");
        v.field("price", &self.price).min(0);
        v.field("email", &self.email).email();
        v.field("sku", &self.sku).matches(r"^[A-Z]{2}-\d{4}$").none_of(&["XX-0000"]);
        v.field("brand", &self.brand).required_if(self.kind == "branded"); // also required_with, required_unless
        v.field("launch", &self.launch).after(renox::chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        v.field("tags", &self.tags).max(5);
        v.each("tags", &self.tags, |tag| tag.required().max(20)); // errors on tags.0, tags.1, …
        v.each("photos", &self.photos, |photo| photo.image().max(2048));
        // Also: digits(n), digits_between(a, b), date(), before…, after_or_equal(d), one_of, same, different,
        // alpha, alpha_num, alpha_dash, lowercase, uppercase, starts_with(&["08"]), ends_with,
        // uuid, ulid, ip, mac_address, json, timezone, hex_color, ascii, size(n), required_without(&other),
        // required_with_all(&[&a, &b]), prohibited(), prohibited_if(cond), prohibits("other", &other),
        // declined(), numeric(), integer(), decimal(2, 2), currency(), money(&self.currency), multiple_of(500), min_digits(n), not_matches(re),
        // gt("min_price", &self.min_price) / gte / lt / lte (numbers, dates, lengths), mimes(&["pdf", "jpg"])
        // and dimensions(&validation::Dimensions::new().min_width(800)) for an Upload,
        // v.distinct("tags", &self.tags), v.nested("lines", &self.lines) for a Vec of structs,
        // .apply(&MyRule) with `validation::Rule`.
    }
}

/// The same kind of rules as attributes: each item is a call on the field's rules
/// (`max = 100` is `.max(100)`), plus `each(…)`, `distinct` and `rename = "…"`.
/// `#[validate(hooks)]` + `impl validation::ValidateHooks` for prepare/authorize/after.
#[derive(Deserialize, Validate)]
struct ContactForm {
    #[validate(required, max = 100, label = "Your name")]
    name: String,
    #[validate(required, email)]
    email: String,
    #[serde(default)]
    #[validate(max = 5, each(required, max = 20), distinct)]
    topics: Vec<String>,
}

// Invalid input never reaches the handler. A plain form post goes back with
// errors and old input; an HTMX post gets a 422 and the errors appear next to
// the fields. Every field's errors show at once, even when one doesn't parse.
async fn store(session: Session, Valid(form): Valid<ProductForm>) -> Result<Redirect> {
    // Here `form` has passed every rule.
    let _ = form.name;
    session.flash("status", "Saved.")?; // session.keep(&["status"]) carries it one more request;
    Ok(Redirect::to("/products"))       // session.flash_now(key, value) shows one on this page only
}

/// Two forms on one page with the same field names: this one's errors go in the `login` bag,
/// shown by `error('email', bag='login')` / `<rx-input name="email" label="Email" bag="login"/>`.
#[derive(Deserialize, Validate)]
#[validate(bag = "login")]
struct SignIn {
    #[validate(required, email)]
    email: String,
    #[validate(required, current_password)] // the logged-in user's password (Laravel's current_password)
    current_password: String,
}
```

```html
<form method="post" action="{{ route('products.update', product.id) }}">
  {{ csrf_field() }}{{ method_field('PUT') }}   {# routed as PUT; also PATCH, DELETE #}
  <input name="name" value="{{ old('name', product.name) }}">
  <p class="error" data-error-for="name">{{ error('name') }}</p>
  <button>Save</button>
</form>
```

A form that mirrors a model comes from the model: `#[model(table = "products", form)]` generates
`ProductForm` (Deserialize + Serialize + Validate), rules in `#[form(validate(required, max = 100))]`
on the fields, `#[form(skip)]` / `#[form(upload)]`, and `form.fill(&mut product)`:

```rust
use renox::prelude::*;
use serde::Serialize;

#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "gadgets", form)]
pub struct Gadget {
    pub id: i64,
    #[form(validate(required, max = 100))]
    pub name: String,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

async fn save_gadget(Valid(form): Valid<GadgetForm>) -> Result<()> {
    let mut gadget = Gadget::default();
    form.fill(&mut gadget);
    Ok(())
}
```

A form request (Laravel's `FormRequest`) adds three optional steps around the rules: tidy the
input first (`prepare`), check the user may send it (`authorize`), and run extra checks at the
end (`after`).

```rust
use renox::prelude::*;
use renox::validation::FormContext;

#[derive(serde::Deserialize)]
struct Invite { email: String }

impl Validate for Invite {
    fn prepare(&mut self) { self.email = self.email.trim().to_lowercase(); } // first
    async fn authorize(&self, form: &FormContext<'_>) -> Result<bool> {    // false -> 403
        Ok(form.user.is_some_and(|u| u.has_role("owner")))
    }
    fn rules(&self, v: &mut Validator) { v.field("email", &self.email).required().email(); }
    async fn after(&self, form: &FormContext<'_>, errors: &mut Errors) -> Result { // rules passed
        if User::find_by_email(&form.state.db, &self.email).await?.is_none() {
            errors.add("email", "Nobody has signed up with this email yet."); // shown like a rule's
        }
        Ok(())
    }
}
```

One form can serve both "create" and "edit". The row's id travels in a hidden input:

```rust
use renox::prelude::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct ProductForm {
    #[serde(default)]
    id: i64, // <input type="hidden" name="id" value="{{ product.id }}">; 0 when creating
    sku: String,
    category_id: i64,
}

impl Validate for ProductForm {
    fn rules(&self, v: &mut Validator) {
        v.field("sku", &self.sku).required().unique("products", "sku").ignore(self.id); // skip this row
        v.field("category_id", &self.category_id).exists("categories", "id").message("Pick a category.");
        // Also: label("SKU code"), url(), between(1, 10), confirmed(&self.password_confirmation),
        // accepted() for a checkbox, rule(self.qty % 6 == 0, "Sold by the half dozen.").
    }
}
```

## Model, migration, queries

A migration is a SQL file that creates or changes a table. A model is a Rust struct that
matches the table: one struct is one row. The query builder finds and changes rows without
writing SQL by hand.

```sql
-- migrations/20260101000000_create_products_table.up.sql
-- (PostgreSQL: *.postgres.up.sql with BIGINT GENERATED BY DEFAULT AS IDENTITY, TIMESTAMPTZ)
CREATE TABLE products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    price INTEGER NOT NULL,
    created_at TEXT, updated_at TEXT, deleted_at TEXT
);
```

```rust
use renox::prelude::*;
use serde::Serialize;

/// One row of the `products` table. Each field is the column with the same name.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "products", soft_deletes)]
struct Product {
    id: i64, // 0 = not saved yet
    user_id: i64,
    name: String,
    price: i64,
    created_at: Option<DateTime>,
    updated_at: Option<DateTime>,
    deleted_at: Option<DateTime>,
}

/// Optional schema hints for `rnx make:migration --auto`: `index(a, b)` / `unique(slug)` on the
/// struct (repeatable), `default = "0"` and `references = "users"` on a field. Use them as
/// `#[model(table = "products", index(user_id))]` and `#[model(default = "0")] price: i64`.
/// In a test, `TestApp::new(app).await.assert_views_compile()` (or `rnx view:check`) compiles
/// every template and checks its `route('name')` calls, and the variables of typed views
/// (`#[derive(ViewData)]` structs registered with `App::view_data`, see Views).
/// `rnx view:data` writes `.vscode/renox-components.json` (VS Code tag autocomplete; `rnx serve`
/// refreshes it, `rnx new` sets `html.customData`). JetBrains isn't covered.
/// Register the model so `rnx db:check` (and `assert_models_match_schema` in tests)
/// compares its fields with the table. In a module: `app.model::<Product>()` in `register`.
fn checked() -> App {
    App::new().model::<Product>()
}

async fn queries(db: &Db) -> Result {
    // Insert a row; `create` returns it with its new id.
    let mut tea = Product::create(db, Product { name: "Tea".into(), price: 9_000, ..Default::default() }).await?;
    tea.price = 10_000;
    tea.save(db).await?; // UPDATE; sets updated_at
    // Read the rows that match, sorted by name, at most 10.
    let cheap = Product::query()
        .where_(Product::PRICE.lt(20_000)) // typed columns: a misspelt field is a compile error, the value's type is checked
        .where_(Product::NAME.like("%tea%")) // ignores case
        .order_by(Product::NAME)
        .limit(10)
        .get(db)
        .await?;
    let one = Product::find_or_404(db, tea.id).await?; // missing row -> 404 page
    let total = Product::query().count(db).await?;
    let q = "coffee";
    let found = Product::query()
        .where_any(|any| any.where_(Product::NAME.like(&format!("%{q}%"))).where_(Product::PRICE.lt(5_000))) // (… OR …)
        .when(!q.is_empty(), |query| query.where_(Product::USER_ID.is_not_null()))
        .where_(Product::PRICE.gte(1_000))
        .where_(Product::PRICE.lte(50_000))
        .get(db)
        .await?;
    let revenue = Product::query().sum::<i64, _>(db, "price").await?; // avg, min, max
    let names: Vec<String> = Product::query().order_by(Product::NAME).pluck(db, Product::NAME).await?;
    Product::query().where_(Product::USER_ID.eq(1)).update(db, &[("price", &12_000)]).await?; // sets updated_at
    Product::query().where_(Product::ID.eq(tea.id)).increment(db, "price", 500).await?;
    let first = Product::query().where_(Product::NAME.eq("Tea")).first_or_404(db).await?;
    Product::insert_many(db, vec![Product { name: "Coffee".into(), ..Default::default() }]).await?;
    tea.delete(db).await?; // soft delete; .with_trashed() / .only_trashed() / restore()
    let _ = (cheap, one, total, found, revenue, names, first);
    Ok(())
}

async fn more_queries(db: &Db) -> Result {
    let today = Product::query()
        .where_raw("DATE(created_at) = DATE(?)", [renox::db::now()]) // SQL the builder lacks
        .order_by_raw("price DESC, id")
        .get(db)
        .await?;
    let per_owner: Vec<(i64, i64)> = Product::query() // a report, read into a tuple
        .group_by("user_id")
        .having_raw("COUNT(*) > ?", [1])
        .select_as(db, "user_id, COUNT(*)")
        .await?;
    let mut tea = Product::query().where_(Product::NAME.eq("Tea"))
        .update_or_create(db, || Product { name: "Tea".into(), ..Default::default() }, |p| p.price = 9_500)
        .await?; // also first_or_new
    tea.refresh(db).await?; // reload after someone else changed it
    let page = Product::query().simple_paginate(db, 1, 50).await?; // no COUNT: prev/next only
    let feed = Product::query().cursor_paginate(db, None, 50).await?; // .next_cursor for the next call
    let (sql, values) = Product::query().where_(Product::PRICE.eq(1)).to_sql(db.dialect())?; // debugging
    let moved = db
        .transaction_retrying(3, |tx| {
            // committed on Ok, rolled back on Err, retried on SQLite busy / PostgreSQL conflicts
            // A lock covers row-reading calls only (get, first, pluck, select_as, a page's items),
            // not count, exists, sum/avg/min/max, update, increment or delete.
            Box::pin(async move { Product::query().where_(Product::ID.eq(1)).lock_for_update().first(&mut *tx).await })
        })
        .await?;
    let _ = (today, per_owner, page, feed, sql, values, moved);
    Ok(())
}

async fn even_more_queries(db: &Db) -> Result {
    let picked = Product::query()
        .where_(Product::ID.is_in([1, 2, 3])) // an empty list matches nothing
        // (a list of over 1000 values is sent as one JSON array)
        .where_(Product::DELETED_AT.is_null())
        .where_any(|any| any.where_(Product::PRICE.eq(0)).where_all(|all| all.where_(Product::PRICE.gt(100)).where_(Product::USER_ID.is_not_null())))
        .order_by_desc(Product::PRICE) // also latest(): created_at DESC, id DESC (id DESC without created_at)
        .offset(20)
        .limit(10)
        .get(db)
        .await?;
    let of_big_sellers = Product::query() // also where_not_in_query
        .where_in_query("user_id", Product::query().where_(Product::PRICE.gt(100_000)), "user_id")
        .get(db)
        .await?;
    let any_free = Product::query().where_(Product::PRICE.eq(0)).exists(db).await?;
    let some = Product::find_many(db, [1, 2, 3]).await?; // in id order; missing ids are skipped
    let coffee = Product::query().where_(Product::NAME.eq("Coffee"))
        .first_or_create(db, || Product { name: "Coffee".into(), ..Default::default() })
        .await?;
    Product::upsert(db, vec![coffee.clone()], &["name"], &["price"]).await?; // needs a unique index on name
    let seen = Product::query() // 500 rows at a time, in id order (the query's order and limit don't apply)
        .chunk(db, 500, |rows| async move {
            let _ = rows;
            Ok(())
        })
        .await?;
    let mut tx = db.begin().await?;
    let row = Product::query().where_(Product::ID.eq(1)).shared_lock().first(&mut tx).await?; // FOR SHARE on PostgreSQL
    tx.commit().await?;
    let mut gone = Product::query().only_trashed().first_or_404(db).await?;
    gone.restore(db).await?; // a soft-deleted row back
    let _ = (picked, of_big_sellers, any_free, some, seen, row);
    Ok(())
}
```

Write queries with the typed columns (`Product::PRICE.lt(20_000)`, `order_by(Product::NAME)`): the
derive makes one constant per field, in SHOUTY_SNAKE_CASE. `#[model(no_typed_columns)]` turns them
off for a struct that has its own items of those names. Column names that are only known at run
time (a sort chosen by the user) are passed as `&str`.

The query builder checks every column name against the model's fields, and `where_op` takes only
`=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`, `like` and `not like`. Anything else (and an `update` that
sets `id`) fails when the query runs, with an "invalid query" error. A field marked
`#[model(skip)]` is not a column: the model never writes it, and it holds `Default` after a load.

Keys other than numbers: the `id` field's type is the key (`rnx make:model Invoice --key ulid
-m` writes both). `Ulid` and `Uuid` (renox's `uuid` feature, a v7) are made on insert; a `String`
key is yours to set, so a new row goes in with `insert`/`create` (`save` updates a set key).
Relations and pivots follow the key type.

```sql
CREATE TABLE invoices (id TEXT PRIMARY KEY, number TEXT NOT NULL, total INTEGER NOT NULL);
-- Uuid: id BLOB PRIMARY KEY (SQLite), id UUID PRIMARY KEY (PostgreSQL)
CREATE TABLE invoice_tags (invoice_id TEXT NOT NULL, tag_id INTEGER NOT NULL, UNIQUE (invoice_id, tag_id));
```

```rust
use renox::db::Ulid;
use renox::db::relations::Pivot;
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "invoices")]
struct Invoice {
    id: Ulid, // Ulid::default() = not saved yet; `01J9Z3…` once saved
    number: String,
    total: i64,
}

const INVOICE_TAGS: Pivot<Ulid, i64> = Pivot::new("invoice_tags", "invoice_id", "tag_id");

async fn show(State(db): State<Db>, Path(id): Path<Ulid>) -> Result<Json<Invoice>> {
    // A malformed ULID in the path is a 404, like a missing row.
    Ok(Json(Invoice::find_or_404(&db, id).await?))
}

async fn tag(db: &Db, invoice: &Invoice) -> Result {
    INVOICE_TAGS.sync(db, invoice.id.clone(), [1, 2]).await
}
```

Generic code over models names the key when it needs one: `fn like<P: Model<Key = i64>>(p: &P)`.

Relations are explicit: a method for one related row, and loaders for a page of rows
(`relations::belongs_to`, `has_many`, `has_many_through` (a country's orders through its
customers, two queries), `Pivot` for many-to-many (two queries: the pivot rows, then the
models), `count_many`/`sum_many` for counts and totals per row; the others take one query
each, no N+1). Filter by related rows with
`.where_has(Review::query().where_(Review::STARS.eq(5)), "product_id")` / `.where_doesnt_have(…)`.
For joins and reports, use `sql("…").fetch_as::<T>(&db)` with `#[derive(FromRow)]` or a tuple.
See [docs/relations.md](docs/relations.md): pivot columns (`attach_with`, `load_with_pivot`,
`toggle`) and polymorphic relations (`Morph`) are there too.

## Full-text search (details in [docs/search.md](docs/search.md))

Find rows by the words in them, best matches first, on SQLite (FTS5) and PostgreSQL
(`tsvector`) alike. Name the columns on the model, add the index's migration, then search.

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Model, Serialize, Default)]
#[model(table = "posts", search = "title, body")] // the title weighs more
struct Post {
    id: i64,
    title: String,
    body: String,
    published: bool,
}

fn app() -> App {
    App::new()
        .migrations(renox::migrations!())
        // After the migration creating `posts`: an FTS5 table + triggers on
        // SQLite, a generated column + GIN index on PostgreSQL.
        .migrations(&[renox::db::search::migration::<Post>("20260104000000_search_posts")])
}

#[derive(Deserialize)]
struct Search {
    q: Option<String>,
}

async fn index(State(db): State<Db>, Page(page): Page, Query(s): Query<Search>) -> Result<View> {
    let q = s.q.unwrap_or_default(); // user input is safe: only its words are used
    let posts = Post::search(&q) // every word, prefixes ("cof" → coffee), word forms
        .where_(Post::PUBLISHED.eq(true))
        .latest() // ties: newest first
        .paginate(&db, page, 20)
        .await?;
    Ok(view("posts/index.html", context! { posts, q }))
}
```

`where_search(q)` filters without ranking, `order_by_relevance(q)` ranks only. The database
keeps the index current on every write (bulk updates and raw SQL too).
`#[model(search_language = "simple")]` turns English stemming off (PostgreSQL also takes
`spanish`, `german`, …). A data grid over a searchable model searches through the index.

Arabic text: `#[model(search = "name, notes", normalize = "arabic")]` indexes and searches a
normalised copy (marks, tatweel, alef/ya/ta marbuta variants, Persian ک/ی and Arabic-Indic digits
ignored), on both databases; `renox::db::search::arabic_sql(dialect, "expr")` is the same in SQL.
`renox::text` has the Rust side: `normalize_arabic("أَحْمَد")` is `"احمد"`, `transliterate("محمد")`
is `"mhmd"`, and `match_key` is equal for "محمد", "Mohammed" and "Muhammad". For "possible
duplicate" checks keep a `name_key` column filled in the `saving` hook with
`self.name_key = renox::text::match_key(&self.name)` and query `Party::NAME_KEY.eq(match_key(&input))`.

## Model hooks, partial saves, encrypted values

Run your own code just before or after a model is saved or deleted (hooks), save only some
columns, and keep secrets encrypted in the database.

```rust
use renox::db::{Encrypted, ModelHooks};
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "posts", hooks)]
struct Post {
    id: i64,
    title: String,
    slug: String,
    views: i64,
    api_secret: Encrypted<String>, // a TEXT column holding it sealed; any serde type works
}

impl ModelHooks for Post {
    // Before INSERT/UPDATE; an Err stops the save. Also `deleting` before a delete.
    fn saving(&mut self, _creating: bool) -> Result {
        self.slug = self.title.to_lowercase().replace(' ', "-");
        Ok(())
    }

    // After the write; also `deleted`. The current app, in a request, job, task or command:
    async fn saved(&self, _created: bool) -> Result {
        if let Some(state) = renox::context::app() {
            state.cache.forget("posts.latest").await?;
        }
        Ok(())
    }
}

async fn edit(state: &AppState, id: i64) -> Result {
    let db = &state.db;
    let original = Post::find_or_404(db, id).await?;
    let mut post = original.clone();
    post.title = "New title".into();
    post.save_only(db, &["title"]).await?; // only these columns (+ updated_at); `views` untouched
    post.save_changes(db, &original).await?; // the columns that differ; false if none
    post.api_secret = Encrypted::new("sk_live_123".into()); // sealed (AES-256-GCM, APP_KEY) on save
    let secret: &str = &post.api_secret; // plain when read; Debug prints `Encrypted(..)`
    // But Serialize writes the plain value: keep such a field out of Json(post) and views.
    // Can't be searched (a fresh nonce per write). For a value outside a model:
    let sealed = state.encrypt("sk_live_123");
    let plain = state.decrypt(&sealed)?; // Err if tampered or another key
    let _ = (secret, plain);
    Ok(())
}
```

Hooks run for `save`, `create`/`insert`, `save_only`, `save_changes`, `delete` and `force_delete`, not
for `restore` or bulk `Query::update`/`delete`, `insert_many` or `upsert`. `save_changes` runs
`saving` even when nothing changed; then it skips the query and `saved`.

## Every field type (details in docs/types.md)

Which Rust type to use for each kind of form field, and the column that stores it (SQLite /
PostgreSQL).

```rust
use renox::chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use renox::db::Json;
use renox::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Default)]
enum Size { Small, #[default] Medium, Large } // <select>; TEXT "small" | "medium" | "large"

#[derive(Model, Serialize, Default)]
#[model(table = "products")]
struct Product {
    id: i64,
    name: String,                 // <input>            TEXT
    price: i64,                   // money in cents     INTEGER / BIGINT
    weight_kg: f64,               // step="0.01"        REAL / DOUBLE PRECISION
    available: bool,              // checkbox           INTEGER 0/1 / BOOLEAN
    size: Size,                   // <select>           TEXT
    colors: Json<Vec<String>>,    // <select multiple>  TEXT / JSONB
    opens_at: Option<NaiveTime>,          // type=time            TEXT / TIME
    launch_at: Option<NaiveDateTime>,     // type=datetime-local  TEXT / TIMESTAMP
    released_on: Option<NaiveDate>,       // type=date            TEXT / DATE
}

#[derive(Deserialize)]
struct ProductForm {
    available: bool,     // "on" → true; unchecked (not sent) → false (Valid<T> only: Form<T> refuses "on")
    #[serde(default)]
    colors: Vec<String>, // colors=black&colors=red; nothing chosen → []
    size: Size,          // "huge" → a validation error, reported with the others
}
```

Money in a currency of its own: `Money` (`renox::money`) is the amount in the smallest unit plus its
ISO 4217 `Currency`; `#[model(money)]` stores it as `<field>` (`BIGINT`) and `<field>_currency` (`TEXT`)
(docs/types.md, docs/validation.md). A fixed currency stays an `i64`.

```rust
use renox::money::{Currency, Money, Rounding};
use renox::prelude::*;

#[derive(Model, serde::Serialize, Debug)]
#[model(table = "invoices", form)] // InvoiceForm: "12.50" + "usd" texts, checked together; fill() builds the Money
struct Invoice {
    id: i64,
    #[model(money(currency = "currency"))] // subtotal, tax and total share the `currency` column
    subtotal: Money,
    #[model(money(currency = "currency"))]
    tax: Money,
    #[model(money)] // deposit + deposit_currency, both NULL for None
    deposit: Option<Money>,
}

fn money() -> Result<(), renox::money::MoneyError> {
    let total = Money::usd(1999).percent(500, Rounding::HalfUp)?; // always an explicit rounding: $1.00
    let sum = total.checked_add(Money::usd(1))?;                  // Err(CurrencyMismatch) across currencies
    let parts = Money::usd(100).allocate(3)?;                     // 34, 33, 33: no minor unit lost
    let yen = sum.convert("150.25", Currency::JPY, Rounding::HalfEven)?; // an exact decimal rate, never f64
    assert_eq!(Money::parse("12.50", Currency::USD)?.format("de"), "$12,50"); // too many decimals is an error
    let _ = (parts, yen, Money::kwd(12_345).to_string()); // "KWD 12.345"; {"minor":12345,"currency":"KWD"} in JSON
    Ok(())
}
```

`{{ invoice.total | money }}` shows a `Money` in its own currency (an `i64` still uses `APP_CURRENCY`);
`<rx-input name="deposit" type="money" currency="JPY"/>` steps by the currency's smallest unit;
`Column::money("total", "Total").currency_from("total_currency")` (grid: per-row currency, one summary
per currency, `?cur.total=JPY`) and `Field::money("total", "Total").currency_from("total_currency")` (admin).

Exact decimals with units: `Quantity<SCALE>` (`renox::quantity`) is an `i64` of 10^-SCALE units (`Quantity<3>` is thousandths),
one `BIGINT`; the unit is a `String` column beside it, never part of the type (docs/types.md, docs/validation.md).

```rust
use renox::money::Rounding;
use renox::prelude::*;
use renox::quantity::{Quantity, QuantityError, Units};

#[derive(Model, serde::Serialize, Debug)]
#[model(table = "bins", form)] // BinForm: "12.5" texts checked with quantity(3); fill() builds the Quantity
struct Bin {
    id: i64,
    stock: Quantity<3>,         // BIGINT NOT NULL; the scale is the type's, not the schema's
    stock_unit: String,         // "kg", "g", "t"
    spare: Option<Quantity<3>>, // NULL when none
}

fn quantities() -> Result<(), QuantityError> {
    let kg = Quantity::<3>::parse_exact("2.5")?;                       // Err(Inexact) when digits would be lost
    let tie = Quantity::<3>::parse("0.0005", Rounding::HalfUp)?;       // always an explicit rounding: 0.001
    let sum = kg.checked_add(Quantity::parse_exact("0.1")?)?;          // exact, Err(Overflow) at the limits
    let units = Units::new().base("kg").unit("g", "kg", "0.001").build()?; // exact decimal factors, never f64
    let grams = units.convert(sum, "kg", "g", Rounding::HalfEven)?;    // "2600.000"
    let _ = (tie, grams, kg.mul_decimal("1.15", Rounding::HalfUp)?);
    Ok(())
}
```

`Bin::STOCK.lt(..)`, `order_by` and `sum` compare the scaled integer. `{{ bin.stock | quantity(unit=bin.stock_unit) }}` shows
`2.500 kg`; `<rx-input name="stock" type="quantity" scale="3" unit="kg"/>` steps by the scale; rules `.quantity(3)`,
`.quantity_min("0")`, `.quantity_max("100")`, `.unit(&units)`; `Column::quantity("stock", "Stock").unit_from("stock_unit")` (grid:
one summary per unit) and `Field::quantity("stock", "Stock").unit_from("stock_unit")` (admin).

Custom fields an administrator adds without a migration: definitions in the `custom_fields` table, values in one
`Json<CustomValues>` column named by `custom_fields = "extra"` (docs/types.md).

```rust
use renox::custom_fields::{CustomField, CustomValues};
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "parties", form, custom_fields = "extra")] // the form takes extra[key]; Valid<T> checks them
struct Party {
    id: i64,
    name: String,
    extra: renox::db::Json<CustomValues>,
}

async fn custom_fields(db: &Db) -> Result {
    CustomField::text("parties", "passport_no").label("Passport no.").label_in("es", "N.º de pasaporte").required().max(20).save(db).await?;
    CustomField::choice("parties", "segment").options(&["retail", "corporate"]).save(db).await?; // keys are [a-z][a-z0-9_]* and never change
    // number, date, bool, reference(model, key, "customers"), file(model, key, "local"); .meta(json), .position(n), .inactive()
    let fields = CustomField::for_model(db, "parties").await?; // active, ordered, cached per model
    let _ = fields;
    let retail = Party::query()
        .where_(Party::EXTRA.json("segment").eq("retail")) // json_extract / ->>, value bound
        .order_by_json_desc(Party::EXTRA.json("segment"))
        .with_custom_fields(db).await? // unknown key = error; typed casts on PostgreSQL
        .get(db).await?;
    let _ = retail;
    Ok(())
}
```

`CustomField::views(&db, "parties", locale)` feeds `<rx-custom-fields :fields="fields" name="extra" :values="party.extra"/>`
(inputs; `mode="show"` for entries); `Column::custom_field("extra", "segment")` is a grid column (sort, filter, export;
`grid.with_custom_fields::<Party>(&request)` before the sync `filter`); renox-admin adds all of it, and a "Custom fields"
resource, for a model that declares `custom_fields`; `db::json_index(dialect, "parties", "extra", "segment")` gives the
`CREATE INDEX` for a hot key.

## Pagination

Show a long list one page at a time. `Page` reads the page number from `?page=` in the URL.

```rust
use renox::prelude::*;
#[derive(Model, serde::Serialize, Default)]
#[model(table = "products")]
struct Product { id: i64, name: String }

async fn index(State(db): State<Db>, Page(page): Page) -> Result<View> {
    // 20 rows per page, newest first, with the total count for the page links.
    let products = Product::query().latest().paginate(&db, page, 20).await?;
    Ok(view("products/index.html", context! { products })) // products.items, .total, …
}
```

`paginate`, `simple_paginate` and `cursor_paginate` give at most 1000 rows per page, whatever
`per_page` asks for.

## Dashboards: figures and charts (details in docs/ui.md "Dashboards")

Show key figures and charts. Renox draws the charts on the server, so you need no chart
library.

```rust
use renox::prelude::*;
use renox::chart::{Period, Trend};

#[derive(Model, serde::Serialize, Default)]
struct Order { id: i64, total: i64, status: String, created_at: Option<renox::db::DateTime> }

// ?period=7d|30d|90d|12w|12m|mtd|ytd, or ?period=custom&from=2026-09-01&to=2026-09-30
// (Period: 30 days by default, also for a refused range; serializes as "30d" or
// "2026-09-01..2026-09-30"). Per day up to 92 days, per week (Monday-based ISO weeks,
// labelled by the Monday) for 12w, else per month; period.per(Bucket::Week) picks the step.
async fn dashboard(State(state): State<AppState>, period: Period) -> Result<View> {
    let paid = || Order::query().where_(Order::STATUS.eq("paid"));
    let sales = Trend::of(paid(), "created_at").over(period).sum(&state, "total").await?; // or count / average
    let before = Trend::of(paid(), "created_at").over(period.previous()).sum(&state, "total").await?;
    Ok(view("dashboard.html", context! {
        period,
        revenue => sales.total(),
        change => sales.change_from(&before), // percent, or None
        sales => sales.named("Sales"),         // {name, labels (2026-10-02 / 2026-10), values}
        // Points for scatter/bubble charts: [{x, y, size, label}], e.g. from select_as.
        products => [renox::serde_json::json!({ "x": 25_000, "y": 12, "size": 300_000, "label": "Latte" })],
    }))
}
```

```html
<rx-period-filter :selected="period"/>                   {# links to ?period=…, keeping the query #}
{# <rx-period-filter :selected :options="[['7d', 'Week'], …]" label=… custom>: selected is the handler's
   Period; the default options are 7d, 30d, 90d, 12m and ytd; label names the nav for screen readers;
   "Custom" opens two date fields sent as ?period=custom&from=…&to=… (:custom="false" hides it) #}
<rx-stats columns="4">
  <rx-stat label="Revenue" :value="revenue | money" :delta="change" :trend="sales.values" url="/orders"/>   {# good="down" for costs #}
</rx-stats>
<rx-dashboard columns="3">
  <rx-widget title="Sales" description="Paid orders" span="2">
    <rx-chart kind="area" :data="sales" format="money"/>   {# line, area, bar (stacked), pie, doughnut #}
  </rx-widget>
  <rx-widget title="By status" url="{{ route('dashboard.statuses') }}" :poll="60"/>   {# loaded after the page, every 60 s #}
</rx-dashboard>
{# the chart(...) function still works anywhere: {{ chart("bar", labels=…, values=…) }}; the tag is the same chart #}
{{ chart("bar", labels=["Coffee", "Tea"], series=[{"name": "2025", "values": [3, 5]}, {"name": "2026", "values": [4, 6]}]) }}
{# points: {x, y, size, label} maps, or [x, y] / [x, y, size]; several series: series=[{"name": …, "points": […]}] #}
{{ chart("scatter", points=orders, x_title="Items", y_title="Total", format="money") }}
{{ chart("bubble", points=products, x_title="Price", y_title="Units", size_title="Revenue",
         x_format="money", size_format="money") }} {# format is y's; x_format / size_format: number, money, percent #}
{# heatmap: columns/rows are labels; values is one list per row, or cells=[[col, row, value]] (0-based) #}
{{ chart("heatmap", columns=["Mon", "Tue"], rows=["09:00", "10:00"], values=[[1, 4], [0, 9]]) }}
```

## Data grid (details in [docs/grid.md](docs/grid.md), example in [the bike shop's reports](examples/bikeshop/src/app/reports/grids.rs))

A data grid is a table with filters, sorting and pages, all kept in the URL. You describe the
columns in Rust, and the template draws the table.

```rust
use renox::grid::{Column, Grid, GridRequest};
use renox::prelude::*;
#[derive(Model, serde::Serialize, Default)]
#[model(table = "orders")]
struct Order { id: i64, number: String, status: String, total: i64 }

/// Describes the grid once: its columns, how each is shown, and the default sort.
fn orders_grid() -> Grid {
    Grid::new("orders")
        .title("Orders")
        .column(Column::text("number", "Order").frozen().mobile()) // shown on phones, stuck left
        .column(Column::select("status", "Status", [("new", "New"), ("paid", "Paid")]).mobile())
        .column(Column::money("total", "Total").under(["Amounts"])) // a grouped heading
        .column(Column::custom("actions", "").frozen_right())        // drawn by the template
        .sort_by("-total")
}

/// `GridRequest` reads the visitor's filters, sort and page from the URL.
async fn index(request: GridRequest) -> Result<View> {
    let page = orders_grid().page(Order::query(), &request).await?; // filters, sort, page from ?query
    Ok(view("orders/index.html", context! { orders => page }))
}
```

`<rx-grid :page="orders">…</rx-grid>` (the content draws `custom` columns, with `row` and `column`)
inside `<main class="rx-grid-fill">` (`:tools="…"` puts HTML such as a "New" button in
the toolbar); `{{ sparkline(row.trend) }}` draws a small chart, fed by
`page.extend(|order| json!({ "trend": … }))`. Column kinds: `text`, `number`, `money`, `date`,
`datetime`, `bool`, `select`, `tags`, `image`, `color`, `custom`; each takes `.hidden()`,
`.width("8rem")`, `.decimals(2)`, `.sortable(false)`, `.filterable(false)`, `.wrap()`.
`Grid::per_page(50)`, `Grid::filter(query, &request)` (the user's filters on your own query).
More: `.audit()` (who changed a row, under it), `.details()` (a row opens details drawn by
the template, `column.key == "_details"`), `Column::editable()` + `.edit_url("/orders/{id}")`
(PATCH, `Valid<T>` with `Option` fields), `.reorder("position", url)` + `RowOrder::save`,
`Column::merge()` with `sort_by("region,city")`, `.exports()` + `grid.export(query, &request)`
(CSV, Excel with the `xlsx` feature, a print page), `grid.export_as(query, ExportFormat::Csv, &request)`
(any query as a file, outside the grid's page), `Column::searchable()` (the search box),
`.row_url("/orders/{id}")`, `.empty_state(…)`, `.prefix("orders")` for two grids on a page,
`.bulk_action(Action::new("Delete", url).confirm("Sure?").danger())` +
`grid.selected(query, &request, &selection)?` (`Form<Selection>`), `.row_action(…)`,
`Action::link(label, url)` / `.method("POST")`,
`.summary(Summary::Sum)` on a column (footer and group subtotals), `.groups(&["region"])` / `.group_by("region")`,
`.cards_on_mobile()`, `.badges(&[("paid", "success")])`, `.description("email")`, `.icons()`,
`Column::image(…).round()`, `.copyable()`, `.link(url)`, `.tooltip(key)`, `.limit(n)`,
`Column::related(key, label, table, foreign_key, column)`, `Column::count_of(…)`, `Column::sum_of(…)`,
`.advanced_filter()`, `.remember()`, `.poll(30)`.

A whole admin panel over your models (Filament's resources): the `renox-admin` crate,
`.module(Admin::new().authorize(|user| user.has_role("staff")).resource(ProductResource))`
with `impl AdminResource for ProductResource` (`type Model`, `type Form`, `columns()` as
above, `fields()` with `Field::text`/`select`/`money`/`belongs_to`…, `fill`), and the model's
`Policy` asked for `viewAny`, `create`, `update`, `delete`… ([docs/admin.md](docs/admin.md),
[the bike shop's panel](examples/bikeshop/src/app/staff/admin.rs)).
A module adds a resource to the app's panel from `register`: `renox_admin::contribute(app, SupplierResource)`
(or `app.contribute(AdminEntry::new(r).panel("staff"))` for a panel named with `Admin::named("staff")`;
`Admin::only_own_resources()` ignores contributions; a slug clash or an unknown panel fails at boot).
It rests on `Registry::contribute::<T>(value)` (any `Send + Sync + 'static` type, app first, then modules
in order), read with `Contributions::all::<T>()` / `all_from::<T>()` (with the module's name) in
`Module::routes_with(&self, &Contributions)`, or `state.contributions::<T>()` in a handler
([docs/admin.md](docs/admin.md), [the bike shop's module](examples/bikeshop/src/app/purchasing/mod.rs)).

## Seeders and factories

A factory makes models filled with fake data. A seeder uses it to fill the database with
demo data (`rnx db:seed`).

```rust
use renox::fake::{Fake, faker::lorem::en::Word};
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "products")]
struct Product { id: i64, name: String, price: i64 }

/// How to make one product with fake data.
impl Factory for Product {
    fn definition() -> Self {
        Product { name: Word().fake(), price: (5_000..50_000).fake(), ..Default::default() }
    }
}

fn seeders(app: App) -> App {
    // `rnx db:seed`, or `rnx migrate:fresh --seed`
    app.seeder(|state| async move {
        let db = state.db.clone();
        Product::factory().count(50).create(&db).await?; // in one transaction
        // Seeders run in the app's context: config, encrypt, the cache, random_token().
        let state = renox::context::app().expect("in a seeder");
        let _invite = (state.encrypt("secret"), renox::random_token());
        Ok(())
    })
    // A module adds its own in `Module::register`: registry.seeder(|state| async move { … }).
    // The app's seeders run first, then the modules' in the order they were added.
}

/// A state: a plain function that changes the model.
fn premium(p: &mut Product) {
    p.price = 250_000;
}

async fn in_a_test(db: &Db) -> Result {
    let draft = Product::factory().make_one();                // not saved
    let saved = Product::factory().create_one(db).await?; // saved
    let three = Product::factory()
        .count(3)
        .state(premium)
        .sequence(|i, p| p.name = format!("Coffee {}", i + 1)) // by position: 0, 1, 2
        .create(db)                                          // or .make() unsaved
        .await?;
    let _ = (draft, saved, three);
    Ok(())
}
```

```html
<section id="products">
  {% for p in products.items %}<p>{{ p.name }}</p>{% endfor %}
  {# page links swap just this section and keep other query parameters (?q=…) #}
  <div hx-boost="true" hx-target="#products" hx-select="#products" hx-swap="outerHTML">
    {% from "renox/pagination.html" import pagination %}{{ pagination(products) }}
  </div>
</section>
```

## Auth, policies, gates

Auth (authentication) answers "who are you?": logging in and out. Policies and gates answer
"what may you do?".

```rust
use renox::prelude::*;
#[derive(Model, serde::Serialize, Default)]
#[model(table = "products")]
struct Product { id: i64, user_id: i64 }

/// A policy: who may do what with one product.
impl Policy for Product {
    fn allows(&self, user: &User, ability: &str) -> bool {
        match ability {
            // has_role: the current request's roles (the Permissions module)
            "update" | "delete" => self.user_id == user.id || user.has_role("admin"),
            _ => false,
        }
    }
}

// Routes::new().resource("/products", "products", Resource::new().index(index).show(show)…)
// registers index/create/store/show/edit/update/destroy as Laravel does (products.index, …).
// `AuthUser` sends guests to the login page; use `Option<AuthUser>` when optional.
// `Found` loads the product the route's `{id}` (or `{product}`, or a column such as `{slug}`)
// names: a missing row or `/products/abc` is a 404 (Laravel's route model binding).
async fn edit(user: AuthUser, Found(product): Found<Product>) -> Result<View> {
    user.authorize("update", &product)?; // 403 unless allowed
    Ok(view("products/edit.html", context! { product }))
}

/// Gates are named checks that don't belong to one model, such as "is this an admin?".
fn gates(app: App) -> App {
    // A gate on a column the app added to `users`; for roles use the `Permissions` module
    // (below, "Tenants, roles and permissions") and docs/authorization.md.
    app.gate("admin", |user| user.get::<String>("role").as_deref() == Some("admin")) // user.gate("admin")?
        // May query the database; check with `user.gate_async("billing").await?` in handlers.
        .gate_async("billing", |user, state| async move {
            let n: i64 = renox::db::sql("SELECT COUNT(*) FROM team_admins WHERE user_id = ?")
                .bind(user.id)
                .scalar(&state.db)
                .await?;
            Ok(n > 0)
        })
}

// Extra registration fields (add the inputs to your own renox/auth/register.html).
fn auth() -> Auth {
    Auth::new()
        .registration_rules(|form, v| {
            v.field("phone", &form.get("phone")).required().max(20);
        })
        .on_registered(|mut user, form, state| async move {
            user.set(&state.db, "phone", form.get("phone")).await // a failure undoes the sign-up
        })
}

// API tokens (`Authorization: Bearer …`, no CSRF): issue one, and revoke the
// one a request used ("log out" on one device) or all of them.
async fn issue(State(db): State<Db>, user: AuthUser) -> Result<String> {
    Ok(user.create_token(&db, "phone", None).await?.plain) // shown once
}
async fn log_out_device(State(db): State<Db>, user: AuthUser) -> Result<StatusCode> {
    if let Some(id) = user.token_id() {
        user.revoke_token(&db, id).await?; // user.revoke_tokens(&db) for every device
    }
    Ok(StatusCode::NO_CONTENT)
}

// For `{% if can('update', product) %}` in the view, attach the abilities.
async fn index(State(db): State<Db>, user: Option<AuthUser>) -> Result<View> {
    let products: Vec<_> = Product::query()
        .get(&db)
        .await?
        .into_iter()
        .map(|p| Can::new(p, user.as_deref(), &["update", "delete"]))
        .collect(); // on a page: .paginate(…).await?.map(|p| Can::new(…))
    Ok(view("products/index.html", context! { products }))
}
```

```rust
use renox::prelude::*;

fn back_office() -> Auth {
    Auth::new()
        .account()                 // /account: profile, password, other devices, delete
        .password_rules(renox::validation::Password::min(12).mixed_case().numbers()) // also letters(), symbols(), uncompromised()
        .verify_email()            // mails a link; guard routes with .require_verified()
        .without_registration()    // no /register: an admin adds users
        .redirect_to("/dashboard") // after login, when no page asked for one
}

// Passwordless login (docs/authorization.md): a link by mail, a code by mail/SMS/WhatsApp.
// Single use, expiring, throttled; the second step and the audit log apply; `LoggedIn::via`.
fn passwordless() -> Auth {
    Auth::new()
        .magic_link()                  // /login/link: 15 minutes; opening the link logs nobody in, its button does
        .same_browser()                // links work only in the browser that asked (codes always do)
        .register_on_first_use()       // an unknown address that proves it gets an account
        .login_codes(                  // /login/code: bound to the asking browser, burned after max_attempts
            renox::auth::LoginCodes::new()
                .channels(["sms", "mail"])                       // App::channel names; the form lets users pick
                .digits(6)
                .ttl(std::time::Duration::from_secs(300))
                .max_attempts(5)
                .identify(|state, phone: String| async move {   // who a phone number belongs to
                    Ok(renox::db::sql("SELECT * FROM users WHERE phone = ?")
                        .bind(phone.trim())
                        .fetch_optional_as::<User>(&state.db)
                        .await?)
                })
                .deliver(|_state, delivery| async move {         // optional: your own messaging layer
                    let _ = (delivery.channel, delivery.address, delivery.code, delivery.locale);
                    Ok(())
                }),
        )
}
// Tests: `app.last_login_link().await` / `last_login_code().await` (runs the queued delivery).
// Codes for any other action: `renox::auth::OneTimeCode::issue(&state, "payout", &subject, ttl, 5)`
// and `verify(..) -> CodeCheck::{Valid, Wrong { tries_left }, Missing}`.

#[derive(serde::Deserialize)]
struct Login { email: String, password: String }

// Your own login (Auth's /login already does this, with a lockout).
async fn sign_in(State(state): State<AppState>, session: Session, Form(f): Form<Login>) -> Result<Redirect> {
    let Some(user) = User::attempt(&state.db, &f.email, &f.password).await? else {
        return Err(abort(StatusCode::UNAUTHORIZED, "Wrong email or password."));
    };
    renox::auth::login(&session, &user, None)?; // Some(state.config.remember_lifetime): "remember me"
    Ok(Redirect::to("/dashboard"))
}

// Two-factor authentication ready-made: the `renox-2fa` crate, `.module(TwoFactor::new())`
// next to `Auth::new().account()` (docs/two-factor.md). Or a second step of your own,
// from a module's `register`:
// `app.second_factor("otp.challenge", |user, _state| async move { Ok(needs_code(&user)) });`
// its challenge handler calls `renox::auth::pending_login(&session)`, checks the code,
// then `renox::auth::complete_login(&state, &session, &pending, ip)` (docs/authorization.md).
// A menu link from a module or the app: `app.nav("staff", NavLink::route("invoices.index", "Invoices").icon("receipt")
// .group("Billing").order(10).permission("invoices.view").badge(|state: AppState, user: AuthUser| async move { Ok(Some("3".into())) }))`
// (`use renox::nav::NavLink`; also `NavLink::url(url, label).key(k)`, `.gate(g)`, `.when(|user| …)`, `.active("invoices.*")`),
// `app.nav_hide("staff", "invoices.index")`. `.permission` = held globally or in any scope: `user.has_permission_anywhere(p)`.
// Drawn by `<rx-sidebar menu="staff"/>` / `<rx-nav-links menu="portal"/>` or `nav('staff')` (docs/ui.md, docs/authorization.md).

// Brand themes (docs/ui.md "Brand themes"): accent tokens from one colour, light and dark, contrast-checked.
// let theme = Theme::from_brand(&Brand::new("#1E3A5F").second("#B8532E").display_font("Poppins"))?; // Err(ContrastError { suggestion, .. }) for #FFF59D
// App::new().theme(theme)  // or .theme_for(|req: ThemeRequest| async move { Ok(Some(theme)) }) per host/tenant/setting; Ok(None) = app theme
// Layout: <rx-theme/> after <rx-ui/>; served at /_renox/theme/{hash}.css (immutable, CSP strict safe).
// theme.tokens() / theme.dark_tokens() -> BTreeMap; Mail::theme(&theme) colours rx-mail-button/panel, also when queued.
// Install on phones (docs/ui.md "Install on phones"): `rnx make:pwa [--force]` writes offline.html, placeholder icons, the .pwa(..) call.
// App::new().pwa(Pwa::new("Bike Shop").short_name("Bikes").theme_color("#1E3A5F").icons("public/icons").offline_page("offline.html"))
// (`use renox::pwa::{Pwa, PwaBrand, PwaRequest}`; icons dir: icon-192.png, icon-512.png, icon-maskable-512.png; a missing one is a boot error)
// .pwa_for(|req: PwaRequest| async move { Ok(Some(PwaBrand::new("North Bikes").theme_color("#0F766E"))) }) per host (Vary: Host); Ok(None) = static Pwa
// Layout: <rx-pwa/> after <rx-theme/>; serves /manifest.webmanifest, /sw.js (versioned), /_renox/offline; navigations are never cached.
// Offline forms (docs/ui.md "Offline forms"): a failed post waits in an IndexedDB outbox (not encrypted, cleared on logout), replays in order.
// Routes::new().post("/stops/{id}/complete", complete_stop).offline()  // needs Idempotency-Key (400); same key+body = stored reply; other body = 422
// .offline_with(Skew::new().past(Duration::from_secs(72 * 3600)).future(Duration::from_secs(60)))  // default 24 h past, 5 min ahead
// async fn complete_stop(offline: Offline) -> Result<String> { offline.recorded_at / .received_at / .client_id / .skew_exceeded }
// <rx-form action="/stops/7/complete" offline="stops"> ... </rx-form>  // loads renox-offline.js; GET /_renox/csrf gives a fresh token
// `rnx idempotency:prune [--days N]` (IDEMPOTENCY_RETENTION_DAYS, 7). No Background Sync: posts are sent while a page of the app is open.
// A card on /account from a module: `app.account_section("pin/account.html", 10,
// |user, _state| async move { Ok(json!({ "on": true })) })`, read as `section.data`.
// Social login ready-made: the `renox-oauth` crate, `.module(OAuth::new().google().github())`
// with GOOGLE_CLIENT_ID/_SECRET, GITHUB_CLIENT_ID/_SECRET (docs/oauth.md). Company sign-in:
// `.provider(Microsoft::from_config().tenant("contoso.onmicrosoft.com"))` (MICROSOFT_CLIENT_ID/
// _SECRET/_TENANT; personal accounts refused), `Google::from_config().hosted_domain("example.com")`
// (sends `hd`, checks the claim), `OAuth::new()….allow_domains(&["example.com"])` (verified
// email on any provider); a refusal emits `OAuthRefused` (`oauth.refused`). A login of your
// own once you know who it is: `let to = renox::auth::sign_in(&state, &session, &user,
// false, ip).await?;` (the second step included); `renox::auth::register_verified` makes an
// account without a password (`user.has_password()` is false).

async fn sign_out(State(db): State<Db>, session: Session) -> Result<Redirect> {
    renox::auth::logout(&db, &session).await?; // this device (a copied cookie dies too)
    Ok(Redirect::to("/"))
}

async fn security(State(db): State<Db>, session: Session, user: AuthUser) -> Result<View> {
    let mut me = user.user().clone();
    renox::auth::change_password(&db, &session, &mut me, "a new password").await?; // others end
    renox::auth::logout_other_devices(&db, &session, &me).await?; // keeps this one
    // me.revoke_sessions(&db) ends every session, this one too; me.delete_account(&db)
    let admin = user.allows("admin");               // a gate as a bool; allows_async for gate_async
    let billing = user.allows_async("billing").await?;
    Ok(view("account/security.html", context! { admin, billing }))
}

// The database channel's inbox.
async fn inbox(State(db): State<Db>, user: AuthUser) -> Result<View> {
    let notifications = user.notifications(&db, 20).await?; // newest first; .data, .read_at
    let unread = user.unread_notification_count(&db).await?;
    user.mark_all_notifications_read(&db).await?; // or mark_notification_read(&db, id)
    Ok(view("inbox.html", context! { notifications, unread }))
}
```

Routes that should ask for the password again (after three hours): `.require_password_confirmed()`
(the `Auth` module serves `/confirm-password`). Users imported from Laravel keep their bcrypt
hashes and are moved to Argon2id when they next log in.

The `Auth` module's routes, by name (use them with `route('login')` or `Redirect::route`):

- `login` (GET, POST `/login`), `register` (GET, POST `/register`; not with
  `.without_registration()`), `logout` (POST `/logout`).
- `password.request` (GET `/forgot-password`), `password.email` (POST `/forgot-password`),
  `password.reset` (GET `/reset-password/{token}`), `password.update` (POST `/reset-password`),
  `password.confirm` (GET, POST `/confirm-password`).
- `verification.notice` (GET `/verify-email`), `verification.verify`
  (GET `/verify-email/{id}/{hash}`), `verification.send` (POST `/email/verification-notification`).
- With `.account()`: `account.show` (GET `/account`), `account.profile` (PUT
  `/account/profile`), `account.password` (PUT `/account/password`), `account.logout_others`
  (POST `/account/logout-others`), `account.destroy` (DELETE `/account`).
- With `.notifications()`: `notifications.index`, `notifications.stream` and the others under
  `notifications.*`.

## Auth events and the audit log

Renox announces what happens at login, such as "logged in" or "login failed". Listen to these
events, or add the `Audit` module to write them to a log.

```rust
use renox::prelude::*;
use renox::audit::{self, Audit, Entry};
use renox::auth::events::{LoginFailed, Registered};

fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(Audit) // an audit_logs table; records every auth event below
        // Registered, LoggedIn, LoginFailed, LockedOut, LoggedOut, PasswordReset,
        // PasswordChanged, EmailVerified, ProfileUpdated, OtherDevicesLoggedOut, AccountDeleted
        .listen(|e: Registered, state| async move {
            let _ = (e.user_id, state); // e.g. queue a welcome mail
            Ok(())
        })
        .listen(|e: LoginFailed, _state| async move {
            let _ = (e.email, e.ip); // e.g. alert on many failures from one address
            Ok(())
        })
}

async fn refund(State(db): State<Db>, user: AuthUser, ClientIp(ip): ClientIp) -> Result<&'static str> {
    audit::record(&db, Entry::new("order.refunded").user(user.id).subject("orders", 42)
        .data(json!({ "amount": 75_000 })).ip(ip)).await?;
    let history = audit::for_subject(&db, "orders", 42, 20).await?; // also for_user, latest
    let _ = history;
    Ok("refunded")
}
```

`rnx audit:prune --days 365` deletes older entries.

## Settings in the database (details in [docs/settings.md](docs/settings.md))

Typed settings (a company name, a number format, which features are on) kept in the `settings`
table, cached, validated, editable by an administrator. Opt in with the `Settings` module.

```rust
use renox::prelude::*;
use renox::settings::{Setting, Settings, SettingsChanged, SettingsGroup};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Validate, Default, Clone)]
struct Company {
    #[validate(required, max(120))]
    name: String,
    tax_number: Option<String>,
}

impl SettingsGroup for Company {
    const KEY: &'static str = "company"; // one row per group (and scope)
    // const ENCRYPTED: bool = true;     // sealed with APP_KEY (gateway keys)
}

fn app() -> App {
    App::new()
        .module(Settings)                    // the table + settings:get|set|list
        .settings_in_views::<Company>()      // declares it; `{{ settings.company.name }}`
        // .settings::<Company>()            // declares it without sharing it with views
        // .settings_from("storage/settings.json") // seed groups never saved (saved values win)
        .listen(|e: SettingsChanged, _state| async move {
            let _ = (e.key, e.changed, e.actor, e.previous, e.current); // sealed: previous/current are null
            Ok(())
        })
}

async fn footer(company: Setting<Company>) -> String {
    format!("© {}", company.name)
}

async fn rename(state: AppState, user: User) -> Result {
    let company = state.settings::<Company>().await?; // Arc<Company>; the defaults until saved
    let mut changed = (*company).clone();
    changed.name = "Acme".into();
    state.save_settings(&changed, Some(&user)).await?; // validated, audited, emits SettingsChanged
    let _per_branch = state.settings_in::<Company>("branch-7").await?; // a scope; "" is the default
    Ok(())
}
```

`rnx settings:list`, `settings:get company[.name]`, `settings:set company.name "Acme"` (validated;
`--scope S`; encrypted groups print `<sealed>` unless `--reveal`). Admin page:
`Admin::settings::<Company>("Company")` with `impl AdminSettings for Company` (`fields()`,
`allows(user, "view" | "update")`; docs/admin.md). Tests: `TestApp::with_settings(Company { … })`.

## Tenants, roles and permissions

A tenant is one customer's own space, such as a team, whose data others must not see. Roles
and permissions say who may do what.

```rust
use renox::prelude::*;
use renox::auth::{Permissions, permissions};
use renox::axum::{extract::Request, middleware::{Next, from_fn}};

#[derive(Clone)]
struct CurrentTeam(i64);

// Every query of the model starts with its default scope; `Project::unscoped()` skips it.
#[derive(Model, serde::Serialize, Default)]
#[model(table = "projects", default_scope = "team_only")]
struct Project { id: i64, team_id: i64, name: String }

fn team_only(query: renox::db::Query<Project>) -> renox::db::Query<Project> {
    match renox::context::get::<CurrentTeam>() { // this request's (or job's) value
        Some(team) => query.where_(Project::TEAM_ID.eq(team.0)),
        None => query.none(), // no team, no rows
    }
}

async fn pick_team(user: Option<AuthUser>, req: Request, next: Next) -> Response {
    if let Some(team) = user.as_ref().and_then(|u| u.get::<i64>("team_id")) {
        renox::context::set(CurrentTeam(team)); // jobs/commands: set it from the payload
    }
    next.run(req).await
}

fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(Permissions) // roles, permissions, and loading them per request
        .layer(from_fn(pick_team))
        .gate_before(|user, _ability| (user.get::<bool>("super_admin") == Some(true)).then_some(true))
}

fn routes() -> Routes {
    let admin = Routes::new().get("/admin", || async { "hi" }).require_gate("admin"); // 403 unless
    let editors = Routes::new().get("/drafts", || async { "" }).require_role("editor");
    let publish = Routes::new().post("/publish", || async { "" }).require_permission("posts.publish");
    let api = Routes::new().post("/api/orders", || async { "" }).require_ability("orders:write");
    admin.merge(editors).merge(publish).merge(api) // shown in route:list as gate:admin, …
}

async fn setup(db: &Db, user: &User) -> Result {
    // A role must exist before assign_role / sync_roles: they fail for an unknown name.
    permissions::define_role(db, "editor", &["posts.create", "posts.publish"]).await?; // exactly these
    permissions::grant(db, "editor", &["posts.delete"]).await?; // add to an existing role
    permissions::revoke(db, "editor", &["posts.delete"]).await?; // take away
    let _all = permissions::roles(db).await?; // Vec<(role, its permissions)>, by name
    let _gone = permissions::delete_role(db, "intern").await?; // users lose it; false if none
    user.assign_role(db, "editor").await?; // also remove_role, sync_roles, roles, permissions
    let token = user.create_token_with(db, "reports", &["orders:read"], None).await?; // limited
    let _ = token.plain;
    // A kiosk or till without a user row: a device token owned by your own key.
    let kiosk = renox::auth::DeviceToken::create(db, "kiosk:3", "front", Some(&["sales:create"]), None).await?;
    let _ = kiosk.plain; // `Authorization: Bearer d1|…`; extract `renox::auth::Device`,
    // guard with Routes::require_device() / require_device_ability("sales:create")
    Ok(())
}

async fn check(user: AuthUser) -> String {
    // allows: gate_before, a gate, then permissions. has_role/has_permission: exactly that.
    // role_names(): the user's roles, loaded with the request.
    format!("{} {} {} {:?}", user.allows("posts.publish"), user.has_role("editor"),
        user.token_can("orders:read"), user.role_names())
}

#[derive(serde::Deserialize)]
struct ProjectForm { name: String, #[serde(default)] id: i64 }

impl Validate for ProjectForm {
    fn rules(&self, v: &mut Validator) {
        let team = renox::context::get::<CurrentTeam>().map_or(0, |t| t.0);
        v.field("name", &self.name)
            .unique("projects", "name")
            .ignore(self.id)
            .where_eq("team_id", team)    // unique per team
            .where_null("deleted_at");    // soft-deleted rows don't count
    }
}
```

Roles per store (branch, team), with dates: the role means the same everywhere, only its
assignment is scoped. Each request picks its store; checks count global roles plus the store's.

```rust
use renox::prelude::*;
use renox::auth::permissions::{self, Scope, Scopes};
use renox::axum::{extract::Request, middleware::Next};

#[derive(Model, serde::Serialize, Default)]
#[model(table = "stores")]
struct Store { id: i64 }

#[derive(Model, serde::Serialize, Default)]
#[model(table = "orders", default_scope = "my_stores")]
struct Order { id: i64, store_id: i64, pickup_store_id: i64 }

// Rows of the stores where the request's user may see orders (all for a global role).
fn my_stores(query: renox::db::Query<Order>) -> renox::db::Query<Order> {
    permissions::scopes_with::<Store>("orders.view").apply(query, &["store_id", "pickup_store_id"])
}

async fn give(db: &Db, user: &User, store: &Store) -> Result {
    let until = renox::db::now() + renox::chrono::Duration::days(14);
    user.assign_role_in(db, "manager", &Scope::of(store)).until(until).await?; // .from(date) too
    user.sync_roles_in(db, &["clerk"], &Scope::of_id::<Store>(2)).await?; // also remove_role_in
    let _list = user.assignments(db).await?; // role, scope, starts_at, ends_at
    let _managers = permissions::users_with_role_in(db, "manager", &Scope::of(store)).await?;
    Ok(())
}

// An App::layer (before the guards): this request works in the session's store.
async fn pick_store(session: Session, req: Request, next: Next) -> Response {
    if let Some(id) = session.get::<i64>("store_id") { // checked when it was chosen
        permissions::set_scope(Scope::of_id::<Store>(id));
    }
    next.run(req).await
}

impl Policy for Order { // a row's own store, not the request's
    fn allows(&self, user: &User, _ability: &str) -> bool {
        user.has_permission_in("orders.refund", &Scope::of_id::<Store>(self.store_id))
    }
}

fn stores(user: &AuthUser) -> Vec<i64> {
    match user.scopes_with::<Store>("orders.view") { Scopes::All => vec![], Scopes::Only(ids) => ids }
}
```

`rnx permissions:prune --days 30` deletes assignments that ended over 30 days ago (ended ones
already don't count).

In templates: `{% if can('posts.publish') %}` (a gate or a permission) and `auth.roles`.
`rnx tokens:prune` deletes API tokens that expired more than a day ago, and
`rnx notifications:prune --days 30` notifications read more than 30 days ago (unread ones stay).

## HTMX

htmx updates part of a page without a reload. A handler can answer with just one block of the
page, and send htmx headers that say what should happen next.

```rust
use renox::prelude::*;
use renox::{HxPushUrl, HxReswap, HxRetarget};

// A form posted with hx-post gets just the `list` block of the page back;
// a normal request gets the whole page.
async fn add(htmx: Htmx) -> Result<Response> {
    let page = view("todos/index.html", context! {}).fragment("list");
    if htmx.request {
        return Ok((HxTrigger("todo-added".into()), page).into_response());
    }
    Ok(Redirect::to("/todos").into_response())
}

async fn after_delete() -> HxRedirect {
    HxRedirect("/todos".into()) // the browser navigates there
}

async fn after_save(htmx: Htmx) -> Response {
    htmx.redirect("/todos") // HX-Redirect for htmx posts, 303 for plain forms
}

async fn after_toggle() -> (Toast, HxRetarget, HxReswap, HxPushUrl, View) {
    // A toast (HX-Trigger for htmx; the next page otherwise), and several blocks at once:
    // blocks after the first need an id and hx-swap-oob="true".
    (
        // Also info, warning, error (errors stay until dismissed); .body("…"),
        // .link("View", "/orders/7"), .action(ToastAction::event("Undo", "undo")),
        // .action(ToastAction::delete("Undo", "/orders/7/archive")) (a request with the CSRF
        // token, own paths only; answer it with a Toast / HxRefresh / HxRedirect),
        // .seconds(8) / .persistent(), .id("x"). Layout: toasts(position="bottom-end").
        // In page scripts: Renox.toast({kind, message, body, actions}), Renox.dismissToast(id),
        // Renox.request("POST", "/orders/7/retry").
        Toast::success("Order updated"),
        HxRetarget("#orders".into()),
        HxReswap("outerHTML".into()),
        HxPushUrl("/orders?status=open".into()),
        view("orders/index.html", context! {}).fragment("rows").also("count"),
    )
}

async fn after_import(htmx: Htmx) -> Response {
    if htmx.wants_fragment() { // htmx, but not hx-boost
        return HxRefresh.into_response(); // the browser reloads the page
    }
    Redirect::to("/todos").into_response()
}
```

`Back` redirects to the previous page (the Referer). `<form data-live-validate>` (with the UI
kit's script) checks each field against `Valid<T>`'s rules as it's left, without running the
handler. HTMX requests send the CSRF token by
themselves. Errors of list items (`photos.1`, `tags.0`) show at the list's input and its
`data-error-for="photos"` slot, and `{{ error('photos') }}` includes them. Boosted requests (`hx-boost`) get whole pages, so pair them with `hx-select`.

## Live components (details in [docs/live.md](docs/live.md))

A struct holds the state, `#[live(action)]` methods change it, and the view re-renders in place.
`rx-click="toggle(3)"` (JSON arguments), `rx-submit="save"` and `rx-model` / `rx-model.live` /
`rx-model.blur` call them. The state travels in a signed snapshot: no secrets in fields.

```rust
use renox::live_component::LiveContext;
use renox::prelude::*;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Cart {
    lines: Vec<i64>,
    note: String,
}

#[renox::live_component(view = "live/cart.html", name = "cart")]
impl Cart {
    // The view gets `state` (the fields) and `data` (this).
    async fn data(&self, _ctx: &LiveContext) -> Result<serde_json::Value> {
        Ok(json!({ "count": self.lines.len() }))
    }

    #[live(action)]
    async fn add(&mut self, ctx: &mut LiveContext, id: i64) -> Result {
        self.lines.push(id);
        ctx.toast(Toast::success("Added")); // also ctx.validate, ctx.redirect, ctx.dispatch
        Ok(())
    }
}

// App::new().live_component::<Cart>(); a page handler mounts it, and the template shows it with <live-cart :component="cart"/>.
async fn cart_page(ctx: LiveContext) -> Result<View> {
    let cart = ctx.mount(Cart::default()).await?;
    Ok(view("cart.html", context! { cart }))
}

// In tests: let mut c = app.live(Cart::default()); c.call_with("add", json!([3])).await;
```

## State machines (details in [docs/state-machines.md](docs/state-machines.md), example: [the bike shop's rentals](examples/bikeshop/src/app/rentals/machine.rs))

A model with a status declares its states and transitions once; every change goes through
`record.transition(name)`, which checks who, the `from` state and the guards, moves the status
with an optimistic update and writes a history row, in one transaction.
`#[model(state_machine)]` makes `save` refuse a changed status. Errors: 401, 403, 404 and 422
(`TransitionError::into_error`). `.system()` transitions run with `by_system()` only.

```rust
use renox::prelude::*;
use renox::state_machine::{self, Machine, Outcome, StateMachine, Transition, TransitionError};

#[derive(DbEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum OrderStatus { Draft, Placed, Cancelled }

#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "orders", state_machine)]
struct Order { id: i64, status: String, lines: i64 }

impl StateMachine for Order {
    type State = OrderStatus;
    const COLUMN: renox::db::Col<Self, OrderStatus> = renox::db::Col::new("status");

    fn machine() -> Machine<Self> {
        Machine::new()
            .initial([OrderStatus::Draft])
            .state(OrderStatus::Cancelled).tone("danger").terminal()
            .transition(
                Transition::new("place").label("Place order")
                    .from([OrderStatus::Draft]).to(OrderStatus::Placed)
                    .permission("orders.place") // or .permission_in(name, |o| scope) / .system()
                    .guard("has_lines", |o: &Order, _ctx| Box::pin(async move {
                        if o.lines > 0 { Ok(()) } else { Err("The order has no lines.".into()) }
                    })),
            )
            .transition(
                Transition::new("cancel")
                    .from([OrderStatus::Draft, OrderStatus::Placed]).to(OrderStatus::Cancelled)
                    .reason_required(),
            )
    }
}

// App::new().state_machine::<Order>()  (rnx state:diagram, the admin panel, db:check)
async fn cancel(state: AppState, user: AuthUser, order: Order) -> Result<Vec<serde_json::Value>> {
    order.transition("cancel").by(&user).reason("customer asked").run(&state).await
        .map_err(TransitionError::into_error)?;   // run_in(&mut tx) does not commit: outcome.emit(&state) after
    let actions = order.available(&user, &state.db).await?; // name, label, tone, needs_reason, requires_approval
    let _ = actions; // <rx-transition-actions :actions="actions" action="/orders/1/transitions"/>
    // Routes::new().group("/orders", "orders.", state_machine::routes::<Order>())
    let rows = state_machine::history(&state.db, "orders", order.id).await?;
    Ok(state_machine::timeline(&rows, Some(&state_machine::info_of::<Order>()))) // <rx-history :items=…/>
}
// Listen: app.listen(|e: Transitioned<Order>, state: AppState| async move { Ok(()) })
// Approvals: state_machine::set_interceptor(impl TransitionInterceptor) → Decision::Defer → Outcome::Pending
```

## Raw SQL and transactions (SQLite and PostgreSQL)

When the query builder isn't enough, write SQL yourself, with `?` where each value goes. A
transaction groups changes so that either all of them happen, or none.

```rust
use renox::prelude::*;

async fn report(db: &Db) -> Result {
    // `.bind` fills in each `?` in order, safely: a value can never change the SQL.
    let rows = renox::db::sql("SELECT name, price FROM products WHERE price < ?")
        .bind(20_000)
        .fetch_all(db)
        .await?;
    let name: String = rows[0].try_get("name")?;
    let total: i64 = renox::db::sql("SELECT CAST(SUM(price) AS BIGINT) FROM products")
        .scalar(db)
        .await?;
    // Other readers: fetch_one (an error without a row), fetch_optional, fetch_as::<T> /
    // fetch_one_as / fetch_optional_as (into a FromRow type or a tuple), scalar_optional
    // (None without a row), scalars (the first column of every row).
    let names: Vec<String> = renox::db::sql("SELECT name FROM products").scalars(db).await?;
    let first = renox::db::sql("SELECT * FROM products").fetch_optional(db).await?;
    let columns: Vec<String> = match &first {
        Some(row) => row.columns().into_iter().map(String::from).collect(), // the column names
        None => Vec::new(),
    };

    let mut tx = db.begin().await?; // pass `&mut tx` wherever `db` goes
    renox::db::sql("UPDATE products SET price = price + ?").bind(1_000).execute(&mut tx).await?;
    // A savepoint: on Err only its own changes are undone and the transaction goes on (nests).
    let bonus: Result<u64> = tx
        .savepoint(|tx| Box::pin(async move { Ok(renox::db::sql("UPDATE stock SET n = n - 1").execute(&mut *tx).await?) }))
        .await;
    tx.commit().await?; // dropped without commit = rolled back

    // Retried on SQLite busy / PostgreSQL conflicts; borrows freely, may roll back with a value.
    let raised = db
        .retrying(3, || async {
            let mut tx = db.begin().await?;
            let n = renox::db::sql("UPDATE products SET price = ? WHERE price < ?")
                .bind(total) // a local, borrowed
                .bind(total)
                .execute(&mut tx)
                .await?;
            if n > 100 {
                tx.rollback().await?;
                return Ok(0); // too many: undone, still an answer
            }
            tx.commit().await?;
            Ok(n)
        })
        .await?;
    let _ = (name, raised, bonus, names, columns);
    Ok(())
}

/// A helper that takes `&db` or `&mut tx` and runs two statements on it.
async fn record<'c>(db: impl renox::db::Executor<'c>, product: i64, quantity: i64) -> Result {
    let mut conn = db.into_conn(); // each statement gets conn.reborrow()
    renox::db::sql("INSERT INTO movements (product_id, quantity) VALUES (?, ?)")
        .bind(product)
        .bind(quantity)
        .execute(conn.reborrow())
        .await?;
    renox::db::sql("UPDATE levels SET quantity = quantity + ? WHERE product_id = ?")
        .bind(quantity)
        .bind(product)
        .execute(conn.reborrow())
        .await?;
    Ok(())
}
```

SQLite has one writer: while a transaction is open, write through `&mut tx`, not `db` (that
waits for the transaction and fails with "database is locked"). Queries fail with a
`renox::db::DbError` (`is_unique_violation()`, `is_foreign_key_violation()`, `is_row_not_found()`,
`is_timeout()`); a duplicate that slips past a `unique` rule answers 409.

Numbered documents with no gaps (invoices, receipts), details in
[docs/relations.md](docs/relations.md) "Numbering documents":

```rust
use renox::db::{NumberFormat, Period, Sequence};
use renox::prelude::*;

async fn issue(db: Db, company_id: i64) -> Result<String> {
    let today = renox::db::now().date_naive();
    let seq = Sequence::new("invoice").scope(company_id).period(Period::of_year(today));
    let mut tx = db.begin().await?;
    let n = seq.next(&mut tx).await?; // 1, 2, 3…; a rollback gives the number back
    let number = Sequence::format("INV-{yyyy}-{0000}", n, today); // INV-2026-0042
    tx.commit().await?;
    let _next = seq.peek(&db).await?;          // the next number, not taken
    seq.set_start(&db, 1200).await?;           // continue from an old system: next is 1201
    let _loose = seq.next_allowing_gaps(&db).await?; // own transaction, gaps possible
    let _custom = NumberFormat::new("{prefix}-{0000}").token("prefix", "ACM").render(n, today);
    Ok(number)
}
```

Migrations run in a transaction each. A migration with `CREATE INDEX CONCURRENTLY` or its own
`BEGIN … COMMIT` runs without one, as does one with a `-- renox:no-transaction` line. Two
`migrate` runs at once wait for each other; `migrate:status` flags edited and missing files.

## Jobs, events, schedule, mail (details in [docs/queue.md](docs/queue.md) and [docs/scheduling.md](docs/scheduling.md))

A job is work done in the background, such as sending a mail, so the page answers fast. An
event says "this happened", and listeners react to it. The schedule runs tasks at set times.

```rust
use renox::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct SendReceipt {
    order_id: i64, // keep jobs small: ids, not whole models
}

/// A job: Renox stores it in the `jobs` table, and a worker runs `handle` soon after.
impl Job for SendReceipt {
    const NAME: &'static str = "send-receipt";
    const QUEUE: &'static str = "mail";                // default "default"; queue:work --queue mail
    const MAX_ATTEMPTS: u32 = 5;                       // default 3
    const TIMEOUT: Duration = Duration::from_secs(30); // per attempt; default 60 s

    // Also: const ENCRYPTED (payload sealed with APP_KEY), const UNIQUE_FOR + fn unique_id
    // (one queued at a time), fn middleware (Middleware::without_overlapping(key),
    // rate_limited(key, max, per)), async fn failed(self, ctx, error) once it fails for good.

    async fn handle(self, ctx: JobContext) -> Result {
        let mail = ctx.state.mail_view(
            "buyer@example.com",
            "Your receipt",
            "mail/receipt", // resources/views/mail/receipt.html (+ .txt)
            context! { order_id => self.order_id },
        )?;
        ctx.state.mailer.send(mail).await
    }
}

#[derive(Clone)]
struct OrderPlaced {
    order_id: i64,
}

impl Event for OrderPlaced {}

/// Registers the job, a listener for the event, and the scheduled tasks.
fn background(app: App) -> App {
    app.job::<SendReceipt>()
        .listen(|event: OrderPlaced, state| async move {
            state.dispatch(SendReceipt { order_id: event.order_id }).await?;
            Ok(())
        })
        .schedule(|s| {
            // Also every_minute(name, task), hourly(name, task), every(Duration, name, task).
            s.every_minutes(5, "sync-stock", |_state| async move { Ok(()) }) // :00, :05, … on the clock
                .weekdays() // also weekends(), days(&[Weekday::Sat])
                .between("08:00", "18:00");
            s.daily_at("02:00", "cleanup", |state| async move { // in APP_TIMEZONE
                renox::db::sql("DELETE FROM carts WHERE updated_at < ?")
                    .bind(renox::db::now() - renox::chrono::TimeDelta::days(30))
                    .execute(&state.db)
                    .await?;
                Ok(())
            });
            s.cron("30 9 * * 1-5", "standup", |_state| async move { Ok(()) }) // min hour day month weekday
                .timezone("Europe/Amsterdam") // instead of APP_TIMEZONE; DST handled
                .on_failure(|err, _state| async move { eprintln!("standup failed: {err:?}") })
                .on_success(|_state| async move {});
            s.weekly_on(renox::chrono::Weekday::Mon, "07:00", "weekly", |_state| async move { Ok(()) });
            s.monthly_on(1, "00:05", "invoices", |_state| async move { Ok(()) });
        })
}

async fn place_order(State(state): State<AppState>) -> Result<Redirect> {
    state.emit(OrderPlaced { order_id: 1 }).await?; // listeners run, the job is queued
    state.queue.dispatch_after(SendReceipt { order_id: 1 }, Duration::from_secs(3600)).await?; // in an hour
    Ok(Redirect::to("/orders"))
}

async fn several(State(state): State<AppState>) -> Result<String> {
    let queue = &state.queue;
    queue.chain().then(SendReceipt { order_id: 1 }).then(SendReceipt { order_id: 2 }).dispatch().await?; // in order
    let batch = queue
        .batch("receipts") // side by side; the first failure cancels the rest unless .allow_failures()
        .push(SendReceipt { order_id: 3 })
        .push(SendReceipt { order_id: 4 })
        .then(SendReceipt { order_id: 5 }) // all succeeded; also .catch(job), .finally(job)
        .dispatch()
        .await?;
    let progress = queue.batch_status(batch).await?.map_or(0, |b| b.progress()); // 0–100
    queue.dispatch_on("high", SendReceipt { order_id: 6 }).await?; // queue:work --queue high,default
    state.dispatch_sync(SendReceipt { order_id: 7 }).await?; // now, in this request
    Ok(format!("{progress}%"))
}

async fn checkout(State(state): State<AppState>) -> Result {
    let mut tx = state.db.begin().await?;
    // … insert the order with `&mut tx`
    state.queue.dispatch_in(&mut tx, SendReceipt { order_id: 1 }).await?; // only if committed
    tx.commit().await?;
    Ok(())
}

fn card_number(raw: &str) -> Result<u64> {
    // Retrying can't fix a bad number: the job goes straight to failed_jobs.
    raw.parse::<u64>().map_err(Error::permanent)
}
```

Jobs and scheduled tasks run inside `rnx serve` / `my-app serve` (`QUEUE_WORKERS`, `SCHEDULER`).
`schedule:list` shows each task's next run and zone, `schedule:run NAME` runs one now (also
`app.run_scheduled(name)` in tests, at the travelled time). `.module(renox::queue::Dashboard)` adds `/_renox/queue`
(waiting jobs, throughput, failed jobs to retry or forget, batches) for users who pass the
`view-queue-dashboard` gate (`renox::queue::DASHBOARD_GATE`); `state.queue.stats()` has the same
numbers for monitoring.
A job that errors, panics or passes its `TIMEOUT` is retried up to `MAX_ATTEMPTS`, then moved to
`failed_jobs` (`queue:failed`, `queue:retry`, `queue:forget`, `queue:prune-failed`); a panicking
task or listener doesn't stop the others.

## Mail and notifications (details in [docs/mail.md](docs/mail.md))

Mails are built from templates. A notification is one message that can go out through
several channels: mail, the database (an inbox in the app), or your own, such as WhatsApp.

```rust
use renox::prelude::*;
use renox::auth::{Channel, DatabaseMessage, Notification, Recipient};
use renox::mail::Mail;

/// Builds a mail from a template, adds recipients and a PDF, and queues it.
async fn send_invoice(state: &AppState, pdf: Vec<u8>) -> Result {
    let mail = state
        .mail_view("ben@example.com", "Invoice INV-001", "mail/invoice", context! {})? // .html + .txt
        .also_to("sarah@example.com")
        .cc("sales@example.com")
        .bcc("archive@example.com")
        .reply_to("Coffee Shop <hello@shop.example>")
        .from("Billing <billing@shop.example>") // instead of MAIL_FROM_*
        .attach("INV-001.pdf", "application/pdf", pdf);
    state.queue_mail(mail).await?; // or state.mailer.send(mail).await? for now
    // A mailer of its own: App::mailer("newsletter", |c| MailConfig::from_env(c, "NEWSLETTER")),
    // then state.mailer_named("newsletter")?.send(…) or state.queue_mail_via("newsletter", …).
    // MAIL_FAILOVER=backup tries the `backup` mailer when the default one fails.
    Ok(())
}

/// A calendar invitation (RFC 5545): UTC times, folded and escaped for you. The same UID with a
/// higher sequence changes it; `Status::Cancelled` makes the file `METHOD:CANCEL`.
fn invitation(starts: renox::chrono::DateTime<renox::chrono::Utc>, ends: renox::chrono::DateTime<renox::chrono::Utc>) -> Mail {
    use renox::calendar::{Event, Ics, Status};
    let ics = Ics::new("-//Acme//Bookings//EN").event(
        Event::new("booking-42@acme.test", starts, ends)
            .summary("Service at Acme").location("Bay 3").description("Bring the key.")
            .organizer("bookings@acme.test").attendee("ben@example.com")
            .alarm_before(std::time::Duration::from_secs(3600))
            .sequence(0).status(Status::Confirmed),
    );
    // invite.ics as `text/calendar; method=REQUEST; charset=UTF-8`; Download::ics(&ics, "booking.ics")
    Mail::new("ben@example.com", "Your booking", "See you on Tuesday.").attach_ics(&ics)
}

struct OrderShipped { order_id: i64 }

/// One message, sent through every channel that `channels` returns.
impl Notification for OrderShipped {
    fn kind(&self) -> &'static str { "order-shipped" }
    fn channels(&self, to: &Recipient) -> Vec<Channel> {
        // Per recipient (Laravel's `via`); only Channel::Mail by default.
        let mut channels = vec![Channel::Mail, Channel::Database];
        if to.address("whatsapp").is_some() {
            channels.push(Channel::Custom("whatsapp"));
        }
        channels
    }
    fn to_mail(&self, to: &Recipient, state: &AppState) -> Result<Mail> {
        // Built in the recipient's language (Recipient::locale: in_locale(..) or users.locale):
        // t() in the mail view and state.current_lang() here use it.
        let subject = state.current_lang().t("mail.shipped", &[("id", &self.order_id)]);
        Ok(Mail::new(to.email().unwrap_or_default(), subject, "On its way."))
    }
    fn to_database(&self, _: &Recipient, _: &AppState) -> Result<renox::serde_json::Value> {
        // What the kit's notification_bell shows; any JSON works for your own list.
        Ok(DatabaseMessage::success(format!("Order #{} shipped", self.order_id))
            .body("It arrives in 2–3 days.")
            .url(format!("/orders/{}", self.order_id))
            .with("order_id", self.order_id) // n.data.order_id; n.message() reads it back
            .into())
    }
    fn to_channel(&self, _: &str, _: &Recipient, _: &AppState) -> Result<renox::serde_json::Value> {
        Ok(json!({ "text": format!("Order #{} shipped", self.order_id) }))
    }
}

fn channels(app: App) -> App {
    // Your own channel: call WhatsApp, SMS or Slack with what `to_channel` built.
    app.channel("whatsapp", |to: Recipient, message, _state| async move {
        let phone = to.address("whatsapp").or_else(|| to.user()?.get("phone"));
        let _ = (phone, message);
        Ok(())
    })
    // Cap queued sends (notify_later) to this channel: 20 a second, shared by every worker;
    // the rest wait for the next window without using an attempt. Unknown name = boot error.
    .channel_rate_limit("whatsapp", 20, std::time::Duration::from_secs(1))
    // Same for queued mail through a named mailer ("default" = the default one):
    // .mailer_rate_limit("bulk", 100, std::time::Duration::from_secs(60))
}

async fn ship(state: &AppState, user: &User) -> Result {
    state.notify(user, &OrderShipped { order_id: 7 }).await?;       // now
    state.notify_later(user, &OrderShipped { order_id: 7 }).await?; // one queued job per channel
    let guest = Recipient::to("mail", "guest@example.com").and("whatsapp", "+15550123").in_locale("es");
    state.notify(&guest, &OrderShipped { order_id: 7 }).await      // no account: no database row
}
```

The bell: `App::new().module(Auth::new().notifications())`, then in the layout's bar
`<rx-notification-bell :count="unread_notifications"/>`:
a badge, a panel of the latest (mark read/unread, delete, mark all read, clear, open = read +
follow `url`), new ones live over Server-Sent Events (a toast + the badge), and the page
`notifications.index` without JavaScript. Behind nginx: `proxy_buffering off` for
`/notifications/stream`. A new app with it: `rnx new my-app --notifications`.

Your own live events over the same stream (DOM events on `document`; the page needs the bell
or `{{ event_stream() }}`; this process only, nothing stored, so missed by pages not open):

```rust
use renox::prelude::*;

fn order_paid(state: &AppState, buyer_id: i64) -> Result {
    // Every open page: <div hx-get="/orders" hx-trigger="order-updated from:document" …>
    state.broadcast("order-updated", json!({ "id": 7, "status": "paid" }))?;
    // One user's pages; `renox:toast` shows toasts.
    state.broadcast_to(buyer_id, "renox:toast", json!({ "toasts": [Toast::success("Paid")] }))
}
```

Mail views can `{% from "renox/mail/components.html" import button, panel, table, divider %}`:
`{{ button(url, t('mail.track')) }}`, `{% call panel() %}…{% endcall %}`,
`{{ table(rows, head=[…], total=[…]) }}`. `state.mail_view_in("es", …)` renders in a given
language; `state.lang("es").t(…)` and `renox::i18n::set_current_locale("es")` (for the rest of
a job) help code outside a request.

## Calling other services (HTTP)

Call another service's API with `state.http`. In tests, a fake answers instead.

```rust
use renox::prelude::*;
use std::time::Duration;

#[derive(serde::Deserialize)]
struct Rate { eur: f64 }

async fn rate(state: &AppState) -> Result<f64> {
    let rate: Rate = state
        .http // also post/put/patch/delete; .json(&body), .form(&body), .header(..), .basic_auth(..)
        .get("https://api.example.com/rates")
        .query(&[("base", "USD")])
        .bearer("token")
        .timeout(Duration::from_secs(5)) // 30 s by default
        .retry(3, Duration::from_millis(200)) // connection errors, 429, 5xx
        .send()
        .await?
        .error_for_status()? // 4xx/5xx → Err
        .json()?;
    Ok(rate.eur)
}
```

In tests: `let http = app.fake_http(); http.on("https://api.example.com/*",
FakeResponse::json(200, json!({ "eur": 0.92 })));` then `http.assert_sent(|r| …)`. Requests
without a fake fail, so tests never reach the network. Scheduled tasks can ping health checks:
`.ping_before(url)`, `.then_ping(url)`, `.ping_on_success(url)`, `.ping_on_failure(url)`.

## Cookies and downloads

Set and read cookies, and send a file for the browser to show or save.

```rust
use renox::prelude::*;
use renox::{Cookies, Download, SetCookie};
use std::time::Duration;

async fn remember_theme(State(state): State<AppState>) -> (SetCookie, Redirect) {
    let cookie = SetCookie::new(&state, "theme", "dark").max_age(Duration::from_secs(365 * 86_400));
    (cookie, Redirect::to("/")) // HttpOnly, SameSite=Lax, Secure on https; SetCookie::encrypted / remove
}

async fn theme(cookies: Cookies) -> String {
    cookies.get("theme").unwrap_or_default() // get_encrypted for SetCookie::encrypted
}

async fn invoice(State(state): State<AppState>) -> Result<Download> {
    let pdf = state.storage.get("invoices/1.pdf").await?.unwrap_or_default();
    Ok(Download::bytes("INV-001.pdf", "application/pdf", pdf).inline()) // or attachment (default)
    // Download::file(path, name).await (streamed), Download::from_storage(&storage, key, name),
    // Download::stream(name, type, stream) for a CSV written row by row,
    // Download::ics(&ics, "booking.ics") for a calendar file (renox::calendar::Ics)
}
```

## Cache, session, uploads, translations

The cache keeps results you don't want to work out again. The session remembers things about
one visitor between pages. Uploads are files sent with a form. Translations show the app in
the visitor's language.

```rust
use renox::prelude::*;
use std::time::Duration;

async fn misc(State(state): State<AppState>, session: Session, lang: Lang) -> Result<String> {
    // Read from the cache, or run the query and keep its answer for 60 seconds.
    let count: i64 = state
        .cache
        .remember("products.count", Duration::from_secs(60), || async {
            renox::db::sql("SELECT COUNT(*) FROM products").scalar(&state.db).await.map_err(Into::into)
        })
        .await?;
    let visits = state.cache.increment("visits", 1).await?; // atomic; also decrement
    let first = state.cache.add("promo-sent:7", &true, None).await?; // false if already there
    let otp: Option<String> = state.cache.pull("otp:7").await?; // read and remove
    let lock = state.cache.lock("stock:42", Duration::from_secs(30)); // across servers with CACHE_STORE=database
    if let Some(guard) = lock.try_acquire().await? {
        // … one worker at a time; lock.block(Duration::from_secs(5)) waits instead (423 after)
        guard.release().await?; // or drop it
    }
    let files = state.storage.list("invoices/2026").await?; // key, size, modified; any depth
    state.storage.copy("public/a.png", "public/b.png").await?; // also rename (move), size, delete_all(prefix)
    let _ = (visits, first, otp, files);
    // The session belongs to this visitor only, and lasts from one request to the next.
    session.put("cart", vec![1, 2, 3])?;
    session.push("cart", 4)?; // appends to the list (makes one if needed)
    let visits_here = session.increment("visits", 1)?; // 1, 2, …
    let cart: Option<Vec<i64>> = session.get("cart");
    let _ = visits_here;
    // Also: pull (read and remove), remove, regenerate_token(), set_lifetime(Duration).
    let _ = cart;
    Ok(lang.choice("cart.count", count, &[])) // "12 items"; lang.t("cart.hello", &[("name", &"Anna")])
}

async fn switch_language(session: Session, back: Back) -> Result<Back> {
    renox::i18n::remember_locale(&session, "es")?; // (with lang/es.json) this visitor's language from the next request on
    // (App::new().detect_locale() picks the browser's language until the visitor chooses one)
    Ok(back)
}

#[derive(serde::Deserialize)]
struct PhotoForm {
    photo: Upload, // from a multipart form
}

impl Validate for PhotoForm {
    fn rules(&self, v: &mut Validator) {
        v.field("photo", &self.photo).required().image().max(2048); // KB
    }
}

async fn upload(State(state): State<AppState>, Valid(form): Valid<PhotoForm>) -> Result<String> {
    let key = form.photo.store_public(&state.storage, "photos").await?;
    Ok(state.storage.url(&key)) // {{ storage_url(key) }} in templates
}
```

Steps every upload passes after the form's rules (`App::uploads`); a refusal is an error on the
file's field (plain post and HTMX `422`). `store_encrypted` seals a file with `APP_KEY`:

```rust
use renox::prelude::*;
use renox::upload::{ClamAv, Pipeline, StepOutcome, UploadContext, UploadStep};
use renox::upload::BoxFuture;

struct Shout; // any `impl UploadStep`: refuse, change the file, or derive another

impl UploadStep for Shout {
    fn process<'a>(
        &'a self,
        file: &'a mut Upload,
        ctx: &'a UploadContext,
    ) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            Ok(if file.size() == 0 {
                StepOutcome::Reject(format!("{} is empty.", ctx.field()))
            } else {
                StepOutcome::Keep
            })
        })
    }
}

fn app() -> App {
    // ClamAV fails closed (`.timeout(..)`, `.fail_open()`); `Images::new().max_dimensions(2560, 2560)
    // .strip_metadata().thumbnail("thumb", 320, 320)` with renox's `images` feature.
    App::new().uploads(Pipeline::new().step(ClamAv::tcp("127.0.0.1:3310")).step(Shout))
}

async fn keep(State(state): State<AppState>, mut id: Upload) -> Result<Vec<u8>> {
    id.process(&state).await?; // outside `Valid`: run the steps by hand
    let key = id.store_encrypted(&state.storage, "ids").await?; // AES-256-GCM, a data key per file
    let thumb = Upload::derived_key(&key, "thumb"); // where a derived file is stored
    let _ = thumb;
    Ok(state.storage.get_decrypted(&key).await?.unwrap_or_default().to_vec())
    // Download::decrypted(&state.storage, &key, "id.jpg"); storage.rewrap_encrypted("", &old_app_key)
}
```

Renox's own texts are English. Translations live in `resources/lang/<locale>.json` (e.g.
`es.json`), nested or flat, and also translate Renox's texts (`ui.*`, `renox.auth.*`,
`renox.validation.*`); `detect_locale` picks only languages the app has a file for. `one|many` texts are plurals,
and Laravel's ranges pick by count (`{n}` exactly, `[a,b]`, `*` for open):

```json
{ "cart": { "count": "One item|:count items", "hello": "Hello, :name",
            "stock": "{0} Sold out|[1,5] Only :count left|[6,*] In stock" } }
```

```html
<p>{{ t('cart.count', count=cart_count) }} · {{ t('cart.hello', name=auth.user.name) }}</p>
```

Right-to-left (details in docs/ui.md "Right-to-left"): `app.dir` is `rtl` for `ar`, `he`, `fa`, `ur`, `ps`, `yi`,
`dv` and `ckb` (by base language: `ar-AE` too), `ltr` otherwise, also in mails (the mail's own locale). Every
layout writes it; the kit mirrors by itself and loads Noto Sans Arabic only for Arabic text. Write your own CSS
with logical properties (`margin-inline-start`, `inset-inline-end`, `text-align: start`), never `left`/`right`.

```rust
# use renox::prelude::*;
use renox::i18n::{direction, Direction};
assert_eq!(direction("ar-AE"), Direction::Rtl); // .as_str() is "rtl"; AppState::direction(locale) honours the override
# let _ =
App::new().rtl_locales(&["ar", "he", "sd"]) // replaces the default list
# ;
```

```html
<html lang="{{ app.locale }}" dir="{{ app.dir }}">
```

## Signed URLs and private files

A signed URL is a link with a secret stamp: if someone changes it, or it expires, it stops
working. Use it to share something private without asking for a login.

```rust
use renox::prelude::*;
use renox::signed::ValidSignature;
use std::time::Duration;

// Routes::new().get("/invoices/{id}", show_invoice).name("invoices.show")
async fn share(State(state): State<AppState>, Path(id): Path<i64>) -> Result<String> {
    state.signed_url("invoices.show", &[&id], Duration::from_secs(3600)) // absolute; an hour
}

async fn show_invoice(_: ValidSignature, Path(id): Path<i64>) -> String {
    format!("Invoice {id}") // an altered or expired link gets 403
}

async fn private_file(State(state): State<AppState>) -> Result<Redirect> {
    // A key outside `public/`, as a link that works for 10 minutes (presigned on S3).
    let url = state.storage.temporary_url(&state, "invoices/1.pdf", Duration::from_secs(600)).await?;
    Ok(Redirect::to(&url))
}
```

## More disks

A disk is a place to keep files: a folder on the server, or an S3 bucket. Next to the default
disk, you can add more, each with a name.

```rust
use renox::prelude::*;
use renox::storage::StorageConfig;

fn with_disks(app: App) -> App {
    // EXPORTS_DISK=s3 + EXPORTS_BUCKET=… (keys from S3_*), or local: storage/exports.
    app.disk("exports", |config| StorageConfig::from_env(config, "EXPORTS"))
}

async fn save(State(state): State<AppState>) -> Result<String> {
    let exports = state.disk_named("exports")?; // an unknown name is a 500
    exports.put("orders.csv", "id,total\n".into()).await?;
    // Public keys: `/_renox/disks/exports/public/…`; the rest through temporary_url.
    Ok(exports.url("public/logo.png"))
}
```

## Security: CSP, CORS, webhooks

Renox turns on the browser's protections for you. These examples change them where an app
needs to: allow scripts from another site (CSP), let another site call your API (CORS), and
accept posts from other services without a CSRF token.

```rust
use renox::prelude::*;

fn secured(app: App) -> App {
    // Every response gets nosniff, Referrer-Policy, X-Frame-Options, HSTS
    // (production + https) and a Content-Security-Policy (`CSP=relaxed|strict|off`).
    app.csp(|csp| {
        csp.allow("script-src", "https://www.googletagmanager.com")
            .allow("frame-src", "https://www.youtube.com");
    })
    // The CSRF token also as an `XSRF-TOKEN` cookie, accepted back in `X-XSRF-TOKEN` (axios).
    .xsrf_cookie()
    // The Permissions-Policy switches off camera, microphone, geolocation, payment, USB...
    // (`PERMISSIONS_POLICY=default|off|<header value>`); allow what the app uses.
    .permissions_policy(|p| {
        p.allow("geolocation", "self")
            .allow("payment", "https://pay.example.com");
    })
    // TRUSTED_HOSTS=example.com,*.example.com: other `Host`s get a 400.
}

fn routes() -> Routes {
    let api = Routes::new()
        .get("/api/stock", || async { "12" })
        .cors(&["https://app.example.com"]) // or &["*"]
        .etag(); // `ETag`, and 304 when the browser has this version
    let webhooks = Routes::new()
        .post("/webhooks/payment", || async { StatusCode::OK })
        .without_csrf(); // no session: check the gateway's signature instead
    let pages = Routes::new()
        .view("/about", "pages/about.html") // no handler needed
        .redirect("/about-us", "/about"); // 302 (`permanent_redirect`: 301)
    api.merge(webhooks).merge(pages)
}
```

A webhook is a message another service sends to your app, such as a payment gateway saying "this
invoice was paid". Renox checks it really came from them, stores each event once and handles
it in the queue (`webhook:failed`, `webhook:retry <id>`):

```rust
use renox::prelude::*;
use renox::webhook;

struct Xendit;

impl Webhook for Xendit {
    const PROVIDER: &'static str = "xendit";

    fn verify(request: &WebhookRequest, state: &AppState) -> Result {
        let token = webhook::secret(state, "XENDIT_CALLBACK_TOKEN")?; // from .env
        let sent = request.header("x-callback-token").unwrap_or_default();
        webhook::ensure(webhook::same(sent, &token)) // or verify_hmac_sha256, verify_timestamped
    }

    fn event_id(request: &WebhookRequest) -> Result<String> {
        let invoice: serde_json::Value = request.json()?;
        Ok(format!("{}:{}", invoice["id"], invoice["status"]))
    }

    async fn handle(call: WebhookCall, ctx: JobContext) -> Result {
        let invoice: serde_json::Value = call.json()?;
        let _ = (invoice, ctx.state); // mark the order paid; errors are retried
        Ok(())
    }
}

// Module::routes:   Routes::new().webhook::<Xendit>("/webhooks/xendit")
// Module::register: app.webhook::<Xendit>();
```

Outbound webhooks (your app tells others; Standard Webhooks headers `webhook-id`, `webhook-timestamp`,
`webhook-signature`; retries 5 s … 10 h, 8 tries; `docs/operations.md` "Outbound webhooks"):

```rust
# use renox::prelude::*;
# #[derive(Clone)] struct InvoicePaid { id: i64 }
# impl Event for InvoicePaid {}
# #[derive(serde::Deserialize)] struct Ping { note: String }
# async fn demo(state: AppState, url: &str, body: serde_json::Value) -> Result {
use renox::webhook::outbound::{emit_in, Endpoint, EndpointDisabled};

// Module::register: name the event and its payload `data`.
let _ = App::new().webhook_event::<InvoicePaid>("invoice.paid", |e| serde_json::json!({ "invoice": e.id }));

// Subscribe a URL (https; empty events = all). The `whsec_…` secret is returned once, stored sealed.
let (endpoint, secret) = Endpoint::create(&state, "https://example.com/hooks", "Billing", &["invoice.paid"]).await?;
Endpoint::list(&state.db).await?;                 // find, enable, disable(&db, id, reason), set_events, delete

state.emit(InvoicePaid { id: 7 });                  // queues a delivery per subscribed endpoint
let mut tx = state.db.begin().await?;             // from a database change: sent only if it commits
emit_in(&state, &mut tx, &InvoicePaid { id: 7 }).await?;
tx.commit().await?;
// 410 disables an endpoint at once; WEBHOOK_DISABLE_AFTER (20) failed deliveries in a row disable it
// and emit EndpointDisabled { endpoint_id }.

// Between your own apps: sender signs, receiver verifies (401 on bad key, signature, age, replay).
state.http.post(url).json(&body).sign("billing", &secret).send().await?;  // after the body
// App::signing_key("billing", "whsec_…"); handler: async fn(SignedRequest(ping): SignedRequest<Ping>)
// WEBHOOK_TOLERANCE_SECS (300) is the timestamp window.
# Ok(()) }
```

Subscriptions ready-made (Laravel's Cashier): the `renox-billing` crate,
`.module(Billing::new().plan(Plan::new("pro", "Pro").price(1_900, "USD", Interval::Month).trial_days(14)).stripe().xendit())`
with `STRIPE_SECRET`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_PRICE_PRO`, `XENDIT_SECRET_KEY`,
`XENDIT_CALLBACK_TOKEN`. It brings a plans page (`/billing`), a card on `/account`, the
gateways' webhooks (`/billing/webhooks/{gateway}`), `Billing::of(&state, &user)` with
`subscribed()`, `on_trial()`, `checkout(plan)`, `swap(plan)`, `cancel()`, `resume()`, `pause(None)`, `unpause()`, and the
guards `.require_subscription()` / `.require_plan(&["pro"])` (`_named("garage")` variants for `.named(..)` subscriptions) (`use renox_billing::SubscriptionRoutes`)
([docs/billing.md](docs/billing.md), [the bike shop's plans](examples/bikeshop/src/app/plans/billing.rs)).

One-off payments (same crate; Stripe, Xendit and `Billing::ngenius()` with `NGENIUS_API_KEY`, `NGENIUS_OUTLET_REF`,
`NGENIUS_WEBHOOK_SECRET`), kept in `billing_payments` with your reference:

```text
// use renox::money::{Currency, Money}; use renox_billing::{Billing, Charge, PaymentLink, Refund, Unsupported};
let usd = Currency::parse("USD").unwrap();
let paid = Billing::of(&state, &user)                       // pending row; paid.url is the gateway's page
    .charge(Charge::new(Money::new(4_500, usd)).reference("job", 42).description("Brake service")).await?;
let link = Billing::payment_link(&state, PaymentLink::new(Money::new(900, usd)) // anyone can pay; owner optional
    .via("xendit").account("acme-llc").expires_at(renox::db::now() + renox::chrono::Duration::days(7))).await?;
let refund = Billing::refund(&state, paid.id, Refund::new(Money::new(1_500, usd)).reason("Part not fitted")).await?; // pending
let state_now = Billing::payment_status(&state, paid.id).await?;   // asks the gateway, applies it like a webhook
let expired = Billing::expire_links(&state).await?;                // = command/hourly task `billing:expire-links`
// Err(e) if Unsupported::of(&e).is_some() → the gateway can't do it (no row left behind)
// keys per installation (called on every use, never cached): Stripe::resolved(|state| async move { Ok(Some(key)) }),
// Ngenius::resolved_accounts(|state, account: &str| ...); webhooks /billing/webhooks/{gateway}[/{account}]
// events: PaymentSucceeded/PaymentFailed (carry reference_type/id), PaymentRefunded, RefundSucceeded, RefundFailed { reason },
// DisputeOpened { evidence_due_by }, DisputeWon, DisputeLost, PaymentLinkExpired; a success after a failure still wins
```
(Compiled examples: [docs/billing.md](docs/billing.md).)

PDF documents from Typst templates (the `renox-pdf` crate, no browser; `rnx make:pdf invoice` writes
`resources/views/pdf/invoice.typ`):

```text
// use renox_pdf::{Pdf, PdfError}; use renox_pdf::testing::{PdfAssertions, text_of};
App::new().module(Pdf::new())            // .font(bytes) adds a font, .lenient() logs missing glyphs,
                                         // .templates(renox::embedded!()) for release builds
let pdf: Vec<u8> = Pdf::document(&state, "invoice")?   // resources/views/pdf/invoice.typ
    .locale("ar")                        // text language, and rtl direction for Arabic and the like
    .data(&order)                        // any Serialize; the template reads the dictionary `data`
    .asset("logo.png", logo_bytes)       // #image("logo.png"); flat names
    .render(&state).await?;              // blocking thread; PDF_MAX_CONCURRENT (2), PDF_TIMEOUT (30 s)
Ok(Download::bytes("INV-001.pdf", "application/pdf", pdf).inline())      // or Mail::attach("INV-001.pdf", "application/pdf", pdf)
// Err(PdfError::MissingGlyphs(chars)) when no font has a character; Compile, Timeout, Setup, ...
// template: #import "@renox/kit:0.1.0": *   → base, colors, money(cents), date("2026-10-01"), page-header(..), data-table(..)
// tests: res.assert_pdf().assert_pdf_contains("INV-001").assert_pdf_pages(1); text_of(&bytes)
//        (Arabic text is extracted as shaped pieces, not whole words)
```
([docs/pdf.md](docs/pdf.md).)

With `CSP=strict`, an inline script needs `<script nonce="{{ csp_nonce() }}">`, and Alpine
expressions must stay simple (move statements into `Alpine.data(...)`).

## SEO and analytics

SEO tags help search engines and link previews show your pages well. Analytics events go to
Google Analytics.

```html
{% block seo %}{{ seo(title=product.name ~ " · " ~ app.name, description=product.summary,
                      image=storage_url(product.photo), type="product") }}{% endblock %}
{# → <title>, description, canonical URL, OpenGraph and Twitter card tags #}
{# canonical= sets the canonical URL (default: this page's path on APP_URL, without the query);
   a relative image is made absolute on APP_URL #}
```

```rust
use renox::analytics::{self, GaClientId, ServerEvent};
use renox::prelude::*;
use renox::seo::Sitemap;

// Routes::new().get("/sitemap.xml", sitemap).name("sitemap")  → robots.txt links it
async fn sitemap(State(state): State<AppState>) -> Result<Sitemap> {
    Sitemap::new(&state).route("home", &[], None)?.route("products.show", &[&7], None)
}

async fn signed_up(session: Session) -> Result<Redirect> {
    // Reaches gtag / the dataLayer with this htmx swap, this page, or the next one.
    analytics::event(&session, "sign_up", json!({ "method": "email" }))?;
    Ok(Redirect::to("/welcome"))
}

async fn paid(State(state): State<AppState>, GaClientId(client): GaClientId) -> Result<StatusCode> {
    // From the server (GA4 Measurement Protocol), so ad blockers can't drop it.
    state.dispatch(ServerEvent::new(client, "purchase").param("value", 49.99).param("currency", "USD")).await?;
    Ok(StatusCode::OK)
}
```

`.env`: `GOOGLE_SITE_VERIFICATION`, `GA4_MEASUREMENT_ID`, `GA4_API_SECRET`, `GTM_CONTAINER_ID`. Tags
are added (with the CSP nonce and sources) only in production; elsewhere pages say `noindex` and
`robots.txt` disallows everything.

## Tests

`TestApp` starts your whole app inside a test, with no real server. Send requests the way a
browser would, then check the answers.

```rust
use renox::prelude::*;
use renox::testing::TestApp;

fn app() -> App {
    App::new().module(Auth::new())
}

#[renox::test]
async fn members_only() {
    // Start the app with a fresh database, then add a user to it.
    let app = TestApp::new(app()).await; // in-memory SQLite (or TEST_DATABASE_URL), migrated
    let user = User::register(app.db(), "Anna", "anna@example.com", "password123").await.unwrap();

    app.get("/login").await.assert_ok().assert_see("Log in");
    app.acting_as(&user);
    app.post("/logout", &[]).await.assert_status(303);
    app.htmx().post("/register", &[("email", "")]).await.assert_invalid("email");
    app.assert_database_has("users", &[("email", &"anna@example.com")]).await;
}
```

```rust
use renox::prelude::*;
use renox::testing::TestApp;

fn app() -> App {
    App::new()
}

#[renox::test]
async fn with_settings() {
    let app = TestApp::with_config(app(), |c| {
        c.vars.insert("MIDTRANS_SERVER_KEY".into(), "test-key".into()); // what state.config.var reads
        c.locale = "es".into();
    })
    .await;
    let state = app.state(); // the AppState handlers get
    let _ = state.config.var("MIDTRANS_SERVER_KEY");
    let res = app.request().header("x-api-version", "2").json().get("/health").await;
    let body: renox::serde_json::Value = res.json(); // also res.text(), res.header("…")
    let _ = body;
}
```

```rust
use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

#[derive(Clone)]
struct OrderPlaced { id: i64 }
impl Event for OrderPlaced {}

#[renox::test]
async fn test_tools() {
    let app = TestApp::new(App::new().module(Auth::new())).await;
    app.fake_events().fake_notifications().fake_broadcasts(); // record instead of running / sending
    let res = app.get("/login").await;
    res.assert_view("renox/auth/login.html");
    app.assert_guest().assert_session_missing("cart");
    app.travel(Duration::from_secs(3600)); // requests and jobs see the clock an hour on
    app.run_all_jobs().await; // retries waiting for their backoff too
    app.assert_not_emitted::<OrderPlaced>().assert_nothing_notified();
    // JSON: res.assert_json_path("data.0.name", "Coffee"), res.assert_json(json!({ "total": 2 }))
    // Events/notifications: app.assert_emitted::<OrderPlaced>(|e| e.id == 7), app.assert_notified(&user, "kind")
    // Broadcasts: app.assert_broadcast("order-updated", |b| b.data["id"] == 7), app.broadcasts()
    // A real server for a browser test: let url = app.serve().await;
}
```

Also available: `put` / `patch` (form) and `delete`, `post_json`,
`post_multipart(uri, &[("title", "x")], &[("photo", "a.png", &bytes)])`, `post_body` (exact
bytes, e.g. signed webhooks), `request().without_csrf()`, `logout()`, `csrf_token()`,
`app.kernel().call("products:import", ["file.csv"])` (an app command), `assert_redirect`,
`assert_hx_redirect`, `assert_header(name, value)`, `assert_unauthorized`, `assert_forbidden`,
`assert_not_found`, `assert_dont_see`, `assert_database_missing` / `assert_database_count`,
`queued_jobs()`, `run_jobs()`, `run_all_jobs()` (retries and delayed jobs too), `sent_mail()` /
`assert_mail_sent`, `session_cookie()` / `use_session_cookie(…)` (play a second device),
`app.run_scheduled("task")` (at the travelled time), `confirm_password()`, `fake_http()`,
`assert_authenticated(Some(&user))` (`None`: as anyone), `assert_session_has("cart")`,
`session_get::<T>("cart")`, `travel_back()`, `assert_notified_to("guest@example.com", "kind")`,
`emitted::<E>()` / `notifications()` (what the fakes recorded), `mailer()`, and
`let (res, queries) = renox::db::capture_queries(app.get("/posts")).await;` to count the SQL a
request (or any future) runs.

## Errors, logs and debugging

Find out what went wrong: each request has an id in its log lines, errors can be sent to a
service you choose, and `/_renox/debug` shows recent requests while you develop.

```rust
use renox::prelude::*;
use renox::RequestId;
use renox::rate_limit::Limit;
use renox::report::ErrorReport;

/// `RequestId` is this request's id, the same one its log lines carry.
async fn show(id: RequestId) -> String {
    format!("request {id}") // also the response's X-Request-Id and every log line's `id`
}

fn app() -> App {
    App::new()
        // 500s, jobs that failed for good and failed scheduled tasks, in the background.
        .report(|report: ErrorReport, state: AppState| async move {
            // report.kind, .message, .details, .source, .request (method, path, id, ip, user_id)
            let _ = state.http.post("https://errors.example.com/api/events").json(&report).send().await;
        })
        // A limit picked per request; routes use it with `.throttle_by("api")`.
        .rate_limiter("api", |req| match req.user {
            Some(user) if user.has_role("partner") => Limit::none(),
            Some(user) => Limit::per_minute(600).by(format!("user:{}", user.id)),
            None => Limit::per_minute(60), // per IP
        })
}
```

- `LOG_FORMAT=json` writes one JSON object per line (with the request's span: `id`, `method`,
  `uri`, `ip`); `LOG_FILE=storage/logs/app.log` appends there instead of stdout.
- A request's `X-Request-Id` is kept when a proxy sends one (8–64 of `A-Z a-z 0-9 . _ -`),
  otherwise it's generated.
- `/_renox/debug` (only with `APP_DEBUG` and `APP_ENV=local`): the last 50 requests with their
  status, time, view and SQL; a statement run 3+ times is flagged as a likely N+1. Mail is at
  `/_renox/mail`, jobs at the queue dashboard (`renox::queue::Dashboard`).
- A `throttle_by` without its `rate_limiter` stops the app at boot.

## Configuration (`.env`)

Settings live in the `.env` file, one `NAME=value` per line. These are the ones Renox reads
(defaults in parentheses):

`APP_ENV` (`local` | `production` | `testing`; also `dev`/`development`, `prod`, `test`; any other
value, such as `staging`, stops the app at boot), `APP_KEY` (`rnx key:generate`; required in
production), `APP_DEBUG` (on by default in `local`), `APP_NAME`, `APP_URL`, `APP_HOST` (an IP
address, `127.0.0.1`; `0.0.0.0` in a container) and `APP_PORT` (3000), `APP_LOCALE`,
`APP_FALLBACK_LOCALE`, `APP_TIMEZONE` (`Asia/Jakarta`, `+07:00` or `UTC`), `APP_CURRENCY` (`USD`; the `money` filter, which takes cents),
`SESSION_LIFETIME` (minutes, 120), `REMEMBER_LIFETIME` (minutes, 43200 = 30 days),
`SESSION_COOKIE` (`renox_session`), `SESSION_DRIVER` (`cookie` | `database`: the `sessions`
table, no 4 KB limit, a new id at each login/logout), `MAIL_FROM_ADDRESS`, `MAIL_FROM_NAME` (defaults to `APP_NAME`),
`VIEWS_PATH` (`resources/views`), `PUBLIC_PATH` (`public`), `LANG_PATH` (`resources/lang`),
`STORAGE_PATH` (`storage`: the maintenance flag and the local disk),
`DATABASE_URL` (`sqlite://storage/app.db` or `postgres://…` with the `postgres` feature),
`DATABASE_POOL_SIZE` (8), `TEST_DATABASE_URL`, `MAIL_MAILER` (`log` | `smtp` | `memory`),
`MAIL_HOST`, `MAIL_PORT`, `MAIL_USERNAME`, `MAIL_PASSWORD`, `MAIL_ENCRYPTION` (`starttls` |
`tls` | `none`; default `starttls`), `QUEUE_WORKERS` (2; 0 = none in `serve`), `SCHEDULER` (`true`),
`CACHE_STORE` (`memory` | `database`: with several servers, also shares rate limits and the login lock),
`STORAGE_DISK` (`local` | `s3`, with `S3_BUCKET`, `S3_REGION`, `S3_ENDPOINT`, `S3_ACCESS_KEY_ID`,
`S3_SECRET_ACCESS_KEY`, and `STORAGE_URL` for the bucket's public URL), `UPLOAD_MAX_SIZE` (MB, 10),
`RUST_LOG` (e.g. `info,sqlx=warn`), `LOG_FORMAT` (`text` | `json`), `LOG_FILE` (append to this file instead of stdout),
`CSP` (`relaxed` | `strict` | `off`), `PERMISSIONS_POLICY` (`default` | `off` | a header value), `TRUSTED_PROXIES` (`127.0.0.1,10.0.0.0/8` or `*`: behind a
proxy, rate limits, the login lock, logs and the `ClientIp` extractor use `X-Forwarded-For`).
Timeouts in seconds: `DATABASE_ACQUIRE_TIMEOUT` (5), `DATABASE_STATEMENT_TIMEOUT` (30,
PostgreSQL; 0 = none), `REQUEST_TIMEOUT` (60; 0 = none), `MAIL_TIMEOUT` (10).

Your own settings are plain `.env` lines, read with `config.var`. In tests, set them with
`TestApp::with_config(app, |c| { c.vars.insert(…); })` (see Tests).

```rust
use renox::prelude::*;

async fn pay(State(state): State<AppState>) -> Result<String> {
    // Read your own setting from `.env`; stop with a clear message when it's missing.
    let key = state
        .config
        .var("MIDTRANS_KEY") // None when missing or empty
        .ok_or_else(|| abort(StatusCode::SERVICE_UNAVAILABLE, "Payments are not set up."))?;
    let live = state.config.env == Environment::Production; // also state.config.name, .url, .debug
    Ok(format!("key of {} characters, live: {live}", key.len()))
}
```
