//! `/about/fields` (#351): every kind of form field from the browser to the
//! database and back into the edit form, files included. These tests were
//! the fields and uploads examples' until #351; they run on SQLite, or on
//! PostgreSQL with TEST_DATABASE_URL set (the CI's PostgreSQL job), and the
//! last one on S3 with the TEST_S3_* variables and `--features s3`.

use bikeshop::app::about::fields::{FieldSample, FrameSize};
use bikeshop::app::catalog::model::Brand;
use renox::chrono::{NaiveDate, NaiveTime};
use renox::prelude::*;
use renox::testing::{TestApp, TestResponse};

/// Not a real image, but it starts like one: a PNG header saying 800 × 600.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x03\x20\0\0\x02\x58 and no pixels";
/// The same, 8000 pixels wide.
const WIDE_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x1f\x40\0\0\x02\x58 and no pixels";
const PDF: &[u8] = b"%PDF-1.7\n% a manual\n";

const FULL: &[(&str, &str)] = &[
    ("name", "Commuter 3"),
    ("brand", "Riverside"),
    ("description", "A city bike"),
    ("stock", "12"),
    ("weight_kg", "11.25"),
    ("price", "649.99"),
    ("available", "on"),
    ("size", "large"),
    ("colors", "teal"),
    ("colors", "black"),
    ("pickup_at", "07:30"),
    ("launch_at", "2026-10-01T10:30"),
    ("released_on", "2026-09-27"),
];

/// The app with someone logged in.
async fn app() -> (TestApp, User) {
    let app = TestApp::new(bikeshop::app()).await;
    let user = User::register(app.db(), "Rita", "rita@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&user);
    (app, user)
}

async fn only_sample(app: &TestApp) -> FieldSample {
    FieldSample::query().first(app.db()).await.unwrap().unwrap()
}

#[renox::test]
async fn the_reference_is_public_and_the_form_needs_a_login() {
    let app = TestApp::new(bikeshop::app()).await;
    app.get("/about/fields")
        .await
        .assert_ok()
        .assert_view("about/fields.html")
        .assert_see("Input, Rust and database")
        .assert_see("<code>Json&lt;KeyValues&gt;</code>")
        .assert_see("<code>DOUBLE PRECISION</code>")
        .assert_see("Log in to try the form")
        .assert_dont_see(r#"id="sample-form""#);
    app.post("/about/fields", FULL)
        .await
        .assert_redirect("/login");

    let (app, _) = self::app().await;
    app.get("/about/fields")
        .await
        .assert_see(r#"id="sample-form""#)
        .assert_see("No samples yet");
}

#[renox::test]
async fn a_sample_round_trips_from_the_form_to_the_database_and_back() {
    let (app, user) = app().await;
    // The brand field suggests the catalogue's brands.
    Brand::create(
        app.db(),
        Brand {
            name: "Riverside".into(),
            slug: "riverside".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let res = app.post("/about/fields", FULL).await;
    res.assert_status(303);
    let show = res.header("location").unwrap().to_owned();

    let sample = only_sample(&app).await;
    assert_eq!(sample.user_id, user.id);
    assert_eq!(sample.name, "Commuter 3");
    assert_eq!(sample.brand.as_deref(), Some("Riverside"));
    assert_eq!(sample.description.as_deref(), Some("A city bike"));
    assert_eq!(
        (sample.stock, sample.weight_kg, sample.price),
        (12, 11.25, 64_999)
    );
    assert!(sample.available);
    assert_eq!(sample.size, FrameSize::Large);
    assert_eq!(*sample.colors, ["teal", "black"]);
    assert_eq!(sample.pickup_at, NaiveTime::from_hms_opt(7, 30, 0));
    assert_eq!(
        sample.launch_at,
        NaiveDate::from_ymd_opt(2026, 10, 1)
            .unwrap()
            .and_hms_opt(10, 30, 0)
    );
    assert_eq!(sample.released_on, NaiveDate::from_ymd_opt(2026, 9, 27));
    assert_eq!(show, format!("/about/fields/{}", sample.id));

    // The edit form shows every value in the format its input expects.
    app.get(&format!("{show}/edit"))
        .await
        .assert_ok()
        .assert_see(r#"name="name" type="text" value="Commuter 3""#)
        .assert_see(r#"list="rx-brand-list""#)
        .assert_see(r#"<option value="Riverside">"#)
        .assert_see(">A city bike</textarea>")
        .assert_see(r#"value="11.25""#)
        // The price in dollars, stored in cents.
        .assert_see(r#"name="price" type="number" value="649.99""#)
        .assert_see(r#"name="available" value="on" checked"#)
        .assert_see(r#"name="size" value="large" checked"#)
        .assert_see(r#"value="teal" checked"#)
        .assert_see(r#"value="black" checked"#)
        .assert_see(r#"value="07:30:00""#)
        .assert_see(r#"value="2026-10-01T10:30:00""#)
        .assert_see(r#"value="2026-09-27""#)
        // The key, read-only with a copy button; the date in the kit's picker.
        .assert_see(&format!(r#"value="{}" readonly"#, sample.id))
        .assert_see(r#"data-rx-copy="rx-key""#)
        .assert_see(r#"popovertarget="rx-released_on-calendar""#);
}

#[renox::test]
async fn unchecking_and_emptying_fields_saves_them_empty() {
    let (app, _) = app().await;
    let show = app
        .post("/about/fields", FULL)
        .await
        .header("location")
        .unwrap()
        .to_owned();
    app.put(
        &show,
        &[
            ("name", "Commuter 3"),
            ("stock", "0"),
            ("weight_kg", "0"),
            ("price", "0"),
            ("size", "small"),
            ("description", ""),
            ("brand", ""),
            ("pickup_at", ""),
        ],
    )
    .await
    .assert_redirect(&show);
    let sample = only_sample(&app).await;
    assert!(!sample.available);
    assert!(sample.colors.is_empty());
    assert_eq!(sample.description, None);
    assert_eq!(sample.brand, None);
    assert_eq!(sample.pickup_at, None);
    assert_eq!(sample.size, FrameSize::Small);
}

#[renox::test]
async fn invalid_values_are_reported_together() {
    let (app, _) = app().await;
    app.htmx()
        .post(
            "/about/fields",
            &[
                ("name", ""),
                ("stock", "-1"),
                ("weight_kg", "heavy"),
                ("price", "1.234"),
                ("size", "huge"),
                ("colors", "purple"),
            ],
        )
        .await
        .assert_invalid("name")
        .assert_invalid("stock")
        .assert_invalid("weight_kg")
        .assert_invalid("price")
        .assert_invalid("size")
        .assert_invalid("colors.0");
}

async fn post_colors(app: &TestApp, colors: &[&str]) -> TestResponse {
    let mut form = vec![
        ("name", "Kids 16"),
        ("size", "small"),
        ("stock", "1"),
        ("weight_kg", "7"),
        ("price", "199"),
    ];
    form.extend(colors.iter().map(|color| ("colors", *color)));
    app.htmx().post("/about/fields", &form).await
}

#[renox::test]
async fn each_colour_is_checked_and_repeats_are_refused() {
    let (app, _) = app().await;
    // Only the unknown one is reported, under its index.
    let res = post_colors(&app, &["black", "purple"]).await;
    res.assert_invalid("colors.1");
    assert!(res.json_path("errors.colors.0").is_null());
    // The repeat gets the error (case and spaces don't make it new).
    post_colors(&app, &["teal", "Teal "])
        .await
        .assert_invalid("colors.1");

    // Without htmx: back to the form, with the first item's error in the
    // `colors` slot, and only what was sent ticked.
    app.request()
        .header("referer", "/about/fields")
        .post(
            "/about/fields",
            &[("name", "Kids 16"), ("colors", "purple")],
        )
        .await
        .assert_redirect("/about/fields");
    app.get("/about/fields")
        .await
        .assert_see(
            r#"data-error-for="colors" aria-live="polite">The selected colors #1 is invalid."#,
        )
        .assert_dont_see(r#"name="colors" value="black" checked"#);
}

#[renox::test]
async fn tags_and_specifications_ride_a_nested_form() {
    let (app, _) = app().await;
    // `specs[0][key]` makes the whole form nested: every other field still
    // parses from its text (numbers, the enum, the checkbox, dates, times).
    let mut form = FULL.to_vec();
    form.extend([
        ("tags", "commuter"),
        ("tags", "e-bike"),
        ("tags", ""),
        ("specs[0][key]", "Frame"),
        ("specs[0][value]", "Aluminium"),
        ("specs[1][key]", ""),
        ("specs[1][value]", ""),
        ("specs[2][key]", "Gears"),
        ("specs[2][value]", "9"),
    ]);
    let res = app.post("/about/fields", &form).await;
    res.assert_status(303);
    let show = res.header("location").unwrap().to_owned();
    let sample = only_sample(&app).await;
    assert_eq!(*sample.tags, ["commuter", "e-bike"]);
    assert_eq!(
        sample.specs.iter().collect::<Vec<_>>(),
        [("Frame", "Aluminium"), ("Gears", "9")]
    );
    assert_eq!(sample.size, FrameSize::Large);
    assert!(sample.available);
    assert_eq!(sample.pickup_at, NaiveTime::from_hms_opt(7, 30, 0));
    assert_eq!(*sample.colors, ["teal", "black"]);

    // The edit form shows them back: chips, and a row per pair.
    app.get(&format!("{show}/edit"))
        .await
        .assert_see(r#"<input type="hidden" name="tags" value="e-bike">"#)
        .assert_see(r#"name="specs[1][key]" type="text" value="Gears""#)
        .assert_see(r#"name="specs[1][value]" type="text" value="9""#);

    // Too many tags: the error is the list's.
    let mut form = FULL.to_vec();
    for tag in ["a", "b", "c", "d", "e", "f"] {
        form.push(("tags", tag));
    }
    form.push(("specs[0][key]", "Frame"));
    app.htmx()
        .post("/about/fields", &form)
        .await
        .assert_invalid("tags");
}

#[renox::test]
async fn the_sample_page_formats_every_field() {
    let (app, _) = app().await;
    let mut form: Vec<(&str, &str)> = FULL
        .iter()
        .map(|&(k, v)| {
            (
                k,
                if k == "description" {
                    "**Light** frame <b>here</b>"
                } else {
                    v
                },
            )
        })
        .collect();
    form.extend([
        ("tags", "commuter"),
        ("specs[0][key]", "Frame"),
        ("specs[0][value]", "Aluminium"),
    ]);
    app.post("/about/fields", &form).await.assert_status(303);
    let sample = only_sample(&app).await;
    app.get("/about/fields").await.assert_see(&format!(
        r#"href="/about/fields/{}"><strong>Commuter 3</strong>"#,
        sample.id
    ));
    app.get(&format!("/about/fields/{}", sample.id))
        .await
        .assert_ok()
        .assert_view("about/fields_show.html")
        .assert_see(&format!(r#"data-rx-copy-text="{}""#, sample.id))
        // Markdown, with the HTML typed in shown as text.
        .assert_see("<strong>Light</strong> frame &lt;b&gt;here&lt;/b&gt;")
        .assert_see("$649.99")
        .assert_see("11.25<span class=\"rx-entry__affix\">kg</span>")
        .assert_see("</svg>Yes</span>")
        .assert_see(r#"<span class="rx-badge rx-badge--info">Large</span>"#)
        .assert_see(r#"style="background: teal""#)
        .assert_see(r#"<span class="rx-badge">commuter</span>"#)
        .assert_see(r#"<th scope="row">Frame</th><td>Aluminium</td>"#)
        .assert_see(">07:30:00<")
        .assert_see(">01 Oct 2026, 10:30<")
        .assert_see(">27 Sep 2026<")
        .assert_see(">just now</time>");
}

#[renox::test]
async fn saving_shows_a_toast_on_the_next_page() {
    let (app, _) = app().await;
    let res = app.post("/about/fields", FULL).await;
    let show = res.header("location").unwrap().to_owned();
    app.get(&show).await.assert_see("Sample saved.");
    app.put(&show, FULL).await.assert_redirect(&show);
    app.get(&show).await.assert_see("Changes saved.");
    // Once only.
    app.get(&show).await.assert_dont_see("Changes saved.");
}

#[renox::test]
async fn samples_are_private_and_can_be_deleted() {
    let (app, _) = app().await;
    app.post("/about/fields", FULL).await.assert_status(303);
    let sample = only_sample(&app).await;
    let show = format!("/about/fields/{}", sample.id);

    // Someone else gets a 404, and doesn't see it in their list.
    let other = User::register(app.db(), "Sam", "sam@example.com", "password123")
        .await
        .unwrap();
    app.acting_as(&other);
    app.get(&show).await.assert_not_found();
    app.get(&format!("{show}/edit")).await.assert_not_found();
    app.delete(&show).await.assert_not_found();
    app.get("/about/fields")
        .await
        .assert_dont_see("<strong>Commuter 3</strong>");
    // A key that isn't a UUID is a 404 too.
    app.get("/about/fields/not-a-uuid").await.assert_not_found();

    let owner = User::find_by_email(app.db(), "rita@example.com")
        .await
        .unwrap()
        .unwrap();
    app.acting_as(&owner);
    app.delete(&show).await.assert_redirect("/about/fields");
    assert!(
        FieldSample::find(app.db(), sample.id)
            .await
            .unwrap()
            .is_none()
    );
    app.get("/about/fields")
        .await
        .assert_see("“Commuter 3” deleted.")
        .assert_see("No samples yet");
    app.delete(&show).await.assert_not_found();
}

#[renox::test]
async fn the_editors_save_rich_text_cleaned_and_code_as_typed() {
    let (app, _) = app().await;
    let mut form = FULL.to_vec();
    form.extend([
        (
            "details",
            r#"<div>Serviced <strong>yearly</strong><img src=x onerror="alert(1)"></div>"#,
        ),
        ("settings", "{\"wheel\": \"<700c>\"}"),
    ]);
    app.post("/about/fields", &form).await.assert_status(303);
    let sample = only_sample(&app).await;
    assert_eq!(
        sample.details.as_deref(),
        Some("<div>Serviced <strong>yearly</strong></div>")
    );
    assert_eq!(sample.settings.as_deref(), Some("{\"wheel\": \"<700c>\"}"));

    // The edit form gives each editor its value; the page loads the editors once.
    let edit = app
        .get(&format!("/about/fields/{}/edit", sample.id))
        .await
        .assert_ok()
        .assert_see(r#"name="details" value="&lt;div&gt;Serviced &lt;strong&gt;yearly&lt;/strong&gt;&lt;/div&gt;""#)
        .assert_see(r#"data-rx-code-editor data-language="json""#)
        .assert_see(">{&quot;wheel&quot;: &quot;&lt;700c&gt;&quot;}</textarea>")
        .assert_see("data-rx-markdown")
        .text();
    assert_eq!(edit.matches("data-renox-editors").count(), 1);

    // The page shows the rich text and highlights the code.
    app.get(&format!("/about/fields/{}", sample.id))
        .await
        .assert_see("<div>Serviced <strong>yearly</strong></div>")
        .assert_see(r#"<code class="language-json" data-rx-highlight translate="no">{&quot;wheel&quot;: &quot;&lt;700c&gt;&quot;}</code>"#)
        .assert_see(&format!(
            r#"href="/about/fields/{}/edit" aria-label="Edit""#,
            sample.id
        ));

    // Settings that aren't JSON are refused; an emptied rich text editor
    // and empty settings store nothing.
    let mut bad = FULL.to_vec();
    bad.extend([("details", "<div><br></div>"), ("settings", "{wheel")]);
    app.htmx()
        .post("/about/fields", &bad)
        .await
        .assert_invalid("settings");
    let mut empty = FULL.to_vec();
    empty.extend([("details", "<div><br></div>"), ("settings", "")]);
    let res = app.post("/about/fields", &empty).await;
    let id = res.header("location").unwrap().rsplit('/').next().unwrap();
    let saved = FieldSample::find(app.db(), id.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!((saved.details, saved.settings), (None, None));
}

#[renox::test]
async fn files_are_checked_by_content_and_stored_public_or_private() {
    let (app, _) = app().await;
    let fields: Vec<(&str, &str)> = FULL.to_vec();

    // A text file named .png, a photo too wide, a manual that isn't a PDF.
    app.htmx()
        .post_multipart(
            "/about/fields",
            &fields,
            &[("photo", "bike.png", b"just text")],
        )
        .await
        .assert_invalid("photo");
    app.htmx()
        .post_multipart("/about/fields", &fields, &[("photo", "wide.png", WIDE_PNG)])
        .await
        .assert_invalid("photo")
        .assert_see("invalid image dimensions");
    app.htmx()
        .post_multipart("/about/fields", &fields, &[("manual", "manual.pdf", PNG)])
        .await
        .assert_invalid("manual");

    // Both right: the photo on the public part of the disk, the manual private.
    let res = app
        .post_multipart(
            "/about/fields",
            &fields,
            &[
                ("photo", "bike.png", PNG),
                ("manual", "Owner manual.pdf", PDF),
            ],
        )
        .await;
    res.assert_status(303);
    let show = res.header("location").unwrap().to_owned();
    let sample = only_sample(&app).await;
    let storage = &app.state().storage;
    let photo_url = storage.url(sample.photo.as_deref().unwrap());
    assert!(photo_url.starts_with("/storage/samples/"), "{photo_url}");
    app.get(&photo_url).await.assert_ok();
    assert_eq!(sample.manual_name.as_deref(), Some("Owner manual.pdf"));
    let manual_key = sample.manual.clone().unwrap();
    // The private file has no public address…
    app.get(&storage.url(&manual_key)).await.assert_not_found();
    // …the app sends it, inline, with its name, to its owner only.
    let file = app.get(&format!("{show}/manual")).await;
    file.assert_ok();
    let disposition = file.header("content-disposition").unwrap_or_default();
    assert!(disposition.starts_with("inline"), "{disposition}");
    assert!(disposition.contains("Owner manual.pdf"), "{disposition}");
    app.get(&show)
        .await
        .assert_see(&photo_url)
        .assert_see(&format!(r#"href="{show}/manual""#));

    // A new photo replaces the old file; deleting the sample deletes the rest.
    let old_photo = sample.photo.clone().unwrap();
    // A multipart form says PUT with `_method` (`method_field('PUT')`).
    let mut put = fields.clone();
    put.push(("_method", "PUT"));
    app.post_multipart(&show, &put, &[("photo", "new.png", PNG)])
        .await
        .assert_redirect(&show);
    assert!(!storage.exists(&old_photo).await.unwrap());
    let sample = only_sample(&app).await;
    app.delete(&show).await.assert_redirect("/about/fields");
    assert!(
        !storage
            .exists(sample.photo.as_deref().unwrap())
            .await
            .unwrap()
    );
    assert!(!storage.exists(&manual_key).await.unwrap());
}

/// Files on S3: runs with `--features s3` and a bucket (CI starts SeaweedFS):
///
/// ```text
/// TEST_S3_ENDPOINT=http://127.0.0.1:8333 TEST_S3_BUCKET=renox-test \
///     TEST_S3_ACCESS_KEY_ID=renox TEST_S3_SECRET_ACCESS_KEY=renox-secret \
///     cargo test -p bikeshop --features s3 --test fields
/// ```
#[cfg(feature = "s3")]
mod on_s3 {
    use super::{FULL, PDF, PNG, only_sample};
    use renox::prelude::*;
    use renox::testing::TestApp;

    async fn s3_app() -> Option<TestApp> {
        let endpoint = std::env::var("TEST_S3_ENDPOINT").ok()?;
        let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"));
        let (bucket, id, secret) = (
            var("TEST_S3_BUCKET"),
            var("TEST_S3_ACCESS_KEY_ID"),
            var("TEST_S3_SECRET_ACCESS_KEY"),
        );
        let app = TestApp::with_config(bikeshop::app(), |c| {
            c.storage.disk = renox::storage::DiskDriver::S3;
            c.storage.endpoint = Some(endpoint.clone());
            c.storage.bucket = Some(bucket.clone());
            c.storage.region = Some("us-east-1".into());
            c.storage.access_key_id = Some(id);
            c.storage.secret_access_key = Some(secret);
            // Public files are read straight from the bucket.
            c.storage.url = Some(format!("{endpoint}/{bucket}"));
        })
        .await;
        let user = User::register(app.db(), "Rita", "rita@example.com", "password123")
            .await
            .unwrap();
        app.acting_as(&user);
        Some(app)
    }

    /// GET over plain HTTP (the test server speaks http): status and body.
    async fn http_get(url: &str) -> (u16, Vec<u8>) {
        use renox::tokio::io::{AsyncReadExt, AsyncWriteExt};
        let rest = url.strip_prefix("http://").expect("an http:// URL");
        let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let mut stream = renox::tokio::net::TcpStream::connect(host).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&response[..split]);
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        (status, response[split + 4..].to_vec())
    }

    #[renox::test]
    async fn photos_and_manuals_live_on_s3() {
        let Some(app) = s3_app().await else {
            return;
        };
        let res = app
            .post_multipart(
                "/about/fields",
                FULL,
                &[("photo", "bike.png", PNG), ("manual", "manual.pdf", PDF)],
            )
            .await;
        res.assert_status(303);
        let show = res.header("location").unwrap().to_owned();
        let sample = only_sample(&app).await;
        let storage = &app.state().storage;
        let manual = sample.manual.clone().unwrap();
        assert_eq!(storage.get(&manual).await.unwrap().as_deref(), Some(PDF));

        // The page shows the photo at its address on the bucket. (Whether
        // the bucket answers anonymous reads is the bucket's policy, which
        // CI's SeaweedFS doesn't set, so the file is read through Storage.)
        let photo = sample.photo.as_deref().unwrap();
        let url = storage.url(photo);
        assert!(url.starts_with("http://"), "{url}");
        app.get(&show).await.assert_see(&url);
        assert_eq!(storage.get(photo).await.unwrap().as_deref(), Some(PNG));
        // A presigned link to the private manual works without the app too…
        let link = storage
            .temporary_url(app.state(), &manual, std::time::Duration::from_secs(300))
            .await
            .unwrap();
        assert!(link.contains("X-Amz-Signature"), "{link}");
        assert_eq!(http_get(&link).await, (200, PDF.to_vec()));
        // …and the app sends it with its original name.
        let file = app.get(&format!("{show}/manual")).await;
        file.assert_ok();
        assert!(
            file.header("content-disposition")
                .unwrap_or_default()
                .contains("manual.pdf")
        );

        // Deleting the sample deletes the objects.
        app.delete(&show).await.assert_redirect("/about/fields");
        assert!(!storage.exists(&manual).await.unwrap());
        assert!(
            !storage
                .exists(sample.photo.as_deref().unwrap())
                .await
                .unwrap()
        );
    }
}
