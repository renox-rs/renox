//! The "About this page" mechanism (#231, #232): every page explains itself.
//!
//! The walker test fails when a GET route has no explanation, when an
//! explanation links to a guide section or a file that doesn't exist, or
//! when one is left over for a route that's gone. Then the panel, the
//! `/about/pages` index and the smoke tests of the skeleton's pages.

use bikeshop::explain::{self, Audience};
use renox::prelude::*;
use renox::testing::TestApp;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The repository's root (the explanations' paths start there).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A heading's anchor as GitHub and the docs site make it (site/src/render.rs
/// `slugify`): lowercase, letters, digits, `-` and `_` kept, spaces to `-`,
/// the rest dropped.
fn slugify(text: &str) -> String {
    let mut slug = String::new();
    for c in text.trim().to_lowercase().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            slug.push(c);
        } else if c.is_whitespace() {
            slug.push('-');
        }
    }
    slug
}

/// The heading text a reader sees: `code` and [links](…) reduced to their text.
fn heading_text(line: &str) -> String {
    let mut text = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' | '*' => {}
            '[' => {}
            ']' if chars.peek() == Some(&'(') => {
                // Skip the link's target.
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            _ => text.push(c),
        }
    }
    text
}

/// Every heading anchor of a Markdown file, numbered like the site's
/// (`name`, `name-1`, …), skipping fenced code (its `# ` lines are hidden
/// doctest lines, not headings).
fn anchors(markdown: &str) -> HashSet<String> {
    let mut found = HashSet::new();
    let mut fenced = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || !trimmed.starts_with('#') {
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        let rest = &trimmed[hashes..];
        if hashes > 6 || !rest.starts_with(' ') {
            continue;
        }
        let slug = slugify(&heading_text(rest));
        let mut candidate = slug.clone();
        let mut n = 0;
        while !found.insert(candidate.clone()) {
            n += 1;
            candidate = format!("{slug}-{n}");
        }
    }
    found
}

// [explain:about.pages.test]
#[renox::test]
async fn every_get_route_has_an_explanation() {
    let app = TestApp::new(bikeshop::app()).await;
    // [/explain:about.pages.test]
    let explanations = explain::all();
    let not_pages = explain::not_pages();
    let mut problems = Vec::new();

    // One explanation per route name.
    let mut by_route: HashMap<&str, usize> = HashMap::new();
    for e in &explanations {
        *by_route.entry(e.route).or_default() += 1;
    }
    for (route, count) in &by_route {
        if *count > 1 {
            problems.push(format!("`{route}` has {count} explanations"));
        }
    }
    let skipped: HashSet<&str> = not_pages.iter().map(|n| n.route).collect();

    // [explain:about.pages.test]
    // Every GET route of the app (Renox's own `/_renox/*`, `/health`… aside).
    let mut get_routes = HashSet::new();
    for route in app.kernel().routes() {
        if route.method != "GET" || route.module == "renox" {
            continue;
        }
        let Some(name) = route.name.as_deref() else {
            problems.push(format!(
                "GET {} has no name: give it one with `.name(…)`",
                route.path
            ));
            continue;
        };
        get_routes.insert(name.to_owned());
        if !by_route.contains_key(name) && !skipped.contains(name) {
            problems.push(format!(
                "GET {} (`{name}`, module {}) has no \"About this page\" entry: add one to \
                 src/app/<area>/explain.rs, or a NotAPage with the reason it isn't a page",
                route.path, route.module
            ));
        }
    }
    // [/explain:about.pages.test]

    // Each entry's path is its route's, and the panel finds the entry on the
    // page itself. Renox names the current route from the path alone (the
    // alphabetically first name on it, whatever the method), so check what
    // the panel gets: that name and an address made from the pattern.
    let routes = app.kernel().routes();
    for route in routes
        .iter()
        .filter(|r| r.method == "GET" && r.module != "renox")
    {
        let Some(name) = route.name.as_deref() else {
            continue;
        };
        let Some(entry) = explanations.iter().find(|e| e.route == name) else {
            continue;
        };
        if entry.path != route.path {
            problems.push(format!(
                "`{name}`: the explanation's path is `{}`, the route's `{}`",
                entry.path, route.path
            ));
            continue;
        }
        let reported = routes
            .iter()
            .filter(|r| r.path == route.path && r.domain == route.domain)
            .filter_map(|r| r.name.as_deref())
            .min();
        let address: String = route
            .path
            .split('/')
            .map(|s| if s.starts_with('{') { "1" } else { s })
            .collect::<Vec<_>>()
            .join("/");
        let shown = explain::resolve(reported, &address).map(|e| e.route);
        if shown != Some(name) {
            problems.push(format!(
                "GET {} would show the panel of {shown:?} instead of `{name}`",
                route.path
            ));
        }
    }

    // No entry for a route that doesn't exist (renamed or removed).
    for route in by_route.keys().chain(skipped.iter()) {
        if !get_routes.contains(*route) {
            problems.push(format!(
                "`{route}` is explained but isn't a GET route of the app"
            ));
        }
    }

    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

#[test]
fn every_explanation_is_complete_and_links_to_what_exists() {
    let root = repo_root();
    let mut docs: HashMap<String, HashSet<String>> = HashMap::new();
    let mut problems = Vec::new();

    for e in explain::all() {
        let route = e.route;
        for (field, text) in [
            ("title", e.title),
            ("purpose", e.purpose),
            ("who", e.who),
            ("under_hood", e.under_hood),
        ] {
            if text.trim().is_empty() {
                problems.push(format!("`{route}`: `{field}` is empty"));
            }
        }
        if e.audience.is_empty() {
            problems.push(format!("`{route}`: nobody uses it (`audience` is empty)"));
        }
        if e.features.is_empty() {
            problems.push(format!("`{route}`: no Renox feature listed"));
        }
        for f in e.features {
            if f.api.trim().is_empty() || f.why.trim().is_empty() {
                problems.push(format!("`{route}`: a feature without its API or its why"));
            }
        }
        if e.docs.is_empty() {
            problems.push(format!("`{route}`: no link to the guide"));
        }
        if e.sources.is_empty() {
            problems.push(format!("`{route}`: no source file"));
        }

        for link in e.docs {
            let Some((file, anchor)) = link.split_once('#') else {
                problems.push(format!("`{route}`: `{link}` has no #anchor"));
                continue;
            };
            if !file.starts_with("docs/") || !file.ends_with(".md") {
                problems.push(format!("`{route}`: `{link}` isn't a docs/*.md file"));
                continue;
            }
            let anchors = docs.entry(file.to_owned()).or_insert_with(|| {
                std::fs::read_to_string(root.join(file))
                    .map(|text| anchors(&text))
                    .unwrap_or_default()
            });
            if anchors.is_empty() {
                problems.push(format!("`{route}`: `{file}` doesn't exist"));
            } else if !anchors.contains(anchor) {
                problems.push(format!(
                    "`{route}`: `{file}` has no heading with the anchor `#{anchor}`"
                ));
            }
        }
        for source in e.sources {
            if !root.join(source).is_file() {
                problems.push(format!("`{route}`: the file `{source}` doesn't exist"));
            }
        }
    }

    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

#[test]
fn code_markers_are_closed_and_named_once() {
    assert!(
        bikeshop::code::PROBLEMS.is_empty(),
        "\n{}\n",
        bikeshop::code::PROBLEMS.join("\n")
    );
}

#[test]
fn every_code_sample_is_a_short_region_that_exists() {
    let mut problems = Vec::new();
    let mut shown = HashSet::new();
    for e in explain::all() {
        let route = e.route;
        if e.code.is_empty() {
            problems.push(format!(
                "`{route}`: no code sample: mark the handler (and the template's key part) \
                 and name them in `code`"
            ));
        } else if e.code.len() > 3 {
            problems.push(format!(
                "`{route}`: {} code samples, keep to three",
                e.code.len()
            ));
        }
        for code in e.code {
            shown.insert(code.region);
            let (tab, caption) = explain::Code::split_title(code.title);
            if tab.is_empty() || tab.chars().count() > 14 || caption.is_empty() {
                problems.push(format!(
                    "`{route}`: the title `{}` should read \"Tab: what it shows\", with a short tab name",
                    code.title
                ));
            }
            match bikeshop::code::region(code.region) {
                None => problems.push(format!(
                    "`{route}`: no file marks the region `{}` (`[explain:{}]` … `[/explain:{}]`)",
                    code.region, code.region, code.region
                )),
                Some(region) if region.lines() > 40 => problems.push(format!(
                    "`{route}`: the region `{}` has {} lines, keep a sample under 40",
                    code.region,
                    region.lines()
                )),
                Some(_) => {}
            }
        }
    }
    // A marked region nobody shows is left over from a sample that moved.
    for region in bikeshop::code::REGIONS {
        if !shown.contains(region.name) {
            problems.push(format!(
                "{}:{}: the region `{}` isn't shown on any page",
                region.path, region.first_line, region.name
            ));
        }
    }
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

#[test]
fn a_region_is_the_marked_lines_without_their_indentation() {
    let routes = bikeshop::code::region("home.routes").expect("the home page's routes are marked");
    assert_eq!(routes.path, "examples/bikeshop/src/app/home/mod.rs");
    assert_eq!(routes.language, "rust");
    assert!(
        routes.text.starts_with("fn routes(&self) -> Routes {"),
        "{}",
        routes.text
    );
    // Two parts, joined by a `…` line.
    assert!(
        routes.text.contains("\n// …\nasync fn index() -> View {"),
        "{}",
        routes.text
    );
    assert!(!routes.text.contains("[explain:"));
    // The lines on GitHub are the ones in the file.
    let file = std::fs::read_to_string(repo_root().join(routes.path)).unwrap();
    let lines: Vec<&str> = file.lines().collect();
    assert!(
        lines[routes.first_line - 1]
            .trim_start()
            .starts_with("fn routes")
    );
    assert_eq!(lines[routes.last_line - 1], "}");
    let template = bikeshop::code::region("home.template").unwrap();
    assert_eq!(template.language, "html");
    assert!(template.text.starts_with("<app-hero "), "{}", template.text);
}

#[test]
fn addresses_match_the_most_specific_pattern() {
    use bikeshop::explain::path_matches;
    assert_eq!(path_matches("/", "/"), Some(1));
    assert_eq!(path_matches("/", "/staff"), None);
    assert_eq!(path_matches("/account", "/account"), Some(1));
    assert_eq!(
        path_matches("/reset-password/{token}", "/reset-password/abc"),
        Some(1)
    );
    assert_eq!(
        path_matches("/reset-password/{token}", "/reset-password"),
        None
    );
    assert_eq!(path_matches("/products/new", "/products/new"), Some(2));
    assert_eq!(path_matches("/products/{id}", "/products/new"), Some(1));
    assert_eq!(path_matches("/files/{*rest}", "/files/a/b"), Some(1));
    assert_eq!(path_matches("/staff", "/account"), None);
    // The bug the fallback works around: GET /account is named after the DELETE.
    assert_eq!(
        explain::resolve(Some("account.destroy"), "/account").map(|e| e.route),
        Some("account.show")
    );
}

#[test]
fn the_anchor_rules_match_the_docs_site() {
    let found = anchors(
        "# Title\n## The `Auth` module's routes\n```rust\n# use x;\n```\n## A [link](x.md) here\n## Twice\n## Twice\n",
    );
    for anchor in [
        "title",
        "the-auth-modules-routes",
        "a-link-here",
        "twice",
        "twice-1",
    ] {
        assert!(found.contains(anchor), "{anchor} in {found:?}");
    }
    assert!(!found.contains("use-x"));
}

/// Someone on a store's staff (the staff side needs `staff.access` in a store).
async fn staff_member(app: &TestApp) -> User {
    use bikeshop::seed::fixtures;
    fixtures::roles(app.db()).await.unwrap();
    let store = fixtures::store(app.db(), "North").await.unwrap();
    let staff = bikeshop::app::access::catalogue::STAFF;
    fixtures::person(app.db(), "sam@example.com", &[(staff, Some(store.id))])
        .await
        .unwrap()
}

#[renox::test]
async fn the_skeleton_pages_answer() {
    let app = TestApp::new(bikeshop::app()).await;
    app.get("/")
        .await
        .assert_ok()
        .assert_view("home/index.html")
        .assert_see("rx-navbar");
    app.get("/about/pages")
        .await
        .assert_ok()
        .assert_view("about/pages.html");
    app.get("/login").await.assert_ok();

    // The staff side needs a login.
    app.get("/staff").await.assert_redirect("/login");
    let user = staff_member(&app).await;
    app.acting_as(&user);
    app.get("/staff")
        .await
        .assert_ok()
        .assert_view("staff/dashboard.html")
        .assert_see("rx-shell")
        .assert_see("rx-sidebar");
}

#[renox::test]
async fn every_page_shows_its_panel_with_motion() {
    let app = TestApp::new(bikeshop::app()).await;
    let user = staff_member(&app).await;
    for (uri, title, logged_in) in [
        ("/", "Home", false),
        ("/about/pages", "Every page and its features", false),
        ("/login", "Log in", false),
        ("/staff", "Staff dashboard", true),
        ("/account", "Your account", true),
    ] {
        if logged_in {
            app.acting_as(&user);
        } else {
            app.logout();
        }
        let page = app.get(uri).await;
        page.assert_ok()
            .assert_see("data-rx-open=\"about-page\"")
            .assert_see("id=\"about-page\"")
            .assert_see("id=\"explain-dock\"")
            .assert_see("bs-docked")
            .assert_see(title)
            .assert_see("vendor/motion/motion.js")
            .assert_see("app.js");
    }
}

#[renox::test]
async fn the_panel_shows_features_docs_and_sources() {
    let app = TestApp::new(bikeshop::app()).await;
    app.get("/")
        .await
        .assert_see("About this page")
        .assert_see("Renox features used, and why")
        .assert_see("App::detect_locale")
        // Guides on the docs site, sources on GitHub.
        .assert_see("https://docs.renox.rs/docs/routing#apps-modules-and-routes")
        .assert_see(
            "https://github.com/renox-rs/renox/blob/main/examples/bikeshop/src/app/home/mod.rs",
        );
}

#[renox::test]
async fn the_panel_shows_the_code_coloured_with_its_file_on_github() {
    let app = TestApp::new(bikeshop::app()).await;
    let page = app.get("/").await;
    page.assert_ok()
        .assert_see("The code")
        // The kit's tabs, one per sample, in the docked panel and the sheet.
        .assert_see(r#"id="explain-dock-code-tab-0""#)
        .assert_see(r#"id="about-page-code-tab-0""#)
        .assert_see(">Routes</button>")
        // Coloured on the server, with the lines' link and a copy button.
        .assert_see(r#"<span class="hl-kw">fn</span> <span class="hl-fn">routes</span>"#)
        .assert_see(
            "https://github.com/renox-rs/renox/blob/main/examples/bikeshop/src/app/home/mod.rs#L",
        )
        .assert_see("data-rx-copy-text=\"fn routes(&amp;self) -&gt; Routes {");
    // Not folded unless the cookie says so.
    assert!(!page.text().contains("bs-docked--rail"));
}

#[renox::test]
async fn the_folded_panel_is_drawn_folded_from_its_cookie() {
    let app = TestApp::new(bikeshop::app()).await;
    app.request()
        .header("Cookie", "bikeshop_explain=rail")
        .get("/")
        .await
        .assert_ok()
        .assert_see("bs-docked bs-docked--rail")
        .assert_see(r#"<div class="bs-dock__panel" id="explain-dock-panel" hidden>"#);
    app.request()
        .header("Cookie", "bikeshop_explain=open")
        .get("/")
        .await
        .assert_see(r#"<div class="bs-dock__panel" id="explain-dock-panel">"#)
        .assert_dont_see("bs-docked--rail");
}

#[renox::test]
async fn bikeshop_explain_false_hides_the_panels() {
    let app = TestApp::with_config(bikeshop::app(), |c| {
        c.vars.insert("BIKESHOP_EXPLAIN".into(), "false".into());
    })
    .await;
    app.get("/")
        .await
        .assert_ok()
        .assert_dont_see("id=\"about-page\"")
        .assert_dont_see("id=\"explain-dock\"")
        .assert_dont_see("bs-docked")
        .assert_dont_see("data-rx-open=\"about-page\"");
    // The index of every page stays.
    app.get("/about/pages").await.assert_ok().assert_see("Home");
}

#[renox::test]
async fn the_panel_speaks_spanish_and_falls_back_to_english() {
    let app = TestApp::new(bikeshop::app()).await;
    app.request()
        .header("Accept-Language", "es")
        .get("/")
        .await
        .assert_ok()
        .assert_see("Sobre esta página")
        // Translated in es.json, the API names and paths left as they are.
        .assert_see("La puerta de entrada de la tienda")
        .assert_see("Una ruta <code>GET /</code> llamada <code>home</code>")
        .assert_dont_see("route named <code>home</code>");

    // A text a language lacks falls back to English, field by field.
    let home = explain::for_route("home").expect("the home page is explained");
    let page =
        home.localize(&|key: &str| (key == "about_page.home.title").then(|| "Inicio".to_owned()));
    assert_eq!(page.title, "Inicio");
    assert_eq!(page.purpose, home.purpose);
    assert_eq!(page.features[0].why, home.features[0].why);
}

#[renox::test]
async fn the_index_filters_by_feature_and_by_role() {
    let app = TestApp::new(bikeshop::app()).await;
    let all = explain::all();

    let page = app.get("/about/pages").await;
    page.assert_ok();
    for e in &all {
        page.assert_see(e.title);
    }

    // By feature: only the pages that use it.
    let uri = format!("/about/pages?feature={}", "App%3A%3Adetect_locale");
    app.get(&uri)
        .await
        .assert_ok()
        .assert_see(r#"rx-link" href="/">Home</a>"#)
        .assert_dont_see(r#"rx-link" href="/staff">Staff dashboard</a>"#);

    // By role.
    app.get("/about/pages?audience=mechanic")
        .await
        .assert_ok()
        .assert_see(r#"rx-link" href="/staff">Staff dashboard</a>"#)
        .assert_dont_see(r#"rx-link" href="/register">Register</a>"#);
    assert!(all.iter().any(|e| e.audience.contains(&Audience::Mechanic)));

    // An unknown filter is ignored rather than failing.
    app.get("/about/pages?feature=nope&audience=nobody")
        .await
        .assert_ok()
        .assert_see(r#"rx-link" href="/">Home</a>"#);
}

#[renox::test]
async fn the_language_menu_remembers_the_choice() {
    let app = TestApp::new(bikeshop::app()).await;
    app.post("/locale/es", &[]).await.assert_status(303);
    app.get("/").await.assert_see("Inicio");
    // A language the shop doesn't have is ignored.
    app.post("/locale/xx", &[]).await.assert_status(303);
    app.get("/").await.assert_see("Inicio");
}

#[renox::test]
async fn every_view_compiles() {
    let app = TestApp::new(bikeshop::app()).await;
    app.assert_views_compile();
}
