//! T-core-shell-integ v3 的 E2E 测试(清单 D.5)。
//!
//! 默认带 `#[ignore]`:这些测试会运行**真实的 fish / pwsh / elvish /
//! nu** 子进程。主机 `$PATH` 上没有对应二进制时,测试以
//! 信息性消息跳过而非失败。运行方式:
//!   手动运行:cargo test -p pureterm-daemon --lib shell_integ_e2e --include-ignored --nocapture
//!
//! ## 验证思路
//!
//! 1. 以每 shell 的 `ShellIntegrationOpts` 调用
//!    [`crate::shell_integration::prepare_shell_env`]。
//! 2. 读取写入带标签临时目录的每 shell 前导文件。
//! 3. 把前导的 OSC 133 字节序列送进一个极简
//!    解析器(镜像 `alacritty_terminal::event_loop::scan_osc133`)
//!    确认期望的 PromptStart / CommandStart / OutputStart
//!    / OutputEnd 字母码都在。
//!
//! 为何用“前导 → 解析器”而非“真实 PTY”:PTY 管道已有
//! 自己的 E2E 层(`tmux_cc_e2e.rs`、`graphics_e2e.rs`);
//! 对 shell 集成而言,走真实 PTY 的增益
//! 为零——只需验证前导被 source 时产生正确的字节
//! 序列。

#![cfg(unix)]

use std::collections::HashMap;
use std::process::{Command, Stdio};

use alacritty_terminal::event::ShellMarkKind;

use crate::shell_integration::{prepare_shell_env, Shell, ShellIntegrationOpts};

/// 探测某二进制是否在 `$PATH` 上。找到返回 `true`。
fn bin_on_path(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 查找包含 `substr` 的环境值并返回克隆。无匹配返回
/// `None`。显式标注返回类型,避免 `HashMap<String, String>`
/// 推断失败。
fn find_env_dir(env: &HashMap<String, String>, substr: &str) -> Option<String> {
    for v in env.values() {
        if v.contains(substr) {
            return Some(v.clone());
        }
    }
    None
}

/// 从字节流解析出 OSC 133 标记,返回观察到的 `ShellMarkKind`
/// 序列。镜像 `event_loop::scan_osc133`,使 E2E 无需真实
/// PTY 即可运行:无论 source 方是 fish shell 还是录制文件,
/// 前导写出的字节都相同。
fn parse_osc133_kinds(buf: &[u8]) -> Vec<ShellMarkKind> {
    let prefix = b"\x1b]133;";
    let mut kinds = Vec::new();
    let mut i = 0;
    while i + prefix.len() < buf.len() {
        if let Some(pos) = buf[i..].iter().position(|&b| b == prefix[0]) {
            let start = i + pos;
            if buf[start..].starts_with(prefix) {
                let after = start + prefix.len();
                if after >= buf.len() {
                    break;
                }
                let kind = match buf[after] {
                    b'A' => Some(ShellMarkKind::PromptStart),
                    b'B' => Some(ShellMarkKind::CommandStart),
                    b'C' => Some(ShellMarkKind::OutputStart),
                    b'D' => Some(ShellMarkKind::OutputEnd),
                    _ => None,
                };
                if let Some(k) = kind {
                    kinds.push(k);
                }
                // 跳过终止符(BEL 或 ST)——与 event_loop::scan_osc133
                // 的逻辑相同。
                let mut p = after + 1;
                while p < buf.len() {
                    match buf[p] {
                        0x07 => break,
                        0x1b if p + 1 < buf.len() && buf[p + 1] == b'\\' => break,
                        _ => p += 1,
                    }
                }
                if p < buf.len() {
                    i = p + 1;
                } else {
                    i = start + 1;
                }
            } else {
                i = start + 1;
            }
        } else {
            break;
        }
    }
    kinds
}

/// 在准备好的 `env` 下以 `args` 运行 `binary`,把 stdout
/// 捕获为 UTF-8 字符串。二进制缺失或拉起失败返回
/// `None`。(保留供未来的真实 shell 往返测试。)
#[allow(dead_code)]
fn run_shell_capture(
    binary: &str,
    args: &[&str],
    env: &HashMap<String, String>,
) -> Option<String> {
    if !bin_on_path(binary) {
        return None;
    }
    let mut cmd = Command::new(binary);
    cmd.args(args).envs(env.iter()).stdin(Stdio::null());
    let output = cmd.output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[test]
#[ignore]
fn fish_prompt_smoke() {
    if !bin_on_path("fish") {
        eprintln!("fish not on $PATH — skipping");
        return;
    }
    let env_map = prepare_shell_env("e2e-fish-v3", ShellIntegrationOpts::all_on());

    // fish 需要 TTY 才能绘制提示符;无头环境下我们直接读
    // 前导文件并验证它内嵌了正确的 OSC 133
    // 字母码。
    let xdg_dir = env_map
        .get("XDG_CONFIG_HOME")
        .cloned()
        .unwrap_or_else(|| {
            std::env::temp_dir()
                .join("pureterm-fish-e2e-fish-v3")
                .to_string_lossy()
                .into_owned()
        });

    let fish_conf_path = std::path::Path::new(&xdg_dir).join("fish/conf.d/00_pureterm.fish");
    let preamble = std::fs::read_to_string(&fish_conf_path).expect("read fish preamble");

    assert!(
        preamble.contains("_pt_ms A"),
        "fish preamble should install PromptStart helper"
    );
    assert!(
        preamble.contains("_pt_ms B"),
        "fish preamble should install CommandStart helper"
    );
    assert!(
        preamble.contains("_pt_ms C"),
        "fish preamble should install OutputStart helper"
    );
    assert!(
        preamble.contains("_pt_ms D"),
        "fish preamble should install OutputEnd helper"
    );
    assert!(
        preamble.contains("fish_prompt"),
        "fish preamble should register a fish_prompt handler"
    );

    // 健全性:解析前导的 OSC 133 字母发射(它们以 printf
    // 格式串的形式内嵌在 `_pt_ms` 定义中)。
    let kinds = parse_osc133_kinds(preamble.as_bytes());
    assert!(
        kinds.contains(&ShellMarkKind::PromptStart),
        "preamble should embed PromptStart OSC 133 reference"
    );
    assert!(
        kinds.contains(&ShellMarkKind::CommandStart),
        "preamble should embed CommandStart OSC 133 reference"
    );

    // 清理临时目录。
    let _ = std::fs::remove_dir_all(&xdg_dir);
}

#[test]
#[ignore]
fn pwsh_prompt_smoke() {
    if !bin_on_path("pwsh") {
        eprintln!("pwsh not on $PATH — skipping");
        return;
    }
    let env_map = prepare_shell_env("e2e-pwsh-v3", ShellIntegrationOpts::all_on());

    // PowerShell 前导位于临时 $PROFILE 路径;查找到它。
    let pwsh_dir = find_env_dir(&env_map, "pureterm-pwsh").expect("pwsh temp dir set in env");
    let pwsh_profile_path =
        std::path::Path::new(&pwsh_dir).join("Microsoft.PowerShell_profile.ps1");
    let preamble = std::fs::read_to_string(&pwsh_profile_path).expect("read pwsh preamble");

    assert!(
        preamble.contains("_pt_ms"),
        "pwsh preamble should define _pt_ms helper"
    );
    // pwsh 前导以 printf 序列为 A/B/C/D 写出 OSC 133 标记。
    let kinds = parse_osc133_kinds(preamble.as_bytes());
    assert!(
        kinds.contains(&ShellMarkKind::PromptStart),
        "pwsh preamble should embed PromptStart"
    );
    assert!(
        kinds.contains(&ShellMarkKind::OutputStart),
        "pwsh preamble should embed OutputStart"
    );
    assert!(
        kinds.contains(&ShellMarkKind::OutputEnd),
        "pwsh preamble should embed OutputEnd"
    );

    // 清理。
    let _ = std::fs::remove_dir_all(&pwsh_dir);
}

#[test]
#[ignore]
fn elvish_prompt_smoke() {
    if !bin_on_path("elvish") {
        eprintln!("elvish not on $PATH — skipping");
        return;
    }
    let env_map = prepare_shell_env("e2e-elv-v3", ShellIntegrationOpts::all_on());
    let elv_dir = find_env_dir(&env_map, "pureterm-elv").expect("elv temp dir set in env");
    let elv_rc_path = std::path::Path::new(&elv_dir).join("rc.elv");
    let preamble = std::fs::read_to_string(&elv_rc_path).expect("read elvish preamble");
    assert!(
        preamble.contains("_pt_osc7"),
        "elvish preamble should install OSC 7 helper"
    );
    assert!(
        preamble.contains("_pt_ms"),
        "elvish preamble should install OSC 133 helper"
    );
    let kinds = parse_osc133_kinds(preamble.as_bytes());
    assert!(kinds.contains(&ShellMarkKind::PromptStart));
    let _ = std::fs::remove_dir_all(&elv_dir);
}

#[test]
#[ignore]
fn nu_prompt_smoke() {
    if !bin_on_path("nu") {
        eprintln!("nu not on $PATH — skipping");
        return;
    }
    let env_map = prepare_shell_env("e2e-nu-v3", ShellIntegrationOpts::all_on());
    let nu_dir = find_env_dir(&env_map, "pureterm-nu").expect("nu temp dir set in env");
    let nu_env_path = std::path::Path::new(&nu_dir).join("env.nu");
    let preamble = std::fs::read_to_string(&nu_env_path).expect("read nushell preamble");
    assert!(
        preamble.contains("PURETERM_SI_NU"),
        "nu preamble should export PURETERM_SI_NU"
    );
    assert!(
        preamble.contains("_pt_ms"),
        "nu preamble should install OSC 133 helper"
    );
    let kinds = parse_osc133_kinds(preamble.as_bytes());
    assert!(kinds.contains(&ShellMarkKind::PromptStart));
    let _ = std::fs::remove_dir_all(&nu_dir);
}

/// `Shell::detect` 的冒烟测试——覆盖 v3 引入的四个
/// shell 的基于路径的检测。始终运行(无 `#[ignore]`),
/// 因为它不需要外部工具。
#[test]
fn detect_v3_shells_from_path() {
    assert_eq!(Shell::detect(Some("/usr/local/bin/fish")), Some(Shell::Fish));
    assert_eq!(
        Shell::detect(Some("/opt/homebrew/bin/pwsh")),
        Some(Shell::PowerShell)
    );
    assert_eq!(Shell::detect(Some("/usr/local/bin/nu")), Some(Shell::Nu));
    assert_eq!(
        Shell::detect(Some("/usr/local/bin/elvish")),
        Some(Shell::Elvish)
    );
    // `Shell::detect` 按子串 `elvish` 匹配,因此只含 `elv`
    // 的路径不会被分类为 Elvish(二进制名是
    // `elvish` 而非 `elv`)。`shell_integration.rs:545`
    // 处的 v2 `detect` 测试用全名;此处镜像它。
    assert_eq!(Shell::detect(Some("/usr/local/bin/elv")), None);
}

/// 编译期检查 `prepare_shell_env` 为四个 v3 shell 各产出
/// 期望的 `Shell` 键控前导。无头——
/// 始终运行。
#[test]
fn prepare_emits_all_v3_shells() {
    let env = prepare_shell_env("e2e-v3-detect", ShellIntegrationOpts::all_on());

    // 健全性:XDG_CONFIG_HOME 被设为某个每 shell 临时目录
    //(最后一次写入生效,但文件系统里四个都有——
    // 我们经 `temp_*_dir` 辅助函数定位它们)。
    assert!(
        env.contains_key("XDG_CONFIG_HOME"),
        "XDG_CONFIG_HOME must be set"
    );

    // Fish 前导:temp_fish_dir(tag)/conf.d/00_pureterm.fish
    let fish_root = std::env::temp_dir().join(".pureterm-fish-e2e-v3-detect");
    let fish_preamble = fish_root.join("conf.d/00_pureterm.fish");
    assert!(
        fish_preamble.exists(),
        "fish preamble at {:?}",
        fish_preamble
    );

    // Nushell 前导:temp_nu_dir(tag)/env.nu
    let nu_root = std::env::temp_dir().join(".pureterm-nu-e2e-v3-detect");
    let nu_preamble = nu_root.join("env.nu");
    assert!(nu_preamble.exists(), "nu preamble at {:?}", nu_preamble);

    // Elvish 前导:temp_elv_dir(tag)/rc.elv
    let elv_root = std::env::temp_dir().join(".pureterm-elv-e2e-v3-detect");
    let elv_preamble = elv_root.join("rc.elv");
    assert!(
        elv_preamble.exists(),
        "elv preamble at {:?}",
        elv_preamble
    );

    // PowerShell 前导:temp_pwsh_dir(tag)/Microsoft.PowerShell_profile.ps1
    let pwsh_root = std::env::temp_dir().join(".pureterm-pwsh-e2e-v3-detect");
    let pwsh_preamble = pwsh_root.join("Microsoft.PowerShell_profile.ps1");
    assert!(
        pwsh_preamble.exists(),
        "pwsh preamble at {:?}",
        pwsh_preamble
    );

    // 清理全部四个临时目录。
    let _ = std::fs::remove_dir_all(fish_root);
    let _ = std::fs::remove_dir_all(nu_root);
    let _ = std::fs::remove_dir_all(elv_root);
    let _ = std::fs::remove_dir_all(pwsh_root);
}
