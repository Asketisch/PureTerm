# pureterm-session/src/serial — 串口会话实现

SerialPort 会话实现，基于 serialport crate。

## 文件结构

```
src/
└── mod.rs        # SerialSession 实现 (约 300 行)
```

## 模块依赖

```
serial/ → base.rs（Session trait、ConnectParams）
serial/ → lib.rs（broadcast channel）
```

## 核心类型

### SerialSession（mod.rs）

串口会话实现：

```rust
pub struct SerialSession {
    port_path: String,
    baud_rate: u32,
    stream: Option<Box<SerialPort>>,
    reader_handle: Option<JoinHandle<()>>,
    output_tx: broadcast::Sender<Vec<u8>>,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(port_path, baud_rate)` | 创建串口会话 |
| `start(&mut self) -> Result<()>` | 打开串口并启动读取任务 |
| `resize(&mut self, cols, rows) -> Result<()>` | 无操作（串口不支持 resize） |
| `write(&mut self, data: &[u8]) -> Result<()>` | 写入串口 |
| `kill(&mut self, signal: &str) -> Result<()>` | 无操作 |
| `is_open(&self) -> bool` | 检查连接状态 |
| `output_stream(&self) -> broadcast::Receiver<Vec<u8>>` | 获取输出流 |

**连接流程：**
1. `SerialPort::new(port_path)` 打开串口
2. 配置波特率 `set_baud_rate(baud_rate)?`
3. 克隆端口用于读取：`port.try_clone()`
4. 启动 std 线程读取数据
5. 读取的数据通过 `broadcast::Sender` 发送

### SerialPortInfo

串口信息（用于列表）：

```rust
pub struct SerialPortInfo {
    pub name: String,        // 端口名称（如 /dev/ttyUSB0）
    pub port_type: String,   // 端口类型
    pub baud_rates: Vec<u32>, // 支持的波特率
}
```

### list_ports() — 列出可用串口

```rust
pub fn list_ports() -> Vec<SerialPortInfo> {
    // 使用 serialport::available_ports() 获取可用端口
}
```

**返回示例：**
```rust
[
    SerialPortInfo {
        name: "/dev/ttyUSB0",
        port_type: "USB",
        baud_rates: [9600, 19200, 38400, 57600, 115200],
    },
    SerialPortInfo {
        name: "/dev/ttyS0",
        port_type: "RS-232",
        baud_rates: [9600, 19200, 38400, 115200],
    },
]
```

## 串口配置选项

| 参数 | 说明 | 默认值 |
|------|------|-------|
| `baud_rate` | 波特率 | 9600 |
| `data_bits` | 数据位 (5/6/7/8) | 8 |
| `stop_bits` | 停止位 (1/2) | 1 |
| `parity` | 校验位 (None/Odd/Even) | None |
| `flow_control` | 流控制 (None/Hardware/Software) | None |

## 与 Session trait 的集成

SerialSession 实现 `Session` trait：

```rust
#[async_trait]
impl Session for SerialSession {
    fn is_open(&self) -> bool { ... }
    async fn start(&mut self) -> Result<()> { ... }
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        Ok(()) // 串口不支持 resize
    }
    fn write(&mut self, data: &[u8]) -> Result<()> { ... }
    fn kill(&mut self, signal: &str) -> Result<()> {
        Ok(()) // 串口不支持信号
    }
    fn output_stream(&self) -> broadcast::Receiver<Vec<u8>> { ... }
    fn session_type_name(&self) -> &'static str { "serial" }
}
```

## 常见任务

### 添加新的串口配置

编辑 `SerialSession::new()` 和 `start()` 方法，添加更多配置选项。

### 实现流控制

在 `start()` 中添加：
```rust
port.set_flow_control(serial::FlowControl::Hardware)?;
```

### 调试串口连接

1. 检查 `SerialPort::new()` 是否成功
2. 验证波特率设置：`port.baud_rate()`
3. 检查 reader 线程是否正常启动
4. 验证 broadcast channel 发送

## 父级导航

← [pureterm-session/AGENTS.md](../../AGENTS.md)