//! `rnx make:module <name> --resource`: a whole resource from a field list,
//! with the model, its migration and factory, validated forms, the seven
//! handlers, views built on the UI kit, and tests.

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use heck::{ToKebabCase, ToSnakeCase, ToTitleCase, ToUpperCamelCase};

use crate::Database;
use crate::generate::{add_mod, check_name, register_in_main, write_new};

/// A field's type, as written in `--fields "name:string price:money"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    String,
    Text,
    Int,
    Money,
    Float,
    Bool,
    Date,
}

#[derive(Debug, Clone)]
struct Field {
    name: String,
    kind: Kind,
}

const KINDS: &str = "string, text, int, money, float, bool, date";

fn parse_fields(spec: &str) -> Result<Vec<Field>> {
    let mut fields = Vec::new();
    for part in spec.split([' ', ',']).filter(|p| !p.is_empty()) {
        let (name, kind) = part.split_once(':').unwrap_or((part, "string"));
        let kind = match kind {
            "string" | "str" => Kind::String,
            "text" => Kind::Text,
            "int" | "integer" | "i64" => Kind::Int,
            "money" => Kind::Money,
            "float" | "f64" | "decimal" => Kind::Float,
            "bool" | "boolean" => Kind::Bool,
            "date" => Kind::Date,
            other => bail!("unknown field type `{other}` for `{name}`; use one of {KINDS}"),
        };
        let name = name.to_snake_case();
        check_name(&name)?;
        if ["id", "created_at", "updated_at"].contains(&name.as_str()) {
            bail!("`{name}` is added for you; leave it out of --fields");
        }
        if fields.iter().any(|f: &Field| f.name == name) {
            bail!("`{name}` is listed twice");
        }
        fields.push(Field { name, kind });
    }
    if fields.is_empty() {
        fields.push(Field {
            name: "name".into(),
            kind: Kind::String,
        });
    }
    Ok(fields)
}

impl Field {
    /// Sentence case, as labels are in the HIG: `due_on` → "Due on".
    fn label(&self) -> String {
        let words = self.name.to_title_case().to_lowercase();
        let mut chars = words.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    }

    fn rust_type(&self) -> &'static str {
        match self.kind {
            Kind::String | Kind::Text => "String",
            Kind::Int | Kind::Money => "i64",
            Kind::Float => "f64",
            Kind::Bool => "bool",
            Kind::Date => "NaiveDate",
        }
    }

    fn column(&self, database: Database) -> String {
        let sql = match (self.kind, database) {
            (Kind::String | Kind::Text, _) => "TEXT NOT NULL DEFAULT ''",
            (Kind::Int | Kind::Money, Database::Sqlite) => "INTEGER NOT NULL DEFAULT 0",
            (Kind::Int | Kind::Money, Database::Postgres) => "BIGINT NOT NULL DEFAULT 0",
            (Kind::Float, Database::Sqlite) => "REAL NOT NULL DEFAULT 0",
            (Kind::Float, Database::Postgres) => "DOUBLE PRECISION NOT NULL DEFAULT 0",
            (Kind::Bool, Database::Sqlite) => "INTEGER NOT NULL DEFAULT 0",
            (Kind::Bool, Database::Postgres) => "BOOLEAN NOT NULL DEFAULT FALSE",
            (Kind::Date, Database::Sqlite) => "TEXT NOT NULL DEFAULT '1970-01-01'",
            (Kind::Date, Database::Postgres) => "DATE NOT NULL DEFAULT '1970-01-01'",
        };
        format!("\"{}\" {sql}", self.name)
    }

    /// The field's `#[validate(…)]` rules, for `#[derive(Validate)]`.
    fn rules(&self) -> Option<&'static str> {
        match self.kind {
            Kind::String => Some("required, max = 255"),
            Kind::Text => Some("required, max = 10_000"),
            Kind::Money => Some("min = 0"),
            _ => None,
        }
    }

    fn fake(&self) -> String {
        match self.kind {
            Kind::String => "Words(1..3).fake::<Vec<String>>().join(\" \")".into(),
            Kind::Text => "Sentence(4..10).fake()".into(),
            Kind::Int => "(1..100).fake()".into(),
            Kind::Money => "(1_000..100_000).fake()".into(),
            Kind::Float => "(1.0..100.0).fake()".into(),
            Kind::Bool => "(0..2).fake::<u8>() == 1".into(),
            Kind::Date => {
                "NaiveDate::from_ymd_opt(2026, (1..13).fake(), (1..29).fake()).unwrap_or_default()"
                    .into()
            }
        }
    }

    /// The form field in the create and edit views (UI kit tags).
    fn input(&self) -> String {
        let (name, label) = (&self.name, self.label());
        let value = format!(":value=\"record.{name} if record else none\"");
        match self.kind {
            Kind::String => {
                format!("<rx-input name=\"{name}\" label=\"{label}\" {value} required/>")
            }
            Kind::Text => {
                format!("<rx-textarea name=\"{name}\" label=\"{label}\" {value} required/>")
            }
            Kind::Int => format!(
                "<rx-input name=\"{name}\" label=\"{label}\" type=\"number\" {value} required/>"
            ),
            Kind::Money => format!(
                "<rx-input name=\"{name}\" label=\"{label}\" {value} required hint=\"In cents (the smallest unit), without dots.\" inputmode=\"numeric\"/>"
            ),
            Kind::Float => format!(
                "<rx-input name=\"{name}\" label=\"{label}\" type=\"number\" {value} required step=\"any\"/>"
            ),
            Kind::Bool => format!(
                "<rx-checkbox name=\"{name}\" label=\"{label}\" :checked=\"record.{name} if record else false\" switch/>"
            ),
            Kind::Date => format!(
                "<rx-input name=\"{name}\" label=\"{label}\" type=\"date\" {value} required/>"
            ),
        }
    }

    /// How the value shows in the list and on the record's page.
    fn display(&self, var: &str) -> String {
        let value = format!("{var}.{}", self.name);
        match self.kind {
            Kind::Money => format!("{{{{ {value} | number }}}}"),
            Kind::Bool => format!(
                "<rx-badge rx-if=\"{value}\" kind=\"success\">Yes</rx-badge><rx-badge rx-else>No</rx-badge>"
            ),
            _ => format!("{{{{ {value} }}}}"),
        }
    }

    fn numeric(&self) -> bool {
        matches!(self.kind, Kind::Int | Kind::Money | Kind::Float)
    }

    /// A value the generated tests post.
    fn sample(&self, second: bool) -> &'static str {
        match (self.kind, second) {
            (Kind::String, false) => "First value",
            (Kind::String, true) => "Changed value",
            (Kind::Text, false) => "Some longer text.",
            (Kind::Text, true) => "Other longer text.",
            (Kind::Int, false) => "5",
            (Kind::Int, true) => "7",
            (Kind::Money, false) => "12000",
            (Kind::Money, true) => "15000",
            (Kind::Float, false) => "1.5",
            (Kind::Float, true) => "2.5",
            (Kind::Bool, _) => "on",
            (Kind::Date, false) => "2026-01-15",
            (Kind::Date, true) => "2026-02-20",
        }
    }
}

/// `products` → `Product`; `categories` → `Category`; `news` stays.
fn singular(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
        format!("{stem}y")
    } else if word.ends_with("sses") || word.ends_with("xes") || word.ends_with("ches") {
        word[..word.len() - 2].to_owned()
    } else if word.ends_with('s') && !word.ends_with("ss") && !word.ends_with("news") {
        word[..word.len() - 1].to_owned()
    } else {
        word.to_owned()
    }
}

/// The plural of a snake_case name's last word, for table names
/// (`waitlist_signup` → `waitlist_signups`, `category` → `categories`); the
/// inverse of `singular`. Words already plural, or with no plural, stay.
pub(crate) fn plural(name: &str) -> String {
    let (head, word) = match name.rsplit_once('_') {
        Some((head, word)) => (format!("{head}_"), word),
        None => (String::new(), name),
    };
    let unchanged = [
        "news",
        "staff",
        "data",
        "media",
        "series",
        "info",
        "equipment",
    ];
    let word = if unchanged.contains(&word) || (word.ends_with('s') && singular(word) != word) {
        word.to_owned()
    } else if let Some(stem) = word.strip_suffix('y')
        && !stem.ends_with(['a', 'e', 'i', 'o', 'u'])
    {
        format!("{stem}ies")
    } else if word.ends_with(['s', 'x', 'z']) || word.ends_with("ch") || word.ends_with("sh") {
        format!("{word}es")
    } else {
        format!("{word}s")
    };
    format!("{head}{word}")
}

/// The app's crate name, from its Cargo.toml (`-` becomes `_`).
pub(crate) fn crate_name(root: &Path) -> Result<String> {
    let manifest = fs::read_to_string(root.join("Cargo.toml"))?;
    let name = manifest
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "name").then(|| value.trim().trim_matches('"').to_owned())
        })
        .ok_or_else(|| anyhow::anyhow!("no package name in Cargo.toml"))?;
    Ok(name.replace('-', "_"))
}

/// `rnx make:module products --resource [--model Product] [--fields …]`.
pub fn resource(
    root: &Path,
    name: &str,
    model: Option<&str>,
    fields: Option<&str>,
) -> Result<String> {
    check_name(name)?;
    let module = name.to_snake_case();
    let module_type = name.to_upper_camel_case();
    let model = model.map_or_else(
        || singular(&module).to_upper_camel_case(),
        |m| m.to_upper_camel_case(),
    );
    if model == module_type {
        bail!(
            "the module `{module}` and its model would both be `{model}`: name the module in the plural, or pass --model"
        );
    }
    let path = module.to_kebab_case();
    // The model's table, plural, as `make:model` names it (#127): the module
    // only names the routes and pages (`news` with `--model Article` →
    // `articles`; `products` → `Product` → `products`).
    let table = plural(&model.to_snake_case());
    let title = module.to_title_case();
    let fields = parse_fields(fields.unwrap_or(""))?;
    let crate_name = crate_name(root)?;
    let dir = root.join("src/app").join(&module);
    if dir.exists() {
        bail!("{} already exists", dir.display());
    }

    let replace = |template: &str| -> String {
        template
            .replace("__Model__", &model)
            .replace("__Module__", &module_type)
            .replace("__module__", &module)
            .replace("__path__", &path)
            .replace("__table__", &table)
            .replace("__title__", &title)
            .replace("__crate__", &crate_name)
    };

    // The model and its factory.
    let uses_date = fields.iter().any(|f| f.kind == Kind::Date);
    let model_fields: String = fields
        .iter()
        .map(|f| {
            let rules = f
                .rules()
                .map(|rules| format!("    #[form(validate({rules}))]\n"))
                .unwrap_or_default();
            format!("{rules}    pub {}: {},\n", f.name, f.rust_type())
        })
        .collect();
    let fakes: String = fields
        .iter()
        .map(|f| format!("            {}: {},\n", f.name, f.fake()))
        .collect();
    let model_rs = replace(MODEL)
        .replace("__fields__", &model_fields)
        .replace("__fakes__", &fakes)
        .replace(
            "__date_use__",
            if uses_date {
                "use renox::chrono::NaiveDate;\n"
            } else {
                ""
            },
        );
    let fakers: Vec<&str> = [(Kind::Text, "Sentence"), (Kind::String, "Words")]
        .into_iter()
        .filter(|(kind, _)| fields.iter().any(|f| f.kind == *kind))
        .map(|(_, faker)| faker)
        .collect();
    let faker_use = if fakers.is_empty() {
        String::new()
    } else {
        format!(
            "use renox::fake::faker::lorem::en::{{{}}};\n",
            fakers.join(", ")
        )
    };
    let model_rs = model_rs.replace("__faker_use__", &faker_use);
    write_new(&dir.join("model.rs"), &model_rs)?;

    // The module: routes, form, handlers.
    let mod_rs = replace(MODULE);
    write_new(&dir.join("mod.rs"), &mod_rs)?;

    // Views.
    let listed: Vec<&Field> = fields.iter().take(3).collect();
    let cells: String = listed
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let align = if f.numeric() { " align=\"num\"" } else { "" };
            let value = f.display("record");
            let label = f.label();
            if i == 0 {
                format!("      <rx-column label=\"{label}\"{align}><a class=\"rx-link\" href=\"{{{{ route('__module__.show', record.id) }}}}\"><strong>{value}</strong></a></rx-column>\n")
            } else {
                format!("      <rx-column label=\"{label}\"{align}>{value}</rx-column>\n")
            }
        })
        .collect();
    let first = &fields[0];
    let heading = match first.kind {
        Kind::String => format!("{{{{ record.{} }}}}", first.name),
        _ => format!("{} #{{{{ record.id }}}}", model.to_title_case()),
    };
    let inputs: String = fields
        .iter()
        .map(|f| format!("        {}\n", f.input()))
        .collect();
    let details: String = fields
        .iter()
        .map(|f| {
            format!(
                "        <div class=\"rx-row\"><span class=\"rx-subtitle\">{}</span><span class=\"rx-spacer\"></span><span>{}</span></div>\n",
                f.label(),
                f.display("record")
            )
        })
        .collect();
    let views = root.join("resources/views").join(&module);
    write_new(
        &views.join("index.html"),
        &replace(INDEX_VIEW).replace("__cells__", &replace(&cells)),
    )?;
    write_new(
        &views.join("form.html"),
        &replace(FORM_VIEW).replace("__inputs__", &inputs),
    )?;
    write_new(
        &views.join("show.html"),
        &replace(SHOW_VIEW)
            .replace("__heading__", &heading)
            .replace("__details__", &details),
    )?;

    // Tests, through HTTP like a browser.
    let form = |second: bool| -> String {
        fields
            .iter()
            .map(|f| format!("(\"{}\", \"{}\")", f.name, f.sample(second)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let check = fields
        .iter()
        .find(|f| f.kind == Kind::String)
        .map(|f| (f.name.clone(), f.sample(false), f.sample(true)));
    let blanked = |blank: &str| -> String {
        fields
            .iter()
            .map(|f| {
                let value = if f.name == blank { "" } else { f.sample(false) };
                format!("(\"{}\", \"{value}\")", f.name)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (seen_first, seen_second, has_row, has_changed, invalid) = match &check {
        Some((name, first, second)) => (
            format!("\n        .assert_see(\"{first}\")"),
            format!("\n    app.get(\"/__path__\").await.assert_see(\"{second}\");"),
            format!(
                "\n    app.assert_database_has(\"__table__\", &[(\"{name}\", &\"{first}\")]).await;"
            ),
            format!(
                "\n    app.assert_database_has(\"__table__\", &[(\"{name}\", &\"{second}\")]).await;"
            ),
            format!(
                "\n\n#[renox::test]\nasync fn invalid___module___are_refused() {{\n    let app = TestApp::new(__crate__::app()).await;\n    app.acting_as(&user(&app).await);\n    app.htmx()\n        .post(\"/__path__\", &[{blank}])\n        .await\n        .assert_invalid(\"{name}\");\n    app.assert_database_count(\"__table__\", 0).await;\n}}\n",
                blank = blanked(name)
            ),
        ),
        None => Default::default(),
    };
    let tests = replace(TESTS)
        .replace("__form1__", &form(false))
        .replace("__form2__", &form(true))
        .replace("__seen_first__", &seen_first)
        .replace("__seen_second__", &replace(&seen_second))
        .replace("__has_row__", &replace(&has_row))
        .replace("__has_changed__", &replace(&has_changed))
        .replace("__invalid__", &replace(&invalid));
    write_new(&root.join("tests").join(format!("{module}.rs")), &tests)?;

    // The table.
    let database = Database::of_current_app();
    let columns: String = fields
        .iter()
        .map(|f| format!("    {},\n", f.column(database)))
        .collect();
    let (id, stamp) = match database {
        Database::Sqlite => ("id INTEGER PRIMARY KEY AUTOINCREMENT", "TEXT"),
        Database::Postgres => (
            "id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY",
            "TIMESTAMPTZ",
        ),
    };
    let up = format!(
        "CREATE TABLE \"{table}\" (\n    {id},\n{columns}    created_at {stamp},\n    updated_at {stamp}\n);\n"
    );
    crate::make::migration_with(
        &format!("create_{table}_table"),
        &root.join("migrations"),
        &up,
        &format!("DROP TABLE \"{table}\";\n"),
    )?;

    add_mod(&root.join("src/app/mod.rs"), &module)?;
    register_in_main(
        &root.join(if root.join("src/lib.rs").is_file() {
            "src/lib.rs"
        } else {
            "src/main.rs"
        }),
        &format!("app::{module}::{module_type}"),
    )?;
    Ok(path)
}

const MODEL: &str = r#"use renox::fake::Fake;
__faker_use__use renox::prelude::*;
__date_use__use serde::{Deserialize, Serialize};

// `#[model(form)]` makes `__Model__Form` (the create/edit form) from the fields;
// rules go in `#[form(validate(…))]`.
#[derive(Model, Serialize, Deserialize, Default, Debug, Clone)]
#[model(table = "__table__", form)]
pub struct __Model__ {
    pub id: i64,
__fields__    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// Fake records for seeders and tests: `__Model__::factory().count(20).create(&db)`.
impl Factory for __Model__ {
    fn definition() -> Self {
        __Model__ {
__fakes__            ..Default::default()
        }
    }
}
"#;

const MODULE: &str = r#"//! Made with `rnx make:module __module__ --resource`: list, show, create,
//! edit and delete __title__, for logged-in users.

pub mod model;

use renox::prelude::*;
use model::{__Model__, __Model__Form};

pub struct __Module__;

impl Module for __Module__ {
    fn name(&self) -> &'static str {
        "__module__"
    }

    fn register(&self, app: &mut Registry) {
        app.model::<__Model__>();
    }

    fn routes(&self) -> Routes {
        Routes::new()
            // GET /__path__, GET /__path__/new, POST /__path__, GET /__path__/{id},
            // GET /__path__/{id}/edit, PUT /__path__/{id}, DELETE /__path__/{id};
            // named __module__.index, __module__.create, …
            .resource(
                "/__path__",
                "__module__",
                Resource::new()
                    .index(index)
                    .create(create)
                    .store(store)
                    .show(show)
                    .edit(edit)
                    .update(update)
                    .destroy(destroy),
            )
            .require_auth()
    }
}

async fn index(State(db): State<Db>, Page(page): Page) -> Result<View> {
    let records = __Model__::query().latest().paginate(&db, page, 20).await?;
    Ok(view("__module__/index.html", context! { records }))
}

async fn create() -> View {
    view("__module__/form.html", context! {})
}

async fn store(State(db): State<Db>, Valid(form): Valid<__Model__Form>) -> Result<(Toast, Redirect)> {
    let mut record = __Model__::default();
    form.fill(&mut record);
    __Model__::create(&db, record).await?;
    Ok((Toast::success("Saved."), Redirect::to("/__path__")))
}

async fn show(State(db): State<Db>, Path(id): Path<i64>) -> Result<View> {
    let record = __Model__::find_or_404(&db, id).await?;
    Ok(view("__module__/show.html", context! { record }))
}

async fn edit(State(db): State<Db>, Path(id): Path<i64>) -> Result<View> {
    let record = __Model__::find_or_404(&db, id).await?;
    Ok(view("__module__/form.html", context! { record }))
}

async fn update(
    State(db): State<Db>,
    Path(id): Path<i64>,
    Valid(form): Valid<__Model__Form>,
) -> Result<(Toast, Redirect)> {
    let mut record = __Model__::find_or_404(&db, id).await?;
    form.fill(&mut record);
    record.save(&db).await?;
    Ok((Toast::success("Changes saved."), Redirect::to(&format!("/__path__/{id}"))))
}

async fn destroy(State(db): State<Db>, Path(id): Path<i64>) -> Result<(Toast, Redirect)> {
    let mut record = __Model__::find_or_404(&db, id).await?;
    record.delete(&db).await?;
    Ok((Toast::success("Deleted."), Redirect::to("/__path__")))
}
"#;

const INDEX_VIEW: &str = r#"<rx-page layout="layouts/app.html" title="__title__">
  <rx-stack>
    <rx-page-header title="__title__">
      <rx-link-button route="__module__.create" label="New" variant="primary"/>
    </rx-page-header>
    <rx-table :rows="records" as="record" :key="record.id" caption="__title__">
__cells__      <rx-row-actions>
        <rx-link-button route="__module__.edit" label="Edit" variant="plain" size="small"/>
        <rx-confirm id="delete-{{ record.id }}" route="__module__.destroy" label="Delete" title="Delete this?" size="small">This can't be undone.</rx-confirm>
      </rx-row-actions>
      <rx-slot name="empty"><rx-empty title="Nothing here yet" message="What you add shows up in this list." route="__module__.create" action-label="Add the first"/></rx-slot>
    </rx-table>
  </rx-stack>
</rx-page>
"#;

// One form for both: `record` is only set when editing.
const FORM_VIEW: &str = r#"<rx-page layout="layouts/app.html" :title="'Edit' if record else 'New'">
  <rx-stack>
    <rx-page-header :title="'Edit' if record else 'New'" back="{{ route('__module__.index') }}" back-label="__title__"/>
    <rx-form :action="route('__module__.update', record.id) if record else route('__module__.store')" live>
      <input type="hidden" name="_method" value="PUT" rx-if="record">
      <rx-card>
        <rx-form-errors/>
__inputs__        <div class="rx-card__footer">
          <rx-link-button route="__module__.index" label="Cancel" variant="plain"/>
          <rx-button :label="'Save changes' if record else 'Create'"/>
        </div>
      </rx-card>
    </rx-form>
  </rx-stack>
</rx-page>
"#;

const SHOW_VIEW: &str = r#"<rx-page layout="layouts/app.html" title="__title__">
  <rx-stack>
    <rx-page-header title="__heading__" back="{{ route('__module__.index') }}" back-label="__title__">
      <rx-link-button href="{{ route('__module__.edit', record.id) }}" label="Edit" variant="secondary" size="small"/>
      <rx-confirm id="delete-{{ record.id }}" action="{{ route('__module__.destroy', record.id) }}" label="Delete" title="Delete this?" size="small">This can't be undone.</rx-confirm>
    </rx-page-header>
    <rx-card>
__details__    </rx-card>
  </rx-stack>
</rx-page>
"#;

const TESTS: &str = r#"//! Made with `rnx make:module __module__ --resource`.

use renox::prelude::*;
use renox::testing::TestApp;

async fn user(app: &TestApp) -> User {
    User::register(app.db(), "Test", "test@example.com", "password123")
        .await
        .unwrap()
}

#[renox::test]
async fn guests_are_sent_to_log_in() {
    let app = TestApp::new(__crate__::app()).await;
    app.get("/__path__").await.assert_redirect("/login");
}

#[renox::test]
async fn __module___are_created_listed_changed_and_deleted() {
    let app = TestApp::new(__crate__::app()).await;
    app.acting_as(&user(&app).await);
    app.get("/__path__").await.assert_ok().assert_view("__module__/index.html");
    app.get("/__path__/new").await.assert_ok();

    app.post("/__path__", &[__form1__])
        .await
        .assert_redirect("/__path__");__has_row__
    app.get("/__path__")
        .await
        .assert_ok()__seen_first__;
    app.get("/__path__/1").await.assert_ok().assert_view("__module__/show.html");
    app.get("/__path__/1/edit").await.assert_ok();

    app.put("/__path__/1", &[__form2__])
        .await
        .assert_redirect("/__path__/1");__has_changed____seen_second__
    app.delete("/__path__/1").await.assert_redirect("/__path__");
    app.assert_database_count("__table__", 0).await;
    app.get("/__path__/1").await.assert_not_found();
}__invalid__

#[renox::test]
async fn __module___match_their_table() {
    let app = TestApp::new(__crate__::app()).await;
    app.assert_models_match_schema().await;
}

#[renox::test]
async fn __module___views_compile() {
    let app = TestApp::new(__crate__::app()).await;
    app.assert_views_compile();
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/app/home")).unwrap();
        fs::create_dir_all(dir.path().join("migrations")).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-shop\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("src/app/mod.rs"), "pub mod home;\n").unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            "pub mod app;\n\npub fn app() -> renox::App {\n    renox::App::new()\n        .migrations(renox::migrations!())\n        .module(app::home::Home)\n}\n",
        )
        .unwrap();
        dir
    }

    fn read(dir: &tempfile::TempDir, path: &str) -> String {
        fs::read_to_string(dir.path().join(path)).unwrap()
    }

    #[test]
    fn fields_parse_with_types_and_aliases() {
        let fields = parse_fields("title, Price:money stock:int done:boolean due_on:date").unwrap();
        let kinds: Vec<_> = fields.iter().map(|f| (f.name.as_str(), f.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("title", Kind::String),
                ("price", Kind::Money),
                ("stock", Kind::Int),
                ("done", Kind::Bool),
                ("due_on", Kind::Date),
            ]
        );
        assert_eq!(fields[4].label(), "Due on");
        assert_eq!(fields[1].rust_type(), "i64");
        assert!(fields[1].numeric() && !fields[0].numeric());
        assert_eq!(fields[0].rules(), Some("required, max = 255"));
        assert_eq!(fields[3].rules(), None);
        assert!(
            fields[3]
                .column(Database::Postgres)
                .contains("BOOLEAN NOT NULL DEFAULT FALSE")
        );
        assert!(
            fields[3]
                .column(Database::Sqlite)
                .contains("INTEGER NOT NULL DEFAULT 0")
        );
        assert!(fields[1].display("row").contains("| number"));

        // Nothing given: one `name` text field.
        let fields = parse_fields("").unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(
            (fields[0].name.as_str(), fields[0].kind),
            ("name", Kind::String)
        );
    }

    #[test]
    fn bad_fields_are_refused() {
        let error = |spec: &str| parse_fields(spec).unwrap_err().to_string();
        assert!(error("price:currency").contains("unknown field type `currency`"));
        assert!(error("id:int").contains("added for you"));
        assert!(error("created_at:date").contains("added for you"));
        assert!(error("name name:text").contains("listed twice"));
    }

    #[test]
    fn names_become_plural() {
        for (single, plural_) in [
            ("product", "products"),
            ("category", "categories"),
            ("box", "boxes"),
            ("class", "classes"),
            ("batch", "batches"),
            ("wish", "wishes"),
            ("day", "days"),
            ("waitlist_signup", "waitlist_signups"),
            ("stock_movement", "stock_movements"),
            ("order", "orders"),
            ("user", "users"),
            ("news", "news"),
            ("staff", "staff"),
            ("products", "products"),
        ] {
            assert_eq!(plural(single), plural_, "{single}");
        }
        // Round trip with `singular`, which `--resource` uses on module names.
        for word in ["product", "category", "box", "class", "batch", "address"] {
            assert_eq!(singular(&plural(word)), word);
        }
    }

    #[test]
    fn plurals_become_singular() {
        for (plural, single) in [
            ("products", "product"),
            ("categories", "category"),
            ("boxes", "box"),
            ("classes", "class"),
            ("batches", "batch"),
            ("news", "news"),
            ("address", "address"),
            ("staff", "staff"),
        ] {
            assert_eq!(singular(plural), single);
        }
    }

    #[test]
    fn the_crate_name_comes_from_cargo_toml() {
        let dir = app();
        assert_eq!(crate_name(dir.path()).unwrap(), "my_shop");
        fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        assert!(crate_name(dir.path()).is_err());
    }

    #[test]
    fn a_resource_writes_every_file_and_registers_its_module() {
        let dir = app();
        resource(
            dir.path(),
            "products",
            None,
            Some("name price:money in_stock:bool"),
        )
        .unwrap();

        let model = read(&dir, "src/app/products/model.rs");
        assert!(model.contains("pub struct Product"));
        assert!(model.contains("pub price: i64,"));
        assert!(model.contains("pub in_stock: bool,"));
        let module = read(&dir, "src/app/products/mod.rs");
        assert!(module.contains("pub struct Products;"));
        assert!(!module.contains("__"), "every placeholder is filled");
        assert!(model.contains("form)]"), "{model}");
        assert!(
            model.contains("#[form(validate(required, max = 255))]"),
            "{model}"
        );
        assert!(!module.contains("struct ProductForm"), "{module}");
        for view in ["index", "form", "show"] {
            let html = read(&dir, &format!("resources/views/products/{view}.html"));
            assert!(!html.contains("__Model__") && !html.contains("__path__"));
        }
        let tests = read(&dir, "tests/products.rs");
        assert!(tests.contains("my_shop::app()"));
        assert!(!tests.contains("__"));
        assert!(
            module.contains("fn register(&self, app: &mut Registry)")
                && module.contains("app.model::<Product>();"),
            "{module}"
        );
        assert!(tests.contains("assert_models_match_schema"));
        assert!(tests.contains("async fn products_match_their_table()"));
        assert!(tests.contains("app.assert_views_compile();"));
        // Every resource carries both checks.
        resource(dir.path(), "orders", None, Some("note")).unwrap();
        let orders = read(&dir, "tests/orders.rs");
        assert!(
            orders.contains("orders_match_their_table") && orders.contains("orders_views_compile")
        );
        assert!(!orders.contains("__"));

        let migrations: Vec<_> = fs::read_dir(dir.path().join("migrations"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        let up = migrations
            .iter()
            .find(|name| name.ends_with("create_products_table.up.sql"))
            .expect("an up migration");
        let sql = read(&dir, &format!("migrations/{up}"));
        assert!(sql.contains("CREATE TABLE \"products\""));
        assert!(sql.contains("\"in_stock\""));

        assert!(read(&dir, "src/app/mod.rs").contains("pub mod products;"));
        assert!(read(&dir, "src/lib.rs").contains(".module(app::products::Products)"));

        // A second time is refused, without touching the first.
        let again = resource(dir.path(), "products", None, None).unwrap_err();
        assert!(again.to_string().contains("already exists"));
    }

    #[test]
    fn a_resource_needs_a_model_name_apart_from_its_module() {
        let dir = app();
        let error = resource(dir.path(), "news", None, None).unwrap_err();
        assert!(error.to_string().contains("--model"), "{error}");
        resource(dir.path(), "news", Some("article"), Some("title body:text")).unwrap();
        let model = read(&dir, "src/app/news/model.rs");
        assert!(model.contains("pub struct Article"));
        // The model's table, not the module's (#127).
        assert!(
            model.contains(r#"#[model(table = "articles", form)]"#),
            "{model}"
        );
        let migrations: Vec<String> = fs::read_dir(dir.path().join("migrations"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            migrations
                .iter()
                .all(|name| name.contains("_create_articles_table.")),
            "{migrations:?}"
        );
        resource(dir.path(), "product", Some("item"), Some("name")).unwrap();
        assert!(read(&dir, "src/app/product/model.rs").contains(r#"table = "items""#));
        assert!(resource(dir.path(), "bad name!", None, None).is_err());
    }

    // #248: the field kinds no other test generates.

    #[test]
    fn whole_numbers_decimals_and_dates_in_every_part() {
        let fields = parse_fields("count:int ratio:float due:date").unwrap();
        let columns = |db| fields.iter().map(|f| f.column(db)).collect::<Vec<_>>();
        let sqlite = columns(Database::Sqlite).join(", ");
        assert!(sqlite.contains("\"count\" INTEGER"), "{sqlite}");
        assert!(
            sqlite.contains("\"due\" TEXT NOT NULL DEFAULT '1970-01-01'"),
            "{sqlite}"
        );
        let postgres = columns(Database::Postgres).join(", ");
        assert!(
            postgres.contains("\"count\" BIGINT NOT NULL DEFAULT 0"),
            "{postgres}"
        );
        assert!(postgres.contains("\"due\" DATE"), "{postgres}");

        let dir = app();
        resource(
            dir.path(),
            "readings",
            None,
            Some("count:int ratio:float due:date"),
        )
        .unwrap();
        let model = read(&dir, "src/app/readings/model.rs");
        assert!(model.contains("pub count: i64,"), "{model}");
        assert!(model.contains("pub ratio: f64,"), "{model}");
        assert!(model.contains("pub due: NaiveDate,"), "{model}");
        assert!(model.contains("use renox::chrono::NaiveDate;"), "{model}");
        let form = read(&dir, "resources/views/readings/form.html");
        assert!(
            form.contains(r#"type="number""#) && form.contains(r#"type="date""#),
            "{form}"
        );
        for view in ["index", "form", "show"] {
            let text = read(&dir, &format!("resources/views/readings/{view}.html"));
            assert!(text.contains("<rx-page"), "{view}: {text}");
            assert!(!text.contains("{%"), "{view}: {text}");
        }
        let tests = read(&dir, "tests/readings.rs");
        assert!(!tests.contains("__"), "{tests}");
        let model = read(&dir, "src/app/readings/model.rs");
        assert!(model.contains("NaiveDate"), "{model}");
        // Every generated file together: the fake values and the test's
        // sample values for each kind, the float column, and the heading
        // that falls back to the id when the first field isn't text.
        let mut all = String::new();
        for entry in walk(dir.path()) {
            all.push_str(&fs::read_to_string(entry).unwrap_or_default());
        }
        for part in [
            "(1..100).fake()",
            "(1.0..100.0).fake()",
            "NaiveDate::from_ymd_opt(2026, (1..13).fake(), (1..29).fake())",
            "\"count\", \"5\"",
            "\"ratio\", \"1.5\"",
            "\"due\", \"2026-01-15\"",
            "\"ratio\" REAL",
            "Reading #{{ record.id }}",
        ] {
            assert!(all.contains(part), "{part} in:\n{all}");
        }
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(walk(&path));
            } else {
                files.push(path);
            }
        }
        files
    }
}
