# pureterm-session — 连接会话管理

提供统一的 `Session` trait 和四种实现：本地 PTY、SSH、串口、Telnet。包含中间件系统用于处理数据流。

## 关键依赖

内部依赖：`pureterm-core`

外部依赖：`tokio` + `async-trait` + `futures`（异步）、`russh`（SSH）、`serialport`（串口）、`portable-pty`（PTY）、`serde`、`thiserror`、`anyhow`、`bytes 1`。

## 文件结构

```
src/
├── lib.rs           # 模块导出
├── base.rs          # Session trait + ConnectParams + SessionType + SshAuth
├── local.rs         # LocalSession（PTY）
├── middleware.rs    # SessionMiddleware trait + MiddlewareStack
├── osc_processor.rs # OSC 处理中间件
├── ssh/
│   ├── mod.rs       # 重导出
│   ├── auth.rs      # SshAuth 枚举
│   └── client.rs    # SshSession（russh）
├── serial/
│   └── mod.rs       # SerialSession（serialport）
└── telnet/
    └── mod.rs       # TelnetSession（TCP）
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
- 额外方法：`child_id()` 获取子进程 ID
- 注意：`portable-pty 0.8` 使用 `take_writer()` 而非旧版的 `try_clone_writer()`

### SshSession（ssh/client.rs）

基于 `russh 0.60` 的 SSH 实现。

- `start()` — 连接 → 认证（支持 None、Password、PublicKey；Agent 和 PromptPassword 为 TODO）→ 打开 shell channel → 分离读写 → 启动 tokio 任务读取输出
- `resize()` — 调用 `window_change()`
- `write()` — 调用 `channel.data()`
- `kill()` — 发送 EOF 并断开连接
- **注意**：`SshClientHandler` 目前接受所有服务器密钥（TODO: known_hosts 验证）

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

### 版本兼容性注意
- `russh` 和 `russh-keys` 有严格的版本耦合 — 使用 workspace.dependencies 中的版本
- `portable-pty 0.8` API 变更：用 `take_writer()` 而非 `try_clone_writer()`
