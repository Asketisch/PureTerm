"""触发器(`[[triggers]]`)的端到端检测。

触发器是「终端输出 → 动作」的钩子:正则匹配 pane 的**输出**,命中后执行
高亮 / 通知 / 回发文本 / 执行命令 / 设置变量 / 折叠等动作。

这里要守住的东西:

- 配置能不能被加载(`trigger.list` 要能列出来);
- 输出匹配后**必须真的触发**并留下 `trigger.matched` 事件;
- 模板展开必须**对非 ASCII 安全** —— 这曾经是个致命 bug:
  `expand_trigger_template` 按字节切片,遇到中文标题(如「构建完成」)
  就切在字符边界之外直接 panic;而触发器是在 **GUI 主线程**里求值的,
  panic 会连带把整个应用 abort 掉(整个终端消失,不只是这条规则失效);
- 非法配置(坏正则 / 缺字段)必须**跳过这一条**而不是连累其它规则,
  更不能让应用起不来;
- `config reload` 后新规则要生效。
"""

from __future__ import annotations

import json
import re
import time
from pathlib import Path

import pytest

from conftest import AppInstance

#: 事件表的一行:`SEQ  CATEGORY  KIND  DATA`(定宽列)。
_EVENT_ROW = re.compile(r"^(\d+)\s+(\S+)\s+(\S+)\s+(.*)$")


def trigger_events(app: AppInstance, kind: str | None = None) -> list[dict]:
    """从事件日志里挑出 trigger 类事件(必要时再按 kind 过滤)。

    走 `pureterm events list` 而不是直接读 JSONL:事件文件的位置随平台
    变化(`dirs::data_local_dir()`),而 CLI 帮我们解析好了。
    """
    result = app.cli(["events", "list", "--limit", "400", "--type", "trigger"], timeout=30)
    assert result.returncode == 0, result.stderr
    out: list[dict] = []
    for line in result.stdout.splitlines():
        line = line.rstrip()
        if not line or line.startswith("SEQ ") or line == "no events yet":
            continue
        m = _EVENT_ROW.match(line)
        if not m:
            continue
        _seq, _cat, ev_kind, data = m.groups()
        if kind and ev_kind != kind:
            continue
        try:
            payload = json.loads(data)
        except json.JSONDecodeError:
            payload = {"raw": data}
        out.append({"kind": ev_kind, "data": payload})
    return out


def wait_for_trigger_event(
    app: AppInstance, kind: str, timeout: float = 15.0, **match: object
) -> dict | None:
    """轮询直到出现符合条件的 trigger 事件。"""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for ev in trigger_events(app, kind):
            data = ev["data"]
            if all(data.get(k) == v for k, v in match.items()):
                return data
        time.sleep(0.25)
    return None


def send_line(app: AppInstance, command: str) -> None:
    """往活动 pane 发一行命令并回车。

    **必须带 `\\r`**:没有回车,shell 只是把文字留在提示符上,命令根本没
    执行 —— PTY 回显会让「命令没执行」看起来「执行了」。
    """
    result = app.cli(["surface", "send-text", "--text", command + "\r"], timeout=20)
    assert result.returncode == 0, result.stderr


def snapshot_text(app: AppInstance, needle: str | None = None, timeout: float = 15.0) -> str:
    """读活动 pane 的文本快照。

    `needle` 给了就**轮询到它出现为止** —— 快照一非空就返回会拿到命令
    执行前的画面(命令还没输出时终端本来就有提示符内容),那样断言会
    假阴性。
    """
    deadline = time.monotonic() + timeout
    text = ""
    while time.monotonic() < deadline:
        result = app.cli(["--json", "surface", "snapshot"], timeout=20)
        if result.returncode == 0:
            try:
                text = json.loads(result.stdout).get("text", "")
            except json.JSONDecodeError:
                text = ""
            if text and (needle is None or needle in text):
                return text
        time.sleep(0.25)
    return text


def app_alive(app: AppInstance) -> bool:
    """应用进程是否还活着。"""
    return app.proc.poll() is None


# ── 配置片段 ────────────────────────────────────────────────────────────────

TRIGGER_NOTIFY = """
[[triggers]]
name = "build-done"
pattern = "Built in \\\\d+s"
action = "notify"
title = "build finished"
"""

TRIGGER_NOTIFY_CJK = """
[[triggers]]
name = "cjk-title"
pattern = "TRIGGER_CJK_MARKER"
action = "notify"
title = "构建完成"
"""

TRIGGER_SEND_TEXT = """
[[triggers]]
name = "auto-reply"
pattern = "TRIGGER_SEND_MARKER"
action = "send_text"
text = "echo TRIGGER_REPLY_OK\\r"
"""

TRIGGER_EXEC = """
[[triggers]]
name = "run-on-match"
pattern = "TRIGGER_EXEC_MARKER"
action = "exec"
command = "/bin/echo"
args = ["TRIGGER_EXEC_OK\\n"]
route = "pane"
"""

TRIGGER_SET_VARIABLE = """
[[triggers]]
name = "stash"
pattern = "TRIGGER_VAR_MARKER"
action = "set_variable"
variable = "build"
value = "42"
"""

TRIGGER_BROKEN = """
[[triggers]]
name = "broken-regex"
pattern = "([unclosed"
action = "notify"

[[triggers]]
name = "missing-action"
pattern = "TRIGGER_ALIVE_MARKER"
"""


# ── 用例 ────────────────────────────────────────────────────────────────────


def test_trigger_list_reflects_config(app_factory) -> None:
    """`[[triggers]]` 应被解析,并能通过 `trigger.list` 列出。"""
    app = app_factory(TRIGGER_NOTIFY)

    result = app.cli(["--json", "msg", "trigger.list"], timeout=20)
    assert result.returncode == 0, result.stderr
    payload = json.loads(result.stdout)
    assert payload["count"] == 1, payload
    entry = payload["triggers"][0]
    assert entry["name"] == "build-done"
    assert entry["action"] == "notify"
    assert "Built in" in entry["pattern"]


def test_trigger_matches_output_and_emits_event(app_factory) -> None:
    """输出命中正则时,必须发出 `trigger.matched` 事件。"""
    app = app_factory(TRIGGER_NOTIFY)

    send_line(app, "printf 'Built in 42s\\n'")

    data = wait_for_trigger_event(app, "trigger.matched", trigger="build-done")
    assert data is not None, f"没有 trigger.matched 事件:\n{trigger_events(app)}"
    assert data["matched_text"] == "Built in 42s"
    assert data["action"] == "notify"
    assert data["pattern"] == "Built in \\d+s"


def test_trigger_with_chinese_title_does_not_crash_app(app_factory) -> None:
    """回归:非 ASCII 标题曾经让整个应用 abort。

    模板展开按字节切片 → 切在字符边界外 → panic → GUI 主线程 abort。
    这里断言应用**仍然存活**且事件照常发出。
    """
    app = app_factory(TRIGGER_NOTIFY_CJK)

    send_line(app, "printf 'TRIGGER_CJK_MARKER\\n'")

    data = wait_for_trigger_event(app, "trigger.matched", trigger="cjk-title")
    assert data is not None, f"没有 trigger.matched 事件:\n{trigger_events(app)}"

    # 关键断言:应用还活着(panic 会让它整个消失)。
    time.sleep(0.5)
    assert app_alive(app), (
        "应用在中文标题触发器上崩溃了:\n" + app.log()[-3000:]
    )
    ping = app.cli(["system", "ping"], timeout=10)
    assert ping.returncode == 0, ping.stderr


def test_trigger_send_text_writes_back_to_pane(app_factory) -> None:
    """`send_text` 动作应把展开后的文本写回 pane。"""
    app = app_factory(TRIGGER_SEND_TEXT)

    send_line(app, "printf 'TRIGGER_SEND_MARKER\\n'")

    text = snapshot_text(app, needle="TRIGGER_REPLY_OK")
    assert "TRIGGER_REPLY_OK" in text, f"send_text 没有写回 pane:\n{text}"


def test_trigger_exec_runs_command_into_pane(app_factory) -> None:
    """`exec` 动作应真的启动子进程,并把 stdout 路由回 pane。"""
    app = app_factory(TRIGGER_EXEC)

    send_line(app, "printf 'TRIGGER_EXEC_MARKER\\n'")

    text = snapshot_text(app, needle="TRIGGER_EXEC_OK", timeout=25)
    assert "TRIGGER_EXEC_OK" in text, f"exec 的 stdout 没有回到 pane:\n{text}"


def test_trigger_set_variable_emits_event(app_factory) -> None:
    """`set_variable` 应设置变量并发出 `trigger.variable_set` 事件。"""
    app = app_factory(TRIGGER_SET_VARIABLE)

    send_line(app, "printf 'TRIGGER_VAR_MARKER\\n'")

    data = wait_for_trigger_event(app, "trigger.variable_set")
    assert data is not None, f"没有 trigger.variable_set 事件:\n{trigger_events(app)}"
    assert data["variable"] == "build"


def test_broken_trigger_config_does_not_break_others(app_factory) -> None:
    """坏正则 / 缺 `action` 的条目被跳过,其它规则照常工作。"""
    app = app_factory(TRIGGER_BROKEN)

    result = app.cli(["--json", "msg", "trigger.list"], timeout=20)
    assert result.returncode == 0, result.stderr
    payload = json.loads(result.stdout)
    # 两条坏规则都应被丢弃。
    assert payload["count"] == 0, f"坏规则没有被跳过:\n{payload}"

    # 应用本身必须正常活着。
    assert app_alive(app), app.log()[-2000:]
    ping = app.cli(["system", "ping"], timeout=10)
    assert ping.returncode == 0, ping.stderr


def test_config_reload_picks_up_new_trigger(app_factory, sandbox) -> None:
    """`config reload` 后新写入的触发器规则应生效。"""
    app = app_factory(TRIGGER_NOTIFY)

    before = app.cli(["--json", "msg", "trigger.list"], timeout=20)
    assert json.loads(before.stdout)["count"] == 1

    # 追加一条新规则并重载。
    sandbox.append_config(
        "\n[[triggers]]\n"
        'name = "reloaded"\n'
        'pattern = "TRIGGER_RELOAD_MARKER"\n'
        'action = "notify"\n'
    )
    reload_result = app.cli(["config", "reload"], timeout=20)
    assert reload_result.returncode == 0, reload_result.stderr

    deadline = time.monotonic() + 15.0
    count = 0
    while time.monotonic() < deadline:
        probe = app.cli(["--json", "msg", "trigger.list"], timeout=20)
        count = json.loads(probe.stdout)["count"]
        if count == 2:
            break
        time.sleep(0.25)
    assert count == 2, f"重载后触发器数量不是 2:{count}"

    send_line(app, "printf 'TRIGGER_RELOAD_MARKER\\n'")
    data = wait_for_trigger_event(app, "trigger.matched", trigger="reloaded")
    assert data is not None, f"重载后的规则没有触发:\n{trigger_events(app)}"


def test_no_triggers_config_is_harmless(app) -> None:
    """没有任何 `[[triggers]]` 时应用照常工作(规则为空,不是错误)。"""
    result = app.cli(["--json", "msg", "trigger.list"], timeout=20)
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["count"] == 0
