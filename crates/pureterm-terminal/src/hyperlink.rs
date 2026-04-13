use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Global hyperlink ID counter for generating unique IDs.
static HYPERLINK_ID: AtomicU64 = AtomicU64::new(1);

/// A hyperlink associated with terminal cells (OSC 8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hyperlink {
    /// Unique identifier for this hyperlink instance.
    pub id: u64,
    /// The URI (e.g., "https://example.com").
    pub uri: String,
    /// Optional params (e.g., "id=xyz").
    pub params: Option<String>,
}

impl Hyperlink {
    /// Create a new hyperlink with a unique ID.
    pub fn new(uri: String, params: Option<String>) -> Self {
        Self {
            id: HYPERLINK_ID.fetch_add(1, Ordering::Relaxed),
            uri,
            params,
        }
    }

    /// Parse the params string from OSC 8 into key-value pairs.
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
