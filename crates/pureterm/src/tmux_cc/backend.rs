//! T-core-tmux-cc:持有 `tmux -CC` 子进程的 `Surface` 变体。
//!
//! v1:一个结构体,保存 GUI 生成与控制 control-mode tmux 子进程所需的
//! 元数据,外加 `SurfaceBackend` 枚举,让 [`crate::split_tree::Surface`]
//! 表达「这是一个 tmux -CC 面板」。

use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use parking_lot::Mutex;

use super::envelope::TmuxEnvelope;

/// 运行中的 control-mode tmux 子进程。v1 保存:
/// - tmux 分配给我们的 pane id(用于把 `%output` 路由回正确的
///   `alacritty_terminal::Term`);
/// - session id(`$N`),GUI 可将其显示在标签页标题中;
/// - 来自 `%windows … %end` 的最新窗口快照;
/// - GUI 在分离时置位的 kill 标志。
///
/// 实际的生成 / stdio 线程 / SIGHUP 清理在 [`super::pty`] 中(v1 仅支持 Unix)。
#[derive(Debug)]
pub struct TmuxControlPty {
    /// tmux 会话 id(`$0`, `$1`, …)。
    pub session_id: String,
    /// tmux 为 `-CC` 附加创建的初始 pane id。后续面板通过 `%window-add` / `%output` 返回。
    pub pane_id: u32,
    /// 最新窗口快照:`window_id → window_index`。
    pub windows: Mutex<Vec<(String, u32)>>,
    /// 第一个 `%begin … %end` 块中收到的回放载荷。
    /// 渲染器用它们为回滚缓冲区补充内容。
    pub replay: Mutex<Vec<u8>>,
    /// 进程句柄(Unix pid,用于分离时发送 SIGHUP)。
    #[cfg(unix)]
    pub pid: u32,
    /// GUI 请求的最新网格尺寸。每次窗口大小变化时作为 `resize-pane` 发送。
    pub last_size: Mutex<WindowSize>,
}

impl TmuxControlPty {
    /// 根据 tmux 附加期间打印的元数据构造新的 `TmuxControlPty`。
    /// `replay` 是第一个 `%output` 载荷(若有)。
    #[cfg(unix)]
    pub fn new(session_id: String, pane_id: u32, pid: u32) -> Self {
        Self {
            session_id,
            pane_id,
            windows: Mutex::new(Vec::new()),
            replay: Mutex::new(Vec::new()),
            pid,
            last_size: Mutex::new(WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 7,
                cell_height: 14,
            }),
        }
    }

    /// 非 Unix 构造函数(v1 的 Windows 回退 —— 目前 control-mode 后端仅支持 Unix,但保留该类型,`Surface` 无需在每处匹配点加 `#[cfg]` 即可携带它)。
    #[cfg(not(unix))]
    pub fn new(session_id: String, pane_id: u32) -> Self {
        Self {
            session_id,
            pane_id,
            windows: Mutex::new(Vec::new()),
            replay: Mutex::new(Vec::new()),
            last_size: Mutex::new(WindowSize {
                num_lines: 24,
                num_cols: 80,
                cell_width: 7,
                cell_height: 14,
            }),
        }
    }

    /// 将收到的 envelope 应用到本地状态。渲染器在 GUI 批量处理完 envelope 后查询该状态。
    pub fn apply_envelope(&self, env: &TmuxEnvelope) {
        match env {
            TmuxEnvelope::Output { pane_id, bytes } if *pane_id == self.pane_id => {
                self.replay.lock().extend_from_slice(bytes);
            },
            TmuxEnvelope::Begin { .. } => {
                // 新的回放块开始;清空缓冲区。
                // tmux 的 `%begin` 是**全局**帧边界(帧内可含多个 pane
                // 的 `%output`),不带 pane id —— 因此不能按 pane 过滤。
                self.replay.lock().clear();
            },
            TmuxEnvelope::End => {
                // 帧结束 —— GUI 可以一次性把 self.replay 刷入终端网格。
            },
            TmuxEnvelope::SessionChanged { session_id, .. } => {
                // 替换本地 session id(重新附加的目标已移动)。
                // `self.session_id` 是 `String`(而非 `Mutex<String>`),
                // 因为 GUI 只在附加时写入一次;任何竞争都是 v1 中已记录的限制。
                let mut cur = self.session_id.clone();
                cur = session_id.clone();
                // 无法写入 `self.session_id`(没有 `&mut self`),所以 v1 中这是空操作;
                // 会话变化时,GUI 应生成新的 `TmuxControlPty`。
                let _ = cur;
            },
            _ => {},
        }
    }
}

/// `Surface` 的后端类型。枚举用 `cfg` 门控,使旧的 `Local` / `Ssh` / `Serial` 变体不会把 Unix 专用代码带入 Windows 构建;新增的 `TmuxControl` 变体同样仅限 Unix,因为 v1 唯一的后端实现就是 Unix。
#[derive(Debug, Clone)]
pub enum SurfaceBackend {
    /// 本地 PTY。
    Local,
    /// SSH PTY(仅 Unix —— Windows 路径位于 `pureterm-ssh::windows`)。
    #[cfg(unix)]
    Ssh,
    /// 串口 / telnet(仅 Unix)。
    #[cfg(unix)]
    Serial,
    /// tmux control mode 子进程。
    #[cfg(unix)]
    TmuxControl(Arc<TmuxControlPty>),
}
