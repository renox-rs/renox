//! The app's own commands, run like the built-in ones: `my-app admin:create
//! --email a@b.c` (or `rnx admin:create …` during development).
//!
//! ```
//! # use renox::prelude::*;
//! use renox::command::Args;
//!
//! async fn create_admin(args: Args, state: AppState) -> Result {
//!     let (Some(email), Some(password)) = (args.value("--email"), args.value("--password")) else {
//!         return Err(Error::BadRequest("usage: admin:create --email E --password P".into()));
//!     };
//!     User::register(&state.db, "Admin", email, password).await?;
//!     println!("Created {email}.");
//!     Ok(())
//! }
//!
//! # let _ =
//! App::new().command("admin:create", "Create an admin user (--email, --password)", create_admin)
//! # ;
//! ```
//!
//! Commands run after the app boots (the database is connected, migrations
//! are not run for you) and appear in `my-app help`.
//!
//! A typed command declares its arguments with clap, so they're parsed and
//! checked for you, and `my-app catalog:import --help` prints its usage:
//!
//! ```
//! # use renox::prelude::*;
//! use renox::clap;
//! use renox::command::AppCommand;
//!
//! /// Import products from a CSV file.
//! #[derive(clap::Parser)]
//! #[command(name = "catalog:import")]
//! struct ImportCatalog {
//!     /// The CSV file.
//!     file: std::path::PathBuf,
//!     /// Show what would change without saving.
//!     #[arg(long)]
//!     dry_run: bool,
//!     #[arg(long, default_value_t = 500)]
//!     batch: usize,
//! }
//!
//! impl AppCommand for ImportCatalog {
//!     async fn run(self, state: AppState) -> Result {
//!         println!("importing {} ({} a batch)", self.file.display(), self.batch);
//!         # let _ = state;
//!         Ok(())
//!     }
//! }
//!
//! # let _ =
//! App::new().typed_command::<ImportCatalog>()
//! # ;
//! ```
//!
//! Ask for what's missing with [`crate::prompt`].

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{AppState, Result};

/// The words after the command's name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args(Vec<String>);

impl Args {
    /// Arguments from these words, e.g. for `Kernel::call` in tests.
    pub fn new(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(args.into_iter().map(Into::into).collect())
    }

    /// Every word, as given.
    pub fn all(&self) -> &[String] {
        &self.0
    }

    /// The words that aren't flags or flag values, e.g. `["report.csv"]` for
    /// `import report.csv --dry-run`.
    pub fn positional(&self) -> Vec<&str> {
        let mut out = Vec::new();
        let mut words = self.0.iter().peekable();
        while let Some(word) = words.next() {
            if word.starts_with("--") {
                // `--flag value`: skip the value too.
                if !word.contains('=') && words.peek().is_some_and(|next| !next.starts_with("--")) {
                    words.next();
                }
            } else {
                out.push(word.as_str());
            }
        }
        out
    }

    /// The value of `--flag value` or `--flag=value`.
    pub fn value(&self, flag: &str) -> Option<&str> {
        let mut words = self.0.iter();
        while let Some(word) = words.next() {
            if word == flag {
                return words
                    .next()
                    .map(String::as_str)
                    .filter(|v| !v.starts_with("--"));
            }
            if let Some(value) = word
                .strip_prefix(flag)
                .and_then(|rest| rest.strip_prefix('='))
            {
                return Some(value);
            }
        }
        None
    }

    /// Whether `--flag` was given (with or without a value).
    pub fn has(&self, flag: &str) -> bool {
        self.0
            .iter()
            .any(|word| word == flag || word.starts_with(&format!("{flag}=")))
    }
}

/// A command whose arguments are a clap `Parser`: its `#[command(name = …)]`
/// is the command's name, and its doc comment (or `about`) the line in
/// `my-app help`. Register it with [`App::typed_command`](crate::App::typed_command).
pub trait AppCommand: clap::Parser + Send + 'static {
    /// Runs the command with its parsed arguments.
    fn run(self, state: AppState) -> impl Future<Output = Result> + Send;
}

pub(crate) type CommandFn =
    Arc<dyn Fn(AppState, Args) -> Pin<Box<dyn Future<Output = Result> + Send>> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct Command {
    pub name: String,
    pub about: String,
    pub run: CommandFn,
}

pub(crate) fn command<F, Fut>(name: &str, about: &str, run: F) -> Command
where
    F: Fn(Args, AppState) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result> + Send + 'static,
{
    Command {
        name: name.to_owned(),
        about: about.to_owned(),
        run: Arc::new(move |state, args| Box::pin(run(args, state))),
    }
}

pub(crate) fn typed<T: AppCommand>() -> Command {
    let definition = T::command();
    let name = definition.get_name().to_owned();
    let about = definition
        .get_about()
        .map(|about| about.to_string())
        .unwrap_or_default();
    let bin = name.clone();
    Command {
        name,
        about,
        run: Arc::new(move |state, args: Args| {
            let bin = bin.clone();
            Box::pin(async move {
                match T::try_parse_from(std::iter::once(bin).chain(args.0)) {
                    Ok(command) => command.run(state).await,
                    // `--help` / `--version`: printed, and that's a success.
                    Err(err)
                        if matches!(
                            err.kind(),
                            clap::error::ErrorKind::DisplayHelp
                                | clap::error::ErrorKind::DisplayVersion
                        ) =>
                    {
                        print!("{}", err.render());
                        Ok(())
                    }
                    // What's wrong, then how to call it (clap includes the
                    // usage for some errors; `Error: ` is added by main).
                    Err(err) => {
                        let rendered = err.render().to_string();
                        let message = rendered.trim_end();
                        let message = message.strip_prefix("error: ").unwrap_or(message);
                        Err(crate::Error::Internal(if message.contains("Usage:") {
                            anyhow::anyhow!("{message}")
                        } else {
                            anyhow::anyhow!("{message}\n\n{}", T::command().render_usage())
                        }))
                    }
                }
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flags_and_positional_words() {
        let args = Args::new(["report.csv", "--email", "a@b.c", "--dry-run", "--limit=5"]);
        assert_eq!(args.value("--email"), Some("a@b.c"));
        assert_eq!(args.value("--limit"), Some("5"));
        assert_eq!(args.value("--dry-run"), None);
        assert!(args.has("--dry-run") && args.has("--limit") && !args.has("--force"));
        assert_eq!(args.positional(), ["report.csv"]);
    }
}
