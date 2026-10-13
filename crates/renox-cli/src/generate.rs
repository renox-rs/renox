//! `rnx make:*` generators. Each writes new files into the app (never over
//! existing ones) and wires them up where that's safe to do automatically.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use heck::{ToKebabCase, ToSnakeCase, ToTitleCase, ToUpperCamelCase};

/// `path` relative to the app root, for messages.
fn shown(path: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    path.strip_prefix(&cwd)
        .unwrap_or(path)
        .display()
        .to_string()
}

pub(crate) fn write_new(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        bail!("{} already exists", shown(path));
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("could not write {}", path.display()))?;
    crate::format::touched(path);
    println!("Created {}", shown(path));
    Ok(())
}

pub(crate) fn check_name(name: &str) -> Result<()> {
    let ok = name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !ok {
        bail!(
            "`{name}` is not a valid name: use letters, digits, `_` or `-`, starting with a letter"
        );
    }
    let snake = name.to_snake_case();
    if is_reserved(&snake) {
        bail!("`{name}` can't be used: `{snake}` is a Rust keyword or a crate the app uses");
    }
    Ok(())
}

/// Rust keywords (current and reserved) and crate names an app's modules
/// and crate must not take.
pub(crate) fn is_reserved(word: &str) -> bool {
    const RESERVED: &[&str] = &[
        "as",
        "async",
        "await",
        "break",
        "const",
        "continue",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "false",
        "fn",
        "for",
        "gen",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "static",
        "struct",
        "super",
        "trait",
        "true",
        "type",
        "unsafe",
        "use",
        "where",
        "while",
        "abstract",
        "become",
        "box",
        "do",
        "final",
        "macro",
        "override",
        "priv",
        "try",
        "typeof",
        "unsized",
        "virtual",
        "yield",
        "std",
        "core",
        "alloc",
        "renox",
        "renox_core",
        "renox_macros",
        "serde",
        "tokio",
        "test",
    ];
    RESERVED.contains(&word)
}

/// Adds `pub mod {name};` to a `mod.rs`, after its other module lines.
pub(crate) fn add_mod(mod_rs: &Path, name: &str) -> Result<()> {
    let source = fs::read_to_string(mod_rs).unwrap_or_default();
    let line = format!("pub mod {name};");
    if source
        .lines()
        .any(|l| l.trim() == line || l.trim() == format!("mod {name};"))
    {
        return Ok(());
    }
    let mut lines: Vec<&str> = source.lines().collect();
    let at = lines
        .iter()
        .rposition(|l| l.starts_with("pub mod ") || l.starts_with("mod "))
        .map_or(0, |i| i + 1);
    lines.insert(at, &line);
    let mut out = lines.join("\n");
    out.push('\n');
    if let Some(dir) = mod_rs.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(mod_rs, out)?;
    crate::format::touched(mod_rs);
    println!("Updated {}", shown(mod_rs));
    Ok(())
}

fn module_dir(root: &Path, module: &str) -> Result<PathBuf> {
    let dir = root.join("src/app").join(module);
    if !dir.join("mod.rs").is_file() {
        bail!("there is no module `{module}`; create it with `rnx make:module {module}`");
    }
    Ok(dir)
}

/// `rnx make:module stock_items`: a module with an index route and view.
pub fn module(root: &Path, name: &str) -> Result<()> {
    check_name(name)?;
    let snake = name.to_snake_case();
    let pascal = name.to_upper_camel_case();
    let kebab = name.to_kebab_case();
    let title = name.to_title_case();

    write_new(
        &root.join("src/app").join(&snake).join("mod.rs"),
        &format!(
            r#"use renox::prelude::*;

pub struct {pascal};

impl Module for {pascal} {{
    fn name(&self) -> &'static str {{
        "{snake}"
    }}

    fn routes(&self) -> Routes {{
        Routes::new().get("/{kebab}", index).name("{snake}.index")
    }}
}}

async fn index() -> View {{
    view("{snake}/index.html", context! {{}})
}}
"#
        ),
    )?;
    write_new(
        &root.join("resources/views").join(&snake).join("index.html"),
        &format!(
            "{{% extends \"layouts/app.html\" %}}\n\n{{% block content %}}\n<h1>{title}</h1>\n{{% endblock %}}\n"
        ),
    )?;
    add_mod(&root.join("src/app/mod.rs"), &snake)?;
    // Apps from `rnx new` build the App in src/lib.rs; older ones in main.rs.
    register_in_main(&app_file(root), &format!("app::{snake}::{pascal}"))
}

/// Adds `.module(path)` after the last `.module(` call in `lib.rs`/`main.rs`.
pub(crate) fn register_in_main(main_rs: &Path, path: &str) -> Result<()> {
    register_call(main_rs, &format!(".module({path})"))
}

/// Adds `call` (e.g. `.seeder(…)`) after the last `.module(` call in
/// `lib.rs`/`main.rs`; prints it when the file doesn't look like that.
fn register_call(main_rs: &Path, call: &str) -> Result<()> {
    let hint = || println!("Register it where the App is built: {call}");
    let Ok(source) = fs::read_to_string(main_rs) else {
        hint();
        return Ok(());
    };
    if source.contains(call) {
        return Ok(());
    }
    let mut lines: Vec<String> = source.lines().map(str::to_owned).collect();
    let Some(last) = lines
        .iter()
        .rposition(|l| l.trim_start().starts_with(".module("))
    else {
        hint();
        return Ok(());
    };
    let indent: String = lines[last]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect();
    lines.insert(last + 1, format!("{indent}{call}"));
    fs::write(main_rs, lines.join("\n") + "\n")?;
    crate::format::touched(main_rs);
    println!("Updated {} ({call})", shown(main_rs));
    Ok(())
}

/// Where the App is built: `src/lib.rs` (apps from `rnx new`) or `main.rs`.
fn app_file(root: &Path) -> PathBuf {
    let lib = root.join("src/lib.rs");
    if lib.is_file() {
        lib
    } else {
        root.join("src/main.rs")
    }
}

/// Adds `mod {name};` to the app file, after its other `mod` lines.
fn add_top_mod(root: &Path, name: &str) -> Result<()> {
    let file = app_file(root);
    let source = fs::read_to_string(&file).unwrap_or_default();
    if source
        .lines()
        .any(|l| l.trim() == format!("mod {name};") || l.trim() == format!("pub mod {name};"))
    {
        return Ok(());
    }
    let mut lines: Vec<&str> = source.lines().collect();
    let at = lines
        .iter()
        .rposition(|l| l.starts_with("mod ") || l.starts_with("pub mod "))
        .map_or(0, |i| i + 1);
    let line = format!("mod {name};");
    lines.insert(at, &line);
    fs::write(&file, lines.join("\n") + "\n")?;
    crate::format::touched(&file);
    println!("Updated {} (mod {name};)", shown(&file));
    Ok(())
}

/// Adds `call` to the module's `register`, creating that method before
/// `fn routes` when the module has none. Prints what to add when the file
/// doesn't look like a generated module.
fn register_in_module(mod_rs: &Path, call: &str) -> Result<()> {
    let hint = || println!("Register it in the module's `register`: {call}");
    let Ok(source) = fs::read_to_string(mod_rs) else {
        hint();
        return Ok(());
    };
    if source.contains(call) {
        return Ok(());
    }
    let mut lines: Vec<String> = source.lines().map(str::to_owned).collect();
    if let Some(at) = lines
        .iter()
        .position(|l| l.trim() == "fn register(&self, app: &mut Registry) {")
    {
        let indent: String = lines[at]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        lines.insert(at + 1, format!("{indent}    {call}"));
    } else if let Some(at) = lines
        .iter()
        .position(|l| l.trim() == "fn routes(&self) -> Routes {")
    {
        let indent: String = lines[at]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        let method = [
            format!("{indent}fn register(&self, app: &mut Registry) {{"),
            format!("{indent}    {call}"),
            format!("{indent}}}"),
            String::new(),
        ];
        lines.splice(at..at, method);
    } else {
        hint();
        return Ok(());
    }
    fs::write(mod_rs, lines.join("\n") + "\n")?;
    crate::format::touched(mod_rs);
    println!("Updated {} ({call})", shown(mod_rs));
    Ok(())
}

/// `rnx make:model Product [--module products] [--migration] [--key ulid]`.
pub fn model(
    root: &Path,
    name: &str,
    module: Option<&str>,
    migration: bool,
    key: crate::KeyType,
) -> Result<()> {
    check_name(name)?;
    let pascal = name.to_upper_camel_case();
    let snake = name.to_snake_case();
    // Tables are plural, as `make:module --resource`, the docs and the
    // examples have them (#127): `WaitlistSignup` → `waitlist_signups`.
    let table = crate::scaffold::plural(&snake);
    let module = module.map_or_else(|| snake.clone(), |m| m.to_snake_case());
    let dir = module_dir(root, &module)?;
    let file = if dir.join("model.rs").exists() {
        snake.clone()
    } else {
        "model".into()
    };

    let (import, id_doc) = match key {
        crate::KeyType::Integer => ("", ""),
        crate::KeyType::Ulid => (
            "use renox::db::Ulid;\n",
            "    /// Made on insert (`Ulid::default()` means \"not saved yet\").\n",
        ),
        crate::KeyType::Uuid => (
            "use renox::uuid::Uuid;\n",
            "    /// A UUID v7 made on insert (needs renox's `uuid` feature).\n",
        ),
        crate::KeyType::String => (
            "",
            "    /// Set by the app: save a new row with `insert` or `create`.\n",
        ),
    };
    let id_type = key.rust_type();
    write_new(
        &dir.join(format!("{file}.rs")),
        &format!(
            r#"{import}use renox::prelude::*;
use serde::{{Deserialize, Serialize}};

#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "{table}")]
pub struct {pascal} {{
{id_doc}    pub id: {id_type},
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}}
"#
        ),
    )?;
    add_mod(&dir.join("mod.rs"), &file)?;
    register_in_module(
        &dir.join("mod.rs"),
        &format!("app.model::<{file}::{pascal}>();"),
    )?;
    if migration {
        crate::make::migration_keyed(
            &format!("create_{table}_table"),
            &root.join("migrations"),
            key,
        )?;
    }
    if let Some(hint) = key_hint(key) {
        println!("{hint}");
    }
    Ok(())
}

/// What a key type needs that a new app doesn't have.
fn key_hint(key: crate::KeyType) -> Option<&'static str> {
    (key == crate::KeyType::Uuid).then_some(
        "Uuid keys need renox's `uuid` feature: renox = { …, features = [\"uuid\"] } in Cargo.toml",
    )
}

/// `rnx make:job SendReceipt --module orders`.
pub fn job(root: &Path, name: &str, module: &str) -> Result<()> {
    check_name(name)?;
    let pascal = name.to_upper_camel_case();
    let snake = name.to_snake_case();
    let module = module.to_snake_case();
    let dir = module_dir(root, &module)?;
    write_new(
        &dir.join(format!("{snake}.rs")),
        &format!(
            r#"use renox::prelude::*;
use serde::{{Deserialize, Serialize}};

#[derive(Serialize, Deserialize)]
pub struct {pascal} {{}}

impl Job for {pascal} {{
    const NAME: &'static str = "{kebab}";

    async fn handle(self, ctx: JobContext) -> Result {{
        let _ = ctx;
        Ok(())
    }}
}}
"#,
            kebab = name.to_kebab_case()
        ),
    )?;
    add_mod(&dir.join("mod.rs"), &snake)?;
    register_in_module(
        &dir.join("mod.rs"),
        &format!("app.job::<{snake}::{pascal}>();"),
    )
}

/// `rnx make:command admin:create --module users`: an app command in the
/// module, run as `my-app admin:create …`.
pub fn command(root: &Path, name: &str, module: &str) -> Result<()> {
    let valid = name.split(':').all(|part| {
        part.starts_with(|c: char| c.is_ascii_lowercase())
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    });
    if !valid {
        bail!(
            "`{name}` is not a valid command name: use lowercase words separated by `:`, e.g. admin:create"
        );
    }
    let snake = name.replace([':', '-'], "_");
    if is_reserved(&snake) {
        bail!("`{name}` can't be used: `{snake}` is a Rust keyword or a crate the app uses");
    }
    let module = module.to_snake_case();
    let dir = module_dir(root, &module)?;
    let pascal = snake.to_upper_camel_case();
    write_new(
        &dir.join(format!("{snake}.rs")),
        &format!(
            r#"use renox::clap;
use renox::command::AppCommand;
use renox::prelude::*;

/// What it does (shown in `my-app help`).
#[derive(clap::Parser)]
#[command(name = "{name}")]
pub struct {pascal} {{
    /// Show what would happen without changing anything.
    #[arg(long)]
    pub dry_run: bool,
}}

impl AppCommand for {pascal} {{
    /// `my-app {name} [--dry-run]`; `--help` lists the arguments. Ask for what's
    /// missing with `renox::prompt::ask(…)`.
    async fn run(self, state: AppState) -> Result {{
        let _ = state;
        println!("{name}: done{{}}", if self.dry_run {{ " (dry run)" }} else {{ "" }});
        Ok(())
    }}
}}
"#
        ),
    )?;
    add_mod(&dir.join("mod.rs"), &snake)?;
    register_in_module(
        &dir.join("mod.rs"),
        &format!("app.typed_command::<{snake}::{pascal}>();"),
    )
}

/// `rnx make:policy Product --module products`: `impl Policy` for a model.
pub fn policy(root: &Path, model: &str, module: &str) -> Result<()> {
    check_name(model)?;
    let pascal = model.to_upper_camel_case();
    let module = module.to_snake_case();
    let dir = module_dir(root, &module)?;
    write_new(
        &dir.join("policy.rs"),
        &format!(
            r#"use renox::prelude::*;

use super::model::{pascal};

impl Policy for {pascal} {{
    fn allows(&self, user: &User, ability: &str) -> bool {{
        let _ = user;
        match ability {{
            "view" => true,
            // e.g. "update" | "delete" => self.user_id == user.id,
            _ => false,
        }}
    }}
}}
"#
        ),
    )?;
    add_mod(&dir.join("mod.rs"), "policy")
}

/// `rnx make:mail order_shipped`: an HTML and a text template.
pub fn mail(root: &Path, name: &str) -> Result<()> {
    check_name(name)?;
    let snake = name.to_snake_case();
    let dir = root.join("resources/views/mail");
    write_new(
        &dir.join(format!("{snake}.html")),
        r#"{% extends "renox/mail/layout.html" %}
{% from "renox/mail/button.html" import button %}
{% block content %}
<p>Hello!</p>
{{ button(app.url, "Open " ~ app.name) }}
{% endblock %}
"#,
    )?;
    write_new(
        &dir.join(format!("{snake}.txt")),
        "Hello!\n\n{{ app.url }}\n",
    )?;
    println!(
        "Send it with: state.queue_mail(state.mail_view(to, subject, \"mail/{snake}\", context! {{}})?).await?\n\
         (or state.mailer.send(…) to send it now)"
    );
    Ok(())
}

/// `rnx make:factory Product --module products`: fake records for seeders and tests.
pub fn factory(root: &Path, model: &str, module: &str) -> Result<()> {
    check_name(model)?;
    let pascal = model.to_upper_camel_case();
    let snake = model.to_snake_case();
    let module = module.to_snake_case();
    let dir = module_dir(root, &module)?;
    let from = if dir.join(format!("{snake}.rs")).is_file() {
        snake.clone()
    } else {
        "model".into()
    };
    write_new(
        &dir.join(format!("{snake}_factory.rs")),
        &format!(
            r#"use renox::fake::Fake;
use renox::fake::faker::lorem::en::Word;
use renox::prelude::*;

use super::{from}::{pascal};

/// Fake records: `{pascal}::factory().make_one()` (unsaved),
/// `{pascal}::factory().create_one(&db)`, `{pascal}::factory().count(20).create(&db)`.
impl Factory for {pascal} {{
    fn definition() -> Self {{
        let _word: String = Word().fake(); // fill the fields with fake data
        {pascal} {{
            ..Default::default()
        }}
    }}
}}
"#
        ),
    )?;
    add_mod(&dir.join("mod.rs"), &format!("{snake}_factory"))
}

/// `rnx make:seeder DemoData`: a seeder for `db:seed`, registered on the App.
pub fn seeder(root: &Path, name: &str) -> Result<()> {
    check_name(name)?;
    let snake = name.to_snake_case();
    write_new(
        &root.join(format!("src/seeders/{snake}.rs")),
        r#"use renox::prelude::*;

/// Run by `rnx db:seed` (and `migrate:fresh --seed`), in the app's context:
/// `renox::context::app()` gives the config, `encrypt`, the cache.
pub async fn run(state: AppState) -> Result {
    let _ = state; // e.g. User::register(&state.db, "Admin", "admin@example.com", "password123").await?;
    Ok(())
}
"#,
    )?;
    add_mod(&root.join("src/seeders/mod.rs"), &snake)?;
    add_top_mod(root, "seeders")?;
    register_call(&app_file(root), &format!(".seeder(seeders::{snake}::run)"))
}

/// `rnx make:test Checkout`: an integration test file.
pub fn test(root: &Path, name: &str) -> Result<()> {
    check_name(name)?;
    let snake = name.to_snake_case();
    let crate_name = crate::scaffold::crate_name(root)?;
    write_new(
        &root.join(format!("tests/{snake}.rs")),
        &format!(
            r#"use renox::prelude::*;
use renox::testing::TestApp;

#[renox::test]
async fn {snake}_works() {{
    let app = TestApp::new({crate_name}::app()).await;
    let user = User::register(app.db(), "Test", "test@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&user);
    app.get("/").await.assert_ok();
    // Also: app.post(..), assert_redirect, assert_see, assert_view,
    // assert_database_has, app.fake_events(), app.fake_notifications(),
    // app.fake_http(), app.travel(..), app.run_jobs().
}}
"#
        ),
    )?;
    Ok(())
}

/// `rnx make:notification OrderShipped --module orders`.
pub fn notification(root: &Path, name: &str, module: &str) -> Result<()> {
    check_name(name)?;
    let pascal = name.to_upper_camel_case();
    let snake = name.to_snake_case();
    let kebab = name.to_kebab_case();
    let module = module.to_snake_case();
    let dir = module_dir(root, &module)?;
    write_new(
        &dir.join(format!("{snake}.rs")),
        &format!(
            r#"use renox::auth::{{Channel, Notification, Recipient}};
use renox::mail::Mail;
use renox::prelude::*;

/// Send it with `state.notify(&user, &{pascal} {{ .. }})` (now) or
/// `state.notify_later(&user, &…)` (through the queue).
pub struct {pascal} {{
    pub id: i64,
}}

impl Notification for {pascal} {{
    fn kind(&self) -> &'static str {{
        "{kebab}"
    }}

    fn channels(&self, _to: &Recipient) -> Vec<Channel> {{
        vec![Channel::Mail, Channel::Database]
    }}

    fn to_mail(&self, to: &Recipient, state: &AppState) -> Result<Mail> {{
        // Written in the recipient's language: t() in a mail view, or
        // state.current_lang().t(..) here.
        let _ = state;
        Ok(Mail::new(to.email().unwrap_or_default(), "{title}", "…"))
    }}

    fn to_database(&self, _to: &Recipient, _state: &AppState) -> Result<renox::serde_json::Value> {{
        Ok(json!({{ "id": self.id }}))
    }}
}}
"#,
            title = name.to_title_case()
        ),
    )?;
    add_mod(&dir.join("mod.rs"), &snake)
}

/// `rnx make:event OrderPlaced --module orders`: the event and a listener.
pub fn event(root: &Path, name: &str, module: &str) -> Result<()> {
    check_name(name)?;
    let pascal = name.to_upper_camel_case();
    let snake = name.to_snake_case();
    let module = module.to_snake_case();
    let dir = module_dir(root, &module)?;
    write_new(
        &dir.join(format!("{snake}.rs")),
        &format!(
            r#"use renox::prelude::*;

/// Emit it with `state.emit({pascal} {{ .. }}).await?`; the listeners
/// registered in the module's `register` run in turn.
#[derive(Clone, Debug)]
pub struct {pascal} {{
    pub id: i64,
}}

impl Event for {pascal} {{}}
"#
        ),
    )?;
    add_mod(&dir.join("mod.rs"), &snake)?;
    register_in_module(
        &dir.join("mod.rs"),
        &format!(
            "app.listen(|event: {snake}::{pascal}, _state| async move {{ let _ = event.id; Ok(()) }});"
        ),
    )
}

/// `rnx make:rule TaxId --module invoices`: a validation rule to `apply`.
pub fn rule(root: &Path, name: &str, module: &str) -> Result<()> {
    check_name(name)?;
    let pascal = name.to_upper_camel_case();
    let snake = name.to_snake_case();
    let module = module.to_snake_case();
    let dir = module_dir(root, &module)?;
    write_new(
        &dir.join(format!("{snake}.rs")),
        &format!(
            r#"use renox::validation::{{Inspected, Rule}};

/// `v.field("{snake}", &self.{snake}).required().apply(&{pascal})`.
pub struct {pascal};

impl Rule for {pascal} {{
    fn check(&self, value: &Inspected) -> std::result::Result<(), String> {{
        let Inspected::Text(text) = value else {{
            return Ok(());
        }};
        if text.trim().is_empty() {{
            return Err("The :attribute is not valid.".into()); // :attribute is the field's label
        }}
        Ok(())
    }}
}}
"#
        ),
    )?;
    add_mod(&dir.join("mod.rs"), &snake)
}

/// `rnx make:middleware StampRequests`: a middleware on every route.
pub fn middleware(root: &Path, name: &str) -> Result<()> {
    check_name(name)?;
    let snake = name.to_snake_case();
    write_new(
        &root.join(format!("src/middleware/{snake}.rs")),
        r#"use renox::axum::extract::Request;
use renox::axum::middleware::Next;
use renox::prelude::*;

/// Runs around every route of the app's modules (`App::layer`), after the
/// session and the user are loaded: take `Option<AuthUser>`, `Session`… as
/// arguments before `req`.
pub async fn handle(req: Request, next: Next) -> Response {
    // Before the handler: e.g. renox::context::set(..), or return early.
    let res = next.run(req).await;
    // After: e.g. add a header.
    res
}
"#,
    )?;
    add_mod(&root.join("src/middleware/mod.rs"), &snake)?;
    add_top_mod(root, "middleware")?;
    register_call(
        &app_file(root),
        &format!(".layer(renox::axum::middleware::from_fn(middleware::{snake}::handle))"),
    )
}

pub fn component(root: &Path, name: &str, macro_form: bool) -> Result<()> {
    check_name(name)?;
    let snake = name.to_snake_case();
    let path = root.join(format!("resources/views/components/{snake}.html"));
    if macro_form {
        write_new(&path, &COMPONENT.replace("NAME", &snake))?;
        println!("Use it with: {{% from \"components/{snake}.html\" import {snake} %}}");
    } else {
        let kebab = name.to_kebab_case();
        write_new(&path, &COMPONENT_TAG.replace("KEBAB", &kebab))?;
        println!("Use it with: <app-{kebab} name=\"…\" label=\"…\">");
    }
    Ok(())
}

const COMPONENT_TAG: &str = r#"<rx-props name label>
{#- <app-KEBAB name="…" label="…"></app-KEBAB> in any page. It sees the request
    like the page does: old(), error(), t(), csrf_field(), can(), auth, request. -#}
<div class="rx-field">
  <label class="rx-label" for="rx-{{ name }}">{{ label }}</label>
  <input class="rx-input" id="rx-{{ name }}" name="{{ name }}" value="{{ old(name) }}"
    {%- if error(name) %} aria-invalid="true"{% endif %} aria-describedby="rx-{{ name }}-error">
  <p class="rx-error" id="rx-{{ name }}-error" data-error-for="{{ name }}" aria-live="polite">{{ error(name) }}</p>
  <rx-slot />
</div>
"#;

const COMPONENT: &str = r#"{#- {% from "components/NAME.html" import NAME %}{{ NAME("field", "Label") }}
    A component sees the request like the page does: old(), error(), t(),
    csrf_field(), can(), auth, request. -#}
{% macro NAME(name, label) -%}
<div class="rx-field">
  <label class="rx-label" for="rx-{{ name }}">{{ label }}</label>
  <input class="rx-input" id="rx-{{ name }}" name="{{ name }}" value="{{ old(name) }}"
    {%- if error(name) %} aria-invalid="true"{% endif %} aria-describedby="rx-{{ name }}-error">
  <p class="rx-error" id="rx-{{ name }}-error" data-error-for="{{ name }}" aria-live="polite">{{ error(name) }}</p>
</div>
{%- endmacro %}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/app/home")).unwrap();
        fs::write(dir.path().join("src/app/mod.rs"), "pub mod home;\n").unwrap();
        fs::write(
            dir.path().join("src/app/home/mod.rs"),
            "use renox::prelude::*;\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/main.rs"),
            "mod app;\n\nfn main() -> renox::Result {\n    renox::App::new()\n        .migrations(renox::migrations!())\n        .module(app::home::Home)\n        .run()\n}\n",
        )
        .unwrap();
        dir
    }

    fn read(dir: &tempfile::TempDir, path: &str) -> String {
        fs::read_to_string(dir.path().join(path)).unwrap()
    }

    #[test]
    fn modules_are_created_and_registered() {
        let dir = app();
        module(dir.path(), "stock_items").unwrap();
        let code = read(&dir, "src/app/stock_items/mod.rs");
        assert!(code.contains("pub struct StockItems;"));
        assert!(
            code.contains(r#"Routes::new().get("/stock-items", index).name("stock_items.index")"#)
        );
        assert!(
            read(&dir, "resources/views/stock_items/index.html").contains("<h1>Stock Items</h1>")
        );
        assert_eq!(
            read(&dir, "src/app/mod.rs"),
            "pub mod home;\npub mod stock_items;\n"
        );
        assert!(read(&dir, "src/main.rs").contains(
            "        .module(app::home::Home)\n        .module(app::stock_items::StockItems)\n        .run()"
        ));
        assert!(
            module(dir.path(), "stock_items").is_err(),
            "never overwrites"
        );
        assert!(module(dir.path(), "../evil").is_err());
    }

    #[test]
    fn models_jobs_policies_and_mails() {
        let dir = app();
        module(dir.path(), "product").unwrap();
        model(dir.path(), "Product", None, true, crate::KeyType::Integer).unwrap();
        let module_file = read(&dir, "src/app/product/mod.rs");
        assert!(
            module_file.contains("app.model::<model::Product>();"),
            "{module_file}"
        );
        let code = read(&dir, "src/app/product/model.rs");
        // Plural tables, as `--resource` and the docs have them (#127).
        assert!(
            code.contains(r#"#[model(table = "products")]"#)
                && code.contains("pub struct Product {"),
            "{code}"
        );
        let mut migrations: Vec<String> = fs::read_dir(dir.path().join("migrations"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        migrations.sort();
        assert_eq!(migrations.len(), 2, "up and down");
        assert!(
            migrations[0].ends_with("_create_products_table.down.sql"),
            "{migrations:?}"
        );
        let up = read(&dir, &format!("migrations/{}", migrations[1]));
        assert!(up.contains(r#"CREATE TABLE "products""#), "{up}");

        model(
            dir.path(),
            "Category",
            Some("product"),
            false,
            crate::KeyType::Integer,
        )
        .unwrap();
        model(
            dir.path(),
            "Invoice",
            Some("product"),
            false,
            crate::KeyType::Ulid,
        )
        .unwrap();
        let invoice = read(&dir, "src/app/product/invoice.rs");
        assert!(
            invoice.starts_with("use renox::db::Ulid;\n")
                && invoice.contains("    pub id: Ulid,\n"),
            "{invoice}"
        );
        assert!(
            dir.path().join("src/app/product/category.rs").exists(),
            "model.rs is taken"
        );
        assert!(read(&dir, "src/app/product/category.rs").contains(r#"table = "categories""#));
        model(
            dir.path(),
            "WaitlistSignup",
            Some("product"),
            false,
            crate::KeyType::Integer,
        )
        .unwrap();
        assert!(
            read(&dir, "src/app/product/waitlist_signup.rs")
                .contains(r#"#[model(table = "waitlist_signups")]"#)
        );

        job(dir.path(), "MailReceipt", "product").unwrap();
        assert!(
            read(&dir, "src/app/product/mail_receipt.rs")
                .contains(r#"const NAME: &'static str = "mail-receipt";"#)
        );
        command(dir.path(), "stock:import", "product").unwrap();
        assert!(
            read(&dir, "src/app/product/stock_import.rs")
                .contains("impl AppCommand for StockImport")
        );
        assert!(command(dir.path(), "Bad Name", "product").is_err());
        assert!(command(dir.path(), "self", "product").is_err());
        policy(dir.path(), "Product", "product").unwrap();
        assert!(read(&dir, "src/app/product/policy.rs").contains("impl Policy for Product"));

        let mods = read(&dir, "src/app/product/mod.rs");
        for m in [
            "pub mod model;",
            "pub mod category;",
            "pub mod mail_receipt;",
            "pub mod stock_import;",
            "pub mod policy;",
        ] {
            assert!(mods.contains(m), "{mods}");
        }
        assert!(
            model(
                dir.path(),
                "Order",
                Some("nope"),
                false,
                crate::KeyType::Integer
            )
            .is_err(),
            "unknown module"
        );

        mail(dir.path(), "order_shipped").unwrap();
        assert!(
            read(&dir, "resources/views/mail/order_shipped.html")
                .contains("renox/mail/layout.html")
        );
        assert!(
            dir.path()
                .join("resources/views/mail/order_shipped.txt")
                .exists()
        );
    }

    #[test]
    fn jobs_and_commands_register_themselves_in_their_module() {
        let dir = app();
        module(dir.path(), "orders").unwrap();
        job(dir.path(), "SendReceipt", "orders").unwrap();
        command(dir.path(), "orders:close", "orders").unwrap();
        job(dir.path(), "SendReceipt", "orders").unwrap_err(); // the file exists
        let code = read(&dir, "src/app/orders/mod.rs");
        assert!(
            code.contains(
                "    fn register(&self, app: &mut Registry) {\n        \
             app.typed_command::<orders_close::OrdersClose>();\n        \
             app.job::<send_receipt::SendReceipt>();\n    }\n\n    fn routes(&self) -> Routes {"
            ),
            "{code}"
        );
        assert_eq!(code.matches("fn register").count(), 1);
    }

    #[test]
    fn components_use_the_request_helpers() {
        let dir = app();
        component(dir.path(), "PriceTag", false).unwrap();
        let body = read(&dir, "resources/views/components/price_tag.html");
        assert!(body.starts_with("<rx-props name label>"), "{body}");
        assert!(body.contains("<app-price-tag name=") && body.contains("<rx-slot />"));
        assert!(body.contains("{{ old(name) }}") && body.contains("{{ error(name) }}"));
        assert!(!body.contains("macro"), "{body}");
        assert!(
            component(dir.path(), "PriceTag", false).is_err(),
            "no overwrite"
        );

        component(dir.path(), "LegacyTag", true).unwrap();
        let body = read(&dir, "resources/views/components/legacy_tag.html");
        assert!(body.contains("{% macro legacy_tag(name, label) -%}"));
        assert!(body.contains("{{ old(name) }}") && body.contains("{{ error(name) }}"));
    }

    // #248: the generators the tests above don't call, and the helpers'
    // other branches.

    #[test]
    fn factories_seeders_and_tests() {
        let dir = app();
        module(dir.path(), "product").unwrap();
        model(
            dir.path(),
            "Product",
            Some("product"),
            false,
            crate::KeyType::Integer,
        )
        .unwrap();
        factory(dir.path(), "Product", "product").unwrap();
        let code = read(&dir, "src/app/product/product_factory.rs");
        assert!(code.contains("impl Factory for Product {"), "{code}");
        assert!(code.contains("use super::model::Product;"), "{code}");
        assert!(read(&dir, "src/app/product/mod.rs").contains("pub mod product_factory;"));
        let err = factory(dir.path(), "Product", "nope").unwrap_err();
        assert!(
            err.to_string().contains("there is no module `nope`"),
            "{err}"
        );

        // An app built in main.rs (older apps): the seeder goes there.
        seeder(dir.path(), "DemoData").unwrap();
        assert!(
            read(&dir, "src/seeders/demo_data.rs").contains("pub async fn run(state: AppState)")
        );
        assert_eq!(read(&dir, "src/seeders/mod.rs"), "pub mod demo_data;\n");
        let main = read(&dir, "src/main.rs");
        assert!(main.starts_with("mod app;\nmod seeders;\n"), "{main}");
        assert!(
            main.contains(
                "        .module(app::product::Product)\n        .seeder(seeders::demo_data::run)\n"
            ),
            "{main}"
        );
        // A second seeder: the mod line and the call aren't repeated.
        seeder(dir.path(), "MoreData").unwrap();
        let main = read(&dir, "src/main.rs");
        assert_eq!(main.matches("mod seeders;").count(), 1, "{main}");
        assert_eq!(main.matches(".seeder(seeders::demo_data::run)").count(), 1);

        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-shop\"\n",
        )
        .unwrap();
        test(dir.path(), "Checkout").unwrap();
        let code = read(&dir, "tests/checkout.rs");
        assert!(code.contains("TestApp::new(my_shop::app())"), "{code}");
        assert!(code.contains("async fn checkout_works()"), "{code}");
        assert!(test(dir.path(), "Checkout").is_err(), "never overwrites");
    }

    #[test]
    fn notifications_events_rules_and_middleware() {
        let dir = app();
        module(dir.path(), "orders").unwrap();
        notification(dir.path(), "OrderShipped", "orders").unwrap();
        let code = read(&dir, "src/app/orders/order_shipped.rs");
        assert!(
            code.contains(r#""order-shipped""#)
                && code.contains("impl Notification for OrderShipped")
        );
        // The first event adds `register` before `routes`; the next one
        // goes into it.
        event(dir.path(), "OrderPlaced", "orders").unwrap();
        event(dir.path(), "OrderPaid", "orders").unwrap();
        let module = read(&dir, "src/app/orders/mod.rs");
        let register = module
            .find("fn register(&self, app: &mut Registry) {")
            .unwrap();
        assert!(
            register < module.find("fn routes(&self) -> Routes {").unwrap(),
            "{module}"
        );
        assert!(
            module.contains("app.listen(|event: order_placed::OrderPlaced"),
            "{module}"
        );
        assert!(
            module.contains("app.listen(|event: order_paid::OrderPaid"),
            "{module}"
        );
        assert_eq!(module.matches("fn register").count(), 1);
        rule(dir.path(), "TaxId", "orders").unwrap();
        assert!(read(&dir, "src/app/orders/tax_id.rs").contains("impl Rule for TaxId"));
        assert!(rule(dir.path(), "self", "orders").is_err(), "a keyword");
        assert!(
            rule(dir.path(), "9lives", "orders").is_err(),
            "starts with a digit"
        );

        // An app built in lib.rs (apps from `rnx new`): the middleware goes there.
        fs::write(
            dir.path().join("src/lib.rs"),
            "mod app;\n\npub fn app() -> renox::App {\n    renox::App::new()\n        .module(app::home::Home)\n}\n",
        )
        .unwrap();
        middleware(dir.path(), "StampRequests").unwrap();
        let lib = read(&dir, "src/lib.rs");
        assert!(lib.contains("mod middleware;"), "{lib}");
        assert!(
            lib.contains(
                ".layer(renox::axum::middleware::from_fn(middleware::stamp_requests::handle))"
            ),
            "{lib}"
        );
        assert!(!read(&dir, "src/main.rs").contains("middleware"));
    }

    #[test]
    fn registration_hints_when_the_files_dont_look_generated() {
        let dir = app();
        // No `.module(` line: the call is printed, the file left alone.
        fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        seeder(dir.path(), "Demo").unwrap();
        assert_eq!(read(&dir, "src/main.rs"), "mod seeders;\nfn main() {}\n");
        // A module file with neither `register` nor `routes`.
        fs::write(dir.path().join("src/app/home/mod.rs"), "pub struct Home;\n").unwrap();
        event(dir.path(), "Visited", "home").unwrap();
        let module = read(&dir, "src/app/home/mod.rs");
        assert!(!module.contains("app.listen"), "{module}");
        assert!(module.contains("pub mod visited;"));
        // No app file at all.
        fs::remove_file(dir.path().join("src/main.rs")).unwrap();
        register_call(&app_file(dir.path()), ".seeder(x)").unwrap();
        assert!(!dir.path().join("src/main.rs").exists());
    }

    #[test]
    fn uuid_keys_and_files_that_exist() {
        let dir = app();
        model(
            dir.path(),
            "Ticket",
            Some("home"),
            false,
            crate::KeyType::Uuid,
        )
        .unwrap();
        // The module's first model is model.rs; the next ones are named.
        let code = read(&dir, "src/app/home/model.rs");
        assert!(
            code.contains("use renox::uuid::Uuid;") && code.contains("pub id: Uuid,"),
            "{code}"
        );
        model(
            dir.path(),
            "Code",
            Some("home"),
            false,
            crate::KeyType::String,
        )
        .unwrap();
        assert!(read(&dir, "src/app/home/code.rs").contains("pub id: String,"));
        // Every generator refuses to overwrite.
        job(dir.path(), "SendReceipt", "home").unwrap();
        assert!(job(dir.path(), "SendReceipt", "home").is_err());
        mail(dir.path(), "Welcome").unwrap();
        assert!(mail(dir.path(), "Welcome").is_err());
        policy(dir.path(), "Ticket", "home").unwrap();
        assert!(policy(dir.path(), "Ticket", "home").is_err());
        command(dir.path(), "tickets:close", "home").unwrap();
        assert!(command(dir.path(), "tickets:close", "home").is_err());
        // Only uuid keys need a feature.
        assert!(
            key_hint(crate::KeyType::Uuid)
                .unwrap()
                .contains("features = [\"uuid\"]")
        );
        assert_eq!(key_hint(crate::KeyType::Integer), None);
        assert_eq!(key_hint(crate::KeyType::String), None);
        // A mail whose text part exists already is refused too.
        fs::write(dir.path().join("resources/views/mail/receipt.txt"), "mine").unwrap();
        let err = mail(dir.path(), "Receipt").unwrap_err();
        assert!(format!("{err:#}").contains("receipt.txt"), "{err:#}");
        assert_eq!(read(&dir, "resources/views/mail/receipt.txt"), "mine");
    }

    /// `rnx make:seeder` in an app built in lib.rs (what `rnx new` makes),
    /// and `add_mod` when the line is there already.
    #[test]
    fn seeders_in_lib_apps_and_mod_lines_kept_once() {
        let dir = app();
        fs::write(
            dir.path().join("src/lib.rs"),
            "mod app;\n\npub fn app() -> renox::App {\n    renox::App::new()\n        .module(app::home::Home)\n}\n",
        )
        .unwrap();
        seeder(dir.path(), "DemoData").unwrap();
        let lib = read(&dir, "src/lib.rs");
        assert!(lib.contains("mod seeders;"), "{lib}");
        assert!(lib.contains(".seeder(seeders::demo_data::run)"), "{lib}");
        assert!(!read(&dir, "src/main.rs").contains("seeders"));

        // A job whose file was deleted but whose mod line stayed: made again
        // without a second line.
        job(dir.path(), "SendReceipt", "home").unwrap();
        fs::remove_file(dir.path().join("src/app/home/send_receipt.rs")).unwrap();
        job(dir.path(), "SendReceipt", "home").unwrap();
        let module = read(&dir, "src/app/home/mod.rs");
        assert_eq!(
            module.matches("pub mod send_receipt;").count(),
            1,
            "{module}"
        );
        // `mod x;` (without pub) counts as there too.
        let mod_rs = dir.path().join("src/app/home/mod.rs");
        fs::write(&mod_rs, "mod private_part;\n").unwrap();
        add_mod(&mod_rs, "private_part").unwrap();
        assert_eq!(read(&dir, "src/app/home/mod.rs"), "mod private_part;\n");
    }
}
