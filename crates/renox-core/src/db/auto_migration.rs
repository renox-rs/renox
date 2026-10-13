//! `db:diff`: writes the migration files for what changed in the registered
//! models, by comparing them with the schema the migrations build.

use std::fs;
use std::path::Path;

use anyhow::{Context, anyhow};

use super::schema_check::{
    ForeignKey, foreign_keys_to, sqlite_table_sql, table_columns, table_indexes,
};
use super::schema_diff::{Change, ModelState, TableState, plan, resolve};
use super::schema_sql::{SqliteContext, postgres, sqlite};
use super::{Dialect, drop_scratch};
use crate::app::Kernel;
use crate::command::Args;

/// Runs `db:diff [name] [--yes] [--path DIR]`.
pub(crate) async fn run(kernel: &Kernel, args: &[String]) -> crate::Result {
    let args = Args::new(args.iter().cloned());
    let name = args
        .positional()
        .first()
        .map_or("update_schema", |n| *n)
        .to_owned();
    check_name(&name)?;
    let yes = args.has("--yes");
    let dir = args.value("--path").unwrap_or("migrations").to_owned();

    let db = kernel.scratch_db().await?;
    let result = compute(kernel, &db, yes).await;
    drop_scratch(&db).await;
    let Some((changes, sqlite_files, postgres_files)) = result? else {
        println!("Nothing to change.");
        return Ok(());
    };

    let dir = Path::new(&dir);
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let stem = format!("{}_{name}", next_version(dir));
    let mut files = vec![
        (format!("{stem}.up.sql"), sqlite_files.0),
        (format!("{stem}.down.sql"), sqlite_files.1),
    ];
    if let Some((up, down)) = postgres_files {
        files.push((format!("{stem}.postgres.up.sql"), up));
        files.push((format!("{stem}.postgres.down.sql"), down));
    }
    for (file, _) in &files {
        let path = dir.join(file);
        if path.exists() {
            return Err(anyhow!("{} already exists", path.display()).into());
        }
    }
    for (file, contents) in &files {
        let path = dir.join(file);
        fs::write(&path, contents)
            .with_context(|| format!("could not write {}", path.display()))?;
        println!("Created {}", path.display());
    }
    println!("Changes:");
    for change in &changes {
        println!("  {}", describe(change));
    }
    Ok(())
}

/// The same rule as `rnx make:migration`.
fn check_name(name: &str) -> crate::Result {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('_')
        && !name.contains("__")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        return Err(anyhow!(
            "`{name}` is not a valid migration name: use snake_case, e.g. update_schema"
        )
        .into());
    }
    Ok(())
}

type Files = (String, String);

/// The changes, the scratch engine's `(up, down)` and, for SQLite apps whose
/// PostgreSQL text differs, that text.
type Computed = (Vec<Change>, Files, Option<Files>);

async fn compute(kernel: &Kernel, db: &super::Db, yes: bool) -> crate::Result<Option<Computed>> {
    let dialect = db.dialect();
    let models: Vec<ModelState> = kernel
        .models()
        .iter()
        .filter_map(|info| {
            let columns = (info.columns)();
            // A hand-written model describes nothing: `db:check` reports it.
            (!columns.is_empty()).then(|| ModelState {
                table: info.table,
                columns,
                indexes: (info.indexes)(),
            })
        })
        .collect();
    let mut tables = Vec::with_capacity(models.len());
    let mut cx = SqliteContext::default();
    for model in &models {
        let columns = table_columns(db, model.table).await?;
        if columns.is_empty() {
            tables.push(None);
            continue;
        }
        let state = TableState {
            table: model.table.to_owned(),
            columns,
            indexes: table_indexes(db, model.table).await?,
        };
        if dialect == Dialect::Sqlite {
            if let Some(text) = sqlite_table_sql(db, model.table).await? {
                cx.table_sql.insert(model.table.to_owned(), text);
            }
            let referenced: Vec<ForeignKey> = foreign_keys_to(db, model.table).await?;
            cx.referenced_by.insert(model.table.to_owned(), referenced);
            cx.tables.insert(model.table.to_owned(), state.clone());
        }
        tables.push(Some(state));
    }
    let plan = plan(dialect, &models, &tables)?;

    if yes
        && (!plan.rename_candidates.is_empty()
            || !plan.column_drops.is_empty()
            || !plan.index_drops.is_empty())
    {
        let mut lines = Vec::new();
        for (t, from, to) in &plan.rename_candidates {
            lines.push(format!(
                "  `{t}.{from}` may have been renamed to `{t}.{to}`"
            ));
        }
        for (t, c) in &plan.column_drops {
            lines.push(format!("  drop column `{t}.{c}`"));
        }
        for (_, index) in &plan.index_drops {
            lines.push(format!("  drop index `{}`", index.name));
        }
        return Err(anyhow!(
            "--yes never renames or drops anything; run without it to answer these:\n{}",
            lines.join("\n")
        )
        .into());
    }

    let mut renames: Vec<(String, String, String)> = Vec::new();
    for (t, from, to) in &plan.rename_candidates {
        let taken = renames
            .iter()
            .any(|(rt, rf, rto)| rt == t && (rf == from || rto == to));
        if taken {
            continue;
        }
        let question = format!("Was `{t}.{from}` renamed to `{t}.{to}`?");
        if crate::prompt::confirm(&question, false).await? {
            renames.push((t.clone(), from.clone(), to.clone()));
        }
    }
    let mut column_drops = Vec::new();
    for (t, c) in &plan.column_drops {
        if renames.iter().any(|(rt, rf, _)| rt == t && rf == c) {
            continue;
        }
        let question = format!("Drop column `{t}.{c}` (its data is lost)?");
        if crate::prompt::confirm(&question, false).await? {
            column_drops.push((t.clone(), c.clone()));
        }
    }
    let mut index_drops = Vec::new();
    for (_, index) in &plan.index_drops {
        if crate::prompt::confirm(&format!("Drop index `{}`?", index.name), false).await? {
            index_drops.push(index.name.clone());
        }
    }
    let changes = resolve(plan, &renames, &column_drops, &index_drops);
    if changes.is_empty() {
        return Ok(None);
    }

    match dialect {
        Dialect::Postgres => {
            let files = postgres(&changes);
            Ok(Some((changes, files, None)))
        }
        Dialect::Sqlite => {
            let files = sqlite(&changes, &cx)?;
            let pg = postgres(&changes);
            let pg = (pg != files).then_some(pg);
            Ok(Some((changes, files, pg)))
        }
    }
}

/// One line for the summary.
fn describe(change: &Change) -> String {
    match change {
        Change::CreateTable { model } => format!("create table `{}`", model.table),
        Change::AddColumn { table, column } => format!("add column `{table}.{}`", column.name),
        Change::DropColumn { table, old } => format!("drop column `{table}.{}`", old.name),
        Change::AlterColumn { table, new, .. } => format!("change column `{table}.{}`", new.name),
        Change::RenameColumn { table, from, to } => {
            format!("rename column `{table}.{from}` to `{table}.{to}`")
        }
        Change::CreateIndex { name, .. } => format!("create index `{name}`"),
        Change::DropIndex { old, .. } => format!("drop index `{}`", old.name),
    }
}

/// The current time as `YYYYMMDDHHMMSS`, moved past the newest migration in
/// `dir` (the same rule as `rnx make:migration`).
fn next_version(dir: &Path) -> String {
    let now: chrono::DateTime<chrono::Utc> = crate::clock::system_now().into();
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
    format!("{:014}", now.max(newest + 1))
}
