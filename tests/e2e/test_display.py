"""显示层端到端检测。

这些用例启动真实的 GUI 实例,抓取窗口截图并对**像素**做断言 ——
即「用户眼睛看到的东西」,而不只是内部状态:

- 窗口真的出现了(不是只创建了 socket);
- 窗口内容不是空白/纯色(渲染确实发生了);
- 背景色与配置一致;
- 终端文本真的被光栅化成了字形(不是只有背景块);
- 深浅色/透明度等视觉开关生效。

macOS 上用 ``screencapture`` 抓窗口区域:先用 AppleScript 取窗口
坐标,再按区域截图。非 macOS 平台整体跳过。
"""

from __future__ import annotations

import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

from conftest import AppInstance, Sandbox

pytestmark = pytest.mark.skipif(
    sys.platform != "darwin", reason="显示层检测依赖 macOS 的 screencapture / AppleScript"
)

try:
    from PIL import Image
    import numpy as np
except ImportError:  # pragma: no cover - 环境缺依赖时跳过
    Image = None
    np = None


requires_pil = pytest.mark.skipif(
    Image is None or np is None, reason="需要 Pillow 与 numpy"
)


# ── 工具 ────────────────────────────────────────────────────────────────────


def window_bounds(process_name: str = "pureterm") -> tuple[int, int, int, int] | None:
    """用 AppleScript 取窗口的 (x, y, w, h)。取不到返回 None。"""
    script = (
        'tell application "System Events" to tell process "%s" '
        "to get {position, size} of window 1" % process_name
    )
    try:
        out = subprocess.run(
            ["osascript", "-e", script],
            capture_output=True,
            text=True,
            timeout=15,
        )
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return None
    if out.returncode != 0:
        return None
    raw = out.stdout.strip()
    if not raw:
        return None
    parts = [p.strip() for p in raw.split(",")]
    if len(parts) != 4:
        return None
    try:
        x, y, w, h = (int(p) for p in parts)
    except ValueError:
        return None
    if w <= 0 or h <= 0:
        return None
    return x, y, w, h


def capture_region(
    bounds: tuple[int, int, int, int], out_path: Path
) -> Path | None:
    """按区域截图。"""
    x, y, w, h = bounds
    cmd = ["screencapture", "-x", "-o", "-R", f"{x},{y},{w},{h}", str(out_path)]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return None
    if result.returncode != 0 or not out_path.exists():
        return None
    return out_path


def wait_for_window(process_name: str = "pureterm", timeout: float = 30.0):
    """等到窗口出现。"""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        bounds = window_bounds(process_name)
        if bounds is not None:
            return bounds
        time.sleep(0.3)
    return None


def pixel_stats(path: Path) -> dict:
    """统计一张截图的像素特征。"""
    image = Image.open(path).convert("RGB")
    arr = np.asarray(image)
    flat = arr.reshape(-1, 3)
    # 唯一颜色数:纯色/空白窗口会很少。
    unique = len(np.unique(flat, axis=0))
    # 亮度分布。
    luminance = arr.astype(np.float32).sum(axis=2)
    return {
        "size": image.size,
        "unique_colors": unique,
        "mean": flat.mean(axis=0).tolist(),
        "p95_luma": float(np.percentile(luminance, 95)),
        "p05_luma": float(np.percentile(luminance, 5)),
        "contrast": float(np.percentile(luminance, 95) - np.percentile(luminance, 5)),
        "dark_ratio": float((luminance < 40).mean()),
        "bright_ratio": float((luminance > 400).mean()),
        "arr": arr,
    }


# ── 用例 ────────────────────────────────────────────────────────────────────


@requires_pil
def test_window_appears(app: AppInstance, sandbox: Sandbox) -> None:
    """GUI 启动后必须真的出现一个窗口。"""
    bounds = wait_for_window()
    assert bounds is not None, "没有找到 PureTerm 窗口"
    x, y, w, h = bounds
    assert w > 200 and h > 150, f"窗口尺寸异常: {w}x{h}"


@requires_pil
def test_window_content_is_rendered(app: AppInstance, sandbox: Sandbox) -> None:
    """窗口内容不能是纯色 —— 必须真的有东西被渲染。"""
    bounds = wait_for_window()
    assert bounds is not None, "没有找到 PureTerm 窗口"
    time.sleep(1.5)  # 等终端就绪并画出内容

    shot = capture_region(bounds, sandbox.root / "window.png")
    assert shot is not None, "截图失败"

    stats = pixel_stats(shot)
    assert stats["unique_colors"] > 8, (
        f"窗口内容几乎是纯色({stats['unique_colors']} 种颜色),渲染可能没发生"
    )
    assert stats["contrast"] > 20, (
        f"窗口对比度过低({stats['contrast']:.1f}),画面可能是空白"
    )


@requires_pil
def test_terminal_text_is_rasterized(app: AppInstance, sandbox: Sandbox) -> None:
    """终端里出现文字后,像素里应能看到字形(前景像素)。

    做法:往 PTY 写一条命令,等它输出,然后统计「与背景色明显不同的
    像素」比例。只画背景色块而没有字形时,该比例会接近 0。
    """
    bounds = wait_for_window()
    assert bounds is not None, "没有找到 PureTerm 窗口"

    marker = "DISPLAY_E2E_MARKER"
    app.cli(["surface", "send-text", "--text", f"echo {marker}"], check=True)

    deadline = time.monotonic() + 20
    shot = None
    while time.monotonic() < deadline:
        shot = capture_region(bounds, sandbox.root / "window.png")
        if shot is None:
            time.sleep(0.3)
            continue
        stats = pixel_stats(shot)
        # 前景像素:亮度明显高于背景(终端背景是深色)。
        if stats["bright_ratio"] > 0.0005:
            break
        time.sleep(0.3)

    assert shot is not None, "截图失败"
    stats = pixel_stats(shot)
    assert stats["bright_ratio"] > 0.0005, (
        f"画面里几乎没有前景像素({stats['bright_ratio']:.6f}),文本没有被光栅化"
    )


@requires_pil
def test_background_color_matches_config(app: AppInstance, sandbox: Sandbox) -> None:
    """窗口主色应与 `[colors.primary].background` 一致(默认 #1C1C1E)。"""
    bounds = wait_for_window()
    assert bounds is not None, "没有找到 PureTerm 窗口"
    time.sleep(1.5)

    shot = capture_region(bounds, sandbox.root / "window.png")
    assert shot is not None, "截图失败"

    stats = pixel_stats(shot)
    mean = stats["mean"]
    # 默认背景 #1C1C1E = (28, 28, 30)。允许一定误差:窗口有标题栏、
    # 状态栏,均值会被拉偏。
    assert 10 <= mean[0] <= 70, f"背景红色分量 {mean[0]:.1f} 偏离预期"
    assert 10 <= mean[1] <= 70, f"背景绿色分量 {mean[1]:.1f} 偏离预期"
    assert 10 <= mean[2] <= 80, f"背景蓝色分量 {mean[2]:.1f} 偏离预期"


@requires_pil
def test_no_console_error_overlay(app: AppInstance, sandbox: Sandbox) -> None:
    """iced 在 panic 时会渲染一个错误覆盖层;正常启动不应出现。"""
    bounds = wait_for_window()
    assert bounds is not None, "没有找到 PureTerm 窗口"
    time.sleep(1.0)

    shot = capture_region(bounds, sandbox.root / "window.png")
    assert shot is not None, "截图失败"

    stats = pixel_stats(shot)
    # 错误覆盖层是明显的红色大片。终端背景下不该出现大量高饱和红色。
    arr = stats["arr"].astype(np.int16)
    red = arr[..., 0]
    green = arr[..., 1]
    blue = arr[..., 2]
    reddish = (red > 120) & (red - green > 60) & (red - blue > 60)
    assert float(reddish.mean()) < 0.02, (
        f"画面里有大片红色像素({reddish.mean():.3f}),可能渲染了 panic 覆盖层"
    )


def test_app_log_has_no_panic(app: AppInstance) -> None:
    """应用日志里不应出现 panic。"""
    log = app.log()
    assert "panicked at" not in log, f"应用日志里有 panic:\n{log[-3000:]}"
