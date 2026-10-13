//! The benchmark's bare Axum app: the same four endpoints as the Renox app,
//! on the crates Renox uses (axum, sqlx, MiniJinja), with no middleware at
//! all. It is the floor: the difference to Renox is what Renox's defaults
//! (sessions, CSRF, security headers, request ids, error pages) cost.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use minijinja::{Environment, context, path_loader};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    views: Arc<Environment<'static>>,
}

#[derive(Serialize, FromRow)]
struct Item {
    id: i64,
    name: String,
    price: i64,
    stock: i64,
}

#[derive(Deserialize)]
struct Id {
    id: Option<i64>,
}

impl Id {
    fn id(&self) -> i64 {
        self.id.unwrap_or(1).clamp(1, 9_980)
    }
}

fn error(e: impl std::fmt::Display) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
}

async fn plaintext() -> &'static str {
    "Hello, World!"
}

async fn json() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "message": "Hello, World!" }))
}

async fn db_row(State(state): State<AppState>, Query(q): Query<Id>) -> Response {
    match sqlx::query_as::<_, Item>("SELECT id, name, price, stock FROM items WHERE id = ?")
        .bind(q.id())
        .fetch_one(&state.db)
        .await
    {
        Ok(item) => Json(item).into_response(),
        Err(e) => error(e),
    }
}

async fn page(State(state): State<AppState>, Query(q): Query<Id>) -> Response {
    let items = match sqlx::query_as::<_, Item>(
        "SELECT id, name, price, stock FROM items WHERE id >= ? ORDER BY id LIMIT 20",
    )
    .bind(q.id())
    .fetch_all(&state.db)
    .await
    {
        Ok(items) => items,
        Err(e) => return error(e),
    };
    let rendered = state
        .views
        .get_template("page.html")
        .and_then(|t| t.render(context! { items }));
    match rendered {
        Ok(html) => Html(html).into_response(),
        Err(e) => error(e),
    }
}

#[tokio::main]
async fn main() {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/bench.db".into());
    let options: SqliteConnectOptions = url.parse().expect("DATABASE_URL");
    let pool_size = std::env::var("DATABASE_POOL_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let db = SqlitePoolOptions::new()
        .max_connections(pool_size)
        .connect_with(options.read_only(true))
        .await
        .expect("database");
    let mut env = Environment::new();
    env.set_loader(path_loader(
        std::env::var("VIEWS_PATH").unwrap_or_else(|_| "views".into()),
    ));
    let state = AppState {
        db,
        views: Arc::new(env),
    };
    let app = Router::new()
        .route("/plaintext", get(plaintext))
        .route("/json", get(json))
        .route("/db", get(db_row))
        .route("/page", get(page))
        .with_state(state);
    let port = std::env::var("APP_PORT").unwrap_or_else(|_| "3000".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("bind");
    axum::serve(listener, app).await.expect("serve");
}
