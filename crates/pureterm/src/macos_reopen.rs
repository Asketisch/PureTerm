use iced::futures::SinkExt;
use iced::Subscription;

#[derive(Debug, Clone)]
pub struct DockReopen;

pub fn dock_reopen_subscription() -> Subscription<DockReopen> {
    Subscription::run(dock_reopen_stream)
}

fn dock_reopen_stream() -> impl iced::futures::Stream<Item = DockReopen> {
    iced::stream::channel(1, move |mut output: iced::futures::channel::mpsc::Sender<DockReopen>| async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            if is_active_with_no_visible_windows() {
                let _ = output.send(DockReopen).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    })
}

fn is_active_with_no_visible_windows() -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);

    if !app.isActive() {
        return false;
    }

    let windows = app.windows();
    let count = windows.count();
    if count == 0 {
        return true;
    }

    for i in 0..count {
        let window: objc2::rc::Retained<objc2_app_kit::NSWindow> = windows.objectAtIndex(i);
        if window.isVisible() {
            return false;
        }
    }
    true
}
