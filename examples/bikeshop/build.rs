//! Rebuilds on migrations, views and public files, and makes the table of
//! code regions the "About this page" panels show (`src/code.rs`).
//!
//! A region is marked in the source by two comment lines, `[explain:<name>]`
//! and `[/explain:<name>]`, in the comment syntax of the file:
//!
//! ```text
//! // [explain:rentals.reserve]          Rust, JavaScript
//! {# [explain:rentals.reserve.form] #}   templates (`{#- … #}` works too)
//! -- [explain:rentals.overlap]          SQL
//! /* [explain:rentals.css] */           CSS
//! ```
//!
//! The lines between become the region's text, without the marker lines and
//! without their common indentation. A name opened again later in the same
//! file continues the region: the parts are joined with a `…` line, so a
//! sample can skip what doesn't matter. Mistakes (a marker never closed, a
//! close without an open, one name in two files) don't fail the build: they
//! go into the table's `PROBLEMS`, which `tests/about.rs` reports.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    // Rebuild when migrations change, so `renox::migrations!()` sees new files.
    println!("cargo:rerun-if-changed=migrations");
    // And when views, translations or public files change, for `renox::embedded!()`.
    println!("cargo:rerun-if-changed=resources");
    println!("cargo:rerun-if-changed=public");
    // The code regions come from these too.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=tests");

    let mut files = Vec::new();
    for dir in ["src", "resources/views", "tests", "migrations", "public"] {
        collect(Path::new(dir), &mut files);
    }
    files.sort();

    let mut regions: BTreeMap<String, Region> = BTreeMap::new();
    let mut problems = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let path = format!(
            "examples/bikeshop/{}",
            file.to_string_lossy().replace('\\', "/")
        );
        scan(&path, &text, &mut regions, &mut problems);
    }

    let mut out = String::from("&[\n");
    for (name, region) in &regions {
        let text = region.text();
        writeln!(
            out,
            "    Region {{ name: {name:?}, path: {:?}, language: {:?}, first_line: {}, last_line: {}, text: {text:?} }},",
            region.path,
            region.language,
            region.first_line,
            region.last_line,
        )
        .unwrap();
    }
    out.push_str("]\n");
    let problems = format!("&{problems:?}\n");

    let dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(dir.join("regions.rs"), out).unwrap();
    std::fs::write(dir.join("region_problems.rs"), problems).unwrap();
}

/// Every text file under `dir` a region may live in (not the vendored ones).
fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "vendor") {
                continue;
            }
            collect(&path, files);
        } else if language(&path).is_some() {
            files.push(path);
        }
    }
}

/// The language a file's samples are highlighted as.
fn language(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "rs" => Some("rust"),
        "html" => Some("html"),
        "sql" => Some("sql"),
        "css" => Some("css"),
        "js" | "mjs" => Some("js"),
        _ => None,
    }
}

/// One region as it is being collected.
struct Region {
    path: String,
    language: &'static str,
    /// Each part: its lines and the line number of the first (from 1).
    parts: Vec<(usize, Vec<String>)>,
    first_line: usize,
    last_line: usize,
}

impl Region {
    /// The parts, each without its common indentation, joined by a `…`
    /// line in the language's comment syntax.
    fn text(&self) -> String {
        let mut out = Vec::new();
        for (n, (_, lines)) in self.parts.iter().enumerate() {
            if n > 0 {
                out.push(
                    match self.language {
                        "html" => "{# … #}",
                        "sql" => "-- …",
                        "css" => "/* … */",
                        _ => "// …",
                    }
                    .to_owned(),
                );
            }
            let indent = lines
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .min()
                .unwrap_or(0);
            for line in lines {
                let line = line.trim_end();
                out.push(line.get(indent..).unwrap_or(line.trim_start()).to_owned());
            }
        }
        // No blank lines at either end.
        while out.first().is_some_and(|l| l.is_empty()) {
            out.remove(0);
        }
        while out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out.join("\n")
    }
}

/// `[explain:name]` → (true, name), `[/explain:name]` → (false, name), when
/// the line holds nothing but the marker in a comment.
fn marker(line: &str) -> Option<(bool, &str)> {
    let mut rest = line.trim();
    for open in ["{#-", "{#", "//", "--", "/*"] {
        if let Some(r) = rest.strip_prefix(open) {
            rest = r;
            break;
        }
    }
    for close in ["-#}", "#}", "*/"] {
        if let Some(r) = rest.strip_suffix(close) {
            rest = r;
            break;
        }
    }
    let rest = rest.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (open, name) = match rest.strip_prefix('/') {
        Some(name) => (false, name),
        None => (true, rest),
    };
    let name = name.strip_prefix("explain:")?;
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    valid.then_some((open, name))
}

fn scan(
    path: &str,
    text: &str,
    regions: &mut BTreeMap<String, Region>,
    problems: &mut Vec<String>,
) {
    let language = language(Path::new(path)).unwrap_or("text");
    // Open regions: name → (first line number, lines so far).
    let mut open: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let number = n + 1;
        match marker(line) {
            Some((true, name)) => {
                if open.contains_key(name) {
                    problems.push(format!(
                        "{path}:{number}: `[explain:{name}]` is opened again before it was closed"
                    ));
                } else {
                    open.insert(name.to_owned(), (number + 1, Vec::new()));
                }
            }
            Some((false, name)) => {
                let Some((first, lines)) = open.remove(name) else {
                    problems.push(format!(
                        "{path}:{number}: `[/explain:{name}]` closes a region that isn't open"
                    ));
                    continue;
                };
                if lines.iter().all(|l| l.trim().is_empty()) {
                    problems.push(format!("{path}:{number}: `{name}` is empty"));
                    continue;
                }
                let last = number - 1;
                match regions.get_mut(name) {
                    Some(region) if region.path == path => {
                        region.parts.push((first, lines));
                        region.last_line = last;
                    }
                    Some(region) => problems.push(format!(
                        "{path}:{number}: `{name}` is a region of {} already",
                        region.path
                    )),
                    None => {
                        regions.insert(
                            name.to_owned(),
                            Region {
                                path: path.to_owned(),
                                language,
                                parts: vec![(first, lines)],
                                first_line: first,
                                last_line: last,
                            },
                        );
                    }
                }
            }
            None => {
                for (_, lines) in open.values_mut() {
                    lines.push(line.to_owned());
                }
            }
        }
    }
    for (name, (first, _)) in open {
        problems.push(format!(
            "{path}:{}: `[explain:{name}]` is never closed",
            first - 1
        ));
    }
}
