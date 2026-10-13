use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::{Database, KeyType};

/// Creates `<timestamp>_<name>.up.sql` and `.down.sql`.
pub fn migration(name: &str, dir: &Path) -> Result<()> {
    migration_keyed(name, dir, KeyType::Integer)
}

/// `migration`, with the `id` column of a `create_<table>_table` template
/// for a model keyed by `key`.
pub fn migration_keyed(name: &str, dir: &Path, key: KeyType) -> Result<()> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('_')
        && !name.contains("__")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        bail!("`{name}` is not a valid migration name: use snake_case, e.g. create_product_table");
    }

    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let stem = format!("{}_{name}", next_version(dir, chrono::Utc::now()));
    let (up_sql, down_sql) = template(name, Database::of_current_app(), key);
    for (suffix, contents) in [("up", up_sql), ("down", down_sql)] {
        let path = dir.join(format!("{stem}.{suffix}.sql"));
        if path.exists() {
            bail!("{} already exists", path.display());
        }
        fs::write(&path, contents)
            .with_context(|| format!("could not write {}", path.display()))?;
        println!("Created {}", path.display());
    }
    Ok(())
}

/// A migration with the SQL given (for generators that know the table).
pub fn migration_with(name: &str, dir: &Path, up: &str, down: &str) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let stem = format!("{}_{name}", next_version(dir, chrono::Utc::now()));
    for (suffix, contents) in [("up", up), ("down", down)] {
        let path = dir.join(format!("{stem}.{suffix}.sql"));
        if path.exists() {
            bail!("{} already exists", path.display());
        }
        fs::write(&path, contents)
            .with_context(|| format!("could not write {}", path.display()))?;
        println!("Created {}", path.display());
    }
    Ok(())
}

/// The current time as `YYYYMMDDHHMMSS`, moved past the newest migration in
/// `dir`: migrations made in the same second (`make:model -m`, then
/// `make:migration`) still run in the order they were made.
fn next_version(dir: &Path, now: chrono::DateTime<chrono::Utc>) -> String {
    let now: u64 = now.format("%Y%m%d%H%M%S").to_string().parse().unwrap_or(0);
    let newest = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let digits = name.split('_').next()?;
            (digits.len() == 14).then(|| digits.parse::<u64>().ok())?
        })
        .max()
        .unwrap_or(0);
    // Past a `…59` second this isn't a valid time, but it only has to sort.
    format!("{:014}", now.max(newest + 1))
}

/// A starting point: a table for `create_<table>_table`, otherwise comments.
fn template(name: &str, database: Database, key: KeyType) -> (String, String) {
    let id = key.column(database);
    match name
        .strip_prefix("create_")
        .and_then(|rest| rest.strip_suffix("_table"))
    {
        Some(table) => (
            match database {
                // Quoted: tables are often named after SQL keywords (`order`, `user`).
                Database::Sqlite => format!(
                    "CREATE TABLE \"{table}\" (\n    \
                     {id},\n    \
                     created_at TEXT,\n    \
                     updated_at TEXT\n);\n"
                ),
                Database::Postgres => format!(
                    "CREATE TABLE \"{table}\" (\n    \
                     {id},\n    \
                     created_at TIMESTAMPTZ,\n    \
                     updated_at TIMESTAMPTZ\n);\n"
                ),
            },
            format!("DROP TABLE \"{table}\";\n"),
        ),
        None => (format!("-- {name}\n"), format!("-- Undo {name}\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_made_in_the_same_second_keep_their_order() {
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-28T10:00:00Z")
            .unwrap()
            .to_utc();
        assert_eq!(next_version(dir.path(), now), "20260928100000");
        fs::write(dir.path().join("20260928100000_create_a_table.up.sql"), "").unwrap();
        fs::write(dir.path().join("00010101000100_renox.up.sql"), "").unwrap();
        fs::write(dir.path().join("README.md"), "").unwrap();
        assert_eq!(next_version(dir.path(), now), "20260928100001");
        let later = now + chrono::Duration::minutes(1);
        assert_eq!(next_version(dir.path(), later), "20260928100100");
    }

    #[test]
    fn creates_table_templates() {
        let (up, down) = template("create_product_table", Database::Sqlite, KeyType::Integer);
        assert!(up.starts_with("CREATE TABLE \"product\" ("));
        assert!(up.contains("AUTOINCREMENT"));
        assert_eq!(down, "DROP TABLE \"product\";\n");
        let (up, _) = template("create_order_table", Database::Sqlite, KeyType::Integer);
        assert!(
            up.starts_with("CREATE TABLE \"order\" ("),
            "a keyword works as a table name"
        );
        let (up, _) = template("create_product_table", Database::Postgres, KeyType::Integer);
        assert!(up.contains("IDENTITY") && up.contains("TIMESTAMPTZ"));
        let (up, _) = template("create_invoice_table", Database::Sqlite, KeyType::Ulid);
        assert!(up.contains("    id TEXT PRIMARY KEY,\n"), "{up}");
        let (up, _) = template("create_invoice_table", Database::Postgres, KeyType::Uuid);
        assert!(up.contains("    id UUID PRIMARY KEY,\n"), "{up}");
        assert!(
            template("add_stock_to_product", Database::Sqlite, KeyType::Integer)
                .0
                .starts_with("-- ")
        );
    }

    #[test]
    fn writes_both_files() {
        let dir = tempfile_dir();
        migration("create_product_table", &dir).unwrap();
        let mut files: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("_create_product_table.down.sql"));
        assert!(files[1].ends_with("_create_product_table.up.sql"));
        assert!(migration("Bad Name", &dir).is_err());
        assert!(migration("create__table", &dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("renox-make-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }
}
