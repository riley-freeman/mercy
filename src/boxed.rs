use crate::{alloc::{self, Allocator}, error::Error};
use std::{marker::PhantomData, mem};


pub struct Box<T> {
    id: u128,
    _phantom: PhantomData<T>
}

impl<T> Drop for Box<T> {
    fn drop(&mut self) {
        alloc::free(&self.id);
    }
}

impl<T> Box<T> {
    pub fn new(allocator: &mut dyn Allocator, val: T) -> Result<Box<T>, Error> {
        let size = mem::size_of::<T>() as _;
        let id = allocator.alloc(size)?;

        // Copy the memory into the block
        let rf = unsafe { &mut *(allocator.map_id(id).unwrap() as *mut T) };
        *rf = val;

        Ok(Box::<T> {
            id,
            _phantom: PhantomData
        })
    }


    pub fn as_ref(&self) -> Option<&T> {
        alloc::map_id(&self.id).map(|ptr| unsafe {&*(ptr as *mut T)})
    }

    pub fn as_mut(&mut self) -> Option<&mut T> {
        alloc::map_id(&self.id).map(|ptr| unsafe {&mut *(ptr as *mut T)})
    }
}