# Relations and queries beyond one table

Real data is spread over several tables that point at each other: a product has a category, a
post has comments. This guide shows how to read such connected rows in Renox, quickly and
without surprises.

### In this guide

- how to get the related rows of **one** row (a product's category);
- how to get the related rows of a **whole page** of rows, without running one query per row;
- many-to-many links, extra data on a link, and rows that can belong to several tables;
- plain SQL when a join or a report is the clearest way;
- a long table of query-builder methods, with their Laravel names.

### Words you'll meet

| Word | What it means |
|---|---|
| **table**, **row**, **column** | A table is like a spreadsheet: each row is one thing (one product), each column one piece of it (its name, its price). |
| **model** | A Rust struct that matches a table: each field is a column, each value is one row. |
| **key** (or id) | The column that names a row, usually `id`. No two rows share the same key. |
| **foreign key** | A column that holds another table's key. `products.category_id` says which category a product is in. |
| **relation** | A link between two tables, made by a foreign key. |
| **belongs to** | The row holds the foreign key: a product *belongs to* a category. |
| **has many** | The other side: a category *has many* products, a product *has many* reviews. |
| **many to many** | Both sides have many: a product has many tags, and a tag is on many products. |
| **pivot table** | The extra table that stores many-to-many links, one row per link (`product_tags`). |
| **polymorphic** | A row that can belong to one of *several* tables (a comment on a post *or* on a video). |
| **query** | One question sent to the database, like "give me the products of category 3". |
| **N+1 queries** | The classic slowdown: 1 query for a list, then 1 more for *each* row in it. 20 products → 21 queries. |
| **join** | SQL that reads two tables at once, matching rows by a key. |
| **NULL** | "No value" in a database column. In Rust it becomes `None`. |

### No lazy relations: what Renox does instead

Renox has no Eloquent-style lazy relations: `product.category` doesn't quietly run a query.

Why not?

- Rust has no reflection (a way for code to look at a struct's fields while the program runs),
  so there is nothing to build them from.
- Hidden queries are where N+1 problems come from. If reading a field can run a query, a loop
  over 20 rows quietly runs 20 queries.

Instead, Renox gives you four plain tools:

- a foreign key column in the struct;
- a method when you need one related row;
- loaders that fetch the related rows of a whole page in one query;
- plain SQL, read into structs, when a join is the clearest way.

> [!NOTE]
> **Coming from Laravel:** there is no `$product->category` that loads by itself, and no
> `with('category')` on the query. You write a small method for one row, or call a loader for a
> page of rows. The table at the end of this page maps Laravel's names to Renox's.

[examples/bikeshop](../examples/bikeshop) uses them at volume: `Pivot` for which parts fit which bikes
([src/app/catalog/model.rs](../examples/bikeshop/src/app/catalog/model.rs)), `Morph` for its stock ledger and
payments ([src/app/stock/model.rs](../examples/bikeshop/src/app/stock/model.rs)), `has_many_through` for a
customer's work orders through their bikes
([src/app/workshop/model.rs](../examples/bikeshop/src/app/workshop/model.rs)), with tests that count queries
on 15,000 rentals ([tests/queries.rs](../examples/bikeshop/tests/queries.rs)). Its `/about/data` page explains the
whole data model.

### The tables used on this page

The examples below use this schema (the list of tables and their columns):

```sql
-- Categories: just a name.
CREATE TABLE categories (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);
-- Products: each one may be in a category (category_id may be NULL).
CREATE TABLE products (id INTEGER PRIMARY KEY AUTOINCREMENT, category_id INTEGER REFERENCES categories (id),
                       name TEXT NOT NULL, price INTEGER NOT NULL);
-- Reviews: each one is about exactly one product.
CREATE TABLE reviews (id INTEGER PRIMARY KEY AUTOINCREMENT, product_id INTEGER NOT NULL REFERENCES products (id),
                      stars INTEGER NOT NULL);
CREATE TABLE tags (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);
-- The pivot table: one row per (product, tag) link, and each link at most once.
CREATE TABLE product_tags (product_id INTEGER NOT NULL, tag_id INTEGER NOT NULL, UNIQUE (product_id, tag_id));
```

In words:

- a product **belongs to** a category (through `products.category_id`);
- a product **has many** reviews (each review has a `product_id`);
- products and tags are **many to many**, linked through the `product_tags` pivot table.

## One related row, or the rows of one model

When you have **one** product and want its category or its reviews, write a method. It is one
line, it is typed, and the query it runs is visible where it's called:

```rust
use renox::prelude::*;

/// A row of `categories`.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "categories")]
pub struct Category { pub id: i64, pub name: String }

/// A row of `products`. `category_id` is `Option` because a product may have no category.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "products")]
pub struct Product { pub id: i64, pub category_id: Option<i64>, pub name: String, pub price: i64 }

/// A row of `reviews`. Every review is about one product.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "reviews")]
pub struct Review { pub id: i64, pub product_id: i64, pub stars: i64 }

impl Product {
    /// belongs to
    ///
    /// The product's category, or `None` when it has none.
    pub async fn category(&self, db: &Db) -> Result<Option<Category>> {
        // No category id means no category: don't ask the database at all.
        match self.category_id {
            Some(id) => Category::find(db, id).await,
            None => Ok(None),
        }
    }

    /// has many
    ///
    /// The product's reviews, best first.
    pub async fn reviews(&self, db: &Db) -> Result<Vec<Review>> {
        Review::query().where_(Review::PRODUCT_ID.eq(self.id)).order_by_desc(Review::STARS).get(db).await
    }
}
```

What's going on:

- `category` reads the product's `category_id` and looks up that one category with `find`.
- `reviews` asks for every review whose `product_id` is this product's id, best rated first.
- Each method runs exactly one query, and you see the call (`product.reviews(&db)`) in your
  code, so nothing happens behind your back.

> [!WARNING]
> These methods are fine for one row. In a loop over many rows, they cause the N+1 problem.
> For a list or a page, use the loaders in the next section.

## A page of rows with their relations (no N+1)

Calling `product.category(&db)` in a loop over 20 products runs 21 queries: 1 for the
products, then 1 per product.

The loaders in `renox::db::relations` fix that. Each loader fetches the related rows of the
**whole page** in one query, and hands them back in a `HashMap` (a lookup table, from a key to
a value). You then look each row's relations up by id:

```rust
use renox::prelude::*;
use renox::db::relations::{Pivot, belongs_to, has_many};
# #[derive(Model, serde::Serialize, Default, Clone)] #[model(table = "categories")] pub struct Category { pub id: i64, pub name: String }
# #[derive(Model, serde::Serialize, Default, Clone)] #[model(table = "products")] pub struct Product { pub id: i64, pub category_id: Option<i64>, pub name: String, pub price: i64 }
# #[derive(Model, serde::Serialize, Default, Clone)] #[model(table = "reviews")] pub struct Review { pub id: i64, pub product_id: i64, pub stars: i64 }
# #[derive(Model, serde::Serialize, Default, Clone)] #[model(table = "tags")] pub struct Tag { pub id: i64, pub name: String }

/// many to many, through a pivot table
///
/// The arguments: the pivot table, its column for the product, its column for the tag.
pub const PRODUCT_TAGS: Pivot = Pivot::new("product_tags", "product_id", "tag_id");

/// What the template gets for each product.
#[derive(serde::Serialize)]
struct ProductCard {
    /// `flatten` puts the product's own fields (`name`, `price`…) right on the card.
    #[serde(flatten)]
    product: Product,
    category: Option<Category>,
    reviews: Vec<Review>,
    tags: Vec<Tag>,
}

/// A page of 20 products, each with its category, reviews and tags: 6 queries in all.
async fn index(State(db): State<Db>, Page(page): Page) -> Result<View> {
    let products = Product::query().order_by(Product::NAME).paginate(&db, page, 20).await?; // 2 queries
    // The categories of all 20 products, keyed by category id.
    let mut categories = belongs_to::<Category, _, _>(&db, &products.items, |p| p.category_id).await?; // 1
    // The reviews of all 20 products, grouped by product id, best first.
    let mut reviews = has_many(&db, &products.items, Review::query().order_by_desc(Review::STARS),
                               "product_id", |r| r.product_id).await?; // 1
    // The tags of all 20 products, grouped by product id.
    let mut tags = PRODUCT_TAGS.load_for::<Tag, _>(&db, &products.items).await?; // 2
    // Build one card per product, taking its relations out of the maps.
    let cards = products.map(|product| ProductCard {
        category: product.category_id.and_then(|id| categories.remove(&id)),
        reviews: reviews.remove(&product.id).unwrap_or_default(),
        tags: tags.remove(&product.id).unwrap_or_default(),
        product,
    });
    Ok(view("products/index.html", context! { products => cards }))
}
```

What's going on:

- `paginate` gets one page of 20 products (2 queries: the rows, and the total for the page
  links).
- `belongs_to` gets all the categories those products point at, in 1 query.
- `has_many` gets all their reviews in 1 query.
- `load_for` gets all their tags (2 queries: the links in the pivot table, then the tags).
- `remove(&id)` takes a product's entry out of a map. A product with no reviews or tags has no
  entry, so `unwrap_or_default()` gives it an empty list.
- That's 6 queries for the whole page, however many products it shows. A loop calling
  `product.category(&db)` and `product.reviews(&db)` would add 2 queries per product instead.

In the template, `{{ card.category.name }}` and `{% for tag in card.tags %}` read plain data:
nothing there can run a query.

Each loader takes the page's rows, so it works the same for `get()`, `paginate()` and `chunk()`.

### The loaders

- `belongs_to::<Parent, _, _>(db, &children, |child| child.parent_id)` returns a
  `HashMap<parent id, Parent>`. The closure (the small `|child| …` function) returns the
  parent's key, or an `Option` of it: `i64`, or a `Ulid`, `Uuid` or `String` for parents keyed
  that way (`.clone()` those).
- `has_many(db, &parents, Child::query()…, "parent_id", |child| child.parent_id)` returns a
  `HashMap<parent id, Vec<Child>>`. The query you pass sets the children's order and filters.
- `Pivot::load_for::<Target, _>(db, &parents)` / `load(db, ids)` returns a
  `HashMap<parent id, Vec<Target>>`. `Pivot::inverse()` gives the other direction (from tags to
  products).
- `Model::find_many(db, ids)` returns the rows with these ids.

The loaders read the related model through its normal query, so its default scope (for example
the current team) and its soft deletes apply. A parent hidden that way is simply missing from
the map, like a parent that doesn't exist. `model.refresh(&db)` is different: it reloads the
row by its id, ignoring the default scope and soft deletes.

### Keys that aren't numbers

The maps are keyed by the parent's key type (`Model::Key`). Most models use an `i64` key, but
a model can also be keyed by a `Ulid`, `Uuid` or `String` (see [types.md](types.md)).

- A pivot between models with other keys names them, left then right: `Pivot<Ulid, i64>`.
  `Pivot` alone is `Pivot<i64, i64>`.
- A polymorphic relation's id column holds the parents' key, so every parent type of one
  `Morph` needs the same key type.

> [!NOTE]
> **Coming from Laravel:** this is Renox's version of eager loading (`with('category')`). The
> difference: you get maps back and put the pieces together yourself, so it's always clear
> which queries run.

## Changing a many-to-many

To add or remove links between a product and its tags, call methods on the pivot:

```rust
# use renox::prelude::*;
# use renox::db::relations::Pivot;
# const PRODUCT_TAGS: Pivot = Pivot::new("product_tags", "product_id", "tag_id");
# async fn demo(db: Db, product_id: i64, checked: Vec<i64>) -> Result {
PRODUCT_TAGS.attach(&db, product_id, [1, 2]).await?; // adds links that aren't there yet; returns how many
PRODUCT_TAGS.detach(&db, product_id, [2]).await?; // returns how many links were removed
PRODUCT_TAGS.sync(&db, product_id, checked).await?;  // exactly these (a form's checkboxes), in a transaction
let tag_ids = PRODUCT_TAGS.ids(&db, product_id).await?;
# let _ = tag_ids; Ok(()) }
```

What each one does:

- `attach` links the product to tags 1 and 2. Links that already exist are left alone. It
  returns how many links it added.
- `detach` removes the link to tag 2, and returns how many links it removed.
- `sync` makes the links **exactly** this list: it adds what's missing and removes the rest.
  That's what you want after a form with one checkbox per tag. It runs in a transaction (all
  the changes happen, or none do).
- `ids` returns the ids of the tags the product is linked to now.

`attach`, `detach`, `ids`, `attach_with` and `update_pivot` take any database handle, so they
also run inside your own transaction (`&mut tx`). `sync` and `toggle` take `&Db` only: they open
a transaction of their own, so they can't be part of yours.

## Pivot columns

A pivot table may carry more than the two ids: a member's role, a quantity, when it was added.
Here a team has many users, and each membership has a role:

```rust
# use renox::prelude::*;
use renox::db::relations::Pivot;

// team_user (team_id, user_id, role TEXT NOT NULL, created_at, updated_at)
/// The `team_user` pivot. `with_timestamps` fills its `created_at` and `updated_at` columns.
const MEMBERS: Pivot = Pivot::new("team_user", "team_id", "user_id").with_timestamps();

/// The extra columns of one link, read next to each user.
#[derive(FromRow)]
struct Membership { role: String, created_at: Option<DateTime> }

# async fn demo(db: Db, team_id: i64) -> Result {
MEMBERS.attach_with(&db, team_id, 7, &[("role", &"admin")]).await?; // false if already linked
MEMBERS.update_pivot(&db, team_id, 7, &[("role", &"member")]).await?; // false if not linked
let (added, removed) = MEMBERS.toggle(&db, team_id, [7, 8]).await?; // flips each link
let members = MEMBERS.load_with_pivot::<User, Membership>(&db, [team_id]).await?;
// Each user of the team comes with their link's columns.
for (user, membership) in members.get(&team_id).map_or(&[][..], Vec::as_slice) {
    println!("{}: {}", user.name, membership.role);
}
# let _ = (added, removed); Ok(()) }
```

What's going on:

- `attach_with` links user 7 to the team and sets the link's `role` to `"admin"`. It returns
  `false` if they were already linked.
- `update_pivot` changes the role on an existing link. It returns `false` if there is no link.
- `toggle` flips each link: users 7 and 8 are linked if they weren't, and unlinked if they
  were. It returns the ids it linked and the ids it unlinked.
- `load_with_pivot` loads the team's users, each paired with its `Membership` (the link's own
  columns), grouped by team id.

`with_timestamps()` fills `created_at`/`updated_at` on `attach`, `attach_with`, `sync` and
`toggle`, and `updated_at` on `update_pivot`.

## Polymorphic relations

Sometimes a child can belong to one of several tables: comments on posts **and** on videos.
One foreign key can't point at two tables, so the child keeps two columns instead:

- the parent's **table name** (`commentable_type`, for example `"posts"`);
- the parent's **id** (`commentable_id`).

Renox writes `P::TABLE` (`"posts"`) as the type.

```rust
# use renox::prelude::*;
use renox::db::relations::Morph;

# #[derive(Model, serde::Serialize, Default, Clone)] #[model(table = "posts")] struct Post { id: i64, title: String }
# #[derive(Model, serde::Serialize, Default, Clone)] #[model(table = "videos")] struct Video { id: i64, title: String }
/// A comment on a post or a video: which table, and which row in it.
#[derive(Model, serde::Serialize, Default, Clone)]
#[model(table = "comments")]
struct Comment { id: i64, commentable_type: String, commentable_id: i64, body: String }

/// Names the two columns that point at the parent.
const COMMENTABLE: Morph = Morph::new("commentable_type", "commentable_id");

# async fn demo(db: Db, post: Post) -> Result {
// A comment on a post: the type is the posts table's name, the id the post's id.
let comment = Comment {
    commentable_type: Post::TABLE.into(),
    commentable_id: post.id,
    body: "Nice".into(),
    ..Default::default()
};
Comment::create(&db, comment).await?;

let count = COMMENTABLE.of(&post, Comment::query()).count(&db).await?; // this post's comments
// The comments of a page of posts, grouped by post id, in one query.
let posts = Post::query().latest().limit(20).get(&db).await?;
let comments = COMMENTABLE.load_many(&db, &posts, Comment::query(), |c| c.commentable_id).await?;

// The other way (morphTo): one query per parent type.
let recent = Comment::query().latest().limit(50).get(&db).await?;
let parent = |c: &Comment| (c.commentable_type.clone(), c.commentable_id);
let on_posts = COMMENTABLE.parents::<Post, _>(&db, &recent, parent).await?;
let on_videos = COMMENTABLE.parents::<Video, _>(&db, &recent, parent).await?;
# let _ = (count, comments, on_posts, on_videos); Ok(()) }
```

What's going on:

- Saving a comment: set `commentable_type` to `Post::TABLE` and `commentable_id` to the post's
  id.
- `COMMENTABLE.of(&post, …)` is a query for one post's comments. Here it counts them.
- `load_many` gets the comments of a whole page of posts, in one query (no N+1).
- `parents` goes the other way: from comments to the posts (or videos) they're on. It takes one
  query per parent type, so two here.

> [!WARNING]
> The database can't enforce a foreign key here, so delete the children yourself, e.g. in the
> parent's `deleting` hook (see `ModelHooks` in the cheatsheet). Index the two columns together.

> [!NOTE]
> **Coming from Laravel:** `Morph::load_many` is an eager `morphMany`, `Morph::of` is
> `$post->comments()`, and `Morph::parents` is `morphTo`.

## Through a middle model: `has_many_through`

Sometimes the rows you want are two steps away. A category's comments are the comments of its
posts: category → posts → comments.

`has_many_through` loads them for a page of parents in two queries, the middle rows and then
the children, grouped by parent:

```rust
# use renox::prelude::*;
use renox::db::relations::has_many_through;
# #[derive(Model, serde::Serialize, Default)] #[model(table = "categories")] struct Category { id: i64, name: String }
# #[derive(Model, serde::Serialize, Default)] #[model(table = "posts")] struct Post { id: i64, category_id: Option<i64>, title: String }
# #[derive(Model, serde::Serialize, Default)] #[model(table = "comments")] struct Comment { id: i64, post_id: i64, body: String }
# async fn demo(db: Db, categories: Vec<Category>) -> Result {
let comments = has_many_through(
    &db,
    &categories,
    Post::query(), "category_id", |p: &Post| p.category_id, // the middle model, pointing at the parent
    Comment::query().latest(), "post_id", |c: &Comment| c.post_id, // the children, pointing at the middle
)
.await?;
// The first category's comments, or an empty list when it has none.
let first = comments.get(&categories[0].id).map_or(&[][..], Vec::as_slice);
# let _ = first; Ok(()) }
```

What's going on:

- The middle step is posts: `Post::query()`, their column pointing at the category
  (`"category_id"`), and how to read it from a post.
- The last step is comments: `Comment::query().latest()`, their column pointing at the post
  (`"post_id"`), and how to read it from a comment.
- The result maps each category's id to its comments.

Filters and order go on either query.

> [!WARNING]
> A `limit` on the children's query counts across all the parents, not per parent. So use it
> with one parent (say, the five latest comments of one post).

> [!NOTE]
> **Coming from Laravel:** this is `hasManyThrough`.

## Joins and reports: SQL read into structs

A join or an aggregate (a total, a count, an average) is clearest as SQL. `fetch_as` reads the
rows into:

- a `#[derive(FromRow)]` struct (columns by name; `#[row(rename = "…")]`, `#[row(skip)]`);
- a model;
- a tuple (columns by position).

```rust
use renox::prelude::*;

/// One line of the sales report. Each field is read from the column of the same name.
#[derive(FromRow, serde::Serialize)]
struct CategorySales {
    category: String,
    products: i64,
    revenue: i64,
}

# async fn demo(db: Db) -> Result {
// Per category: how many products, and the sum of their prices, biggest first.
let sales: Vec<CategorySales> = renox::db::sql(
    "SELECT c.name AS category, COUNT(p.id) AS products, CAST(SUM(p.price) AS BIGINT) AS revenue \
     FROM categories c JOIN products p ON p.category_id = c.id \
     GROUP BY c.name ORDER BY revenue DESC",
)
.fetch_as(&db)
.await?;
// A tuple reads the columns in order: the id first, then the name.
let names: Vec<(i64, String)> = renox::db::sql("SELECT id, name FROM products").fetch_as(&db).await?;
# let _ = (sales, names); Ok(()) }
```

What's going on:

- The SQL joins `categories` and `products`, then groups the rows by category name.
- `AS category`, `AS products` and `AS revenue` name the columns, so they match the struct's
  fields.
- `fetch_as` turns each row into a `CategorySales`.

> [!IMPORTANT]
> On PostgreSQL, `SUM` and `COUNT` of `BIGINT` come back as `NUMERIC` and `BIGINT`. Cast sums
> with `CAST(… AS BIGINT)`, as above, so the same struct reads on both databases.

## Filtering by a related table without a join

You can filter rows by what's in another table, without writing a join. Here: products in an
active category, and (when there is a search text) whose name matches it or whose price is low:

```rust
# use renox::prelude::*;
# #[derive(Model, serde::Serialize, Default)] #[model(table = "categories")] struct Category { id: i64, name: String, active: bool }
# #[derive(Model, serde::Serialize, Default)] #[model(table = "products")] struct Product { id: i64, category_id: Option<i64>, name: String, price: i64 }
# async fn demo(db: Db, q: String) -> Result {
let products = Product::query()
    .where_in_query("category_id", Category::query().where_(Category::ACTIVE.eq(true)), "id") // IN (SELECT id FROM categories …)
    // Only when `q` isn't empty: the name contains `q`, OR the price is under 10,000.
    .when(!q.is_empty(), |query| {
        query.where_any(|any| any.where_(Product::NAME.like(&format!("%{q}%"))).where_(Product::PRICE.lt(10_000)))
    })
    .get(&db)
    .await?;
# let _ = products; Ok(()) }
```

What's going on:

- `where_in_query` keeps the products whose `category_id` is in the list of active
  categories' ids. The database works that list out itself (a sub-query).
- `when(condition, …)` adds the conditions inside only when the condition is true.
- `where_any` joins its conditions with OR: one of them is enough.
- `%` in `where_like` means "any text", so `%{q}%` finds `q` anywhere in the name.

## More of the query builder

Where a column is named, `Model::COLUMN` (`Product::PRICE.lt(10_000)`, `.order_by(Product::NAME)`) is checked by the compiler; a string name still works. See [types.md](types.md#typed-columns).

The query builder is the chain of methods you call on `Model::query()` (or `where_eq` and
friends) to say which rows you want. This table lists more of it. The left column has
Laravel's name for the same thing, for readers who know Laravel; the right column is what you
write in Renox.

| Laravel | Renox |
|---|---|
| `where(fn …)` / `orWhere` | `.where_any(\|q\| …)` (OR), `.where_all(\|q\| …)` (AND), nestable |
| `whereBetween`, `whereNotIn` | `.where_between(col, low, high)`, `.where_not_in(col, list)` |
| `when($cond, fn …)` | `.when(cond, \|q\| …)` |
| `sum`, `avg`, `min`, `max` | `.sum::<i64, _>(&db, col)`, `.avg(&db, col)`, `.min::<T, _>(…)`, `.max::<T, _>(…)` |
| `pluck` | `.pluck::<T, _>(&db, col)` |
| `update([...])` | `.update(&db, &[("status", &"paid")])`, which sets `updated_at` too |
| `increment` / `decrement` | `.increment(&db, "stock", -1)` |
| `firstOrFail`, `firstOrCreate` | `.first_or_404(&db)`, `.first_or_create(&db, \|\| new)` |
| `chunk` | `.chunk(&db, 1000, \|rows\| async { … })` (in id order; the query's own order and limit are ignored) |
| Scout's `Post::search('…')` | `Post::search(&q)` on a model with `#[model(search = "…")]`; `.where_search`, `.order_by_relevance` ([search.md](search.md)) |
| `insert([...])`, `upsert` | `Model::insert_many(&db, rows)`, `Model::upsert(&db, rows, &["sku"], &["qty"])` |
| `with('category')` | `relations::belongs_to` / `has_many` / `Pivot::load_for` (above) |
| pivot `withPivot`, `withTimestamps`, `toggle`, `updateExistingPivot` | `Pivot::with_timestamps()`, `attach_with`, `load_with_pivot::<T, Row>`, `toggle`, `update_pivot` |
| `morphMany` (eager, for a page of parents) | `Morph::load_many` |
| `$post->comments()` (one parent's children, as a query) | `COMMENTABLE.of(&post, Comment::query())` (`Morph::of`) |
| `morphTo` | `Morph::parents::<P, _>` (one query per parent type) |
| `withCount` on a morph relation | `Morph::count_many` |
| model events / observers | `#[model(hooks)]` + `impl ModelHooks` (`saving`, `saved`, `deleting`, `deleted`) |
| `save()` of dirty columns, `update([...])` on a model | `model.save_changes(&db, &original)`, `model.save_only(&db, &["price"])` |
| `withCount`, `withSum` | `relations::count_many(&db, &posts, Comment::query(), "post_id")`, `sum_many::<i64, _, _>(…, "total")` (0 for rows without children) |
| `whereHas`, `whereDoesntHave` | `.where_has(Comment::where_eq("approved", true), "post_id")`, `.where_doesnt_have(…)` (EXISTS) |
| `whereNotIn(fn …)` (sub-query) | `.where_not_in_query(col, Other::query(), "col")` |
| `whereRaw`, `orderByRaw` | `.where_raw("DATE(created_at) = DATE(?)", [value])`, `.order_by_raw("total DESC")` |
| `groupBy`, `having`, `selectRaw` | `.group_by(col).having_raw("COUNT(*) > ?", [2]).select_as::<(i64, i64), _>(&db, "col, COUNT(*)")`; group by an expression with `.group_by_raw("strftime('%Y', created_at)")` |
| `lockForUpdate`, `sharedLock` | `.lock_for_update()` / `.shared_lock()` on `&mut tx` (PostgreSQL; SQLite: `db.begin_immediate()`). Only calls that read rows lock them: `get`, `first`, `pluck`, `select_as` and a page's items, not `count`, `exists`, `sum`/`avg`/`min`/`max`, `update`, `increment` or `delete` |
| `firstOrNew`, `updateOrCreate`, `refresh` | `.first_or_new(&db, \|\| new)`, `.update_or_create(&db, \|\| new, \|m\| …)`, `model.refresh(&db)` |
| `HasUlids`, `HasUuids`, string keys | `id: Ulid` / `id: Uuid` / `id: String` (made on insert, or set by you); `Pivot<Ulid, i64>` |
| `insert()` of one model with its key | `model.insert(&db)` (always an INSERT; `Model::insert_many` for many rows) |
| `encrypted` cast | `Encrypted<T>` fields |
| `DB::transaction` inside a transaction | `tx.savepoint(\|tx\| Box::pin(async move { … }))` |
| `simplePaginate`, `cursorPaginate` | `.simple_paginate(&db, page, per)` (`simple_pagination` macro), `.cursor_paginate(&db, cursor, per)` (newest id first; the query's own order is ignored). Like `paginate`, they give at most 1000 rows per page |
| `DB::transaction(fn, 3)` | `db.retrying(3, \|\| async { let mut tx = db.begin().await?; … })` (borrows from the caller), `db.transaction_retrying(…)`, `db.transaction(…)` |
| `toSql` | `.to_sql(db.dialect())` |

The builder checks what you give it. Column names must be the model's fields, and `where_op`
takes only `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=`, `like` and `not like`. Anything else (and an
`update` that sets `id`) fails when the query runs, with an "invalid query" error, so a name
from user input can never turn into SQL. A `where_in` list longer than 1000 values is sent as
one JSON array, so a long list still works. A field marked `#[model(skip)]` isn't a column:
the model never writes it, and it holds its `Default` value after a load.

A few words from the table, in plain English:

- **OR / AND:** `where_any` keeps a row when *any* of its conditions is true; `where_all` only
  when *all* of them are. You can put one inside another.
- **sum, avg, min, max, pluck:** a total, an average, the smallest or largest value of a
  column, or just that one column's values as a list.
- **upsert:** insert the rows, or update them when a row with the same `sku` already exists.
- **dirty columns:** the fields you changed since you read the row. `save_changes` writes only
  those.
- **EXISTS:** `where_has` keeps rows that have at least one matching child (posts with an
  approved comment); `where_doesnt_have` keeps the ones that have none.
- **raw:** `where_raw` and `order_by_raw` take a piece of SQL as you write it. Pass values with
  `?` and a list, never by pasting them into the text.
- **lock:** `lock_for_update` stops other requests from changing the rows until your
  transaction ends. It locks the rows a call reads (`get`, `first`…), so a `count` or an
  `update` with it locks nothing.
- **transaction:** a group of changes that all happen, or none do. A **savepoint** is a smaller
  transaction inside one: if its part fails, only that part is undone.
- **cursor pagination:** instead of page numbers, the next page starts after the last row you
  saw. It stays fast on very large tables.
- **`to_sql`:** shows the SQL a query would run, which helps when debugging.
