# Full-text search

Find records by the words in them, the way people type into a search box: "roast coffee"
finds the post titled "Roasting coffee at home", best matches first. Renox gives you one API
on both databases: SQLite's FTS5 and PostgreSQL's `tsvector`. It is Renox's answer to
Laravel Scout's database engine, without a separate search server. [examples/bikeshop](../examples/bikeshop)'s
catalogue search is a complete example on both databases.

Three steps:

1. Say which columns are searched, on the model.
2. Create the index with a migration.
3. Search with `Post::search(&words)`, a query like any other.

## 1. The model

```rust
use renox::prelude::*;

#[derive(Model, serde::Serialize, Default)]
#[model(table = "posts", search = "title, body")]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
}
```

`search` lists text columns, the most important first: a word in the title counts for more
than the same word in the body when results are ranked. The derive refuses a name that isn't
one of the model's columns.

## 2. The index

The index is made by a migration that Renox writes from the model, so its columns and
language always match what the queries expect. Register it after the migration that creates
the table (migrations run in name order):

```rust
# use renox::prelude::*;
# #[derive(Model, serde::Serialize, Default)]
# #[model(table = "posts", search = "title, body")]
# pub struct Post { pub id: i64, pub title: String, pub body: String }
pub fn app() -> App {
    App::new()
        .migrations(renox::migrations!())
        .migrations(&[renox::db::search::migration::<Post>(
            "20260104000000_search_posts",
        )])
}
```

Then `cargo run -- migrate`, as for any migration. The rows already in the table are indexed
by the migration itself.

What it creates, for the `posts` table above:

| | SQLite | PostgreSQL |
|---|---|---|
| The index | an FTS5 table `posts_search` that reads its text from `posts` (external content) | a column `search_vector` (`tsvector GENERATED ALWAYS … STORED`) and a GIN index `posts_search_index` |
| Kept current by | three triggers on `posts`: `posts_search_insert`, `_update`, `_delete` | the database: a generated column is recomputed on every write |
| Rolled back | triggers and FTS5 table dropped | index and column dropped |

Changed the searchable columns? Add another `migration::<Post>(…)` under a new name. It
replaces the index with one for the new columns and fills it; rolling that migration back
removes the index altogether (roll back further, or migrate again, to restore the old one).

Prefer SQL files? `migration::<Post>("x").up_for(Dialect::Sqlite)` (and `Dialect::Postgres`,
`down_for`) give the SQL to paste into `*.sqlite.up.sql` / `*.postgres.up.sql` files.

## 3. Searching

```rust
# use renox::prelude::*;
# #[derive(Model, serde::Serialize, Default)]
# #[model(table = "posts", search = "title, body")]
# pub struct Post { pub id: i64, pub title: String, pub body: String, pub published: bool }
#[derive(serde::Deserialize)]
struct Search {
    q: Option<String>,
}

async fn index(State(db): State<Db>, Page(page): Page, Query(s): Query<Search>) -> Result<View> {
    let q = s.q.unwrap_or_default();
    let posts = Post::search(&q)          // the matches, best first
        .where_(Post::PUBLISHED.eq(true)) // any other filter
        .latest()                         // breaks ties between equal matches
        .paginate(&db, page, 20)
        .await?;
    Ok(view("posts/index.html", context! { posts, q }))
}
```

`Post::search(words)` is `Post::query().search(words)`: the query keeps the model's default
scope and hides soft-deleted rows, and takes every filter, `count`, `first`, `pluck`,
`paginate`, `update` and `delete` as usual. Its two halves are there on their own too:

| Method | Does |
|---|---|
| `Model::search(words)` | `query().search(words)` |
| `Query::search(words)` | `where_search` then `order_by_relevance` |
| `Query::where_search(words)` | keeps the matching rows, in the query's own order |
| `Query::order_by_relevance(words)` | best matches first; rows that don't match last |

```rust
# use renox::prelude::*;
# #[derive(Model, serde::Serialize, Default)]
# #[model(table = "posts", search = "title, body")]
# pub struct Post { pub id: i64, pub title: String, pub body: String, pub published: bool }
# async fn demo(db: Db) -> Result {
// Newest first, among the matches.
let recent = Post::query().where_search("espresso").latest().limit(5).get(&db).await?;
// How many drafts mention it.
let drafts = Post::query().where_(Post::PUBLISHED.eq(false)).where_search("espresso").count(&db).await?;
# let _ = (recent, drafts); Ok(()) }
```

### What matches

- **Every word** of the search must be in the row (in any of the searchable columns).
- **A word matches the start of longer words:** `cof` finds "coffee", so a search box can
  search as the user types.
- **Other forms of a word match** (English, the default): "roasted" finds "roasting".
- **Case doesn't matter.** On SQLite, accents don't either ("cafe" finds "Café").
- **Only letters and digits count.** Everything else separates words, so `don't-stop!` is the
  three words `don`, `t` and `stop`. Search syntax (`OR`, `NEAR`, quotes, `*`, `-`, `&`, `:`)
  is never interpreted: it's text like any other, and user input is always safe to pass
  as it is. The words are bound as a value, never written into the SQL.
- **A search without any word** (empty, or only punctuation) filters nothing.

### Ranking

Best matches come first: SQLite ranks with BM25, PostgreSQL with `ts_rank`. The column
listed first in `search` weighs most (1.0, then 0.4, 0.2, and 0.1 for the fourth and later).
Add `order_by`/`latest` after `search` to order equal matches.

### Language

English is the default. For other languages:

```rust
# use renox::prelude::*;
#[derive(Model, serde::Serialize, Default)]
#[model(table = "recetas", search = "titulo, cuerpo", search_language = "spanish")]
pub struct Receta { pub id: i64, pub titulo: String, pub cuerpo: String }
```

On PostgreSQL the name is a text search configuration (`\dF` in `psql` lists them:
`spanish`, `german`, `simple` for no stemming, …). SQLite stems English only, so with any
other language it matches words as typed (prefixes still work). PostgreSQL's `english` also
leaves out common words ("the", "a", "or") that SQLite keeps.

## Kept in sync on every write

The database keeps the index current, not the model: `save`, `delete`, `force_delete`, and
also bulk `Query::update`/`delete`, `insert_many`, `upsert` and raw SQL, which all skip model
hooks. That is why triggers (SQLite) and a generated column (PostgreSQL) were chosen over
model hooks: no write path can forget the index, and a transaction that rolls back takes its
index changes with it.

Soft-deleted rows stay in the index, and queries hide them as usual (`with_trashed()` brings
them back).

**Rebuilding.** Should the SQLite index ever miss rows (written while its triggers were
dropped, or a table restored on its own), fill it again from the table:

```rust
# use renox::prelude::*;
# #[derive(Model, serde::Serialize, Default)]
# #[model(table = "posts", search = "title, body")]
# pub struct Post { pub id: i64, pub title: String, pub body: String }
# fn register(app: App) -> App {
app.command("search:rebuild", "Fill the posts' search index again", |_args, state| async move {
    renox::db::search::rebuild::<Post>(&state.db).await
})
# }
```

On PostgreSQL `rebuild` does nothing: a generated column can't fall behind.

## In the data grid

When a grid lists a searchable model, its search box uses the index: every word of the box
must be in the index (all of `search`'s columns, also those the grid doesn't show), longer
words and other forms match, and rows come best match first until the user sorts by a
heading. Searchable grid columns outside the index (a related value, a number) are still
matched as text: each word must then be in the index or in one of those columns.

```rust
# use renox::prelude::*;
use renox::grid::{Column, Grid};
# #[derive(Model, serde::Serialize, Default)]
# #[model(table = "posts", search = "title, body")]
# pub struct Post { pub id: i64, pub title: String, pub body: String }
let grid = Grid::new("posts")
    .column(Column::text("title", "Title").searchable()) // shows the box; the body is searched too
    .sort_by("-id");
# let _ = grid;
```

## Limits and notes

- Codes with hyphens or dots (`GIR-JER-0001-1`, `SKU-12.5`) are found typed whole or in part
  on both databases. SQLite splits them into words; PostgreSQL's parser reads `-1` or
  `-12.5` as one signed number, so on PostgreSQL the search also asks for the text as typed,
  read by that parser.
- Searchable columns are text columns (`TEXT`, `VARCHAR`). On PostgreSQL an integer column
  works too; a date doesn't.
- SQLite's FTS5 table uses the table's `rowid`, so the table must not be `WITHOUT ROWID` (any
  key type works otherwise: `i64`, `Ulid`, `Uuid`, `String`).
- On PostgreSQL the table gets a `search_vector` column. Models list their columns, so it
  isn't read; a model with `SELECT_ALL` (the built-in `User`) would see it.
- A search uses at most 16 words of up to 64 characters each.
- Laravel Scout's other engines (Algolia, Meilisearch, Typesense) aren't built in: for those,
  call their HTTP APIs with `renox::http` from model hooks or a job.

## Laravel → Renox

| Laravel Scout | Renox |
|---|---|
| `use Searchable;` + `toSearchableArray()` | `#[model(search = "title, body")]` |
| the `database` engine, a full-text index in a migration | `renox::db::search::migration::<Post>("…")` |
| `Post::search('coffee')->get()` | `Post::search("coffee").get(&db)` |
| `->where('published', true)->paginate(20)` | `.where_(Post::PUBLISHED.eq(true)).paginate(&db, page, 20)` |
| `scout:import` | `renox::db::search::rebuild::<Post>(&db)` (rarely needed: triggers keep it current) |
| `searchable()` / `unsearchable()` on a model | automatic on every write |
