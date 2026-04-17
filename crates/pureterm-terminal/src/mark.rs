use serde::{Deserialize, Serialize};

/// 行的标记类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkKind {
    /// Shell 提示符开始 (OSC 133;A)。
    PromptStart,
    /// 命令开始 (OSC 133;B)。
    CommandStart,
    /// 命令输出开始 (OSC 133;C)。
    OutputStart,
    /// 命令输出结束 (OSC 133;D)。
    OutputEnd,
    /// 用户定义的标记。
    User,
}

/// 与网格中一行关联的标记。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mark {
    pub kind: MarkKind,
    /// 用户标记的可选标签。
    pub label: Option<String>,
}

impl Mark {
    pub fn new(kind: MarkKind) -> Self {
        Self { kind, label: None }
    }

    pub fn with_label(kind: MarkKind, label: String) -> Self {
        Self {
            kind,
            label: Some(label),
        }
    }

    /// 解析 OSC 133 分号分隔的标记。
    /// OSC 133 ; <marker> ST
    /// marker: A, B, C, D
    pub fn from_osc133(marker: &str) -> Option<Self> {
        match marker {
            "A" => Some(Self::new(MarkKind::PromptStart)),
            "B" => Some(Self::new(MarkKind::CommandStart)),
            "C" => Some(Self::new(MarkKind::OutputStart)),
            "D" => Some(Self::new(MarkKind::OutputEnd)),
            _ => None,
        }
    }
}
