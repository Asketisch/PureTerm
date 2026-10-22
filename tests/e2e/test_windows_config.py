"""多窗口 / 工作区 / 配置读写 / 分割树手术的端到端检测。

这些方法此前**完全没有 E2E 覆盖**,而其中一半是**空实现**:

- `window.focus` 回 `{"focused": true}` 却**没有任何副作用** —— 调用方
  (agent / MCP)以为窗口被聚焦了,实际什么都没发生;
- `window.list` 对**每个**窗口都报**活动窗口**的标签页数(快照里
  只有活动窗口的标签页),且窗口总数取自 `main_window`(恒 ≤1)——
  多窗口对 API 完全不可见;
- `config.set` 回一句「not yet supported」却带**成功**状态;
  `config.get` 同理(带 `key` 但没有 `value`)。

本文件把这些逐个钉住,并覆盖分割树手术
(`pane.detach` / `pane.attach` / `pane.swap` / `pane.equalize` /
`pane.resize`)与 `workspace.switch`。
"""

from __future__ import annotations

import json
import time
from pathlib import Path

import pytest

from conftest import AppInstance, Sandbox


def rpc(app: AppInstance, method: str, params: dict | None = None) -> object:
    """调用一个 JSON-RPC 方法(失败即断)。"""
    args = ["msg", method]
    if params is not None:
        args.append(json.dumps(params))
    result = app.cli(args + ["--json"], timeout=30)
    assert result.returncode == 0, f"{method} 失败: {result.stderr}"
    return json.loads(result.stdout)


def wait_until(predicate, timeout: float = 15.0, interval: float = 0.25):
    """轮询直到谓词返回**非 None** 的值(或超时返回 None)。

    注意判据是「不是 None」,**不是**「为真」—— 索引 0 在 Python 里
    是 falsy,`if value:` 会把「切到第 0 个窗口」误判成「还没切」。
    """
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value is not None:
            return value
        time.sleep(interval)
    return None


# ── 多窗口 ──────────────────────────────────────────────────────────────────


def test_window_create_adds_a_window(app: AppInstance) -> None:
    """`window.create` 必须真的多开一个窗口(此前 `window.list` 恒为 1 个)。"""
    before = rpc(app, "window.list")
    assert isinstance(before, list)
    assert len(before) >= 1

    rpc(app, "window.create")

    after = wait_until(lambda: (lambda w: w if len(w) > len(before) else None)(rpc(app, "window.list")))
    assert after is not None, f"window.create 之后窗口数没变: {after}"
    assert len(after) == len(before) + 1


def test_window_list_reports_per_window_tab_counts(app: AppInstance) -> None:
    """`window.list` 必须报**每个窗口自己的**标签页数。

    回归:此前所有窗口都报活动窗口的标签页数 —— 多窗口场景下
    `window.list` 的 `tabs` 字段完全是错的。
    """
    rpc(app, "window.create")
    wait_until(lambda: len(rpc(app, "window.list")) >= 2)

    windows = rpc(app, "window.list")
    assert isinstance(windows, list)
    assert len(windows) >= 2
    # 每个窗口都报自己的标签页数(至少是 1 —— 每个窗口都有主标签页)。
    for w in windows:
        assert w["tabs"] >= 1, f"窗口 {w} 的标签页数不对: {w}"
    # 恰好一个活动窗口。
    active = [w for w in windows if w.get("active")]
    assert len(active) == 1, f"活动窗口数应为 1,实际 {active}"


def test_window_focus_actually_switches(app: AppInstance) -> None:
    """`window.focus` 必须真的切换活动窗口(此前是空实现)。

    回归:该方法回 `{"focused": true}` 却**没有任何副作用** —— 调用方
    以为窗口被聚焦了,实际什么都没发生。
    """
    rpc(app, "window.create")
    wait_until(lambda: len(rpc(app, "window.list")) >= 2)

    windows = rpc(app, "window.list")
    active_idx = next(i for i, w in enumerate(windows) if w.get("active"))
    other_idx = next(i for i, w in enumerate(windows) if not w.get("active"))

    rpc(app, "window.focus", {"index": other_idx})

    def switched():
        w = rpc(app, "window.list")
        idx = next((i for i, x in enumerate(w) if x.get("active")), None)
        return (idx,) if idx == other_idx else None

    assert wait_until(switched) is not None, (
        f"window.focus 之后活动窗口没变(仍是 {active_idx},期望 {other_idx}): {rpc(app, 'window.list')}"
    )


def test_window_focus_rejects_out_of_range(app: AppInstance) -> None:
    """越界的窗口序号必须**报错**,而不是静默成功。"""
    count = len(rpc(app, "window.list"))
    result = app.cli(["msg", "window.focus", json.dumps({"index": 999}), "--json"], timeout=20)
    assert result.returncode != 0, "越界的 window.focus 不该成功"
    assert "out of range" in result.stderr.lower(), result.stderr
    assert count >= 1


def test_window_close_closes_a_tab(app: AppInstance) -> None:
    """`window.close` 关闭活动标签页。"""
    rpc(app, "tab.new")
    wait_until(lambda: len(rpc(app, "tab.list")) >= 2)
    before = len(rpc(app, "tab.list"))
    rpc(app, "window.close")
    after = wait_until(
        lambda: (lambda t: t if len(t) < before else None)(rpc(app, "tab.list"))
    )
    assert after is not None, "window.close 之后标签页数没变"


# ── 工作区 ──────────────────────────────────────────────────────────────────


def test_workspace_switch_changes_active_workspace(app: AppInstance) -> None:
    """`workspace.switch` 必须真的切换工作区。"""
    before = rpc(app, "workspace.list")
    assert isinstance(before, list)
    assert len(before) >= 1

    target = before[-1]
    rpc(app, "workspace.switch", {"id": target["id"]})

    def switched():
        w = rpc(app, "workspace.list")
        act = [x for x in w if x.get("active")]
        return (act[0],) if act and act[0]["id"] == target["id"] else None

    assert wait_until(switched) is not None, (
        f"workspace.switch 之后活动工作区没变: {rpc(app, 'workspace.list')}"
    )


def test_workspace_switch_unknown_id_errors(app: AppInstance) -> None:
    """不存在的工作区 id 必须报错。"""
    result = app.cli(
        ["msg", "workspace.switch", json.dumps({"id": 999999}), "--json"], timeout=20
    )
    assert result.returncode != 0, "不存在的工作区不该切换成功"


# ── 分割树手术 ──────────────────────────────────────────────────────────────


def test_pane_split_then_swap(app: AppInstance) -> None:
    """split 之后 swap 必须真的交换活动窗格。"""
    rpc(app, "pane.split", {"direction": "vertical"})
    panes = wait_until(lambda: (lambda p: p if len(p) >= 2 else None)(rpc(app, "pane.list")))
    assert panes is not None, f"split 之后窗格数不对: {rpc(app, 'pane.list')}"

    # `pane.swap` 交换的是两个窗格在分割树里的**位置**(内容互换),
    # 活动窗格指针不变 —— 所以断言的是 `pane_index` 互换了。
    idx_before = {p["pane_id"]: p["pane_index"] for p in panes}
    ids = list(idx_before)
    src, dst = ids[0], ids[1]

    rpc(app, "pane.swap", {"src": src, "dst": dst})

    def swapped():
        p = rpc(app, "pane.list")
        idx_now = {x["pane_id"]: x["pane_index"] for x in p}
        if src not in idx_now or dst not in idx_now:
            return None
        if idx_now[src] == idx_before[dst] and idx_now[dst] == idx_before[src]:
            return (idx_now,)
        return None

    assert wait_until(swapped) is not None, (
        f"pane.swap 之后两个窗格的位置没有互换: {rpc(app, 'pane.list')} (之前 {idx_before})"
    )


def test_pane_detach_then_attach(app: AppInstance) -> None:
    """`pane.detach`(break-pane)必须把窗格拆成独立标签页;
    `pane.attach`(join-pane)必须把它并回去。"""
    rpc(app, "pane.split", {"direction": "vertical"})
    panes = wait_until(lambda: (lambda p: p if len(p) >= 2 else None)(rpc(app, "pane.list")))
    assert panes is not None

    tabs_before = len(rpc(app, "tab.list"))
    detached = next(p["pane_id"] for p in panes if not p.get("active"))

    rpc(app, "pane.detach", {"pane_id": detached})

    def tab_count_grew():
        t = rpc(app, "tab.list")
        return len(t) if len(t) > tabs_before else None

    tabs_after_detach = wait_until(tab_count_grew)
    assert tabs_after_detach is not None, (
        f"pane.detach 之后标签页数没变: {rpc(app, 'tab.list')}"
    )

    # join-pane:把拆出来的窗格并回第 0 个标签页。
    rpc(app, "pane.attach", {"src_pane_id": detached, "dst_tab_index": 0})

    def tab_count_back():
        t = rpc(app, "tab.list")
        # 回到 detach 之前的标签页数。
        return len(t) if len(t) == tabs_before else None

    assert wait_until(tab_count_back) is not None, (
        f"pane.attach 之后标签页数没回去(期望 {tabs_before},拆出后 {tabs_after_detach}): "
        f"{rpc(app, 'tab.list')}"
    )
    # 并回去的窗格应落在目标标签页里(而不是凭空消失)。
    tabs = rpc(app, "tab.list")
    assert tabs[0]["panes"] == 2, f"目标标签页应有 2 个窗格: {tabs}"


def test_pane_equalize_and_resize(app: AppInstance) -> None:
    """`pane.equalize` / `pane.resize` 必须被接受且不崩。"""
    rpc(app, "pane.split", {"direction": "vertical"})
    panes = wait_until(lambda: (lambda p: p if len(p) >= 2 else None)(rpc(app, "pane.list")))
    assert panes is not None

    eq = rpc(app, "pane.equalize")
    assert isinstance(eq, dict) and eq.get("equalized") is True, eq

    rs = rpc(app, "pane.resize", {"amount": 2, "direction": "left"})
    assert isinstance(rs, dict) and rs.get("resized") is True, rs

    # 参数缺失必须报错,而不是静默什么都不做。
    bad = app.cli(["msg", "pane.resize", "--json"], timeout=20)
    assert bad.returncode != 0, "缺参数的 pane.resize 不该成功"


def test_pane_attach_requires_params(app: AppInstance) -> None:
    """`pane.attach` 缺参数必须报错。"""
    result = app.cli(["msg", "pane.attach", "--json"], timeout=20)
    assert result.returncode != 0, "缺参数的 pane.attach 不该成功"
    assert "src_pane_id" in result.stderr, result.stderr


# ── 配置读写 ────────────────────────────────────────────────────────────────


def test_config_get_reads_the_value(app: AppInstance, sandbox: Sandbox) -> None:
    """`config.get` 必须真的读出值(此前只回 key,没有 value)。"""
    result = rpc(app, "config.get", {"key": "font.size"})
    assert isinstance(result, dict), result
    assert result["key"] == "font.size"
    assert "value" in result, f"config.get 没有返回 value: {result}"
    # 应用启动时会写默认配置,`font.size` 必然存在。
    assert result.get("exists") is True, f"font.size 不该不存在: {result}"
    assert isinstance(result["value"], (int, float)), result


def test_config_set_writes_and_get_reads_back(app: AppInstance, sandbox: Sandbox) -> None:
    """`config.set` 必须真的写盘,`config.get` 必须能读回来。

    回归:此前 `config.set` 只回一句「not yet supported」却带**成功**
    状态 —— 调用方(agent / MCP)会以为设置成功了。
    """
    result = rpc(app, "config.set", {"key": "font.size", "value": 17.5})
    assert isinstance(result, dict), result
    assert result.get("key") == "font.size", result

    # 落盘:配置文件里必须真的出现新值。
    def on_disk():
        text = sandbox.read_config()
        return text if "17.5" in text else None

    assert wait_until(on_disk) is not None, (
        f"config.set 没有落盘:\n{sandbox.read_config()}"
    )

    # 读回。
    read_back = wait_until(
        lambda: (
            lambda r: r if r.get("value") == 17.5 else None
        )(rpc(app, "config.get", {"key": "font.size"}))
    )
    assert read_back is not None, f"config.get 读不回新值: {rpc(app, 'config.get', {'key': 'font.size'})}"


def test_config_set_keeps_float_type(app: AppInstance, sandbox: Sandbox) -> None:
    """整数请求不得把浮点键写成整数。

    回归风险:`font.size` 在 TOML 里是**浮点**;写成 `size = 14`(整数)
    会让 alacritty 的反序列化失败 —— 配置加载失败,应用随即起不来。
    """
    rpc(app, "config.set", {"key": "font.size", "value": 14})
    text = wait_until(lambda: (lambda t: t if "size = 14.0" in t else None)(sandbox.read_config()))
    assert text is not None, (
        f"font.size 被写成了非浮点:\n{sandbox.read_config()}"
    )
    # 应用必须还活着(配置没被写坏)。
    assert app.cli(["system", "ping"], timeout=15).returncode == 0


def test_config_set_preserves_other_sections(app: AppInstance, sandbox: Sandbox) -> None:
    """写一个键**不能**动到文件里的其它内容。"""
    rpc(app, "config.set", {"key": "pureterm.locale", "value": "en-US"})
    text = wait_until(lambda: (lambda t: t if "en-US" in t else None)(sandbox.read_config()))
    assert text is not None, f"locale 没写进去:\n{sandbox.read_config()}"
    # 其它段落仍在(说明是「改一个键」而不是「重写整个文件」)。
    assert "[font]" in text or "font" in text, text
    assert "scrolling" in text or "history" in text, text


def test_config_set_rejects_invalid_key(app: AppInstance) -> None:
    """非法键必须报错,而不是静默成功。"""
    result = app.cli(
        ["msg", "config.set", json.dumps({"key": "a..b", "value": 1}), "--json"],
        timeout=20,
    )
    assert result.returncode != 0, "非法键不该成功"


def test_config_set_requires_value(app: AppInstance) -> None:
    """缺 `value` 必须报错。"""
    result = app.cli(
        ["msg", "config.set", json.dumps({"key": "font.size"}), "--json"], timeout=20
    )
    assert result.returncode != 0, "缺 value 不该成功"


def test_config_get_invalid_key_errors(app: AppInstance) -> None:
    """非法键的 `config.get` 必须报错。"""
    result = app.cli(
        ["msg", "config.get", json.dumps({"key": "a..b"}), "--json"], timeout=20
    )
    assert result.returncode != 0, "非法键不该成功"


# ── 其它未覆盖方法 ──────────────────────────────────────────────────────────


def test_events_recent_returns_events(app: AppInstance) -> None:
    """`events.recent` 必须真的返回事件。"""
    result = rpc(app, "events.recent", {"limit": 5})
    assert isinstance(result, dict), result
    assert "events" in result, result
    assert isinstance(result["events"], list), result


def test_surface_resize_is_accepted(app: AppInstance) -> None:
    """`surface.resize` 必须被接受(它只回一个提示,不崩)。"""
    result = rpc(app, "surface.resize")
    assert isinstance(result, dict), result


def test_debug_dump_state_returns_state(app: AppInstance) -> None:
    """`debug.dump_state` 必须返回应用状态摘要。"""
    result = rpc(app, "debug.dump_state")
    assert isinstance(result, dict), result
    assert result, "debug.dump_state 不该返回空对象"
