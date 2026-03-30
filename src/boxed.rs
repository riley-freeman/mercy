use crate::{
    alloc::{self, Allocator, HasAllocId, HasInner},
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
        let type_name = std::any::type_name::<T>();
        let id = allocator.alloc(size)?;
        
        println!("[DEBUG] [Box::new] ID: {}, Size: {}, Type: {}", id, size, type_name);
        let ptr = allocator.map_id(id).unwrap();
        println!("[DEBUG] [Box::new] Pointer mapped: {:?}", ptr);

        // Safely write to uninitialized memory without dropping old garbage
        unsafe { std::ptr::write(ptr as *mut T, val) };
        println!("[DEBUG] [Box::new] Wrote val!");

        Ok(Box::<T> {
            id,
            _phantom: PhantomData,
        })
    }
}

impl<T: Clone> HasInner for Box<T> {
    type Inner = T;
    fn clone_inner(&self) -> Self::Inner {
        let ptr = alloc::map_id(&self.id).unwrap();
        unsafe { &*(ptr as *const T) }.clone()
    }
    fn set_inner(&mut self, value: Self::Inner) {
        let ptr = alloc::map_id(&self.id).unwrap();
        unsafe { &mut *(ptr as *mut T) }.clone_from(&value);
    }
}

impl<T: Clone> HasAllocId for Box<T> {
    // type Inner = T;
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
        let size = mem::size_of::<T>() as _;
        let type_name = std::any::type_name::<T>();
        println!("[DEBUG] [Box::clone] Reallocating ID from {}, Size: {}, Type: {}", self.id, size, type_name);
        
        let new_id = alloc::realloc(&self.id, size).unwrap();
        
        println!("[DEBUG] [Box::clone] Mapping new ID: {}", new_id);
        let new_ptr = alloc::map_id(&new_id).unwrap() as *mut T;
        
        println!("[DEBUG] [Box::clone] Mapping old ID: {}", self.id);
        let old_ptr = alloc::map_id(&self.id).unwrap() as *const T;
        
        println!("[DEBUG] [Box::clone] Calling clone on old_ref");
        let cloned_val = unsafe { &*old_ptr }.clone();
        
        println!("[DEBUG] [Box::clone] Writing to new_ref");
        unsafe { std::ptr::write(new_ptr, cloned_val) };
        println!("[DEBUG] [Box::clone] Finished write!");

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
