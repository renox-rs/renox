use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, LitStr, Result};

pub fn expand(input: DeriveInput) -> Result<TokenStream> {
    let ident = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(Error::new_spanned(ident, "DbEnum works on enums"));
    };
    let mut variants = Vec::new();
    let mut names = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                variant,
                "DbEnum variants can't have fields",
            ));
        }
        let mut name = variant.ident.to_string().to_snake_case();
        for attr in &variant.attrs {
            if attr.path().is_ident("db") {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rename") {
                        name = meta.value()?.parse::<LitStr>()?.value();
                        Ok(())
                    } else {
                        Err(meta.error("expected `rename = \"...\"`"))
                    }
                })?;
            }
        }
        if names.contains(&name) {
            return Err(Error::new_spanned(
                variant,
                format!("`{name}` is used twice"),
            ));
        }
        variants.push(&variant.ident);
        names.push(name);
    }
    let type_name = ident.to_string();
    let expected = names.join(", ");

    Ok(quote! {
        impl #ident {
            /// Every variant, e.g. for a `<select>`.
            pub const ALL: &'static [Self] = &[#(Self::#variants),*];

            /// The text stored in the database and sent in forms and JSON.
            pub fn as_str(&self) -> &'static str {
                match self {
                    #(Self::#variants => #names,)*
                }
            }
        }

        impl ::core::fmt::Display for #ident {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::core::str::FromStr for #ident {
            type Err = ::std::string::String;

            fn from_str(text: &str) -> ::core::result::Result<Self, Self::Err> {
                match text {
                    #(#names => ::core::result::Result::Ok(Self::#variants),)*
                    other => ::core::result::Result::Err(::std::format!(
                        "`{}` is not a {}; expected one of: {}", other, #type_name, #expected
                    )),
                }
            }
        }

        impl ::renox::db::ToDbValue for #ident {
            fn to_db_value(&self) -> ::renox::db::DbValue {
                ::renox::db::DbValue::Text(::std::string::ToString::to_string(self.as_str()))
            }
        }

        impl ::renox::db::ColumnType for #ident {
            const KIND: ::renox::db::ColumnKind = ::renox::db::ColumnKind::Text;
        }

        impl ::renox::serde::Serialize for #ident {
            fn serialize<S: ::renox::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::core::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> ::renox::serde::Deserialize<'de> for #ident {
            fn deserialize<D: ::renox::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::core::result::Result<Self, D::Error> {
                let text = <::std::string::String as ::renox::serde::Deserialize>::deserialize(deserializer)?;
                text.parse().map_err(<D::Error as ::renox::serde::de::Error>::custom)
            }
        }

        ::renox::__db_text_type!(#ident);
    })
}
