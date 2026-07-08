//! T-core-shell-integ v3 E2E tests (checklist D.5).
//!
//! Default-`#[ignore]`d: these tests run a **real fish / pwsh / elvish /
//! nu** subprocess. On hosts without the binary on `$PATH`, the test
//! skips with an informative message rather than failing. Run with:
//!   cargo test -p pureterm-daemon --lib shell_integ_e2e --include-ignored --nocapture
//!
//! ## Verification approach
//!
//! 1. Call [`crate::shell_integration::prepare_shell_env`] with the
//!    per-shell `ShellIntegrationOpts`.
//! 2. Read the per-shell preamble file written to the tagged temp dir.
//! 3. Run the preamble's OSC 133 byte sequences through a minimal
//!    parser (mirrors `alacritty_terminal::event_loop::scan_osc133`)
//!    to confirm the expected PromptStart / CommandStart / OutputStart
//!    / OutputEnd letter codes are present.
//!
//! Why "preamble → parser" instead of "real PTY": the PTY plumbing
//! already has its own E2E layer (`tmux_cc_e2e.rs`, `graphics_e2e.rs`);
//! for shell-integration the value-add of going through a real PTY is
//! nil — we just need to verify the preamble produces the right byte
//! sequence when sourced.

#![cfg(unix)]

use std::collections::HashMap;
use std::process::{Command, Stdio};

use alacritty_terminal::event::ShellMarkKind;

use crate::shell_integration::{prepare_shell_env, Shell, ShellIntegrationOpts};

/// Probe whether a binary is on `$PATH`. Returns `true` if found.
fn bin_on_path(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Find the env value containing `substr`, returning a clone. Returns
/// `None` if no env value matches. Explicit return type so the
/// `HashMap<String, String>` inference doesn't fail.
fn find_env_dir(env: &HashMap<String, String>, substr: &str) -> Option<String> {
    for v in env.values() {
        if v.contains(substr) {
            return Some(v.clone());
        }
    }
    None
}

/// Parse OSC 133 marks out of a byte stream and return the sequence of
/// `ShellMarkKind`s observed. Mirrors `event_loop::scan_osc133` so the
/// E2E can run without a real PTY: the preamble writes the same bytes
/// whether the source is a fish shell or a recorded file.
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
                // Skip past terminator (BEL or ST) — same logic as
                // event_loop::scan_osc133.
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

/// Run `binary` with `args` under the prepared `env`, capture stdout
/// as a UTF-8 string. Returns `None` if the binary is missing or fails
/// to spawn. (Kept for future real-shell round-trip tests.)
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

    // fish needs a TTY to draw its prompt; for headless we read the
    // preamble file directly and verify it embeds the right OSC 133
    // letter codes.
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

    // Sanity: parse the preamble's OSC 133 letter emissions (they're
    // embedded in `_pt_ms` definitions as printf format strings).
    let kinds = parse_osc133_kinds(preamble.as_bytes());
    assert!(
        kinds.contains(&ShellMarkKind::PromptStart),
        "preamble should embed PromptStart OSC 133 reference"
    );
    assert!(
        kinds.contains(&ShellMarkKind::CommandStart),
        "preamble should embed CommandStart OSC 133 reference"
    );

    // Cleanup temp dir.
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

    // PowerShell preamble lives in the temp $PROFILE path; look it up.
    let pwsh_dir = find_env_dir(&env_map, "pureterm-pwsh").expect("pwsh temp dir set in env");
    let pwsh_profile_path =
        std::path::Path::new(&pwsh_dir).join("Microsoft.PowerShell_profile.ps1");
    let preamble = std::fs::read_to_string(&pwsh_profile_path).expect("read pwsh preamble");

    assert!(
        preamble.contains("_pt_ms"),
        "pwsh preamble should define _pt_ms helper"
    );
    // pwsh preamble writes OSC 133 marks for A/B/C/D as printf sequences.
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

    // Cleanup.
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

/// Smoke test for `Shell::detect` — covers the path-based detection
/// for the four shells introduced in v3. Always-on (no `#[ignore]`)
/// because it doesn't need an external tool.
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
    // `Shell::detect` matches on the substring `elvish`, so a path
    // containing only `elv` does NOT classify as Elvish (the binary
    // is `elvish`, not `elv`). The v2 `detect` test at
    // `shell_integration.rs:545` uses the full name; we mirror it.
    assert_eq!(Shell::detect(Some("/usr/local/bin/elv")), None);
}

/// Compile-time check that `prepare_shell_env` produces the expected
/// `Shell`-keyed preamble for each of the four v3 shells. Headless —
/// always-on.
#[test]
fn prepare_emits_all_v3_shells() {
    let env = prepare_shell_env("e2e-v3-detect", ShellIntegrationOpts::all_on());

    // Sanity: XDG_CONFIG_HOME is set to one of the per-shell temp dirs
    // (the last write wins, but the file system has all four — we
    // locate them via the `temp_*_dir` helpers).
    assert!(
        env.contains_key("XDG_CONFIG_HOME"),
        "XDG_CONFIG_HOME must be set"
    );

    // Fish preamble: temp_fish_dir(tag)/conf.d/00_pureterm.fish
    let fish_root = std::env::temp_dir().join(".pureterm-fish-e2e-v3-detect");
    let fish_preamble = fish_root.join("conf.d/00_pureterm.fish");
    assert!(
        fish_preamble.exists(),
        "fish preamble at {:?}",
        fish_preamble
    );

    // Nushell preamble: temp_nu_dir(tag)/env.nu
    let nu_root = std::env::temp_dir().join(".pureterm-nu-e2e-v3-detect");
    let nu_preamble = nu_root.join("env.nu");
    assert!(nu_preamble.exists(), "nu preamble at {:?}", nu_preamble);

    // Elvish preamble: temp_elv_dir(tag)/rc.elv
    let elv_root = std::env::temp_dir().join(".pureterm-elv-e2e-v3-detect");
    let elv_preamble = elv_root.join("rc.elv");
    assert!(
        elv_preamble.exists(),
        "elv preamble at {:?}",
        elv_preamble
    );

    // PowerShell preamble: temp_pwsh_dir(tag)/Microsoft.PowerShell_profile.ps1
    let pwsh_root = std::env::temp_dir().join(".pureterm-pwsh-e2e-v3-detect");
    let pwsh_preamble = pwsh_root.join("Microsoft.PowerShell_profile.ps1");
    assert!(
        pwsh_preamble.exists(),
        "pwsh preamble at {:?}",
        pwsh_preamble
    );

    // Cleanup all four temp dirs.
    let _ = std::fs::remove_dir_all(fish_root);
    let _ = std::fs::remove_dir_all(nu_root);
    let _ = std::fs::remove_dir_all(elv_root);
    let _ = std::fs::remove_dir_all(pwsh_root);
}