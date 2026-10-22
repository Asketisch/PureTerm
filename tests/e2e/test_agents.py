"""Agent 适配器(安装 / 卸载 Claude Code 钩子)的端到端检测。

`pureterm agents install` / `uninstall` 会**改写用户自己的
`~/.claude/settings.json`** —— 这是最容易把用户配置搞坏的路径之一:

- 安装时必须**保留**用户已有的设置(`model`、其它工具的钩子);
- 卸载时必须**只移除** PureTerm 自己装的那一条;
- 重复安装必须幂等(不能装出重复条目);
- `--dry-run` 必须**不写盘**。

这些都不需要 GUI,纯 CLI 即可验证。
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from conftest import Sandbox, run_cli

#: 用户自己的一份 `~/.claude/settings.json`(含自有钩子)。
USER_SETTINGS = {
    "model": "opus",
    "hooks": {
        "PreToolUse": [
            {"hooks": [{"type": "command", "command": "/usr/local/bin/my-own-hook"}]}
        ]
    },
}

#: PureTerm 装入的钩子长这样(命令以 `pureterm hook <Event>` 结尾)。
PURETERM_HOOK_SUFFIX = "hook "


def claude_settings(sandbox: Sandbox) -> Path:
    return sandbox.home / ".claude" / "settings.json"


def install(sandbox: Sandbox, binary: Path, *args: str) -> tuple[int, str, str]:
    result = run_cli(
        ["agents", "install", "claude-code", *args], sandbox, binary, timeout=30
    )
    return result.returncode, result.stdout, result.stderr


def read_settings(sandbox: Sandbox) -> dict:
    path = claude_settings(sandbox)
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def hook_commands(settings: dict) -> list[str]:
    out: list[str] = []
    for entries in settings.get("hooks", {}).values():
        for entry in entries:
            for hook in entry.get("hooks", []):
                out.append(hook.get("command", ""))
    return out


@pytest.fixture
def user_settings(sandbox: Sandbox) -> Path:
    """预置一份用户自己的 Claude Code 配置。"""
    path = claude_settings(sandbox)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(USER_SETTINGS, indent=2), encoding="utf-8")
    return path


# ── 用例 ────────────────────────────────────────────────────────────────────


def test_install_registers_hooks(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """`--all-events` 应注册全部 9 个事件。"""
    code, out, err = install(sandbox, pureterm_bin, "--all-events")
    assert code == 0, err

    settings = read_settings(sandbox)
    events = set(settings.get("hooks", {}))
    assert len(events) == 9, f"期望 9 个事件,实际 {sorted(events)}"
    assert "PreToolUse" in events
    assert "SessionStart" in events


def test_install_preserves_user_settings(
    pureterm_bin: Path, sandbox: Sandbox, user_settings: Path
) -> None:
    """安装不得破坏用户已有的配置。"""
    code, _, err = install(sandbox, pureterm_bin, "--all-events")
    assert code == 0, err

    settings = read_settings(sandbox)
    assert settings.get("model") == "opus", "用户的 model 设置被弄丢了"

    commands = hook_commands(settings)
    assert "/usr/local/bin/my-own-hook" in commands, "用户自己的钩子被弄丢了"
    assert any(PURETERM_HOOK_SUFFIX in c for c in commands), "PureTerm 的钩子没装上"


def test_install_is_idempotent(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """重复安装不应装出重复条目。"""
    install(sandbox, pureterm_bin, "--all-events")
    first = hook_commands(read_settings(sandbox))
    code, _, err = install(sandbox, pureterm_bin, "--all-events")
    assert code == 0, err
    second = hook_commands(read_settings(sandbox))
    assert len(second) == len(first), f"重复安装装出了重复条目:{len(first)} → {len(second)}"


def test_uninstall_removes_only_our_hooks(
    pureterm_bin: Path, sandbox: Sandbox, user_settings: Path
) -> None:
    """卸载只应移除 PureTerm 自己的钩子。"""
    install(sandbox, pureterm_bin, "--all-events")

    result = run_cli(
        ["agents", "uninstall", "claude-code"], sandbox, pureterm_bin, timeout=30
    )
    assert result.returncode == 0, result.stderr

    settings = read_settings(sandbox)
    assert settings.get("model") == "opus", "用户的 model 设置被弄丢了"
    commands = hook_commands(settings)
    assert "/usr/local/bin/my-own-hook" in commands, "用户自己的钩子被误删了"
    assert not any(PURETERM_HOOK_SUFFIX in c for c in commands), (
        f"PureTerm 的钩子没被移除: {commands}"
    )


def test_dry_run_does_not_write(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """`--dry-run` 只预览,不写盘。"""
    code, out, err = install(sandbox, pureterm_bin, "--all-events", "--dry-run")
    assert code == 0, err
    assert not claude_settings(sandbox).exists(), "--dry-run 仍然写了文件"
    assert "PreToolUse" in out or "PreToolUse" in err


def test_install_uses_configured_hook_rules(
    pureterm_bin: Path, sandbox: Sandbox
) -> None:
    """配置了 `[[agent.hook]]` 规则时,只注册这些规则涉及的事件。"""
    sandbox.write_config(
        "[[agent.hook]]\n"
        'event = "PreToolUse"\n'
        'deny_pattern = "rm -rf"\n'
        'deny_reason = "禁止 rm -rf"\n'
    )
    code, _, err = install(sandbox, pureterm_bin)
    assert code == 0, err

    settings = read_settings(sandbox)
    events = set(settings.get("hooks", {}))
    assert events == {"PreToolUse"}, f"只应注册 PreToolUse,实际 {sorted(events)}"


def test_install_without_rules_warns(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """没有配置任何规则时,应给出提示(而不是假装装好了)。"""
    code, _, err = install(sandbox, pureterm_bin)
    assert "no [[agent.hook]] rules" in err or "no [[agent.hook]] rules" in code.to_string()
