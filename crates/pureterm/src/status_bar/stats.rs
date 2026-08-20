//! GAP-24:共享系统状态快照 + 订阅身份。
//!
//! 由单个长驻的发布器任务([`super::publisher`])产出,
//! 由持有统计数据的各个状态栏段(CPU/内存/网络/电池)消费,
//! 消费入口是 `StatusBarComponent::apply_stats`。历史向量来自发布器的
//! 环形缓冲(窗口约 120),每次采样克隆一份——在 1 Hz 下开销很小。

use crate::app_config::StatusBarConfig;

/// 状态栏渲染的全部系统指标在某一时刻的采样。
#[derive(Clone, Debug, Default)]
pub struct StatSnapshot {
    /// 全部核心的聚合 CPU 使用率,范围 0..100。
    pub cpu_pct: f32,
    /// 已使用的常驻内存,单位字节。
    pub mem_used_bytes: u64,
    /// 物理内存总量,单位字节。
    pub mem_total_bytes: u64,
    /// `mem_used_bytes / mem_total_bytes`,范围 0..1——为迷你走势图预计算。
    pub mem_fraction: f32,
    /// 网络发送吞吐量,单位字节/秒(非回环接口求和)。
    pub net_up_bps: u64,
    /// 网络接收吞吐量,单位字节/秒。
    pub net_down_bps: u64,
    /// 存在电池时的电量百分比,范围 0..100。
    pub battery_pct: Option<f32>,
    /// 电池当前是否正在充电。
    pub battery_charging: Option<bool>,

    /// 用于迷你走势图的环形缓冲历史(最近的在末尾)。
    pub cpu_history: Vec<f32>,
    pub mem_history: Vec<f32>,
    pub net_up_history: Vec<f32>,
    pub net_down_history: Vec<f32>,
}

/// 统计订阅的身份标识。`iced` 会在该值变化(Hash+Eq)时
/// 拆除并重建订阅,因此在配置中修改 `refresh_interval_secs`
/// 即会以新的采样节奏重启采样器。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StatsKey(pub StatusBarConfig);
