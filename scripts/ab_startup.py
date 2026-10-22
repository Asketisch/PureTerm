#!/usr/bin/env python3
"""启动耗时 A/B 对照。

外部测量「进程启动 → 终端真正可用(`surface list` 返回非空)」,
两个二进制交替运行,以抵消 macOS Metal shader 缓存冷热带来的偏差。

用法::

    python3 scripts/ab_startup.py /tmp/pureterm-base /tmp/pureterm-new --rounds 6
"""

from __future__ import annotations

import argparse
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def _find_socket(tmp: Path) -> Path | None:
    """在 TMPDIR 里找 GUI 的 IPC socket。"""
    for candidate in sorted(tmp.glob("PureTerm-*.sock")):
        return candidate
    return None


def _stop_daemon(binary: Path, env: dict[str, str], root: Path) -> None:
    """回收本轮拉起的 `puretermd`(它不会自己退出)。"""
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
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if not sock.exists():
                return
            time.sleep(0.1)
    print(f"警告:守护进程没能停下({sock})", file=sys.stderr)


def _socket_ready(tmp: Path) -> bool:
    """socket 是否已能应答 `surface.list` 且有内容。"""
    path = _find_socket(tmp)
    if path is None:
        return False
    try:
        with socket_mod.socket(socket_mod.AF_UNIX, socket_mod.SOCK_STREAM) as sock:
            sock.settimeout(2.0)
            sock.connect(str(path))
            sock.sendall(b'{"jsonrpc":"2.0","id":1,"method":"surface.list"}\n')
            sock.shutdown(socket_mod.SHUT_WR)
            chunks = []
            while True:
                data = sock.recv(65536)
                if not data:
                    break
                chunks.append(data)
    except OSError:
        return False
    raw = b"".join(chunks).decode().strip()
    if not raw:
        return False
    try:
        payload = json.loads(raw.splitlines()[0])
    except json.JSONDecodeError:
        return False
    result = payload.get("result")
    return isinstance(result, list) and len(result) > 0


def measure(binary: Path, timeout: float = 40.0) -> float | None:
    """返回「启动到终端可用」的秒数;失败返回 None。"""
    root = Path(tempfile.mkdtemp(prefix="pureterm-ab-"))
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
                # 守护进程 socket 也要隔离:否则每一轮都在**默认** socket
                # 上拉起守护进程,和用户自己的那个撞车。
                "PURETERM_DAEMON_SOCKET": str(tmp / "puretermd.sock"),
            }
        )
        env.pop("RUST_LOG", None)

        log_path = root / "app.log"
        with open(log_path, "w", encoding="utf-8") as log:
            t0 = time.monotonic()
            proc = subprocess.Popen(
                [str(binary)],
                env=env,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
            )
            try:
                deadline = t0 + timeout
                while time.monotonic() < deadline:
                    if proc.poll() is not None:
                        return None
                    # 直接轮询 IPC socket —— 不走 CLI 子进程。
                    # 每次 CLI 调用要 fork+exec 一次(~30-50ms),会把
                    # 测量本身的开销算进启动耗时,掩盖真实差异。
                    if _socket_ready(tmp):
                        return time.monotonic() - t0
                    time.sleep(0.005)
                return None
            finally:
                if proc.poll() is None:
                    try:
                        os.killpg(os.getpgid(proc.pid), 15)
                        proc.wait(timeout=8)
                    except Exception:
                        proc.kill()
                # 守护进程**故意**在 GUI 退出后继续存活(会话要扛过 GUI
                # 重启),没有自停手段 —— 不回收就每轮泄漏一个进程。
                _stop_daemon(binary, env, root)
    finally:
        shutil.rmtree(root, ignore_errors=True)


import json  # noqa: E402  (放在函数外以便上面直接使用)
import socket as socket_mod  # noqa: E402


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binaries", nargs="+", help="待比较的二进制")
    parser.add_argument("--rounds", type=int, default=5)
    args = parser.parse_args()

    bins = [Path(b).resolve() for b in args.binaries]
    for b in bins:
        if not b.exists():
            sys.exit(f"找不到二进制: {b}")

    # 预热:每个二进制先跑一次,把 Metal shader 缓存热起来。
    print("预热中…")
    for b in bins:
        measure(b)
        print(f"  已预热 {b.name}")

    results: dict[Path, list[float]] = {b: [] for b in bins}
    for r in range(args.rounds):
        for b in bins:
            t = measure(b)
            if t is None:
                print(f"  第 {r + 1} 轮 {b.name}: 失败")
            else:
                results[b].append(t)
                print(f"  第 {r + 1} 轮 {b.name}: {t * 1000:.0f}ms")

    print("\n启动到终端可用(毫秒):")
    base = None
    for b in bins:
        vals = results[b]
        if not vals:
            continue
        med = statistics.median(vals) * 1000
        if base is None:
            base = med
        delta = f"  ({med - base:+.0f}ms vs 首个)" if len(bins) > 1 else ""
        print(
            f"  {b.name:<24} 中位 {med:>7.0f}  最小 {min(vals) * 1000:>7.0f}"
            f"  最大 {max(vals) * 1000:>7.0f}{delta}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
