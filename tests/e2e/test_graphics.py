"""图形通道(Sixel / Kitty)的端到端检测。

图形链路很长:PTY 输出 → 终端内核解析图形转义 → `Event::GraphicsImage`
→ 图像存储 → 后台解码 worker → GPU 图集 → 绘制到屏幕。任何一环断了,
屏幕上就什么都看不到。

因此这里用**截图断言**来覆盖整条链:

1. 往窗格的 PTY 写一段 **Kitty 图形**转义(终端内核会完整解码出
   RGBA 像素,不需要 `img2sixel` 之类的外部工具);
2. 等图像被解码、上传到 GPU 并绘制;
3. 抓窗口截图,断言画��上出现了那块纯色区域。

Kitty 序列格式(见 alacritty 内核的 kitty 处理器)::

    ESC _ G a=T,f=32,i=<id>,c=<列>,r=<行>;<base64 RGBA> ESC \\
"""

from __future__ import annotations

import base64
import os
import sys
import time
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

#: 图像边长(像素)。
#:
#: 必须是**小**的:转义序列要经 shell 执行,而 zsh 的行编辑器(ZLE)对
#: 单行长度有 512 字节上限 —— 更长的命令会被 zsh 直接丢弃,图像根本
#: 不会进入 PTY。2x2 像素配上 20x20 单元格的显示占位,画面上仍是
#: 一大块纯色,足够截图检出。
IMAGE_PIXELS = 2

#: 图像占用的单元格数(显示尺寸)。kitty 规范里 `c`/`r` 是**单元格**
#: 数而非像素数 —— 像素尺寸由载荷长度决定。
IMAGE_CELLS = 20

#: 目标颜色(纯品红)。
MARKER_RGB = (255, 0, 255)


def kitty_rgba_sequence(
    rgb: tuple[int, int, int], image_id: int = 1, cells: int = IMAGE_CELLS
) -> str:
    """构造一段 Kitty 图形转义(直接透传 RGBA 像素)。"""
    # `f=32` 是 RGBA —— 每像素 **4 字节**。少一个字节通道,尺寸就与
    # 载荷对不上,图像会被下游丢弃。
    pixels = bytes((rgb[0], rgb[1], rgb[2], 255)) * (IMAGE_PIXELS * IMAGE_PIXELS)
    payload = base64.b64encode(pixels).decode("ascii")
    return (
        f"\x1b_Ga=T,f=32,i={image_id},c={cells},r={cells};"
        f"{payload}\x1b\\"
    )


def shell_command_for(sequence: str) -> str:
    """把转义序列包成一条可由 shell 执行的命令(带回车)。

    关键:序列里的 **ESC 字节必须变成文本的 `\\033`** —— 直接把真实
    ESC 字节塞进命令行,zsh 的行编辑器会把它当成一次按键,命令被
    截断,终端内核根本收不到图形序列。文本 `\\033` 则由 printf
    在执行时转成真正的 ESC。
    """
    # 顺序要紧:先转义已有的反斜杠,再把 ESC 换成文本 `\033`
    # (这个反斜杠是 printf 需要的转义标记,不能再被加倍)。
    escaped = sequence.replace("\\", "\\\\").replace("\x1b", "\\033")
    escaped = escaped.replace("'", "'\\''")
    return f"printf '{escaped}'\r"


def window_bounds(pid: int) -> tuple[int, int, int, int] | None:
    """取指定进程窗口的坐标(按 pid 定位,避免抓到别的实例)。"""
    from conftest import window_bounds_for_pid

    return window_bounds_for_pid(pid)


def capture(bounds, out_path: Path) -> Path | None:
    import subprocess

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


def changed_ratio(before: Path, after: Path, tolerance: int = 40) -> float:
    """两张截图之间「明显变化」的像素占比。

    这是最稳健的判据:图像出现的位置**必然**与基线不同,而且完全不
    受色彩空间影响。

    为什么不用「匹配绝对颜色」:`screencapture` 会套用显示器的色彩
    配置文件(广色域 → sRGB),同一块纯色抓出来的值与设定值可以差很
    多 —— 实测纯品红 (255,0,255) 抓成 (234,52,247),纯绿
    (0,255,0) 抓成 (118,251,77)(红通道被抬了 118)。绝对色匹配在
    饱和色上非常脆弱。
    """
    a = np.asarray(Image.open(before).convert("RGB")).astype(np.int16)
    b = np.asarray(Image.open(after).convert("RGB")).astype(np.int16)
    if a.shape != b.shape:
        return 0.0
    diff = np.abs(a - b).sum(axis=2)
    return float((diff > tolerance).mean())


def magenta_dominant_ratio(path: Path) -> float:
    """画面中「品红占主导」的像素占比。

    只看通道间的相对关系(红蓝都高、绿低),不看绝对值 —— 广色域转换
    会整体改变数值,但不会改变「谁是主导通道」。
    """
    arr = np.asarray(Image.open(path).convert("RGB")).astype(np.int16)
    r, g, b = arr[..., 0], arr[..., 1], arr[..., 2]
    dom = (r > 110) & (b > 110) & (g + 60 < r) & (g + 60 < b)
    return float(dom.mean())


def green_dominant_ratio(path: Path) -> float:
    """画面中「绿色占主导」的像素占比(只看通道相对关系)。"""
    arr = np.asarray(Image.open(path).convert("RGB")).astype(np.int16)
    r, g, b = arr[..., 0], arr[..., 1], arr[..., 2]
    dom = (g > 110) & (g > r + 50) & (g > b + 50)
    return float(dom.mean())


class RunningApp:
    """自包含的 GUI 实例(配置先行,再启动)。"""

    def __init__(self, proc, log_path: Path, sandbox: Sandbox):
        self.proc = proc
        self.log_path = log_path
        self.sandbox = sandbox
        self.binary = _discover_binary()

    def log(self) -> str:
        if not self.log_path.exists():
            return ""
        return self.log_path.read_text(encoding="utf-8", errors="replace")

    def cli(self, args: list[str], timeout: float = 20.0):
        import subprocess

        return subprocess.run(
            [str(self.binary), *args],
            env=self.sandbox.env,
            cwd=str(self.sandbox.root),
            capture_output=True,
            text=True,
            timeout=timeout,
        )

    def wait_for_ipc(self, timeout: float = 30.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(
                    f"应用提前退出(码 {self.proc.returncode}):\n{self.log()[-2000:]}"
                )
            if self.cli(["system", "ping"], timeout=10).returncode == 0:
                break
            time.sleep(0.2)
        else:
            raise TimeoutError(f"IPC 未就绪:\n{self.log()[-2000:]}")

    def wait_for_terminal(self, timeout: float = 30.0) -> None:
        """等到第一个终端窗格建立。

        IPC 就绪 ≠ 终端就绪:窗口先显示 “Starting PureTerm…” 占位符,
        守护进程就绪后才创建真正的 PTY 窗格。在那之前发文本会被
        **静默丢弃**(没有活动窗格)。
        """
        import json

        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            probe = self.cli(["--json", "surface", "list"], timeout=10)
            if probe.returncode == 0:
                try:
                    data = json.loads(probe.stdout or "[]")
                except json.JSONDecodeError:
                    data = []
                if data:
                    return
            time.sleep(0.15)
        raise TimeoutError(f"终端窗格未建立:\n{self.log()[-2000:]}")

    def stop(self) -> None:
        import signal

        if self.proc.poll() is None:
            try:
                os.killpg(os.getpgid(self.proc.pid), signal.SIGTERM)
                self.proc.wait(timeout=8)
            except Exception:
                self.proc.kill()


def launch(sandbox: Sandbox, log_name: str = "graphics.log") -> RunningApp:
    """按当前沙箱配置启动一个 GUI 实例。"""
    import subprocess

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


@pytest.fixture
def graphics_app(sandbox: Sandbox) -> RunningApp:
    """启用了图形通道的 GUI 实例。

    `[terminal_graphics] enabled` 默认是 **false** —— 应用默认忽略所有
    图形事件(终端输出里的 Sixel / Kitty / 内联图像)。要验证图形链路,
    必须先开启它,这也是本用例自己启动应用(而不是用 `app` fixture)
    的原因:配置必须写在启动之前。
    """
    sandbox.write_config("[terminal_graphics]\nenabled = true\nmax_images = 64\n")
    app = launch(sandbox)
    try:
        app.wait_for_ipc()
        app.wait_for_terminal()
        yield app
    finally:
        app.stop()


def wait_for_change(
    app: RunningApp,
    bounds,
    sandbox: Sandbox,
    baseline: Path,
    hue: str,
    timeout: float = 25.0,
) -> tuple[float, float]:
    """轮询截图,直到画面出现目标色块。

    返回 `(变化像素占比, 色相占比)`。判据:
    - 变化像素 —— 图像出现的位置必然与基线不同(与色彩空间无关);
    - 色相占比 —— 该色块确实是目标颜色(只看通道主导关系)。
    """
    deadline = time.monotonic() + timeout
    changed = 0.0
    hue_ratio = 0.0
    shot = None
    while time.monotonic() < deadline:
        shot = capture(bounds, sandbox.root / "poll.png")
        if shot is None:
            time.sleep(0.4)
            continue
        changed = changed_ratio(baseline, shot)
        hue_ratio = (
            magenta_dominant_ratio(shot) if hue == "magenta" else green_dominant_ratio(shot)
        )
        if changed > 0.0005 and hue_ratio > 0.0002:
            break
        time.sleep(0.4)
    assert shot is not None, "截图失败"
    return changed, hue_ratio


@requires_pil
def test_kitty_graphics_reaches_the_screen(graphics_app: RunningApp, sandbox: Sandbox) -> None:
    """Kitty 图形应真的画到屏幕上。

    覆盖整条链路:PTY → 终端内核 → `Event::GraphicsImage` → 图像存储
    → 解码 worker → GPU 图集 → 绘制。
    """
    bounds = window_bounds(graphics_app.proc.pid)
    assert bounds is not None, "没有找到 PureTerm 窗口"
    time.sleep(1.5)

    # 基线:图像出现前的画面。
    before = capture(bounds, sandbox.root / "before.png")
    assert before is not None, "基线截图失败"
    assert magenta_dominant_ratio(before) < 0.002, "基线里已经有品红色块"

    # 写入 Kitty 图形。
    # 注意:必须经 shell `printf` 发出去 —— 直接把转义序列写进 PTY 会被
    # zsh 的行编辑器吞掉(未知转义序列不回显),终端内核根本收不到。
    sequence = kitty_rgba_sequence(MARKER_RGB)
    result = graphics_app.cli(
        ["surface", "send-text", "--text", shell_command_for(sequence)]
    )
    assert result.returncode == 0, result.stderr

    # 等解码 + 上传 + 绘制。
    changed, hue_ratio = wait_for_change(
        graphics_app, bounds, sandbox, before, "magenta"
    )

    assert changed > 0.0005, (
        f"画面相对基线几乎没有变化({changed:.4%})—— "
        f"图形链路的某一环断了(PTY → 事件 → 存储 → 解码 → 图集 → 绘制)"
    )
    assert hue_ratio > 0.0002, f"变化区域不是品红色(占比 {hue_ratio:.4%})"


@requires_pil
def test_graphics_pipeline_survives_multiple_images(
    graphics_app: RunningApp, sandbox: Sandbox
) -> None:
    """连续写入多张不同 id 的图形,不应互相覆盖或让应用崩溃。"""
    bounds = window_bounds(graphics_app.proc.pid)
    assert bounds is not None, "没有找到 PureTerm 窗口"
    time.sleep(1.0)

    before = capture(bounds, sandbox.root / "before_multi.png")
    assert before is not None, "基线截图失败"

    # 单元格占位要小:每张图会把光标往下推 `cells` 行,终端只有 24 行,
    # 大图会把前一张顶出可视区。
    cells = 8

    # 1) 先发一张品红(id=1)。
    first = kitty_rgba_sequence(MARKER_RGB, image_id=1, cells=cells)
    result = graphics_app.cli(
        ["surface", "send-text", "--text", shell_command_for(first)]
    )
    assert result.returncode == 0, result.stderr
    changed, magenta_ratio = wait_for_change(
        graphics_app, bounds, sandbox, before, "magenta"
    )
    assert magenta_ratio > 0.0002, f"第一张图没有出现在画面上({magenta_ratio:.4%})"

    # 2) 用**同一个 id** 重传一张绿色的 —— kitty 客户端重传同一路径的
    #    图片很常见(重新加载、动画帧)。
    #
    #    回归背景:图集命中缓存时原本**只刷 LRU 就返回旧位置**,新像素
    #    从未上传 —— 屏幕会永远停在第一张(品红)图上。
    second = kitty_rgba_sequence((0, 255, 0), image_id=1, cells=cells)
    result = graphics_app.cli(
        ["surface", "send-text", "--text", shell_command_for(second)]
    )
    assert result.returncode == 0, result.stderr

    # 重传后画面应当出现绿色(品红被新像素替换)。
    deadline = time.monotonic() + 25
    green_ratio = 0.0
    while time.monotonic() < deadline:
        shot = capture(bounds, sandbox.root / "retransmit.png")
        if shot is None:
            time.sleep(0.4)
            continue
        green_ratio = green_dominant_ratio(shot)
        if green_ratio > 0.0002:
            break
        time.sleep(0.4)

    assert green_ratio > 0.0002, (
        f"同 id 重传后新像素没有上屏(绿色占比 {green_ratio:.4%})—— "
        f"图集命中缓存时没有重新上传像素"
    )

    # 应用还活着(没有 panic / abort)。
    assert graphics_app.cli(["system", "ping"]).returncode == 0
    log = graphics_app.log()
    assert "panicked at" not in log, f"图形路径导致应用 panic:\n{log[-2000:]}"
