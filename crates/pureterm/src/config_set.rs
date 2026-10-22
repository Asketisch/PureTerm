//! `config.get` / `config.set` 的纯逻辑核心 —— 点分键路径 ↔ TOML 表。
//!
//! 键名沿用 `config.list` 已公布���约定:点分路径,最后一段是**键**,
//! 前面的是**表路径**。
//!
//! ```text
//! "font.normal.family"  →  表 ["font","normal"],键 "family"
//! "font.size"           →  表 ["font"],        键 "size"
//! "pureterm.locale"     →  表 ["pureterm"],    键 "locale"
//! ```
//!
//! 为什么要单独成模块:数值类型必须**与文件里已有的一致** ——
//! `font.size` 在 TOML 里是浮点(`size = 14.0`);写成整数 `size = 14`
//! 会让 alacritty 的配置反序列化失败,应用随即**起不来**。所以写
//! 数值时若键已存在,沿用它原本是整数还是浮点。

use serde_json::Value;
use toml_edit::{DocumentMut, Item};

/// 数值键的种类(仅整数 / 浮点需要区分)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberKind {
    Integer,
    Float,
}

/// 把点分键拆成 `(表路径, 键名)`。
///
/// `None` 表示键不合法(空串 / 只有表路径没有键名)。
pub fn split_dotted_key(key: &str) -> Option<(Vec<String>, String)> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = key.split('.').map(|s| s.trim().to_string()).collect();
    // 丢掉空段("a..b" / ".a" / "a." 都是畸形键)。
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let field = parts.pop()?;
    Some((parts, field))
}

/// 读一个键的当前值(不存在 → `None`)。
pub fn read_value(doc: &DocumentMut, table: &[String], key: &str) -> Option<Value> {
    let mut cur = doc.as_table();
    for seg in table {
        cur = cur.get(seg)?.as_table()?;
    }
    let v = cur.get(key)?.as_value()?;
    Some(toml_to_json(v))
}

/// 键当前是整数还是浮点(用于写回时保持类型)。
pub fn existing_number_kind(doc: &DocumentMut, table: &[String], key: &str) -> Option<NumberKind> {
    let mut cur = doc.as_table();
    for seg in table {
        cur = cur.get(seg)?.as_table()?;
    }
    let v = cur.get(key)?.as_value()?;
    if v.is_integer() {
        Some(NumberKind::Integer)
    } else if v.is_float() {
        Some(NumberKind::Float)
    } else {
        None
    }
}

/// 把一个 JSON 值写进文档。
///
/// `existing` 是该键**原有**的数值种类(键不存在时为 `None`):
/// 整数键收到浮点请求、或浮点键收到整数请求时,按 `existing` 归一 ——
/// 否则 TOML 里会出现 `size = 14`(整数)而 alacritty 期望浮点,配置
/// 加载直接失败,应用起不来。
///
/// 返回 `Err` 的情形:值类型不受支持(数组 / 对象 / null)。
pub fn write_value(
    doc: &mut DocumentMut,
    table: &[String],
    key: &str,
    value: &Value,
    existing: Option<NumberKind>,
) -> Result<(), String> {
    let table_refs: Vec<&str> = table.iter().map(|s| s.as_str()).collect();
    let item = match value {
        Value::String(s) => Item::Value(toml_edit::Value::from(s.clone())),
        Value::Bool(b) => Item::Value(toml_edit::Value::from(*b)),
        Value::Number(n) => {
            if let Some(kind) = existing {
                match kind {
                    NumberKind::Float => {
                        let f = n.as_f64().ok_or("number is not representable as f64")?;
                        Item::Value(toml_edit::Value::from(f))
                    },
                    NumberKind::Integer => {
                        let i = n
                            .as_i64()
                            .or_else(|| n.as_f64().map(|f| f.round() as i64))
                            .ok_or("number is not representable as i64")?;
                        Item::Value(toml_edit::Value::from(i))
                    },
                }
            } else if let Some(i) = n.as_i64() {
                Item::Value(toml_edit::Value::from(i))
            } else {
                let f = n.as_f64().ok_or("number is not representable as f64")?;
                Item::Value(toml_edit::Value::from(f))
            }
        },
        other => return Err(format!("unsupported config value type: {}", json_type(other))),
    };
    let tbl = pureterm_settings::config_io::ensure_table(doc, &table_refs);
    tbl.insert(key, item);
    Ok(())
}

/// TOML 值 → JSON(用于 `config.get` 的应答)。
fn toml_to_json(value: &toml_edit::Value) -> Value {
    if let Some(b) = value.as_bool() {
        return Value::Bool(b);
    }
    if let Some(i) = value.as_integer() {
        return Value::from(i);
    }
    if let Some(f) = value.as_float() {
        return serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null);
    }
    if let Some(s) = value.as_str() {
        return Value::String(s.to_string());
    }
    if let Some(d) = value.as_datetime() {
        return Value::String(d.to_string());
    }
    if let Some(a) = value.as_array() {
        return Value::Array(a.iter().map(toml_to_json).collect());
    }
    if let Some(t) = value.as_inline_table() {
        let mut map = serde_json::Map::new();
        for (k, v) in t.iter() {
            map.insert(k.to_string(), toml_to_json(v));
        }
        return Value::Object(map);
    }
    Value::Null
}

/// JSON 值的类型名(错误信息用)。
fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(text: &str) -> DocumentMut {
        text.parse::<DocumentMut>().expect("valid toml")
    }

    #[test]
    fn splits_dotted_keys() {
        assert_eq!(
            split_dotted_key("font.normal.family"),
            Some((vec!["font".into(), "normal".into()], "family".into()))
        );
        assert_eq!(
            split_dotted_key("font.size"),
            Some((vec!["font".into()], "size".into()))
        );
        // 畸形键。
        assert_eq!(split_dotted_key(""), None);
        assert_eq!(split_dotted_key("a..b"), None);
        assert_eq!(split_dotted_key(".a"), None);
        assert_eq!(split_dotted_key("a."), None);
    }

    #[test]
    fn reads_nested_value() {
        let doc = doc_with("[font.normal]\nfamily = \"monospace\"\n");
        let (t, k) = split_dotted_key("font.normal.family").unwrap();
        assert_eq!(
            read_value(&doc, &t, &k),
            Some(Value::String("monospace".into()))
        );
        // 不存在的键 → None(而不是报错)。
        let (t, k) = split_dotted_key("font.normal.nope").unwrap();
        assert_eq!(read_value(&doc, &t, &k), None);
    }

    #[test]
    fn writes_creates_missing_tables() {
        let mut doc = doc_with("");
        let (t, k) = split_dotted_key("font.normal.family").unwrap();
        write_value(&mut doc, &t, &k, &Value::String("Menlo".into()), None).unwrap();
        assert!(doc.to_string().contains("[font.normal]"));
        assert!(doc.to_string().contains("family = \"Menlo\""));
    }

    /// 回归:`font.size` 在 TOML 里是**浮点**。写成整数 `14` 会让
    /// alacritty 的反序列化失败 → 整个配置加载失败 → 应用起不来。
    #[test]
    fn keeps_existing_float_type() {
        let mut doc = doc_with("[font]\nsize = 14.0\n");
        let (t, k) = split_dotted_key("font.size").unwrap();
        assert_eq!(existing_number_kind(&doc, &t, &k), Some(NumberKind::Float));
        // 请求值是整数 JSON(14)——但键原本是浮点,必须写浮点。
        write_value(&mut doc, &t, &k, &Value::from(14), Some(NumberKind::Float)).unwrap();
        assert!(doc.to_string().contains("size = 14.0"), "got: {}", doc);
    }

    #[test]
    fn keeps_existing_integer_type() {
        let mut doc = doc_with("[scrolling]\nhistory = 10000\n");
        let (t, k) = split_dotted_key("scrolling.history").unwrap();
        assert_eq!(existing_number_kind(&doc, &t, &k), Some(NumberKind::Integer));
        write_value(&mut doc, &t, &k, &Value::from(5000.0), Some(NumberKind::Integer)).unwrap();
        assert!(doc.to_string().contains("history = 5000"), "got: {}", doc);
    }

    #[test]
    fn new_number_keys_use_json_type() {
        let mut doc = doc_with("");
        let (t, k) = split_dotted_key("a.b").unwrap();
        write_value(&mut doc, &t, &k, &Value::from(7), None).unwrap();
        assert!(doc.to_string().contains("b = 7"), "got: {}", doc);

        let mut doc2 = doc_with("");
        let (t, k) = split_dotted_key("a.c").unwrap();
        write_value(&mut doc2, &t, &k, &Value::from(7.5), None).unwrap();
        assert!(doc2.to_string().contains("c = 7.5"), "got: {}", doc2);
    }

    #[test]
    fn unsupported_value_types_error() {
        let mut doc = doc_with("");
        let (t, k) = split_dotted_key("a.b").unwrap();
        assert!(write_value(&mut doc, &t, &k, &Value::Array(vec![]), None).is_err());
        assert!(write_value(&mut doc, &t, &k, &Value::Null, None).is_err());
        assert!(write_value(&mut doc, &t, &k, &Value::Object(Default::default()), None).is_err());
    }

    #[test]
    fn round_trip_get_after_set() {
        let mut doc = doc_with("[font]\nsize = 14.0\n");
        let (t, k) = split_dotted_key("font.size").unwrap();
        let kind = existing_number_kind(&doc, &t, &k);
        write_value(&mut doc, &t, &k, &Value::from(16), kind).unwrap();
        let (t2, k2) = split_dotted_key("font.size").unwrap();
        let v = read_value(&doc, &t2, &k2).unwrap();
        assert_eq!(v.as_f64(), Some(16.0), "got {v}");
    }
}
