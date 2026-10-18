# Authorization: gates, policies, roles, tokens and tenants

Your app knows *who* someone is once they log in. This guide is about the next question: what
that person **may** see or change. Renox has one small tool for each kind of question, and this
page shows when to reach for each one and how they fit together.

In this guide:

- [Gates](#gates-may-this-user-do-x): "may this user do X at all?"
- [Policies](#policies-may-this-user-do-x-to-this-row): "may this user do X to *this* row?"
- [Roles and permissions](#roles-and-permissions): jobs like "editor", and what each job allows
- [Roles per branch](#roles-per-branch-a-role-in-one-store-for-a-while): a role in one store,
  for a while (scoped roles with dates)
- [Navigation links](#showing-links-only-to-who-may-use-them): who sees which menu entry
- [Super-admins](#super-admins-gate_before): one person who may do everything
- [API tokens and abilities](#api-tokens-and-abilities): limits for programs that call your API
- [Tenants](#tenants-rows-that-belong-to-a-team): keeping each team's data apart
- [Sensitive actions](#sensitive-actions-and-the-audit-trail), a
  [second login step](#a-second-login-step-two-factor-authentication),
  [passwordless login](#passwordless-login-magic-links-and-one-time-codes) (magic links and
  codes by mail, SMS or WhatsApp) and [testing](#testing-authorization)
- [The `Auth` module's routes](#the-auth-modules-routes): every page and address it adds

Want the short version of every API? See the [cheat-sheet](../CHEATSHEET.md) (the parts "Auth,
policies, gates" and "Tenants, roles and permissions").

[examples/bikeshop](../examples/bikeshop) uses these tools in a whole business:

- roles made of permissions, and code that checks permissions only, never role names
  ([src/app/access/catalogue.rs](../examples/bikeshop/src/app/access/catalogue.rs));
- roles per store with dates (`assign_role_in`, `Scope`, `permissions::set_scope`) and the
  active store kept in the session and the context
  ([src/app/access/active_store.rs](../examples/bikeshop/src/app/access/active_store.rs));
- records checked against their owner, location or operating store (`scopes_with`,
  [src/app/access/policy.rs](../examples/bikeshop/src/app/access/policy.rs));
- a `Policy` for the admin panel's models
  ([src/app/staff/admin.rs](../examples/bikeshop/src/app/staff/admin.rs));
- API tokens with abilities ([src/app/api/mod.rs](../examples/bikeshop/src/app/api/mod.rs));
- an activity log ([src/app/staff/audit.rs](../examples/bikeshop/src/app/staff/audit.rs)).

### Words you'll meet

| Word | What it means |
|---|---|
| **authentication** | Finding out *who* someone is: logging in. |
| **authorization** | Deciding what that person *may do*. This guide is about this one. |
| **guest** | A visitor who isn't logged in. |
| **gate** | A named yes/no question about the user, like "may they see reports?". |
| **policy** | Rules on a model that answer per row, like "may this user edit *this* invoice?". |
| **ability** | The name of an action you ask about, like `"view"`, `"update"` or `"orders:read"`. |
| **role** | A job someone has in your app, like "editor" or "admin". |
| **permission** | One thing a role allows, like `posts.publish`. A role is a bundle of them. |
| **token** | A long secret string a program sends instead of logging in with a password. |
| **tenant** | A customer whose data must stay apart from others: a team, a shop, a school. |
| **401** | The answer "you're not logged in" (an HTTP status code). |
| **403** | The answer "Forbidden": we know who you are, but you may not do this. |
| **404** | The answer "Not Found": as far as you can tell, this row doesn't exist. |

### Which tool answers which question

| Question | Tool | Where it's checked |
|---|---|---|
| Is someone logged in? | `AuthUser`, `.require_auth()` | in a handler's arguments, or on a route |
| May this user do X at all? | a gate (`App::gate`, `gate_async`) | `.require_gate("x")`, `user.gate_async("x").await?`; for sync gates only: `user.gate("x")?`, `can('x')` |
| May this user do X to *this* row? | a policy (`impl Policy`) | `user.authorize("update", &row)?`, `can('update', row)` |
| Which job does the user have? | roles and permissions (the `Permissions` module) | `.require_role`, `.require_permission`, `has_role` |
| Which job does the user have in *this* store? | roles in a scope (`assign_role_in`, `permissions::set_scope`) | the same checks, plus `has_permission_in`, `scopes_with` |
| May this user send this form? | `Validate::authorize` (a form request) | the `Valid<T>` extractor: 403 before the rules run |
| Who may do everything? | `App::gate_before` | asked first by `AuthUser`'s checks (`allows`, `can`, `authorize`, `.require_*`) |
| What may this API token do? | token abilities | `.require_ability("orders:write")`, `token_can` |
| Which rows exist for this user at all? | a default scope (tenants) | every query of the model |
| Is it really them, right now? | `.require_password_confirmed()` | on a route |
| Who did it? | the `Audit` module | `audit::record` |

## Gates: "may this user do X?"

A **gate** is a named yes/no question about the user alone. Use one when the answer doesn't
depend on a particular row. "May this user see the reports page?" is a gate. "May this user edit
invoice 7?" is not (that's a [policy](#policies-may-this-user-do-x-to-this-row)).

There are two kinds of gate:

- a **sync** gate is a plain closure that sees the `User`;
- an **async** gate also gets the app's state, so it can ask the database.

```rust
use renox::prelude::*;

/// Builds the app and defines two gates on it.
fn app() -> App {
    App::new()
        .module(Auth::new())
        // A sync gate: only people with an @example.com address may see reports.
        .gate("reports", |user| user.email.ends_with("@example.com"))
        // An async gate: asks the database whether the user owns billing.
        .gate_async("billing", |user, state| async move {
            let owners: i64 = renox::db::sql("SELECT COUNT(*) FROM billing_owners WHERE user_id = ?")
                .bind(user.id)
                .scalar(&state.db)
                .await?;
            Ok(owners > 0)
        })
}

/// Protects a route with a gate.
fn routes() -> Routes {
    // Guests are sent to log in, other users get 403. Like every guard, it covers the routes
    // added before it.
    Routes::new()
        .get("/reports", || async { "reports" })
        .require_gate("reports")
}

/// Asks the gates inside a handler instead.
async fn invoices(user: AuthUser) -> Result<String> {
    user.gate_async("billing").await?; // 403 unless allowed; `allows_async` for a bool
    // `allows` gives a plain true or false instead of stopping with a 403.
    Ok(format!("reports too: {}", user.allows("reports")))
}
```

What's going on:

- `.gate("reports", …)` gives the question a name, `"reports"`, and the closure answers it.
- `.require_gate("reports")` on the routes says "only users the gate allows may open these".
- In a handler, `user.gate_async("billing").await?` stops with a 403 when the answer is no.
  If you'd rather get a `bool` and decide yourself, use `allows_async`.

In templates, `{% if can('reports') %}` asks a gate (or a permission with that name).

### Sync and async gates in templates

There is a catch with async gates. `can('x')` in templates, and `user.gate("x")` /
`user.allows("x")` in Rust, are **synchronous**: they can't wait for the database. So they only
answer gates made with `App::gate`, and permissions.

For a gate made with `gate_async`, they say **no** (unless `gate_before` answers first).

To ask an async gate, use one of these:

- `.require_gate("x")` on the route;
- `user.gate_async("x")` or `user.allows_async("x")` in the handler. These two answer plain
  gates too.

> [!TIP]
> Want to show or hide something in a page based on an async gate? Ask the gate in the handler
> and pass the answer to the view as a value.

## Policies: "may this user do X to this row?"

A **policy** lives on the model. It answers per row, usually by checking who owns the row:

```rust
use renox::prelude::*;

/// An invoice: a row of the `invoices` table.
#[derive(Model, serde::Serialize, Default)]
#[model(table = "invoices")]
struct Invoice { id: i64, user_id: i64, paid: bool }

/// The invoice's policy: who may do what to one invoice.
impl Policy for Invoice {
    /// Answers "may `user` do `ability` to this invoice?".
    fn allows(&self, user: &User, ability: &str) -> bool {
        match ability {
            "view" => self.user_id == user.id,
            "update" => self.user_id == user.id && !self.paid,
            _ => false,
        }
    }
}

/// The edit page of one invoice.
async fn edit(State(db): State<Db>, user: AuthUser, Path(id): Path<i64>) -> Result<View> {
    let invoice = Invoice::find_or_404(&db, id).await?;
    user.authorize("update", &invoice)?; // 403 unless allowed
    Ok(view("invoices/edit.html", context! { invoice }))
}

/// The list of the user's invoices.
async fn index(State(db): State<Db>, user: AuthUser) -> Result<View> {
    // For `{% if can('update', invoice) %}` in the view, wrap each row with its answers.
    let invoices: Vec<_> = Invoice::query().where_(Invoice::USER_ID.eq(user.id))
        .get(&db)
        .await?
        .into_iter()
        .map(|invoice| Can::new(invoice, Some(&user), &["update"]))
        .collect();
    Ok(view("invoices/index.html", context! { invoices }))
}
```

What's going on:

- `allows` gets the user and an **ability** (a name like `"view"` or `"update"`) and returns
  `true` or `false`. Here, you may view your own invoices, and update them only while unpaid.
  Any other ability is a no.
- `user.authorize("update", &invoice)?` asks the policy. When the answer is no, the `?` stops
  the handler and the visitor gets a 403.
- Templates can't run the policy themselves. So in `index`, `Can::new` wraps each invoice with
  the answers for the abilities you list (`&["update"]`). Then `{% if can('update', invoice) %}`
  in the view reads them.

### Policies that check roles

A policy can check roles too. `has_role` on the `User` it gets reads the roles the request
already loaded (with the [`Permissions` module](#roles-and-permissions)). So a rule like
"accountants see every invoice" fits in `allows`:

```rust
use renox::prelude::*;

/// An invoice, with only the fields this example needs.
#[derive(Model, serde::Serialize, Default)]
#[model(table = "invoices")]
struct Invoice { id: i64, user_id: i64 }

impl Policy for Invoice {
    /// The owner may view the invoice, and so may every accountant.
    fn allows(&self, user: &User, ability: &str) -> bool {
        ability == "view" && (self.user_id == user.id || user.has_role("accountant"))
    }
}

/// Shows one invoice to the people allowed to see it.
async fn show(State(db): State<Db>, user: AuthUser, Path(id): Path<i64>) -> Result<View> {
    let invoice = Invoice::find_or_404(&db, id).await?;
    user.authorize("view", &invoice)?; // the owner or an accountant
    Ok(view("invoices/show.html", context! { invoice }))
}
```

> [!WARNING]
> Outside a request (in a job or a command) the roles aren't loaded, so `has_role` says
> `false`. Ask `user.roles(&db)` there instead.

### Forms can ask too

A form can check the user before its rules run. `Validate::authorize` gets a `FormContext` (the
app's state and the user). If it returns `false`, the answer is a 403. The CHEATSHEET's "Form +
validation" part shows such a form (Laravel calls it a "form request").

## Roles and permissions

A **role** is a job someone has, like "editor". A **permission** is one thing a role allows, like
`posts.publish`.

The `Permissions` module stores all of this. It's opt-in: you add it when you want it. It keeps:

- the roles;
- the permissions each role grants;
- who has which role.

These live in the tables `roles`, `permissions`, `permission_role` and `role_user`. The module
loads the user's roles and permissions once per request, so the checks below cost no extra
database query.

```rust
use renox::prelude::*;
use renox::auth::{Permissions, permissions};

/// Turns the `Permissions` module on, next to `Auth`.
fn app() -> App {
    App::new().module(Auth::new()).module(Permissions)
}

/// One route for editors, one for anyone allowed to publish.
fn routes() -> Routes {
    let drafts = Routes::new().get("/drafts", || async { "drafts" }).require_role("editor");
    let publish = Routes::new()
        .post("/posts/{id}/publish", || async { "published" })
        .require_permission("posts.publish");
    drafts.merge(publish)
}

// In a seeder, a migration's data step or an admin page:
async fn setup(db: &Db, user: &User) -> Result {
    // Creates the role if it's new and sets exactly these permissions.
    permissions::define_role(db, "editor", &["posts.create", "posts.publish"]).await?;
    user.assign_role(db, "editor").await?; // also remove_role, sync_roles
    Ok(())
}

/// Checks a role and a permission inside a handler.
async fn check(user: AuthUser) -> String {
    // `allows` asks gate_before, then a gate of that name, then the permissions.
    format!("{} {}", user.has_role("editor"), user.allows("posts.publish"))
}
```

What's going on:

- `.require_role("editor")` lets only editors open `/drafts`.
- `.require_permission("posts.publish")` lets in anyone whose roles grant that permission.
- `define_role` makes the "editor" role with two permissions. `assign_role` gives it to a user
  (`remove_role` takes it away, `sync_roles` sets the exact list).
- `has_role` and `allows` answer in a handler without a database query.

> [!IMPORTANT]
> Define a role before you hand it out. `assign_role` and `sync_roles` fail with an error for a
> role that doesn't exist yet ("there is no role `editor`"); they don't create it.

> [!TIP]
> Check **permissions** in your code (`require_permission("posts.publish")`) and use roles as
> bundles of them. Then a new role needs no code change: you just give it permissions.
> A role with no permissions (`&[]`) is fine when the app only checks the role itself.
> The bike shop goes the other way: its code never names a role, and a test fails if it does
> ([tests/access.rs](../examples/bikeshop/tests/access.rs)).

More you can do:

- In templates: `{% if 'editor' in auth.roles %}` and `{% if can('posts.publish') %}`.
- `permissions::users_with_role(&db, "editor")` lists the users with a role, for example to
  notify every editor.
- `permissions::grant(&db, "editor", &["posts.delete"])` adds permissions to a role that
  exists, and `permissions::revoke(&db, "editor", &["posts.delete"])` takes them away. Unlike
  `define_role`, they leave the role's other permissions alone.
- `permissions::delete_role(&db, "editor")` deletes a role (its users lose it) and says whether
  it existed.
- `permissions::roles(&db)` lists every role with its permissions, for an admin page.
- `user.role_names()` on an `AuthUser` gives the roles loaded for this request.

## Roles per branch: a role in one store, for a while

In a business with several stores (branches, teams, projects), a person is often a **manager in
one store and a clerk in another**, or covers a store for two weeks. That's still role-based
access (RBAC), with two attributes on each assignment (ABAC): **where** it counts and **when**.

A role means the same everywhere: "manager" grants the same permissions in every store. What
changes is who has it where. So only the assignment gets a scope and dates:

```rust
use renox::prelude::*;
use renox::auth::permissions::{self, Scope};
use renox::axum::{extract::Request, middleware::{Next, from_fn}};

/// A store (branch) of the business.
#[derive(Model, serde::Serialize, Default)]
#[model(table = "stores")]
struct Store { id: i64, name: String }

/// Ana manages the north store, and covers the south one for two weeks.
async fn setup(db: &Db, ana: &User, north: &Store, south: &Store) -> Result {
    permissions::define_role(db, "manager", &["orders.refund", "stock.adjust"]).await?;
    ana.assign_role_in(db, "manager", &Scope::of(north)).await?;
    let start = renox::db::now();
    ana.assign_role_in(db, "manager", &Scope::of(south))
        .from(start)
        .until(start + renox::chrono::Duration::days(14))
        .await?;
    Ok(())
}

/// Picks the store this request works in, after checking the user may work there.
async fn pick_store(user: Option<AuthUser>, session: Session, req: Request, next: Next) -> Response {
    if let (Some(user), Some(store)) = (&user, session.get::<i64>("store_id")) {
        // The user's own stores: any role there, or a global one.
        let scope = Scope::of_id::<Store>(store);
        if user.has_role_in("manager", &scope) || user.has_role_in("clerk", &scope) {
            permissions::set_scope(scope);
        }
    }
    next.run(req).await
}

fn app() -> App {
    App::new()
        .module(Auth::new())
        .module(permissions::Permissions)
        .layer(from_fn(pick_store))
}

/// Refunds need `orders.refund` in the store the request works in.
fn routes() -> Routes {
    Routes::new()
        .post("/orders/{id}/refund", || async { "refunded" })
        .require_permission("orders.refund")
}
```

What's going on:

- `Scope::of(&store)` names one record: its model's table and its key (`stores`, `7`). Use
  `Scope::of_id::<Store>(7)` when you only have the id.
- `assign_role_in` gives a role there. `.from(date)` and `.until(date)` are optional: before
  `from` and from `until` on, the assignment simply doesn't count. Giving the same role in the
  same store again replaces its dates.
- `permissions::set_scope(scope)` tells this request which store it works in, like the current
  team in [Tenants](#tenants-rows-that-belong-to-a-team). From then on `require_role`,
  `require_permission`, `has_role`, `has_permission`, `allows` and `can()` in templates count the
  user's **global** roles plus the roles **in that store** that are within their dates.
- With no scope set, only global roles count, exactly as before. An app that never gives a role
  in a scope sees no change.

> [!IMPORTANT]
> Set the scope before the guards run: in an `App::layer`, as above, or in a route layer added
> after the `.require_permission(…)` it should cover (layers added later run first). The roles
> are loaded once per request (all of them, with their stores and dates) and filtered at each
> check, so a scope set late in the request still counts. `rnx route:list` marks role and
> permission guards with `*` to say they count the active scope.

### Any scope: `has_permission_anywhere`

`user.has_permission_anywhere("orders.refund")` is true when the user holds the permission
globally or in at least one scope (a store, a team) that is active today. It answers "does this
person do this anywhere?", which is what a menu needs; for a record use `has_permission_in`.

### Checking one record: `has_permission_in`

A policy decides about one row, which belongs to its own store, whatever store the request
works in. Ask about that store:

```rust
use renox::prelude::*;
use renox::auth::permissions::Scope;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "stores")]
struct Store { id: i64 }

#[derive(Model, serde::Serialize, Default)]
#[model(table = "orders")]
struct Order { id: i64, store_id: i64 }

impl Policy for Order {
    fn allows(&self, user: &User, ability: &str) -> bool {
        let store = Scope::of_id::<Store>(self.store_id);
        match ability {
            "refund" => user.has_permission_in("orders.refund", &store),
            _ => user.has_permission_in("orders.view", &store),
        }
    }
}
```

`has_permission_in` (and `has_role_in`) count global roles plus the roles given in that store.
Like `has_permission`, they answer from the roles loaded for the current request.

### Lists: `scopes_with`

"Show the orders of every store where I may see orders" needs the list of stores.
`user.scopes_with::<Store>("orders.view")` answers `Scopes::All` when a global role grants the
permission, else `Scopes::Only(ids)`. A default scope has no user at hand, so
`permissions::scopes_with::<Store>(…)` asks for the logged-in user of the request, and
`Scopes::apply` turns the answer into a filter on one or more columns:

```rust
use renox::prelude::*;
use renox::auth::permissions;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "stores")]
struct Store { id: i64 }

/// A transfer of stock is seen by the store it leaves and the store it goes to.
#[derive(Model, serde::Serialize, Default)]
#[model(table = "transfers", default_scope = "my_stores")]
struct Transfer { id: i64, from_store_id: i64, to_store_id: i64 }

/// `from_store_id IN (my stores) OR to_store_id IN (my stores)`; everything for a
/// global role; nothing without a user (fails closed).
fn my_stores(query: renox::db::Query<Transfer>) -> renox::db::Query<Transfer> {
    permissions::scopes_with::<Store>("transfers.view")
        .apply(query, &["from_store_id", "to_store_id"])
}
```

`apply` is a shortcut for `query.where_any(|q| q.where_in("from_store_id", ids.clone())
.where_in("to_store_id", ids))`, which you can write yourself for other shapes.

### Managing assignments

- `user.assign_role_in(&db, role, &scope)` (with `.from` / `.until`), `user.remove_role_in(&db,
  role, &scope)`, and `user.sync_roles_in(&db, &[roles], &scope)` (exactly these roles in that
  store; other stores stay).
- `assign_role`, `remove_role` and `sync_roles` are about **global** roles and leave roles in a
  store alone. `Scope::global()` with `assign_role_in` gives a global role with dates
  ("admin until Friday").
- `user.assignments(&db)` lists every role the user has, with its `scope`, `starts_at` and
  `ends_at`, for an account or admin page.
- `permissions::users_with_role_in(&db, "manager", &scope)` lists who has the role in a store now
  (given there, or globally), for example to notify its managers. `users_with_role` lists the
  global ones.
- `permissions::users_with_permission_in(&db, "rentals.return", &scope)` asks the same about a
  permission: who may do it in that store now, through whichever role. A job uses it to notify
  "everyone who may take overdue bikes back here" without naming roles.
  `users_with_permission` lists who may do it through a global role.
- Ended assignments stop counting by themselves. `rnx permissions:prune --days 30` deletes the
  ones that ended more than 30 days ago; schedule it if the table grows.

> [!NOTE]
> Permissions stay global: there's no "may refund in store 1 only" without a role. Make a role
> for it and give that role in store 1.

## Showing links only to who may use them

A navigation link added with `NavLink` (see
[docs/ui.md](ui.md#menus-that-modules-and-the-app-add-to)) can carry its own visibility. The
checks run in this order, and the link is shown only if all of them pass:

1. `App::nav_hide(menu, key)` removes it for everyone.
2. `.permission("invoices.view")`: the user holds the permission **globally or in at least one
   scope** (`user.has_permission_anywhere("invoices.view")`). A manager of one store sees the
   "Refunds" link even though the permission is not global; the page it opens still checks
   the scope with `has_permission_in`. Without the Permissions module there are no roles, so
   the name falls back to the gates and `allows`; an unknown name denies.
3. `.gate("view-reports")`: the gate allows this user.
4. `.when(|user| user.has_role("auditor"))`: your own check.

An anonymous visitor sees only the links with no restriction. Hiding a link is not
protection: guard the route itself with `require_permission` or a gate.

## Super-admins: `gate_before`

Some apps have a person who may do everything: a super-admin. `App::gate_before` is the place for
that. The checks on an `AuthUser` (the logged-in user of a request) ask it **before** the gate,
permission or policy itself, and it returns one of three answers:

- `Some(true)`: allow, without asking the check itself;
- `Some(false)`: deny, without asking the check itself;
- `None`: no opinion, go on to the check itself.

Those checks are `allows`, `gate`, `allows_async`, `gate_async`, `can` and `authorize` on
`AuthUser`, the route guards `.require_gate` and `.require_permission`, `can('gate')` in
templates, and `Can::new` given an `AuthUser` (which `can('update', row)` then reads).

It doesn't answer `require_role` / `has_role`: those mean "has exactly this role". You can base
it on a role (from the `Permissions` module) or on the user's row, for example a column:

```rust
use renox::prelude::*;

/// Lets super-admins through every gate, permission and policy.
fn app() -> App {
    App::new()
        .module(Auth::new())
        // `then_some(true)` gives Some(true) for a super-admin, and None for everyone else.
        .gate_before(|user, _ability| user.has_role("super-admin").then_some(true))
        // or: (user.get::<bool>("super_admin") == Some(true)).then_some(true)
}
```

`User::has_role` and `User::has_permission` answer from the roles loaded for the current
request. So they work in `gate_before`, and in `Policy::allows` ("admins may edit any post").

> [!WARNING]
> A plain `User` skips `gate_before`. `User::can` and `User::authorize` (in a job or a command,
> say) and `Can::new` given a `&User` ask only the policy. If a super-admin must pass there too,
> check it in the policy itself.

> [!WARNING]
> For another user, or outside a request (a job, a command), `has_role` and `has_permission`
> say `false`. Use the async `user.roles(&db)` there.

## API tokens and abilities

Programs that call your API don't log in with a password. They send a **token** in a header:
`Authorization: Bearer …`. A token can be limited to some **abilities** (like `orders:read`)
and given an expiry date.

`.require_ability(x)` checks requests made with a token. A guest (no session, no valid token)
is sent to log in, or gets a 401 from an API. A token without the ability gets a 403.

> [!IMPORTANT]
> `.require_ability` doesn't make a route token-only. Users logged in with a session pass it,
> and so do tokens made without abilities and tokens with the `"*"` ability. For a route only
> programs may call, check `user.token_id()` in the handler: it is `None` for a session login.

```rust
use renox::prelude::*;

/// Makes a token that may only read orders, and stops working in 30 days.
async fn issue(State(db): State<Db>, user: AuthUser) -> Result<String> {
    let in_30_days = renox::db::now() + renox::chrono::Duration::days(30);
    let token = user
        .create_token_with(&db, "reporting", &["orders:read"], Some(in_30_days))
        .await?;
    Ok(token.plain) // shown once; only its hash is stored
}

/// Reading orders needs `orders:read`; creating one needs `orders:write`.
fn api() -> Routes {
    let read = Routes::new().get("/api/orders", || async { "[]" }).require_ability("orders:read");
    let write = Routes::new().post("/api/orders", || async { "{}" }).require_ability("orders:write");
    read.merge(write).require_auth() // 401 without a valid token, then 403 without the ability
}
```

What's going on:

- `create_token_with` makes a token named "reporting" with one ability and an expiry date.
- `token.plain` is the token itself. You can show it only once: the database keeps just its
  **hash** (a scrambled fingerprint), so nobody can read it back later.
- A request without a valid token gets a 401. A valid token without the ability gets a 403.

More to know:

- `user.token_can("orders:read")` answers the same question inside a handler.
- The ability `"*"` allows everything: `create_token_with(&db, "admin", &["*"], None)`.
- A password reset deletes all of the user's API tokens, since whoever reset it may be taking
  back a stolen account. Programs then need new tokens.
- `rnx tokens:prune` deletes tokens that expired more than a day ago. Schedule it daily
  (`renox::auth::prune_expired_tokens` in a `daily_at` task); it prunes device tokens too.

### Tokens for a device, not a user

A store's kiosk, a till or a sensor is not a person, and making a placeholder user for it
(with a password nobody knows) clutters `users` and every list of them. A **device token** is
owned by a key you choose, like `"kiosk:3"`, and authenticates a request as a `Device`, with
no user at all:

```rust
use renox::auth::{Device, DeviceToken};
use renox::prelude::*;

async fn make(State(db): State<Db>) -> Result<String> {
    // Shown once, like a user's token; it starts with `d` (`d12|…`).
    let made = DeviceToken::create(&db, "kiosk:3", "front desk", Some(&["sales:create"]), None).await?;
    Ok(made.plain)
}

async fn sale(device: Device) -> String {
    format!("sale at {}", device.key()) // device.id_of("kiosk") == Some("3")
}

struct Kiosks;

impl Module for Kiosks {
    fn name(&self) -> &'static str { "kiosks" }
    fn routes(&self) -> Routes {
        Routes::new()
            .post("/kiosk/sales", sale)
            .require_device_ability("sales:create") // 401 without a device token, 403 without the ability
    }
}
```

- `Routes::require_device()` and `require_device_ability(…)` guard routes; both answer JSON.
  `Device` is an extractor (`Option<Device>` too); `device.can("sales:create")` answers inside
  a handler. A device request has no user: `AuthUser` answers 401 on it, and `Routes::require_auth`
  does not let it in. Like any bearer token, it needs no CSRF token.
- `DeviceToken::for_device`, `revoke(db, device, id)` and `revoke_all(db, device)` list and
  revoke, per device key: retire a kiosk and all its tokens go. `prune_expired` is the pruning.
- Tokens live in `device_tokens` (a migration of the `Auth` module). Named rate limiters see
  the device (`LimitRequest::device`), and the default limit key is `device:<key>`.

## Tenants: rows that belong to a team

In a **multi-tenant** app, many customers share one app and one database. Most rows belong to a
team (or a shop, a school…). One forgotten `where team_id = ?` would show one customer another
customer's data.

A **default scope** fixes that. It makes the filter part of the model, so handlers can't forget
it:

```rust
use renox::prelude::*;
use renox::axum::{extract::Request, middleware::{Next, from_fn}};

/// The team the current request works for.
#[derive(Clone)]
struct CurrentTeam(i64);

/// A project belongs to one team. Every query goes through `team_only`.
#[derive(Model, serde::Serialize, Default)]
#[model(table = "projects", default_scope = "team_only")]
struct Project { id: i64, team_id: i64, name: String }

/// The default scope: adds "only this team's rows" to every query of `Project`.
fn team_only(query: renox::db::Query<Project>) -> renox::db::Query<Project> {
    match renox::context::get::<CurrentTeam>() {
        Some(team) => query.where_(Project::TEAM_ID.eq(team.0)),
        None => query.none(), // no team, no rows: fail closed
    }
}

// Runs for every request: puts the user's current team in this request's context.
async fn pick_team(user: Option<AuthUser>, req: Request, next: Next) -> Response {
    if let Some(team) = user.as_ref().and_then(|u| u.get::<i64>("current_team_id")) {
        renox::context::set(CurrentTeam(team));
    }
    next.run(req).await
}

/// Adds `pick_team` as a layer, so it runs before every handler.
fn app() -> App {
    App::new().module(Auth::new()).layer(from_fn(pick_team))
}

/// Shows one project of the current team.
async fn show(State(db): State<Db>, Path(id): Path<i64>) -> Result<View> {
    // Another team's project id is a 404: the scope is part of the query.
    let project = Project::find_or_404(&db, id).await?;
    Ok(view("projects/show.html", context! { project }))
}

/// Counts the projects of every team, skipping the scope.
async fn count_all(db: &Db) -> Result<u64> {
    Project::unscoped().count(db).await // admin code and commands that must see every team
}
```

What's going on:

- `default_scope = "team_only"` tells the model to pass every query through `team_only`.
- `renox::context` is a small box of values that belongs to one request. The middleware
  `pick_team` puts the user's team in it; `team_only` reads it back.
- With no team in the box, `query.none()` returns no rows at all. This is called **failing
  closed**: when in doubt, show nothing.
- In `show`, a project id from another team simply isn't found, so the visitor gets a 404.

Things to know:

- `query()`, `find`, `all`, `where_eq` and the relation loaders apply the scope. `unscoped()`
  skips it. Saving and deleting a model you already loaded work by its id.
- Check the team the user asks for before you put it in the context: is the user really a member?
  The bike shop keeps the active store in the session and checks the person's roles there in
  its middleware ([src/app/access/active_store.rs](../examples/bikeshop/src/app/access/active_store.rs)).
- Uniqueness is per team too: `.unique("projects", "name").ignore(id).where_eq("team_id", team)`.

> [!WARNING]
> Every request, job, scheduled task and app command starts with an **empty** context. A job for
> a team carries the team id in its payload and calls `renox::context::set(CurrentTeam(id))`
> first. A task started with `tokio::spawn` starts without one too: wrap it in
> `renox::context::scope`.

> [!WARNING]
> Bulk `Query::update` / `delete` start from `query()`, so they are scoped as well. Raw
> `renox::db::sql(…)` is **not** scoped: there you must add the team filter yourself.

## Sensitive actions and the audit trail

- `.require_password_confirmed()` asks for the password again before routes such as billing, API
  keys or deleting a team. It asks at most every three hours.
- The opt-in `Audit` module keeps a record of who did what (an **audit trail**). It records
  logins, failed logins and account changes by itself.
- Record your app's own actions with `audit::record(&db, Entry::new("order.refunded")
  .user(id).subject("orders", order_id).ip(ip))`.
- Read them back with `for_subject`, `for_user` or `latest`.
- `rnx audit:prune --days 365` trims old rows from the table.

## A second login step (two-factor authentication)

Some apps ask for one more thing after the password, such as a code from an authenticator app on
the user's phone. This is called **two-factor authentication** (2FA). A module can add such a
step.

> [!TIP]
> For codes from an authenticator app, use the `renox-2fa` crate: one module adds the account
> card, the QR code, the challenge and recovery codes, built on this extension point. See
> [two-factor.md](two-factor.md). Read on to write a second step of your own (a PIN, a code
> by mail…). Social logins (`renox-oauth`, [oauth.md](oauth.md)) go through the same step.

In `Module::register`, the module says two things: which users must pass the step, and the route
where the challenge (the "enter your code" page) lives:

```rust
use renox::prelude::*;

/// A module that asks some users for a PIN after their password.
struct Pin;

impl Module for Pin {
    fn name(&self) -> &'static str {
        "pin"
    }

    /// Registers the second login step with the app.
    fn register(&self, app: &mut Registry) {
        // After the right password, these users go to the `pin.challenge` route
        // instead of being logged in.
        app.second_factor("pin.challenge", |user, _state| async move {
            Ok(user.extra.contains_key("pin"))
        });
    }
}
```

How the challenge works, step by step:

- **The login waits** in the session for ten minutes. `renox::auth::pending_login(&session)`
  gives it to the challenge's handler (`user_id`, `remember`), or `None` once it has expired.
- **A right code:** the handler calls `renox::auth::complete_login(&state, &session, &pending,
  ip)`. That logs the user in just as the login page would: a new session id, "remember me", the
  password counted as confirmed, and the `LoggedIn` event. It returns where to go next, or
  `None` if the user changed their password in the meantime.
- **A wrong code:** `pending.failed(&state, ip)` counts it towards the login throttle (the limit
  on login attempts) and fires `LoginFailed`. `pending.locked_out(&state, ip)` says how long to
  wait. The throttle is only cleared once the step is passed, so knowing the password alone
  doesn't reset the count.

> [!IMPORTANT]
> API tokens and registration don't go through the second step. Nor does your own code that
> calls `renox::auth::login` directly. A password reset doesn't log anyone in: it sends the user
> to the login page, where the step applies as usual.

Only one module may set a second step. Two of them, or a challenge route that doesn't exist,
are errors when the app starts.

The module documentation of `renox::auth::second_factor` shows a whole challenge handler.

### A section on the account page

A module like this usually lets users turn its step on and off from the account page,
`/account` (`Auth::new().account()`).

To add a card there, call `app.account_section(template, order, |user, state| async { … })`
in `Module::register`:

- **`template`** is the card's template. Add it with `app.templates`, or as a file in the app's
  views. It's rendered with the page's context (`user`, `text`), and reads what the closure
  returned as `section.data`.
- **`order`** places the card. Sections show in `order`, after the built-in cards and before
  "Delete account".
- **The closure** loads the data the card shows.

If you replace `renox/auth/account.html` with a page of your own, keep the sections with
`{% include "renox/auth/account_sections.html" %}`.

### Logging in another way

A module that proves who someone is without their password (a social login, a link sent by
mail) logs them in with the same rules as the login page:

- **`renox::auth::sign_in(&state, &session, &user, remember, ip)`** logs `user` in and returns
  where to send the browser: the page that asked for a login, else `Auth::redirect_to`, else
  `home`. When the second step applies to the user, the login waits for it instead, and the
  address is the challenge's.
- **`renox::auth::register_verified(&state, name, email, &[("provider", "google")])`** makes an
  account for an address another service verified: no password, the address verified, then
  `Auth::on_registered` (the extra pairs are in its `Registration`) and `Registered`.
  `renox::auth::registration_open(&state)` says whether sign-ups are allowed at all.
  Addresses are stored trimmed and in lowercase; compare one you got elsewhere through
  `renox::auth::normalize_email(email)`, as Renox does at registration and login.
- **`renox::auth::confirm_identity(&session)`** counts as typing the password at
  `/confirm-password`, and returns the page that asked for it.

Such a user has no password: `password` is empty, which no typed password matches, and
`user.has_password()` is `false`. On `/account` they choose one without a "current password",
and the actions that ask for a password ask for a recent confirmation instead.
The login, register and confirm-password pages include `renox/auth/login_options.html` when a
module provides it (with `page` set to `login`, `register` or `confirm`), for its buttons.
The `renox-oauth` crate is built on these ([oauth.md](oauth.md)).

## Passwordless login: magic links and one-time codes

Some users rarely log in, or have no email address (customers in a portal, field workers with
only a phone number). They can log in with a **link** sent by email or a short **code** sent
through any notification channel, expiring and single-use, with the second login step, the
login throttle and the audit log working as for passwords. Both are options of `Auth`:

```rust
use std::time::Duration;
use renox::auth::LoginCodes;
use renox::prelude::*;

# async fn demo() {
let app = App::new()
    // An SMS channel (see notifications): a provider adapter is the app's or a plugin's job.
    .channel("sms", |to, message, _state| async move {
        let _ = (to.address("sms"), message["text"].as_str()); // call the provider here
        Ok(())
    })
    .module(
        Auth::new()
            .magic_link() // /login/link: email in, link out (15 minutes, once)
            .login_codes(
                LoginCodes::new()
                    .channel("sms") // any `App::channel`; "mail" works too
                    .digits(6)
                    .ttl(Duration::from_secs(300))
                    .max_attempts(5)
                    // Who a phone number (or any identifier) belongs to. `phone` is a column
                    // the app added to `users` with its own migration.
                    .identify(|state, input: String| async move {
                        Ok(renox::db::sql("SELECT * FROM users WHERE phone = ?")
                            .bind(input.trim())
                            .fetch_optional_as::<User>(&state.db)
                            .await?)
                    }),
            ),
    );
# let _ = app;
# }
```

The login page lists the options that are on. Delivery goes through the **queue**
(`queue:work`, which `serve` runs by default), so the answer doesn't wait for the provider.

**Magic links.** `/login/link` (`login.link.request`) asks for an email address and mails a link
to `/login/link/{token}`. The link opens a "Log in as …?" page; only its button, a CSRF-protected
`POST` (`login.link.consume`), logs in and burns the token, because mail scanners open every link.
A token works once, for 15 minutes (`.magic_link_ttl(..)`), and a newer link replaces an older
one. It works in any browser by default, since people read mail on another device;
`.magic_link().same_browser()` makes it work only in the browser that asked for it (a try from
another browser is refused and doesn't burn it).

**Login codes.** `/login/code` (`login.code.request`, its form `login.code.send`) asks for an
identifier, and `/login/code/verify` (`login.code.verify`) takes the code. A code is valid only in
the browser session that asked for it, so a forwarded SMS alone isn't enough. After `max_attempts`
wrong codes the code is **burned**: a new one must be requested, and the right code is refused
until then. Locking out an account stays with the login throttle (5 wrong tries per identifier and
IP a minute, as for passwords; they fire `LoginFailed` and `LockedOut`). Without `identify`, the
identifier is an email address.

- **Several channels:** `.channels(["sms", "whatsapp", "mail"])` adds a choice to the request form
  and "send it another way" to the verify page (`login.code.resend`).
- **Your own delivery:** `.deliver(|state, delivery| async move { .. })` replaces the built-in
  message on every channel, for apps with a messaging layer of their own (templates, language,
  delivery logs, providers that require pre-approved authentication templates). `delivery` has
  the `user`, `channel`, `address` (what was typed), `code`, `expires_in`, `locale` (the language
  of the request) and the built-in `text`. Without it, mail gets `renox/mail/auth/login-code`
  and a custom channel gets `{"kind": "login_code", "code", "text", "expires_in"}`.
- **`register_on_first_use()`** makes an account (no password, the address verified, then
  `on_registered`) for an unknown email address that proves it owns it with a link or a code sent
  by mail, through `register_verified`; it needs open registration.
- **`LoggedIn::via`** says how: `"password"`, `"magic_link"`, `"code"` or `"external"` (a module
  that called `sign_in`). The audit log's `auth.login` entries carry it in `data.via`. Logging in
  by link or code sent by mail also marks the address verified.

**What an attacker sees.** Asking for a link or code answers the same whether the account exists
or not, and does the same work: a code is stored for unknown identifiers too (only the queued
delivery is skipped), and a wrong code is as wrong for a stranger as for a user. Sending is
limited per identifier (5 in 15 minutes; over it nothing is sent, and the page still answers as
always) and per IP address (20, then `429`). Only keyed hashes (HMAC with `APP_KEY`) are stored in
`one_time_codes`, the delivery job's payload is stored encrypted, and `rnx tokens:prune` deletes
expired rows. Someone who can type your user's phone number can cost them a pending code (a new
request replaces it) but can't log in.

**`OneTimeCode`** is the same primitive for anything else you confirm with a code:

```rust
use std::time::Duration;
use renox::auth::{CodeCheck, OneTimeCode};
use renox::prelude::*;

async fn start(state: &AppState, user: &User) -> Result<String> {
    // Six digits, 5 minutes, 5 tries; the code is returned once, only its hash is stored.
    OneTimeCode::issue(state, "payout", &user.id.to_string(), Duration::from_secs(300), 5).await
}

async fn confirm(state: &AppState, user: &User, typed: &str) -> Result<bool> {
    Ok(match OneTimeCode::verify(state, "payout", &user.id.to_string(), typed).await? {
        CodeCheck::Valid => true, // used up
        CodeCheck::Wrong { tries_left: _ } => false,
        CodeCheck::Missing => false, // expired, used, or burned by wrong tries
    })
}
```

Each check first uses up a try in one statement, so parallel guesses can't exceed the limit, and
the right code is deleted in the step that accepts it, so two requests can't both use it. It
needs the `Auth` module (it owns the `one_time_codes` table).

**Testing.** `TestApp::last_login_link()` and `last_login_code()` (or `last_login_code_on("sms")`)
run the queued jobs and return what Renox was asked to deliver (kept in memory in debug mode, which
`TestApp` is). Travel in time with `app.travel(..)`:

```rust
# use renox::prelude::*;
# use renox::testing::TestApp;
# async fn demo(app: TestApp) {
app.post("/login/link", &[("email", "ana@example.com")]).await;
let link = app.last_login_link().await.unwrap();
let path = &link[link.find("/login/").unwrap()..];
app.get(path).await.assert_ok(); // opening the link logs nobody in
app.post(path, &[]).await.assert_redirect("/"); // the button does
# }
```

Replace the pages with files of the same name: `renox/auth/login-link.html`,
`login-link-confirm.html`, `login-code.html` and `login-code-verify.html`, and the mails
`renox/mail/auth/login-link` and `login-code` (`.html` and `.txt`). Texts are the `renox.auth.*`
keys (`login_link_*`, `login_code_*`, `mail_link_*`, `mail_code_*`, `channel_<name>`).

## The `Auth` module's routes

`.module(Auth::new())` adds these pages and addresses. Each has a **name**, so you can link to
it with `route('login')` in a template or `state.url("login", &[])` in Rust, whatever its
address. Where one name covers two methods, the `GET` shows a page and the `POST` sends its form.

| Method | Address | Name | What it does |
|---|---|---|---|
| GET, POST | `/login` | `login` | The login page, and logging in. Guests only. |
| GET, POST | `/register` | `register` | The sign-up page, and creating the account. Guests only; gone with `.without_registration()`. |
| POST | `/logout` | `logout` | Logs out this device, then goes to the `home` route (or `/`). |
| GET | `/forgot-password` | `password.request` | The "forgot your password?" page. Guests only. |
| POST | `/forgot-password` | `password.email` | Mails a reset link. Guests only. |
| GET | `/reset-password/{token}` | `password.reset` | The page the reset link opens. Guests only. |
| POST | `/reset-password` | `password.update` | Sets the new password, then goes to the login page. Guests only. |
| GET, POST | `/confirm-password` | `password.confirm` | Asks for the password again, for `.require_password_confirmed()`. Logged-in users only. |
| GET | `/verify-email` | `verification.notice` | "Check your email" for a user who hasn't verified yet. Logged-in users only. |
| GET | `/verify-email/{id}/{hash}` | `verification.verify` | The signed link from the mail: marks the email verified. Logged-in users only. |
| POST | `/email/verification-notification` | `verification.send` | Mails the verification link again. Logged-in users only. |

`.magic_link()` and `.login_codes(..)` add the [passwordless](#passwordless-login-magic-links-and-one-time-codes)
pages. All of them are for guests only:

| Method | Address | Name | What it does |
|---|---|---|---|
| GET | `/login/link` | `login.link.request` | Asks for an email address. |
| POST | `/login/link` | `login.link.send` | Mails a login link (the same answer for any address). |
| GET | `/login/link/{token}` | `login.link.show` | "Log in as …?" Opening it uses nothing up. |
| POST | `/login/link/{token}` | `login.link.consume` | Logs in and burns the token. |
| GET | `/login/code` | `login.code.request` | Asks for an identifier (and a channel, with several). |
| POST | `/login/code` | `login.code.send` | Sends a code. |
| GET, POST | `/login/code/verify` | `login.code.verify` | The page that takes the code, and checking it. |
| POST | `/login/code/resend` | `login.code.resend` | A new code, perhaps on another channel. |

The verification routes are always there. `.verify_email()` makes Renox send the mail when
someone registers; `.require_verified()` on your routes sends unverified users to
`verification.notice`.

`.account()` adds the account page. All of these need a logged-in user, and the last two ask for
the password in the form:

| Method | Address | Name | What it does |
|---|---|---|---|
| GET | `/account` | `account.show` | The account page. |
| PUT | `/account/profile` | `account.profile` | Changes the name and email. |
| PUT | `/account/password` | `account.password` | Changes the password (this logs out the other devices). |
| POST | `/account/logout-others` | `account.logout_others` | Logs out every other device. |
| DELETE | `/account` | `account.destroy` | Deletes the account, then goes to the `home` route (or `/`). |

`.notifications()` adds the in-app notification list behind the UI kit's `notification_bell`.
All of these need a logged-in user too:

| Method | Address | Name | What it does |
|---|---|---|---|
| GET | `/notifications` | `notifications.index` | The list (with htmx, only the bell's panel). |
| DELETE | `/notifications` | `notifications.clear` | Deletes all of the user's notifications. |
| GET | `/notifications/stream` | `notifications.stream` | Server-Sent Events: the unread count, and new notifications. |
| POST | `/notifications/read-all` | `notifications.read_all` | Marks them all read. |
| POST | `/notifications/{id}/read` | `notifications.read` | Marks one read. |
| POST | `/notifications/{id}/unread` | `notifications.unread` | Marks one unread. |
| POST | `/notifications/{id}/open` | `notifications.open` | Marks one read and goes where it points. |
| DELETE | `/notifications/{id}` | `notifications.destroy` | Deletes one. |

`rnx route:list` prints them all for your app. To change how a page looks, put a file with the
same name as the built-in one (such as `renox/auth/login.html`) in your views.

## Testing authorization

Renox's `TestApp` has helpers for this:

- `TestApp::acting_as(&user)` logs a user in.
- `confirm_password()` lets it through `require_password_confirmed`.
- `assert_forbidden()`, `assert_not_found()` and `assert_redirect("/login")` check the refusals.

> [!TIP]
> Test the "no" cases, not just the "yes" ones: another user's row, another team's row, a token
> without the ability, a guest.

## Coming from Laravel

> [!NOTE]
> **Coming from Laravel:** each Laravel tool has a Renox counterpart. This table maps them.

| Laravel | Renox |
|---|---|
| `Gate::define`, `@can`, `can:` middleware | `App::gate` / `gate_async`, `can(…)`, `.require_gate(…)` |
| Policies, `$this->authorize()` | `impl Policy`, `user.authorize(…)?`, `Can::new` for views |
| `Gate::before` | `App::gate_before` |
| FormRequest `authorize()` | `impl Validate { async fn authorize(&self, form: &FormContext) }`, or `#[derive(Validate)]` + `#[validate(hooks)]` + `impl ValidateHooks` |
| spatie `User::role('x')->get()` | `permissions::users_with_role(&db, "x")` |
| spatie/laravel-permission | the `Permissions` module |
| spatie Teams (`setPermissionsTeamId`), Bouncer scopes (`Bouncer::scope()->to(…)`) | `assign_role_in(&db, role, &Scope::of(&store))` + `permissions::set_scope` |
| Sanctum abilities, `tokenCan` | `create_token_with`, `.require_ability(…)`, `token_can` |
| Global scopes (`addGlobalScope`), tenancy packages | `#[model(default_scope = "…")]` + `renox::context` |
| `password.confirm` middleware | `.require_password_confirmed()` |
| spatie/laravel-activitylog | the `Audit` module |
