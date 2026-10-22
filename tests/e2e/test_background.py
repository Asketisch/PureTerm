"""背景图 / 图像四边形通道的渲染检测。

这两条通道的 GPU 管线是**按需创建**的(见 `terminal_shader::pipeline`
的 `LazyPipeline`):背景图与 Sixel / 内联图像在多数会话里根本不会
出现,没必要让每次启动都先付一次 Metal shader 编译。

按需创建把「创建管线」挪到了第一次真正绘制的那一刻 —— 因此必须
验证那条路径确实能画出来:

- 背景图:配置一张纯色 PNG,截图后应能看到该颜色出现在窗口里;
- 关掉背景图:同一张截图里不应再出现该颜色(证明开关真的生效,
  也证明没有「残留的旧管线」在画东西)。
"""

from __future__ import annotations

import os
import signal
import struct
import subprocess
import sys
import time
import zlib
from pathlib import Path

import pytest

from conftest import Sandbox, _discover_binary

pytestmark = pytest.mark.skipif(
    sys.platform != "darwin", reason="显示层检测依赖 macOS 的 screencapture / AppleScript"
)

try:
    from PIL import Image
    import numpy as np
except ImportError:  # pragma: no cover
    Image = None
    np = None

requires_pil = pytest.mark.skipif(
    Image is None or np is None, reason="需要 Pillow 与 numpy"
)


# ── 工具 ────────────────────────────────────────────────────────────────────


def write_png(path: Path, rgb: tuple[int, int, int], size: int = 64) -> Path:
    """写一张纯色 PNG(不依赖 Pillow,保证测试自洽)。"""
    width = height = size
    raw = b"".join(b"\x00" + bytes(rgb) * width for _ in range(height))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)
    return path


def window_bounds(process_name: str = "pureterm") -> tuple[int, int, int, int] | None:
    script = (
        'tell application "System Events" to tell process "%s" '
        "to get {position, size} of window 1" % process_name
    )
    try:
        out = subprocess.run(
            ["osascript", "-e", script], capture_output=True, text=True, timeout=15
        )
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return None
    if out.returncode != 0:
        return None
    parts = [p.strip() for p in out.stdout.strip().split(",")]
    if len(parts) != 4:
        return None
    try:
        x, y, w, h = (int(p) for p in parts)
    except ValueError:
        return None
    return (x, y, w, h) if w > 0 and h > 0 else None


def capture(bounds, out_path: Path) -> Path | None:
    x, y, w, h = bounds
    try:
        r = subprocess.run(
            ["screencapture", "-x", "-o", "-R", f"{x},{y},{w},{h}", str(out_path)],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return None
    return out_path if r.returncode == 0 and out_path.exists() else None


def color_ratio(path: Path, rgb: tuple[int, int, int], tolerance: int = 26) -> float:
    """画面中与目标颜色相近的像素占比。"""
    arr = np.asarray(Image.open(path).convert("RGB")).astype(np.int16)
    target = np.array(rgb, dtype=np.int16)
    distance = np.abs(arr - target).sum(axis=2)
    return float((distance <= tolerance).mean())


class RunningApp:
    """自包含的 GUI 实例(配置先行,再启动)。"""

    def __init__(self, proc, log_path: Path, sandbox: Sandbox):
        self.proc = proc
        self.log_path = log_path
        self.sandbox = sandbox

    def log(self) -> str:
        if not self.log_path.exists():
            return ""
        return self.log_path.read_text(encoding="utf-8", errors="replace")

    def wait_for_ipc(self, timeout: float = 30.0) -> None:
        binary = _discover_binary()
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(
                    f"应用提前退出(码 {self.proc.returncode}):\n{self.log()[-3000:]}"
                )
            probe = subprocess.run(
                [str(binary), "system", "ping"],
                env=self.sandbox.env,
                cwd=str(self.sandbox.root),
                capture_output=True,
                text=True,
                timeout=10,
            )
            if probe.returncode == 0:
                return
            time.sleep(0.2)
        raise TimeoutError(f"IPC 未就绪:\n{self.log()[-3000:]}")

    def stop(self) -> None:
        if self.proc.poll() is None:
            try:
                os.killpg(os.getpgid(self.proc.pid), signal.SIGTERM)
                self.proc.wait(timeout=8)
            except Exception:
                self.proc.kill()


def launch(sandbox: Sandbox, log_name: str = "bg.log") -> RunningApp:
    """按当前沙箱配置启动一个 GUI 实例。"""
    binary = _discover_binary()
    log_path = sandbox.root / log_name
    log = open(log_path, "w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(binary)],
        env=sandbox.env,
        cwd=str(sandbox.root),
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    return RunningApp(proc, log_path, sandbox)


def wait_for_window(timeout: float = 30.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        b = window_bounds()
        if b is not None:
            return b
        time.sleep(0.3)
    return None


# ── 用例 ────────────────────────────────────────────────────────────────────


def dominant_color(path: Path) -> tuple[int, int, int]:
    """画面里占比最高的颜色。"""
    arr = np.asarray(Image.open(path).convert("RGB"))
    colors, counts = np.unique(arr.reshape(-1, 3), axis=0, return_counts=True)
    return tuple(int(v) for v in colors[int(np.argmax(counts))])


@requires_pil
def test_background_image_is_rendered(sandbox: Sandbox) -> None:
    """配置一张纯色背景图后,窗口里必须能看到那张图。

    这条用例直接覆盖「背景图管线按需创建」这条路径:管线是在第一次
    真正绘制背景图时才编译出来的 —— 如果这条路径坏了,画面会只剩
    纯色背景,这里就会失败。

    断言用「与无背景基线对比」而不是匹配绝对颜色:`screencapture`
    会套用显示器的色彩配置文件,同一张图抓出来的 RGB 与源文件并不
    逐通道相等(实测 (220,30,90) → (202,55,93))。所以比较的是
    **通道间的相对关系**(红 ≫ 绿、且红 > 蓝),这不受色彩配置文件影响。
    """
    marker_color = (220, 30, 90)
    png = write_png(sandbox.root / "bg.png", marker_color)

    # 基线:同样的界面,但不启用背景图。
    sandbox.write_config(f'[background]\nenabled = false\nimage = "{png}"\n')
    baseline_app = launch(sandbox, "baseline.log")
    try:
        baseline_app.wait_for_ipc()
        bounds = wait_for_window()
        assert bounds is not None, f"没有找到窗口:\n{baseline_app.log()[-2000:]}"
        time.sleep(1.5)
        baseline_shot = capture(bounds, sandbox.root / "baseline.png")
        assert baseline_shot is not None, "基线截图失败"
        baseline_dominant = dominant_color(baseline_shot)
    finally:
        baseline_app.stop()

    # 开启背景图。
    sandbox.write_config(
        f'[background]\nenabled = true\nimage = "{png}"\nopacity = 1.0\nmode = "stretch"\n'
    )
    app = launch(sandbox, "with_bg.log")
    try:
        app.wait_for_ipc()
        bounds = wait_for_window()
        assert bounds is not None, f"没有找到窗口:\n{app.log()[-2000:]}"
        time.sleep(2.5)

        shot = capture(bounds, sandbox.root / "with_bg.png")
        assert shot is not None, "截图失败"

        dominant = dominant_color(shot)
        assert dominant != baseline_dominant, (
            f"开启背景图后画面主色没变(仍是 {dominant})—— 背景图通道没画出来"
        )
        r, g, b = dominant
        assert r > g * 3, f"主色 {dominant} 的红通道不够突出,不像目标色 {marker_color}"
        assert r > b, f"主色 {dominant} 的红通道应高于蓝通道,不像目标色 {marker_color}"
    finally:
        app.stop()


@requires_pil
def test_background_image_disabled_leaves_no_trace(sandbox: Sandbox) -> None:
    """关闭背景图后,画面里不应再出现那个颜色。"""
    marker_color = (220, 30, 90)
    png = write_png(sandbox.root / "bg.png", marker_color)
    sandbox.write_config(
        f'[background]\nenabled = false\nimage = "{png}"\nopacity = 1.0\n'
    )

    app = launch(sandbox)
    try:
        app.wait_for_ipc()
        bounds = wait_for_window()
        assert bounds is not None, f"没有找到窗口:\n{app.log()[-2000:]}"
        time.sleep(1.5)

        shot = capture(bounds, sandbox.root / "no_bg.png")
        assert shot is not None, "截图失败"

        ratio = color_ratio(shot, marker_color)
        assert ratio < 0.01, f"背景图已关闭,画面里却仍有 {ratio:.3%} 的背景图颜色"
    finally:
        app.stop()
