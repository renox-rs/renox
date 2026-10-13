//! learn.renox.rs: every route answers, the SEO tags and the structured data
//! are there, the course's outline is in the HTML, and every link to
//! docs.renox.rs names a page that exists.

use renox::testing::TestApp;

async fn site() -> TestApp {
    TestApp::with_config(renox_learn::app(), |c| {
        c.url = "https://learn.renox.rs".into()
    })
    .await
}

/// Every `https://docs.renox.rs/docs/{slug}` in `html`.
fn doc_links(html: &str) -> Vec<String> {
    html.split("https://docs.renox.rs/docs/")
        .skip(1)
        .map(|rest| {
            rest.split(['"', '#', '?', ')', ' ', '<', '\n'])
                .next()
                .unwrap_or("")
                .to_owned()
        })
        .collect()
}

#[renox::test]
async fn the_home_page_lists_every_stage_with_its_tags_and_structured_data() {
    let app = site().await;
    let page = app.get("/").await;
    page.assert_ok()
        .assert_see("<title>Learn Renox: build a web app in Rust, stage by stage</title>")
        .assert_see("<meta name=\"description\" content=\"A course that builds one Renox app")
        .assert_see("<link rel=\"canonical\" href=\"https://learn.renox.rs/\">")
        .assert_see("\"@type\":\"Course\"")
        .assert_see("\"@type\":\"Syllabus\"")
        .assert_see("<a href=\"/\" aria-current=\"page\">Course</a>");
    for stage in renox_learn::course::STAGES {
        page.assert_see(&format!("id=\"{}\"", stage.slug));
    }
    let html = page.text();
    assert_eq!(html.matches("<h1").count(), 1, "one h1");
    // The JSON-LD can't close its <script> early.
    let ld = html.split("application/ld+json\">").nth(1).unwrap();
    assert!(!ld.split("</script>").next().unwrap().contains("</"));
}

#[renox::test]
async fn the_stages_are_numbered_in_order_with_unique_slugs() {
    let stages = renox_learn::course::STAGES;
    for (i, stage) in stages.iter().enumerate() {
        assert_eq!(
            usize::from(stage.number),
            i,
            "{} is out of order",
            stage.slug
        );
        assert!(!stage.topics.is_empty(), "{} has no topics", stage.slug);
    }
    let mut slugs: Vec<_> = stages.iter().map(|s| s.slug).collect();
    slugs.sort();
    slugs.dedup();
    assert_eq!(slugs.len(), stages.len(), "a slug is used twice");
}

#[renox::test]
async fn every_docs_link_names_a_page_of_the_docs_site() {
    let app = site().await;
    let slugs: Vec<&str> = renox_site::content::PAGES.iter().map(|p| p.slug).collect();
    let mut checked = 0;
    for path in ["/", "/llms.txt", "/no-such-page"] {
        let html = app.get(path).await.text();
        for link in doc_links(&html) {
            assert!(
                slugs.contains(&link.as_str()),
                "{path} links to docs.renox.rs/docs/{link}, which doesn't exist"
            );
            checked += 1;
        }
    }
    assert!(checked >= 10, "checked {checked} links");
}

#[renox::test]
async fn sitemap_robots_and_llms_txt_are_served() {
    let app = site().await;
    app.get("/sitemap.xml")
        .await
        .assert_ok()
        .assert_see("<loc>https://learn.renox.rs/</loc>");
    app.get("/robots.txt")
        .await
        .assert_ok()
        .assert_see("Sitemap: https://learn.renox.rs/sitemap.xml");
    let llms = app.get("/llms.txt").await;
    llms.assert_ok()
        .assert_header("content-type", "text/plain; charset=utf-8")
        .assert_see("# Learn Renox")
        .assert_see("- Stage 7: A live component on a model:");
}

#[renox::test]
async fn an_unknown_page_is_a_404_in_the_layout_and_not_indexed() {
    let app = site().await;
    app.get("/no-such-page")
        .await
        .assert_status(404)
        .assert_see("This page doesn&#x27;t exist.")
        .assert_see("<meta name=\"robots\" content=\"noindex\">")
        .assert_see("learn renox");
}

#[renox::test]
async fn the_home_page_has_an_etag_and_answers_304_to_it() {
    let app = site().await;
    let first = app.get("/").await;
    let etag = first.header("etag").expect("an ETag").to_owned();
    app.request()
        .header("if-none-match", &etag)
        .get("/")
        .await
        .assert_status(304);
}

#[renox::test]
async fn the_views_compile_and_are_written_with_tags_only() {
    let app = site().await;
    app.assert_views_compile();
    // The course teaches the 1.1 way, so its own pages are written that way:
    // tags and rx-if / rx-for, no {% %} statements.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/views");
    let mut stack = vec![dir];
    let mut checked = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(
                    !text.contains("{%"),
                    "{} has a {{% %}} statement",
                    path.display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 3, "checked {checked} views");
}
