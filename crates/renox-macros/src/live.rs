//! `#[renox::live_component]`: an `impl` block's marked methods become the
//! actions of a `LiveComponent`.

use heck::ToKebabCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Error, FnArg, ImplItem, ImplItemFn, ItemImpl, LitStr, Result};

pub fn expand(attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let mut view: Option<LitStr> = None;
    let mut name: Option<LitStr> = None;
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("view") {
            view = Some(meta.value()?.parse()?);
            Ok(())
        } else if meta.path.is_ident("name") {
            name = Some(meta.value()?.parse()?);
            Ok(())
        } else {
            Err(meta.error("expected `view = \"…\"` or `name = \"…\"`"))
        }
    });
    syn::parse::Parser::parse2(parser, attr)?;

    let mut imp: ItemImpl = syn::parse2(item)?;
    let ty = imp.self_ty.clone();
    let Some(view) = view else {
        return Err(Error::new_spanned(
            &ty,
            "live_component needs `view = \"…\"`, the template that renders it",
        ));
    };
    if imp.trait_.is_some() {
        return Err(Error::new_spanned(
            &imp,
            "live_component goes on an inherent `impl Type { … }` block",
        ));
    }
    let name = match name {
        Some(name) => name,
        None => {
            let syn::Type::Path(path) = &*ty else {
                return Err(Error::new_spanned(
                    &ty,
                    "give the component a name: `name = \"…\"`",
                ));
            };
            let last = path.path.segments.last().expect("a path has a segment");
            LitStr::new(&last.ident.to_string().to_kebab_case(), last.ident.span())
        }
    };

    let mut arms = Vec::new();
    let mut has_data = false;
    for item in &mut imp.items {
        let ImplItem::Fn(f) = item else { continue };
        if f.sig.ident == "data" {
            has_data = true;
        }
        let before = f.attrs.len();
        f.attrs.retain(|a| !is_live_action(a));
        if f.attrs.len() == before {
            continue;
        }
        arms.push(action_arm(f)?);
    }

    let data = has_data.then(|| {
        quote! {
            async fn data(
                &self,
                ctx: &::renox::live_component::LiveContext,
            ) -> ::renox::Result<::renox::serde_json::Value> {
                ::renox::serde_json::to_value(<#ty>::data(self, ctx).await?)
                    .map_err(|e| ::renox::Error::Internal(e.into()))
            }
        }
    });

    Ok(quote! {
        #imp

        impl ::renox::live_component::LiveComponent for #ty {
            const NAME: &'static str = #name;
            const VIEW: &'static str = #view;

            #data

            async fn call(
                &mut self,
                action: &str,
                args: ::std::vec::Vec<::renox::serde_json::Value>,
                ctx: &mut ::renox::live_component::LiveContext,
            ) -> ::renox::Result {
                match action {
                    #(#arms)*
                    _ => ::core::result::Result::Err(::renox::Error::NotFound),
                }
            }
        }
    })
}

fn is_live_action(attr: &syn::Attribute) -> bool {
    attr.path().is_ident("live")
        && attr
            .parse_nested_meta(|m| {
                if m.path.is_ident("action") {
                    Ok(())
                } else {
                    Err(m.error("expected `#[live(action)]`"))
                }
            })
            .is_ok()
}

fn action_arm(f: &ImplItemFn) -> Result<TokenStream> {
    let ident = &f.sig.ident;
    let action = ident.to_string();
    if action.starts_with('_') {
        return Err(Error::new_spanned(
            ident,
            "action names starting with `_` are reserved for the framework",
        ));
    }
    if f.sig.asyncness.is_none() {
        return Err(Error::new_spanned(&f.sig, "an action must be `async fn`"));
    }
    let mut inputs = f.sig.inputs.iter();
    match inputs.next() {
        Some(FnArg::Receiver(r))
            if matches!(r.kind, syn::ReceiverKind::Reference(_, _, Some(_))) => {}
        Some(other) => {
            return Err(Error::new_spanned(
                other,
                "an action takes `&mut self` as its receiver",
            ));
        }
        None => {
            return Err(Error::new_spanned(
                &f.sig,
                "an action takes `&mut self` as its receiver",
            ));
        }
    }
    if inputs.next().is_none() {
        return Err(Error::new_spanned(
            &f.sig,
            "an action's first argument after `&mut self` is `ctx: &mut LiveContext`",
        ));
    }
    let mut lets = Vec::new();
    let mut names = Vec::new();
    for (i, input) in inputs.enumerate() {
        let FnArg::Typed(t) = input else {
            return Err(Error::new_spanned(input, "unexpected receiver"));
        };
        let var = format_ident!("__arg{i}");
        let ty = &t.ty;
        lets.push(quote! { let #var: #ty = ::renox::live_component::arg(&args, #i)?; });
        names.push(var);
    }
    let n = names.len();
    Ok(quote! {
        #action => {
            ::renox::live_component::arity(&args, #n)?;
            #(#lets)*
            self.#ident(ctx, #(#names),*).await
        }
    })
}
