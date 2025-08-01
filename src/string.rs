use core::{fmt, slice};
use std::{fmt::Display, ops::Deref};

use libc::strlen;

use crate::{alloc::{self, Allocator}, error::Error};


pub struct String {
    id: u128,
}

impl Drop for String {
    fn drop(&mut self) {
        alloc::free(&self.id);
    }
}

impl String {
    pub fn new(allocator: &mut dyn Allocator, value: &str) -> Result<Self, Error> {
        let len = value.len();
        let id = allocator.alloc(len as u32 + 1)?;

        // Write the (only byte) data into the buffer
        let ptr = match allocator.map_id(id) {
            Some(ptr) => ptr,
            // Ok mf... im drunk as fuck... I KNOW. there is a better way to do this shit... one that looks better... but take that up with the judge bitch.
            None => return Err(Error::BlockNotFound { allocation_id: id })
        };

        unsafe {
            libc::memcpy(ptr as _, value.as_ptr() as _, len);
            libc::memcpy(ptr.byte_add(len) as _ , &0 as *const _ as _, 1);
        }

        Ok(String {id})
    }

    pub fn push_char(&mut self, allocator: &mut dyn Allocator, c: char) -> Result<(), Error> {
        self.push_str(allocator, &c.to_string())
    }

    pub fn push_str(&mut self, allocator: &mut dyn Allocator, string: &str)  -> Result<(), Error> {
        // Create the old string
        let og_str = unsafe {
            let ptr = alloc::map_id(&self.id).unwrap();
            let len = libc::strlen(ptr as _);
            str::from_utf8_unchecked(slice::from_raw_parts(ptr, len))
        };

        let new_str = format!("{}{}", og_str, string);
        let new_id = allocator.alloc(new_str.len() as u32 + 1)?;

        // Write the new data
        unsafe {
            let ptr = allocator.map_id(new_id).unwrap();
            let len = new_str.len();
            libc::memcpy(ptr as _, new_str.as_ptr() as _, len);
            libc::memcpy(ptr.byte_add(len)as _, &0 as *const _ as _, 1);
        }

        // Free that old shit lowkey
        alloc::free(&self.id);
        self.id = new_id;

        Ok(())
    }
}

impl AsRef<str>  for String {
    fn as_ref(&self) -> &str {
        unsafe {
            let ptr = alloc::map_id(&self.id).unwrap();
            let len = strlen(ptr as _);
            let slice = slice::from_raw_parts(ptr, len);
            str::from_utf8_unchecked(slice)
        }
    }
}

impl Deref for String {
    type Target = str; 
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl Display for String {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}



