use std::fmt::Debug;

use crate::alloc::Allocator;
use crate::error::Error;
use crate::sync::Mutex;
use crate::worker::Worker;

#[cfg(target_os = "macos")]
pub mod apple;

#[cfg(target_os = "ios")]
pub mod apple;

#[cfg(target_os = "linux")]
pub mod posix;

pub trait DispatchContext: Debug + Send + Sync + Allocator {
    fn spawn_worker(&mut self, role: &str, args: Vec<String>) -> Result<u64, Error>;
    fn send_message(
        &mut self,
        worker: &Worker,
        message: serde_value::Value,
        callback: Box<dyn FnOnce(serde_value::Value) + Send + 'static>,
    ) -> Result<(), Error>;

    fn mutex(&mut self) -> Result<u64, Error>;
    fn expose_mutex(&mut self, mutex_id: u64) -> Result<(), Error>;

    fn set_worker_state(&mut self, state_id: u128) -> Result<(), Error>;
    fn get_worker_state(&mut self, worker_id: u64) -> Result<Option<u128>, Error>;
}

pub trait DispatchMutex: Send + Sync {
    fn lock(&self);
    fn unlock(&self);
}
