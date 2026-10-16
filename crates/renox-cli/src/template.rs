//! Template repositories for `rnx new --from`: the optional
//! `renox-template.toml` and the walker that copies a template directory into
//! a new app.
//!
//! Nothing in a template is ever executed. Files are read and written, never
//! run, and symlinks are skipped so a template cannot reach outside itself.

// `rnx new --from` calls into this module in a later step of issue #35.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::tools;

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

/// Where `rnx new` takes its files from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Renox's own stubs, compiled in.
    Builtin {
        /// `--starter`.
        starter: bool,
        /// `--notifications`.
        notifications: bool,
    },
    /// A template directory on this machine.
    Path(PathBuf),
    /// A template repository: `git+<url>#<ref>`.
    Git {
        /// The repository URL.
        url: String,
        /// A tag or branch; `None` is the default branch.
        git_ref: Option<String>,
    },
}

impl Source {
    /// Reads the value of `--from`: `git+<url>[#<ref>]` is a repository,
    /// anything else a local path.
    pub fn parse(from: &str) -> Result<Source> {
        let Some(rest) = from.strip_prefix("git+") else {
            return Ok(Source::Path(PathBuf::from(from)));
        };
        let (url, git_ref) = match rest.split_once('#') {
            Some((url, git_ref)) => (url, Some(git_ref)),
            None => (rest, None),
        };
        if url.is_empty() {
            bail!("--from {from}: the repository URL is empty (git+<url>#<ref>)");
        }
        if url.starts_with('-') {
            bail!("--from {from}: the repository URL must not start with `-`");
        }
        if git_ref == Some("") {
            bail!(
                "--from {from}: the ref after `#` is empty (use a tag or branch, or drop the `#`)"
            );
        }
        Ok(Source::Git {
            url: url.to_string(),
            git_ref: git_ref.map(str::to_string),
        })
    }

    /// Makes the template files available as a directory: a path is checked,
    /// a repository is cloned into a temporary directory that is removed when
    /// the result is dropped. `Builtin` has no directory and is an error.
    pub fn resolve(&self) -> Result<Resolved> {
        match self {
            Source::Builtin { .. } => bail!("the built-in starter is not a template directory"),
            Source::Path(path) => {
                if !path.is_dir() {
                    bail!("--from {}: not a directory", path.display());
                }
                Ok(Resolved {
                    dir: path.clone(),
                    temp: None,
                })
            }
            Source::Git { url, git_ref } => {
                let temp = TempDir::create()?;
                let dir = temp.0.join("template");
                match tools::git_clone(url, git_ref.as_deref(), &dir) {
                    Ok(()) => Ok(Resolved {
                        dir,
                        temp: Some(temp),
                    }),
                    Err(tools::CloneError::NoGit) => {
                        bail!("`git` was not found on PATH; install git to use --from git+...")
                    }
                    Err(tools::CloneError::Failed(why)) => {
                        let hint = match git_ref {
                            Some(r) if looks_like_sha(r) => {
                                " (a commit SHA is not supported; use a tag or branch)"
                            }
                            _ => "",
                        };
                        let at = git_ref
                            .as_deref()
                            .map(|r| format!("#{r}"))
                            .unwrap_or_default();
                        bail!("could not clone {url}{at}: {why}{hint}")
                    }
                }
            }
        }
    }
}

/// Seven or more hex digits and nothing else: probably a commit SHA.
fn looks_like_sha(text: &str) -> bool {
    (7..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A template directory ready to read.
#[derive(Debug)]
pub struct Resolved {
    /// The directory holding the template files.
    pub dir: PathBuf,
    /// The temporary clone, deleted on drop.
    temp: Option<TempDir>,
}

impl Resolved {
    /// Whether the files come from a temporary clone.
    pub fn is_clone(&self) -> bool {
        self.temp.is_some()
    }
}

/// Splits repeated `--set key=value` arguments. The value may be empty or
/// contain `=`.
pub fn parse_set(args: &[String]) -> Result<Vec<(String, String)>> {
    args.iter()
        .map(|arg| match arg.split_once('=') {
            Some((key, value)) if !key.is_empty() => Ok((key.to_string(), value.to_string())),
            _ => bail!("--set expects key=value, got `{arg}`"),
        })
        .collect()
}

/// Collects the answer for every `[[ask]]` of the template: `--set` first,
/// then (when `interactive`) a prompt on `output` read from `input`, where an
/// empty line takes the default. Without a TTY a missing answer is an error
/// naming `--set key=value`. A `--set` key that no prompt declares is an
/// error. Returns `(key, answer)` pairs in the order of the config.
pub fn answers(
    asks: &[Ask],
    set: &[(String, String)],
    interactive: bool,
    input: &mut impl std::io::BufRead,
    output: &mut impl std::io::Write,
) -> Result<Vec<(String, String)>> {
    for (key, _) in set {
        if !asks.iter().any(|ask| &ask.key == key) {
            let known: Vec<&str> = asks.iter().map(|ask| ask.key.as_str()).collect();
            if known.is_empty() {
                bail!("--set {key}: the template asks no questions");
            }
            bail!(
                "--set {key}: the template has no such question (known: {})",
                known.join(", ")
            );
        }
    }
    let mut out = Vec::new();
    for ask in asks {
        // The last `--set` for a key wins.
        if let Some((_, value)) = set.iter().rev().find(|(key, _)| key == &ask.key) {
            out.push((ask.key.clone(), value.clone()));
            continue;
        }
        if !interactive {
            match &ask.default {
                Some(default) => out.push((ask.key.clone(), default.clone())),
                None => bail!(
                    "the template asks \"{}\" and there is no terminal to answer: pass --set {}=<value>",
                    ask.question,
                    ask.key
                ),
            }
            continue;
        }
        match &ask.default {
            Some(default) => write!(output, "{} [{default}] ", ask.question),
            None => write!(output, "{} ", ask.question),
        }?;
        output.flush()?;
        let mut line = String::new();
        input.read_line(&mut line)?;
        let line = line.trim_end_matches(['\r', '\n']);
        let answer = if line.trim().is_empty() {
            match &ask.default {
                Some(default) => default.clone(),
                None => bail!(
                    "no answer for \"{}\" (pass --set {}=<value>)",
                    ask.question,
                    ask.key
                ),
            }
        } else {
            line.trim().to_string()
        };
        out.push((ask.key.clone(), answer));
    }
    Ok(out)
}

/// A fresh directory under the system temp dir, removed on drop.
#[derive(Debug)]
struct TempDir(PathBuf);

impl TempDir {
    fn create() -> Result<TempDir> {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rnx-template-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).with_context(|| {
            format!("could not create the temporary directory {}", dir.display())
        })?;
        Ok(TempDir(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }

    /// A repo with tag `v1` and branch `side` at the first commit, and a later
    /// commit on the default branch. `None` when git is not installed.
    fn repo() -> Option<tempfile::TempDir> {
        tools::find("git")?;
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        write(dir.path(), "name.txt", b"one {{name}}");
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "--quiet", "-m", "one"]);
        git(dir.path(), &["tag", "v1"]);
        git(dir.path(), &["branch", "side"]);
        write(dir.path(), "name.txt", b"two {{name}}");
        git(dir.path(), &["commit", "--quiet", "-am", "two"]);
        Some(dir)
    }

    fn ask(key: &str, default: Option<&str>) -> Ask {
        Ask {
            key: key.into(),
            question: format!("{key}?"),
            default: default.map(String::from),
        }
    }

    fn run(
        asks: &[Ask],
        set: &[(&str, &str)],
        tty: bool,
        typed: &str,
    ) -> (Result<Vec<(String, String)>>, String) {
        let set: Vec<(String, String)> = set
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let mut out = Vec::new();
        let result = answers(asks, &set, tty, &mut typed.as_bytes(), &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn parses_set_arguments() {
        let args = ["a=1".to_string(), "b=x=y".to_string(), "c=".to_string()];
        assert_eq!(
            parse_set(&args).unwrap(),
            vec![
                ("a".into(), "1".into()),
                ("b".into(), "x=y".into()),
                ("c".into(), "".into())
            ]
        );
        assert!(parse_set(&["nokey".to_string()]).is_err());
        assert!(parse_set(&["=v".to_string()]).is_err());
    }

    #[test]
    fn asks_on_a_terminal_and_takes_defaults() {
        let asks = [ask("company", None), ask("city", Some("Oslo"))];
        let (result, shown) = run(&asks, &[], true, "Acme Inc\n\n");
        assert_eq!(
            result.unwrap(),
            vec![
                ("company".into(), "Acme Inc".into()),
                ("city".into(), "Oslo".into())
            ]
        );
        assert!(shown.contains("company? ") && shown.contains("city? [Oslo] "));
        // An empty answer without a default is an error.
        assert!(run(&asks, &[], true, "\n").0.is_err());
    }

    #[test]
    fn set_answers_without_prompting() {
        let asks = [ask("company", None)];
        let (result, shown) = run(&asks, &[("company", "A"), ("company", "B")], true, "");
        assert_eq!(result.unwrap(), vec![("company".into(), "B".into())]);
        assert!(shown.is_empty());
    }

    #[test]
    fn without_a_terminal_a_missing_answer_names_set() {
        let asks = [ask("company", None), ask("city", Some("Oslo"))];
        let err = run(&asks, &[], false, "").0.unwrap_err().to_string();
        assert!(err.contains("--set company=<value>"), "{err}");
        let (result, _) = run(&asks, &[("company", "A")], false, "");
        assert_eq!(result.unwrap()[1], ("city".into(), "Oslo".into()));
    }

    #[test]
    fn set_for_an_unknown_key_is_an_error() {
        let asks = [ask("company", None)];
        let err = run(&asks, &[("nope", "x")], true, "").0.unwrap_err();
        assert!(err.to_string().contains("nope") && err.to_string().contains("company"));
        let err = run(&[], &[("nope", "x")], true, "").0.unwrap_err();
        assert!(err.to_string().contains("no questions"));
    }

    #[test]
    fn parses_sources() {
        assert_eq!(
            Source::parse("../t").unwrap(),
            Source::Path(PathBuf::from("../t"))
        );
        assert_eq!(
            Source::parse("git+https://x.test/a.git#v3").unwrap(),
            Source::Git {
                url: "https://x.test/a.git".into(),
                git_ref: Some("v3".into())
            }
        );
        assert_eq!(
            Source::parse("git+file:///r").unwrap(),
            Source::Git {
                url: "file:///r".into(),
                git_ref: None
            }
        );
        assert!(Source::parse("git+").is_err());
        assert!(Source::parse("git+https://x.test/a.git#").is_err());
        assert!(Source::parse("git+-oevil").is_err());
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let err = Source::Path(PathBuf::from("/no/such/template"))
            .resolve()
            .unwrap_err();
        assert!(err.to_string().contains("not a directory"), "{err}");
    }

    #[test]
    fn clones_a_tag_a_branch_and_the_default() {
        let Some(repo) = repo() else {
            eprintln!("skipped: git is not installed");
            return;
        };
        let url = format!("file://{}", repo.path().display());
        for (git_ref, want) in [(Some("v1"), "one"), (Some("side"), "one"), (None, "two")] {
            let source = Source::Git {
                url: url.clone(),
                git_ref: git_ref.map(str::to_string),
            };
            let resolved = source.resolve().unwrap();
            assert!(resolved.is_clone());
            let text = fs::read_to_string(resolved.dir.join("name.txt")).unwrap();
            assert!(text.starts_with(want), "{git_ref:?}: {text}");
            let temp = resolved.dir.clone();
            drop(resolved);
            assert!(!temp.exists(), "the clone is removed");
        }
    }

    #[test]
    fn an_unknown_ref_and_a_sha_are_clear_errors() {
        let Some(repo) = repo() else {
            eprintln!("skipped: git is not installed");
            return;
        };
        let url = format!("file://{}", repo.path().display());
        let unknown = Source::Git {
            url: url.clone(),
            git_ref: Some("nope".into()),
        };
        let err = unknown.resolve().unwrap_err().to_string();
        assert!(
            err.contains("could not clone") && err.contains("#nope"),
            "{err}"
        );
        let sha = Source::Git {
            url,
            git_ref: Some("0123456789abcdef".into()),
        };
        let err = sha.resolve().unwrap_err().to_string();
        assert!(err.contains("commit SHA is not supported"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_clone_does_not_run_template_hooks() {
        use std::os::unix::fs::PermissionsExt;
        let Some(repo) = repo() else {
            eprintln!("skipped: git is not installed");
            return;
        };
        // Hooks are not cloned; this also proves the clone never runs ours.
        let marker = repo.path().join("hook-ran");
        let hook = repo.path().join(".git/hooks/post-checkout");
        fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let url = format!("file://{}", repo.path().display());
        Source::Git { url, git_ref: None }.resolve().unwrap();
        assert!(!marker.exists());
    }

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
