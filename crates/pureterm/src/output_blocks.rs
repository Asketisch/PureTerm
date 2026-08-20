//! GAP-51：基于块的输出数据模型 + 每 pane 存储。
//!
//! 当 `[block_output].enabled = true` 时，每个 OSC 133 D 标记会追加一个带网格
//! 切片输出文本的 [`OutputBlock`]。[`BlockSumTree`] 提供 O(log n) 的行查找；
//! Blocks 侧边栏用来列出、筛选与搜索块。
//!
//! `BlockFilter` 是视图侧过滤的唯一事实来源：调用方（侧边栏视图）构造一个
//! `BlockFilter { exit, query }` 并向存储索要过滤后的切片——视图层不再内联
//! 自己的过滤循环。

use crate::shell_state::FinishedCommand;

/// 每个 pane 保留的最大块数（保留最新）。
pub const BLOCK_STORE_MAX: usize = 256;

/// 块的来源。当前只有命令；为未来的 AI 标注预留（例如与命令并存的
/// AI 建议/解释）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockKind {
    #[default]
    Command,
    AiAnnotation,
}

/// 新完成块的旁路元数据——会话 / cwd / 开始时间。与 `FinishedCommand`
///（归 shell 状态所有）分开存放，使存储 API 可以在不动 shell 状态机的情况下演进。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockMeta {
    /// 顶层 PureTerm 会话 id（字符串形式，压栈时捕获）。
    pub session_id: Option<String>,
    /// 该 pane 最近一次已知的 OSC 7 cwd（缓存在 `pane_cwds` 中）。
    pub cwd: Option<String>,
    /// OSC 133 B（CommandStart）标记处的 UNIX 纪元微秒数——即“命令开始”
    /// 时间戳。B 标记从未到达时为 `None`。
    pub started_at_us: Option<u64>,
}

/// [`crate::surface_snapshot::capture_row_range_async`] 返回的已捕获块负载。
/// 携带 `PaneBlockStore::push` 所需的全部信息，使 iced 循环可以在单个
/// `Message` 处理器中完成追加，无需重新读取网格（`text` 已在主线程之外切片）。
///
/// `start_row` / `end_row` 在 OSC 133 D 标记时刻冻结，后续网格滚动不会使
/// 记录的区间与捕获的文本失同步。
#[derive(Clone, Debug)]
pub struct BlockCaptureRequest {
    pub pane_id: u64,
    pub cmd: crate::shell_state::FinishedCommand,
    pub start_row: Option<usize>,
    pub end_row: usize,
    pub meta: BlockMeta,
    pub text: Option<String>,
}

/// 一个命令/输出块边界的元数据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputBlock {
    pub id: u64,
    pub pane_id: u64,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_us: Option<u64>,
    /// 命令行所在网格行（OSC 133 B 标记）。
    pub command_row: usize,
    /// 输出开始的网格行（OSC 133 C 标记），存在时才有值。
    pub output_start_row: Option<usize>,
    /// 输出结束的网格行（OSC 133 D 标记）。
    pub output_end_row: usize,
    /// 输出区域的已捕获网格文本（网格切片）。
    pub output_text: Option<String>,
    // ── v1 新增（最小闭环）──
    /// 顶层 PureTerm 会话 id（例如 `puretermd` 会话）。
    pub session_id: Option<String>,
    /// 块完成时的工作目录（来自 OSC 7）。
    pub cwd: Option<String>,
    /// OSC 133 B（CommandStart）处的 UNIX 纪元微秒数。
    pub started_at_us: Option<u64>,
    /// 块来源。默认为 `Command`；为未来的 AI 条目预留。
    pub kind: BlockKind,
    /// 自由格式标签（AI 上下文、标签）。默认为空。
    pub tags: Vec<String>,
}

impl OutputBlock {
    pub fn row_range(&self) -> (usize, usize) {
        (self.command_row, self.output_end_row)
    }

    pub fn failed(&self) -> bool {
        self.exit_code.is_some_and(|c| c != 0)
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_us.map(|us| us / 1000)
    }

    /// 在 `command` + `output_text` 上做不区分大小写的子串搜索。
    /// 由 [`PaneBlockStore::filtered`] 用作“该块是否匹配查询”的唯一事实来源。
    pub fn has_text(&self, query_lower: &str) -> bool {
        if query_lower.is_empty() {
            return true;
        }
        self.command
            .as_deref()
            .is_some_and(|c| c.to_lowercase().contains(query_lower))
            || self
                .output_text
                .as_deref()
                .is_some_and(|t| t.to_lowercase().contains(query_lower))
    }
}

/// 块行区间的前缀和索引——按网格行 O(log n) 查找。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockSumTree {
    /// 每个块的累计结束行（单调递增）。
    end_rows: Vec<usize>,
}

impl BlockSumTree {
    /// 压入新块的 `output_end_row`。摊还 O(1)；树与 `PaneBlockStore.blocks`
    /// 保持单调对齐（溢出时从头部丢弃，并调用 [`BlockSumTree::drop_prefix`]
    /// 保持同步）。
    pub fn push(&mut self, output_end_row: usize) {
        self.end_rows.push(output_end_row);
    }

    /// 丢弃前 `n` 条（当 `blocks` 超过 `BLOCK_STORE_MAX` 时调用）。O(n)
    /// 但只在溢出时运行。
    pub fn drop_prefix(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let end = n.min(self.end_rows.len());
        self.end_rows.drain(0..end);
    }

    /// 查找 `[command_row, output_end_row]` 区间包含 `row` 的块索引
    ///（即拥有该网格行的块）。O(log n)。
    pub fn block_index_for_row(&self, row: usize) -> Option<usize> {
        if self.end_rows.is_empty() {
            return None;
        }
        let idx = self.end_rows.partition_point(|&end| end < row);
        self.end_rows.get(idx).map(|_| idx)
    }
}

/// 每 pane 的只追加块列表，带前缀和树行索引。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneBlockStore {
    blocks: Vec<OutputBlock>,
    index: BlockSumTree,
    next_id: u64,
}

/// [`PaneBlockStore::filtered`] 应用的组合过滤器。空 `query` 表示
/// “无文本过滤”；退出码过滤始终生效。
#[derive(Clone, Debug, Default)]
pub struct BlockFilter<'a> {
    pub exit: BlockExitFilter,
    pub query: Option<&'a str>,
}

impl PaneBlockStore {
    /// 把一条已完成的命令（含旁路 `meta` 与网格 `output_text`）追加为新块。
    /// 返回新存块的引用。
    ///
    /// `output_text` 在 GUI 线程之外捕获（见 `surface_snapshot::
    /// capture_row_range_async`）——当该 future 完成时，调用方把得到的
    /// `Option<String>` 传进来。`None` 只是保持 `output_text` 不设置
    ///（例如没有本地网格的 SSH pane）。
    pub fn push(
        &mut self,
        pane_id: u64,
        cmd: &FinishedCommand,
        output_end_row: usize,
        output_start_row: Option<usize>,
        output_text: Option<String>,
        meta: &BlockMeta,
    ) -> &OutputBlock {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let block = OutputBlock {
            id,
            pane_id,
            command: cmd.text.clone(),
            exit_code: cmd.exit_code,
            duration_us: cmd.duration_us,
            command_row: cmd.pane_row_at_b,
            output_start_row,
            output_end_row,
            output_text,
            session_id: meta.session_id.clone(),
            cwd: meta.cwd.clone(),
            started_at_us: meta.started_at_us,
            kind: BlockKind::Command,
            tags: Vec::new(),
        };
        self.blocks.push(block);
        self.index.push(output_end_row);
        // 从头部修剪（并让前缀和树保持同步）。
        if self.blocks.len() > BLOCK_STORE_MAX {
            let drop_n = self.blocks.len() - BLOCK_STORE_MAX;
            self.blocks.drain(0..drop_n);
            self.index.drop_prefix(drop_n);
        }
        self.blocks.last().expect("just pushed")
    }

    pub fn blocks(&self) -> &[OutputBlock] {
        &self.blocks
    }

    pub fn index(&self) -> &BlockSumTree {
        &self.index
    }

    pub fn last(&self) -> Option<&OutputBlock> {
        self.blocks.last()
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn block_containing_row(&self, row: usize) -> Option<&OutputBlock> {
        let idx = self.index.block_index_for_row(row)?;
        self.blocks.get(idx).filter(|b| b.command_row <= row)
    }

    /// 应用 `filter`（退出码 + 在 `command` + `output_text` 上的可选不区分大小写
    /// 子串）。按插入顺序（最老在前）返回匹配的块。“Blocks 侧边栏列表”以此
    /// 为唯一事实来源——视图层直接渲染其返回值，不做第二次过滤。
    pub fn filtered(&self, filter: &BlockFilter<'_>) -> Vec<&OutputBlock> {
        let q_lower;
        let q_trim: &str = match filter.query {
            Some(q) => {
                q_lower = q.trim().to_lowercase();
                q_lower.as_str()
            }
            None => "",
        };
        self.blocks
            .iter()
            .filter(|b| filter.exit.matches(b))
            .filter(|b| b.has_text(q_trim))
            .collect()
    }

    /// 供不需要独立查询的调用方使用：只需对 command/output_text 做“退出码 +
    /// 自由子串”。空 `query` 禁用文本过滤。
    #[allow(dead_code)]
    pub fn search_blocks(&self, query: &str, exit_filter: BlockExitFilter) -> Vec<&OutputBlock> {
        self.filtered(&BlockFilter {
            exit: exit_filter,
            query: Some(query),
        })
    }
}

/// Blocks 侧边栏的退出码过滤器。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockExitFilter {
    #[default]
    All,
    SuccessOnly,
    FailuresOnly,
}

impl BlockExitFilter {
    pub fn cycle(self) -> Self {
        match self {
            Self::All => Self::FailuresOnly,
            Self::FailuresOnly => Self::SuccessOnly,
            Self::SuccessOnly => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::SuccessOnly => "OK",
            Self::FailuresOnly => "Fail",
        }
    }

    pub fn matches(self, block: &OutputBlock) -> bool {
        match self {
            Self::All => true,
            Self::SuccessOnly => block.exit_code == Some(0),
            Self::FailuresOnly => block.failed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell_state::FinishedCommand;

    fn cmd(text: &str, exit: i32, row: usize) -> FinishedCommand {
        FinishedCommand {
            text: Some(text.into()),
            exit_code: Some(exit),
            duration_us: Some(5000),
            pane_row_at_b: row,
            started_at_us: Some(1_700_000_000_000_000),
        }
    }

    fn meta() -> BlockMeta {
        BlockMeta {
            session_id: Some("sess-1".into()),
            cwd: Some("/home/u".into()),
            started_at_us: Some(1_700_000_000_000_000),
        }
    }

    #[test]
    fn push_records_output_text_and_sum_tree() {
        let mut store = PaneBlockStore::default();
        let c = cmd("cargo test", 0, 5);
        store.push(1, &c, 20, Some(6), Some("ok\n".into()), &meta());
        assert_eq!(store.len(), 1);
        assert_eq!(store.blocks()[0].output_text.as_deref(), Some("ok\n"));
        assert_eq!(store.block_containing_row(10).unwrap().id, 0);
    }

    #[test]
    fn push_increments_sum_tree() {
        // 两个块：[0..=5] 与 [6..=20]。行 10 必须命中块 #1（id=1，command_row=6）。
        let mut store = PaneBlockStore::default();
        store.push(1, &cmd("first", 0, 0), 5, None, None, &meta());
        store.push(1, &cmd("second", 0, 6), 20, None, None, &meta());
        assert_eq!(store.len(), 2);
        let hit = store.block_containing_row(10).unwrap();
        assert_eq!(hit.id, 1);
        assert_eq!(hit.command_row, 6);
        assert_eq!(store.block_containing_row(3).unwrap().id, 0);
        assert_eq!(store.block_containing_row(25), None);
    }

    #[test]
    fn store_trims_to_max() {
        let mut store = PaneBlockStore::default();
        let c = FinishedCommand {
            text: None,
            exit_code: Some(0),
            duration_us: None,
            pane_row_at_b: 0,
            started_at_us: None,
        };
        for _ in 0..(BLOCK_STORE_MAX + 5) {
            store.push(1, &c, 1, None, None, &meta());
        }
        assert_eq!(store.len(), BLOCK_STORE_MAX);
    }

    #[test]
    fn push_trims_to_max_and_drops_oldest() {
        // 溢出后最老的块必须被移除，前缀和树也必须反映这一点——
        // `block_index_for_row` 不应返回过期的（越界）索引。
        let mut store = PaneBlockStore::default();
        let c = FinishedCommand {
            text: None,
            exit_code: Some(0),
            duration_us: None,
            pane_row_at_b: 0,
            started_at_us: None,
        };
        // 压入 BLOCK_STORE_MAX + 3 个块，output_end_row 与 command_row 单调递增，
        // 使最老的 3 个被淘汰。
        for i in 0..(BLOCK_STORE_MAX + 3) {
            let mut c = c.clone();
            c.pane_row_at_b = i;
            c.started_at_us = Some(i as u64);
            store.push(1, &c, i + 1, None, None, &meta());
        }
        assert_eq!(store.len(), BLOCK_STORE_MAX);
        // 第一个保留块的 id 必须是 3（0、1、2 已被丢弃）。
        assert_eq!(store.blocks()[0].id, 3);
        assert_eq!(store.blocks()[0].command_row, 3);
        // 第一个保留块之前的行不应被找到。
        assert!(store.block_containing_row(2).is_none());
        // 行 3 应命中第一个保留块。
        assert!(store.block_containing_row(3).is_some());
        // 最后一个块的 end_row 与压栈时一致。
        let last = store.last().unwrap();
        assert_eq!(last.output_end_row, BLOCK_STORE_MAX + 3);
    }

    #[test]
    fn push_records_new_fields() {
        let mut store = PaneBlockStore::default();
        let c = cmd("ls -l", 0, 1);
        store.push(7, &c, 10, Some(2), Some("foo\n".into()), &meta());
        let b = &store.blocks()[0];
        assert_eq!(b.session_id.as_deref(), Some("sess-1"));
        assert_eq!(b.cwd.as_deref(), Some("/home/u"));
        assert_eq!(b.started_at_us, Some(1_700_000_000_000_000));
        assert_eq!(b.kind, BlockKind::Command);
        assert!(b.tags.is_empty());
        assert_eq!(b.pane_id, 7);
    }

    #[test]
    fn filtered_applies_exit_filter_and_query() {
        let mut store = PaneBlockStore::default();
        let c1 = cmd("cargo build", 0, 0);
        let c2 = cmd("false", 1, 5);
        let c3 = cmd("echo needle", 0, 10);
        let c4 = cmd("echo hay", 1, 20);
        store.push(1, &c1, 4, None, Some("compiled\n".into()), &meta());
        store.push(1, &c2, 9, None, Some("error\n".into()), &meta());
        store.push(1, &c3, 19, None, Some("needle in haystack\n".into()), &meta());
        store.push(1, &c4, 25, None, Some("plain hay\n".into()), &meta());

        // 仅按退出码：FailuresOnly → 2（c2、c4）。
        let f = BlockFilter { exit: BlockExitFilter::FailuresOnly, query: None };
        assert_eq!(store.filtered(&f).len(), 2);

        // 仅按查询："needle" → 1（只有 c3——c2 是 "error" 而非 "needle"）。
        let f = BlockFilter { exit: BlockExitFilter::All, query: Some("needle") };
        assert_eq!(store.filtered(&f).len(), 1);

        // 组合：FailuresOnly + "hay" → 1（c4：output_text 含 haystack/hay 且退出码为 1）。
        //（c2 也是失败，但其 output_text 是 "error"，不含 "hay"。）
        let f = BlockFilter { exit: BlockExitFilter::FailuresOnly, query: Some("hay") };
        let hits = store.filtered(&f);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].command.as_deref(), Some("echo hay"));

        // 纯空白查询：不应用文本过滤（视为空）。
        let f = BlockFilter { exit: BlockExitFilter::All, query: Some("   ") };
        assert_eq!(store.filtered(&f).len(), 4);

        // 不区分大小写："NEEDLE" 能匹配 c3。
        let f = BlockFilter { exit: BlockExitFilter::All, query: Some("NEEDLE") };
        assert_eq!(store.filtered(&f).len(), 1);
    }

    #[test]
    fn search_matches_output_text() {
        // 为遗留调用方保留（侧边栏以前调用此函数）。
        let mut store = PaneBlockStore::default();
        store.push(
            1,
            &cmd("echo", 0, 0),
            3,
            None,
            Some("needle in haystack".into()),
            &meta(),
        );
        let hits = store.search_blocks("needle", BlockExitFilter::All);
        assert_eq!(hits.len(), 1);
    }
}