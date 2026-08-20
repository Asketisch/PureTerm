//! OSC 7 shell 集成的环境注入。
//!
//! 由 GUI 的直连本地 PTY 路径
//! (`pureterm::terminal_context`)与常驻守护进程
//! (`pureterm-daemon::session`)共用,使每个本地 shell 都经
//! OSC 7 上报工作目录——这正是新标签页/窗格
//! cwd 继承的动力来源。
//!
//! 返回一组经 `tty::Options::env` 注入子 shell 的环境变量,
//! 而非用 `std::env::set_var` 改动拉起方的进程
//! 环境。环境变量方式避开了此前基于 `set_var` 注入的两个
//! 问题:(1)并发守护会话间的竞态;(2)把临时 `ZDOTDIR` 泄漏到
//! 恢复之前发生的无关拉起。
//! 恢复之前发生的无关拉起。

use std::collections::HashMap;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

/// OSC 7 的 printf 主体:发出 `ESC ] 7 ; file://$HOST$PWD ST`,使
/// 终端获知 shell 的当前工作目录。
const OSC7_FUNC: &str = r#"_pt_osc7() { printf '\e]7;file://%s%s\e\\' "$HOST" "$PWD"; }"#;

/// 以 0o600(仅属主)权限把 `content` 写入 `path`。临时 ZDOTDIR 的
/// `.zshenv` 内嵌用户真实的 `ZDOTDIR`/`HOME` 路径,因此绝不能让
/// 其它人可读。由 OSC 7 与 OSC 133(GAP-15)注入共用。错误
/// 会浮出(调用方以 `let _ =` 吞掉,与此前行为一致)。
pub(crate) fn write_private(path: &str, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

/// 构建把 OSC 7 cwd 上报钩子注入新拉起本地
/// shell 的环境变量集。
///
/// `tag` 每次拉起必须唯一(如守护会话 id 或 GUI
/// 进程 pid),使每 shell 的临时 zdotdir 目录永不冲突。
///
/// - zsh:临时目录 `/tmp/.pureterm-zdotdir-<tag>/.zshenv` 恢复
///   原 `ZDOTDIR`,source 用户真实的 `.zshenv`,再把
///   `_pt_osc7` 注册到 `chpwd` 钩子并调用一次。
/// - bash:设置 `PROMPT_COMMAND` 来定义并运行 `_pt_osc7`,保留任何
///   既有的 `PROMPT_COMMAND`。
///
/// zsh + bash 已覆盖;fish / nu / elvish / PowerShell 由
/// [`crate::shell_integration`] 处理(OSC 7 + OSC 133 的合并路径);
/// 本模块是尚未选择完整集合者的 OSC-7 专用回退。
/// 受 cmux 的 shell 集成启发。
pub fn prepare_osc7_env(tag: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();

    // 拉起方进程看到的原始 ZDOTDIR;zsh 的引导
    // .zshenv 会恢复它,使用户自己的 zsh 配置照常加载。
    let real_zdotdir = std::env::var("ZDOTDIR")
        .unwrap_or_else(|_| std::env::var("HOME").unwrap_or_else(|_| "/".into()));

    // -- zsh:ZDOTDIR 引导 --
    // t-core-windows-conpty:改走 `std::env::temp_dir()`
    // 而非硬编码 `/tmp`(Windows 没有该路径;该路径也是
    // `shell_integration::temp_zdotdir` 所用的,使 v1 仅 OSC 7 与
    // v1 OSC 7 + OSC 133 保持一致)。
    let zdot_dir = crate::shell_integration::temp_zdotdir(tag);
    let zdot_str = zdot_dir.to_string_lossy().to_string();
    // 仅属主(0700):内部写入的 `.zshenv` 内嵌用户真实
    // ZDOTDIR/HOME 路径且会被 shell source,因此目录与文件都不得
    // 让他人可访问。0700 目录还能阻断共享主机上(可猜测的)
    // `/tmp` 路径的预填充竞态。(GAP-15 评审。)
    let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&zdot_dir);
    let zshenv_content = format!(
        r#"# PureTerm zsh bootstrap — restores original ZDOTDIR and loads OSC 7 hook.
ZDOTDIR="{real_zdotdir}"
if [ -f "$ZDOTDIR/.zshenv" ]; then . "$ZDOTDIR/.zshenv"; fi
{osc7_func}
autoload -Uz add-zsh-hook 2>/dev/null && add-zsh-hook chpwd _pt_osc7
_pt_osc7
"#,
        real_zdotdir = real_zdotdir.replace('"', "\\\""),
        osc7_func = OSC7_FUNC,
    );
    let _ = write_private(&format!("{zdot_str}/.zshenv"), &zshenv_content);
    env.insert("ZDOTDIR".to_string(), zdot_str);

    // -- bash:PROMPT_COMMAND 环境变量 --
    // 函数定义 + 初次调用必须放进 PROMPT_COMMAND,因为 bash
    // 启动时读取此环境变量。
    env.insert(
        "PROMPT_COMMAND".to_string(),
        format!(
            "{func}; _pt_osc7; __pt_pc=$PROMPT_COMMAND; PROMPT_COMMAND=\"_pt_osc7${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}\"",
            func = OSC7_FUNC,
        ),
    );

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_contains_zdotdir_and_prompt_command() {
        let env = prepare_osc7_env("test-tag");
        assert!(env.contains_key("ZDOTDIR"));
        assert!(env["ZDOTDIR"].ends_with("-test-tag"));
        assert!(env.contains_key("PROMPT_COMMAND"));
        assert!(env["PROMPT_COMMAND"].contains("_pt_osc7"));
    }
}
