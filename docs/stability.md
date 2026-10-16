# Stability and versions

This page tells you what can change when you upgrade Renox, and what can't. In short: from 1.0,
any 1.x upgrade should just work, and the few exceptions are listed here.

### In this guide

- [The promise](#the-promise): what every 1.x release keeps working.
- [What 1.x may add without a major release](#what-1x-may-add-without-a-major-release): things that may grow, and how to write code that doesn't mind.
- [Public dependencies](#public-dependencies): other crates you can see through Renox's API.
- [Also outside the promise](#also-outside-the-promise): what may change in any release.

### Words you'll meet

| Word | What it means |
|---|---|
| **crate** | A Rust package. Renox is made of a few crates; your app is one too. |
| **version number** | Three numbers, like `1.4.2`: **major**.**minor**.**patch**. |
| **semver** (semantic versioning) | A rule for version numbers: the major number goes up only when an upgrade can break your code. |
| **breaking change** | A change that can make your code stop compiling, or stop working as before. |
| **minor release** | A release like 1.4 → 1.5. It may add things, but must not break anything. |
| **major release** | A release like 1.x → 2.0. It may break things, and the changelog tells you how to move over. |
| **public API** | Everything your code can use: types, functions, methods, macros, settings. |
| **deprecated** | Marked as "still works, but please use this other thing". The compiler warns you. |
| **`#[non_exhaustive]`** | A Rust marker on a struct or enum that says "this may get more fields or variants later". |
| **MSRV** | Minimum supported Rust version: the oldest Rust that can build Renox. |
| **dependency** | Another crate that Renox (or your app) uses. |

### Versions in short

From 1.0, Renox follows [semantic versioning]. That means: code that compiles against
`renox = "1.x"` keeps compiling on every later 1.x release, except where this page says
otherwise.

Before 1.0, breaking changes were allowed. Each release's notes list them.

New apps made by `rnx new` depend on the Renox version of the `rnx` that made them. An `rnx`
installed from crates.io writes that version as `renox = "1.2"`, which lets `cargo update` take
fixes and new features but never a breaking change. An
`rnx` built from a Git checkout pins the app to that exact commit instead, so the Renox code
doesn't change by itself.

[semantic versioning]: https://semver.org

### Which crate to depend on

Renox is three library crates: `renox`, `renox-core` and `renox-macros`.

- They are released together, always with the same version number.
- They pin each other exactly. That's because the macros write code that uses renox-core's
  items from the same release.
- **Depend on `renox` only.** The other two come along with it.

`renox-cli` (the `rnx` tool) has the same version too. The apps it makes depend on its own
release of Renox.

`renox-2fa` is an optional plugin crate: two-factor authentication for apps that want it
([two-factor.md](two-factor.md)). It is released at the same version as `renox` and depends on
it. You add it yourself, next to `renox`. Its public items (the module, its events, the
`TwoFactorCredential` model, `totp`, `recovery` and `qr`) follow the same promise as Renox's.

`renox-editors` is another optional plugin crate: rich text, Markdown and code editor fields
and a code entry ([editors.md](editors.md)), at the same version as `renox`. Its public items
(the `Editors` module, `RichText`, `sanitize`, the library version constants) and its
template macros' arguments follow the same promise. Which tags `sanitize` keeps may shrink in
any release if one turns out to be unsafe; the versions of the bundled JavaScript libraries
may change in minor releases. It depends publicly on no other crate.

`renox-blocks` is the optional plugin crate of interactive blocks beyond the UI kit
([blocks.md](blocks.md)), at the same version as `renox`. Its public items (the `Blocks`
module and `TEXTS`) and its template macros' arguments follow the same promise. Its `rx-`
class names and `data-rx-*` attributes may change in minor releases (style the blocks through
the kit's tokens, not their inner classes), and texts may be added to `TEXTS`. It depends
publicly on no other crate.

`renox-oauth` is the optional social login plugin crate ([oauth.md](oauth.md)), at the same
version as `renox`. Its public items (the `OAuth` module, the `Provider` trait, `Profile`,
`Token`, `TokenRequest`, `Credentials`, `Google`, `GitHub`, the `OAuthAccount` model and its
events) follow the same promise. `Profile`, `Token`, `TokenRequest` and the events are
`#[non_exhaustive]`; `Provider` may get new methods with default bodies in minor releases. The
endpoints and scopes Google and GitHub use may change when the providers change them.

`renox-admin` is the optional admin panel plugin crate ([admin.md](admin.md)), at the same
version as `renox`. Its public items (the `Admin` module, the `AdminResource` trait, `Field`,
`FieldKind`, `Entry`, `Filter`, `AdminAction`, `ActionContext`, `ActionInput`, `SaveContext`,
`RelationManager`, `Slot`) follow the same promise. `Field`, `FieldKind`, `Entry`,
`ActionContext`, `SaveContext` and `Slot` are `#[non_exhaustive]`; `AdminResource` may
get new methods with default bodies in minor releases. The templates' markup and the values
they receive may change in minor releases; an app that replaced one keeps its own. It
depends publicly on `renox` (its grid's `Column` and `Grid`, `Query`, `Validator`).

`renox-billing` is the optional subscriptions plugin crate ([billing.md](billing.md)), at the
same version as `renox`. Its public items (the `Billing` module, `Plan`, `Interval`,
`Customer`, `Billable`, `Owner`, the `Gateway` trait, `Remote`, `Notice`, `Payment`,
`Checkout`, `CheckoutRequest`, `Stripe`, `Xendit`, `SubscriptionRoutes` (with its `_named` guards), the `Subscription`
and `BillingCustomer` models, `SubscriptionStatus` and its events) follow the same promise.
`Plan`, `Interval`, `Owner`, `Notice`, `Payment`, `Checkout`, `CheckoutRequest`,
`SubscriptionStatus` (which gained `Paused`) and the events (`SubscriptionPaused`, `SubscriptionUnpaused` among them) are `#[non_exhaustive]`; `Gateway` may get new methods
with default bodies in minor releases. The Stripe and Xendit API calls (and the API versions
they read) may change when the providers change them; the templates' markup and the values
they receive may change in minor releases. It depends publicly on `renox` (`AppState`,
`Config`, `Routes`, axum's `HeaderMap`).

## The promise

From 1.0.0, every 1.x release keeps these promises.

**Your code keeps compiling.** Public items aren't removed, renamed or changed in a breaking
way.

To catch mistakes, CI (the checks that run on every change to Renox) runs [cargo-semver-checks]
on every pull request. From 1.0 it compares the code with the latest release on crates.io. So
an accidental break fails the build before it ships.

**Deprecated first, removed only in 2.0.** When an item is replaced, it gets `#[deprecated]`,
with a note naming its replacement. It stays deprecated for at least one minor release, and it
stays in Renox until the next major release.

**Your data keeps working.**

- Framework tables only change through new migrations (never by editing an old one). So after
  an upgrade, running `migrate` is enough.
- Sessions, encrypted columns, signed URLs, API tokens and password hashes made by one 1.x
  release can be read by every later one. So an upgrade logs nobody out.
- Jobs queued by one 1.x release run on later ones. That matters for a rolling deploy, where the
  old and new versions share one queue for a while.

**Your settings keep their meaning.** A `.env` setting keeps its name and its default. A new
setting comes with a default that keeps the old behaviour.

**Fixes go to the latest minor release.** Security fixes also go to the minor release before it,
for six months after the newer one ships (see [SECURITY.md](../SECURITY.md)).

### When a change would break the promise

A change that would break any of this waits for 2.0. CHANGELOG.md then lists it, with a way to
move over.

> [!NOTE]
> Bug fixes are the exception. Say Renox accepted an invalid value, while its docs said it
> wouldn't. A minor release may make it stop, and the changelog marks the change.

[cargo-semver-checks]: https://github.com/obi1kenobi/cargo-semver-checks

## What 1.x may add without a major release

Adding things is not a breaking change, as long as your code is written to expect it. This
section lists what may grow, and the small habits that keep your code safe.

Most of these types are marked `#[non_exhaustive]`. The compiler then makes you write your code
in the safe way, so you can't get it wrong by accident.

### New fields on structs

**New fields on these structs.** Build them with their constructors, `Default` or
`TestApp::with_config(app, |c| …)`, not with struct literals.

A struct literal is when you write every field out, like `Config { name: …, port: … }`. If a
new field is added later, that line stops compiling. A constructor or `Default` fills in the new
field for you.

- `Config`, `MailConfig`, `StorageConfig`, `AnalyticsConfig`
- `Mail`, `User`, `Paginated`, `SimplePage`, `CursorPage`, `RouteInfo`, `MigrationStatus`,
  `FailedJob`, `audit::AuditLog`, the `auth::events` structs
- `DatabaseNotification`, `AccessToken`, `NewToken`, `auth::DeviceToken`, `auth::NewDeviceToken`,
  `auth::Device`, `i18n::RequestLocale`
- `WebhookRequest`, `WebhookCall`, `JobContext`, `Htmx`, `Down`, `analytics::Event`
- `auth::Can<T>` (use `Can::new`), `schedule::UpcomingRun`, `maintenance::DownOptions` (use
  `DownOptions::new()` or `Default`)
- `view::Component`, `view::Prop`, `view::ViewContext`, `auth::Registration`, `auth::Recipient`, `mail::Attachment`
- `Toast`, `ToastAction`, `auth::DatabaseMessage`, `auth::PendingLogin`, `chart::Series`,
  `report::ErrorReport`, `report::RequestReport`, `validation::FormContext`,
  `rate_limit::LimitRequest`, `SentNotification`, `SentBroadcast`, `db::InvalidUlid`, `grid::Grid`,
  `grid::Column`, `db::ModelColumn`, `db::ModelIndex`, `db::Condition` (made by a typed
  column: `Product::PRICE.lt(…)`), `grid::GridPrefs`, `grid::RowOrder`, `grid::Workbook`,
  `grid::Action` (`Action::sheet`), `grid::Selection`, `storage::FileInfo`, `queue::BatchStatus`,
  `queue::QueueCounts`, `queue::QueueStats` (the dashboard's), `http::SentRequest`,
  `select::SelectOption` (use `SelectOption::new`), `select::OptionQuery`, `Upload` (use
  `Upload::new`), `db::Migration` (use `Migration::new(..).sqlite(..).postgres(..)`),
  `live_component::Mounted`, `import::ImportReport`, `import::FailedRow`, `auth::permissions::Assignment`
  (`auth::permissions::Scope` has private fields: make one with `Scope::of`, `of_id`, `new`
  or `global`; `Scopes` stays `All` / `Only`, so a `match` on it needs no `_` arm)
- `AppStateInner`: what an `AppState` holds, read as the state's fields
  (`state.db`, `state.config`) through `Deref`; only Renox builds one (#334)

### New variants on enums

**New variants on these enums.** A `match` on them needs a `_` arm.

A `_` arm is the "anything else" case at the end of a `match`. With it, your `match` still
compiles when a new variant appears.

- `Error`, `Environment`, `CspMode`, `PermissionsMode`, `security::PermissionsPolicy`, `Channel`, `DbValue`, `Inspected` (a `Rule`
  matching on `Inspected` needs a `_` arm), `ToastKind`, `chart::Bucket`, `report::ReportKind`,
  `grid::Kind`, `db::ColumnKind`, `db::search::Normalize`, `grid::Summary`, `grid::ExportFormat`
- the settings: `SessionDriver`, `LogFormat`, `CacheStore`, `mail::MailDriver`,
  `mail::MailEncryption`, `storage::DiskDriver`, and `webhook::WebhookStatus`

### New fields on an enum variant

**New fields on this enum variant.** A pattern on it needs `..`.

`..` in a pattern means "and any other fields". With it, the pattern still matches when a field
is added.

- `Inspected::File { image, .. }` (it gained `dimensions` in M33)

### New methods, functions and settings

1.x may also add new methods, functions, modules, template functions, validation rules, CLI
commands and `.env` settings. New settings always come with defaults.

A command your app binary gets in 1.x never stops your app from booting: an app command
(`App::command`) with the same name keeps running in its place. Only the 1.0 built-ins
(`migrate`, `queue:work`, … as `help` lists them) are names an app command can't take.

### New trait methods with defaults

1.x may add new provided methods and associated constants, with defaults, on traits you
implement. Examples: `Model`, `Notification`, `ModelHooks`, `validation::ValidateHooks`,
`Validate::ERROR_BAG`, and so on.

(A **provided method** is a trait method that already has a body. You don't have to write it,
so a new one doesn't break your `impl`.)

`FromRow` stays one method. `view::ViewData` is meant to be derived (`#[derive(ViewData)]`);
a constant added to it in 1.x comes with a default like the others.

### Sealed traits

Some traits are **sealed**: only Renox can implement them. That lets Renox add to them without
breaking anyone.

- `db::Number` is sealed: only `i64` and `f64`.
- `db::ModelKey` is sealed too (`i64`, `Ulid`, `Uuid`, `String`). So new key types and new
  methods on it aren't breaking.
- So is `RedirectExt` (only for axum's `Redirect`).
- `db::Executor` is sealed: only `&Db` and `&mut Transaction` (and the `db::Conn` that
  `into_conn` makes of them, for several statements).
- `db::relations::ForeignKey` is sealed: only a key type or an `Option` of one.
- `db::Comparable` (which values a typed `Col` accepts) is implemented by Renox only.

### Why `Dialect` isn't on the list

`Dialect` (the list of supported databases) is deliberately not in that list. Supporting a third
database would change the SQL every app writes, so it would come with a major release.

## Public dependencies

Some other crates show up in Renox's API: you use their types when you use Renox. When one of
them makes a breaking release, Renox moves to it in a **major** release (or keeps the old one).
That way `renox = "1"` never breaks your code because of a dependency.

| Crate | Where it shows up |
|---|---|
| `axum` (0.8) | Handlers and extractors (`Query`, `Form`, `Json`, `State`), `Routes::route(MethodRouter)`, `From<axum::Router>`, `Kernel::router()`, re-exported as `renox::axum`. The prelude's `Path` is Renox's own `renox::Path` (a 404 when a value doesn't parse) |
| `clap` (4) | `command::AppCommand` (a `clap::Parser`), re-exported as `renox::clap` |
| `anyhow` (1) | `Error::Internal`, `Error::permanent`, re-exported as `renox::anyhow` |
| `tower` / `tower-http` (0.5 / 0.7) | `App::layer(L)`, `Routes::route_layer(L)`, `Routes::cors_layer(CorsLayer)` (`renox::cors`) |
| `minijinja` (2) | `context!`, template values, `App::templates` / `Registry::templates` (a `minijinja::Environment`), re-exported as `renox::minijinja` |
| `tokio` (1), `serde` (1), `serde_json` (1), `chrono` (0.4) | Re-exported and used throughout |
| `bytes` (1) | `Bytes` in `Storage::put`/`get`, `Upload::bytes`, `WebhookRequest::body`, `Download::bytes` (as `axum::body::Bytes`) |
| `http` (1) | `HeaderMap`, `Method`, `StatusCode` in `FormContext`, `WebhookRequest`, `TestResponse` (as `axum::http`) |
| `fake` (5) | `Factory` definitions, re-exported as `renox::fake` (the `fake` feature, on by default) |
| `uuid` (1) | `Uuid` model keys and fields, re-exported as `renox::uuid` (the `uuid` feature) |

"Re-exported as `renox::axum`" means you can reach that crate through Renox, without adding it
to your own `Cargo.toml`.

### sqlx

**`sqlx` is not part of the stable API.** sqlx is the database library Renox uses inside.

- Database errors are Renox's own `db::DbError`. Rows are `db::Row`, and values go through
  `ToDbValue`/`FromDb`.
- For anything Renox doesn't cover, some **escape hatches** hand out sqlx's own types:
  `Db::sqlite()`, `Db::postgres()`, `Row::sqlite()`, `Row::postgres()`, `DbError::sqlx()` and the
  re-export `renox::db::sqlx`.
- Renox may move to a new sqlx version in a minor release. Code that uses the escape hatches may
  then need a change.

> [!WARNING]
> `FromDb` and `RowIndex` are implemented for whatever sqlx can decode, through sqlx's traits.
> The promise only covers the types Renox lists:
>
> - for `FromDb`: the integer and float types, `bool`, `String`, `Vec<u8>`, the chrono date and
>   time types, `Option<T>` of those, `db::Json<T>`, `db::Encrypted<T>`, `db::Ulid`, `Uuid`
>   (the `uuid` feature) and `#[derive(DbEnum)]` enums;
> - for `RowIndex`: column names (`&str`) and positions (`usize`).
>
> A type of your own, made decodable by implementing sqlx's traits, works too. But it is an
> escape hatch: a sqlx upgrade may need it changed.

## Also outside the promise

These things may change in any release, even a minor one.

- **Hidden items.** Items marked `#[doc(hidden)]` are used by Renox's own macros. Examples are
  `Model`'s `values`, `set_id`, `touch` and `set_deleted_at`. Implement `Model` with
  `#[derive(Model)]`, not by hand.
- **The HTML of the built-in pages** under `renox/…`. To fix their markup, override them in
  your views.
- **The inside of the UI kit.** For the UI kit (`renox/ui.html`), these are kept: the macro
  names and keyword arguments, the `rx-*` class names apps use, and the `--rx-*` tokens. Its
  inner markup may change. (`rnx make:component --ui` copies the kit into your app, to freeze
  it as it is.)
- **The exact wording of built-in messages.**
- **The minimum supported Rust version (MSRV).** It is now Rust 1.94 (`rust-version` in
  `Cargo.toml`, checked in CI). It may rise in a minor release, but only to a Rust release at
  least six months old, and CHANGELOG.md says so.
- **What `rnx new` and the `rnx make:*` generators write.** Generated files are yours. A newer
  `rnx` may write them differently (the starter kit too), but it never changes files you
  already have.
