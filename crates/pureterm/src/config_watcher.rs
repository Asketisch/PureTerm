use std::path::PathBuf;

use iced::futures::SinkExt;
use iced::Subscription;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug, Clone)]
pub struct ConfigChanged;

pub fn watch(config_path: PathBuf) -> Subscription<ConfigChanged> {
    Subscription::run_with(config_path, config_watch_stream)
}

fn config_watch_stream(config_path: &PathBuf) -> impl iced::futures::Stream<Item = ConfigChanged> {
    let config_path = config_path.clone();
    iced::stream::channel(4, move |mut output: iced::futures::channel::mpsc::Sender<ConfigChanged>| async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(4);

        let parent = config_path.parent().unwrap_or(&config_path).to_path_buf();
        let file_name = config_path.file_name().map(|n| n.to_os_string());

        let _watcher = {
            let tx = tx.clone();
            let file_name = file_name.clone();
            let watcher = RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        if !matches!(
                            event.kind,
                            EventKind::Create(_) | EventKind::Modify(_)
                        ) {
                            return;
                        }
                        let matches_file = file_name.as_ref().map_or(true, |name| {
                            event.paths.iter().any(|p| {
                                p.file_name().map_or(false, |n| n == name.as_os_str())
                            })
                        });
                        if matches_file {
                            let _ = tx.try_send(());
                        }
                    }
                },
                notify::Config::default(),
            );

            match watcher {
                Ok(mut w) => {
                    let _ = w.watch(&parent, RecursiveMode::NonRecursive);
                    Some(w)
                }
                Err(_) => None,
            }
        };

        loop {
            if rx.recv().await.is_none() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            while rx.try_recv().is_ok() {}

            let _ = output.send(ConfigChanged).await;
        }

        std::future::pending::<()>().await;
    })
}
