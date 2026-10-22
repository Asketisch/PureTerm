"""Browser / WebView 面板的端到端检测。

内嵌浏览器(GAP-3.5 / GAP-7.8)是一条长链路:

    IPC `browser.*` → GUI 派发到 WebView → WKWebView 执行 JS → 回传结果

任何一环断了,症状都是「命令没反应」或「返回空」,很难定位。这里用
一个**本地 HTTP 服务器**提供确定内容的页面,再用 `browser.*` 的
JSON-RPC 去驱动它,断言:

- `pane split-web` 真的创建了一个 web 窗格;
- `browser get-url` / `get-title` 返回的是**服务器上那个页面**的内容;
- `browser extract` 能把 DOM 里的文本取出来;
- `browser navigate` 之后内容确实换了;
- `browser wait` 能等到选择器出现;
- 整个过程应用不 panic。
"""

from __future__ import annotations

import json
import socket
import subprocess
import sys
import time
from pathlib import Path

import pytest

from conftest import AppInstance

PAGE_ONE = """<!doctype html>
<html>
  <head><title>PureTerm E2E Page One</title></head>
  <body>
    <h1 id="marker">PAGE_ONE_MARKER_TEXT</h1>
    <p class="content">Hello from the local test server.</p>
    <input id="field" type="text" />
  </body>
</html>
"""

PAGE_TWO = """<!doctype html>
<html>
  <head><title>PureTerm E2E Page Two</title></head>
  <body>
    <h1 id="marker">PAGE_TWO_MARKER_TEXT</h1>
  </body>
</html>
"""

#: 一个**动态**页面:元素在 800ms 后由页面自己的 `setTimeout` 插入。
#:
#: 这是 `browser.wait` 的关键场景 —— 旧的实现(在 JS 里同步忙等)会
#: **占住页面的 JS 线程**,页面自己的定时器因此永远得不到执行机会,
#: 元素永远不会出现,`wait` 必然超时。改成 Rust 侧轮询之后,两次
#: 探测之间页面的 JS 线程是空闲的,元素能正常出现。
PAGE_DYNAMIC = """<!doctype html>
<html>
  <head><title>PureTerm E2E Dynamic Page</title></head>
  <body>
    <h1 id="marker">DYNAMIC_PAGE_LOADED</h1>
    <script>
      setTimeout(function () {
        var el = document.createElement("div");
        el.id = "late";
        el.textContent = "LATE_ELEMENT_APPEARED";
        document.body.appendChild(el);
      }, 800);
    </script>
  </body>
</html>
"""


@pytest.fixture(scope="module")
def http_server(tmp_path_factory):
    """一个跑在随机端口上的本地 HTTP 服务器(**独立子进程**)。

    必须用子进程:WKWebView 的 WebContent 是个独立进程,若服务器跑在
    pytest 主进程里,页面加载与测试线程的阻塞相互影响,表现为
    「browser op channel closed」(JS 求值回调永不触发)。
    """
    root = tmp_path_factory.mktemp("webroot")
    (root / "one.html").write_text(PAGE_ONE, encoding="utf-8")
    (root / "two.html").write_text(PAGE_TWO, encoding="utf-8")
    (root / "dynamic.html").write_text(PAGE_DYNAMIC, encoding="utf-8")

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]

    proc = subprocess.Popen(
        [sys.executable, "-m", "http.server", str(port), "--bind", "127.0.0.1"],
        cwd=str(root),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    try:
        # 等服务器就绪。
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
                probe.settimeout(0.5)
                if probe.connect_ex(("127.0.0.1", port)) == 0:
                    break
            time.sleep(0.1)
        yield f"http://127.0.0.1:{port}"
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


def rpc(app: AppInstance, method: str, params: dict | None = None) -> object:
    """调用一个 browser.* JSON-RPC 方法。"""
    args = ["msg", method]
    if params is not None:
        args.append(json.dumps(params))
    result = app.cli(args + ["--json"], timeout=60)
    assert result.returncode == 0, f"{method} 失败: {result.stderr}"
    return json.loads(result.stdout)


def find_web_pane(app: AppInstance) -> int | None:
    """找出 web 窗格的 id(通过标题 “🌐 Browser” 识别)。"""
    for pane in app.json_cli(["pane", "list"]):
        title = str(pane.get("tab_title", "")) + str(pane.get("title", ""))
        if "Browser" in title:
            return int(pane["pane_id"])
    return None


def activate(app: AppInstance) -> None:
    """把 PureTerm 窗口置为前台。

    WKWebView 的 WebContent 进程只在窗口**可见**时才加载页面 / 执行
    脚本求值;窗口不在前台时,`browser.get_title` 这类操作会一直等不到
    JS 回调(表现为 “browser op channel closed”)。
    """
    import subprocess

    subprocess.run(
        [
            "osascript",
            "-e",
            'tell application "System Events" to set frontmost of '
            f'(first process whose unix id is {app.proc.pid}) to true',
        ],
        capture_output=True,
        timeout=15,
    )


def wait_for_browser(app: AppInstance, timeout: float = 30.0) -> str:
    """等到 web 窗格就绪,返回页面标题。

    `pane list` 不带 `kind` / `url` 字段(只有 pane_id / tab 等),所以
    「web 窗格是否就绪」只能靠 `browser.get_title` 是否有返回来判断 ——
    它同时也是对功能本身的验证。
    """
    activate(app)
    deadline = time.monotonic() + timeout
    last_error = ""
    while time.monotonic() < deadline:
        # 显式指定 web 窗格:不依赖「取第一个 web 窗格」的默认解析。
        pane_id = find_web_pane(app)
        args = ["msg", "browser.get_title", "--json"]
        if pane_id is not None:
            args += ["--pane-id", str(pane_id)]
        result = app.cli(args, timeout=20)
        if result.returncode == 0:
            # `browser.get_title` 的结果是 JS 求值返回的对象:
            # `{"ok": true, "title": "..."}`。
            payload = json.loads(result.stdout or "null")
            if isinstance(payload, dict):
                title = payload.get("title")
                if isinstance(title, str) and title:
                    return title
        else:
            last_error = result.stderr.strip()
        time.sleep(0.4)
    pytest.fail(
        f"web 窗格在 {timeout}s 内没有就绪。最后错误: {last_error!r}\n"
        f"--- 应用日志尾部 ---\n{app.log()[-1500:]}"
    )


# ── 用例 ────────────────────────────────────────────────────────────────────


def test_split_web_creates_browser_pane(app: AppInstance, http_server: str) -> None:
    """`pane split-web` 应创建一个 web 窗格并加载页面。"""
    before = len(app.json_cli(["pane", "list"]))
    result = app.cli(
        ["pane", "split-web", "--url", f"{http_server}/one.html", "--direction", "vertical"],
        timeout=30,
    )
    assert result.returncode == 0, result.stderr

    title = wait_for_browser(app)
    assert "Page One" in title
    assert len(app.json_cli(["pane", "list"])) > before


def test_browser_get_url_and_title(app: AppInstance, http_server: str) -> None:
    """`get-url` / `get-title` 应返回服务器上那个页面的信息。"""
    app.cli(
        ["pane", "split-web", "--url", f"{http_server}/one.html", "--direction", "vertical"],
        timeout=30,
    )
    title = wait_for_browser(app)
    url_payload = rpc(app, "browser.get_url")
    url = str((url_payload or {}).get("url", "")) if isinstance(url_payload, dict) else ""
    assert "Page One" in title, f"get_title 没有返回页面标题: {title!r}"
    assert "/one" in url, f"get_url 没有返回页面地址: {url!r}"


def test_browser_extract_reads_dom_text(app: AppInstance, http_server: str) -> None:
    """`extract` 应能把 DOM 里的文本取出来。"""
    app.cli(
        ["pane", "split-web", "--url", f"{http_server}/one.html", "--direction", "vertical"],
        timeout=30,
    )
    wait_for_browser(app)

    deadline = time.monotonic() + 20
    text = ""
    while time.monotonic() < deadline:
        payload = rpc(app, "browser.extract", {"selector": "#marker"})
        text = str(payload.get("value", "")) if isinstance(payload, dict) else ""
        if text:
            break
        time.sleep(0.4)
    assert "PAGE_ONE_MARKER_TEXT" in text, f"extract 没有取到标记文本: {text!r}"


def test_browser_navigate_switches_content(app: AppInstance, http_server: str) -> None:
    """`navigate` 之后页面内容应确实换了。"""
    app.cli(
        ["pane", "split-web", "--url", f"{http_server}/one.html", "--direction", "vertical"],
        timeout=30,
    )
    wait_for_browser(app)

    # 等第一页就绪。
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        payload = rpc(app, "browser.extract", {"selector": "#marker"})
        value = str(payload.get("value", "")) if isinstance(payload, dict) else ""
        if "PAGE_ONE_MARKER_TEXT" in value:
            break
        time.sleep(0.3)

    rpc(app, "browser.navigate", {"url": f"{http_server}/two.html"})

    deadline = time.monotonic() + 20
    text = ""
    while time.monotonic() < deadline:
        payload = rpc(app, "browser.extract", {"selector": "#marker"})
        text = str(payload.get("value", "")) if isinstance(payload, dict) else ""
        if "PAGE_TWO_MARKER_TEXT" in text:
            break
        time.sleep(0.3)
    assert "PAGE_TWO_MARKER_TEXT" in text, f"navigate 之后内容没换: {text!r}"


def test_browser_wait_finds_selector(app: AppInstance, http_server: str) -> None:
    """`wait` 应能等到选择器出现。"""
    app.cli(
        ["pane", "split-web", "--url", f"{http_server}/one.html", "--direction", "vertical"],
        timeout=30,
    )
    wait_for_browser(app)

    result = app.cli(
        # `wait` 的选择器是**位置参数**。
        ["browser", "wait", "#marker", "--timeout-ms", "10000"], timeout=30
    )
    assert result.returncode == 0, f"browser wait 失败: {result.stderr}"


def test_browser_wait_finds_element_inserted_by_page_script(
    app: AppInstance, http_server: str
) -> None:
    """回归：`wait` 必须能等到**页面脚本动态插入**的元素。

    旧实现在 JS 里同步忙等,占住了页面的 JS 线程 —— 页面自己的
    `setTimeout` 因此永远得不到执行机会,元素永远不出现,`wait`
    必然超时。改成 Rust 侧轮询后,两次探测之间页面线程是空闲的。
    """
    app.cli(
        [
            "pane",
            "split-web",
            "--url",
            f"{http_server}/dynamic.html",
            "--direction",
            "vertical",
        ],
        timeout=30,
    )
    wait_for_browser(app)

    result = app.cli(
        ["browser", "wait", "#late", "--timeout-ms", "10000"], timeout=30
    )
    assert result.returncode == 0, (
        f"browser wait 等不到动态插入的元素: {result.stderr}\n"
        f"--- 应用日志尾部 ---\n{app.log()[-1500:]}"
    )


def test_browser_wait_times_out_on_missing_selector(
    app: AppInstance, http_server: str
) -> None:
    """元素始终不出现时,`wait` 必须**明确超时**(而不是永远挂着)。"""
    app.cli(
        ["pane", "split-web", "--url", f"{http_server}/one.html", "--direction", "vertical"],
        timeout=30,
    )
    wait_for_browser(app)

    result = app.cli(
        ["browser", "wait", "#never-exists", "--timeout-ms", "1200"], timeout=30
    )
    assert result.returncode != 0, f"不存在的选择器不该成功: {result.stdout}"
    assert "timeout" in (result.stderr + result.stdout).lower(), (
        f"超时错误信息不明确: {result.stderr!r} / {result.stdout!r}"
    )


def test_browser_operations_do_not_crash_app(app: AppInstance, http_server: str) -> None:
    """一连串浏览器操作之后应用仍应健在。"""
    app.cli(
        ["pane", "split-web", "--url", f"{http_server}/one.html", "--direction", "vertical"],
        timeout=30,
    )
    wait_for_browser(app)

    for args in (
        ["browser", "get-url"],
        ["browser", "get-title"],
        ["browser", "extract", "--selector", "body"],
        ["browser", "scroll", "--y", "100"],
    ):
        app.cli(args, timeout=30)

    assert app.cli(["system", "ping"], timeout=15).returncode == 0
    log = app.log()
    assert "panicked at" not in log, f"浏览器操作导致应用 panic:\n{log[-2000:]}"
