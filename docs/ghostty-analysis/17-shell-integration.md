# 17 - Shell 集成

## 功能概述

自动为各种 Shell 设置语义提示标记（OSC 133），实现命令检测、提示识别、工作目录追踪。

## Ghostty 源码文件

| 文件 | 行数 | 功能 |
|------|------|------|
| `termio/shell_integration.zig` | ~500 | Shell 检测和集成设置 |

## 支持的 Shell

| Shell | 检测方式 | 集成方式 |
|-------|---------|---------|
| bash | 从 SHELL 或 passwd | 注入 `PROMPT_COMMAND` + `DEBUG trap` |
| zsh | 从 SHELL 或 passwd | 注入 `precmd` + `preexec` 函数 |
| fish | 从 SHELL 或 passwd | 注入 `fish_prompt`/`fish_preexec` |
| nushell | 从 SHELL | 注入 `env.config.hooks` |
| elvish | 从 SHELL | 注入 `edit:before-readline` |

## 集成注入的标记

```
OSC 133 ; A ; <opts> ST  - 提示开始
  opts: dreaw=true, click=true, ...

OSC 133 ; B ST            - 提示结束/命令开始

OSC 133 ; C ST            - 命令执行开始

OSC 133 ; D [; <exit>] ST - 命令输出结束
  exit: 退出码（0=成功）

OSC 7 ; file://host/path ST - 当前工作目录
```

## 纯 Rust 实现要点

1. **Shell 检测**：检查 `SHELL` 环境变量
2. **集成模式**：`detect`（自动检测）| `none` | `zsh` | `bash` | ...
3. **注入方式**：通过 `pre_exec` 回调设置环境变量
4. **Shell 脚本**：需要为每种 Shell 编写注入脚本

```rust
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Nushell,
    Elvish,
}

pub fn setup_shell_integration(shell: &Shell, env: &mut HashMap<String, String>) {
    match shell {
        Shell::Zsh => {
            env.insert("ZDOTDIR".into(), integration_script_dir());
        }
        // ...
    }
}
```
