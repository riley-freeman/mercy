use std::any::{Any, TypeId};
use std::fmt::{Debug, Display};
use std::marker::PhantomData;

use std::mem;
use std::ops::Deref;
use std::sync::atomic::AtomicU32;

use crate::alloc::{self, Allocator};
use crate::error::Error;
use crate::header::typeid_to_u64;

pub struct Arc<T: ?Sized> {
    data_id:    u128,
    rcs_id:     u128,
    type_id:    u64,
    _phantom: PhantomData<T>
}

// Atomic Reference Counting Reference Counts
struct ArcReferenceCounts {
    count: AtomicU32,
    weaks: AtomicU32,
}

impl<T: ?Sized> Drop for Arc<T> {
    fn drop(&mut self) {
        unsafe { self.decrement_strong_count().unwrap() };
    }
}

impl<T> Arc<T> 
where
    T: 'static
{
    pub fn new(allocator: &mut dyn Allocator, val: T) -> Result<Arc<T>, Error> {
        let size = mem::size_of::<T>() as _;
        let rcs_size = mem::size_of::<ArcReferenceCounts>() as _;

        let data_id = allocator.alloc(size)?;
        let rcs_id = allocator.alloc(rcs_size)?;

        // Get pointers to the memory
        let data = unsafe { &mut *(allocator.map_id(data_id).unwrap() as *mut T) };
        let rcs = unsafe { &mut *(allocator.map_id(rcs_id).unwrap() as *mut ArcReferenceCounts) };

        *data = val;
        *rcs = ArcReferenceCounts {
            count: AtomicU32::new(1),
            weaks: AtomicU32::new(0),
        };

        Ok(Arc::<T> {
            data_id,
            rcs_id,
            type_id: typeid_to_u64(TypeId::of::<T>()),
            _phantom: PhantomData
        })
    }
}

impl <T: ?Sized> Arc<T> {
    pub unsafe fn increment_strong_count(&mut self) -> Result<(), Error> {
        unsafe { self.increment_strong_count_backend() }
    }

    unsafe fn increment_strong_count_backend(&self) -> Result<(), Error> {
        match alloc::map_id(&self.rcs_id) {
            Some(raw) => {
                let rcs = unsafe {&mut *(raw as *mut ArcReferenceCounts)};
                rcs.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            },
            None => Err(Error::BlockNotFound { allocation_id: self.rcs_id })
        }
    }


    pub unsafe fn decrement_strong_count(&mut self) -> Result<(), Error> {
        match alloc::map_id(&self.rcs_id) {
            Some(raw) => {
                let rcs = unsafe {&mut *(raw as *mut ArcReferenceCounts)};
                let prev = rcs.count.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);

                if prev <= 1 {
                    // Deallocate the data block
                    alloc::free(&self.data_id);
                    self.data_id = 0_u128;
                    if rcs.weaks.load(std::sync::atomic::Ordering::Acquire) <= 0 {
                        // Deallocate the reference block
                        alloc::free(&self.rcs_id);
                        self.rcs_id = 0_u128;
                    }
                }

                Ok(())
            },
            None => Err(Error::BlockNotFound { allocation_id: self.rcs_id })
        }
    }

}

impl<T: Debug> Debug for Arc<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match alloc::map_id(&self.data_id) {
            Some(ptr) => unsafe {
                T::fmt(&*(ptr as *mut T), f)
            }
            None => write!(f, "mercy::sync::Arc: ERROR. Data Block not found!")
        }
    }
}

impl<T: Display> Display for Arc<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match alloc::map_id(&self.data_id) {
            Some(ptr) => unsafe {
                T::fmt(&*(ptr as *mut T), f)
            }
            None => write!(f, "mercy::sync::Arc: ERROR. Data Block not found!")
        }
    }
}

impl<T> Clone for Arc<T> {
    fn clone(&self) -> Self {
        // Update the reference count
        unsafe { self.increment_strong_count_backend().unwrap(); }

        Arc {
            data_id: self.data_id,
            rcs_id: self.rcs_id,
            type_id: self.type_id,
            _phantom: PhantomData::<T>,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        unsafe {
            match self.decrement_strong_count() {
                Ok (_) => {},
                Err(Error::BlockNotFound { allocation_id: _ }) => {},
                Err(e) => panic!("{:?}", e),
            }
        }

        self.data_id = source.data_id;
        self.rcs_id = source.rcs_id;
        self.type_id = source.type_id;

        // Update the reference count
        unsafe { self.increment_strong_count_backend().unwrap(); }
    }
}

impl<T> AsRef<T> for Arc<T> {
    fn as_ref(&self) -> &T {
        let data = unsafe { &*(alloc::map_id(&self.data_id).unwrap() as *mut T) };
        data
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let data = unsafe { &*(alloc::map_id(&self.data_id).unwrap() as *mut T) };
        data
    }
}


impl<T> Arc<T>
where
    T: ?Sized + Any + Send + Sync,
{
    pub fn downcast<U: 'static>(self) -> Result<Arc<U>, Arc<T>> {
        let req_id = typeid_to_u64(TypeId::of::<U>());
        if self.type_id == req_id {
            // SAFETY: We just checked the type ID.
            let me = mem::ManuallyDrop::new(self);
            let arc_u = Arc {
                data_id: me.data_id,
                rcs_id: me.rcs_id,
                type_id: me.type_id,
                _phantom: PhantomData,
            };
            Ok(arc_u)
        } else {
            Err(self)
        }
    }
}


impl<T> From<Arc<T>> for Arc<dyn Any + Send + Sync>
where
    T: Any + Send + Sync + 'static,
{
    fn from(arc: Arc<T>) -> Self {
        let arc = mem::ManuallyDrop::new(arc); // prevent double-drop
        Arc {
            data_id: arc.data_id,
            rcs_id: arc.rcs_id,
            type_id: arc.type_id,
            _phantom: PhantomData,
        }
    }
}

