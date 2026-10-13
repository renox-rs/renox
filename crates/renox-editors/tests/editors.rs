//! The editor fields in a form (#149) and the code entry (#150): what they
//! render, the files they load, a form's round trip through `Valid<T>`,
//! live validation, the Markdown preview, and rich text cleaned on the way
//! in and out.

use renox::prelude::*;
use renox::testing::TestApp;
use renox_editors::{Editors, RichText};
use serde::Deserialize;

const FORM: &str = r##"{% from "renox-editors/editors.html" import rich_editor, markdown_editor, code_editor %}
<html><head>{{ renox_head() }}</head><body>
<form method="post" action="/posts" data-live-validate>
{{ csrf_field() }}
{{ rich_editor("body", "Body", value=post.body, required=true, hint="What happened") }}
{{ markdown_editor("notes", "Notes", value=post.notes, rows=5, placeholder="Markdown", required=true) }}
{{ code_editor("config", "Config", value=post.config, language="json") }}
{{ rich_editor("summary", "Summary") }}
</form></body></html>"##;

const SHOW: &str = r##"{% from "renox/ui.html" import infolist %}
{% from "renox-editors/editors.html" import code_entry %}
<div class="body">{{ post.body | rich_text }}</div>
{% call infolist() %}
{{ code_entry("Config", post.config, language="json") }}
{{ code_entry("Settings", post.settings) }}
{{ code_entry("Script", "if a < b { run() }", language="rust", copyable=false, max_height="10rem") }}
{{ code_entry("Nothing", none) }}
{% endcall %}"##;

#[derive(Deserialize, Validate)]
struct PostForm {
    #[validate(required, max = 40)]
    body: RichText,
    #[validate(required)]
    notes: String,
    #[validate(required, json)]
    config: String,
    summary: Option<RichText>,
}

struct Posts;

impl Module for Posts {
    fn name(&self) -> &'static str {
        "posts"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/posts/new", || async {
                view(
                    "form.html",
                    context! { post => context! {
                        body => "<div>Hello <strong>there</strong><script>alert(1)</script></div>",
                        notes => "**Hi**",
                        config => "{\"a\": 1}",
                    } },
                )
            })
            .post("/posts", |Valid(form): Valid<PostForm>| async move {
                Json(json!({
                    "body": form.body,
                    "text": form.body.text(),
                    "notes": form.notes,
                    "config": form.config,
                    "summary": form.summary.map(RichText::into_string),
                }))
            })
            .get("/posts/1", || async {
                view(
                    "show.html",
                    context! { post => context! {
                        body => r#"<p onclick="x()">Saved <a href="javascript:alert(1)">link</a></p><img src=x onerror=alert(1)>"#,
                        config => "{\"a\": \"<b>\"}",
                        settings => json!({"theme": "dark", "size": 2}),
                    } },
                )
            })
    }
}

async fn app() -> TestApp {
    TestApp::new(
        App::new()
            .module(Editors::new())
            .module(Posts)
            .templates(|env| {
                env.add_template("form.html", FORM).unwrap();
                env.add_template("show.html", SHOW).unwrap();
            }),
    )
    .await
}

#[renox::test]
async fn the_fields_render_with_their_names_values_and_files() {
    let app = app().await;
    let html = app.get("/posts/new").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");

    // Rich text: a hidden field of the name holding cleaned HTML, and Trix
    // tied to it and to its toolbar.
    has(
        r#"<input type="hidden" id="rx-body" name="body" value="&lt;div&gt;Hello &lt;strong&gt;there&lt;/strong&gt;&lt;/div&gt;">"#,
    );
    has(
        r#"<trix-editor class="rx-editor__area rx-prose" id="rx-body-editor" input="rx-body" toolbar="rx-body-toolbar""#,
    );
    has(
        r#"aria-labelledby="rx-body-label" aria-describedby="rx-body-hint rx-body-error" aria-required="true""#,
    );
    has(r#"data-trix-attribute="bold" data-trix-key="b""#);
    has(r#"aria-label="Bold" data-rx-tip="Bold""#);
    // The link dialog's address isn't sent with the form.
    has(r#"name="href" form="rx-editor-none""#);
    has(r#"<p class="rx-error" id="rx-body-error" data-error-for="body" aria-live="polite"></p>"#);
    assert!(!html.contains("alert(1)"), "{html}");

    // Markdown: a textarea of the name, a toolbar and the preview's route.
    has(r#"data-preview-url="/_renox/editors/preview""#);
    has(
        r#"<textarea class="rx-editor__area rx-editor__textarea" id="rx-notes" name="notes" rows="5" required aria-required="true" placeholder="Markdown""#,
    );
    has(">**Hi**</textarea>");
    has(r#"data-md="bold""#);

    // Code: a textarea (the field, and the editor without JavaScript).
    has(r#"data-rx-code-editor data-language="json" data-tab="  ""#);
    has(r#"name="config" rows="10" spellcheck="false""#);
    has(">{&quot;a&quot;: 1}</textarea>");
    has(r#"<span class="rx-required">(optional)</span>"#);

    // The files load once, however many editors the page has.
    assert_eq!(html.matches("<script type=\"module\"").count(), 1, "{html}");
    let css = between(&html, "<link rel=\"stylesheet\" href=\"", "\"");
    let js = between(&html, "<script type=\"module\" src=\"", "\"");
    assert!(css.starts_with("/_renox/editors/editors-") && css.ends_with(".css"));
    assert!(js.starts_with("/_renox/editors/editors-") && js.ends_with(".js"));
    has(r#"data-trix="/_renox/editors/trix-2.1.19.min.js""#);
    has(r#"data-prism="/_renox/editors/prism-1.30.0.min.js""#);
    has(r#"data-codejar="/_renox/editors/codejar-4.3.0.js""#);

    for (path, kind, text) in [
        (css.as_str(), "text/css; charset=utf-8", "trix-editor"),
        (
            js.as_str(),
            "text/javascript; charset=utf-8",
            "data-renox-editors",
        ),
        (
            "/_renox/editors/trix-2.1.19.min.js",
            "text/javascript; charset=utf-8",
            "Trix 2.1.19",
        ),
        (
            "/_renox/editors/prism-1.30.0.min.js",
            "text/javascript; charset=utf-8",
            "Prism.manual = true",
        ),
        (
            "/_renox/editors/codejar-4.3.0.js",
            "text/javascript; charset=utf-8",
            "export function CodeJar",
        ),
    ] {
        let res = app.get(path).await;
        res.assert_ok()
            .assert_header("content-type", kind)
            .assert_header("cache-control", "public, max-age=31536000, immutable")
            .assert_see(text);
        assert_eq!(res.header("set-cookie"), None, "{path}");
    }
}

#[renox::test]
async fn a_valid_form_arrives_as_plain_fields_with_rich_text_cleaned() {
    let app = app().await;
    let res = app
        .post(
            "/posts",
            &[
                ("body", r#"<div>Hi <em>all</em><img src=x onerror="alert(1)"><script>alert(2)</script></div>"#),
                ("notes", "# Title\n\n<script>x</script>"),
                ("config", "{\"a\": [1, 2]}"),
                ("summary", r#"<a href="javascript:alert(1)" onclick="x">s</a>"#),
            ],
        )
        .await;
    res.assert_ok()
        .assert_json_path("body", "<div>Hi <em>all</em></div>")
        .assert_json_path("text", "Hi all")
        // Markdown and code are kept as typed: they're text, escaped when shown.
        .assert_json_path("notes", "# Title\n\n<script>x</script>")
        .assert_json_path("config", "{\"a\": [1, 2]}")
        .assert_json_path("summary", r#"<a rel="noopener noreferrer nofollow">s</a>"#);
}

#[renox::test]
async fn errors_and_old_input_come_back_to_the_fields() {
    let app = app().await;
    app.get("/posts/new").await.assert_ok();
    // An emptied editor still sends markup: `required` sees no text.
    // `max` counts letters, not tags.
    let back = app
        .request()
        .header("referer", "/posts/new")
        .post(
            "/posts",
            &[
                ("body", "<div><br></div>"),
                ("notes", ""),
                ("config", "{not json"),
            ],
        )
        .await;
    back.assert_redirect("/posts/new");
    let html = app.get("/posts/new").await.text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"rx-editor rx-editor--rich rx-editor--invalid"#);
    has(
        r#"<input type="hidden" id="rx-body" name="body" value="&lt;div&gt;&lt;br&gt;&lt;/div&gt;" aria-invalid="true">"#,
    );
    has(r#"data-error-for="body" aria-live="polite">The body field is required."#);
    has(r#"data-error-for="notes" aria-live="polite">The notes field is required."#);
    has(r#"data-error-for="config" aria-live="polite">The config must be valid JSON."#);
    // Old input wins over the value the page gives.
    has(">{not json</textarea>");
    has(r#"rows="5" required aria-required="true" placeholder="Markdown" aria-invalid="true""#);

    // Long rich text: the letters are counted, not the tags.
    let long = format!("<div><strong>{}</strong></div>", "a".repeat(41));
    app.htmx()
        .post(
            "/posts",
            &[("body", &long), ("notes", "n"), ("config", "{}")],
        )
        .await
        .assert_invalid("body");
    let short = format!("<div><strong>{}</strong></div>", "a".repeat(40));
    app.htmx()
        .post(
            "/posts",
            &[("body", &short), ("notes", "n"), ("config", "{}")],
        )
        .await
        .assert_ok();
}

#[renox::test]
async fn live_validation_checks_an_editor_field() {
    let app = app().await;
    let check = |field: &'static str, value: &'static str| {
        let app = &app;
        async move {
            app.request()
                .header("X-Renox-Validate", field)
                .header("Accept", "application/json")
                .post("/posts", &[(field, value)])
                .await
                .json::<renox::serde_json::Value>()
        }
    };
    let empty = check("body", "<div><br></div>").await;
    assert_eq!(empty["field"], "body");
    assert_eq!(empty["errors"][0], "The body field is required.");
    let fine = check("body", "<div>Hi</div>").await;
    assert_eq!(fine["errors"], json!([]));
    let code = check("config", "[1,").await;
    assert_eq!(code["errors"][0], "The config must be valid JSON.");
}

#[renox::test]
async fn the_preview_renders_markdown_as_the_page_does() {
    let app = app().await;
    app.get("/posts/new").await;
    app.post(
        "/_renox/editors/preview",
        &[("text", "**Bold** and <script>alert(1)</script>\n\n- one")],
    )
    .await
    .assert_ok()
    .assert_see("<strong>Bold</strong>")
    .assert_see("&lt;script&gt;alert(1)&lt;/script&gt;")
    .assert_see("<li>one</li>")
    .assert_dont_see("<script>");
    // A form post like any other: it needs the CSRF token.
    app.request()
        .without_csrf()
        .post("/_renox/editors/preview", &[("text", "x")])
        .await
        .assert_status(419);
    // Very long text isn't rendered.
    let long = "a".repeat(200_001);
    app.post("/_renox/editors/preview", &[("text", &long)])
        .await
        .assert_status(413);
}

#[renox::test]
async fn stored_rich_text_is_cleaned_again_and_code_is_shown_highlighted() {
    let app = app().await;
    let html = app.get("/posts/1").await.assert_ok().text();
    let has = |needle: &str| assert!(html.contains(needle), "missing {needle}\n{html}");
    has(r#"<div class="body"><p>Saved <a rel="noopener noreferrer nofollow">link</a></p></div>"#);
    assert!(
        !html.contains("onerror") && !html.contains("onclick") && !html.contains("javascript:")
    );

    // Code entries: escaped text in a highlighted block, with a copy button.
    has(r#"<div class="rx-entry rx-entry--code rx-span-full">"#);
    has(
        r#"<code class="language-json" data-rx-highlight translate="no">{&quot;a&quot;: &quot;&lt;b&gt;&quot;}</code>"#,
    );
    has(r#"data-rx-copy-text="{&quot;a&quot;: &quot;&lt;b&gt;&quot;}""#);
    // A map is shown as indented JSON.
    has(
        "<code class=\"language-json\" data-rx-highlight translate=\"no\">{\n  &quot;size&quot;: 2,\n  &quot;theme&quot;: &quot;dark&quot;\n}</code>",
    );
    has(r#"style="max-height: 10rem""#);
    has(
        r#"<code class="language-rust" data-rx-highlight translate="no">if a &lt; b { run() }</code>"#,
    );
    assert_eq!(html.matches("rx-code-entry__copy").count(), 2, "{html}");
    has(r#"<span class="rx-entry__empty">—</span>"#);
    // The files come with the first entry.
    assert_eq!(html.matches("<script type=\"module\"").count(), 1);
}

#[renox::test]
async fn an_app_translates_the_editors_texts() {
    let dir = tempfile_dir();
    std::fs::write(
        dir.join("es.json"),
        r#"{"editors.bold": "Negrita", "editors.preview": "Vista previa"}"#,
    )
    .unwrap();
    let lang = dir.clone();
    let app = TestApp::with_config(
        App::new()
            .module(Editors::new())
            .module(Posts)
            .templates(|env| env.add_template("form.html", FORM).unwrap()),
        move |c| {
            c.locale = "es".into();
            c.lang_path = lang;
        },
    )
    .await;
    app.get("/posts/new")
        .await
        .assert_see(r#"aria-label="Negrita""#)
        .assert_see("<span>Vista previa</span>")
        .assert_see(r#"aria-label="Italic""#);
    let _ = std::fs::remove_dir_all(dir);
}

/// A fresh directory under the system's temp dir.
fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("renox-editors-{}", renox::random_token()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn between(html: &str, start: &str, end: &str) -> String {
    let from = html.find(start).unwrap() + start.len();
    let to = html[from..].find(end).unwrap() + from;
    html[from..to].to_owned()
}

/// Rich text bound in a query is the cleaned HTML; read back as text it
/// stays clean. An empty preview renders an empty page body.
#[renox::test]
async fn rich_text_is_stored_cleaned_and_an_empty_preview_is_fine() {
    let app = app().await;
    let db = app.db();
    renox::db::sql("CREATE TABLE notes (body TEXT NOT NULL)")
        .execute(db)
        .await
        .unwrap();
    let body = RichText::new(r#"<p onclick="x()">Hi <script>alert(1)</script><b>there</b></p>"#);
    renox::db::sql("INSERT INTO notes (body) VALUES (?)")
        .bind(body.clone())
        .execute(db)
        .await
        .unwrap();
    let stored: String = renox::db::sql("SELECT body FROM notes")
        .scalar(db)
        .await
        .unwrap();
    assert_eq!(stored, body.as_str());
    assert!(
        !stored.contains("script") && !stored.contains("onclick"),
        "{stored}"
    );
    assert!(stored.contains("<b>there</b>"), "{stored}");
    // Read back into a form-like value, it's cleaned again on the way in.
    let again: RichText = renox::serde_json::from_value(json!(stored)).unwrap();
    assert_eq!(again.as_str(), stored);

    app.get("/posts/new").await;
    app.post("/_renox/editors/preview", &[("text", "")])
        .await
        .assert_ok()
        .assert_dont_see("<p>");
    app.post("/_renox/editors/preview", &[]).await.assert_ok();
}

#[renox::test]
async fn the_editors_work_as_tags() {
    let tags = r#"<form method="post" action="/posts">
<rx-rich-editor name="body" label="Body" :value="post.body" required />
<rx-markdown-editor name="notes" label="Notes" :value="post.notes" rows="5" />
<rx-code-editor name="config" label="Config" :value="post.config" language="json" />
</form>
<dl><rx-code-entry label="Config" :value="post.config" language="json" /></dl>"#;
    let app = TestApp::new(
        App::new()
            .module(Editors::new())
            .module(Posts)
            .module(Tags)
            .templates(move |env| {
                renox::view::add_template(env, "tags.html", tags).unwrap();
            }),
    )
    .await;
    let html = app.get("/tags").await.assert_ok().text();
    for needle in [
        r#"name="body""#,
        r#"<trix-editor"#,
        r#"data-preview-url="/_renox/editors/preview""#,
        r#"data-rx-code-editor data-language="json""#,
        r#"class="rx-code-entry""#,
    ] {
        assert!(html.contains(needle), "missing {needle}\n{html}");
    }
}

struct Tags;

impl Module for Tags {
    fn name(&self) -> &'static str {
        "tags"
    }

    fn routes(&self) -> Routes {
        Routes::new().get("/tags", || async {
            view(
                "tags.html",
                context! { post => context! { body => "<p>Hi</p>", notes => "**Hi**", config => "{\"a\": 1}" } },
            )
        })
    }
}
