use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::meta::ParseNestedMeta;
use syn::{DataEnum, Generics, Ident};

use crate::serde_replace;

pub fn derive_deserialize(ident: Ident, generics: Generics, data_enum: DataEnum) -> TokenStream {
    let visitor = format_ident!("{}Visitor", ident);

    // 创建 match 臂流并取得所有可用值的列表。
    let mut match_arms_stream = TokenStream2::new();
    let mut available_values = String::from("one of ");
    for variant in data_enum.variants.iter().filter(|variant| {
        // 跳过 `#[config(skip)]` 字段的反序列化。
        variant.attrs.iter().all(|attr| {
            let is_skip = |meta: ParseNestedMeta| {
                if meta.path.is_ident("skip") { Ok(()) } else { Err(meta.error("not skip")) }
            };
            !attr.path().is_ident("config") || attr.parse_nested_meta(is_skip).is_err()
        })
    }) {
        let variant_ident = &variant.ident;
        let variant_str = variant_ident.to_string();
        available_values = format!("{available_values}`{variant_str}`, ");

        let literal = variant_str.to_lowercase();

        match_arms_stream.extend(quote! {
            #literal => Ok(#ident :: #variant_ident),
        });
    }

    // 移除最后一个枚举变体尾部的 `, `。
    available_values.truncate(available_values.len().saturating_sub(2));

    // 生成反序列化实现。
    let mut tokens = quote! {
        struct #visitor;
        impl<'de> serde::de::Visitor<'de> for #visitor {
            type Value = #ident;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(#available_values)
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match s.to_lowercase().as_str() {
                    #match_arms_stream
                    _ => Err(E::custom(
                            &format!("unknown variant `{}`, expected {}", s, #available_values)
                    )),
                }
            }
        }

        impl<'de> serde::Deserialize<'de> for #ident {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_str(#visitor)
            }
        }
    };

    // 自动实现 [`alacritty_config::SerdeReplace`]。
    tokens.extend(serde_replace::derive_direct(ident, generics));

    tokens.into()
}
