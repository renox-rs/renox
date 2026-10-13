use std::path::{Path, PathBuf};

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Error, Result};

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| Error::new(Span::call_site(), e.to_string()))?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((relative, path));
        }
    }
    Ok(())
}

fn files(root: &Path) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out
        .into_iter()
        .map(|(name, path)| (name, path.to_string_lossy().into_owned()))
        .collect())
}

pub fn expand(input: TokenStream) -> Result<TokenStream> {
    if !input.is_empty() {
        return Err(Error::new(
            Span::call_site(),
            "embedded!() takes no arguments",
        ));
    }
    let root = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| Error::new(Span::call_site(), "CARGO_MANIFEST_DIR is not set"))?;
    let root = PathBuf::from(root);

    let text = |dir: &str| -> Result<Vec<TokenStream>> {
        Ok(files(&root.join(dir))?
            .into_iter()
            .map(|(name, path)| quote! { (#name, ::core::include_str!(#path)) })
            .collect())
    };
    let views = text("resources/views")?;
    let lang = text("resources/lang")?;
    let public: Vec<TokenStream> = files(&root.join("public"))?
        .into_iter()
        .map(|(name, path)| quote! { (#name, ::core::include_bytes!(#path) as &[u8]) })
        .collect();

    Ok(quote! {
        ::renox::Embedded {
            views: &[#(#views),*],
            lang: &[#(#lang),*],
            public: &[#(#public),*],
        }
    })
}
