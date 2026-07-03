#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# PureTerm Build Script
# 用法: ./build.sh [OPTIONS] <MODE>
# =============================================================================

readonly PROJECT_NAME="PureTerm"
readonly BINARY_NAME="pureterm"
readonly DIST_DIR="./dist"
readonly SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# 从 Cargo.toml 提取版本号
extract_version() {
    grep '^version' "$SCRIPT_DIR/crates/pureterm/Cargo.toml" \
        | head -1 \
        | sed 's/.*"\([^"]*\)".*/\1/'
}

readonly VERSION="$(extract_version)"

# =============================================================================
# 彩色输出
# =============================================================================

# 检测是否支持颜色
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    readonly RED='\033[0;31m'
    readonly GREEN='\033[0;32m'
    readonly YELLOW='\033[0;33m'
    readonly BLUE='\033[0;34m'
    readonly BOLD='\033[1m'
    readonly RESET='\033[0m'
else
    readonly RED='' readonly GREEN='' readonly YELLOW='' readonly BLUE=''
    readonly BOLD='' readonly RESET=''
fi

info()    { printf "${GREEN}[INFO]${RESET}  %s\n" "$*"; }
warn()    { printf "${YELLOW}[WARN]${RESET}  %s\n" "$*" >&2; }
error()   { printf "${RED}[ERROR]${RESET} %s\n" "$*" >&2; }
step()    { printf "${BLUE}==>${RESET}     %s\n" "$*"; }
success() { printf "${GREEN}  ✓${RESET}    %s\n" "$*"; }

# =============================================================================
# 帮助信息
# =============================================================================

usage() {
    cat <<EOF
${BOLD}用法${RESET}: $(basename "$0") [OPTIONS] <MODE>

${BOLD}模式${RESET}:
  debug     开发构建（dev profile）
  release   发布构建（thin LTO + strip 优化）
  app       发布构建 + 打包为平台应用包
  test      运行测试套件（自动检测 cargo-nextest，否则回退 cargo test）
  clean     清除所有构建缓存

${BOLD}选项${RESET}:
  --clean             构建前清除增量编译缓存（推荐字体/资源变更后使用）
  --target <triple>   指定交叉编译目标（如 aarch64-apple-darwin）
  --timings           生成编译时序报告（HTML，位于 target/cargo-timings/）
  --profile <name>    test 模式下选择 cargo profile（默认 dev；可选 test / release-fast）
  --all-features      test 模式下启用全部 features
  --no-run            test 模式下只编译，不运行
  -h, --help          显示此帮助信息

${BOLD}示例${RESET}:
  ./build.sh debug
  ./build.sh --clean debug        # 清除缓存后构建（解决增量编译问题）
  ./build.sh release
  ./build.sh app
  ./build.sh test                 # 跑测试（自动用 nextest，没有则 cargo test）
  ./build.sh test --profile test  # 显式使用 [profile.test] 加速测试编译
  ./build.sh test --no-run        # 只编译测试二进制，不跑
  ./build.sh clean                # 仅清除缓存
  ./build.sh --target aarch64-apple-darwin release

EOF
}

# =============================================================================
# 工具链检测（sccache / zld / mold / lld）
# =============================================================================

TOOLCHAIN_INFO=""

detect_toolchain() {
    local tools=()

    # sccache
    if command -v sccache &>/dev/null; then
        TOOLS_SCCACHE="yes"
        export RUSTC_WRAPPER=sccache
        tools+=("sccache")
    else
        TOOLS_SCCACHE=""
    fi

    # cargo-nextest（并行测试运行器，test 模式自动选用；缺失时 fallback 到 cargo test）
    # 双重探测：command -v 找到 + `cargo nextest --help` 真正能跑（避免 PATH 上有同名的损坏 wrapper）
    if command -v cargo-nextest &>/dev/null && cargo nextest --help &>/dev/null; then
        TOOLS_NEXTEST="yes"
        tools+=("nextest")
    else
        TOOLS_NEXTEST=""
    fi

    # Fast linker detection (Linux only).
    # macOS 15+/Xcode 16+ use a fast default linker (ld-prime); zld is unmaintained
    # and incompatible, so we deliberately do not look for it on Darwin.
    local uname_s
    uname_s="$(uname -s)"

    if [ "$uname_s" = "Linux" ]; then
        if command -v mold &>/dev/null; then
            tools+=("mold")
        elif command -v lld &>/dev/null; then
            tools+=("lld")
        fi
    fi

    if [ ${#tools[@]} -gt 0 ]; then
        TOOLCHAIN_INFO=" (using: $(IFS=', '; echo "${tools[*]}"))"
    else
        TOOLCHAIN_INFO=""
    fi
}

# =============================================================================
# 平台检测
# =============================================================================

detect_platform() {
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        case "$TARGET_TRIPLE" in
            *-apple-darwin*)  echo "macos" ;;
            *-linux-*)        echo "linux" ;;
            *-windows-*)      echo "windows" ;;
            *)                echo "unknown" ;;
        esac
    else
        case "$(uname -s)" in
            Darwin)   echo "macos" ;;
            Linux)    echo "linux" ;;
            MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
            *)        echo "unknown" ;;
        esac
    fi
}

# =============================================================================
# 清除缓存
# =============================================================================

# 涉及 include_bytes! 的 crate 列表，增量编译可能无法正确追踪嵌入资源变更
readonly EMBEDDED_RESOURCE_CRATES=(pureterm-gui)

do_clean() {
    local mode="${1:-all}"

    if [ "$mode" = "all" ]; then
        step "清除所有构建缓存..."
        cargo clean
        success "已清除 target/ 下所有构建产物"
    else
        step "清除增量编译缓存（保留依赖）..."
        for crate in "${EMBEDDED_RESOURCE_CRATES[@]}"; do
            cargo clean -p "$crate" 2>/dev/null && \
                info "已清除 ${crate}" || true
        done
        cargo clean -p "${BINARY_NAME}" 2>/dev/null && \
            info "已清除 ${BINARY_NAME}" || true
        success "增量编译缓存已清除"
    fi
}

# =============================================================================
# 构建函数
# =============================================================================

get_binary_path() {
    local profile="$1"
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        echo "target/${TARGET_TRIPLE}/${profile}/${BINARY_NAME}"
    else
        echo "target/${profile}/${BINARY_NAME}"
    fi
}

build_debug() {
    if [ "$CLEAN_BEFORE_BUILD" = "true" ]; then
        do_clean incremental
    fi

    step "构建 debug 版本 (dev profile)..."
    local cmd="cargo build --bin ${BINARY_NAME}"
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        cmd="${cmd} --target ${TARGET_TRIPLE}"
    fi
    if [ "$TIMINGS_ENABLED" = "true" ]; then
        cmd="${cmd} --timings"
    fi

    if $cmd; then
        local bin
        bin="$(get_binary_path debug)"
        local size
        size="$(du -h "$bin" | cut -f1)"
        success "debug 构建完成"
        info "二进制文件: ${bin} (${size})"
    else
        error "debug 构建失败"
        return 1
    fi
}

build_release() {
    if [ "$CLEAN_BEFORE_BUILD" = "true" ]; then
        do_clean incremental
    fi

    step "构建 release 版本 (thin LTO + strip 优化)..."
    local cmd="cargo build --release --bin ${BINARY_NAME}"
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        cmd="${cmd} --target ${TARGET_TRIPLE}"
    fi
    if [ "$TIMINGS_ENABLED" = "true" ]; then
        cmd="${cmd} --timings"
    fi

    if $cmd; then
        local bin
        bin="$(get_binary_path release)"
        local size
        size="$(du -h "$bin" | cut -f1)"
        success "release 构建完成"
        info "二进制文件: ${bin} (${size})"
    else
        error "release 构建失败"
        return 1
    fi
}

# =============================================================================
# Test 运行
# =============================================================================

build_test() {
    step "运行测试套件 (cargo profile: ${TEST_PROFILE})..."

    local cmd
    if [ -n "${TOOLS_NEXTEST}" ]; then
        info "使用 cargo-nextest（并行执行）"
        # nextest 内部会调用 cargo —— 用 --cargo-profile 透传，让 [profile.test] 等
        # cargo profile 段在测试构建阶段生效。
        cmd="cargo nextest run --workspace --cargo-profile ${TEST_PROFILE}"
        if [ "$NO_RUN" = "true" ]; then
            cmd="${cmd} --no-run"
        fi
    else
        info "使用 cargo test（未安装 cargo-nextest，建议 `cargo install cargo-nextest --locked`）"
        cmd="cargo test --workspace --profile ${TEST_PROFILE}"
        if [ "$NO_RUN" = "true" ]; then
            cmd="${cmd} --no-run"
        fi
    fi

    if [ "$ALL_FEATURES" = "true" ]; then
        cmd="${cmd} --all-features"
    fi
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        cmd="${cmd} --target ${TARGET_TRIPLE}"
    fi

    $cmd
}

# =============================================================================
# App 打包
# =============================================================================

build_app() {
    # 先执行 release 构建（主二进制 + settings）
    build_release || return 1

    step "构建 pureterm-settings..."
    local settings_cmd="cargo build --release --bin pureterm-settings"
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        settings_cmd="${settings_cmd} --target ${TARGET_TRIPLE}"
    fi
    if [ "$TIMINGS_ENABLED" = "true" ]; then
        settings_cmd="${settings_cmd} --timings"
    fi
    if ! $settings_cmd; then
        warn "pureterm-settings 构建失败，Settings UI 将不可用"
    fi

    step "构建 puretermd (GAP-5.1 持久 PTY 守护进程)..."
    local daemon_cmd="cargo build --release --bin puretermd"
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        daemon_cmd="${daemon_cmd} --target ${TARGET_TRIPLE}"
    fi
    if ! $daemon_cmd; then
        warn "puretermd 构建失败，会话将无法在 GUI 关闭后存活（回退为直接本地 PTY）"
    fi

    local bin
    bin="$(get_binary_path release)"

    # 验证二进制文件存在
    if [ ! -f "$bin" ]; then
        error "找不到构建产物: ${bin}"
        return 1
    fi

    local platform
    platform="$(detect_platform)"

    step "打包应用 (${platform})..."

    case "$platform" in
        macos)  bundle_macos "$bin" ;;
        linux)  bundle_linux "$bin" ;;
        windows)
            error "Windows 应用打包暂不支持（缺少平台字体路径定义）"
            info "请使用 './build.sh release' 获取独立二进制文件"
            return 1
            ;;
        *)
            error "未知平台，无法打包"
            return 1
            ;;
    esac
}

# macOS: 创建 .app 包
bundle_macos() {
    local bin="$1"
    local app_dir="${DIST_DIR}/${PROJECT_NAME}.app"
    local contents_dir="${app_dir}/Contents"

    step "创建 macOS 应用包..."

    # 清理旧的构建产物
    rm -rf "$app_dir"

    # 创建目录结构
    mkdir -p "${contents_dir}/MacOS"
    mkdir -p "${contents_dir}/Resources"
    mkdir -p "${contents_dir}/Frameworks"

    # 复制二进制文件
    cp "$bin" "${contents_dir}/MacOS/${BINARY_NAME}"
    chmod +x "${contents_dir}/MacOS/${BINARY_NAME}"

    # 复制 pureterm-settings（如果存在）
    local settings_bin
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        settings_bin="target/${TARGET_TRIPLE}/release/pureterm-settings"
    else
        settings_bin="target/release/pureterm-settings"
    fi
    if [ -f "$settings_bin" ]; then
        cp "$settings_bin" "${contents_dir}/MacOS/pureterm-settings"
        chmod +x "${contents_dir}/MacOS/pureterm-settings"
        step "已复制 pureterm-settings"
    fi

    # 复制 puretermd（GAP-5.1 持久 PTY 守护进程；GUI spawn-on-demand 从同目录定位它）
    local daemon_bin
    if [ -n "${TARGET_TRIPLE:-}" ]; then
        daemon_bin="target/${TARGET_TRIPLE}/release/puretermd"
    else
        daemon_bin="target/release/puretermd"
    fi
    if [ -f "$daemon_bin" ]; then
        cp "$daemon_bin" "${contents_dir}/MacOS/puretermd"
        chmod +x "${contents_dir}/MacOS/puretermd"
        step "已复制 puretermd"
    else
        warn "未找到 puretermd，会话将无法在 GUI 关闭后存活"
    fi

    # 生成 Info.plist
    cat > "${contents_dir}/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>${BINARY_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>com.pureterm.app</string>
    <key>CFBundleName</key>
    <string>${PROJECT_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${PROJECT_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleIconFile</key>
    <string>${PROJECT_NAME}</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>Copyright $(date +%Y) ${PROJECT_NAME}. All rights reserved.</string>
</dict>
</plist>
EOF

    # 如果 assets 目录下有图标文件，复制到 Resources
    if [ -f "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" ]; then
        cp "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" "${contents_dir}/Resources/"
        step "已复制应用图标"
    fi

    local app_size
    app_size="$(du -sh "$app_dir" | cut -f1)"
    success "macOS 应用包创建完成"
    info "应用路径: ${app_dir} (${app_size})"
    info "运行: open ${app_dir}"
    info "安装: cp -r ${app_dir} /Applications/"
}

# Linux: 安装到用户目录
bundle_linux() {
    local bin="$1"
    local install_dir="${HOME}/.local/bin"
    local desktop_dir="${HOME}/.local/share/applications"

    step "安装到 Linux 用户目录..."

    # 安装二进制文件
    mkdir -p "$install_dir"
    cp "$bin" "${install_dir}/${BINARY_NAME}"
    chmod +x "${install_dir}/${BINARY_NAME}"
    success "二进制文件已安装到 ${install_dir}/${BINARY_NAME}"

    # 创建桌面入口
    mkdir -p "$desktop_dir"
    cat > "${desktop_dir}/${BINARY_NAME}.desktop" <<EOF
[Desktop Entry]
Name=${PROJECT_NAME}
Comment=GPU-accelerated terminal emulator
Exec=${install_dir}/${BINARY_NAME}
Icon=utilities-terminal
Type=Application
Categories=System;TerminalEmulator;
Keywords=terminal;shell;
StartupNotify=true
EOF

    # 更新桌面数据库
    if command -v update-desktop-database &>/dev/null; then
        update-desktop-database "$desktop_dir" 2>/dev/null || true
    fi

    success "桌面入口已创建: ${desktop_dir}/${BINARY_NAME}.desktop"

    # 检查 PATH
    if [[ ":${PATH}:" != *":${install_dir}:"* ]]; then
        warn "请将 ${install_dir} 添加到 PATH:"
        info "  echo 'export PATH=\"${install_dir}:\$PATH\"' >> ~/.bashrc"
    fi
}

# =============================================================================
# 参数解析
# =============================================================================

TARGET_TRIPLE=""
MODE=""
CLEAN_BEFORE_BUILD="false"
TIMINGS_ENABLED="false"
TEST_PROFILE="dev"
ALL_FEATURES="false"
NO_RUN="false"

while [[ $# -gt 0 ]]; do
    case "$1" in
        debug|release|app|test|clean)
            if [ -n "$MODE" ]; then
                error "只能指定一个模式"
                usage
                exit 1
            fi
            MODE="$1"
            shift
            ;;
        --clean)
            CLEAN_BEFORE_BUILD="true"
            shift
            ;;
        --timings)
            TIMINGS_ENABLED="true"
            shift
            ;;
        --target)
            if [ -z "${2:-}" ]; then
                error "--target 需要指定目标三元组"
                exit 1
            fi
            TARGET_TRIPLE="$2"
            shift 2
            ;;
        --profile)
            if [ -z "${2:-}" ]; then
                error "--profile 需要指定 profile 名（dev / test / release-fast 等）"
                exit 1
            fi
            TEST_PROFILE="$2"
            shift 2
            ;;
        --all-features)
            ALL_FEATURES="true"
            shift
            ;;
        --no-run)
            NO_RUN="true"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            error "未知参数: $1"
            usage
            exit 1
            ;;
    esac
done

# =============================================================================
# 主流程
# =============================================================================

if [ -z "$MODE" ]; then
    error "请指定构建模式"
    usage
    exit 1
fi

# 验证目标平台
if [ -n "$TARGET_TRIPLE" ]; then
    step "目标平台: ${TARGET_TRIPLE}"
fi

detect_toolchain
local_platform="$(detect_platform)"
step "当前平台: ${local_platform}${TOOLCHAIN_INFO}"
info "${PROJECT_NAME} v${VERSION}"

case "$MODE" in
    debug)   build_debug   ;;
    release) build_release ;;
    app)     build_app     ;;
    test)    build_test    ;;
    clean)   do_clean all  ;;
esac
