# pureterm-session/src/ssh — SSH 子系统

SSH 会话完整实现：基于 `russh 0.60` 的 SSH client、SFTP v3 文件传输、连接复用（multiplexer）、X11 转发、Agent 转发、端口转发、已知主机验证。

## 文件结构

```
src/
├── mod.rs              # 模块重导出 (17 行)
├── auth.rs             # SshAuth 枚举 (13 行)
├── client.rs           # SshSession + SshClientHandler (655 行)
├── agent_forward.rs    # AgentForwardManager (53 行)
├── forward.rs         # PortForwardManager + ForwardType/ForwardTunnel (146 行)
├── known_hosts.rs      # KnownHosts TOFU 验证 (195 行)
├── multiplexer.rs     # SshMultiplexer 连接复用 (387 行)
├── sftp.rs            # SftpClient SFTP v3 客户端 (592 行)
└── x11.rs             # X11ForwardManager + X11State (125 行)
```

## 模块依赖

```
ssh/ → russh + russh-keys（外部）
ssh/ → base.rs（Session trait）
```

## 核心类型

### SshAuth（auth.rs）

SSH 认证方式枚举：

```rust
pub enum SshAuth {
    PromptPassword,                        // 交互式密码输入
    Password(String),                      // 已知密码
    PublicKey { key_path, passphrase },   // 公钥认证
    Agent,                                 // SSH Agent 认证 (TODO)
    None,                                  // 无认证
}
```

### SshSession（client.rs）

基于 `russh 0.60` 的 SSH 会话实现。

**SshClientHandler（client.rs）** — russh 事件处理器：

```rust
pub struct SshClientHandler {
    host: String,
    port: u16,
    known_hosts: KnownHosts,
}
```

实现 `client::Handler` trait，其中 `check_server_key()` 调用 `KnownHosts::verify_host_key()` 进行 TOFU 验证。

**连接流程（`start()`）：**
1. TCP 连接目标主机
2. 创建 `SshClientHandler` + `russh::client::Handle`
3. 认证（支持 None、Password、PublicKey；Agent 和 PromptPassword 为 TODO）
4. 打开 shell channel (`channel.open()`)
5. 分离读写：写入通过 `channel.data()`，读取启动 tokio 任务
6. 处理 ExtendedData（ext==1 为 stderr）

**关键方法：**
- `start()` — 建立连接并认证
- `resize(cols, rows)` — 调用 `window_change()`
- `write(data)` — 调用 `channel.data()`
- `kill(signal)` — 发送 EOF
- `start_remote_forward(host, port)` — 调用 `handle.tcpip_forward()` 请求远程端口转发

**已知问题：** `SshClientHandler::check_server_key()` 目前接受所有服务器密钥（TODO: known_hosts 交互式验证）

### SftpClient（sftp.rs）

SFTP v3 客户端，基于 russh SFTP 子系统。

**常量：**
- `SFTP_VERSION = 3`
- 包类型：INIT/OPEN/CLOSE/READ/WRITE/STAT/OPENDIR/READDIR/MKDIR/REMOVE/RENAME
- 状态码：OK/EOF/NO_SUCH_FILE/PERMISSION_DENIED

**关键方法：**
- `open_file(path, flags)` — 打开文件
- `read_file(handle, offset, len)` — 读取文件
- `write_file(path, data)` — 写入文件
- `list_dir(path)` — 列出目录
- `create_dir(path)` — 创建目录
- `remove_file(path)` — 删除文件

### SshMultiplexer（multiplexer.rs）

SSH 连接复用，多个会话共享同一底层 SSH 传输。

**类型：**

```rust
pub struct ConnectionKey {
    pub host: String,
    pub port: u16,
    pub user: String,
}

pub struct ConnectionStats {
    pub key: ConnectionKey,
    pub channel_count: usize,
    pub created_at: std::time::Instant,
    pub last_activity: std::time::Instant,
}

// 内部
type SharedHandle = Arc<Mutex<russh::client::Handle<SshClientHandler>>>;
```

**关键方法：**
- `get_or_connect(host, port, user, handle)` → 获取已有连接或注册新连接
- `has_connection(key)` / `get_handle(key)` → 查询现有连接
- `register_connection(key, handle)` / `release_connection(key)` → 管理 channel 计数
- `cleanup_idle()` → 清理超过 `max_idle_time`（默认 30 分钟）的空闲连接
- `total_connections()` → 当前连接数

**限制：** 最大 10 个连接，超限时自动淘汰最空闲连接。

### KnownHosts（known_hosts.rs）

SSH 已知主机 TOFU（Trust on First Use）验证。

**验证结果：**
- `Match` — 密钥匹配
- `Unknown` — 首次连接，接受并存储
- `Mismatch` — 密钥不匹配，安全警告（TODO: 交互式确认）

### PortForwardManager（forward.rs）

SSH 端口转发管理器。

**ForwardType：**

```rust
pub enum ForwardType {
    Local { local_addr, local_port, remote_host, remote_port },
    Remote { remote_addr, remote_port, local_host, local_port },
    Dynamic { local_addr, local_port },  // SOCKS5
}

pub enum TunnelState {
    Starting, Active, Stopped, Error(String),
}
```

**行为：**
- Local/Remote forwarding 使用 `TcpListener` 监听本地/远程端口
- Dynamic forwarding 在本地端口建立 SOCKS5 代理
- 数据通过 SSH channel 转发

### X11ForwardManager（x11.rs）

X11 图形转发管理器。

**类型：**

```rust
pub struct X11ForwardConfig {
    pub auth_protocol: String,  // 通常是 "MIT-MAGIC-COOKIE-1"
    pub auth_cookie: Vec<u8>,
    pub screen_number: u32,
    pub single_connection: bool,
}

pub enum X11State {
    Pending,   // 已请求，等待服务器确认
    Active,    // X11 转发活跃
    Rejected,  // 服务器拒绝
    Closed,    // 已关闭
}
```

### AgentForwardManager（agent_forward.rs）

SSH Agent 转发管理器，允许远程主机使用本地 SSH agent 进行认证。

```rust
pub struct AgentForwardManager {
    requested: bool,  // 是否已请求
    active: bool,      // 服务器是否确认
}
```

请求通过 `channel.request_agent_forward()` 发送。

## 版本兼容性注意

- `russh 0.60` 和 `russh-keys` 有严格的版本耦合 — 使用 workspace.dependencies 中的版本
- `russh::client::Handle<SshClientHandler>` 需通过 `Arc<Mutex<>>` 共享

## 父级导航

← [pureterm-session/AGENTS.md](../../AGENTS.md)
