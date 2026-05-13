# pureterm-terminal/src/input — 输入处理

终端输入处理：按键编码、鼠标事件、修饰键、剪贴板粘贴。

## 文件结构

```
src/
├── mod.rs          # 模块重导出 (17 行)
├── key.rs          # 按键定义 (501 行)
├── key_encode.rs   # 按键编码 (601 行)
├── binding.rs      # 按键绑定 (721 行)
├── mods.rs         # 修饰键状态 (354 行)
├── mouse.rs        # 鼠标事件 (442 行)
└── paste.rs        # 粘贴处理 (228 行)
```

## 模块依赖

```
input/ → lib.rs (Terminal, TermMode)
input/ → screen/ (Cell)
```

## 核心类型

### Key（key.rs）

按键枚举：

```rust
pub enum Key {
    /// 字符按键
    Char(char),
    /// 控制字符
    Control(char),
    /// 修饰键 + 字符
    Alt(char),
    Ctrl(char),
    /// 功能键
    F(u8),           // F1-F12
    /// 方向键
    Arrow(ArrowKey),
    /// 其他特殊键
    Enter, Backspace, Tab, Escape,
    Home, End, PageUp, PageDown,
    Insert, Delete,
    /// 修饰键
    Shift, Control, Alt, Super,
}

pub enum ArrowKey {
    Up, Down, Left, Right,
}
```

### Modifiers（mods.rs）

修饰键状态：

```rust
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
}
```

**方法：**
- `from_state(state: &ModifiersState) -> Modifiers` — 从 winit 修饰键状态转换
- `contains(self, other: Modifiers) -> bool` — 检查是否包含指定修饰键

### MouseEvent（mouse.rs）

鼠标事件：

```rust
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub button: MouseButton,
    pub modifiers: Modifiers,
    pub x: usize,    // 单元格坐标
    pub y: usize,
    pub pixel_x: f32,  // 像素坐标
    pub pixel_y: f32,
}

pub enum MouseEventKind {
    Press,
    Release,
    Moved,
    Scrolled(i16),  // 滚动量（负=上，正=下）
    Drag,
}

pub enum MouseButton {
    Left,
    Middle,
    Right,
    ScrollUp,
    ScrollDown,
}
```

### KeyBinding（binding.rs）

按键绑定配置：

```rust
pub struct KeyBinding {
    pub key: Key,
    pub modifiers: Modifiers,
    pub action: KeyAction,
    pub description: &'static str,
}

pub enum KeyAction {
    Send(String),           // 发送原始字节
    SendEsc(String),       // 发送 ESC 前缀
    AppCursor,             // 应用光标模式
    NormalCursor,          // 普通光标模式
    ScrollUp,              // 滚动
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,
    ClearScrollback,
    Reset,
    Bell,
    Paste,
    Copy,
}
```

### KeyEncoder（key_encode.rs）

按键编码器，将 Key 转换为终端字节序列：

```rust
pub struct KeyEncoder {
    mode: TermMode,
}

impl KeyEncoder {
    /// 将按键编码为字节序列
    pub fn encode(&mut self, key: Key, modifiers: Modifiers) -> Vec<u8> {
        // 根据模式生成转义序列
    }
}
```

**编码规则：**

| 场景 | 输出 |
|------|------|
| Ctrl + 字母 A-Z | 0x01-0x1A |
| Alt + 字符 | ESC + char |
| 方向键（无修饰） | CSI A/B/C/D |
| Ctrl + 方向键 | CSI 1;5 A/B/C/D |
| Alt + 方向键 | CSI 1;3 A/B/C/D |
| F1-F4 | SS3 OP/Q/R/S |
| F5-F12 | CSI 15~ 到 24~ |
| App Cursor ON | SS3 OA/OB/OC/OD |

### ClipboardPaste（paste.rs）

剪贴板粘贴处理：

```rust
pub struct ClipboardPaste {
    bracketed_paste: bool,  // 是否启用括号粘贴模式
    trimmed: bool,           // 是否去除尾随换行
}
```

**bracketed paste 模式：**
- 启用时：发送 `\x1b[200~` 前缀和 `\x1b[201~` 后缀
- 禁用时：直接发送内容

## 输入流程

```
用户按键/鼠标
  → winit 事件
  → KeyEncoder::encode() / MouseEvent::from_winit()
  → Terminal::write() / Session::write()
  → VteParser 解析
  → Grid 更新
```

## 与 TerminalPane 的集成

TerminalPane 使用 `input` 模块进行按键编码：

```rust
// 在 TerminalPane 中
let bytes = key_encoder.encode(key, modifiers);
session.write(&bytes)?;
```

## 常见任务

### 添加新的按键处理

1. 在 `key.rs` 中添加新的 Key 变体
2. 在 `key_encode.rs` 的 `KeyEncoder::encode()` 中添加编码逻辑
3. 在 `binding.rs` 中添加默认绑定（可选）

### 修改鼠标事件编码

编辑 `mouse.rs` 的 `encode_mouse()` 函数。

### 添加修饰键组合

在 `mods.rs` 中添加新的修饰键标志，更新 `Modifiers::contains()` 逻辑。

## 父级导航

← [src/AGENTS.md](../AGENTS.md)