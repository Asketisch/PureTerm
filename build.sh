#!/usr/bin/env bash
# =============================================================================
# PureTerm cross-platform build & bundle script
# Supports: macOS (aarch64 / x86_64), Linux (gnu + musl, aarch64 / x86_64),
#           Windows MSVC (aarch64 / x86_64).
#
# Usage: ./build.sh [OPTIONS] <MODE>
#
# Modes:
#   debug     development build (dev profile)
#   release   optimised release build (thin LTO + strip)
#   test      run the test suite (nextest → fallback cargo test)
#   app       build + bundle for the host OS in the host target triple
#   app-all   build + bundle for the host OS in every installed target triple
#   matrix    build + bundle across the full target matrix
#             (skips targets whose toolchain isn't installed)
#   info      print detected platform, installed targets, available tools
#   clean     remove all build artefacts
#
# Run `./build.sh --help` for the full option list.
# =============================================================================
set -euo pipefail

# -----------------------------------------------------------------------------
# Project constants
# -----------------------------------------------------------------------------
readonly PROJECT_NAME="PureTerm"
readonly BINARY_NAME="pureterm"
readonly SETTINGS_BIN="pureterm-settings"
readonly DAEMON_BIN="puretermd"
readonly DIST_DIR="./dist"
readonly SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# -----------------------------------------------------------------------------
# Static target matrix
#
# Each entry: "<host-os-key>:<triple>:<arch-suffix>". `os-key` is what
# `bundle_<os>` switches on; `arch-suffix` becomes part of the bundle name
# (e.g. `aarch64-unknown-linux-gnu` → `linux-gnu-aarch64`).
#
# `matrix` and `app-all` only consider entries whose triple is in
# `rustup target list --installed`; entries the user can't build right now
# are skipped with a `WARN`, never a hard error.
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
# Version extraction (single source of truth: crates/pureterm/Cargo.toml)
# -----------------------------------------------------------------------------
extract_version() {
    grep '^version' "$SCRIPT_DIR/crates/pureterm/Cargo.toml" \
        | head -1 \
        | sed 's/.*"\([^"]*\)".*/\1/'
}
readonly VERSION="$(extract_version)"

# -----------------------------------------------------------------------------
# Colours (auto-disabled when piped or NO_COLOR set, per no-color.org)
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
# Help text
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
# Toolchain detection
# -----------------------------------------------------------------------------
# Populates: TOOL_* flags (yes/"") + TOOLCHAIN_INFO (pretty summary).
detect_toolchain() {
    local tools=()

    # sccache — silently degrade if missing.
    # The repo's .cargo/config.toml has `rustc-wrapper = "sccache"` which is
    # a hard config setting (not an env var), so we need to *override* it
    # at the cargo invocation level when sccache isn't on PATH. Otherwise
    # `cargo build` exits 127 with "process didn't exit successfully: sccache".
    if command -v sccache &>/dev/null; then
        TOOL_SCCACHE="yes"
        export RUSTC_WRAPPER=sccache
        tools+=("sccache")
    else
        TOOL_SCCACHE=""
        unset RUSTC_WRAPPER 2>/dev/null || true
    fi

    # cargo-nextest — double-check it actually works (PATH can lie).
    if command -v cargo-nextest &>/dev/null && cargo nextest --help &>/dev/null; then
        TOOL_NEXTEST="yes"
        tools+=("nextest")
    else
        TOOL_NEXTEST=""
    fi

    # Fast Linux linkers.
    local uname_s
    uname_s="$(uname -s)"
    if [ "$uname_s" = "Linux" ]; then
        if command -v mold &>/dev/null; then
            tools+=("mold")
        elif command -v lld &>/dev/null; then
            tools+=("lld")
        fi
    fi

    # Platform-specific bundlers — informational, NEVER fatal.
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

    # PowerShell on Windows for zip fallback (only relevant on Windows).
    command -v powershell   &>/dev/null && TOOL_PWSH="yes"         || TOOL_PWSH=""
    command -v pwsh         &>/dev/null && TOOL_PWSH_CORE="yes"    || TOOL_PWSH_CORE=""

    if [ ${#tools[@]} -gt 0 ]; then
        TOOLCHAIN_INFO=" (using: $(IFS=', '; echo "${tools[*]}"))"
    else
        TOOLCHAIN_INFO=""
    fi
}

# -----------------------------------------------------------------------------
# Platform / target detection
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

# Translate a triple's arch-token to our bundle arch-suffix.
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
# Target matrix expansion
# -----------------------------------------------------------------------------
# Input: --arch / --target / --targets (global vars).
# Output: TARGETS[] = list of triples to build, in stable order.
expand_targets() {
    local host
    host="$(host_os)"

    # Explicit --target wins.
    if [ -n "${OPT_TARGET:-}" ]; then
        TARGETS=("$OPT_TARGET")
        return
    fi

    # Explicit --targets (comma-separated) wins.
    if [ -n "${OPT_TARGETS:-}" ]; then
        IFS=',' read -ra TARGETS <<<"$OPT_TARGETS"
        return
    fi

    # --arch filter: emit matching entries from TARGET_MATRIX.
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

    # Default: just the host triple.
    TARGETS=("$(rustc -vV | sed -n 's|host: ||p')")
}

# Return 0 if triple is in `rustup target list --installed`, 1 otherwise.
target_installed() {
    local t="$1"
    rustup target list --installed 2>/dev/null | tr -d ' ' | grep -qx "$t"
}

# Make sure triple is installed; if not, optionally install (--auto-install-targets).
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
# Pre-flight: confirm Cargo workspace parses.
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
# Clean
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
# Binary path resolution
# -----------------------------------------------------------------------------
# Per-target binary directory under target/<triple>/<profile>/
target_dir() {
    local profile="$1" triple="${2:-${HOST_TRIPLE:-}}"
    if [ -n "$triple" ]; then
        echo "target/${triple}/${profile}"
    else
        echo "target/${profile}"
    fi
}

# Resolved on-disk path to a built binary for the given triple + profile.
binary_path() {
    local name="$1" profile="$2" triple="${3:-${HOST_TRIPLE:-}}"
    if [ -n "$triple" ]; then
        echo "target/${triple}/${profile}/${name}"
    else
        echo "target/${profile}/${name}"
    fi
}

# -----------------------------------------------------------------------------
# Cargo invocation helpers
# -----------------------------------------------------------------------------
# Emit the cargo `--config` flags needed to make this run robust.
#   - When sccache isn't on PATH, override the repo's `[build] rustc-wrapper
#     = "sccache"` so cargo doesn't invoke a missing binary.
cargo_config_args() {
    local args=()
    if [ -z "${TOOL_SCCACHE:-}" ]; then
        args+=(--config 'build.rustc-wrapper=""')
    fi
    printf '%s\n' "${args[@]}"
}

# Build one binary in one triple. Logs a clear error if the binary isn't on
# disk after a "successful" cargo build (catches missing --features cases).
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
    # Inject any --config overrides (e.g. disable sccache when missing).
    while IFS= read -r arg; do
        [ -n "$arg" ] && cmd+=("$arg")
    done < <(cargo_config_args)

    step "cargo: pkg=${pkg} bin=${bin} profile=${profile} target=${triple:-host}"
    if ! "${cmd[@]}"; then
        return 1
    fi

    # Verify the artifact actually exists.
    local path
    path="$(binary_path "$bin" "$profile" "$triple")"
    if [ ! -x "$path" ] && [ ! -f "$path" ]; then
        error "构建报告成功但找不到产物: $path"
        error "(检查 --features: ${bin} 可能需要额外的 feature flag)"
        return 1
    fi
    return 0
}

# Build all workspace binaries the bundler cares about.
# Args: <profile> <triple>
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
# Mode: build (debug / release) — no bundle, just compile for the host.
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
# Mode: test
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
# Mode: info
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
# Bundlers
# =============================================================================

# -----------------------------------------------------------------------------
# macOS bundler — produces .app (+ .app.zip, optional .dmg, optional codesign
# + notarize).
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

    # Fresh layout.
    rm -rf "$app_dir"
    mkdir -p "${app_dir}/Contents/MacOS" \
             "${app_dir}/Contents/Resources" \
             "${app_dir}/Contents/Frameworks"

    # Binaries.
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

    # Info.plist — both the helper-managed and inline-fallback branches are
    # served by `bundle_macos_write_info_plist`, which is shared with the
    # universal builder.
    bundle_macos_write_info_plist "$app_dir"

    # Icons.
    # macOS reads `Contents/Resources/PureTerm.icns` whenever Info.plist
    # declares `CFBundleIconFile = PureTerm` (the `.icns` extension is
    # implicit). We copy only the .icns — a stray `.png` next to it can
    # confuse Finder's icon cache and the `deficon` slot for documents.
    if [ -f "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" ]; then
        cp "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" \
           "${app_dir}/Contents/Resources/${PROJECT_NAME}.icns"
    else
        warn "缺少 assets/${PROJECT_NAME}.icns — .app 将没有图标"
    fi
    # Drop any cached icon.png; we keep assets/icon.png as the cross-platform
    # source icon (Linux/Windows), not a macOS bundle resource.
    rm -f "${app_dir}/Contents/Resources/icon.png"

    success "macOS .app 创建完成: ${app_dir} ($(du -sh "$app_dir" | cut -f1))"

    # ---- codesign (optional) ----
    if [ "${OPT_SIGN:-no}" = "yes" ] && [ -n "${TOOL_CODESIGN}" ] && \
       [ -n "${CODESIGN_IDENTITY:-}" ]; then
        step "codesign (identity=${CODESIGN_IDENTITY})..."
        codesign --force --deep --options runtime --sign "$CODESIGN_IDENTITY" \
                 "${app_dir}" || warn "codesign 失败 (继续)"
    elif [ "${OPT_SIGN:-no}" = "yes" ]; then
        warn "未配置 CODESIGN_IDENTITY — 跳过签名"
    fi

    # ---- notarize (optional) ----
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

    # ---- zip (always) ----
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        bundle_macos_zip "$app_dir" "$app_name"
    fi

    # ---- dmg (optional, requires hdiutil) ----
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
    # $app_dir is "./dist/PureTerm.app" relative to repo root.
    # After `cd $DIST_DIR`, the relative path is just "PureTerm.app".
    local rel_target
    rel_target="$(basename "$app_dir")"
    # `zip` is invoked from inside $DIST_DIR, so the output file must
    # also be relative to that directory — otherwise "./dist/..." doesn't
    # resolve after we `cd` into "./dist/".
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

    # Mountable read-write dmg first.
    rm -f "$rw_dmg" "$final_dmg"
    hdiutil create -srcfolder "$app_dir" -volname "$name" -fs HFS+ \
        -fsargs "-c c=64,a=16,e=16" -format UDRW "$rw_dmg" 2>/dev/null || \
    hdiutil create -srcfolder "$app_dir" -volname "$name" -fs HFS+ \
        -format UDRW "$rw_dmg"

    # Convert to compressed read-only (UDZO).
    hdiutil convert "$rw_dmg" -format UDZO -imagekey zlib-level=9 -o "$final_dmg"
    rm -f "$rw_dmg"
    success "已生成: ${final_dmg} ($(du -h "$final_dmg" | cut -f1))"
}

# -----------------------------------------------------------------------------
# Linux bundler — produces a staged tree + .tar.gz (and .tar.zst if zstd).
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

    # Binaries.
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

    # .desktop — rewrite Exec= to the staged absolute path.
    if [ -f "${SCRIPT_DIR}/extra/linux/pureterm.desktop" ]; then
        sed "s|^Exec=pureterm$|Exec=${bin_install_dir}/${BINARY_NAME}|" \
            "${SCRIPT_DIR}/extra/linux/pureterm.desktop" \
            > "${data_dir}/applications/pureterm.desktop"
        chmod 644 "${data_dir}/applications/pureterm.desktop"
    fi

    # AppStream metadata.
    if [ -f "${SCRIPT_DIR}/extra/linux/pureterm.appdata.xml" ]; then
        cp "${SCRIPT_DIR}/extra/linux/pureterm.appdata.xml" \
           "${data_dir}/metainfo/pureterm.appdata.xml"
        chmod 644 "${data_dir}/metainfo/pureterm.appdata.xml"
    fi

    # Icons.
    if [ -f "${SCRIPT_DIR}/assets/icon.png" ]; then
        cp "${SCRIPT_DIR}/assets/icon.png" \
           "${data_dir}/icons/hicolor/256x256/apps/pureterm.png"
    fi
    if [ -f "${SCRIPT_DIR}/assets/${PROJECT_NAME}.svg" ]; then
        cp "${SCRIPT_DIR}/assets/${PROJECT_NAME}.svg" \
           "${data_dir}/icons/hicolor/scalable/apps/pureterm.svg"
    fi

    # Documentation.
    if [ -f "${SCRIPT_DIR}/LICENSE" ]; then
        cp "${SCRIPT_DIR}/LICENSE" "${data_dir}/doc/pureterm/copyright"
    fi
    if [ -f "${SCRIPT_DIR}/README.md" ]; then
        cp "${SCRIPT_DIR}/README.md" "${data_dir}/doc/pureterm/README.md"
    fi
    if [ -f "${SCRIPT_DIR}/build/linux/pureterm.1" ]; then
        cp "${SCRIPT_DIR}/build/linux/pureterm.1" "${data_dir}/man/man1/pureterm.1"
    fi

    # INSTALL.md
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

    # ---- tar.gz (always) ----
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        local out="${DIST_DIR}/${stage_name}.tar.gz"
        step "打包 .tar.gz: ${out}"
        (cd "$DIST_DIR" && tar -czf "$out" "$stage_name")
        success "已生成: ${out} ($(du -h "$out" | cut -f1))"

        # ---- tar.zst (optional) ----
        if [ "${OPT_BUNDLE:-all}" = "all" ] && [ -n "${TOOL_ZSTD}" ]; then
            local out_zst="${DIST_DIR}/${stage_name}.tar.zst"
            step "打包 .tar.zst: ${out_zst}"
            (cd "$DIST_DIR" && tar -cf - "$stage_name" | zstd -T0 -19 -o "$out_zst")
            success "已生成: ${out_zst} ($(du -h "$out_zst" | cut -f1))"
        fi
    fi

    # ---- direct install (optional, gated behind --install) ----
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
# Windows bundler — produces a folder + .zip (signtool optional).
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

    # README-Windows.md (auto-stub if missing).
    if [ ! -f "${SCRIPT_DIR}/build/windows/README-Windows.md" ]; then
        cat > "${stage}/README-Windows.md" <<'WINREADME'
# PureTerm (Windows)

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

    # ---- signtool (optional) ----
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

    # ---- zip (always when bundling) ----
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        local out="${DIST_DIR}/${stage_name}.zip"
        if [ -n "${TOOL_ZIP}" ]; then
            step "打包 .zip (zip): ${out}"
            (cd "$DIST_DIR" && rm -f "$out" && zip -yrq "$out" "$stage_name")
        elif [ -n "${TOOL_PWSH}" ] || [ -n "${TOOL_PWSH_CORE}" ]; then
            local pwsh_cmd="powershell"
            [ -n "${TOOL_PWSH_CORE}" ] && pwsh_cmd="pwsh"
            step "打包 .zip (Compress-Archive): ${out}"
            # PowerShell Compress-Archive needs an absolute destination path.
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
# Bundle dispatcher — pick the right bundler for the current host's OS key.
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
# macOS universal2 bundler — builds both arm64 + x86_64, runs `lipo -create`
# to merge each Mach-O into a single universal2 binary inside `Contents/MacOS`,
# then assembles a single .app that runs natively on both Intel and Apple
# Silicon Macs. This is what mainstream Rust-native terminal macOS apps ship.
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

    # Build both architectures. We need BOTH to succeed — unlike the matrix
    # mode (which skips on failure), universal is opt-in so a partial build
    # is a hard error.
    ensure_target "$arm64_triple" || return 1
    ensure_target "$x64_triple"   || return 1
    build_all_binaries "$profile" "$arm64_triple" || return 1
    build_all_binaries "$profile" "$x64_triple"   || return 1

    # Stage per-arch binaries into a tmp directory so `bundle_macos` can
    # copy from there. We override `binary_path` semantics by copying each
    # pair into a parallel `tmp_universal/<triple>/release/` layout.
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

    # Now build the .app, but with per-arch staging dirs as the source. We
    # do a one-off assembly (not through `bundle_macos` because that function
    # expects a single triple).
    local app_dir="${DIST_DIR}/${PROJECT_NAME}.app"
    rm -rf "$app_dir"
    mkdir -p "${app_dir}/Contents/MacOS" \
             "${app_dir}/Contents/Resources" \
             "${app_dir}/Contents/Frameworks"

    # Info.plist
    bundle_macos_write_info_plist "$app_dir"

    # Combine binaries with lipo for universal2 output.
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

    # Same for the helper binaries (best-effort).
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

    # Icons.
    if [ -f "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" ]; then
        cp "${SCRIPT_DIR}/assets/${PROJECT_NAME}.icns" \
           "${app_dir}/Contents/Resources/${PROJECT_NAME}.icns"
    fi
    rm -f "${app_dir}/Contents/Resources/icon.png"

    success "macOS universal2 .app 创建完成: ${app_dir} ($(du -sh "$app_dir" | cut -f1))"

    # Sanity check.
    if command -v lipo &>/dev/null; then
        step "验证 universal2 binary:"
        file "${app_dir}/Contents/MacOS/${BINARY_NAME}"
    fi

    # Zip + dmg + codesign + notary if requested.
    if [ "${OPT_BUNDLE:-all}" != "none" ]; then
        bundle_macos_zip "$app_dir" "${PROJECT_NAME}-${VERSION}-universal"
    fi
    if [ "${OPT_BUNDLE:-all}" = "all" ] || [ "${OPT_BUNDLE:-}" = "dmg" ]; then
        if [ -n "${TOOL_HDIUTIL}" ]; then
            bundle_macos_dmg "$app_dir" "${PROJECT_NAME}-${VERSION}-universal"
        fi
    fi

    # Codesign / notarize (same path as single-arch).
    bundle_macos_codesign_notarize "$app_dir"
}

# Helper used by both bundle_macos (single arch) and bundle_macos_universal
# to emit Info.plist. Always sets `CFBundleIconFile = PureTerm`.
bundle_macos_write_info_plist() {
    local app_dir="$1"
    if [ -f "${SCRIPT_DIR}/extra/osx/Info.plist" ]; then
        cp "${SCRIPT_DIR}/extra/osx/Info.plist" "${app_dir}/Contents/Info.plist"
        # Resolve PlistBuddy by absolute path — it's at `/usr/libexec/PlistBuddy`
        # on macOS but not on bare PATH in many contexts (CI, fresh shells).
        # `plutil` is also installed under the same prefix; fall back if
        # PlistBuddy isn't there at all.
        local plist="${app_dir}/Contents/Info.plist"
        local pb
        pb="/usr/libexec/PlistBuddy"
        if [ ! -x "$pb" ]; then
            pb="$(command -v PlistBuddy 2>/dev/null || true)"
        fi
        if [ -x "$pb" ]; then
            # `Set` only updates existing keys; use `Add` so we can introduce
            # CFBundleIconFile / NSHumanReadableCopyright even when the source
            # extra/osx/Info.plist doesn't ship them (which is the current
            # state — Finder then falls back to a generic icon).
            "$pb" -c "Set :CFBundleShortVersionString ${VERSION}" "$plist" >/dev/null 2>&1 || true
            "$pb" -c "Set :CFBundleVersion 1" "$plist" >/dev/null 2>&1 || true
            "$pb" -c "Add :CFBundleIconFile string ${PROJECT_NAME}" "$plist" >/dev/null 2>&1 || \
                "$pb" -c "Set :CFBundleIconFile ${PROJECT_NAME}" "$plist" >/dev/null 2>&1 || true
            "$pb" -c "Add :NSHumanReadableCopyright string 'Copyright $(date +%Y) ${PROJECT_NAME}.'" \
                   "$plist" >/dev/null 2>&1 || \
                "$pb" -c "Set :NSHumanReadableCopyright 'Copyright $(date +%Y) ${PROJECT_NAME}.'" \
                       "$plist" >/dev/null 2>&1 || true
        elif command -v plutil &>/dev/null; then
            # plutil fallback: keys that aren't already set are inserted via
            # `plutil -replace`; absent keys are written with `plutil -insert`.
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

# Shared codesign + notary path (extracted from bundle_macos for reuse).
bundle_macos_codesign_notarize() {
    local app_dir="$1"

    # ---- codesign (optional) ----
    if [ "${OPT_SIGN:-no}" = "yes" ] && [ -n "${TOOL_CODESIGN}" ] && \
       [ -n "${CODESIGN_IDENTITY:-}" ]; then
        step "codesign (identity=${CODESIGN_IDENTITY})..."
        codesign --force --deep --options runtime --sign "$CODESIGN_IDENTITY" \
                 "${app_dir}" || warn "codesign 失败 (继续)"
    elif [ "${OPT_SIGN:-no}" = "yes" ]; then
        warn "未配置 CODESIGN_IDENTITY — 跳过签名"
    fi

    # ---- notarize (optional) ----
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
# Top-level modes: app, app-all, matrix
# -----------------------------------------------------------------------------
mode_app() {
    # --universal takes precedence on macOS — produces a single .app that runs
    # natively on both Apple Silicon and Intel. Otherwise build + bundle for
    # the (explicit or host) target.
    if [ "${OPT_UNIVERSAL:-no}" = "yes" ] && [ "$(host_os)" = "macos" ]; then
        bundle_macos_universal
        return $?
    fi

    # Default to the host arch (arm64 on Apple Silicon) so the .app doesn't
    # trigger Rosetta on day one. --target can override.
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

        # Cross-compile build is best-effort: even if the rustup target is
        # installed, the host may not have a sysroot / linker for it.
        # We try the build; on failure we warn and continue.
        if ! ensure_target "$triple"; then
            continue
        fi
        if ! build_all_binaries release "$triple"; then
            warn "构建失败: ${triple} (跨编译需要匹配的 sysroot/linker;继续下一个)"
            continue
        fi
        # Bundling must run on a host that matches the target OS.
        # Skip foreign-OS bundles: e.g. we can't produce a .dmg on Linux.
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
# Argument parsing
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
# Main
# =============================================================================
if [ -z "$MODE" ]; then
    error "请指定构建模式 (debug / release / app / test / info / clean / ...)"
    usage
    exit 1
fi

# `info` runs before preflight so users can debug env without a cargo check.
if [ "$MODE" = "info" ]; then
    detect_toolchain
    mode_info
    exit 0
fi

preflight
detect_toolchain
HOST_TRIPLE="$(rustc -vV | sed -n 's|host: ||p')"

# Resolve TARGETS[] for any mode that needs it.
case "$MODE" in
    app|app-all|matrix)
        expand_targets
        if [ "$MODE" = "matrix" ]; then
            # Use the static matrix verbatim; --arch / --target / --targets are
            # still honored by expand_targets.
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
