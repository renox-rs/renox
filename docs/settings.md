# Settings

Some values aren't code and aren't secrets in a `.env` file: the company's name and tax number,
the format of invoice numbers, which features are on. They belong to the people who run the app,
and they should change without a deploy. Renox keeps them in the database as **typed settings**:
a plain struct per group, with defaults, validation and an audit trail, read quickly from any
handler, job or command and edited by an administrator.

In this guide:

- [Add settings to your app](#add-settings-to-your-app)
- [Read and save](#read-and-save)
- [Settings in templates and handlers](#settings-in-templates-and-handlers)
- [Secrets: encrypted groups](#secrets-encrypted-groups)
- [Commands and seeding](#commands-and-seeding)
- [A page in the admin panel](#a-page-in-the-admin-panel)
- [Events and the audit log](#events-and-the-audit-log)
- [One group per company: scopes](#one-group-per-company-scopes)
- [How it is cached](#how-it-is-cached)
- [Testing](#testing)

> [!NOTE]
> **Coming from Laravel:** this is what `spatie/laravel-settings` gives you, with the rules of a
> form request on every save.

## Add settings to your app

A group of settings is a struct that serializes, validates and has defaults. Its `KEY` names its
row in the `settings` table:

```rust
use renox::prelude::*;
use renox::settings::{Settings, SettingsGroup};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Validate, Default, Clone)]
pub struct Company {
    #[validate(required, max(120))]
    pub name: String,
    pub tax_number: Option<String>,
    /// How invoice numbers look: "INV-{yyyy}-{0000}".
    #[validate(max(40))]
    pub invoice_format: String,
}

impl SettingsGroup for Company {
    const KEY: &'static str = "company"; // one row per group
}

pub fn app() -> App {
    App::new()
        .module(Settings) // the `settings` table and the commands
        .settings::<Company>() // declares the group
}
```

`Settings` is an opt-in module, like `Audit`: it adds the `settings` table (run your migrations)
and the `settings:get`, `settings:set` and `settings:list` commands. A module can declare its own
groups in `Module::register` with `app.settings::<T>()`, so a billing module brings its own
settings with their defaults.

Add a field to a group later and nothing needs migrating: a stored group without the new field
takes the struct's `Default` for it.

## Read and save

```rust
use renox::prelude::*;
use renox::settings::SettingsGroup;
# #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
# pub struct Company { #[validate(required)] pub name: String }
# impl SettingsGroup for Company { const KEY: &'static str = "company"; }

async fn rename(state: AppState, user: User) -> Result {
    // `Arc<Company>`: the defaults until something is saved, then what was saved.
    let company = state.settings::<Company>().await?;

    let mut changed = (*company).clone();
    changed.name = "Acme".into();
    // Validated with the struct's rules; `Some(&user)` is who changed it.
    state.save_settings(&changed, Some(&user)).await?;
    Ok(())
}
```

`state.settings::<T>()` works anywhere you have an `AppState`: handlers, jobs, listeners,
scheduled tasks and commands. Saving checks the group's `#[validate]` rules and returns a
`ValidationError` (the messages your forms use) if one fails; nothing is written then. Saving a
group with the values it already has does nothing.

`state.settings_info::<T>()` tells when a group was last saved and by whom (`None` while it never
was).

## Settings in templates and handlers

In a handler, the `Setting<T>` extractor is `state.settings::<T>()`:

```rust
use renox::prelude::*;
use renox::settings::{Setting, SettingsGroup};
# #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
# pub struct Company { pub name: String }
# impl SettingsGroup for Company { const KEY: &'static str = "company"; }

async fn footer(company: Setting<Company>) -> String {
    format!("© {}", company.name)
}
```

To use a group in every template, declare it with `settings_in_views` and read it as
`settings.<key>.<field>`:

```rust
use renox::prelude::*;
use renox::settings::{Settings, SettingsGroup};
# #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
# pub struct Company { pub name: String }
# impl SettingsGroup for Company { const KEY: &'static str = "company"; }

pub fn app() -> App {
    App::new()
        .module(Settings)
        .settings_in_views::<Company>() // `{{ settings.company.name }}` in any page
}
```

An encrypted group can't be shared with views.

## Secrets: encrypted groups

Set `ENCRYPTED` and the group is sealed with `APP_KEY` before it is stored, as an
[`Encrypted`](types.md) model column is. The table holds an opaque string; reading gives the
plain struct. Use it for gateway keys and API tokens:

```rust
use renox::prelude::*;
use renox::settings::SettingsGroup;

#[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
pub struct Gateway {
    #[validate(required)]
    pub secret_key: String,
    pub webhook_secret: String,
}

impl SettingsGroup for Gateway {
    const KEY: &'static str = "gateway";
    const ENCRYPTED: bool = true;
}
```

Changing `APP_KEY` makes sealed groups unreadable, as for any `Encrypted` column. Sealed groups
are never printed: the commands show `<sealed>` unless you pass `--reveal`, the admin page
shows "Set, last changed …" instead of the values, and the audit entry names the fields that
changed without their values.

## Commands and seeding

```text
my-app settings:list
my-app settings:get company
my-app settings:get company.name
my-app settings:set company.name "Acme"
my-app settings:set company.employees 42
```

`settings:set` changes one field and saves the group, so the struct's types and rules apply: a
number for a number field, and nothing is written if a rule fails. The value is read as JSON when
it is JSON (`true`, `42`, `["a"]`), as text otherwise. `settings:get` and `settings:list` print
`<sealed>` for an encrypted group unless `--reveal` is given. All three take `--scope`.

A deploy tool can seed values on the first run with a JSON file:

```json
{ "company": { "name": "Acme", "invoice_format": "INV-{yyyy}-{0000}" } }
```

```rust
use renox::prelude::*;
use renox::settings::Settings;

pub fn app() -> App {
    App::new()
        .module(Settings)
        .settings_from("storage/settings.json")
}
```

A group is seeded the first time it is read and has no row; values already in the table win, a
missing file seeds nothing, and a seeded group has to pass its rules like any save.

## A page in the admin panel

[renox-admin](admin.md) adds a form for a group. Say which fields it has, and who may change it:

```rust
use renox::prelude::*;
use renox::settings::SettingsGroup;
use renox_admin::{Admin, AdminSettings, Field};
# #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
# pub struct Company { #[validate(required)] pub name: String, pub tax_number: Option<String> }
# impl SettingsGroup for Company { const KEY: &'static str = "company"; }

impl AdminSettings for Company {
    fn fields() -> Vec<Field> {
        vec![
            Field::text("name", "Company name").required(),
            // Shown, but only a seed file or `settings:set` changes it.
            Field::text("tax_number", "Tax number").readonly_on_edit(),
        ]
    }

    fn allows(user: &AuthUser, ability: &str) -> bool {
        ability == "view" || user.has_role("owner")
    }
}

pub fn admin() -> Admin {
    Admin::new()
        .authorize(|user| user.has_role("staff"))
        .settings::<Company>("Company") // /admin/settings/company
}
```

The page validates with the group's rules, saves through `save_settings` (so the change is
audited and emits `SettingsChanged`) and shows under a "Settings" heading of the navigation. An
encrypted group's fields start empty with a note saying whether they are set; a field left empty
keeps its stored value.

## Events and the audit log

Every save emits `SettingsChanged { key, scope, actor, changed, previous, current }`: the user,
the names of the fields that changed, and the group before and after as JSON. For an encrypted
group `previous` and `current` are `null`, so only the field names travel. Listen for it to warm a
cache or tell someone:

```rust
use renox::prelude::*;
use renox::settings::SettingsChanged;

pub fn app() -> App {
    App::new().listen(|changed: SettingsChanged, _state| async move {
        println!("{} changed: {}", changed.key, changed.changed.join(", "));
        Ok(())
    })
}
```

With the [`Audit` module](../CHEATSHEET.md#auth-events-and-the-audit-log) on, the same change is
an `audit_logs` entry named `settings.changed`, with the user and the same data.

## One group per company: scopes

An app installed for one customer sometimes needs the same group per company, branch or account:
an invoice format for each issuing company. Every read and save has a scope variant, and the
default scope is `""`:

```rust
use renox::prelude::*;
use renox::settings::SettingsGroup;
# #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
# pub struct Company { pub name: String }
# impl SettingsGroup for Company { const KEY: &'static str = "company"; }

async fn per_branch(state: AppState) -> Result {
    let branch = state.settings_in::<Company>("branch-7").await?;
    state.save_settings_in(&*branch, "branch-7", None).await?;
    Ok(())
}
```

Each scope is its own row (`(key, scope)` is the primary key) with its own cache. The commands take
`--scope`. `settings_in_views` and the admin pages use the default scope; for a page per branch,
keep a map inside the group.

## How it is cached

Each process keeps the groups it has read, so a read costs no query. The first read of a group is
one query; a save replaces the group in the process that wrote it at once and stamps a version row
in the `cache` table (`renox:settings:<key>`). With several servers on one database the others ask
for that stamp, one small query, at most once a second per group, and reload the group when it
changed. So a change shows on every server within about a second.

## Testing

`TestApp::with_settings` starts a test with a group saved:

```rust
use renox::prelude::*;
use renox::settings::{Settings, SettingsGroup};
use renox::testing::TestApp;
# #[derive(serde::Serialize, serde::Deserialize, Validate, Default, Clone)]
# pub struct Company { pub name: String }
# impl SettingsGroup for Company { const KEY: &'static str = "company"; }

#[renox::test]
async fn the_footer_names_the_company() {
    let app = TestApp::new(App::new().module(Settings).settings::<Company>())
        .await
        .with_settings(Company { name: "Acme".into() })
        .await;
    assert_eq!(app.state().settings::<Company>().await.unwrap().name, "Acme");
}
```

`renox::db::capture_queries` shows how many queries a read costs; `app.fake_events()` and
`app.assert_emitted(|e: &SettingsChanged| …)` check the event.
