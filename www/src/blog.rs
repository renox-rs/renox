//! The blog: Markdown files in `content/blog/`, compiled in by build.rs.
//!
//! A post starts with a front matter block between `---` lines, one
//! `key: value` per line:
//!
//! ```text
//! ---
//! title: Why we built Renox
//! description: One sentence for search results and the feed (under 160 characters).
//! date: 2026-10-07
//! updated: 2026-10-08        (optional)
//! author: Arif Rachim        (optional)
//! tags: rust, laravel        (optional, comma separated)
//! ---
//! The post, in Markdown.
//! ```
//!
//! The file name gives the address: `2026-10-07-why-we-built-renox.md` is
//! `/blog/why-we-built-renox` (a leading `YYYY-MM-DD-` is dropped). Posts
//! are newest first; a post with a date in the future is not shown.

use std::sync::LazyLock;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};
use serde::Serialize;

include!(concat!(env!("OUT_DIR"), "/posts.rs"));

/// One post, parsed and rendered.
#[derive(Debug, Clone, Serialize)]
pub struct Post {
    /// `/blog/{slug}`.
    pub slug: String,
    pub title: String,
    /// For search results, the feed and social cards.
    pub description: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// `YYYY-MM-DD`, when it changed after it was published.
    pub updated: Option<String>,
    pub author: String,
    pub tags: Vec<String>,
    /// The body as Markdown (for `/blog/{slug}.md` and llms-full.txt).
    pub markdown: String,
    /// The body as HTML, code highlighted.
    pub html: String,
    /// The second-level headings, for the table of contents: (id, text).
    pub toc: Vec<(String, String)>,
    /// Minutes to read (220 words a minute, at least 1).
    pub minutes: usize,
}

impl Post {
    /// The last day it changed: `updated`, else `date`.
    pub fn modified(&self) -> &str {
        self.updated.as_deref().unwrap_or(&self.date)
    }
}

/// Every post, newest first.
pub static POSTS: LazyLock<Vec<Post>> = LazyLock::new(|| {
    let mut posts: Vec<Post> = POST_FILES
        .iter()
        .map(|(file, text)| {
            parse(file, text).unwrap_or_else(|e| panic!("content/blog/{file}: {e}"))
        })
        .collect();
    posts.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.slug.cmp(&b.slug)));
    posts
});

/// The posts to show today (not those dated in the future).
pub fn published() -> Vec<&'static Post> {
    let today = renox::db::now().format("%Y-%m-%d").to_string();
    POSTS.iter().filter(|p| p.date <= today).collect()
}

/// A published post by its slug.
pub fn find(slug: &str) -> Option<&'static Post> {
    published().into_iter().find(|p| p.slug == slug)
}

/// Every tag of the published posts, with how many posts have it, by name.
pub fn tags() -> Vec<(String, usize)> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for post in published() {
        for tag in &post.tags {
            *counts.entry(tag.clone()).or_default() += 1;
        }
    }
    counts.into_iter().collect()
}

/// A post from its file: the front matter, then the Markdown.
pub fn parse(file: &str, text: &str) -> Result<Post, String> {
    let text = text.replace("\r\n", "\n");
    let rest = text
        .strip_prefix("---\n")
        .ok_or("no front matter (start with ---)")?;
    let (head, body) = rest
        .split_once("\n---\n")
        .ok_or("the front matter has no closing ---")?;
    let mut get = std::collections::HashMap::new();
    for line in head.lines().filter(|l| !l.trim().is_empty()) {
        let (k, v) = line
            .split_once(':')
            .ok_or_else(|| format!("`{line}` is not `key: value`"))?;
        // A value may be quoted (`title: "A: b"`), as YAML allows.
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(v);
        get.insert(k.trim().to_owned(), v.to_owned());
    }
    let need = |k: &str| {
        get.get(k)
            .cloned()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("`{k}` is missing"))
    };
    let date = need("date")?;
    if !is_date(&date) {
        return Err(format!("date `{date}` is not YYYY-MM-DD"));
    }
    let updated = get.get("updated").cloned().filter(|v| !v.is_empty());
    if let Some(u) = &updated
        && !is_date(u)
    {
        return Err(format!("updated `{u}` is not YYYY-MM-DD"));
    }
    let description = need("description")?;
    let stem = file.trim_end_matches(".md");
    let slug = if stem.len() > 11 && is_date(&stem[..10]) && &stem[10..11] == "-" {
        &stem[11..]
    } else {
        stem
    };
    let markdown = body.trim().to_owned();
    let (html, toc) = render(&markdown);
    let words = markdown.split_whitespace().count();
    Ok(Post {
        slug: slug.to_owned(),
        title: need("title")?,
        description,
        date,
        updated,
        author: get
            .get("author")
            .cloned()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "The Renox team".into()),
        tags: get
            .get("tags")
            .map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        markdown,
        html,
        toc,
        minutes: (words / 220).max(1),
    })
}

fn is_date(s: &str) -> bool {
    s.len() == 10
        && s.chars().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Markdown as HTML: headings get ids (for links and the table of
/// contents), code blocks are highlighted like the docs, links to other
/// sites open with `rel="noopener"`. Raw HTML in a post is shown as text.
pub fn render(markdown: &str) -> (String, Vec<(String, String)>) {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_SMART_PUNCTUATION;
    let events: Vec<Event> = Parser::new_ext(markdown, options).collect();
    let mut out: Vec<Event> = Vec::with_capacity(events.len());
    let mut toc = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::Heading { level, .. }) => {
                // The heading's text, to make its id.
                let mut text = String::new();
                let mut j = i + 1;
                while j < events.len() && !matches!(events[j], Event::End(TagEnd::Heading(_))) {
                    if let Event::Text(t) | Event::Code(t) = &events[j] {
                        text.push_str(t);
                    }
                    j += 1;
                }
                let id = renox_site::render::slugify(&text);
                // A post's own title is the page's h1: headings start at h2.
                let level = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    _ => 4,
                };
                if level == 2 {
                    toc.push((id.clone(), text.clone()));
                }
                let mut inner = String::new();
                html::push_html(&mut inner, events[i + 1..j].iter().cloned());
                out.push(Event::Html(
                    format!("<h{level} id=\"{id}\"><a class=\"anchor\" href=\"#{id}\" aria-hidden=\"true\" tabindex=\"-1\">#</a>{inner}</h{level}>\n").into(),
                ));
                i = j + 1;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split([',', ' ']).next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                let mut code = String::new();
                let mut j = i + 1;
                while j < events.len() && !matches!(events[j], Event::End(TagEnd::CodeBlock)) {
                    if let Event::Text(t) = &events[j] {
                        code.push_str(t);
                    }
                    j += 1;
                }
                let label = if language.is_empty() {
                    ""
                } else {
                    renox_site::highlight::label(&language)
                };
                out.push(Event::Html(
                    format!(
                        "<figure class=\"code\"><figcaption>{label}</figcaption><pre><code class=\"language-{lang}\">{body}</code></pre></figure>\n",
                        lang = escape(&language),
                        body = renox_site::highlight::highlight(&language, code.trim_end()),
                    )
                    .into(),
                ));
                i = j + 1;
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) if dest_url.starts_with("http") && !dest_url.starts_with("https://renox.rs") => {
                let title_attr = if title.is_empty() {
                    String::new()
                } else {
                    format!(" title=\"{}\"", escape(title))
                };
                out.push(Event::Html(
                    format!(
                        "<a href=\"{}\"{title_attr} rel=\"noopener\">",
                        escape(dest_url)
                    )
                    .into(),
                ));
                i += 1;
            }
            Event::Html(raw) | Event::InlineHtml(raw) => {
                out.push(Event::Text(raw.clone()));
                i += 1;
            }
            other => {
                out.push(other.clone());
                i += 1;
            }
        }
    }
    let mut html_out = String::new();
    html::push_html(&mut html_out, out.into_iter());
    (html_out, toc)
}

/// HTML-escaped text.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const POST: &str = "---\ntitle: Hello\ndescription: A first post.\ndate: 2026-10-07\ntags: Rust, laravel\n---\nIntro.\n\n## A part\n\n```rust\nlet x = 1;\n```\n\n<script>alert(1)</script>\n\n[docs](https://docs.renox.rs)\n";

    #[test]
    fn a_post_is_parsed_and_rendered() {
        let post = parse("2026-10-07-hello-world.md", POST).unwrap();
        assert_eq!(post.slug, "hello-world");
        assert_eq!(post.title, "Hello");
        assert_eq!(post.tags, ["rust", "laravel"]);
        assert_eq!(post.toc, [("a-part".to_owned(), "A part".to_owned())]);
        assert!(post.html.contains("<h2 id=\"a-part\">"));
        assert!(
            post.html.contains("class=\"hl-kw\">let</span>"),
            "{}",
            post.html
        );
        assert!(post.html.contains("&lt;script&gt;"), "raw HTML is text");
        assert!(post.html.contains("rel=\"noopener\""));
    }

    #[test]
    fn bad_front_matter_is_refused() {
        assert!(parse("x.md", "no front matter").is_err());
        assert!(
            parse("x.md", "---\ntitle: T\ndate: 2026-10-07\n---\nbody")
                .unwrap_err()
                .contains("description")
        );
        assert!(
            parse(
                "x.md",
                "---\ntitle: T\ndescription: D\ndate: 7 Oct\n---\nbody"
            )
            .unwrap_err()
            .contains("YYYY-MM-DD")
        );
    }

    #[test]
    fn every_post_in_the_repository_parses() {
        assert!(!POSTS.is_empty());
        for post in POSTS.iter() {
            assert!(
                post.description.chars().count() <= 160,
                "{}: description over 160 characters",
                post.slug
            );
            assert!(
                post.title.chars().count() <= 70,
                "{}: title over 70 characters",
                post.slug
            );
        }
    }
}
