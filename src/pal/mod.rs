use std::fmt::Debug;

use crate::alloc::Allocator;
use crate::worker::Worker;

#[cfg(target_os = "macos")]
pub mod apple;

#[cfg(target_os = "ios")]
pub mod apple;

#[cfg(target_os = "linux")]
pub mod posix;

pub trait DispatchContext: Debug + Send + Sync + Allocator {
    fn spawn_worker(&mut self, role: &str, args: Vec<String>) -> Result<u64, crate::error::Error>;
    fn send_message(
        &mut self,
        worker: &Worker,
        message: serde_value::Value,
        callback: Box<dyn FnOnce(serde_value::Value) + Send + 'static>,
    ) -> Result<(), crate::error::Error>;
}
