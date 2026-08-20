//! 同步原语类型。
//!
//! 最重要的是包含一个公平互斥量。

use parking_lot::{Mutex, MutexGuard};

/// 公平互斥量。
///
/// 用一把额外的锁保证:若有线程在等待,它会在同一线程重新加锁前
/// 拿到锁。
pub struct FairMutex<T> {
    /// 数据。
    data: Mutex<T>,
    /// 下一个访问者。
    next: Mutex<()>,
}

impl<T> FairMutex<T> {
    /// 创建新的公平互斥量。
    pub fn new(data: T) -> FairMutex<T> {
        FairMutex { data: Mutex::new(data), next: Mutex::new(()) }
    }

    /// 获取租约以预留互斥锁。
    ///
    /// 这会阻止他人获取终端锁,但若已有他人持有租约则
    /// 阻塞。
    pub fn lease(&self) -> MutexGuard<'_, ()> {
        self.next.lock()
    }

    /// 锁定互斥量。
    pub fn lock(&self) -> MutexGuard<'_, T> {
        // 必须绑定到临时值,否则锁在进入 data.lock()
        // 之前就被释放。
        let _next = self.next.lock();
        self.data.lock()
    }

    /// 不公平地锁定互斥量。
    pub fn lock_unfair(&self) -> MutexGuard<'_, T> {
        self.data.lock()
    }

    /// 不公平地尝试锁定互斥量。
    pub fn try_lock_unfair(&self) -> Option<MutexGuard<'_, T>> {
        self.data.try_lock()
    }
}
