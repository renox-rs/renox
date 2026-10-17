//! #513: a tag nobody uses is exercised by nothing but unit tests. This test lists every
//! `<rx-…>` tag of the component table (`contracts.rs`) and of the plugins
//! (`Registry::component`), and fails when one appears in no template that runs: an example's,
//! a fixture's (tests/browser, the site, the starter kit) or one built into Renox. The guides
//! do not count: their HTML is never rendered, so a tag shown only there is untested.
//!
//! `NOT_SHOWN_YET` is the one place that excuses a tag. Showing a tag on a real page
//! (an example, or a fixture the browser tests use) and deleting its line is how the
//! tag proves itself; the test fails for a line whose tag is used after all.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The tags no example, fixture or built-in template uses yet, each with the reason. Remove a line when
/// the tag is used somewhere real.
const NOT_SHOWN_YET: &[(&str, &str)] = &[
    (
        "rx-signature-pad",
        "added in #53 part 1/5; a fixture or example page shows it in a later part of #53",
    ),
    (
        "rx-rating",
        "added in #54 part 1/6; the bikeshop /about/blocks page shows it in part 5/6 of #54",
    ),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !matches!(&*name, "target" | "node_modules" | ".git") {
                files(&path, extensions, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| extensions.contains(&e.to_string_lossy().as_ref()))
        {
            out.push(path);
        }
    }
}

/// Every string literal `"rx-…"` of `source` (the whole literal is a tag name).
fn tag_literals(source: &str) -> Vec<String> {
    source
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            s.strip_prefix("rx-").is_some_and(|rest| {
                !rest.is_empty()
                    && rest
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            })
        })
        .map(str::to_owned)
        .collect()
}

/// The tags of the component table and of the plugins.
fn all_tags() -> BTreeSet<String> {
    let mut tags = BTreeSet::new();
    let table =
        std::fs::read_to_string(root().join("crates/renox-core/src/components/contracts.rs"))
            .unwrap();
    for line in table.lines() {
        if let Some(rest) = line.trim().strip_prefix("tag: \"rx-")
            && let Some(name) = rest.strip_suffix("\",")
        {
            tags.insert(format!("rx-{name}"));
        }
    }
    // The plugins register theirs with `Component::macro_call`.
    let mut sources = Vec::new();
    files(&root().join("crates"), &["rs"], &mut sources);
    for path in sources {
        let text = path.to_string_lossy().replace('\\', "/");
        if text.contains("crates/renox-core/") || text.contains("/tests/") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        if source.contains("Component::macro_call(") {
            tags.extend(tag_literals(&source));
        }
    }
    tags
}

/// `source` without template comments (`{# #}`, `<!-- -->`), where a tag may be named
/// without being used.
fn without_comments(source: &str) -> String {
    let mut text = source.to_owned();
    for (open, close) in [("{#", "#}"), ("<!--", "-->")] {
        while let Some(start) = text.find(open) {
            let end = text[start..]
                .find(close)
                .map_or(text.len(), |e| start + e + close.len());
            text.replace_range(start..end, "");
        }
    }
    text
}

/// Everything that shows tags in use: the templates of the examples, the fixtures, the site, the
/// starter kit and the framework itself (without their comments).
fn corpus() -> String {
    let root = root();
    let mut templates = Vec::new();
    for dir in [
        "examples",
        "tests",
        "site",
        "www",
        "crates/renox-core/views",
        "crates/renox-2fa/views",
        "crates/renox-admin/views",
        "crates/renox-billing/views",
        "crates/renox-oauth/views",
        "crates/renox-cli/stubs",
    ] {
        // `.txt` too: the text version of a mail is a template (`<rx-mail-text>`).
        files(&root.join(dir), &["html", "txt"], &mut templates);
    }
    let mut text = String::new();
    for path in templates {
        let slash = path.to_string_lossy().replace('\\', "/");
        // Templates only: not a plugin's own editor assets or vendored pages.
        if slash.contains("/public/") || slash.contains("/vendor/") {
            continue;
        }
        text.push_str(&without_comments(&std::fs::read_to_string(path).unwrap()));
        text.push('\n');
    }
    text
}

/// Whether `<tag` opens an element in `text`: the tag ends at a space, `>`, `/` or a line end.
fn used(text: &str, tag: &str) -> bool {
    let open = format!("<{tag}");
    text.match_indices(&open).any(|(at, _)| {
        text[at + open.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/')
    })
}

#[test]
fn every_tag_is_shown_somewhere_or_listed_as_not_yet() {
    let tags = all_tags();
    assert!(tags.len() > 90, "{} tags found", tags.len());
    for editor in [
        "rx-code-entry",
        "rx-markdown-editor",
        "rx-rich-editor",
        "rx-code-editor",
    ] {
        assert!(tags.contains(editor), "plugin tags are covered: {editor}");
    }
    let text = corpus();
    let excused: BTreeSet<&str> = NOT_SHOWN_YET.iter().map(|(t, _)| *t).collect();
    for (tag, reason) in NOT_SHOWN_YET {
        assert!(!reason.is_empty(), "{tag} needs a reason");
        assert!(
            tags.contains(*tag),
            "{tag} is in NOT_SHOWN_YET but is no tag"
        );
    }
    let unshown: Vec<&String> = tags
        .iter()
        .filter(|t| !excused.contains(t.as_str()) && !used(&text, t))
        .collect();
    assert!(
        unshown.is_empty(),
        "no example, fixture or built-in template uses {unshown:?}: show each on a real \
         page (a guide or a comment does not count), or excuse it, with a reason, in NOT_SHOWN_YET"
    );
    let stale: Vec<&&str> = excused.iter().filter(|t| used(&text, t)).collect();
    assert!(
        stale.is_empty(),
        "{stale:?} is used now: delete its line from NOT_SHOWN_YET"
    );
}

#[test]
fn the_tag_finder_reads_templates_the_way_the_compiler_does() {
    assert!(used("<rx-card title=\"x\">", "rx-card"));
    assert!(used("<rx-card>", "rx-card"));
    assert!(used("<rx-card/>", "rx-card"));
    assert!(used("<rx-card\n  title=\"x\">", "rx-card"));
    assert!(!used("<rx-card-grid>", "rx-card"));
    assert!(!used("rx-card", "rx-card"));
    assert!(!used(&without_comments("{# <rx-card> #}<p/>"), "rx-card"));
    assert!(!used(
        &without_comments("<!-- <rx-card> --><p/>"),
        "rx-card"
    ));
    assert_eq!(
        tag_literals("a(\"rx-a-b\", \"rx-C\", \"rx-\", \"x rx-y\")"),
        vec!["rx-a-b".to_owned()]
    );
}
