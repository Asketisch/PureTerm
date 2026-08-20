//! GAP-6.7:统一的事件 schema。
//!
//! GUI 关心的每次状态转移都会记录为一条 [`AppEvent`]:
//! 标签页/窗格生命周期、配置重载、feed 审批、agent 会话、PTY
//! 生成/退出等。由 [`EventBus`](crate::events_bus::EventBus) 把每条
//! 事件持久化到 `events.jsonl` 并广播给进程内订阅者;CLI
//! (`pureterm events`)会 tail 该文件以实时消费。
//!
//! 本模块只提供稳定的公开 schema——不含 I/O,也不含总线逻辑。

use serde_json::Value;

/// 16 种事件分类。序列化为小写(`Feed` → `"feed"`)。
///
/// `Session` 承载 PTY/守护进程会话的生命周期(生成/退出/重连/
/// 休眠/恢复),作为一等关注点,而非折叠进
/// `Surface` —— GAP-2.1 会重新定义它,现在做别名将来就得改名。
/// `Notification`/`Sidebar` 保留:它们的生产者随未来
/// GAP 落地(通知系统 / 可编程侧边栏)。`Workspace`/`Browser`
/// 由 GAP-2.1 / GAP-3.5 产生;`Git` 由 GAP-32 产生;`Shell` 由 GAP-15 产生。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Window,
    Workspace,
    Surface,
    Pane,
    Session,
    Notification,
    Feed,
    Agent,
    Sidebar,
    Browser,
    App,
    Config,
    Git,
    /// GAP-15:shell 集成标记(提示符/命令/输出边界、退出码),
    /// 来自从 PTY 解析出的 OSC 133 序列。
    Shell,
    /// GAP-02:错误恢复——命令失败时的 LLM 修复建议。
    ErrorRecovery,
    /// GAP-10:模式匹配触发器——每次匹配到终端输出中的正则时,
    /// 都会发出 `trigger.matched`。
    Trigger,
}

impl Category {
    /// 小写传输名(与 serde 表示一致),用于
    /// `--type` 分类前缀过滤,而无需经过 serialize。
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Window => "window",
            Category::Workspace => "workspace",
            Category::Surface => "surface",
            Category::Pane => "pane",
            Category::Session => "session",
            Category::Notification => "notification",
            Category::Feed => "feed",
            Category::Agent => "agent",
            Category::Sidebar => "sidebar",
            Category::Browser => "browser",
            Category::App => "app",
            Category::Config => "config",
            Category::Git => "git",
            Category::Shell => "shell",
            Category::ErrorRecovery => "error_recovery",
            Category::Trigger => "trigger",
        }
    }
}

/// 一条结构化事件。以每行一条序列化进 `events.jsonl`。
///
/// 字段顺序固定,便于直接阅读原始日志:`seq`(单调
/// id)、`ts`(epoch 毫秒)、`cat`(分类)、`kind`(`"<cat>.<verb>"`)、
/// `session_id`(可选)、`data`(任意载荷)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppEvent {
    pub seq: u64,
    pub ts: u64,
    pub cat: Category,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

impl AppEvent {
    /// 用给定的 kind + 载荷构造事件;`seq`/`ts` 由
    /// [`EventBus::emit`](crate::events_bus::EventBus::emit) 填充。
    pub fn new(cat: Category, kind: impl Into<String>, session_id: Option<String>, data: Value) -> Self {
        Self {
            seq: 0,
            ts: 0,
            cat,
            kind: kind.into(),
            session_id,
            data,
        }
    }
}

/// 解析后的 `--type` 过滤器。`--type feed` → 只看分类;`--type feed.resolved`
/// → 分类 + 精确 kind。空过滤器(无 `--type`)匹配全部。
#[derive(Debug, Clone, Default)]
pub struct TypeFilter {
    pub category: Option<String>,
    pub kind: Option<String>,
}

impl TypeFilter {
    /// 解析 `--type` 参数。`None` 或空 → 匹配全部。
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::default();
        };
        match raw.split_once('.') {
            Some((cat, kind)) if !kind.is_empty() => Self {
                category: Some(cat.to_string()),
                kind: Some(kind.to_string()),
            },
            _ => Self {
                category: Some(raw.to_string()),
                kind: None,
            },
        }
    }

    /// 该事件是否通过此过滤器?
    pub fn matches(&self, event: &AppEvent) -> bool {
        if let Some(cat) = &self.category {
            if event.cat.as_str() != cat.as_str() {
                return false;
            }
        }
        if let Some(kind) = &self.kind {
            // 带点的 `cat.verb` 锁定精确的完整 kind `cat.verb`(分类已在
            // 上面校验过)。若用后缀匹配,`--type feed.resolved` 会
            // 错误匹配假设中的 `feed.unresolved`,与
            // [`TypeFilter`] 上记录的「精确 kind」约定相矛盾。
            let expected: String = match &self.category {
                Some(cat) => format!("{cat}.{kind}"),
                None => kind.clone(),
            };
            if event.kind != expected {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(cat: Category, kind: &str) -> AppEvent {
        AppEvent::new(cat, kind, None, Value::Null)
    }

    #[test]
    fn category_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Category::Feed).unwrap(),
            "\"feed\""
        );
        assert_eq!(Category::Session.as_str(), "session");
        // GAP-32:Git 分类在两条路径上都以小写往返。
        assert_eq!(serde_json::to_string(&Category::Git).unwrap(), "\"git\"");
        assert_eq!(Category::Git.as_str(), "git");
    }

    #[test]
    fn app_event_round_trips_through_jsonl() {
        let mut e = ev(Category::Feed, "feed.requested");
        e.seq = 7;
        e.ts = 1719000000000;
        e.session_id = Some("abc".into());
        e.data = serde_json::json!({"tool": "Bash"});
        let line = serde_json::to_string(&e).unwrap();
        let back: AppEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(back.seq, 7);
        assert_eq!(back.cat, Category::Feed);
        assert_eq!(back.kind, "feed.requested");
        assert_eq!(back.session_id.as_deref(), Some("abc"));
        assert_eq!(back.data["tool"], "Bash");
    }

    #[test]
    fn app_event_omits_null_data_and_none_session() {
        let e = ev(Category::App, "app.started");
        let line = serde_json::to_string(&e).unwrap();
        assert!(!line.contains("session_id"));
        assert!(!line.contains("data"));
    }

    #[test]
    fn type_filter_category_prefix_matches_namespace() {
        let f = TypeFilter::parse(Some("feed"));
        assert!(f.matches(&ev(Category::Feed, "feed.requested")));
        assert!(f.matches(&ev(Category::Feed, "feed.resolved")));
        assert!(!f.matches(&ev(Category::App, "app.started")));
    }

    #[test]
    fn type_filter_dotted_matches_exact_kind_suffix() {
        let f = TypeFilter::parse(Some("feed.resolved"));
        assert!(f.matches(&ev(Category::Feed, "feed.resolved")));
        assert!(!f.matches(&ev(Category::Feed, "feed.requested")));
    }

    #[test]
    fn type_filter_dotted_pins_exact_kind_not_suffix() {
        // 回归:`--type feed.resolved` 不得后缀匹配 `feed.unresolved`。
        // 带点的过滤器锁定精确完整 kind `cat.verb`,而非尾部后缀。
        let f = TypeFilter::parse(Some("feed.resolved"));
        assert!(f.matches(&ev(Category::Feed, "feed.resolved")));
        assert!(!f.matches(&ev(Category::Feed, "feed.unresolved")));
        // 多段 kind 也精确匹配,而非按尾部后缀。
        let g = TypeFilter::parse(Some("error_recovery.applied"));
        assert!(g.matches(&ev(Category::ErrorRecovery, "error_recovery.applied")));
        assert!(!g.matches(&ev(Category::ErrorRecovery, "error_recovery.not_applied")));
    }

    #[test]
    fn type_filter_empty_matches_all() {
        let f = TypeFilter::parse(None);
        assert!(f.matches(&ev(Category::App, "app.started")));
        let f2 = TypeFilter::parse(Some(""));
        assert!(f2.matches(&ev(Category::App, "app.started")));
    }

    #[test]
    fn type_filter_matches_git_category() {
        // GAP-32:`--type git` 显示 git.* 事件流。
        let f = TypeFilter::parse(Some("git"));
        assert!(f.matches(&ev(Category::Git, "git.branch_changed")));
        assert!(f.matches(&ev(Category::Git, "git.quick_action")));
        assert!(!f.matches(&ev(Category::App, "app.started")));
    }

    #[test]
    fn type_filter_matches_shell_category() {
        // GAP-15:`--type shell` 显示 shell.* 事件流。
        let f = TypeFilter::parse(Some("shell"));
        assert!(f.matches(&ev(Category::Shell, "shell.command_finished")));
        assert!(f.matches(&ev(Category::Shell, "shell.prompt_started")));
        assert!(!f.matches(&ev(Category::App, "app.started")));
    }

    #[test]
    fn type_filter_error_recovery_category_prefix() {
        let f = TypeFilter::parse(Some("error_recovery"));
        assert!(f.matches(&ev(Category::ErrorRecovery, "error_recovery.triggered")));
        assert!(f.matches(&ev(Category::ErrorRecovery, "error_recovery.applied")));
        assert!(!f.matches(&ev(Category::App, "app.started")));
    }

    #[test]
    fn shell_category_round_trips_lowercase() {
        // GAP-15:Shell 分类经序列化 + as_str() 后均为小写。
        assert_eq!(serde_json::to_string(&Category::Shell).unwrap(), "\"shell\"");
        assert_eq!(Category::Shell.as_str(), "shell");
        let back: Category = serde_json::from_str("\"shell\"").unwrap();
        assert_eq!(back, Category::Shell);
    }
}
