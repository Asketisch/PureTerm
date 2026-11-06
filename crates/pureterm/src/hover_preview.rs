//! GAP-4.2:智能选中悬停预览——防抖提示框 + 语义操作。
//!
//! 纯逻辑层:智能匹配检测、URL 标题抓取、路径元数据、
//! 打开/拷贝/编辑器操作以及叠加状态。`iced_app` 负责鼠标事件、
//! 异步任务与固定提示框 / 上下文菜单组件。

use std::path::{Path, PathBuf};
use std::time::Duration;

use alacritty_terminal::index::Point;
use alacritty_terminal::smart_selection::SmartMatch;
use alacritty_terminal::term::Term;
use regex::Regex;

pub const DEBOUNCE_MS: u64 = 200;

/// 相对路径 / 编译器的 `path:line[:col]` 片段(GAP-36)。
fn semantic_path_pattern() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:~/?|[\w][\w./_\-]*/)[\w./_\-]+:\d+(?::\d+)?|\b[\w][\w./_\-]*:\d+(?::\d+)?\b")
            .expect("semantic path regex")
    })
}

/// 需要显示悬停预览 + 语义上下文菜单的智能选中规则。
/// 网页部分(HTML 解析 / favicon / 抓取)——见 [`html`]。
pub mod html;

pub use html::{favicon_url_for, fetch_url_preview, parse_html_title};

pub fn is_previewable_rule(rule_id: &str) -> bool {
    matches!(rule_id, "url" | "ssh-url" | "file-path")
}

/// 在网格点击处评估智能选中,只保留可预览的规则。
pub fn smart_match_at_point<T>(term: &Term<T>, point: Point) -> Option<SmartMatch> {
    if !term.smart_selection.is_enabled() {
        return None;
    }
    let (text, byte_offset, _, _) = term.smart_selection_line_text(point)?;
    let m = term.smart_selection.evaluate(&text, byte_offset)?;
    is_previewable_rule(&m.rule_id).then_some(m)
}

/// GAP-36:网格点处的语义文件位置(智能选中 + path:line 正则)。
pub fn semantic_target_at_point<T>(
    term: &Term<T>,
    pane_id: u64,
    point: Point,
) -> Option<HoverTarget> {
    if let Some(m) = smart_match_at_point(term, point) {
        if m.rule_id == "file-path" {
            return Some(HoverTarget::from_match(pane_id, point, &m));
        }
    }
    let (text, byte_offset, _, _) = term.smart_selection_line_text(point)?;
    let span = semantic_span_at_offset(&text, byte_offset)?;
    Some(HoverTarget {
        pane_id,
        line: point.line.0,
        column: point.column.0,
        rule_id: "semantic-path".to_string(),
        text: span,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HoverTarget {
    pub pane_id: u64,
    pub line: i32,
    pub column: usize,
    pub rule_id: String,
    pub text: String,
}

impl HoverTarget {
    pub fn from_match(pane_id: u64, point: Point, m: &SmartMatch) -> Self {
        Self {
            pane_id,
            line: point.line.0,
            column: point.column.0,
            rule_id: m.rule_id.clone(),
            text: m.text.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewContent {
    Loading,
    Url {
        title: Option<String>,
        favicon_url: Option<String>,
    },
    Path {
        kind: PathKind,
        size_label: Option<String>,
        modified: Option<String>,
        gitignored: bool,
    },
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    File,
    Directory,
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverPreviewMenuItem {
    Open,
    Copy,
    OpenInEditor,
}

impl HoverPreviewMenuItem {
    pub const ALL: [Self; 3] = [Self::Open, Self::Copy, Self::OpenInEditor];

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "打开",
            Self::Copy => "复制",
            Self::OpenInEditor => "在编辑器中打开",
        }
    }

    pub fn applies_to(self, rule_id: &str) -> bool {
        match self {
            Self::OpenInEditor => rule_id == "file-path" || rule_id == "semantic-path",
            _ => true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HoverPreviewMenuState {
    pub position: (f32, f32),
    pub target: HoverTarget,
    pub hovered: Option<HoverPreviewMenuItem>,
}

/// 提示框 + 防抖状态(不依赖 iced,可测试)。
#[derive(Debug)]
pub struct HoverPreviewState {
    pub target: Option<HoverTarget>,
    pub content: PreviewContent,
    /// 提示框定位用的窗口内容坐标。
    pub position: (f32, f32),
    debounce_gen: u64,
    fetch_gen: u64,
    pending_target: Option<HoverTarget>,
    pub context_menu: Option<HoverPreviewMenuState>,
}

impl Default for HoverPreviewState {
    fn default() -> Self {
        Self::new()
    }
}

impl HoverPreviewState {
    pub fn new() -> Self {
        Self {
            target: None,
            content: PreviewContent::Loading,
            position: (0.0, 0.0),
            debounce_gen: 0,
            fetch_gen: 0,
            pending_target: None,
            context_menu: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.target.is_some() && self.context_menu.is_none()
    }

    pub fn dismiss(&mut self) {
        self.target = None;
        self.pending_target = None;
        self.content = PreviewContent::Loading;
        self.context_menu = None;
        self.debounce_gen += 1;
        self.fetch_gen += 1;
    }

    pub fn dismiss_tooltip(&mut self) {
        self.target = None;
        self.pending_target = None;
        self.content = PreviewContent::Loading;
        self.debounce_gen += 1;
        self.fetch_gen += 1;
    }

    pub fn show_context_menu(&mut self, target: HoverTarget, position: (f32, f32)) {
        self.context_menu = Some(HoverPreviewMenuState {
            position,
            target: target.clone(),
            hovered: None,
        });
        self.target = Some(target);
        self.position = position;
        self.debounce_gen += 1;
        self.fetch_gen += 1;
    }

    /// 鼠标移过可预览目标(或离开)。需要启动新的防抖计时器时,
    /// 返回对应的防抖代数。
    pub fn on_mouse_move(
        &mut self,
        target: Option<HoverTarget>,
        position: (f32, f32),
    ) -> Option<u64> {
        self.position = position;
        let Some(target) = target else {
            if self.context_menu.is_none() {
                self.dismiss_tooltip();
            }
            return None;
        };
        if self.context_menu.is_some() {
            return None;
        }
        if self.target.as_ref() == Some(&target) {
            return None;
        }
        if self.pending_target.as_ref() == Some(&target) {
            return None;
        }
        self.pending_target = Some(target);
        self.debounce_gen += 1;
        Some(self.debounce_gen)
    }

    pub fn on_debounce(&mut self, gen: u64) -> Option<(u64, HoverTarget)> {
        if gen != self.debounce_gen {
            return None;
        }
        let target = self.pending_target.take()?;
        self.target = Some(target.clone());
        self.content = PreviewContent::Loading;
        self.fetch_gen += 1;
        Some((self.fetch_gen, target))
    }

    pub fn apply_preview(&mut self, fetch_gen: u64, target: &HoverTarget, content: PreviewContent) {
        if fetch_gen != self.fetch_gen {
            return;
        }
        if self.target.as_ref() != Some(target) {
            return;
        }
        self.content = content;
    }

    pub fn active_target(&self) -> Option<&HoverTarget> {
        self.context_menu
            .as_ref()
            .map(|m| &m.target)
            .or(self.target.as_ref())
    }
}

pub fn debounce_duration() -> Duration {
    Duration::from_millis(DEBOUNCE_MS)
}

/// 拆分编译器 / ripgrep 输出中常见的 `path:line[:col]` 后缀。
pub fn parse_path_line(text: &str) -> (String, Option<u64>, Option<u64>) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return (String::new(), None, None);
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with("ssh://") {
        return (trimmed.to_string(), None, None);
    }
    // `src/main.rs:42:5` —— 两者均为数字时去掉末尾的 `:col`。
    if let Some((path, rest)) = trimmed.rsplit_once(':') {
        if let Some((path2, col_str)) = path.rsplit_once(':') {
            if !path2.is_empty()
                && rest.chars().all(|c| c.is_ascii_digit())
                && col_str.chars().all(|c| c.is_ascii_digit())
            {
                if let (Ok(line), Ok(_col)) = (col_str.parse::<u64>(), rest.parse::<u64>()) {
                    if line > 0 {
                        return (path2.to_string(), Some(line), Some(rest.parse().unwrap_or(1)));
                    }
                }
            }
        }
        if !path.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(line) = rest.parse::<u64>() {
                if line > 0 {
                    return (path.to_string(), Some(line), None);
                }
            }
        }
    }
    (trimmed.to_string(), None, None)
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

pub fn resolve_path(text: &str, cwd: Option<&Path>) -> PathBuf {
    let (path_part, _, _) = parse_path_line(text);
    let expanded = expand_tilde(&path_part);
    if expanded.is_absolute() {
        expanded
    } else if let Some(cwd) = cwd {
        cwd.join(expanded)
    } else {
        expanded
    }
}

/// 在 `line` 的 `byte_offset` 处查找语义文件位置。
pub fn semantic_span_at_offset(line: &str, byte_offset: usize) -> Option<String> {
    for m in semantic_path_pattern().find_iter(line) {
        if byte_offset >= m.start() && byte_offset < m.end() {
            return Some(m.as_str().to_string());
        }
    }
    None
}

/// 解析编辑器可执行文件:`$EDITOR` > 配置 > `"code"`。
pub fn resolve_editor(config_editor: &str) -> String {
    std::env::var("EDITOR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if config_editor.trim().is_empty() {
                "code".to_string()
            } else {
                config_editor.to_string()
            }
        })
}

pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn path_is_gitignored(path: &Path) -> bool {
    let mut dir = path.parent().unwrap_or(path).to_path_buf();
    loop {
        let gi_path = dir.join(".gitignore");
        if gi_path.is_file() {
            let (gi, err) = ignore::gitignore::Gitignore::new(&gi_path);
            if err.is_none() && gi.matched(path, path.is_dir()).is_ignore() {
                return true;
            }
        }
        if dir.join(".git").exists() {
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    false
}

pub fn path_preview(path: &Path) -> PreviewContent {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let kind = if meta.is_dir() {
                PathKind::Directory
            } else {
                PathKind::File
            };
            let size_label = if meta.is_file() {
                Some(format_file_size(meta.len()))
            } else {
                Some("directory".into())
            };
            let modified = meta.modified().ok().and_then(|t| {
                use chrono::{DateTime, Local};
                DateTime::<Local>::from(t).format("%Y-%m-%d %H:%M").to_string().into()
            });
            PreviewContent::Path {
                kind,
                size_label,
                modified,
                gitignored: path_is_gitignored(path),
            }
        }
        Err(_) => PreviewContent::Path {
            kind: PathKind::Missing,
            size_label: None,
            modified: None,
            gitignored: false,
        },
    }
}

pub async fn fetch_preview(target: &HoverTarget, cwd: Option<&Path>) -> PreviewContent {
    match target.rule_id.as_str() {
        "url" | "ssh-url" => fetch_url_preview(&target.text).await,
        "file-path" => {
            let path = resolve_path(&target.text, cwd);
            path_preview(&path)
        }
        _ => PreviewContent::Error("unsupported target".into()),
    }
}

pub fn copy_target(text: &str) {
    let _ = std::panic::catch_unwind(|| {
        arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text))
    });
}

pub fn open_target(target: &HoverTarget, cwd: Option<&Path>) {
    match target.rule_id.as_str() {
        "url" | "ssh-url" => open_url(&target.text),
        "file-path" => {
            let path = resolve_path(&target.text, cwd);
            open_path(&path);
        }
        _ => {}
    }
}

pub fn open_in_editor_target(
    target: &HoverTarget,
    cwd: Option<&Path>,
    config_editor: &str,
) {
    if target.rule_id != "file-path" && target.rule_id != "semantic-path" {
        return;
    }
    let (path_part, line, _) = parse_path_line(&target.text);
    let path = resolve_path(&path_part, cwd);
    let line = line.unwrap_or(1);
    open_in_editor(&path, line, config_editor);
}

/// 悬停 / 语义文件目标对应的已解析文件系统路径(GAP-35 LSP didOpen)。
pub fn resolved_path_for_target(target: &HoverTarget, cwd: Option<&Path>) -> Option<PathBuf> {
    if target.rule_id != "file-path" && target.rule_id != "semantic-path" {
        return None;
    }
    let (path_part, _, _) = parse_path_line(&target.text);
    Some(resolve_path(&path_part, cwd))
}

/// Cmd-点击 / 语义历史条目:在配置的编辑器中按行号打开文件。
pub fn open_semantic_text(text: &str, cwd: Option<&Path>, config_editor: &str) {
    let (path_part, line, _) = parse_path_line(text);
    let path = resolve_path(&path_part, cwd);
    let line = line.unwrap_or(1);
    open_in_editor(&path, line, config_editor);
}

fn open_url(url: &str) {
    if cfg!(target_os = "macos") {
        if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
            log::warn!("hover_preview: could not open url '{url}': {e}");
        }
    } else if cfg!(target_os = "linux") {
        if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
            log::warn!("hover_preview: could not open url '{url}': {e}");
        }
    }
}

fn open_path(path: &Path) {
    if cfg!(target_os = "macos") {
        if let Err(e) = std::process::Command::new("open").arg(path).spawn() {
            log::warn!("hover_preview: could not open path '{}': {e}", path.display());
        }
    } else if cfg!(target_os = "linux") {
        if let Err(e) = std::process::Command::new("xdg-open").arg(path).spawn() {
            log::warn!("hover_preview: could not open path '{}': {e}", path.display());
        }
    }
}

fn open_in_editor(path: &Path, line: u64, config_editor: &str) {
    let editor = resolve_editor(config_editor);
    let base = editor.split_whitespace().next().unwrap_or(&editor);
    let base_name = std::path::Path::new(base)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(base);

    let mut cmd = if base_name == "code" || base_name == "cursor" {
        let mut c = std::process::Command::new(base);
        c.arg("-g").arg(format!("{}:{}", path.display(), line));
        c
    } else if base_name == "emacs" || base_name == "emacsclient" {
        let mut c = std::process::Command::new(base);
        c.arg(format!("+{line}")).arg(path);
        c
    } else {
        // vim / nvim / $EDITOR 采用 +行号 约定
        let mut c = std::process::Command::new(base);
        c.arg(format!("+{line}")).arg(path);
        c
    };

    if let Err(e) = cmd.spawn() {
        log::warn!("hover_preview: could not launch editor '{editor}': {e}");
    }
}

/// 从窗格局部鼠标位置构建悬停目标(测试用的纯辅助函数)。
pub fn hover_target_from_grid(pane_id: u64, point: Point, m: SmartMatch) -> HoverTarget {
    HoverTarget::from_match(pane_id, point, &m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::smart_selection::SmartSelectionService;

    #[test]
    fn previewable_rules() {
        assert!(is_previewable_rule("url"));
        assert!(is_previewable_rule("file-path"));
        assert!(!is_previewable_rule("email"));
    }

    #[test]
    fn parse_path_line_suffix() {
        assert_eq!(
            parse_path_line("src/main.rs:42"),
            ("src/main.rs".into(), Some(42), None)
        );
        assert_eq!(
            parse_path_line("src/main.rs:42:5"),
            ("src/main.rs".into(), Some(42), Some(5))
        );
        assert_eq!(
            parse_path_line("https://example.com/a:1"),
            ("https://example.com/a:1".into(), None, None)
        );
    }

    #[test]
    fn semantic_span_at_offset_finds_rustc_path() {
        let line = "error[E0308]: src/main.rs:42:5 expected bool, found i32";
        let off = line.find("src/main").unwrap() + 3;
        assert_eq!(
            semantic_span_at_offset(line, off).as_deref(),
            Some("src/main.rs:42:5")
        );
    }

    #[test]
    fn resolve_editor_prefers_env() {
        std::env::set_var("EDITOR", "vim");
        assert_eq!(resolve_editor("code"), "vim");
        std::env::remove_var("EDITOR");
        assert_eq!(resolve_editor("emacs"), "emacs");
        assert_eq!(resolve_editor(""), "code");
    }

    #[test]
    fn parse_html_title_extracts() {
        let html = "<html><head><title>Hello &amp; World</title></head></html>";
        assert_eq!(parse_html_title(html).as_deref(), Some("Hello &amp; World"));
    }

    #[test]
    fn favicon_from_link_or_default() {
        let html = r#"<link rel="icon" href="/f.ico">"#;
        assert_eq!(
            favicon_url_for("https://ex.com/x", html).as_deref(),
            Some("https://ex.com/f.ico")
        );
        assert_eq!(
            favicon_url_for("https://ex.com/x", "<html></html>").as_deref(),
            Some("https://ex.com/favicon.ico")
        );
    }

    /// 回归：`<link rel="icon">` 之后 240 字节内出现多字节字符时，
    /// 字节切片会切在字符边界之外直接 panic —— 而这里的 HTML 来自
    /// **远程页面**，一个中文标题就足以触发。
    #[test]
    fn favicon_with_multibyte_chars_in_window_does_not_panic() {
        let prefix = r#"<link rel="icon" href="/f.ico">"#;
        // 切片右端点固定在 `rel_pos + 240`（字节）。让一个 3 字节的
        // 汉字**正好跨过**这个下标，切片就会切在字符内部 → panic。
        let rel_pos = prefix.find("rel=").expect("rel=");
        let cut = rel_pos + 240;
        let filler = "a".repeat(cut - prefix.len() - 1);
        let html = format!("{prefix}{filler}构完成");
        assert!(
            !html.is_char_boundary(cut),
            "测试构造失效：cut 落在字符边界上，触发不了 panic"
        );
        // 不 panic 即可；favicon 仍应被正确解析出来。
        assert_eq!(
            favicon_url_for("https://ex.com/x", &html).as_deref(),
            Some("https://ex.com/f.ico")
        );
    }

    /// 回归：emoji（4 字节）落在窗口边界上同样不能 panic。
    #[test]
    fn favicon_with_emoji_in_window_does_not_panic() {
        let prefix = r#"<link rel="icon" href="/f.ico">"#;
        let rel_pos = prefix.find("rel=").expect("rel=");
        let cut = rel_pos + 240;
        let filler = "a".repeat(cut - prefix.len() - 2);
        let html = format!("{prefix}{filler}✅");
        assert!(!html.is_char_boundary(cut), "测试构造失效");
        assert_eq!(
            favicon_url_for("https://ex.com/x", &html).as_deref(),
            Some("https://ex.com/f.ico")
        );
    }

    #[test]
    fn format_sizes() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(2048), "2.0 KB");
    }

    #[test]
    fn debounce_state_machine() {
        let mut st = HoverPreviewState::new();
        let t = HoverTarget {
            pane_id: 1,
            line: 0,
            column: 0,
            rule_id: "url".into(),
            text: "https://example.com".into(),
        };
        let gen = st.on_mouse_move(Some(t.clone()), (10.0, 20.0)).expect("debounce");
        assert!(st.on_debounce(gen + 1).is_none());
        let (fetch_gen, got) = st.on_debounce(gen).expect("ready");
        assert_eq!(got, t);
        st.apply_preview(
            fetch_gen,
            &got,
            PreviewContent::Url {
                title: Some("Ex".into()),
                favicon_url: None,
            },
        );
        assert!(matches!(st.content, PreviewContent::Url { .. }));
    }

    #[test]
    fn smart_selection_url_rule_matches() {
        let svc = SmartSelectionService::with_defaults();
        let line = "see https://example.com/path for details";
        let click = line.find("example").unwrap();
        let m = svc.evaluate(line, click).unwrap();
        assert_eq!(m.rule_id, "url");
        assert!(is_previewable_rule(&m.rule_id));
    }

    #[test]
    fn path_preview_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, b"hi").unwrap();
        match path_preview(&file) {
            PreviewContent::Path {
                kind,
                size_label,
                gitignored,
                ..
            } => {
                assert!(matches!(kind, PathKind::File));
                assert_eq!(size_label.as_deref(), Some("2 B"));
                assert!(!gitignored);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn menu_items_respect_rule() {
        assert!(HoverPreviewMenuItem::Open.applies_to("url"));
        assert!(!HoverPreviewMenuItem::OpenInEditor.applies_to("url"));
        assert!(HoverPreviewMenuItem::OpenInEditor.applies_to("file-path"));
        assert!(HoverPreviewMenuItem::OpenInEditor.applies_to("semantic-path"));
    }
}
