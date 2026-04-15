# 12 - 终端 I/O 与 PTY

## 功能概述

终端的输入输出管理：PTY 管理、子进程启动、独立的 IO 线程、同步输出、Shell 集成。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `termio/Termio.zig` | ~800 | **主 Termio 结构**：终端 IO 状态 |
| `termio/Thread.zig` | ~600 | 写线程（xev 事件循环） |
| `termio/Exec.zig` | ~1000 | PTY 后端：子进程管理 |
| `termio/message.zig` | ~200 | IO 消息类型 |
| `termio/mailbox.zig` | ~200 | 邮箱实现（SPSC 队列） |
| `termio/Options.zig` | ~100 | Termio 配置 |
| `termio/backend.zig` | ~100 | 后端枚举 |
| `termio/stream_handler.zig` | ~800 | VT 流处理器（回调） |
| `termio/shell_integration.zig` | ~500 | Shell 集成设置 |
| `pty.zig` | ~500 | PTY 抽象（POSIX/Windows/Null） |
| `Command.zig` | ~900 | 子进程启动器 |

## 核心数据结构

### 1. Termio (`Termio.zig`)

```
Termio:
  ├── backend: Backend             - 后端 (Exec)
  ├── terminal: Terminal           - 终端状态
  ├── renderer_state: RenderState  - 渲染状态
  ├── mailbox: Mailbox             - 消息队列
  ├── stream: Stream(StreamHandler) - VT 流处理器
  ├── mutex_state: MutexState      - 互斥保护的状态
  ├── config: DerivedConfig        - 派生配置
  └── 方法:
        ├── init()                 - 初始化
        ├── threadEnter()          - 进入 IO 线程
        ├── process()              - 处理输入数据
        ├── resize()               - 终端大小变更
        ├── queueWrite()           - 队列写入数据
        ├── focusGained()          - 焦点获得
        └── changeConfig()         - 配置变更
```

### 2. IO 写线程 (`Thread.zig`)

```
Thread:
  ├── event_loop: xev             - 事件循环
  ├── mailbox: Mailbox            - 消息队列
  ├── wakeup: Async               - 唤醒句柄
  ├── pty_writer                  - PTY 写入端
  ├── sync_timer                  - 同步输出定时器
  ├── resize_state                - 大小调整状态
  └── 方法:
        ├── threadMain()          - 线程入口
        ├── drainMailbox()        - 处理消息
        ├── writePty()            - 写入 PTY
        └── startSynchronizedOutput() - 同步输出
```

**同步输出**：当终端请求同步模式时，延迟输出直到完整帧到达，避免屏幕撕裂。

### 3. Exec 后端 (`Exec.zig`)

```
Exec:
  ├── pty: Pty                    - PTY 实例
  ├── command: Command            - 子进程
  ├── read_thread: Thread         - 读取线程
  ├── termios_poll: ?Timer        - termios 轮询
  └── 方法:
        ├── init()                - 初始化 (fork + exec)
        ├── readThreadMain()      - 读取线程主循环
        ├── termiosPoll()         - 终端模式轮询
        └── waitpid()             - 等待子进程退出

Subprocess:
  ├── pid: pid_t
  ├── exited: bool
  └── exit_code: ?Exit
```

**读取线程**：持续从 PTY 读取数据，通过回调传递给 Termio 处理。

### 4. PTY (`pty.zig`)

```
PosixPty:
  ├── fd: fd_t                    - PTY 文件描述符
  ├──方法:
  │     ├── open(size)            - 打开 PTY
  │     ├── getSize()             - 获取终端大小
  │     ├── setSize(size)         - 设置终端大小
  │     ├── getMode()             - 获取 termios 模式
  │     ├── childPreExec()        - 子进程 exec 前设置
  │     └── getProcessInfo()      - 获取前台进程信息

childPreExec() 执行:
  1. 创建新会话 (setsid)
  2. 设置控制终端
  3. 重置信号处理
  4. 设置环境变量
```

### 5. 子进程 (`Command.zig`)

```
Command:
  ├── path: [:0]const u8          - 可执行路径
  ├── args: []const [:0]const u8  - 参数
  ├── env: ?*const EnvMap         - 环境变量
  ├── cwd: ?[:0]const u8          - 工作目录
  ├── stdin/stdout/stderr         - 标准 IO 重定向
  ├── os_pre_exec                 - OS 级 pre-exec 回调
  ├── rt_pre_exec                 - 运行时 pre-exec 回调
  ├── rt_post_fork                - 运行时 post-fork 回调
  └── pseudo_console              - Windows 伪控制台
```

### 6. 消息类型 (`message.zig`)

```
Message:
  ├── write_small: [64]u8         - 小数据写入 (inline)
  ├── write_stable: []const u8    - 稳定数据写入
  ├── write_alloc: []const u8     - 分配数据写入
  ├── resize: Size                - 大小变更
  ├── change_config: Config       - 配置变更
  ├── focused: bool               - 焦点变化
  ├── color_scheme_report         - 颜色方案报告
  ├── size_report: SizeReportKind - 大小报告
  ├── clear_screen: ClearScreen   - 清屏
  ├── scroll_viewport: i16        - 滚动视口
  ├── selection_scroll            - 选区滚动
  ├── jump_to_prompt: i16         - 跳转到提示
  ├── start_synchronized_output   - 开始同步输出
  ├── linefeed_mode: bool         - 换行模式
  ├── inspector: bool             - 检查器切换
  └── crash: CrashThread          - 崩溃测试
```

### 7. VT 流处理器 (`stream_handler.zig`)

```
StreamHandler 回调:
  ├── bell()                      - 响铃
  ├── title(title)                - 窗口标题变更
  ├── clipboard(clipboard_op)     - 剪贴板操作
  ├── write_pty(data)             - 写回 PTY
  ├── color_scheme(scheme)        - 颜色方案变更
  ├── device_attrs(da_type)       - 设备属性响应
  ├── device_status(ds_type)      - 设备状态响应
  ├── cursor_style(style)         - 光标样式变更
  ├── mouse_shape(shape)          - 鼠标形状变更
  ├── desktop_notification(...)   - 桌面通知
  ├── pwd(url)                    - 当前目录
  ├── report_prompt(...)          - 提示标记
  ├── icon_name(name)             - 图标名称
  ├── color_report(color)         - 颜色报告
  ├── kitty_graphics(...)         - Kitty 图形协议
  └── size_report(report)         - 大小报告
```

## 纯 Rust 实现要点

1. **PTY 用 `portable-pty` crate**：跨平台 PTY 管理
2. **IO 线程用 `std::thread`**：独立读取线程
3. **消息用 `crossbeam-channel`**：高性能无锁队列
4. **VT 流处理器**：回调/trait 模式
5. **Shell 集成脚本**：为 zsh/bash/fish 注入

### Rust 实现框架

```rust
pub struct Termio {
    backend: ExecBackend,
    terminal: Terminal,
    mailbox: crossbeam::Sender<IoMessage>,
    stream_handler: StreamHandler,
}

pub struct ExecBackend {
    pty: portable_pty::Pty,
    child: Box<dyn portable_pty::MasterPty>,
    read_thread: Option<std::thread::JoinHandle<()>>,
}

pub enum IoMessage {
    Write(Vec<u8>),
    Resize(Size),
    FocusGained(bool),
    ChangeConfig(Arc<Config>),
    // ...
}
```
