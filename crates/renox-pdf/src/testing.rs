//! Test helpers for the PDFs an app makes: read the text back out of the
//! bytes and assert on it, the way `renox::testing::TestResponse` asserts on
//! a page.
//!
//! ```
//! use renox::prelude::*;
//! use renox::testing::TestApp;
//! use renox_pdf::Pdf;
//! use renox_pdf::testing::{PdfAssertions, text_of};
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let app = TestApp::new(App::new().module(Pdf::new())).await;
//! # let dir = std::env::temp_dir().join(format!("renox-pdf-doc-{}", std::process::id()));
//! # std::fs::create_dir_all(dir.join("pdf")).unwrap();
//! # std::fs::write(dir.join("pdf/note.typ"), "Order 42 is paid").unwrap();
//! # let views = dir.clone();
//! # let app = TestApp::with_config(App::new().module(Pdf::new()), move |c| c.views_path = views.clone()).await;
//! let bytes = Pdf::document(app.state(), "note")
//!     .unwrap()
//!     .render(app.state())
//!     .await
//!     .unwrap();
//!
//! assert!(text_of(&bytes).contains("Order 42"));
//! bytes
//!     .assert_pdf()
//!     .assert_pdf_contains("is paid")
//!     .assert_pdf_missing("refund")
//!     .assert_pdf_pages(1);
//! # let _ = std::fs::remove_dir_all(dir);
//! # });
//! ```
//!
//! The text is read from the produced PDF itself (its page content and the
//! fonts' `ToUnicode` maps), so it is what a viewer would copy. Latin text
//! comes back in reading order. Typst stores a shaped Arabic word in several
//! pieces, and the text keeps them as stored: every letter is there, but a
//! whole Arabic word is not one string, so assert on its letters or pieces,
//! or compare against a reference file. No PDF library is involved, only a
//! small reader for the PDFs Typst writes; a PDF from another producer may
//! give less text.

use std::collections::HashMap;
use std::io::Read;

use renox::testing::TestResponse;

/// The text of every page of a PDF made by this crate, pages separated by a
/// blank line and text runs on one line of the page kept on one line.
///
/// The runs of one line are joined as the PDF holds them, so cells of a table
/// row are not separated unless the template wrote a space. Returns an empty string when `bytes` is not a PDF it can read.
pub fn text_of(bytes: &[u8]) -> String {
    let pdf = Pdf::parse(bytes);
    pdf.pages()
        .iter()
        .map(|page| pdf.page_text(page))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The number of pages of a PDF made by this crate (0 when it cannot be read).
pub fn page_count(bytes: &[u8]) -> usize {
    Pdf::parse(bytes).pages().len()
}

/// Assertions on PDF bytes, for the bytes [`Document::render`] returns and for
/// a [`TestResponse`] that downloads a PDF. Like the response assertions in
/// `renox::testing` they panic with the start of the text and return `&Self`,
/// so they chain.
///
/// [`Document::render`]: crate::Document::render
pub trait PdfAssertions {
    /// The bytes to inspect.
    #[doc(hidden)]
    fn pdf_bytes(&self) -> &[u8];

    /// The `Content-Type` of the response, when there is one.
    #[doc(hidden)]
    fn pdf_content_type(&self) -> Option<&str> {
        None
    }

    /// The text of the PDF, as [`text_of`] reads it.
    fn pdf_text(&self) -> String {
        text_of(self.pdf_bytes())
    }

    /// Panics unless the bytes are a PDF (and, for a response, it is served as
    /// `application/pdf`).
    #[track_caller]
    fn assert_pdf(&self) -> &Self {
        let bytes = self.pdf_bytes();
        if !bytes.starts_with(b"%PDF-") {
            panic!("expected a PDF, got {} bytes starting {:?}", bytes.len(), {
                let start = &bytes[..bytes.len().min(40)];
                String::from_utf8_lossy(start).into_owned()
            });
        }
        if let Some(content_type) = self.pdf_content_type()
            && !content_type.starts_with("application/pdf")
        {
            panic!("expected Content-Type application/pdf, got {content_type:?}");
        }
        self
    }

    /// Panics unless the PDF's text contains `text`. Runs of white space
    /// count as one space on both sides, since a line break in the PDF is not
    /// something the template wrote.
    #[track_caller]
    fn assert_pdf_contains(&self, text: &str) -> &Self {
        let found = self.pdf_text();
        if !squash(&found).contains(&squash(text)) {
            panic!("expected the PDF to contain {text:?}:\n{}", excerpt(&found));
        }
        self
    }

    /// Panics if the PDF's text contains `text` (white space compared as in
    /// [`assert_pdf_contains`](PdfAssertions::assert_pdf_contains)).
    #[track_caller]
    fn assert_pdf_missing(&self, text: &str) -> &Self {
        let found = self.pdf_text();
        if squash(&found).contains(&squash(text)) {
            panic!(
                "expected the PDF not to contain {text:?}:\n{}",
                excerpt(&found)
            );
        }
        self
    }

    /// Panics unless the PDF has `expected` pages.
    #[track_caller]
    fn assert_pdf_pages(&self, expected: usize) -> &Self {
        let pages = page_count(self.pdf_bytes());
        if pages != expected {
            panic!(
                "expected {expected} PDF pages, got {pages}:\n{}",
                excerpt(&self.pdf_text())
            );
        }
        self
    }
}

impl PdfAssertions for [u8] {
    fn pdf_bytes(&self) -> &[u8] {
        self
    }
}

impl PdfAssertions for Vec<u8> {
    fn pdf_bytes(&self) -> &[u8] {
        self
    }
}

impl PdfAssertions for TestResponse {
    fn pdf_bytes(&self) -> &[u8] {
        &self.body
    }

    fn pdf_content_type(&self) -> Option<&str> {
        self.header("content-type")
    }
}

/// White space runs collapsed to one space.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn excerpt(text: &str) -> String {
    match text.char_indices().nth(600) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// A small reader for the PDFs Typst writes: plain objects, Flate streams,
// Type0 fonts with a ToUnicode map, text shown with Tj / TJ.
// ---------------------------------------------------------------------------

/// One indirect object: its dictionary (or other value) as tokens, and its
/// decoded stream.
#[derive(Default)]
struct Object {
    tokens: Vec<String>,
    stream: Vec<u8>,
}

struct Pdf {
    objects: HashMap<u32, Object>,
}

impl Pdf {
    fn parse(bytes: &[u8]) -> Pdf {
        let mut objects = HashMap::new();
        if !bytes.starts_with(b"%PDF-") {
            return Pdf { objects };
        }
        let mut at = 0;
        while let Some(found) = find(bytes, b" 0 obj", at) {
            let digits = bytes[..found]
                .iter()
                .rev()
                .take_while(|b| b.is_ascii_digit())
                .count();
            let body = found + 6;
            at = body;
            let Some(number) = std::str::from_utf8(&bytes[found - digits..found])
                .ok()
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            let end = find(bytes, b"endobj", body).unwrap_or(bytes.len());
            let stream_at = find(bytes, b"stream", body).filter(|s| *s < end);
            let (head, stream, next) = match stream_at {
                None => (&bytes[body..end], Vec::new(), end),
                Some(s) => {
                    let head = &bytes[body..s];
                    let mut data = s + 6;
                    if bytes.get(data) == Some(&b'\r') {
                        data += 1;
                    }
                    if bytes.get(data) == Some(&b'\n') {
                        data += 1;
                    }
                    let length = direct_length(head);
                    let data_end = match length {
                        Some(n) if data + n <= bytes.len() => data + n,
                        _ => find(bytes, b"endstream", data).unwrap_or(bytes.len()),
                    };
                    let raw = &bytes[data..data_end];
                    let decoded = if find(head, b"/FlateDecode", 0).is_some() {
                        inflate(raw)
                    } else {
                        raw.to_vec()
                    };
                    (head, decoded, data_end)
                }
            };
            objects.insert(
                number,
                Object {
                    tokens: tokens(&String::from_utf8_lossy(head)),
                    stream,
                },
            );
            at = at.max(next);
        }
        Pdf { objects }
    }

    /// The page objects in reading order.
    fn pages(&self) -> Vec<u32> {
        let catalog = self
            .objects
            .values()
            .find(|o| o.tokens.iter().any(|t| t == "/Catalog"));
        let mut pages = Vec::new();
        if let Some(root) = catalog.and_then(|c| refs_after(&c.tokens, "/Pages").first().copied()) {
            self.collect_pages(root, &mut pages, 0);
        }
        if pages.is_empty() {
            // No readable page tree: every page object, in file order.
            let mut all: Vec<u32> = self
                .objects
                .iter()
                .filter(|(_, o)| is_type(&o.tokens, "/Page"))
                .map(|(n, _)| *n)
                .collect();
            all.sort_unstable();
            pages = all;
        }
        pages
    }

    fn collect_pages(&self, node: u32, out: &mut Vec<u32>, depth: usize) {
        let Some(object) = self.objects.get(&node) else {
            return;
        };
        if is_type(&object.tokens, "/Page") {
            out.push(node);
        } else if depth < 16 {
            for kid in refs_after(&object.tokens, "/Kids") {
                self.collect_pages(kid, out, depth + 1);
            }
        }
    }

    fn page_text(&self, page: &u32) -> String {
        let Some(page) = self.objects.get(page) else {
            return String::new();
        };
        let fonts = self.page_fonts(&page.tokens);
        let mut writer = Writer::default();
        for content in refs_after(&page.tokens, "/Contents") {
            if let Some(object) = self.objects.get(&content) {
                show_text(&object.stream, &fonts, &mut writer);
            }
        }
        writer.finish()
    }

    /// The `ToUnicode` map of each font the page names, by resource name.
    fn page_fonts(&self, page: &[String]) -> HashMap<String, CMap> {
        // `/Resources` is a reference or an inline dictionary.
        let resources: Vec<String> = match refs_after(page, "/Resources").first() {
            Some(r) => self.objects.get(r).map(|o| o.tokens.clone()),
            None => value_after(page, "/Resources").map(<[String]>::to_vec),
        }
        .unwrap_or_default();
        let entries = match value_after(&resources, "/Font") {
            Some(inline) if inline.first().is_some_and(|t| t == "<") => name_refs(inline),
            _ => match refs_after(&resources, "/Font").first() {
                Some(r) => self
                    .objects
                    .get(r)
                    .map(|o| name_refs(&o.tokens))
                    .unwrap_or_default(),
                None => Vec::new(),
            },
        };
        let mut fonts = HashMap::new();
        for (name, number) in entries {
            let cmap = self
                .objects
                .get(&number)
                .and_then(|font| refs_after(&font.tokens, "/ToUnicode").first().copied())
                .and_then(|map| self.objects.get(&map))
                .map(|map| CMap::parse(&map.stream))
                .unwrap_or_default();
            fonts.insert(name, cmap);
        }
        fonts
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// `/Length 123` when the length is written directly (not as a reference).
fn direct_length(head: &[u8]) -> Option<usize> {
    let toks = tokens(&String::from_utf8_lossy(head));
    let at = toks.iter().position(|t| t == "/Length")?;
    if toks.get(at + 2).is_some_and(|t| t == "R") {
        return None;
    }
    toks.get(at + 1)?.parse().ok()
}

fn inflate(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // A damaged stream gives what was decoded before the damage.
    let _ = flate2::read::ZlibDecoder::new(raw)
        .take(64 * 1024 * 1024)
        .read_to_end(&mut out);
    out
}

/// Splits dictionary text into names (`/Type`), numbers, words and the
/// delimiters `[ ] < >` (each one a token); string literals are dropped.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '[' | ']' | '<' | '>' => out.push(c.to_string()),
            '(' => {
                let mut depth = 1;
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                let mut word = c.to_string();
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || "/[]<>()".contains(n) {
                        break;
                    }
                    word.push(n);
                    chars.next();
                }
                out.push(word);
            }
        }
    }
    out
}

fn is_type(tokens: &[String], kind: &str) -> bool {
    tokens.windows(2).any(|w| w[0] == "/Type" && w[1] == kind)
}

/// The tokens of the value of `key`: a `<< .. >>` dictionary or `[ .. ]`
/// array (brackets included), or the single token (or `N 0 R`) otherwise.
fn value_after<'a>(tokens: &'a [String], key: &str) -> Option<&'a [String]> {
    let at = tokens.iter().position(|t| t == key)? + 1;
    let rest = tokens.get(at..)?;
    let (open, close) = match rest.first()?.as_str() {
        "<" => ("<", ">"),
        "[" => ("[", "]"),
        _ => {
            let len = if rest.get(2).is_some_and(|t| t == "R") {
                3
            } else {
                1
            };
            return rest.get(..len);
        }
    };
    let mut depth = 0;
    for (i, t) in rest.iter().enumerate() {
        if t == open {
            depth += 1;
        } else if t == close {
            depth -= 1;
            if depth == 0 {
                return rest.get(..=i);
            }
        }
    }
    None
}

/// The object numbers of the references in the value of `key`, in order.
fn refs_after(tokens: &[String], key: &str) -> Vec<u32> {
    let Some(value) = value_after(tokens, key) else {
        return Vec::new();
    };
    value
        .windows(3)
        .filter(|w| w[1] == "0" && w[2] == "R")
        .filter_map(|w| w[0].parse().ok())
        .collect()
}

/// `/name N 0 R` pairs of a dictionary's tokens.
fn name_refs(tokens: &[String]) -> Vec<(String, u32)> {
    tokens
        .windows(4)
        .filter(|w| w[0].starts_with('/') && w[2] == "0" && w[3] == "R")
        .filter_map(|w| Some((w[0][1..].to_owned(), w[1].parse().ok()?)))
        .collect()
}

/// A `ToUnicode` map: character code to text.
#[derive(Default)]
struct CMap(HashMap<u32, String>);

enum Hex {
    Value(Vec<u8>),
    Open,
    Close,
}

impl CMap {
    fn parse(stream: &[u8]) -> CMap {
        let text = String::from_utf8_lossy(stream);
        let mut map = HashMap::new();
        for (begin, end, ranges) in [
            ("beginbfchar", "endbfchar", false),
            ("beginbfrange", "endbfrange", true),
        ] {
            let mut rest: &str = &text;
            while let Some(start) = rest.find(begin) {
                let section = &rest[start + begin.len()..];
                let stop = section.find(end).unwrap_or(section.len());
                let items = hex_items(&section[..stop]);
                if ranges {
                    Self::ranges(&items, &mut map);
                } else {
                    for pair in items.chunks(2) {
                        if let [Hex::Value(code), Hex::Value(to)] = pair {
                            map.insert(be(code), utf16(to));
                        }
                    }
                }
                rest = &section[stop..];
            }
        }
        CMap(map)
    }

    fn ranges(items: &[Hex], map: &mut HashMap<u32, String>) {
        let mut i = 0;
        while i + 2 < items.len() {
            let (Hex::Value(lo), Hex::Value(hi)) = (&items[i], &items[i + 1]) else {
                i += 1;
                continue;
            };
            let (lo, hi) = (be(lo), be(hi));
            match &items[i + 2] {
                Hex::Value(to) => {
                    let mut units = utf16_units(to);
                    for code in lo..=hi.min(lo.saturating_add(0xFFFF)) {
                        map.insert(code, String::from_utf16_lossy(&units));
                        if let Some(last) = units.last_mut() {
                            *last = last.wrapping_add(1);
                        }
                    }
                    i += 3;
                }
                Hex::Open => {
                    let mut code = lo;
                    i += 3;
                    while i < items.len() {
                        match &items[i] {
                            Hex::Close => {
                                i += 1;
                                break;
                            }
                            Hex::Value(to) => {
                                map.insert(code, utf16(to));
                                code += 1;
                            }
                            Hex::Open => {}
                        }
                        i += 1;
                    }
                }
                Hex::Close => i += 1,
            }
        }
    }
}

/// The `<hex>` strings and array brackets of a CMap section.
fn hex_items(section: &str) -> Vec<Hex> {
    let mut items = Vec::new();
    let mut rest = section;
    while let Some(at) = rest.find(['<', '[', ']']) {
        let c = rest.as_bytes()[at];
        rest = &rest[at + 1..];
        match c {
            b'[' => items.push(Hex::Open),
            b']' => items.push(Hex::Close),
            _ => {
                let Some(close) = rest.find('>') else { break };
                let digits: Vec<u8> = rest[..close]
                    .bytes()
                    .filter(u8::is_ascii_hexdigit)
                    .collect();
                items.push(Hex::Value(hex_bytes(&digits)));
                rest = &rest[close + 1..];
            }
        }
    }
    items
}

fn hex_bytes(digits: &[u8]) -> Vec<u8> {
    let value = |d: u8| (d as char).to_digit(16).unwrap_or(0) as u8;
    digits
        .chunks(2)
        .map(|pair| (value(pair[0]) << 4) | pair.get(1).map_or(0, |d| value(*d)))
        .collect()
}

fn be(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0, |acc, b| (acc << 8) | u32::from(*b))
}

fn utf16_units(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks(2)
        .map(|pair| (u16::from(pair[0]) << 8) | pair.get(1).map_or(0, |b| u16::from(*b)))
        .collect()
}

fn utf16(bytes: &[u8]) -> String {
    String::from_utf16_lossy(&utf16_units(bytes))
}

/// Collects the shown text, one line per text baseline.
#[derive(Default)]
struct Writer {
    out: String,
    /// The baseline of the last run, if any.
    y: Option<f64>,
}

impl Writer {
    fn push(&mut self, y: f64, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.y {
            // The same line: the runs are joined as the PDF holds them.
            Some(last) if (last - y).abs() < 0.5 => {}
            Some(_) => {
                let trimmed = self.out.trim_end_matches(' ').len();
                self.out.truncate(trimmed);
                self.out.push('\n');
            }
            None => {}
        }
        self.y = Some(y);
        self.out.push_str(text);
    }

    fn finish(mut self) -> String {
        let trimmed = self.out.trim_end().len();
        self.out.truncate(trimmed);
        self.out
    }
}

enum Operand {
    Number(f64),
    Name(String),
    Text(Vec<u8>),
    Array(Vec<Vec<u8>>),
}

/// Runs a page content stream and writes the text it shows.
fn show_text(content: &[u8], fonts: &HashMap<String, CMap>, writer: &mut Writer) {
    let empty = CMap::default();
    let mut operands: Vec<Operand> = Vec::new();
    let mut array: Option<Vec<Vec<u8>>> = None;
    let mut marked: Vec<String> = Vec::new();
    let mut font: &CMap = &empty;
    let mut y = 0.0;
    let mut i = 0;
    while i < content.len() {
        let c = content[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' | 0 | 0x0c => i += 1,
            b'%' => {
                while i < content.len() && content[i] != b'\n' && content[i] != b'\r' {
                    i += 1;
                }
            }
            b'[' => {
                array = Some(Vec::new());
                i += 1;
            }
            b']' => {
                if let Some(items) = array.take() {
                    operands.push(Operand::Array(items));
                }
                i += 1;
            }
            b'(' => {
                let (text, next) = literal(content, i + 1);
                match array.as_mut() {
                    Some(items) => items.push(text),
                    None => operands.push(Operand::Text(text)),
                }
                i = next;
            }
            b'<' if content.get(i + 1) == Some(&b'<') => {
                // An inline dictionary (marked-content properties): skip it.
                let mut depth = 0;
                while i < content.len() {
                    if content[i..].starts_with(b"<<") {
                        depth += 1;
                        i += 2;
                    } else if content[i..].starts_with(b">>") {
                        depth -= 1;
                        i += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
            }
            b'<' => {
                let end = content[i..]
                    .iter()
                    .position(|b| *b == b'>')
                    .map_or(content.len(), |p| i + p);
                let digits: Vec<u8> = content[i + 1..end]
                    .iter()
                    .copied()
                    .filter(u8::is_ascii_hexdigit)
                    .collect();
                let text = hex_bytes(&digits);
                match array.as_mut() {
                    Some(items) => items.push(text),
                    None => operands.push(Operand::Text(text)),
                }
                i = end + 1;
            }
            b'/' => {
                let end = word_end(content, i + 1);
                operands.push(Operand::Name(
                    String::from_utf8_lossy(&content[i + 1..end]).into_owned(),
                ));
                i = end;
            }
            _ => {
                let end = word_end(content, i).max(i + 1);
                let word = String::from_utf8_lossy(&content[i..end]).into_owned();
                i = end;
                if let Ok(number) = word.parse::<f64>() {
                    if array.is_none() {
                        operands.push(Operand::Number(number));
                    }
                    continue;
                }
                match word.as_str() {
                    "Tf" => {
                        if let Some(Operand::Name(name)) = operands.first() {
                            font = fonts.get(name).unwrap_or(&empty);
                        }
                    }
                    "cm" => {
                        if let Some(Operand::Number(f)) = operands.get(5) {
                            y = *f;
                        }
                    }
                    "BMC" | "BDC" => marked.push(match operands.first() {
                        Some(Operand::Name(name)) => name.clone(),
                        _ => String::new(),
                    }),
                    "EMC" => {
                        marked.pop();
                    }
                    "Tj" | "TJ" | "'" | "\"" => {
                        let reversed = marked.iter().any(|m| m == "ReversedChars");
                        let mut text = String::new();
                        for operand in &operands {
                            match operand {
                                Operand::Text(bytes) => text.push_str(&decode(bytes, font)),
                                Operand::Array(items) => {
                                    for bytes in items {
                                        text.push_str(&decode(bytes, font));
                                    }
                                }
                                _ => {}
                            }
                        }
                        // A right-to-left run is stored in visual order.
                        if reversed {
                            text = text.chars().rev().collect();
                        }
                        writer.push(y, &text);
                    }
                    _ => {}
                }
                operands.clear();
            }
        }
    }
}

fn word_end(content: &[u8], from: usize) -> usize {
    let mut end = from;
    while end < content.len() && !b" \t\r\n\0\x0c/[]<>()%".contains(&content[end]) {
        end += 1;
    }
    end
}

/// A literal string starting after its `(`: the bytes and where it ended.
fn literal(content: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 1;
    while i < content.len() {
        let c = content[i];
        i += 1;
        match c {
            b'\\' => {
                let Some(&e) = content.get(i) else { break };
                i += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'0'..=b'7' => {
                        let mut value = u32::from(e - b'0');
                        for _ in 0..2 {
                            match content.get(i) {
                                Some(d @ b'0'..=b'7') => {
                                    value = value * 8 + u32::from(d - b'0');
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(value as u8);
                    }
                    b'\r' | b'\n' => {}
                    other => out.push(other),
                }
            }
            b'(' => {
                depth += 1;
                out.push(c);
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    (out, i)
}

/// The text of a shown string: two bytes per code (Typst writes Identity-H).
fn decode(bytes: &[u8], font: &CMap) -> String {
    bytes
        .chunks(2)
        .filter_map(|code| font.0.get(&be(code)))
        .map(String::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cmap_with_chars_and_ranges() {
        let cmap = CMap::parse(
            b"1 beginbfchar\n<0001> <0041>\nendbfchar\n2 beginbfrange\n<0010> <0012> <0061>\n<0020> <0021> [<0078> <0079>]\nendbfrange",
        );
        assert_eq!(cmap.0[&1], "A");
        assert_eq!(cmap.0[&0x10], "a");
        assert_eq!(cmap.0[&0x12], "c");
        assert_eq!(cmap.0[&0x20], "x");
        assert_eq!(cmap.0[&0x21], "y");
    }

    #[test]
    fn a_literal_string_unescapes() {
        let (text, end) = literal(b"\\000\\001a\\(b\\\\)rest", 0);
        assert_eq!(text, b"\0\x01a(b\\");
        assert_eq!(end, 15);
    }

    #[test]
    fn not_a_pdf_has_no_text() {
        assert_eq!(text_of(b"hello"), "");
        assert_eq!(page_count(b"<html>"), 0);
    }

    #[test]
    #[should_panic(expected = "expected a PDF")]
    fn assert_pdf_rejects_other_bytes() {
        b"<html></html>".to_vec().assert_pdf();
    }
}
