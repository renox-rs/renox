use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, LitStr, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let mut template: Option<LitStr> = None;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("view")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("template") {
                template = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("expected `template = \"page.html\"`"))
            }
        })?;
    }
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("serde")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                return Err(meta.error(
                    "ViewData doesn't follow `rename_all`: rename the fields one by one with `#[serde(rename = \"…\")]`",
                ));
            }
            // Other serde options don't change the names.
            skip_value(&meta)
        })?;
    }
    let Some(template) = template else {
        return Err(Error::new_spanned(
            ident,
            "ViewData needs the page's template: add `#[view(template = \"page.html\")]` to the struct",
        ));
    };

    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            ident,
            "ViewData can only be derived for structs",
        ));
    };
    let fields = match &data.fields {
        Fields::Named(named) => &named.named,
        Fields::Unit => &syn::punctuated::Punctuated::new(),
        Fields::Unnamed(_) => {
            return Err(Error::new_spanned(
                ident,
                "ViewData needs a struct with named fields (they are the template's variables)",
            ));
        }
    };

    let mut names = Vec::new();
    for field in fields {
        let field_ident = field.ident.clone().expect("named fields have identifiers");
        let mut name = field_ident.to_string().trim_start_matches("r#").to_owned();
        let mut skipped = false;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("serde")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    if meta.input.peek(syn::Token![=]) {
                        name = meta.value()?.parse::<LitStr>()?.value();
                    } else {
                        // `rename(serialize = "…", deserialize = "…")`
                        meta.parse_nested_meta(|inner| {
                            let value = inner.value()?.parse::<LitStr>()?.value();
                            if inner.path.is_ident("serialize") {
                                name = value;
                            }
                            Ok(())
                        })?;
                    }
                } else if meta.path.is_ident("skip") || meta.path.is_ident("skip_serializing") {
                    skipped = true;
                } else if meta.path.is_ident("flatten") {
                    return Err(meta.error(
                        "ViewData can't list a flattened field's names: make it a field of its own",
                    ));
                } else {
                    skip_value(&meta)?;
                }
                Ok(())
            })?;
        }
        if !skipped {
            names.push(name);
        }
    }

    Ok(quote! {
        impl #impl_generics ::renox::view::ViewData for #ident #type_generics #where_clause {
            const TEMPLATE: &'static str = #template;
            const FIELDS: &'static [&'static str] = &[#(#names),*];
        }
    })
}

/// Reads past a serde option this derive doesn't need: `name`, `name = value` or
/// `name(…)`.
fn skip_value(meta: &syn::meta::ParseNestedMeta) -> Result<()> {
    if meta.input.peek(syn::Token![=]) {
        meta.value()?.parse::<syn::Expr>()?;
    } else if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        content.parse::<TokenStream>()?;
    }
    Ok(())
}
