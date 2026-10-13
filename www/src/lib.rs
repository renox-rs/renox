//! renox.rs: Renox's landing page and blog, built with Renox and compiled
//! into one binary (views, posts and public files embedded).
//!
//! Nothing on it needs JavaScript to be read: the landing page's text, code
//! and numbers are rendered here, and `public/www.js` only animates them.
//! For search engines and language models it serves `sitemap.xml`,
//! `robots.txt`, an Atom feed, JSON-LD on every page, `llms.txt`,
//! `llms-full.txt` and each post as Markdown at `/blog/{slug}.md`.
//!
//! Run it from this directory: `cargo run` (or `rnx serve`), then open
//! <http://127.0.0.1:3000>. Deploying: README.md.

use renox::axum::http::header::CONTENT_TYPE;
use renox::axum::response::{IntoResponse, Response};
use renox::prelude::*;
use serde_json::json;

pub mod blog;
pub mod landing;

/// The site's own address (canonical links, the feed, JSON-LD) when
/// `APP_URL` isn't set to it.
pub const SITE: &str = "https://renox.rs";

pub fn app() -> App {
    App::new()
        .embed(renox::embedded!())
        // What every view gets: the links that appear in the layout.
        .share("links", |_ctx: renox::view::ViewContext| async move {
            Ok(json!({ "docs": landing::DOCS, "github": landing::REPOSITORY, "demo": landing::DEMO }))
        })
        // Google Analytics 4, in production only: the measurement id the
        // layout's gtag snippet uses (with the request's CSP nonce).
        .share("ga4", |ctx: renox::view::ViewContext| async move {
            let config = &ctx.state.config;
            let id = (config.env == renox::Environment::Production).then(|| {
                config
                    .analytics
                    .ga4_measurement_id
                    .clone()
                    .unwrap_or_else(|| landing::GA4.to_owned())
            });
            Ok(json!(id))
        })
        // What gtag.js loads and talks to.
        .csp(|csp| {
            csp.allow("script-src", "https://www.googletagmanager.com")
                .allow("connect-src", "https://*.google-analytics.com")
                .allow("connect-src", "https://*.analytics.google.com")
                .allow("connect-src", "https://*.googletagmanager.com");
        })
        .module(Www)
}

struct Www;

impl Module for Www {
    fn name(&self) -> &'static str {
        "www"
    }

    fn routes(&self) -> Routes {
        // The pages change only with a new binary: ETags let browsers and
        // proxies keep them.
        let pages = Routes::new()
            .get("/", home)
            .name("home")
            .get("/blog", blog_index)
            .name("blog.index")
            .get("/blog/tags/{tag}", blog_tag)
            .name("blog.tag")
            .get("/blog/{slug}", blog_show)
            .name("blog.show")
            .etag();
        Routes::new()
            .merge(pages)
            .get("/blog/feed.xml", feed)
            .name("blog.feed")
            .get("/sitemap.xml", sitemap)
            .name("sitemap")
            .get("/llms.txt", llms)
            .name("llms")
            .get("/llms-full.txt", llms_full)
            .name("llms.full")
            .redirect("/docs", landing::DOCS)
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
    let posts: Vec<_> = blog::published().into_iter().take(3).collect();
    let ld = json!([
        {
            "@context": "https://schema.org",
            "@type": "WebSite",
            "name": "Renox",
            "url": format!("{base}/"),
            "description": "A batteries-included web framework for Rust, modelled on Laravel.",
        },
        {
            "@context": "https://schema.org",
            "@type": "SoftwareSourceCode",
            "name": "Renox",
            "description": "A batteries-included web framework for Rust: routing, models, migrations, validation, auth, queues, mail, a UI kit and more, behind one dependency, deployed as one binary.",
            "codeRepository": landing::REPOSITORY,
            "programmingLanguage": "Rust",
            "license": "https://opensource.org/licenses/MIT",
            "url": format!("{base}/"),
            "documentation": landing::DOCS,
            "keywords": "rust, web framework, laravel, axum, htmx, sqlite, postgresql",
        },
    ]);
    Ok(view(
        "home.html",
        context! {
            metrics => landing::metrics(),
            crates => landing::CRATES,
            features => landing::features(),
            pipeline => landing::pipeline(),
            pairs => landing::pairs(),
            comparison => landing::comparison(),
            examples => landing::examples(),
            bench => landing::benchmarks(),
            posts,
            json_ld => json_ld(ld),
        },
    ))
}

async fn blog_index(State(state): State<AppState>) -> Result<View> {
    let base = base(&state);
    let posts = blog::published();
    let ld = json!({
        "@context": "https://schema.org",
        "@type": "Blog",
        "name": "The Renox blog",
        "url": format!("{base}/blog"),
        "blogPost": posts.iter().map(|p| json!({
            "@type": "BlogPosting",
            "headline": p.title,
            "url": format!("{base}/blog/{}", p.slug),
            "datePublished": p.date,
        })).collect::<Vec<_>>(),
    });
    Ok(view(
        "blog/index.html",
        context! { posts, tags => blog::tags(), tag => None::<String>, json_ld => json_ld(ld) },
    ))
}

async fn blog_tag(State(state): State<AppState>, Path(tag): Path<String>) -> Result<View> {
    let tag = tag.to_lowercase();
    let posts: Vec<_> = blog::published()
        .into_iter()
        .filter(|p| p.tags.contains(&tag))
        .collect();
    if posts.is_empty() {
        return Err(Error::NotFound);
    }
    let base = base(&state);
    let ld = json!({
        "@context": "https://schema.org",
        "@type": "BreadcrumbList",
        "itemListElement": [
            { "@type": "ListItem", "position": 1, "name": "Blog", "item": format!("{base}/blog") },
            { "@type": "ListItem", "position": 2, "name": tag, "item": format!("{base}/blog/tags/{tag}") },
        ],
    });
    Ok(view(
        "blog/index.html",
        context! { posts, tags => blog::tags(), tag, json_ld => json_ld(ld) },
    ))
}

/// `/blog/{slug}` (the page) or `/blog/{slug}.md` (its Markdown, for
/// language models and anyone who wants the source).
async fn blog_show(State(state): State<AppState>, Path(slug): Path<String>) -> Result<Response> {
    if let Some(slug) = slug.strip_suffix(".md") {
        let post = blog::find(slug).ok_or(Error::NotFound)?;
        let text = format!(
            "# {}\n\n> {}\n\n{}\n",
            post.title, post.description, post.markdown
        );
        return Ok(([(CONTENT_TYPE, "text/markdown; charset=utf-8")], text).into_response());
    }
    let post = blog::find(&slug).ok_or(Error::NotFound)?;
    let base = base(&state);
    let url = format!("{base}/blog/{}", post.slug);
    let posts = blog::published();
    let at = posts.iter().position(|p| p.slug == post.slug).unwrap_or(0);
    let newer = at.checked_sub(1).and_then(|i| posts.get(i)).copied();
    let older = posts.get(at + 1).copied();
    let ld = json!([
        {
            "@context": "https://schema.org",
            "@type": "BlogPosting",
            "headline": post.title,
            "description": post.description,
            "datePublished": post.date,
            "dateModified": post.modified(),
            "author": { "@type": "Person", "name": post.author },
            "publisher": { "@type": "Organization", "name": "Renox", "url": format!("{base}/"), "logo": { "@type": "ImageObject", "url": format!("{base}/img/logo.png") } },
            "mainEntityOfPage": url,
            "image": format!("{base}/img/og.png"),
            "keywords": post.tags.join(", "),
            "wordCount": post.markdown.split_whitespace().count(),
        },
        {
            "@context": "https://schema.org",
            "@type": "BreadcrumbList",
            "itemListElement": [
                { "@type": "ListItem", "position": 1, "name": "Blog", "item": format!("{base}/blog") },
                { "@type": "ListItem", "position": 2, "name": post.title, "item": url },
            ],
        },
    ]);
    Ok(view(
        "blog/post.html",
        context! { post, newer, older, json_ld => json_ld(ld) },
    )
    .into_response())
}

/// The blog's Atom feed, newest first.
async fn feed(State(state): State<AppState>) -> Response {
    let base = base(&state);
    let posts = blog::published();
    let updated = posts
        .first()
        .map(|p| p.modified().to_owned())
        .unwrap_or_else(|| "2026-10-07".into());
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<feed xmlns=\"http://www.w3.org/2005/Atom\">\n  <title>The Renox blog</title>\n  <subtitle>News, releases and deep dives from Renox, the batteries-included web framework for Rust.</subtitle>\n  <link href=\"{base}/blog\"/>\n  <link rel=\"self\" href=\"{base}/blog/feed.xml\"/>\n  <id>{base}/blog</id>\n  <updated>{updated}T00:00:00Z</updated>\n"
    );
    for p in posts {
        xml.push_str(&format!(
            "  <entry>\n    <title>{title}</title>\n    <link href=\"{base}/blog/{slug}\"/>\n    <id>{base}/blog/{slug}</id>\n    <published>{date}T00:00:00Z</published>\n    <updated>{modified}T00:00:00Z</updated>\n    <author><name>{author}</name></author>\n    <summary>{summary}</summary>\n{tags}    <content type=\"html\">{content}</content>\n  </entry>\n",
            title = blog::escape(&p.title),
            slug = p.slug,
            date = p.date,
            modified = p.modified(),
            author = blog::escape(&p.author),
            summary = blog::escape(&p.description),
            tags = p.tags.iter().map(|t| format!("    <category term=\"{}\"/>\n", blog::escape(t))).collect::<String>(),
            content = blog::escape(&p.html),
        ));
    }
    xml.push_str("</feed>\n");
    ([(CONTENT_TYPE, "application/atom+xml; charset=utf-8")], xml).into_response()
}

fn day(date: &str) -> Option<renox::db::DateTime> {
    renox::chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|d| d.and_utc())
}

/// Every page for search engines, with when it last changed.
async fn sitemap(State(state): State<AppState>) -> Result<renox::seo::Sitemap> {
    let posts = blog::published();
    let newest = posts.first().and_then(|p| day(p.modified()));
    let mut map = renox::seo::Sitemap::new(&state)
        .route("home", &[], newest)?
        .route("blog.index", &[], newest)?;
    for post in &posts {
        map = map.route("blog.show", &[&post.slug], day(post.modified()))?;
    }
    for (tag, _) in blog::tags() {
        map = map.route("blog.tag", &[&tag], None)?;
    }
    Ok(map)
}

/// `llms.txt` (llmstxt.org): what Renox is, where its docs are (every page
/// of docs.renox.rs with one line), and the posts.
async fn llms(State(state): State<AppState>) -> Response {
    let base = base(&state);
    let mut text = String::from(
        "# Renox\n\n> Renox is a batteries-included web framework for Rust, modelled on Laravel: Axum, HTMX and Alpine.js, SQLite or PostgreSQL. One dependency (`renox`, `use renox::prelude::*`) gives routing, sessions, CSRF, views (MiniJinja), models and migrations, validation, auth with roles and permissions, queues, a scheduler, mail, notifications, cache, storage, i18n, webhooks, a UI kit and a data grid. Apps compile into one binary. Open source, MIT OR Apache-2.0.\n\n",
    );
    text.push_str(&format!(
        "- Docs: {docs}\n- Source: {repo}\n- Live demo (a bike shop with three stores): {demo}\n- Everything on this site as one text: {base}/llms-full.txt\n\n## Docs\n\n",
        docs = landing::DOCS,
        repo = landing::REPOSITORY,
        demo = landing::DEMO,
    ));
    for page in renox_site::content::PAGES {
        text.push_str(&format!(
            "- [{}]({}/docs/{}): {}\n",
            page.nav,
            landing::DOCS,
            page.slug,
            page.blurb
        ));
    }
    text.push_str("\n## Blog\n\n");
    for post in blog::published() {
        text.push_str(&format!(
            "- [{}]({base}/blog/{}.md): {}\n",
            post.title, post.slug, post.description
        ));
    }
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}

/// `llms-full.txt`: the landing page's text and every post, as Markdown.
async fn llms_full(State(state): State<AppState>) -> Response {
    let base = base(&state);
    let mut text = String::from("# Renox: Laravel's productivity, Rust's guarantees\n\n");
    text.push_str("Renox is a batteries-included web framework for Rust. Routing, models, migrations, validation, auth, queues, mail, a UI kit and more, behind one dependency, compiled into one binary.\n\n");
    text.push_str(&format!(
        "Docs: {} · Source: {} · Demo: {}\n\n## What's included\n\n",
        landing::DOCS,
        landing::REPOSITORY,
        landing::DEMO
    ));
    for f in landing::features() {
        text.push_str(&format!(
            "- **{}**: {} Guide: {}/docs/{}\n",
            f.title,
            f.text,
            landing::DOCS,
            f.doc
        ));
    }
    text.push_str("\n## How a request is handled\n\n");
    for l in landing::pipeline() {
        text.push_str(&format!("- **{}**: {}\n", l.title, l.text));
    }
    text.push_str("\n## Compared with Laravel, Loco and Axum\n\n| | Renox | Laravel | Loco | Axum + crates |\n|---|---|---|---|---|\n");
    for c in landing::comparison() {
        text.push_str(&format!("| {} | {} |\n", c.what, c.cells.join(" | ")));
    }
    text.push_str("\n## Quick start\n\n```bash\ncargo install renox-cli\nrnx new shop && cd shop\nrnx serve\n```\n");
    for post in blog::published() {
        text.push_str(&format!(
            "\n---\n\n# {}\n\n{} · {base}/blog/{}\n\n{}\n",
            post.title, post.date, post.slug, post.markdown
        ));
    }
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}
