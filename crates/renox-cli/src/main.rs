//! `rnx`: the command-line tool for the Renox web framework.

mod deploy;
mod doctor;
mod finish;
mod format;
mod generate;
mod make;
mod new;
mod scaffold;
mod serve;
mod tailwind;
mod tools;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "rnx",
    version,
    about = "rnx: the Renox web framework CLI",
    after_help = "Any other command runs in your app (cargo run -- <command>), e.g.\n  rnx queue:work, rnx queue:failed, rnx schedule:list"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new Renox application.
    New {
        /// Directory and package name, e.g. `shop`.
        name: String,
        /// Use a local checkout of Renox instead of the Git repository.
        #[arg(long, value_name = "DIR")]
        renox_path: Option<PathBuf>,
        /// The database the app starts with.
        #[arg(long, value_enum, default_value_t = Database::Sqlite)]
        database: Database,
        /// Style pages with Tailwind CSS (its standalone CLI, no Node) next to the UI kit.
        #[arg(long)]
        tailwind: bool,
        /// The starter kit: email verification, roles, a dashboard, the users
        /// page and the activity log in a sidebar layout.
        #[arg(long)]
        starter: bool,
        /// The notification bell in the navigation bar: in-app
        /// notifications, live (the starter kit always has it).
        #[arg(long)]
        notifications: bool,
    },
    /// Check this machine and app, and say how to fix what's missing.
    Doctor {
        /// Skip building the app (no database or migrations check).
        #[arg(long)]
        no_build: bool,
    },
    /// Run the app, rebuilding and restarting it when source files change.
    Serve {
        /// Extra arguments for `cargo build`, e.g. `--release`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        cargo_args: Vec<String>,
    },
    /// Create a migration in `migrations/`, e.g. `create_products_table`.
    #[command(name = "make:migration")]
    MakeMigration {
        /// Snake-case description, e.g. `create_products_table`. Optional with `--auto`.
        name: Option<String>,
        /// Directory for the migration files.
        #[arg(long, default_value = "migrations")]
        path: PathBuf,
        /// Write the migration from what changed in the models (`db:diff`).
        #[arg(long)]
        auto: bool,
        /// With `--auto`: accept ambiguous changes without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Create a module: routes, an index view, and its registration.
    #[command(name = "make:module")]
    MakeModule {
        /// e.g. `products` or `stock_items`.
        name: String,
        /// A whole resource: model, migration, factory, form, the seven
        /// handlers (`Routes::resource`), views on the UI kit, and tests.
        #[arg(long)]
        resource: bool,
        /// With --resource: the fields, e.g. "name:string price:money
        /// notes:text active:bool due:date" (string, text, int, money, float,
        /// bool, date). Default: name:string.
        #[arg(long)]
        fields: Option<String>,
        /// With --resource: the model's name (default: the module's in the
        /// singular, `products` → `Product`).
        #[arg(long)]
        model: Option<String>,
        /// With --resource: don't run migrate afterwards.
        #[arg(long)]
        no_migrate: bool,
        /// With --resource: open the new page in the browser.
        #[arg(long)]
        open: bool,
    },
    /// Fake records for a model (`impl Factory`).
    #[command(name = "make:factory")]
    MakeFactory {
        model: String,
        #[arg(long)]
        module: String,
    },
    /// A seeder for `db:seed`, registered on the App.
    #[command(name = "make:seeder")]
    MakeSeeder { name: String },
    /// An integration test file in tests/.
    #[command(name = "make:test")]
    MakeTest { name: String },
    /// A notification (mail and database) in a module.
    #[command(name = "make:notification")]
    MakeNotification {
        name: String,
        #[arg(long)]
        module: String,
    },
    /// An event and a listener registered in its module.
    #[command(name = "make:event")]
    MakeEvent {
        name: String,
        #[arg(long)]
        module: String,
    },
    /// A validation rule (`impl Rule`) in a module.
    #[command(name = "make:rule")]
    MakeRule {
        name: String,
        #[arg(long)]
        module: String,
    },
    /// A middleware on every route of the app, registered with App::layer.
    #[command(name = "make:middleware")]
    MakeMiddleware { name: String },
    /// Create a model (and with --migration, its table's migration).
    #[command(name = "make:model")]
    MakeModel {
        /// e.g. `Product`.
        name: String,
        /// Module to put it in; defaults to the model's name in snake_case.
        #[arg(long)]
        module: Option<String>,
        /// Also create a `create_<table>_table` migration.
        #[arg(short, long)]
        migration: bool,
        /// The `id` type: a database-numbered integer, a ULID or UUID made
        /// on insert, or a string the app sets.
        #[arg(long, value_enum, default_value = "integer")]
        key: KeyType,
    },
    /// Create a queued job in a module.
    #[command(name = "make:job")]
    MakeJob {
        /// e.g. `SendReceipt`.
        name: String,
        #[arg(long)]
        module: String,
    },
    /// Create an app command in a module (`my-app <name>`).
    #[command(name = "make:command")]
    MakeCmd {
        /// e.g. `admin:create`.
        name: String,
        #[arg(long)]
        module: String,
    },
    /// Create a policy for a module's model.
    #[command(name = "make:policy")]
    MakePolicy {
        /// The model, e.g. `Product`.
        model: String,
        #[arg(long)]
        module: String,
    },
    /// Create a view component (a macro that can use `old`, `error`, `t`…),
    /// or with `--ui` copy Renox's UI kit into the app to change it.
    #[command(name = "make:component")]
    MakeComponent {
        /// e.g. `price_tag`: resources/views/components/price_tag.html.
        name: Option<String>,
        /// Copy renox/ui.html and its CSS into the app (`my-app ui:publish`).
        #[arg(long)]
        ui: bool,
        /// With --ui: replace files already there.
        #[arg(long)]
        force: bool,
        /// Write the older macro form (`{% from … import … %}`) instead of an
        /// `<app-…>` tag.
        #[arg(long = "macro")]
        macro_form: bool,
    },
    /// Create an HTML and a text mail template.
    #[command(name = "make:mail")]
    MakeMail {
        /// e.g. `order_shipped`.
        name: String,
    },
    /// Run pending migrations.
    Migrate {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    /// Undo the last batch of migrations (`--step N` for more).
    #[command(name = "migrate:rollback")]
    MigrateRollback {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Drop all tables and run every migration (`--seed` to seed too).
    #[command(name = "migrate:fresh")]
    MigrateFresh {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// List migrations and whether they have run.
    #[command(name = "migrate:status")]
    MigrateStatus,
    /// Compare the registered models with the tables the migrations build.
    #[command(name = "db:check")]
    DbCheck,
    /// Run the app's seeders.
    #[command(name = "db:seed")]
    DbSeed,
    /// Build a release binary into dist/ (and, with Tailwind, minified CSS first).
    Build,
    /// Build public/css/app.css with Tailwind (`rnx serve` and `rnx build` do it for you).
    Tailwind {
        /// Rebuild whenever a view or the input changes.
        #[arg(long)]
        watch: bool,
        /// Optimize and minify, as `rnx build` does.
        #[arg(long)]
        minify: bool,
    },
    /// Download the pinned Tailwind CSS standalone CLI (checked by SHA-256) and print its path.
    #[command(name = "tailwind:install")]
    TailwindInstall,
    /// Create a Dockerfile, a systemd unit, a Litestream config and a deploy guide.
    #[command(name = "make:deploy")]
    MakeDeploy,
    /// Generate an APP_KEY and write it to `.env`.
    #[command(name = "key:generate")]
    KeyGenerate {
        /// Print the key instead of writing it to `.env`.
        #[arg(long)]
        show: bool,
    },
    /// Any other command is run by the app itself.
    #[command(external_subcommand)]
    App(Vec<String>),
}

fn main() -> Result<()> {
    let result = run(Cli::parse().command);
    // The Rust files the command wrote, in rustfmt's style (format.rs).
    if result.is_ok() {
        format::format_touched();
    }
    result
}

fn run(command: Command) -> Result<()> {
    match command {
        Command::New {
            name,
            renox_path,
            database,
            tailwind,
            starter,
            notifications,
        } => new::run(
            &name,
            renox_path.as_deref(),
            new::Options {
                database,
                tailwind,
                starter,
                notifications,
            },
        ),
        Command::Tailwind { watch, minify } => tailwind::run(&app_root()?, watch, minify),
        Command::TailwindInstall => {
            println!("{}", tailwind::binary()?.display());
            Ok(())
        }
        Command::Doctor { no_build } => doctor::run(no_build),
        Command::Serve { cargo_args } => serve::run(&cargo_args),
        Command::KeyGenerate { show } => key_generate(show),
        Command::Build => deploy::build(&app_root()?),
        Command::MakeDeploy => deploy::make_deploy(&app_root()?),
        Command::MakeMigration {
            name,
            path,
            auto,
            yes,
        } => {
            if auto {
                app_command("db:diff", &diff_args(name, &path, yes))
            } else if yes {
                anyhow::bail!("--yes only works with --auto")
            } else {
                let name = name.context("give a name, or --auto")?;
                make::migration(&name, &path)
            }
        }
        Command::MakeModule {
            name,
            resource: true,
            fields,
            model,
            no_migrate,
            open,
        } => {
            let root = app_root()?;
            let path = scaffold::resource(&root, &name, model.as_deref(), fields.as_deref())?;
            finish::after_resource(&root, &path, !no_migrate, open)
        }
        Command::MakeModule { name, .. } => generate::module(&app_root()?, &name),
        Command::MakeFactory { model, module } => generate::factory(&app_root()?, &model, &module),
        Command::MakeSeeder { name } => generate::seeder(&app_root()?, &name),
        Command::MakeTest { name } => generate::test(&app_root()?, &name),
        Command::MakeNotification { name, module } => {
            generate::notification(&app_root()?, &name, &module)
        }
        Command::MakeEvent { name, module } => generate::event(&app_root()?, &name, &module),
        Command::MakeRule { name, module } => generate::rule(&app_root()?, &name, &module),
        Command::MakeMiddleware { name } => generate::middleware(&app_root()?, &name),
        Command::MakeModel {
            name,
            module,
            migration,
            key,
        } => generate::model(&app_root()?, &name, module.as_deref(), migration, key),
        Command::MakeJob { name, module } => generate::job(&app_root()?, &name, &module),
        Command::MakeCmd { name, module } => generate::command(&app_root()?, &name, &module),
        Command::MakePolicy { model, module } => generate::policy(&app_root()?, &model, &module),
        Command::MakeMail { name } => generate::mail(&app_root()?, &name),
        Command::MakeComponent {
            ui: true, force, ..
        } => {
            let args: Vec<String> = if force {
                vec!["--force".into()]
            } else {
                Vec::new()
            };
            app_command("ui:publish", &args)
        }
        Command::MakeComponent {
            name: Some(name),
            macro_form,
            ..
        } => generate::component(&app_root()?, &name, macro_form),
        Command::MakeComponent { .. } => Err(anyhow::anyhow!("give a name, or --ui")),
        Command::Migrate { args } => app_command("migrate", &args),
        Command::MigrateRollback { args } => app_command("migrate:rollback", &args),
        Command::MigrateFresh { args } => app_command("migrate:fresh", &args),
        Command::MigrateStatus => app_command("migrate:status", &[]),
        Command::DbCheck => app_command("db:check", &[]),
        Command::DbSeed => app_command("db:seed", &[]),
        Command::App(args) => match args.split_first() {
            Some((command, rest)) => app_command(command, rest),
            None => Ok(()),
        },
    }
}

/// The arguments `rnx make:migration --auto` passes to the app's `db:diff`.
fn diff_args(name: Option<String>, path: &Path, yes: bool) -> Vec<String> {
    let mut args: Vec<String> = name.into_iter().collect();
    if path != Path::new("migrations") {
        args.push("--path".into());
        args.push(path.display().to_string());
    }
    if yes {
        args.push("--yes".into());
    }
    args
}

/// Runs a command built into the app binary (`cargo run -- <command>`), since
/// migrations and seeders are compiled into the app.
fn app_command(command: &str, args: &[String]) -> Result<()> {
    if !app_command_status(command, args)? {
        std::process::exit(1);
    }
    Ok(())
}

/// Runs a command built into the app binary and says whether it succeeded.
pub(crate) fn app_command_status(command: &str, args: &[String]) -> Result<bool> {
    if !std::path::Path::new("Cargo.toml").is_file() {
        anyhow::bail!("no Cargo.toml here; run `rnx {command}` from your app's directory");
    }
    let status = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--", command])
        .args(args)
        .status()
        .context("could not run cargo")?;
    Ok(status.success())
}

/// A setting as the app sees it: `var(name)` when not empty, else the
/// `name=` line of the `.env` text.
pub(crate) fn setting_with(
    var: impl Fn(&str) -> Option<String>,
    env_file: Option<&str>,
    name: &str,
) -> Option<String> {
    if let Some(value) = var(name).filter(|v| !v.is_empty()) {
        return Some(value);
    }
    let prefix = format!("{name}=");
    env_file?
        .lines()
        .find_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
            line.strip_prefix(prefix.as_str())
        })
        .map(|v| v.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|v| !v.is_empty())
}

/// A setting of the app in `dir`: the environment, then its `.env`.
pub(crate) fn setting_in(dir: &Path, name: &str) -> Option<String> {
    setting_with(
        |n| std::env::var(n).ok(),
        std::fs::read_to_string(dir.join(".env")).ok().as_deref(),
        name,
    )
}

/// The current directory, if it looks like a Renox app.
/// The database engine an app uses.
/// A model's key type (`make:model --key`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum KeyType {
    Integer,
    Ulid,
    Uuid,
    String,
}

impl KeyType {
    /// The `id` field's Rust type.
    pub(crate) fn rust_type(self) -> &'static str {
        match self {
            KeyType::Integer => "i64",
            KeyType::Ulid => "Ulid",
            KeyType::Uuid => "Uuid",
            KeyType::String => "String",
        }
    }

    /// The `id` column in a `CREATE TABLE`.
    pub(crate) fn column(self, database: Database) -> &'static str {
        match (self, database) {
            (KeyType::Integer, Database::Sqlite) => "id INTEGER PRIMARY KEY AUTOINCREMENT",
            (KeyType::Integer, Database::Postgres) => {
                "id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY"
            }
            (KeyType::Uuid, Database::Sqlite) => "id BLOB PRIMARY KEY",
            (KeyType::Uuid, Database::Postgres) => "id UUID PRIMARY KEY",
            (KeyType::Ulid | KeyType::String, _) => "id TEXT PRIMARY KEY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Database {
    Sqlite,
    Postgres,
}

impl Database {
    /// The app's engine, from `DATABASE_URL` in the environment or in `.env`
    /// in the current directory; SQLite when neither says otherwise.
    pub(crate) fn of_current_app() -> Self {
        Self::of_url(
            setting_in(Path::new("."), "DATABASE_URL")
                .as_deref()
                .unwrap_or_default(),
        )
    }

    pub(crate) fn of_url(url: &str) -> Self {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Database::Postgres
        } else {
            Database::Sqlite
        }
    }
}

fn app_root() -> Result<PathBuf> {
    app_root_in(std::env::current_dir()?)
}

/// `root` if it holds an app (a `Cargo.toml` and `src/`).
fn app_root_in(root: PathBuf) -> Result<PathBuf> {
    if !root.join("Cargo.toml").is_file() || !root.join("src").is_dir() {
        anyhow::bail!("run this from your app's directory (the one with Cargo.toml and src/)");
    }
    Ok(root)
}

pub(crate) fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    format!("base64:{}", STANDARD.encode(bytes))
}

fn key_generate(show: bool) -> Result<()> {
    key_generate_in(std::path::Path::new("."), show)
}

/// `rnx key:generate` in the app at `dir`.
fn key_generate_in(dir: &std::path::Path, show: bool) -> Result<()> {
    let key = generate_key();
    if show {
        println!("{key}");
        return Ok(());
    }

    // No .env yet (a fresh clone): start one from .env.example, or empty.
    let env = match std::fs::read_to_string(dir.join(".env")) {
        Ok(env) => env,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if !dir.join("Cargo.toml").is_file() {
                anyhow::bail!("no .env or Cargo.toml here; run from your app or pass --show");
            }
            std::fs::read_to_string(dir.join(".env.example")).unwrap_or_default()
        }
        Err(err) => return Err(err).context("could not read .env"),
    };
    std::fs::write(dir.join(".env"), with_key(&env, &key)).context("could not write .env")?;
    println!("APP_KEY written to .env. Existing sessions are now invalid.");
    Ok(())
}

/// `.env`'s text with `APP_KEY` set to `key` (added when missing; a later
/// definition is commented out, an `export` kept).
fn with_key(env: &str, key: &str) -> String {
    let mut replaced = false;
    let mut lines: Vec<String> = env
        .lines()
        .map(|line| {
            // `export APP_KEY=…` (a .env also sourced by a shell) keeps its `export`.
            let export = line.starts_with("export ");
            let rest = line.strip_prefix("export ").unwrap_or(line).trim_start();
            if rest.starts_with("APP_KEY=") && !replaced {
                replaced = true;
                format!("{}APP_KEY={key}", if export { "export " } else { "" })
            } else if rest.starts_with("APP_KEY=") {
                // A second definition would override the new key.
                format!("# {line}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    if !replaced {
        lines.push(format!("APP_KEY={key}"));
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    #[test]
    fn diff_args_are_built_in_order() {
        use super::diff_args;
        use std::path::Path;
        let p = Path::new("migrations");
        assert!(diff_args(None, p, false).is_empty());
        assert_eq!(diff_args(Some("x".into()), p, true), ["x", "--yes"]);
        assert_eq!(
            diff_args(Some("x".into()), Path::new("m2"), true),
            ["x", "--path", "m2", "--yes"]
        );
    }

    #[test]
    fn settings_come_from_the_environment_then_env() {
        let env = |v: &'static str| move |n: &str| (n == "A").then(|| v.to_owned());
        let file = "B=1\nexport A=\"x y\"\nA_OLD=no\nC=''\nD='q'\n";
        assert_eq!(
            setting_with(env("e"), Some(file), "A").as_deref(),
            Some("e")
        );
        assert_eq!(
            setting_with(env(""), Some(file), "A").as_deref(),
            Some("x y")
        );
        assert_eq!(
            setting_with(|_| None, Some(file), "D").as_deref(),
            Some("q")
        );
        assert_eq!(setting_with(|_| None, Some(file), "C"), None);
        assert_eq!(setting_with(|_| None, Some(file), "Z"), None);
        assert_eq!(setting_with(|_| None, None, "A"), None);
        assert_eq!(setting_with(|_| None, Some("A_OLD=no\n"), "A"), None);
    }

    use super::*;

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(std::iter::once("rnx").chain(args.iter().copied()))
            .unwrap()
            .command
    }

    #[test]
    fn commands_parse_with_their_options() {
        assert!(matches!(
            parse(&["new", "shop", "--database", "postgres", "--tailwind"]),
            Command::New { name, database: Database::Postgres, tailwind: true, renox_path: None, starter: false, notifications: false } if name == "shop"
        ));
        assert!(matches!(
            parse(&["new", "shop", "--starter"]),
            Command::New {
                starter: true,
                tailwind: false,
                ..
            }
        ));
        assert!(matches!(
            parse(&["new", "shop", "--notifications"]),
            Command::New {
                notifications: true,
                starter: false,
                ..
            }
        ));
        assert!(matches!(
            parse(&["make:model", "Order", "-m", "--key", "ulid"]),
            Command::MakeModel {
                migration: true,
                key: KeyType::Ulid,
                module: None,
                ..
            }
        ));
        assert!(matches!(
            parse(&["make:model", "Order"]),
            Command::MakeModel {
                key: KeyType::Integer,
                ..
            }
        ));
        assert!(matches!(
            parse(&["make:module", "products", "--resource", "--fields", "name price:money"]),
            Command::MakeModule { resource: true, fields: Some(f), model: None, .. } if f == "name price:money"
        ));
        assert!(matches!(
            parse(&["doctor", "--no-build"]),
            Command::Doctor { no_build: true }
        ));
        assert!(matches!(
            parse(&[
                "make:module",
                "products",
                "--resource",
                "--no-migrate",
                "--open"
            ]),
            Command::MakeModule {
                resource: true,
                no_migrate: true,
                open: true,
                ..
            }
        ));
        assert!(matches!(
            parse(&["serve", "--release"]),
            Command::Serve { cargo_args } if cargo_args == ["--release"]
        ));
        assert!(matches!(
            parse(&["migrate:rollback", "--step", "2"]),
            Command::MigrateRollback { args } if args == ["--step", "2"]
        ));
        assert!(matches!(
            parse(&["make:component", "--ui", "--force"]),
            Command::MakeComponent {
                name: None,
                ui: true,
                force: true,
                macro_form: false
            }
        ));
        assert!(matches!(
            parse(&["make:component", "price_tag", "--macro"]),
            Command::MakeComponent {
                name: Some(_),
                macro_form: true,
                ..
            }
        ));
        // Anything else goes to the app.
        assert!(matches!(
            parse(&["queue:work", "--queue", "mail"]),
            Command::App(args) if args == ["queue:work", "--queue", "mail"]
        ));
        // A generator's required option is required.
        assert!(Cli::try_parse_from(["rnx", "make:job", "SendReceipt"]).is_err());
        assert!(Cli::try_parse_from(["rnx", "make:model", "X", "--key", "float"]).is_err());
    }

    #[test]
    fn database_urls_pick_the_engine() {
        assert_eq!(Database::of_url("postgres://u@h/db"), Database::Postgres);
        assert_eq!(Database::of_url("postgresql://u@h/db"), Database::Postgres);
        assert_eq!(
            Database::of_url("sqlite://storage/app.db"),
            Database::Sqlite
        );
        assert_eq!(Database::of_url(""), Database::Sqlite);
    }

    #[test]
    fn key_types_have_their_columns() {
        assert_eq!(KeyType::Uuid.rust_type(), "Uuid");
        assert_eq!(
            KeyType::Uuid.column(Database::Postgres),
            "id UUID PRIMARY KEY"
        );
        assert_eq!(
            KeyType::Uuid.column(Database::Sqlite),
            "id BLOB PRIMARY KEY"
        );
        assert_eq!(
            KeyType::String.column(Database::Sqlite),
            "id TEXT PRIMARY KEY"
        );
        assert!(
            KeyType::Integer
                .column(Database::Sqlite)
                .contains("AUTOINCREMENT")
        );
    }

    #[test]
    fn generated_keys_are_32_random_bytes() {
        let key = generate_key();
        let bytes = STANDARD
            .decode(key.strip_prefix("base64:").unwrap())
            .unwrap();
        assert_eq!(bytes.len(), 32);
        assert_ne!(key, generate_key());
    }

    #[test]
    fn the_key_goes_into_env() {
        assert_eq!(with_key("", "k"), "APP_KEY=k\n");
        assert_eq!(
            with_key("APP_NAME=Shop\nAPP_KEY=\nAPP_DEBUG=true", "k"),
            "APP_NAME=Shop\nAPP_KEY=k\nAPP_DEBUG=true\n"
        );
        assert_eq!(
            with_key("export APP_KEY=old\nAPP_KEY=other", "k"),
            "export APP_KEY=k\n# APP_KEY=other\n"
        );
    }

    // #248: key:generate and the app's directory, without changing the
    // working directory (tests run in parallel).

    #[test]
    fn key_generate_starts_from_the_example_or_refuses_outside_an_app() {
        let dir = tempfile::tempdir().unwrap();
        let err = key_generate_in(dir.path(), false).unwrap_err();
        assert!(
            err.to_string().contains("no .env or Cargo.toml here"),
            "{err}"
        );
        key_generate_in(dir.path(), true).unwrap(); // --show writes nothing
        assert!(!dir.path().join(".env").exists());

        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
        std::fs::write(dir.path().join(".env.example"), "APP_NAME=Shop\nAPP_KEY=\n").unwrap();
        key_generate_in(dir.path(), false).unwrap();
        let env = std::fs::read_to_string(dir.path().join(".env")).unwrap();
        assert!(env.contains("APP_NAME=Shop"), "{env}");
        assert!(env.contains("APP_KEY=base64:"), "{env}");
        // Again: a new key replaces the old one.
        key_generate_in(dir.path(), false).unwrap();
        let again = std::fs::read_to_string(dir.path().join(".env")).unwrap();
        assert_ne!(again, env);
        assert_eq!(again.matches("APP_KEY=").count(), 1, "{again}");

        // A .env that can't be read (here a directory) is an error, not a
        // fresh start that would drop its settings.
        let odd = tempfile::tempdir().unwrap();
        std::fs::create_dir(odd.path().join(".env")).unwrap();
        let err = key_generate_in(odd.path(), false).unwrap_err();
        assert!(err.to_string().contains("could not read .env"), "{err}");
    }

    #[test]
    fn the_app_root_needs_a_manifest_and_src() {
        let dir = tempfile::tempdir().unwrap();
        let err = app_root_in(dir.path().to_path_buf()).unwrap_err();
        assert!(
            err.to_string()
                .contains("run this from your app's directory"),
            "{err}"
        );
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert!(app_root_in(dir.path().to_path_buf()).is_err(), "no src/");
        std::fs::create_dir(dir.path().join("src")).unwrap();
        assert_eq!(app_root_in(dir.path().to_path_buf()).unwrap(), dir.path());
        // From a folder inside the app (src/), it's refused too: generators
        // write paths relative to the app's root.
        assert!(app_root_in(dir.path().join("src")).is_err());
    }

    #[test]
    fn make_component_needs_a_name_or_ui() {
        let err = run(parse(&["make:component"])).unwrap_err();
        assert!(err.to_string().contains("give a name, or --ui"), "{err}");
    }
}
