# PureTerm 构建命令
#
# 所有跨平台 / 跨 target 工作均经 ./build.sh。
# 运行 `./build.sh --help` 查看完整选项列表,或 `./build.sh info`
# 查看当前宿主上可用的工具。

# --- 核心构建 -------------------------------------------------------------

# 构建 debug
build:
    cargo build --bin pureterm

# 构建 release
release:
    cargo build --release --bin pureterm

# 在宿主 triple 上为宿主 OS 构建 + 打包。
#   macOS  → dist/PureTerm.app、.app.zip、.dmg(应用包)
#   Linux  → dist/pureterm-*/ + .tar.gz(目录树 + 压缩包)
#   Win    → dist/pureterm-*/ + .zip(目录树 + 压缩包)
app:
    bash build.sh app

# 在所有已安装 target triple 上为宿主 OS 打包。
app-all:
    bash build.sh app-all

# 跨平台构建矩阵。跳过未安装工具链的
# target(无硬报错)。跨 OS 构建(如 Linux 上产 .dmg)
# 由脚本跳过 — 只运行宿主 OS 的打包。
matrix:
    bash build.sh matrix

# 打印检测到的环境:宿主 OS、已安装 rustup targets、
# 以及哪些打包工具(hdiutil、codesign、zstd、zip、signtool…)可用。
# CI 上调试失败构建步骤时有用。
info:
    bash build.sh info

# --- 测试 / lint ------------------------------------------------------------

# 运行 clippy
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# 检查格式化
fmt:
    cargo fmt --all -- --check

# 运行测试(自动检测 cargo-nextest 以并行执行)
# 安装:cargo install cargo-nextest --locked
test:
    #!/usr/bin/env bash
    set -e
    if command -v cargo-nextest >/dev/null 2>&1; then
        echo "→ using cargo-nextest"
        cargo nextest run --workspace
    else
        echo "→ using cargo test (install cargo-nextest --locked for faster parallel runs)"
        cargo test --workspace
    fi

# --- 实用工具 --------------------------------------------------------------

# 清理构建产物(以及 dist/ 目录)。
clean:
    bash build.sh clean

# 显示当前配置
show-config:
    cargo run -- show-config

# 列出可用主题
list-themes:
    cargo run -- list-themes
