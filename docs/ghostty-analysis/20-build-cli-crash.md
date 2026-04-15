# 20 - 构建/CLI/崩溃/基准

## 功能概述

构建系统、命令行界面、崩溃报告、性能基准测试。这些是工具性模块，不是终端核心。

## Ghostty 源码文件

### 构建系统 (`build/`, 35 files)
Ghostty 用 Zig 构建系统的自定义步骤。PureTerm 用 Cargo 不需要这些。

### CLI (`cli/`, 25 files)
命令行子命令：`+help`, `+version`, `+list-fonts`, `+list-themes`, `+show-config` 等。

### 崩溃报告 (`crash/`, 4 files)
Sentry SDK 集成，仅本地存储，不发送网络。

### 基准测试 (`benchmark/`, 12 files)
性能基准：VT 解析、OSC 解析、屏幕克隆等。

### 合成数据 (`synthetic/`, 9 files)
随机测试数据生成器。

### Shell 补全 (`extra/`, 5 files)
bash/zsh/fish 补全和 Vim/Sublime 语法文件。

### Terminfo (`terminfo/`, 3 files)
Ghostty 的 terminfo 定义。

## 纯 Rust 实现要点

### CLI（需要的部分）
用 `clap` crate 实现：

```rust
#[derive(Parser)]
#[command(name = "pureterm")]
enum Cli {
    /// 运行终端
    Run,
    /// 显示配置
    ShowConfig,
    /// 列出字体
    ListFonts,
    /// 列出主题
    ListThemes,
    /// 显示版本
    Version,
}
```

### 构建系统
用 Cargo 标准构建，不需要自定义构建系统。

### 崩溃报告
可选，用 `sentry` crate。

### Terminfo
需要为 PureTerm 定义自己的 terminfo 条目，或使用 `xterm-256color` 兼容。

### 其余
不需要实现。
