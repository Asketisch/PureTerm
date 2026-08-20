//! GAP-09:macOS 桌面通知——基于 UserNotifications 框架的弹窗提示。
//!
//! 与 `macos_dock_badge.rs` / `macos_eyedropper.rs` 相同:一个小的 objc2 垫片,
//! 其所有调用点都在 iced 主线程。我们在启动时请求
//! `.alert | .sound | .badge` 授权(尽力而为,
//! 即发即忘——完成回调为空操作),随后 `post(title,
//! body) 调度立即投递的 `UNNotificationRequest`。
//!
//! macOS 细节:若授权被拒绝、或 `post` 运行时授权尚未完成,
//! 请求会被*丢弃*——这可以接受(应用内响铃、
//! 状态栏计数与 Dock 角标不受影响;它们不经过
//! UNUserNotificationCenter)。弹窗显示需要正确的 bundle id
//! (见 GAP-09 计划:通过 `./build.sh app` 重新打包)。
//!
//! 版本说明:固定使用 objc2-user-notifications **0.3.x**(objc2 0.6 线),
//! 而非 wry 传递引入的 0.2.x——0.2.x 绑定 objc2 0.5,其
//! `NSString`/`Retained` 与本 crate 的 objc2 0.6 类型不同。

use std::sync::atomic::{AtomicU64, Ordering};

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::ClassType;
use objc2_foundation::{NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound,
    UNUserNotificationCenter,
};

/// 进程内单调递增的通知标识计数器。稳定/空的标识
/// 会让 macOS 合并或覆盖先前已投递的通知;
/// 每次投递使用新 id 可保证每条弹窗互不相同。(v1 暂不需要
/// 稳定 id——尚无点击通知聚焦的接线。)
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// 在启动时请求一次通知授权(`.alert | .sound | .badge`)。
/// 幂等且尽力而为:完成回调不做任何事;
/// 授权结果落定前发出的弹窗会被系统直接丢弃。
/// 授权被拒仅意味着弹窗不显示——调用点只依据
/// `[notification].desktop_notifications` 与焦点/可见性逻辑做门控,
/// 绝不依赖(异步、未知的)授权结果。
pub fn request_authorization() {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let options = UNAuthorizationOptions::Alert
        | UNAuthorizationOptions::Sound
        | UNAuthorizationOptions::Badge;
    // 空的 `Fn` 块:授权即发即忘。block2 要求 `Fn`
    // (而非 `FnOnce`);空闭包天然是 `Fn`。
    let completion = RcBlock::new(move |_granted: Bool, _err: *mut NSError| {});
    center.requestAuthorizationWithOptions_completionHandler(options, &completion);
}

/// 立即投递一条桌面通知(标题 + 正文)。标题显示为
/// 提醒标题,正文显示为副标题行。若应用
/// 缺少/被拒授权则无副作用(系统丢弃请求)。
pub fn post(title: &str, body: &str) {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    // 每次投递使用新标识,保证每条弹窗都是独立的已投递
    // 通知(空/稳定的 id 会被合并)。
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let identifier = NSString::from_str(&format!("pureterm-{id}"));
    // `as_super()` 把 `&UNMutableNotificationContent` 引用向上转型为
    // 构造器所需的 `&UNNotificationContent`。`None` 触发器 ⇒
    // 立即投递。
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &identifier,
        content.as_super(),
        None,
    );
    // 完成回调可选;我们不需要其结果(此处的错误意味着
    // 授权被拒——表现为"弹窗未出现",
    // 属于预期)。
    center.addNotificationRequest_withCompletionHandler(&request, None);
}
