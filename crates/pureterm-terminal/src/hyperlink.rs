use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// 用于生成唯一 ID 的全局超链接 ID 计数器。
static HYPERLINK_ID: AtomicU64 = AtomicU64::new(1);

/// 与终端单元格关联的超链接 (OSC 8)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hyperlink {
    /// 此超链接实例的唯一标识符。
    pub id: u64,
    /// URI (例如 "https://example.com")。
    pub uri: String,
    /// 可选参数(例如 "id=xyz")。
    pub params: Option<String>,
}

impl Hyperlink {
    /// 使用唯一 ID 创建新的超链接。
    pub fn new(uri: String, params: Option<String>) -> Self {
        Self {
            id: HYPERLINK_ID.fetch_add(1, Ordering::Relaxed),
            uri,
            params,
        }
    }

    /// 将 OSC 8 的 params 字符串解析为键值对。
    pub fn parse_params(params_str: &str) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for pair in params_str.split(':') {
            if let Some((key, value)) = pair.split_once('=') {
                map.insert(key.to_string(), value.to_string());
            }
        }
        map
    }
}

impl PartialEq for Hyperlink {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::hash::Hash for Hyperlink {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
