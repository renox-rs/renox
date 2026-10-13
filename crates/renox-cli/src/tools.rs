//! Programs on this machine and the fast linker `rnx new` can pick.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The first file called `name` in the directories of `path` (with `.exe`
/// tried too when `windows`).
pub(crate) fn find_in(name: &str, path: Option<&OsStr>, windows: bool) -> Option<PathBuf> {
    for dir in std::env::split_paths(path?) {
        let plain = dir.join(name);
        if plain.is_file() {
            return Some(plain);
        }
        if windows {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// [`find_in`] on this process's `PATH`.
pub(crate) fn find(name: &str) -> Option<PathBuf> {
    find_in(name, std::env::var_os("PATH").as_deref(), cfg!(windows))
}

/// A linker faster than the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Linker {
    /// mold.
    Mold,
    /// LLVM's lld.
    Lld,
}

impl Linker {
    /// The name `-fuse-ld=` takes.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Linker::Mold => "mold",
            Linker::Lld => "lld",
        }
    }
}

/// The Rust target for a Linux CPU, if Renox sets up a linker for it.
pub(crate) fn linux_target(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" => Some("x86_64-unknown-linux-gnu"),
        "aarch64" => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

/// The fast linker to use here, if the tools for it are installed (`has`).
pub(crate) fn fast_linker(
    os: &str,
    arch: &str,
    gnu: bool,
    has: impl Fn(&str) -> bool,
) -> Option<Linker> {
    if !(os == "linux" && gnu && linux_target(arch).is_some()) {
        return None;
    }
    if has("clang") && has("mold") {
        Some(Linker::Mold)
    } else if arch == "aarch64" && has("clang") && has("ld.lld") {
        Some(Linker::Lld)
    } else {
        None
    }
}

/// The text of `.cargo/config.toml` that links with `linker` for `target`.
pub(crate) fn linker_config(linker: Linker, target: &str) -> String {
    let linker = linker.name();
    format!(
        "# Written by `rnx new`: link with {linker}, found on this machine, for faster\n\
         # rebuilds (docs/development.md). It depends on this machine, so .gitignore\n\
         # keeps it out of Git; delete it to use Rust's default linker.\n\
         [target.{target}]\n\
         linker = \"clang\"\n\
         rustflags = [\"-C\", \"link-arg=-fuse-ld={linker}\"]\n"
    )
}

/// The fast linker a piece of configuration already names.
pub(crate) fn mentions_linker(text: &str) -> Option<Linker> {
    if text.contains("mold") {
        Some(Linker::Mold)
    } else if text.contains("lld") {
        Some(Linker::Lld)
    } else {
        None
    }
}

/// Cargo's home: `CARGO_HOME`, else `~/.cargo`, with the environment read
/// through `var`.
pub(crate) fn cargo_home_with(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(dir) = var("CARGO_HOME") {
        return Some(PathBuf::from(dir));
    }
    let home = if cfg!(windows) {
        var("USERPROFILE")
    } else {
        var("HOME")
    };
    home.map(|home| PathBuf::from(home).join(".cargo"))
}

/// The configuration that may already choose a linker: the app's and cargo
/// home's `config.toml`, and the rustflags variables. Missing ones are skipped.
pub(crate) fn configured_texts(app: Option<&Path>) -> Vec<String> {
    let mut texts = Vec::new();
    if let Some(app) = app
        && let Ok(text) = std::fs::read_to_string(app.join(".cargo/config.toml"))
    {
        texts.push(text);
    }
    if let Some(home) = cargo_home_with(|name| std::env::var_os(name))
        && let Ok(text) = std::fs::read_to_string(home.join("config.toml"))
    {
        texts.push(text);
    }
    for name in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"] {
        if let Some(value) = std::env::var_os(name) {
            texts.push(value.to_string_lossy().into_owned());
        }
    }
    texts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_in_uses_the_second_dir_and_skips_directories() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::create_dir(a.path().join("tool")).unwrap();
        std::fs::write(b.path().join("tool"), "x").unwrap();
        let path = std::env::join_paths([a.path(), b.path()]).unwrap();
        assert_eq!(
            find_in("tool", Some(&path), false),
            Some(b.path().join("tool"))
        );
        assert_eq!(find_in("none", Some(&path), false), None);
        assert_eq!(find_in("tool", None, false), None);
    }

    #[test]
    fn find_in_tries_exe_only_on_windows() {
        let a = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("x.exe"), "x").unwrap();
        let path = std::env::join_paths([a.path()]).unwrap();
        assert_eq!(find_in("x", Some(&path), false), None);
        assert_eq!(
            find_in("x", Some(&path), true),
            Some(a.path().join("x.exe"))
        );
    }

    #[test]
    fn linux_targets() {
        assert_eq!(linux_target("x86_64"), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(linux_target("aarch64"), Some("aarch64-unknown-linux-gnu"));
        assert_eq!(linux_target("riscv64"), None);
    }

    #[test]
    fn fast_linker_choice() {
        let all = |_: &str| true;
        assert_eq!(
            fast_linker("linux", "x86_64", true, all),
            Some(Linker::Mold)
        );
        assert_eq!(
            fast_linker("linux", "aarch64", true, all),
            Some(Linker::Mold)
        );
        let lld_only = |n: &str| n == "clang" || n == "ld.lld";
        assert_eq!(fast_linker("linux", "x86_64", true, lld_only), None);
        assert_eq!(
            fast_linker("linux", "aarch64", true, lld_only),
            Some(Linker::Lld)
        );
        assert_eq!(fast_linker("macos", "aarch64", true, all), None);
        assert_eq!(fast_linker("windows", "x86_64", true, all), None);
        assert_eq!(fast_linker("linux", "x86_64", false, all), None);
        assert_eq!(fast_linker("linux", "riscv64", true, all), None);
        assert_eq!(fast_linker("linux", "x86_64", true, |n| n == "mold"), None);
    }

    #[test]
    fn linker_config_text() {
        assert_eq!(
            linker_config(Linker::Mold, "x86_64-unknown-linux-gnu"),
            "# Written by `rnx new`: link with mold, found on this machine, for faster\n\
             # rebuilds (docs/development.md). It depends on this machine, so .gitignore\n\
             # keeps it out of Git; delete it to use Rust's default linker.\n\
             [target.x86_64-unknown-linux-gnu]\n\
             linker = \"clang\"\n\
             rustflags = [\"-C\", \"link-arg=-fuse-ld=mold\"]\n"
        );
        let lld = linker_config(Linker::Lld, "aarch64-unknown-linux-gnu");
        assert!(lld.contains("link with lld,"));
        assert!(lld.contains("[target.aarch64-unknown-linux-gnu]"));
        assert!(lld.ends_with("link-arg=-fuse-ld=lld\"]\n"));
    }

    #[test]
    fn mentions() {
        assert_eq!(mentions_linker("-fuse-ld=mold"), Some(Linker::Mold));
        assert_eq!(mentions_linker("-fuse-ld=lld"), Some(Linker::Lld));
        assert_eq!(mentions_linker("nothing"), None);
    }

    #[test]
    fn cargo_home() {
        let var = |vars: &'static [(&'static str, &'static str)]| {
            move |n: &str| {
                vars.iter()
                    .find(|(k, _)| *k == n)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        assert_eq!(
            cargo_home_with(var(&[("CARGO_HOME", "/c"), ("HOME", "/h")])),
            Some(PathBuf::from("/c"))
        );
        let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let vars: &'static [(&str, &str)] = Box::leak(Box::new([(key, "/h")]));
        assert_eq!(
            cargo_home_with(var(vars)),
            Some(PathBuf::from("/h").join(".cargo"))
        );
        assert_eq!(cargo_home_with(|_| None), None);
    }
}
