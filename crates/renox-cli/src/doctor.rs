//! `rnx doctor`: checks this machine and app and says how to fix what's missing.

use std::path::Path;

use anyhow::Result;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::{tailwind, tools};

/// How a check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// Fine.
    Ok,
    /// Works, but could be better.
    Warn,
    /// Broken: `rnx doctor` exits with 1.
    Fail,
    /// Not checked.
    Skip,
}

impl Status {
    /// The mark printed before the check.
    fn mark(self) -> &'static str {
        match self {
            Status::Ok => "✓",
            Status::Warn => "!",
            Status::Fail => "✗",
            Status::Skip => "-",
        }
    }
}

/// One line of the report, with how to fix it.
#[derive(Debug)]
pub(crate) struct Check {
    pub status: Status,
    pub what: String,
    pub fix: Option<String>,
}

impl Check {
    pub(crate) fn ok(what: impl Into<String>) -> Check {
        Check {
            status: Status::Ok,
            what: what.into(),
            fix: None,
        }
    }

    pub(crate) fn warn(what: impl Into<String>, fix: impl Into<String>) -> Check {
        Check {
            status: Status::Warn,
            what: what.into(),
            fix: Some(fix.into()),
        }
    }

    pub(crate) fn fail(what: impl Into<String>, fix: impl Into<String>) -> Check {
        Check {
            status: Status::Fail,
            what: what.into(),
            fix: Some(fix.into()),
        }
    }

    pub(crate) fn skip(what: impl Into<String>) -> Check {
        Check {
            status: Status::Skip,
            what: what.into(),
            fix: None,
        }
    }
}

/// The report: each section's title and checks, then a summary line.
pub(crate) fn render(sections: &[(String, Vec<Check>)]) -> String {
    let mut out = String::new();
    let (mut problems, mut suggestions) = (0, 0);
    for (title, checks) in sections {
        out.push_str(title);
        out.push('\n');
        for check in checks {
            out.push_str(&format!("  {} {}\n", check.status.mark(), check.what));
            if let Some(fix) = &check.fix {
                for line in fix.lines() {
                    out.push_str(&format!("      {line}\n"));
                }
            }
            match check.status {
                Status::Fail => problems += 1,
                Status::Warn => suggestions += 1,
                _ => {}
            }
        }
        out.push('\n');
    }
    if problems + suggestions == 0 {
        out.push_str("Everything looks good.\n");
    } else {
        out.push_str(&format!(
            "{problems} problem(s), {suggestions} suggestion(s).\n"
        ));
    }
    out
}

/// 1 if any check failed, else 0.
pub(crate) fn exit_code(sections: &[(String, Vec<Check>)]) -> i32 {
    let failed = sections
        .iter()
        .any(|(_, checks)| checks.iter().any(|c| c.status == Status::Fail));
    i32::from(failed)
}

/// `X.Y[.Z]` as numbers (a missing patch is 0).
fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = match parts.next() {
        Some(p) => p.parse().ok()?,
        None => 0,
    };
    Some((major, minor, patch))
}

/// Is the Rust that `rustc --version` printed at least `msrv`?
fn rust_check(version: Option<&str>, msrv: &str) -> Check {
    let missing = || Check::fail("Rust: rustc not found", "install Rust: https://rustup.rs");
    let Some(text) = version else {
        return missing();
    };
    let Some(found) = text
        .strip_prefix("rustc ")
        .and_then(|rest| rest.split_whitespace().next())
    else {
        return missing();
    };
    let (Some(have), Some(need)) = (parse_version(found), parse_version(msrv)) else {
        return missing();
    };
    let what = format!("Rust {found} (Renox needs {msrv} or newer)");
    if have >= need {
        Check::ok(what)
    } else {
        Check::fail(what, "rustup update stable")
    }
}

/// Which linker this machine uses, and whether a faster one is on offer.
fn linker_check(
    os: &str,
    arch: &str,
    gnu: bool,
    configured: Option<tools::Linker>,
    has: impl Fn(&str) -> bool,
) -> Check {
    if let Some(linker) = configured {
        return Check::ok(format!("Linker: {} (set in a Cargo config)", linker.name()));
    }
    match (os, arch) {
        ("linux", "x86_64") if gnu => Check::ok(
            "Linker: rust-lld (Rust's default here); mold is faster: install mold and clang, \
             then add .cargo/config.toml as docs/development.md shows",
        ),
        ("linux", "aarch64") if gnu => match tools::fast_linker(os, arch, gnu, has) {
            Some(linker) => Check::warn(
                format!("Linker: {} is installed but not used", linker.name()),
                tools::linker_config(linker, "aarch64-unknown-linux-gnu"),
            ),
            None => Check::warn(
                "Linker: the system default (slow)",
                "install mold and clang (e.g. sudo apt install mold clang), then run rnx doctor again",
            ),
        },
        ("macos", _) => Check::ok("Linker: the system linker (ld-prime)"),
        ("windows", _) => Check::warn(
            "Linker: link.exe (slow)",
            "add to .cargo/config.toml:\n[target.x86_64-pc-windows-msvc]\nlinker = \"rust-lld.exe\"",
        ),
        _ => Check::skip("Linker: no advice for this platform"),
    }
}

/// Is sccache in use, or at least installed?
fn sccache_check(configured: bool, on_path: bool) -> Check {
    if configured {
        Check::ok("sccache: on")
    } else if on_path {
        Check::warn(
            "sccache: installed but not used",
            "export RUSTC_WRAPPER=sccache   (add it to your shell's profile)",
        )
    } else {
        Check::warn(
            "sccache: not installed (shares compiled crates between apps)",
            "cargo install sccache --locked",
        )
    }
}

/// Is there a `.env` in the app?
fn env_check(root: &Path) -> Check {
    if root.join(".env").is_file() {
        Check::ok(".env")
    } else {
        Check::fail(".env: missing", "cp .env.example .env && rnx key:generate")
    }
}

/// Is `APP_KEY` set to something the framework accepts (32 bytes or more)?
fn key_check(value: Option<&str>) -> Check {
    let fix = "rnx key:generate";
    let Some(value) = value else {
        return Check::fail("APP_KEY: not set", fix);
    };
    let len = match value.strip_prefix("base64:") {
        Some(rest) => match STANDARD.decode(rest) {
            Ok(bytes) => bytes.len(),
            Err(_) => return Check::fail("APP_KEY: not valid base64", fix),
        },
        None => value.len(),
    };
    if len < 32 {
        Check::fail("APP_KEY: shorter than 32 bytes", fix)
    } else {
        Check::ok("APP_KEY")
    }
}

/// Does `build.rs` rerun when migrations change?
fn build_rs_check(text: Option<&str>) -> Check {
    if text.is_some_and(|t| t.contains("rerun-if-changed=migrations")) {
        Check::ok("build.rs reruns when migrations change")
    } else {
        Check::fail(
            "build.rs: new migrations won't be seen",
            "in build.rs's main: println!(\"cargo:rerun-if-changed=migrations\");",
        )
    }
}

/// Is a Tailwind binary available (`bin` is `TAILWIND_BIN`)?
fn tailwind_check(bin: Option<&str>, pinned: Option<&Path>) -> Check {
    if let Some(bin) = bin {
        if Path::new(bin).is_file() {
            return Check::ok("Tailwind: TAILWIND_BIN");
        }
        return Check::fail(
            format!("Tailwind: TAILWIND_BIN={bin} is not a file"),
            "unset TAILWIND_BIN, or point it at the binary",
        );
    }
    if pinned.is_some_and(Path::is_file) {
        Check::ok(format!("Tailwind {}", tailwind::VERSION))
    } else {
        Check::warn("Tailwind: not downloaded yet", "rnx tailwind:install")
    }
}

/// The app's own checks.
fn project_checks(root: &Path, no_build: bool) -> Vec<Check> {
    let mut checks = vec![
        env_check(root),
        key_check(crate::setting_in(root, "APP_KEY").as_deref()),
        build_rs_check(
            std::fs::read_to_string(root.join("build.rs"))
                .ok()
                .as_deref(),
        ),
    ];
    if tailwind::enabled(root) {
        let bin = std::env::var("TAILWIND_BIN").ok().filter(|v| !v.is_empty());
        let pinned = tailwind::pinned_path().ok().filter(|p| p.is_file());
        checks.push(tailwind_check(bin.as_deref(), pinned.as_deref()));
    }
    if no_build {
        checks.push(Check::skip("Database and migrations: skipped (--no-build)"));
    } else {
        eprintln!("Building the app to ask it about its database (skip with --no-build)…");
        // `serve::build` runs cargo in the current directory.
        let built = std::env::set_current_dir(root)
            .map_err(anyhow::Error::from)
            .and_then(|()| crate::serve::build(&[]));
        match built {
            Ok(Some(exe)) => match std::process::Command::new(&exe)
                .arg("migrate:status")
                .current_dir(root)
                .output()
            {
                Ok(out) => checks.extend(status_checks(
                    out.status.success(),
                    &String::from_utf8_lossy(&out.stdout),
                    &String::from_utf8_lossy(&out.stderr),
                    scheme(crate::setting_in(root, "DATABASE_URL").as_deref()),
                )),
                Err(e) => checks.push(Check::fail(
                    format!("Database: couldn't run the app ({e})"),
                    "cargo build",
                )),
            },
            Ok(None) | Err(_) => {
                checks.push(Check::fail("The app doesn't build", "cargo build"));
            }
        }
    }
    checks
}

/// What `<app> migrate:status` says about the database and the migrations.
fn status_checks(success: bool, stdout: &str, stderr: &str, scheme: &str) -> Vec<Check> {
    if !success {
        let mut lines: Vec<&str> = stderr
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.trim().is_empty())
            .take(5)
            .collect();
        lines.push("check DATABASE_URL in .env (PostgreSQL: is the server running, and the database created?)");
        return vec![Check::fail(
            "Database: the app couldn't start",
            lines.join("\n"),
        )];
    }
    let pending = stdout
        .lines()
        .filter(|l| l.trim_start().starts_with("pending"))
        .count();
    let mut checks = vec![Check::ok(format!("Database: reachable ({scheme})"))];
    checks.push(if pending == 0 {
        Check::ok("Migrations: all ran")
    } else {
        Check::warn(format!("Migrations: {pending} pending"), "rnx migrate")
    });
    checks
}

/// The kind of database a `DATABASE_URL` points at.
fn scheme(url: Option<&str>) -> &'static str {
    match url {
        Some(u) if u.starts_with("postgres://") || u.starts_with("postgresql://") => "postgres",
        Some(u) if !u.starts_with("sqlite:") => "other",
        _ => "sqlite",
    }
}

/// `rnx doctor`.
pub fn run(no_build: bool) -> Result<()> {
    let app = crate::app_root_in(std::env::current_dir()?).ok();
    let texts = tools::configured_texts(app.as_deref());
    let version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
    let configured = texts.iter().find_map(|t| tools::mentions_linker(t));
    let wrapper = std::env::var("RUSTC_WRAPPER")
        .map(|w| w.contains("sccache"))
        .unwrap_or(false)
        || texts.iter().any(|t| t.contains("sccache"));

    let machine = vec![
        rust_check(version.as_deref(), env!("CARGO_PKG_RUST_VERSION")),
        linker_check(
            std::env::consts::OS,
            std::env::consts::ARCH,
            cfg!(target_env = "gnu"),
            configured,
            |name| tools::find(name).is_some(),
        ),
        sccache_check(wrapper, tools::find("sccache").is_some()),
    ];
    let project = match &app {
        None => vec![Check::skip(
            "not in an app directory: run rnx doctor in one to check it too",
        )],
        Some(app) => project_checks(app, no_build),
    };
    let sections = vec![
        ("Machine".to_string(), machine),
        (
            format!(
                "App ({})",
                app.as_deref()
                    .map_or_else(String::new, |a| a.display().to_string())
            ),
            project,
        ),
    ];
    print!("{}", render(&sections));
    if exit_code(&sections) == 1 {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_check_compares_versions() {
        let ok = rust_check(Some("rustc 1.94.0 (abc 2026-01-01)"), "1.94");
        assert_eq!(ok.status, Status::Ok);
        assert!(ok.what.contains("Rust 1.94.0") && ok.what.contains("1.94 or newer"));
        assert_eq!(
            rust_check(Some("rustc 1.95.1 (abc 2026-01-01)"), "1.94").status,
            Status::Ok
        );
        let old = rust_check(Some("rustc 1.90.0 (abc 2026-01-01)"), "1.94");
        assert_eq!(old.status, Status::Fail);
        assert_eq!(old.fix.as_deref(), Some("rustup update stable"));
        let none = rust_check(None, "1.94");
        assert_eq!(none.status, Status::Fail);
        assert_eq!(none.what, "Rust: rustc not found");
        assert_eq!(rust_check(Some("hello"), "1.94").status, Status::Fail);
        assert_eq!(rust_check(Some("rustc x.y"), "1.94").status, Status::Fail);
    }

    #[test]
    fn linker_check_per_platform() {
        let none = |_: &str| false;
        let all = |_: &str| true;
        let c = linker_check("linux", "x86_64", true, Some(tools::Linker::Mold), none);
        assert_eq!(c.status, Status::Ok);
        assert!(c.what.contains("mold (set in a Cargo config)"));
        let c = linker_check("linux", "x86_64", true, None, none);
        assert_eq!(c.status, Status::Ok);
        assert!(c.what.contains("rust-lld"));
        let c = linker_check("linux", "aarch64", true, None, all);
        assert_eq!(c.status, Status::Warn);
        assert!(c.what.contains("mold is installed but not used"));
        assert!(
            c.fix
                .unwrap()
                .contains("[target.aarch64-unknown-linux-gnu]")
        );
        let c = linker_check("linux", "aarch64", true, None, none);
        assert_eq!(c.status, Status::Warn);
        assert!(c.what.contains("system default"));
        assert_eq!(
            linker_check("macos", "aarch64", false, None, none).status,
            Status::Ok
        );
        let c = linker_check("windows", "x86_64", false, None, none);
        assert_eq!(c.status, Status::Warn);
        assert!(c.fix.unwrap().contains("rust-lld.exe"));
        assert_eq!(
            linker_check("freebsd", "x86_64", false, None, none).status,
            Status::Skip
        );
    }

    #[test]
    fn sccache_check_three_ways() {
        assert_eq!(sccache_check(true, true).status, Status::Ok);
        let c = sccache_check(false, true);
        assert_eq!(c.status, Status::Warn);
        assert!(c.fix.unwrap().contains("RUSTC_WRAPPER=sccache"));
        let c = sccache_check(false, false);
        assert!(c.what.contains("not installed"));
        assert_eq!(c.fix.as_deref(), Some("cargo install sccache --locked"));
    }

    #[test]
    fn render_marks_fixes_and_summaries() {
        let good = vec![("Machine".to_string(), vec![Check::ok("fine")])];
        let text = render(&good);
        assert!(text.contains("Machine\n  ✓ fine\n"));
        assert!(text.ends_with("Everything looks good.\n"));
        let mixed = vec![(
            "Machine".to_string(),
            vec![
                Check::fail("bad", "line one\nline two"),
                Check::warn("meh", "do this"),
                Check::skip("later"),
            ],
        )];
        let text = render(&mixed);
        assert!(text.contains("  ✗ bad\n      line one\n      line two\n"));
        assert!(text.contains("  ! meh\n      do this\n"));
        assert!(text.contains("  - later\n"));
        assert!(text.ends_with("1 problem(s), 1 suggestion(s).\n"));
    }

    #[test]
    fn exit_code_follows_failures() {
        let warn = vec![("A".to_string(), vec![Check::warn("w", "f")])];
        assert_eq!(exit_code(&warn), 0);
        let fail = vec![
            ("A".to_string(), vec![Check::ok("o")]),
            ("B".to_string(), vec![Check::fail("x", "y")]),
        ];
        assert_eq!(exit_code(&fail), 1);
    }

    #[test]
    fn env_check_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let c = env_check(dir.path());
        assert_eq!(c.status, Status::Fail);
        assert_eq!(
            c.fix.as_deref(),
            Some("cp .env.example .env && rnx key:generate")
        );
        std::fs::write(dir.path().join(".env"), "").unwrap();
        assert_eq!(env_check(dir.path()).status, Status::Ok);
    }

    #[test]
    fn key_check_cases() {
        assert_eq!(key_check(None).what, "APP_KEY: not set");
        assert_eq!(
            key_check(Some("base64:!!")).what,
            "APP_KEY: not valid base64"
        );
        let short = format!("base64:{}", STANDARD.encode([0u8; 16]));
        assert_eq!(
            key_check(Some(&short)).what,
            "APP_KEY: shorter than 32 bytes"
        );
        assert_eq!(key_check(Some(&"a".repeat(32))).status, Status::Ok);
        assert_eq!(key_check(Some("tiny")).status, Status::Fail);
        assert_eq!(key_check(Some(&crate::generate_key())).status, Status::Ok);
    }

    #[test]
    fn build_rs_check_cases() {
        assert_eq!(
            build_rs_check(Some(include_str!("../stubs/build.rs"))).status,
            Status::Ok
        );
        assert_eq!(build_rs_check(Some("")).status, Status::Fail);
        assert_eq!(build_rs_check(None).status, Status::Fail);
        assert_eq!(
            build_rs_check(Some("cargo::rerun-if-changed=migrations")).status,
            Status::Ok
        );
    }

    #[test]
    fn tailwind_check_branches() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tw");
        std::fs::write(&file, "").unwrap();
        let file_str = file.to_str().unwrap();
        assert_eq!(
            tailwind_check(Some(file_str), None).what,
            "Tailwind: TAILWIND_BIN"
        );
        let c = tailwind_check(Some("/no/such/tw"), Some(&file));
        assert_eq!(c.status, Status::Fail);
        assert!(c.what.contains("/no/such/tw is not a file"));
        let c = tailwind_check(None, Some(&file));
        assert_eq!(c.status, Status::Ok);
        assert_eq!(c.what, format!("Tailwind {}", tailwind::VERSION));
        let c = tailwind_check(None, None);
        assert_eq!(c.status, Status::Warn);
        assert_eq!(c.fix.as_deref(), Some("rnx tailwind:install"));
    }

    #[test]
    fn status_checks_cases() {
        let c = status_checks(true, "  ran (batch 1)  a\n", "", "sqlite");
        assert_eq!(c[0].what, "Database: reachable (sqlite)");
        assert_eq!(c[1].what, "Migrations: all ran");
        let out = "  ran (batch 1)  a\n  pending          b\n  pending          c\n";
        let c = status_checks(true, out, "", "postgres");
        assert_eq!(c[1].status, Status::Warn);
        assert_eq!(c[1].what, "Migrations: 2 pending");
        assert_eq!(c[1].fix.as_deref(), Some("rnx migrate"));
        let err = (1..=7).map(|n| format!("e{n}\n")).collect::<String>();
        let c = status_checks(false, "", &err, "sqlite");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].status, Status::Fail);
        let fix = c[0].fix.as_deref().unwrap();
        assert!(fix.contains("e5") && !fix.contains("e6"));
        assert!(fix.ends_with("the database created?)"));
    }

    #[test]
    fn scheme_cases() {
        assert_eq!(scheme(Some("postgres://x")), "postgres");
        assert_eq!(scheme(Some("postgresql://x")), "postgres");
        assert_eq!(scheme(Some("sqlite://a.db")), "sqlite");
        assert_eq!(scheme(None), "sqlite");
        assert_eq!(scheme(Some("mysql://x")), "other");
    }
}
