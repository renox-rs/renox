//! Attribute values to MiniJinja expressions (Decisions 10 and 11 of #372).
//!
//! Pure functions: they know nothing about components. The caller adds the component's name to
//! the error messages.

use super::scan::Attr;
use super::suggest;

/// What a property accepts as a written value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Text, possibly with `{{ }}`.
    Text,
    /// `true` or `false`.
    Bool,
    /// A number.
    Number,
    /// Data: only `:name="expr"`.
    Data,
    /// One of a fixed list of words.
    Enum,
}

/// Decodes the five basic entities. `&amp;` goes last so `&amp;lt;` becomes `&lt;`.
fn decode(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Text as a MiniJinja string literal: entities decoded, `\` and `"` escaped, line breaks
/// turned into spaces.
pub(crate) fn literal(text: &str) -> String {
    let decoded = decode(text);
    let mut out = String::with_capacity(decoded.len() + 2);
    out.push('"');
    for c in decoded.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Text with `{{ }}` as one expression: `(e)` when the value is exactly one expression,
/// else the parts joined with ` ~ `.
pub(crate) fn text_expr(value: &str) -> Result<String, String> {
    if value.contains("{%") {
        return Err(
            "attribute \"{n}\" holds {% … %}; use {{ … }} for text or :{n} for data".to_owned(),
        );
    }
    let mut parts: Vec<String> = Vec::new();
    let mut expressions = 0;
    let mut rest = value;
    while let Some(start) = rest.find("{{") {
        if start > 0 {
            parts.push(literal(&rest[..start]));
        }
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| "\"{{\" in an attribute is never closed".to_owned())?;
        let inner = after[..end].trim_start_matches('-').trim_end_matches('-');
        let inner = inner.trim();
        if inner.is_empty() {
            return Err("an empty {{ }} in an attribute".to_owned());
        }
        parts.push(format!("({})", inner.replace(['\n', '\r'], " ")));
        expressions += 1;
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        parts.push(literal(rest));
    }
    if expressions == 0 && parts.is_empty() {
        return Ok("\"\"".to_owned());
    }
    Ok(parts.join(" ~ "))
}

/// A macro parameter name: `hide-label` becomes `hide_label`.
pub(crate) fn snake(name: &str) -> String {
    name.replace('-', "_")
}

/// The expression for one attribute given to a property of `kind`. `values` lists the words of
/// an `Enum`. Error messages carry `{n}`-style names already filled in, without the component.
pub(crate) fn prop_expr(attr: &Attr<'_>, kind: Kind, values: &[&str]) -> Result<String, String> {
    if let Some(name) = attr.name.strip_prefix(':') {
        let value = attr.value.unwrap_or("");
        if value.trim().is_empty() {
            return Err(format!("\":{name}\" needs an expression"));
        }
        return Ok(format!("({})", value.trim().replace(['\n', '\r'], " ")));
    }
    let n = attr.name;
    let Some(value) = attr.value else {
        return Ok("true".to_owned());
    };
    if value.contains("{%") {
        return Err(format!(
            "attribute \"{n}\" holds {{% … %}}; use {{{{ … }}}} for text or :{n} for data"
        ));
    }
    let has_expr = value.contains("{{");
    match kind {
        Kind::Text => text_expr(value),
        Kind::Data => Err(format!("\"{n}\" takes data: write :{n}=\"…\"")),
        Kind::Bool => match value {
            "true" | "false" => Ok(value.to_owned()),
            _ => Err(format!("\"{n}\" is true or false: write {n} or :{n}=\"…\"")),
        },
        Kind::Number => {
            if !has_expr && value.trim().parse::<f64>().is_ok() {
                Ok(value.trim().to_owned())
            } else {
                Err(format!("\"{n}\" takes a number"))
            }
        }
        Kind::Enum => {
            if has_expr {
                return text_expr(value);
            }
            if values.contains(&value) {
                return Ok(literal(value));
            }
            let hint = suggest::did_you_mean(value, values.iter().copied())
                .map(|x| format!("; did you mean \"{x}\"?"))
                .unwrap_or_default();
            Err(format!(
                "\"{n}\" must be one of: {}{hint}",
                values.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr<'a>(name: &'a str, value: Option<&'a str>) -> Attr<'a> {
        Attr {
            name,
            value,
            quoted: value.is_some(),
            span: 0..0,
            line: 1,
        }
    }

    #[test]
    fn literal_decodes_and_escapes() {
        assert_eq!(literal("Fish &amp; chips"), "\"Fish & chips\"");
        assert_eq!(
            literal("&lt;b&gt; &quot;x&quot; &#39;y&#39;"),
            "\"<b> \\\"x\\\" 'y'\""
        );
        assert_eq!(literal("a\\b\"c"), "\"a\\\\b\\\"c\"");
        assert_eq!(literal("a\nb"), "\"a b\"");
        assert_eq!(literal("&amp;lt;"), "\"&lt;\"");
    }

    #[test]
    fn text_expr_forms() {
        assert_eq!(text_expr("plain").unwrap(), "\"plain\"");
        assert_eq!(text_expr("").unwrap(), "\"\"");
        assert_eq!(text_expr("{{ t('a') }}").unwrap(), "(t('a'))");
        assert_eq!(text_expr("{{- x -}}").unwrap(), "(x)");
        assert_eq!(
            text_expr("Delete “{{ s.name }}”?").unwrap(),
            "\"Delete “\" ~ (s.name) ~ \"”?\""
        );
        assert_eq!(text_expr("{{ a }}{{ b }}").unwrap(), "(a) ~ (b)");
        assert_eq!(text_expr("{{ a }} ").unwrap(), "(a) ~ \" \"");
    }

    #[test]
    fn text_expr_errors() {
        let e = text_expr("{% if x %}a{% endif %}").unwrap_err();
        assert!(e.contains("holds {% … %}"));
        assert!(text_expr("{{ a").is_err());
        assert!(text_expr("{{ }}").is_err());
    }

    #[test]
    fn snake_case() {
        assert_eq!(snake("hide-label"), "hide_label");
        assert_eq!(snake("a"), "a");
    }

    #[test]
    fn colon_and_bare() {
        let k = Kind::Data;
        assert_eq!(
            prop_expr(&attr(":rows", Some("products")), k, &[]).unwrap(),
            "(products)"
        );
        assert_eq!(
            prop_expr(&attr(":rows", Some("a\nb")), k, &[]).unwrap(),
            "(a b)"
        );
        assert!(prop_expr(&attr(":rows", Some("")), k, &[]).is_err());
        assert_eq!(
            prop_expr(&attr("required", None), Kind::Bool, &[]).unwrap(),
            "true"
        );
    }

    #[test]
    fn kinds() {
        let t = |n, v, k, vals: &[&str]| prop_expr(&attr(n, Some(v)), k, vals);
        assert_eq!(
            t("label", "Fish &amp; chips", Kind::Text, &[]).unwrap(),
            "\"Fish & chips\""
        );
        assert_eq!(
            t("label", "{{ t('a') }}", Kind::Text, &[]).unwrap(),
            "(t('a'))"
        );
        assert!(
            t("label", "{% x %}", Kind::Text, &[])
                .unwrap_err()
                .starts_with(
                    "attribute \"label\" holds {% … %}; use {{ … }} for text or :label for data"
                )
        );
        assert_eq!(
            t("rows", "x", Kind::Data, &[]).unwrap_err(),
            "\"rows\" takes data: write :rows=\"…\""
        );
        assert_eq!(t("required", "true", Kind::Bool, &[]).unwrap(), "true");
        assert_eq!(t("required", "false", Kind::Bool, &[]).unwrap(), "false");
        assert_eq!(
            t("required", "yes", Kind::Bool, &[]).unwrap_err(),
            "\"required\" is true or false: write required or :required=\"…\""
        );
        assert_eq!(t("rows", "3", Kind::Number, &[]).unwrap(), "3");
        assert_eq!(t("step", "0.5", Kind::Number, &[]).unwrap(), "0.5");
        assert_eq!(
            t("rows", "many", Kind::Number, &[]).unwrap_err(),
            "\"rows\" takes a number"
        );
        let vals = ["primary", "danger"];
        assert_eq!(
            t("tone", "danger", Kind::Enum, &vals).unwrap(),
            "\"danger\""
        );
        assert_eq!(t("tone", "{{ x }}", Kind::Enum, &vals).unwrap(), "(x)");
        let e = t("tone", "dangr", Kind::Enum, &vals).unwrap_err();
        assert_eq!(
            e,
            "\"tone\" must be one of: primary, danger; did you mean \"danger\"?"
        );
        let e = t("tone", "zzzzzzzz", Kind::Enum, &vals).unwrap_err();
        assert_eq!(e, "\"tone\" must be one of: primary, danger");
    }
}
