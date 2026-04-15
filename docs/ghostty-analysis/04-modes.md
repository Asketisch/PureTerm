# 04 - 终端模式管理

## 功能概述

管理终端的 DEC 私有模式和 ANSI 模式：光标样式、自动换行、鼠标报告、焦点事件、备屏幕、括号粘贴模式等。包括模式的设置/重置、保存/恢复、查询/报告。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `terminal/modes.zig` | ~1200 | 完整模式系统（packed struct + ModeState） |
| `terminal/charsets.zig` | ~200 | 字符集槽 (G0-G3) 和映射 |
| `terminal/cursor.zig` | ~100 | 光标样式枚举 |
| `terminal/focus.zig` | ~50 | 焦点事件编码 |
| `terminal/mouse.zig` | ~200 | 鼠标事件类型和格式 |

## 核心数据结构

### 1. 终端模式 (`modes.zig`)

使用 packed struct 存储所有模式标志位：

```
Modes (packed struct, ~100 个字段):
  ├── DEC 私有模式:
  │     ├── cursor_key_application (1)  - 应用光标键模式
  │     ├── auto_wrap (7)              - 自动换行
  │     ├── origin (6)                 - 原点模式
  │     ├── column_132 (3)             - 132 列模式
  │     ├── inverse (5)                - 反色屏幕
  │     ├── mouse_x10 (9)             - X10 鼠标报告
  │     ├── mouse_normal (1000)       - 正常鼠标报告
  │     ├── mouse_button (1002)       - 按钮事件鼠标
  │     ├── mouse_any (1003)          - 任意事件鼠标
  │     ├── mouse_sgr (1006)          - SGR 鼠标格式
  │     ├── focus_report (1004)       - 焦点报告
  │     ├── alt_screen (1047)         - 备屏幕
  │     ├── alt_screen_save (1048)    - 备屏幕 + 保存光标
  │     ├── alt_screen_full (1049)    - 完整备屏幕切换
  │     ├── bracketed_paste (2004)    - 括号粘贴
  │     ├──IRM (4)                    - 插入模式
  │     ├── DECNKM (66)               - 数字键盘模式
  │     └── ... 更多
  ├── ANSI 模式:
  │     ├── LNM (20)                  - 换行模式
  │     └── KAM (2)                   - 键盘动作模式
  └── 扩展模式:
        ├── synchronize_output (2026)
        ├── unicode (2027)
        ├── grapheme (2028)
        ├── modify_other_keys (5)
        └── kitty_keyboard (5)
```

**ModeState** - 模式的保存/恢复栈：
```
ModeState:
  ├── modes: Modes                    - 当前模式
  ├── saved: [N]Modes                 - 保存栈（主/备屏幕各一个）
  ├── get(mode_tag) bool
  ├── set(mode_tag, bool)
  ├── save()                          - 保存当前模式
  ├── restore()                       - 恢复保存的模式
  └── report(mode_tag) []const u8     - 生成 DECRPM 报告
```

**DECRPM 报告格式**：`CSI ? Pp ; Ps $ y`
- Pp = 模式号
- Ps = 0(未知), 1(设置), 2(重置), 3(永久设置), 4(永久重置)

### 2. 字符集 (`charsets.zig`)

```
字符集系统:
  ├── 4 个槽位: G0, G1, G2, GL
  ├── GL/GR 映射: 哪个槽位映射到左/右半区
  └── 支持的字符集:
        ├── UTF-8 (默认)
        ├── ASCII
        ├── British (BS → £)
        └── DEC Special Graphics (映射特殊符号)
```

每个字符集都有 comptime 生成的查找表，将输入字符映射到目标字符。

### 3. 光标样式 (`cursor.zig`)

```
CursorStyle (enum):
  ├── block              - 实心方块
  ├── block_hollow       - 空心方块
  ├── bar                - 竖线 (I-beam)
  └── underline          - 下划线

通过 DECSCUSR (CSI n q) 设置:
  1, 2 → block (闪烁/不闪烁)
  3, 4 → bar (闪烁/不闪烁)
  5, 6 → underline (闪烁/不闪烁)
```

### 4. 焦点事件 (`focus.zig`)

```
FocusEvent (enum):
  ├── focus_in           - 获得焦点 → ESC[I
  └── focus_out          - 失去焦点 → ESC[O

仅当 focus_report 模式 (1004) 启用时发送
```

### 5. 鼠标模式 (`mouse.zig`)

```
MouseEvent (enum):
  ├── none               - 不报告鼠标
  ├── x10                - 仅按下（X10 兼容）
  ├── normal             - 按下和释放（无移动）
  ├── button             - 按下、释放、拖动
  └── any                - 所有事件（包括移动）

MouseFormat (enum):
  ├── x10                - CSI M Cb Cx Cy (传统)
  ├── utf8               - CSI M Cb Utf8X Utf8Y
  ├── sgr                - CSI < Cb ; Cx ; Cy M/m
  ├── urxvt              - CSI Cb ; Cx ; Cy M
  └── sgr_pixels         - CSI < Cb ; Px ; Py M/m (像素坐标)
```

鼠标光标形状（W3C 标准）：
```
MouseShape: default, text, crosshair, pointer, move, not_allowed, context_menu, etc.
```

## 模式设置/查询序列

| 序列 | 功能 |
|------|------|
| `CSI ? Pm h` | 设置 DEC 私有模式 (SM) |
| `CSI ? Pm l` | 重置 DEC 私有模式 (RM) |
| `CSI Pm h` | 设置 ANSI 模式 |
| `CSI Pm l` | 重置 ANSI 模式 |
| `CSI ? Pm $ p` | 请求模式报告 (DECRQM) |
| `CSI ? Pp ; Ps $ y` | 模式报告响应 (DECRPM) |

## 纯 Rust 实现要点

1. **Modes 用 `bitflags!` 宏**：每个模式一个标志位
2. **模式标签用 enum**：每个可能的模式号对应一个枚举变体
3. **ModeState 用 `HashMap<ScreenType, Modes>` 保存栈**
4. **字符集映射用查找表**：`[u8; 128]` 或 `HashMap<u8, char>`
5. **光标样式简单枚举**
6. **鼠标模式和格式用枚举**

### 关键 Rust 类型

```rust
bitflags::bitflags! {
    pub struct ModeFlags: u128 {
        const CURSOR_KEY_APPLICATION = 1 << 0;
        const AUTO_WRAP = 1 << 1;
        const ORIGIN = 1 << 2;
        // ... ~100 个模式
    }
}

#[derive(Clone, Copy)]
pub enum CursorStyle {
    Block,
    BlockHollow,
    Bar,
    Underline,
}

#[derive(Clone, Copy)]
pub enum MouseEvent {
    None,
    X10,
    Normal,
    Button,
    Any,
}
```
