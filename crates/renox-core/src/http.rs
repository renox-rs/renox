//! An HTTP client for calling other services (payment gateways, shipping
//! rates, webhooks you send), with timeouts, retries and a fake for tests.
//!
//! ```
//! # use renox::prelude::*;
//! # use std::time::Duration;
//! #[derive(serde::Deserialize)]
//! struct Rates { eur: f64 }
//!
//! # async fn demo(state: AppState, token: &str) -> Result {
//! let rates: Rates = state
//!     .http
//!     .get("https://api.example.com/rates")
//!     .query(&[("base", "USD")])
//!     .bearer(token)
//!     .timeout(Duration::from_secs(5))
//!     .retry(3, Duration::from_millis(200)) // on connection errors, 429 and 5xx
//!     .send()
//!     .await?
//!     .error_for_status()?                   // a 4xx/5xx becomes an error
//!     .json()?;
//!
//! let created = state
//!     .http
//!     .post("https://api.example.com/orders")
//!     .json(&renox::serde_json::json!({ "total": 7_500 }))
//!     .send()
//!     .await?;
//! assert!(created.ok());
//! # let _ = rates; Ok(()) }
//! ```
//!
//! In tests, [`TestApp::fake_http`](crate::testing::TestApp::fake_http)
//! answers requests instead of the network and records them:
//!
//! ```
//! # use renox::prelude::*;
//! use renox::http::FakeResponse;
//! # async fn demo(app: renox::testing::TestApp) {
//! let http = app.fake_http();
//! http.on("https://api.example.com/rates*", FakeResponse::json(200, json!({ "eur": 0.92 })));
//! http.on("POST https://api.example.com/orders", FakeResponse::status(503)); // then…
//! http.on("POST https://api.example.com/orders", FakeResponse::json(201, json!({ "id": 7 })));
//! // … exercise the app …
//! http.assert_sent(|r| r.method == "POST" && r.body.contains("7500"));
//! # }
//! ```
//!
//! Requests with no fake fail while faking, so a test never reaches the
//! network by accident. Real requests need the `http` feature (on by
//! default).

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{Error, Result};

/// How long a request may take unless it says otherwise.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// The app's HTTP client (`state.http`).
#[derive(Clone, Default)]
pub struct Http {
    fake: Arc<Mutex<Option<Arc<Mutex<FakeState>>>>>,
}

impl fmt::Debug for Http {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Http")
    }
}

impl Http {
    /// A `GET` request to `url`.
    pub fn get(&self, url: impl Into<String>) -> Request {
        self.request(Method::GET, url)
    }

    /// A `POST` request to `url`.
    pub fn post(&self, url: impl Into<String>) -> Request {
        self.request(Method::POST, url)
    }

    /// A `PUT` request to `url`.
    pub fn put(&self, url: impl Into<String>) -> Request {
        self.request(Method::PUT, url)
    }

    /// A `PATCH` request to `url`.
    pub fn patch(&self, url: impl Into<String>) -> Request {
        self.request(Method::PATCH, url)
    }

    /// A `DELETE` request to `url`.
    pub fn delete(&self, url: impl Into<String>) -> Request {
        self.request(Method::DELETE, url)
    }

    /// A request with any method; 30 s timeout and no retries unless changed.
    pub fn request(&self, method: Method, url: impl Into<String>) -> Request {
        Request {
            http: self.clone(),
            method,
            url: url.into(),
            query: Vec::new(),
            headers: Vec::new(),
            body: None,
            timeout: DEFAULT_TIMEOUT,
            retries: 0,
            retry_delay: Duration::from_millis(100),
            error: None,
        }
    }

    /// Answers requests with fakes from now on (see [`FakeHttp`]); the
    /// same fake for every clone of this client.
    pub fn fake(&self) -> FakeHttp {
        let mut slot = self.fake.lock().unwrap_or_else(|e| e.into_inner());
        let state = slot.get_or_insert_with(Arc::default).clone();
        FakeHttp(state)
    }

    fn faked(&self) -> Option<Arc<Mutex<FakeState>>> {
        self.fake.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// A request being built; send it with [`Request::send`].
#[must_use = "a request does nothing until sent"]
pub struct Request {
    http: Http,
    method: Method,
    url: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Option<(String, Vec<u8>)>,
    timeout: Duration,
    retries: u32,
    retry_delay: Duration,
    error: Option<Error>,
}

impl Request {
    /// Adds query parameters (encoded).
    pub fn query(mut self, pairs: &[(&str, &str)]) -> Self {
        self.query.extend(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        self
    }

    /// Adds a header (repeatable: a name may be sent more than once).
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }

    /// `Authorization: Bearer <token>`.
    pub fn bearer(self, token: &str) -> Self {
        self.header("authorization", format!("Bearer {token}"))
    }

    /// `Authorization: Basic …`.
    pub fn basic_auth(self, user: &str, password: &str) -> Self {
        use base64::Engine;
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
        self.header("authorization", format!("Basic {encoded}"))
    }

    /// A JSON body (`Content-Type: application/json`).
    pub fn json(mut self, body: &impl Serialize) -> Self {
        match serde_json::to_vec(body) {
            Ok(bytes) => self.body = Some(("application/json".into(), bytes)),
            Err(err) => self.error = Some(err.into()),
        }
        self
    }

    /// A form body (`application/x-www-form-urlencoded`).
    pub fn form(mut self, body: &impl Serialize) -> Self {
        match serde_urlencoded::to_string(body) {
            Ok(text) => {
                self.body = Some((
                    "application/x-www-form-urlencoded".into(),
                    text.into_bytes(),
                ));
            }
            Err(err) => self.error = Some(anyhow::Error::new(err).into()),
        }
        self
    }

    /// A raw body with its content type.
    pub fn body(mut self, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some((content_type.to_owned(), body.into()));
        self
    }

    /// How long one attempt may take (30 s by default).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Tries again up to `times` more times after a connection error, a
    /// timeout, a 429 or a 5xx, waiting `delay`, then twice that, and so on.
    pub fn retry(mut self, times: u32, delay: Duration) -> Self {
        self.retries = times;
        self.retry_delay = delay;
        self
    }

    /// The URL with its query parameters.
    fn full_url(&self) -> Result<String> {
        if self.query.is_empty() {
            return Ok(self.url.clone());
        }
        let query = serde_urlencoded::to_string(&self.query).map_err(anyhow::Error::new)?;
        let joiner = if self.url.contains('?') { '&' } else { '?' };
        Ok(format!("{}{joiner}{query}", self.url))
    }

    /// Sends the request (with its retries) and reads the whole response.
    /// Any status is a response; see [`Response::error_for_status`].
    pub async fn send(mut self) -> Result<Response> {
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        let url = self.full_url()?;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let outcome = match self.http.faked() {
                Some(fake) => fake_send(&fake, &self, &url),
                None => self.send_once(&url).await,
            };
            let retryable = match &outcome {
                Ok(response) => {
                    response.status == StatusCode::TOO_MANY_REQUESTS
                        || response.status.is_server_error()
                }
                Err(_) => true,
            };
            if !retryable || attempt > self.retries {
                return outcome;
            }
            tracing::debug!(url = %url, attempt, "HTTP request failed, retrying");
            tokio::time::sleep(self.retry_delay * attempt).await;
        }
    }

    #[cfg(feature = "http")]
    async fn send_once(&self, url: &str) -> Result<Response> {
        let mut request = client()
            .request(self.method.clone(), url)
            .timeout(self.timeout);
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }
        if let Some((content_type, body)) = &self.body {
            request = request
                .header("content-type", content_type)
                .body(body.clone());
        }
        let response = request
            .send()
            .await
            .map_err(|err| anyhow::anyhow!("{} {url}: {}", self.method, err_chain(&err)))?;
        let status = response.status();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_owned(),
                    v.to_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let body = response
            .bytes()
            .await
            .map_err(|err| anyhow::anyhow!("{} {url}: {}", self.method, err_chain(&err)))?;
        Ok(Response {
            status,
            headers,
            body: body.to_vec(),
            url: url.to_owned(),
        })
    }

    #[cfg(not(feature = "http"))]
    async fn send_once(&self, url: &str) -> Result<Response> {
        Err(anyhow::anyhow!(
            "{} {url}: Renox was built without the `http` feature",
            self.method
        )
        .into())
    }
}

#[cfg(feature = "http")]
fn err_chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// One client for the process: it keeps connections open between requests.
#[cfg(feature = "http")]
pub(crate) fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
        // reqwest is built without a crypto provider (so aws-lc isn't
        // compiled); use ring, like the mailer. Fails only if one is installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .user_agent(concat!("renox/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default()
    });
    &CLIENT
}

/// A response, read whole.
#[derive(Debug, Clone)]
pub struct Response {
    status: StatusCode,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    url: String,
}

impl Response {
    /// The response's status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// A 2xx status.
    pub fn ok(&self) -> bool {
        self.status.is_success()
    }

    /// The first header `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The body as raw bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    /// The body as text; invalid UTF-8 becomes U+FFFD.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The body as JSON into `T`.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|err| {
            anyhow::anyhow!(
                "the response of {} is not the expected JSON: {err}",
                self.url
            )
            .into()
        })
    }

    /// The response, or an error for a 4xx or 5xx status (with the start
    /// of the body, which usually says why).
    pub fn error_for_status(self) -> Result<Self> {
        if self.status.is_client_error() || self.status.is_server_error() {
            let body = self.text();
            let excerpt: String = body.chars().take(300).collect();
            return Err(anyhow::anyhow!("{} answered {}: {excerpt}", self.url, self.status).into());
        }
        Ok(self)
    }
}

/// A request the fake received.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SentRequest {
    /// The HTTP method, upper case (`"POST"`).
    pub method: String,
    /// With its query string.
    pub url: String,
    /// The headers sent, including `content-type` when the request had a body.
    pub headers: Vec<(String, String)>,
    /// The body as text (lossy UTF-8); empty when there was none.
    pub body: String,
}

impl SentRequest {
    /// The first header `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The body as JSON.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_default()
    }
}

/// A canned answer for [`FakeHttp::on`].
#[derive(Debug, Clone)]
pub struct FakeResponse {
    status: StatusCode,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// Fail as if the connection broke.
    fails: bool,
}

impl FakeResponse {
    /// An empty response with this status (500 if the code is invalid).
    pub fn status(status: u16) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            headers: Vec::new(),
            body: Vec::new(),
            fails: false,
        }
    }

    /// A JSON response with `content-type: application/json`.
    pub fn json(status: u16, body: serde_json::Value) -> Self {
        Self::status(status)
            .header("content-type", "application/json")
            .body(body.to_string())
    }

    /// A plain-text response with this status and body.
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self::status(status).body(body.into())
    }

    /// A connection error instead of a response.
    pub fn connection_error() -> Self {
        Self {
            fails: true,
            ..Self::status(500)
        }
    }

    /// Adds a response header.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Sets the response body.
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
}

#[derive(Default)]
struct FakeState {
    /// Pattern and its answers, used in order; the last one repeats.
    routes: Vec<(String, Vec<FakeResponse>)>,
    sent: Vec<SentRequest>,
}

/// The fake behind `state.http` in tests; from [`Http::fake`] or
/// `TestApp::fake_http`.
#[derive(Clone)]
pub struct FakeHttp(Arc<Mutex<FakeState>>);

impl FakeHttp {
    /// Answers requests matching `pattern` with `response`. The pattern is a
    /// URL where `*` matches anything, optionally after a method
    /// (`"POST https://api.example.com/orders"`). Several answers for one
    /// pattern are given in turn; the last one repeats.
    pub fn on(&self, pattern: &str, response: FakeResponse) -> &Self {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match state.routes.iter_mut().find(|(p, _)| p == pattern) {
            Some((_, answers)) => answers.push(response),
            None => state.routes.push((pattern.to_owned(), vec![response])),
        }
        self
    }

    /// The requests sent so far, oldest first.
    pub fn sent(&self) -> Vec<SentRequest> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sent
            .clone()
    }

    /// Panics unless a sent request matches.
    #[track_caller]
    pub fn assert_sent(&self, check: impl Fn(&SentRequest) -> bool) {
        let sent = self.sent();
        assert!(
            sent.iter().any(check),
            "no matching HTTP request was sent; sent: {:#?}",
            sent.iter()
                .map(|r| format!("{} {}", r.method, r.url))
                .collect::<Vec<_>>()
        );
    }

    /// Panics if a sent request matches.
    #[track_caller]
    pub fn assert_not_sent(&self, check: impl Fn(&SentRequest) -> bool) {
        assert!(
            !self.sent().iter().any(check),
            "a matching HTTP request was sent"
        );
    }

    /// Panics unless exactly `expected` requests were sent.
    #[track_caller]
    pub fn assert_sent_count(&self, expected: usize) {
        assert_eq!(self.sent().len(), expected, "HTTP requests sent");
    }
}

fn fake_send(fake: &Mutex<FakeState>, request: &Request, url: &str) -> Result<Response> {
    let mut state = fake.lock().unwrap_or_else(|e| e.into_inner());
    let mut headers = request.headers.clone();
    if let Some((content_type, _)) = &request.body {
        headers.push(("content-type".into(), content_type.clone()));
    }
    state.sent.push(SentRequest {
        method: request.method.to_string(),
        url: url.to_owned(),
        headers,
        body: request
            .body
            .as_ref()
            .map(|(_, body)| String::from_utf8_lossy(body).into_owned())
            .unwrap_or_default(),
    });
    let method = request.method.as_str();
    let answers = state
        .routes
        .iter_mut()
        .find(|(pattern, _)| matches(pattern, method, url))
        .map(|(_, answers)| answers)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no fake for {method} {url}: add one with `fake_http().on(\"{url}\", …)`"
            )
        })?;
    let answer = if answers.len() > 1 {
        answers.remove(0)
    } else {
        answers[0].clone()
    };
    if answer.fails {
        return Err(anyhow::anyhow!("{method} {url}: connection refused (fake)").into());
    }
    Ok(Response {
        status: answer.status,
        headers: answer.headers,
        body: answer.body,
        url: url.to_owned(),
    })
}

/// Whether `pattern` (`[METHOD ]url-with-*`) matches.
fn matches(pattern: &str, method: &str, url: &str) -> bool {
    let (want_method, pattern) = match pattern.split_once(' ') {
        Some((m, rest)) if m.chars().all(|c| c.is_ascii_uppercase()) => (Some(m), rest),
        _ => (None, pattern),
    };
    if want_method.is_some_and(|m| m != method) {
        return false;
    }
    glob(pattern.as_bytes(), url.as_bytes())
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|i| glob(rest, &text[i..])),
        Some((c, rest)) => text.first() == Some(c) && glob(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "http")]
    #[test]
    fn the_client_builds_with_the_ring_provider() {
        // Would panic if no rustls crypto provider were available.
        let _ = client();
    }

    #[test]
    fn patterns() {
        assert!(matches("https://a.test/*", "GET", "https://a.test/x?y=1"));
        assert!(matches("POST https://a.test/o", "POST", "https://a.test/o"));
        assert!(!matches("POST https://a.test/o", "GET", "https://a.test/o"));
        assert!(!matches("https://a.test/o", "GET", "https://a.test/o/1"));
        assert!(matches("*", "DELETE", "https://b.test"));
        assert!(matches("https://*.test/*/x", "GET", "https://b.test/1/2/x"));
    }

    #[tokio::test]
    async fn fakes_answer_in_turn_and_retries_follow() {
        let http = Http::default();
        let fake = http.fake();
        fake.on("POST https://a.test/pay", FakeResponse::connection_error())
            .on("POST https://a.test/pay", FakeResponse::status(503))
            .on(
                "POST https://a.test/pay",
                FakeResponse::json(201, serde_json::json!({"id": 7})),
            );
        let response = http
            .post("https://a.test/pay")
            .json(&serde_json::json!({"total": 5}))
            .retry(2, Duration::ZERO)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.json::<serde_json::Value>().unwrap()["id"], 7);
        fake.assert_sent_count(3);
        assert_eq!(fake.sent()[0].json()["total"], 5);
        assert_eq!(
            fake.sent()[0].header("Content-Type"),
            Some("application/json")
        );

        // Without retries the first answer is the result; the last repeats.
        fake.on("https://a.test/flaky", FakeResponse::status(500));
        let res = http.get("https://a.test/flaky").send().await.unwrap();
        assert!(res.clone().error_for_status().is_err());
        assert!(!res.ok());
        assert!(
            http.get("https://a.test/none").send().await.is_err(),
            "no fake"
        );
    }

    #[tokio::test]
    async fn builds_urls_and_headers() {
        let http = Http::default();
        let fake = http.fake();
        fake.on("*", FakeResponse::text(200, "ok"));
        http.get("https://a.test/r?x=1")
            .query(&[("q", "iced coffee"), ("n", "2")])
            .bearer("t0k")
            .send()
            .await
            .unwrap();
        http.post("https://a.test/f")
            .basic_auth("u", "p")
            .form(&[("a", "1 2")])
            .send()
            .await
            .unwrap();
        let sent = fake.sent();
        assert_eq!(sent[0].url, "https://a.test/r?x=1&q=iced+coffee&n=2");
        assert_eq!(sent[0].header("authorization"), Some("Bearer t0k"));
        assert_eq!(sent[1].header("authorization"), Some("Basic dTpw"));
        assert_eq!(sent[1].body, "a=1+2");
        fake.assert_sent(|r| r.method == "POST");
        fake.assert_not_sent(|r| r.method == "DELETE");
    }

    // #252: the rest of the client's builder and response, on the fake.

    #[tokio::test]
    async fn put_patch_raw_bodies_and_response_helpers() {
        let http = Http::default();
        let fake = http.fake();
        fake.on(
            "PUT https://api.test/items/1",
            FakeResponse::text(200, "not json").header("x-request-id", "r1"),
        );
        fake.on("PATCH https://api.test/items/1", FakeResponse::status(204));
        let res = http
            .put("https://api.test/items/1")
            .body("text/csv", "a,b\n1,2")
            .send()
            .await
            .unwrap();
        assert_eq!(res.header("X-Request-Id"), Some("r1"));
        assert_eq!(res.bytes(), b"not json");
        let err = res.json::<serde_json::Value>().unwrap_err();
        assert!(
            format!("{err:?}")
                .contains("the response of https://api.test/items/1 is not the expected JSON"),
            "{err:?}"
        );
        http.patch("https://api.test/items/1").send().await.unwrap();
        let sent = fake.sent();
        assert_eq!(sent[0].method, "PUT");
        assert_eq!(sent[0].body, "a,b\n1,2");
        assert!(
            sent[0]
                .headers
                .iter()
                .any(|(k, v)| k == "content-type" && v == "text/csv")
        );
        assert_eq!(sent[1].method, "PATCH");
        // Debug says what it is, nothing more.
        assert_eq!(format!("{http:?}"), "Http");
    }

    #[tokio::test]
    async fn bodies_that_cant_be_encoded_fail_at_send() {
        let http = Http::default();
        let fake = http.fake();
        let mut odd = std::collections::HashMap::new();
        odd.insert((1, 2), 3);
        assert!(
            http.post("https://api.test/a")
                .json(&odd)
                .send()
                .await
                .is_err()
        );
        let nested = [("a", vec![1, 2])];
        assert!(
            http.post("https://api.test/b")
                .form(&nested)
                .send()
                .await
                .is_err()
        );
        assert!(fake.sent().is_empty(), "nothing was sent");
    }

    #[tokio::test]
    #[should_panic(
        expected = "no matching HTTP request was sent; sent: [\n    \"GET https://api.test/x\",\n]"
    )]
    async fn assert_sent_lists_what_was_sent() {
        let http = Http::default();
        let fake = http.fake();
        fake.on("https://api.test/*", FakeResponse::status(200));
        http.get("https://api.test/x").send().await.unwrap();
        fake.assert_sent(|r| r.method == "POST");
    }
}
