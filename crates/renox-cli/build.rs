//! Records the Renox commit this `rnx` was built from, so `rnx new` can pin
//! new apps to it (`cargo install --git` builds from a git checkout).

use std::process::Command;

fn main() {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|s| s.trim().to_owned())
    };
    let rev = git(&["rev-parse", "HEAD"])
        .filter(|rev| rev.len() == 40)
        .or_else(checkout_rev);
    if let Some(rev) = rev {
        println!("cargo:rustc-env=RENOX_GIT_REV={rev}");
    } else if from_registry() {
        // `cargo install renox-cli`: new apps depend on this release.
        println!("cargo:rustc-env=RENOX_FROM_CRATES_IO=1");
    }
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/refs/heads");
    }
    println!("cargo:rerun-if-changed=build.rs");
}

/// `cargo install renox-cli` builds in `~/.cargo/registry/src/<index>/renox-cli-<version>/`.
fn from_registry() -> bool {
    std::env::var("CARGO_MANIFEST_DIR").is_ok_and(|dir| {
        let dir = dir.replace('\\', "/");
        dir.contains("/registry/src/")
    })
}

/// `cargo install --git` builds in `~/.cargo/git/checkouts/<repo>/<short rev>/`,
/// which has no `.git`; the directory's name is the commit.
fn checkout_rev() -> Option<String> {
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    let root = manifest.parent()?.parent()?; // crates/renox-cli -> the checkout
    let rev = root.file_name()?.to_str()?;
    let in_checkouts = root.parent()?.parent()?.file_name()? == "checkouts";
    (in_checkouts && rev.len() >= 7 && rev.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| rev.to_owned())
}
