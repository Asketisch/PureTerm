# pureterm-ui/src/split — 分割面板布局管理

分割面板的容器结构管理和像素布局计算。支持四方向拆分（左右上下）、递归嵌套容器、拖拽调整分割比例。

## 文件结构

```
src/
├── mod.rs        # 模块重导出 (5 行)
├── container.rs  # SplitContainer + SplitChild + Direction + SplitSide (292 行)
└── layout.rs     # 布局计算函数 (311 行)
```

## 模块依赖

```
split/ → terminal/pane.rs（TerminalPane）
```

## 核心类型

### Direction（container.rs）

```rust
pub enum Direction {
    Horizontal, // 左右分割
    Vertical,   // 上下分割
}
```

### SplitSide（container.rs）

四方向拆分枚举：

```rust
pub enum SplitSide {
    Right,  // 在目标右侧插入（Vertical，insert after）
    Left,   // 在目标左侧插入（Vertical，insert before）
    Down,   // 在目标下方插入（Horizontal，insert after）
    Up,     // 在目标上方插入（Horizontal，insert before）
}
```

**方法：**
- `direction()` → SplitSide 对应的 Direction
- `insert_before()` → Left/Up 返回 true（在其之前插入）

### SplitChild（container.rs）

```rust
pub enum SplitChild {
    Container(Box<SplitContainer>), // 嵌套容器
    Pane(u64),                      // 窗格 ID
}
```

### SplitContainer（container.rs）

递归分割容器：

```rust
pub struct SplitContainer {
    pub direction: Direction,
    pub children: Vec<SplitChild>,
    pub ratios: Vec<f32>,  // 每个子元素的宽度/高度比例
    pub id: u64,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(direction)` | 创建空容器 |
| `split_pane(pane_id, new_pane_id, direction)` | 递归分割：同方向在容器内插入相邻，垂直方向创建嵌套容器 |
| `split_pane_directional(pane_id, new_pane_id, side: SplitSide)` | 四方向分割：Right/Down 在目标后插入，Left/Up 在目标前插入；垂直方向时创建嵌套容器 |
| `close_pane(pane_id)` | 递归关闭子窗格，自动清理空容器 |
| `equalize()` | 递归等分所有子元素比例 |
| `adjust_ratio(index, delta)` | 调整指定子元素的比例（用于拖拽） |
| `find_container_mut(id)` | 递归查找指定 ID 的容器（用于拖拽时 adjust_ratio） |
| `pane_ids()` / `pane_count()` | 获取所有窗格 ID 和数量 |

**分割算法：**

`split_pane_directional` 逻辑：
1. 在目标 pane 所在容器中查找
2. 同方向拆分 → 在 children 中相邻位置插入新 pane
3. 垂直方向拆分（Vertical split from Horizontal side 或反之）→ 创建嵌套容器包裹目标和新区

**关闭算法：**

`close_pane` 会：
1. 递归在 children 中查找 pane
2. 从容器中移除该 child
3. 如果容器 children 变成 1，则用剩余 child 替换容器（容器合并）
4. 递归向上处理父容器

## 布局计算（layout.rs）

### 常量

| 常量 | 值 | 说明 |
|------|-----|------|
| `DIVIDER_WIDTH` | 4.0 | 分隔线可见宽度（像素） |
| `DIVIDER_HIT_WIDTH` | 8.0 | 分隔线命中检测区域宽度（像素） |

### PaneRect

```rust
pub struct PaneRect {
    pub x: f32, pub y: f32, pub w: f32, pub h: f32,
}
```

### DividerInfo

拖拽时找到的分隔线信息：

```rust
pub struct DividerInfo {
    pub container_id: u64,    // 所属容器 ID
    pub child_index: usize,   // 左/上侧子元素索引
    pub direction: Direction, // 方向
    pub rect: PaneRect,       // 分隔线矩形
    pub container_size: f32,  // 容器在分割方向上的总像素大小
}
```

### 布局函数

| 函数 | 说明 |
|------|------|
| `compute_pane_rects(container, x, y, total_w, total_h)` | 递归计算所有窗格的像素矩形，返回 `Vec<(pane_id, PaneRect)>` |
| `compute_divider_rects(container, x, y, total_w, total_h)` | 递归计算所有分隔线的矩形 |
| `divider_at_point(container, px, py, x, y, w, h)` | 检测鼠标坐标是否在分隔线上（使用 `DIVIDER_HIT_WIDTH` 扩展命中区域） |
| `pane_at_point(layout, px, py)` | 检测鼠标坐标在哪个窗格上 |

**compute_pane_rects 算法：**
1. 计算分隔线总宽度：`num_gaps * DIVIDER_WIDTH`
2. 垂直分割：可用宽度减去分隔线，按 ratios 分配给每个子元素
3. 水平分割：可用高度减去分隔线，按 ratios 分配
4. 递归处理嵌套容器

## 父级导航

← [src/AGENTS.md](../AGENTS.md)
