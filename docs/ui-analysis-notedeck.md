# UI 组件分析：notedeck → PureTerm

## Context

用户喜欢 notedeck(本地路径 /Users/demon/Code/notedeck-master) 的美术风格，想知道该项目使用了什么 UI 组件，以及这些组件是否可用于 PureTerm。

---

## notedeck UI 组件总结

### 1. 框架依赖
- **egui 0.31.1** + **egui_extras** (表格、布局、图片加载)
- **egui_nav** - 导航抽屉系统
- **egui_tabs** - 标签栏组件
- **egui_virtual_list** - 虚拟化列表

### 2. 设计系统 (Design Tokens)

**颜色主题** (`crates/notedeck/src/theme.rs`):
- 语义化颜色系统：`surface_primary/secondary/elevated`, `text_primary/secondary/muted`
- 强调色：`accent`, `destructive`, `warning`, `success`
- 边框：`border_default`, `border_strong`
- 交互动画：`interactive_hover`, `interactive_pressed`

**圆角** (`crates/notedeck/src/tokens.rs`):
- `RADIUS_SM: 4.0` - 卡片、输入框
- `RADIUS_MD: 8.0` - 按钮、面板
- `RADIUS_LG: 12.0` - 模态框
- `RADIUS_PILL: 18.0` - 搜索框、标签

**间距**: `4/8/12/16/24/32` 像素梯度

**字体**:
- 主字体：Onest
- 等宽字体：Inconsolata
- Emoji：NotoEmoji (1.1x 缩放)
- CJK：NotoSansCJK

### 3. 自定义组件

| 组件 | 路径 | 功能 |
|------|------|------|
| `search_input_box` | `notedeck_ui/src/widgets.rs` | 胶囊形搜索框 |
| `styled_button_toggleable` | `notedeck_ui/src/widgets.rs` | 可切换状态按钮 |
| `x_button` | `notedeck_ui/src/widgets.rs` | 关闭按钮 |
| `AnimationHelper` | `notedeck_ui/src/anim.rs` | 动画辅助（脉冲、滚动数字） |
| `ProfilePic` | `notedeck_ui/src/profile/picture.rs` | 圆形头像（hover 展开） |
| `NoteView` | `notedeck_ui/src/note/mod.rs` | 笔记卡片（作者、内容、互动栏） |
| `context_menu` | `notedeck_ui/src/context_menu.rs` | 上下文菜单 |

### 4. 主题系统

- 支持 **Dark/Light/System** 三种模式
- 通过 `egui::ThemePreference` 切换
- 主题存储在 egui Context 中

---

## 是否可用于 PureTerm？

### ✅ 可以直接复用的部分

1. **设计系统 (tokens.rs, theme.rs)**
   - 颜色主题结构
   - 圆角、间距常量
   - 字体配置方案

2. **基础组件模式**
   - 胶囊形搜索框
   - 图标按钮
   - 上下文菜单

3. **主题切换机制**

### ⚠️ 需要适配的部分

1. **PureTerm 已有**
   - `pureterm-gui` 已有自己的渲染管线 (wgpu)
   - `pureterm-ui` 已有 egui 组件层

2. **notedeck 特定组件**
   - `ProfilePic` - 社交网络头像组件，PureTerm 不需要
   - `NoteView` - 笔记卡片，PureTerm 不需要
   - `NoteView` 有参考价值但需重写

3. **依赖版本**
   - notedeck 用 egui 0.31.1，需确认 PureTerm 版本兼容性

---

## 建议实施方式

如果要将 notedeck 风格应用到 PureTerm：

1. **移植设计系统**
   - 创建 `pureterm-ui/src/tokens.rs` 包含颜色、圆角、间距
   - 创建 `pureterm-ui/src/theme.rs` 支持 Dark/Light 主题

2. **迁移通用组件**
   - 搜索框、按钮、图标按钮可迁移
   - 动画系统可简化后迁移

3. **关键文件路径**
   ```
   notedeck 设计系统参考:
   - crates/notedeck/src/theme.rs
   - crates/notedeck/src/tokens.rs
   - crates/notedeck/src/fonts.rs

   PureTerm 目标位置:
   - crates/pureterm-ui/src/theme.rs (新建)
   - crates/pureterm-ui/src/tokens.rs (新建)
   ```

---

## 验证方式

1. 运行 `cargo build` 确认 PureTerm 编译通过
2. 启动应用目视检查主题切换
3. 对比 notedeck 截图与 PureTerm 界面
