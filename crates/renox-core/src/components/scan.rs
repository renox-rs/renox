//! The tag scanner: template source in, text / start tags / end tags out. It does not understand
//! HTML; it only finds tags, and treats MiniJinja syntax, comments, `{% raw %}` and the content
//! of `<script>`, `<style>`, `<rx-script>` and `<rx-style>` as opaque text.

use std::ops::Range;

use super::CompileError;

/// One attribute of a start tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Attr<'a> {
    /// The name as written.
    pub name: &'a str,
    /// The value without its quotes; `None` for a bare attribute.
    pub value: Option<&'a str>,
    /// Whether the value was in quotes.
    pub quoted: bool,
    /// Where the attribute is in the source.
    pub span: Range<usize>,
    /// The line it starts on.
    pub line: usize,
}

/// A piece of the template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    /// Anything that is not a tag.
    Text(Range<usize>),
    /// A start tag.
    Open {
        /// The tag name, lower-cased.
        name: String,
        /// Its attributes.
        attrs: Vec<Attr<'a>>,
        /// Written as `<x />`.
        self_closing: bool,
        /// The whole tag.
        span: Range<usize>,
        /// The line it starts on.
        line: usize,
    },
    /// An end tag.
    Close {
        /// The tag name, lower-cased.
        name: String,
        /// The whole tag.
        span: Range<usize>,
        /// The line it starts on.
        line: usize,
    },
}

struct Scanner<'a> {
    src: &'a str,
    lines: Vec<usize>,
}

impl<'a> Scanner<'a> {
    fn line(&self, offset: usize) -> usize {
        self.lines.partition_point(|&s| s <= offset)
    }

    fn err(&self, offset: usize, message: &str) -> CompileError {
        CompileError {
            line: self.line(offset),
            message: message.to_string(),
        }
    }

    /// The end (exclusive) of the `{{`, `{%` or `{#` block starting at `at`, if one starts there.
    fn jinja_end(&self, at: usize) -> Result<Option<usize>, CompileError> {
        // Callers step byte by byte, so `at` may fall inside a multi-byte character.
        let Some(rest) = self.src.get(at..) else {
            return Ok(None);
        };
        let close = if rest.starts_with("{{") {
            "}}"
        } else if rest.starts_with("{%") {
            "%}"
        } else if rest.starts_with("{#") {
            "#}"
        } else {
            return Ok(None);
        };
        match rest[2..].find(close) {
            Some(i) => Ok(Some(at + 2 + i + 2)),
            None => Err(self.err(at, &format!("unterminated `{}`", &rest[..2]))),
        }
    }

    /// If a `{% raw %}` starts at `at`, the end of its `{% endraw %}`.
    fn raw_end(&self, at: usize, block_end: usize) -> Result<Option<usize>, CompileError> {
        let inner = self.src[at + 2..block_end - 2]
            .trim_matches(['-', '+'])
            .trim();
        if !self.src[at..].starts_with("{%") || inner != "raw" {
            return Ok(None);
        }
        let mut from = block_end;
        while let Some(i) = self.src[from..].find("{%") {
            let start = from + i;
            let Some(end) = self.jinja_end(start)? else {
                break;
            };
            if self.src[start + 2..end - 2].trim_matches(['-', '+']).trim() == "endraw" {
                return Ok(Some(end));
            }
            from = end;
        }
        Err(self.err(at, "`{% raw %}` has no `{% endraw %}`"))
    }

    fn skip_ws(&self, mut i: usize) -> usize {
        let b = self.src.as_bytes();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    }

    /// Scans a tag from `start` (at `<`). Returns the token and where it ends.
    fn tag(&self, start: usize) -> Result<(Token<'a>, usize), CompileError> {
        let b = self.src.as_bytes();
        let closing = b[start + 1] == b'/';
        let name_start = start + if closing { 2 } else { 1 };
        let mut i = name_start;
        while i < b.len()
            && !b[i].is_ascii_whitespace()
            && !matches!(b[i], b'/' | b'>' | b'{' | b'=')
        {
            i += 1;
        }
        let name = self.src[name_start..i].to_ascii_lowercase();
        let line = self.line(start);
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            i = self.skip_ws(i);
            if i >= b.len() {
                return Err(self.err(
                    start,
                    &format!("the tag `<{name}` is never closed with `>`"),
                ));
            }
            if b[i] == b'>' {
                i += 1;
                break;
            }
            if b[i] == b'/' && b.get(i + 1) == Some(&b'>') {
                self_closing = true;
                i += 2;
                break;
            }
            if let Some(end) = self.jinja_end(i)? {
                i = end;
                continue;
            }
            if b[i] == b'/' {
                i += 1;
                continue;
            }
            // An attribute.
            let a_start = i;
            let n_start = i;
            if b[i] == b'=' {
                i += 1;
            }
            while i < b.len()
                && !b[i].is_ascii_whitespace()
                && !matches!(b[i], b'=' | b'>' | b'{')
                && !(b[i] == b'/' && b.get(i + 1) == Some(&b'>'))
            {
                i += 1;
            }
            if i == n_start {
                // A lone `{` that starts no Jinja block.
                i += 1;
            }
            let a_name = &self.src[n_start..i];
            let after = self.skip_ws(i);
            let mut value = None;
            let mut quoted = false;
            if b.get(after) == Some(&b'=') {
                let v = self.skip_ws(after + 1);
                match b.get(v) {
                    Some(&q @ (b'"' | b'\'')) => {
                        let mut j = v + 1;
                        loop {
                            if j >= b.len() {
                                return Err(self.err(v, "an attribute value is never closed"));
                            }
                            if let Some(end) = self.jinja_end(j)? {
                                j = end;
                            } else if b[j] == q {
                                break;
                            } else {
                                j += 1;
                            }
                        }
                        value = Some(&self.src[v + 1..j]);
                        quoted = true;
                        i = j + 1;
                    }
                    Some(_) => {
                        let mut j = v;
                        while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                            if let Some(end) = self.jinja_end(j)? {
                                j = end;
                            } else {
                                j += 1;
                            }
                        }
                        value = Some(&self.src[v..j]);
                        i = j;
                    }
                    None => i = after + 1,
                }
            }
            attrs.push(Attr {
                name: a_name,
                value,
                quoted,
                span: a_start..i,
                line: self.line(a_start),
            });
        }
        let span = start..i;
        let token = if closing {
            Token::Close { name, span, line }
        } else {
            Token::Open {
                name,
                attrs,
                self_closing,
                span,
                line,
            }
        };
        Ok((token, i))
    }
}

/// Splits `src` into text, start tags and end tags.
pub(crate) fn scan(src: &str) -> Result<Vec<Token<'_>>, CompileError> {
    let mut lines = vec![0];
    lines.extend(src.match_indices('\n').map(|(i, _)| i + 1));
    let s = Scanner { src, lines };
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut text_from = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' => {
                if let Some(end) = s.jinja_end(i)? {
                    i = s.raw_end(i, end)?.unwrap_or(end);
                } else {
                    i += 1;
                }
            }
            b'<' => {
                let rest = &src[i..];
                if rest.starts_with("<!--") {
                    match rest.find("-->") {
                        Some(n) => i += n + 3,
                        None => return Err(s.err(i, "unterminated `<!--`")),
                    }
                } else if rest.starts_with("<!") || rest.starts_with("<?") {
                    i += rest.find('>').map_or(rest.len(), |n| n + 1);
                } else {
                    let next = b.get(i + 1).copied().unwrap_or(0);
                    let named = next.is_ascii_alphabetic()
                        || (next == b'/' && b.get(i + 2).is_some_and(u8::is_ascii_alphabetic));
                    if !named {
                        i += 1;
                        continue;
                    }
                    let (token, end) = s.tag(i)?;
                    if i > text_from {
                        out.push(Token::Text(text_from..i));
                    }
                    let opaque = match &token {
                        Token::Open {
                            name,
                            self_closing: false,
                            ..
                        } if matches!(
                            name.as_str(),
                            "script" | "style" | "rx-script" | "rx-style"
                        ) =>
                        {
                            Some(name.clone())
                        }
                        _ => None,
                    };
                    out.push(token);
                    i = end;
                    text_from = end;
                    if let Some(name) = opaque {
                        let close = format!("</{name}");
                        let lower = src[end..].to_ascii_lowercase();
                        let at = lower.find(&close).map_or(src.len(), |n| end + n);
                        i = at;
                    }
                }
            }
            _ => i += 1,
        }
    }
    if text_from < src.len() {
        out.push(Token::Text(text_from..src.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn non_ascii_attribute_values_scan() {
        let toks = scan(r#"<rx-menu-action action="/x" label="Español"/>"#).unwrap();
        assert_eq!(toks.len(), 1);
    }

    use super::*;

    fn kinds(src: &str) -> Vec<String> {
        scan(src)
            .unwrap()
            .iter()
            .map(|t| match t {
                Token::Text(_) => "text".to_string(),
                Token::Open { name, .. } => format!("open {name}"),
                Token::Close { name, .. } => format!("close {name}"),
            })
            .collect()
    }

    #[test]
    fn open_text_close() {
        let src = r#"<rx-card title="A">x</rx-card>"#;
        assert_eq!(kinds(src), ["open rx-card", "text", "close rx-card"]);
        let Token::Open { attrs, .. } = &scan(src).unwrap()[0] else {
            panic!()
        };
        assert_eq!(attrs[0].name, "title");
        assert_eq!(attrs[0].value, Some("A"));
        assert!(attrs[0].quoted);
    }

    #[test]
    fn self_closing() {
        let toks = scan(r#"<rx-badge text="x" />"#).unwrap();
        assert_eq!(toks.len(), 1);
        assert!(matches!(
            toks[0],
            Token::Open {
                self_closing: true,
                ..
            }
        ));
    }

    #[test]
    fn jinja_blocks_hold_no_tags() {
        assert_eq!(
            kinds("{% if a < b %}<p>{% endif %}"),
            ["text", "open p", "text"]
        );
        assert_eq!(kinds("{{ a < b }}{# <p> #}<!-- <p> -->"), ["text"]);
        assert_eq!(kinds("{% raw %}<p>{% endraw %}<b>"), ["text", "open b"]);
    }

    #[test]
    fn jinja_inside_a_tag() {
        let toks = scan("<input{% if x %} disabled{% endif %}>").unwrap();
        assert_eq!(toks.len(), 1);
        let Token::Open { attrs, .. } = &toks[0] else {
            panic!()
        };
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs[0].name, "disabled");
    }

    #[test]
    fn expression_in_a_value() {
        let toks = scan(r#"<rx-x title="{{ a > b }}" n=3 u='q' bare>"#).unwrap();
        assert_eq!(toks.len(), 1);
        let Token::Open { attrs, .. } = &toks[0] else {
            panic!()
        };
        assert_eq!(attrs[0].value, Some("{{ a > b }}"));
        assert_eq!(attrs[1].value, Some("3"));
        assert!(!attrs[1].quoted);
        assert_eq!(attrs[2].value, Some("q"));
        assert_eq!(attrs[3].value, None);
    }

    #[test]
    fn script_and_style_are_opaque() {
        assert_eq!(
            kinds("<script>if (a<b) {}</script>"),
            ["open script", "text", "close script"]
        );
        assert_eq!(
            kinds("<style>a>b{}</STYLE><p>"),
            ["open style", "text", "close style", "open p"]
        );
    }

    #[test]
    fn rx_script_and_rx_style_are_opaque() {
        assert_eq!(
            kinds("<rx-script>if (a<b && c) {}</rx-script><p>"),
            ["open rx-script", "text", "close rx-script", "open p"]
        );
        assert_eq!(
            kinds("<rx-style>a>b{}</rx-style>"),
            ["open rx-style", "text", "close rx-style"]
        );
    }

    #[test]
    fn doctype_is_text_and_names_are_lowercased() {
        assert_eq!(
            kinds("<!DOCTYPE html><DIV></DIV>"),
            ["text", "open div", "close div"]
        );
    }

    #[test]
    fn lines_of_a_multi_line_tag() {
        let src = "a\n<rx-card\n  title=\"A\"\n  :open=\"x\"\n>\n</rx-card>";
        let toks = scan(src).unwrap();
        let Token::Open {
            attrs, line, span, ..
        } = &toks[1]
        else {
            panic!()
        };
        assert_eq!(*line, 2);
        assert_eq!(attrs[0].line, 3);
        assert_eq!(attrs[1].line, 4);
        assert_eq!(src[span.clone()].matches('\n').count(), 3);
        let Token::Close { line, .. } = toks.last().unwrap() else {
            panic!()
        };
        assert_eq!(*line, 6);
    }

    #[test]
    fn unclosed_tag_is_an_error() {
        let e = scan("a\n<rx-card title=\"A\"").unwrap_err();
        assert_eq!(e.line, 2);
    }
}
