use crate::{
    alloc::{self, Allocator, HasAllocId},
    error::Error,
};
use core::slice;
use std::{
    fmt::Debug,
    marker::PhantomData,
    mem,
    ops::{Deref, DerefMut},
    ptr,
};

pub struct Vec<T> {
    id: u128,
    len: usize,
    capacity: usize,
    _phantom: PhantomData<T>,
}

impl<T> Drop for Vec<T> {
    fn drop(&mut self) {
        if self.capacity > 0 {
            let ptr = alloc::map_id(&self.id).unwrap() as *mut T;
            for i in 0..self.len {
                unsafe {
                    ptr::drop_in_place(ptr.add(i));
                }
            }
            alloc::free(&self.id);
        } else {
            // Free the initial allocation used to get the alloc_id
            alloc::free(&self.id);
        }
    }
}

impl<T> Vec<T> {
    pub fn new(allocator: &mut dyn Allocator) -> Result<Self, Error> {
        Self::with_capacity(allocator, 0)
    }

    pub fn with_capacity(allocator: &mut dyn Allocator, capacity: usize) -> Result<Self, Error> {
        let size = if capacity == 0 {
            1
        } else {
            (capacity * mem::size_of::<T>()) as u32
        };

        let id = allocator.alloc(size)?;

        Ok(Vec {
            id,
            len: 0,
            capacity,
            _phantom: PhantomData,
        })
    }

    pub fn push(&mut self, value: T) -> Result<(), Error> {
        if self.len == self.capacity {
            let new_capacity = if self.capacity == 0 {
                1
            } else {
                self.capacity * 2
            };
            let new_size = (new_capacity * mem::size_of::<T>()) as u32;

            let new_id = alloc::realloc(&self.id, new_size)?;

            let new_ptr = alloc::map_id(&new_id).unwrap() as *mut T;

            if self.len > 0 {
                let old_ptr = alloc::map_id(&self.id).unwrap() as *const T;
                unsafe {
                    ptr::copy_nonoverlapping(old_ptr, new_ptr, self.len);
                }
            }

            alloc::free(&self.id);
            self.id = new_id;
            self.capacity = new_capacity;
        }

        let ptr = alloc::map_id(&self.id).unwrap() as *mut T;
        unsafe {
            ptr::write(ptr.add(self.len), value);
        }
        self.len += 1;

        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            let ptr = alloc::map_id(&self.id).unwrap() as *mut T;
            Some(unsafe { ptr::read(ptr.add(self.len)) })
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[T] {
        if self.capacity == 0 || self.len == 0 {
            &[]
        } else {
            let ptr = alloc::map_id(&self.id).unwrap() as *const T;
            unsafe { slice::from_raw_parts(ptr, self.len) }
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        if self.capacity == 0 || self.len == 0 {
            &mut []
        } else {
            let ptr = alloc::map_id(&self.id).unwrap() as *mut T;
            unsafe { slice::from_raw_parts_mut(ptr, self.len) }
        }
    }
}

impl<T: Clone> HasAllocId for Vec<T> {
    // type Inner = std::vec::Vec<T>;
    fn alloc_id(&self) -> u128 {
        self.id
    }
}

impl<T> Deref for Vec<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for Vec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T: Debug> Debug for Vec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

impl<T: Clone> Clone for Vec<T> {
    fn clone(&self) -> Self {
        let size = if self.capacity == 0 {
            1
        } else {
            (self.capacity * mem::size_of::<T>()) as u32
        };

        let new_id = alloc::realloc(&self.id, size).unwrap();

        let new_ptr = alloc::map_id(&new_id).unwrap() as *mut T;

        if self.len > 0 {
            let old_ptr = alloc::map_id(&self.id).unwrap() as *const T;
            for i in 0..self.len {
                unsafe {
                    let val = (*old_ptr.add(i)).clone();
                    ptr::write(new_ptr.add(i), val);
                }
            }
        }

        Vec {
            id: new_id,
            len: self.len,
            capacity: self.capacity,
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::alloc::AllocatesTypes;
    use crate::context::ContextBuilder;

    #[test]
    fn the_vec_test() {
        let id = String::from("crayon.mercy.test.vec");

        println!("Opening context with id: {}", id);
        tracing::debug!("Opening context with id: {}", id);
        ContextBuilder::new(&id)
            .main(|mut context| {
                let mut vec = context.new_vec::<u32>().unwrap();

                assert_eq!(vec.len(), 0);
                assert_eq!(vec.capacity(), 0);

                vec.push(10).unwrap();
                vec.push(20).unwrap();
                vec.push(30).unwrap();

                assert_eq!(vec.len(), 3);
                assert!(vec.capacity() >= 3);

                assert_eq!(vec[0], 10);
                assert_eq!(vec[1], 20);
                assert_eq!(vec[2], 30);

                let mut vec_clone = vec.clone();
                assert_eq!(vec_clone.len(), 3);

                vec_clone.push(40).unwrap();
                assert_eq!(vec_clone.len(), 4);
                assert_eq!(vec.len(), 3);

                assert_eq!(vec.pop(), Some(30));
                assert_eq!(vec.pop(), Some(20));
                assert_eq!(vec.pop(), Some(10));
                assert_eq!(vec.pop(), None);

                assert_eq!(vec_clone.pop(), Some(40));
            })
            .start();
    }
}
