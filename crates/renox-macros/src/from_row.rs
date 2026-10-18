use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, LitStr, Result};

/// `impl FromRow`: each field is read from the column of the same name, or
/// `#[row(rename = "col")]`; `#[row(skip)]` fields get `Default`.
pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let ident = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "FromRow can't be derived for generic structs",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            ident,
            "FromRow can only be derived for structs",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(Error::new_spanned(
            ident,
            "FromRow needs a struct with named fields",
        ));
    };
    let mut fields = Vec::new();
    for field in &named.named {
        let field_ident = field.ident.clone().expect("named fields have identifiers");
        let mut column = field_ident.to_string().trim_start_matches("r#").to_owned();
        let mut skip = false;
        let mut money = None;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("row")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    skip = true;
                    Ok(())
                } else if meta.path.is_ident("rename") {
                    column = meta.value()?.parse::<LitStr>()?.value();
                    Ok(())
                } else if meta.path.is_ident("money") {
                    money = Some(parse_money(&meta)?);
                    Ok(())
                } else {
                    Err(meta.error(
                        "expected `skip`, `rename = \"column\"`, `money` or `money(currency = \"column\")`",
                    ))
                }
            })?;
        }
        fields.push(if skip {
            quote! { #field_ident: ::core::default::Default::default() }
        } else if let Some(currency) = money {
            let currency = currency.unwrap_or_else(|| format!("{column}_currency"));
            read_money(&field_ident, &column, &currency, &field.ty)
        } else {
            quote! { #field_ident: row.try_get(#column)? }
        });
    }
    Ok(from_row_impl(ident, quote! { #(#fields),* }))
}

pub fn from_row_impl(ident: &syn::Ident, fields: TokenStream) -> TokenStream {
    quote! {
        impl ::renox::db::FromRow for #ident {
            fn from_row(
                row: &::renox::db::Row,
            ) -> ::core::result::Result<Self, ::renox::db::DbError> {
                ::core::result::Result::Ok(Self { #fields })
            }
        }
    }
}

/// Parses `money` or `money(currency = "column")`; the inner value is the
/// shared currency column, `None` for the default `<column>_currency`.
pub fn parse_money(meta: &syn::meta::ParseNestedMeta) -> Result<Option<String>> {
    let mut currency = None;
    if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|inner| {
            if inner.path.is_ident("currency") {
                currency = Some(inner.value()?.parse::<LitStr>()?.value());
                Ok(())
            } else {
                Err(inner.error("expected `currency = \"column\"`"))
            }
        })?;
    }
    Ok(currency)
}

/// `field: row.try_get_money(amount, currency)?` (or the `_opt` form for an `Option`).
pub fn read_money(ident: &syn::Ident, amount: &str, currency: &str, ty: &syn::Type) -> TokenStream {
    if is_option(ty) {
        quote! { #ident: row.try_get_money_opt(#amount, #currency)? }
    } else {
        quote! { #ident: row.try_get_money(#amount, #currency)? }
    }
}

pub fn is_option(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Option"))
}
