//! `rnx build` and `rnx make:deploy`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use heck::ToTitleCase;

/// The package name from `Cargo.toml`.
fn package_name(root: &Path) -> Result<String> {
    let manifest = fs::read_to_string(root.join("Cargo.toml")).context("no Cargo.toml here")?;
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package
            && let Some(value) = line.strip_prefix("name").map(str::trim_start)
            && let Some(value) = value.strip_prefix('=')
        {
            return Ok(value.trim().trim_matches('"').to_owned());
        }
    }
    bail!("Cargo.toml has no [package] name")
}

/// `rnx build`: a release build copied to `dist/<name>`.
pub fn build(root: &Path) -> Result<()> {
    // Embedded apps carry public/ in the binary, so the CSS comes first.
    if crate::tailwind::enabled(root) {
        crate::tailwind::build(root, true)?;
        println!("Built {} (minified).", crate::tailwind::OUTPUT);
    }
    let Some(exe) = crate::serve::build(&["--release".to_owned()])? else {
        bail!("the release build failed");
    };
    let name = exe.file_name().context("the build produced no file")?;
    let dist = root.join("dist");
    fs::create_dir_all(&dist)?;
    let target: PathBuf = dist.join(name);
    fs::copy(&exe, &target).with_context(|| format!("could not copy to {}", target.display()))?;
    let size = fs::metadata(&target)?.len() as f64 / 1024.0 / 1024.0;
    println!(
        "Built {} ({size:.1} MB). Deploy it with a .env file; views, translations and public\n\
         files are inside if the app calls .embed(renox::embedded!()). See `rnx make:deploy`.",
        Path::new("dist").join(name).display()
    );
    Ok(())
}

const FILES: &[(&str, &str)] = &[
    (
        "Dockerfile",
        include_str!("../stubs/deploy/Dockerfile.stub"),
    ),
    (
        ".dockerignore",
        include_str!("../stubs/deploy/dockerignore.stub"),
    ),
    (
        "deploy/{{name}}.service",
        include_str!("../stubs/deploy/service.stub"),
    ),
    (
        "deploy/{{name}}.socket",
        include_str!("../stubs/deploy/socket.stub"),
    ),
    (
        "deploy/litestream.yml",
        include_str!("../stubs/deploy/litestream.stub"),
    ),
    (
        "deploy/README.md",
        include_str!("../stubs/deploy/README.stub"),
    ),
];

/// `rnx make:deploy`: Dockerfile, systemd unit, Litestream config and a guide.
/// Existing files are left alone.
pub fn make_deploy(root: &Path) -> Result<()> {
    let name = package_name(root)?;
    let title = name.to_title_case();
    for (path, contents) in FILES {
        let path = root.join(path.replace("{{name}}", &name));
        let shown = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if path.exists() {
            println!("Kept {shown} (already exists)");
            continue;
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(
            &path,
            contents
                .replace("{{name}}", &name)
                .replace("{{title}}", &title),
        )?;
        println!("Created {shown}");
    }
    println!("Next: read deploy/README.md.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_deploy_files_once() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"coffee-shop\"\nversion = \"0.1.0\"\n\n[dependencies]\nname = \"not this\"\n",
        )
        .unwrap();
        make_deploy(dir.path()).unwrap();
        let docker = fs::read_to_string(dir.path().join("Dockerfile")).unwrap();
        assert!(docker.contains("cp target/release/coffee-shop /coffee-shop"));
        assert!(
            docker.contains("cargo chef cook --release"),
            "dependencies in their own layer"
        );
        let ignore = fs::read_to_string(dir.path().join(".dockerignore")).unwrap();
        assert!(ignore.lines().any(|l| l == ".cargo"), "{ignore}");
        let unit = fs::read_to_string(dir.path().join("deploy/coffee-shop.service")).unwrap();
        assert!(
            unit.contains("Description=Coffee Shop")
                && unit.contains("ExecStartPre=/opt/coffee-shop/coffee-shop migrate")
        );
        let socket = fs::read_to_string(dir.path().join("deploy/coffee-shop.socket")).unwrap();
        assert!(
            socket.contains("Description=Coffee Shop (listening socket)")
                && socket.contains("ListenStream=127.0.0.1:3000"),
            "{socket}"
        );
        assert!(
            fs::read_to_string(dir.path().join("deploy/litestream.yml"))
                .unwrap()
                .contains("/opt/coffee-shop/storage/app.db")
        );

        fs::write(dir.path().join("Dockerfile"), "custom").unwrap();
        make_deploy(dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("Dockerfile")).unwrap(),
            "custom",
            "kept"
        );
    }

    #[test]
    fn the_package_name_comes_from_the_package_section_only() {
        let dir = tempfile::tempdir().unwrap();
        let err = package_name(dir.path()).unwrap_err();
        assert!(err.to_string().contains("no Cargo.toml here"), "{err}");
        // A workspace manifest: a `name` outside [package] doesn't count.
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\"]\n\n[workspace.package]\nname = \"no\"\n",
        )
        .unwrap();
        let err = make_deploy(dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("Cargo.toml has no [package] name"),
            "{err}"
        );
        assert!(!dir.path().join("Dockerfile").exists());
    }
}
