"""原始 JSON-RPC 方法级端到端测试。

CLI 只覆盖了 Socket API 的一部分;这里直接用 `pureterm msg <method>`
打到每一个已注册的方法上,验证:

- 方法确实存在(未被误删);
- 返回值形状符合契约;
- 非法参数返回 JSON-RPC error 而不是 panic 或空响应;
- 只读方法不改变应用状态,写方法确实生效。
"""

from __future__ import annotations

import time

import pytest

from conftest import AppInstance

#: 只读方法:调用不应改变应用状态。
READ_ONLY_METHODS = [
    "system.ping",
    "system.info",
    "system.version",
    "system.methods",
    "window.list",
    "tab.list",
    "pane.list",
    "surface.list",
    "config.list",
    "config.get",
    "debug.ping",
    "debug.dump_state",
    "workspace.list",
    "trigger.list",
    "shell.context",
    "ssh.list_profiles",
]


def rpc(app: AppInstance, method: str, params: dict | None = None) -> object:
    """调用一个 JSON-RPC 方法,返回解析后的 result。"""
    args = ["msg", method]
    if params is not None:
        import json as _json

        args.append(_json.dumps(params))
    return app.json_cli(args)


# ── 覆盖面 ──────────────────────────────────────────────────────────────────


@pytest.mark.parametrize("method", READ_ONLY_METHODS)
def test_read_only_method_responds(app: AppInstance, method: str) -> None:
    """每个只读方法都必须返回结构化结果,不能报错或返回空。"""
    result = rpc(app, method)
    assert result is not None, f"{method} 没有返回结果"


def test_system_methods_matches_documented_surface(app: AppInstance) -> None:
    """`system.methods` 应列出全部已注册方法。"""
    result = rpc(app, "system.methods")
    assert isinstance(result, dict)
    names = set(result["methods"])
    assert result["count"] == len(names), "count 与 methods 长度不一致"
    for expected in ("system.ping", "tab.list", "pane.list", "surface.list"):
        assert expected in names, f"system.methods 缺少 {expected}"


def test_config_list_known_keys(app: AppInstance) -> None:
    result = rpc(app, "config.list")
    assert "keys" in result
    keys = result["keys"]
    assert "font.normal.family" in keys
    assert "font.size" in keys


def test_config_get_with_unknown_key_is_graceful(app: AppInstance) -> None:
    """未知配置键不应让 RPC 崩掉。"""
    result = rpc(app, "config.get", {"key": "no.such.key"})
    assert result is not None


def test_trigger_list_shape(app: AppInstance) -> None:
    result = rpc(app, "trigger.list")
    assert "triggers" in result
    assert "count" in result
    assert result["count"] == len(result["triggers"])


def test_shell_context_shape(app: AppInstance) -> None:
    result = rpc(app, "shell.context")
    for key in ("pane_id", "integrated", "cwd", "last_command", "recent_commands"):
        assert key in result, f"shell.context 缺少 {key}"


def test_workspace_list_shape(app: AppInstance) -> None:
    result = rpc(app, "workspace.list")
    assert isinstance(result, list)
    assert result, "至少应有一个工作区"
    assert "index" in result[0]
    assert "id" in result[0]


def test_surface_list_shape(app: AppInstance) -> None:
    result = rpc(app, "surface.list")
    assert isinstance(result, list)
    assert result, "至少应有一个 surface"
    assert "pane_id" in result[0]


def test_pane_list_shape(app: AppInstance) -> None:
    result = rpc(app, "pane.list")
    assert isinstance(result, list)
    assert "pane_id" in result[0]


def test_tab_list_shape(app: AppInstance) -> None:
    result = rpc(app, "tab.list")
    assert isinstance(result, list)
    assert "index" in result[0]
    assert "panes" in result[0]


def test_window_list_shape(app: AppInstance) -> None:
    result = rpc(app, "window.list")
    assert isinstance(result, list)
    assert result


# ── 错误路径 ────────────────────────────────────────────────────────────────


def test_unknown_method_returns_error(app: AppInstance) -> None:
    result = app.cli(["msg", "no.such.method"])
    assert result.returncode != 0
    assert "Error" in result.stderr


def test_invalid_params_returns_error(app: AppInstance) -> None:
    """参数非法时应返回 JSON-RPC error,而不是 panic。"""
    result = app.cli(["msg", "pane.focus", '{"direction":"sideways"}'])
    assert result.returncode != 0, "非法参数不应被接受"


def test_malformed_params_json_returns_error(app: AppInstance) -> None:
    result = app.cli(["msg", "pane.focus", "{not json"])
    assert result.returncode != 0


def test_ssh_connect_without_profiles_errors(app: AppInstance) -> None:
    """没有配置 SSH 档案时,`ssh.connect` 必须报明确错误。"""
    result = app.cli(["msg", "ssh.connect", '{"index":0}'])
    assert result.returncode != 0
    assert "SSH" in result.stderr or "ssh" in result.stderr.lower()


# ── 写路径 ──────────────────────────────────────────────────────────────────


def test_tab_new_then_list_reflects_change(app: AppInstance) -> None:
    before = len(rpc(app, "tab.list"))
    rpc(app, "tab.new")
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if len(rpc(app, "tab.list")) > before:
            break
        time.sleep(0.1)
    assert len(rpc(app, "tab.list")) > before


def test_pane_split_then_list_reflects_change(app: AppInstance) -> None:
    before = len(rpc(app, "pane.list"))
    rpc(app, "pane.split", {"direction": "vertical"})
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if len(rpc(app, "pane.list")) > before:
            break
        time.sleep(0.1)
    assert len(rpc(app, "pane.list")) > before


def test_config_reload_side_effect_is_accepted(app: AppInstance) -> None:
    result = rpc(app, "config.reload")
    assert result.get("reloaded") is True


def test_notification_post_is_accepted(app: AppInstance) -> None:
    result = rpc(app, "notification.post", {"body": "rpc-e2e"})
    assert result is not None


def test_agent_session_started_is_accepted(app: AppInstance) -> None:
    result = rpc(app, "agent.session_started", {"session_id": "e2e-session"})
    assert result.get("ok") is True


def test_surface_send_text_and_snapshot_via_rpc(app: AppInstance) -> None:
    marker = "RPC_E2E_MARKER_7C1"
    rpc(app, "surface.send_text", {"text": marker})

    deadline = time.monotonic() + 20
    seen = False
    while time.monotonic() < deadline:
        result = rpc(app, "surface.snapshot")
        text = result.get("text", "") if isinstance(result, dict) else ""
        if marker in text:
            seen = True
            break
        time.sleep(0.25)
    assert seen, "surface.snapshot 没有读到 send_text 写入的内容"
