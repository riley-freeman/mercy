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