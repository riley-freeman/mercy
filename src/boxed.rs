use crate::{
    alloc::{self, Allocator, HasAllocId},
    error::Error,
};
use std::{
    fmt::{Debug, Display},
    marker::PhantomData,
    mem, ptr,
};

pub struct Box<T> {
    id: u128,
    _phantom: PhantomData<T>,
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
            _phantom: PhantomData,
        })
    }
}

impl<T> HasAllocId for Box<T> {
    type Inner = T;
    fn alloc_id(&self) -> u128 {
        self.id
    }
}

impl<T> AsRef<T> for Box<T> {
    fn as_ref(&self) -> &T {
        let ptr = alloc::map_id(&self.id).unwrap();
        unsafe { &*(ptr as *const T) }
    }
}

impl<T> AsMut<T> for Box<T> {
    fn as_mut(&mut self) -> &mut T {
        let ptr = alloc::map_id(&self.id).unwrap();
        unsafe { &mut *(ptr as *mut T) }
    }
}

impl<T: Clone> Clone for Box<T> {
    fn clone(&self) -> Self {
        let new_id = alloc::realloc(&self.id, mem::size_of::<T>() as _).unwrap();
        let new_ref: *mut T = unsafe { &mut *(alloc::map_id(&new_id).unwrap() as *mut T) };

        let old_ref = unsafe { &*(alloc::map_id(&self.id).unwrap() as *const T) };
        unsafe { *new_ref = old_ref.clone() };

        Box::<T> {
            id: new_id,
            _phantom: PhantomData,
        }
    }
}

impl<T: Debug> Debug for Box<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match alloc::map_id(&self.id) {
            Ok(ptr) => unsafe {
                let r = &*(ptr as *mut T);
                r.fmt(f)
            },
            Err(_) => std::fmt::Result::Err(std::fmt::Error),
        }
    }
}

impl<T: Display> Display for Box<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match alloc::map_id(&self.id) {
            Ok(ptr) => unsafe {
                let r = &*(ptr as *mut T);
                r.fmt(f)
            },
            Err(_) => std::fmt::Result::Err(std::fmt::Error),
        }
    }
}
