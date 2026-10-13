use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{Data, DeriveInput, Error, Fields, LitStr, Meta, Result, Token};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let mut hooks = false;
    let mut bag: Option<LitStr> = None;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("validate")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("hooks") {
                hooks = true;
                Ok(())
            } else if meta.path.is_ident("bag") {
                bag = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error(
                    "expected `hooks` or `bag = \"name\"` on the struct (rules go on its fields)",
                ))
            }
        })?;
    }
    let bag = bag.map(|bag| {
        quote! { const ERROR_BAG: ::core::option::Option<&'static str> = ::core::option::Option::Some(#bag); }
    });

    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            ident,
            "Validate can only be derived for structs",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(Error::new_spanned(
            ident,
            "Validate needs a struct with named fields",
        ));
    };

    let mut statements = Vec::new();
    for field in &named.named {
        let field_ident = field.ident.clone().expect("named fields have identifiers");
        let mut name = field_ident.to_string().trim_start_matches("r#").to_owned();
        let mut chain = Vec::new();
        let mut each: Option<Vec<TokenStream>> = None;
        let mut distinct = false;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("validate")) {
            let items = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
            for item in items {
                if item.path().is_ident("rename") {
                    let Meta::NameValue(value) = &item else {
                        return Err(Error::new_spanned(item, "expected `rename = \"field\"`"));
                    };
                    name = syn::parse2::<LitStr>(value.value.to_token_stream())?.value();
                } else if item.path().is_ident("distinct") {
                    distinct = true;
                } else if item.path().is_ident("each") {
                    let Meta::List(list) = &item else {
                        return Err(Error::new_spanned(item, "expected `each(rule, …)`"));
                    };
                    let inner =
                        list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
                    each = Some(inner.iter().map(call).collect::<Result<_>>()?);
                } else if item.path().is_ident("label") {
                    // First: messages are made when a rule fails, with the
                    // label known by then.
                    chain.insert(0, call(&item)?);
                } else {
                    chain.push(call(&item)?);
                }
            }
        }
        if !chain.is_empty() {
            statements.push(quote! {
                v.field(#name, &self.#field_ident) #(#chain)*;
            });
        }
        if let Some(each) = each {
            let item = format_ident!("item");
            statements.push(quote! {
                v.each(#name, &self.#field_ident, |#item| #item #(#each)*);
            });
        }
        if distinct {
            statements.push(quote! {
                v.distinct(#name, &self.#field_ident);
            });
        }
    }

    let hooks_fns = hooks.then(|| {
        quote! {
            fn prepare(&mut self) {
                <Self as ::renox::validation::ValidateHooks>::prepare(self)
            }
            fn authorize(
                &self,
                form: &::renox::validation::FormContext<'_>,
            ) -> impl ::std::future::Future<Output = ::renox::Result<bool>> + Send {
                <Self as ::renox::validation::ValidateHooks>::authorize(self, form)
            }
            fn after(
                &self,
                form: &::renox::validation::FormContext<'_>,
                errors: &mut ::renox::Errors,
            ) -> impl ::std::future::Future<Output = ::renox::Result> + Send {
                <Self as ::renox::validation::ValidateHooks>::after(self, form, errors)
            }
        }
    });

    Ok(quote! {
        impl #impl_generics ::renox::Validate for #ident #type_generics #where_clause {
            #bag

            #[allow(unused_variables)]
            fn rules(&self, v: &mut ::renox::Validator) {
                #(#statements)*
            }

            #hooks_fns
        }
    })
}

/// A rule as a method call on the field: `required` → `.required()`,
/// `max = 100` → `.max(100)`, `unique("users", "email")` →
/// `.unique("users", "email")`.
fn call(item: &Meta) -> Result<TokenStream> {
    let method = item
        .path()
        .get_ident()
        .ok_or_else(|| Error::new_spanned(item.path(), "expected a rule name, e.g. `required`"))?;
    Ok(match item {
        Meta::Path(_) => quote! { .#method() },
        Meta::NameValue(value) => {
            let value = &value.value;
            quote! { .#method(#value) }
        }
        Meta::List(list) => {
            let args = &list.tokens;
            quote! { .#method(#args) }
        }
    })
}

use quote::ToTokens;
