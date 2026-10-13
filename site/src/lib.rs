//! Renox's documentation site, built with Renox: the guides, the tutorial,
//! the cheat sheet and the changelog of this repository, compiled into one
//! binary and shown with the UI kit.
//!
//! Run it from this directory: `cargo run` (or `rnx serve`), then open
//! <http://127.0.0.1:3000>. Deploying: see README.md.

use renox::prelude::*;
use serde::{Deserialize, Serialize};

pub mod content;
pub mod highlight;
pub mod icons;
pub mod render;

use content::{PAGES, Section};
use render::{RENDERED, REPOSITORY};

pub fn app() -> App {
    App::new()
        .embed(renox::embedded!())
        .templates(|env| {
            // `{{ icon("search") }}`: an inline SVG (icons.rs).
            env.add_function("icon", |name: String| {
                renox::minijinja::Value::from_safe_string(icons::svg(&name))
            });
        })
        .module(Docs)
}

struct Docs;

impl Module for Docs {
    fn name(&self) -> &'static str {
        "docs"
    }

    fn routes(&self) -> Routes {
        // The pages change only with a new binary: an ETag lets browsers and
        // proxies keep them, and a repeat visit gets a 304.
        let pages = Routes::new()
            .get("/", home)
            .name("home")
            .get("/docs/{slug}", show)
            .name("docs.show")
            .etag();
        Routes::new()
            .merge(pages)
            .redirect("/docs", "/docs/overview")
            .get("/search", search)
            .name("search")
            // Named `sitemap`: robots.txt points search engines at it.
            .get("/sitemap.xml", sitemap)
            .name("sitemap")
    }
}

/// The sidebar: each section with its pages.
#[derive(Serialize)]
struct NavSection {
    title: &'static str,
    icon: &'static str,
    pages: Vec<NavPage>,
}

#[derive(Serialize)]
struct NavPage {
    slug: &'static str,
    nav: &'static str,
    icon: &'static str,
    blurb: &'static str,
}

fn navigation() -> Vec<NavSection> {
    Section::ALL
        .iter()
        .map(|section| NavSection {
            title: section.title(),
            icon: section.icon(),
            pages: PAGES
                .iter()
                .filter(|page| page.section == *section)
                .map(|page| NavPage {
                    slug: page.slug,
                    nav: page.nav,
                    icon: page.icon,
                    blurb: page.blurb,
                })
                .collect(),
        })
        .collect()
}

/// The quick start on the home page, for the version this site documents
/// (the site shares the workspace's version).
fn quick_start() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "# 1. Install `rnx`, Renox's command-line tool\n\
         cargo install renox-cli --version {version}\n\
         \n\
         # 2. Make a new app called \"blog\" and go into its folder\n\
         rnx new blog && cd blog\n\
         \n\
         # 3. Run it, then open http://127.0.0.1:3000\n\
         rnx serve\n"
    )
}

async fn home() -> View {
    view(
        "home.html",
        context! {
            nav => navigation(),
            current => "",
            repository => REPOSITORY,
            quick_start => render::code_panel("bash", &quick_start()),
        },
    )
}

async fn show(Path(slug): Path<String>) -> Result<View> {
    let index = PAGES
        .iter()
        .position(|page| page.slug == slug)
        .ok_or(Error::NotFound)?;
    let page = &PAGES[index];
    let rendered = &RENDERED[page.slug];
    let previous = index.checked_sub(1).map(|i| &PAGES[i]);
    let next = PAGES.get(index + 1);
    let link = |page: Option<&content::Page>| {
        page.map(|page| context! { slug => page.slug, nav => page.nav, icon => page.icon })
    };
    Ok(view(
        "page.html",
        context! {
            nav => navigation(),
            current => page.slug,
            title => &rendered.title,
            page_icon => page.icon,
            blurb => page.blurb,
            section => page.section.title(),
            minutes => rendered.minutes,
            html => &rendered.html,
            toc => &rendered.toc,
            source => format!("{REPOSITORY}/blob/main/{}", page.path),
            edit => format!("{REPOSITORY}/edit/main/{}", page.path),
            previous => link(previous),
            next => link(next),
            repository => REPOSITORY,
        },
    ))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

/// A page that matched a search, with a line of its text around the match.
#[derive(Serialize)]
struct Hit {
    slug: &'static str,
    icon: &'static str,
    title: String,
    snippet: String,
}

async fn search(Query(query): Query<SearchQuery>) -> View {
    let q = query.q.trim().to_owned();
    let hits = if q.is_empty() { Vec::new() } else { find(&q) };
    view(
        "search.html",
        context! { nav => navigation(), current => "", q, hits, repository => REPOSITORY },
    )
}

/// Pages where every word of `q` appears, the ones with the words in their
/// title first, then by how often they appear.
fn find(q: &str) -> Vec<Hit> {
    let words: Vec<String> = q.split_whitespace().map(str::to_lowercase).collect();
    let mut hits: Vec<(usize, Hit)> = PAGES
        .iter()
        .filter_map(|page| {
            let rendered = &RENDERED[page.slug];
            let text = rendered.text.to_lowercase();
            let title = rendered.title.to_lowercase();
            if !words
                .iter()
                .all(|w| text.contains(w.as_str()) || title.contains(w.as_str()))
            {
                return None;
            }
            let in_title = words.iter().filter(|w| title.contains(w.as_str())).count();
            let count: usize = words.iter().map(|w| text.matches(w.as_str()).count()).sum();
            Some((
                in_title * 1000 + count,
                Hit {
                    slug: page.slug,
                    icon: page.icon,
                    title: rendered.title.clone(),
                    snippet: snippet(&rendered.text, &words[0]),
                },
            ))
        })
        .collect();
    hits.sort_by_key(|hit| std::cmp::Reverse(hit.0));
    hits.into_iter().map(|(_, hit)| hit).collect()
}

/// About thirty words of `text` around the first that contains `word`.
fn snippet(text: &str, word: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let hit = words
        .iter()
        .position(|w| w.to_lowercase().contains(word))
        .unwrap_or(0);
    let start = hit.saturating_sub(10);
    let end = (hit + 20).min(words.len());
    let mut snippet = words[start..end].join(" ");
    if start > 0 {
        snippet.insert_str(0, "… ");
    }
    if end < words.len() {
        snippet.push_str(" …");
    }
    snippet
}

async fn sitemap(State(state): State<AppState>) -> Result<renox::seo::Sitemap> {
    let mut map = renox::seo::Sitemap::new(&state).route("home", &[], None)?;
    for page in PAGES {
        map = map.route("docs.show", &[&page.slug], None)?;
    }
    Ok(map)
}
