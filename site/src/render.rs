//! Markdown to HTML for the site: anchors on headings and a table of
//! contents, doctest setup lines (`# …`) left out of Rust examples, and
//! links between the repository's files turned into the site's addresses.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use serde::Serialize;

use crate::content::{PAGES, Page, page_for_path};
use crate::highlight;
use crate::icons;

/// The repository, for files that aren't pages.
pub const REPOSITORY: &str = "https://github.com/renox-rs/renox";
const RAW: &str = "https://raw.githubusercontent.com/renox-rs/renox/main";

/// A heading in a page's table of contents.
#[derive(Debug, Clone, Serialize)]
pub struct Heading {
    pub level: u8,
    pub id: String,
    pub text: String,
}

/// A page, rendered once.
#[derive(Debug, Clone, Serialize)]
pub struct Rendered {
    /// The first `# heading`, else the sidebar name.
    pub title: String,
    pub html: String,
    pub toc: Vec<Heading>,
    /// The words of the page (no code), for search.
    #[serde(skip)]
    pub text: String,
    /// About how long reading it takes, in minutes.
    pub minutes: usize,
}

/// Every page, rendered at first use.
pub static RENDERED: LazyLock<HashMap<&'static str, Rendered>> =
    LazyLock::new(|| PAGES.iter().map(|page| (page.slug, render(page))).collect());

/// `page` as HTML.
pub fn render(page: &Page) -> Rendered {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        // `> [!TIP]` callouts, as GitHub shows them.
        | Options::ENABLE_GFM;
    let events: Vec<Event> = Parser::new_ext(page.markdown, options).collect();
    let mut out: Vec<Event> = Vec::with_capacity(events.len());
    let mut title = None;
    let mut toc = Vec::new();
    let mut ids = HashSet::new();
    let mut text = String::new();
    // The code block being read: its language and its text so far.
    let mut code: Option<(String, String)> = None;
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::Heading { level, .. }) => {
                let level = *level;
                let mut end = i + 1;
                while !matches!(events[end], Event::End(TagEnd::Heading(_))) {
                    end += 1;
                }
                let inner: Vec<Event> = events[i + 1..end].to_vec();
                let plain = plain_text(&inner);
                text.push_str(&plain);
                text.push('\n');
                if level == HeadingLevel::H1 && title.is_none() {
                    // Shown in the page header instead.
                    title = Some(plain);
                    i = end + 1;
                    continue;
                }
                let id = unique_id(&slugify(&plain), &mut ids);
                let number = heading_number(level);
                if number <= 3 {
                    toc.push(Heading {
                        level: number,
                        id: id.clone(),
                        text: plain,
                    });
                }
                out.push(Event::Html(CowStr::from(format!(
                    "<h{number} id=\"{id}\"><a class=\"site-anchor\" href=\"#{id}\" aria-label=\"Link to this section\">#</a>"
                ))));
                out.extend(inner.into_iter().map(|event| rewrite(event, page.path)));
                out.push(Event::Html(CowStr::from(format!("</h{number}>"))));
                i = end + 1;
                continue;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split([',', ' ']).next().unwrap_or_default().to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                // A fence with no language is a doctest: Rust.
                let language = if language.is_empty() && is_doctest(kind) {
                    "rust".into()
                } else if language.is_empty() {
                    "text".into()
                } else {
                    language
                };
                code = Some((language, String::new()));
            }
            Event::Text(body) if code.is_some() => {
                if let Some((_, text)) = code.as_mut() {
                    text.push_str(body);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, text)) = code.take() {
                    let text = if language == "rust" {
                        without_hidden_lines(&text)
                    } else {
                        text
                    };
                    out.push(Event::Html(CowStr::from(code_panel(&language, &text))));
                }
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                let html = match callout(*kind) {
                    Some((class, icon, title)) => format!(
                        "<div class=\"site-callout site-callout--{class}\">\
                         <p class=\"site-callout__title\">{}{title}</p>",
                        icons::svg(icon)
                    ),
                    None => "<div class=\"site-callout site-callout--quote\">".to_owned(),
                };
                out.push(Event::Html(CowStr::from(html)));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                out.push(Event::Html(CowStr::from("</div>")));
            }
            Event::Text(words) | Event::Code(words) => {
                text.push_str(words);
                text.push(' ');
                out.push(events[i].clone());
            }
            event => out.push(rewrite(event.clone(), page.path)),
        }
        i += 1;
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, out.into_iter());
    // About 200 words a minute, and longer for code.
    let words = text.split_whitespace().count() + code_lines(page.markdown) * 3;
    Rendered {
        title: title.unwrap_or_else(|| page.nav.to_owned()),
        html,
        toc,
        text,
        minutes: (words / 200).max(1),
    }
}

/// How many lines of code a Markdown file has, roughly (its fenced lines).
fn code_lines(markdown: &str) -> usize {
    let mut fenced = false;
    let mut lines = 0;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if fenced {
            lines += 1;
        }
    }
    lines
}

/// A code sample as a panel: a bar with its language and a copy button
/// (shown by site.js when the browser can copy), then the coloured code.
pub fn code_panel(language: &str, code: &str) -> String {
    let code = code.strip_suffix('\n').unwrap_or(code);
    let label = highlight::label(language);
    let icon = if label == "Terminal" || label == "Console" {
        "terminal"
    } else {
        "code"
    };
    format!(
        "<div class=\"site-code\" data-language=\"{language}\">\
         <div class=\"site-code__bar\"><span class=\"site-code__lang\">{}{label}</span>\
         <button class=\"site-code__copy\" type=\"button\" data-copy hidden>{}{}\
         <span class=\"site-code__copy-label\" aria-live=\"polite\">Copy</span></button></div>\
         <pre><code class=\"language-{language}\">{}</code></pre></div>",
        icons::svg(icon),
        icons::svg("copy"),
        icons::svg("check"),
        highlight::highlight(language, code),
    )
}

/// A callout's class, icon and title: GitHub's `> [!NOTE]` kinds. A plain
/// quote has none.
fn callout(kind: Option<BlockQuoteKind>) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match kind? {
        BlockQuoteKind::Tip => ("tip", "lightbulb", "Tip"),
        BlockQuoteKind::Important => ("important", "circle-alert", "Important"),
        BlockQuoteKind::Warning => ("warning", "triangle-alert", "Watch out"),
        BlockQuoteKind::Caution => ("caution", "octagon-alert", "Careful"),
        BlockQuoteKind::Note => ("note", "info", "Note"),
    })
}

fn heading_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn is_doctest(kind: &CodeBlockKind) -> bool {
    matches!(kind, CodeBlockKind::Fenced(info) if info.is_empty())
}

/// The text of a heading's events.
fn plain_text(events: &[Event]) -> String {
    let mut text = String::new();
    for event in events {
        if let Event::Text(t) | Event::Code(t) = event {
            text.push_str(t);
        }
    }
    text.trim().to_owned()
}

/// `Forms and validation` → `forms-and-validation`, as GitHub makes anchors.
pub fn slugify(text: &str) -> String {
    let mut slug = String::new();
    for c in text.trim().to_lowercase().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            slug.push(c);
        } else if c.is_whitespace() {
            slug.push('-');
        }
    }
    if slug.is_empty() {
        "section".into()
    } else {
        slug
    }
}

fn unique_id(slug: &str, ids: &mut HashSet<String>) -> String {
    if ids.insert(slug.to_owned()) {
        return slug.to_owned();
    }
    (1..)
        .map(|n| format!("{slug}-{n}"))
        .find(|candidate| ids.insert(candidate.clone()))
        .expect("an unused id")
}

/// A doctest's setup lines (`# use …`, `#` alone) aren't shown, as rustdoc
/// hides them; `##` stands for a line starting with `#`.
fn without_hidden_lines(code: &str) -> String {
    let mut shown = String::with_capacity(code.len());
    for line in code.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("##") {
            let indent = &line[..line.len() - trimmed.len()];
            shown.push_str(indent);
            shown.push_str(&trimmed[1..]);
        } else if trimmed == "#" || trimmed == "#\n" || trimmed.starts_with("# ") {
            continue;
        } else {
            shown.push_str(line);
        }
    }
    shown
}

fn rewrite(event: Event<'static>, from: &str) -> Event<'static> {
    match event {
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: CowStr::from(link_target(&dest_url, from)),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: CowStr::from(image_source(&dest_url, from)),
            title,
            id,
        }),
        other => other,
    }
}

/// Where a link in the file `from` goes on the site: another page, a
/// section of this one, or the repository on GitHub.
pub fn link_target(url: &str, from: &str) -> String {
    if url.starts_with('#') || url.contains("://") || url.starts_with("mailto:") {
        return url.to_owned();
    }
    let (path, anchor) = match url.split_once('#') {
        Some((path, anchor)) => (path, format!("#{anchor}")),
        None => (url, String::new()),
    };
    let resolved = resolve(from, path);
    match page_for_path(&resolved) {
        Some(page) => format!("/docs/{}{anchor}", page.slug),
        None if resolved.is_empty() => format!("{REPOSITORY}{anchor}"),
        None if is_directory(&resolved) => format!("{REPOSITORY}/tree/main/{resolved}{anchor}"),
        None => format!("{REPOSITORY}/blob/main/{resolved}{anchor}"),
    }
}

fn image_source(url: &str, from: &str) -> String {
    if url.contains("://") {
        url.to_owned()
    } else {
        format!("{RAW}/{}", resolve(from, url))
    }
}

/// `path` relative to the file `from`, as a path from the repository's root.
fn resolve(from: &str, path: &str) -> String {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop(); // the file's own name
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// Folders (`examples/hello`) have no extension.
fn is_directory(path: &str) -> bool {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .is_some_and(|name| !name.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_become_site_addresses() {
        assert_eq!(link_target("routing.md", "docs/ui.md"), "/docs/routing");
        assert_eq!(
            link_target("docs/routing.md#guards", "README.md"),
            "/docs/routing#guards"
        );
        assert_eq!(
            link_target("../CHEATSHEET.md", "docs/ui.md"),
            "/docs/cheatsheet"
        );
        assert_eq!(link_target("#models", "README.md"), "#models");
        assert_eq!(
            link_target("examples/hello", "README.md"),
            format!("{REPOSITORY}/tree/main/examples/hello")
        );
        assert_eq!(
            link_target("examples/hello/src/lib.rs", "README.md"),
            format!("{REPOSITORY}/blob/main/examples/hello/src/lib.rs")
        );
        assert_eq!(
            link_target("https://htmx.org", "README.md"),
            "https://htmx.org"
        );
    }

    #[test]
    fn doctest_setup_lines_are_hidden() {
        let code = "# use renox::prelude::*;\nfn main() {}\n    # let x = 1;\n#\n## not hidden\n";
        assert_eq!(without_hidden_lines(code), "fn main() {}\n# not hidden\n");
    }

    #[test]
    fn callouts_and_code_panels() {
        let page = Page {
            slug: "t",
            nav: "T",
            section: crate::content::Section::Start,
            icon: "compass",
            blurb: "",
            path: "docs/t.md",
            markdown: "# T\n\n> [!TIP]\n> Try it.\n\n> Just a quote.\n\n```\n# use x;\nlet a = 1;\n```\n",
        };
        let html = render(&page).html;
        assert!(
            html.contains("<div class=\"site-callout site-callout--tip\">"),
            "{html}"
        );
        assert!(html.contains("Tip</p>"));
        assert!(html.contains("<p>Try it.</p>"));
        assert!(html.contains("<div class=\"site-callout site-callout--quote\">"));
        // A fence with no language is a doctest: Rust, setup lines hidden.
        assert!(html.contains("data-language=\"rust\""));
        assert!(
            html.contains("<span class=\"hl-kw\">let</span> a = <span class=\"hl-num\">1</span>;")
        );
        assert!(!html.contains("use x"));
    }

    #[test]
    fn headings_get_github_style_ids() {
        assert_eq!(slugify("Forms and validation"), "forms-and-validation");
        assert_eq!(slugify("`Valid<T>` reads"), "validt-reads");
        let mut ids = HashSet::new();
        assert_eq!(unique_id("a", &mut ids), "a");
        assert_eq!(unique_id("a", &mut ids), "a-1");
    }
}
