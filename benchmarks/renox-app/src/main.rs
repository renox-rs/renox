//! The benchmark's Renox app: four endpoints over the shared SQLite database
//! (`data/bench.db`, made by `seed.py`). Everything else is Renox as an app
//! gets it: its default middleware (security headers, sessions, CSRF, the
//! request id, the view layer…), production settings from the environment
//! (`run.sh`), no workers and no scheduler.

use renox::db::sql;
use renox::prelude::*;
use serde::Serialize;

#[derive(Serialize, FromRow)]
struct Item {
    id: i64,
    name: String,
    price: i64,
    stock: i64,
}

/// `GET /plaintext`: the framework's own cost.
async fn plaintext() -> &'static str {
    "Hello, World!"
}

/// `GET /json`: a small JSON body.
async fn json() -> Json<renox::serde_json::Value> {
    Json(json!({ "message": "Hello, World!" }))
}

/// `GET /db?id=N`: one row by its key, as JSON.
async fn db(State(state): State<AppState>, Query(q): Query<Id>) -> Result<Json<Item>> {
    let item = sql("SELECT id, name, price, stock FROM items WHERE id = ?")
        .bind(q.id())
        .fetch_one_as::<Item>(&state.db)
        .await?;
    Ok(Json(item))
}

/// `GET /page?id=N`: twenty rows through a template, a page as an app serves it.
async fn page(State(state): State<AppState>, Query(q): Query<Id>) -> Result<View> {
    let items = sql("SELECT id, name, price, stock FROM items WHERE id >= ? ORDER BY id LIMIT 20")
        .bind(q.id())
        .fetch_as::<Item>(&state.db)
        .await?;
    Ok(view("page.html", context! { items }))
}

#[derive(serde::Deserialize)]
struct Id {
    id: Option<i64>,
}

impl Id {
    fn id(&self) -> i64 {
        self.id.unwrap_or(1).clamp(1, 9_980)
    }
}

struct Bench;

impl Module for Bench {
    fn name(&self) -> &'static str {
        "bench"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/plaintext", plaintext)
            .get("/json", json)
            .get("/db", db)
            .get("/page", page)
    }
}

fn main() -> renox::Result {
    App::new().module(Bench).run()
}
