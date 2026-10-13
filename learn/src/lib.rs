//! learn.renox.rs: a course that builds one Renox app (a product catalog
//! with stock) stage by stage, with screenshots, videos and challenges
//! solved in an editor. Built with Renox and compiled into one binary
//! (views and public files embedded).
//!
//! The plan is epic #538. This crate is its skeleton (#523): the layout, the
//! course's outline on the home page, the sitemap and `llms.txt`.
//!
//! Run it from this directory: `cargo run` (or `rnx serve`), then open
//! <http://127.0.0.1:3000>. Deploying: README.md.

use renox::axum::http::header::CONTENT_TYPE;
use renox::axum::response::{IntoResponse, Response};
use renox::prelude::*;
use serde_json::json;

pub mod course;

/// The site's own address (canonical links, JSON-LD) when `APP_URL` isn't
/// set to it.
pub const SITE: &str = "https://learn.renox.rs";
/// The documentation site.
pub const DOCS: &str = "https://docs.renox.rs";
/// The landing page.
pub const WWW: &str = "https://renox.rs";
/// The source.
pub const REPOSITORY: &str = "https://github.com/renox-rs/renox";

pub fn app() -> App {
    App::new()
        .embed(renox::embedded!())
        // What every view gets: the links that appear in the layout.
        .share("links", |_ctx: renox::view::ViewContext| async move {
            Ok(json!({ "docs": DOCS, "www": WWW, "github": REPOSITORY }))
        })
        .module(Learn)
}

struct Learn;

impl Module for Learn {
    fn name(&self) -> &'static str {
        "learn"
    }

    fn routes(&self) -> Routes {
        // The pages change only with a new binary: ETags let browsers and
        // proxies keep them.
        let pages = Routes::new().get("/", home).name("home").etag();
        Routes::new()
            .merge(pages)
            .get("/sitemap.xml", sitemap)
            .name("sitemap")
            .get("/llms.txt", llms)
            .name("llms")
    }
}

/// This site's base URL: `APP_URL` without a trailing slash.
fn base(state: &AppState) -> String {
    let url = state.config.url.trim_end_matches('/');
    if url.is_empty() {
        SITE.to_owned()
    } else {
        url.to_owned()
    }
}

/// A JSON-LD block, safe inside `<script>` (no `</`).
fn json_ld(value: serde_json::Value) -> String {
    value.to_string().replace("</", "<\\/")
}

async fn home(State(state): State<AppState>) -> Result<View> {
    let base = base(&state);
    let ld = json!({
        "@context": "https://schema.org",
        "@type": "Course",
        "name": "Learn Renox",
        "description": "Build one web app in Rust with Renox, stage by stage: migrations, models, pages, components, forms, live components and tests.",
        "url": format!("{base}/"),
        "inLanguage": "en",
        "provider": { "@type": "Organization", "name": "Renox", "url": WWW },
        "syllabusSections": course::STAGES.iter().map(|s| json!({
            "@type": "Syllabus",
            "name": format!("Stage {}: {}", s.number, s.title),
            "description": s.summary,
        })).collect::<Vec<_>>(),
    });
    Ok(view(
        "home.html",
        context! { stages => course::STAGES, json_ld => json_ld(ld) },
    ))
}

/// Every page for search engines.
async fn sitemap(State(state): State<AppState>) -> Result<renox::seo::Sitemap> {
    renox::seo::Sitemap::new(&state).route("home", &[], None)
}

/// `llms.txt` (llmstxt.org): what the course is and what each stage teaches.
async fn llms(State(state): State<AppState>) -> Response {
    let base = base(&state);
    let mut text = String::from(
        "# Learn Renox\n\n> A course that builds one web app in Rust with Renox (a product catalog with stock), stage by stage, from `rnx new` to tested live components.\n\n",
    );
    text.push_str(&format!(
        "- Course: {base}/\n- Docs: {DOCS}\n- Source: {REPOSITORY}\n\n## Stages\n\n"
    ));
    for s in course::STAGES {
        text.push_str(&format!(
            "- Stage {}: {}: {} Guide: {DOCS}/docs/{}\n",
            s.number, s.title, s.summary, s.guide
        ));
    }
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}
