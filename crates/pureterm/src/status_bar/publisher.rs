//! GAP-24: 全局唯一的系统状态发布器。
//!
//! 由单个长驻的 `iced::stream::channel` 任务持有唯一的 `sysinfo::System` /
//! `Networks` 与 `battery::Manager`,按配置的采样周期取样并保留
//! 约 120 个样本的环形缓冲区,每次采样发出一个 `Message::StatusBarStats`。
//! 这参考了主流终端单例发布器的模式(一个
//! 定时器投喂所有订阅者),也沿用仓库内 `config_watcher.rs` 的订阅模式。
//!
//! 发布器自身的休眠循环是唯一采样者;`StatusBarTick` 不
//! 触发采样——将指标节奏(与性能相关)与 git
//! 子进程节奏(与 UX 相关)解耦,并保证每次
//! 渲染间隔最多只采样一次。

use std::collections::VecDeque;
use std::time::Duration;

use iced::futures::SinkExt;
use iced::Subscription;

use crate::iced_app::Message;

use super::stats::{StatSnapshot, StatsKey};

const HISTORY_CAP: usize = 120;

/// 按配置的采样周期,通过订阅流式下发每个 `StatSnapshot`。
pub fn stats_subscription(cfg: crate::app_config::StatusBarConfig) -> Subscription<Message> {
    Subscription::run_with(StatsKey(cfg), |key: &StatsKey| stats_stream(key.0.clone()))
}

fn stats_stream(cfg: crate::app_config::StatusBarConfig) -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        4,
        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
        let interval = Duration::from_secs(cfg.refresh_interval_secs.max(1));
        let dt = interval.as_secs_f32().max(1.0);

        let mut sys = sysinfo::System::new_all();
        let mut nets = sysinfo::Networks::new_with_refreshed_list();
        // 注意:`battery::Manager` 不能在此处构造——在 macOS 上它持有一个
        // `Rc<IoKitManager>`(!Send),跨 `.await` 持有会令
        // 该 future 变为 !Send,因此改为在下方同步 IIFE 内每次采样重新创建。

        // 预热 CPU,使首次休眠后的差值有意义(sysinfo 需要
        // 两次刷新才能算出发用量差值)。
        sys.refresh_cpu_usage();

        let mut cpu_hist: VecDeque<f32> = VecDeque::with_capacity(HISTORY_CAP);
        let mut mem_hist: VecDeque<f32> = VecDeque::with_capacity(HISTORY_CAP);
        let mut up_hist: VecDeque<f32> = VecDeque::with_capacity(HISTORY_CAP);
        let mut dn_hist: VecDeque<f32> = VecDeque::with_capacity(HISTORY_CAP);
        let mut last_net: (u64, u64) = (0, 0);

        // 首次正式采样前先休眠一个间隔。
        tokio::time::sleep(interval).await;

        loop {
            sys.refresh_memory();
            sys.refresh_cpu_usage();
            nets.refresh_list();

            // CPU:各核心使用率的平均值。
            let cpu_pct = if sys.cpus().is_empty() {
                0.0
            } else {
                sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / sys.cpus().len() as f32
            };

            let mem_used = sys.used_memory();
            let mem_total = sys.total_memory();
            let mem_fraction = if mem_total > 0 {
                mem_used as f32 / mem_total as f32
            } else {
                0.0
            };

            // 网络字节/秒:由累计计数器差值换算(跳过回环接口)。
            let (cur_up, cur_dn): (u64, u64) = nets
                .iter()
                .filter(|(name, _)| !is_loopback(name))
                .map(|(_, data)| (data.total_transmitted(), data.total_received()))
                .fold((0, 0), |(a, b), (x, y)| (a + x, b + y));
            let up_bps = cur_up.saturating_sub(last_net.0) as f32 / dt;
            let dn_bps = cur_dn.saturating_sub(last_net.1) as f32 / dt;
            last_net = (cur_up, cur_dn);

            // 电池:使用作用域 IIFE,让 !Send 的 Manager(macOS 上的
            // `Rc<IoKitManager>`)在 `.send`/`sleep` 的 await 之前被释放,
            // 从而保持 future 为 Send。每次采样重新创建(按配置节奏开销很小)。
            let (battery_pct, battery_charging) = (|| {
                let mgr = battery::Manager::new().ok()?;
                let mut it = mgr.batteries().ok()?;
                let b = it.next()?.ok()?;
                Some((
                    b.state_of_charge().get::<battery::units::ratio::ratio>() * 100.0,
                    b.state() == battery::State::Charging,
                ))
            })()
            .unzip();

            push_cap(&mut cpu_hist, cpu_pct);
            push_cap(&mut mem_hist, mem_fraction * 100.0);
            push_cap(&mut up_hist, up_bps);
            push_cap(&mut dn_hist, dn_bps);

            let snap = StatSnapshot {
                cpu_pct,
                mem_used_bytes: mem_used,
                mem_total_bytes: mem_total,
                mem_fraction,
                net_up_bps: up_bps as u64,
                net_down_bps: dn_bps as u64,
                battery_pct,
                battery_charging,
                cpu_history: cpu_hist.iter().copied().collect(),
                mem_history: mem_hist.iter().copied().collect(),
                net_up_history: up_hist.iter().copied().collect(),
                net_down_history: dn_hist.iter().copied().collect(),
            };

            let _ = output.send(Message::StatusBarStats(snap)).await;
            tokio::time::sleep(interval).await;
        }
    })
}

fn push_cap(q: &mut VecDeque<f32>, v: f32) {
    if q.len() >= HISTORY_CAP {
        q.pop_front();
    }
    q.push_back(v);
}

/// 启发式回环接口过滤器(覆盖 `lo`、`lo0`、`loopback`)。
fn is_loopback(name: &str) -> bool {
    name == "lo" || name == "lo0" || name == "loopback" || name.starts_with("lo")
}
