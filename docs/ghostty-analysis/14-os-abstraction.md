# 14 - 操作系统抽象

## 功能概述

跨平台操作系统接口抽象层：文件系统、环境变量、路径、用户信息、XDG 目录、macOS 特定 API、进程管理、国际化。

## Ghostty 源码文件

| 文件 | 功能 |
|------|------|
| `os/env.zig` | 环境变量读写 |
| `os/path.zig` | PATH 搜索 |
| `os/file.zig` | 文件描述符限制、临时目录 |
| `os/xdg.zig` | XDG Base Directory |
| `os/homedir.zig` | 主目录检测 |
| `os/shell.zig` | Shell 命令构建 |
| `os/macos.zig` | macOS 版本检测、AppSupport 目录、QoS |
| `os/desktop.zig` | 桌面环境检测 |
| `os/pipe.zig` | 跨平台管道 |
| `os/open.zig` | 用默认程序打开 URL/文件 |
| `os/hostname.zig` | 主机名验证 |
| `os/flatpak.zig` | Flatpak 检测 |
| `os/mach.zig` | macOS Mach VM 标签 |
| `os/cgroup.zig` | Linux cgroup 检测 |
| `os/passwd.zig` | Unix passwd 查询 |
| `os/mouse.zig` | 系统双击间隔 |
| `os/args.zig` | 参数迭代器 |
| `os/locale.zig` | 区域设置初始化 |
| `os/TempDir.zig` | 临时目录管理 |
| `os/resourcesdir.zig` | 资源目录定位 |
| `os/uri.zig` | URI 解析 |
| `os/kernel_info.zig` | Linux 内核版本 |
| `os/dbus.zig` | D-Bus 检测 |
| `os/systemd.zig` | systemd 检测 |
| `os/i18n.zig` | 国际化 |
| `os/wasm.zig` | WASM OS 抽象 |
| `os/windows.zig` | Windows 工具 |

## 纯 Rust 实现要点

大部分功能 Rust 标准库和常用 crate 已覆盖：

| 功能 | Rust 方案 |
|------|----------|
| 环境变量 | `std::env` |
| 路径/文件 | `std::path`, `std::fs` |
| 主目录 | `dirs` crate |
| XDG 目录 | `dirs` crate |
| 主机名 | `hostname` crate |
| macOS API | `core-foundation` crate |
| 区域设置 | `sys-locale` crate |
| 子进程 | `std::process::Command` |
| PTY | `portable-pty` crate |
| 打开 URL | `open` crate |

**关键点**：Rust 生态已有成熟的跨平台方案，不需要手写 OS 抽象层。直接在各 crate 中使用标准库和平台 crate 即可。
