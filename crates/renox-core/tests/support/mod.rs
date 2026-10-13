#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use renox_core::{App, Config, Environment};
use tempfile::TempDir;
use tower::ServiceExt;

/// An app under test that keeps its session cookie between requests, like a browser.
pub struct TestApp {
    router: Router,
    cookie: Option<String>,
    _dir: TempDir,
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl TestApp {
    pub async fn new(debug: bool, views: &[(&str, &str)], build: impl FnOnce(App) -> App) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (name, source) in views {
            let path = dir.path().join("views").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("public")).unwrap();
        std::fs::write(dir.path().join("public/robots.txt"), "User-agent: *").unwrap();

        let config = {
            let mut c = Config::default();
            c.name = "Test App".into();
            c.env = Environment::Testing;
            c.debug = debug;
            c.views_path = dir.path().join("views");
            c.public_path = dir.path().join("public");
            c.key = Some(renox_core::generate_key());
            c
        };
        let router = build(App::with_config(config)).into_router().await.unwrap();
        Self {
            router,
            cookie: None,
            _dir: dir,
        }
    }

    pub async fn send(&mut self, mut req: Request<Body>) -> TestResponse {
        if let Some(cookie) = &self.cookie {
            req.headers_mut().insert(COOKIE, cookie.parse().unwrap());
        }
        let res = self.router.clone().oneshot(req).await.unwrap();
        if let Some(set) = res.headers().get(SET_COOKIE) {
            let pair = set.to_str().unwrap().split(';').next().unwrap();
            self.cookie = Some(pair.to_owned());
        }
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        TestResponse {
            status,
            headers,
            body: String::from_utf8(body.to_vec()).unwrap(),
        }
    }

    pub async fn get(&mut self, uri: &str) -> TestResponse {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
            .await
    }

    pub async fn htmx_get(&mut self, uri: &str) -> TestResponse {
        self.send(
            Request::get(uri)
                .header("hx-request", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub async fn post_form(&mut self, uri: &str, body: &str) -> TestResponse {
        self.send(
            Request::post(uri)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
    }

    pub async fn htmx_post(&mut self, uri: &str, token: &str) -> TestResponse {
        self.send(
            Request::post(uri)
                .header("hx-request", "true")
                .header("x-csrf-token", token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub fn clear_cookies(&mut self) {
        self.cookie = None;
    }
}
