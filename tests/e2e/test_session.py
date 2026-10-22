"""会话恢复与跨 GUI 重启存活的端到端测试。

守护进程(`puretermd`)存在的意义就是让终端会话在 GUI 关闭后继续存活;
`session_restore` 则负责在下次启动时把上次的布局接回来。这两件事是
PureTerm 区别于普通终端的关键能力,必须有端到端覆盖。

注意:这些用例**不用** `app` fixture —— 它们需要自己控制 GUI 的
启动/关闭时序,才能验证「关掉再打开」这条路径。
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import time
from pathlib import Path

import pytest

from conftest import Sandbox, _discover_binary


def launch(sandbox: Sandbox, binary: Path, log_name: str = "app.log") -> subprocess.Popen:
    """按沙箱环境启动 GUI。"""
    log_path = sandbox.root / log_name
    log = open(log_path, "a", encoding="utf-8")
    proc = subprocess.Popen(
        [str(binary)],
        env=sandbox.env,
        cwd=str(sandbox.root),
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    proc._log = log  # type: ignore[attr-defined]
    proc._log_path = log_path  # type: ignore[attr-defined]
    return proc


def stop(proc: subprocess.Popen, timeout: float = 10.0) -> None:
    if proc.poll() is None:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
            proc.wait(timeout=timeout)
        except (subprocess.TimeoutExpired, ProcessLookupError):
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
                proc.wait(timeout=5)
            except (subprocess.TimeoutExpired, ProcessLookupError):
                pass
    log = getattr(proc, "_log", None)
    if log is not None and not log.closed:
        log.close()


def wait_for_ipc(sandbox: Sandbox, binary: Path, proc: subprocess.Popen, timeout: float = 30.0):
    """等待 IPC 就绪。"""
    deadline = time.monotonic() + timeout
    last = ""
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(
                f"应用提前退出(码 {proc.returncode}):\n"
                f"{Path(proc._log_path).read_text(errors='replace')[-3000:]}"
            )
        probe = subprocess.run(
            [str(binary), "--json", "system", "ping"],
            env=sandbox.env,
            cwd=str(sandbox.root),
            capture_output=True,
            text=True,
            timeout=10,
        )
        if probe.returncode == 0:
            return
        last = probe.stderr.strip()
        time.sleep(0.2)
    raise TimeoutError(f"IPC 未就绪({timeout}s): {last}")


def cli_json(sandbox: Sandbox, binary: Path, args: list[str]) -> object:
    probe = subprocess.run(
        [str(binary), "--json", *args],
        env=sandbox.env,
        cwd=str(sandbox.root),
        capture_output=True,
        text=True,
        timeout=20,
    )
    assert probe.returncode == 0, f"`{' '.join(args)}` 失败: {probe.stderr}"
    return json.loads(probe.stdout)


@pytest.fixture
def binary() -> Path:
    return _discover_binary()


# ── 会话跨 GUI 重启存活 ─────────────────────────────────────────────────────


def test_session_survives_gui_restart(sandbox: Sandbox, binary: Path) -> None:
    """核心能力:关掉 GUI 再打开,之前的终端会话仍在。

    流程:启动 GUI → 在窗格里跑一个会输出唯一标记的命令 → 关掉 GUI
    → 重新启动 → 在恢复的窗格里读回标记。
    """
    marker = "SESSION_SURVIVE_MARKER"

    first = launch(sandbox, binary, "first.log")
    try:
        wait_for_ipc(sandbox, binary, first)

        # 先建一个标签页,确保恢复时有东西可恢复。
        subprocess.run(
            [str(binary), "tab", "new"],
            env=sandbox.env,
            cwd=str(sandbox.root),
            capture_output=True,
            timeout=20,
        )
        time.sleep(1.0)

        subprocess.run(
            [str(binary), "surface", "send-text", "--text", f"echo {marker}"],
            env=sandbox.env,
            cwd=str(sandbox.root),
            capture_output=True,
            timeout=20,
        )
        # 等标记真的出现在屏上。
        deadline = time.monotonic() + 20
        seen = False
        while time.monotonic() < deadline:
            snap = subprocess.run(
                [str(binary), "surface", "snapshot"],
                env=sandbox.env,
                cwd=str(sandbox.root),
                capture_output=True,
                text=True,
                timeout=20,
            )
            if snap.returncode == 0 and marker in snap.stdout:
                seen = True
                break
            time.sleep(0.3)
        assert seen, "第一次运行里就没看到标记,无法验证会话存活"
    finally:
        stop(first)

    # 重新启动 GUI —— 应恢复之前的会话,标记还在。
    second = launch(sandbox, binary, "second.log")
    try:
        wait_for_ipc(sandbox, binary, second)
        deadline = time.monotonic() + 30
        restored = False
        while time.monotonic() < deadline:
            snap = subprocess.run(
                [str(binary), "surface", "snapshot"],
                env=sandbox.env,
                cwd=str(sandbox.root),
                capture_output=True,
                text=True,
                timeout=20,
            )
            if snap.returncode == 0 and marker in snap.stdout:
                restored = True
                break
            time.sleep(0.4)
        assert restored, (
            "GUI 重启后会话没有恢复(标记丢失)。\n"
            "--- 第二次启动的日志 ---\n"
            f"{Path(second._log_path).read_text(errors='replace')[-3000:]}"
        )
    finally:
        stop(second)


def test_restart_does_not_leave_stuck_placeholder(sandbox: Sandbox, binary: Path) -> None:
    """重启后不能一直卡在 “Starting PureTerm…” 占位符。"""
    first = launch(sandbox, binary, "first.log")
    try:
        wait_for_ipc(sandbox, binary, first)
        time.sleep(1.0)
    finally:
        stop(first)

    second = launch(sandbox, binary, "second.log")
    try:
        wait_for_ipc(sandbox, binary, second)
        # 等到 pane 真的建立(占位符消失)。
        deadline = time.monotonic() + 30
        panes = []
        while time.monotonic() < deadline:
            try:
                panes = cli_json(sandbox, binary, ["pane", "list"])
            except AssertionError:
                panes = []
            if panes:
                break
            time.sleep(0.3)
        assert panes, "重启后始终没有窗格,可能卡在启动占位符"
    finally:
        stop(second)
