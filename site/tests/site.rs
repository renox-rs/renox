//! Every page renders, every link between pages lands on a page and a
//! heading that exists, and doctest setup lines stay hidden.

use std::collections::HashSet;

use renox::testing::TestApp;
use renox_site::content::PAGES;

async fn site() -> TestApp {
    TestApp::new(renox_site::app()).await
}

/// The `href`s in a page's HTML.
fn hrefs(html: &str) -> Vec<String> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(|href| href.replace("&amp;", "&"))
        .collect()
}

/// The text of some HTML, without its tags (code is split into colour spans).
fn text_of(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text
}

/// The `id`s in a page's HTML.
fn ids(html: &str) -> HashSet<String> {
    html.split(" id=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_owned)
        .collect()
}

#[renox::test]
async fn every_page_renders_in_the_layout() {
    let site = site().await;
    site.get("/")
        .await
        .assert_ok()
        .assert_see("Start the tutorial");
    for page in PAGES {
        let res = site.get(&format!("/docs/{}", page.slug)).await;
        res.assert_ok()
            .assert_view("page.html")
            .assert_see(&format!("href=\"/docs/{}\"", page.slug));
        assert!(res.header("etag").is_some(), "{}: no ETag", page.slug);
    }
    site.get("/docs/nope").await.assert_not_found();
    site.get("/docs").await.assert_redirect("/docs/overview");
}

#[renox::test]
async fn links_between_pages_land_on_a_heading_that_exists() {
    let site = site().await;
    let mut pages = std::collections::HashMap::new();
    for page in PAGES {
        let html = site.get(&format!("/docs/{}", page.slug)).await.text();
        pages.insert(format!("/docs/{}", page.slug), html);
    }
    let mut broken = Vec::new();
    for (from, html) in &pages {
        for href in hrefs(html) {
            let (path, anchor) = match href.split_once('#') {
                Some((path, anchor)) => (path.to_owned(), Some(anchor.to_owned())),
                None => (href.clone(), None),
            };
            if !path.starts_with("/docs/") && !path.is_empty() {
                continue; // other routes, assets, GitHub
            }
            let target = if path.is_empty() { from.clone() } else { path };
            match pages.get(&target) {
                None => broken.push(format!("{from} → {href}: no such page")),
                Some(target_html) => {
                    if let Some(anchor) = anchor
                        && !ids(target_html).contains(&anchor)
                    {
                        broken.push(format!("{from} → {href}: no such heading"));
                    }
                }
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}

#[renox::test]
async fn doctest_setup_lines_are_not_shown() {
    let site = site().await;
    for page in PAGES {
        let html = text_of(&site.get(&format!("/docs/{}", page.slug)).await.text());
        for hidden in ["\n# use renox", "\n# async fn", "# Ok(())"] {
            assert!(
                !html.contains(hidden),
                "{}: shows a doctest setup line {hidden:?}",
                page.slug
            );
        }
    }
}

#[renox::test]
async fn search_finds_pages_by_their_words() {
    let site = site().await;
    let res = site.get("/search?q=route+model+binding").await;
    res.assert_ok().assert_see("href=\"/docs/routing\"");
    site.get("/search?q=zzzqqq")
        .await
        .assert_see("Nothing found");
    site.get("/search")
        .await
        .assert_ok()
        .assert_dont_see("Nothing found");
}

#[renox::test]
async fn the_sitemap_lists_every_page() {
    let site = site().await;
    let xml = site.get("/sitemap.xml").await.text();
    for page in PAGES {
        assert!(
            xml.contains(&format!("/docs/{}</loc>", page.slug)),
            "{}",
            page.slug
        );
    }
}

#[renox::test]
async fn code_is_shown_in_coloured_panels() {
    let site = site().await;
    let html = site.get("/docs/tutorial").await.text();
    assert!(html.contains("<div class=\"site-code\" data-language=\"rust\">"));
    assert!(html.contains("<div class=\"site-code\" data-language=\"bash\">"));
    assert!(html.contains("<span class=\"hl-kw\">fn</span>"));
    assert!(html.contains("data-copy hidden"));
    // Every page's code is coloured, never left as bare `<pre><code>`.
    for page in PAGES {
        let html = site.get(&format!("/docs/{}", page.slug)).await.text();
        assert_eq!(
            html.matches("<pre>").count(),
            html.matches("<div class=\"site-code\"").count(),
            "{}",
            page.slug
        );
    }
}

#[renox::test]
async fn the_home_page_installs_this_version() {
    let site = site().await;
    site.get("/").await.assert_see(&format!(
        "renox-cli <span class=\"hl-attr\">--version</span> {}",
        env!("CARGO_PKG_VERSION")
    ));
}
