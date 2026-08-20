# 更新日志

本终端内核 crate 的所有重要变更记录于此。各小节按 `Added`(新增)、
`Changed`(变更)、`Deprecated`(废弃)、`Fixed`(修复)、`Removed`(移除)
顺序排列。

**破坏性变更以粗体标注。**

## 0.26.1-dev

## 0.26.0

### 新增

- `tty::Options` 新增 `escape_args` 字段,用于控制 Windows shell 参数转义

### 变更

- macOS 上若存在 `~/.hushlogin`,向 `login` 传入 `-q`
- **`ChildEvent::Exited` 与 `Event::ChildExit` 现在携带 `ExitStatus` 而非 `i32`**

## 0.25.0

### 变更

- `Options::hold` 替换为 `Options::drain_on_exit`

## 0.24.2

### 新增

- 光标前进制表位的转义序列(CSI Ps I)

## 0.24.1

### 变更

- macOS 上不再加载 shell RC 文件

### 修复

- 语义搜索对全角字符的处理
- 行内搜索忽略换行标志的问题
- 主进程清理 `XDG_ACTIVATION_TOKEN` 与 `DESKTOP_STARTUP_ID`
- Unix 上关闭 PTY 时的 FD 泄漏
- ConPTY 创建失败时的崩溃

## 0.24.0

### 新增

- `tty::unix::from_fd()`:从已打开的 PTY 文件描述符创建 TTY

### 变更

- **`Term` 默认不再处于聚焦状态**
