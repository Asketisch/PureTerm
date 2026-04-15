# 07 - 输入系统

## 功能概述

完整的键盘和鼠标输入处理系统：按键事件建模、终端编码（传统/Kitty/fixterms）、鼠标编码（5 种格式）、按键绑定、粘贴处理、键映射。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `input/key.zig` | ~1500 | 按键事件、Key 枚举（130+ 按键码） |
| `input/key_mods.zig` | ~800 | 修饰键位域、重映射 |
| `input/key_encode.zig` | ~2000+ | 键盘编码（核心，78KB） |
| `input/keycodes.zig` | ~500 | USB HID/平台/W3C 按键码映射 |
| `input/function_keys.zig` | ~600 | PC 风格功能键序列表 |
| `input/kitty.zig` | ~400 | Kitty 键盘协议映射表 |
| `input/mouse.zig` | ~200 | 鼠标事件类型 |
| `input/mouse_encode.zig` | ~600 | 鼠标编码（5 种格式） |
| `input/paste.zig` | ~200 | 粘贴编码（括号/非括号） |
| `input/Binding.zig` | ~5000+ | 按键绑定系统（165KB） |
| `input/command.zig` | ~500 | 命令面板定义 |
| `input/Link.zig` | ~100 | 链接匹配定义 |
| `input/config.zig` | ~20 | OptionAsAlt 配置 |
| `input/keyboard.zig` | ~100 | 键盘布局检测 |
| `input/KeymapDarwin.zig` | ~300 | macOS Carbon 键盘映射 |
| `input/KeymapNoop.zig` | ~50 | 非 macOS 空实现 |
| `input/helpgen_actions.zig` | ~100 | 帮助文档生成 |

## 核心数据结构

### 1. 按键事件 (`key.zig`)

```
KeyEvent:
  ├── action: Action      - press/release/repeat
  ├── key: Key            - 物理按键码
  ├── mods: Mods          - 修饰键状态
  ├── consumed_mods: Mods - 已消费的修饰键
  ├── composing: bool     - 死键组合中
  ├── utf8: []const u8   - 生成的文本
  └── unshifted_codepoint: u21 - 无 shift 时的码点

Key 枚举 (130+ 值，基于 W3C UI Events 标准):
  写入键: a-z, 0-9, 标点符号
  功能键: Shift/Ctrl/Alt/Meta (左右), Enter, Tab, Backspace, Escape
  方向键: Up/Down/Left/Right
  导航键: Home, End, Insert, Delete, PageUp, PageDown
  小键盘: Num0-9, +-*/, Enter, 小键盘方向/导航
  功能键: F1-F25
  媒体键: 音量, 播放/暂停, 等
```

### 2. 修饰键 (`key_mods.zig`)

```
Mods (packed struct, u16):
  ├── shift: bool
  ├── ctrl: bool
  ├── alt: bool
  ├── super: bool
  ├── caps_lock: bool
  ├── num_lock: bool
  └── sides: Side (u4) - 左右追踪

RemapSet:
  ├── map: HashMap(Mods, Mods) - 修饰键重映射
  └── mask: Mask - 快速预检查
```

### 3. 键盘编码 (`key_encode.zig`) ⭐ 核心

三层编码策略：

#### 层 1: Kitty 键盘协议
```
条件: kitty_flags 非空

编码格式: CSI <key> ; <mods> <event> <final>
  key: u21 码点或 Kitty 映射键码
  mods: 位域 (1=shift, 2=alt, 4=ctrl, 8=super, ...)
  event: u (press), :u (repeat), ;u (release)
  final: u (普通键), ~ (特殊键), A-D/F/H/P/Q/S (光标键)

示例: CSI 97 ; 5 u = Ctrl+A press
```

特殊处理：
- 仅 press 事件（除非 report_events）
- 死键状态只发送修饰键
- IME 确认发送文本
- 替代键报告（shifted/base 布局）
- 关联文本报告

#### 层 2: 传统 xterm + modifyOtherKeys
```
优先级:
1. PC 风格功能键表 (function_keys.zig)
   - 方向键: CSI A/B/C/D (normal), SS3 A/B/C/D (application)
   - Home/End: CSI H/F 或 SS3 H/F
   - F1-F4: SS3 P/Q/R/S (无修饰) 或 CSI 1X P/Q/R/S (有修饰)
   - F5-F12: CSI 1X ~ 格式
   - 小键盘: SS3 + 应用模式序列
   - 支持 15 种修饰键组合

2. 控制序列映射
   Ctrl+A→\x01, Ctrl+B→\x02, ..., Ctrl+Z→\x1A

3. Alt-ESC 前缀
   Alt+X → ESC X (当 alt_esc_prefix 启用时)

4. modifyOtherKeys mode 2
   CSI 27 ; <mods> ; <codepoint> ~

5. fixterms CSI-u
   CSI <codepoint> ; <mods+1> u
```

#### 层 3: 直接文本输出
```
无修饰键的可打印字符直接输出 UTF-8
macOS Command 键抑制文本
```

### 4. 鼠标编码 (`mouse_encode.zig`)

```
5 种编码格式:

X10 (传统):    ESC M Cb Cx Cy        (坐标 ≤ 223)
UTF-8:         ESC M Cb UtfX UtfY    (坐标用 UTF-8 编码)
SGR:           ESC [ < Cb ; Cx ; Cy M/m  (M=按下, m=释放)
URXVT:         ESC [ Cb ; Cx ; Cy M
SGR-Pixels:    ESC [ < Cb ; Px ; Py M/m  (像素坐标)

按钮编码:
  left=0, middle=1, right=2, release=3
  wheel_up=4(+32), wheel_down=5(+32)
  buttons 6-11 = +64-129
  shift=+4, alt=+8, ctrl=+16
  motion=+32
```

运动去重：非 SGR-Pixels 模式下，仅在单元格变化时发送事件。

### 5. 粘贴编码 (`paste.zig`)

```
Options:
  bracketed: bool    - 括号粘贴模式状态

编码规则:
  括号模式: ESC[200~ <data> ESC[201~
  非括号模式: \n → \r

安全过滤（替换为空格）:
  NUL, BS, ENQ, EOT, ESC, DEL
  VINTR, VQUIT, VKILL, VSUSP, VSTART, VSTOP, VWERASE, VLNEXT, VREPRINT, VDISCARD

isSafe(): 检查数据是否不含换行和粘贴终止序列
```

### 6. 按键绑定 (`Binding.zig`)

```
Binding:
  ├── trigger: Trigger     - 触发条件
  ├── action: Action       - 执行动作 (~60 种)
  └── flags: Flags         - 绑定标志

Trigger:
  ├── key: Trigger.Key
  │     ├── physical: Key     - 物理按键
  │     ├── unicode: u21      - Unicode 字符
  │     └── catch_all         - 通配符
  └── mods: Mods              - 修饰键

Action (~60 种变体):
  终端: copy/paste/reset/search/scroll
  字体: increase/decrease/reset font size
  窗口: new_window/tab/split, close, toggle_fullscreen
  配置: open_config, reload_config
  特殊: inspector, crash, toggle_quick_terminal

Set:
  ├── bindings: HashMap(Trigger, Value) - 正向映射
  ├── reverse: ReverseMap              - 反向映射 (Action → Trigger)
  └── 支持:
        ├── 多键序列 (ctrl+a > ctrl+b = action)
        ├── 动作链 (一个触发器多个动作)
        ├── catch-all 绑定
        ├── 全局绑定
        └── 向后兼容 (1.1.x 键名)
```

## 纯 Rust 实现要点

1. **Key 枚举**：用带关联值的 enum，对标 W3C 标准
2. **Mods 用 bitflags**：包括左右追踪
3. **键盘编码三层分发**：Kitty → 传统 → 直接
4. **鼠标编码用 match 分发**：5 种格式
5. **绑定系统用 HashMap**：trigger hash → action
6. **多键序列用嵌套 HashMap**：leader → nested set

### 关键 Rust 类型

```rust
bitflags::bitflags! {
    pub struct Mods: u16 {
        const SHIFT = 0x01;
        const CTRL = 0x02;
        const ALT = 0x04;
        const SUPER = 0x08;
        const CAPS_LOCK = 0x10;
        const NUM_LOCK = 0x20;
    }
}

pub struct KeyEvent {
    pub action: KeyAction,
    pub key: Key,
    pub mods: Mods,
    pub consumed_mods: Mods,
    pub composing: bool,
    pub utf8: String,
    pub unshifted_codepoint: Option<char>,
}

pub enum TriggerKey {
    Physical(Key),
    Unicode(char),
    CatchAll,
}

pub struct Binding {
    pub trigger: Trigger,
    pub action: BindingAction,
    pub flags: BindingFlags,
}
```
