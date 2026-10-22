"""MCP stdio 服务器端到端测试。

`pureterm mcp serve` 用 rmcp 的 stdio 传输(换行分隔的 JSON-RPC)。
这里模拟一个最小 MCP 客户端:握手 → 列工具 → 调用工具,并验证
工具调用确实桥接到了运行中的 GUI 实例。
"""

from __future__ import annotations

import json
import subprocess
import threading
import time
from pathlib import Path
from typing import Any

import pytest

from conftest import AppInstance, Sandbox

PROTOCOL_VERSION = "2024-11-05"


class McpClient:
    """最小的 MCP stdio 客户端。"""

    def __init__(self, proc: subprocess.Popen) -> None:
        self.proc = proc
        self._next_id = 0
        self._responses: dict[int, dict[str, Any]] = {}
        self._lock = threading.Lock()
        self._reader = threading.Thread(target=self._read_loop, daemon=True)
        self._reader.start()

    def _read_loop(self) -> None:
        assert self.proc.stdout is not None
        for line in self.proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            with self._lock:
                msg_id = msg.get("id")
                if isinstance(msg_id, int):
                    self._responses[msg_id] = msg

    def send(self, method: str, params: Any = None, notify: bool = False) -> int:
        self._next_id += 1
        msg: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if not notify:
            msg["id"] = self._next_id
        if params is not None:
            msg["params"] = params
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()
        return self._next_id

    def wait_for(self, msg_id: int, timeout: float = 20.0) -> dict[str, Any]:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                if msg_id in self._responses:
                    return self._responses.pop(msg_id)
            if self.proc.poll() is not None:
                raise RuntimeError(f"MCP 服务器提前退出(码 {self.proc.returncode})")
            time.sleep(0.05)
        raise TimeoutError(f"等待 MCP 响应 {msg_id} 超时({timeout}s)")

    def close(self) -> None:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=5)


@pytest.fixture
def mcp_server(sandbox: Sandbox, pureterm_bin: Path):
    """启动一个 `pureterm mcp serve` 进程。"""
    proc = subprocess.Popen(
        [str(pureterm_bin), "mcp", "serve"],
        env=sandbox.env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    client = McpClient(proc)
    try:
        yield client
    finally:
        client.close()


def initialize(client: McpClient) -> dict[str, Any]:
    """完成 MCP 握手。"""
    msg_id = client.send(
        "initialize",
        {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "pureterm-e2e", "version": "1.0.0"},
        },
    )
    response = client.wait_for(msg_id)
    client.send("notifications/initialized", notify=True)
    return response


# ── 握手 ────────────────────────────────────────────────────────────────────


def test_initialize_handshake(mcp_server: McpClient) -> None:
    response = initialize(mcp_server)
    assert "result" in response, f"initialize 没有返回 result: {response}"
    result = response["result"]
    assert "protocolVersion" in result
    assert "serverInfo" in result
    assert result["serverInfo"]["name"], "serverInfo.name 为空"


def test_tools_list_returns_expected_tools(mcp_server: McpClient) -> None:
    initialize(mcp_server)
    msg_id = mcp_server.send("tools/list")
    response = mcp_server.wait_for(msg_id)
    assert "result" in response, f"tools/list 失败: {response}"
    tools = response["result"]["tools"]
    names = {t["name"] for t in tools}
    for expected in ("list_surfaces", "capture_pane", "send_text", "new_tab", "split_pane"):
        assert expected in names, f"工具名册缺少 {expected};实际有 {sorted(names)}"


def test_tool_schema_has_input_schema(mcp_server: McpClient) -> None:
    initialize(mcp_server)
    msg_id = mcp_server.send("tools/list")
    tools = mcp_server.wait_for(msg_id)["result"]["tools"]
    for tool in tools:
        assert "inputSchema" in tool, f"工具 {tool['name']} 缺少 inputSchema"
        assert tool["inputSchema"].get("type") == "object"


def test_unknown_method_returns_error(mcp_server: McpClient) -> None:
    initialize(mcp_server)
    msg_id = mcp_server.send("definitely/not/a/method")
    response = mcp_server.wait_for(msg_id)
    assert "error" in response, f"未知方法应返回 error,实际: {response}"


# ── 工具调用(需要 GUI 实例)────────────────────────────────────────────────


def test_list_surfaces_tool_calls_gui(app: AppInstance, mcp_server: McpClient) -> None:
    """MCP 工具应能通过 socket 桥接到运行中的 GUI。"""
    initialize(mcp_server)
    msg_id = mcp_server.send(
        "tools/call",
        {"name": "list_surfaces", "arguments": {}},
    )
    response = mcp_server.wait_for(msg_id)
    assert "result" in response, f"tools/call 失败: {response}"
    text = response["result"].get("content", [{}])[0].get("text", "")
    assert "pane_id" in text, f"list_surfaces 的返回里没有 pane_id: {text[:400]}"


def test_send_text_and_capture_pane_roundtrip(app: AppInstance, mcp_server: McpClient) -> None:
    """send_text → capture_pane 的完整回路。"""
    initialize(mcp_server)

    msg_id = mcp_server.send(
        "tools/call",
        {"name": "send_text", "arguments": {"text": "echo MCP_ROUNDTRIP_MARKER"}},
    )
    response = mcp_server.wait_for(msg_id)
    assert "result" in response, f"send_text 失败: {response}"

    deadline = time.monotonic() + 20
    seen = False
    while time.monotonic() < deadline:
        msg_id = mcp_server.send(
            "tools/call",
            {"name": "capture_pane", "arguments": {}},
        )
        response = mcp_server.wait_for(msg_id)
        text = response["result"].get("content", [{}])[0].get("text", "")
        if "MCP_ROUNDTRIP_MARKER" in text:
            seen = True
            break
        time.sleep(0.3)
    assert seen, "capture_pane 始终没有读到 send_text 写入的标记"


def test_tool_error_is_reported(mcp_server: McpClient) -> None:
    """没有 GUI 实例时,工具调用应报告错误而不是崩溃。"""
    initialize(mcp_server)
    msg_id = mcp_server.send(
        "tools/call",
        {"name": "list_surfaces", "arguments": {}},
    )
    response = mcp_server.wait_for(msg_id)
    # rmcp 把工具错误放在 result.isError 里。
    result = response.get("result", {})
    if result.get("isError"):
        text = result.get("content", [{}])[0].get("text", "")
        assert "connect" in text.lower() or "socket" in text.lower() or "pureterm" in text.lower()
    else:
        pytest.fail(f"没有 GUI 时 list_surfaces 不应成功,实际: {response}")
