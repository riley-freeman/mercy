use crate::{alloc::{self, Allocator}, error::Error, rec::SmartRef};
use std::{fmt::{Debug, Display}, marker::PhantomData, mem};


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

    pub fn map(&self) -> Option<SmartRef<T>> {
        SmartRef::new(self.id).ok()
    }
}


impl<T: Debug> Debug for Box<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match alloc::map_id(&self.id) {
            Some(ptr) => unsafe {
                let r = &*(ptr as *mut T);
                r.fmt(f)
            }
            None => std::fmt::Result::Err(std::fmt::Error)
        }
    }
}

impl <T: Display> Display for Box<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match alloc::map_id(&self.id) {
            Some(ptr) => unsafe {
                let r = &*(ptr as *mut T);
                r.fmt(f)
            }
            None => std::fmt::Result::Err(std::fmt::Error)
        }
    }
}