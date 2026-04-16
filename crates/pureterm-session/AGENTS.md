# pureterm-session — 连接会话管理

提供统一的 `Session` trait 和四种实现：本地 PTY、SSH、串口、Telnet。包含中间件系统用于处理数据流，以及跳板机、日志、同步输入等功能。

## 关键依赖

内部依赖：`pureterm-core`

外部依赖：`tokio` + `async-trait` + `futures`（异步）、`russh`（SSH）、`serialport`（串口）、`portable-pty`（PTY）、`serde`、`thiserror`、`anyhow`、`bytes 1`、`regex`、`base64`、`dirs`、`rpassword`。

## 文件结构

```
src/
├── lib.rs              # 模块导出
├── base.rs             # Session trait + ConnectParams + SessionType + SshAuth
├── local.rs            # LocalSession（PTY）
├── local_provider.rs   # Local ProfileProvider
├── middleware.rs       # SessionMiddleware trait + MiddlewareStack
├── osc_processor.rs    # OSC 处理中间件
├── proxy.rs            # 代理支持
├── jump_host.rs        # SSH 跳板机（bastion）支持
├── session_log.rs      # 会话日志记录
├── sync_input.rs       # 同步输入管理
├── clone_shell.rs      # 克隆 shell 会话
├── login_script.rs     # 登录脚本中间件
├── shell_integration.rs # Shell 集成（OSC 133）
├── ssh/
│   ├── mod.rs          # 重导出
│   ├── auth.rs         # SshAuth 枚举
│   ├── client.rs       # SshSession（russh 0.60）+ 端口转发 + 代理
│   ├── agent_forward.rs # SSH Agent 转发
│   ├── forward.rs      # 端口转发管理（Local/Remote/Dynamic）
│   ├── known_hosts.rs  # 主机密钥验证（TOFU）
│   ├── multiplexer.rs  # 连接复用（存储 Arc<Mutex<Handle>>）
│   ├── sftp.rs         # SFTP v3 客户端
│   ├── x11.rs          # X11 转发
│   └── AGENTS.md       # SSH 子系统详解
├── ssh_provider.rs     # Ssh ProfileProvider
├── serial/
│   ├── mod.rs          # SerialSession（serialport）
│   └── AGENTS.md       # 串口会话详解
├── serial_provider.rs  # Serial ProfileProvider
├── telnet/
│   ├── mod.rs          # TelnetSession（TCP）
│   ├── iac.rs          # IAC 解析器
│   └── AGENTS.md       # Telnet 会话详解
├── telnet_provider.rs  # Telnet ProfileProvider
├── xmodem.rs           # Xmodem 文件传输
└── zmodem.rs            # Zmodem 文件传输
```

## 核心接口

### Session trait（base.rs）

```rust
#[async_trait]
trait Session: Send {
    fn is_open(&self) -> bool;
    async fn start(&mut self) -> Result<()>;
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;
    fn write(&mut self, data: &[u8]) -> Result<()>;
    fn kill(&mut self, signal: &str) -> Result<()>;
    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>>;
    fn session_type_name(&self) -> &'static str;
}
```

所有实现都通过 `broadcast::Sender<Vec<u8>>` 输出数据，调用方通过 `output_stream()` 获取接收端。

### 辅助类型（base.rs）

| 类型 | 说明 |
|------|------|
| `ConnectParams` | 连接参数：session_type、env、cwd |
| `SessionType` | Local{shell, args} / Ssh{host, port, user, auth, ...} / Serial{path, baud_rate, ...} / Telnet{host, port} |
| `SshAuth` | PromptPassword / Password / PublicKey / Agent / None |

## 实现详情

### LocalSession（local.rs）

基于 `portable-pty` 的本地 PTY 实现。

- `start()` — 创建 PTY，读取 `$SHELL`（默认 `/bin/sh`），启动 reader 线程广播输出
- `resize()` — 调用 `master.resize(PtySize)`
- `write()` — 写入 PTY writer
- 额外方法：`child_id()` 获取子进程 Id
- 注意：`portable-pty 0.8` 使用 `take_writer()` 而非旧版的 `try_clone_writer()`

### SshSession（ssh/client.rs）

详细文档：[ssh/AGENTS.md](src/ssh/AGENTS.md)

基于 `russh 0.60` 的 SSH 实现，包含：连接认证、SFTP、端口转发、X11/Agent 转发、连接复用。

### SerialSession（serial/mod.rs）

基于 `serialport` 的串口实现。

- `start()` — 打开串口，克隆端口用于 reader，启动 std 线程读取并广播
- `resize()` — 无操作（串口不支持）
- `write()` — 写入端口
- 静态方法：`list_ports() -> Vec<SerialPortInfo>`

### TelnetSession（telnet/mod.rs）

基于 `TcpStream` 的 Telnet 实现。

- `start()` — TCP 连接 → 分离读写 → 启动 tokio 任务读取
- `resize()` — 无操作（TODO: NAWS 协议）
- `write()` — 写入 socket

### 中间件系统（middleware.rs）

```rust
trait SessionMiddleware: Send {
    fn process_output_to_terminal(&mut self, data: &[u8]) -> Bytes;
    fn process_output_to_session(&mut self, data: &[u8]) -> Bytes;
}
```

`MiddlewareStack` 链式调用所有中间件。目前 `OscProcessorMiddleware` 为透传实现（OSC 由 VTE 解析器处理）。

### JumpHostManager（jump_host.rs）

SSH 跳板机（bastion/proxy host）支持：

| 类型 | 说明 |
|------|------|
| `JumpHostConfig` | 跳板机配置：host、port、user、hops（多跳） |
| `JumpHop` | 单跳配置 |
| `JumpHostManager` | 管理跳板机规则，按目标主机名模式匹配 |

支持多跳 SSH 隧道连接。

### SshMultiplexer（ssh/multiplexer.rs）

详细文档：[ssh/AGENTS.md](src/ssh/AGENTS.md)

SSH 连接复用，支持多会话共享同一底层 SSH 传输，自动清理空闲连接（默认 30 分钟）。

### SessionLog（session_log.rs）

会话日志记录功能：

| 类型 | 说明 |
|------|------|
| `SessionLogConfig` | 日志配置：enabled、log_dir、log_input、max_file_size |
| `SessionLog` | 日志写入器，带时间戳和文件轮转 |

支持输出记录、可选输入记录、文件大小限制和自动轮转。

### LoginScriptMiddleware（login_script.rs）

登录脚本中间件，在会话启动时自动发送预定义脚本。

### SyncInputManager（sync_input.rs）

同步输入管理，用于多终端同步输入：

| 类型 | 说明 |
|------|------|
| `SyncInputManager` | 管理同步输入会话 |
| `SyncInputResult` | 同步结果 |
| `SyncSessionHandle` | 同步会话句柄 |

### CloneShell（clone_shell.rs）

克隆现有 shell 会话，创建新的 PTY 链接到同一个 shell 进程。

### shell_integration.rs — Shell 集成

Shell 集成模块，注入 OSC 133 标记用于提示和命令跟踪：

| 类型 | 说明 |
|------|------|
| `Shell` | Shell 类型枚举：Bash、Zsh、Fish、Nushell、Elvish、Unknown |
| `ShellEnv` | 环境变量和文件集合 |
| `Shell::detect_from_env()` | 从 SHELL 环境变量检测 |
| `Shell::detect_from_path(path)` | 从路径检测（如 `/bin/bash`） |
| `ShellEnv::for_zsh/bash/fish()` | 生成对应 shell 的集成环境 |
| `apply_env(shell, env)` | 应用 shell 集成到会话环境 |

**OSC 133 标记：**
- `A` — Prompt Start（提示开始）
- `B` — Command Start（命令开始）
- `C` — Command Execute（命令执行）
- `D` — Command Output End（命令输出结束）

**集成方式：**
- Zsh：通过 `ZDOTDIR` 环境变量引入 `pureterm-integration.zsh` 脚本
- Bash：通过 `PROMPT_COMMAND` 和 `DEBUG` trap 注入
- Fish：通过 Fish 函数和事件钩子注入

### ZmodemMiddleware / Xmodem（zmodem.rs / xmodem.rs）

文件传输中间件，支持 Zmodem 和 Xmodem 协议。

### Profile Providers

提供从 Profile 配置创建会话的工厂：

| Provider | 说明 |
|----------|------|
| `LocalProfileProvider` | 从 Local 类型 Profile 创建 LocalSession |
| `SshProfileProvider` | 从 Ssh 类型 Profile 创建 SshSession |
| `SerialProfileProvider` | 从 Serial 类型 Profile 创建 SerialSession |
| `TelnetProfileProvider` | 从 Telnet 类型 Profile 创建 TelnetSession |

## 常见任务

### 添加新的会话类型
1. 在 `src/` 添加新文件（如 `docker.rs`）
2. 使用 `#[async_trait]` 实现 `Session` trait
3. 在 `lib.rs` 中添加模块导出和重导出
4. 在 `base.rs` 的 `SessionType` 中添加对应变体
5. 在 `pureterm-core` 的 `ProfileType` 和 `ProfileOptions` 中添加对应配置

### 修改 SSH 认证
编辑 `ssh/client.rs` 中的认证逻辑。当前 Agent 和 PromptPassword 认证为 TODO。

### 添加中间件
1. 实现 `SessionMiddleware` trait
2. 在 `MiddlewareStack` 中注册
3. 在 `TerminalPane` 初始化时配置中间件栈

### 配置跳板机连接
使用 `JumpHostManager::register()` 注册跳板机规则，匹配目标主机模式。

### 版本兼容性注意
- `russh` 和 `russh-keys` 有严格的版本耦合 — 使用 workspace.dependencies 中的版本
- `portable-pty 0.8` API 变更：用 `take_writer()` 而非 `try_clone_writer()`
