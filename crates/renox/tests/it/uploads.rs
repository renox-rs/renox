use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use renox::Kernel;
use renox::prelude::*;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDRsmall-but-valid-enough-for-sniffing";

#[derive(Deserialize, Serialize)]
struct ProductForm {
    name: String,
    photo: Option<Upload>,
}

impl Validate for ProductForm {
    fn rules(&self, v: &mut Validator) {
        v.field("name", &self.name).required();
        v.field("photo", &self.photo).image().max(2);
    }
}

#[derive(Deserialize)]
struct DocumentForm {
    document: Upload,
}

impl Validate for DocumentForm {
    fn rules(&self, v: &mut Validator) {
        v.field("document", &self.document)
            .required()
            .mimes(&["pdf"]);
    }
}

struct Shop;

impl Module for Shop {
    fn name(&self) -> &'static str {
        "shop"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { "home" })
            .name("home")
            .get("/token", |session: Session| async move { session.token() })
            .get("/products/create", || async { view("create.html", ()) })
            .post("/products", store)
            .post(
                "/documents",
                |Valid(form): Valid<DocumentForm>| async move { form.document.file_name().to_owned() },
            )
    }
}

async fn store(State(state): State<AppState>, Valid(form): Valid<ProductForm>) -> Result<String> {
    let url = match &form.photo {
        Some(photo) => state
            .storage
            .url(&photo.store_public(&state.storage, "products").await?),
        None => "no photo".into(),
    };
    Ok(format!("{} {url}", form.name))
}

async fn kernel(dir: &std::path::Path, upload_max_size: usize) -> Kernel {
    std::fs::create_dir_all(dir.join("views")).unwrap();
    std::fs::write(
        dir.join("views/create.html"),
        r#"<input name="name" value="{{ old('name') }}">{{ error('photo') }}"#,
    )
    .unwrap();
    let config = {
        let mut c = Config::default();
        c.env = Environment::Testing;
        c.key = Some(renox::generate_key());
        c.views_path = dir.join("views");
        c.storage_path = dir.join("storage");
        c.upload_max_size = upload_max_size;
        c
    };
    let kernel = App::with_config(config).module(Shop).boot().await.unwrap();
    kernel.migrate().await.unwrap();
    kernel
}

enum Part<'a> {
    Text(&'a str),
    File(&'a str, &'a [u8]),
}

const BOUNDARY: &str = "renox-test-boundary";

fn multipart(parts: &[(&str, Part)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, part) in parts {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        match part {
            Part::Text(value) => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                        .as_bytes(),
                );
            }
            Part::File(file_name, bytes) => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
                );
                body.extend_from_slice(bytes);
                body.extend_from_slice(b"\r\n");
            }
        }
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

struct Browser {
    router: axum::Router,
    cookie: std::sync::Mutex<String>,
    token: String,
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Bytes,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

impl Browser {
    async fn new(kernel: &Kernel) -> Self {
        let router = kernel.router();
        let res = router
            .clone()
            .oneshot(Request::get("/token").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let cookie = res.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let token = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec())
            .unwrap();
        Self {
            router,
            cookie: std::sync::Mutex::new(cookie),
            token,
        }
    }

    /// Sends the session cookie and keeps the one the response sets, like a browser.
    async fn send(&self, req: axum::http::request::Builder, body: Vec<u8>) -> Reply {
        let cookie = self.cookie.lock().unwrap().clone();
        let req = req.header("cookie", cookie).body(Body::from(body)).unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        if let Some(set) = res.headers().get("set-cookie") {
            let pair = set.to_str().unwrap().split(';').next().unwrap();
            *self.cookie.lock().unwrap() = pair.to_owned();
        }
        let (status, headers) = (res.status(), res.headers().clone());
        Reply {
            status,
            headers,
            body: res.into_body().collect().await.unwrap().to_bytes(),
        }
    }

    /// An HTMX upload: CSRF token in the header.
    async fn upload(&self, uri: &str, parts: &[(&str, Part<'_>)]) -> Reply {
        let req = Request::post(uri)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .header("hx-request", "true")
            .header("x-csrf-token", &self.token);
        self.send(req, multipart(parts)).await
    }

    /// A plain form upload: CSRF token as a field.
    async fn form_upload(&self, uri: &str, token: &str, parts: &[(&str, Part<'_>)]) -> Reply {
        let mut all = vec![("_token", Part::Text(token))];
        all.extend(parts.iter().map(|(n, p)| {
            (
                *n,
                match p {
                    Part::Text(t) => Part::Text(t),
                    Part::File(f, b) => Part::File(f, b),
                },
            )
        }));
        let req = Request::post(uri)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .header("referer", "/products/create");
        self.send(req, multipart(&all)).await
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Request::get(uri), Vec::new()).await
    }
}

#[tokio::test]
async fn images_are_uploaded_stored_and_served() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel(dir.path(), 1024 * 1024).await;
    let browser = Browser::new(&kernel).await;

    let reply = browser
        .upload(
            "/products",
            &[
                ("name", Part::Text("Coffee")),
                ("photo", Part::File("coffee.PNG", PNG)),
            ],
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text());
    let text = reply.text();
    let url = text.strip_prefix("Coffee ").unwrap().to_owned();
    assert!(
        url.starts_with("/storage/products/") && url.ends_with(".png"),
        "{url}"
    );

    let served = browser.get(&url).await;
    assert_eq!(served.status, StatusCode::OK);
    assert_eq!(&served.body[..], PNG);
    assert!(
        served.headers.get("set-cookie").is_none(),
        "public files skip the session"
    );

    let key = format!("public/{}", url.trim_start_matches("/storage/"));
    assert!(kernel.state().storage.exists(&key).await.unwrap());

    let no_photo = browser
        .upload(
            "/products",
            &[("name", Part::Text("Tea")), ("photo", Part::File("", b""))],
        )
        .await;
    assert_eq!(
        no_photo.text(),
        "Tea no photo",
        "an empty file input is no file"
    );
}

#[tokio::test]
async fn file_rules_check_content_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel(dir.path(), 1024 * 1024).await;
    let browser = Browser::new(&kernel).await;
    let errors = |reply: Reply| -> serde_json::Value {
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            reply.text()
        );
        serde_json::from_slice::<serde_json::Value>(&reply.body).unwrap()["errors"].clone()
    };

    let disguised = browser
        .upload(
            "/products",
            &[
                ("name", Part::Text("x")),
                ("photo", Part::File("photo.png", b"MZ not an image")),
            ],
        )
        .await;
    assert_eq!(errors(disguised)["photo"][0], "The photo must be an image.");

    let big = [PNG, &[0u8; 4096]].concat();
    let too_big = browser
        .upload(
            "/products",
            &[
                ("name", Part::Text("x")),
                ("photo", Part::File("big.png", &big)),
            ],
        )
        .await;
    assert_eq!(
        errors(too_big)["photo"][0],
        "The photo may not be greater than 2 kilobytes."
    );

    let text_instead = browser
        .upload(
            "/products",
            &[("name", Part::Text("x")), ("photo", Part::Text("hello"))],
        )
        .await;
    assert_eq!(
        errors(text_instead)["photo"][0],
        "The photo must be a file."
    );

    let wrong_type = browser
        .upload("/documents", &[("document", Part::File("scan.pdf", PNG))])
        .await;
    assert_eq!(
        errors(wrong_type)["document"][0],
        "The document must be a file of type: pdf.",
        "the content wins over the name"
    );
    let missing = browser
        .upload("/documents", &[("document", Part::File("", b""))])
        .await;
    assert_eq!(
        errors(missing)["document"][0],
        "The document field is required."
    );
    let pdf = browser
        .upload(
            "/documents",
            &[("document", Part::File("report.pdf", b"%PDF-1.7 ..."))],
        )
        .await;
    assert_eq!(pdf.text(), "report.pdf");
}

#[tokio::test]
async fn plain_multipart_forms_carry_the_csrf_token_as_a_field() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel(dir.path(), 1024 * 1024).await;
    let browser = Browser::new(&kernel).await;

    let ok = browser
        .form_upload(
            "/products",
            &browser.token,
            &[("name", Part::Text("Coffee"))],
        )
        .await;
    assert_eq!(ok.text(), "Coffee no photo");
    let forged = browser
        .form_upload("/products", "wrong", &[("name", Part::Text("Coffee"))])
        .await;
    assert_eq!(forged.status.as_u16(), 419);

    let invalid = browser
        .form_upload(
            "/products",
            &browser.token,
            &[
                ("name", Part::Text("Coffee")),
                ("photo", Part::File("x.png", b"nope")),
            ],
        )
        .await;
    assert_eq!(invalid.status, StatusCode::SEE_OTHER);
    let page = browser.get("/products/create").await.text();
    assert!(
        page.contains(r#"value="Coffee""#) && page.contains("The photo must be an image."),
        "{page}"
    );
}

#[tokio::test]
async fn large_bodies_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel(dir.path(), 4 * 1024).await;
    let browser = Browser::new(&kernel).await;
    let big = vec![b'x'; 8 * 1024];
    let reply = browser
        .upload(
            "/products",
            &[
                ("name", Part::Text("x")),
                ("photo", Part::File("big.bin", &big)),
            ],
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        reply.text()
    );
}

#[tokio::test]
async fn private_files_need_a_temporary_url() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = kernel(dir.path(), 1024 * 1024).await;
    let (state, storage) = (kernel.state(), &kernel.state().storage);
    let browser = Browser::new(&kernel).await;

    storage
        .put(
            "invoices/2026/1.pdf",
            Bytes::from_static(b"%PDF-1.7 invoice"),
        )
        .await
        .unwrap();
    assert_eq!(
        storage.get("invoices/2026/1.pdf").await.unwrap().unwrap(),
        &b"%PDF-1.7 invoice"[..]
    );
    assert_eq!(
        browser.get("/storage/invoices/2026/1.pdf").await.status,
        StatusCode::NOT_FOUND,
        "not public"
    );

    let url = storage
        .temporary_url(state, "invoices/2026/1.pdf", Duration::from_secs(60))
        .await
        .unwrap();
    let path = url.trim_start_matches("http://127.0.0.1:3000");
    let reply = browser.get(path).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers["content-type"], "application/pdf");
    assert_eq!(
        browser.get(&path.replace("1.pdf", "2.pdf")).await.status,
        StatusCode::FORBIDDEN,
        "signature covers the key"
    );

    for bad in ["../secrets", "a/../../b", ""] {
        assert!(storage.put(bad, Bytes::new()).await.is_err(), "{bad}");
    }
    storage.delete("invoices/2026/1.pdf").await.unwrap();
    storage.delete("invoices/2026/1.pdf").await.unwrap();
    assert!(!storage.exists("invoices/2026/1.pdf").await.unwrap());
}

#[cfg(not(feature = "s3"))]
#[tokio::test]
async fn s3_needs_the_feature() {
    let dir = tempfile::tempdir().unwrap();
    let config = {
        let mut c = Config::default();
        c.storage.disk = renox::storage::DiskDriver::S3;
        c
    };
    let _ = dir;
    let err = App::with_config(config).boot().await.err().unwrap();
    assert!(format!("{err:?}").contains("`s3` feature"), "{err:?}");
}
