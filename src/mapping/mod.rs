use crate::error::Error;

#[cfg(unix)]
pub mod unix;

#[allow(unused)]
pub trait Mapping: std::fmt::Debug + Send + Sync {
    fn size(&self) -> usize;
    fn ptr(&self) -> *const u8;
    fn ptr_mut(&mut self) -> *mut u8;

    fn is_owner(&self) -> bool;
    unsafe fn set_ownership(&mut self, status: bool);
}

pub fn new_mapping(id: &str, size: usize) -> Result<Box<dyn Mapping>, Error> {
    #[cfg(unix)]
    return Ok(Box::new(unix::new_mapping(id, size)?));

    #[cfg(not(unix))]
    Err(crate::Error::OperationUnsupported)
}

pub fn open_mapping(id: &str) -> Result<Box<dyn Mapping>, Error> {
    #[cfg(unix)]
    return Ok(Box::new(unix::open_mapping(id)?));

    #[cfg(not(unix))]
    Err(crate::Error::OperationUnsupported)
}

// Right now only unix is supported... womp womp
// Maybe Microsoft would go Unix-based one day... but we can only really hope...

