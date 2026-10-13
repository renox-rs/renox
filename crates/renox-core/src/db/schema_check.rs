//! Compares a model's columns with the columns of its table: the logic
//! behind `db:check`.

use std::fmt;

use super::schema::ModelInfo;
use super::{ColumnKind, Db, Dialect, ModelColumn, sql};

/// One column of a table, as the database reports it.
#[derive(Debug, Clone)]
pub(crate) struct TableColumn {
    pub name: String,
    pub sql_type: String,
    pub not_null: bool,
    pub default: Option<String>,
    pub primary_key: bool,
    pub identity: bool,
}

/// Reads a table's columns; an empty list means the table is missing.
pub(crate) async fn table_columns(db: &Db, table: &str) -> crate::Result<Vec<TableColumn>> {
    match db.dialect() {
        Dialect::Sqlite => {
            let rows = sql(
                r#"SELECT name, type, "notnull", dflt_value, pk FROM pragma_table_info(?) ORDER BY cid"#,
            )
            .bind(table)
            .fetch_all(db)
            .await?;
            let mut columns = Vec::with_capacity(rows.len());
            for row in &rows {
                columns.push(TableColumn {
                    name: row.try_get::<String>("name")?,
                    sql_type: row.try_get::<String>("type")?,
                    not_null: row.try_get::<i64>("notnull")? != 0,
                    default: row.try_get::<Option<String>>("dflt_value")?,
                    primary_key: row.try_get::<i64>("pk")? != 0,
                    identity: false,
                });
            }
            Ok(columns)
        }
        Dialect::Postgres => {
            let rows = sql(
                "SELECT a.attname AS name, format_type(a.atttypid, a.atttypmod) AS type, \
                 a.attnotnull AS not_null, pg_get_expr(d.adbin, d.adrelid) AS dflt, \
                 (a.attidentity <> '') AS identity \
                 FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 WHERE n.nspname = current_schema() AND c.relname = ? \
                 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
            )
            .bind(table)
            .fetch_all(db)
            .await?;
            let mut columns = Vec::with_capacity(rows.len());
            for row in &rows {
                let name = row.try_get::<String>("name")?;
                columns.push(TableColumn {
                    primary_key: name == "id",
                    name,
                    sql_type: row.try_get::<String>("type")?,
                    not_null: row.try_get::<bool>("not_null")?,
                    default: row.try_get::<Option<String>>("dflt")?,
                    identity: row.try_get::<bool>("identity")?,
                });
            }
            Ok(columns)
        }
    }
}

/// One index of a table (the primary key's own index is left out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableIndex {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    /// The `CREATE INDEX` statement; `None` for indexes the database made for a
    /// `UNIQUE` constraint on SQLite.
    pub sql: Option<String>,
}

/// Reads a table's indexes, without the primary key's.
pub(crate) async fn table_indexes(db: &Db, table: &str) -> crate::Result<Vec<TableIndex>> {
    let mut indexes = Vec::new();
    match db.dialect() {
        Dialect::Sqlite => {
            let rows = sql(r#"SELECT name, "unique", origin FROM pragma_index_list(?)"#)
                .bind(table)
                .fetch_all(db)
                .await?;
            for row in &rows {
                if row.try_get::<String>("origin")? == "pk" {
                    continue;
                }
                let name = row.try_get::<String>("name")?;
                let columns = sql("SELECT name FROM pragma_index_info(?) ORDER BY seqno")
                    .bind(name.as_str())
                    .scalars::<Option<String>>(db)
                    .await?
                    .into_iter()
                    .flatten()
                    .collect();
                let index_sql =
                    sql("SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?")
                        .bind(name.as_str())
                        .scalar_optional::<Option<String>>(db)
                        .await?
                        .flatten();
                indexes.push(TableIndex {
                    unique: row.try_get::<i64>("unique")? != 0,
                    name,
                    columns,
                    sql: index_sql,
                });
            }
        }
        Dialect::Postgres => {
            let rows = sql("SELECT i.relname AS name, ix.indisunique AS uniq, \
                 pg_get_indexdef(ix.indexrelid) AS sql, \
                 array_to_string(ARRAY(SELECT a.attname FROM unnest(ix.indkey) \
                 WITH ORDINALITY k(n, o) JOIN pg_attribute a ON a.attrelid = ix.indrelid \
                 AND a.attnum = k.n ORDER BY k.o), ',') AS cols \
                 FROM pg_index ix JOIN pg_class i ON i.oid = ix.indexrelid \
                 JOIN pg_class t ON t.oid = ix.indrelid \
                 JOIN pg_namespace n ON n.oid = t.relnamespace \
                 WHERE n.nspname = current_schema() AND t.relname = ? \
                 AND NOT ix.indisprimary ORDER BY i.relname")
            .bind(table)
            .fetch_all(db)
            .await?;
            for row in &rows {
                let cols = row.try_get::<String>("cols")?;
                indexes.push(TableIndex {
                    name: row.try_get::<String>("name")?,
                    columns: cols
                        .split(',')
                        .filter(|c| !c.is_empty())
                        .map(str::to_string)
                        .collect(),
                    unique: row.try_get::<bool>("uniq")?,
                    sql: Some(row.try_get::<String>("sql")?),
                });
            }
        }
    }
    Ok(indexes)
}

/// A foreign key in another table that points at the table being checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForeignKey {
    /// The table holding the key.
    pub table: String,
    pub column: String,
    /// The table it points at.
    pub references: String,
    /// `NO ACTION`, `RESTRICT`, `CASCADE`, `SET NULL` or `SET DEFAULT`.
    pub on_delete: String,
}

/// Lists the foreign keys of other tables that point at `table`.
pub(crate) async fn foreign_keys_to(db: &Db, table: &str) -> crate::Result<Vec<ForeignKey>> {
    let mut keys = Vec::new();
    match db.dialect() {
        Dialect::Sqlite => {
            let tables = sql("SELECT name FROM sqlite_master WHERE type = 'table' \
                 AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .scalars::<String>(db)
            .await?;
            for name in tables {
                let rows =
                    sql(r#"SELECT "from", "table", on_delete FROM pragma_foreign_key_list(?)"#)
                        .bind(name.as_str())
                        .fetch_all(db)
                        .await?;
                for row in &rows {
                    if row.try_get::<String>("table")? == table {
                        keys.push(ForeignKey {
                            table: name.clone(),
                            column: row.try_get::<String>("from")?,
                            references: table.to_string(),
                            on_delete: row.try_get::<String>("on_delete")?,
                        });
                    }
                }
            }
        }
        Dialect::Postgres => {
            let rows = sql(
                "SELECT cl.relname AS child, a.attname AS col, co.confdeltype::text AS del \
                 FROM pg_constraint co JOIN pg_class cl ON cl.oid = co.conrelid \
                 JOIN pg_attribute a ON a.attrelid = co.conrelid AND a.attnum = co.conkey[1] \
                 WHERE co.contype = 'f' AND co.confrelid = (quote_ident(?))::regclass \
                 ORDER BY cl.relname, a.attname",
            )
            .bind(table)
            .fetch_all(db)
            .await?;
            for row in &rows {
                let on_delete = match row.try_get::<String>("del")?.as_str() {
                    "r" => "RESTRICT",
                    "c" => "CASCADE",
                    "n" => "SET NULL",
                    "d" => "SET DEFAULT",
                    _ => "NO ACTION",
                };
                keys.push(ForeignKey {
                    table: row.try_get::<String>("child")?,
                    column: row.try_get::<String>("col")?,
                    references: table.to_string(),
                    on_delete: on_delete.to_string(),
                });
            }
        }
    }
    Ok(keys)
}

/// The `CREATE TABLE` statement SQLite keeps for a table; `None` when the table
/// is missing (and always on PostgreSQL, which keeps no such text).
pub(crate) async fn sqlite_table_sql(db: &Db, table: &str) -> crate::Result<Option<String>> {
    if db.dialect() != Dialect::Sqlite {
        return Ok(None);
    }
    Ok(
        sql("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(table)
            .scalar_optional::<Option<String>>(db)
            .await?
            .flatten(),
    )
}

/// SQLite's type affinity for a declared type (SQLite docs, section 3.1).
fn sqlite_affinity(decl: &str) -> &'static str {
    let decl = decl.to_ascii_uppercase();
    if decl.contains("INT") {
        "INTEGER"
    } else if decl.contains("CHAR") || decl.contains("CLOB") || decl.contains("TEXT") {
        "TEXT"
    } else if decl.contains("BLOB") || decl.trim().is_empty() {
        "BLOB"
    } else if decl.contains("REAL") || decl.contains("FLOA") || decl.contains("DOUB") {
        "REAL"
    } else {
        "NUMERIC"
    }
}

/// Lowercases a PostgreSQL type and removes its `(…)` parts.
fn postgres_type(sql_type: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in sql_type.to_lowercase().chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a column of `sql_type` can hold a field of `kind`.
pub(crate) fn kind_fits(dialect: Dialect, kind: ColumnKind, sql_type: &str) -> bool {
    match dialect {
        Dialect::Sqlite => {
            let affinity = sqlite_affinity(sql_type);
            let allowed: &[&str] = match kind {
                ColumnKind::BigInt | ColumnKind::Int | ColumnKind::SmallInt | ColumnKind::Bool => {
                    &["INTEGER", "NUMERIC"]
                }
                ColumnKind::Double | ColumnKind::Real => &["REAL", "NUMERIC"],
                ColumnKind::Text => &["TEXT"],
                ColumnKind::Blob => &["BLOB"],
                ColumnKind::DateTime
                | ColumnKind::NaiveDateTime
                | ColumnKind::Date
                | ColumnKind::Time
                | ColumnKind::Json => &["TEXT", "NUMERIC"],
                ColumnKind::Uuid => &["BLOB", "NUMERIC"],
                _ => return true,
            };
            allowed.contains(&affinity)
        }
        Dialect::Postgres => {
            let ty = postgres_type(sql_type);
            let allowed: &[&str] = match kind {
                ColumnKind::BigInt => &["bigint"],
                ColumnKind::Int => &["integer"],
                ColumnKind::SmallInt => &["smallint"],
                ColumnKind::Double => &["double precision"],
                ColumnKind::Real => &["real"],
                ColumnKind::Text => &["text", "character varying", "character"],
                ColumnKind::Blob => &["bytea"],
                ColumnKind::Bool => &["boolean"],
                ColumnKind::DateTime => &["timestamp with time zone"],
                ColumnKind::NaiveDateTime => &["timestamp without time zone"],
                ColumnKind::Date => &["date"],
                ColumnKind::Time => &["time without time zone"],
                ColumnKind::Json => &["jsonb", "json", "text", "character varying"],
                ColumnKind::Uuid => &["uuid"],
                _ => return true,
            };
            allowed.contains(&ty.as_str())
        }
    }
}

/// One difference between a model and its table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Problem {
    MissingTable,
    NoColumnInfo,
    MissingColumn {
        column: &'static str,
        rust_type: &'static str,
    },
    RequiredExtra {
        column: String,
    },
    WrongType {
        column: &'static str,
        rust_type: &'static str,
        sql_type: String,
    },
    OptionOverNotNull {
        column: &'static str,
    },
    NonOptionOverNullable {
        column: &'static str,
        rust_type: &'static str,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTable => write!(f, "the table is missing"),
            Self::NoColumnInfo => write!(
                f,
                "the model gives no column info (a hand-written `impl Model`), so it can't be checked"
            ),
            Self::MissingColumn { column, rust_type } => {
                write!(f, "missing column `{column}` ({rust_type})")
            }
            Self::RequiredExtra { column } => write!(
                f,
                "`{column}` is NOT NULL without a default and the model doesn't set it: inserts will fail"
            ),
            Self::WrongType {
                column,
                rust_type,
                sql_type,
            } => write!(f, "`{column}` is {rust_type}, but the column is {sql_type}"),
            Self::OptionOverNotNull { column } => {
                write!(f, "`{column}` is an Option, but the column is NOT NULL")
            }
            Self::NonOptionOverNullable { column, rust_type } => {
                write!(f, "`{column}` is {rust_type}, but the column allows NULL")
            }
        }
    }
}

/// Compares a model's columns with its table's.
pub(crate) fn compare(
    dialect: Dialect,
    model: &[ModelColumn],
    table: &[TableColumn],
) -> Vec<Problem> {
    let mut problems = Vec::new();
    for column in model {
        let Some(found) = table.iter().find(|t| t.name == column.name) else {
            problems.push(Problem::MissingColumn {
                column: column.name,
                rust_type: column.rust_type,
            });
            continue;
        };
        if column.kind != ColumnKind::Unknown && !kind_fits(dialect, column.kind, &found.sql_type) {
            problems.push(Problem::WrongType {
                column: column.name,
                rust_type: column.rust_type,
                sql_type: if found.sql_type.is_empty() {
                    "untyped".to_owned()
                } else {
                    found.sql_type.clone()
                },
            });
        }
        if column.name != "id" {
            if column.nullable && found.not_null {
                problems.push(Problem::OptionOverNotNull {
                    column: column.name,
                });
            } else if !column.nullable && !found.not_null {
                problems.push(Problem::NonOptionOverNullable {
                    column: column.name,
                    rust_type: column.rust_type,
                });
            }
        }
    }
    for found in table {
        if found.not_null
            && found.default.is_none()
            && !found.identity
            && !found.primary_key
            && !model.iter().any(|m| m.name == found.name)
        {
            problems.push(Problem::RequiredExtra {
                column: found.name.clone(),
            });
        }
    }
    problems
}

/// What the check found for one model.
#[derive(Debug)]
pub(crate) struct ModelReport {
    pub model: &'static str,
    pub table: &'static str,
    pub problems: Vec<Problem>,
}

/// Checks every model against the database.
pub(crate) async fn check(db: &Db, models: &[ModelInfo]) -> crate::Result<Vec<ModelReport>> {
    let mut reports = Vec::with_capacity(models.len());
    for info in models {
        let columns = (info.columns)();
        let table = table_columns(db, info.table).await?;
        let problems = if table.is_empty() {
            vec![Problem::MissingTable]
        } else if columns.is_empty() {
            vec![Problem::NoColumnInfo]
        } else {
            compare(db.dialect(), &columns, &table)
        };
        reports.push(ModelReport {
            model: info.name,
            table: info.table,
            problems,
        });
    }
    Ok(reports)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The report as text, and the number of problems in it.
pub(crate) fn render(reports: &[ModelReport]) -> (String, usize) {
    let mut out = String::new();
    let mut problems = 0;
    let mut bad = 0;
    for report in reports {
        if report.problems.is_empty() {
            continue;
        }
        bad += 1;
        problems += report.problems.len();
        out.push_str(&format!("{} ({})\n", report.table, report.model));
        for problem in &report.problems {
            out.push_str(&format!("  - {problem}\n"));
        }
    }
    if problems == 0 {
        out.push_str(&format!(
            "{} match the schema.\n",
            plural(reports.len(), "model", "models")
        ));
    } else {
        out.push_str(&format!(
            "{} in {} of {}.\n",
            plural(problems, "problem", "problems"),
            bad,
            plural(reports.len(), "model", "models")
        ));
    }
    (out, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ColumnKind as K;

    const S: Dialect = Dialect::Sqlite;
    const P: Dialect = Dialect::Postgres;

    #[tokio::test]
    async fn indexes_foreign_keys_and_table_sql_are_read() {
        let db = crate::db::connect(&crate::Config::default()).await.unwrap();
        for stmt in [
            "CREATE TABLE sc_parent (id BIGINT PRIMARY KEY, code TEXT NOT NULL UNIQUE, a TEXT, b TEXT)",
            "CREATE INDEX sc_parent_a ON sc_parent (a)",
            "CREATE INDEX sc_parent_ab ON sc_parent (a, b)",
            "CREATE TABLE sc_child (id BIGINT PRIMARY KEY, parent_id BIGINT REFERENCES sc_parent (id) ON DELETE CASCADE)",
        ] {
            sql(stmt).execute(&db).await.unwrap();
        }
        let mut indexes = table_indexes(&db, "sc_parent").await.unwrap();
        indexes.sort_by(|x, y| x.name.cmp(&y.name));
        let names: Vec<_> = indexes.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(indexes.len(), 3, "{names:?}");
        let by = |cols: &[&str]| {
            indexes
                .iter()
                .find(|i| i.columns == cols)
                .unwrap_or_else(|| panic!("no index on {cols:?}: {names:?}"))
        };
        let single = by(&["a"]);
        assert_eq!(single.name, "sc_parent_a");
        assert!(!single.unique && single.sql.as_deref().unwrap().contains("sc_parent_a"));
        let composite = by(&["a", "b"]);
        assert_eq!(composite.name, "sc_parent_ab");
        assert!(!composite.unique);
        let unique = by(&["code"]);
        assert!(unique.unique);
        if db.dialect() == Dialect::Sqlite {
            assert_eq!(unique.sql, None);
        } else {
            assert!(unique.sql.is_some());
        }

        let keys = foreign_keys_to(&db, "sc_parent").await.unwrap();
        assert_eq!(
            keys,
            vec![ForeignKey {
                table: "sc_child".into(),
                column: "parent_id".into(),
                references: "sc_parent".into(),
                on_delete: "CASCADE".into(),
            }]
        );
        assert!(foreign_keys_to(&db, "sc_child").await.unwrap().is_empty());

        let table_sql = sqlite_table_sql(&db, "sc_child").await.unwrap();
        if db.dialect() == Dialect::Sqlite {
            assert!(table_sql.unwrap().contains("ON DELETE CASCADE"));
            assert_eq!(sqlite_table_sql(&db, "nope").await.unwrap(), None);
        } else {
            assert_eq!(table_sql, None);
        }
    }

    #[test]
    fn sqlite_kinds_fit_by_affinity() {
        for kind in [K::BigInt, K::Int, K::SmallInt, K::Bool] {
            assert!(kind_fits(S, kind, "INTEGER"));
            assert!(kind_fits(S, kind, "BIGINT"));
            assert!(kind_fits(S, kind, "NUMERIC"));
            assert!(!kind_fits(S, kind, "TEXT"));
            assert!(!kind_fits(S, kind, "REAL"));
        }
        for kind in [K::Double, K::Real] {
            assert!(kind_fits(S, kind, "REAL"));
            assert!(kind_fits(S, kind, "DOUBLE PRECISION"));
            assert!(kind_fits(S, kind, "DECIMAL(10,2)"));
            assert!(!kind_fits(S, kind, "INTEGER"));
        }
        assert!(kind_fits(S, K::Text, "VARCHAR(255)"));
        assert!(!kind_fits(S, K::Text, "INTEGER"));
        assert!(!kind_fits(S, K::Text, "NUMERIC"));
        assert!(kind_fits(S, K::Blob, "BLOB"));
        assert!(kind_fits(S, K::Blob, ""));
        assert!(!kind_fits(S, K::Blob, "TEXT"));
        for kind in [K::DateTime, K::NaiveDateTime, K::Date, K::Time, K::Json] {
            assert!(kind_fits(S, kind, "TEXT"));
            assert!(kind_fits(S, kind, "DATETIME"));
            assert!(!kind_fits(S, kind, "INTEGER"));
            assert!(!kind_fits(S, kind, "BLOB"));
        }
        assert!(kind_fits(S, K::Uuid, "BLOB"));
        assert!(kind_fits(S, K::Uuid, "NUMERIC"));
        assert!(!kind_fits(S, K::Uuid, "TEXT"));
        assert!(kind_fits(S, K::Unknown, "ANYTHING"));
    }

    #[test]
    fn postgres_kinds_fit_by_type_text() {
        assert!(kind_fits(P, K::BigInt, "bigint"));
        assert!(!kind_fits(P, K::BigInt, "integer"));
        assert!(kind_fits(P, K::Int, "integer"));
        assert!(!kind_fits(P, K::Int, "bigint"));
        assert!(kind_fits(P, K::SmallInt, "smallint"));
        assert!(!kind_fits(P, K::SmallInt, "integer"));
        assert!(kind_fits(P, K::Double, "double precision"));
        assert!(!kind_fits(P, K::Double, "real"));
        assert!(kind_fits(P, K::Real, "real"));
        assert!(kind_fits(P, K::Text, "text"));
        assert!(kind_fits(P, K::Text, "character varying(255)"));
        assert!(kind_fits(P, K::Text, "character(2)"));
        assert!(!kind_fits(P, K::Text, "bigint"));
        assert!(kind_fits(P, K::Blob, "bytea"));
        assert!(kind_fits(P, K::Bool, "boolean"));
        assert!(!kind_fits(P, K::Bool, "smallint"));
        assert!(kind_fits(P, K::DateTime, "timestamp(6) with time zone"));
        assert!(!kind_fits(P, K::DateTime, "timestamp without time zone"));
        assert!(kind_fits(
            P,
            K::NaiveDateTime,
            "timestamp without time zone"
        ));
        assert!(kind_fits(P, K::Date, "date"));
        assert!(kind_fits(P, K::Time, "time without time zone"));
        assert!(!kind_fits(P, K::Time, "time with time zone"));
        for ty in ["jsonb", "json", "text", "character varying"] {
            assert!(kind_fits(P, K::Json, ty));
        }
        assert!(!kind_fits(P, K::Json, "bytea"));
        assert!(kind_fits(P, K::Uuid, "uuid"));
        assert!(!kind_fits(P, K::Uuid, "text"));
        assert!(kind_fits(P, K::Unknown, "anything"));
    }

    fn col(name: &'static str, kind: K, nullable: bool) -> ModelColumn {
        ModelColumn::new(name, "T", kind, nullable)
    }

    fn tcol(name: &str, ty: &str, not_null: bool) -> TableColumn {
        TableColumn {
            name: name.to_owned(),
            sql_type: ty.to_owned(),
            not_null,
            default: None,
            primary_key: false,
            identity: false,
        }
    }

    #[test]
    fn a_matching_table_has_no_problems() {
        let model = [col("id", K::BigInt, false), col("note", K::Text, true)];
        let mut id = tcol("id", "INTEGER", false);
        id.primary_key = true;
        let table = [id, tcol("note", "TEXT", false)];
        assert!(compare(S, &model, &table).is_empty());
    }

    #[test]
    fn a_missing_column_is_reported() {
        let model = [col("price", K::BigInt, false)];
        let problems = compare(S, &model, &[tcol("other", "TEXT", false)]);
        assert!(problems.contains(&Problem::MissingColumn {
            column: "price",
            rust_type: "T"
        }));
        assert_eq!(problems[0].to_string(), "missing column `price` (T)");
    }

    #[test]
    fn a_required_extra_column_is_reported_unless_it_has_a_default() {
        let mut stock = tcol("stock", "INTEGER", true);
        let problems = compare(S, &[], std::slice::from_ref(&stock));
        assert_eq!(
            problems,
            [Problem::RequiredExtra {
                column: "stock".into()
            }]
        );
        assert!(problems[0].to_string().contains("inserts will fail"));
        stock.default = Some("0".into());
        assert!(compare(S, &[], &[stock.clone()]).is_empty());
        stock.default = None;
        stock.identity = true;
        assert!(compare(P, &[], &[stock.clone()]).is_empty());
        stock.identity = false;
        stock.primary_key = true;
        assert!(compare(S, &[], &[stock]).is_empty());
    }

    #[test]
    fn a_wrong_type_is_reported() {
        let problems = compare(
            S,
            &[col("name", K::Text, false)],
            &[tcol("name", "INTEGER", true)],
        );
        assert_eq!(
            problems,
            [Problem::WrongType {
                column: "name",
                rust_type: "T",
                sql_type: "INTEGER".into()
            }]
        );
        assert_eq!(
            problems[0].to_string(),
            "`name` is T, but the column is INTEGER"
        );
        // Unknown kinds skip the type check.
        assert!(
            compare(
                S,
                &[col("name", K::Unknown, false)],
                &[tcol("name", "INTEGER", true)]
            )
            .is_empty()
        );
    }

    #[test]
    fn an_option_over_not_null_is_reported() {
        let problems = compare(
            S,
            &[col("notes", K::Text, true)],
            &[tcol("notes", "TEXT", true)],
        );
        assert_eq!(problems, [Problem::OptionOverNotNull { column: "notes" }]);
        assert!(problems[0].to_string().contains("NOT NULL"));
    }

    #[test]
    fn a_non_option_over_a_nullable_column_is_reported_but_not_for_id() {
        let problems = compare(
            S,
            &[col("notes", K::Text, false), col("id", K::BigInt, false)],
            &[tcol("notes", "TEXT", false), tcol("id", "INTEGER", false)],
        );
        assert_eq!(
            problems,
            [Problem::NonOptionOverNullable {
                column: "notes",
                rust_type: "T"
            }]
        );
    }

    #[test]
    fn table_level_problems_have_messages() {
        assert!(Problem::MissingTable.to_string().contains("missing"));
        assert!(Problem::NoColumnInfo.to_string().contains("column info"));
    }

    #[test]
    fn render_groups_problems_per_model() {
        let reports = [
            ModelReport {
                model: "my_app::Product",
                table: "products",
                problems: vec![Problem::MissingColumn {
                    column: "price",
                    rust_type: "i64",
                }],
            },
            ModelReport {
                model: "my_app::Brand",
                table: "brands",
                problems: vec![],
            },
            ModelReport {
                model: "my_app::Tag",
                table: "tags",
                problems: vec![],
            },
        ];
        let (text, n) = render(&reports);
        assert_eq!(n, 1);
        assert_eq!(
            text,
            "products (my_app::Product)\n  - missing column `price` (i64)\n1 problem in 1 of 3 models.\n"
        );
        let (text, n) = render(&reports[1..]);
        assert_eq!((text.as_str(), n), ("2 models match the schema.\n", 0));
    }
}
