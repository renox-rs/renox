//! Filters every template gets: `number`, `money`, `date`, `since`,
//! `words` and `markdown`.

use minijinja::value::Kwargs;
use minijinja::{Error, ErrorKind, State, Value};

/// The request's locale (`app.locale`), or `en`.
fn locale(state: &State) -> String {
    state
        .lookup("app")
        .and_then(|app| app.get_attr("locale").ok())
        .and_then(|l| l.as_str().map(str::to_owned))
        .unwrap_or_else(|| "en".into())
}

/// Thousands and decimal separators for a locale.
fn separators(locale: &str) -> (&'static str, &'static str) {
    let language = locale.split(['-', '_']).next().unwrap_or(locale);
    match language {
        "id" | "de" | "nl" | "es" | "it" | "pt" | "tr" | "da" | "nb" | "ms" | "vi" => (".", ","),
        "fr" | "ru" | "pl" | "cs" | "sv" | "fi" | "uk" => ("\u{202f}", ","),
        _ => (",", "."),
    }
}

/// `{{ 75000 | number }}` → `75,000` (en) or `75.000` (es, de);
/// `{{ 3.14159 | number(2) }}` → `3.14` / `3,14`.
pub(crate) fn number(state: &State, value: Value, decimals: Option<u32>) -> Result<String, Error> {
    let n = to_number("number", &value)?;
    Ok(format_number(n, decimals.unwrap_or(0), &locale(state)))
}

/// A template value as a number: an integer, a float or numeric text.
fn to_number(filter: &str, value: &Value) -> Result<f64, Error> {
    Ok(if let Ok(i) = i64::try_from(value.clone()) {
        i as f64
    } else if let Some(s) = value.as_str() {
        s.trim().parse().map_err(|_| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("{filter}: `{s}` is not a number"),
            )
        })?
    } else {
        f64::try_from(value.clone()).map_err(|_| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("{filter}: `{value}` is not a number"),
            )
        })?
    })
}

/// `{{ order.total | money }}` → `$75.00` for `7500` with `APP_CURRENCY=USD`
/// (the default), `Rp 75,000` for `75000` with `IDR`, with the page's
/// separators. The amount is in the currency's smallest unit (cents), as
/// the data grid's money columns take it: it is divided by
/// `10^currency_decimals(code)` (100 for `USD`, 1 for `IDR`). Keywords:
/// `currency` (another ISO 4217 code for this amount), `decimals`, and
/// `divide_by`, which replaces that divisor (`divide_by=1` for an amount
/// already in whole units).
pub(crate) fn money(
    currency: String,
) -> impl Fn(&State, Value, Kwargs) -> Result<String, Error> + Send + Sync + 'static {
    move |state: &State, value: Value, kwargs: Kwargs| {
        let code: Option<String> = kwargs.get("currency")?;
        let decimals: Option<u32> = kwargs.get("decimals")?;
        let divide_by: Option<f64> = kwargs.get("divide_by")?;
        kwargs.assert_all_used()?;
        let code = code.map_or_else(|| currency.clone(), |c| c.trim().to_ascii_uppercase());
        let amount = to_number("money", &value)? / money_divisor(&code, divide_by);
        Ok(format_money(amount, &code, decimals, &locale(state)))
    }
}

/// What an amount in `code`'s smallest unit is divided by to give whole
/// units: `divide_by` when given (and not 0), else `10^currency_decimals`.
pub(crate) fn money_divisor(code: &str, divide_by: Option<f64>) -> f64 {
    divide_by
        .filter(|d| *d != 0.0)
        .unwrap_or_else(|| 10f64.powi(currency_decimals(code) as i32))
}

/// `amount` in **whole units** of the currency `code` (ISO 4217; unlike
/// the `money` template filter, which takes the smallest unit), with its
/// symbol, its usual decimals (or `decimals`) and the locale's separators: `$1,250.50`,
/// `€1.250,50` in `de`, `Rp 75.000` in `es` (what the `money` template filter
/// uses). An unknown code is written before the amount (`CHF 12.00`).
pub fn format_money(amount: f64, code: &str, decimals: Option<u32>, locale: &str) -> String {
    let (symbol, usual) = currency(code);
    let figures = format_number(amount.abs(), decimals.unwrap_or(usual), locale);
    let sign = if amount < 0.0 && figures.chars().any(|c| c.is_ascii_digit() && c != '0') {
        "-"
    } else {
        ""
    };
    // `Rp 75.000`, `RM 12.00`, but `$75.00`.
    let space = if symbol.ends_with(|c: char| c.is_ascii_alphabetic()) {
        " "
    } else {
        ""
    };
    format!("{sign}{symbol}{space}{figures}")
}

/// The decimals a currency is usually written with: 2 for `USD` or `AED`,
/// 0 for `IDR` or `JPY`. An amount in the smallest unit is divided by
/// `10^decimals` to give whole units (as the data grid's `money` columns
/// do).
///
/// ```
/// assert_eq!(renox::currency_decimals("USD"), 2);
/// assert_eq!(renox::currency_decimals("IDR"), 0);
/// ```
pub fn currency_decimals(code: &str) -> u32 {
    currency(code).1
}

/// A currency's symbol and usual decimals from the one table in
/// [`crate::money`] (the code itself and 2 when it isn't listed).
fn currency(code: &str) -> (&str, u32) {
    crate::money::info(code)
}

/// `n` with `decimals` decimals and the locale's separators: `75.000` in
/// `id`, `75,000` in `en` (what the `number` template filter uses).
pub fn format_number(n: f64, decimals: u32, locale: &str) -> String {
    let (thousands, decimal) = separators(locale);
    let fixed = format!("{:.*}", decimals as usize, n.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push_str(thousands);
        }
        grouped.push(digit);
    }
    let sign = if n < 0.0 && fixed.chars().any(|c| c.is_ascii_digit() && c != '0') {
        "-"
    } else {
        ""
    };
    if fraction.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}{decimal}{fraction}")
    }
}

/// `{{ order.created_at | date }}` → `2026-10-01`; `date("%d/%m/%Y %H:%M")`
/// with chrono's format codes. A moment (`created_at`, …) is shown in
/// `APP_TIMEZONE`; a date or a local date-time is shown as it is.
pub(crate) fn date(
    zone: crate::timezone::Zone,
) -> impl Fn(Value, Option<String>, Kwargs) -> Result<String, Error> + Send + Sync + 'static {
    move |value: Value, format: Option<String>, kwargs: Kwargs| {
        kwargs.assert_all_used()?;
        let text = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        let format = format.as_deref().unwrap_or("%Y-%m-%d");
        let formatted = if let Ok(moment) = chrono::DateTime::parse_from_rfc3339(&text) {
            let offset = zone.fixed_at(moment.timestamp());
            moment.with_timezone(&offset).format(format).to_string()
        } else if let Ok(local) = text.parse::<chrono::NaiveDateTime>() {
            local.format(format).to_string()
        } else if let Ok(day) = text.parse::<chrono::NaiveDate>() {
            day.format(format).to_string()
        } else {
            return Err(Error::new(
                ErrorKind::InvalidOperation,
                format!("date: `{text}` is not a date"),
            ));
        };
        Ok(formatted)
    }
}

/// A moment from a template value: an RFC 3339 time, or a local date-time
/// or date (both in `zone`).
fn moment(value: &Value, zone: crate::timezone::Zone) -> Option<i64> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    if let Ok(moment) = chrono::DateTime::parse_from_rfc3339(&text) {
        return Some(moment.timestamp());
    }
    let local = text.parse::<chrono::NaiveDateTime>().ok().or_else(|| {
        text.parse::<chrono::NaiveDate>()
            .ok()
            .and_then(|day| day.and_hms_opt(0, 0, 0))
    })?;
    zone.resolve(local)
        .or_else(|| Some(local.and_utc().timestamp()))
}

/// `{{ order.created_at | since }}` → `3 hours ago`, `in 2 days`, `just
/// now` (`3 hours ago`), from the clock `TestApp::travel`
/// moves. The texts are `ui.since.*` translations.
pub(crate) fn since(
    zone: crate::timezone::Zone,
) -> impl Fn(&State, Value) -> Result<String, Error> + Send + Sync + 'static {
    move |state: &State, value: Value| {
        let then = moment(&value, zone).ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("since: `{value}` is not a date"),
            )
        })?;
        let seconds = crate::clock::unix_secs() - then;
        let (key, count) = since_unit(seconds.unsigned_abs());
        let text = |key: &str, args: &[(&str, Value)]| -> String {
            let kwargs = minijinja::value::Kwargs::from_iter(
                args.iter().map(|(k, v)| ((*k).to_owned(), v.clone())),
            );
            state
                .lookup("t")
                .and_then(|t| t.call(state, &[Value::from(key), Value::from(kwargs)]).ok())
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| {
                    let params: Vec<(&str, String)> = args
                        .iter()
                        .filter(|(k, _)| *k != "count")
                        .map(|(k, v)| (*k, v.to_string()))
                        .collect();
                    let count = args
                        .iter()
                        .find(|(k, _)| *k == "count")
                        .and_then(|(_, v)| i64::try_from(v.clone()).ok());
                    crate::i18n::format(&crate::i18n::builtin_text("en", key), &params, count)
                })
        };
        let Some(unit) = key else {
            return Ok(text("ui.since.now", &[]));
        };
        let time = text(unit, &[("count", Value::from(count))]);
        let side = if seconds >= 0 {
            "ui.since.past"
        } else {
            "ui.since.future"
        };
        Ok(text(side, &[("time", Value::from(time))]))
    }
}

/// The unit and count for a distance in seconds; `None` under 45 seconds.
fn since_unit(seconds: u64) -> (Option<&'static str>, i64) {
    let round = |n: f64| (n.round() as i64).max(1);
    let s = seconds as f64;
    match seconds {
        0..45 => (None, 0),
        45..2_700 => (Some("ui.since.minutes"), round(s / 60.0)),
        2_700..79_200 => (Some("ui.since.hours"), round(s / 3_600.0)),
        79_200..2_246_400 => (Some("ui.since.days"), round(s / 86_400.0)),
        2_246_400..27_648_000 => (Some("ui.since.months"), round(s / 2_629_746.0)),
        _ => (Some("ui.since.years"), round(s / 31_556_952.0)),
    }
}

/// `{{ post.summary | words(20) }}`: the first 20 words, then `…` (or
/// `end="…"`) when there were more.
pub(crate) fn words(value: Value, count: usize, kwargs: Kwargs) -> Result<String, Error> {
    let end: Option<String> = kwargs.get("end")?;
    kwargs.assert_all_used()?;
    let text = value.as_str().map(str::to_owned).unwrap_or_else(|| {
        if value.is_none() || value.is_undefined() {
            String::new()
        } else {
            value.to_string()
        }
    });
    let all: Vec<&str> = text.split_whitespace().collect();
    if all.len() <= count {
        return Ok(all.join(" "));
    }
    Ok(format!(
        "{}{}",
        all[..count].join(" "),
        end.as_deref().unwrap_or("…")
    ))
}

/// `{{ product.description | markdown }}`: Markdown (CommonMark with
/// tables, strikethrough and task lists) as HTML. Safe for text people
/// typed: HTML in it is shown as text, and links or images to anything but
/// http(s), mailto, tel or a relative URL point nowhere.
pub(crate) fn markdown(value: Value) -> Value {
    let text = value.as_str().map(str::to_owned).unwrap_or_else(|| {
        if value.is_none() || value.is_undefined() {
            String::new()
        } else {
            value.to_string()
        }
    });
    Value::from_safe_string(markdown_html(&text))
}

fn markdown_html(text: &str) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events = Parser::new_ext(text, options).map(|event| match event {
        Event::Html(html) | Event::InlineHtml(html) => Event::Text(html),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_url(dest_url),
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
            dest_url: safe_url(dest_url),
            title,
            id,
        }),
        other => other,
    });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html
}

/// The URL if it is http(s), mailto, tel or relative, else `#`.
fn safe_url(url: pulldown_cmark::CowStr<'_>) -> pulldown_cmark::CowStr<'_> {
    // Browsers ignore control characters and spaces inside a scheme.
    let cleaned: String = url
        .chars()
        .filter(|c| !c.is_ascii_control() && !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let scheme_end = cleaned.find(':');
    let before_path = cleaned.find(['/', '?', '#']).unwrap_or(usize::MAX);
    match scheme_end {
        Some(end) if end < before_path => {
            if matches!(&cleaned[..end], "http" | "https" | "mailto" | "tel") {
                url
            } else {
                "#".into()
            }
        }
        _ => url,
    }
}

/// The value of a conditional attribute (`?name="expr"` in a template, #518): `false`, `none`
/// and an undefined name leave the attribute out (`none`), `true` writes it bare (`true`), and
/// anything else is its text.
pub(crate) fn attr_value(value: Value) -> Value {
    if value.is_undefined() || value.is_none() {
        return Value::from(());
    }
    if let minijinja::value::ValueKind::Bool = value.kind() {
        return if value.is_true() {
            Value::from(true)
        } else {
            Value::from(())
        };
    }
    Value::from(value.to_string())
}

/// `rx_attr("name", value)`: ` name`, ` name="text"` (escaped) or nothing, by [`attr_value`].
/// What `?name="expr"` compiles to on an HTML element.
pub(crate) fn rx_attr(name: String, value: Value) -> Value {
    let value = attr_value(value);
    if value.is_none() {
        return Value::from_safe_string(String::new());
    }
    if matches!(value.kind(), minijinja::value::ValueKind::Bool) {
        return Value::from_safe_string(format!(" {name}"));
    }
    let mut out = format!(" {name}=\"");
    for c in value.to_string().chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out.push('"');
    Value::from_safe_string(out)
}

/// `class_names('btn', {'btn-active': active, 'hidden': not shown})`: the
/// strings given, plus the keys of maps whose value is true, joined with
/// spaces (Blade's `@class`). Empty strings and `none` are skipped.
///
/// ```text
/// <a class="{{ class_names('tab', {'tab-active': route_is('orders.*')}) }}">Orders</a>
/// ```
pub(crate) fn class_names(parts: minijinja::value::Rest<minijinja::Value>) -> String {
    let mut classes: Vec<String> = Vec::new();
    for part in parts.iter() {
        if let Some(text) = part.as_str() {
            classes.extend(text.split_whitespace().map(str::to_owned));
        } else if part.kind() == minijinja::value::ValueKind::Map
            && let Ok(keys) = part.try_iter()
        {
            for key in keys {
                let on = part.get_item(&key).is_ok_and(|v| v.is_true());
                if on && let Some(name) = key.as_str() {
                    classes.extend(name.split_whitespace().map(str::to_owned));
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    classes.retain(|class| seen.insert(class.clone()));
    classes.join(" ")
}

/// `{{ sparkline(values) }}`: a small chart of numbers as inline SVG, for a
/// grid cell or a card. `kind="bars"` draws bars instead of a line;
/// `width`/`height` in pixels (96×28). It takes the text color, so a
/// `class="rx-up"` around it colors it.
///
/// ```text
/// {{ sparkline(row.trend) }}  {{ sparkline([3, 5, 2, 8], kind="bars", width=60) }}
/// ```
pub(crate) fn sparkline(
    values: minijinja::Value,
    kwargs: minijinja::value::Kwargs,
) -> Result<minijinja::Value, minijinja::Error> {
    let kind: Option<String> = kwargs.get("kind")?;
    let width: Option<f64> = kwargs.get("width")?;
    let height: Option<f64> = kwargs.get("height")?;
    let label: Option<String> = kwargs.get("label")?;
    kwargs.assert_all_used()?;
    let (w, h) = (
        width.unwrap_or(96.0).max(8.0),
        height.unwrap_or(28.0).max(8.0),
    );
    let points: Vec<f64> = match values.try_iter() {
        Ok(iter) => iter.filter_map(|v| f64::try_from(v).ok()).collect(),
        Err(_) => Vec::new(),
    };
    Ok(minijinja::Value::from_safe_string(spark_svg(
        &points,
        kind.as_deref() == Some("bars"),
        w,
        h,
        label.as_deref(),
    )))
}

fn spark_svg(points: &[f64], bars: bool, w: f64, h: f64, label: Option<&str>) -> String {
    let fmt = |v: f64| format!("{:.1}", v);
    let aria = match (label, points.first(), points.last()) {
        (Some(label), ..) => label.to_owned(),
        (None, Some(first), Some(last)) => format!("{} → {}", trim(*first), trim(*last)),
        _ => String::new(),
    };
    let aria = aria
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;");
    let mut svg = format!(
        "<svg class=\"rx-spark\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-label=\"{aria}\">"
    );
    if !points.is_empty() {
        let (lo, hi) = points
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        let span = if hi > lo { hi - lo } else { 1.0 };
        let pad = 2.0;
        let y = |v: f64| h - pad - (v - lo) / span * (h - 2.0 * pad);
        if bars {
            let step = w / points.len() as f64;
            let base = if lo < 0.0 {
                y(0.0_f64.clamp(lo, hi))
            } else {
                h - pad
            };
            for (i, v) in points.iter().enumerate() {
                let top = if lo >= 0.0 && hi == lo { pad } else { y(*v) };
                let (top, bottom) = if top < base { (top, base) } else { (base, top) };
                svg.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"1\" fill=\"currentColor\"/>",
                    fmt(i as f64 * step + step * 0.15),
                    fmt(top),
                    fmt(step * 0.7),
                    fmt((bottom - top).max(1.0)),
                ));
            }
        } else {
            let step = if points.len() > 1 {
                (w - 2.0 * pad) / (points.len() - 1) as f64
            } else {
                0.0
            };
            let coords: Vec<String> = points
                .iter()
                .enumerate()
                .map(|(i, v)| format!("{},{}", fmt(pad + i as f64 * step), fmt(y(*v))))
                .collect();
            svg.push_str(&format!(
                "<polyline points=\"{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
                coords.join(" ")
            ));
            if let Some(last) = coords.last() {
                let (cx, cy) = last.split_once(',').unwrap_or(("0", "0"));
                svg.push_str(&format!(
                    "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"2\" fill=\"currentColor\"/>"
                ));
            }
        }
    }
    svg.push_str("</svg>");
    svg
}

fn trim(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparklines_draw_lines_and_bars() {
        let line = spark_svg(&[1.0, 3.0, 2.0], false, 96.0, 28.0, None);
        assert!(line.contains("<polyline points=\"2.0,"), "{line}");
        assert!(line.contains("aria-label=\"1 → 2\""));
        let bars = spark_svg(&[1.0, -2.0, 4.0], true, 60.0, 20.0, Some("Sales <q1>"));
        assert_eq!(bars.matches("<rect").count(), 3);
        assert!(bars.contains("aria-label=\"Sales &lt;q1>\""));
        assert!(spark_svg(&[], false, 96.0, 28.0, None).ends_with("></svg>"));
        assert!(spark_svg(&[5.0], false, 96.0, 28.0, None).contains("<circle"));
        // Fractions keep their decimals in the label.
        let label = spark_svg(&[1.5, 2.0, 2.25], false, 96.0, 28.0, None);
        assert!(label.contains("aria-label=\"1.5 → 2.25\""), "{label}");
    }

    #[test]
    fn formats_money_per_currency_and_locale() {
        assert_eq!(format_money(75_000.0, "IDR", None, "es"), "Rp 75.000");
        assert_eq!(format_money(1_250.5, "USD", None, "en"), "$1,250.50");
        assert_eq!(format_money(1_250.5, "EUR", None, "de"), "€1.250,50");
        assert_eq!(format_money(-5_000.0, "IDR", None, "en"), "-Rp 5,000");
        assert_eq!(format_money(12.0, "CHF", None, "en"), "CHF 12.00");
        assert_eq!(format_money(12.0, "MYR", Some(0), "en"), "RM 12");
        assert_eq!(format_money(-0.001, "USD", None, "en"), "$0.00");
    }

    #[test]
    fn money_is_divided_by_the_currencys_smallest_unit() {
        // Cents for USD, nothing for IDR and JPY; `divide_by` wins.
        assert_eq!(money_divisor("USD", None), 100.0);
        assert_eq!(money_divisor("IDR", None), 1.0);
        assert_eq!(money_divisor("JPY", None), 1.0);
        assert_eq!(money_divisor("XYZ", None), 100.0);
        assert_eq!(money_divisor("USD", Some(1.0)), 1.0);
        assert_eq!(money_divisor("IDR", Some(100.0)), 100.0);
        assert_eq!(money_divisor("USD", Some(0.0)), 100.0);
    }

    #[test]
    fn picks_units_for_distances() {
        assert_eq!(since_unit(10), (None, 0));
        assert_eq!(since_unit(60), (Some("ui.since.minutes"), 1));
        assert_eq!(since_unit(44 * 60), (Some("ui.since.minutes"), 44));
        assert_eq!(since_unit(3 * 3_600), (Some("ui.since.hours"), 3));
        assert_eq!(since_unit(23 * 3_600), (Some("ui.since.days"), 1));
        assert_eq!(since_unit(40 * 86_400), (Some("ui.since.months"), 1));
        assert_eq!(since_unit(400 * 86_400), (Some("ui.since.years"), 1));
    }

    #[test]
    fn markdown_keeps_html_and_scripts_out() {
        let html = markdown_html(
            "**Bold** <script>alert(1)</script>\n\n[x](javascript:alert(1)) [y](https://renox.dev) [z](/a:b) ![i](JaVaScRiPt:x) [w](%20data:text/html,x)",
        );
        assert!(html.contains("<strong>Bold</strong>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(
            !html.to_lowercase().contains("script:") && !html.contains("data:"),
            "{html}"
        );
        assert!(
            html.contains("href=\"https://renox.dev\"") && html.contains("href=\"/a:b\""),
            "{html}"
        );
        assert!(
            html.contains("href=\"#\"") && html.contains("src=\"#\""),
            "{html}"
        );
        let table = markdown_html("| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n\n~~old~~");
        assert!(
            table.contains("<table>") && table.contains("checkbox") && table.contains("<del>"),
            "{table}"
        );
    }

    #[test]
    fn formats_numbers_per_locale() {
        assert_eq!(format_number(75_000.0, 0, "es"), "75.000");
        assert_eq!(format_number(1_234_567.891, 2, "en"), "1,234,567.89");
        assert_eq!(format_number(-1234.5, 1, "de"), "-1.234,5");
        assert_eq!(format_number(999.0, 0, "en"), "999");
        assert_eq!(format_number(-0.001, 2, "en"), "0.00");
    }

    // #251: the filters' error messages and their less common inputs, in an
    // environment with no `t` (as in some renders), so `since` falls back to
    // the built-in English texts.

    fn env(zone: crate::timezone::Zone) -> minijinja::Environment<'static> {
        let mut env = minijinja::Environment::new();
        env.add_filter("number", number);
        env.add_filter("date", date(zone));
        env.add_filter("since", since(zone));
        env.add_filter("words", words);
        env.add_filter("markdown", markdown);
        env.add_function("class_names", class_names);
        env.add_function("sparkline", sparkline);
        env
    }

    fn render(env: &minijinja::Environment<'static>, source: &str) -> Result<String, String> {
        env.render_str(
            source,
            minijinja::context! { nothing => (), list => vec![1, 2] },
        )
        .map_err(|err| format!("{err:#}"))
    }

    #[test]
    fn number_reads_numeric_text_and_names_what_isnt() {
        let env = env(crate::timezone::Zone::UTC);
        assert_eq!(
            render(&env, "{{ ' 1234.5 ' | number(1) }}").unwrap(),
            "1,234.5"
        );
        let err = render(&env, "{{ 'cheap' | number }}").unwrap_err();
        assert!(err.contains("number: `cheap` is not a number"), "{err}");
        let err = render(&env, "{{ list | number }}").unwrap_err();
        assert!(err.contains("is not a number"), "{err}");
    }

    #[test]
    fn date_shows_moments_in_the_apps_zone() {
        let jakarta: crate::timezone::Zone = "+07:00".parse().unwrap();
        let env = env(jakarta);
        // A moment moves to the zone; a local date-time or a date stays.
        assert_eq!(
            render(
                &env,
                "{{ '2026-10-01T23:30:00Z' | date('%Y-%m-%d %H:%M') }}"
            )
            .unwrap(),
            "2026-10-02 06:30"
        );
        assert_eq!(
            render(&env, "{{ '2026-10-01T23:30:00' | date('%H:%M') }}").unwrap(),
            "23:30"
        );
        assert_eq!(
            render(&env, "{{ '2026-10-01' | date('%d/%m') }}").unwrap(),
            "01/10"
        );
        let err = render(&env, "{{ 'tomorrow' | date }}").unwrap_err();
        assert!(err.contains("date: `tomorrow` is not a date"), "{err}");
    }

    #[test]
    fn since_without_t_uses_the_english_texts() {
        let env = env(crate::timezone::Zone::UTC);
        let now = chrono::Utc::now();
        let at = |delta: i64| (now + chrono::Duration::seconds(delta)).to_rfc3339();
        let show = |delta: i64| {
            env.render_str("{{ at | since }}", minijinja::context! { at => at(delta) })
                .unwrap()
        };
        assert_eq!(show(-10), "just now");
        assert_eq!(show(-3 * 3600), "3 hours ago");
        assert_eq!(show(2 * 86_400 + 60), "in 2 days");
        // A local date-time and a date are read in the zone too.
        let yesterday = (now - chrono::Duration::days(1)).date_naive().to_string();
        let shown = env
            .render_str("{{ at | since }}", minijinja::context! { at => yesterday })
            .unwrap();
        assert!(shown.ends_with("ago"), "{shown}");
        let err = render(&env, "{{ 'later' | since }}").unwrap_err();
        assert!(err.contains("since: `later` is not a date"), "{err}");
    }

    #[test]
    fn words_and_markdown_take_nothing_and_other_values() {
        let env = env(crate::timezone::Zone::UTC);
        assert_eq!(render(&env, "[{{ nothing | words(3) }}]").unwrap(), "[]");
        assert_eq!(render(&env, "[{{ missing | words(3) }}]").unwrap(), "[]");
        assert_eq!(
            render(&env, "{{ 'one two three four' | words(2, end=' (more)') }}").unwrap(),
            "one two (more)"
        );
        assert_eq!(render(&env, "{{ 12345 | words(1) }}").unwrap(), "12345");
        assert_eq!(render(&env, "[{{ nothing | markdown }}]").unwrap(), "[]");
        assert!(
            render(&env, "{{ 42 | markdown }}")
                .unwrap()
                .contains("<p>42</p>")
        );
    }

    #[test]
    fn class_names_skip_values_that_are_neither_text_nor_maps() {
        let env = env(crate::timezone::Zone::UTC);
        assert_eq!(
            render(
                &env,
                "{{ class_names('a b', 3, none, {'c': true, 'd': false}, 'a') }}"
            )
            .unwrap(),
            "a b c"
        );
    }

    #[test]
    fn sparkline_reads_its_keywords() {
        let env = env(crate::timezone::Zone::UTC);
        let bars = render(
            &env,
            "{{ sparkline(list, kind='bars', width=2, height=2, label='Sales') }}",
        )
        .unwrap();
        assert_eq!(bars.matches("<rect").count(), 2, "{bars}");
        // Width and height are at least 8 pixels.
        assert!(
            bars.contains("width=\"8\"") && bars.contains("height=\"8\""),
            "{bars}"
        );
        assert!(bars.contains("aria-label=\"Sales\""), "{bars}");
        // Something that isn't a list draws an empty chart.
        assert!(
            render(&env, "{{ sparkline(5) }}")
                .unwrap()
                .ends_with("></svg>")
        );
        let err = render(&env, "{{ sparkline(list, colour='red') }}").unwrap_err();
        assert!(err.contains("colour"), "{err}");
    }
}
