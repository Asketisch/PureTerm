use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Error, Field, Generics, Ident, Type};

use crate::{Attr, GenericsStreams, MULTIPLE_FLATTEN_ERROR, serde_replace};

/// 以本 crate 名作为日志目标。
const LOG_TARGET: &str = env!("CARGO_PKG_NAME");

pub fn derive_deserialize<T>(
    ident: Ident,
    generics: Generics,
    fields: Punctuated<Field, T>,
) -> TokenStream {
    // 为实现创建全部必要的 token。
    let GenericsStreams { unconstrained, constrained, phantoms } =
        crate::generics_streams(&generics.params);
    let FieldStreams { flatten, match_assignments } = fields_deserializer(&fields);
    let visitor = format_ident!("{}Visitor", ident);

    // 生成反序列化实现。
    let mut tokens = quote! {
        #[derive(Default)]
        #[allow(non_snake_case)]
        struct #visitor <#unconstrained> {
            #phantoms
        }

        impl <'de, #constrained> serde::de::Visitor<'de> for #visitor <#unconstrained> {
            type Value = #ident <#unconstrained>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a mapping")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut config = Self::Value::default();

                // 供扁平化与警告使用的未用键。
                let mut unused = toml::Table::new();

                while let Some((key, value)) = map.next_entry::<String, toml::Value>()? {
                    match key.as_str() {
                        #match_assignments
                        _ => {
                            unused.insert(key, value);
                        },
                    }
                }

                #flatten

                // 警告未使用的键。
                for key in unused.keys() {
                    log::warn!(target: #LOG_TARGET, "Unused config key: {}", key);
                }

                Ok(config)
            }
        }

        impl <'de, #constrained> serde::Deserialize<'de> for #ident <#unconstrained> {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_map(#visitor :: default())
            }
        }
    };

    // 自动实现 [`alacritty_config::SerdeReplace`]。
    tokens.extend(serde_replace::derive_recursive(ident, generics, fields));

    tokens.into()
}

// 由结构体字段创建的 token 流。
#[derive(Default)]
struct FieldStreams {
    match_assignments: TokenStream2,
    flatten: TokenStream2,
}

/// 为 match 臂与扁平化字段创建反序列化器。
fn fields_deserializer<T>(fields: &Punctuated<Field, T>) -> FieldStreams {
    let mut field_streams = FieldStreams::default();

    // 为每个字段创建反序列化流。
    for field in fields.iter() {
        if let Err(err) = field_deserializer(&mut field_streams, field) {
            field_streams.flatten = err.to_compile_error();
            return field_streams;
        }
    }

    field_streams
}

/// 向流追加单个字段的反序列化器。
fn field_deserializer(field_streams: &mut FieldStreams, field: &Field) -> Result<(), Error> {
    let ident = field.ident.as_ref().expect("unreachable tuple struct");
    let literal = ident.to_string();
    let mut literals = vec![literal.clone()];

    // 创建反序列化字段的默认流。
    let mut match_assignment_stream = quote! {
        match serde::Deserialize::deserialize(value) {
            Ok(value) => config.#ident = value,
            Err(err) => {
                log::error!(
                    target: #LOG_TARGET,
                    "Config error: {}: {}",
                    #literal,
                    err.to_string().trim(),
                );
            },
        }
    };

    // 遍历所有 #[config(...)] 属性。
    for attr in field.attrs.iter().filter(|attr| attr.path().is_ident("config")) {
        let parsed = match attr.parse_args::<Attr>() {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };

        match parsed.ident.as_str() {
            // 跳过 `#[config(skip)]` 字段的反序列化。
            "skip" => return Ok(()),
            "flatten" => {
                // 注意:出于复杂度考虑,目前每个结构体只支持单个 flatten 实例。
                // (即不支持同时出现多个 flatten 字段)。
                if !field_streams.flatten.is_empty() {
                    return Err(Error::new(attr.span(), MULTIPLE_FLATTEN_ERROR));
                }

                // 创建从未用字段反序列化扁平化结构体的 token。
                field_streams.flatten.extend(quote! {
                    // 排空未用字段,它们将用于扁平化。
                    let flattened = std::mem::replace(&mut unused, toml::Table::new());

                    config.#ident = serde::Deserialize::deserialize(flattened).unwrap_or_default();
                });
            },
            "deprecated" | "removed" => {
                // 构建弃用/移除消息,支持属性覆盖。
                let mut message = format!("Config warning: {} has been {}", literal, parsed.ident);
                if let Some(warning) = parsed.param {
                    message = format!("{}; {}", message, warning.value());
                }
                message.push_str("\nUse `alacritty migrate` to automatically resolve it");

                // 追加记录弃用/移除警告的流。
                match_assignment_stream.extend(quote! {
                    log::warn!(target: #LOG_TARGET, #message);
                });
            },
            // 向 match 模式添加别名。
            "alias" => {
                if let Some(alias) = parsed.param {
                    literals.push(alias.value());
                }
            },
            _ => (),
        }
    }

    // 创建把 "none" 字符串反序列化为 `Option<T>` 的 token 流。
    if let Type::Path(type_path) = &field.ty {
        if type_path.path.segments.iter().next_back().is_some_and(|s| s.ident == "Option") {
            match_assignment_stream = quote! {
                if value.as_str().is_some_and(|s| s.eq_ignore_ascii_case("none")) {
                    config.#ident = None;
                    continue;
                }
                #match_assignment_stream
            };
        }
    }

    // 创建反序列化与错误处理的 token 流。
    field_streams.match_assignments.extend(quote! {
        #(#literals)|* => { #match_assignment_stream },
    });

    Ok(())
}
