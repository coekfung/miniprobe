use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, RwLock};

#[derive(Debug)]
pub struct SharedOwnable<T> {
    data: RwLock<T>,
    owned: AtomicBool,
    owner_notify: Notify,
}

pub struct ReadGuard<'a, T> {
    guard: tokio::sync::RwLockReadGuard<'a, T>,
}

pub struct WriteGuard<'a, T> {
    guard: tokio::sync::RwLockWriteGuard<'a, T>,
}

pub struct OwnershipGuard<T> {
    value: Arc<SharedOwnable<T>>,
}

#[allow(dead_code)]
impl<T> SharedOwnable<T> {
    pub fn new(value: T) -> Arc<Self> {
        Arc::new(Self {
            data: RwLock::new(value),
            owned: AtomicBool::new(false),
            owner_notify: Notify::new(),
        })
    }

    /// Read value, will not be blocked by ownership
    pub async fn read(&self) -> ReadGuard<'_, T> {
        let guard = self.data.read().await;
        ReadGuard { guard }
    }

    /// Try to get ownership, return None if already owned
    pub fn try_own(self: &Arc<Self>) -> Option<OwnershipGuard<T>> {
        if self
            .owned
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(OwnershipGuard {
                value: self.clone(),
            })
        } else {
            None
        }
    }

    /// Get ownership, will wait until available
    pub async fn own(self: &Arc<Self>) -> OwnershipGuard<T> {
        loop {
            if let Some(guard) = self.try_own() {
                return guard;
            }
            self.owner_notify.notified().await;
        }
    }

    /// Check if it is owned
    pub fn is_owned(&self) -> bool {
        self.owned.load(Ordering::Acquire)
    }

    async fn write(&self) -> WriteGuard<'_, T> {
        let guard = self.data.write().await;
        WriteGuard { guard }
    }
}

impl<T> std::ops::Deref for ReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T> std::ops::Deref for WriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T> std::ops::DerefMut for WriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

#[allow(dead_code)]
impl<T> OwnershipGuard<T> {
    pub async fn read(&self) -> ReadGuard<'_, T> {
        self.value.read().await
    }

    pub async fn write(&self) -> WriteGuard<'_, T> {
        self.value.write().await
    }
}

impl<T> Drop for OwnershipGuard<T> {
    fn drop(&mut self) {
        self.value.owned.store(false, Ordering::Release);
        self.value.owner_notify.notify_one();
    }
}

unsafe impl<T: Send> Send for SharedOwnable<T> {}
unsafe impl<T: Send + Sync> Sync for SharedOwnable<T> {}
