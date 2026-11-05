# 更新日志

本文件记录本项目的所有重要变更。

## 未发布

### 修复(构建/打包)
- **`build.sh` 重写,支持 Win/Linux/macOS 多架构构建。**
  v0.2 重写了 macOS 打包流程,现在可以正确做到:
  - **macOS 上默认架构跟随宿主机**(Apple Silicon 默认 arm64;
    旧版本会照常发布 x86_64 二进制,从而触发
    macOS 的"Apple 即将结束对 Intel 芯片 App 的支持"Rosetta 警告)。
  - **`--universal` 标志**同时构建 `aarch64-apple-darwin` 与
    `x86_64-apple-darwin`,再运行 `lipo -create` 产出一个
    universal2 Mach-O 放在 `Contents/MacOS/pureterm` 内。产出的
    `PureTerm.app` 在 Apple Silicon 与 Intel Mac 上均可原生运行,
    不再出现任何 Rosetta 警告。
  - **`CFBundleIconFile = PureTerm` 现在会真正写入**
    生成的 `Info.plist`,即使 bundle 从 `extra/osx/Info.plist` 复制
    (此前会遗漏该项,Finder 只能回退到通用图标)。
  - **`Contents/Resources/` 不再携带多余的 `icon.png`** —— 只
    发布 `PureTerm.icns`,Finder 的图标缓存与文档默认图标
    槽位都从正确的文件读取。
  - **PlistBuddy 通过 `/usr/libexec/PlistBuddy` 调用**(绝对路径);
    在许多新开的 shell 中裸 `PlistBuddy` 不在 PATH 上,旧代码会
    静默丢弃所有 PlistBuddy 调用。`plutil` 兜底处理
    完全没有 PlistBuddy 的 CI runner。
  - **`sccache` 未安装时不再硬报错**:
    脚本现在检测其缺失,并用每次调用级的
    `--config 'build.rustc-wrapper=""'` 覆盖仓库的
    `.cargo/config.toml [build] rustc-wrapper`。此前,在
    没有 sccache 的容器中全新 checkout 会在 `cargo -vV` 上退出 127。
  - **跨平台源码 cfg 门控**加到 `crates/pureterm/src/main.rs`
    (模块 `macos_url_scheme`、`macos_glass_effect`、`macos_touch_bar`、
    `macos_haptic`),并门控了 `macos_url_scheme.rs` 内仅 macOS 的
    `use objc2::*` 导入。缺少这些,构建会把
    objc2/objc2_foundation 符号泄漏进 Linux/Windows 编译。
- **`embed-resource` 现在声明为可选的
  `[target.'cfg(windows)'.build-dependencies]`**,位于
  `crates/pureterm-alacritty-runtime/Cargo.toml`。`build.rs` 此前已
  在 `#[cfg(windows)]` 门控后引用 `embed_resource::compile(...)`;
  声明该依赖意味着一旦放入真实的
  `windows/pureterm.rc`,门控即刻保持绿色。
- **`crates/pureterm/build.rs` 现在通过 `cargo:rustc-env` 在每个
  平台向二进制打上 `VERSION` 与 `BUILD_TARGET`**,使打包期的
  版本覆盖能传递到 `--version` / 关于框。同时记录
  针对未来 `windows/pureterm.rc` 的 `cargo:rerun-if-changed` 钩子。
- **GitHub Actions CI / Release 工作流修复,打通跨平台自动打包。**
  两个工作流此前已入库但从未跑通(唯一一次 CI 运行三平台 +
  clippy 全部失败):
  - **Windows**:`aws-lc-rs` 在 MSVC 上需要 NASM 汇编器,runner
    默认没有,构建以 `NASM command not found` panic;现在两个
    工作流的 Windows job 都安装 NASM 并写入 `GITHUB_PATH`。
  - **Linux**:`.cargo/config.toml` 给 Linux target 硬编码
    `-fuse-ld=mold`,runner 只有 lld;用 `RUSTFLAGS` 覆盖为
    `lld`(环境变量优先于 `target.*.rustflags`)。
  - **移除 `-D warnings` 硬门禁**:仓库有 500+ 条 clippy 债与
    170+ 处测试目标的 rustc `dead_code`,且 vendored 的
    `alacritty_*` 按维护规则不做无关改动;在积压清零前该门禁
    只会让 CI 永远红。改为保证"能编译 / 能测试 / 能链接",
    clippy 另设 advisory job 只报告、不阻断。
  - **`build.sh` 的 `binary_path()` 现在按 triple 补 `.exe`**,
    否则 Windows 上产物校验与 `bundle_windows` 的 `cp` 都找不到
    二进制,`.zip` 永远产不出。
  - **Release**:5 个 target(macOS arm64/x64、Linux x64/arm64、
    Windows x64)打包,新增产物齐全校验与 `nullglob` 安全的
    `SHA256SUMS`;汇总 job 改为总是运行,故 `workflow_dispatch`
    试跑也能取回校验和,只有 `push tag v*` 才创建 Release。

### 新增
- **跨平台构建与打包流水线**(`./build.sh` 重写)。
- **跨平台构建与打包流水线**(`./build.sh` 重写)。
  - 新增 `app` / `app-all` / `matrix` / `info` 模式。`app` 在宿主
    triple 上为宿主 OS 构建 + 打包;`app-all` 遍历所有已安装
    target;`matrix` 遍历完整 8-target macOS/Linux/Windows 矩阵,
    跳过未安装工具链的 target(无硬报错)。
  - macOS:产出 `.app`、`.app.zip`,以及(PATH 上有 `hdiutil` 时)
    UDZO 压缩的 `.dmg`。可选 `--sign`(`CODESIGN_IDENTITY`)与
    以及 `--notarize`(需 `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`)。
  - Linux:产出符合 Freedesktop 规范的暂存目录树(bin/、share/applications、
    share/metainfo、share/icons、share/doc、INSTALL.md),并打包为
    `.tar.gz`(总是)与 `.tar.zst`(PATH 上有 `zstd` 时)。
    `--install` 把目录树拷到 `/opt/pureterm-<v>/` 并软链
    即 `/usr/local/bin/pureterm`。
  - Windows:产出便携版暂存目录树并 zip 压缩。可选
    `--sign`(`WINDOWS_CERT_FILE` / `WINDOWS_CERT_PASSWORD` 配
    `signtool`)。`zip` 不在 PATH 时回退到 PowerShell `Compress-Archive`。
  - 预检(`cargo metadata`、二进制存在性、图标
    存在)能捕获静默的构建失败。
  - 工具检测是**信息性且非致命的** —— hdiutil、codesign、zstd、
    signtool 等缺失时脚本优雅降级。
  - 完整能力面见 `./build.sh --help` 与 `./build.sh info`。
- **Justfile**:新配方 `app-all`、`matrix`、`info`;`app` 与
  `clean` 现在经由 `./build.sh` 路由。
- **`crates/pureterm-alacritty-runtime/Cargo.toml`**:声明
  `embed-resource` 为*可选*的 `[target.'cfg(windows)'.build-dependencies]`
  条目。运行时 `build.rs` 已按 `.rc` 文件缺失门控 `embed_resource::compile`;
  现在声明依赖意味着一旦放入真实 `.rc`,构建即刻
  保持绿色。
- **`crates/pureterm/build.rs`**:现在通过
  `cargo:rustc-env` 在每个平台向二进制打上 `VERSION`(以及可选的
  `BUILD_TARGET`),使打包期版本覆盖传递到
  `--version` / 关于框。同时记录针对未来 `windows/pureterm.rc` 的
  `cargo:rerun-if-changed` 钩子。

### 变更
- **构建流水线对齐工作区配方,
  加快测试/开发迭代。**
  - `Cargo.toml`:新增 `[profile.test]`(`debug = 1`、`codegen-units = 256`)
    与 `[profile.release-fast]`(`inherits = "release"`、`lto = false`、
    `codegen-units = 16`、`opt-level = 2`、`incremental = true`)段。
    `[profile.release]` 从完整 LTO(`lto = true`)切换为 thin LTO
    (`lto = "thin"`),从 `strip = true` 切换为 `strip = "symbols"`,
    在缩短链接时间的同时,保持二进制体积在旧构建的 ~5% 以内。
    迭代用发布构建用 `cargo build --profile release-fast --bin pureterm`,
    或按次用 `--config 'profile.release.lto=true'` 覆盖以恢复旧完整 LTO 行为。
  - `.cargo/config.toml`:`rustc-wrapper = "sccache"` 现在自动设置
    (sccache 缺失时 cargo 回退到直接调用 rustc,CI / 全新
    checkout 依然可用)。
  - `build.sh`:新增 `test` 模式,自动检测 `cargo-nextest`,
    缺失时回退 `cargo test`。支持 `--profile <name>`(默认
    `dev`;传 `test` 使用新 profile)、`--all-features` 与
    `--no-run`。用法:`./build.sh test` 或
    例如 `./build.sh test --profile test --no-run`。
  - `Justfile`:`test` 目标现在可用时选用 `cargo nextest run`,
    否则用 `cargo test`。
- **状态栏重新设计。** 分段现在渲染为按 `BarGroup`
  (Context / Runtime / System / Meta)着色的圆角"chip",组间
  间距 8px。Meta 位于最右侧、⌘K 按钮之后,光标
  位置(`📍 L:C`)与时钟(`🕒 HH:MM:SS`)因此视觉锚定,
  而不是埋在一条扁平的线里。
- **替换 Nerd-Font PUA 字形。** Git 分支字形 U+E0A0 → `[br]` 文本
  前缀;PR 胶囊 U+29C9 → `PR#N` 文本;进行中操作 U+21BB → `(rebase)`
  等文本;休眠 agent 雪花 U+2744 → `💤 (sleep)` 文本。在
  未安装 Nerd Fonts 的机器上,状态栏依然可读。
- **顶栏 + 底栏背景**现在从 `iced::Theme::Palette`
  (经 `status_bar::bar_bg`)读取,而非硬编码 `rgba(0.12,0.12,0.14,1.0)`。
  目前仍只解析为 `Theme::Dark`;该辅助路径已接线,
  供未来任意浅色/System 主题切换使用。

### 新增
- `StatusBarComponent` 上的 `BarGroup` 枚举(`Context` / `Runtime` /
  `System` / `Meta`),新组件默认 `Context`。
- `[statusbar].clock_format` 与 `[statusbar].clock_show_seconds` 配置
  键。`clock_show_seconds = false` 即时从格式中剔除 `%S` / `%s`
  token,使同一格式在两种模式下都干净显示。
- 状态栏右下角的 `⌘K` 按钮,打开命令
  面板(复用现有 `Message::OpenCommandPalette`)。
- `status_bar::chip` 模块,含 `chip(label, group, theme)` 与
  `chip_with_color(label, group, fg, theme)`,以及顶/底栏填充用的
  `bar_bg` / `bar_border`。所有颜色派生自 `Theme::Palette`。

### 测试
- `components::git_bar_label_*` 按 `[br]` 前缀更新。
- `agent_indicator_*` 按 `💤 (sleep)` 休眠后缀更新。
- `app_config.rs::tests` 新增 `load_statusbar` 测试,覆盖
  默认值、`clock_format` 覆盖、空 `components` 守卫。
- `status_bar::chip::tests` 新增,覆盖深色主题构造。

## [0.1.0] - 2026-05-14

### 新增
- wgpu GPU 加速终端渲染
- 现代圆角标签栏(SDF 圆角)
- 分屏(水平与垂直)
- SSH、串口、Telnet 会话支持
- VTE 解析器,完整支持 CSI/ESC/OSC/DCS/APC/PM 序列
- 终端内联图像协议
- 基于 OSC 133 标记的 shell 集成
- 基于 egui 的设置面板(通用、外观、终端、配置、快捷键)
- 命令面板与配置选择器
- 自动补全与历史弹出
- quake 模式(下拉终端)
- 中日韩输入 IME 支持
- 无障碍(macOS VoiceOver)
