#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

__modassets__
__modcommands__
mod model;
mod texts;

use renox::db::Migration;
use renox::prelude::*;
use serde::Serialize;

pub use model::Entry;

/// The templates, compiled in. An app replaces one with a file of the same
/// name under its views directory (`resources/views/__crate__/index.html`).
const VIEWS: &[(&str, &str)] = &[(
    "__crate__/index.html",
    include_str!("../views/__crate__/index.html"),
)];

/// The __Title__ module. Add it to the app with `.module(__lib__::__Struct__)`.
pub struct __Struct__;

/// What the page needs; `view:check` compares it with the template.
#[derive(Serialize, ViewData)]
#[view(template = "__crate__/index.html")]
struct IndexPage {
    heading: String,
    empty: String,
    entries: Vec<Entry>,
}

impl Module for __Struct__ {
    fn name(&self) -> &'static str {
        "__crate__"
    }

    // Public on purpose: add `.require_auth()` to protect the page.
    fn routes(&self) -> Routes {
        Routes::new().get("/__crate__", index).name("__lib__.index")
    }

    fn migrations(&self) -> &'static [Migration] {
        renox::migrations!()
    }

    fn register(&self, app: &mut Registry) {
        __extras__
        app.model::<Entry>().view_data::<IndexPage>();
        app.templates(|env| {
            for (name, source) in VIEWS {
                // The app's own file of that name wins.
                if env.get_template(name).is_err() {
                    renox::view::add_template(env, *name, *source)
                        .expect("a built-in template compiles");
                }
            }
        });
    }
}

async fn index(State(db): State<Db>, lang: Lang) -> Result<View> {
    let entries = Entry::query().latest().get(&db).await?;
    Ok(View::typed(IndexPage {
        heading: texts::t(&lang, "__lib__.title"),
        empty: texts::t(&lang, "__lib__.empty"),
        entries,
    }))
}
