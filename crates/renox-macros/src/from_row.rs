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
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("row")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    skip = true;
                    Ok(())
                } else if meta.path.is_ident("rename") {
                    column = meta.value()?.parse::<LitStr>()?.value();
                    Ok(())
                } else {
                    Err(meta.error("expected `skip` or `rename = \"column\"`"))
                }
            })?;
        }
        fields.push(if skip {
            quote! { #field_ident: ::core::default::Default::default() }
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
