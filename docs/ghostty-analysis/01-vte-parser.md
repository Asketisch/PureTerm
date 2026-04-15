# 01 - VTE 解析引擎

## 功能概述

VT 系列终端的转义序列解析器，是整个终端模拟器的基础。负责将原始字节流解析为结构化的终端动作。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `terminal/Parser.zig` | ~900 | 核心 VT 解析器状态机 |
| `terminal/parse_table.zig` | ~300 | comptime 生成的状态转移表 |
| `terminal/stream.zig` | ~600 | 通用流处理器 `Stream(Handler)` |
| `terminal/stream_terminal.zig` | ~1500 | 面向 Terminal 的流处理器 |
| `terminal/ansi.zig` | ~200 | ANSI/VT 常量定义 |
| `terminal/csi.zig` | ~100 | CSI 命令枚举 |
| `terminal/size_report.zig` | ~150 | 终端尺寸报告编码 |
| `terminal/UTF8Decoder.zig` | ~150 | DFA UTF-8 解码器 |
| `terminal/device_attributes.zig` | ~200 | DA1/DA2/DA3 请求响应 |
| `terminal/device_status.zig` | ~100 | DSR 请求类型 |
| `terminal/highlight.zig` | ~300 | 高亮区域管理 |
| `terminal/render.zig` | ~500 | 渲染状态快照 |

## 核心数据结构

### 1. 解析器状态机 (`Parser.zig`)

基于 vt100.net DEC ANSI 解析器状态机实现：

```
状态列表：
  ground        - 基础状态，处理可打印字符和控制字符
  escape        - 收到 ESC，等待后续字符
  escape_intermediate - ESC + 中间字符
  csi_entry     - CSI 入口 (ESC [)
  csi_param     - CSI 参数收集
  csi_intermediate - CSI 中间字符
  csi_ignore    - CSI 忽略
  dcs_entry     - DCS 入口 (ESC P)
  dcs_param     - DCS 参数收集
  dcs_intermediate - DCS 中间字符
  dcs_ignore    - DCS 忽略
  dcs_passthrough - DCS 数据透传
  osc_string    - OSC 数据收集
  sos_pm_apc_string - SOS/PM/APC 数据
```

动作列表：
- `print` - 打印字符到屏幕
- `execute` - 执行控制字符 (LF, CR, BS, TAB, BEL 等)
- `dispatch` - 分发完整序列给处理器
- `collect` - 收集中间字符
- `param` - 收集参数
- `clear` - 清除状态

### 2. 状态转移表 (`parse_table.zig`)

comptime 根据状态机定义生成的查找表：
- 输入: `(当前状态, 字节值)`
- 输出: `(下一状态, 动作)`
- 覆盖 0x00-0x7F 所有字节值

### 3. 流处理器 (`stream.zig`)

```zig
pub fn Stream(comptime Handler: type) type {
    return struct {
        parser: Parser,
        handler: Handler,
        // 内部状态
    };
}
```

发出的动作类型 (`StreamAction`)：
- `print` / `grapheme` - 文本输出
- `execute_c0` / `execute_c1` - 控制字符
- `dispatch_esc` - ESC 序列
- `dispatch_csi` - CSI 序列
- `dispatch_osc` - OSC 序列（8/52/133 等）
- `dispatch_dcs` - DCS 序列
- `dispatch_apc` - APC 序列（Kitty 图形）
- `dispatch_pm` - PM 序列

### 4. CSI 参数解析

支持的 CSI 参数格式：
- 数字参数: `CSI 1 ; 2 H` → 参数 `[1, 2]`
- 子参数: `CSI 1 : 2 : 3 m` → 嵌套参数
- 问号前缀: `CSI ? 25 h` → 私有模式
- 感叹号/大于号前缀: `CSI ! p`, `CSI > c`

### 5. UTF-8 解码器 (`UTF8Decoder.zig`)

基于 Bjoern Hoehrmann DFA 设计：
- 非分配式
- 错误序列替换为 U+FFFD
- 状态: `ACCEPT`, `REJECT`, 以及各字节长度的中间状态

## CSI 命令清单 (`csi.zig`)

| 命令 | 序列 | 功能 |
|------|------|------|
| ED | `CSI n J` | 擦除显示 (0=光标后, 1=光标前, 2=全部, 3=回滚) |
| EL | `CSI n K` | 擦除行 (0=光标后, 1=光标前, 2=整行) |
| TBC | `CSI n g` | 制表位清除 |
| XTWINOPS | `CSI 22/23 t` | 窗口操作 (图标化/最大化) |
| TitlePushPop | `CSI 22/23 t` | 标题压栈/弹栈 |

## 纯 Rust 实现要点

1. **状态机用 enum + match 实现**：Zig 的 comptime 状态表用 Rust 的 `match` 或查找表代替
2. **零拷贝设计**：解析器应只保存指针/偏移，不复制数据
3. **流式处理**：支持部分数据到达，维护中间状态
4. **关键 crate 选择**：
   - 可参考 `vte` crate 的解析器设计（但需要自己实现更多功能）
   - 推荐从头实现，精确对标 Ghostty 的状态机

### 实现步骤

1. 定义 `State` 枚举（14 个状态）
2. 定义 `Action` 枚举（10+ 个动作）
3. 生成状态转移表（可硬编码或用宏生成）
4. 实现 `Parser` 结构体（状态 + 参数缓冲区）
5. 实现 `parse(&mut self, bytes: &[u8])` 方法
6. 定义 `StreamAction` 枚举
7. 实现 `Stream<H>` 泛型流处理器
8. 实现针对 Terminal 的具体处理器
9. UTF-8 解码器（或使用 Rust 内置的 `std::str`)
10. 设备属性/状态响应编码
