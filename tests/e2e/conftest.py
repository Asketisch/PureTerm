"""PureTerm 端到端测试的公共 fixture。

设计要点:

- **隔离**:每个测试用例拿到独立的 ``HOME`` / ``TMPDIR`` / ``XDG_*``,
  绝不读写用户真实的 ``~/.config/pureterm`` 或事件日志。
- **二进制定位**:优先 ``PURETERM_BIN`` 环境变量,其次 ``target/release``,
  最后 ``target/debug``。release 优先,因为启动耗时断言只有在
  优化构建上才有意义。
- **socket 覆盖**:GUI 的 IPC socket 由 ``PURETERM_SOCKET`` 指定到
  临时目录,守护进程的 socket 由 ``TMPDIR`` 决定 —— 测试因此
  可以同时并行跑多份实例而互不干扰。

运行::

    pytest tests/e2e -v
    PURETERM_BIN=target/release/pureterm pytest tests/e2e -v
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import pytest

# ── 常量 ────────────────────────────────────────────────────────────────────

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_TIMEOUT = 20.0


# ── 二进制定位 ──────────────────────────────────────────────────────────────


def _discover_binary() -> Path:
    """定位待测的 ``pureterm`` 二进制。"""
    env_bin = os.environ.get("PURETERM_BIN")
    if env_bin:
        candidate = Path(env_bin).expanduser().resolve()
        if not candidate.exists():
            raise pytest.UsageError(f"PURETERM_BIN points to a missing file: {candidate}")
        return candidate

    for profile in ("release", "release-fast", "debug"):
        candidate = REPO_ROOT / "target" / profile / "pureterm"
        if candidate.exists():
            return candidate

    raise pytest.UsageError(
        "找不到 pureterm 二进制。请先 `cargo build --release --bin pureterm`,"
        " 或用 PURETERM_BIN 环境变量指定路径。"
    )


# ── 隔离环境 ────────────────────────────────────────────────────────────────


@dataclass
class Sandbox:
    """一次测试用的隔离运行环境。"""

    root: Path
    home: Path
    tmp: Path

    @property
    def env(self) -> dict[str, str]:
        """子进程环境变量(完全替换,避免继承用户配置)。"""
        env = dict(os.environ)
        # 把被测二进制所在目录加进 PATH:`puretermd` 是守护进程,GUI 在
        # 冷启动时按「与主二进制同目录 → PATH」的顺序查找它。测试用的二进制
        # 通常在 target/<profile>/ 下、不在 PATH 里;不补这一项的话守护进程
        # 永远拉不起来,应用会静默退回直连 PTY(会话持久化失效)。
        bin_dir = str(_discover_binary().parent)
        path = env.get("PATH", "")
        env.update(
            {
                "HOME": str(self.home),
                "TMPDIR": str(self.tmp),
                "XDG_CONFIG_HOME": str(self.root / "xdg-config"),
                "XDG_DATA_HOME": str(self.root / "xdg-data"),
                "XDG_CACHE_HOME": str(self.root / "xdg-cache"),
                "XDG_RUNTIME_DIR": str(self.tmp),
                "PURETERM_SOCKET": str(self.tmp / "pureterm.sock"),
                "PURETERM_DAEMON_DATA_DIR": str(self.root / "daemon-data"),
                # 显式指定守护进程 socket,避免多个测试实例共享
                # `/tmp/pureterm-<uid>/puretermd.sock`。
                "PURETERM_DAEMON_SOCKET": str(self.tmp / "puretermd.sock"),
                "PATH": f"{bin_dir}{os.pathsep}{path}" if path else bin_dir,
                # 关掉 macOS 的 App Nap / 节能,避免测试机被挂起导致
                # 启动耗时测量出现几十秒的离群值。
                "NSUnbufferedIO": "YES",
            }
        )
        # 保证子进程不会因为继承了 IDE 的调试变量而行为异常。
        env.pop("RUST_LOG", None)
        return env

    @property
    def config_dir(self) -> Path:
        return self.home / ".config" / "pureterm"

    @property
    def config_file(self) -> Path:
        return self.config_dir / "pureterm.toml"

    @property
    def daemon_socket(self) -> Path:
        return self.tmp / "puretermd.sock"

    def read_config(self) -> str:
        return self.config_file.read_text(encoding="utf-8")

    def write_config(self, contents: str) -> Path:
        """写入用户配置(覆盖)。用于开启默认关闭的特性。"""
        self.config_dir.mkdir(parents=True, exist_ok=True)
        self.config_file.write_text(contents, encoding="utf-8")
        return self.config_file

    def append_config(self, contents: str) -> Path:
        """追加到用户配置(保留已有内容)。"""
        self.config_dir.mkdir(parents=True, exist_ok=True)
        existing = self.config_file.read_text(encoding="utf-8") if self.config_file.exists() else ""
        self.config_file.write_text(existing + "\n" + contents, encoding="utf-8")
        return self.config_file


@pytest.fixture
def sandbox(tmp_path: Path) -> Sandbox:
    """每用例一个隔离沙箱。

    沙箱放在 `/tmp` 下而**不是** pytest 的 `tmp_path`:pytest 的临时
    路径很长(`/private/var/folders/.../pytest-of-user/pytest-N/test_x0/`),
    会把 Unix socket 路径顶破 104 字节上限(`sockaddr_un::sun_path`),
    导致 socket 根本无法创建 —— 应用能起来,但 CLI / IPC / 守护进程
    全部静默失效。短路径让整套测试跑在真实可用的配置下。
    """
    root = Path(tempfile.mkdtemp(prefix="pt-", dir="/tmp"))
    home = root / "home"
    tmp = root / "tmp"
    home.mkdir(parents=True, exist_ok=True)
    tmp.mkdir(parents=True, exist_ok=True)
    (root / "xdg-config").mkdir(parents=True, exist_ok=True)
    try:
        yield Sandbox(root=root, home=home, tmp=tmp)
    finally:
        # 守护进程**不会自己退出**(它要扛过 GUI 重启),所以每个用到本
        # 沙箱的用例都会留下一个 —— 几百个 `puretermd` 足以把机器拖垮
        # (实测 E2E 套件耗时 85s → 345s,大批用例超时)。
        #
        # 回收放在这里而不是各个用例里:有的用例**直接** Popen 起 GUI
        # (不走 AppInstance),在沙箱层回收才能一次覆盖全部。
        stop_sandbox_daemon(root, tmp)
        shutil.rmtree(root, ignore_errors=True)


@pytest.fixture(scope="session")
def pureterm_bin() -> Path:
    """待测二进制路径(会话级)。"""
    return _discover_binary()


# ── CLI 调用助手 ────────────────────────────────────────────────────────────


@dataclass
class CliResult:
    """一次 CLI 调用的结果。"""

    args: list[str]
    returncode: int
    stdout: str
    stderr: str

    @property
    def output(self) -> str:
        return self.stdout + self.stderr

    def json(self) -> object:
        """把 stdout 解析成 JSON。"""
        return json.loads(self.stdout)

    def json_list(self) -> list:
        """把 stdout 解析成 JSON 数组。"""
        data = self.json()
        assert isinstance(data, list), f"期望 JSON 数组,实际得到 {type(data).__name__}: {self.stdout[:200]}"
        return data


def run_cli(
    args: list[str],
    sandbox: Sandbox,
    binary: Path,
    timeout: float = DEFAULT_TIMEOUT,
    stdin: str | None = None,
    check: bool = False,
) -> CliResult:
    """以隔离环境运行一次 ``pureterm`` CLI 调用。"""
    proc = subprocess.run(
        [str(binary), *args],
        env=sandbox.env,
        cwd=str(sandbox.root),
        capture_output=True,
        text=True,
        timeout=timeout,
        input=stdin,
    )
    result = CliResult(
        args=args,
        returncode=proc.returncode,
        stdout=proc.stdout,
        stderr=proc.stderr,
    )
    if check and proc.returncode != 0:
        raise AssertionError(
            f"`pureterm {' '.join(args)}` 退出码 {proc.returncode}\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    return result


# ── GUI 实例 fixture ────────────────────────────────────────────────────────


class AppInstance:
    """一个运行中的 PureTerm GUI 实例。"""

    def __init__(self, proc: subprocess.Popen, sandbox: Sandbox, binary: Path, log_path: Path):
        self.proc = proc
        self.sandbox = sandbox
        self.binary = binary
        self.log_path = log_path

    @property
    def socket(self) -> Path:
        return self.sandbox.tmp / "pureterm.sock"

    def cli(self, args: list[str], timeout: float = DEFAULT_TIMEOUT, check: bool = False) -> CliResult:
        """对本实例发一条 CLI 命令。"""
        return run_cli(args, self.sandbox, self.binary, timeout=timeout, check=check)

    def json_cli(self, args: list[str], timeout: float = DEFAULT_TIMEOUT) -> object:
        """发一条 CLI 命令并解析 JSON 输出。"""
        return self.cli(args + ["--json"], timeout=timeout, check=True).json()

    def log(self) -> str:
        """读取应用 stderr 日志。"""
        if not self.log_path.exists():
            return ""
        return self.log_path.read_text(encoding="utf-8", errors="replace")

    def wait_for_terminal(self, timeout: float = 30.0) -> None:
        """等到**第一个终端窗格**真正建立。

        IPC 就绪 ≠ 终端就绪:应用先开窗口(显示 “Starting PureTerm…”
        占位符),守护进程就绪后才创建真正的 PTY 窗格。在那之前往
        ``surface send-text`` 发文本会被**静默丢弃**(没有活动窗格),
        测试就会莫名其妙地“命令没生效”。
        """
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            probe = self.cli(["--json", "surface", "list"], timeout=10.0)
            if probe.returncode == 0:
                try:
                    data = json.loads(probe.stdout or "[]")
                except json.JSONDecodeError:
                    data = []
                if data:
                    return
            time.sleep(0.15)
        raise TimeoutError("终端窗格在超时内没有建立")

    def wait_for_ipc(self, timeout: float = 30.0) -> None:
        """等待 IPC 就绪(``system ping`` 成功)。"""
        deadline = time.monotonic() + timeout
        last_error = ""
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(
                    f"PureTerm 进程提前退出(码 {self.proc.returncode})。日志:\n{self.log()[-4000:]}"
                )
            result = self.cli(["system", "ping"], timeout=5.0)
            if result.returncode == 0:
                return
            last_error = result.stderr.strip()
            time.sleep(0.15)
        raise TimeoutError(
            f"PureTerm IPC 在 {timeout}s 内未就绪。最后错误: {last_error!r}\n"
            f"--- 应用日志 ---\n{self.log()[-4000:]}"
        )

    def terminate(self, timeout: float = 10.0) -> None:
        """优雅终止实例;超时则强杀。"""
        if self.proc.poll() is not None:
            return
        try:
            self.proc.send_signal(signal.SIGTERM)
            self.proc.wait(timeout=timeout)
        except (subprocess.TimeoutExpired, ProcessLookupError):
            try:
                self.proc.kill()
                self.proc.wait(timeout=5)
            except (subprocess.TimeoutExpired, ProcessLookupError):
                pass

    def stop_daemon(self) -> None:
        """回收这个沙箱里的 `puretermd`。

        守护进程**故意**在 GUI 退出后继续存活(会话要扛过 GUI 重启),
        所以它自己不会退出 —— 每个 E2E 用例都会留下一个。几百个
        `puretermd` 足以把机器拖垮:实测 E2E 套件耗时从 85s 涨到 345s,
        大量用例超时。

        沙箱给每个用例一个**专属**的 daemon socket(`PURETERM_DAEMON_SOCKET`),
        因此停掉它不会影响别的用例,更不会碰到用户自己的守护进程。
        """
        socket = self.sandbox.daemon_socket
        if not socket.exists():
            return
        try:
            subprocess.run(
                [str(self.binary), "daemon", "stop"],
                env=self.sandbox.env,
                cwd=str(self.sandbox.root),
                capture_output=True,
                text=True,
                timeout=15,
            )
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass


@pytest.fixture
def app(sandbox: Sandbox, pureterm_bin: Path) -> AppInstance:
    """启动一个隔离的 PureTerm GUI 实例。"""
    log_path = sandbox.root / "app.log"
    log_file = open(log_path, "w", encoding="utf-8")

    env = sandbox.env
    # 独立进程组:terminate 时能连带干掉子进程(守护/PTY)。
    proc = subprocess.Popen(
        [str(pureterm_bin)],
        env=env,
        cwd=str(sandbox.root),
        stdout=log_file,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    instance = AppInstance(proc, sandbox, pureterm_bin, log_path)
    try:
        instance.wait_for_ipc()
        instance.wait_for_terminal()
        yield instance
    finally:
        instance.terminate()
        # 守护进程不会自己退出 —— 必须显式回收,否则每个用例泄漏一个。
        instance.stop_daemon()
        log_file.close()


@pytest.fixture
def app_factory(sandbox: Sandbox, pureterm_bin: Path):
    """按需写配置再启动 GUI 的工厂。

    有些配置只在**启动时**被读一次(SSH 档案、图形开关等),测试若在
    应用起来之后才写配置就来不及了 —— 工厂把「写配置」放在启动之前。

    用法::

        app = app_factory("[[ssh.profiles]]\nname = \"x\"\nhost = \"h\"\n")
    """
    created: list[AppInstance] = []

    def make(config: str | None = None) -> AppInstance:
        if config is not None:
            sandbox.write_config(config)
        log_path = sandbox.root / "app.log"
        log_file = open(log_path, "a", encoding="utf-8")
        proc = subprocess.Popen(
            [str(pureterm_bin)],
            env=sandbox.env,
            cwd=str(sandbox.root),
            stdout=log_file,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        instance = AppInstance(proc, sandbox, pureterm_bin, log_path)
        instance._log_file = log_file  # type: ignore[attr-defined]
        try:
            instance.wait_for_ipc()
            instance.wait_for_terminal()
        except Exception:
            instance.terminate()
            log_file.close()
            raise
        created.append(instance)
        return instance

    try:
        yield make
    finally:
        for instance in created:
            instance.terminate()
            instance.stop_daemon()
            log_file = getattr(instance, "_log_file", None)
            if log_file is not None and not log_file.closed:
                log_file.close()


@pytest.fixture(scope="module")
def module_app(tmp_path_factory, pureterm_bin: Path) -> AppInstance:
    """模块级共享的 GUI 实例。

    只给**只读**用例用:每个用例都重启一次 GUI 太慢(一次冷启动约
    1-3 秒),而只读断言不需要隔离状态。会改状态的用例请用函数级的
    [`app`] fixture。
    """
    root = tmp_path_factory.mktemp("module_app")
    home = root / "home"
    tmp = root / "tmp"
    home.mkdir(parents=True, exist_ok=True)
    tmp.mkdir(parents=True, exist_ok=True)
    sandbox = Sandbox(root=root, home=home, tmp=tmp)

    log_path = root / "app.log"
    log_file = open(log_path, "w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(pureterm_bin)],
        env=sandbox.env,
        cwd=str(root),
        stdout=log_file,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    instance = AppInstance(proc, sandbox, pureterm_bin, log_path)
    try:
        instance.wait_for_ipc()
        instance.wait_for_terminal()
        yield instance
    finally:
        instance.terminate()
        instance.stop_daemon()
        log_file.close()


# ── 守护进程 fixture ────────────────────────────────────────────────────────


@pytest.fixture
def daemon(sandbox: Sandbox, pureterm_bin: Path):
    """启动一个隔离的 ``puretermd`` 守护进程。"""
    daemon_bin = pureterm_bin.parent / "puretermd"
    if not daemon_bin.exists():
        pytest.skip("puretermd 未构建;跳过守护进程测试")

    log_path = sandbox.root / "daemon.log"
    log_file = open(log_path, "w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(daemon_bin)],
        env=sandbox.env,
        cwd=str(sandbox.root),
        stdout=log_file,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    deadline = time.monotonic() + 15.0
    try:
        while time.monotonic() < deadline:
            if sandbox.daemon_socket.exists():
                break
            if proc.poll() is not None:
                raise RuntimeError(
                    f"puretermd 提前退出(码 {proc.returncode}):\n"
                    f"{log_path.read_text(encoding='utf-8', errors='replace')[-2000:]}"
                )
            time.sleep(0.1)
        else:
            raise TimeoutError("puretermd 未在 15s 内创建 socket")
        yield proc
    finally:
        if proc.poll() is None:
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
            except (ProcessLookupError, PermissionError):
                proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
        log_file.close()
        # 守护进程是本用例自己起的 —— 收尾时清掉 socket,别让下一个用例
        # 撞上「另一个实例已在运行」的静默退出路径。
        try:
            sandbox.daemon_socket.unlink()
        except OSError:
            pass


# ── 其它 ────────────────────────────────────────────────────────────────────


@pytest.fixture
def sample_project(tmp_path: Path) -> Path:
    """一个小 Python 项目,供代码索引 / LSP 相关测试使用。"""
    project = tmp_path / "sample_project"
    (project / "pkg").mkdir(parents=True, exist_ok=True)
    (project / "pkg" / "core.py").write_text(
        '"""核心模块。"""\n\n\n'
        "def calculate_total(prices):\n"
        '    """把所有价格相加。"""\n'
        "    return sum(prices)\n\n\n"
        "class Calculator:\n"
        "    def add(self, a, b):\n"
        "        return a + b\n",
        encoding="utf-8",
    )
    (project / "README.md").write_text("# Sample\n", encoding="utf-8")
    return project


def window_bounds_for_pid(pid: int, process_name: str = "pureterm"):
    """取指定 pid 的应用窗口的 (x, y, w, h);取不到返回 None。

    必须按 pid 精确定位:整个测试会话里会有多个 PureTerm 实例先后
    启动/退出,按**进程名**取窗口可能抓到上一个实例的残留窗口 ——
    于是截图拍的是别的应用,断言会莫名其妙地失败。
    """
    import subprocess

    script = (
        'tell application "System Events" to tell '
        f'(first process whose unix id is {pid}) '
        "to get {position, size} of window 1"
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


def pytest_report_header(config: pytest.Config) -> list[str]:
    """在测试报告头部显示待测二进制路径。"""
    try:
        binary = _discover_binary()
    except pytest.UsageError:
        return ["PureTerm E2E: 未找到二进制"]
    return [f"PureTerm E2E: binary = {binary}"]


def stop_sandbox_daemon(root: Path, tmp: Path) -> None:
    """回收某个沙箱里的 `puretermd`。

    守护进程**故意**在 GUI 退出后继续存活(会话要扛过 GUI 重启),但它
    **没有任何自停手段** —— 每个用例都会留下一个。几百个 `puretermd`
    足以把机器拖垮(实测 E2E 套件耗时 85s → 345s,大批用例超时)。

    沙箱给每个用例一个**专属**的 daemon socket(`PURETERM_DAEMON_SOCKET`),
    因此停掉它不会影响别的用例,更不会碰到用户自己的守护进程。

    失败无所谓:最坏情况是残留一个进程,不影响断言。
    """
    socket_path = tmp / "puretermd.sock"
    if not socket_path.exists():
        return
    try:
        binary = _discover_binary()
    except pytest.UsageError:
        return
    env = dict(os.environ)
    env.update(
        {
            "HOME": str(root / "home"),
            "TMPDIR": str(tmp),
            "PURETERM_DAEMON_SOCKET": str(socket_path),
            "PURETERM_DAEMON_DATA_DIR": str(root / "daemon-data"),
        }
    )
    try:
        subprocess.run(
            [str(binary), "daemon", "stop"],
            env=env,
            cwd=str(root),
            capture_output=True,
            text=True,
            timeout=15,
        )
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass
