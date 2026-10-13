//! HTML from a rich text editor, cleaned on the server.
//!
//! A browser can send anything in a form field, whatever the editor on the
//! page allows, so the HTML is cleaned here before it is stored or shown:
//! only formatting tags stay (paragraphs, bold, italic, struck text,
//! headings, quotes, code, lists and links), links keep only `http`,
//! `https`, `mailto` and `tel` addresses, and every other tag, attribute,
//! style and script is removed.
//!
//! ```
//! use renox_editors::{RichText, sanitize};
//!
//! let clean = sanitize(r#"<p onclick="steal()">Hi <b>there</b><script>steal()</script></p>"#);
//! assert_eq!(clean, "<p>Hi <b>there</b></p>");
//!
//! let body = RichText::new(r#"<a href="javascript:alert(1)">click</a>"#);
//! assert_eq!(body.as_str(), r#"<a rel="noopener noreferrer nofollow">click</a>"#);
//! assert_eq!(body.text(), "click");
//! ```

use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

use renox::db::{DbValue, ToDbValue};
use renox::validation::{FieldValue, Inspected};
use serde::{Deserialize, Deserializer, Serialize};

/// The tags the rich text editor writes, and the few a pasted document
/// commonly has (`p`, `b`, `i`, `s`, `h2`, `h3`, `code`).
const TAGS: &[&str] = &[
    "a",
    "b",
    "blockquote",
    "br",
    "code",
    "del",
    "div",
    "em",
    "h1",
    "h2",
    "h3",
    "i",
    "li",
    "ol",
    "p",
    "pre",
    "s",
    "strong",
    "u",
    "ul",
];

/// The addresses a link may have.
const SCHEMES: &[&str] = &["http", "https", "mailto", "tel"];

static CLEANER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut builder = ammonia::Builder::empty();
    builder
        .tags(TAGS.iter().copied().collect::<HashSet<_>>())
        .tag_attributes(
            [("a", ["href", "title"].into_iter().collect::<HashSet<_>>())]
                .into_iter()
                .collect(),
        )
        .url_schemes(SCHEMES.iter().copied().collect())
        // Links to other sites don't pass on the page they were found on,
        // and don't vouch for spam in search results.
        .link_rel(Some("noopener noreferrer nofollow"))
        .strip_comments(true)
        // Relative links (`/pricing`) stay; `//other.site` is a host.
        .url_relative(ammonia::UrlRelative::PassThrough);
    // What's inside these is dropped too, not shown as text.
    builder.clean_content_tags(
        ["script", "style", "template", "textarea"]
            .into_iter()
            .collect(),
    );
    builder
});

/// `html` with only formatting left: the tags of a rich text editor, links
/// to `http`, `https`, `mailto` and `tel` addresses, and nothing that runs
/// (no scripts, event handlers, styles, frames or forms). Safe to show as
/// it is.
///
/// ```
/// use renox_editors::sanitize;
///
/// assert_eq!(sanitize(r#"<img src=x onerror="alert(1)">Hi"#), "Hi");
/// assert_eq!(sanitize("<h1>Title</h1><ul><li>one</li></ul>"), "<h1>Title</h1><ul><li>one</li></ul>");
/// ```
pub fn sanitize(html: &str) -> String {
    CLEANER.clean(html).to_string()
}

/// HTML written in a rich text editor (`rich_editor` in a form), cleaned
/// with [`sanitize`] as it is read: a form field of this type never holds
/// markup that runs.
///
/// ```
/// use renox::prelude::*;
/// use renox_editors::RichText;
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Validate)]
/// struct PostForm {
///     #[validate(required, max = 20000)]
///     title: String,
///     /// `required` and `max` count the text's letters, not its tags.
///     #[validate(required, max = 20000)]
///     body: RichText,
/// }
///
/// async fn store(Valid(form): Valid<PostForm>) -> Result<Redirect> {
///     let body: String = form.body.into_string(); // store it as text
///     # let _ = body;
///     Ok(Redirect::to("/posts"))
/// }
/// ```
///
/// Store it as text (`into_string`, or the `ToDbValue` it implements in a
/// query) and show it with the `rich_text` filter, which cleans it again on
/// the way out: `{{ post.body | rich_text }}`.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RichText(String);

impl RichText {
    /// `html`, cleaned.
    pub fn new(html: &str) -> Self {
        Self(sanitize(html))
    }

    /// The cleaned HTML.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The cleaned HTML, to store.
    pub fn into_string(self) -> String {
        self.0
    }

    /// The words without the tags: for a search index, an excerpt or a
    /// mail's text part. Blocks and line breaks become line breaks.
    pub fn text(&self) -> String {
        text_of(&self.0)
    }

    /// Whether there is no text (an emptied editor still sends `<div><br></div>`).
    pub fn is_empty(&self) -> bool {
        self.text().trim().is_empty()
    }
}

impl fmt::Debug for RichText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RichText").field(&self.0).finish()
    }
}

impl fmt::Display for RichText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<RichText> for String {
    fn from(text: RichText) -> Self {
        text.0
    }
}

impl From<&str> for RichText {
    fn from(html: &str) -> Self {
        Self::new(html)
    }
}

impl From<String> for RichText {
    fn from(html: String) -> Self {
        Self::new(&html)
    }
}

impl<'de> Deserialize<'de> for RichText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let html = String::deserialize(deserializer)?;
        Ok(Self::new(&html))
    }
}

/// Rules see the text: `required` fails for an emptied editor, and
/// `min`/`max` count letters, not tags.
impl FieldValue for RichText {
    fn inspect(&self) -> Inspected {
        let text = self.text();
        if text.trim().is_empty() {
            Inspected::Missing
        } else {
            Inspected::Text(text)
        }
    }

    fn db_value(&self) -> DbValue {
        self.0.to_db_value()
    }
}

impl ToDbValue for RichText {
    fn to_db_value(&self) -> DbValue {
        self.0.to_db_value()
    }
}

/// The text of cleaned HTML: tags removed, the entities the cleaner
/// writes decoded, a line break for each block and `<br>`.
fn text_of(html: &str) -> String {
    const BLOCKS: &[&str] = &[
        "br",
        "div",
        "p",
        "li",
        "h1",
        "h2",
        "h3",
        "blockquote",
        "pre",
        "ul",
        "ol",
    ];
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(['<', '&']) {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if rest.starts_with('<') {
            let end = rest.find('>').map_or(rest.len(), |i| i + 1);
            let name: String = rest[1..end]
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            if BLOCKS.contains(&name.as_str()) && !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            rest = &rest[end..];
        } else {
            let end = rest.find(';').filter(|&i| i <= 10);
            let decoded = end.and_then(|end| entity(&rest[1..end]));
            match (end, decoded) {
                (Some(end), Some(c)) => {
                    out.push(c);
                    rest = &rest[end + 1..];
                }
                _ => {
                    out.push('&');
                    rest = &rest[1..];
                }
            }
        }
    }
    out.push_str(rest);
    out.trim_end().to_owned()
}

/// The character an entity (`amp`, `#39`, `#x27`) stands for.
fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => {
            let code = name.strip_prefix('#')?;
            let n = match code.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => code.parse().ok()?,
            };
            char::from_u32(n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_and_handlers_are_removed() {
        let payloads = [
            ("<script>alert(1)</script>ok", "ok"),
            (r#"<img src=x onerror=alert(1)>ok"#, "ok"),
            (r#"<div onmouseover="alert(1)">ok</div>"#, "<div>ok</div>"),
            (
                r#"<a href="javascript:alert(1)">x</a>"#,
                r#"<a rel="noopener noreferrer nofollow">x</a>"#,
            ),
            (
                r#"<a href=" JaVaScRiPt:alert(1)">x</a>"#,
                r#"<a rel="noopener noreferrer nofollow">x</a>"#,
            ),
            (
                r#"<a href="data:text/html,<script>alert(1)</script>">x</a>"#,
                r#"<a rel="noopener noreferrer nofollow">x</a>"#,
            ),
            (r#"<iframe src="https://evil.example"></iframe>ok"#, "ok"),
            (r#"<svg><script>alert(1)</script></svg>ok"#, "ok"),
            (
                r#"<p style="background:url(javascript:alert(1))">ok</p>"#,
                "<p>ok</p>",
            ),
            (r#"<form action="/x"><input name=a></form>ok"#, "ok"),
            ("<!-- <script>alert(1)</script> -->ok", "ok"),
            (r#"<style>body{display:none}</style>ok"#, "ok"),
        ];
        for (dirty, clean) in payloads {
            assert_eq!(sanitize(dirty), clean, "{dirty}");
        }
    }

    #[test]
    fn formatting_and_safe_links_stay() {
        let html = r#"<div>Hello <strong>bold</strong> <em>it</em> <del>old</del><br></div><h1>Title</h1><blockquote>q</blockquote><pre>let x = 1;</pre><ul><li>a</li></ul><ol><li>b</li></ol>"#;
        assert_eq!(sanitize(html), html);
        assert_eq!(
            sanitize(r#"<a href="https://renox.rs" title="Docs" class="x">docs</a>"#),
            r#"<a href="https://renox.rs" title="Docs" rel="noopener noreferrer nofollow">docs</a>"#
        );
        assert_eq!(
            sanitize(r#"<a href="/pricing">p</a>"#),
            r#"<a href="/pricing" rel="noopener noreferrer nofollow">p</a>"#
        );
    }

    #[test]
    fn text_counts_letters_not_tags() {
        let body = RichText::new("<div>Fish &amp; chips<br>at 5 &lt; 6</div><ul><li>one</li></ul>");
        assert_eq!(body.text(), "Fish & chips\nat 5 < 6\none");
        assert!(RichText::new("<div><br></div>").is_empty());
        assert_eq!(
            RichText::new("<div><br></div>").inspect(),
            Inspected::Missing
        );
        assert_eq!(
            RichText::new("<b>Hi</b>").inspect(),
            Inspected::Text("Hi".into())
        );
    }

    #[test]
    fn deserializing_cleans() {
        let body: RichText =
            renox::serde_json::from_str(r#""<b onclick=x>Hi</b><script>x</script>""#).unwrap();
        assert_eq!(body.as_str(), "<b>Hi</b>");
        assert_eq!(
            renox::serde_json::to_string(&body).unwrap(),
            r#""<b>Hi</b>""#
        );
    }

    // #261: every entity form the plain text understands, what it keeps as
    // written, and the conversions.
    #[test]
    fn entities_in_plain_text() {
        let html = "<p>&gt; &quot;a&quot; &apos;b&apos; x&nbsp;y &#39;c&#39; &#x27;d&#X27; &bogus; AT&T &averyveryverylongname;</p>";
        assert_eq!(
            RichText::new(html).text(),
            "> \"a\" 'b' x\u{a0}y 'c' 'd' &bogus; AT&T &averyveryverylongname;"
        );
        assert_eq!(text_of("&#xZZ; &#;"), "&#xZZ; &#;");
    }

    #[test]
    fn conversions_debug_display_and_storage() {
        let from_str: RichText = "<p>Hi<script>x</script></p>".into();
        let from_string: RichText = String::from("<p>Hi</p>").into();
        assert_eq!(from_str.as_str(), from_string.as_str(), "both sanitized");
        assert_eq!(from_str.to_string(), "<p>Hi</p>");
        assert!(format!("{from_str:?}").contains("<p>Hi</p>"));
        let plain: String = from_str.clone().into();
        assert_eq!(plain, from_str.clone().into_string());
        assert!(matches!(from_str.to_db_value(), renox::db::DbValue::Text(t) if t == "<p>Hi</p>"));
    }
}
