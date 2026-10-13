//! renox.rs: every route answers, the SEO tags are there, the words are in
//! the HTML (not only in JavaScript), and every link to docs.renox.rs names
//! a page that exists.

use renox::testing::TestApp;

async fn site() -> TestApp {
    TestApp::with_config(renox_www::app(), |c| c.url = "https://www.renox.rs".into()).await
}

/// Every `https://docs.renox.rs/docs/{slug}` in `html`.
fn doc_links(html: &str) -> Vec<String> {
    html.split("https://docs.renox.rs/docs/")
        .skip(1)
        .map(|rest| {
            rest.split(['"', '#', '?', ')', ' ', '<'])
                .next()
                .unwrap_or("")
                .to_owned()
        })
        .collect()
}

#[renox::test]
async fn the_home_page_has_its_words_its_tags_and_its_structured_data() {
    let app = site().await;
    let page = app.get("/").await;
    page.assert_ok()
        .assert_see("<title>Renox: Laravel&#x27;s productivity, Rust&#x27;s guarantees</title>")
        .assert_see("<meta name=\"description\" content=\"Renox is a batteries-included web framework for Rust")
        .assert_see("<link rel=\"canonical\" href=\"https://www.renox.rs/\">")
        .assert_see("<meta property=\"og:image\" content=\"https://www.renox.rs/img/og.png\">")
        .assert_see("<meta name=\"twitter:card\" content=\"summary_large_image\">")
        .assert_see("\"@type\":\"SoftwareSourceCode\"")
        .assert_see("\"codeRepository\":\"https://github.com/renox-rs/renox\"")
        // The text the motion animates is in the HTML: the terminal, the
        // pipeline's explanations, the code pairs, the crates.
        .assert_see("cargo install renox-cli")
        .assert_see("Security headers &amp; CSP")
        .assert_see("OrderController")
        .assert_see("<span class=\"hl-kw\">")
        .assert_see("tower-sessions")
        .assert_see("Not a hello world.")
        .assert_see("href=\"/blog/renox-1-0\"")
        // The benchmark section, from content/benchmarks.json.
        .assert_see("<h2 id=\"bench-title\">")
        .assert_see("<tr class=\"me\"><th scope=\"row\">Renox</th>")
        .assert_see("9,630");
    let html = page.text();
    assert_eq!(html.matches("<h1").count(), 1, "one h1");
    assert!(html.contains("rel=\"alternate\" type=\"application/atom+xml\""));
}

#[renox::test]
async fn every_docs_link_names_a_page_of_the_docs_site() {
    let app = site().await;
    let slugs: Vec<&str> = renox_site::content::PAGES.iter().map(|p| p.slug).collect();
    let mut checked = 0;
    for path in ["/", "/blog", "/blog/why-we-built-renox"] {
        let html = app.get(path).await.assert_ok().text();
        for link in doc_links(&html) {
            assert!(
                slugs.contains(&link.as_str()),
                "{path} links to docs.renox.rs/docs/{link}, which doesn't exist"
            );
            checked += 1;
        }
    }
    assert!(checked > 10, "checked {checked} links");
}

#[renox::test]
async fn the_blog_lists_posts_and_each_post_is_an_article() {
    let app = site().await;
    app.get("/blog")
        .await
        .assert_ok()
        .assert_see("The Renox blog")
        .assert_see("How a benchmark made Renox 3.3× faster")
        .assert_see("\"@type\":\"Blog\"");
    app.get("/blog/how-a-benchmark-made-renox-faster")
        .await
        .assert_ok()
        .assert_see("<meta property=\"og:type\" content=\"article\">")
        .assert_see("<meta property=\"article:published_time\" content=\"2026-10-07T00:00:00Z\">")
        .assert_see("<link rel=\"canonical\" href=\"https://www.renox.rs/blog/how-a-benchmark-made-renox-faster\">")
        .assert_see("\"@type\":\"BlogPosting\"")
        .assert_see("\"@type\":\"BreadcrumbList\"")
        .assert_see("<h2 id=\"the-fix\">")
        .assert_see("<nav class=\"toc\"");
    app.get("/blog/tags/performance")
        .await
        .assert_ok()
        .assert_see("Posts about performance");
    app.get("/blog/tags/nothing-here").await.assert_status(404);
    app.get("/blog/no-such-post").await.assert_status(404);
}

#[renox::test]
async fn a_post_is_also_markdown() {
    let app = site().await;
    let res = app.get("/blog/why-we-built-renox.md").await;
    res.assert_ok()
        .assert_header("content-type", "text/markdown; charset=utf-8");
    assert!(res.text().starts_with("# Why we built Renox\n"));
    app.get("/blog/no-such-post.md").await.assert_status(404);
}

#[renox::test]
async fn crawlers_get_a_sitemap_a_feed_and_robots_txt() {
    let app = site().await;
    app.get("/sitemap.xml")
        .await
        .assert_ok()
        .assert_see("<loc>https://www.renox.rs/</loc>")
        .assert_see("<loc>https://www.renox.rs/blog/meet-the-bike-shop</loc>")
        .assert_see("<loc>https://www.renox.rs/blog/tags/rust</loc>");
    app.get("/blog/feed.xml")
        .await
        .assert_ok()
        .assert_header("content-type", "application/atom+xml; charset=utf-8")
        .assert_see("<feed xmlns=\"http://www.w3.org/2005/Atom\">")
        .assert_see("<id>https://www.renox.rs/blog/why-we-built-renox</id>");
    app.get("/robots.txt")
        .await
        .assert_ok()
        .assert_see("Allow: /")
        .assert_see("Sitemap: https://www.renox.rs/sitemap.xml");
}

#[renox::test]
async fn language_models_get_llms_txt() {
    let app = site().await;
    let llms = app.get("/llms.txt").await;
    llms.assert_ok()
        .assert_see("# Renox\n")
        .assert_see("- [Tutorial](https://docs.renox.rs/docs/tutorial)")
        .assert_see("https://www.renox.rs/blog/why-we-built-renox.md");
    app.get("/llms-full.txt")
        .await
        .assert_ok()
        .assert_see("## What's included")
        .assert_see("# How a benchmark made Renox 3.3× faster");
}

#[renox::test]
async fn a_missing_page_says_so_and_is_not_indexed() {
    let app = site().await;
    app.get("/nothing-here")
        .await
        .assert_status(404)
        .assert_see("This page doesn't exist.")
        .assert_see("noindex");
    app.get("/docs")
        .await
        .assert_redirect("https://docs.renox.rs");
}

#[renox::test]
async fn google_analytics_is_sent_in_production_only() {
    // Local (the tests' default): no tag.
    let app = site().await;
    app.get("/")
        .await
        .assert_ok()
        .assert_dont_see("googletagmanager.com/gtag");
    // Production: the tag with the request's nonce, and a CSP that lets it load.
    let app = TestApp::with_config(renox_www::app(), |c| {
        c.url = "https://www.renox.rs".into();
        c.env = renox::Environment::Production;
        c.debug = false;
    })
    .await;
    let page = app.get("/").await;
    page.assert_ok()
        .assert_see("<script async src=\"https://www.googletagmanager.com/gtag/js?id=G-48GJK3ZM4J\" nonce=\"")
        .assert_see("gtag('config', 'G-48GJK3ZM4J');");
    let csp = page
        .header("content-security-policy")
        .unwrap_or_default()
        .to_owned();
    assert!(csp.contains("https://www.googletagmanager.com"), "{csp}");
    assert!(csp.contains("https://*.google-analytics.com"), "{csp}");
}
