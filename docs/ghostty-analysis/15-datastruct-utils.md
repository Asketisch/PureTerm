# 15 - 数据结构与工具

## 功能概述

Ghostty 使用的自定义数据结构、Unicode 支持和 SIMD 加速例程。

## 数据结构 (`datastruct/`)

| 文件 | 功能 | Rust 替代 |
|------|------|----------|
| `blocking_queue.zig` | SPSC 阻塞队列 | `crossbeam::channel` |
| `cache_table.zig` | 固定桶哈希表 + LRU | `lru` crate |
| `circ_buf.zig` | 循环缓冲区 | `VecDeque` |
| `split_tree.zig` | 不可变分割树 | 自定义树结构 |
| `segmented_pool.zig` | 稳定指针池 | `slotmap` crate |
| `intrusive_linked_list.zig` | 侵入式双向链表 | 自定义或 `intrusive-collections` |
| `lru.zig` | LRU HashMap | `lru` crate |
| `message_data.zig` | 小数据优化消息 | `smallvec` + enum |
| `array_list_collection.zig` | ArrayList 集合 | `Vec<Vec<T>>` |
| `comparison.zig` | 深度相等比较 | `PartialEq` derive |

## Unicode 支持 (`unicode/`)

| 文件 | 功能 | Rust 替代 |
|------|------|----------|
| `grapheme.zig` | 字形簇断字 | `unicode-segmentation` crate |
| `props.zig` | Unicode 属性 | `unicode-width` crate |
| `props_table.zig` | 属性查找表 | 编译时生成或使用现成 crate |
| `symbols_table.zig` | 符号查找表 | 编译时生成 |
| `lut.zig` | 3 级查找表生成器 | 不需要，直接用 crate |

## SIMD 加速 (`simd/`)

| 文件 | 功能 | Rust 替代 |
|------|------|----------|
| `vt.zig` | SIMD UTF-8 解码到 ESC | `simdutf8` crate |
| `base64.zig` | SIMD Base64 解码 | `base64` crate (已 SIMD) |
| `index_of.zig` | SIMD 字节搜索 | `memchr` crate |
| `codepoint_width.zig` | SIMD 码点宽度 | `unicode-width` crate |

## 其他工具

| 文件 | 功能 | Rust 替代 |
|------|------|----------|
| `fastmem.zig` | 快速 memcpy/memmove | `std::ptr::copy` (已优化) |
| `math.zig` | 4x4 矩阵 + 正交投影 | `cgmath` / `glam` crate |
| `quirks.zig` | 环境 workaround | 必须实现（兼容各种终端行为） |
| `tripwire.zig` | 错误注入测试 | 必须实现（用于测试验证） |
| `file_type.zig` | 图像类型检测 | `infer` crate |

## 纯 Rust 实现要点

**核心观点**：Ghostty 手写了很多数据结构和工具，是因为 Zig 生态不如 Rust 成熟。Rust 中大多数都有现成 crate：

```toml
[dependencies]
# 数据结构
crossbeam = "0.8"           # 无锁队列
lru = "0.12"                # LRU 缓存
slotmap = "1.0"             # 稳定 ID 池
smallvec = "1.13"           # 小数据优化

# Unicode
unicode-segmentation = "1.11"  # 字形簇
unicode-width = "0.2"          # 字符宽度

# SIMD/性能
memchr = "2.7"              # SIMD 字节搜索
simdutf8 = "0.1"            # SIMD UTF-8 验证
base64 = "0.22"             # Base64 编解码

# 数学
glam = "0.28"               # 线性代数

# 文件检测
infer = "0.16"              # 文件类型检测
```

唯一需要手写的是 `SplitTree`（分割视图树），因为它是终端特有的数据结构。
