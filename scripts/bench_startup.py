#!/usr/bin/env python3
"""PureTerm 冷启动耗时基准。

用应用内埋点(`PURETERM_TRACE_STARTUP=1`)把启动拆成若干阶段,
多轮测量并输出中位数与分位数,用于在改动前后做对比。

用法::

    python3 scripts/bench_startup.py                    # release 二进制,5 轮
    python3 scripts/bench_startup.py --rounds 10
    PURETERM_BIN=target/debug/pureterm python3 scripts/bench_startup.py
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
TRACE_RE = re.compile(r"^PURETERM_STARTUP (\S+) ([\d.]+)(?: \+([\d.]+))?$", re.M)

#: 关注的里程碑(按启动顺序)。
KEY_MILESTONES = [
    "main_enter",
    "app_new_enter",
    "config_loads",
    "glyph_atlas",
    "app_new_exit",
    "first_frame",
    "window_opened",
    "daemon_ready",
    "terminal_ready",
    "gpu_pipeline_enter",
    "gpu_pipelines_built",
]


def find_binary(explicit: str | None) -> Path:
    if explicit:
        p = Path(explicit).expanduser().resolve()
        if not p.exists():
            sys.exit(f"找不到二进制: {p}")
        return p
    for profile in ("release", "release-fast", "debug"):
        p = REPO_ROOT / "target" / profile / "pureterm"
        if p.exists():
            return p
    sys.exit("找不到 pureterm 二进制,请先 `cargo build --release --bin pureterm`。")


def _stop_daemon(binary: Path, env: dict[str, str], root: Path) -> None:
    """回收本轮拉起的 `puretermd`。

    守护进程**故意**在 GUI 退出后继续存活(会话要扛过 GUI 重启),它
    没有自停手段 —— 基准脚本不回收的话,每跑一轮就泄漏一个进程。
    """
    sock = Path(env.get("PURETERM_DAEMON_SOCKET", ""))
    for _ in range(5):
        try:
            subprocess.run(
                [str(binary), "daemon", "stop"],
                env=env,
                cwd=str(root),
                capture_output=True,
                timeout=15,
            )
        except (subprocess.TimeoutExpired, FileNotFoundError):
            return
        # 守护进程回完 ACK 后才真正退出(它要先杀掉自己持有的会话),
        # 给它一点时间;socket 消失即视为收工。
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if not sock.exists():
                return
            time.sleep(0.1)
    print(f"警告:守护进程没能停下({sock})", file=sys.stderr)


def one_run(binary: Path) -> dict[str, float]:
    """跑一轮冷启动,返回 `{里程碑: 毫秒}`。"""
    root = Path(tempfile.mkdtemp(prefix="pureterm-bench-"))
    try:
        home = root / "home"
        tmp = root / "tmp"
        home.mkdir()
        tmp.mkdir()

        env = dict(os.environ)
        env.update(
            {
                "HOME": str(home),
                "TMPDIR": str(tmp),
                "XDG_CONFIG_HOME": str(root / "cfg"),
                "XDG_DATA_HOME": str(root / "data"),
                "XDG_CACHE_HOME": str(root / "cache"),
                "XDG_RUNTIME_DIR": str(tmp),
                "PURETERM_SOCKET": str(tmp / "pureterm.sock"),
                "PURETERM_DAEMON_DATA_DIR": str(root / "daemon-data"),
                # 守护进程的 socket 也要隔离 —— 否则每一轮都会在**默认**
                # socket 上拉起一个守护进程,和用户自己的那个撞车。
                "PURETERM_DAEMON_SOCKET": str(tmp / "puretermd.sock"),
                "PURETERM_TRACE_STARTUP": "1",
            }
        )
        env.pop("RUST_LOG", None)

        log_path = root / "app.log"
        with open(log_path, "w", encoding="utf-8") as log:
            proc = subprocess.Popen(
                [str(binary)],
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            trace: dict[str, float] = {}
            deadline = time.monotonic() + 30
            try:
                while time.monotonic() < deadline:
                    if log_path.exists():
                        text = log_path.read_text(encoding="utf-8", errors="replace")
                        trace = {
                            m.group(1): float(m.group(2))
                            for m in TRACE_RE.finditer(text)
                        }
                    if "terminal_ready" in trace:
                        break
                    if proc.poll() is not None:
                        break
                    time.sleep(0.05)
            finally:
                if proc.poll() is None:
                    try:
                        os.killpg(os.getpgid(proc.pid), 15)
                        proc.wait(timeout=8)
                    except Exception:
                        proc.kill()
                # 守护进程**故意**在 GUI 退出后继续存活(会话要扛过
                # GUI 重启),所以基准脚本必须显式回收它 —— 否则每跑
                # 一轮就泄漏一个 `puretermd`。
                _stop_daemon(binary, env, root)
        return trace
    finally:
        shutil.rmtree(root, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description="PureTerm 冷启动耗时基准")
    parser.add_argument("--rounds", type=int, default=5, help="测量轮数(默认 5)")
    parser.add_argument("--bin", default=None, help="指定二进制路径")
    args = parser.parse_args()

    binary = find_binary(args.bin)
    print(f"二进制: {binary}")
    print(f"轮数:   {args.rounds}\n")

    runs: list[dict[str, float]] = []
    for i in range(args.rounds):
        trace = one_run(binary)
        runs.append(trace)
        ready = trace.get("terminal_ready")
        print(f"  第 {i + 1} 轮: terminal_ready = {ready:.0f}ms" if ready else f"  第 {i + 1} 轮: 失败")

    if not runs:
        print("\n没有成功的测量。")
        return 1

    print("\n各阶段耗时(毫秒):")
    print(f"  {'里程碑':<22}{'中位数':>10}{'最小':>10}{'最大':>10}")
    print("  " + "-" * 52)
    for label in KEY_MILESTONES:
        values = [r[label] for r in runs if label in r]
        if not values:
            continue
        print(
            f"  {label:<22}{statistics.median(values):>10.1f}"
            f"{min(values):>10.1f}{max(values):>10.1f}"
        )

    ready = [r["terminal_ready"] for r in runs if "terminal_ready" in r]
    if ready:
        print(f"\n冷启动 → 终端就绪: 中位 {statistics.median(ready):.0f}ms")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
