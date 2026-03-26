use syn::token::Super;

use super::context;
use crate::error::Error;

pub trait Allocator {
    fn alloc(&mut self, size: u32) -> Result<u128, Error>;
    fn free(&mut self, id: u128);

    fn map_id(&mut self, id: u128) -> Result<*mut u8, Error>;
}

pub trait HasAllocId {
    fn alloc_id(&self) -> u128;
}

pub trait HasInner {
    type Inner: Clone;
    fn clone_inner(&self) -> Self::Inner;
    fn set_inner(&mut self, value: Self::Inner);
}

pub trait AllocatesTypes: Allocator + Sized {
    fn new_box<T>(&mut self, val: T) -> Result<super::boxed::Box<T>, Error> {
        crate::boxed::Box::new(self, val)
    }

    fn new_arc<T: 'static>(&mut self, val: T) -> Result<super::sync::Arc<T>, Error> {
        crate::sync::Arc::new(self, val)
    }

    fn new_string(&mut self, value: &str) -> Result<super::string::String, Error> {
        crate::string::String::new(self, value)
    }

    fn new_vec<T>(&mut self) -> Result<super::vec::Vec<T>, Error> {
        crate::vec::Vec::new(self)
    }

    fn new_vec_with_capacity<T>(&mut self, capacity: usize) -> Result<super::vec::Vec<T>, Error> {
        crate::vec::Vec::with_capacity(self, capacity)
    }
}

impl<T> AllocatesTypes for T where T: Allocator {}

pub fn free(id: &u128) {
    let implementation = *id as u16;
    match implementation {
        0 => {
            // Try getting the context
            let family_id = (id >> 64) as u64;

            let mut context = match context::check_registered_contexts(family_id) {
                Some(context) => context,
                None => {
                    return;
                }
            };

            context.free(*id);
        }
        _ => {}
    }
}

pub fn map_id(id: &u128) -> Result<*mut u8, Error> {
    let implementation = *id as u16;
    println!("[DEBUG] [map_id] implementation: {}", implementation);

    if id.eq(&0_u128) {
        return Err(Error::BlockNotFound { allocation_id: *id });
    }

    match implementation {
        0 => {
            // Try getting the context
            let family_id = (id >> 64) as u64;

            let mut context = match context::check_registered_contexts(family_id) {
                Some(context) => context,
                None => return Err(Error::RequestedContextNotFound { id: *id }),
            };

            context.map_id(*id)
        }
        _ => Err(Error::OperationUnsupported),
    }
}

pub fn len(alloc_id: &u128) -> Result<u32, Error> {
    let implementation = *alloc_id as u16;
    println!("[DEBUG] implementation: {}", implementation);

    if alloc_id.eq(&0_128) {
        return Err(Error::BlockNotFound {
            allocation_id: *alloc_id,
        });
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
        _ => Err(Error::RequestedAllocInfoNotFound { id: *alloc_id }),
    }
}

// Not doing documentaion right now.. this doesnt free anything, it just allocates a new
// whatever with what originally allocated the original ID... Just look at the code
// If you have no idea what i'm talmbout fr ong.
pub fn realloc(id: &u128, size: u32) -> Result<u128, Error> {
    let implementation = *id as u16;
    println!("[DEBUG] [realloc] implementation: {}", implementation);

    if id.eq(&0_u128) {
        return Err(Error::OperationUnsupported);
    }

    match implementation {
        0 => {
            // Try getting the context
            let family_id = (id >> 64) as u64;
            let mut context = context::check_registered_contexts(family_id)
                .ok_or(Error::RequestedContextNotFound { id: *id })?;

            context.alloc(size)
        }
        _ => Err(Error::OperationUnsupported),
    }
}

#[test]
fn the_realloc_test() {
    use crate::alloc::{Allocator, free, realloc};
    use crate::context::ContextBuilder;

    // Create a new context
    let id = String::from("crayon.mercy.test.alloc.realloc");

    println!("Creating context with family ID: {}", id);
    tracing::debug!("Creating context with family ID: {}", id);
    ContextBuilder::new(&id)
        .main(|res| {
            let mut context = res.unwrap();

            // Allocate a buffer
            let one = context.alloc(64).unwrap();
            let two = realloc(&one, 64).unwrap();

            // Should be the same context
            assert_eq!((one >> 64) as u64, (two >> 64) as u64);
            // Should be the same size
            assert_eq!((one >> 32) as u32, (two >> 32) as u32);
            // Should different alloc ids
            assert_ne!((one >> 16) as u16, (two >> 16) as u16);
            // Should be the same implementation
            assert_eq!(one as u16, two as u16);

            free(&one);
            free(&two);
        })
        .start();
}
