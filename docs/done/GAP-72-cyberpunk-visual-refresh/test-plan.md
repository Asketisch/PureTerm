# GAP-72 手动验收清单

> 自动化覆盖:预设解析 / 调色板等价性 / ANSI 注入 / 像素级 cyberpunk 断言。
> 下面是需要**人眼**确认的部分(发光强度、配色观感、交互流畅度)。

## 准备

```bash
cargo build --release --bin pureterm
cp ~/.config/pureterm/alacritty.toml /tmp/pureterm-backup.toml   # 先备份
```

在 `~/.config/pureterm/alacritty.toml` 追加:

```toml
[appearance]
preset = "cyberpunk"
terminal_palette = "follow"
```

重启 PureTerm。

## 1. classic 逐像素回归(默认路径)

1. 把 `preset` 改回 `"classic"`(或删掉整段)并重启。
2. 与改动前的截图对比:标签栏 / 侧栏 / 状态栏 / 浮层应**基本一致**。
3. 已知差异(有意为之):
   - 浮层面板背景被**统一**成一种(`overlay_bg`),原先几个浮层各用各的深灰;
   - 浮层描边统一为 1px 半透明白;
   - 状态栏 chip 配色与改造前逐值一致(单测已锁)。

## 2. cyberpunk 观感

- [ ] 标签栏:激活标签有霓虹青底 + 底部 2px 发光霓虹条,顶边有青→紫→品红渐变细线;
- [ ] 左栏:激活工作区左侧有青色竖条,分组标题带霓虹短线;
- [ ] 终端区:背景更深(近黑靛蓝),ANSI 色明显更"霓虹";
- [ ] 右栏:分段控件激活项有青色下划线,搜索框聚焦时有青色发光边框;
- [ ] 状态栏:chip 按 Context(青)/ Runtime(品红)/ System(紫)/ Meta(灰蓝)分组着色;
- [ ] 命令面板(⌘⇧P):面板底色更深,边框为霓虹青半透明,整块有外发光;
- [ ] 正文文字在深色背景上**不刺眼**、对比度 ≥ 4.5:1;
- [ ] classic 下**不出现**霓虹条 / 强调色标题(该装饰由 `neon_active_tab` 开关
      门控,仅 cyberpunk 开启)。

## 3. 终端 ANSI 色生效

```sh
printf '\e[31mRED\e[0m \e[32mGREEN\e[0m \e[36mCYAN\e[0m\n'
printf '\e[38;5;196m256-color-index\e[0m\n'
```

- [ ] RED 应为 `#FF3B6B` 系(而不是经典 `#CC0000`);
- [ ] `38;5;196` 索引色同样变成霓虹红(证明 0..15 槽位注入生效);
- [ ] 选区高亮为霓虹青半透明;搜索命中为琥珀色。

## 4. 逃生阀

在 cyberpunk 预设下设 `terminal_palette = "user"` 并重启:

- [ ] 外壳仍是赛博朋克;
- [ ] 终端前景/背景回到 `[colors.primary]` 的值,ANSI 16 色回到经典板。

## 5. 设置窗口

- [ ] ⌘, 打开设置 → Appearance → 顶部「主题 / 外观预设」下拉;
- [ ] 选 Cyberpunk → **设置窗口当场换肤**(不必重启);
- [ ] 保存后主应用重启也保持 cyberpunk。

## 6. 性能

```bash
python3 scripts/bench_startup.py --rounds 5
```

- [ ] 启动耗时与基线差 < 5%(preset 只在启动解析一次);
- [ ] 交互中无掉帧(发光仅 3 处,不应有可感知卡顿);
- [ ] 终端跑 `yes` 10 分钟,帧率与改动前一致。

## 回滚

把 `[appearance]` 段的 `preset` 改回 `"classic"`(或删掉整段)并重启 —— 无需重装、无需清缓存。
