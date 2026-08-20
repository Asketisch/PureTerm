# GAP-65 — 终端自识别（TERM_PROGRAM + XTVERSION + XTGETTCAP）

## 背景 / 动机

Agent CLI 通过环境变量 + 运行时探测序列识别宿主终端，决定启用哪些能力（图片协议、keyboard protocol、真彩等）。当前 PureTerm：
- 子进程环境**只注入 `TERM`/`COLORTERM`**（`tty/mod.rs:100-108`），**缺 `TERM_PROGRAM`/`TERM_PROGRAM_VERSION`/`PURETERM`** → Agent 无法识别为 PureTerm。
- DA1/DA2 响应报告的是 alacritty 风格（`mod.rs:1284 ?6c`、`mod.rs:1293 >0;v;1c`），未带 PureTerm 标识。
- **不响应 XTVERSION**（`CSI > 0 q`，应回 `DCS > | PureTerm <ver> ST`）→ Agent 拿不到精确终端名+版本。
- **不响应 XTGETTCAP**（`DCS + q`，按 terminfo 名查能力）→ Agent 无法按 terminfo key 查 RGB/kitty-graphics 等。

这导致 Agent（尤其是 Claude Code/Codex CLI 的能力探测）无法可靠识别 PureTerm，降级到保守能力集。

参考：[XTVERSION](https://vtdn.dev/docs/dcs/xtversion)、[XTGETTCAP](http://vtdn.dev/docs/dcs/xtgettcap/)、Win Terminal #18382。

## 目标

1. 子进程拿到 `TERM_PROGRAM=PureTerm` 等标识变量。
2. XTVERSION 响应 `PureTerm <version>`。
3. XTGETTCAP 响应常用 terminfo 能力布尔值。
4. DA2 响应带 PureTerm 标识。

## 现状证据

- 环境注入：`tty/mod.rs:100-109`（只 TERM/COLORTERM）；调用点 `terminal_context/construct.rs:73`
- DA1/DA2：`term/mod.rs:1280-1298`
- XTVERSION/XTGETTCAP：全仓库 0 命中

## 变更点

### A. 环境变量注入
`crates/alacritty_terminal/src/tty/mod.rs:100 setup_env()`：
```rust
unsafe { env::set_var("TERM_PROGRAM", "PureTerm"); }
unsafe { env::set_var("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")); }
unsafe { env::set_var("PURETERM", "1"); }
```
同样在 daemon / ssh spawn 路径注入（`pureterm-daemon`、`pureterm-ssh` 的 env 构造处）。

### B. DA2 响应改 PureTerm 标识
`term/mod.rs:1293` secondary DA：版本号编码改为 PureTerm 自有标识（保持 6c primary 不变以维持 xterm 兼容）。

### C. XTVERSION 响应
vte 0.15 是否把 `CSI > 0 q` 路由到 handler 需确认；若 vte 未处理（unhandled），需在 PureTerm 字节扫描器（`event_loop.rs`）加 `scan_xtversion` —— 但 XTVERSION 是**查询**（程序→终端），终端要**响应**（写 PTY），这与现有扫描器（终端←程序）方向不同。

正确做法：vte 的 CSI `q` dispatch（`> 0 q`）会调 `terminal.attribute_status`。若 vte 未实现，则在 `term/mod.rs` 的 handler trait 增加该 dispatch，或复用 `identify_terminal` 路径：
- 响应 `Event::PtyWrite(Arc::new(format!("\x1bP>|PureTerm {}\x1b\\", version)))`

### D. XTGETTCAP 响应
`DCS + q <hex-key1> ; <hex-key2> ST` → 终端回 `DCS 1 + r <hex-key>=<hex-val> ST`（支持则 val，不支持回 `0`）。
- 在 vte DCS dispatch 或 PureTerm 扫描器识别 `DCS + q`，解析 hex key，查内置能力表：
  - `Tc` / `RGB` → 支持（true，回 `1`）
  - `hs`（has status line）/ `tsl`/`fsl` → 标题能力（PureTerm 支持 OSC 0/2）
  - 其他未实现的 → `0`
- 本轮实现核心几个（`Tc`/`RGB`/`setrgbf`/`setrgbb`），其余回 `0`。

### E. 版本号常量
集中定义 `pub const PURETERM_VERSION: &str = env!("CARGO_PKG_VERSION");` 供多处引用。

## 验证 / Test Plan

**手测**：
```sh
echo "TERM_PROGRAM=$TERM_PROGRAM"
# XTVERSION（需程序发查询，用 printf）
printf '\x1b[>0q'   # 然后从 PTY 读响应
# 在 shell 里：XTVERSION 探测脚本
python3 -c 'import sys; sys.stdout.write("\x1b[>0q"); sys.stdout.flush()'
```
预期响应包含 `PureTerm <version>`。

**单测**：setup_env 注入后 env 包含三个变量（需在测试中隔离 env）。

**回归**：现有 DA1/DA2 响应不破坏（Agent 仍能拿到 6c）。

## 优先级 / 范围

- **优先级**：P0（Agent 自识别是能力协商前提）
- **范围**：env 注入 + XTVERSION + XTGETTCAP 核心 key + DA2 标识
- **不做**：完整 terminfo 数据库（只覆盖 Agent 常用的几个 boolean/string capability）
