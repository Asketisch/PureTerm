"""守护进程(puretermd)端到端测试。

守护进程负责让终端会话在 GUI 重启后存活。测试覆盖:

- 守护进程能启动、能响应 `daemon.ping` / `daemon.status`;
- 能在守护进程里创建会话、列出会话、杀掉会话;
- `pureterm daemon status` 的退出码正确(可达 → 0,不可达 → 1);
- **GUI 冷启动时必须真的连上守护进程** —— 否则应用会静默退回
  直连 PTY,会话持久化随之失效(这曾经是一个真实缺陷)。

会话 RPC 直接走守护进程的控制 socket($TMPDIR/puretermd.sock),
不经过 GUI。
"""

from __future__ import annotations

import json
import socket as socket_mod
import subprocess
import time
from pathlib import Path
from typing import Any

import pytest

from conftest import AppInstance, Sandbox, run_cli


def daemon_rpc(
    socket_path: Path, method: str, params: Any = None, timeout: float = 10.0
) -> dict:
    """直接向守护进程控制 socket 发一条 JSON-RPC。"""
    request: dict[str, Any] = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        request["params"] = params
    with socket_mod.socket(socket_mod.AF_UNIX, socket_mod.SOCK_STREAM) as sock:
        sock.settimeout(timeout)
        sock.connect(str(socket_path))
        sock.sendall((json.dumps(request) + "\n").encode())
        sock.shutdown(socket_mod.SHUT_WR)
        chunks = []
        while True:
            data = sock.recv(65536)
            if not data:
                break
            chunks.append(data)
    raw = b"".join(chunks).decode()
    return json.loads(raw.strip().splitlines()[0])


def daemon_ping(socket_path: Path, timeout: float = 3.0) -> bool:
    """守护进程控制 socket 是否还应答 `daemon.ping`。"""
    try:
        response = daemon_rpc(socket_path, "daemon.ping", timeout=timeout)
    except (OSError, ValueError, IndexError):
        return False
    return response.get("error") is None


# ── 基本可达性 ──────────────────────────────────────────────────────────────


def test_daemon_responds_to_ping(daemon, sandbox: Sandbox) -> None:
    response = daemon_rpc(sandbox.daemon_socket, "daemon.ping")
    assert response.get("error") is None, f"daemon.ping 失败: {response}"
    assert response["result"].get("ok") is True


def test_daemon_status_reports_pid(daemon, sandbox: Sandbox) -> None:
    response = daemon_rpc(sandbox.daemon_socket, "daemon.status")
    assert response.get("error") is None, f"daemon.status 失败: {response}"
    result = response["result"]
    assert "pid" in result
    assert "sessions" in result
    assert "exe_mtime" in result, "守护进程必须报告 exe_mtime(陈旧性检测依赖它)"


def test_daemon_status_cli_exit_codes(
    daemon, sandbox: Sandbox, pureterm_bin: Path
) -> None:
    """守护进程可达时 `daemon status` 退出 0。"""
    result = run_cli(["daemon", "status"], sandbox, pureterm_bin)
    assert result.returncode == 0, result.stderr
    assert "daemon reachable    : true" in result.stdout


def test_daemon_status_cli_fails_when_down(
    sandbox: Sandbox, pureterm_bin: Path
) -> None:
    """守护进程不可达时 `daemon status` 必须退出非 0。"""
    result = run_cli(["daemon", "status"], sandbox, pureterm_bin)
    assert result.returncode != 0, "守护进程不可达时 daemon status 应返回非 0"


def test_daemon_stop_cli_stops_the_daemon(
    daemon, sandbox: Sandbox, pureterm_bin: Path
) -> None:
    """`pureterm daemon stop` 必须真的把守护进程停掉。

    回归背景:守护进程**故意**在 GUI 退出后继续存活(会话要扛过 GUI
    重启),但它**没有任何自停手段** —— 用户没法回收它,自动化测试更是
    每个用例泄漏一个(几百个 `puretermd` 足以把机器拖垮:E2E 套件耗时
    从 85s 涨到 345s,大批用例超时)。
    """
    # 先确认它活着。
    assert run_cli(["daemon", "status"], sandbox, pureterm_bin).returncode == 0

    stopped = run_cli(["daemon", "stop"], sandbox, pureterm_bin)
    assert stopped.returncode == 0, f"daemon stop 失败: {stopped.stderr}"
    assert "stopped" in stopped.stdout.lower(), stopped.stdout

    # 守护进程应已退出:socket 不再可达。
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if not daemon_ping(sandbox.daemon_socket):
            break
        time.sleep(0.2)
    else:
        pytest.fail("daemon stop 之后守护进程仍在响应 ping")


def test_daemon_stop_cli_fails_when_not_running(
    sandbox: Sandbox, pureterm_bin: Path
) -> None:
    """守护进程没在跑时 `daemon stop` 必须**报错退出**,而不是假装成功。"""
    result = run_cli(["daemon", "stop"], sandbox, pureterm_bin)
    assert result.returncode != 0, "守护进程没在跑,daemon stop 不该成功"
    assert "not running" in result.stderr.lower(), result.stderr


# ── 会话生命周期 ────────────────────────────────────────────────────────────


def test_session_create_list_kill(daemon, sandbox: Sandbox) -> None:
    """创建 → 列出 → 杀掉一个守护会话。"""
    created = daemon_rpc(sandbox.daemon_socket, "session.create", {"cols": 80, "rows": 24})
    assert created.get("error") is None, f"session.create 失败: {created}"
    result = created["result"]
    session_id = result.get("session_id")
    assert session_id, f"session.create 没有返回 session_id: {result}"

    listed = daemon_rpc(sandbox.daemon_socket, "session.list")
    assert listed.get("error") is None
    sessions = listed["result"]["sessions"]
    assert any(s.get("session_id") == session_id for s in sessions), (
        f"新建的会话 {session_id} 没有出现在列表里: {sessions}"
    )

    killed = daemon_rpc(sandbox.daemon_socket, "session.kill", {"session_id": session_id})
    assert killed.get("error") is None, f"session.kill 失败: {killed}"

    listed = daemon_rpc(sandbox.daemon_socket, "session.list")
    assert not any(s.get("session_id") == session_id for s in listed["result"]["sessions"]), (
        "被杀掉的会话仍在列表里"
    )


def test_session_api_uses_session_id_consistently(daemon, sandbox: Sandbox) -> None:
    """会话 id 在整套守护进程 API 里必须都叫 `session_id`。

    回归背景:`session.create` / `attach` / `kill` 用 `session_id`,
    而 `session.list` 的条目曾用 `id` —— 同一套 API 两种叫法,调用方
    得同时认两个键,拼错一个就静默拿到空值。
    """
    created = daemon_rpc(sandbox.daemon_socket, "session.create", {"cols": 80, "rows": 24})
    session_id = created["result"]["session_id"]

    listed = daemon_rpc(sandbox.daemon_socket, "session.list")
    entry = next(
        s for s in listed["result"]["sessions"] if s.get("session_id") == session_id
    )
    assert "id" not in entry, f"条目里不应再出现旧的 `id` 键: {entry}"
    assert "session_id" in entry

    attached = daemon_rpc(
        sandbox.daemon_socket, "session.attach", {"session_id": session_id}
    )
    assert attached["result"]["session_id"] == session_id


def test_session_attach_unknown_id_errors(daemon, sandbox: Sandbox) -> None:
    """挂接不存在的会话必须返回错误,而不是崩溃。"""
    response = daemon_rpc(
        sandbox.daemon_socket, "session.attach", {"session_id": "no-such-session"}
    )
    assert response.get("error") is not None, f"应返回 error,实际: {response}"


def test_unknown_daemon_method_errors(daemon, sandbox: Sandbox) -> None:
    response = daemon_rpc(sandbox.daemon_socket, "definitely.not.a.method")
    assert response.get("error") is not None


# ── 与 GUI 的集成 ───────────────────────────────────────────────────────────


def test_gui_attaches_to_daemon_on_cold_start(
    app: AppInstance, sandbox: Sandbox, pureterm_bin: Path
) -> None:
    """GUI 冷启动必须连上守护进程,而不是退回直连 PTY。

    回归背景:守护进程二进制找不到(或拉起失败)时,应用只打一条 WARN
    就静默退回直连 PTY,会话持久化随之失效 —— 用户看不出任何异常。
    这里断言 GUI 启动后 `daemon status` 显示可达。
    """
    result = run_cli(["daemon", "status"], sandbox, pureterm_bin)
    assert result.returncode == 0, (
        "GUI 启动后守护进程仍不可达 —— 应用很可能退回了直连 PTY:\n"
        f"{result.stdout}\n{result.stderr}\n--- 应用日志 ---\n{app.log()[-3000:]}"
    )


def test_gui_does_not_warn_about_unreachable_daemon(app: AppInstance) -> None:
    """应用日志里不应出现「守护进程不可达,退回直连 PTY」。"""
    log = app.log()
    assert "daemon unreachable on bootstrap" not in log, (
        f"应用在冷启动时没能连上守护进程:\n{log[-3000:]}"
    )


def test_gui_surface_is_backed_by_daemon(app: AppInstance, sandbox: Sandbox) -> None:
    """GUI 的 surface 应当挂接在守护进程上(会话可跨 GUI 重启存活)。"""
    sessions = daemon_rpc(sandbox.daemon_socket, "session.list")["result"]["sessions"]
    assert sessions, "守护进程里没有任何会话,说明 GUI 没有走守护进程"
