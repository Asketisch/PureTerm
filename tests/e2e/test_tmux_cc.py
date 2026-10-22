"""tmux 控制模式(`tmux -CC`)在 GUI 里的端到端检测。

PureTerm 可以把一个**真实的 tmux 会话**接进 GUI:它拉起
`tmux -CC` 子进程,把 tmux 用 `%output` 信封送来的字节泵进终端网格,
键盘输入经 `%input` 信封送回。

这里要守住的:

- `pureterm tmux-attach --session <name>` 真的在 GUI 里**开出一个
  标签页**(早前它只打印一行 `tmux -CC …` 命令行就退出 0 ——
  看起来「附加成功了」,实际上什么都没发生);
- 附加后窗格里能看到**该会话当前的屏幕内容**。这里有个坑:tmux 3.6a
  在控制模式下**不会**在附加时回放屏幕(只发 `%begin`/`%end`/
  `%session-changed`,没有任何 `%output`),所以必须主动发
  `capture-pane -p` 把当前屏幕取回来 —— 否则窗格是空白的;
- GUI 里敲的键能**真的**送到 tmux 会话里(走 `%input` 信封);
- 会话不存在时必须**明确报错**,而不是留下一个空白标签页。

需要 PATH 上有 `tmux`;没有时整个模块优雅跳过。
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path

import pytest

from conftest import AppInstance

pytestmark = pytest.mark.skipif(
    shutil.which("tmux") is None, reason="PATH 上没有 tmux(brew install tmux 可启用)"
)


#: 本次测试运行专用的 tmux socket 名。
#:
#: **必须**用它:tmux 服务器默认全机只有一个(`/tmp/tmux-<uid>/default`)。
#: 不用独立 socket 的话,测试会在**用户自己的** tmux 服务器里建/杀会话
#: (污染现场),而且前一个用例的客户端退出时服务器可能跟着退出,下一个用例
#: 撞上正在死掉的服务器 → 收到 `%exit server exited unexpectedly`。
SOCKET = f"pureterm-e2e-py-{os.getpid()}"


def tmux(*args: str, check: bool = False) -> subprocess.CompletedProcess:
    """跑一条 tmux 命令(带 `-L <SOCKET>`,只碰我们自己的服务器)。"""
    proc = subprocess.run(
        ["tmux", "-L", SOCKET, *args], capture_output=True, text=True, timeout=20
    )
    if check and proc.returncode != 0:
        raise AssertionError(f"tmux {' '.join(args)} 失败: {proc.stderr}")
    return proc


def make_session(name: str, body: str) -> None:
    """建一个后台会话,跑 `body` 后驻留。"""
    proc = tmux("new-session", "-d", "-s", name, body)
    assert proc.returncode == 0, f"tmux new-session 失败: {proc.stderr}"


def kill_session(name: str) -> None:
    tmux("kill-session", "-t", name)


def tab_titles(app: AppInstance) -> list[str]:
    """`tab list` 返回的字段名是 `title`(`pane list` 才是 `tab_title`)。"""
    return [str(t.get("title", "")) for t in app.json_cli(["tab", "list"])]


def wait_for_tmux_tab(app: AppInstance, timeout: float = 25.0) -> bool:
    """等到出现一个 tmux 标签页。"""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if any("tmux" in t for t in tab_titles(app)):
            return True
        time.sleep(0.25)
    return False


def active_pane_text(app: AppInstance, needle: str, timeout: float = 20.0) -> str:
    """轮询活动窗格的快照,直到出现 `needle`。"""
    deadline = time.monotonic() + timeout
    text = ""
    while time.monotonic() < deadline:
        result = app.cli(["--json", "surface", "snapshot"], timeout=20)
        if result.returncode == 0:
            try:
                text = json.loads(result.stdout).get("text", "")
            except json.JSONDecodeError:
                text = ""
            if needle in text:
                return text
        time.sleep(0.3)
    return text


@pytest.fixture(scope="module", autouse=True)
def _tmux_server_cleanup():
    """用例跑完后停掉我们自己的 tmux 服务器(socket 已隔离)。"""
    yield
    subprocess.run(
        ["tmux", "-L", SOCKET, "kill-server"], capture_output=True, timeout=20
    )
    # `kill-server` 之后 socket 文件有时会留下来(服务器已死,但 inode
    # 还在)—— 删掉它,免得每次跑测试都在 /tmp 里堆一个。
    sock = Path("/tmp") / f"tmux-{os.getuid()}" / SOCKET
    try:
        sock.unlink()
    except OSError:
        pass


# ── 用例 ────────────────────────────────────────────────────────────────────


def test_tmux_attach_opens_a_tab(app: AppInstance) -> None:
    """`tmux-attach` 应在 GUI 里开出一个 tmux 标签页。"""
    name = f"pt-e2e-attach-{os.getpid()}"
    make_session(name, "printf 'TMUX_ATTACH_MARKER\\n'; sleep 120")
    try:
        before = len(tab_titles(app))
        result = app.cli(
            ["tmux-attach", "--session", name, "--tmux-socket", SOCKET], timeout=30
        )
        assert result.returncode == 0, f"tmux-attach 失败: {result.stderr}"

        assert wait_for_tmux_tab(app), f"没有出现 tmux 标签页: {tab_titles(app)}"
        assert len(tab_titles(app)) > before, "标签页数量没有增加"
    finally:
        kill_session(name)


def test_tmux_attach_shows_current_session_screen(app: AppInstance) -> None:
    """附加后窗格里应能看到该会话**当前的屏幕内容**。

    tmux 3.6a 在控制模式下附加时**不回放**屏幕 —— 必须主动
    `capture-pane -p` 才拿得到。这是本用例守住的关键行为。
    """
    name = f"pt-e2e-screen-{os.getpid()}"
    make_session(name, "printf 'TMUX_SCREEN_MARKER\\n'; sleep 120")
    try:
        # 等 shell 把标记打印出来,再附加(这样「当前屏幕」里确实有它)。
        time.sleep(1.0)
        result = app.cli(
            ["tmux-attach", "--session", name, "--tmux-socket", SOCKET], timeout=30
        )
        assert result.returncode == 0, f"tmux-attach 失败: {result.stderr}"
        assert wait_for_tmux_tab(app), f"没有出现 tmux 标签页: {tab_titles(app)}"

        text = active_pane_text(app, "TMUX_SCREEN_MARKER", timeout=25)
        assert "TMUX_SCREEN_MARKER" in text, (
            f"附加后窗格没有显示会话的当前屏幕:\n{text}\n"
            f"--- 应用日志尾部 ---\n{app.log()[-1500:]}"
        )
    finally:
        kill_session(name)


def test_tmux_attach_sends_input_to_session(app: AppInstance) -> None:
    """GUI 里敲的键应真的送进 tmux 会话(经 `%input` 信封)。"""
    name = f"pt-e2e-input-{os.getpid()}"
    make_session(name, "sleep 120")
    try:
        result = app.cli(
            ["tmux-attach", "--session", name, "--tmux-socket", SOCKET], timeout=30
        )
        assert result.returncode == 0, f"tmux-attach 失败: {result.stderr}"
        assert wait_for_tmux_tab(app), f"没有出现 tmux 标签页: {tab_titles(app)}"

        # 通过 `surface send-text` 往活动窗格写文本 —— 它经 `%input`
        # 信封送到 tmux 会话。
        sent = app.cli(
            ["surface", "send-text", "--text", "echo TMUX_INPUT_ECHOED\r"], timeout=20
        )
        assert sent.returncode == 0, sent.stderr

        text = active_pane_text(app, "TMUX_INPUT_ECHOED", timeout=25)
        assert "TMUX_INPUT_ECHOED" in text, (
            f"输入没有到达 tmux 会话:\n{text}\n"
            f"--- 应用日志尾部 ---\n{app.log()[-1500:]}"
        )
    finally:
        kill_session(name)


def test_tmux_attach_missing_session_reports_error(app: AppInstance) -> None:
    """附加到不存在的会话必须**明确报错**,不能留下空白标签页。"""
    before = len(tab_titles(app))
    result = app.cli(
        [
            "tmux-attach",
            "--session",
            "pt-e2e-does-not-exist-xyz",
            "--socket",
            SOCKET,
        ],
        timeout=30,
    )
    # IPC 本身会成功(副作用是异步的);关键是**不能**多出一个空白标签页。
    time.sleep(1.5)
    assert len(tab_titles(app)) == before, (
        f"附加失败却开出了标签页: {tab_titles(app)}\n"
        f"--- 应用日志尾部 ---\n{app.log()[-1500:]}"
    )
    assert app.cli(["system", "ping"], timeout=15).returncode == 0, "应用还活着"
