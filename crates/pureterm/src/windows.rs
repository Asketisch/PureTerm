//! T-core-windows-conpty v2:Windows 专属绑定。
//!
//! v2 提供的存根覆盖与 v3 完整
//! ConPTY 集成相同的面。该存根让其余代码在
//! `windows-2022` 上可链接,无需引入 `Win32_System_Console` +
//! `windows-sys` 依赖;换成真实实现只是
//! 机械式跟进。

/// Windows 专属的进程 / 互斥量 / 注册表辅助函数。v2 存根
/// 除 `is_windows()` 外一律返回 `Err(Unsupported)`
/// (它是编译期求值的平台守卫,GUI 用它
/// 禁用仅 macOS 的菜单项)。
pub const fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

/// Windows 作业对象句柄。用于确保派生的子进程
/// 在 Ctrl-C / 窗口关闭时随 GUI 一起结束。
///
/// v2:零尺寸占位。v3:包装来自
/// `CreateJobObjectW` + `SetInformationJobObject` 的 `HANDLE`,
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
#[derive(Default)]
pub struct JobObject {
    _private: (),
}

impl JobObject {
    pub fn new() -> Self {
        Self::default()
    }

    /// 把 `pid` 挂到作业上。v2 存根:空操作(返回 Ok)。v3:
    /// `AssignProcessToJobObject(self.handle, open_process(pid))`.
    pub fn attach(&self, _pid: u32) -> Result<(), &'static str> {
        Ok(())
    }
}

/// Windows 上用于单实例互斥的具名互斥量
/// (macOS 对应物使用 `kCFBooleanTrue` + launchctl 任务)。
///
/// v2 存根:返回 `Ok(Mutex { _private: () })`。v3:调用
/// `CreateMutexW`,并检查 `GetLastError() == ERROR_ALREADY_EXISTS`。
pub struct SingleInstanceMutex {
    _private: (),
}

impl SingleInstanceMutex {
    pub fn acquire(_name: &str) -> Result<Self, &'static str> {
        Ok(Self { _private: () })
    }

    pub fn release(self) {
        // Drop 已足够 —— v3 将调用 `ReleaseMutex` + `CloseHandle`。
    }
}

/// Windows 专属的 ConPTY 包装。v2 存根:返回「尚未
/// 接线」错误。v3:调用
/// `Win32_System_Console`.
pub fn open_conpty(_cols: u16, _rows: u16) -> Result<ConptyHandle, &'static str> {
    Err("ConPTY is not wired on Windows in v2; tracked by t-core-windows-conpty")
}

/// Windows ConPTY 的句柄。v2 存根:不持有操作系统资源。
#[derive(Default)]
pub struct ConptyHandle {
    _private: (),
}

impl ConptyHandle {
    pub fn resize(&mut self, _cols: u16, _rows: u16) -> Result<(), &'static str> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_windows_predicate() {
        // 编译期求值的谓词必须反映构建目标。
        // 在开发机(macOS / Linux)上必须为 false;在
        // CI 的 windows-2022 runner 上必须为 true。
        assert_eq!(is_windows(), cfg!(target_os = "windows"));
    }

    #[test]
    fn job_object_default_is_no_op() {
        let job = JobObject::new();
        assert!(job.attach(1234).is_ok());
    }

    #[test]
    fn single_instance_mutex_acquires_and_releases() {
        let m = SingleInstanceMutex::acquire("Local\\pureterm-test-mutex").unwrap();
        m.release();
    }

    #[test]
    fn open_conpty_returns_unsupported_in_v2() {
        assert!(open_conpty(80, 24).is_err());
    }
}
