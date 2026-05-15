# Triggers System — Design Spec

## Context

PureTerm 已有孤立的 `TriggerService`（`crates/pureterm-core/src/trigger.rs`）：6 种动作、regex 评估、内置默认规则，但**未接入任何 session/terminal 输出流**——是死代码。本 spec 设计完整接入 + 5 种新增动作，对齐 iTerm2 的 Triggers 能力。

参考差距文档：`docs/feature-gap-analysis.md` §1。

## Goals

- 把 `TriggerService` 接入 PTY 输出流，规则可配置、可视化编辑、对生产负载安全。
- 新增 5 种动作：`SendText`、`CaptureToFile`、`Hyperlink`、`DirectoryTrack`、`PasswordMask`。
- 既有 6 种动作（`Notify` / `PlaySound` / `Highlight` / `RunCommand` / `CopyMatch` / `Bell`）全部 work。

## Non-Goals

- 不集成 Keychain / 1Password（PasswordMask 只触发可视脱敏）。
- 不做 profile 级覆盖；本轮仅全局 triggers。
- 不做后台线程 / RegexSet 高阶优化；MVP 在主线程对新完成行评估。

## Architecture

### 数据流

```
PTY bytes → output_buffer → pane.drain_output()
                                  │
                                  ├─ terminal.write(buffer)            # VTE → Screen
                                  │
                                  └─ for line in terminal.drain_completed_lines():
                                        for m in trigger_service.evaluate(&line.text):
                                            pane.dispatch(m, &line)    # 副作用归 pane
```

### 设计选择理由

**为何选"队列 + pane 拉取"而非纯回调**：Term 内部触发回调时，pane 无法再持 `&mut term` 应用 `Highlight` / `Hyperlink` 写回（再入借用）。队列让 pane 在 `drain_output()` 同 frame 内同步取走、做匹配、回调 Term 写回，全部在主线程完成，零跨线程同步。

**为何 line_id 用 u64 单调自增 + Grid 并行数组**：Grid 已有 `marks` / `double_modes` / `wrapped` 的并行行元数据惯用法，line_id 走同一条路最贴合。`u64` 永不回绕，Grid 的 scrollback 淘汰策略只让 pane 拿到的旧 id "查找返 None"，pane harmless 跳过。

**为何 Highlight 复用 SGR 背景色**：`Cell.bg: Color` 已存在，render pipeline 已支持任意 RGB 背景，与 search highlight 同机制。无需新 render pass。

**为何 PasswordMask 走 SecretRedaction**：`crates/pureterm-core/src/secret_redaction.rs` 已实现可视脱敏（替换为 `•`）。Trigger 只在匹配区间标记，复用全部下游渲染。

## Components

### 1. `pureterm-core/src/trigger.rs`（扩展现有文件）

**新增 5 种 action 变体**：
```rust
pub enum TriggerAction {
    // 已有
    Notify { title: String, body: String },
    PlaySound { sound: Option<String> },
    Highlight { color: String, scope: HighlightScope },   // scope 字段为新增
    RunCommand { command: String },
    CopyMatch,
    Bell,
    // 新增
    SendText { text: String, append_newline: bool },
    CaptureToFile { path: String, max_bytes: u64 },
    Hyperlink { url_template: String },
    DirectoryTrack,
    PasswordMask,
}

pub enum HighlightScope { WholeLine, Match }
```

**模板字符串支持**（`SendText.text`、`CaptureToFile.path`、`Hyperlink.url_template`）：
- `\1`..`\9` —— regex 捕获组
- `${cwd}` —— 当前 OSC 7 报告的 cwd
- `${date}` —— ISO-8601 当前时间
- `${env:NAME}` —— 环境变量（缺失时空字符串）

新增辅助函数 `expand_template(tpl: &str, ctx: &TemplateContext) -> String`。

**TriggerService 内部优化**：缓存编译后的 regex（`compiled: HashMap<String, Regex>`），新增/修改规则时重建该项；evaluate 不再每次重新编译。

**TriggerMatch 增加字段**：
```rust
pub struct TriggerMatch {
    pub trigger_id: String,
    pub matched_text: String,
    pub start: usize,
    pub end: usize,
    pub captures: Vec<String>,    // 新增：用于模板插值
    pub action: TriggerAction,
}
```

### 2. `pureterm-terminal`（终端引擎扩展）

**`screen/grid.rs`** —— 增加 line-id 并行数组：
```rust
struct Grid {
    // 现有字段...
    line_ids: Vec<Option<u64>>,   // 与 visible+history 行平行
    next_line_id: u64,
}

impl Grid {
    pub fn next_line_id(&mut self) -> u64;          // 自增并返回
    pub fn line_id_for_visible(&self, row: usize) -> Option<u64>;
    pub fn find_row_by_line_id(&self, id: u64) -> Option<RowLocation>;  // Visible(usize) | History(usize)
    pub fn row_mut_by_line_id(&mut self, id: u64) -> Option<&mut Vec<Cell>>;
}
```

行 id 在以下时机分配：滚动产生新底部行、scroll_region_up 产生新底部行、resize 添加行。

**`parser/perform.rs`** —— 在 linefeed 路径插入"行完成"队列：
```rust
struct VtePerform {
    // 现有字段...
    completed_lines: VecDeque<CompletedLine>,    // 上限 256
}

pub struct CompletedLine {
    pub line_id: u64,
    pub text: String,
    pub cwd: Option<String>,    // 完成时刻的快照
}
```

入队时机：
- `execute(LF)` 在 cursor 实际换行后，把刚刚的"上一行"提取为 text 入队。
- `linewrap()` 在 auto-wrap 触发时同样入队。
- ED/EL 等 erase 操作 **不**入队。

文本提取：从 row cells 读取，跳过 `WideMode::SpacerTail/SpacerHead`，trim 尾部空白。

**`term.rs`** —— 三个新公开 API：
```rust
impl Terminal {
    pub fn drain_completed_lines(&mut self) -> Vec<CompletedLine>;
    pub fn apply_highlight(&mut self, line_id: u64, range: Range<usize>, bg: Color);
    pub fn apply_hyperlink(&mut self, line_id: u64, range: Range<usize>, url: &str);
    pub fn apply_password_mask(&mut self, line_id: u64, range: Range<usize>);
}
```

`apply_*` 走 `Grid::row_mut_by_line_id` 拿到 row，遍历 range 写 cell 字段。`Highlight.WholeLine` 时 range = `0..cols`。

### 3. `pureterm-session/src/middleware.rs` —— 不动

中间件用于字节级处理（如 zmodem 二进制守卫），triggers 在 Screen 层评估，与中间件不冲突。`MiddlewareStack` 不增加新成员。

### 4. `pureterm-ui/src/terminal/pane.rs` —— 集成入口

新增字段：
```rust
struct Pane {
    // 现有...
    trigger_service: Option<Arc<RwLock<TriggerService>>>,
    capture_writers: HashMap<String, BufWriter<File>>,    // path → file handle，复用 SessionLog 模式
    template_ctx_env: HashMap<String, String>,            // 进程启动时快照
}
```

在 `drain_output()` 中 `self.terminal.write(&buffer);` 之后插入：
```rust
if let Some(svc) = &self.trigger_service {
    let lines = self.terminal.drain_completed_lines();
    let svc = svc.read().unwrap();
    for line in lines {
        for m in svc.evaluate_with_captures(&line.text) {
            self.dispatch_trigger(m, &line);
        }
    }
}
```

`dispatch_trigger` 按 action 分派：

| Action | Pane 行为 |
|--------|-----------|
| `Notify` | 调用 `self.terminal` 已注册的 notification_callback |
| `PlaySound` | 复用 bell 通道 |
| `Bell` | 复用 bell 通道 |
| `Highlight` | `terminal.apply_highlight(line_id, range, parse_color(color))` |
| `CopyMatch` | 调用 clipboard_callback |
| `RunCommand` | `std::process::Command::new("sh").arg("-c").arg(expanded).spawn()`（detach） |
| `SendText` | 模板展开后 `self.session.write(text.as_bytes())` |
| `CaptureToFile` | 路径展开后写入 `~/.local/share/pureterm/captures/<path>`（追加 `\n`），按 `max_bytes` 滚动（rename `.0`） |
| `Hyperlink` | 模板展开后 `terminal.apply_hyperlink(line_id, range, url)` |
| `DirectoryTrack` | `terminal.set_current_dir(matched_text)` |
| `PasswordMask` | `terminal.apply_password_mask(line_id, range)` |

### 5. `pureterm-core/src/config/mod.rs` —— 配置接入

```rust
#[serde(default = "default_triggers")]
pub triggers: Vec<Trigger>,
```

`default_triggers()` 返回现有 `TriggerService::default_triggers()` 三条内置规则。
`Default` impl 同步加上 `triggers: default_triggers()`。

config 加载完成后 `app.rs` 创建 `Arc<RwLock<TriggerService>>` 并注入每个 pane。

### 6. `pureterm-ui/src/settings/triggers.rs` —— 新增 Settings 页

参考现有 `settings/general.rs`、`settings/appearance.rs` 的 egui 模式。功能：
- 表格列出全部 triggers（id / name / pattern / action 概要 / enabled toggle）
- "Add" 按钮 → 弹窗 / inline form：选择 action 类型、填字段、regex 实时校验
- 行内 "Edit" / "Delete"
- "Test" 输入框：粘贴样本文本，下方实时显示哪条规则会匹配、捕获组内容
- 保存写回 `Config.triggers`，触发 `ConfigService::save()`，热更新现有 `Arc<RwLock<TriggerService>>`

注册到 settings tab list（`settings/mod.rs` 现有的 tab 枚举）。

## 性能边界

- 单次 `drain_completed_lines()` 上限 256 行；超过时丢最早并 `log::warn!`。
- `TriggerService::evaluate_with_captures` 用 `RegexSet` 先一趟过滤再做局部捕获——单行匹配 < 50µs（n=20 触发器）。
- 高频日志输出（万行/秒）下：worst case = 256 lines × 50µs = 12ms/帧——仍在 60fps 预算内。
- `capture_writers` 用 `BufWriter`，每个文件一次性 open，`drop(pane)` 时 flush。

## 错误处理

- 无效 regex：保留现有行为（`log::warn!` 跳过）。
- `RunCommand` 启动失败：`log::error!`，不影响其他动作。
- `CaptureToFile` IO 失败（权限、磁盘满）：`log::error!`，下次匹配重试 open；不阻塞 pane 渲染。
- 模板插值缺失变量：替换为空字符串，不报错。
- `apply_highlight` 等的 `line_id` 未找到（已淘汰）：silently no-op。

## 安全边界

- `RunCommand` 走 `sh -c`，用户配置项；接入 settings UI 时显示警告。
- `SendText` 不会回包含 `\r` / `\n` 之外的转义注入到 PTY，文本展开后直接写。
- 模板 `${env:NAME}` 仅读 pane 启动时的 env 快照，避免泄露后续 setenv 的密钥。
- `CaptureToFile.path` 经过 `Path::canonicalize` + 限定在 `~/.local/share/pureterm/captures/`；用户传绝对路径也会重锚到该目录。

## 测试策略

| 层级 | 测试 |
|------|------|
| `pureterm-core/trigger` | regex 编译缓存、模板插值、`evaluate_with_captures` 捕获组、所有 action 变体序列化往返 |
| `pureterm-terminal/grid` | line_id 单调性、scroll/scrollback 淘汰后查找返 None、resize 后 id 稳定 |
| `pureterm-terminal/perform` | LF 入队、wrap 入队、ED/EL 不入队、completed_lines 上限 |
| `pureterm-terminal/term` | apply_highlight / apply_hyperlink / apply_password_mask 写回正确，跨 visible/history 都 work |
| `pureterm-ui/pane` | dispatch_trigger 各分支（mock session writer / clipboard cb / file fs） |
| 集成 | E2E：写入 `error: foo\nwarning: bar\n` 后断言 cells.bg 染色、bell 触发、capture file 内容 |

## 迁移与配置兼容

- 老 `Trigger` 结构无 `captures` 字段——保持 backward compatible，反序列化不破坏。
- 老配置无 `triggers` 数组——`#[serde(default)]` 默认填三条内置。
- `TriggerAction` 加新变体——untagged enum 反序列化不影响现有规则。

## 实施顺序

1. **基础**：`trigger.rs` 扩 5 个 action 变体 + 模板插值 + `RegexSet` 缓存 + 测试。
2. **Term API**：Grid `line_ids` + perform `completed_lines` + Term `drain_completed_lines` / apply_* + 测试。
3. **Pane 集成**：`drain_output` 接入 + dispatch 分派 + capture writers + 测试。
4. **Config**：`Config.triggers` + `default_triggers` + ConfigService 热更新通道。
5. **Settings UI**：triggers 页表格 + Add/Edit/Delete + Test 输入框。
6. **App wiring**：`app.rs` 启动时构造 `Arc<RwLock<TriggerService>>` 注入新建 pane。

## 开放问题

无（已通过澄清问答收敛）。

