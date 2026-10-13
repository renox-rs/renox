//! Syntax colours for the code samples, worked out once when a page is
//! rendered: no JavaScript and no theme files. Each token becomes a
//! `<span class="hl-…">` that site.css colours (in light and dark mode):
//!
//! | class      | what                                         |
//! |------------|----------------------------------------------|
//! | `hl-com`   | comments                                     |
//! | `hl-kw`    | keywords (`fn`, `let`, `SELECT`, `{% if %}`) |
//! | `hl-str`   | strings and characters                       |
//! | `hl-num`   | numbers                                      |
//! | `hl-lit`   | `true`, `false`, `none`, `null`              |
//! | `hl-fn`    | functions, methods, shell commands, filters  |
//! | `hl-ty`    | types (`Routes`, `Result`, `BIGINT`)         |
//! | `hl-mac`   | Rust macros (`view!`, `vec!`)                |
//! | `hl-attr`  | Rust attributes, command-line flags          |
//! | `hl-life`  | Rust lifetimes                               |
//! | `hl-tag`   | HTML tags, TOML tables, CSS selectors        |
//! | `hl-prop`  | HTML attributes, TOML/JSON keys, CSS props   |
//! | `hl-var`   | shell and PHP variables                      |
//! | `hl-tpl`   | template delimiters (`{{ }}`, `{% %}`)       |
//!
//! It is a small hand-written lexer per language rather than a full
//! grammar: good enough to make samples easy to read, never more than that.
//! An unknown language is shown without colours.

/// `code` as HTML (escaped), with colour spans for `language`.
pub fn highlight(language: &str, code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    let mut out = Out::default();
    match language {
        "rust" | "rs" => rust(&chars, &mut out),
        "html" | "htm" | "jinja" | "minijinja" | "xml" | "svg" | "blade" => {
            markup(&chars, &mut out, language == "blade")
        }
        "bash" | "sh" | "shell" | "console" | "zsh" => shell(&chars, &mut out),
        "toml" => toml(&chars, &mut out),
        "json" => json(&chars, &mut out),
        "sql" => sql(&chars, &mut out),
        "php" => php(&chars, &mut out),
        "css" => css(&chars, &mut out),
        "nginx" | "conf" | "ini" | "systemd" => conf(&chars, &mut out),
        _ => out.plain(&chars),
    }
    out.html
}

/// The name shown on a code panel.
pub fn label(language: &str) -> &'static str {
    match language {
        "rust" | "rs" => "Rust",
        "html" | "htm" | "jinja" | "minijinja" => "Template",
        "xml" => "XML",
        "svg" => "SVG",
        "blade" => "Blade",
        "bash" | "sh" | "shell" | "zsh" => "Terminal",
        "console" => "Console",
        "toml" => "TOML",
        "json" => "JSON",
        "sql" => "SQL",
        "php" => "PHP",
        "css" => "CSS",
        "nginx" => "nginx",
        "markdown" | "md" => "Markdown",
        "conf" | "ini" | "systemd" => "Config",
        _ => "Text",
    }
}

#[derive(Default)]
struct Out {
    html: String,
}

impl Out {
    fn plain(&mut self, text: &[char]) {
        for &c in text {
            self.push_escaped(c);
        }
    }

    fn token(&mut self, class: &str, text: &[char]) {
        if text.is_empty() {
            return;
        }
        self.html.push_str("<span class=\"hl-");
        self.html.push_str(class);
        self.html.push_str("\">");
        self.plain(text);
        self.html.push_str("</span>");
    }

    fn push_escaped(&mut self, c: char) {
        match c {
            '&' => self.html.push_str("&amp;"),
            '<' => self.html.push_str("&lt;"),
            '>' => self.html.push_str("&gt;"),
            '"' => self.html.push_str("&quot;"),
            c => self.html.push(c),
        }
    }
}

// Small scanning helpers. Positions are indexes into a `&[char]`.

fn starts(s: &[char], i: usize, with: &str) -> bool {
    (i..).zip(with.chars()).all(|(j, c)| s.get(j) == Some(&c))
}

/// Where the line holding `i` ends (the newline's index, or the end).
fn line_end(s: &[char], i: usize) -> usize {
    (i..s.len()).find(|&j| s[j] == '\n').unwrap_or(s.len())
}

/// The index just after `end`, searched from `from`; the end of the text if
/// it never comes.
fn after(s: &[char], from: usize, end: &str) -> usize {
    (from..s.len())
        .find(|&j| starts(s, j, end))
        .map_or(s.len(), |j| j + end.chars().count())
}

/// The end of a string opened by `quote` at `i` (backslash escapes).
fn string_end(s: &[char], i: usize, quote: char) -> usize {
    let mut j = i + 1;
    while j < s.len() {
        match s[j] {
            '\\' => j += 2,
            c if c == quote => return j + 1,
            _ => j += 1,
        }
    }
    s.len()
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn ident_end(s: &[char], i: usize) -> usize {
    (i..s.len()).find(|&j| !is_ident(s[j])).unwrap_or(s.len())
}

/// The first character at or after `i` that isn't a space or tab.
fn next_visible(s: &[char], i: usize) -> Option<char> {
    s[i.min(s.len())..]
        .iter()
        .copied()
        .find(|c| *c != ' ' && *c != '\t')
}

fn number_end(s: &[char], i: usize) -> usize {
    let mut j = i;
    while j < s.len()
        && (is_ident(s[j]) || s[j] == '.' && s.get(j + 1).is_some_and(|c| c.is_ascii_digit()))
    {
        j += 1;
    }
    j
}

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "type", "unsafe", "use", "where",
    "while", "yield",
];

fn rust(s: &[char], out: &mut Out) {
    let mut i = 0;
    // The last word, so the name after `fn` counts as a function.
    let mut last_word = String::new();
    while i < s.len() {
        let c = s[i];
        if starts(s, i, "//") {
            let end = line_end(s, i);
            out.token("com", &s[i..end]);
            i = end;
        } else if starts(s, i, "/*") {
            let end = after(s, i + 2, "*/");
            out.token("com", &s[i..end]);
            i = end;
        } else if starts(s, i, "#[") || starts(s, i, "#![") {
            let end = attribute_end(s, i);
            out.token("attr", &s[i..end]);
            i = end;
        } else if let Some(end) = raw_string_end(s, i) {
            out.token("str", &s[i..end]);
            i = end;
        } else if c == '"' || c == 'b' && s.get(i + 1) == Some(&'"') {
            let open = if c == 'b' { i + 1 } else { i };
            let end = string_end(s, open, '"');
            out.token("str", &s[i..end]);
            i = end;
        } else if c == '\'' {
            let end = quote_end(s, i);
            let class = if end > i + 1 && s[end - 1] == '\'' {
                "str"
            } else {
                "life"
            };
            out.token(class, &s[i..end]);
            i = end;
        } else if c.is_ascii_digit() {
            let end = number_end(s, i);
            out.token("num", &s[i..end]);
            i = end;
        } else if is_ident_start(c) {
            let end = ident_end(s, i);
            let word: String = s[i..end].iter().collect();
            if s.get(end) == Some(&'!') && s.get(end + 1) != Some(&'=') {
                out.token("mac", &s[i..end + 1]);
                i = end + 1;
                last_word.clear();
                continue;
            }
            let class = if RUST_KEYWORDS.contains(&word.as_str()) {
                Some("kw")
            } else if word == "true" || word == "false" {
                Some("lit")
            } else if last_word == "fn" {
                Some("fn")
            } else if word.starts_with(|c: char| c.is_uppercase()) {
                Some("ty")
            } else if s.get(end) == Some(&'(') || starts(s, end, "::<") {
                Some("fn")
            } else {
                None
            };
            match class {
                Some(class) => out.token(class, &s[i..end]),
                None => out.plain(&s[i..end]),
            }
            last_word = word;
            i = end;
        } else {
            if !c.is_whitespace() {
                last_word.clear();
            }
            out.push_escaped(c);
            i += 1;
        }
    }
}

/// `#[…]` with nested brackets and strings inside.
fn attribute_end(s: &[char], i: usize) -> usize {
    let mut depth = 0;
    let mut j = i;
    while j < s.len() {
        match s[j] {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            '"' => {
                j = string_end(s, j, '"');
                continue;
            }
            '\n' if depth == 0 => return j,
            _ => {}
        }
        j += 1;
    }
    s.len()
}

/// `r"…"`, `r#"…"#`, `br"…"`: where it ends, if one starts at `i`.
fn raw_string_end(s: &[char], i: usize) -> Option<usize> {
    if i > 0 && is_ident(s[i - 1]) {
        return None;
    }
    let mut j = i;
    if s.get(j) == Some(&'b') {
        j += 1;
    }
    if s.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let hashes = s[j..].iter().take_while(|c| **c == '#').count();
    j += hashes;
    if s.get(j) != Some(&'"') {
        return None;
    }
    let close: String = std::iter::once('"')
        .chain(std::iter::repeat_n('#', hashes))
        .collect();
    Some(after(s, j + 1, &close))
}

/// A character literal (`'a'`, `'\n'`) ends at its closing quote; a
/// lifetime (`'a`, `'static`) at the end of its name.
fn quote_end(s: &[char], i: usize) -> usize {
    if s.get(i + 1) == Some(&'\\') {
        return (i + 2..s.len().min(i + 12))
            .find(|&j| s[j] == '\'')
            .map_or(i + 1, |j| j + 1);
    }
    if s.get(i + 2) == Some(&'\'') {
        return i + 3;
    }
    if s.get(i + 1).is_some_and(|c| is_ident_start(*c)) {
        return ident_end(s, i + 1);
    }
    i + 1
}

/// HTML with MiniJinja (`{{ }}`, `{% %}`, `{# #}`), Alpine and htmx
/// attributes; Blade's `@directives` too when `blade`.
fn markup(s: &[char], out: &mut Out, blade: bool) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if starts(s, i, "<!--") {
            let end = after(s, i, "-->");
            out.token("com", &s[i..end]);
            i = end;
        } else if starts(s, i, "{{--") {
            let end = after(s, i, "--}}");
            out.token("com", &s[i..end]);
            i = end;
        } else if let Some(end) = template(s, i, out) {
            i = end;
        } else if c == '<'
            && s.get(i + 1)
                .is_some_and(|c| c.is_alphabetic() || *c == '/' || *c == '!')
        {
            i = tag(s, i, out);
        } else if blade
            && c == '@'
            && s.get(i + 1).is_some_and(|c| c.is_alphabetic())
            && (i == 0 || !is_ident(s[i - 1]))
        {
            let end = ident_end(s, i + 1);
            out.token("kw", &s[i..end]);
            i = end;
        } else {
            out.push_escaped(c);
            i += 1;
        }
    }
}

/// A template comment or tag starting at `i`, written out; where it ends.
fn template(s: &[char], i: usize, out: &mut Out) -> Option<usize> {
    if starts(s, i, "{#") {
        let end = after(s, i, "#}");
        out.token("com", &s[i..end]);
        return Some(end);
    }
    let close = if starts(s, i, "{{") {
        "}}"
    } else if starts(s, i, "{%") {
        "%}"
    } else if starts(s, i, "{!!") {
        "!!}"
    } else {
        return None;
    };
    let mut open = i + if close == "!!}" { 3 } else { 2 };
    if s.get(open) == Some(&'-') {
        open += 1;
    }
    let end = (open..s.len()).find(|&j| starts(s, j, close));
    let Some(mut inner_end) = end else {
        out.token("tpl", &s[i..open]);
        return Some(open);
    };
    let close_end = inner_end + close.chars().count();
    if inner_end > open && s[inner_end - 1] == '-' {
        inner_end -= 1;
    }
    out.token("tpl", &s[i..open]);
    expression(&s[open..inner_end], out);
    out.token("tpl", &s[inner_end..close_end]);
    Some(close_end)
}

const TEMPLATE_KEYWORDS: &[&str] = &[
    "if",
    "elif",
    "else",
    "endif",
    "for",
    "endfor",
    "in",
    "not",
    "and",
    "or",
    "is",
    "set",
    "endset",
    "macro",
    "endmacro",
    "call",
    "endcall",
    "extends",
    "block",
    "endblock",
    "import",
    "from",
    "include",
    "with",
    "endwith",
    "filter",
    "endfilter",
    "as",
    "recursive",
    "raw",
    "endraw",
    "autoescape",
    "endautoescape",
];

/// The inside of `{{ … }}` / `{% … %}`.
fn expression(s: &[char], out: &mut Out) {
    let mut i = 0;
    let mut after_pipe = false;
    while i < s.len() {
        let c = s[i];
        if c == '"' || c == '\'' {
            let end = string_end(s, i, c).min(s.len());
            out.token("str", &s[i..end]);
            i = end;
        } else if c.is_ascii_digit() {
            let end = number_end(s, i);
            out.token("num", &s[i..end]);
            i = end;
        } else if is_ident_start(c) {
            let end = ident_end(s, i);
            let word: String = s[i..end].iter().collect();
            let class = if after_pipe || s.get(end) == Some(&'(') {
                Some("fn")
            } else if TEMPLATE_KEYWORDS.contains(&word.as_str()) {
                Some("kw")
            } else if matches!(
                word.as_str(),
                "true" | "false" | "none" | "True" | "False" | "None"
            ) {
                Some("lit")
            } else {
                None
            };
            match class {
                Some(class) => out.token(class, &s[i..end]),
                None => out.plain(&s[i..end]),
            }
            after_pipe = false;
            i = end;
        } else {
            if c == '|' {
                after_pipe = true;
            } else if !c.is_whitespace() {
                after_pipe = false;
            }
            out.push_escaped(c);
            i += 1;
        }
    }
}

/// An HTML tag from its `<` to its `>`: name, attributes, values (which may
/// hold template tags).
fn tag(s: &[char], i: usize, out: &mut Out) -> usize {
    let mut j = i + 1;
    if matches!(s.get(j), Some('/') | Some('!')) {
        j += 1;
    }
    let name_end = (j..s.len())
        .find(|&k| !(s[k].is_alphanumeric() || s[k] == '-' || s[k] == ':'))
        .unwrap_or(s.len());
    out.token("tag", &s[i..name_end]);
    j = name_end;
    while j < s.len() {
        let c = s[j];
        if c == '>' || starts(s, j, "/>") {
            let end = if c == '>' { j + 1 } else { j + 2 };
            out.token("tag", &s[j..end]);
            return end;
        }
        if let Some(end) = template(s, j, out) {
            j = end;
        } else if c == '"' || c == '\'' {
            j = attribute_value(s, j, c, out);
        } else if c.is_whitespace() || c == '=' {
            out.push_escaped(c);
            j += 1;
        } else {
            let end = (j..s.len())
                .find(|&k| {
                    s[k].is_whitespace()
                        || matches!(s[k], '=' | '>' | '"' | '\'')
                        || starts(s, k, "/>")
                        || starts(s, k, "{{")
                        || starts(s, k, "{%")
                })
                .unwrap_or(s.len())
                .max(j + 1);
            out.token("prop", &s[j..end]);
            j = end;
        }
    }
    s.len()
}

/// A quoted attribute value; template tags inside keep their own colours.
fn attribute_value(s: &[char], i: usize, quote: char, out: &mut Out) -> usize {
    let mut start = i;
    let mut j = i + 1;
    while j < s.len() {
        if s[j] == quote {
            out.token("str", &s[start..j + 1]);
            return j + 1;
        }
        if starts(s, j, "{{") || starts(s, j, "{%") {
            out.token("str", &s[start..j]);
            j = template(s, j, out).unwrap_or(j + 1);
            start = j;
            continue;
        }
        j += 1;
    }
    out.token("str", &s[start..]);
    s.len()
}

/// Shell commands: the command word, flags, strings, variables, comments.
fn shell(s: &[char], out: &mut Out) {
    let mut i = 0;
    let mut command_next = true;
    while i < s.len() {
        let c = s[i];
        if c == '\n' {
            // A line ending in `\` goes on with the same command.
            let continued = s[..i].iter().rev().find(|c| **c != ' ') == Some(&'\\');
            command_next = !continued;
            out.push_escaped(c);
            i += 1;
        } else if c == ' ' || c == '\t' {
            out.push_escaped(c);
            i += 1;
        } else if c == '#' && (i == 0 || s[i - 1].is_whitespace()) {
            let end = line_end(s, i);
            out.token("com", &s[i..end]);
            i = end;
        } else if c == '\'' {
            let end = (i + 1..s.len())
                .find(|&j| s[j] == '\'')
                .map_or(s.len(), |j| j + 1);
            out.token("str", &s[i..end]);
            i = end;
            command_next = false;
        } else if c == '"' {
            let end = string_end(s, i, '"');
            out.token("str", &s[i..end]);
            i = end;
            command_next = false;
        } else if c == '$' && s.get(i + 1) == Some(&'{') {
            let end = after(s, i, "}");
            out.token("var", &s[i..end]);
            i = end;
        } else if c == '$' && s.get(i + 1).is_some_and(|c| is_ident_start(*c)) {
            let end = ident_end(s, i + 1);
            out.token("var", &s[i..end]);
            i = end;
        } else if matches!(c, '|' | '&' | ';' | '(' | ')') {
            command_next = true;
            out.push_escaped(c);
            i += 1;
        } else {
            let end = (i..s.len())
                .find(|&j| {
                    s[j].is_whitespace() || matches!(s[j], '|' | '&' | ';' | '"' | '\'' | ')')
                })
                .unwrap_or(s.len())
                .max(i + 1);
            let word = &s[i..end];
            let assignment = word
                .iter()
                .position(|c| *c == '=')
                .is_some_and(|eq| eq > 0 && word[..eq].iter().all(|c| is_ident(*c)));
            if command_next && assignment {
                let eq = word.iter().position(|c| *c == '=').unwrap_or(0);
                out.token("var", &word[..eq]);
                out.plain(&word[eq..]);
            } else if command_next {
                out.token("fn", word);
                // `sudo cmd`: the next word is the command too.
                let w: String = word.iter().collect();
                command_next = matches!(w.as_str(), "sudo" | "env" | "time" | "exec");
            } else if word[0] == '-' {
                out.token("attr", word);
            } else {
                out.plain(word);
            }
            i = end;
        }
    }
}

/// TOML: tables, keys, strings, numbers, booleans, comments.
fn toml(s: &[char], out: &mut Out) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let line_start = s[..i]
            .iter()
            .rev()
            .take_while(|c| **c != '\n')
            .all(|c| c.is_whitespace());
        if c == '#' {
            let end = line_end(s, i);
            out.token("com", &s[i..end]);
            i = end;
        } else if c == '[' && line_start {
            let end = line_end(s, i);
            let close = (i..end).rev().find(|&j| s[j] == ']').map_or(end, |j| j + 1);
            out.token("tag", &s[i..close]);
            i = close;
        } else if c == '"' || c == '\'' {
            let end = if starts(s, i, "\"\"\"") {
                after(s, i + 3, "\"\"\"")
            } else {
                string_end(s, i, c)
            };
            out.token("str", &s[i..end]);
            i = end;
        } else if c.is_ascii_digit() {
            let end = number_end(s, i);
            out.token("num", &s[i..end]);
            i = end;
        } else if is_ident_start(c) {
            let end = (i..s.len())
                .find(|&j| !(is_ident(s[j]) || s[j] == '-' || s[j] == '.'))
                .unwrap_or(s.len());
            let word: String = s[i..end].iter().collect();
            if next_visible(s, end) == Some('=') {
                out.token("prop", &s[i..end]);
            } else if word == "true" || word == "false" {
                out.token("lit", &s[i..end]);
            } else {
                out.plain(&s[i..end]);
            }
            i = end;
        } else {
            out.push_escaped(c);
            i += 1;
        }
    }
}

fn json(s: &[char], out: &mut Out) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == '"' {
            let end = string_end(s, i, '"');
            let class = if next_visible(s, end) == Some(':') {
                "prop"
            } else {
                "str"
            };
            out.token(class, &s[i..end]);
            i = end;
        } else if c.is_ascii_digit() || c == '-' && s.get(i + 1).is_some_and(char::is_ascii_digit) {
            let end = number_end(s, i + 1);
            out.token("num", &s[i..end]);
            i = end;
        } else if is_ident_start(c) {
            let end = ident_end(s, i);
            out.token("lit", &s[i..end]);
            i = end;
        } else {
            out.push_escaped(c);
            i += 1;
        }
    }
}

const SQL_KEYWORDS: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "AS",
    "ASC",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASCADE",
    "CASE",
    "CHECK",
    "COMMIT",
    "CONFLICT",
    "CONSTRAINT",
    "CREATE",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DO",
    "DROP",
    "ELSE",
    "END",
    "EXISTS",
    "FOREIGN",
    "FROM",
    "GENERATED",
    "GROUP",
    "HAVING",
    "IDENTITY",
    "IF",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NOT",
    "NOTHING",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "PRIMARY",
    "REFERENCES",
    "RETURNING",
    "ROLLBACK",
    "SELECT",
    "SET",
    "TABLE",
    "THEN",
    "TO",
    "UNIQUE",
    "UPDATE",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
    "WITH",
    "ALWAYS",
];

const SQL_TYPES: &[&str] = &[
    "BIGINT",
    "BIGSERIAL",
    "BLOB",
    "BOOLEAN",
    "BYTEA",
    "CHAR",
    "DATE",
    "DECIMAL",
    "DOUBLE",
    "FLOAT",
    "INT",
    "INTEGER",
    "JSON",
    "JSONB",
    "NUMERIC",
    "REAL",
    "SERIAL",
    "SMALLINT",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "TIMESTAMPTZ",
    "UUID",
    "VARCHAR",
];

fn sql(s: &[char], out: &mut Out) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if starts(s, i, "--") {
            let end = line_end(s, i);
            out.token("com", &s[i..end]);
            i = end;
        } else if c == '\'' {
            let end = string_end(s, i, '\'');
            out.token("str", &s[i..end]);
            i = end;
        } else if c.is_ascii_digit() {
            let end = number_end(s, i);
            out.token("num", &s[i..end]);
            i = end;
        } else if is_ident_start(c) {
            let end = ident_end(s, i);
            let word: String = s[i..end].iter().collect::<String>().to_uppercase();
            let class = if SQL_KEYWORDS.contains(&word.as_str()) {
                Some("kw")
            } else if SQL_TYPES.contains(&word.as_str()) {
                Some("ty")
            } else if word == "TRUE" || word == "FALSE" {
                Some("lit")
            } else if s.get(end) == Some(&'(') {
                Some("fn")
            } else {
                None
            };
            match class {
                Some(class) => out.token(class, &s[i..end]),
                None => out.plain(&s[i..end]),
            }
            i = end;
        } else {
            out.push_escaped(c);
            i += 1;
        }
    }
}

const PHP_KEYWORDS: &[&str] = &[
    "abstract",
    "array",
    "as",
    "class",
    "echo",
    "else",
    "extends",
    "fn",
    "foreach",
    "function",
    "if",
    "implements",
    "new",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "use",
];

/// Enough PHP for the Laravel comparisons.
fn php(s: &[char], out: &mut Out) {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if starts(s, i, "//") || c == '#' && !starts(s, i, "#[") {
            let end = line_end(s, i);
            out.token("com", &s[i..end]);
            i = end;
        } else if starts(s, i, "/*") {
            let end = after(s, i + 2, "*/");
            out.token("com", &s[i..end]);
            i = end;
        } else if starts(s, i, "<?php") {
            out.token("kw", &s[i..i + 5]);
            i += 5;
        } else if c == '"' || c == '\'' {
            let end = string_end(s, i, c);
            out.token("str", &s[i..end]);
            i = end;
        } else if c == '$' && s.get(i + 1).is_some_and(|c| is_ident_start(*c)) {
            let end = ident_end(s, i + 1);
            out.token("var", &s[i..end]);
            i = end;
        } else if c.is_ascii_digit() {
            let end = number_end(s, i);
            out.token("num", &s[i..end]);
            i = end;
        } else if is_ident_start(c) {
            let end = ident_end(s, i);
            let word: String = s[i..end].iter().collect();
            let class = if PHP_KEYWORDS.contains(&word.as_str()) {
                Some("kw")
            } else if matches!(word.as_str(), "true" | "false" | "null") {
                Some("lit")
            } else if s.get(end) == Some(&'(') {
                Some("fn")
            } else if word.starts_with(|c: char| c.is_uppercase()) {
                Some("ty")
            } else {
                None
            };
            match class {
                Some(class) => out.token(class, &s[i..end]),
                None => out.plain(&s[i..end]),
            }
            i = end;
        } else {
            out.push_escaped(c);
            i += 1;
        }
    }
}

fn css(s: &[char], out: &mut Out) {
    let mut i = 0;
    let mut depth = 0;
    while i < s.len() {
        let c = s[i];
        if starts(s, i, "/*") {
            let end = after(s, i + 2, "*/");
            out.token("com", &s[i..end]);
            i = end;
        } else if c == '"' || c == '\'' {
            let end = string_end(s, i, c);
            out.token("str", &s[i..end]);
            i = end;
        } else if c == '{' || c == '}' {
            depth = if c == '{' {
                depth + 1
            } else {
                (depth - 1).max(0)
            };
            out.push_escaped(c);
            i += 1;
        } else if depth == 0 && !c.is_whitespace() {
            let end = (i..s.len())
                .find(|&j| s[j] == '{' || starts(s, j, "/*"))
                .unwrap_or(s.len());
            let trimmed = (i..end)
                .rev()
                .find(|&j| !s[j].is_whitespace())
                .map_or(i, |j| j + 1);
            out.token("tag", &s[i..trimmed]);
            out.plain(&s[trimmed..end]);
            i = end;
        } else if depth > 0 && (c.is_alphabetic() || c == '-') {
            let end = (i..s.len())
                .find(|&j| !(s[j].is_alphanumeric() || s[j] == '-'))
                .unwrap_or(s.len());
            if next_visible(s, end) == Some(':') {
                out.token("prop", &s[i..end]);
            } else {
                out.plain(&s[i..end]);
            }
            i = end;
        } else if c.is_ascii_digit() {
            let end = (i..s.len())
                .find(|&j| !(s[j].is_alphanumeric() || s[j] == '.' || s[j] == '%'))
                .unwrap_or(s.len());
            out.token("num", &s[i..end]);
            i = end;
        } else {
            out.push_escaped(c);
            i += 1;
        }
    }
}

/// nginx and systemd-style files: comments, the directive or key that
/// starts each statement, strings and numbers.
fn conf(s: &[char], out: &mut Out) {
    let mut i = 0;
    let mut statement = true;
    while i < s.len() {
        let c = s[i];
        if c == '#' || c == ';' && statement {
            let end = line_end(s, i);
            out.token("com", &s[i..end]);
            i = end;
        } else if c == '"' || c == '\'' {
            let end = string_end(s, i, c);
            out.token("str", &s[i..end]);
            i = end;
            statement = false;
        } else if c == '[' && statement {
            let end = after(s, i, "]");
            out.token("tag", &s[i..end]);
            i = end;
        } else if matches!(c, '\n' | ';' | '{' | '}') {
            statement = true;
            out.push_escaped(c);
            i += 1;
        } else if c.is_whitespace() {
            out.push_escaped(c);
            i += 1;
        } else {
            let end = (i..s.len())
                .find(|&j| s[j].is_whitespace() || matches!(s[j], ';' | '{' | '}' | '='))
                .unwrap_or(s.len())
                .max(i + 1);
            if statement {
                out.token("kw", &s[i..end]);
                statement = false;
            } else if s[i].is_ascii_digit() {
                out.token("num", &s[i..end]);
            } else {
                out.plain(&s[i..end]);
            }
            i = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::highlight;

    #[test]
    fn rust_tokens_get_their_colours() {
        let html = highlight(
            "rust",
            "#[derive(Debug)]\npub async fn home(id: i64) -> Result<View> { // hi\n    let s = \"a<b\"; vec![1]; 'x'; &'a str; s.len()\n}",
        );
        assert!(
            html.contains(r#"<span class="hl-attr">#[derive(Debug)]</span>"#),
            "{html}"
        );
        assert!(html.contains(r#"<span class="hl-kw">pub</span>"#));
        assert!(html.contains(r#"<span class="hl-fn">home</span>"#));
        assert!(html.contains(r#"<span class="hl-ty">Result</span>"#));
        assert!(html.contains(r#"<span class="hl-com">// hi</span>"#));
        assert!(html.contains(r#"<span class="hl-str">&quot;a&lt;b&quot;</span>"#));
        assert!(html.contains(r#"<span class="hl-mac">vec!</span>"#));
        assert!(html.contains(r#"<span class="hl-str">'x'</span>"#));
        assert!(html.contains(r#"<span class="hl-life">'a</span>"#));
        assert!(html.contains(r#"<span class="hl-fn">len</span>"#));
        assert!(html.contains(r#"<span class="hl-num">1</span>"#));
    }

    #[test]
    fn raw_strings_and_ranges() {
        let html = highlight("rust", "let x = r#\"say \"hi\"\"#; for i in 0..3 {}");
        assert!(
            html.contains(r#"<span class="hl-str">r#&quot;say &quot;hi&quot;&quot;#</span>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<span class="hl-num">0</span>..<span class="hl-num">3</span>"#),
            "{html}"
        );
    }

    #[test]
    fn templates_inside_html() {
        let html = highlight(
            "html",
            "<a href=\"{{ route('home') }}\" hx-get=\"/x\">{% if user %}{{ name | upper }}{% endif %}</a>",
        );
        assert!(
            html.contains(r#"<span class="hl-tag">&lt;a</span>"#),
            "{html}"
        );
        assert!(html.contains(r#"<span class="hl-prop">href</span>"#));
        assert!(html.contains(r#"<span class="hl-tpl">{{</span>"#));
        assert!(html.contains(r#"<span class="hl-fn">route</span>"#));
        assert!(html.contains(r#"<span class="hl-str">'home'</span>"#));
        assert!(html.contains(r#"<span class="hl-prop">hx-get</span>"#));
        assert!(html.contains(r#"<span class="hl-kw">if</span>"#));
        assert!(html.contains(r#"<span class="hl-fn">upper</span>"#));
        assert!(html.contains(r#"<span class="hl-tag">&lt;/a</span>"#));
    }

    #[test]
    fn shell_commands_flags_and_comments() {
        let html = highlight(
            "bash",
            "cargo install renox-cli --locked   # installs `rnx`\nAPP_ENV=testing rnx serve && cd \"$HOME\"",
        );
        assert!(
            html.contains(r#"<span class="hl-fn">cargo</span>"#),
            "{html}"
        );
        assert!(html.contains(r#"<span class="hl-attr">--locked</span>"#));
        assert!(html.contains(r#"<span class="hl-com"># installs `rnx`</span>"#));
        assert!(html.contains(r#"<span class="hl-var">APP_ENV</span>=testing"#));
        assert!(html.contains(r#"<span class="hl-fn">rnx</span>"#));
        assert!(html.contains(r#"<span class="hl-fn">cd</span>"#));
    }

    #[test]
    fn toml_sql_and_json() {
        let toml = highlight(
            "toml",
            "[dependencies]\nrenox = { version = \"1\", default-features = false }",
        );
        assert!(
            toml.contains(r#"<span class="hl-tag">[dependencies]</span>"#),
            "{toml}"
        );
        assert!(toml.contains(r#"<span class="hl-prop">default-features</span>"#));
        assert!(toml.contains(r#"<span class="hl-lit">false</span>"#));
        let sql = highlight("sql", "CREATE TABLE posts (id INTEGER PRIMARY KEY) -- x");
        assert!(
            sql.contains(r#"<span class="hl-kw">CREATE</span>"#),
            "{sql}"
        );
        assert!(sql.contains(r#"<span class="hl-ty">INTEGER</span>"#));
        assert!(sql.contains(r#"<span class="hl-com">-- x</span>"#));
        let json = highlight("json", r#"{"message": "x", "n": -1, "ok": true}"#);
        assert!(
            json.contains(r#"<span class="hl-prop">&quot;message&quot;</span>"#),
            "{json}"
        );
        assert!(json.contains(r#"<span class="hl-str">&quot;x&quot;</span>"#));
        assert!(json.contains(r#"<span class="hl-num">-1</span>"#));
    }

    #[test]
    fn unknown_languages_are_only_escaped() {
        assert_eq!(highlight("text", "a < b & c"), "a &lt; b &amp; c");
    }

    #[test]
    fn the_text_is_never_changed() {
        // Whatever the lexer does, removing the tags gives back the code.
        let samples = [
            ("rust", "fn main() { let s = r\"x\"; /* unterminated"),
            (
                "html",
                "<div x-data=\"{ open: false }\" {% if a %}hidden{% endif %}>",
            ),
            ("bash", "echo 'unterminated"),
            ("toml", "a = \"\"\"multi\nline\"\"\""),
            ("css", ".a { color: red; } /* x"),
            ("php", "<?php $x = fn() => 1; // hi"),
            ("nginx", "server { listen 80; # x\n}"),
            ("sql", "SELECT 'it''s'"),
        ];
        for (language, code) in samples {
            let html = highlight(language, code);
            let mut text = String::new();
            let mut in_tag = false;
            for c in html.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' if in_tag => in_tag = false,
                    c if !in_tag => text.push(c),
                    _ => {}
                }
            }
            let text = text
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&amp;", "&");
            assert_eq!(text, code, "{language}");
        }
    }
}
