use std::collections::HashMap;

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use renox::Kernel;
use renox::prelude::*;
use tower::ServiceExt;

#[derive(serde::Serialize)]
struct Post {
    owner_id: i64,
}

impl Policy for Post {
    fn allows(&self, user: &User, ability: &str) -> bool {
        match ability {
            "update" => self.owner_id == user.id,
            _ => true,
        }
    }
}

struct Dashboard;

impl Module for Dashboard {
    fn name(&self) -> &'static str {
        "dashboard"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/", || async { view("home.html", ()) })
            .name("home")
            .get("/token", |session: Session| async move { session.token() })
            .get("/posts", |auth: Option<AuthUser>| async move {
                let posts: Vec<_> = [1, 2]
                    .map(|owner_id| Can::new(Post { owner_id }, auth.as_deref(), &["update"]))
                    .into();
                view("posts.html", context! { posts })
            })
            .get("/whoami", |auth: Option<AuthUser>| async move {
                auth.map_or("guest".to_owned(), |a| a.name.clone())
            })
            .merge(
                Routes::new()
                    .get("/dashboard", |auth: AuthUser| async move {
                        format!("hi {}", auth.name)
                    })
                    .get("/admin", |auth: AuthUser| async move {
                        auth.gate("admin")?;
                        Ok::<_, Error>("admin area")
                    })
                    .get(
                        "/posts/{owner}/edit",
                        |auth: AuthUser, Path(owner): Path<i64>| async move {
                            auth.authorize("update", &Post { owner_id: owner })?;
                            Ok::<_, Error>("editing")
                        },
                    )
                    .require_auth(),
            )
    }
}

async fn kernel(auth: Auth) -> (Kernel, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("home.html"),
        "{% if auth.check %}Hello {{ auth.user.name }}{% else %}Guest{% endif %}|admin={% if can('admin') %}yes{% else %}no{% endif %}|{{ auth.user | tojson }}",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("posts.html"),
        "{% for p in posts %}{{ p.owner_id }}:{{ 'edit' if can('update', p) else 'view' }};{% endfor %}",
    )
    .unwrap();
    let config = {
        let mut c = Config::default();
        c.env = Environment::Testing;
        c.key = Some(renox::generate_key());
        c.views_path = dir.path().to_path_buf();
        c
    };
    let kernel = App::with_config(config)
        .module(auth)
        .module(Dashboard)
        .gate("admin", |user| user.email.ends_with("@shop.test"))
        .boot()
        .await
        .unwrap();
    kernel.migrate().await.unwrap();
    (kernel, dir)
}

/// A browser: keeps the session cookie and sends the CSRF token.
struct Client {
    router: axum::Router,
    cookie: Option<String>,
}

struct Reply {
    status: StatusCode,
    headers: HashMap<String, String>,
    body: String,
}

impl Reply {
    fn location(&self) -> Option<&str> {
        self.headers.get("location").map(String::as_str)
    }
}

impl Client {
    fn new(kernel: &Kernel) -> Self {
        Self {
            router: kernel.router(),
            cookie: None,
        }
    }

    async fn send(&mut self, mut req: Request<Body>) -> Reply {
        if let Some(cookie) = &self.cookie {
            req.headers_mut().insert(COOKIE, cookie.parse().unwrap());
        }
        let res = self.router.clone().oneshot(req).await.unwrap();
        let headers: HashMap<_, _> = res
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_owned()))
            .collect();
        if let Some(set) = headers.get(SET_COOKIE.as_str()) {
            self.cookie = Some(set.split(';').next().unwrap().to_owned());
        }
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        Reply {
            status,
            headers,
            body: String::from_utf8(body.to_vec()).unwrap(),
        }
    }

    async fn get(&mut self, uri: &str) -> Reply {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
            .await
    }

    async fn post(&mut self, uri: &str, body: &str) -> Reply {
        let token = self.get("/token").await.body;
        self.send(
            Request::post(uri)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header("referer", uri)
                .body(Body::from(format!("_token={token}&{body}")))
                .unwrap(),
        )
        .await
    }

    async fn login(&mut self, email: &str, password: &str) -> Reply {
        self.post("/login", &format!("email={email}&password={password}"))
            .await
    }
}

#[tokio::test]
async fn registering_logs_in_and_goes_home() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    let mut client = Client::new(&kernel);
    assert!(
        client
            .get("/register")
            .await
            .body
            .contains(r#"name="password_confirmation""#)
    );

    let reply = client
        .post(
            "/register",
            "name=Alex&email=alex@example.com&password=secret123&password_confirmation=secret123",
        )
        .await;
    assert_eq!(
        (reply.status, reply.location()),
        (StatusCode::SEE_OTHER, Some("/"))
    );
    let home = client.get("/").await.body;
    assert!(home.starts_with("Hello Alex|admin=no|"), "{home}");
    assert!(
        !home.contains("argon2") && !home.contains("password"),
        "the hash never reaches templates: {home}"
    );

    let user = User::find_by_email(kernel.db(), "ALEX@example.com")
        .await
        .unwrap()
        .unwrap();
    assert!(user.password.starts_with("$argon2id$"));
}

#[tokio::test]
async fn registration_is_validated() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    User::register(kernel.db(), "Alex", "alex@example.com", "secret123")
        .await
        .unwrap();
    let mut client = Client::new(&kernel);

    let reply = client
        .post(
            "/register",
            "name=Other&email=ALEX@example.com&password=short&password_confirmation=different",
        )
        .await;
    assert_eq!(reply.location(), Some("/register"));
    let page = client.get("/register").await.body;
    assert!(
        page.contains("The email has already been taken."),
        "emails are unique regardless of case"
    );
    assert!(page.contains("The password must be at least 8 characters."));
    assert!(page.contains(r#"value="Other""#), "old input is kept");
    assert_eq!(client.get("/whoami").await.body, "guest");
}

#[tokio::test]
async fn logging_in_and_out() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    User::register(kernel.db(), "Alex", "alex@shop.test", "secret123")
        .await
        .unwrap();
    let mut client = Client::new(&kernel);

    let reply = client.login("alex@shop.test", "wrong").await;
    assert_eq!(reply.location(), Some("/login"));
    let page = client.get("/login").await.body;
    assert!(page.contains("These credentials do not match our records."));
    assert!(page.contains(r#"value="alex@shop.test""#));

    let guest_token = client.get("/token").await.body;
    let reply = client.login("Alex@Shop.test", "secret123").await;
    assert_eq!(
        (reply.status, reply.location()),
        (StatusCode::SEE_OTHER, Some("/"))
    );
    assert!(
        client
            .get("/")
            .await
            .body
            .starts_with("Hello Alex|admin=yes")
    );
    assert_ne!(
        client.get("/token").await.body,
        guest_token,
        "logging in issues a new CSRF token"
    );
    assert_eq!(
        client.get("/login").await.location(),
        Some("/"),
        "login is for guests only"
    );

    let reply = client.post("/logout", "").await;
    assert_eq!(reply.location(), Some("/"));
    assert!(client.get("/").await.body.starts_with("Guest"));
}

#[tokio::test]
async fn guards_send_guests_to_login_and_back() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    User::register(kernel.db(), "Alex", "alex@example.com", "secret123")
        .await
        .unwrap();
    let mut client = Client::new(&kernel);

    assert_eq!(
        client.get("/dashboard?tab=2").await.location(),
        Some("/login")
    );
    let htmx = client
        .send(
            Request::get("/dashboard")
                .header("hx-request", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        htmx.headers.get("hx-redirect").map(String::as_str),
        Some("/login")
    );
    let json = client
        .send(
            Request::get("/dashboard")
                .header("accept", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(json.status, StatusCode::UNAUTHORIZED);
    assert_eq!(json.body, r#"{"message":"Unauthenticated."}"#);

    // The first guarded GET is remembered and wins over `home` after login.
    let mut client = Client::new(&kernel);
    client.get("/dashboard?tab=2").await;
    assert_eq!(
        client
            .login("alex@example.com", "secret123")
            .await
            .location(),
        Some("/dashboard?tab=2")
    );
    assert_eq!(client.get("/dashboard").await.body, "hi Alex");
}

#[tokio::test]
async fn htmx_logins_redirect_the_whole_page() {
    let (kernel, _dir) = kernel(Auth::new().redirect_to("/dashboard")).await;
    User::register(kernel.db(), "Alex", "alex@example.com", "secret123")
        .await
        .unwrap();
    let mut client = Client::new(&kernel);
    let token = client.get("/token").await.body;
    let reply = client
        .send(
            Request::post("/login")
                .header("hx-request", "true")
                .header("x-csrf-token", token)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("email=alex@example.com&password=secret123"))
                .unwrap(),
        )
        .await;
    assert_eq!(
        reply.headers.get("hx-redirect").map(String::as_str),
        Some("/dashboard")
    );
}

#[tokio::test]
async fn remember_me_extends_the_session_cookie() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    User::register(kernel.db(), "Alex", "alex@example.com", "secret123")
        .await
        .unwrap();

    let mut client = Client::new(&kernel);
    let reply = client.login("alex@example.com", "secret123").await;
    assert!(reply.headers["set-cookie"].contains("Max-Age=7200"));

    let mut client = Client::new(&kernel);
    let reply = client
        .post(
            "/login",
            "email=alex@example.com&password=secret123&remember=1",
        )
        .await;
    assert!(
        reply.headers["set-cookie"].contains("Max-Age=2592000"),
        "{}",
        reply.headers["set-cookie"]
    );
    assert!(
        client.get("/dashboard").await.headers["set-cookie"].contains("Max-Age=2592000"),
        "and stays extended"
    );
}

#[tokio::test]
async fn changing_the_password_logs_out_other_sessions() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    let mut user = User::register(kernel.db(), "Alex", "alex@example.com", "secret123")
        .await
        .unwrap();
    let mut laptop = Client::new(&kernel);
    laptop.login("alex@example.com", "secret123").await;
    assert_eq!(laptop.get("/whoami").await.body, "Alex");

    user.set_password(kernel.db(), "new-secret").await.unwrap();
    assert_eq!(laptop.get("/whoami").await.body, "guest");
    assert_eq!(
        laptop
            .login("alex@example.com", "new-secret")
            .await
            .location(),
        Some("/")
    );
}

#[tokio::test]
async fn deleted_users_are_logged_out() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    let user = User::register(kernel.db(), "Alex", "alex@example.com", "secret123")
        .await
        .unwrap();
    let mut client = Client::new(&kernel);
    client.login("alex@example.com", "secret123").await;
    user.force_delete(kernel.db()).await.unwrap();
    assert_eq!(client.get("/whoami").await.body, "guest");
}

#[tokio::test]
async fn repeated_failures_are_throttled() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    User::register(kernel.db(), "Alex", "throttle@example.com", "secret123")
        .await
        .unwrap();
    let mut client = Client::new(&kernel);
    for _ in 0..5 {
        client.login("throttle@example.com", "wrong").await;
    }
    client.login("throttle@example.com", "secret123").await;
    let page = client.get("/login").await.body;
    assert!(
        page.contains("Too many login attempts. Please try again in"),
        "{page}"
    );
    assert_eq!(
        client.get("/whoami").await.body,
        "guest",
        "even the right password waits"
    );
}

#[tokio::test]
async fn policies_and_gates() {
    let (kernel, _dir) = kernel(Auth::new()).await;
    let owner = User::register(kernel.db(), "Alex", "alex@example.com", "secret123")
        .await
        .unwrap();
    let mut client = Client::new(&kernel);
    assert_eq!(client.get("/posts").await.body, "1:view;2:view;", "guests");
    client.login("alex@example.com", "secret123").await;
    assert_eq!(
        client.get("/posts").await.body,
        format!(
            "1:{};2:{};",
            if owner.id == 1 { "edit" } else { "view" },
            if owner.id == 2 { "edit" } else { "view" }
        ),
        "can('update', post) in templates asks the policy"
    );

    assert_eq!(
        client.get(&format!("/posts/{}/edit", owner.id)).await.body,
        "editing"
    );
    assert_eq!(
        client
            .get(&format!("/posts/{}/edit", owner.id + 1))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client.get("/admin").await.status,
        StatusCode::FORBIDDEN,
        "not an @shop.test email"
    );
}

#[tokio::test]
async fn registration_can_be_turned_off() {
    let (kernel, _dir) = kernel(Auth::new().without_registration()).await;
    let mut client = Client::new(&kernel);
    assert_eq!(client.get("/register").await.status, StatusCode::NOT_FOUND);
    assert!(!client.get("/login").await.body.contains("/register"));
}
