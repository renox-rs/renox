//! Lint: the kit's own stylesheets use logical properties, so a page in a
//! right-to-left locale mirrors by itself. A physical `left`, `margin-right`,
//! `border-left`, `text-align: right` and the like fails this test unless it
//! has an entry in `ALLOWED` with a reason. Vendored CSS is not scanned.

/// The stylesheets that are ours: `(name, source)`.
const SHEETS: &[(&str, &str)] = &[
    (
        "renox-ui.css",
        include_str!("../../../renox-core/assets/renox-ui.css"),
    ),
    (
        "renox-grid.css",
        include_str!("../../../renox-core/assets/renox-grid.css"),
    ),
    (
        "blocks.css",
        include_str!("../../../renox-blocks/assets/blocks.css"),
    ),
    (
        "editors.css",
        include_str!("../../../renox-editors/assets/editors.css"),
    ),
];

/// Physical declarations that are meant to stay: `(sheet, selector, property, reason)`.
/// A stale entry (one that no longer matches anything) fails the test too.
const ALLOWED: &[(&str, &str, &str, &str)] = &[
    // Charts are drawn on a left-to-right axis (the chart frame is an LTR
    // isolate), so their absolute positioning is physical on purpose.
    (
        "renox-ui.css",
        ".rx-chart__y span",
        "right",
        "chart axis, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__rule",
        "left",
        "chart axis, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__rule",
        "right",
        "chart axis, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__rule--x",
        "right",
        "chart axis, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__rule--x",
        "border-left",
        "chart axis, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__rule--x.rx-chart__rule--base",
        "border-left-color",
        "chart axis, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__bar",
        "left",
        "chart bar, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__bar",
        "right",
        "chart bar, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__cross",
        "border-left",
        "chart crosshair, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__heat-row",
        "padding-right",
        "chart heatmap, LTR isolate",
    ),
    (
        "renox-ui.css",
        ".rx-chart__value",
        "margin-left",
        "chart legend, LTR isolate",
    ),
];

/// Whether `prop: value` is a physical (left/right) declaration.
fn is_physical(prop: &str, value: &str) -> bool {
    match prop {
        "left" | "right" => true,
        "text-align" | "float" | "clear" => matches!(value.trim(), "left" | "right"),
        _ => {
            let edge = [
                "margin-",
                "padding-",
                "border-",
                "scroll-margin-",
                "scroll-padding-",
            ]
            .iter()
            .any(|p| {
                prop.strip_prefix(p)
                    .is_some_and(|rest| rest.starts_with("left") || rest.starts_with("right"))
            });
            let corner = prop.starts_with("border-")
                && prop.ends_with("-radius")
                && (prop.contains("-left-") || prop.contains("-right-"));
            edge || corner
        }
    }
}

/// Every physical declaration of a sheet as `(selector, property)`.
fn physical_declarations(css: &str) -> Vec<(String, String)> {
    // Drop comments.
    let mut clean = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        clean.push_str(&rest[..start]);
        rest = match rest[start..].find("*/") {
            Some(end) => &rest[start + end + 2..],
            None => "",
        };
    }
    clean.push_str(rest);

    let mut found = Vec::new();
    let mut selectors: Vec<String> = Vec::new();
    let mut chunk = String::new();
    let mut paren = 0usize;
    for ch in clean.chars() {
        match ch {
            '(' => {
                paren += 1;
                chunk.push(ch);
            }
            ')' => {
                paren = paren.saturating_sub(1);
                chunk.push(ch);
            }
            '{' if paren == 0 => {
                selectors.push(chunk.split_whitespace().collect::<Vec<_>>().join(" "));
                chunk.clear();
            }
            '}' if paren == 0 => {
                declaration(&chunk, &selectors, &mut found);
                chunk.clear();
                selectors.pop();
            }
            ';' if paren == 0 => {
                declaration(&chunk, &selectors, &mut found);
                chunk.clear();
            }
            _ => chunk.push(ch),
        }
    }
    found
}

fn declaration(chunk: &str, selectors: &[String], found: &mut Vec<(String, String)>) {
    let Some((prop, value)) = chunk.split_once(':') else {
        return;
    };
    let prop = prop.trim();
    if prop.is_empty() || prop.contains(' ') || !is_physical(prop, value) {
        return;
    }
    // The innermost selector that is not an at-rule.
    let selector = selectors
        .iter()
        .rev()
        .find(|s| !s.starts_with('@'))
        .cloned()
        .unwrap_or_default();
    found.push((selector, prop.to_string()));
}

#[test]
fn kit_css_uses_logical_properties() {
    let mut offenders = Vec::new();
    let mut used = vec![false; ALLOWED.len()];
    for (name, css) in SHEETS {
        for (selector, prop) in physical_declarations(css) {
            let hit = ALLOWED
                .iter()
                .position(|(s, sel, p, _)| s == name && *sel == selector && *p == prop);
            match hit {
                Some(i) => used[i] = true,
                None => offenders.push(format!("{name}: `{selector}` uses physical `{prop}`")),
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "use logical properties (margin-inline-start, inset-inline-end, text-align: start, ...) \
         or add an allowlist entry with a reason:\n{}",
        offenders.join("\n")
    );
    let stale: Vec<_> = ALLOWED
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|((s, sel, p, _), _)| format!("{s}: `{sel}` `{p}`"))
        .collect();
    assert!(
        stale.is_empty(),
        "stale allowlist entries:\n{}",
        stale.join("\n")
    );
}

#[test]
fn the_lint_flags_physical_properties() {
    let css = "/* margin-left: 1px; */ .a { margin-left: 1px; color: red } \
               @media (x) { .b { text-align: right; inset-inline-start: 0 } } \
               .c { border-top-left-radius: 2px; border-start-start-radius: 2px; left: 0 }";
    assert_eq!(
        physical_declarations(css),
        vec![
            (".a".to_string(), "margin-left".to_string()),
            (".b".to_string(), "text-align".to_string()),
            (".c".to_string(), "border-top-left-radius".to_string()),
            (".c".to_string(), "left".to_string()),
        ]
    );
}
