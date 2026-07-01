# GAP-5.2 SOCKS5 SSH 隧道 —— 手动 E2E runbook

> CI 已覆盖**逻辑不变量**（`cargo test -p pureterm-daemon`，69 绿）：
> - TOFU 决策（`decide_host_key`：no-store 接受 / Match 接受 / Unknown 记录+接受 / Mismatch 拒绝）
> - `known_hosts.rs`：`format_host_pattern`、Unknown 追加后复连 Match、Mismatch 拒绝且文件不变、Match 不写、缺文件开空、不同端口互不冲突
> - `persist.rs`：round-trip、缺字段默认（port 22 / remote_dns true）、缺文件/损坏 → None、`TunnelConfig` 不含 `password`、version 字段
> - 既有 tunnel：状态串契约、id 唯一、list/stop/provide_auth、SOCKS5 地址/端口格式
>
> CI 无法证明**真实 SSH 行为**：经真实网关的 SOCKS5 转发、远端 DNS、agent 转发往返、TOFU 在真实 host key 下、
> 跨 daemon 重启的持久化恢复。本页是那条人工置信层。**非 CI gate**（GAP-5.6/5.4 先例）。

## 前置

```bash
./build.sh app          # 重打包 dist/PureTerm.app —— tunnel.rs/known_hosts/persist 是 daemon 端,
                        # 必须重打包 .app 才让新 puretermd 生效(cargo build 不够,见 memory app-runs-from-dist-bundle)
open dist/PureTerm.app
```

> daemon 改了，必须用**新** puretermd。GUI 启动会按 `exe_mtime` 检测旧 daemon 并重启;若没自动重启,
> 手动 `pkill -f puretermd` 让 GUI 的 spawn-on-demand 拉起新实例。

数据文件路径(macOS):`~/Library/Application Support/pureterm/daemon/{known_hosts,tunnels.json}`。

另开一个终端跟状态:

```bash
pureterm ssh tunnel-list     # 看 tunnel.list(状态/错误)
```

---

## 场景 1 —— 基础 SOCKS5 转发(核心 DoD)

1. 起隧道(把 `gw` 换成你能 ssh 通的网关):
   ```bash
   pureterm ssh tunnel alice@gw --local-port 1080
   ```
   - 首次会提示认证(identity 文件用 `-i`、密码交互输入、或 `-A` 走 agent)。
2. `pureterm ssh tunnel-list` → 该 tunnel `status: "connected"`。
3. 经隧道访问只有网关能解析的内网主机:
   ```bash
   curl --socks5-hostname 127.0.0.1:1080 http://intranet-host/
   ```
   **预期**:返回内网页面(直连本机访问不到)。

## 场景 2 —— 远端 DNS(atyp=0x03 域名透传)

1. 隧道连着(场景 1)。
2. **必须**用 `--socks5-hostname`(不是 `--socks5`):
   ```bash
   curl --socks5-hostname 127.0.0.1:1080 http://only-resolvable-from-gateway/
   ```
   **预期**:成功。`--socks5-hostname` 让 curl 把域名**原样**交给代理(`atyp=3`),由 SSH 网关解析 DNS
   (`tunnel.rs::read_dst_addr` 的 `0x03` 分支);`--socks5` 会本机先解析 → 解析不到 → 失败(这是预期对照)。

## 场景 3 —— agent 转发往返(GAP-5.4 Track B 即隧道)

1. 本机先 `ssh-add` 一个 key。
2. `pureterm ssh tunnel alice@gw --local-port 1080 -A`(`-A`/`--forward-agent`)。
3. 隧道起来后,**在网关上**用 agent(经隧道访问网关能 reach 的第三台):
   ```bash
   # 经隧道 SOCKS5 让网关上的 ssh 用转发的 agent 免密连第三台
   ```
   或最简验证:网关侧跑一个需要 agent 的操作,确认 `SSH_AUTH_SOCK` 已被服务端 `auth-agent@openssh.com` 通道桥回。
   **预期**:网关能用本机 agent 的 key(`tunnel.rs::server_channel_open_agent_forward` 字节泵 + `agent_forward` 请求生效)。
   > 这是 GAP-5.4 Track B 的真机 E2E(此前一直标"待补")—— 隧道路径自带,无需额外开关。

## 场景 4 —— TOFU host-key(首连记录 / 复连通过 / 换 key 拒绝)

1. 删旧记录干净起步:`rm -f ~/Library/Application\ Support/pureterm/daemon/known_hosts`。
2. `pureterm ssh tunnel alice@gw --local-port 1080` → 首连成功。
3. `cat ~/Library/Application\ Support/pureterm/daemon/known_hosts` → **预期**多了一行
   `gw ssh-ed25519 <base64>`(端口 22 裸 host;非 22 是 `[gw]:port`)。
4. `pureterm ssh tunnel-stop <id>`,再起一条同 host 的隧道 → **复连静默通过**(不再写新行;`Match`)。
5. **换 key 模拟中间人**:手动改 known_hosts 里那行的 base64 为别的值(或网关真换了 host key),
   再起隧道 → **预期** `tunnel-list` 显示该 tunnel `status: "failed"`,`error` 含 `mismatch` 且列出
   `known=SHA256:… live=SHA256:…` 两个指纹;**known_hosts 文件不被改写**(Mismatch 只读)。

## 场景 5 —— 持久化(跨 daemon 重启仅恢复配置)

1. 用 **key 认证** 起一条隧道:`pureterm ssh tunnel alice@gw --local-port 1080 -i ~/.ssh/id_ed25519`。
2. `cat ~/Library/Application\ Support/pureterm/daemon/tunnels.json` → **预期**列了这条 tunnel 的配置
   (host/user/port/identity_file/local_port/remote_dns/forward_agent;**无 password 字段**)。
3. `pureterm ssh tunnel-stop <id>` → tunnels.json 中该条移除(或随 stop 更新)。
4. 再起一条(key 认证),然后**重启 daemon**:`pkill -f puretermd`(GUI 会自动拉起新实例)。
5. 新 daemon 起来后 `pureterm ssh tunnel-list` → **预期**那条隧道以 `status: "stopped"` 出现
   (config-only 恢复,**不**自动重连)。
6. 用相同参数再 `pureterm ssh tunnel ...` 重新 start → 成功连上(配置没丢)。
7. **密码隧道对照**:用密码起一条,stop,重启 daemon → 恢复为 `stopped`;重新 start 时回到 `need_auth`
   (密码**未**持久化,无泄漏)。

---

## 通过判据(flip 🚧→✅)

> **flip 状态(2026-06-27):✅ 已 flip(用户授权"代码闭环 + 单测"先行,GAP-5.6/5.4 先例)。**
> 逻辑不变量由 69 个 daemon 单测覆盖(TOFU 决策 + known_hosts + persist round-trip + 既有 tunnel)。
> 下面 5 个**真实网关**交互复选框本次 flip 前**未逐条人工跑过**,作为**非阻塞 follow-up**。任一场景未来
> 若证伪,回滚此处 ✅。

- [ ] 场景 1:经隧道 `curl --socks5-hostname` 访问内网主机成功。
- [ ] 场景 2:`--socks5-hostname` 远端解析生效(`--socks5` 对照失败)。
- [ ] 场景 3:`-A` 后网关能用本机 agent key。
- [ ] 场景 4:首连写行 / 复连 Match / 换 key `failed`+mismatch 指纹 + 文件不变。
- [ ] 场景 5:重启后 `stopped` 恢复 + 重连成功 + 密码隧道回 `need_auth`(无泄漏)。

**已执行(2026-06-27)**:`docs/done.md` GAP-5.2、`FEATURES.md` 加 GAP-5.2 段、
`docs/features/GAP-5.2-socks5-tunnel/spec.md` ✅ + 删除陈旧 agent-forwarding 项 + gui-sessions.json→tunnels.json 修正 ——
同提交 flip(memory `closing-inprogress-gap-to-done`)。

## 已知边界(非 DoD)

- **不自动重连**:重启 daemon 后隧道恢复为 `stopped`,需手动 `tunnel.start`。自动重连 + 退避 reaper 留作后续
  (避免 GAP-5.6 同款 hammering 抖动网关 + 密码隧道静默落 NeedAuth)。
- **隔离 known_hosts**:PureTerm 用 `data_dir()/known_hosts`,**不**读写用户 `~/.ssh/known_hosts`。代价:
  已在 OpenSSH 信任的 host 在隧道里需重新 TOFU 一次。行格式仍为 OpenSSH,便于将来迁移。
- **仅字面 host 匹配**:不解析 wildcard / hashed-host(`|1|…`)格式。MVP 只写/匹配 `[host]:port` 字面量。
- **HTTP CONNECT 未实现**:标题写"SOCKS5/HTTP CONNECT",实际只有 SOCKS5 CONNECT(`fast-socks5` 未引入,
  手写)。HTTP CONNECT 不在本 gap 范围。
- **密码不持久化**:`TunnelConfig` 无 `password` 字段(对齐 SSH-profile 不存密钥)→ 密码隧道重启后回 `need_auth`。
