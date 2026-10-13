//! What `rnx make:module --resource` does after the files are written:
//! migrate, print the page's URL, optionally open it.

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use anyhow::{Result, bail};

/// Whether `APP_ENV` names a local environment (unset counts as local).
pub(crate) fn is_local(app_env: Option<&str>) -> bool {
    matches!(app_env, None | Some("local" | "dev" | "development"))
}

/// Whether to run `migrate`: always when local, else only if a person at a
/// terminal says yes.
pub(crate) fn should_migrate(
    app_env: Option<&str>,
    terminal: bool,
    ask: impl FnOnce() -> bool,
) -> bool {
    is_local(app_env) || (terminal && ask())
}

/// The page's address: `APP_URL` (or the local default) and the path.
pub(crate) fn page_url(app_url: Option<&str>, path: &str) -> String {
    format!(
        "{}/{path}",
        app_url
            .unwrap_or("http://127.0.0.1:3000")
            .trim_end_matches('/')
    )
}

/// The program and arguments that open `url` in the browser on `os`.
pub(crate) fn opener(os: &str, url: &str) -> (String, Vec<String>) {
    match os {
        "macos" => ("open".into(), vec![url.into()]),
        "windows" => (
            "cmd".into(),
            vec!["/C".into(), "start".into(), "".into(), url.into()],
        ),
        _ => ("xdg-open".into(), vec![url.into()]),
    }
}

fn ask_yes() -> bool {
    print!("APP_ENV is not local. Run migrate now? [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok()
        && line.trim_start().starts_with(['y', 'Y'])
}

/// Migrates (unless told not to), prints the URL and the layout hint, and
/// opens the page when asked.
pub fn after_resource(root: &Path, path: &str, migrate: bool, open: bool) -> Result<()> {
    let env = crate::setting_in(root, "APP_ENV");
    if !migrate {
        println!("Next: rnx migrate.");
    } else if should_migrate(env.as_deref(), std::io::stdin().is_terminal(), ask_yes) {
        if !crate::app_command_status("migrate", &[])? {
            bail!(
                "the files are written, but migrate failed (above); fix it, then run rnx migrate"
            );
        }
    } else {
        println!(
            "Not migrated (APP_ENV is {}); run rnx migrate.",
            env.as_deref().unwrap_or("local")
        );
    }
    let url = page_url(crate::setting_in(root, "APP_URL").as_deref(), path);
    println!(
        "Open {url}. The layout needs {{{{ renox_ui() }}}} in <head> and {{{{ toasts() }}}} in <body> (apps from `rnx new` have them)."
    );
    if open {
        let (program, args) = opener(std::env::consts::OS, &url);
        if let Err(e) = std::process::Command::new(&program).args(&args).spawn() {
            eprintln!("could not open the browser ({program}): {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_words() {
        for w in [None, Some("local"), Some("dev"), Some("development")] {
            assert!(is_local(w));
        }
        assert!(!is_local(Some("production")));
        assert!(!is_local(Some("testing")));
    }

    #[test]
    fn migrate_decision() {
        assert!(should_migrate(Some("local"), false, || panic!("asked")));
        assert!(!should_migrate(Some("production"), false, || panic!(
            "asked"
        )));
        assert!(should_migrate(Some("production"), true, || true));
        assert!(!should_migrate(Some("production"), true, || false));
    }

    #[test]
    fn urls() {
        assert_eq!(
            page_url(Some("https://x.io/"), "products"),
            "https://x.io/products"
        );
        assert_eq!(page_url(None, "products"), "http://127.0.0.1:3000/products");
    }

    #[test]
    fn openers() {
        assert_eq!(opener("linux", "u"), ("xdg-open".into(), vec!["u".into()]));
        assert_eq!(opener("macos", "u"), ("open".into(), vec!["u".into()]));
        assert_eq!(
            opener("windows", "u"),
            (
                "cmd".into(),
                vec!["/C".into(), "start".into(), "".into(), "u".into()]
            )
        );
    }
}
