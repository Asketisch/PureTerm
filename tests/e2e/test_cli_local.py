"""本地 CLI 命令组端到端测试。

这些命令**不经过 IPC**,只读写本地文件/配置,因此无需启动 GUI,
可以在秒级内跑完全部用例。它们覆盖了 CLI 解析、退出码、JSON 输出
形状、配置文件 CRUD 等最容易出错的接缝。
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pytest

from conftest import Sandbox, run_cli


# ── 顶层解析 ────────────────────────────────────────────────────────────────


def test_help_exits_zero(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["--help"], sandbox, pureterm_bin, check=True)
    assert "GPU-accelerated terminal emulator" in result.stdout


def test_unknown_command_exits_two(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["definitely-not-a-command"], sandbox, pureterm_bin)
    assert result.returncode == 2


def test_unknown_subcommand_exits_two(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["system", "definitely-not-a-subcommand"], sandbox, pureterm_bin)
    assert result.returncode == 2


@pytest.mark.parametrize(
    "group",
    [
        "system",
        "window",
        "tab",
        "pane",
        "surface",
        "config",
        "debug",
        "mcp",
        "events",
        "feed",
        "replay",
        "ssh",
        "daemon",
        "browser",
        "shortcuts",
        "url",
        "plugin",
        "sync",
        "share",
        "update",
        "workflow",
        "notebook",
        "capture",
        "script",
        "tmux",
        "agents",
        "agent",
    ],
)
def test_command_group_help(group: str, pureterm_bin: Path, sandbox: Sandbox) -> None:
    """每个命令组都必须能打印帮助(即 clap 结构完整)。"""
    result = run_cli([group, "--help"], sandbox, pureterm_bin, check=True)
    assert "Usage:" in result.stdout


def test_ipc_commands_fail_without_running_app(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """没有 GUI 实例时,IPC 命令必须以非 0 退出(便于脚本判定失败)。"""
    for args in (
        ["system", "ping"],
        ["system", "info"],
        ["config", "list"],
        ["tab", "list"],
        ["pane", "list"],
        ["window", "list"],
        ["surface", "list"],
    ):
        result = run_cli(args, sandbox, pureterm_bin)
        assert result.returncode != 0, f"`{' '.join(args)}` 在无实例时不应返回 0"
        assert "Cannot connect" in result.stderr


# ── shortcuts ───────────────────────────────────────────────────────────────


def test_shortcuts_list_outputs_bindings(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["shortcuts", "list"], sandbox, pureterm_bin, check=True)
    assert result.stdout.strip(), "shortcuts list 不应为空"
    assert "→" in result.stdout


@pytest.mark.parametrize("preset", ["default", "vim", "tmux", "macos", "vscode", "agent-triage"])
def test_shortcuts_preset_writes_config(preset: str, pureterm_bin: Path, sandbox: Sandbox) -> None:
    """每个预设都应能写入配置并留下 keyboard 段。"""
    result = run_cli(["shortcuts", "preset", preset], sandbox, pureterm_bin, check=True)
    assert preset in result.stderr
    config = sandbox.read_config()
    assert "[keyboard" in config, "预设未写入 keyboard 段"


def test_shortcuts_unknown_preset_fails(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["shortcuts", "preset", "no-such-preset"], sandbox, pureterm_bin)
    assert result.returncode != 0
    assert "Unknown preset" in result.stderr


def test_shortcuts_reset_removes_keyboard_section(pureterm_bin: Path, sandbox: Sandbox) -> None:
    run_cli(["shortcuts", "preset", "vim"], sandbox, pureterm_bin, check=True)
    assert "[keyboard" in sandbox.read_config()
    run_cli(["shortcuts", "reset"], sandbox, pureterm_bin, check=True)
    assert "[keyboard" not in sandbox.read_config()


# ── mcp ─────────────────────────────────────────────────────────────────────


def test_mcp_list_tools_is_tabular(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["mcp", "list-tools"], sandbox, pureterm_bin, check=True)
    assert "TOOL" in result.stdout
    # 关键工具必须在名册里。
    for tool in ("capture_pane", "send_text", "list_surfaces", "new_tab", "split_pane"):
        assert tool in result.stdout, f"MCP 名册缺少 {tool}"


def test_mcp_list_tools_does_not_need_gui(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """`mcp list-tools` 是纯本地命令,不应要求 GUI。"""
    result = run_cli(["mcp", "list-tools"], sandbox, pureterm_bin)
    assert result.returncode == 0
    assert "Cannot connect" not in result.stderr


def test_mcp_install_is_idempotent(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """重复安装不应破坏已有配置,也不应报错。"""
    first = run_cli(["mcp", "install", "claude-code"], sandbox, pureterm_bin)
    second = run_cli(["mcp", "install", "claude-code"], sandbox, pureterm_bin)
    assert first.returncode == 0, first.stderr
    assert second.returncode == 0, second.stderr

    listing = run_cli(["mcp", "list"], sandbox, pureterm_bin, check=True)
    # `mcp list` 列出全部已知客户端;安装状态在 INSTALLED 列。
    line = next(
        (l for l in listing.stdout.splitlines() if "claude-code" in l),
        None,
    )
    assert line is not None, f"mcp list 没有 claude-code 行:\n{listing.stdout}"
    assert " yes " in line or line.split()[1] == "yes", (
        f"安装后 claude-code 的状态应�� yes:\n{line}"
    )


def test_mcp_uninstall_removes_server(pureterm_bin: Path, sandbox: Sandbox) -> None:
    run_cli(["mcp", "install", "claude-code"], sandbox, pureterm_bin, check=True)
    run_cli(["mcp", "uninstall", "claude-code"], sandbox, pureterm_bin, check=True)
    listing = run_cli(["mcp", "list"], sandbox, pureterm_bin, check=True)
    line = next(
        (l for l in listing.stdout.splitlines() if "claude-code" in l),
        None,
    )
    assert line is not None
    assert " no " in line or line.split()[1] == "no", (
        f"卸载后 claude-code 的状态应为 no:\n{line}"
    )


def test_mcp_install_writes_into_cwd(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """Claude Code 适配器写的是**项目级** `.mcp.json`(当前目录)。

    这一点很重要:测试必须在自己的沙箱目录里跑,否则会往用户的
    代码仓库里丢一个 `.mcp.json`。
    """
    run_cli(["mcp", "install", "claude-code"], sandbox, pureterm_bin, check=True)
    target = sandbox.root / ".mcp.json"
    assert target.exists(), ".mcp.json 没有写到当前目录"
    data = json.loads(target.read_text(encoding="utf-8"))
    assert "pureterm" in data["mcpServers"]
    assert data["mcpServers"]["pureterm"]["args"] == ["mcp", "serve"]


# ── events / feed / replay ──────────────────────────────────────────────────


def test_events_list_on_empty_log(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """空事件日志不应崩溃,应给出友好提示。"""
    result = run_cli(["events", "list"], sandbox, pureterm_bin, check=True)
    assert "no events" in result.stdout.lower() or "SEQ" in result.stdout


def test_feed_list_on_empty_log(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["feed", "list"], sandbox, pureterm_bin, check=True)
    assert result.stdout.strip()


def test_replay_list_on_empty_log(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["replay", "list"], sandbox, pureterm_bin, check=True)
    assert result.stdout.strip()


# ── url ─────────────────────────────────────────────────────────────────────


def test_url_parse_is_json(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["url", "parse", "pureterm://tab/new"], sandbox, pureterm_bin, check=True)
    data = json.loads(result.stdout)
    assert isinstance(data, dict)
    assert "action" in data or "kind" in data


def test_url_parse_rejects_garbage(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["url", "parse", "not-a-pureterm-url"], sandbox, pureterm_bin)
    assert result.returncode != 0


# ── plugin ──────────────────────────────────────────────────────────────────


def test_plugin_install_list_uninstall_cycle(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """插件系统默认关闭,需先在配置里开启。"""
    sandbox.write_config("[plugins]\nenabled = true\n")
    install = run_cli(["plugin", "install", "demo-plugin"], sandbox, pureterm_bin)
    assert install.returncode == 0, install.stderr

    listing = run_cli(["plugin", "list"], sandbox, pureterm_bin, check=True)
    assert "demo-plugin" in listing.stdout

    uninstall = run_cli(["plugin", "uninstall", "demo-plugin"], sandbox, pureterm_bin)
    assert uninstall.returncode == 0, uninstall.stderr

    listing = run_cli(["plugin", "list"], sandbox, pureterm_bin, check=True)
    assert "demo-plugin" not in listing.stdout


def test_plugin_disabled_by_default(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """未开启时插件命令必须明确拒绝,而不是静默成功。"""
    result = run_cli(["plugin", "install", "demo-plugin"], sandbox, pureterm_bin)
    assert result.returncode != 0
    assert "disabled" in result.stderr


def test_plugin_enable_disable_toggles_manifest(pureterm_bin: Path, sandbox: Sandbox) -> None:
    sandbox.write_config("[plugins]\nenabled = true\n")
    run_cli(["plugin", "install", "toggle-plugin"], sandbox, pureterm_bin, check=True)
    disable = run_cli(["plugin", "disable", "toggle-plugin"], sandbox, pureterm_bin, check=True)
    assert disable.returncode == 0, disable.stderr
    listing = run_cli(["plugin", "list"], sandbox, pureterm_bin, check=True)
    assert "toggle-plugin" in listing.stdout


# ── workflow / notebook ─────────────────────────────────────────────────────


def test_workflow_list_on_empty_config(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["workflow", "list"], sandbox, pureterm_bin, check=True)
    assert result.stdout.strip()


def test_notebook_new_and_run(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """新建 notebook → 跑第一个单元格 → 拿到 stdout。"""
    sandbox.write_config("[notebooks]\nenabled = true\n")
    nb = sandbox.root / "demo.ipynb"
    created = run_cli(["notebook", "new", str(nb)], sandbox, pureterm_bin)
    assert created.returncode == 0, created.stderr
    assert nb.exists()

    # 塞一个代码单元,验证 `notebook run` 能执行并捕获输出。
    # PureTerm 的 notebook 是自有格式(不是 Jupyter):单元格用
    # `type` 标签区分 code / markdown / output,代码由用户的 $SHELL 执行。
    notebook = {
        "title": "demo",
        "cells": [
            {
                "type": "code",
                "language": "sh",
                "source": "echo hello-from-notebook",
            }
        ],
    }
    nb.write_text(json.dumps(notebook), encoding="utf-8")

    run = run_cli(["notebook", "run", str(nb), "0"], sandbox, pureterm_bin)
    assert run.returncode == 0, run.stderr
    assert "hello-from-notebook" in run.stdout


def test_notebook_show_lists_cells(pureterm_bin: Path, sandbox: Sandbox) -> None:
    sandbox.write_config("[notebooks]\nenabled = true\n")
    nb = sandbox.root / "demo.ipynb"
    notebook = {
        "title": "demo",
        "cells": [
            {"type": "markdown", "text": "# 标题"},
            {"type": "code", "language": "sh", "source": "echo hi"},
        ],
    }
    nb.write_text(json.dumps(notebook), encoding="utf-8")
    result = run_cli(["notebook", "show", str(nb)], sandbox, pureterm_bin, check=True)
    assert "markdown" in result.stdout
    assert "code" in result.stdout


def test_notebooks_disabled_by_default(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["notebook", "new", str(sandbox.root / "x.ipynb")], sandbox, pureterm_bin)
    assert result.returncode != 0
    assert "disabled" in result.stderr


# ── agents ──────────────────────────────────────────────────────────────────


def test_agents_list_reports_adapters(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["agents", "list"], sandbox, pureterm_bin, check=True)
    assert result.stdout.strip()


def test_agents_registry_is_json(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """配置了 `[[vault.agents]]` 后,registry 应打印 JSON 数组。"""
    sandbox.write_config(
        "[[vault.agents]]\n"
        'kind = "claude-code"\n'
        'detect = "claude"\n'
        'resume_command = ["claude", "--resume", "{session_id}"]\n'
        "never_hibernate = true\n"
    )
    result = run_cli(["agents", "registry"], sandbox, pureterm_bin, check=True)
    output = result.stdout
    assert "claude-code" in output, f"registry 没有列出 kind:\n{output}"
    assert "detect=claude" in output, f"registry 没有列出 detect:\n{output}"
    assert "never_hibernate=true" in output, f"registry 没有列出 never_hibernate:\n{output}"
    assert "resume=claude --resume {session_id}" in output, (
        f"registry 没有列出 resume_command:\n{output}"
    )


def test_agents_registry_empty_is_friendly(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """没有配置时给出友好提示,而不是空输出或崩溃。"""
    result = run_cli(["agents", "registry"], sandbox, pureterm_bin, check=True)
    assert "no [[vault.agents]] entries" in result.stdout


# ── hook ────────────────────────────────────────────────────────────────────


def test_hook_shim_allows_by_default(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """没有配置任何规则时,hook 垫片放行(exit 0,无输出)。

    Claude Code 的约定:退出码 0 且 stdout 为空 = 放行。
    """
    payload = json.dumps(
        {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo hi"},
        }
    )
    result = run_cli(["hook"], sandbox, pureterm_bin, stdin=payload)
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "", "放行时不应有任何输出"


def test_hook_shim_denies_matching_pattern(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """命中 `deny_pattern` 时输出 Claude Code 协议的 deny 决策。"""
    sandbox.write_config(
        "[[agent.hook]]\n"
        'event = "PreToolUse"\n'
        'deny_pattern = "rm -rf"\n'
        'deny_reason = "禁止 rm -rf"\n'
    )
    payload = json.dumps(
        {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf /"},
        }
    )
    result = run_cli(["hook"], sandbox, pureterm_bin, stdin=payload)
    assert result.returncode == 0, result.stderr
    decision = json.loads(result.stdout)
    output = decision["hookSpecificOutput"]
    assert output["hookEventName"] == "PreToolUse"
    assert output["permissionDecision"] == "deny"
    assert "rm -rf" in output["permissionDecisionReason"]


def test_hook_shim_allows_non_matching_pattern(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """不命中时放行。"""
    sandbox.write_config(
        "[[agent.hook]]\n"
        'event = "PreToolUse"\n'
        'deny_pattern = "rm -rf"\n'
        'deny_reason = "禁止 rm -rf"\n'
    )
    payload = json.dumps(
        {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "echo hello"},
        }
    )
    result = run_cli(["hook"], sandbox, pureterm_bin, stdin=payload)
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == ""


def test_hook_shim_rejects_invalid_stdin(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """非法 stdin JSON 必须以非 0 退出并给出错误,而不是静默放行。"""
    result = run_cli(["hook"], sandbox, pureterm_bin, stdin="not json at all")
    assert result.returncode != 0
    assert "invalid stdin JSON" in result.stderr


# ── sync ────────────────────────────────────────────────────────────────────


def test_sync_push_creates_package(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """云同步默认关闭;开启后应能导出本地同步包。"""
    sandbox.write_config("[cloud_sync]\nenabled = true\n")
    result = run_cli(["sync", "push"], sandbox, pureterm_bin)
    assert result.returncode == 0, result.stderr


def test_sync_disabled_by_default(pureterm_bin: Path, sandbox: Sandbox) -> None:
    result = run_cli(["sync", "push"], sandbox, pureterm_bin)
    assert result.returncode != 0
    assert "disabled" in result.stderr


# ── shell 脚本类命令(不依赖 GUI)──────────────────────────────────────────


def test_update_check_local(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """不带 --remote 的更新检查是本地的,不应访问网络。"""
    result = run_cli(["update", "check"], sandbox, pureterm_bin)
    assert result.returncode in (0, 1), result.stderr
