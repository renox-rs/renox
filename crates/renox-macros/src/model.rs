use heck::{ToShoutySnakeCase, ToSnakeCase};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Fields, LitStr, Result, Type};

struct Field {
    ident: syn::Ident,
    name: String,
    ty: Type,
    skip: bool,
    default: Option<LitStr>,
    references: Option<LitStr>,
    /// `#[model(money)]`: the currency column (`<name>_currency` unless shared).
    money: Option<String>,
    vis: syn::Visibility,
    form: FormField,
}

/// What `#[form(...)]` says about a field.
#[derive(Default)]
struct FormField {
    skip: bool,
    upload: bool,
    rules: Vec<TokenStream>,
    /// The first `#[form]` attribute, for the error when `#[model(form)]` is off.
    attr: Option<syn::Attribute>,
}

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let ident = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "Model can't be derived for generic structs",
        ));
    }

    let mut table = ident.to_string().to_snake_case();
    let mut soft_deletes = false;
    let mut default_scope: Option<syn::Path> = None;
    let mut hooks = false;
    let mut typed_columns = true;
    let mut search: Option<LitStr> = None;
    let mut search_language: Option<LitStr> = None;
    let mut search_normalize: Option<LitStr> = None;
    let mut index_specs: Vec<(bool, Vec<syn::Ident>)> = Vec::new();
    let mut form = false;
    let mut state_machine = false;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("model")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                table = meta.value()?.parse::<LitStr>()?.value();
                Ok(())
            } else if meta.path.is_ident("soft_deletes") {
                soft_deletes = true;
                Ok(())
            } else if meta.path.is_ident("no_typed_columns") {
                typed_columns = false;
                Ok(())
            } else if meta.path.is_ident("form") {
                form = true;
                Ok(())
            } else if meta.path.is_ident("state_machine") {
                state_machine = true;
                Ok(())
            } else if meta.path.is_ident("hooks") {
                hooks = true;
                Ok(())
            } else if meta.path.is_ident("default_scope") {
                default_scope = Some(meta.value()?.parse::<LitStr>()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("search") {
                search = Some(meta.value()?.parse::<LitStr>()?);
                Ok(())
            } else if meta.path.is_ident("search_language") {
                search_language = Some(meta.value()?.parse::<LitStr>()?);
                Ok(())
            } else if meta.path.is_ident("normalize") {
                search_normalize = Some(meta.value()?.parse::<LitStr>()?);
                Ok(())
            } else if meta.path.is_ident("index") || meta.path.is_ident("unique") {
                let unique = meta.path.is_ident("unique");
                let mut cols = Vec::new();
                meta.parse_nested_meta(|inner| {
                    cols.push(inner.path.get_ident().cloned().ok_or_else(|| {
                        inner.error("expected a column name")
                    })?);
                    Ok(())
                })?;
                if cols.is_empty() {
                    return Err(meta.error("expected at least one column"));
                }
                index_specs.push((unique, cols));
                Ok(())
            } else {
                Err(meta.error(
                    "expected `table = \"...\"`, `soft_deletes`, `hooks`, `state_machine`, `no_typed_columns`, `default_scope = \"path::to::fn\"`, `search = \"col, col\"`, `search_language = \"...\"` or `normalize = \"arabic\"`, `form`, `index(col, …)` or `unique(col, …)`",
                ))
            }
        })?;
    }

    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            ident,
            "Model can only be derived for structs",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(Error::new_spanned(
            ident,
            "Model needs a struct with named fields",
        ));
    };

    let mut fields = Vec::new();
    for field in &named.named {
        let mut skip = false;
        let mut default = None;
        let mut references = None;
        let mut money: Option<Option<String>> = None;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("model")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    skip = true;
                    Ok(())
                } else if meta.path.is_ident("default") {
                    default = Some(meta.value()?.parse::<LitStr>()?);
                    Ok(())
                } else if meta.path.is_ident("references") {
                    references = Some(meta.value()?.parse::<LitStr>()?);
                    Ok(())
                } else if meta.path.is_ident("money") {
                    money = Some(crate::from_row::parse_money(&meta)?);
                    Ok(())
                } else {
                    Err(meta.error(
                        "expected `skip`, `default = \"…\"`, `references = \"table\"`, `money` or `money(currency = \"column\")`",
                    ))
                }
            })?;
        }
        let mut form_field = FormField::default();
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("form")) {
            form_field.attr.get_or_insert_with(|| attr.clone());
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    form_field.skip = true;
                    Ok(())
                } else if meta.path.is_ident("upload") {
                    form_field.upload = true;
                    Ok(())
                } else if meta.path.is_ident("validate") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    form_field.rules.push(content.parse::<TokenStream>()?);
                    Ok(())
                } else {
                    Err(meta.error("expected `skip`, `upload` or `validate(...)`"))
                }
            })?;
        }
        if !form && let Some(attr) = &form_field.attr {
            return Err(Error::new_spanned(
                attr,
                "#[form(…)] needs #[model(form)] on the struct",
            ));
        }
        if form_field.upload && !is_string_or_option_string(&field.ty) {
            return Err(Error::new_spanned(
                &field.ty,
                "`upload` goes on a String or Option<String> column (the stored path)",
            ));
        }
        let field_ident = field.ident.clone().expect("named fields have identifiers");
        let name = field_ident.to_string().trim_start_matches("r#").to_owned();
        let money = money.map(|shared| shared.unwrap_or_else(|| format!("{name}_currency")));
        fields.push(Field {
            name,
            money,
            ident: field_ident,
            ty: field.ty.clone(),
            skip,
            default,
            references,
            vis: field.vis.clone(),
            form: form_field,
        });
    }

    let column = |name: &str| fields.iter().find(|f| f.name == name && !f.skip);
    let Some(id_field) = column("id") else {
        return Err(Error::new_spanned(
            ident,
            "a Model needs an `id` field (`i64`, `Ulid`, `Uuid` or `String`)",
        ));
    };
    let key_type = &id_field.ty;
    let deleted_at = column("deleted_at");
    if soft_deletes && deleted_at.is_none() {
        return Err(Error::new_spanned(
            ident,
            "`soft_deletes` needs a `deleted_at: Option<DateTime>` field",
        ));
    }

    let phys = physical_columns(&fields)?;
    let columns: Vec<&str> = phys.iter().map(|p| p.name.as_str()).collect();

    let mut index_items = Vec::new();
    for (unique, cols) in &index_specs {
        let mut names = Vec::new();
        for col in cols {
            let name = col.to_string();
            let name = name.trim_start_matches("r#").to_owned();
            if !columns.contains(&name.as_str()) {
                let kind = if *unique { "unique" } else { "index" };
                return Err(Error::new_spanned(
                    col,
                    format!("`{kind}`: `{name}` isn't a column of this model"),
                ));
            }
            names.push(name);
        }
        index_items.push(quote! { ::renox::db::ModelIndex::new(&[#(#names),*], #unique) });
    }
    let indexes_fn = (!index_items.is_empty()).then(|| {
        quote! {
            fn indexes() -> ::std::vec::Vec<::renox::db::ModelIndex> {
                ::std::vec![#(#index_items),*]
            }
        }
    });

    let mut searchable: Vec<String> = Vec::new();
    if let Some(lit) = &search {
        for name in lit.value().split(',').map(str::trim) {
            if name.is_empty() {
                continue;
            }
            if name == "id" || !columns.contains(&name) {
                return Err(Error::new_spanned(
                    lit,
                    format!("`search`: `{name}` isn't a text column of this model"),
                ));
            }
            if searchable.iter().any(|s| s == name) {
                return Err(Error::new_spanned(
                    lit,
                    format!("`search`: `{name}` is listed twice"),
                ));
            }
            searchable.push(name.to_owned());
        }
        if searchable.is_empty() {
            return Err(Error::new_spanned(
                lit,
                "`search` needs at least one column: `search = \"title, body\"`",
            ));
        }
    }
    let search_const = (!searchable.is_empty()).then(|| {
        quote! { const SEARCHABLE: &'static [&'static str] = &[#(#searchable),*]; }
    });
    let language_const = match &search_language {
        Some(lit) => {
            let language = lit.value();
            if language.is_empty() || !language.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            {
                return Err(Error::new_spanned(
                    lit,
                    "`search_language` is a lowercase name such as `english` or `simple`",
                ));
            }
            if searchable.is_empty() {
                return Err(Error::new_spanned(
                    lit,
                    "`search_language` needs `search = \"…\"` too",
                ));
            }
            Some(quote! { const SEARCH_LANGUAGE: &'static str = #language; })
        }
        None => None,
    };

    let normalize_const = match &search_normalize {
        Some(lit) => {
            if lit.value() != "arabic" {
                return Err(Error::new_spanned(
                    lit,
                    "`normalize` is `\"arabic\"` (the only normalisation so far)",
                ));
            }
            if searchable.is_empty() {
                return Err(Error::new_spanned(
                    lit,
                    "`normalize` needs `search = \"…\"` too",
                ));
            }
            Some(quote! {
                const SEARCH_NORMALIZE: ::core::option::Option<::renox::db::search::Normalize> =
                    ::core::option::Option::Some(::renox::db::search::Normalize::Arabic);
            })
        }
        None => None,
    };

    let from_row = fields.iter().map(|f| {
        let (ident, name) = (&f.ident, &f.name);
        if f.skip {
            quote! { #ident: ::core::default::Default::default() }
        } else if let Some(currency) = &f.money {
            crate::from_row::read_money(ident, name, currency, &f.ty)
        } else {
            quote! { #ident: row.try_get(#name)? }
        }
    });

    let values = phys.iter().filter(|p| p.name != "id").map(|p| &p.value);

    let created = column("created_at").map(|f| {
        let ident = &f.ident;
        if is_option(&f.ty) {
            quote! { if creating && self.#ident.is_none() { self.#ident = ::core::option::Option::Some(now); } }
        } else {
            quote! { if creating { self.#ident = now; } }
        }
    });
    let updated = column("updated_at").map(|f| {
        let ident = &f.ident;
        if is_option(&f.ty) {
            quote! { self.#ident = ::core::option::Option::Some(now); }
        } else {
            quote! { self.#ident = now; }
        }
    });
    // `replicate()`: a copy gets new timestamps when it is saved.
    let column_infos = phys.iter().map(|p| {
        let (name, rust_type, nullable) = (&p.name, &p.rust_type, p.nullable);
        let kind = &p.kind;
        let default = p.default.as_ref().map(|l| quote! { .default_sql(#l) });
        let references = p.references.as_ref().map(|l| quote! { .references(#l) });
        quote! {
            ::renox::db::ModelColumn::new(#name, #rust_type, #kind, #nullable)
                #default #references
        }
    });
    let forget_timestamps = ["created_at", "updated_at"].map(|name| {
        column(name).filter(|f| is_option(&f.ty)).map(|f| {
            let ident = &f.ident;
            quote! { self.#ident = ::core::option::Option::None; }
        })
    });
    let set_deleted_at = deleted_at.filter(|_| soft_deletes).map(|f| {
        let ident = &f.ident;
        quote! {
            fn set_deleted_at(&mut self, at: ::core::option::Option<::renox::db::DateTime>) {
                self.#ident = at;
            }
        }
    });

    let default_scope_fn = default_scope.map(|path| {
        quote! {
            fn default_scope(query: ::renox::db::Query<Self>) -> ::renox::db::Query<Self> {
                #path(query)
            }
        }
    });

    let state_machine_fns = state_machine.then(|| {
        quote! {
            fn guarded_column() -> ::core::option::Option<&'static str> {
                ::core::option::Option::Some(
                    <Self as ::renox::state_machine::StateMachine>::COLUMN.name(),
                )
            }
            fn check_insert(&self) -> ::renox::Result {
                ::renox::state_machine::check_insert(self)
            }
        }
    });

    let hooks_fns = hooks.then(|| {
        quote! {
            fn saving(&mut self, creating: bool) -> ::renox::Result {
                <Self as ::renox::db::ModelHooks>::saving(self, creating)
            }
            fn saved(&self, created: bool) -> impl ::std::future::Future<Output = ::renox::Result> + Send {
                <Self as ::renox::db::ModelHooks>::saved(self, created)
            }
            fn deleting(&self) -> ::renox::Result {
                <Self as ::renox::db::ModelHooks>::deleting(self)
            }
            fn deleted(&self) -> impl ::std::future::Future<Output = ::renox::Result> + Send {
                <Self as ::renox::db::ModelHooks>::deleted(self)
            }
        }
    });

    let from_row_impl = crate::from_row::from_row_impl(ident, quote! { #(#from_row),* });
    let typed_consts = typed_columns.then(|| {
        let consts = phys.iter().map(|p| {
            let konst = format_ident!("{}", p.name.to_shouty_snake_case());
            let (ty, name) = (&p.col_type, &p.name);
            let doc = format!("The `{name}` column.");
            quote! { #[doc = #doc] pub const #konst: ::renox::db::Col<#ident, #ty> = ::renox::db::Col::new(#name); }
        });
        quote! { impl #ident { #(#consts)* } }
    });
    let form_struct = form.then(|| form_tokens(&input, &fields));
    Ok(quote! {
        #from_row_impl
        #typed_consts
        #form_struct

        impl ::renox::db::Model for #ident {
            const TABLE: &'static str = #table;
            const COLUMNS: &'static [&'static str] = &[#(#columns),*];
            const SOFT_DELETES: bool = #soft_deletes;
            #search_const
            #language_const
            #normalize_const

            type Key = #key_type;

            #[allow(clippy::clone_on_copy)]
            fn id(&self) -> Self::Key {
                ::core::clone::Clone::clone(&self.id)
            }

            fn set_id(&mut self, id: Self::Key) {
                self.id = id;
            }

            fn values(&self) -> ::std::vec::Vec<::renox::db::DbValue> {
                ::std::vec![#(#values),*]
            }

            #[allow(unused_variables)]
            fn touch(&mut self, now: ::renox::db::DateTime, creating: bool) {
                #created
                #updated
            }

            fn column_info() -> ::std::vec::Vec<::renox::db::ModelColumn> {
                use ::renox::db::schema::{KnownColumn as _, UnknownColumn as _};
                ::std::vec![#(#column_infos),*]
            }

            #indexes_fn

            fn forget_timestamps(&mut self) {
                #(#forget_timestamps)*
            }

            #set_deleted_at

            #default_scope_fn

            #hooks_fns

            #state_machine_fns
        }
    })
}

/// `String` or `Option<String>` (by the last path segment).
fn is_string_or_option_string(ty: &Type) -> bool {
    let last = |ty: &Type| match ty {
        Type::Path(p) if p.qself.is_none() => p.path.segments.last().cloned(),
        _ => None,
    };
    let Some(seg) = last(ty) else { return false };
    if seg.ident == "String" {
        return true;
    }
    if seg.ident != "Option" {
        return false;
    }
    match &seg.arguments {
        syn::PathArguments::AngleBracketed(a) => matches!(
            a.args.first(),
            Some(syn::GenericArgument::Type(inner)) if last(inner).is_some_and(|s| s.ident == "String")
        ),
        _ => false,
    }
}

/// One database column of the model. A `#[model(money)]` field makes two:
/// the amount and, once per name, the currency.
struct Physical {
    name: String,
    /// The type of the typed column constant.
    col_type: TokenStream,
    rust_type: String,
    /// An expression for the `ColumnKind`.
    kind: TokenStream,
    nullable: bool,
    value: TokenStream,
    default: Option<LitStr>,
    references: Option<LitStr>,
}

fn physical_columns(fields: &[Field]) -> Result<Vec<Physical>> {
    let mut out: Vec<Physical> = Vec::new();
    for f in fields.iter().filter(|f| !f.skip) {
        let (ident, ty) = (&f.ident, &f.ty);
        let Some(currency) = &f.money else {
            out.push(Physical {
                name: f.name.clone(),
                col_type: quote!(#ty),
                rust_type: quote!(#ty).to_string(),
                kind: quote! { (&::renox::db::schema::ColumnProbe::<#ty>::new()).kind() },
                nullable: is_option(ty),
                value: quote! { ::renox::db::ToDbValue::to_db_value(&self.#ident) },
                default: f.default.clone(),
                references: f.references.clone(),
            });
            continue;
        };
        let optional = is_option(ty);
        if f.name == "id" {
            return Err(Error::new_spanned(ty, "the `id` can't be a `Money`"));
        }
        let (amount_ty, amount_value) = if optional {
            (
                quote!(::core::option::Option<i64>),
                quote! { ::renox::db::ToDbValue::to_db_value(&self.#ident.as_ref().map(|m| m.minor)) },
            )
        } else {
            (
                quote!(i64),
                quote! { ::renox::db::ToDbValue::to_db_value(&self.#ident.minor) },
            )
        };
        out.push(Physical {
            name: f.name.clone(),
            rust_type: amount_ty.to_string(),
            col_type: amount_ty,
            kind: quote!(::renox::db::ColumnKind::BigInt),
            nullable: optional,
            value: amount_value,
            default: f.default.clone(),
            references: f.references.clone(),
        });
        if out.iter().any(|p| &p.name == currency) {
            continue;
        }
        if fields
            .iter()
            .any(|o| !o.skip && o.money.is_none() && &o.name == currency)
        {
            return Err(Error::new_spanned(
                ty,
                format!("the currency column `{currency}` is also a field of this model"),
            ));
        }
        // Fields sharing the column write it from the first one that is not an
        // `Option` (or the first); the column is nullable only if all are.
        let sharing: Vec<&Field> = fields
            .iter()
            .filter(|o| !o.skip && o.money.as_ref() == Some(currency))
            .collect();
        let writer = sharing
            .iter()
            .find(|o| !is_option(&o.ty))
            .unwrap_or(&sharing[0]);
        let w = &writer.ident;
        let value = if is_option(&writer.ty) {
            quote! { ::renox::db::ToDbValue::to_db_value(&self.#w.as_ref().map(|m| m.currency)) }
        } else {
            quote! { ::renox::db::ToDbValue::to_db_value(&self.#w.currency) }
        };
        let nullable = sharing.iter().all(|o| is_option(&o.ty));
        out.push(Physical {
            name: currency.clone(),
            col_type: quote!(::std::string::String),
            rust_type: "Currency".to_owned(),
            kind: quote!(::renox::db::ColumnKind::Text),
            nullable,
            value,
            default: None,
            references: None,
        });
    }
    Ok(out)
}

fn is_option(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Option"))
}

/// `<Model>Form` and its `fill`, for `#[model(form)]`.
fn form_tokens(input: &DeriveInput, fields: &[Field]) -> TokenStream {
    let ident = &input.ident;
    let vis = &input.vis;
    let form_ident = format_ident!("{}Form", ident);
    let included: Vec<&Field> = fields
        .iter()
        .filter(|f| {
            !f.skip
                && !f.form.skip
                && !matches!(
                    f.name.as_str(),
                    "id" | "created_at" | "updated_at" | "deleted_at"
                )
        })
        .collect();
    let members = included.iter().map(|f| {
        let (field_vis, name, ty) = (&f.vis, &f.ident, &f.ty);
        if f.form.upload {
            let rules = f.form.rules.iter().map(|r| quote! { #[validate(#r)] });
            return quote! {
                #(#rules)*
                #[serde(skip_serializing)]
                #field_vis #name: ::core::option::Option<::renox::Upload>
            };
        }
        let rules = f.form.rules.iter().map(|r| quote! { #[validate(#r)] });
        let default = matches!(ty, Type::Path(p) if p.qself.is_none() && p.path.is_ident("bool"))
            .then(|| quote! { #[serde(default)] });
        quote! { #(#rules)* #default #field_vis #name: #ty }
    });
    let assigns = included.iter().filter(|f| !f.form.upload).map(|f| {
        let name = &f.ident;
        quote! { record.#name = self.#name; }
    });
    let doc = format!("The form for [`{ident}`], made by `#[model(form)]`.");
    quote! {
        #[doc = #doc]
        #[derive(::renox::serde::Deserialize, ::renox::serde::Serialize, ::renox::Validate)]
        #[serde(crate = "::renox::serde")]
        #vis struct #form_ident {
            #(#members,)*
        }

        impl #form_ident {
            /// Copies the form's values onto `record`.
            pub fn fill(self, record: &mut #ident) {
                #(#assigns)*
            }
        }
    }
}
