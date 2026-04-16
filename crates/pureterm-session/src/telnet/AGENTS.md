# pureterm-session/src/telnet — Telnet 会话实现

Telnet 协议会话实现，基于 TCP socket。

## 文件结构

```
src/
├── mod.rs        # TelnetSession 实现 (148 行)
└── iac.rs        # IAC (Interpret As Command) 解析器 (547 行)
```

## 模块依赖

```
telnet/ → base.rs（Session trait、ConnectParams）
telnet/ → lib.rs（broadcast channel）
```

无外部依赖（使用 `std::net::TcpStream`）。

## 核心类型

### TelnetSession（mod.rs）

Telnet 会话实现：

```rust
pub struct TelnetSession {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
    reader_handle: Option<JoinHandle<()>>,
    output_tx: broadcast::Sender<Vec<u8>>,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(host, port)` | 创建 Telnet 会话 |
| `start(&mut self) -> Result<()>` | 连接并启动读取任务 |
| `resize(&mut self, cols, rows) -> Result<()>` | NAWS 协商（TODO） |
| `write(&mut self, data: &[u8]) -> Result<()>` | 写入 socket |
| `kill(&mut self, signal: &str) -> Result<()>` | 发送 Ctrl+C |
| `is_open(&self) -> bool` | 检查连接状态 |
| `output_stream(&self) -> broadcast::Receiver<Vec<u8>>` | 获取输出流 |

**连接流程：**
1. `TcpStream::connect((host, port))`
2. 启动 tokio 任务读取数据
3. 读取的数据通过 `broadcast::Sender` 发送

### IacParser（iac.rs）

IAC（Interpret As Command）解析器，处理 Telnet 命令：

```rust
pub struct IacParser {
    state: IacState,
    subneg_buffer: Vec<u8>,
    current_cmd: Option<IacCommand>,
}
```

**IAC 状态机：**

```rust
enum IacState {
    Data,           // 普通数据
    Iac,            // 收到 IAC (0xFF)
    Command,        // 处理命令（DO/DONT/WILL/WONT/INF）
    Subnegotiate,   // 子协商中
    SubnegData,     // 子协商数据
}
```

### IacCommand（iac.rs）

Telnet IAC 命令：

```rust
pub enum IacCommand {
    /// WILL (0xFB) — 发送者请求启用选项
    Will(TelnetOption),
    /// DO (0xFD) — 发送者请求对端启用选项
    Do(TelnetOption),
    /// WONT (0xFC) — 发送者拒绝启用选项
    Wont(TelnetOption),
    /// DONT (0xFE) — 发送者要求对端禁用选项
    Dont(TelnetOption),
    /// SB (0xFA) — 子协商开始
    SubnegotiateStart(TelnetOption),
    /// SE (0xF0) — 子协商结束
    SubnegotiateEnd,
    /// IAC (0xFF) — 转义（数据中的 0xFF）
    Data(u8),
}
```

### TelnetOption（iac.rs）

Telnet 选项编号：

```rust
pub enum TelnetOption {
    BinaryTransmission = 0,     // 8-bit数据通道
    Echo = 1,                   // 回显
    SuppressGoAhead = 3,       // 抑制GA
    Status = 5,                 // 状态
    TimingMark = 6,              // 时间标记
    TermType = 24,              // 终端类型
    WindowSize = 31,            // 窗口大小
    TerminalSpeed = 32,          // 终端速度
    LineMode = 34,              // 行模式
    Environment = 36,           // 环境变量
}
```

## IAC 协议处理

### 命令格式

```
IAC WILL/DO/WONT/DONT <option>
IAC SB <option> <params> IAC SE
```

### 标准选项协商

| 选项 | 默认 | 说明 |
|------|------|------|
| Binary Transmission (0) | OFF | 8位数据通道 |
| Echo (1) | OFF | 回显 |
| Suppress Go Ahead (3) | ON | 抑制GA |
| Timing Mark (6) | OFF | 时间标记 |

### 子协商

**终端类型（TermType）：**
```rust
// 请求终端类型
IAC DO TERMINAL TYPE
// 发送终端类型
IAC SB TERMINAL TYPE SEND IAC SE
// 响应
IAC SB TERMINAL TYPE IS <type> IAC SE
```

**窗口大小（WindowSize）：**
```rust
IAC SB WINDOW SIZE <width> <height> IAC SE
```

## 与 Session trait 的集成

TelnetSession 实现 `Session` trait：

```rust
#[async_trait]
impl Session for TelnetSession {
    fn is_open(&self) -> bool { ... }
    async fn start(&mut self) -> Result<()> { ... }
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> { ... }
    fn write(&mut self, data: &[u8]) -> Result<()> { ... }
    fn kill(&mut self, signal: &str) -> Result<()> { ... }
    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>> { ... }
    fn session_type_name(&self) -> &'static str { "telnet" }
}
```

## 常见任务

### 添加新的 Telnet 选项

1. 在 `TelnetOption` 中添加选项编号
2. 在 `IacParser::handle_command()` 中添加处理逻辑
3. 在 `TelnetSession::start()` 中添加选项协商

### 实现 NAWS（窗口大小协商）

在 `resize()` 方法中发送：
```rust
IAC SB WINDOW SIZE <cols> <rows> IAC SE
```

### 调试 Telnet 连接

1. 检查 `IacParser` 状态机是否正确处理 IAC 转义
2. 验证 `subneg_buffer` 正确累积和解析
3. 检查 broadcast channel 是否正确初始化

## 父级导航

← [pureterm-session/AGENTS.md](../../AGENTS.md)