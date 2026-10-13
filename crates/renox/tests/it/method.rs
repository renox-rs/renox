//! Method spoofing: `_method` in forms (urlencoded and multipart) and the
//! `X-HTTP-Method-Override` header route a POST as PUT, PATCH or DELETE.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use renox::prelude::*;
use renox::testing::TestApp;
use tower::ServiceExt;

struct Items;

impl Module for Items {
    fn name(&self) -> &'static str {
        "items"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { view("form.html", ()) })
            .name("home")
            .get("/token", |session: Session| async move { session.token() })
            .post("/items/{id}", |Path(id): Path<i64>| async move {
                format!("post {id}")
            })
            .put("/items/{id}", |Path(id): Path<i64>| async move {
                format!("put {id}")
            })
            .patch("/items/{id}", |Path(id): Path<i64>| async move {
                format!("patch {id}")
            })
            .delete("/items/{id}", |Path(id): Path<i64>| async move {
                format!("delete {id}")
            })
    }
}

async fn app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("form.html"),
        "<form method=\"post\">{{ method_field('put') }}{{ method_field('<script>') }}</form>",
    )
    .unwrap();
    let views = dir.path().to_path_buf();
    let app = TestApp::with_config(App::new().module(Items), |c| c.views_path = views).await;
    (app, dir)
}

#[renox::test]
async fn forms_choose_put_patch_or_delete() {
    let (app, _dir) = app().await;
    let form = |method: &'static str| [("_method", method)];

    app.post("/items/7", &form("PUT"))
        .await
        .assert_ok()
        .assert_see("put 7");
    app.post("/items/7", &form("patch"))
        .await
        .assert_see("patch 7");
    app.post("/items/7", &form("DELETE"))
        .await
        .assert_see("delete 7");
    // Only those three: anything else stays a POST.
    app.post("/items/7", &form("GET"))
        .await
        .assert_see("post 7");
    app.post("/items/7", &[]).await.assert_see("post 7");
    app.request()
        .header("x-http-method-override", "DELETE")
        .post("/items/7", &[])
        .await
        .assert_see("delete 7");
    // CSRF still applies to the spoofed request.
    app.request()
        .without_csrf()
        .post("/items/7", &form("PUT"))
        .await
        .assert_status(419);
}

#[renox::test]
async fn multipart_forms_can_spoof_too() {
    let (app, _dir) = app().await;
    let router = app.kernel().router();
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
    let token =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();

    let body = format!(
        "--b\r\nContent-Disposition: form-data; name=\"_token\"\r\n\r\n{token}\r\n\
         --b\r\nContent-Disposition: form-data; name=\"_method\"\r\n\r\nDELETE\r\n--b--\r\n"
    );
    let req = Request::post("/items/3")
        .header("content-type", "multipart/form-data; boundary=b")
        .header("cookie", cookie)
        .body(Body::from(body))
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    let status = res.status();
    let text =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert_eq!((status, text.as_str()), (StatusCode::OK, "delete 3"));
}

#[renox::test]
async fn method_field_writes_the_hidden_input() {
    let (app, _dir) = app().await;
    app.get("/")
        .await
        .assert_see(r#"<input type="hidden" name="_method" value="PUT">"#)
        .assert_see(r#"<input type="hidden" name="_method" value="SCRIPT">"#);
}
