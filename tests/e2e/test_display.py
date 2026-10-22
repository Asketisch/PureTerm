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


def window_bounds(
    process_name: str = "pureterm", pid: int | None = None
) -> tuple[int, int, int, int] | None:
    """用 AppleScript 取窗口的 (x, y, w, h)。取不到返回 None。"""
    if pid is not None:
        script = (
            "tell application \"System Events\" to tell "
            f"(first process whose unix id is {int(pid)}) "
            "to get {position, size} of window 1"
        )
    else:
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


def _process_clause(pid: int | None) -> str:
    """System Events 里定位 PureTerm 进程的子句。

    同一台机器上可能同时有多个 `pureterm` 进程(不同沙箱各起一个),
    按名字取会拿到**别的**实例的窗口 —— 因此优先按 unix id 精确指定。
    """
    if pid is not None:
        return f'(first process whose unix id is {int(pid)})'
    return 'process "pureterm"'


def activate_pureterm(pid: int | None = None) -> None:
    """把 PureTerm 置前 —— 否则区域截图可能抓到压在上面的其它窗口。

    透明窗口 + 全屏内容视图(overlay 标题栏)之后,窗口的取景区域更容易
    与其它窗口重叠,置前是可靠前提。
    """
    script = (
        "tell application \"System Events\" to tell "
        f"{_process_clause(pid)} to set frontmost to true"
    )
    try:
        subprocess.run(
            ["osascript", "-e", script],
            capture_output=True,
            timeout=10,
        )
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass


def wait_for_window(process_name: str = "pureterm", timeout: float = 30.0, pid: int | None = None):
    """等到窗口出现,返回 (x, y, w, h)。`pid` 精确指定实例。"""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        bounds = window_bounds(process_name, pid=pid)
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


def _sandbox_preset(sandbox: Sandbox) -> str:
    """读沙箱配置里的 `[appearance].preset`(缺省 classic)。"""
    try:
        text = sandbox.read_config()
    except OSError:
        return "classic"
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("preset"):
            if "=" in line:
                return line.split("=", 1)[1].strip().strip('"').strip("'")
    return "classic"


@requires_pil
def test_background_color_matches_config(app: AppInstance, sandbox: Sandbox) -> None:
    """窗口主色应与生效的终端背景一致。

    - `classic`(默认):`[colors.primary].background` = #1C1C1E;
    - `cyberpunk`:预设接管终端配色 → #05070F。
    """
    bounds = wait_for_window()
    assert bounds is not None, "没有找到 PureTerm 窗口"
    time.sleep(1.5)
    activate_pureterm()
    time.sleep(0.8)

    # 置前后再取屏;若仍被遮挡(均值明显偏暗)则重试。
    preset = _sandbox_preset(sandbox)
    mean = [0.0, 0.0, 0.0]
    for _ in range(10):
        shot = capture_region(bounds, sandbox.root / "window.png")
        assert shot is not None, "截图失败"
        mean = pixel_stats(shot)["mean"]
        if mean[0] > 15.0:
            break
        activate_pureterm()
        time.sleep(0.6)
    if preset == "cyberpunk":
        # cyberpunk 终端背景 #05070F = (5, 7, 15) —— 明显比 classic 更暗。
        assert mean[0] < 40, f"cyberpunk 背景红色分量 {mean[0]:.1f} 偏离预期"
        assert mean[1] < 40, f"cyberpunk 背景绿色分量 {mean[1]:.1f} 偏离预期"
        assert mean[2] < 55, f"cyberpunk 背景蓝色分量 {mean[2]:.1f} 偏离预期"
    else:
        # 默认背景 #1C1C1E = (28, 28, 30)。允许一定误差:窗口有标题栏、
        # 状态栏,均值会被拉偏。
        assert 10 <= mean[0] <= 70, f"背景红色分量 {mean[0]:.1f} 偏离预期"
        assert 10 <= mean[1] <= 70, f"背景绿色分量 {mean[1]:.1f} 偏离预期"
        assert 10 <= mean[2] <= 80, f"背景蓝色分量 {mean[2]:.1f} 偏离预期"


@requires_pil
def test_cyberpunk_palette_renders(app_factory, sandbox: Sandbox) -> None:
    """GAP-72:`preset = "cyberpunk"` 时终端 ANSI 16 色应真的变成霓虹色。

    做法:往终端打印**具名色**文本(cyan / magenta),抓真实窗口像素,
    断言出现 cyberpunk 预设的霓虹色 —— 这直接验证「ANSI 16 色注入
    `Colors[0..15]` 槽位」这条核心链路(具名色与 `38;5;N` 索引色同槽)。

    颜色容差说明:窗口是透明窗口,经 macOS 合成 + 屏幕色彩描述文件转换后,
    实测 `#00E5FF` 落点约 (105,226,251)、`#FF2E97` 落点约 (235,70,150),
    因此青色用「高 G/B、低 R」的判据,品红用逐通道容差。
    """
    app = app_factory(
        '[appearance]\npreset = "cyberpunk"\nterminal_palette = "follow"\n'
    )
    bounds = wait_for_window(pid=app.proc.pid)
    assert bounds is not None, "没有找到 PureTerm 窗口"

    # 窗口置前 —— 否则区域截图可能抓到压在上面的其它窗口。
    activate_pureterm(app.proc.pid)
    time.sleep(1.0)

    # 打印霓虹青 + 霓虹品红(具名色路径)。
    colored = (
        "printf '\\e[36mNEONCYAN\\e[0m \\e[35mNEONMAGENTA\\e[0m'\n"
    )

    def ratios(arr: "np.ndarray") -> tuple[float, float]:
        red, green, blue = arr[..., 0], arr[..., 1], arr[..., 2]
        # 霓虹青:高 G/B、明显低 R(排除 classic 的 #06989A 与灰蓝)。
        cyan = (green >= 180) & (blue >= 190) & (red <= 160)
        # 霓虹品红:贴近 #FF2E97(classic 品红是 #75507B,完全不像)。
        magenta = (
            (np.abs(red - 255) <= 40)
            & (np.abs(green - 46) <= 40)
            & (np.abs(blue - 151) <= 40)
        )
        return float(cyan.mean()), float(magenta.mean())

    # shell 可能还没就绪 —— 每轮**重发**一次,直到屏幕上真的出现霓虹色。
    # (只发一次的话,文本可能在 PTY 尚未就绪时被丢掉。)
    shot = None
    cyan_ratio = magenta_ratio = 0.0
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            app.cli(["surface", "send-text", "--text", colored], check=False)
        except Exception:  # noqa: BLE001 —— 发送失败就下一轮重试
            pass
        time.sleep(1.0)
        shot = capture_region(bounds, sandbox.root / "cyberpunk.png")
        if shot is None:
            time.sleep(0.3)
            continue
        cyan_ratio, magenta_ratio = ratios(pixel_stats(shot)["arr"].astype(np.int16))
        if magenta_ratio > 0.0002 and cyan_ratio > 0.0002:
            break
        time.sleep(0.5)

    assert shot is not None, "截图失败"
    assert magenta_ratio > 0.0002, (
        f"画面里几乎没有霓虹品红像素({magenta_ratio:.6f}),cyberpunk 的 ANSI 16 色可能没生效"
    )
    assert cyan_ratio > 0.0002, (
        f"画面里几乎没有霓虹青像素({cyan_ratio:.6f}),cyberpunk 的 ANSI 16 色可能没生效"
    )


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


def traffic_light_pixels(arr: "np.ndarray") -> tuple[int, int]:
    """数一数 macOS 红绿灯的红色 / 绿色像素个数。

    红绿灯是系统画的(与主题无关),所以它们的**颜色**很稳定:
    红色约 (236,104,101)、绿色约 (102,196,102) —— 用这个特征区分
    「系统装饰还在」与「装饰已隐藏」比用位置可靠。
    """
    red, green, blue = arr[..., 0], arr[..., 1], arr[..., 2]
    red_lights = (red > 170) & (green < 140) & (blue < 140)
    green_lights = (green > 140) & (red < 140) & (blue < 140)
    return int(red_lights.sum()), int(green_lights.sum())


@requires_pil
def test_native_titlebar_keeps_system_chrome(app_factory, sandbox: Sandbox) -> None:
    """GAP-74:`titlebar = "native"` → 系统原生标题栏保留。

    macOS 上就是**红绿灯还在**。这条用例守的是「三态语义」:
    `native` 不该偷偷把系统装饰也关掉(unified 才关)。
    """
    app = app_factory('[window]\ntitlebar = "native"\n')
    bounds = wait_for_window(pid=app.proc.pid)
    assert bounds is not None, "没有找到 PureTerm 窗口"
    x, y, w, h = bounds

    activate_pureterm(app.proc.pid)
    time.sleep(1.5)

    corner = capture_region((x, y, 200, 40), sandbox.root / "native_corner.png")
    assert corner is not None, "窗口左上角截图失败"
    red, green = traffic_light_pixels(pixel_stats(corner)["arr"].astype(np.int16))
    assert red > 40 and green > 40, (
        f"native 模式下没看到系统红绿灯(红 {red} / 绿 {green} 像素)—— "
        "系统标题栏被误关了"
    )


@requires_pil
def test_unified_titlebar_renders(app_factory, sandbox: Sandbox) -> None:
    """GAP-74:`titlebar = "unified"` → 应用自绘标题栏真的画出来了。

    断言三件事(全部是「用户眼睛看到的东西」):

    1. 窗口顶部 28px 是**自绘的标题栏**,底色与终端底色**不同** ——
       不是系统灰条,也不是终端背景;
    2. 右上角有**三个窗口按钮字形**(─ □ ✕);
    3. 字形**居中**在各自的 38px 按钮里 —— iced 0.14 的 `Button` 会把
       内容摆到盒子左上角(`layout::padded` → `content.move_to(...)`),
       不修的话字形会贴着按钮左缘,既难看也让人按「右缘」去点时落空。
    """
    app = app_factory(
        '[appearance]\npreset = "cyberpunk"\n'
        '[window]\ntitlebar = "unified"\n'
    )
    bounds = wait_for_window(pid=app.proc.pid)
    assert bounds is not None, "没有找到 PureTerm 窗口"
    x, y, w, h = bounds

    activate_pureterm(app.proc.pid)
    time.sleep(1.5)

    # unified = 隐藏系统装饰 → macOS 的红绿灯必须**消失**
    # (工具型窗口相反:设置窗口保留系统边框,见 test_native_* )。
    corner = capture_region((x, y, 200, 40), sandbox.root / "unified_corner.png")
    assert corner is not None, "窗口左上角截图失败"
    red, green = traffic_light_pixels(pixel_stats(corner)["arr"].astype(np.int16))
    assert red < 40 and green < 40, (
        f"unified 模式下仍看到系统红绿灯(红 {red} / 绿 {green} 像素)—— "
        "系统装饰没有隐藏"
    )

    bar = capture_region((x, y, w, 28), sandbox.root / "unified_bar.png")
    assert bar is not None, "标题栏区域截图失败"
    # 终端区(往下 200px)取样,用来对比底色 —— 标题栏底色必须与它不同。
    term = capture_region((x, y + 200, 400, 120), sandbox.root / "unified_term.png")
    assert term is not None, "终端区域截图失败"

    bar_arr = pixel_stats(bar)["arr"].astype(np.int16)
    term_arr = pixel_stats(term)["arr"].astype(np.int16)
    bar_mean = bar_arr.reshape(-1, 3).mean(axis=0)
    term_mean = term_arr.reshape(-1, 3).mean(axis=0)
    distance = float(np.linalg.norm(bar_mean - term_mean))
    assert distance > 12, (
        f"标题栏底色 {bar_mean.round(1)} 与终端底色 {term_mean.round(1)} 太接近,"
        "自绘标题栏可能没画出来"
    )

    # 三个窗口按钮字形:标题栏右端 3 × 38 逻辑 px。
    height, width = bar_arr.shape[0], bar_arr.shape[1]
    scale = width / w  # 设备像素 / 逻辑像素(Retina = 2)
    bright = bar_arr.sum(axis=2) > 330
    columns = np.nonzero(bright.any(axis=0))[0]
    assert len(columns), "标题栏右侧没有任何亮像素(窗口按钮没画出来?)"

    clusters: list[tuple[int, int]] = []
    start = prev = int(columns[0])
    for column in columns[1:]:
        column = int(column)
        if column - prev > 12:
            clusters.append((start, prev))
            start = column
        prev = column
    clusters.append((start, prev))

    # 只看右端的三个按钮(标题文字在左边)。
    button_area = width - int(3 * 38 * scale)
    button_clusters = [c for c in clusters if c[0] >= button_area]
    assert len(button_clusters) == 3, (
        f"标题栏右端应有 3 个窗口按钮字形(─ □ ✕),实际 {len(button_clusters)} 个:"
        f"{button_clusters}"
    )

    # 居中:最右字形(✕)的右缘距窗口右缘应约 17 逻辑 px;
    # 左对齐(iced 的坑)会是 ~32 逻辑 px。
    gap_logical = (width - button_clusters[-1][1]) / scale
    assert gap_logical < 24, (
        f"✕ 字形距窗口右缘 {gap_logical:.1f} 逻辑 px —— 字形没居中"
        "(iced 0.14 的 Button 会把内容摆到左上角)"
    )


def test_app_log_has_no_panic(app: AppInstance) -> None:
    """应用日志里不应出现 panic。"""
    log = app.log()
    assert "panicked at" not in log, f"应用日志里有 panic:\n{log[-3000:]}"
