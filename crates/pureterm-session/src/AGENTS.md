# pureterm-session/src — 会话管理源码

会话管理 crate 的所有源文件：Session trait 定义、本地/SSH/串口/Telnet/Tmux 五种会话实现、中间件系统、Shell 集成、文件传输、跳板机、代理、日志等。

← [pureterm-session/AGENTS.md](../AGENTS.md)

## 文件结构

```
src/
├── lib.rs                # crate 入口，re-export 所有 Session 类型和中间件 (38 行)
├── base.rs               # Session trait + ConnectParams + SessionType + SshAuth (83 行)
├── clone_shell.rs        # Shell 克隆 — 复制会话环境 (78 行)
├── jump_host.rs          # 跳板机支持 — SSH bastion 多跳 (92 行)
├── local.rs              # LocalSession — 本地 PTY 会话 (178 行)
├── local_provider.rs     # LocalProfileProvider — 本地配置文件工厂 (73 行)
├── login_script.rs       # LoginScriptMiddleware — 登录脚本自动注入 (75 行)
├── middleware.rs          # SessionMiddleware trait + MiddlewareStack (51 行)
├── osc_processor.rs      # OscProcessorMiddleware — OSC 处理中间件 (46 行)
├── proxy.rs              # ProxyConfig — HTTP/SOCKS5 代理支持 (201 行)
├── serial_provider.rs    # SerialProfileProvider — 串口配置文件工厂 (74 行)
├── session_log.rs        # SessionLog — 会话日志记录与轮转 (359 行)
├── shell_integration.rs  # Shell 集成 — OSC 133 标记注入 (421 行)
├── ssh_provider.rs       # SshProfileProvider — SSH 配置文件工厂 (98 行)
├── sync_input.rs         # SyncInputManager — 多会话同步输入 (199 行)
├── telnet_provider.rs    # TelnetProfileProvider — Telnet 配置文件工厂 (105 行)
├── tmux_session.rs       # TmuxSession — Tmux 控制模式 Session (592 行)
├── xmodem.rs             # XMODEM 文件传输协议 (151 行)
├── zmodem.rs             # ZMODEM 文件传输中间件 (142 行)
├── ssh/                  → ssh/AGENTS.md — SSH 子系统
├── serial/               → serial/AGENTS.md — 串口会话
└── telnet/               → telnet/AGENTS.md — Telnet 会话
```

## 模块详情

### lib.rs — crate 入口

声明所有子模块，并通过 `pub use` 重导出：Session、ConnectParams、LocalSession、SshSession、SerialSession、TelnetSession、各 ProfileProvider、MiddlewareStack、OscProcessorMiddleware、SessionLog、ZmodemMiddleware、SyncInputManager、TmuxSession 等。

### base.rs — Session trait 与连接参数

| 类型 | 说明 |
|------|------|
| `Session` | 异步 trait：is_open、start、resize、write、kill、output_stream、session_type_name |
| `ConnectParams` | 连接参数：session_type、env、cwd |
| `SessionType` | Local{shell, args} / Ssh{host, port, user, auth, ...} / Serial{path, baud_rate, ...} / Telnet{host, port} / Tmux{session_name, attach, socket_path} |
| `SshAuth` | PromptPassword / Password / PublicKey / Agent / None |

所有实现通过 `broadcast::Sender<Vec<u8>>` 输出数据，调用方通过 `output_stream()` 获取接收端。

### clone_shell.rs — Shell 克隆

| 类型 | 说明 |
|------|------|
| `CloneInfo` | 克隆信息：session_type、cwd、env、connection_params |
| `ConnectionCloneParams` | 连接参数克隆：Ssh / Telnet / Serial |
| `CloneShellManager` | 管理会话克隆，读取现有会话环境变量 |

### jump_host.rs — 跳板机支持

| 类型 | 说明 |
|------|------|
| `JumpHostConfig` | 跳板机配置：host、port、user、hops（多跳链） |
| `JumpHop` | 单跳节点：host、port、user |
| `JumpHostManager` | 按目标主机名模式注册跳板机规则 |

支持多跳 SSH 隧道连接，按模式匹配目标主机。

### local.rs — 本地 PTY 会话

| 类型 | 说明 |
|------|------|
| `LocalSession` | 基于 `portable-pty` 的本地终端会话 |

- `start()` — 创建 PTY 对，读取 `$SHELL`（默认 `/bin/sh`），注入 Shell 集成，启动 reader 线程广播输出
- `resize()` — 调用 `master.resize(PtySize)`
- `write()` — 写入 PTY writer（使用 `take_writer()`）
- `child_id()` — 获取子进程 PID

### local_provider.rs — 本地配置文件工厂

`LocalProfileProvider` 实现 `ProfileProvider` trait：
- `create_tab_params()` — 从 Profile 创建 TabParams
- `quick_connect(query)` — 支持路径快速连接

### login_script.rs — 登录脚本中间件

`LoginScriptMiddleware` 实现 `SessionMiddleware`：
- 首次用户输入时自动发送预定义脚本（每个脚本后加 `\r\n`）
- 后续输入正常透传

### middleware.rs — 中间件系统

| 类型 | 说明 |
|------|------|
| `SessionMiddleware` | trait：process_output_to_terminal / process_output_to_session |
| `MiddlewareStack` | 链式中间件处理器，Vec<Box<dyn SessionMiddleware>> |

数据流向：Session -> 中间件栈 -> Terminal（输出方向），Terminal -> 中间件栈 -> Session（输入方向）。

### osc_processor.rs — OSC 处理中间件

`OscProcessorMiddleware` 当前为透传实现（OSC 由 VTE 解析器处理），预留 OSC 7 当前目录回调：
- `on_cwd(callback)` — 注册 CWD 变化回调

### proxy.rs — 代理支持（201 行）

| 类型 | 说明 |
|------|------|
| `ProxyConfig` | None / Http{addr, auth} / Socks5{addr, auth} |
| `ProxyAuth` | username + password |

- `from_url(url)` — 解析 `http://user:pass@host:port` 或 `socks5://...` 格式
- 支持 HTTP CONNECT 和 SOCKS5 两种代理协议

### serial_provider.rs — 串口配置文件工厂

`SerialProfileProvider`：从 Serial Profile 创建串口连接参数，标题格式 `{path} @ {baud_rate} baud`。

### session_log.rs — 会话日志记录（359 行）

| 类型 | 说明 |
|------|------|
| `SessionLogConfig` | 日志配置：enabled、log_dir、log_input、max_file_size |
| `SessionLog` | 日志写入器，带时间戳和文件轮转 |

- 默认目录 `~/.config/pureterm/logs/`
- 超过 `max_file_size` 时自动轮转（追加文件索引）
- `log_output(data)` / `log_input(data)` 分离记录
- 支持从配置启用/禁用

### shell_integration.rs — Shell 集成（421 行）

| 类型 | 说明 |
|------|------|
| `Shell` | Shell 类型：Bash、Zsh、Fish、Nushell、Elvish、Unknown |
| `ShellEnv` | 环境变量和集成文件集合 |
| `detect_from_env()` / `detect_from_path()` | 自动检测 Shell 类型 |

为每种 Shell 生成 OSC 133 标记注入脚本：
- **Zsh**：通过 `ZDOTDIR` 环境变量引入集成脚本
- **Bash**：通过 `PROMPT_COMMAND` 和 `DEBUG` trap 注入
- **Fish**：通过 Fish 函数和事件钩子注入
- **Nushell** / **Elvish**：预留接口

OSC 133 标记：A=PromptStart、B=CommandStart、C=OutputStart、D=OutputEnd。

### ssh_provider.rs — SSH 配置文件工厂

`SshProfileProvider`：从 Ssh Profile 创建 SSH 连接参数，标题格式 `{user}@{host}:{port}`。

### sync_input.rs — 同步输入管理（199 行）

| 类型 | 说明 |
|------|------|
| `SyncInputManager` | 管理同步输入会话注册表 |
| `SyncSessionHandle` | 同步会话句柄：id、name、writer 闭包 |
| `SyncInputResult` | 广播结果：total、succeeded、failed |

通过闭包机制与 Session trait 的 `&mut self` 解耦，支持向多个会话同时广播输入。

### telnet_provider.rs — Telnet 配置文件工厂

`TelnetProfileProvider`：从 Telnet Profile 创建连接参数，标题格式 `{host}:{port}`。

### tmux_session.rs — Tmux 控制模式 Session（592 行）

| 类型 | 说明 |
|------|------|
| `TmuxSession` | 基于 `tmux -C -u` 的控制模式会话 |
| `TmuxSessionConfig` | 配置：session_name、attach、socket_path、extra_flags |
| `TmuxEvent` | 事件：Output、LayoutChange、SessionChanged、WindowAdd/Close、PaneMode、Exit |

- `start()` — 启动 tmux 控制模式进程，启动 reader 线程解析 `%output` 等事件
- 支持 `%layout-change` 事件通知 UI 更新布局
- 通过 `send_command()` 发送 tmux 命令

### xmodem.rs — XMODEM 文件传输

| 类型 | 说明 |
|------|------|
| `FileTransferProtocol` | XModem / XModemCrc / YModem / YModemG |
| `TransferState` | Initial / Transferring / Complete / Cancelled / Error |

定义 XMODEM 协议常量（SOH、STX、EOT、ACK、NAK、CAN、CRC16）和状态机。

### zmodem.rs — ZMODEM 文件传输中间件

| 类型 | 说明 |
|------|------|
| `ZmodemMiddleware` | 实现 `SessionMiddleware`，检测 Zmodem 传输序列 |
| `ZmodemState` | Normal / TransferStart / Transferring / TransferEnd |

检测 `**\x18B0` 起始标记和 `**\x18B0OO` 结束标记，将二进制数据替换为可读占位符。

## 子模块链接

- [ssh/AGENTS.md](ssh/AGENTS.md) — SSH 子系统（russh 0.60、认证、端口转发、SFTP、X11）
- [serial/AGENTS.md](serial/AGENTS.md) — 串口会话实现
- [telnet/AGENTS.md](telnet/AGENTS.md) — Telnet 会话实现

## 常见任务

### 添加新的会话类型
1. 在 `src/` 添加新模块文件
2. 实现 `Session` trait（`#[async_trait]`）
3. 在 `lib.rs` 中添加模块导出和重导出
4. 在 `base.rs` 的 `SessionType` 中添加对应变体
5. 创建对应的 `ProfileProvider` 实现
6. 在 `pureterm-core` 的 `ProfileType` 和 `ProfileOptions` 中添加配置

### 添加中间件
1. 实现 `SessionMiddleware` trait（process_output_to_terminal / process_output_to_session）
2. 在 `MiddlewareStack` 中注册
3. 在 `TerminalPane` 初始化时配置中间件栈

### 修改 Shell 集成
编辑 `shell_integration.rs` 中对应 Shell 的环境变量和脚本生成逻辑。

### 配置跳板机连接
使用 `JumpHostManager::register(target_pattern, config)` 注册跳板机规则。

### 配置代理
使用 `ProxyConfig::from_url()` 解析代理 URL，传入 SSH 连接参数。
