//! SSH 子命令组 — profile/连接/隧道。Unix only。
//!
//! connect 带 `[user@]host` 走 daemon-direct（GAP-5.1 M3），`--index` 走
//! GUI-IPC；tunnel/tunnel-stop/tunnel-list 走 daemon SOCKS5（GAP-5.2）。

use clap::Subcommand;
use serde_json::{json, Value};

#[cfg(unix)]
#[derive(Subcommand)]
pub enum SshSub {
    /// 列出已配置的 SSH 会话档案
    ListProfiles,
    /// 按索引连接 SSH 会话档案
    Connect {
        /// `[user@]host` — 守护进程直连:开启一个持久的 puretermd 会话
        /// 并把当前终端接入其中(GAP-5.1 M3 完成标准)。省略此参数
        /// 而改传 `--index`,让运行中的 GUI 打开标签页。
        target: Option<String>,
        /// 供 GUI-IPC 路径使用的档案索引(在运行中的 GUI 里打开标签页)。
        #[arg(long)]
        index: Option<usize>,
        /// SSH 端口(仅守护进程直连路径)。
        #[arg(long)]
        port: Option<u16>,
        /// 身份文件 / 私钥路径(仅守护进程直连路径)。
        #[arg(long)]
        identity: Option<String>,
        /// GAP-5.4:把本地 ssh-agent 转发到远程主机
        /// (`-o ForwardAgent=yes`)。默认关闭(在不可信主机上有信任风险)。
        #[arg(short = 'A', long)]
        forward_agent: bool,
        /// GAP-40:本地端口转发(`ssh -L`),可重复(`8080:localhost:80`)。
        #[arg(short = 'L', long = "local-forward")]
        local_forwards: Vec<String>,
        /// GAP-40:远程端口转发(`ssh -R`),可重复。
        #[arg(short = 'R', long = "remote-forward")]
        remote_forwards: Vec<String>,
        /// GAP-40:不受信任的 X11 转发(`ssh -X`)。
        #[arg(short = 'X')]
        forward_x11: bool,
        /// GAP-40:受信任的 X11 转发(`ssh -Y`)。
        #[arg(short = 'Y')]
        forward_x11_trusted: bool,
    },
    /// 断开连接(关闭 SSH 标签页)。GAP-5.6:`--pane-id` 指定特定窗格;
    /// 省略则断开活动窗格。
    Disconnect {
        #[arg(long)]
        pane_id: Option<u64>,
    },
    /// GAP-5.6:查询每个 SSH 窗格的连接状态(`ssh.state` Socket API)。
    /// 按窗格显示已连接 / 重连中 / 失败(含失败类型)。
    State,
    /// GAP-5.2:通过网关开启基于 russh 的 SOCKS5 隧道(守护进程持有)。
    /// `curl --socks5-hostname 127.0.0.1:<local-port> https://...` 随后经
    /// 网关路由,可访问仅网关能到达的主机。
    Tunnel {
        /// `[user@]host` 网关。
        target: String,
        /// SSH 端口(默认 22)。
        #[arg(long)]
        port: Option<u16>,
        /// 身份文件 / 私钥路径(公钥认证)。
        #[arg(short = 'i', long)]
        identity: Option<String>,
        /// 本地 SOCKS5 监听端口(默认 1080)。
        #[arg(long)]
        local_port: Option<u16>,
        /// 密码(密码认证;与 `--identity` 同用时为私钥口令)。
        /// 若省略且未给 `--identity`,则在 stdin 上提示输入。
        #[arg(long)]
        password: Option<String>,
        /// 在本地解析 DNS 而非经网关(默认:由网关解析,
        /// 即 socks5h / `--socks5-hostname`)。
        #[arg(long)]
        no_remote_dns: bool,
        /// GAP-5.4:经隧道会话转发本地 ssh-agent,并
        /// 优先用 ssh-agent 密钥认证(无需 `--identity`)。默认关闭。
        #[arg(short = 'A', long)]
        forward_agent: bool,
    },
    /// GAP-5.2:按 id 停止隧道。
    TunnelStop {
        /// 来自 `ssh tunnel-list` 的隧道 id。
        id: String,
    },
    /// GAP-5.2:列出活动隧道。
    TunnelList,
}

#[cfg(unix)]
impl SshSub {
    pub(crate) fn method_and_params(&self) -> (&'static str, Option<Value>) {
        match self {
            SshSub::ListProfiles => ("ssh.list_profiles", None),
            SshSub::Connect { index: Some(i), .. } => {
                ("ssh.connect", Some(json!({ "index": i })))
            }
            SshSub::Connect { .. } => ("ssh.connect", None),
            SshSub::Disconnect { pane_id } => {
                ("ssh.disconnect", pane_id.map(|p| json!({ "pane_id": p })))
            }
            SshSub::State => ("ssh.state", None),
            // GAP-5.2:守护进程直连(在 `run` 中先于 GUI-IPC 回退处理)。
            // 这些分支在 IPC 路径上不可达,但让 match 保持穷尽。
            SshSub::Tunnel { .. } => ("tunnel.start", None),
            SshSub::TunnelStop { .. } => ("tunnel.stop", None),
            SshSub::TunnelList => ("tunnel.list", None),
        }
    }
}
