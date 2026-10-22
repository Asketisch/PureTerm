//! 启动耗时追踪(可选)。
//!
//! 设 `PURETERM_TRACE_STARTUP=1` 后,应用在关键里程碑向 stderr 打印
//! `PURETERM_STARTUP <label> <ms>`,其中 `<ms>` 是自进程入口
//! (`main()` 第一行)以来的毫秒数。
//!
//! 用途:把「启动慢」从主观感受变成可回归的数字。发布前的性能
//! 基线测试(`tests/e2e/test_startup.py`)解析这些行,一旦某段
//! 明显回归即可在 CI 中被发现。
//!
//! 未启用时开销近似为零:只多一次 `OnceLock<bool>` 读取。

use std::sync::OnceLock;
use std::time::Instant;

/// 进程入口时刻。`main()` 的第一行调用 [`init`] 固定它。
static START: OnceLock<Instant> = OnceLock::new();

/// 是否启用追踪。
static ENABLED: OnceLock<bool> = OnceLock::new();

/// 固定启动计时起点并读取开关。必须在 `main()` 最开始调用。
pub fn init() {
    let _ = START.get_or_init(Instant::now);
    let _ = ENABLED.get_or_init(|| {
        std::env::var_os("PURETERM_TRACE_STARTUP").is_some_and(|v| !v.is_empty() && v != "0")
    });
}

/// 追踪是否启用。
#[inline]
pub fn enabled() -> bool {
    ENABLED.get().copied().unwrap_or(false)
}

/// 记录一个里程碑。`label` 应为稳定的 snake_case 标识。
///
/// 未启用时立即返回,不产生任何 I/O。
pub fn mark(label: &str) {
    if !enabled() {
        return;
    }
    let elapsed = START
        .get()
        .map(|s| s.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or_default();
    eprintln!("PURETERM_STARTUP {label} {elapsed:.1}");
}

/// 记录一个里程碑,并额外打印自上一个里程碑以来的增量。
///
/// 增量比累计值更适合定位「哪一段变慢」——它直接给出该段耗时。
pub fn mark_delta(label: &str) {
    if !enabled() {
        return;
    }
    static LAST: OnceLock<parking_lot::Mutex<Option<Instant>>> = OnceLock::new();
    let last = LAST.get_or_init(|| parking_lot::Mutex::new(None));
    let mut guard = last.lock();
    let delta = guard
        .map(|prev| prev.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or_default();
    *guard = Some(Instant::now());
    let elapsed = START
        .get()
        .map(|s| s.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or_default();
    eprintln!("PURETERM_STARTUP {label} {elapsed:.1} +{delta:.1}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_is_silent_when_disabled() {
        // 未设 `PURETERM_TRACE_STARTUP` 时 `enabled()` 为 false,
        // `mark` 不应 panic,也不应要求 START 已初始化。
        if !enabled() {
            mark("unit-test-noop");
            mark_delta("unit-test-noop");
        }
    }
}
