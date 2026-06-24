//! GAP-2.1 hybrid workspace tier —— 「切换不丢 Surface 状态」的纯函数实现。
//!
//! PureTermApp 采用 flat-active + background-swap 混合模型:活跃 workspace 的
//! `tabs`/`active_tab` 作为 PureTermApp 的扁平字段(保持 disjoint borrow,避开
//! ~30 个 handler 的借出检查墙);其余 workspace 存于 `background: Vec<Workspace>`。
//! 切换 = `mem::swap` flat ↔ background(`Vec` 整体移动,Arc-bearing `Tab` 不 clone
//! → PTY/session 跟随 pane 存活)。不变量见 `workspace_e2e::hybrid_*`。
//!
//! 刻意做成自由函数(取若干 `&mut` 离散字段):既能在 `PureTermApp` 里一次性借用
//! 多个 disjoint 字段调用,又能 headless 单测(无需 GUI/PTY)。全量把 `tabs` 下放
//! 进 `Window` 的 cutover(detached 统一等)留作 Phase 4 单独进行。

use crate::iced_app::{Tab, TabId, Workspace, WorkspaceId};
use std::mem::swap;

/// 新建命名 workspace:把当前活跃(flat)的内容快照进 `background`,重置 flat 为
/// 新 workspace(调用方随后为新 workspace 创建首个 tab)。返回新 workspace 的 id。
///
/// `next_tab_id` 在混合模型里是全局计数器(保证 `TabId` 跨 workspace 唯一,避免
/// 切换后陈旧 `TabId` 串扰),因此快照里的 `Workspace::next_tab_id` 仅作占位、
/// 不再读回。
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_workspace(
    flat_tabs: &mut Vec<Tab>,
    flat_active_tab: &mut TabId,
    active_id: &mut WorkspaceId,
    active_name: &mut String,
    background: &mut Vec<Workspace>,
    next_workspace_id: &mut u64,
    new_name: &str,
) -> WorkspaceId {
    // 把当前活跃(flat)整体快照进 background —— Vec move,Arc 跟随存活。
    let snapshot = Workspace {
        id: *active_id,
        name: std::mem::replace(active_name, new_name.to_string()),
        tabs: std::mem::take(flat_tabs),
        active_tab: *flat_active_tab,
        next_tab_id: 0, // 占位:混合模型用全局计数器,不读回。
    };
    background.push(snapshot);

    // 新 workspace 身份。flat_tabs 已被 take 空,等调用方建首个 tab。
    let new_id = WorkspaceId(*next_workspace_id);
    *next_workspace_id += 1;
    *active_id = new_id;
    *flat_active_tab = TabId(0); // 占位:新 tab 建好后由调用方覆盖。
    new_id
}

/// 切换 active workspace(混合模型:swap flat ↔ background)。
///
/// - 目标 == 当前 active:返回 `true`(no-op)。
/// - 目标在 `background`:`mem::swap` 内容(tabs/active_tab)+ 身份(id/name),
///   旧活跃落入 background、目标升为活跃。`Vec` 移动而非 clone → Arc 身份存活。
/// - 目标未知:返回 `false` 且不动任何状态。
#[allow(clippy::too_many_arguments)]
pub(crate) fn switch_active_workspace(
    flat_tabs: &mut Vec<Tab>,
    flat_active_tab: &mut TabId,
    active_id: &mut WorkspaceId,
    active_name: &mut String,
    background: &mut Vec<Workspace>,
    target_id: WorkspaceId,
) -> bool {
    if *active_id == target_id {
        return true; // 已是 active,no-op。
    }
    let Some(idx) = background.iter().position(|w| w.id == target_id) else {
        return false; // 未知 workspace,不改任何状态。
    };
    // 内容 swap:Vec 整体移动,Arc-bearing Tab 不 clone → PTY/session 跟随存活。
    swap(flat_tabs, &mut background[idx].tabs);
    swap(flat_active_tab, &mut background[idx].active_tab);
    // 身份 swap:background[idx] 变成旧活跃(内容+id/name 一致)。
    swap(active_id, &mut background[idx].id);
    swap(active_name, &mut background[idx].name);
    true
}
