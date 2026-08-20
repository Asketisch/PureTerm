use std::thread::{Builder, JoinHandle};

/// 类似 `thread::spawn`,但带 `name` 参数。
pub fn spawn_named<F, T, S>(name: S, f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
    S: Into<String>,
{
    Builder::new().name(name.into()).spawn(f).expect("thread spawn works")
}
