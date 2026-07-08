# 编写 PureTerm 插件

> **状态**: T-core-plugin-runtime v3 已落地骨架。本文档描述当前可用的
> 插件接口与示例。host-function 反向调用(`HostFn::Parser` /
> `HostFn::Ipc` 等)需要 `wasm-runtime` feature + v3+1 的 iced_app 集成
> PR;在它落地之前,插件可以注册 manifest + provides,但 host 还不会
> 真实调用插件的导出函数。

PureTerm 插件是一个 `.wasm` 模块 + 一份 `plugin.toml` 清单。host
在启动时扫描 `~/.config/pureterm/plugins/`,加载所有启用的插件,
然后按 `provides` 把它们的导出函数挂到对应的 hook 点。

## 目录结构

```
my-plugin/
├── plugin.toml      # 清单(必填)
├── Cargo.toml       # Rust 包定义
└── src/
    └── lib.rs       # 插件入口
```

## plugin.toml 清单

```toml
[plugin]
name = "my-plugin"             # 唯一名,匹配 [a-z0-9-]+
version = "0.1.0"
description = "一句话简介"
author = "你的名字"
abi_version = 1                # 当前 ABI 版本

[plugin.provides]
# 至少一项。key 是 Provides 能力名,value 是插件里对应的导出函数名。
status_bar_component = "render"   # 状态栏组件
ipc_method = "plugin.my_method"   # JSON-RPC 方法
terminal_parser_hook = "parse"    # PTY 输出预处理

[plugin.config_schema]
# 可选:声明插件读取的配置项。host 用它校验用户的 [[plugins.config]] 块。
refresh_interval_ms = { type = "integer", default = 1000 }
```

## Provides 能力一览

| 能力 | 导出签名 | host 调用时机 |
|------|----------|--------------|
| `status_bar_component` | `fn() -> *const u8` (JSON widget) | 状态栏刷新 tick |
| `ipc_method` | `fn(params_ptr: *const u8, params_len: usize) -> *const u8` | JSON-RPC `plugin.<name>` 到达 |
| `terminal_parser_hook` | `fn(buf_ptr: *const u8, buf_len: usize) -> *const u8` | 每次 PTY 读 |
| `renderer_overlay` | `fn() -> *const u8` | GPU draw 阶段 |
| `config_provider` | `fn() -> *const u8` (JSON) | 配置合并 |
| `sidebar_tab` | `fn() -> *const u8` | 侧栏渲染 |
| `command_palette_item` | `fn() -> *const u8` | 命令面板填充 |
| `tab_context_menu_provider` | `fn() -> *const u8` | 右键菜单构建 |
| `profile_provider` | `fn() -> *const u8` | 启动 profile |

## 编译

需要 `wasm32-wasip1` target:

```bash
rustup target add wasm32-wasip1
cd my-plugin
cargo build --release --target wasm32-wasip1
cp ../../../target/wasm32-wasip1/release/my_plugin.wasm .
```

## 安装

```bash
pureterm plugin install ./my_plugin.wasm
# 校验清单 + 复制到 ~/.config/pureterm/plugins/my_plugin/
pureterm plugin list
pureterm plugin enable my-plugin
pureterm plugin disable my-plugin
pureterm plugin uninstall my-plugin
```

## Parser hook:OSC 1337;PureTerm-Plugin-<name>;<payload>

除了 host 主动调用插件的导出函数外,插件还可以反过来从 PTY 字节流
接收消息。任何形如下面的 OSC 序列:

```
\x1b]1337;PureTerm-Plugin-my-plugin;hello world\x07
```

都会被 `event_loop::scan_plugin_osc` 解析成
`Event::PluginOsc { name: "my-plugin", payload: b"hello world" }`,
host 据此把 payload 路由到对应插件的 `HostFn::Parser` 回调。

shell 端典型用法(让脚本通知插件某事件发生):

```bash
notify_plugin() {
  printf '\e]1337;PureTerm-Plugin-%s;%s\a' "$1" "$2"
}
notify_plugin "my-plugin" "build-done"
```

## 示例

仓库内有两个参考实现:

- [`examples/plugins/cpu-status/`](../../examples/plugins/cpu-status/) ——
  `StatusBarComponent` 能力,显示 CPU 占用百分比。
- [`examples/plugins/ipc-hello/`](../../examples/plugins/ipc-hello/) ——
  `IpcMethod` 能力,实现 `plugin.hello(name)` JSON-RPC 方法。

复制其中一个作为起点,改 `name` / `provides` / 导出函数即可。

## ABI 兼容性

- `abi_version = 1` 是当前唯一支持的版本。host 在加载时检查
  manifest 的 `abi_version`,不匹配则拒绝加载并记日志。
- 字符串传递约定:length-prefixed(4 字节小端 u32)+ UTF-8 字节。
  host 提供 `__free_string(ptr, len)` 释放插件泄漏的字符串。
- 后续 ABI 演进遵循 SemVer:minor 增量只加新能力,major 增量
  允许删除旧能力。

## 限制(v3 当前)

- **wasmtime 链接默认关闭**:需要 `cargo build --features wasm-runtime`
  才会拉入 wasmtime(50+ MB)。不开启时 `WasmPluginLoader::load`
  返回 `Err("wasmtime is not wired …")`,GUI 优雅降级。
- **host-function 反向调用未接入**:`HostFn::Parser` /
  `HostFn::Ipc` 等回调的签名已定义,但 iced_app 还没把活动
  `TerminalContext` / JSON-RPC router 注入到 linker。这是 v3+1
  的下一个 PR 工作。
- **静态插件优先**:`StaticPluginLoader` 始终可用,适合不需要
  沙箱的第一方插件(直接编译进 binary)。