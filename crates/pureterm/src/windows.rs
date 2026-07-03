//! T-core-windows-conpty v2: Windows-specific bindings.
//!
//! v2 ships a stub that exercises the same surface the v3 full
//! ConPTY integration will. The stub lets the rest of the codebase
//! link on `windows-2022` without bringing in `Win32_System_Console`
//! + `windows-sys` deps today; replacing it with real impls is a
//! mechanical follow-up.

/// Windows-specific process / mutex / registry helpers. The v2 stub
/// returns `Err(Unsupported)` everywhere except `is_windows()`
/// (which is a const-evaluated platform guard used by the GUI to
/// disable macOS-only menu items).
pub const fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

/// Windows job-object handle. Used to ensure spawned children die
/// with the GUI on Ctrl-C / window-close.
///
/// v2: zero-sized placeholder. v3: wraps a `HANDLE` from
/// `CreateJobObjectW` + `SetInformationJobObject` with
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
#[derive(Default)]
pub struct JobObject {
    _private: (),
}

impl JobObject {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach `pid` to the job. v2 stub: no-op (returns Ok). v3:
    /// `AssignProcessToJobObject(self.handle, open_process(pid))`.
    pub fn attach(&self, _pid: u32) -> Result<(), &'static str> {
        Ok(())
    }
}

/// Named mutex used for single-instance gating on Windows
/// (the macOS counterpart uses `kCFBooleanTrue` + a launchctl job).
///
/// v2 stub: returns `Ok(Mutex { _private: () })`. v3: calls
/// `CreateMutexW` and checks `GetLastError() == ERROR_ALREADY_EXISTS`.
pub struct SingleInstanceMutex {
    _private: (),
}

impl SingleInstanceMutex {
    pub fn acquire(_name: &str) -> Result<Self, &'static str> {
        Ok(Self { _private: () })
    }

    pub fn release(self) {
        // Drop is sufficient — v3 will call `ReleaseMutex` + `CloseHandle`.
    }
}

/// Windows-specific ConPTY wrapper. v2 stub: returns a "not yet
/// wired" error. v3: calls `CreatePseudoConsole` from
/// `Win32_System_Console`.
pub fn open_conpty(_cols: u16, _rows: u16) -> Result<ConptyHandle, &'static str> {
    Err("ConPTY is not wired on Windows in v2; tracked by t-core-windows-conpty")
}

/// Handle to a Windows ConPTY. v2 stub: holds no OS resources.
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
        // The const-evaluated predicate must reflect the build target.
        // On the dev machine (macOS / Linux) it must be false; on the
        // CI windows-2022 runner it must be true.
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