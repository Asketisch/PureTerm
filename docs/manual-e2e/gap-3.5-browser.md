# GAP-3.5 内置 Browser 面板 —— 手动 pixel-smoke runbook

> CI 自动 harness(`crates/pureterm/src/browser_address_bar_e2e.rs` + `iced_app.rs` /
> `keymap.rs` 单测)已覆盖**数据/逻辑不变量**:`⌘L`→`FocusAddressBar`、`⌘⇧L`(双形)
> →`SplitWeb` 键位解析;`normalize_url_input`(域名/scheme/搜索/`about:blank`/空);
> `spawn_web_pane` 的 split→insert→active 序列(Web pane + Arc 身份 + split_tree 两叶);
> `AddressBarConfirm` 的 normalize→`load_url` 在 wry WebView 未 realize 前即更新 ctx url。
> CI 无法证明**像素/交互**:WKWebView 真渲染、滚动、地址栏 overlay 聚焦/IME、
> z-order 与 terminal Shader 共存。本页是那条人工置信层。**非 CI gate**。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app(改了 pureterm 后必须)
open dist/PureTerm.app
```

> 用户跑的是 `dist/PureTerm.app`,`cargo build` 不够 —— 必须重打包 .app 才见效
> (见 memory `app-runs-from-dist-bundle`)。

---

## 默认键位(GAP-3.5)

| 动作 | 键 |
|---|---|
| 分出新 Web pane 并聚焦地址栏 | `⌘⇧L` |
| 聚焦地址栏(导航活跃 Web pane;无则新建) | `⌘L` |
| (CLI 平价)分出 Web pane | `pureterm pane split-web --url <url>` |

> 后退/前进本轮**未做**:`⌘[`/`⌘]` 仍是切 pane 焦点,`⌘{`/`⌘}` 仍是切 tab —— 与
> 浏览器后退/前进有意冲突,defer 到后续。
> Linux 本轮**未做原生嵌入**:web pane 在 Linux 上点击走「系统默认浏览器打开」兜底
> (macOS 之外 `try_realize` 为 no-op)。

地址栏输入归一化(`normalize_url_input`,无依赖):含 `://` → 原样;裸域名(无空格、
含 `.`)→ 前缀 `https://`;`about:blank` 这类无 `://` 的 bare scheme → 原样;否则 →
Google 搜索(space→`+`)。回车(`AddressBarConfirm`)加载。

---

## DoD 验证(在 pane 中打开 https://example.com,可渲染 + 滚动)

1. **⌘⇧L 开 Web pane + 地址栏** — 在终端 pane 按 `⌘⇧L`。
   - [ ] 分出一个新 pane(右侧),带浅蓝活跃边框。
   - [ ] 顶部居中弹出地址栏 overlay(蓝框),输入框**已聚焦**(光标在)。

2. **键入 URL 加载** — 输入 `example.com` 回车。
   - [ ] overlay 关闭。
   - [ ] 新 pane 渲染 `https://example.com`(归一化加了 `https://`)的页面 ——
     **DoD 核心**:可见 Example Domain 文字 + 能用滚轮/触控板滚动。

3. **裸域名 / scheme / 搜索归一化** — 用 `⌘L` 各试一次:
   - [ ] `example.com` → 加载 `https://example.com`。
   - [ ] `http://localhost:8080` → 原样(不断掉 scheme)。
   - [ ] `how do i exit vim` → 跳 Google 搜索。

4. **⌘L 编辑当前 URL(prefill)** — 已有 Web pane 时按 `⌘L`。
   - [ ] 地址栏预填该 pane 当前 URL(全选,键入即替换)—— 像 browser 的「编辑当前地址」。
   - [ ] 改 URL 回车 → 同一 pane 导航到新页(不新建 pane)。

5. **⌘L 无 Web pane → 新建** — 关掉所有 Web pane 后按 `⌘L`,输入 URL 回车。
   - [ ] split 出一个新 Web pane 并加载(target=None 分支,等价 ⌘⇧L 带一个 URL)。

6. **Esc 关闭** — 地址栏打开时按 `Esc`。
   - [ ] overlay 关闭,不加载任何东西。

7. **键位不泄漏到 PTY** — 地址栏打开时键入字符。
   - [ ] 字符进地址栏,**不**进终端 PTY(overlay guard 生效)。

8. **切 tab 隐藏 / 切回恢复** — 多 tab 时另一 tab 有 Web pane,切走再切回。
   - [ ] 非活跃 tab 的 WebView 隐藏;切回重新显示、**不重新加载**(状态保留)。

9. **CLI 平价** — 另开终端:
   ```bash
   pureterm pane split-web --url https://example.com
   ```
   - [ ] 与 `⌘⇧L` + 输入 URL 等价(分出 Web pane 并加载)。

10. **拖拽不撕裂(R2)+ z-order(R3)** — 拖动窗口边框 resize。
    - [ ] WebView 跟随 resize,无撕裂/错位;不遮挡 tab bar;与 terminal pane 共存
      时两者都不被对方 Shader 盖掉。

## 已知限制(非 DoD 阻塞)

- **无后退/前进**:`⌘[`/`⌘]` 仍是 pane 焦点切换 —— 浏览器后退/前进有意 defer。
- **Linux 无原生嵌入**:`webkit2gtk` 嵌入未做;web pane 在 Linux 走 open-in-browser 兜底。
- ** detached 窗口**:WebView 嵌入取「活动窗口」content NSView(MVP),多 detached 窗口
  的精确匹配留待后续(NSWindow identifier 标签)。

## 签字

每次相关代码改动后跑一遍,勾选 + 日期:

- [ ] 2026-06-25 ⌘⇧L / ⌘L 地址栏 overlay 闭环 —— *(待执行;GUI 验证延后。CI 已绿:
  `browser_address_bar_e2e`(spawn 序列 + load_url)+ `normalize_url_input` 单测 +
  `keymap` ⌘L/⌘⇧L 解析,共 442 单测)*
