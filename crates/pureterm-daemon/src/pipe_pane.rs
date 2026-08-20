//! GAP-6.6:pipe-pane——把会话的 PTY 输出接到一条 shell 命令(tmux
//! `pipe-pane`).
//!
//! 泵线程把 PTY 字节喂进 mpsc 通道;一个 tokio 任务把通道排空到
//! 拉起命令的 stdin。停止 = 丢弃 sender(任务
//! 结束;`kill_on_drop` 收殓子进程)。泵是普通 `std::thread`,
//! 从不触碰 runtime——只调 `UnboundedSender::send`(永不
//! 阻塞),因此缓慢/死掉的子进程不会拖停向订阅者投递 PTY。

use std::io;
use std::process::Stdio;

use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::session::SessionManager;

/// 开始把会话输出导入 `command`(经 `sh -c` 运行)。拉起子进程
/// + 一个把馈送通道排空进子进程 stdin 的 runtime 任务。
/// 返回泵向其写入 PTY 字节的 sender。任务持有
/// `Child`(带 `kill_on_drop`),因此丢弃 sender 即结束任务并
/// 收殓子进程。
pub fn start(rt: &tokio::runtime::Handle, command: &str) -> io::Result<UnboundedSender<Vec<u8>>> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdin = child.stdin.take();
    let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
    rt.spawn(drain(rx, stdin, child));
    Ok(tx)
}

async fn drain(
    mut rx: UnboundedReceiver<Vec<u8>>,
    mut stdin: Option<ChildStdin>,
    child: tokio::process::Child,
) {
    use tokio::io::AsyncWriteExt;
    // 为任务生命周期持有 `child`,使 `kill_on_drop` 只在
    // 任务结束时触发(sender 丢弃 → recv 返回 None → 循环退出)。命名
    // 为 `_child`,既不因未使用被标记,又保持其存活。
    let _child = child;
    while let Some(chunk) = rx.recv().await {
        if let Some(s) = stdin.as_mut() {
            if s.write_all(&chunk).await.is_err() {
                break;
            }
        }
    }
    drop(stdin);
}

/// GAP-6.6:设置会话的管道(tmux `pipe-pane [command]`)。`command` 为 None
/// ⇒ 清除(停止导入)。替换任何既有管道(丢弃其 sender 会杀掉
/// 旧子进程)。会话未知或拉起失败返回 Err。
pub fn set(
    manager: &SessionManager,
    rt: &tokio::runtime::Handle,
    sid: &str,
    command: Option<&str>,
) -> io::Result<()> {
    let handle = manager
        .session_handle(sid)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("unknown session {sid:?}")))?;
    // 先丢弃旧 sender(杀掉其子进程),再启动新的,
    // 使泵看到 None(不会部分喂给垂死的子进程)。
    let new_tx = {
        let h = handle.lock().unwrap();
        let _prev = h.pipe_feeder.lock().unwrap().take();
        match command {
            Some(cmd) => Some(start(rt, cmd)?),
            None => None,
        }
    };
    let h = handle.lock().unwrap();
    *h.pipe_feeder.lock().unwrap() = new_tx;
    Ok(())
}

/// GAP-6.6:停止会话导入(tmux 不带命令的 `pipe-pane`)。会话未知
/// 或本无管道时空操作。
pub fn clear(manager: &SessionManager, sid: &str) {
    if let Some(handle) = manager.session_handle(sid) {
        let h = handle.lock().unwrap();
        let _ = h.pipe_feeder.lock().unwrap().take();
    }
}

/// GAP-6.6:当前存在活动 pipe-pane 管道的会话 id。
pub fn list(manager: &SessionManager) -> Vec<String> {
    manager.pipe_status()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pipe-pane 的核心契约:推入馈送通道的字节到达
    /// 拉起命令的 stdin。`start` 是独立入口(无需
    /// `SessionManager`),因此这里直接演练 泵→排空→子进程 路径——
    /// 分派层 `set`/`clear` 测试未覆盖的那部分。
    #[tokio::test]
    async fn start_feeds_bytes_to_child_stdin() {
        let out =
            std::env::temp_dir().join(format!("pt_pipe_test_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let rt = tokio::runtime::Handle::current();
        // `dd bs=1` 对每个字节各做一次 write() 系统调用(无块
        // 缓冲),因此载荷一到即落盘——不依赖子进程退出或被收殓。
        // (`cat` 对普通文件会块缓冲,而排空任务的
        // `kill_on_drop` 在关闭 stdin 后立即收殓子进程,会与
        // `cat` 的 EOF 冲刷竞速——且竞输。)
        // 且竞输。)
        let cmd = format!("dd bs=1 of={} 2>/dev/null", out.display());
        let tx = start(&rt, &cmd).expect("start pipe");
        tx.send(b"payload-bytes".to_vec()).expect("send chunk");

        let mut got = String::new();
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(&out) {
                if s.contains("payload-bytes") {
                    got = s;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        drop(tx); // stop the pipe (drain task ends, child reaped)
        let _ = std::fs::remove_file(&out);
        assert!(got.contains("payload-bytes"), "child stdin never received bytes: {got:?}");
    }
}
