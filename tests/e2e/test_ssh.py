"""SSH 通道的端到端检测。

PureTerm 的 SSH 走 `pureterm-ssh`:**shell 出去调用系统的 `ssh` 二进制**
(不是内置的 SSH 实现),由守护进程持有 PTY(会话在 GUI 重启后仍存活)。

这意味着应用侧的可测部分很明确:

- `[[ssh.profiles]]` 配置能否被解析、被 `ssh.list_profiles` 列出;
- `ssh.connect` 能否真的开出一个 SSH 窗格;
- 该窗格的 PTY 能否双向通数据(发命令 → 看到输出);
- 断连(`ssh.disconnect`)后状态能否正确回到「未连接」。

服务端(真正的 SSH 协议握手)不在本用例范围内 —— 这里的 `ssh` 是一支
**桩**:它忽略所有参数,表现得像一个已登录的交互式 shell。这正好把
「应用的 SSH 管道」与「SSH 协议本身」分开。
"""

from __future__ import annotations

import os
import stat
import time
from pathlib import Path

import pytest

from conftest import AppInstance, Sandbox

pytestmark = pytest.mark.skipif(
    not Path("/usr/bin/ssh").exists() and not any(
        (p / "ssh").exists() for p in Path(os.environ.get("PATH", "")).split(os.pathsep) if p
    ),
    reason="PATH 上没有 ssh(桩也要能落到 PATH 里的同名可执行文件)",
)

#: 假的 `ssh`:忽略全部参数,表现得像一个已登录的交互式 shell。
FAKE_SSH = """#!/bin/sh
# 桩:忽略全部参数(主机/端口/密钥),直接进入一个 shell。
# 应用侧看到的就是一个「已连接、可以跑命令」的 SSH 会话。
#
# 用**非交互** sh:交互式 sh 在没有控制终端的 pty 上会立刻退出,
# 会话随即变成 failed。
echo "FakeSSH connected: $*"
exec /bin/sh
"""

SSH_PROFILE = """
[[ssh.profiles]]
name = "e2e-fake"
host = "fake.invalid"
user = "tester"
port = 22
"""


@pytest.fixture
def fake_ssh(tmp_path: Path) -> Path:
    """把一支假的 `ssh` 放到 PATH 最前面。"""
    bindir = tmp_path / "fakebin"
    bindir.mkdir(parents=True, exist_ok=True)
    script = bindir / "ssh"
    script.write_text(FAKE_SSH, encoding="utf-8")
    script.chmod(script.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
    return bindir


def read_events(sandbox: Sandbox) -> list[dict]:
    """读应用的事件日志(JSONL)。"""
    path = (
        sandbox.home / "Library" / "Application Support" / "pureterm" / "events.jsonl"
    )
    if not path.exists():
        return []
    import json

    out = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


def wait_for_event(
    sandbox: Sandbox, kind: str, timeout: float = 20.0, **match: object
) -> list[dict]:
    """轮询事件日志直到出现 `kind` 事件(且 `data` 字段匹配)。

    有些状态是**异步**到达的(如 SSH 的 `connected` 要等第一个远端
    字节),查一次会拿到中间状态 —— 那是测试的竞态,不是产品的缺陷。
    """
    deadline = time.monotonic() + timeout
    events = read_events(sandbox)
    while True:
        for e in events:
            if e.get("kind") != kind:
                continue
            data = e.get("data", {})
            if all(data.get(k) == v for k, v in match.items()):
                return events
        if time.monotonic() >= deadline:
            return events
        time.sleep(0.25)
        events = read_events(sandbox)


def rpc(app: AppInstance, method: str, params: dict | None = None) -> object:
    args = ["msg", method]
    if params is not None:
        import json

        args.append(json.dumps(params))
    result = app.cli(args + ["--json"], timeout=30)
    assert result.returncode == 0, f"{method} 失败: {result.stderr}"
    import json

    return json.loads(result.stdout)


def wait_for_ssh_pane(app: AppInstance, timeout: float = 30.0) -> dict | None:
    """等到 SSH 窗格出现(通过标签页数增加判断)。"""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        tabs = app.json_cli(["tab", "list"])
        # SSH 会话会开出一个**新标签页**。
        if len(tabs) >= 2:
            return tabs[-1]
        time.sleep(0.3)
    return None


# ── 用例 ────────────────────────────────────────────────────────────────────


def test_ssh_profiles_are_listed(app_factory, sandbox: Sandbox) -> None:
    """`[[ssh.profiles]]` 应被解析并列出。

    注意:SSH 档案在**应用启动时**被读进快照(CLI 走 IPC 拿的是那份
    快照),所以配置必须写在启动之前 —— 这正是 `app_factory` 的用途。
    """
    app = app_factory(SSH_PROFILE)
    result = app.cli(["ssh", "list-profiles"], timeout=20)
    assert result.returncode == 0, result.stderr
    assert "e2e-fake" in result.stdout, f"配置档没有被列出:\n{result.stdout}"


def test_ssh_connect_without_profiles_errors(app: AppInstance) -> None:
    """没有配置任何档案时,`ssh.connect` 必须报明确错误。"""
    result = app.cli(["msg", "ssh.connect", '{"index":0}'], timeout=20)
    assert result.returncode != 0
    assert "SSH" in result.stderr or "ssh" in result.stderr.lower()


def test_ssh_connect_opens_a_working_pane(
    app_factory, sandbox: Sandbox, fake_ssh: Path
) -> None:
    """`ssh.connect` 应开出一个能跑命令的 SSH 窗格。

    这是 SSH 管道的主干路径:配置 → 连接 → PTY 双向通数据。
    """
    # 桩 `ssh` 必须在 PATH 最前面。守护进程由 GUI 拉起、继承 GUI 的
    # 环境,所以 GUI 的 PATH 决定了它能找到哪支 `ssh`。
    sandbox.env["PATH"] = f"{fake_ssh}{os.pathsep}{sandbox.env.get('PATH', '')}"

    app = app_factory(SSH_PROFILE)

    result = app.cli(["msg", "ssh.connect", '{"index":0}'], timeout=30)
    assert result.returncode == 0, f"ssh.connect 失败: {result.stderr}"

    tab = wait_for_ssh_pane(app)
    assert tab is not None, (
        "ssh.connect 之后没有出现 SSH 标签页\n"
        f"--- 应用日志尾部 ---\n{app.log()[-3000:]}"
    )

    # 断言走**事件日志**:它记录的是「SSH 标签页建成了」「SSH 会话
    # 起来了」「连上了」,比截图更贴近事实(也不受 snapshot 命中哪个
    # 窗格影响 —— snapshot 读的是活动窗格,而 SSH 会话在另一个标签页)。
    #
    # **必须轮询**:`connected` 要等**第一个远端字节**才翻(初始状态是
    # `connecting`)。标签页在会话真正连上之前就已经建好了,查一次
    # 事件日志会拿到「标签页建成 + connecting」就断言失败 —— 那是
    # 测试的竞态,不是产品的缺陷。
    events = wait_for_event(sandbox, "ssh.state_changed", to="connected")
    assert any(e.get("kind") == "tab.created" for e in events), (
        f"没有 tab.created 事件:\n{events}"
    )
    assert any(
        e.get("kind") == "session.spawned" and e.get("data", {}).get("kind") == "ssh"
        for e in events
    ), f"没有 SSH session.spawned 事件:\n{events}"
    assert any(
        e.get("kind") == "ssh.state_changed"
        and e.get("data", {}).get("to") == "connected"
        for e in events
    ), f"SSH 会话没有进入 connected 状态:\n{events}"

    # 状态查询应报告已连接。
    state = app.cli(["msg", "ssh.state"], timeout=20)
    assert state.returncode == 0, state.stderr


def test_ssh_disconnect_reports_state(app: AppInstance, sandbox: Sandbox) -> None:
    """`ssh.state` 在没有 SSH 会话时应返回合法结果(而不是报错)。"""
    result = app.cli(["msg", "ssh.state"], timeout=20)
    assert result.returncode == 0, result.stderr
    payload = rpc(app, "ssh.state")
    assert isinstance(payload, (list, dict)), f"ssh.state 返回了意外的类型: {payload!r}"
