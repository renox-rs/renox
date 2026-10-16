//! `rnx make:package`: a reusable crate with a `Module`, joined to the app's
//! workspace. This file holds the plumbing: the name, the `--with` extras,
//! the workspace `members` edit and the `renox` dependency the package uses.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use heck::{ToTitleCase, ToUpperCamelCase};

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

/// The package's files: where they go (`__crate__` is the crate name) and
/// their stubs.
const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        include_str!("../stubs/package/Cargo.toml.stub"),
    ),
    ("build.rs", include_str!("../stubs/package/build.rs")),
    ("README.md", include_str!("../stubs/package/README.md")),
    ("src/lib.rs", include_str!("../stubs/package/src/lib.rs")),
    (
        "src/model.rs",
        include_str!("../stubs/package/src/model.rs"),
    ),
    (
        "src/texts.rs",
        include_str!("../stubs/package/src/texts.rs"),
    ),
    (
        "views/__crate__/index.html",
        include_str!("../stubs/package/views/index.html"),
    ),
    (
        "tests/package.rs",
        include_str!("../stubs/package/tests/package.rs"),
    ),
];

/// The files `--with` adds.
const EXTRA_FILES: &[(Extra, &str, &str)] = &[
    (
        Extra::Assets,
        "src/assets.rs",
        include_str!("../stubs/package/src/assets.rs"),
    ),
    (
        Extra::Assets,
        "public/__lib__.css",
        include_str!("../stubs/package/public/package.css"),
    ),
    (
        Extra::Commands,
        "src/commands.rs",
        include_str!("../stubs/package/src/commands.rs"),
    ),
];

/// The first migration's files: the suffix after its name, and the SQL.
const MIGRATION: &[(&str, &str)] = &[
    ("up.sql", include_str!("../stubs/package/migrations/up.sql")),
    (
        "postgres.up.sql",
        include_str!("../stubs/package/migrations/up.postgres.sql"),
    ),
    (
        "down.sql",
        include_str!("../stubs/package/migrations/down.sql"),
    ),
];

/// Every file of the package as (path inside the package, contents), with
/// the placeholders filled in. `version` is the migration's timestamp. Fails
/// when a placeholder is left over.
pub(crate) fn render(plan: &Plan, version: &str) -> Result<Vec<(PathBuf, String)>> {
    let title = plan.crate_name.to_title_case();
    let has = |extra| plan.extras.contains(&extra);
    // `src/lib.rs` declares and registers the extras, in the order rustfmt
    // keeps the `mod` lines.
    let line = |on: bool, text: &str| if on { text.to_owned() } else { String::new() };
    let fill = |text: &str| {
        text.replace(
            "__modassets__\n",
            &line(has(Extra::Assets), "mod assets;\n"),
        )
        .replace(
            "__modcommands__\n",
            &line(has(Extra::Commands), "mod commands;\n"),
        )
        .replace(
            "        __extras__\n",
            &format!(
                "{}{}",
                line(has(Extra::Assets), "        assets::register(app);\n"),
                line(has(Extra::Commands), "        commands::register(app);\n"),
            ),
        )
        .replace("__Struct__", &plan.struct_name)
        .replace("__Title__", &title)
        .replace("__crate__", &plan.crate_name)
        .replace("__lib__", &plan.lib_name)
        .replace("__renox__", &plan.renox_dependency)
    };
    let stem = format!("{version}_create_{}_entries_table", plan.lib_name);
    let mut files = Vec::new();
    for (path, text) in FILES {
        files.push((PathBuf::from(fill(path)), fill(text)));
    }
    for (extra, path, text) in EXTRA_FILES {
        if has(*extra) {
            files.push((PathBuf::from(fill(path)), fill(text)));
        }
    }
    for (suffix, text) in MIGRATION {
        files.push((
            Path::new("migrations").join(format!("{stem}.{suffix}")),
            fill(text),
        ));
    }
    for (path, text) in &files {
        if let Some(left) = placeholder(text) {
            bail!("{}: `{left}` was not filled in", path.display());
        }
    }
    Ok(files)
}

/// A `__word__` left in `text`, if any.
fn placeholder(text: &str) -> Option<&str> {
    let mut rest = text;
    while let Some(at) = rest.find("__") {
        let after = &rest[at + 2..];
        let word = after.len()
            - after
                .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                .len();
        if word > 0 && after[word..].starts_with("__") {
            return Some(&rest[at..at + word + 4]);
        }
        rest = &rest[at + 2..];
    }
    None
}

/// Writes the package under `root`, adds it to the workspace `members` and
/// registers it in the app. Everything is rendered first, so a failure
/// leaves nothing behind.
pub(crate) fn write(root: &Path, plan: &Plan, now: chrono::DateTime<chrono::Utc>) -> Result<()> {
    let version = crate::make::next_version(&root.join(&plan.path).join("migrations"), now);
    let files = render(plan, &version)?;
    // The app's manifest with the path dependency, worked out before writing.
    let app = match &plan.register {
        None => None,
        Some(dir) => {
            let file = root.join(dir).join("Cargo.toml");
            let current = if dir == Path::new(".") {
                plan.manifest.clone()
            } else {
                std::fs::read_to_string(&file)
                    .with_context(|| format!("could not read {}", file.display()))?
            };
            let path = package_path(dir, &plan.path);
            Some((dir, file, add_dependency(&current, &plan.crate_name, &path)))
        }
    };
    let base = root.join(&plan.path);
    for (path, text) in files {
        let target = base.join(&path);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        std::fs::write(&target, text)
            .with_context(|| format!("could not write {}", target.display()))?;
        println!("Created {}", plan.path.join(&path).display());
    }
    let manifest = match &app {
        Some((dir, _, updated)) if *dir == Path::new(".") => updated,
        _ => &plan.manifest,
    };
    std::fs::write(root.join("Cargo.toml"), manifest).context("could not update Cargo.toml")?;
    println!("Updated Cargo.toml (workspace members)");
    if let Some((dir, file, updated)) = &app {
        if *dir != Path::new(".") {
            std::fs::write(file, updated)
                .with_context(|| format!("could not update {}", file.display()))?;
        }
        println!(
            "Updated {} (dependency {})",
            file.display(),
            plan.crate_name
        );
        let app_root = root.join(dir);
        crate::generate::register_in_main(
            &crate::generate::app_file(&app_root),
            &format!("{}::{}", plan.lib_name, plan.struct_name),
        )?;
    }
    Ok(())
}

/// The `path = "…"` of the package as seen from the crate in `from`
/// (both relative to the workspace root).
fn package_path(from: &Path, package: &Path) -> String {
    let depth = from
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    let rel = package.to_string_lossy().replace('\\', "/");
    format!("{}{rel}", "../".repeat(depth))
}

/// Add `name = { path = "…" }` to `[dependencies]` of a manifest (creating
/// the section when missing); unchanged when the dependency is there.
pub(crate) fn add_dependency(manifest: &str, name: &str, path: &str) -> String {
    let line = format!("{name} = {{ path = \"{path}\" }}");
    let mut lines: Vec<String> = manifest.lines().map(str::to_owned).collect();
    let mut section = String::new();
    let mut last = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if t.starts_with('[') {
            section = t.to_owned();
            if t == "[dependencies]" {
                last = Some(i);
            }
        } else if section == "[dependencies]" {
            if t.starts_with(&format!("{name} ")) || t.starts_with(&format!("{name}=")) {
                return manifest.to_owned();
            }
            if !t.is_empty() {
                last = Some(i);
            }
        }
    }
    match last {
        Some(i) => lines.insert(i + 1, line),
        None => {
            lines.push(String::new());
            lines.push("[dependencies]".to_owned());
            lines.push(line);
        }
    }
    lines.join("\n") + "\n"
}

/// `rnx make:package`.
pub(crate) fn run(
    root: &Path,
    name: &str,
    path: Option<PathBuf>,
    with: &[String],
    register: Option<PathBuf>,
) -> Result<()> {
    let plan = plan(root, name, path, with, register)?;
    write(root, &plan, chrono::Utc::now())
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
    if value.replace(' ', "").contains("workspace=true") {
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
        // A path that merely contains the word is still a path.
        let app = "[package]\nname = \"a\"\n[dependencies]\nrenox = { path = \"/home/me/workspace/renox\" }\n";
        assert_eq!(
            renox_dependency(app, dir),
            "renox = { path = \"/home/me/workspace/renox\" }"
        );
        let inherited = "[package]\nname = \"a\"\n[dependencies]\nrenox = { workspace = true }\n";
        assert_eq!(renox_dependency(inherited, dir), "renox.workspace = true");
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
    fn writes_the_package_tree() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), APP).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "fn app() {\n    App::new()\n        .module(Home)\n        .run();\n}\n",
        )
        .unwrap();
        let plan = plan(dir.path(), "my-pkg", None, &[], None).unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-16T10:00:00Z")
            .unwrap()
            .to_utc();
        write(dir.path(), &plan, now).unwrap();
        let pkg = dir.path().join("crates/my-pkg");
        let mut found = Vec::new();
        let mut stack = vec![pkg.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let text = std::fs::read_to_string(&p).unwrap();
                    assert!(placeholder(&text).is_none(), "{}", p.display());
                    found.push(p.strip_prefix(&pkg).unwrap().to_string_lossy().into_owned());
                }
            }
        }
        found.sort();
        assert_eq!(
            found,
            [
                "Cargo.toml",
                "README.md",
                "build.rs",
                "migrations/20261016100000_create_my_pkg_entries_table.down.sql",
                "migrations/20261016100000_create_my_pkg_entries_table.postgres.up.sql",
                "migrations/20261016100000_create_my_pkg_entries_table.up.sql",
                "src/lib.rs",
                "src/model.rs",
                "src/texts.rs",
                "tests/package.rs",
                "views/my-pkg/index.html",
            ]
        );
        let lib = std::fs::read_to_string(pkg.join("src/lib.rs")).unwrap();
        assert!(lib.contains("pub struct MyPkg;") && lib.contains("my_pkg.index"));
        let cargo = std::fs::read_to_string(pkg.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my-pkg\""));
        assert!(
            cargo.contains("renox = { path = \"../../../x/crates/renox\" }")
                || cargo.contains("renox = { path = \"/x/crates/renox\" }")
        );
        let sql = std::fs::read_to_string(
            pkg.join("migrations/20261016100000_create_my_pkg_entries_table.up.sql"),
        )
        .unwrap();
        assert!(sql.contains("\"my_pkg_entries\""));
        let root = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert!(root.contains("members = [\".\", \"crates/my-pkg\"]"));
        assert!(root.contains("my-pkg = { path = \"crates/my-pkg\" }"));
        let app = std::fs::read_to_string(dir.path().join("src/lib.rs")).unwrap();
        assert!(app.contains(".module(my_pkg::MyPkg)"));
        // A second run is refused by the plan, so nothing is overwritten.
        assert!(super::plan(dir.path(), "my-pkg", None, &[], None).is_err());
    }

    #[test]
    fn extras_add_their_files_and_registrations() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), APP).unwrap();
        let with = vec!["assets".to_owned(), "commands".to_owned()];
        let extras = plan(dir.path(), "my-pkg", None, &with, None).unwrap();
        let files: std::collections::BTreeMap<String, String> = render(&extras, "20261016100000")
            .unwrap()
            .into_iter()
            .map(|(p, t)| (p.to_string_lossy().into_owned(), t))
            .collect();
        for path in ["src/assets.rs", "src/commands.rs", "public/my_pkg.css"] {
            assert!(files.contains_key(path), "{path}");
        }
        let lib = &files["src/lib.rs"];
        assert!(lib.contains("mod assets;\nmod commands;\nmod model;\n"));
        assert!(lib.contains("        assets::register(app);\n        commands::register(app);\n"));
        assert!(files["src/assets.rs"].contains("\"/_my_pkg/my_pkg-0.1.0.css\""));
        assert!(files["src/commands.rs"].contains("name = \"my-pkg:status\""));

        // Without extras none of it is there.
        let plain = plan(dir.path(), "my-pkg", None, &[], None).unwrap();
        let files = render(&plain, "20261016100000").unwrap();
        assert!(files.iter().all(|(p, _)| p != Path::new("src/assets.rs")));
        let lib = files
            .iter()
            .find(|(p, _)| p == Path::new("src/lib.rs"))
            .unwrap();
        assert!(lib.1.contains("mod texts;\n\nuse renox::db::Migration;"));
        assert!(!lib.1.contains("register(app)"));
    }

    #[test]
    fn leftover_placeholders_are_found() {
        assert_eq!(placeholder("a __crate__ b"), Some("__crate__"));
        assert_eq!(placeholder("x __lib___entries"), Some("__lib__"));
        assert_eq!(placeholder("snake_case __ and_ _x_ __1__"), None);
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

    #[test]
    fn dependency_is_added_once() {
        let m = "[package]\nname = \"a\"\n\n[dependencies]\nrenox = \"1\"\n\n[dev-dependencies]\n";
        let once = add_dependency(m, "inv", "crates/inv");
        assert!(once.contains("renox = \"1\"\ninv = { path = \"crates/inv\" }\n\n[dev-dep"));
        assert_eq!(add_dependency(&once, "inv", "crates/inv"), once);
        assert!(
            add_dependency("[package]\n", "inv", "x")
                .ends_with("[dependencies]\ninv = { path = \"x\" }\n")
        );
        assert_eq!(
            package_path(Path::new("app"), Path::new("crates/inv")),
            "../crates/inv"
        );
        assert_eq!(
            package_path(Path::new("."), Path::new("crates/inv")),
            "crates/inv"
        );
    }

    #[test]
    fn virtual_workspace_registers_in_the_named_crate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("app/src")).unwrap();
        std::fs::write(
            dir.path().join("app/Cargo.toml"),
            "[package]\nname = \"app\"\n\n[dependencies]\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("app/src/lib.rs"), "    .module(Home)\n").unwrap();
        let now = chrono::Utc::now();
        let none = plan(dir.path(), "inv", None, &[], None).unwrap();
        write(dir.path(), &none, now).unwrap();
        let app = std::fs::read_to_string(dir.path().join("app/Cargo.toml")).unwrap();
        assert!(!app.contains("inv"));
        let named = plan(dir.path(), "inv2", None, &[], Some("app".into())).unwrap();
        write(dir.path(), &named, now).unwrap();
        let app = std::fs::read_to_string(dir.path().join("app/Cargo.toml")).unwrap();
        assert!(app.contains("inv2 = { path = \"../crates/inv2\" }"));
        let lib = std::fs::read_to_string(dir.path().join("app/src/lib.rs")).unwrap();
        assert!(lib.contains(".module(inv2::Inv2)"));
        let root = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert!(root.contains("\"crates/inv\"") && root.contains("\"crates/inv2\""));
    }
}
