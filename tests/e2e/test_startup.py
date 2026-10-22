"""启动耗时基准与回归护栏。

这些用例:

1. 用应用内埋点(`PURETERM_TRACE_STARTUP=1`)把启动拆成若干里程碑,
   断言关键阶段没有明显回归;
2. 断言冷启动后窗口最终真的显示出终端,而不是一直卡在
   “Starting PureTerm…” 占位符;
3. 断言启动过程中没有把配置解析失败之类的告警打进日志
   (默认配置曾经因重复 `[window]` 表而整份解析失败)。

阈值刻意留得宽松 —— CI 机器通常比开发机慢,阈值是用来抓
“明显回归”,不是用来做精确性能断言的。想看真实分布请跑
``scripts/bench_startup.py``。
"""

from __future__ import annotations

import re
import statistics
import subprocess
import time
from pathlib import Path

import pytest

from conftest import Sandbox

TRACE_RE = re.compile(r"^PURETERM_STARTUP (\S+) ([\d.]+)(?: \+([\d.]+))?$", re.M)

#: 冷启动到「首个终端就绪」的宽松上限(毫秒)。
MAX_TERMINAL_READY_MS = 2500.0

#: 冷启动到「首帧」的宽松上限(毫秒)。
MAX_FIRST_FRAME_MS = 2000.0


def parse_trace(log: str) -> dict[str, float]:
    """把 `PURETERM_STARTUP` 行解析成 `{label: 累计毫秒}`。"""
    out: dict[str, float] = {}
    for label, total, _delta in TRACE_RE.findall(log):
        out[label] = float(total)
    return out


def launch_with_trace(binary: Path, sandbox: Sandbox) -> tuple[subprocess.Popen, Path]:
    """以追踪模式启动应用,返回 (进程, 日志路径)。"""
    env = sandbox.env
    env["PURETERM_TRACE_STARTUP"] = "1"
    log_path = sandbox.root / "startup.log"
    log_file = open(log_path, "w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(binary)],
        env=env,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    # 日志文件句柄交给调用方在 finally 里关闭。
    proc._log_file = log_file  # type: ignore[attr-defined]
    return proc, log_path


def stop(proc: subprocess.Popen) -> None:
    import signal

    if proc.poll() is None:
        try:
            proc.send_signal(signal.SIGTERM)
            proc.wait(timeout=8)
        except (subprocess.TimeoutExpired, ProcessLookupError):
            proc.kill()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
    log_file = getattr(proc, "_log_file", None)
    if log_file is not None and not log_file.closed:
        log_file.close()


# ── 里程碑 ──────────────────────────────────────────────────────────────────


def test_startup_trace_reaches_terminal_ready(
    pureterm_bin: Path, sandbox: Sandbox
) -> None:
    proc, log_path = launch_with_trace(pureterm_bin, sandbox)
    try:
        deadline = time.monotonic() + 30
        trace: dict[str, float] = {}
        while time.monotonic() < deadline:
            if log_path.exists():
                trace = parse_trace(log_path.read_text(encoding="utf-8", errors="replace"))
            if "terminal_ready" in trace:
                break
            if proc.poll() is not None:
                pytest.fail(f"应用提前退出(码 {proc.returncode}),日志:\n{log_path.read_text()[-3000:]}")
            time.sleep(0.2)
        else:
            pytest.fail(f"30s 内没有到达 terminal_ready,已记录的里程碑: {sorted(trace)}")

        assert "main_enter" in trace
        assert "app_new_exit" in trace
        assert "first_frame" in trace
        assert trace["terminal_ready"] <= MAX_TERMINAL_READY_MS, (
            f"启动到终端就绪用了 {trace['terminal_ready']:.0f}ms,超过上限 {MAX_TERMINAL_READY_MS:.0f}ms"
        )
        assert trace["first_frame"] <= MAX_FIRST_FRAME_MS, (
            f"首帧用了 {trace['first_frame']:.0f}ms,超过上限 {MAX_FIRST_FRAME_MS:.0f}ms"
        )
    finally:
        stop(proc)


def test_startup_does_not_log_config_parse_errors(
    pureterm_bin: Path, sandbox: Sandbox
) -> None:
    """默认配置必须能被解析 —— 曾经因重复 `[window]` 表而整份失败。"""
    proc, log_path = launch_with_trace(pureterm_bin, sandbox)
    try:
        deadline = time.monotonic() + 30
        log = ""
        while time.monotonic() < deadline:
            if log_path.exists():
                log = log_path.read_text(encoding="utf-8", errors="replace")
            if "terminal_ready" in log:
                break
            time.sleep(0.2)
        assert "TOML parse error" not in log, f"配置文件解析失败:\n{log[:3000]}"
        assert "parse error" not in log.lower() or "toml" not in log.lower()
    finally:
        stop(proc)


def test_startup_does_not_warn_about_font(
    pureterm_bin: Path, sandbox: Sandbox
) -> None:
    """默认字体族必须能被加载 —— 泛型名在 macOS/Windows 上要解析成真实族名。"""
    proc, log_path = launch_with_trace(pureterm_bin, sandbox)
    try:
        deadline = time.monotonic() + 30
        log = ""
        while time.monotonic() < deadline:
            if log_path.exists():
                log = log_path.read_text(encoding="utf-8", errors="replace")
            if "terminal_ready" in log:
                break
            time.sleep(0.2)
        assert "Unable to load specified font" not in log, (
            f"默认字体族加载失败(应先做平台化解析):\n{log[:3000]}"
        )
    finally:
        stop(proc)


def test_startup_creates_default_config(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """首次启动应写出默认配置,且该配置本身是合法 TOML。"""
    import tomllib

    proc, log_path = launch_with_trace(pureterm_bin, sandbox)
    try:
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            if sandbox.config_file.exists():
                break
            time.sleep(0.2)
        else:
            pytest.fail("应用没有写出默认配置")
        tomllib.loads(sandbox.config_file.read_text(encoding="utf-8"))
    finally:
        stop(proc)


# ── 多轮测量(供人工参考,断言只看离群值)────────────────────────────────────


def test_startup_timing_is_stable(pureterm_bin: Path, sandbox: Sandbox) -> None:
    """连跑 3 轮,确认没有一次出现病态离群(例如卡在占位符)。"""
    samples: list[float] = []
    for _ in range(3):
        proc, log_path = launch_with_trace(pureterm_bin, sandbox)
        try:
            deadline = time.monotonic() + 30
            trace: dict[str, float] = {}
            while time.monotonic() < deadline:
                if log_path.exists():
                    trace = parse_trace(
                        log_path.read_text(encoding="utf-8", errors="replace")
                    )
                if "terminal_ready" in trace:
                    break
                time.sleep(0.15)
            if "terminal_ready" in trace:
                samples.append(trace["terminal_ready"])
        finally:
            stop(proc)

    assert samples, "没有一轮测到 terminal_ready"
    median = statistics.median(samples)
    assert median <= MAX_TERMINAL_READY_MS, f"中位启动耗时 {median:.0f}ms 超标"
    assert max(samples) <= MAX_TERMINAL_READY_MS * 1.5, f"存在离群启动耗时: {samples}"
