"""IPC 命令组端到端测试(需要运行中的 PureTerm GUI 实例)。

这些用例覆盖 Socket API 的主要读写路径:系统信息、窗口 / 标签页 /
窗格操作、surface 读写、配置读写、事件日志、tmux 兼容子集与
``script`` 自动化入口。fixture ``app`` 会启动一个隔离实例并在
用例结束后销毁。
"""

from __future__ import annotations

import time
from pathlib import Path

import pytest

from conftest import AppInstance


# ── system ──────────────────────────────────────────────────────────────────


def test_system_ping(app: AppInstance) -> None:
    data = app.json_cli(["system", "ping"])
    assert data == {"status": "ok"}


def test_system_info_shape(app: AppInstance) -> None:
    data = app.json_cli(["system", "info"])
    assert isinstance(data, dict)
    for key in ("os", "version", "tabs", "panes", "windows"):
        assert key in data, f"system info 缺少 {key}"
    assert data["windows"] >= 1
    assert data["tabs"] >= 1
    assert data["panes"] >= 1


def test_system_version(app: AppInstance) -> None:
    result = app.cli(["system", "version"], check=True)
    assert result.stdout.strip()


def test_system_methods_lists_rpc_methods(app: AppInstance) -> None:
    result = app.cli(["system", "methods"], check=True)
    for method in ("system.ping", "tab.list", "pane.list", "surface.list"):
        assert method in result.stdout, f"methods 列表缺少 {method}"


# ── window / tab / pane ─────────────────────────────────────────────────────


def test_window_list_has_main_window(app: AppInstance) -> None:
    data = app.json_cli(["window", "list"])
    assert isinstance(data, list)
    assert len(data) >= 1


def test_tab_list_shape(app: AppInstance) -> None:
    data = app.json_cli(["tab", "list"])
    assert isinstance(data, list)
    assert len(data) >= 1
    assert "index" in data[0]
    assert "panes" in data[0]


def test_pane_list_shape(app: AppInstance) -> None:
    data = app.json_cli(["pane", "list"])
    assert isinstance(data, list)
    assert len(data) >= 1
    assert "pane_id" in data[0]


def test_tab_new_increases_count(app: AppInstance) -> None:
    before = len(app.json_cli(["tab", "list"]))
    app.cli(["tab", "new"], check=True)
    # IPC 是异步生效的,给事件循环一点时间。
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        after = len(app.json_cli(["tab", "list"]))
        if after > before:
            break
        time.sleep(0.1)
    assert after > before, "新标签页没有出现"


def test_pane_split_and_close(app: AppInstance) -> None:
    before = len(app.json_cli(["pane", "list"]))
    app.cli(["pane", "split", "--direction", "vertical"], check=True)

    deadline = time.monotonic() + 10
    after = before
    while time.monotonic() < deadline:
        after = len(app.json_cli(["pane", "list"]))
        if after > before:
            break
        time.sleep(0.1)
    assert after > before, "分割后窗格数没有增加"

    app.cli(["pane", "close"], check=True)
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if len(app.json_cli(["pane", "list"])) <= after - 1:
            break
        time.sleep(0.1)
    assert len(app.json_cli(["pane", "list"])) <= after - 1, "关闭后窗格数没有减少"


def test_pane_zoom_toggles(app: AppInstance) -> None:
    app.cli(["pane", "zoom"], check=True)
    app.cli(["pane", "zoom"], check=True)


def test_pane_focus_cycles(app: AppInstance) -> None:
    app.cli(["pane", "focus", "--direction", "next"], check=True)
    app.cli(["pane", "focus", "--direction", "prev"], check=True)


# ── surface ─────────────────────────────────────────────────────────────────


def test_surface_list_shape(app: AppInstance) -> None:
    data = app.json_cli(["surface", "list"])
    assert isinstance(data, list)
    assert len(data) >= 1


def test_surface_send_text_and_snapshot(app: AppInstance) -> None:
    """往 PTY 写一段唯一的标记文本,再从 surface 快照里读回来。"""
    marker = "PURETERM_E2E_MARKER_9F3A"
    app.cli(["surface", "send-text", "--text", marker], check=True)

    deadline = time.monotonic() + 15
    seen = False
    while time.monotonic() < deadline:
        result = app.cli(["surface", "snapshot"], check=True)
        if marker in result.stdout:
            seen = True
            break
        time.sleep(0.2)
    assert seen, f"surface 快照里始终没有出现 {marker}"


def test_surface_snapshot_returns_text(app: AppInstance) -> None:
    result = app.cli(["surface", "snapshot"], check=True)
    assert isinstance(result.stdout, str)


# ── config ──────────────────────────────────────────────────────────────────


def test_config_list_shows_keys(app: AppInstance) -> None:
    result = app.cli(["config", "list"], check=True)
    assert result.stdout.strip()


def test_config_get_reads_a_key(app: AppInstance) -> None:
    result = app.cli(["config", "get", "--key", "font.size"], check=True)
    assert result.stdout.strip()


def test_config_reload_is_accepted(app: AppInstance) -> None:
    app.cli(["config", "reload"], check=True)


# ── debug ───────────────────────────────────────────────────────────────────


def test_debug_dump_state(app: AppInstance) -> None:
    result = app.cli(["debug", "dump-state"], check=True)
    assert result.stdout.strip()


# ── 原始 JSON-RPC 逃生口 ───────────────────────────────────────────────────


def test_msg_raw_rpc(app: AppInstance) -> None:
    data = app.json_cli(["msg", "system.ping"])
    assert data == {"status": "ok"}


def test_msg_unknown_method_fails(app: AppInstance) -> None:
    result = app.cli(["msg", "definitely.not.a.method"])
    assert result.returncode != 0


# ── notify ──────────────────────────────────────────────────────────────────


def test_notify_posts_notification(app: AppInstance) -> None:
    app.cli(["notify", "e2e-notification"], check=True)
    # 通知会写入事件日志,稍后由 events 用例验证。
    time.sleep(0.5)


# ── events ──────────────────────────────────────────────────────────────────


def test_events_log_records_lifecycle(app: AppInstance) -> None:
    """应用启动后,事件日志应包含 app.started / window.opened。"""
    deadline = time.monotonic() + 10
    kinds: list[str] = []
    while time.monotonic() < deadline:
        result = app.cli(["events", "list"], check=True)
        kinds = [line.split()[-2] for line in result.stdout.splitlines() if len(line.split()) >= 4]
        if "app.started" in kinds:
            break
        time.sleep(0.3)
    assert "app.started" in kinds, f"事件日志里没有 app.started,实际有: {kinds[:10]}"
    assert "window.opened" in kinds, f"事件日志里没有 window.opened,实际有: {kinds[:10]}"


def test_events_tail_does_not_hang_forever(app: AppInstance) -> None:
    """`events tail` 是跟随式命令,应当能被中断(用超时模拟 Ctrl-C)。"""
    import subprocess

    proc = subprocess.Popen(
        [str(app.binary), "events", "tail"],
        env=app.sandbox.env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    time.sleep(1.5)
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)
        pytest.fail("events tail 在 SIGTERM 后没有退出")


# ── script(AppleScript 友好入口)──────────────────────────────────────────


def test_script_new_tab(app: AppInstance) -> None:
    before = len(app.json_cli(["tab", "list"]))
    app.cli(["script", "new-tab"], check=True)
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if len(app.json_cli(["tab", "list"])) > before:
            return
        time.sleep(0.1)
    pytest.fail("script new-tab 没有创建新标签页")


def test_script_send_text_and_get_cwd(app: AppInstance) -> None:
    app.cli(["script", "send-text", "--text", "echo SCRIPT_E2E"], check=True)
    result = app.cli(["script", "get-cwd"], check=True)
    assert result.stdout.strip(), "get-cwd 没有输出"


# ── tmux 兼容子集 ───────────────────────────────────────────────────────────


def test_tmux_list_panes(app: AppInstance) -> None:
    result = app.cli(["tmux", "list-panes"], check=True)
    assert result.stdout.strip()


def test_tmux_capture_pane(app: AppInstance) -> None:
    result = app.cli(["tmux", "capture-pane"], check=True)
    assert result.stdout.strip() is not None


def test_tmux_send_keys(app: AppInstance) -> None:
    result = app.cli(["tmux", "send-keys", "echo TMUX_E2E", "Enter"], check=True)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        out = app.cli(["tmux", "capture-pane"], check=True).stdout
        if "TMUX_E2E" in out:
            return
        time.sleep(0.2)
    pytest.fail("tmux send-keys 的内容没有出现在 capture-pane 里")


# ── capture export(面板快照导出)──────────────────────────────────────────


def test_capture_export_writes_text_file(app: AppInstance, sandbox) -> None:
    """`capture export` 必须**真的把文件写出来**。

    回归背景:早前它只发 RPC、打印快照,随后打印一句 “wiring
    deferred” 就返回 —— 文件根本没生成,而命令看上去是成功的。
    """
    out = sandbox.root / "snap.txt"
    result = app.cli(["capture", "export", "--output", str(out)], timeout=30)
    assert result.returncode == 0, result.stderr
    assert out.exists(), f"导出文件没有生成: {out}"
    content = out.read_text(encoding="utf-8")
    assert content.strip(), "导出的快照是空的"


def test_capture_export_writes_svg_file(app: AppInstance, sandbox) -> None:
    """`--format svg` 应产出合法 SVG。"""
    out = sandbox.root / "snap.svg"
    result = app.cli(
        ["capture", "export", "--output", str(out), "--format", "svg"], timeout=30
    )
    assert result.returncode == 0, result.stderr
    assert out.exists(), f"导出文件没有生成: {out}"
    content = out.read_text(encoding="utf-8")
    assert content.lstrip().startswith("<?xml"), "不是合法的 SVG"
    assert "<svg" in content


def test_capture_export_png_reports_honestly(app: AppInstance, sandbox) -> None:
    """PNG 还没接线 —— 必须**明确报错**,不能假装成功。"""
    out = sandbox.root / "snap.png"
    result = app.cli(
        ["capture", "export", "--output", str(out), "--format", "png"], timeout=30
    )
    assert result.returncode != 0, "PNG 未接线时应以非 0 退出"
    assert "PNG" in result.stderr or "png" in result.stderr.lower()


def test_capture_export_respects_disabled_config(app_factory, sandbox) -> None:
    """`[pane_capture].enabled = false` 应真的关掉导出。

    回归背景:该配置项之前被读出来就扔掉(`_cfg`),开关形同虚设。
    """
    app = app_factory("[pane_capture]\nenabled = false\n")
    out = sandbox.root / "snap.txt"
    result = app.cli(["capture", "export", "--output", str(out)], timeout=30)
    assert result.returncode != 0, "enabled = false 时不应允许导出"
    assert not out.exists(), "enabled = false 时不应产生文件"


def test_capture_export_uses_configured_default_format(app_factory, sandbox) -> None:
    """省略 `--format` 时应采用 `[pane_capture].default_format`。"""
    app = app_factory('[pane_capture]\nenabled = true\ndefault_format = "svg"\n')
    out = sandbox.root / "snap-default.svg"
    result = app.cli(["capture", "export", "--output", str(out)], timeout=30)
    assert result.returncode == 0, result.stderr
    assert out.exists(), "没有按配置的 default_format 导出"
    assert "<svg" in out.read_text(encoding="utf-8")
