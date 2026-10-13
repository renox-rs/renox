//! The bike shop speaks English and Spanish (#243).
//!
//! - Every text of `resources/lang/en.json` has its Spanish in `es.json`,
//!   with the same `:placeholders` and as many plural parts, and only a short
//!   list of texts (brand names, symbols) is the same in both.
//! - Every "About this page" entry has its Spanish under
//!   `about_page.<route>` (`src/explain.rs`), every field and every feature.
//! - The pages, walked in Spanish as a visitor, a customer and the owner, show
//!   none of the English texts the app and Renox would show untranslated.

use bikeshop::explain;
use renox::prelude::*;
use renox::serde_json::{self, Value};
use renox::testing::TestApp;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

/// Texts that are the same in English and Spanish, on purpose.
const SAME_IN_BOTH: &[&str] = &[
    "shop.name",                          // the brand: "Bike Shop"
    "staff.nav.roles",                    // "Roles"
    "staff.team.roles",                   // "Roles"
    "staff.stores.minutes_unit",          // "min"
    "blocks.datetime.minutes",            // ":n min"
    "blocks.page.signature",              // "Macro"
    "data_page.pagila.pagila",            // "Pagila", a name
    "sales.checkout.total",               // "Total"
    "sales.orders.subtotal",              // "Subtotal"
    "sales.orders.total",                 // "Total"
    "rentals.receipt.total",              // "Total"
    "workshop.fields.total",              // "Total"
    "workshop.book.total",                // "Total (:minutes min)"
    "stock.fields.sku",                   // "SKU"
    "stock.consignments.from_to",         // ":owner → :location"
    "stock.purchasing.total",             // "Total"
    "multistore.help.from_to",            // ":from → :to"
    "multistore.statement.total",         // "Total"
    "multistore.fees.changed",            // ":store: :from % → :to %"
    "plans.fields.plan",                  // "Plan"
    "reports.fields.total",               // "Total"
    "reports.workbook.total",             // "Total"
    "reports.hours",                      // ":hours h"
    "reports.payable.plan_subscriptions", // "Plan"
    "fields.no",                          // "No"
];

/// English a Spanish page may still show, and why.
const STILL_ENGLISH: &[&str] = &[
    // renox-admin's grid columns and bulk actions are set in Rust with no
    // language (the panel's own texts and its form labels are translated).
    "Move to a category",
    "Lead time (days)",
    // The seeded catalogue is data, written once: a plan's description
    // ("Every three months the full service…") contains a frequency label.
    "Every three months",
    // /about/blocks shows the blocks with made-up English demo data.
    "Ready for pickup",
    "Waiting for parts",
    "Discount on parts",
];

fn lang(locale: &str) -> BTreeMap<String, String> {
    let path = format!(
        "{}/resources/lang/{locale}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let json: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut flat = BTreeMap::new();
    flatten("", &json, &mut flat);
    flat
}

fn flatten(prefix: &str, value: &Value, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&key, value, out);
            }
        }
        Value::String(text) => {
            out.insert(prefix.to_owned(), text.clone());
        }
        other => {
            out.insert(prefix.to_owned(), other.to_string());
        }
    }
}

/// The `:placeholders` of a text.
fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (i, c) in text.char_indices() {
        if c != ':' {
            continue;
        }
        let name: String = text[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            found.insert(name.to_ascii_lowercase());
        }
    }
    found
}

#[test]
fn every_english_text_has_its_spanish() {
    let en = lang("en");
    let es = lang("es");
    let mut problems = Vec::new();
    for (key, english) in &en {
        let Some(spanish) = es.get(key) else {
            problems.push(format!("`{key}` has no Spanish (en: {english:?})"));
            continue;
        };
        if spanish.trim().is_empty() {
            problems.push(format!("`{key}` is empty in Spanish"));
        }
        if spanish == english && !SAME_IN_BOTH.contains(&key.as_str()) {
            problems.push(format!(
                "`{key}` is the same in Spanish ({english:?}); translate it, or add it to SAME_IN_BOTH"
            ));
        }
        if placeholders(english) != placeholders(spanish) {
            problems.push(format!(
                "`{key}`: placeholders {:?} in English, {:?} in Spanish",
                placeholders(english),
                placeholders(spanish)
            ));
        }
        if english.matches('|').count() != spanish.matches('|').count() {
            problems.push(format!("`{key}`: the plural parts differ ({spanish:?})"));
        }
    }
    for key in SAME_IN_BOTH {
        assert!(
            en.contains_key(*key),
            "SAME_IN_BOTH names `{key}`, which en.json lacks"
        );
    }
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

#[test]
fn every_explanation_has_its_spanish() {
    let es = lang("es");
    let mut problems = Vec::new();
    for e in explain::all() {
        let mut fields: Vec<(String, &str)> = vec![
            ("title".into(), e.title),
            ("purpose".into(), e.purpose),
            ("who".into(), e.who),
            ("under_hood".into(), e.under_hood),
        ];
        for (n, f) in e.features.iter().enumerate() {
            fields.push((format!("features.{n}"), f.why));
        }
        for (field, english) in fields {
            let key = format!("about_page.{}.{field}", e.route);
            match es.get(&key) {
                None => problems.push(format!("`{key}` has no Spanish")),
                Some(spanish) if spanish == english => {
                    problems.push(format!("`{key}` is still English"))
                }
                Some(_) => {}
            }
        }
        // No Spanish for a feature that isn't there (a feature removed in English).
        let extra = format!("about_page.{}.features.{}", e.route, e.features.len());
        if es.contains_key(&extra) {
            problems.push(format!(
                "`{extra}` translates a feature the page doesn't have"
            ));
        }
    }
    // Nor for a page that doesn't exist.
    let routes: HashSet<&str> = explain::all().iter().map(|e| e.route).collect();
    for key in es.keys().filter_map(|k| k.strip_prefix("about_page.")) {
        let route = ["title", "purpose", "who", "under_hood", "features"]
            .iter()
            .find_map(|field| key.split_once(&format!(".{field}")).map(|(route, _)| route));
        if let Some(route) = route
            && !routes.contains(route)
        {
            problems.push(format!("`about_page.{key}`: no page is named `{route}`"));
        }
    }
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

/// English texts a Spanish page must not show: Renox's and its plugins'
/// own (the kit, the grid, the auth and account pages, renox-2fa,
/// renox-oauth, renox-admin), and every longer text of the app's en.json.
fn english_texts() -> Vec<String> {
    let mut texts: Vec<String> = [
        "Remember me",
        "Forgot your password?",
        "Create an account",
        "Already registered?",
        "Please check the highlighted fields.",
        "Skip to content",
        "Clear filters",
        "Previous page",
        "Next page",
        "Show on this screen",
        "Reset columns",
        "Nothing matches these filters.",
        "Print or save as PDF",
        "Mark all as read",
        "No notifications",
        "Your account",
        "Change password",
        "Delete account",
        "Log out other devices",
        "Two-factor authentication",
        "Linked accounts",
        "Or continue with",
        "Choose a date",
        "vs previous period",
        "Custom range",
        "Show the data",
        "Renox features used, and why",
        "Under the hood",
        "Source files",
        "Admin navigation",
        "Bike Shop admin",
        // The catalogue's and the data page's texts, written in Rust.
        "Read the audit log.",
        "Runs all three stores",
        "Places and customers",
    ]
    .map(str::to_owned)
    .to_vec();
    let es = lang("es");
    // A feature's API name (`renox::grid`, "Bike shop blocks") is a name,
    // the same in every language.
    let names: HashSet<&str> = explain::all()
        .iter()
        .flat_map(|e| e.features.iter().map(|f| f.api))
        .collect();
    for (key, english) in lang("en") {
        // Long enough to be a sentence of the app's (not "Total" or "SKU"),
        // and without placeholders (filled in, they read differently).
        if english.len() >= 16
            && english.contains(' ')
            && !english.contains(':')
            && !english.contains('|')
            && !english.contains('`')
            && !english.contains('*')
            && es.get(&key) != Some(&english)
            && !names.contains(english.as_str())
            && !STILL_ENGLISH.iter().any(|s| english.contains(s))
        {
            texts.push(english);
        }
    }
    texts.retain(|t| !STILL_ENGLISH.iter().any(|s| t.contains(s)));
    texts
}

/// What a reader sees of a page: its text without tags, scripts, styles and
/// `<code>` (API names stay English in every language), entities decoded.
fn visible(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        out.push(' ');
        rest = &rest[start..];
        let lower = rest.get(..10).unwrap_or(rest).to_ascii_lowercase();
        let skip_to = ["<script", "<style", "<code", "<pre"]
            .iter()
            .find(|tag| lower.starts_with(**tag))
            .map(|tag| format!("</{}", &tag[1..]));
        if let Some(close) = skip_to {
            match rest.to_ascii_lowercase().find(&close) {
                Some(end) => rest = &rest[end..],
                None => break,
            }
        }
        match rest.find('>') {
            Some(end) => rest = &rest[end + 1..],
            None => break,
        }
    }
    out.push_str(rest);
    out.replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// The app's own links on a page (`href="/…"`), without query or fragment.
fn links(html: &str) -> Vec<String> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter(|href| href.starts_with('/') && !href.starts_with("//"))
        .map(|href| {
            href.split(['?', '#'])
                .next()
                .unwrap_or(href)
                .replace("&amp;", "&")
        })
        .collect()
}

/// Links not worth following: files, downloads, logging out, JSON, Renox's
/// own tools.
fn skipped(path: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "/_renox",
        "/storage",
        "/api/",
        "/auth/",
        "/logout",
        "/webhooks",
        "/billing/webhooks",
        "/notifications/stream",
        "/verify-email/",
        "/reset-password/",
    ];
    const SUFFIXES: &[&str] = &[
        ".css", ".js", ".png", ".svg", ".jpg", ".webp", ".ico", ".csv", ".xlsx", ".pdf", ".txt",
        ".xml", ".json", "/export", "/print", "/feed",
    ];
    PREFIXES.iter().any(|p| path.starts_with(p))
        || SUFFIXES.iter().any(|s| path.ends_with(s))
        || path.contains("/export")
        || path.starts_with("/reports")
        || path.starts_with("/staff/reports")
}

/// A page's kind, for visiting only a few of each: `/products/{x}`.
fn kind(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut shape: Vec<String> = parts
        .iter()
        .map(|p| {
            if p.chars().any(|c| c.is_ascii_digit()) {
                "{n}".to_owned()
            } else {
                (*p).to_owned()
            }
        })
        .collect();
    // A slug after a fixed segment (`/products/road-bike-x`) is one kind.
    if shape.len() >= 2 && matches!(shape[0].as_str(), "products" | "shop" | "plans") {
        shape.truncate(1);
        shape.push("{slug}".into());
    }
    format!("/{}/{}", shape.join("/"), parts.len())
}

/// Walks the app from `starts` in Spanish and returns the English texts
/// found, page by page.
async fn walk(app: &TestApp, starts: &[&str], english: &[String]) -> (usize, Vec<String>) {
    let mut queue: VecDeque<String> = starts.iter().map(|s| (*s).to_owned()).collect();
    let mut seen: HashSet<String> = queue.iter().cloned().collect();
    let mut per_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut found = Vec::new();
    let mut visited = 0;
    while let Some(path) = queue.pop_front() {
        if visited >= 260 {
            break;
        }
        let count = per_kind.entry(kind(&path)).or_default();
        if *count >= 2 {
            continue;
        }
        *count += 1;
        let response = app.get(&path).await;
        if response.status != 200 {
            continue;
        }
        let is_html = response
            .header("content-type")
            .is_some_and(|t| t.starts_with("text/html"));
        if !is_html {
            continue;
        }
        visited += 1;
        let html = response.text();
        let text = visible(&html);
        if let Ok(dir) = std::env::var("BIKESHOP_I18N_DUMP") {
            let name = path.trim_matches('/').replace('/', "_");
            let _ = std::fs::write(format!("{dir}/{name}.txt"), &text);
        }
        for phrase in english {
            if text.contains(phrase.as_str()) {
                found.push(format!("{path}: {phrase:?}"));
            }
        }
        for link in links(&html) {
            if !skipped(&link) && seen.insert(link.clone()) {
                queue.push_back(link);
            }
        }
    }
    (visited, found)
}

async fn seeded() -> TestApp {
    let app = TestApp::with_config(bikeshop::app(), |config| {
        // Staff log in with a password only here (no authenticator app).
        config
            .vars
            .insert("BIKESHOP_STAFF_2FA".into(), "optional".into());
    })
    .await;
    bikeshop::seed::run(app.state().clone()).await.unwrap();
    app
}

/// Switches the session (and the account, when logged in) to Spanish with
/// the navbar's language menu.
async fn spanish(app: &TestApp) {
    let to = app.state().url("locale.update", &[&"es"]).unwrap();
    let answer = app.post(&to, &[]).await;
    assert!(
        answer.status.as_u16() < 400,
        "the language menu answered {}",
        answer.status
    );
}

async fn user(app: &TestApp, email: &str) -> User {
    User::find_by_email(app.db(), email)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{email} is seeded"))
}

#[renox::test]
async fn the_pages_in_spanish_show_no_english() {
    let app = seeded().await;
    let english = english_texts();
    let mut found = Vec::new();
    let mut visited = 0;

    // A first visit from a Spanish browser (App::detect_locale).
    let home = app
        .request()
        .header("Accept-Language", "es-ES,es;q=0.9")
        .get("/")
        .await;
    home.assert_ok().assert_see("lang=\"es\"");

    // A visitor, who chose Spanish in the menu.
    spanish(&app).await;
    let (n, f) = walk(
        &app,
        &[
            "/",
            "/login",
            "/register",
            "/forgot-password",
            "/about/pages",
            "/about/data",
        ],
        &english,
    )
    .await;
    visited += n;
    found.extend(f);

    // A customer.
    app.acting_as(&user(&app, "customer@bikeshop.test").await);
    spanish(&app).await;
    let (n, f) = walk(&app, &["/account", "/", "/notifications"], &english).await;
    visited += n;
    found.extend(f);

    // The owner, who sees every staff page and the admin panel.
    app.acting_as(&user(&app, "owner@bikeshop.test").await);
    spanish(&app).await;
    let (n, f) = walk(&app, &["/staff", "/admin", "/account"], &english).await;
    visited += n;
    found.extend(f);

    assert!(visited >= 60, "only {visited} pages were walked");
    found.sort();
    found.dedup();
    assert!(
        found.is_empty(),
        "{} English texts on Spanish pages ({visited} pages):\n{}\n",
        found.len(),
        found.join("\n")
    );
}

#[renox::test]
async fn spanish_formats_money_and_counts() {
    let app = seeded().await;
    spanish(&app).await;
    let page = app.get("/shop").await;
    page.assert_ok();
    let html = page.text();
    // Money with the Spanish separators ($1.249,99), never English ones
    // ($1,249.99): every price ends in a decimal comma.
    let text = visible(&html);
    let prices: Vec<&str> = text
        .split('$')
        .skip(1)
        .filter_map(|rest| {
            let figures = rest
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
                .next()?
                .trim_end_matches(['.', ',']);
            (!figures.is_empty()).then_some(figures)
        })
        .collect();
    assert!(!prices.is_empty(), "prices are shown");
    for price in &prices {
        assert!(
            price.len() > 3 && price.as_bytes()[price.len() - 3] == b',',
            "${price}: no English separators in Spanish"
        );
    }
    // The plural range of the result count, in Spanish.
    assert!(text.contains(" productos"), "the count is Spanish");
}
