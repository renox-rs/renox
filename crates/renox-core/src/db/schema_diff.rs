//! Turns models plus the current schema into a list of changes: the logic
//! behind `db:diff`. No SQL is written here.

use anyhow::anyhow;

use super::schema_check::{TableColumn, TableIndex, kind_fits};
use super::{ColumnKind, Dialect, ModelColumn, ModelIndex};

/// A table as the database has it.
#[derive(Debug, Clone)]
pub(crate) struct TableState {
    #[allow(dead_code)] // kept for debugging output
    pub table: String,
    pub columns: Vec<TableColumn>,
    pub indexes: Vec<TableIndex>,
}

/// A model as the code declares it.
#[derive(Debug, Clone)]
pub(crate) struct ModelState {
    pub table: &'static str,
    pub columns: Vec<ModelColumn>,
    pub indexes: Vec<ModelIndex>,
}

/// One step from the current schema to the models'.
#[derive(Debug, Clone)]
pub(crate) enum Change {
    CreateTable {
        model: ModelState,
    },
    AddColumn {
        table: String,
        column: ModelColumn,
    },
    DropColumn {
        table: String,
        old: TableColumn,
    },
    AlterColumn {
        table: String,
        old: TableColumn,
        new: ModelColumn,
    },
    RenameColumn {
        table: String,
        from: String,
        to: String,
    },
    CreateIndex {
        table: String,
        name: String,
        columns: Vec<String>,
        unique: bool,
    },
    DropIndex {
        table: String,
        old: TableIndex,
    },
}

/// The changes found, and the choices that need an answer.
#[derive(Debug, Clone, Default)]
pub(crate) struct Plan {
    pub changes: Vec<Change>,
    /// (table, dropped column, added column) that may be a rename.
    pub rename_candidates: Vec<(String, String, String)>,
    /// (table, column) the model no longer has.
    pub column_drops: Vec<(String, String)>,
    /// (table, index) that follows the naming convention but isn't declared.
    pub index_drops: Vec<(String, TableIndex)>,
    /// The table's columns behind `column_drops`, for `DropColumn`.
    dropped: Vec<(String, TableColumn)>,
}

/// The conventional name of an index: `{table}_{cols}_index` or `…_unique`.
pub(crate) fn index_name(table: &str, columns: &[impl AsRef<str>], unique: bool) -> String {
    let cols: Vec<&str> = columns.iter().map(AsRef::as_ref).collect();
    format!(
        "{table}_{}_{}",
        cols.join("_"),
        if unique { "unique" } else { "index" }
    )
}

fn unknown(table: &str, column: &ModelColumn) -> crate::Error {
    crate::Error::Internal(anyhow!(
        "`{table}.{}` has the type `{}`, which Renox can't map to a column: \
         implement `renox::db::ColumnType` for it",
        column.name,
        column.rust_type
    ))
}

/// Compares `models` with `tables` (same order; `None` = no such table).
pub(crate) fn plan(
    dialect: Dialect,
    models: &[ModelState],
    tables: &[Option<TableState>],
) -> crate::Result<Plan> {
    for (i, a) in models.iter().enumerate() {
        if let Some(b) = models[..i].iter().find(|b| b.table == a.table) {
            return Err(crate::Error::Internal(anyhow!(
                "two models use the table `{}` ({} columns and {} columns declared): \
                 register one model per table",
                a.table,
                b.columns.len(),
                a.columns.len()
            )));
        }
    }
    let mut plan = Plan::default();
    for (model, state) in models.iter().zip(tables) {
        let table = model.table;
        let Some(state) = state else {
            if let Some(c) = model.columns.iter().find(|c| c.kind == ColumnKind::Unknown) {
                return Err(unknown(table, c));
            }
            plan.changes.push(Change::CreateTable {
                model: model.clone(),
            });
            for index in &model.indexes {
                plan.changes.push(create_index(table, index));
            }
            continue;
        };
        let mut added = Vec::new();
        for column in &model.columns {
            let Some(found) = state.columns.iter().find(|t| t.name == column.name) else {
                if column.kind == ColumnKind::Unknown {
                    return Err(unknown(table, column));
                }
                if !column.nullable && column.default.is_none() {
                    return Err(crate::Error::Internal(anyhow!(
                        "`{table}.{}` is a new NOT NULL column without a default: \
                         add `#[model(default = …)]` or make it `Option<{}>`",
                        column.name,
                        column.rust_type
                    )));
                }
                added.push(column);
                plan.changes.push(Change::AddColumn {
                    table: table.to_owned(),
                    column: column.clone(),
                });
                continue;
            };
            let is_id = column.name == "id";
            let type_differs = column.kind != ColumnKind::Unknown
                && !kind_fits(dialect, column.kind, &found.sql_type);
            let null_differs = !is_id && column.nullable == found.not_null;
            if type_differs || null_differs {
                if is_id {
                    return Err(crate::Error::Internal(anyhow!(
                        "`{table}.id` doesn't match the model's key type: \
                         change the key by hand in a migration"
                    )));
                }
                if column.kind == ColumnKind::Unknown {
                    return Err(unknown(table, column));
                }
                plan.changes.push(Change::AlterColumn {
                    table: table.to_owned(),
                    old: found.clone(),
                    new: column.clone(),
                });
            }
        }
        let extra: Vec<&TableColumn> = state
            .columns
            .iter()
            .filter(|t| !model.columns.iter().any(|c| c.name == t.name))
            .collect();
        for dropped in &extra {
            plan.column_drops
                .push((table.to_owned(), dropped.name.clone()));
            plan.dropped.push((table.to_owned(), (*dropped).clone()));
            for new in &added {
                if new.kind != ColumnKind::Unknown
                    && kind_fits(dialect, new.kind, &dropped.sql_type)
                {
                    plan.rename_candidates.push((
                        table.to_owned(),
                        dropped.name.clone(),
                        new.name.to_owned(),
                    ));
                }
            }
        }
        for index in &model.indexes {
            let exists = state
                .indexes
                .iter()
                .any(|t| t.unique == index.unique && t.columns == index.columns);
            if !exists {
                plan.changes.push(create_index(table, index));
            }
        }
        for old in &state.indexes {
            let declared = model
                .indexes
                .iter()
                .any(|i| i.unique == old.unique && old.columns == i.columns);
            if !declared && old.name == index_name(table, &old.columns, old.unique) {
                plan.index_drops.push((table.to_owned(), old.clone()));
            }
        }
    }
    Ok(plan)
}

fn create_index(table: &str, index: &ModelIndex) -> Change {
    Change::CreateIndex {
        table: table.to_owned(),
        name: index_name(table, index.columns, index.unique),
        columns: index.columns.iter().map(|c| (*c).to_owned()).collect(),
        unique: index.unique,
    }
}

/// Applies the answers: accepted renames, column drops (by table and
/// column) and index drops (by name).
pub(crate) fn resolve(
    plan: Plan,
    renames: &[(String, String, String)],
    column_drops: &[(String, String)],
    index_drops: &[String],
) -> Vec<Change> {
    let mut changes: Vec<Change> = plan
        .changes
        .into_iter()
        .filter(|change| match change {
            Change::AddColumn { table, column } => !renames
                .iter()
                .any(|(t, _, to)| t == table && to == column.name),
            _ => true,
        })
        .collect();
    for (table, from, to) in renames {
        changes.push(Change::RenameColumn {
            table: table.clone(),
            from: from.clone(),
            to: to.clone(),
        });
    }
    for (table, old) in plan.dropped {
        let renamed = renames
            .iter()
            .any(|(t, from, _)| *t == table && *from == old.name);
        let accepted = column_drops
            .iter()
            .any(|(t, c)| *t == table && *c == old.name);
        if accepted && !renamed {
            changes.push(Change::DropColumn { table, old });
        }
    }
    for (table, old) in plan.index_drops {
        if index_drops.contains(&old.name) {
            changes.push(Change::DropIndex { table, old });
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ColumnKind as K;

    const S: Dialect = Dialect::Sqlite;

    fn col(name: &'static str, kind: K, nullable: bool) -> ModelColumn {
        ModelColumn::new(name, "T", kind, nullable)
    }

    fn tcol(name: &str, ty: &str, not_null: bool) -> TableColumn {
        TableColumn {
            name: name.to_owned(),
            sql_type: ty.to_owned(),
            not_null,
            default: None,
            primary_key: name == "id",
            identity: false,
        }
    }

    fn tindex(name: &str, columns: &[&str], unique: bool) -> TableIndex {
        TableIndex {
            name: name.to_owned(),
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
            unique,
            sql: Some(format!("CREATE INDEX {name}")),
        }
    }

    fn model(columns: Vec<ModelColumn>, indexes: Vec<ModelIndex>) -> ModelState {
        ModelState {
            table: "things",
            columns,
            indexes,
        }
    }

    fn table(columns: Vec<TableColumn>, indexes: Vec<TableIndex>) -> Option<TableState> {
        Some(TableState {
            table: "things".into(),
            columns,
            indexes,
        })
    }

    fn id() -> (ModelColumn, TableColumn) {
        (col("id", K::BigInt, false), tcol("id", "INTEGER", false))
    }

    fn run(m: ModelState, t: Option<TableState>) -> crate::Result<Plan> {
        plan(S, &[m], &[t])
    }

    fn message(r: crate::Result<Plan>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn a_missing_table_is_created_with_its_indexes() {
        let (mid, _) = id();
        let m = model(vec![mid], vec![ModelIndex::new(&["a", "b"], false)]);
        let p = run(m, None).unwrap();
        assert_eq!(p.changes.len(), 2);
        assert!(matches!(p.changes[0], Change::CreateTable { .. }));
        let Change::CreateIndex { name, unique, .. } = &p.changes[1] else {
            panic!("expected an index");
        };
        assert_eq!(name, "things_a_b_index");
        assert!(!unique);
    }

    #[test]
    fn a_missing_column_is_added() {
        let (mid, tid) = id();
        let m = model(vec![mid, col("note", K::Text, true)], vec![]);
        let p = run(m, table(vec![tid], vec![])).unwrap();
        assert!(matches!(&p.changes[0], Change::AddColumn { column, .. } if column.name == "note"));
        assert!(p.column_drops.is_empty());
    }

    #[test]
    fn a_type_change_is_altered() {
        let (mid, tid) = id();
        let m = model(vec![mid, col("n", K::Text, false)], vec![]);
        let p = run(m, table(vec![tid, tcol("n", "INTEGER", true)], vec![])).unwrap();
        assert!(matches!(&p.changes[0], Change::AlterColumn { new, .. } if new.name == "n"));
    }

    #[test]
    fn a_nullability_change_is_altered() {
        let (mid, tid) = id();
        let m = model(vec![mid, col("n", K::Text, true)], vec![]);
        let p = run(m, table(vec![tid, tcol("n", "TEXT", true)], vec![])).unwrap();
        assert!(matches!(&p.changes[0], Change::AlterColumn { .. }));
    }

    #[test]
    fn a_matching_table_has_no_changes() {
        let (mid, tid) = id();
        let m = model(vec![mid, col("n", K::Text, true)], vec![]);
        let p = run(m, table(vec![tid, tcol("n", "TEXT", false)], vec![])).unwrap();
        assert!(p.changes.is_empty() && p.column_drops.is_empty());
    }

    #[test]
    fn a_dropped_and_an_added_column_of_one_kind_may_be_a_rename() {
        let (mid, tid) = id();
        let m = model(vec![mid, col("title", K::Text, true)], vec![]);
        let p = run(m, table(vec![tid, tcol("name", "TEXT", false)], vec![])).unwrap();
        assert_eq!(
            p.rename_candidates,
            vec![("things".into(), "name".into(), "title".into())]
        );
        assert_eq!(p.column_drops, vec![("things".into(), "name".into())]);

        let renamed = resolve(p.clone(), &p.rename_candidates, &p.column_drops, &[]);
        assert_eq!(renamed.len(), 1);
        assert!(
            matches!(&renamed[0], Change::RenameColumn { from, to, .. } if from == "name" && to == "title")
        );

        let kept = resolve(p.clone(), &[], &p.column_drops, &[]);
        assert!(matches!(kept[0], Change::AddColumn { .. }));
        assert!(matches!(kept[1], Change::DropColumn { .. }));
        assert_eq!(resolve(p, &[], &[], &[]).len(), 1);
    }

    #[test]
    fn a_column_the_model_lacks_is_a_drop_candidate() {
        let (mid, tid) = id();
        let m = model(vec![mid], vec![]);
        let p = run(m, table(vec![tid, tcol("old", "BLOB", false)], vec![])).unwrap();
        assert!(p.changes.is_empty() && p.rename_candidates.is_empty());
        assert_eq!(p.column_drops, vec![("things".into(), "old".into())]);
        let c = resolve(p.clone(), &[], &p.column_drops, &[]);
        assert!(matches!(&c[0], Change::DropColumn { old, .. } if old.name == "old"));
    }

    #[test]
    fn a_new_index_is_created_with_the_conventional_name() {
        let (mid, tid) = id();
        let m = model(vec![mid], vec![ModelIndex::new(&["slug"], true)]);
        let p = run(m, table(vec![tid], vec![])).unwrap();
        assert!(matches!(&p.changes[0],
            Change::CreateIndex { name, unique: true, .. } if name == "things_slug_unique"));
    }

    #[test]
    fn an_undeclared_conventional_index_is_a_drop_candidate() {
        let (mid, tid) = id();
        let m = model(vec![mid], vec![]);
        let idx = tindex("things_a_index", &["a"], false);
        let p = run(m, table(vec![tid], vec![idx])).unwrap();
        assert_eq!(p.index_drops.len(), 1);
        assert!(resolve(p.clone(), &[], &[], &[]).is_empty());
        let c = resolve(p, &[], &[], &["things_a_index".to_owned()]);
        assert!(matches!(&c[0], Change::DropIndex { old, .. } if old.name == "things_a_index"));
    }

    #[test]
    fn indexes_off_the_convention_are_left_alone() {
        let (mid, tid) = id();
        let m = model(vec![mid], vec![ModelIndex::new(&["a"], false)]);
        let idx = tindex("my_own_name", &["a"], false);
        let other = tindex("by_hand", &["b"], false);
        let p = run(m, table(vec![tid], vec![idx, other])).unwrap();
        assert!(p.changes.is_empty());
        assert!(p.index_drops.is_empty());
    }

    #[test]
    fn a_not_null_column_without_a_default_is_refused() {
        let (mid, tid) = id();
        let m = model(vec![mid, col("n", K::Int, false)], vec![]);
        let msg = message(run(m, table(vec![tid], vec![])));
        assert!(
            msg.contains("things.n") && msg.contains("#[model(default"),
            "{msg}"
        );
        let (mid, tid) = id();
        let m = model(vec![mid, col("n", K::Int, false).default_sql("0")], vec![]);
        assert!(run(m, table(vec![tid], vec![])).is_ok());
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        let (mid, tid) = id();
        let mut odd = col("odd", K::Unknown, true);
        odd.rust_type = "Weird";
        let msg = message(run(model(vec![mid.clone(), odd.clone()], vec![]), None));
        assert!(msg.contains("things.odd") && msg.contains("Weird"), "{msg}");
        let msg = message(run(
            model(vec![mid.clone(), odd.clone()], vec![]),
            table(vec![tid.clone()], vec![]),
        ));
        assert!(msg.contains("Weird"), "{msg}");
        let msg = message(run(
            model(vec![mid, odd], vec![]),
            table(vec![tid, tcol("odd", "TEXT", true)], vec![]),
        ));
        assert!(msg.contains("Weird"), "{msg}");
    }

    #[test]
    fn changing_the_id_is_refused() {
        let m = model(vec![col("id", K::Text, false)], vec![]);
        let msg = message(run(m, table(vec![tcol("id", "INTEGER", false)], vec![])));
        assert!(
            msg.contains("things.id") && msg.contains("by hand"),
            "{msg}"
        );
    }

    #[test]
    fn two_models_on_one_table_are_refused() {
        let (mid, _) = id();
        let a = model(vec![mid.clone()], vec![]);
        let b = model(vec![mid], vec![]);
        let err = plan(S, &[a, b], &[None, None]).unwrap_err().to_string();
        assert!(
            err.contains("things") && err.contains("two models"),
            "{err}"
        );
    }
}
