//! M15b: more validation rules, lists of values and files, cookies and
//! downloads.

use std::time::Duration;

use renox::prelude::*;
use renox::testing::TestApp;
use renox::validation::{Inspected, Rule};
use renox::{Cookies, Download, SetCookie};
use serde::Deserialize;

struct TaxId;

impl Rule for TaxId {
    fn check(&self, value: &Inspected) -> std::result::Result<(), String> {
        let Inspected::Text(text) = value else {
            return Ok(());
        };
        let digits = text.chars().filter(char::is_ascii_digit).count();
        if digits == 15 || digits == 16 {
            Ok(())
        } else {
            Err("The :attribute must be a valid tax ID.".into())
        }
    }
}

#[derive(Deserialize)]
struct Signup {
    code: String,
    pin: String,
    phone: String,
    kind: String,
    company: Option<String>,
    birthday: renox::chrono::NaiveDate,
    starts: String,
    ends: String,
    username: String,
    email: String,
    email_again: String,
    tax_id: String,
    #[serde(default)]
    tags: Vec<String>,
}

impl Validate for Signup {
    fn rules(&self, v: &mut Validator) {
        let today = renox::chrono::NaiveDate::from_ymd_opt(2026, 9, 28).unwrap();
        v.field("code", &self.code).matches(r"^[A-Z]{2}\d{4}$");
        v.field("pin", &self.pin).digits(6);
        v.field("phone", &self.phone).digits_between(10, 13);
        v.field("company", &self.company)
            .required_if(self.kind == "company");
        v.field("birthday", &self.birthday).before(today);
        v.field("starts", &self.starts).date().after_or_equal(today);
        let starts = renox::chrono::NaiveDate::parse_from_str(&self.starts, "%Y-%m-%d").ok();
        v.field("ends", &self.ends)
            .date()
            .after(starts.unwrap_or(today));
        v.field("username", &self.username)
            .none_of(&["admin", "root"]);
        v.field("email_again", &self.email_again)
            .same("email", &self.email);
        v.field("tax_id", &self.tax_id).apply(&TaxId);
        v.field("tags", &self.tags).max(3);
        v.each("tags", &self.tags, |tag| tag.max(5));
    }
}

#[derive(Deserialize)]
struct Line {
    sku: String,
    qty: i64,
}

impl Validate for Line {
    fn rules(&self, v: &mut Validator) {
        v.field("sku", &self.sku).required();
        v.field("qty", &self.qty).min(1);
    }
}

#[derive(Deserialize)]
struct Order {
    lines: Vec<Line>,
}

impl Validate for Order {
    fn rules(&self, v: &mut Validator) {
        v.field("lines", &self.lines).required();
        v.nested("lines", &self.lines);
    }
}

#[derive(Deserialize)]
struct Photos {
    #[serde(default)]
    photos: Vec<Upload>,
}

impl Validate for Photos {
    fn rules(&self, v: &mut Validator) {
        v.field("photos", &self.photos).required().max(3);
        v.each("photos", &self.photos, |photo| photo.image());
    }
}

struct Pages;

impl Module for Pages {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .post("/signup", |Valid(_): Valid<Signup>| async { "ok" })
            .post("/orders", |Valid(order): Valid<Order>| async move {
                order.lines.len().to_string()
            })
            .post("/photos", |Valid(p): Valid<Photos>| async move {
                p.photos
                    .iter()
                    .map(|f| f.file_name().to_owned())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .get("/cookies/set", |State(state): State<AppState>| async move {
                (
                    SetCookie::new(&state, "theme", "dark").max_age(Duration::from_secs(3600)),
                    SetCookie::encrypted(&state, "ref", "partner-7"),
                    "set",
                )
            })
            .get("/cookies/read", |cookies: Cookies| async move {
                format!(
                    "{}|{}",
                    cookies.get("theme").unwrap_or_default(),
                    cookies
                        .get_encrypted("ref")
                        .unwrap_or_else(|| "none".into())
                )
            })
            .get(
                "/cookies/forget",
                |State(state): State<AppState>| async move {
                    (SetCookie::remove(&state, "theme"), "gone")
                },
            )
            .get("/dl/bytes", || async {
                Download::bytes("invoice May.pdf", "application/pdf", b"%PDF".to_vec())
            })
            .get("/dl/inline", || async {
                Download::bytes("invoice.pdf", "application/pdf", b"%PDF".to_vec()).inline()
            })
            .get("/dl/html", || async {
                Download::bytes("page.html", "text/html", b"<script>".to_vec()).inline()
            })
            .get("/dl/file", |State(state): State<AppState>| async move {
                Download::file(state.config.storage_path.join("report.csv"), "report.csv").await
            })
            .get("/dl/missing", || async {
                Download::file("/no/such/file.csv", "x.csv").await
            })
            .get("/dl/storage", |State(state): State<AppState>| async move {
                Download::from_storage(&state.storage, "exports/data.json", "data.json").await
            })
            .get("/dl/stream", || async {
                let rows = ["id,name\n", "1,Coffee\n", "2,Tea\n"]
                    .map(|line| Ok::<_, std::io::Error>(renox::axum::body::Bytes::from(line)));
                Download::stream("items.csv", "text/csv", futures_util::stream::iter(rows))
            })
    }
}

async fn app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = dir.path().to_path_buf();
    let app =
        TestApp::with_config(App::new().module(Pages), move |c| c.storage_path = storage).await;
    (app, dir)
}

fn signup(overrides: &[(&str, &str)]) -> renox::serde_json::Value {
    let mut form = json!({
        "code": "AB1234", "pin": "123456", "phone": "081234567890", "kind": "person",
        "birthday": "1990-05-01", "starts": "2026-10-01", "ends": "2026-10-05",
        "username": "alex", "email": "a@b.co", "email_again": "a@b.co",
        "tax_id": "01.234.567.8-901.000", "tags": ["latte", "tea"],
    });
    for (key, value) in overrides {
        form[*key] = json!(value);
    }
    form
}

#[renox::test]
async fn more_rules_pass_and_fail_one_by_one() {
    let (app, _dir) = app().await;
    app.post_json("/signup", &signup(&[]))
        .await
        .assert_ok()
        .assert_see("ok");
    for (field, value) in [
        ("code", "ab1234"),
        ("pin", "12345"),
        ("pin", "12345a"),
        ("phone", "0812"),
        ("birthday", "2030-01-01"),
        ("starts", "2026-09-01"),
        ("starts", "not a date"),
        ("ends", "2026-10-01"),
        ("username", "admin"),
        ("email_again", "c@d.co"),
        ("tax_id", "123"),
    ] {
        let res = app.post_json("/signup", &signup(&[(field, value)])).await;
        res.assert_invalid(field);
    }
    let res = app
        .post_json("/signup", &signup(&[("kind", "company")]))
        .await;
    res.assert_invalid("company");
    let body: renox::serde_json::Value = res.json();
    assert_eq!(
        body["errors"]["company"][0],
        "The company field is required."
    );

    let res = app
        .post_json("/signup", &signup(&[("email_again", "x@y.co")]))
        .await;
    let body: renox::serde_json::Value = res.json();
    assert_eq!(
        body["errors"]["email_again"][0],
        "The email again and email must match."
    );
    let res = app.post_json("/signup", &signup(&[("tax_id", "1")])).await;
    let body: renox::serde_json::Value = res.json();
    assert_eq!(
        body["errors"]["tax_id"][0],
        "The tax id must be a valid tax ID."
    );
    let res = app
        .post_json("/signup", &signup(&[("ends", "2026-09-30")]))
        .await;
    let body: renox::serde_json::Value = res.json();
    assert_eq!(
        body["errors"]["ends"][0],
        "The ends must be a date after 2026-10-01."
    );
}

#[renox::test]
async fn each_item_and_nested_structs_are_validated() {
    let (app, _dir) = app().await;
    let mut form = signup(&[]);
    form["tags"] = json!(["latte", "espresso"]);
    let res = app.post_json("/signup", &form).await;
    res.assert_invalid("tags.1");
    let body: renox::serde_json::Value = res.json();
    assert_eq!(
        body["errors"]["tags.1"][0],
        "The tags #2 may not be longer than 5 characters."
    );
    form["tags"] = json!(["a", "b", "c", "d"]);
    app.post_json("/signup", &form).await.assert_invalid("tags");

    app.post_json("/orders", &json!({ "lines": [{ "sku": "K1", "qty": 2 }] }))
        .await
        .assert_see("1");
    let res = app
        .post_json(
            "/orders",
            &json!({ "lines": [{ "sku": "K1", "qty": 2 }, { "sku": "", "qty": 0 }] }),
        )
        .await;
    res.assert_invalid("lines.1.sku")
        .assert_invalid("lines.1.qty");
    app.post_json("/orders", &json!({ "lines": [] }))
        .await
        .assert_invalid("lines");
}

#[renox::test]
async fn several_files_in_one_field() {
    let (app, _dir) = app().await;
    let png: &[u8] =
        b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89";
    let res = app
        .htmx()
        .post_multipart(
            "/photos",
            &[],
            &[("photos", "a.png", png), ("photos", "b.png", png)],
        )
        .await;
    res.assert_ok().assert_see("a.png,b.png");
    let res = app
        .htmx()
        .post_multipart(
            "/photos",
            &[],
            &[("photos", "a.png", png), ("photos", "notes.txt", b"hello")],
        )
        .await;
    res.assert_invalid("photos.1");
    app.htmx()
        .post_multipart("/photos", &[], &[])
        .await
        .assert_invalid("photos");
}

#[renox::test]
async fn cookies_plain_encrypted_and_removed() {
    let (app, _dir) = app().await;
    let res = app.get("/cookies/set").await;
    let set: Vec<String> = res
        .headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .filter(|c| !c.starts_with("renox_session"))
        .collect();
    assert_eq!(set.len(), 2, "{set:?}");
    let theme = set.iter().find(|c| c.starts_with("theme=")).unwrap();
    assert!(
        theme.contains("HttpOnly")
            && theme.contains("SameSite=Lax")
            && theme.contains("Max-Age=3600"),
        "{theme}"
    );
    assert!(!theme.contains("Secure"), "not on http");
    let referral = set.iter().find(|c| c.starts_with("ref=")).unwrap();
    assert!(!referral.contains("partner-7"), "encrypted: {referral}");

    let pair = |c: &str| c.split(';').next().unwrap().to_owned();
    let cookie = format!("{}; {}", pair(theme), pair(referral));
    app.request()
        .header("cookie", &cookie)
        .get("/cookies/read")
        .await
        .assert_see("dark|partner-7");
    // A changed encrypted value reads as missing.
    let forged = format!("{}; ref=partner-7", pair(theme));
    app.request()
        .header("cookie", &forged)
        .get("/cookies/read")
        .await
        .assert_see("dark|none");

    let gone = app.get("/cookies/forget").await;
    let removal = gone.header("set-cookie").unwrap_or_default().to_owned();
    let all: Vec<String> = gone
        .headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    assert!(
        all.iter()
            .any(|c| c.starts_with("theme=") && c.contains("Max-Age=0")),
        "{all:?} {removal}"
    );
}

#[renox::test]
async fn downloads_set_safe_headers() {
    let (app, dir) = app().await;
    let res = app.get("/dl/bytes").await;
    res.assert_ok()
        .assert_header("content-type", "application/pdf")
        .assert_header("content-length", "4")
        .assert_header("x-content-type-options", "nosniff");
    assert_eq!(
        res.header("content-disposition"),
        Some("attachment; filename=\"invoice May.pdf\"; filename*=UTF-8''invoice%20May.pdf")
    );
    assert!(
        app.get("/dl/inline")
            .await
            .header("content-disposition")
            .unwrap()
            .starts_with("inline;")
    );
    assert!(
        app.get("/dl/html")
            .await
            .header("content-disposition")
            .unwrap()
            .starts_with("attachment;"),
        "HTML is never shown inline"
    );

    std::fs::write(dir.path().join("report.csv"), "id,total\n1,75000\n").unwrap();
    let res = app.get("/dl/file").await;
    res.assert_ok()
        .assert_header("content-type", "text/csv")
        .assert_header("content-length", "17");
    assert_eq!(res.text(), "id,total\n1,75000\n");
    app.get("/dl/missing").await.assert_not_found();

    app.get("/dl/storage").await.assert_not_found();
    app.state()
        .storage
        .put(
            "exports/data.json",
            renox::axum::body::Bytes::from_static(b"{\"ok\":true}"),
        )
        .await
        .unwrap();
    let res = app.get("/dl/storage").await;
    res.assert_ok()
        .assert_header("content-type", "application/json");
    assert_eq!(res.text(), "{\"ok\":true}");

    let res = app.get("/dl/stream").await;
    res.assert_ok().assert_header("content-type", "text/csv");
    assert!(res.header("content-length").is_none());
    assert_eq!(res.text(), "id,name\n1,Coffee\n2,Tea\n");
}
