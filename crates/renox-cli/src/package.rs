//! `rnx make:package`: a reusable crate with a `Module`, joined to the app's
//! workspace. This file holds the plumbing: the name, the `--with` extras,
//! the workspace `members` edit and the `renox` dependency the package uses.

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};
use heck::ToUpperCamelCase;

/// What `--with` can add to a package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Extra {
    /// A `public/` folder registered with `Registry::asset`.
    Assets,
    /// A command the app can run.
    Commands,
}

/// Parse the `--with` values. `admin` needs `Registry::contribute`, which
/// this Renox doesn't have yet, so it is an error; so is anything unknown.
pub(crate) fn parse_with(values: &[String]) -> Result<Vec<Extra>> {
    let mut extras = Vec::new();
    for value in values {
        let extra = match value.trim() {
            "assets" => Extra::Assets,
            "commands" => Extra::Commands,
            "admin" => bail!("`--with admin` needs a newer renox (Registry::contribute)"),
            other => bail!("unknown --with value `{other}`: use `assets` or `commands`"),
        };
        if !extras.contains(&extra) {
            extras.push(extra);
        }
    }
    Ok(extras)
}

/// What the generator works out before it writes anything.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The crate's name as given (`-` kept).
    pub crate_name: String,
    /// The library name (`-` become `_`).
    pub lib_name: String,
    /// The module struct, e.g. `Invoicing`.
    pub struct_name: String,
    /// The package directory, relative to the workspace root.
    pub path: PathBuf,
    pub extras: Vec<Extra>,
    /// The `renox = …` line for the package's `Cargo.toml`.
    pub renox_dependency: String,
    /// The workspace `Cargo.toml` after adding the package to `members`.
    pub manifest: String,
    /// The crate whose `src/lib.rs` gets `.module(..)`: `None` for a
    /// virtual workspace without `--register`.
    pub register: Option<PathBuf>,
}

/// Work out everything for `rnx make:package` in `root` without writing.
/// Nothing is changed when this fails.
pub(crate) fn plan(
    root: &Path,
    name: &str,
    path: Option<PathBuf>,
    with: &[String],
    register: Option<PathBuf>,
) -> Result<Plan> {
    crate::new::validate_name(name)?;
    let extras = parse_with(with)?;
    let manifest_path = root.join("Cargo.toml");
    if !manifest_path.is_file() {
        bail!("run this from your app's directory (the one with Cargo.toml)");
    }
    let manifest = std::fs::read_to_string(&manifest_path)?;
    let path = path.unwrap_or_else(|| Path::new("crates").join(name));
    if path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        bail!("--path must be inside the workspace, e.g. `crates/{name}`");
    }
    if root.join(&path).exists() {
        bail!("`{}` already exists", path.display());
    }
    let member = path.to_string_lossy().replace('\\', "/");
    let is_virtual = !has_package(&manifest);
    let register = match (is_virtual, register) {
        (true, register) => register,
        (false, Some(_)) => bail!("--register is only for a virtual workspace"),
        (false, None) => Some(PathBuf::from(".")),
    };
    Ok(Plan {
        crate_name: name.to_owned(),
        lib_name: name.replace('-', "_"),
        struct_name: name.replace('-', "_").to_upper_camel_case(),
        renox_dependency: renox_dependency(&manifest, &path),
        manifest: add_member(&manifest, &member)?,
        path,
        extras,
        register,
    })
}

/// `rnx make:package`. The files themselves come with the next steps.
pub(crate) fn run(
    root: &Path,
    name: &str,
    path: Option<PathBuf>,
    with: &[String],
    register: Option<PathBuf>,
) -> Result<()> {
    let _plan = plan(root, name, path, with, register)?;
    bail!("make:package doesn't write the package yet")
}

fn has_package(manifest: &str) -> bool {
    manifest.lines().any(|line| line.trim() == "[package]")
}

/// Add `member` to `[workspace] members` in a `Cargo.toml`, making the
/// manifest a workspace (with `"."` as a member) when it isn't one. Fails
/// when the `members` value isn't a plain list of strings.
pub(crate) fn add_member(manifest: &str, member: &str) -> Result<String> {
    let quoted = format!("\"{member}\"");
    let lines: Vec<&str> = manifest.lines().collect();
    let Some(header) = lines.iter().position(|l| l.trim() == "[workspace]") else {
        if !has_package(manifest) {
            bail!("Cargo.toml has neither [package] nor [workspace]");
        }
        let mut out = manifest.trim_end().to_owned();
        out.push_str(&format!("\n\n[workspace]\nmembers = [\".\", {quoted}]\n"));
        return Ok(out);
    };
    let end = lines[header + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| header + 1 + i);
    let section = header + 1..end;
    let members = lines[section.clone()]
        .iter()
        .position(|l| {
            let l = l.trim_start();
            l.starts_with("members") && l["members".len()..].trim_start().starts_with('=')
        })
        .map(|i| header + 1 + i);
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    match members {
        None => {
            let list = if has_package(manifest) {
                format!("members = [\".\", {quoted}]")
            } else {
                format!("members = [{quoted}]")
            };
            out.insert(header + 1, list);
        }
        Some(start) => {
            let Some(close) = (start..end).find(|&i| lines[i].contains(']')) else {
                bail!("can't read `members` in [workspace]: add \"{member}\" to it by hand");
            };
            let text = lines[start..=close].join("\n");
            let Some(open) = text.find('[') else {
                bail!("can't read `members` in [workspace]: add \"{member}\" to it by hand");
            };
            let inner = &text[open + 1..text.rfind(']').unwrap_or(text.len())];
            if inner.contains(&quoted) || inner.contains(&format!("'{member}'")) {
                return Ok(join(&out, manifest));
            }
            if inner.contains('#') || text[..open].contains('[') {
                bail!("can't edit `members` in [workspace]: add \"{member}\" to it by hand");
            }
            if start == close {
                let line = lines[start];
                let at = line.rfind(']').unwrap_or(line.len());
                let before = line[..at].trim_end();
                let comma = if before.ends_with('[') || before.ends_with(',') {
                    ""
                } else {
                    ","
                };
                let space = if before.ends_with('[') { "" } else { " " };
                out[start] = format!("{before}{comma}{space}{quoted}{}", &line[at..]);
            } else {
                let last = (start..close)
                    .rev()
                    .find(|&i| !lines[i].trim().is_empty() && i > start || i == start);
                let last = last.unwrap_or(start);
                let indent: String = lines
                    .get(start + 1)
                    .map(|l| l.chars().take_while(|c| c.is_whitespace()).collect())
                    .filter(|s: &String| !s.is_empty())
                    .unwrap_or_else(|| "    ".to_owned());
                if !out[last].trim_end().ends_with(',') && !out[last].trim_end().ends_with('[') {
                    out[last].push(',');
                }
                out.insert(last + 1, format!("{indent}{quoted},"));
            }
        }
    }
    Ok(join(&out, manifest))
}

fn join(lines: &[String], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The `renox = …` line for a package at `package_dir` (relative to the
/// workspace root): the workspace form when the workspace declares renox in
/// `[workspace.dependencies]` and the manifest is virtual, else what the app
/// itself depends on (a path is rewritten relative to the package), else the
/// version of this CLI.
pub(crate) fn renox_dependency(manifest: &str, package_dir: &Path) -> String {
    let fallback = format!("renox = \"{}\"", env!("CARGO_PKG_VERSION"));
    let mut section = "";
    let mut workspace_dep = false;
    let mut own = None;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed;
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("renox") else {
            continue;
        };
        let rest = rest.trim_start();
        if section == "[workspace.dependencies]" && rest.starts_with('=') {
            workspace_dep = true;
        } else if section == "[dependencies]" && rest.starts_with('=') {
            own = Some(rest[1..].trim().to_owned());
        }
    }
    if !has_package(manifest) && workspace_dep {
        return "renox.workspace = true".to_owned();
    }
    let Some(value) = own else {
        return if workspace_dep {
            "renox.workspace = true".to_owned()
        } else {
            fallback
        };
    };
    if value.contains("workspace") {
        return "renox.workspace = true".to_owned();
    }
    format!("renox = {}", rebase_path(&value, package_dir))
}

/// Rewrite a relative `path = "…"` in a dependency value so it works from
/// `package_dir` instead of the workspace root.
fn rebase_path(value: &str, package_dir: &Path) -> String {
    let Some(at) = value.find("path") else {
        return value.to_owned();
    };
    let after = &value[at + 4..];
    let Some(eq) = after.find('=') else {
        return value.to_owned();
    };
    let after = &after[eq + 1..];
    let Some(open) = after.find('"') else {
        return value.to_owned();
    };
    let Some(len) = after[open + 1..].find('"') else {
        return value.to_owned();
    };
    let original = &after[open + 1..open + 1 + len];
    if Path::new(original).is_absolute() {
        return value.to_owned();
    }
    let depth = package_dir
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    let rebased = format!(
        "{}{}",
        "../".repeat(depth),
        original.trim_start_matches("./")
    );
    value.replacen(&format!("\"{original}\""), &format!("\"{rebased}\""), 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str =
        "[package]\nname = \"shop\"\n\n[dependencies]\nrenox = { path = \"/x/crates/renox\" }\n";

    #[test]
    fn single_package_becomes_a_workspace() {
        let out = add_member(APP, "crates/invoicing").unwrap();
        assert!(out.ends_with("[workspace]\nmembers = [\".\", \"crates/invoicing\"]\n"));
    }

    #[test]
    fn appends_to_existing_members_once() {
        let m = "[package]\nname = \"a\"\n\n[workspace]\nmembers = [\".\", \"crates/a\"]\n\n[dependencies]\n";
        let once = add_member(m, "crates/b").unwrap();
        assert!(once.contains("members = [\".\", \"crates/a\", \"crates/b\"]"));
        assert_eq!(add_member(&once, "crates/b").unwrap(), once);
        assert!(once.contains("[dependencies]"));
    }

    #[test]
    fn appends_to_multiline_members() {
        let m = "[workspace]\nmembers = [\n    \"crates/a\",\n    \"crates/b\"\n]\n";
        let out = add_member(m, "crates/c").unwrap();
        assert_eq!(
            out,
            "[workspace]\nmembers = [\n    \"crates/a\",\n    \"crates/b\",\n    \"crates/c\",\n]\n"
        );
    }

    #[test]
    fn workspace_without_members_gets_them() {
        let out = add_member("[workspace]\nresolver = \"3\"\n", "crates/a").unwrap();
        assert_eq!(
            out,
            "[workspace]\nmembers = [\"crates/a\"]\nresolver = \"3\"\n"
        );
    }

    #[test]
    fn odd_manifests_fail_loudly() {
        assert!(add_member("[dependencies]\n", "crates/a").is_err());
        assert!(add_member("[workspace]\nmembers = [ # x\n\"a\"]\n", "crates/b").is_err());
    }

    #[test]
    fn dependency_path_is_rebased() {
        let dir = Path::new("crates/invoicing");
        assert_eq!(
            renox_dependency(APP, dir),
            "renox = { path = \"/x/crates/renox\" }"
        );
        let rel = APP.replace("/x/crates/renox", "../renox/crates/renox");
        assert_eq!(
            renox_dependency(&rel, dir),
            "renox = { path = \"../../../renox/crates/renox\" }"
        );
        let with_features = "[package]\nname = \"a\"\n[dependencies]\nrenox = { version = \"1\", features = [\"postgres\"] }\n";
        assert_eq!(
            renox_dependency(with_features, dir),
            "renox = { version = \"1\", features = [\"postgres\"] }"
        );
    }

    #[test]
    fn dependency_falls_back_and_follows_the_workspace() {
        let dir = Path::new("crates/a");
        assert_eq!(
            renox_dependency("[package]\nname = \"a\"\n", dir),
            format!("renox = \"{}\"", env!("CARGO_PKG_VERSION"))
        );
        let virt = "[workspace]\nmembers = []\n[workspace.dependencies]\nrenox = \"1\"\n";
        assert_eq!(renox_dependency(virt, dir), "renox.workspace = true");
    }

    #[test]
    fn with_values_are_checked() {
        let v = |s: &[&str]| s.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            parse_with(&v(&["assets", "commands", "assets"])).unwrap(),
            [Extra::Assets, Extra::Commands]
        );
        let err = parse_with(&v(&["admin"])).unwrap_err().to_string();
        assert!(err.contains("needs a newer renox"));
        assert!(parse_with(&v(&["nope"])).is_err());
    }

    #[test]
    fn plan_names_and_rejects_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), APP).unwrap();
        let plan = plan(dir.path(), "my-pkg", None, &[], None).unwrap();
        assert_eq!(plan.lib_name, "my_pkg");
        assert_eq!(plan.struct_name, "MyPkg");
        assert_eq!(plan.path, Path::new("crates/my-pkg"));
        assert_eq!(plan.register, Some(PathBuf::from(".")));
        assert!(plan.manifest.contains("\"crates/my-pkg\""));

        assert!(super::plan(dir.path(), "Bad Name", None, &[], None).is_err());
        assert!(super::plan(dir.path(), "x", None, &["admin".into()], None).is_err());
        assert!(super::plan(dir.path(), "x", Some("../out".into()), &[], None).is_err());
        assert!(super::plan(dir.path(), "x", None, &[], Some("app".into())).is_err());
        std::fs::create_dir_all(dir.path().join("crates/taken")).unwrap();
        assert!(super::plan(dir.path(), "taken", None, &[], None).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(),
            APP
        );
    }

    #[test]
    fn virtual_workspace_registers_only_on_request() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\"]\n[workspace.dependencies]\nrenox = \"1\"\n",
        )
        .unwrap();
        let p = plan(dir.path(), "inv", None, &[], None).unwrap();
        assert_eq!(p.register, None);
        assert_eq!(p.renox_dependency, "renox.workspace = true");
        assert!(p.manifest.contains("members = [\"app\", \"crates/inv\"]"));
        let p = plan(dir.path(), "inv", None, &[], Some("app".into())).unwrap();
        assert_eq!(p.register, Some(PathBuf::from("app")));
    }
}
