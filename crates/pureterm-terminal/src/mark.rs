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
    /// 命令退出码（仅 OutputEnd 标记）。
    pub exit_code: Option<i32>,
    /// 命令持续时间（微秒）（仅 OutputEnd 标记）。
    pub duration_micros: Option<u64>,
}

impl Mark {
    pub fn new(kind: MarkKind) -> Self {
        Self { kind, label: None, exit_code: None, duration_micros: None }
    }

    pub fn with_label(kind: MarkKind, label: String) -> Self {
        Self { kind, label: Some(label), exit_code: None, duration_micros: None }
    }

    /// 解析 OSC 133 分号分隔的标记。
    /// OSC 133 ; <marker>[;<key>=<value>...] ST
    /// marker: A, B, C, D
    /// 扩展参数（D 标记）: exit=<code>, duration=<microseconds>
    pub fn from_osc133(marker: &str) -> Option<Self> {
        let mut parts = marker.split(';');
        let kind_char = parts.next()?;
        let kind = match kind_char {
            "A" => MarkKind::PromptStart,
            "B" => MarkKind::CommandStart,
            "C" => MarkKind::OutputStart,
            "D" => MarkKind::OutputEnd,
            _ => return None,
        };

        let mut mark = Self::new(kind);

        // 解析扩展参数
        for param in parts {
            if let Some(value) = param.strip_prefix("exit=") {
                mark.exit_code = value.parse().ok();
            } else if let Some(value) = param.strip_prefix("duration=") {
                mark.duration_micros = value.parse().ok();
            }
        }

        Some(mark)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_marks() {
        assert_eq!(Mark::from_osc133("A").unwrap().kind, MarkKind::PromptStart);
        assert_eq!(Mark::from_osc133("B").unwrap().kind, MarkKind::CommandStart);
        assert_eq!(Mark::from_osc133("C").unwrap().kind, MarkKind::OutputStart);
        assert_eq!(Mark::from_osc133("D").unwrap().kind, MarkKind::OutputEnd);
    }

    #[test]
    fn test_extended_d_mark() {
        let mark = Mark::from_osc133("D;exit=0;duration=123456").unwrap();
        assert_eq!(mark.kind, MarkKind::OutputEnd);
        assert_eq!(mark.exit_code, Some(0));
        assert_eq!(mark.duration_micros, Some(123456));
    }

    #[test]
    fn test_extended_d_mark_failure() {
        let mark = Mark::from_osc133("D;exit=1;duration=50000").unwrap();
        assert_eq!(mark.kind, MarkKind::OutputEnd);
        assert_eq!(mark.exit_code, Some(1));
        assert_eq!(mark.duration_micros, Some(50000));
    }

    #[test]
    fn test_plain_d_mark() {
        let mark = Mark::from_osc133("D").unwrap();
        assert_eq!(mark.kind, MarkKind::OutputEnd);
        assert_eq!(mark.exit_code, None);
        assert_eq!(mark.duration_micros, None);
    }
}
