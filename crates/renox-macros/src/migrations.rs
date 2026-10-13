use std::collections::BTreeMap;
use std::path::PathBuf;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Error, LitStr, Result};

#[derive(Default)]
struct Files {
    up: Option<PathBuf>,
    down: Option<PathBuf>,
}

/// A migration's plain files and each database's own versions.
#[derive(Default)]
struct Set {
    plain: Files,
    sqlite: Files,
    postgres: Files,
}

impl Set {
    fn files(&mut self, dialect: Option<&str>) -> &mut Files {
        match dialect {
            Some("sqlite") => &mut self.sqlite,
            Some("postgres") => &mut self.postgres,
            _ => &mut self.plain,
        }
    }
}

fn include(path: &Option<PathBuf>) -> TokenStream {
    match path {
        Some(path) => {
            let path = path.to_string_lossy().into_owned();
            quote! { ::core::option::Option::Some(::core::include_str!(#path)) }
        }
        None => quote! { ::core::option::Option::None },
    }
}

/// `name.postgres` -> (`name`, Some("postgres")); other names are plain.
fn split_dialect(stem: &str) -> (&str, Option<&str>) {
    for dialect in ["sqlite", "postgres"] {
        if let Some(name) = stem.strip_suffix(&format!(".{dialect}")) {
            return (name, Some(dialect));
        }
    }
    (stem, None)
}

pub fn expand(input: TokenStream) -> Result<TokenStream> {
    let dir = if input.is_empty() {
        "migrations".to_owned()
    } else {
        syn::parse2::<LitStr>(input)?.value()
    };
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| Error::new(Span::call_site(), "CARGO_MANIFEST_DIR is not set"))?;
    let path = PathBuf::from(root).join(&dir);

    let mut migrations: BTreeMap<String, Set> = BTreeMap::new();
    if path.is_dir() {
        let entries = std::fs::read_dir(&path).map_err(|e| {
            Error::new(
                Span::call_site(),
                format!("reading {}: {e}", path.display()),
            )
        })?;
        for entry in entries.flatten() {
            let file = entry.path();
            let Some(file_name) = file.file_name().and_then(|n| n.to_str()).map(str::to_owned)
            else {
                continue;
            };
            let (stem, down) = if let Some(stem) = file_name.strip_suffix(".down.sql") {
                (stem, true)
            } else if let Some(stem) = file_name.strip_suffix(".up.sql") {
                (stem, false)
            } else if let Some(stem) = file_name.strip_suffix(".sql") {
                (stem, false)
            } else {
                continue;
            };
            let (name, dialect) = split_dialect(stem);
            let files = migrations
                .entry(name.to_owned())
                .or_default()
                .files(dialect);
            if down {
                files.down = Some(file);
            } else {
                files.up = Some(file);
            }
        }
    }

    let mut items = Vec::new();
    for (name, set) in &migrations {
        for (dialect, files) in [
            ("sqlite", &set.sqlite),
            ("postgres", &set.postgres),
            ("", &set.plain),
        ] {
            // A database's own `down` is used only with its own `up`: next to
            // a plain `up` it would be ignored, and rolling back would run the
            // plain `down` instead, so it's refused.
            if files.down.is_some() && files.up.is_none() {
                let dot = if dialect.is_empty() {
                    String::new()
                } else {
                    format!(".{dialect}")
                };
                return Err(Error::new(
                    Span::call_site(),
                    format!(
                        "migration `{name}` has a {dot}.down.sql but no {dot}.up.sql in {dir} \
                         (a database's own down is used only with its own up)"
                    ),
                ));
            }
        }
        if set.plain.up.is_none() && (set.sqlite.up.is_none() || set.postgres.up.is_none()) {
            return Err(Error::new(
                Span::call_site(),
                format!(
                    "migration `{name}` needs a {name}.up.sql, or both {name}.sqlite.up.sql \
                     and {name}.postgres.up.sql, in {dir}"
                ),
            ));
        }
        let up = match &set.plain.up {
            Some(up) => {
                let up = up.to_string_lossy().into_owned();
                quote! { ::core::include_str!(#up) }
            }
            None => quote! { "" },
        };
        let down = include(&set.plain.down);
        // `.sqlite(up, down)` / `.postgres(up, down)` for a database's own files.
        let own = |method: proc_macro2::TokenStream, files: &Files| match &files.up {
            Some(up) => {
                let up = up.to_string_lossy().into_owned();
                let down = include(&files.down);
                quote! { .#method(::core::include_str!(#up), #down) }
            }
            None => quote! {},
        };
        let sqlite = own(quote! { sqlite }, &set.sqlite);
        let postgres = own(quote! { postgres }, &set.postgres);
        items.push(quote! {
            ::renox::db::Migration::new(#name, #up, #down) #sqlite #postgres
        });
    }

    Ok(quote! {{
        const MIGRATIONS: &[::renox::db::Migration] = &[#(#items),*];
        MIGRATIONS
    }})
}
