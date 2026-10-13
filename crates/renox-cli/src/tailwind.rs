//! Tailwind CSS through its standalone CLI, so apps need no Node.
//!
//! An app uses Tailwind when `resources/css/app.css` exists (`rnx new
//! --tailwind` writes it). `rnx serve` then runs Tailwind in watch mode next
//! to the app, `rnx build` runs it minified before compiling, and the result
//! is `public/css/app.css`, which the layout links with `asset('css/app.css')`.
//!
//! The binary is downloaded once per version into the user's cache and checked
//! against the SHA-256 sums pinned below. `TAILWIND_BIN` points at another one.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

pub const VERSION: &str = "4.3.3";

/// Release assets and their SHA-256, from the release's `sha256sums.txt`.
const SUMS: &[(&str, &str)] = &[
    (
        "tailwindcss-linux-arm64",
        "55fd0b241214eff3de1e8ee4f22796662f2d2e7a49bcfca7477cfd0bac398195",
    ),
    (
        "tailwindcss-linux-arm64-musl",
        "71ea4be79c9de9827545682df3e040053fb535d37c71ed2cfdedf9385a0868e0",
    ),
    (
        "tailwindcss-linux-x64",
        "dc61b3ac6b8c9ca874c0cc4c57b2409791a64c5540404ca5f5367360babc313a",
    ),
    (
        "tailwindcss-linux-x64-musl",
        "a04d34ceacc8f52cbe8920ad846cdeb61d3d0021dba32db0d1f77c9d9fad7a6c",
    ),
    (
        "tailwindcss-macos-arm64",
        "cdf646702987a743464dff4d9c60fd4480d1c1e73dd819a9a67f1078815dce9d",
    ),
    (
        "tailwindcss-macos-x64",
        "7922e0953f2110c05976e3bf58f14e643d90427575e766b7d433f5f80cbee7e1",
    ),
    (
        "tailwindcss-windows-x64.exe",
        "e0e260ce048014e9268f6237ff18f8ccf02cef521cbd0ae04e82c2cdf7aa3955",
    ),
];

pub const INPUT: &str = "resources/css/app.css";
pub const OUTPUT: &str = "public/css/app.css";

/// The input `rnx new --tailwind` writes.
pub const INPUT_STUB: &str = r#"/* Tailwind CSS (https://tailwindcss.com/docs), built by `rnx serve` and `rnx build`
   into public/css/app.css with the standalone CLI: no Node needed.
   Classes are found in the views; add other places with more @source lines. */
@import "tailwindcss";
@source "../views";

/* The UI kit (rx-* classes) sits outside Tailwind's layers, so its components keep their
   look next to utilities. Rebrand it with its tokens, e.g. :root { --rx-accent: #0a7d5a; } */
"#;

/// Whether the app in `root` uses Tailwind.
pub fn enabled(root: &Path) -> bool {
    root.join(INPUT).is_file()
}

/// The release asset for this machine.
fn asset() -> Result<&'static str> {
    asset_for(
        env::consts::OS,
        env::consts::ARCH,
        cfg!(target_env = "musl"),
    )
}

/// The release asset for an OS, architecture and C library.
fn asset_for(os: &str, arch: &str, musl: bool) -> Result<&'static str> {
    Ok(match (os, arch) {
        ("linux", "x86_64") if musl => "tailwindcss-linux-x64-musl",
        ("linux", "x86_64") => "tailwindcss-linux-x64",
        ("linux", "aarch64") if musl => "tailwindcss-linux-arm64-musl",
        ("linux", "aarch64") => "tailwindcss-linux-arm64",
        ("macos", "x86_64") => "tailwindcss-macos-x64",
        ("macos", "aarch64") => "tailwindcss-macos-arm64",
        ("windows", "x86_64") => "tailwindcss-windows-x64.exe",
        (os, arch) => bail!(
            "Tailwind has no standalone CLI for {os}/{arch}; install one and set TAILWIND_BIN"
        ),
    })
}

/// Where downloaded tools live: `RNX_CACHE_DIR`, else the platform's cache.
fn cache_dir() -> Result<PathBuf> {
    cache_dir_with(|name| env::var_os(name))
}

/// [`cache_dir`] with the environment read through `var` (for tests, which
/// mustn't set variables: they run in parallel).
fn cache_dir_with(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<PathBuf> {
    if let Some(dir) = var("RNX_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        var("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
    } else {
        var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".cache")))
    };
    Ok(base
        .context("no cache directory (set RNX_CACHE_DIR)")?
        .join("renox"))
}

/// The Tailwind binary: `TAILWIND_BIN`, else the pinned version, downloaded
/// on first use.
pub fn binary() -> Result<PathBuf> {
    if let Some(bin) = env::var_os("TAILWIND_BIN").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(bin));
    }
    let path = pinned_path()?;
    if !path.is_file() {
        download(asset()?, &path)?;
    }
    Ok(path)
}

/// Where the pinned Tailwind binary is (or will be) kept.
pub fn pinned_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join(format!("{VERSION}-{}", asset()?)))
}

/// The pinned SHA-256 of `asset`.
fn expected_sum(asset: &str) -> Result<&'static str> {
    SUMS.iter()
        .find(|(name, _)| *name == asset)
        .map(|(_, sum)| *sum)
        .context("no checksum for this platform")
}

/// Whether `bytes` are the pinned `asset`; an error naming both sums if not.
fn check_sum(asset: &str, bytes: &[u8]) -> Result<()> {
    let expected = expected_sum(asset)?;
    let actual = hex(&Sha256::digest(bytes));
    if actual != expected {
        bail!("{asset} has SHA-256 {actual}, expected {expected}; not using it");
    }
    Ok(())
}

/// Downloads `asset` with the system's `curl` and checks its SHA-256.
fn download(asset: &str, to: &Path) -> Result<()> {
    expected_sum(asset)?;
    let url =
        format!("https://github.com/tailwindlabs/tailwindcss/releases/download/v{VERSION}/{asset}");
    let dir = to.parent().context("the cache path has a parent")?;
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let partial = to.with_extension("download");
    eprintln!("rnx: downloading Tailwind CSS {VERSION} ({asset})…");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--retry",
            "2",
        ])
        .arg("--output")
        .arg(&partial)
        .arg(&url)
        .status()
        .context("could not run curl; install it, or download Tailwind and set TAILWIND_BIN")?;
    if !status.success() {
        let _ = fs::remove_file(&partial);
        bail!("could not download {url}");
    }
    let bytes = fs::read(&partial)?;
    if let Err(err) = check_sum(asset, &bytes) {
        let _ = fs::remove_file(&partial);
        return Err(err);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&partial, to)?;
    eprintln!("rnx: saved {}", to.display());
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn command(root: &Path) -> Result<Command> {
    let mut command = Command::new(binary()?);
    command
        .current_dir(root)
        .args(["--input", INPUT, "--output", OUTPUT]);
    Ok(command)
}

/// Builds `public/css/app.css` once.
pub fn build(root: &Path, minify: bool) -> Result<()> {
    let mut command = command(root)?;
    if minify {
        command.arg("--minify");
    }
    let output = command.output().context("could not run Tailwind")?;
    if !output.status.success() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        bail!("Tailwind failed");
    }
    Ok(())
}

/// Starts Tailwind in watch mode; it rebuilds when a view or the input changes.
pub fn watch(root: &Path) -> Result<Child> {
    command(root)?
        // `always`: keep watching although stdin isn't a terminal.
        .arg("--watch=always")
        .stdin(Stdio::null())
        .spawn()
        .context("could not start Tailwind")
}

/// `rnx tailwind`: one build (`--minify`), or `--watch`.
pub fn run(root: &Path, watching: bool, minify: bool) -> Result<()> {
    if !enabled(root) {
        bail!("no {INPUT} here: this app doesn't use Tailwind (see `rnx new --tailwind`)");
    }
    if watching {
        let mut child = watch(root)?;
        child.wait()?;
        return Ok(());
    }
    build(root, minify)?;
    println!("Built {OUTPUT}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_has_a_pinned_sum() {
        assert!(
            SUMS.iter()
                .any(|(name, _)| Ok(*name) == asset().as_deref().map_err(|_| ()))
        );
        for (_, sum) in SUMS {
            assert_eq!(sum.len(), 64);
            assert!(sum.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert_eq!(hex(&[0, 15, 255]), "000fff");
    }

    #[test]
    fn apps_with_an_input_file_use_tailwind() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!enabled(dir.path()));
        let input = dir.path().join(INPUT);
        fs::create_dir_all(input.parent().unwrap()).unwrap();
        fs::write(&input, INPUT_STUB).unwrap();
        assert!(enabled(dir.path()));
    }

    // #248: the platform mapping, the cache directory and the checksum,
    // without the network.

    #[test]
    fn each_platform_maps_to_its_asset() {
        let cases = [
            ("linux", "x86_64", false, "tailwindcss-linux-x64"),
            ("linux", "x86_64", true, "tailwindcss-linux-x64-musl"),
            ("linux", "aarch64", false, "tailwindcss-linux-arm64"),
            ("linux", "aarch64", true, "tailwindcss-linux-arm64-musl"),
            ("macos", "x86_64", false, "tailwindcss-macos-x64"),
            ("macos", "aarch64", false, "tailwindcss-macos-arm64"),
            ("windows", "x86_64", false, "tailwindcss-windows-x64.exe"),
        ];
        for (os, arch, musl, asset) in cases {
            assert_eq!(asset_for(os, arch, musl).unwrap(), asset, "{os}/{arch}");
            assert!(expected_sum(asset).is_ok(), "{asset} has a pinned sum");
        }
        let err = asset_for("freebsd", "x86_64", false).unwrap_err();
        assert!(err.to_string().contains("set TAILWIND_BIN"), "{err}");
        assert!(
            asset().is_ok() || cfg!(not(any(target_os = "linux", target_os = "macos", windows)))
        );
    }

    #[test]
    fn the_cache_directory_comes_from_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| std::ffi::OsString::from(*v))
            }
        };
        assert_eq!(
            cache_dir_with(env(&[("RNX_CACHE_DIR", "/tmp/rnx"), ("HOME", "/home/a")])).unwrap(),
            PathBuf::from("/tmp/rnx")
        );
        assert!(cache_dir_with(env(&[])).is_err(), "nowhere to put it");
        if cfg!(target_os = "linux") {
            assert_eq!(
                cache_dir_with(env(&[("XDG_CACHE_HOME", "/c"), ("HOME", "/home/a")])).unwrap(),
                PathBuf::from("/c/renox")
            );
            assert_eq!(
                cache_dir_with(env(&[("HOME", "/home/a")])).unwrap(),
                PathBuf::from("/home/a/.cache/renox")
            );
        }
    }

    #[test]
    fn a_download_with_another_checksum_is_refused() {
        let err = check_sum("tailwindcss-linux-x64", b"not tailwind").unwrap_err();
        let shown = err.to_string();
        assert!(
            shown.contains("has SHA-256") && shown.contains("not using it"),
            "{shown}"
        );
        assert!(
            check_sum("tailwindcss-amiga", b"").is_err(),
            "no sum for it"
        );
    }
}
