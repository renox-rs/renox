//! Full-text search over a model's text columns, with one API on both
//! databases: an FTS5 table on SQLite, a `tsvector` column on PostgreSQL.
//!
//! 1. Name the columns on the model: `#[model(search = "title, body")]`
//!    (columns listed first weigh more in the ranking).
//! 2. Create the index with a migration made by [`migration`].
//! 3. Search: `Post::search(&words)` gives a [`Query`](super::Query) of
//!    the matching rows, best first, that takes more filters, the default
//!    scope, soft deletes and pagination like any other.
//!
//! ```
//! # use renox::prelude::*;
//! #[derive(Model, serde::Serialize, Default)]
//! #[model(table = "posts", search = "title, body")]
//! struct Post { id: i64, title: String, body: String, published: bool }
//!
//! # fn app() -> App {
//! App::new().migrations(&[
//!     // After the migration that creates `posts`.
//!     renox::db::search::migration::<Post>("20260101000100_search_posts"),
//! ])
//! # }
//! # async fn demo(db: Db) -> Result {
//! let found = Post::search("coffee roast")
//!     .where_eq("published", true)
//!     .paginate(&db, 1, 20)
//!     .await?;
//! # let _ = found; Ok(()) }
//! ```
//!
//! **What a search matches:** every word of the text, as a word or the
//! start of one (`cof` finds "coffee"), ignoring case. Punctuation separates
//! words and is otherwise ignored, so user input is always safe to pass:
//! no search syntax reaches the database, only words, bound as a value.
//! A text without any word matches every row (no filter).
//!
//! **Language:** `english` by default, which matches word forms ("roasts"
//! finds "roasting"). `#[model(search_language = "simple")]` matches words
//! as typed, for other languages. On PostgreSQL the name is a text search
//! configuration (`spanish`, `german`, …: `\dF` in psql lists them);
//! SQLite stems only English, so any other name there matches as typed.
//! PostgreSQL's `english` also skips stop words ("the", "a").
//!
//! **Keeping the index current:** the database does it, so every way of
//! writing rows is covered: `save`, `delete`, but also bulk
//! `Query::update`/`delete`, `insert_many` and raw SQL, which skip model
//! hooks. On SQLite the migration adds triggers on the table; on
//! PostgreSQL the column is `GENERATED ALWAYS … STORED`. Soft-deleted rows
//! stay in the index; queries leave them out as usual.
//!
//! **What the migration creates**, for `#[model(table = "posts", search = "title, body")]`:
//! - SQLite: an FTS5 table `posts_search` reading its text from `posts`
//!   ("external content"), triggers `posts_search_insert`,
//!   `posts_search_update` and `posts_search_delete`, and fills it from the
//!   rows already there.
//! - PostgreSQL: a column `search_vector` (`tsvector`, generated from the
//!   columns) and a GIN index `posts_search_index` on it.
//!
//! Changed the searchable columns? Add another migration made by
//! [`migration`] under a new name: it replaces the index (rolling it back
//! removes the index).

use super::{Dialect, Executor, Migration, Model, quote, sql};
use crate::Result;
use anyhow::anyhow;

/// At most this many words of a search are used.
const MAX_TERMS: usize = 16;
/// Longer words are cut to this many characters.
const MAX_TERM_CHARS: usize = 64;

/// The words of `text` (letters and digits; anything else separates
/// them), joined by single spaces: the one value a search binds. `None`
/// when there is no word. Keeping only letters and digits is what makes
/// user input safe in FTS5's and `to_tsquery`'s syntax.
pub(crate) fn terms(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(MAX_TERMS)
        .map(|w| w.chars().take(MAX_TERM_CHARS).collect())
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Why `M` can't be searched, if it can't.
pub(crate) fn unsearchable<M: Model>() -> Option<String> {
    if M::SEARCHABLE.is_empty() {
        return Some(format!(
            "`{}` has no full-text search: add #[model(search = \"…\")]",
            M::TABLE
        ));
    }
    if !valid_language(M::SEARCH_LANGUAGE) {
        return Some(format!(
            "search language `{}` isn't a plain name (letters and `_`)",
            M::SEARCH_LANGUAGE
        ));
    }
    None
}

fn valid_language(language: &str) -> bool {
    !language.is_empty() && language.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// The FTS5 table's name on SQLite.
fn fts_table<M: Model>() -> String {
    format!("{}_search", M::TABLE)
}

/// The value `Query::search` binds: the words (`terms`), then, after a
/// unit separator, the text as typed (control characters left out, at most
/// 200 characters). One value for both databases; each takes what it needs.
pub(crate) fn bound(text: &str) -> Option<String> {
    let words = terms(text)?;
    let typed: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
    Some(format!("{words}{SEPARATOR}{}", typed.trim()))
}

/// Between the words and the typed text in `bound`'s value.
const SEPARATOR: char = '\u{1f}';

/// The match expression for the one bound value (`bound`).
///
/// SQLite: each word as a prefix, all of them required. PostgreSQL: the
/// same, or the text as typed read by PostgreSQL's own parser, which splits
/// it the way it split the row: it reads `-1` in `GIR-JER-0001-1` and
/// `-12.5` in `SKU-12.5` as signed numbers, so a code typed whole matches
/// only that way (#304). Built in SQL from the bound value, read once in a
/// subquery so the statement has a single `?`.
fn match_expression(dialect: Dialect) -> &'static str {
    match dialect {
        // "cof"* "tea"*
        Dialect::Sqlite => {
            "(SELECT '\"' || replace(substr(v, 1, instr(v, char(31)) - 1), ' ', '\"* \"') || '\"*' \
             FROM (SELECT ? AS v))"
        }
        // cof:* & tea:*, or what PostgreSQL makes of the typed text
        Dialect::Postgres => {
            "(SELECT to_tsquery(c, replace(split_part(v, chr(31), 1), ' ', ':* & ') || ':*') \
             || plainto_tsquery(c, split_part(v, chr(31), 2)) FROM (SELECT ?::text AS v, %CONFIG% AS c) AS s)"
        }
    }
}

/// The language as a SQL literal; `unsearchable` has checked it is only
/// lowercase letters and `_` (it comes from the model, never from input).
fn config_literal<M: Model>() -> String {
    format!("'{}'::regconfig", M::SEARCH_LANGUAGE)
}

/// `WHERE` condition: the row matches the search (one `?`).
pub(crate) fn filter_sql<M: Model>(dialect: Dialect) -> String {
    match dialect {
        Dialect::Sqlite => format!(
            "{}.rowid IN (SELECT rowid FROM {fts} WHERE {fts} MATCH {})",
            quote(M::TABLE),
            match_expression(dialect),
            fts = quote(&fts_table::<M>()),
        ),
        Dialect::Postgres => format!(
            "{}.\"search_vector\" @@ {}",
            quote(M::TABLE),
            match_expression(dialect).replace("%CONFIG%", &config_literal::<M>())
        ),
    }
}

/// The weight of the column at `index` (A, B, C, D on PostgreSQL).
fn weight(index: usize) -> (char, &'static str) {
    match index {
        0 => ('A', "1.0"),
        1 => ('B', "0.4"),
        2 => ('C', "0.2"),
        _ => ('D', "0.1"),
    }
}

/// `ORDER BY` term: best matches first (one `?`). Rows that don't match
/// come last.
pub(crate) fn rank_sql<M: Model>(dialect: Dialect) -> String {
    match dialect {
        Dialect::Sqlite => {
            // bm25 is negative, lower is better; non-matches get 0.
            let weights: Vec<&str> = (0..M::SEARCHABLE.len()).map(|i| weight(i).1).collect();
            format!(
                "COALESCE((SELECT bm25({fts}, {}) FROM {fts} WHERE {fts} MATCH {} AND {fts}.rowid = {}.rowid), 0) ASC",
                weights.join(", "),
                match_expression(dialect),
                quote(M::TABLE),
                fts = quote(&fts_table::<M>()),
            )
        }
        Dialect::Postgres => format!(
            "ts_rank({}.\"search_vector\", {}) DESC",
            quote(M::TABLE),
            match_expression(dialect).replace("%CONFIG%", &config_literal::<M>())
        ),
    }
}

/// A string literal in SQL.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn sqlite_up<M: Model>() -> String {
    let table = quote(M::TABLE);
    let fts = quote(&fts_table::<M>());
    let columns: Vec<String> = M::SEARCHABLE.iter().map(|c| quote(c)).collect();
    let columns = columns.join(", ");
    let new: Vec<String> = M::SEARCHABLE
        .iter()
        .map(|c| format!("new.{}", quote(c)))
        .collect();
    let old: Vec<String> = M::SEARCHABLE
        .iter()
        .map(|c| format!("old.{}", quote(c)))
        .collect();
    let (new, old) = (new.join(", "), old.join(", "));
    let tokenize = if M::SEARCH_LANGUAGE == "english" {
        "porter unicode61 remove_diacritics 2"
    } else {
        "unicode61 remove_diacritics 2"
    };
    let trigger = |suffix: &str| quote(&format!("{}_search_{suffix}", M::TABLE));
    format!(
        "{drop}\n\
         CREATE VIRTUAL TABLE {fts} USING fts5({columns}, content={content}, tokenize={tokenize});\n\
         CREATE TRIGGER {insert} AFTER INSERT ON {table} BEGIN\n\
         \x20   INSERT INTO {fts}(rowid, {columns}) VALUES (new.rowid, {new});\n\
         END;\n\
         CREATE TRIGGER {delete} AFTER DELETE ON {table} BEGIN\n\
         \x20   INSERT INTO {fts}({fts}, rowid, {columns}) VALUES ('delete', old.rowid, {old});\n\
         END;\n\
         CREATE TRIGGER {update} AFTER UPDATE OF \"id\", {columns} ON {table} BEGIN\n\
         \x20   INSERT INTO {fts}({fts}, rowid, {columns}) VALUES ('delete', old.rowid, {old});\n\
         \x20   INSERT INTO {fts}(rowid, {columns}) VALUES (new.rowid, {new});\n\
         END;\n\
         INSERT INTO {fts}({fts}) VALUES ('rebuild');",
        drop = sqlite_down::<M>(),
        content = literal(M::TABLE),
        tokenize = literal(tokenize),
        insert = trigger("insert"),
        delete = trigger("delete"),
        update = trigger("update"),
    )
}

fn sqlite_down<M: Model>() -> String {
    let trigger = |suffix: &str| quote(&format!("{}_search_{suffix}", M::TABLE));
    format!(
        "DROP TRIGGER IF EXISTS {};\nDROP TRIGGER IF EXISTS {};\nDROP TRIGGER IF EXISTS {};\nDROP TABLE IF EXISTS {};",
        trigger("insert"),
        trigger("update"),
        trigger("delete"),
        quote(&fts_table::<M>())
    )
}

fn postgres_up<M: Model>() -> String {
    let vector: Vec<String> = M::SEARCHABLE
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "setweight(to_tsvector({}, coalesce({}::text, '')), '{}')",
                config_literal::<M>(),
                quote(c),
                weight(i).0
            )
        })
        .collect();
    format!(
        "{drop}\n\
         ALTER TABLE {table} ADD COLUMN \"search_vector\" tsvector GENERATED ALWAYS AS ({vector}) STORED;\n\
         CREATE INDEX {index} ON {table} USING GIN (\"search_vector\");",
        drop = postgres_down::<M>(),
        table = quote(M::TABLE),
        vector = vector.join(" || "),
        index = quote(&format!("{}_search_index", M::TABLE)),
    )
}

fn postgres_down<M: Model>() -> String {
    format!(
        "DROP INDEX IF EXISTS {};\nALTER TABLE {} DROP COLUMN IF EXISTS \"search_vector\";",
        quote(&format!("{}_search_index", M::TABLE)),
        quote(M::TABLE)
    )
}

/// A migration that creates `M`'s full-text index (and removes it when
/// rolled back), from the model's `#[model(search = …)]` columns and
/// language. Name it so it sorts after the migration creating the table,
/// and register it with [`App::migrations`](crate::App::migrations).
/// Running it again under a new name replaces the index, e.g. after the
/// searchable columns changed. The module docs list what it creates.
///
/// Call it once, while building the app: the SQL it makes lives as long
/// as the program.
///
/// # Panics
///
/// When `M` has no searchable columns.
pub fn migration<M: Model>(name: &'static str) -> Migration {
    if let Some(problem) = unsearchable::<M>() {
        panic!("renox::db::search::migration: {problem}");
    }
    let leak = |sql: String| -> &'static str { Box::leak(sql.into_boxed_str()) };
    Migration::new(name, "", None)
        .sqlite(leak(sqlite_up::<M>()), Some(leak(sqlite_down::<M>())))
        .postgres(leak(postgres_up::<M>()), Some(leak(postgres_down::<M>())))
}

/// Fills `M`'s full-text index again from the table's rows. The index
/// keeps itself current, so this is for repairs: rows written while its
/// triggers were dropped, or a restored backup of the table alone. On
/// PostgreSQL the generated column is always current, so it does nothing.
///
/// ```
/// # use renox::prelude::*;
/// # #[derive(Model, serde::Serialize, Default)]
/// # #[model(table = "posts", search = "title, body")]
/// # struct Post { id: i64, title: String, body: String }
/// # async fn demo(db: Db) -> Result {
/// renox::db::search::rebuild::<Post>(&db).await?;
/// # Ok(()) }
/// ```
pub async fn rebuild<'c, M: Model>(db: impl Executor<'c>) -> Result {
    if let Some(problem) = unsearchable::<M>() {
        return Err(anyhow!("{problem}").into());
    }
    let db = db.into_conn();
    if db.dialect() == Dialect::Sqlite {
        let fts = quote(&fts_table::<M>());
        sql(format!("INSERT INTO {fts}({fts}) VALUES ('rebuild')"))
            .execute(db)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::terms;

    #[test]
    fn terms_keep_only_words() {
        assert_eq!(terms("  Coffee, roast!  "), Some("Coffee roast".into()));
        assert_eq!(
            terms("\"a\" OR b* NEAR(c) -d"),
            Some("a OR b NEAR c d".into())
        );
        assert_eq!(
            terms("it's x'; DROP TABLE posts; --"),
            Some("it s x DROP TABLE posts".into())
        );
        assert_eq!(terms("café 東京"), Some("café 東京".into()));
        assert_eq!(terms("!!! ... ---"), None);
        assert_eq!(terms(""), None);
        let many = "w ".repeat(40);
        assert_eq!(terms(&many).unwrap().split(' ').count(), 16);
        assert_eq!(terms(&"x".repeat(100)).unwrap().len(), 64);
    }
}
