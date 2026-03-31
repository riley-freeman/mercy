use libc::{pthread_mutex_lock, pthread_mutex_t, pthread_mutex_unlock};

use crate::pal::DispatchMutex;

pub struct PosixMutex {
    pub(crate) posix_mutex: pthread_mutex_t,
}

unsafe impl Send for PosixMutex {}
unsafe impl Sync for PosixMutex {}

impl DispatchMutex for PosixMutex {
    fn lock(&self) {
        unsafe { pthread_mutex_lock(&self.posix_mutex as *const _ as _) };
    }
    fn unlock(&self) {
        unsafe { pthread_mutex_unlock(&self.posix_mutex as *const _ as _) };
    }
}
