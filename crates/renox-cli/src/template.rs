//! Template repositories for `rnx new --from`: the optional
//! `renox-template.toml` and the walker that copies a template directory into
//! a new app.
//!
//! Nothing in a template is ever executed. Files are read and written, never
//! run, and symlinks are skipped so a template cannot reach outside itself.

// `rnx new --from` calls into this module in a later step of issue #35.
#![allow(dead_code)]

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The file name of the optional template configuration (never copied).
pub const CONFIG_FILE: &str = "renox-template.toml";

/// The parsed `renox-template.toml`. Unknown keys are an error.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Globs of files that get placeholder replacement. `None` (the key is
    /// absent) means every text file.
    #[serde(default)]
    pub replace: Option<Vec<String>>,
    /// Globs of files and directories that are not copied.
    #[serde(default)]
    pub drop: Vec<String>,
    /// Extra prompts; each answer fills `{{key}}`.
    #[serde(default)]
    pub ask: Vec<Ask>,
}

/// One `[[ask]]` entry of `renox-template.toml`.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    /// The placeholder name: the answer replaces `{{key}}`.
    pub key: String,
    /// The question shown to the user.
    pub question: String,
    /// Used when the answer is empty.
    #[serde(default)]
    pub default: Option<String>,
}

impl Config {
    /// Parses the text of a `renox-template.toml`.
    pub fn parse(text: &str) -> Result<Config> {
        let config: Config =
            toml::from_str(text).with_context(|| format!("invalid {CONFIG_FILE}"))?;
        let mut seen: Vec<&str> = Vec::new();
        for ask in &config.ask {
            let valid = !ask.key.is_empty()
                && ask
                    .key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if !valid {
                bail!(
                    "invalid {CONFIG_FILE}: ask key `{}` must be letters, digits, `_` or `-`",
                    ask.key
                );
            }
            if seen.contains(&ask.key.as_str()) {
                bail!("invalid {CONFIG_FILE}: ask key `{}` appears twice", ask.key);
            }
            seen.push(&ask.key);
        }
        Ok(config)
    }

    /// Reads `renox-template.toml` from the template root; a template without
    /// one gets the defaults.
    pub fn load(root: &Path) -> Result<Config> {
        let path = root.join(CONFIG_FILE);
        match fs::read_to_string(&path) {
            Ok(text) => Config::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
        }
    }

    fn is_dropped(&self, rel: &str) -> bool {
        self.drop.iter().any(|pattern| matches(pattern, rel))
    }

    fn replaces(&self, rel: &str) -> bool {
        match &self.replace {
            None => true,
            Some(patterns) => patterns.iter().any(|pattern| matches(pattern, rel)),
        }
    }
}

/// Matches a glob against a path relative to the template root (always `/`
/// separated). `*` and `?` stay inside one path segment, `**` spans any number
/// of segments. A pattern without `/` is matched against every segment of the
/// path (so `*.png` hits `a/b/c.png` and `node_modules` hits the directory
/// and everything in it); a pattern with `/` is anchored at the root and also
/// hits everything inside a matching directory.
pub fn matches(pattern: &str, rel: &str) -> bool {
    let pattern = pattern.trim_start_matches("./");
    let segments: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
    if !pattern.contains('/') {
        return segments.iter().any(|s| segment_match(pattern, s));
    }
    let pattern: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    (1..=segments.len()).any(|n| path_match(&pattern, &segments[..n]))
}

fn path_match(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|n| path_match(rest, &path[n..])),
        Some((first, rest)) => match path.split_first() {
            Some((segment, tail)) => segment_match(first, segment) && path_match(rest, tail),
            None => false,
        },
    }
}

fn segment_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|n| go(rest, &t[n..])),
            Some(('?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    go(&p, &t)
}

/// A file is text when it is valid UTF-8 and has no NUL byte.
pub fn as_text(bytes: &[u8]) -> Option<&str> {
    if bytes.contains(&0) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

/// Copies the template at `src` into `dst` (created when missing).
///
/// Skipped: `.git` (at any depth), `target` at the root, the config file, files
/// matching `drop`, and symlinks (each reported on stderr). Text files in
/// `replace` (default: all) go through `fill(file_name, text)`; everything else
/// is copied byte for byte. File modes are kept. Returns the relative paths
/// written.
pub fn copy(
    src: &Path,
    dst: &Path,
    config: &Config,
    fill: &dyn Fn(&str, &str) -> String,
) -> Result<Vec<String>> {
    fs::create_dir_all(dst).with_context(|| format!("could not create {}", dst.display()))?;
    let mut written = Vec::new();
    walk(src, dst, "", config, fill, &mut written)?;
    Ok(written)
}

fn walk(
    src: &Path,
    dst: &Path,
    prefix: &str,
    config: &Config,
    fill: &dyn Fn(&str, &str) -> String,
    written: &mut Vec<String>,
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(src)
        .with_context(|| format!("could not read {}", src.display()))?
        .collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let kind = entry.file_type()?;
        if name == ".git" || (prefix.is_empty() && (name == "target" || name == CONFIG_FILE)) {
            continue;
        }
        if kind.is_symlink() {
            eprintln!("warning: skipped symlink {rel} (symlinks in a template are not followed)");
            continue;
        }
        if config.is_dropped(&rel) {
            continue;
        }
        let to = dst.join(&name);
        if kind.is_dir() {
            fs::create_dir_all(&to)
                .with_context(|| format!("could not create {}", to.display()))?;
            walk(&entry.path(), &to, &rel, config, fill, written)?;
        } else if kind.is_file() {
            let bytes = fs::read(entry.path())
                .with_context(|| format!("could not read {}", entry.path().display()))?;
            match as_text(&bytes) {
                Some(text) if config.replaces(&rel) => fs::write(&to, fill(&name, text)),
                _ => fs::write(&to, &bytes),
            }
            .with_context(|| format!("could not write {}", to.display()))?;
            fs::set_permissions(&to, entry.metadata()?.permissions())
                .with_context(|| format!("could not set the mode of {}", to.display()))?;
            written.push(rel);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acme(_: &str, text: &str) -> String {
        text.replace("{{name}}", "ACME")
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn parses_the_config() {
        let config = Config::parse(
            "replace = [\"src/**\"]\ndrop = [\"*.bak\"]\n\n\
             [[ask]]\nkey = \"company\"\nquestion = \"Company name?\"\ndefault = \"Acme\"\n",
        )
        .unwrap();
        assert_eq!(config.replace, Some(vec!["src/**".to_owned()]));
        assert_eq!(config.drop, vec!["*.bak".to_owned()]);
        assert_eq!(config.ask[0].key, "company");
        assert_eq!(config.ask[0].default.as_deref(), Some("Acme"));
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn rejects_unknown_and_bad_keys() {
        let e = Config::parse("replce = []").unwrap_err();
        assert!(format!("{e:#}").contains("replce"), "{e:#}");
        let e = Config::parse("[[ask]]\nkey = \"a\"\nquestion = \"q\"\nextra = 1").unwrap_err();
        assert!(format!("{e:#}").contains("extra"), "{e:#}");
        assert!(Config::parse("[[ask]]\nkey = \"a b\"\nquestion = \"q\"").is_err());
        let twice =
            "[[ask]]\nkey = \"a\"\nquestion = \"q\"\n[[ask]]\nkey = \"a\"\nquestion = \"q\"";
        assert!(Config::parse(twice).is_err());
    }

    #[test]
    fn glob_matching() {
        assert!(matches("*.png", "a/b/c.png"));
        assert!(!matches("*.png", "a/b/c.jpg"));
        assert!(matches("node_modules", "x/node_modules/y.js"));
        assert!(matches("docs/**", "docs/a/b.md"));
        assert!(matches("docs", "docs/a.md"));
        assert!(!matches("docs/*.md", "x/docs/a.md"));
        assert!(matches("docs/*.md", "docs/a.md"));
        assert!(matches("src/**/mod.rs", "src/a/b/mod.rs"));
        assert!(matches("src/**/mod.rs", "src/mod.rs"));
        assert!(matches("a?.txt", "ab.txt"));
        assert!(!matches("a?.txt", "a/b.txt"));
    }

    #[test]
    fn detects_text() {
        assert!(as_text(b"hello").is_some());
        assert!(as_text(&[0x89, b'P', b'N', b'G', 0]).is_none());
        assert!(as_text(&[0xff, 0xfe, 0x41]).is_none());
        assert!(as_text(b"a\0b").is_none());
    }

    #[test]
    fn copies_a_template() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let out = dst.path().join("app");
        let png = b"\x89PNG\0{{name}}";
        write(src.path(), "a.txt", b"hi {{name}} {{ name }} {{other}}");
        write(src.path(), "img/logo.png", png);
        write(src.path(), "keep/b.txt", b"{{name}}");
        write(src.path(), "notes.bak", b"drop me");
        write(src.path(), "old/x.txt", b"drop me");
        write(src.path(), ".git/config", b"git");
        write(src.path(), "target/out", b"build");
        write(src.path(), "sub/target/kept.txt", b"kept");
        write(src.path(), CONFIG_FILE, b"drop = [\"*.bak\", \"old\"]");
        let config = Config::load(src.path()).unwrap();
        let written = copy(src.path(), &out, &config, &acme).unwrap();

        assert_eq!(
            written,
            ["a.txt", "img/logo.png", "keep/b.txt", "sub/target/kept.txt"]
        );
        assert_eq!(
            fs::read_to_string(out.join("a.txt")).unwrap(),
            "hi ACME {{ name }} {{other}}"
        );
        assert_eq!(fs::read(out.join("img/logo.png")).unwrap(), png);
        for gone in [".git", "target", CONFIG_FILE, "notes.bak", "old"] {
            assert!(!out.join(gone).exists(), "{gone} was copied");
        }
    }

    #[test]
    fn replace_restricts_where_text_is_filled() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        write(src.path(), "src/a.rs", b"{{name}}");
        write(src.path(), "views/a.html", b"{{name}}");
        write(src.path(), CONFIG_FILE, b"replace = [\"src/**\"]");
        let config = Config::load(src.path()).unwrap();
        copy(src.path(), dst.path(), &config, &acme).unwrap();
        assert_eq!(
            fs::read_to_string(dst.path().join("src/a.rs")).unwrap(),
            "ACME"
        );
        assert_eq!(
            fs::read_to_string(dst.path().join("views/a.html")).unwrap(),
            "{{name}}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn keeps_modes_and_skips_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let secret = tempfile::tempdir().unwrap();
        write(secret.path(), "secret.txt", b"secret");
        write(src.path(), "run.sh", b"#!/bin/sh\n{{name}}\n");
        let run = src.path().join("run.sh");
        fs::set_permissions(&run, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(
            secret.path().join("secret.txt"),
            src.path().join("link.txt"),
        )
        .unwrap();
        symlink(secret.path(), src.path().join("linkdir")).unwrap();
        copy(src.path(), dst.path(), &Config::default(), &acme).unwrap();
        let mode = fs::metadata(dst.path().join("run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(!dst.path().join("link.txt").exists());
        assert!(!dst.path().join("linkdir").exists());
    }

    #[cfg(unix)]
    #[test]
    fn nothing_in_a_template_is_executed() {
        use std::os::unix::fs::PermissionsExt;
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let marker = dst.path().join("MARKER");
        let script = format!("#!/bin/sh\ntouch {}\n", marker.display());
        let build = format!(
            "fn main() {{ std::fs::write({:?}, \"x\").unwrap(); }}",
            marker
        );
        write(src.path(), "build.rs", build.as_bytes());
        write(src.path(), "setup.sh", script.as_bytes());
        write(src.path(), ".git/hooks/post-checkout", script.as_bytes());
        for f in ["setup.sh", ".git/hooks/post-checkout"] {
            fs::set_permissions(src.path().join(f), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let out = dst.path().join("app");
        copy(src.path(), &out, &Config::default(), &acme).unwrap();
        assert!(out.join("build.rs").exists());
        assert!(out.join("setup.sh").exists());
        assert!(!out.join(".git").exists());
        assert!(!marker.exists(), "a template file was executed");
    }
}
