#!/usr/bin/env bash
# =============================================================================
# PureTerm 跨平台构建与打包脚本
# 支持:macOS(aarch64 / x86_64)、Linux(gnu + musl,aarch64 / x86_64)、
#           以及 Windows MSVC(aarch64 / x86_64)。
#
# 用法:./build.sh [OPTIONS] <MODE>
#
# 模式:
#   debug     开发构建(dev profile)
#   release   优化发布构建(thin LTO + strip)
#   test      运行测试套件(nextest,回退 cargo test)
#   app       在宿主 target triple 上为宿主 OS 构建 + 打包
#   app-all   在所有已安装 target triple 上为宿主 OS 构建 + 打包
#   matrix    跨完整 target 矩阵构建 + 打包
#             (跳过未安装工具链的 target)
#   info      打印检测到的平台、已安装 target、可用工具
#   clean     删除所有构建产物
#
# 运行 `./build.sh --help` 查看完整选项列表。
# =============================================================================
set -euo pipefail

# -----------------------------------------------------------------------------
# 项目常量
# -----------------------------------------------------------------------------
readonly PROJECT_NAME="PureTerm"
readonly BINARY_NAME="pureterm"
readonly SETTINGS_BIN="pureterm-settings"
readonly DAEMON_BIN="puretermd"
readonly DIST_DIR="./dist"
readonly SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# -----------------------------------------------------------------------------
# 静态 target 矩阵
#
# 每条条目:"<host-os-key>:<triple>:<arch-suffix>"。`os-key` 是
# `bundle_<os>` 分支判断用的键;`arch-suffix` 成为包名的一部分
# (例如 `aarch64-unknown-linux-gnu` → `linux-gnu-aarch64`)。
#
# `matrix` 与 `app-all` 只考虑 triple 在
# `rustup target list --installed` 中的条目;用户当前无法构建的
# 条目以 `WARN` 跳过,绝不硬报错。
# -----------------------------------------------------------------------------
readonly TARGET_MATRIX=(
    "macos:aarch64-apple-darwin:arm64"
    "macos:x86_64-apple-darwin:x64"
    "linux:aarch64-unknown-linux-gnu:arm64-gnu"
    "linux:x86_64-unknown-linux-gnu:x64-gnu"
    "linux:aarch64-unknown-linux-musl:arm64-musl"
    "linux:x86_64-unknown-linux-musl:x64-musl"
    "windows:aarch64-pc-windows-msvc:arm64"
    "windows:x86_64-pc-windows-msvc:x64"
)

# -----------------------------------------------------------------------------
# 版本提取(唯一真源:crates/pureterm/Cargo.toml)
# -----------------------------------------------------------------------------
extract_version() {
    grep '^version' "$SCRIPT_DIR/crates/pureterm/Cargo.toml" \
        | head -1 \
        | sed 's/.*"\([^"]*\)".*/\1/'
}
readonly VERSION="$(extract_version)"

# -----------------------------------------------------------------------------
# 颜色(管道输出或设置 NO_COLOR 时自动禁用,遵循 no-color.org 约定)
# -----------------------------------------------------------------------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    # 使用 ANSI-C quoting ($'...') 让 \033 在赋值时被解析为真实 ESC
    # 字符 (0x1B)。如果用单引号,反斜杠会原样保留,后续 unquoted heredoc
    # (cat <<EOF) 里的 `\` 会被 bash 当作转义前缀吞掉,导致终端上输出
    # 字面 `\033[1m` 等乱码。
    readonly RED=$'\033[0;31m'
    readonly GREEN=$'\033[0;32m'
    readonly YELLOW=$'\033[0;33m'
    readonly BLUE=$'\033[0;34m'
    readonly BOLD=$'\033[1m'
    readonly RESET=$'\033[0m'
else
    readonly RED='' readonly GREEN='' readonly YELLOW='' readonly BLUE=''
    readonly BOLD='' readonly RESET=''
fi

info()    { printf "${GREEN}[INFO]${RESET}  %s\n" "$*"; }
warn()    { printf "${YELLOW}[WARN]${RESET}  %s\n" "$*" >&2; }
error()   { printf "${RED}[ERROR]${RESET} %s\n" "$*" >&2; }
step()    { printf "${BLUE}==>${RESET}     %s\n" "$*"; }
success() { printf "${GREEN}  ✓${RESET}    %s\n" "$*"; }
note()    { printf "${BOLD}  ·${RESET}     %s\n" "$*"; }

# -----------------------------------------------------------------------------
# 帮助文本
# -----------------------------------------------------------------------------
usage() {
    cat <<EOF
${BOLD}用法${RESET}: $(basename "$0") [OPTIONS] <MODE>

${BOLD}模式${RESET}:
  debug              开发构建 (dev profile)
  release            发布构建 (thin LTO + strip)
  test               运行测试套件 (nextest → cargo test)
  app                为主机 OS + 主机 triple 打包
  app-all            为主机 OS 的所有已安装 triple 打包
  matrix             跨全平台目标矩阵打包 (跳过未安装的 triple)
  info               打印检测到的平台 / 已安装目标 / 可用工具
  clean              清除所有构建缓存

${BOLD}目标选择${RESET} (用于 app / app-all / matrix):
  --target <triple>           指定单一目标 (例如 aarch64-apple-darwin)
  --targets <a,b,c>           多个目标，逗号分隔
  --arch <x86_64|aarch64|all> 按架构筛选 (会展开为目标矩阵的子集)
  --auto-install-targets      若 triple 未安装,自动 rustup target add

${BOLD}打包选项${RESET}:
  --bundle <app|dmg|tar|zip|none|all>
                              强制使用指定打包格式;默认按平台选最合适的。
                              none = 仅构建,不打包;all = 全部可用格式。
  --sign                      启用代码签名 (macOS: codesign;Windows: signtool)
  --notarize                  macOS 提交公证 (需 APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID)
  --install                   Linux: 把 staged tree 复制到 /opt 并建立 symlink
  --universal                 macOS: 构建 universal2 二进制 (arm64 + x86_64 via lipo)
  --no-bundle                 同 --bundle none

${BOLD}构建选项${RESET}:
  --clean                     构建前清除增量编译缓存
  --timings                   生成编译时序报告 (target/cargo-timings/)
  --jobs <N>                  传递给 cargo -jN
  --profile <name>            test 模式下选择 cargo profile (默认 dev)
  --all-features              test 模式启用全部 features
  --no-run                    test 模式只编译,不运行
  --skip-tests                matrix 模式下跳过每个目标的 cargo test

${BOLD}其它${RESET}:
  -h, --help                  显示此帮助

${BOLD}示例${RESET}:
  ./build.sh release
  ./build.sh app                              # 当前主机 .app / .tar.gz / .zip
  ./build.sh --target aarch64-apple-darwin app
  ./build.sh --arch x86_64 --bundle all app   # 强制 x86_64 + 所有格式
  ./build.sh --targets aarch64-apple-darwin,x86_64-apple-darwin app
  ./build.sh --bundle dmg --sign --notarize app
  ./build.sh matrix                           # 跨平台矩阵打包
  ./build.sh info

EOF
}

# -----------------------------------------------------------------------------
# 工具链检测
# -----------------------------------------------------------------------------
# 填充:TOOL_* 标志(yes/"")+ TOOLCHAIN_INFO(格式化摘要)。
detect_toolchain() {
    local tools=()

    # sccache —— 缺失时静默降级。
    # 仓库的 .cargo/config.toml 中设有 `rustc-wrapper = "sccache"`,这是
    # 硬性配置(不是环境变量),sccache 不在 PATH 时需要在
    # cargo 调用层面 *覆盖* 它。否则
    # `cargo build` 会以 127 退出,报 "process didn't exit successfully: sccache"。
    if command -v sccache &>/dev/null; then
        TOOL_SCCACHE="yes"
        export RUSTC_WRAPPER=sccache
        tools+=("sccache")
    else
        TOOL_SCCACHE=""
        unset RUSTC_WRAPPER 2>/dev/null || true
    fi

    # cargo-nextest —— 二次确认它真的能跑(PATH 可能说谎)。
    if command -v cargo-nextest &>/dev/null && cargo nextest --help &>/dev/null; then
        TOOL_NEXTEST="yes"
        tools+=("nextest")
    else
        TOOL_NEXTEST=""
    fi

    # 快速 Linux 链接器。
    local uname_s
    uname_s="$(uname -s)"
    if [ "$uname_s" = "Linux" ]; then
        if command -v mold &>/dev/null; then
            tools+=("mold")
        elif command -v lld &>/dev/null; then
            tools+=("lld")
        fi
    fi

    # 平台专用打包器 —— 仅信息性,绝不致命。
    command -v hdiutil      &>/dev/null && TOOL_HDIUTIL="yes"      || TOOL_HDIUTIL=""
    command -v codesign     &>/dev/null && TOOL_CODESIGN="yes"     || TOOL_CODESIGN=""
    if command -v xcrun &>/dev/null && xcrun -f notarytool &>/dev/null; then
        TOOL_NOTARY="yes"
    else
        TOOL_NOTARY=""
    fi
    command -v dpkg-deb     &>/dev/null && TOOL_DPKG="yes"         || TOOL_DPKG=""
    command -v rpmbuild     &>/dev/null && TOOL_RPMBUILD="yes"     || TOOL_RPMBUILD=""
    command -v appimagetool &>/dev/null && TOOL_APPIMAGE="yes"     || TOOL_APPIMAGE=""
    command -v zstd         &>/dev/null && TOOL_ZSTD="yes"         || TOOL_ZSTD=""
    command -v zip          &>/dev/null && TOOL_ZIP="yes"          || TOOL_ZIP=""
    command -v signtool     &>/dev/null && TOOL_SIGNTOOL="yes"     || TOOL_SIGNTOOL=""

    # Windows 上的 PowerShell,作为 zip 兜底(仅 Windows 相关)。
    command -v powershell   &>/dev/null && TOOL_PWSH="yes"         || TOOL_PWSH=""
    command -v pwsh         &>/dev/null && TOOL_PWSH_CORE="yes"    || TOOL_PWSH_CORE=""

    if [ ${#tools[@]} -gt 0 ]; then
        TOOLCHAIN_INFO=" (using: $(IFS=', '; echo "${tools[*]}"))"
    else
        TOOLCHAIN_INFO=""
    fi
}

# -----------------------------------------------------------------------------
# 平台 / target 检测
# -----------------------------------------------------------------------------
host_os() {
    case "$(uname -s)" in
        Darwin)   echo "macos" ;;
        Linux)    echo "linux" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)        echo "unknown" ;;
    esac
}

host_arch() {
    case "$(uname -m)" in
        arm64|aarch64) echo "aarch64" ;;
        x86_64|amd64)  echo "x86_64" ;;
        *)             echo "unknown" ;;
    esac
}

# 把 triple 的架构 token 翻译为我们的包架构后缀。
arch_from_triple() {
    local triple="$1"
    case "$triple" in
        aarch64-*)  echo "arm64" ;;
        x86_64-*)   echo "x64" ;;
        arm-*)      echo "arm" ;;
        *)          echo "$(echo "$triple" | cut -d- -f1)" ;;
    esac
}

# -----------------------------------------------------------------------------
# target 矩阵展开
# -----------------------------------------------------------------------------
# 输入:--arch / --target / --targets(全局变量)。
# 输出:TARGETS[] = 待构建 triple 列表,顺序稳定。
expand_targets() {
    local host
    host="$(host_os)"

    # 显式 --target 优先。
    if [ -n "${OPT_TARGET:-}" ]; then
        TARGETS=("$OPT_TARGET")
        return
    fi

    # 显式 --targets(逗号分隔)优先。
    if [ -n "${OPT_TARGETS:-}" ]; then
        IFS=',' read -ra TARGETS <<<"$OPT_TARGETS"
        return
    fi

    # --arch 过滤:从 TARGET_MATRIX 输出匹配条目。
    if [ -n "${OPT_ARCH:-}" ]; then
        TARGETS=()
        local want_arch="$OPT_ARCH"
        [ "$want_arch" = "all" ] && want_arch=""
        local entry triple arch_suffix
        for entry in "${TARGET_MATRIX[@]}"; do
            IFS=':' read -r _ triple arch_suffix <<<"$entry"
            if [ -z "$want_arch" ] || [ "$arch_suffix" = "$want_arch" ] || \
               { [ -n "$want_arch" ] && [ "$triple" = "$want_arch" ]; }; then
                TARGETS+=("$triple")
            fi
        done
        return
    fi

    # 默认:仅宿主 triple。
    TARGETS=("$(rustc -vV | sed -n 's|host: ||p')")
}

# triple 在 `rustup target list --installed` 中返回 0,否则 1。
target_installed() {
    local t="$1"
    rustup target list --installed 2>/dev/null | tr -d ' ' | grep -qx "$t"
}

# 确保 triple 已安装;未装时可选安装(--auto-install-targets)。
ensure_target() {
    local t="$1"
    if target_installed "$t"; then
        return 0
    fi
    if [ "${OPT_AUTO_INSTALL_TARGETS:-no}" = "yes" ]; then
        warn "目标 $t 未安装,正在 rustup target add ..."
        if rustup target add "$t"; then
            return 0
        fi
        error "rustup target add $t 失败"
        return 1
    fi
    warn "目标 $t 未安装 (使用 --auto-install-targets 自动安装,或 rustup target add $t)"
    return 1
}

# -----------------------------------------------------------------------------
# 预检:确认 Cargo 工作区可解析。
# -----------------------------------------------------------------------------
preflight() {
    if ! command -v cargo &>/dev/null; then
        error "cargo 不在 PATH 中。请先安装 Rust: https://rustup.rs/"
        exit 1
    fi
    if [ ! -f "$SCRIPT_DIR/Cargo.toml" ]; then
        error "找不到 $SCRIPT_DIR/Cargo.toml,请在仓库根目录运行此脚本"
        exit 1
    fi
    if ! cargo metadata --no-deps --format-version=1 &>/dev/null; then
        error "cargo metadata 解析失败 — Cargo.toml 有问题"
        exit 1
    fi
}

# -----------------------------------------------------------------------------
# 清理
# -----------------------------------------------------------------------------
readonly EMBEDDED_RESOURCE_CRATES=(pureterm-gui)

do_clean() {
    local mode="${1:-all}"
    if [ "$mode" = "all" ]; then
        step "清除所有构建缓存..."
        cargo clean
        rm -rf "$DIST_DIR"
        success "已清除 target/ 与 dist/"
    else
        step "清除增量编译缓存 (保留依赖)..."
        for crate in "${EMBEDDED_RESOURCE_CRATES[@]}"; do
            cargo clean -p "$crate" 2>/dev/null && info "已清除 ${crate}" || true
        done
        cargo clean -p "${BINARY_NAME}" 2>/dev/null && info "已清除 ${BINARY_NAME}" || true
        success "增量编译缓存已清除"
    fi
}

# -----------------------------------------------------------------------------
# 二进制路径解析
# -----------------------------------------------------------------------------
# target/<triple>/<profile>/ 下的每 target 二进制目录
target_dir() {
    local profile="$1" triple="${2:-${HOST_TRIPLE:-}}"
    if [ -n "$triple" ]; then
        echo "target/${triple}/${profile}"
    else
        echo "target/${profile}"
    fi
}

# 解析给定 triple + profile 已构建二进制的磁盘路径。
binary_path() {
    local name="$1" profile="$2" triple="${3:-${HOST_TRIPLE:-}}"
    # Windows(MSVC)的 cargo 产物带 .exe。此前无条件拼无后缀路径,
    # 导致 cargo_build_one 的产物校验与 bundle_windows 的 cp 在
    # Windows 上都找不到二进制、产不出 .zip。按 triple 补后缀。
    local suffix=""
    case "$triple" in
        *-windows-*) suffix=".exe" ;;
    esac
    if [ -n "$triple" ]; then
        echo "target/${triple}/${profile}/${name}${suffix}"
    else
        echo "target/${profile}/${name}${suffix}"
    fi
}

# -----------------------------------------------------------------------------
# cargo 调用辅助
# -----------------------------------------------------------------------------
# 输出使本次运行稳健所需的 cargo `--config` 标志。
#   - sccache 不在 PATH 时,覆盖仓库的 `[build] rustc-wrapper
#     = "sccache",以免 cargo 调用缺失的二进制。
cargo_config_args() {
    local args=()
    if [ -z "${TOOL_SCCACHE:-}" ]; then
        args+=(--config 'build.rustc-wrapper=""')
    fi
    printf '%s\n' "${args[@]}"
}

# 在一个 triple 上构建一个二进制。cargo build "成功"
# 之后二进制不在盘上时记录清晰错误(捕获漏 --features 的情况)。
cargo_build_one() {
    local pkg="$1" bin="$2" profile="$3" triple="$4"
    local cmd=(cargo build)
    [ "$profile" = "release" ] && cmd+=(--release)
    [ -n "$triple" ] && cmd+=(--target "$triple")
    [ -n "${OPT_JOBS:-}" ] && cmd+=(-j "$OPT_JOBS")
    if [ "$TIMINGS_ENABLED" = "true" ]; then
        cmd+=(--timings)
    fi
    cmd+=(-p "$pkg" --bin "$bin")
    # 注入所有 --config 覆盖(例如 sccache 缺失时禁用它)。
    while IFS= read -r arg; do
        [ -n "$arg" ] && cmd+=("$arg")
    done < <(cargo_config_args)

    step "cargo: pkg=${pkg} bin=${bin} profile=${profile} target=${triple:-host}"
    if ! "${cmd[@]}"; then
        return 1
    fi

    # 校验产物确实存在。
    local path
    path="$(binary_path "$bin" "$profile" "$triple")"
    if [ ! -x "$path" ] && [ ! -f "$path" ]; then
        error "构建报告成功但找不到产物: $path"
        error "(检查 --features: ${bin} 可能需要额外的 feature flag)"
        return 1
    fi
    return 0
}

# 构建打包器关心的所有工作区二进制。
# 参数:<profile> <triple>
build_all_binaries() {
    local profile="$1" triple="$2"

    cargo_build_one "${BINARY_NAME}" "${BINARY_NAME}"    "$profile" "$triple" || return 1
    cargo_build_one "pureterm-settings" "${SETTINGS_BIN}" "$profile" "$triple" || {
        warn "${SETTINGS_BIN} 构建失败 — Settings UI 将不可用"
    }
    cargo_build_one "pureterm-daemon"  "${DAEMON_BIN}"  "$profile" "$triple" || {
        warn "${DAEMON_BIN} 构建失败 — GUI 关闭后会话将无法存活"
    }
}

# -----------------------------------------------------------------------------
# 模式:build(debug / release)— 不打包,仅为宿主编译。
# -----------------------------------------------------------------------------
build_debug() {
    if [ "$CLEAN_BEFORE_BUILD" = "true" ]; then
        do_clean incremental
    fi
    cargo_build_one "${BINARY_NAME}" "${BINARY_NAME}" "debug" "${HOST_TRIPLE:-}" || return 1
    local bin
    bin="$(binary_path "${BINARY_NAME}" debug "${HOST_TRIPLE:-}")"
    success "debug 构建完成: ${bin} ($(du -h "$bin" | cut -f1))"
}

build_release() {
    if [ "$CLEAN_BEFORE_BUILD" = "true" ]; then
        do_clean incremental
    fi
    cargo_build_one "${BINARY_NAME}" "${BINARY_NAME}" "release" "${HOST_TRIPLE:-}" || return 1
    local bin
    bin="$(binary_path "${BINARY_NAME}" release "${HOST_TRIPLE:-}")"
    success "release 构建完成: ${bin} ($(du -h "$bin" | cut -f1))"
}

# -----------------------------------------------------------------------------
# 模式:test
# -----------------------------------------------------------------------------
build_test() {
    step "运行测试套件 (cargo profile: ${TEST_PROFILE})..."

    local cmd
    if [ -n "${TOOL_NEXTEST}" ]; then
        info "使用 cargo-nextest (并行执行)"
        cmd=(cargo nextest run --workspace --cargo-profile "${TEST_PROFILE}")
    else
        info "使用 cargo test (建议安装 cargo-nextest --locked 提速)"
        cmd=(cargo test --workspace --profile "${TEST_PROFILE}")
    fi
    if [ "$NO_RUN" = "true" ]; then
        cmd+=(--no-run)
    fi
    if [ "$ALL_FEATURES" = "true" ]; then
        cmd+=(--all-features)
    fi
    if [ -n "${OPT_TARGET:-}" ]; then
        cmd+=(--target "$OPT_TARGET")
    fi
    [ -n "${OPT_JOBS:-}" ] && cmd+=(-j "$OPT_JOBS")
    while IFS= read -r arg; do
        [ -n "$arg" ] && cmd+=("$arg")
    done < <(cargo_config_args)

    "${cmd[@]}"
}

# -----------------------------------------------------------------------------
# 模式:info
# -----------------------------------------------------------------------------
mode_info() {
    local host
    host="$(host_os)"
    local arch
    arch="$(host_arch)"
    local host_t
    host_t="$(rustc -vV | sed -n 's|host: ||p')"

    cat <<EOF
${BOLD}PureTerm 构建环境信息${RESET}
  项目:        ${PROJECT_NAME} v${VERSION}
  主机 OS:     ${host} (${arch})
  Rust host:   ${host_t}
  Rust 版本:   $(rustc --version 2>/dev/null || echo "n/a")
  Cargo:       $(cargo --version 2>/dev/null || echo "n/a")

${BOLD}已安装的 rustup 目标${RESET}
$(rustup target list --installed 2>/dev/null | sed 's/^/  /' || echo "  (rustup 不可用)")

${BOLD}工具可用性${RESET}
  sccache:        ${TOOL_SCCACHE:-no}
  nextest:        ${TOOL_NEXTEST:-no}
  hdiutil:        ${TOOL_HDIUTIL:-no}   (macOS .dmg)
  codesign:       ${TOOL_CODESIGN:-no}  (macOS 代码签名)
  notarytool:     ${TOOL_NOTARY:-no}    (macOS 公证)
  dpkg-deb:       ${TOOL_DPKG:-no}      (Linux .deb, v1 未使用)
  rpmbuild:       ${TOOL_RPMBUILD:-no}  (Linux .rpm, v1 未使用)
  appimagetool:   ${TOOL_APPIMAGE:-no}  (Linux AppImage, v1 未使用)
  zstd:           ${TOOL_ZSTD:-no}      (Linux .tar.zst)
  zip:            ${TOOL_ZIP:-no}       (Windows .zip 打包)
  powershell:     ${TOOL_PWSH:-no}${TOOL_PWSH_CORE:+ (pwsh)}  (Windows .zip fallback)
  signtool:       ${TOOL_SIGNTOOL:-no}  (Windows 代码签名)

${BOLD}当前 build.sh 配置${RESET}
  MODE:           ${MODE:-<unset>}
  TARGETS:        ${TARGETS[*]:-<none>}
  BUNDLE:         ${OPT_BUNDLE:-<default>}
  JOBS:           ${OPT_JOBS:-<default>}
  AUTO_INSTALL:   ${OPT_AUTO_INSTALL_TARGETS:-no}
EOF
}

# =============================================================================
# 打包器
# =============================================================================

# -----------------------------------------------------------------------------
# macOS 打包器—— 产出 .app(+ .app.zip,可选 .dmg,可选签名
# + 公证)。
# -----------------------------------------------------------------------------
bundle_macos() {
    local triple="$1"
    local profile="${2:-release}"

    local bin settings_bin daemon_bin
    bin="$(binary_path "${BINARY_NAME}"    "$profile" "$triple")"
    settings_bin="$(binary_path "${SETTINGS_BIN}" "$profile" "$triple")"
    daemon_bin="$(binary_path "${DAEMON_BIN}"  "$profile" "$triple")"

    if [ ! -f "$bin" ]; then
        error "找不到主二进制: $bin"
        return 1
    fi

    local arch_suffix
    arch_suffix="$(arch_from_triple "$triple")"
    local app_name="${PROJECT_NAME}-${VERSION}-${arch_suffix}"
    local app_dir="${DIST_DIR}/${PROJECT_NAME}.app"

    step "组装 macOS .app: ${app_dir}"

    # 全新布局。
    rm -rf "$app_dir"
    mkdir -p "${app_dir}/Contents/MacOS" \
             "${app_dir}/Contents/Resources" \
             "${app_dir}/Contents/Frameworks"

    # 二进制。
    cp "$bin" "${app_dir}/Contents/MacOS/${BINARY_NAME}"
    chmod +x "${app_dir}/Contents/MacOS/${BINARY_NAME}"
    if [ -f "$settings_bin" ]; then
        cp "$settings_bin" "${app_dir}/Contents/MacOS/${SETTINGS_BIN}"
        chmod +x "${app_dir}/Contents/MacOS/${SETTINGS_BIN}"
    fi
    if [ -f "$daemon_bin" ]; then
        cp "$daemon_bin" "${app_dir}/Contents/MacOS/${DAEMON_BIN}"
        chmod +x "${app_dir}/Contents/MacOS/${DAEMON_BIN}"
    else
        warn "未找到 ${DAEMON_BIN} — 会话将无法在 GUI 关闭后存活"
    fi

    # Info.plist—— 辅助函数管理与内联兜底两个分支都由
    # `bundle_macos_write_info_plist` 提供,与 universal
    # 构建器共享。
    bundle_macos_write_info_plist "$app_dir"

    # 图标。
    # macOS 读取 `Contents/Resources/PureTerm.icns`,只要 Info.plist
    # 声明了 `CFBundleIconFile = PureTerm`(`.icns` 扩展名
    # 隐含)。我们只拷贝 .icns—— 旁边多余的 `.png` 可能
    # 弄乱 Finder 图标缓存与文档 `deficon` 槽位。
    if [ -f "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" ]; then
        cp "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" \
           "${app_dir}/Contents/Resources/${PROJECT_NAME}.icns"
    else
        warn "缺少 assets/${PROJECT_NAME}.icns — .app 将没有图标"
    fi
    # 丢弃任何缓存的 icon.png;assets/icon.png 保留为跨平台
    # 源图标(Linux/Windows),而非 macOS 包资源。
    rm -f "${app_dir}/Contents/Resources/icon.png"

    success "macOS .app 创建完成: ${app_dir} ($(du -sh "$app_dir" | cut -f1))"

    # ---- 代码签名(可选)----
    if [ "${OPT_SIGN:-no}" = "yes" ] && [ -n "${TOOL_CODESIGN}" ] && \
       [ -n "${CODESIGN_IDENTITY:-}" ]; then
        step "codesign (identity=${CODESIGN_IDENTITY})..."
        codesign --force --deep --options runtime --sign "$CODESIGN_IDENTITY" \
                 "${app_dir}" || warn "codesign 失败 (继续)"
    elif [ "${OPT_SIGN:-no}" = "yes" ]; then
        warn "未配置 CODESIGN_IDENTITY — 跳过签名"
    fi

    # ---- 公证(可选)----
    if [ "${OPT_NOTARIZE:-no}" = "yes" ] && [ -n "${TOOL_NOTARY}" ] && \
       [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
        step "notarytool 提交公证..."
        local zip_for_notary="${DIST_DIR}/${app_name}.app.zip"
        if [ ! -f "$zip_for_notary" ]; then
            (cd "$DIST_DIR" && zip -yrq "${app_name}.app.zip" "${PROJECT_NAME}.app")
        fi
        xcrun notarytool submit "$zip_for_notary" \
            --apple-id "$APPLE_ID" \
            --password "$APPLE_PASSWORD" \
            --team-id "$APPLE_TEAM_ID" \
            --wait || warn "notarytool 失败 (继续)"
        xcrun staple staple "${app_dir}" 2>/dev/null || true
    elif [ "${OPT_NOTARIZE:-no}" = "yes" ]; then
        warn "未配置 APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID — 跳过公证"
    fi

    # ---- zip(总是)----
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        bundle_macos_zip "$app_dir" "$app_name"
    fi

    # ---- dmg(可选,需要 hdiutil)----
    if [ "${OPT_BUNDLE:-all}" = "all" ] || [ "${OPT_BUNDLE:-}" = "dmg" ]; then
        if [ -n "${TOOL_HDIUTIL}" ]; then
            bundle_macos_dmg "$app_dir" "$app_name"
        else
            warn "hdiutil 不可用 — 跳过 .dmg 生成"
        fi
    fi
}

bundle_macos_zip() {
    local app_dir="$1" name="$2"
    local out="${DIST_DIR}/${name}.app.zip"
    step "打包 .app.zip: ${out}"
    # $app_dir 是相对仓库根的 "./dist/PureTerm.app"。
    # `cd $DIST_DIR` 之后,相对路径只是 "PureTerm.app"。
    local rel_target
    rel_target="$(basename "$app_dir")"
    # `zip` 在 $DIST_DIR 内部调用,输出文件必须
    # 同样相对该目录—— 否则 cd 进 "./dist/" 后 "./dist/..."
    # 无法解析。
    local out_name
    out_name="$(basename "$out")"
    (cd "$DIST_DIR" && rm -f "$out_name" && zip -yrq "$out_name" "$rel_target")
    success "已生成: ${out} ($(du -h "$out" | cut -f1))"
}

bundle_macos_dmg() {
    local app_dir="$1" name="$2"
    local rw_dmg="${DIST_DIR}/${name}-rw.dmg"
    local final_dmg="${DIST_DIR}/${name}.dmg"
    step "创建 .dmg (hdiutil)..."

    # 先做可挂载的可读写 dmg。
    rm -f "$rw_dmg" "$final_dmg"
    hdiutil create -srcfolder "$app_dir" -volname "$name" -fs HFS+ \
        -fsargs "-c c=64,a=16,e=16" -format UDRW "$rw_dmg" 2>/dev/null || \
    hdiutil create -srcfolder "$app_dir" -volname "$name" -fs HFS+ \
        -format UDRW "$rw_dmg"

    # 再转为压缩只读(UDZO)。
    hdiutil convert "$rw_dmg" -format UDZO -imagekey zlib-level=9 -o "$final_dmg"
    rm -f "$rw_dmg"
    success "已生成: ${final_dmg} ($(du -h "$final_dmg" | cut -f1))"
}

# -----------------------------------------------------------------------------
# Linux 打包器—— 产出暂存目录树 + .tar.gz(有 zstd 时加 .tar.zst)。
# -----------------------------------------------------------------------------
bundle_linux() {
    local triple="$1"
    local profile="${2:-release}"

    local bin settings_bin daemon_bin
    bin="$(binary_path "${BINARY_NAME}"    "$profile" "$triple")"
    settings_bin="$(binary_path "${SETTINGS_BIN}" "$profile" "$triple")"
    daemon_bin="$(binary_path "${DAEMON_BIN}"  "$profile" "$triple")"

    if [ ! -f "$bin" ]; then
        error "找不到主二进制: $bin"
        return 1
    fi

    local arch_suffix
    arch_suffix="$(arch_from_triple "$triple")"
    local stage_name="pureterm-${VERSION}-${arch_suffix}-${triple##*-}"
    local stage="${DIST_DIR}/${stage_name}"
    local bin_install_dir="${stage}/bin"
    local data_dir="${stage}/share"

    step "组装 Linux 目录树: ${stage}"

    rm -rf "$stage"
    mkdir -p "${bin_install_dir}" \
             "${data_dir}/applications" \
             "${data_dir}/metainfo" \
             "${data_dir}/icons/hicolor/256x256/apps" \
             "${data_dir}/icons/hicolor/scalable/apps" \
             "${data_dir}/doc/pureterm" \
             "${data_dir}/man/man1"

    # 二进制。
    cp "$bin" "${bin_install_dir}/${BINARY_NAME}"
    chmod +x "${bin_install_dir}/${BINARY_NAME}"
    if [ -f "$settings_bin" ]; then
        cp "$settings_bin" "${bin_install_dir}/${SETTINGS_BIN}"
        chmod +x "${bin_install_dir}/${SETTINGS_BIN}"
    fi
    if [ -f "$daemon_bin" ]; then
        cp "$daemon_bin" "${bin_install_dir}/${DAEMON_BIN}"
        chmod +x "${bin_install_dir}/${DAEMON_BIN}"
    fi

    # .desktop—— 把 Exec= 改写为暂存绝对路径。
    if [ -f "${SCRIPT_DIR}/extra/linux/pureterm.desktop" ]; then
        sed "s|^Exec=pureterm$|Exec=${bin_install_dir}/${BINARY_NAME}|" \
            "${SCRIPT_DIR}/extra/linux/pureterm.desktop" \
            > "${data_dir}/applications/pureterm.desktop"
        chmod 644 "${data_dir}/applications/pureterm.desktop"
    fi

    # AppStream 元数据。
    if [ -f "${SCRIPT_DIR}/extra/linux/pureterm.appdata.xml" ]; then
        cp "${SCRIPT_DIR}/extra/linux/pureterm.appdata.xml" \
           "${data_dir}/metainfo/pureterm.appdata.xml"
        chmod 644 "${data_dir}/metainfo/pureterm.appdata.xml"
    fi

    # 图标。
    if [ -f "${SCRIPT_DIR}/assets/icon.png" ]; then
        cp "${SCRIPT_DIR}/assets/icon.png" \
           "${data_dir}/icons/hicolor/256x256/apps/pureterm.png"
    fi
    if [ -f "${SCRIPT_DIR}/assets/${PROJECT_NAME}.svg" ]; then
        cp "${SCRIPT_DIR}/assets/${PROJECT_NAME}.svg" \
           "${data_dir}/icons/hicolor/scalable/apps/pureterm.svg"
    fi

    # 文档。
    if [ -f "${SCRIPT_DIR}/LICENSE" ]; then
        cp "${SCRIPT_DIR}/LICENSE" "${data_dir}/doc/pureterm/copyright"
    fi
    if [ -f "${SCRIPT_DIR}/README.md" ]; then
        cp "${SCRIPT_DIR}/README.md" "${data_dir}/doc/pureterm/README.md"
    fi
    if [ -f "${SCRIPT_DIR}/build/linux/pureterm.1" ]; then
        cp "${SCRIPT_DIR}/build/linux/pureterm.1" "${data_dir}/man/man1/pureterm.1"
    fi

    # INSTALL.md(安装说明)
    cat > "${stage}/INSTALL.md" <<'INSTALL'
# PureTerm — Linux 安装说明

本目录的布局遵循 GNU Coding Standards / Freedesktop 规范。

## 用户级安装 (无需 root)

把 `bin/`, `share/`, `LICENSE` 等内容合并到 `~/.local/`:

```bash
cp -r bin/*           ~/.local/bin/
cp -r share/*         ~/.local/share/
cp LICENSE            ~/.local/share/doc/pureterm/copyright
```

> 确认 `~/.local/bin` 在 `PATH` 中(参见 ~/.bashrc / ~/.zshrc)。

## 系统级安装 (需要 root,使用 /opt)

```bash
sudo cp -r . /opt/pureterm
sudo ln -sf /opt/pureterm/bin/pureterm /usr/local/bin/pureterm
```

## 卸载

```bash
sudo rm -rf /opt/pureterm
sudo rm -f  /usr/local/bin/pureterm
# 用户级: rm -rf ~/.local/share/pureterm ~/.local/bin/pureterm
```

INSTALL
    chmod 644 "${stage}/INSTALL.md"

    success "staged tree 准备完成: ${stage} ($(du -sh "$stage" | cut -f1))"

    # ---- tar.gz(总是)----
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        local out="${DIST_DIR}/${stage_name}.tar.gz"
        step "打包 .tar.gz: ${out}"
        # `cd "$DIST_DIR"` 之后输出路径必须相对该目录,
        # 否则 "./dist/xxx.tar.gz" 会解析成 dist/dist/xxx.tar.gz。
        # (macOS 的 bundle_macos_zip 早已如此处理。)
        local out_name
        out_name="$(basename "$out")"
        (cd "$DIST_DIR" && tar -czf "$out_name" "$stage_name")
        success "已生成: ${out} ($(du -h "$out" | cut -f1))"

        # ---- tar.zst(可选)----
        if [ "${OPT_BUNDLE:-all}" = "all" ] && [ -n "${TOOL_ZSTD}" ]; then
            local out_zst="${DIST_DIR}/${stage_name}.tar.zst"
            local out_zst_name
            out_zst_name="$(basename "$out_zst")"
            step "打包 .tar.zst: ${out_zst}"
            (cd "$DIST_DIR" && tar -cf - "$stage_name" | zstd -T0 -19 -o "$out_zst_name")
            success "已生成: ${out_zst} ($(du -h "$out_zst" | cut -f1))"
        fi
    fi

    # ---- 直接安装(可选,--install 门控)----
    if [ "${OPT_INSTALL:-no}" = "yes" ]; then
        local target="/opt/pureterm-${VERSION}"
        step "安装到 ${target}..."
        if [ ! -w "/opt" ] && [ -w "/opt/.." ]; then
            if command -v sudo &>/dev/null; then
                sudo cp -r "$stage" "$target"
                sudo ln -sf "${target}/bin/${BINARY_NAME}" "/usr/local/bin/${BINARY_NAME}"
            else
                error "/opt 不可写且未找到 sudo — 跳过 --install"
                return 0
            fi
        else
            cp -r "$stage" "$target"
            ln -sf "${target}/bin/${BINARY_NAME}" "/usr/local/bin/${BINARY_NAME}"
        fi
        success "已安装到 ${target},符号链接 /usr/local/bin/${BINARY_NAME}"
    fi
}

# -----------------------------------------------------------------------------
# Windows 打包器—— 产出目录 + .zip(signtool 可选)。
# -----------------------------------------------------------------------------
bundle_windows() {
    local triple="$1"
    local profile="${2:-release}"

    local bin settings_bin daemon_bin
    bin="$(binary_path "${BINARY_NAME}"    "$profile" "$triple")"
    settings_bin="$(binary_path "${SETTINGS_BIN}" "$profile" "$triple")"
    daemon_bin="$(binary_path "${DAEMON_BIN}"  "$profile" "$triple")"

    if [ ! -f "$bin" ]; then
        error "找不到主二进制: $bin"
        return 1
    fi

    local arch_suffix
    arch_suffix="$(arch_from_triple "$triple")"
    local stage_name="pureterm-${VERSION}-${arch_suffix}"
    local stage="${DIST_DIR}/${stage_name}"

    step "组装 Windows 目录树: ${stage}"

    rm -rf "$stage"
    mkdir -p "$stage"

    cp "$bin" "${stage}/${BINARY_NAME}.exe"
    if [ -f "$settings_bin" ]; then
        cp "$settings_bin" "${stage}/${SETTINGS_BIN}.exe"
    fi
    if [ -f "$daemon_bin" ]; then
        cp "$daemon_bin" "${stage}/${DAEMON_BIN}.exe"
    fi

    if [ -f "${SCRIPT_DIR}/assets/${PROJECT_NAME}.ico" ]; then
        cp "${SCRIPT_DIR}/assets/${PROJECT_NAME}.ico" "${stage}/"
    fi
    if [ -f "${SCRIPT_DIR}/LICENSE" ]; then
        cp "${SCRIPT_DIR}/LICENSE" "${stage}/LICENSE"
    fi

    # README-Windows.md(缺失时自动生成桩)。
    if [ ! -f "${SCRIPT_DIR}/build/windows/README-Windows.md" ]; then
        cat > "${stage}/README-Windows.md" <<'WINREADME'
# PureTerm Windows 版

本目录包含便携版 PureTerm。

## 运行

双击 `pureterm.exe` 即可。

## 已知限制 (v1)

- 仅便携式 .zip 打包,无 MSI / NSIS 安装器
  (需要时再发 ticket)。
- 代码签名可选 — 设置 `WINDOWS_CERT_FILE` 与 `WINDOWS_CERT_PASSWORD`
  环境变量并使用 `--sign` 标志即可。

## 反馈

在 GitHub 仓库开 issue。
WINREADME
    else
        cp "${SCRIPT_DIR}/build/windows/README-Windows.md" "${stage}/README-Windows.md"
    fi

    success "staged tree 准备完成: ${stage} ($(du -sh "$stage" | cut -f1))"

    # ---- signtool(可选)----
    if [ "${OPT_SIGN:-no}" = "yes" ] && [ -n "${TOOL_SIGNTOOL}" ] && \
       [ -n "${WINDOWS_CERT_FILE:-}" ] && [ -n "${WINDOWS_CERT_PASSWORD:-}" ]; then
        step "signtool 签名 .exe..."
        local exe
        for exe in "${stage}/"*.exe; do
            signtool sign /f "$WINDOWS_CERT_FILE" /p "$WINDOWS_CERT_PASSWORD" \
                /tr http://timestamp.digicert.com /td sha256 /fd sha256 "$exe" \
                || warn "signtool 失败: $exe"
        done
    elif [ "${OPT_SIGN:-no}" = "yes" ]; then
        warn "未配置 WINDOWS_CERT_FILE / WINDOWS_CERT_PASSWORD — 跳过签名"
    fi

    # ---- zip(打包时总是)----
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        local out="${DIST_DIR}/${stage_name}.zip"
        local out_name
        out_name="$(basename "$out")"
        if [ -n "${TOOL_ZIP}" ]; then
            step "打包 .zip (zip): ${out}"
            # 同 Linux 打包器:cd 进 dist/ 后输出路径要相对该目录。
            (cd "$DIST_DIR" && rm -f "$out_name" && zip -yrq "$out_name" "$stage_name")
        elif [ -n "${TOOL_PWSH}" ] || [ -n "${TOOL_PWSH_CORE}" ]; then
            local pwsh_cmd="powershell"
            [ -n "${TOOL_PWSH_CORE}" ] && pwsh_cmd="pwsh"
            step "打包 .zip (Compress-Archive): ${out}"
            # PowerShell Compress-Archive 需要绝对目标路径。
            local abs_out
            abs_out="$(cd "$DIST_DIR" && pwd)/$(basename "$out")"
            (cd "$DIST_DIR" && \
                "$pwsh_cmd" -NoProfile -Command \
                "Compress-Archive -Path '${stage_name}/*' -DestinationPath '${abs_out}' -Force")
        else
            error "找不到 zip 或 PowerShell,无法打包 Windows .zip"
            return 1
        fi
        success "已生成: ${out} ($(du -h "$out" | cut -f1))"
    fi
}

# -----------------------------------------------------------------------------
# 打包分发器—— 按当前宿主的 OS 键选择正确的打包器。
# -----------------------------------------------------------------------------
bundle_for_host() {
    local triple="$1" profile="${2:-release}"
    local host
    host="$(host_os)"
    case "$host" in
        macos)   bundle_macos   "$triple" "$profile" ;;
        linux)   bundle_linux   "$triple" "$profile" ;;
        windows) bundle_windows "$triple" "$profile" ;;
        *)
            error "未知平台: $host"
            return 1
            ;;
    esac
}

# -----------------------------------------------------------------------------
# macOS universal2 打包器—— 构建 arm64 + x86_64 双架构,运行 `lipo -create`
# 把每个 Mach-O 合并为 `Contents/MacOS` 内单个 universal2 二进制,
# 再组装一个在 Intel 与 Apple
# Silicon Mac 上都能原生运行的 .app。主流 Rust 原生终端 macOS App 皆如此。
# -----------------------------------------------------------------------------
bundle_macos_universal() {
    local arm64_triple="aarch64-apple-darwin"
    local x64_triple="x86_64-apple-darwin"
    local profile="release"

    if [ "$(host_os)" != "macos" ]; then
        error "--universal 只在主机为 macOS 时生效 (cross-build universal 需要 macOS toolchain)"
        return 1
    fi
    if ! command -v lipo &>/dev/null; then
        error "未找到 lipo — universal 二进制需要在主机 macOS (Xcode CLT) 上打包"
        return 1
    fi

    step "开始 macOS universal2 构建 (arm64 + x86_64)..."

    # 构建两种架构。两者必须都成功—— 与 matrix
    # 模式(失败即跳过)不同,universal 是显式选择,部分构建
    # 即硬错误。
    ensure_target "$arm64_triple" || return 1
    ensure_target "$x64_triple"   || return 1
    build_all_binaries "$profile" "$arm64_triple" || return 1
    build_all_binaries "$profile" "$x64_triple"   || return 1

    # 把每架构二进制暂存到临时目录,供 `bundle_macos`
    # 从中拷贝。通过把每对二进制拷进平行的
    # `tmp_universal/<triple>/release/` 布局来覆盖 `binary_path` 语义。
    local tmp_universal
    tmp_universal="$(mktemp -d -t pureterm-universal)"
    trap "rm -rf '$tmp_universal'" RETURN

    local arch_specific=("arm64" "x64")
    local triples=("$arm64_triple" "$x64_triple")
    local i
    for i in 0 1; do
        local t="${triples[$i]}"
        local dir="${tmp_universal}/${t}/release"
        mkdir -p "$dir"
        cp "target/${t}/release/${BINARY_NAME}"     "${dir}/${BINARY_NAME}"     2>/dev/null || return 1
        cp "target/${t}/release/${SETTINGS_BIN}"   "${dir}/${SETTINGS_BIN}"   2>/dev/null || true
        cp "target/${t}/release/${DAEMON_BIN}"     "${dir}/${DAEMON_BIN}"     2>/dev/null || true
    done

    # 现在构建 .app,但源是每架构暂存目录。我们
    # 做一次性组装(不经 `bundle_macos`,因为该函数
    # 只预期单个 triple)。
    local app_dir="${DIST_DIR}/${PROJECT_NAME}.app"
    rm -rf "$app_dir"
    mkdir -p "${app_dir}/Contents/MacOS" \
             "${app_dir}/Contents/Resources" \
             "${app_dir}/Contents/Frameworks"

    # Info.plist(应用信息)
    bundle_macos_write_info_plist "$app_dir"

    # 用 lipo 合并二进制为 universal2 输出。
    local arm_bin="${tmp_universal}/${arm64_triple}/release/${BINARY_NAME}"
    local x64_bin="${tmp_universal}/${x64_triple}/release/${BINARY_NAME}"
    if [ ! -f "$arm_bin" ] || [ ! -f "$x64_bin" ]; then
        error "universal lipo 失败: 缺至少一个架构的二进制"
        return 1
    fi
    step "lipo -create arm64 + x86_64 -> ${BINARY_NAME} (universal2)"
    lipo -create \
        -arch arm64 "$arm_bin" \
        -arch x86_64 "$x64_bin" \
        -output "${app_dir}/Contents/MacOS/${BINARY_NAME}"
    chmod +x "${app_dir}/Contents/MacOS/${BINARY_NAME}"

    # 辅助二进制同样处理(尽力而为)。
    for helper in "${SETTINGS_BIN}" "${DAEMON_BIN}"; do
        local a="${tmp_universal}/${arm64_triple}/release/${helper}"
        local x="${tmp_universal}/${x64_triple}/release/${helper}"
        if [ -f "$a" ] && [ -f "$x" ]; then
            lipo -create \
                -arch arm64 "$a" \
                -arch x86_64 "$x" \
                -output "${app_dir}/Contents/MacOS/${helper}" || warn "lipo ${helper} 失败"
            chmod +x "${app_dir}/Contents/MacOS/${helper}"
        fi
    done

    # 图标。
    if [ -f "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" ]; then
        cp "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" \
           "${app_dir}/Contents/Resources/${PROJECT_NAME}.icns"
    fi
    rm -f "${app_dir}/Contents/Resources/icon.png"

    success "macOS universal2 .app 创建完成: ${app_dir} ($(du -sh "$app_dir" | cut -f1))"

    # 健全性检查。
    if command -v lipo &>/dev/null; then
        step "验证 universal2 binary:"
        file "${app_dir}/Contents/MacOS/${BINARY_NAME}"
    fi

    # 按要求 zip + dmg + 签名 + 公证。
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        bundle_macos_zip "$app_dir" "${PROJECT_NAME}-${VERSION}-universal"
    fi
    if [ "${OPT_BUNDLE:-all}" = "all" ] || [ "${OPT_BUNDLE:-}" = "dmg" ]; then
        if [ -n "${TOOL_HDIUTIL}" ]; then
            bundle_macos_dmg "$app_dir" "${PROJECT_NAME}-${VERSION}-universal"
        fi
    fi

    # 签名 / 公证(与单架构同路径)。
    bundle_macos_codesign_notarize "$app_dir"
}

# bundle_macos(单架构)与 bundle_macos_universal
# 共用的 Info.plist 输出辅助。总是设置 `CFBundleIconFile = PureTerm`。
bundle_macos_write_info_plist() {
    local app_dir="$1"
    if [ -f "${SCRIPT_DIR}/extra/osx/Info.plist" ]; then
        cp "${SCRIPT_DIR}/extra/osx/Info.plist" "${app_dir}/Contents/Info.plist"
        # PlistBuddy 用绝对路径解析—— 它在 macOS 的 `/usr/libexec/PlistBuddy`
        # 上,但许多场景(CI、新 shell)裸 PATH 上没有。
        # `plutil` 也装在同一前缀下;PlistBuddy
        # 完全不存在时回退。
        local plist="${app_dir}/Contents/Info.plist"
        local pb
        pb="/usr/libexec/PlistBuddy"
        if [ ! -x "$pb" ]; then
            pb="$(command -v PlistBuddy 2>/dev/null || true)"
        fi
        if [ -x "$pb" ]; then
            # `Set` 只更新已存在的键;用 `Add` 才能在源
            # extra/osx/Info.plist 未带 CFBundleIconFile /
            # NSHumanReadableCopyright 时也引入它们(这正是当前
            # 状态—— 否则 Finder 回退到通用图标)。
            "$pb" -c "Set :CFBundleShortVersionString ${VERSION}" "$plist" >/dev/null 2>&1 || true
            "$pb" -c "Set :CFBundleVersion 1" "$plist" >/dev/null 2>&1 || true
            "$pb" -c "Add :CFBundleIconFile string ${PROJECT_NAME}" "$plist" >/dev/null 2>&1 || \
                "$pb" -c "Set :CFBundleIconFile ${PROJECT_NAME}" "$plist" >/dev/null 2>&1 || true
            "$pb" -c "Add :NSHumanReadableCopyright string 'Copyright $(date +%Y) ${PROJECT_NAME}.'" \
                   "$plist" >/dev/null 2>&1 || \
                "$pb" -c "Set :NSHumanReadableCopyright 'Copyright $(date +%Y) ${PROJECT_NAME}.'" \
                       "$plist" >/dev/null 2>&1 || true
        elif command -v plutil &>/dev/null; then
            # plutil 兜底:已设置的键用
            # `plutil -replace`,缺失的键用 `plutil -insert` 写入。
            local copyright_str="Copyright $(date +%Y) ${PROJECT_NAME}."
            plutil -replace CFBundleShortVersionString -string "${VERSION}" "$plist" >/dev/null 2>&1 || true
            plutil -replace CFBundleVersion -string "1" "$plist" >/dev/null 2>&1 || true
            plutil -insert CFBundleIconFile -string "${PROJECT_NAME}" "$plist" >/dev/null 2>&1 || \
                plutil -replace CFBundleIconFile -string "${PROJECT_NAME}" "$plist" >/dev/null 2>&1 || true
            plutil -insert NSHumanReadableCopyright -string "$copyright_str" "$plist" >/dev/null 2>&1 || \
                plutil -replace NSHumanReadableCopyright -string "$copyright_str" "$plist" >/dev/null 2>&1 || true
        else
            warn "找不到 PlistBuddy 或 plutil — 无法设置 CFBundleIconFile;Finder 将显示通用图标"
        fi
    else
        cat > "${app_dir}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleExecutable</key><string>${BINARY_NAME}</string>
    <key>CFBundleIdentifier</key><string>com.pureterm.app</string>
    <key>CFBundleName</key><string>${PROJECT_NAME}</string>
    <key>CFBundleDisplayName</key><string>${PROJECT_NAME}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${VERSION}</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleIconFile</key><string>${PROJECT_NAME}</string>
    <key>LSMinimumSystemVersion</key><string>10.15</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSHumanReadableCopyright</key><string>Copyright $(date +%Y) ${PROJECT_NAME}. All rights reserved.</string>
</dict>
</plist>
PLIST
    fi
}

# 签名 + 公证共享路径(从 bundle_macos 抽出复用)。
bundle_macos_codesign_notarize() {
    local app_dir="$1"

    # ---- 代码签名(可选)----
    if [ "${OPT_SIGN:-no}" = "yes" ] && [ -n "${TOOL_CODESIGN}" ] && \
       [ -n "${CODESIGN_IDENTITY:-}" ]; then
        step "codesign (identity=${CODESIGN_IDENTITY})..."
        codesign --force --deep --options runtime --sign "$CODESIGN_IDENTITY" \
                 "${app_dir}" || warn "codesign 失败 (继续)"
    elif [ "${OPT_SIGN:-no}" = "yes" ]; then
        warn "未配置 CODESIGN_IDENTITY — 跳过签名"
    fi

    # ---- 公证(可选)----
    if [ "${OPT_NOTARIZE:-no}" = "yes" ] && [ -n "${TOOL_NOTARY}" ] && \
       [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
        step "notarytool 提交公证..."
        local app_name
        app_name="$(basename "$app_dir")"
        local zip_for_notary
        zip_for_notary="${DIST_DIR}/${app_name%.app}.app.zip"
        if [ ! -f "$zip_for_notary" ]; then
            (cd "$DIST_DIR" && zip -yrq "$(basename "$zip_for_notary")" "$app_name")
        fi
        xcrun notarytool submit "$zip_for_notary" \
            --apple-id "$APPLE_ID" \
            --password "$APPLE_PASSWORD" \
            --team-id "$APPLE_TEAM_ID" \
            --wait || warn "notarytool 失败 (继续)"
        xcrun staple staple "${app_dir}" 2>/dev/null || true
    elif [ "${OPT_NOTARIZE:-no}" = "yes" ]; then
        warn "未配置 APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID — 跳过公证"
    fi
}

# -----------------------------------------------------------------------------
# 顶层模式:app、app-all、matrix
# -----------------------------------------------------------------------------
mode_app() {
    # macOS 上 --universal 优先—— 产出单个在
    # Apple Silicon 与 Intel 都原生运行的 .app。否则为
    # (显式或宿主)target 构建 + 打包。
    if [ "${OPT_UNIVERSAL:-no}" = "yes" ] && [ "$(host_os)" = "macos" ]; then
        bundle_macos_universal
        return $?
    fi

    # 默认宿主架构(Apple Silicon 上为 arm64),使 .app
    # 开箱即不触发 Rosetta。--target 可覆盖。
    local triple="${TARGETS[0]:-${HOST_TRIPLE:-}}"
    if [ -n "$triple" ] && [ "$triple" != "${HOST_TRIPLE:-}" ]; then
        ensure_target "$triple" || return 1
    fi
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        build_all_binaries release "$triple" || return 1
    else
        cargo_build_one "${BINARY_NAME}" "${BINARY_NAME}" "release" "$triple" || return 1
    fi
    bundle_for_host "$triple" release
}

mode_app_all() {
    local triple
    for triple in "${TARGETS[@]}"; do
        if ! ensure_target "$triple"; then
            continue
        fi
        step "==== app-all: ${triple} ===="
        if ! build_all_binaries release "$triple"; then
            warn "构建失败: ${triple} (继续下一个)"
            continue
        fi
        bundle_for_host "$triple" release || warn "打包失败: ${triple} (继续)"
    done
}

mode_matrix() {
    local entry os_key triple arch_suffix ok
    ok=0
    for entry in "${TARGET_MATRIX[@]}"; do
        IFS=':' read -r os_key triple arch_suffix <<<"$entry"
        step "==== matrix: ${os_key} / ${triple} ===="

        # 交叉编译构建尽力而为:即使 rustup target
        # 已安装,宿主可能没有对应 sysroot / 链接器。
        # 尝试构建;失败则警告并继续。
        if ! ensure_target "$triple"; then
            continue
        fi
        if ! build_all_binaries release "$triple"; then
            warn "构建失败: ${triple} (跨编译需要匹配的 sysroot/linker;继续下一个)"
            continue
        fi
        # 打包必须在与目标 OS 匹配的宿主上运行。
        # 跳过外 OS 的打包:例如 Linux 上产不出 .dmg。
        if [ "$os_key" != "$(host_os)" ]; then
            note "跳过 ${os_key} 打包: 当前主机 $(host_os) 不匹配"
            continue
        fi
        bundle_for_host "$triple" release || warn "打包失败: ${triple} (继续)"
        ok=$((ok + 1))
    done
    if [ "$ok" -eq 0 ]; then
        warn "matrix 模式未产出任何工件"
    fi
}

# =============================================================================
# 参数解析
# =============================================================================
MODE=""
CLEAN_BEFORE_BUILD="false"
TIMINGS_ENABLED="false"
TEST_PROFILE="dev"
ALL_FEATURES="false"
NO_RUN="false"
HOST_TRIPLE=""

OPT_TARGET=""
OPT_TARGETS=""
OPT_ARCH=""
OPT_BUNDLE=""
OPT_AUTO_INSTALL_TARGETS="no"
OPT_SIGN="no"
OPT_NOTARIZE="no"
OPT_INSTALL="no"
OPT_UNIVERSAL="no"
OPT_JOBS=""
OPT_SKIP_TESTS="no"

while [[ $# -gt 0 ]]; do
    case "$1" in
        debug|release|app|app-all|matrix|test|info|clean)
            if [ -n "$MODE" ]; then
                error "只能指定一个模式"
                usage
                exit 1
            fi
            MODE="$1"
            shift
            ;;
        --clean)                 CLEAN_BEFORE_BUILD="true"; shift ;;
        --timings)               TIMINGS_ENABLED="true"; shift ;;
        --target)
            [ -z "${2:-}" ] && { error "--target 需要指定 triple"; exit 1; }
            OPT_TARGET="$2"; shift 2 ;;
        --targets)
            [ -z "${2:-}" ] && { error "--targets 需要指定逗号分隔的 triple 列表"; exit 1; }
            OPT_TARGETS="$2"; shift 2 ;;
        --arch)
            [ -z "${2:-}" ] && { error "--arch 需要 x86_64 / aarch64 / all"; exit 1; }
            OPT_ARCH="$2"; shift 2 ;;
        --auto-install-targets)  OPT_AUTO_INSTALL_TARGETS="yes"; shift ;;
        --bundle)
            [ -z "${2:-}" ] && { error "--bundle 需要值"; exit 1; }
            case "$2" in
                app|dmg|tar|zip|none|all) OPT_BUNDLE="$2" ;;
                *) error "未知的 --bundle 值: $2 (允许: app / dmg / tar / zip / none / all)"; exit 1 ;;
            esac
            shift 2 ;;
        --no-bundle)             OPT_BUNDLE="none"; shift ;;
        --sign)                  OPT_SIGN="yes"; shift ;;
        --notarize)              OPT_NOTARIZE="yes"; shift ;;
        --install)               OPT_INSTALL="yes"; shift ;;
        --universal)             OPT_UNIVERSAL="yes"; shift ;;
        --jobs)
            [ -z "${2:-}" ] && { error "--jobs 需要 N"; exit 1; }
            OPT_JOBS="$2"; shift 2 ;;
        --profile)
            [ -z "${2:-}" ] && { error "--profile 需要 profile 名"; exit 1; }
            TEST_PROFILE="$2"; shift 2 ;;
        --all-features)          ALL_FEATURES="true"; shift ;;
        --no-run)                NO_RUN="true"; shift ;;
        --skip-tests)            OPT_SKIP_TESTS="yes"; shift ;;
        -h|--help)               usage; exit 0 ;;
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
    error "请指定构建模式 (debug / release / app / test / info / clean / ...)"
    usage
    exit 1
fi

# `info` 在预检前运行,用户无需 cargo check 即可调试环境。
if [ "$MODE" = "info" ]; then
    detect_toolchain
    mode_info
    exit 0
fi

preflight
detect_toolchain
HOST_TRIPLE="$(rustc -vV | sed -n 's|host: ||p')"

# 为需要它的任何模式解析 TARGETS[]。
case "$MODE" in
    app|app-all|matrix)
        expand_targets
        if [ "$MODE" = "matrix" ]; then
            # 原样使用静态矩阵;--arch / --target / --targets
            # 仍由 expand_targets 遵循。
            if [ -z "${OPT_TARGET:-}${OPT_TARGETS:-}${OPT_ARCH:-}" ]; then
                TARGETS=()
                _entry=""
                _triple=""
                for _entry in "${TARGET_MATRIX[@]}"; do
                    IFS=':' read -r _ _triple _ <<<"$_entry"
                    TARGETS+=("$_triple")
                done
                unset _entry _triple
            fi
        fi
        ;;
esac

local_platform="$(host_os)"
step "PureTerm v${VERSION} — 模式: ${MODE}, 主机: ${local_platform} (${HOST_TRIPLE})${TOOLCHAIN_INFO}"
if declare -p TARGETS &>/dev/null && [ "${#TARGETS[@]}" -gt 0 ]; then
    if [ "$MODE" = "matrix" ]; then
        info "matrix 目标: ${TARGETS[*]}"
    else
        info "目标: ${TARGETS[*]}"
    fi
fi

case "$MODE" in
    debug)   build_debug   ;;
    release) build_release ;;
    test)    build_test    ;;
    app)     mode_app      ;;
    app-all) mode_app_all  ;;
    matrix)  mode_matrix   ;;
    clean)   do_clean all  ;;
esac
