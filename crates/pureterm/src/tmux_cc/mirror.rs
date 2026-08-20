//! T-core-tmux-cc："镜像"——把 tmux 服务端事件（窗口列表变化、
//! 布局变化）翻译成对本地 `split_tree` 的变更。
//!
//! v1：纯数据结构。实际的 `split_tree` 变更由 GUI 的事件循环完成；
//! `MirrorEvent` 枚举就是传入的消息。

use std::collections::HashMap;

use crate::split_tree::SplitDirection;

/// tmux 控制模式镜像发出的事件。GUI 的事件循环消费这些事件
/// 并把它们应用到自身的 `split_tree`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirrorEvent {
    /// 服务端发送了一个 `%windows <w> <s>` 块。`windows` 是
    /// `window_id → window_index` 映射；GUI 用它替换当前的窗口列表。
    WindowList(HashMap<String, u32>),
    /// 出现了新窗口（`%window-add`）。
    WindowAdded { id: String, index: u32 },
    /// 窗口关闭（`%window-close`）。
    WindowClosed(String),
    /// 窗口被重命名（`%window-renamed`）。
    WindowRenamed { id: String, name: String },
    /// 活动窗口发生变化（`%window-active`）。
    WindowActive(String),
    /// 窗口布局发生变化（`%layout-change`）。`tree` 是一棵
    /// 左右树（Leaf / Split { dir, ratio, … }）；v1 把它保留为
    /// `String`（原始 tmux 布局）——GUI 通过 [`parse_layout`] 重新解析。
    LayoutChange { window_id: String, tree: String },
}

/// 解析 tmux 布局树（tmux 在 `list-windows -F '#{window_layout}'` 和
/// `%layout-change` 时打印的格式），得到 GUI 可渲染的树。v1 只支持
/// 常见情况（单叶、水平/垂直分屏、嵌套分屏）；完整的 `layouts` 枚举
/// （tiled / even-horizontal / main-vertical / …）留到 v2。
///
/// 格式是一个括号化树，例如：
/// ```text
/// 83x24,0,0,0
/// ```
/// 单个叶子时，或：
/// ```text
/// 168x24,0,0[83x24,0,0,0,84x24,0,83,0]
/// ```
/// 水平分屏时（两个子节点并排）。
pub fn parse_layout(s: &str) -> Layout {
    parse_layout_inner(s).unwrap_or(Layout::Leaf)
}

/// 递归解析器。布局畸形时返回 `None`。
///
/// tmux 的布局文法（来自 `man tmux`，已简化）：
///
/// ```text
/// layout ::= cellspec '[' children ']'
/// children ::= child { ',' child }   // 0..N 个子节点
/// child ::= cellspec   // 一个叶子，或
///           cellspec '[' children ']'  // 一个嵌套分屏
/// cellspec ::= NUM 'x' NUM ',' NUM ',' NUM [ ',' NUM 'x' NUM ]
/// ```
///
/// cell-spec *不在*方括号内——只有子节点列表在括号内。也就是说，
/// 主体里的顶层逗号既分隔子节点、又会出现在叶子的 cell-spec 里；
/// 我们按深度区分：子节点是一段连续区间（"深度 0 直到下一个括号闭合
/// 或下一个叶子开头的 `<NUM>x<NUM>,`"）。v1 采用更简单的捷径：
/// 数顶层逗号、除以 3（叶子 cell-spec 有 3 个顶层逗号），即得子节点
/// 数量。带括号的子树递归处理。
fn parse_layout_inner(s: &str) -> Option<Layout> {
    if !s.contains('[') {
        return Some(Layout::Leaf);
    }
    // 找到最外层的 `[…]`。
    let open = s.find('[')?;
    let close = matching_close(s, open)?;
    let body = &s[open + 1..close];

    // 父节点的 cell-spec 在 `s[..open]` 里，例如 "168x24,0,0"。
    // v1 不需要它。

    // 找到顶层子节点。子节点是括号平衡的连续区间。我们在深度 0 处
    // 紧跟闭合 `]` 的逗号上切开，或在紧跟叶子结尾（即后面是
    // `<NUM>x<NUM>,`）的逗号上切开。
    let children = split_layout_children(body);
    if children.len() < 2 {
        return Some(Layout::Leaf);
    }
    let parsed: Option<Vec<_>> = children.iter().map(|c| parse_layout_inner(c)).collect();
    Some(Layout::Split {
        dir: SplitDirection::Horizontal,
        ratio: 0.5,
        children: parsed?,
    })
}

/// 找到与 `open_idx` 处 `[` 匹配的 `]` 的索引。
/// 输入不平衡时返回 `None`。
fn matching_close(s: &str, open_idx: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    for i in open_idx..bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            },
            _ => {},
        }
    }
    None
}

/// 把布局主体切成子节点。每个子节点以 `<NUM>x<NUM>` cell-spec 开头。
/// 我们遍历字符串，每当在深度 0 看到后面（忽略空白）跟着 cell-spec
/// 模式的逗号就切出一个新子节点。
fn split_layout_children(body: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let bytes = body.as_bytes();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            b',' if depth == 0 => {
                // 跳过空白向前看；如果后面是 `<NUM>x<NUM>`，说明这个逗号
                // 是子节点边界；否则它属于 cell-spec 的一部分。
                if starts_cell_spec(&body[i + 1..]) {
                    out.push(&body[start..i]);
                    start = i + 1;
                }
            },
            _ => {},
        }
        i += 1;
    }
    out.push(&body[start..]);
    out
}

/// `s`（在开头逗号 + 可选空白之后）是否以 `83x24,` 这样的
/// cell-spec 开头？
fn starts_cell_spec(s: &str) -> bool {
    let s = s.trim_start();
    // 走数字。
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() || bytes[i] != b'x' {
        return false;
    }
    i += 1;
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i > digits_start && i < bytes.len() && bytes[i] == b','
}

/// 解析后的 tmux 布局树。
#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Leaf,
    Split {
        dir: SplitDirection,
        ratio: f32,
        children: Vec<Layout>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_leaf() {
        let l = parse_layout("83x24,0,0,0");
        assert_eq!(l, Layout::Leaf);
    }

    #[test]
    fn parse_horizontal_split() {
        let l = parse_layout("168x24,0,0[83x24,0,0,0,84x24,0,83,0]");
        assert_eq!(
            l,
            Layout::Split {
                dir: SplitDirection::Horizontal,
                ratio: 0.5,
                children: vec![Layout::Leaf, Layout::Leaf],
            }
        );
    }

    #[test]
    fn parse_nested_split() {
        // 两个子节点的外层水平分屏；右子节点是两个叶子的垂直分屏。
        let l = parse_layout(
            "168x48,0,0[83x48,0,0[42x48,0,0,0,40x48,0,42,0],84x48,0,83,0]",
        );
        match l {
            Layout::Split { dir, children, .. } => {
                assert_eq!(dir, SplitDirection::Horizontal);
                assert_eq!(children.len(), 2);
                assert!(matches!(children[0], Layout::Split { .. }));
                assert!(matches!(children[1], Layout::Leaf));
            },
            _ => panic!("expected split root"),
        }
    }

    #[test]
    fn mirror_event_window_list_round_trip() {
        let mut m = HashMap::new();
        m.insert("@0".to_string(), 0u32);
        m.insert("@1".to_string(), 1);
        let ev = MirrorEvent::WindowList(m.clone());
        assert_eq!(ev, MirrorEvent::WindowList(m));
    }
}
