# PureTerm 端到端测试(Python)

Rust 单元测试覆盖内部逻辑;这里的用例覆盖**真实进程之间**的接缝:
CLI ↔ GUI ↔ 守护进程 ↔ MCP ↔ 操作系统(窗口服务器、Unix socket)。

## 运行

```bash
# 1) 先构建二进制(测试跑的是真实二进制,不是 debug 断言)
cargo build --release --bin pureterm --bin puretermd

# 2) 跑全部用例
python3 -m pytest tests/e2e -v

# 只跑某一组
python3 -m pytest tests/e2e/test_display.py -v

# 指定被测二进制
PURETERM_BIN=target/debug/pureterm python3 -m pytest tests/e2e -v
```

## 用例分组

| 文件 | 覆盖范围 | 是否需要 GUI |
|------|----------|--------------|
| `test_cli_local.py` | 只走本地文件/配置的 CLI:命令组解析、退出码、JSON 形状、配置 CRUD、快捷键预设、MCP 安装、插件、记事本、hook 垫片 | 否 |
| `test_cli_ipc.py` | 经 Socket API 的 CLI:窗口/标签页/窗格/surface 操作、事件日志、tmux 兼容子集、`script` 自动化 | 是 |
| `test_rpc.py` | 原始 JSON-RPC 方法级覆盖(每一个已注册方法 + 错误路径) | 是 |
| `test_daemon.py` | `puretermd` 生命周期、会话增删查、GUI 冷启动是否真的挂上守护进程 | 部分 |
| `test_mcp.py` | MCP stdio 协议:握手、`tools/list`、`tools/call` 与 GUI 的桥接 | 部分 |
| `test_display.py` | **显示层**:抓真实窗口截图并对像素断言(非空白、字形已光栅化、背景色、无 panic 覆盖层) | 是 |
| `test_startup.py` | 启动里程碑回归护栏 + 启动耗时上限 | 是 |
| `test_session.py` | 会话跨 GUI 重启存活、启动占位符不卡死 | 是 |
| `test_graphics.py` | 终端图形链路(Kitty 图像真的画到屏幕上) | 是 |
| `test_browser.py` | 内嵌浏览器(WebView 加载 / 取 DOM / 跳转 / 等待选择器 —— 含**页面脚本动态插入**的元素) | 是 |
| `test_ssh.py` | SSH 通道(假 `ssh` 桩 + 配置档 → 连接 → 会话建立) | 是 |
| `test_triggers.py` | 模式匹配触发器(`[[triggers]]` → 匹配 → 动作 → 事件;含**非 ASCII 标题**) | 是 |
| `test_tmux_cc.py` | tmux 控制模式(`tmux -CC` 会话接进 GUI:标签页 / 当前屏幕 / 按键回传) | 是 |
| `test_agents.py` | agent 适配器安装/卸载 Claude Code 钩子(幂等 / 不动用户配置 / dry-run) | 否 |
| `test_windows_config.py` | 多窗口 / 工作区 / 配置读写 / 分割树手术(`window.*` / `workspace.switch` / `config.get|set` / `pane.detach|attach|swap|equalize|resize`) | 是 |

## 踩过的坑(写新用例前先读)

这些都不是"测试写得不好",而是**产品真 bug**,已被修掉并各有回归护栏:

- **窗口要按 pid 定位**,不能按进程名 —— 测试会话里会有多个 PureTerm
  实例先后启停,按名字取窗口会抓到上一个实例的残留窗口,截图拍错对象。
- **颜色断言不能匹配绝对值** —— `screencapture` 会套用显示器的广色域
  配置文件,纯品红 (255,0,255) 抓出来是 (234,52,247),纯绿 (0,255,0)
  抓出来是 (118,251,77)。用「基线差分」+「通道主导关系」判定。
- **IPC 就绪 ≠ 终端就绪** —— 窗口先显示 “Starting PureTerm…” 占位符,
  守护进程就绪后才创建 PTY 窗格。在那之前 `surface send-text` 会被
  **静默丢弃**(没有活动窗格)。`AppInstance.wait_for_terminal()` 负责等。
- **往 shell 灌长命令会被 zsh 吞掉** —— zsh 行编辑器(ZLE)对单行长度有
  512 字节上限。要在终端里执行长内容,得让 shell 自己 `printf` 出来,
  且转义序列里的 ESC 必须写成**文本** `\033`(真 ESC 字节会被当成按键)。
- **配置要在应用启动前写** —— SSH 档案、图形开关等都是**启动时**读进
  快照的(CLI 走 IPC 拿的是那份快照)。用 `app_factory` fixture:它把
  「写配置」放在启动之前。
- **SSH 断言看事件日志**(`tab.created` / `session.spawned` /
  `ssh.state_changed`→connected),不要看 `surface snapshot` —— snapshot
  读的是**活动**窗格,而 SSH 会话在另一个标签页里,看 snapshot 会
  误判成「连的是本地 shell」(PTY 回显会让它「看起来」通过)。
- **沙箱要建在 `/tmp` 下** —— pytest 的 `tmp_path` 很长,会把 Unix socket
  路径顶破 104 字节上限。
- **浏览器用例要先把应用置前台** —— WKWebView 只在窗口可见/在前台时才
  加载页面并执行脚本求值;否则 `browser.get-title` 会一直等不到 JS 回调
  (报 “browser op channel closed”)。
- **`browser.*` 的返回值是 JS 求值对象**,不是裸字符串:标题在
  `result["title"]`、URL 在 `result["url"]`、DOM 文本在
  `result["value"]`。
- **往 pane 发文本必须带回车** —— `surface send-text` 原样发送,不带
  `\r` 的话 shell 只把文字留在提示符上,命令**根本没执行**;而 PTY
  回显会让它「看起来」执行了。
- **`tab list` 的字段名是 `title`**,`pane list` 才是 `tab_title`。
- **快照要轮询到目标内容出现** —— 快照一非空就返回会拿到命令执行**前**
  的画面,断言会假阴性。
- **tmux 控制模式附加时不会回放屏幕** —— tmux 3.6a 只发
  `%begin`/`%end`/`%session-changed`,**没有任何 `%output`**;必须主动发
  `capture-pane -p` 把当前屏幕取回来(结果在一个 `%begin flags=1` 的
  块里,块内是**原始行**)。
- **tmux 3.6a 不接受客户端发来的 `%input`** —— 它回一句
  `parse error: syntax error`,按键根本没送达;正确形式是
  `send-keys -t %<pane> -H <逐字节十六进制>`。
- **tmux 用例必须用独立 socket(`tmux -L <name>`)** —— tmux 服务器默认
  全机只有一个:不隔离的话测试会在**用户自己的**服务器里建/杀会话,而且
  多个控制模式客户端并发连接会让服务器在启动期**随机退出**(所有客户端
  只收到 `%exit server exited unexpectedly`)。
- **别给子命令加与全局参数同名的选项** —— 顶层已有全局 `--socket`
  (PureTerm 的 IPC 路径);`tmux-attach --socket` 会让 clap 在**解析时
  panic**,命令行直接崩。tmux 那个叫 `--tmux-socket`。
- **轮询判据用「不是 None」而不是「为真」** —— 索引 0 在 Python 里是
  falsy,`if value:` 会把「切到第 0 个窗口」误判成「还没切」。

## 设计约束

- **隔离**:每个用例拿到独立的 `HOME` / `TMPDIR` / `XDG_*`,绝不读写
  用户真实的配置或事件日志。CLI 调用一律在沙箱目录里执行 —— Claude Code
  的 MCP 适配器写的是**当前目录**下的 `.mcp.json`,否则会污染代码仓库。
- **短路径**:沙箱建在 `/tmp` 下而不是 pytest 的 `tmp_path`。pytest 的临时
  路径很长,会把 Unix socket 路径顶破 `sockaddr_un::sun_path` 的 104 字节
  上限,导致 socket 根本无法创建 —— 那是一种真实故障,不该被测试掩盖。
- **不留残留**:GUI 实例与守护进程在用例结束后被终止。守护进程**不会
  自己退出**(它要扛过 GUI 重启),所以 fixture 会显式调
  `pureterm daemon stop` 回收它 —— 否则几百个 `puretermd` 会把机器
  拖垮(实测套件耗时 85s → 345s,大批用例超时)。沙箱给每个用例**专属**
  的 daemon socket,所以回收它不会影响别的用例,更不会碰到用户自己的
  守护进程。

## 启动耗时基准

```bash
python3 scripts/bench_startup.py --rounds 5          # 分阶段耗时
python3 scripts/ab_startup.py A B --rounds 6          # 两个二进制交替对照
```

`PURETERM_TRACE_STARTUP=1` 会让应用把启动里程碑打到 stderr:

```
PURETERM_STARTUP main_enter 0.0
PURETERM_STARTUP app_new_enter 44.6
PURETERM_STARTUP app_new_exit 79.3
PURETERM_STARTUP first_frame 508.4
PURETERM_STARTUP terminal_ready 710.0
```
