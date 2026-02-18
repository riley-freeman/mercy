use std::fmt::Debug;

use crate::alloc::Allocator;

#[cfg(target_os = "macos")]
pub mod apple;

#[cfg(target_os = "ios")]
pub mod apple;

pub trait DispatchContext: Debug + Send + Sync + Allocator {}
