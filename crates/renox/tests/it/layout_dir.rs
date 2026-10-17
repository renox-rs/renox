//! Every layout the repository ships (built-in views, CLI stubs, examples and
//! the browser fixture) writes `dir` on `<html>`, so a page in a right-to-left
//! locale flips by itself (#28). The English-only error and debug pages say
//! `dir="ltr"` on purpose. `rnx new` output is checked in the CLI's own tests.

use std::path::{Path, PathBuf};

/// The files that are English-only on purpose and must say `dir="ltr"`.
const ENGLISH_ONLY: &[&str] = &[
    "crates/renox-core/views/error.html",
    "crates/renox-core/views/debug.html",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.html` file under `dir`, skipping build output and vendored code.
fn html_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | "node_modules" | "vendor" | ".git") {
                html_files(&path, out);
            }
        } else if name.ends_with(".html") {
            out.push(path);
        }
    }
}

#[test]
fn every_shipped_layout_writes_dir() {
    let root = root();
    let mut files = Vec::new();
    for top in ["crates", "examples", "tests/browser/fixture"] {
        html_files(&root.join(top), &mut files);
    }
    let mut layouts = 0;
    let mut missing = Vec::new();
    for path in files {
        let html = std::fs::read_to_string(&path).unwrap();
        let Some(at) = html.find("<html") else {
            continue;
        };
        layouts += 1;
        let tag = &html[at..at + html[at..].find('>').unwrap()];
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let want = if ENGLISH_ONLY.contains(&relative.as_str()) {
            "dir=\"ltr\""
        } else {
            "dir=\"{{ app.dir }}\""
        };
        if !tag.contains(want) {
            missing.push(format!("{relative}: {tag}> lacks {want}"));
        }
    }
    assert!(layouts >= 20, "the walk finds the layouts ({layouts})");
    assert!(
        missing.is_empty(),
        "layouts without the right dir:\n{}",
        missing.join("\n")
    );
}
