use crate::error::Error;
use super::context;

pub trait Allocator {
    fn alloc(&mut self, size: u32) -> Result<u128, Error>;
    fn free(&mut self, id: u128);

    fn map_id(&mut self, id: u128) -> Option<*mut u8>;
}

pub trait AllocatesTypes: Allocator + Sized {
    fn new_box<T>(&mut self, val: T) -> Result<super::boxed::Box<T>, Error> {
        crate::boxed::Box::new(self, val)
    }

    fn new_arc<T: 'static>(&mut self, val:T) -> Result<super::sync::Arc<T>, Error> {
        crate::sync::Arc::new(self, val)
    }

    fn new_string(&mut self, value: &str) -> Result<super::string::String, Error> {
        crate::string::String::new(self, value)
    }
}

impl<T> AllocatesTypes for T
where 
    T: Allocator
{}

pub fn free(id: &u128) {
    let implementation = *id as u16;
    match implementation {
        0 => {
            // Try getting the context
            let context_id = (id >> 64) as u64;

            let mut context = match context::check_registered_contexts(context_id) {
                Some(context) => context,
                None => { return; }
            };

            context.free(*id);
        }
        _ => {}
    }
}

pub fn map_id(id: &u128) -> Option<*mut u8> {
    let implementation = *id as u16;

    if id.eq(&0_u128) {
        return None;
    }

    match implementation {
        0 => {
            // Try getting the context
            let context_id = (id >> 64) as u64;

            let mut context = match context::check_registered_contexts(context_id) {
                Some(context) => context,
                None => { return None }
            };

            context.map_id(*id)
        }
        _ => {
            None
        }
    }
}

pub fn len(alloc_id: &u128) -> Result<u32, Error> {
    let implementation = *alloc_id as u16;

    if alloc_id.eq(&0_128) {
        return Err(Error::BlockNotFound { allocation_id: *alloc_id })
    }

    match implementation {
        0 => {
            let size = (alloc_id >> 32) as u32;
            if size != 0 {
                Ok(size)
            } else {
                Err(Error::RequestedAllocInfoNotFound { id: *alloc_id })
            }
        }
        _ => {
            Err(Error::RequestedAllocInfoNotFound { id: *alloc_id })
        }
    }

}